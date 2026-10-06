# EDMS Backend API Reference — Test View, Collections, Dashboard

Base URL: `http://localhost:3000` (same whether running via `cargo run` or Docker).

---

## Running it

### Option A — plain Docker
```bash
cd backend
docker build -f webserver/Dockerfile -t edms-local .
docker run -d --name edms-local -p 3000:3000 edms-local
```
No data persistence — every `docker run` starts fresh. Fine for quick checks.

### Option B — Docker Compose (recommended, persists data across restarts)
```bash
cd backend
docker compose up --build -d
```
Stop it with `docker compose down`. `edms.db` lands at `backend/webserver/data/edms.db` on your host — you can open it directly with any SQLite tool (Docker auto-creates that folder, no manual setup needed). `edms_data`/`edms_root` persist via named Docker volumes (not directly browsable as host folders — use `docker cp` to pull files out if you need to look at them).

To wipe everything and start fresh:
```bash
docker compose down -v
rm -rf webserver/data
```

> **Two things worth knowing if you're on an older checkout:**
> 1. `edms.db` used to be bind-mounted as a single file (`./webserver/edms.db:/app/edms.db`) rather than a directory. On Docker Desktop for Windows, single-file bind mounts don't reliably sync writes back to the host — the container ran fine, but the host-side copy silently stayed empty forever. Mounting a directory instead fixed it.
> 2. Separately, any endpoint that opens its own SQLite connection per request (not the app's shared one — e.g. collections, tags) could fail with `CannotOpen` under Docker specifically, even after fix #1. Docker Desktop for Windows bind mounts don't reliably support the shared-memory locking WAL mode needs for a second connection to the same file. Fixed by switching `journal_mode` to `DELETE` in `base.rs`.

### Option C — native (Rust toolchain required)
```bash
cd backend/webserver
cargo run --bin rust-webserver
```

### Inspecting the data directly (no SQLite CLI needed)
```bash
python -c "import sqlite3; c = sqlite3.connect('webserver/data/edms.db'); print(c.execute('SELECT * FROM endpoints').fetchall())"
```
Swap the table name (`endpoints`, `tags`, `bookmarks`, `history`, `collections`) or the db path as needed. For a collection's own file, pull it out of the volume first: `docker cp backend-webserver-1:/app/edms_root/storage/collections/<name>.sqlite .` then point the same command at it (table name is `membership` there).

---

## The one rule that matters for WebSocket routes

**WebSocket only notifies — REST delivers the actual data.** Every WS connection below streams messages shaped like:
```json
{ "type": "event", "event": { "type": "<EventName>", "payload": { ... } } }
```
Treat these as "something changed, go re-fetch," not as the final source of truth for anything except live progress (test timers).

---

## Views

Static, no-param metadata routes — mostly a place for the frontend to sanity-check which view it's on.

| Method | Path | Type | Notes |
|---|---|---|---|
| GET | `/home` | REST | `{"view":"home", ...}` |
| GET | `/test-view` | REST | `{"view":"test-view", ...}` |
| GET | `/list-view` | REST | `{"view":"list-view", ...}` |

---

## Endpoints

The only way endpoint definitions currently enter the system — Import (below) extracts files to disk but does not create DB rows yet.

| Method | Path | Body | Notes |
|---|---|---|---|
| POST | `/endpoints/create` | `{"endpoint_id","endpoint_str","annotation"?,"method"?}` | `method` optional — one of `GET/POST/PUT/PATCH/DELETE` if given; an endpoint created without one shows as unclassified in the CRUD Operations dashboard breakdown. Rejects a duplicate `endpoint_id` cleanly (400, not a 500) |
| POST | `/endpoints/:endpoint_id/delete` | — | Does **not** cascade — orphaned bookmarks, collection memberships, and history/request/response data can be left behind (see Known limitations) |
| POST | `/endpoints/:endpoint_id/annotation` | `{"annotation"}` | Sets/replaces the endpoint's annotation after creation (previously create-time-only). 404 if the endpoint doesn't exist. Broadcasts `EndpointAnnotationUpdated` on the shared WS channel (same one `/test-view/run` uses) so open List/Test Views know to re-fetch |
| GET | `/endpoints/lookup?endpoint_str=&method=` | — | Looks up an endpoint by its exact `(endpoint_str, method)` pair — lets a caller check "does this already exist" before deciding whether to pass an existing `endpoint_id` vs. let a fresh one get allocated. 404 if none exists |

## EQP report export

| Method | Path | Body | Notes |
|---|---|---|---|
| POST | `/reports/:endpoint_id/export` | `{"format":"pdf|html|md","output_filename"?}` | Reads the EID's request, response, and header JSON files from the configured `edms-data/storage/globalEQPData/` folder, combines them with endpoint metadata and tags, and sends the resulting payload to the compute converter. Missing, unreadable, or invalid QP JSON files become null in the affected field; available fields and other QPs still export. Returns `202 Accepted`; the timestamped file is written under `edms-data/takeout/PDF`, `HTML`, or `MD`. |

---

## Test View

| Method | Path | Type | Body / Notes |
|---|---|---|---|
| GET | `/test-view/endpoints/load` | WS | Sends a snapshot of all endpoints on connect, then streams events |
| GET | `/test-view/:collection/bookmarks/load` | WS | Same, for a specific collection's bookmark set — see the multi-collection redesign note below |
| GET | `/test-view/history/load` | WS | Same, for history — sends `{"type":"snapshot","history":[{"id","endpoint_id","action","details","timestamp"}]}` on connect, then streams events |
| GET | `/test-view/run` | WS | Send `{"type":"run_test","payload":{"endpoint_id"?,"endpoint_str","method","body","timeout_ms","tick_interval_ms","headers"?,"annotation"?}}` to start a test. **`endpoint_id` is optional as of 2026-09-08** — an endpoint is only created the moment it's tested: omit it to auto-allocate a fresh canonical EID, pass an existing one to re-test it (the common case), or a not-yet-existing one to create it with that exact id. `endpoint_str` is always required (used to create the row if it doesn't exist yet; ignored — the stored value wins — if it does). `annotation` is used only when this run creates a new endpoint. Streams `TestStarted` → `TimerTick`s → `TestFinished`/`TestTimeout` |
| POST | `/test-view/stop` | REST | Body `{"endpoint_id","request_number"}` — cancels the app's tracking of an in-flight test (does not kill the underlying HTTP call already running) |
| POST | `/test-view/save/history` | REST | Body `{"endpoint_id","action","details"}` — manual history entry (History also now auto-records on every completed test, no manual call needed for that case) |
| POST | `/test-view/save/bookmark` | REST | Body `{"collection","endpoint_id","notes"}` — bookmarks into the named collection. `collection` is required (400 if missing) — as of 2026-09-22 there's no more implicit "loaded" collection to fall back to |
| GET | `/test-view/:collection/add` | WS | Send `{"endpoint_id"}` — bookmarks into the named collection. `:collection` is always a real collection name now, no more `active` alias |
| GET | `/test-view/:collection/delete` | WS | Send `{"endpoint_id"}` — removes the bookmark entirely (does not touch collection membership) |
| GET | `/test-view/:endpoint_id/request/:request_number` | REST | Fetch a saved request body |
| GET | `/test-view/:endpoint_id/response/:request_number` | REST | Fetch a saved response body |
| GET | `/test-view/:endpoint_id/headers/:request_number` | REST | Fetch saved headers — body is `{"request_headers":{...},"response_headers":{...}}` |
| GET | `/test-view/:endpoint_id/qps` | REST | Lists every QP pair (test run) saved for this endpoint, oldest first: `{"ok":true,"qps":[{"request_number","method","timestamp","status_code","response_time_ms"}]}`. `status_code`/`response_time_ms` are `null` if the response hasn't landed yet |
| POST | `/test-view/:endpoint_id/qps/create` | REST | Creates a QP pair by hand — no real HTTP test runs. Body: `{"method","request_body","response_body"}` (the two bodies are JSON-encoded strings, same shape a saved file already round-trips as). Writes the request/response files directly, plus an empty headers file (`{"request_headers":{},"response_headers":{}}`) so `GET .../headers/:n` doesn't 404 for it. `status_code`/`response_time_ms` are left `null` — there's no real response. 404 if the endpoint doesn't exist. Response: `{"ok":true,"request_number"}`. Broadcasts `QpCreated` on the shared WS channel |
| POST | `/test-view/:endpoint_id/qps/:request_number/update` | REST | Overwrites an existing QP's request/response bodies in place. Body: `{"request_body","response_body"}`. Leaves `status_code`/`response_time_ms` and the headers file untouched. 404 if that QP doesn't exist. Broadcasts `QpUpdated` on the shared WS channel |
| POST | `/test-view/:endpoint_id/qps/:request_number/delete` | REST | Deletes one QP pair — its `request_metadata`/`response_metadata` rows and the three saved JSON files (request/response/headers). 404 if it doesn't exist. Broadcasts `QpDeleted` on the shared WS channel (same one `/test-view/run` uses) so open views know to re-fetch the list above |
| POST | `/test-view/history/clearall` | REST | Wipes all history |
| POST | `/test-view/:collection/bookmark/clearall` | REST | Wipes that collection's bookmark set only, not every collection's |

