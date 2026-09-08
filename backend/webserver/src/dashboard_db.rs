use rusqlite::{Connection, OptionalExtension, Row};
use std::fs;
use std::path::Path;
use chrono::Utc;
use serde::Serialize;
use tracing::info;

const SNAPSHOT_COLUMNS: &str = "snapshot_time, endpoint_count, bookmark_count, unique_tag_count, \
    total_tag_count, endpoint_last_updated, sqlite_size_mb, storage_path, storage_size_mb, file_count";

pub const DASHBOARD_FOLDER: &str = "dashboard";
pub const DASHBOARD_DB_FILE: &str = "dashboard.db";

#[derive(Debug, Serialize)]
pub struct DashboardSnapshot {
    pub snapshot_time: String,
    pub endpoint_count: i64,
    pub bookmark_count: i64,
    pub unique_tag_count: i64,
    pub total_tag_count: i64,
    pub endpoint_last_updated: Option<String>,
    pub sqlite_size_mb: f64,
    pub storage_path: String,
    pub storage_size_mb: f64,
    pub file_count: i64,
}

/// Creates the `dashboard/` folder (if missing) and opens/initializes
/// `dashboard.db` inside it, ensuring the snapshots table exists.
pub fn init_dashboard_db(app_root: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    let dashboard_dir = app_root.join(DASHBOARD_FOLDER);

    if !dashboard_dir.exists() {
        fs::create_dir_all(&dashboard_dir)?;
        info!(path = %dashboard_dir.display(), "Created dashboard folder");
    }

    let db_path = dashboard_dir.join(DASHBOARD_DB_FILE);
    let conn = Connection::open(&db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS dashboard_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            snapshot_time TEXT NOT NULL,
            endpoint_count INTEGER NOT NULL,
            bookmark_count INTEGER NOT NULL,
            unique_tag_count INTEGER NOT NULL,
            total_tag_count INTEGER NOT NULL,
            endpoint_last_updated TEXT,
            sqlite_size_mb REAL NOT NULL,
            storage_path TEXT NOT NULL,
            storage_size_mb REAL NOT NULL,
            file_count INTEGER NOT NULL
        )",
        [],
    )?;

    // One row per calendar day, captured by the date-change watcher when the
    // date actually rolls over — not derived from dashboard_snapshots, since
    // a quiet day with zero transactions would otherwise have no reference
    // point. Used for day-over-day comparison (e.g. day 1 vs day 6), not the
    // live dashboard. Same 30-day rotation policy as dashboard_snapshots.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS daily_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            snapshot_date TEXT NOT NULL UNIQUE,
            captured_at TEXT NOT NULL,
            endpoint_count INTEGER NOT NULL,
            bookmark_count INTEGER NOT NULL,
            unique_tag_count INTEGER NOT NULL,
            total_tag_count INTEGER NOT NULL,
            endpoint_last_updated TEXT,
            sqlite_size_mb REAL NOT NULL,
            storage_path TEXT NOT NULL,
            storage_size_mb REAL NOT NULL,
            file_count INTEGER NOT NULL
        )",
        [],
    )?;

    // Holds the latest CRUD Operations breakdown (Ravi's [1] approach —
    // global, refresh-triggered computation via a compute child process,
    // not a per-transaction update). Cleared and repopulated on every
    // refresh, so this only ever holds one "generation" of data at a time.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS crud_operations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            computed_at TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            method TEXT NOT NULL,
            count INTEGER NOT NULL
        )",
        [],
    )?;

    info!(path = %db_path.display(), "Dashboard DB ready");
    Ok(conn)
}

/// Recursively computes total size (in bytes) and file count for a folder.
fn folder_stats(path: &Path) -> (u64, i64) {
    let mut total_size = 0u64;
    let mut file_count = 0i64;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let (s, c) = folder_stats(&p);
                total_size += s;
                file_count += c;
            } else if let Ok(meta) = entry.metadata() {
                total_size += meta.len();
                file_count += 1;
            }
        }
    }

    (total_size, file_count)
}

/// The numbers that go into both the per-transaction snapshot and the daily
/// snapshot — same computation, two different write triggers/tables.
struct CurrentStats {
    endpoint_count: i64,
    bookmark_count: i64,
    unique_tag_count: i64,
    total_tag_count: i64,
    endpoint_last_updated: Option<String>,
    sqlite_size_mb: f64,
    storage_size_mb: f64,
    file_count: i64,
}

