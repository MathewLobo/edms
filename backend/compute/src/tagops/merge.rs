use crate::tagops::{
    collection_tags::merge_collection_tags_op,
    db::{
        classify_batch, endpoints_with_tags_in_collection, get_endpoint_details, insert_bookmark,
        insert_endpoint, log_entry, open_core, union_tags, DynResult,
    },
    types::{ActivityEntry, CollectionTagFilter},
};
use edms::EdmsCore;
use edms::query_loader::QueryMap;
use std::collections::HashSet;

pub(super) const DEFAULT_MERGE_BATCH_SIZE: usize = 500;

/// Merge coordinator: bounded-transaction, per-endpoint SQL execution.
///
/// # SQL statement count per batch
///
/// This is NOT a set-based merge. For a 500-endpoint batch the actual count is:
/// - 1  classify query (one SQL, dynamic IN clause)
/// - N  union_tags     (one SQL per matched/net-new endpoint)
/// - N  insert_bm      (one SQL per endpoint)
/// - M  insert_ep      (one SQL per net-new endpoint)
///
/// The scalability mechanism is *bounded transactions* + *idempotent operations*,
/// not a reduction in per-row SQL statements.
///
/// # Transaction semantics (NON-ATOMIC)
///
/// Endpoint batches and the C(T) merge each run in their own `BEGIN IMMEDIATE`
/// transaction. The overall merge is therefore NOT globally atomic:
///
/// - Batch 1..N-1 committed, batch N fails → batches 1..N-1 remain committed.
///   Retry re-classifies; already-merged endpoints become MATCHED, tag union is
///   idempotent. The merge converges correctly on retry.
///
/// - All batches committed, C(T) fails → endpoints are intact, collection tags
///   not yet merged. A retry of the full request is safe and idempotent.
///
/// # C(T) gate
///
/// C(T) is merged **if and only if** `src.tags.is_empty()` (whole-folder merge).
/// Filtered merges (`tags` non-empty) do NOT touch `collection_tag_memberships`.
/// There is no other code path that invokes CT_MERGE.
pub fn merge_by_tags_coordinator(
    db_path: &str,
    target_collection: &str,
    sources: &[CollectionTagFilter],
    log: &mut Vec<ActivityEntry>,
) -> DynResult<()> {
    if sources.is_empty() {
        return Err("No sources provided".into());
    }

    for src in sources {
        if src.collection_id == target_collection {
            return Err(format!(
                "Self-merge is not allowed: source and target are both '{}'",
                target_collection
            )
            .into());
        }
    }

    let (core, queries) = open_core(db_path)?;
    log_entry(log, format!("Starting merge into '{}'", target_collection));

    for src in sources {
        // Sole gate: empty tag filter = whole-folder = merge C(E,T) + C(T).
        // Non-empty tag filter = filtered merge = C(E,T) only, C(T) unchanged.
        let is_whole_folder = src.tags.is_empty();

        log_entry(
            log,
            format!(
                "  Processing '{}' ({} tag filter(s))",
                src.collection_id,
                src.tags.len()
            ),
        );

        let source_eids = endpoints_with_tags_in_collection(
            &core,
            &queries,
            &src.collection_id,
            &src.tags,
        )?;
        let source_eids = deduplicate_logical_endpoints(&core, &queries, source_eids)?;

        let mut total_matched = 0;
        let mut total_net_new = 0;

        for chunk in source_eids.chunks(DEFAULT_MERGE_BATCH_SIZE) {
            let (matched, net_new) =
                process_merge_batch(&core, &queries, target_collection, chunk)?;
            total_matched += matched;
            total_net_new += net_new;
            log_entry(
                log,
                format!(
                    "    → Batch: processed {}, matched {}, net-new {}",
                    chunk.len(),
                    matched,
                    net_new
                ),
            );
        }

        // C(T): runs ONCE per source, AFTER all its endpoint batches,
        // ONLY for whole-folder merges, in its own short transaction.
        if is_whole_folder {
            core.base.execute("BEGIN IMMEDIATE", &[])?;
            match merge_collection_tags_op(&core, &queries, &src.collection_id, target_collection)
            {
                Ok(tags_added) => {
                    let _ = core.base.execute("COMMIT", &[]);
                    log_entry(log, format!("    → Collection tags merged ({} new)", tags_added));
                }
                Err(e) => {
                    let _ = core.base.execute("ROLLBACK", &[]);
                    return Err(e);
                }
            }
        }

        log_entry(
            log,
            format!(
                "  Completed '{}': matched {}, net-new {}",
                src.collection_id, total_matched, total_net_new
            ),
        );
    }

    log_entry(log, format!("Merge into '{}' complete.", target_collection));
    Ok(())
}

fn deduplicate_logical_endpoints(
    core: &EdmsCore,
    queries: &QueryMap,
    eids: Vec<String>,
) -> DynResult<Vec<String>> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for eid in eids {
        if let Ok((ep_str, method)) = get_endpoint_details(core, queries, &eid) {
            // logical identity = endpoint_str + COALESCE(method, '')
            let logical_id = format!("{}::{}", ep_str, method);
            if seen.insert(logical_id) {
                deduped.push(eid);
            }
        }
    }
    Ok(deduped)
}

fn process_merge_batch(
    core: &EdmsCore,
    queries: &QueryMap,
    target_collection: &str,
    source_eids: &[String],
) -> DynResult<(usize, usize)> {
    core.base.execute("BEGIN IMMEDIATE", &[])?;

    let classifications =
        match classify_batch(core, queries, target_collection, source_eids) {
            Ok(c) => c,
            Err(e) => {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
        };

    let mut matched = 0;
    let mut net_new = 0;

    for cls in classifications {
        if let Some(target_eid) = cls.target_eid {
            // MATCHED: retain target endpoint_id, union source tags, ensure bookmark.
            if let Err(e) = union_tags(core, queries, &cls.source_eid, &target_eid) {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
            if let Err(e) = insert_bookmark(core, queries, &target_eid, target_collection) {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
            matched += 1;
        } else {
            // NET-NEW: generate UUID, insert endpoint, copy tags, create bookmark.
            let new_eid = uuid::Uuid::new_v4().to_string();
            let method = if cls.method.is_empty() { "" } else { cls.method.as_str() };

            if let Err(e) = insert_endpoint(
                core,
                queries,
                &new_eid,
                &cls.endpoint_str,
                cls.annotation.as_deref(),
                method,
            ) {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
            if let Err(e) = union_tags(core, queries, &cls.source_eid, &new_eid) {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
            if let Err(e) = insert_bookmark(core, queries, &new_eid, target_collection) {
                let _ = core.base.execute("ROLLBACK", &[]);
                return Err(e);
            }
            net_new += 1;
        }
    }

    core.base.execute("COMMIT", &[])?;
    Ok((matched, net_new))
}
