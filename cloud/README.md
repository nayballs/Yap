# yap-cloud — Yap accounts

The optional sign-in service behind Yap's **Settings → Account**. It's a
Cloudflare Worker running [Better Auth](https://better-auth.com) on D1, served
at `https://auth.contextmirror.com`. It only knows who someone is; audio,
transcripts, notes and settings never leave the user's PC.

## How sign-in works

| Method | Flow |
|---|---|
| **Email code** | Yap calls the API directly: `POST /api/auth/email-otp/send-verification-otp` → the user types the 6-digit code → `POST /api/auth/sign-in/email-otp`, which returns `{ token, user }`. |
| **Google / GitHub / Discord** | Yap opens the system browser (RFC 8252) at `/api/auth/electron/init-oauth-proxy?provider=…&client_id=yap-desktop&code_challenge=…&state=…`. After the provider, the server drops a short-lived `better-auth.yap-desktop` cookie and lands on `/` (`public/index.html`), which hands a one-time code back to Yap. Yap redeems it with its PKCE verifier at `POST /api/auth/electron/token` → `{ token, user }`. |
| **Phone (QR code)** | Yap gets a code at `POST /api/auth/device/code` and shows `/device?user_code=…` as a QR code. On the phone, `/device` (`public/device.html`) offers every method above, then asks to approve the code; Yap polls `POST /api/auth/device/token`, which answers `{ access_token }` (a session token) once approved. See below. |

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

**Sign in with your phone** is OAuth 2.0 device authorization (RFC 8628,
Better Auth's `deviceAuthorization` plugin): any sign-in method, done on the
phone (its password manager, 2FA, mail app).

1. Yap: `POST /api/auth/device/code` `{ "client_id": "yap-desktop" }` (no
   other client gets one) → `device_code` (Yap's polling secret), `user_code`
   (8 letters from RFC 8628's 20 consonants, shown as `WDJB-MJHT`),
   `verification_uri` (`/device`), `verification_uri_complete`
   (`/device?user_code=…`, drawn as a QR code inside Yap), `expires_in` 600
   and `interval` 5.
2. Phone: `/device` (`public/device.html` + `device.js`; opened by typing
   the address, it asks for the code). It looks the code up without cookies,
   `GET /api/auth/device?user_code=…`, then offers every method
   `/api/providers` lists: providers through `POST /api/auth/sign-in/social`
   (`callbackURL` and `errorCallbackURL` both `/device?user_code=…`), email
   through the usual two code calls. Signed in, it shows the account and the
   code to compare with the PC's screen, says what approving does, and warns
   off links someone else sent.
3. Approve or Deny: a signed-in `GET /device?user_code=…` claims the code
   for that account (only the claimer may decide, so the page claims only
   then, and "Not you?" can still switch accounts), then
   `POST /api/auth/device/approve` or `/deny` `{ "userCode" }`. Either way it
   then signs the browser out, like the hand-back page: only Yap gets a
   session.
4. Yap polls `POST /api/auth/device/token` `{ "grant_type":
   "urn:ietf:params:oauth:grant-type:device_code", "device_code",
   "client_id" }`: 400 `authorization_pending` (or `slow_down`: +5 s) until
   approved, then `{ access_token, token_type: "Bearer", expires_in }`. The
   access token is a plain session token, made on Yap's request (the session
   row has Yap's user agent) and used as `Authorization: Bearer` like the
   others; Yap reads the profile from `get-session`. `access_denied`,
   `expired_token` and `invalid_grant` (spent or unknown) end the attempt.

Per-IP limits (`rateLimit.customRules`): `/device/code` 5 a minute,
`/device` lookups 10 per 10 minutes (the plugin's own is 5 per code
lifetime), approve and deny 5 a minute each. The plugin deletes a code once
Yap redeems it, or polls it after a deny or expiry; `src/index.ts` clears
abandoned ones (expired over an hour ago) whenever a new code is issued.
Table `deviceCode`: `migrations/0003_device_authorization.sql`.

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
requested.

Everything Yap doesn't call is switched off (`disabledPaths` in
`src/auth.ts`, 404): passwords, profile edits, provider tokens
(`get-access-token`), manual account linking, `electron/transfer-user`, and
the email plugin's other flows. After a browser sign-in, the hand-back page
signs the *browser* out (Yap gets its own session when it redeems the code),
so nothing usable stays behind in the browser.

On Workers a cancelled request stops mid-await, which can leave a shared
Better Auth instance hanging every later call. `handleAuth` (`src/auth.ts`)
shares an instance only after it has served a request end to end, starts a
fresh one when a request on it never finished, and answers a call stuck for
8 s with a retry (GETs) or a 503. See its comment before changing it.

## Layout

```
src/index.ts       Hono app: the desktop sign-in entry, /api/auth/* → Better Auth, /api/health
src/auth.ts        Better Auth config (email OTP, social providers, bearer, desktop handoff)
src/mail.ts        Resend sender + the Yap-styled emails
src/throttle.ts    limits on emailed sign-in codes (per address, per IP)
src/env.d.ts       bindings, vars and secrets
auth.cli.ts        schema-only config for `npm run schema` (not deployed)
migrations/        D1 schema (generated)
public/            static pages: handoff (/), phone sign-in (/device), /error, /privacy, /terms
dev/               mock-provider.mjs: a local stand-in provider for testing
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

To test the browser handoff with no real provider, run `npm run mock:provider`
and set `DEV_MOCK_PROVIDER=http://127.0.0.1:8790` in `.dev.vars`, then open
`http://localhost:8787/api/auth/electron/init-oauth-proxy?provider=microsoft&client_id=yap-desktop&code_challenge=<S256 of a verifier>&state=<anything>`
in a browser: a local sign-in form stands in for the provider and the flow
ends on the hand-back page (`MOCK_AUTO_APPROVE=1` skips the form,
`MOCK_UNVERIFIED=1` signs in with an unverified email). The mock borrows
Better Auth's Microsoft provider (the only built-in one that can point at
another server); Yap doesn't offer Microsoft, so the app shows no button for
it. Run `wrangler dev --local-upstream localhost:8787`, or cookie-carrying
POSTs fail the origin check (wrangler otherwise rewrites the host to the
production domain).

Debug builds of Yap talk to `http://localhost:8787`; set `YAP_AUTH_URL` to
point any build elsewhere.

To test phone sign-in without the app, play Yap with curl:
`POST /api/auth/device/code` with `{"client_id":"yap-desktop"}`, open the
returned `verification_uri_complete` in a browser on the PC (a phone can't
reach its `localhost`), sign in there (the stand-in provider, or an email
code from the console) and approve. Then `POST /api/auth/device/token` with
`{"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":"…","client_id":"yap-desktop"}`
returns the `access_token`; `GET /api/auth/get-session` with
`Authorization: Bearer <access_token>` shows the account. Polls closer than
5 s apart answer `slow_down`.

After changing plugins or schema options in `src/auth.ts`, mirror them in
`auth.cli.ts` and run `npm run schema`: it writes the whole schema to
`.wrangler/schema.sql` (gitignored). Add what's new as the next numbered
migration; never edit an applied one.

## Deploying

Live at `https://auth.contextmirror.com` (Cloudflare account
"Nayballs@googlemail.com's Account"): Worker `yap-auth` on its custom domain
only, D1 `yap-auth` (WEUR). Client ids are public and live in `wrangler.toml`
`[vars]`; the secrets are set with `npx wrangler secret put <NAME>`:
`BETTER_AUTH_SECRET`, `GOOGLE_CLIENT_SECRET`, `GITHUB_CLIENT_SECRET`,
`DISCORD_CLIENT_SECRET`, `RESEND_API_KEY`. (Paste with right-click in Windows Terminal: Ctrl+V can send
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
deploy (phone sign-in needs `0003_device_authorization.sql` there first).

### Discord sign-in

Discord application "Yap" (discord.com/developers/applications):
OAuth2 → Redirects: `https://auth.contextmirror.com/api/auth/callback/discord`;
General Information: icon `docs/brand/yap-logo-512.png`, terms
`https://auth.contextmirror.com/terms`, privacy
`https://auth.contextmirror.com/privacy`. The client id is `DISCORD_CLIENT_ID`
in `wrangler.toml`; the secret is `DISCORD_CLIENT_SECRET` (OAuth2 → Reset
Secret → `npx wrangler secret put DISCORD_CLIENT_SECRET`). Scopes are
`identify email`.

Discord can hand back an email its user never verified, so new accounts are
created only for verified emails (`databaseHooks` in `src/auth.ts`); an
unverified Discord email ends on `/error?error=unable_to_create_user`, which
explains how to verify it. Microsoft sign-in was dropped (2026-10-04).
