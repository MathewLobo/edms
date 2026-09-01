use crate::core::EdmsCore;
use crate::error::EdmsResult;
use crate::query_loader::QueryMap;
use std::sync::Arc;

pub struct CollectionTagMembershipOps {
    pub core: EdmsCore,
    queries: Arc<QueryMap>,
}

impl CollectionTagMembershipOps {
    pub fn new(db_path: &str) -> Self {
        CollectionTagMembershipOps {
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

    pub fn add(&self, collection_name: &str, tagname: &str) -> EdmsResult<usize> {
        let query = self.queries.get_merge_query("CT_INSERT").unwrap();
        self.core.proc(query, &[&collection_name, &tagname])
    }

    pub fn remove(&self, collection_name: &str, tagname: &str) -> EdmsResult<usize> {
        let query = self.queries.get_merge_query("CT_DELETE").unwrap();
        self.core.proc(query, &[&collection_name, &tagname])
    }

    pub fn list(&self, collection_name: &str) -> EdmsResult<Vec<String>> {
        let query = self.queries.get_merge_query("CT_LIST").unwrap();
        self.core.cproc(query, &[&collection_name], |row| row.get(0))
    }

    pub fn collections_by_tag(&self, tagname: &str) -> EdmsResult<Vec<String>> {
        let query = self.queries.get_merge_query("CT_COLLECTIONS_BY_TAG").unwrap();
        self.core.cproc(query, &[&tagname], |row| row.get(0))
    }
}
