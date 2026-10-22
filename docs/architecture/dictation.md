# Dictation

How a dictation runs: the hotkey and push-to-talk gestures, the mic, speech-to-text, the order of the passes after it, Upload, history, and other apps' sound while you talk. Where the text goes is [pasting.md](./pasting.md); the dictionary is [dictionary.md](./dictionary.md); the bar that shows it all is [yap-bar.md](./yap-bar.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## The pipeline order

Pipeline order after transcription: **AI cleanup → dictionary (exact, then a
fuzzy near-miss pass) → names on screen (`screen_terms.rs`: a name heard a
little wrong spelled as the window shows it) → the "press enter" voice
command → snippets → fit to the
kind of app (`app_kind.rs`: a terminal gets one line and no trailing space; a
chat app's short message loses its full stop) and to the text around the
cursor (smart spacing; where the field couldn't be read, append-space as
before) → inject (Ctrl+V, or the terminal's own paste keys — Shift+Insert in
Git Bash, Ctrl+Shift+V in Alacritty…; a paste a Win32 text box never got is
typed instead, once; an app running as administrator takes no keys from Yap:
the text is left on the clipboard and the person told, `elevated.rs`) →
submit key (auto-submit or the voice command, once; in a
terminal Enter, only for the voice command or a coding CLI's auto-submit) —
or, with no submit key, the correction watch (`learn.rs`: fix a word Yap
typed and it offers to add it to the dictionary)**
(`pipeline::finish_dictation_with` does the dictionary, the names on screen,
the command and snippets;
`text_injector::prepare_dictation` the fitting; `app_kind::submit_key` the
key). Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation
never blocks. It keeps the person's **English spelling** ("Write in my
region's spelling", on, Settings → Language Models → Dictation Cleanup;
the region is About you's; 2026-10-21): a UK user's
"color" comes out "colour", a US user's "colour" "color" — one line in
Yap's own layer of the cleanup prompt (the system prompt's end and the
dictation's restated instruction: `llm::cleanup_system_prompt`,
`cleanup_message`, `about_you::cleanup_spelling`), never in the editable
preset; another language gets no line. How well it's followed depends on
the model (the 4B mostly; the 1.5B hardly — docs/model-evals.md). The dictionary runs Handy's split (ported from its source):
**Whisper models** get the correct spellings as the decoder's `initial_prompt`
(ASR biasing, threaded through `SttEngine::transcribe` at every call site —
dictation/partials/upload/meeting — the words used most lately first,
`dict_usage::ranked`; plus an OpenWhispr-ported prompt-echo guard
in `stt.rs`), **ONNX models** get **`fuzzy.rs`** — Levenshtein + Soundex 1–3-word
n-gram correction ("jaison"→"JSON", "Chat G P T"→"ChatGPT"; threshold 0.18,
≥3-char terms), gated by `config.dictionary_fuzzy` (default on, "Catch
near-misses" toggle in the Dictionary view) with a **per-entry ≈ opt-out**
(`entry.fuzzy` — exempts corrections whose near-misses are real words, e.g.
`json → JSON` eating the name "Jason"). See `docs/fuzzy-dictionary.md`. The
exact + fuzzy correction is ONE function, `pipeline::apply_corrections(text,
&cfg)`, run the same way on a dictation, on every meeting segment (both sides,
before `meeting::ingest`) and on an Upload's whole text; the dictionary words
in its result count as used (`dict_usage::record`). **Snippets** (an entry
with `snippet: true`: a spoken phrase → text, may span lines;
`config::apply_snippets`) are dictation-only: whole phrase, any case and
punctuation, longest first in one pass, a phrase said alone becomes exactly
its text, and cleanup's full stop after a phrase is dropped where it would
spoil the text (`stray_full_stop`: after a text ending in punctuation, or as
the dictation's last character after a text of several lines — a sign-off or
address ends as saved; a one-line snippet mid-sentence keeps the sentence's
"."); never in the corrections, the fuzzy pass or the Whisper prompt (the
cleanup model is told to keep the phrases word for word). The **"press
enter" voice command** (`voice_enter`, default on;
`pipeline::take_voice_enter`): a dictation whose LAST words are "press
enter"/"press return" (final text, else the raw transcript in case cleanup
dropped it) loses them and presses `auto_submit_key` after the paste.
**Names on screen** ("Spell names the way they're written on screen",
`screen_names`, on; [dictionary.md](./dictionary.md), `screen_terms.rs`):
`capture_target` starts a background read of the target window's visible
text beside the cursor read (a pasted dictation only: not a rewrite, the
assistant or the Chat box); `deliver` takes them once
(`screen_terms::take`) for cleanup's "Spell these names as written: …"
line and the near-miss pass (`finish_dictation_with`); a cancel forgets
them. Never waited for: still reading = not used (a robot test delays the
read 8 s: the dictation pastes as fast as without it). `run_stt`'s Whisper
prompt goes through `screen_terms::whisper_prompt` (`dictation_prompt`),
which leaves the names out — measured, see dictionary.md. Upload and
meetings never get them.

