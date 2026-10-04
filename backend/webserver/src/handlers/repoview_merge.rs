//! Two-way merge between Collections and RepoView, per Ravi (2026-09-29):
//! "Collections <> (WebView, RepoView), it's two way" - the motivation being
//! (1) importing a subset of a RepoView back out, and (2) recovering after
//! someone loses their EDMS instance, since a RepoView holds real copies of
//! its members' data rather than references into the central tables.
//!
//! Everything here sits on the RepoView side: it only *calls* Collections'
//! existing membership API (add members, create a collection) and never
//! changes how Collections work.
//!
//! What makes recovery possible: every time endpoints enter a RepoView
//! (create, add-more, tag-import) `ingest_endpoints` also snapshots each
//! endpoint's central row (URL, method, annotation) and its QP metadata
//! into two extra tables in the RepoView's own SQLite file, alongside the
//! copied files and tags. Exporting back to a Collection can then rebuild
//! an endpoint that no longer exists centrally - same EID, reserved with
//! the allocator so it can't be handed out twice.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::collection_membership_ops::CollectionMembershipOps;
use edms::ops::tag_ops::TagOps;
use edms::ops::view_ops::ViewKind;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path as FsPath;

use crate::{
    db,
    handlers::view_catalog::{
        collection_file_path, copy_dir_recursive, open_catalog, open_existing_membership,
        open_membership, repoview_data_dir,
    },
    state::AppState,
};

const SNAPSHOT_DDL: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS endpoint_snapshot (
        endpoint_id  TEXT PRIMARY KEY,
        endpoint_str TEXT NOT NULL,
        method       TEXT,
        annotation   TEXT
    )",
    "CREATE TABLE IF NOT EXISTS qp_snapshot (
        endpoint_id      TEXT NOT NULL,
        request_number   INTEGER NOT NULL,
        method           TEXT,
        status_code      INTEGER,
        response_time_ms INTEGER,
        PRIMARY KEY (endpoint_id, request_number)
    )",
];

