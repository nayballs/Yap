// Yap accounts: the Better Auth instance, one per isolate.
import { env, waitUntil } from "cloudflare:workers";
import { betterAuth } from "better-auth";
import { bearer, deviceAuthorization, emailOTP } from "better-auth/plugins";
import { electron } from "@better-auth/electron";
import { accountDeletedEmail, otpEmail, sendMail } from "./mail";

/** Client id Yap sends with browser sign-ins (src-tauri/src/auth.rs). The
 *  electron plugin binds the handoff code to it and names the handoff cookie
 *  `better-auth.yap-desktop` after it (public/index.html reads that cookie).
 *  Phone sign-in codes are issued to it alone. */
export const DESKTOP_CLIENT_ID = "yap-desktop";

const DAY = 60 * 60 * 24;

/** RFC 8628 §6.1's 20 consonants: no vowels (no words spelled by chance), no
 *  digits (no keyboard switch on a phone), nothing to mistake for 0/O or 1/I. */
const PHONE_CODE_LETTERS = "BCDFGHJKLMNPQRSTVWXZ";

/** The code people check between Yap and their phone, and type there if the
 *  camera fails: 8 letters, shown as WDJB-MJHT. 20^8 ≈ 2^34.6 codes, a few
 *  live at once for 10 minutes each, and 10 lookups per IP per 10 minutes
 *  (rateLimit below): guessing one is hopeless. */
function phoneUserCode(): string {
  let code = "";
  while (code.length < 8) {
    for (const byte of crypto.getRandomValues(new Uint8Array(16))) {
      // 240 = 12 × 20: dropping the rest keeps every letter equally likely.
      if (byte < 240 && code.length < 8) code += PHONE_CODE_LETTERS[byte % 20];
    }
  }
  return code;
}

const isLocalDev = new URL(env.BETTER_AUTH_URL).hostname === "localhost";

// Each provider switches on only once its id and secret are both set, so local dev
// runs with email codes alone. `select_account` lets people with several
// accounts pick one instead of being signed in silently (Discord has no
// account picker: it shows its authorise screen the first time).
const socialProviders = {
  ...(env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET && {
    google: {
      clientId: env.GOOGLE_CLIENT_ID,
      clientSecret: env.GOOGLE_CLIENT_SECRET,
      prompt: "select_account" as const,
    },
  }),
  ...(env.GITHUB_CLIENT_ID && env.GITHUB_CLIENT_SECRET && {
    github: {
      clientId: env.GITHUB_CLIENT_ID,
      clientSecret: env.GITHUB_CLIENT_SECRET,
      prompt: "select_account" as const,
    },
  }),
  // Scopes identify + email (Better Auth's default): name, avatar, email and
  // whether Discord has verified that email (see databaseHooks below).
  ...(env.DISCORD_CLIENT_ID && env.DISCORD_CLIENT_SECRET && {
    discord: {
      clientId: env.DISCORD_CLIENT_ID,
      clientSecret: env.DISCORD_CLIENT_SECRET,
    },
  }),
  // Local tests only: dev/mock-provider.mjs plays a sign-in provider so the
  // whole browser → Yap hand-back runs offline. It borrows Better Auth's
  // Microsoft provider, the one built-in provider whose endpoints can be
  // pointed elsewhere; Yap doesn't offer Microsoft (the app lists only
  // google/github/discord, so this never shows as a button).
  ...(isLocalDev && env.DEV_MOCK_PROVIDER && {
    microsoft: {
      clientId: "mock",
      clientSecret: "mock",
      tenantId: "common",
      authority: env.DEV_MOCK_PROVIDER,
      disableDefaultScope: true,
      scope: ["openid", "profile", "email"],
      disableProfilePhoto: true,
      mapProfileToUser: (claims: Record<string, unknown>) => (claims.xms_edov === true ? { emailVerified: true } : {}),
    },
  }),
};

/** Sign-in methods the app may offer: the configured providers + email codes
 *  (only once mail can actually be sent). Served at /api/providers. */
export const signInMethods = () => ({
  providers: Object.keys(socialProviders),
  email: Boolean(env.RESEND_API_KEY) || isLocalDev,
});