## Modules

- **`pipeline.rs`** — the heart. Owns the mic stream + shared state (`recording`,
  audio `buffer`, idle `preroll` ring, warm STT `engine`, live `config`,
  `last_activity`, `target_hwnd`). Audio callback buffers while recording,
  downmixes→mono, resamples→16 kHz, and emits a throttled **peak amplitude**
  (`yap-amp`) for the scrolling waveform; while idle it keeps a rolling ~300 ms
  **pre-roll** ring that `start_recording` prepends (anti first-word-clipping).
  `recording_mode` selects toggle vs push-to-talk; in push-to-talk `on_key` reads
  **gestures** (`gesture.rs`, Wispr Flow's by behaviour): hold to talk, a quick tap
  (< 300 ms) + a second press within 500 ms locks it **hands-free** until the next
  press (`yap-dictation-locked` → a lock on the overlay), a lone tap is thrown away
  (never a junk dictation) and earns a coaching hint (`coaching.rs`), as does a
  hold of a minute or more. Key events and the tap timer act under one lock
  (`gesture`), timed by the input hook's own timestamps; a recording started or
  stopped elsewhere (the bar's Dictate, the tray's Cancel) is followed, and one
  started by Dictate stops on the next press. `run_stt` (async) transcribes, then
  `deliver` (also robot mode's way in, `realtest.rs`; the target is captured by
  `capture_target`, mic audio arrives through `take_audio`) does cleanup →
  `finish_dictation` (dictionary via `apply_corrections`, shared with meetings
  and Upload; then the "press enter" command, then snippets) →
  `text_injector::prepare_dictation` (fitted to the kind of app,
  `target_kind` — `app_kind::of_window`, read in `start_recording` beside
  `target_hwnd`, with one UI Automation question in an editor (is the focus
  its integrated terminal?) — and to the text around the cursor, which `start_recording`
  started reading in the background via `selection::begin_dictation` — not
  for edit mode, nor a terminal with terminal mode on) → inject (into the
  captured `target_hwnd`, with the keys `target_kind` as read pastes with —
  `read_kind`, before the settings; `Delivered.paste`/`.typed` tell the robot)
  → the submit key when auto-submit or the voice command
  asked (once, `app_kind::submit_key`; a dictation of only "press enter" presses it only while the
  target window still has the focus). **The dictation signal** for a meeting
  recording: `start_recording` (plain dictation, the edit hotkey, Commands by
  name, the assistant shortcut) calls `dictation_began()` and the stop/cancel `dictation_ended()`, one
  atomic (`DICTATION`: dictations started << 1 | recording) that
  `meeting.rs` reads lock-free in its mic callback (`dictation_state()`) to
  keep dictations out of a meeting's "You" side. `PREROLL_SAMPLES` is shared
  with it (the meeting blanks a dictation's pre-roll too). **Commands** (two
  different things share no name any more, 2026-10-18: *Commands* type for
  you, the *Voice Assistant* answers out loud): by name (`agent_detect.rs`,
  OpenWhispr `detectAgentName` port) a dictation addressing the
  agent (`agent_name`, default "Yap"; fuzzy-matched) routes the whole
  transcript through the Commands scope (`llm_scopes["voiceAgent"]`, its key kept) in write mode (`run_agent`, shared with
  the edit hotkey = the rewrite shortcut) instead of cleanup — the agent prompt strips the name+command.
  **The Voice Assistant's shortcut** (`Pipeline::on_assistant_key`, the
  hook's `assistant-key-*` or the in-page `assistant_key`): the same
  `on_key` (toggle / push-to-talk gestures; no coaching hints for it) with
  `ASSISTANT_ARMED` set for the press, so the recording it starts is the
  assistant's (`destination(edit, assistant, chat_armed, chat_focused,
  own_window)` → `Goes::Paste | ChatBox | Assistant`, pure, in
  `capture_target`, which calls `assistant::listening` before "recording");
  `run_stt` hands its transcript, after `apply_corrections` only, to
  `assistant::heard` (`ask_assistant`): no cleanup, Command, paste or
  history; nothing heard → "Yap didn't catch a question"; a cancel isn't
  kept for Undo; every early end calls `assistant::nothing`.
  **Barge-in**: `start_recording` first calls `tts::stop_for_dictation()` —
  a spoken reply stops before anything records, and if Yap was audible in
  the last ~450 ms the pre-roll is left out (it would hold Yap's voice).
  **Questions for the Chat view** (`to_chat`, decided in `capture_target` by
  the pure `chat_target(edit, armed, focused, own_window)`): the chat's mic
  (`chat_dictate` → `toggle_for_chat`, which arms `CHAT_ARMED` for the
  recording it starts) or a dictation while the chat box has the focus
  (`chat_input_focus` → `CHAT_FOCUSED`) and one of Yap's windows is in front;
  never edit mode. Such a dictation skips the wake word and the paste:
  `deliver_to_chat` emits `yap-chat-dictation {text, send}` (`send` =
  `tts::sends_by_itself`: "Talk back hands-free" or "press enter" said) and
  keeps it in history. A `processing` guard blocks
  starting a new recording while one is still transcribing (no overlapping `run_stt`
  / duplicate model load), and the buffer is capped at 15 min so a stuck key can't
  OOM. With `streaming_partials` on, `stream_partials` (a per-session worker)
  transcribes a bounded **sliding window** of the buffer every ~500 ms
  (`partials.rs`: text older than the window freezes as committed text, the
  window advances at quiet points via `media::quietest_index`, 20 s hard cap —
  per-tick cost independent of recording length; de-flickered by `smart_diff`,
  adaptive backoff on slow machines, warm-loads the engine if it was
  idle-unloaded) and emits `yap-partial` text. An **idle watcher** unloads the model after
  `model_unload_timeout`; the next dictation lazily reloads it. **No microphone** (none
  plugged in, a headset that connects after login, a CI runner) doesn't take the
  pipeline down: it runs without a stream (`has_mic`), a hotkey press retries
  (`ensure_mic`), picking a mic (`set_input_device`) starts capture, and a recording
  attempt without one says "No microphone found" (on the overlay) — or, when
  Windows' privacy switches are why, "Windows is blocking microphone access for
  desktop apps" (`mic_privacy.rs`) — Upload, models and Settings work regardless. **Dead-mic watchdog** (`mic_watch.rs`): the audio
  callback tallies each recording's live audio (`mic_tally`: how much, any sample
  not exactly 0.0); a per-recording watcher (`spawn_mic_watch`) reopens the stream
  once (`reopen_mic`) after 2 s without audio or 3 s of nothing but zeros, and at
  5 s says "No sound from <mic> — check it isn't muted or unplugged"
  (`yap-dictation-error`, plus `yap-mic-silent` for the overlay's line) while
  the recording carries on;
  sound coming back takes it back, and a recording that ends still silent isn't
  transcribed (the overlay ends on the reason). **Windows' microphone privacy
  switches** (`mic_privacy::blocked`) are checked when a recording starts — if
  they block desktop apps it's said at once (`say_mic_blocked`: the overlay's
  "Windows is blocking the microphone" + an alert with **Open settings**,
  `alert.rs`; `mic_dead` set, so a stop with no sound isn't transcribed; the
  recording goes on, and sound arriving takes it back) — and again before the
  watchdog's report, which then blames the switch instead of the mic.
  Capture start, `ensure_mic`,
  `set_input_device` and the reopen all go through `open_stream` (the stream slot
  is shared weakly with `Shared`; the reopen stops the dead stream first).
  **Cancel and Undo**: `cancel(undo)` — the bar's ✕, the tray's Cancel and
  `cancel_recording` keep a recording of ≥ 0.5 s that heard a sound in `kept`
  (`Kept`: its audio, `target_hwnd`, `target_kind`, edit mode and selection)
  for `UNDO_MS` (10 s): `yap-dictation-cancelled { undoMs }` + `yap-state`
  "cancelled" (the bar's "Dictation cancelled · Undo"), then "idle" and it's
  dropped; a new dictation drops it too (`capture_target`), and
  push-to-talk's lone tap is simply thrown away. `Pipeline::undo` (`bar_action`
  "dictation-undo") puts the target back and runs `run_stt` on the kept audio
  as a stop would — the paste goes back to the window the dictation started
  in, whatever has the focus now, or nowhere (`undoing` → `inject_text`'s
  `strict`: a window that's closed or won't come to the front leaves the
  text on the clipboard, `Delivery::WindowGone`, with "Your text is copied:
  press Ctrl+V") — after reading that window's cursor context afresh (a
  closed window's never: `read_context` refuses a target whose process is
  gone). `Pipeline::stop` is the bar's ✓ (the hotkey's second
  press). `holds_undo` keeps the updater from restarting under a pending Undo.
- **`gesture.rs`** — push-to-talk's pure gesture state machine: key down/up and
  the clock in (`now_ms`, monotonic, also what the input hook stamps events
  with), Start / Stop / Discard / Lock + an optional hint + "tick me in N ms"
  out. States Idle → Held → (TapPending → LockHeld → Locked). `TAP_MAX_MS` 300,
  `DOUBLE_TAP_GAP_MS` 500, `LONG_HOLD_MS` 60 s. Every call is told whether a
  dictation records right now and follows that first. Unit-tested.
- **`coaching.rs`** — push-to-talk coaching hints: a lone tap → "Hold {key} while
  you talk" (or double-tap for hands-free), a minute-long hold → "Double-tap
  {key} for hands-free". Each ≤ 3 times, never within 10 min of another, none
  once double-tap has been used; counts in `<data>/hints.json` (not config.json:
  Settings' auto-saved copy would write old counts back). Surface (one
  place, `notice::place`): a Tip toast in the main window while it's focused
  (`yap-hint`), else a Yap bar card (icon `tip`; it waits out a dictation or
  a meeting and is dropped if it can't show within 10 s), else the window's
  toast if it's on screen; neither → nothing shown, nothing counted (never a
  Windows notification). "Show tips" off (`show_tips`) → none. Texts say
  `{key}`; the page names the hotkey (`fillHotkey` in `lib/hotkeys.js`; the
  card's `hotkey` field). Not for the edit hotkey.
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks; specs `kb:VKEY`,
  `kb:ctrl+shift+VKEY` (modifier combo; `win` is a modifier too, e.g. the
  meeting shortcut `kb:alt+win+77` = Win+Alt+M), `kb:165` (single right-side modifier, e.g.
  RightAlt — never suppressed, it's AltGr), `mods:ctrl+alt` (modifier-only chord) /
  `mouse:ID` — combo semantics ported from OpenWhispr's `windows-key-listener.c`
  (press = key down w/ required modifiers held, release = key up OR required
  modifier up; chords fire on completion; suppressed keys are excluded from the
  GetAsyncKeyState self-heal — the hook eats them before the key-state table
  updates). Four bindings: dictation, edit (`edit-key-*`, Commands' rewrite
  shortcut), the meeting
  shortcut (`meeting-key-*`, `configure_meeting`; only its press is used —
  `meeting_guard`) and the Voice Assistant's (`assistant-key-*`,
  `configure_assistant`; unbound by default; `configure_assistant_from`
  leaves one that clashes unbound). **`clashes(a, b)`** (pure, unit-tested;
  `hotkeysClash` in `lib/hotkeys.js` is the same rule, Settings refuses with
  it): the same key however written, combos on one key nesting (Ctrl+Space
  inside Ctrl+Shift+Space), a chord completing inside a combo or bigger
  chord; a bare key and its combos don't (a bare key fires only with no
  modifier held), unless it's a modifier itself. A combo holding **Alt or Win** whose main key is swallowed
  taps an unassigned **menu-mask key** (vkE8, AutoHotkey's `#MenuMaskKey`)
  while the modifiers are still down, sent by the forwarder thread
  (`needs_menu_mask`): otherwise releasing Alt opens the focused app's menu
  bar (Office KeyTips) and Win the Start menu. Emits press AND release (via an emit-forwarder thread — the hook
  callback never blocks — plus a 30 s re-hook self-heal), each with the moment the
  key moved as its payload (`gesture::now_ms`): events are handled one after
  another, and push-to-talk's tap timing mustn't stretch while the last one starts. The capture UI is
  `ui/HotkeyInput.svelte` + shared `lib/hotkeys.js` (parse/format/match — also
  drives the in-window fallbacks; combos read Win first, "Win + Alt + M"). ⚠ **Known Windows
  gotcha:** when one of Yap's OWN WebView2 windows has focus, the LL hook never
  receives the hotkey (WebView2/Chromium front-runs the hook chain on focus) —
  so the Settings + onboarding pages catch the hotkey **in-page** (keydown and
  keyup → `dictation_key { pressed }`, the hook's own path into `on_key`, so
  push-to-talk's gestures work there too; Settings and the notepad also catch the meeting shortcut →
  `meeting_shortcut`, and the assistant shortcut → `assistant_key { pressed }`). Any new Yap window with focusable UI needs the
  same fallback.
