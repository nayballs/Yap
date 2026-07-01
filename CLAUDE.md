# CLAUDE.md — how Yap works

Yap is a tiny **local voice-dictation pill**: press a global hotkey, speak, press
again — Yap transcribes **locally on the GPU**, optionally runs the text through an
**AI cleanup pass** (filler/punctuation/grammar), and types it into whatever window
is focused. A chime marks start/stop, a correction dictionary fixes mis-heard jargon,
and a floating overlay shows a live waveform while you talk.

This file documents how Yap actually runs today. For *where it's going* and the
competitive strategy, see [`ROADMAP.md`](./ROADMAP.md).

> Origin note: the core dictation plumbing (input hook, text injection) was ported
> from "Voice Mirror"; the multi-engine STT, AI cleanup, settings, tray, overlay,
> installer and the rest is Yap's own.

---

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Windows: **pill**, **settings**,
  **onboarding**, **overlay**.
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime).
- **STT:** [`transcribe-rs`](https://crates.io/crates/transcribe-rs) — one crate that
  wraps **whisper.cpp** (CUDA) *and* a family of **ONNX** models (Parakeet, Moonshine,
  SenseVoice, GigaAM, Canary, Cohere) via `ort`/ONNX Runtime (DirectML). Behind the
  `engines`/`cuda` feature flags (see below).
- **AI cleanup:** `reqwest` → any **OpenAI-compatible** chat endpoint (Groq/OpenAI/
  OpenRouter, or local Ollama/LM Studio). `src/llm.rs`.
- **Text injection:** `arboard` (clipboard) + Win32 `SendInput` (paste / Enter).
- **Model download:** `reqwest` streaming + `sha2` verify + `flate2`/`tar` extract.
- **Updates/install:** `tauri-plugin-updater` (GitHub Releases) + custom NSIS installer.
- **Autostart:** `tauri-plugin-autostart`.
- **Data dir:** `%APPDATA%/yap/` (`config.json`, `models/`, `groq_usage.json`,
  `history.json`).

---

## Architecture / runtime flow

```
global hotkey ─▶ input_hook ─▶ "dictation-key-pressed" / "-released"
                                        │  (lib.rs routes both → pipeline.on_key)
                                        ▼
                              Pipeline (toggle OR push-to-talk)
                    ┌───────────────────┴───────────────────┐
              start_recording                        stop_and_transcribe
                    │                                        │
        cpal mic stream → 16kHz mono f32      take audio ─▶ STT engine (blocking task)
        buffer (downmix + resample in              │  (transcribe-rs, warm engine)
        the audio callback); emits           AI cleanup pass (llm.rs, if enabled)
        "yap-amp" peak for the                    │
        scrolling waveform                   apply_dictionary()
                    │                              │
            "yap-state" drives the        text_injector (clipboard paste [+ Enter])
            overlay + tray                        │
                                            emits "yap-transcript"
```

Pipeline order after transcription: **AI cleanup → dictionary → append-space →
auto-submit (Enter) → inject**. Cleanup is best-effort — any error/timeout falls
back to the raw transcript, so dictation never blocks.

### Key modules (`src-tauri/src/`)
- **`lib.rs`** — app entry / Tauri `setup`. Runs `portable::init()`, registers the
  updater/process/autostart/single-instance plugins, starts the input hook + pipeline,
  routes `dictation-key-pressed`/`-released` → `Pipeline.on_key()` (so recording works
  before the webview is ready), drives the **overlay** and **tray** off `yap-state`,
  builds the tray (when `show_tray_icon` *or* the pill is hidden), and reconciles
  autostart. Clears ort's 0-byte `DirectML.dll` stub (`stt::fix_directml_stub`).
- **`pipeline.rs`** — the heart. Owns the mic stream + shared state (`recording`,
  audio `buffer`, idle `preroll` ring, warm STT `engine`, live `config`,
  `last_activity`, `target_hwnd`). Audio callback buffers while recording,
  downmixes→mono, resamples→16 kHz, and emits a throttled **peak amplitude**
  (`yap-amp`) for the scrolling waveform; while idle it keeps a rolling ~300 ms
  **pre-roll** ring that `start_recording` prepends (anti first-word-clipping).
  `recording_mode` selects toggle vs push-to-talk. `run_stt` (async) does cleanup →
  dictionary → inject (into the captured `target_hwnd`). With `streaming_partials`
  on, `stream_partials` (a per-session worker) re-transcribes the growing buffer
  every ~500 ms and emits de-flickered (`smart_diff`) `yap-partial` text. An **idle
  watcher** unloads the model after `model_unload_timeout`; the next dictation lazily
  reloads it.
- **`stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **16-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  `apply_accelerator_settings` sets whisper→CUDA(Auto) / ONNX→DirectML.
- **`llm.rs`** — the AI cleanup client (OpenAI-compatible). Frames the transcript as
  data (delimiters + one-shot) so small models *clean* it instead of *answering* it.
  The system prompt is split FluidVoice-style: an immutable `BASE_PROMPT` (guardrails:
  output-only, never answer the transcript) that's always prepended via
  `build_system_prompt()` to the user's editable **body** (tone/format = a preset or
  custom text). Records token/request usage (best-effort).
- **`history.rs`** — local-only transcription history (`history.json`): each
  dictation's timestamp, raw + final text, model, and focused app. Best-effort,
  gated by `history_enabled`. Derives the stats dashboard (words, time-saved vs
  typing, day streak, 30-day activity) without a date crate (UTC day-numbers like
  `usage.rs`). Powers `get_history`/`clear_history`/`get_stats`.
- **`usage.rs`** — daily Groq usage tracker (tokens summed locally + requests from
  `x-ratelimit-*` headers), persisted to `groq_usage.json`, auto-resets at midnight
  UTC; powers the `get_groq_usage` command + `groq-usage` event.
- **`config.rs`** — `YapConfig` (hotkey, model_size, use_gpu, input_device, sound +
  volume, output_device, mute_while_recording, recording_mode, pill_scale, show_pill,
  show_overlay, overlay_position, dictionary, append_trailing_space, auto_submit(+key),
  restore_clipboard, show_tray_icon, autostart, model_unload_timeout, selected_language,
  translate_to_english, the `pp*` AI-cleanup fields incl. `pp_preset` (Default/Email/
  Notes/Slack/Code/Custom) + the editable `pp_prompt` body, streaming_partials,
  history_enabled, update_checks_enabled). JSON
  load/save + `apply_dictionary`. `data_dir()` is portable-aware.
- **`tray.rs`** — state-aware tray icon (runtime-generated coloured dot) + right-click
  menu (model submenu w/ checkmark, Cancel while recording, Settings/Quit, Check for
  updates); left-click opens Settings.
- **`overlay.rs`** — shows/positions the bottom (or top) center "transcribing" overlay
  window on `yap-state`.
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks; spec `kb:VKEY` /
  `mouse:ID`; emits press AND release.
- **`text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`. Captures the
  dictation **target window** at record-start (`current_foreground`, skipping Yap's
  own windows) and **re-focuses** it before pasting (`focus_window`, via the
  `AttachThreadInput` workaround) so focus changes mid-transcription don't misfire.
  Falls back to direct Unicode typing (`type_unicode`) if the clipboard is
  unavailable. (UI-Automation content verification is a deferred follow-up.)
- **`sound.rs`** — start/stop chimes (volume + output-device aware).
- **`mute.rs`** — mute-while-recording (currently a logged stub; WASAPI TODO).
- **`portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **`commands.rs`** — Tauri commands: recording (`toggle_recording`, `cancel_recording`),
  config (`get_config`/`save_config`), models (`installed_models`, `download_model`,
  `download_model_size`, `set_active_model`, `delete_model`, `model_language_info`),
  devices (`list_audio_devices`, `list_output_devices`), windows (`open_settings`,
  `open_onboarding`, `close_onboarding`, `set_pill_visible`, `set_pill_scale`),
  `configure_hotkey`, `set_autostart`, `is_portable`, `test_post_process`,
  `get_groq_usage`, history (`get_history`, `clear_history`, `get_stats`).

### Frontend (`src/`)
- **`lib/Pill.svelte`** — always-on-top pill. `yap-state` dot, scrolling amplitude
  waveform (`yap-amp`), cancel ✕ while recording, model-download button, gear.
- **`lib/Overlay.svelte`** — the click-through bottom/top overlay; same scrolling
  waveform + "Transcribing…".
- **`lib/Settings.svelte`** — sidebar sections: **General** (hotkey, recording mode,
  mic, sound+volume, mute, pill size, show pill/overlay, overlay position), **Models**
  (`ModelManager` + GPU + language/translate), **AI Cleanup** (provider/key/model/
  preset + editable instructions + Test + usage meter), **History** (stats dashboard
  + recent list + enable/clear), **Advanced** (output toggles, system, dictionary),
  **About** (version, updates).
- **`lib/ModelManager.svelte` / `ModelCard.svelte` / `models.js`** — the 16-model
  browser (download/switch/delete/progress, "Your models" vs "Available").
- **`lib/Onboarding.svelte`** — first-run model picker.
- **`lib/ui/`** — primitives: Toggle, Select, Slider, Group, Row, Button, Input, Textarea.

### Window config (`src-tauri/tauri.conf.json`)
- **pill**: 210×60, transparent, decorations off, always-on-top, skip-taskbar. Hidden
  by default (`show_pill = false`) — the overlay + tray are the default surface.
- **settings**: 720×640, normal, hidden, hide-on-close.
- **onboarding**: 620×720, hidden, hide-on-close.
- **overlay**: 330×48, transparent, click-through, always-on-top, not focused, hidden
  until recording/processing.

---

## Build, run & feature flags (important)

Cargo features in `src-tauri/Cargo.toml`:

| Feature | Effect |
|---------|--------|
| *(default)* | **STUB** — no `transcribe-rs`. `cargo check` stays fast. The app runs but returns placeholder text. |
| `engines` | Real multi-engine STT: `transcribe-rs` with whisper-cpp + ONNX + **ort-directml**. Whisper runs on CPU. Broadly compatible (no CUDA). **This is what release builds use.** |
| `cuda` | `engines` + `whisper-cuda` → Whisper on the GPU (NVIDIA). **This is what we dev with.** |
| `whisper` | Back-compat alias for `engines`. |
| `custom-protocol` | Required for release/standalone builds — embeds the frontend. `tauri build` sets it automatically. |

GPU policy: **Whisper → CUDA** (the `cuda` feature), **ONNX → DirectML** (works on any
GPU). On CUDA machines set `CMAKE_CUDA_ARCHITECTURES=native` so nvcc targets the local
GPU (Blackwell/5070 Ti = sm_120).

### Run in dev (what we use)
Use **`scripts/dev.bat`** ("yap.dev") — it sets `CMAKE_CUDA_ARCHITECTURES=native` and
runs `npm run tauri dev -- --features cuda` (the **real** GPU pipeline). A commented
line in the script switches to the fast no-GPU stub for pure UI work. Dev hot-reloads
the frontend on every edit (Vite on **:1430**).

```bash
# real GPU pipeline (default in dev.bat)
CMAKE_CUDA_ARCHITECTURES=native npm run tauri dev -- --features cuda
# stub (fast, no transcription)
npm run tauri dev
```

> ⚠️ A *compiled release build* bakes the frontend into the binary — editing `src/`
> and restarting that `.exe` changes nothing. For live frontend changes use dev
> (Vite on :1430). If :1430 isn't listening, you're looking at a release build.

### Release / installer
Tagging `v*` (or running the **release** GitHub Action) builds via `tauri-action`
with `--features engines`, producing a custom **NSIS installer** (normal/portable,
WebView2 bootstrap) + a signed `latest.json` on a draft GitHub Release. The in-app
updater (`tauri-plugin-updater`) checks that endpoint. **Currently unsigned**
(Authenticode) — Windows shows a SmartScreen warning until a cert is added; the
`signCommand` slot is ready. Updater artifacts are minisign-signed
(`TAURI_SIGNING_PRIVATE_KEY` GitHub secret).

### Release channels (stable + nightly)
Yap ships **two auto-update channels** (Chrome Stable/Canary style), both CI-built
on GitHub Actions (Yap builds cleanly there — ONNX + DirectML, no CUDA):

- **Stable** — tag `v*` → `.github/workflows/release.yml` → a normal (non-prerelease)
  GitHub Release. Installed stable copies check
  `…/releases/latest/download/latest.json` (the `endpoints` in `tauri.conf.json`).
  Cut deliberately for curated versions (`0.1.0`, `0.2.0`, …).
- **Nightly** — `.github/workflows/nightly.yml` (daily `schedule` cron at 05:00 UTC,
  plus manual `workflow_dispatch`) → a **single rolling `nightly` pre-release** whose
  assets are overwritten in place (`gh release upload --clobber`). Version is
  `<baseVersion>-nightly.<run_number>` (e.g. `0.1.0-nightly.42`) — a semver prerelease,
  monotonic via the run number so the updater always sees "newer". The installer + sig
  are renamed to the **constant** names `Yap-nightly-setup.exe(.sig)` so the download
  URL never changes across nightlies.

**Channel separation:** a nightly install follows the nightly endpoint because it is
built with `-c src-tauri/tauri.nightly.conf.json`, which overrides only the updater
`endpoints` to `…/releases/download/nightly/latest.json` (same identifier/productName
as stable — it's the same app on a different endpoint). Because a GitHub *pre-release*
never resolves as `/releases/latest/`, stable users never see nightly builds, and the
two channels don't cross. Both channels sign with the **same** minisign key
(`TAURI_SIGNING_PRIVATE_KEY`) — the pubkey in `tauri.conf.json` must match it or
installed copies reject updates. See `docs/SIGNING.md` for Authenticode plans.

---

## Config & data

- Config: `%APPDATA%/yap/config.json` (auto-created; old files load — every field is
  `#[serde(default)]`). Portable mode → `<exe>/Data/`.
- Models: `%APPDATA%/yap/models/` — Whisper `.bin` files and extracted ONNX dirs,
  downloaded from `https://blob.handy.computer/` (SHA-256 verified).
- Groq usage: `%APPDATA%/yap/groq_usage.json`.
- History: `%APPDATA%/yap/history.json` (local-only; cleared from Settings → History).
- Notable defaults: hotkey `kb:120` (F9, rebindable), **default model
  `parakeet-tdt-0.6b-v3`** (fast/accurate, ONNX→DirectML), `use_gpu = true`,
  recording mode `toggle`, **pill hidden**, overlay shown, AI cleanup **off**.

---

## Current status

**Transcription is REAL and GPU-accelerated** (no longer a stub in `cuda`/`engines`
builds). Working end-to-end: multi-engine STT (Whisper/CUDA + ONNX/DirectML), the
16-model registry + manager, settings, tray, overlay, scrolling waveform, recording
modes, language/translate, **AI cleanup** (BYO key or local), the Groq usage meter,
and the installer + auto-updater + portable mode + release CI. The default (no-feature)
build still ships the stub for fast `cargo check`.

Not yet done: VAD pre-roll + streaming partials, transcription history, cleanup
presets, Authenticode signing, real WASAPI mute, non-Windows polish. See
[`ROADMAP.md`](./ROADMAP.md).

---

## Competitive context (why the roadmap looks the way it does)

Yap is in the **local-STT, hotkey, type-anywhere** category. Handy (~25k★, same
Rust+Tauri stack) is the OSS leader but **outputs raw, unpolished text** — it has no
AI cleanup. Paid tools (Wispr Flow, superwhisper, Aqua) win on exactly that cleanup
layer; Wispr Flow's own stack is **Whisper + a fine-tuned Llama** — the same two
stages Yap now runs, except Yap keeps transcription **local/free** and uses a cheap/
fast cleanup model (Groq `llama-3.1-8b-instant`) or a fully-local one.

**Yap's wedge (now real):** local + private + free transcription **plus** instant AI
cleanup, Windows-first. Monetisation stays fair — core free/local forever; any future
paid tier is *convenience* (a hosted cleanup option) or a one-time Pro, never the basic
dictation.

> Keep this file updated as features land — it should always reflect what's actually
> in the code.
