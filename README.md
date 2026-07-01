# Yap

A tiny **local voice-dictation pill** for Windows. Press a global hotkey, speak, press
again — Yap transcribes **on your GPU**, optionally runs the text through an **AI
cleanup pass** (filler/punctuation/grammar), and types it into whatever window is
focused. Local-first, private, and free.

> **Why Yap?** The free/open dictation tools transcribe but dump out raw, unpunctuated
> text. The paid tools (Wispr Flow et al.) add an AI cleanup layer and charge ~$15/mo.
> Yap does **both** — local transcription *plus* instant AI cleanup — Windows-first.

---

## Features

- **Local multi-engine STT** via [`transcribe-rs`](https://crates.io/crates/transcribe-rs):
  Whisper on **Vulkan**, plus ONNX models (Parakeet, Moonshine, SenseVoice, GigaAM,
  Canary, Cohere) on **DirectML** — GPU-accelerated on any Windows GPU, no CUDA.
- **16-model library** with in-app download (SHA-256 verified), switch, and delete.
  Default model **Parakeet V3** (fast + accurate). Per-model language + translate.
- **Optional AI cleanup** — an OpenAI-compatible pass (Groq / OpenAI / OpenRouter, or
  a **local** Ollama/LM Studio model) that fixes filler, punctuation, grammar, and
  self-corrections. Off by default; falls back to the raw transcript on any error.
- **Live Groq usage meter** — daily token/request usage at a glance.
- **Polished UX** — a floating bottom overlay with a scrolling waveform, an
  (optional) always-on-top pill, a state-aware system tray with quick model switching,
  toggle **or** push-to-talk, a correction dictionary, and a sidebar settings page.
- **Installer + auto-update** — custom NSIS installer (normal/portable, WebView2
  bootstrap), in-app updates via GitHub Releases, and a release CI workflow.

## How it works

```
🎤 voice → Whisper / Parakeet (LOCAL, GPU)  → raw transcript
        → AI cleanup (Groq or local, optional) → polished text
        → pasted into the focused app
```

Transcription never leaves your machine. Only the optional cleanup uses an API — and
you can point it at a **local** model to keep everything offline.

## Requirements

- **Windows.**
- A GPU helps but **any** GPU works — Whisper runs on **Vulkan** and the ONNX models
  (Parakeet etc.) on **DirectML**, both universal (NVIDIA/AMD/Intel). No CUDA required.
  CPU works too, just slower for Whisper.
- For AI cleanup (optional): a free [Groq](https://console.groq.com) API key, **or**
  a local [Ollama](https://ollama.com)/LM Studio model.

## Build & run (dev)

Real GPU pipeline (what we develop with) — use **`scripts/dev.bat`**, or:

```bash
npm run tauri dev -- --features engines
```

(Building the Whisper Vulkan backend needs the [Vulkan SDK](https://vulkan.lunarg.com).)
Fast UI-only build with the STT stub (no real transcription):

```bash
npm run tauri dev
```

Feature flags: `default` = stub · `engines` = real STT (Whisper→Vulkan + ONNX→DirectML,
universal GPU). Release installers build with `--features engines` — one installer,
GPU-accelerated on any GPU, no CUDA needed on end-user machines.

## Status

Early but functional: transcription, the model library, AI cleanup, the UI, and the
installer/auto-updater all work. Installers are currently **unsigned**, so Windows
shows a SmartScreen warning on first run. See [`ROADMAP.md`](./ROADMAP.md) for what's
next (VAD/streaming, cleanup presets, history, signing).

## Docs

- [`CLAUDE.md`](./CLAUDE.md) — how Yap is built (architecture, modules, feature flags).
- [`ROADMAP.md`](./ROADMAP.md) — where it's going and the competitive strategy.

## License

MIT.
