use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
};

use crate::{logging::LOG_FILE_NAME, state::AppState};

const MAX_LINES: usize = 500;

/// GET /logs — plain-text tail of the app log file (app.log under the
/// EDMS root), most recent lines last.
pub async fn get_logs(State(state): State<AppState>) -> impl IntoResponse {
    let path = state.storage_root.join(LOG_FILE_NAME);

    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(MAX_LINES);
            let tail = lines[start..].join("\n");
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                tail,
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("log file not available yet: {e}"),
        )
            .into_response(),
    }
}