- **`mic_watch.rs`** — the dictation's dead-mic watchdog, pure: `MicTally`
  (lock-free, fed by the audio callback), `DeadMicWatch::poll(elapsed, samples,
  heard)` → Wait / Reopen / Report / Recovered / Fine, the messages (the full one
  and the overlay's short "No sound from <device> — is it muted?"). Only exact
  zeros count (a quiet room is never all zeros). Unit-tested over simulated
  callback chunks. Before it blames the mic, the pipeline asks Windows' privacy
  switches (`mic_privacy.rs`). Meetings have their own quiet-side watch (`meeting.rs`).
- **`mic_privacy.rs`** — Windows' microphone privacy switches, read only (never
  written): HKLM `…\CapabilityAccessManager\ConsentStore\microphone` `Value`
  ("Microphone access", the device), HKCU's ("Let apps access your
  microphone"), HKCU's — else HKLM's — `…\microphone\NonPackaged` ("Let desktop
  apps access your microphone"), and HKLM `…\Policies\Microsoft\Windows\AppPrivacy`
  `LetAppsAccessMicrophone` = 2 (an organisation's policy); a
  missing value is no answer. Pure `decide` names the widest switch that's off
  (policy, device, apps, desktop apps), unit-tested. Asked at a recording's
  start, in the no-mic path and before the dead-mic watchdog's report (see
  `pipeline.rs`): "Windows is blocking microphone access for desktop apps" +
  which switch to turn on + **Open settings** (`ms-settings:privacy-microphone`)
  as an alert (`alert.rs`), "Windows is blocking the microphone" in the
  overlay's line; logged when capture starts. Test mode reads no registry:
  debug-only `e2e_mic_privacy { blocked }` stands in; an `#[ignore]`d
  `mic_privacy_this_machine` prints this PC's switches.
