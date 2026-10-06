//! Import / Export ("takeout") for RepoViews - v1.0 (Ravi, 2026-10-05/06) -
//! and, the same way, WebViews (`Flavor` says which; a WebView's folder also
//! holds `front-page.json`, which travels with it).
//!
//! A RepoView is only a list, so this is the one place its EQP data is
//! touched:
//!
//! - **Takeout** copies the EQP data of every listed endpoint out of
//!   globalEQPData into `storage/takeout/{name}/`, next to the generated
//!   Tables files, with the SQLite index *removed* - "friendly for git or
//!   web". The existing `compute::table_view::takeout_item` does the copy of
//!   the RepoView folder, the SQLite stripping and the name-collision check.
//! - Because a takeout has no SQLite, it also carries
//!   `repoview-manifest.json`: the RepoView's own details plus each
//!   endpoint's URL, method, tags and QP status codes/timings. A real EQP
//!   folder holds only request/response/header files, so without this an
//!   import would have no way to rebuild the index.
//! - **Import** reads a takeout folder, gives every endpoint a *fresh* EID
//!   ("IE always have their own unique EIDs even if the same endpoint exists
//!   in a different EID"), renames the EQP files to match, writes the central
//!   rows, and builds the RepoView's index.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use edms::ops::collection_membership_ops::CollectionMembershipOps;
use edms::ops::tag_ops::TagOps;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};

use crate::{
    db,
    handlers::{
        repoview_index::{build_index, catalog_rows, init_snapshot_tables},
        repoview_tables::{generate_tables_blocking, parse_approach_line, Approach},
        view_catalog::{open_catalog, open_existing_membership, open_membership, validate_folder_name},
        view_flavor::Flavor,
    },
    state::AppState,
};

pub(crate) const FRONT_PAGE_FILE: &str = "front-page.json";
pub(crate) const MANIFEST_FILE: &str = "repoview-manifest.json";
const MANIFEST_FORMAT: &str = "edms-repoview";
const MANIFEST_VERSION: u32 = 1;
const EQP_DIR: &str = "globalEQPData";

