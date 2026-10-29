//! Status codes for the dev API (`/hashedtokens/edms/api/history/` and
//! `/hashedtokens/edms/api/collections/`).
//!
//! A call can mean a lot more than success or fail: it can carry several QPs,
//! some of them stored and some of them blocked as duplicates, and it can
//! create an endpoint or a collection on the way. So the result is one
//! **overall** code for the call plus codes for the **endpoint**, the
//! **collection** and each **QP**. This file is the one index of those codes
//! and their messages; the webserver routes and the compute ops (history and
//! collections) both use it, so neither side invents its own.
//!
//! Numbers are grouped by what happened:
//!
//! | Range  | Outcome   | Meaning                                              |
//! |--------|-----------|------------------------------------------------------|
//! | 1xxx   | success   | stored what was asked                                |
//! | 2xxx   | partial   | nothing failed, but not everything asked was stored  |
//! | 4xxx   | rejected  | the request itself is wrong, nothing was attempted   |
//! | 5xxx   | failed    | the request was fine, the work did not complete      |
//!
//! Codes are never reused or renumbered. Add new ones at the end of their
//! group.

use serde::de::Deserializer;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};

/// What kind of thing a code describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The whole call (`overall`).
    Call,
    /// One QP of the call.
    Qp,
    /// The endpoint the QPs belong to.
    Endpoint,
    /// The collection the call targets.
    Collection,
}

/// The group a code belongs to, taken from its thousands digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Partial,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiStatus {
    // ── Call ─────────────────────────────────────────────────────────
    Ok,
    PartialSuccess,
    NothingNew,
    InvalidRequest,
    MissingParam,
    UnknownOp,
    InvalidParam,
    TooManyQps,
    InternalError,
    NothingStored,
    ComputeUnavailable,
    ComputeStalled,
    // ── QP ───────────────────────────────────────────────────────────
    QpInserted,
    QpDuplicateBlocked,
    QpInvalid,
    QpTestFailed,
    QpTestTimedOut,
    // ── Endpoint ─────────────────────────────────────────────────────
    EndpointCreated,
    EndpointReused,
    // ── Collection ───────────────────────────────────────────────────
    CollectionCreated,
    CollectionReused,
    AddedToCollection,
    AlreadyInCollection,
}

struct Row {
    code: u16,
    name: &'static str,
    scope: Scope,
    message: &'static str,
}

const fn row(code: u16, name: &'static str, scope: Scope, message: &'static str) -> Row {
    Row { code, name, scope, message }
}

