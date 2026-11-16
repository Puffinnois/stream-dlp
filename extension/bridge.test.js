import {
  createBridge,
  handlePopupMessage,
  HOST,
  inScope,
  readSession,
  REFRESH_INTERVAL_MS,
  toAppCookie,
} from "./bridge.js";

function assert(value, message = "assertion failed") { if (!value) throw new Error(message); }
function equal(a, b) { assert(JSON.stringify(a) === JSON.stringify(b), `${JSON.stringify(a)} !== ${JSON.stringify(b)}`); }
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

const cookie = (domain, name, extra = {}) => ({
  domain, name, value: "fake-test-value", path: "/", hostOnly: !domain.startsWith("."),
  secure: true, httpOnly: false, session: false, expirationDate: 4102444800, sameSite: "lax",
  storeId: "0", ...extra,
});

/// A fake `chrome` with one native port that records what the extension posts.
function fakeChrome(cookies = [cookie(".youtube.com", "GOOD")]) {
  const api = {
    queries: [],
    connects: [],
    posted: [],
    port: null,
    runtime: {
      id: "edngekgkkjhaoohkfddejaoohfaggiac",
      lastError: undefined,
      connectNative(name) {
        api.connects.push(name);
        const listeners = { message: [], disconnect: [] };
        api.port = {
          disconnected: false,
          postMessage: (m) => api.posted.push(JSON.parse(JSON.stringify(m))),
          disconnect() { this.disconnected = true; },
          onMessage: { addListener: (f) => listeners.message.push(f) },
          onDisconnect: { addListener: (f) => listeners.disconnect.push(f) },
          receive: (m) => listeners.message.forEach((f) => f(m)),
          drop: () => listeners.disconnect.forEach((f) => f()),
        };
        return api.port;
      },
    },
    cookies: {
      getAll(details) {
        api.queries.push(details);
        return Promise.resolve(cookies);
      },
    },
  };
  return api;
}

Deno.test("only youtube.com and its subdomains are in scope", () => {
  for (const d of ["youtube.com", ".youtube.com", "www.youtube.com", ".M.YouTube.com"]) assert(inScope(d), d);
  for (const d of ["notyoutube.com", "youtube.com.evil.test", ".youtu.be", "google.com", "", null, undefined, 3]) assert(!inScope(d), String(d));
});

Deno.test("the session holds only scoped, unpartitioned cookies with only the app's fields", async () => {
  const api = fakeChrome([
    cookie(".youtube.com", "GOOD"),
    cookie("www.youtube.com", "HOST", { session: true, expirationDate: undefined, httpOnly: true }),
    cookie(".example.com", "OTHER"),
    cookie(".youtube.com.evil.test", "EVIL"),
    cookie(".youtube.com", "PARTITIONED", { partitionKey: { topLevelSite: "https://example.com" } }),
  ]);
  const session = await readSession(api);
  equal(api.queries, [{ domain: "youtube.com" }]);
  equal(session.map((c) => c.name), ["GOOD", "HOST"]);
  equal(Object.keys(session[0]).sort(), ["domain", "expirationDate", "hostOnly", "httpOnly", "name", "path", "secure", "value"]);
  assert(!("expirationDate" in session[1]), "A session cookie has no expiry");
  equal(toAppCookie(cookie(".youtube.com", "X")).hostOnly, false);
});

Deno.test("connect says hello, sends the session after the app accepts, and reports the import", async () => {
  const api = fakeChrome();
  const states = [];
  const bridge = createBridge(api, (s) => states.push(s.state));
  bridge.connect();
  equal(api.connects, [HOST]);
  equal(api.posted, [{ type: "hello", protocol: 1 }]);
  api.port.receive({ type: "status", state: "connected" });
  await tick();
  equal(api.posted[1].type, "session");
  equal(api.posted[1].cookies.length, 1);
  api.port.receive({ type: "status", state: "imported" });
  equal(bridge.status().state, "connected");
  equal(states, ["connecting", "sending", "connected"]);
});

Deno.test("refresh is answered at most once per interval and only when connected", async () => {
  let now = 1_000_000;
  const api = fakeChrome();
  const bridge = createBridge(api, () => {}, () => now);
  api.port?.receive({ type: "refresh" });
  bridge.connect();
  api.port.receive({ type: "refresh" });
  await tick();
  equal(api.posted.length, 1, "No session before the app accepted the connection");
  api.port.receive({ type: "status", state: "connected" });
  await tick();
  api.port.receive({ type: "status", state: "imported" });
  api.port.receive({ type: "refresh" });
  await tick();
  equal(api.posted.length, 3);
  api.port.receive({ type: "status", state: "imported" });
  api.port.receive({ type: "refresh" });
  await tick();
  equal(api.posted.length, 3, "A second refresh within the interval is ignored");
  now += REFRESH_INTERVAL_MS;
  api.port.receive({ type: "refresh" });
  await tick();
  equal(api.posted.length, 4);
});

Deno.test("disconnect on either side closes the port; an unreachable app is explained", async () => {
  const api = fakeChrome();
  const bridge = createBridge(api);
  bridge.connect();
  const first = api.port;
  bridge.disconnect();
  equal(api.posted.at(-1), { type: "disconnect" });
  assert(first.disconnected);
  equal(bridge.status().state, "disconnected");

  bridge.connect();
  api.port.receive({ type: "disconnect" });
  assert(api.port.disconnected);
  equal(bridge.status().state, "disconnected");

  bridge.connect();
  api.port.receive({ type: "status", state: "app_unavailable" });
  assert(api.port.disconnected);
  assert(bridge.status().detail.includes("not reachable"));

  bridge.connect();
  api.runtime.lastError = { message: "Specified native messaging host not found." };
  api.port.drop();
  assert(bridge.status().detail.includes("not registered"));
});

Deno.test("an empty profile sends nothing and unknown messages are ignored", async () => {
  const api = fakeChrome([cookie(".example.com", "OTHER")]);
  const bridge = createBridge(api);
  bridge.connect();
  api.port.receive({ type: "status", state: "connected" });
  await tick();
  equal(api.posted.length, 1);
  assert(bridge.status().detail.includes("Sign in to YouTube"));
  for (const m of [null, "x", { type: "eval", code: "1" }, { type: "status", state: "owned" }]) api.port.receive(m);
  equal(api.posted.length, 1);
});

Deno.test("popup messages are accepted only from this extension", () => {
  const api = fakeChrome();
  const bridge = createBridge(api);
  const replies = [];
  handlePopupMessage(bridge, api, { type: "connect" }, { id: "another-extension" }, (r) => replies.push(r));
  handlePopupMessage(bridge, api, { type: "steal" }, { id: api.runtime.id }, (r) => replies.push(r));
  equal(api.connects.length, 0);
  equal(replies.length, 0);
  handlePopupMessage(bridge, api, { type: "connect" }, { id: api.runtime.id }, (r) => replies.push(r));
  equal(api.connects.length, 1);
  equal(replies[0].state, "connecting");
});
