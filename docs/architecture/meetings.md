# Meetings

The meeting recorder and its audio kept on disk, rolling digests and the action plan, the end of a recording, the notepad's AI helpers, and the meeting notepad window, plus the Meetings view. The full behaviour guide is [`docs/meetings.md`](../meetings.md); telling voices apart is [speakers.md](./speakers.md); call detection and guard rails are [call-detection-and-guards.md](./call-detection-and-guards.md); the calendar is [calendar.md](./calendar.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`meetings/meeting.rs`** — the meeting recorder (OpenWhispr `meetingRecordingStore`
  port, fully offline): mic ("You") + **WASAPI loopback** ("Them" — cpal input
  stream on the default output device) on a dedicated capture thread; a worker
  cuts each source into ~15 s chunks **at the quietest 20 ms frame of the last
  4 s** (`media::quietest_frame`, not mid-word; every mid-meeting cut leaves
  the newest 400 ms buffered), transcribes them on the shared
  warm engine (`pipeline::EngineSlot` = `EngineCell`: taken out for one
  chunk and given back — a dictation that ends meanwhile waits for it
  instead of loading a second copy, and an engine that errs is dropped so
  the next chunk loads a fresh one; silence-gated), runs the **correction
  dictionary** over each chunk's
  text (`pipeline::apply_corrections`, both sides, so the echo check compares
  corrected with corrected), works off a backlog in ≤30 s chunks back to back,
  caps each source's buffer at 20 min (then drops audio + a "Transcription
  is falling behind" notice),
  dates each segment by its speech onset, places it on the recording's clock
  (`start_ms`/`end_ms`) with its words' times (`stt::transcribe_timed` →
  `speakers::word_times`, lined up with the corrected text; `Heard::line`),
  flags **speaker echo** (a "you"
  chunk whose words repeat "them" in order AND whose 20 ms loudness envelope
  follows the call 0–600 ms later — the timing test keeps a headphone user's
  own reply; flagged segments are kept, hidden, left out of summaries), and
  `ingest`s segments: persist to `notes.transcript`, emit `yap-meeting-segment`,
  kick the rolling digests. Each source is a `Captured` (`AudioBuf`) on a
  **session clock** (positions in samples); `push_audio` is the one push path
  (device callbacks and the e2e player). **Dictation kept out of "You"**: the
  mic's `DictationGate` reads `pipeline::dictation_state()` per callback
  (lock-free) and, while a hotkey dictation records, appends silence instead of
  the audio (same length, so times and the echo check stay aligned); a new
  dictation also blanks the newest ~340 ms already buffered (its pre-roll +
  40 ms), the callback that sees it end is blanked too, and each blanked span
  leaves a `dictated` marker segment (dated where it began; one per span,
  however the chunks are cut). **One clock**: `keep_in_step` (every 200 ms)
  fills a side whose device went silent while the other moved on ≥ 0.5 s
  (loopback with nothing playing, a device switch) with silence up to the
  other. **Follows the default output**: the capture thread reads Windows'
  default render endpoint id every 2 s (`IMMDeviceEnumerator`, eConsole like
  cpal; `OutputFollower` decides) and reopens the loopback stream on a new one
  (old stream first, a failure keeps the old one and retries; logged).
  **Follows the microphone** (2026-10-25, "Following the microphone"):
  "You" opens the first microphone in the person's order that's connected,
  then Windows' default (`open_mic` over `mics::candidates`, as dictation;
  a meeting with none fails with `no_mic_error`, which names the first in
  the order). `MicFollower` on the capture thread looks again when its
  stream stops by itself (cpal's error callback sets `stopped`) or Windows'
  list moves (`mics::generation`): pure `mic_move(open, stopped, listed)` →
  **Stay**, **Reopen** (the same device, still listed: a reset; a failed
  reopen is a Next), **Next** (its device is gone: the next one, the gap
  `keep_in_step`'s silence, a marker at that place —
  `Captured::mic_changed`, `TranscriptSegment::mic_marker`: `source` "you",
  `mic` the new one's name or "" with none left, no text, taken with the
  chunks that pass it, `take_switches` — and "Your microphone was
  disconnected · Yap is recording you with Northwind Audio now" through
  `warn`, button Update settings → Dictation → Microphone) or **Retry**
  (none was left: the order again with every change). A microphone that
  merely arrives mid-meeting is left for dictation; the meeting keeps its
  own. The marker isn't talk (`is_talk`), so digests, the action plan and
  the Ask context skip it; Transcribe again keeps it where it was
  (`redo.rs`); the views draw "Your mic was disconnected · now Northwind
  Audio" (`notepadText.js` `transcriptItems` kind `mic`). The test player
  follows made-up microphones the same way (`e2e_mic_unplug`: "You" gets
  nothing for a second, then the marker). Unit tests (`mic_move`,
  `a_mic_change_is_marked…`), e2e `meeting-mic.spec.js`.
  **Quiet sides**: `QuietWatch` (every 200 ms) — one side heard nothing above
  the silence gate for 3 min of meeting while the other talked ≥ 10 s →
  "Yap can't hear the call" / "…your mic" once per quiet stretch ("Yap
  hasn't heard the call/your Teams call for 3 minutes. If it plays through a
  headset, make that Windows' default output device." / "…your mic…Settings
  → General"), in one place (`warn` → `meeting_guard::notice`: the main
  window while it's focused, else the Yap bar, else a Windows notification;
  "Transcription is falling behind" the same way); a dictation counts as the
  mic heard, not as talk. `activity()` (the recording's note, its audio
  clock in ms — captured audio, standing still while none arrives — and a
  talk counter: both sides' talk samples plus talk segments ingested) is
  what `meeting_guard`'s quiet-meeting watch reads each second; the
  `Session` keeps both sources for it. **Failed chunks** (2026-10-24, code
  review #6): a chunk that can't be transcribed (no speech model, an engine
  error, a crash) was recorded as silence; now its stretch is a marker
  (`TranscriptSegment::failed`, `start_ms`..`end_ms` on the recording's
  clock, no text, not talk: summaries, the AI title and "Started by
  mistake?" skip it), logged at `warn` (side, seconds), and the person is
  told once per meeting (`tell_failed`: "Yap couldn't transcribe part of
  this meeting" through `meeting_guard::notice`, with Open note; its body
  says whether the refine will try again); `replay` marks one that fails
  again the same way. Test mode: `e2e_meeting_fail {side, from, to}` fails
  the live chunks over a stretch (never a replay's). **One recorder**
  (#10): `start` claims the recorder's slot (`Slot::Starting`, `Claim`)
  under its lock before it opens the devices and lets go if they fail —
  until 2026-10-24 two starts at once (the meeting shortcut and the call
  prompt) both passed the "already recording?" check and recorded twice;
  `is_recording` counts a start under way. **Deleting a note**
  (`delete_note`, every delete that can meet a recording: the Notes view,
  the local API — `409` —, Discard) refuses the note being recorded or
  started, under the same lock. Commands: `meeting_start`/
  `meeting_state`; a recording ends through `meeting_end`/`meeting_pause`
  (meetings/meeting_end.rs). Test mode opens no audio device (WAVs
  from `YAP_E2E_MEETING_AUDIO`, or silence; `e2e::e2e_meeting_feed`; the
  player also stages a dictation and an output switch, and follows made-up
  microphones unplugged, see `shell/e2e.rs`). A
  meeting started right after Yap did waits for the speech model, still
  loading in the background (`pipeline::wait_for_model`), never loading a
  second copy. **Memory after a meeting** (measured 2026-10-22, the e2e
  meeting audio — 10 minutes a side at 4× — through Parakeet V3 on DirectML,
  `yap.exe`'s private working set): nothing outlives the recording — 259 MB
  before, 333 MB at its end (77 MB of it the test player's two WAVs), 256 MB
  a minute after. The ~170 MB the polish sweep saw kept after a meeting
  (reproduced on the build before the DirectML fix, 4b822b5: 147 MB before →
  318 MB after, +930 MB committed) was ONNX Runtime planning DirectML's
  encoder for every new chunk length and keeping each plan for the engine's
  life; since the encoder's input is padded to the one length it plans at
  load (vendored transcribe-rs, `YAP.md`) a meeting adds nothing, and that
  plan's memory is part of the loaded model's (~258 MB idle with it).
  **Speaker diarization's tee**: with "Tell speakers apart" on and its model
  installed, `start` opens a `diarize::Session` before the capture and gives
  the "Them" `Captured` its `Tee` — every sample appended (device audio,
  `fill_if_stalled`'s silence) also goes to the engine, so its clock is the
  session clock; the worker calls `Session::finish` after its last chunk and
  before the final `yap-meeting-state` (so the summary starts after the final
  labels). Off, there is no tee. **The labels on lines**
  (`speakers::labels`): `start` registers the recording once its capture
  runs (`labels::begin(note, Session::id)` — its lines are those from the
  transcript's length then), `ingest` lets lines the latest labels cover get
  theirs at once (`after_ingest`), and the worker, after `finish` (or, in a
  test run with no engine, `diarize::finish_scripted`), calls `labels::end`
  (an engine that never sent final labels: labelled as far as it went).
  **The audio on disk** (`meetings/meeting_audio.rs`, below): `start` makes a
  `meeting_audio::Recorder` before the capture, which turns on each side's
  `disk` queue (`keep_on_disk`): the push path copies every sample it
  appends — a dictation already blanked (and its pre-roll blanked in the
  queue too, `disk_spans` noting where), `fill_if_stalled`'s silence — so
  the files are the session clock sample for sample; `take_for_disk` holds
  the mic's newest `DICTATION_LOOKBACK` back until the end. The worker
  reports how far each side is in the note after every batch
  (`Recorder::progress`, the chunk's `to`) and `finish`es it after the
  final labels, before the last `yap-meeting-state`. **`replay`** — the
  worker's own steps over audio read back from disk (`Feed`): the same cuts
  at quiet moments (topped up to 16 s, final drain at the end), the same
  engine, correction dictionary (the note's attendees in the prompt:
  `transcribe_chunk` takes the note) and echo check (`Source::replayed_from`
  numbers its loudness frames as live), markers for dictations from where
  they began; it waits for a hotkey dictation (and 3 s after one) between
  chunks so the dictation's own transcription gets the warm engine; its
  `Ctl` carries a job's Cancel (checked between chunks, and while it waits)
  and `background` (the refine's: each call on a lowest-priority thread,
  `meeting_refine::LowPriority`). `NoFeed`: a side with nothing to read
  (Try again on the other side's stretch).
- **`meetings/meeting_audio.rs`** (+ `meetings/meeting_audio/flac.rs`, `meetings/meeting_audio/redo.rs`)
  — **meeting audio kept on disk** (2026-10-23; Wispr Flow keeps 14 days to
  play, Meetily transcribes again): `<data>/meeting-audio/<note
  id>/<recording>-you.flac` / `-them.flac` + `<recording>.json` (`Meta`: its
  clock's start, `endedMs` — `None` while it records and after a crash —,
  `keep`, `you/themDone` how far the note has it, `you/themLen`, `dictated`
  where dictations began). **Format**: FLAC, 16 kHz mono 16-bit, written by
  `flac.rs`'s own encoder (fixed 4,096-sample blocks, FLAC's fixed predictors
  0–4 + partitioned Rice coding, no LPC, ~300 lines, no new crate; Symphonia,
  already in Yap for Upload, decodes and seeks it — a binary search for frame
  headers, no seek table). Lossless, so a later model hears what the live
  one did; measured (`cargo test -- --ignored flac_sizes`): digital silence
  0.2 MB/h, a −60 dBFS room 47 MB/h, Windows TTS speech 60 MB/h (52 % of
  WAV's 115), speech over that room 70 MB/h (61 %) — a two-hour meeting
  ~0.2–0.25 GB for both sides instead of 0.46. Each frame (256 ms) is
  appended as it fills: a crash leaves a file that decodes to its last
  whole frame (a write torn inside the next frame's header costs the frame
  before it too); the header says "length unknown" until `finish` writes
  it, and a crash's is filled in when it's found. **`Recorder`** (made by
  `meeting::start`): both files opened write-only-to-us (`FILE_SHARE_READ`:
  other programs read, never write or delete — which is also how a second
  Yap on the same data tells a live recording from a crash's, `in_use`), a
  writer thread emptying the queues every 500 ms and syncing every 5 s,
  `finish` closing them (lengths into the header and the meta), marking it
  ended and deleting it at once when "Keep meeting audio" was off;
  `discard` when the capture never started; dropped unfinished = left for a
  recovery. **Retention** (`prune`, at start and hourly; `to_drop` pure):
  older than `meeting_audio_days` (14) by its start, a deleted note's
  (`notes::delete` → `forget`; none while the notes store is empty), then
  the oldest past `MAX_BYTES` (10 GB) — never a recording being written;
  half-written `.tmp` leftovers. **After a crash** (`init` → `crashed`):
  recordings not ended, not in use, their note there, ≥ 1 s never in the
  note (lengths counted from `done` to the end) are `pending`; one with
  nothing left is closed quietly. `offer` puts "Yap closed during a meeting"
  in one place (`alert::show`, kind `meetingRecovery`, held while dictating
  or recording: the window's toast, the bar, a notification) with
  **Transcribe it** (`RECOVER_ACTION` → `recover_all`); the note says it
  too (`meeting_audio_note`'s `recoverableSecs`). **Jobs** (`start_job`, one
  per note, refused while a meeting records — `replay` also waits for one that starts, and for a dictation —; `yap-meeting-redo {noteId, kind:
  recover|again|refine|retry, state: running|done|error|cancelled,
  percent, model, error, summary, waiting, thenSummary}`; every job has a
  **Cancel**, `meeting_job_cancel`, between chunks): **recover** =
  `meeting::replay` from `done` to the end, `notes::insert_transcript` (by
  time: the end normally, before a later recording's lines with a stop mark
  after them), the recording ended, then `meeting_end::recovered` (the stop
  mark, "Started by mistake?" or the AI title and the action plan, as a
  stop); **again** = every recording replayed from 0 on the speech model in
  use now, then `redo::rebuild` under `notes::modify`: each recording's
  lines (dated within it) replaced, lines with no audio and every
  dictation marker kept, **every line the person edited kept as written**
  (a new line of its side over one keeps only its words outside it — by its
  word times, else by its share of its place and none when that's under
  half —; `notes/note_edits.rs`; whatever calls `rebuild` inherits it; the
  rebuilt lines sorted by date, then place), a new line taking the voice and lock of the old
  line of its side it overlaps by at least half the shorter (split by
  `speakers::split_line` where two old voices meet), the stop marks moved
  to the same places, the digests dropped (written again from the new
  lines by the next summary), My thoughts, the speaker map and the summary
  untouched (the windows say the summary is the earlier one). **Play a
  line** (`clip`): both sides of `[start − 250 ms, end + 250 ms]` on its
  recording's clock (the recording the line's `ts` falls in), mixed, as a
  WAV (`recent_audio::wav`), ≤ 60 s. Commands `meeting_audio_status` /
  `meeting_audio_clear` (Settings), `meeting_audio_note`,
  `meeting_audio_clip`, `meeting_transcribe_again`, `meeting_recover`,
  `meeting_retry_chunk {noteId, source, ts, startMs}` (**Try again** on a
  failed chunk's marker: that side's stretch read back — `RangeFeed` — and
  replayed on the model in use, its lines in the marker's place,
  `redo::fill`: digests and stop marks after it move along), and
  `meeting_job_cancel`; **refine** (`start_refine`, from
  `meetings/meeting_refine.rs`): published at once, it waits its turn (the note's
  summary job first unless it comes first, `waiting: "summary"`; never
  beside a recording, `"meeting"`), then **the same `transcribe_again`**
  on the chosen model — the warm slot when it's the model in use, else an
  `EngineCell` of its own loaded once —, `meeting_refine::agrees` checking
  the new words against the live ones before anything is replaced, and
  the replacement waiting while the note's summary is written. Done with
  a summary on the note: `summary: true`, the windows' "Update summary",
  answered when the summary is written again (`summary_redone`, from
  `meeting_end::start_summary`). Audio not kept ("Keep meeting audio" off)
  is **held** after a meeting with failed chunks or a refine due
  (`Recorder::finish(hold)`) and goes once it's done (`release`, when no
  failed stretch is left), a day at most (`to_drop`'s `held`, `HELD_MS`);
  each recording's `.json` names its live model (`Meta::model`);
  debug-only `e2e_meeting_recovery_offer`; events `yap-meeting-audio
  {noteId}` and `yap-meeting-redo`. Logs: ids, counts, lengths only.
  Unit-tested (the encoder against Symphonia: every block kind, cut-off
  files, seeking; retention, metas, a crash's recording found and clipped;
  `rebuild`, a failed chunk's marker replaced by a refine, `fill`; audio
  only held, a day; the disk queue against the buffer with a dictation; inserting
  recovered lines); e2e `meeting-audio-kept.spec.js` and
  `meeting-crash.spec.js` (a killed test instance, restarted).
- **`meetings/meeting_refine.rs`** — **the refined transcript, by itself** when a
  meeting ends (2026-10-24, Wispr Flow's "refined transcript"): the meeting
  transcribed again in the background from its audio on disk, through the
  same replacement as Transcribe again. **When** (`decide`, pure; Settings →
  Meetings → "Improve the transcript after the meeting", `meeting_refine`):
  *Automatically when it helps* (default) — chunks of the live transcript
  failed, or a more accurate model is installed for the meeting's language
  —, *Always* (the best model there is, the live one when it's the best),
  *Never*; never without its audio, on a pause, or for "Started by
  mistake?". **Which model** (`best_model`, pure): the most accurate
  installed model (`MODELS`: `src/lib/core/speechModels.js`'s leaderboard figures and
  each model's languages) that speaks the meeting's language — Settings'
  speech language, else Windows' (`about_you`); unknown: one that speaks
  every language the live one does — and beats the live one (`Meta::model`,
  the least accurate recording's) by `MARGIN` (0.015). `agrees`: at least
  half the live words found again, else the live transcript is kept ("didn't
  match the meeting (another language?)"). **Which comes first**
  (`Plan::first`): holes in the live transcript (chunks failed) → the
  refine first, then the end steps (`meeting_end::end_steps`: the summary,
  or "Started by mistake?") from the whole transcript; a whole transcript →
  the summary first (instant: its digests were made during the meeting),
  then the refine, then **"Transcript improved · Update summary"**
  (`meeting_summarise`: what the summary sends today, cloud or on-device;
  not automatic — a summary read or edited doesn't change under the person,
  and a long meeting's digests would all be written again). **Low priority**:
  after the summary, never beside a recording, a dictation always first
  (`replay`'s `give_way`: 3 s after one), sharing the warm engine a chunk at
  a time (`EngineCell`) or on its own copy of another model, each call on a
  lowest-priority thread (`LowPriority`; the runtimes' worker threads keep
  theirs); Cancel. `may_refine`: whether a meeting ending now may be refined
  (its audio held). Test mode: `YAP_E2E_REFINE_CHUNK_MS` slows each refine
  chunk (`e2e_slow_chunk`). Logs: ids, the plan, counts, the agreement share
  — never a line. Unit-tested (`decide`, `best_model`, `agrees`).
- **`meetings/meeting_summary.rs`** — meeting summaries that keep up with long meetings
  (map-reduce, the map done while recording). A bigger on-device context
  lets the final call read more (`FinalBudget::for_endpoint`: the single
  pass and the final input × `local_llm::context::budget_factor_for`, 1–4 — 14,000 /
  18,000 at 32K — only on the running sidecar; `prepare_final` and
  `compose_meeting_input_within` take it; the live digests and the helpers
  keep the 8K budgets). Every ~10 min of new talk
  (2,000 est. tokens, or 10 min with ≥ 250) a background **digest** (key
  points, decisions, action items `Owner: task (due: …)`, open questions;
  `llm::MEETING_DIGEST_PROMPT` + a one-shot; ≤ 2,600 tokens of transcript + the
  meeting so far ≤ 400) is written with the Note Formatting endpoint and
  stored on the note (`yap-meeting-digest`); owners must be You/Everyone/an
  attendee/a name in the speaker map/a name said in that part (else
  Unassigned; `speakers::known_people`) and deadlines must have
  been said (`check_digest`). **Speakers**: every input labels lines with the
  speaker map's names (`transcript_lines(&note.speakers, …)` →
  `speakers::prompt_label`: "You", "Them", "Priya", "Speaker 2") and, when
  anyone is named or voices are told apart, adds "Speakers besides You: …"
  under "Attendees" (`people_lines`; a calendar guess is said to be one);
  plain You/Them meetings read exactly as before. On a **local** endpoint digests wait for
  dictations to finish and are dropped mid-call when one starts
  (`unless_busy` — closing the connection cancels it in llama.cpp), and
  replies are capped. `prepare_final` (one digest turn at a time,
  `DIGEST_TURN`) digests any backlog with `yap-meeting-summary-progress`;
  `compose_meeting_input` keeps the final input ≤ 4,500 tokens whatever the
  length (raw transcript when ≤ 3,500); `postcheck_action_plan` moves invented
  owners' tasks to Unassigned, strips made-up deadlines and restores dropped
  digest tasks; `ask_context` bounds a long meeting for the Ask bar and the
  chat `get_note` tool. Every input reads talk only (`TranscriptSegment::
  is_talk`: no echo, no "You dictated here" markers; markers weigh 0 tokens in
  the digest windows). The notepad's helpers live here too: `catch_up_input`
  ("What did I miss?": what was said since segment `since`, raw when ≤ 2,400
  tokens, else the digests covering it + the latest ≤ 1,400; context ≤ ~700),
  `meeting_ask_messages` (a follow-up question, ≤ ~3.3k), `title_messages` +
  `clean_title` (the AI meeting title: the start of the meeting, one-shot,
  ≤ 60 chars), the one-tap helpers' inputs — `questions_input` (the latest
  ≤ 1,200 tokens of talk + digests of what came before ≤ 500), `recap_input`
  (the action plan ≤ 1,200 + `ask_context` of the meeting: ≤ 1,800 with no
  plan, else 500 for the email / 800 for the gaps), both with attendees, the
  invite ≤ 200 and typed notes ≤ 300, `None` (too little: < 20 words and
  nothing else) — `helper_messages`, `clean_email` (the email as plain text:
  preamble, fences and markdown out, "Subject: " first, `email_subject` when
  missing) — every helper call ≤ ~3.4k with its reply; and `chat_beside_dictation` (on a local endpoint: wait for
  dictation to finish, drop the call when one starts and ask again — never
  slows a dictation). See [`docs/meetings.md`](../meetings.md).
- **`meetings/meeting_end.rs`** — the end of a meeting recording, wherever it was
  stopped from (notepad Stop, Notes view "End meeting & summarise", the Yap
  bar, call detection, an automatic stop). It follows the recorder through
  `yap-meeting-state` (Rust listener; `meetings/meeting.rs` is untouched; the final
  one comes after the speaker engine's last labels are on the lines, so the
  title and the action plan read the voices): a start
  opens the notepad (`notepad::on_meeting_started`); every stop (pause or end)
  marks its place (`notes::add_break` → `yap-meeting-breaks {noteId,
  breaks}`, the notepad's ⏸ divider); a stop is the **end** of
  the meeting unless it came through `meeting_pause` (the Notes view's Pause).
  The end: fewer than 20 words of speech (`MISTAKE_WORDS`) and nothing typed →
  `yap-meeting-ended {noteId, mistake, words, surface}` and no summary (the
  window Rust picks — where the stop came from (`meeting_end`'s `origin`) if
  on screen, else the notepad, else the main window — shows **"Started by
  mistake?"** Keep / Discard; with no Yap window on screen, a **Yap bar card**
  asks instead (`meeting-mistake`; Keep, ✕ or a minute = keep; withdrawn on
  resume or delete; the bar off: nobody asks); `meeting_discard` deletes the
  note, emits `yap-note-deleted`, hides the notepad; `meeting_delete` — the
  notepad's ⋯ → Delete — pauses a recording note first, then the same);
  otherwise the AI title (if due) and
  the **action plan job** (`start_summary`: one per note, `yap-meeting-summary
  {noteId, run, state: running|done|error|needsAi|nothing|kept, step, steps, error}`,
  "Step 2 of 3"; `meeting_summary_status`; `meeting_summarise(noteId,
  replace)` runs it again — Generate summary, Retry, Update, the Notes view's
  Action plan; `kept`: the person edited the summary and nobody said
  `replace`, so no model call and nothing written: `notes/note_edits.rs`). Commands
  `meeting_end(origin)`, `meeting_pause`, `meeting_summarise`,
  `meeting_summary_status`, `meeting_discard`, `meeting_delete`; Rust callers
  use `end()` / `pause()`. The job first asks the meeting's AI who each
  voice still unnamed is (`speakers::naming::ask_model`, "Suggest who's
  speaking" on, a model that can call tools; best-effort), so the plan
  reads the names. **The end and the refine** (`finished` → `end_steps`):
  the refine due (`meeting_refine::plan`) comes first when the live
  transcript has holes — then `end_steps` follows it however it ends —
  else `end_steps` now and the refine after the summary; a meeting with a
  failed stretch never asks "Started by mistake?". `meeting_summarise`
  while the refine comes first does nothing (the summary follows by
  itself); `summary_running_for` is what a refine waits on.
- **`meetings/meeting_assist.rs`** — the notepad's AI helpers, on the meeting's model
  (Note Formatting scope, else cleanup), bounded for an 8k local model, via
  `chat_beside_dictation`: **"What did I miss?"** (`meeting_catch_up(noteId,
  since, question?)` → `{answer, nothingNew, upto}`; with nothing new since
  `since` it answers "Nothing new since you last looked." without a model;
  `question` = a follow-up from the same mini chat), the **one-tap helpers**
  (`meeting_assist(noteId, kind)` → `{answer, plain, tooLittle}`; `kind` =
  `Helper`: `questions` — "Questions to ask", 3–5 for now, while recording;
  `followUpEmail` — "Follow-up email", a recap to send the attendees,
  `plain` text, `clean_email`ed; `blindSpots` — "Gaps and risks": not
  decided / missing owners or dates / risks nobody followed up; inputs from
  `meeting_summary::questions_input` / `recap_input`, talk only; too little
  to go on → `Helper::too_little()` without a model; reply caps 300 / 600 /
  500 on a local model) and the **AI meeting
  title** (`maybe_title`, on every `yap-meeting-segment` and at the end: once
  the meeting has 150 words of speech or a digest — or, at the end, 20 words
  — and its title is open to the AI, `llm::MEETING_TITLE_PROMPT` → `clean_title`
  → `notes::set_ai_title` → `yap-note-changed {origin: "ai"}`; ≤ 3 tries, 2 min
  apart).
- **`meetings/notepad.rs`** — the **meeting notepad** window (label `notepad`, static in
  tauri.conf.json, created hidden + unfocused so `calls/capture.rs` always finds it
  and opening it never takes the foreground — tao keeps the don't-focus marker,
  so every `show()` is `SW_SHOWNOACTIVATE`; close = hide, the recording goes
  on; denylisted from the window-state plugin; gets the DWM-cloak init).
  `on_meeting_started` (from `meeting_end`) switches it to the recording note
  (`yap-notepad-note`) and, with `meeting_open_notepad` (default on), docks it
  to the right edge of the work area — full height, `notepad_width` = 30% of
  the width clamped to 400–800 px at 100% (scaled) and ≤ half (768 px on a
  2560 px screen, Wispr's size) — on the monitor
  with the call's window, else the cursor's; already on screen, it stays put.
  **Split the screen** (`meeting_split_screen`, default off): during a
  detected call, `pick_call_window` (unit-tested) finds the call app's main
  window — visible, top-level, not Yap's process, not minimised/tool/owned/
  cloaked, ≥ 200×150, titled; a browser window whose title shows the meeting
  first, else the largest — from `meeting_detect::call_window_exes()` (its
  exes, or the browser a meeting tab is in); a maximised window is restored first, then
  moved left of the notepad (`SetWindowPos`, async, invisible borders added
  back via `DWMWA_EXTENDED_FRAME_BOUNDS`). Never in test mode. The notepad's
  **split button** does it now (`notepad_split` → `split_now`; test mode: an
  error, "Test runs never move other apps' windows."), and **hovering** it
  shows a glass outline over the call's slot (`notepad_split_preview(show)`:
  an on-demand `split-preview` window — transparent, borderless,
  click-through, unfocusable, always on top, the slot + 24 px for its shadow,
  `preview_rect`; a generation counter keeps a late show from sticking; never
  in test mode; in `capture::MEETING_WINDOWS` and the window-state denylist).
  `open(app, noteId)` (the Notes view's "Notepad", the Yap bar) docks if
  hidden and focuses. Commands `notepad_open`, `notepad_state`,
  `notepad_split`, `notepad_split_preview`, `notepad_consent_message` (saves
  `meeting_consent_message`, emits `yap-consent-message-changed` for Settings'
  config copy); `yap-notepad-visible` tells the page when Rust shows/hides it.

## Frontend

- **`lib/Notepad.svelte`** — the **meeting notepad** (window `notepad`,
  `meetings/notepad.rs`), **Wispr Flow's notepad, measured** (2026-10-05, from its DOM
  over CDP: `references/wispr-flow/notepad-spec.md` (beside this repo); sizes, type,
  motion matched in Yap's own code, wording and icons; the colours are Yap's
  own since 2026-10-06). Palette = the `--yap-paper-*` tokens in app.css
  (paper `#faf9f6`, ink `#26231c`, surfaces s1/s2, the burnt-orange
  `--yap-live` and recording-red `--yap-live-strong`, the spring
  `--yap-ease-spring`, popover/dialog/sheet shadows). Top to bottom:
  **header** (52 px, a drag region): **back** (beige square → the note in the
  main window's Notes, `yap-meeting-open-note`), **⋯** (Copy as Markdown /
  Copy as text / Copy consent message / Audio settings while recording →
  Settings → Dictation's microphone / Save as .md via the dialog plugin + `note_export` /
  **Delete** in red → a Wispr-style confirm dialog → `meeting_delete`),
  **split screen** (tooltip "Split screen with meeting"; hover →
  `notepad_split_preview`, click → `notepad_split`), a **Share** split button
  (a local "Share notes" popover: Copy as Markdown / Copy as text / Save as .md
  / Email = a `mailto:` with the summary as text; the link half copies the
  notes in one click, its icon swapping to ✓), and min / max / close (close
  hides; the recording goes on). Ink tooltips via `data-tip`. **Title**: a
  textarea in EB Garamond 36/45, −0.04em, placeholder "New note", growing with
  its lines; a made-up title stays muted until the AI or the person names it;
  the date in the locale's format ("5 Oct, 22:32"). **Tabs** (15/600, ink
  underline, a hairline under the row): My thoughts (the note's `content`,
  autosaved, synced with NotesView both ways via `yap-note-changed`/`origin`),
  Transcript (a 3-bar burnt-orange waveform while recording), Summary ("+" until a
  summary exists). **Transcript**: the timer box (clock, elapsed, search —
  an inline input that filters and highlights lines, dividers hidden — and
  copy all) with the dismissible tip strip attached; speaker groups (a run of
  one voice: "You" / "Them" in teal, or the speaker map's names — "Priya",
  "Speaker 2", a guess in italics as "Priya?") of chat bubbles with Wispr's
  grouped corners and, on hover, a person button ("Who said this?") and a
  copy button each; the name (or the person button, for one line) opens
  **`SpeakerMenu.svelte`** (shared with the Notes view): "It's Priya" / "Not
  Priya" on a guess, Rename X everywhere…, Rename just this line / these N
  lines… (a name field with the attendees and known names as picks), Merge
  with… (other voices; never You), It was me (a voice told apart) → the
  `speaker_*` commands; over the lines (and over the Summary), the name
  card **`SpeakerCard.svelte`** (shared with the Notes view): "Speaker 2 is
  Priya Shah?" with why, Yes / Someone else (then "Who is Speaker 2?") /
  Not now, "1 of 2", "Speaker 3 is you?" → It was me + Undo
  (`speakers/naming.rs`, `notepadText.js` `pendingSuggestions`,
  `suggestionQuestion`, `suggestionWhy`); other windows
  follow `yap-note-changed`'s `lineSpeakers` (`withLineSpeakers`); Escape
  closes just the menu; a dashed **⏸ divider** wherever a recording stopped
  (`note.breaks`, `yap-meeting-breaks`; **a late read never takes it
  back**: the notepad re-reads its note on a stop, on `yap-notes-changed`
  and when a recording resumes in it, and each read keeps the fields an
  event changed while it was out — `changed`/`changing` in Notepad.svelte —
  and is dropped when a newer read was asked for; until 2026-10-22 a stop's
  reload answered after `meeting_end`'s break event put the old breaks back
  on a loaded PC, the divider gone until the next reload) and a "**You dictated here · left
  out of the notes**" divider (mic glyph) for a `dictated` marker; echo
  behind "Show N lines…"; "Yap is listening" / "No transcript yet" / "Live
  transcript is off" states (layout: `notepadText.js` `transcriptItems`,
  `bubbleShape`, `highlightParts`). **The meeting's brief**
  (`calendar_brief_for_note`, followed by `yap-meeting-brief`; the
  calendar's pre-meeting brief, [calendar.md](./calendar.md#pre-meeting-briefs)):
  at the top of Summary as "Brief before the meeting" (`BriefView`), open
  until the action plan is written, then folded above it with its teaser;
  while the meeting records, one dismissible line over My thoughts —
  "Brief · Last met Thu 16 Oct · 3 open items" — that opens it. On Summary
  rather than in My thoughts: My thoughts is the person's own words (and
  the action plan's input), Summary is Yap's, and empty while recording.
  **Summary** (the Rust job: "• Turning 12
  minutes of talk into an action plan… · Step 2 of 3", "Catching up on the
  meeting: part 3 of 12…", the rendered plan, the AI-setup card, the digests
  so far while recording; over the plan **Edit** / Done — its Markdown in a
  box, saved as typed —, "Edited", and "2 new lines since this summary ·
  **Update**"; its tasks tick in place; Update and Retry ask before writing
  over an edited summary). **A line's words** are fixed by double-clicking
  it or **Edit line** in its menu (Enter saves, Escape cancels, leaving
  saves; "edited" after it; the dictionary offer as this window's toast) —
  [notes.md](./notes.md#notes-you-can-fix-note_editsrs-tasksrs). **Bottom** (over the content, a 150 px fade):
  recording → the consent line "Let people know you're taking notes. **Learn
  more**" — a popover with why ("Yap transcribes on this PC; nothing is
  uploaded") and an **editable message for the meeting chat** (default in
  `notepadText.js`; saved on blur via `notepad_consent_message` →
  `meetingConsentMessage`, empty = the default; Copy → "Copied: paste it into
  the meeting chat"); after the meeting → an ink **Generate summary** pill
  (no summary yet) or the summary's error line ("The summary didn't come
  through" + detail + a Retry icon, `role="alert"`); then the **bar** (a
  32 px-radius tray): a white pill **Stop** / **Resume** / **Start** (recording-red
  ■ or ring-dot) and the **Ask anything** pill input, with the lead helper
  chip inside it while the sheet is down — "What did I miss?" while
  recording, "Follow-up email" after (a send arrow once typed). Focusing it
  or the chip opens the **Ask sheet** (a bottom sheet, 45% tall, sheet
  shadow; grab handle, New chat, Minimise; Stop shrinks to a 48 px circle):
  right-aligned question bubbles, markdown answers with 👍 👎 ⎘ (ratings kept
  in this webview's localStorage only, `yapNotepadFeedback`, never sent; the
  email shown `plain`, pre-wrapped, exactly as ⎘ copies it), and under the
  thread a **Suggestions** row of one-tap helper chips switching with the
  recording state (`meetingHelpers`/`MEETING_HELPERS` in `notepadText.js`:
  What did I miss? + Questions to ask while recording, Follow-up email +
  Gaps and risks after; hidden while a question is typed; the pill chip
  moves the focus to the field as it goes);
  "What did I miss?" = `meeting_catch_up` with `since` = the segments seen
  (all of them whenever the Transcript tab is on screen — window shown per
  `yap-notepad-visible`/`isVisible` — and scrolled to its newest line
  unsearched, or covered by the last answer); typed questions = the same
  command with `question`; no AI → a link to Language Models. Escape or a
  click above it puts it away. Toasts sit above the bar
  (`--yap-toast-bottom`); the quiet countdown, "Still in your meeting?" and the quiet stop's
  notice whenever Rust picks the notepad (it's on screen;
  `meetingCheckIn.js`). It has
  the **in-page hotkey fallback** (dictation, and the meeting shortcut when
  one is set), config re-read on focus.
- **`lib/stores/meetingAudio.svelte.js`** — a meeting note's audio on disk, for the
  Notes view and the notepad alike: `meeting_audio_note` per open note
  (refreshed on `yap-meeting-audio`), the jobs from `yap-meeting-redo`
  (`jobLine`: "Transcribing again with Parakeet V3 · 40%", "Transcribing the
  rest of the meeting"; `onJobDone` → the view reloads the note; one toast,
  in the window that started it: an error, or "Transcribed again", adding
  that the summary is the earlier one when there is one),
  `askTranscribeAgain` (the window's ConfirmDialog: "Transcribe “Supplier
  call” again?" · the model · notes and matching names kept · "The summary
  stays as it is until you generate it again."), `recoverMeeting`, and a
  line's ▶ (`playLine`: `meeting_audio_clip` as a blob in an `Audio`, one at
  a time; `canPlay`: placed, not a marker, audio kept). The **notepad**: ⋯ →
  **Transcribe again** (not while it records; greyed with the reason), ▶
  left of a line's person and copy buttons (and Play this line in its
  menu), and over the transcript the job's line or the crash banner ("Yap
  closed while this meeting was recording. The last 5 seconds weren't
  transcribed; the audio is kept." · **Transcribe it**). The **Notes view**:
  the note's menu **Transcribe again…** on meeting notes (off with the
  reason), ▶ beside a bubble on hover and in its menu, the same banner and
  line in the transcript box. Since 2026-10-24 `jobState` (the running
  job, a refine's "improved" offer, or why it changed nothing), `jobLine`
  for `refine` ("Improving the transcript · 40%", "…, then the summary",
  "…once the summary is written", "…after the meeting that's recording")
  and `retry`, `cancelJob`, `updateSummary`, `summaryFollows` (the
  notepad's Generate summary pill hides while the refine comes first) and
  `retryChunk`; `canPlay` skips failed markers.
- **`lib/components/MeetingJob.svelte`** — a note's job line over the transcript, in
  the notepad (`look="paper"`) and the Notes view (`look="app"`): spinner +
  `jobLine` + **Cancel**; "✓ Transcript improved · **Update summary**"; a
  refine that changed nothing, with ✕.
- **`lib/components/FailedChunk.svelte`** — a failed chunk's marker in both views (a
  `transcriptItems` item of kind `failed`, `notepadText.js` `failedText`):
  a dashed divider "⚠ Couldn't transcribe 0:42–1:12 · **Try again**"
  (`retryChunk`; off while it records — "After the meeting" —, while a job
  runs, or with no audio), its tooltip naming the side.
- **`lib/notepad/SplitPreview.svelte`** — the split preview window's page (label
  `split-preview`, App.svelte routes it, transparent body): one glass
  rectangle inset 24 px (`rgba(255,255,255,.10)`, a 2 px
  `rgba(255,255,255,.45)` border, radius 12, a dark hairline + soft shadow),
  readable against any desktop.
- **`MeetingsView.svelte`** = the Meetings
  view (Wispr's Notetaker page): serif title, Sync calendar (an icon) /
  settings gear / **◉ Take notes** (beige, Wispr's "Start Notetaker": the
  meeting shortcut as a button via `meeting_shortcut` — notes on the live
  call, else a new meeting note; **■ Stop and summarise** while one
  records); **Today** and **Upcoming · Next 7 days** (by day, 3 at a time +
  Show more; time, name, Conflict / Maybe tags, service, attendees, "In 1
  min", Note ready; from 10 min before: Join meeting / Start / Join +
  Start, or Switch notes while another records, or ● Recording · Open note;
  with "Prepare a brief before meetings" on, a **Brief** button on every
  meeting still to come that unfolds its pre-meeting brief under the row,
  made now if need be (`loadBrief` → `calendar_brief`; its summary follows);
  opening a meeting makes or opens its note via `calendar_event`); with no
  calendar, one grey card (calendar icon, "No meetings found", ink Connect
  calendar → the dialog); **Past meeting notes** by day ("TODAY, 6 OCT",
  time · attendees · "Generate summary" on a meeting with talk and no plan
  → `meeting_summarise` + opens the note) with search
  (`calendar_meeting_notes`) and, over the list, **Moments that match**
  (`search_moments`: up to 5 passages — title, day, time into the meeting,
  speaker, the line — "· by meaning" when the search model helped; one
  opens its meeting at the line; [search.md](./search.md#the-meetings-views-moments));
  a bottom **Ask bar** cycling example
  questions ("What questions were left unanswered in my last meeting?")
  that opens Chat with the question (scope `meetings`, through
  `chatRequest.svelte.js`: asked across every meeting and note, answered
  from cited passages — [search.md](./search.md)), plus a **Past chats ↗**
  chip (opens Chat).
