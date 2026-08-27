use axum::{extract::{Path, State}, http::StatusCode, Json};
use edms::ops::tag_ops::TagOps;
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

fn open_tag_ops(state: &AppState) -> Result<TagOps, String> {
    let ops = TagOps::new(&state.db_path.display().to_string());
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

/// POST /tags/:endpoint_id/add
pub async fn add_tag(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
    Json(payload): Json<TagRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_tag_ops(&state)?;
            ops.add(&endpoint_id, &payload.tag).map_err(|e| format!("{e:?}"))
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

/// POST /tags/:endpoint_id/remove
pub async fn remove_tag(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
    Json(payload): Json<TagRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<usize, String> {
            let ops = open_tag_ops(&state)?;
            ops.remove(&endpoint_id, &payload.tag).map_err(|e| format!("{e:?}"))
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

/// GET /tags/:endpoint_id
pub async fn list_tags_for_endpoint(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<String>, String> {
            let ops = open_tag_ops(&state)?;
            ops.get_by_endpoint(&endpoint_id).map_err(|e| format!("{e:?}"))
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

/// GET /tags/popular
pub async fn popular_tags(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<(String, i32)>, String> {
            let ops = open_tag_ops(&state)?;
            ops.get_popular_tags().map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(tags)) => (
            StatusCode::OK,
            Json(json!({
                "tags": tags.into_iter().map(|(tag, count)| json!({ "tag": tag, "count": count })).collect::<Vec<_>>()
            })),
        ),
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
