use crate::tagops::{
    db::{DynResult, endpoints_with_tags_in_collection, insert_if_absent, log_entry, open_core},
    types::*,
};

/// Merge endpoints — filtered by tag sets — from several source collections into
/// a new bookmark collection. Queries: B3/B8 (filter), B9/B7 (dedup + insert).
pub fn merge_by_tags(req: MergeRequest, log: &mut Vec<ActivityEntry>) -> DynResult<()> {
    log_entry(log, format!("Starting merge into '{}'", req.target_collection_name));
    let (core, queries) = open_core(&req.db_path)?;

    for src in &req.sources {
        log_entry(log, format!("  Processing '{}' ({} tag filter(s))", src.collection_id, src.tags.len()));
        let eids = endpoints_with_tags_in_collection(&core, &queries, &src.collection_id, &src.tags)?;
        for eid in &eids {
            insert_if_absent(&core, &queries, eid, &req.target_collection_name)?;
        }
        log_entry(log, format!("    → {} endpoint(s) copied", eids.len()));
    }

    log_entry(log, format!("Merge into '{}' complete.", req.target_collection_name));
    Ok(())
}

/// Create a new collection from endpoints in one source collection filtered by tags.
/// Queries: B3/B8, B9/B7.
pub fn create_from_tags(req: CreateFromTagsRequest, log: &mut Vec<ActivityEntry>) -> DynResult<()> {
    log_entry(log, format!("Creating '{}' from {} tag(s) in '{}'", req.new_collection_name, req.tags.len(), req.source_collection_id));
    let (core, queries) = open_core(&req.db_path)?;

    let eids = endpoints_with_tags_in_collection(&core, &queries, &req.source_collection_id, &req.tags)?;
    for eid in &eids {
        insert_if_absent(&core, &queries, eid, &req.new_collection_name)?;
    }

    log_entry(log, format!("Collection '{}' created ({} endpoint(s)).", req.new_collection_name, eids.len()));
    Ok(())
}

/// Add tags to multiple endpoints. T1 — UNIQUE constraint makes it idempotent.
pub fn bulk_add_tags(req: BulkTagRequest, log: &mut Vec<ActivityEntry>) -> DynResult<()> {
    log_entry(log, format!("Bulk-add {} tag(s) to {} endpoint(s)", req.tags.len(), req.endpoint_ids.len()));
    let (core, queries) = open_core(&req.db_path)?;
    let t1 = queries.get_tag_query("T1").ok_or("missing query T1")?;
    for eid in &req.endpoint_ids {
        for tag in &req.tags {
            core.proc(t1, &[eid, tag])?;
        }
    }
    log_entry(log, "Bulk-add complete.".to_string());
    Ok(())
}

/// Remove tags from multiple endpoints. T4.
pub fn bulk_remove_tags(req: BulkTagRequest, log: &mut Vec<ActivityEntry>) -> DynResult<()> {
    log_entry(log, format!("Bulk-remove {} tag(s) from {} endpoint(s)", req.tags.len(), req.endpoint_ids.len()));
    let (core, queries) = open_core(&req.db_path)?;
    let t4 = queries.get_tag_query("T4").ok_or("missing query T4")?;
    for eid in &req.endpoint_ids {
        for tag in &req.tags {
            core.proc(t4, &[eid, tag])?;
        }
    }
    log_entry(log, "Bulk-remove complete.".to_string());
    Ok(())
}

/// Rename a tag across all endpoints that carry it. T8.
pub fn rename_tag(req: RenameTagRequest, log: &mut Vec<ActivityEntry>) -> DynResult<()> {
    log_entry(log, format!("Renaming tag '{}' → '{}'", req.old_name, req.new_name));
    let (core, queries) = open_core(&req.db_path)?;
    let t8 = queries.get_tag_query("T8").ok_or("missing query T8")?;
    let rows = core.proc(t8, &[&req.new_name, &req.old_name])?;
    log_entry(log, format!("Renamed '{}' → '{}' on {} row(s).", req.old_name, req.new_name, rows));
    Ok(())
}
