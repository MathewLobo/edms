use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::tag_ops::TagOps;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path as FsPath;

use crate::{db, ipc, state::AppState};

#[derive(Debug, Deserialize)]
pub struct EqpExportRequest {
    /// Accepted values: pdf, html, md, markdown.
    pub format: String,
    /// Optional user-facing filename. Defaults to the EID.
    pub output_filename: Option<String>,
}

struct ExportFormat {
    task: &'static str,
    directory: &'static str,
    extension: &'static str,
}

/// POST /reports/:endpoint_id/export
///
/// Reads the selected EQP directly from the configured EDMS root, combines
/// its request/response/header files with database metadata and tags, and
/// sends that complete JSON payload to the compute converter.
pub async fn export_eqp_report(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
    Json(request): Json<EqpExportRequest>,
) -> (StatusCode, Json<Value>) {
    if !safe_filename(&endpoint_id) {
        return error(StatusCode::BAD_REQUEST, "invalid endpoint_id");
    }

    let format = match parse_format(&request.format) {
        Some(format) => format,
        None => {
            return error(
                StatusCode::BAD_REQUEST,
                "format must be one of: pdf, html, md, markdown",
            )
        }
    };
    let output_filename = request
        .output_filename
        .unwrap_or_else(|| endpoint_id.clone());
    if !safe_filename(&output_filename) {
        return error(
            StatusCode::BAD_REQUEST,
            "output_filename must be a non-empty filename, not a path",
        );
    }

    let assembled = tokio::task::spawn_blocking({
        let state = state.clone();
        let endpoint_id = endpoint_id.clone();
        move || assemble_from_edms_data(&state, &endpoint_id)
    })
    .await;

    let report = match assembled {
        Ok(Ok(Some(report))) => report,
        Ok(Ok(None)) => return error(StatusCode::NOT_FOUND, "endpoint_id was not found"),
        Ok(Err(message)) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message),
        Err(join_error) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to assemble EQP report: {join_error}"),
            )
        }
    };

    let takeout_root = state.storage_root.join("takeout");
    let input_data = match serde_json::to_string(&report) {
        Ok(input_data) => input_data,
        Err(serialize_error) => {
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to serialize EQP report: {serialize_error}"),
            )
        }
    };

    ipc::spawn_child(
        format.task,
        json!({
            "input_data": input_data,
            "output_filename": output_filename,
            "output_filepath": takeout_root,
        }),
        3000,
    );

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "ok": true,
            "status": "accepted",
            "endpoint_id": endpoint_id,
            "format": format.extension,
            "output_directory": takeout_root.join(format.directory),
            "message": "EQP data was read from edms-data and sent to the compute converter"
        })),
    )
}

fn assemble_from_edms_data(state: &AppState, endpoint_id: &str) -> Result<Option<Value>, String> {
    let endpoint = db::get_endpoint(&state.core, &state.queries, endpoint_id)
        .map_err(|database_error| format!("failed to read endpoint metadata: {database_error:?}"))?;
    let Some(endpoint) = endpoint else {
        return Ok(None);
    };

    let tag_ops = TagOps::new(&state.db_path.display().to_string());
    tag_ops
        .initialize()
        .map_err(|database_error| format!("failed to initialize tags: {database_error:?}"))?;
    let tags = tag_ops
        .get_by_endpoint(endpoint_id)
        .map_err(|database_error| format!("failed to read endpoint tags: {database_error:?}"))?;
    let qps = db::list_qps_for_endpoint(&state.core, &state.queries, endpoint_id)
        .map_err(|database_error| format!("failed to read QP metadata: {database_error:?}"))?;

    let report = assemble_eqp_payload(&state.storage_root, endpoint, tags, qps)?;
    Ok(Some(report))
}

