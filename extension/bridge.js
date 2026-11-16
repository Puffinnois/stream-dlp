// The extension's logic, kept free of the global `chrome` so Deno can test it with a fake.
// It reads YouTube cookies only when the person presses Connect (or when the connected app
// asks for one refresh), and sends them to stream-dlp's native host. No other site, no
// storage, no network requests. The app checks everything again.

export const HOST = "com.stream_dlp.youtube_session";
export const PROTOCOL = 1;
/// The domain the app's YouTube session policy accepts, with its subdomains.
export const DOMAIN = "youtube.com";
/// The app asks for at most one refresh per inspection; this also limits a misbehaving host.
export const REFRESH_INTERVAL_MS = 10_000;

export function inScope(domain) {
  if (typeof domain !== "string") return false;
  const bare = (domain.startsWith(".") ? domain.slice(1) : domain).toLowerCase();
  return bare === DOMAIN || bare.endsWith("." + DOMAIN);
}

/// Exactly the fields the app accepts; anything else in the browser's cookie stays here.
export function toAppCookie(cookie) {
  const out = {
    domain: cookie.domain,
    hostOnly: Boolean(cookie.hostOnly),
    path: cookie.path,
    secure: Boolean(cookie.secure),
    httpOnly: Boolean(cookie.httpOnly),
    name: cookie.name,
    value: cookie.value,
  };
  if (!cookie.session && typeof cookie.expirationDate === "number") {
    out.expirationDate = cookie.expirationDate;
  }
  return out;
}

/// This profile's unpartitioned YouTube cookies, in the app's shape.
export async function readSession(api) {
  const all = await api.cookies.getAll({ domain: DOMAIN });
  return all.filter((c) => !c.partitionKey && inScope(c.domain)).map(toAppCookie);
}

const MESSAGES = {
  connecting: "Connecting to stream-dlp…",
  sending: "Sending your YouTube session…",
  connected: "Connected. stream-dlp has your YouTube session.",
  disconnected: "Not connected.",
  noSession: "No YouTube session in this profile. Sign in to YouTube here first.",
  readFailed: "Could not read the YouTube session.",
  rejected: "stream-dlp refused the session. Check the message in the app.",
  busy: "stream-dlp is connected to another browser or profile. Disconnect it there first.",
  unavailable: "stream-dlp is not reachable. Open it, turn on Dev mode, choose Chrome or Edge and press Connect there, then try again.",
  hostMissing: "stream-dlp is not registered with this browser. Press Connect in the app first.",
  byApp: "stream-dlp disconnected.",
};

/// The connection to the native host. `onChange` receives `{state, detail}`.
export function createBridge(api, onChange = () => {}, clock = () => Date.now()) {
  let port = null;
  let state = "disconnected";
  let detail = MESSAGES.disconnected;
  let lastRefresh = -Infinity;

  const set = (next, text) => {
    state = next;
    detail = text;
    onChange({ state, detail });
  };

  const close = () => {
    if (port) {
      const old = port;
      port = null;
      try { old.disconnect(); } catch { /* already closed */ }
    }
  };

  async function sendSession() {
    let cookies;
    try {
      cookies = await readSession(api);
    } catch {
      set("error", MESSAGES.readFailed);
      return;
    }
    if (!port) return;
    if (cookies.length === 0) {
      set("error", MESSAGES.noSession);
      return;
    }
    port.postMessage({ type: "session", cookies });
    set("sending", MESSAGES.sending);
  }

  function onMessage(message) {
    if (!message || typeof message !== "object") return;
    if (message.type === "status") {
      switch (message.state) {
        case "connected": sendSession(); break;
        case "imported": set("connected", MESSAGES.connected); break;
        case "rejected": set("error", MESSAGES.rejected); break;
        case "busy": close(); set("error", MESSAGES.busy); break;
        case "app_unavailable": close(); set("error", MESSAGES.unavailable); break;
      }
    } else if (message.type === "refresh") {
      const now = clock();
      if (state === "connected" && now - lastRefresh >= REFRESH_INTERVAL_MS) {
        lastRefresh = now;
        sendSession();
      }
    } else if (message.type === "disconnect") {
      close();
      set("disconnected", MESSAGES.byApp);
    }
  }

  function onDisconnect() {
    const error = api.runtime.lastError?.message ?? "";
    port = null;
    if (state === "disconnected" || state === "error") return;
    set("error", /not found|forbidden|not registered/i.test(error) ? MESSAGES.hostMissing : MESSAGES.unavailable);
  }

  function connect() {
    if (port) return sendSession();
    try {
      port = api.runtime.connectNative(HOST);
    } catch {
      set("error", MESSAGES.hostMissing);
      return;
    }
    port.onMessage.addListener(onMessage);
    port.onDisconnect.addListener(onDisconnect);
    port.postMessage({ type: "hello", protocol: PROTOCOL });
    set("connecting", MESSAGES.connecting);
  }

  function disconnect() {
    if (port) {
      try { port.postMessage({ type: "disconnect" }); } catch { /* already closed */ }
    }
    close();
    set("disconnected", MESSAGES.disconnected);
  }

  return { connect, disconnect, status: () => ({ state, detail }) };
}

/// Messages from the popup: only from this extension, only these three requests.
export function handlePopupMessage(bridge, api, message, sender, reply) {
  if (sender?.id !== api.runtime.id || !message || typeof message !== "object") return false;
  switch (message.type) {
    case "connect": bridge.connect(); break;
    case "disconnect": bridge.disconnect(); break;
    case "status": break;
    default: return false;
  }
  reply(bridge.status());
  return false;
}