fn compute_current_stats(
    edms_db_path: &Path,
    storage_root: &Path,
) -> Result<CurrentStats, Box<dyn std::error::Error + Send + Sync>> {
    let edms_conn = Connection::open(edms_db_path)?;
    // Match EdmsBase::connect()'s pragmas — without these this ad-hoc
    // connection was found (2026-09-08) to leave edms.db persistently
    // locked against every other connection (state.core's shared one
    // included) after the very first call, on Windows: no busy_timeout
    // meant zero retry tolerance, and this read connection wasn't
    // dropped fast enough relative to a concurrent writer to avoid
    // wedging the file's lock state. Reachable via plain
    // POST /endpoints/create — pre-existing, not new to this session.
    edms_conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA busy_timeout = 5000;"
    )?;

    let endpoint_count: i64 =
        edms_conn.query_row("SELECT COUNT(*) FROM endpoints", [], |r| r.get(0))?;

    let bookmark_count: i64 =
        edms_conn.query_row("SELECT COUNT(*) FROM bookmarks", [], |r| r.get(0))?;

    let unique_tag_count: i64 = edms_conn
        .query_row("SELECT COUNT(DISTINCT tag) FROM tags", [], |r| r.get(0))
        .unwrap_or(0);

    let total_tag_count: i64 = edms_conn
        .query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0))
        .unwrap_or(0);

    let endpoint_last_updated: Option<String> = edms_conn
        .query_row("SELECT MAX(updated_at) FROM endpoints", [], |r| r.get(0))
        .unwrap_or(None);

    let sqlite_size_mb = fs::metadata(edms_db_path)
        .map(|m| m.len() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0);

    let (storage_bytes, file_count) = folder_stats(storage_root);
    let storage_size_mb = storage_bytes as f64 / (1024.0 * 1024.0);

    Ok(CurrentStats {
        endpoint_count,
        bookmark_count,
        unique_tag_count,
        total_tag_count,
        endpoint_last_updated,
        sqlite_size_mb,
        storage_size_mb,
        file_count,
    })
}

/// Gathers current stats from the main edms.db plus filesystem stats,
/// and inserts one snapshot row.
pub fn take_snapshot(
    dashboard_conn: &Connection,
    edms_db_path: &Path,
    storage_root: &Path,
) -> Result<DashboardSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let stats = compute_current_stats(edms_db_path, storage_root)?;
    let snapshot_time = Utc::now().to_rfc3339();

    let snapshot = DashboardSnapshot {
        snapshot_time,
        endpoint_count: stats.endpoint_count,
        bookmark_count: stats.bookmark_count,
        unique_tag_count: stats.unique_tag_count,
        total_tag_count: stats.total_tag_count,
        endpoint_last_updated: stats.endpoint_last_updated,
        sqlite_size_mb: stats.sqlite_size_mb,
        storage_path: storage_root.display().to_string(),
        storage_size_mb: stats.storage_size_mb,
        file_count: stats.file_count,
    };

    dashboard_conn.execute(
        "INSERT INTO dashboard_snapshots (
            snapshot_time, endpoint_count, bookmark_count, unique_tag_count,
            total_tag_count, endpoint_last_updated, sqlite_size_mb,
            storage_path, storage_size_mb, file_count
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            snapshot.snapshot_time,
            snapshot.endpoint_count,
            snapshot.bookmark_count,
            snapshot.unique_tag_count,
            snapshot.total_tag_count,
            snapshot.endpoint_last_updated,
            snapshot.sqlite_size_mb,
            snapshot.storage_path,
            snapshot.storage_size_mb,
            snapshot.file_count,
        ],
    )?;

    Ok(snapshot)
}

/// Deletes snapshot rows older than 30 days.
pub fn rotate_old_snapshots(dashboard_conn: &Connection) -> Result<usize, rusqlite::Error> {
    dashboard_conn.execute(
        "DELETE FROM dashboard_snapshots WHERE snapshot_time < datetime('now', '-30 days')",
        [],
    )
}

