# Call detection and meeting guard rails

Noticing a call and offering to take notes, the call apps' own icons, the guard rails around a recording (length limit, quiet meetings, check-ins, the meeting shortcut) and hiding meeting windows from screen capture. See [`docs/meetings.md`](../meetings.md) "Call detection" and "Guard rails".

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`meeting_detect.rs`** — **call detection** (OpenWhispr
  `meetingDetectionEngine.js` port): notices a call starting (Teams, Zoom,
  Google Meet, Slack huddles, Discord, Webex, GoTo, WhatsApp/Signal/Telegram…),
  offers to take notes, and offers to stop and summarise when it ends. Signal =
  Windows' per-app **microphone consent store** (`HKCU\…\CapabilityAccessManager\
  ConsentStore\microphone`: packaged apps + `NonPackaged\<exe path, \ → #>`;
  `LastUsedTimeStop == 0` = on the mic now, ignored if it predates this boot),
  read for a known-call-app allowlist only (`APPS`); a browser counts when one
  of its windows shows a meeting (Meet/Teams/Zoom… in the title; the call
  sticks while you switch tabs). Yap's own mic use never counts (`yap.exe`
  isn't a call app — it holds the mic while it dictates and a minute after,
  `mic.rs`, or the whole time with "Keep the microphone ready"). Event-driven:
  a thread blocks in `RegNotifyChangeKeyValue`; the detector rescans every
  2 s only while a call is starting/live/ending (60 s safety net; 5 s polling
  if the watch fails). Debounce: starts after 5 s on the mic (20 s for chat
  apps — voice notes), ends after 15 s off it (a device switch or rejoin is a
  gap). **Per-app choice**: each `APPS` entry has `asks_by_default` (work apps
  — Teams, Zoom, Meet, Webex, Slack, GoTo, Whereby, Jitsi — yes; Discord,
  WhatsApp, Signal, Telegram no); the person's overrides live in
  `config.meeting_detect_apps` (app id → bool; effective = override, else the
  default, so apps added later start at theirs). A switched-off app's calls
  are still tracked but get no prompt (start or end) and no tray item. Asks
  **once per call**, never while already recording, in one of two styles
  (`config.meeting_detect_style`): **"popup"** (default) — in **one place**
  (`notice::place_for`): a sticky in-app toast while the main window is
  focused, else a card on the **Yap bar** (`native::bar_card`: Wispr's
  "Meeting detected" card — the app's icon (its own as installed,
  `app_icons.rs`, else its mark), "Teams call" over "● Now", split button
  Record notes | ^ Not now / Don't ask for Teams, corner ✕; the call ending
  asks in a notice card), else — the bar off or hidden — a silent Windows
  notification via `win_toast.rs`, else the in-app toast if the window is on
  screen (also when Windows refuses the notification); focusing the window
  moves a pending prompt in-app (`on_main_window_focused`, off the bar), and
  a start prompt left alone **fades after 30 s as "Not now"** (OpenWhispr's
  auto-dismiss; `fadeMs` in the prompt view, the toast's `onExpire`; it
  answers `ignore`, a "Not now" that fatigue counts apart) — or **"quiet"** —
  no in-app toast, the notification goes
  silently into the notification centre (`win_toast::post_quietly` →
  `SetSuppressPopup`) and stays out of the window unless its body is clicked.
  Both styles: start prompts carry a third answer, **"Don't ask for Teams"**
  (`never`: saves the override, withdraws the prompt everywhere, emits
  `yap-meeting-detect-choice {app, asks, confirm}` so Settings adopts it and,
  window on screen, a "Won't ask about Teams calls" toast links to Settings →
  Meetings → Ask about calls in); the end prompt follows the style. Prompts wait for a
  dictation to finish (+2.5 s); "Not now" snoozes that app for 5 min
  (OpenWhispr's cooldown). Opt-in **"Start notes automatically after 10
  seconds"** (`meeting_auto_start`, Wispr's countdown; off by default because
  nothing records without a click): a start prompt that's sure to be seen (a
  focused window's toast, or the bar's card) counts down (`auto_start_at` in
  the prompt view; Start now / Not now / Esc cancel), then runs "Record notes"
  (`auto_start_later`, after any dictation); the same app's call again within
  10 min (a rejoin, a reload) is asked about without one. In a test run the
  suite says whether the main window counts as focused (`notice::window_view`:
  by default while it's on screen; `e2e_focus`). **Call-prompt fatigue**
  (`notice::Fatigue`, Wispr's by behaviour; `<data>/notices.json`): three
  "Not now"s in a row, or five start prompts left to fade, and "Stop asking
  about calls?" is offered once (`offer_to_stop`, an alert with **Stop
  asking**); 35 start prompts with no meeting ever recorded and call
  detection switches itself off with "Yap stopped asking about calls" +
  **Turn back on** (`stopped_asking`); Record notes / Don't ask reset the
  streaks; someone who has recorded a meeting (≥ 20 words) is never asked.
  `set_detection` saves the switch and tells Settings
  (`yap-meeting-detect-choice {enabled}`). **Tray**: while a call of an asked-about app is live
  and nothing records, `tray_item()` puts "Record this Teams call" at the top
  of the idle menu (`meeting_record:<call id>` → `on_tray_record`, same as
  Record notes); `sync_tray` calls `tray::refresh` when it changes. **Record
  notes** → `notes::create` ("Teams call · 5 Oct, 14:30", folder Meetings,
  source "meeting", `set_title_auto` so the AI title may replace it) +
  `notes::mark_meeting` + the `meeting_start` command (which opens the
  notepad); opens the note if the window is visible — that's the
  confirmation, no toast on top — (else a "Taking notes…" bar card or Windows
  notification); a failed start deletes the note and says why.
  **Call ended** while recording it → "Stop and summarise?" — by default never
  an auto-stop (OpenWhispr doesn't, and the mic can't tell an ended call from
  a rejoin, breakout room or phone hand-off): `stop_and_summarise` ends the
  meeting (`meeting_end::end` — Rust writes the Action Plan) and shows it
  where the person looks: the notepad if it's on screen, the note in Notes
  (`yap-meeting-open-note {noteId, stop: false}`) if the main window is, else
  the main window comes up on it. Opt-in **"When a call
  ends: Stop and summarise automatically"** (`config.meeting_call_end` "ask" | "stop",
  Wispr's "Stop Notetaker when a call ends"; `at_call_end`) stops and
  summarises without asking, via `meeting_guard::stop_after_call` (same rule:
  only apps Yap asks about; no window comes up). For the notepad's split
  screen: `call_window_exes()` (the live call's exes) and `title_shows`.
  **Calendar**: before asking, `calendar::claims_call(app)` (under this
  module's lock; it takes only the calendar's) — a call during a meeting
  whose reminder card is up or was answered isn't asked about, a snoozed
  card comes back instead; each call start also goes to
  `calendar::on_call_started` (outside the lock: the switch card for
  back-to-back meetings). Snapshot
  `meeting_detect_status` + `yap-meeting-detect` (`{enabled, style, apps,
  calls, prompt}`); answers `meeting_detect_respond(promptId,
  record|dismiss|never|stop|keep)`; debug-only `meeting_detect_simulate(appId,
  active, fadeMs?)` (no debounce; `fadeMs` shortens that call's fade) for the
  e2e suite. Gated by `config.meeting_detection` (default **on**, as
  OpenWhispr's `notifyMeetingDetection`; `sync()` on every config save re-reads
  the style and per-app choices too). `mic_records()` hands `app_icons.rs` the
  call apps' consent-store keys (exe paths, package families) and their
  `LastUsedTimeStart`, nothing else. Lock rule: never touch windows/WinRT
  while holding its state lock (window getters wait on the main thread). See
  [`docs/meetings.md`](../meetings.md).
- **`app_icons.rs`** — the call apps' **own icons as installed on this PC**
  (`meeting_app_icons` → `{appId: data URL}`, off the main thread) for
  Settings' "Ask about calls in" list and the bar's call card
  (`CallAppIcon.svelte`). Read-only and local (nothing launched, no network;
  only icon and manifest files, plus the consent-store keys call detection
  reads): **desktop apps** from the exe paths in the microphone record
  (`meeting_detect::mic_records`, most recently used first), then the usual
  install folders (`KNOWN_PATHS`; Squirrel's `app-*` → the newest with the
  exe) — the exe's icon at 48 px (`SHDefExtractIconW` → `GetIconInfo` →
  32-bit `GetDIBits` → straight RGBA, alpha or mask → PNG via the `png`
  crate, already built for arboard); **packaged apps** (new Teams, WhatsApp,
  Telegram/Slack from the Store) from the consent store's family names +
  `FAMILIES` → `GetPackagesByPackageFamily` + `GetPackagePathByFullName` →
  `AppxManifest.xml`'s `Square44x44Logo` (a small tag scanner; the listed
  application's, else the store logo) → the variant made for a light
  background at ~48 px (`targetsize-48_altform-lightunplated`; high-contrast
  and dark-theme ones last, folder qualifiers too). Cache `<data>/icons/
  <app>.png` + `sources.json` (path, size, mtime, when made): remade when the
  source changes or after a week, dropped when the app's gone. With call
  detection off the microphone record isn't read. Browser-only apps (Meet,
  Whereby, Jitsi) have none: their bundled mark. Test mode reads no
  installed apps: debug-only `YAP_E2E_APP_ICONS` names a folder of stand-in
  `<app>.png`s. An `#[ignore]`d unit test (`this_machine`, `YAP_ICONS_OUT`)
  runs the real thing read-only and writes the cache somewhere to look at.
- **`meeting_guard.rs`** — guard rails around a meeting recording (Phase 8,
  Wispr Flow's Notetaker settings) + their notices. **Maximum recording
  length** (`config.meeting_max_minutes`: 60/120/180/240, 0 = no limit,
  default **120**, as Wispr): a watcher thread per recording (follows
  `yap-meeting-state`; Settings re-read every second) warns 5 min before —
  "Notes stop in 5 minutes" + **Keep going** (another hour;
  `meeting_keep_going`), in one place (`notice::place`): a sticky in-app
  toast while the main window is focused (`yap-meeting-limit` with `inApp`,
  `meeting_limit_status` after a reload), else a Yap bar card, else a
  Windows notification — then stops and writes the action plan. A
  warning always gives the full 5 min (limit lowered mid-meeting, PC slept
  through it); raising/removing the limit takes it back (`step`/`kept_going`,
  pure + unit-tested). **Quiet meetings** (`config.meeting_end_when_quiet`,
  "End notes when the meeting goes quiet", default **on**, as Wispr), on
  the same watcher: after 10 min with nobody speaking (from the start or the
  last speech) "Ending notes in 30 s…" counts down — **Keep going** (another
  10 min) / **Stop now**; anyone speaking or the setting going off cancels
  it — then `stop_and_summarise` + a notice that says why ("Stopped: the
  meeting went quiet"). Speech = `meeting::activity()`'s talk counter (audio
  above the recorder's silence gate on either side, a dictation's blanked
  span not; plus transcribed talk segments, the e2e feed's too); quiet time
  runs on the **meeting's audio clock** (`clock_ms`: captured audio, so a PC
  asleep or a dead device never counts). **Check-ins** "Still in your
  meeting?" (Stop and summarise / Keep recording): 10 min after the end of
  the note's calendar meeting (`notes::event_of`; < 24 h long, recording
  started before then) and 1 h into any recording; each once per
  recording, held back while anyone spoke in the last 3 min, never while
  the length warning is up or due within 10 min (`Watch::length_busy`),
  gone unanswered after 2 min; the countdown replaces one, the length
  warning's Keep going counts as one. Rules pure + unit-tested
  (`QuietWatch::step`, `pick_surface`). **Where they show**
  (`surface_now`): the notepad if on screen, else the main window if
  focused (both as a sticky toast: `yap-meeting-checkin {prompt}` with
  `surface` = the window label, `meeting_checkin_status` after a reload,
  `meetingCheckIn.js`), else a Yap bar card (id `meeting-checkin`; the
  countdown as its headline, `Countdown.headline`), else a Windows
  notification (tag `meeting-checkin`); answers `meeting_checkin_respond {id,
  answer: keep|stop}`; the stop notice goes to the same surface
  (`Notice.surface`). The countdown's ✕ only hides it; a check-in's ✕ =
  Keep recording. **The meeting shortcut** (`config.meeting_hotkey`,
  default `kb:alt+win+77` = **Win+Alt+M**, as Wispr; "" = unbound):
  `start_or_stop` — recording → stop and summarise; a live call (any app) →
  `meeting_detect::on_tray_record`; else a new "Meeting · 5 Oct, 14:30" note
  (Meetings) + `meeting_start`, opened if the window is visible, and a
  "Taking notes" notice. 1 s debounce. Global hook event
  `meeting-key-pressed`; the in-page fallbacks (Settings, the notepad) call
  `meeting_shortcut { origin }` with their window label, so a question about
  the meeting ("Started by mistake?") shows where the key was pressed.
  `notice` (`yap-meeting-notice` `{kind, title, body, icon, variant,
  settings, noteId}`) carries the screen-share tip, "Stopped at 2 hours",
  "Teams call ended · writing your action plan", "Taking notes" and the
  recorder's warnings (kind `warning`) in **one place** (`notice::place`:
  the main window while it's focused, else the Yap bar via `notice_native`,
  else a Windows notification, else the window on screen); one addressed to
  a window (`surface`) shows there; the same one again within 10 min isn't
  shown (`notice.rs`). **Every stop goes through
  `stop_and_summarise`** = `meeting_end::end` (Rust writes the action plan;
  no window comes up — the notepad shows it when open). Debug-only `e2e_meeting_limit
  {limitMs, warnMs, keepGoingMs}` and `e2e_meeting_checkins {quietMs,
  countdownMs, recentMs, afterEndMs, hourMs, fadeMs, marginMs}` (which also
  turns on the test player's live mic, `e2e::set_live_mic`) shorten the
  timings for the e2e suite.
- **`capture.rs`** — hides Yap's meeting windows from screen capture and
  sharing: while a meeting records and `config.meeting_hide_from_capture` is
  on (default **on**, Wispr's "Don't show Notepad and Flow Bar in screen
  capture"), the windows labelled **`overlay`**, **`notepad`** and
  **`split-preview`** (`MEETING_WINDOWS`, looked up by label, a missing one
  skipped; the preview syncs itself when created) get
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (fallback
  `WDA_MONITOR` before Windows 10 2004): on your monitor, left out of
  screenshots, recordings and every screen share. Lifted when the recording
  stops. Applied on the main thread (`run_on_main_thread`, in order, no lock)
  on every `yap-meeting-state`, on `save_config` (toggled mid-meeting), and on
  `on_page_load` (lib.rs: a window created mid-recording; `sync_window`).
  Off + a meeting starting → the screen-share tip once per note ("Your
  meeting notes show up in screen shares and screenshots" + **Update
  settings** → `meetings#screen-sharing`; a notification still carrying the
  old `general#screen-sharing` lands there too, settings/nav.js). Debug-only `capture_affinity` reads
  each window's affinity back (`GetWindowDisplayAffinity`) for the e2e suite.
