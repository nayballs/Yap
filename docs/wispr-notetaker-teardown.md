# Wispr Flow teardown: Notetaker and Scratchpad

> **Purpose.** What Wispr Flow's two note-taking products do, **Notetaker** (meeting
> notes without a bot joining the call) and **Scratchpad** (a quick-capture notepad),
> and a build-ready checklist for **feature parity with Notetaker wherever it fits
> Yap's local-first design**. Paired with [`meetings.md`](./meetings.md) (Yap's
> meeting recorder), [`e2e-tests.md`](./e2e-tests.md) (the suite every checklist item
> is tested with), [`openwhispr-teardown.md`](./openwhispr-teardown.md) and
> [`competitive-analysis.md`](./competitive-analysis.md). The latter dates from July
> 2026, before Notetaker shipped, so its "Meeting recording: no" for Wispr is out of
> date.
>
> **Sources.** Public material read on **5 Oct 2026**: Wispr's help centre, marketing
> pages and changelog, press coverage and one hands-on review. Plus a **hands-on
> session** with Nathan's installed Wispr Flow for Windows (v1.6.1034, 5 Oct 2026),
> written up outside the repo ([hands-on]); its 20 screenshots stay there and are
> named here by file. No Wispr program files were opened or decompiled. Wispr's help
> centre is edited almost daily and sometimes contradicts itself or the shipped app;
> those spots say **(conflict)** and are collected in [§10](#10-where-wisprs-sources-disagree).
> Keys like ([rec]) link to the [Sources](#sources).

Legend. **Yap today**: Have / Partial / Gap. **Effort**: **S** = a contained change in
one or two existing files; **M** = a new module, or a change across backend and UI;
**L** = a new subsystem (new models, a cloud service, or automating other apps).
**e2e** = the Playwright suite in `e2e/` (`npm run test:app`, the stub build; see
[`e2e-tests.md`](./e2e-tests.md)).

---

## TL;DR

- **What Wispr built.** Notetaker records a call from your own computer (mic plus
  system audio, no bot), streams the audio to Wispr's cloud for a live transcript,
  re-transcribes it after the call to name the speakers, and writes a "Flow Summary"
  (a recap, next steps by owner, decisions). Around that: a calendar with a heads-up
  card and one-button "join and record", a pre-meeting brief, **"What did I miss?"**,
  an "Ask anything" chat with citations, share links, a remote MCP server, and imports
  from Granola and Otter. Mac since 5 Aug 2026, Windows since 15 Sep 2026, still
  labelled beta. The meeting UI is a **notepad docked to the right edge of the
  screen**. Scratchpad (May 2026) is a floating, synced, multi-tab notepad you can
  dictate into.
- **Where Yap stands.** Yap already has the core loop, and runs it fully offline: a
  You/Them transcript made on the PC, rolling digests, an action plan whose owners and
  deadlines are checked in code, call detection, a per-note Ask bar and an AI Chat
  over notes. Wispr keeps every transcript and summary in its cloud, with no opt-out
  ([sharing]).
- **What's missing is mostly small.** Two are bugs found while comparing (from the
  code; confirm once in dev): **Yap writes your own dictations into the meeting's
  "You" transcript**, and **meeting lines skip the dictionary's replacements**. Then:
  "Them" goes silent, with no warning, when the call plays on a non-default device;
  nothing outside the main window shows that a meeting is recording; the meeting UI is
  the big main window rather than a panel beside the call; answers carry no
  citations; there's no calendar.
- **Top 10 of the build checklist** ([§8](#8-build-checklist-the-top-10)):
  1. Keep dictation out of the meeting transcript (S)
  2. Hear the call wherever it plays, and say when a side goes quiet (M)
  3. A meeting notepad docked beside the call (M)
  4. Show that a meeting is recording, plus Wispr's guard rails (S–M)
  5. Run the dictionary over meeting transcripts (S)
  6. "What did I miss?", since you last looked (S)
  7. Answers you can check: passage search and citations into the transcript (M)
  8. A calendar without a cloud account: ICS, heads-up card, one-button record (M)
  9. A local MCP server over the bridge (S–M)
  10. Screen-share and consent hygiene (S)
- **Worth knowing.** Wispr's Windows Notetaker asks for 16 GB of RAM ([win]). On
  Windows it can't name Teams speakers ([editor]). The one published hands-on review
  (by tl;dv, a competitor) found speaker labels unreliable and its MCP search skipping
  transcripts, and relayed early users' view that its summaries trail Granola's
  ([tldv]). The help centre documents settings (auto-start, a consent section) that
  the Windows build doesn't show ([hands-on]).

---

## 1. Overview

### Notetaker
- **What it is.** A meeting recorder inside the Wispr Flow desktop app. It captures
  your microphone and the computer's audio on your own machine, so no bot joins the
  call, and sends the audio to Wispr's servers for a live transcript, a second
  "refined" transcript with speaker names after the call, and a summary ([privacy],
  [launch]). Wispr lists Google Meet, Zoom (and Zoom Gov), Teams, Webex, GoTo
  Meeting, BlueJeans, Whereby, Amazon Chime, Dialpad, Slack huddles and in-person
  meetings ([by-speaking]).
- **Platforms.** macOS 13+ ([start]); Windows 10/11 with 16 GB RAM and 2 GB free disk
  ([win]). The iPhone app only lists upcoming meetings and opens synced notes; it
  can't record ([ios]). No Android.
- **Dates.** Mac on 5 Aug 2026, Windows on 15 Sep 2026 ([releases], [cw]).
  English-only at launch ([reworked]), 21 languages since the Windows release
  ([releases]). Since 17 Sep, transcription runs on Wispr's own "Canto" speech model
  ([whats-new]).
- **Status.** Every Notetaker help article is titled "(beta)". Some enterprise
  accounts see "Notetaker is coming soon!" ([summaries]); a signed HIPAA BAA blocks it
  ([start]).
- **Price and limits.** Pricing page: Free has a "Limited" Notetaker with a weekly
  meeting limit; Pro ($15 per user a month, $12 billed yearly) has higher limits and
  longer retention; Growth ($23, $18 yearly) and Enterprise get unlimited Notetaker as
  an add-on; teams on dictation-only plans get it free until 31 Oct ([pricing]). The
  hub article says paid plans get 100 meeting notes a week, with a banner showing
  what's left ([hub]). **(conflict)** The plans article says Notetaker "isn't limited"
  on Free ([plans]). The Free number isn't published ([tldv]). A recording stops at 3
  hours at most ([settings]).
- **Weight.** tl;dv measured about 750 MB of RAM idle and 990 MB while recording on an
  8 GB M3 iMac, with screen-share stutter in the call ([tldv]).
- **Context.** Wispr has raised $81M ([cw]). Press tied the launch to class actions
  against Otter and Granola over recording people without telling them, and noted
  that Wispr's terms (updated 25 Jul 2026) put consent on the user ([reworked], [tc]).

### Scratchpad
- Launched on 1 May 2026 in desktop v1.5.113 as a beta "notepad that floats on top",
  on Option+S on Mac, with tabs, version history, a notes sidebar, images and sync to
  iPhone ([rel-1.5.113], [whats-new]). It shipped with **Transforms**, an AI rewrite
  of selected text ([rel-1.5.113]).
- Today: Mac and Windows, with "Notes" in the iPhone app's Scratchpad tab (iOS 18.3+)
  ([scratchpad], [ios-notes]). There's no default shortcut any more ([scratchpad]).
  Notes sync whenever you're signed in and online, whatever "Dictation Cloud Storage"
  says ([scratchpad]), and MCP clients can read them ([mcp]). Details in
  [§5](#5-scratchpad).

### What Wispr's Notetaker page promises, and Yap's answer
The landing page's five workflow claims ([notetaker-page]):
- **"Starts automatically"**: one button joins and records, and it catches
  back-to-back meetings and calls that aren't on your calendar. *Yap:* call
  detection already offers "Record notes" for any call (`meeting_detect.rs`); the
  one-button join needs a calendar (#8); back-to-back calls in one app merge into one
  note today.
- **"Whatever you use"**: Google Meet, Teams, Huddles, lunch catch-ups. *Yap:* any
  call app through loopback, in-person through the mic; detection knows 12 apps and
  browser meeting tabs.
- **"Connects with your AI"**: Claude, ChatGPT, Gemini and any MCP tool. *Yap:* the
  local REST bridge today; MCP is #9.
- **"Switch without losing a thing"**: import your meeting history from Granola or
  Otter (source, date, summary and transcript). *Yap:* no import yet (R17).
- **"For business"**: control Notetaker access across the organisation, with optional
  consent acknowledgement, no model training and central privacy rules. *Yap:* no
  organisations; the local-first answer is machine policies for IT (R20).

### Timeline
- **1 May 2026**: Scratchpad (beta) and Transforms (beta), desktop v1.5.113; iPhone
  1.59 syncs ([rel-1.5.113], [whats-new]).
- **25 Jul**: terms updated ahead of Notetaker ([reworked], [tc]).
- **5 Aug**: Notetaker on Mac: speaker names from calendar invites, topic summaries
  with action items, Ask across meetings, briefs, MCP ([releases], [launch]).
- **28 Aug**: an org-wide on/off switch in the admin portal; self-serve Growth plans,
  with or without unlimited Notetaker ([releases]).
- **4 Sep**: Otter import (v1.6.721) ([releases]).
- **15 Sep**: Notetaker on Windows, with Gemini over MCP, 21 languages, Outlook
  calendars, Granola import, "Draft from your notes" and admin-required consent
  confirmation ([releases], [whats-new], [win]).
- **17 Sep**: the Canto speech model ([whats-new]).
- **5 Oct**: help centre still labels Notetaker beta; Windows build v1.6.1034
  ([hands-on]).

---

## 2. The meeting lifecycle, from the docs

### 2.1 Before the call

**Detection**
- "Automatically detect any call" (on by default) shows a "Meeting detected" card when
  you join a call ([settings], [detect]).
- With "Start Notetaker automatically" (off by default, per device) the card counts
  down, 10 s by default, adjustable from 5 to 60 s, then records. Hovering or dragging
  pauses the countdown, "Start Notetaker" skips it, Esc or ✕ means "Not now", and the
  caret menu offers "Join only" or "Snooze for 2 min". Declining covers the whole
  call, so a rejoin comes back without a countdown ([rec], [detect]).
- **(conflict)** The dictation article says Notetaker never starts on its own and asks
  "Transcribe this meeting with Wispr?" (Yes / Another time) ([dictate]); the Windows
  build has no auto-start setting at all ([hands-on]).

**Calendar and reminders**
- Google Calendar and Outlook (since 15 Sep), connected in Settings → Connectors by
  browser OAuth; no iCloud ([detect], [releases]).
- It reads meetings with invitees: their links (HTTPS only), attendees and start
  times. Events without invitees, all-day events and events over 6 hours are left out
  ([detect]). The privacy article lists what's kept: titles, times, conference links,
  colours, recurrence, attendees, descriptions, and Google attendees' names and
  photos, but not locations ([privacy]).
- The hub's "Upcoming" list: the next 7 days, up to 50 meetings, three at a time with
  "Show more"; overlaps say "Conflict" (or "Maybe"); "Sync calendar" refreshes
  ([hub], [detect]).
- Row actions: "Join + Start" opens the link and records (from 10 minutes before the
  start), "Start" records without opening it, "Join meeting" opens it without
  recording. Opening a future event creates a draft note with the invitees and time
  ([detect], [hub]). **(conflict)** The recording article calls it "Join and record"
  ([rec]); the heads-up card says "Join meeting & start Notetaker" ([hands-on]).
- "Notify before scheduled meetings start": "Right before the meeting" (15 s, the
  default), 1 minute, 2 minutes or Never. Reminders show until 5 minutes after the
  start and can be snoozed ([detect], [settings]).
- Organisations can block meetings by attendee domain ([detect]).

**The brief**
- A short AI briefing before the call: who you're meeting (internal or external, 1:1
  or group) and where it sits in a series, attached to the reminder and kept in the
  note's "Brief" tab. The help article said Mac only, rolling out gradually, and had
  been taken down by 5 Oct ([brief-doc]).
- Marketing: drawn from the calendar invite, Gmail, Slack, past notes and web research
  (people's profiles and published work), "bullet-first", at most three bullets, each
  linked to its source; for recurring meetings, what was decided and what's still
  open ([prep]).
- Disconnecting the calendar deletes briefs that aren't attached to a note or
  recording ([detect]).

**Starting**
- From the hub: "New note" or "Start Notetaker" for unplanned meetings; Ctrl+N; the
  shortcut Win+Alt+M ("Join meeting / start Notetaker"); a triple tap of the
  dictation key ([start], [rec], [settings]).
- Onboarding must be finished first. A setup checklist asks for "Enable Microphone"
  and "Enable System Audio" ("Turn on"), then says "All set!" ([start]).
- One meeting records at a time ([rec]).

### 2.2 During the call

**Recording and audio**
- The notepad opens when recording starts ("Open Notepad when starting Notetaker", on).
  Closing it doesn't stop the recording; the Flow Bar's recording pill brings it back;
  "Stop Notetaker" ends it ([rec], [hub]).
- The microphone is always the Windows default, not the dictation mic setting ([rec],
  [ts-audio]).
- System audio on Windows: **(conflict)** the getting-started article says only the
  default output device is captured, so a headset chosen inside Teams is missed
  ([start]); the newer troubleshooting article says Windows 10 version 2004 (build
  19041) and later capture every output device, older Windows only the default, and a
  virtual-cable mic falls back to the default ([ts-audio]). Changing the default output
  mid-recording doesn't stop capture ([ts-audio]).
- Warnings: no mic audio for a minute gives "Notetaker isn't hearing audio from your
  mic" (Microphone settings / Ignore); then "Still there? Notetaker isn't hearing
  anything"; "Other-party audio is silent — try reconnecting your audio devices"; and
  on Windows "Could not capture system audio" or "System audio capture stopped", which
  need a new recording ([ts-audio]).
- While recording, the Flow Bar's Notetaker button becomes an upcoming-meetings
  drawer: "Open Note" for this meeting, "Switch" for others. Switch ends the current
  recording and starts the next one without asking ([detect], [hub]).

**Live transcript**
- On by default ("Show live transcript"). Turning it off hides the tab; the audio is
  still processed after the call ([live], [settings]).
- It follows new speech; scrolling up stops following; "Jump to bottom" resumes
  ([live]).
- Markers for pauses, resumes, sleep, "Dictated" and offline gaps ([live], [offline]).
- Find with Ctrl+F or "/" (two characters minimum, wraps, ignores case); copy the
  whole transcript with its speaker labels ([live]).
- Echo: on speakers the live view shows lines twice, under You and under Others; the
  refined transcript removes the echo from the mic side, including about 2 s before it
  starts ([live]). Echo filtering needs both sources, pauses during reconnects and can
  turn itself off for the rest of a meeting; headphones are recommended ([ts-audio]).

**Speaker labels**
- Live: "You" and "Others" by audio source, or numbered speakers ([live]).
- After the call, names are read from the meeting app's own window: Google Meet in
  Chrome, Edge or Brave; native Zoom (on Windows, only in English); Teams on Mac with
  its window visible; Slack huddles in the Mac desktop app. Webex and Discord stay
  numbered, and **Teams on Windows stays anonymous** ([live], [editor], [sharing]).
- No voiceprints. Calendar attendees are "possible names, not proof"; ambiguous
  speakers stay "Speaker N" ([summaries], [ts-notes]). Marketing: name a speaker once
  and it applies to the whole transcript ([by-speaking], [catch-up]).
- tl;dv's three-person test got it wrong: one speaker split in two, lines filed under
  the wrong name ([tldv]).

**"What did I miss?" and chat during the call**
- A "What did I miss?" button while recording returns a short recap, in the meeting's
  chat, of what others said, leaving out your own speech ([catch-up], [cw],
  [whats-new]). Hands-on, it covers what happened **since you last looked** ([hands-on]
  14).
- "Ask anything" works during the call, for questions or explicit edits to notes,
  summary or title ([editor]).

**Dictating during a meeting**
- Dictate into any app without stopping Notetaker: dictation borrows the mic, your
  dictated words are left out of the transcript, and a "Dictated" marker sits in the
  gap ([dictate], [by-speaking]). Automatic stopping waits while you dictate
  ([by-speaking]). Starting a recording mid-dictation says "Microphone is busy with
  dictation" ([dictate]).
- Dictation and Notetaker share one personal dictionary ([by-speaking], [win]).

**The consent notice**
- "Post a consent notice in the meeting chat": when recording starts, Flow types a
  short notice (up to 450 characters, editable, "Reset to default", in the app's
  language) into the meeting's own chat in Google Meet, Zoom, Slack huddles or Teams.
  On Mac it uses Accessibility (browser meetings need Chrome with the meeting as the
  active tab); a clipboard fallback restores your clipboard. It isn't repeated within
  2 minutes and waits for you to be admitted from a waiting room ([consent]).
- If it can't post, or can't find the meeting within 25 s, an "Obtain consent" dialog
  offers "Copy consent message" and "Done", and can't be dismissed with Esc; "Quit and
  discard meeting" deletes everything ([consent]). Organisations can require a
  "Consent obtained" confirmation ([start]) and lock the notice on ([consent]).
- "Calendar notice": Off, Notice only, or Notice and notes link, added to events you
  organise in the next 7 days (Google Calendar with write access; not Outlook)
  ([settings]).
- The privacy article is plain about it: bot-free recording tells nobody, and the
  notice doesn't collect consent; that's on you ([privacy]). At launch the only notice
  was a 15 s on-screen reminder ([reworked]). The Windows build shows no Consent
  section, only a footer line in the notepad ([hands-on]).

**Ending**
- "Stop Notetaker when a call ends" (on): it stops a few seconds after you leave. It
  follows native Teams, Zoom, Webex and Slack calls ending on both platforms, Zoom's
  and Teams' leave buttons on Mac, and on Windows a closed meeting tab, with a 20 s
  wait unless Chrome or Edge confirms sooner ([rec]).
- Prompts beside the Flow Bar: "Notetaker stopping" (a countdown), "Meeting still
  going?" (stops after a 30 s warning), "Still taking notes", "Stop taking notes?"
  ([rec]).
- Check-ins: after 10 minutes without transcribed speech, 10 minutes past the
  calendar end, and after an hour of recording; the last two are skipped if anything
  was said in the previous 3 minutes ([rec], [ts-audio]).
- "Maximum recording length": 30 minutes, 1, 2 (default) or 3 hours, with a warning 5
  minutes before (2 for the 30-minute limit); resumes count toward it ([rec],
  [settings]).
- After an automatic stop, "Notetaker stopped" offers "Resume" for 4 seconds. Resume
  adds a segment to the same meeting while the local audio is kept (7 days) ([rec]).
- Short or accidental recordings: "Started by mistake?" (Keep / Discard); failed starts
  under 10 s are discarded ([rec], [summaries]).

### 2.3 After the call

**Flow Summaries**
- Written automatically after more than 5 minutes of audio; shorter meetings need
  "Generate summary"; 50 spoken words or fewer gets "The meeting was too short to
  summarize." ([summaries], [offline]).
- The first time: about 2 minutes "to improve the transcript, identify speakers and
  generate the summary", then a "View summary" notification ([summaries]).
- Layout, in order ([summaries]):
  - **Recap**: a TL;DR, then sections by topic, short points in meeting order.
  - **Next Steps**: verb-first, only open personal commitments, grouped by owner, in
    order within each owner; "(Everyone)" for group tasks; "Unknown" only when other
    steps have owners.
  - **Decisions Made**: each with the reasoning on hover.
  - Empty sections are left out.
- Rules: work already under way and vague intentions aren't next steps; uncertain
  names are flagged, not guessed; numbers stay as spoken ([summaries]).
- Editable once written. After more audio, "Update Summary" ("New audio since this
  summary.") replaces the summary and your edits; "Add summary" brings back a deleted
  one; imported meetings can't be updated ([summaries], [editor]).
- "Draft from your notes": follow-up emails, project updates or briefs from a meeting
  ([whats-new]).

**The editor**
- Tabs: "My thoughts" (your typed notes; never rewritten by AI; never shared),
  "Summary", "Transcript" (read-only), "Docs" (attached documents, which also rescue
  unsaved summary edits), "Tasks" (where enabled, with a done/total badge), and "Brief"
  on Mac ([editor], [brief-doc]).
- Autosave, sync, and an AI title when the title is blank (a calendar title wins)
  ([editor]).
- "Open note in window" pops the notepad out ([editor]). "Ask anything" can rename
  speakers once processing finishes, and titles up to 120 characters ([editor]).

**Copy, export and sharing**
- "Copy summary" (formatted and plain text; Ctrl+C on the Summary tab) and "Copy as
  Markdown" (the whole meeting: notes, attendees, Brief, summary, chat, transcript and
  tasks, with your profile name in place of "You") ([sharing], [editor]).
- "Share" opens "Share notes": invite people and pick "Only people invited", "Anyone
  with the link" or "Everyone at {domain} with the link", then "Copy link". A link
  shows the summary only; the transcript needs an email invite and the desktop app;
  "My thoughts" is never shared ([sharing], [shared-link]). The default visibility is
  "Anyone with the link" unless an organisation narrows it ([privacy]).
- Recipients get a read-only web page with "Ask anything about this meeting" and "Open
  in Wispr Flow" ([shared-link], [ask]).
- "Auto-share notes" (off): share with all attendees, or your domain's, when the notes
  are ready ([settings]).
- Enterprise accounts log every view, "Anonymous" for signed-out visitors ([privacy]).

**The hub and search**
- Sections: "Upcoming", "Up Next", "Past notes" (with "View more"), and "Shared notes"
  or "Shared with me" ([hub], [ask], [hands-on]).
- A search button across meetings and notes; Ctrl+F inside one ([hub], [editor]).
- "Report" a meeting (issue type and "What went wrong?") sends the transcript, notes,
  summary and logs, never audio ([ts-notes]).

### 2.4 Ask Wispr
- "Ask anything" inside a meeting (that meeting first) or in the hub (across
  meetings). It searches transcripts, notes, summaries, titles, attendees and shared
  notes (not their transcripts unless you were invited), and the web for questions
  about the outside world ([ask]).
- **Citations:** answers cite their sources as readable labels, such as a meeting's
  title. Hub answers list up to five clickable meetings; answers inside a meeting link
  inline; marketing promises a click-through "to the exact moment" in the transcript
  ([ask], [by-speaking], [catch-up]).
- Dates work: "meetings yesterday", "with alex@… on Monday", in the device's time
  zone, by calendar start time, weeks Monday to Sunday ([ask]).
- It edits only when asked ("Update my notes with the budget decision" lands in "My
  thoughts") and never rewrites transcripts ([ask]).
- "New chat", "Past chats" per meeting, "Stop" ("[Request stopped]"); deleting a chat
  is permanent ([ask]).
- It says it won't follow instructions found inside retrieved meetings, notes or web
  pages ([ask]).

### 2.5 MCP
- Wispr is a **remote MCP server** at `https://api.wisprflow.ai/connect/mcp`. Settings
  → MCP has "Add to Claude", "Add to ChatGPT", "Add to Gemini", "Add to Cursor" and a
  server URL for other apps; sign-in is browser OAuth (Google, Apple, Microsoft or SSO;
  email-and-password accounts can't authorise) ([mcp], [hands-on] 06).
- Readable: meeting notes, summaries, briefs, transcripts on request, action items,
  Scratchpad notes, Google and Microsoft calendar events, participants and their email
  addresses, notes shared with you. Not dictation history. Access covers every
  meeting, past ones included, with no per-meeting or date scoping and no per-session
  approval; enterprise reads are audit-logged ([mcp]).
- Meetings need Cloud Sync; ones recorded with it off never show up ([mcp]).
- On the Free plan too ([pricing], [tldv]). tl;dv found that search covered titles,
  summaries and notes but **not transcripts**, that long meetings come in 12,000
  character chunks (at most 40,000 per request), and that task tools were listed but
  never loaded ([tldv]).
- The pitch: ask Claude across Wispr and your Granola, Otter or Fireflies connectors at
  once ([mcp]).

### 2.6 Importing from Granola and Otter
- Settings → Notetaker → "Import your meeting notes": Granola as CSV or ZIP; Otter as
  TXT (preferred) or SRT, zipped or as a folder ([import]).
- Imported: dates, titles, summaries (labelled "Summary (Granola AI)" or "Summary
  (Otter AI)"), Granola's notes, transcripts with speaker names and times (Otter TXT).
  Not imported: audio, attendees, links. Imports aren't re-processed and can't use
  "Update Summary" ([import], [summaries]).
- Limits: 50 MB a file; Granola 5,000 rows; Otter 200 MB unpacked, about 4,000
  conversations ([import]). Granola's "Them" stays "Them" ([vs-granola]).

### 2.7 Retention and deletion
- "Notetaker transcript retention" (Settings → Data and Privacy): "Never delete" by
  default, or 1, 7, 30, 90, 180 or 365 days, confirmed with "Auto-delete older
  transcripts?". It deletes from devices and Wispr's servers; titles, notes and
  summaries stay. Enterprise admins can enforce one window, with 24 hours' notice
  ([retention], [rec]).
- Audio: uploaded audio is deleted 7 days after upload; local copies 7 days after the
  last write, which is also the Resume window ([privacy], [retention]).
- Deleting a meeting removes notes, summary, transcript, audio and speaker data on
  every device, for good. Part of a meeting can't be deleted, so withdrawing consent
  means deleting all of it ([retention], [privacy]).

### 2.8 Offline recording
- "Offline" means record now, transcribe in the cloud later: "Record offline" when the
  live transcript is unavailable; banners such as "You're offline. Don't worry, we're
  still recording!" and an "Offline for <duration>…" marker; "Waiting for a
  connection" until the upload finishes ([offline]).
- Uploads over 500 MB fail for good; a full disk stops the recording; there's no live
  translation offline ([offline]).

### 2.9 Languages
- 21 languages: Arabic, Bengali, Chinese (with Cantonese), Czech, Danish, Dutch,
  English, French, German, Hindi, Italian, Japanese, Korean, Marathi, Polish,
  Portuguese, Russian, Spanish, Swedish, Turkish, Urdu ([languages]).
- Detected automatically; you can't pick one, and dictation's language settings don't
  apply ([languages]). tl;dv got US spelling despite a British English account
  ([tldv]).
- The summary follows the main spoken language; the top headings and speaker labels
  stay English ([languages]).
- "This sounds like a language we don't support yet" ("Learn more" / "Got it"), once a
  meeting per device; "Translate transcript" (Beta) on the Transcript tab, online only
  ([languages]).

---

## 3. Hands-on: Wispr Flow for Windows (v1.6.1034)

From the session on Nathan's installed Wispr Flow, 5 Oct 2026 ([hands-on]); the file
names refer to its screenshots. To capture the notepad at all, its "Don't show Notepad
and Flow Bar in screen capture" setting had to be switched off for a moment: it uses
Windows' exclude-from-capture flag, so with it on, no screenshot tool can see those
windows.

- **Hub** (`00-notetaker-hub-empty`): "Notetaker" with a gear and "+ New note"; a dark
  hero card, "Take Notetaker for a test run", with "Try Notetaker" (a guided demo on a
  fake calendar event); **TODAY**: "No meetings found" and "Connect calendar", pitched
  as the way to get speaker names and richer summaries; tabs "Past notes" and "Shared
  with me". The app's left rail: Home, Dictation, Notetaker, Insights, Dictionary,
  Snippets, Style, Scratchpad; Team, Rewards, Settings and Help at the bottom.
- **Heads-up card** (`08-pre-meeting-card`): a dark card at the bottom centre of the
  screen, "Meeting with Tanay · ● In 1 min", with one button: "Join meeting & start
  Notetaker".
- **The notepad is the meeting UI** (`09-notepad-docked-and-capture-warning`,
  `10-notepad-my-thoughts`): starting a note docks a panel to the right edge of the
  screen, full height and about a third of the width. At the top: back, ⋯, Share, a
  link button and window buttons; the title and date (grey until generated); tabs **My
  thoughts** (the default), **Transcript** and **+ Summary**. At the bottom: a line
  reminding you to get consent ("Learn more"), a green ■ **Stop** on the left and
  **What did I miss?** on the right. With capture-hiding off, a Tip toast warned that
  the Flow Bar and notepad show up in screen shares, with **Update settings**.
- **Transcript tab** (`11-transcript-empty-state`, `12-transcript-live-speaker`): the
  elapsed time ("0:24"); a dismissible hint that the transcript will be improved and
  speakers labelled after the meeting; an empty state inviting you to say hello to
  Notetaker; then live lines under a coloured "Speaker 1" label.
- **Recording pill** (`13-flow-bar-recording`): while it records, the Flow Bar is a
  small dark pill above the taskbar with a waveform glyph and a stop button.
- **What did I miss?** (`14-what-did-i-miss`): opens a small chat at the bottom of the
  notepad scoped to what happened **since you last looked**; with nothing new, it says
  so. By then the speaker label had turned from "Speaker 1" into **"You"** (the mic).
- **After Stop** (`15-after-stop`, `16-started-by-mistake-and-buttons`): a toast,
  "Started by mistake? Only a few words were captured…", with **Discard** and
  **Keep**; the notepad offers **Resume** and **+ Generate summary**.
- **AI title** (`17-ai-title`): the note got a generated title, set in the serif
  display face.
- **Summary progress** (`18-summary-generating`): a one-line status ("Turning your
  1-minute meeting into a two-minute read…") with **Step 1 of 3**, and a card, "See
  who said what", offering Connect calendar.
- **Summary error** (`19-summary-error`): a meeting too short to summarise showed "We
  hit a snag processing your meeting" with a retry button; the empty tab says the
  summary is written when you stop.
- **Settings → Notetaker** (`01`–`04`), as shipped, with **Start tutorial** at the
  top right:
  - *Meeting detection*: Notify before scheduled meetings start (15 sec);
    Automatically detect any call (on; it asks whether to start a note when you join
    a call); Maximum recording length (2 hours, with a warning shortly before); Stop
    Notetaker when a call ends (on).
  - *While Notetaker is active*: Don't show Notepad and Flow Bar in screen capture
    (on).
  - *Notepad*: Open Notepad when starting Notetaker (on); Split the screen when
    joining (meeting left, notepad right); Keyboard shortcut (Win + Alt + M, "Change
    shortcut").
  - *Transcript*: Show live transcript (on).
  - *Sharing*: Default for Notetaker links ("Anyone with the link"; transcripts only
    for people invited by email); Auto-share notes ("no one").
  - **Not on this build**, though the help centre documents them: Start Notetaker
    automatically, the Consent section, the triple tap, and the Mac-only menu-bar
    countdown.
  - The Settings sidebar: General, System, Notetaker, Vibe coding, Connectors, MCP;
    then Account, Plans and Billing, Data and Privacy.
- **Connectors** (`05-settings-connectors`): Google Calendar (reminders before
  meetings) and Slack (context for summaries and briefs), plus a pointer to MCP. No
  Outlook on this install.
- **MCP** (`06-settings-mcp`): one-click "Add to Claude / ChatGPT / Gemini / Cursor"
  and "All other apps"; the page notes the MCP can't see your dictations.
- **Scratchpad** (`07-scratchpad-empty`): "Scratchpad (Beta)", pitched for quick
  thoughts to come back to; "Start new note", "Add to Flow Bar", a shortcut slot (off)
  and "Recents".
- **The quality bar** (the write-up's takeaways):
  - The notepad is the meeting UI: notes first, the transcript one tab away, the
    summary when you stop.
  - Every state has a friendly line: a hint, an empty state, a step counter.
  - Guard rails everywhere: the consent footer, the capture warning, "Started by
    mistake?", a maximum length with a warning, a stop when the call ends, a retryable
    summary.
  - "What did I miss?" works from *since you last looked*.
  - Speakers go from "Speaker 1" to "You" to real names once a calendar is connected.

---

## 4. Settings ("Notetaker settings, explained")

The help article's list ([settings]), grouped as it groups them. What the Windows
build actually shows is in [§3](#3-hands-on-wispr-flow-for-windows-v161034).

- **Notifications and detection**
  - "Notify before scheduled meetings start": "Right before the meeting" (15 s,
    default), 1 minute, 2 minutes, Never; per device.
  - "Show your next meeting in the menu bar": on; Mac only.
  - "Automatically detect any call": on; needed for auto-start; off still leaves
    scheduled reminders ([detect]).
  - "Start Notetaker automatically": off; needs detection; a 10 s countdown, 5–60 s
    ([rec]). Not on the Windows build ([hands-on]).
  - "Maximum recording length": 30 minutes, 1, 2 (default) or 3 hours.
  - "Stop Notetaker when a call ends": on; turning it off also turns off the stops
    after long silences.
  - The keyboard shortcut: Option+M on Mac, Win+Alt+M on Windows; changed with "Change
    shortcut" while nothing records ([rec]).
- **While Notetaker is active**
  - "Don't show Notepad and Flow Bar in screen capture": off by default per the
    article; on Windows it needs OS support. **(conflict)** On in the hands-on install
    ([hands-on]).
- **Notepad**
  - "Open Notepad when starting Notetaker": on.
  - "Split the screen when joining": off; on Mac it needs Accessibility.
  - "Triple tap <your dictation key> for Notetaker": on; per device. Not on the
    Windows build ([hands-on]).
- **Transcript**
  - "Show live transcript": on.
- **Consent** (shown only where enabled for the account; not on the Windows build)
  - "Post a consent notice in the meeting chat": follows the organisation (on without
    one); the text is editable up to 450 characters ([consent]).
  - "Calendar notice": Off, Notice only, Notice and notes link; Google Calendar with
    write access.
- **Sharing**
  - "Default for Notetaker links": Private, Shared with team, Anyone with link; an
    organisation can restrict the choice.
  - "Auto-share notes": off; No one, All attendees, or your domain's attendees; needs a
    calendar event and skips attendees who declined.
- **Elsewhere**: Settings → Data and Privacy → "Notetaker transcript retention"
  ([retention]); Settings → Connectors ([connectors]); Settings → MCP ([mcp]); Settings
  → Notetaker → "Import your meeting notes" ([import]); "Improve the model for
  everyone", asked during onboarding ([start]).

---

## 5. Scratchpad

**Desktop (Mac, Windows)**, from [scratchpad] unless noted:
- A floating rich-text notepad window with tabs (five per window), a sidebar with
  search, and images (PNG, JPEG, WebP, GIF, BMP; up to 10 a note, 5 MB each after
  compression; GIFs lose their animation; not on iPhone).
- Opened from the Notes hub ("+"), a shortcut you assign ("Open Scratchpad" in Settings
  → General → Shortcuts; no default today, Option+S at launch, [rel-1.5.113]), or an
  optional Flow Bar button. On Mac it floats without taking focus.
- "Scratchpad open behavior": "Resume last note" (default), "Open in new tab" or "Open
  last active pinned note".
- Dictating: the mic button or the push-to-talk key; a green ✓ inserts, ✕ cancels;
  holding or double-tapping the shortcut starts dictating in a fresh tab when the
  current one has content.
- Autosave; blank notes aren't kept; a title from the first words once a note reaches
  "10 characters or three words".
- Pins (per device, not synced); search with snippets and "Load more" after 20; keys:
  j/k, Enter, c or Ctrl+N, Ctrl+F or "/".
- "Version history": up to 50 versions; edits of the same kind within 10 minutes
  merge; each transform gets its own entry; one click restores.
- Deleting is permanent, history and images included, on every device.
- Sync: saved locally first, synced whenever you're signed in and online, regardless of
  "Dictation Cloud Storage"; only an organisation can turn it off.

**Transforms** ([transforms]): select text anywhere, then the wand or Win+Alt+1/2/O;
built-ins "Polish" and "Prompt Engineer"; up to 8 custom transforms (name, shortcut,
instructions, up to 5 writing samples); 1 to 1,000 words on desktop; the result lands
in place, or in Scratchpad.

**iPhone Notes** ([ios-notes]): in the Scratchpad tab (iOS 18.3+): dictate, ✓ to
insert (not word by word), autosave after about 2 s, markdown rendered when reading, a
title from the first three words, search, swipe to copy, share or delete. Quick
capture from a Lock Screen widget, Control Center, Siri ("save note with Flow"), the
Action Button and Spotlight. HIPAA and Privacy Mode accounts keep notes local, without
AI summaries or sync; signing out deletes local notes.

---

## 6. Privacy model

**Wispr** ([privacy] unless noted)
- *On your computer:* capture (mic and system audio, no bot); a local copy of meeting
  audio for 7 days, for Resume and retries; offline recordings until they upload
  ([offline]); Scratchpad notes before they sync ([scratchpad]).
- *In Wispr's cloud:* live transcription (streamed), the refined transcript and speaker
  naming, summaries, Ask, briefs and MCP. Transcripts and summaries are stored there
  **with no setting to opt out** ([sharing], [start]); uploaded audio for 7 days.
  Third-party AI providers process it as subprocessors (named in the Trust Center, not
  the article). Processing is in the US ([reworked], [tldv]).
- *Training:* "Improve the model for everyone" is on by default for Free and Pro and off
  for Enterprise; turning it off doesn't stop cloud storage ([start]). **(conflict)**
  Wispr's comparison page says data is "never used for model training unless you opt
  in" ([vs-granola]); tl;dv found it on by default ([tldv]).
- *Security:* encrypted in transit and at rest; SOC 2 via the Trust Center; ISO 27001
  and SOC 2 Type II on the pricing page ([pricing]). A HIPAA BAA **blocks** Notetaker
  rather than covering it ([start], [win]).
- *Calendar data kept:* titles, times, links, colours, recurrence, attendees (Google:
  names and photos) and descriptions, not locations.
- *Consent:* your job. Wispr's terms make US customers indemnify Wispr and waive class
  actions over meeting data ([reworked]). No voiceprints ([by-speaking], [summaries]).

**Yap**
- Capture and transcription happen on the PC: mic and WASAPI loopback, transcribed in
  ~15 s chunks on the local engine (`meeting.rs`). The audio lives in memory buffers
  and is never written to disk.
- Transcripts, digests, action plans and chats are local JSON (`notes.json`,
  `chats.json` in `%APPDATA%\yap`), written atomically.
- The AI steps (digests, the action plan, Ask, Chat) use the model the person picks:
  the bundled on-device llamafile keeps everything on the PC; a cloud key they bring
  sends transcript **text**, never audio, to that provider (`meeting_summary.rs`,
  `llm.rs`, `local_llm.rs`).
- Call detection reads Windows' own record of mic use, locally; window titles are
  never logged or stored (`meeting_detect.rs`, [`meetings.md`](./meetings.md)).
- No account needed: a signed-out Yap never contacts the account service. No share
  links. The local API listens on loopback only, behind a token (`bridge.rs`).
- The same consent duty as Wispr: Yap reminds ("Let people know you're taking notes")
  but tells nobody on the call.

**What it means.** "Wispr's Notetaker, with nothing leaving your PC" is a real pitch,
as long as Yap also earns the trust features that come with recording people: a visible
hot-mic state, dictations kept out of meeting notes, and help with consent.

---

## 7. Gap analysis vs Yap

The checklist in §8 and §9 refers to rows by ID. In the Verdict column, **#n** is
item n of the top 10 (§8) and **Rn** an item in the rest of the checklist (§9).

| ID | Feature | Wispr | Yap today | Verdict | Effort |
|---|---|---|---|---|---|
| B1 | Call detection | Asks when you join a call; optional auto-start after a 10 s countdown (docs; not in the Windows build) ([rec], [hands-on]) | Have: `meeting_detect.rs` (Windows' mic-consent record, per-app choice, pop-up or quiet, 30 s fade, tray item) | Yap already better (never records without a click) | — |
| B2 | Calendar | Google and Outlook by OAuth; "Upcoming" 7 days; attendees and links ([detect], [hub]) | Gap | adapt local-first: ICS subscription (#8) | M |
| B3 | Heads-up card, one-button record | 15 s to 2 min before: "Meeting with Tanay · In 1 min" and "Join meeting & start Notetaker" ([settings], [hands-on]) | Gap | adapt local-first, on B2 (#8) | S |
| B4 | Brief | Calendar, Gmail, Slack, past notes, web; up to 3 cited bullets; Mac-only rollout ([prep], [brief-doc]) | Gap | adapt local-first: "last time" from Yap's own notes (R2) | M |
| B5 | Start shortcut | Win+Alt+M; triple tap of the dictation key (docs) ([settings], [hands-on]) | Gap: Record chip in a note; tray item while a detected call is live | replicate: a combo hotkey, no triple tap (R1) | S |
| D1 | Call audio | Every output device on Windows 10 2004+ ([ts-audio]); (conflict) default only ([start]) | Partial: the default output device, picked once at start (`meeting.rs` `build_capture_stream`) | adapt: follow device changes, then all outputs (#2) | M |
| D2 | Dead-side warnings | "isn't hearing audio from your mic", "Other-party audio is silent…", "Could not capture system audio" ([ts-audio]) | Partial: only "falling behind" (`yap-meeting-warning`) | replicate (#2) | S |
| D3 | Meeting notepad | Docked to the right third of the screen: My thoughts / Transcript / Summary, Stop, What did I miss?, consent footer; "Split the screen when joining" ([hands-on]) | Partial: the meeting lives in the main window's Notes view (`NotesView.svelte`) | adapt: a docked notepad window (#3) | M |
| D4 | Live transcript | Cloud-streamed; "Jump to bottom"; markers; "Speaker 1" then "You" ([live], [hands-on]) | Have: local ~15 s chunks, You/Them bubbles (`meeting.rs`, `NotesView.svelte`) | Yap already better (offline); markers come with #1 and R6 | — |
| D5 | Find in a meeting | Ctrl+F or "/" in transcript and summary ([live]) | Gap | replicate (#7) | S |
| D6 | Speaker names | Read from Meet, Zoom and Teams-on-Mac windows after the call; "Speaker N"; name once ([editor], [by-speaking]); Teams on Windows unnamed | Partial: You/Them; attendees and names said decide task owners (`meeting_summary.rs`) | adapt: name "Them" in 1:1 calls (R3); local diarization later (L); skip reading app windows | S |
| D7 | Echo | The refined transcript strips echo from the mic side ([live]) | Have: whole-chunk echo flag (`meeting.rs`) | adapt: frame-level gating (R5) | M |
| D8 | What did I miss? | Recap of what others said since you last looked ([hands-on], [catch-up]) | Gap | replicate (#6) | S |
| D9 | Ask during the call | "Ask anything", edits on request ([editor]) | Have: the note's Ask bar (`commands.rs` `note_ask`) | skip (parity) | — |
| D10 | Dictating mid-call | Dictated words left out; "Dictated" marker ([dictate]) | Gap: the meeting's mic stream keeps running, so dictations land in "You" (`meeting.rs`; nothing in `pipeline.rs` pauses it) | replicate (#1) | S |
| D11 | One dictionary | Shared by dictation and meetings ([by-speaking]) | Partial: meetings get only Whisper's `initial_prompt`; the exact and fuzzy passes run on dictations only (`pipeline.rs`), so the default Parakeet gets none | replicate (#5) | S |
| D12 | Consent | Posts an editable notice into Meet, Zoom, Slack or Teams chat; "Copy consent message" fallback; calendar notice ([consent]); footer in the notepad ([hands-on]) | Partial: a reminder line in Settings → Meetings and in the call prompt | adapt: editable message + Copy; skip auto-posting (#10) | S |
| D13 | Hide from screen share | "Don't show Notepad and Flow Bar in screen capture"; a toast warns when it's off ([settings], [hands-on]) | Gap | replicate (#10) | S |
| D14 | Recording indicator | Flow Bar pill (waveform and stop) above the taskbar ([hands-on]) | Gap: tray and overlay follow dictation only (`tray.rs`) | replicate: tray state and menu (#4) | S |
| D15 | Ending and limits | Stops when the call ends (on); check-ins at 10 min of silence, after the calendar end, after 1 h; max length 30 min to 3 h, default 2 h ([rec], [hands-on]) | Partial: asks "Stop and summarise?" when a detected call ends (`meeting_detect.rs`); nothing else | adapt: check-ins that ask (#4); auto-stop as an option (R4); no hard cap | S |
| D16 | Started by mistake | "Only a few words were captured": Discard / Keep ([hands-on]) | Partial: "Nothing to summarise yet" for empty recordings (`NotesView.svelte`) | replicate (#4) | S |
| D17 | Offline | Records locally, transcribes in the cloud later; 500 MB cap ([offline]) | Have: always local | Yap already better | — |
| A1 | Summary | Recap → Next Steps by owner → Decisions Made with reasons ([summaries]) | Have: Action Plan with code-checked owners and deadlines, rolling digests (`meeting_summary.rs`, `llm.rs`) | Yap already better; adapt: a topic recap and decision reasons (R8) | S |
| A2 | Summary language | Spoken language; English top headings ([languages]) | Partial: no language rule in the prompts (`llm.rs`) | replicate, keeping the parsed headings in English (R8) | S |
| A3 | Progress and errors | "Turning your 1-minute meeting into a two-minute read…", "Step 1 of 3"; "We hit a snag…" with retry ([hands-on]) | Partial: "Catching up on the meeting — part 3 of 12"; a failure is a toast and an error line (`NotesView.svelte` `runAction`) | replicate: inline Try again (R9) | S |
| A4 | Update after more audio | "Update Summary"; "New audio since this summary." ([summaries]) | Partial: "Action plan" re-runs; the stale dot tracks typed notes only (`notes::content_hash`) | replicate (R10) | S |
| A5 | Editable summary | Editable, synced; "Add summary" ([summaries]) | Gap: the Enhanced view is read-only (`markdown.js`) | replicate (R11) | S–M |
| A6 | AI title | Generated when blank ([editor], [hands-on]) | Partial: "Teams call · 5 Oct, 14:30" from detection, otherwise first words | replicate (R7) | S |
| A7 | Tasks | "Tasks" tab with done/total, where enabled ([editor]) | Partial: `- [ ]` render as disabled checkboxes (`markdown.js`) | replicate: tick in place, plus "My tasks" (R12) | M |
| A8 | Copy and export | "Copy summary" (rich and plain); "Copy as Markdown" (the whole meeting) ([sharing]) | Have: Copy markdown / Copy text / Export .md with transcript (`commands.rs` `note_export`) | adapt: rich copy for email; no echo lines in exports (R13) | S |
| A9 | Share links | Links (summary), email invites (transcript), domain sharing, auto-share, web viewer ([sharing], [shared-link]) | Gap | skip for the free core (needs a Yap cloud) | L |
| A10 | Follow-up drafts | "Draft from your notes" ([whats-new]) | Have: custom Actions (`ActionManager.svelte`) | replicate: a built-in "Follow-up email" (R14) | S |
| A11 | Hub and search | Upcoming / Past notes / Shared with me; search ([hub], [hands-on]) | Partial: Notes search matches title and a 120-character preview in the open folder (`NotesView.svelte` `shownNotes`) | replicate: full text, transcripts included (#7) | S |
| Q1 | Citations | Hub answers list up to 5 meetings; inline links; jump to the moment ([ask], [by-speaking]) | Gap: Chat sends each note's first 500 characters (`commands.rs` `rag_context`); no citations (`ChatView.svelte`) | adapt: passages and citation chips (#7) | M |
| Q2 | Date questions | "meetings yesterday"; weeks Monday to Sunday ([ask]) | Gap: `tools::search_notes` has no dates | replicate (#7) | S |
| Q3 | Chats per meeting | "New chat" / "Past chats" ([ask]) | Gap: the note's Ask thread is page state (`NotesView.svelte` `chatThread`) | replicate with `chats.rs` and a note id (R15) | S |
| Q4 | Injection guard | Ignores instructions inside retrieved notes and pages ([ask]) | Gap: no such rule in the Chat and Ask prompts | replicate (R16) | S |
| Q5 | Web search | For questions about the outside world ([ask]) | Gap | skip | — |
| I1 | MCP | Remote server (OAuth) for Claude, ChatGPT, Gemini, Cursor; reads everything; transcripts not searchable ([mcp], [tldv]) | Partial: local REST bridge and "Copy API guide" (`bridge.rs`, `IntegrationsView.svelte`) | adapt local-first: stdio MCP over the bridge (#9) | S–M |
| I2 | Import | Granola CSV/ZIP, Otter TXT/SRT: date, title, summary, transcript ([import], [notetaker-page]) | Gap: Upload takes audio only (`media.rs`) | adapt: Otter, Granola, and Teams/Zoom VTT into meeting notes (R17) | S–M |
| I3 | Connectors | Google Calendar and Slack on the Windows build; docs add Outlook and LinkedIn ([hands-on], [connectors]) | Gap | skip (calendar via ICS, B2) | — |
| I4 | Organisation controls | Admin on/off, consent acknowledgement, no training, privacy rules ([notetaker-page], [consent]) | Gap: no organisations | later: machine policies for IT (R20) | S–M |
| P1 | Retention | Transcripts kept "Never delete" up to 365 days; audio 7 days; delete removes it everywhere ([retention], [privacy]) | Have: local until deleted; no audio on disk | Yap already better; adapt: optional transcript auto-delete (R18) | S |
| P2 | Audio kept, refined pass | Local audio 7 days for Resume and re-processing; a better transcript after the call ([privacy], [launch]) | Gap: audio only in memory (`meeting.rs`) | adapt: opt-in local audio for playback and re-transcription (R19) | M |
| P3 | Languages | 21, detected automatically, no way to pick; "Translate transcript" (Beta) ([languages]) | Have: whatever the model supports; pinned or auto (`selected_language`) | Yap already better; skip translation for now | — |
| P4 | Platforms | Mac and Windows; iPhone view-only ([ios]) | Windows only | skip (Windows-first) | — |
| S1 | Quick capture | Floating notepad on its own shortcut; dictate into it; "Start new note", "Add to Flow Bar" ([scratchpad], [hands-on]) | Gap: notes live in the main window | adapt: a quick-note window and hotkey (R21) | M |
| S2 | Pins, version history | Pins per device; 50 versions, edits within 10 min merge ([scratchpad]) | Gap: `notes.rs` has neither | replicate (R22) | S–M |
| S3 | Tabs, images | Five tabs; up to 10 images a note ([scratchpad]) | Gap | skip | — |
| S4 | Sync | Desktop and iPhone whenever signed in ([scratchpad]) | Gap | skip (local-first; a paid convenience at most) | L |
| S5 | Transforms | Select, then the wand or Win+Alt+1/2/O; "Polish", "Prompt Engineer", 8 custom ([transforms]) | Have: edit/rewrite hotkey, Voice Agent, note Actions | skip (parity) | — |
| S6 | iPhone capture | Lock Screen widget, Siri, Action Button ([ios-notes]) | No mobile app | skip | — |

---

## 8. Build checklist: the top 10

Ranked by value to Yap's users and fit with the local-first, free-core strategy. Each
item: why, what to build, the modules, an e2e plan, and an open question for Nathan.
The e2e plans lean on the suite's existing tools (the stub build, `meeting_detect_simulate`,
`e2e_meeting_feed`, WAV meeting audio, `support/fake-llm.js`); the few new hooks they
need are listed under [Harness additions](#harness-additions).

### 1. Keep dictation out of the meeting transcript (S; D10)
*Why:* Yap's users dictate, and mid-call they'll dictate a Slack reply or an email.
Today those words land in "You", can turn into tasks in the action plan, and travel
wherever the notes go. Wispr leaves them out and marks the gap ([dictate]). (Read from
the code: `meeting.rs` opens its own mic stream and nothing pauses it during a
dictation. Confirm once in dev.)
- [ ] **Build:** while a dictation records, the meeting's mic side gets silence
  instead of audio (plus the last ~300 ms already buffered, the dictation's pre-roll),
  and the transcript gets a "Dictated" marker at that point: a divider in the UI,
  skipped by digests and the action plan.
- **Modules:** `pipeline.rs` (a shared "dictating" flag, set when a recording starts
  and cleared when it stops); `meeting.rs` (one mic-push path, used by the device
  callback and the test player, that writes zeros while the flag is set, keeping `ts`
  and the echo envelope aligned, and notes the span); `notes.rs`
  (`TranscriptSegment.kind`, `#[serde(default)]`, value `"dictated"`);
  `meeting_summary.rs` (skip markers); `NotesView.svelte` (the divider).
- [ ] **e2e** (`meetings.spec.js`): a test-mode hook `YAP_E2E_MEETING_DICTATION=2-12`
  (seconds of test audio during which `e2e::spawn_meeting_audio` holds the flag);
  `you.wav` talks only inside that span. Expect no "you" bubble, a "Dictated" divider
  in the transcript log, a `kind: "dictated"` segment in notes.json, and the "them"
  side untouched. A Rust unit test covers the span bookkeeping. On a machine with a
  mic, a second test presses F24 with `pressHotkey(main)` mid-recording; it's skipped
  on CI like `app.spec.js`'s stub dictation.
- **Open question:** drop the dictated words, as Wispr does, or keep them as a
  private, hidden line?

### 2. Hear the call wherever it plays, and say when a side goes quiet (M; D1, D2)
*Why:* if Teams plays into a headset that isn't Windows' default output, "Them" stays
silent for the whole meeting, the action plan misses everyone else, and nothing says
so. Wispr's newer troubleshooting article says it records every output device on
Windows 10 2004+ and warns when either side goes quiet ([ts-audio]).
- [ ] **Build:** (a) follow default-output changes during a recording by reopening the
  loopback stream; (b) capture every active output: one loopback stream per active
  render endpoint mixed into "Them", or Windows' process-loopback capture of everything
  except Yap, which would also keep Yap's own chimes out of "Them" (spike both, and
  check that process loopback covers non-default devices); (c) "Yap can't hear the
  call. Is Teams playing through another device?" and "Yap can't hear your mic" after
  about a minute of one dead side while the other talks.
- **Modules:** `meeting.rs` (`build_capture_stream`, `spawn_device_capture`, whose
  thread already wakes every 200 ms; the worker already measures each chunk's peak);
  `meeting_detect.rs` (the live call's app name for the message); `NotesView.svelte`
  (already shows `yap-meeting-warning`).
- [ ] **e2e** (`meetings.spec.js`): `them.wav` silent throughout, `you.wav` talking,
  `YAP_E2E_SILENCE_WARN_SECS=3`: the "can't hear the call" warning appears; and the
  mirror case for the mic. Device following goes behind a small device-source trait
  with Rust unit tests, plus one manual dev check with Teams on a non-default headset.
- **Open question:** is "follow the default and warn" enough for a first version, with
  every-device capture after?

### 3. A meeting notepad docked beside the call (M; D3)
*Why:* hands-on, Wispr's notepad *is* the meeting UI: a panel on the right edge, notes
first, the transcript a tab away, Stop and "What did I miss?" at the bottom, a consent
line ([hands-on] 09–12). Yap's meeting lives in the 1200×800 main window, which fights
the call for the screen.
- [ ] **Build:** a `notepad` window (full height, about a third of the width, on the
  right edge of the monitor the call is on, else the one under the cursor) that opens
  when a recording starts (a setting, "Open the notepad when recording starts", on by
  default). It shows the title, tabs **My notes / Transcript / Action plan**, Pause
  and End & summarise, "What did I miss?" (#6), the Ask bar and a consent footer
  (#10); "Open in Notes" hands over to the main window. Closing it doesn't stop the
  recording.
- **Modules:** `tauri.conf.json` (the window, created hidden and unfocused); `lib.rs`
  (the `init_hidden_webview` cloak); a new `src/lib/MeetingNotepad.svelte` built from
  pieces split out of `NotesView.svelte` (transcript bubbles, the Ask bar, the recorder
  chip); `overlay.rs` (its monitor-under-the-cursor helper, for placement);
  `meeting_detect.rs` (Record notes opens the notepad); `commands.rs` (`open_notepad`);
  `config.rs`. Like every Yap window, it needs the in-page hotkey fallback.
- [ ] **e2e:** `support/yap.js` attaches the new window as `yap.notepad`. In
  `meeting-detect.spec.js`, Record notes opens it with the note's title, the tabs, Stop
  and the consent footer; text typed in My notes reaches notes.json; End & summarise
  runs the summary step there; its position (`window.screenX` and `outerWidth` from the
  page) sits against the right edge. Screenshots of each state.
- **Open question:** also put the call on the left, like Wispr's "Split the screen when
  joining"? Moving another app's window is easy on Windows, but intrusive.

### 4. Show that a meeting is recording, plus Wispr's guard rails (S–M; D14, D15, D16)
*Why:* hide Yap's window mid-meeting and nothing on screen says it's still recording;
the tray and overlay follow dictation only, which breaks Yap's own rule that a hot mic
is always visible. Wispr keeps a recording pill with Stop above the taskbar, checks in
on long silences, caps the length, and asks "Started by mistake?" ([hands-on] 13,
15–16; [rec]).
- [ ] **Build:**
  - The tray gets a meeting state (an icon variant and the tooltip "Recording Teams
    call · 12:34") with "Open meeting notes", "Pause" and "End meeting & summarise".
  - Check-ins ask and never stop on their own: after 10 minutes with no speech on
    either side, "Still in a meeting?" (Keep recording / Stop and summarise); and a
    long-meeting reminder after 1, 2 or 3 hours.
  - Stopping a recording that caught only a few words, with no typed notes, asks
    whether to keep it (Yap's wording, Wispr's pattern) with Discard and Keep.
- **Modules:** `tray.rs` (state from `meeting::state()`, refreshed through
  `tray::refresh` like `meeting_detect::sync_tray`); `meeting.rs` (a silence clock
  from the chunk peaks, a word count); `meeting_detect.rs`, `win_toast.rs` and
  `meetingDetect.svelte.js` (the prompt plumbing); `NotesView.svelte` (the stop flow;
  Discard calls `notes::delete`); `config.rs`.
- [ ] **e2e:** tray menus can't be clicked over CDP, so the menu's labels per state get
  Rust unit tests, and a debug-only `tray_items` snapshot command lets
  `meetings.spec.js` assert "End meeting & summarise" while recording. Check-in: a
  silent recording with `YAP_E2E_CHECKIN_SECS=3` shows "Still in a meeting?"; Stop and
  summarise reaches the summary step. Keep-or-discard: record two seconds of silence
  and end it; Discard removes the note from notes.json, Keep leaves it.
- **Open question:** when a check-in is ignored, keep recording (Yap's "nothing stops
  without a click") or pause after another 30 minutes of silence?

### 5. Run the dictionary over meeting transcripts (S; D11)
*Why:* Wispr sells one dictionary for dictation and meetings ([by-speaking]). In Yap,
meeting chunks get the dictionary only as Whisper's `initial_prompt`; the exact
replacements and the fuzzy pass run on dictations only, so with the default Parakeet
model, meetings get no corrections at all. Names matter twice here: the action plan
only accepts owners who are attendees or names said in the meeting.
- [ ] **Build:** apply `config::apply_dictionary`, then `fuzzy::apply_fuzzy` when
  `dictionary_fuzzy` is on and the model isn't Whisper, to every segment in
  `meeting::ingest`, mirroring `pipeline.rs` (around lines 887–898). `ingest` also
  receives the e2e feed, so tests cover it. Do the same for Upload
  (`pipeline::run_file_transcription`).
- **Modules:** `meeting.rs`, `pipeline.rs`, `config.rs`, `fuzzy.rs`.
- [ ] **e2e:** an instance seeded with `dictionary: [{ from: 'jaison', to: 'JSON' },
  { from: 'chat gpt', to: 'ChatGPT' }]`; `e2e_meeting_feed` segments "send the jaison
  file" and "ask Chat G P T": the bubbles and notes.json read "JSON" and "ChatGPT".
- **Open question:** should the fuzzy pass touch other people's speech ("Them")? Each
  entry can already opt out.

### 6. "What did I miss?", since you last looked (S; D8)
*Why:* the most-quoted Notetaker feature at launch ([cw], [renascence]). Hands-on, it
covers what was said since you last looked, and says so when nothing was ([hands-on]
14). A few minutes of transcript is a small input, which the bundled on-device model
handles.
- [ ] **Build:** a "What did I miss?" button (notepad and Notes) while recording. Yap
  remembers when you last looked at this meeting (its transcript on screen in a focused
  window, or the last catch-up). Nothing new: a line saying so in Yap's own words, with
  no AI call. Otherwise, the other side's lines since then (echo left out; anything
  older than about 10 minutes comes from the digests) become 3–5 bullets in the Ask
  thread.
- **Modules:** `commands.rs` (`meeting_catch_up(id, since_ts)` beside `note_ask`);
  `llm.rs` (an immutable `CATCH_UP_PROMPT`: only what was said, never invent);
  `meeting_summary.rs` (digests for long gaps; the Note Formatting endpoint with
  `max_tokens` for local servers); `NotesView.svelte` and the notepad.
- [ ] **e2e:** fake-llm learns a `catchUp` kind (recognised by its prompt's first
  words). Feed ten minutes of segments and click: the reply shows in the thread, and the
  recorded request holds only "Them:" lines after the last look and no echo lines.
  Click again: the nothing-new line, and no new request.
- **Open question:** include your own lines? Wispr leaves them out.

### 7. Answers you can check: passage search and citations (M; Q1, Q2, A11, D5)
*Why:* Wispr cites meetings and jumps to the moment ([ask], [by-speaking]). Yap's Chat
gives the model each matching note's first 500 characters, so something said at
minute 40 rarely reaches it unless a tool-capable model calls `get_note`, and the
bundled 1.5B model can't use tools.
- [ ] **Build:** passage search (about a minute of transcript, or a paragraph of
  notes, with its time) used by Chat and the Ask bar; answers cite `[1]`; a chip opens
  the note with that transcript line in view and highlighted. Date words ("yesterday",
  "last week") filter by meeting date, parsed in Rust. The Notes search box searches
  full text and transcripts; Ctrl+F finds within a meeting.
- **Modules:** `tools.rs` (`search_passages(query, k, from, to)` on the existing
  keyword scorer); `commands.rs` (`rag_context` emits blocks such as
  `<source n="1" note="12" at="40:12">`;
  `chat_send` and `note_ask` return the sources); `ChatView.svelte` (chips);
  `NotesView.svelte` (`noteRequest` gains a transcript time; the search box; Ctrl+F);
  `bridge.rs` (`/v1/notes/search` reuses it, for #9).
- [ ] **e2e** (a new `chat.spec.js`, `llmScopes.chat` pointed at fake-llm): feed a
  60-minute meeting with a unique phrase at minute 40, then ask about it. The recorded
  request carries that passage in a `<source>` block; fake-llm answers with `[1]`; the
  chip opens Notes on that meeting with the line in view. Notes search finds a word
  that's only in a transcript. Dates: seed notes.json with yesterday's and last
  month's meetings (a harness addition); "yesterday" cites only the first.
- **Open question:** keyword passages now, or local embeddings (`fastembed-rs` and
  `sqlite-vec`, as the OpenWhispr teardown suggests)?

### 8. A calendar without a cloud account (M; B2, B3)
*Why:* the calendar is what turns Wispr's notes into "Meeting with Tanay", with
attendees, a heads-up card and one button to join and record ([hands-on] 08,
[detect]). Attendees matter even more to Yap: its action plan only gives tasks to
attendees or names said aloud.
- [ ] **Build:** paste a private ICS address (Google Calendar's secret iCal address,
  or Outlook's published-calendar link) in Settings → Meetings. Yap fetches it itself
  (no Yap server, no OAuth app), lists today's meetings in Notes, shows a heads-up card
  shortly before each ("Weekly sync · in 1 min", with Join & record), and fills the
  note's title and attendees from the event. Call detection matches a call to the event
  in progress, and a call during the next event offers to switch notes (back-to-back
  meetings).
- **Modules:** a new `calendar.rs` (fetch every ~15 minutes while running, iCalendar
  parsing and recurrence expansion, the next 7 days in memory); the ICS address is a
  secret, so it goes to Windows Credential Manager like the account token
  (`keyring-core`); `meeting_detect.rs` (event matching, the card); `notes.rs` (title,
  participants); `win_toast.rs` and `meetingDetect.svelte.js` (the card);
  `Settings.svelte` (Meetings group); `NotesView.svelte` (today's list).
- [ ] **e2e** (a new `calendar.spec.js`): a local server, like
  `support/update-feed.js`, serves an .ics generated at test time with an event
  starting in 60 seconds and two attendees; a test-mode `YAP_E2E_CALENDAR_URL` stands
  in for Credential Manager. Notes lists the event; the heads-up toast appears; Join &
  record creates a note with the event's title and attendees, recording;
  `meeting_detect_simulate('teams', true)` during the event asks nothing more.
  Recurrence and time zones get Rust unit tests.
- **Open question:** ICS (no server, read-only; how much attendee detail each provider
  includes needs checking) or real OAuth? And is a network call acceptable in the
  meetings feature, given that it's configured by the person and goes only to their
  calendar provider?

### 9. A local MCP server over the bridge (S–M; I1)
*Why:* MCP is on Wispr's Free plan and in its headline claims ([mcp],
[notetaker-page]). Yap already has the local bridge. A local server hands Claude
Desktop, Claude Code, Cursor or VS Code your meetings without uploading them first,
and can search transcripts, which tl;dv found Wispr's doesn't ([tldv]).
- [ ] **Build:** a stdio MCP server with read tools: search (the passages from #7),
  get note (with its transcript, in chunks), list meetings by date, open action items.
  Write tools (create note) are off by default. Integrations gets a "Use with Claude
  Desktop / Cursor" card with copyable config.
- **Modules:** a second small binary (`src-tauri/src/bin/yap-mcp.rs`) or a `--mcp`
  mode in `main.rs`, on the official Rust SDK (`rmcp`), talking to the running app
  through the bridge (`~/.yap/cli-bridge.json` and its token), so Yap stays the only
  writer of notes.json; `bridge.rs` (passage search, meetings by date, tasks);
  `IntegrationsView.svelte`; the NSIS installer (ship the binary); `docs/local-api.md`.
- [ ] **e2e:** test mode lets the bridge's discovery file live in the run folder
  (`YAP_BRIDGE_FILE`; the suite keeps the bridge off today because that file lives in
  `~/.yap`). A new `mcp.spec.js` starts the MCP binary with the same variable, speaks
  JSON-RPC over stdio (initialize, tools/list, tools/call search) and gets back a
  passage from a fed meeting; plus an Integrations screenshot. Tool schemas get Rust
  unit tests.
- **Open question:** a separate binary or a flag on `yap.exe`? Read-only by default?
  Should it also work while Yap is closed, reading notes.json directly?

### 10. Screen-share and consent hygiene (S; D12, D13)
*Why:* Wispr hides its notepad from screen capture (Windows' exclude-from-capture
flag), warns when that's off ([hands-on] 09), and keeps a consent reminder under the
notes ([hands-on] 10). Meeting recording is in a legal spotlight ([reworked]), and a
local-first tool has something reassuring to say in its consent message.
- [ ] **Build:** "Hide Yap from screen sharing" in Settings → General → Meetings (on
  while a meeting records, by default), with a one-time warning toast when it's off and
  a recording starts. An editable consent message (about 450 characters at most, with
  Reset to default), a "Copy consent message" action on the call prompt and in the
  notepad, and a consent footer in the notepad.
- **Modules:** `config.rs`; `commands.rs` and `lib.rs` (Tauri's `set_content_protected`
  on `settings`, the notepad and the overlay; on Windows that's
  `SetWindowDisplayAffinity`, so confirm it uses `WDA_EXCLUDEFROMCAPTURE` and not the
  black-box `WDA_MONITOR`); `Settings.svelte`; `meetingDetect.svelte.js`;
  `NotesView.svelte` and the notepad.
- [ ] **e2e:** the toggle persists to config.json; a debug-only `window_capture_state`
  command reports each window's display affinity, excluded while recording and cleared
  after; Copy consent message hands the saved text to a stubbed
  `navigator.clipboard.writeText`, as `meeting-summary.spec.js` does. Check that the
  suite's own screenshots, which CDP renders from the page, still work with the flag
  on.
- **Open question:** the default wording; hide during meetings by default, or always?

---

## 9. Build checklist: the rest

The remaining parity items, in lifecycle order, in the same shape (build, modules,
e2e). IDs refer to the table in §7.

**Before the call**
- [ ] **R1 · A meeting hotkey (B5), S.** *Build:* `meeting_hotkey` (a combo spec like
  `edit_hotkey`): with no meeting, a new meeting note that starts recording (and opens
  the notepad); while recording, End & summarise. *Modules:* `config.rs`,
  `input_hook.rs`, `lib.rs` routing, `commands.rs`, the hotkey row in `Settings.svelte`.
  *e2e:* the in-page fallback, as `pressHotkey` does for F24, with its own test key: a
  recording meeting note, then the summary step.
- [ ] **R2 · A "last time" brief (B4), M, after #8.** *Build:* with the heads-up card,
  the previous notes with the same attendees or recurring title: open tasks, decisions
  and a line of context, all local. *Modules:* `calendar.rs`, `tools.rs` (search), the
  stored action plans, the notepad. *e2e:* seed notes.json with an earlier "Weekly
  sync" holding an open task; the card and notepad show it.

**During the call**
- [ ] **R3 · Name "Them" in 1:1 calls (D6), S.** *Build:* with exactly one other
  attendee, bubbles, prompts and exports use their name. *Modules:*
  `NotesView.svelte`, `meeting_summary.rs` (the transcript lines it sends),
  `commands.rs` (`note_export`). *e2e:* `meetings.spec.js` with one attendee: bubbles
  say "Priya", and the recorded action-plan request has "Priya:" lines.
- [ ] **R4 · Optional auto-stop when the call ends (D15), S.** *Build:* a setting (off
  by default) that replaces the question with "Stopping in 15 s" and Keep recording;
  Resume stays one click away. *Modules:* `meeting_detect.rs`, `config.rs`,
  `meetingDetect.svelte.js`. *e2e:* `meeting-detect.spec.js` with the setting on:
  simulate the call ending, the countdown (shortened like `fadeMs`) runs out, the
  summary step follows; Keep recording cancels it.
- [ ] **R5 · Finer echo removal (D7), M.** *Build:* gate mic frames by the loopback
  envelope inside a chunk, so a chunk with both your speech and the call's echo keeps
  only yours. *Modules:* `meeting.rs`. *e2e:* the stub engine only reports durations,
  so this is Rust unit tests on the gating; the existing echo test must still pass.
- [ ] **R6 · Pause markers (D4), S.** *Build:* Pause and Resume leave a "Paused
  12:34–12:40" divider, on the marker field from #1. *Modules:* `meeting.rs`,
  `notes.rs`, `NotesView.svelte`. *e2e:* Pause, then Resume, in `meetings.spec.js`:
  the divider is there.

**After the call**
- [ ] **R7 · AI title (A6), S.** *Build:* when the title is still the default ("Teams
  call · …" or empty), the action-plan run also asks for a title of up to six words.
  *Modules:* `commands.rs` (`note_enhance`), `llm.rs`, `notes.rs`. *e2e:* fake-llm
  returns a title, which reaches notes.json and the header; a title the person typed is
  never replaced.
- [ ] **R8 · Summary in the meeting's language, plus a recap (A1, A2), S.** *Build:*
  `ACTION_PLAN_BASE_PROMPT` and `MEETING_DIGEST_PROMPT` say to write in the
  transcript's language and keep the given headings exactly (parsing stays on English
  headings); the default Action Plan layout gains a short topic recap and a reason per
  decision. *Modules:* `llm.rs`, `meeting_summary.rs`. *e2e:* the recorded request
  carries the rule; Rust tests parse a plan with non-English bullets.
- [ ] **R9 · Inline progress and retry (A3), S.** *Build:* "Writing your action plan
  (step 2 of 3)", an inline Try again after a failure, and friendly empty states (where
  the transcript will appear; that the plan is written when the meeting ends).
  *Modules:* `NotesView.svelte` and the notepad. *e2e:* fake-llm fails the next request
  once (a new switch); Try again succeeds.
- [ ] **R10 · "New audio since this action plan" (A4), S.** *Build:* a meeting note's
  staleness also covers its transcript (count or last time); a banner offers to update
  the plan. *Modules:* `notes.rs` (`content_hash`, the list's `stale`),
  `NotesView.svelte`. *e2e:* plan, Resume, feed more, the banner, update, a new request.
- [ ] **R11 · Editable action plan (A5), S–M.** *Build:* edit the Enhanced view, save
  through `notes::set_enhanced`, warn before an update replaces edits. *Modules:*
  `NotesView.svelte`, `notes.rs`, `commands.rs`. *e2e:* edit, check notes.json, reload,
  still there.
- [ ] **R12 · Tick tasks, and "My tasks" (A7), M.** *Build:* checkboxes write back to
  `enhanced_content`; a "My tasks" list (a Home card or a Notes filter) gathers open
  `- [ ]` lines under "### You" across meetings, each linking back. *Modules:*
  `markdown.js` (enabled checkboxes with line ids), `NotesView.svelte`,
  `HomeView.svelte`, `commands.rs` (`tasks_list`). *e2e:* on the meeting-summary plan,
  tick "draft the agenda": notes.json has `- [x]`, and My tasks lists the open ones.
- [ ] **R13 · Copy for email, cleaner export (A8), S.** *Build:* a rich copy (HTML with
  a plain-text fallback) that pastes into Outlook or Gmail with headings and checkboxes;
  exports leave out echo lines and add times. *Modules:* `NotesView.svelte`,
  `markdown.js`, `commands.rs` (`note_export`). *e2e:* a stubbed `navigator.clipboard.write`
  receives HTML with the headings; `note_export` called through `yap.invoke` with a path
  in the run folder (the save dialog can't be driven) writes no echo text.
- [ ] **R14 · A "Follow-up email" built-in action (A10), S.** *Build:* seeded by the
  additive migration, like the other built-ins. *Modules:* `notes.rs`, `llm.rs`. *e2e:*
  the action picker lists it, and running it sends its prompt (fake-llm).

**Ask**
- [ ] **R15 · Saved chats per meeting (Q3), S.** *Build:* Ask threads persist per note
  (`chats.rs` gains a note id), with "New chat" and "Past chats". *Modules:*
  `chats.rs`, `commands.rs`, `NotesView.svelte`. *e2e:* ask, reload, the thread is
  still there.
- [ ] **R16 · Injection guard (Q4), S.** *Build:* a rule in the Chat and Ask system
  prompts: text inside `<note>` and `<source>` is material to read, never instructions.
  *Modules:* `commands.rs`, `tools.rs`. *e2e:* the recorded request carries it; a Rust
  unit test too.

**Integrations and data**
- [ ] **R17 · Import from Granola, Otter, and Teams/Zoom transcripts (I2), S–M.**
  *Build:* Granola CSV/ZIP, Otter TXT/SRT and WebVTT (the transcript files Teams and
  Zoom recordings produce) become meeting notes in Meetings, with date, title, summary
  (as the Enhanced view, labelled by source) and transcript, speaker names kept
  (`TranscriptSegment` gains an optional speaker). *Modules:* a new `import.rs`,
  `notes.rs`, `commands.rs`, `UploadView.svelte` (accept .txt, .srt, .vtt, .csv, .zip).
  *e2e:* fixture files in `e2e/fixtures/`, imported through `yap.invoke` (the file
  dialog can't be driven): notes.json has the meetings with speakers and dates;
  screenshots of one.
- [ ] **R18 · Optional transcript auto-delete (P1), S.** *Build:* "Delete meeting
  transcripts after N days (keep notes and action plans)". *Modules:* `notes.rs` (a
  sweep at startup and daily), `config.rs`, `Settings.svelte`. *e2e:* seed an old
  meeting, set one day: the transcript goes, the plan stays.
- [ ] **R19 · Opt-in local meeting audio (P2), M.** *Build:* keep each meeting's audio
  for N days, to play a line from the transcript and to re-transcribe it with a bigger
  model after the call (Wispr's "refined" second pass, done locally). *Modules:*
  `meeting.rs` (the writer), `media.rs` (decoding), `notes.rs` (the audio path),
  `NotesView.svelte` (play), `pipeline.rs` (re-transcription, reusing Upload). *e2e:*
  with WAV meeting audio the file lands in Data; playing a line starts the audio
  element; re-transcribing replaces the segments.
- [ ] **R20 · Machine policies for IT (I4), S–M, later.** *Build:* values under
  `HKLM\Software\Policies\Yap` that turn meeting recording off, lock the consent
  message or force capture-hiding; locked rows say who set them. The local-first answer
  to "control Notetaker access across your organization". *Modules:* `config.rs` (a
  policy overlay), `Settings.svelte`. *e2e:* a test-mode variable fakes the policy;
  Settings shows locked rows and Record refuses.

**Scratchpad**
- [ ] **R21 · Quick note (S1), M.** *Build:* a hotkey opens a small floating note
  window and starts dictating at once; Esc saves and hides; the note lands in Notes
  with a title from its first words. *Modules:* a new `quicknote` window in
  `tauri.conf.json` (created unfocused and cloaked like the others), `config.rs`
  (`quick_note_hotkey`), `input_hook.rs` and `lib.rs` routing, a new
  `QuickNote.svelte` with the in-page hotkey fallback. Dictating into it already works:
  with one of Yap's own windows in front, `text_injector::foreground_window` returns 0
  and the paste lands in the focused field. *e2e:* attach the window; a stub dictation
  (skipped without a mic) or typed text lands in a new note in notes.json.
- [ ] **R22 · Pins and version history (S2), S–M.** *Build:* `pinned` on notes;
  `versions` (up to about 50, edits within 10 minutes merged) with a restore menu.
  *Modules:* `notes.rs`, `NotesView.svelte`. *e2e:* edit twice, restore the first:
  notes.json matches.
- [ ] **Later: local diarization (D6), L.** Separate the people behind "Them" on the
  loopback stream (speaker embeddings and clustering on the PC), then name each once.

### Harness additions
- [ ] `launchYap({ data })` seeds data files (notes.json, chats.json) next to
  config.json (for #7's dates, R2, R18).
- [ ] Test-mode hooks in `e2e.rs`: `YAP_E2E_MEETING_DICTATION` (#1),
  `YAP_E2E_SILENCE_WARN_SECS` (#2), `YAP_E2E_CHECKIN_SECS` (#4), `YAP_E2E_CALENDAR_URL`
  (#8), `YAP_BRIDGE_FILE` (#9); debug-only commands `tray_items` (#4) and
  `window_capture_state` (#10).
- [ ] `support/fake-llm.js`: `catchUp`, `chat` (scripted replies with `[1]`) and
  `title` kinds, and a fail-next switch (R9).
- [ ] `support/yap.js`: attach the `notepad` (#3) and `quicknote` (R21) windows.
- [ ] `support/calendar-feed.js` (an ICS server, #8), `support/mcp.js` (a tiny stdio
  JSON-RPC client, #9), and `e2e/fixtures/` (Otter TXT and SRT, Granola CSV, VTT;
  R17).
- [ ] Keep the coverage table in [`e2e-tests.md`](./e2e-tests.md) up to date as specs
  land.

### Deliberately skipped
- **Share links, the web viewer, email invites and auto-share** (A9): they need a Yap
  cloud. A possible paid convenience later, in line with the monetisation plan (paid =
  convenience, never the local core); Nathan's call.
- **Posting the consent notice into meeting chats** (D12): automating other apps' UIs
  is fragile, and Wispr's own article lists a dozen ways it fails ([consent]). The copy
  button (#10) does the job.
- **Reading speaker names from meeting apps' windows** (D6): fragile; even Wispr leaves
  Teams on Windows unnamed and needs English interfaces.
- **Cloud briefs** (Gmail, Slack, web research), **the Slack connector** and **web
  search in Ask**: cloud-shaped; R2 is the local version.
- **An auto-start countdown** (B1): the Windows build doesn't offer it either
  ([hands-on]); "one button joins and records" is #8.
- **A hard maximum length** (D15): Yap's memory stays bounded however long a meeting
  runs, and #4's check-ins catch the forgotten recording.
- **Weekly allowances, upload caps, Cloud Sync gates, HIPAA BAA flows**: SaaS-shaped.
- **Scratchpad tabs, images and sync; iPhone capture; transcript translation** (S3, S4,
  S6, P3): later, if people ask.

---

## 10. Where Wispr's sources disagree

- **Auto-start:** a 10 s countdown with "Start Notetaker automatically" ([rec],
  [settings]); "never starts on its own" and a "Transcribe this meeting with Wispr?"
  prompt ([dictate]); and neither on the Windows build, whose setting says it asks
  whether to start a note when you join a call ([hands-on]).
- **Windows system audio:** the default output device only ([start]) versus every
  output device on build 19041+ ([ts-audio]).
- **Free plan:** "Limited", with a weekly limit ([pricing], [rec]) and 100 meeting
  notes a week on paid plans ([hub]), versus "Notetaker isn't limited" on Free
  ([plans]).
- **Training:** on by default for Free and Pro ([start], [tldv]), versus "never used
  for model training unless you opt in" ([vs-granola]); the landing page lists "no
  model training" among the business controls ([notetaker-page]).
- **Capture-hiding default:** off ([settings]); on in the hands-on install
  ([hands-on]).
- **The join button:** "Join and record" ([rec]), "Join + Start" ([detect], [hub]),
  "Join meeting & start Notetaker" on the card ([hands-on]).
- **The stop setting:** "Stop Notetaker when a call ends" ([settings], [hands-on])
  versus "Stop the Notetaker when the meeting ends" ([ts-audio]).
- **Settings on Windows:** no Consent section, no triple tap and no auto-start
  ([hands-on]), though documented ([settings]); gated per account, or not yet built
  for Windows.
- **Connectors:** Outlook supported since 15 Sep ([releases]), yet the Windows install
  listed only Google Calendar and Slack ([hands-on]).
- **Transcript retention:** documented ([retention]); tl;dv couldn't find it in the app
  ([tldv]).
- **"What did I miss?":** "the last stretch" of the call ([catch-up]) versus "since you
  last looked" ([hands-on]).
- **The brief:** the help article (Mac only, rolling out) was gone by 5 Oct
  ([brief-doc]); marketing describes it without caveats ([prep]).

---

## Sources

**Hands-on**
- The coordinator's session with Wispr Flow for Windows v1.6.1034, 5 Oct 2026:
  `E:\Projects\references\wispr-flow\README.md` and its `screenshots\` folder, kept
  outside the repo ([hands-on]).

**Wispr help centre** (all read 5 Oct 2026)
- Getting started with Notetaker (beta) ([start])
- Recording a meeting with Notetaker (beta) ([rec])
- Meeting detection, reminders, and calendar in Notetaker (beta) ([detect])
- The live transcript in Notetaker (beta) ([live])
- Dictating during a meeting with Notetaker (beta) ([dictate])
- Meeting notes and the editor in Notetaker (beta) ([editor])
- Flow Summaries in Notetaker (beta) ([summaries])
- The Notetaker hub: your meetings in one place (beta) ([hub])
- Troubleshooting Notetaker transcripts, summaries and the hub (beta) ([ts-notes])
- Troubleshooting Notetaker recording and audio (beta) ([ts-audio])
- Transcript retention, syncing and deleting meetings in Notetaker (beta)
  ([retention])
- Ask Wispr: chat with your meetings and notes (beta) ([ask])
- Sharing meeting notes from Notetaker (beta) ([sharing])
- Notetaker settings, explained (beta) ([settings])
- Switching to Wispr Flow: keep all your notetakers in one place (MCP) ([mcp])
- Offline meeting recording in Notetaker ([offline])
- The meeting recording consent notice ([consent])
- Use Notetaker with multiple languages ([languages])
- Notetaker privacy and security overview ([privacy])
- Import your past meetings from Granola or Otter into Notetaker ([import])
- Upcoming meetings in Notetaker on iOS ([ios])
- Connectors in Wispr Flow ([connectors])
- Using the Scratchpad to save and edit notes ([scratchpad])
- Using Notes in Wispr Flow for iOS ([ios-notes])
- What you see when you open a shared Wispr Flow note link ([shared-link])
- How to use Transforms (beta) ([transforms])
- Flow plans and what's included ([plans])
- Pre-reads: get briefed before your meetings (beta) ([brief-doc]); it returned 404
  on 5 Oct, so it's summarised from the search index.

**Wispr marketing and changelog**
- The Wispr Notetaker landing page ([notetaker-page])
- How to take meeting notes by voice, 29 Aug 2026 ([by-speaking])
- How to catch up on a meeting you missed, 28 Aug 2026 ([catch-up])
- How to prepare for a meeting with AI, 28 Aug 2026 ([prep])
- Wispr Notetaker on Windows: setup and notes, 15 Sep 2026 ([win])
- Wispr Notetaker vs Granola ([vs-granola])
- Wispr Flow Notetaker is here, 5 Aug 2026 ([launch])
- Pricing ([pricing])
- What's new ([whats-new])
- Wispr Flow releases on releases.sh ([releases])
- Desktop v1.5.113: Scratchpad, Transforms, 1 May 2026 ([rel-1.5.113])

**Press and reviews**
- Computerworld, "Wispr moves beyond AI dictation with note-taking assistant", 7 Aug
  2026 ([cw])
- Reworked, on Wispr entering the AI meeting-notes market, 5 Aug 2026 ([reworked])
- TechCrunch, on the updated terms that pointed to a notetaker, 5 Aug 2026 ([tc])
- Renascence, on Notetaker and its pre-meeting briefs, 8 Aug 2026 ([renascence])
- tl;dv, "Is Wispr Flow Notetaker worth it in 2026?", 25 Sep 2026; written by a
  competitor, which it discloses ([tldv])

[hands-on]: ../../references/wispr-flow/README.md
[start]: https://docs.wisprflow.ai/articles/3665250541-getting-started-with-notetaker-beta
[rec]: https://docs.wisprflow.ai/articles/9238501024-recording-a-meeting-with-notetaker-beta
[detect]: https://docs.wisprflow.ai/articles/8955305188-meeting-detection-reminders-and-calendar-in-notetaker-beta
[live]: https://docs.wisprflow.ai/articles/4091106439-the-live-transcript-in-notetaker-beta
[dictate]: https://docs.wisprflow.ai/articles/8175153619-dictating-during-a-meeting-with-notetaker-beta
[editor]: https://docs.wisprflow.ai/articles/9406970664-meeting-notes-and-the-editor-in-notetaker-beta
[summaries]: https://docs.wisprflow.ai/articles/1422535682-flow-summaries-in-notetaker-beta
[hub]: https://docs.wisprflow.ai/articles/9247277218-the-notetaker-hub-your-meetings-in-one-place-beta
[ts-notes]: https://docs.wisprflow.ai/articles/4251565280-troubleshooting-notetaker-transcripts-summaries-and-the-hub-beta
[ts-audio]: https://docs.wisprflow.ai/articles/3089221553-troubleshooting-notetaker-recording-and-audio-beta
[retention]: https://docs.wisprflow.ai/articles/1761520520-transcript-retention-syncing-and-deleting-meetings-in-notetaker-beta
[ask]: https://docs.wisprflow.ai/articles/5694642921-ask-wispr-chat-with-your-meetings-and-notes-beta
[sharing]: https://docs.wisprflow.ai/articles/5073796184-sharing-meeting-notes-from-notetaker-beta
[settings]: https://docs.wisprflow.ai/articles/9319084321-notetaker-settings-explained-beta
[mcp]: https://docs.wisprflow.ai/articles/5166327995-switching-to-wispr-flow-keep-all-your-notetakers-in-one-place-mcp
[offline]: https://docs.wisprflow.ai/articles/5810080970-offline-meeting-recording-in-notetaker
[consent]: https://docs.wisprflow.ai/articles/3808660402-the-meeting-recording-consent-notice-how-it-s-posted-and-how-admins-control-it
[languages]: https://docs.wisprflow.ai/articles/3512887047-use-notetaker-with-multiple-languages
[privacy]: https://docs.wisprflow.ai/articles/4497184932-notetaker-privacy-and-security-overview
[import]: https://docs.wisprflow.ai/articles/4484290718-import-your-past-meetings-from-granola-or-otter-into-notetaker
[ios]: https://docs.wisprflow.ai/articles/3693950131-upcoming-meetings-in-notetaker-on-ios
[connectors]: https://docs.wisprflow.ai/articles/9962971864-connectors-in-wispr-flow-connect-disconnect-and-organization-blocked-connectors
[scratchpad]: https://docs.wisprflow.ai/articles/9618237082-using-the-scratchpad-to-save-and-edit-notes
[ios-notes]: https://docs.wisprflow.ai/articles/3529886556-using-notes-in-wispr-flow-for-ios
[shared-link]: https://docs.wisprflow.ai/articles/2228313746-what-you-see-when-you-open-a-shared-wispr-flow-note-link
[transforms]: https://docs.wisprflow.ai/articles/8068950331-how-to-use-transforms-beta
[plans]: https://docs.wisprflow.ai/articles/9559327591-flow-plans-and-what-s-included
[brief-doc]: https://docs.wisprflow.ai/articles/5718486414-pre-reads-get-briefed-before-your-meetings-beta
[notetaker-page]: https://wisprflow.ai/notetaker
[by-speaking]: https://wisprflow.ai/notetaker/take-meeting-notes-by-speaking
[catch-up]: https://wisprflow.ai/notetaker/how-to-catch-up-on-a-meeting-you-missed
[prep]: https://wisprflow.ai/notetaker/how-to-prepare-for-a-meeting-with-ai
[win]: https://wisprflow.ai/notetaker/wispr-notetaker-windows
[vs-granola]: https://wisprflow.ai/notetaker/vs-granola
[launch]: https://wisprflow.ai/post/wispr-flow-notetaker
[pricing]: https://wisprflow.ai/pricing
[whats-new]: https://wisprflow.ai/whats-new
[releases]: https://releases.sh/wispr-flow/releases
[rel-1.5.113]: https://releases.sh/release/rel_eLZUErhaqh55ehPvFTw76
[cw]: https://computerworld.com/article/4206765/wispr-moves-beyond-ai-dictation-with-note-taking-assistant.html
[reworked]: https://www.reworked.co/collaboration-productivity/wispr-enters-crowded-ai-meeting-notes-market-with-notetaker/
[tc]: https://techcrunch.com/2026/08/05/wispr-flow-is-preparing-to-launch-a-meeting-notetaker-updated-terms-suggest/
[renascence]: https://www.renascence.io/news/17221/wispr-flow-notetaker-ai-meeting-tool-with-pre-meeting-briefs
[tldv]: https://tldv.io/blog/wispr-flow-notetaker-review/
