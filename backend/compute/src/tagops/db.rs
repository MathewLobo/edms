// DB helpers shared by all tag operations.
// SQL keys reference webserver/libs/edms/src/queries.yaml.

use edms::{EdmsCore, query_loader::QueryMap};
use crate::tagops::types::ActivityEntry;

pub(super) type DynResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Open a WAL-mode EdmsCore connection and load the embedded QueryMap.
pub(super) fn open_core(db_path: &str) -> DynResult<(EdmsCore, QueryMap)> {
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
