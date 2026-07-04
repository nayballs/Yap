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
| **Handy** (cjpais) | Rust + Tauri, whisper-rs, Parakeet | ~20k | The one to beat. Fully offline, cross-platform, MIT. **Maintainer has *publicly committed to never* adding AI cleanup** (tone/filler/rewrite/per-app are explicit "won't do"). Added Cohere Transcribe + a dictionary + a Raycast extension. **Leaves Yap's exact gap wide open on purpose.** |
| **Whispering** (Epicenter, YC) | Tauri, whisper.cpp + BYO cloud key | ~4.6k | Privacy-first, chainable LLM transforms, now custom OpenAI-compatible endpoints — but **BYO-API-key / BYO-server**, no bundled local model. |
| **OpenWhispr** | Electron, Whisper/Parakeet | ~3.7k | Local + AI cleanup, but the cloud/AI path is **BYOK** and free tier caps cloud at 2k words/wk. |
| **VoiceInk** (Beingpax) | Swift, whisper.cpp + Parakeet | ~5.3k | Feature-rich (Power Mode, context awareness) but **macOS only**. |
| **whisper-writer** (savbell) | Python, faster-whisper | ~1.1k | Most configurable, but CUDA/Python install pain. |
| **Quobi / Whisper Local (drajb)** | local Parakeet / Whisper + local cleanup | small | The *closest* pitch-matches (no API key, cleanup off by default via local Ollama) — but obscure/unpolished. |
| **nerd-dictation / Numen / BlahST** | Linux-only (VOSK / whisper.cpp) | — | Define the Linux/accessibility end. |

> **Landscape recheck (July 2026).** Since late 2025 several tools moved into the
> "free + local + cleanup" box, so the *literal* claim is no longer unique — **but no
> popular, polished, Windows-first tool ships zero-config local cleanup.** The matches
> are Mac-first (superwhisper, VoiceInk, Aqua), BYO-key/BYO-Ollama (Whispering,
> OpenWhispr), cloud-tiered (Wispr Flow — still no offline mode at any tier), or obscure
> (Quobi). **Yap's moat is now execution, not category:** bundled llamafile+Qwen (no
> Ollama, no API key), Windows-first, universal GPU (Vulkan/DirectML vs CUDA/Apple lock-in).
> Rising 2026 differentiators — per-app formatting, voice edit/rewrite, sub-second
> streaming — Yap has **already built** (Phase 4 + streaming partials); the gap is
> validation + marketing, not code. Also new: on-device cleanup LLMs going mainstream
> (Google **Eloquent** = offline Gemma + Gemini cleanup; Voicebox local Qwen3), which
> *validates* Yap's local-small-model bet.

### Paid / commercial leaders (the UX bar)
| Tool | Model | Price | What they do best |
|------|-------|-------|-------------------|
| **Wispr Flow** | Cloud ASR + LLM | $12–15/mo | Auto-cleans rambling into polished, formatted text. Category leader. |
| **superwhisper** | Local-first + optional cloud LLM | $8.49/mo or $249 lifetime | "Modes" — per-app model + post-processor + prompt. |
| **Aqua Voice** | Cloud "Avalon" model | ~$8–13/mo | Best editing UX: natural-language inline edits ("make this a list"). 97% accuracy on code/jargon. |

**Tools to benchmark against:** Wispr Flow (UX bar), Handy (OSS bar), superwhisper
(local power), VoiceInk (feature richness), Whispering (philosophy), Aqua (editing frontier).
For a feature-by-feature breakdown of **superwhisper** and **Wispr Flow** vs Yap — what
to match, what to skip, and effort estimates — see
[`docs/competitive-analysis.md`](./docs/competitive-analysis.md).

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

**Transcription is real and GPU-accelerated** (the `engines` build; the default
no-feature build is still a fast stub for quick `cargo check`). Shipped and working:

