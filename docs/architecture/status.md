# Current status

What works today, how it was tested, and what's not done yet. `ROADMAP.md` is in the private docs repo (`nayballs/yap-private`).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Measured (2026-10-22, a release `engines` build, run portable)

Ryzen 7 9800X3D, RTX 5070 Ti, Windows 11, Parakeet V3; memory = private working set
(Task Manager's "Memory") summed over `yap.exe` + its WebView2 processes:
- **Idle in the tray: ~290 MB** (321–325 MB before one chunk per window and the
  hidden webviews' low memory target), **0.7 % of one core**.
- **A 10-minute meeting** (real mic + loopback): flat at ~535 MB, 23 % of one core;
  ~200 MB wasn't given back after it ended — ONNX Runtime's DirectML encoder
  plans per chunk length, gone since the encoder runs at one planned length
  (2026-10-22: `yap.exe` 259 MB before a meeting, 256 MB after; `meetings.md`).
- **Hotkey → recording: 1–3 ms**; stop → transcript 0.30–0.45 s for 3.4 s of audio.
- **Cold start → tray and bar: ~2.5 s**, nearly all of it loading the speech model on
  the setup thread (7.4 s on the very first launch); the main window comes up then on a
  launch by the person (`app-shell.md`). Since 2026-10-22 the model loads in the
  background: tray, bar and window at ~1.0 s instead of ~3.4 s (a release-optimised
  build with debug assertions, test mode), the model ready at ~3.3 s; a dictation
  started sooner waits for it ("Loading the speech model…").
- **The microphone is closed while Yap idles** (2026-10-22, `dictation/mic.rs`): it opens when a
  dictation starts and stays warm a minute (never on Bluetooth: a headset keeps its
  music profile). Idle `yap.exe` 0.54 % → 0.21 % of one core; Windows no longer shows
  the mic in use. USB mics deliver ~30 ms after the press; a wireless headset's dongle
  gave digital silence for ~0.5 s, which the start chime now waits out.
- **Installer 20.7 MB** (NSIS) + the 456 MB model on first run.
- For comparison, Wispr Flow's Windows app is reported at ~800 MB and ~8 % CPU idle
  (user reports, no official figure); its own package is 221–316 MB.

## Measured on smaller PCs (2026-10-24, `yap.exe bench`, emulated)

The same PC held to 4 and 2 threads, its 2-unit integrated Radeon, and
DirectML on Windows' software renderer (no graphics driver) — **an emulation,
not a laptop** (faster cores, a weaker integrated GPU than a laptop's). Full
tables, method and how anyone can run it and send theirs:
[`docs/benchmarks.md`](../benchmarks.md).
- **Speech (Parakeet V3, 11 s of speech): 0.13 s on the card, 0.34 s on 16
  threads, 0.38 s on 4, 0.60 s on 2**; 3 s of speech 0.10–0.18 s everywhere.
  On the integrated GPU ~4 s (the encoder's 30-s plan), on the software
  renderer ~1 s: speech now starts on the processor without a real card
  (`ai/hardware.rs`). Moonshine Base on DirectML (any real GPU) gets every word
  wrong; fine on the processor.
- **Cleanup (Qwen2.5 1.5B): 0.08 s on the card, 0.9 / 1.1 / 1.6 s on 16 / 4 / 2
  threads**; Qwen3.5 4B 0.16 s on the card, 2.5–3.9 s on the processor;
  Groq's gpt-oss-20b 0.5–0.8 s. A small PC (< 8 threads or < 8 GB) is offered
  the cloud first in onboarding, with both times.
- **Cold start to the first-run window: 0.7–1.2 s** (a throwaway portable copy,
  fresh WebView2 profile). **First run**: the 478 MB speech model step ~15–18 s
  on a fast line (the model loaded once now, not twice), the first dictation
  0.43 s from its end to its text with the 4B's cleanup. **Home** has its data
  73 ms after its page starts (132 ms before `home_snapshot`).

## What works

**Trust: network receipts and Report a problem** (2026-10-24,
[privacy-receipts.md](./privacy-receipts.md)): every request Yap makes goes
through `privacy/net.rs` (a unit test fails on any that doesn't), recorded by host,
purpose, feature, status and bytes — never content — in `connections.json`
for a week; Settings → Privacy & data → Network connections lists them by
purpose and host, cloud and on this PC alike, with a filter and Clear.
Settings → About → Report a problem builds a diagnostic .zip (system, an
allow-listed settings summary, connections, errors, 3 days of `info`+ logs
scrubbed), shows it before it's saved through the Save dialog, uploads
nothing, then offers GitHub's new-issue page. Unit-tested (the recorder on
a real local request, the store's folding and roll-over, hosts without
paths or queries, a bundle seeded with fake keys and personal details
holding none of them) and e2e-tested (`e2e/network-receipts.spec.js`: a
cloud call and one on this PC through fake servers, the filter, light and
dark, Clear; the preview, Save, the zip searched, the issue page). **Not
yet tried**: the list over a real day of use (a real cloud provider, the
updater's real check against GitHub, a model download), the real Save
dialog and Show in folder.

**AI cleanup that sounds right with no set-up** (2026-10-24,
`ai/cleanup_styles.rs`, ai-models.md): **styles by app** — Email formal, Work
and Personal chat casual, Documents formal, AI prompts literal (no
"please", nothing rephrased, never answered), Terminal commands as said,
everything else in the speaker's own words — each kind a cleanup profile
with built-in apps (programs, and web apps by their tab's title; coding
CLIs and shells as `pasting/app_kind.rs` reads them), editable in Settings →
Language Models → Dictation cleanup → Styles by app with You say / Yap
types examples; the person's own app rules win as before; filler alone
asks no model. **Offered in first setup** (on this PC when the graphics
card runs the recommended model, else the cloud with what goes there;
nothing on until chosen) and once, as a tip, to someone with cleanup off
on a PC it fits. Existing installs keep cleanup as they had it; someone with
their own body keeps it everywhere (styles off for them). Model
evaluations (`--styles`, docs/model-evals.md "Styles by app"): 22 cases
per kind and tone passing 55 % → 100 % on Qwen3.5-4B, 62 % → 100 % on
Groq's gpt-oss-20b (8 cases), 55 % → 73 % on Qwen2.5-1.5B, at no time a
dictation would notice (+7 ms on the card). Unit-tested (category detection, resolution, lines,
fitting, the offer), e2e-tested (`cleanup-styles.spec.js`,
`cleanup-offer.spec.js`), robot tests written (`cleanup-styles.real.js`:
a web mail page, a chat page, a plain page, PowerShell, the Claude Code
stand-in). Not yet tried with a real voice in real Outlook, Slack or
WhatsApp, nor the 1.5B on a PC without a graphics card day to day.

**Dictation polish** (2026-10-25, Wispr Flow's by behaviour): **Paste last
transcript** and **Copy last transcript** on shortcuts of their own
(Settings → Shortcuts, unbound by default; Copy last also in the tray; the
copy out of Win+V history, `pasting/last.rs`); **"Taking longer than
usual…"** on the bar when the transcription or the AI cleanup runs well
past this PC's usual time — measured (`stt_speed`'s timings, this
session's dictations and cleanups), 2.5× + 1 s, never before 4 s — with
**Skip cleanup** for a slow cleanup, local or cloud (`dictation/slow.rs`);
**the 15-minute limit** warned a minute before on the bar, then stopped,
transcribed and pasted back into its own window (or left on the
clipboard) with **Recover text** (`dictation/cap.rs`; before, the buffer
stopped growing with only a log line); **a level meter that moves for
whispers** (`dictation/meter.rs`: dB over the mic's noise, gently
compressed); **"Hold on, still transcribing…"** when the shortcut is
pressed during a transcription. Tested: unit tests (the meter's scale and
floor, the usual time and its threshold, the limit's crossings and words,
the shortcuts' clash rules); e2e `dictation-polish.spec.js` (8: the
Settings rows, both shortcuts through `e2e_shortcut`, a slowed stub
transcription, a slowed fake cleanup skipped from the bar, an 8 s limit
with Recover text, whisper and full-scale tones on the waveform, Hold on);
robot (`last-transcript.real.js`: Paste last into the test pad and an Edge
text box, Copy last's private clipboard; `dictation-extras.real.js`: a 2 s
limit pasting a spoken dictation, Recover text's clipboard). Not yet tried
by hand: the shortcuts on a real keyboard, a real slow model (a big local
model's first cleanup, a slow network) and a real 15-minute dictation; how
the whisper meter looks with a real quiet voice on different mics.

**Trusting a dictation** (2026-10-22): **the speech engine's device on each
PC** (`dictation/stt_speed.rs`: a one-time background benchmark per model at three
dictation lengths, Settings → Speech-to-Text → Speed "Runs on: graphics card
— fastest for your dictations on this PC (150 ms vs 280 ms)" with Automatic
/ Graphics card / Processor; everyday lengths decide: the graphics card
unless the processor takes ≥ 20 % less time at both 3 and 11 s, weighed by
the person's own dictation lengths for the line). **Parakeet on DirectML
made ~3× faster** (vendored transcribe-rs, `vendor/transcribe-rs/YAP.md`):
the decoder loop and preprocessor on the CPU, the encoder's input padded to
the one length it planned for at load. On this PC (RTX 5070 Ti, Ryzen 7
9800X3D, the processor ~35–40 % busy with the owner's apps; release build,
the call a dictation makes, devices interleaved, medians of 7 warm runs,
each a new length), at 2.9 / 11.1 / 35.6 s of speech: V3 on DirectML
441 / 630 / 1,244 ms before → 133 / 158 / 578 ms after, the processor
140 / 424 / 1,743 ms; V2 128 / 161 / 564 ms vs 125 / 421 / 1,749 ms. The
words are the same (V3 identical on every clip; V2's 11-second clip varies
a comma between runs, as it did before and on the processor). The first
dictation after a load: ~0.17 s on DirectML (was ~0.8 s, and ~2.6 s for
the process's first) — the load takes ~0.8 s longer for it (~3.3 s, the
encoder planning at load). Automatic picks the graphics card for both;
Whisper large-v3 (q5_0) stays on Vulkan (345 / 805 / 2,279 ms warm; on the
processor one 11-second dictation took 16 minutes, so it isn't tried beside
a graphics card that's quick warm). Processor load slows the processor path
only: the benchmark waits for a processor under 50 % busy, and a narrow win
never moves a model off the graphics card. **"Use what
I said"** (`pasting/as_said.rs`: the shortcut, the bar's offer, the tray's paste as
said), checked by the robot in the test pad, Notepad, an Edge box, a chat
app and its contenteditable (swapped) and in a terminal, after an edit and
with another window in front (refused, copied). **What the AI changed, and
undo it later** (2026-10-23): Home's "AI edited · N changes" on a dictation
whose words cleanup changed opens into a word diff (`lib/core/wordDiff.js`,
history's `said`), with **Paste what I said** (the latest dictation swapped
in its app within 2 minutes, its window brought back; any other copied) —
the diff unit-tested (`word-diff.spec.js`), the Home UI e2e-tested
(`home-ai-edits.spec.js`, light and dark), the swap from Home a robot test
written (`as-said.real.js`), not yet run by the coordinator nor tried by a
person (the window brought back to the front from Home is the part only a
person's click shows). **Rewriting the last dictation** (2026-10-23): the
rewrite shortcut with nothing selected, within a minute of a dictation in
that window, rewrites it in place (the paste still ending at the cursor,
else nothing pasted and said) — the rules unit-tested (`as_said::
rewritable`, `keep_spaces`), robot tests written (`rewrite-last.real.js`:
the test pad, an Edge box, a contenteditable, typed since and the cursor
moved refused, a selection or another window as before, a terminal never),
not yet run; not yet tried with a real voice and AI model. **Saved
transforms** (2026-10-24, `dictation/transforms.rs`): one-key rewrites without
speaking — Polish, Shorter, Fix grammar only, Translate to a language you
pick, Make it a prompt, and your own — on the selection or (nothing
selected, the rewrite shortcut's rules) the last dictation, never in a
terminal; their own shortcuts (12 input-hook slots), the tray's
Transforms menu, the bar's "Polish ✓ · Undo". The rules (slots and
clashes, what a transform works on, the model's preamble stripped)
unit-tested, the Settings list and every outcome e2e-tested
(`transforms.spec.js`, through test hooks), the built-ins' prompts
measured on the 4B and Groq's gpt-oss-20b (docs/model-evals.md "Saved
transforms"); robot tests written (`transforms.real.js`: the test pad, an
Edge box and a contenteditable, Translate on the last dictation, a
terminal refused, the bar's Undo), not yet run; not yet tried by a person
(a real shortcut press, the tray's menu bringing the app back). The spoken
"undo that" / "revert" was left out: "revert" alone is a real message, and
"undo that" reads as "delete it" in Windows' own voice commands. **My
languages** (2026-10-25, `dictation/languages.rs`; Settings → Dictation →
My languages): the languages you speak picked from a search, which
installed speech model covers which and the one to recommend (the model is
never changed for you), the main shortcut on Auto or one language,
language shortcuts (a key that dictates in a language, or pastes it
translated by the AI: on this PC or in the cloud), cleanup told the
dictation's language, and the nudge ("You spoke French · Add it to My
languages?"). Whisper picks the likeliest of your languages from its own
detection (74 / 56 ms for large-v3 on an RTX 5070 Ti: the work it already
did when not given a language); Parakeet V3 and SenseVoice recognise the
language by themselves, Canary and Cohere are told your first language,
Parakeet V2 (English only) is unchanged — only Whisper says what it heard,
so the nudge works with Whisper models alone. The rules (plans, slots,
coverage, the nudge's limits, the language line, the leading filler)
unit-tested; Settings, a translating shortcut through the fake AI (and
through the stub speech engine), the AI failing and the nudge in the main
window and on the bar e2e-tested (`languages.spec.js`); the prompts measured
on the 4B (cleanup 27/27 after vs 21/27 before, translation 12/12), the 1.5B
(0/27 → 15/27: it still writes German in English) and Groq's gpt-oss-20b
(13/13, docs/model-evals.md "My languages"); robot tests run with Whisper
large-v3 on English speech (`languages.real.js`: heard as English with
French alone picked, the nudge offered, English added, a shortcut fixed to
English with no detection asked). Not yet tried by a person: a real
non-English voice (Windows' text-to-speech here speaks only English), a real
key press of a language shortcut, Parakeet V3 or Canary with several
languages. **A day of dictation audio**
(`dictation/recent_audio.rs`): failed dictations listed, Run again (any installed
model) and Play in Home, a crash mid-dictation recovered at the next start —
e2e-tested on the stub engine (`dictation-trust.spec.js`), Run again on real
speech not yet tried by a person.

**Transcription is REAL and GPU-accelerated** (no longer a stub in `engines`
builds), and the dictation pipeline around it is complete: multi-engine STT
(Whisper/Vulkan + ONNX/DirectML), the 14-model registry + manager, recording modes
(push-to-talk with double-tap hands-free and coaching tips, 2026-10-17), a dead-mic
watchdog (reopen, then "No sound from <mic>"), **microphone management**
(2026-10-25, `dictation/mics.rs`: an order of microphones — the first
connected, then Windows' default —, "Contoso H2 connected · Use it for
dictation?" for a new one, "Using …" when the order switches by itself, and a
microphone unplugged mid-dictation ending it, its words transcribed, pasted
only into their own window and kept, the next dictation on the next one; a
meeting moving to the next microphone with a marker — unit-tested, e2e-tested
with made-up devices (`mics.spec.js`, `meeting-mic.spec.js`); Core Audio's
real notifications, a real USB headset pulled out mid-dictation and a
Bluetooth headset in the order are not yet tried by a person),
language/translate, **AI cleanup** (BYO key or local sidecar — Qwen3.5 4B on a capable
graphics card, else Qwen2.5 1.5B, 2026-10-18, chosen and measured with the model
evaluations; cleanup ~0.34 s a dictation on the 4B, ~0.13 s on the 1.5B, on an RTX
5070 Ti) with per-app routing +
named profiles + per-profile model choice, **Commands** (the rewrite shortcut +
the agent's name in a dictation; "Voice Agent" until 2026-10-18), the **Voice
Assistant** (`chat/assistant.rs`, 2026-10-18: its own shortcut, ask Chat out loud
from any app and hear the answer, a bar capsule and card, follow-ups,
barge-in, off (and Yap silent) during a meeting recording or a call; the rules unit-tested and the whole
flow e2e-tested from a staged transcript with the main window hidden and
in front — not yet asked with a real mic and heard on speakers; since
2026-10-23 a pause ends a question, `dictation/pause.rs`: unit-tested, e2e-tested
with tones over a room's hiss through the real key path, and a robot test
written with Windows TTS speech and Parakeet's live preview — not yet
tried with a real voice in a real room), **opening
apps when asked** (`apps.rs`, 2026-10-18: "open Teams" in Chat or out loud,
from the Start menu's list, with no model needed; nicknames, apps switched
off, the `open_app`/`list_apps` tools; matching and the request patterns
unit-tested, this PC's real list indexed read-only, the flows e2e-tested
against a stand-in list — never launching anything; same day, games: a
desktop app opens through its Start menu shortcut (World of Warcraft's
launcher had lost its `--productcode`), numbers and sound-alikes ("boulders
gate three"), "play X", "open it again", Yap answering "yes" / "the second
one" to its own questions, a yes to the model's offer as consent, lighter
tool steps for Groq's rate limit, no "Opening…" from a model that can't —
checked with real shortcut launches in a temp folder, not yet with a real
game), **spoken replies** (`voice/tts.rs`, 2026-10-18: Chat's answers read
aloud in Windows' voices — the Voice replies toggle, ▶ on every answer,
"only when I asked by voice", hands-free talk-back, barge-in; text prep, the
queue and the decisions unit-tested, synthesis checked silently on this PC's
voices, the whole loop e2e-tested in test mode; not yet heard on speakers
by a person) with **Kokoro** as the natural local voice and the default
(2026-10-18: Kokoro-82M v1.0 fp32 on Yap's `ort`, CPU; measured on this PC
— Ryzen 7 9800X3D, release build, 4 threads: ~0.6 s to load + 0.17 s for a
lexicon, a 5.4 s sentence in 0.71 s, the research answer at RTF 0.12 (0.18
at 2 threads, 0.31 at 1), its first 6 s piece ready in 0.75 s, a stop ends
a sentence being made within ~10 ms; Parakeet round trip of the research
answer after the text pass: 0 misread words for af_bella / bf_emma /
bm_george (before it, "£12,400" lost its "pounds"); the download, the
Settings rows and the fallback e2e-tested against a local server, the real
download and msiexec unpack checked once; not yet heard on speakers by a
person) and **ElevenLabs** as an optional cloud voice on the person's own
key (2026-10-22, `voice/tts/elevenlabs.rs` + `voice/tts/cloud.rs`: streamed PCM per
sentence, the account's voices with ▶ samples, Flash v2.5 by default,
usage; any failure hands the answer to the local voice with one notice, or
a notice and silence without one; request building, PCM, errors and the
fallback rules unit-tested against an in-process stand-in, Settings and
the reply/fallback flows e2e-tested against `fake-elevenlabs.js`; first
audio measured on this PC with `first_audio_latency`: Kokoro af_bella
1.4 s cold / ~0.43 s warm for the preview sentence, ElevenLabs' path
< 1 ms of Yap's own on top of the service's answer; not yet tried with a
real key), **Read aloud, anywhere** (`voice/read_aloud.rs` + `voice/tts/marks.rs`,
2026-10-22: the selection in any app, a note or a Chat answer read in that
voice with a karaoke reading card on the Yap bar; word timing from Windows'
own word cues, ElevenLabs' `with-timestamps` alignment, else from Kokoro's
audio — measured silently against Parakeet's word times (itself −21 ms from
Windows' real cues): Kokoro's karaoke a median 100 ms off, 71 % of words
within 150 ms;
the selection read for real by the robot in the test pad, Notepad and an
Edge page, a terminal never by Ctrl+C; the card, karaoke, ⏸ ⏭ ⏮, speed,
Esc, a 30,000-character text made a sentence ahead, e2e-tested; not yet
heard on speakers by a person, nor with a real ElevenLabs key or a PDF
viewer), combo hotkeys, the audio pre-roll (anti first-word clipping), **live
streaming partials** (sliding-window, on by default, word-paced overlay reveal —
validated live 2026-07-10), transcription history + stats, cleanup presets,
**other apps silenced or paused while you dictate** (`dictation/mute.rs` + `dictation/media_pause.rs`,
2026-10-17: per app after the start chime, never calls, back to exactly
their level after the stop chime; the rules unit-tested, the volume drop and
restore and the pause and resume checked by the robot on an Edge page it
opened),
the dictionary's **snippets**, the **"press enter" voice command** and a Whisper
vocabulary ranked by use (2026-10-17: unit-tested, the Dictionary/Settings UI
e2e-tested, not yet tried with real speech), **terminal & coding-CLI pasting**
and **no full stop on short chat messages** (`pasting/app_kind.rs`, 2026-10-17:
classification and text rules unit-tested, checked read-only against this PC's
real windows, the Settings switches e2e-tested; not yet dictated into a real
terminal or chat app), **word suggestions from your corrections** (`dictionary/learn.rs`,
2026-10-17: fix a word Yap typed and it offers to add it to the dictionary;
the same day aligned with Wispr Flow's measured mechanics — a 60 s watch of
the whole field that survives switching apps, the drift gate, the last good
edit judged however the watch ends (sent, Enter, time), 4-word corrections,
no contractions, removed words never offered again; the rules and the
watch's ends unit-tested, the prompt/Add/toggle/bar card, a fix-then-send
and a removed word e2e-tested through staged edits and field reads, the UIA
reads and Windows' spell checker by `--ignored` tests on hidden windows —
not yet tried in real apps),
**the Dictionary view for hundreds of entries** (`views/dictionary/`,
2026-10-25: search with Ctrl+F, stars (starred words lead the Whisper
prompt) and four remembered sorts, select several with a bar for Delete…
with Undo, ≈ and Export…, Import… / Export… as CSV or text with a preview
of what's added, already there, spelled differently and left out; the
parsing, sorting and Undo rules unit-tested, the view e2e-tested in light
and dark with 2,000 entries measured quick in the stub debug build; not yet
tried with a file exported from another app or by hand in Excel),
the Groq usage meter, and the installer + auto-updater (background checks + download
while Yap runs, "Restart to update" from toast/tray/About/Windows notification —
`updates.rs`, 2026-10-05) + portable mode + release CI (since 2026-10-25 every
release also carries `SHA256SUMS.txt` and a build provenance attestation for the
installer — actionlint-checked and dry-run against v0.1.0, not yet through a real
release or nightly run; [updates-and-release.md](./updates-and-release.md)).
The **Yap bar** (`bar.rs`, 2026-10-05, after Wispr Flow's Flow Bar) is the
always-there surface: a pill above the taskbar on the cursor's monitor,
clickable only where it draws, never focused, hidden over fullscreen apps,
with Dictate / Meeting notes on hover and Yap's notices as cards (call prompts
as Wispr's "Meeting detected" card) — e2e-tested with real-window checks of
its ex-styles and hit-testing, and real mouse clicks on its cards (✕, buttons,
a call card's ^ menu, a click as a card appears) robot-tested in
`e2e-real/bar.real.js` (2026-10-21); multi-monitor moves, DPI mixes and games
still want a hands-on pass.
On top of that, the main window is now a full **ControlPanel** (Home dictation feed
w/ Ctrl+K search, Chat, Notes, Upload, Dictionary, Settings as an always-mounted
modal, app-wide toasts): local audio-**file** transcription (Upload — `dictation/media.rs`
Symphonia decode + chunking; since 2026-10-24 the job, like a speech model's
download, outlives its view: leave mid-job and come back to its progress,
Cancel and result — e2e `upload.spec.js`), an **AI Notepad** (`notes.rs` — folders/actions/
participants/transcripts, an Actions engine, ActionPicker/ActionManager, attendee +
folder management, markdown export, an embedded per-note chat), a **meeting
recorder** (`meetings/meeting.rs` — mic + WASAPI loopback → You/Them transcript, cut in
pauses, speaker echo flagged, corrected with the dictionary like a dictation;
hotkey dictations kept out of "You" with a "You dictated here" marker; "Them"
follows Windows' default output device, both sides kept on one clock; a side
quiet for 3 minutes while the other talks gets a warning — all e2e-tested in
`meeting-audio.spec.js`, the real device switch only through a simulation
hook so far; `meetings/meeting_summary.rs` — rolling ~10-minute digests
while it records, then **End meeting & summarise** → an **action plan** with a
section per person + Decisions / Open questions / Unassigned, every AI call
within an 8k local context whatever the length; e2e-tested with a two-hour
meeting against a fake AI, and once with real speech through Parakeet
2026-10-05) with **call detection** (`calls/meeting_detect.rs`
— Teams/Zoom/Meet/Slack/Discord/Webex… taking the mic → "Record notes?", the
call ending → "Stop and summarise?"; per-app choice (work apps on, Discord &
co. off, "Don't ask for X" on the prompt), a pop-up or quiet style, a 30 s
fade, and a tray item; e2e-tested via a simulation hook, the
registry signal checked read-only on the test PC, no real call yet),
**guard rails** (`calls/meeting_guard.rs` + `calls/capture.rs`, Wispr Flow's Notetaker
settings: meeting windows hidden from screen capture while recording, a
2-hour maximum length with a "Keep going" warning, an optional auto-stop
when a recorded call ends, and the Win+Alt+M meeting shortcut;
e2e-tested, the capture flag also checked once with a desktop capture of the
overlay, a real screen share not yet; 2026-10-17: a meeting quiet for 10
minutes ends after an "Ending notes in 30 s…" countdown, and "Still in
your meeting?" check-ins after a calendar meeting's end and an hour in —
rules unit-tested, e2e-tested in seconds through the notepad, the bar and
a local iCal feed, not yet in a real meeting) and the **meeting notepad**
(`meetings/notepad.rs` + `Notepad.svelte`, Phase 8, 2026-10-05: a window docked to the
right of the screen when a meeting starts, in Wispr Flow's measured layout —
My thoughts synced with Notes, the live transcript in grouped bubbles with
paused/dictated dividers, search and copy, the summary written by Rust in
steps with a Retry, the Ask sheet ("What did I miss?" since you last looked,
questions, 👍 👎 kept locally; one-tap helpers, 2026-10-17: Questions to ask
while recording, a plain-text Follow-up email and Gaps and risks after, also
on a meeting note in Notes — e2e-tested against the fake AI, not yet tried
with a real model), the AI meeting title, "Started by mistake?"
Keep/Discard (on the Yap bar when no window is on screen), the ⋯ menu,
local Share (copy, .md, email), delete, an editable consent message, split
screen with the call now or when joining (a glass preview on hover); every
stop, Yap's own included, writes the action plan in Rust without bringing up
the main window; e2e-tested in `notepad.spec.js`, split screen and its
preview unit-tested only), **meeting audio kept on disk** (`meetings/meeting_audio.rs`,
2026-10-23: both sides as lossless FLAC from Yap's own small encoder,
written as they're captured, dictations blanked, 14 days by default — a
crash loses nothing: the next start offers to transcribe what never became
lines, then summarises; "Transcribe again" on the current model keeps the
notes, the summary and names on matching lines; ▶ plays a line; Settings →
Meetings keeps, ages and deletes it; unit-tested, e2e-tested with a killed
and restarted test instance, the sizes measured on Windows TTS speech
— not yet with a real call, a power cut, or hours of audio), **meeting notes
you can fix** (`notes/note_edits.rs`, `notes/tasks.rs`, 2026-10-24: a transcript line's
words fixed in the notepad or the Notes view — marked edited, what Yap heard
kept, ▶ still plays it, kept through Transcribe again, offered to the
dictionary by "Suggest words from your corrections"; the summary edited in
place and never written over unasked — regenerating asks, a resumed
meeting's end keeps it, "New since this summary · Update"; the action plan's
tasks ticked in place, kept through a new summary, and a **Tasks** view
across meetings by meeting or person with "Said at" back to the line; read
only over the local API and MCP; unit-tested and e2e-tested in
`notes-edit.spec.js` against the stub engine and a fake model — not yet by
hand in a real meeting, nor with a real model's plans in their many
layouts),
and since
2026-10-24 a **transcript you can trust**: a chunk that fails to transcribe
is a marker with Try again, never silence, the person told once
(`meetings/meeting.rs`); one recorder however many starts at once, the note being
recorded never deleted (the local API included); the warm engine lent in
turns, so a dictation during a meeting's chunk waits for it instead of
loading a second copy (`pipeline::EngineCell`); and the **refined
transcript** when a meeting ends (`meetings/meeting_refine.rs`: when chunks failed
or a more accurate model is installed for its language, or always; the
summary first unless the transcript has holes, then "Update summary"; low
priority, cancellable) — unit-tested (the races and the engine lease fail
on the old code), e2e-tested with a forced chunk failure, the three
settings, a dictation mid-refine and two starts at once; the real-speech
robot test is written, not yet run; not yet tried in a real meeting,
and an **AI Chat** surface (`chat/chats.rs` + eager
keyword-RAG over notes, plus a **tool-calling agent loop** in `chat/tools.rs` — nine tools,
including safe partial note edits (search → exact-once replace, `chat/note_text.rs`),
≤20-step loop, gated to cloud or ≥4B local models). An **MCP server** (`integrations/mcp.rs`,
`yap.exe mcp`) lets Claude, ChatGPT desktop/Codex, Gemini CLI, Cursor, Claude Code,
VS Code and Windsurf read meetings and notes (never dictations) through the local
API, added in one click from Settings → AI apps (MCP) (`integrations/mcp_clients.rs`); tested in-process,
against the real binary and end to end (2026-10-05), not yet with a real AI app.
The **calendar** (`calendar.rs` + `calendar/`, Wispr Flow's, local-first,
2026-10-05): Google Calendar in one click (read-only, PKCE, loopback) or any
private iCal link (Outlook's published calendar, Google's secret address,
iCloud…) under Settings → **Connectors**; a **Meetings** view (today, the next
7 days, Conflict/Maybe, Join + Start, past meeting notes with search, an Ask
bar into Chat); a reminder card before each meeting, on the Yap bar while the
main window isn't focused (**Join & take notes**, switch notes for
back-to-back meetings); every recording during a meeting
takes its name, attendees and invite, and call detection doesn't ask twice.
Recurrence and time zones (incl. Windows zone names) unit-tested; e2e-tested
against a local iCal server and a local fake of Google. Google needs the
OAuth client set up in Google Cloud + the two repository secrets
([`docs/calendar.md`](../calendar.md)); not yet tried against a real
calendar.
Every JSON store now writes
atomically with corrupt-file quarantine, and **a write that fails is said**
(2026-10-24, `shell/saving.rs`: the data kept in memory and saved with the next
change; "Couldn't save your note: the disk is full." with Try again where
the person made the change, one notice for a save that failed by itself;
unit-tested on locked and read-only files, e2e-tested with staged
failures in `save-errors.spec.js` — a real full disk not tried); the temp
files a crash mid-write leaves are swept at start-up, Yap's own stale ones
only (unit-tested); and every setting changed outside Settings (the Chat
picker, the voice's volume, Read aloud's speed, the on-device context,
call detection's choices) changes in one step (`config::update` /
`try_update`), never a load → change → save that could lose a change saved
meanwhile (unit-tested, with a lint-style test). The default (no-feature) build still ships
the stub for fast `cargo check`. **Optional accounts** (`auth.rs` + `cloud/`): email
codes and Google/GitHub/Discord sign-in, sign-out, delete-account — tested end
to end locally (2026-10-01, `wrangler dev` + a mock provider). **Live since
2026-10-02** at `https://auth.contextmirror.com` (contextmirror.com's DNS moved to
Cloudflare; D1 `yap-auth`, WEUR): Google (Cloud project `yap-accounts`, published;
branding verified 2026-10-04, so its chooser says "continue to Yap" with the logo)
and GitHub (OAuth app "Yap") verified with real sign-ins; email codes send via
Resend from `mail.contextmirror.com`. Discord (application "Yap") is live and
verified with a real sign-in (2026-10-04); Microsoft was dropped the same day. New
accounts need a verified email (`databaseHooks` in `cloud/src/auth.ts`). **Sign in
with your phone** (QR code, device flow) verified end to end in the installed
nightly `0.1.1-nightly.107` (2026-10-04), along with the "Where you're signed in"
device list. Known quirk: Better Auth links accounts by exact email, so
`…@gmail.com` and `…@googlemail.com` (same Gmail inbox) are separate accounts.
**Real-app robot tests** (`npm run test:real`, `shell/realtest.rs` + `e2e-real/`,
2026-10-17): paste, smart spacing, snippets, "press enter", Paste last transcript,
the clipboard, terminals (PowerShell, a Claude Code stand-in; Git Bash and a
VS Code terminal stand-in since 2026-10-21), chat apps, the mic
path, the dead-mic watchdog, learning words from corrections and real Parakeet
STT, all checked against real windows by Claude. The three gaps its first run
found are fixed (2026-10-17): a WinForms text box gives UI Automation no caret,
so mid-text dictations were fitted as if at the end — Yap now asks the edit
control (`EM_GETSEL`); the fuzzy pass swallowed a short word before a term
("Deploy it to Kubernetes" → "Deploy it Kubernetes"); and Edge/Chrome report an
empty text box's text as its label or U+FFFC (a web chat message kept its full
stop and got a trailing space) and their ranges ran past the box into the page
— reads now stay in the field and an empty one reads as empty.
**Pasting everywhere** (2026-10-21): each terminal gets its own paste keys
(Shift+Insert in Git Bash's mintty, PuTTY/KiTTY, MobaXterm; Ctrl+Shift+V in
Alacritty, WezTerm, Tabby, Hyper, Rio; Ctrl+V where it pastes), VS Code's
integrated terminal (and Cursor's, Windsurf's…) counts as a terminal when
it has the focus (xterm.js's input, over UI Automation), and a paste a
Win32 text box never got is typed instead, once (verify-after-paste: its
length and selection before and after). The mapping, the label parsing and
the verdict unit-tested, the edit-control reads by the `--ignored` hidden-
window test, and robot-tested: Shift+Insert into the real Git Bash
(`gitbash.real.js`), an Edge stand-in for VS Code's terminal
(`editor-terminal.real.js`: it showed Chromium folds the label's line
breaks, now handled), the pad's "nopaste" box typed into once, and no
other paste in the full run typed twice. VS Code itself isn't installed
here, and Alacritty, WezTerm, PuTTY & co. are untried.
**Names as written on screen** (`dictionary/screen_terms.rs`, 2026-10-22;
[dictionary.md](./dictionary.md)): the target window's visible text read
over UI Automation at record start (in the background, never waited for),
its names and terms used for that dictation only — a strict near-miss pass
for every model and cleanup's "Spell these names as written" line, not the
Whisper prompt (measured: it made Whisper hear "row shell" as "Rochelle").
Extraction, the guards (≈ 50 everyday sentences against a busy screen's
~60 terms with this PC's spell checker), the session and the cleanup line
unit-tested; robot-tested on an Edge page and the test pad, with the
stand-in cleanup model, a read delayed 8 s (the paste doesn't wait), and
real speech (`screen-names-stt.real.js`: Parakeet 4 → 5 of 7 names right,
Whisper large-v3 6 → 7, no everyday sentence changed). The read: 18–107 ms,
median 39, over the robot suite's 94 reads (debug build, a busy PC), ~50–100
ms on a 3,000-paragraph Edge
page and a 5,000-line code view (a VS Code stand-in: VS Code isn't
installed here), 30–80 ms on the WinForms pad. Gaps: a name a model spells
far from the screen ("Shivon" for Siobhan with Parakeet) isn't fixed, nor a
name heard as everyday words or in lower case; the walk over a big native
app's tree is bounded, not timed in a real one (Outlook, Teams); WebView2
apps' content (another process) isn't read, as for the cursor's context.
**Dictation extras** (2026-10-17): a paste into an app running as
administrator is spotted (its integrity level) and left on the clipboard
with "Your text is copied: press Ctrl+V" (`pasting/elevated.rs`, `notices/alert.rs`); text
boxes read through their labels (LabeledBy), the text field a focused part
is in, and the legacy read-only bit (`pasting/selection.rs`); ✕ / ✓ on the
dictation capsule (off by default, the same day: Settings → Yap bar turns
them on when pointed at, or always; never while push-to-talk is held) and
"Dictation cancelled · Undo" (10 s); Windows'
microphone privacy switches named with Open settings (`dictation/mic_privacy.rs`).
Unit-tested, e2e-tested (`bar.spec.js`, `dictation-alerts.spec.js`,
`no-mic.spec.js`) and robot-tested (`dictation-extras.real.js`, the label
case in `browser.real.js`); a real elevated window and a privacy switch
really flipped in Windows are still untried (a UAC prompt; the test
PC's settings).
**Calmer notices** (2026-10-17, after Wispr Flow's rules by behaviour):
one notice policy in Rust and on the page (`notices/notice.rs`,
`ui/noticePolicy.js`): one place per notice (the window while it's focused,
else the bar), one card on the bar and one notice + one answer in a window
at a time, by priority; tips and notices held while dictating; stale ones
dropped; repeats within a cooldown dropped; call-prompt fatigue ("Stop
asking about calls?"); "Show tips"; a dictation's errors on the overlay
only; 19 success toasts that repeated what the screen showed removed.
Unit-tested and e2e-tested (`notices.spec.js` + the updated specs); real
use (a call while the window sits on a second monitor, the bar off with
Windows notifications, Focus Assist) not yet tried.

**Looking things up on the web** (`web.rs`, 2026-10-18, off by default):
Chat and the Voice Assistant search (Exa or Tavily with the person's free
key, else Wikipedia and game wikis with none), read pages on the PC behind
an SSRF guard, cite `[1]` with a Sources row, and never act (apps, note
writes, clipboard) once web content is in a turn; small local models get
a fixed search-then-answer pipeline. Unit-tested (recorded Exa/Tavily/wiki
JSON, the URL guard, the safety rule through the real loop), e2e-tested
against a local fake web (`web-search.spec.js`), and the keyless lookup
checked once for real (Wikipedia, bg3.wiki); not yet tried with a real Exa
or Tavily key. **The AI's own search** (`web/native.rs`, 2026-10-18): cloud
models search with their provider's own search as a sub-call, local ones
with Yap's. Parsers unit-tested on each provider's documented shape (Groq's
recorded), the Groq and Gemini paths and the fallback e2e-tested against
the fake AI; for real, through the eval harness on the maintainer's keys: Groq
gpt-oss-20b passed all 4 web scenarios (its search sub-call 3.9–4.8k
tokens, ~2.4 s; a searched answer ~8k tokens over 3 calls, so the free
plan's 8,000 a minute makes the next answer wait), Gemini 3.5 Flash on a
free key has no Google Search grounding ("check your plan and billing")
and fell back to bg3.wiki correctly; Gemini 2.5 Flash/Flash-Lite are
closed to new keys. Anthropic, OpenAI and OpenRouter searches untried
with real keys; Gemini grounding untried on a billed key. **Units where the
person is**: web pages' temperatures (2026-10-23), speeds and distances
(2026-10-25, `ai/about_you/units.rs`: mph and miles for the UK and the US,
km/h and km elsewhere) reach the model in the person's units first, with a
line saying so — unit-tested, and model-evaluated on stand-in pages
(Qwen3.5-4B 35/35 on the wind and distance scenarios, from 10/35; Groq
gpt-oss-20b 2/2; Qwen3.5-2B's fixed pipeline 14/14; model-evals.md); not yet
checked against real weather sites through Exa or Tavily. A Voice Assistant
already speaking a step's opening words that turn out to come before a tool
call now stops, and says the answer from its start (2026-10-25, e2e-tested).

**Memory** (`memory.rs`, 2026-10-22; [memory.md](./memory.md)): what Yap
remembers about the person, learned from quiet chats (and, opt-in,
meetings) by a Chat model that can call tools, sent with questions,
changed by "Tell Yap what to change", imports, "Remember that…" and Settings
→ Memory, the names of people and projects fed to dictation. Unit-tested
(the rules, guards, prompts, export, the names pass over ~200 everyday
sentences with this PC's spell checker, the memory tools through the real
loop), e2e-tested against the fake model (Settings → Memory, Tell, import,
the learned toast, no AI, a small model), evaluated on real models
(Qwen3.5-4B 42/42, then 43/44 with About you, on the GPU, Groq gpt-oss-20b 8/8, Qwen2.5-1.5B on the CPU
— model-evals.md "Memory"), and the dictation side robot-tested in the test
pad. Not yet used day to day: whether what a real week of chats teaches it
is worth keeping, and the 5-minute scheduler and the quit/restart path,
only unit-level.

**Snip to Chat** (2026-10-23, [snip.md](./snip.md)): Win+Alt+S, ✂ on the bar,
the tray, Chat's + or saying "snip"; every monitor captured at once on
Windows.Graphics.Capture (FP16 ÷ the SDR white level on an HDR monitor), a
native overlay per monitor, the box saved as a PNG and put in Chat's box (the
conversation Chat shows, else a new one; never sent by itself), the three
one-tap prompts, Enter on an empty box for "What's this?", the model told when
it can't read images with a one-click switch, screenshots kept 30 days by
default. **Measured** (`snip-dump`, an optimised build, two 1440p monitors,
one HDR at 240 nits): every monitor captured in ~26 ms, the overlay on screen
**~50–60 ms after the request** (87–91 ms in a debug build); a full monitor as
PNG in 6–7 ms. HDR: known swatches from a GDI window read exactly (worst error
0) through the FP16 path on the HDR monitor. Unit-tested (the HDR round trip at
five white levels, crop maths at mixed DPI, request shapes per provider, the
vision rules, retention, the voice phrases) and e2e-tested over fake monitors
(`e2e/snip.spec.js`, 11 tests). **Not yet tried**: the interactive overlay by
hand (a real drag, the focus, Esc through the hook) — `e2e-real/snip.real.js`
is written, not yet run; an HDR-aware app's swatches (Edge) on the HDR
monitor; a real Anthropic / OpenAI / Gemini / OpenRouter request with an
image (shapes from their docs; the fake's checked).

**Snip to Chat, phase 2** (2026-10-25, [snip.md](./snip.md)): a click or
Enter takes the whole monitor, Space or Alt picks a window (its DWM frame), a
box can cross monitors (put together from each frame); Ctrl as you let go
copies (PNG + DIB, out of Win+V history), Shift saves (the dialog, the folder
used last), and a chat screenshot's menu does both; pictures pasted or
dropped into Chat go in like a snip (Windows' decoders); "Send its text
instead" sends a screenshot's text, read by Windows' OCR on this PC, to a
model that can't read images; "Reads images" in the model picker and
Settings' Chat list. **Tested**: unit (the selection state machine, the
geometry across monitors, the DIB, the decoders, and Windows' **real OCR on
a fixture** on the maintainer's PC), e2e (`e2e/snip.spec.js`, now 19 tests
over fake monitors and windows: every action above, a synthetic paste and a
Tauri drop event, a fake OCR text, the tags), and **robot-tested on the
maintainer's PC** (`e2e-real/snip.real.js`, 5 of 5, 25 Oct — which also
settles phase 1's "not yet run" above: real drags over Edge swatches on
both monitors, the HDR one included, within ±2, the overlay up in 92–101
ms in a debug build, a real Esc): a real click and Enter for the whole
monitor, Space and a real click picking the Edge window exactly, Ctrl held
on a real release putting the very PNG, a DIB and the privacy formats on
the real clipboard, and a real Ctrl+V pasting it into Chat pixel for pixel.
**Not yet tried**: a real drop from File Explorer (the e2e test sends
Tauri's own event), the Save dialog by hand, a box across monitors on real
mixed-DPI monitors (the maintainer's two are both 100 %), OCR quality on
real screenshots beyond the fixture (small print, other languages),
picking windows on a third monitor or a rotated one.

**Meeting search and Ask across meetings** (2026-10-25,
[search.md](./search.md)): every meeting and note cut into passages and read
by EmbeddingGemma-300M on Yap's own llamafile (downloaded on demand from
Settings → Privacy & data, 318 MiB), the vectors kept in
`search-index/` in the background and never in a dictation's way; hybrid
search (BM25 + meaning, weighted RRF, the question's dates and the voices it
names); the Meetings view's moments; Chat's Ask across meetings citing
passages as pills that open the meeting at the line, kept for follow-ups,
with the passages themselves when no model is set up. **Measured**
(`yap.exe eval --search`, 33 questions over a made-up team's meetings):
recall@1 0.82 / @3 1.0 hybrid against 0.30 / 0.67 for Chat's whole-note
keyword search; on an RTX 5070 Ti 100 passages/s and a question in ~8 ms,
on its processor 14 passages/s and ~14 ms; answers 33/35 on Qwen3.5-4B,
4/5 on Groq gpt-oss-20b. Unit-tested (passages, store, dates, ranking, the
checks) and e2e-tested against a stand-in embedding server
(`e2e/meeting-search.spec.js`, 5 tests). **Not yet tried**: a real year of
meetings (thousands of passages) on a laptop's processor, the app's own
download from Hugging Face (tested against a stand-in), a dictation started
mid-batch against the real server (the batch is dropped by the meeting
digests' `unless_busy`, not exercised here end to end), and other languages'
meetings (the model reads 100+; the keyword half's stemming is English).

## Not yet done

Not yet done: the AI Chat surface searches by meaning only when asked
across the meetings (search.md; elsewhere keyword-RAG; its answers stream since 2026-10-22 —
unit- and e2e-tested against stand-ins, measured on the on-device
Qwen3.5-4B and Groq, not yet through a real Anthropic, Gemini, OpenAI or
OpenRouter stream, whose shapes are taken from their docs), no calendar tools and no conversation
search/archive (Chat's `search_chats` searches past chats by keyword for the
model, not for the person); memory isn't synced between PCs, matches
questions by keyword (no embeddings), and doesn't learn with a model too
small to call tools; ElevenLabs as the reply voice isn't yet tried with a real key; spoken replies read Kokoro's
Japanese and Mandarin voices' text as English, don't check a slow PC's
speed before choosing Kokoro (the research's first-use RTF check), wait for the whole answer
before the first sentence, don't listen again by themselves after an answer,
and have no echo cancellation (a mic that hears the speakers during an
answer is a later job); the AI Notepad has no rich markdown editor (plain textarea)
or folder "add existing note" picker; meetings tell the call's voices apart
only with the beta speaker engine on, and name them only from what's said
and the invite (suggestions to confirm, a strong one as a guess; 0 wrong
names but 34–60% found in the evaluation's fictional meetings; reading the
call app's window is built behind a flag but untried in a real call; voices
of two recordings of one meeting aren't matched up; tasks are attributed
from attendees, the speaker map and names said)
and remove speaker echo only from chunks that are entirely the call (see
[`docs/meetings.md`](../meetings.md) "Limits"), and a real call (live
WASAPI loopback + a real LLM's action plan, a headset plugged in mid-call, a
hotkey dictation mid-call, the mic unplugged mid-call) still wants one
hands-on pass; the microphones' order and the unplug handling have only
met made-up devices in tests (a real headset's plug, unplug and Bluetooth
wake-up are the maintainer's to try); the refine after
a meeting judges models by the leaderboard's figures and the meeting's
language by Settings or Windows (not measured, not detected), and lowers
only its calling thread's priority; the speaker engine isn't run again
over meeting audio, and it can't be exported; a fixed line can't be split
or deleted, an edited summary has no "back to Yap's" (only Replace), and a
task's "Said at" is the line sharing most of its words (a guess: none when
no line shares enough);
the calendar has no one-click Outlook (Microsoft Graph); the pre-meeting
brief (2026-10-22: unit-, e2e- and model-evaluated, not yet seen before a
real meeting) checks the AI's lines word by word, so a mix-up of two things
its notes both say can pass (no "In short" from a small on-device model for
that reason);
a true streaming model for the partial pass (spike-gated
Stage 2 — see `ROADMAP.md` Phase 1);
multi-phrase dictionary entries; verify-after-paste beyond Win32 edit
controls (a web or code editor's hidden input can't say whether a paste
landed); a JetBrains IDE's or Visual Studio's terminal, and a browser's
(vscode.dev, Codespaces), still count as the editor;
Authenticode signing (blocked on SignPath approval); audio-history export;
numbers from a real laptop (the smaller-PC numbers are an emulation on the
test desktop; a hybrid laptop's DirectML on its card is untried), and
non-Windows (Linux/macOS) polish. See `ROADMAP.md`.
