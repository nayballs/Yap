# Calendar

Yap reads your own calendar straight from your PC to list your meetings
(the **Meetings** view), remind you just before each one (**Join & take
notes**), and give a meeting note the meeting's name, attendees and invite.
It's Wispr Flow's calendar, local-first: there is no Yap server in between.
Yap talks to Google, or fetches your calendar's private iCal link, itself;
the tokens and links live in Windows Credential Manager, and meetings stay
on the PC.

| Piece | Code |
|---|---|
| Connections, sync, the reminder card, notes, the nudge, the Ask bar's context | `src-tauri/src/calendar.rs` |
| Google: OAuth for installed apps, the Calendar API | `src-tauri/src/calendar/google.rs` |
| iCalendar parsing, recurrence | `src-tauri/src/calendar/ics.rs` |
| Time zones (IANA, Windows names, a feed's own VTIMEZONEs) | `src-tauri/src/calendar/tz.rs` |
| Which events count, attendees, reminders, matching a recording to a meeting | `src-tauri/src/calendar/model.rs` |
| Join links | `src-tauri/src/calendar/links.rs` |
| Credential Manager | `src-tauri/src/calendar/vault.rs` |
| A note's meeting (`Note::event`, `link_event`) | `src-tauri/src/notes.rs` |
| UI | `src/lib/MeetingsView.svelte`, `src/lib/ConnectorsSection.svelte`, `src/lib/calendar.svelte.js`, `src/lib/CalendarLinkCard.svelte` |

## Connecting a calendar

**Settings → Connectors** (the Connections group, above MCP), laid out like
Wispr Flow's: a card per connector, **Connect** on the right, and once
connected, a row with the account and a **⋯** menu (**Sync now**,
**Disconnect**). Up to 8 calendars.

- **Google Calendar**: **Connect** opens Google's consent page in your
  browser. Approve, the page says "Calendar connected", and Yap lists your
  meetings. Only in builds that carry Google's client id (see
  [Enabling Google](#enabling-google-for-the-maintainer)); others say so and
  point at the iCal link instead.
- **Outlook Calendar**: Yap has no Microsoft app yet, so **Connect** walks
  through publishing the calendar: Outlook on the web → Settings → Calendar
  → Shared calendars → **Publish a calendar**, your calendar, **Can view all
  details** (so Yap sees who's invited) → **Publish**, then paste the **ICS**
  link.
- **Other calendar**: any private iCal link. Google Calendar: Settings → your
  calendar → Integrate calendar → **Secret address in iCal format**. iCloud:
  Calendar → share → **Public Calendar**. Fastmail and others: the calendar's
  sharing or export settings. `webcal://` links work.

A link that stops working (reset, unpublished) shows on its row, and the
meetings Yap had stay until it works again.

## Privacy

- **Straight from the PC.** Requests go to Google
  (`accounts.google.com`, `oauth2.googleapis.com`, `www.googleapis.com`) or
  to your iCal link's host, and nowhere else. Calendar has nothing to do
  with Yap accounts: it works signed out.
- **Read-only, and the least Google allows.** Two scopes:
  `calendar.calendarlist.readonly` (which calendars to read; the primary
  calendar's id is the account's address, so no profile or email scope) and
  `calendar.events.owned.readonly` (events on calendars you own:
  invitations land there, so colleagues' or holiday calendars you merely
  subscribe to stay out of reach). Yap asks the API for the fields it uses
  and nothing else.
- **Secrets in Credential Manager.** Google's refresh token, or the iCal
  link (which works like a password), is a Windows Credential Manager
  generic credential, `yap-calendar-<1-8>.com.yap.dictation` (portable:
  `yap-calendar-<data-dir hash>-<n>`), Local persistence: never in
  `config.json`, `calendar.json`, the logs or the webview (the page only
  sees the account's address or the link's host).
- **What's kept.** `<data>/calendar.json` keeps the connections' names, the
  meetings from 12 hours ago to 7 days ahead (title, times, attendees' names
  and addresses, join link, the invite's description cut to 1,000
  characters), which reminders were answered (2 days) and the nudge. It's
  written atomically and quarantined if corrupt, like every store.
- **Disconnect** forgets the credential, drops that calendar's meetings and,
  for Google, revokes the token. The uninstaller's **Delete the application
  data** removes `calendar.json` with the rest of `%APPDATA%\yap` and the
  eight `yap-calendar-*` credentials.
- **Join links** open in your browser only when they're HTTPS links to a
  known meeting service (Teams, Meet, Zoom, Webex, Slack, GoTo, Whereby,
  Jitsi, Chime, BlueJeans, Skype, RingCentral, Dialpad, Around). An iCal link
  must be HTTPS too, except on this PC (`localhost`, for the tests).

## How it works

### Sync

Every 12 minutes while Yap runs, straight after the PC wakes (the scheduler
wakes at least every 30 s; a bigger jump in the wall clock means it slept),
on **Sync calendar** / **Sync now**, and right after a connection is added.
Each sync replaces a connection's meetings with the window from 12 hours ago
to 7 days ahead; one that fails keeps what it had and shows why. Google
access tokens refresh themselves; a refresh token Google no longer accepts
(`invalid_grant`: revoked, password changed, or 7 days old in a Testing-mode
app) shows **Connect again** on its row (the same account takes its old
place).

Google: `calendarList` (`minAccessRole=owner`; the primary calendar plus
those shown in Google Calendar, at most 10), then each calendar's `events`
with `singleEvents=true`, so Google expands recurrence. iCal links: fetched
(at most 32 MB), parsed and expanded by Yap.

### Which events count

Wispr Flow's filters (`model::finish`):

- **a meeting has other people**: at least one invitee besides you (rooms,
  resources and people who declined don't count), or a join link;
- **no all-day events**, nothing **longer than 6 hours**, nothing cancelled,
  nothing **you declined** ("Maybe" shows a **Maybe** tag);
- Google's out-of-office, focus time, working location and birthday events
  are skipped.

You (to leave out of attendees and to read your reply) are the account on
Google; in an iCal feed, the calendar's name when it's an address, else the
one person on at least 90 % of its meetings with others (at least three of
them). Two such people (every meeting is with Sam) is a tie and nobody:
taking Sam for you would leave those meetings with no one else and hide
them.

### Recurrence and time zones

- `rrule` 0.14 expands RFC 5545 rules (the crate the `icalendar` crate
  builds on, maintained, chrono-based). Yap expands on wall-clock time, then
  maps each occurrence through the event's zone, so a weekly 09:00 stays at
  09:00 across a DST change. EXDATE, RDATE and RECURRENCE-ID overrides
  (moved or cancelled occurrences) apply; UNTIL in UTC or as a date; the
  start is always an occurrence; second/minute/hour rules (never meetings)
  are skipped.
- `chrono-tz` 0.10 is the IANA database. TZIDs resolve in this order: IANA
  names (also as path suffixes, `/mozilla.org/…/Europe/London`), Windows
  names (`GMT Standard Time`, Outlook's; CLDR's `windowsZones.xml`, all 139),
  the feed's own VTIMEZONE rules, "(UTC+01:00) …" display names, else
  floating (local time). Gaps and overlaps follow RFC 5545 (a time in a gap
  is read with the offset before it; one in an overlap is the first).
- The parser is Yap's own and forgiving on purpose: byte-level unfolding
  (a fold may split a UTF-8 character), quoted parameters, RFC 6868
  escapes, and a malformed line or a missing END skipped rather than failing
  the feed, so one odd line never hides the rest of a calendar.

### The Meetings view

In the sidebar beside Notes: **Today** and **Upcoming** (the next 7 days,
grouped by day), three at a time with **Show more**, each row with its
time, name, service and attendees, a **Conflict** tag on overlapping
meetings and **Maybe** on tentative ones. From 10 minutes before a meeting
until its end, its row offers **Join meeting**, **Start** and **Join +
Start** (just **Start** without a link; **Switch notes** / **Join + Switch**
while another meeting records, and **Recording** · **Open note** on the one
recording); opening a meeting makes or opens its note (a draft ahead of
time: title, date, attendees, Meetings folder). Below, **Past meeting
notes** with search, and an **Ask bar** that cycles example questions and
asks Chat about your
meetings (scope `meetings`: the three latest meetings and up to three that
match the question, bounded for small local models), with **Past chats ↗**.
Without a calendar it offers one; a "Connect your calendar" nudge also
comes once after a meeting ends, and **Not now** means never.

### The reminder card

**Settings → General → Meetings → Notify before scheduled meetings start**:
Right before (15 s, the default), 1 minute, 2 minutes, Never
(`meeting_reminder`: `15s` | `1min` | `2min` | `never`), with a **Calendar**
row under it that opens Connectors. The card, "Design review · In 1 min ·
Teams · with Tanay Kothari, Priya Shah", offers **Join & take notes**
(opens the link and starts notes) and **Start notes** (only that without a
link), **Snooze 2 min** and ✕ (Esc, outside a text box). It stays
until 5 minutes after the start, one card at a time, earliest first. While
another meeting's notes record, it offers to **switch** notes instead
(**Join & switch notes** / **Switch notes**): the running meeting ends
(`meeting_end::end`, so its action plan is written) and the next one's
notes start.

Every card goes on screen through **one function**, `calendar::present_card`
(and comes off through `withdraw_card`), the way call detection's prompts do:
the main window's in-app toast, and while the window isn't focused (the call
app in front, Yap in the tray) a card on the **Yap bar** (`bar::show_card`,
id `calendar`), after call detection's "Meeting detected" card: the call
app's mark (a calendar when there's none), the meeting over "● In 1 min ·
with Tanay +1" (amber, green once it's on; kept current with `update_card`),
a split button **Join & take notes** whose ^ menu holds **Start notes** and
**Snooze 2 min**, and ✕ = dismiss. Esc isn't watched for it (that would be
every Esc in every app for minutes). With the bar off or hidden for an hour,
the card is a Windows notification with the same buttons instead. Answering
any copy answers them all; a card shown while the window was focused moves
onto the bar when the window loses focus, and off a bar that goes away.

### Into meeting notes

However a recording starts during a meeting (the card, the Meetings view,
call detection's **Record notes**, Notes' Record, the meeting shortcut),
`on_meeting_started` ties the note to the meeting: the meeting's name
replaces a made-up title ("Teams call · 5 Oct, 14:30") and counts as a real
title (the AI title leaves it alone), the attendees join the note's
participants (the names the action plan gives tasks to, and Whisper's
spelling hints for the meeting), and the invite's description, without its
links and dial-in boilerplate, becomes context for the action plan and the
note's Ask bar. A recording is matched to the meeting on now (from 10
minutes before it) whose service matches the live call, the one starting
nearest. Resuming an older meeting's notes doesn't count.

**Not asking twice**: call detection asks the calendar before "Teams call
detected — Record notes?" (`claims_call`): a call during a meeting whose
card is up, or was answered (joined, started, dismissed), isn't asked
about; a snoozed card comes back instead. **Back to back**: a call during
the next meeting while the last one's notes still record offers the switch
card (`on_call_started`).

## Enabling Google (for the maintainer)

A build connects to Google only with an OAuth client baked in at build time
(`YAP_GOOGLE_CALENDAR_CLIENT_ID` / `_SECRET`, read with `option_env!`; both
workflows pass them from repository secrets; empty = no Google).

**Status (2026-10-06):** done on `yap-accounts`. Steps 1–6 are complete:
- the Calendar API is enabled;
- both scopes are added, with the justification;
- the privacy page is deployed;
- there's a Desktop client, "Yap desktop - Calendar";
- both repository secrets are set.

The app stays published, and sign-in's consent page is unchanged ("continue to Yap",
checked). **Left:** step 7's verification submission for
`calendar.events.owned.readonly`, which needs a demo video. Until it's approved,
connecting shows Google's unverified-app screen and is capped at 100 users.