const createAuth = () => betterAuth({
  appName: "Yap",
  telemetry: { enabled: false }, // off by default too; stated so it stays off
  // Set explicitly: Workers have no NODE_ENV, and outside "production" Better
  // Auth would silently fall back to a default secret and disable rate limits.
  baseURL: env.BETTER_AUTH_URL,
  secret: env.BETTER_AUTH_SECRET,
  database: env.DB, // raw D1 binding; Better Auth's built-in D1 driver
  socialProviders,
  // Desktop sessions: 30 days, sliding (renewed at most once a day while used).
  session: { expiresIn: 30 * DAY, updateAge: DAY },
  account: {
    // Same verified email across Google / GitHub / Discord / email code = one
    // account. Better Auth links only when the provider vouches for the email
    // and the existing account's email is verified too.
    accountLinking: { enabled: true },
    // Yap only needs identity, but providers hand back tokens regardless.
    encryptOAuthTokens: true,
  },
  databaseHooks: {
    user: {
      create: {
        // A Yap account always starts from a proven email: email codes prove
        // it, Google and GitHub vouch for theirs, but Discord can hand over an
        // address its user never verified. Refusing those stops anyone from
        // claiming an address that isn't theirs (the sign-in then ends on
        // /error?error=unable_to_create_user, which says what to do).
        before: async (user) => (user.emailVerified ? undefined : false),
      },
    },
  },
  user: {
    // Deleted from inside Yap. Better Auth refuses unless the session is
    // under a day old (session.freshAge), so an older sign-in has to sign in
    // again first: "sudo mode", since a confirmation link would need a
    // browser session that desktop users don't have.
    deleteUser: {
      enabled: true,
      afterDelete: async (user) => {
        waitUntil(sendMail(user.email, accountDeletedEmail()));
      },
    },
  },
  rateLimit: {
    enabled: true,
    storage: "database", // the default in-memory limiter is per isolate (useless on Workers)
    // Per IP. Code checks match the per-code attempt budget (allowedAttempts
    // below), so a few typos never lock someone out before the code does;
    // sends stay at the plugin's 3 a minute.
    customRules: {
      "/sign-in/email-otp": { window: 60, max: 5 },
      // Phone sign-in. Each new code is a row, so a few a minute. Looking a
      // code up (the phone page checks it, then claims it to approve) is what
      // a guesser would hammer: the plugin allows 5 per code lifetime; 10
      // leaves room for a reload and a second try, or a few people behind
      // one carrier IP, and guessing stays hopeless (see phoneUserCode).
      "/device/code": { window: 60, max: 5 },
      "/device": { window: 600, max: 10 },
      "/device/approve": { window: 60, max: 5 },
      "/device/deny": { window: 60, max: 5 },
    },
  },
  // Only what Yap calls stays reachable (src-tauri/src/auth.rs: get-session,
  // list-accounts, sign-out, delete-user, email codes, the desktop handoff;
  // index.ts calls sign-in/social). Off, with a reason each:
  disabledPaths: [
    // Passwords and profile edits: Yap has neither.
    "/sign-up/email",
    "/sign-in/email",
    "/change-password",
    "/verify-password",
    "/request-password-reset",
    "/reset-password",
    "/change-email",
    "/send-verification-email",
    "/verify-email",
    "/update-user",
    "/update-session",
    // Provider tokens: Yap only needs identity, and these hand a session
    // holder the user's Google/GitHub access token.
    "/get-access-token",
    "/refresh-token",
    "/account-info",
    // Linking is automatic (same verified email); by hand it would let
    // anyone holding a session attach their own provider account.
    "/link-social",
    "/unlink-account",
    // Deletion happens in Yap, never from an email link.
    "/delete-user/callback",
    // index.ts serves the desktop entry itself; transfer-user would let a
    // browser session mint a new desktop sign-in code.
    "/electron/init-oauth-proxy",
    "/electron/transfer-user",
    // The email plugin's other code flows: each one more way to make the
    // service send mail. (Sign-in codes go through index.ts's limits.)
    "/email-otp/check-verification-otp",
    "/email-otp/verify-email",
    "/email-otp/request-password-reset",
    "/forget-password/email-otp",
    "/email-otp/reset-password",
    "/email-otp/request-email-change",
    "/email-otp/change-email",
  ],
  onAPIError: { errorURL: "/error" },
  advanced: {
    ipAddress: { ipAddressHeaders: ["cf-connecting-ip"] },
    backgroundTasks: { handler: waitUntil },
    // No runtime schema check. Better Auth runs it once per instance and has
    // every concurrent request await that same promise; on Workers a promise
    // whose request is cancelled never settles, so one cancelled first
    // request left every later /api/auth call on that isolate hanging (seen
    // 2026-10-04: get-session timing out on ~40% of calls). The schema comes
    // from migrations/ anyway.
    database: { validateSchema: false },
  },
  plugins: [
    emailOTP({
      otpLength: 6,
      expiresIn: 600, // 10 minutes
      allowedAttempts: 5,
      storeOTP: "hashed",
      async sendVerificationOTP({ email, otp }) {
        // Not awaited: response timing must not reveal whether mail was sent.
        waitUntil(sendMail(email, otpEmail(otp)));
      },
    }),
    bearer(), // Yap sends `Authorization: Bearer <session token>`
    electron({ clientID: DESKTOP_CLIENT_ID }), // PKCE browser → app handoff
    // Sign in with your phone (OAuth 2.0 device authorization, RFC 8628):
    // Yap gets a code at /device/code and shows it as a QR code for
    // /device?user_code=… (public/device.html). The person signs in there
    // and approves; Yap polls /device/token, which then answers with a
    // session token (`access_token`) for Bearer use like any other. Paths
    // in use, so never in disabledPaths: Yap's device/code + device/token;
    // the page's sign-in/social, email codes, device, device/approve|deny
    // and sign-out.
    deviceAuthorization({
      verificationUri: "/device",
      expiresIn: "10m",
      interval: "5s",
      generateUserCode: phoneUserCode,
      // Codes are issued to (and redeemed by) Yap alone.
      validateClient: (clientId) => clientId === DESKTOP_CLIENT_ID,
    }),
  ],
});

