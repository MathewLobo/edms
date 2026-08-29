//! Per-view tag counts (create/delete/rename) — collections, webview,
//! repoview. Backed by Ravi's schema (2026-08-25): a separate tagname+count
//! table per view type, not a shared table+column, and not a full tag
//! entity with membership tracking — just an incrementally maintained
//! counter. `bookmarks`/`eqptags` is untouched — not part of this schema.
//!
//! Payload shapes are a documented assumption pending confirmation: `create`
//! takes a set of endpoint ids (its length becomes the count increment),
//! `delete` takes multiple tag names in one call, `rename` is single.
//!
//! Per the WS-vs-REST rule: these mutate visible tag state, so each handler
//! triggers a `ViewTagsUpdated` event after the REST call completes.

use axum::{extract::State, http::StatusCode, Json};
use edms::ops::view_ops::{ViewKind, ViewTagCountOps};
use serde::Deserialize;
use serde_json::json;

use crate::{events::ServerEvent, state::AppState};

#[derive(Debug, Deserialize)]
pub struct CreateViewTagRequest {
    pub name: String,
    #[serde(default)]
    pub endpoint_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteViewTagsRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameViewTagRequest {
    pub old_name: String,
    pub new_name: String,
}

fn open_ops(state: &AppState) -> Result<ViewTagCountOps, String> {
    let ops = ViewTagCountOps::new(&state.db_path.display().to_string());
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

async fn create_for(
    kind: ViewKind,
    view: &'static str,
    state: AppState,
    payload: CreateViewTagRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<(), String> {
            let ops = open_ops(&state)?;
            let increment = payload.endpoint_ids.len().max(1) as i64;
            ops.create(kind, &payload.name, increment)
                .map_err(|e| format!("{e:?}"))?;
            Ok(())
        }
    })
    .await;

    match res {
        Ok(Ok(())) => {
            state.refresh_dashboard_snapshot();
            state.emit(ServerEvent::ViewTagsUpdated { view: view.to_string() }).await;
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

async fn delete_for(
    kind: ViewKind,
    view: &'static str,
    state: AppState,
    payload: DeleteViewTagsRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_ops(&state)?;
            ops.delete_many(kind, &payload.names).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(deleted)) => {
            state.refresh_dashboard_snapshot();
            state.emit(ServerEvent::ViewTagsUpdated { view: view.to_string() }).await;
            (StatusCode::OK, Json(json!({ "ok": true, "deleted": deleted })))
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

async fn rename_for(
    kind: ViewKind,
    view: &'static str,
    state: AppState,
    payload: RenameViewTagRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<(), String> {
            let ops = open_ops(&state)?;
            ops.rename(kind, &payload.old_name, &payload.new_name)
                .map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(())) => {
            state.refresh_dashboard_snapshot();
            state.emit(ServerEvent::ViewTagsUpdated { view: view.to_string() }).await;
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

async fn list_for(kind: ViewKind, state: AppState) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<(String, i64)>, String> {
            let ops = open_ops(&state)?;
            ops.list(kind).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(rows)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "tags": rows.into_iter().map(|(tagname, count)| json!({ "tagname": tagname, "count": count })).collect::<Vec<_>>()
            })),
        ),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

macro_rules! view_tag_routes {
    ($kind:expr, $view:literal, $create:ident, $delete:ident, $rename:ident, $list:ident) => {
        pub async fn $create(
            State(state): State<AppState>,
            Json(payload): Json<CreateViewTagRequest>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            create_for($kind, $view, state, payload).await
        }

        pub async fn $delete(
            State(state): State<AppState>,
            Json(payload): Json<DeleteViewTagsRequest>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            delete_for($kind, $view, state, payload).await
        }

        pub async fn $rename(
            State(state): State<AppState>,
            Json(payload): Json<RenameViewTagRequest>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            rename_for($kind, $view, state, payload).await
        }

        pub async fn $list(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
            list_for($kind, state).await
        }
    };
}

view_tag_routes!(ViewKind::Collections, "collections", create_collections_tag, delete_collections_tags, rename_collections_tag, list_collections_tags);
view_tag_routes!(ViewKind::Webview, "webview", create_webview_tag, delete_webview_tags, rename_webview_tag, list_webview_tags);
view_tag_routes!(ViewKind::Repoview, "repoview", create_repoview_tag, delete_repoview_tags, rename_repoview_tag, list_repoview_tags);
