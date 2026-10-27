import { classify, resolve } from "./tiktok-resolve.js";
function assert(value) { if (!value) throw new Error("assertion failed"); }
async function rejects(fn) { let failed = false; try { await fn(); } catch { failed = true; } assert(failed); }
const full = "https://www.tiktok.com/@creator/video/123?lang=en";
Deno.test("short-link redirects are manual, anonymous, and stop at an individual video", async () => {
  let calls = 0;
  const result = await resolve("https://vm.tiktok.com/ABC/", (url, opts) => {
    calls++;
    assert(opts.redirect === "manual" && opts.credentials === "omit" && opts.method === "HEAD");
    return Promise.resolve(new Response(null, { status: 302, headers: { location: full } }));
  });
  assert(result === full && calls === 1);
});
Deno.test("unsafe and unsupported destinations never receive requests", async () => {
  for (const target of ["https://evil.test/video/123", "http://www.tiktok.com/@c/video/123", "https://www.tiktok.com.evil.test/", "https://user@www.tiktok.com/@c/video/123", "https://127.0.0.1/", "https://www.tiktok.com:444/@c/video/123", "https://www.tiktok.com/@c/photo/123", "https://www.tiktok.com/login", "file:///tmp/test"]) {
    let calls = 0;
    await rejects(() => resolve("https://vt.tiktok.com/ABC/", () => {
      calls++; return Promise.resolve(new Response(null, { status: 302, headers: { location: target } }));
    }));
    assert(calls === 1);
  }
});
Deno.test("redirect loops, limits, expired links, and cancellation terminate", async () => {
  let calls = 0;
  await rejects(() => resolve("https://vm.tiktok.com/ABC/", () => {
    calls++; return Promise.resolve(new Response(null, { status: 302, headers: { location: `/ABC${calls}/` } }));
  }));
  assert(calls === 5);
  await rejects(() => resolve("https://vm.tiktok.com/ABC/", () => Promise.resolve(new Response(null, { status: 302, headers: { location: "/ABC/" } }))));
  await rejects(() => resolve("https://vm.tiktok.com/ABC/", () => Promise.resolve(new Response(null, { status: 200 }))));
  await rejects(() => resolve("https://vm.tiktok.com/ABC/", () => { throw new Error("must not request"); }, AbortSignal.abort()));
});
Deno.test("mobile routes normalize and HEAD fallback remains bounded", async () => {
  assert(classify("https://m.tiktok.com/v/123.html?lang=en").url === "https://www.tiktok.com/share/video/123?lang=en");
  let calls = 0;
  const result = await resolve("https://www.tiktok.com/t/ABC", (_url, opts) => {
    calls++;
    assert(opts.method === (calls === 1 ? "HEAD" : "GET"));
    return Promise.resolve(new Response(null, calls === 1 ? { status: 405 } : {status: 302, headers: {location: full}}));
  });
  assert(calls === 2 && result === full);
});
