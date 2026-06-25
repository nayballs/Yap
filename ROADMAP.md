# Blip Roadmap — becoming the best local dictation pill

> Goal: make Blip the best open-source "press a hotkey, speak, and it types
> anywhere" dictation tool of its kind — local-first, private, polished, and
> genuinely faster than typing.

This roadmap is paired with [`CLAUDE.md`](./CLAUDE.md), which documents how Blip
actually runs today. Read that first for the architecture; this file is the
"where we're going and why."

---

## 1. The landscape (mid-2026)

Blip competes in the **local-Whisper, global-hotkey, type-into-any-app** category.
The market is split into two camps, and **neither one fully wins**:

### Free / open-source local tools (Blip's direct peers)
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
> lacks it. That is Blip's wedge — especially on **Windows**, where most polished
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

**Blip's transcription is currently a stub.** The default Cargo build ships
`WhisperStt` in stub mode (no real inference) — see `src-tauri/src/stt.rs` and the
`whisper` feature flag in `Cargo.toml`. The whole audio pipeline, hotkey, text
injection, model download, settings, and pill UI are real and working; **only the
actual Whisper inference needs to be turned on and validated.** Everything below
assumes Phase 0 lands first.

---

## 4. The build plan (prioritized)

### Phase 0 — Make it actually transcribe (foundation)
- [ ] Build/run with the `whisper` feature and verify real `whisper-rs` inference.
- [ ] Document the GPU path (`cuda` feature) and CPU fallback; pick sane defaults.
- [ ] Validate end-to-end accuracy with `large-v3-turbo` as the default model
      (good speed/accuracy balance) and confirm model download UX works.

### Phase 1 — Beat the latency complaints (this is felt immediately)
- [ ] **Pre-warm** the mic stream + model so there is **no first-word clipping**
      (a top, concrete Handy complaint). Keep the engine warm between dictations
      (already done — engine is put back after each transcription).
- [ ] **Streaming partial results** — `SttEngine::transcribe_streaming` is already
      stubbed in the trait. Show text appearing live in the pill for perceived speed.
- [ ] Target sub-second insertion after speech ends.

### Phase 2 — The differentiator: local AI cleanup layer
- [ ] Optional post-processing pass that removes filler, fixes grammar, applies
      punctuation/formatting, and resolves self-corrections.
- [ ] **Local-first**: small local LLM (llama.cpp / Ollama) so privacy is preserved;
      **optional** BYO cloud key (Groq is cheap & fast) for users who want max quality.
- [ ] Off by default → "raw" mode for the privacy purists; one toggle for "polish".
- [ ] This is the single feature that converts "raw dictation" into "magic."

### Phase 3 — Accuracy on the hard cases
- [ ] Custom dictionary already exists (`config::apply_dictionary`) — expose a great
      UI and seed sensible defaults.
- [ ] Offer **Parakeet** as a fast/accurate alternative model (praised ~10× faster
      than Whisper on CPU); keep `large-v3` for accents/multilingual.
- [ ] A code/jargon-aware path for the developer audience (Whisper is weak here).

### Phase 4 — App-aware formatting + light command mode
- [ ] Detect the focused app and adjust tone/format (casual in Slack, formatted in
      docs, code in editors) — the superwhisper "Modes" idea, but with great defaults.
- [ ] A few **natural-language edits** ("make this a list", "more concise") without
      forcing a memorized command grammar (the Aqua approach users love).

### Phase 5 — Trust, polish & distribution
- [ ] **Sign the installer** so Windows Defender doesn't flag the global-input hook
      as a keylogger (a real distribution/trust problem for this category).
- [ ] Low idle CPU/RAM; no battery drain; reliable injection into every field.
- [ ] Crisp recording indicator (done — the glowing pill), per-app hotkeys, no conflicts.
- [ ] Ship great defaults; hide power features. Avoid the onboarding cliff.

### Phase 6 — Reach
- [ ] **Linux / Wayland** support — a real, underserved wedge for an OSS tool.
- [ ] Keep the cross-platform Tauri codebase honest (macOS parity).

---

## 5. Positioning in one line

> **Blip = Wispr Flow's polish, Handy's privacy and price, with no latency —
> Windows-first.** Free, local, open-source, and it cleans up your speech so the
> output is ready to send.

---

## 6. Pricing posture

This audience resents subscriptions and rewards free / one-time. Keep the core tool
free and fully local. If monetized later, reserve paid for *optional* cloud AI polish
or a one-time "pro" binary — never gate the basic local dictation behind a sub.