**Bookmarks ↔ Collections — multiple collections at once (redesigned 2026-09-22):**

Previously, "active bookmarks" was the draft state of whichever single Collection was loaded into one shared server-side value (`active_collection`) — only one collection could ever be loaded anywhere, for every connected client at once, so opening a second collection in another tab silently evicted the first. Now every bookmark route takes the collection explicitly (path param or body field), and the central `bookmarks` table's `folder` column holds the real collection name directly instead of one shared `__active__` bucket. Two tabs can have two different collections open with no interference, and two tabs on the *same* collection both see the same live updates via `BookmarksUpdated`/`CollectionMembershipUpdated`, which now carry a `collection` field to filter by. There's no more "load a collection first" gate, and no more session-backup mechanism — nothing is shared, so nothing needs backing up when switching.

| Method | Path | Type | Notes |
|---|---|---|---|
| GET | `/bookmarks/:collection/load` | WS | Snapshot of that collection's bookmark count, then streams events. Pure read now — no wipe, no copy, no backup |
| POST | `/bookmarks/:collection/:endpoint_id/save` | REST | Persists a bookmarked endpoint's membership (EID + timestamp only) into the named collection. 400 if `:endpoint_id` isn't currently bookmarked there. Broadcasts `CollectionMembershipUpdated { collection }` — previously this emitted nothing at all, so no other tab could ever know a save/unsave happened |
| POST | `/bookmarks/:collection/:endpoint_id/unsave` | REST | Drops that endpoint's membership from the named collection — **stays bookmarked** afterward, only the collection membership is removed. Same new broadcast as save |

