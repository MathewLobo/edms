use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerEvent {
    // ── Existing events ──────────────────────────────────────────────
    ActiveWorkspaceEndpointsLoaded { count: usize },
    ActiveWorkspaceBookmarksLoaded { count: usize },
    CollectionLoaded { collection: String, moved_to_backup: bool },
    HistoryUpdated { count: usize },
    BookmarksUpdated { count: usize },
    FolderBecameActive { folder: String },
    TestStarted { endpoint_id: String, request_number: i32 },
    TestFinished {
        endpoint_id: String,
        request_number: i32,
        status_code: i32,
        response_time_ms: i32,
        response_file: String,
    },
    TestTimeout { endpoint_id: String, request_number: i32 },
    Error { message: String },

    ExportReady { message: String },
    CrudOperationsUpdated { computed_at: String },
    TimerTick {
        endpoint_id: String,
        request_number: i32,
        elapsed_ms: u64,
        remaining_ms: u64,
        limit_ms: u64,
    },

    
    TimerCancelled {
        endpoint_id: String,
        request_number: i32,
        elapsed_ms: u64,
    },

    ViewTagsUpdated { view: String },
}

impl ServerEvent {
    /// Maps this event to its WS Update Code per Ravi's index (2026-09-02),
    /// format `ViewType:[0-9]{2}` e.g. `B01`. UCs are pure triggers, not
    /// data — the frontend counts them and, on manual refresh, looks up
    /// which REST call(s) to make per code (the R2 of R1WR2).
    ///
    /// `route` is which WS connection is broadcasting this — needed because
    /// a few event variants (BookmarksUpdated, HistoryUpdated) are emitted
    /// from more than one part of the backend and don't carry enough on
    /// their own to say which view they belong to. Pass the same short
    /// string each call site already implicitly represents (e.g.
    /// `"bookmarks"`, `"test_view"`, `"dataview"`).
    ///
    /// Returns `None` for:
    /// - events with no assigned code yet (B02/W02/R02 "endpoint segments
    ///   updated" — meaning still unconfirmed with Ravi; webview/repoview
    ///   catalog changes and List View codes — not given yet)
    /// - continuous progress events (TimerTick, TimerCancelled) that
    ///   already carry live data in the payload itself, so they're not UC
    ///   candidates per the "UC = trigger only" rule
    pub fn update_code(&self, route: &str) -> Option<&'static str> {
        match self {
            ServerEvent::ViewTagsUpdated { view } => match view.as_str() {
                "collections" => Some("B01"),
                "webview" => Some("W01"),
                "repoview" => Some("R01"),
                _ => None,
            },
            ServerEvent::BookmarksUpdated { .. } | ServerEvent::CollectionLoaded { .. } => {
                match route {
                    "bookmarks" => Some("B03"),
                    "test_view" => Some("T05"),
                    _ => None,
                }
            }
            ServerEvent::HistoryUpdated { .. } if route == "test_view" => Some("T04"),
            ServerEvent::TestStarted { .. } => Some("T01"),
            ServerEvent::TestFinished { .. } => Some("T02"),
            ServerEvent::TestTimeout { .. } => Some("T03"),
            _ => None,
        }
    }

    /// Builds the actual WS wire message: the existing `{type, event}`
    /// envelope plus the `code` field (`null` when this event has no
    /// assigned UC yet, so the frontend can tell "not coded" apart from
    /// "coded but decode failed").
    pub fn to_ws_message(&self, route: &str) -> serde_json::Value {
        json!({
            "type": "event",
            "event": self,
            "code": self.update_code(route),
        })
    }
}
