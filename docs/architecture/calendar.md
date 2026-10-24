# Calendar

Connecting Google Calendar or an iCal link, sync, parsing, the reminder card, pre-meeting briefs and tying recordings to meetings, plus the "Connect your calendar" dialog. The full guide is [`docs/calendar.md`](../calendar.md); the Meetings view is in [meetings.md](./meetings.md).

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
  **one place** (`notices/notice.rs`): the in-app toast while the main window is
  focused (the snapshot's `card` with `inApp`), else the **Yap bar**
  (`bar::show_card`, id "calendar": a call-style card
  — the call app's mark or a calendar, the meeting over "● In 1 min · with
  Tanay +1" in amber, green once it's on, kept current with `update_card`
  every 5 s; Join & take notes | ^ Start notes, Snooze 2 min; ✕ = dismiss; no
  Esc watch), else — the bar off or hidden — a Windows notification under
  the "calendar" tag; a card follows you onto the bar when the window loses
  focus, back into the window when it gets it (`MoveIn`,
  `on_main_window_focused`), and off a bar that goes away. **Into notes**:
  `on_meeting_started` (from `meetings::commands::meeting_start`, every route) ties the
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

## Pre-meeting briefs

- **`calendar/brief.rs`** (pure, unit-tested, run as is by `yap.exe eval
  --brief`) — what to know going into a meeting (Granola's "Briefs", local),
  ≤ 150 words (`MAX_WORDS`): `gather(meeting, notes, memory, now)` →
  `Facts`: **Last time** (≤ 3 earlier meeting notes, newest first, with an
  attendee in common — `same_person`: full names agree, a lone first name
  only while one attendee has it — or of the same series, `series_of(key)`:
  the iCal UID before `:<original start>`, Google's id before
  `_YYYYMMDDTHHMMSSZ`, without the connection; notes with nothing in them,
  this meeting's own and anything later left out; "First meeting with X"
  for someone new, "First meeting with these people" with nobody known),
  **Open items** (`plan_tasks`: unticked `- [ ]` under "### Name" in the
  action plan, or "Name: task" lines, else the digests' actions; You's or an
  attendee's, ≤ 4, once each, deadline kept), **From memory** (People items
  for the attendees, Projects the title or invite name; ≤ 3), **On the
  invite** (≤ 30 words of the cleaned description); each line cites
  numbered `Source`s (note id + date, memory id, the invite). `messages`:
  `llm::MEETING_BRIEF_PROMPT` + a worked example (people no real brief has)
  + `model_input` (the meeting, each source as the model reads it — a note's
  plan, else digests, else typed notes + transcript, ≤ 600 tokens each — and
  "Yap lists these open tasks… don't repeat them"); none when only an
  invite (nothing to add). `check_summary` keeps a bullet only when it
  cites real sources, is grounded (`ungrounded`: capitalised words — not a
  sentence's everyday first word, `learn::is_frequent`, unless a name-word
  — numbers, days, months, relative days and deadline phrases must be in
  its cited sources, the meeting's title or attendees; ≥ 40 % of its
  content words and ≥ 50 % of each clause's; an "on the agenda" clause
  needs the invite, which Yap adds to the citation), keeps owners
  (`swaps_owner`, per clause, over the cited sources' tasks else any's,
  including in a plan-less 1:1 what each side said they'd do), says no
  open task was done (`claims_done`: the task's verb in the past, "already",
  or a "will/I'll X" of its sources in the past) and no open question
  settled (`settles_question`); ≤ 3 bullets, ≤ 55 words; "the user" → "you".
  `assemble` fits the parts under 150 words (the invite gives way first,
  then memory, open items past two, summary bullets past one) and keeps only
  cited sources. `writes_summary(local, model)`: a local model under 3.5B
  (`SUMMARY_MIN_PARAMS_B`) writes no "In short". `write(app?, facts, ep)`:
  the call (`chat_beside_dictation` in the app, straight in an evaluation)
  + the checks. `Facts::fingerprint(ai)`: SHA-256 of the meeting, the notes'
  and memory items' ids + edit times and the model.
- **In `calendar.rs`**: `prepare_brief(key, force)` (blocking: reads
  notes and memory outside the lock) returns the kept brief when its
  fingerprint matches, else stores Yap's part at once (`writing` while a
  summary is due) and spawns `write_brief` (one at a time,
  `brief_job`/`brief_waiting`; a failure waits `BRIEF_RETRY`); `BriefAi::of`
  = the meeting's endpoint (`meeting_summary::resolve_endpoint`), or why
  not (no model, the on-device model stopped — never started for a brief —,
  too small). `tick_briefs` (each scheduler turn): meetings from
  `BRIEF_AHEAD` (15 min) before their start until 5 min after, with no brief
  or `briefs_dirty` (notes/memory/settings changed); emits the snapshot when
  the setting flips. `store_brief` → `calendar.json` `briefs` (pruned:
  `BRIEF_KEEP` 2 days after the start, `BRIEF_MAX` 40), `yap-meeting-brief`,
  the snapshot (`card.brief`, `briefs`) and the bar card
  (`bar::update_card` → `Card.brief`). Commands `calendar_brief(key,
  refresh?)`, `calendar_brief_for_note(noteId)`, `calendar_brief_open(key,
  n)` (a citation: the note in the main window's Notes via
  `yap-meeting-open-note`, memory → `yap-open-settings` "memory", the
  invite → the meeting's own note, `note_for`). Logs counts at info, the
  brief's text at debug only.
- **Pages**: `BriefView.svelte` (one renderer: parts, `who` / `due`,
  source pills with their titles, tones paper / dark / bar), `brief.js`
  (`briefTeaser`, "Last met Thu 16 Oct · 3 open items"); the card toast's
  `expand.brief` (`calendar.svelte.js`, kept current as the summary
  lands), the bar call card's brief line (`bar/CallCard.svelte`), the
  Meetings view's Brief button (`loadBrief`, `briefs.byKey`), the notepad's
  Summary card and My thoughts line ([meetings.md](./meetings.md)).
  Settings → Meetings → Calendar → "Prepare a brief before meetings"
  (`meeting_brief`, on; `settingsIndex.js` anchor `meeting-brief`).

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
