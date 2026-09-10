// init/seed.mjs — first-boot demo data for the EDMS stack.
//
// Idempotent: if the backend already has endpoints it exits without
// touching anything. Otherwise it populates a realistic dataset — every
// endpoint is created by actually being RUN against https://dummyjson.com
// (a public test API), so each one carries real request / response /
// headers data and a genuine history entry, exactly as the product
// intends ("an endpoint exists because it was tested").
//
// Runs automatically as the `seed` service in docker-compose.yml on
// `docker compose up`, then exits. Also runnable by hand:
//   EDMS_BASE=http://localhost:3000 node init/seed.mjs
//
// Requires Node 22+ (built-in fetch + WebSocket) and outbound internet
// from the backend container (it's the backend that makes the test call).
// Offline: endpoints still get created, just without run data.
//
// Talks only to the HTTP/WS API — never the DB or filesystem — so it
// stays correct if the on-disk storage layout moves.

const BASE = process.env.EDMS_BASE || "http://localhost:3000";
const WS_BASE = BASE.replace(/^http/, "ws");

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

function wsOnce(path, send, waitType, timeoutMs = 15000) {
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
// Real, reachable URLs against https://dummyjson.com. `runs` is how many
// times to test it (>1 gives an endpoint multiple request/response sets).

const API = "https://dummyjson.com";
const ENDPOINTS = [
  { method: "POST",   url: `${API}/auth/login`,            note: "Authenticate with username + password, receive a bearer token.", runs: 2 },
  { method: "GET",    url: `${API}/auth/me`,               note: "Return the currently authenticated user (401 without a token).", runs: 1 },
  { method: "GET",    url: `${API}/users`,                 note: "List all users, paginated (limit / skip).", runs: 2 },
  { method: "GET",    url: `${API}/users/1`,               note: "Fetch a single user by ID.", runs: 1 },
  { method: "GET",    url: `${API}/users/search?q=John`,   note: "Search users by name.", runs: 1 },
  { method: "POST",   url: `${API}/users/add`,             note: "Provision a new user.", runs: 1 },
  { method: "PUT",    url: `${API}/users/1`,               note: "Update a user's profile.", runs: 1 },
  { method: "DELETE", url: `${API}/users/1`,               note: "Deactivate a user.", runs: 1 },
  { method: "GET",    url: `${API}/products`,              note: "List products, paginated.", runs: 2 },
  { method: "GET",    url: `${API}/products/1`,            note: "Fetch a product by ID.", runs: 1 },
  { method: "GET",    url: `${API}/products/search?q=phone`, note: "Full-text product search.", runs: 1 },
  { method: "GET",    url: `${API}/products/categories`,   note: "List product categories.", runs: 1 },
  { method: "POST",   url: `${API}/products/add`,          note: "Create a product.", runs: 1 },
  { method: "PUT",    url: `${API}/products/1`,            note: "Update a product.", runs: 1 },
  { method: "DELETE", url: `${API}/products/1`,            note: "Remove a product.", runs: 1 },
  { method: "GET",    url: `${API}/carts`,                 note: "List all carts.", runs: 1 },
  { method: "GET",    url: `${API}/carts/1`,               note: "Fetch a cart with its line items.", runs: 1 },
  { method: "POST",   url: `${API}/carts/add`,             note: "Create a cart for a user.", runs: 1 },
  { method: "GET",    url: `${API}/posts`,                 note: "List posts, paginated.", runs: 2 },
  { method: "GET",    url: `${API}/posts/1`,               note: "Fetch a post by ID.", runs: 1 },
  { method: "GET",    url: `${API}/posts/1/comments`,      note: "Comments on a post.", runs: 1 },
  { method: "POST",   url: `${API}/posts/add`,             note: "Create a post.", runs: 1 },
  { method: "GET",    url: `${API}/todos`,                 note: "List todos.", runs: 1 },
  { method: "GET",    url: `${API}/todos/random`,          note: "Fetch a random todo.", runs: 1 },
  { method: "GET",    url: `${API}/comments`,              note: "List all comments.", runs: 1 },
];

const EXTRA_HISTORY = [
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

// Create-by-running: first run (no endpoint_id) creates + tests the
// endpoint; extra runs reuse the allocated id. Returns the EID, or null
// if the run never reported back (e.g. offline).
async function seedEndpoint(e) {
  const first = await wsOnce(
    "/test-view/run",
    { type: "run_test", payload: { endpoint_str: e.url, method: e.method, annotation: e.note, body: {}, timeout_ms: 8000, tick_interval_ms: 300 } },
    "TestFinished",
    18000,
  );
  const eid = first?.event?.payload?.endpoint_id;
  if (!eid) return null;
  for (let k = 1; k < (e.runs || 1); k++) {
    await wsOnce(
      "/test-view/run",
      { type: "run_test", payload: { endpoint_id: eid, endpoint_str: e.url, method: e.method, body: {}, timeout_ms: 8000, tick_interval_ms: 300 } },
      "TestFinished",
      18000,
    );
  }
  return eid;
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

  console.log(`[seed] creating + running ${ENDPOINTS.length} endpoints against ${API} …`);
  const ids = [];
  let ran = 0;
  for (const e of ENDPOINTS) {
    const id = await seedEndpoint(e);
    ids.push(id);
    if (id) ran++;
  }
  const eids = ids.filter(Boolean);
  console.log(`[seed] ${ran}/${ENDPOINTS.length} endpoints have run data` + (ran < ENDPOINTS.length ? " (rest created without it — offline?)" : ""));

  if (eids.length === 0) throw new Error("no endpoints were created — is the backend reachable and online?");

  console.log("[seed] tagging …");
  const tag = (id, t) => post(`/tags/${id}/add`, { tag: t });
  for (const id of eids.slice(0, 10)) await tag(id, "production");
  for (const id of eids.slice(10, 15)) await tag(id, "internal");
  for (const id of eids.slice(15, 17)) await tag(id, "deprecated");

  console.log("[seed] extra history …");
  for (const id of eids.slice(0, 10)) {
    await post("/test-view/save/history", {
      endpoint_id: id,
      action: "test",
      details: EXTRA_HISTORY[Math.floor(Math.random() * EXTRA_HISTORY.length)],
    });
  }

  console.log("[seed] collections …");
  await seedCollection("user-directory", eids.slice(2, 8));
  await seedCollection("product-catalog", eids.slice(8, 15));
  await seedCollection("content-api", eids.slice(18, 24));

  // Leave content-api loaded with two endpoints bookmarked but NOT saved
  // into it, so Bookmark View shows both states.
  if (eids[0]) await post("/test-view/save/bookmark", { endpoint_id: eids[0], notes: "" });
  if (eids[1]) await post("/test-view/save/bookmark", { endpoint_id: eids[1], notes: "" });

  console.log(`[seed] done — ${eids.length} endpoints, 3 collections, 3 tags, history populated.`);
}

main()
  .then(() => process.exit(0))
  .catch((e) => {
    console.error("[seed] FAILED:", e);
    process.exit(1);
  });
