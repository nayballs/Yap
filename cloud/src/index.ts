// Yap accounts Worker. Static pages in ./public are served by the assets
// layer before this runs; only API paths reach the Hono app.
import { env, waitUntil } from "cloudflare:workers";
import { Hono, type MiddlewareHandler } from "hono";
import { secureHeaders } from "hono/secure-headers";
import { DESKTOP_CLIENT_ID, handleAuth, signInMethods } from "./auth";
import { newSignInEmail, sendMail } from "./mail";
import {
  describeOrigin,
  isNewPhoneSession,
  normalizeUserCode,
  originPlace,
  recordOrigin,
  recordPhoneSession,
  sweep,
} from "./phone";
import { recordSend, secondsUntilNextSend, sendKeys, takeLookup } from "./throttle";

const app = new Hono();

// API responses get the usual hardening headers (nosniff, HSTS, no framing,
// no referrer). The static pages get theirs from public/_headers.
app.use("*", secureHeaders({ xFrameOptions: "DENY" }));

// Request bodies. Everything Yap and the pages send is a few hundred bytes
// of JSON, but Better Auth keeps some of what it's handed in D1 (a phone
// code's scope, a sign-in's callback URLs), so a bigger body is refused
// before it gets that far, and so is one of unknown length (chunked). The
// request init-oauth-proxy builds goes straight to handleAuth, not through
// here.
const MAX_BODY = 4096;
const limitBody: MiddlewareHandler = async (c, next) => {
  if (c.req.method === "POST") {
    const length = c.req.header("content-length");
    if (!length || !/^\d{1,9}$/.test(length) || Number(length) > MAX_BODY) {
      return c.json({ code: "PAYLOAD_TOO_LARGE", message: "That request is too large." }, 413);
    }
  }
  await next();
};
app.use("/api/auth/*", limitBody);
app.use("/api/account/*", limitBody);

// What Yap sends (src-tauri/src/auth.rs): `state` is a 16-letter nonce, plus
// "-<port>" for a loopback hand-back; `code_challenge` is an S256 PKCE
// challenge, a SHA-256 in base64url (43 characters). The desktop plugin
// keeps both until the sign-in finishes, so nothing else gets in.
const STATE = /^[A-Za-z0-9._~-]{1,128}$/;
const S256_CHALLENGE = /^[A-Za-z0-9_-]{43}$/;