fn assemble_eqp_payload(
    storage_root: &FsPath,
    endpoint: db::EndpointDto,
    tags: Vec<String>,
    qps: Vec<db::QpSummary>,
) -> Result<Value, String> {
    let endpoint_dir = storage_root
        .join("storage")
        .join("globalEQPData")
        .join(&endpoint.endpoint_id);
    let mut qp_data = Vec::with_capacity(qps.len());

    for qp in qps {
        let request_number = qp.request_number;
        let prefix = format!("{}-{request_number}", endpoint.endpoint_id);
        qp_data.push(json!({
            "request_number": request_number,
            "method": qp.method,
            "timestamp": qp.timestamp,
            "status_code": qp.status_code,
            "response_time_ms": qp.response_time_ms,
            "request": read_json(&endpoint_dir.join(format!("{}-request-{request_number}.json", endpoint.endpoint_id)))?,
            "response": read_json(&endpoint_dir.join(format!("{}-response-{request_number}.json", endpoint.endpoint_id)))?,
            "headers": read_json(&endpoint_dir.join(format!("{}-headers-{request_number}.json", endpoint.endpoint_id)))?,
            "source": prefix,
        }));
    }

    Ok(json!({
        "eid": endpoint.endpoint_id,
        "crud": endpoint.method.unwrap_or_else(|| "UNCLASSIFIED".to_string()),
        "endpoint_string": endpoint.endpoint_str,
        "annotation": endpoint.annotation,
        "tags": tags,
        "qp_data": qp_data,
    }))
}

fn read_json(path: &FsPath) -> Result<Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|file_error| format!("failed to read {}: {file_error}", path.display()))?;
    serde_json::from_str(&content)
        .map_err(|json_error| format!("invalid JSON in {}: {json_error}", path.display()))
}

fn parse_format(input: &str) -> Option<ExportFormat> {
    match input.trim().to_ascii_lowercase().as_str() {
        "pdf" => Some(ExportFormat {
            task: "convert_to_pdf",
            directory: "PDF",
            extension: "pdf",
        }),
        "html" => Some(ExportFormat {
            task: "convert_to_html",
            directory: "HTML",
            extension: "html",
        }),
        "md" | "markdown" => Some(ExportFormat {
            task: "convert_to_markdown",
            directory: "MD",
            extension: "md",
        }),
        _ => None,
    }
}

fn safe_filename(value: &str) -> bool {
    let path = FsPath::new(value);
    !value.trim().is_empty()
        && path.file_name().is_some()
        && path.file_name() == Some(path.as_os_str())
        && value != "."
        && value != ".."
}

fn error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "ok": false, "error": message })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn assembles_request_response_and_headers_from_an_eid_folder() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("edms-report-export-{unique}"));
        let endpoint_dir = root.join("storage/globalEQPData/E0001-AAA");
        fs::create_dir_all(&endpoint_dir).expect("create sample EID directory");
        fs::write(endpoint_dir.join("E0001-AAA-request-1.json"), r#"{"username":"emilys"}"#)
            .expect("write request");
        fs::write(endpoint_dir.join("E0001-AAA-response-1.json"), r#"{"id":1}"#)
            .expect("write response");
        fs::write(
            endpoint_dir.join("E0001-AAA-headers-1.json"),
            r#"{"request_headers":{},"response_headers":{"content-type":"application/json"}}"#,
        )
        .expect("write headers");

        let report = assemble_eqp_payload(
            &root,
            db::EndpointDto {
                endpoint_id: "E0001-AAA".to_string(),
                endpoint_str: "https://dummyjson.com/auth/login".to_string(),
                annotation: Some("Login".to_string()),
                method: Some("POST".to_string()),
            },
            vec!["production".to_string()],
            vec![db::QpSummary {
                request_number: 1,
                method: Some("POST".to_string()),
                timestamp: Some("2026-09-21 00:47:49".to_string()),
                status_code: Some(200),
                response_time_ms: Some(408),
            }],
        )
        .expect("assemble report");

        assert_eq!(report["eid"], "E0001-AAA");
        assert_eq!(report["tags"][0], "production");
        assert_eq!(report["qp_data"][0]["request"]["username"], "emilys");
        assert_eq!(report["qp_data"][0]["response"]["id"], 1);
        assert_eq!(
            report["qp_data"][0]["headers"]["response_headers"]["content-type"],
            "application/json"
        );

        fs::remove_dir_all(root).expect("remove sample data");
    }

    #[test]
    fn rejects_paths_as_output_filenames() {
        assert!(safe_filename("report"));
        assert!(!safe_filename("../report"));
        assert!(!safe_filename("folder/report"));
        assert!(!safe_filename(""));
    }
}
