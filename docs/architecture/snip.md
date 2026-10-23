# Snip to Chat

Drag a box over any screen, and the picture goes into Chat for the selected
AI model (2026-10-23). The capture, the HDR conversion, the native overlay,
the chat screenshots on disk, which models read images and how a question
carries one. Chat itself is [chat-and-assistant.md](./chat-and-assistant.md);
the models are [ai-models.md](./ai-models.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## How it works

```
Win+Alt+S · ✂ on the bar · tray "Snip to Chat" · Chat's + · saying "snip"
        │   (the shortcut again while it's up: cancel)
        ▼
snip::start ─ refused during a dictation ("Finish the dictation first")
        │      remembers the window in front (focus back on a cancel) and
        │      where it goes: the conversation Chat shows, if the main window
        │      is on screen on one, else a new chat
        ▼
capture::for_snip(true) ─ every Yap window on screen leaves the capture
        │                 (WDA_EXCLUDEFROMCAPTURE, one DwmFlush)
        ▼
snip/capture.rs ─ every monitor at once: WGC, FP16 + SDR white on an HDR
        │         monitor, BGRA8 elsewhere, GDI if WGC can't
        ▼
capture::for_snip(false) ─ back as the meeting setting wants
        ▼
snip/overlay.rs ─ one native window per monitor, the frozen picture dimmed,
        │         painted while cloaked, uncloaked together, the focus taken
        ├── Esc · right-click · the shortcut again · another app · the
        │   monitors changing · two minutes: cancelled, focus back
        ▼
release (≥ 6×6 px) → crop at physical pixels → PNG → <data>/chat-images/
        ▼
yap-snip {image, conversationId} → the main window up on Chat, the
        screenshot in the message box, the caret there — never sent by
        itself; the copy for the selected model made in the background
```

## Modules

- **`snip.rs`** — the feature's root: [`start`] (its own thread), the
  state (`CURRENT`, the generation, `Last` for the tests and the log),
  [`finish`] (save, hand to Chat, or give the focus back), the voice phrase
  (`is_snip_phrase`: "snip", "snip it", "snip this", "take a snip",
  "screenshot", "take a screenshot", "screen shot", "please" either side —
  only the whole utterance), the page's commands (`snip_start {origin}`,
  `chat_images_usage`, `chat_images_prepare`), the `yapsnip` URI scheme
  (`serve`: a chat screenshot's PNG by id only, `http://yapsnip.localhost/<id>`
  in WebView2, anything else a 404) and `init` (the capture device warmed 8 s
  after launch when the shortcut is bound; the retention sweep 30 s after
  launch and daily).
- **`snip/capture.rs`** — monitors (`EnumDisplayMonitors`, physical pixels,
  per-monitor DPI) and their HDR state from DisplayConfig
  (`QueryDisplayConfig` → each path's GDI source name, its advanced colour
  state — HDR = enabled and not "wide colour enforced", which is auto colour
  management on an SDR display — and `DISPLAYCONFIG_SDR_WHITE_LEVEL`, "a
  multiplier of 80 nits × 1000"). **Windows.Graphics.Capture**, one frame per
  monitor, every monitor's session started together on one warm Direct3D 11
  device (its own MTA thread, `snip-capture`): `R16G16B16A16Float` on an HDR
  monitor, `B8G8R8A8UIntNormalized` elsewhere, the cursor off, the yellow
  border off (`GraphicsCaptureAccess::RequestAccessAsync(Borderless)`, asked
  once — Windows allowed it on the maintainer's PC). A monitor whose frame
  doesn't come within 800 ms, or that WGC refuses (Windows' "Let desktop apps
  take screenshots" switch), is read with **GDI** (`BitBlt` | `CAPTUREBLT`:
  SDR only). Ported from robmikh/screenshot-rs and windows-capture (MIT,
  [THIRD-PARTY-NOTICES.md](../../THIRD-PARTY-NOTICES.md)); no capture crate
  (both pull a second `windows` crate, 0.62, and neither converts HDR).
- **`snip/hdr.rs`** — pure, unit-tested: FP16 ↔ f32, the sRGB curve, and
  `ToSrgb8`, one 65,536-entry table per white level from a channel's FP16
  bits straight to its 8-bit sRGB value (`v × 80 / white_nits`, negatives →
  0, highlights clip; never a frame-adaptive tone map — ShareX #8895's
  bug), the frame converted on every core.
- **`snip/geom.rs`** — pure: the drag's box (clamped to its monitor; a
  click or a twitch under 6 px snips nothing), the size label's place, a
  box scaled to a frame of another size, the crop (BGRA → RGBA), the
  monitor under a point, `scaled` (the overlay's own lengths at a monitor's
  DPI). Physical pixels end to end, so mixed DPI needs no conversion.
