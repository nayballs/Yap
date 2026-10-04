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

Email codes are limited beyond Better Auth's 3 sends a minute per IP: one
address gets a code every 30 s at most and 10 a day, one IP 20 a day
(`src/throttle.ts`, table `emailCodeSend`: keyed hashes, kept a day). Over the
limit, the send answers 429 `TOO_MANY_REQUESTS` with `Retry-After` and
doesn't touch the code already sent. Only `type: "sign-in"` codes can be
requested, and the email plugin's other flows (password reset, email change,
email verification) are switched off with `disabledPaths`.

## Layout

```
src/index.ts       Hono app: the desktop sign-in entry, /api/auth/* → Better Auth, /api/health
src/auth.ts        Better Auth config (email OTP, social providers, bearer, desktop handoff)
src/mail.ts        Resend sender + the Yap-styled emails
src/throttle.ts    limits on emailed sign-in codes (per address, per IP)
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

Live at `https://auth.contextmirror.com` (Cloudflare account
"Nayballs@googlemail.com's Account"): Worker `yap-auth` on its custom domain
only, D1 `yap-auth` (WEUR). Client ids are public and live in `wrangler.toml`
`[vars]`; the secrets are set with `npx wrangler secret put <NAME>`:
`BETTER_AUTH_SECRET`, `GOOGLE_CLIENT_SECRET`, `GITHUB_CLIENT_SECRET`,
`RESEND_API_KEY`. (Paste with right-click in Windows Terminal: Ctrl+V can send
a stray control character into the hidden prompt.)

- Google: Cloud project `yap-accounts` → Google Auth Platform (published),
  web client "Yap account service". Branding (name + logo) is verified, so
  the account chooser says "continue to Yap". That rests on the Search
  Console ownership TXT at `contextmirror.com` (`google-site-verification=…`):
  keep it, and re-verify branding after changing the logo, name or links.
- GitHub: OAuth app "Yap" (github.com/settings/applications/3897989), logo
  `docs/brand/yap-logo-512.png` on badge colour `#29251d`.
- Resend: domain `mail.contextmirror.com` (eu-west-1, DNS via Cloudflare).

Ship a change: `npm run typecheck && npm run deploy`. A schema change gets a
new numbered migration, applied with `npm run db:migrate:remote` before the
deploy.

### Adding Microsoft sign-in

Needs a Microsoft Entra ID tenant (a free Azure account comes with one).

1. entra.microsoft.com → App registrations → New registration: name **Yap**;
   supported accounts **any Entra ID tenant + personal Microsoft accounts**;
   redirect URI (Web)
   `https://auth.contextmirror.com/api/auth/callback/microsoft`.
2. Token configuration → Add optional claim → ID token: `email`, `xms_edov`
   (`src/auth.ts` only trusts emails Microsoft marks verified).
3. Certificates & secrets → New client secret (longest expiry; note the
   renewal date) → copy the **Value**.
4. Branding & properties: homepage `https://contextmirror.com/yap`, terms
   `https://auth.contextmirror.com/terms`, privacy
   `https://auth.contextmirror.com/privacy`, logo `docs/brand/yap-logo-512.png`.
5. Add the Application (client) ID to `wrangler.toml` as `MICROSOFT_CLIENT_ID`,
   run `npx wrangler secret put MICROSOFT_CLIENT_SECRET`, then `npm run deploy`.
   `/api/providers` then lists `microsoft` and Yap shows the button, with no
   app update.
