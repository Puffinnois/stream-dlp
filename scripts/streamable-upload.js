// Embedded by Rust. No packages, private endpoints, or access to normal profiles.
export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 0;
    this.pending = new Map();
    this.events = new Map();
    socket.addEventListener("message", ({ data }) => {
      const message = JSON.parse(data);
      if (message.method) {
        const listener = this.events.get(message.method);
        if (listener) { this.events.delete(message.method); clearTimeout(listener.timer); listener.resolve(message.params); }
        return;
      }
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error) pending.reject(new Error("browser_command_failed"));
      else pending.resolve(message.result);
    });
    socket.addEventListener("close", () => {
      for (const pending of this.pending.values()) {
        clearTimeout(pending.timer);
        pending.reject(new Error("browser_closed"));
      }
      this.pending.clear();
      for (const listener of this.events.values()) { clearTimeout(listener.timer); listener.reject(new Error("browser_closed")); }
      this.events.clear();
    });
  }
  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { socket.close(); reject(new Error("browser_timeout")); }, 10000);
      socket.addEventListener("open", () => { clearTimeout(timer); resolve(); }, { once: true });
      socket.addEventListener("error", () => { clearTimeout(timer); reject(new Error("browser_connection_failed")); }, { once: true });
    });
    return new Cdp(socket);
  }
  call(method, params = {}) {
    if (this.socket.readyState !== WebSocket.OPEN) return Promise.reject(new Error("browser_closed"));
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); const error = new Error("browser_timeout"); error.command = method; reject(error); }, 10000);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }
  async value(expression) {
    const result = await this.call("Runtime.evaluate", { expression, returnByValue: true, userGesture: true });
    if (result.exceptionDetails) throw new Error("page_changed");
    return result.result.value;
  }
  once(event) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.events.delete(event); reject(new Error("upload_control_missing")); }, 10000);
      this.events.set(event, {resolve, reject, timer});
    });
  }
  async newPage(url) {
    const local = new URL(this.socket.url);
    // window.open preserves this private browser context across Edge/Chrome.
    await this.value(`window.open(${JSON.stringify(url)}, '_blank'); true`);
    let page;
    const deadline = Date.now() + 10000;
    while (!page && Date.now() < deadline) {
      const pages = await (await fetch(`http://127.0.0.1:${local.port}/json/list`)).json();
      page = pages.find(page => page.type === "page" && page.url === url);
      if (!page) await sleep(100);
    }
    if (!page) throw new Error("playback_failed");
    const socket = new URL(page.webSocketDebuggerUrl);
    if (!["127.0.0.1", "localhost"].includes(socket.hostname) || socket.port !== local.port) throw new Error("invalid_browser_endpoint");
    socket.hostname = "127.0.0.1";
    return Cdp.connect(socket.href);
  }
  close() { this.socket.close(); }
}

export function shareLink(value, origin = "https://streamable.com") {
  try {
    const url = new URL(value);
    return url.origin === origin && !url.username && !url.password && !url.search && !url.hash &&
      /^\/[a-zA-Z0-9]{3,16}$/.test(url.pathname) ? url.href : null;
  } catch { return null; }
}

// Reads only rendered upload controls/status, never browser storage or cookies.
const snapshot = `(() => {
  const visible = e => !!e && e.getClientRects().length > 0;
  const text = document.body?.innerText || '';
  const progress = [...document.querySelectorAll('[role="progressbar"],progress,.progress-bar')].find(visible);
  const raw = progress?.getAttribute('aria-valuenow') || progress?.getAttribute('value') || progress?.style.width?.replace('%','');
  const percent = raw === undefined || raw === null || raw === '' ? null : Number(raw);
  return {
    origin: location.origin,
    input: !!document.querySelector('input[type=file]'),
    uploadButton: [...document.querySelectorAll('button')].some(e => visible(e) && /^Upload video(?: for free)?$/i.test(e.innerText.replace(/\\s+/g,' ').trim())),
    challenge: /verify you are human|checking your browser|verification required/i.test(text) ||
      [...document.querySelectorAll('iframe[src*="challenges.cloudflare.com"],iframe[src*="recaptcha"]')].some(visible),
    links: [...document.querySelectorAll('a#video-url-input')].map(e => e.href),
    processing: /processing/i.test(text),
    error: [...document.querySelectorAll('.alert-danger,[role=alert]')].some(e => visible(e) && /failed|error|too large|limit|unsupported/i.test(e.innerText)),
    percent: Number.isFinite(percent) && percent >= 0 && percent <= 100 ? percent : null
  };
})()`;

