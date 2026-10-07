//! RepoView / WebView Import-Export work that touches many files: takeout
//! (copy every listed endpoint's EQP data out, index stripped) and the EQP
//! half of an import. They run in `edms-child` so the webserver's request
//! returns at once; each reports progress and the final result back through
//! the usual callback, tagged with the `job_id` the webserver gave it.
//!
//! Only plain filesystem work lives here. Everything that needs the central
//! database (reading the index, writing endpoint/QP/tag rows, EID
//! allocation) stays in the webserver, which hands this module ready-made
//! paths and lists.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::table_view::{check_item_format, takeout_item, ViewPurpose};
use std::io::Read;

/// `E0001-AAA-request-1.json` -> `E0042-AAA-request-1.json` when the file
/// belongs to `old`; any other name is returned unchanged.
pub fn rename_eid_file(file_name: &str, old: &str, new: &str) -> String {
    match file_name.strip_prefix(old) {
        Some(rest) if rest.starts_with('-') => format!("{new}{rest}"),
        _ => file_name.to_string(),
    }
}

/// Copies the regular files of one EQP folder (they're flat) into `dst`,
/// renaming those that carry the old EID. Subfolders and symlinks are skipped
/// on purpose - a takeout may come from git, so only plain files are trusted.
/// Returns how many files were copied.
pub fn copy_eqp_dir(src: &Path, dst: &Path, old: &str, new: &str) -> std::io::Result<usize> {
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

/// Lets a long loop report progress without flooding: at most one report per
/// `interval`, but always the last one.
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval_ms: u64) -> Self {
        Throttle { interval: Duration::from_millis(interval_ms), last: None }
    }

    pub fn ready(&mut self, done: usize, total: usize) -> bool {
        let due = done >= total || self.last.map_or(true, |t| t.elapsed() >= self.interval);
        if due {
            self.last = Some(Instant::now());
        }
        due
    }
}

fn progress_json(step: &str, done: usize, total: usize) -> Value {
    json!({ "step": step, "done": done, "total": total })
}

// ── Takeout ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqpEntry {
    pub eid: String,
    /// The index lists saved QPs for it. An endpoint with none has no EQP
    /// folder, which is normal; one that should have data but doesn't is a
    /// gap, reported in `missing_eqp_data`.
    pub has_qps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewTakeoutRequest {
    /// Echoed back in the result so the webserver can find its job.
    pub job_id: String,
    /// The view's own folder (index + generated files / front page). Copied
    /// with every SQLite file stripped.
    pub view_dir: String,
    /// `storage/takeout/{dest_name}/` is the destination.
    pub dest_name: String,
    /// The `storage` directory (its `takeout` folder holds the takeouts).
    pub storage_dir: String,
    pub overwrite: bool,
    /// `storage/globalEQPData`
    pub eqp_root: String,
    pub endpoints: Vec<EqpEntry>,
    /// Written next to the data (the index describes the endpoints; a real
    /// EQP folder can't).
    pub manifest: Value,
    pub manifest_file: String,
}

