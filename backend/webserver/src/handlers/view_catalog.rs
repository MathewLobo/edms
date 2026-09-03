//! Catalog registration for collections/webview/repoview — which instances
//! exist, per Ravi's schema (2026-08-25). `file_path` (the independent
//! SQLite file each one gets) isn't created for webview/repoview by this
//! pass yet — those catalog rows still get `file_path: null`.
//!
//! Collections are further along (per Ravi, 2026-09-03): each one gets its
//! own real SQLite file under `storage/collections/{name}.sqlite`, holding
//! just endpoint_id + when it was added — never a copy of the endpoint's
//! actual data, which always stays in the central `endpoints` table.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::collection_membership_ops::CollectionMembershipOps;
use edms::ops::view_ops::{ViewCatalogOps, ViewKind};
use serde::Deserialize;
use serde_json::json;

use crate::{db, state::AppState};

#[derive(Debug, Deserialize)]
pub struct RegisterViewRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct EndpointIdRequest {
    pub endpoint_id: String,
}

fn open_catalog(state: &AppState) -> Result<ViewCatalogOps, String> {
    let ops = ViewCatalogOps::new(&state.db_path.display().to_string());
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

/// Where a collection's own file lives on disk — matches the Sept-1 schema
/// path, and the directory is guaranteed to already exist by the folder
/// init work (`storage/collections`) that runs on every launch.
fn collection_file_path(state: &AppState, name: &str) -> String {
    state
        .storage_root
        .join("storage")
        .join("collections")
        .join(format!("{name}.sqlite"))
        .display()
        .to_string()
}

fn open_membership(file_path: &str) -> Result<CollectionMembershipOps, String> {
    let ops = CollectionMembershipOps::new(file_path);
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

async fn register_for(
    kind: ViewKind,
    state: AppState,
    payload: RegisterViewRequest,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_catalog(&state)?;
            ops.register(kind, &payload.name, None).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(inserted)) => {
            state.refresh_dashboard_snapshot();
            (StatusCode::OK, Json(json!({ "ok": true, "inserted": inserted })))
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
        move || -> Result<Vec<(String, Option<String>, String)>, String> {
            let ops = open_catalog(&state)?;
            ops.list(kind).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(rows)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "items": rows.into_iter().map(|(name, file_path, created_at)| json!({
                    "name": name, "file_path": file_path, "created_at": created_at
                })).collect::<Vec<_>>()
            })),
        ),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /collections/create — registers the catalog row AND creates the
