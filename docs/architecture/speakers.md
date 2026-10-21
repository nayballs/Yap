# Speakers: who said what

Telling the call side's voices apart (the beta speaker engine, a child process) and the speaker map that puts names on lines. See [`docs/meetings.md`](../meetings.md) "Who said what" and [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`diarize.rs`** (+ `diarize/`) — **telling the voices on the call side
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
  Install = download (resume), size + SHA-256, unpack into
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
  platform | voice | calendar | ai, locked, mergedInto, not}`) and the names
  it puts on lines everywhere. Voices: "you", "them" (the call, not told
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
  `speaker_merge` / `speaker_not` (return the note, emit `yap-note-changed`
  with `lineSpeakers` + `yap-notes-changed`); `suggest` is the automatic
  path. **1:1 calls** (`calendar_guess`, from `notes::link_event`): exactly
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
  the words.

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