impl ApiStatus {
    /// Every code, in numeric order. Keep in step with the enum (the
    /// `all_lists_every_code` test covers the numbers, `row` covers the rest).
    pub const ALL: &'static [ApiStatus] = &[
        ApiStatus::Ok,
        ApiStatus::PartialSuccess,
        ApiStatus::NothingNew,
        ApiStatus::QpInserted,
        ApiStatus::EndpointCreated,
        ApiStatus::EndpointReused,
        ApiStatus::CollectionCreated,
        ApiStatus::CollectionReused,
        ApiStatus::AddedToCollection,
        ApiStatus::AlreadyInCollection,
        ApiStatus::QpDuplicateBlocked,
        ApiStatus::InvalidRequest,
        ApiStatus::MissingParam,
        ApiStatus::UnknownOp,
        ApiStatus::InvalidParam,
        ApiStatus::TooManyQps,
        ApiStatus::QpInvalid,
        ApiStatus::InternalError,
        ApiStatus::NothingStored,
        ApiStatus::ComputeUnavailable,
        ApiStatus::ComputeStalled,
        ApiStatus::QpTestFailed,
        ApiStatus::QpTestTimedOut,
    ];

    fn row(self) -> Row {
        use Scope::*;
        match self {
            // 1xxx — stored what was asked
            ApiStatus::Ok => row(1000, "OK", Call, "Every QP was stored"),
            ApiStatus::QpInserted => row(1010, "QP_INSERTED", Qp, "QP stored"),
            ApiStatus::EndpointCreated => row(1020, "ENDPOINT_CREATED", Endpoint, "A new endpoint was created"),
            ApiStatus::EndpointReused => row(1021, "ENDPOINT_REUSED", Endpoint, "The endpoint already existed and was reused"),
            ApiStatus::CollectionCreated => row(1030, "COLLECTION_CREATED", Collection, "A new collection was created"),
            ApiStatus::CollectionReused => row(1031, "COLLECTION_REUSED", Collection, "The collection already existed and was reused"),
            ApiStatus::AddedToCollection => row(1032, "ADDED_TO_COLLECTION", Collection, "The endpoint was added to the collection"),
            // 2xxx — nothing failed, not everything stored
            ApiStatus::PartialSuccess => row(2000, "PARTIAL_SUCCESS", Call, "Some QPs were stored and some were not; see each QP's code"),
            ApiStatus::NothingNew => row(2001, "NOTHING_NEW", Call, "Every QP was a duplicate of one already stored; nothing was written"),
            ApiStatus::QpDuplicateBlocked => row(2010, "QP_DUPLICATE_BLOCKED", Qp, "An identical QP is already stored; it was not added again"),
            ApiStatus::AlreadyInCollection => row(2030, "ALREADY_IN_COLLECTION", Collection, "The endpoint was already in the collection"),
            // 4xxx — the request is wrong, nothing was attempted
            ApiStatus::InvalidRequest => row(4000, "INVALID_REQUEST", Call, "The request body is not valid JSON or not an object"),
            ApiStatus::MissingParam => row(4001, "MISSING_PARAM", Call, "A mandatory parameter is missing"),
            ApiStatus::UnknownOp => row(4002, "UNKNOWN_OP", Call, "The ops value is not one this endpoint supports"),
            ApiStatus::InvalidParam => row(4003, "INVALID_PARAM", Call, "A parameter has a value that is not allowed"),
            ApiStatus::TooManyQps => row(4004, "TOO_MANY_QPS", Call, "The call carries more QPs than one call may"),
            ApiStatus::QpInvalid => row(4010, "QP_INVALID", Qp, "The QP data is not usable"),
            // 5xxx — the request was fine, the work did not complete
            ApiStatus::InternalError => row(5000, "INTERNAL_ERROR", Call, "The server failed while handling the call"),
            ApiStatus::NothingStored => row(5001, "NOTHING_STORED", Call, "No QP was stored and at least one failed; see each QP's code"),
            ApiStatus::ComputeUnavailable => row(5002, "COMPUTE_UNAVAILABLE", Call, "The compute process could not be started"),
            ApiStatus::ComputeStalled => row(5003, "COMPUTE_STALLED", Call, "The compute process never reported back"),
            ApiStatus::QpTestFailed => row(5010, "QP_TEST_FAILED", Qp, "The test request failed, so the QP was not stored"),
            ApiStatus::QpTestTimedOut => row(5011, "QP_TEST_TIMED_OUT", Qp, "The test request timed out, so the QP was not stored"),
        }
    }

    pub fn code(self) -> u16 {
        self.row().code
    }

    /// Stable machine-readable name, e.g. `QP_DUPLICATE_BLOCKED`.
    pub fn name(self) -> &'static str {
        self.row().name
    }

    pub fn message(self) -> &'static str {
        self.row().message
    }

    pub fn scope(self) -> Scope {
        self.row().scope
    }

    pub fn outcome(self) -> Outcome {
        match self.code() / 1000 {
            1 => Outcome::Success,
            2 => Outcome::Partial,
            4 => Outcome::Rejected,
            _ => Outcome::Failed,
        }
    }

    pub fn from_code(code: u16) -> Option<ApiStatus> {
        ApiStatus::ALL.iter().copied().find(|s| s.code() == code)
    }

    /// The HTTP status a route answers with when it must reply directly
    /// instead of `202`: a bad request, or a failure before any work started.
    /// `None` for codes that only ever arrive in the WebSocket result, since
    /// by then the HTTP call was already answered.
    pub fn http_status(self) -> Option<u16> {
        match self {
            ApiStatus::InvalidRequest
            | ApiStatus::MissingParam
            | ApiStatus::UnknownOp
            | ApiStatus::InvalidParam
            | ApiStatus::TooManyQps => Some(400),
            ApiStatus::InternalError => Some(500),
            ApiStatus::ComputeUnavailable => Some(503),
            _ => None,
        }
    }
}