fn row_to_snapshot(row: &Row) -> rusqlite::Result<DashboardSnapshot> {
    Ok(DashboardSnapshot {
        snapshot_time: row.get(0)?,
        endpoint_count: row.get(1)?,
        bookmark_count: row.get(2)?,
        unique_tag_count: row.get(3)?,
        total_tag_count: row.get(4)?,
        endpoint_last_updated: row.get(5)?,
        sqlite_size_mb: row.get(6)?,
        storage_path: row.get(7)?,
        storage_size_mb: row.get(8)?,
        file_count: row.get(9)?,
    })
}

/// Returns the most recently taken snapshot, if any exist yet.
pub fn get_latest_snapshot(
    dashboard_conn: &Connection,
) -> rusqlite::Result<Option<DashboardSnapshot>> {
    dashboard_conn
        .query_row(
            &format!("SELECT {SNAPSHOT_COLUMNS} FROM dashboard_snapshots ORDER BY id DESC LIMIT 1"),
            [],
            row_to_snapshot,
        )
        .optional()
}

/// Returns all retained snapshots (rolling 30-day window), oldest first —
/// suitable for a trend chart.
pub fn get_snapshot_history(dashboard_conn: &Connection) -> rusqlite::Result<Vec<DashboardSnapshot>> {
    let mut stmt = dashboard_conn
        .prepare(&format!("SELECT {SNAPSHOT_COLUMNS} FROM dashboard_snapshots ORDER BY id ASC"))?;
    let rows = stmt.query_map([], row_to_snapshot)?;
    rows.collect()
}

/* ---------------- Daily snapshots (for day-over-day comparison) ---------------- */

#[derive(Debug, Serialize)]
pub struct DailySnapshot {
    pub snapshot_date: String,
    pub captured_at: String,
    pub endpoint_count: i64,
    pub bookmark_count: i64,
    pub unique_tag_count: i64,
    pub total_tag_count: i64,
    pub endpoint_last_updated: Option<String>,
    pub sqlite_size_mb: f64,
    pub storage_path: String,
    pub storage_size_mb: f64,
    pub file_count: i64,
}

const DAILY_COLUMNS: &str = "snapshot_date, captured_at, endpoint_count, bookmark_count, \
    unique_tag_count, total_tag_count, endpoint_last_updated, sqlite_size_mb, storage_path, \
    storage_size_mb, file_count";

fn row_to_daily_snapshot(row: &Row) -> rusqlite::Result<DailySnapshot> {
    Ok(DailySnapshot {
        snapshot_date: row.get(0)?,
        captured_at: row.get(1)?,
        endpoint_count: row.get(2)?,
        bookmark_count: row.get(3)?,
        unique_tag_count: row.get(4)?,
        total_tag_count: row.get(5)?,
        endpoint_last_updated: row.get(6)?,
        sqlite_size_mb: row.get(7)?,
        storage_path: row.get(8)?,
        storage_size_mb: row.get(9)?,
        file_count: row.get(10)?,
    })
}

/// Captures (or overwrites, if one already exists for that date — e.g. the
/// app restarted the same day) the daily snapshot for `date` ("YYYY-MM-DD").
/// Called by the date-change watcher for the day that just ended, not the
/// day that just started.
pub fn take_daily_snapshot(
    dashboard_conn: &Connection,
    edms_db_path: &Path,
    storage_root: &Path,
    date: &str,
) -> Result<DailySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let stats = compute_current_stats(edms_db_path, storage_root)?;
    let captured_at = Utc::now().to_rfc3339();

    let snapshot = DailySnapshot {
        snapshot_date: date.to_string(),
        captured_at,
        endpoint_count: stats.endpoint_count,
        bookmark_count: stats.bookmark_count,
        unique_tag_count: stats.unique_tag_count,
        total_tag_count: stats.total_tag_count,
        endpoint_last_updated: stats.endpoint_last_updated,
        sqlite_size_mb: stats.sqlite_size_mb,
        storage_path: storage_root.display().to_string(),
        storage_size_mb: stats.storage_size_mb,
        file_count: stats.file_count,
    };

    dashboard_conn.execute(
        "INSERT INTO daily_snapshots (
            snapshot_date, captured_at, endpoint_count, bookmark_count, unique_tag_count,
            total_tag_count, endpoint_last_updated, sqlite_size_mb, storage_path,
            storage_size_mb, file_count
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(snapshot_date) DO UPDATE SET
            captured_at = excluded.captured_at,
            endpoint_count = excluded.endpoint_count,
            bookmark_count = excluded.bookmark_count,
            unique_tag_count = excluded.unique_tag_count,
            total_tag_count = excluded.total_tag_count,
            endpoint_last_updated = excluded.endpoint_last_updated,
            sqlite_size_mb = excluded.sqlite_size_mb,
            storage_path = excluded.storage_path,
            storage_size_mb = excluded.storage_size_mb,
            file_count = excluded.file_count",
        rusqlite::params![
            snapshot.snapshot_date,
            snapshot.captured_at,
            snapshot.endpoint_count,
            snapshot.bookmark_count,
            snapshot.unique_tag_count,
            snapshot.total_tag_count,
            snapshot.endpoint_last_updated,
            snapshot.sqlite_size_mb,
            snapshot.storage_path,
            snapshot.storage_size_mb,
            snapshot.file_count,
        ],
    )?;

    Ok(snapshot)
}

