# CLAUDE.md — how Blip works

Blip is a tiny **local voice-dictation pill**: press a global hotkey, speak, press
again — Blip transcribes locally with Whisper and types the text into whatever
window is focused. A chime marks recording start/stop, and a correction dictionary
fixes mis-heard jargon.

This file documents how Blip actually runs. For *where it's going* and the
competitive strategy, see [`ROADMAP.md`](./ROADMAP.md).

> Origin note: the dictation engine (input hook, STT, text injection) was ported
> from "Voice Mirror"; everything else here is the slim glue around it.

---

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Two windows: the **pill** and **settings**.
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime).
- **STT:** `whisper-rs` (FFI to whisper.cpp), **behind a feature flag** (see below).
- **Text injection:** `arboard` (clipboard paste).
- **Model download:** `reqwest` streaming from HuggingFace.
- **Data dir:** `%APPDATA%/blip/` (`config.json` + `models/`).

---

## Architecture / runtime flow

```
global hotkey ─▶ input_hook ─▶ "dictation-key-pressed" event
                                        │
                                        ▼
                              Pipeline.toggle()
                    ┌───────────────────┴───────────────────┐
              start_recording                        stop_and_transcribe
                    │                                        │
        cpal mic stream fills          take audio buffer ─▶ STT engine (blocking task)
        a 16kHz mono f32 buffer                              │
        (resampled + downmixed                       apply_dictionary()
         in the audio callback)                              │
                    │                                  text_injector (clipboard paste)
            emits "blip-level"                                │
            for the pill waveform                     emits "blip-transcript"
```

### Key modules (`src-tauri/src/`)
- **`lib.rs`** — app entry / Tauri `setup`. Starts the input hook, starts the
  pipeline, wires the tray (Settings/Quit), routes `dictation-key-pressed` →
  `Pipeline.toggle()` **in Rust** (so dictation works even before the pill webview
  is ready), and makes the settings window hide-on-close instead of destroy.
- **`pipeline.rs`** — the heart. Owns the mic stream + shared state
  (`recording` flag, audio `buffer`, the STT `engine`, live `config`). The audio
  callback only buffers while `recording` is true, **downmixes to mono**, **resamples
  to 16 kHz** (`resample_linear`), and emits a throttled RMS level for the waveform.
  `stop_and_transcribe` runs STT on a blocking task, applies the dictionary, and
  injects the text. The engine is **kept warm** (put back after each transcription)
  to avoid a ~200MB reallocation per use.
- **`stt.rs`** — `SttEngine` trait + implementations: real **whisper-rs** (behind
  the `whisper` feature), a **stub** fallback, and a cloud placeholder. Owns the
  model descriptor registry (maps `model_size` → GGML filename + HF repo) and the
  streaming model download. The trait already declares `transcribe_streaming` for
  future partial results.
- **`config.rs`** — `BlipConfig` (hotkey, model_size, use_gpu, input_device,
  sound_enabled, pill_scale, dictionary), JSON load/save, and
  `apply_dictionary` (case-insensitive literal replacement; `$` is safe — uses a
  replacement closure, not regex backrefs).
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks
  (`WH_KEYBOARD_LL` / `WH_MOUSE_LL`); hotkey spec format `kb:VKEY` or `mouse:ID`.
- **`text_injector.rs`** — pastes the transcript into the focused window via clipboard.
- **`commands.rs`** — Tauri commands the frontend calls: `toggle_recording`,
  `get_config`, `save_config`, `download_model`, `open_settings`,
  `list_audio_devices`, `configure_hotkey`, `set_pill_scale`. Also `apply_pill_scale`
  (resizes the pill window and emits `blip-scale` to the frontend).
- **`sound.rs`** — start/stop chimes.

### Frontend (`src/`)
- **`lib/Pill.svelte`** — the always-on-top pill overlay. Listens for `blip-state`
  (`idle`/`recording`/`processing`/`needs-model`), `blip-level` (waveform), and
  `blip-scale`. Has the status dot, "Blip" label / live waveform, a model-download
  button (when `needs-model`), and the settings gear.
