//! Index Table generation for RepoView, per Ravi's Index Table wiki
//! (2026-09-05). Two approaches, one active at a time — both write the same
//! `Tables-NNN.md` + `Tables-meta.md` names, so generating with one
//! naturally replaces whatever the other produced; `Tables-meta.md` names
//! the approach on its first line so the folder is self-describing.
//!
//! - [B] Endpoint Segments — generic, no tags needed. One row per endpoint:
//!   its URL path (scheme/host stripped), EID, method; sorted by path.
//! - [A] Sorted Tags — a tag -> segment-count pivot: one line per tag,
//!   listing every distinct segment under it with how many endpoints have
//!   that (tag, segment) pair. An endpoint with several tags counts once
//!   under each. Untagged endpoints land in a synthetic `(untagged)` line,
//!   always last.
//!
//! Deliberate simplification vs. the wiki's literal sketch: the wiki hints
//! at splitting a single tag's count across two files to fill a batch to
//! exactly `batch_size` (`subset(N3)`, `k3` / `N3-k3`) — but its own two
//! write-ups don't use consistent notation for that. Here a tag's full
//! segment breakdown always stays together, and `batch_size` counts whole
//! lines (tag-lines for [A], endpoint-rows for [B]).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use edms::ops::view_ops::ViewKind;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::path::Path as FsPath;

use crate::{
    handlers::{
        repoview_index::snapshot_endpoints,
        view_catalog::{open_catalog, open_existing_membership, repoview_dir, validate_repoview_name},
    },
    state::AppState,
};

const DEFAULT_BATCH_SIZE: usize = 100;
const UNTAGGED: &str = "(untagged)";

#[derive(Debug, Deserialize)]
pub struct GenerateTablesRequest {
    #[serde(default)]
    pub batch_size: Option<usize>,
    /// `"endpoint_segments"` (default) or `"sorted_tags"`.
    #[serde(default)]
    pub approach: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Approach {
    EndpointSegments,
    SortedTags,
}

impl Approach {
    fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("endpoint_segments") => Ok(Self::EndpointSegments),
            Some("sorted_tags") => Ok(Self::SortedTags),
            Some(other) => Err(format!(
                "unknown approach '{other}' - expected 'endpoint_segments' or 'sorted_tags'"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::EndpointSegments => "Endpoint Segments",
            Self::SortedTags => "Sorted Tags",
        }
    }

    fn api_name(self) -> &'static str {
        match self {
            Self::EndpointSegments => "endpoint_segments",
            Self::SortedTags => "sorted_tags",
        }
    }

    fn table_header(self) -> &'static str {
        match self {
            Self::EndpointSegments => "| Segment Path | EID | Method |\n|---|---|---|\n",
            Self::SortedTags => "| Tag | Endpoint Segments |\n|---|---|\n",
        }
    }

    fn meta_header(self) -> &'static str {
        match self {
            Self::EndpointSegments => {
                "| File | First Segment | Last Segment | Count |\n|---|---|---|---|\n"
            }
            Self::SortedTags => "| File | First Tag | Last Tag | Tag Count |\n|---|---|---|---|\n",
        }
    }
}

struct MemberRow {
    segment_path: String,
    endpoint_id: String,
    method: String,
}

