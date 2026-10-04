// Yap accounts Worker. Static pages in ./public are served by the assets
// layer before this runs; only API paths reach the Hono app.
import { env, waitUntil } from "cloudflare:workers";
import { Hono } from "hono";
import { secureHeaders } from "hono/secure-headers";
import { DESKTOP_CLIENT_ID, handleAuth, signInMethods } from "./auth";
import { recordSend, secondsUntilNextSend, sendKeys } from "./throttle";

const app = new Hono();

// API responses get the usual hardening headers (nosniff, HSTS, no framing,
// no referrer). The static pages get theirs from public/_headers.
app.use("*", secureHeaders({ xFrameOptions: "DENY" }));

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

  const res = await handleAuth(
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

// Emailed codes: sign-in codes only, within the limits in throttle.ts. This
// runs before Better Auth, because every send issues a new code: a request
// refused any later would still void the code someone is about to type.
app.post("/api/auth/email-otp/send-verification-otp", async (c, next) => {
  const body = (await c.req.raw.clone().json().catch(() => null)) as { email?: unknown; type?: unknown } | null;
  if (body?.type !== "sign-in") {
    return c.json({ code: "INVALID_OTP_TYPE", message: "Only sign-in codes can be requested." }, 400);
  }
  if (typeof body.email !== "string") return next(); // Better Auth explains what's wrong

  const keys = await sendKeys(body.email.toLowerCase(), c.req.header("cf-connecting-ip"));
  const wait = await secondsUntilNextSend(keys);
  if (wait > 0) {
    return c.json(
      { code: "TOO_MANY_REQUESTS", message: "Too many sign-in codes requested. Try again later." },
      429,
      { "Retry-After": String(wait) },
    );
  }
  await next();
  if (c.res.status === 200) await recordSend(keys);
});

// Phone sign-in codes (device authorization, src/auth.ts). Better Auth
// deletes one when Yap redeems it, or polls it after a deny or expiry; one
// nobody finished (cancelled in Yap, never scanned) would stay for good. So
// each new code clears those that expired over an hour ago: late enough that
// a straggling poll still hears "expired". Better Auth keeps D1 dates as ISO
// 8601 strings, which compare in time order.
app.post("/api/auth/device/code", async (c, next) => {
  await next();
  if (c.res.status !== 200) return;
  const cutoff = new Date(Date.now() - 60 * 60 * 1000).toISOString();
  waitUntil(
    env.DB.prepare(`DELETE FROM "deviceCode" WHERE "expiresAt" < ?1`)
      .bind(cutoff)
      .run()
      .then(() => undefined, (e) => console.error("[device] couldn't clear expired codes", e)),
  );
});

// Better Auth: sign-in (social, email code), sessions (cookie or Bearer),
// the desktop handoff (/electron/*), sign-out and account deletion.
app.all("/api/auth/*", (c) => handleAuth(c.req.raw));

app.get("/api/health", (c) => c.json({ ok: true }));

// Which sign-in buttons Yap should show (providers come and go with config).
app.get("/api/providers", (c) => c.json(signInMethods()));

// Settings → Account's "Where you're signed in". Better Auth's /list-sessions
// answers only sessions signed in within a day (freshAge, so most Yap
// sessions couldn't list) and hands back every session's token; these list
// ids (no tokens, no IPs) for any signed-in session and sign one out by id.

/** The caller's session, via Better Auth's own get-session (Bearer token). */
async function callerSession(req: Request): Promise<{ id: string; userId: string } | null> {
  const authorization = req.headers.get("authorization");
  if (!authorization) return null;
  const res = await handleAuth(
    new Request(new URL("/api/auth/get-session", env.BETTER_AUTH_URL), { headers: { authorization } }),
  );
  if (!res.ok) return null;
  const data = (await res.json().catch(() => null)) as { session?: { id?: string; userId?: string } } | null;
  const { id, userId } = data?.session ?? {};
  return id && userId ? { id, userId } : null;
}

const notSignedIn = { code: "UNAUTHORIZED", message: "Not signed in." };

app.get("/api/account/sessions", async (c) => {
  const me = await callerSession(c.req.raw);
  if (!me) return c.json(notSignedIn, 401);
  const { results } = await env.DB.prepare(
    `SELECT id, createdAt, updatedAt, expiresAt, userAgent FROM session
      WHERE userId = ?1 AND expiresAt > ?2 ORDER BY updatedAt DESC`,
  )
    .bind(me.userId, new Date().toISOString())
    .all<{ id: string; createdAt: string; updatedAt: string; expiresAt: string; userAgent: string | null }>();
  const sessions = results.map((s) => ({ ...s, userAgent: s.userAgent ?? "", current: s.id === me.id }));
  c.header("Cache-Control", "no-store");
  return c.json({ sessions });
});

app.post("/api/account/sessions/revoke", async (c) => {
  const me = await callerSession(c.req.raw);
  if (!me) return c.json(notSignedIn, 401);
  const body = (await c.req.json().catch(() => null)) as { id?: unknown } | null;
  if (typeof body?.id !== "string" || !body.id) {
    return c.json({ code: "INVALID_REQUEST", message: "Which session?" }, 400);
  }
  if (body.id === me.id) {
    return c.json({ code: "CURRENT_SESSION", message: "That's this device: sign out instead." }, 400);
  }
  // Only the caller's own sessions; an unknown id is already signed out.
  const { meta } = await env.DB.prepare(`DELETE FROM session WHERE id = ?1 AND userId = ?2`).bind(body.id, me.userId).run();
  return c.json({ success: true, removed: meta.changes > 0 });
});

export default app;
