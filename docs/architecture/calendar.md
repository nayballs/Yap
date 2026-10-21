# Calendar

Connecting Google Calendar or an iCal link, sync, parsing, the reminder card and tying recordings to meetings, plus the "Connect your calendar" dialog. The full guide is [`docs/calendar.md`](../calendar.md); the Meetings view is in [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`calendar.rs`** (+ `calendar/`) — **the calendar** (Wispr Flow's,
  local-first: no Yap server; see [`docs/calendar.md`](../calendar.md)).
  **Connections** (≤ 8; Settings → Connectors, or the "Connect your
  calendar" dialog every other Connect calendar opens): Google Calendar in one click
  (`calendar/google.rs`: OAuth for installed apps in the system browser, PKCE
  S256 + `state` via `auth.rs`'s helpers, a one-shot `tiny_http` listener on
  `127.0.0.1`, `access_type=offline`; scopes `calendar.calendarlist.readonly`
  + `calendar.events.owned.readonly` only; the client id/secret baked in from
  `YAP_GOOGLE_CALENDAR_CLIENT_ID`/`_SECRET` by `option_env!` — none = no
  Google: "Google sign-in is in the installed Yap. In this build, use a
  private iCal link…"; the token is revoked on Disconnect) or any private iCal link
  ("Outlook Calendar" = a guided publish-your-calendar ICS flow; "Other
  calendar"; HTTPS only, `webcal://` → https). Secrets (refresh token, link)
  in Credential Manager (`calendar/vault.rs`: `yap-calendar-<n>.com.yap.dictation`,
  Local persistence; test mode: `calendar-secrets.e2e.json`); `calendar.json`
  keeps connections' names, meetings from 12 h back to 7 days ahead, reminder
  answers (2 days) and the nudge. **Sync** (a tokio scheduler waking ≤ 30 s;
  a wall-clock jump = the PC slept → sync now): every 12 min, on wake, on
  Sync, after a connect; a failure keeps the old meetings and shows why;
  `invalid_grant` → "Connect again". **Parsing** (`calendar/ics.rs`, Yap's own,
  lenient: byte-wise unfolding, RFC 6868, bad lines skipped) + recurrence via
  `rrule` on wall-clock time then the event's zone (EXDATE/RDATE/
  RECURRENCE-ID overrides); **zones** (`calendar/tz.rs`): IANA (`chrono-tz`) →
  Windows names (CLDR table) → the feed's VTIMEZONE → "(UTC±hh:mm)" → floating;
  RFC 5545 gap/overlap rules. **Filters** (`calendar/model.rs`, Wispr's): a
  meeting has another invitee (not a room, not declined) or a join link; no
  all-day, nothing > 6 h, nothing cancelled or declined; Maybe = tentative,
  overlaps = Conflict. **Join links** (`calendar/links.rs`): HTTPS links of
  known services (Teams/Meet/Zoom/Webex/Slack/GoTo/Whereby/Jitsi/…) from
  conference data, location or description. **The reminder card**
  (`meeting_reminder`: "15s" default | "1min" | "2min" | "never"): "Design
  review · In 1 min", Join & take notes / Start notes / Snooze 2 min / ✕,
  until 5 min after the start, one at a time; while another meeting records,
  a **switch** card (ends it via `meeting_end::end`, then starts the next).
  It goes on screen through **`present_card`/`withdraw_card` only**, in
  **one place** (`notice.rs`): the in-app toast while the main window is
  focused (the snapshot's `card` with `inApp`), else the **Yap bar**
  (`bar::show_card`, id "calendar": a call-style card
  — the call app's mark or a calendar, the meeting over "● In 1 min · with
  Tanay +1" in amber, green once it's on, kept current with `update_card`
  every 5 s; Join & take notes | ^ Start notes, Snooze 2 min; ✕ = dismiss; no
  Esc watch), else — the bar off or hidden — a Windows notification under
  the "calendar" tag; a card follows you onto the bar when the window loses
  focus, back into the window when it gets it (`MoveIn`,
  `on_main_window_focused`), and off a bar that goes away. **Into notes**:
  `on_meeting_started` (from `commands::meeting_start`, every route) ties the
  note to the meeting on now (service of the live call first, nearest start;
  from 10 min before) — title over a placeholder (`is_placeholder_title` /
  `title_open_to_ai`; then a real title), attendees, the cleaned invite
  (`invite_context` → action plan + Ask), and for a 1:1 meeting "Them" named
  after the one other invitee as a guess (`notes::link_event` →
  `speakers::calendar_guess`) — and `note_changed`s every window;
  `claims_call` / `on_call_started` for call detection (above);
  `meeting::with_attendees` puts a meeting note's participants (and the
  speaker map's names) into Whisper's prompt. Commands `calendar_status`/`_sync`/`_connect_google`/
  `_cancel_google`/`_add_link`/`_disconnect`/`_event(key, open|join|start|
  joinStart|switch|joinSwitch)`/`_card(id, join|start|snooze|dismiss)`/
  `_nudge`/`_meeting_notes(query)` (`hasPlan`, `hasTalk`: "Generate
  summary"); events `yap-calendar` (snapshot), `yap-calendar-connected`
  (`{kind, label, meetings}`: the new calendar's meetings on now or in the
  next 7 days, which `_add_link` also returns), `yap-calendar-error`;
  `meetings_context` backs the
  Meetings view's Ask bar (`chat_send` scope "meetings"). Debug-only:
  `YAP_GOOGLE_{AUTH,TOKEN,REVOKE,API}_URL` (+ the client at run time) and
  `calendar_e2e_opened` (test mode records links instead of opening them).
  Lock rule: never hold its state lock while touching windows, the network,
  notes or call detection.

## The "Connect your calendar" dialog

- **`ConnectCalendarDialog.svelte`** = Wispr Flow's "Connect your calendar"
  modal (its screenshot 27) in Yap's flows, mounted once by ControlPanel and
  opened by every Connect calendar outside Settings (the Meetings view's
  card, the nudge, Integrations' Calendar card): a warm beige band
  (`--yap-paper-band`) showing the bar's real pre-meeting card
  (`bar/CallCard.svelte` with `preview`, sample data "Design review · ● In 2
  min · with Sam", Join / & take notes ^), then "Connect your calendar", one
  line, and three full-width outlined buttons with `CalendarMark.svelte`'s
  marks (shared with Connectors): **Continue with Google** (the browser
  flow, waiting in place with Cancel; closing doesn't cancel it; off in a
  build without the client, "Google sign-in is in the installed Yap. In this
  build, use a private iCal link below."), **Continue with Outlook** (the
  publish steps + the ICS field) and **iCloud or another calendar** (where
  to find the link + the field; Yap's checks say why a link fails), then
  "Calendar connected · 6 meetings in the next 7 days" for 2.4 s and it
  closes. Wispr's dialog tokens (scrim `rgba(26,26,26,.3)`, paper card,
  radius 20, the dialog shadow, scale-in); Esc / round ✕ / scrim close it;
  labelled by its title, focus trapped (Tab wraps) and returned (to the
  opener, or the Meetings title).
