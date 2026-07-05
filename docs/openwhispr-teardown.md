# OpenWhispr teardown — settings UX, AI Notepad, AI Chat, Audio Upload

> Purpose: a source-level breakdown of the four OpenWhispr features Yap wants to learn
> from — its **settings UX**, **AI Notepad**, **AI Chat**, and **Audio Upload** — with
> **what to steal, what to skip, and rough effort (S/M/L)** for each, framed for Yap's
> stack. Paired with [`ROADMAP.md`](../ROADMAP.md) (the prioritized plan),
> [`CLAUDE.md`](../CLAUDE.md) (how Yap works today), and
> [`competitive-analysis.md`](./competitive-analysis.md) (superwhisper/Wispr Flow).
> Sourced from a full read of the cloned OpenWhispr repo (Electron/React,
> `E:\Projects\references\openwhispr`) — file:line refs are into **that** tree, not Yap.
> Last refreshed: **July 2026** (OpenWhispr ~1.7.x, ~4.3k★).

Legend for Yap status: ✅ have · ◐ partial · ✗ gap · 🏆 Yap already ahead.

---

## TL;DR — what to steal, in priority order

OpenWhispr is an Electron notes-and-meetings app built *around* a dictation core; Yap is a
dictation pill. Most of what makes OpenWhispr feel richer is **patterns + a small notes data
model + stateless LLM calls** — all of which port cleanly. The genuinely heavy parts (a
Qdrant sidecar, a cloud SaaS backend, an ONNX utility process) are things Yap should **drop
and replace with lighter Rust-native equivalents**, not copy.

| # | Steal this | Where it lands in Yap | Value | Effort | Verdict |
|---|---|---|---|:---:|---|
| 1 | **Scope-driven AI config editor** (one editor drives N AI use-cases via a `scope` prop) | Settings | High | M | **Do it** — the single biggest reason their settings feel "better" |
| 2 | **Radio-list "mode selector"** (icon tile + label + Active pill + description) | Settings | High | S | **Do it** — huge polish-per-line |
| 3 | **Hotkey-capture input** (live modifier chips, hold-to-capture, inline conflict validation) | Settings | High | M | **Do it** — Yap already validates conflicts; this is the UI |
| 4 | **AI "Actions" engine** (user-editable prompt fragment × app-owned system prompt → `enhanced_content`) | New notepad | High | S–M | **Do it** — smallest infra, biggest "AI notepad" payoff |
| 5 | **Audio Upload** (drag a file → local transcription → note) | New / Phase 6 | Med | M | **Do it** — decoder is the only real cost; reused by meeting work |
| 6 | **Notes + folders store** + markdown editor + raw/enhanced dual-view | New notepad | Med | M | Do it if pursuing the notepad surface |
| 7 | **Eager RAG injection** (search notes *before* the LLM, inline top-5 snippets into the prompt) | AI chat | Med | S | **Do it** — "knows your notes" with no agent loop, works on the tiny local model |
| 8 | **Container-query responsive settings rows** | Settings | Low | S | Do it — native CSS `@container`, cleaner than their JS |
| 9 | Tool-calling **agent loop** over `llm.rs` + `search_notes` | AI chat | Med | M–L | Later — gate on model capability |
| 10 | Full **semantic search** (embeddings + vector store + RRF) | AI chat | Med | L | Later / optional — use `fastembed-rs` + `sqlite-vec`, **not** Qdrant |
| — | Speaker diarization, cloud sync, enterprise providers, meeting auto-detection | — | — | L | **Skip** — orthogonal to Yap's wedge |

**One-line strategy:** ship #1–#4 + #7 first (they're small and independently useful),
treat the notepad/chat/upload surface as a deliberate expansion from "pill" to "pill + notes,"
and replace every Electron/SaaS-shaped dependency with a Rust-native one.

---

## 0. Stack translation (read this once)

Everything below assumes this mapping. OpenWhispr runs almost all logic in the **renderer**
(Electron gives it Node access via `window.electronAPI`); Yap's Svelte webview is sandboxed,
so the same logic moves to **Rust** and the UI talks to it over Tauri commands/events.