// Where Yap's browser sign-in starts (src-tauri/src/auth.rs). This replaces
// @better-auth/electron's own /electron/init-oauth-proxy, which re-enters the
// Worker over HTTP (a fetch to its own base URL). On Cloudflare that hop goes
// back out through the edge and arrives from the Workers egress IP, so every
// user's sign-in would land in one rate-limit bucket. It costs a round trip
// too. The same request goes straight to the auth handler instead, with the
// caller's own headers (and IP).
app.get("/api/auth/electron/init-oauth-proxy", async (c) => {
  const { provider, state, code_challenge } = c.req.query();
  if (!provider || !state || !code_challenge || !STATE.test(state) || !S256_CHALLENGE.test(code_challenge)) {
    return c.redirect("/error?error=invalid_request");
  }

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

/** A signed-in caller, as Better Auth's own get-session sees it. */
interface Caller {
  id: string;
  userId: string;
  email: string;
  /** The session token: only ever hashed (src/phone.ts), never logged. */
  token: string;
}

/** The session a request carries, via get-session: its Bearer token (Yap),
 *  its cookie (the pages), or whichever Better Auth would use of the two.
 *  The caller's IP goes along so get-session's rate limit counts per caller
 *  (without one, Better Auth puts every such call in one shared bucket). */
async function sessionFrom(req: Request, via: { bearer?: boolean; cookie?: boolean }): Promise<Caller | null> {
  const headers = new Headers();
  const authorization = via.bearer ? req.headers.get("authorization") : null;
  const cookie = via.cookie ? req.headers.get("cookie") : null;
  if (!authorization && !cookie) return null;
  if (authorization) headers.set("authorization", authorization);
  if (cookie) headers.set("cookie", cookie);
  const ip = req.headers.get("cf-connecting-ip");
  if (ip) headers.set("cf-connecting-ip", ip);
  const res = await handleAuth(new Request(new URL("/api/auth/get-session", env.BETTER_AUTH_URL), { headers }));
  if (!res.ok) return null;
  const data = (await res.json().catch(() => null)) as {
    session?: { id?: string; userId?: string; token?: string };
    user?: { email?: string };
  } | null;
  const { id, userId, token } = data?.session ?? {};
  return id && userId && token ? { id, userId, token, email: data?.user?.email ?? "" } : null;
}

/** Yap's session (Bearer token). */
const callerSession = (req: Request) => sessionFrom(req, { bearer: true });

const notSignedIn = { code: "UNAUTHORIZED", message: "Not signed in." };

// ---- Sign in with your phone (device authorization, src/auth.ts) ----

// Codes go to Yap alone, which asks with exactly `{ "client_id":
// "yap-desktop" }` in JSON. The plugin would also take `scope` and `user_id`
// and keep them on the code (`user_id` even binds the code to an account
// before anyone has signed in), so anything else gets RFC 8628's
// invalid_request; the plugin answers invalid_client for any other client
// id. A client id is public: it names the app, it doesn't prove who's asking.
//
// A new code also records where it came from for the phone page (src/
// phone.ts) and sweeps what's done with: codes, origins and marks past their
// time, and abandoned browser sessions.
app.post("/api/auth/device/code", async (c, next) => {
  const isJson = c.req.header("content-type")?.toLowerCase().startsWith("application/json") ?? false;
  const body: unknown = isJson ? await c.req.raw.clone().json().catch(() => null) : null;
  const fields = body !== null && typeof body === "object" && !Array.isArray(body) ? Object.keys(body) : [];
  if (fields.length !== 1 || fields[0] !== "client_id" || typeof (body as { client_id?: unknown }).client_id !== "string") {
    return c.json({ error: "invalid_request", error_description: "Expected a JSON body with client_id only." }, 400);
  }
  await next();
  if (c.res.status !== 200) return;
  const data = (await c.res.clone().json().catch(() => null)) as { user_code?: unknown } | null;
  const userCode = typeof data?.user_code === "string" ? data.user_code : null;
  waitUntil(
    Promise.all([
      userCode && recordOrigin(userCode, c.req.raw).catch((e) => console.error("[device] couldn't record a code's origin", e)),
      sweep().catch((e) => console.error("[device] sweep failed", e)),
    ]),
  );
});

// Approved on a phone: the account gets an email saying so, in case it
// wasn't them. The approving session is the phone page's own, which signs
// out as soon as this answers, hence looked up before answering.
app.post("/api/auth/device/approve", async (c, next) => {
  const body = (await c.req.raw.clone().json().catch(() => null)) as { userCode?: unknown } | null;
  await next();
  if (c.res.status !== 200 || typeof body?.userCode !== "string") return;
  const me = await sessionFrom(c.req.raw, { bearer: true, cookie: true });
  if (!me?.email) return;
  const userCode = body.userCode;
  const timeZone = (c.req.raw.cf as IncomingRequestCfProperties | undefined)?.timezone; // the phone's
  const at = new Date();
  waitUntil(
    originPlace(userCode)
      .catch(() => "")
      .then((place) => sendMail(me.email, newSignInEmail(place, at, timeZone)))
      .catch((e) => console.error("[device] couldn't send the new sign-in email", e)),
  );
});

// Yap's sign-in by phone came through: its session is marked (a keyed hash
// of the token, src/phone.ts) before Yap gets the token, so that for a day
// it can't delete the account or sign other devices out (below). Someone
// tricked into approving a stranger's code keeps a way back.
app.post("/api/auth/device/token", async (c, next) => {
  await next();
  if (c.res.status !== 200) return;
  const data = (await c.res.clone().json().catch(() => null)) as { access_token?: unknown } | null;
  if (typeof data?.access_token !== "string" || !data.access_token) return;
  await recordPhoneSession(data.access_token).catch((e) =>
    console.error("[device] couldn't mark a phone sign-in's session", e),
  );
});

const newPhoneSession = {
  code: "NEW_PHONE_SESSION",
  message:
    "For your security, a sign-in made with your phone can't do this during its first day. Sign in another way to carry on.",
};

/** Turns away a session that a phone sign-in made under a day ago. */
const notNewPhoneSession: MiddlewareHandler = async (c, next) => {
  const me = await sessionFrom(c.req.raw, { bearer: true, cookie: true });
  if (me && (await isNewPhoneSession(me.token))) return c.json(newPhoneSession, 403);
  await next();
};
app.post("/api/auth/delete-user", notNewPhoneSession);
app.post("/api/auth/revoke-other-sessions", notNewPhoneSession);

// Better Auth: sign-in (social, email code), sessions (cookie or Bearer),
// the desktop handoff (/electron/*), sign-out and account deletion.
app.all("/api/auth/*", (c) => handleAuth(c.req.raw));

app.get("/api/health", (c) => c.json({ ok: true }));

// Which sign-in buttons Yap should show (providers come and go with config).
app.get("/api/providers", (c) => c.json(signInMethods()));

// The phone page's "Requested 2 min ago by Yap 0.1.1 on Windows, near
// Leeds, United Kingdom" (public/device.js), plus whether the phone is on
// the same network, or in the same country, as the PC that asked. Only for
// a signed-in browser (the page signs in first) and a code still pending
// that no other account has claimed, and limited per IP. No IPs in or out:
// places are Cloudflare's guess, networks keyed hashes.
app.get("/api/account/device-origin", async (c) => {
  c.header("Cache-Control", "no-store");
  const userCode = normalizeUserCode(c.req.query("user_code") ?? "");
  if (userCode.length !== 8) return c.json({ code: "INVALID_REQUEST", message: "Which code?" }, 400);
  const wait = await takeLookup(c.req.header("cf-connecting-ip"));
  if (wait > 0) {
    return c.json({ code: "TOO_MANY_REQUESTS", message: "Too many requests. Try again later." }, 429, {
      "Retry-After": String(wait),
    });
  }
  const me = await sessionFrom(c.req.raw, { cookie: true });
  if (!me) return c.json(notSignedIn, 401);
  const origin = await describeOrigin(userCode, c.req.raw, me.userId);
  if (!origin) return c.json({ code: "NOT_FOUND", message: "No sign-in is waiting for that code." }, 404);
  return c.json(origin);
});

// Settings → Account's "Where you're signed in". Better Auth's /list-sessions
// answers only sessions signed in within a day (freshAge, so most Yap
// sessions couldn't list) and hands back every session's token; these list
// ids (no tokens, no IPs) for any signed-in session and sign one out by id.

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
  if (await isNewPhoneSession(me.token)) return c.json(newPhoneSession, 403);
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

export default {
  fetch: app.fetch,
  // wrangler.toml [triggers]: every 30 minutes, whether or not anyone signs in.
  scheduled: (_controller, _env, ctx) => {
    ctx.waitUntil(sweep().catch((e) => console.error("[cron] sweep failed", e)));
  },
} satisfies ExportedHandler<Cloudflare.Env>;
