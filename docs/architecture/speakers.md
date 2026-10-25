# Speakers: who said what

Telling the call side's voices apart (the beta speaker engine, a child process) and the speaker map that puts names on lines. See [`docs/meetings.md`](../meetings.md) "Who said what" and [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`speakers/diarize.rs`** (+ `speakers/diarize/`) — **telling the voices on the call side
  apart** (beta, off by default; the spike: yap-private
  `docs/diarization-spike.md`). Engine: **Nemotron-3-Diarization** (q8_0 GGUF,
  107 MB, OpenMDW-1.1) on **NeMo-Speech.cpp 0.2.0** (CPU build, 5.5 MB zip,
  Apache-2.0), streaming with the **30 s preset** (`v3-offline`) on 4 OpenMP
  threads: ~2.4% of real time, labels ~10 s (≤ 21 s) behind the audio, final
  at the stop, no audio kept. **`yap.exe diarize --runtime <dir> --model
  <gguf> [--preset] [--every-ms] [--no-dither] [--seed]`** (main.rs →
  `child.rs`): its own process (a 0.2.0 runtime's asserts abort it; its ggml
  isn't whisper.cpp's), below-normal priority, no window; loads
  `nemo_speech_asr_c.dll` with `LoadLibraryExW(…, DLL_LOAD_DIR |
  DEFAULT_DIRS)` (`libloading`; its ggml + MSVC runtime from that folder),
  runs in the model's folder (the C ABI's narrow path), adds **−60 dBFS
  white-noise dither** (`Dither`, seeded; exact digital silence breaks
  Nemotron), pushes ≥ 0.25 s at a time. **Wire** (`protocol.rs`, `PROTOCOL`
  1): stdin = frames `tag:u8 len:u32le payload` — `'A'` f32le samples, `'S'`
  a u32 count of silence; EOF = finish. stdout = JSON lines: `ready
  {protocol, speakers, frame_ms, runtime, load_ms}`, `segments {final,
  audio_ms, labelled_ms, compute_ms, lag_ms_median?, lag_ms_max?, segments:
  [{start_ms, end_ms, speaker}]}` (the whole list each time; speakers 1…8 by
  first appearance, stable — `timeline::SpeakerOrder`) every 5 s of audio
  once labels moved on, then `final: true`; or `error {message}`. stderr →
  the log at debug. **Yap side**: `Session::for_meeting(note_id)` (config +
  installed) → `tee()` → `finish()` (≤ ~20 s, normally < 1 s; dropped
  unfinished = cancelled, nobody told). A feeding thread batches the tee's
  frames into the child's stdin; a child that dies is started again
  (`MAX_RESTARTS` 2) at its place on the clock as a new `stream` (a voice is
  `(stream, speaker)`; the gap is in `gaps`); past 5 min of queued audio the
  tee sends silence (memory bounded, clock right). **The API for the speaker
  map**: `diarize::on_segments(|tl: &Timeline| …)` — every few seconds while
  recording and once with `done: true`; `diarize::timeline(note_id)` — the
  latest. `Timeline {note_id, session, segments: [Segment {start_ms, end_ms,
  speaker, stream}], audio_ms, labelled_ms, done, restarts, gaps, error}`, ms
  on that recording's session clock ("Them" sample position / 16; a resumed
  meeting is a new session, new numbers; `Session::id` = its `session`).
  `speakers::labels` (registered at setup) puts them on the "Them" lines;
  the final handler runs before `finish` returns. **Models** (`registry.rs`): Nemotron-3 (Recommended, the
  default; HF `nvidia/Nemotron-3-Diarization` pinned to commit `f667ed73…`)
  + its runtime package (NVIDIA's v0.2.0 release zip: `bin/*.dll` minus the
  CLI/llama/nmt/tts, `share/licenses/` → `licenses/`), and the sherpa-onnx
  fallback (pyannote seg-3.0 + ERes2Net + sherpa 1.13.8, pinned) listed as
  **"For phone-only calls — coming later"** (`available: false`, no engine).
  Install = download (resume; Cancel, `download_cancel {id:
  "diarization:<id>"}`, through the shared helper `shell/download.rs`,
  app-shell.md, 2026-10-25: the partial file and the half-made folder
  go), size + SHA-256, unpack into
  `<folder>.installing/`, rename into `<data>/models/diarization/<folder>/`,
  plus a `NOTICE.txt`; installed = every required file there at its pinned
  size; delete takes the runtime too when nothing else uses it (refused
  while a recording uses it). Commands `diarization_status` / `_download`
  (`diarization-download-progress {id, stage, percent, downloadedBytes,
  totalBytes}`, then `diarization-changed`) / `_delete`; debug-only
  `diarize_wav {path, speakers?}` (the real child on a file, a second at a
  time; `speakers` caps voices afterwards — the engine takes no count),
  `diarization_timeline {noteId}` and `e2e_diarize_timeline {segments,
  labelledMs, done, error?}` (test mode: a scripted timeline for the
  recording now running, session `E2E_SESSION`; a `done` one is held until
  the recording stops — `finish_scripted` — where the engine's final labels
  come). Debug-only env: `YAP_DIARIZATION_MODELS`
  (another models folder), `YAP_DIARIZATION_SOURCES` (stand-in URLs + SHA-256s,
  JSON per package); a test run only fetches from loopback. Logs: counts and
  times, never audio. Measured (`tests/diarize_child.rs`, the spike's TTS
  meetings, 0.25 s collar): 0.4–2.7% on 2–3 voices and dial-ins, 2.3% on 4
  with overlap + noise, 0.3% / 0.7% at 30 / 60 min, 15.9% on 4 voices made
  from two (as the spike), ~50% on phone-only audio (the fallback's job).
- **`speakers.rs`** — **who said what** in a meeting: the note's speaker map
  (`Note::speakers`: voice id → `Speaker {name, email, source: user |
  platform | voice | calendar | ai, locked, mergedInto, not, suggestion}`)
  and the names it puts on lines everywhere. Voices: "you", "them" (the call, not told
  apart), later "them-1"… (the voice separation), "p-1"… (a person the user
  named on some lines). **Precedence** (OpenWhispr's `speakerAssignmentPolicy`
  by behaviour): the user's name is locked against every automatic relabel,
  and so is a line they assigned (`speaker_locked`); otherwise call app >
  voice match > calendar = AI guess, never downwards; "Not Priya" (`not`)
  keeps that name off the voice for good. A calendar/AI name is a guess:
  displayed "Priya?" (`display`), told to the prompts as a guess
  (`speakers_line`). Edits (`rename` everywhere — to another voice's name =
  merge; `rename_lines` — a voice by that name, else a new person, lines
  locked; naming a line after a guess confirms it; `merge` — the more trusted
  name wins, later lines of the merged id follow; `not`; "You" never renamed
  or merged) are the commands `speaker_rename` / `speaker_rename_lines` /
  `speaker_merge` / `speaker_not`, plus the name cards' `speaker_suggestion
  {answer: yes | no | later}` and "It was me" (`speaker_is_me` /
  `speaker_not_me`) — each returns the note, suggests names again from
  what's left (`naming::refresh`), and emits `yap-note-changed` with
  `lineSpeakers` (`[voice, locked, echo]` per line) and `enhancedContent`,
  + `yap-notes-changed`; a name put on a voice that a summary already
  called "Speaker 2" is written into the summary and the digests' owners
  too (`naming::rename_in_summary`). `suggest` is the automatic path. **1:1 calls** (`calendar_guess`, from `notes::link_event`): exactly
  one other invitee (no you, rooms or declines) → "Them" named after them,
  source calendar. `known_people` (attendees + map names) is what owner
  checks accept. **Line timing**: `word_times(text,
  timed)` (LCS line-up of the model's words with the corrected text; a word
  the dictionary changed takes what it replaced), `line_timing`, and
  `split_line(seg, &[Turn {start_ms, end_ms, speaker}])` — a line split at
  its words where the speaker changes (most overlap, else nearest ≤ 1.5 s,
  else neighbours; one-word blips < 400 ms smoothed), in proportion without
  word times; a user-locked line, marker or unplaced line comes back
  unchanged. Lines' `start_ms`/`end_ms` are on the same session clock as
  `diarize::Timeline` (sample position / 16, 0 at each recording's start).
  **`speakers/labels.rs` — the engine's voices on lines**: a recording
  (`begin(note, session)` from `meeting::start`: its lines = from the
  transcript's length then) takes its session's timelines (`on_segments`;
  in a test run with no engine, `E2E_SESSION`'s): each labels that
  recording's "Them" lines that are talk, placed, settled (`end_ms ≤
  labelled_ms`; all once `done` without an error), unlabelled and not
  `speaker_locked` (never You/echo/markers/old lines) — `split_line` with
  the turns near it, the parts replacing the line under `notes::modify`
  (`notes::replace_line`: digests and breaks after it move along; a line
  with no voice near stays "Them") — then emits `yap-note-changed` (origin
  "speakers": the windows take only the lines and the map, never the
  title/notes being typed) with `lineSpeakers` + `lineSplice {at, remove,
  lines}` + `breaks` (windows splice, else refetch; `hasLine` drops a
  segment event for a line they already have). `after_ingest` labels lines the latest
  timeline already covers; `end` handles an engine that never sent its
  final labels (as far as they went). **Voice ids**: engine voice `(stream,
  speaker)` → "them-N", N numbered on from the note's highest "them-N" in
  the order voices first get a line (`VoiceIds`; a restarted engine's
  voices and a resumed meeting's are new numbers, never merged by
  themselves). **The 1:1 rule** (`settle_them`, each pass, per recording):
  "Them"'s name (a calendar guess, or the user's) goes to the recording's
  voice when it found exactly one (a guess stays a guess); with several, a
  guess goes to the voice with the most words (the first to qualify keeps
  it while recording; moved at the end), never one with its own name or a
  "Not" for it, and a user's name stays on "Them"; "Them" keeps its name for
  unlabelled lines (`voices` leaves out a line-less "Them" once voices are
  told apart; `speakers_line` says a name once). An error at the end →
  `yap-meeting-speakers {noteId, message}` (the notepad: "Couldn't tell
  speakers apart this time.") + an info log. Unit-tested; e2e with scripted
  timelines; measured with the real engine + Parakeet on the spike's TTS
  meetings (`diarization-real.spec.js`, 2026-10-18): 93.6–99.5% of words on
  the right person, every call line labelled, a name ~15 s (≤ ~23 s) after
  the words. Each pass ends with the names (`naming::refresh`, with the call
  app's readings once the pass gave its voices ids); a voice that repeats
  yours never takes "Them"'s 1:1 guess (`naming::repeats_you`).
- **`speakers/naming.rs`** — **who each voice is**: names suggested for a
  meeting's voices from what the meeting says and who was invited, for the
  person to confirm ("Speaker 2 is Priya?" — Yes / Someone else / Not
  now). No voiceprints. **Utterances**: every talk line split at its
  sentences, placed by its word times (in proportion without), in time
  order across both sides; **turns** = runs of one voice (backchannels —
  "Yeah.", "Mm-hmm." — skipped when looking for the turn before or after).
  **Names** (`Roster`): the note's participants (the invite's attendees and
  any added), names the map already has (the person's or the call app's are
  *taken*: never suggested for another voice), Memory's People, the call
  app's readings, and names *said* in the meeting that no list has — in an
  introduction ("I'm Rochelle", "Ben here", "Declan from legal here"), or
  twice where someone is handed over to or thanked, a word Windows' spell
  checker knows in lower case never counting (`memory::Words`); first
  names and usual short forms ("Kate" → Katherine, "Rob" → Robert,
  `short_forms`) as aliases where only one person has them; the person's
  own name (About you) is "You", never a call voice. **Cues** (a weight per
  (voice, person)): an introduction +1.3–1.8 for the voice saying it
  ("introducing someone else" — "this is Sam, our new analyst" — is −1);
  a handover at the end of a turn ("Priya, can you…?", "…, Sam?", "over
  to you, Siobhan", "the floor's yours", "Hi Sam, can you hear me?") +1.0
  (+1.2 explicit; ×0.6 / ×0.3 a sentence or two before the turn's end) for
  the next turn's voice; thanks at the start of a turn ("Thanks, Priya",
  "Please do, thanks Ola"; "Great point, Sam" +0.7; a greeting right before
  the name +0.3; split between several names) for the turn before; the
  voice saying it gets −2 (nobody hands over to or thanks themselves), and
  any other mention −1 ("Priya will send it", "Sam's dropped off"); the 1:1
  invite's guess +1.2; the AI's checked answer +1.0; the call app +3 × its
  share. **Deciding** (`propose`): net ≥ `SUGGEST` (1.0) and a lead of
  `MARGIN` (0.6) over the voice's next name and the name's next voice; one
  name a voice, strongest first. **Applied** as Yap's guess (shown "Priya?",
  the AI told it's a guess, so the action plan reads it) only at net ≥
  `APPLY` (2.0), lead ≥ 1.5, nothing against it, two separate cues or an
  introduction, and a name from a list (or said twice); the call app's firm
  word (≥ 3 readings, ≥ 75% of the voice's) as the call app's name, no
  card. Never "the one invitee left" — measured wrong when an invitee is
  away and someone uninvited speaks. **"It was me"**: a voice ≥ 60% of
  whose words (≥ 8) repeat one of your lines within 6 s (`echo_lines`, LCS)
  is asked "Speaker 3 is you?"; `is_me` merges it into You (the one way a
  voice merges into You) and marks its repeats `echo` (hidden, out of the
  summaries); `not_me` undoes it and never asks again. **`refresh`** writes
  the proposals into the map (`Speaker::suggestion {name, email, me,
  confidence, why: {kind, by, quote}, later}`): new or changed cards
  (`later` survives while the name stays), stale ones gone with the guess
  they put on; one from the AI or the call app stays while nothing
  contradicts it. Runs after each labelling pass, after the person's own
  speaker edits, every 10 s of a recording with no speaker engine
  (`labels::after_ingest`, "Them" only), and at its end. **The AI step**
  (`ask_model`, from `meeting_end::write_action_plan` before the plan; only
  a model `tools::supports_tools` passes — cloud, or ≥ 4B on this PC): one
  question per voice told apart that said ≥ 10 words and has no firm name
  (≤ `ASK_MAX` 4), with the names still open and an excerpt of ≤ 650 words
  (the lines around where a name is said near that voice, then its first
  lines; one line per run of a voice), `ASK_PROMPT` (only addressed then
  answers / thanked after / introduces itself; never topic or style; else
  "unknown"); a model on this PC answers under the JSON schema as
  llama.cpp's grammar, `evidence` before `name` (the properties go out in
  alphabetical order, and the grammar follows them), inside the sidecar's
  Busy guard and never during a dictation (`meeting_summary::
  chat_beside_dictation_with`); `check_answer` keeps an answer only when
  its evidence, labels off, is in the excerpt word for word (or nearly),
  says the name, and isn't the voice itself naming that person (unless an
  introduction). Settings → Meetings → "Suggest who's speaking"
  (`config.meeting_speaker_names`, on) switches all of it. Logs: counts.
- **`speakers/call_ui.rs`** — **who the call app shows talking**
  (experimental, `config.meeting_call_names`, off; no setting shows it).
  `begin` (from `labels::begin`) watches each recording; with the flag on
  (never in a test run) a `call-ui` thread finds the live call's window
  (`meeting_detect::call_window_exes` + `notepad::call_window_quiet`, again
  every 10 s until found) and reads it every 800 ms (slower if a read is
  slow, nothing while it's minimised): `win::snapshot` — one cached UI
  Automation request (`ElementFromHandleBuildCache`, `TreeScope_Subtree`,
  the raw view, ≤ 30,000 elements, 2 s timeouts) of control type, name,
  class (a Chromium page's DOM `class`), automation id, item status, help
  text, ARIA role and properties, offscreen; nothing invoked, focused or
  moved, no screenshot. `read` applies the app's `Profile`
  (`call_ui/profiles.json`, built from Wispr Flow's defaults — Meet's
  speaking class `kssMZb` on a tile `oZRSLe`, Teams' `vdi-frame-occlusion`,
  Zoom's "Active speaker" texts; an app's entry in
  `<data>/call_speakers.json` wins, no rebuild): a speaking class → the
  tile's name (its tile class, else the nearest named element); a speaking
  text ("{name}, speaking", "Active speaker: {name}"); self marks ("(You)",
  "(Me)", "you are speaking"); names cleaned (`clean_name`: marks and
  states off, "Shah, Priya" → "Priya Shah", control words never a name).
  Readings go on the recording's clock (`meeting::them_clock_ms`, the
  engine's clock) as spans; `align` gives each voice the name shown alone
  (one person, not you) for most of its speech (≥ 3 readings' worth, ≥ 60%;
  the highlight counted from 0.3 s after a stretch begins to 1.2 s after it
  ends): `votes_for` (voices told apart) / `votes` ("Them" with no engine:
  only a name shown ≥ 90% of the time) → `naming`. `finish` once the
  recording's final labels are in. Debug-only `e2e_call_readings {spans,
  selfName}` stands in for the window. **`yap.exe call-dump`** (`dump_main`,
  any build; a release exe prints nothing but still writes the file): the
  call window (by `--window` title text, else the live call app's or a
  browser showing a meeting) read `--count` times every `--every` ms into a
  `Dump` JSON (each read with what the rules found), `--names
  "Real=Stand-in;…"` swapping names for sharing. The fixtures in
  `call_ui/fixtures/` are synthetic, not captures.

## Testing the speaker engine

**The speaker engine** has three layers of tests: unit tests (`diarize/*`:
the wire, dither, speaker order, timelines, the registry, the zip), the e2e
`diarization.spec.js` (Settings with a fake model + runtime from a local
server via `YAP_DIARIZATION_SOURCES`; test mode never downloads from the
internet), and, locally with the files on disk (never on CI, never
downloading): `cargo test --test diarize_child -- --ignored --nocapture`
(`YAP_DIARIZATION_MODELS`, `YAP_DIARIZATION_AUDIO` = the spike's
`audio/conv`: the real child on its WAVs, scored against their RTTMs) and
`diarization-real.spec.js` (`YAP_E2E_DIARIZATION_MODELS` /
`_AUDIO`: `diarize_wav`, and a meeting recorded from a 3-voice file with
the engine labelling it, its "Them" lines ending with one voice per person;
`YAP_E2E_DIARIZATION_STT` = a Parakeet folder with an `engines` build as
`YAP_E2E_EXE` scores the words put on the right person). The labels on
lines without a model: `speaker-labels.spec.js` (scripted timelines,
`e2e_diarize_timeline`). The real download once:
`cargo test real_download -- --ignored --nocapture`
(`YAP_DIARIZATION_DOWNLOAD_DIR`, ~112 MB).
The names: `cargo test speakers::` (the rules, the cards' answers, "It was
me", the AI step's checks, the call reader's fixtures and alignment),
`cargo test eval::speakers` (Rust alone over every meeting in
`evals/speakers/`: **no wrong name allowed**, coverage ≥ 55%),
`cargo test --lib -- --ignored reads_a_real_window` (the real UI
Automation read, on a window of the test's own far off screen), and the e2e
`speaker-names.spec.js` (the card in the notepad and the Notes view, Yes /
Someone else / Not now, the names in the action plan's input, the AI's
checked answer at the end of a meeting, "It was me" and Undo, the call
app's readings naming voices through `e2e_call_readings`).

## Names for the voices: evaluations

`yap.exe eval --speakers [--model …]` (`ai/eval/speakers.rs`,
[`docs/model-evals.md`](../model-evals.md)) scores Rust alone, then Rust +
each model, on `evals/speakers/*.json`: 31 fictional meetings whose call
lines are already told apart as the engine would (`them-1`…), their
attendees, what Memory knows, and the truth per voice (`null`: nobody to
name — two people the engine merged, a chair nobody names). Traps on
purpose: an invitee off sick and talked about, someone asked by name who
dropped off and another answering, "Will" and "Mark" used as words, two
Sams, "Thanks Priya and Sam", greetings while people join, your voice on
the call side (with and without a 1:1 invite), an invitee away and someone
uninvited chairing, a voice split in two, two merged; five written after
the rules were tuned (`holdout-*`). Per voice: **correct**, **wrong** (a
name that isn't theirs — worse than "Speaker 2"), **missed**; *named* = put
on the voice as Yap's guess (what the action plan reads before anyone
answers), *suggested* = every card. 73 voices have a name to find.

| Run (2026-10-22) | Named correct | Named wrong | Suggested correct | Suggested wrong | AI questions (kept / thrown out by Rust) | Per question |
|---|---|---|---|---|---|---|
| Rust only | 25 (34%) | 0 | 44 (60%) | 0 | — | — |
| Rust + Qwen3.5-4B, graphics card (`--ngl 999`) | 27 (37%) | 0 | 46 (63%) | 0 | 45 (4 / 4) | ~0.2 s |
| Rust + Qwen2.5-1.5B, processor | 25 (34%) | 0 | 44 (60%) | 0 | 45 (0 / 37) | ~11 s |

What the numbers say: the transcript's own cues carry it (names said where
the meeting hands over, thanks and introduces); the 4B adds a little
(it read "I'm joined by my colleague Ben" before Ben spoke; two of its
four kept answers were names Rust had already); the 1.5B answered with
names it couldn't back 37 times out of 45 and Rust threw every one out —
so the AI step only asks a model that can call tools. Misses are mostly
voices nobody names (a chair), and voices the engine split or merged. The
holdouts, before three general fixes ("Sorry Hannah, go on" isn't thanks;
"the floor's yours"; short forms like Kate / Rob), had one name that was
right but in another form ("Kate" for Katherine) and more misses; after,
none wrong. Coverage on real meetings will be lower than this: these are
written to have names in them.

Before the 4B's numbers counted, two things were fixed from its
transcripts: its schema put `name` before `quote` (alphabetical), so it
named before it found the words, and said "unknown" to all 45; and it
quoted lines with their labels ("Speaker 1: Thanks, Priya…"), which the
first check threw out.

## Reading the call app: what a real call has to show

Nobody can join a call from the build machine, so `call_ui.rs`'s rules are
untested against a real Teams, Zoom or Meet window: the markers in
`call_ui/profiles.json` are Wispr Flow's remote defaults and what's public
(Wispr's own notes say Teams on Windows stays anonymous for them, and Zoom
names only in English). One capture per app settles it. **Steps**
(maintainer, in a real call with at least two other people, cameras on
for some and off for others):

1. Use a debug build's `yap.exe` (Yap - Dev's, or `cargo build` in
   `src-tauri/`): it prints as it reads. The installed (release) exe writes
   the same file but prints nothing.
2. Start the call and leave its window **on screen, not minimised** (a
   minimised window shows nothing). Gallery view first.
3. In a terminal: `yap.exe call-dump --app teams --count 90 --every 1000
   --out teams-gallery.json --names "Their Real Name=Priya Shah;Other Real
   Name=Sam Patel;Your Name=Imogen Hart"` (`--app zoom`, `--app meet`; for a
   browser meeting add `--window "Meet -"` or `--window "Microsoft
   Teams"`). It reads the window 90 times, once a second — read only.
4. While it runs, **talk in turns and note the times**: person A ~10 s,
   person B ~10 s, you ~10 s, A and B at once ~5 s, silence ~5 s. The file's
   `atMs` is from the start; each read's `found` is what the current rules
   saw.
5. Again for: speaker view (Teams "Speaker", Zoom "Speaker view"), with
   someone sharing a screen, with the participants list open. One file each.
6. Apps: the new Teams app, Zoom's desktop app, Google Meet in Edge and in
   Chrome, Teams in a browser — whichever calls come up.
7. **Check each file before it leaves the PC**: search it for the real
   names (the `--names` swap misses nicknames and email addresses) and for
   chat messages; it holds whatever the window showed. Keep the raw files
   in the private repo or `references/`, never this one.
8. With a file: where `found` stays empty, compare a read where someone
   talks with one where nobody does (an element whose `class`, `name`,
   `status` or `aria` changes), put that marker in
   `<data>/call_speakers.json` (the same shape as `profiles.json`; an app's
   entry there wins), re-run `call-dump` (or point a unit test at the
   file) until `found` matches who talked; then move it into
   `profiles.json`, and add the cut-down, renamed dump to
   `call_ui/fixtures/` with `expect` set on its reads.
9. Then try it whole: Yap - Dev with `"meetingCallNames": true` in
   config.json and "Tell speakers apart" on, record a meeting during a real
   call: the voices should take the people's names, no "?", within ~15 s
   of the labels.

## The speaker engine measured (2026-10-22), and why it stays in beta

`cargo test --test diarize_child -- --ignored --nocapture` (the real child,
started as Yap starts it, frame-level error with a 0.25 s collar and the
best speaker mapping, overlap scored) on the spike's Windows-voice meetings
and on new ones made from **Kokoro** voices (eight distinct US/UK voices
reading the spike's 70 meeting sentences, mixed by the spike's composer;
the scripts and WAVs are in `references/diarization-spike/` —
`scripts/kokoro_utts.py`, `scripts/compose_kokoro.py`,
`audio/conv-kokoro/`). The PC was busy (other builds and benchmarks), so the
times are high; the error isn't affected.

| Meeting | People | Found | Error |
|---|---|---|---|
| Kokoro, 2 / 3 / 4 voices, clean | 2 / 3 / 4 | all | 0.4% / 3.0% / 1.6% |
| Kokoro, 5 / 6 voices, clean | 5 / 6 | all | 7.7% / 10.0% |
| Kokoro, 4 / 6 voices, overlaps, levels −9…0 dB, noise at 15 dB | 4 / 6 | all | 0.3% / 5.5% |
| Kokoro, crosstalk (half the turns start 0.5–2.5 s early, backchannels over most long turns), 3 / 4 voices | 3 / 4 | all | 0.6% / 0.8% |
| Kokoro + Windows voices mixed, 5, overlaps, noise at 20 dB | 5 | 5 | 1.6% |
| Kokoro, 4 voices, 30 min | 4 | 4 | 0.3% |
| **Three similar voices** (three US women) | 3 | **2** | **32.9%** |
| Spike: 2 / 3 voices clean, 3 noisy, 3 / 4 dial-ins, 4 hard (Opus) | — | all | 2.7% / 1.5% / 0.7% / 0.4% / 8.0% / 2.3% |
| Spike: 4–6 voices made from three (pitch-shifted copies) | 4–6 | 5–6 | 12.5–21.0% |
| Spike: 30 / 60 min, Opus | 4 | 4 | 0.3% / 0.7% |
| Spike: everyone on a phone line | 2 / 3 | 1 / 2 | 48.0% / 50.9% |

**CPU and memory over a one-hour meeting** (`long60_opus`, the engine as
Yap runs it — 4 OpenMP threads, a timeline every 5 s of audio, below-normal
priority — CPU time read by psutil, `scripts/cpu_cost.py` beside the others): **391 CPU seconds an hour of meeting**,
on average **~11% of one core** while it records (0.7% of this 16-thread
PC), **148 MB** at most; the same hour fed as fast as it would take it ran in
122 s. **A resumed meeting** (`diarization-real.spec.js`, the real engine
on the spike's 3-voice meeting recorded twice into one note): a second
engine for the second recording, its voices Speaker 4–6 after the first's
1–3, the first recording's lines untouched. **The download** (`cargo test
diarize::registry::tests::real_download -- --ignored`, the pinned NVIDIA
release + Hugging Face commit): 112.5 MB in all downloaded, SHA-256 checked,
unpacked into place with its licences and NOTICE files, passed.

**Verdict: "Tell speakers apart" stays beta and off by default.** Distinct
voices are told apart well (≤ 3% for 2–4 people, overlap and crosstalk
included, steady over an hour) and it's cheap. But: every number above is
synthetic speech — not one real call has been measured; voices that sound
alike merge (three similar women came out as two, 33% error), and that is
the failure that hurts most, because one "Speaker 2" then holds two people
(name suggestions abstain on such a voice, but its lines are mixed); 5–6
people climb to 8–10%; phone-only calls are ~50% wrong; and the runtime is
NeMo-Speech.cpp 0.2.0, whose asserts abort the child (Yap restarts it, ≤
2). The Settings hint now says so plainly ("measured on test meetings, not
yet on real calls; voices that sound alike can come out as one"). What
would take it out of beta: one real multi-person call measured (the
`diarization-real.spec.js` path with a recording of it, or the maintainer's
own meeting with the call app's names as the truth once `call_ui.rs` is
checked), and the similar-voices case improved or at least flagged in the
notepad.