// ── The manifest ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub format: String,
    pub version: u32,
    pub repoview: ManifestRepoview,
    pub endpoints: Vec<ManifestEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ManifestRepoview {
    pub name: String,
    pub annotation: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    /// `"repoview"` or `"webview"`. Takeouts made before WebView existed
    /// have no such field, and are RepoViews.
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "repoview".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ManifestEndpoint {
    pub eid: String,
    pub endpoint_str: String,
    pub method: Option<String>,
    pub annotation: Option<String>,
    pub tags: Vec<String>,
    pub qps: Vec<ManifestQp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ManifestQp {
    pub request_number: i32,
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
}

/// Everything an import needs, read straight from the RepoView's own index.
pub(crate) fn build_manifest(
    membership: &CollectionMembershipOps,
    repoview: ManifestRepoview,
) -> Result<Manifest, String> {
    init_snapshot_tables(membership)?;

    let rows: Vec<(String, String, Option<String>, Option<String>)> = membership
        .core
        .cproc(
            "SELECT s.endpoint_id, s.endpoint_str, s.method, s.annotation
             FROM endpoint_snapshot s JOIN membership m ON m.endpoint_id = s.endpoint_id
             ORDER BY s.endpoint_id",
            &[],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| format!("{e:?}"))?;

    let tag_rows: Vec<(String, String)> = membership
        .core
        .cproc(
            "SELECT endpoint_id, tag FROM endpoint_tags
             WHERE endpoint_id IN (SELECT endpoint_id FROM membership) ORDER BY endpoint_id, tag",
            &[],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut tags: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (eid, tag) in tag_rows {
        tags.entry(eid).or_default().push(tag);
    }

    let qp_rows: Vec<(String, ManifestQp)> = membership
        .core
        .cproc(
            "SELECT endpoint_id, request_number, method, status_code, response_time_ms FROM qp_snapshot
             WHERE endpoint_id IN (SELECT endpoint_id FROM membership) ORDER BY endpoint_id, request_number",
            &[],
            |r| {
                Ok((
                    r.get(0)?,
                    ManifestQp {
                        request_number: r.get(1)?,
                        method: r.get(2)?,
                        status_code: r.get(3)?,
                        response_time_ms: r.get(4)?,
                    },
                ))
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut qps: BTreeMap<String, Vec<ManifestQp>> = BTreeMap::new();
    for (eid, qp) in qp_rows {
        qps.entry(eid).or_default().push(qp);
    }

    let endpoints = rows
        .into_iter()
        .map(|(eid, endpoint_str, method, annotation)| ManifestEndpoint {
            tags: tags.remove(&eid).unwrap_or_default(),
            qps: qps.remove(&eid).unwrap_or_default(),
            eid,
            endpoint_str,
            method,
            annotation,
        })
        .collect();

    Ok(Manifest {
        format: MANIFEST_FORMAT.to_string(),
        version: MANIFEST_VERSION,
        repoview,
        endpoints,
    })
}

/// Rejects a manifest that isn't ours, or that would make an import unsafe
/// (an EID is used to build folder and file names, so it must be a real one).
pub(crate) fn validate_manifest(m: &Manifest) -> Result<(), String> {
    if m.format != MANIFEST_FORMAT {
        return Err(format!(
            "not a RepoView takeout: manifest format is '{}', expected '{MANIFEST_FORMAT}'",
            m.format
        ));
    }
    if m.version != MANIFEST_VERSION {
        return Err(format!(
            "unsupported manifest version {} (this build reads version {MANIFEST_VERSION})",
            m.version
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for e in &m.endpoints {
        if !compute::eid::is_valid_eid(&e.eid) {
            return Err(format!("'{}' in the manifest is not a valid EID", e.eid));
        }
        if !seen.insert(e.eid.as_str()) {
            return Err(format!("EID '{}' appears twice in the manifest", e.eid));
        }
        if e.endpoint_str.trim().is_empty() {
            return Err(format!("endpoint '{}' has no URL in the manifest", e.eid));
        }
    }
    Ok(())
}

// ── EID-aware file copying ───────────────────────────────────────────────

/// `E0001-AAA-request-1.json` -> `E0042-AAA-request-1.json` when the file
/// belongs to `old`; any other name is returned unchanged.
pub(crate) fn rename_eid_file(file_name: &str, old: &str, new: &str) -> String {
    match file_name.strip_prefix(old) {
        Some(rest) if rest.starts_with('-') => format!("{new}{rest}"),
        _ => file_name.to_string(),
    }
}

/// Copies the regular files of one EQP folder (they're flat) into `dst`,
/// renaming those that carry the old EID. Subfolders and symlinks are skipped
/// on purpose - a takeout may come from git, so only plain files are trusted.
/// Returns how many files were copied.
pub(crate) fn copy_eqp_dir(src: &FsPath, dst: &FsPath, old: &str, new: &str) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut copied = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        std::fs::copy(entry.path(), dst.join(rename_eid_file(&name, old, new)))?;
        copied += 1;
    }
    Ok(copied)
}

fn takeout_root(state: &AppState) -> PathBuf {
    state.storage_root.join("storage")
}

fn imports_dir(flavor: Flavor, state: &AppState) -> PathBuf {
    state
        .storage_root
        .join("storage")
        .join("imports")
        .join("uncompressed")
        .join(flavor.import_folder())
}

// ── Takeout (export) ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TakeoutRequest {
    /// Folder name under `storage/takeout/`. Defaults to the RepoView's name.
    #[serde(default)]
    pub dest_name: Option<String>,
    /// Replace an existing takeout of that name instead of refusing.
    #[serde(default)]
    pub overwrite: bool,
}

enum IeError {
    Conflict { message: String, options: Vec<&'static str> },
    Bad(String),
}

impl From<String> for IeError {
    fn from(e: String) -> Self {
        IeError::Bad(e)
    }
}

fn respond(state: &AppState, res: Result<Result<Value, IeError>, tokio::task::JoinError>) -> (StatusCode, Json<Value>) {
    match res {
        Ok(Ok(body)) => {
            state.refresh_dashboard_snapshot();
            (StatusCode::OK, Json(body))
        }
        Ok(Err(IeError::Conflict { message, options })) => (
            StatusCode::CONFLICT,
            Json(json!({ "ok": false, "conflict": true, "error": message, "options": options })),
        ),
        Ok(Err(IeError::Bad(e))) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// POST /repoview/:name/takeout
///
/// Copies the EQP data of every listed endpoint out of globalEQPData into
/// `storage/takeout/{dest_name}/globalEQPData/{eid}/`, alongside the Tables
/// files and `repoview-manifest.json`. The SQLite index is never included.
/// If that takeout folder already exists: **409** with `options:
/// ["overwrite", "rename"]` and nothing changes.
pub async fn takeout_repoview(
    State(state): State<AppState>,
    Extension(flavor): Extension<Flavor>,
    Path(name): Path<String>,
    Json(payload): Json<TakeoutRequest>,
) -> (StatusCode, Json<Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || takeout_blocking(flavor, &state, &name, payload.dest_name.as_deref(), payload.overwrite)
    })
    .await;
    respond(&state, res)
}

fn takeout_blocking(flavor: Flavor, state: &AppState, name: &str, dest_name: Option<&str>, overwrite: bool) -> Result<Value, IeError> {
    flavor.validate_name(name)?;
    let dest_name = dest_name.map(str::trim).filter(|d| !d.is_empty()).unwrap_or(name);
    validate_folder_name(dest_name, "Takeout name")?;

    let membership = open_existing_membership(state, flavor.kind(), name)
        .map_err(|_| IeError::Bad(flavor.not_found(name)))?;
    let dir = flavor.dir(state, name);
    if !dir.is_dir() {
        return Err(IeError::Bad(format!("{} '{name}' has no folder yet", flavor.label())));
    }

    let row = catalog_rows(state, &flavor.catalog_query("GET"), &[&name])?
        .into_iter()
        .next()
        .ok_or_else(|| IeError::Bad(flavor.not_found(name)))?;
    let row_tags = flavor.row_tag_ops(state)?.list(name).map_err(|e| format!("{e:?}"))?;
    let manifest = build_manifest(
        &membership,
        ManifestRepoview {
            name: name.to_string(),
            annotation: row.3,
            source: row.4,
            tags: row_tags,
            created_at: row.2,
            kind: flavor.route().to_string(),
        },
    )?;

    // The RepoView folder (Tables files; the SQLite is stripped) goes through
    // the existing takeout - it also owns the name-collision check.
    let result = compute::table_view::takeout_item(&dir, dest_name, &takeout_root(state), overwrite)
        .map_err(|e| IeError::Bad(e.to_string()))?;
    if result.collision {
        return Err(IeError::Conflict {
            message: format!("A takeout named '{dest_name}' already exists"),
            options: vec!["overwrite", "rename"],
        });
    }
    let dest = PathBuf::from(&result.destination);

    // A RepoView made before v1.0 may carry its own old copy of the data;
    // the takeout gets a fresh one from globalEQPData instead.
    let eqp_root = dest.join(EQP_DIR);
    if eqp_root.exists() {
        std::fs::remove_dir_all(&eqp_root).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&eqp_root).map_err(|e| e.to_string())?;

    let mut copied_dirs = 0usize;
    let mut files = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for endpoint in &manifest.endpoints {
        let src = state.endpoint_storage_dir(&endpoint.eid);
        if src.is_dir() {
            files += copy_eqp_dir(&src, &eqp_root.join(&endpoint.eid), &endpoint.eid, &endpoint.eid)
                .map_err(|e| format!("failed to copy the data for {}: {e}", endpoint.eid))?;
            copied_dirs += 1;
        } else if !endpoint.qps.is_empty() {
            // An endpoint with no saved QPs has no EQP folder, and that's
            // normal; only one that *should* have data but doesn't is a gap.
            missing.push(endpoint.eid.clone());
        }
    }

    let json_text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join(MANIFEST_FILE), json_text).map_err(|e| e.to_string())?;

    Ok(json!({
        "ok": missing.is_empty(),
        "destination": result.destination,
        "endpoints": manifest.endpoints.len(),
        "eqp_folders_copied": copied_dirs,
        "eqp_files_copied": files,
        "index_files_stripped": result.sqlite_files_stripped,
        "missing_eqp_data": missing,
        "manifest": MANIFEST_FILE
    }))
}

// ── Import ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// Folder name under `storage/imports/uncompressed/repo/` - a takeout
    /// copied back from git (or straight from `storage/takeout/`).
    pub folder: String,
    /// Name for the new RepoView. Defaults to the name in the manifest.
    #[serde(default)]
    pub name: Option<String>,
}

/// POST /repoview/import
///
/// Reads `repoview-manifest.json` from the folder and builds a RepoView:
/// every endpoint gets a **fresh EID** (even if the same URL already exists
/// under another one), its EQP files are copied into globalEQPData under the
/// new name, its central rows and tags are written, and the RepoView's index
/// is built from them. If the RepoView name is taken: **409** with
/// `options: ["rename"]` - retry with `name`. Endpoints that can't be
/// imported are reported in `skipped`; if none can, nothing is created.
pub async fn import_repoview(
    State(state): State<AppState>,
    Extension(flavor): Extension<Flavor>,
    Json(payload): Json<ImportRequest>,
) -> (StatusCode, Json<Value>) {
    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        move || import_blocking(flavor, &state, &payload.folder, payload.name.as_deref())
    })
    .await;
    respond(&state, res)
}

/// Undoes one endpoint's partial import so a failure never leaves a
/// half-written endpoint behind.
fn rollback_endpoint(state: &AppState, new_eid: &str) {
    let _ = db::delete_endpoint(&state.core, &state.queries, new_eid);
    let _ = std::fs::remove_dir_all(state.endpoint_storage_dir(new_eid));
    let _ = state.eid_allocator.release(new_eid);
}

fn import_blocking(flavor: Flavor, state: &AppState, folder: &str, name_override: Option<&str>) -> Result<Value, IeError> {
    validate_folder_name(folder, "Folder name")?;
    let src = imports_dir(flavor, state).join(folder);
    if !src.is_dir() {
        return Err(IeError::Bad(format!(
            "'{folder}' was not found under storage/imports/uncompressed/{}/",
            flavor.import_folder()
        )));
    }

    let manifest_text = std::fs::read_to_string(src.join(MANIFEST_FILE))
        .map_err(|e| IeError::Bad(format!("can't read {MANIFEST_FILE} in '{folder}': {e}")))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|e| IeError::Bad(format!("{MANIFEST_FILE} in '{folder}' is not valid: {e}")))?;
    validate_manifest(&manifest)?;
    if manifest.repoview.kind != flavor.route() {
        return Err(IeError::Bad(format!(
            "'{folder}' is a {} takeout, not a {} - import it through /{}/import",
            manifest.repoview.kind,
            flavor.route(),
            manifest.repoview.kind
        )));
    }

    let name = name_override
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&manifest.repoview.name)
        .to_string();
    flavor.validate_name(&name)?;
    let catalog = open_catalog(state)?;
    if catalog.get(flavor.kind(), &name).map_err(|e| format!("{e:?}"))?.is_some() {
        return Err(IeError::Conflict {
            message: format!("{} '{name}' already exists", flavor.label()),
            options: vec!["rename"],
        });
    }

    let tag_ops = TagOps::new(&state.db_path.display().to_string());
    tag_ops.initialize().map_err(|e| format!("{e:?}"))?;
    let insert_request = state
        .queries
        .get_request_query("R1")
        .ok_or_else(|| "query R1 is missing".to_string())?;
    let insert_response = state
        .queries
        .get_response_query("RES1")
        .ok_or_else(|| "query RES1 is missing".to_string())?;

    let mut mapping: Vec<Value> = Vec::new();
    let mut new_eids: Vec<String> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();

    for endpoint in &manifest.endpoints {
        let skip = |why: String| json!({ "eid": endpoint.eid, "reason": why });

        let new_eid = match state.eid_allocator.allocate() {
            Ok(e) => e,
            Err(e) => {
                skipped.push(skip(format!("couldn't allocate a new EID: {e:?}")));
                continue;
            }
        };

        let dto = db::EndpointDto {
            endpoint_id: new_eid.clone(),
            endpoint_str: endpoint.endpoint_str.clone(),
            annotation: endpoint.annotation.clone(),
            method: Some(endpoint.method.clone().unwrap_or_else(|| "GET".to_string())),
        };
        if let Err(e) = db::insert_endpoint(&state.core, &state.queries, &dto) {
            let _ = state.eid_allocator.release(&new_eid);
            skipped.push(skip(format!("couldn't create the endpoint: {e:?}")));
            continue;
        }

        let eqp_src = src.join(EQP_DIR).join(&endpoint.eid);
        let eqp_dst = state.endpoint_storage_dir(&new_eid);
        if eqp_src.is_dir() {
            if let Err(e) = copy_eqp_dir(&eqp_src, &eqp_dst, &endpoint.eid, &new_eid) {
                rollback_endpoint(state, &new_eid);
                skipped.push(skip(format!("couldn't copy its EQP data: {e}")));
                continue;
            }
        }

        let mut qps_written = 0usize;
        let mut failed: Option<String> = None;
        for qp in &endpoint.qps {
            let request_path = eqp_dst.join(format!("{new_eid}-request-{}.json", qp.request_number));
            if !request_path.is_file() {
                continue; // no saved request for this QP in the takeout
            }
            let response_path = eqp_dst.join(format!("{new_eid}-response-{}.json", qp.request_number));
            let (req, res) = (request_path.display().to_string(), response_path.display().to_string());
            let written = state
                .core
                .proc(insert_request, &[&new_eid, &qp.request_number, &req, &qp.method])
                .and_then(|_| {
                    state.core.proc(
                        insert_response,
                        &[&new_eid, &qp.request_number, &res, &qp.status_code, &qp.response_time_ms],
                    )
                });
            match written {
                Ok(_) => qps_written += 1,
                Err(e) => {
                    failed = Some(format!("couldn't record QP {}: {e:?}", qp.request_number));
                    break;
                }
            }
        }
        if let Some(why) = failed {
            rollback_endpoint(state, &new_eid);
            skipped.push(skip(why));
            continue;
        }

        for tag in &endpoint.tags {
            let _ = tag_ops.add(&new_eid, tag);
        }

        mapping.push(json!({ "old_eid": endpoint.eid, "new_eid": new_eid, "qps": qps_written }));
        new_eids.push(new_eid);
    }

    if new_eids.is_empty() {
        return Err(IeError::Bad("none of the endpoints in this takeout could be imported".to_string()));
    }

    // Build the RepoView's index from what was just written - the same code
    // Create uses, so an imported RepoView is indistinguishable from a
    // created one.
    let dir = flavor.dir(state, &name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file_path = flavor.file_path(state, &name);
    let created = (|| -> Result<(), String> {
        let membership = open_membership(&file_path)?;
        build_index(state, &membership, &new_eids)?;
        let query = state
            .queries
            .get_catalog_query(&flavor.catalog_query("CREATE"))
            .ok_or_else(|| format!("query {} is missing", flavor.catalog_query("CREATE")))?;
        catalog
            .core
            .proc(
                query,
                &[&name, &file_path, &manifest.repoview.annotation, &manifest.repoview.source],
            )
            .map_err(|e| format!("{e:?}"))?;
        let row_tags = flavor.row_tag_ops(state)?;
        for tag in &manifest.repoview.tags {
            let _ = row_tags.add(&name, tag);
        }
        Ok(())
    })();
    if let Err(e) = created {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(IeError::Bad(format!(
            "the endpoints were imported but the {} couldn't be created: {e}",
            flavor.label()
        )));
    }

    // A WebView's front page comes back with it (it names no EIDs, so it is
    // copied as is - but only if it really is JSON).
    let front_page_restored = flavor == Flavor::Web
        && std::fs::read_to_string(src.join(FRONT_PAGE_FILE))
            .ok()
            .filter(|text| serde_json::from_str::<Value>(text).is_ok())
            .map(|text| std::fs::write(dir.join(FRONT_PAGE_FILE), text).is_ok())
            .unwrap_or(false);

    // A takeout without a (valid) front page still gets the default one.
    if flavor == Flavor::Web && !front_page_restored {
        let _ = crate::handlers::webview_front_page::ensure_default(&dir);
    }

    // The takeout's Tables name EIDs that no longer exist here, so rebuild
    // them (same approach) rather than copying stale files.
    let tables_regenerated = flavor.has_tables()
        && std::fs::read_to_string(src.join("Tables-meta.md"))
        .ok()
        .and_then(|meta| parse_approach_line(&meta))
        .and_then(|api_name| Approach::parse(Some(api_name)).ok())
        .map(|approach| generate_tables_blocking(state, &name, approach, 100).is_ok())
        .unwrap_or(false);

    Ok(json!({
        "ok": skipped.is_empty(),
        "name": name,
        "endpoints_imported": new_eids.len(),
        "mapping": mapping,
        "skipped": skipped,
        "tables_regenerated": tables_regenerated,
        "front_page_restored": front_page_restored
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("edms-ie-{label}-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_manifest() -> Manifest {
        Manifest {
            format: MANIFEST_FORMAT.to_string(),
            version: MANIFEST_VERSION,
            repoview: ManifestRepoview {
                name: "rv".into(),
                annotation: Some("note".into()),
                source: Some("src".into()),
                tags: vec!["production".into()],
                created_at: "2026-10-06 10:00:00".into(),
                kind: "repoview".into(),
            },
            endpoints: vec![ManifestEndpoint {
                eid: "E0001-AAA".into(),
                endpoint_str: "https://x.com/users".into(),
                method: Some("GET".into()),
                annotation: None,
                tags: vec!["t1".into()],
                qps: vec![ManifestQp {
                    request_number: 1,
                    method: Some("GET".into()),
                    status_code: Some(200),
                    response_time_ms: Some(12),
                }],
            }],
        }
    }

    #[test]
    fn eid_files_are_renamed_and_everything_else_is_left_alone() {
        assert_eq!(
            rename_eid_file("E0001-AAA-request-1.json", "E0001-AAA", "E0042-AAA"),
            "E0042-AAA-request-1.json"
        );
        assert_eq!(rename_eid_file("E0001-AAA-headers-12.json", "E0001-AAA", "E0042-AAA"), "E0042-AAA-headers-12.json");
        // another endpoint's file, and a name that only *starts like* the old EID
        assert_eq!(rename_eid_file("E0002-AAA-request-1.json", "E0001-AAA", "E0042-AAA"), "E0002-AAA-request-1.json");
        assert_eq!(rename_eid_file("E0001-AAAX-request-1.json", "E0001-AAA", "E0042-AAA"), "E0001-AAAX-request-1.json");
        assert_eq!(rename_eid_file("notes.txt", "E0001-AAA", "E0042-AAA"), "notes.txt");
    }

    #[test]
    fn copying_an_eqp_folder_renames_the_files_and_skips_subfolders() {
        let src = temp_dir("src");
        let dst = temp_dir("dst").join("E0042-AAA");
        std::fs::write(src.join("E0001-AAA-request-1.json"), "req").unwrap();
        std::fs::write(src.join("E0001-AAA-response-1.json"), "res").unwrap();
        std::fs::write(src.join("readme.txt"), "keep my name").unwrap();
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub").join("hidden.json"), "nope").unwrap();

        let copied = copy_eqp_dir(&src, &dst, "E0001-AAA", "E0042-AAA").unwrap();

        assert_eq!(copied, 3);
        assert_eq!(std::fs::read_to_string(dst.join("E0042-AAA-request-1.json")).unwrap(), "req");
        assert_eq!(std::fs::read_to_string(dst.join("E0042-AAA-response-1.json")).unwrap(), "res");
        assert!(dst.join("readme.txt").exists());
        assert!(!dst.join("sub").exists(), "subfolders must not be followed");
        assert!(!dst.join("E0001-AAA-request-1.json").exists());
        std::fs::remove_dir_all(src).unwrap();
        std::fs::remove_dir_all(dst.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_manifest_survives_a_json_round_trip() {
        let m = sample_manifest();
        let text = serde_json::to_string_pretty(&m).unwrap();
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
        assert!(validate_manifest(&back).is_ok());
    }

    #[test]
    fn a_bad_manifest_is_refused() {
        let mut wrong_format = sample_manifest();
        wrong_format.format = "something-else".into();
        assert!(validate_manifest(&wrong_format).unwrap_err().contains("not a RepoView takeout"));

        let mut wrong_version = sample_manifest();
        wrong_version.version = 99;
        assert!(validate_manifest(&wrong_version).unwrap_err().contains("unsupported manifest version"));

        for bad_eid in ["../E0001-AAA", "E1", "", "E0001-AAA/../x"] {
            let mut m = sample_manifest();
            m.endpoints[0].eid = bad_eid.into();
            assert!(validate_manifest(&m).is_err(), "{bad_eid:?} must be rejected");
        }

        let mut dup = sample_manifest();
        dup.endpoints.push(dup.endpoints[0].clone());
        assert!(validate_manifest(&dup).unwrap_err().contains("twice"));

        let mut no_url = sample_manifest();
        no_url.endpoints[0].endpoint_str = "  ".into();
        assert!(validate_manifest(&no_url).is_err());
    }

    #[test]
    fn the_manifest_is_built_from_the_index() {
        let dir = temp_dir("manifest");
        let m = CollectionMembershipOps::new(&dir.join("repoview.sqlite").display().to_string());
        m.initialize().unwrap();
        init_snapshot_tables(&m).unwrap();
        for (eid, url, method) in [("E0002-AAA", "https://x.com/b", "POST"), ("E0001-AAA", "https://x.com/a", "GET")] {
            m.add(eid).unwrap();
            m.core
                .proc(
                    "INSERT INTO endpoint_snapshot (endpoint_id, endpoint_str, method, annotation, data_size_bytes) VALUES (?, ?, ?, NULL, 5)",
                    &[&eid, &url, &method],
                )
                .unwrap();
        }
        m.add_tag("E0001-AAA", "t1").unwrap();
        m.add_tag("E0001-AAA", "t2").unwrap();
        m.core
            .proc(
                "INSERT INTO qp_snapshot (endpoint_id, request_number, method, status_code, response_time_ms) VALUES ('E0001-AAA', 1, 'GET', 200, 7)",
                &[],
            )
            .unwrap();
        // a stray snapshot row for something that isn't a member must not leak in
        m.core
            .proc(
                "INSERT INTO endpoint_snapshot (endpoint_id, endpoint_str, method, annotation, data_size_bytes) VALUES ('E0099-AAA', 'https://x.com/ghost', 'GET', NULL, 0)",
                &[],
            )
            .unwrap();

        let manifest = build_manifest(&m, sample_manifest().repoview).unwrap();

        let eids: Vec<&str> = manifest.endpoints.iter().map(|e| e.eid.as_str()).collect();
        assert_eq!(eids, vec!["E0001-AAA", "E0002-AAA"]);
        assert_eq!(manifest.endpoints[0].tags, vec!["t1", "t2"]);
        assert_eq!(manifest.endpoints[0].qps.len(), 1);
        assert_eq!(manifest.endpoints[0].qps[0].status_code, Some(200));
        assert!(manifest.endpoints[1].tags.is_empty() && manifest.endpoints[1].qps.is_empty());
        assert!(validate_manifest(&manifest).is_ok());

        drop(m);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
