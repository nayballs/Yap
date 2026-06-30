# Yap Roadmap — becoming the best local dictation pill

> Goal: make Yap the best open-source "press a hotkey, speak, and it types
> anywhere" dictation tool of its kind — local-first, private, polished, and
> genuinely faster than typing.

This roadmap is paired with [`CLAUDE.md`](./CLAUDE.md), which documents how Yap
actually runs today. Read that first for the architecture; this file is the
"where we're going and why."

---

## 1. The landscape (mid-2026)

Yap competes in the **local-Whisper, global-hotkey, type-into-any-app** category.
The market is split into two camps, and **neither one fully wins**:

### Free / open-source local tools (Yap's direct peers)
| Tool | Stack | Stars | Notes |
|------|-------|-------|-------|
| **Handy** (cjpais) | Rust + Tauri, whisper-rs, Parakeet | ~25k | The one to beat. Fully offline, cross-platform, MIT. **Outputs raw text — no AI cleanup.** |
| **Whispering** (Epicenter) | Tauri, whisper.cpp + BYO cloud key | ~4.6k | Privacy-first, LLM transform pipelines, but BYO-API-key setup friction. |
| **OpenWhispr** | Electron, Whisper/Parakeet | ~1k | Local or BYOK, hotkey-to-cursor. |
| **VoiceInk** (Beingpax) | Swift, whisper.cpp + Parakeet | ~5.3k | Feature-rich (Power Mode, context awareness) but **macOS only**. |
| **whisper-writer** (savbell) | Python, faster-whisper | ~1.1k | Most configurable, but CUDA/Python install pain. |
| **nerd-dictation / Numen / BlahST** | Linux-only (VOSK / whisper.cpp) | — | Define the Linux/accessibility end. |

### Paid / commercial leaders (the UX bar)
| Tool | Model | Price | What they do best |
|------|-------|-------|-------------------|
| **Wispr Flow** | Cloud ASR + LLM | $12–15/mo | Auto-cleans rambling into polished, formatted text. Category leader. |
| **superwhisper** | Local-first + optional cloud LLM | $8.49/mo or $249 lifetime | "Modes" — per-app model + post-processor + prompt. |
| **Aqua Voice** | Cloud "Avalon" model | ~$8–13/mo | Best editing UX: natural-language inline edits ("make this a list"). 97% accuracy on code/jargon. |

**Tools to benchmark against:** Wispr Flow (UX bar), Handy (OSS bar), superwhisper
(local power), VoiceInk (feature richness), Whispering (philosophy), Aqua (editing frontier).

---

## 2. The core insight — where the gap is

The category bifurcates cleanly:

- **Free local tools** win on privacy, price, and offline — but they dump out
  **raw, rambling, word-for-word text with a 2–5 second delay and weak punctuation.**
  Editing the output can cost more time than it saved.
- **Paid cloud tools** win for one reason: they run speech through an **AI cleanup
  layer** that strips filler ("um", "uh"), fixes grammar, handles mid-sentence
  self-corrections, and formats the result — instantly. **This is the #1 reason
  people pay.** The cost is privacy (cloud), subscription fatigue, and resource bloat
  (Wispr Flow ~800MB RAM).

> **The wide-open whitespace: a local, private, free tool that ALSO does the AI
> polish and feels instant.** Nobody has nailed this. Handy (the leader) explicitly
> lacks it. That is Yap's wedge — especially on **Windows**, where most polished
> tools are Mac-first or Mac-only.

### What users actually love (build toward these)
1. Speaking 3–4× faster than typing, with near-zero friction.
2. **Low *perceived* latency** — streaming words as you speak beats pasting one chunk.
3. **AI cleanup of rambling → polished text** (the killer paid feature).
4. Auto-punctuation/formatting *without* spoken commands.
5. Works in every app/field.
6. Accessibility — repeatedly called "life-changing" (RSI, tremor, dyslexia).

