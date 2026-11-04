# Yap Link, home preview (phone ↔ PC)

**What it is** (2026-10-31): talk to Yap's AI from your **phone** while you
walk round the house — a Bluetooth headset paired to the phone, the answer in
your ear. The phone opens a page Yap serves itself over HTTPS on the home
Wi-Fi; a WebSocket carries the question's audio up and Yap's voice down. The
question goes the **Voice Assistant's** way (`chat/assistant.rs`): Chat's model
with its chosen effort, the short spoken answer, the conversation choice, the
answer kept in Chat. Off by default: **Settings → Phone → "Use Yap from your
phone (on your home Wi-Fi)"** (`link_enabled`).

Since 2026-11-03 the page is **Yap's Chat on a phone**: a question can be
**typed** too (`ask`: the same conversation, answered as Chat answers a typed
question, not spoken; ▶ under an answer has Yap say it, `read_aloud`), and
the model chip opens **Chat's own picker** (the provider rail with its logos,
Latest and More models, the effort), built by the desktop picker's own code.
It's still Ask Yap only: the phone never types into apps on the PC (phone
dictation was built and removed on purpose, `7bbec66e`).

Later that day the page took **the Claude app's layout** (the maintainer:
"screenshots go into the chat window on the phone", the model picker and
its effort "in the chat window", not the header): ☰ opens a drawer of
Chat's conversations and the phone keeps **its own conversation** (one
opened there, or a new chat); a question carries **pictures** taken or
chosen on the phone, sent as Chat sends a typed question's screenshots; the
model chip with its effort sits in the composer.

Then **Talk's words go in the box first** (the maintainer: Done "just sends
the message", with nothing "to see what u said"; the Claude and ChatGPT
apps' way instead): with hands-free off — the default — Talk (a tap then
Done, or a hold then release) puts its words in the composer's text box, at
the caret and after whatever was typed, to check, fix and send yourself.
**Hands-free** still sends at once and Yap answers out loud. The PC only
transcribes them (`start {mode: "words"}` → `words`): nothing reaches Chat
until Send.

That evening **Send answers Talk's words out loud** (the maintainer: "When
I hit send, I don't hear a voice"; till then Send asked them as a typed
question, answered on screen): when any of the box's words came from Talk
— since the box was last empty or sent; fixing them by hand doesn't change
that — Send asks them as the spoken question they were (`ask {speak:
true}`): the bubble's 🎙, the Voice Assistant's spoken answer, marked by
voice in Chat, said on the phone with ■, barge-in by Talk and Clearer
sound, exactly as a spoken question's. A question only typed is still
answered on screen, ▶ to hear it.

And **pairing survives a phone's tabs** (the same evening: Firefox on
Android restored a phone's tabs after it was closed, each with an old
`#pair=` code in its address; each sent that code *instead of* the device
it had, was refused, and the refusals — strikes — locked the phone out for
10 minutes, a fresh QR code included): a hello carries the device **and**
any code the tab has, and the PC counts the device first; the code leaves
the address once the PC answers; only guesses are strikes; a new pairing
link forgives them; a paired phone and the live code are never turned away
for them; "Too many tries" says when to try again. Details: Pairing, below.

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
| `link/session.rs` | One phone's conversation: the hello, pairing, its questions one at a time (spoken or typed), Talk's words for the phone's box (transcribed only), ▶ on an answer, interrupt. |
| `link/voice.rs` | Yap's voice for the phone: the speaker's output sent down the socket, never this PC's speakers. |
| `link/models.rs` | Chat's model and the others it can use (`ready.models`, the first pages'), what Chat's picker shows (`picker`: the very data the desktop picker reads — `ai_model::status` without keys, `model_catalog::snapshot`, `local_llm_status`'s models, "Reads images"), and a switch, an effort or Fast mode from the phone, set as the picker sets them (`ai_model::chat_set_model`, `effort::chat_set_effort`). |
| `link/chats.rs` | Chat's conversations for the phone's drawer (`recents`: Chat's list, not archived, not a meeting's Ask thread, newest first, 200 at most) and a conversation opened (`thread`: its last 40 messages' words, 6,000 characters each at most, an answer's id for ▶, a question's screenshot text left out), pure. |
| `link/pictures.rs` | A question's pictures as they come (`picture` and its `0x03` frames): in order, whole, 4 at most, 3 MB each; the question takes them all or none, and says why. Pure. |
| `link/lan.rs` | This PC's addresses (IP Helper), which peers may connect, Windows' Public networks (Network List Manager). |
| `link/wire.rs` | The protocol (below), pure. |
| `link/page.rs` | The page's files, embedded at build time from `src-tauri/link-page/` (`build.rs` lists them; its README.md left out) and the desktop's own Chat picker modules and logos beside them (`build.rs` `LINK_SHARED`: `core/`, `providers/`), their content types and the Content-Security-Policy. |

The phone's page itself (`src-tauri/link-page/`: plain HTML, CSS and JS
modules, no build step) has its own notes in its
[README.md](../../src-tauri/link-page/README.md) and tests in `e2e-link/`.