| OpenWhispr (Electron/React) | Yap (Tauri/Rust/Svelte) |
|---|---|
| `window.electronAPI.foo()` IPC | `#[tauri::command] fn foo()` in `commands.rs` |
| `event.sender.send("x", payload)` | `app.emit("x", payload)` + Svelte `listen()` |
| `better-sqlite3` DB | `tauri-plugin-sql` (SQLite) **or** JSON files (Yap's current style) |
| Zustand flat store + per-key `localStorage` writes | `YapConfig` struct + `get_config`/`save_config` (debounced) |
| React + shadcn/ui + Radix | Svelte 5 + `src/lib/ui/` primitives |
| TipTap (ProseMirror, React) editor | Milkdown / CodeMirror 6 / framework-agnostic Tiptap in a Svelte action |
| FFmpeg (bundled) decode → 16 kHz WAV | **Symphonia** (pure-Rust decode) → resample → f32 |
| ONNX utility process + Qdrant sidecar | `fastembed-rs` (in-process) + `sqlite-vec`/`usearch` |
| `ReasoningService.processText` (8-provider registry) | Yap's existing `llm.rs` (OpenAI-compatible) + llamafile sidecar |
| whisper.cpp server / sherpa-onnx child procs | `transcribe-rs` warm engine (`stt.rs`/`pipeline.rs`) |

Key consequence: several of OpenWhispr's most elaborate mechanisms exist **only** to survive
Electron's constraints (process-isolating native ONNX so a `bad_alloc` doesn't kill the app;
"lazy keep-alive" of settings sections so an unmounting React tree doesn't drop an in-flight
download). In Rust those problems mostly evaporate — **don't port the workaround, port the goal.**

---

## 1. Settings UX — why theirs feels better

### Architecture
A Radix dialog (`SettingsModal.tsx`) wraps a reusable sidebar shell (`ui/SidebarModal.tsx`)
around one ~4,000-line page (`SettingsPage.tsx`) that renders exactly one section at a time
via `switch (activeSection)`. State is a **single flat Zustand store**
(`stores/settingsStore.ts`) where every setting is a top-level key and setter factories
(`createStringSetter`/`createBooleanSetter`, `settingsStore.ts:674`) write to `localStorage`
**and** `setState` in one call, so persistence is synchronous and automatic.

### Information architecture (9 sections, 4 groups)
Defined in `SettingsModal.tsx:57-128`. Two sections have their own **sub-tabs**.

| Group | Section | Contents |
|---|---|---|
| Account | Account / Plans & Billing / Workspace | user card, Stripe, teams *(SaaS — skip for Yap)* |
| App | **General** | theme, UI language, mic, notifications, audio cues, auto-paste, autostart |
| App | **Hotkeys** | dictation / meeting / chat-agent / voice-agent hotkeys, activation mode |
| AI Models | **Speech-to-Text** | sub-tabs: **Dictation · Note recording · Upload** — each a transcription-mode picker + VAD |
| AI Models | **LLMs** | sub-tabs: **Dictation cleanup · Dictation agent · Note formatting · Chat intelligence** — each an `InferenceConfigEditor` |
| System | **Privacy & Data** | permission cards, telemetry, audio retention |
| System | **System** | updates, dev tools, remove models |

The lesson: OpenWhispr has **four separate AI use-cases** (cleanup, agent, note formatting,
chat) and **three transcription contexts** (dictation, note recording, upload), and it exposes
each with the *same* editor rather than bespoke UI. That's the whole trick.

### Standout patterns (with refs)

- **Per-scope inference editor — the flagship.** `settings/InferenceConfigEditor.tsx` is one
  component parameterized by `scope`. It renders a mode selector (cloud / BYOK / local /
  self-hosted / enterprise, `:63-96`), a model picker, endpoint panels, and a "disable
  thinking" toggle that only appears when the model supports it (`:152-156`). It reads/writes
  purely through `selectResolvedLLMConfig(state, scope)` / `setResolvedLLMConfig(scope, patch)`
  and knows **nothing** about which storage keys back it — that's declared in
  `config/inferenceScopes.ts`, where each scope names its keys and `noteFormatting` sets
  `fallbackScope: "dictationCleanup"` (`:55`) so an unconfigured scope inherits another's
  provider/model (recursive resolver, `settingsStore.ts:1861-1890`). **This is why 4 AI
  features share one polished editor with zero duplication.**
- **Radio-list mode selector.** `InferenceModeSelector` (`ui/SettingsSection.tsx:141-218`):
  each mode is a full-width row — icon tile, label, live "Active" pill, description, custom
  radio dot; disabled modes show a badge ("Free account required") and route to onboarding
  instead of selecting. Reused verbatim for transcription mode, upload mode, and all 4 LLM
  scopes.
- **Hotkey capture input.** `ui/HotkeyInput.tsx` — a focusable `div role="button"` reading raw
  `KeyboardEvent.code` via a `CODE_TO_KEY` table (`:7-129`), handling modifier-only combos with
  a 200 ms hold threshold (`:222`), right-vs-left modifier disambiguation (`:224`), and mouse
  buttons 4/5. Live "listening" state renders held modifiers as `<kbd>` chips, runs a
  `validate` callback, and surfaces conflicts inline (`:274-285`); sets `data-capturing` so
  Escape won't close the modal mid-capture. Two variants: compact inline + a large `hero` for
  onboarding.
