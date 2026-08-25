use crate::core::EdmsCore;
use crate::error::EdmsResult;
use crate::query_loader::QueryMap;
use std::sync::Arc;

/// Which of the three per-view-type schemas an operation targets. Each maps
/// to its own tables (`collections`/`collections_tags`, etc.) per Ravi's
/// schema (2026-08-25) — kept as separate tables, not one shared table with
/// a view column, matching his explicit design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Collections,
    Webview,
    Repoview,
}

impl ViewKind {
    fn prefix(self) -> &'static str {
        match self {
            ViewKind::Collections => "COLLECTIONS",
            ViewKind::Webview => "WEBVIEW",
            ViewKind::Repoview => "REPOVIEW",
        }
    }
}

/// Catalog registry — which collections/webviews/repoviews exist, and where
/// each one's independent SQLite file lives (once that file actually gets
/// created; not built by this pass — see schema.rs).
pub struct ViewCatalogOps {
    pub core: EdmsCore,
    queries: Arc<QueryMap>,
}

impl ViewCatalogOps {
    pub fn new(db_path: &str) -> Self {
        ViewCatalogOps {
            core: EdmsCore::new(db_path),
            queries: Arc::new(QueryMap::load_or_default()),
        }
    }

    pub fn initialize(&self) -> EdmsResult<()> {
        self.core.connect()
    }

    pub fn shutdown(&self) -> EdmsResult<()> {
        self.core.disconnect()
    }

    pub fn register(&self, kind: ViewKind, name: &str, file_path: Option<&str>) -> EdmsResult<usize> {
        let key = format!("{}_CREATE", kind.prefix());
        let query = self.queries.get_catalog_query(&key).unwrap();
        self.core.proc(query, &[&name, &file_path])
    }

    pub fn list(&self, kind: ViewKind) -> EdmsResult<Vec<(String, Option<String>, String)>> {
        let key = format!("{}_LIST", kind.prefix());
        let query = self.queries.get_catalog_query(&key).unwrap();
        self.core
            .cproc(query, &[], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    }
}

/// Central per-view-type tag count rollups — tagname + an incrementally
/// maintained count, not a full tag entity with membership tracking.
pub struct ViewTagCountOps {
    pub core: EdmsCore,
    queries: Arc<QueryMap>,
}

impl ViewTagCountOps {
    pub fn new(db_path: &str) -> Self {
        ViewTagCountOps {
            core: EdmsCore::new(db_path),
            queries: Arc::new(QueryMap::load_or_default()),
        }
    }

    pub fn initialize(&self) -> EdmsResult<()> {
        self.core.connect()
    }

    pub fn shutdown(&self) -> EdmsResult<()> {
        self.core.disconnect()
    }

    fn query(&self, kind: ViewKind, suffix: &str) -> &str {
        let key = format!("{}_{suffix}", kind.prefix());
        self.queries
            .get_view_tag_count_query(&key)
            .unwrap_or_else(|| panic!("missing view_tag_counts query for key {key}"))
    }

    /// Creates the tag if it doesn't exist (count = increment_by), or adds
    /// increment_by to its existing count. `increment_by` is normally the
    /// number of endpoints just tagged in one call.
    pub fn create(&self, kind: ViewKind, tagname: &str, increment_by: i64) -> EdmsResult<usize> {
        let query = self.query(kind, "UPSERT").to_string();
        self.core.proc(&query, &[&tagname, &increment_by])
    }

    pub fn delete(&self, kind: ViewKind, tagname: &str) -> EdmsResult<usize> {
        let query = self.query(kind, "DELETE").to_string();
        self.core.proc(&query, &[&tagname])
    }

    pub fn delete_many(&self, kind: ViewKind, tagnames: &[String]) -> EdmsResult<usize> {
        let mut deleted = 0;
        for name in tagnames {
            deleted += self.delete(kind, name)?;
        }
        Ok(deleted)
    }

    /// Renames a tag, preserving its count. If a tag with `new_name` already
    /// exists, the counts are merged into it rather than overwritten.
    pub fn rename(&self, kind: ViewKind, old_name: &str, new_name: &str) -> EdmsResult<()> {
        let get_query = self.query(kind, "GET").to_string();
        let count: i64 = self
            .core
            .cproc(&get_query, &[&old_name], |row| row.get(0))?
            .into_iter()
            .next()
            .unwrap_or(0);

        if count == 0 {
            return Ok(());
        }

        let delete_query = self.query(kind, "DELETE").to_string();
        self.core.proc(&delete_query, &[&old_name])?;

        let upsert_query = self.query(kind, "UPSERT").to_string();
        self.core.proc(&upsert_query, &[&new_name, &count])?;

        Ok(())
    }

    pub fn list(&self, kind: ViewKind) -> EdmsResult<Vec<(String, i64)>> {
        let query = self.query(kind, "LIST_TAGS").to_string();
        self.core
            .cproc(&query, &[], |row| Ok((row.get(0)?, row.get(1)?)))
    }
}
