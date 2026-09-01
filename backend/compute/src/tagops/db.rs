// DB helpers shared by all tag operations.
// SQL keys reference webserver/libs/edms/src/queries.yaml.

use edms::{EdmsCore, query_loader::QueryMap};
use crate::tagops::types::ActivityEntry;

pub type DynResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Open a WAL-mode EdmsCore connection and load the embedded QueryMap.
pub fn open_core(db_path: &str) -> DynResult<(EdmsCore, QueryMap)> {
    let core = EdmsCore::new(db_path);
    core.connect().map_err(|e| format!("db connect: {e:?}"))?;
    Ok((core, QueryMap::load()))
}

/// Distinct endpoint_ids in `collection_id` that carry any tag from `tags`.
/// When `tags` is empty, returns all endpoints in the collection (B3).
/// When non-empty, joins bookmarks × tags with a dynamic IN clause (B8).
pub(super) fn endpoints_with_tags_in_collection(
    core:          &EdmsCore,
    queries:       &QueryMap,
    collection_id: &str,
    tags:          &[String],
) -> DynResult<Vec<String>> {
    if tags.is_empty() {
        let b3 = queries.get_bookmark_query("B3").ok_or("missing query B3")?;
        return Ok(core.cproc(b3, &[&collection_id], |row| row.get(0))?);
    }

    let placeholders = tags.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let b8 = queries.get_bookmark_query("B8").ok_or("missing query B8")?;
    let sql = b8.replacen("IN (?)", &format!("IN ({placeholders})"), 1);

    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&collection_id];
    for t in tags { params.push(t); }

    Ok(core.cproc(&sql, params.as_slice(), |row| row.get(0))?)
}

/// Insert endpoint into folder only if it is not already there (B9 check → B7 insert).
pub(super) fn insert_if_absent(
    core:        &EdmsCore,
    queries:     &QueryMap,
    endpoint_id: &str,
    folder:      &str,
) -> DynResult<()> {
    let b9 = queries.get_bookmark_query("B9").ok_or("missing query B9")?;
    let counts: Vec<i64> = core.cproc(b9, &[&endpoint_id, &folder], |row| row.get(0))?;
    if counts.first().copied().unwrap_or(0) > 0 {
        return Ok(());
    }
    let b7 = queries.get_bookmark_query("B7").ok_or("missing query B7")?;
    core.proc(b7, &[&endpoint_id, &folder])?;
    Ok(())
}

pub(super) fn log_entry(log: &mut Vec<ActivityEntry>, message: String) {
    let ts = chrono::Utc::now().to_rfc3339();
    tracing::info!("{}", message);
    log.push(ActivityEntry { timestamp: ts, message });
}

pub struct MergeClassification {
    pub source_eid: String,
    pub endpoint_str: String,
    pub method: String,
    pub annotation: Option<String>,
    pub target_eid: Option<String>,
}

pub(super) fn classify_batch(
    core: &EdmsCore,
    queries: &QueryMap,
    target_collection: &str,
    source_eids: &[String],
) -> DynResult<Vec<MergeClassification>> {
    if source_eids.is_empty() { return Ok(vec![]); }

    let placeholders = source_eids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let q = queries.get_merge_query("M_CLASSIFY").ok_or("missing M_CLASSIFY")?;
    let sql = format!("{q} ({placeholders})");

    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&target_collection];
    for eid in source_eids { params.push(eid); }

    Ok(core.cproc(&sql, params.as_slice(), |row| {
        Ok(MergeClassification {
            source_eid: row.get(0)?,
            endpoint_str: row.get(1)?,
            method: row.get(2)?,
            annotation: row.get(3)?,
            target_eid: row.get(4)?,
        })
    })?)
}

pub(super) fn union_tags(core: &EdmsCore, queries: &QueryMap, source_eid: &str, target_eid: &str) -> DynResult<usize> {
    let q = queries.get_merge_query("M_UNION_TAGS").ok_or("missing M_UNION_TAGS")?;
    Ok(core.proc(q, &[&target_eid, &source_eid])?)
}

pub(super) fn insert_endpoint(core: &EdmsCore, queries: &QueryMap, new_eid: &str, endpoint_str: &str, annotation: Option<&str>, method: &str) -> DynResult<usize> {
    let q = queries.get_merge_query("M_INSERT_EP").ok_or("missing M_INSERT_EP")?;
    Ok(core.proc(q, &[&new_eid, &endpoint_str, &annotation, &method])?)
}

pub(super) fn insert_bookmark(core: &EdmsCore, queries: &QueryMap, eid: &str, folder: &str) -> DynResult<usize> {
    let q = queries.get_merge_query("M_INSERT_BM").ok_or("missing M_INSERT_BM")?;
    Ok(core.proc(q, &[&eid, &folder])?)
}

pub(super) fn get_endpoint_details(core: &EdmsCore, queries: &QueryMap, eid: &str) -> DynResult<(String, String)> {
    let q = queries.get_endpoint_query("E2").ok_or("missing E2")?;
    let mut results = core.cproc(q, &[&eid], |row| Ok((row.get(1)?, row.get(3).unwrap_or_else(|_| "".to_string()))))?;
    results.pop().ok_or_else(|| "Endpoint not found".into())
}