**It's Yap's Chat in the Claude app's layout** (2026-11-03, the day's third
pass). **The header**: ☰ on the left opens the drawer, the title in the
middle — "Yap · <PC name>" with the connection's dot (green, amber while
connecting, grey unpaired), the chat's title under it (or how the
connection is) —, a small amber spinner while Yap works the question out,
and ⋯ on the right for the settings sheet (Hands-free and its wait, Clearer
sound, the microphone, Sound details). **The thread** fills the screen:
your question in a bubble on the right (Chat's amber wash, 🎙 when it was
said, its pictures above it — their thumbnails while the page has them,
else "2 pictures"), the answer as plain text the width of the screen with
its markdown (`markdown.js`, built as nodes, never HTML), Chat's activity
line while it works, and under a finished answer **Copy** and **▶ Read
aloud** as quiet icons. It keeps its end in view as the answer's words come
and its activity and Speaking lines show, until the person scrolls up to
read (only scrolling up lets go: a scroll event read a frame late, after
more was drawn below, never does — Firefox stopped following there,
2026-11-04); back at the end, it follows again. **The composer card** at the bottom: the pictures
as thumbnails inside it with ✕, the text box ("Ask Yap, or hold to talk";
Enter sends, Shift+Enter a new line, it grows to five lines), then a row —
**+** (Take photo, Choose photos), **the model chip with its effort**
("Claude Opus 5.5 · Extra", or where it runs when there's no effort; a
narrow card drops "Claude" and the chevron: "Opus 5.5 · Extra"), a gap,
**the mic** (Talk: a tap, then Done; or a hold while held — a *question*
to Yap's AI, never dictation into the PC's apps) and **Send ↑** (Yap's
amber, dim with nothing to send), which is **■** while Yap works the
question out or says an answer. While Yap listens the card is the bar's
capsule: ✕, the ✦ and the bar's live waveform, the time, and the mic
turned into **Done** (a ✓ when its words go in the box, ↑ when it sends).
**Talk's words in the box** (hands-free off, the default; the Claude and
ChatGPT apps' way): at Done the capsule says **"Transcribing…"** with
Chat's three amber dots (no waveform, ✕ still cancels), then the text box
is back with the words in it — at the caret, after whatever was typed (one
space between; talking never wipes the box, a selection stays), the caret
after them, the box grown to show them, Send lit. Nothing is asked and no
bubble shows until **Send**, which asks them as the spoken question they
were (`ask {speak: true}`: the bubble's 🎙, the answer said on the phone
with ■, as a spoken question's) — whenever any of the box's words came from
Talk since it was last empty or sent, however they were fixed; a question
only typed is answered on screen, ▶ to hear it. The pictures in the card
wait for Send. (A PC from before `"speak"` in `ready.can` reads the field
as nothing and answers on screen; the bubble shows no 🎙 then, as Chat
keeps it typed.)
Nothing heard keeps the box as it was: "Yap didn't hear anything. Try
again, or type it." **Hands-free** (the settings sheet: "On: sends when
you stop talking, and Yap answers out loud. Off: your words go in the box
to check first.") sends at Done or its wait of quiet, the question's bubble
from the start, the answer spoken — as a PC from before words (no
`"words"` in `ready.can`) always does. **One Stop
at a time**: under the answer being said only the bar's bars and
"Speaking…"; when ▶ on an answer started it, that ▶ is the Stop, in place,
and the composer has none. **The drawer** slides in from the left over a
scrim: "Yap" in EB Garamond, a search box filtering the titles, **Recents**
(Chat's conversations on the PC, newest first, the one this phone is in
marked), and at the foot the PC's name and **+ New chat**. When Chat's model
can't read the pictures the card shows **Chat's own line**: "Qwen3.5 4B
can't read images." with Use <the model that can>, **Send its text
instead**, Choose another model…, and Send without the pictures. It follows Yap's tokens — `link.css` copies
`src/app.css`'s `--yap-*` values, light and the warm dark, following the
phone's `prefers-color-scheme`, and `e2e-link/tokens.spec.js` holds the copy
to app.css — with Figtree and EB Garamond served from the PC
(`link-page/fonts/`, 36 KB; never Google Fonts). Where a phone needs its own
way it has it: 44–48 px targets, safe areas, the composer riding above the
on-screen keyboard (`interactive-widget=resizes-content`, else the visual
viewport), text that grows with the phone's font size (to Android's
largest, 200 %: the round buttons stop at 56 px, the chip's name first),
no hover-only affordances, reduced motion, and **on its side two columns**
(#17's idea kept: the conversation on the left, the composer card on the
right at the bottom, under the thumb). **Chat's picker** is the desktop's
code itself: `src/lib/core/chatPicker.js` (and the pure modules it imports)
builds the rows on both, served to the phone as they are with the provider
logos (`build.rs` `LINK_SHARED`, `e2e-link/shared.spec.js` keeps them pure).
Details: the README's "The look" and "Chat's picker".

## The protocol (v1)

v1 with **additions** (2026-11-03, marked ⁺ below): typed questions, ▶ on an
answer, Chat's picker and the effort; then the phone's own conversation
(the drawer) and its pictures; then Talk's words for the phone's box
(`start {mode: "words"}` → `words`), and asked out loud once sent (`ask
{speak: true}`); and a hello with both `device` and `pair`. They're
backward compatible both ways:
`hello.v` stays 1; a first page ignores the new fields of `ready` and
`answer`; a new page offers typing, ▶ and the picker only when `ready.can`
says the PC answers them (a PC from before them: "Yap on your PC needs an
update to answer typed questions…", and the plain model list; one without
`"words"`: Talk sends the question at Done, as before — a PC from before
takes any `start` as a question, `mode` unread; one without `"speak"` reads
`ask`'s `speak` as nothing and answers on screen, no error; any PC judges a
hello's device before its `pair`, so a page sending both connects to one
from before too); a message a PC
doesn't know is answered `error` "Yap on your PC doesn't know how to do that
yet. Update Yap on the PC, then try again." (`wire::UNKNOWN`; a PC from before
says "Yap didn't understand that message").

