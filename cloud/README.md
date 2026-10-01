# yap-cloud — Yap accounts

The optional sign-in service behind Yap's **Settings → Account**. It's a
Cloudflare Worker running [Better Auth](https://better-auth.com) on D1, served
at `https://auth.contextmirror.com`. It only knows who someone is; audio,
transcripts, notes and settings never leave the user's PC.

## How sign-in works

| Method | Flow |
|---|---|
| **Email code** | Yap calls the API directly: `POST /api/auth/email-otp/send-verification-otp` → the user types the 6-digit code → `POST /api/auth/sign-in/email-otp`, which returns `{ token, user }`. |
| **Google / Microsoft / GitHub** | Yap opens the system browser (RFC 8252) at `/api/auth/electron/init-oauth-proxy?provider=…&client_id=yap-desktop&code_challenge=…&state=…`. After the provider, the server drops a short-lived `better-auth.yap-desktop` cookie and lands on `/` (`public/index.html`), which hands a one-time code back to Yap. Yap redeems it with its PKCE verifier at `POST /api/auth/electron/token` → `{ token, user }`. |

The handoff page returns the code by one of three channels:

- **Deep link** `com.contextmirror.yap://auth/callback#token=…` for installed
  builds (the installer registers the scheme).
- **Loopback** `http://127.0.0.1:<port>/callback?token=…` when Yap's `state`
  ends in `-<port>`: dev and portable builds, where the scheme isn't
  registered to the running exe. Yap then redirects the tab to `/#done`.
- **Copy and paste**: the page always shows the code too.

`init-oauth-proxy` is served by `src/index.ts`, not the plugin. The plugin's
version fetches the Worker's own URL, which on Cloudflare re-enters through
the edge from the Workers egress IP (one rate-limit bucket for everyone).

Yap keeps the session token in Windows Credential Manager and sends it as
`Authorization: Bearer <token>`. Sessions last 30 days and slide while used.

Endpoints Yap uses: `GET /api/auth/get-session`, `GET /api/auth/list-accounts`,
`POST /api/auth/sign-out`, `POST /api/auth/delete-user`, `GET /api/health`,
and `GET /api/providers` (which buttons to show: configured providers, and
whether email codes can be sent).
POSTs need a JSON body (`{}` will do). Deleting needs a session under a day
old (`SESSION_EXPIRED` otherwise: sign in again first) and emails a receipt.

## Layout

```
src/index.ts       Hono app: the desktop sign-in entry, /api/auth/* → Better Auth, /api/health
src/auth.ts        Better Auth config (email OTP, social providers, bearer, desktop handoff)
src/mail.ts        Resend sender + the Yap-styled emails
src/env.d.ts       bindings, vars and secrets
auth.cli.ts        schema-only config for `npm run schema` (not deployed)
migrations/        D1 schema (generated)
public/            static pages: handoff (/), /error, /privacy, /terms
dev/               mock-microsoft.mjs: a local stand-in provider for testing
```

## Local development

```bash
npm install
cp .dev.vars.example .dev.vars   # then set BETTER_AUTH_SECRET (openssl rand -base64 32)
npm run db:migrate:local
npm run dev                      # http://localhost:8787
npm run typecheck
```

Without `RESEND_API_KEY`, emails (and their codes) print to the `wrangler dev`
console instead of sending. Social providers switch on when their client
id/secret are set.

To test the browser handoff with no real provider, run `npm run mock:microsoft`
and enable the mock lines in `.dev.vars`; "Continue with Microsoft" then goes
through a local sign-in form (`MOCK_AUTO_APPROVE=1` skips the form).

Debug builds of Yap talk to `http://localhost:8787`; set `YAP_AUTH_URL` to
point any build elsewhere.

After changing plugins or schema options in `src/auth.ts`, mirror them in
`auth.cli.ts`, run `npm run schema`, and add the result as a new numbered
migration rather than editing an applied one.

## Deploying

One-time:

1. `npx wrangler d1 create yap-auth` and put the id in `wrangler.toml`.
2. Secrets: `npx wrangler secret put BETTER_AUTH_SECRET` (and `RESEND_API_KEY`,
   plus `GOOGLE_*`, `MICROSOFT_*`, `GITHUB_*` client ids and secrets).
3. Uncomment the `auth.contextmirror.com` route in `wrangler.toml` (the zone
   must be on Cloudflare).
4. OAuth redirect URIs to register with each provider:
   `https://auth.contextmirror.com/api/auth/callback/{google|microsoft|github}`.
   Microsoft: add the optional ID-token claims `email` and `xms_edov`
   (`src/auth.ts` only trusts emails Microsoft marks verified).
5. Resend: verify `mail.contextmirror.com` and add its DNS records.

Then: `npm run db:migrate:remote && npm run deploy`.
