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
| **Phone (QR code)** | Yap gets a code at `POST /api/auth/device/code` and shows `/device?user_code=…` as a QR code. On the phone, `/device` (`public/device.html`) offers every method above, shows who asked for the code, then asks to approve it; Yap polls `POST /api/auth/device/token`, which answers `{ access_token }` (a session token) once approved. See below. |

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

1. Yap: `POST /api/auth/device/code` with exactly `{ "client_id":
   "yap-desktop" }` as JSON → `device_code` (Yap's polling secret),
   `user_code` (8 letters from RFC 8628's 20 consonants, shown as
   `WDJB-MJHT`), `verification_uri` (`/device`), `verification_uri_complete`
   (`/device?user_code=…`, drawn as a QR code inside Yap), `expires_in` 600
   and `interval` 3. Anything else in the body (`scope`, `user_id`, which
   would pre-bind the code to an account, form encoding) gets 400
   `invalid_request`; another client id gets `invalid_client`. The client id
   is public, so it identifies the app rather than authenticating it. Each
   new code records where it came from (`deviceOrigin`, below).
2. Phone: `/device` (`public/device.html` + `device.js`; opened by typing
   the address, it asks for the code). It looks the code up without cookies,
   `GET /api/auth/device?user_code=…`, then offers every method
   `/api/providers` lists: providers through `POST /api/auth/sign-in/social`
   (`callbackURL` and `errorCallbackURL` both `/device?user_code=…`), email
   through the usual two code calls. Signed in, it shows the account, the
   code to compare with the PC's screen and who asked for it
   (`GET /api/account/device-origin`: "Requested 2 min ago by Yap 0.1.1 on
   Windows, near Leeds, United Kingdom", then a green "same network as this
   phone", an amber "different network" note, or a red "another country"
   warning; RFC 8628 §5.4), and warns off links someone else sent. Approve
   and Deny carry equal weight.
3. Approve or Deny: a signed-in `GET /device?user_code=…` claims the code
   for that account (only the claimer may decide, so the page claims only
   then, and "Not you?" can still switch accounts), then
   `POST /api/auth/device/approve` or `/deny` `{ "userCode" }`. Either way it
   then signs the browser out, like the hand-back page: only Yap gets a
   session (leaving the page mid-way signs out too, on `pagehide`). An
   approval emails the account "New sign-in to your Yap account": the app
   and version, the PC's rough location, the time and how it was approved,
   with a "Review your devices" button to `/security` (`public/security.html`:
   how to sign a device out, plus an "Open Yap" button, the app's
   `com.contextmirror.yap://account` deep link to Settings → Account).
4. Yap polls `POST /api/auth/device/token` `{ "grant_type":
   "urn:ietf:params:oauth:grant-type:device_code", "device_code",
   "client_id" }` every 3 s: 400 `authorization_pending` (or `slow_down`:
   +5 s) until approved, then `{ access_token, token_type: "Bearer",
   expires_in }`. The access token is a plain session token, made on Yap's
   request (the session row has Yap's user agent) and used as
   `Authorization: Bearer` like the others; Yap reads the profile from
   `get-session`. `access_denied`, `expired_token` and `invalid_grant` (spent
   or unknown) end the attempt.
5. For its first 24 hours a session made this way can't delete the account
   or sign other devices out: `POST /api/auth/delete-user`,
   `POST /api/auth/revoke-other-sessions` and
   `POST /api/account/sessions/revoke` answer 403
   `{ "code": "NEW_PHONE_SESSION", "message": "For your security, a sign-in
   made with your phone can't do this during its first day. Sign in another
   way to carry on." }` (Telegram does the same). Someone tricked into
   approving a stranger's code can still sign that session out from any
   other sign-in. `/device/token`'s answer marks the session (`phoneSession`:
   a keyed hash of the token, never the token) before Yap receives it; the
   check resolves the caller's session through `get-session`, so a token
   sent signed, raw or as a cookie is caught alike.

`GET /api/account/device-origin?user_code=…` needs the page's signed-in
browser session (cookie) and a code that's still pending and not claimed by
another account, and answers
`{ label, place, minutesAgo, sameNetwork, sameCountry }` (`Cache-Control:
no-store`): `label` is "Yap 0.1.1 on Windows" when the asking user agent
is Yap's own `Yap/<version> (Windows)`, else "Yap"; `place` is "near
<city>, <country>", "in <country>" or `""`; the booleans compare the phone's
request with the PC's (`null` when either side is unknown). 401 without a
session, 404 when no sign-in waits for that code, 400 for a malformed code,
429 over 20 look-ups per IP per 10 minutes.

What `deviceOrigin` keeps, per code: Cloudflare's country and city for the
asking IP, its user agent (first 200 characters), and `network`, a keyed
hash (HMAC with `BETTER_AUTH_SECRET`) of its IPv4 /24 or IPv6 /64, so the
phone can tell "same network" without anyone storing an IP.

Per-IP limits (`rateLimit.customRules`): `/device/code` 5 a minute,
`/device` lookups 10 per 10 minutes (the plugin's own is 5 per code
lifetime), approve and deny 5 a minute each; `device-origin` 20 per 10
minutes (`src/throttle.ts`, counted in `emailCodeSend` under its own key
prefix). The plugin deletes a code once Yap redeems it, or polls it after a
deny or expiry. A sweep (`src/phone.ts`) runs after every new code and on a
Cron Trigger every 30 minutes (`wrangler.toml` `[triggers]`): codes that
expired over an hour ago, `deviceOrigin` rows after 70 minutes,
`phoneSession` marks after 2 days, send/look-up counts after a day, and
browser sessions left behind. Those are sessions whose user agent starts
`Mozilla/` and that are over 30 minutes old: the only browser sessions this
service makes are the hand-back page's and the phone page's, which sign out
when they finish (Yap's own carry `Yap/…`), so one still there was
abandoned mid-way. A future web page that needs to stay signed in would
have to change this.

At 3 s a poll costs about two D1 writes (the code's `lastPolledAt` and the
rate-limit count): a sign-in that takes 30 s is ~20 writes; a code left on
screen for its whole 10 minutes, ~400.

Tables: `deviceCode` (`migrations/0003_device_authorization.sql`),
`deviceOrigin` and `phoneSession` (`migrations/0004_device_origin.sql`).

Yap keeps the session token in Windows Credential Manager and sends it as
`Authorization: Bearer <token>`. Sessions last 30 days and slide while used.

Endpoints Yap uses: `GET /api/auth/get-session`, `GET /api/auth/list-accounts`,
`POST /api/auth/sign-out`, `POST /api/auth/delete-user`, `GET /api/health`,
`GET /api/providers` (which buttons to show: configured providers, and
whether email codes can be sent), and for Settings → Account's "Where you're
signed in": `GET /api/account/sessions` (Bearer; this service's own route:
`{ sessions: [{ id, current, createdAt, updatedAt, expiresAt, userAgent }] }`,
no tokens or IPs), `POST /api/account/sessions/revoke` (`{ "id": … }`, one
other session; 400 `CURRENT_SESSION` for the caller's own) and
`POST /api/auth/revoke-other-sessions`. Better Auth's own `/list-sessions`
is disabled: it answers only sessions under a day old and returns every
session's token.
POSTs need a JSON body (`{}` will do). Deleting needs a session under a day
old (`SESSION_EXPIRED` otherwise: sign in again first) and emails a receipt.
A phone sign-in's session gets 403 `NEW_PHONE_SESSION` from delete-user and
both revoke calls for its first 24 hours (see above).

Every POST under `/api/auth/` and `/api/account/` must declare a
`Content-Length` of at most 4096 bytes (413 `PAYLOAD_TOO_LARGE` otherwise,
chunked bodies included): every real body is a few hundred bytes, and
Better Auth keeps some of what it's given. `init-oauth-proxy` takes a
`state` of up to 128 characters from `A-Z a-z 0-9 . _ ~ -` and an S256
`code_challenge` (43 base64url characters); anything else lands on
`/error?error=invalid_request`.

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
src/index.ts       Hono app: the desktop sign-in entry, /api/auth/* → Better Auth, /api/account/*, the cron
src/auth.ts        Better Auth config (email OTP, social providers, bearer, desktop handoff, device codes)
src/phone.ts       phone sign-in records: where a code came from, phone sessions, the sweep
src/mail.ts        Resend sender + the emails (one layout: logo, headline, account chip, details, button)
src/throttle.ts    limits on emailed sign-in codes (per address, per IP) and origin look-ups
src/env.d.ts       bindings, vars and secrets
auth.cli.ts        schema-only config for `npm run schema` (not deployed)
migrations/        D1 schema (generated)
public/            static pages: handoff (/), phone sign-in (/device), /security, /error, /privacy, /terms;
                   email/yap-logo.png is the emails' logo (Gmail drops SVG)
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

**End-to-end checks** (`dev/e2e-phone.mjs`, ~100 checks: size caps, every
sign-in route, the full phone flow, "who's asking", the approval email,
day-one limits, the cleanup cron). Start the mock on 8852 and wrangler dev on
8851, then run `npm run test:e2e` (or `npm run test:e2e -- <section>`):

```bash
PORT=8852 MOCK_AUTO_APPROVE=1 npm run mock:provider
npx wrangler dev --port 8851 --local-upstream localhost:8851 --test-scheduled --var BETTER_AUTH_URL:http://localhost:8851 --var DEV_MOCK_PROVIDER:http://127.0.0.1:8852
npm run test:e2e
```

Wrangler doesn't flush the Worker's console to a redirected log promptly;
the script reads emailed codes from its local observability API instead
(`/cdn-cgi/local/explorer/api/local/observability/query`).

To test phone sign-in without the app, play Yap with curl:
`POST /api/auth/device/code` with `{"client_id":"yap-desktop"}`, open the
returned `verification_uri_complete` in a browser on the PC (a phone can't
reach its `localhost`), sign in there (the stand-in provider, or an email
code from the console) and approve. Then `POST /api/auth/device/token` with
`{"grant_type":"urn:ietf:params:oauth:grant-type:device_code","device_code":"…","client_id":"yap-desktop"}`
returns the `access_token`; `GET /api/auth/get-session` with
`Authorization: Bearer <access_token>` shows the account. Polls closer than
3 s apart answer `slow_down`. The approval email prints to the console like
the codes. Locally the phone page's origin line shows wrangler's idea of
your location, and "same network" (both requests come from this PC).

To run the cron's sweep by hand, start `wrangler dev --test-scheduled` and
open `http://localhost:8787/__scheduled?cron=*/30+*+*+*+*`.

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
deploy (phone sign-in needs `0003_device_authorization.sql` there first, and
this version needs `0004_device_origin.sql`: without its tables, deleting an
account and signing other devices out fail with a 500). Deploying also
registers the Cron Trigger in `wrangler.toml`.

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
