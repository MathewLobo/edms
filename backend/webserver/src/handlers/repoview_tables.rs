//! Index Table generation for RepoView — Approach [B], Endpoint Segments
//! (per Ravi's Index Table wiki, 2026-09-05). Generic, no tags needed:
//! sorts every member's URL path alphabetically and splits it into
//! Tables-NNN.md batches, plus a Tables-meta.md summarizing which file
//! covers which segment range so the frontend never has to open a batch
//! file just to find where something lives.
//!
//! Approach [A] (Sorted Tags) isn't built yet — Ravi flagged it as the
//! more complicated one (overlapping, "disconnected" tag sets), and B is
//! the generic baseline this was scoped against first.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::view_ops::ViewKind;
use serde::Deserialize;
use serde_json::json;

use crate::{
    db,
    handlers::view_catalog::{open_existing_membership, repoview_dir},
    state::AppState,
};

const DEFAULT_BATCH_SIZE: usize = 100;

#[derive(Debug, Deserialize)]
pub struct GenerateTablesRequest {
    #[serde(default)]
    pub batch_size: Option<usize>,
}

struct SegmentRow {
    segment_path: String,
    endpoint_id: String,
    method: String,
}

/// Strips scheme + host from a URL, keeping the path as-is — including
/// placeholder syntax like `:id`/`{id}` kept literal, per the wiki's own
/// examples. Falls back to the raw string if it doesn't look like a URL,
/// so a malformed `endpoint_str` still sorts somewhere instead of
/// vanishing from the output.
fn segment_path(endpoint_str: &str) -> String {
    // No "://" at all means this doesn't look like a URL — return it
    // verbatim rather than guessing, so it still sorts somewhere instead
    // of silently becoming "/".
    let Some((_, after_scheme)) = endpoint_str.split_once("://") else {
        return endpoint_str.to_string();
    };
    let path = match after_scheme.find('/') {
        Some(idx) => &after_scheme[idx..],
        None => "/",
    };
    // Query string / fragment aren't path segments.
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// POST /repoview/:name/tables/generate
///
/// Deliberately NOT run as part of create (a "zero time op" per the spec)
/// or on every membership change — generation is its own explicit, "time
/// consuming" action, same category as delete/duplicate. Replaces every
/// existing Tables-*.md in the folder on each call, since the number of
/// batches needed can shrink or grow as membership changes.
pub async fn generate_repoview_tables(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<GenerateTablesRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let batch_size = payload.batch_size.filter(|&n| n > 0).unwrap_or(DEFAULT_BATCH_SIZE);

    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<serde_json::Value, String> {
            let membership = open_existing_membership(&state, ViewKind::Repoview, &name)?;
            let member_ids = membership.list_ids().map_err(|e| format!("{e:?}"))?;

            let mut rows: Vec<SegmentRow> = Vec::with_capacity(member_ids.len());
            for eid in &member_ids {
                if let Some(endpoint) = db::get_endpoint(&state.core, &state.queries, eid)
                    .map_err(|e| format!("{e:?}"))?
                {
                    rows.push(SegmentRow {
                        segment_path: segment_path(&endpoint.endpoint_str),
                        endpoint_id: endpoint.endpoint_id,
                        method: endpoint.method.unwrap_or_else(|| "UNCLASSIFIED".to_string()),
                    });
                }
                // An id with no central endpoint row left (deleted out
                // from under this RepoView's membership) is silently
                // skipped — best-effort, matches the stats endpoint.
            }
            rows.sort_by(|a, b| {
                a.segment_path.cmp(&b.segment_path).then(a.endpoint_id.cmp(&b.endpoint_id))
            });

            let dir = repoview_dir(&state, &name);
            if !dir.is_dir() {
                return Err(format!("RepoView '{name}' has no folder yet"));
            }

            for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if file_name.starts_with("Tables-") && file_name.ends_with(".md") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }

            if rows.is_empty() {
                std::fs::write(
                    dir.join("Tables-meta.md"),
                    "# Tables-meta\n\nNo endpoints in this RepoView yet.\n",
                )
                .map_err(|e| e.to_string())?;
                return Ok(json!({
                    "ok": true,
                    "batch_size": batch_size,
                    "total_endpoints": 0,
                    "files_written": ["Tables-meta.md"]
                }));
            }

            let mut meta_rows: Vec<(String, String, String, usize)> = Vec::new();
            let mut files_written: Vec<String> = Vec::new();

            for (batch_index, chunk) in rows.chunks(batch_size).enumerate() {
                let file_name = format!("Tables-{:03}.md", batch_index + 1);
                let mut content = String::from("| Segment Path | EID | Method |\n|---|---|---|\n");
                for row in chunk {
                    content.push_str(&format!(
                        "| {} | {} | {} |\n",
                        row.segment_path, row.endpoint_id, row.method
                    ));
                }
                std::fs::write(dir.join(&file_name), content).map_err(|e| e.to_string())?;

                meta_rows.push((
                    file_name.clone(),
                    chunk.first().unwrap().segment_path.clone(),
                    chunk.last().unwrap().segment_path.clone(),
                    chunk.len(),
                ));
                files_written.push(file_name);
            }

            let mut meta_content = String::from(
                "# Tables-meta\n\n| File | First Segment | Last Segment | Count |\n|---|---|---|---|\n",
            );
            for (file_name, first, last, count) in &meta_rows {
                meta_content.push_str(&format!("| {file_name} | {first} | {last} | {count} |\n"));
            }
            std::fs::write(dir.join("Tables-meta.md"), meta_content).map_err(|e| e.to_string())?;
            files_written.push("Tables-meta.md".to_string());

            Ok(json!({
                "ok": true,
                "batch_size": batch_size,
                "total_endpoints": rows.len(),
                "files_written": files_written
            }))
        }
    })
    .await;

    match res {
        Ok(Ok(body)) => (StatusCode::OK, Json(body)),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::segment_path;

    #[test]
    fn strips_scheme_and_host_keeping_placeholder_syntax() {
        assert_eq!(segment_path("https://api.example.com/users/:id/orders"), "/users/:id/orders");
        assert_eq!(segment_path("http://api.example.com/a/{b}/c"), "/a/{b}/c");
    }

    #[test]
    fn handles_no_path_query_and_fragment() {
        assert_eq!(segment_path("https://api.example.com"), "/");
        assert_eq!(segment_path("https://api.example.com/items?x=1"), "/items");
        assert_eq!(segment_path("https://api.example.com/items#frag"), "/items");
    }

    #[test]
    fn falls_back_to_the_raw_string_for_non_urls() {
        assert_eq!(segment_path("not-a-url"), "not-a-url");
    }
}