/// On the wire a status is `{code, name, message}`. Reading accepts that or
/// just the number, so a compute op can return `1010` and be understood.
impl Serialize for ApiStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut st = serializer.serialize_struct("ApiStatus", 3)?;
        st.serialize_field("code", &self.code())?;
        st.serialize_field("name", self.name())?;
        st.serialize_field("message", self.message())?;
        st.end()
    }
}

impl<'de> Deserialize<'de> for ApiStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Number(u16),
            Object { code: u16 },
        }
        let code = match Wire::deserialize(deserializer)? {
            Wire::Number(c) | Wire::Object { code: c } => c,
        };
        ApiStatus::from_code(code)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown API status code {code}")))
    }
}

/// What happened to one QP of the call. `index` is its position in the
/// request's `qps` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QpOutcome {
    pub index: usize,
    pub status: ApiStatus,
    /// The QP number it was stored as (`QpInserted`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_number: Option<i32>,
    /// The QP it duplicates (`QpDuplicateBlocked`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<i32>,
    /// Extra text for the person reading the log (what was wrong, the error).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl QpOutcome {
    pub fn new(index: usize, status: ApiStatus) -> Self {
        QpOutcome { index, status, request_number: None, duplicate_of: None, detail: None }
    }
}

/// The whole result of a call, as delivered over the WebSocket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallResult {
    pub status: ApiStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<ApiStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<ApiStatus>,
    pub qps: Vec<QpOutcome>,
}

impl CallResult {
    /// Builds the result with the overall code worked out from the QPs.
    pub fn from_qps(
        endpoint: Option<ApiStatus>,
        collection: Option<ApiStatus>,
        qps: Vec<QpOutcome>,
    ) -> Self {
        CallResult { status: overall(&qps), endpoint, collection, qps }
    }
}

