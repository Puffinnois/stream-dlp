// Embedded in the app. No browser state, cookie jar, remote modules, or generic extraction.
export function classify(value) {
  const u = new URL(value);
  if (u.protocol !== "https:" || u.username || u.password || u.port || u.href.length > 8192)
    throw new Error("unsafe destination");
  const main = ["tiktok.com", "www.tiktok.com", "m.tiktok.com"].includes(u.hostname);
  const direct = main && /^\/(?:@[\w.-]+|share)\/video\/\d{1,20}\/?$/.test(u.pathname);
  const mobile = main && /^\/v\/\d{1,20}\.html\/?$/.test(u.pathname);
  const short = (main && /^\/t\/[\w-]{1,128}\/?$/.test(u.pathname)) ||
    (["vm.tiktok.com", "vt.tiktok.com"].includes(u.hostname) && /^\/[\w-]{1,128}\/?$/.test(u.pathname));
  if (!direct && !mobile && !short) throw new Error("unsupported destination");
  u.hash = "";
  if (direct || mobile) {
    if (mobile) u.pathname = `/share/video/${u.pathname.match(/\d+/)[0]}`;
    u.hostname = "www.tiktok.com";
  }
  return { url: u.href, direct: direct || mobile };
}

export async function resolve(value, request = fetch, signal = AbortSignal.timeout(15000)) {
  let current = classify(value);
  const visited = new Set();
  // Match yt-dlp's public short-link resolver; TikTok otherwise redirects some
  // share URLs to its homepage instead of the video.
  const headers = { "User-Agent": "facebookexternalhit/1.1" };
  for (let hop = 0; hop <= 5; hop++) {
    signal.throwIfAborted();
    if (current.direct) return current.url;
    if (hop === 5 || visited.has(current.url)) throw new Error("redirect limit");
    visited.add(current.url);
    let response = await request(current.url, { method: "HEAD", headers, redirect: "manual", credentials: "omit", signal });
    if (response.status === 405) {
      await response.body?.cancel();
      response = await request(current.url, { method: "GET", headers, redirect: "manual", credentials: "omit", signal });
    }
    const location = response.headers.get("location");
    await response.body?.cancel();
    if (![301, 302, 303, 307, 308].includes(response.status) || !location)
      throw new Error("no video redirect");
    // Validate before making another request. Never follow arbitrary website destinations.
    current = classify(new URL(location, current.url).href);
  }
  throw new Error("redirect limit");
}

if (import.meta.main) {
  try { console.log(await resolve(Deno.args[0])); }
  catch { console.error("Could not resolve this TikTok share link. Open it in your browser and copy the full video link."); Deno.exitCode = 1; }
}
