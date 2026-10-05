use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::repoview_tag_ops::RepoviewTagMembershipOps;
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

fn open_ops(state: &AppState) -> Result<RepoviewTagMembershipOps, String> {
    let ops = RepoviewTagMembershipOps::new(&state.db_path.display().to_string());
    ops.initialize().map_err(|e| format!("{e:?}"))?;
    Ok(ops)
}

/// POST /repoview/:name/membership-tags/add
pub async fn add_repoview_tag(
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

/// POST /repoview/:name/membership-tags/remove
pub async fn remove_repoview_tag(
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

/// GET /repoview/:name/membership-tags
pub async fn list_repoview_tags_for_name(
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

/// GET /repoview/by-tag/:tagname
pub async fn repoviews_by_tag(
    State(state): State<AppState>,
    Path(tagname): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || -> Result<Vec<String>, String> {
            let ops = open_ops(&state)?;
            ops.repoviews_by_tag(&tagname).map_err(|e| format!("{e:?}"))
        }
    })
    .await;

    match res {
        Ok(Ok(repoviews)) => (StatusCode::OK, Json(json!({ "repoviews": repoviews }))),
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