Steps:

1. **Project**: in Google Cloud, either `yap-accounts` (its branding, "Yap"
   with the logo, is already verified, so the consent page looks right;
   adding the scopes below sends those scopes for verification) or a new
   project (leaves the live sign-in's consent screen untouched, but needs
   its own branding and verification).
2. **APIs & Services → Library → Google Calendar API → Enable.**
3. **Google Auth Platform → Data Access → Add or remove scopes**: add
   `https://www.googleapis.com/auth/calendar.calendarlist.readonly` and
   `https://www.googleapis.com/auth/calendar.events.owned.readonly`. Both are
   *sensitive* (not restricted: no security assessment). Justification, for
   example: "Yap is a Windows dictation and meeting-notes app. It reads the
   user's own calendar events (title, time, attendees, conference link,
   description) for the next 7 days to remind them before meetings, join in
   one click, and name meeting notes with the meeting's title and attendees.
   The data is read on the user's PC and kept only there; nothing is sent to
   Yap's servers. The calendar list is read only to find the user's own
   calendars and their address."
4. **Branding**: home page, privacy policy and terms links on the authorised
   domain. The privacy policy has to say what Yap reads from Google
   Calendar, how it's used and kept, and carry Google's Limited Use line:
   `cloud/public/privacy.html` has that section ("Your calendar, in the Yap
   app", `#calendar`); deploy `cloud/` so
   `https://auth.contextmirror.com/privacy` shows it before submitting.
5. **Clients → Create client → Application type: Desktop app** (name it, say,
   "Yap desktop – Calendar"). No redirect URIs: desktop clients accept the
   loopback address (`http://127.0.0.1:<port>`) Yap listens on. Copy the
   client ID and secret (Google treats a desktop app's secret as public; it
   stays out of the repo all the same).
6. **GitHub → nayballs/Yap → Settings → Secrets and variables → Actions**:
   add `YAP_GOOGLE_CALENDAR_CLIENT_ID` and `YAP_GOOGLE_CALENDAR_CLIENT_SECRET`.
   `release.yml` and `nightly.yml` already pass them to the build.
7. **Audience**: while the app is in *Testing*, only its test users (up to
   100) can connect, and their refresh tokens expire after 7 days (Yap then
   shows **Connect again**); add your own address to try it. **Publish app** and
   submit the two scopes for verification (a demo video showing the consent
   screen, with the client ID in the address bar, and how Yap uses the data)
   to open it to everyone; until verified, people see the "Google hasn't
   verified this app" screen when connecting a calendar.
8. **Check**: dispatch a nightly, install it, Settings → Connectors → Google
   Calendar → **Connect**: the consent page lists the two read-only scopes,
   then "Calendar connected", and the Meetings view lists your meetings.

To try a real client in a dev build without baking it in, set both
variables in the environment before `scripts\dev.bat` (debug builds read
them at run time).

## Testing

- **Unit tests** (`cargo test --lib calendar`, 46): parsing (unfolding,
  split UTF-8, broken lines, parameters), recurrence (weekly with EXDATE,
  moved and cancelled overrides, UNTIL/COUNT, a date-only UNTIL, RDATE, DST
  wall clock, an override moved into the window, unknown rule parts), time
  zones (Windows names, every Windows zone mapping to a known IANA zone,
  custom VTIMEZONEs, display offsets, floating, gaps and overlaps), the
  filters (rooms, declines, all-day, longer than 6 hours, no invitees and no
  link), finding the feed's owner (and a tie being nobody), join links (services, HTTPS only, help pages, punctuation, HTML,
  source order), invite cleanup, reminder timing (due, stays 5 minutes,
  snooze), matching a recording to the nearest meeting, placeholder titles,
  Google's events address and items, the card on the Yap bar (mark, status
  line keeping time, answers) and as a Windows notification (its XML).