**WebSocket** at `wss://<pc>:51443/ws`. **Text frames** are JSON
`{ "type": … }`. **Binary frames**: byte 0 the kind — `0x01` **up** (the
phone's mic, 16 kHz mono Int16 LE) or `0x02` **down** (Yap's voice, mono
Int16 LE at `speak_start`'s rate) — bytes 1–4 `seq` (u32 LE), the rest samples;
or ⁺ `0x03` **a picture's piece** — bytes 1–4 the piece's number (u32 LE, from
0 for each picture), the rest the picture's bytes (32,000 at most). A frame
is at most 32,005 bytes (a second of audio); a text frame at most 16 KiB.

**Phone → PC**

| Message | Meaning |
|---|---|
| `hello {v:1, name, nonce, device?:{id, secret}, pair?:{token}}` | The first message. `device` on a later visit, `pair` from the QR — ⁺ both when the page has both (a tab restored with an old `#pair=` in its address, the code scanned again on a paired phone): the device counts first (Pairing, below). |
| `start {mode:"ask", chat?⁺}` | A question starts; up frames follow, 20–40 ms each (the page sends exactly 320 samples, `seq` from 0 for each question). `chat`: the conversation it goes to (below). |
| `start {mode:"words"⁺, chat?}` | The same, for **the words alone** (`wire::Talk::Words`): recorded as a question is, then on `stop` (or the 2-minute cap) only transcribed — the same `PhoneEar` and dictionary — and answered `words`, then `state idle`. No `heard`, no thinking, no Voice Assistant, nothing written to Chat; empty: `error` "Yap didn't hear anything. Try again, or type it." + idle; a transcription that failed: its `error` + idle. The page sends it with hands-free off where `ready.can` has `"words"`. |
| `stop {pictures?⁺, asText?⁺}` | The question ended (after its last frame): transcribe and answer, with the `pictures` sent just before it (their text instead when `asText`). For `"words"`: transcribe only (no pictures go; any announced are dropped). |
| `cancel` | Drop the question (listening, or being worked on) — or the words being transcribed. |
| `interrupt` | Stop speaking and abandon the answer. |
| `ask {text, chat?, pictures?, asText?, speak?}` ⁺ | A question from the composer (4,000 characters at most): `heard`, thinking, the answer as for a spoken one, in the same conversation. Typed: **not spoken**. ⁺ `speak: true` (the page sends it when any of the box's words came from Talk, since it was last empty or sent): asked as the spoken question it was — by voice in Chat, the Voice Assistant's spoken answer, said on the phone as a `stop`'s is (`speak_start` …). `chat`, `pictures`, `asText` as for `start` / `stop`. |
| `picture {n, size}` ⁺ | Picture `n` (from 0) of the next question: `size` bytes (3 MB at most), in `0x03` frames that follow. Four at most a question. |
| `chats` ⁺ | Chat's conversations, for the drawer (`chats`). |
| `open_chat {id}` ⁺ | This phone's questions go to conversation `id` from now on; its messages come back (`thread`). |
| `new_chat` ⁺ | The next question starts a conversation (`thread` with `id: null`). |
| `read_aloud {id}` ⁺ | Say answer `id` (`answer.id`, one this phone was given while Yap runs) on the phone, whole, in Yap's voice. |
| `set_model {id}` | Switch Chat's model (one of `ready.models`). |
| `set_model {provider, model, baseUrl?, file?}` ⁺ | Switch Chat's model to a row of Chat's picker, as the desktop picker does (`chat_set_model`'s own checks: a provider with a key, a server Settings knows, an on-device model on this PC). |
| `set_effort {level}` ⁺ | The effort Chat's model answers at (`null`: its default). |
| `set_fast {on}` ⁺ | Fast mode (Claude Opus 5.5). |
| `ping` | Every 15 s; answered at once, mid-answer too. |

**PC → phone**

| Message | Meaning |
|---|---|
| `pair_wait {code}` | The pairing waits for a yes on the PC, which shows the same code. |
| `paired {device:{id, secret}}` | Paired: the phone keeps these for its next visits. |
| `denied {reason}` | Refused, in words the phone can show ("Too many tries from this phone. Try again in 4 minutes."); the socket closes. |
| `ready {pc, model:{id,label,onThisPc}, models:[{id,label,onThisPc}], can⁺, picker⁺}` | Connected: this PC's name, Chat's model (or `null`: none set up) and the others; `can`: what it answers beyond v1 (`["ask", "read_aloud", "picker", "effort", "chats", "pictures", "words", "speak"]`); `picker`: what Chat's picker shows (`{status, catalog, local, images}`, `link/models.rs`; never a key; `status.images` and `status.imagesSwitch` say whether Chat's model reads pictures and which would). Sent again whenever Chat's model changes (here, in Chat, or from a phone: `yap-chat-model-changed`) and after every `set_model`, which the page takes as the truth. |
| `chats {items:[{id, title, ts}]}` ⁺ | Chat's conversations, newest first (`link/chats.rs`). |
| `thread {id, title, messages:[{role, text, id?, voice?, pictures?}], more}` ⁺ | A conversation's latest messages (words only; an answer's `id` for ▶; `pictures`: how many a question carried), `more` when older ones were left out; `id: null` for a new chat (or one that's gone, after an `error`). |
| `chat {id, title}` ⁺ | This phone's question went to conversation `id` (the one chosen for it, or a new chat's): its questions go there from now on. |
| `title {id, title}` ⁺ | Conversation `id` was renamed in Chat (every phone hears it). Chat's titles are a conversation's first words; a rename is the only change. |
| `cant_read {message}` ⁺ | Chat's model can't read the question's pictures, or its server turned them down (`snip::vision`'s words). The page puts the question back in the composer with them and shows Chat's line. |
| `state {phase}` | `"idle"` \| `"listening"` \| `"transcribing"` \| `"thinking"` \| `"speaking"`. |
| `heard {text}` | The question, transcribed and corrected (a typed one, as it came). |
| `words {text}` ⁺ | Talk's words for the phone's box (`start {mode:"words"}`), transcribed and corrected, trimmed: nothing was asked. Then `state idle`. |
| `step {label}` | What the answer is doing, in Chat's activity line's words (`chat/activity.rs`: "Searching the web · …"). |
| `answer {text, final, id⁺}` | **The whole answer so far** (each replaces the last; `""` = words taken back before a tool call); `final:true` with the checked answer and its `id` in Chat (`chat:<conversation>:<index>`, for ▶). |
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
(then it's asked as it is; Talk's words, transcribed as they are). An
unpaired or bad hello gets `denied` and a close. Talk's words are a turn as
a question is: `cancel`, `interrupt`, a new `start`, an `ask` or a
`read_aloud` drops them, and their late `words` never go out (the page
drops them too, as an old question's news).
An `ask` abandons whatever was under way too (answered `state idle` first when
there was something), then `heard`, `state thinking`, the answer and `state
idle` — no voice; with `speak`, the answer's voice as after a `stop`
(`state speaking`, `speak_start`, down frames, `speak_end`, `state idle`),
and `interrupt` or a `start` abandons it the same way. The page readies its
sound on Send's tap (a phone wants a gesture for it), as for ▶. A
`read_aloud` always answers `state idle` first (the
page's cue that the old news has ended), then the voice (`state speaking`,
`speak_start`, down frames, `speak_end`, `state idle`), or `error` ("That
answer isn't in Chat on your PC any more…") for an id it didn't give this
phone or one gone from Chat. The page sends `cancel` or `interrupt` itself
before an `ask` or a `read_aloud` while Yap works or speaks, as before a
`start`.

**The phone's conversation** (`chat` on `ask` / `start`: a conversation's
id, or `"new"`; `assistant::phone_conversation`, pure): the one it names
while it's still in Chat's list (not deleted, not archived, not a meeting's
Ask thread); a new one for `"new"` — or when the one it names is gone, never
another; and with no `chat` (a page from before, or a phone that never
picked one) the Voice Assistant's own choice (`choose_conversation`: the
one open in Chat, else the latest within a few minutes). `open_chat` and
`new_chat` set it for the questions after them too. The PC says where each
question went (`chat {id}`); a phone that has named one stays in it, one
that never did is asked as the Voice Assistant chooses each time. The page
keeps it in `localStorage` (`yap.link.chat`) and asks for its messages once
it's up (`open_chat`), so a reload shows what was written meanwhile. Opening
a conversation makes its answers ones ▶ may read on that phone.

