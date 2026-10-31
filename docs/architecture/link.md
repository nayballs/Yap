# Yap Link, home preview (phone ↔ PC)

**What it is** (2026-10-31): talk to Yap's AI from your **phone** while you
walk round the house — a Bluetooth headset paired to the phone, the answer in
your ear. The phone opens a page Yap serves itself over HTTPS on the home
Wi-Fi; a WebSocket carries the question's audio up and Yap's voice down. The
question goes the **Voice Assistant's** way (`chat/assistant.rs`): Chat's model
with its chosen effort, the short spoken answer, the conversation choice, the
answer kept in Chat. Off by default: **Settings → Phone → "Use Yap from your
phone (on your home Wi-Fi)"** (`link_enabled`).

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
| `link/devices.rs` | Paired phones in `link_devices.json` (a hash of each phone's secret only). |
| `link/session.rs` | One phone's conversation: the hello, pairing, its questions one at a time, interrupt. |
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
| `start {mode:"ask"}` | A question starts; up frames follow, 20–40 ms each (the page sends exactly 320 samples, `seq` from 0 for each question). |
| `stop` | The question ended (after its last frame): transcribe and answer. |
| `cancel` | Drop the question (listening, or being worked on). |
| `interrupt` | Stop speaking and abandon the answer. |
| `set_model {id}` | Switch Chat's model (one of `ready.models`). |
| `ping` | Every 15 s; answered at once, mid-answer too. |

**PC → phone**

| Message | Meaning |
|---|---|
| `pair_wait {code}` | The pairing waits for a yes on the PC, which shows the same code. |
| `paired {device:{id, secret}}` | Paired: the phone keeps these for its next visits. |
| `denied {reason}` | Refused, in words the phone can show; the socket closes. |
| `ready {pc, model:{id,label,onThisPc}, models:[{id,label,onThisPc}]}` | Connected: this PC's name, Chat's model (or `null`: none set up) and the others. Sent again whenever Chat's model changes (here, in Chat, or from a phone: `yap-chat-model-changed`) and after every `set_model`, which the page takes as the truth. |
| `state {phase}` | `"idle"` \| `"listening"` \| `"transcribing"` \| `"thinking"` \| `"speaking"`. |
| `heard {text}` | The question, transcribed and corrected. |
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
  rules: opening apps on this PC, notes, the web), and switch Chat's model. It
  can't type into the PC, start meeting notes or read anything else.

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

**"Clearer sound in a headset"** (the page's switch under Hands-free, off by
default, kept in the phone's storage): while the page holds the mic, a classic
Bluetooth headset stays in its call link (HFP/SCO: narrowband, with
packet-loss artefacts that sound like crackles). With it on, the page lets the
mic go as soon as the question is sent and, in any case, at `speak_start`, so
the headset can play the answer over its music link (A2DP); the mic opens again
on Talk, or, with hands-free on, once the answer has played out. A new mic's
frames count only once its track is live and unmuted (at most 3 s), while the
headset switches back. You can't talk over Yap by voice then; Stop still
works. **Sound details** at the bottom of the page (the sample rate, the
latencies, the last answer's start delay and buffer, its gaps, whether the mic
was open while it played, the browser) tells a stream that ran dry on the
phone from the headset's own crackle.

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

## Privacy

- No question, answer or audio in the log at `info` and above: phases, lengths,
  sample counts and timings only (`privacy/log_privacy.rs` checks the tree).
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
  "connected now".

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
  the on-device model named once (`models.rs`).
- **E2E** `e2e/link.spec.js`: a test run listens on a free port of 127.0.0.1
  (`YAP_E2E_LINK_PORT=0`, the port in `link_status`); a headless Chromium
  (Playwright's, else Edge) with a fake microphone opens the real page over
  Yap's own HTTPS (`ignoreHTTPSErrors`, as a phone's Proceed): Settings → Phone
  and its QR, the pairing question with the code both screens show, Allow, Talk,
  the stub's transcript `heard`, the fake model's answer, the voice down at 24
  kHz (test mode's stand-in voice), the conversation in `chats.json`, the
  receipts, Remove cutting the phone off; a replayed link, a wrong secret, Deny
  and another site's page refused.
- The page on its own: `npm run test:link` (`e2e-link/`, the PC mocked): the
  sound maths in Node (the resampler both ways, the jitter buffer, the voice
  detector), then the page in a headless Chromium — among the rest, the answer's
  stream (starting at ~250 ms, a late frame faded and counted, Stop's fade),
  Clearer sound releasing and reopening the mic, the hands-free wait, Sound
  details.
