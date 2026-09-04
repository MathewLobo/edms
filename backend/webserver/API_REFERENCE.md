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
# first time only — the bind-mounted db file must exist before Docker mounts it,
# otherwise Docker creates it as a directory instead of a file
touch webserver/edms.db
docker compose up --build -d
```
Stop it with `docker compose down`. Data survives via named/bind-mounted volumes (`edms.db`, `edms_data`, `edms_root`).

### Option C — native (Rust toolchain required)
```bash
cd backend/webserver
cargo run --bin rust-webserver
```

---

## The one rule that matters for WebSocket routes

**WebSocket only notifies — REST delivers the actual data.** Every WS connection below streams messages shaped like:
```json
{ "type": "event", "event": { "type": "<EventName>", "payload": { ... } } }
```
Treat these as "something changed, go re-fetch," not as the final source of truth for anything except live progress (test timers).

---

## Test View

| Method | Path | Type | Body / Notes |
|---|---|---|---|
| GET | `/test-view` | REST | Static view metadata |
| GET | `/test-view/endpoints/load` | WS | Sends a snapshot of all endpoints on connect, then streams events |
| GET | `/test-view/bookmarks/load` | WS | Same, for the `active` bookmark set |
| GET | `/test-view/run` | WS | Send `{"type":"run_test","payload":{"endpoint_id","method","body","timeout_ms","tick_interval_ms"}}` to start a test. Streams `TestStarted` → `TimerTick`s → `TestFinished`/`TestTimeout` |
| POST | `/test-view/stop` | REST | Body `{"endpoint_id","request_number"}` — cancels the app's tracking of an in-flight test (does not kill the underlying HTTP call already running) |
| POST | `/test-view/save/history` | REST | Body `{"endpoint_id","action","details"}` — manual history entry (History also now auto-records on every completed test, no manual call needed for that case) |
| POST | `/test-view/save/bookmark` | REST | Body `{"endpoint_id","notes"}` — bookmarks into `active` |
| GET | `/test-view/:bookmark/add` | WS | Send `{"endpoint_id"}` — adds to the named folder (not just `active`) |
| GET | `/test-view/:bookmark/delete` | WS | Send `{"endpoint_id"}` — removes from the named folder |
| GET | `/test-view/:endpoint_id/request/:request_number` | REST | Fetch a saved request body |
| GET | `/test-view/:endpoint_id/response/:request_number` | REST | Fetch a saved response body |
| POST | `/test-view/history/clearall` | REST | Wipes all history |
| POST | `/test-view/bookmark/clearall` | REST | Wipes the `active` bookmark set |

**Bookmarks (System 1 — old, still used by Test View):**

| Method | Path | Type | Notes |
|---|---|---|---|
| POST | `/bookmarks/:collection/create` | REST | Copies `active` into the named folder |
| GET | `/bookmarks/:collection/load` | WS | Loads a named folder into `active` (backs up the previous `active` first), then streams events |

---

## Collections (System 2 — new, per-collection files)

Each collection is its own real file (`storage/collections/{name}.sqlite`), holding just `endpoint_id` + `added_at`. The endpoint's actual data always stays in the central `endpoints` table — Collections never copies it.

| Method | Path | Body | Notes |
|---|---|---|---|
| POST | `/collections/create` | `{"name"}` | Creates the catalog row + the real file |
| GET | `/collections/list` | — | All collections |
| GET | `/collections/:name` | — | One collection's catalog row; 404 if missing |
| POST | `/collections/:name/rename` | `{"new_name"}` | Renames the catalog entry + moves the file; rejects a name collision cleanly, no data loss |
| POST | `/collections/:name/delete` | — | Removes the catalog row + deletes the file |
| POST | `/collections/:name/endpoints/add` | `{"endpoint_id"}` | Rejects an endpoint_id that doesn't exist centrally |
| POST | `/collections/:name/endpoints/remove` | `{"endpoint_id"}` | |
| GET | `/collections/:name/endpoints` | — | Lists members with `added_at` |

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
- Deleting an endpoint doesn't cascade — orphaned bookmarks, collection memberships, and history/request/response data can be left behind.
- Only request/response are captured per test — no separate headers file yet.