**Pictures** go just before the question that takes them: for each, `picture
{n, size}` then its bytes in `0x03` frames, in order; then `ask` (or `stop`)
with `pictures: N`. The PC keeps the caps (4, 3 MB each, every piece in turn,
`link/pictures.rs`) — anything amiss and the question doesn't go: `error`
says why ("A picture didn't reach your PC whole…"). Each picture is then read
on the PC as Chat reads a pasted one (`snip::attach::keep`: decoded by
Windows' codecs, stored again as a PNG in `chat-images/`, so nothing that
isn't a picture is kept) and the question is asked the way
Chat's typed question with screenshots is — the same function,
`chat::commands::question_images`: a model known not to read images gets
`cant_read` (nothing asked); else the on-device model loads its projector, the
copies for the provider go with every call of the turn, and a server that
turns them down makes that model a "no" (`snip::vision::refused`) and gets
`cant_read`; with `asText` their text, read on this PC by Windows' OCR, goes
after the question instead (`snip::ocr`). The question keeps its pictures in
Chat, as a typed question's screenshots are kept. A question with pictures
isn't a Skill or an app to open.

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

**A phone's tabs** (2026-11-03). Every QR scan opens a tab, and a phone's
browser keeps them: Firefox on Android restored a phone's tabs with the
address each was opened at — `#pair=` and all, from before the page took it
out with `history.replaceState` — and each then sent its old code *instead
of* the device it had (the page's old rule: "a code scanned again pairs
afresh"). Every one was refused, every refusal was a strike, and five
strikes in 10 minutes locked the phone's address out — the paired phone and
a fresh QR code too ("Too many tries…" came before anything else was
looked at). Re-scanning also paired the same phone afresh each time, so
Settings filled with copies of it. Now:

- **The page sends the device and the code** when a tab has both
  (`link.js` `connect`), the device read afresh from `localStorage` for
  every hello (another tab may have paired since). **The PC judges the
  device first** (`pairing::judge`): known, it's connected whatever the
  code — and when the code is the **live invitation's** (scanned again on a
  phone paired already) that's used up, `Verdict::Known { invited }`, and
  Settings → Phone is told (`yap-link-already-paired`: the code closes,
  "“Android · Firefox” is already paired"), so nothing waits on it; not
  known, the code pairs as before (a phone removed, or new). A PC from
  before already judged the device first.
- **The code leaves the address** once the PC answers — `paired`, `denied`
  or now `ready` — so a reload or a restored tab never brings it again.
- **A refused tab comes back** when another tab pairs the phone (the
  `storage` event: at once if it's in front, else when it's next shown).
- **A code opened in a tab that's already open** (the link pasted into its
  address bar: only the fragment changes, so the page isn't loaded again)
  is said at once (`hashchange`).
- **Only guesses are strikes** (`Refusal::strikes`): a wrong secret or an
  unknown phone ("removed"), a code that isn't the live invitation's ("bad
  link"). The real code used already or run out ("used", "expired") came
  from the QR on the PC's screen; an old page ("wrong version") or a hello
  with nothing in it ("not paired") guessed nothing.
- **A struck-out address waits only for hellos that are refused anyway**
  (`session.rs` `hello_verdict`: the minute's cap, then the hello judged,
  then the wait): a paired phone's hello and the live code's go on — a
  256-bit secret and a 128-bit one-time code can't be guessed, and the
  minute's cap (20 hellos) still bounds how fast anything is tried.
- **A new pairing link forgives every address's strikes** (`link_pair_start`
  → `Limiter::forgive`): the person at the PC is pairing on purpose.
- **"Too many tries" says when**: "Too many tries from this phone. Try again
  in 4 minutes." (`pairing::too_many`: until the strike that counts is 10
  minutes old, or the minute's first hello a minute old; whole minutes,
  rounded up).
