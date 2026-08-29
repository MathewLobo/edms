//! Catalog registration for collections/webview/repoview — which instances
//! exist, per Ravi's schema (2026-08-25). `file_path` (the independent
//! SQLite file each one gets) isn't created by this pass; a catalog row can
//! exist with `file_path: null` until that infrastructure lands.

use axum::{extract::State, http::StatusCode, Json};
use edms::ops::view_ops::{ViewCatalogOps, ViewKind};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterViewRequest {
    pub name: String,
}

fn open_ops(state: &AppState) -> Result<ViewCatalogOps, String> {
    let ops = ViewCatalogOps::new(&state.db_path.display().to_string());
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
            let ops = open_ops(&state)?;
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

pub async fn create_collection_entry(
    State(state): State<AppState>,
    Json(payload): Json<RegisterViewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    register_for(ViewKind::Collections, state, payload).await
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
