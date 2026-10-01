const ws = new WebSocket("ws://localhost:3000/test-view/run");
ws.addEventListener("message", (e) => console.log(e.data));
await new Promise((r) => (ws.onopen = r));
ws.send(JSON.stringify({
  type: "run_test",
  payload: { endpoint_id: "MY_TEST", method: "GET", body: {}, timeout_ms: 8000, tick_interval_ms: 500 },
}));
await new Promise((r) => setTimeout(r, 5000));
ws.close();