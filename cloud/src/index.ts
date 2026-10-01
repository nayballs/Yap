// Yap accounts Worker. Static pages in ./public are served by the assets
// layer before this runs; only API paths reach the Hono app.
import { env } from "cloudflare:workers";
import { Hono } from "hono";
import { DESKTOP_CLIENT_ID, getAuth } from "./auth";

const app = new Hono();

// Where Yap's browser sign-in starts (src-tauri/src/auth.rs). This replaces
// @better-auth/electron's own /electron/init-oauth-proxy, which re-enters the
// Worker over HTTP (a fetch to its own base URL). On Cloudflare that hop goes
// back out through the edge and arrives from the Workers egress IP, so every
// user's sign-in would land in one rate-limit bucket. It costs a round trip
// too. The same request goes straight to the auth handler instead, with the
// caller's own headers (and IP).
app.get("/api/auth/electron/init-oauth-proxy", async (c) => {
  const { provider, state, code_challenge } = c.req.query();
  if (!provider || !state || !code_challenge) return c.redirect("/error?error=invalid_request");

  const target = new URL("/api/auth/sign-in/social", env.BETTER_AUTH_URL);
  target.search = new URLSearchParams({ client_id: DESKTOP_CLIENT_ID, code_challenge, state }).toString();
  const headers = new Headers(c.req.raw.headers);
  headers.set("origin", target.origin);
  headers.set("content-type", "application/json");
  headers.delete("content-length");

  const res = await getAuth().handler(
    new Request(target, { method: "POST", headers, body: JSON.stringify({ provider }) }),
  );
  const data = (await res.json().catch(() => null)) as { url?: string } | null;
  if (!res.ok || !data?.url) {
    console.warn(`[init-oauth-proxy] sign-in/social ${res.status} for provider ${provider}`);
    return c.redirect(`/error?error=${res.status === 429 ? "rate_limited" : "provider_unavailable"}`);
  }

  // Better Auth's state cookie + the plugin's signed transfer cookie ride along.
  const out = new Response(null, { status: 302, headers: { location: data.url } });
  for (const cookie of res.headers.getSetCookie()) out.headers.append("set-cookie", cookie);
  return out;
});

// Better Auth: sign-in (social, email code), sessions (cookie or Bearer),
// the desktop handoff (/electron/*), sign-out and account deletion.
app.all("/api/auth/*", (c) => getAuth().handler(c.req.raw));

app.get("/api/health", (c) => c.json({ ok: true }));

export default app;
