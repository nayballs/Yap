# Yap Link: the phone's page

The page a phone opens to talk to Yap's AI on the PC, through a Bluetooth headset
or the phone itself: ask out loud and hear the answer, or type a question.
Yap on the PC embeds these files in `yap.exe` and serves them over HTTPS on
the home Wi-Fi (port 51443, a self-signed certificate the phone accepts once),
with the WebSocket at `/ws`.

Plain HTML, CSS and JavaScript modules: no build step, no framework, nothing
fetched from anywhere but the PC (no CDN; Yap's two fonts come from the PC
too). **It's Yap's Chat on a phone, in the Claude app's layout**
(2026-11-03): ☰ and a drawer of Chat's conversations, the thread, a composer
card with the pictures, +, Chat's model chip with its effort, the mic and
Send — **Chat's own picker** the desktop's code itself, served beside the
page ("Chat's own modules", below) — in Yap's tokens (`src/app.css`), fonts
and mark, on a phone's terms ("The look", below). About 356 KB as served
(36 KB of it the fonts, 54 KB Chat's picker modules, 22 KB the logos), 140
KB gzipped (317 KB and 127 KB before the Claude-app layout; 175 KB and 80 KB
before the Chat redesign).

**The protocol** (v1: the hello, pairing, the JSON messages and the binary
audio frames) is written down in
[`docs/architecture/link.md`](../../docs/architecture/link.md). This page keeps
to it exactly; the notes below are what it does where the protocol leaves a
choice.

## The files

| File | What it is | Served as |
|---|---|---|
| `index.html` | The page: the header (☰, "Yap · <PC>" with the connection's dot and the chat's title, the spinner, ⋯), pairing, the thread and Chat's activity line, the composer card (Chat's line for pictures its model can't read, the pictures, the text box, then +, the model chip, the mic, Send / ■, ✕ / ■, the waveform and time while Yap listens), the drawer, the settings sheet (Hands-free and its wait, Clearer sound in a headset, the microphone picker, Sound details) and the model sheet. | `text/html` |
| `link.css` | The look ("The look", below): Yap's warm light and dark `--yap-*` tokens (values copied from `src/app.css`, following `prefers-color-scheme`; `e2e-link/tokens.spec.js` holds them to it), its fonts, mobile first, 48 px targets, reduced motion respected. | `text/css` |
| `markdown.js` | An answer's markdown as Chat shows it (its spoken-answer subset: paragraphs and line breaks, headings, lists, bold, italic, struck, code, quotes, rules), built as DOM nodes, never HTML; links as their words, `[1]` citations left out, a mark still being written shown without its stars. | `text/javascript` |
| `fonts/figtree.woff2`, `fonts/eb-garamond.woff2` | Yap's UI font and its display serif, cut down from the desktop's own (below). | `font/woff2` |
| `link.js` | Everything the page does: the connection (hello, pairing, reconnects with backoff, keep-alive pings), typed questions and ▶ Read aloud, the composer and the keyboard, the mic (opened a timed step at a time), Talk (tap or hold), hands-free, the spoken answer (resampled frame after frame, posted to the player worklet), Clearer sound, barge-in, the conversation (kept in this tab's sessionStorage across a reload), the step and its time, the model chip, the settings sheet, Sound details, the screen wake lock. Exposes a read-only `window.__yapLink.snapshot()` for the tests. | `text/javascript` |
| `picker.js` | Chat's model picker as a sheet ("Chat's picker", below): the rail with the logos, the panes, More models, the effort; its rows built by `core/chatPicker.js`; a PC from before gets its plain list. The chip's words and its effort (`chipWords`, `chipEffort`). | `text/javascript` |
| `drawer.js` | The drawer of chats ("The drawer", below): Recents from the PC (`chats`), the search, the one this phone is in marked, + New chat. | `text/javascript` |
| `photos.js` | A question's pictures ("Pictures", below): a photo made smaller (2,048 px, a JPEG of at most 3 MB, upright, on white) and sent in `0x03` frames. | `text/javascript` |
| `core/*.js` | **Not in this folder**: Chat's own picker modules from `src/lib/core/` (`chatPicker`, `modelCatalog`, `latestModels`, `ppModels`, `localContext`, `modelIcons`, `providerIds`, `effort`), served as they are (`build.rs` `LINK_SHARED`). | `text/javascript` |
| `providers/*.svg` | **Not in this folder**: the provider and model-family logos from `src/assets/providers/` the picker and the chip show (Yap's own assets; `build.rs` `LINK_SHARED`). | `image/svg+xml` |
| `dsp.js` | The sound maths, with nothing of the page in it: the streaming resampler (windowed sinc, a polyphase table where the rates allow: the mic's rate down to 16 kHz, Yap's 24 kHz voice up to the phone's rate), the answer's ring and jitter buffer (`Playout`), Int16, the frame level, hands-free's energy voice detector and its waits, the times as Chat says them. Runs in the page, both worklets and Node's tests. | `text/javascript` |
| `mic-worklet.js` | The mic's AudioWorklet: the mic at the phone's own rate, mixed to mono, resampled to 16 kHz Int16 (imports `dsp.js`), cut into 20 ms frames. | `text/javascript` |
| `player-worklet.js` | The answer's AudioWorklet: Yap's voice, already at the phone's rate, played as one stream from `dsp.js`'s `Playout` (the jitter buffer, its fades, underruns counted); reports to the page every ~50 ms. | `text/javascript` |
| `manifest.webmanifest` | Add to Home screen: name, the page's background as its colours (light, and dark in `user_preferences.color_scheme_dark` where the browser reads it), the icon. | `application/manifest+json` |
| `icon.svg` | The Yap mark (a copy of `src/assets/yap-logo.svg`; `tokens.spec.js` checks it's the same). | `image/svg+xml` |
| `README.md` | This file (not served). | — |

The `.js` files are ES modules: the PC must serve them as
`text/javascript`, or the browser refuses them (module scripts and worklets
check the type).

**No service worker**, on purpose: on an origin whose certificate Chrome only
accepted after a warning, registration fails. The page always comes from the PC,
which is fine: without the PC there's nothing to talk to.

**The headset's button** isn't used: a page can't own it while the mic is open
(the headset is in call mode, and Media Session only fires for media that
plays). That's for a native app (phone-remote design §4.5, §6.2).

## The look (2026-11-03)

**It's Yap's Chat in the Claude app's layout** (the day's third pass: the
maintainer wanted "screenshots … into the chat window on the phone" and the
model picker with its effort "in the chat window", not the header): the
header with ☰ and ⋯, the conversation filling the screen, a composer card
at the bottom, a drawer of chats. Yap's look throughout; where a phone needs
its own way, the phone wins.

- **The header**: ☰ on the left (the drawer; hidden for a PC from before
  it), the title in the middle — "Yap · <PC name>" with the connection's dot
  (green connected, amber connecting, grey unpaired), the chat's title under
  it, or how the connection is ("Reconnecting to Office PC…") —, a small
  amber spinner while Yap works the question out, and ⋯ on the right: the
  settings sheet (Hands-free and its wait, Clearer sound in a headset, the
  microphone, Sound details, Done). No rule under it, as the Claude app.
- **The thread**: your question in a bubble on the right (Chat's amber
  wash, a small tail at the bottom right, 🎙 "Asked by voice" when it was
  said; its pictures above it — their thumbnails while this page has them,
  else "2 pictures"); **the answer as plain text** the width of the screen,
  with its markdown (`markdown.js`) and, while it streams, Chat's amber
  caret; under a finished answer **Copy** and **▶ Read aloud** as quiet
  icons (Copy says ✓ for a moment). While an answer is being said, the bar's
  three bars and "Speaking…" under it, a status only: **one Stop at a time**
  — the composer's for a spoken answer (where the thumb is), or, when ▶ on
  an answer started it, that ▶ turned into ■ in place (the composer's stays
  away then). The step Yap is on as Chat's activity line (three amber dots,
  the step, the time) under the thread. Nothing asked yet: the Voice
  Assistant's ✦, "Ask Yap anything" in Yap's display serif, how to ask, and
  "Keep this page open…". A conversation opened with more messages than the
  PC sends: "Earlier messages are in Chat on your PC." at its top.
- **The composer card** (Yap's paper, a hairline, a soft shadow, 26 px
  corners): **the pictures** inside it, above the text — 88 px thumbnails,
  each with ✕ on its corner (a 44 px target), a shimmer while one is made
  ready; **the text box** ("Ask Yap, or hold to talk"); then a row — **+**
  (a small menu above it: Take photo, Choose photos), **the model chip with
  its effort** (the provider's mark, the model, "· Extra" — or "· where it
  runs" without an effort —, a chevron; a narrow card drops "Claude", where
  it runs and then the chevron, in `em` so it follows the text size: "Opus
  5.5 · Extra", as the Claude app's chip; it opens Chat's picker), a gap,
  **the mic** (round; Talk: a tap talks hands-free, a hold while held — a
  spoken *question*, never dictation) and **Send ↑** in Yap's amber (the
  Claude app's orange; dim while there's nothing to send), which is **■**
  while Yap works the question out (cancel) or says an answer (Stop) — unless
  there are words to send instead. **While it listens the card is the Yap
  bar's capsule**, the Voice Assistant's variant (`bar/DictationCapsule.svelte`):
  an amber edge, ✕ on the left, the ✦ and the bar's live waveform (thin amber
  bars, newest on the right, at least 7 % tall, `--yap-live`), the time, and
  the mic turned into the bar's ink pill saying **Done** (or "Release to
  send" while held, "Wait…" while the mic opens, with "Opening the
  microphone…" where the waveform goes). The round buttons are 44 px and
  grow with large text to 56 px at most (`--round`), so the row still fits
  at Android's largest text (200 %).
- **Chat's line for pictures its model can't read** (`Composer.svelte`'s
  vision line), in the card above the pictures: "Qwen3.5 4B can't read
  images." (or "couldn't read the picture", after the PC's `cant_read`), then
  **Use <the model that can>** (the PC's `imagesSwitch`: `set_model`, then
  the question goes), **Send its text instead** (read on the PC by Windows'
  OCR), Choose another model… (Chat's picker) and Send without the pictures.
- **The drawer** (`drawer.js`), from the left over a scrim, on Yap's paper:
  "Yap" in its display serif and ✕, a search box ("Search chats": every word,
  any case, without accents), **Recents** (each a chat bubble and its title,
  the one this phone is in marked), and at the foot the PC's name (a
  computer mark) and **+ New chat**. Esc, ✕ or a tap on the scrim closes it.
- **Tokens.** `link.css` copies `src/app.css`'s `--yap-*` values: `:root` for
  light, `:root[data-yap-theme='dark']` for Yap's warm dark, here following
  the phone's `prefers-color-scheme`. It can't import them (no build step, a
  page served on its own), so `e2e-link/tokens.spec.js` compares every
  `--yap-*` link.css sets with app.css's, in both themes: **change a colour in
  app.css and that test says which line here to change.** Only the font
  stacks (phone fallbacks added) and the page's own `--gutter` aren't
  app.css's. The page sits on Chat's content sheet (`--yap-s1`), which is
  also the address bar's colour (the `theme-color` metas) and the manifest's,
  light and dark.
- **Fonts.** Figtree for everything, EB Garamond (Yap's display serif, as
  Settings' and Insights' titles) for the pairing title, the empty thread's
  line and the sheets' titles. **Served from the PC, never Google Fonts**:
  the phone may have no internet. Cut down from the desktop's own files
  (`@fontsource-variable/figtree` 5.2.10 and `/eb-garamond` 5.2.7, their
  `latin` `wght-normal` subsets), with fontTools (`varLib.instancer`, then
  `subset`, keeping every name-table entry so the OFL copyright and licence
  stay in each file; neither font has a Reserved Font Name): Figtree's weight
  axis held to 400–700 (16.5 KB), EB Garamond fixed at weight 550 and Latin-1
  plus quotes, dashes and the ellipsis (19.8 KB). Both preloaded. A
  character a font lacks (→, ✓, a Polish ł in an answer) falls back to the
  phone's own font, as on the desktop.
- **Components.** Buttons as `ui/Button.svelte`'s (filled ones ink: dark on
  light, cream on dark; secondary ones a card with a hairline); switches as
  `ui/Toggle.svelte`'s (on is ink, not amber) on the right of their row, as
  Settings has them; the wait, ChatGPT's two ways and the effort as
  `ui/Segmented.svelte`; the microphone as `ui/Select.svelte`; Yap's focus
  ring (`--yap-focus`: a gap in the surface's colour, then amber) on every
  control.
- **The phone's own way.** Every target 44–48 px. Safe areas
  (`env(safe-area-inset-*)`, `viewport-fit=cover`). **The on-screen
  keyboard**: the composer rides above it — Chrome resizes the page for it
  (`interactive-widget=resizes-content` in the viewport meta), and elsewhere
  (iPhones) the page is fitted to the visual viewport (`--app-h`, `--app-top`
  from `visualViewport`, `link.js` `fitViewport`) —, and the thread keeps its
  end in view (unless you've scrolled up to read). Text in `rem`, so it grows
  with the phone's font size (`<meta name="text-scale">` for Chrome, Dynamic
  Type's body size on iPhones). The text box and the select at 16 px or more,
  so the phone doesn't zoom in on them. Nothing needs hover. On its side
  (landscape, up to 560 px tall): **two columns**, #17's idea kept — the
  conversation on the left, the composer card on the right at the bottom,
  under the thumb — and a slimmer header and empty state. Reduced motion:
  nothing pulses, blinks, spins or slides (the spinner a still ring).

## Chat's picker (2026-11-03)

The model chip in the composer card opens **Chat's own model picker**
(`views/chat/ChatModelPicker.svelte`) as a sheet from the bottom, with the
same structure and words: the **provider rail with its logos** across the
top (the picker's narrow layout: Recent, Groq, Gemini, Claude, ChatGPT,
OpenRouter, Custom when a server elsewhere is set up, On this PC; a green
dot when connected, hollow when not), the chosen provider's pane under it
(its mark on a tile, its name and status line: "Connected · checked 2 min
ago", "Not set up", "Using ChatGPT plan"), **Latest** — the newest of each of
its families — then **More models (N)** opening in place, ✓ on the model in
use, the tags ("Reads images", "New", "In use", "Recommended for this PC",
"no note tools", "Add key"), OpenAI's two ways as one **ChatGPT** entry with
**Your plan | API key** when both are connected, On this PC's installed
models and self-hosted servers, and **the effort** of the model in use (Low
… Max, its hint) with Fast mode where the model has it.

**Chat's own modules.** The rows aren't rebuilt for the phone: they're
`src/lib/core/chatPicker.js`'s, the module the desktop picker builds its rows
with too (from a `ctx` its stores fill; here `picker.js` fills it from the
PC's `ready.picker`, which is what the desktop picker reads: `link/models.rs`
`picker`). It and the pure modules it imports (`modelCatalog`: Yap's curated
lists merged with the live ones; `latestModels`: the families; `ppModels`,
`localContext`, `modelIcons`, `providerIds`, `effort`) are served as they are
at `core/`, the logos at `providers/` (`src-tauri/build.rs` `LINK_SHARED`).
So they must stay pure — no Tauri, no Svelte, no assets, no imports beyond
each other: `e2e-link/shared.spec.js` and `link/page.rs`'s tests hold them to
it.

**What stays on the PC**: adding a key or signing in with ChatGPT (the pane
says where: "Add an … API key in Yap on your PC: Settings → Language Models";
a provider's models show greyed with "Add key", as the desktop's do),
downloading or uninstalling an on-device model ("… to download in Chat's
model picker on your PC"), asking a self-hosted server for its list (the
model set up for it shows), and search. Picking sends `set_model` with the
row's provider, model, server or file (`chat_set_model` on the PC, its own
checks); the chip shows the pick at once, the PC's next `ready` has the last
word. Recent is the phone's own (`localStorage` `yap.link.recent`, the last
three). A PC from before the picker (no `ready.picker`) gets its plain list.

## The drawer and the phone's own conversation (2026-11-03)

☰ opens the drawer and asks the PC for Chat's conversations (`chats`: Chat's
list — not archived, not a meeting's Ask thread — newest first). Tapping
one opens it (`open_chat {id}`): its latest messages come back (`thread`:
the questions with 🎙 when said and how many pictures they carried, the
answers with their ids, so ▶ reads them), its title shows under the
header's, and **this phone's questions go there** (`chat: <id>` on `ask`
and `start`). **+ New chat** empties the thread (`new_chat`) and the next
question starts a conversation (`chat: "new"`); the PC says which it made
(`chat {id, title}`), and the questions after it go there. A phone that
never picked one sends no `chat`, and the PC asks it as the Voice Assistant
chooses — and says where it went (`chat`), which the page keeps from then
on. Whatever Yap was doing stops first (as Talk does). The conversation is
kept in `localStorage` (`yap.link.chat`: its id or `"new"`); once the page is
up it asks the PC for it again (`open_chat`), so a reload shows what was
written meanwhile, and one deleted or archived on the PC says so and starts
afresh. A rename in Chat on the PC (`title`) shows in the header and the
drawer. A PC from before the drawer (no `"chats"` in `ready.can`): no ☰,
and nothing about a conversation is sent.

## Pictures (2026-11-03)

**+** in the composer card opens a small menu: **Take photo** (`<input
type=file accept=image/* capture=environment>`: the camera) and **Choose
photos** (the phone's pictures, several at once). Each picture is made
ready on the phone (`photos.js`): read with `createImageBitmap(file,
{imageOrientation: 'from-image'})` (upright as the phone shows it), drawn on
white at 2,048 px on its longest side at most, and saved as a JPEG (quality
0.85; smaller tries until it's under 3 MB). It shows as a thumbnail inside
the card with ✕; four at most ("A question can carry 4 pictures at most"),
as Chat takes them. The pictures go with the next question, typed or spoken:
for each, `picture {n, size}` then its bytes in `0x03` frames of 32,000
bytes, in order, then `ask {…, pictures: N}` (or, for a spoken one, just
before `stop {pictures: N}`); the question's bubble shows them. Chat's model
known not to read images (`ready.picker.status.images` false) or the PC's
`cant_read`: **Chat's line** in the card ("The look", above) — the question
and its pictures back in the composer, then Use <the model that can>
(`set_model`, then the question), **Send its text instead** (`asText`: read on
the PC by Windows' OCR), Choose another model…, Send without the pictures.
Nothing of a picture is stored on the phone: the thumbnails live as long as
the page. A PC from before pictures (no `"pictures"` in `ready.can`): no +.

## Typing (2026-11-03)

A question typed in the composer goes to Yap's AI on the PC (`ask {text}`)
and is answered in the same conversation the spoken ones use, as Chat
answers a typed question, **not spoken**: the answer's ▶ Read aloud has Yap
say it (`read_aloud {id}`, the whole answer, on the phone). Enter sends,
Shift+Enter makes a new line; the box grows to five lines, then scrolls. The
question shows in the thread at once (without 🎙), Chat's activity line while
Yap works it out, ■ in Send's place to stop it. Typed while Yap speaks: its voice
stops first (`interrupt`); while it works on a question: that one's
cancelled (`cancel`). A PC from before typed questions (no `"ask"` in
`ready.can`): "Yap on your PC needs an update to answer typed questions.
Update Yap there; talking works as before." — nothing sent, the words kept in
the box. **It isn't dictation**: nothing is ever typed into an app on the PC.
The conversation is kept in this tab (`sessionStorage` `yap.link.thread`), so
a reload keeps it; closing the tab doesn't.

## What it does where the protocol leaves a choice

- **Pairing.** `#pair=<token>` in the address: hello with `pair: {token}` (and
  no `device`, so a code scanned again pairs afresh), the 4-digit code from
  `pair_wait` shown big, `paired`'s device kept in `localStorage`
  (`yap.link.device`), the fragment cleared. Later visits say hello with the
  device. No device and no code: how to pair, and no connection is tried.
- **`denied`.** How to pair, said plainly ("This phone isn't paired with Yap on
  your PC any more." / "Yap on your PC didn't allow this phone."; the `reason`
  is shown instead when it reads as a sentence, with a space in it). No
  reconnecting; the device is kept, with a Try again button. A refused pairing
  code is dropped.
- **The hello's `name`** is "Android · Chrome", "iPhone · Safari" and so on;
  `nonce` is 16 random bytes, base64url.
- **Up frames.** `start {mode: "ask"}`, then frames of exactly 320 samples (20
  ms at 16 kHz; the last one padded with silence), `seq` counting from 0 for
  each question, then `stop` after the last frame. At most 6,000 frames (2
  minutes): then it stops by itself.
- **Talk** (the mic). A tap starts, the next tap (or Done) sends; held for
  350 ms or more, letting go sends. Cancel (✕) while listening sends `cancel` (no `stop`). Hands-free (the
  settings sheet, ⋯): the wait chosen under it — **"Wait before sending": Short (1 s) · Normal (1.5 s,
  the default) · Long (2.5 s)**, remembered (`yap.link.wait`) — of quiet after
  speech sends `stop`; nothing said for 10 s sends `cancel`. Speech starts 12
  dB over the room's noise (and over −45 dB); once started it carries on while
  the level stays 6 dB over the noise and over −52 dB (hysteresis), so a soft
  last word isn't taken for quiet. For that hold the noise is also the room's
  typical (median) level before you spoke, so a fluttering noise (a TV) can't
  keep it going for ever.
- **Barge-in.** Talk while it speaks: playback stops at once, then `interrupt`,
  then `start`. Talk while Yap is working on a question: `cancel`, then `start`.
  After a `cancel` or `interrupt`, the old question's late `heard`, `step`,
  `answer`, `speak_*` and down frames are dropped, until a `state` of `idle` or
  `listening` arrives (or 1.5 s pass).
- **`answer.text`** is taken as the whole answer so far (each one replaces the
  last), not a piece to add; `final: true` marks it done. It's shown as Chat
  shows an answer (`markdown.js`: its markdown as nodes, Chat's caret until
  it's final); `[1]`-style citations are left out, and links show as their
  words.
- **`state`.** idle → "Ready", listening → "Listening", transcribing and
  thinking → "Working it out", speaking → "Speaking". A `state idle` while the
  phone is listening (after the PC's own `listening`) ends the question on the
  phone without a `stop`: the PC ended it.
- **`step`** shows as Chat's activity line does: the label, then the time since
  the question was sent ("Searching the web · harbour tides · 4s"); "Thinking"
  until a step comes; gone once the answer's final.
- **Down frames** play as **one stream**. Each is resampled from
  `speak_start`'s `rate` (24 kHz) to the AudioContext's (48 or 44.1 kHz) as it
  comes, in one windowed-sinc filter carried on from frame to frame, and posted
  to `player-worklet.js`, which plays from a jitter buffer (`dsp.js`
  `Playout`): silent until **250 ms** is waiting (or `speak_end` came), then
  faded in over 5 ms. **Running dry** (the next frame late): the last 5 ms fade
  out, an underrun is counted, and it waits for **150 ms** more before fading
  back in — never a hard gap. A frame that lands just in time only dips.
  **Stop / barge-in**: silent within 5 ms (a fade, not a cut), what's left
  dropped. `speak_end`, or a `state idle`, plays out what's left (its last 5
  ms faded); then the answer's finished and the mic's 15 s starts. Before,
  each frame was its own `AudioBufferSourceNode`: the browser resampled each
  clip on its own (a click at every join) and a late one left a hard gap.
- **Clearer sound in a headset** (a switch under Hands-free in the settings sheet, **on by default**
  for anyone who hasn't chosen; a saved choice is kept, `yap.link.clearer`):
  "A headset's call mode can crackle, so this closes the mic while Yap talks
  and the headset uses its music mode. You can't talk over Yap by voice; Stop
  still works." With the mic open a Bluetooth headset sits in its call link
  (HFP/SCO: narrowband, packet-loss artefacts that sound like crackles); closed,
  it can play the answer over its music link (A2DP). On by default since 31 Oct:
  that crackle was heard in a real headset and went with this on. The cost:
  Talk while Yap speaks opens the mic afresh (a moment while the headset
  switches back) instead of listening at once. When on, the mic's tracks stop
  as soon as the question is sent (so the headset can switch while Yap works it
  out) and, in any case, at `speak_start`; switched on mid-answer, at once. The
  mic opens again on Talk, or, with hands-free on, once the answer has played
  out (kept warm 15 s as usual). Off, the mic stays open through the answer and
  Talk while it speaks listens on it straight away.
- **Opening the mic never waits silently.** Each step has a time limit and a
  reason on screen when it runs out (`doOpenMic`): the permission's state where
  the browser says (`navigator.permissions.query`, 1 s; Firefox may not know
  `microphone` there), then the recorder's module and the player's
  (`mic-worklet.js`, `player-worklet.js`: **one at a time**, never both
  `addModule`s at once on a context, a suspect in Firefox; 5 s each, then "The
  audio setup didn't finish: reload the page, then try again.", and the next
  Talk sets the sound up afresh on a new AudioContext), `getUserMedia` (after 6
  s, or at once when the browser says the mic is blocked: "Waiting for the
  microphone permission. Tap the icon left of the address bar → Microphone →
  Allow, then tap Talk again.", and it waits on: answering the prompt late
  still works, Talk then tries afresh, Cancel gives up), the phone's sound
  (`AudioContext.resume()`, asked in Talk's press and again once the mic is
  open, as Firefox lets a page that records start its sound: 3 s once Talk is
  let go, then "The phone's sound didn't start. Tap Talk again, or reload the
  page."), and sound from the mic (below). **Cancel** always works at any step:
  that opening is given up (a mic it gets later is stopped at once) and Talk is
  back. A touch's press may not count as a gesture yet (its release does), so
  the sound is asked to start again on every release, tap or key.
- **A tap or a hold** (350 ms) is judged by the release's own `timeStamp`, not
  when the page handled it, and counted from when the press was handled, not
  when it came: setting the sound up can keep the page busy in the press
  (Firefox's `new AudioContext()` takes 0.1–0.4 s), and a finger lifted
  meanwhile is a tap — before, a quick tap there read as a too-short hold
  ("The microphone took a moment to open…") and nothing was asked.
- **A new mic waits for sound**: after `getUserMedia`, Talk stays at "Opening
  the microphone…" until the track is live and unmuted (a headset switching
  back to its call link), at most 3 s, so no frame is counted (or VAD'd) before
  the mic really sends sound; still nothing then, it listens anyway and says so
  ("The microphone hasn't sent any sound yet…").
- **Sound details** (a disclosure at the foot of the settings sheet): the AudioContext's sample
  rate, `baseLatency` / `outputLatency` where the browser gives them, the last
  answer (seconds played, how long after its first sound it started and how
  much was buffered then), its gaps (underruns), whether the mic was open while
  it played (and now), **opening the microphone** (the step it's on and for how
  long, live — "Asking for the microphone… 7.2 s so far" — or where the last
  one ended: "Waiting for sound from the mic: 120 ms (open after 410 ms)",
  "Loading the recorder: stopped after 5.0 s", "…: cancelled after …"), and
  the browser. After a crackly answer: gaps mean the stream ran dry here;
  none, with the mic open, points at the headset's call link.
- **`set_model`.** Sent with the picked row's `provider`, `model`, `baseUrl` and
  `file` (with its `id` for a PC from before the picker). The chip shows the
  picked model at once; a `ready` the PC sends afterwards has the last word.
  `set_effort` and `set_fast` are sent as the effort and Fast mode change.
- **`ask` and `read_aloud`.** Before either, while Yap speaks the page stops
  its voice and sends `interrupt`, while it works it sends `cancel` (as Talk
  does), and drops the old question's late news until the PC's `state idle`.
  ▶ shows under an answer once its `answer.id` came (and the PC says
  `read_aloud` in `can`); the turn being said shows "Speaking…" (its ▶ as
  Stop when ▶ started it, else no button: the composer's Stop) until the
  voice has played out, Stop is pressed or the PC is idle. One Stop on the
  page at a time.
- **Keep-alive.** A `ping` every 15 s. Anything from the PC counts as alive;
  nothing for 8 s after a ping and the connection is dropped and made again.
  Reconnects back off from 0.5 s to 15 s (with jitter), saying "Reconnecting to
  <PC name>…"; coming back on screen or online tries at once.
- **The mic** opens on Talk with `echoCancellation`, `noiseSuppression` and
  `autoGainControl`, at the device's rate, and stays open until 15 s after the
  answer ends, so a Bluetooth headset keeps one call link (SCO) from the question
  to the end of the answer, and a follow-up starts at once (when Clearer sound
  is off, above). The chosen mic is
  remembered (`yap.link.mic`, by id and name); if it's gone, the phone's own is
  used and the page says so.
- **The screen** is kept on with a wake lock, taken again whenever the page
  comes back on screen.

## Tests

```bash
npm run test:link
```

- `e2e-link/dsp.test.mjs` (Node's test runner): the resampler (48 kHz and
  44.1 kHz in → exactly 16,000 samples a second, the same sine; any block size;
  aliasing filtered; DC; 24 kHz up to 48 and 44.1 kHz: a sine in 7 odd-sized
  pieces the same as whole, exactly 48,000 / 44,100 samples a second, images
  gone), the ring, the playout (silent until 250 ms, the fade in, running dry:
  a 5 ms fade, one underrun, 150 ms more, the fade back; a frame just in time
  only dips; the end played out; Stop's fade; frames resampled one after
  another played without a seam), Int16, the voice detector (each wait, a soft
  last word kept, a fluttering room neither starting nor holding it), the times.
- `e2e-link/*.spec.js` (Playwright, `e2e-link/playwright.config.js`): the page
  in a headless Chromium (Playwright's own if it's installed, else Edge;
  `YAP_LINK_CHANNEL` to choose) at a made-up HTTPS origin served by
  `page.route`, with Yap on the PC mocked by `page.routeWebSocket` and the mic
  faked from WAV files the setup writes (`--use-file-for-fake-audio-capture`):
  pairing, a later visit, denied, reconnecting, pings, the wake lock, Talk's
  frames (kind, size, `seq`, and the rate, from a 440 Hz tone's pitch), hold to
  talk, what's shown while Yap works, the answer's markdown (bold, lists one
  inside another, code, a heading; citations gone, a half-written mark without
  its stars, a link as its words, a model's HTML as text and never run), the
  answer as one stream played to the
  end, Stop (silent, a fade), barge-in, Cancel, a PC from before the picker
  (the chip, On this PC said once, its plain list), the mic picker, hands-free
  and its wait choice. `typing.spec.js`: a typed question (`ask` over the
  wire, the answer in the thread, nothing spoken and no mic), Enter and
  Shift+Enter and the box growing, ▶ Read aloud and its ■, typing over Yap
  speaking or working, a PC from before typed questions, the keyboard up
  (the page resized for it, or fitted to the visual viewport), a reload
  keeping the thread. `picker.spec.js`: Chat's picker — the rail in Chat's
  order with its logos (the PC's own files, loaded) and dots, opening on the
  model in use, Latest and More models in place, a switch as `set_model`,
  the PC's word after, Recent, a provider without a key, ChatGPT's two
  ways, the effort and Fast mode, a PC from before. `chats.spec.js`: the
  drawer — ☰, Recents from the PC with the PC's name and + New chat, the
  search (every word, any case), Esc; a chat opened (`open_chat`, its
  messages with 🎙 and a picture counted, its title under the header's, ▶ on
  an old answer by its id, the next `ask` and `start` with its `chat`, marked
  in the drawer); + New chat (`new_chat`, `chat: "new"`, the PC's `chat`
  then the next question there, a rename's `title`); a reload asking again,
  one gone on the PC; no `chat` from a phone that never picked one; a PC from
  before (no ☰). `pictures.spec.js`: + → Choose photos (thumbnails with ✕,
  a 3,000 × 2,000 photo made 2,048 × 1,365, under 3 MB), a typed question
  with two (`picture` and its pieces whole and in order, then `ask` with
  `pictures: 2`, JPEGs of the right size, the bubble's thumbnails), a spoken
  one (the pieces after the audio, before `stop {pictures: 1}`), four at
  most, Chat's line when its model can't read images (Send doesn't send past
  it; "Send its text instead" as `asText`; Use <model> as `set_model` then
  the question), the PC's `cant_read` (the question and its picture back in
  the composer; Send without them), a PC from before (no +).
  `shared.spec.js` (no
  browser): the desktop modules served at `core/` import only each other,
  nothing of the app, and the logos the picker draws are served. `sound.spec.js`: the
  stream starting at ~250 ms, a short answer, a late frame (one underrun, never
  a cut: the largest step between samples stays the tone's own), Clearer sound
  (on for a fresh page, a saved off or on kept; the mic's tracks stopped at
  `speak_start` and when a question is sent, opened again after with
  hands-free; off, Talk over Yap listens on the mic already open, on, it opens
  it afresh), Sound details. `mic-open.spec.js`: a `getUserMedia` nobody
  answers (the permission message after ~6 s, Talk and Cancel, Cancel
  recovering, the late mic stopped), answered late, allowed and Talk tried
  afresh, blocked (said at once), an `addModule` that never resolves (the setup
  message, the player's never started beside it, Talk working after on a new
  context, one module at a time), the sound never starting, Cancel at any step,
  and Sound details' step all along; `link-page.spec.js` also has a tap whose
  release the page handles late, and one whose press a slow AudioContext keeps
  busy. `screens.spec.js` leaves named screenshots at 360 × 780 and
  390 × 844, light and dark (pairing, empty with the chip's effort, the
  drawer, a chat opened from it, typing with the keyboard up, answering with
  ■, listening, two pictures in the composer, the model sheet, the settings
  sheet; landscape's two columns, large text at 130 and 200 %), in
  `test-results/link/screenshots/`, and checks every control is 44
  px or more, Yap's two fonts and the logos load from the page's own origin
  (nothing asked of anywhere else), and at 360 × 780, 390 × 844, 430 × 932 and
  on its side (844 × 390) nothing scrolls sideways and the composer and Talk
  are on the first screen, Talk low and on the right, under the thumb; at
  200 % text the row still fits, the model's name with room.
  `tokens.spec.js` (no browser): link.css's `--yap-*` values are app.css's,
  light and dark; the `theme-color` metas and the manifest are the page's
  background in each; `icon.svg` is Yap's mark. Nothing on the network, no Yap
  build, no window on screen.
- **In Firefox too** (the `firefox` project, when Playwright's own Firefox is
  installed: `npx playwright install firefox`; `YAP_LINK_FIREFOX=0` leaves it
  out): `link-page`, `sound`, `mic-open`, `typing`, `picker`, `chats` and `pictures`, with Firefox's fake mic
  (`media.navigator.streams.fake`, `media.navigator.permission.disabled`, its
  tone at 440 Hz) and its output muted (`media.volume_scale`). Hands-free needs
  Chromium's recorded speech, and the screenshots are Chromium's.

One thing the harness does that a phone doesn't: Playwright can't route a
worklet's own requests, so the tests load `mic-worklet.js`,
`player-worklet.js` and `dsp.js` through the page and hand each worklet one
inlined module (`support/harness.js`). On a phone the worklets load them the
ordinary way.