/// Strips scheme + host from a URL, keeping the path as-is - including
/// placeholder syntax like `:id`/`{id}` kept literal, per the wiki's own
/// examples. Falls back to the raw string if it doesn't look like a URL,
/// so a malformed `endpoint_str` still sorts somewhere instead of
/// vanishing from the output.
pub(crate) fn segment_path(endpoint_str: &str) -> String {
    // No "://" at all means this doesn't look like a URL - return it
    // verbatim rather than guessing, so it still sorts somewhere instead
    // of silently becoming "/".
    let Some((_, after_scheme)) = endpoint_str.split_once("://") else {
        return endpoint_str.to_string();
    };
    let path = match after_scheme.find('/') {
        Some(idx) => &after_scheme[idx..],
        None => "/",
    };
    // Query string / fragment aren't path segments.
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// The individual path components of an endpoint's URL - what the UI calls
/// its "segments" (`getEndpointSegments` in repoview.js is exactly
/// `split('/')` + drop empties). `https://x.com/users/:id/orders` gives
/// `["users", ":id", "orders"]`.
pub(crate) fn path_segments(endpoint_str: &str) -> Vec<String> {
    segment_path(endpoint_str)
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

/// Keeps a value from breaking out of its markdown table cell.
fn md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

/// Approach [A]'s core: tag -> (segment -> count of endpoints). `members`
/// is one `(segment_path, tags)` pair per endpoint. Tags come out sorted
/// alphabetically, segments sorted within each tag, and untagged endpoints
/// (if any) are collected into a final `(untagged)` line.
fn build_tag_lines(members: &[(String, Vec<String>)]) -> Vec<(String, Vec<(String, usize)>)> {
    let mut by_tag: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut untagged: BTreeMap<String, usize> = BTreeMap::new();

    for (segment, tags) in members {
        if tags.is_empty() {
            *untagged.entry(segment.clone()).or_insert(0) += 1;
            continue;
        }
        for tag in tags {
            *by_tag
                .entry(tag.clone())
                .or_default()
                .entry(segment.clone())
                .or_insert(0) += 1;
        }
    }

    let mut lines: Vec<(String, Vec<(String, usize)>)> = by_tag
        .into_iter()
        .map(|(tag, segments)| (tag, segments.into_iter().collect()))
        .collect();
    if !untagged.is_empty() {
        lines.push((UNTAGGED.to_string(), untagged.into_iter().collect()));
    }
    lines
}

/// One batch-able item: the label used for the meta table's first/last
/// columns, and the already-rendered markdown row.
struct Item {
    key: String,
    row: String,
}

/// Deletes every existing `Tables-*.md` in `dir`, then writes fresh
/// `Tables-NNN.md` batches of `batch_size` items plus a `Tables-meta.md`.
/// Returns the file names written. Replacing (not patching) is deliberate -
/// the batch count can shrink or grow as membership changes, so a stale
/// file from a larger previous run must never be left behind.
fn write_batches(
    dir: &FsPath,
    approach: Approach,
    items: &[Item],
    batch_size: usize,
) -> Result<Vec<String>, String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with("Tables-") && file_name.ends_with(".md") {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    let approach_line = format!("Approach: {}\n\n", approach.label());

    if items.is_empty() {
        std::fs::write(
            dir.join("Tables-meta.md"),
            format!("# Tables-meta\n\n{approach_line}No endpoints in this RepoView yet.\n"),
        )
        .map_err(|e| e.to_string())?;
        return Ok(vec!["Tables-meta.md".to_string()]);
    }

    let mut files_written: Vec<String> = Vec::new();
    let mut meta_rows = String::new();

    for (batch_index, chunk) in items.chunks(batch_size).enumerate() {
        let file_name = format!("Tables-{:03}.md", batch_index + 1);
        let mut content = String::from(approach.table_header());
        for item in chunk {
            content.push_str(&item.row);
            content.push('\n');
        }
        std::fs::write(dir.join(&file_name), content).map_err(|e| e.to_string())?;

        meta_rows.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            file_name,
            md_cell(&chunk.first().unwrap().key),
            md_cell(&chunk.last().unwrap().key),
            chunk.len()
        ));
        files_written.push(file_name);
    }

    std::fs::write(
        dir.join("Tables-meta.md"),
        format!(
            "# Tables-meta\n\n{approach_line}{}{meta_rows}",
            approach.meta_header()
        ),
    )
    .map_err(|e| e.to_string())?;
    files_written.push("Tables-meta.md".to_string());

    Ok(files_written)
}

