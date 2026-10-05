use crate::core::EdmsCore;
use crate::error::EdmsResult;
use crate::query_loader::QueryMap;
use std::sync::Arc;

/// Row-level tags on a RepoView instance itself — the "Tags (modifiable)"
/// field from the RepoView spec (2026-09-05). Mirrors
/// `CollectionTagMembershipOps` exactly, just against `repoview_tag_memberships`
/// instead of `collection_tag_memberships` — kept as a separate struct rather
/// than generalizing that one, so Collections' already-working code stays
/// untouched.
pub struct RepoviewTagMembershipOps {
    pub core: EdmsCore,
    queries: Arc<QueryMap>,
}

impl RepoviewTagMembershipOps {
    pub fn new(db_path: &str) -> Self {
        RepoviewTagMembershipOps {
            core: EdmsCore::new(db_path),
            queries: Arc::new(QueryMap::load()),
        }
    }

    pub fn initialize(&self) -> EdmsResult<()> {
        self.core.connect()
    }

    pub fn shutdown(&self) -> EdmsResult<()> {
        self.core.disconnect()
    }

    pub fn add(&self, repoview_name: &str, tagname: &str) -> EdmsResult<usize> {
        let query = self.queries.get_merge_query("RT_INSERT").unwrap();
        self.core.proc(query, &[&repoview_name, &tagname])
    }

    pub fn remove(&self, repoview_name: &str, tagname: &str) -> EdmsResult<usize> {
        let query = self.queries.get_merge_query("RT_DELETE").unwrap();
        self.core.proc(query, &[&repoview_name, &tagname])
    }

    pub fn list(&self, repoview_name: &str) -> EdmsResult<Vec<String>> {
        let query = self.queries.get_merge_query("RT_LIST").unwrap();
        self.core.cproc(query, &[&repoview_name], |row| row.get(0))
    }

    pub fn repoviews_by_tag(&self, tagname: &str) -> EdmsResult<Vec<String>> {
        let query = self.queries.get_merge_query("RT_REPOVIEWS_BY_TAG").unwrap();
        self.core.cproc(query, &[&tagname], |row| row.get(0))
    }
}