export async function automate(cdp, file, emit, origin = "https://streamable.com", timeout = 18 * 60 * 1000) {
  const deadline = Date.now() + timeout;
  let previous = "";
  const progress = (stage, percent = null) => {
    const value = JSON.stringify({ kind: "progress", stage, percent });
    if (value !== previous) { previous = value; emit(JSON.parse(value)); }
  };
  progress("starting");
  let baseline;
  while (Date.now() < deadline) {
    let state;
    try { state = await cdp.value(snapshot); } catch (error) { if (error.message === "browser_closed") throw error; await sleep(400); continue; }
    if (state.origin === origin && !state.challenge && state.uploadButton) {
      baseline = new Set(state.links);
      // Cookie preferences are independent of upload consent, given in the app.
      await cdp.value("document.querySelector('#onetrust-reject-all-handler')?.click(); true");
      await cdp.call("Page.enable");
      await cdp.call("Page.setInterceptFileChooserDialog", {enabled:true});
      const chooser = cdp.once("Page.fileChooserOpened");
      // Handle native chooser creation before waiting for the click to return.
      const click = cdp.value(`(() => { if(location.origin !== ${JSON.stringify(origin)}) throw new Error('wrong_origin'); const b=[...document.querySelectorAll('button')].find(e=>e.getClientRects().length && /^Upload video(?: for free)?$/i.test(e.innerText.replace(/\\s+/g,' ').trim())); if(!b) throw new Error('missing'); b.click(); return true; })()`);
      const clickOutcome = click.catch(() => false);
      const chosen = await chooser;
      if (!chosen.backendNodeId) throw new Error("upload_control_missing");
      const {frameTree} = await cdp.call("Page.getFrameTree");
      if (chosen.frameId !== frameTree.frame.id || await cdp.value("location.origin") !== origin) throw new Error("upload_control_missing");
      // Exactly one submission. Never resubmit when processing is ambiguous.
      await cdp.call("DOM.setFileInputFiles", { backendNodeId: chosen.backendNodeId, files: [file] });
      await clickOutcome;
      progress("uploading");
      break;
    }
    progress("attention");
    await sleep(500);
  }
  if (!baseline) throw new Error("upload_page_timeout");
  let link;
  while (Date.now() < deadline) {
    let state;
    try { state = await cdp.value(snapshot); } catch (error) { if (error.message === "browser_closed") throw error; await sleep(400); continue; }
    if (state.origin !== origin || state.challenge) { progress("attention"); await sleep(500); continue; }
    if (state.error) throw new Error("website_upload_failed");
    link = state.links.map(value => shareLink(value, origin)).find(value => value && !baseline.has(value));
    if (link) break;
    progress(state.processing ? "processing" : "uploading", state.percent);
    await sleep(500);
  }
  if (!link) throw new Error("upload_timeout");
  emit({ kind: "candidate", url: link });
  progress("verifying");
  // Leave the uploading page alive. Navigating it away can interrupt processing
  // or stall on a before-unload dialog while the website still owns the upload.
  const verifier = await cdp.newPage(link);
  try {
  let lastReload = Date.now();
  while (Date.now() < deadline) {
    try {
      const media = await verifier.value(`(() => {
        if (location.origin !== ${JSON.stringify(origin)} || location.pathname !== ${JSON.stringify(new URL(link).pathname)}) return null;
        const play = [...document.querySelectorAll('button')].find(e => e.getClientRects().length && /^Play(?:\\b|\\s|\\()/i.test(e.getAttribute('aria-label') || e.innerText));
        const v = document.querySelector('video');
        if (!v) { play?.click(); return null; }
        if (v.paused) v.play().catch(() => {});
        return {ready:v.readyState, duration:v.duration, width:v.videoWidth, time:v.currentTime, error:v.error?.code || null};
      })()`);
      if (media && !media.error && media.ready >= 2 && media.duration > 0 && media.width > 0 && media.time > 0.05) {
        emit({ kind: "complete", url: link });
        return link;
      }
      if (media?.error) throw new Error("playback_failed");
    } catch (error) { if (["playback_failed", "browser_closed"].includes(error.message)) throw error; }
    if (Date.now() - lastReload > 30000) { verifier.call("Page.reload").catch(() => {}); lastReload = Date.now(); }
    await sleep(500);
  }
  throw new Error("playback_verification_timeout");
  } finally { verifier.close(); }
}

if (import.meta.main) {
  let cdp;
  try {
    const [endpoint, file] = Deno.args;
    const local = new URL(endpoint);
    if (local.protocol !== "http:" || local.hostname !== "127.0.0.1" || !local.port || !file) throw new Error("invalid_arguments");
    let page;
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      const pages = await (await fetch(`${local.origin}/json/list`)).json();
      page = pages.find(p => p.type === "page" && p.url.startsWith("https://streamable.com/"));
      if (page) break;
      await sleep(300);
    }
    if (!page) throw new Error("upload_page_missing");
    const socket = new URL(page.webSocketDebuggerUrl);
    if (socket.hostname !== "127.0.0.1" && socket.hostname !== "localhost") throw new Error("invalid_browser_endpoint");
    socket.hostname = "127.0.0.1";
    if (socket.port !== local.port) throw new Error("invalid_browser_endpoint");
    cdp = await Cdp.connect(socket.href);
    await automate(cdp, file, value => console.log(JSON.stringify(value)));
  } catch (error) {
    // Never print page text, tokens, account information, or raw browser errors.
    const known = ["upload_control_missing", "upload_page_timeout", "website_upload_failed", "upload_timeout", "playback_failed", "playback_verification_timeout", "upload_page_missing", "browser_closed", "browser_timeout"];
    const commands = ["Runtime.evaluate", "DOM.setFileInputFiles", "Page.navigate", "Page.reload"];
    console.log(JSON.stringify({ kind: "failed", stage: error.message === "browser_timeout" && commands.includes(error.command) ? `browser_timeout:${error.command}` : known.includes(error.message) ? error.message : "browser_failure" }));
    Deno.exitCode = 1;
  } finally { cdp?.close(); }
}
