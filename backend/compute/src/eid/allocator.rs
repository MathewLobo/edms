//! Persistent Gap-List EID Allocator.
//!
//! Maintains high-watermark allocation and recycled gap management persisted in SQLite.
//! Thread-safe in-process (Mutex) and process-safe across processes (BEGIN IMMEDIATE).

use super::{format_eid_with_suffix, parse_eid, EidError};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationStatus {
    pub watermark: u64,
    pub gaps_count: usize,
    pub active_count: u64,
    pub gaps: Vec<u64>,
}

pub struct EidAllocator {
    db_path: String,
    suffix: String,
    lock: Mutex<()>,
}

impl EidAllocator {
    /// Creates a new allocator connected to the specified SQLite database.
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
            suffix: "AAA".to_string(),
            lock: Mutex::new(()),
        }
    }

    /// Creates an allocator with a custom 3-letter suffix (e.g. "ABC").
    pub fn with_suffix(db_path: &str, suffix: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
            suffix: suffix.to_string(),
            lock: Mutex::new(()),
        }
    }

    /// Ensures the `eid_allocation` tracking table exists.
    pub fn initialize(&self) -> Result<(), EidError> {
        let conn = self.open_connection()?;
        Self::initialize_conn(&conn)
    }

    /// Ensures the tracking table exists on an existing SQLite connection.
    pub fn initialize_conn(conn: &Connection) -> Result<(), EidError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS eid_allocation (
                id        INTEGER PRIMARY KEY CHECK (id = 1),
                watermark INTEGER NOT NULL DEFAULT 0,
                gaps_json TEXT NOT NULL DEFAULT '[]'
            )",
            [],
        )?;

        let has_row: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM eid_allocation WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        if has_row == 0 {
            // Check if endpoints table exists and has existing canonical EIDs
            let mut max_watermark = 0u64;
            let mut existing_nums = BTreeSet::new();

            let has_endpoints_table: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='endpoints'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            if has_endpoints_table > 0 {
                if let Ok(mut stmt) = conn.prepare("SELECT endpoint_id FROM endpoints") {
                    if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
                        for eid_res in rows.flatten() {
                            if let Ok((num, _)) = parse_eid(&eid_res) {
                                if num > max_watermark {
                                    max_watermark = num;
                                }
                                existing_nums.insert(num);
                            }
                        }
                    }
                }
            }

            let mut gaps = BTreeSet::new();
            for n in 1..max_watermark {
                if !existing_nums.contains(&n) {
                    gaps.insert(n);
                }
            }

            let gaps_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT OR IGNORE INTO eid_allocation (id, watermark, gaps_json) VALUES (1, ?, ?)",
                rusqlite::params![max_watermark as i64, gaps_json],
            )?;
        }

        Ok(())
    }

    fn open_connection(&self) -> Result<Connection, EidError> {
        let conn = Connection::open(&self.db_path)?;
        // DELETE, not WAL — matches EdmsBase::connect()'s pragma, the
        // established working setting for this exact db file everywhere
        // else in the app. WAL is a *persistent*, file-level setting
        // (stored in the db header, not per-connection): the first
        // allocate()/reserve() call was silently flipping the whole
        // edms.db into WAL mode for every other connection too, which
        // then reliably deadlocked (confirmed 2026-09-08 — this is the
        // exact same failure class already hit and fixed once before,
        // see the Docker/WAL note in API_REFERENCE.md, just reintroduced
        // here for a different connection).
        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        Ok(conn)
    }

    /// Allocates the next available canonical EID using an open connection.
    /// If freed gaps exist, the lowest gap number is reused first.
    /// Otherwise, watermark is incremented.
    pub fn allocate_with_conn(&self, conn: &Connection) -> Result<String, EidError> {
        let _guard = self.lock.lock().map_err(|e| EidError::LockError(e.to_string()))?;
        Self::initialize_conn(conn)?;

        conn.execute("BEGIN IMMEDIATE", [])?;
        let res = (|| -> Result<String, EidError> {
            let (watermark_i64, gaps_json): (i64, String) = conn.query_row(
                "SELECT watermark, gaps_json FROM eid_allocation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut watermark = watermark_i64 as u64;

            let mut gaps: BTreeSet<u64> = serde_json::from_str(&gaps_json)
                .unwrap_or_default();

            let has_endpoints: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='endpoints'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) > 0;

            let assigned_num = loop {
                let cand = if let Some(first_gap) = gaps.iter().next().cloned() {
                    gaps.remove(&first_gap);
                    first_gap
                } else {
                    watermark += 1;
                    watermark
                };

                let exists = if has_endpoints {
                    let eid_str = format_eid_with_suffix(cand, &self.suffix);
                    conn.query_row(
                        "SELECT COUNT(*) FROM endpoints WHERE endpoint_id = ?",
                        [&eid_str],
                        |r| r.get::<_, i64>(0),
                    ).unwrap_or(0) > 0
                } else {
                    false
                };

                if !exists {
                    break cand;
                }
            };

            let updated_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "UPDATE eid_allocation SET watermark = ?, gaps_json = ? WHERE id = 1",
                rusqlite::params![watermark as i64, updated_json],
            )?;

            Ok(format_eid_with_suffix(assigned_num, &self.suffix))
        })();

        match res {
            Ok(eid) => {
                conn.execute("COMMIT", [])?;
                Ok(eid)
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Allocates the next available canonical EID by opening a connection to `db_path`.
    pub fn allocate(&self) -> Result<String, EidError> {
        let conn = self.open_connection()?;
        self.allocate_with_conn(&conn)
    }

    /// Allocates `count` canonical EIDs in a single transaction.
    ///
    /// Includes the same per-candidate collision-check against the `endpoints`
    /// table as `allocate_with_conn`, ensuring correctness even when
    /// `eid_allocation` was bootstrapped against an existing database.
    pub fn allocate_batch_with_conn(
        &self,
        conn: &Connection,
        count: usize,
    ) -> Result<Vec<String>, EidError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let _guard = self.lock.lock().map_err(|e| EidError::LockError(e.to_string()))?;
        Self::initialize_conn(conn)?;

        conn.execute("BEGIN IMMEDIATE", [])?;
        let res = (|| -> Result<Vec<String>, EidError> {
            let (watermark_i64, gaps_json): (i64, String) = conn.query_row(
                "SELECT watermark, gaps_json FROM eid_allocation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut watermark = watermark_i64 as u64;

            let mut gaps: BTreeSet<u64> = serde_json::from_str(&gaps_json)
                .unwrap_or_default();

            let has_endpoints: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='endpoints'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) > 0;

            let mut allocated = Vec::with_capacity(count);

            for _ in 0..count {
                // Same collision-check loop as allocate_with_conn: skip any
                // number that is already present in the endpoints table so that
                // bootstrapping against a pre-existing DB never produces a
                // UNIQUE-constraint failure on insert.
                let assigned_num = loop {
                    let cand = if let Some(first_gap) = gaps.iter().next().cloned() {
                        gaps.remove(&first_gap);
                        first_gap
                    } else {
                        watermark += 1;
                        watermark
                    };

                    let exists = if has_endpoints {
                        let eid_str = format_eid_with_suffix(cand, &self.suffix);
                        conn.query_row(
                            "SELECT COUNT(*) FROM endpoints WHERE endpoint_id = ?",
                            [&eid_str],
                            |r| r.get::<_, i64>(0),
                        ).unwrap_or(0) > 0
                    } else {
                        false
                    };

                    if !exists {
                        break cand;
                    }
                };
                allocated.push(format_eid_with_suffix(assigned_num, &self.suffix));
            }

            let updated_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "UPDATE eid_allocation SET watermark = ?, gaps_json = ? WHERE id = 1",
                rusqlite::params![watermark as i64, updated_json],
            )?;

            Ok(allocated)
        })();

        match res {
            Ok(eids) => {
                conn.execute("COMMIT", [])?;
                Ok(eids)
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Allocates `count` canonical EIDs by opening a connection to `db_path`.
    pub fn allocate_batch(&self, count: usize) -> Result<Vec<String>, EidError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let conn = self.open_connection()?;
        self.allocate_batch_with_conn(&conn, count)
    }

    /// Releases an existing EID, placing its number back into the gap list for reuse.
    /// If the released ID was the watermark, the watermark shrinks.
    pub fn release_with_conn(&self, conn: &Connection, eid: &str) -> Result<(), EidError> {
        let (num, _) = parse_eid(eid)?;
        let _guard = self.lock.lock().map_err(|e| EidError::LockError(e.to_string()))?;
        Self::initialize_conn(conn)?;

        conn.execute("BEGIN IMMEDIATE", [])?;
        let res = (|| -> Result<(), EidError> {
            let (watermark_i64, gaps_json): (i64, String) = conn.query_row(
                "SELECT watermark, gaps_json FROM eid_allocation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut watermark = watermark_i64 as u64;

            let mut gaps: BTreeSet<u64> = serde_json::from_str(&gaps_json)
                .unwrap_or_default();

            if num == watermark {
                watermark = watermark.saturating_sub(1);
                // cascade-shrink: if the numbers just below watermark are
                // already in the gap list, collapse them too so the watermark
                // always points at the highest *live* allocation.
                while watermark > 0 && gaps.remove(&watermark) {
                    watermark -= 1;
                }
            } else if num < watermark {
                gaps.insert(num);
            } else {
                // num > watermark: the EID was never issued by this allocator
                // (e.g. a legacy non-canonical ID or a manual DB edit).
                // The gap list cannot represent it, so we leave the allocator
                // state unchanged but emit a warning for diagnostics.
                tracing::warn!(
                    eid = eid,
                    num,
                    watermark,
                    "EID release ignored: number is above current allocation watermark"
                );
            }

            let updated_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "UPDATE eid_allocation SET watermark = ?, gaps_json = ? WHERE id = 1",
                rusqlite::params![watermark as i64, updated_json],
            )?;

            Ok(())
        })();

        match res {
            Ok(()) => {
                conn.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Releases an existing EID by opening a connection to `db_path`.
    pub fn release(&self, eid: &str) -> Result<(), EidError> {
        let conn = self.open_connection()?;
        self.release_with_conn(&conn, eid)
    }

    /// Reserves a specific EID (e.g. during an import where the source EID is accepted).
    /// If `eid` is higher than the current watermark, all missing intermediate numbers
    /// are added to the gap list.
    pub fn reserve_with_conn(&self, conn: &Connection, eid: &str) -> Result<(), EidError> {
        let (num, _) = parse_eid(eid)?;
        let _guard = self.lock.lock().map_err(|e| EidError::LockError(e.to_string()))?;
        Self::initialize_conn(conn)?;

        conn.execute("BEGIN IMMEDIATE", [])?;
        let res = (|| -> Result<(), EidError> {
            let (watermark_i64, gaps_json): (i64, String) = conn.query_row(
                "SELECT watermark, gaps_json FROM eid_allocation WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut watermark = watermark_i64 as u64;

            let mut gaps: BTreeSet<u64> = serde_json::from_str(&gaps_json)
                .unwrap_or_default();

            if num > watermark {
                for missing in (watermark + 1)..num {
                    gaps.insert(missing);
                }
                watermark = num;
            } else {
                gaps.remove(&num);
            }

            let updated_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "UPDATE eid_allocation SET watermark = ?, gaps_json = ? WHERE id = 1",
                rusqlite::params![watermark as i64, updated_json],
            )?;

            Ok(())
        })();

        match res {
            Ok(()) => {
                conn.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Reserves an existing EID by opening a connection to `db_path`.
    pub fn reserve(&self, eid: &str) -> Result<(), EidError> {
        let conn = self.open_connection()?;
        self.reserve_with_conn(&conn, eid)
    }

    /// Returns the current allocation status (watermark, gap count, active count).
    pub fn status_with_conn(&self, conn: &Connection) -> Result<AllocationStatus, EidError> {
        Self::initialize_conn(conn)?;
        let (watermark_i64, gaps_json): (i64, String) = conn.query_row(
            "SELECT watermark, gaps_json FROM eid_allocation WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let watermark = watermark_i64 as u64;

        let gaps: BTreeSet<u64> = serde_json::from_str(&gaps_json).unwrap_or_default();
        let gaps_vec: Vec<u64> = gaps.into_iter().collect();
        let active_count = watermark.saturating_sub(gaps_vec.len() as u64);

        Ok(AllocationStatus {
            watermark,
            gaps_count: gaps_vec.len(),
            active_count,
            gaps: gaps_vec,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sequential_allocation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        assert_eq!(allocator.allocate().unwrap(), "E0001-AAA");
        assert_eq!(allocator.allocate().unwrap(), "E0002-AAA");
        assert_eq!(allocator.allocate().unwrap(), "E0003-AAA");

        let status = allocator.open_connection().and_then(|c| allocator.status_with_conn(&c)).unwrap();
        assert_eq!(status.watermark, 3);
        assert_eq!(status.active_count, 3);
        assert_eq!(status.gaps_count, 0);
    }

    #[test]
    fn test_gap_reuse() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        // Allocate 1, 2, 3, 4, 5
        let eids: Vec<String> = (0..5).map(|_| allocator.allocate().unwrap()).collect();
        assert_eq!(eids, vec!["E0001-AAA", "E0002-AAA", "E0003-AAA", "E0004-AAA", "E0005-AAA"]);

        // Release 3
        allocator.release("E0003-AAA").unwrap();

        // Next allocation MUST reuse gap 3
        assert_eq!(allocator.allocate().unwrap(), "E0003-AAA");

        // Next allocation must continue to 6
        assert_eq!(allocator.allocate().unwrap(), "E0006-AAA");
    }

    #[test]
    fn test_watermark_shrink_on_tail_release() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        allocator.allocate().unwrap(); // 1
        allocator.allocate().unwrap(); // 2
        allocator.allocate().unwrap(); // 3

        // Release 3 (tail): watermark should shrink to 2
        allocator.release("E0003-AAA").unwrap();
        let status = allocator.open_connection().and_then(|c| allocator.status_with_conn(&c)).unwrap();
        assert_eq!(status.watermark, 2);
        assert_eq!(status.gaps_count, 0);

        // Next allocate should re-issue 3
        assert_eq!(allocator.allocate().unwrap(), "E0003-AAA");
    }

    #[test]
    fn test_batch_allocation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        let conn = allocator.open_connection().unwrap();
        let batch = allocator.allocate_batch_with_conn(&conn, 4).unwrap();
        assert_eq!(batch, vec!["E0001-AAA", "E0002-AAA", "E0003-AAA", "E0004-AAA"]);
    }

    #[test]
    fn test_reserve_out_of_order() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        // Reserve 5 directly (e.g. from imported archive)
        allocator.reserve("E0005-AAA").unwrap();

        // Status should have watermark 5, and gaps [1, 2, 3, 4]
        let conn = allocator.open_connection().unwrap();
        let status = allocator.status_with_conn(&conn).unwrap();
        assert_eq!(status.watermark, 5);
        assert_eq!(status.gaps, vec![1, 2, 3, 4]);

        // Next allocation fills the lowest gap: 1
        assert_eq!(allocator.allocate().unwrap(), "E0001-AAA");
    }

    #[test]
    fn test_persistence_across_instances() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();

        {
            let allocator1 = EidAllocator::new(&db_path);
            assert_eq!(allocator1.allocate().unwrap(), "E0001-AAA");
            assert_eq!(allocator1.allocate().unwrap(), "E0002-AAA");
            allocator1.release("E0001-AAA").unwrap();
        }

        {
            let allocator2 = EidAllocator::new(&db_path);
            // Must reuse gap 1 from previous instance
            assert_eq!(allocator2.allocate().unwrap(), "E0001-AAA");
            // Must continue to 3
            assert_eq!(allocator2.allocate().unwrap(), "E0003-AAA");
        }
    }

    #[test]
    fn test_uniqueness_large_scale() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = EidAllocator::new(&db_path);

        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            let eid = allocator.allocate().unwrap();
            assert!(seen.insert(eid), "Duplicate EID generated!");
        }
        assert_eq!(seen.len(), 1000);
    }

    #[test]
    fn test_concurrent_allocation() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let allocator = Arc::new(EidAllocator::new(&db_path));
        allocator.initialize().unwrap();

        let num_threads = 8;
        let per_thread = 50;
        let mut handles = Vec::new();

        for _ in 0..num_threads {
            let alloc_clone = Arc::clone(&allocator);
            handles.push(thread::spawn(move || {
                let mut allocated = Vec::new();
                for _ in 0..per_thread {
                    allocated.push(alloc_clone.allocate().unwrap());
                }
                allocated
            }));
        }

        let mut all_eids = std::collections::HashSet::new();
        for handle in handles {
            let eids = handle.join().unwrap();
            for eid in eids {
                assert!(all_eids.insert(eid), "Duplicate EID across threads!");
            }
        }

        assert_eq!(all_eids.len(), num_threads * per_thread);
    }
}
