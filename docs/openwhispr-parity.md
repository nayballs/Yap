# OpenWhispr ↔ Yap — Language Models feature parity

> **Purpose.** A living checklist of OpenWhispr's **Language Models** settings surface
> (all four tabs × five modes × every sub-feature) mapped to Yap's status, so "are we
> 1:1?" is a document lookup, not a screenshot hunt. Regenerate by re-running the
> `openwhispr-parity-matrix` audit workflow when either side changes.

Reference repo: `E:/Projects/references/openwhispr`. Last audit: 2026-07-06.

**Legend** — ✅ Done (equivalent exists) · 🟡 Partial (present but less complete) · ❌ Gap (missing, not a deliberate choice) · ⚪ Intentional (deliberate divergence, documented in `ROADMAP.md`).

**Status:** 35/57 ✅ done · 6 🟡 partial · 10 ❌ gap · 6 ⚪ intentional *(as of 2026-07-06, recounted from the table below)*.

> **Update — 2026-07-05, post-audit.** Built the **Local-mode model browser** (former headline
> gap #1). The 6 SHA-pinned curated models are now surfaced as downloadable **cards** — name, size,
> blurb, **"Learn more" → HuggingFace**, **"Recommended"** badge, per-card **Download** with
> progress, **Use**, and **delete** — backed by the existing `local_llm_install(id)` plus a new
> `local_llm_delete` command. This closes Area-5 rows: available-models list, per-model download
> progress, delete, and Learn-more/Recommended.
>
> **Update 2 — same day.** Added **per-family provider tabs** (Qwen / Meta Llama / Gemma / Mistral /
> Phi) with **brand icons**, plus a **brand icon on every model card** — matching OpenWhispr's local
> layout. Expanded the curated set from 6 → **11 real, SHA-pinned models** (added Qwen2.5 0.5B/7B,
> Llama 3.1 8B, Gemma 2 9B, Mistral 7B v0.3; hashes fetched from each HuggingFace repo's LFS
> pointer). Note OpenWhispr's own local registry lists ~26 models, but they're **fictional
> near-future names** (Qwen3.5, Gemma 4, GPT-OSS) on non-existent repos with **no hashes** — Yap
> uses real, verifiable GGUFs instead. Re-run the audit workflow for a fresh full matrix; the table
> below still reflects pre-build state.

## Headline gaps (prioritised)

1. ~~Local mode is a single bundled model + BYO-folder picker~~ — **✅ closed (2026-07-05):** built the curated model browser (see the update above).
2. Self-Hosted mode can't discover models: it takes a free-text model id with no live {base}/models fetch, Refresh, model card list, or endpoint validation (Settings.svelte:1434).
3. ~~No "Disable thinking output" toggle anywhere~~ — **✅ closed (2026-07-05):** added the toggle (shown for reasoning models via `ppModels.js` `PP_THINKING_MODELS`, or custom/self-hosted endpoints) on the cleanup tab + all scopes; when on, the backend strips `<think>…</think>` blocks from the output (`llm::strip_thinking`, provider-agnostic — safer than OpenWhispr's request-param approach given Yap's raw-fallback design). Config: `pp_disable_thinking` + `LlmScope.disable_thinking`.
4. Local downloader lacks per-model download progress detail, cancel, and delete — one all-or-nothing install button with percent/MB only (no speed/ETA), and GGUF removal is folder-only.
5. No GPU-acceleration banner for the local LLM (Vulkan enable/status/remove/retry); mitigated because llamafile bundles GPU backends and auto-offloads, but there's no visibility into whether the GPU is actually being used.
6. Model cards omit "Recommended" badges and "Learn more"/HF repo links across both cloud and local (ppModels.js carries only value/label/desc).
7. Prompt Studio Test tab has no dynamic Instruction/Cleanup badge and no agent-name detection in the input row (PromptStudio.svelte:218).
8. No GpuDeviceSelector for the intelligence/cleanup LLM and no mode-switch toasts — minor polish gaps versus OpenWhispr's cross-cutting UI.

## Full matrix

### Area 1 — Section header + 4 bubble tabs

| Feature | Status | Notes |
|---|---|---|
| "Language Models" section title + description | ✅ Done | Settings.svelte renders a Language Models section with the SCOPE_TABS bubble row; header copy present. |
| 4 scoped bubble tabs (Cleanup / Voice Agent / Note Formatting / Chat) with icons | ✅ Done | SCOPE_TABS + PillTabs renderIcon={scopeIcon} (Settings.svelte:114,1325) draws the same four tabs with wand/sparkles/book/message icons. |
| Only active tab renders; state preserved (hidden others) | ✅ Done | Yap swaps the active scope component via llmScope state; per-scope state lives in cfg.llmScopes so it persists across tab switches. |
| Tab selection persisted to localStorage (settings.llmsTab) | ❌ Gap | llmScope is plain $state (Settings.svelte:~1325) that resets on reload; would take one localStorage read/write to match — minor. |

### Area 2 — Per-tab enable toggle + GPU selector

| Feature | Status | Notes |
|---|---|---|
| Cleanup "Enable text cleanup" toggle gating the mode editor | ✅ Done | cfg.postProcessEnabled toggle gates the whole cleanup config (config.rs post_process_enabled; Settings.svelte cloudcfg class:off). |
| Voice Agent "Enable voice agent" toggle | ✅ Done | VoiceAgentConfig.svelte binds scope.enabled with the wake-word desc copy matching OpenWhispr. |
| Note Formatting "Auto-generate note titles" toggle | ⚪ Intentional | NoteFormattingConfig.svelte deliberately drops the auto-title toggle (documented in-file: nothing to drive without a notes backend) and substitutes an "Enable note formatting" toggle. |
| Chat: mode editor renders unconditionally (no enable toggle) | 🟡 Partial | ChatConfig.svelte adds an "Enable chat" toggle instead of always-on; harmless divergence since Chat runtime is coming-soon. |
| GpuDeviceSelector purpose="intelligence" under the cleanup mode editor | ❌ Gap | No per-device GPU dropdown for the cleanup LLM; llamafile auto-offloads via -ngl 999 (local_llm.rs:246) so there is no device choice to expose — not documented as intentional. |

### Area 3 — Mode selector (5 modes)

| Feature | Status | Notes |
|---|---|---|
| Radio-style mode selector (icon tile + label + desc + Active pill) | ✅ Done | PP_MODES + ModeSelector (Settings.svelte:87,1350) and ScopeProviderConfig MODES render the icon-tile radio list. |
| Mode: Cloud Providers (BYOK) | ✅ Done | ppMode 'cloud' + CLOUD_TABS provider pills (Settings.svelte:94,1358). |
| Mode: Local (on-device) | ✅ Done | ppMode 'ondevice' → PROVIDER_ONDEVICE llamafile sidecar (local_llm.rs). |
| Mode: Self-Hosted | ✅ Done | ppMode 'selfhosted' → provider 'local', OpenAI-compatible base URL (Settings.svelte:1421). |
| Mode: OpenWhispr Cloud (managed, zero-config) | ⚪ Intentional | Yap has no hosted backend; documented as parked research in ROADMAP Phase 4 "Hosted / managed AI modes" — Yap ships 3 real modes. |
| Mode: Enterprise (Bedrock/Azure/Vertex brokerage) | ⚪ Intentional | Explicitly out of scope per ROADMAP Phase 4 (needs cloud-account brokerage Yap doesn't have). |
| Switching mode auto-clears invalid provider/model | ✅ Done | onModeChange/onCloudProviderChange reset provider, baseUrl and model when invalid (ScopeProviderConfig.svelte:59-83; Settings.svelte:730). |

### Area 4 — Cloud Providers (BYOK) sub-UI

| Feature | Status | Notes |
|---|---|---|
| Provider pill-tab row | ✅ Done | CLOUD_TABS: Groq/Anthropic/OpenAI/Gemini/OpenRouter/Custom — a superset of OpenWhispr's 5 (OpenRouter is an intentional add, ROADMAP Phase 2). |
| Per-provider API key heading + "Get your API key →" link + masked input | ✅ Done | ScopeProviderConfig keyhead + keylink (PP_CLOUD_MODELS[p].keyUrl) + masked keymask. Links open externally via tauri-plugin-opener (externalLinks.js, port of OpenWhispr utils/externalLinks.ts) — a bare target=_blank is dead in a Tauri webview. |
| API keys global per provider, shared across all scopes | ✅ Done | Like OpenWhispr's openai_api_key etc.: standard-provider keys live in cfg.ppApiKeys and are shared by every scope (ScopeProviderConfig write-through + YapConfig::provider_api_key backend fallback); only custom/self-hosted keys stay per-scope (= OpenWhispr customApiKey). Fixed rewrites firing keyless at Anthropic while the Cleanup tab held a Groq key. |
| Model registries (OpenAI 7 / Anthropic 7 / Gemini 6 / Groq 9) | ✅ Done | ppModels.js matches those counts (groq 9, anthropic 7, openai 7, gemini 6) plus openrouter 5; fast-cleanup-first ordering is an intentional divergence (ppModels.js header). |
| Model cards: LED dot, brand icon, name, desc, Active badge, click-to-select | ✅ Done | SelectList over cloudModelOptions with brand icon + label + desc + selected state (ScopeProviderConfig.svelte:85; Settings.svelte:1415). |
| Per-model "Recommended" badge + "Learn more" link | 🟡 Partial | Local (on-device) cards have Recommended badges + Learn-more links; cloud model rows still don't carry them. |
| "Disable thinking output" toggle for thinking-capable models | ✅ Done | PP_THINKING_MODELS/modelThinks() gates a toggle in the cleanup tab + every scope (ScopeProviderConfig); backend strips `<think>` blocks (llm::strip_thinking) — output-stripping instead of OpenWhispr's request params, safer with Yap's raw-fallback design. |

### Area 5 — Local mode (on-device downloader)

| Feature | Status | Notes |
|---|---|---|
| Multi-family local provider tabs (Qwen/Mistral/Llama/OpenAI/Gemma) | ✅ Done | Family pill tabs (Qwen/Llama/Gemma/Mistral/Phi) with brand icons over the curated browser (Settings.svelte LLM_FAMILY_META/familyModels). |
| "Available Models" list with many downloadable models | ✅ Done | 11 SHA-pinned CURATED_MODELS (local_llm.rs) surfaced as OpenWhispr-style cards: icon, name, size, desc, Recommended badge, Learn-more link, Download/Active. |
| Per-model download with progress bar (bytes/speed/ETA) | 🟡 Partial | Per-model download via install_curated + local-llm-download-progress — percent + MB, no speed/ETA. |
| Cancel in-progress download | ❌ Gap | installLocalLlm has no cancel path; download runs to completion or errors. |
| Delete a downloaded model (trash + ConfirmDialog) | ✅ Done | Per-card delete via local_llm_delete (commands.rs); switching/active model guarded. |
| BYO custom model picker | ✅ Done | Drop any .gguf into <data>/llm/ and select via cfg.ppLocalModel; list_models() enumerates the folder (local_llm.rs:113; Settings.svelte:797) — Yap-specific, matches its single-sidecar design. |
| HF "Learn more" links + Recommended badges on model cards | ❌ Gap | Not present; the folder picker is a plain Select with no HF repo links or badges. |
| GPU-acceleration banner (Vulkan enable/status/remove/retry) | ❌ Gap | No GPU banner; llamafile bundles GPU backends and auto-offloads (local_llm.rs:246), so there's no separate Vulkan binary to manage — arguably moot but undocumented as intentional. |
| "Disable thinking output" toggle for local thinking models | ❌ Gap | Absent; same missing supportsThinking plumbing as cloud. |

### Area 6 — Self-Hosted sub-UI

| Feature | Status | Notes |
|---|---|---|
| Endpoint URL + optional API key inputs | ✅ Done | selfhosted mode renders Base URL + optional API key rows (Settings.svelte:1421; ScopeProviderConfig.svelte:163). |
| Live model fetch from {base}/models + Refresh + validation states | ❌ Gap | No live model discovery; the user types a model id into a text Input (Settings.svelte:1434) — no /models query, Refresh, or HTTPS/401 validation messaging. |
| Model card list of fetched remote models | ❌ Gap | Consequence of the above — self-hosted has no selectable model list, only free-text entry. |
| "Disable thinking output" toggle | ❌ Gap | Not present for self-hosted mode. |

### Area 7 — Enterprise sub-UI

| Feature | Status | Notes |
|---|---|---|
| AWS Bedrock / Azure OpenAI / Vertex config (auth, regions, test connection) | ⚪ Intentional | No Enterprise mode at all; documented out-of-scope in ROADMAP Phase 4 (cloud-account brokerage Yap deliberately doesn't offer). |

### Area 8 — Voice Agent tab extras

| Feature | Status | Notes |
|---|---|---|
| Agent Name field + Save (syncs into dictionary) | ✅ Done | VoiceAgentConfig.svelte saveAgentName persists cfg.agentName and adds a self-mapping dictionary entry so STT spells it right (config.rs agent_name). |
| "How it works" + 3 "Examples" sections interpolating {agentName} | ✅ Done | VoiceAgentConfig.svelte renders both, with live displayName interpolation. |
| Agent prompt (Prompt Studio) when enabled | ✅ Done | PromptStudio bound to scope.prompt with EDIT_BASE_PROMPT guardrails via get_edit_base_prompt (VoiceAgentConfig.svelte:161). |
| Voice Agent wake-word runtime (fires by saying the name during dictation) | ✅ Done | `agent_detect.rs` = faithful detectAgentName port (word-boundary + adjacent-join + length-scaled Levenshtein, unit-tested); pipeline `wake_word_hit` (reachability port) routes the whole transcript through the Voice-Agent scope in write mode. Default agent prompt = OpenWhispr's `fullPrompt` verbatim ({{agentName}} substituted at request time; old default migrates). Both triggers now work: wake word + edit hotkey. |
| "Agent Name Updated" alert dialog on save | 🟡 Partial | Yap shows an inline savednote confirmation instead of a modal alert — equivalent feedback, lighter UX. |

### Area 9 — Note Formatting tab extras

| Feature | Status | Notes |
|---|---|---|
| 5-mode inference editor (scope=noteFormatting) | ✅ Done | NoteFormattingConfig.svelte mounts ScopeProviderConfig over cfg.llmScopes.noteFormatting (3 modes; managed/enterprise intentionally absent). |
| Note-formatting runtime (actually formats notes) | ✅ Done | The Notes surface's Enhance button (NotesView.svelte → note_enhance): OpenWhispr's Actions engine ported — BASE_SYSTEM_PROMPT verbatim (llm::NOTE_BASE_PROMPT) + the scope's editable fragment (default = the built-in "Generate Notes" prompt verbatim), temp 0.3, enhanced_content + len+first-50 staleness hash, endpoint fallback to cleanup (fallbackScope). ⚠ Awaiting live runtime test. |
| "Auto-generate note titles" toggle | ⚪ Intentional | Still skipped: Yap derives titles from the first 6 words (OpenWhispr's own fallback); AI title generation can come with the Actions expansion. |

### Area 10 — Chat tab extras

| Feature | Status | Notes |
|---|---|---|
| System Prompt editor | ✅ Done | ChatConfig.svelte uses the full PromptStudio (View/Customize/Test) instead of OpenWhispr's plain 4-row textarea — a superset. |
| Chat runtime (answers questions over notes) | 🟡 Partial | THREE live layers: the **embedded per-note chat** (note_ask), the **Chat surface** (ChatView.svelte: date-grouped conversation sidebar, chats.json persistence, eager keyword-RAG, last-20 history, Ctrl+N), and the **tool-calling agent loop** (tools.rs: their six tools w/ near-verbatim schemas + TOOL_INSTRUCTIONS, ≤20-step Rust loop, their 4B capability gate, tool-activity chips in the thread — live-verified creating notes and copying to the clipboard). Remaining vs theirs: streaming, semantic vectors, web_search/calendar tools, conversation search/archive/rename. |

### Area 11 — Prompt Studio

| Feature | Status | Notes |
|---|---|---|
| 3-tab card: View / Customize / Test | ✅ Done | PromptStudio.svelte replicates the tabbed card, shared by Cleanup + all three scope tabs. |
| View: Default/Custom label, "Modified" pill, Copy button, mono <pre> | ✅ Done | PromptStudio.svelte:137-157 — shows full effective prompt (guardrails composed in) with Modified chip + Copy. |
| Customize: Caution note, mono textarea, Save + Reset | ✅ Done | PromptStudio.svelte:158-189, with a savedNote confirmation. |
| Test: meta row, input, Run Test, output block, disabled banner | ✅ Done | PromptStudio.svelte:190-241 runs the real test_post_process command with edited-prompt apply/restore semantics. |
| Test input dynamic "Instruction/Cleanup" badge (agent-name detection) | ❌ Gap | Yap's Test input has a static "Input" label (PromptStudio.svelte:218); no badge that flips on wake-word detection. |
| {{agentName}} placeholder templating + substitution | ⚪ Intentional | Yap doesn't use {{agentName}} templating (documented intentional in ROADMAP Phase 2 cleanup-mirror notes); the agent name is applied via the dictionary instead. |
| Named tone/format preset picker | ✅ Done | Yap ADDS what OpenWhispr lacks: PP_PRESETS (Default/Email/Notes/Slack/Code) via cfg.ppPreset (config.rs pp_preset) — a superset feature. |

### Cross-cutting

| Feature | Status | Notes |
|---|---|---|
| Toast notification system (ui/Toast.tsx) | ✅ Done | Full port: ui/toast.svelte.js + ToastHost.svelte (variant accent bars, hover-pause, destructive mono error box w/ copy, progress hairline, slide in/out; 3.5 s / 6 s durations). Mounted in ControlPanel; wired to action runs, meeting start/stop, upload done/error, copies, Debug-mode toggle (their exact toast copy), and backend `yap-error` events. Mode-transition toasts in the LLM config remain unwired (cosmetic). |
| Local model downloader infrastructure (reusable) | ✅ Done | The Local-mode LLM browser (family tabs + cards + download/delete) shipped in Settings, reusing the ModelManager patterns; backend = 11 curated SHA-pinned GGUFs. |
| Local mode shared across all 4 scope tabs | 🟡 Partial | Non-cleanup scopes' Local mode just points users to install under Dictation Cleanup and shares that one sidecar (ScopeProviderConfig.svelte:186) rather than each tab having its own downloader — reasonable for Yap's single-sidecar model. |