`GET /test-view/:collection/bookmarks/load`'s snapshot carries `"collection": <name>` and, per bookmark entry, `"in_collection": true/false` (cross-referenced against that collection's own membership set) and `"updated": <timestamp>`.

**Cascades that come with storing bookmarks centrally by folder name, not a per-collection file:** deleting a collection also deletes its bookmarks (`DELETE FROM bookmarks WHERE folder = ?`); renaming a collection also renames its bookmarks' folder value, so they stay attached; deleting an endpoint also deletes all of its bookmarks, across every collection it was bookmarked into. None of these existed before this redesign — a deleted/renamed collection, or a deleted endpoint, used to leave orphaned rows behind.

---

## Collections (per-collection files)

Each collection is its own real file (`storage/collections/{name}.sqlite`), holding just `endpoint_id` + `added_at`. The endpoint's actual data always stays in the central `endpoints` table — Collections never copies it. **A collection is always created empty** — the only way an endpoint becomes a member is the bookmark flow above (`/bookmarks/:collection/:endpoint_id/save`); there is no longer a direct "add any endpoint to any collection" route.

| Method | Path | Body | Notes |
|---|---|---|---|
| POST | `/collections/create` | `{"name","annotation"?}` | Creates the catalog row + the real file, empty. `annotation` is optional |
| GET | `/collections/list` | — | All collections, each with `annotation` (`null` if unset) and `endpoint_count` (`null` if the collection has no file yet) |
| GET | `/collections/:name` | — | One collection's catalog row, including `annotation` and `endpoint_count`; 404 if missing |
| POST | `/collections/:name/rename` | `{"new_name"}` | Renames the catalog entry + moves the file; rejects a name collision cleanly, no data loss |
| POST | `/collections/:name/annotation` | `{"annotation"}` | Sets/replaces the collection's annotation. 404 if the collection doesn't exist |
| POST | `/collections/:name/delete` | — | Removes the catalog row + deletes the file |
| POST | `/collections/:name/endpoints/remove` | `{"endpoint_id"}` | Direct removal stays available — removal doesn't carry the same "must be deliberately curated via testing" risk as addition |
| GET | `/collections/:name/endpoints` | — | Lists members with `added_at` |
| POST | `/collections/:name/tags/import` | `{"tags":[...],"export_existing_tags"?}` | The "Add to Collection" move flow — finds every endpoint carrying any of the given tags (read-only against the central tags table), adds them as members of this collection, and — if `export_existing_tags` is true — copies each one's current tags into this collection's own per-endpoint tag record (capped at 25 tags/endpoint; excess is silently skipped and counted in `tags_skipped_cap`). Central tags table is never modified. Returns `{"endpoints_matched","endpoints_added","tags_exported","tags_skipped_cap"}` |
| GET | `/collections/:name/tags/endpoints` | — | Lists every `(endpoint_id, tag)` pair this collection carries from the import route above — distinct from the central tags table and from the collection-wide tag rollups below |

