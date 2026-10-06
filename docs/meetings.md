# Meetings

Yap records a meeting into a note, from any call app or none (no bot joins):
your microphone is "You", the computer's sound output is "Them". It
transcribes locally as the meeting goes, summarises every ~10 minutes in the
background, and at the end turns the whole meeting into an **action plan**: a
section per person with their tasks and deadlines, then decisions, open
questions and unassigned tasks, back in seconds however long the meeting ran.
It can also notice a call starting and offer to take notes
([Call detection](#call-detection)), keeps a recording in bounds (its
windows stay out of screen shares, it stops at a maximum length, it can stop
when the call ends, and a shortcut starts and stops it:
[Guard rails](#guard-rails)), and while a meeting records, a
[meeting notepad](#the-meeting-notepad) docks to the edge of the screen beside
the call. With a calendar connected, the Meetings view lists what's coming,
a card offers **Join & take notes** just before each meeting, and a note
takes its meeting's name, attendees and invite ([The calendar](#the-calendar),
[`calendar.md`](./calendar.md)).

| Piece | Code |
|---|---|
| Recorder: capture, chunking, echo check, dictations kept out, following the output device, quiet sides | `src-tauri/src/meeting.rs` |
| The dictation signal; the correction dictionary (`apply_corrections`) | `src-tauri/src/pipeline.rs` |
| Guard rails: maximum length, the meeting shortcut, notices | `src-tauri/src/meeting_guard.rs` |
| Hiding meeting windows from screen capture | `src-tauri/src/capture.rs` |
| Rolling digests, the final input, the checks; the notepad's catch-up and title inputs | `src-tauri/src/meeting_summary.rs` |
| The end of a meeting: pause or end, "Started by mistake?", the action plan job | `src-tauri/src/meeting_end.rs` |
| "What did I miss?", the AI meeting title | `src-tauri/src/meeting_assist.rs` |
| The notepad window: docking, split screen | `src-tauri/src/notepad.rs` |
| The calendar: meetings, the reminder card, a note's meeting | `src-tauri/src/calendar.rs` (+ `calendar/`; [`calendar.md`](./calendar.md)) |
| Prompts (`MEETING_DIGEST_PROMPT`, `ACTION_PLAN_BASE_PROMPT`, `ACTION_PLAN_DEFAULT_FRAGMENT`, `CATCH_UP_PROMPT`, `MEETING_ASK_PROMPT`, `MEETING_TITLE_PROMPT`) | `src-tauri/src/llm.rs` |
| Storage (`Note::digests`, `TranscriptSegment::echo` / `::dictated`, `Action::kind`, `Note::title_auto`) | `src-tauri/src/notes.rs` |
| The action run (`note_enhance` / `run_enhance`) | `src-tauri/src/commands.rs` |
| UI | `src/lib/NotesView.svelte`, `src/lib/Notepad.svelte`, `src/lib/meetingSummary.svelte.js` |

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
  check it isn't muted, and pick the right mic in Settings → General."

Both sides quiet is a break, not a fault, and a cough isn't talk. The warning
is the existing `yap-meeting-warning`: a toast in the main window, in
whichever view is open (`meetingGuard.js`, 12 s), and in the meeting notepad.
When neither is in view (the notepad isn't on screen and the main window
isn't focused: the person is in their call app), it's also a Windows
notification ("Yap can't hear the call", or "…your mic" with **Update
settings**). The recording carries on either way. An in-person meeting (no
call) gets the "Them" warning once too; its wording says "If".

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
built-in **Action Plan** action (`meeting_end.rs`, following the recorder's
`yap-meeting-state`). **Pause** (`meeting_pause`) stops without it. It no
longer depends on a window: until 2026-10 the Notes view wrote the plan, and
only for the note it was showing. Every window follows the job through
`yap-meeting-summary` (`running` with its step — 1 reading the meeting, 2
writing, 3 checking owners and deadlines — then `done`, `error`, `needsAi` or
`nothing`). On a finished meeting note, **Action plan** in the transcript box
(or the notepad's **Generate summary** / **Retry**) runs it again
(`meeting_summarise`).

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
5. The result is the note's Enhanced view, with **Copy markdown** and **Copy
   text** (headings plain, tasks as ☐, for a chat message or an email).

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

## The meeting notepad

While a meeting records, its notes live in a slim window docked to the right
edge of the screen, beside the call (Wispr Flow's Notetaker notepad, ported to
Yap: `src-tauri/src/notepad.rs`, `src/lib/Notepad.svelte`). Its look and
behaviour follow Wispr's notepad as measured from its DOM
(`E:\Projects\references\wispr-flow\notepad-spec.md`: sizes, type, colours,
motion), in Yap's own code, wording and icons; the palette is the
`--yap-paper-*` tokens in `src/app.css`.

**When it opens.** Whenever a meeting recording starts, from anywhere ("Record
notes" on a call prompt, the Notes view's Record, the tray, the Yap bar), with
**Open the notepad when a meeting starts** on (Settings → General → Meetings,
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
recording: Settings → General), Save as .md (a save dialog, the same file as
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
"Them: …" lines), with a tip under it you can dismiss for good (lines land
about every 15 seconds as Yap transcribes on this PC, the last few when you
stop). Then the lines as they arrive, following the newest one unless you
scroll up: a speaker's run of lines is one group under its label (You, or
Them in teal), a chat bubble per line with grouped corners, each with a copy
button on hover. A dashed line with ⏸ marks where a recording stopped
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
(with a **What did I miss?** chip inside it while recording). Above it while
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
  title inputs, title clean-up, and a typed title never open to the AI.
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
- Speaker labels are You / Them (the mic and the call); naming the people on
  the call is a later item (calendar attendees, diarization). Yap doesn't
  re-transcribe or relabel speakers after the meeting (Wispr does both in its
  cloud).
- Share is local: no links to the notes or shared access (Wispr's sharing is
  its cloud's). Email hands a `mailto:` link to the mail app, so a long
  summary is cut to fit.
- The 👍 👎 ratings stay in the notepad's own storage; nothing reads them yet.
- "Since you last looked" lives in the notepad page: reloading the window (or
  restarting Yap) forgets it, and the next "What did I miss?" covers the whole
  meeting.

## Guard rails

Wispr Flow's Notetaker settings, ported (Settings → General → Meetings, one
block under "Ask about calls in"). Code: `src-tauri/src/meeting_guard.rs`,
`src-tauri/src/capture.rs`, `src/lib/meetingGuard.js`.

| Setting | Config | Default | Wispr's |
|---|---|---|---|
| When a call ends: **Ask me** / **Stop and summarise automatically** | `meetingCallEnd` `"ask"` / `"stop"` | Ask me | "Stop Notetaker when a call ends", on |
| **Maximum recording length**: 1, 2, 3, 4 hours / No limit | `meetingMaxMinutes` (0 = none) | 2 hours | the same, 2 hours |
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
- **e2e** (`npm run test:app`), with no audio device and no real AI:
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
  - Capture hiding, once by hand (2026-10-05): a GDI desktop capture of just
    the overlay's rectangle (the path screenshot tools use) while a meeting
    recorded, with the overlay shown. Excluded: identical to the background
    (0 % of pixels differ); hiding off: the capsule shows (27 % differ). So
    the flag works on Tauri's transparent WebView2 overlay.
  - Unit tests (`cargo test --lib -- meeting_guard capture input_hook`): the
    length guard's steps (on time, late, raised, removed, Keep going), the
    wording, the shortcut's label, the notification XML, Win combos in hotkey
    specs and which combos need the menu-mask key.
  - Harness: `support/fake-llm.js`, a deterministic OpenAI-compatible server
    that records every request and tests the checks by inventing an owner,
    dropping a task and slipping in a task for someone never mentioned. It
    also answers the notepad's title, catch-up and question calls, and can
    fail (`failNext`) or slow down (`delay`) a kind of call, for the summary's
    progress and error states.
    Test-mode hooks, all compiled out of release builds (`e2e.rs`):
    `YAP_E2E_MEETING_AUDIO` (a folder with `you.wav` and `them.wav`),
    `YAP_E2E_MEETING_SPEED`, the `e2e_meeting_feed { segments }` command
    (corrected with the dictionary like real lines), and for the recorder's
    audio `e2e_meeting_dictation { from, to }`, `e2e_meeting_output_change
    { at? }` (seconds of the test audio) and `e2e_meeting_quiet { quietSecs?,
    talkSecs? }`. The player logs "the test audio has played to the end", so
    a test can stop after all of it.
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

- **Who's who on the call.** Everyone remote is "Them". Tasks are attributed
  from the attendee list and names said out loud ("Bob, can you…"); a remote
  "I'll do it" with no name is Unassigned. Speaker diarization (OpenWhispr's
  `liveSpeakerIdentifier.js`) is the follow-up.
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
  turns within a chunk). With **Mute while recording** on, the call itself is
  muted during a dictation, so "Them" misses that stretch too. Checked with
  the e2e player's signal; a real hotkey dictation mid-call wants a dev pass.
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
"Record this Teams call" either way. Code: `src-tauri/src/meeting_detect.rs`
(+ `src/lib/meetingDetect.svelte.js` for the in-app prompts,
`src-tauri/src/win_toast.rs` for the Windows notifications, `src-tauri/src/tray.rs`
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

- **Only known call apps count** (`APPS` in `meeting_detect.rs`): Teams (new
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
    call app's mark, "Teams call detected" over "● Now", a light split
    button **[Yap] Record notes** whose **^** menu holds **Not now** and
    **Don't ask for Teams**, and a small **✕** on its top-left corner (Not
    now). It fades after 30 s as Not now, like the toast, and pauses while
    the pointer is on it. The marks are Simple Icons glyphs (CC0) where
    Simple Icons has them; Teams, Slack, Webex and Whereby get a monogram in
    their colour, anything else a phone. With the bar off or hidden for an
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
  - **Start notes automatically after 10 seconds** (Settings → General →
    Meetings, `meetingAutoStart`, **off** by default: nothing records without
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
  about Teams calls" with **Open Settings** (General → Meetings).
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
as after **End meeting & summarise** (in Rust, `meeting_end.rs`), and shows
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

Settings → General → Meetings: **Detect calls and offer to take notes**
(`meetingDetection`, default **on**, like OpenWhispr's
`notifyMeetingDetection`). It only reads Windows' own record of mic use,
locally, and only ever asks, and the prompt is how people find out notes
exist. Under it, greyed out while it's off:

- **How Yap asks**: Pop-up / Quietly (`meetingDetectStyle`), with a line on
  what the choice means.
- **Ask about calls in**: a switch per call app in two columns, showing what
  Yap does now (the person's choice, else the default above). Flipping one
  saves it in `meetingDetectApps`.

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

### Testing

- Unit tests (`cargo test --lib meeting_detect`): consent-store key names → apps,
  dictation apps/games/Yap never matching, browser titles, the stale-entry rule,
  the start/end debounce, note titles, prompt wording, the per-app defaults and
  the person's choice winning over them, no prompt for a switched-off app, the
  quiet style (no in-app toast, a quiet notification), the 30 s fade (start
  prompts only), the tray item, the Settings list's order, the
  notification XML (three answers on a start prompt; built into a WinRT toast,
  never shown, with `SuppressPopup` set only when quiet), the Yap bar's card
  (the same answers, "● Now" / "● Still recording", its fade, ✕ and Esc), and
  the auto-start countdown (off by default, only where it's sure to be seen,
  never quiet, never for a rejoin, never for an end prompt).
- Read-only check of this PC's microphone record:
  `cargo test --lib meeting_detect::tests::this_machine -- --ignored --nocapture`
  (prints exe/package names and recognised meetings only, never window titles).
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
  the quiet style, the Settings list, and the master toggle. The tray item and
  the Windows notifications can't be driven over CDP: their logic is in the
  unit tests. `e2e/bar.spec.js` covers the prompt as a card on the Yap bar
  (its menu, its ✕, an app's mark), over a borderless and an exclusive
  fullscreen app, and the countdown (Esc cancels; left alone it records; the
  end card follows). A test run never counts the main window as focused, so
  the card shows whatever the desktop does around the suite.

### Limits

- Apps not in `APPS`/browser titles aren't detected (record manually from a note).
- Exe names for Webex and GoTo installs vary by version. Only Teams, Discord
  and a browser have been seen in the consent store on the development PC.
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
  before, 1 or 2 minutes, never; Settings → General → Meetings): **Join &
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
