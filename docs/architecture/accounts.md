# Accounts

Optional Yap accounts: sign-in routes (email codes, Google/GitHub/Discord, your phone), the session in Credential Manager, where you're signed in, and Settings → Account. The server is `cloud/` (see `cloud/README.md`); the phone flow is [`docs/qr-sign-in.md`](../qr-sign-in.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`auth.rs`** — Yap accounts (optional; nothing in dictation depends on it).
  Email codes: `auth_email_send`/`auth_email_verify` call the account service
  directly. Google/GitHub/Discord: `auth_start` opens the system browser on
  the server's `/api/auth/electron/init-oauth-proxy` with a PKCE challenge +
  `state`; the account page hands back a one-time code by **deep link**
  (installed builds — `is_registered` must point at *this* exe), a one-shot
  **loopback** listener (dev/portable — the port rides at the end of `state`
  as `-<port>`), or **paste** (`auth_submit_code`); `redeem` swaps it for a
  session at `/electron/token`. **Phone** (RFC 8628 device authorization):
  `auth_device_start` gets a code from `/device/code` (client `yap-desktop`),
  checks both links stay on the service's origin, draws the QR **locally**
  (`qrcode` crate, encoder only → one SVG path; `Status.device` carries it +
  the grouped code `WDJB-MJHT` + expiry + `expired`) and polls `/device/token`
  at the server's interval (`slow_down` adds 5 s; one attempt at a time — a
  newer one or `auth_device_cancel` retires the poller; the device code never
  leaves Rust). A **lapsed** code (`expired_token`, or Yap's deadline passing
  while the service still answers) is no dead end: polling stops and the view
  stays with `expired: true`, **no** `yap-auth-error`; the page asks for the
  next code with `auth_device_start({renew: true})`, which does nothing once
  the attempt was cancelled or signed in. Approval answers with an ordinary
  session token → `get-session` → `finish_sign_in`, the attempt marked
  approved meanwhile so no renewal can replace (and discard) it. Denied /
  code gone / unreachable → `yap-auth-error`; approved after a cancel, or a
  session `get-session` can't put a name to → that session is revoked.
  `finish_sign_in` (every route) drops any other sign-in left waiting
  (browser or phone), emits signed-in at once and fills in the linked
  providers after. Every route's token (+ a profile copy) is stored in
  Windows Credential Manager (`yap-account.com.yap.dictation`, **Local**
  persistence — never config.json, logs or the webview; any non-production
  service URL gets its own entry, `yap-account@<host>`, so a dev build can't
  touch the real session). Signed in, startup + every 24 h
  `refresh` re-validates via `get-session` (server says no → signed out;
  unreachable → keep, flagged `offline`). `auth_sign_out` revokes best-effort
  then forgets locally; `auth_delete_account` returns `"reauth"` when the
  server wants a fresh sign-in (sessions > 1 day old). A phone sign-in under
  a day old can't delete the account or sign devices out (403
  `NEW_PHONE_SESSION` → a friendly "sign in another way" error). A fresh sign-in that
  replaces this PC's session (that re-auth) revokes the old one server-side,
  so it can't linger as a phantom device. **Where you're signed in**:
  `auth_list_sessions` (the service's own `GET /api/account/sessions` →
  `{id, current, label, createdAt, lastActiveAt, expiresAt}`, unix secs, this
  PC first; `label` parsed from the session's user agent — "Yap 0.1.1 on
  Windows", "Chrome on macOS"), `auth_revoke_session(id)` (`POST
  /api/account/sessions/revoke`, refuses this PC) and
  `auth_revoke_other_sessions` (Better Auth's). The service's endpoints work
  for a session of any age and send no tokens or IPs (Better Auth's own
  `/list-sessions` wanted a sign-in < 1 day old and handed out every
  session's token; it's disabled). A 401 from these re-checks via
  `get-session` and signs out only if the session is really gone. Emits
  `yap-auth-changed` (status snapshot) and `yap-auth-error`. The Account page
  only offers what the service reports at `/api/providers` (configured
  providers; email once mail can be sent) — `auth_check_methods` when the page
  opens, plus daily while signed in. ⚠ A **signed-out Yap never contacts the
  account service on its own** (the privacy promise in the README, the
  release notes and SignPath's policy) — keep any new auth call behind a
  sign-in or a user action. Debug builds talk
  to `http://localhost:8787` (`cloud/`'s `wrangler dev`); `YAP_AUTH_URL`
  overrides any build.
- **Nothing about the person syncs (yet).** Settings → General → About you
  (`about_you.rs`: the region, a name, a note for the AI) stays in
  config.json on this PC; signing in doesn't send it anywhere. Syncing it
  between a person's PCs could come with accounts later — behind the
  sign-in, never from a signed-out Yap.

## Settings → Account

- **`lib/AccountSection.svelte` / `account.svelte.js`** — Settings → Account:
  Continue with Google/GitHub/Discord, "Email me a code" → 6-digit entry
  (auto-submits), a waiting state with paste-the-code fallback, then the
  profile (avatar or initials, linked providers), sign-out, a **"Where you're
  signed in"** group (one row per session: label + amber "This device" chip,
  "Signed in <date> · last active <day>" — day granularity, the server only
  renews a session about daily — per-row Sign out for the others, and "Sign
  out of other devices" with a confirm step; loads whenever a session shows,
  for a session of any age), and delete-account (with an in-place "Confirm
  it's you" re-sign-in when the server wants a session < 1 day old; it only
  deletes if the same account signs back in). **Sign in with your phone**
  (under the provider buttons; not in "Confirm it's you", which a phone
  sign-in can't satisfy) swaps the options for the QR code from
  `status.device` (its SVG path at 5 px per module, `forced-color-adjust:
  none`; the code in large monospace, spelled out for screen readers; the
  "Can't scan?" typed-address fallback; an `aria-live` "Waiting for your
  phone…" with a quiet "New code in m:ss"; Cancel). A lapsed code (clock at
  0 or `device.expired`) is blurred under **"Show a new code"** (WhatsApp's
  reload-QR pattern); while the panel is really in view (window visible —
  asked of the window on every focus change, `isVisible` && !`isMinimized`,
  since WebView2 reports the page visible while the window is hidden — +
  IntersectionObserver, as the Settings modal hides with `display: none` but
  stays mounted) it renews itself at most 3 times per start by the person
  (every poll costs the service D1 writes), then waits for the button. Focus
  goes to the panel heading on start and back to the phone button (signed
  in: the profile heading) when the panel closes. It stays until the phone
  approves — then it flips to signed in like any sign-in — or the attempt
  fails (toast says why). `account.svelte.js` is the
  shared runes store (`auth_status` + `yap-auth-changed`) that also drives the
  "signed in as" buttons at the bottom of both sidebars (ControlPanel +
  Settings) and toasts background sign-in results.
