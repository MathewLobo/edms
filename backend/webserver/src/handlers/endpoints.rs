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

/// Validates a caller-supplied method string, if any, against the allowed
/// set. `None` in, `None` out — method stays optional at the DB layer.
fn validate_method(method: Option<&str>) -> Result<Option<String>, String> {
    match method.map(str::to_uppercase) {
        Some(m) if VALID_METHODS.contains(&m.as_str()) => Ok(Some(m)),
        Some(m) => Err(format!(
            "Invalid method '{m}' — must be one of {VALID_METHODS:?}"
        )),
        None => Ok(None),
    }
}

/// Resolves a caller-supplied (optional) endpoint_id into a concrete
/// canonical EID — allocates a fresh one if none was given, or validates +
/// reserves the given one. Doesn't touch the `endpoints` table itself;
/// callers decide what to do once they have an ID. Returns
/// `(eid, was_freshly_allocated)` — the bool tells the caller whether to
/// release it back to the allocator's gap list on a later failure.
fn resolve_new_eid(state: &AppState, endpoint_id: Option<&str>) -> Result<(String, bool), (StatusCode, String)> {
    match endpoint_id.map(str::trim) {
        None | Some("") => match state.eid_allocator.allocate() {
            Ok(allocated) => Ok((allocated, true)),
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to allocate canonical EID: {e:?}"),
            )),
        },
        Some(custom) => {
            if !compute::eid::is_valid_eid(custom) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Invalid endpoint_id format '{custom}' — must follow canonical EID format 'E<number>-<suffix>' (e.g. 'E0001-AAA')"
                    ),
                ));
            }
            if let Err(e) = state.eid_allocator.reserve(custom) {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to reserve EID '{custom}': {e:?}"),
                ));
            }
            Ok((custom.to_string(), false))
        }
    }
}

pub async fn create_endpoint(
    State(state): State<AppState>,
    Json(payload): Json<CreateEndpointRequest>,
) -> (StatusCode, Json<CreateEndpointResponse>) {
    let method = match validate_method(payload.method.as_deref()) {
        Ok(m) => m,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CreateEndpointResponse {
                    success: false,
                    message,
                    endpoint_id: None,
                }),
            )
        }
    };

    let (eid, was_allocated) = match resolve_new_eid(&state, payload.endpoint_id.as_deref()) {
        Ok(pair) => pair,
        Err((status, message)) => {
            return (
                status,
                Json(CreateEndpointResponse {
                    success: false,
                    message,
                    endpoint_id: None,
                }),
            )
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

/// Used by Test View's run-test flow — per Mathew (2026-09-08), an
/// endpoint is only created the moment it's actually tested, not via a
/// separate manual step. Unlike `create_endpoint` (an explicit
/// create-only action that correctly rejects a duplicate ID), this is a
/// true get-or-create: re-testing an endpoint that already exists by ID
/// is the expected common case, not an error.
///
/// Not wrapped in spawn_blocking, matching create_endpoint's existing
/// convention above — callers running on the async runtime should wrap
/// this themselves if that becomes a problem in practice.
pub async fn get_or_create_endpoint(
    state: &AppState,
    endpoint_id: Option<&str>,
    endpoint_str: &str,
    method: Option<String>,
    annotation: Option<String>,
) -> Result<(EndpointDto, bool), (StatusCode, String)> {
    if let Some(id) = endpoint_id.map(str::trim).filter(|s| !s.is_empty()) {
        match db::get_endpoint(&state.core, &state.queries, id) {
            Ok(Some(existing)) => return Ok((existing, false)),
            Ok(None) => {} // doesn't exist yet — fall through and create it with this exact id
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to look up endpoint '{id}': {e:?}"),
                ))
            }
        }
    }

    let (eid, was_allocated) = resolve_new_eid(state, endpoint_id)?;
    let ep = EndpointDto {
        endpoint_id: eid.clone(),
        endpoint_str: endpoint_str.to_string(),
        annotation,
        method,
    };

    match db::insert_endpoint(&state.core, &state.queries, &ep) {
        Ok(_) => Ok((ep, true)),
        Err(e) => {
            if was_allocated {
                let _ = state.eid_allocator.release(&eid);
            }
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create endpoint '{eid}': {e:?}"),
            ))
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