/// Deletes daily snapshot rows older than 30 days — same rolling window as
/// dashboard_snapshots, per the original spec ("hardcoded" 30-day retention).
pub fn rotate_old_daily_snapshots(dashboard_conn: &Connection) -> Result<usize, rusqlite::Error> {
    dashboard_conn.execute(
        "DELETE FROM daily_snapshots WHERE snapshot_date < date('now', '-30 days')",
        [],
    )
}

/// Returns the most recent date with a captured daily snapshot, if any
/// exist yet. Used at startup so the date-change watcher remembers what it
/// already knows across restarts, instead of blindly trusting the current
/// wall-clock date — that's what lets a restart detect (and at least log)
/// a gap left by downtime spanning a missed rollover, rather than silently
/// losing track of it.
pub fn latest_daily_snapshot_date(dashboard_conn: &Connection) -> rusqlite::Result<Option<String>> {
    dashboard_conn.query_row("SELECT MAX(snapshot_date) FROM daily_snapshots", [], |row| row.get(0))
}

/// Looks up the daily snapshot for one exact date ("YYYY-MM-DD"). `None` if
/// no snapshot was captured for that date — either it's outside the 30-day
/// retention window, or it hasn't happened yet (today/future), or the app
/// wasn't running at that day's rollover.
pub fn get_daily_snapshot(
    dashboard_conn: &Connection,
    date: &str,
) -> rusqlite::Result<Option<DailySnapshot>> {
    dashboard_conn
        .query_row(
            &format!("SELECT {DAILY_COLUMNS} FROM daily_snapshots WHERE snapshot_date = ?1"),
            [date],
            row_to_daily_snapshot,
        )
        .optional()
}

/* ---------------- CRUD Operations (Ravi's global/refresh-triggered approach) ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct CrudOperationsRow {
    pub entity_type: String,
    pub method: String,
    pub count: i64,
}

/// Replaces the entire crud_operations table with a fresh set of rows —
/// this always holds one "generation" of data (the latest refresh), not a
/// history, matching the refresh-button model (no periodic accumulation).
pub fn store_crud_operations(
    dashboard_conn: &Connection,
    computed_at: &str,
    rows: &[CrudOperationsRow],
) -> rusqlite::Result<()> {
    dashboard_conn.execute("DELETE FROM crud_operations", [])?;
    for row in rows {
        dashboard_conn.execute(
            "INSERT INTO crud_operations (computed_at, entity_type, method, count) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![computed_at, row.entity_type, row.method, row.count],
        )?;
    }
    Ok(())
}

/// Returns the latest computed CRUD Operations breakdown, plus when it was
/// computed. `None` if a refresh has never completed.
pub fn get_crud_operations(
    dashboard_conn: &Connection,
) -> rusqlite::Result<Option<(String, Vec<CrudOperationsRow>)>> {
    let computed_at: Option<String> = dashboard_conn
        .query_row("SELECT computed_at FROM crud_operations LIMIT 1", [], |row| row.get(0))
        .optional()?;

    let Some(computed_at) = computed_at else {
        return Ok(None);
    };

    let mut stmt = dashboard_conn
        .prepare("SELECT entity_type, method, count FROM crud_operations ORDER BY entity_type, method")?;
    let rows: Vec<CrudOperationsRow> = stmt
        .query_map([], |row| {
            Ok(CrudOperationsRow {
                entity_type: row.get(0)?,
                method: row.get(1)?,
                count: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(Some((computed_at, rows)))
}