- **`snip/overlay.rs`** — the native overlay (below).
- **`snip/images.rs`** — the screenshots on disk and the copies models get
  (below).
- **`snip/turn.rs`** — a question's screenshots on their way to the model
  (below).
- **`snip/vision.rs`** — which models read images, and the switch (below).
- **`snip/dump.rs`** — `yap.exe snip-dump` (debug builds), the spike.

## The overlay: native, not a webview (the spike)

`yap.exe snip-dump --out <folder> [--runs N] [--no-overlay]` captures every
monitor with each path (WGC + Yap's HDR conversion, WGC 8-bit, GDI), times
it, writes the PNGs, shows the overlay over the frozen pictures
click-through and never focused (it takes neither the mouse nor the keys),
and checks a window of known swatches. On the maintainer's PC (two
2560×1440 monitors, the primary HDR at 240 nits, Windows 11 build 26200):

| | debug build | optimised build |
|---|---|---|
| Direct3D device, first time | 127–162 ms | 118 ms |
| every monitor captured, warm (FP16 conversion of the HDR one) | 38 ms (12 ms) | 26 ms (1.5 ms) |
| overlay painted and on screen after the capture | ~48 ms | ~23 ms |
| **request → overlay on screen** | **87–91 ms** | **50–61 ms** |
| a full monitor as PNG (`Compression::Fast`) | 60–85 ms* | 6–7 ms |

\* with the PNG crates optimised in dev builds too (`[profile.dev.package]`
in Cargo.toml); unoptimised it was 1.4 s.

A webview overlay would have to be handed each monitor's frame — 15 MB raw at
1440p, 33 MB at 4K, through WebView2's resource handler, or a PNG encode
first — then decode and draw it, inside WebView2's own compositor. A native
window paints straight from memory: **the native overlay was chosen**, after
PowerToys ZoomIt's `SelectRectangle` (MIT) with Kiri's per-monitor physical
placement (MIT). The e2e suite can't drive it over CDP, so test mode drives
it with posted window messages instead.

How it looks and behaves:

- One `YapSnipOverlay` window per monitor (`WS_POPUP`, topmost, tool
  window), made once and kept **hidden and DWM-cloaked** (Yap's rule: never
  "parked off-screen"), DWM's transitions off. A snip places each on its
  monitor at physical pixels (`WM_DPICHANGED` ignored: its size is the
  monitor's), paints it while cloaked (`RedrawWindow` with
  `RDW_UPDATENOW`), then uncloaks them all and waits one composition pass
  (`DwmFlush`): the picture is whole from its first frame — no white or
  half-drawn one, no flicker.
- The frozen picture, dimmed (each channel × 150/256, on every core); the
  box at full brightness with a 1.5 px (× scale) edge in Yap's amber; the
  size in physical pixels ("812 × 430") in an ink pill by its corner (below,
  else above, else inside); "Drag to snip · Esc to cancel" at the top of the
  monitor under the cursor, fading after 2.5 s. Repaints compose each changed
  rectangle off screen and copy it in one go; labels are anti-aliased pills
  (software coverage, `AlphaBlend`), text in Segoe UI Variable.
- Its colours are the bar's tokens (`--bar-tooltip`, `--bar-tooltip-key`,
  `--bar-chip`): a native window can't read app.css, so a test reads app.css
  and checks they match.
- Input: drag (the mouse captured past the monitor's edge, clamped to it);
  release = done; Esc, right-click, the shortcut again, another app taking the
  focus (`WM_ACTIVATEAPP`), the monitors changing (`WM_DISPLAYCHANGE`) or two
  minutes cancel. If Windows didn't let the overlay take the focus, Esc still
  cancels through Yap's keyboard hook (`input_hook::snip_escape`: swallowed
  while a snip is up). Phase 1 keeps a box on the monitor it started on.

