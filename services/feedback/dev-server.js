// The Worker on this machine with an in-memory database, to try the app against:
//   tools/deno run --no-config --no-remote --allow-read=services/feedback --allow-net=127.0.0.1 --allow-env=FEEDBACK_ADMIN_TOKEN services/feedback/dev-server.js
// then start a debug build with STREAM_DLP_FEEDBACK_URL=http://127.0.0.1:8787
import { handle } from "./worker.js";
import { memoryDatabase } from "./sqlite-d1.js";

const schema = await Deno.readTextFile(new URL("./schema.sql", import.meta.url));
const env = {
  DB: memoryDatabase(schema),
  SENDER_KEY: "local-only",
  ADMIN_TOKEN: Deno.env.get("FEEDBACK_ADMIN_TOKEN") ?? "local-admin-token-local-admin-token",
};

Deno.serve({ hostname: "127.0.0.1", port: 8787 }, (request) => {
  // Cloudflare sets this header in production.
  const headers = new Headers(request.headers);
  headers.set("cf-connecting-ip", "127.0.0.1");
  return handle(new Request(request, { headers }), env);
});
