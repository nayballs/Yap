// Yap accounts: the Better Auth instance, one per isolate.
import { env, waitUntil } from "cloudflare:workers";
import { betterAuth } from "better-auth";
import { bearer, emailOTP } from "better-auth/plugins";
import { electron } from "@better-auth/electron";
import { accountDeletedEmail, otpEmail, sendMail } from "./mail";

/** Client id Yap sends with browser sign-ins (src-tauri/src/auth.rs). The
 *  electron plugin binds the handoff code to it and names the handoff cookie
 *  `better-auth.yap-desktop` after it (public/index.html reads that cookie). */
export const DESKTOP_CLIENT_ID = "yap-desktop";

const DAY = 60 * 60 * 24;

const isLocalDev = new URL(env.BETTER_AUTH_URL).hostname === "localhost";

/** Microsoft passes work/school emails through unverified (the "nOAuth"
 *  problem), so trust an address only when its `xms_edov` claim (an optional
 *  claim enabled on the app registration) says the domain owner verified it;
 *  personal Microsoft accounts always carry it as true. */
const microsoftEmailVerified = (claims: Record<string, unknown>) =>
  [true, 1, "1", "true"].includes(claims.xms_edov as never);

// Each provider switches on only once its keys are configured, so local dev
// runs with email codes alone. `select_account` lets people with several
// accounts (personal + work) pick one instead of being signed in silently.
const socialProviders = {
  ...(env.GOOGLE_CLIENT_ID && {
    google: {
      clientId: env.GOOGLE_CLIENT_ID,
      clientSecret: env.GOOGLE_CLIENT_SECRET,
      prompt: "select_account" as const,
    },
  }),
  ...(env.MICROSOFT_CLIENT_ID && {
    microsoft: {
      clientId: env.MICROSOFT_CLIENT_ID,
      clientSecret: env.MICROSOFT_CLIENT_SECRET,
      tenantId: "common", // personal + work/school accounts
      prompt: "select_account" as const,
      // Identity only: drops the default User.Read + offline_access consent.
      disableDefaultScope: true,
      scope: ["openid", "profile", "email"],
      disableProfilePhoto: true,
      mapProfileToUser: (claims: Record<string, unknown>) =>
        microsoftEmailVerified(claims) ? { emailVerified: true } : {},
      // Local testing only: dev/mock-microsoft.mjs stands in for Microsoft.
      ...(isLocalDev && env.DEV_MICROSOFT_AUTHORITY && { authority: env.DEV_MICROSOFT_AUTHORITY }),
    },
  }),
  ...(env.GITHUB_CLIENT_ID && {
    github: {
      clientId: env.GITHUB_CLIENT_ID,
      clientSecret: env.GITHUB_CLIENT_SECRET,
      prompt: "select_account" as const,
    },
  }),
};

const createAuth = () => betterAuth({
  appName: "Yap",
  // Set explicitly: Workers have no NODE_ENV, and outside "production" Better
  // Auth would silently fall back to a default secret and disable rate limits.
  baseURL: env.BETTER_AUTH_URL,
  secret: env.BETTER_AUTH_SECRET,
  database: env.DB, // raw D1 binding; Better Auth's built-in D1 driver
  socialProviders,
  // Desktop sessions: 30 days, sliding (renewed at most once a day while used).
  session: { expiresIn: 30 * DAY, updateAge: DAY },
  account: {
    // Same verified email across Google / Microsoft / GitHub / email code = one
    // account. Better Auth links only when the provider vouches for the email
    // and the existing account's email is verified too; an email-code sign-in
    // strips any access an unverified account picked up before the proof.
    accountLinking: { enabled: true },
    // Yap only needs identity, but providers hand back tokens regardless.
    encryptOAuthTokens: true,
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
    customRules: { "/sign-in/email-otp": { window: 60, max: 5 } },
  },
  onAPIError: { errorURL: "/error" },
  advanced: {
    ipAddress: { ipAddressHeaders: ["cf-connecting-ip"] },
    backgroundTasks: { handler: waitUntil },
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
  ],
});

let instance: ReturnType<typeof createAuth> | undefined;

/** Built on first use rather than at module scope: Better Auth checks the D1
 *  schema as it starts, and Workers forbid I/O outside a request, so a
 *  module-scope instance logs a failed check on every cold start. Building it
 *  inside the first request lets the check run; later requests reuse it. */
export const getAuth = () => (instance ??= createAuth());