/// POST /repoview/:name/tables/generate
///
/// Deliberately NOT run as part of create (a "zero time op" per the spec)
/// or on every membership change - generation is its own explicit, "time
/// consuming" action, same category as delete/duplicate. Replaces every
/// existing Tables-*.md in the folder on each call.
///
/// Both approaches read the RepoView's *own* index (`endpoint_snapshot` and
/// `endpoint_tags`, filled at create time), never the central tables, so the
/// files describe this RepoView as the self-contained list it is.
pub async fn generate_repoview_tables(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<GenerateTablesRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let approach = match Approach::parse(payload.approach.as_deref()) {
        Ok(a) => a,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
    };
    let batch_size = payload.batch_size.filter(|&n| n > 0).unwrap_or(DEFAULT_BATCH_SIZE);

    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<serde_json::Value, String> {
            let membership = open_existing_membership(&state, ViewKind::Repoview, &name)?;

            // Straight from the RepoView's own index - it describes itself,
            // so these files don't depend on the central tables at all.
            let mut members: Vec<MemberRow> = snapshot_endpoints(&membership)?
                .into_iter()
                .map(|(endpoint_id, endpoint_str, method)| MemberRow {
                    segment_path: segment_path(&endpoint_str),
                    endpoint_id,
                    method,
                })
                .collect();

            let dir = repoview_dir(&state, &name);
            if !dir.is_dir() {
                return Err(format!("RepoView '{name}' has no folder yet"));
            }

            let items: Vec<Item> = match approach {
                Approach::EndpointSegments => {
                    members.sort_by(|a, b| {
                        a.segment_path
                            .cmp(&b.segment_path)
                            .then(a.endpoint_id.cmp(&b.endpoint_id))
                    });
                    members
                        .iter()
                        .map(|m| Item {
                            key: m.segment_path.clone(),
                            row: format!(
                                "| {} | {} | {} |",
                                md_cell(&m.segment_path),
                                m.endpoint_id,
                                m.method
                            ),
                        })
                        .collect()
                }
                Approach::SortedTags => {
                    let mut tags_by_endpoint: HashMap<String, Vec<String>> = HashMap::new();
                    for (eid, tag) in membership.list_all_endpoint_tags().map_err(|e| format!("{e:?}"))? {
                        tags_by_endpoint.entry(eid).or_default().push(tag);
                    }
                    let pairs: Vec<(String, Vec<String>)> = members
                        .iter()
                        .map(|m| {
                            (
                                m.segment_path.clone(),
                                tags_by_endpoint.remove(&m.endpoint_id).unwrap_or_default(),
                            )
                        })
                        .collect();
                    build_tag_lines(&pairs)
                        .into_iter()
                        .map(|(tag, segments)| {
                            let cell = segments
                                .iter()
                                .map(|(segment, count)| format!("{}{{{}}}", md_cell(segment), count))
                                .collect::<Vec<_>>()
                                .join(", ");
                            Item {
                                key: tag.clone(),
                                row: format!("| {} | {} |", md_cell(&tag), cell),
                            }
                        })
                        .collect()
                }
            };

            let files_written = write_batches(&dir, approach, &items, batch_size)?;

            Ok(json!({
                "ok": true,
                "approach": approach.api_name(),
                "batch_size": batch_size,
                "total_endpoints": members.len(),
                "rows": items.len(),
                "files_written": files_written
            }))
        }
    })
    .await;

    match res {
        Ok(Ok(body)) => (StatusCode::OK, Json(body)),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

// ── Reading the generated files back ─────────────────────────────────────
//
// The spec's "Endpoint Data table: click on this to load in another tab
// (markdown rendered)" needs the browser to fetch what generate wrote - it
// can't open files on the server. These two routes are that read side. They
// only ever serve `Tables-meta.md` and `Tables-NNN.md` (exact-name check, so
// no path can be smuggled in), and return JSON like the other read routes
// (`{ok, body}` for saved request/response files) so the frontend's existing
// JSON helper works unchanged.

/// True only for `Tables-meta.md` or `Tables-<digits>.md` - the exact names
/// `generate` writes, and the only files the read routes will touch.
pub(crate) fn is_table_file_name(name: &str) -> bool {
    if name == "Tables-meta.md" {
        return true;
    }
    name.strip_prefix("Tables-")
        .and_then(|rest| rest.strip_suffix(".md"))
        .map(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or(false)
}

/// Meta first, then batches in numeric order (so `Tables-1000.md` sorts after
/// `Tables-999.md`, which a plain string sort would get wrong).
fn table_sort_key(name: &str) -> (u8, u64) {
    if name == "Tables-meta.md" {
        return (0, 0);
    }
    let number = name
        .strip_prefix("Tables-")
        .and_then(|rest| rest.strip_suffix(".md"))
        .and_then(|digits| digits.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    (1, number)
}

/// Reads the `Approach:` line `generate` writes at the top of
/// `Tables-meta.md` and maps it back to the API name.
fn parse_approach_line(meta: &str) -> Option<&'static str> {
    let label = meta.lines().find_map(|line| line.strip_prefix("Approach:"))?.trim();
    [Approach::EndpointSegments, Approach::SortedTags]
        .into_iter()
        .find(|a| a.label() == label)
        .map(|a| a.api_name())
}

fn is_registered(state: &AppState, name: &str) -> Result<bool, String> {
    open_catalog(state)?
        .get(ViewKind::Repoview, name)
        .map(|row| row.is_some())
        .map_err(|e| format!("{e:?}"))
}

/// GET /repoview/:name/tables - which generated files exist right now, and
/// which approach produced them. `generated: false` (with an empty list)
/// means generate hasn't been run yet, or the RepoView has no folder.
pub async fn list_repoview_tables(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_repoview_name(&name) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e })));
    }

    let res = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || -> Result<Option<serde_json::Value>, String> {
            if !is_registered(&state, &name)? {
                return Ok(None);
            }
            let dir = repoview_dir(&state, &name);

            let mut files: Vec<(String, u64)> = Vec::new();
            if dir.is_dir() {
                for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if is_table_file_name(&file_name) && entry.file_type().map_err(|e| e.to_string())?.is_file() {
                        files.push((file_name, entry.metadata().map_err(|e| e.to_string())?.len()));
                    }
                }
            }
            files.sort_by_key(|(file_name, _)| table_sort_key(file_name));

            let approach = std::fs::read_to_string(dir.join("Tables-meta.md"))
                .ok()
                .and_then(|meta| parse_approach_line(&meta));

            Ok(Some(json!({
                "ok": true,
                "generated": !files.is_empty(),
                "approach": approach,
                "files": files
                    .into_iter()
                    .map(|(file_name, size_bytes)| json!({ "name": file_name, "size_bytes": size_bytes }))
                    .collect::<Vec<_>>()
            })))
        }
    })
    .await;

    match res {
        Ok(Ok(Some(body))) => (StatusCode::OK, Json(body)),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": format!("RepoView '{name}' does not exist") })),
        ),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
    }
}

