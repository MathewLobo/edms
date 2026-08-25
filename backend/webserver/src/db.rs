use edms::core::EdmsCore;
use edms::error::{EdmsError, EdmsResult};
use edms::query_loader::QueryMap;
use rusqlite::ToSql;
use serde::{Deserialize, Serialize};

pub const ACTIVE_FOLDER: &str = "__active__";
pub const SESSION_BACKUP_FOLDER: &str = "__session_backup__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDto {
    pub endpoint_id: String,
    pub endpoint_str: String,
    pub annotation: Option<String>,
    pub method: Option<String>,
}

/* ---------------- endpoints (queries.yaml) ---------------- */

pub fn insert_endpoint(core: &EdmsCore, queries: &QueryMap, ep: &EndpointDto) -> EdmsResult<usize> {
    let q = queries.get_endpoint_query("E1").ok_or(EdmsError::UnknownError)?;
    // E1: INSERT INTO endpoints (endpoint_id, endpoint_str, annotation, method) VALUES (?, ?, ?, ?)
    core.proc(
        q,
        &[
            &ep.endpoint_id,
            &ep.endpoint_str,
            &ep.annotation.as_deref(),
            &ep.method.as_deref(),
        ],
    )
}

pub fn list_endpoints(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<Vec<EndpointDto>> {
    let q = queries.get_endpoint_query("E3").ok_or(EdmsError::UnknownError)?;
    // E3: SELECT endpoint_id, endpoint_str, annotation, method FROM endpoints
    core.cproc(q, &[], |row| {
        Ok(EndpointDto {
            endpoint_id: row.get(0)?,
            endpoint_str: row.get(1)?,
            annotation: row.get(2)?,
            method: row.get(3)?,
        })
    })
}

pub fn get_endpoint(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str) -> EdmsResult<Option<EndpointDto>> {
    let q = queries.get_endpoint_query("E2").ok_or(EdmsError::UnknownError)?;
    // E2: SELECT endpoint_id, endpoint_str, annotation, method FROM endpoints WHERE endpoint_id = ?
    let rows = core.cproc(q, &[&endpoint_id], |row| {
        Ok(EndpointDto {
            endpoint_id: row.get(0)?,
            endpoint_str: row.get(1)?,
            annotation: row.get(2)?,
            method: row.get(3)?,
        })
    })?;
    Ok(rows.into_iter().next())
}

pub fn update_annotation(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str, annotation: &str) -> EdmsResult<usize> {
    let q = queries.get_endpoint_query("E4").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[&annotation, &endpoint_id])
}

pub fn delete_endpoint(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str) -> EdmsResult<usize> {
    let q = queries.get_endpoint_query("E5").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[&endpoint_id])
}

/* ---------------- request/response metadata (queries.yaml) ---------------- */

pub fn get_next_request_number(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str) -> EdmsResult<i32> {
    let q = queries.get_request_query("R6").ok_or(EdmsError::UnknownError)?;
    let rows: Vec<Option<i32>> = core.cproc(q, &[&endpoint_id], |row| row.get(0))?;
    match rows.first() {
        Some(Some(max)) => Ok(max + 1),
        _ => Ok(1),
    }
}

pub fn insert_request_metadata(
    core: &EdmsCore,
    queries: &QueryMap,
    endpoint_id: &str,
    request_number: i32,
    file_path: &str,
    method: &str,
) -> EdmsResult<usize> {
    let q = queries.get_request_query("R1").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[&endpoint_id, &request_number, &file_path, &method])
}

pub fn insert_response_metadata(
    core: &EdmsCore,
    queries: &QueryMap,
    endpoint_id: &str,
    request_number: i32,
    file_path: &str,
    status_code: i32,
    response_time_ms: Option<i32>,
) -> EdmsResult<usize> {
    let q = queries.get_response_query("RES1").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[&endpoint_id, &request_number, &file_path, &status_code, &response_time_ms])
}