- **`lib/Settings.svelte`** — hotkey rebind, dictionary editor, model/GPU/mic pickers,
  pill-size slider.
- CSS uses a `--s` scale variable so the whole pill scales with `pill_scale`.

### Window config (`src-tauri/tauri.conf.json`)
- **pill**: 210×60, decorations off, transparent, always-on-top, skip-taskbar.
- **settings**: 470×640, normal window, starts hidden, hide-on-close.

---

## Build, run & the feature flags (important)

Cargo features in `src-tauri/Cargo.toml`:

| Feature | Effect |
|---------|--------|
| *(default)* | **No whisper** → ships the STUB. `cargo check` stays fast. **The app will NOT really transcribe.** |
| `whisper` | Enables `whisper-rs` — real local Whisper inference (CPU). |
| `cuda` | `whisper` + CUDA GPU acceleration. |
| `custom-protocol` | **Required for release/standalone builds** — loads the EMBEDDED frontend instead of the dev-server URL. `tauri build` sets this automatically; a bare `cargo build` does not. |

### Run in dev (hot-reload, what we use while iterating)
```bash
npm run tauri dev
```
This runs `beforeDevCommand` (`npm run dev` → Vite on **:1430**), launches the pill,
and **hot-reloads the frontend on every edit**. Note: dev uses **default features =
the STT stub**. To exercise real transcription in dev, run with `--features whisper`.

### Build a release
```bash
npm run tauri build   # runs `vite build` → ../dist, embeds frontend, custom-protocol on
```

> ⚠️ **Gotcha that cost us time:** a *compiled release build* has the frontend
> **baked into the binary**. Editing `src/` and restarting that release `.exe`
> changes **nothing** — it never reads the source. To see live frontend changes you
> must run `npm run tauri dev` (Vite dev server on :1430). If port 1430 isn't
> listening, you're looking at a release build, not dev.

---

## Config & data

- Config: `%APPDATA%/blip/config.json` (auto-created from defaults).
- Models: `%APPDATA%/blip/models/ggml-*.bin`, downloaded from
  `ggerganov/whisper.cpp` on HuggingFace.
- Defaults: hotkey `kb:120` (F9 — note: a running instance logged `kb:56`/Alt, i.e.
  it's user-rebindable), model `large-v3`, `use_gpu = true`, sound on, scale 1.0.

---

## Current status (read before building features)

**Transcription is a STUB in the default build.** The pipeline, hotkey, audio
capture/resampling, text injection, model download, settings, tray, and pill UI are
all real and working. The actual Whisper inference is gated behind the `whisper`
feature and needs to be turned on and validated — this is **Phase 0** in
[`ROADMAP.md`](./ROADMAP.md).

---

## Competitive context (why the roadmap looks the way it does)

Blip is in the **local-Whisper, hotkey, type-anywhere** category. The market splits:

- **Free/local tools** (Handy ~25k★ — the leader, same Rust+Tauri stack;
  Whispering; OpenWhispr; VoiceInk — Mac only) win on privacy/price/offline but
  **output raw, rambling, unpunctuated text with 2–5s latency.**
- **Paid cloud tools** (Wispr Flow, superwhisper, Aqua Voice) win because they run
  speech through an **AI cleanup layer** (strip filler, fix grammar, format,
  resolve self-corrections) and feel instant. That cleanup is the #1 reason people pay.

**Blip's wedge:** a local, private, free tool that *also* does the AI polish and
feels instant — **Windows-first**, where most polished tools are Mac-only.

Top things to get right (from real user feedback):
1. Kill **first-word clipping** + latency (pre-warm mic/model; stream partials).
2. Add an **optional local AI cleanup** pass (the magic feature).
3. Accuracy on **code/jargon/accents** (custom dictionary already exists; add Parakeet).
4. **Sign the installer** (global-input hook gets flagged as a keylogger otherwise).
5. Free / one-time pricing; great defaults; no onboarding cliff.

Full detail and the prioritized phases are in [`ROADMAP.md`](./ROADMAP.md).