**Tag rollups** (global count per tag, not per-collection membership):

| Method | Path | Body |
|---|---|---|
| POST | `/collections/tags/create` | `{"name","endpoint_ids"}` |
| POST | `/collections/tags/delete` | `{"names"}` |
| POST | `/collections/tags/rename` | `{"old_name","new_name"}` |
| GET | `/collections/tags/list` | — |

**Membership-tags** (a different concept — tracks which tags a collection has, used for merge classification, not endpoint membership):

| Method | Path | Body |
|---|---|---|
| POST | `/collections/:name/membership-tags/add` | `{"tag"}` |
| POST | `/collections/:name/membership-tags/remove` | `{"tag"}` |
| GET | `/collections/:name/membership-tags` | — |
| GET | `/collections/by-tag/:tagname` | — |

---

## Data View (folder management)

Manages "folders" under `edms_data`/`edms_root` — a separate, older filesystem-folder concept from Collections above. All fire-and-forget except `active`.

| Method | Path | Type | Notes |
|---|---|---|---|
| POST | `/dataview/:folder/delete` | REST | Deletes the folder from disk immediately (not fire-and-forget — this one's synchronous) |
| POST | `/dataview/:folder/merge` | REST | 202 immediately; spawns an `export_merge` child task, result arrives via `/internal/callback` → `ExportReady` event |
| GET | `/dataview/:folder/active` | WS | Marks the folder active (in-memory + spawns a child to sync it to disk), broadcasts `FolderBecameActive`, then streams events |

---

## Tags (per-endpoint)

A different table from Collections' tag rollups above — tracks tags directly on an `endpoint_id`.

| Method | Path | Body |
|---|---|---|
| GET | `/tags/popular` | — returns `[{"tag","count"}]` |
| GET | `/tags/:endpoint_id` | — returns `{"tags":[...]}` |
| POST | `/tags/:endpoint_id/add` | `{"tag"}` |
| POST | `/tags/:endpoint_id/remove` | `{"tag"}` |

---

## Webview

Same catalog pattern as Collections (register a name, list, per-view tag rollups) but **not** as far along — no independent SQLite file per instance yet (`file_path` stays `null`), and no endpoint-membership routes (no `webview/:name/endpoints/add` equivalent exists).

| Method | Path | Body |
|---|---|---|
| POST | `/webview/create` | `{"name"}` |
| GET | `/webview/list` | — |
| POST | `/webview/tags/create` | `{"name","endpoint_ids"?}` |
| POST | `/webview/tags/delete` | `{"names"}` |
| POST | `/webview/tags/rename` | `{"old_name","new_name"}` |
| GET | `/webview/tags/list` | — |

---

## Repoview

**v1.0 (Ravi, 2026-10-05/06): a RepoView is a list, not a copy of the data.** Its folder (`storage/repoviews/:name/`) holds only a SQLite index (`repoview.sqlite`) and the generated `Tables-meta.md` / `Tables-NNN.md`. The index lists the members and snapshots, for each one, its endpoint row (URL, method, annotation), its tags, its QP metadata and the size of its EQP data - everything the list view shows, and everything Import/Export will need to know what to copy. **No EQP data is stored in a RepoView**; it is copied out of `globalEQPData` only at Import/Export (takeout) time, which also keeps `create` instant. Because the index describes itself, a RepoView's numbers and Tables files don't change when endpoints are later deleted from the central tables.

The UI shows statistics per row (endpoint count, QP count, method counts, tags in the data, endpoint segments) but never lists the EQP data itself, so there are **no routes to browse, add or remove a RepoView's endpoints, and no endpoint-level tag operations** - those only exist in Collections (Bookmark / Test View).

| Method | Path | Body | Notes |
|---|---|---|---|
| POST | `/repoview/create` | `{"name","annotation"?,"source_collection","endpoint_ids"?}` | Builds the list from the chosen members of `source_collection` (all of them if `endpoint_ids` is omitted/empty): members, each one's current central tags, a snapshot of its endpoint row and QP metadata, and the size of its EQP data (measured, **not copied**). Returns `endpoints_added`, `tags_copied`, `qps_snapshotted`, `data_size_bytes`. **Name rules** (also enforced on `rename`, `duplicate`'s `new_name`, and the Collection names `convert-to-collection` creates): the name becomes a file/folder name, so it can't be empty, `.`/`..`, longer than 100 characters, start/end with a space, end with a dot, or contain `/ \ : * ? " < > \|` or control characters. 400 if the name exists, the source collection doesn't, or a requested id isn't a member of it. **Changed from the first RepoView release:** `source_collection` is required, and `endpoints_with_data_copied` is gone (nothing is copied) |
| GET | `/repoview/list` | — | Every RepoView with **all the columns the UI table reads, in one call**: `id`, `name`, `annotation`, `source`, `tags` (the row's own tags), `dataTags`, `crud` (count per HTTP method), `segments` (distinct URL path components across its endpoints) and `segmentCount`, `eidCount`, `qpCount`, `indexLists` (number of `Tables-NNN.md` files), `dateCreated`, `dataSizeBytes` - plus the earlier `file_path` and `created_at`. A RepoView whose index can't be read comes back with zeros and an `error` string instead of failing the whole list. The UI's field names are camelCase; the rest of the API is snake_case |
| GET | `/repoview/:name` | — | One RepoView, same data in snake_case: `eid_count`, `qp_count`, `data_size_bytes`, `tags_in_data`, `crud_types`, `segments`, `segment_count`, `index_lists`, `source`, `tags`, `annotation`, `created_at`. Computed from the RepoView's own index, never the central tables. 404 if it doesn't exist |
| POST | `/repoview/:name/rename` | `{"new_name"}` | Renames the catalog entry and moves the whole RepoView folder to match |
| POST | `/repoview/:name/annotation` | `{"annotation"}` | |
| POST | `/repoview/:name/duplicate` | `{"new_name"}` | Clones the whole folder (index, generated Tables files) under a new name, plus the row's annotation, source and tags. 400 if `new_name` exists or the source doesn't |
| POST | `/repoview/:name/delete` | — | Removes the catalog row and the whole folder |
| POST | `/repoview/delete` | `{"names":[...]}` | Multi-select delete - each name independently, so one bad name doesn't block the rest. Always `200`; check each entry's own `"ok"` in `results`; top-level `"ok"` is `true` only if every name succeeded |
| POST | `/repoview/:name/convert-to-collection` | `{"collection","on_exists"?,"new_name"?}` | The RMB "convert to Collection" pop-up. If `collection` doesn't exist it is created. If it **does** exist and no `on_exists` is given: **409** `{conflict:true, options:["merge","rename"]}` and nothing changes - this is the warning. Retry with `on_exists:"merge"` to add to the existing Collection, or `on_exists:"rename"` plus `new_name` to create a new one (a `new_name` that also exists is another 409). Only endpoints that **still exist in the central tables, and are still the same URL+method**, are added; the rest come back in `skipped` with a reason (top-level `ok` is then `false`). 400 if none of them exist (no empty Collection is created) |
| POST | `/repoview/:name/membership-tags/add` | `{"tag"}` | Tags on the RepoView itself (the list-level tag ops) - separate from `dataTags`, the tags of the endpoints inside it |
| POST | `/repoview/:name/membership-tags/remove` | `{"tag"}` | |
| GET | `/repoview/:name/membership-tags` | — | |
| GET | `/repoview/by-tag/:tagname` | — | Returns `{"repoviews":[...]}` |
| POST | `/repoview/tags/create` | `{"name","endpoint_ids"?}` | Central tag-count rollup, same as Collections/Webview |
| POST | `/repoview/tags/delete` | `{"names"}` | |
| POST | `/repoview/tags/rename` | `{"old_name","new_name"}` | |
| GET | `/repoview/tags/list` | — | |
| POST | `/repoview/:name/tables/generate` | `{"batch_size"?,"approach"?}` | Generates the Index Table files (Ravi's Index Table wiki) from the RepoView's own index. `approach` is `"endpoint_segments"` (default; one row per endpoint, sorted by URL path) or `"sorted_tags"` (tag to segment-count pivot, untagged last); only one is active at a time since both write the same `Tables-NNN.md` / `Tables-meta.md`, and `Tables-meta.md`'s first line names which. `batch_size` (default 100) counts whole rows; a tag's breakdown is never split across files. Replaces every existing `Tables-*.md` on each call and isn't run automatically. A JSON body is always required, even `{}`. 400 on an unknown `approach` |
| GET | `/repoview/:name/tables` | — | Which generated files exist and which approach produced them: `{generated, approach, files:[{name,size_bytes}]}` |
| GET | `/repoview/:name/tables/:file` | — | One generated file's markdown as `{ok, file, content}` - what the UI's "Endpoint Data table" loads. Only `Tables-meta.md` and `Tables-<number>.md` are ever served (400 otherwise); 404 if not generated |

**Removed in v1.0** (they shipped briefly in the first RepoView release; nothing in the frontend called them): `GET /repoview/:name/endpoints`, `POST .../endpoints/add`, `POST .../endpoints/remove`, `POST .../tags/import`, `GET .../tags/endpoints`, and `POST .../export-to-collection` (replaced by `convert-to-collection`).

**Not built yet:** Import/Export for RepoViews (the EQP data copy out of `globalEQPData` with the index stripped for git/web, and import with fresh EIDs), combining RepoViews (the list-level tag op), moving RepoView operations into Compute with live-update events, and the same treatment for WebView.

---

## Repo Export / Import

| Method | Path | Notes |
|---|---|---|
| GET | `/repo/:collection/:filename/export` | Returns markdown immediately (built from the endpoints already fetched for the collection); also fires `export_collection` (zip packaging) and `generate_markdown` child tasks in the background — result arrives via `ExportReady` |
| POST | `/repo/:collection/:filename/import` | 202 immediately; unzips the file at the path `export` writes to (`edms_root/exports/:filename`) back into the path `export` reads its source from (`edms_root/endpoints/reports/:collection`). Result arrives via `/internal/callback` → `ImportReady`. **Only extracts files to disk — does not parse them back into the DB** (no endpoint/bookmark rows are created from an import; that reconciliation isn't built yet) |

---

## Logs

| Method | Path | Notes |
|---|---|---|
| GET | `/logs` | Plain-text tail of `app.log` (last 500 lines) |

---

## Internal — not for frontend use

| Method | Path | Notes |
|---|---|---|
| POST | `/internal/callback` | edms-child → webserver callback channel. Every `ipc::spawn_child` task's result lands here and gets routed to a task-specific handler internally |

---

## Dashboard

| Method | Path | Notes |
|---|---|---|
| GET | `/dataview/dashboard` | Live counts: `{active_folder, endpoints, bookmarks, history}` |
| GET | `/dashboard/snapshot` | Latest periodic snapshot (endpoint/bookmark/tag counts, db size, storage size) |
| GET | `/dashboard/snapshot/history` | Every retained snapshot, oldest first (30-day rolling window) — for trend charts |
| GET | `/dashboard/static` | Config-driven static info: limits, stability/commit info, links |
| GET | `/dashboard/crud-operations` | Breakdown by entity type + HTTP method; 404 until the first refresh runs |
| POST | `/dashboard/crud-operations/refresh` | Fire-and-forget — triggers a recompute, result lands via internal callback |
| GET | `/dashboard/compare?from=YYYY-MM-DD&to=YYYY-MM-DD` | Day-over-day comparison between two daily snapshots |

**Known gap:** Dashboard has no WebSocket connection of its own yet — none of the above pushes live updates. Poll, or re-fetch after triggering a refresh.

---

## Known limitations worth knowing before integrating

- Bookmark actions don't validate that an endpoint exists before bookmarking it (Collections does).
- No size limits enforced anywhere (Collections count, endpoints-per-list, History/Bookmarks caps).
- Deleting an endpoint now cascades its bookmarks correctly (2026-09-22), but **not** collection memberships or history/request/response data — those can still be left behind, orphaned, referencing a dead endpoint.
- A QP (request/response pair — see Test View above) is generated automatically by every test run, not created/edited by hand. There's no route to edit a QP's saved request/response in place, only to list and delete.
- Import (`/repo/:collection/:filename/import`) only extracts a zip to disk — it does not create/update endpoint, bookmark, or collection-membership DB rows from the imported files.
- Webview still has no independent per-instance SQLite file (unlike Collections and now Repoview) and no endpoint-membership routes at all.
- Renaming or deleting a RepoView doesn't cascade its row-level membership-tags (`repoview_tag_memberships`) — same pre-existing gap Collections has with `collection_tag_memberships`, not something new introduced here.
- A RepoView is a point-in-time list: it doesn't follow its source Collection or the central tables afterwards (an endpoint renamed or deleted centrally keeps its old row in the index), and the generated `Tables-*.md` are a snapshot until `tables/generate` is called again. Its `source` is the Collection it was created from.
- A RepoView's name, annotation, source and row tags still live in the central database's `repoview*` tables, not in its folder, so a RepoView folder carried to a brand-new instance can't be re-registered yet. Import/Export (not built) is the intended way across instances.
- RepoViews created before 2026-10-04 have no endpoint snapshot in their index, so their stats and Tables files come out empty; re-create them. Ones created between 2026-10-04 and v1.0 still have a leftover `globalEQPData/` folder inside, which is now ignored (it is removed with the RepoView).
- `(endpoint_str, method)` is only a plain index centrally, not a unique one, so nothing in the DB itself stops two endpoints sharing a URL+method; the frontend should use `GET /endpoints/lookup` first.