/// The one overall code for a call, from what happened to each QP:
///
/// - every QP stored -> `Ok`
/// - some stored, some not -> `PartialSuccess`
/// - none stored, all of them duplicates -> `NothingNew`
/// - none stored, all of them unusable -> `InvalidParam`
/// - none stored otherwise (a test failed or timed out) -> `NothingStored`
///
/// A call with no QPs at all is `MissingParam`.
pub fn overall(qps: &[QpOutcome]) -> ApiStatus {
    if qps.is_empty() {
        return ApiStatus::MissingParam;
    }
    let inserted = qps.iter().filter(|q| q.status == ApiStatus::QpInserted).count();
    if inserted == qps.len() {
        return ApiStatus::Ok;
    }
    if inserted > 0 {
        return ApiStatus::PartialSuccess;
    }
    if qps.iter().all(|q| q.status == ApiStatus::QpDuplicateBlocked) {
        ApiStatus::NothingNew
    } else if qps.iter().all(|q| q.status == ApiStatus::QpInvalid) {
        ApiStatus::InvalidParam
    } else {
        ApiStatus::NothingStored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn qp(index: usize, status: ApiStatus) -> QpOutcome {
        QpOutcome::new(index, status)
    }

    #[test]
    fn all_lists_every_code() {
        // Each code appears once, in numeric order, and codes and names are unique.
        let codes: Vec<u16> = ApiStatus::ALL.iter().map(|s| s.code()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes.len(), sorted.len(), "a code appears twice");
        let names: HashSet<&str> = ApiStatus::ALL.iter().map(|s| s.name()).collect();
        assert_eq!(names.len(), ApiStatus::ALL.len(), "a name appears twice");
        assert_eq!(ApiStatus::ALL.len(), 23, "update the count when adding a code");
    }

    #[test]
    fn every_code_round_trips() {
        for s in ApiStatus::ALL {
            assert_eq!(ApiStatus::from_code(s.code()), Some(*s));
        }
        assert_eq!(ApiStatus::from_code(9999), None);
    }

    #[test]
    fn outcome_follows_the_thousands_digit() {
        assert_eq!(ApiStatus::Ok.outcome(), Outcome::Success);
        assert_eq!(ApiStatus::QpDuplicateBlocked.outcome(), Outcome::Partial);
        assert_eq!(ApiStatus::MissingParam.outcome(), Outcome::Rejected);
        assert_eq!(ApiStatus::QpTestTimedOut.outcome(), Outcome::Failed);
        for s in ApiStatus::ALL {
            assert!(!s.message().is_empty());
        }
    }

    #[test]
    fn only_codes_a_route_can_answer_with_have_an_http_status() {
        assert_eq!(ApiStatus::UnknownOp.http_status(), Some(400));
        assert_eq!(ApiStatus::TooManyQps.http_status(), Some(400));
        assert_eq!(ApiStatus::ComputeUnavailable.http_status(), Some(503));
        assert_eq!(ApiStatus::InternalError.http_status(), Some(500));
        assert_eq!(ApiStatus::QpInserted.http_status(), None);
        assert_eq!(ApiStatus::ComputeStalled.http_status(), None);
    }

    #[test]
    fn overall_ok_when_every_qp_is_stored() {
        let qps = vec![qp(0, ApiStatus::QpInserted), qp(1, ApiStatus::QpInserted)];
        assert_eq!(overall(&qps), ApiStatus::Ok);
    }

    #[test]
    fn overall_partial_when_some_are_stored() {
        let dup = vec![qp(0, ApiStatus::QpInserted), qp(1, ApiStatus::QpDuplicateBlocked)];
        assert_eq!(overall(&dup), ApiStatus::PartialSuccess);
        let failed = vec![qp(0, ApiStatus::QpTestFailed), qp(1, ApiStatus::QpInserted)];
        assert_eq!(overall(&failed), ApiStatus::PartialSuccess);
    }

    #[test]
    fn overall_when_nothing_is_stored() {
        let dups = vec![qp(0, ApiStatus::QpDuplicateBlocked), qp(1, ApiStatus::QpDuplicateBlocked)];
        assert_eq!(overall(&dups), ApiStatus::NothingNew);
        let invalid = vec![qp(0, ApiStatus::QpInvalid)];
        assert_eq!(overall(&invalid), ApiStatus::InvalidParam);
        let timed_out = vec![qp(0, ApiStatus::QpTestTimedOut)];
        assert_eq!(overall(&timed_out), ApiStatus::NothingStored);
        let mixed = vec![qp(0, ApiStatus::QpDuplicateBlocked), qp(1, ApiStatus::QpTestFailed)];
        assert_eq!(overall(&mixed), ApiStatus::NothingStored);
        assert_eq!(overall(&[]), ApiStatus::MissingParam);
    }

    #[test]
    fn a_status_serializes_with_code_name_and_message() {
        let v = serde_json::to_value(ApiStatus::QpDuplicateBlocked).unwrap();
        assert_eq!(v["code"], 2010);
        assert_eq!(v["name"], "QP_DUPLICATE_BLOCKED");
        assert!(v["message"].as_str().unwrap().contains("identical"));
    }

    #[test]
    fn a_status_reads_from_a_number_or_an_object() {
        let from_number: ApiStatus = serde_json::from_value(json!(1010)).unwrap();
        assert_eq!(from_number, ApiStatus::QpInserted);
        let from_object: ApiStatus = serde_json::from_value(json!({"code": 2000})).unwrap();
        assert_eq!(from_object, ApiStatus::PartialSuccess);
        assert!(serde_json::from_value::<ApiStatus>(json!(1)).is_err());
    }

    #[test]
    fn a_call_result_has_the_shape_clients_read() {
        let mut stored = qp(0, ApiStatus::QpInserted);
        stored.request_number = Some(3);
        let mut dup = qp(1, ApiStatus::QpDuplicateBlocked);
        dup.duplicate_of = Some(1);
        let result = CallResult::from_qps(
            Some(ApiStatus::EndpointReused),
            Some(ApiStatus::CollectionCreated),
            vec![stored, dup],
        );
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(v["status"]["code"], 2000);
        assert_eq!(v["endpoint"]["code"], 1021);
        assert_eq!(v["collection"]["code"], 1030);
        assert_eq!(v["qps"][0]["request_number"], 3);
        assert_eq!(v["qps"][1]["duplicate_of"], 1);
        assert!(v["qps"][0].get("duplicate_of").is_none());
        let back: CallResult = serde_json::from_value(v).unwrap();
        assert_eq!(back, result);
    }
}
