# Notices

How loud Yap may be: the notice policy (one place, one at a time, held while dictating, cooldowns, call-prompt fatigue), alerts the person must act on, and Windows notifications. The page's half (the toast system) is in [frontend.md](./frontend.md) (`lib/views/ControlPanel.svelte`); the bar's cards are [yap-bar.md](./yap-bar.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`notices/notice.rs`** — **Yap's notice policy** (Wispr Flow's rules, by
  behaviour), the one place that decides how loud a notice may be; the page
  runs the same rules (`ui/noticePolicy.js`). **One place**
  (`place_for`/`place`): the main window's toast while it's focused, else
  the Yap bar, else a Windows notification, else the window if it's on
  screen — used by alerts, coaching, learn, call prompts, calendar cards,
  the length warning and meeting notices — among them, since 2026-10-24,
  "Yap couldn't transcribe part of this meeting" (`meetings/meeting.rs`, once per
  meeting, with Open note), and since 2026-10-25 **"Your summary is
  ready"** (`meetings/meeting_end.rs`: only when `yap_in_front` says no Yap
  window — the main window or the notepad — has the focus; then the Yap
  bar's card, else a Windows notification, with Open note) — (`window_view`: test runs say for
  themselves whether the window counts as focused, `e2e_focus`). **Rules per
  kind** (`rule`: a bar card's id / a notification's tag): priority (Prompt —
  call (also "Stop taking notes?" and "Switch to Zoom?", 2026-10-29),
  calendar, meeting-limit, meeting-checkin (also the meeting's other
  questions: the call on another mic, "Your PC was asleep", "Start notes
  automatically for Teams calls?"; `meeting_guard::ask`) — > Alert — micBlocked,
  pasteBlocked (an admin app, keys Windows didn't take, "Select a text box
  first": `pasting/text_injector.rs` `Left`, one per failed paste),
  clipboardBusy (edit mode's Ctrl+C left the clipboard alone or couldn't
  put it back), calls-fatigue, configReset (held over a dictation, never
  dropped), saveFailed ("Couldn't save your dictation history", `shell/saving.rs`:
  held over a dictation, never dropped, the same words not twice in 10 min),
  assistantNoModel, micLost ("Contoso H2 disconnected", the dictation's mic
  unplugged: `dictation/pipeline/unplugged.rs`), lidClosed ("Your laptop's lid
  is closed, so its microphone can't hear you", `dictation/lid.rs`, once per
  closing there, the same words not twice in 10 min), meetingRecovery ("Yap
  closed during a meeting", held while dictating or recording, waiting as
  long as it takes; `meetings/meeting_audio.rs`), and the Voice Assistant's
  answer `assistant` (never held, never stale) — > Info — micOffer ("Contoso
  H2 connected · Use it for dictation?", `dictation/mics/offer.rs`, which
  rate-limits it itself and never asks during a dictation), micSwitched
  ("Using Contoso H2 for dictation", the same words not twice in 10 min),
  language-added ("French is in My languages", 10 s) — > Tip — hint,
  micTip ("Yap didn't hear you clearly", `dictation/mic_tips.rs`, which
  rate-limits it itself — one a day —: held over a dictation and a meeting,
  and not even offered (nor counted) while one records, "Show tips" turns
  it off), featureTip ("Yap caught your correction", "Yap made that a
  list"…, `dictation/tips.rs`, once each: held over a dictation and a
  meeting, not offered or counted while one records, dropped unseen after
  a minute, "Show tips" turns it off),
  word-suggestion, snippet-suggestion ("Type that often? Save it as a
  snippet", `dictionary/snippet_offer.rs`, one a day there; held over a
  dictation, dropped unseen after a minute; snippet-saved 10 s),
  language-nudge ("You spoke French · Add it to My
  languages?", `dictation/languages/nudge.rs`, which rate-limits it itself:
  held over a dictation, dropped unseen after a minute), milestone
  ("You've dictated 1,000 words", `dictation/milestones.rs`, once each:
  held over a dictation and a meeting, "Show tips" turns it off, dropped
  unseen after a minute)), held
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
- **`notices/kinds.rs`** — **kinds of notice, each with a switch and a
  cap** (2026-10-29; Wispr Flow's notification categories, by behaviour):
  the nice-to-know notices fall into four kinds — **suggestions**
  (word-suggestion, snippet-suggestion, language-nudge), **milestones**
  (`dictation/milestones.rs`'s "milestone", and any kind starting so), **feature tips** (hint, micTip, featureTip,
  the bar's first-use pointers) and **news** (the "Updated to …" note after
  an update) — each switched off on its own in Settings → General →
  Notifications (`notify_suggestions`, `notify_milestones`,
  `notify_feature_tips`, `notify_news`, synced with `show_tips`; "Show
  tips" still mutes every tip), and each shown at most `cap` times a day
  (suggestions 3, milestones 2, feature tips 3, news 1; counted in memory,
  a restart starts the day afresh). Through the one policy: a module about
  to show one asks **`notice::place_tip(app, kind)`** — its place, or
  Nowhere when its kind is off or used up today, counted when it goes
  somewhere (coaching, mic tips, feature tips, word milestones, learn, the
  snippet offer, the language nudge) — and the page asks `notice_allow` for its own
  ("Updated to …"). Questions, alerts and notices have no kind and are
  never capped. A test run counts the caps only once a spec asks
  (`e2e_notice_caps`; `e2e_notice_reset` turns them off again). Pure parts
  unit-tested.
- **The offer of AI cleanup** (`cleanup_styles::offer`, `lib/notices/cleanupOffer.js`,
  2026-10-24): one tip, ever, in the main window — to someone with cleanup
  off on a PC the on-device model fits, after five dictations — asked of
  Rust after a dictation is saved or when the window comes to the front;
  `cleanup_offer_status` says yes only while `window_view` has the person
  looking at the main window and `moment()` has no dictation, meeting or
  first-run setup, and never with tips off. Set it up / Not now; shown =
  `cleanup_offer_seen`.
- **`notices/alert.rs`** — one-off notices the person must act on (`pasting/elevated.rs`'s
  "Your text is copied: press Ctrl+V", `dictation/mic_privacy.rs`'s "Windows is blocking
  microphone access…", call-prompt fatigue's "Stop asking about calls?" /
  "Yap stopped asking about calls", the Voice Assistant's "Pick an AI model
  for the Voice Assistant" with Open settings → Language Models → Chat &
  Voice Assistant, spoken replies' "ElevenLabs didn't answer (quota used
  up)" — kind `voiceFallback`, once per reason in 10 minutes
  (`voice/tts/cloud.rs`), Open settings → Voice Assistant → Voice; the
  microphones' "Contoso H2 disconnected" (kind `micLost`) and "Using
  Contoso H2 for dictation" (kind `micSwitched`), with **Microphones** /
  **Change** → Settings → Dictation → Microphone (`OPEN_MIC_ORDER`,
  `dictation/mics.rs`); `meetings/meeting_audio.rs`'s
  "Yap closed during a meeting" at the next start, with **Transcribe it**
  — `recoverMeetings`), a settings
  file Yap couldn't read ("Your settings were reset" / "Some of your
  settings were reset", kind `configReset`, once, with **Show file** →
  `config::show_old_file`: Explorer on the copy set aside; `config.rs`), a
  save that failed by itself ("Couldn't save your dictation history" — "The
  disk is full. Yap is keeping it in memory and will try again with the next
  change.", kind `saveFailed`, once until a save of that store works, with
  **Try again** → `saving::retry_soon`: "Saved", or "Still couldn't save…";
  `shell/saving.rs`): in one place (`notice::place`) — a
  toast in the main window while it's focused (`yap-alert` →
  `lib/notices/alerts.js`, kind `alert`), else a Yap bar card, else a Windows
  notification, else the window on screen; the same alert again within its
  kind's cooldown isn't shown (the mic one: once per 10 min; the overlay's
  line says it every press). One optional button (`Action`: **Open
  settings**, **Stop asking** / **Turn back on** →
  `meeting_detect::set_detection`) comes back to `act` from all three
  (`alert_action` from the toast). `show_soon` runs it on a thread of its
  own (window queries; a hotkey's handler may hold the pipeline's locks).
  Test mode only logs what Open settings would open. Since 2026-10-25 also
  a dictation stopped at its length limit ("Dictation stopped at 15
  minutes", kind `dictationCapped`: never held or dropped, with **Recover
  text** → `cap::recover`; `dictation/cap.rs`), and **`confirm`**: the same
  place minus the Windows notification, for quiet notes — Paste / Copy last
  transcript's "Copied your last dictation", "Nothing to paste yet" (kind
  `lastTranscript`: shown now or within 5 s, else dropped;
  `pasting/last.rs`) and Recover text's "Copied your dictation".
- **`notices/win_toast.rs`** — the shared Windows-notification plumbing (WinRT
  `ToastNotification`, Windows only): `app_id` (the NSIS shortcut's
  AppUserModelID; dev builds borrow PowerShell's), `logo_xml`, `esc`, and
  `post(tag, xml, data, on_activated)` / `update` / `remove` in the "yap"
  group, plus `post_quietly` (no banner — `SetSuppressPopup`, straight into
  the notification centre; the call prompts' "Quietly" style) and `build`
  (the toast, unshown — what the unit tests check). updates.rs posts under the
  "update" tag (always with a banner), calls/meeting_detect.rs under "call",
  calls/meeting_guard.rs under "meeting-limit" (Keep going) and "meeting-notice"
  (Update settings / Open note), calendar.rs under "calendar" (the reminder
  card's buttons), notices/alert.rs under the alert's kind; each keeps its live
  toast so the buttons work from the notification center. `post` goes
  through the notice policy: the same notification again within its kind's
  cooldown isn't posted, and it skips the banner (`notice::native_quietly`)
  while a dictation records or `SHQueryUserNotificationState` says busy,
  unless it's an alert or a question. `allowed()` = not portable, not a test
  run.