/* ---------------- history (queries.yaml) ---------------- */

pub fn history_count(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    let q = queries.get_history_query("H1").ok_or(EdmsError::UnknownError)?;
    let rows: Vec<i64> = core.cproc(q, &[], |row| row.get(0))?;
    Ok(rows.first().copied().unwrap_or(0) as usize)
}

pub fn clear_history(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    let q = queries.get_history_query("H2").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[])
}

pub fn insert_history(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str, action: &str, details: Option<&str>) -> EdmsResult<usize> {
    let q = queries.get_history_query("H3").ok_or(EdmsError::UnknownError)?;
    core.proc(
        q,
        &[&endpoint_id, &action, &details],
    )
}

pub fn list_history_endpoint_ids(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<Vec<String>> {
    // Most recent first
    let q = queries.get_history_query("H4").ok_or(EdmsError::UnknownError)?;
    core.cproc(q, &[], |row| row.get(0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub endpoint_id: String,
    pub action: String,
    pub details: Option<String>,
    pub timestamp: String,
}

pub fn list_history(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<Vec<HistoryEntry>> {
    let q = queries.get_history_query("H5").ok_or(EdmsError::UnknownError)?;
    core.cproc(q, &[], |row| {
        Ok(HistoryEntry {
            id: row.get(0)?,
            endpoint_id: row.get(1)?,
            action: row.get(2)?,
            details: row.get(3)?,
            timestamp: row.get(4)?,
        })
    })
}

/* ---------------- bookmarks table (direct SQL) ---------------- */

pub fn bookmarks_count_active(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    let q = queries.get_bookmark_query("B1").ok_or(EdmsError::UnknownError)?;
    let rows: Vec<i64> = core.cproc(q, &[&ACTIVE_FOLDER], |row| row.get(0))?;
    Ok(rows.first().copied().unwrap_or(0) as usize)
}

pub fn clear_bookmarks_active(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    let q = queries.get_bookmark_query("B2").ok_or(EdmsError::UnknownError)?;
    core.proc(q, &[&ACTIVE_FOLDER])
}

pub fn list_bookmarked_endpoints_active(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<Vec<String>> {
    // Returns endpoint_ids in active list
    let q = queries.get_bookmark_query("B3").ok_or(EdmsError::UnknownError)?;
    core.cproc(q, &[&ACTIVE_FOLDER], |row| row.get(0))
}

pub fn insert_bookmark_active(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str, notes: Option<&str>) -> EdmsResult<usize> {
    // Check if already bookmarked to avoid duplicates
    let b4 = queries.get_bookmark_query("B4").ok_or(EdmsError::UnknownError)?;
    let existing: Vec<i64> = core.cproc(
        b4,
        &[&ACTIVE_FOLDER, &endpoint_id],
        |row| row.get(0)
    )?;
    
    if existing.first().copied().unwrap_or(0) > 0 {
        // Already bookmarked, return 0 rows affected
        return Ok(0);
    }
    
    let b5 = queries.get_bookmark_query("B5").ok_or(EdmsError::UnknownError)?;
    core.proc(
        b5,
        &[&endpoint_id, &ACTIVE_FOLDER, &notes],
    )
}

pub fn delete_bookmark_active(core: &EdmsCore, queries: &QueryMap, endpoint_id: &str) -> EdmsResult<usize> {
    let b6 = queries.get_bookmark_query("B6").ok_or(EdmsError::UnknownError)?;
    core.proc(
        b6,
        &[&ACTIVE_FOLDER, &endpoint_id],
    )
}

/* ---------------- collections (folder column) ---------------- */

pub fn create_collection_from_active(core: &EdmsCore, queries: &QueryMap, collection: &str) -> EdmsResult<usize> {
    // Copy active bookmarks into folder=collection
    let endpoint_ids = list_bookmarked_endpoints_active(core, queries)?;
    let mut inserted = 0usize;

    for eid in endpoint_ids {
        // Check for duplicates in target collection too
        let b4 = queries.get_bookmark_query("B4").ok_or(EdmsError::UnknownError)?;
        let existing: Vec<i64> = core.cproc(
            b4,
            &[&collection, &eid],
            |row| row.get(0)
        )?;
        
        if existing.first().copied().unwrap_or(0) == 0 {
            let b7 = queries.get_bookmark_query("B7").ok_or(EdmsError::UnknownError)?;
            inserted += core.proc(
                b7,
                &[&eid, &collection],
            )?;
        }
    }

    Ok(inserted)
}

pub fn load_collection_into_active(core: &EdmsCore, queries: &QueryMap, collection: &str) -> EdmsResult<(bool, usize)> {
    let active_count = bookmarks_count_active(core, queries)?;
    let moved_to_backup = active_count > 0;

    if moved_to_backup {
        // Clear old backup before creating new one
        let b2 = queries.get_bookmark_query("B2").ok_or(EdmsError::UnknownError)?;
        let _ = core.proc(b2, &[&SESSION_BACKUP_FOLDER]);
        
        // Copy active into backup
        let endpoint_ids = list_bookmarked_endpoints_active(core, queries)?;
        let b7 = queries.get_bookmark_query("B7").ok_or(EdmsError::UnknownError)?;
        for eid in endpoint_ids {
            let _ = core.proc(
                b7,
                &[&eid, &SESSION_BACKUP_FOLDER],
            )?;
        }
    }

    // Replace active with collection
    clear_bookmarks_active(core, queries)?;
    
    let b3 = queries.get_bookmark_query("B3").ok_or(EdmsError::UnknownError)?;
    let ids: Vec<String> = core.cproc(b3, &[&collection], |row| row.get(0))?;
    let mut loaded = 0usize;
    for eid in ids {
        loaded += insert_bookmark_active(core, queries, &eid, None)?;
    }

    Ok((moved_to_backup, loaded))
}

pub fn restore_from_backup(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    // Clear current active
    clear_bookmarks_active(core, queries)?;
    
    // Copy backup to active
    let b3 = queries.get_bookmark_query("B3").ok_or(EdmsError::UnknownError)?;
    let ids: Vec<String> = core.cproc(b3, &[&SESSION_BACKUP_FOLDER], |row| row.get(0))?;
    
    let mut restored = 0usize;
    for eid in ids {
        restored += insert_bookmark_active(core, queries, &eid, None)?;
    }
    
    Ok(restored)
}

pub fn clear_session_backup(core: &EdmsCore, queries: &QueryMap) -> EdmsResult<usize> {
    let b2 = queries.get_bookmark_query("B2").ok_or(EdmsError::UnknownError)?;
    core.proc(b2, &[&SESSION_BACKUP_FOLDER])
}

pub fn endpoints_for_ids(core: &EdmsCore, _queries: &QueryMap, ids: &[String]) -> EdmsResult<Vec<EndpointDto>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build IN clause with one placeholder per id
    let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
    let query = format!(
        "SELECT endpoint_id, endpoint_str, annotation, method FROM endpoints WHERE endpoint_id IN ({})",
        placeholders.join(", ")
    );

    // Convert ids to params
    let params: Vec<&dyn ToSql> = ids.iter().map(|s| s as &dyn ToSql).collect();

    // Execute batch query
    let endpoints: Vec<EndpointDto> = core.cproc(&query, params.as_slice(), |row| {
        Ok(EndpointDto {
            endpoint_id: row.get(0)?,
            endpoint_str: row.get(1)?,
            annotation: row.get(2)?,
            method: row.get(3)?,
        })
    })?;

    // Preserve the original order from `ids`
    let mut result = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(ep) = endpoints.iter().find(|e| &e.endpoint_id == id) {
            result.push(ep.clone());
        }
    }

    Ok(result)
}
