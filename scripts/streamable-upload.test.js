import { automate, Cdp, shareLink, sleep } from "./streamable-upload.js";

function assert(value, message = "assertion failed") { if (!value) throw new Error(message); }
Deno.test("share links reject credentials, lookalikes, queries and non-video paths", () => {
  assert(shareLink("https://streamable.com/abc123"));
  for (const value of ["https://streamable.com.evil.test/abc123", "http://streamable.com/abc123", "https://user@streamable.com/abc123", "https://streamable.com/abc123?secret=1", "https://streamable.com/videos/abc123"]) assert(!shareLink(value));
});
Deno.test("allocation alone never produces complete, and file submission is not retried", async () => {
  let submissions = 0;
  let navigated = false;
  const events = [];
  const fake = {
    async value(expression) {
      if (expression === "location.origin") return "https://streamable.com";
      if (expression.includes("reject-all")) return true;
      if (expression.includes("document.querySelector('video')")) return null;
      return { origin: "https://streamable.com", uploadButton: true, links: submissions ? ["https://streamable.com/abc123"] : [], challenge: false, percent: null };
    },
    async call(method) {
      if (method === "Page.getFrameTree") return {frameTree:{frame:{id:"main"}}};
      if (method === "Runtime.evaluate") return { result: { objectId: "file-input" } };
      if (method === "DOM.setFileInputFiles") submissions++;
      if (method === "Page.navigate") navigated = true;
      return {};
    },
    once() { return {backendNodeId: 1, frameId:"main"}; },
    async newPage() { navigated = true; return this; },
    close() {},
  };
  let failed = false;
  try { await automate(fake, "fixture.mp4", value => events.push(value), "https://streamable.com", 50); } catch { failed = true; }
  assert(failed && navigated && submissions === 1);
  assert(events.some(value => value.kind === "candidate"));
  assert(!events.some(value => value.kind === "complete"));
});

Deno.test({
  name: "real Chromium selects the file, uploads exact bytes, ignores old links, and verifies playable media",
  ignore: Deno.args.length < 2,
  sanitizeOps: false, sanitizeResources: false,
  async fn() {
    const [browser, sample] = Deno.args;
    const source = await Deno.readFile(sample);
    const work = await Deno.makeTempDir({ dir: "test-output", prefix: "streamable-fixture-" });
    const profile = `${await Deno.realPath(work)}/profile`;
    let uploaded;
    let uploadedName;
    let count = 0;
    const server = Deno.serve({ hostname: "127.0.0.1", port: 0, onListen() {} }, async request => {
      const path = new URL(request.url).pathname;
      if (path === "/clip.mp4") return new Response(source, { headers: { "content-type": "video/mp4" } });
      if (path === "/upload" && request.method === "POST") {
        count++;
        const data = await request.formData();
        const file = data.get("file");
        uploadedName = file.name;
        uploaded = new Uint8Array(await file.arrayBuffer());
        return new Response("ok");
      }
      const html = path === "/new123" ? '<video muted autoplay playsinline src="/clip.mp4"></video>' : `
        <input type="file" id="inactive"><!-- deliberate inactive input -->
        <div class="video-creator"><input type="file" id="active"><button onclick="document.querySelector('#active').click()">Upload video for free</button></div>
        <a id="video-url-input" href="/old123">old upload</a>
        <div id="status"></div>
        <script>
          document.querySelector('#active').addEventListener('change', async event => {
            const body = new FormData(); body.set('file', event.target.files[0]);
            document.querySelector('#status').innerText = 'Uploading';
            await fetch('/upload', {method:'POST', body});
            document.querySelector('#status').innerText = 'Processing';
            setTimeout(() => {const link=document.createElement('a');link.id='video-url-input';link.href='/new123';link.innerText='new video';document.body.append(link);},200);
          });
        </script>`;
      return new Response(html, { headers: { "content-type": "text/html" } });
    });
    const origin = `http://127.0.0.1:${server.addr.port}`;
    const process = new Deno.Command(browser, {
      args: ["--headless=new", "--inprivate", "--disable-sync", "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", origin],
      stdout: "null", stderr: "null",
    }).spawn();
    let cdp;
    let browserSocket;
    try {
      let port;
      const deadline = Date.now() + 20000;
      while (!port && Date.now() < deadline) {
        try { [port, browserSocket] = (await Deno.readTextFile(`${profile}/DevToolsActivePort`)).split(/\r?\n/); } catch { await sleep(100); }
      }
      assert(port, "browser did not expose its dedicated test endpoint");
      let page;
      while (!page && Date.now() < deadline) {
        page = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(value => value.type === "page" && value.url.startsWith(origin));
        if (!page) await sleep(100);
      }
      assert(page, "fixture page not found");
      cdp = await Cdp.connect(page.webSocketDebuggerUrl);
      const events = [];
      const result = await automate(cdp, await Deno.realPath(sample), value => events.push(value), origin, 20000);
      assert(result === `${origin}/new123`, "old or unverified link returned");
      assert(count === 1, "file was submitted more than once");
      assert(uploadedName === sample.split(/[\\/]/).at(-1), "filename changed");
      assert(uploaded?.length === source.length && uploaded.every((byte, i) => byte === source[i]), "uploaded bytes differ from original");
      assert(events.at(-1).kind === "complete", "playback was never verified");
      const browserCdp = await Cdp.connect(`ws://127.0.0.1:${port}${browserSocket}`);
      await browserCdp.call("Browser.close").catch(() => {});
      browserCdp.close();
    } finally {
      cdp?.close();
      try { process.kill(); } catch { /* already closed */ }
      await process.status;
      await server.shutdown();
      // Browser shutdown can finish asynchronously on Windows.
      for (let attempt = 0; attempt < 10; attempt++) {
        try { await Deno.remove(work, { recursive: true }); break; } catch { await sleep(200); }
      }
      try { await Deno.stat(work); throw new Error("temporary browser profile was not removed"); }
      catch (error) { if (!(error instanceof Deno.errors.NotFound)) throw error; }
    }
  },
});
