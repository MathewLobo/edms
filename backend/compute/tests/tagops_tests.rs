// compute/tests/tagops_tests.rs
//
// Integration tests for tagops.rs.
//
// Tests here use the public crate API (imported via `compute::tagops`), exactly
// as an external caller would. Where real DB work is needed we use
// `tempfile::NamedTempFile` to get an isolated on-disk SQLite file so that each
// test is fully independent and idempotent.

use compute::tagops::{
    ActivityEntry, BulkTagRequest, CollectionTagFilter, CreateFromTagsRequest,
    MergeRequest, RenameTagRequest,
    bulk_add_tags, bulk_remove_tags, create_from_tags, merge_by_tags, rename_tag,
};
use rusqlite::Connection;
use tempfile::NamedTempFile;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn tags(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn make_merge_req(db_path: &str, target: &str, sources: Vec<(&str, &[&str])>) -> MergeRequest {
    MergeRequest {
        db_path: db_path.to_string(),
        target_collection_name: target.to_string(),
        sources: sources
            .into_iter()
            .map(|(cid, ts)| CollectionTagFilter {
                collection_id: cid.to_string(),
                tags: tags(ts),
            })
            .collect(),
    }
}

/// Create a temp SQLite file, open it, initialise the two tables we need, and
/// return both the file handle (keeps the file alive) and the connection.
fn setup_temp_db() -> (NamedTempFile, Connection) {
    let file = NamedTempFile::new().expect("temp file");
    let conn = Connection::open(file.path()).expect("open temp db");
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS tags (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            tag         TEXT NOT NULL,
            created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(endpoint_id, tag)
        );
        CREATE TABLE IF NOT EXISTS bookmarks (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            folder      TEXT,
            notes       TEXT,
            timestamp   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
    ").expect("schema init");
    (file, conn)
}

/// Helper: count bookmarks in a given folder via an existing connection.
fn count_bookmarks(conn: &Connection, folder: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM bookmarks WHERE folder = ?1",
        rusqlite::params![folder],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// Helper: list tags on an endpoint.
fn list_tags(conn: &Connection, eid: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT tag FROM tags WHERE endpoint_id = ?1 ORDER BY tag")
        .unwrap();
    stmt.query_map(rusqlite::params![eid], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// merge_by_tags — Activity Log contract (pure log-shape, no DB)
// ─────────────────────────────────────────────────────────────────────────────

// Obsolete merge tests removed (covered by tagops_merge_tests.rs)

// ─────────────────────────────────────────────────────────────────────────────
// create_from_tags — Activity Log contract
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_create_from_tags_produces_two_log_entries() {
    let (file, _conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let mut log = Vec::new();
    let req = CreateFromTagsRequest {
        db_path,
        new_collection_name: "NewCol".to_string(),
        source_collection_id: "SourceCol".to_string(),
        tags: tags(&["admin", "user"]),
    };
    create_from_tags(req, &mut log).unwrap();

    assert_eq!(log.len(), 2);
}

#[test]
fn test_create_from_tags_log_names_collection() {
    let (file, _conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let mut log = Vec::new();
    let req = CreateFromTagsRequest {
        db_path,
        new_collection_name: "Snapshot".to_string(),
        source_collection_id: "raw".to_string(),
        tags: tags(&["public"]),
    };
    create_from_tags(req, &mut log).unwrap();

    assert!(log[0].message.contains("Snapshot"));
    assert!(log[1].message.contains("Snapshot"));
}

#[test]
fn test_create_from_tags_log_reports_tag_count() {
    let (file, _conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let mut log = Vec::new();
    let req = CreateFromTagsRequest {
        db_path,
        new_collection_name: "X".to_string(),
        source_collection_id: "src".to_string(),
        tags: tags(&["a", "b", "c"]),
    };
    create_from_tags(req, &mut log).unwrap();

    assert!(log[0].message.contains("3 tag(s)"));
}

// ─────────────────────────────────────────────────────────────────────────────
// create_from_tags — real DB integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_create_from_tags_populates_collection() {
    let (file, conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    conn.execute_batch("
        INSERT INTO bookmarks (endpoint_id, folder) VALUES ('e1', 'src');
        INSERT INTO bookmarks (endpoint_id, folder) VALUES ('e2', 'src');
        INSERT INTO tags (endpoint_id, tag) VALUES ('e1', 'public');
        INSERT INTO tags (endpoint_id, tag) VALUES ('e2', 'private');
    ").unwrap();

    let mut log = Vec::new();
    let req = CreateFromTagsRequest {
        db_path,
        new_collection_name: "PublicOnly".to_string(),
        source_collection_id: "src".to_string(),
        tags: tags(&["public"]),
    };
    create_from_tags(req, &mut log).unwrap();

    assert_eq!(count_bookmarks(&conn, "PublicOnly"), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// bulk_add_tags / bulk_remove_tags — real DB integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_bulk_add_tags_inserts_all_combinations() {
    let (file, conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let req = BulkTagRequest {
        db_path,
        endpoint_ids: vec!["e1".to_string(), "e2".to_string()],
        tags: tags(&["alpha", "beta"]),
    };
    let mut log = Vec::new();
    bulk_add_tags(req, &mut log).unwrap();

    assert_eq!(list_tags(&conn, "e1"), vec!["alpha", "beta"]);
    assert_eq!(list_tags(&conn, "e2"), vec!["alpha", "beta"]);
}

#[test]
fn test_bulk_add_tags_is_idempotent() {
    // Adding the same tag twice must not produce duplicates (UNIQUE constraint).
    let (file, conn) = setup_temp_db();
    let _db_path = file.path().to_str().unwrap().to_string();

    for _ in 0..2 {
        let req = BulkTagRequest {
            db_path: file.path().to_str().unwrap().to_string(),
            endpoint_ids: vec!["e1".to_string()],
            tags: tags(&["dup"]),
        };
        let mut log = Vec::new();
        bulk_add_tags(req, &mut log).unwrap();
    }

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tags WHERE endpoint_id = 'e1' AND tag = 'dup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_bulk_add_then_remove_roundtrip() {
    let (file, conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    // Add alpha and beta to e1.
    let add_req = BulkTagRequest {
        db_path: db_path.clone(),
        endpoint_ids: vec!["e1".to_string()],
        tags: tags(&["alpha", "beta"]),
    };
    bulk_add_tags(add_req, &mut Vec::new()).unwrap();
    assert_eq!(list_tags(&conn, "e1").len(), 2);

    // Remove alpha only.
    let rm_req = BulkTagRequest {
        db_path: db_path.clone(),
        endpoint_ids: vec!["e1".to_string()],
        tags: tags(&["alpha"]),
    };
    bulk_remove_tags(rm_req, &mut Vec::new()).unwrap();
    assert_eq!(list_tags(&conn, "e1"), vec!["beta"]);
}

#[test]
fn test_bulk_remove_nonexistent_tag_is_noop() {
    // Removing a tag that was never added must not error.
    let (file, _conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let req = BulkTagRequest {
        db_path,
        endpoint_ids: vec!["e1".to_string()],
        tags: tags(&["ghost"]),
    };
    let mut log = Vec::new();
    bulk_remove_tags(req, &mut log).unwrap();
    assert!(log.iter().any(|e| e.message.contains("complete")));
}

// ─────────────────────────────────────────────────────────────────────────────
// rename_tag — real DB integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rename_tag_updates_all_rows() {
    let (file, conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    conn.execute_batch("
        INSERT INTO tags (endpoint_id, tag) VALUES ('e1', 'old');
        INSERT INTO tags (endpoint_id, tag) VALUES ('e2', 'old');
        INSERT INTO tags (endpoint_id, tag) VALUES ('e3', 'other');
    ").unwrap();

    let req = RenameTagRequest {
        db_path,
        old_name: "old".to_string(),
        new_name: "renamed".to_string(),
    };
    let mut log = Vec::new();
    rename_tag(req, &mut log).unwrap();

    // Both rows with 'old' become 'renamed'; 'other' is untouched.
    assert_eq!(list_tags(&conn, "e1"), vec!["renamed"]);
    assert_eq!(list_tags(&conn, "e2"), vec!["renamed"]);
    assert_eq!(list_tags(&conn, "e3"), vec!["other"]);
}

#[test]
fn test_rename_tag_log_mentions_row_count() {
    let (file, conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    conn.execute_batch("
        INSERT INTO tags (endpoint_id, tag) VALUES ('e1', 'foo');
        INSERT INTO tags (endpoint_id, tag) VALUES ('e2', 'foo');
    ").unwrap();

    let req = RenameTagRequest {
        db_path,
        old_name: "foo".to_string(),
        new_name: "bar".to_string(),
    };
    let mut log = Vec::new();
    rename_tag(req, &mut log).unwrap();

    // Second log entry should mention how many rows were affected.
    assert!(log.last().unwrap().message.contains("2 row(s)"));
    drop(conn);
}

#[test]
fn test_rename_nonexistent_tag_is_noop() {
    let (file, _conn) = setup_temp_db();
    let db_path = file.path().to_str().unwrap().to_string();

    let req = RenameTagRequest {
        db_path,
        old_name: "ghost".to_string(),
        new_name: "new".to_string(),
    };
    let mut log = Vec::new();
    rename_tag(req, &mut log).unwrap();
    // 0 rows affected — no error.
    assert!(log.last().unwrap().message.contains("0 row(s)"));
}

// Removed obsolete test_merge_then_create_independent_logs