/// GET /repoview/:name/tables/:file - the markdown of one generated file,
/// as `{ok, file, content}`. `file` must be `Tables-meta.md` or
/// `Tables-NNN.md`; anything else is a 400, a missing/not-yet-generated file
/// is a 404.
pub async fn get_repoview_table_file(
    State(state): State<AppState>,
    Path((name, file)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_repoview_name(&name) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e })));
    }
    if !is_table_file_name(&file) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "file must be 'Tables-meta.md' or 'Tables-<number>.md'"
            })),
        );
    }

    let registered = tokio::task::spawn_blocking({
        let state = state.clone();
        let name = name.clone();
        move || is_registered(&state, &name)
    })
    .await;
    match registered {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "ok": false, "error": format!("RepoView '{name}' does not exist") })),
            )
        }
        Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e }))),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": e.to_string() })),
            )
        }
    }

    let path = repoview_dir(&state, &name).join(&file);
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => (StatusCode::OK, Json(json!({ "ok": true, "file": file, "content": content }))),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "error": format!(
                    "{file} hasn't been generated for RepoView '{name}' - call POST /repoview/{name}/tables/generate first"
                )
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn only_the_exact_generated_file_names_are_readable() {
        for ok in ["Tables-meta.md", "Tables-001.md", "Tables-1000.md", "Tables-7.md"] {
            assert!(is_table_file_name(ok), "{ok} should be allowed");
        }
        for bad in [
            "", "repoview.sqlite", "Tables-.md", "Tables-001.md.bak", "tables-001.md", "Tables-0a1.md",
            "../Tables-001.md", "Tables-001.md/../x", "Tables-meta.MD", "Tables-meta.md ", "sub/Tables-001.md",
            "..\\Tables-001.md", "Tables--1.md",
        ] {
            assert!(!is_table_file_name(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn files_sort_meta_first_then_numerically() {
        let mut names = vec!["Tables-1000.md", "Tables-002.md", "Tables-meta.md", "Tables-999.md", "Tables-001.md"];
        names.sort_by_key(|n| table_sort_key(n));
        assert_eq!(
            names,
            vec!["Tables-meta.md", "Tables-001.md", "Tables-002.md", "Tables-999.md", "Tables-1000.md"]
        );
    }

    #[test]
    fn approach_line_round_trips_through_the_meta_file() {
        let dir = temp_dir("approach");
        for approach in [Approach::EndpointSegments, Approach::SortedTags] {
            write_batches(&dir, approach, &[item("a")], 10).unwrap();
            let meta = std::fs::read_to_string(dir.join("Tables-meta.md")).unwrap();
            assert_eq!(parse_approach_line(&meta), Some(approach.api_name()));
        }
        assert_eq!(parse_approach_line("# nothing here"), None);
        assert_eq!(parse_approach_line("Approach: Something Else"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }


    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("edms-tables-{label}-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(key: &str) -> Item {
        Item { key: key.to_string(), row: format!("| {key} |") }
    }

    #[test]
    fn strips_scheme_and_host_keeping_placeholder_syntax() {
        assert_eq!(segment_path("https://api.example.com/users/:id/orders"), "/users/:id/orders");
        assert_eq!(segment_path("http://api.example.com/a/{b}/c"), "/a/{b}/c");
    }

    #[test]
    fn handles_no_path_query_and_fragment() {
        assert_eq!(segment_path("https://api.example.com"), "/");
        assert_eq!(segment_path("https://api.example.com/items?x=1"), "/items");
        assert_eq!(segment_path("https://api.example.com/items#frag"), "/items");
    }

    #[test]
    fn falls_back_to_the_raw_string_for_non_urls() {
        assert_eq!(segment_path("not-a-url"), "not-a-url");
    }

    #[test]
    fn approach_parses_known_names_and_rejects_the_rest() {
        assert_eq!(Approach::parse(None).unwrap(), Approach::EndpointSegments);
        assert_eq!(Approach::parse(Some("")).unwrap(), Approach::EndpointSegments);
        assert_eq!(Approach::parse(Some("Sorted_Tags")).unwrap(), Approach::SortedTags);
        assert!(Approach::parse(Some("bogus")).is_err());
    }

    #[test]
    fn md_cell_escapes_pipes_and_newlines() {
        assert_eq!(md_cell("a|b\nc"), "a\\|b c");
    }

    #[test]
    fn tag_lines_count_each_endpoint_once_per_tag_it_carries() {
        let members = vec![
            ("/users".to_string(), vec!["prod".to_string(), "api".to_string()]),
            ("/users".to_string(), vec!["prod".to_string()]),
            ("/orders".to_string(), vec!["api".to_string()]),
        ];
        let lines = build_tag_lines(&members);
        assert_eq!(
            lines,
            vec![
                ("api".to_string(), vec![("/orders".to_string(), 1), ("/users".to_string(), 1)]),
                ("prod".to_string(), vec![("/users".to_string(), 2)]),
            ]
        );
    }

    #[test]
    fn untagged_endpoints_get_their_own_line_sorted_last() {
        let members = vec![
            ("/a".to_string(), vec![]),
            ("/b".to_string(), vec!["zeta".to_string()]),
            ("/a".to_string(), vec![]),
        ];
        let lines = build_tag_lines(&members);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, "zeta");
        assert_eq!(lines[1], ("(untagged)".to_string(), vec![("/a".to_string(), 2)]));
    }

    #[test]
    fn batches_split_by_batch_size_and_meta_names_the_approach() {
        let dir = temp_dir("batches");
        let items: Vec<Item> = ["a", "b", "c", "d", "e"].iter().map(|k| item(k)).collect();

        let files = write_batches(&dir, Approach::SortedTags, &items, 2).unwrap();
        assert_eq!(
            files,
            vec!["Tables-001.md", "Tables-002.md", "Tables-003.md", "Tables-meta.md"]
        );

        let meta = std::fs::read_to_string(dir.join("Tables-meta.md")).unwrap();
        assert!(meta.contains("Approach: Sorted Tags"));
        assert!(meta.contains("| Tables-001.md | a | b | 2 |"));
        assert!(meta.contains("| Tables-003.md | e | e | 1 |"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn regenerating_removes_stale_batches_and_switching_approach_replaces_meta() {
        let dir = temp_dir("regen");
        let many: Vec<Item> = ["a", "b", "c", "d"].iter().map(|k| item(k)).collect();
        write_batches(&dir, Approach::EndpointSegments, &many, 1).unwrap();
        assert!(dir.join("Tables-004.md").exists());

        let few: Vec<Item> = vec![item("a")];
        write_batches(&dir, Approach::SortedTags, &few, 10).unwrap();
        assert!(!dir.join("Tables-002.md").exists());
        assert!(!dir.join("Tables-004.md").exists());
        let meta = std::fs::read_to_string(dir.join("Tables-meta.md")).unwrap();
        assert!(meta.contains("Approach: Sorted Tags"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_input_writes_only_a_meta_file() {
        let dir = temp_dir("empty");
        let files = write_batches(&dir, Approach::EndpointSegments, &[], 100).unwrap();
        assert_eq!(files, vec!["Tables-meta.md"]);
        let meta = std::fs::read_to_string(dir.join("Tables-meta.md")).unwrap();
        assert!(meta.contains("No endpoints in this RepoView yet."));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
