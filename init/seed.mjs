// init/seed.mjs — first-boot demo data for the EDMS stack.
//
// Idempotent: if the backend already has endpoints it exits without
// touching anything. Otherwise it populates a realistic hashedtokens /
// EDMS dataset — endpoints, tags, test-view history, and collections
// whose members include endpoints that have actually been run (so they
// carry request / response / headers data).
//
// Runs automatically as the `seed` service in docker-compose.yml on
// `docker compose up`. Also runnable by hand against a live backend:
//   EDMS_BASE=http://localhost:3000 node init/seed.mjs
//
// Requires Node 22+ (built-in fetch + WebSocket).
//
// It talks only to the HTTP/WS API — it never writes files or the DB
// directly — so it stays correct if the on-disk storage layout moves.

const BASE = process.env.EDMS_BASE || "http://localhost:3000";
const WS_BASE = BASE.replace(/^http/, "ws");

// The backend routes the test call through a detached child process that
// runs inside the webserver container, so "internal" endpoints target
// the server's own loopback — reachable, fast, deterministic.
const LOOPBACK = "http://localhost:3000";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function post(path, body) {
  const r = await fetch(BASE + path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  let json = null;
  try { json = await r.json(); } catch {}
  return { status: r.status, json };
}

function wsOnce(path, send, waitType, timeoutMs = 12000) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_BASE + path);
    let done = false;
    const finish = (v) => {
      if (done) return;
      done = true;
      clearTimeout(t);
      try { ws.close(); } catch {}
      resolve(v);
    };
    const t = setTimeout(() => finish(null), timeoutMs);
    ws.addEventListener("open", () => {
      if (send !== undefined) ws.send(typeof send === "string" ? send : JSON.stringify(send));
    });
    ws.addEventListener("message", (e) => {
      let m;
      try { m = JSON.parse(e.data); } catch { return; }
      const ty = m?.event?.type || m?.type;
      if (!waitType || ty === waitType) finish(m);
    });
    ws.addEventListener("error", (err) => {
      if (!done) { done = true; clearTimeout(t); reject(err); }
    });
  });
}

// ── data ────────────────────────────────────────────────────────────

