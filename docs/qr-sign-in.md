# Sign in with your phone (QR code)

Settings → Account → **Sign in with your phone** shows a QR code and an 8-letter
code. The phone's camera opens `https://auth.contextmirror.com/device?user_code=…`;
the person signs in there with any method the service offers (Google, GitHub,
Discord, email code), checks who is asking, and taps **Approve**. Yap, polling in
the background, is then signed in.

It is the OAuth 2.0 Device Authorization Grant ([RFC 8628]) through Better Auth's
`deviceAuthorization` plugin. Code: `cloud/src/auth.ts` (plugin config, limits),
`cloud/src/index.ts` (wrappers around the plugin's routes), `cloud/public/device.*`
(phone page), `src-tauri/src/auth.rs` (`auth_device_*`, polling, QR drawing),
`src/lib/AccountSection.svelte` (the panel).

## Why this design (review of 2026-10-04)

Three independent reviews (industry landscape, security, UX/engineering) looked
at the first version:

- **Pattern.** Without a Yap mobile app, a QR code for an RFC 8628 code approved
  in the phone's browser is the industry standard (YouTube TV, GitHub CLI,
  Microsoft). Discord, Steam, WhatsApp, Telegram and Signal are faster only
  because their own signed-in app scans the code, and all of them have been
  mass-phished anyway. The stronger pattern, passkeys over the phone's
  Bluetooth-checked "hybrid" link, is what [RFC 10027] (cross-device security
  BCP, Aug 2026) recommends, but it needs a passkey the person already has, so it
  complements this flow rather than replacing it (see "Later").
- **Threat.** Device-code phishing is real and current ([Storm-2372], 2025; an
  AI-assisted campaign in April 2026): an attacker sends a victim a code or link
  for *the attacker's* device and gets a session when the victim approves.
  RFC 8628 §5.4 says the approval screen SHOULD show information about the
  device; RFC 10027 §6.2.1.5 says to use the device grant only with extra
  mitigations. Yap's browser sign-in stays the primary route; this is the
  convenience path.

## What protects it

- Codes: 8 letters from RFC 8628's no-vowel alphabet (~2^34.6), grouped
  `WDJB-MJHT`, 10-minute life, one use, unique; per-IP limits on issuing,
  looking up, approving and denying; the plugin allows few lookups per code.
- The phone page looks a code up **without** cookies; only Approve/Deny claim it,
  for the account that taps it. "Not you?" switches account without claiming.
- **Who's asking**: the approval card shows the requesting app, when, and an
  approximate place, plus a same-network check (green) or a different-country
  warning (red) — kept ~70 minutes, network stored only as a keyed hash.
- **New-sign-in email** to the account whenever a phone approves a sign-in.
- **No trust on day one**: a session created by phone sign-in can't delete the
  account or sign out other devices for its first 24 hours (Telegram's rule).
- The phone's browser session ends after Approve/Deny (and on leaving the page);
  any leftover browser session is deleted after 30 minutes by a cron.
- Requests are size-capped and `/device/code` accepts only `{ client_id }`, so the
  database can't be filled through it.
- Desktop: the polling secret never leaves Rust or reaches logs; the QR link must
  be on the service's own origin before it's drawn (locally, no QR service); an
  approval that arrives after Cancel is revoked; the session token lives in
  Windows Credential Manager.
- Phone page: strict CSP (own scripts only), server text via `textContent`,
  same-origin referrer (the code never leaks to Google/GitHub/Discord).

## Tie-off checklist

Done when every box is ticked.

- [x] Server pass merged and deployed 2026-10-05 (`f024b1b`): 413 over 4 KB,
      `/device/code` takes only `{ client_id }`, `state`/`code_challenge` format
      checks, "who's asking" (`GET /api/account/device-origin`, table
      `deviceOrigin`), approval email, day-one limits (403 `NEW_PHONE_SESSION`,
      table `phoneSession`), the 30-minute sweep cron, 3 s polling, phone-page
      layout/focus/copy. Re-verified on the merged tree with the end-to-end
      script (all sections); production smoke-tested.
- [x] Desktop pass merged (`a44efd9`): renewable codes (up to 3 automatic
      renewals while the panel is on screen, then "Show a new code"; no toast on
      a plain expiry; a renewal never discards an approval), focus/aria/contrast
      fixes, the leaked-session fix, the `NEW_PHONE_SESSION` message, the phone
      option hidden in "Confirm it's you", sign-in announced before the
      provider lookup.
- [ ] Shipped in a nightly and tried on the installed app.
- [x] Migration `0004` applied to production (2026-10-05).
- [ ] Manual tests on a real phone (iPhone Safari and/or Android Chrome):
  - [ ] Google already signed in on the phone (≈4 taps from scan).
  - [ ] No sessions on the phone; email-code path; code typed at `/device`.
  - [ ] Approval card shows app, time and place; same-network check appears.
  - [ ] Deny; let it expire (renews by itself, then "Show a new code").
  - [ ] Cancel on the PC, then approve on the phone (PC stays signed out).
  - [ ] New-sign-in email arrives after Approve.
  - [ ] Phone-made session: "Delete account" / "Sign out other devices" refused
        with the day-one message; works from a browser/email sign-in.
- [ ] CLAUDE.md, `cloud/README.md` and the privacy policy describe the shipped
      behaviour.

## Later (deliberately not now)

- **Before accounts hold paid plans or synced data:** when the phone and PC are
  on different networks, have the phone show a 6-digit number that must be typed
  into Yap before `/device/token` hands over the session (RFC 10027 §6.1.16;
  Apple's "Allow, then type the code"). Also require an emailed code to delete an
  account.
- **Passkeys** (Better Auth `@better-auth/passkey`, SimpleWebAuthn supports
  Workers) on the phone page and the system-browser sign-in page — not in the
  Tauri webview (its origin can't use contextmirror.com passkeys; WebView2
  passkey support is unreliable).
- A "scanned — finish on your phone" state; withdrawing a code on Cancel or
  renewal; phone-page dark mode; an alarm on spikes of wrong-code lookups.
- Only with a Yap mobile app: app-scans-QR behind biometrics, or push approval
  with number matching.

[RFC 8628]: https://www.rfc-editor.org/rfc/rfc8628
[RFC 10027]: https://www.rfc-editor.org/rfc/rfc10027
[Storm-2372]: https://www.microsoft.com/en-us/security/blog/2025/02/13/storm-2372-conducts-device-code-phishing-campaign/
