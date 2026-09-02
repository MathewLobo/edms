use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::collection_tag_ops::CollectionTagMembershipOps;
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

fn open_ops(state: &AppState) -> Result<CollectionTagMembershipOps, String> {
    let ops = CollectionTagMembershipOps::new(&state.db_path.display().to_string());
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

/// POST /collections/:name/membership-tags/add
pub async fn add_collection_tag(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<TagRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_ops(&state)?;
            ops.add(&name, &payload.tag).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(rows)) => {
            state.refresh_dashboard_snapshot();
            (StatusCode::OK, Json(json!({ "ok": true, "inserted": rows })))
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /collections/:name/membership-tags/remove
pub async fn remove_collection_tag(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<TagRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_ops(&state)?;
            ops.remove(&name, &payload.tag).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(rows)) => {
            state.refresh_dashboard_snapshot();
            (StatusCode::OK, Json(json!({ "ok": true, "deleted": rows })))
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// GET /collections/:name/membership-tags
pub async fn list_collection_tags(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<String>, String> {
            let ops = open_ops(&state)?;
            ops.list(&name).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(tags)) => (StatusCode::OK, Json(json!({ "tags": tags }))),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// GET /collections/by-tag/:tagname
pub async fn collections_by_tag(
    State(state): State<AppState>,
    Path(tagname): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<String>, String> {
            let ops = open_ops(&state)?;
            ops.collections_by_tag(&tagname).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(collections)) => (StatusCode::OK, Json(json!({ "collections": collections }))),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}