## No Yap window in the shot

`capture::for_snip(app, true)` gives every Yap window on screen
`WDA_EXCLUDEFROMCAPTURE` for the moment of the capture (the bar, the main
window when the snip came from Chat or its in-page shortcut, the notepad),
then waits one composition pass; `for_snip(false)` puts back what the
meeting setting wants (`meeting_hide_from_capture` keeps the meeting
windows excluded while a meeting records). The overlay windows themselves
come up after the capture, so they're never in it. Yap's meeting windows
are left out of every capture while a meeting records anyway (their own
setting): they never show in your own snips then either.

## HDR, done right

With HDR on, Windows composes in scRGB (FP16, linear, 1.0 = 80 nits) and
boosts every SDR app to the SDR white level; an 8-bit read of that desktop
clips everything above `80 / white` linear — the washed-out screenshot.
Yap captures FP16 on an HDR monitor and divides the boost out
(`snip/hdr.rs`). The round trip is exact (unit-tested: every 8-bit sRGB
value through Windows' boost at 80, 120, 200, 280 and 480 nits comes back
within ±1; at 200 nits #C0C0C0, #E8E8E8 and #F5F5F5 exactly).

**On the maintainer's PC** (`snip-dump`'s swatch check: #202020, #808080,
#C0C0C0, #E8E8E8, #F5F5F5 and a red painted by a plain GDI window on each
monitor): Yap's FP16 path read every swatch exactly (worst error 0) on the
240-nit HDR monitor and on the SDR one. On this Windows build (26200) the
8-bit WGC and GDI reads of that plain GDI window were exact too — Windows
now composes an 8-bit capture of SDR windows without the boost. The case
that still washes out is an **HDR-aware app** (Chromium, WebView2, Discord)
that draws SDR content into an FP16 surface at the white level itself
(ShareX #8895): the robot test (`e2e-real/snip.real.js`) snips swatches on
an Edge page on every monitor for that. Real HDR highlights (video, games)
clip to white in phase 1.

## The shortcut

Win+Alt+S by default (`snip_hotkey`, "S for snip", beside Win+Alt+M),
recorded in Settings → Shortcuts like the others, never sharing a key
(`input_hook::clashes`; `configure_snip_from` leaves it unbound if a
hand-edited config clashes, and the log says why). Checked free on the
maintainer's PC with `RegisterHotKey` (2026-10-23): Win+Alt+S, H, X, C, Q
and Z are free; Win+Alt+R, B, G, T, K, D, PrtScn, Win+Shift+S and Win+S are
taken by Windows and Game Bar (Win+Alt+M is registered by another program
too; Yap's hook sees it first, for meeting notes). The hook taps the
menu mask with it, so letting go of Win and Alt opens nothing. The main
window's in-page fallback (`Settings.svelte`) starts a snip too, since the
global hook never sees keys while a Yap window has the focus.

## Into Chat

- `yap-snip {image, conversationId, origin}` goes to the main window, which
  comes up (`commands::show_settings`); ControlPanel switches to Chat
  (`chatRequest.pending = {snip}`), or ChatView takes it itself while it's
  open (`takeSnip`). **Which conversation**: the one Chat shows, when the
  main window was on screen on one at the snip (`assistant::chat_viewing`
  + `notice::window_view`); else a new chat.
- The composer: the thumbnail (52 px high, its size, ✕), the three one-tap
  prompts — **What's this?**, **Explain this error**, **Summarise** (each
  sends at once, with the screenshot) — "Ask about this screenshot…" in the
  box, the caret there (dictating into it works: Chat's mic, or the hotkey's
  in-page fallback). **Enter on an empty box asks "What's this?"**. Up to 4
  screenshots a question (snipping again adds one). They wait while another
  view shows (`chatShots.svelte.js`). Never sent by itself.
- A question keeps its screenshots: a thumbnail in its bubble (click: large;
  Esc closes), or "Screenshot removed after 30 days" (the setting's period)
  once the sweep removed it (`chat_get` marks it `gone`).
- + → "Take a screenshot" starts one from Chat (the main window leaves the
  shot).

## Which models read images

`ai_model_status` says `images` (true / false / null) and `imagesSwitch`
for the Chat model (`snip/vision.rs`), most certain first:

1. **a refusal this session**: a server that turned an image down
   (`vision::is_image_refusal`: OpenRouter's "No endpoints found that support
   image input", llama-server without its projector, a 400 naming
   `image_url`) makes that model a "no" until restart;
2. **the provider's own list** (`model_catalog`, `CatalogModel.vision`):
   Anthropic's `capabilities.image_input.supported`, Groq's and
   OpenRouter's input modalities;
3. **family rules** where the list doesn't say: every Claude model; every
   `gemini-*` chat model; OpenAI's GPT-4o, 4.1, 5.x, 6, o1, o3, o4-mini (not
   gpt-oss, o1-mini, o3-mini, 3.5); ids that say so ("vision", "vl",
   "llava", "pixtral", Groq's `qwen3.8`); **the on-device model: no**
   (its projector is phase 3).

Anything else — a self-hosted server, an id nothing says about — is **not
known**: Chat lets it try, and a refusal teaches it.

When the model can't: one amber line above the box, "**GPT-OSS 20B can't
read images.** [Use Qwen3.8 27B on Groq] [Choose another model…] Send
without the screenshot", Send and the prompts waiting meanwhile. The switch
(`vision::suggest`) is the same provider first (same key, no setup), then
every provider with a key in Settings' order; each provider's pick is Yap's
curated one its live list has (`PICKS`: Groq `qwen/qwen3.8-27b`, Anthropic
`claude-haiku-5-5`, Gemini `gemini-3.6-flash`, OpenAI `gpt-5.6-luna`,
OpenRouter `openai/gpt-5-mini`), else its list's first model that reads
images. One click: `chat_set_model`, the copies for the new model made
(`chat_images_prepare`). No provider has one: "None of your models can:
Claude, Gemini and OpenAI models can. [Connect one…]" (the model picker).
`chat_send` refuses a question with screenshots to a model known not to
read them (`vision::CANT_READ`), and turns a server's refusal into
`vision::COULDNT_READ`: the page shows the line instead of an error, the
question and its screenshot back in the box.

## A question's screenshots on their way (`snip/turn.rs`)

- Every provider takes the same OpenAI-style part, `{"type": "image_url",
  "image_url": {"url": "data:image/png;base64,…"}}`, in a user message:
  the images first, then the text (Anthropic's advice); **the text first for
  OpenRouter** (its advice); OpenAI's gets `detail: "auto"`.
- `chat_send(images: [ids])` checks the model, fits the copies
  (`turn::prepare`, off the async runtime) and runs the turn inside a
  task-local (`turn::with`); the two places that put the question in a
  request — `tools::tool_loop` (every step re-sends the messages) and
  `llm::note_chat` (a model without tools) — call `turn::attach`, which
  makes the question's message a content array. `chat_turn` saves the
  question with its images (`chats::append_question`). Outside a turn with
  screenshots nothing changes.
- **Earlier screenshots** (`turn::with_history_images`): the newest 3 in
  the history window are sent again (fewer within the provider's cap: Groq
  takes 3 a request in all), each older one becomes "(An earlier
  screenshot, not sent again.)", a removed one "(A screenshot was here; it
  was removed.)". None to a model on this PC.
- A request carrying images gets at least 45 s (`llm::IMAGE_TIMEOUT`: the
  upload). Code that reads a message's text accepts parts
  (`turn::text_of`). A model evaluation's log has each image as its size
  (`turn::redact_images`: "data:image/png;base64,<183 KB>").

## Screenshots on disk (`snip/images.rs`)

- `<data>/chat-images/img_<16 hex>.png` (portable: `<exe>\Data\…`), written
  atomically; `chats.json` keeps `ChatImage {id, w, h, bytes, source}` on
  the question (`ChatMessage.images`, only when there are some) — never
  pixels. Ids are checked everywhere (`is_id`): no path can hide in one.
- **Kept**: with their conversation (`chats::delete_many` deletes its
  screenshots after the store write), and for "Keep chat screenshots"
  (Settings → Privacy & data, `chat_image_days`: 7 / 30 (default) / 90 /
  0 = forever; anything else counts as 30). The sweep (`images::sweep`, at
  launch and daily, and at once when the setting changes) deletes files past
  it, an image no conversation refers to (a snip never sent) after a day,
  and leftover temp files.
- **The copy a model gets** (`fitted`): never the stored PNG changed; long
  edge ≤ 2,576 px for Claude, 3,072 for Gemini, 2,048 for OpenAI, Groq and
  OpenRouter, 1,024 for a model on this PC (`long_edge`), area-averaged
  (`shrink`), re-encoded, smaller again while over 4 MB; made in the
  background as the snip lands (`prepare`) and kept in memory (the last 12):
  Send is instant.
- **Privacy**: never logged (sizes, counts and timings only; `log_privacy.rs`
  now also flags `png`, `rgba`, `bgra`, `pixels`, `data_url`), never synced,
  not in the local API or the MCP server; an image leaves the PC only inside
  the Chat request you send (and that conversation's follow-ups).

## When capture can't work

- **The secure desktop** (UAC prompts, the lock screen): no app can capture
  it, and Yap's shortcut doesn't fire there.
- **Protected content**: DRM video and windows that ask to be left out
  (`WDA_EXCLUDEFROMCAPTURE`: some password managers and banking apps) come out
  black or missing — exactly as the frozen picture shows them.
- **Windows' "Let desktop apps take screenshots" off**: WGC fails, GDI
  takes over (SDR only; logged).
- Nothing captured at all: "Couldn't take the screenshot" (an alert,
  `alert::snip_failed`), saying the two cases above. The Shortcuts row's
  hint says them too.

## Tests

- **Unit** (`cargo test`): `snip/hdr.rs` (every half; the round trip at five
  white levels; a light grey isn't white; out of gamut, NaN, highlights; a
  frame row by row), `snip/geom.rs` (drags any way, clamping, the 6 px
  minimum, physical pixels at 100–200 %, a monitor left of the primary,
  frames of another size, the crop, the label), `snip/images.rs` (ids, PNG
  round trip, per-provider sizes, area averaging, fitting, the sweep and
  retention, deleting with a conversation), `snip/turn.rs` (request shapes
  per provider, only the question gets the images, the task-local,
  redaction, which earlier images go again), `snip/vision.rs` (family rules,
  the lists first, refusals, the switch), `snip/overlay.rs` (the dim, the
  pill, the colours against app.css), `snip.rs` (the voice phrases and their
  negatives), `model_catalog` (the lists' vision flags), `input_hook`
  (Win+Alt+S, its clashes).
- **e2e** (`e2e/snip.spec.js`, 11 tests): fake monitors through
  `e2e_snip` (a 1280×720 HDR one at 240 nits through the real FP16 path, a
  960×540 one at 150 %), the hidden overlay driven by `e2e_snip_input`, how
  it went from `e2e_snip_last`: the snip in a new chat with its pixels exact,
  Enter on an empty box, the prompts, Esc / right-click / a click, the
  conversation Chat shows, the "can't read images" line and its switch, a
  server's refusal, saying "snip", the Settings rows, ✂ / + / the in-page
  shortcut, retention and deletion. `e2e/support/png.js` reads PNGs with
  Playwright's own pngjs; the fake AI records `images` and can refuse them
  (`rejectImages`).
- **Real-app robot** (`e2e-real/snip.real.js`): real monitors, a real drag
  (`yaprobot snipdrag`, only onto the robot's own overlay) over swatches on
  an Edge page on each monitor: every swatch within ±2, the overlay within
  150 ms; a real Esc cancels and the focus goes back.

## Not done yet

- **Phase 2**: a box across monitors; Enter/Space for the whole monitor and
  click-to-pick a window; a magnifier; HDR highlight roll-off; copy and save
  (right-click a screenshot, Ctrl/Shift on release); paste and drag-drop into
  Chat; "Send its text instead" (Windows OCR) for a text-only model; the
  picker's "Reads images" tag and filter; a JPEG fallback for photo-heavy
  snips.
- **Phase 3**: the on-device model reading images (Qwen3.5-4B's projector,
  `--mmproj`), vision detection for Ollama / LM Studio / llama-server.
- Snips in the Voice Assistant (asking about the screen out loud).