- **Animated sliding sub-tabs.** `ui/ProviderTabs.tsx` — one absolutely-positioned indicator
  slides to the active pill via `getBoundingClientRect` + `ResizeObserver` (`:39-70`).
- **Container-query-style responsive rows.** `SidebarModal.tsx:52-58` watches its own width via
  `ResizeObserver`; below 800 px it collapses the sidebar to icons and publishes `isCompact`
  through context so rows stack (`SettingsSection.tsx:72-78`) — no media queries.
- **Prompt Studio.** `ui/PromptStudio.tsx` — a Current/Edit/**Test** tabbed editor that runs the
  system prompt live against the configured model before saving. Embedded in cleanup + agent.
- **Permission cards.** `ui/PermissionCard.tsx` — flips from a "grant" affordance to a green
  check when `granted` (`:31-49`).

### Port to Yap

| Pattern | Match / Skip | Effort | Notes |
|---|---|:---:|---|
| **Per-scope AI editor** + `INFERENCE_SCOPES` map + `resolvedConfig(config, scope)` w/ fallback chain | **Match** | **M–L** | Highest value. Pays off the moment Yap has a 2nd AI use-case (cleanup + note-formatting/agent). Because Yap persists a whole struct, `setResolvedLLMConfig` = "patch struct fields → debounced `save_config`" — *simpler* than their per-key localStorage. Skip the "Enterprise" + OpenWhispr-cloud modes; keep providers(BYOK)/local/self-hosted. |
| **Radio-list mode selector** | **Match** | **S** | Pure presentation over Yap's `Group`/`Row`. Do this even if you skip the scope editor. |
| **Hotkey capture input** | **Match** | **M** | Port the `CODE_TO_KEY` table verbatim (platform-agnostic). **Skip the macOS Fn/Globe IPC branch** — Yap is Windows-first. Yap already has cross-slot conflict validation; this gives it a real UI. Registration still goes through Tauri global-shortcut (roll back on failure for good UX). |
| **Container-query responsive rows** | **Match, simplified** | **S** | Use native CSS `@container` (`container-type: inline-size`) instead of their ResizeObserver+context — that machinery is an Electron-era workaround. |
| **Prompt Studio (Current/Edit/Test)** | **Match** | **S–M** | Yap already has a "Test" button for cleanup; the Current/Edit/Test tabs are a nice upgrade. |
| **Animated sliding sub-tabs** | Match | **S–M** | Only once a Yap section grows sub-modes (Models/AI Cleanup will). A Svelte `transition:` is idiomatic. |
| **Permission cards** | Match, Windows-scoped | **S** | Mic permission only; skip macOS accessibility/system-audio variants. |
| Lazy keep-alive of heavy sections | **Skip** | — | Keep download state in a store/Rust, not component state, and the problem disappears. |
| Flat Zustand store + per-key localStorage | **Skip** | — | Yap's `YapConfig` + `save_config` is the right model; don't adopt their persistence. |

**Net:** the three that make OpenWhispr read as "much better" are the **scope-driven AI editor
(#1)**, the **radio-list mode selector (#2)**, and the **hotkey input (#3)**. Everything else is
polish on top.

---

## 2. AI Notepad — "meeting notes enhanced by AI"

Two features sharing one data model: **(a) an AI-enhanced markdown notepad** and **(b) a live
meeting transcriber**, both writing into a `notes` table and both able to invoke the same
"Actions". ([openwhispr.com/notepad](https://openwhispr.com/notepad))

### Data model (`src/helpers/database.js`, SQLite)
- **`notes`** (`:104`+): `id, title, content` (raw markdown), `note_type` (`personal|meeting`),
  `enhanced_content` (LLM output — the "Enhanced" tab), `enhancement_prompt`,
  `enhanced_at_content_hash` (staleness marker), `folder_id`, `transcript` (serialized
  meeting segments), `participants` (JSON), `source_file`, `audio_duration_seconds`,
  timestamps. A `notes_fts` FTS5 virtual table mirrors `title/content/enhanced_content` via
  triggers (`:138-168`) for keyword search.
- **`folders`** (`:181`): seeded with **Personal** + **Meetings** on first run (`:191-198`).
- **`actions`** (`:216`): `{name, description, prompt, icon, is_builtin, sort_order}` — seeded
  with one built-in **"Generate Notes"** (`:283-308`).
- Meeting aux: `speaker_profiles` (voice embedding BLOB), `speaker_mappings`,
  `note_speaker_embeddings`, `contacts`, Google Calendar tables. *(All optional for Yap.)*

`transcript` is **not** free text — it's a serialized array of `TranscriptSegment`
`{id, text, source: "mic"|"system", timestamp, speaker, speakerName}`
(`src/utils/transcriptSpeakerState.ts`). So a note carries three distinct text streams:
`content` (typed), `transcript` (recorded), `enhanced_content` (AI output).

### The editor
**TipTap (ProseMirror) + a markdown bridge** (`src/components/ui/RichTextEditor.tsx`).
Extensions: `StarterKit` (H1–H3, lists), **`task-list` + `task-item`** (checkboxes for meeting
action items), `placeholder`, `tiptap-markdown`. Critically, **data in/out is markdown
strings** — `onUpdate` reads `editor.storage.markdown.getMarkdown()`; external changes call
`editor.commands.setContent(md)` guarded against re-entrancy (`:73-90`). The whole feature is
markdown text end-to-end → very portable. Dictation does **not** stream tokens into the doc;
recording accumulates a separate `transcript`, merged with `content` only when an Action runs.

### The "Actions" engine — the crown jewel (`src/stores/actionProcessingStore.ts`)
This is the highest-value, lowest-infra thing to steal. An **Action** is just
`{name, description, prompt, icon}` — the `prompt` is a user-editable **instruction fragment**,
not a full system prompt. When fired (`ActionPicker.tsx`, a split button — left half re-runs the
last-used action):

1. **Assemble input** (`PersonalNotesView.tsx:970-1017`): concatenate typed `content` +
   `## Meeting Transcript\n` + the transcript formatted as `You:` / `Them:` lines.
2. **Pick the base system prompt by note kind**: `BASE_SYSTEM_PROMPT` vs `MEETING_SYSTEM_PROMPT`
   (`actionProcessingStore.ts:56-85`). These are strict format contracts — "output clean
   markdown, no preamble/title, don't guess names"; the meeting one emits
   `## Key Discussion Points / ## Decisions Made / ## Action Items / ## Follow-ups` with
   `- [ ]` checkboxes attributed to You/Them.
3. **Final system prompt = `basePrompt + action.prompt`** (+ dictionary/language suffix). The
   app owns the format rules; the user's fragment is appended after, so the base rules win.
4. **One LLM call**: `reasoningService.processText(content, modelId, null, {systemPrompt,
   temperature: 0.3, provider})`. Model comes from the `noteFormatting` scope
   (`inferenceScopes.ts:44-56`, falls back to `dictationCleanup`).
5. **Persist to `enhanced_content`** + `enhancement_prompt` + `enhanced_at_content_hash`
   (a cheap `len + first-50-chars` hash used only to show a "stale" dot when `content` later
   changes). Raw `content` is never overwritten; UI auto-switches to the Enhanced tab.
   Per-note `idle/processing/success` state lives in a Zustand map **keyed by note id**, so it
   survives note-switching and unmounts.

**Insight:** personal-note enhancement and meeting-note generation are the *same* pipeline with
a different base prompt selected by `isMeetingNote`. It's ~150 lines of orchestration + one
text-in/text-out call.

### Meeting-notes flow (`src/stores/meetingRecordingStore.ts`)
Record → capture mic (24 kHz AudioContext + inline AudioWorklet → Int16 PCM chunks) + system
audio (native WASAPI loopback on Windows, else `getDisplayMedia` loopback) → stream PCM to a
transcription provider (local whisper/Parakeet or a cloud realtime provider) → fold
`partial/final/retract` segments into an ordered `segments[]` with `mic`/`system` source →
render as You/Them chat bubbles (`MeetingTranscriptChat.tsx`) → persist `transcript` every 30 s
+ on stop → after stop, run the Actions engine with `MEETING_SYSTEM_PROMPT` → structured meeting
notes in `enhanced_content`.

### Port to Yap

| Piece | Match / Skip | Effort | New plumbing |
|---|---|:---:|---|
| **Actions engine** | **Match — do first** | **S–M** | ~150-line `actions` module on top of `llm.rs`. Store actions as rows/JSON; seed "Generate Notes" with their prompt; port `BASE_SYSTEM_PROMPT`/`MEETING_SYSTEM_PROMPT` verbatim as Rust constants; write output to `enhanced_content` + staleness hash. Delivers "AI notepad" value with almost no new infra. |
| **Notes + folders store** | Match | **M** | New `notes.rs` (SQLite via `tauri-plugin-sql` — recommended for FTS/relations — or JSON-per-note). Mirror the schema; **skip** all `cloud_id/client_note_id/sync_status/deleted_at` unless doing cloud sync. |
| **Markdown editor** | Match | **M** | TipTap is React-only → use **Milkdown** (ProseMirror, Svelte-friendly), CodeMirror 6, or framework-agnostic Tiptap mounted in a Svelte action. Must keep: headings, lists, **task-list checkboxes**, placeholder, markdown get/set. Store markdown strings exactly as they do. |
| **Raw/Enhanced dual-view + staleness dot** | Match | **S** | Two tabs over `content`/`enhanced_content`; recompute `len+first50` hash to flag stale. |
| **Custom-actions manager UI** | Match | **S** | Svelte dialog over the actions CRUD; block deleting built-ins. |
| **Background action state (survives navigation)** | Match | **S** | Run the LLM call in an async Tauri command; hold a Svelte store keyed by note id; emit on completion. |
| **Meeting recording → transcript** | Match, **align with Phase 6** | **L** | Directly overlaps Yap's planned WASAPI-loopback + chunked meeting work. Capture mic + loopback in Rust, chunk, feed `transcribe-rs`, accumulate `TranscriptSegment`, persist to `notes.transcript`. Do **local chunked** transcription (offline) instead of their realtime-cloud WebSocket. |
| **Meeting-notes enhancement** | Match (free once above exist) | **S** | Same Actions pipeline + `MEETING_SYSTEM_PROMPT`. |
| **Speaker diarization / voice profiles** | **Skip v1** | **L** | For v1 tag mic = "You", loopback = "Them" (their `oneOnOneAttendee` fast-path does ~this). |
| Embedded per-note "Ask" chat, Google Calendar, sharing/cloud sync, note-file mirror, meeting auto-detection | **Skip** | L each | Orthogonal to the notepad core. |

**Build order:** Actions engine (S–M, useful for typed notes alone) → notes/folders + editor (M)
→ fold in Phase-6 meeting recording writing to `transcript`, then reuse the Actions engine for
meeting notes (L, shared with existing roadmap).

---

## 3. AI Chat — "chat that knows your meetings"

A text/voice assistant grounded in the user's own notes/transcripts, in **three surfaces sharing
one engine** ([openwhispr.com/ai-chat](https://openwhispr.com/ai-chat)):
- **AI Chat view** (`chat/ChatView.tsx`) — full two-pane page (conversation sidebar + streaming
  thread); conversations persist in SQLite.
- **Agent overlay** (`AgentOverlay.tsx`) — small always-on-top window on a dedicated hotkey;
  **voice-first** (speak → transcribe, skipping cleanup → same engine).
- **Embedded note chat** (`useEmbeddedChat.ts`) — pinned to one note, injects that note's
  content into the system prompt.

All three call the same `useChatStreaming` (agent loop) + `useChatPersistence` (SQLite). The
difference between "AI Chat" and "the agent overlay" is windowing + input modality, not logic.
Meetings are just notes with `note_type="meeting"`, so the same retrieval covers them.

### The agent loop (`chat/useChatStreaming.ts` → `services/ReasoningService.ts`)
1. Resolve provider/mode; decide `supportsTools` (cloud always; local GGUF only if est. params
   ≥ 4B — a regex on the model id).
2. **Eager RAG (`buildRAGContext`, `:20-39`)** — *before the LLM is called*, run
   `semanticSearchNotes(userText, 5)`, fetch each note, inline up to 5 × 500-char snippets as
   `<note id title>…</note>` into the system prompt. So the agent gets relevant notes injected
   **and** can additionally call `search_notes` itself.
3. Assemble system prompt (`config/prompts.ts:getAgentSystemPrompt`) + last 20 turns.
4. Stream via one of two paths, both yielding a unified `content|tool_calls|tool_result|done`
   chunk: **cloud** (client-side loop re-invokes an NDJSON stream, executing tools locally, up
   to `MAX_TOOL_STEPS=20`) or **BYOK/LAN/local** (Vercel AI SDK `streamText` with
   `stopWhen: stepCountIs(20)`, tools passed as `registry.toAISDKFormat()`).

**Tools** (`services/tools/index.ts`, gated by settings): `search_notes`, `get_note`,
`create_note`, `update_note`, `list_folders`, `copy_to_clipboard` (always); `web_search`
(signed-in); `get_calendar_events` (Google connected). The flagship, `search_notes`
(`searchNotesTool.ts:12-31`):

```json
{
  "name": "search_notes",
  "description": "Search the user's notes using semantic search. Understands meaning and context, not just keywords. Returns matching notes with title, date, relevance score, and a preview of content.",
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "The search query to find relevant notes" },
      "limit": { "type": "number", "description": "Maximum number of results to return (default 5)" }
    },
    "required": ["query"],
    "additionalProperties": false
  }
}
```

Its `execute` runs a **fallback chain**: cloud search → local semantic (hybrid RRF) → FTS5
keyword. Each result trimmed to 500 chars.

### The semantic-search stack (what makes it "know your meetings")
A fully-local hybrid retrieval pipeline across four processes:
- **Embeddings**: `all-MiniLM-L6-v2` (384-dim, cosine), ONNX + tokenizer auto-downloaded
  (`localEmbeddings.js`). Run in an **isolated ONNX utility process** (`workers/onnxWorker.js`)
  so native crashes don't kill the app; client respawns with exponential backoff. Embed =
  tokenize (≤256 tok) → run → **mean-pool + L2-normalize**.
- **Vector store**: **Qdrant** (Rust binary spawned as a child process, ports 6333-6350,
  `qdrantManager.js`); collection `notes` (size 384, cosine), point id = note id.
- **Index-on-write**: after any note create/update, `_asyncVectorUpsert(note)` fires
  fire-and-forget (`ipcHandlers.js:406-412`); delete → `_asyncVectorDelete`. Failures swallowed
  so the DB write never blocks.
- **Query — hybrid FTS5 + vector with RRF** (`db-semantic-search-notes`, `ipcHandlers.js:1100`):
  run FTS5 + Qdrant in parallel (each `limit*2`); drop vector hits with **cosine ≤ 0.3**;
  **RRF merge, K=60** (`score += 1/(60+rank)` across both lists); hydrate top-`limit` from
  SQLite. If Qdrant isn't ready → straight FTS5 fallback.

### Chat UI + persistence
Components: `ChatView` → `ConversationList`/`ConversationItem` + `ChatMessages` → `ChatMessage`
+ `ChatInput`; `toolIcons.ts` maps tool → lucide icon for tool pills. Streaming mutates message
state as chunks arrive; an `AgentState` machine (`idle|listening|transcribing|thinking|
streaming|tool-executing`) drives affordances; streams cancellable via `AbortController`.
Persistence = two SQLite tables (`database.js:236-252`): `agent_conversations` (id, title,
timestamps, `note_id`) + `agent_messages` (conversation_id, role, content, `metadata` JSON
holding tool calls). Conversation created lazily on first message; title = first 50 chars.

### Port to Yap — the big one; be selective

| Piece | Match / Skip | Effort | Notes |
|---|---|:---:|---|
| **Eager RAG injection** | **Match — do first** | **S** | Highest leverage: run a note search before the LLM, inline top-5 snippets into the system prompt. Model-agnostic, works even with the tiny local Qwen (no tool-calling needed). Gives "knows your notes" behavior with *no agent loop at all.* |
| **Chat UI (Svelte)** | Match | **M** | Rebuild `ChatView`/`ChatMessages`/`ChatInput`; the OpenWhispr components are a clean reference for streaming bubbles + tool pills. A pill/overlay version fits a Tauri secondary window. |
| **Chat persistence** | Match | **S–M** | First cut: JSON (`{id,title,messages:[{role,content,metadata}]}`) mirroring the 2-table schema. Add SQLite only if you want FTS later. |
| **Agent / tool-calling loop over `llm.rs`** | Match | **M–L** | Port `processTextStreamingAI` to Rust: OpenAI-compatible `/chat/completions` with `tools`, stream SSE, detect `tool_calls`, execute, append `assistant(tool_call)`+`tool(result)`, re-loop ≤20. **Skip** the AI-SDK dependency and the OpenWhispr-cloud NDJSON path. **Gate tools on model capability** (mirror their ≥4B heuristic — the bundled llamafile Qwen2.5-1.5B can't reliably tool-call; disable tools/agent for it, fall back to plain chat). |
| **`search_notes` — keyword-only** | **Match** | **S–M** | Cheapest "knows your notes": implement `search_notes` as FTS5 (or BM25/substring over JSON notes). Their own fallback chain proves keyword-only is a legit mode. Recommended starting point. |
| **Full semantic hybrid (embeddings + vector + RRF)** | Later / optional | **L** | To match them: (a) Rust embeddings via **`fastembed-rs`** (bundles MiniLM + tokenizer; no ONNX utility-process needed — Rust won't crash the webview); (b) **skip the Qdrant sidecar** → embedded `sqlite-vec`/`usearch`, in-process, no port management; (c) index-on-write hook in the note save path; (d) the RRF merge (K=60, 0.3 threshold) ports in ~15 lines. Model ~22 MB, downloads once. Arguably overkill for a pill until keyword recall proves insufficient. |
| **Additional tools** (`create_note`/`clipboard`/`web_search`/…) | Optional | S each | Port only what the roadmap wants; `copy_to_clipboard` + `create_note` are trivial Tauri commands. |
| **Voice-first overlay** (AgentOverlay analog) | Match | **M** | Yap already dictates; wire "transcribe → feed transcript into chat engine (bypass cleanup)" in a Tauri overlay window. Ties into the roadmap's parked "agentic voice-command mode." |
| OpenWhispr-cloud backend, enterprise providers, ONNX utility-process isolation, Qdrant sidecar | **Skip** | — | Electron/SaaS-shaped; replace with Rust-native equivalents above. |

**Pragmatic MVP for a lightweight pill:** eager **keyword**-RAG injection + a plain chat loop.
Add the tool-calling agent and semantic vectors as opt-in later phases.

---

## 4. Audio Upload — "transcribe any audio file locally"

Drag a file (or browse) → transcribe with the configured engine → the transcript becomes a
**note** (`source="upload"`) with an auto-generated title, filed in a chosen folder. The body is
saved **verbatim** — no AI cleanup, only an optional AI-generated title.
([openwhispr.com/audio-upload](https://openwhispr.com/audio-upload))

### UI (`src/components/notes/UploadAudioView.tsx`)
State machine `idle|selected|transcribing|complete|error` (+ a no-provider view). Drop zone is a
`role="button"` div with `onDrop`/`onDragOver`; on drop it reads `dataTransfer.files[0]`,
validates the extension, and resolves the **real filesystem path** via
`webUtils.getPathForFile` (`:289`) — everything downstream is **path-based**, never a Blob.
Progress is either real chunk progress ("N of M chunks", cloud-only) or a **fake timer** creeping
to 90 %. Complete view shows a 150-char preview + folder picker + "Open note".

### Pipeline (three modes; the local one is what Yap cares about)
Renderer picks an IPC channel by resolved mode: `transcribe-audio-file` (local),
`transcribe-audio-file-cloud`, or `transcribe-audio-file-byok`.

**Local** (`ipcHandlers.js:1602`): `fs.readFileSync(path)` → full encoded buffer →
`whisperManager.transcribeLocalWhisper(buffer, {...vad})` or
`parakeetManager.transcribeLocalParakeet(buffer)`. Both converge on
`whisperServer.transcribe` (`whisperServer.js:629`), which **always FFmpeg-normalizes**: write
temp file → `convertToWav(-ar 16000 -ac 1 -c:a pcm_s16le)` → POST WAV to whisper-server's
`/inference` (`response_format=json`, 300 s timeout). **No app-level chunking** on the local path
— the whole file goes in one request; whisper.cpp does its own internal windowing.
`parseWhisperResult` flattens segments to a single `text` (timestamps/segments discarded; no
diarization on this path). Only the **cloud** path chunks (>4 MB → FFmpeg 240-second segments,
5-way parallel, real progress events).

### Formats / limits
Extensions: `mp3, wav, m4a, webm, ogg, oga, flac, aac`. Local = **no size limit**; BYOK = 25 MB;
cloud free = 25 MB / Pro = 500 MB. Uploads have their **own** transcription config
(`selectResolvedUploadTranscription`, `settingsStore.ts:1812`) falling back to the dictation
settings — so you can transcribe files with a bigger model than live dictation.

### Port to Yap

The crux: **Yap has no decoder and no file path today** — it only ever sees raw f32 mic samples.
whisper.cpp itself does **not** decode compressed audio (OpenWhispr always FFmpeg-converts
first), so "let the engine decode it" is not an option — Yap must decode before `transcribe-rs`.

| Piece | Match / Skip | Effort | Notes |
|---|---|:---:|---|
| **File-path command + drag-drop + picker** | Match | **S** | Tauri's drag-drop already delivers **filesystem paths** (`onDragDropEvent`) — simpler than Electron's `getPathForFile` dance. `tauri-plugin-dialog` for the picker. |
| **Decode → mono → 16 kHz** | Match, **the real cost** | **M** | **Recommended: `symphonia` (pure-Rust)** decodes mp3/aac/flac/ogg/wav/m4a with no native dep — ideal for Windows-first Tauri; avoids bundling FFmpeg. Decode → downmix mono → resample 16 kHz f32 (`rubato`) → feed the **existing warm `transcribe-rs` engine** exactly like mic samples. Risk = container edge cases (m4a/webm). Alternative: bundle FFmpeg (mirrors OpenWhispr 1:1) — only if Symphonia coverage proves short. |
| **Long-audio chunking** | Match, **simplified** | **S** | Don't copy their mp3-segment-and-reupload scheme. Chunk **in-memory**: slice the 16 kHz f32 vector into ~30–120 s windows (small overlap), run each through the warm engine sequentially, concatenate in order, emit `upload_progress {done,total}` after each. No re-encode, no disk I/O. |
| **Uploads get their own model config (fallback to dictation)** | Match | **S** | Add upload model-override fields to `config.rs`. |
| **Result → note + optional AI title** | Match | **S** | Save verbatim; optional title via `llm.rs`. (Needs the notes store from §2 — or just drop the transcript into the editor/clipboard if notes aren't built yet.) |
| BYOK/cloud paths, size tiering, upgrade CTAs, fake progress timer | **Skip** | — | SaaS-shaped; Yap emits *real* chunk progress instead. |

**Where it slots in:** `commands.rs` gets `transcribe_file(path, opts)`; a new `decode.rs`
(Symphonia) front-ends the existing `stt.rs`. **Big synergy with Phase 6:** the
decode → mono → 16 kHz → in-memory-chunked-over-warm-engine machinery is exactly what long
meeting recordings need. Build upload first and you get a reusable `decode.rs`, a reusable
chunker + progress contract, and a proven "save transcript as note" sink — so Phase 6 becomes
"point the chunker at a growing capture" rather than net-new work.

---

## 5. Recommended sequencing for Yap

These features layer — build the small, independently-useful pieces first so each ships value
before the next depends on it.

1. **Settings-UX patterns (#1–#3, #7 from the TL;DR)** — scope-driven AI editor, radio-list mode
   selector, hotkey-capture input, container-query rows. Pure front-end + config; no new
   surface. *Makes the app Yap already has feel like OpenWhispr's.*
2. **Actions engine** on `llm.rs` (§2 crown jewel) — works on typed text immediately, even
   before a full notes UI (run it over the last dictation, or a scratch buffer).
3. **Audio Upload** (§4) — its `decode.rs` + in-memory chunker are the foundation for Phase 6.
4. **Notes + folders + markdown editor** (§2) — the notepad surface; Upload results and dictation
   land here.
5. **Meeting recording → `transcript` → meeting-notes Action** (§2 + roadmap Phase 6) — reuses
   #3's chunker and #2's Actions engine.
6. **AI Chat**, escalating: eager keyword-RAG (§3, S) → tool-calling agent (M–L) → semantic
   vectors via `fastembed-rs`+`sqlite-vec` (L, optional).

**Strategic caveat:** items 2–6 expand Yap from a **dictation pill** into a **pill + notes/
meetings surface** — a real scope decision, not just more polish. Item 1 is unambiguously worth
doing regardless. See [`ROADMAP.md`](../ROADMAP.md) for where each lands.

---

## Appendix — OpenWhispr file index (for future spelunking)

Paths are in the cloned Electron repo (`E:\Projects\references\openwhispr`).

**Settings:** `components/SettingsModal.tsx` · `components/SettingsPage.tsx` ·
`components/ui/SidebarModal.tsx` · `components/settings/InferenceConfigEditor.tsx` ·
`config/inferenceScopes.ts` · `stores/settingsStore.ts` (`selectResolvedLLMConfig` `:1861`) ·
`components/ui/SettingsSection.tsx` (`InferenceModeSelector` `:141`) ·
`components/ui/HotkeyInput.tsx` · `components/ui/ProviderTabs.tsx` ·
`components/ui/PromptStudio.tsx` · `components/ui/PermissionCard.tsx`.

**Notepad:** `helpers/database.js` (`notes` `:104`, `folders` `:181`, `actions` `:216`,
built-in seed `:283`) · `components/ui/RichTextEditor.tsx` · `components/notes/NoteEditor.tsx` ·
`components/notes/PersonalNotesView.tsx` (action input assembly `:970`) ·
`stores/actionProcessingStore.ts` (engine; prompts `:56-85`) ·
`components/notes/ActionPicker.tsx` · `components/notes/ActionManagerDialog.tsx` ·
`stores/meetingRecordingStore.ts` · `components/notes/MeetingTranscriptChat.tsx`.

**Chat:** `components/chat/ChatView.tsx` · `components/chat/useChatStreaming.ts` (RAG `:20`) ·
`components/chat/useChatPersistence.ts` · `components/AgentOverlay.tsx` ·
`services/ReasoningService.ts` · `services/tools/searchNotesTool.ts` (schema `:12`) ·
`services/tools/index.ts` · `helpers/localEmbeddings.js` · `helpers/qdrantManager.js` ·
`helpers/vectorIndex.js` · `workers/onnxWorker.js` · `helpers/ipcHandlers.js`
(`db-semantic-search-notes` `:1100`) · `helpers/database.js` (`agent_conversations` `:236`).

**Upload:** `components/notes/UploadAudioView.tsx` · `components/settings/UploadSettings.tsx` ·
`helpers/ipcHandlers.js` (`transcribe-audio-file` `:1602`, `chunkedCloudTranscribe` `:198`) ·
`helpers/ffmpegUtils.js` (`convertToWav`, `splitAudioFile`) · `helpers/whisperServer.js`
(`transcribe` `:629`) · `stores/settingsStore.ts` (`selectResolvedUploadTranscription` `:1812`).