/// Copies the view folder (index stripped) and each endpoint's EQP data into
/// the takeout, then writes the manifest. `progress` gets `{step, done,
/// total}` reports while endpoints are copied. A name collision is an error
/// here: the webserver checks it first (so the caller gets an immediate 409),
/// this only catches a takeout that appeared in between.
pub fn run_takeout(req: &ViewTakeoutRequest, progress: &mut dyn FnMut(Value)) -> Result<Value, String> {
    let result = takeout_item(
        Path::new(&req.view_dir),
        &req.dest_name,
        Path::new(&req.storage_dir),
        req.overwrite,
    )
    .map_err(|e| e.to_string())?;
    if result.collision {
        return Err(format!("a takeout named '{}' already exists", req.dest_name));
    }
    let dest = PathBuf::from(&result.destination);

    // A view made before v1.0 may carry its own old copy of the data; the
    // takeout gets a fresh one from globalEQPData instead.
    let eqp_dest = dest.join("globalEQPData");
    if eqp_dest.exists() {
        std::fs::remove_dir_all(&eqp_dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&eqp_dest).map_err(|e| e.to_string())?;

    let total = req.endpoints.len();
    let mut throttle = Throttle::new(150);
    let mut copied_dirs = 0usize;
    let mut files = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for (i, endpoint) in req.endpoints.iter().enumerate() {
        let src = Path::new(&req.eqp_root).join(&endpoint.eid);
        if src.is_dir() {
            files += copy_eqp_dir(&src, &eqp_dest.join(&endpoint.eid), &endpoint.eid, &endpoint.eid)
                .map_err(|e| format!("failed to copy the data for {}: {e}", endpoint.eid))?;
            copied_dirs += 1;
        } else if endpoint.has_qps {
            missing.push(endpoint.eid.clone());
        }
        if throttle.ready(i + 1, total) {
            progress(progress_json("copying EQP data", i + 1, total));
        }
    }

    let manifest_text = serde_json::to_string_pretty(&req.manifest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join(&req.manifest_file), manifest_text).map_err(|e| e.to_string())?;

    Ok(json!({
        "ok": missing.is_empty(),
        "destination": result.destination,
        "endpoints": total,
        "eqp_folders_copied": copied_dirs,
        "eqp_files_copied": files,
        "index_files_stripped": result.sqlite_files_stripped,
        "missing_eqp_data": missing,
        "manifest": req.manifest_file
    }))
}

// ── Import (the EQP half) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportItem {
    /// The EID inside the takeout.
    pub old_eid: String,
    /// The fresh EID the webserver allocated for it.
    pub new_eid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewImportCopyRequest {
    pub job_id: String,
    /// The takeout folder (its `globalEQPData/{old_eid}/` is the source).
    pub src_dir: String,
    /// `storage/globalEQPData`: `{new_eid}/` is created under it.
    pub eqp_root: String,
    pub items: Vec<ImportItem>,
}

/// An EID becomes a folder name, so it must be a plain one.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Copies each item's EQP files into `globalEQPData/{new_eid}/`, renamed to
/// the new EID. One item failing doesn't stop the others: every item gets its
/// own `{files, error}` in the result and the webserver decides what to do
/// (it rolls the failed ones back). An item with no EQP folder in the takeout
/// is fine - an endpoint with no saved QPs has none.
///
/// A folder already at `{new_eid}` is cleared first: the EID was only just
/// allocated, so whatever is there belongs to an endpoint that was deleted
/// (deleting an endpoint leaves its files behind) and must not mix into the
/// imported one.
pub fn run_import_copy(req: &ViewImportCopyRequest, progress: &mut dyn FnMut(Value)) -> Result<Value, String> {
    let total = req.items.len();
    let mut throttle = Throttle::new(150);
    let mut results: Vec<Value> = Vec::with_capacity(total);

    for (i, item) in req.items.iter().enumerate() {
        let outcome = if !is_plain_name(&item.old_eid) || !is_plain_name(&item.new_eid) {
            Err("not a valid EID".to_string())
        } else {
            let src = Path::new(&req.src_dir).join("globalEQPData").join(&item.old_eid);
            let dst = Path::new(&req.eqp_root).join(&item.new_eid);
            if !src.is_dir() {
                Ok(0)
            } else {
                let copied = (|| -> std::io::Result<usize> {
                    if dst.exists() {
                        std::fs::remove_dir_all(&dst)?;
                    }
                    copy_eqp_dir(&src, &dst, &item.old_eid, &item.new_eid)
                })();
                match copied {
                    Ok(n) => Ok(n),
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&dst);
                        Err(e.to_string())
                    }
                }
            }
        };
        results.push(match outcome {
            Ok(files) => json!({ "old_eid": item.old_eid, "new_eid": item.new_eid, "files": files, "error": null }),
            Err(e) => json!({ "old_eid": item.old_eid, "new_eid": item.new_eid, "files": 0, "error": e }),
        });
        if throttle.ready(i + 1, total) {
            progress(progress_json("copying EQP data", i + 1, total));
        }
    }

    let failed = results.iter().filter(|r| !r["error"].is_null()).count();
    Ok(json!({ "ok": failed == 0, "items": results }))
}

