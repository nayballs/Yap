# Yap Link: the phone's page

The page a phone opens to talk to Yap's AI on the PC, through a Bluetooth headset
or the phone itself: ask out loud, hear the answer. Yap on the PC embeds these
files in `yap.exe` and serves them over HTTPS on the home Wi-Fi (port 51443, a
self-signed certificate the phone accepts once), with the WebSocket at `/ws`.

Plain HTML, CSS and JavaScript modules: no build step, no framework, nothing
fetched from anywhere but the PC (no CDN, no web fonts; system fonts). About
101 KB as served, 31 KB gzipped.

**The protocol** (v1: the hello, pairing, the JSON messages and the binary
audio frames) is written down in
[`docs/architecture/link.md`](../../docs/architecture/link.md). This page keeps
to it exactly; the notes below are what it does where the protocol leaves a
choice.

## The files

| File | What it is | Served as |
|---|---|---|
| `index.html` | The page: the header (Yap, the connection, the model chip), pairing, the conversation, Talk, Stop / Cancel, Hands-free and its wait, Clearer sound in a headset, the microphone picker, Sound details, the model sheet. | `text/html` |
| `link.css` | The look: Yap's warm light and dark `--yap-*` tokens (values copied from `src/app.css`, following `prefers-color-scheme`), mobile first, 48 px targets, reduced motion respected. | `text/css` |
| `link.js` | Everything the page does: the connection (hello, pairing, reconnects with backoff, keep-alive pings), the mic, Talk (tap or hold), hands-free, the spoken answer (resampled frame after frame, posted to the player worklet), Clearer sound, barge-in, the conversation, the step and its time, the model chip, Sound details, the screen wake lock. Exposes a read-only `window.__yapLink.snapshot()` for the tests. | `text/javascript` |
| `dsp.js` | The sound maths, with nothing of the page in it: the streaming resampler (windowed sinc, a polyphase table where the rates allow: the mic's rate down to 16 kHz, Yap's 24 kHz voice up to the phone's rate), the answer's ring and jitter buffer (`Playout`), Int16, the frame level, hands-free's energy voice detector and its waits, the times as Chat says them. Runs in the page, both worklets and Node's tests. | `text/javascript` |
| `mic-worklet.js` | The mic's AudioWorklet: the mic at the phone's own rate, mixed to mono, resampled to 16 kHz Int16 (imports `dsp.js`), cut into 20 ms frames. | `text/javascript` |
| `player-worklet.js` | The answer's AudioWorklet: Yap's voice, already at the phone's rate, played as one stream from `dsp.js`'s `Playout` (the jitter buffer, its fades, underruns counted); reports to the page every ~50 ms. | `text/javascript` |
| `manifest.webmanifest` | Add to Home screen: name, colours, the icon. | `application/manifest+json` |
| `icon.svg` | The Yap mark (a copy of `src/assets/yap-logo.svg`). | `image/svg+xml` |
| `README.md` | This file (not served). | — |

The four `.js` files are ES modules: the PC must serve them as
`text/javascript`, or the browser refuses them (module scripts and worklets
check the type).

**No service worker**, on purpose: on an origin whose certificate Chrome only
accepted after a warning, registration fails. The page always comes from the PC,
which is fine: without the PC there's nothing to talk to.

**The headset's button** isn't used: a page can't own it while the mic is open
(the headset is in call mode, and Media Session only fires for media that
plays). That's for a native app (phone-remote design §4.5, §6.2).

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
- **Talk.** A tap starts, the next tap sends; held for 350 ms or more, letting
  go sends. Cancel while listening sends `cancel` (no `stop`). Hands-free: the
  wait chosen under it — **"Wait before sending": Short (1 s) · Normal (1.5 s,
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
  last), not a piece to add; `final: true` marks it done. Markdown's marks and
  `[1]`-style citations are left out of what's shown.
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
- **Clearer sound in a headset** (a switch under Hands-free, off by default,
  `yap.link.clearer`): "Closes the mic while Yap talks, so the headset can use
  its music mode. You can't talk over Yap by voice; Stop still works." With the
  mic open a Bluetooth headset sits in its call link (HFP/SCO: narrowband,
  packet-loss artefacts that sound like crackles); closed, it can play the
  answer over its music link (A2DP). When on, the mic's tracks stop as soon as
  the question is sent (so the headset can switch while Yap works it out) and,
  in any case, at `speak_start`; switched on mid-answer, at once. The mic opens
  again on Talk, or, with hands-free on, once the answer has played out (kept
  warm 15 s as usual).
- **A new mic waits for sound**: after `getUserMedia`, Talk stays at "Opening
  the microphone…" until the track is live and unmuted (a headset switching
  back to its call link), at most 3 s, so no frame is counted (or VAD'd) before
  the mic really sends sound.
- **Sound details** (a disclosure at the bottom): the AudioContext's sample
  rate, `baseLatency` / `outputLatency` where the browser gives them, the last
  answer (seconds played, how long after its first sound it started and how
  much was buffered then), its gaps (underruns), whether the mic was open while
  it played (and now), and the browser. After a crackly answer: gaps mean the
  stream ran dry here; none, with the mic open, points at the headset's call
  link.
- **`set_model`.** The chip shows the picked model at once; a `ready` the PC
  sends afterwards has the last word.
- **Keep-alive.** A `ping` every 15 s. Anything from the PC counts as alive;
  nothing for 8 s after a ping and the connection is dropped and made again.
  Reconnects back off from 0.5 s to 15 s (with jitter), saying "Reconnecting to
  <PC name>…"; coming back on screen or online tries at once.
- **The mic** opens on Talk with `echoCancellation`, `noiseSuppression` and
  `autoGainControl`, at the device's rate, and stays open until 15 s after the
  answer ends, so a Bluetooth headset keeps one call link (SCO) from the question
  to the end of the answer, and a follow-up starts at once (unless Clearer sound
  is on, above). The chosen mic is
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
  talk, what's shown while Yap works, the answer as one stream played to the
  end, Stop (silent, a fade), barge-in, Cancel, the model chip (On this PC said
  once), the mic picker, hands-free and its wait choice. `sound.spec.js`: the
  stream starting at ~250 ms, a short answer, a late frame (one underrun, never
  a cut: the largest step between samples stays the tone's own), Clearer sound
  (the mic's tracks stopped at `speak_start` and when a question is sent, opened
  again after with hands-free), Sound details. `screens.spec.js` leaves named
  screenshots, light and dark, in `test-results/link/screenshots/`. Nothing on
  the network, no Yap build, no window on screen.

One thing the harness does that a phone doesn't: Playwright can't route a
worklet's own requests, so the tests load `mic-worklet.js`,
`player-worklet.js` and `dsp.js` through the page and hand each worklet one
inlined module (`support/harness.js`). On a phone the worklets load them the
ordinary way.
