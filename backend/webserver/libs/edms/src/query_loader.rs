use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct QueryConfig {
    pub help: String,
    pub query: String,
}

#[derive(Debug, Deserialize)]
pub struct QueryMap {
    pub endpoints: HashMap<String, QueryConfig>,
    pub requests: HashMap<String, QueryConfig>,
    pub responses: HashMap<String, QueryConfig>,
    pub tags: HashMap<String, QueryConfig>,
    pub history: HashMap<String, QueryConfig>,
    pub bookmarks: HashMap<String, QueryConfig>,
    pub catalog: HashMap<String, QueryConfig>,
    pub view_tag_counts: HashMap<String, QueryConfig>,
}

impl QueryMap {
    pub fn load() -> Self {
        const EMBEDDED: &str = include_str!("queries.yaml");
        serde_yaml::from_str(EMBEDDED)
            .expect("queries.yaml embedded at compile time is invalid")
    }

    pub fn get_endpoint_query(&self, key: &str) -> Option<&str> {
        self.endpoints.get(key).map(|c| c.query.as_str())
    }

    pub fn get_request_query(&self, key: &str) -> Option<&str> {
        self.requests.get(key).map(|c| c.query.as_str())
    }

    pub fn get_response_query(&self, key: &str) -> Option<&str> {
        self.responses.get(key).map(|c| c.query.as_str())
    }

    pub fn get_tag_query(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(|c| c.query.as_str())
    }

    pub fn get_history_query(&self, key: &str) -> Option<&str> {
        self.history.get(key).map(|c| c.query.as_str())
    }

    pub fn get_bookmark_query(&self, key: &str) -> Option<&str> {
        self.bookmarks.get(key).map(|c| c.query.as_str())
    }

    pub fn get_catalog_query(&self, key: &str) -> Option<&str> {
        self.catalog.get(key).map(|c| c.query.as_str())
    }

    pub fn get_view_tag_count_query(&self, key: &str) -> Option<&str> {
        self.view_tag_counts.get(key).map(|c| c.query.as_str())
    }
}