/// Idempotent - also upgrades RepoViews created before snapshots existed
/// (they just have nothing in these tables until something is re-ingested).
pub(crate) fn init_snapshot_tables(membership: &CollectionMembershipOps) -> Result<(), String> {
    for ddl in SNAPSHOT_DDL {
        membership.core.proc(ddl, &[]).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct IngestStats {
    pub added: usize,
    pub data_copied: usize,
    pub tags_copied: usize,
    pub qps_snapshotted: usize,
}

/// The one place that defines "an endpoint enters a RepoView": membership,
/// a real copy of its files, a copy of its current central tags, and a
/// snapshot of its endpoint row + QP metadata (for recovery). Used by
/// create, add-from-collection, and tag-import alike. Safe to re-run for an
/// endpoint that's already a member - it refreshes the copies.
pub(crate) fn ingest_endpoints(
    state: &AppState,
    repoview_name: &str,
    membership: &CollectionMembershipOps,
    endpoint_ids: &[String],
) -> Result<IngestStats, String> {
    init_snapshot_tables(membership)?;

    let mut stats = IngestStats::default();
    stats.added = membership.add_batch(endpoint_ids).map_err(|e| format!("{e:?}"))?;

    let data_dir = repoview_data_dir(state, repoview_name);
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let tag_ops = TagOps::new(&state.db_path.display().to_string());
    tag_ops.initialize().map_err(|e| format!("{e:?}"))?;

    for eid in endpoint_ids {
        let source_dir = state.endpoint_storage_dir(eid);
        if source_dir.is_dir() {
            copy_dir_recursive(&source_dir, &data_dir.join(eid))
                .map_err(|e| format!("failed to copy data for {eid}: {e}"))?;
            stats.data_copied += 1;
        }

        // A RepoView's whole point is being a complete, recoverable
        // snapshot, so tag-copying isn't optional here (unlike Collections'
        // opt-in `export_existing_tags`).
        for tag in tag_ops.get_by_endpoint(eid).map_err(|e| format!("{e:?}"))? {
            if membership.add_tag(eid, &tag).map_err(|e| format!("{e:?}"))? > 0 {
                stats.tags_copied += 1;
            }
        }

        if let Some(ep) = db::get_endpoint(&state.core, &state.queries, eid)
            .map_err(|e| format!("{e:?}"))?
        {
            membership
                .core
                .proc(
                    "INSERT OR REPLACE INTO endpoint_snapshot (endpoint_id, endpoint_str, method, annotation) VALUES (?, ?, ?, ?)",
                    &[&ep.endpoint_id, &ep.endpoint_str, &ep.method, &ep.annotation],
                )
                .map_err(|e| format!("{e:?}"))?;
        }

        for qp in db::list_qps_for_endpoint(&state.core, &state.queries, eid)
            .map_err(|e| format!("{e:?}"))?
        {
            membership
                .core
                .proc(
                    "INSERT OR REPLACE INTO qp_snapshot (endpoint_id, request_number, method, status_code, response_time_ms) VALUES (?, ?, ?, ?, ?)",
                    &[&eid, &qp.request_number, &qp.method, &qp.status_code, &qp.response_time_ms],
                )
                .map_err(|e| format!("{e:?}"))?;
            stats.qps_snapshotted += 1;
        }
    }

    Ok(stats)
}

/// Drops everything this RepoView holds *besides* the membership row and the
/// copied files for one endpoint (those are handled by the caller): its
/// copied tags and its recovery snapshots.
pub(crate) fn forget_endpoint(membership: &CollectionMembershipOps, endpoint_id: &str) -> Result<(), String> {
    init_snapshot_tables(membership)?;
    for sql in [
        "DELETE FROM endpoint_tags WHERE endpoint_id = ?",
        "DELETE FROM endpoint_snapshot WHERE endpoint_id = ?",
        "DELETE FROM qp_snapshot WHERE endpoint_id = ?",
    ] {
        membership.core.proc(sql, &[&endpoint_id]).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

/// Resolves an optional requested subset against a membership set: absent or
/// empty means "all of them" (sorted, for stable output); otherwise every
/// requested id must be a member.
fn select_members(
    requested: Option<Vec<String>>,
    members: &HashSet<String>,
    owner: &str,
) -> Result<Vec<String>, String> {
    match requested {
        Some(ids) if !ids.is_empty() => {
            let mut missing: Vec<&String> = ids.iter().filter(|id| !members.contains(*id)).collect();
            if !missing.is_empty() {
                missing.sort();
                return Err(format!("not members of {owner}: {missing:?}"));
            }
            Ok(ids)
        }
        _ => {
            let mut all: Vec<String> = members.iter().cloned().collect();
            all.sort();
            Ok(all)
        }
    }
}

/// Copies files from `src` into `dst` only where the target doesn't already
/// exist - recovery must never overwrite anything that's still there.
fn copy_missing(src: &FsPath, dst: &FsPath) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copied += copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

// ── Collection -> RepoView ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddFromCollectionRequest {
    pub source_collection: String,
    /// Subset of the collection's members to merge in. Omitted or empty
    /// means all of them.
    #[serde(default)]
    pub endpoint_ids: Option<Vec<String>>,
}

fn stats_json(stats: &IngestStats) -> serde_json::Value {
    json!({
        "endpoints_added": stats.added,
        "endpoints_with_data_copied": stats.data_copied,
        "tags_copied": stats.tags_copied,
        "qps_snapshotted": stats.qps_snapshotted
    })
}

/// POST /repoview/:name/endpoints/add - merge more endpoints into an
/// *existing* RepoView from any Collection (not just the one it was created
/// from). Members already present are refreshed rather than duplicated.
/// The RepoView's recorded `source` stays the collection it was created
/// from - it's a non-modifiable field per the spec, so later merges aren't
/// reflected in it.
pub async fn add_endpoints_to_repoview(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<AddFromCollectionRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<serde_json::Value, String> {
            let membership = open_existing_membership(&state, ViewKind::Repoview, &name)
                .map_err(|_| format!("RepoView '{name}' does not exist"))?;

            let source = open_existing_membership(&state, ViewKind::Collections, &payload.source_collection)
                .map_err(|_| format!("Collection '{}' does not exist", payload.source_collection))?;
            let source_members = source.list_set().map_err(|e| format!("{e:?}"))?;
            let ids = select_members(
                payload.endpoint_ids,
                &source_members,
                &format!("collection '{}'", payload.source_collection),
            )?;

            let stats = ingest_endpoints(&state, &name, &membership, &ids)?;
            let mut body = stats_json(&stats);
            body["ok"] = json!(true);
            body["endpoints_requested"] = json!(ids.len());
            Ok(body)
        }
    })
    .await;

    finish(&state, res)
}

#[derive(Debug, Deserialize)]
pub struct ImportTagsIntoRepoviewRequest {
    pub tags: Vec<String>,
}

/// POST /repoview/:name/tags/import - the "via Tag Ops" half of Ravi's
/// merge: every endpoint carrying any of the given central tags is merged
/// into this RepoView (members, real data, tags and recovery snapshot).
pub async fn import_tags_into_repoview(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<ImportTagsIntoRepoviewRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<serde_json::Value, String> {
            let membership = open_existing_membership(&state, ViewKind::Repoview, &name)
                .map_err(|_| format!("RepoView '{name}' does not exist"))?;

            let tag_ops = TagOps::new(&state.db_path.display().to_string());
            tag_ops.initialize().map_err(|e| format!("{e:?}"))?;

            let mut ids: HashSet<String> = HashSet::new();
            for tag in &payload.tags {
                ids.extend(tag_ops.get_endpoints_by_tag(tag).map_err(|e| format!("{e:?}"))?);
            }
            let mut ids: Vec<String> = ids.into_iter().collect();
            ids.sort();

            let stats = ingest_endpoints(&state, &name, &membership, &ids)?;
            let mut body = stats_json(&stats);
            body["ok"] = json!(true);
            body["endpoints_matched"] = json!(ids.len());
            Ok(body)
        }
    })
    .await;

    finish(&state, res)
}

