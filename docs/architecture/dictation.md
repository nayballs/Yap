# Dictation

How a dictation runs: the hotkey and push-to-talk gestures, the mic, speech-to-text, the order of the passes after it, Upload, history, and other apps' sound while you talk. Where the text goes is [pasting.md](./pasting.md); the dictionary is [dictionary.md](./dictionary.md); the bar that shows it all is [yap-bar.md](./yap-bar.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## The pipeline order

The transcription itself is told what's known of the language (My
languages, `dictation/languages.rs` below: Whisper picks the likeliest of
the person's languages and says what it heard), and a language shortcut
that translates sends the transcript to the AI in cleanup's place. Hindi
written as Hinglish (My languages' "Write in Latin letters (Hinglish)",
`languages/hinglish.rs`, 2026-10-29) is spelt out in Latin letters first,
on this PC (`languages::hinglish_text`, at the start of `deliver` and Run
again), so cleanup, the words as said and history all have it so.
Pipeline order after transcription: **AI cleanup (in the style of the kind
of app it goes into — email formal, chats casual, AI prompts literal,
terminal commands as said: `ai/cleanup_styles.rs`, [ai-models.md](./ai-models.md);
the person's own app rules first, as ever; at the strength set — None /
Light / Medium / High, `ai/cleanup_strength.rs`; a spoken list or an
email's greeting and sign-off laid out, `ai/cleanup_format.rs`; with the
text before the cursor for a model on this PC, `ai/cleanup_context.rs`) →
a transform on every dictation, if one is picked (`transforms::for_dictation`,
below) → dictionary (exact, then a
fuzzy near-miss pass) → names on screen (`dictionary/screen_terms.rs`: a name heard a
little wrong spelled as the window shows it) → the "press enter" voice
command → snippets → fit to the
kind of app (`pasting/app_kind.rs`: a terminal gets one line and no trailing space; a
chat app's short message loses its full stop) and to the text around the
cursor (smart spacing; where the field couldn't be read, append-space as
before) → inject (Ctrl+V, or the terminal's own paste keys — Shift+Insert in
Git Bash, Ctrl+Shift+V in Alacritty…; a paste a Win32 text box never got is
typed instead, once; an app running as administrator takes no keys from Yap:
the text is left on the clipboard and the person told, `pasting/elevated.rs`) →
submit key (auto-submit or the voice command, once; in a
terminal Enter, only for the voice command or a coding CLI's auto-submit) —
or, with no submit key, the correction watch (`dictionary/learn.rs`: fix a word Yap
typed and it offers to add it to the dictionary)**
(`pipeline::finish_dictation_with` does the dictionary, the names on screen,
the command and snippets;
`text_injector::prepare_dictation` the fitting; `app_kind::submit_key` the
key). Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation
never blocks; so does an answer that left out most of what was said
(`cleanup_styles::lost_the_dictation`, ai-models.md). It keeps the person's **English spelling** ("Write in my
region's spelling", on, Settings → Language Models → Dictation Cleanup;
the region is About you's; 2026-10-21): a UK user's
"color" comes out "colour", a US user's "colour" "color" — one line in
Yap's own layer of the cleanup prompt (the system prompt's end and the
dictation's restated instruction: `llm::cleanup_system_prompt`,
`cleanup_message`, `about_you::cleanup_spelling`), never in the editable
preset; another language gets no line, nor does a dictation known to be in
another language (My languages, `languages::cleanup_spelling`), which gets
a line keeping it in its language instead (`languages::language_line`,
below). How well it's followed depends on
the model (the 4B mostly; the 1.5B hardly — docs/model-evals.md). The dictionary runs Handy's split (ported from its source):
**Whisper models** get the correct spellings as the decoder's `initial_prompt`
(ASR biasing, threaded through `SttEngine::transcribe` at every call site —
dictation/partials/upload/meeting — the words used most lately first,
`dict_usage::ranked`; plus an OpenWhispr-ported prompt-echo guard
in `dictation/stt.rs`), **ONNX models** get **`dictionary/fuzzy.rs`** — Levenshtein + Soundex 1–3-word
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
**"Snip"** (`snip_voice`, default on; `snip::is_snip_phrase`, checked first
in `deliver` and in `ask_assistant`): a dictation — or a Voice Assistant
question — that is only "snip", "snip it", "take a snip", "screenshot" or
"take a screenshot" starts Snip to Chat ([snip.md](./snip.md)) instead of a
paste or a question; inside a sentence ("snip the ends off") it's dictated.
**Names on screen** ("Spell names the way they're written on screen",
`screen_names`, on; [dictionary.md](./dictionary.md), `dictionary/screen_terms.rs`):
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

**The words as said** (2026-10-22): when cleanup changed the transcript,
`deliver` also makes the dictation's words before cleanup —
`pipeline::as_said_text`: the corrections (without counting them as used
again), the names on screen (the same `screen_terms::take()` the cleanup
and `finish_dictation_with` got: taken once per dictation in `deliver`),
the voice command and snippets — and fits them to the same place
(`text_injector::prepare_dictation_as_said`), so **"Use what I said"**
(`pasting/as_said.rs`, [pasting.md](./pasting.md)) can swap the paste for them; the
bar offers it for a few seconds when the words differ (`yap-state`
"cleaned", `Shared::end_state`). History keeps them beside the text
(`said`, fitted the same way; `history::record_dictation`), so Home shows
**what the AI changed** ("AI edited · N changes", a word diff) and **Paste
what I said** later ([frontend.md](./frontend.md)); the paste is then the
**last paste** `as_said` remembers, tagged with its history entry
(`as_said::recorded`). **Rewrite the last dictation** (2026-10-23): the
rewrite shortcut with nothing selected, within `as_said::REWRITE_WITHIN`
(a minute) of a paste into the window in front (not a terminal), applies
the instruction to that paste instead of writing new text — see
`dictation/pipeline.rs` below. Settings → Commands → "Rewrite your last dictation
when nothing is selected" (`rewrite_last_dictation`, on) turns it off:
nothing selected then always writes new text, as before. **Every dictation's audio** is kept a day
(`dictation/recent_audio.rs`, below) from `run_stt`'s start, and its history entry
carries it; a dictation that never got transcribed (no model, the model
failed, Yap closed mid-way) or whose paste didn't land is listed as such,
to **Run again** from Home.

## Modules

- **`dictation/pipeline.rs`** — the heart. Owns the shared state (`recording`, audio
  `buffer`, idle `preroll` ring, the mic — `dictation/mic.rs`, opened on demand —, warm
  STT `engine`, live `config`, `last_activity`, `target_hwnd`). The mic's
  audio (`mic::Sink` for `Shared`: downmixed→mono, resampled→16 kHz on the
  audio thread) is buffered while recording — and from the moment
  `start_recording` begins (`gather`), so a cold mic's first audio is all
  kept —, and emits a throttled **level** (`yap-amp`: each ~80 ms's peak on
  a perceptual scale, `dictation/meter.rs`) for the scrolling waveform;
  while idle and the mic is warm it keeps a rolling
  ~300 ms **pre-roll** ring that `start_recording` prepends (anti
  first-word-clipping; never a closed mic's stale ring). **The start chime
  follows the mic, not the key** (2026-10-22): `start_recording` holds the
  mic (`Mic::hold`, its `mic_lease`), and when it isn't live yet the chime,
  the other apps going quiet after it and the bar's "speak now" wait
  (`PendingStart`, `fire_pending_start`) for its first real audio
  (`mic::Sink::live`) — or `LIVE_FALLBACK_MS` (3 s) — while the capsule
  says "Starting the mic…" (`yap-mic-ready` false, then true). A mic that
  can't be opened ends the recording at once (`mic_failed` →
  `end_without_mic`: "No microphone found", or Windows' privacy switches);
  one that already gave audio and then fails a reopen carries on. The
  recording lets go of the mic after its stop chime
  (`stop_chime_then_restore`; a cancel at once, `release_now`), then
  `mute::after_stop_chime_mic` — told when a Bluetooth mic was just closed,
  so the restore waits for the headset to leave its call profile. **The
  speech model loads off the startup thread** (`Pipeline::start` → an
  `stt-load` thread; the tray, the bar and the window come first):
  `model_loading` / `wait_for_model` — a dictation (`run_stt`, "Loading the
  speech model…" on the bar, `yap-model-loading`), an Upload, a Run again
  or a meeting chunk that needs the engine meanwhile waits for it instead of
  failing or loading a second copy; the live partials skip until it's in.
  Debug-only `YAP_E2E_MODEL_LOAD_MS` slows the load in a test run. **The
  warm engine is lent in turns** (`EngineSlot` = `EngineCell`, 2026-10-24,
  code review #9): a dictation (`run_stt`), a meeting's chunk, an Upload's
  chunk, Run again and a meeting transcribed again each take it out for one
  transcription (`take` / `lease_engine`) and give it back as soon as it's
  done (a `Lease`, dropped — before a dictation's AI cleanup). A taker that
  finds it out **waits** for it (a dictation up to 20 s, then loads a copy
  after all; "Speech engine: in use; waiting" in the log) — until then a
  dictation that ended during a meeting's chunk loaded a second copy of the
  model. A taker that finds nobody has it loads it for everyone (the others
  wait for that load); an engine whose transcription erred or panicked
  isn't given back (the next taker loads a fresh one); a model switch
  meanwhile wins (`give_back` keeps the newer). The slot itself
  (`lock`/`try_lock`) is unchanged for its other users (the live preview,
  the idle unload, `set_engine`). Unit-tested (`a_dictation_waits_for_the_
  engine_a_meeting_chunk_has_out` fails on the old take-or-load).
  `recording_mode` selects toggle vs push-to-talk; in push-to-talk `on_key` reads
  **gestures** (`dictation/gesture.rs`, Wispr Flow's by behaviour): hold to talk, a quick tap
  (< 300 ms) + a second press within 500 ms locks it **hands-free** until the next
  press (`yap-dictation-locked` → a lock on the overlay), a lone tap is thrown away
  (never a junk dictation) and earns a coaching hint (`dictation/coaching.rs`), as does a
  hold of a minute or more. Key events and the tap timer act under one lock
  (`gesture`), timed by the input hook's own timestamps; a recording started or
  stopped elsewhere (the bar's Dictate, the tray's Cancel) is followed, and one
  started by Dictate stops on the next press. `run_stt` (async) transcribes, then
  `deliver` (also robot mode's way in, `shell/realtest.rs`; the target is captured by
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
  `meetings/meeting.rs` reads lock-free in its mic callback (`dictation_state()`) to
  keep dictations out of a meeting's "You" side. `PREROLL_SAMPLES` is shared
  with it (the meeting blanks a dictation's pre-roll too). **Commands** (two
  different things share no name any more, 2026-10-18: *Commands* type for
  you, the *Voice Assistant* answers out loud): by name (`dictation/agent_detect.rs`,
  OpenWhispr `detectAgentName` port) a dictation addressing the
  agent (`agent_name`, default "Yap"; fuzzy-matched) routes the whole
  transcript through the Commands scope (`llm_scopes["voiceAgent"]`, its key kept) in write mode (`run_agent`, shared with
  the edit hotkey = the rewrite shortcut) instead of cleanup — the agent prompt strips the name+command.
  **The rewrite shortcut with nothing selected** (2026-10-23; Wispr Flow's
  Command Mode / Transforms by behaviour): `capture_target(edit)` reads the
  selection as ever; none, and `as_said::rewrite_target(on, target)` names
  the last paste when "Rewrite your last dictation when nothing is
  selected" is on (`on`: `config.rewrite_last_dictation`, default on), it
  went into this window within a minute and isn't a terminal's
  (`as_said::rewritable`, pure) → `rewrite_last` (kept with a cancel for
  Undo); off, `rewrite_last` stays empty and it's write mode exactly as
  before. `run_rewrite` hands it to `run_agent(instruction,
  selection, last)`: before the AI is asked, `as_said::rewrite_check` (the
  window in front, not an admin app, the paste still ending exactly at the
  caret — typed after it, edited, or the cursor moved away refuses: the
  instruction may not be about it any more) → refused: "Couldn't rewrite
  your last dictation" (`as_said::cant_rewrite`, an alert), nothing pasted;
  else the paste is the text the AI changes, and its answer goes through
  `as_said::replace_last` (selected exactly again, pasted over with the
  spaces around it kept, read back; refused → the field left alone, the
  rewrite copied, said). Something selected, another window, over a
  minute, a terminal: write mode or the selection's rewrite, as before.
  Whatever `run_agent` pastes is the last paste from then on ("now
  shorter" rewrites the rewrite); a paste that didn't land forgets it.
  Robot mode: `Robot::rewrite_text` (`realtest_rewrite_text`), its report
  (`Rewritten`, debug-only). Test mode: `e2e_rewrite` (the same, nothing
  read or pasted) with `e2e_target_window` — dictations and the shortcut go
  to a stand-in window no window has (`pipeline::test_target`), the only
  one `rewrite_target` takes in a test run; edit mode reads no selection in
  a test run (`selection::capture_selection`, as every other read).
  **The Voice Assistant's shortcut** (`Pipeline::on_assistant_key`, the
  hook's `assistant-key-*` or the in-page `assistant_key`): the same
  `on_key` (toggle / push-to-talk gestures; no coaching hints for it) with
  `ASSISTANT_ARMED` set for the press, so the recording it starts is the
  assistant's (`destination(edit, assistant, chat_armed, chat_focused,
  own_window, note)` → `Goes::Paste | ChatBox | Assistant | QuickNote`, pure, in
  `capture_target`, which calls `assistant::listening` before "recording");
  `run_stt` hands its transcript, after `apply_corrections` only, to
  `assistant::heard` (`ask_assistant`): no cleanup, Command, paste or
  history; nothing heard → "Yap didn't catch a question"; a cancel isn't
  kept for Undo; every early end calls `assistant::nothing`. Nothing ends a
  recording on a pause, the assistant's question included ("Stop listening
  when I pause" was removed 2026-10-25: [chat-and-assistant.md](./chat-and-assistant.md)).
  `stop_and_transcribe` does nothing for a recording that's already stopped
  (two stops racing).
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
  keeps it in history. **Dictations for the quick note** (2026-10-28,
  `to_note`, [notes.md](./notes.md#the-quick-note-notesquick_noters)):
  while the quick-note window is the one in front, or its mic button asked
  (`quick_note::takes_dictation`, the `note` of `destination`: after the
  Chat view's own mic, before a focused chat box), a dictation is the
  note's — cleaned up as any (no Commands by name), then
  `deliver_to_note` emits `yap-quick-note-dictation {text, newline}` for
  its page to put at the cursor; nothing pasted, kept in history; `to_note`
  lives through a cancel, so Undo lands there too. A `processing` guard blocks
  starting a new recording while one is still transcribing (no overlapping `run_stt`
  / duplicate model load) — the press is ignored, and since 2026-10-25 the
  bar says "Hold on, still transcribing…" (`yap-dictation-busy`, for
  2.5 s) — and a recording stops at its **length limit**
  (`dictation/cap.rs`, below: 15 minutes, warned at 14, nothing lost; the
  buffer never grows past it, so a stuck key can't OOM). **The guard goes with the task, however it ends** (`Busy`,
  2026-10-23): a stop's `run_stt`, Undo, Upload, Run again and robot mode's
  dictations each hold a `Busy` (`take`: refused while held; `hold` for a
  stop), spawned with it (`spawn_busy`) or done by hand (`done`). A panic in
  the task (a bug: `app_kind::word_before`'s was one) used to skip the line
  after the `.await` that cleared the flag — every press ignored until a
  restart, the bar on "processing", the updater waiting, nobody told. Now
  the flag drops with the task, and `Holder::crashed` first puts things
  back and says so: for a dictation `undoing` cleared, a Voice Assistant
  question ended (`assistant::nothing`), the dictation listed as failed
  with its audio ("The dictation crashed", Run again) and "Dictation failed
  — Yap hit an error. Try again" on the bar (`yap-dictation-error` +
  "error"); an Upload `yap-upload-error`; Run again a `yap-error` toast.
  Unit test `a_transcription_that_crashes_lets_dictation_go_and_says_so`;
  e2e `dictation-alerts.spec.js` (debug-only `e2e_crash_next_dictation`
  panics the next dictation's paste step on purpose). With `streaming_partials` on, `stream_partials` (a per-session worker)
  transcribes a bounded **sliding window** of the buffer every ~500 ms
  (`dictation/partials.rs`: text older than the window freezes as committed text, the
  window advances at quiet points via `media::quietest_index`, 20 s hard cap —
  per-tick cost independent of recording length; adaptive backoff on slow
  machines, warm-loads the engine if it was idle-unloaded) and emits
  `yap-partial` text. **Words settle once two decodes agree on them**
  (2026-10-26, LocalAgreement as in whisper_streaming; it replaced
  FluidVoice's `smartDiffUpdate`): each decode of the window changes its mind
  somewhere — an "um", "Claude." / "Claude,", "haiku" / "hiking" — and the
  old prefix-keeping rewrote the whole line from the first change. Now the
  window's words are `confirmed` (shown for good, as first shown) or
  `pending` (the newest decode's last words, which may still change); a new
  decode is lined up with the confirmed words by their last three
  (`find_end`: the very last word matching first, then most words, then the
  nearest place), and one that can't be lined up changes nothing, twice at
  most (`MAX_MISSES`), then is lined up by count; filler sounds (um, uh…)
  never show. At a window advance the words shown up to where the frozen
  part's decode ends among them are frozen **as shown** — never re-decoded
  onto the line — and those after the cut stay; an empty frozen part
  freezes nothing (freezing everything shown, as before, showed the
  window's words twice once the new window decoded them). Replayed on a
  tester's 30 dictations (8 minutes, `yap.exe partials-dump`, below): shown
  words that changed 3,235 → 293, lines shorter than the one before 42 →
  11, the last line as close to the final transcript (median 0.97) — a
  word confirmed wrongly stays wrong in the preview; the paste is always
  the final pass. **`yap.exe partials-dump <audio.wav>… --out ticks.json`**
  (debug builds, `--features engines`; `dictation/partials/dump.rs`) feeds
  recordings to the model on the worker's timing and writes every tick's
  decode and line; `--ticks ticks.json` replays them through today's
  stitching with no model (compare one stitching with another on the same
  words); it prints lines shown, lines shorter, shown words changed. An **idle watcher** unloads the model after
  `model_unload_timeout`; the next dictation lazily reloads it. **No microphone** (none
  plugged in, a headset that connects after login, a CI runner) doesn't take the
  pipeline down: no stream is open until a recording needs one, every recording
  tries to open it (`dictation/mic.rs`) — the first microphone in the person's
  order that's connected, then Windows' default (`dictation/mics.rs`, below) —
  and a recording attempt with none says "No microphone found" (on the overlay) — or, when
  Windows' privacy switches are why, "Windows is blocking microphone access for
  desktop apps" (`dictation/mic_privacy.rs`) — Upload, models and Settings work regardless. **Dead-mic watchdog** (`dictation/mic_watch.rs`): the audio
  callback tallies each recording's live audio (`mic_tally`: how much, any sample
  not exactly 0.0); a per-recording watcher (`spawn_mic_watch`) reopens the stream
  once (`reopen_mic` → `Mic::reopen`) after 2 s without audio or 3 s of
  nothing but zeros — counted from when the stream runs, a cold mic's open
  (a Bluetooth headset's second or two) not counted, up to
  `OPEN_PATIENCE_MS` (6 s) —, and at
  5 s says "No sound from <mic> — check it isn't muted or unplugged"
  (`yap-dictation-error`, plus `yap-mic-silent` for the overlay's line) while
  the recording carries on;
  sound coming back takes it back, and a recording that ends still silent isn't
  transcribed (the overlay ends on the reason). **Windows' microphone privacy
  switches** (`mic_privacy::blocked`) are checked when a recording starts — if
  they block desktop apps it's said at once (`say_mic_blocked`: the overlay's
  "Windows is blocking the microphone" + an alert with **Open settings**,
  `notices/alert.rs`; `mic_dead` set, so a stop with no sound isn't transcribed; the
  recording goes on, and sound arriving takes it back) — and again before the
  watchdog's report, which then blames the switch instead of the mic.
  Opening, closing, the device switch and the reopen all happen on the mic's
  own thread (`dictation/mic.rs`; the reopen stops the dead stream first).
  **The mic unplugged mid-dictation** (2026-10-25, `pipeline/unplugged.rs`):
  until then the recording went on hearing nothing (its stream had stopped
  and the watchdog had stopped watching once the mic first worked). Now the
  mic's thread tells the dictation recording from it (`mic::Sink::lost`,
  on a thread of its own) and pure `on_lost(recording, ours, robot, heard,
  samples)` decides: it heard something (≥ 0.25 s, not all zeros) → it ends
  as a stop would (`stop_and_transcribe`: stop chime, other apps back) and
  what it heard is transcribed and kept like any dictation (history, its
  audio a day in `recent_audio`), pasted **only into the window it was
  for** (`unplugged` set → `inject_text`'s strict paste, as Undo's: a window
  gone leaves it on the clipboard, and
  Home lists it as "Didn't paste · Your microphone was disconnected and its
  window was gone", with Copy and Run again); nothing heard → it ends at once
  (`end_now`, shared with `end_without_mic`), the bar saying "<mic> was
  disconnected". Either way **one** alert (`lost_alert`, kind `micLost`,
  button **Microphones** → Settings), by what became of the words
  (`Outcome`): "Contoso H2 disconnected · Yap stopped the dictation and
  pasted what it heard. The next one uses Fabrikam USB Mic." — or "What it
  heard is on the clipboard: press Ctrl+V." (instead of Undo's own "Your text
  is copied"), "kept what it heard" (the Chat box, the assistant, no words:
  said once the transcription is done) or "before it heard anything"; the
  paste says it (`tell_unplugged`, once). The next microphone named is the
  next one in the order, which the next dictation opens (the one lost passed
  over while Windows still lists it). Unit tests (`unplugged::tests`), e2e
  `mics.spec.js`.
  **Cancel and Undo**: `cancel(undo)` — the bar's ✕, the tray's Cancel and
  `cancel_recording` keep a recording of ≥ 0.5 s that heard a sound in `kept`
  (`Kept`: its audio, `target_hwnd`, `target_kind`, edit mode and selection)
  for `UNDO_MS` (10 s): `yap-dictation-cancelled { undoMs }` + `yap-state`
  "cancelled" (the bar's "Dictation cancelled · Undo"), then "idle" and it's
  dropped; a new dictation drops it too (`capture_target`), and
  push-to-talk's lone tap is simply thrown away (as is a press that grew
  into a chord, below). **Never lost** (2026-10-25): a kept cancel's audio
  also goes to `recent_audio` at once and history lists it as **Cancelled**
  (`keep_cancelled` → `history::record_cancelled`; not edit mode's
  instruction, never the assistant's question) — Home's **Transcribe** is
  Run again's path, for its day (or longer with **Keep**); Undo takes it
  back out (`history::forget_audio` + `recent_audio::discard`: it's
  transcribed as a new one). **A ten-minute dictation** (`stop_and_transcribe`,
  `cap::meeting_hint_ms`; not a rewrite's instruction or a question)
  suggests meeting notes once it's delivered (`suggest_meeting_notes`:
  "Long dictation? Try meeting notes", naming the meeting shortcut — they
  go on past the 15-minute limit and summarise; `coaching.rs`, once ever). `Pipeline::undo` (`bar_action`
  "dictation-undo") puts the target back and runs `run_stt` on the kept audio
  as a stop would — the paste goes back to the window the dictation started
  in, whatever has the focus now, or nowhere (`undoing` → `inject_text`'s
  `strict`: a window that's closed or won't come to the front leaves the
  text on the clipboard, `Delivery::WindowGone`, with "Your text is copied:
  press Ctrl+V") — after reading that window's cursor context afresh (a
  closed window's never: `read_context` refuses a target whose process is
  gone). `Pipeline::stop` is the bar's ✓ (the hotkey's second
  press). `holds_undo` keeps the updater from restarting under a pending Undo.
  **Kept audio and failures** (`dictation/recent_audio.rs`): `run_stt` keeps the
  audio first (`audio_id`), `deliver_text` hands its id to the history entry
  (with `notPasted` when the paste didn't land: the app runs as
  administrator, its window was gone, an inject error), `list_failed` lists a
  dictation that never got transcribed (no model, the STT failed or
  panicked) and `drop_kept_audio` drops what nothing took; `Rerun`
  (`Pipeline::rerunner`) runs a kept one again. **The bar's offer**
  (`end_state`): "cleaned" instead of "idle" for `as_said::OFFER_MS` after a
  paste AI cleanup changed the words of (`Delivered.offer`), then "idle"
  unless something came after. `deliver` is cleanup (`clean_up`, also Run
  again's) then `deliver_text`.
- **`dictation/gesture.rs`** — push-to-talk's pure gesture state machine: key down/up and
  the clock in (`now_ms`, monotonic, also what the input hook stamps events
  with), Start / Stop / Discard / Lock + an optional hint + "tick me in N ms"
  out. States Idle → Held → (TapPending → LockHeld → Locked). `TAP_MAX_MS` 300,
  `DOUBLE_TAP_GAP_MS` 500, `LONG_HOLD_MS` 60 s. Every call is told whether a
  dictation records right now and follows that first. `hands_free()`:
  locked (LockHeld / Locked), what the Voice Assistant's pause asks before
  it ends a question. **`chord(now)`** (2026-10-25, Wispr Flow's
  other-shortcut guard): another key down under `CHORD_MS` (1 s) into a
  hold (Held only: never locked, never a tap's wait) → Discard + the
  `ChordHold` hint — the person meant a shortcut sharing the key (AltGr+E,
  Ctrl+Win+D); the pipeline drops it silently (`cancel(false)`: no stop
  chime, no Undo, nothing kept). **`key_lost()`**: the input hook's sweep
  let go of a stuck key → Held / LockHeld become Locked (`Unstick`: the
  bar's hands-free lock, `yap-dictation-locked`, not counted as double-tap
  used) — never a start or a stop. **Triple-tap** (`set_triple_tap`,
  `triple_tap_meeting`, off): a double-tap whose second press was a tap
  too, and a third press within `DOUBLE_TAP_GAP_MS` of it → `TripleTap`
  (the pipeline throws the dictation away and calls
  `meeting_guard::start_or_stop`; during a meeting it just stops the
  dictation, never the meeting). Unit-tested.
- **`dictation/coaching.rs`** — push-to-talk coaching hints: a lone tap → "Hold {key} while
  you talk" (or double-tap for hands-free), a minute-long hold → "Double-tap
  {key} for hands-free", a press that grew into a chord → "Hold only {key}
  to dictate" ("Press only…" in toggle mode; 2026-10-25), and a dictation of
  ten minutes → "Long dictation? Try meeting notes" (naming the meeting
  shortcut, `{key}` its spec; once ever, `max_shows`). Each ≤ 3 times, never
  within 10 min of another, the tap and long-hold ones none
  once double-tap has been used; counts in `<data>/hints.json` (not config.json:
  Settings' auto-saved copy would write old counts back). Surface (one
  place, `notice::place`): a Tip toast in the main window while it's focused
  (`yap-hint`), else a Yap bar card (icon `tip`; it waits out a dictation or
  a meeting and is dropped if it can't show within 10 s), else the window's
  toast if it's on screen; neither → nothing shown, nothing counted (never a
  Windows notification). "Show tips" off (`show_tips`) → none. Texts say
  `{key}`; the page names the hotkey (`fillHotkey` in `lib/core/hotkeys.js`; the
  card's `hotkey` field). Not for the edit hotkey.
- **`dictation/input_hook.rs`** — low-level Windows keyboard + mouse hooks; specs `kb:VKEY`,
  `kb:ctrl+shift+VKEY` (modifier combo; `win` is a modifier too, e.g. the
  meeting shortcut `kb:alt+win+77` = Win+Alt+M), `kb:165` (single right-side modifier, e.g.
  RightAlt — never suppressed, it's AltGr), `mods:ctrl+alt` (modifier-only chord) /
  `mouse:ID` — combo semantics ported from OpenWhispr's `windows-key-listener.c`
  (press = key down w/ required modifiers held, release = key up OR required
  modifier up; chords fire on completion; suppressed keys are excluded from the
  GetAsyncKeyState self-heal — the hook eats them before the key-state table
  updates). Ten bindings: dictation, edit (`edit-key-*`, Commands' rewrite
  shortcut), the meeting
  shortcut (`meeting-key-*`, `configure_meeting`; only its press is used —
  `meeting_guard`) and the Voice Assistant's (`assistant-key-*`,
  `configure_assistant`; unbound by default; `configure_assistant_from`
  leaves one that clashes unbound), Read aloud's (`read-key-*`,
  `configure_read`/`_from`, unbound by default; only its press is used —
  `read_aloud::read_selection`) and "Use what I said" (`as-said-key-*`,
  `configure_as_said`/`_from`, unbound by default, giving way to any of the
  other five; only its press is used — `as_said::swap_soon`), then Snip to
  Chat's (`snip-key-*`, [snip.md](./snip.md)), then **Paste last
  transcript** and **Copy last transcript** (2026-10-25; `paste-last-key-*`
  / `copy-last-key-*`, `configure_paste_last`/`_from`,
  `configure_copy_last`/`_from`; unbound by default, each giving way to
  every shortcut older than it — `older_shortcuts`, `unless_clash`; only
  the press is used — `pasting/last.rs`, [pasting.md](./pasting.md)), then
  **Quick note** (2026-10-28; `quick-note-key-*`, `configure_quick_note`/`_from`,
  unbound by default, giving way to all nine older ones; its press and
  release since 2026-10-29 — `notes/quick_note/key.rs`: a tap opens or hides
  it, a hold dictates into it — and one of the hold keys, `HOLDS`, so a
  chord or a stuck key reaches it too; `quick_note_key_up` lets go of it
  when a Yap window's page heard its release, [notes.md](./notes.md#the-quick-note-notesquick_noters)). Beside them, **up to 12
  saved transforms' shortcuts** (`TRANSFORM_BINDINGS`, a slot each;
  `configure_transforms_from` at setup and every save,
  `transforms::assign` giving each transform with a key a slot in list
  order unless it would fire with one of the ten or an earlier
  transform's — left unbound, logged; `configure_transform(id, spec)` is
  Settings' pause while a key is picked); a slot's press goes to the
  forwarder as `transform-key-N` and out as one `transform-key-pressed`
  event carrying the slot (releases dropped), which `lib.rs` hands to
  `transforms::on_key` — see `dictation/transforms.rs` below. Before the
  transforms, **four language shortcuts' slots** (`LANGUAGE_BINDINGS`,
  My languages: `configure_languages_from` / `configure_language`,
  `languages::assign`; their press and release go out as one
  `language-key {slot, pressed, at}` event — `dictation/languages.rs`
  below). **More shortcuts for one action** (2026-10-25, Wispr Flow's "Add
  another"): each of the ten fixed actions (`FIXED`, indexed by
  `DICTATION`…`QUICK_NOTE`, with `FIXED_KEYS` / `FIXED_NAMES` /
  `FIXED_EVENTS`) has its first shortcut (its old config field) and up to
  `MORE` (3) more (`MORE_BINDINGS`, `config.more_hotkeys`,
  `configure_more_from` at setup and every save; Settings' live pause
  `configure_more` → `configure_more_hotkeys`). `assign_more` (pure): each
  gives way to every first shortcut and earlier more ones — its own
  action's too —; `transforms::action_shortcuts` lists them, so language
  shortcuts and transforms give way to them all. An action's keys act as
  one (`handle_binding_event`'s `action`: the press goes out when the first
  goes down, the release when the last comes up). `tidy_more` (at `load`)
  keeps the lists clean. **What's held** (`HELD`, a bit per virtual key;
  `is_held`, `held_keys`): every key the hook saw go down and not up,
  swallowed ones included (Windows' own key state never shows those —
  measured: a key an LL hook swallows never reaches `GetAsyncKeyState`).
  **A hold that grows into a chord**: a key going down that wasn't held
  (`note_new_key`; not the binding's own key or modifiers, AltGr's made-up
  left Ctrl, nor the menu-mask key — `held::another_key`) under a second
  after a hold-to-talk press (`HOLDS`: dictation, rewrite, assistant, Quick
  note, and the language slots) is sent once as `CHORD` → `pipeline::key_chord`
  (and `quick_note::key::key_chord`; the stuck-key sweep's `STUCK` reaches
  both too)
  (push-to-talk: `gesture::chord`; toggle: the press that started this
  recording, `GestureSlot.toggle_press`) — dropped silently, with the "hold
  only {key}" tip. **The stuck-key sweep** (`sweep`, on the hook's thread;
  `input_hook/held.rs`, pure, unit-tested): every `SWEEP_MS` (5 s), at once
  on `EVENT_SYSTEM_DESKTOPSWITCH` (the lock screen, a UAC prompt, and
  back), and on `sweep_now` (a push-to-talk hold of a minute,
  `ask_sweep_after_a_long_hold`; a dictation near its length limit). It
  notices a sleep since the last sweep (the tick count ran > 2 s past the
  unbiased interrupt time) and input the hook missed (`GetLastInputInfo`
  newer than `HEARD_TICK` by > 1 s: another desktop, a hook Windows took
  away) — `Evidence::Away` — and checks every active binding: a chord, a
  modifier bound alone or a combo's modifiers against `GetAsyncKeyState`
  (never judged in its first second, `GRACE_MS`); a swallowed key by
  evidence only — the keyboard's repeats of it stopped ≥ 2 s ago with no
  other key down since (a key that never repeated, a mouse button or a key
  a mouse's software sends, is never judged so), or `Away`. A stuck one is
  let go **without its release event** (a warning in the log: which
  shortcut and why); one that held a recording sends `STUCK` →
  `pipeline::key_stuck` → `gesture::key_lost`: the recording goes on
  hands-free, the next press stops it. The table is kept honest too (a key
  no binding holds that Windows says is up is forgotten). **Robot mode**
  (`realtest.rs`, debug builds): the hook runs, acting only on events whose
  extra info is `ROBOT_MARK`; `UNHEARD_MARK` events pass by unheard (the
  robot's lost key-up); everything else passes untouched (the developer's
  keys are never seen). Debug-only `e2e_hotkey_bindings` (`binding_states`).
  **`clashes(a, b)`** (pure, unit-tested;
  `hotkeysClash` in `lib/core/hotkeys.js` is the same rule, Settings refuses with
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
  `ui/HotkeyInput.svelte` (with a guided **Mouse button** pick, 2026-10-25:
  "Press the mouse button you want to use" — the middle one or a side
  button; left and right refused, saying why, as is a button the hook
  can't hear) + shared `lib/core/hotkeys.js` (parse/format/match — also
  drives the in-window fallbacks; combos read Win first, "Win + Alt + M"). ⚠ **Known Windows
  gotcha:** when one of Yap's OWN WebView2 windows has focus, the LL hook never
  receives the hotkey (WebView2/Chromium front-runs the hook chain on focus) —
  so the Settings + onboarding pages catch the hotkey **in-page** (keydown and
  keyup → `dictation_key { pressed }`, the hook's own path into `on_key`, so
  push-to-talk's gestures work there too; Settings and the notepad also catch the meeting shortcut →
  `meeting_shortcut`, and the assistant shortcut → `assistant_key { pressed }`). Any new Yap window with focusable UI needs the
  same fallback. **The Copilot key** (new Windows 11 laptops; the keyboard
  sends Win + Shift + F23) is the combo `kb:shift+win+134` (`COPILOT_KEY` in
  `lib/core/hotkeys.js`, shown as one key, "Copilot", "the Copilot key" in
  hints). Nothing tells a PC that has one apart, and Windows may act on it
  before a focused Yap window sees F23, so the shortcut picker offers a
  **Copilot key** button while it captures (Settings, onboarding). Checked
  on a release build's real hook (2026-10-22, injected with `SendInput`,
  both as one burst of all six events and spaced 30 ms apart, with an
  observing LL hook installed behind Yap's): the press starts and the next
  stops a dictation, F23 never reaches the next hook, the menu-mask key
  goes down before Win comes up, and nothing else takes the front. Without
  Yap the same keys opened Windows Settings on that PC (a desktop with no
  Copilot app). Many laptops send the key's press and release together, so
  holding it can't keep push-to-talk going: Settings → Dictation says so
  (double-tap for hands-free, or Toggle).
- **`dictation/transforms.rs`** — **saved transforms** (2026-10-24; Wispr Flow's
  Transforms, OpenWhispr's translation hotkey, Murmure's numbered rewrites,
  by behaviour): one-key rewrites applied **without speaking**. The list
  (`config.transforms`, Settings → Commands → Transforms,
  [frontend-settings.md](./frontend-settings.md)): Yap's five built-ins —
  Polish, Shorter, Fix grammar only, Translate to <language> (a picker),
  Make it a prompt — whose names and instructions are Yap's
  (`builtin_instruction`, measured in docs/model-evals.md "Saved
  transforms"; a stored name or instruction on a built-in is ignored), and
  the person's own (name + instruction, edited, moved, deleted); at most
  `MAX` (12), each with an optional shortcut. **A press** (`on_key` → its
  own thread → `run_by_id` → `pipeline::Transformer::hold`: the
  `processing` guard (`Job::Transform`), refused while a dictation records
  or transcribes, "Yap is busy with a dictation") runs `run`: the
  shortcut's modifiers let go first (`as_said::wait_for_modifiers_up`); the
  window in front (the tray's menu: the top app window brought back, as
  "Read selection aloud" does); its kind; what's selected
  (`selection::editable_selection`: a field that takes typing — read-only
  text or a password box refused —, UI Automation, Ctrl+C only where it
  can't read the field, never in a terminal); then **`pick`** (pure,
  unit-tested): a **terminal never** ("Transforms don't work in a
  terminal", nothing typed), else **the selection**, else — nothing
  selected — **the last dictation** under the rewrite shortcut's exact
  rules (`as_said::rewrite_target`: "Rewrite your last dictation when
  nothing is selected" on, the same window, within a minute; then
  `rewrite_check`, the paste still ending at the cursor — else "Couldn't
  polish your last dictation", the AI not asked), else **"Nothing to
  transform"**, said plainly (alerts, `notices/alert.rs`). The model is the
  **Commands scope's** (`pipeline::commands_model`, shared with
  `run_agent`: its own provider when set up, else the cleanup model; the
  on-device sidecar or a cloud key) with **`llm::transform`**
  (`TRANSFORM_BASE_PROMPT`'s guardrails + the instruction; never the
  Commands prompt, which may answer questions; [ai-models.md](./ai-models.md))
  and `clean_output` (an echoed `<text>` tag or a "Here is…:" line off).
  Polish, Shorter and the person's own carry the **writing samples** for the
  window's kind of app (`ai/writing_samples.rs`: `for_transform`, the
  receipts' "Commands + your writing samples"); Fix grammar only, Translate
  and Make it a prompt never do.
  The bar says the name meanwhile (`yap-transform` "running" + `yap-state`
  "processing": "Polish…"). **In place**: over the selection after
  `verify_selection` (the window still in front, the same text still
  selected — else the field is left alone and the result copied, said),
  with the selection's own spaces kept (`as_said::keep_spaces`), back into
  its window or nowhere (`inject_text` strict); or over the last dictation,
  selected exactly again (`as_said::replace_last`). One paste either way
  (the app's own Ctrl+Z takes it back), and the last paste from then on
  ("Shorter" after "Polish" works on the polished text). **Undo** on the
  bar for `as_said::OFFER_MS` ("Polish ✓ · Undo", `yap-transform` "done" +
  `yap-state` "cleaned"; `bar_action "transform-undo"` →
  `as_said::undo_transform`: the result selected exactly, still ending at
  the caret, and the text it replaced pasted back; refused → that text
  copied, said). The same text back from the AI: nothing pasted,
  "Polish: no changes needed". The AI failing or no model: on the bar as a
  rewrite's failure is. Also the tray's **Transforms** submenu (`shell/tray.rs`,
  rebuilt when the list changes); not the bar's ^ menu, which manages the
  bar and Yap — the bar shows a transform running and its Undo instead. No
  in-page fallback in Yap's windows: a transform works on another app's
  text. The log says which transform (a built-in's kind, or "custom"), the source, the outcome and
  lengths, never the text. Test mode: `e2e_transform { id, selection?,
  terminal? }` (the stand-in window, `e2e_target_window`; nothing read or
  pasted), `e2e_transform_slots`; robot mode `realtest_transform`.
  Unit tests (`transforms::tests`, `input_hook`'s slot test, `as_said`'s
  undoable), e2e `transforms.spec.js`, robot `e2e-real/transforms.real.js`
  (written, not yet run). **A transform on every dictation** (2026-10-25;
  Wispr Flow's auto-applied transform, by behaviour; optional, off by
  default): Settings → Commands → Transforms → "Run on every dictation" picks
  one (`config.auto_transform`) and where (`auto_transform_kind`: every app,
  or one kind of app — `cleanup_styles::category`). `for_dictation` (pure):
  none in a terminal (by its kind or what `app_kind` read), none in another
  kind, none for a transform that can't run (deleted: Settings clears it).
  It runs **once**, in `pipeline::clean_up` after the cleanup
  (`Shared::transform_after`: `run_on_dictation` — the Commands scope's
  model, `llm::transform`, `clean_output`, as a dictation's AI call), on what
  the style's cleanup wrote, watched like the cleanup (the bar's Skip gives
  the cleaned text back); its result is never cleaned or transformed again,
  a transform's own shortcut never starts it (no dictation is delivered), a
  language shortcut that translates skips cleanup and it, Run again runs it
  once on its fresh transcript, the Chat view's box never gets it. With
  cleanup off it still runs on the words as said; no model or a failure
  pastes the text as it was (logged, lengths only). "Use what I said" swaps
  the paste back to the words as said. e2e `cleanup-strength.spec.js`.
- **`dictation/languages.rs`** (+ `languages/catalog.rs`, `languages/nudge.rs`)
  — **My languages** (2026-10-25; Wispr Flow's languages and its "Speaking a
  different language?" nudge of 28 Sep, OpenWhispr's dual-language hotkeys
  request, by behaviour; Settings → Dictation → My languages,
  [frontend-settings.md](./frontend-settings.md)).
  **The languages** (`config.my_languages`, codes in the person's order;
  `my_languages`: a config from before keeps the one language
  `selected_language` named, no migration). **`catalog.rs`** (pure): Whisper's
  hundred languages (code, English name, own name: the search), which model
  covers which and **how it learns the language** (`Tells`): Whisper (Small,
  Medium, Large v3, Turbo; Breeze ASR for Chinese and English) all of them
  and says which it heard (`Probabilities`); Parakeet V3 (25 European) and
  SenseVoice (Chinese, Cantonese, English, Japanese, Korean) recognise it by
  themselves and never say (`Itself`); Canary 1B v2 (the same 25), Canary
  180M Flash (en/de/es/fr) and Cohere (14) must be told (`Told`, English if
  not); Parakeet V2, Moonshine and Distil Whisper are English only, GigaAM
  Russian only (`One`). `recommend` names the model for a set of languages:
  English alone → Parakeet V2 if installed, else V3; otherwise the first of
  Parakeet V3, SenseVoice (only with a CJK language), Whisper Large v3 Turbo,
  Large, Medium, Small, Canary 1B v2, Cohere, GigaAM that covers them all,
  an installed one first. Settings says it; **Yap never changes the model**
  (the maintainer's Parakeet V2 stays).
  **What a dictation tells the engine** (`plan`, pure → `stt::Spoken`): a
  language shortcut's language; else the main shortcut's fixed one
  (`selected_language` still holds it: "auto" or a code); else
  `Among(my languages)`; none → `Auto`, as before. `stt::SttAdapter::
  transcribe_spoken` (`run_stt` and Run again; Upload, meetings, the live
  preview and the speed benchmark keep `selected_language`): a model that
  tells is first asked which language it is
  (`SttEngine::detect_language`: Whisper's own detection —
  `whisper_lang_auto_detect` on its first 30 s, the vendored transcribe-rs's
  `WhisperEngine::detect_language`, see its `YAP.md` — exactly what
  `full()` ran by itself when given no language, so it costs nothing more:
  the language picked is then passed in), and `choose` (pure) picks **the
  likeliest of the person's languages** — Whisper restricted to them, so a
  short English dictation isn't heard as Welsh — and returns what it heard
  (`stt::Heard`: the likeliest of all, its probability, the one picked); a
  model that must be told gets the first of them it knows; one that
  recognises by itself, or knows one language, gets nothing (Parakeet V2: no
  change). `heard` keeps it for this dictation (the log: codes and
  probabilities). **Language shortcuts** (`config.language_shortcuts`,
  `LanguageShortcut { id, language (code | "auto"), translateTo, hotkey }`,
  at most `MAX_SHORTCUTS` 4): `input_hook.rs`'s four language slots
  (`configure_languages_from`; `assign`, pure: never a key that fires with
  one of the ten fixed shortcuts (Paste last, Copy last and Quick note included) or an earlier one; **transforms give way
  to them**, `transforms::fixed_shortcuts`), press **and release** as
  `language-key {slot, pressed, at}` (`init` → `Pipeline::on_language_key`),
  plus the in-page fallback (`language_key`). A press records exactly as the
  dictation key's (toggle, push-to-talk's gestures): `arm(id)` around
  `on_key`, taken by the recording it starts (`begin_dictation`, from
  `capture_target`; not for the rewrite shortcut's instruction or the Voice
  Assistant), kept until the next recording (an Undo keeps it).
  **Translate on a shortcut** (`translate_dictation`, in `deliver` before
  cleanup): a shortcut with `translateTo` sends the transcript to
  `llm::transform` with `translate_instruction(from, to)` (the language
  spoken, the target, fillers out, names/numbers/links kept, a question
  translated never answered — measured, docs/model-evals.md "My languages")
  on the **Commands scope's model** (`pipeline::commands_model`: its own
  provider, else the cleanup model — on this PC or in the cloud), in AI
  cleanup's place; `transforms::clean_output`. Spoken in the target language
  already: nothing to translate (cleanup as ever). No model, or it fails:
  pasted as said, and "Couldn't translate into Spanish" (an alert) — a
  dictation never waits on it. The words as said stay one **Use what I
  said** away (the bar's offer: the original language). **Cleanup in the
  right language** (`clean_up` in `pipeline/delivery.rs`): `language_line`
  just before the transcript (with the style's line and the names on
  screen, never in the system prompt the server caches): "This transcript is
  in French: write the cleaned text in French, never translating it." when
  it's known (Whisper heard it, or a fixed language the model knows), else —
  My languages holding one besides English — "This transcript may be in
  English or French: keep it in the language it is in, never translating
  it."; none for English. The region's spelling line (`about_you`) only
  where it may be English (`cleanup_spelling`). **The nudge**
  (`languages/nudge.rs`): Whisper sure (`MIN_P` 0.85) of a language not in
  My languages, after a dictation of at least 2 s, "Tell me when I speak a
  language I didn't pick" on (`language_nudge`, default on) → "You spoke
  French · Add it to My languages?" (Add French / Not now / Don't ask about
  French) in one place (`notice::place`: the main window's toast,
  `lib/notices/languageNudge.js`, else a Yap bar card `language-nudge`;
  never a Windows notification), on its own thread once no dictation runs
  (`notice::moment`, up to a minute; the policy holds the card too); at most
  one per 10 minutes, Not now twice or Don't ask → never again for that
  language (`<data>/language_nudges.json`); Add → `shell::settings::
  change_config` (saved, applied, every window told). Only Whisper says
  which language it heard: with any other model Settings says the nudge
  can't work. Test mode: the stub engine's detection stands in
  (`e2e_language_stage {shortcut?, heard?, fresh?}`, also the next
  `e2e_dictation`'s language), `e2e_language_heard {heard, secs}` (a
  transcription in, without a mic), `e2e_language_slots`. Unit tests
  (`languages::tests`, `catalog::tests`, `nudge::tests`, `input_hook`'s
  language slots), e2e `languages.spec.js`, robot
  `e2e-real/languages.real.js`. **Hinglish** (2026-10-29; Wispr Flow's
  "Hindi in Latin letters", by behaviour; `languages/hinglish.rs`): with
  Hindi among My languages, the page offers "Write in Latin letters
  (Hinglish)" (`config.hinglish`, off) and says which speech model hears
  Hindi — **only Whisper's** (Small, Medium, Large v3, Turbo; `catalog.rs`:
  not Parakeet V2 or V3, Canary, Cohere, SenseVoice or Breeze), so a
  Parakeet user is told to pick Whisper Large v3 Turbo. On, a dictation in
  Hindi (Whisper heard it, or not known — never Marathi or Nepali, which share
  the script: `writes_hinglish_in`) has its Devanagari **spelt out in Latin
  letters on this PC** before anything else (`hinglish_text` → `to_latin`,
  pure: Yap's own table — letters and vowel signs, Hindi's dropped "a"
  ("kal", "karna", "samajhta"), "aa" in a word's first syllable and "a" at
  its end, "ee" inside and "i" at the end — and two lists: Hindi's
  commonest words as they're typed ("hai", "nahi", "mein", "chahiye") and
  English words Whisper writes in Devanagari, back in English (मीटिंग →
  "meeting"); Latin letters Whisper wrote are left as they are; dandas as
  full stops, Devanagari digits as 0–9, a sentence's first word with its
  capital). Then AI cleanup, when it's on, is told it's Hinglish
  (`language_line_with`: "write the cleaned text in Latin letters, the way
  Hinglish is typed (hai, nahi, kya, accha), English words in their English
  spelling, never translating it and never writing Devanagari"), and
  whatever it still writes in Devanagari is spelt out too; a cleanup that
  kept under half the words (it translated them: the bundled Qwen2.5-1.5B
  did, 24 times in 30) gives way to the words as Yap spelt them out
  (`hinglish_kept`, `hinglish::kept_the_words`). Measured: the 4B 30/30
  from the rules' Latin, against 25/30 given the Devanagari itself. A language
  shortcut translating into Hindi writes Hinglish too. No dependency: the
  only Rust crate found (vidyut-lipi, MIT) writes Sanskrit's schemes (IAST,
  ITRANS), with no dropped "a" and no Hinglish spellings. Off, or Hindi not
  among My languages: everything exactly as before (`language_line` and
  `hinglish_text` unchanged, unit-tested). Measured in docs/model-evals.md
  "Hinglish". e2e `hinglish.spec.js`.
- **`dictation/mic.rs`** — **the dictation's microphone, opened on demand**
  (2026-10-22). Until then Yap held Windows' default mic open the whole time
  it ran, for the pre-roll: Windows showed the mic in use all day, a
  **Bluetooth headset dropped to its call profile (HFP) system-wide** (music
  in call quality because Yap was open) and the idle stream was most of
  Yap's idle CPU. Now one thread (`dictation-mic`; cpal streams are `!Send`)
  owns the stream and opens it when something **holds** it — a dictation
  (`Mic::hold` in `start_recording`, released after its stop chime,
  `Mic::release`; leases, so an old release never lets go of a newer
  dictation's hold), onboarding's mic check (`Mic::test`, from
  `set_mic_test`; closing the onboarding window turns it off, since WebView2
  says a hidden page is visible) or **"Keep the microphone ready"**
  (`config.keep_mic_ready`, off; Settings → Dictation → Microphone and sound;
  `Mic::settings` on every config save) — and closes it when nothing does:
  after a **warm window** (`WARM_MS`, 60 s; debug-only `YAP_MIC_WARM_MS`) so a
  quick follow-up starts at once with the pre-roll, or **at once for a
  Bluetooth input** (`is_bluetooth`, pure: the endpoint's
  `PKEY_Device_EnumeratorName` "BTHENUM" / "BTHHFENUM" / "BTHLE…", its device
  path, or a "Hands-Free" name — read with `IMMDevice::OpenPropertyStore`
  for the input cpal opened), once its stop chime has played on it. What's
  next is pure (`next(holds, open, bluetooth, warm_until, now)` → Stay /
  Open / Close / CloseAt), unit-tested; a timed close first handles anything
  asked meanwhile. An open never blocks the recording: it runs in the
  background, and the stream's callback tells the pipeline (`Sink`) its
  audio, its **first real audio** (`live_now`: a sample that isn't exactly
  0.0, or `ZEROS_LIVE_MS` 1.5 s of silence — a muted mic), that it opened,
  or that it failed. `Status` (debug-only `e2e_mic_status`): open, live,
  Bluetooth, opens/closes and the last open's timing (`OpenTiming`: build,
  first audio, live, in ms from asking) — the log says the same at `info`
  ("Microphone: live … first_audio_ms"). **Measured on this PC**
  (2026-10-22; the `--ignored` `open_latency_this_pc`, five cold opens
  each): USB mics (a Razer Seiren Mini, a USB audio adapter) build in
  4–8 ms, their first audio arrives 38–47 ms after asking and starts
  28–37 ms after it (the first callback carries 10 ms); in the app
  (`start_recording` → live) 10–23 ms. A wireless gaming headset's USB
  dongle (Logitech G435) delivers from ~50 ms but **digital silence until
  ~515–535 ms** (its radio link waking): the chime waits for it, and up to
  ~0.5 s of a word said with the key is lost on a cold start there — the
  warm window and "Keep the microphone ready" cover it. No Bluetooth headset
  was paired here; HFP's profile switch is known to take 0.5–2 s, which the
  same wait covers. Against the old always-open ring, a cold start on a
  wired mic loses what was said in the 300 ms before the press and the
  ~30 ms after it.
  **Which microphone, and a device going** (2026-10-25, `dictation/mics.rs`):
  `open` tries `mics::candidates(order, Windows' list, away)` in turn — the
  person's order (`config.mic_order`), then Windows' default; one that won't
  open gives way to the next; none at all → "No microphone found". The
  running stream knows its endpoint (`Running.endpoint`). Windows' list
  changing (`mics::on_change` → `Cmd::Devices`) and cpal's error callback
  (every error stops a WASAPI stream: `DeviceNotAvailable` when it's
  unplugged → `Cmd::Stopped { serial }`) both come to the mic's thread:
  pure `after_stop(listed, held, reopened_lately)` → **Reopen** the same
  device once (Windows still lists it: a reset, a format change in Sound
  settings) or **Lost** (`Sink::lost`; the device goes on an `away` list for
  10 s or until Windows drops it; the mic test or "Keep the microphone
  ready" opens the next one at once). A microphone that comes first now (a
  higher one plugged in, the order changed in Settings, Windows' default
  moved for someone on it) is used from the next dictation, **never in the
  middle of one** (`reconsider`): a warm stream on another closes, one the
  mic test or "Keep the microphone ready" holds moves. The Bluetooth rules
  are the stream's (never warm, closed after the stop chime), whichever
  microphone in the order it is, and a ranked headset that's connected but
  asleep holds nothing up: the recording starts at once and the chime
  waits for its first audio, as above. `Status` has `device` and `losses`.
  A test run's made-up microphones (`mics::fake`) open as a quiet tone
  (`fake::Tone`, real-time pace) that stops by itself when unplugged.
- **`dictation/mics.rs`** (+ `mics/watch.rs`, `mics/offer.rs`) — **the
  microphones' order and Windows' list** (2026-10-25; Wispr Flow's ranked
  mics, by behaviour). `MicChoice { id, name }` (`config.mic_order`,
  `mic_not_now`): kept by its Core Audio **endpoint id**, the same when the
  device is plugged in again, with its name for one picked before ids were
  kept (`migrate_once` v4) or plugged into another port (`matches`: the id,
  else the name when no device has that id). `Endpoint { id, name,
  bluetooth, default, virtual }`: Windows' active capture endpoints
  (`win::capture_endpoints`, the default first; `virtual` = enumerator
  "ROOT", a streaming app's). Pure: `candidates(order, present, away)` →
  `[Target]` (each connected one in order, then Windows' default — the next
  one if the default is away; `Target::Default` only when Windows' list
  couldn't be read, as before), `in_use`, `rank_of`, `short_name`
  ("Headset Microphone (Contoso H2)" → "Contoso H2"), `on_arrival(ep, order,
  not_now, present)` → **Ask** (new to Yap, not what Yap uses) / **Using**
  (in the order and now in use: higher than the one before) / **Quiet** (lower
  in the order, "Not now" before, virtual, or Windows' new default that Yap
  follows anyway), and `Gate` (one question per `ASK_GAP_MS`, 10 min, and
  per microphone per run). The list: `present()` (cached; read on first
  use), `refresh()` (read now, one at a time; a change goes to `changed`:
  the listeners — the dictation's mic —, `GENERATION` for a meeting's
  capture, `yap-mics-changed` for Settings, and each arrival to the offer).
  **`watch.rs`** registers an `IMMNotificationClient` (`#[implement]`,
  hence the `windows-core` dependency) on a `mic-devices` thread: its
  callbacks only nudge a channel (nothing may wait in them); a burst of
  notifications settles (400 ms), then `refresh()`. **`offer.rs`**: an
  arrival waits 800 ms (Windows may make it its default), then until no
  dictation is going on (`notice::moment`, the settle after one included;
  dropped if it's gone again or after 10 min), then **Ask** → "Contoso H2
  connected · Use it for dictation?" in one place (`notice::place`, never a
  Windows notification): the main window's toast (`yap-mic-offer` →
  `lib/notices/micOffer.js`, kind `info`) or a Yap bar card (`micOffer`),
  **Use it** (`put_first`: first in the order, off "Not now") / **Not now**
  (`add_not_now`: never asked again; Settings can still add it), answered
  through `mic_offer_respond` or the card (`shell/settings::change_config`:
  saved and applied); **Using** → "Using Contoso H2 for dictation" (an
  alert, kind `micSwitched`, the same words not twice in 10 minutes, button
  **Change** → Settings → Dictation → Microphone). Commands `list_mics`,
  `mic_offer_respond`; debug-only `e2e_mics { devices }` (made-up
  microphones instead of Windows', ids prefixed "e2e-"; the gate forgotten),
  `e2e_mic_plug { device }`, `e2e_mic_unplug { id }` (told as Windows tells
  a change), `YAP_E2E_MICS` (made-up ones from the start; `[]`: none). The
  log says counts at `info`, names only at `debug`. Unit tests
  (`mics::tests`, `offer::tests`), e2e `mics.spec.js`.
- **`dictation/mic_watch.rs`** — the dictation's dead-mic watchdog, pure: `MicTally`
  (lock-free, fed by the audio callback), `DeadMicWatch::poll(elapsed, samples,
  heard)` → Wait / Reopen / Report / Recovered / Fine, the messages (the full one
  and the overlay's short "No sound from <device> — is it muted?"). Only exact
  zeros count (a quiet room is never all zeros). Unit-tested over simulated
  callback chunks. Before it blames the mic, the pipeline asks Windows' privacy
  switches (`dictation/mic_privacy.rs`). Meetings have their own quiet-side watch (`meetings/meeting.rs`).
- **`dictation/mic_privacy.rs`** — Windows' microphone privacy switches, read only (never
  written): HKLM `…\CapabilityAccessManager\ConsentStore\microphone` `Value`
  ("Microphone access", the device), HKCU's ("Let apps access your
  microphone"), HKCU's — else HKLM's — `…\microphone\NonPackaged` ("Let desktop
  apps access your microphone"), and HKLM `…\Policies\Microsoft\Windows\AppPrivacy`
  `LetAppsAccessMicrophone` = 2 (an organisation's policy); a
  missing value is no answer. Pure `decide` names the widest switch that's off
  (policy, device, apps, desktop apps), unit-tested. Asked at a recording's
  start, in the no-mic path and before the dead-mic watchdog's report (see
  `dictation/pipeline.rs`): "Windows is blocking microphone access for desktop apps" +
  which switch to turn on + **Open settings** (`ms-settings:privacy-microphone`)
  as an alert (`notices/alert.rs`), "Windows is blocking the microphone" in the
  overlay's line; logged when capture starts. Test mode reads no registry:
  debug-only `e2e_mic_privacy { blocked }` stands in; an `#[ignore]`d
  `mic_privacy_this_machine` prints this PC's switches.
- **`dictation/mic_tips.rs`** — **mic-quality tips** (2026-10-29; Wispr Flow's
  "didn't hear you clearly" and AirPods advice, by behaviour). As a
  dictation stops (`stop_and_transcribe`; every kind: a rewrite's
  instruction and a question too) its audio is measured (`measure`, pure:
  20 ms frames' dBFS — the voice the 90th percentile, the room the 10th —
  and the share of samples at full scale) and judged (`trouble`, pure, the
  worst first): **clipped** (≥ 0.1 % at ≥ 0.98), **quiet** (the voice under
  −42 dBFS), **noisy** (the voice < 12 dB above a room louder than −50
  dBFS); under 2 s, or no voice at all (the dead-mic watchdog's), never.
  The microphone it used comes from the mic's thread before the stop chime
  lets it go (`Mic::current`: its name, endpoint, Bluetooth and **its own
  sample rate**, `Running.rate`). One tip (`pick`, pure): a Bluetooth headset
  in hands-free mode (`hands_free`: Bluetooth at 8 or 16 kHz — HFP; an LE
  Audio mic at 48 kHz isn't) with another mic connected → "Bluetooth headsets
  sound worse while their mic is on: try the PC's mic" (or the other mic's
  name; `other_mic`: not Bluetooth or a software device, the laptop's own
  first, then the order), with **Microphones** — after any dictation on
  one, its signal weak or not; clipped → "…turn its level down in Windows'
  sound settings"; quiet → "Move closer to the microphone, or turn its level
  up…", both with **Sound settings** (`alert::OPEN_SOUND_SETTINGS`,
  `ms-settings:sound`); noisy → the other mic ("Try Fabrikam USB Mic: put it
  first in Microphones") or "Move closer… or somewhere quieter". Title "Yap
  didn't hear you clearly". **Rarely** (`eligible`, pure): one mic tip a
  day, each kind at most 3 times and not twice in a week (`mic_tips.json`);
  shown once the dictation is done (`notice::moment`, within a minute), in
  one place (`notice::place`: the main window's toast, `yap-mic-tip` →
  `lib/notices/micTips.js`, kind `tip`; else a Yap bar card `micTip`,
  never a Windows notification); "Show tips" off, none; notice kind
  `micTip` is a tip held over dictations and meetings, the same words not
  twice in a day. The log: the levels and the trouble (numbers only). Test
  mode judges nothing until `e2e_mic_tips { on }` (which also forgets the
  counts and returns the last judgement); made-up Bluetooth mics open at
  16 kHz. Unit tests (`mic_tips::tests`: synthetic speech in syllables over
  a room's hiss — clear, a whisper, far, a loud room, clipped; the tips;
  the rate limits), e2e `mic-tips.spec.js`, robot `e2e-real/mic-tips.real.js`
  (Windows' voice as recorded, 32 dB quieter, under a hiss, 12 dB past full
  scale; not run yet).
- **`dictation/lid.rs`** — **the laptop's lid** (2026-10-29; Wispr Flow's
  clamshell rule, by behaviour). Windows says when the lid opens or closes
  (`PowerSettingRegisterNotification(GUID_LIDSWITCH_STATE_CHANGE)` with a
  callback, no window; its state once when Yap starts watching); a desktop
  has no lid and is never told it's closed. Closed: the laptop's own
  microphone (`mics::is_built_in`, pure: not USB, Bluetooth or a software
  device, and a microphone array by its jack subtype —
  `PKEY_AudioEndpoint_JackSubType` KSNODETYPE_MICROPHONE_ARRAY or
  PROCESSING_MICROPHONE_ARRAY — or named "Microphone Array", "Internal
  Microphone"…; `Endpoint.built_in`) goes last (`mics::lid_rule`, in
  `candidates`, so dictation, a meeting and "in use" all follow it): the
  next in the order, else Windows' default, else any other connected one
  (not a software one), the built-in one still tried last. A change is told
  as Windows' list changes are (`mics::lid_changed`: a warm stream on it
  closes, never mid-dictation). When the built-in one is all there is, a
  dictation opening it says "Your laptop's lid is closed, so its microphone
  can't hear you · Open the lid, or plug in a microphone and put it first in
  Microphones" (`alert.rs` kind `lidClosed`, **Microphones**), once per
  closing. Test mode reads no lid: debug-only `e2e_lid { closed }` (`null`:
  no lid). Unit tests (`lid::tests`, `mics::tests`: the order with the lid
  closed, the built-in mic told apart), e2e `mic-tips.spec.js`. Not tried
  on a real laptop yet (this PC is a desktop).
- **`dictation/stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **14-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  **Each engine loads on a `Device`** — the graphics card (Whisper through
  Vulkan, the ONNX models through DirectML) or the processor —
  `create_stt_engine` asking `stt_speed::device_for` (the measured pick, or
  Settings' "Runs on"), `create_stt_engine_on` for a given one (Whisper by
  `WhisperLoadParams.use_gpu`; ONNX by setting transcribe-rs's execution
  provider for that load, one ONNX load at a time); `SttAdapter::device()` /
  `model()` say which. `apply_accelerator_settings` only sets the globals'
  defaults now. `model_fingerprint` (the files' size + newest change) tells a
  re-downloaded model. **transcribe-rs is vendored**
  (`src-tauri/vendor/transcribe-rs`, through `[patch.crates-io]`; what
  changed is in its `YAP.md`): on DirectML, Parakeet's decoder/joiner and
  preprocessor load on the CPU (the TDT loop's ~200 runs per 11 s of speech
  cost ~1.4 ms each as GPU round trips, ~0.2 ms on the CPU), and the
  waveform is zero-padded to one planned length (30.25 s; the real length
  still passed, so the words don't change) because DirectML's encoder runs
  in ~55 ms only at the length its first run planned for and ~350 ms at any
  other — the model transcribes a second of silence at load to plan it. On
  this PC that took Parakeet V3 on DirectML from 441 / 630 / 1,244 ms to
  133 / 158 / 578 ms at 3 / 11 / 36 s of speech. **Parakeet's TDT decode
  reads the model's durations** (2026-10-27, every device): 0.3.11 ignored
  them and asked a frame again after every token, so on some words the model
  said one token ten times a frame for several frames ("definitive d d d d
  d d d d d d d d edition", "how how how how…"), in 4 of a day's 12 real
  dictations replayed (`yap.exe partials-dump`); with the durations, none.
  `detect_language` (default `None`; Whisper's own detection, the vendored
  transcribe-rs's `WhisperEngine::detect_language`) and
  `SttAdapter::transcribe_spoken(audio, &Spoken, …) → Said { text, heard }`
  are My languages' (`dictation/languages.rs`). `transcribe_timed` (default: `transcribe`, no times) keeps the times the
  model gives (`Timed { text, words: Vec<TimedText> }`, seconds from the
  audio's start): Parakeet's words (`TimestampGranularity::Word`), Whisper's
  segments (a phrase each), SenseVoice's tokens joined at "▁"; none for
  Moonshine/GigaAM/Canary/Cohere or the stub. Only meetings call it.
  `set_gpu_adapter` (2026-10-24): the DXGI adapter the ONNX models load on
  (lib.rs: `gpu::speech_adapter`, the card rather than a hybrid laptop's
  integrated GPU listed first); `set_cpu_threads`: a processor-thread cap
  for every model loaded (ONNX intra-op) or run (Whisper's `n_threads`)
  afterwards — `yap.exe bench`'s emulated small PCs only (both through the
  vendored transcribe-rs's new globals, its `YAP.md`).
- **`dictation/stt_speed.rs`** — **the speech engine's device on this PC** (2026-10-22):
  a one-time background benchmark per installed model and PC, **the
  graphics card unless the processor is clearly faster**. The clip
  (`assets/stt-speed-clip.wav`, ~36 s of Kokoro's af_bella — Apache-2.0 —
  reading six fictional sentences with a pause between them, made by the
  `--ignored` `make_clip` test) is cut at pauses (`cuts`) into three
  dictation lengths (`LENGTHS`: short ~3 s, typical ~11 s, long = all
  36 s) and transcribed on each device with **the call a dictation makes**
  (`Call`: the language, translate and the Whisper vocabulary prompt, from
  `Pipeline::transcribe_options`; `transcribe_timed`). Per device the load
  and the **first transcription after it** (the typical length — DirectML
  and Vulkan build their graphs here) are timed and shown but never part of
  the choice; then three warm rounds of every length, the devices taking
  turns, each run a length the engine hasn't had (300 ms of trailing
  silence plus 160 ms more a round: a real dictation never repeats a
  length, and a runtime that caches its plan per input shape would look
  faster on a repeated clip than a dictation finds it), each length's median
  kept (`Timing`). The processor isn't timed further when its first
  transcription took 5× the graphics card's second, warm one (and over 3 s),
  and Whisper on the processor isn't tried at all beside a graphics card
  whose warm transcription took ≤ 3 s (`skip_cpu_whisper`: whisper.cpp's 4
  processor threads need far longer for its 30-second window — large-v3 took
  16 minutes for 11 s of speech on this PC against Vulkan's 0.9 s warm; its
  first Vulkan run, 8.3 s with the shaders being built, would have let it
  through); the graphics card is always timed. **The pick** (`pick`,
  pure): everyday dictations decide — the short and typical lengths
  (`EVERYDAY`); the long one is measured and shown, never counted. The
  graphics card, unless the processor took at most 80 % of its time at
  **both** (`clearly_faster`, `CPU_MARGIN` 0.2) — or the graphics card
  failed or got the words wrong (the processor's are the reference, a word
  in twenty aside: `agrees`). Each device's time **for your dictations**
  (`Run::everyday_ms`, Settings' "400 ms vs 630 ms") weighs the two lengths
  by this person's latest 200 dictations (`history::recent_dictation_secs`:
  the spoken length from the words at 150 a minute; `short_share`: the
  share nearer 3 s than 11 s; half and half with none). A narrow win leaves
  the processor to the person's apps: a meeting recording or a busy app
  slows the processor path (the graphics
  card's hardly), which the benchmark can't see. **When** (`schedule`): 60 s
  after start and 15 s after a model is installed or switched to
  (`Pipeline::set_engine`), only when the stored result doesn't fit
  (`fits`: `METHOD` — 3, so results from before the everyday rule and the
  DirectML fixes measure again —, the model's files, and `pc_print` — each graphics adapter's ids,
  name and driver version, from DXGI — so a new driver measures again), and
  on demand (Settings' Measure again, `stt_speed_measure`). Never beside a
  dictation (or an Upload, or within 20 s of one), a meeting recording, a
  model download, or while the processor is over 50 % busy
  (`cpu_load`, `GetSystemTimes` over a second; `wait_for_quiet`): it waits
  for a quiet moment, and a dictation, meeting or download starting mid-way
  stops it (`Stopped::Interrupted`, retried in 2 min). Kept in
  `<data>/stt_speed.json` (with the processor's load before it); the
  numbers logged at `info` (ms per device and length, whether the words
  matched — never the text). `device_for` / `device_choice`: Settings'
  "Runs on" (`config.stt_device`: auto | gpu | cpu; the old `use_gpu` off =
  the processor) else the measured pick, else — not measured yet —
  `hardware::speech_device_before_measuring` (2026-10-24): the graphics card
  when there's one that isn't small (`gpu::is_small`), else the processor.
  Before, it was always the graphics card, and `yap.exe bench` showed what
  that cost a PC without one: Parakeet on DirectML took ~4 s a dictation on
  an integrated GPU (the encoder's fixed 30-s plan, below, on a 2-CU Radeon)
  and ~1–2 s on Windows' software renderer (no graphics driver), against
  ~0.4 s for 11 s of speech on 4 processor threads
  ([docs/benchmarks.md](../benchmarks.md)). Moonshine Base starts on the
  processor whatever the card (`stt::wrong_on_directml`: on DirectML it got
  every word wrong, on an RTX and on a Radeon);
  `has_gpu` follows a test run's stand-in card (`YAP_E2E_GPU`);
  `reload_if_needed` moves the warm engine to the device it should be on at
  the next quiet moment (`Pipeline::replace_engine`). Settings → Speech-to-
  Text → Speed (`stt_speed_status`, `yap-stt-speed`). Test runs: only on
  demand, on the stub engine, `YAP_E2E_STT_SPEED` ("gpu=450,cpu=150")
  standing in for each device's cost per run. An `--ignored`
  `bench_this_pc` (release build) prints Yap's own benchmark and pick, then
  per device the load and first dictation, and per length the warm median at
  a new length and at the same length again (the numbers in
  [status.md](./status.md)); `profile_parakeet` times Parakeet's stages
  (preprocessor, encoder, decoder loop) on each device with ort directly,
  and with `YAP_PROFILE_SHAPES` / `_BUCKETS` / `_SAME` shows how DirectML's
  encoder takes lengths, fixed lengths, and the old path's words against the
  new one's. `warm_points(model, device)`: the stored warm medians per
  length, what "Taking longer than usual…" (`slow.rs`) calls usual.
- **`dictation/slow.rs`** (+ `dictation/pipeline/slow.rs`) — **"Taking
  longer than usual…"** (2026-10-25, Wispr Flow's by behaviour). When a
  dictation's transcription or its AI cleanup runs well past what's usual
  on this PC, the bar's capsule says so quietly instead of "Transcribing…"
  (`yap-slow {stage, skip}` + `yap-state` "processing-slow"; back to
  "processing" and `yap-slow null` when the stage ends — `SlowWatch`, both
  under one lock so "slow" never lands after "done"), and while it's the
  **AI cleanup** — the on-device model or a cloud one, whichever is set up
  — it offers **Skip cleanup** (`bar_action "skip-cleanup"` →
  `Pipeline::skip_cleanup` → a `tokio::sync::Notify` the watched cleanup
  races in `Shared::watched_cleanup`: the request is dropped and the words
  as transcribed go on to the corrections and the paste, as a failed
  cleanup's would). Run again's cleanup isn't watched (`clean_up`'s
  `watch`). **Usual** (pure, unit-tested): a transcription's is a straight
  line (`fit`: a fixed cost plus a cost per second of audio, least squares,
  never sloping down) through `stt_speed`'s measured warm timings for the
  model and device the warm engine is on (`stt_speed::warm_points`),
  scaled by this session's own dictations against it (the median ratio,
  0.5–3×), or — not measured yet — the line through those dictations; a
  cleanup's, the line through this session's cleanups by the same model
  (`cleanup_key`: on this PC — `ai_model::runs_here` — or the cloud, the
  provider and model) by the transcript's length. **Well past**
  (`slow_after_ms`): 2.5× the usual + 1 s, never before 4 s; nothing known:
  4 s + half the audio's length (a transcription), 4 s + 6 ms a character
  (a cleanup). The last 20 of each are kept in memory (`record_stt` with
  the transcribe call's own time; `record_cleanup` for a cleanup that
  finished, not one skipped or failed). It replaced a fixed 8 s
  "Transcribing (CPU — slow)…". The log says the stage, the times and
  lengths, never the words. Test mode: `e2e_dictation_timing { floorMs,
  sttMs, forget }` (a lower floor, the stub's transcription slowed).
- **`dictation/cap.rs`** — **a dictation's length limit** (2026-10-25; Wispr
  warns at 19 minutes and stops at 20 with "Recover text"; Yap at 14 and
  15). The buffer never grows past `LIMIT_MS` (`take_audio` takes only what
  fits); `crossed(cap, before, after)` (pure) tells when the mic's latest
  audio crossed the warning or the limit, once each. At `WARN_MS` the bar
  gets `yap-dictation-cap {leftMs, text}` ("1 minute left · it'll stop at
  15 minutes", `warning`): the capsule shows the words for a few seconds
  (at most half the time left), then an amber hairline runs down the time
  left. At the limit the mic's thread hands off to `stop_at_cap` (its own
  thread, under the gesture lock as the pause watch's stop): `capped` is
  set and the recording stops as the hotkey's second press would — chime,
  transcription, cleanup, the paste **back into its own window or onto the
  clipboard** (`inject_text`'s `strict`, as Undo's: nobody pressed
  anything, so never into whatever is in front now; Undo's own "window
  gone" alert gives way). `deliver_text` then calls `stopped`: an alert
  ("Dictation stopped at 15 minutes", kind `dictationCapped`: never held,
  never dropped) saying it's pasted, or that it couldn't be, with
  **Recover text** (`RECOVER` → `recover`: that dictation's text onto the
  clipboard again, privately — out of Win+V history — and a quiet
  "Copied your dictation"). History keeps it with its audio (Run again).
  `capped` is cleared at every recording's start and when its `run_stt`
  ends, however it ends (`ClearCapped`). Unit tests (`cap::tests`: the
  warning and the stop each crossed once, the words); e2e
  `dictation-polish.spec.js` and robot `dictation-extras.real.js` with a
  shortened limit (debug-only `e2e_dictation_cap { limitMs, warnMs,
  meetingHintMs }` — the last the length that suggests meeting notes,
  `MEETING_HINT_MS`, 10 minutes). The warning also asks the input hook to
  check for a stuck key (`input_hook::sweep_now`).
- **`dictation/meter.rs`** — **the level meter that moves for whispers**
  (2026-10-25): what `yap-amp` carries since (the bar's waveform,
  onboarding's mic check), pure, unit-tested. The pipeline's `AmpMeter`
  takes each ~80 ms's peak and `Floor::level` puts it on a perceptual
  scale: its dBFS between a **floor that follows the mic's noise** (the
  quietest recent bars + 6 dB, kept within -60…-48 dBFS; it falls to a
  quieter bar at once and rises ~3 dB a second, so only a steady hiss or
  hum lifts it, never the dips between syllables, and a whisper always
  stays above it) and 0 dBFS, gently compressed (`0.92 · t^0.75`). A
  whisper (-45 dBFS) now draws about a third of the height (the old linear
  gain ×3.5, ^0.7 gave it 6 %, under the bars' 7 % minimum), speech
  (-20…-10) two thirds to four fifths, full scale 0.92 — a shout never
  flattens against the top. The page draws the level as it comes
  (`dictation.svelte.js`); onboarding counts > 0.3 as hearing you. Test
  mode: `e2e_dictate_audio`'s `amplitude` (the tone then comes in
  syllables: 240 ms of it, 160 ms at a tenth).
- **`dictation/recent_audio.rs`** — **never lose a dictation** (2026-10-22, Wispr
  Flow's of Aug 2026): each dictation's audio (16 kHz mono 16-bit WAV) in
  `<data>/recent-audio/<id>.wav`, the id its time in unix ms, written on its
  own thread as `run_stt` starts (`keep`; not for edit mode or the Voice
  Assistant; only with `keep_recent_audio` and history on), with a
  `<id>.pending` marker until its history entry is written (`finished`) — so
  at the next start (`init` → `recover`) one Yap never finished is listed as
  failed ("Yap closed before it was transcribed"). A Command or an empty
  transcript drops it (`discard`). Kept 24 h, ≤ 500 files and ≤ 200 MB, the
  oldest first (`to_drop`, pure; `prune` at start, after each dictation and
  hourly); deleting a history entry deletes its audio, clearing history
  clears it all, and Settings → Privacy & data has the switch and Clear
  (`recent_audio_status`, `recent_audio_clear`). **A cancelled dictation**
  (2026-10-25) keeps its audio too and is listed as "Cancelled" (above,
  `dictation/pipeline.rs`); **Keep** (`set_keep`, `recent_audio_keep`:
  moved into `recent-audio/kept/`, which no sweep reads — so an older Yap
  sharing the data folder, the installed nightly beside a dev build, never
  deletes it either) holds any dictation's audio past its day and outside
  the caps until its entry is deleted or the audio cleared (un-keeping
  moves it back, its day counted from when it was said); **Save audio…** (`recent_audio_save`: Windows' Save dialog,
  "Yap dictation 2026-10-25 14.30.wav"; a test run names the path,
  debug-only `e2e_audio_save_path`) writes its WAV where the person picks.
  **Run again**
  (`history_rerun {ts, audio, model?}` → `pipeline::Rerun`, under the
  `processing` guard: the warm engine, or another installed model loaded for
  it; AI cleanup as for its app, the corrections, snippets; nothing pasted)
  gives the entry its new text and clears its failure; **Play**
  (`recent_audio_wav`: the WAV's bytes). Logs say ids and counts, never the
  folder's path beside text. Nothing here leaves the PC.
- **`dictation/media.rs`** — audio-file decode front-end for Upload: pure-Rust **Symphonia**
  (mp3/wav/m4a/aac/flac/ogg-vorbis; no opus yet) → downmix mono → 16 kHz
  (`pipeline::resample_linear`), plus `chunk_ranges` (~60 s windows cut at the
  quietest sample of each window's last 5 s). Consumed by
  `pipeline::run_file_transcription` (progress events, cancel flag, `processing`
  guard, the correction dictionary over the whole joined text via
  `pipeline::apply_corrections` — so a mis-hearing split across two chunks is
  still caught — and a history record with the raw and corrected text) via the
  `transcribe_file` command.
- **`dictation/sound.rs`** — start/stop chimes (volume + output-device aware); each
  takes an optional `Played` callback, run on the chime's thread once it has
  played (or at once when it can't): how `dictation/mute.rs` goes after the start
  chime and comes back after the stop chime. `open_output` (the configured
  output device, else the default) is shared with `voice/tts.rs`.
- **`dictation/mute.rs`** — **other apps' sound while you dictate** (2026-10-17; it
  replaced the old endpoint mute, which silenced Yap's own start chime and
  any call): **"Silence other apps while you dictate"**
  (`config.quiet_other_apps`, off; old configs' `muteWhileRecording` is its
  serde alias) and **"Pause music and videos while you dictate"**
  (`config.pause_media`, off, as Wispr's; `dictation/media_pause.rs`). The pipeline
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
  (`codec_wait`; no wait when it was already in one at the start — "Keep
  the microphone ready" on a headset, a call — unless the dictation's own
  Bluetooth mic was just closed, `after_stop_chime_mic`: the mic opens on
  demand, so the headset was in its call profile before the start chime
  and leaves it now). Crash safety: Windows keeps an
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
- **`dictation/media_pause.rs`** — "Pause music and videos while you dictate":
  WinRT `GlobalSystemMediaTransportControlsSessionManager` (windows crate
  feature `Media_Control`), sessions **Playing** → explicit `TryPauseAsync`
  (never the play/pause toggle), kept; afterwards explicit `TryPlayAsync`
  only for those still **Paused**. Never a call app's session (its app id
  against the call apps' exes and package names), no browser's while a
  meeting records or is live in one (`pause_verdict`, pure). Debug-only
  `list` for the robot.
- **`dictation/history.rs`** — local-only transcription history (`history.json`): each
  dictation's timestamp, raw + final text, its words as said when AI
  cleanup changed them (`said`: corrections, voice command and snippets,
  no cleanup, fitted as the text was — `record_dictation`, which returns
  the entry's `ts`; Run again sets it afresh; `find(ts, text)` is Home's
  lookup), model, and focused app — and its
  audio's id (`audio`, `dictation/recent_audio.rs`; listed only while the file's kept)
  and, for one that went wrong, `status` (`failed`: never transcribed, empty
  text, kept in time order — `record_failed`; `notPasted`: its paste didn't
  land — `record_with`; `cancelled`: the person cancelled it, kept with its
  audio — `record_cancelled`, taken back out by Undo, `forget_audio`) and
  `error`; `rerun_result` (Run again, Home's Transcribe) fixes it up. The
  list says `keep` for one whose audio is kept past its day.
  Failed entries don't count in the stats nor as the last dictation to paste;
  unit tests keep it in memory. Best-effort,
  gated by `history_enabled`. Derives the stats dashboard (words, time-saved vs
  typing, day streak, 30-day activity) without a date crate (UTC day-numbers like
  `ai/usage.rs`). Powers `get_history`/`clear_history`/`get_stats`. Every change
  (a dictation or upload saved, an entry deleted from Home or the local API,
  all cleared) emits **`yap-history-changed`** after the write, which Home and
  Insights re-pull on. Not `yap-transcript`: a dictation emits that before it
  pastes and saves, so a re-pull on it could miss the new entry.