// One Better Auth instance per isolate keeps warm requests at a few ms of
// CPU (building one costs 30-70 ms; the free plan allows 10 per request).
// But on Workers a cancelled request (client gone) stops mid-await, and the
// shared instance can be left with work that never settles: in production a
// burst of cancelled requests left later /api/auth calls on those isolates
// hanging, both mid-setup and after it (2026-10-04). Hence:
//  - an instance is shared only once it has served a request end to end;
//  - each request on it is marked running until it finishes. A cancelled one
//    never unmarks itself, so a mark older than SUSPECT_MS means "possibly
//    stuck": the next request builds a fresh instance instead of waiting;
//  - backstop: a call pending after STUCK_MS answers 503 and retires it.
interface Instance {
  auth: ReturnType<typeof createAuth>;
  running: Set<{ since: number }>;
}
let shared: Instance | undefined;

/** Normal calls take well under a second. A slow but healthy one (a provider
 *  callback can take 1-2 s) only costs the next request a fresh instance. */
const SUSPECT_MS = 1500;
/** Yap waits 20 s; answer well before that. */
const STUCK_MS = 8000;

/** Runs a request through Better Auth (see above). A stuck GET (get-session,
 *  list-accounts) changes nothing, so it gets one more try on a fresh
 *  instance; anything else answers 503. */
export async function handleAuth(request: Request, retry = true): Promise<Response> {
  const now = Date.now();
  if (shared && [...shared.running].some((mark) => now - mark.since > SUSPECT_MS)) {
    console.warn("[auth] a request on the shared instance never finished; starting a fresh one");
    shared = undefined;
  }
  const instance: Instance = shared ?? { auth: createAuth(), running: new Set() };
  const mark = { since: now };
  instance.running.add(mark);
  const again = retry && (request.method === "GET" || request.method === "HEAD") ? request.clone() : null;

  let timer: ReturnType<typeof setTimeout> | null = null;
  const stuck = new Promise<"stuck">((resolve) => {
    timer = setTimeout(() => resolve("stuck"), STUCK_MS);
  });
  try {
    const result = await Promise.race([instance.auth.handler(request), stuck]);
    if (result === "stuck") {
      if (shared === instance) shared = undefined;
      console.error(`[auth] ${new URL(request.url).pathname} stuck for ${STUCK_MS} ms; instance retired`);
      if (again) return await handleAuth(again, false);
      return Response.json(
        { code: "SERVICE_UNAVAILABLE", message: "The account service was busy. Try again." },
        { status: 503, headers: { "Retry-After": "1" } },
      );
    }
    shared ??= instance;
    return result;
  } finally {
    clearTimeout(timer);
    instance.running.delete(mark);
  }
}
