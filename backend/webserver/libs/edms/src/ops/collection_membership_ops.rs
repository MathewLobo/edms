use crate::core::EdmsCore;
use crate::error::EdmsResult;
use crate::query_loader::QueryMap;
use std::sync::Arc;

/// One row from a collection's own file: which endpoint, and when it was
/// added to this collection.
#[derive(Debug, Clone)]
pub struct MembershipEntry {
    pub endpoint_id: String,
    pub added_at: String,
}

/// Operates on ONE collection's own SQLite file — not the shared edms.db.
/// Construct with that specific file's path (e.g.
/// `storage/collections/{name}.sqlite`). The file holds just endpoint IDs +
/// timestamps; the endpoint's real data (URL, tags, request/response
/// history) always lives centrally in the main database — this is a
/// membership list, never a copy.
pub struct CollectionMembershipOps {
    pub core: EdmsCore,
    queries: Arc<QueryMap>,
}

impl CollectionMembershipOps {
    pub fn new(file_path: &str) -> Self {
        CollectionMembershipOps {
            core: EdmsCore::new(file_path),
            queries: Arc::new(QueryMap::load()),
        }
    }

    /// Connects and ensures this collection's own table exists. Safe to
    /// call every time, including against a brand-new empty file — SQLite
    /// creates the file on first connect, and `CREATE TABLE IF NOT EXISTS`
    /// makes the schema step idempotent.
    pub fn initialize(&self) -> EdmsResult<()> {
        self.core.connect()?;
        self.core.proc(
            "CREATE TABLE IF NOT EXISTS membership (
                endpoint_id TEXT NOT NULL UNIQUE,
                added_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            &[],
        )?;
        Ok(())
    }

    pub fn shutdown(&self) -> EdmsResult<()> {
        self.core.disconnect()
    }

    pub fn add(&self, endpoint_id: &str) -> EdmsResult<usize> {
        let q = self
            .queries
            .get_collection_membership_query("ADD")
            .ok_or(crate::error::EdmsError::UnknownError)?;
        self.core.proc(q, &[&endpoint_id])
    }

    pub fn remove(&self, endpoint_id: &str) -> EdmsResult<usize> {
        let q = self
            .queries
            .get_collection_membership_query("REMOVE")
            .ok_or(crate::error::EdmsError::UnknownError)?;
        self.core.proc(q, &[&endpoint_id])
    }

    pub fn list(&self) -> EdmsResult<Vec<MembershipEntry>> {
        let q = self
            .queries
            .get_collection_membership_query("LIST")
            .ok_or(crate::error::EdmsError::UnknownError)?;
        self.core.cproc(q, &[], |row| {
            Ok(MembershipEntry {
                endpoint_id: row.get(0)?,
                added_at: row.get(1)?,
            })
        })
    }

    pub fn count(&self) -> EdmsResult<i64> {
        let q = self
            .queries
            .get_collection_membership_query("COUNT")
            .ok_or(crate::error::EdmsError::UnknownError)?;
        let rows: Vec<i64> = self.core.cproc(q, &[], |row| row.get(0))?;
        Ok(rows.first().copied().unwrap_or(0))
    }
}
