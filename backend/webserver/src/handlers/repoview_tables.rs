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
    db,
    handlers::view_catalog::{open_existing_membership, repoview_dir},
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
fn segment_path(endpoint_str: &str) -> String {
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
/// [A] reads tags from the RepoView's *own* copy (`endpoint_tags`, filled
/// at create / add time) rather than the central table, so the index
/// reflects this RepoView as a self-contained snapshot.
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
            let member_ids = membership.list_ids().map_err(|e| format!("{e:?}"))?;

            let mut members: Vec<MemberRow> = Vec::with_capacity(member_ids.len());
            for eid in &member_ids {
                if let Some(endpoint) = db::get_endpoint(&state.core, &state.queries, eid)
                    .map_err(|e| format!("{e:?}"))?
                {
                    members.push(MemberRow {
                        segment_path: segment_path(&endpoint.endpoint_str),
                        endpoint_id: endpoint.endpoint_id,
                        method: endpoint.method.unwrap_or_else(|| "UNCLASSIFIED".to_string()),
                    });
                }
                // An id with no central endpoint row left (deleted out
                // from under this RepoView's membership) is silently
                // skipped - best-effort, matches the stats endpoint.
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
