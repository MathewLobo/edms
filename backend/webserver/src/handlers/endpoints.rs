use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::db::{self, EndpointDto};
use crate::state::AppState;

const VALID_METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

#[derive(Debug, Deserialize)]
pub struct CreateEndpointRequest {
    #[serde(default)]
    pub endpoint_id: Option<String>,
    pub endpoint_str: String,
    pub annotation: Option<String>,
    /// Optional for now — an endpoint created without one shows up as
    /// unclassified in the CRUD Operations dashboard breakdown.
    pub method: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateEndpointResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
}

pub async fn create_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<CreateEndpointRequest>,
) -> (StatusCode, Json<CreateEndpointResponse>) {
    let method = match payload.method.as_deref().map(str::to_uppercase) {
        Some(m) if VALID_METHODS.contains(&m.as_str()) => Some(m),
        Some(m) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CreateEndpointResponse {
                    success: false,
                    message: format!(
                        "Invalid method '{m}' — must be one of {VALID_METHODS:?}"
                    ),
                    endpoint_id: None,
                }),
            )
        }
        None => None,
    };

    let (eid, was_allocated) = match payload.endpoint_id.as_deref().map(str::trim) {
        None | Some("") => match state.eid_allocator.allocate() {
            Ok(allocated) => (allocated, true),
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(CreateEndpointResponse {
                        success: false,
                        message: format!("Failed to allocate canonical EID: {e:?}"),
                        endpoint_id: None,
                    }),
                );
            }
        },
        Some(custom) => {
            if !compute::eid::is_valid_eid(custom) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(CreateEndpointResponse {
                        success: false,
                        message: format!(
                            "Invalid endpoint_id format '{custom}' — must follow canonical EID format 'E<number>-<suffix>' (e.g. 'E0001-AAA')"
                        ),
                        endpoint_id: None,
                    }),
                );
            }
            if let Err(e) = state.eid_allocator.reserve(custom) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(CreateEndpointResponse {
                        success: false,
                        message: format!("Failed to reserve EID '{custom}': {e:?}"),
                        endpoint_id: None,
                    }),
                );
            }
            (custom.to_string(), false)
        }
    };

    let ep = EndpointDto {
        endpoint_id: eid.clone(),
        endpoint_str: payload.endpoint_str,
        annotation: payload.annotation,
        method,
    };

    match db::insert_endpoint(&state.core, &state.queries, &ep) {
        Ok(_) => {
            state.refresh_dashboard_snapshot();

            (
                StatusCode::CREATED,
                Json(CreateEndpointResponse {
                    success: true,
                    message: format!("Endpoint '{eid}' created"),
                    endpoint_id: Some(eid),
                }),
            )
        }
        Err(e) => {
            if was_allocated {
                let _ = state.eid_allocator.release(&eid);
            }
            let message = if db::is_unique_violation(&e) {
                format!("Endpoint '{eid}' already exists")
            } else {
                format!("Failed to create endpoint: {e:?}")
            };
            (
                StatusCode::BAD_REQUEST,
                Json(CreateEndpointResponse {
                    success: false,
                    message,
                    endpoint_id: None,
                }),
            )
        }
    }
}

use axum::extract::Path;

pub async fn delete_endpoint(
    State(state): State<AppState>,
    Path(endpoint_id): Path<String>,
) -> (StatusCode, Json<CreateEndpointResponse>) {
    match db::delete_endpoint(&state.core, &state.queries, &endpoint_id) {
        Ok(rows) if rows > 0 => {
            let _ = state.eid_allocator.release(&endpoint_id);
            state.refresh_dashboard_snapshot();

            (
                StatusCode::OK,
                Json(CreateEndpointResponse {
                    success: true,
                    message: format!("Endpoint '{endpoint_id}' deleted"),
                    endpoint_id: Some(endpoint_id),
                }),
            )
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(CreateEndpointResponse {
                success: false,
                message: format!("Endpoint '{endpoint_id}' not found"),
                endpoint_id: None,
            }),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(CreateEndpointResponse {
                success: false,
                message: format!("Failed to delete endpoint: {e:?}"),
                endpoint_id: None,
            }),
        ),
    }
}