- **`stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **14-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  `apply_accelerator_settings` sets whisper→Vulkan(Auto) / ONNX→DirectML.
  `transcribe_timed` (default: `transcribe`, no times) keeps the times the
  model gives (`Timed { text, words: Vec<TimedText> }`, seconds from the
  audio's start): Parakeet's words (`TimestampGranularity::Word`), Whisper's
  segments (a phrase each), SenseVoice's tokens joined at "▁"; none for
  Moonshine/GigaAM/Canary/Cohere or the stub. Only meetings call it.
- **`media.rs`** — audio-file decode front-end for Upload: pure-Rust **Symphonia**
  (mp3/wav/m4a/aac/flac/ogg-vorbis; no opus yet) → downmix mono → 16 kHz
  (`pipeline::resample_linear`), plus `chunk_ranges` (~60 s windows cut at the
  quietest sample of each window's last 5 s). Consumed by
  `pipeline::run_file_transcription` (progress events, cancel flag, `processing`
  guard, the correction dictionary over the whole joined text via
  `pipeline::apply_corrections` — so a mis-hearing split across two chunks is
  still caught — and a history record with the raw and corrected text) via the
  `transcribe_file` command.
- **`sound.rs`** — start/stop chimes (volume + output-device aware); each
  takes an optional `Played` callback, run on the chime's thread once it has
  played (or at once when it can't): how `mute.rs` goes after the start
  chime and comes back after the stop chime. `open_output` (the configured
  output device, else the default) is shared with `tts.rs`.
- **`mute.rs`** — **other apps' sound while you dictate** (2026-10-17; it
  replaced the old endpoint mute, which silenced Yap's own start chime and
  any call): **"Silence other apps while you dictate"**
  (`config.quiet_other_apps`, off; old configs' `muteWhileRecording` is its
  serde alias) and **"Pause music and videos while you dictate"**
  (`config.pause_media`, off, as Wispr's; `media_pause.rs`). The pipeline
  calls `begin(plan)` in `start_recording` and `after_start_chime(token)`
  from the start chime's `Played` (no chime: a thread), `end()` at stop /
  cancel and `after_stop_chime(token)` after the stop chime (`Pipeline::
  stop_chime_then_restore`; a cancel: at once). Quieting = per app, never
  the device: every active output's sessions (`IAudioSessionManager2` →
  `IAudioSessionControl2` + `ISimpleAudioVolume`) set to 0, never Yap's
  own pid, system sounds, a call app's exe (`meeting_detect::call_audio`:
  every `APPS` exe/package, call or not), the browser a live meeting tab is
  in, or any browser while a meeting records (`quiet_verdict`, pure).
  Restore = each session by its instance id back to exactly its level, only
  if it's still at Yap's level (`restore_verdict`: one that moved is left
  alone); a dictation that ended before its chime finished quiets nothing,
  one that starts before the last restore keeps everything down and brings
  it all back at its end (a generation token; COM/WinRT work serialized on
  `WORK`, never under the state lock the pipeline takes). Bluetooth: if the
  default output dropped into a call profile (< 44.1 kHz) during the
  dictation, the restore polls it every 50 ms ≤ 5 s, + 500 ms
  (`codec_wait`; no wait when it was already in one at the start — Yap's
  always-open mic on a headset, a call). Crash safety: Windows keeps an
  app's session level across restarts, so what's down is written to
  `<data>/quieted_apps.json` while it is; `recover()` at setup and every
  restore bring back sessions of those apps still at Yap's level (a week).
  `restore_all()` in `shutdown_cleanup`. **Test runs** touch nothing
  (`realtest::other_apps` = Nobody); **robot mode** only the sessions of
  processes the test started (`realtest::audio_permits`: the process or an
  ancestor is in `YAP_REALTEST_PIDS`, or owns only windows titled with the
  nonce) and media whose title has the nonce (`media_permits`), never the
  developer's own music. Debug-only `robot_view` (the robot's
  `realtest_other_apps`).
- **`media_pause.rs`** — "Pause music and videos while you dictate":
  WinRT `GlobalSystemMediaTransportControlsSessionManager` (windows crate
  feature `Media_Control`), sessions **Playing** → explicit `TryPauseAsync`
  (never the play/pause toggle), kept; afterwards explicit `TryPlayAsync`
  only for those still **Paused**. Never a call app's session (its app id
  against the call apps' exes and package names), no browser's while a
  meeting records or is live in one (`pause_verdict`, pure). Debug-only
  `list` for the robot.
- **`history.rs`** — local-only transcription history (`history.json`): each
  dictation's timestamp, raw + final text, model, and focused app. Best-effort,
  gated by `history_enabled`. Derives the stats dashboard (words, time-saved vs
  typing, day streak, 30-day activity) without a date crate (UTC day-numbers like
  `usage.rs`). Powers `get_history`/`clear_history`/`get_stats`. Every change
  (a dictation or upload saved, an entry deleted from Home or the local API,
  all cleared) emits **`yap-history-changed`** after the write, which Home and
  Insights re-pull on. Not `yap-transcript`: a dictation emits that before it
  pastes and saves, so a re-pull on it could miss the new entry.