/// collection's own SQLite file, storing its path in `file_path`.
pub async fn create_collection_entry(
    State(state): State<AppState>,
    Json(payload): Json<RegisterViewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = payload.name.clone();
        move || -> Result<(usize, String), String> {
            let path = collection_file_path(&state, &name);

            // Create the file + its schema first — if this fails, we don't
            // want a catalog row pointing at a file that doesn't exist.
            open_membership(&path)?;

            let catalog = open_catalog(&state)?;
            let inserted = catalog
                .register(ViewKind::Collections, &name, Some(&path))
                .map_err(|e| format!("{e:?}"))?;
            Ok((inserted, path))
        }
    })
    .await;

    match res {
        Ok(Ok((inserted, path))) => {
            state.refresh_dashboard_snapshot();
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "inserted": inserted, "file_path": path })),
            )
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /collections/:name/delete — removes the catalog row and deletes
/// the collection's own file from disk.
pub async fn delete_collection_entry(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<(usize, bool), String> {
            let catalog = open_catalog(&state)?;
            let existing = catalog
                .get(ViewKind::Collections, &name)
                .map_err(|e| format!("{e:?}"))?;

            let deleted_rows = catalog
                .remove(ViewKind::Collections, &name)
                .map_err(|e| format!("{e:?}"))?;

            let mut file_deleted = false;
            if let Some((_, Some(file_path), _)) = existing {
                if std::path::Path::new(&file_path).exists() {
                    std::fs::remove_file(&file_path).map_err(|e| e.to_string())?;
                    file_deleted = true;
                }
            }

            Ok((deleted_rows, file_deleted))
        }
    })
    .await;

    match res {
        Ok(Ok((deleted_rows, file_deleted))) => {
            state.refresh_dashboard_snapshot();
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "deleted_rows": deleted_rows, "file_deleted": file_deleted })),
            )
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /collections/:name/endpoints/add — adds an endpoint to this
/// collection's own file. Rejects endpoint IDs that don't exist in the
/// central endpoints table — a collection is a membership list over real
/// data, never a place to reference something that doesn't exist.
pub async fn add_endpoint_to_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<EndpointIdRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        let endpoint_id = payload.endpoint_id.clone();
        move || -> Result<usize, String> {
            let exists = db::get_endpoint(&state.core, &state.queries, &endpoint_id)
                .map_err(|e| format!("{e:?}"))?
                .is_some();
            if !exists {
                return Err(format!("Endpoint '{endpoint_id}' does not exist"));
            }

            let catalog = open_catalog(&state)?;
            let row = catalog
                .get(ViewKind::Collections, &name)
                .map_err(|e| format!("{e:?}"))?
                .ok_or_else(|| format!("Collection '{name}' does not exist"))?;
            let path = row.1.ok_or_else(|| format!("Collection '{name}' has no file yet"))?;

            let membership = open_membership(&path)?;
            membership.add(&endpoint_id).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(inserted)) => (StatusCode::OK, Json(json!({ "ok": true, "inserted": inserted }))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /collections/:name/endpoints/remove
pub async fn remove_endpoint_from_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<EndpointIdRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        let endpoint_id = payload.endpoint_id.clone();
        move || -> Result<usize, String> {
            let catalog = open_catalog(&state)?;
            let row = catalog
                .get(ViewKind::Collections, &name)
                .map_err(|e| format!("{e:?}"))?
                .ok_or_else(|| format!("Collection '{name}' does not exist"))?;
            let path = row.1.ok_or_else(|| format!("Collection '{name}' has no file yet"))?;

            let membership = open_membership(&path)?;
            membership.remove(&endpoint_id).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(deleted)) => (StatusCode::OK, Json(json!({ "ok": true, "deleted": deleted }))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// GET /collections/:name/endpoints — list this collection's members.
pub async fn list_collection_endpoints(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<Vec<(String, String)>, String> {
            let catalog = open_catalog(&state)?;
            let row = catalog
                .get(ViewKind::Collections, &name)
                .map_err(|e| format!("{e:?}"))?
                .ok_or_else(|| format!("Collection '{name}' does not exist"))?;
            let path = row.1.ok_or_else(|| format!("Collection '{name}' has no file yet"))?;

            let membership = open_membership(&path)?;
            let entries = membership.list().map_err(|e| format!("{e:?}"))?;
            Ok(entries.into_iter().map(|e| (e.endpoint_id, e.added_at)).collect())
        }
    })
    .await;

    match res {
        Ok(Ok(entries)) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "endpoints": entries.into_iter().map(|(endpoint_id, added_at)| json!({
                    "endpoint_id": endpoint_id, "added_at": added_at
                })).collect::<Vec<_>>()
            })),
        ),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

pub async fn list_collections(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    list_for(ViewKind::Collections, state).await
}

pub async fn create_webview_entry(
    State(state): State<AppState>,
    Json(payload): Json<RegisterViewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    register_for(ViewKind::Webview, state, payload).await
}

pub async fn list_webviews(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    list_for(ViewKind::Webview, state).await
}

pub async fn create_repoview_entry(
    State(state): State<AppState>,
    Json(payload): Json<RegisterViewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    register_for(ViewKind::Repoview, state, payload).await
}

pub async fn list_repoviews(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    list_for(ViewKind::Repoview, state).await
}
