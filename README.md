<div align="center">

<img src="src/assets/128x128@2x.png" width="96" alt="Yap icon" />

# Yap

**Talk instead of type — anywhere on Windows.**

Press a hotkey, speak, press again. Yap transcribes on your GPU — locally, privately,
free — cleans up the "um"s and grammar with AI, and types the result into whatever
app you're in.

[![CI](https://github.com/nayballs/Yap/actions/workflows/ci.yml/badge.svg)](https://github.com/nayballs/Yap/actions/workflows/ci.yml)
![Platform](https://img.shields.io/badge/platform-Windows-0078D4)
![GPU](https://img.shields.io/badge/GPU-any%20(Vulkan%20%2B%20DirectML)-8A2BE2)
![License](https://img.shields.io/badge/license-MIT-green)

**[⬇ Download for Windows](https://github.com/nayballs/Yap/releases/download/nightly/Yap-nightly-setup.exe)** · [all releases](https://github.com/nayballs/Yap/releases)

</div>

---

## Why Yap?

Voice dictation tools force a choice: the free, open-source ones transcribe locally but
dump out raw, unpunctuated text — while the polished ones charge ~$15/month and send
your voice to the cloud. Yap refuses the choice:

|  | Free/OSS tools | Paid apps (Wispr Flow, etc.) | **Yap** |
|---|:---:|:---:|:---:|
| Transcribes locally (voice never uploaded) | ✅ | ❌ cloud | ✅ |
| AI cleanup (filler, punctuation, grammar) | ❌ raw text | ✅ | ✅ — even **fully offline** |
| Price | Free | ~$15/mo | **Free, forever** |
| Open source | ✅ | ❌ | ✅ MIT |

```
🎤 your voice → Whisper / Parakeet (LOCAL, on your GPU) → raw transcript
             → AI cleanup (built-in local model, optional) → polished text
             → typed into the focused app
```

## Features

- 🎙️ **Dictate into anything** — one global hotkey (default `F9`), toggle or
  push-to-talk. The text lands in whatever window has focus: Slack, email, your IDE.
- ⚡ **Local GPU transcription** — 16 models to choose from (Parakeet, Whisper,
  Moonshine, SenseVoice, Canary…), downloaded in-app and SHA-256 verified. Runs on
  **any** GPU — NVIDIA, AMD, or Intel — via Vulkan + DirectML. No CUDA. CPU fallback included.
- 🧠 **AI cleanup, private by default** — one click downloads a small local model
  (Qwen2.5 1.5B via Mozilla llamafile) that strips filler words, fixes punctuation and
  grammar, and resolves "no wait, I meant…" self-corrections — **entirely on your PC**.
  Prefer your own stack? Point it at Ollama/LM Studio, or bring a Groq/OpenAI key.
  Even bring your own GGUF.
- 🎯 **Per-app cleanup profiles** — terse for Slack, formal for email, code-aware for
  your editor. Yap detects the app you're dictating into and applies the right style.
- 📊 **History & stats** — a local-only log of your dictations plus a dashboard:
  time saved vs typing, day streak, 30-day activity heatmap.
- 📖 **Correction dictionary** — teach it your jargon once ("Power to Keep" → "Parakeet"),
  never fix it again.
- 🌍 **Multilingual** — per-model language selection and translate-to-English.
- 🍰 **Polished, minimal UI** — a floating waveform overlay while you speak, an optional
  always-on-top pill, a state-aware tray icon, live streaming partials (opt-in).
- 📦 **Zero-friction install** — small NSIS installer (normal or portable), in-app
  auto-updates, autostart option.

## Quick start

1. **[Download the installer](https://github.com/nayballs/Yap/releases/download/nightly/Yap-nightly-setup.exe)** and run it.
   > Builds are not yet Authenticode-signed, so SmartScreen will warn on first run —
   > click *More info → Run anyway*. (Signing is on the [roadmap](./ROADMAP.md).)
2. Pick a model in onboarding — the default **Parakeet V3** is fast and accurate.
3. Press **F9**, talk, press **F9** again. That's it.
4. *(Optional)* Settings → **AI Cleanup** → enable → **Built-in local AI** for
   polished text with nothing ever leaving your machine.

## Privacy

- **Your voice never leaves your PC.** Transcription is 100% local, always.
- AI cleanup is **off by default**; the built-in option runs locally too. Cloud
  providers are strictly opt-in, bring-your-own-key.
- Dictation history is stored **only on your machine** and can be disabled or cleared
  in Settings. No telemetry, no accounts.

## Building from source

```bash
git clone https://github.com/nayballs/Yap && cd Yap
npm ci

# full GPU pipeline (needs the Vulkan SDK: https://vulkan.lunarg.com)
npm run tauri dev -- --features engines

# fast UI-only build (stubbed transcription, no SDK needed)
npm run tauri dev
```

On Windows, `scripts\dev.bat` wraps the above with hot reload and sensible defaults.
Release installers build with `--features engines`: one installer, GPU-accelerated on
any GPU, nothing extra to install for end users.

## Docs & roadmap

- [`ROADMAP.md`](./ROADMAP.md) — where Yap is going, and the competitive strategy.
- [`CLAUDE.md`](./CLAUDE.md) — architecture deep-dive (modules, pipeline, feature flags).

Yap is pre-1.0 and moving fast — the download above is the rolling nightly build,
auto-updated as fixes land. A curated stable channel is imminent.

## License

[MIT](./LICENSE) — free forever. The core local dictation will never be paywalled.