- **The log says which refusal** (info): `kind` = removed, not paired, used,
  expired, bad link, wrong version or too many, and whether it was a strike
  — never an id, a secret, a code or an address.

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
- **Hellos per address**: at most 20 a minute (each judged); 5 guesses
  refused in 10 minutes (a wrong secret, a code that isn't Yap's) and that
  address's refused hellos wait until the first of them is 10 minutes old,
  told how long (`pairing::Limiter`; a paired phone's hello and the live
  code's never wait, and a new pairing link forgives: Pairing, above). A
  hello must come within 10 s.
- **What a phone can do**: ask the Voice Assistant, out loud or typed (its
  tools follow their own rules: opening apps on this PC, notes, the web),
  with pictures it sends (kept in `chat-images/` as Chat keeps a pasted one,
  only once they've been read as pictures, at most 4 of 3 MB a question);
  list Chat's conversations and open one (their titles, and a conversation's
  last 40 messages' words — never a meeting's Ask thread, an archived chat,
  a screenshot or a file); have one of the answers it was given or opened
  read out (▶: only those, by their id); and switch Chat's model, its effort
  and Fast mode as Chat's picker does. It can't type into the PC (no
  dictation), add a key, download a model, start meeting notes, delete or
  rename a chat, or read anything else. The picker's data never carries a
  key, not even masked. (A paired phone is trusted with Chat as the person
  at the PC is: pairing takes a yes on the PC with the code both screens
  show, and Settings → Phone removes a phone at once.)
- **Chat's picker code on the page** is the desktop's own (`src/lib/core/`,
  pure modules: no Tauri, no Svelte, no imports beyond each other), served as
  static files from this PC with the logos; the page builds its rows as DOM
  nodes, never HTML.

## The voice loop

`start` → listening (up frames into a buffer, 2 minutes at most) → `stop` →
**transcribing**: the warm engine, lent as a dictation's is and never a second
copy (`pipeline::PhoneEar` → `Shared::phone_question`: `lease_engine`, My
languages, the Whisper prompt), then the correction dictionary (exact, then
near-misses) — no AI cleanup, as the Voice Assistant's question (for
`start {mode:"words"}` it stops here: `words`, then idle — `session::words`)
→ `heard` → **thinking**: `assistant::ask_from_phone`, the phone a new origin of the Voice
Assistant's own `answer` (`Asker::Phone`): the conversation choice (the
phone's own when it named one, `phone_conversation`), its pictures (Chat's
`question_images`), the "no AI
model" and "couldn't answer" words (as `error`), `chat_turn` with scope
"assistant" and `SPOKEN_ANSWER_NOTE`, the turn's `stream::Stoppable`
(`link-<session>-<turn>`), the Chat view following live (`yap-assistant-turn`,
`yap-assistant-stream`), the network receipts' "Voice Assistant from your
phone". Not the PC's own things: the window in front, the screenshots in Chat's
box (the phone's own pictures go instead), a quoted passage, the bar's
capsule and card.

**Speaking**: the same `Streamed` sink speaks from the answer's first whole
sentence and the rest once it's checked, as much as "How much to read aloud"
says. The voice is **Yap's own speaker** (`tts::speak_streamed_to`: one voice
at a time, a sentence made while the one before plays, a cloud voice's local
stand-in) with its output on the phone (`link/voice.rs`'s `PhonePlayback`, an
`Opener` in rodio's place): each sentence's PCM brought to mono 24 kHz (any
engine's rate), sent as down frames of up to half a second between
`speak_start` and `speak_end`. **Never played on this PC**, so it never turns the
PC's other apps down (`voice/duck.rs` is the PC's speakers' only). Its "speaking" lasts
as long as the phone's playback (the audio's length on a clock from its first
frame). The voice: with "Speak replies" on, the one Settings picked
(`tts::voicing`: Kokoro, Windows voices, ElevenLabs, or OpenAI, Gemini or Groq on
the key Language Models has); with it off, still on
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

**"Play the voice through the media player"** (the page's switch under
Clearer sound, **off** unless chosen, kept in the phone's storage; 4 Nov):
the answer isn't played by Web Audio but cut at its pauses (`dsp.js`
`Segmenter`: a piece ends in the middle of an 80 ms pause once it holds
600 ms of voice — 1.5 s after the first —, or at its quietest 10 ms when it
runs 6 s with no pause; nothing lost or repeated), each piece a WAV file
(`dsp.js` `wav`, at Yap's own 24 kHz: the browser resamples it) that an
`<audio>` element plays whole, the next loading meanwhile (`link.js`
`mediaVoice`). Why: Firefox for Android ignores `latencyHint` and gives Web
Audio its smallest output buffer (about 14 ms), so a busy moment on the phone
can crackle without the jitter buffer ever running dry; the media player's
output keeps far more sound ready. The cost: a slightly longer pause where it
was cut, and Stop without the fade. A piece not there when the last one ends
counts as a gap, as the worklet's do. A phone answer crackling with no gaps
counted (Firefox, 4 Nov) is what it's for: one long answer each way tells
the browser's sound from Yap's stream.

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
(the sample rate, the latencies, which way it played — Web Audio or the media
player —, the last answer's start delay and buffer, its
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

**Hands-free** (off by default; off, Talk's words go in the box to check
first) sends a question, answered out loud, after the person's chosen wait — **Short (1 s) ·
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
  replay, a wrong secret, no nonce, another version; a paired phone with an
  old code too (known), with the live one (known, the code spent), a phone
  unknown with the live code (pairs) (`pairing.rs`); which refusals are
  strikes, their words and kinds; "Try again in 4 minutes."; the code;
  tokens; names; the limiter (the minute's cap and its wait, the strikes'
  wait, a new link forgiving them) and the 3 Nov log replayed (restored tabs
  never lock their paired phone out, the live code pairs a struck-out
  address); the device store (hash only, Remove, the oldest
  forgotten); a non-private peer, the QR's address (`lan.rs`); the
  certificate's names and that it loads into rustls (`cert.rs`); request
  parsing, the Origin and Host rules, RFC 6455's accept key (`server.rs`); the
  page's files and policy, Chat's picker modules and logos beside them
  (`page.rs`); the phone's voice frames, its end, interrupt and taken-back
  words (`voice.rs`); the model list and wire ids, the on-device model named
  once, the picker's data (what Chat's picker reads, no key) (`models.rs`);
  `ask`, `read_aloud`, the picker's `set_model`, `set_effort`, `set_fast` and
  a newer page's unknown message (`wire.rs`); the answers a phone may have
  read (`link.rs` `Given`), an answer's id (`session.rs`); a typed question
  answered as typed and not said (`chat/assistant.rs`); the drawer's messages
  both ways, a question's `chat`, `picture` and its frames (`wire.rs`);
  Recents (newest first, archived and meeting threads left out, bounded) and
  a thread (its latest words, capped, answer ids, a screenshot's text left
  out: `chats.rs`); pictures whole, in order, the caps, one cut short or out
  of turn (`pictures.rs`); the phone's conversation chosen and a gone one
  falling back to a new chat (`chat/assistant.rs`); the picker's data with
  `imagesSwitch` and without `imagesLocal` (`models.rs`); Talk's words —
  `start {mode:"words"}` (`Talk::of`: exactly "words", else a question),
  `words` written, `"words"` in `can` (`wire.rs`) — and what the box gets:
  the words trimmed, or why there are none (`session.rs` `words_out`);
  `ask {speak}` read (only a true one), `"speak"` last in `can`, a hello
  with both `device` and `pair` (`wire.rs`).
- **E2E** `e2e/link.spec.js`: a test run listens on a free port of 127.0.0.1
  (`YAP_E2E_LINK_PORT=0`, the port in `link_status`); a headless Chromium
  (Playwright's, else Edge) with a fake microphone opens the real page over
  Yap's own HTTPS (`ignoreHTTPSErrors`, as a phone's Proceed): Settings → Phone
  and its QR, the pairing question with the code both screens show, Allow, Talk
  with hands-free on, the stub's transcript `heard`, the fake model's answer,
  the voice down at 24 kHz (test mode's stand-in voice), the conversation in
  `chats.json`, the receipts; Talk with hands-free off: the stub's transcript
  in the phone's box, nothing in Chat, then fixed by hand and Send asking
  it out loud (by voice in `chats.json`, the spoken answer's note to the
  model, the answer said on the phone); a typed question in the same conversation, not "by voice", with
  nothing said, then ▶ saying it; ☰'s drawer listing that conversation,
  marked; + New chat and a question going to a conversation of its own; a
  picture from the phone (GPT-OSS can't read it: the composer's line, then
  "Send its text instead" with test mode's OCR stand-in), kept with the
  question in `chats.json` (`asText`); the first
  conversation opened again from the drawer; Chat's picker over the wire
  switching Chat's model; Remove cutting the phone off; a replayed link, a
  wrong secret, Deny and another site's page refused; a phone's tabs — three
  restored with old codes (the one it paired with among them) connecting on
  its device, the code scanned again on it spent and Settings saying it's
  paired already, a new phone's five tabs with old codes refused and a
  sixth told "Try again in 10 minutes.", the paired phone still connecting
  from that address, the live code still pairing the new one, and its
  refused tabs coming back with its device.
- The page on its own: `npm run test:link` (`e2e-link/`, the PC mocked): the
  sound maths in Node (the resampler both ways, the jitter buffer, the voice
  detector, the media player's pieces cut in the pauses and their WAV files),
  then the page in a headless Chromium — among the rest, the answer's
  stream (starting at ~250 ms, a late frame faded and counted, Stop's fade),
  the media player (off at first, kept, an answer in pieces, Stop, a late
  piece counted as a gap), Clearer sound (on by default, a saved choice kept, releasing and reopening
  the mic, talking over Yap with it off and on), opening the mic's time limits
  (a permission nobody answers, answered late, blocked; a module that never
  loads; the sound never starting; Cancel at any step; `mic-open.spec.js`), the
  hands-free wait, Sound details, the answer's markdown (a model's HTML shown
  as text), typed questions (`typing.spec.js`: `ask` over the wire, the
  answer in the thread with nothing spoken, ▶ and its ■, Enter and
  Shift+Enter, the box growing, typing over Yap, a PC from before, the
  thread following an answer as it's written, typed and spoken, and staying
  put when scrolled up, the keyboard up both ways, a reload keeping the
  thread), Chat's picker
  (`picker.spec.js`: the rail and its logos, Latest, More models in place, a
  switch, a provider without a key, ChatGPT's two ways, the effort and Fast
  mode, a PC from before), the drawer and the phone's conversation
  (`chats.spec.js`: ☰, Recents, the search, a chat opened — its messages,
  its title, ▶ on an old answer, the next questions' `chat` —, + New chat
  and the PC's `chat`, a rename, a reload asking again, one gone, a PC from
  before), pictures (`pictures.spec.js`: + → Choose photos, thumbnails with
  ✕, made smaller to 2,048 px JPEGs, sent in pieces before `ask` or `stop`,
  four at most, Chat's line with the switch and their text instead, the PC's
  `cant_read`, a PC from before), Talk's words in the box (`words.spec.js`:
  `start {mode: "words"}`, Done's ✓, "Transcribing…" with ✕, the words in
  the box at the caret and after what was typed, a hold and its release,
  Send asking them out loud — `speak`, the 🎙, the voice, ■ and Talk over
  it — while a question only typed (or the box emptied first) stays typed,
  nothing heard keeping the box, ✕ while transcribing and the late words
  dropped, the pictures waiting for Send and Chat's line keeping them said,
  a PC from before `"speak"` answering on screen with no error, the PC's
  own 2 minutes, the mic gone, hands-free and a PC from before still
  sending, at 200 % text and on its side), a phone's tabs
  (`pairing.spec.js`, against a PC that judges hellos as Yap's does: a saved
  device with an old `#pair=` connecting and the code leaving the address,
  six tabs restored at once never locked out, a fresh code pairing after
  old tabs were refused and those tabs coming back, a tab with a removed
  device taking the new one at its next hello, a code opened in a tab
  already open, "Too many tries" with its wait and no hammering), the shared modules staying pure
  (`shared.spec.js`), the look (Yap's fonts and the logos from the page's
  own origin; at 360, 390 and 430 px wide, on its side and at 130 and 200 %
  text, nothing sideways and Talk in the thumb's reach; `tokens.spec.js`:
  link.css's colours are app.css's) — and the same specs (bar hands-free and
  the screenshots) in Playwright's own Firefox when it's installed.
