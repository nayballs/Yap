# Meetings

Yap records a meeting into a note, from any call app or none (no bot joins):
your microphone is "You", the computer's sound output is "Them". It
transcribes locally as the meeting goes, summarises every ~10 minutes in the
background, and at the end turns the whole meeting into an **action plan**: a
section per person with their tasks and deadlines, then decisions, open
questions and unassigned tasks, back in seconds however long the meeting ran.
It can also notice a call starting and offer to take notes
([Call detection](#call-detection)).

| Piece | Code |
|---|---|
| Recorder: capture, chunking, echo check | `src-tauri/src/meeting.rs` |
| Rolling digests, the final input, the checks | `src-tauri/src/meeting_summary.rs` |
| Prompts (`MEETING_DIGEST_PROMPT`, `ACTION_PLAN_BASE_PROMPT`, `ACTION_PLAN_DEFAULT_FRAGMENT`) | `src-tauri/src/llm.rs` |
| Storage (`Note::digests`, `TranscriptSegment::echo`, `Action::kind`) | `src-tauri/src/notes.rs` |
| The action run (`note_enhance`) | `src-tauri/src/commands.rs` |
| UI | `src/lib/NotesView.svelte` |

## Recording

**Record** on a note's chip row starts it; the chip becomes **● 12:34 · Pause**
(stop without summarising; **Resume** carries on in the same note) and the
bottom bar's action picker becomes **End meeting & summarise**.

- **Two streams.** The mic ("You") and WASAPI loopback ("Them": a cpal input
  stream on the default output device), each downmixed and resampled to
  16 kHz on its own capture thread.
- **Chunks cut between words.** Once a source has 15 s buffered, the worker
  cuts it at the quietest 20 ms frame of the last 4 s, leaving the newest
  0.3 s, where a word may still be going on (`media::quietest_frame`). It used
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
- **Saved as it comes.** Segments `{ source, text, ts }` are emitted live
  (`yap-meeting-segment`) and appended to the note on every chunk, so a crash
  loses at most one chunk. `ts` is when the segment's speech started (its
  first 20 ms frame of speech), which also orders a "You" and a "Them" chunk
  cut at the same moment.
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

**End meeting & summarise** (or a stop from elsewhere, like call detection's
**Stop and summarise**) stops the recorder, which transcribes the last few
seconds, then runs the built-in **Action Plan** action. **Pause** stops
without it. On a finished meeting note, **Action plan** in the transcript
box runs it again.

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
an AI model…" with **Open Language Models**. A recording where nothing was
transcribed and no notes were typed says "Nothing to summarise yet" instead
of calling a model.

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
  digest giving way to a dictation (`unless_busy`).
- **e2e** (`npm run test:app`), with no audio device and no real AI:
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
  - Harness: `support/fake-llm.js`, a deterministic OpenAI-compatible server
    that records every request and tests the checks by inventing an owner,
    dropping a task and slipping in a task for someone never mentioned.
    Test-mode hooks, all compiled out of release builds (`e2e.rs`):
    `YAP_E2E_MEETING_AUDIO` (a folder with `you.wav` and `them.wav`),
    `YAP_E2E_MEETING_SPEED` and the `e2e_meeting_feed { segments }` command.
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

## Call detection

When a call starts in Teams, Zoom, Google Meet, a Slack huddle, Discord, Webex
or another call app, Yap asks once, "Teams call detected. Record notes?". It
records only after a click, into a new meeting note. When the call ends while Yap is
recording it, Yap asks "Stop and summarise?". Code: `src-tauri/src/meeting_detect.rs`
(+ `src/lib/meetingDetect.svelte.js` for the in-app prompts,
`src-tauri/src/win_toast.rs` for the Windows notifications).

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

### Asking

- **Once per call**, and never while Yap already records a meeting.
- Main window on screen: a sticky in-app toast ("Teams call detected", "Record
  notes? Let people know you're taking notes.", **Record notes** / **Not
  now**; its ✕ means Not now).
- Main window hidden, minimized or behind the call app: a silent Windows
  notification with Yap's logo and the same buttons. If the window is open but
  not focused, both surfaces show; answering one withdraws the other.
  Focusing the window moves a pending prompt into it.
- The call ending before anyone answers withdraws the prompt.
- Windows notifications follow Do Not Disturb and Yap's notification switch.
  Portable Yap and test runs post none, so their prompts show in the window.

### Record notes

Creates a note through the notes API (`notes::create` + `notes::mark_meeting`):

- title **"Teams call · 5 Oct, 14:30"** (local time),
- folder **Meetings**, `noteType` **meeting**, `source` **meeting**,

then starts the existing recorder (`meeting_start`: mic = "You", system audio =
"Them"). With the window visible the note opens in Notes. With it hidden, a
Windows notification says "Taking notes on your Teams call" and offers **Open
note**. If recording can't start (no microphone, no speakers), the note is
deleted and the prompt says why.

### When the call ends

If Yap is recording that call, it asks "Teams call ended. Stop recording and
summarise your notes?" with **Stop and summarise** / **Keep recording**. "That
call" means the recording its prompt started, or one started during the call.
**Stop and summarise** opens the note and ends it from the Notes view, so the
action plan is written exactly as after **End meeting & summarise**. If the page
hasn't stopped it after 8 s, Yap stops it directly. A call that starts while
Yap still records the note "Record notes" started (a rejoin, or after **Keep
recording**) carries that recording on. If the same app rejoins while this
prompt is up, the prompt is withdrawn, and the call's own end asks again. A
recording started by hand before the call, such as an in-person meeting, is
never offered for stopping just because a call ended.

Yap asks rather than auto-stopping, following OpenWhispr: its engine never
ends a meeting recording by itself, and its calendar "meeting ended" event
isn't acted on. The mic signal also can't tell a finished call from one
moved to a phone, a breakout room or a dropped connection that's about to
rejoin. Auto-stopping on a guess would cut meetings short and summarise half a
transcript. The person decides both ends: nothing records without a click,
and nothing stops without one.

### Setting

Settings → General → Meetings: **Detect calls and offer to take notes**
(`meetingDetection`, default **on**, like OpenWhispr's
`notifyMeetingDetection`). It only reads Windows' own record of mic use,
locally, and only ever asks, and the prompt is how people find out notes
exist. An always-visible line under it reminds people of consent: "Recording a call? Let
people know you're taking notes." Switching it off withdraws any prompt
(a recording in progress carries on) and stops all detection work.

### Testing

- Unit tests (`cargo test --lib meeting_detect`): consent-store key names → apps,
  dictation apps/games/Yap never matching, browser titles, the stale-entry rule,
  the start/end debounce, note titles, prompt wording, and the notification XML
  (built into a WinRT toast, never shown).
- Read-only check of this PC's microphone record:
  `cargo test --lib meeting_detect::tests::this_machine -- --ignored --nocapture`
  (prints exe/package names and recognised meetings only, never window titles).
- e2e (`npm run test:app`): `e2e/meeting-detect.spec.js` and
  `e2e/meeting-detect-no-mic.spec.js` drive the flow through the debug-only
  `meeting_detect_simulate { appId, active }` hook (no debounce; test mode reads no
  registry). They cover the prompt, Not now, a prompt withdrawn with its call, Record
  notes (the note and the recording, which in test mode opens no audio device;
  with a configured microphone that isn't plugged in, the error and no note
  left behind), Stop and summarise, and the Settings toggle.

### Limits

- Apps not in `APPS`/browser titles aren't detected (record manually from a note).
- Exe names for Webex and GoTo installs vary by version. Only Teams, Discord
  and a browser have been seen in the consent store on the development PC.
- A meeting tab in a browser is named from the windows' titles, i.e. the
  active tab of each window, so a call started in a background tab is named
  once its tab is shown.
- Windows only. Other platforms build but don't detect.

Sources: OpenWhispr `src/helpers/meetingDetectionEngine.js`,
`audioActivityDetector.js`, `meetingProcessDetector.js`,
`resources/windows-mic-listener.c`, `src/components/MeetingNotificationOverlay.tsx`,
`src/stores/settingsStore.ts` (`notifyMeetingDetection` default);
Microsoft, [RegNotifyChangeKeyValue](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regnotifychangekeyvalue);
the same consent-store technique in
[automattermostatus #17](https://gitlab.com/matclab/automattermostatus/-/issues/17)
(Teams' packaged entry sits directly under `microphone`).