- **Multi-engine STT** via `transcribe-rs` — Whisper on **Vulkan** (any GPU), plus
  ONNX models (Parakeet, Moonshine, SenseVoice, GigaAM, Canary, Cohere) on **DirectML**.
- **14-model registry** + manager (download → SHA-256 → extract; switch/delete;
  per-model language + translate). Default model **Parakeet V3**.
- **AI cleanup layer** (Phase 2) — OpenAI-compatible (Groq / OpenAI / local Ollama),
  off by default, raw-fallback on error; + a live Groq usage meter.
- Sidebar **settings**, **tray** (state icon + model submenu), bottom **overlay**
  with a Claude-Code-style scrolling waveform, pill show/hide, recording modes
  (toggle / push-to-talk), and polish toggles.
- **Installer + auto-update + portable mode + release CI** (unsigned for now).

What's left: validating streaming partials on the Vulkan build (+ a true streaming
model for the partial pass), accuracy extras (fuzzy dictionary, verify-after-paste),
Authenticode signing, audio-history export, and reach (Linux/macOS) — see the phases
below (✅ = done).

---

## 4. The build plan (prioritized)

### Phase 0 — Make it actually transcribe (foundation) — ✅ DONE
- [x] Real inference via **`transcribe-rs`** (whisper.cpp + ONNX), not the stub.
- [x] GPU path documented + wired: **Whisper→Vulkan** (any GPU), **ONNX→DirectML**;
      CPU fallback via the `engines` feature. Vulkan SDK required at build time.
- [x] End-to-end validated; default model **`parakeet-tdt-0.6b-v3`** (fast + accurate,
      GPU via DirectML); model download/extract/verify UX works.

### Phase 1 — Beat the latency complaints (felt immediately)
- [x] Engine kept **warm** between dictations (+ lazy reload after idle-unload).
- [x] Sub-second insertion after speech on a GPU (confirmed on a 5070 Ti).
- [x] **Audio pre-roll** to kill first-word clipping — a rolling ~300 ms ring of
      mic audio is kept while idle and prepended to the buffer at record-start, so a
      word already in flight when you press the key isn't lost. (Simpler than a VAD
      onset detector and helps toggle *and* push-to-talk. `transcribe-rs` also ships
      `vad::SileroVad`/`EnergyVad` if we later want true VAD-triggered auto-start.)
- [x] **Streaming partial results** (opt-in, off by default) — `streaming_partials`
      spawns a worker that every ~500 ms re-transcribes the growing buffer on the
      warm engine and emits `yap-partial`, shown live in the overlay. A `smart_diff`
      de-flicker keeps the stable longest-common word-prefix and appends the new
      tail (replace wholesale if <50% overlaps). A `try_lock` re-entrancy guard
      skips a tick if a transcription is already running and never blocks the
      authoritative final pass. (Only Moonshine offers true token streaming in
      `transcribe-rs`, and we pulled it as broken, hence the re-transcribe approach.)
      ⚠ Needs validation on the GPU (Vulkan) build before enabling by default.
- [ ] **Add a true streaming model for the partial pass** (July-2026 research). The
      re-transcribe-the-growing-buffer approach runs Parakeet in exactly the mode where
      it degrades ~2× (batch ~6% → streaming ~12.8% WER), and cost grows O(n) with
      recording length. Purpose-built streaming models now run on Yap's *same* DirectML
      runtime and would give better, lower-latency partials: **Moonshine v2 streaming**
      (245 MB, 6.65% WER, ONNX/.ort — Yap already lists an older Moonshine) or **NVIDIA
      Nemotron on-device streaming** (0.67 GB int4, 0.56 s latency, ONNX via MS Foundry
      Local, only ~0.2% batch→stream loss). Pair with a sliding-window buffer for the
      partial pass to kill the O(n) re-transcribe cost. Keep Parakeet TDT 0.6B v3 as the
      authoritative final-pass default — nothing dethroned it for a lightweight local pill.

