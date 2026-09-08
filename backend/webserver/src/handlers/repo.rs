use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;
use crate::{db, ipc, state::AppState};

pub async fn export_collection(
    State(state): State<AppState>,
    Path((collection, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    // 1. Fetch endpoint IDs from DB — same as before
    let res = tokio::task::spawn_blocking({
        let st = state.clone();
        let collection = collection.clone();
        move || {
            let q = "SELECT endpoint_id FROM bookmarks WHERE folder = ? ORDER BY timestamp DESC";
            let ids: Vec<String> = st.core.cproc(q, &[&collection], |row| row.get(0))?;
            db::endpoints_for_ids(&st.core, &st.queries, &ids)
        }
    })
    .await;

    let endpoints = match res {
        Ok(Ok(ep)) => ep,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                "collection not found or DB query failed".to_string(),
            )
                .into_response()
        }
    };

    // 2. Spawn edms-child to package the zip export
    //    Result arrives via /internal/callback — fire and forget
    ipc::spawn_child(
        "export_collection",
        json!({
            "source": format!("edms_root/endpoints/reports/{collection}"),
            "output": format!("edms_root/exports/{filename}"),
        }),
        3000,
    );

    // 3. Spawn edms-child to generate markdown
    //    Uses the endpoint data we already fetched from DB
    ipc::spawn_child(
        "generate_markdown",
        json!({
            "repo_path": "edms_root/endpoints/reports",
            "per_file":  20,
            "endpoints": endpoints.iter().map(|ep| json!({
                "eid":           0,
                "eid_string":    ep.endpoint_id,
                "endpoint_type": "http",
                "request_type":  "rest",
                "annotation":    ep.annotation.clone().unwrap_or_default(),
                "tags":          "",
                "req_count":     0,
                "res_count":     0,
            })).collect::<Vec<_>>()
        }),
        3000,
    );

    // 4. Return local markdown immediately as the HTTP response
    //    (same as the original fallback — client gets something right away)
    let mut md = String::new();
    md.push_str(&format!("# Export: {collection}\n\n"));
    md.push_str(&format!("Generated as `{filename}`\n\n"));
    for ep in &endpoints {
        md.push_str(&format!("- **{}**: `{}`", ep.endpoint_id, ep.endpoint_str));
        if let Some(ref a) = ep.annotation {
            md.push_str(&format!(" — {}", a));
        }
        md.push('\n');
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        md,
    )
        .into_response()
}

/// POST /repo/{collection}/{filename}/import
///
/// The compute-side `import_zip` task already existed (zipops::import_zip_impl)
/// but nothing in the webserver called it — this was the missing wiring.
/// Mirrors export_collection in reverse, using the same path convention:
/// unzips the file export_collection writes to (`edms_root/exports/{filename}`)
/// back into the same path export_collection reads its source from
/// (`edms_root/endpoints/reports/{collection}`). Fire-and-forget — result
/// arrives via /internal/callback.
///
/// Honest limitation: this only extracts files to disk. It does not parse
/// the extracted request/response/headers JSON back into the DB (endpoints,
/// bookmarks, etc.) — that's a separate piece of logic nobody has specified
/// yet (see Ravi's 2026-09-07 email: IE "depends only on Collections" but
/// the UI/backend split for it isn't finalized).
pub async fn import_collection(
    State(_state): State<AppState>,
    Path((collection, filename)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    ipc::spawn_child(
        "import_zip",
        json!({
            "zip": format!("edms_root/exports/{filename}"),
            "destination": format!("edms_root/endpoints/reports/{collection}"),
        }),
        3000,
    );

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "ok": true,
            "status": "import queued",
            "collection": collection,
            "filename": filename,
        })),
    )
}