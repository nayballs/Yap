// Bindings, vars and secrets the Worker reads through `cloudflare:workers`.
declare namespace Cloudflare {
  interface Env {
    DB: D1Database;
    /** Public origin, e.g. https://auth.contextmirror.com ([vars]; .dev.vars locally). */
    BETTER_AUTH_URL: string;
    EMAIL_FROM: string;

    // Secrets (`wrangler secret put`). Optional ones switch features on.
    BETTER_AUTH_SECRET: string;
    RESEND_API_KEY?: string;
    GOOGLE_CLIENT_ID?: string;
    GOOGLE_CLIENT_SECRET?: string;
    GITHUB_CLIENT_ID?: string;
    GITHUB_CLIENT_SECRET?: string;
    DISCORD_CLIENT_ID?: string;
    DISCORD_CLIENT_SECRET?: string;

    /** Local testing only (.dev.vars): dev/mock-provider.mjs's address. */
    DEV_MOCK_PROVIDER?: string;
  }
}
