use axum::{extract::State, http::StatusCode, Json};
use serde_json::json;

use crate::{dashboard_db, state::AppState};

/// GET /dashboard/snapshot
/// Returns the most recent dashboard snapshot (endpoint/bookmark/tag counts,
/// main DB size, storage size, file count) plus the current process's
/// start timestamp, which isn't stored per-snapshot since it doesn't change
/// between snapshots.
pub async fn get_dashboard_snapshot(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let conn = state.dashboard_conn.lock().unwrap();
    match dashboard_db::get_latest_snapshot(&conn) {
        Ok(Some(snapshot)) => {
            let mut body = json!(snapshot);
            body["app_started_at"] = json!(state.started_at);
            (StatusCode::OK, Json(body))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no dashboard snapshot available yet" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e}") })),
        ),
    }
}

/// GET /dashboard/static
/// Returns the hardcoded static data loaded from config.yaml at startup:
/// application limits, stability/commit info, and external links.
pub async fn get_static_data(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(json!(state.config.as_ref())))
}

/// GET /dashboard/snapshot/history
/// Returns every retained snapshot (rolling 30-day window), oldest first,
/// for trend charts.
pub async fn get_dashboard_snapshot_history(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let conn = state.dashboard_conn.lock().unwrap();
    match dashboard_db::get_snapshot_history(&conn) {
        Ok(snapshots) => (StatusCode::OK, Json(json!(snapshots))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e}") })),
        ),
    }
}