// ── RepoView -> Collection ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExportToCollectionRequest {
    pub collection: String,
    #[serde(default)]
    pub endpoint_ids: Option<Vec<String>>,
    #[serde(default)]
    pub create_if_missing: bool,
}

/// Restores one endpoint that is missing from the central tables, from this
/// RepoView's own copy. Returns the per-endpoint result plus whether the
/// endpoint is now usable (so it can safely become a collection member).
///
/// Rule: **only when the endpoint no longer exists centrally.** If it's still
/// there, nothing central is touched - not its files, QPs or tags - so a tag
/// or QP someone deliberately removed after the snapshot isn't resurrected.
///
/// "Still there" means the same endpoint, not just the same id: deleting an
/// endpoint through the API releases its EID back to the allocator, so a
/// brand-new, unrelated endpoint can be handed that very id. When the
/// snapshot's URL+method disagree with what now sits under the id, it is
/// reported and skipped rather than treated as the endpoint being recovered.
fn restore_endpoint(
    state: &AppState,
    repoview_name: &str,
    membership: &CollectionMembershipOps,
    tag_ops: &TagOps,
    eid: &str,
) -> Result<(serde_json::Value, bool), String> {
    let skip = |why: String| Ok((json!({ "endpoint_id": eid, "restored_endpoint": false, "skipped": why }), false));

    let snapshot: Option<(String, Option<String>, Option<String>)> = membership
        .core
        .cproc(
            "SELECT endpoint_str, method, annotation FROM endpoint_snapshot WHERE endpoint_id = ?",
            &[&eid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .next();

    if let Some(central) = db::get_endpoint(&state.core, &state.queries, eid).map_err(|e| format!("{e:?}"))? {
        if let Some((snap_str, snap_method, _)) = &snapshot {
            let same_url = central.endpoint_str == *snap_str;
            let same_method = central.method.as_deref().unwrap_or("GET") == snap_method.as_deref().unwrap_or("GET");
            if !(same_url && same_method) {
                return skip(format!(
                    "this EID now belongs to a different endpoint ({} {}) - not added",
                    central.method.as_deref().unwrap_or("GET"),
                    central.endpoint_str
                ));
            }
        }
        return Ok((
            json!({
                "endpoint_id": eid, "restored_endpoint": false,
                "note": "already exists centrally - left untouched"
            }),
            true,
        ));
    }

    let Some((endpoint_str, method, annotation)) = snapshot else {
        return skip("missing centrally, and this RepoView has no snapshot of it to restore from".to_string());
    };

    if !compute::eid::is_valid_eid(eid) {
        return skip(format!("'{eid}' is not a valid canonical EID"));
    }

    // (endpoint_str, method) is only a plain index centrally, not unique -
    // so the DB itself would happily let a restore create a duplicate of an
    // endpoint someone has since re-created under a new id. Guard it here,
    // the same pairing GET /endpoints/lookup uses for dedup.
    let method = method.unwrap_or_else(|| "GET".to_string());
    if let Some(existing) =
        db::find_endpoint_by_str_and_method(&state.core, &state.queries, &endpoint_str, &method)
            .map_err(|e| format!("{e:?}"))?
    {
        return skip(format!(
            "another endpoint ({}) already uses this URL and method",
            existing.endpoint_id
        ));
    }

    // Reserve first, then insert - same order resolve_new_eid uses for a
    // caller-supplied EID - so the allocator can never hand this id out
    // again to a brand-new endpoint.
    state
        .eid_allocator
        .reserve(eid)
        .map_err(|e| format!("failed to reserve EID '{eid}': {e:?}"))?;

    let dto = db::EndpointDto {
        endpoint_id: eid.to_string(),
        endpoint_str,
        annotation,
        method: Some(method),
    };
    if let Err(e) = db::insert_endpoint(&state.core, &state.queries, &dto) {
        let _ = state.eid_allocator.release(eid);
        return skip(format!("{e:?}"));
    }

    let source_dir = repoview_data_dir(state, repoview_name).join(eid);
    let target_dir = state.endpoint_storage_dir(eid);
    let files = if source_dir.is_dir() {
        copy_missing(&source_dir, &target_dir).map_err(|e| format!("failed to restore files for {eid}: {e}"))?
    } else {
        0
    };

    let qps: Vec<(i32, Option<String>, Option<i32>, Option<i32>)> = membership
        .core
        .cproc(
            "SELECT request_number, method, status_code, response_time_ms FROM qp_snapshot WHERE endpoint_id = ? ORDER BY request_number",
            &[&eid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| format!("{e:?}"))?;
    let insert_request = state
        .queries
        .get_request_query("R1")
        .ok_or_else(|| "query R1 missing".to_string())?;
    let insert_response = state
        .queries
        .get_response_query("RES1")
        .ok_or_else(|| "query RES1 missing".to_string())?;
    // Deleting an endpoint through the API doesn't cascade to its QP rows
    // (a known gap), so they can still be sitting there when it's restored.
    // request_metadata has no unique key, so inserting blindly would
    // duplicate them - only add the numbers that aren't already present.
    let already_there: HashSet<i32> = db::list_qps_for_endpoint(&state.core, &state.queries, eid)
        .map_err(|e| format!("{e:?}"))?
        .into_iter()
        .map(|q| q.request_number)
        .collect();
    let mut qps_restored = 0usize;
    for (number, qp_method, status, time) in &qps {
        if already_there.contains(number) {
            continue;
        }
        let request_path = target_dir.join(format!("{eid}-request-{number}.json")).display().to_string();
        let response_path = target_dir.join(format!("{eid}-response-{number}.json")).display().to_string();
        state
            .core
            .proc(insert_request, &[&eid, number, &request_path, qp_method])
            .map_err(|e| format!("{e:?}"))?;
        state
            .core
            .proc(insert_response, &[&eid, number, &response_path, status, time])
            .map_err(|e| format!("{e:?}"))?;
        qps_restored += 1;
    }

    let mut tags_restored = 0usize;
    for tag in membership.list_tags_for_endpoint(eid).map_err(|e| format!("{e:?}"))? {
        tags_restored += tag_ops.add(eid, &tag).map_err(|e| format!("{e:?}"))?;
    }

    Ok((
        json!({
            "endpoint_id": eid, "restored_endpoint": true,
            "restored_files": files, "restored_qps": qps_restored, "restored_tags": tags_restored
        }),
        true,
    ))
}

/// POST /repoview/:name/export-to-collection - the reverse direction. Adds
/// the chosen members to a Collection and, for any that no longer exist in
/// the central tables, rebuilds them from this RepoView's own copy first
/// (endpoint row under its original EID, files, QP metadata, tags). That's
/// Ravi's "somebody lost their EDMS instance" recovery case.
pub async fn export_repoview_to_collection(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<ExportToCollectionRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<serde_json::Value, String> {
            let membership = open_existing_membership(&state, ViewKind::Repoview, &name)
                .map_err(|_| format!("RepoView '{name}' does not exist"))?;
            init_snapshot_tables(&membership)?;
            let members = membership.list_set().map_err(|e| format!("{e:?}"))?;
            let selected = select_members(payload.endpoint_ids, &members, &format!("RepoView '{name}'"))?;

            let collection = payload.collection;
            let catalog = open_catalog(&state)?;
            let existing = catalog.get(ViewKind::Collections, &collection).map_err(|e| format!("{e:?}"))?;
            let (target, created_collection) = match existing {
                Some((_, Some(path), _, _)) => (open_membership(&path)?, false),
                Some((_, None, _, _)) => return Err(format!("Collection '{collection}' has no file yet")),
                None if payload.create_if_missing => {
                    let path = collection_file_path(&state, &collection);
                    let target = open_membership(&path)?;
                    catalog
                        .register(ViewKind::Collections, &collection, Some(&path), None)
                        .map_err(|e| format!("{e:?}"))?;
                    (target, true)
                }
                None => {
                    return Err(format!(
                        "Collection '{collection}' does not exist (pass create_if_missing to create it)"
                    ))
                }
            };

            let tag_ops = TagOps::new(&state.db_path.display().to_string());
            tag_ops.initialize().map_err(|e| format!("{e:?}"))?;

            let mut results = Vec::with_capacity(selected.len());
            let mut usable: Vec<String> = Vec::new();
            let mut restored = 0usize;
            let mut skipped = 0usize;
            for eid in &selected {
                let (result, ok) = restore_endpoint(&state, &name, &membership, &tag_ops, eid)?;
                if result["restored_endpoint"].as_bool().unwrap_or(false) {
                    restored += 1;
                }
                if ok {
                    usable.push(eid.clone());
                } else {
                    skipped += 1;
                }
                results.push(result);
            }

            let members_added = target.add_batch(&usable).map_err(|e| format!("{e:?}"))?;

            Ok(json!({
                "ok": skipped == 0,
                "collection": collection,
                "created_collection": created_collection,
                "members_added": members_added,
                "restored_endpoints": restored,
                "skipped": skipped,
                "results": results
            }))
        }
    })
    .await;

    finish(&state, res)
}

fn finish(
    state: &AppState,
    res: Result<Result<serde_json::Value, String>, tokio::task::JoinError>,
) -> (StatusCode, Json<serde_json::Value>) {
    match res {
        Ok(Ok(body)) => {
            state.refresh_dashboard_snapshot();
            (StatusCode::OK, Json(body))
        }
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("edms-merge-{label}-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn select_members_defaults_to_all_sorted() {
        let all = select_members(None, &set(&["b", "a", "c"]), "x").unwrap();
        assert_eq!(all, vec!["a", "b", "c"]);
        let empty = select_members(Some(vec![]), &set(&["b", "a"]), "x").unwrap();
        assert_eq!(empty, vec!["a", "b"]);
    }

    #[test]
    fn select_members_rejects_ids_that_are_not_members() {
        let err = select_members(Some(vec!["a".into(), "zzz".into()]), &set(&["a"]), "collection 'c'")
            .unwrap_err();
        assert!(err.contains("not members of collection 'c'"));
        assert!(err.contains("zzz"));
        let ok = select_members(Some(vec!["a".into()]), &set(&["a", "b"]), "x").unwrap();
        assert_eq!(ok, vec!["a"]);
    }

    #[test]
    fn copy_missing_never_overwrites_existing_files() {
        let src = temp_dir("src");
        let dst = temp_dir("dst");
        std::fs::write(src.join("kept.json"), "from-repoview").unwrap();
        std::fs::write(src.join("new.json"), "from-repoview").unwrap();
        std::fs::write(dst.join("kept.json"), "already-central").unwrap();

        let copied = copy_missing(&src, &dst).unwrap();

        assert_eq!(copied, 1);
        assert_eq!(std::fs::read_to_string(dst.join("kept.json")).unwrap(), "already-central");
        assert_eq!(std::fs::read_to_string(dst.join("new.json")).unwrap(), "from-repoview");
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(dst).unwrap();
    }

    #[test]
    fn snapshot_tables_are_idempotent_and_forget_clears_one_endpoint_only() {
        let dir = temp_dir("db");
        let path = dir.join("repoview.sqlite").display().to_string();
        let membership = CollectionMembershipOps::new(&path);
        membership.initialize().unwrap();

        init_snapshot_tables(&membership).unwrap();
        init_snapshot_tables(&membership).unwrap(); // second call must be a no-op

        for eid in ["E0001-AAA", "E0002-AAA"] {
            membership.add(eid).unwrap();
            membership.add_tag(eid, "t").unwrap();
            membership
                .core
                .proc(
                    "INSERT INTO endpoint_snapshot (endpoint_id, endpoint_str, method, annotation) VALUES (?, 'https://x/y', 'GET', NULL)",
                    &[&eid],
                )
                .unwrap();
            membership
                .core
                .proc(
                    "INSERT INTO qp_snapshot (endpoint_id, request_number, method, status_code, response_time_ms) VALUES (?, 1, 'GET', 200, 5)",
                    &[&eid],
                )
                .unwrap();
        }

        forget_endpoint(&membership, "E0001-AAA").unwrap();

        let count = |sql: &str| -> i64 {
            membership.core.cproc(sql, &[], |r| r.get::<_, i64>(0)).unwrap()[0]
        };
        assert_eq!(count("SELECT COUNT(*) FROM endpoint_tags"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM endpoint_snapshot"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM qp_snapshot"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM endpoint_snapshot WHERE endpoint_id = 'E0002-AAA'"), 1);

        drop(membership);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
