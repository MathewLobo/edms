//! Bookmark ↔ Collection merge (Mathew, 2026-09-08).
//!
//! The old System 1 (arbitrary named `bookmarks.folder` values acting as
//! pseudo-collections — `POST /bookmarks/:collection/create`) is retired.
//! "Active bookmarks" is now always the *draft state of whichever real
//! Collection (System 2, `storage/collections/{name}.sqlite`) is currently
//! loaded* — tracked in `state.active_collection`. The flow:
//!
//! 1. Collection tab creates an empty, named collection (`/collections/create`
//!    — unchanged, in view_catalog.rs).
//! 2. This module's `ws_load_collection` loads it into `active`, resolving
//!    full endpoint data (not just EIDs) via the same bookmark-subscription
//!    machinery test_view.rs already had.
//! 3. History → bookmark (test_view.rs's `save_bookmark`/`ws_add_from_history_to_bookmark`)
//!    adds tested endpoints into `active` — but now requires a collection
//!    to already be loaded (`require_active_collection` below), since a
//!    bookmark with nothing to save into doesn't make sense in this model.
//! 4. Per-endpoint, in bookmark view: `save_to_collection` persists a
//!    bookmarked endpoint's membership (EID + timestamp only) into the
//!    loaded collection's file; `remove_from_collection` drops that
//!    membership but leaves it bookmarked (per Mathew: "let it just be
//!    bookmarked then not part of the collection"). Deleting from bookmark
//!    view entirely is test_view.rs's existing `ws_delete_from_bookmark`.

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use crate::{db, events::ServerEvent, handlers::view_catalog::open_existing_collection_membership, state::AppState};

/// Shared gate for anything that adds to `active` — per Mathew
/// (2026-09-08): "There has to be a collection loaded into active.
/// Otherwise, a popup alert should come up saying, Load a collection
/// first." Returns the loaded collection's name on success.
pub(crate) async fn require_active_collection(
    state: &AppState,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    state.active_collection.read().await.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "no_collection_loaded",
                "message": "Load a collection first"
            })),
        )
    })
}

/// GET /bookmarks/:collection/load
pub async fn ws_load_collection(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(collection): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        handle_ws_load_collection(socket, state, collection).await;
    })
}

async fn handle_ws_load_collection(mut socket: WebSocket, state: AppState, collection: String) {
    let res = tokio::task::spawn_blocking({
        let st = state.clone();
        let c = collection.clone();
        move || -> Result<(bool, usize), String> {
            let membership = open_existing_collection_membership(&st, &c)?;
            let ids = membership.list_ids().map_err(|e| format!("{e:?}"))?;

            let moved_to_backup =
                db::backup_active_bookmarks(&st.core, &st.queries).map_err(|e| format!("{e:?}"))?;
            db::clear_bookmarks_active(&st.core, &st.queries).map_err(|e| format!("{e:?}"))?;

            let mut loaded = 0usize;
            for eid in ids {
                loaded += db::insert_bookmark_active(&st.core, &st.queries, &eid, None)
                    .map_err(|e| format!("{e:?}"))?;
            }
            Ok((moved_to_backup, loaded))
        }
    })
    .await;

    let (moved_to_backup, loaded) = match res {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            let resp = json!({"type":"error","message": e});
            let _ = socket.send(Message::Text(resp.to_string())).await;
            return;
        }
        Err(e) => {
            let resp = json!({"type":"error","message": format!("{e}")});
            let _ = socket.send(Message::Text(resp.to_string())).await;
            return;
        }
    };

    // This collection is now the loaded context — bookmarking, save, and
    // unsave all key off this from here on.
    *state.active_collection.write().await = Some(collection.clone());

    let count = tokio::task::spawn_blocking({
        let st = state.clone();
        move || db::bookmarks_count_active(&st.core, &st.queries)
    })
    .await
    .ok()
    .and_then(|x| x.ok())
    .unwrap_or(0);

    state
        .emit(ServerEvent::CollectionLoaded {
            collection: collection.clone(),
            moved_to_backup,
        })
        .await;
    state.emit(ServerEvent::BookmarksUpdated { count }).await;

    let resp = json!({
        "type": "collection_loaded",
        "collection": collection,
        "moved_to_backup": moved_to_backup,
        "loaded_into_active": loaded
    });
    let _ = socket.send(Message::Text(resp.to_string())).await;

    // Stream events after load — same pattern every other subscribe-style
    // WS route in this codebase uses.
    let mut rx = state.events_tx.subscribe();
    while let Ok(evt) = rx.recv().await {
        let msg = json!({"type":"event","event": evt});
        if socket.send(Message::Text(msg.to_string())).await.is_err() {
            break;
        }
    }
}

/// POST /bookmarks/active/:endpoint_id/save — persists a bookmarked
/// endpoint's membership (EID + timestamp only, never a copy of its data)
/// into whichever collection is currently loaded.
pub async fn save_to_collection(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let collection = match require_active_collection(&state).await {
        Ok(c) => c,
        Err((status, body)) => return (status, body),
    };

    let res = tokio::task::spawn_blocking({
        let st = state.clone();
        let eid = endpoint_id.clone();
        let collection = collection.clone();
        move || -> Result<usize, String> {
            let bookmarked = db::list_bookmarked_endpoints_active(&st.core, &st.queries)
                .map_err(|e| format!("{e:?}"))?
                .contains(&eid);
            if !bookmarked {
                return Err(format!("'{eid}' is not bookmarked in the active workspace"));
            }
            let membership = open_existing_collection_membership(&st, &collection)?;
            membership.add(&eid).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(inserted)) => {
            state.refresh_dashboard_snapshot();
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "collection": collection, "inserted": inserted })),
            )
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /bookmarks/active/:endpoint_id/unsave — drops an endpoint's
/// membership from the currently loaded collection. Per Mathew
/// (2026-09-08): it stays bookmarked/visible in active afterward — only
/// the collection membership is removed, not the bookmark itself.
pub async fn remove_from_collection(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let collection = match require_active_collection(&state).await {
        Ok(c) => c,
        Err((status, body)) => return (status, body),
    };

    let res = tokio::task::spawn_blocking({
        let st = state.clone();
        let eid = endpoint_id.clone();
        let collection = collection.clone();
        move || -> Result<usize, String> {
            let membership = open_existing_collection_membership(&st, &collection)?;
            membership.remove(&eid).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(deleted)) => {
            state.refresh_dashboard_snapshot();
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "collection": collection, "deleted": deleted })),
            )
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}
