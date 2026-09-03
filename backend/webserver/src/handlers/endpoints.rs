use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::db::{self, EndpointDto};
use crate::state::AppState;

const VALID_METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

#[derive(Debug, Deserialize)]
pub struct CreateEndpointRequest {
    pub endpoint_id: String,
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
                }),
            )
        }
        None => None,
    };

    let ep = EndpointDto {
        endpoint_id: payload.endpoint_id.clone(),
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
                    message: format!("Endpoint '{}' created", payload.endpoint_id),
                }),
            )
        }
        Err(e) => {
            let message = if db::is_unique_violation(&e) {
                format!("Endpoint '{}' already exists", payload.endpoint_id)
            } else {
                format!("Failed to create endpoint: {e:?}")
            };
            (
                StatusCode::BAD_REQUEST,
                Json(CreateEndpointResponse {
                    success: false,
                    message,
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
            state.refresh_dashboard_snapshot();

            (
                StatusCode::OK,
                Json(CreateEndpointResponse {
                    success: true,
                    message: format!("Endpoint '{endpoint_id}' deleted"),
                }),
            )
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(CreateEndpointResponse {
                success: false,
                message: format!("Endpoint '{endpoint_id}' not found"),
            }),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(CreateEndpointResponse {
                success: false,
                message: format!("Failed to delete endpoint: {e:?}"),
            }),
        ),
    }
}