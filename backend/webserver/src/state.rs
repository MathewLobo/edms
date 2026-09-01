use crate::config::AppConfig;
use crate::events::ServerEvent;
use crate::timer::TimerHandle;
use edms::core::EdmsCore;
use edms::query_loader::QueryMap;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<EdmsCore>,
    pub queries: Arc<QueryMap>,
    pub events_tx: broadcast::Sender<ServerEvent>,
    pub active_folder: Arc<RwLock<Option<String>>>,
    pub dashboard_conn: Arc<Mutex<Connection>>,
    pub db_path: PathBuf,
    pub storage_root: PathBuf,
    pub started_at: String,
    pub config: Arc<AppConfig>,
    /// Timer handles for in-flight test runs, keyed by (endpoint_id,
    /// request_number) — so whichever code path learns the test's real
    /// outcome (success or timeout, in callback.rs) can cancel the app's
    /// own independent countdown instead of letting it keep ticking or
    /// fire a second, uncoordinated TestTimeout on its own schedule.
    pub active_timers: Arc<Mutex<HashMap<(String, i32), TimerHandle>>>,
}

impl AppState {
    pub fn new(
        core: Arc<EdmsCore>,
        queries: Arc<QueryMap>,
        dashboard_conn: Connection,
        db_path: PathBuf,
        storage_root: PathBuf,
        config: Arc<AppConfig>,
    ) -> Self {
        let (events_tx, _) = broadcast::channel(256);
        Self {
            core,
            queries,
            events_tx,
            active_folder: Arc::new(RwLock::new(None)),
            dashboard_conn: Arc::new(Mutex::new(dashboard_conn)),
            db_path,
            storage_root,
            started_at: chrono::Utc::now().to_rfc3339(),
            config,
            active_timers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Removes and cancels the timer for (endpoint_id, request_number), if
    /// one is still running. Call this once the test's real outcome is
    /// known — a no-op if the timer already fired and removed itself.
    pub fn cancel_timer(&self, endpoint_id: &str, request_number: i32) {
        let handle = self
            .active_timers
            .lock()
            .unwrap()
            .remove(&(endpoint_id.to_string(), request_number));
        if let Some(handle) = handle {
            handle.cancel();
        }
    }

    pub async fn emit(&self, evt: ServerEvent) {
        let _ = self.events_tx.send(evt);
    }

    /// Takes a new dashboard snapshot using the current state's paths.
    /// Call this after any change that should be reflected immediately
    /// (e.g. a new endpoint being created).
   pub fn refresh_dashboard_snapshot(&self) {
        let conn = self.dashboard_conn.lock().unwrap();
        match crate::dashboard_db::take_snapshot(&conn, &self.db_path, &self.storage_root) {
            Ok(snapshot) => {
                tracing::info!("Dashboard snapshot refreshed: {:?}", snapshot);
            }
            Err(e) => {
                tracing::warn!("Failed to refresh dashboard snapshot: {e}");
            }
        }

        match crate::dashboard_db::rotate_old_snapshots(&conn) {
            Ok(deleted) if deleted > 0 => {
                tracing::info!("Rotated {deleted} snapshot(s) older than 30 days");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Failed to rotate old snapshots: {e}");
            }
        }
    }
}