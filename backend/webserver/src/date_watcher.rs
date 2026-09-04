//! Date-change watcher logic — captures a daily snapshot whenever the
//! calendar date actually rolls over, for day-over-day dashboard
//! comparison.
//!
//! The check-and-capture decision is extracted from the polling loop as a
//! standalone function taking dates as explicit parameters (not calling
//! Utc::now() internally), so it's callable directly with fake dates to
//! prove the real write-path works — no need to wait for an actual
//! midnight. See the tests below.

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::Connection;
use tracing::{info, warn};

use crate::dashboard_db;

/// If `current` differs from `last`, captures a daily snapshot for `last`
/// (the day that just ended) and rotates old ones. Returns the date that
/// should become the new `last` for the next check — always `current`.
pub fn check_date_rollover(
    dashboard_conn: &Connection,
    edms_db_path: &Path,
    storage_root: &Path,
    last: NaiveDate,
    current: NaiveDate,
) -> NaiveDate {
    if current != last {
        let ended_date = last.to_string();
        match dashboard_db::take_daily_snapshot(dashboard_conn, edms_db_path, storage_root, &ended_date) {
            Ok(snap) => info!("Daily snapshot captured for {ended_date}: {snap:?}"),
            Err(e) => warn!("Failed to capture daily snapshot for {ended_date}: {e}"),
        }
        if let Err(e) = dashboard_db::rotate_old_daily_snapshots(dashboard_conn) {
            warn!("Failed to rotate old daily snapshots: {e}");
        }
    }
    current
}

/// Figures out what `last_date` the watcher should start from, using the
/// database's own memory rather than blindly trusting the current
/// wall-clock date. If there's a gap between the last captured snapshot and
/// yesterday (e.g. the app was down across one or more rollovers), logs it
/// clearly — that data is genuinely unrecoverable (there's no way to know
/// today what the counts looked like on a day nobody recorded them), but at
/// least it's a visible, diagnosable gap instead of a silent one.
pub fn initial_last_date(dashboard_conn: &Connection, today: NaiveDate) -> NaiveDate {
    match dashboard_db::latest_daily_snapshot_date(dashboard_conn) {
        Ok(Some(date_str)) => match NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
            Ok(last_captured) => {
                let yesterday = today - chrono::Duration::days(1);
                if last_captured < yesterday {
                    let gap_days = (yesterday - last_captured).num_days();
                    warn!(
                        "Daily snapshot gap detected: last captured was {last_captured}, \
                         yesterday was {yesterday} — {gap_days} day(s) missing, likely from \
                         downtime spanning a rollover. That data can't be reconstructed \
                         retroactively; resuming from today."
                    );
                }
                today
            }
            Err(e) => {
                warn!("Failed to parse stored snapshot_date '{date_str}': {e} — resuming from today");
                today
            }
        },
        Ok(None) => today, // first run ever, nothing to compare against
        Err(e) => {
            warn!("Failed to check latest daily snapshot date: {e} — resuming from today");
            today
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A real temp edms.db (schema initialized) + a real temp dashboard.db,
    /// both cleaned up when the returned TempPaths drops.
    struct TempPaths {
        dir: std::path::PathBuf,
        edms_db: std::path::PathBuf,
    }

    impl Drop for TempPaths {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn setup() -> (TempPaths, Connection) {
        let dir = std::env::temp_dir().join(format!("edms_date_watcher_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let edms_db = dir.join("edms.db");
        let edms_conn = Connection::open(&edms_db).unwrap();
        edms::schema::initialize_schema(&edms_conn).unwrap();
        drop(edms_conn);

        let dashboard_conn = dashboard_db::init_dashboard_db(&dir).unwrap();

        (TempPaths { dir: dir.clone(), edms_db }, dashboard_conn)
    }

    #[test]
    fn rollover_captures_a_snapshot_for_the_day_that_ended() {
        let (paths, conn) = setup();
        // Relative to today, not hardcoded — rotate_old_daily_snapshots
        // (called inside check_date_rollover) deletes anything older than
        // 30 real days, so a fixed date here goes stale and gets rotated
        // away the moment real time passes it by. day1 must stay well
        // inside the retention window no matter when this test runs.
        let today = chrono::Utc::now().date_naive();
        let day1 = today - chrono::Duration::days(2);
        let day2 = today - chrono::Duration::days(1);
        let day1_str = day1.to_string();

        let result = check_date_rollover(&conn, &paths.edms_db, &paths.dir, day1, day2);

        assert_eq!(result, day2);
        let snap = dashboard_db::get_daily_snapshot(&conn, &day1_str)
            .unwrap()
            .expect("snapshot for day1 should have been captured");
        assert_eq!(snap.snapshot_date, day1_str);
    }

    #[test]
    fn same_day_captures_nothing() {
        let (paths, conn) = setup();
        let day1 = chrono::Utc::now().date_naive() - chrono::Duration::days(2);
        let day1_str = day1.to_string();

        check_date_rollover(&conn, &paths.edms_db, &paths.dir, day1, day1);

        assert!(dashboard_db::get_daily_snapshot(&conn, &day1_str).unwrap().is_none());
    }

    #[test]
    fn initial_last_date_resumes_from_today_on_first_ever_run() {
        let (_paths, conn) = setup();
        let today = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();

        assert_eq!(initial_last_date(&conn, today), today);
    }

    #[test]
    fn initial_last_date_detects_a_gap_without_erroring() {
        let (paths, conn) = setup();
        // Simulate a snapshot captured 5 days before "today" — a gap left
        // by downtime. Should still resume cleanly from today, not panic
        // or get stuck trying to backfill.
        check_date_rollover(
            &conn,
            &paths.edms_db,
            &paths.dir,
            NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(),
        );

        let today = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
        assert_eq!(initial_last_date(&conn, today), today);
    }
}
