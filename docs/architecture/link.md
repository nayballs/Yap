# Yap Link, home preview (phone ↔ PC)

**What it is** (2026-10-31): talk to Yap's AI from your **phone** while you
walk round the house — a Bluetooth headset paired to the phone, the answer in
your ear. The phone opens a page Yap serves itself over HTTPS on the home
Wi-Fi; a WebSocket carries the question's audio up and Yap's voice down. The
question goes the **Voice Assistant's** way (`chat/assistant.rs`): Chat's model
with its chosen effort, the short spoken answer, the conversation choice, the
answer kept in Chat. Off by default: **Settings → Phone → "Use Yap from your
phone (on your home Wi-Fi)"** (`link_enabled`).

**And dictation from the phone** (2026-10-31): the page's mode switch, **Ask
Yap · Dictate to PC · Dictate to phone**. A dictation is an ordinary one on
the PC with the phone as its microphone (`pipeline/phone.rs`): "Dictate to
PC" is typed into the window that was in front when Talk was pressed (with
the phone's **"May type into this PC"**), "Dictate to phone" comes back to
the phone, which copies it. [Below](#dictation-from-the-phone).

The long-term design (yap-private `docs/phone-remote-design.md`: a PWA on
Yap's website, WebRTC, away from home through a relay) keeps this protocol's
messages: moving there is a change of transport only.

## Modules (`src-tauri/src/link.rs` + `link/`)

| File | What it does |
|---|---|
| `link.rs` | On/off (`sync`, at start and on every save), the `Hub` the server shares (the invitation, the hello limiter, the pairing questions waiting, the connected phones), the commands, the events. |
| `link/server.rs` | HTTPS on TCP **51443**, every interface: who may connect, TLS, one HTTP request at a time, the page's files, `/ws`. |
| `link/cert.rs` | The self-signed certificate (rcgen, ECDSA P-256, on ring), kept in `<data>/link/`. |
| `link/pairing.rs` | The QR's one-time token, the 4-digit code, who may say hello (`judge`, pure), hellos per address (`Limiter`). |
| `link/devices.rs` | Paired phones in `link_devices.json` (a hash of each phone's secret only; "May type into this PC", `typeIntoPc`). |
| `link/session.rs` | One phone's conversation: the hello, pairing, its questions and dictations one at a time, interrupt, cancel. |
| `link/dictate.rs` | Dictation from the phone: which talk a `start` is, who may dictate and when (`refusal`), the words for what became of it (`outcome`), the phone's end of a dictation (`Dictating`, the pipeline's `PhoneSink`). |
| `link/voice.rs` | Yap's voice for the phone: the speaker's output sent down the socket, never this PC's speakers. |
| `link/models.rs` | Chat's model and the others it can use, listed for the phone and switched from it as the picker does. |
| `link/lan.rs` | This PC's addresses (IP Helper), which peers may connect, Windows' Public networks (Network List Manager). |
| `link/wire.rs` | The protocol (below), pure. |
| `link/page.rs` | The page's files, embedded at build time from `src-tauri/link-page/` (`build.rs` lists them; its README.md left out), their content types and the Content-Security-Policy. |

The phone's page itself (`src-tauri/link-page/`: plain HTML, CSS and JS
modules, no build step) has its own notes in its
[README.md](../../src-tauri/link-page/README.md) and tests in `e2e-link/`.

## The protocol (v1)

**WebSocket** at `wss://<pc>:51443/ws`. **Text frames** are JSON
`{ "type": … }`. **Binary frames** are audio: byte 0 the kind — `0x01` **up**
(the phone's mic, 16 kHz mono Int16 LE) or `0x02` **down** (Yap's voice, mono
Int16 LE at `speak_start`'s rate) — bytes 1–4 `seq` (u32 LE), the rest samples.
A frame is at most a second of audio (32,005 bytes); a text frame at most 16 KiB.

**Phone → PC**

| Message | Meaning |
|---|---|
| `hello {v:1, name, nonce, device?:{id, secret}, pair?:{token}}` | The first message. `device` on a later visit, `pair` from the QR. |
| `start {mode:"ask"}` | A question starts; up frames follow, 20–40 ms each (the page sends exactly 320 samples, `seq` from 0 for each question). No `mode` (an older page): a question. |
| `start {mode:"dictate", target:"pc"\|"phone"}` | A **dictation** starts (since 31 Oct; no `target`: `"pc"`); up frames follow as for a question. `"pc"`: typed into the window in front on the PC; `"phone"`: the text comes back. |
| `stop` | The question (or dictation) ended (after its last frame): transcribe and answer (or type). |
| `cancel` | Drop the question (listening, or being worked on); call a dictation off (listening: cancelled on the PC, kept there a moment for Undo; being typed: nothing pasted). |
| `interrupt` | Stop speaking and abandon the answer. |
| `set_model {id}` | Switch Chat's model (one of `ready.models`). |
| `ping` | Every 15 s; answered at once, mid-answer too. |

**PC → phone**

| Message | Meaning |
|---|---|
| `pair_wait {code}` | The pairing waits for a yes on the PC, which shows the same code. |
| `paired {device:{id, secret}}` | Paired: the phone keeps these for its next visits. |
| `denied {reason}` | Refused, in words the phone can show; the socket closes. |
| `ready {pc, model:{id,label,onThisPc}, models:[{id,label,onThisPc}], mayType}` | Connected: this PC's name, Chat's model (or `null`: none set up) and the others, and whether this phone may type into the PC (`mayType`, Settings → Phone). Sent again whenever Chat's model changes (here, in Chat, or from a phone: `yap-chat-model-changed`), after every `set_model`, which the page takes as the truth, and when the phone's "May type into this PC" is switched. |
| `state {phase}` | `"idle"` \| `"listening"` \| `"transcribing"` \| `"thinking"` \| `"speaking"`. A dictation: listening → transcribing → idle. |
| `heard {text, to?}` | The question, transcribed and corrected — or a **dictation's text as it was pasted** (cleaned up, corrected, fitted) or sent back, with `to` (`"pc"` or `"phone"`; none on a question's). |
| `pasted {ok, app?}` / `pasted {ok:false, reason}` | A dictation to the PC: pasted into `app` (its name, "Notepad"; none when Yap couldn't tell), or not, and why in plain words ("No text box in front: copied on the PC instead"). |
| `step {label}` | What the answer is doing, in Chat's activity line's words (`chat/activity.rs`: "Searching the web · …"). |
| `answer {text, final}` | **The whole answer so far** (each replaces the last; `""` = words taken back before a tool call); `final:true` with the checked answer. |
| `speak_start {rate}`, down frames, `speak_end` | Yap's voice (24 kHz). |
| `error {message}` | In plain words. |
| `pong` | |

A model's `id` is `provider|model` (`groq|openai/gpt-oss-20b`, `ondevice|`), a
self-hosted or custom server's `provider|model|address`. Only an id `ready`
listed can be picked. Its `label` is the name and where it runs ("GPT OSS 20B ·
Groq", "llama3 · localhost:11434"), except the on-device model's, which is its
name alone ("Qwen3.5 4B"): the page's own "On this PC" tag (from `onThisPc`)
says where, once.

**Rules.** One question at a time per phone. A `start` while Yap thinks or
speaks abandons that question first (an interrupt, then a start). `cancel` and
`interrupt` are answered with `state idle` at once (the page ignores the old
question's late news until it sees idle or listening); an abandoned question's
late `heard`, `step`, `answer`, `error` and audio never go out (frames already
queued are dropped by the writer). Yap never says `idle` while the phone is
listening unless it ended the question itself. A question is at most 2 minutes
(then it's asked as it is). An unpaired or bad hello gets `denied` and a close.

**A dictation's rules.** One talk at a time: a dictation is the current talk
from its `start` (a later `start` of any kind makes its late news go
unsaid; the PC still types it). A `start` while one is still listening calls
it off; one still being typed carries on, and a new dictation is refused
until it's done. Refused with `error` then `state idle`: "Dictate to PC"
without the phone's "May type into this PC" (said first: it's the one to
fix), either kind while a meeting records, or beside a dictation under way
on the PC (recording, or still being transcribed or pasted). The PC can stop
a phone's dictation itself — its shortcut, the bar's ✓, its 15-minute limit
— and the phone hears `state transcribing` while it still thinks it's
listening (the page stops then); the bar's ✕ (or Esc) cancels it:
`error` "Cancelled on your PC." and idle. An unknown `mode` or `target`
gets an `error` in words.

## Pairing

1. Settings → Phone → **Pair a phone** (`link_pair_start`): an invitation — a
   **128-bit** random token, **good once, for 5 minutes**; a new one replaces
   it — shown as a QR code of `https://<address>:51443/#pair=<token>` and as
   the link with Copy. The address is the PC's best on the router's network
   (`lan::reachable`: one with a gateway, Wi-Fi or Ethernet, IPv4, the home
   ranges), or another the person picks when the PC has several.
2. The phone opens it (accepting the certificate once), and says `hello` with
   `pair.token` and a fresh `nonce`. The token is used up there and then,
   whatever comes next (a replay: "already used").
3. Yap answers `pair_wait {code}` and the main window asks **"Allow ‘Android ·
   Chrome’ to use Yap?" with "Code 4821"** (`components/LinkPairPrompt.svelte`
   on `ui/ConfirmDialog.svelte`: Allow / Deny; Esc is a no). **The code** is
   SHA-256 of the UTF-8 text `"<token>:<nonce>"`, its first 4 bytes as a
   big-endian number, modulo 10,000, as 4 digits (`pairing::code`). The page
   shows `pair_wait`'s code; it can work it out itself the same way
   (WebCrypto's SHA-256). No answer in 2 minutes, the phone leaving or Link
   going off: denied.
4. **Allow**: a device id (`p-…`) and a **256-bit** secret go to the phone
   (`paired`); Yap keeps only the secret's SHA-256 with the name, when it was
   paired and last used (`link_devices.json`). Later visits say `hello` with
   `device`, compared in constant time; a wrong secret and an unknown phone read
   the same ("This phone was removed in Yap. Pair it again in Settings →
   Phone.").
5. Settings → Phone lists the paired phones (connected now, or when last used)
   with **Remove**: gone from the store, and a phone connected now is cut off
   at once (`denied`, the socket closed).

## Security

- **Home network only.** A peer that isn't on a private (10/8, 172.16/12,
  192.168/16), link-local (169.254/16, fe80::/10), unique-local (fc00::/7) or
  loopback (127/8, ::1) address is dropped before a byte is read
  (`lan::allowed_peer`; an IPv4 address written as IPv6 is judged as IPv4).
- **Not on a Public network.** A connection that came in on an address of a
  network Windows marks **Public** (a café's, a hotel's) is dropped
  (`lan::public_network`: the Network List Manager's connected networks and
  their adapters, matched to the local address by IP Helper; read off the
  async runtime, kept 30 s). When Windows can't say, the address rule alone
  decides. Settings → Phone says so when the QR's address is on a Public
  network ("Windows treats this network as Public", `link_status.public`), with
  **Open network settings** (`ms-settings:network-status`): a home network
  left as Public (Windows' default when you said no to "find devices") has to
  be made Private first — Windows' firewall asks about private networks only,
  too.
- **HTTPS** with Yap's self-signed certificate (rustls on ring). Its names: the
  PC's reachable addresses, `<name>.local`, `localhost`, 127.0.0.1, ::1. The
  phone warns once ("Your connection is not private" → Advanced → Proceed);
  Settings → Phone says so in one line. The page needs HTTPS for the
  microphone (`getUserMedia` wants a secure context).
- **The page from this PC only**: `default-src 'self'; script-src 'self';
  style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self'
  blob:; font-src 'self'; connect-src 'self' wss://<host>; worker-src 'self'
  blob:; manifest-src 'self'; object-src 'none'; base-uri 'none';
  frame-ancestors 'none'; form-action 'none'`, with `nosniff`, `no-referrer`,
  `Permissions-Policy: microphone=(self)` and `Cross-Origin-Opener-Policy`.
  No external requests.
- **The socket only from the page itself**: `/ws`'s `Origin` must be
  `https://<its Host>` (any web page may open a WebSocket anywhere), and the
  `Host` must be an address, `localhost`, a `.local` name or this PC's name (a
  rebound domain is refused).
- **Hellos per address**: at most 10 a minute; 5 refused in 10 minutes and that
  address waits out the 10 (`pairing::Limiter`). A hello must come within 10 s.
- **What a phone can do**: ask the Voice Assistant (its tools follow their own
  rules: opening apps on this PC, notes, the web), switch Chat's model,
  dictate to itself, and — with its **"May type into this PC"** — dictate
  into the window in front on this PC (a wireless keyboard, in effect:
  the bar's capsule shows it while it listens). It can't start meeting notes
  or read anything else; nothing it says on the PC is read back to it but its
  own dictation's text.
- **"May type into this PC"** (per phone, Settings → Phone, `link_set_typing`;
  `typeIntoPc` in `link_devices.json`): on for a phone paired from 31 Oct —
  the pairing question names it ("…and type what you dictate on it into this
  PC") — off for one paired before (it was paired to ask, not to type), and
  off again if an older Yap saves the store without it (the safe way round).
  Checked at every `start`; switching it tells a connected phone at once
  (`ready`'s `mayType`, the page's hint under its mode switch).

## The voice loop

`start` → listening (up frames into a buffer, 2 minutes at most) → `stop` →
**transcribing**: the warm engine, lent as a dictation's is and never a second
copy (`pipeline::PhoneEar` → `Shared::phone_question`: `lease_engine`, My
languages, the Whisper prompt), then the correction dictionary (exact, then
near-misses) — no AI cleanup, as the Voice Assistant's question → `heard` →
**thinking**: `assistant::ask_from_phone`, the phone a new origin of the Voice
Assistant's own `answer` (`Asker::Phone`): the conversation choice, the "no AI
model" and "couldn't answer" words (as `error`), `chat_turn` with scope
"assistant" and `SPOKEN_ANSWER_NOTE`, the turn's `stream::Stoppable`
(`link-<session>-<turn>`), the Chat view following live (`yap-assistant-turn`,
`yap-assistant-stream`), the network receipts' "Voice Assistant from your
phone". Not the PC's own things: the window in front, the screenshots in Chat's
box, a quoted passage, the bar's capsule and card.

**Speaking**: the same `Streamed` sink speaks from the answer's first whole
sentence and the rest once it's checked, as much as "How much to read aloud"
says. The voice is **Yap's own speaker** (`tts::speak_streamed_to`: one voice
at a time, a sentence made while the one before plays, a cloud voice's local
stand-in) with its output on the phone (`link/voice.rs`'s `PhonePlayback`, an
`Opener` in rodio's place): each sentence's PCM brought to mono 24 kHz (any
engine's rate), sent as down frames of up to half a second between
`speak_start` and `speak_end`. **Never played on this PC.** Its "speaking" lasts
as long as the phone's playback (the audio's length on a clock from its first
frame). The voice: with "Speak replies" on, the one Settings picked
(`tts::voicing`: Kokoro, Windows voices or ElevenLabs); with it off, still on
the phone, in Yap's own voice — Kokoro if downloaded, else Windows voices
(`tts::phone_voicing`; never a cloud voice not switched on).

**On the phone** (the page's side, `src-tauri/link-page/`; details in its
README): the down frames play as **one stream**. Each frame is resampled from
24 kHz to the phone's output rate as it comes, in one windowed-sinc filter
carried on from frame to frame (`dsp.js` `Resampler`), and played by an
AudioWorklet (`player-worklet.js`) from a **jitter buffer** (`dsp.js`
`Playout`): the sound starts once **250 ms** is waiting (or `speak_end` came);
if it runs dry, the last 5 ms fade out, an underrun is counted, and it waits for
**150 ms** more before fading back in; Stop and barge-in are a 5 ms fade;
`speak_end` plays out what's left. (Before 31 Oct each frame was its own Web
Audio clip, resampled on its own: a click at every join, a hard gap when one
was late — a crackle heard in Bluetooth headsets.)

**"Clearer sound in a headset"** (the page's switch under Hands-free, **on by
default** since 31 Oct for anyone who hasn't chosen; a saved choice is kept in
the phone's storage): while the page holds the mic, a classic Bluetooth
headset stays in its call link (HFP/SCO: narrowband, with packet-loss
artefacts that sound like crackles — heard in a real headset, and gone with
this on). With it on, the page lets the mic go as soon as the question is sent
and, in any case, at `speak_start`, so the headset can play the answer over
its music link (A2DP); the mic opens again on Talk, or, with hands-free on,
once the answer has played out. A new mic's frames count only once its track
is live and unmuted (at most 3 s), while the headset switches back. The cost:
you can't talk over Yap by voice (Talk while it speaks opens the mic afresh
first); Stop still works. Off, the mic stays open through the answer and Talk
while it speaks listens at once. **Sound details** at the bottom of the page
(the sample rate, the latencies, the last answer's start delay and buffer, its
gaps, whether the mic was open while it played, the step opening the mic is on
or ended on and its time, the browser) tells a stream that ran dry on the
phone from the headset's own crackle, and a stuck mic from a slow one.

**Opening the mic never waits silently** (the page's `doOpenMic`; details in
its README): every step has a time limit and a reason on screen when it runs
out — the permission's state where the browser says (1 s), the recorder's and
the player's AudioWorklet modules (loaded one at a time, never both
`addModule`s at once on a context, a suspect in Firefox; 5 s each, then "The
audio setup didn't finish…" and a fresh AudioContext on the next Talk),
`getUserMedia` (after 6 s, or at once when the browser says it's blocked, how
to allow the mic in the address bar, waiting on: a late answer still works,
Talk tries afresh), the AudioContext starting (3 s once Talk is let go; asked
again once the mic is open, which Firefox allows a recording page), and sound
from the mic (3 s, then on anyway). Cancel works at any step and gives Talk
back; a mic a given-up opening gets later is stopped at once. A tap is told
from a hold by the release's own time stamp, counted from when the press was
handled (Firefox's new AudioContext can keep the press busy 0.1–0.4 s), so a
busy page doesn't turn a tap into a too-short hold. (31 Oct: a phone on Firefox once sat on "Opening the
microphone… / Wait…" with nothing said.)

**Hands-free** ends a question after the person's chosen wait — **Short (1 s) ·
Normal (1.5 s, the default) · Long (2.5 s)** — of quiet (`dsp.js`
`EnergyVad`): speech starts 12 dB over the room's noise (and over −45 dB), then
carries on while the level stays 6 dB over the noise and over −52 dB
(hysteresis: a soft last word isn't quiet), the noise for that hold being at
least the room's typical level before speech began (a fluttering noise can't
hold it open). None of this changes the protocol: the PC still gets `start`,
up frames and `stop`.

`interrupt` (or a `start`) **abandons** the question: no longer current (its
queued frames dropped), its answer's stream stopped (what was written stays in
Chat, as after ■), its voice stopped (`tts::stop` when it's still the one
speaking; Kokoro's run cut), `speak_end`, then `state idle`. Within a frame or
two on the PC side; the page stops its own playback at once.

**Off during meetings**: while a meeting records or a call is on
(`assistant::is_off`), a `start` (and a question heard meanwhile) gets
`error`: "The Voice Assistant is off during meetings. …". A dictation started
on the PC while the answer comes stops the voice, as it stops the PC's.

## Dictation from the phone

**The phone is the microphone of an ordinary dictation on the PC** — the
pipeline isn't forked; only where the audio comes from changes
(`dictation/pipeline/phone.rs`, `pipeline::PhoneMic`):

- **`start {mode:"dictate"}`** → `link/session.rs` checks the refusals
  (`dictate::refusal`), then `PhoneMic::start`: under the gesture lock, as a
  shortcut's start, nothing recording or transcribing (`busy_now`), the
  recording marked the phone's (`from_phone`), **the window in front
  captured as for any dictation** (`capture_target`: the app and its kind,
  the text around the cursor, the names on screen, Yap's on-device model
  loading early — or, "Dictate to phone", none: `Goes::Phone`), the meeting's
  dictation signal, `yap-state recording` (the bar's capsule, its waveform
  from the phone's audio), live partials if they're on. No chime, no
  quieting other apps, and **this PC's microphone is never held**: its audio
  is dropped while a phone's recording runs (`mic::Sink::audio`), and its
  failures and losses leave it alone.
- **Up frames** → `PhoneMic::audio` → `take_audio`, the path a mic's (and
  the robot's WAV's) audio takes: the buffer, its length limit, the level
  meter.
- **`stop`** → `PhoneMic::stop` → `stop_and_transcribe`, as a second press:
  the `processing` guard, `run_stt` on **the warm engine** (no second copy),
  then `deliver`: **AI cleanup in the style of the app in front**, the
  dictionary, snippets and "press enter", smart insert, **the paste with its
  read check** — into its own window or onto the clipboard, never into
  whatever has the focus now (`own_window_only`: the person may be in another
  room) — history and the stats. Commands by name and "snip" aren't run from
  a phone (the words are typed as said).
- **The phone hears** (`PhoneSink`, the session's `dictate::Dictating`):
  `state transcribing` when it stops (whoever stopped it), then
  `heard {text, to}` — the text as pasted — and, to the PC, `pasted {ok,
  app}` or `{ok:false, reason}`, then `state idle`. Nothing heard, no
  speech model, a failed transcription, a crash: an `error` in words, then
  idle (`PhoneEnd`, held for the transcription, tells it however it ends).
- **Cancel**: still listening, `PhoneMic::cancel` → the bar's ✕'s own
  cancel — "Dictate to PC" kept a moment for **Undo** on the bar, and in
  Home as "Cancelled" with Transcribe; being typed, the dictation is called
  off and nothing is pasted (`phone_called_off`, checked before the paste).
  The phone leaving mid-dictation cancels it the same way; one already being
  typed finishes.
- **"Dictate to phone"**: the same path to the finished text (cleaned up as
  "everything else", corrected, snippets); `deliver_to_phone` sends it back
  and pastes nothing here. The page copies it
  (`navigator.clipboard.writeText`, in the tap's user activation) and says
  "Copied" with **Copy again**; when the browser won't copy by itself
  (hands-free: no tap), "Ready to copy" with **Copy**.
- **History**: every phone dictation is marked `via: "phone"`
  (`HistoryEntry::via`; Home says "from your phone"); a "Dictate to phone"
  one's `app` is `"phone"` ("Your phone"). An older Yap reads such entries as
  ever and drops only the `via` mark when it saves history (as `secs`); the
  `app` stays.

**The page** (`src-tauri/link-page/`, its README): the mode switch near Talk
(remembered, `yap.link.mode`; not switched mid-talk), the same Talk (tap,
hold, hands-free with its wait), the result under Talk ("Pasted into
Notepad" and the text, the reason, or "Copied" and the text with Copy
again), the model chip and "Clearer sound in a headset" for Ask only, a hint
under the switch when this phone may not type into the PC (and Talk says so
without recording). A dictation can be 15 minutes (the PC's limit), a
question 2.

## Privacy

- No question, answer or audio in the log at `info` and above: phases, lengths,
  sample counts and timings only (`privacy/log_privacy.rs` checks the tree).
  A dictation's text is never logged at `info` either (its length only).
- **Network receipts** (`net::phone`, `Purpose::Phone`, kind "phone"): every
  phone connection, inbound — the page's files and each conversation — as
  "Yap Link · your phone", its host the phone's address, so **your network**
  (or this PC for a test's loopback), with its status and bytes. The AI calls a
  phone's question makes are "Voice Assistant from your phone · <provider>".
- **A signed-out Yap contacts no service for this**: it only answers phones on
  the home network. Finding the PC's addresses and the networks' categories
  reads Windows' own lists; nothing goes on the network.
- The certificate's key and the paired phones stay in the data folder; Reset
  app keeps them (like an account sign-in).

## Limits (this preview)

- **The same Wi-Fi only.** Away from home it doesn't connect (the design's
  phase 5: a relay, signed in).
- **The certificate warning, once per phone** (and again if the PC's address
  changes to one the certificate doesn't name, or after two years).
- **The phone's screen stays on** while you talk (the page holds a wake lock);
  a locked phone stops the page.
- **No headset button yet**: a page can't own it while the mic is open (native
  app, design §6.2). Press Talk on the screen, or use hands-free.
- **Windows Firewall** may ask the first time Yap listens ("Allow Yap on
  private networks"); without that, phones can't reach it.
- No Yap bar indicator for a connected phone yet: Settings → Phone shows
  "connected now" (a phone's dictation shows the usual capsule while it
  listens).
- **No live text on the phone** while it dictates (the design's `partial`):
  the result comes once it's typed. Commands by name aren't run from a phone.

## Tests

- **Unit** (`cargo test link::`): the binary framing and the JSON messages both
  ways (`wire.rs`); the hello and auth rules — a bad token, an expired one, a
  replay, a wrong secret, no nonce, another version (`pairing.rs`); the code;
  tokens; names; the limiter; the device store (hash only, Remove, the oldest
  forgotten); a non-private peer, the QR's address (`lan.rs`); the
  certificate's names and that it loads into rustls (`cert.rs`); request
  parsing, the Origin and Host rules, RFC 6455's accept key (`server.rs`); the
  page's files and policy (`page.rs`); the phone's voice frames, its end,
  interrupt and taken-back words (`voice.rs`); the model list and wire ids,
  the on-device model named once (`models.rs`); dictation's messages both ways
  (`start`'s mode and target, an older page's start, `heard`'s `to`,
  `pasted`, `ready`'s `mayType`), the refusals, the words for each outcome,
  app names, an old or called-off dictation saying nothing (`dictate.rs`),
  "May type into this PC" (on when paired now, off for an older phone,
  `devices.rs`), a phone's dictation waiting for one under way
  (`pipeline/phone.rs`) and going back to it whatever is in front
  (`pipeline.rs`), the history mark (`history.rs`).
- **E2E** `e2e/link.spec.js`: a test run listens on a free port of 127.0.0.1
  (`YAP_E2E_LINK_PORT=0`, the port in `link_status`); a headless Chromium
  (Playwright's, else Edge) with a fake microphone opens the real page over
  Yap's own HTTPS (`ignoreHTTPSErrors`, as a phone's Proceed): Settings → Phone
  and its QR, the pairing question with the code both screens show, Allow, Talk,
  the stub's transcript `heard`, the fake model's answer, the voice down at 24
  kHz (test mode's stand-in voice), the conversation in `chats.json`, the
  receipts, Remove cutting the phone off; a replayed link, a wrong secret, Deny
  and another site's page refused. Dictation: the pairing question naming
  typing, "May type into this PC" on; "Dictate to PC" through the pipeline
  (the stub's transcript) into the text box the test keeps open — the
  stand-in window as Notepad (`e2e_target_window {open: true}`: a test run
  pastes into nothing real) — "Pasted into Notepad" on the phone, history
  marked; "Dictate to phone" copied on the phone with no paste; typing
  switched off in Settings: the phone told at once, Yap refusing a start
  anyway; an Ask after it all.
- **Robot** (`e2e-real/phone-dictation.real.js`, real windows, not run by
  CI): a phone with a spoken WAV as its mic, paired, "Dictate to PC" with the
  test pad in front: the words in the pad's box, the phone told where.
- The page on its own: `npm run test:link` (`e2e-link/`, the PC mocked): the
  sound maths in Node (the resampler both ways, the jitter buffer, the voice
  detector), then the page in a headless Chromium — among the rest, the answer's
  stream (starting at ~250 ms, a late frame faded and counted, Stop's fade),
  Clearer sound (on by default, a saved choice kept, releasing and reopening
  the mic, talking over Yap with it off and on), opening the mic's time limits
  (a permission nobody answers, answered late, blocked; a module that never
  loads; the sound never starting; Cancel at any step; `mic-open.spec.js`), the
  hands-free wait, Sound details; dictation — the mode switch (remembered, the
  chip and Clearer sound for Ask only), each mode's `start`, "Pasted into
  Notepad" or the reason, the PC stopping a dictation, "Copied" and Copy again
  (and Copy when the browser won't), typing off, Cancel calling a dictation off
  (`dictate.spec.js`) — and the same specs (bar hands-free and the
  screenshots) in Playwright's own Firefox when it's installed.