// External hashedtokens API surface — created but not run. These show up
// in the Endpoints tab and as collection members.
const EXTERNAL = [
  { method: "POST",   url: "https://api.hashedtokens.com/v1/auth/login",           note: "Exchange credentials for an access + refresh token pair." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/auth/logout",          note: "Revoke the current session's refresh token." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/auth/refresh",         note: "Rotate an access token using a valid refresh token." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/auth/register",        note: "Create a new hashedtokens account." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/auth/session",         note: "Return the caller's current session and scopes." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/users",                note: "List users in the org, paginated." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/users/{id}",           note: "Fetch a single user by ID." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/users",               note: "Invite / provision a new user." },
  { method: "PUT",    url: "https://api.hashedtokens.com/v1/users/{id}",           note: "Update a user's profile or role." },
  { method: "DELETE", url: "https://api.hashedtokens.com/v1/users/{id}",           note: "Deactivate a user." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/documents",            note: "List documents visible to the caller." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/documents/{id}",       note: "Fetch document metadata + latest version." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/documents",           note: "Create a new document." },
  { method: "PUT",    url: "https://api.hashedtokens.com/v1/documents/{id}",       note: "Replace a document's contents (new version)." },
  { method: "DELETE", url: "https://api.hashedtokens.com/v1/documents/{id}",       note: "Soft-delete a document." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/documents/{id}/share", note: "Grant another user access to a document." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/webhooks",             note: "List configured webhooks." },
  { method: "POST",   url: "https://api.hashedtokens.com/v1/webhooks",            note: "Register a webhook endpoint + event filter." },
  { method: "DELETE", url: "https://api.hashedtokens.com/v1/webhooks/{id}",        note: "Remove a webhook." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/billing/usage",        note: "Usage counters for the current billing period." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/admin/audit-log",      note: "Org audit log, most recent first." },
  { method: "GET",    url: "https://api.hashedtokens.com/v1/admin/stats",          note: "Internal admin dashboard stats." },
];

// EDMS's own endpoints — these get run, so they carry real
// request/response/headers data and generate genuine history entries.
const INTERNAL = [
  { method: "GET", url: `${LOOPBACK}/home`,               note: "EDMS home view metadata — used as a liveness check." },
  { method: "GET", url: `${LOOPBACK}/list-view`,          note: "EDMS list view metadata." },
  { method: "GET", url: `${LOOPBACK}/dataview/dashboard`, note: "Live dashboard counts (endpoints / bookmarks / history)." },
];

const HISTORY_DETAILS = [
  "200 OK in 41ms", "200 OK in 63ms", "201 Created in 88ms",
  "204 No Content in 33ms", "400 Bad Request in 22ms",
  "401 Unauthorized in 18ms", "404 Not Found in 19ms",
  "429 Too Many Requests in 12ms", "500 Server Error in 120ms",
];

// ── steps ───────────────────────────────────────────────────────────

async function waitForServer() {
  for (let i = 0; i < 60; i++) {
    try {
      const r = await fetch(BASE + "/home");
      if (r.ok) return;
    } catch {}
    await sleep(1000);
  }
  throw new Error(`backend at ${BASE} never came up`);
}

async function alreadySeeded() {
  const snap = await wsOnce("/test-view/endpoints/load", undefined, "snapshot");
  return (snap?.endpoints?.length || 0) > 0;
}

async function createEndpoint(e) {
  const r = await post("/endpoints/create", {
    endpoint_str: e.url,
    method: e.method,
    annotation: e.note,
  });
  if (!r.json?.endpoint_id) throw new Error(`create failed for ${e.method} ${e.url}: ${JSON.stringify(r.json)}`);
  return r.json.endpoint_id;
}

async function runEndpoint(eid, url, method) {
  await wsOnce(
    "/test-view/run",
    { type: "run_test", payload: { endpoint_id: eid, endpoint_str: url, method, body: {}, timeout_ms: 6000, tick_interval_ms: 300 } },
    "TestFinished",
    15000,
  );
}

async function seedCollection(name, eids) {
  await post("/collections/create", { name });
  await wsOnce(`/bookmarks/${name}/load`, undefined, "collection_loaded");
  for (const eid of eids) {
    await post("/test-view/save/bookmark", { endpoint_id: eid, notes: "" });
    await post(`/bookmarks/active/${eid}/save`);
  }
}

async function main() {
  console.log(`[seed] target ${BASE}`);
  await waitForServer();

  if (await alreadySeeded()) {
    console.log("[seed] backend already has endpoints — nothing to do.");
    return;
  }

  console.log(`[seed] creating ${EXTERNAL.length} hashedtokens API endpoints…`);
  const ext = [];
  for (const e of EXTERNAL) ext.push({ ...e, id: await createEndpoint(e) });

  console.log(`[seed] creating + running ${INTERNAL.length} internal endpoints…`);
  const intl = [];
  for (const e of INTERNAL) {
    const id = await createEndpoint(e);
    intl.push({ ...e, id });
    const runs = 1 + Math.floor(Math.random() * 3);
    for (let k = 0; k < runs; k++) await runEndpoint(id, e.url, e.method);
  }

  console.log("[seed] tagging…");
  const tag = (id, t) => post(`/tags/${id}/add`, { tag: t });
  for (const e of ext.slice(0, 8)) await tag(e.id, "production");
  for (const e of ext.slice(8, 12)) await tag(e.id, "internal");
  for (const e of ext.slice(12, 14)) await tag(e.id, "deprecated");
  for (const e of intl) await tag(e.id, "internal");

  console.log("[seed] history…");
  for (const e of ext.slice(0, 16)) {
    await post("/test-view/save/history", {
      endpoint_id: e.id,
      action: "test",
      details: HISTORY_DETAILS[Math.floor(Math.random() * HISTORY_DETAILS.length)],
    });
  }

  console.log("[seed] collections…");
  await seedCollection("auth-service", ext.slice(0, 5).map((e) => e.id));
  await seedCollection("document-api", ext.slice(10, 16).map((e) => e.id));
  await seedCollection("edms-internal", [...intl.map((e) => e.id), ext[20].id, ext[21].id]);

  // Leave edms-internal loaded with two endpoints bookmarked but NOT
  // saved into it, so Bookmark View shows both states.
  await post("/test-view/save/bookmark", { endpoint_id: ext[5].id, notes: "" });
  await post("/test-view/save/bookmark", { endpoint_id: ext[6].id, notes: "" });

  console.log(`[seed] done — ${ext.length + intl.length} endpoints, 3 collections, 3 tags, history populated.`);
}

main()
  .then(() => process.exit(0))
  .catch((e) => {
    console.error("[seed] FAILED:", e);
    process.exit(1);
  });
