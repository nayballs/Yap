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

> **Mission — the best-of-everything blend.** Yap's strategy is to take the best of
> every top-tier dictation app — **superwhisper**, **OpenWhispr**, **Wispr Flow**,
> **Handy**, **Aqua**, **FluidVoice** — and combine them into one Windows-first,
> local-first app. We port proven patterns **from source**, not from screenshots:
> **OpenWhispr is cloned at `E:\Projects\references\openwhispr`** (Handy at
> `references/Handy`) — when working on a feature one of these apps does well, read
> its actual implementation first. See ROADMAP.md "North star" +
> `docs/openwhispr-teardown.md` + `docs/openwhispr-parity.md`.

> Active work note: the Settings / Language-Models / Prompt-Studio surface is being
> reshaped to track **OpenWhispr** (`E:\Projects\references\openwhispr`) as the design
> reference — porting its UX patterns and wording while keeping Yap's own backend
> contract (split immutable-guardrails + editable body). Treat that repo as the
> source of truth for this redesign pass.

---

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Windows: **pill**, **settings**,
  **onboarding**, **overlay**.
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime).
- **STT:** [`transcribe-rs`](https://crates.io/crates/transcribe-rs) — one crate that
  wraps **whisper.cpp** (Vulkan) *and* a family of **ONNX** models (Parakeet, Moonshine,
  SenseVoice, GigaAM, Canary, Cohere) via `ort`/ONNX Runtime (DirectML). Behind the
  `engines` feature flag (see below).
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
  dictionary → inject (into the captured `target_hwnd`). **Voice Agent wake word**
  (`agent_detect.rs`, OpenWhispr `detectAgentName` port): a dictation addressing the
  agent by name (`agent_name`, default "Yap"; fuzzy-matched) routes the whole
  transcript through the Voice-Agent scope in write mode (`run_agent`, shared with
  the edit hotkey) instead of cleanup — the agent prompt strips the name+command. A `processing` guard blocks
  starting a new recording while one is still transcribing (no overlapping `run_stt`
  / duplicate model load), and the buffer is capped at 15 min so a stuck key can't
  OOM. With `streaming_partials` on, `stream_partials` (a per-session worker)
  re-transcribes the growing buffer every ~500 ms and emits de-flickered
  (`smart_diff`) `yap-partial` text. An **idle watcher** unloads the model after
  `model_unload_timeout`; the next dictation lazily reloads it.
- **`stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **14-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  `apply_accelerator_settings` sets whisper→Vulkan(Auto) / ONNX→DirectML.
- **`llm.rs`** — the AI cleanup client (OpenAI-compatible). Frames the transcript as
  data (delimiters + one-shot) so small models *clean* it instead of *answering* it.
  The system prompt is split FluidVoice-style: an immutable `BASE_PROMPT` (guardrails:
  output-only, never answer the transcript) that's always prepended via
  `build_system_prompt()` to the user's editable **body** (tone/format = a preset or
  custom text). Records token/request usage (best-effort).
- **`local_llm.rs`** — the on-device AI cleanup sidecar: runs **Mozilla llamafile**
  (llama.cpp, single-file OpenAI-compatible server) as a hidden child process on a
  free localhost port, serving **Qwen2.5-1.5B-Instruct** (Q4_K_M GGUF) by default
  — or **any user GGUF** dropped into `<data>/llm/` and picked via `pp_local_model`
  (Settings shows a model picker + "Open models folder"; switching restarts the
  sidecar). Owns install (runtime + model download, SHA-256 verified, per-stage
  progress events), process lifecycle (spawn/health-wait/kill + orphan cleanup at
  startup), and `effective_endpoint()` which routes `llm.rs` to the sidecar when
  provider = "ondevice" (falls back to the configured endpoint if it's down).
- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase):
  `content` (raw markdown, never overwritten by AI), `enhanced_content` (the
  Enhanced tab), `enhanced_at_hash` (OpenWhispr's `len+first-50` staleness
  marker). `note_enhance` (commands.rs) = the ported **Actions engine**: the
  Note Formatting scope's endpoint + editable fragment (fallback → global
  cleanup endpoint), `llm::enhance_note` at temp 0.3 under the immutable
  `llm::NOTE_BASE_PROMPT` (OpenWhispr's BASE_SYSTEM_PROMPT verbatim;
  MEETING_NOTE_BASE_PROMPT staged for Phase 6).
- **`media.rs`** — audio-file decode front-end for Upload: pure-Rust **Symphonia**
  (mp3/wav/m4a/aac/flac/ogg-vorbis; no opus yet) → downmix mono → 16 kHz
  (`pipeline::resample_linear`), plus `chunk_ranges` (~60 s windows cut at the
  quietest sample of each window's last 5 s). Consumed by
  `pipeline::run_file_transcription` (progress events, cancel flag, `processing`
  guard, history record) via the `transcribe_file` command.
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
  Notes/Slack/Code/Custom), the editable `pp_prompt` body, `pp_api_keys` (per-provider
  key store — the UI swaps the active `pp_api_key` from it on provider switch), `cleanup_profiles` (each
  with an optional per-profile LLM override: provider/base_url/model/api_key — empty
  provider = inherit global) + `app_routes` smart routing, streaming_partials,
  history_enabled, update_checks_enabled). JSON
  load/save + `apply_dictionary` + `resolve_cleanup` (per-app plan: body + endpoint).
  `data_dir()` is portable-aware.
- **`tray.rs`** — state-aware tray icon (runtime-generated coloured dot) + right-click
  menu (model submenu w/ checkmark, Cancel while recording, Settings/Quit, Check for
  updates); left-click opens Settings.
- **`overlay.rs`** — shows/positions the bottom (or top) center "transcribing" overlay
  window on `yap-state`.
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks; specs `kb:VKEY`,
  `kb:ctrl+shift+VKEY` (modifier combo), `kb:165` (single right-side modifier, e.g.
  RightAlt — never suppressed, it's AltGr), `mods:ctrl+alt` (modifier-only chord) /
  `mouse:ID` — combo semantics ported from OpenWhispr's `windows-key-listener.c`
  (press = key down w/ required modifiers held, release = key up OR required
  modifier up; chords fire on completion; suppressed keys are excluded from the
  GetAsyncKeyState self-heal — the hook eats them before the key-state table
  updates). Emits press AND release (via an emit-forwarder thread — the hook
  callback never blocks — plus a 30 s re-hook self-heal). The capture UI is
  `ui/HotkeyInput.svelte` + shared `lib/hotkeys.js` (parse/format/match — also
  drives the in-window fallbacks). ⚠ **Known Windows
  gotcha:** when one of Yap's OWN WebView2 windows has focus, the LL hook never
  receives the hotkey (WebView2/Chromium front-runs the hook chain on focus) —
  so the Settings + onboarding pages catch the hotkey **in-page** (keydown
  fallback → `toggle_recording`). Any new Yap window with focusable UI needs the
  same fallback.
- **`text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`. Captures the
  dictation **target window** at record-start (`current_foreground`, skipping Yap's
  own windows) and **re-focuses** it before pasting (`focus_window`, via the
  `AttachThreadInput` workaround) so focus changes mid-transcription don't misfire.
  The clipboard snapshot/restore preserves **text or image** so a paste doesn't wipe
  a copied image; `selection_via_copy` (edit-mode fallback) polls for the Ctrl+C
  result instead of a fixed sleep. Falls back to direct Unicode typing
  (`type_unicode`) if the clipboard is unavailable. (UI-Automation content
  verification is a deferred follow-up.)
- **`sound.rs`** — start/stop chimes (volume + output-device aware).
- **`mute.rs`** — mute-while-recording: mutes the default render endpoint via
  WASAPI/COM (`IMMDeviceEnumerator` → `IAudioEndpointVolume`) while recording and
  restores it after — only unmuting what Yap itself muted.
- **`portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **`commands.rs`** — Tauri commands: recording (`toggle_recording`, `cancel_recording`),
  config (`get_config`/`save_config`), models (`installed_models`, `download_model`,
  `download_model_size`, `set_active_model`, `delete_model`, `model_language_info`),
  devices (`list_audio_devices`, `list_output_devices`, `set_input_device` — live
  stream swap, `set_mic_test` — idle level meter), windows (`open_settings`,
  `open_onboarding`, `close_onboarding`, `set_pill_visible`, `set_pill_scale`),
  `configure_hotkey`, `set_autostart`, `is_portable`, `test_post_process`,
  `get_groq_usage`, history (`get_history`, `clear_history`, `get_stats`).

### Frontend (`src/`)
- **`lib/ControlPanel.svelte`** — the **main window** (window label is still
  `settings`, historic): an OpenWhispr-style control panel — slim sidebar
  (**Home / Chat / Notes / Upload / Dictionary**) + **Settings as a modal
  overlay** (cogwheel). `Settings.svelte` renders `embedded` inside the modal
  and stays **always mounted** so its in-window hotkey fallback + auto-save run
  for the window's lifetime. **`HomeView.svelte`** = the dictation feed
  (day-grouped history, per-item copy/delete via `delete_history_entry`, stats
  strip, live refresh on `yap-transcript`). **`DictionaryView.svelte`** = the
  correction dictionary (promoted out of Settings → Advanced; syncs with
  Settings' cfg copy via `yap-dictionary-changed`/`-external` events).
  **`UploadView.svelte`** = local audio-**file** transcription (drop/browse →
  Symphonia decode → chunked transcription on the warm engine with progress +
  cancel — see `media.rs`); **`NotesView.svelte`** = the AI Notepad (list +
  markdown editor, Enhance via the Note Formatting scope, Raw ↔ Enhanced
  dual-view + staleness dot; safe renderer in `lib/markdown.js`); the Home
  feed has **Ctrl+K search**. Chat is a `ComingSoonView` panel awaiting Phase 7.
- **`lib/Pill.svelte`** — always-on-top pill. `yap-state` dot, scrolling amplitude
  waveform (`yap-amp`), cancel ✕ while recording, model-download button, gear.
- **`lib/Overlay.svelte`** — the click-through bottom/top overlay; same scrolling
  waveform + "Transcribing…".
- **`lib/Settings.svelte`** — the settings surface, now rendered **inside the
  ControlPanel's modal** (`embedded` prop; ✕ closes). Grouped sidebar (App / AI models / Data / System):
  **General** (hotkey, recording mode, mic, sound+volume, mute, pill size, show
  pill/overlay, overlay position), **Speech-to-Text** (`ModelManager` + GPU +
  language/translate), **Language Models** (OpenWhispr-style: enable toggle → mode
  selector Cloud Providers/Local/Self-Hosted → provider pill tabs (Groq/Anthropic/
  OpenAI/OpenRouter/Custom, brand icons) → API Key (masked + "Get your API key"
  link) → Select Model registry rows (`ppModels.js` + `ui/SelectList.svelte`);
  UI-only `ppMode`/`cloudProvider` state resolves to the unchanged `ppProvider`
  contract. Below it, **Prompt Studio** (`PromptStudio.svelte`, OpenWhispr port):
  View (full effective prompt via `get_base_prompt`) / Customize (preset +
  body + Save/Reset) / Test tabs. Plus usage meter + profiles w/ per-profile
  model override + per-app rules), **History** (stats
  dashboard + recent list + enable/clear), **Advanced** (output toggles, system,
  dictionary), **About** (version, updates).
- **`lib/ModelManager.svelte` / `ModelRow.svelte` / `models.js`** — the 14-model
  browser, OpenWhispr-style: vendor pill tabs (All/NVIDIA/OpenAI/Community via
  `ui/PillTabs.svelte`) + compact one-line rows (status dot, brand icon from
  `providerIcons.js` + `assets/providers/*.svg` (MIT, from OpenWhispr), name,
  size, Download/Active/delete). `ModelCard.svelte` (big cards) remains only in
  Onboarding.
- **`lib/Onboarding.svelte`** — first-run **guided setup** (5 steps): model picker →
  mic check (live level meter via `set_mic_test` idle-amp mode + live device switch)
  → one-click **local AI cleanup** install → tray pointer → "try it here" live
  dictation test with a change-shortcut recorder.
- **`lib/ui/`** — primitives: Toggle, Select, Slider, Group, Row, Button, Input, Textarea.

### Window config (`src-tauri/tauri.conf.json`)
- **pill**: 210×60, transparent, decorations off, always-on-top, skip-taskbar. Hidden
  by default (`show_pill = false`) — the overlay + tray are the default surface.
- **settings**: 980×700, titled "Yap" (it hosts the ControlPanel — see above),
  normal, hidden, hide-on-close.
- **onboarding**: 620×720, hidden, hide-on-close.
- **overlay**: 330×48, transparent, click-through, always-on-top, not focused, hidden
  until recording/processing.

---

## Build, run & feature flags (important)

Cargo features in `src-tauri/Cargo.toml`:

| Feature | Effect |
|---------|--------|
| *(default)* | **STUB** — no `transcribe-rs`. `cargo check` stays fast. The app runs but returns placeholder text. |
| `engines` | Real multi-engine STT: `transcribe-rs` with **whisper-vulkan** + ONNX + **ort-directml**. Whisper runs on the GPU via **Vulkan** (any GPU), ONNX via DirectML; CPU fallback with no GPU. **This is what release + nightly builds use.** |
| `whisper` | Back-compat alias for `engines`. |
| `custom-protocol` | Required for release/standalone builds — embeds the frontend. `tauri build` sets it automatically. |

GPU policy: **UNIVERSAL, no CUDA.** Whisper → **Vulkan** (NVIDIA/AMD/Intel; `vulkan-1.dll`
ships with the GPU driver), ONNX → **DirectML** (any DX12 GPU). Same approach as **Handy**
(`references/Handy`, whose Windows target is `["whisper-vulkan","ort-directml"]`). Building
`whisper-vulkan` needs the **Vulkan SDK** at build time (glslc + headers + loader) — install
from https://vulkan.lunarg.com locally; CI uses `humbletim/install-vulkan-sdk`. No nvcc /
CUDA arch list. One small installer, GPU on every GPU.

### Run in dev (what we use)
Use **`scripts/dev.bat`** ("yap.dev") — it runs `npm run tauri dev -- --features engines`
(the **real** GPU pipeline; needs the Vulkan SDK installed). A commented line switches to the
fast no-GPU stub for pure UI work. Dev hot-reloads the frontend on every edit (Vite on **:51437**).
A **"Yap - Dev"** desktop shortcut launches it; the plain **"Yap"** desktop shortcut is the
*installed* app (`D:\Hobby Project\Yap`, follows the nightly channel).

```bash
# real GPU pipeline (default in dev.bat) — requires the Vulkan SDK
npm run tauri dev -- --features engines
# stub (fast, no transcription)
npm run tauri dev
```

> ⚠️ A *compiled release build* bakes the frontend into the binary — editing `src/`
> and restarting that `.exe` changes nothing. For live frontend changes use dev
> (Vite on :51437). If :51437 isn't listening, you're looking at a release build.

### CI on every push
`.github/workflows/ci.yml` runs on every push/PR to `main`: `npm run build` (frontend,
also produces the `dist/` that `generate_context!` needs) + `cargo check --locked` on
the fast **stub** build (no `engines`, no Vulkan SDK) — a few minutes on a Windows
runner, so a broken commit can never reach a nightly. The real GPU pipeline is only
exercised by nightly/release builds.

### The dev → nightly → stable workflow
1. **Iterate in dev** (`scripts\dev.bat`): Vite hot-reloads `src/` edits instantly;
   Rust edits need a restart/recompile. Verify UI/behaviour changes HERE — never
   burn a 15-min nightly to check something dev shows in seconds.
2. **Push to main** — CI (above) sanity-checks every push.
3. **Nightly** — cut on demand (or let the 05:00 UTC cron) once a batch of changes
   is worth dogfooding on the real installed app; nightlies catch installer/updater/
   release-build issues, not CSS tweaks.
4. **Stable** — tag `v*` deliberately for curated milestones.

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

#### How to run / cut a nightly (it's all CI — no local build needed)
- **Trigger a nightly now:** `gh workflow run nightly.yml --repo nayballs/Yap`
  (otherwise it fires on the 05:00-UTC cron). Then find the run:
  `gh run list --workflow=nightly.yml --repo nayballs/Yap --limit 1`
- **Watch it to completion:** `gh run watch <run-id> --repo nayballs/Yap --exit-status`
  (build ≈ 15 min — installs the Vulkan SDK, compiles whisper.cpp + Vulkan).
- **Verify it published:**
  `curl -sL https://github.com/nayballs/Yap/releases/download/nightly/latest.json`
  → the `version` field should be the new `0.1.0-nightly.<N>`.
- **Get it on this machine:** in the app, **Settings → Check for updates** (an installed
  nightly auto-follows the nightly channel — no reinstall). First-time install:
  grab `Yap-nightly-setup.exe` from https://github.com/nayballs/Yap/releases/tag/nightly.
- **If a nightly build fails:** `gh run view <run-id> --repo nayballs/Yap --log-failed`.
- **Run from SOURCE instead (live dev, no release):** from the project folder run
  **`scripts\dev.bat`** (= `npm run tauri dev -- --features engines`). Hot-reloads the
  frontend on every edit. Needs the **Vulkan SDK** installed locally
  (https://vulkan.lunarg.com) so the `whisper-vulkan` backend compiles; the commented
  line in `dev.bat` switches to the fast no-GPU stub if you don't have it.
- **Requires** the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret (already set). If that key
  ever has a password, also add `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

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

**Transcription is REAL and GPU-accelerated** (no longer a stub in `engines`
builds). Working end-to-end: multi-engine STT (Whisper/Vulkan + ONNX/DirectML), the
14-model registry + manager, settings, tray, overlay, scrolling waveform, recording
modes, language/translate, **AI cleanup** (BYO key or local sidecar), per-app cleanup
routing + named profiles, **edit/rewrite mode**, the audio pre-roll (anti first-word
clipping), streaming partials (opt-in), transcription history + stats, cleanup presets,
real WASAPI mute, the Groq usage meter, and the installer + auto-updater + portable
mode + release CI. The default (no-feature) build still ships the stub for fast
`cargo check`.

Not yet done: validate + default-on streaming partials on the Vulkan build (and a true
streaming model for the partial pass — see [`ROADMAP.md`](./ROADMAP.md) Phase 1), fuzzy/
near-miss dictionary, verify-after-paste (UIA `ValuePattern`), Authenticode signing
(blocked on SignPath approval), audio-history export, and non-Windows polish. See
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
