// Yap accounts Worker. Static pages in ./public are served by the assets
// layer before this runs; only API paths reach the Hono app.
import { env } from "cloudflare:workers";
import { Hono } from "hono";
import { secureHeaders } from "hono/secure-headers";
import { DESKTOP_CLIENT_ID, getAuth, signInMethods } from "./auth";
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

// Better Auth: sign-in (social, email code), sessions (cookie or Bearer),
// the desktop handoff (/electron/*), sign-out and account deletion.
app.all("/api/auth/*", (c) => getAuth().handler(c.req.raw));

app.get("/api/health", (c) => c.json({ ok: true }));

// Which sign-in buttons Yap should show (providers come and go with config).
app.get("/api/providers", (c) => c.json(signInMethods()));

export default app;
