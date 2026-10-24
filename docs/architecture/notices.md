# Notices

How loud Yap may be: the notice policy (one place, one at a time, held while dictating, cooldowns, call-prompt fatigue), alerts the person must act on, and Windows notifications. The page's half (the toast system) is in [frontend.md](./frontend.md) (`lib/views/ControlPanel.svelte`); the bar's cards are [yap-bar.md](./yap-bar.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`notice.rs`** — **Yap's notice policy** (Wispr Flow's rules, by
  behaviour), the one place that decides how loud a notice may be; the page
  runs the same rules (`ui/noticePolicy.js`). **One place**
  (`place_for`/`place`): the main window's toast while it's focused, else
  the Yap bar, else a Windows notification, else the window if it's on
  screen — used by alerts, coaching, learn, call prompts, calendar cards,
  the length warning and meeting notices — among them, since 2026-10-24,
  "Yap couldn't transcribe part of this meeting" (`meeting.rs`, once per
  meeting, with Open note) — (`window_view`: test runs say for
  themselves whether the window counts as focused, `e2e_focus`). **Rules per
  kind** (`rule`: a bar card's id / a notification's tag): priority (Prompt —
  call, calendar, meeting-limit, meeting-checkin — > Alert — micBlocked,
  pasteBlocked, calls-fatigue, configReset (held over a dictation, never
  dropped), saveFailed ("Couldn't save your dictation history", `saving.rs`:
  held over a dictation, never dropped, the same words not twice in 10 min),
  assistantNoModel, meetingRecovery ("Yap
  closed during a meeting", held while dictating or recording, waiting as
  long as it takes; `meeting_audio.rs`), and the Voice Assistant's
  answer `assistant` (never held, never stale) — > Info > Tip — hint, word-suggestion), held
  while dictating (+ `SETTLE_MS` 1.5 s, and while onboarding is on screen)
  or a meeting records, a cooldown for the same text (micBlocked and
  meeting-notice 10 min), how long it may wait unseen (tips 10 s, a word
  suggestion and notices 60 s; questions as long as it takes), whether a
  sticky offer yields to its equals (meeting-mistake, update), whether "Show
  tips" mutes it. **`pick`** chooses the one card on screen (rank, then the
  one up, then the oldest; a card the pointer is on stays) and the stale
  ones to drop; **`Ledger`** the cooldowns (`admit`); **`Fatigue`** the call
  prompts' 3 / 5 / 35 rule (`notices.json`, `with_call_fatigue`). The moment
  comes from `on_pipeline_state` (lib.rs's `yap-state`), `on_meeting` (the
  bar's `yap-meeting-state`), `on_onboarding` and `sync` (config: `show_tips`).
  All pure parts unit-tested. Debug-only `e2e_notice_reset` (the `main`
  fixture runs it before every test), `e2e_focus {focused}` and `e2e_notice
  {surface: bar|window, kind, title, body, level, timeoutMs, dictating}`
  (a notice through the real policy, or a pretend dictation's `yap-state`).
- **The offer of AI cleanup** (`cleanup_styles::offer`, `lib/notices/cleanupOffer.js`,
  2026-10-24): one tip, ever, in the main window — to someone with cleanup
  off on a PC the on-device model fits, after five dictations — asked of
  Rust after a dictation is saved or when the window comes to the front;
  `cleanup_offer_status` says yes only while `window_view` has the person
  looking at the main window and `moment()` has no dictation, meeting or
  first-run setup, and never with tips off. Set it up / Not now; shown =
  `cleanup_offer_seen`.
- **`alert.rs`** — one-off notices the person must act on (`elevated.rs`'s
  "Your text is copied: press Ctrl+V", `mic_privacy.rs`'s "Windows is blocking
  microphone access…", call-prompt fatigue's "Stop asking about calls?" /
  "Yap stopped asking about calls", the Voice Assistant's "Pick an AI model
  for the Voice Assistant" with Open settings → Language Models → Chat &
  Voice Assistant, spoken replies' "ElevenLabs didn't answer (quota used
  up)" — kind `voiceFallback`, once per reason in 10 minutes
  (`tts/cloud.rs`), Open settings → Voice Assistant → Voice; `meeting_audio.rs`'s
  "Yap closed during a meeting" at the next start, with **Transcribe it**
  — `recoverMeetings`), a settings
  file Yap couldn't read ("Your settings were reset" / "Some of your
  settings were reset", kind `configReset`, once, with **Show file** →
  `config::show_old_file`: Explorer on the copy set aside; `config.rs`), a
  save that failed by itself ("Couldn't save your dictation history" — "The
  disk is full. Yap is keeping it in memory and will try again with the next
  change.", kind `saveFailed`, once until a save of that store works, with
  **Try again** → `saving::retry_soon`: "Saved", or "Still couldn't save…";
  `saving.rs`): in one place (`notice::place`) — a
  toast in the main window while it's focused (`yap-alert` →
  `lib/notices/alerts.js`, kind `alert`), else a Yap bar card, else a Windows
  notification, else the window on screen; the same alert again within its
  kind's cooldown isn't shown (the mic one: once per 10 min; the overlay's
  line says it every press). One optional button (`Action`: **Open
  settings**, **Stop asking** / **Turn back on** →
  `meeting_detect::set_detection`) comes back to `act` from all three
  (`alert_action` from the toast). `show_soon` runs it on a thread of its
  own (window queries; a hotkey's handler may hold the pipeline's locks).
  Test mode only logs what Open settings would open.
- **`win_toast.rs`** — the shared Windows-notification plumbing (WinRT
  `ToastNotification`, Windows only): `app_id` (the NSIS shortcut's
  AppUserModelID; dev builds borrow PowerShell's), `logo_xml`, `esc`, and
  `post(tag, xml, data, on_activated)` / `update` / `remove` in the "yap"
  group, plus `post_quietly` (no banner — `SetSuppressPopup`, straight into
  the notification centre; the call prompts' "Quietly" style) and `build`
  (the toast, unshown — what the unit tests check). updates.rs posts under the
  "update" tag (always with a banner), meeting_detect.rs under "call",
  meeting_guard.rs under "meeting-limit" (Keep going) and "meeting-notice"
  (Update settings / Open note), calendar.rs under "calendar" (the reminder
  card's buttons), alert.rs under the alert's kind; each keeps its live
  toast so the buttons work from the notification center. `post` goes
  through the notice policy: the same notification again within its kind's
  cooldown isn't posted, and it skips the banner (`notice::native_quietly`)
  while a dictation records or `SHQueryUserNotificationState` says busy,
  unless it's an alert or a question. `allowed()` = not portable, not a test
  run.