### Phase 2 — The differentiator: AI cleanup layer — ✅ DONE (v1)
- [x] Optional post-processing pass (filler/grammar/punctuation/self-corrections),
      hardened so small models clean rather than answer.
- [x] **Local OR cloud** via one OpenAI-compatible client (Groq / OpenAI / OpenRouter /
      local Ollama·LM Studio). Off by default; raw-fallback on any error.
- [x] Live **Groq usage meter** (daily tokens/requests) in settings.
- [x] **Split prompt = immutable base + editable body** (FluidVoice pattern). The
      anti-"don't answer the question" guardrails + output-only rules live in a
      hidden base the user can't delete; the editable body holds tone/format. Stops
      users from accidentally breaking refusal behaviour when they customise.
      (`llm::BASE_PROMPT` + `build_system_prompt()`; body = `config.pp_prompt`.)
- [x] **Richer cleanup rules** in the base: self-corrections ("buy milk no wait
      buy water" → "Buy water."), **spoken→digit numbers/dates/times**, **spoken
      punctuation/layout** ("period"/"comma"/"new line" → symbols, only when clearly
      dictated as commands), preserve meaning/language. In `llm::cleanup`'s always-
      applied instruction + a one-shot example (leaves "period" as a noun alone).
      (Spoken emoji not added — noisy; can revisit.)
- [x] **Cleanup presets** (Default / Email / Notes / Slack / Code tone modes) —
      each a named body; pick from a dropdown (`config.pp_preset` + editable
      `pp_prompt`). Foundation for per-app auto-switching (Phase 4).
      (Per-preset temperature still TODO — cleanup runs at a fixed 0.2.)
- [ ] Future: a small **fine-tuned/local** cleanup model (Wispr Flow's real moat).
      If/when local: keep the static prompt-prefix **KV cache** warm so each
      utterance only processes the new transcript tokens (FluidVoice's latency trick).

### Phase 3 — Accuracy on the hard cases
- [x] Custom **dictionary** (exact, case-insensitive) with a UI.
- [x] **Parakeet** shipped as the fast/accurate default; Whisper large-v3 + others for
      accents/multilingual; **language selection + translate** per model.
- [ ] **Better custom-word correction** (catch near-misses) — see
      [`docs/fuzzy-dictionary.md`](./docs/fuzzy-dictionary.md). Key finding: FluidVoice
      has **no** fuzzy string matching (its dictionary is exact-replace like ours; its
      "fuzziness" is acoustic ASR boosting in a non-portable Apple-Silicon lib). Plan:
      (1) word-boundary + multi-trigger polish on the exact path; (2) feed dictionary
      terms into the AI-cleanup prompt as bias context (beats FluidVoice's dictionary);
      (3) optional per-entry true fuzzy (Levenshtein/phonetic) with strict length/
      threshold guards; (4) ASR `initial_prompt`/hotword biasing if `transcribe-rs`
      exposes a hook.

### Phase 4 — App-aware formatting + light command mode
- [x] **Per-app tone/format auto-switching** ("smart routing" — superwhisper
      "Modes" / FluidVoice per-app prompt bindings) — read the **foreground process**
      at record-start (`text_injector::app_name_for`, keyed on the exe name since
      Windows has no macOS bundle id) and pick the matching cleanup body. Resolution
      order ports FluidVoice's `promptResolution`: **app-bound rule → scope guard →
      global default** (`config::YapConfig::resolve_cleanup_body`). Includes the
      `allApps` vs `selectedAppsOnly` **routing scope** and a Settings UI (per-app
      rules; app picker seeded from dictation history).
- [x] **Reusable named cleanup profiles** (FluidVoice `DictationPromptProfile`) — a
      library of named profiles (`config.cleanup_profiles`: `{id,name,prompt}`); each
      per-app rule **binds to a profile** (`AppRoute.profile_id`) instead of carrying
      an inline body, so one profile can serve many apps and is edited in one place.
      Legacy inline-body rules auto-migrate to a generated profile on first load.
      Profiles can be seeded from the built-in presets.
- [x] **Per-profile model choice** (match superwhisper "Modes") — each cleanup profile
      can pick its **own LLM** (provider/base URL/model/API key), not just a prompt, so
      an "Email" profile runs a stronger cloud model while "Slack" runs the fast local
      sidecar. Empty provider = inherit the global AI-cleanup settings. Implemented:
      optional `{provider, base_url, model, api_key}` on `CleanupProfile`;
      `resolve_cleanup` returns a `CleanupPlan` (body + endpoint override);
      `local_llm::effective_endpoint_for` routes "ondevice" through the sidecar for
      profile overrides too (and the sidecar now autostarts if *any* profile selects
      it, re-checked on config save); per-profile picker + endpoint fields in
      Settings → AI Cleanup → Profiles. Closes the clearest gap vs superwhisper's
      per-mode model picker — see
      [`docs/competitive-analysis.md`](./docs/competitive-analysis.md).
- [x] **Edit / Rewrite mode** ("make this a list", "more concise", "fix grammar") —
      FluidVoice's Write/Edit mode. **v1 = rewrite + write, implemented** (pending
      end-to-end runtime test). Shipped: `edit_hotkey` (2nd hotkey via `EDIT_BINDING`
      in `input_hook`), `selection.rs` (UIA `TextPattern` → Ctrl+C-clipboard fallback,
      `windows` crate 0.61), `llm::rewrite()` (+ shared `post_chat`), pipeline
      `edit_mode`/`run_rewrite` (paste result over selection), and the Settings
      recorder + row. Command/terminal mode still deferred (agentic zsh+osascript
      loop with no clean Windows analog). Mode is chosen by **which hotkey fired**,
      not by parsing speech; the selection is captured **at hotkey-press, before
      recording**, while the target window still has focus.

      Implementation detail (as built):
      - **Second global hotkey** `edit_hotkey` (`config.rs` field) → `input_hook.rs`
        registers a 2nd binding → new `Pipeline.on_edit_key()`. (Yap's hook handles one
        hotkey today — this is the main hook change.)
      - **Selection capture** — new `selection.rs`, called before recording against the
        already-captured foreground HWND. **UIA `TextPattern` first**
        (`IUIAutomation::GetFocusedElement` → `TextPattern` → `GetSelection().GetText()`),
        **Ctrl+C-clipboard trick as fallback** (snapshot → send Ctrl+C → read → restore,
        reusing existing clipboard snapshot/restore) for apps where UIA text is patchy
        (Electron/Chromium/terminals). Empty selection ⇒ **write mode**.
      - **Prompt** — new `llm::rewrite()`: system = edit base-prompt + `"Use the
        following selected context:\n{selection}"`; user = `"User's instruction:
        {transcript}\n\nApply it to the selected context. Output ONLY the rewritten
        text."` (write mode omits the context line). Reuses the existing HTTP client.
      - **Apply** — reuse `text_injector` paste-over-selection (Ctrl+V overwrites the
        still-live selection) after re-focusing the captured HWND.
      Ports cleanly (no macOS deps): everything above uses Yap's existing SendInput /
      clipboard / HWND-focus layer. MacOS pieces dropped: `AXSelectedText`, `NSApp.hide`
      focus dance, `osascript`.
- [ ] **Agentic voice-command mode (frontier)** — speech → *actions*, not just text.
      superwhisper/Wispr Flow market "agentic" but in practice it's mostly *dictating
      into* agent tools (Cursor, Claude Code) — which Yap already does by typing into any
      window. The genuinely-open space is voice → a *structured action* ("summarize this
      and send it to #eng", "rename these variables", "commit with message X"). Yap has a
      head start: **edit/rewrite mode already turns speech into a transformation** rather
      than literal dictation. A v1 could route a spoken command to a small **tool-calling**
      LLM turn over the captured selection/context, then apply the result via the existing
      inject layer. Substantial + different risk profile than the polish items — parked as
      a deliberate exploration, not a near-term commitment. See
      [`docs/competitive-analysis.md`](./docs/competitive-analysis.md).
- [x] **Installer** (custom NSIS: normal/portable + WebView2 bootstrap), **auto-update**
      (`tauri-plugin-updater` → GitHub Releases), **portable mode**, **release CI**.
- [~] **Authenticode sign the installer** via **SignPath Foundation** (free for OSS)
      — until then Windows SmartScreen warns on first run. Updater artifacts already
      minisign-signed. Full setup + CI plan in [`docs/SIGNING.md`](./docs/SIGNING.md).
      Status: guide + release-workflow notes landed; **blocked on the SignPath account
      application/approval (user action)**, then we wire + test the sign→updater steps
      on a tag. Key subtlety documented: Authenticode must be applied **before** the
      minisign updater signature is computed, or auto-update breaks. (Paid fallbacks:
      Certum OSS ~£10–30/yr inline `signCommand`; Azure Trusted Signing ~$10/mo.)
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
- [x] **Transcription history** (`history.rs` → `history.json`) — local-only table
      (timestamp, raw + final text, model, focused app), capped, gated by
      `history_enabled`, clearable from Settings → History. (Audio *playback* TBD.)
- [x] **Stats / streak dashboard** (FluidVoice `StatsView`) — computed from history:
      words today/all-time, **time-saved** (words/40 − words/150 WPM), day streak,
      30-day activity bars, dictation count. In Settings → History.
- [ ] **Audio-history export** (opt-in): save each dictation's WAV + a JSONL manifest
      pairing `raw_transcript` ↔ `final_transcript` — a ready-made eval/fine-tune
      dataset for improving cleanup, with a GB budget + orphan GC. (Deferred — the
      text history + stats landed first; audio capture/retention is the next step.)
- [ ] **Meeting recording / long-form transcription** (match superwhisper) — capture
      **system audio** (WASAPI **loopback**) mixed with the mic, record long sessions,
      transcribe in chunks on the warm engine, and save a transcript (+ optional speaker
      diarization, + file/import transcription). Reuses Yap's whole STT stack; the new
      pieces are loopback capture, long-audio chunking, and a recordings UI. Windows-first
      (loopback is per-platform). A distinct surface from push-to-type dictation.
- [ ] **Cross-platform desktop — Linux + macOS.** Tauri + Rust + `transcribe-rs` are
      already portable (Vulkan/ONNX on Linux, Metal/CoreML on macOS); the work is porting
      the **four Windows-specific layers** that make a dictation app work, each already
      `cfg`-gated: **global input hooks** (`input_hook`: Win32 LL hooks → evdev/`libei`
      on Linux, `CGEventTap` on macOS), **text injection** (`text_injector`: SendInput/
      clipboard → XTest/`ydotool`/Wayland-portal, macOS `CGEvent`/Accessibility),
      **selection capture** (`selection`: UIA → AT-SPI / macOS AX API), and **audio +
      mute** (cpal is cross-platform; WASAPI mute → PulseAudio/PipeWire, CoreAudio).
      **Wayland input injection is the hard part** (its security model deliberately
      restricts synthetic input — needs `libei`/portals). **Linux first** (same Rust, no
      Apple hardware needed, and it's the OSS audience that overlaps with Handy); macOS
      after (needs a Mac to build/test).
- [ ] **iOS — a separate product, not a port.** No global hotkey, and sandboxing forbids
      "type into any app"; dictation there goes through a **custom keyboard extension** or
      share sheet, in Swift. None of Yap's Win32/Tauri desktop layer applies. Tracked here
      only so it's a conscious *future separate project*, never mistaken for a Yap platform
      milestone.

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