- **e2e** (`npm run test:app`): `e2e/calendar.spec.js` against a local iCal
  server (`e2e/support/calendar-feed.js`: a meeting a minute away with two
  attendees and a Teams link, a weekly recurring one, an all-day event,
  conflicts, a tentative one) and a local fake of Google
  (`e2e/support/fake-google.js`: consent, token, revoke and the Calendar
  API, checking PKCE, the client and the bearer token; debug-only
  `YAP_GOOGLE_{AUTH,TOKEN,REVOKE,API}_URL`):
  Connectors (the Outlook guide, a reset link, one that works); the Meetings
  view (today and the next 7 days, Conflict and Maybe, all-day hidden, Show
  more, a draft note); the reminder card, in the window and on the Yap bar
  (**Join & take notes** records into a note with the meeting's name and
  attendees, the bar's copy goes too, and its simulated call isn't asked
  about again); Esc, the bar's ^ menu snoozing it, a snoozed card coming
  back with its call, the bar's ✕; back-to-back switching (on the bar too); Google connect → meetings → ⋯ menu → Disconnect
  (token revoked, meetings gone). `e2e/calendar-nudge.spec.js`: no calendar
  yet (and a build without Google), the nudge once after a meeting and "Not
  now", past notes with search and the Ask bar opening Chat. Test mode keeps
  secrets in `calendar-secrets.e2e.json` in the portable data dir instead of
  Credential Manager, and records the links Yap would open
  (`calendar_e2e_opened`) instead of opening a browser. Screenshots:
  `test-results/app/screenshots/calendar/` and `…/calendar-nudge/`.

## Limits and follow-ups

- **Outlook in one click** (Microsoft Graph): needs a Microsoft Entra app
  registration (personal and work accounts, a public client with a loopback
  redirect, delegated `Calendars.Read` + `offline_access`); some work
  tenants require admin consent. Until then, the published calendar: it
  shows attendees only with "Can view all details", Outlook refreshes it on
  its own schedule (it can lag), and some organisations turn publishing off.
- **A brief before the meeting** (Wispr's "Meeting prep") isn't built.
- Google calendars you don't own (a shared team calendar) aren't read, by
  the choice of scope.
- Who "you" are in an iCal feed is a guess (see above): when Yap can't tell,
  you show among the attendees and a meeting you declined still shows.
- The Windows notification card (its XML is unit-tested) and the
  uninstaller's credential cleanup (NSIS, built by CI only) haven't been
  seen on an installed build yet.
