use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id:   String,
    pub name: String,
}

/// Source collection + tag filter for a merge: Cfinal = C1(T1, T2) + C2(T3).
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionTagFilter {
    pub collection_id: String,
    pub tags:          Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MergeRequest {
    pub db_path:                String,
    pub target_collection_name: String,
    pub sources:                Vec<CollectionTagFilter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFromTagsRequest {
    pub db_path:              String,
    pub new_collection_name:  String,
    pub source_collection_id: String,
    pub tags:                 Vec<String>,
}

/// Shared payload for bulk-add and bulk-remove tasks.
#[derive(Debug, Serialize, Deserialize)]
pub struct BulkTagRequest {
    pub db_path:      String,
    pub endpoint_ids: Vec<String>,
    pub tags:         Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameTagRequest {
    pub db_path:  String,
    pub old_name: String,
    pub new_name: String,
}

/// Timestamped entry written to the Activity Log.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActivityEntry {
    pub timestamp: String, // RFC-3339
    pub message:   String,
}