### What users complain about (avoid these)
- Latency + **first-word clipping** (recording starts before the mic/model is warm).
- Bad accuracy on **code, jargon, names, accents** (raw Whisper is weak here).
- Raw verbatim output needing heavy editing.
- Privacy/cloud distrust; subscription resentment (prefer free / one-time).
- Resource bloat, hotkey conflicts, Windows Defender flagging the installer.
- Onboarding cliffs (superwhisper "configures a server rather than opens an app").

---

## 3. Honest status check

**Transcription is real and GPU-accelerated** (`cuda`/`engines` builds; the default
no-feature build is still a fast stub for quick `cargo check`). Shipped and working:

- **Multi-engine STT** via `transcribe-rs` — Whisper on **CUDA**, plus ONNX models
  (Parakeet, Moonshine, SenseVoice, GigaAM, Canary, Cohere) on **DirectML**.
- **16-model registry** + manager (download → SHA-256 → extract; switch/delete;
  per-model language + translate). Default model **Parakeet V3**.
- **AI cleanup layer** (Phase 2) — OpenAI-compatible (Groq / OpenAI / local Ollama),
  off by default, raw-fallback on error; + a live Groq usage meter.
- Sidebar **settings**, **tray** (state icon + model submenu), bottom **overlay**
  with a Claude-Code-style scrolling waveform, pill show/hide, recording modes
  (toggle / push-to-talk), and polish toggles.
- **Installer + auto-update + portable mode + release CI** (unsigned for now).

What's left: the latency *feel* (VAD pre-roll + streaming), accuracy extras, cleanup
presets, signing, history, and reach — see the phases below (✅ = done).

---

## 4. The build plan (prioritized)

### Phase 0 — Make it actually transcribe (foundation) — ✅ DONE
- [x] Real inference via **`transcribe-rs`** (whisper.cpp + ONNX), not the stub.
- [x] GPU path documented + wired: **Whisper→CUDA**, **ONNX→DirectML**; CPU fallback
      via the `engines` feature. `CMAKE_CUDA_ARCHITECTURES=native` for the build.
- [x] End-to-end validated; default model **`parakeet-tdt-0.6b-v3`** (fast + accurate,
      GPU via DirectML); model download/extract/verify UX works.

### Phase 1 — Beat the latency complaints (felt immediately)
- [x] Engine kept **warm** between dictations (+ lazy reload after idle-unload).
- [x] Sub-second insertion after speech on a GPU (confirmed on a 5070 Ti).
- [ ] **VAD pre-roll** to kill first-word clipping — matters most for push-to-talk /
      auto-start; toggle mode already avoids clipping (you press, then speak).
      `transcribe-rs` ships `vad::SileroVad` / `EnergyVad` / `SmoothedVad`
      (with a `frame_buffer()` ring) — keep a rolling pre-roll buffer so the
      audio handed to STT always starts a few hundred ms *before* speech onset.
- [ ] **Streaming partial results** — show words live in the overlay as you talk.
      Note: only Moonshine exposes true token streaming in `transcribe-rs`
      (and we pulled it as broken). So do it FluidVoice-style: a timer
      (~400–600 ms) re-transcribes the growing buffer on the warm engine, and a
      **`smart_diff` de-flicker** keeps the stable longest-common word-prefix and
      only appends new words (replace wholesale only if <50% overlaps). Re-entrancy
      guard: skip the next tick if a transcription overruns the interval, so slow
      machines degrade instead of queueing. Final pass on stop stays authoritative.

### Phase 2 — The differentiator: AI cleanup layer — ✅ DONE (v1)
- [x] Optional post-processing pass (filler/grammar/punctuation/self-corrections),
      hardened so small models clean rather than answer.
- [x] **Local OR cloud** via one OpenAI-compatible client (Groq / OpenAI / OpenRouter /
      local Ollama·LM Studio). Off by default; raw-fallback on any error.
- [x] Live **Groq usage meter** (daily tokens/requests) in settings.
- [ ] **Split prompt = immutable base + editable body** (FluidVoice pattern). The
      anti-"don't answer the question" guardrails + output-only rules live in a
      hidden base the user can't delete; the editable body holds tone/format. Stops
      users from accidentally breaking refusal behaviour when they customise.
