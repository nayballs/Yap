# Meetings

Yap records a meeting into a note, from any call app or none (no bot joins):
your microphone is "You", the computer's sound output is "Them" (until you,
or a 1:1 meeting's invite, name who it is: [Who said what](#who-said-what)). It
transcribes locally as the meeting goes, summarises every ~10 minutes in the
background, and at the end turns the whole meeting into an **action plan**: a
section per person with their tasks and deadlines, then decisions, open
questions and unassigned tasks, back in seconds however long the meeting ran.
It can also notice a call starting and offer to take notes
([Call detection](#call-detection)), keeps a recording in bounds (its
windows stay out of screen shares, it stops at a maximum length, it can stop
when the call ends or once the meeting has gone quiet, it checks in when a
meeting runs over, and a shortcut starts and stops it:
[Guard rails](#guard-rails)), and while a meeting records, a
[meeting notepad](#the-meeting-notepad) docks to the edge of the screen beside
the call. With a calendar connected, the Meetings view lists what's coming,
a card offers **Join & take notes** just before each meeting, and a note
takes its meeting's name, attendees and invite ([The calendar](#the-calendar),
[`calendar.md`](./calendar.md)).

| Piece | Code |
|---|---|
| Recorder: capture, chunking, echo check, dictations kept out, following the output device, quiet sides | `src-tauri/src/meetings/meeting.rs` |
| The dictation signal; the correction dictionary (`apply_corrections`) | `src-tauri/src/dictation/pipeline.rs` |
| Guard rails: maximum length, quiet meetings and check-ins, the meeting shortcut, notices | `src-tauri/src/calls/meeting_guard.rs` |
| Hiding meeting windows from screen capture | `src-tauri/src/calls/capture.rs` |
| Rolling digests, the final input, the checks; the notepad's catch-up, helper and title inputs | `src-tauri/src/meetings/meeting_summary.rs` |
| The end of a meeting: pause or end, "Started by mistake?", the action plan job | `src-tauri/src/meetings/meeting_end.rs` |
| "What did I miss?", the one-tap helpers (Questions to ask, Follow-up email, Gaps and risks), the AI meeting title | `src-tauri/src/meetings/meeting_assist.rs` |
| The notepad window: docking, split screen | `src-tauri/src/meetings/notepad.rs` |
| Who said what: the speaker map, names on lines, 1:1 calendar guesses, line times and splitting | `src-tauri/src/speakers.rs`, `src/lib/components/SpeakerMenu.svelte` |
| Names for the voices: suggestions from what's said and the invite, the AI's checked answers, "It was me"; the cards | `src-tauri/src/speakers/naming.rs`, `src/lib/components/SpeakerCard.svelte` |
| Who the call app shows talking (experimental); `yap.exe call-dump` | `src-tauri/src/speakers/call_ui.rs` (+ `call_ui/profiles.json`) |
| The calendar: meetings, the reminder card, a note's meeting | `src-tauri/src/calendar.rs` (+ `calendar/`; [`calendar.md`](./calendar.md)) |
| Prompts (`MEETING_DIGEST_PROMPT`, `ACTION_PLAN_BASE_PROMPT`, `ACTION_PLAN_DEFAULT_FRAGMENT`, `CATCH_UP_PROMPT`, `MEETING_ASK_PROMPT`, `MEETING_QUESTIONS_PROMPT`, `MEETING_FOLLOW_UP_PROMPT`, `MEETING_BLIND_SPOTS_PROMPT`, `MEETING_TITLE_PROMPT`) | `src-tauri/src/ai/llm.rs` |
| Storage (`Note::digests`, `Note::speakers`, `TranscriptSegment::echo` / `::dictated` / `::speaker` / `::start_ms` / `::words`, `Action::kind`, `Note::title_auto`) | `src-tauri/src/notes.rs` |
| The action run (`note_enhance` / `run_enhance`) | `src-tauri/src/notes/commands.rs` |
| Fixing the notes: a line's words, the summary's, never written over unasked; tasks ticked and listed | `src-tauri/src/notes/note_edits.rs`, `src-tauri/src/notes/tasks.rs`, `src/lib/core/noteEdits.js`, `src/lib/views/TasksView.svelte` |
| UI | `src/lib/NotesView.svelte`, `src/lib/Notepad.svelte`, `src/lib/stores/meetingSummary.svelte.js` |

## Recording

**Record** on a note's chip row starts it; the chip becomes **● 12:34 · Pause**
(stop without summarising, `meeting_pause`; **Resume** carries on in the same
note) and the bottom bar's action picker becomes **End meeting & summarise**
(`meeting_end`). Any recording that starts, from anywhere, also opens the
[meeting notepad](#the-meeting-notepad) (a setting, on by default).

- **Two streams.** The mic ("You") and WASAPI loopback ("Them": a cpal input
  stream on Windows' default output device), each downmixed and resampled to
  16 kHz on its own capture thread. "Them" follows the default output: see
  [Following the output device](#following-the-output-device).
- **Chunks cut between words.** Once a source has 15 s buffered, the worker
  cuts it at the quietest 20 ms frame of the last 4 s, leaving the newest
  0.4 s, where a word may still be going on (`media::quietest_frame`; a
  backlog keeps that margin too, so a dictation's pre-roll is always still
  buffered when it starts). It used
  to cut every 15 s on the dot, splitting words. `media::quietest_index`
  (one sample every 10 ms) isn't enough here: in running speech it lands on
  zero crossings mid-word, while a whole frame only reads quiet in a real
  pause. With real speech the cuts land at commas and between sentences.
- **One warm engine.** Chunks are transcribed on the engine dictation uses,
  taken per chunk and given back, so dictation keeps working.
- **Bounded memory.** The worker keeps each buffer near one chunk. If
  transcription falls behind (a slow model on a CPU), the backlog is worked
  off in chunks of at most 30 s, back to back. Each source buffers at most
  20 minutes (~77 MB); past that, newer audio is dropped and the UI says
  "Transcription is falling behind". The transcript itself is text (~150 KB
  for two hours).
- **The correction dictionary.** Every chunk's text, both sides, goes
  through the dictionary exactly as a dictation does
  (`pipeline::apply_corrections`): the exact replacements, then, for an ONNX
  model with **Catch near-misses** on, the fuzzy pass ("send the jaisen file"
  → "send the JSON file"; entries opted out of it stay exact-only). Until
  2026-10 meetings only gave Whisper the words as its `initial_prompt`, so
  with the default Parakeet model they got no corrections at all; names
  matter twice here, as the action plan only takes owners who are attendees
  or names said in the meeting. The echo check compares corrected text on
  both sides. Upload runs the same function over a file's whole text.
- **Saved as it comes.** Segments `{ source, text, ts }` are emitted live
  (`yap-meeting-segment`) and appended to the note on every chunk, so a crash
  loses at most one chunk. `ts` is when the segment's speech started (its
  first 20 ms frame of speech), which also orders a "You" and a "Them" chunk
  cut at the same moment.
- **One clock.** A position in a side's audio is a time in the meeting, so a
  side must never skip time: WASAPI loopback delivers nothing while nothing
  plays on the device, and a device switch leaves a gap. A side whose device
  has been silent while the other moved on half a second is filled with
  silence up to it (`keep_in_step`, every 200 ms on the capture thread).
  Without that, "Them" would run early after a gap (its segments dated too
  soon, and the echo check holding the wrong stretches side by side).
- **Echo.** On speakers, the mic hears the call too, so "You" repeats "Them",
  and a summary could hand "I'll do it" to the wrong person. A "You" chunk is
  flagged `echo` only when both hold: its words mostly repeat what "Them"
  said around then, in order (≥ 60 % of its content words), **and** the
  mic's 20 ms loudness envelope follows the call audio 0–600 ms later
  (Pearson r ≥ 0.6). The timing test protects headphone users: their own "yes,
  I'll send the budget by Friday" repeats the question's words but not its
  timing. Flagged segments stay in the note, hidden behind "Show 1 line your
  mic picked up from the speakers" and left out of digests and summaries. A
  chunk where the user also spoke is kept whole. OpenWhispr's
  `meetingEchoLeakDetector.js` does the audio side with sample correlation and
  echo cancellation; this is the light, offline version.

### Dictating mid-meeting

Yap's users dictate, and mid-call they dictate a Slack reply or an email with
the hotkey. The meeting's mic stream and the dictation's are two streams on
the same mic, so those words used to land in "You" too, could become tasks in
the action plan, and travelled wherever the notes went. Wispr Flow leaves them
out and marks the gap; so does Yap now.

- **The signal.** `pipeline::start_recording` (plain dictation, the edit
  hotkey and the wake-word path all record through it) bumps one atomic,
  `count << 1 | recording`; the stop and the cancel clear the recording bit.
  The meeting's mic callback reads it once per callback, lock-free
  (`pipeline::dictation_state`), and still notices a dictation that started
  and ended between two of its callbacks (the count moved on).
- **Silence, not a gap.** While a dictation records, the mic side gets
  silence instead of the audio, the same length, so segment times and the echo
  check stay aligned. A new dictation also blanks the newest ~340 ms already
  buffered: its pre-roll (the 300 ms before the press that the dictation
  prepends to catch a word already under way) plus 40 ms for the two streams'
  callbacks not lining up. The callback that sees the dictation end is blanked
  too, as it may hold its last moment. "Them" carries on untouched. A
  dictation counts as the mic being heard (it works), not as meeting talk
  ([Quiet sides](#quiet-sides)).
- **The marker.** Each blanked span leaves one segment
  `{ source: "you", text: "", ts, dictated: true }`, dated where the span
  began (its pre-roll), whichever chunk it falls in or spans. The Notes view
  shows it as a quiet divider with a mic glyph, "You dictated here · left out
  of the notes" (the meeting notepad's Transcript tab draws its own divider
  for the same flag). Digests, the action plan,
  "What did I miss?", the AI title, the Ask bar, the chat `get_note` tool and
  "Started by mistake?" all read talk only (`TranscriptSegment::is_talk`: no
  echo, no markers), and the markdown export writes "_You dictated here (left
  out of the notes)._".
- **What isn't kept.** The dictated words aren't stored with the meeting at
  all (a dictation goes to History as usual); the marker only says when.

### Following the output device

"Them" is a loopback stream on Windows' default output device. Plug in a
headset, or pick another output in Windows, and the call moves to the new
default while the old stream hears nothing. The capture thread reads the
default render endpoint's id every 2 s (`IMMDeviceEnumerator`, the "console"
role, as cpal) and when it changed reopens the loopback stream on the new
device: the old stream stops first, so the two never interleave; if the new
one won't open, the old one carries on and the next check tries again (said
once in the log). The gap is silence ([one clock](#recording)). The log says
`Meeting: Windows' default output changed; "Them" now follows it`, after the
new stream's own line naming the device.

A call app can also play on a device of its own choosing (Teams and Zoom
have speaker settings). When that isn't Windows' default, loopback never
hears the call, which the quiet-side warning below catches.

### Quiet sides

While recording, the worker watches both sides every 200 ms. When one side
has heard nothing above the silence gate for **3 minutes** of the meeting
while the other talked at least **10 s** in that time, Yap says so once, until
that side is heard again:

- "Them" quiet: "Yap hasn't heard your Teams call for 3 minutes. If it plays
  through a headset, make that Windows' default output device." (naming the
  call when call detection sees one; else "the call").
- "You" quiet: "Yap hasn't heard your mic for 3 minutes. If you're talking,
  check it isn't muted, and pick the right mic in Settings → Dictation."

Both sides quiet is a break, not a fault, and a cough isn't talk. The warning
is the existing `yap-meeting-warning`: a toast in the main window, in
whichever view is open (`meetingGuard.js`, 12 s), and in the meeting notepad.
When neither is in view (the notepad isn't on screen and the main window
isn't focused: the person is in their call app), it's also a Windows
notification ("Yap can't hear the call", or "…your mic" with **Update
settings**). The recording carries on either way. An in-person meeting (no
call) gets the "Them" warning once too; its wording says "If".

## The meeting's audio, kept on disk

Since 2026-10-23 (`meetings/meeting_audio.rs`) the recorder keeps the meeting's
audio as well as its lines, on this PC only, so that a crash loses nothing,
a meeting can be **transcribed again** later, and a line can be **played**.
Wispr Flow keeps 14 days of meeting audio to play back; Meetily transcribes
again; Yap does both.

**What's written.** Each recording (a resumed meeting is a second one) is
two files in `<data>/meeting-audio/<note id>/`: `<recording>-you.flac` (the
mic) and `<recording>-them.flac` (the call), plus `<recording>.json` — when
it started, how far each side is in the note, where hotkey dictations began,
and whether it ended. They're written **as the meeting records**: the
recorder's own push path copies every sample it keeps — a dictation already
replaced by silence (its pre-roll too: the newest third of a second is held
back until it can't be blanked any more), a call side that went quiet
already filled — and a writer thread appends it every half second, a FLAC
frame (a quarter second) at a time, onto the disk itself every 5 s. So the
files are the recording's clock sample for sample, and a dictation is kept
out of them as it is out of the transcript.

**Why FLAC.** Two sources for up to a few hours: as 16 kHz 16-bit WAV that's
115 MB an hour each. FLAC is lossless — a better model later hears exactly
what the live one did — and Yap already decodes it (Symphonia, for Upload),
so only an encoder was needed: a small one of Yap's own (`flac.rs`, FLAC's
fixed predictors with Rice coding; no new crate, no C library), a few
microseconds a frame. Measured on this PC (`cargo test --lib -- --ignored
flac_sizes`, `YAP_FLAC_SAMPLE` = a speech WAV): digital silence 0.2 MB an
hour, a room's hiss at −60 dBFS 47 MB, Windows' text-to-speech 60 MB (52 %
of WAV), that speech over the hiss 70 MB (61 %). A call side is digital
silence whenever nobody on the call talks, so a two-hour meeting comes to
roughly 0.2 GB for both sides, against 0.46 GB as WAV. (A lossy codec —
Opus, ~11 MB an hour — would need a C library and Symphonia can't decode it;
IMA ADPCM, 29 MB an hour, loses what a better model might use.)

**How long it's kept.** Settings → Meetings → **Meeting audio**: "Keep
meeting audio" (on) for **14 days** (1, 7, 14, 30 or 90): long enough to
transcribe a meeting again after a better model arrives, or to check what
someone said last week; short enough that it never piles up (a week of
meetings is ~0.5–1 GB). Never more than 10 GB in all (the oldest go first);
a recording going on is never touched. Off, the audio is still written while
a meeting records (the crash case) and deleted as soon as the meeting's
lines are in. Deleting a note deletes its audio; **Delete all meeting
audio** says what it costs ("3 meetings · 412 MB") and clears it, notes and
transcripts staying. Nothing is uploaded, and the log has ids, counts and
lengths only.

**After a crash.** A crash, a power cut or Yap being killed mid-meeting
leaves a recording that never ended. At the next start Yap finds it (not
one another Yap is writing: the files are opened so nobody else may write
them, which a second Yap on the same data can tell), counts what never
became lines, and — at least a second of it — says so in one place
(`notices/notice.rs`: a toast while the main window is looked at, else the Yap bar,
else a Windows notification): "Yap closed during a meeting · The last
3 minutes of “Supplier call” weren't transcribed. The audio is kept:
transcribe it into the note now?" with **Transcribe it**. The note says it
too, in the Notes view and the notepad, until it's done. Transcribe it runs
the recorder's own steps over the audio from where the note stops — the
same cuts in pauses, engine, correction dictionary, echo check and
dictation markers — puts the lines where they belong by time, and then does
what a stop would have: the ⏸ mark, then "Started by mistake?" or the AI
title and the action plan (`meetings/meeting_end.rs`).

**Transcribe again.** On a past meeting with its audio kept — the Notes
view's note menu (**Transcribe again…**) or the notepad's ⋯ — Yap asks first
("Transcribe “Supplier call” again? Yap runs Parakeet V3 over the meeting's
audio and replaces its transcript. Your notes stay as they are, and names
stay on the lines that still match. The summary stays as it is until you
generate it again."), then replays every recording of it on the speech model
in use now, with "Transcribing again with Parakeet V3 · 40%" over the
transcript. What it keeps: **My thoughts** (never touched); **who said
what** where the lines still match by time — a new line takes the name of
the old line of its side it overlaps by at least half, split where two old
voices meet; the **summary** (said so: "The summary is still the earlier
one: generate it again to match"); the ⏸ marks, moved to the same places;
"You dictated here" markers; and lines it has no audio for (a recording from
before audio was kept). The rolling digests are dropped, so the next
summary is written from the new lines.

**Play a line.** ▶ beside a transcript line (on hover, or Play this line in
its right-click menu) plays both sides of its stretch, a quarter second
either side, mixed — while the meeting still records, too.

### A part that couldn't be transcribed

Since 2026-10-24 (`meetings/meeting.rs`, "Failed chunks"; code review #6). A chunk
the speech model can't transcribe — no model, the graphics card lost after
the PC slept, a driver reset, a crash in the engine — used to be recorded
as silence: a meeting could end with an empty transcript and only "Nothing
to summarise yet" to say so. Now:

- its stretch gets a **marker** in the transcript, on its side: "⚠ Couldn't
  transcribe 0:42–1:12 · **Try again**" in the notepad and the Notes view
  (its tooltip says whether it was the call or your mic). Summaries, the AI
  title and the word count skip it, and a meeting with one never asks
  "Started by mistake?" (the meeting may be in there);
- the person is **told once per meeting**, in one place (a toast while the
  main window is looked at, else the Yap bar, else a Windows notification,
  with Open note): "Yap couldn't transcribe part of this meeting · Its audio
  is kept: Yap transcribes that part again when the meeting ends" (or, with
  the refine set to Never, "…use Try again on the transcript once the
  meeting ends");
- the engine that failed is dropped, so the next chunk loads a fresh one
  (the case of a lost graphics card);
- **Try again** (after the meeting) reads that side's stretch from the
  audio on disk, transcribes it on the speech model in use now, and puts
  its lines in the marker's place — the ⏸ marks and digests after it move
  along. Still failing, the marker stays and a toast says to check the
  speech model.

The audio of a meeting with a failed stretch is held even with "Keep
meeting audio" off, until it's transcribed (a day at most).

### The refined transcript, after the meeting

Since 2026-10-24 (`meetings/meeting_refine.rs`), after Wispr Flow's "refined
transcript": when a meeting ends, Yap transcribes it again in the background
from its audio — **when it helps**. Settings → Meetings → Meeting audio →
**Improve the transcript after the meeting**:

- **Automatically when it helps** (the default): part of the live
  transcript couldn't be transcribed, or a **more accurate speech model is
  installed** than the one that made it live — for the meeting's language
  (the one picked in Speech-to-Text, else Windows'; Parakeet V2, English
  only, never refines a German meeting; with the language unknown, only a
  model that speaks every language the live one does);
- **Always**: every meeting, on the most accurate model there is (the one
  in use when it's the best);
- **Never**: Try again and Transcribe again stay.

"More accurate" is the same leaderboard figure as Settings' bars (Cohere
0.93 > Parakeet V2 0.90 > Parakeet V3 0.88 > Canary 1B 0.84 > Whisper Large
0.81…), by a margin. The new lines go in through **the same replacement as
Transcribe again** (My thoughts, names on lines that still match, the ⏸
marks and "You dictated here" kept), after a check that they say what the
live ones did: at least half the live words found again, else the live
transcript stays ("The transcript wasn't improved: … (another language?)").

**Which comes first, the summary or the refine.**

- *The live transcript is whole* (a better model, or Always): **the summary
  first**, from the live transcript, at once — its digests were made while
  the meeting recorded, so even a two-hour meeting's action plan takes
  seconds, and a short meeting still feels instant. Then the refine runs
  ("Improving the transcript · 40%", with Cancel), and when it's done the
  transcript says **"✓ Transcript improved · Update summary"**. Update
  summary writes it again from the new lines, the way Generate summary does
  (on-device or cloud, it sends what a summary sends today, nothing more).
  Not automatic: the refined words rarely change what was decided, a
  summary the person has read or edited shouldn't change under them, and
  for a long meeting every digest would be written again (more tokens on a
  cloud model).
- *It has holes* (chunks failed): **the refine first** ("Improving the
  transcript, then the summary · 40%"), then the summary — or "Started by
  mistake?" — from the whole transcript. A summary written around holes
  misses what was decided there: that's wrong, not just rough, so a little
  later and complete wins. Generate summary meanwhile does nothing (it
  follows by itself), and the notepad hides its pill.

**Never in the way.** It waits for the summary being written, never runs
beside a meeting that records ("…after the meeting that's recording"), and
between chunks waits for any hotkey dictation and 3 s after it, so a
dictation's transcription always finds the engine free; on the model in
use it shares the warm engine a chunk at a time (a dictation that ends
mid-chunk waits for that chunk, never loads a second copy), on another
model it loads a copy of its own and drops it after. Its transcription
calls run on a lowest-priority thread. Cancel stops it between two chunks;
the live transcript stays.

## Who said what

Every transcript line has a side (`source`: the mic "you", the call "them")
and a voice (`speaker`). Until voices are told apart, the call is one voice,
"them"; with "Tell speakers apart in meetings" on (beta, `speakers/diarize.rs`), the
speaker engine's voices go on the "Them" lines as "them-1", "them-2"…
("Speaker 1", "Speaker 2"; see **The speaker engine's labels** below).
A line nobody relabelled stores no `speaker` (its side is its voice), so
every older note reads as You and Them, unchanged.

The note's **speaker map** (`Note::speakers`, `speakers.rs`) names voices:
`{ "them": { "name": "Priya Shah", "email": "…", "source": "calendar" } }`.
Where a name comes from, most trusted first: **the user** (locked: nothing
automatic ever renames it) > **the call app** > **a voice match** > **the
calendar invite** or **an AI guess**. A name never gives way to a less
trusted source, and a line the user assigned (`speakerLocked`) is never
moved by an automatic relabel (OpenWhispr's rule). The last two are
guesses: shown as **"Priya?"**, in italics, and told to the AI as a guess.

- **1:1 calls.** When a meeting note is tied to a calendar event whose
  invite has exactly one other person (not you, not a room, not someone who
  declined), "Them" is named after them, as a guess. Someone else joining
  makes it wrong, but no worse than "Them", and it stays visibly a guess.
- **The menu on a name** (the notepad and the Notes view; on a line's person
  button for that line alone): **It's Priya** / **Not Priya** on a guess
  (Not keeps that name off the voice for good: the calendar won't guess it
  again), **Rename X everywhere…** (every line of the voice, now and later;
  a name another voice already has merges the two), **Rename just this line
  / these lines…** (to a voice by that name, else a new person, "p-1"; the
  lines are then the user's), **Merge with…** (two voices are one person;
  the merged one keeps the more trusted name, and later lines of either read
  as one). "You" is always you: it's never renamed or merged, though a line
  can say someone else said it. Every window follows (`yap-note-changed`
  with each line's voice).
- **Where names go.** The transcript (grouped by voice), Copy as Markdown /
  text, Save as .md, the email body, every AI input — digests, the action
  plan, "What did I miss?", Ask, the helpers, the AI title — as the line
  labels plus a "Speakers besides You: …" line (only when someone is named or
  voices are told apart), the chat's note tools, the local API (`speakers`,
  each line's `speaker`) and the MCP server's transcript pages. A name in the
  map counts as a known person in the owner checks, like an attendee; an
  invented owner still goes to Unassigned.
- **Line times** (for the speaker engine's labels). A recorded line keeps its
  place on the recording's clock (`startMs`/`endMs`, ms; each recording — a
  resume starts a new one — counts from 0; the same clock as
  `diarize::Timeline`: the "Them" sample position / 16) and, with Parakeet, Whisper or
  SenseVoice, its words' times (`words`, ms from `startMs`, one per word of
  the corrected text). `speakers::split_line` cuts a line where the speaker
  changes, at its words (in proportion without them).

### Names for the voices

The speaker engine says which lines are one voice; it doesn't say who. Yap
**suggests** who each voice is and the person confirms
(`speakers/naming.rs`) — from what the meeting itself says and who was
invited, never from how anyone sounds (no voiceprints: they're biometric
data under UK GDPR, and what US lawsuits against note-takers were about).

- **What counts.** Someone handing over by name at the end of a turn
  ("Priya, can you walk us through it?", "What do you think, Sam?", "Over
  to you, Siobhan") and a voice answering; someone thanking a person by
  name right after a voice spoke ("Thanks, Priya"); a voice introducing
  itself ("I'm Rochelle from the agency", "Ben here", "Declan from legal
  here"). A voice that hands over to, thanks or talks about Priya ("Priya
  will send it", "Sam's just dropped off") isn't Priya. A 1:1 invite's
  other person counts too (the guess above), and, when a meeting ends, the
  AI's answer (below). Names come from the invite's attendees and the
  note's, Memory's People, and names said in the meeting (in an
  introduction, or twice when handing over or thanking — never an everyday
  word); "Kate" or "Rob" in the meeting is the invite's Katherine or
  Robert. Your own name (About you) is always You.
- **The card.** Over the transcript (and the summary) in the notepad, and
  over the meeting transcript in the Notes view: **"Speaker 2 is Priya
  Shah?"** with why ("Speaker 1 asked “Priya, can you walk us through the
  new screens?” and Speaker 2 answered"), one card at a time, surest first
  ("1 of 2"). **Yes** makes the name the person's own (locked, like a
  rename; a summary already written says "Priya Shah" where it said
  "Speaker 2"). **Someone else** never suggests that name for the voice
  again and opens "Who is Speaker 2?" (the rename everywhere, attendees as
  picks). **Not now** doesn't ask again in this meeting (the menu on the
  name still offers "It's Priya Shah"). Settings → Meetings → "Suggest
  who's speaking" (on) switches it all off.
- **When Yap puts a name on by itself.** Only a strong suggestion — two
  separate cues (asked, then thanked), or an introduction plus one more,
  and nothing against it — goes on the voice as Yap's guess ("Priya
  Shah?", in italics), so digests, the action plan and Ask read it (told
  it's a guess); everything else waits on the card. A wrong name is worse
  than "Speaker 2": when the cues disagree (one voice asked as Priya and
  thanked as Sam — two people the engine merged), nothing.
- **The AI's answer.** When a meeting ends, before the action plan, the
  meeting's AI (Note Formatting, else cleanup; only a model that can call
  tools — a cloud one or ≥ 4B on this PC) gets one short question per
  voice still unnamed (at most four): the names, the lines around where
  they were said, "which of these is Speaker 2, or unknown?". Rust keeps an
  answer only if the words it quotes are really there, say the name, and
  aren't the voice talking to that person. On its own an answer is a card,
  never a name on the voice. A model on this PC answers under a JSON
  grammar, never during a dictation.
- **"It was me".** When your own voice reaches the call side (the call app
  playing you back, a shared room), the engine finds it as one more voice
  whose lines repeat yours: the card asks **"Speaker 3 is you?"**, and
  **It was me** makes its lines yours, the repeats hidden and left out of
  the summary (Undo in the toast). Any voice's menu has **It was me** too.
- **Measured** (`yap.exe eval --speakers`, 31 fictional meetings with
  diarized voices, 73 voices with a name to find, traps included: an
  absent invitee talked about, someone answering for another, two Sams,
  "Will" and "Mark" as words, a voice split in two or two merged, your
  voice on the call side): Rust alone suggests 44 correct names (60%), puts
  25 on voices (34%), **none wrong**; with Qwen3.5-4B on the graphics card
  46 (63%) and 27 (37%), none wrong, ~0.2 s a question; Qwen2.5-1.5B on a
  processor added nothing (Rust threw out every name it gave) at ~11 s a
  question, so a model that small isn't asked. See
  [`architecture/speakers.md`](./architecture/speakers.md) "Names for the
  voices: evaluations".

### Who the call app shows talking (experimental)

Off, and not in Settings yet (`"meetingCallNames": true` in config.json
turns it on): while a meeting records during a detected call, Yap reads
the call window's accessibility tree about once a second — the tile Google
Meet or Teams lights up for whoever is talking, Zoom's "Active speaker" —
read only (no click, no focus, no screenshot), and dates each reading on
the meeting's clock. A name shown alone for most of one voice's speech is
that voice's (the call app's word ranks above the calendar and the AI:
no "?", no card). The rules come from what's known publicly and from
Wispr Flow's defaults, not yet from a real call: the capture steps that
will check them are in
[`architecture/speakers.md`](./architecture/speakers.md) "Reading the call
app: what a real call has to show".

### The speaker engine's labels

`speakers/labels.rs` puts the engine's timeline (`diarize::Timeline`: who
spoke when on the call side, ms on the recording's clock) on the lines:

- **When.** Each timeline the engine reports (every ~5 s of audio while it
  records; labels settle ~10 s, at most ~21 s, behind the talk) labels the
  lines whose audio it has settled (`endMs ≤ labelledMs`); new lines the
  latest labels already cover get theirs as they arrive. The final timeline
  comes when the recording stops, before it says it stopped: it labels the
  rest, so the AI title, the last digests and the action plan read the
  voices. Rolling digests written while recording keep "Them" (a digest
  that read a line before it was split follows it: `notes::add_digest`).
  The notepad's live transcript fills in names ~15 s behind the talk;
  every window follows `yap-note-changed` with `lineSplice` (the stretch
  that changed) and `lineSpeakers`.
- **Which lines.** That recording's "Them" lines (from where the transcript
  stood when it started) that have a place on its clock, no voice yet, and
  aren't the user's (`speakerLocked`: never moved). Never "You", echo, "You
  dictated here" markers or lines from before line times. A line splits
  where the speaker changes, at its words (Parakeet, Whisper, SenseVoice),
  else in proportion to its text; each part keeps its words and place, and
  digests and stop marks after it move along. A line with no voice heard
  near it stays "Them".
- **Voice ids.** The engine's voices (`(stream, speaker)`: an engine
  started again after a crash counts from 1 again) become "them-N",
  numbered on from the note's highest in the order they first get a line.
  A resumed meeting (a new recording, new engine numbers) carries on at
  "Speaker 4", and is never merged with the first recording's voices by
  itself: the user merges them if they're the same people. The engine says
  which lines are one voice; names stay the speaker map's (the engine's
  labels rank as a voice: below the user and the call app).
- **"Them"'s name and the voices** (the 1:1 rule). Per recording: **one
  voice** found — it is "Them", and takes "Them"'s name (a 1:1 calendar
  guess stays a guess, "Priya?"; a name the user gave "Them" stays theirs).
  **Several** — a guess goes to one of them, never all: the voice that said
  the most words (while recording, the first to qualify keeps it; at the
  end it moves to whoever said the most), never a voice with a name of its
  own or one told "Not Priya". A name the user gave "Them" stays on "Them"
  only. "Them" keeps its name for the lines nobody could label, and the AI
  is told a name once.
- **No labels.** The switch off, no model, or the engine failing: the lines
  stay You and Them. If the engine stopped early (or never sent its final
  labels), the lines it labelled keep their voices, the rest say "Them",
  the log says so, and the notepad shows "Couldn't tell speakers apart this
  time." (or "…for the whole meeting") under the transcript.
- **Measured** (2026-10-18, `diarization-real.spec.js` with an `engines`
  build and Parakeet V3, the spike's TTS meetings played as the call at
  2–5×): words on the right person 99.4–99.5% (3 voices, clean), 96.2%
  (2 voices, overlap + noise + Opus), 93.6% (4 voices, two made from one,
  overlap + noise) — the spike's 83–100% for splitting at word times; every
  "Them" line labelled, one voice per person. A line's name shows up a
  median ~15 s of audio after its last word (at most ~23 s): the engine's
  30 s preset settles its labels in steps of up to ~20 s. The errors are
  words at a change of speaker ("…so finance isn't surprised / later.").
- **Still beta** (measured again 2026-10-22 on eight distinct Kokoro voices,
  crosstalk, an hour of audio, a resumed meeting, the download): distinct
  voices are told apart well and it costs ~11% of one core, but voices that
  sound alike merge and nothing has been measured on a real call yet — see
  [`architecture/speakers.md`](./architecture/speakers.md) "The speaker
  engine measured".

## Summaries that keep up with long meetings

One end-of-meeting call over the whole transcript overflowed the on-device
model's 8k context after ~25–30 minutes of talk (~150 words a minute ≈ 2,000
tokens per 10 minutes), and is slow and costly on cloud models even when it
fits. Yap now does map-reduce, with the map done while the meeting runs.

### Rolling digests

While recording, a **digest** is written in the background once the talk
since the last one reaches **2,000 tokens** (~10 minutes of steady talk) or
spans **10 minutes** of meeting time with at least 250 tokens. One digest
reads at most 2,600 tokens of transcript; a backlog is split into calls of
that size. Each call sends:

- the immutable `llm::MEETING_DIGEST_PROMPT`: notes on this part only, under
  `### Key points`, `### Decisions`, `### Action items`
  (`- [ ] Owner: task (due: …)`) and `### Open questions`; never invent
  names, tasks, decisions or dates; "Unassigned" when no owner was named or
  nobody agreed; deadlines only as said;
- a one-shot example, which is what keeps a 1.5B model in that layout;
- the attendees, and "the meeting so far": the latest digests' key points
  (≤ 400 tokens) for continuity;
- the part's `You:` / `Them:` lines.

Why ~10 minutes: it's ~2,000 tokens, small enough for a small local model to
read carefully (they lose track in long inputs) and for the end-of-meeting
tail to stay short. It's large enough that a two-hour meeting is 11–12 calls,
not one per chunk.

The reply is read back leniently (`meeting_summary::parse_digest`: other
heading styles, numbered lists, "- None") and checked against the part's own
transcript: an owner must be "You", "Everyone", an attendee (spelled as in the
attendee list) or a name actually said in that part, else the task is
**Unassigned**; a deadline's words must appear in that part, else it's
dropped. Digests are stored on the note (`Note::digests`, each covering
`transcript[fromSeg..toSeg]`, each starting where the last stopped), so a
crash or restart keeps them, and they show live in the transcript box under
**AI notes so far** ("AI notes up to 40:12").

Digests use the same model as note actions: the **Note Formatting** scope
when it's on, else the dictation-cleanup model. With no AI configured there
are no live digests (the end step explains). On a cloud provider they send
the same transcript the end-of-meeting summary would, just earlier; an
on-device model keeps it all on the PC. A failed digest rests the worker for
2 minutes; whatever it misses, the end catches up.

### Never slowing dictation

The on-device llamafile answers one request at a time, so on any local
endpoint (the sidecar, Ollama, LM Studio, a `localhost` URL):

- a digest waits until no dictation is recording, transcribing or cleaning
  up, plus 2 s, so a quick follow-up dictation doesn't queue behind it either;
- one already running when a dictation starts is dropped. Dropping the
  request closes its connection, which llama.cpp's server treats as a
  cancel. It's retried after the dictation;
- replies are capped (`max_tokens` 700 per digest, 1,500 for the final
  call).

Cloud endpoints run alongside dictation. They get no `max_tokens`: some models
want `max_completion_tokens` instead, and reasoning models spend part of the
cap thinking.

### Budgets

Estimated tokens (`meeting_summary::estimate_tokens` runs high on purpose:
3.5 characters per token for ASCII, 1 per other character):

| Call | Input | Reply | Total, of an 8,192 context |
|---|---|---|---|
| Digest | ≤ 2,600 transcript + ≤ 400 context + ~700 prompt and example | ≤ 700 | ~4.4k |
| Final (digests) | ≤ 4,500: ≤ 600 typed notes, ≤ 1,800 raw tail, the rest digests + ~550 prompt | ≤ 1,500 | ~6.6k |
| Final (single pass) | transcript + typed notes ≤ 3,500 + ~550 prompt | ≤ 1,500 | ~5.6k |

A two-hour meeting (~30,000 tokens of talk) is 11 digests and one final call
of ~4,500 tokens. For very long meetings the digests' key points are thinned
evenly to fit (fewer per digest, then the lead point of every second, third…
digest); every task still reaches the action plan (see the checks below).

## The end of the meeting: the action plan

**End meeting & summarise**, the notepad's **Stop**, or a stop from anywhere
else (call detection's **Stop and summarise**, the Yap bar, an automatic stop)
stops the recorder, which transcribes the last few seconds; then Rust runs the
built-in **Action Plan** action (`meetings/meeting_end.rs`, following the recorder's
`yap-meeting-state`). **Pause** (`meeting_pause`) stops without it. It no
longer depends on a window: until 2026-10 the Notes view wrote the plan, and
only for the note it was showing. Every window follows the job through
`yap-meeting-summary` (`running` with its step — 1 reading the meeting, 2
writing, 3 checking owners and deadlines — then `done`, `error`, `needsAi`,
`nothing`, or `kept` when the person edited the summary: [Fixing the
notes](#fixing-the-notes)). On a finished meeting note, **Action plan** in the
transcript box (or the notepad's **Generate summary** / **Retry**) runs it
again (`meeting_summarise`, asking first over an edited summary).

**Started by mistake?** A meeting that ends with fewer than 20 words of speech
(about ten seconds of talk) and nothing typed gets no summary. Instead the
window the stop came from (else the notepad, else the main window, whichever
is on screen) asks "Started by mistake? Only a few words were captured. Keep
this meeting or discard it." With no Yap window on screen (a stop from the
Yap bar's ■, or an automatic one), the Yap bar asks it as a card, as Wispr
Flow's Flow Bar does; left alone for a minute it keeps the meeting, and with
the bar off nobody asks. **Discard** deletes the note (`meeting_discard`) and
closes the notepad; **Keep** (or the ✕) leaves it, with **Generate summary**
for when it's wanted after all.

1. `meeting_summary::prepare_final` waits for a live digest in flight (rather
   than writing it twice), then digests whatever the final call can't take
   raw: a backlog, or a meeting recorded before digests existed ("Catching up
   on the meeting: part 3 of 12…").
2. `compose_meeting_input`: the whole raw transcript when transcript + typed
   notes fit in 3,500 tokens (nothing lost to digesting); else attendees,
   meeting length, typed notes, every digest's decisions, tasks and
   questions, the key points, and the raw transcript since the last digest.
3. One call: the immutable `llm::ACTION_PLAN_BASE_PROMPT` (use only the
   input; never invent tasks, names, owners or dates; an owner is someone
   who said they'd do it or was asked and agreed; deadlines only as said;
   keep every distinct task) + the action's editable prompt, by default:
   a 1–2 sentence summary, `## Action plan` with a `### Name` per person
   (`### You` for the user) and `- [ ] task (due: …)` lines, then
   `## Decisions`, `## Open questions` and `## Unassigned`.
4. `postcheck_action_plan`, deterministic: a `### Name` that isn't "You",
   "Everyone", an attendee or a name said in the meeting moves its tasks to
   Unassigned; a deadline whose words aren't in the input is removed; and any
   digest task the reply left out is put back under its owner. (Feeding a
   model summaries instead of the transcript is where tasks get lost.)
5. The result is the note's Summary tab, with **Copy as Markdown** and **Copy
   as text** (headings plain, tasks as ☐, for a chat message or an email).

The Action Plan is a built-in action like Meeting Notes: editable, not
deletable. It's found by `kind: "actionPlan"`, so renaming it is fine. It's
seeded into older note stores by the same additive migration as the other
built-ins (by kind for this one), so edits to any built-in's prompt are kept.
Meeting Notes, Action Items and custom actions on a long meeting read the
same digest-based input; so do the note's **Ask anything** bar and AI Chat's
`get_note` tool (≤ 3,000 tokens of meeting).

**Without an AI model** recording and transcription work as before; the end
step shows "Your meeting is saved. To turn it into an action plan, Yap needs
an AI model…" with **Open Language Models** (in the Notes view and the
notepad). A recording where nothing was transcribed and no notes were typed
asks "Started by mistake?" when it ends; asking for its summary anyway says
"Nothing to summarise yet" instead of calling a model.

## Fixing the notes

A meeting's notes aren't Yap's alone to write: a line it misheard, a
summary that missed the point and the tasks in the action plan are the
person's to fix (Wispr Flow's Notetaker does the same). Whatever model wrote
the summary — on this PC or a cloud one — the person's changes are local data,
and Yap never writes over them without asking. Code: `src-tauri/src/notes/note_edits.rs`
(lines, the summary, the rule), `src-tauri/src/notes/tasks.rs` (tasks),
`src/lib/core/noteEdits.js` and `src/lib/views/TasksView.svelte`.

### A line's words

**Double-click** a line (the notepad or the Notes view), or **Edit line** in
its right-click menu (in the Notes view also **Enter** or **F2** on a focused
line): its words become a box; **Enter** saves, **Escape** puts it back,
clicking away saves. Only the words change: the line keeps its side, voice,
time and place on the recording, so **▶ still plays what was said**. It shows
a quiet **edited**, whose tooltip says what Yap heard (`TranscriptSegment::
edited` and `original` in `notes.json`; an edit back to what Yap heard makes it
Yap's line again). Every window follows (`yap-note-changed` with `lineEdit`).

**It teaches the dictionary.** When the fix replaced a misheard word or name
("We deploy on cube and eighties" → "…on Kubernetes"), "Suggest words from
your corrections" offers it, right there: "Add “Kubernetes” to your
dictionary? Yap heard “cube and eighties”." with Add / Not now / Don't suggest
this — the same rules and answers as for a dictation you fixed
(`learn::offer_for_line`, [dictionary.md](./architecture/dictionary.md)): only
a term's spelling, never an everyday word, nothing turned down or removed
before; off with the setting.

**Transcribe again keeps it.** The new lines go in around a line the person
fixed, never over it: a new line of the same side over it keeps only its words
outside it (by its word times; without them, by its share of the line, and
none when it's mostly the fixed line) (`meetings/meeting_audio/redo.rs` `rebuild`, so
anything that replaces a meeting's lines that way keeps fixes too). The voice
separation never splits a fixed line either.

### The summary

**Edit** (the notepad's Summary tab) or **Edit summary** (the Notes view)
turns the summary into its Markdown in a box, saved as you type; **Done** or
**Escape** shows it again. Changing its words makes it **yours**
(`summary_edited_ms`, an **Edited** mark): from then on
- **regenerating asks first** — the Notes view's Action plan or an action, the
  notepad's Update or Retry: "Replace your edited summary? Yap writes the
  summary again from the meeting and your notes, and the changes you made to
  it go. Tasks you ticked stay ticked." (**Cancel** keeps it, no model call);
- **the end of a resumed meeting writes nothing over it**: its action-plan
  job ends `kept` without calling a model;
- an edit made while a replacing run was writing wins over the run.
A Chat edit of the summary (its `edit_note` tool) counts as yours too.
Emptying it leaves no summary (Generate summary comes back).

**New since this summary.** When lines were said after the summary read the
meeting (a recording resumed later), a quiet "2 new lines since this summary ·
**Update**" sits over it (`summary_ts`; not while it records, and not on notes
from before it was kept). Update writes it again — asking first if it's yours.

### Tasks

The summary's checkboxes (the action plan's `- [ ] task (due: …)` lines under
`### Owner`, `- [ ] Dave: task` lines, any `- [ ]`) **tick in place** in the
notepad and the Notes view. The tick is the summary's own `- [x]`, so it's
the same tick everywhere, Copy as Markdown and the local API carry it, and the
pre-meeting brief's open items leave a done task out. A tick isn't an edit of
the summary, and a new summary keeps it: its tasks with the same words (or
most of them) and owner come ticked (`tasks::carry_ticks`).

**Tasks** in the sidebar lists every task across meetings — open ones, or
**Show done** — **By meeting** (newest first, yours first in each) or **By
person** (You, then names, then Everyone, Unassigned, No owner), with the
deadline, the meeting, and **Said at 12:40**: the line it was most likely said
(the talk line sharing most of its words), opening the meeting in Notes there.
Ticking there ticks in the note (and the other way round); a task just ticked
stays in the list, struck through, until you leave, so a slip is one click to
undo. AI apps read them, read only: `GET /v1/tasks/list?status=open|done|all`
and MCP's `list_tasks` ([integrations.md](./architecture/integrations.md)).

## The meeting notepad

While a meeting records, its notes live in a slim window docked to the right
edge of the screen, beside the call (Wispr Flow's Notetaker notepad, ported to
Yap: `src-tauri/src/meetings/notepad.rs`, `src/lib/Notepad.svelte`). Its look and
behaviour follow Wispr's notepad as measured from its DOM
(`references/wispr-flow/notepad-spec.md`, beside this repo: sizes, type, colours,
motion), in Yap's own code, wording and icons; the palette is the
`--yap-paper-*` tokens in `src/app.css`.

**When it opens.** Whenever a meeting recording starts, from anywhere ("Record
notes" on a call prompt, the Notes view's Record, the tray, the Yap bar), with
**Open the notepad when a meeting starts** on (Settings → Meetings,
`meetingOpenNotepad`, default on). It never takes the focus from the call: the
window is created unfocused, so Windows shows it without activating it.
Closing it only hides it; the recording carries on, and **Notepad** on the
meeting note in the Notes view brings it back (any meeting note, recording or
not). It shows one meeting at a time: the one recording, or the one reopened.
While a meeting records it's left out of screen shares and screenshots with
the overlay ([Guard rails](#hidden-from-screen-capture-and-sharing)). Like
every Yap window you can type in, it catches the dictation key and the
meeting shortcut in the page while it has focus (a focused WebView2 window
never reaches the global hook).

**Where.** Docked to the right edge of the work area (above the taskbar), full
height, 30% of the width, between 400 and 800 px at 100% scaling (scaled for
the monitor's DPI) and never more than half the screen: 576 px on a 1920 px
screen, 768 px on a 2560 px one (Wispr's own size there). It goes on the monitor with the
call's window, else the one with the mouse cursor. Already on screen when a
meeting starts, it stays where the person put it. The window rect reaches a
few pixels past the edge on purpose: that's the invisible resize border, so
the visible frame lines up with the screen edge.

**Split the screen when joining** (`meetingSplitScreen`, off by default, greyed
out while the notepad doesn't open): when a recording starts during a call
Yap detected, the call's window moves to the rest of the work area, left of
the notepad, so both are in full view. The window is the call app's main
window: the largest visible top-level window of its process (Teams'
`ms-teams.exe`, Zoom's `zoom.exe`, or the browser a meeting tab is in, where a
window whose title shows the meeting comes first), titled, at least 200×150,
not minimised, not a tool window, dialog or a cloaked UWP frame, and never one
of Yap's own windows. A maximised window is restored first (it would ignore
the new size); the move is asynchronous, so a hung app can't hang Yap, and
it adds back the window's invisible borders so its visible edge meets the
notepad's. Only the call app's windows have their titles read, and none are
logged or kept. Test runs never move other apps' windows. The notepad's
**split button** does the same at any time (`notepad_split`), and while the
pointer is on it a glass outline shows where the call's window will go: a
small borderless window over that slot (`split-preview`, created on first
use; transparent, click-through, never focused, always on top; translucent
white with a 2 px white border and a soft shadow, so it reads on any
desktop), gone when the pointer leaves or the split happens. With no call
Yap noticed, the button says so.

**The header.** **Back** opens the note in Yap's Notes view. **⋯**: Copy as
Markdown, Copy as text, Copy consent message, Audio settings (while
recording: Settings → Dictation's microphone), Save as .md (a save dialog, the same file as
the Notes view's export), and **Delete** in red, behind a confirm dialog
("Delete this meeting?"; a meeting still recording stops first, then its note
goes: `meeting_delete`). **Share** is local (Wispr's is cloud sharing): a
"Share notes" popover with Copy as Markdown, Copy as text, Save as .md and
**Email** (a `mailto:` link with the title as the subject and the summary as
plain text, or the notes and transcript before there's a summary, cut to fit
a mail client's link limit); the link button beside it copies the notes in
one click. Then minimise, maximise and close.

**The title** is in EB Garamond, editable ("New note" when there's none), the
date under it in the computer's own format ("5 Oct, 22:32").

**My thoughts** (the default tab) is the note's own text, the same as the
Notes view's editor: typed in either window, it's saved after a short pause
and shows up in the other (`yap-note-changed`, each window sending only the
field it edited, so neither overwrites the other's newer text). It goes into
the summary as "Notes typed during the meeting".

**Transcript** (a small green waveform on the tab while recording) opens with
the timer box: the elapsed time, **search** (an inline field: only the lines
that match, highlighted) and **copy** (the whole transcript as "You: …" /
"Them: …" lines, or the speakers' names), with a tip under it you can dismiss for good (lines land
about every 15 seconds as Yap transcribes on this PC, the last few when you
stop). Then the lines as they arrive, following the newest one unless you
scroll up: a speaker's run of lines is one group under its name (You, or
Them / a name in teal; "Priya?" in italics for a guess), a chat bubble per
line with grouped corners, each with a person button ("Who said this?") and
a copy button on hover. The name, or the person button, opens the speaker
menu ([Who said what](#who-said-what)). A dashed line with ⏸ marks where a recording stopped
(`Note::breaks`, the transcript's length at each pause or stop; Resume
carries on below it), and one with a mic, "You dictated here · left out of
the notes", where you dictated mid-meeting ([Dictating
mid-meeting](#dictating-mid-meeting)). Before the first line: "Yap is
listening". Echo lines (the call through your speakers) are hidden behind
"Show N lines your mic picked up from the speakers", as in the Notes view.
With **Show live transcript** off (`meetingLiveTranscript`, Wispr's setting
of that name, on by default) the tab stays quiet while recording ("Live
transcript is off… The transcript shows here when you stop"); Yap still
transcribes as the meeting goes, and nothing hidden counts as seen for "What
did I miss?".

**Summary** ("+ Summary" until there is one) follows the action plan job:
while it's written, "• Turning 12 minutes of talk into an action plan… · Step
2 of 3" ("Catching up on the meeting: part 3 of 12…" while a long meeting's
last digests are written); then the plan. With no AI model: the "Your meeting
is saved…" card. While recording it says the summary is written when you
stop, and shows the AI notes so far (the rolling digests).

**The bottom bar.** A white pill **■ Stop** (the end of the meeting, as End
meeting & summarise) or, after it, **Resume** (records on into the same note;
**Start** on a note with nothing recorded yet), and the **Ask anything** field
(with a **What did I miss?** chip inside it while recording, **Follow-up
email** after; the sheet shows all the [one-tap helpers](#one-tap-helpers)).
Above it while
recording, the consent line ([Consent](#consent)). After the meeting, above
it: **Generate summary** when there's no summary yet, or, if writing it
failed, "The summary didn't come through", the reason and a **Retry** button.
A stop from anywhere else, including Yap's own (the length limit, a call
ending with "Stop and summarise automatically", the meeting shortcut), shows
its progress and plan here too, without bringing up the main window.

### Consent

While a meeting records, the line above the bar reads "Let people know
you're taking notes." Its **Learn more** opens a small card: why (Yap
transcribes on this PC; nothing is uploaded) and a message to paste into the
meeting chat, which you can edit: "Hi all, I'm taking notes with Yap, which
transcribes this call on my computer. Let me know if you'd rather I
didn't." **Copy** copies it ("Copied: paste it into the meeting chat"); an
edit is saved when the box loses focus (`meetingConsentMessage`, through
`notepad_consent_message`; empty, or the default text, saves as empty = the
default). The ⋯ menu's **Copy consent message** copies the saved one
directly. Recording people without telling them may be against the law
where you or they are; the message is there to make telling them quick.

### Ask anything and What did I miss?

The **Ask anything** field opens a sheet over the lower half of the notepad
(a grab handle, **New chat** and **Minimise**; Escape or a click above it
puts it away; Stop shrinks to a round button meanwhile). Your questions show
as bubbles on the right, answers as formatted text with 👍 👎 and copy under
each. A rating is kept in the notepad's own storage on this PC only and never
sent anywhere. **What did I miss?** answers from what was said
**since you last looked** (`meeting_assist::meeting_catch_up`). The notepad
counts a transcript line as seen while the Transcript tab is on screen (the
window shown and not minimised, asked of the window since WebView2 reports a
hidden window's page as visible) and scrolled to its newest line, or when an
answer covered it. With nothing new it says "Nothing new since you last
looked." without calling a model. Otherwise one call on the meeting's model
(the Note Formatting scope, else the cleanup model):

- the new part raw when it's ≤ 2,400 tokens; after a longer absence, the
  digests that cover it (≤ 1,000) and the latest raw talk (≤ 1,400);
- ≤ ~700 tokens of what came before, as context only;
- `llm::CATCH_UP_PROMPT`: at most five short bullets (main points,
  decisions, anything asked of you, tasks with owners and deadlines as said),
  nothing invented, small talk said in one line; a reply cap of 400 tokens on
  a local model.

That's under 4.5k tokens with the reply, comfortable for an 8k local model. A
question typed in the same chat ("Who owns the budget?") is answered from the
whole meeting so far, bounded like the Ask bar (`MEETING_ASK_PROMPT`, ≤ ~3.3k
in). Neither ever slows a dictation (`meeting_summary::chat_beside_dictation`):
on a local endpoint the call waits until no dictation is recording or
transcribing (+2 s), and if one starts mid-call the request is dropped (the
closed connection cancels it in llama.cpp) and asked again after it. Without
an AI model the chat says so and links to Language Models.

### One-tap helpers

Chips in the Ask sheet (a row under the thread) do the common asks in one
tap, built like What did I miss?: one call on the meeting's model through
`meeting_assist::meeting_assist(noteId, kind)`, talk only (never echo or "You
dictated here" markers), never slowing a dictation, the answer in the sheet
with 👍 👎 and copy, and the setup card linking to Language Models when
there's no AI model. The set follows the recording state
(`notepadText.js` `meetingHelpers`):

| Chip | When | Reads (estimated tokens) | Answer |
|---|---|---|---|
| **What did I miss?** | recording | see above | ≤ 5 bullets |
| **Questions to ask** (`questions`) | recording | the latest talk (≤ 1,200), digests of what came before it (≤ 500), attendees, the invite (≤ 200), your typed notes (≤ 300) | 3–5 questions you could ask now, in your voice (`MEETING_QUESTIONS_PROMPT`; reply cap 300) |
| **Follow-up email** (`followUpEmail`) | after | the action plan (or what another action wrote, ≤ 1,200) and ≤ 500 of the meeting, mostly how it ended; with no plan yet, ≤ 1,800 of the meeting (the transcript, or digests + the latest talk); title, attendees, invite, typed notes | a recap to send the attendees in plain text: "Subject: …", greeting, what it covered, decisions, next steps ("Me" for yours, "Unassigned"), deadlines only as said, "Thanks," + "[Your name]" (`MEETING_FOLLOW_UP_PROMPT`; reply cap 600) |
| **Gaps and risks** (`blindSpots`) | after | the same, with ≤ 800 of the meeting beside the plan | Not decided / Missing owners or dates / Risks and open questions, ≤ 8 bullets, each tied to what was said (`MEETING_BLIND_SPOTS_PROMPT`; reply cap 500) |

Every call is at most ~3.4k tokens with its reply, and far less without an
invite or long typed notes: well inside an 8k local model, and a single call
stays well below a free cloud tier's per-minute budget, such as Groq's 8,000
(a rate limit is waited out, as for every AI call). Each prompt says to use only what's in the
meeting, never invent owners, dates or numbers, and say so in one line when
there's too little to go on. With fewer than 20 words of speech and nothing
else (no digests, typed notes, summary, or an invite to ask about), Yap says
so itself without calling a model ("Not enough has been said yet to suggest
questions…").

The email is cleaned up in Rust (`meeting_summary::clean_email`): anything
before the subject line ("Here's a draft:") and code fences go, markdown
(`**`, `#`, `* ` bullets) becomes plain text, blank lines are single, and a
missing subject line becomes "Follow-up: <title>" (or "Meeting follow-up"
for a made-up title). The sheet shows it as it copies (`plain`), so the copy
button puts exactly that on the clipboard, ready to paste into any mail app.

While a question is being typed the chips step aside. When the sheet is down
the lead chip sits in the Ask pill (pressing it opens the sheet and moves the
focus to the field). The **Notes view** shows the same helpers on a meeting
note, as chips over its Ask bar (Questions to ask while that note records,
Follow-up email and Gaps and risks after; What did I miss? stays the
notepad's, which knows what you've seen), answering in its chat with a Copy
button; without an AI model a toast says so with **Open Language Models**.

### The AI meeting title

A meeting with a made-up title — call detection's "Teams call · 5 Oct, 14:30"
(`Note::title_auto`), or none — gets a short real one ("Q3 Budget Review with
Alice") once there's enough talk: 150 words of speech or the first digest
while it records, or 20 words when it ends. One call on the meeting's model
(`MEETING_TITLE_PROMPT` + a one-shot, the first ~700 tokens of the transcript
and the first digests' key points, a 30-token reply cap on a local model, the
same dictation-safe runner), read back by `clean_title` (first line, no
quotes, markdown, "Title:" label or full stop, at most 60 characters; a
refusal or a sentence of chat is no title). A failed try is retried after 2
minutes, three times at most. **A title the person typed is never
replaced**: any edit of the title clears `title_auto` for good, and the AI
title is set under the notes store's lock only while the title is still the
made-up one (or empty). **Nor is a calendar meeting's name**: a note made
for an event ("Join & take notes", Start notes) carries its title as a real
one, and a recording that turns out to be during a calendar meeting swaps a
made-up title for the meeting's name the same way (`notes::link_event`
clears `title_auto`), so the model isn't even asked. Until it's named, a
made-up title shows muted in the notepad and the Notes view.

### Testing the notepad

- Unit tests: `cargo test --lib -- notepad meeting_end meeting_assist
  meeting_summary notes` cover the notepad's width and docking, split-screen
  rectangles, the split preview's frame, the invisible-border compensation,
  picking the call's window (largest, the browser window showing the meeting,
  never Yap's own, minimised/tool/owned/cloaked/tiny ones skipped), a stop
  marking its place once, "Started by mistake?"'s threshold, setup errors,
  the catch-up input (only what's new, nothing new → no call, a long absence
  bounded through the digests, all under 4.5k with the reply), follow-up and
  title inputs, title clean-up, a typed title never open to the AI, and the
  one-tap helpers: Questions to ask reading the latest talk and digests of
  the rest (no echo or markers; too little → no call, unless there's an
  invite), the email and the gaps reading the plan or else the meeting, a
  worst case (2.5 hours, long notes and invite, a long plan) ≤ ~3.4k with
  the reply, the helper kinds as the page names them, and `clean_email`
  (preamble, fence and markdown out, a subject line first, a one-line "too
  little" left alone).
- e2e (`e2e/notepad.spec.js`, the notepad is its own page, `yap.notepad`),
  with screenshots named after the states in Wispr's own captures for a
  side-by-side look: the text it copies, saves and emails and the
  transcript's layout (the pure helpers in `notepadText.js`); it opens docked
  on record without taking the focus, with the serif title, the locale's
  date, the waveform and "+ Summary"; grouped bubbles with coloured labels
  and echo hidden; the tip; closing it keeps recording and Notes brings it
  back; a new note's Start; My thoughts and the title syncing both ways;
  paused and dictated dividers, hover copy, search and copy all; the Ask
  sheet (What did I miss? with nothing new and no model call, then only the
  new lines sent; 👍 kept locally; copy; a typed question; New chat; Escape);
  the one-tap helpers (Questions to ask while recording, pressed from the
  keyboard, its input talk only and lean; after the meeting Follow-up email
  from the pill, the fake AI's markdown and preamble cleaned up and copied
  as plain text, then Gaps and risks reading the action plan; the chips
  switching with the recording state and stepping aside while typing; the
  same chips in the Notes view with Copy; with no AI model, the setup card
  and its link to Language Models, without a model call);
  the header (the ⋯ menu while recording and after, Copy as Markdown / text,
  Audio settings, Share and its one-click copy, the split tooltip and test
  mode's refusal, back to Notes, Delete with its dialog, Delete while
  recording); the consent message (copy, an edit saved on blur and kept by
  Settings' next save, ⋯ → Copy consent message, back to the default); the AI
  title replacing call detection's and never a typed one, then the stopped
  state (the divider, Generate summary, Resume); a note started from a
  calendar meeting (a local iCal feed) keeping the meeting's name, with no
  title call at all; Started by mistake? (Keep
  then Generate summary, Discard deleting the note, and on the Yap bar with
  no window on screen); the summary's "Step 2 of 3", a failure above the bar
  with Retry, then the plan; an automatic stop; the meeting shortcut and the
  dictation hotkey caught in the notepad; the Settings rows.
- Not under test mode: split screen and its preview (a test never moves
  another app's window), placement on a real call's monitor, Save as .md's
  dialog and Email's mail app; the first two need a real call.

### Limits

- The notepad docks only to the right edge (Wispr's default); a left-edge
  option would be a setting.
- Split screen moves only the call's main window; a call app that pops its
  meeting into a separate window after joining (Zoom, Teams) is moved only if
  that window is up when the recording starts.
- Voices on the call aren't told apart yet: "Them" is one voice until the
  user names or splits it, or a 1:1 invite names it as a guess. Yap doesn't
  re-transcribe after the meeting (Wispr does in its cloud).
- Share is local: no links to the notes or shared access (Wispr's sharing is
  its cloud's). Email hands a `mailto:` link to the mail app, so a long
  summary is cut to fit.
- The 👍 👎 ratings stay in the notepad's own storage; nothing reads them yet.
- "Since you last looked" lives in the notepad page: reloading the window (or
  restarting Yap) forgets it, and the next "What did I miss?" covers the whole
  meeting.
- The Ask sheet's answers (the helpers' included) live in the page too: they
  aren't saved with the note. The follow-up email is copied, not sent: there's
  no "open in my mail app" with the attendees filled in yet (the calendar
  invite has their addresses; the note keeps only names).

## Guard rails

Wispr Flow's Notetaker settings, ported (Settings → Meetings, one
block under "Ask about calls in"). Code: `src-tauri/src/calls/meeting_guard.rs`,
`src-tauri/src/calls/capture.rs`, `src/lib/notices/meetingGuard.js`,
`src/lib/notices/meetingCheckIn.js`.

| Setting | Config | Default | Wispr's |
|---|---|---|---|
| When a call ends: **Ask me** / **Stop and summarise automatically** | `meetingCallEnd` `"ask"` / `"stop"` | Ask me | "Stop Notetaker when a call ends", on |
| **Maximum recording length**: 1, 2, 3, 4 hours / No limit | `meetingMaxMinutes` (0 = none) | 2 hours | the same, 2 hours |
| **End notes when the meeting goes quiet** | `meetingEndWhenQuiet` | on | on (behind the same "Stop Notetaker when a call ends" switch) |
| **Hide Yap's meeting windows from screen sharing** | `meetingHideFromCapture` | on | "Don't show Notepad and Flow Bar in screen capture", on |
| **Meeting shortcut** | `meetingHotkey` | Win+Alt+M (`kb:alt+win+77`) | Win+Alt+M |

Every stop below goes through `meeting_guard::stop_and_summarise`, which ends
the meeting as **End meeting & summarise** does (`meeting_end::end`: Rust
writes the action plan once the last chunk is in, or asks "Started by
mistake?") without bringing up any window: the [meeting
notepad](#the-meeting-notepad) shows the progress and the plan when it's open,
and the note has them either way. The shortcut pressed in one of Yap's windows
passes that window along (`meeting_shortcut { origin }`), so a question about
the meeting shows where it was pressed.

### Hidden from screen capture and sharing

While a meeting records, the windows that show it, the docked notepad
(`notepad`, and `split-preview`, the outline its split button shows) and the
recording overlay (`overlay`, which shows the live
transcript while you dictate mid-meeting), get Windows' display affinity
`WDA_EXCLUDEFROMCAPTURE`: they stay on your monitor and leave every capture,
so a Teams, Zoom or Meet screen share, a screenshot or a recording shows what's
behind them. Windows 10 before version 2004 doesn't know that value; there
Yap falls back to `WDA_MONITOR`, which shows them as black boxes in a capture.
The main window isn't hidden (you might be sharing Yap on purpose).

- It's set when a recording starts and lifted when it stops, applied at once
  when the setting changes mid-meeting, and set on a meeting window that's
  created or reloads mid-recording (`on_page_load`). The flag belongs to the
  window, so hiding and showing it keeps it. All of it runs on the main
  thread, in order.
- With the setting **off**, a meeting starting shows Wispr's screen-share tip
  once per note: "Screen sharing · Your meeting notes show up in screen
  shares and screenshots." with **Update settings**, which opens General at
  the switch. In the window, plus a Windows notification while the window
  isn't focused.

### Maximum recording length

- **Five minutes before** the limit: "Notes stop in 5 minutes · Yap will stop
  recording and write your action plan. Keep going gives you another hour."
  A sticky toast in the window (it counts down) and, while the window isn't
  focused, a Windows notification with **Keep going**. Answering one takes
  both down.
- **Keep going** moves the stop an hour past the current limit; the next
  warning comes five minutes before that.
- **At the limit** the recording stops and the action plan is written, with
  "Stopped at 2 hours · The meeting reached the maximum recording length".
- A warning always gives the full five minutes: when the limit is lowered
  below the meeting's length, or the PC slept through it, the warning comes
  then and the stop five minutes later. Raising the limit or choosing No
  limit takes a warning back. Settings are read every second, so a change
  applies at once.
- The limit is per recording: **Pause** then **Resume** starts a new one.

### Quiet meetings and check-ins

Two ways a recording that has outlived its meeting gets noticed, on the same
watcher thread as the length guard (once a second; `QuietWatch::step`, pure
and unit-tested).

- **Speech** is what the recorder counts as talk (`meeting::activity`): any
  audio above its silence gate on either side, or a transcribed line. A
  hotkey dictation's span ("You dictated here") isn't: the recorder blanks
  it. Quiet time runs on the **meeting's audio clock** (how much audio the
  sides have captured), not the wall clock, so a PC asleep or a device that
  stopped delivering never adds up to a quiet meeting.
- **End notes when the meeting goes quiet** (on by default): after **10
  minutes** with nobody speaking (from the start of the recording, or the
  last speech), "**Ending notes in 30 s…**" counts down with **Keep going**
  (another 10 minutes) and **Stop now**. Anyone speaking cancels it. At zero
  the recording stops through `stop_and_summarise` (the action plan, or
  "Started by mistake?"), and a notice says why: "Stopped: the meeting went
  quiet · Nobody had spoken for 10 minutes, so Yap stopped recording and is
  writing your action plan." Switching the setting off mid-countdown cancels
  it.
- **"Still in your meeting?"** (Stop and summarise / Keep recording), once
  per recording each: **10 minutes after the end** of the calendar meeting
  the note belongs to (`Note::event`; not for a "meeting" a day or longer, or
  a recording that started after that point), and **an hour into** any
  recording. A check-in waits while anyone spoke in the last **3 minutes**
  (it comes once the meeting has been quiet that long), never comes on top
  of the length warning or within 10 minutes of it (with a 1-hour limit the
  hour check-in never comes), and goes away by itself after 2 minutes
  unanswered: it only asks. The countdown takes a check-in's place, and the
  length warning's **Keep going** counts as having checked in.
- **Where they show**: the meeting notepad if it's on screen, else the main
  window if it's focused (a sticky toast in either, `yap-meeting-checkin`
  with the window's label as `surface`; the countdown's title counts down
  with a draining bar), else a **Yap bar** card (the countdown as its
  headline, "Ending notes in 28 s…" with the seconds in a ring, readable at a
  glance), else a Windows notification with the two buttons. The stop's
  notice goes to the same place. The countdown's ✕ only hides it (as the
  length warning's does); a check-in's ✕ is Keep recording.

### When a call ends

**Ask me** is call detection's "Stop and summarise?" (below). **Stop
automatically** stops and summarises without asking, with "Teams call ended ·
Yap stopped recording and is writing your action plan.". The same rule as
the prompt applies: only for a call Yap is recording, of an app it asks
about, so a background Discord call ending never stops an unrelated
recording.

### The meeting shortcut

**Win+Alt+M** from any app:

1. while a meeting records: stop and write the action plan;
2. else, during a detected call (any call app, even one Yap doesn't ask
   about: the shortcut is an explicit ask): **Record notes** for that call, as
   from its prompt or the tray ("Teams call · 5 Oct, 14:30");
3. else: a new note "Meeting · 5 Oct, 14:30" in Meetings, recording. It opens
   in Notes with "Taking notes · Press Win + Alt + M again to stop…", or with
   the window hidden a Windows notification says so, with **Open note**.

Presses less than a second apart count once. It's a third binding in the
global hook (`meeting-key-pressed`), rebindable in Settings with the same
recorder as the dictation key (the three keys can't clash); Yap's own focused
windows catch it in the page, as for dictation (WebView2 front-runs the
global hook while one has focus), and call `meeting_shortcut`. Because the
hook swallows the **M**, a combo holding Alt or Win taps an unassigned "menu
mask" key while the modifiers are still down (AutoHotkey's `#MenuMaskKey`):
otherwise releasing Alt would open the focused app's menu bar (Office shows
KeyTips) and Win the Start menu.

## How it's tested

- **Fixing the notes** (`cargo test --lib -- note_edits tasks redo split_line`;
  e2e `notes-edit.spec.js`): a line's edit (what Yap heard kept, its place
  and voice untouched, back to Yap's on a revert, markers refused), the
  window's line found again after a split, older notes loading unchanged
  (nothing new written until set), a tick not being an edit, an edited
  summary replaced only when asked and never by a run that read it before
  the edit, ticks carried into a new summary, tasks parsed in each layout
  (owners by heading or inline, Unassigned, deadlines, code fences, "•"),
  ticking by line or by words, "Said at" finding the line, Transcribe again
  around a fixed line (with and without word times, the other side never
  giving way), a fixed line never split by voice. End to end: a line fixed in
  the notepad (saved, marked, the Notes view following, the dictionary offer
  and its Add), one fixed from the Notes view's menu still playing the same
  audio and kept through Transcribe again, a task ticked in the notepad seen
  and unticked in the Tasks view (by meeting, by person, Show done, Said at →
  the line in Notes), and an edited summary: Action plan asks (Cancel = no
  model call), a resumed meeting's end leaves it ("kept"), "2 new lines since
  this summary · Update" asks and replaces it, the tick carried over.
- **Unit tests** (`cargo test --lib -- meeting media notes`): chunk cuts land in
  pauses (and `quietest_frame` finds the pause `quietest_index` misses),
  backlogs split into bounded chunks, the buffer cap, speech onsets, the echo
  envelope and word checks (bleed vs a headphone user's own reply), digest
  windows (live and final), the token estimate, digest parsing in several
  styles, owner and deadline checks, the final input staying under 4,500
  tokens for a 2-hour and an 8-hour meeting, the post-check (an invented
  owner, a dropped task, a made-up deadline), digests having to continue each
  other, older note stores loading and getting the Action Plan once, and a
  digest giving way to a dictation (`unless_busy`). The recorder's audio
  (`cargo test --lib -- meeting:: pipeline:: notes::`): a dictation blanked
  with its pre-roll and the callback that saw it end, its marker coming once
  with the chunk it starts in when a cut lands in the middle of it, two
  dictations in one chunk (one cancelled a callback later), one that began
  and ended between two callbacks, back-to-back ones as one span, one still
  recording when the meeting starts (blanked) or ended before it (ignored),
  "Them" never blanked, every mid-meeting cut leaving the pre-roll buffered;
  a stalled side filled up to the other and a short hiccup not; following
  the default output (switch, a failed reopen retried and said once, no
  device); the quiet-side watch (said once per stretch, again after the side
  is heard, the mic side, a break, a cough, short timings) and its wording;
  markers left out of every summary input, the catch-up and the word count;
  the dictation signal; and `apply_corrections` (exact then fuzzy for ONNX,
  exact only for Whisper or with near-misses off, the per-entry opt-out).
  Speakers (`cargo test --lib -- speakers named_speakers`): unnamed voices
  reading as today, the 1:1 calendar guess, "Not Priya" for good, the user's
  name locked against every automatic source, the order of trust, renaming
  lines (a new person, back to You, a guess confirmed), merges either way
  round and later lines following, a rename that merges, old notes loading
  and the map's shape on disk; word times lined up with the corrected text
  (joined, swapped, Whisper phrases, none), line timing, and `split_line`
  (at words, in proportion, blips, far words, the user's lines unchanged);
  names in every AI input with the "Speakers besides You" line, the owner
  checks accepting the map's names, and the MCP page. The engine's labels
  (`cargo test --lib -- labels a_split_line`): voice ids numbered on from
  the note's highest (a restarted engine, a resumed meeting), only the
  recording's settled, unlabelled, unlocked call lines (not You, echo,
  markers, unplaced lines or an earlier recording's), lines filling in as
  labels settle and splitting at the change, lines with no voice near and
  the user's staying put, a failed engine labelling only what it settled,
  later lines of a renamed or merged voice following, the 1:1 rule (one
  voice takes the guess, "Not Priya" sticks, several: the most talkative
  one, moved at the end; a user's name for "Them" on one voice only), and
  digests and stop marks moving with a split line (and a digest following
  its last line). The audio on disk (`cargo test --lib -- meeting_audio
  meeting:: notes::`): the FLAC encoder against Symphonia (every kind of
  block, a short last one, a file cut off mid-frame or mid-header, seeking
  to the sample), the disk queue equal to the buffer sample for sample with
  a dictation blanked and a stall filled, retention (age, a deleted note's,
  the cap, never a live one), a crash's recording found, measured and
  clipped, `redo::rebuild` (voices and locks kept on matching lines, a line
  over two voices split, a line that barely touches keeping its side,
  markers and lines without audio staying, each recording's stop mark),
  lines recovered into a resumed meeting. Since 2026-10-24
  (`cargo test --lib -- meeting:: meeting_refine redo bridge:: pipeline::tests::a_
  pipeline::tests::one_`): two starts at once give one recorder, the note
  being recorded or started never deleted (here and by the local API) —
  each fails on the code before the fix —; a failed chunk leaving a marker
  over its stretch (not silence, not talk, read back from disk), a refine
  replacing a marker, Try again's `fill` moving stop marks and digests;
  the refine's decision (auto / always / never, failed chunks first),
  the model choice (language, margin, the live model's languages when it's
  unknown) and the agreement check; the shared engine lent in turns (a
  dictation waiting for a meeting chunk's engine instead of loading a
  copy — failing on the old take-or-load —, one load for every taker, a
  failed or panicked engine dropped, a model switch meanwhile winning);
  audio only held going after a day.
- **e2e** (`npm run test:app`), with no audio device and no real AI:
  - `e2e/meeting-refine.spec.js` (test WAVs at 4×, the fake AI, each refine
    chunk 1.5 s): both sides' first chunk failing (`e2e_meeting_fail`) →
    one notice, a marker per side in notes.json and both views ("Couldn't
    transcribe 0:00–0:… · Try again"), Try again filling each in place;
    the refine setting's three choices — Never (nothing, Cohere installed or
    not), Automatically with nothing better (nothing), with a "Cohere"
    model folder (the summary first, "Improving the transcript" with
    Cancel, every line now Cohere's, "Transcript improved · Update summary",
    the new summary reading Cohere's lines), Always (Parakeet V3 again), and
    Automatically with a failed chunk (the refine first, no summary job
    until it's done, then the summary over the whole transcript); a
    dictation in the middle of a refine never waiting for the engine (the
    log's "Speech engine: in use" — it did before the replay waited between
    the two sides' chunks — and its time against a quiet one); two meeting
    starts on two threads at once (`e2e_meeting_start_twice`) giving one
    recorder, one test player and one note's audio.
  - `e2e/meeting-audio-kept.spec.js`: a recording's two FLAC files and its
    notes on disk (ended, ~24 s each), a line's clip as a WAV of the right
    length, ▶ → Stop playing; the notepad's ⋯ → Transcribe again (the
    confirm dialog's wording) with a changed dictionary replacing every line,
    My thoughts and a name put on a line kept; deleting the meeting deleting
    its audio; Settings → Meetings saying "1 meeting · 62 KB" and Delete
    all… clearing it, the transcript staying, no ▶ left.
  - `e2e/meeting-crash.spec.js`: a test instance killed outright mid-meeting
    (`YapApp.kill()`) and started again on its data (`launchYap({ restart:
    true })`): the note's banner, the offer as a toast (`e2e_meeting_recovery_offer`),
    Transcribe it bringing the call's 48 s into the note in time order, the
    stop mark and the action plan job (needsAi), then Transcribe again from
    the Notes view's menu keeping the summary written while Yap was down and
    a name put on a line.
  - `e2e/speaker-labels.spec.js`: placed lines (with word times) fed to the
    recorder and the engine's timelines scripted (`e2e_diarize_timeline`; a
    `done` one held until the stop): names filling in in the notepad and the
    Notes view as labels settle, a line split at "Thanks" between two voices
    (stored with its words and place), a renamed voice's later lines
    following; the final labels landing before the action plan (its input
    has "Speaker 1", "Speaker 2" and the user's "Sam", whose line stayed
    put); an engine that failed leaving You/Them with "Couldn't tell
    speakers apart this time."; a 1:1 invite's guess going to the one voice,
    and not to a second one who joins.
  - `e2e/speakers.spec.js`: lines fed with voices (`speaker: "them-1"`):
    rename everywhere, merge (a later line of the merged voice follows), just
    this line, in the notepad and the Notes view (each following the other),
    copy with names, "You" only offering a line; a local iCal feed's 1:1
    meeting naming "Them" "Alice Moreau?" (the catch-up prompt sees her and
    the guess), "Not Alice Moreau", and no guess again on the next start;
    names in the AI title, Ask and the action plan's input, a plan keeping
    "### Priya" and moving an invented owner, Copy as Markdown, Save as .md,
    the local API and `yap.exe mcp`; an old notes.json reading as You/Them
    and taking a name.
  - `e2e/meeting-audio.spec.js` (test WAVs at 4×): a dictation staged over
    18–32 s of a meeting (`e2e_meeting_dictation`, the pipeline's own signal)
    while "You" talks at 1–9 s and 21–30 s: one "You" line, a "You dictated
    here" marker, the call carrying on after it, `{ dictated: true }` in
    notes.json; the dictionary on transcribed lines (the stub's "STT stub" →
    "Corrected", both sides), on lines handed over (a near-miss "jaisen" →
    "JSON" and "chat GPT" → "ChatGPT") and on an uploaded file (History
    keeps raw and corrected); a default-output switch 6 s in
    (`e2e_meeting_output_change`: "Them" gets nothing for a second, then
    follows the new device), the log saying so, the recording going on, and
    "Them" as long as "You" afterwards (22 s each); the quiet-side warning
    with 6 s timings (`e2e_meeting_quiet`), for the call and for the mic,
    with the recording going on. Run against builds with the gate or the
    filling switched off, the dictation and switch tests fail.
  - `e2e/meetings.spec.js`: the recorder plays test WAVs (4× real time);
    You/Them segments appear live; ending the meeting without AI shows the
    setup card; a mic track that's a delayed, quieter copy of the call is
    flagged as echo and a turn-taking pair isn't.
  - `e2e/meeting-summary.spec.js`: two hours of transcript fed in ten-minute
    batches through `e2e_meeting_feed`. It checks 11 digest calls, each under
    4,500 tokens with `max_tokens` 700; one final call under 6,000 tokens that
    has the digests and the raw tail but never the raw opening; an action plan
    with Alice, Bob, You, Decisions, Open questions and Unassigned, rendered
    within 15 s of the click; Bob's dropped task put back, the invented
    "Mallory" gone, a made-up deadline removed; and Copy text.
  - `e2e/notepad.spec.js`: the meeting notepad (see
    [Testing the notepad](#testing-the-notepad)).
  - `e2e/meeting-guards.spec.js`: the overlay's and the notepad's display
    affinity is `WDA_EXCLUDEFROMCAPTURE` while a meeting records and 0 after
    (read back with the debug-only `capture_affinity`); with hiding off, the
    screen-share tip and **Update settings** landing on the switch, and
    switching it back on mid-meeting hiding at once; the length warning,
    **Keep going**, the second warning and the stop at the limit, with
    seconds for hours (debug-only `e2e_meeting_limit`); "Stop and summarise
    automatically" on a simulated call end; the shortcut starting a "Meeting
    · …" note and stopping it, and taking notes on a live call; the Settings
    rows, and recording a new shortcut.
  - `e2e/meeting-quiet.spec.js`, with seconds for minutes (debug-only
    `e2e_meeting_checkins`, which also keeps the test player's "mic"
    delivering digital silence, as a real one does, so the audio clock
    runs): a quiet recording counting down in the notepad and stopping with
    "Stopped: the meeting went quiet"; a transcribed line
    (`e2e_meeting_feed`) cancelling the countdown, **Keep going**, then
    **Stop now**; with the notepad closed, the countdown as a bar card
    (`bar_status`, Keep going through the bar's pretend cursor); the hour
    check-in held back while lines keep coming, then Keep recording and no
    second ask; the calendar check-in after a meeting from a local iCal feed
    ran over, then Stop and summarise; and the Settings switch (off: a quiet
    recording carries on).
  - Capture hiding, once by hand (2026-10-05): a GDI desktop capture of just
    the overlay's rectangle (the path screenshot tools use) while a meeting
    recorded, with the overlay shown. Excluded: identical to the background
    (0 % of pixels differ); hiding off: the capsule shows (27 % differ). So
    the flag works on Tauri's transparent WebView2 overlay.
  - Unit tests (`cargo test --lib -- meeting_guard capture input_hook`): the
    length guard's steps (on time, late, raised, removed, Keep going), the
    quiet guard's (ten quiet minutes from the start, speech putting the
    count back and cancelling a countdown, quiet time standing still with
    the audio clock, Keep going, the setting switched off mid-countdown, the
    hour check-in held back by talk and asked once, fading unanswered, the
    calendar check-in and a recording that started too late for it, never
    near the length warning, the countdown replacing a check-in), where a
    prompt shows, the wording, the shortcut's label, the notification XML,
    Win combos in hotkey specs and which combos need the menu-mask key.
  - Harness: `support/fake-llm.js`, a deterministic OpenAI-compatible server
    that records every request and tests the checks by inventing an owner,
    dropping a task and slipping in a task for someone never mentioned. It
    also answers the notepad's title, catch-up and question calls and its
    one-tap helpers (the email in markdown behind a preamble, for Yap to
    clean up), and can
    fail (`failNext`) or slow down (`delay`) a kind of call, for the summary's
    progress and error states.
    Test-mode hooks, all compiled out of release builds (`shell/e2e.rs`):
    `YAP_E2E_MEETING_AUDIO` (a folder with `you.wav` and `them.wav`),
    `YAP_E2E_MEETING_SPEED`, the `e2e_meeting_feed { segments }` command
    (corrected with the dictionary like real lines), and for the recorder's
    audio `e2e_meeting_dictation { from, to }`, `e2e_meeting_output_change
    { at? }` (seconds of the test audio) and `e2e_meeting_quiet { quietSecs?,
    talkSecs? }`; for the guard rails `e2e_meeting_limit` and
    `e2e_meeting_checkins { quietMs?, countdownMs?, recentMs?, afterEndMs?,
    hourMs?, fadeMs?, marginMs? }` (`calls/meeting_guard.rs`). The player logs "the
    test audio has played to the end", so a test can stop after all of it.
    Since 2026-10-24: `e2e_meeting_fail { side?, from?, to? }` (the live
    chunks over a stretch fail), `e2e_meeting_start_twice { first, second }`
    and `YAP_E2E_REFINE_CHUNK_MS` (each refine chunk that long).
  - Robot, written and not run yet (`e2e-real/meeting-refine.real.js`, an
    `engines` build with Parakeet V3, and Whisper Large for the second
    test): a Windows-TTS supplier call whose call side's first chunk fails
    live — the refine comes first and the first sentence must be there, in
    its place —; and a meeting recorded on Whisper Large refined on Parakeet
    V3, both sides still saying what was said.
- **Real speech, once** (2026-10-05): a 74 s scripted two-voice meeting
  rendered to WAV with Windows TTS (George as You, Zira as Them; to files,
  never played), through a debug `engines` build with Parakeet V3 on
  DirectML at 2× speed. With headphones every line came through word-perfect
  ("…we decided the launch will be on March the 3rd"), except the 4 s final
  chunk ("Thanks, all" → "Thanks, or"); the cuts fell at commas and between
  sentences; a 15 s chunk took ~0.5 s to transcribe. On speakers (the mic
  also hearing the call 100 ms late at 30 %), every "You" chunk mixed the
  user's own speech with the call, so the echo check (correlation 0.15–0.29,
  overlap 0.24–0.48) kept them all, as designed.

## Limits and follow-ups

- **Meeting audio.** The refine after a meeting (since 2026-10-24) runs
  only with a more accurate model installed, failed chunks, or Always; a
  model's accuracy is the leaderboard's English figure (the specialists'
  on their language), not measured on the person's own meetings, and the
  meeting's language is Settings' or Windows', not detected. A refine on
  another model holds a second model in memory while it runs. Only the
  calling thread runs at low priority (ONNX Runtime's and whisper.cpp's
  worker threads keep theirs): what keeps it out of a dictation's way is
  when it runs. A dictation shorter than one refine chunk's transcription
  can still wait for that chunk (a fraction of a second on a graphics
  card). The speaker engine isn't run
  again over it (the step toward speakers out of beta); it can't be
  exported; a transcription after the fact doesn't diarize. A line recovered
  into a meeting that was resumed and digested since isn't in the digest
  that covers its place (the next "Generate summary" after Transcribe again
  reads it). A write torn inside a frame's header on a power cut costs the
  quarter second before it too.
- **Who's who on the call.** Everyone remote is "Them" unless named (by the
  user, or as a guess from a 1:1 invite). Tasks are attributed from the
  attendee list, the speaker map and names said out loud ("Bob, can you…");
  a remote "I'll do it" from an unnamed "Them" is Unassigned. With the
  speaker engine on (beta), voices are told apart ("Speaker 1", "Speaker
  2"…) but not named: naming comes from the user, a 1:1 invite or, later,
  the call app's active speaker. Voices from two recordings of one meeting
  (a pause) aren't matched up automatically, and a line's name comes ~15 s
  after its words (the CPU preset; a GPU's 1 s preset is step 4), so the
  newest lines say "Them" for a moment.
- **Speakers.** Echo is caught only in chunks that are entirely the call
  coming through the speakers; a chunk where the user also spoke keeps the
  repeated words. Finer-grained removal (gating mic frames by the loopback
  signal, or echo cancellation) is a follow-up; until then the UI recommends
  headphones.
- **Turns within a chunk.** Each source is transcribed in ~15 s chunks, so a
  chunk can hold several turns of one side. Splitting chunks at long pauses
  would interleave turns better, at the cost of more engine calls (expensive
  for Whisper, which pads every call to 30 s).
- **Very long meetings** (8 h+) keep every decision, task and question but
  only a thinned set of key points; a hierarchical roll-up of old digests
  would keep more.
- **Token counts are estimates**, deliberately high. A real tokenizer per
  model isn't worth it at these margins.
- **Rate limits.** Catching up on a long backlog at the end makes several
  calls in a row; a provider that rate-limits (Groq's free tier) can fail
  one, and the error says which part. Running the action again resumes.
- **Dictation during a meeting chunk.** If a dictation finishes while a
  meeting chunk holds the warm engine, dictation loads a second copy (an
  older behaviour of the shared slot). Waiting briefly for the engine to come
  back would be cheaper.
- **Dictating mid-meeting.** The blanking is at callback granularity (10 ms)
  and assumes the two mic streams deliver within ~40 ms of each other. Meeting
  talk in the same chunk as a dictation is one "You" line, dated at its first
  speech, so talk after the dictation can show above the marker (as with
  turns within a chunk). "Silence other apps while you dictate" never
  touches a call app, nor any browser while a meeting records, so "Them"
  carries on through a dictation (until 2026-10-17 the old "Mute while
  recording" muted the whole output, the call included). Checked with the
  e2e player's signal; a real hotkey dictation mid-call wants a dev pass.
- **The output device.** Yap follows Windows' default ("console") output. A
  call app playing on another device, or on the default *communications*
  device when that differs, isn't heard; the quiet-side warning says so but
  can't fix it. Capturing every active output (a loopback stream per device,
  or process loopback of everything but Yap, which would also keep Yap's own
  chimes out of "Them") is the follow-up. A device that stops working without
  the default changing isn't reopened. The switch itself is tested through a
  simulation hook; a headset plugged in mid-call wants a dev pass.
- **Quiet sides.** An in-person meeting (nothing playing) gets the "Them"
  warning once. Sounds Yap or other apps play on the default output (a
  dictation chime, a notification) count as the call being heard and start a
  new quiet stretch. The level is a peak gate (0.008), so a mic that only
  picks up loud background noise counts as heard.
- **Guard rails.** Hiding from capture is checked through the window's
  affinity and one desktop capture, not yet in a real Teams/Zoom screen
  share. Stops Yap makes itself (the length limit, "Stop and summarise
  automatically", the shortcut) bring up the main window to write the
  action plan, until that's written in Rust. Win+Alt+M is also Wispr Flow's
  shortcut; with both running, whichever hook is newest gets it (Yap
  re-installs its hook every 30 s).

## Call detection

When a call starts in Teams, Zoom, Google Meet, a Slack huddle, Webex or another
work call app, Yap asks once, "Teams call detected. Record notes?". It records
only after a click, into a new meeting note. When the call ends while Yap is
recording it, Yap asks "Stop and summarise?". Personal chat apps (Discord,
WhatsApp, Signal, Telegram) aren't asked about unless switched on, any app can
be switched off from its prompt ("Don't ask for Teams"), and the prompts can
come as a pop-up or quietly. While a call is live, the tray menu offers
"Record this Teams call" either way. Code: `src-tauri/src/calls/meeting_detect.rs`
(+ `src/lib/stores/meetingDetect.svelte.js` for the in-app prompts,
`src-tauri/src/notices/win_toast.rs` for the Windows notifications, `src-tauri/src/shell/tray.rs`
for the tray item).

### How a call is detected

Windows keeps a per-app record of microphone use, the same data behind the
taskbar's microphone indicator and Settings → Privacy & security → Microphone:

```
HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone
  MSTeams_8wekyb3d8bbwe\                     packaged apps, by package family name
  NonPackaged\C:#Users#…#Discord.exe\        desktop apps, by exe path ('\' spelled '#')
    LastUsedTimeStart, LastUsedTimeStop      FILETIMEs; Stop == 0 → on the mic right now
```

- **Only known call apps count** (`APPS` in `calls/meeting_detect.rs`): Teams (new
  and classic), Zoom, Webex, Slack, Discord, GoTo Meeting, WhatsApp, Signal,
  Telegram. Only their subkeys' values are read.
- **Browsers** (Chrome, Edge, Firefox, Brave, Opera, Vivaldi, Arc, …) count when
  they hold the mic **and** one of their windows' titles names a meeting:
  "Meet - abc-defg-hij", "… | Microsoft Teams", "Zoom Meeting", Webex, Slack,
  Discord, Whereby, Jitsi. Once named, the call stays with that browser while it
  holds the mic, so switching tabs doesn't end it. Titles are read only while a
  browser holds the mic and no call is named yet, and are never logged or stored.
- **Yap itself never counts.** Yap holds the mic all the time for its pre-roll,
  plus dictation and the meeting recorder, and `yap.exe` isn't a call app. On
  the development PC the only entry "on the mic" was the installed Yap.
- An entry left "on the mic" from before the last restart is ignored (a
  `LastUsedTimeStart` older than boot means an app died mid-call).

Why not OpenWhispr's approach: OpenWhispr runs a helper exe
(`resources/windows-mic-listener.c`) on WASAPI capture sessions and prompts
for **any** process using the mic except its own. On a typical PC the mic
users include dictation apps, games, OBS and voice assistants. The consent
store names the app (a package or an exe path, not just a PID) and needs no
helper process.

### Cost

A thread blocks in `RegNotifyChangeKeyValue` on the `microphone` key (subtree,
value and subkey changes), so nothing runs until some app takes or releases
the mic. A scan then reads a handful of registry values. While a call is
starting, live or ending, the detector also rescans every 2 s. Otherwise it
rescans every 60 s as a safety net, or every 5 s if the watch can't start. It
is fully local: no network, no audio, no process list.

### Timing

| | Yap | OpenWhispr |
|---|---|---|
| Call starts | app on the mic for **5 s** (**20 s** for chat apps, whose voice messages use the mic too) | mic busy ≥ 2 s (event-driven) / 2 polls 15 s apart |
| Call ends | app off the mic for **15 s** (a headset switch or a rejoin is a gap, not an ending) | — (re-arms after 60 s quiet) |
| After "Not now" | that app's calls stay quiet for **5 min** | all detections quiet for 5 min |
| While dictating | the prompt waits until the dictation finishes, then **2.5 s** | queued, flushed 2.5 s after |
| Already recording | no start prompt | suppressed in meeting mode |
| Prompt left alone | the in-app prompt fades after **30 s**, as "Not now" | hidden after 30 s, as a dismissal (its cooldown starts) |

### Which apps it asks about

Each app in `APPS` has a default (`asks_by_default`): yes for work meetings,
no for personal chat apps, where notes on a call with friends would be an odd
thing to offer.

| Asked about by default | Not by default |
|---|---|
| Teams, Zoom, Google Meet, Webex, Slack, GoTo Meeting, Whereby, Jitsi Meet | Discord, WhatsApp, Signal, Telegram |

The person's choices are stored as overrides, `meetingDetectApps` in
`config.json` (app id → `true`/`false`), from Settings or from "Don't ask for
Teams" on a prompt. An app without an entry follows its default, so an app a
later release adds starts at its own default. A switched-off app's calls are
still noticed (they show in `meeting_detect_status`) but get no prompt, start
or end, and no tray item. Switching an app off withdraws a prompt about its
call at once; switching one on mid-call adds the tray item (the call's start
has passed, so no prompt).

### Asking

- **Once per call**, and never while Yap already records a meeting. Nor
  when the calendar asked already: a call during a meeting whose reminder
  card is up or was answered isn't asked about (`calendar::claims_call`; see
  [The calendar](#the-calendar)).
- **Pop-up** (the default, `meetingDetectStyle: "popup"`):
  - Main window on screen: an in-app toast ("Teams call detected", "Record
    notes? Let people know you're taking notes.", **Record notes** / **Not
    now**, and a small **Don't ask for Teams** link on its own line under
    them, so long names like Google Meet fit; its ✕ means Not now). Left
    alone, it fades after 30 s, a countdown hairline along its bottom edge,
    and that counts as Not now (OpenWhispr hides its meeting prompt after
    30 s and treats it as a dismissal). It stays while the pointer is on it.
  - Main window hidden, minimized or behind the call app: a card on the
    **Yap bar** (`bar.rs`), after Wispr Flow's "Meeting detected" card: the
    call app's icon, "Teams call" over "● Now", a cream split button
    **[Yap] Record notes** whose **^** menu holds **Not now** and **Don't
    ask for Teams**, and a small **✕** on its top-left corner (Not now). It
    fades after 30 s as Not now, like the toast, and pauses while the
    pointer is on it. (The call ending asks in an ordinary bar card: "Teams
    call ended", Keep recording / Stop and summarise.) The icon is the app's
    own as installed on this PC, else its bundled mark or a letter in its
    colour (see [App icons](#app-icons)). With the bar off or hidden for an
    hour, a silent Windows notification with Yap's logo and the same three
    answers as buttons, as before. If the window is open but not focused,
    both surfaces show; answering one withdraws the other. Focusing the
    window moves a pending prompt into it.
  - Over a fullscreen app: a borderless one (a game in a borderless window,
    a video, a browser in F11) still gets the card, over it, without the
    idle pill (Wispr's card appeared over a game). An exclusive-fullscreen
    game or a slideshow (`SHQueryUserNotificationState`) doesn't: the card
    waits, unseen, and shows when you alt-tab out, unless the call has ended
    by then (no countdown runs meanwhile, and Esc stays the game's).
  - **Start notes automatically after 10 seconds** (Settings → Meetings →
    Call detection, `meetingAutoStart`, **off** by default: nothing records without
    a click unless you switch it on). Wispr's card starts notes after a
    countdown; Yap's does too when this is on: "Notes start in 7…" with a
    ring draining round the Yap logo on the bar's card ("Starting notes in
    7…" on the in-app toast), **Start now**, Not now or **Esc** to cancel,
    then Record notes as if clicked (after any dictation in progress). Only
    where the countdown is sure to be seen — the bar's card or a focused
    window's toast — and never in the quiet style. The same app's call again
    within 10 minutes (a rejoin, a reload) is asked about without a
    countdown, as Wispr does.
- **Quietly** (`"quiet"`): no in-app toast. The Windows notification goes
  straight into the notification centre without a banner
  (`ToastNotification.SuppressPopup`), and the prompt stays out of the window
  (clicking the notification's body brings it in). The "call ended" prompt
  follows the same style. Portable Yap and test runs post no notifications,
  so there a quiet prompt is just the tray item.
- **Don't ask for Teams** saves Teams as switched off, withdraws the prompt
  everywhere, and with the window on screen confirms it there: "Won't ask
  about Teams calls" with **Open Settings** (Meetings → Ask about calls in).
- **The tray**, in both styles: while a call of an app Yap asks about is live
  and nothing records, the menu's top item is **Record this Teams call**
  (same as Record notes; it also answers a prompt still up for that call). So
  a prompt that faded, went quietly to the notification centre or got "Not
  now" still leaves a way to record.
- The call ending before anyone answers withdraws the prompt and its Windows
  notification.
- Windows notifications follow Do Not Disturb and Yap's notification switch.
  Portable Yap and test runs post none, so their pop-up prompts show in the
  window (and on the bar). "Taking notes on your Teams call" and "Couldn't
  record the call", feedback on a click, keep their banner in either style
  (on the bar: a card that fades after 8 s). Update notifications
  (`updates.rs`) don't follow the style; with the bar on, "Yap X is ready"
  is a bar card too, as are the guard rails' notices (the length warning
  with Keep going, the screen-share tip, "Stopped at 2 hours").

### Record notes

Creates a note through the notes API (`notes::create` + `notes::mark_meeting`):

- title **"Teams call · 5 Oct, 14:30"** (local time),
- folder **Meetings**, `noteType` **meeting**, `source` **meeting**,

then starts the existing recorder (`meeting_start`: mic = "You", system audio =
"Them"). During a calendar meeting the note then takes the meeting's name,
attendees and invite ([The calendar](#the-calendar)). With the window visible the note opens in Notes. With it hidden, a
Windows notification says "Taking notes on your Teams call" and offers **Open
note**. If recording can't start (no microphone, no speakers), the note is
deleted and the prompt says why.

### When the call ends

If Yap is recording that call, it asks "Teams call ended. Stop recording and
summarise your notes?" with **Stop and summarise** / **Keep recording**, in
the chosen style. It doesn't fade: it's about a recording that's still
running. "That
call" means the recording its prompt started, or one started during the call.
**Stop and summarise** ends the meeting, so the action plan is written exactly
as after **End meeting & summarise** (in Rust, `meetings/meeting_end.rs`), and shows
it where the person looks: the meeting notepad when it's on screen, the note
in the main window's Notes view when that's on screen, and with neither, the
main window comes up on the note. A call that starts while
Yap still records the note "Record notes" started (a rejoin, or after **Keep
recording**) carries that recording on. If the same app rejoins while this
prompt is up, the prompt is withdrawn, and the call's own end asks again. A
recording started by hand before the call, such as an in-person meeting, is
never offered for stopping just because a call ended.

By default Yap asks rather than auto-stopping, following OpenWhispr: its
engine never ends a meeting recording by itself, and its calendar "meeting
ended" event isn't acted on. The mic signal also can't tell a finished call
from one moved to a phone, a breakout room or a dropped connection that's
about to rejoin. Auto-stopping on a guess would cut meetings short and
summarise half a transcript. The person decides both ends: nothing records
without a click, and nothing stops without one, unless they choose **When a
call ends: Stop and summarise automatically** (Wispr Flow's "Stop Notetaker
when a call ends"; see [Guard rails](#when-a-call-ends)), which skips the
question and says so.

### Setting

Settings → Meetings: **Detect calls and offer to take notes**
(`meetingDetection`, default **on**, like OpenWhispr's
`notifyMeetingDetection`). It only reads Windows' own record of mic use,
locally, and only ever asks, and the prompt is how people find out notes
exist. Under it, greyed out while it's off:

- **How Yap asks**: Pop-up / Quietly (`meetingDetectStyle`), with a line on
  what the choice means.
- **Ask about calls in**: a row per call app in two columns, [its icon]
  [name] [switch], the switch showing what Yap does now (the person's
  choice, else the default above). Flipping one saves it in
  `meetingDetectApps`. The icons ([App icons](#app-icons)) line up in both
  columns and grey out with the switches.

Then the [guard rails](#guard-rails): when a call ends (greyed out with
detection, as call ends come from it), the maximum recording length, hiding
meeting windows from screen sharing; the [meeting notepad's](#the-meeting-notepad)
**Open the notepad when a meeting starts** and **Split the screen when
joining**; the meeting shortcut; and **Show live transcript** (Wispr's order:
detection, while it's active, the notepad, the transcript).

An always-visible line at the bottom reminds people of consent: "Recording a
call? Let people know you're taking notes." Changes go through the usual
config save, and the detector applies them at once (`meeting_detect::sync`).
Switching detection off withdraws any prompt (a recording in progress carries
on) and stops all detection work.

### App icons

The call apps show their real icons, as installed on this PC, beside the
switches in Settings and on the bar's call card, the way Wispr's "Meeting
detected" card shows Discord's. One component draws them
(`src/lib/components/CallAppIcon.svelte`), in this order:

1. **The app's own icon as installed here** (`calls/app_icons.rs`, the
   `meeting_app_icons` command);
2. its bundled mark (`src/lib/bar/callApps.js`: Simple Icons, CC0);
3. a letter badge in its colour: Teams, Slack, Webex and Whereby, which
   Simple Icons doesn't carry (their owners had them removed);
4. a phone, for anything else.

Google Meet, Whereby and Jitsi Meet run in a browser, so they always show
their mark or letter.

Where Yap finds the installed icons, read-only and local (nothing launched,
no network, only icon and manifest files read):

- **Desktop apps**: the exe paths in the microphone record call detection
  reads anyway (`NonPackaged\<path>`, the most recently used first), then
  each app's usual install folders (`%LOCALAPPDATA%\Discord\app-*\Discord.exe`,
  `%APPDATA%\Zoom\bin\Zoom.exe`, Slack, Webex, GoTo, Signal, Telegram…; for a
  Squirrel `app-*` folder, the newest that has the exe). Windows draws the
  exe's icon at 48 px (`SHDefExtractIconW`); Yap reads its pixels (alpha, or
  the mask for an old-style icon) and saves them as a PNG.
- **Packaged apps** (new Teams, WhatsApp, Telegram or Slack from the Store):
  the family names in the microphone record, plus the known ones for these
  apps (an app installed but never on the mic isn't in the record), lead to
  the install folder (`GetPackagesByPackageFamily`,
  `GetPackagePathByFullName`). Its `AppxManifest.xml` names the app list logo
  (`Square44x44Logo` of the application shown in the app list), and of that
  logo's variants Yap takes the one made for a light background at about
  48 px (`…targetsize-48_altform-lightunplated.png`), never a high-contrast
  or dark-theme one while there's another.

The icons are cached in `<data>/icons/<app>.png`, with `sources.json`
noting what each was made from (the file, its size and modified time). An
icon is made again when its source changes (an update) or after a week, and
an uninstalled app loses its icon. The command runs off the main thread;
each window asks the first time it draws an icon and again 10 minutes later,
showing the bundled marks until the answer comes. With **Detect calls** off,
Yap doesn't read the microphone record for icons either; the usual install
folders still find most apps.

On the development PC: Discord (its exe), Teams and WhatsApp (both from the
Store) resolve to their own icons.

### Testing

- Unit tests (`cargo test --lib meeting_detect`): consent-store key names → apps,
  dictation apps/games/Yap never matching, browser titles, the stale-entry rule,
  the start/end debounce, note titles, prompt wording, the per-app defaults and
  the person's choice winning over them, no prompt for a switched-off app, the
  quiet style (no in-app toast, a quiet notification), the 30 s fade (start
  prompts only), the tray item, the Settings list's order, the
  notification XML (three answers on a start prompt; built into a WinRT toast,
  never shown, with `SuppressPopup` set only when quiet), the Yap bar's cards
  (the same answers; "Teams call" over "● Now", the call ending as a notice
  card; the fade, ✕ and Esc), and
  the auto-start countdown (off by default, only where it's sure to be seen,
  never quiet, never for a rejoin, never for an end prompt).
- Read-only check of this PC's microphone record:
  `cargo test --lib meeting_detect::tests::this_machine -- --ignored --nocapture`
  (prints exe/package names and recognised meetings only, never window titles).
- App icons (`cargo test --lib app_icons`): the manifest's logo (fixture
  XML: a hidden helper application first, comments, entities, Windows 8
  names, the store logo), the variant chosen for a light background at
  48 px (on disk too, from a resource package and qualifier folders, never
  outside the package), `%VARS%` and Squirrel `app-*` paths, the tables
  naming apps in `APPS`, the microphone record first, an icon's pixels back
  out of an HICON (alpha and mask icons) and through PNG, the cache (reused,
  remade after a change or a week, dropped with the app), and the stand-ins.
  The real thing, read-only, writing the cache where you can look at it:
  `YAP_ICONS_OUT=<folder> cargo test --lib app_icons::tests::this_machine -- --ignored --nocapture`.
- e2e (`npm run test:app`): `e2e/meeting-detect.spec.js` and
  `e2e/meeting-detect-no-mic.spec.js` drive the flow through the debug-only
  `meeting_detect_simulate { appId, active, fadeMs? }` hook (no debounce;
  `fadeMs` makes that call's prompt fade in a second or two instead of 30 s;
  test mode reads no registry). They cover the prompt, Not now, a prompt
  withdrawn with its call, the fade counting as Not now, Record notes (the
  note and the recording, which in test mode opens no audio device; with a
  configured microphone that isn't plugged in, the error and no note left
  behind), Stop and summarise, Discord not asked about by default, "Don't ask
  for Teams" (the confirmation, Settings showing it off, switching it back on),
  the quiet style, the Settings list (every row's icon: its kind, 20 px,
  before the name, lined up in two columns, greyed with detection off), and
  the master toggle. Test mode reads no installed apps' icons: the specs give
  Teams and Discord stand-ins (`YAP_E2E_APP_ICONS`, `e2e/support/icons.js`),
  so both kinds show. The tray item and the Windows notifications can't be
  driven over CDP: their logic is in the unit tests. `e2e/bar.spec.js`
  covers the prompt as a card on the Yap bar (its menu, its ✕, Teams'
  installed icon and Meet's mark), over a borderless and an exclusive
  fullscreen app, and the countdown (Esc cancels; left alone it records; the
  end card follows). A test run never counts the main window as focused, so
  the card shows whatever the desktop does around the suite.

### Limits

- Apps not in `APPS`/browser titles aren't detected (record manually from a note).
- Exe names for Webex and GoTo installs vary by version. Only Teams, Discord
  and a browser have been seen in the consent store on the development PC.
  The same goes for their icons: the install folders `calls/app_icons.rs` knows
  for Zoom, Slack, Webex, GoTo, Signal and Telegram are those apps' usual
  ones, not yet checked against a real install (the microphone record finds
  any that took the mic). A Store app outside the known families is found
  only once it's been on the mic.
- Simple Icons' Zoom mark is its wordmark, which reads small at 20 px (Zoom
  installed shows its own icon instead).
- The Windows notification's three buttons share its width, so a long label
  like "Don't ask for Google Meet" may be cut short there (the in-app link
  has a line of its own). Not yet seen on a real notification.
- A meeting tab in a browser is named from the windows' titles, i.e. the
  active tab of each window, so a call started in a background tab is named
  once its tab is shown.
- Windows only. Other platforms build but don't detect.

Sources: OpenWhispr `src/helpers/meetingDetectionEngine.js`
(`handleNotificationTimeout`),
`audioActivityDetector.js`, `meetingProcessDetector.js`,
`resources/windows-mic-listener.c`, `src/components/MeetingNotificationOverlay.tsx`,
`src/helpers/windowManager.js` (`showMeetingNotification`: the 30 s timeout),
`src/stores/settingsStore.ts` (`notifyMeetingDetection` default);
Microsoft, [ToastNotification.SuppressPopup](https://learn.microsoft.com/en-us/uwp/api/windows.ui.notifications.toastnotification.suppresspopup);
Microsoft, [RegNotifyChangeKeyValue](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regnotifychangekeyvalue);
the same consent-store technique in
[automattermostatus #17](https://gitlab.com/matclab/automattermostatus/-/issues/17)
(Teams' packaged entry sits directly under `microphone`).

## The calendar

With Google Calendar or a private iCal link connected (Settings →
Connectors; setup, privacy and how it works: [`calendar.md`](./calendar.md)),
meetings and their notes meet in four places:

- **The Meetings view** lists today and the next 7 days (Conflict, Maybe,
  Join meeting / Start / Join + Start from 10 minutes before), then past
  meeting notes with search and an Ask bar for questions about your
  meetings.
- **The reminder card** ("Notify before scheduled meetings start": right
  before, 1 or 2 minutes, never; Settings → Meetings): **Join &
  take notes**, **Start notes**, Snooze 2 min, ✕ — in the main window, and
  while it isn't focused on the **Yap bar** (`bar.rs`) as a call card like
  call detection's ("Design review · ● In 1 min · with Tanay +1"), else a
  Windows notification. While another meeting's
  notes record, it offers to **switch** notes: the running meeting ends
  (`meeting_end::end`, so its action plan is written) and the next one's
  notes start. A call starting during the next meeting while the last one
  still records brings the same switch card.
- **A note's meeting.** However a recording starts during a meeting (the
  card, the Meetings view, Record notes, Notes, the meeting shortcut),
  `calendar::on_meeting_started` ties the note to it (`Note::event`): the
  meeting's name over a made-up title (and the AI title then leaves it
  alone), its attendees as participants (who the action plan gives tasks
  to, and spelling hints for Whisper while it transcribes), and the invite's
  description, cleaned of links and dial-in boilerplate, as context for the
  action plan and the note's Ask bar (`calendar::invite_context`).
- **Not asking twice.** Call detection asks the calendar first
  (`claims_call`): a call during a meeting whose card is up, or was
  answered, gets no "Record notes?"; a snoozed card comes back instead.

Tested in `e2e/calendar.spec.js` (the card's Join & take notes recording
into a note with the meeting's name and attendees, its simulated call not
asked about again, Esc and snooze, back-to-back switching) and
`e2e/calendar-nudge.spec.js`.