// ── Format check (the Import/Export table's check button) ────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewCheckRequest {
    pub job_id: String,
    /// A folder, or a `.zip`, under `storage/imports/`.
    pub item_path: String,
    pub purpose: ViewPurpose,
}

/// Runs the existing `check_item_format` as a job. A zip is unpacked first
/// with the strict, size-capped extractor above (the plain check extracts
/// whatever it is given), and the check then runs on that copy. An archive
/// that can't be opened safely fails the check instead of being unpacked.
pub fn run_check(req: &ViewCheckRequest, progress: &mut dyn FnMut(Value)) -> Result<Value, String> {
    let item = Path::new(&req.item_path);
    let report = if item.is_file() {
        let temp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
        match extract_zip_strict(item, temp.path(), progress) {
            Ok(_) => check_item_format(temp.path(), req.purpose),
            Err(why) => crate::validate::ValidationReport {
                passed: false,
                sqlite_status: crate::validate::ComponentStatus::Fail,
                edms_data_status: crate::validate::ComponentStatus::Fail,
                details: vec![why],
            },
        }
    } else {
        check_item_format(item, req.purpose)
    };
    Ok(json!({ "passed": report.passed, "report": report }))
}

// ── Unzip (for importing a zipped takeout) ───────────────────────────────

/// Most entries and most uncompressed bytes an archive may hold. A takeout
/// can come from anywhere, so a zip bomb must not be able to fill the disk.
const MAX_ZIP_ENTRIES: usize = 100_000;
const MAX_ZIP_BYTES: u64 = 20 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewUnzipRequest {
    pub job_id: String,
    /// The `.zip` under `storage/imports/compressed/...`.
    pub zip_path: String,
    /// Where the takeout folder ends up (`storage/imports/uncompressed/.../{stem}`).
    pub dest_dir: String,
    /// Replace `dest_dir` if it already exists (otherwise that is an error).
    pub replace: bool,
    /// The file that proves it is a takeout; it must be at the top of the
    /// archive, or inside the one folder that wraps everything.
    pub manifest_file: String,
}

/// Unzips a takeout. Strict, because the archive is untrusted: any entry that
/// could land outside the destination (`..`, absolute paths, backslashes, a
/// drive letter, symlinks) refuses the whole archive instead of being skipped.
/// It is extracted next to its destination first and only moved into place
/// once it checks out, so a refused or failed archive leaves nothing behind.
pub fn run_unzip(req: &ViewUnzipRequest, progress: &mut dyn FnMut(Value)) -> Result<Value, String> {
    let dest = PathBuf::from(&req.dest_dir);
    if dest.exists() && !req.replace {
        return Err(format!("'{}' already exists", dest.display()));
    }
    let parent = dest.parent().ok_or_else(|| "bad destination".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stem = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let temp = parent.join(format!(".{stem}.extracting-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;

    let finish = (|| -> Result<(usize, u64), String> {
        let (files, bytes) = extract_zip_strict(Path::new(&req.zip_path), &temp, progress)?;
        let root = takeout_root_of(&temp, &req.manifest_file)?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&root, &dest).map_err(|e| e.to_string())?;
        Ok((files, bytes))
    })();
    let _ = std::fs::remove_dir_all(&temp);
    let (files, bytes) = finish?;

    Ok(json!({ "ok": true, "folder": stem, "destination": dest.display().to_string(), "files": files, "bytes": bytes }))
}

fn extract_zip_strict(zip_path: &Path, into: &Path, progress: &mut dyn FnMut(Value)) -> Result<(usize, u64), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("can't open the zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip: {e}"))?;
    let total = archive.len();
    if total > MAX_ZIP_ENTRIES {
        return Err(format!("the zip has {total} entries; the limit is {MAX_ZIP_ENTRIES}"));
    }

    let mut declared: u64 = 0;
    for i in 0..total {
        declared = declared.saturating_add(archive.by_index(i).map_err(|e| e.to_string())?.size());
    }
    if declared > MAX_ZIP_BYTES {
        return Err(format!("the zip would unpack to {declared} bytes; the limit is {MAX_ZIP_BYTES}"));
    }

    let mut throttle = Throttle::new(150);
    let mut files = 0usize;
    let mut written: u64 = 0;
    for i in 0..total {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let raw = entry.name().to_string();
        let unsafe_entry = || format!("refusing the zip: unsafe entry '{raw}'");

        // absolute names are refused too, though `enclosed_name` would tame them
        // (a ':' is a Windows drive prefix: `C:evil` means "on drive C")
        if raw.contains(['\\', '\0', ':']) || raw.starts_with('/') {
            return Err(unsafe_entry());
        }
        // symlinks (unix mode 0o120000) could point anywhere
        if entry.unix_mode().map_or(false, |m| m & 0o170000 == 0o120000) {
            return Err(unsafe_entry());
        }
        let rel = entry.enclosed_name().ok_or_else(unsafe_entry)?;
        let plain = rel.components().all(|c| match c {
            std::path::Component::Normal(part) => !part.to_string_lossy().contains(':'),
            _ => false,
        });
        if !plain {
            return Err(unsafe_entry());
        }

        let out = into.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut outfile = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            // Bounded by what's left of the budget, whatever the header claims.
            let budget = MAX_ZIP_BYTES - written;
            let n = std::io::copy(&mut (&mut entry).take(budget + 1), &mut outfile).map_err(|e| e.to_string())?;
            written += n;
            if written > MAX_ZIP_BYTES {
                return Err(format!("the zip unpacks to more than {MAX_ZIP_BYTES} bytes"));
            }
            files += 1;
        }
        if throttle.ready(i + 1, total) {
            progress(progress_json("unzipping", i + 1, total));
        }
    }
    Ok((files, written))
}