- [ ] **Richer cleanup rules** in the base: self-corrections ("buy milk no wait
      buy water" → "Buy water."), spoken→digit numbers, spoken punctuation/emoji,
      preserve meaning/language. (Several already partly in `llm.rs`.)
- [ ] **Cleanup presets** (Default / Email / Notes / Slack / Code tone modes) —
      each a named body + temperature; pick from a dropdown. Foundation for
      per-app auto-switching (Phase 4).
- [ ] Future: a small **fine-tuned/local** cleanup model (Wispr Flow's real moat).
      If/when local: keep the static prompt-prefix **KV cache** warm so each
      utterance only processes the new transcript tokens (FluidVoice's latency trick).

### Phase 3 — Accuracy on the hard cases
- [x] Custom **dictionary** (exact, case-insensitive) with a UI.
- [x] **Parakeet** shipped as the fast/accurate default; Whisper large-v3 + others for
      accents/multilingual; **language selection + translate** per model.
- [ ] **Fuzzy** custom-words (catch near-misses) + a code/jargon-aware path.

### Phase 4 — App-aware formatting + light command mode
- [ ] Per-app tone/format auto-switching (superwhisper "Modes" / FluidVoice
      per-app prompt profiles) — read the **foreground process** at record-start and
      pick the matching cleanup preset (e.g. terse for Slack, formal for Outlook,
      code-aware for an IDE). Resolution order: app-bound profile → default preset.
- [ ] A few **natural-language edits** ("make this a list", "more concise") —
      FluidVoice's Write/Edit mode: if text is selected, feed it as context and treat
      the utterance as an instruction to rewrite it in place.

### Phase 5 — Trust, polish & distribution
- [x] **Installer** (custom NSIS: normal/portable + WebView2 bootstrap), **auto-update**
      (`tauri-plugin-updater` → GitHub Releases), **portable mode**, **release CI**.
- [ ] **Authenticode sign the installer** (deferred by choice until Yap's worth it —
      until then Windows SmartScreen warns on first run). Updater artifacts already
      minisign-signed.
- [x] Crisp recording indicator (overlay + waveform), great defaults, hidden power
      features, first-run onboarding.
- [ ] Verify low idle CPU/RAM; reliable injection into every field.
- [~] **Harden text injection** (FluidVoice `TypingService` lessons):
      - [x] Capture the **target window (HWND) at record-start** (skipping Yap's own
            windows) and **re-focus** it before pasting — fixes "typed into the wrong
            window" when focus shifts mid-transcription.
      - [x] **Unicode `SendInput` fallback** when the clipboard is unavailable.
      - [x] Clipboard snapshot+restore (already existed).
      - [ ] **Verify-after-paste** via UI Automation `ValuePattern` (read the focused
            element's value before/after) — deferred; needs a COM/UIA integration.

### Phase 6 — Reach
- [ ] **Transcription history** (list + audio playback + retention) — a simple local
      table (timestamp, raw + cleaned text, model, focused app). Powers:
- [ ] **Stats / streak dashboard** (FluidVoice `StatsView`) — computed purely from
      history: words dictated, transcription count, **time-saved** (words/typing-WPM
      − words/speak-WPM), daily streak, 7/30-day chart. Cheap, strong retention hook.
- [ ] **Audio-history export** (opt-in): save each dictation's WAV + a JSONL manifest
      pairing `raw_transcript` ↔ `final_transcript` — a ready-made eval/fine-tune
      dataset for improving cleanup, with a GB budget + orphan GC.
- [ ] **Linux / Wayland** + macOS parity (the engine choices were made Windows-first:
      CUDA/DirectML; Vulkan/Metal/CoreML are available in `transcribe-rs` for later).

---

## 5. Positioning in one line

> **Yap = Wispr Flow's polish, Handy's privacy and price, with no latency —
> Windows-first.** Free, local, open-source, and it cleans up your speech so the
> output is ready to send.

---

## 6. Pricing posture

This audience resents subscriptions and rewards free / one-time. Keep the core tool
free and fully local. If monetized later, reserve paid for *optional* cloud AI polish
or a one-time "pro" binary — never gate the basic local dictation behind a sub.
