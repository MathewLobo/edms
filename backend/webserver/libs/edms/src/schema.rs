use crate::core::EdmsCore;
use crate::error::EdmsResult;
use rusqlite::{Connection, Result};

pub fn initialize_schema_from_core(core: &EdmsCore) -> EdmsResult<()> {
    let conn_guard = core.base.connection.lock().unwrap();
    let conn = conn_guard.as_ref().unwrap();
    initialize_schema(conn).map_err(crate::error::EdmsError::SqliteError)?;
    Ok(())
}

pub fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS endpoints (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT UNIQUE NOT NULL,
            endpoint_str TEXT NOT NULL,
            annotation TEXT,
            method TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    // Migration: DBs created before `method` existed won't have picked it up
    // from CREATE TABLE IF NOT EXISTS above (that's a no-op on an existing
    // table), so add it explicitly if missing. Safe to run every startup.
    let has_method: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('endpoints') WHERE name = 'method'",
        [],
        |row| row.get(0),
    )?;
    if has_method == 0 {
        conn.execute("ALTER TABLE endpoints ADD COLUMN method TEXT", [])?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_endpoints_id ON endpoints(endpoint_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_endpoints_str ON endpoints(endpoint_str)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_endpoints_str_method ON endpoints(endpoint_str, method)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS request_metadata (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            request_number INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            method TEXT,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_request_endpoint ON request_metadata(endpoint_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_request_timestamp ON request_metadata(timestamp)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_request_method ON request_metadata(method)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS response_metadata (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            request_number INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            status_code INTEGER,
            exit_code INTEGER,
            response_time_ms INTEGER,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_response_endpoint ON response_metadata(endpoint_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_response_status ON response_metadata(status_code)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_response_timestamp ON response_metadata(timestamp)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS metadata (
            endpoint_id TEXT PRIMARY KEY,
            request_count INTEGER DEFAULT 0,
            response_count INTEGER DEFAULT 0,
            data_size_bytes INTEGER DEFAULT 0,
            last_tested TIMESTAMP,
            avg_response_time_ms INTEGER
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            tag TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(endpoint_id, tag)
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tags_endpoint ON tags(endpoint_id)",
        [],
    )?;

    conn.execute("CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag)", [])?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS bookmarks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            folder TEXT,
            notes TEXT,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(endpoint_id, folder)
        )",
        [],
    )?;

    let has_unique: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_index_list('bookmarks') WHERE origin = 'u'",
        [], |r| r.get(0)
    )?;
    
    if has_unique == 0 {
        // Dedup: prefer row with non-null notes, then most recent (MAX id)
        conn.execute("
            DELETE FROM bookmarks
            WHERE id NOT IN (
                SELECT CASE
                    WHEN MAX(CASE WHEN notes IS NOT NULL THEN id ELSE 0 END) > 0
                         THEN MAX(CASE WHEN notes IS NOT NULL THEN id ELSE NULL END)
                    ELSE MAX(id)
                END
                FROM bookmarks GROUP BY endpoint_id, folder
            )", []
        )?;

        // Recreate with constraint
        conn.execute("
            CREATE TABLE bookmarks_new (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                endpoint_id TEXT NOT NULL,
                folder      TEXT,
                notes       TEXT,
                timestamp   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(endpoint_id, folder)
            )", []
        )?;
        conn.execute("INSERT INTO bookmarks_new SELECT * FROM bookmarks", [])?;
        conn.execute("DROP TABLE bookmarks", [])?;
        conn.execute("ALTER TABLE bookmarks_new RENAME TO bookmarks", [])?;
    }

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_bookmarks_endpoint ON bookmarks(endpoint_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_bookmarks_folder ON bookmarks(folder)",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            endpoint_id TEXT NOT NULL,
            action TEXT NOT NULL,
            details TEXT,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_history_endpoint ON history(endpoint_id)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp)",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_history_action ON history(action)",
        [],
    )?;

    // Catalog tables — registers which collections/webviews/repoviews exist.
    // Per Ravi (2026-08-25): each one's actual endpoint data + its own local
    // tags/endpoint-segments tables live in an independent SQLite file;
    // file_path points to it. That per-view-instance file isn't created by
    // this pass yet — file_path is nullable until that infrastructure lands.
    for table in ["collections", "webview", "repoview"] {
        conn.execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {table} (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    file_path TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )"
            ),
            [],
        )?;
    }

    // Central tag-count rollups, one table per view type — a simple
    // incrementally-maintained counter (tagname, count), not a full entity
    // with a membership table. Global per view-type, not per collection
    // instance (matches the dashboard's existing aggregate-count model).
    for table in ["collections_tags", "webview_tags", "repoview_tags"] {
        conn.execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {table} (
                    tagname TEXT NOT NULL UNIQUE,
                    count INTEGER NOT NULL DEFAULT 0
                )"
            ),
            [],
        )?;
    }

    // Per-collection tag memberships (C(T)) for merge operations
    conn.execute(
        "CREATE TABLE IF NOT EXISTS collection_tag_memberships (
            collection_name TEXT NOT NULL,
            tagname TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (collection_name, tagname)
        )",
        [],
    )?;

    Ok(())
}