/// The folder holding the takeout: `dir` itself, or the one folder wrapping
/// everything (zipping a folder usually does that).
fn takeout_root_of(dir: &Path, manifest_file: &str) -> Result<PathBuf, String> {
    if dir.join(manifest_file).is_file() {
        return Ok(dir.to_path_buf());
    }
    let mut folders: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| p.file_name().map_or(true, |n| n != "__MACOSX"))
        .collect();
    if folders.len() == 1 && folders[0].join(manifest_file).is_file() {
        return Ok(folders.remove(0));
    }
    Err(format!(
        "no {manifest_file} in the zip (it must be at the top, or inside the one folder that holds everything)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("edms-viewio-{label}-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn renames_only_files_of_the_old_eid() {
        assert_eq!(rename_eid_file("E0001-AAA-request-1.json", "E0001-AAA", "E0042-AAA"), "E0042-AAA-request-1.json");
        assert_eq!(rename_eid_file("notes.txt", "E0001-AAA", "E0042-AAA"), "notes.txt");
        // a longer EID that merely starts the same way is not the same EID
        assert_eq!(rename_eid_file("E0001-AAAB-x.json", "E0001-AAA", "E0042-AAA"), "E0001-AAAB-x.json");
    }

    #[test]
    fn copies_flat_files_renamed_and_skips_subfolders() {
        let root = temp_dir("copy");
        let src = root.join("src");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("E0001-AAA-request-1.json"), "{}").unwrap();
        std::fs::write(src.join("E0001-AAA-response-1.json"), "{}").unwrap();
        std::fs::write(src.join("nested").join("evil.json"), "{}").unwrap();

        let dst = root.join("dst");
        let n = copy_eqp_dir(&src, &dst, "E0001-AAA", "E0099-AAA").unwrap();
        assert_eq!(n, 2);
        assert!(dst.join("E0099-AAA-request-1.json").is_file());
        assert!(dst.join("E0099-AAA-response-1.json").is_file());
        assert!(!dst.join("nested").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_throttle_always_lets_the_last_report_through() {
        let mut t = Throttle::new(60_000);
        assert!(t.ready(1, 10), "the first report goes out");
        assert!(!t.ready(2, 10), "then it waits");
        assert!(!t.ready(9, 10));
        assert!(t.ready(10, 10), "but the final one always does");
    }

    fn request(root: &Path, dest: &str, overwrite: bool) -> ViewTakeoutRequest {
        ViewTakeoutRequest {
            job_id: "job-1".into(),
            view_dir: root.join("view").display().to_string(),
            dest_name: dest.into(),
            storage_dir: root.join("storage").display().to_string(),
            overwrite,
            eqp_root: root.join("eqp").display().to_string(),
            endpoints: vec![
                EqpEntry { eid: "E0001-AAA".into(), has_qps: true },
                EqpEntry { eid: "E0002-AAA".into(), has_qps: false }, // no QPs: no folder, normal
                EqpEntry { eid: "E0003-AAA".into(), has_qps: true },  // should have data, doesn't
            ],
            manifest: json!({ "format": "edms-repoview" }),
            manifest_file: "repoview-manifest.json".into(),
        }
    }

    fn fixture(label: &str) -> PathBuf {
        let root = temp_dir(label);
        std::fs::create_dir_all(root.join("view")).unwrap();
        std::fs::write(root.join("view").join("repoview.sqlite"), "not really sqlite").unwrap();
        std::fs::write(root.join("view").join("Tables-001.md"), "# t").unwrap();
        std::fs::create_dir_all(root.join("eqp").join("E0001-AAA")).unwrap();
        std::fs::write(root.join("eqp").join("E0001-AAA").join("E0001-AAA-request-1.json"), "{}").unwrap();
        std::fs::write(root.join("eqp").join("E0001-AAA").join("E0001-AAA-response-1.json"), "{}").unwrap();
        root
    }

    #[test]
    fn a_takeout_strips_the_index_copies_the_data_and_reports_gaps_and_progress() {
        let root = fixture("takeout");
        let mut reports: Vec<Value> = Vec::new();
        let out = run_takeout(&request(&root, "tk", false), &mut |p| reports.push(p)).unwrap();

        let dest = root.join("storage").join("takeout").join("tk");
        assert!(dest.join("Tables-001.md").is_file());
        assert!(!dest.join("repoview.sqlite").exists(), "the index must never be in a takeout");
        assert!(dest.join("globalEQPData/E0001-AAA/E0001-AAA-request-1.json").is_file());
        assert!(dest.join("repoview-manifest.json").is_file());

        assert_eq!(out["ok"], false, "E0003 should have data and doesn't");
        assert_eq!(out["missing_eqp_data"], json!(["E0003-AAA"]));
        assert_eq!(out["eqp_folders_copied"], 1);
        assert_eq!(out["eqp_files_copied"], 2);
        assert_eq!(out["index_files_stripped"], 1);
        assert_eq!(out["endpoints"], 3);

        let last = reports.last().expect("progress was reported");
        assert_eq!(last["done"], 3);
        assert_eq!(last["total"], 3);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn import_fixture(label: &str) -> PathBuf {
        let root = temp_dir(label);
        let src = root.join("takeout/globalEQPData/E0001-AAA");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("E0001-AAA-request-1.json"), "{}").unwrap();
        std::fs::write(src.join("E0001-AAA-response-1.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("eqp")).unwrap();
        root
    }

    fn import_request(root: &Path, items: Vec<(&str, &str)>) -> ViewImportCopyRequest {
        ViewImportCopyRequest {
            job_id: "job-9".into(),
            src_dir: root.join("takeout").display().to_string(),
            eqp_root: root.join("eqp").display().to_string(),
            items: items
                .into_iter()
                .map(|(o, n)| ImportItem { old_eid: o.into(), new_eid: n.into() })
                .collect(),
        }
    }

    #[test]
    fn an_import_copy_renames_to_the_new_eid_and_reports_each_item() {
        let root = import_fixture("import");
        let mut reports = Vec::new();
        let out = run_import_copy(
            &import_request(&root, vec![("E0001-AAA", "E0042-AAA"), ("E0002-AAA", "E0043-AAA")]),
            &mut |p| reports.push(p),
        )
        .unwrap();

        assert!(root.join("eqp/E0042-AAA/E0042-AAA-request-1.json").is_file());
        assert!(root.join("eqp/E0042-AAA/E0042-AAA-response-1.json").is_file());
        assert!(!root.join("eqp/E0042-AAA/E0001-AAA-request-1.json").exists());
        assert_eq!(out["ok"], true);
        assert_eq!(out["items"][0]["files"], 2);
        // no EQP folder in the takeout for the second one: fine, nothing to copy
        assert_eq!(out["items"][1]["files"], 0);
        assert!(out["items"][1]["error"].is_null());
        assert!(!root.join("eqp/E0043-AAA").exists());
        assert_eq!(reports.last().unwrap()["done"], 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_stale_folder_at_the_new_eid_is_cleared_not_mixed_in() {
        let root = import_fixture("stale");
        let stale = root.join("eqp/E0042-AAA");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("E0042-AAA-request-7.json"), "left over from a deleted endpoint").unwrap();

        run_import_copy(&import_request(&root, vec![("E0001-AAA", "E0042-AAA")]), &mut |_| {}).unwrap();
        assert!(!stale.join("E0042-AAA-request-7.json").exists());
        assert!(stale.join("E0042-AAA-request-1.json").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn eids_that_could_escape_the_folder_are_refused_per_item() {
        let root = import_fixture("escape");
        let out = run_import_copy(
            &import_request(&root, vec![("../evil", "E0042-AAA"), ("E0001-AAA", "..\\..\\x"), ("E0001-AAA", "E0044-AAA")]),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(out["ok"], false);
        assert!(!out["items"][0]["error"].is_null());
        assert!(!out["items"][1]["error"].is_null());
        assert!(out["items"][2]["error"].is_null(), "a bad item must not stop the good ones");
        assert!(root.join("eqp/E0044-AAA/E0044-AAA-request-1.json").is_file());
        assert!(!root.join("eqp/E0042-AAA").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    // ── format check ──

    #[test]
    fn a_zip_is_checked_through_the_strict_extractor() {
        let root = temp_dir("check");
        // a webview takeout: JSON only, no SQLite
        make_zip(&root.join("ok.zip"), &[("front-page.json", "{}"), ("globalEQPData/E1/E1-request-1.json", "{}")]);
        let mut req = ViewCheckRequest {
            job_id: "job-4".into(),
            item_path: root.join("ok.zip").display().to_string(),
            purpose: ViewPurpose::WebView,
        };
        let out = run_check(&req, &mut |_| {}).unwrap();
        assert_eq!(out["passed"], true, "{out}");

        // a SQLite file makes it a failed WebView check, as the existing check says
        make_zip(&root.join("bad.zip"), &[("x.sqlite", "no")]);
        req.item_path = root.join("bad.zip").display().to_string();
        assert_eq!(run_check(&req, &mut |_| {}).unwrap()["passed"], false);

        // an unsafe archive fails the check, with the reason, and is never unpacked
        make_zip(&root.join("evil.zip"), &[("../evil.txt", "x")]);
        req.item_path = root.join("evil.zip").display().to_string();
        let out = run_check(&req, &mut |_| {}).unwrap();
        assert_eq!(out["passed"], false);
        assert!(out["report"]["details"][0].as_str().unwrap().contains("unsafe entry"), "{out}");
        assert!(!root.join("evil.txt").exists());

        // a folder is checked in place
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("dir/front-page.json"), "{}").unwrap();
        req.item_path = root.join("dir").display().to_string();
        assert_eq!(run_check(&req, &mut |_| {}).unwrap()["passed"], true);
        std::fs::remove_dir_all(root).unwrap();
    }

    // ── unzip ──

    fn make_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn unzip_request(root: &Path, replace: bool) -> ViewUnzipRequest {
        ViewUnzipRequest {
            job_id: "job-3".into(),
            zip_path: root.join("t.zip").display().to_string(),
            dest_dir: root.join("out/t").display().to_string(),
            replace,
            manifest_file: "repoview-manifest.json".into(),
        }
    }

    fn leftovers(root: &Path) -> Vec<String> {
        std::fs::read_dir(root.join("out"))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn a_zipped_takeout_is_unpacked_whether_or_not_a_folder_wraps_it() {
        for wrapped in [false, true] {
            let root = temp_dir("unzip");
            let prefix = if wrapped { "takeout/" } else { "" };
            make_zip(
                &root.join("t.zip"),
                &[
                    (&format!("{prefix}repoview-manifest.json"), "{}"),
                    (&format!("{prefix}globalEQPData/E0001-AAA/E0001-AAA-request-1.json"), "{}"),
                ],
            );
            let mut reports = Vec::new();
            let out = run_unzip(&unzip_request(&root, false), &mut |p| reports.push(p)).unwrap();

            assert!(root.join("out/t/repoview-manifest.json").is_file(), "wrapped={wrapped}");
            assert!(root.join("out/t/globalEQPData/E0001-AAA/E0001-AAA-request-1.json").is_file());
            assert_eq!(out["files"], 2);
            assert_eq!(out["folder"], "t");
            assert_eq!(reports.last().unwrap()["done"], 2);
            assert_eq!(leftovers(&root), vec!["t"], "no temp folder left behind");
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn an_archive_with_an_unsafe_entry_is_refused_whole_and_leaves_nothing() {
        for bad in ["../evil.txt", "/abs.txt", "a/../../evil.txt", "..\\evil.txt", "C:evil.txt", "ok/C:/x"] {
            let root = temp_dir("unzip-bad");
            make_zip(&root.join("t.zip"), &[("repoview-manifest.json", "{}"), (bad, "x")]);
            let err = run_unzip(&unzip_request(&root, false), &mut |_| {}).expect_err(&format!("{bad:?} was accepted"));
            assert!(err.contains("unsafe entry"), "{bad:?}: {err}");
            assert!(leftovers(&root).is_empty(), "{bad:?} left {:?}", leftovers(&root));
            assert!(!root.join("evil.txt").exists() && !root.join("out/evil.txt").exists());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn a_zip_that_is_not_a_takeout_is_refused() {
        let root = temp_dir("unzip-nomanifest");
        make_zip(&root.join("t.zip"), &[("readme.txt", "hi"), ("other/file.json", "{}")]);
        let err = run_unzip(&unzip_request(&root, false), &mut |_| {}).unwrap_err();
        assert!(err.contains("no repoview-manifest.json"), "{err}");
        assert!(leftovers(&root).is_empty());

        // two folders is ambiguous, so it is refused too
        make_zip(&root.join("t.zip"), &[("a/repoview-manifest.json", "{}"), ("b/x.txt", "x")]);
        assert!(run_unzip(&unzip_request(&root, false), &mut |_| {}).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_existing_destination_is_kept_unless_told_to_replace_it() {
        let root = temp_dir("unzip-replace");
        make_zip(&root.join("t.zip"), &[("repoview-manifest.json", "{\"new\":true}")]);
        std::fs::create_dir_all(root.join("out/t")).unwrap();
        std::fs::write(root.join("out/t/mine.txt"), "keep me").unwrap();

        let err = run_unzip(&unzip_request(&root, false), &mut |_| {}).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert!(root.join("out/t/mine.txt").is_file(), "a refused unzip must not touch it");

        run_unzip(&unzip_request(&root, true), &mut |_| {}).unwrap();
        assert!(!root.join("out/t/mine.txt").exists());
        assert!(root.join("out/t/repoview-manifest.json").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_or_corrupt_zip_is_an_error_not_a_panic() {
        let root = temp_dir("unzip-corrupt");
        let err = run_unzip(&unzip_request(&root, false), &mut |_| {}).unwrap_err();
        assert!(err.contains("can't open"), "{err}");
        std::fs::write(root.join("t.zip"), "this is not a zip").unwrap();
        let err = run_unzip(&unzip_request(&root, false), &mut |_| {}).unwrap_err();
        assert!(err.contains("not a valid zip"), "{err}");
        assert!(leftovers(&root).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_takeout_that_already_exists_is_refused_unless_overwriting() {
        let root = fixture("collision");
        run_takeout(&request(&root, "tk", false), &mut |_| {}).unwrap();

        let err = run_takeout(&request(&root, "tk", false), &mut |_| {}).unwrap_err();
        assert!(err.contains("already exists"), "{err}");

        // overwrite replaces it, leaving nothing of the old one behind
        std::fs::write(root.join("storage/takeout/tk/stale.txt"), "old").unwrap();
        run_takeout(&request(&root, "tk", true), &mut |_| {}).unwrap();
        assert!(!root.join("storage/takeout/tk/stale.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
