# Competitive analysis — superwhisper & Wispr Flow vs Yap

> Purpose: a feature-by-feature map of the two paid leaders Yap is measured against,
> with **what to match, what to skip, and rough effort** for each gap. Paired with
> [`ROADMAP.md`](../ROADMAP.md) (the prioritized plan) and [`CLAUDE.md`](../CLAUDE.md)
> (how Yap works today). Sourced from superwhisper's public docs
> (`github.com/superultrainc/superwhisper-docs`) and Wispr Flow's site/help-center +
> 2026 hands-on reviews. Last refreshed: **July 2026**.

Legend for Yap status: ✅ have · ◐ partial · ✗ gap · 🏆 Yap is *ahead*.

---

## TL;DR — the strategic picture

- **Both competitors are structurally the opposite of Yap on the axis that matters most.**
  Wispr Flow is **cloud-only at every tier** (no offline mode, ever — audio always
  uploads). superwhisper is local-capable but **its Windows build is materially behind
  its Mac build** (no local LLM cleanup, no coding-agent integration, no keystroke
  simulation, no config sync). Yap is **local + free + private + Windows-first** — a clean
  inversion of both.
- **Yap already has the "hard" differentiators.** Per-app modes, voice edit/rewrite,
  bundled zero-config local cleanup, live streaming partials, history + stats — all
  shipped. The outside-in view (and even these competitors' marketing) assumes OSS tools
  lack these.
- **"Agentic" is mostly marketing.** Neither tool autonomously executes tasks. It's (a)
  context-aware *formatting* and (b) using voice to *dictate into* agent tools (Cursor,
  Claude Code) — which Yap already does by typing into any window.
- **The real, tractable gaps** are: per-profile model choice, richer context-aware
  cleanup, meeting/system-audio recording, and cross-platform reach. See the matrix.

---

## superwhisper — inventory

**Organizing concept — "Modes".** A mode is a saved bundle: **voice (STT) model +
language (LLM) model + AI prompt + context toggles + per-app auto-activation**. The two
stages are independently local-or-cloud (e.g. local STT + cloud LLM). Built-ins: Voice to
Text, Message, Email, Note, **Super**, Meeting, Custom. Modes auto-activate by active
app/website (one-way: once an app's mode fires, no auto-switch back). Stored as JSON;
switchable via hotkey, menu, or `superwhisper://mode?key=…` deep links.

- **Model choice is per-mode.** Local Whisper (whisper.cpp) + local Parakeet (via Argmax
  WhisperKit, Apple-Silicon) + cloud (superwhisper S1, Deepgram Nova). LLMs: Claude,
  GPT-5, Groq Llama, superwhisper S1; **local LLMs macOS-only**; Gemini/Grok/GPT-5.5 via
  BYOK. Any OpenAI-compatible endpoint works.
- **"Agentic" = two bounded things.** (1) **Context-aware formatting** (Super/Custom
  modes read active-app + focused field text + selection + clipboard via macOS
  accessibility APIs) — but explicitly *formatting only*: "make this title case" ✅,
  "summarize this" ✗. (2) A **voice front-end plugin for Claude Code / Codex / OpenCode**
  (macOS-only, v2.13+; **Windows "in development"**) — pipes your voice in as the agent's
  prompt. Neither runs scripts or takes multi-step actions.
- **Meeting mode:** system-audio capture (per-mode "Record from System Audio") + optional
  diarization ("Identify Speakers", best via Deepgram Nova) → structured summaries with
  action items. Plus file/import transcription.
- **Other:** recognition-hint **Vocabulary** (biases the STT model) + deterministic
  post-transcription **Replacements**; realtime streaming (Nova cloud only); local History
  with "Process Again"; toggle/PTT/mouse-button controls; Hold-Shift auto-send; restore
  clipboard; per-keystroke simulation fallback.
- **Platforms:** macOS (full), Windows (behind — no local LLM, no agent integration, no
  keystroke sim, no config sync), iOS (keyboard dictation). **Pricing:** $8.49/mo ·
  $84.99/yr · **$249.99 lifetime**; free tier = basic dictation + free local Whisper
  (Standard/Nano/Fast); Pro gates custom modes, all AI modes, all local models, vocab,
  diarization.

## Wispr Flow — inventory

- **Cloud-only, always.** Every dictation uploads audio to Wispr's servers; **no offline
  mode at any tier** ("transcription always happens in the cloud"). Even "Privacy Mode" is
  zero-retention *cloud*, not local. This is Yap's single biggest wedge.
- **Core UX:** hold-to-talk hotkey; text inserted as a **polished chunk after you stop**
  (not live word-by-word — reviewers note it's "noticeable when you want word-by-word
  feedback"). Bottom recorder overlay. Marketing: "4× faster than typing."
- **Always-on multi-layer cleanup:** filler removal, punctuation, backtracking
  correction, **context-aware formatting** (Slack casual / email formal / code without
  filler), intensity None/Light/Medium/High + per-app "Personalized Style" tone. Known
  weakness: **over-edits** ("improves" what you said).
- **Command Mode (Pro-only) = voice editing.** Separate hotkey; three modes: transform
  selection ("make this concise", "translate to Polish", "rewrite as bullets"), generate/
  Q&A with no selection, and change settings by voice. **Text-only, not agentic** ("cannot
  create calendar events — only read them").
- **Personalization:** auto-learning personal dictionary, snippet library, writing-style
  learning, cross-device sync.
- **Platforms:** Mac, Windows, iOS, Android (one Pro sub). **Pricing:** Free = 2,000
  words/week (Mac/Win); **Pro $15/mo** ($12/mo annual) unlocks unlimited + Command Mode;
  Enterprise adds SOC 2 / ISO 27001 / HIPAA / SSO.

---

## The feature matrix

| Feature | superwhisper | Wispr Flow | Yap | Yap effort to match | Verdict |
|---|:---:|:---:|:---:|---|---|
| Local, on-device STT | ✅ (Mac best) | ✗ cloud-only | ✅ 14 models | — | 🏆 **Yap wins** — free + universal-GPU |
| Global hotkey, type into any app | ✅ | ✅ | ✅ | — | Parity |
| AI cleanup (filler/grammar/punct) | ✅ | ✅ always-on | ✅ | — | Parity |
| **Bundled zero-config local cleanup** | ◐ (Mac-only local LLM) | ✗ | ✅ llamafile+Qwen | — | 🏆 **Yap wins** |
| Per-app modes / auto-switching | ✅ Modes | ✅ Personalized Style | ✅ cleanup routing | — | Parity |
| **Per-profile/mode model choice** | ✅ per-mode | ◐ tone-only | ✅ per-profile override | — | **Matched (July 2026)** |
| Voice edit/rewrite of selection | ◐ format-only | ✅ Command Mode (Pro) | ✅ edit mode | — | Parity (Yap free, no mode-limit) |
| Voice *generation* on command ("summarize") | ✗ (blocked) | ✅ | ◐ write-mode | Low — broaden edit-mode prompt | Small polish |
| **Context-aware cleanup** (read focused text/selection/clipboard as LLM context) | ✅ Super Mode | ✅ | ◐ app-name only | **Med** — inject selection/field/clipboard into cleanup prompt | **MATCH — high value** |
| Live streaming partials | ◐ Nova cloud only | ✗ | ✅ opt-in | — | 🏆 **Yap wins** (validate + market) |
| Custom dictionary (exact replace) | ✅ Replacements | ✅ auto-learn | ✅ | — | Parity |
| Recognition-hint vocab (ASR biasing) | ✅ | — | ✗ | Med–High (engine hook) | See fuzzy-dictionary.md |
| Auto-learning vocabulary | ✗ | ✅ | ✗ | Med | Nice-to-have |
| History (local) | ✅ + reprocess | ◐ cloud | ✅ | — | Parity (+ add "reprocess") |
| Stats / streak dashboard | ✗ | ◐ Insights (Ent) | ✅ | — | 🏆 **Yap wins** |
| **Meeting recording / system audio** | ✅ | ✗ | ✗ | **Med** — WASAPI loopback + chunking | **MATCH — new surface** |
| Speaker diarization | ✅ | ✗ | ✗ | High | Skip for now |
| File/import transcription | ✅ | ✗ | ✗ | Low–Med | Cheap add w/ meeting work |
| Push-to-talk / toggle / mouse-button | ✅ | ◐ hold | ✅ | — | Parity |
| Auto-send (simulate Enter) | ✅ | — | ✅ auto_submit | — | Parity |
| Mute / **pause media** while recording | ✅ both | — | ✅ mute · ✗ pause | Low — send media-pause key | Small polish |
| Restore clipboard + keystroke fallback | ✅ | — | ✅ both | — | Parity |
| Deep-link / automation hooks | ✅ `superwhisper://` | — | ✗ | Low–Med | Optional |
| Config sync across devices | ◐ Mac-only | ✅ | ✗ | Med (needs a backend) | Later |
| **Platforms** | Mac/Win/iOS | Mac/Win/iOS/Android | **Win only** | Linux/Mac = Med–High; iOS = separate product | **MATCH (Linux→Mac)** |
| Price | $249 lifetime | $15/mo | **Free/OSS** | — | 🏆 **Yap wins** |

---

## What Yap already wins on (lean into these)

1. **Local + free + private + universal-GPU.** Wispr can't go offline; superwhisper's
   local path is Apple-Silicon-first. Yap runs any GPU (Vulkan/DirectML), free forever.
2. **Zero-config bundled cleanup.** No API key, no Ollama — the llamafile+Qwen sidecar is
   a real UX moat neither delivers cleanly on Windows.
3. **Live streaming partials.** Wispr has none; superwhisper needs cloud Nova. (Yap's is
   opt-in + needs GPU validation — worth finishing and *advertising a latency number*.)
4. **Windows-first polish.** superwhisper's Windows build is explicitly feature-behind its
   Mac build. This is the cleanest wedge for a Windows-first rival.
5. **History + stats dashboard.** Richer than either competitor's.

## What to match (prioritized — see ROADMAP for detail)

1. ~~**Per-profile model choice**~~ — **done (July 2026):** each cleanup profile can pick
   its own LLM (provider/URL/model/key), inheriting the global settings when unset.
2. **Context-aware cleanup** *(Med)* — feed the captured selection / focused-field text /
   recent clipboard into the cleanup prompt (Yap already captures the target HWND + does
   selection capture for edit mode; extend it to the dictation cleanup path).
3. **Meeting recording / system-audio** *(Med)* — WASAPI loopback + mic, chunked
   transcription, save transcript; file transcription rides along cheaply.
4. **Cross-platform: Linux → macOS** *(Med–High)* — port the four `cfg`-gated Win layers
   (input hooks, injection, selection, audio/mute). Linux first (OSS-audience overlap).
5. Small polish: "pause media while recording", history "Process Again", deep-link record.

## What to deliberately NOT chase (yet)

- **Full "agentic" autonomy / tool-calling.** Neither competitor actually does it; it's a
  research frontier, not a parity item. Yap's edit/rewrite mode is the right seed — treat
  a real version as a *deliberate exploration*, not a race (ROADMAP Phase 4).
- **Speaker diarization** — high effort, niche; revisit after meeting recording lands.
- **iOS** — a separate Swift product (no global hotkey; sandboxing forbids type-anywhere),
  not a port. Park it.
- **Cloud-scale personalization / style-learning + cross-device sync** — needs a backend,
  which cuts against Yap's local-first, no-accounts posture. Only if a hosted tier ever
  ships (see ROADMAP §6 pricing posture).

---

## Hands-on: superwhisper Windows app (installed + screenshotted, July 2026)

First-party observations from installing the Windows build (screenshots in
`E:\Pictures\2026-07-04_22-3*.png`). What their UX actually does, and what Yap
should take from it.

### Onboarding (a 6-step guided flow, progress bar on top)
1. **Permissions** — mic access with a live "✓ Allowed" state + a privacy link.
2. **"Using Superwhisper"** — a picture of the Windows system tray with an arrow
   pointing at their icon; buttons "Got it!" / "I can't find it". Solves tray-app
   discoverability head-on.
3. **Mic test** — "Speak and see if the waves react", live waveform, inline input-device
   picker. Catches wrong-mic problems *before* first dictation.
4. **Pro upsell** (skippable "Maybe later").
5. **Cloud vs Local model** — two equal cards; honest copy ("recordings go to the cloud
   to process but are never stored there").
6. **"Try the shortcut"** — press Ctrl+Space and dictate *into a text box inside the
   onboarding itself*; "Change shortcut" link right there. The user proves the core loop
   works before onboarding closes.

### Main app (sidebar: Home / Modes / Vocabulary / Configuration / Sound / Models library / History)
- **Home** = stats strip (avg WPM · words this week · apps used · minutes saved) + a
  **"Get started" checklist** (start recording / customize shortcuts / create a mode /
  add vocabulary) + a **"What's new?" changelog feed**. Feature discovery, not just config.
- **Vocabulary** — one combined entry bar: type a word → "Add word ↵" *or* "Replace
  with… ⇧↵". Two features (recognition hint vs replacement) in one compact affordance.
- **Sound** — auto mic-volume boost, **silence removal**, dynamic normalization,
  **"Playback when recording: [Pause ▾]"** (the pause-media feature), sound-effects
  toggle + volume.
- **Models library** — a single searchable list mixing **LLMs and STT models** (Anthropic
  Sonnet 4.5/4.6/5, GPT-5.x family, Gemini 3.x, Grok, Deepgram Nova, NVIDIA Parakeet
  1.1 GB, their own S1/Fast/Nano/Pro/Standard/Ultra locals 75 MB–3 GB), with columns for
  **type icon, Speed/Accuracy bars, Cloud/Offline, size + download button**, star
  favourites, a provider filter, and a BYOK key button.
- **Recording pill** — compact floating capsule (mode ✦ · logo · expand ⤢) with a hover
  tooltip "Start recording Ctrl+Space".

### Takeaways for Yap (ranked)
1. **Onboarding v2** — Yap's onboarding is a model picker only. Adopt: mic-test step
   (Yap already has the live waveform component), a **"press F9, try it here"** step with
   an inline textbox, and — critically — an **AI-cleanup step offering the built-in local
   model one-click**. Yap's #1 differentiator (zero-config local cleanup) is currently
   buried in Settings, off by default, invisible to a new user. superwhisper puts model
   choice *in onboarding*; Yap should put its cleanup wedge there.
2. **Tray-discovery step** — Yap is tray-first with all windows hidden; a "here's where
   Yap lives" pointer step is nearly free and prevents "the app vanished" confusion.
3. **Pause media while recording** — they ship it as a Playback dropdown (Pause). Already
   on Yap's small-polish list; screenshot confirms the UX shape.
4. **Speed/Accuracy bars in the model browser** — a cheap visual upgrade to Yap's
   ModelCard (the registry already knows each model's traits).
5. **Get-started checklist on first run** — Yap's power features (edit mode, per-app
   profiles, per-profile models) are exactly the kind that need discovery nudges.
6. **Skip**: the unified LLM+STT model list (Yap's STT-models vs cleanup-provider split is
   clearer), Home-page WPM strip (Yap's stats already live in History).

## Sources

**superwhisper** — [docs repo](https://github.com/superultrainc/superwhisper-docs)
([modes](https://raw.githubusercontent.com/superultrainc/superwhisper-docs/main/modes/modes.mdx),
[voice models](https://raw.githubusercontent.com/superultrainc/superwhisper-docs/main/models/voice.mdx),
[language models](https://raw.githubusercontent.com/superultrainc/superwhisper-docs/main/models/language.mdx),
[super mode/context](https://raw.githubusercontent.com/superultrainc/superwhisper-docs/main/modes/super.mdx),
[windows](https://raw.githubusercontent.com/superultrainc/superwhisper-docs/main/get-started/windows.mdx)),
[site](https://superwhisper.com/), [claude-code integration](https://superwhisper.com/claude-code),
[changelog](https://superwhisper.com/changelog), [pricing](https://superwhisper.com/#pricing).

**Wispr Flow** — [site](https://wisprflow.ai/), [pricing](https://wisprflow.ai/pricing),
[privacy](https://wisprflow.ai/privacy),
[Command Mode](https://docs.wisprflow.ai/articles/4816967992-how-to-use-command-mode),
[plans](https://docs.wisprflow.ai/articles/9559327591-flow-plans-and-what-s-included);
reviews: [spokenly](https://spokenly.app/blog/wispr-flow-review),
[willowvoice](https://willowvoice.com/blog/wispr-flow-review-voice-dictation),
[zapier](https://zapier.com/blog/wispr-flow/).
