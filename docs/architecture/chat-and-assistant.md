# Chat and the Voice Assistant

The AI Chat surface (conversations, the tool-calling loop and its note tools) and the Voice Assistant that asks it out loud. Web search and opening apps are [web-and-apps.md](./web-and-apps.md); reading answers aloud is [spoken-replies.md](./spoken-replies.md); the models are [ai-models.md](./ai-models.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`chats.rs`** — the AI Chat surface's conversation store (`chats.json`): a
  `Conversation` (title, messages, timestamps) with `list`/`get`/`create`/
  `append`/`delete`/`rename` (`chat_rename`: one trimmed line ≤ 80 chars,
  `updated_ts` untouched so the list order stays; unit tests use an
  in-memory store per thread, never `chats.json`); `chat_send` (commands.rs)
  creates a conversation on the first message using a first-50-chars title
  rule. An answer keeps its tool
  chips (`ChatMessage.tools`, only written when there are some), so a
  conversation opened later still shows what the AI looked up and changed,
  and the model that wrote it (`ChatMessage.model` = `ChatModel {provider,
  id, name?, onThisPc?}`, only when known; older answers have none). A
  question that was dictated carries `voice: true` ("Only when I asked by
  voice" reads its answer out); `append` returns the message's index, which
  the pages use as its id (`chat:<conversation>:<index>`). A web answer keeps
  its sources (`ChatMessage.sources` = `Source {n, title, url, site}`,
  numbered as it cites them; `append_sourced`; only written when there are
  some). **A citation with no source behind it** is taken out before the
  answer is saved (`strip_unsourced_citations`, in `chat_turn`: a small model
  answering from memory wrote "Python 3.12.0 [1]" with no search): kept are
  the numbers of this turn's sources, else of the latest earlier answer that
  had some (`earlier_source_numbers`, what ChatView's `citedSources` draws a
  follow-up's "[1]" against); code, a link's text and "[0]" are left alone.
- **`tools.rs`** — the AI Chat tool-calling agent loop: six tools ported
  near-verbatim from OpenWhispr's `services/tools/*` (`search_notes`,
  `get_note`, `create_note`, `update_note`, `list_folders`,
  `copy_to_clipboard` — executed locally over `notes.rs`/`arboard`) and
  three of Yap's own for **changing part of a note safely** (after the note
  tools of Wispr Flow's live Ask — behaviour only; the pure part is
  **`note_text.rs`**): `search_in_note(note_id, query, field?)` (the lines a
  phrase is on, numbered `12| …`, the note's own text, the lines around the
  first 5 hits; forgiving — case, whitespace runs, curly quotes, dashes; in
  content, the `enhanced` summary and the transcript (talk only, `[m:ss]
  You: …`) unless one is named; ≤ 20 hits / ~4k chars), `read_note(note_id,
  field, offset?, limit?)` (a page of ≤ 400 lines and ≤ 3,000 tokens — the
  bound `get_note` puts on a meeting via `ask_context` — with `more` /
  `next_offset`; a line longer than a page is cut and says so) and
  `edit_note(note_id, field: content|enhanced|title, old_text, new_text)`:
  `old_text` must appear **exactly once** (overlapping counts), byte for
  byte except that `\r\n` = `\n` (the new text takes the note's own line
  endings); 0 matches → nothing changes and the error lists the places that
  differ only in spacing/case/quote marks, quoted from the note (`near`),
  and spots copied line numbers; 2+ → the lines they're on; `old_text` ""
  only fills an empty field; the transcript is never edited. Bookkeeping: a
  new title clears `title_auto`, a content edit leaves the summary stale
  (`enhanced_at_hash` untouched), a summary written into an empty field is
  stamped fresh, one emptied is no summary. Matched and replaced under the
  store lock (`notes::modify`). `update_note` stays for folder moves and
  whole rewrites the user asks for (its description and `TOOL_INSTRUCTIONS`
  say so; the instructions steer search → `edit_note` for everything else);
  `get_note` returns the notes (`content`) and the summary (`enhanced`)
  apart, each ≤ 2,000 tokens (`cut` names what was shortened). Failures
  carry `error` (for the model) besides `displayText` (the chip).
  `run_tool_loop` drives the OpenAI tool-call protocol via
  `llm::post_chat_message` for up to `MAX_TOOL_STEPS` (20) turns, takes
  arguments as a JSON string or an object (ids as numbers or strings), and
  hands every note a tool created or changed (`changed_note`; an edit that
  changed nothing doesn't count) to `on_change` as it happens — `chat_send`
  emits `yap-note-changed` (origin `"chat"`) + `yap-notes-changed`, so the
  notepad and the lists follow live. Chips say what happened: `Found
  "beta" on 2 lines in "Launch plan"`, `Edited "Launch plan": "Friday." →
  "Monday."`, `Couldn't edit "Launch plan": the text is in 2 places`. Gated
  by `supports_tools` — cloud providers always qualify (unless the
  provider's own model list says that model can't call tools,
  `model_catalog::tools_for`), local models by
  `local_model_uses_tools`: a family the model evaluations saw fail never
  (`NO_TOOL_FAMILIES`: Llama 3.2, which loops), one they saw succeed from its
  size (`TOOL_FAMILIES`: Qwen3.5 ≥ 4B, thinking off), else ≥4B params
  (`LOCAL_TOOL_MIN_PARAMS_B`; the size read from `-4B`, `_4B`, `:8b`) — so
  smaller local models fall back to plain keyword-RAG chat instead. **A tool
  call the provider rejects** (Groq's HTTP 400 `tool_use_failed`: gpt-oss
  left out `note_id`, or called a tool not in the request; llama.cpp's 500
  "Failed to parse input" for a call written as text — `llm::rejected_tool_call`)
  never ends the turn in an error: the model is told what was wrong (a
  user-role note from Yap, with what it sent) and asked again once
  (`REJECTED_CALL_RETRIES`), then answers without tools (`ANSWER_NOW_NOTE`, as
  when out of steps). A web tool past its per-turn cap stays offered (`web::
  Turn` answers "you've searched 3 times"): taking it away made Groq 400 the
  next call to it. Unit-tested end to end against
  `notes.rs`' **in-memory test store** (under `cfg(test)` every test thread
  gets its own empty store and nothing reads or writes `notes.json`), and
  in `e2e/chat-tools.spec.js` against the scripted fake AI. **Folders are
  Yap's own rule** (a deliberate change from OpenWhispr, whose
  `create_note`/`update_note` made any folder the model named — a chat once
  left an empty "Camping" folder behind): `note_folder` files a note into a
  folder that exists, by any case (as the list spells it); no folder = Personal
  (a new note) / no move; a new folder only with `new_folder: true`, which the
  tool descriptions and `TOOL_INSTRUCTIONS` keep for a user asking for one
  ("Never make up a folder"). An unknown name without it changes nothing
  (an `update_note`'s title included) and tells the model the folders there
  are and to leave `folder` empty; chip `Couldn't create "Tent list": no
  folder called "Camping"`. (The MCP server's `create_note` still makes the
  folder it's given.) Plus **`open_app(name)` / `list_apps(query)`**
  (`app_definitions` + `APP_TOOL_INSTRUCTIONS`, offered while
  `open_apps` is on; run by `apps.rs` off the async runtime, with the
  answer just before the question for its consent check). Every step
  re-sends the instructions + schemas, so they're kept short
  (`a_tool_step_stays_light`: ≤ 2,250 est. tokens; 2,879 before the
  2026-10-18 trim, ~2,180 after). Plus **`web_search(query)` /
  `read_page(url, offset?)`** (`web_definitions` + `WEB_TOOL_INSTRUCTIONS`,
  ~470 tokens more, `the_web_tools_stay_light`; offered while "Look things
  up on the web" is on; run by `web.rs`'s per-turn `web::Turn`, passed as
  `run_tool_loop`'s last argument). **The safety rule**
  (`refused_after_web`, `ACTION_TOOLS` = open_app, create_note,
  update_note, edit_note, copy_to_clipboard): once web content is in a turn
  (`Turn::seen`), those are no longer offered (`without_actions`) and a call
  to one is refused with a reason for the model (chip `Didn't open an app:
  not after reading the web`); the person's next message is a clean turn.
  Unit-tested through the real loop against a local fake model + Exa
  (`a_web_page_can_never_make_the_chat_act`).
- **`assistant.rs`** — the **Voice Assistant** (2026-10-18; Settings →
  Voice Assistant): ask Yap's Chat out loud from any app with its own
  shortcut (`assistant_hotkey`, unbound by default) and hear the answer;
  it never takes the focus or types. Not **Commands** (the agent's name in a
  dictation, the rewrite shortcut: the AI types for you — Wispr's Command
  Mode and old "Hey Flow"; Wispr has no spoken-answer assistant). **Off
  during meetings** (Nathan: Bella talking over a meeting "would be too
  much"): while a meeting records or a call is on (`tts::meeting_quiet` =
  `meeting::is_recording` || `meeting_detect::call_live`), a press doesn't
  listen — `off_for_meeting` shows "The Voice Assistant is off during
  meetings" (alert `assistantMeetingOff`, over the meeting, every press) —
  and nothing in Yap talks: `tts::say` refuses, Chat's auto-speak stays
  quiet. The question is transcribed with the correction dictionary (exact
  + near-miss, as a dictation), never the AI cleanup pass (the Chat model
  reads it as asked; cleanup would add delay), snippets or the voice
  command. Phases
  `Idle → Listening → Thinking → Speaking` in `yap-assistant {phase,
  conversationId, messageId}` (the bar). `listening` (from
  `capture_target`; bumps a generation, drops the last answer's card),
  `heard(question)` (after the STT + dictionary) → `answer`: the
  conversation chosen first, then "Open Teams" — or "Yes." to Yap's "Did
  you mean …?" — needs no model (`apps::plan_for`, below: opened,
  "Opening Microsoft Teams." said); the turn goes through `chat_turn` with
  scope "assistant" (8 history messages, not 19); otherwise no Chat model
  (`commands::resolve_chat_endpoint` errs) → the `assistantNoModel` alert
  with **Open settings** (`alert::OPEN_AI_SETTINGS` → Language Models →
  Chat & Voice Assistant; a model that fails → `assistantFailed`, the
  reason + "Your question is in Chat."); else **`choose_conversation(viewing,
  open_exists, latest, now)`** (pure): the conversation open in the Chat
  view (`chat_view_state {open, conversationId, newChat}`: ChatView
  reports it), a new one if "New chat" was clicked there, else the last
  updated if under `RECENT_SECS` (30 min) — so pressing again asks a
  follow-up — else a new one; then `commands::chat_turn` (the Chat view's
  own turn: RAG, tools, `voice: true`), `yap-assistant-turn {phase:
  asked|answered|failed|open, conversationId, show}` for the main window
  (`show` = the window was in front: ControlPanel brings Chat up on it).
  **`reply(meeting, interrupted)`** (pure): always **spoken** (whatever
  "Speak replies" / "When to speak" say), except when a meeting or call
  started while the answer was coming, or a dictation did → shown only; a
  newer question or ■ (the generation) → not said at all (it's in Chat).
  While the main window isn't in front (`notice::place` = Bar) a card
  (`style: "answer"`, id `assistant`: the question, the answer's markdown
  ≤ 1,200 chars, a note "Not read aloud during a meeting", Open in Chat;
  ✕ / Esc = `stop`) — **"Show the answer on the Yap bar"**
  (`assistant_answer_card`, on) decides it for a spoken answer; one not
  read aloud always shows (`card_on_bar`, pure), also when the voice
  fails —, shown before the voice starts; it goes when the
  voice ends (`on_tts_state`), or after `read_ms` when shown only. **A web
  answer** (`turn.sources`, `web.rs`) is said short (`spoken_answer`:
  `tts::brief` — its first two sentences, no `[n]` — then "The details and
  sources are in Chat."), its card shows that and "2 sources in Chat"; the
  model is told a spoken question wants the answer first. **Any other
  spoken answer** is asked for short (`SPOKEN_ANSWER_NOTE`, last in Chat's
  system prompt for the Voice Assistant, or a question dictated into Chat
  with spoken replies on: "one or two short sentences (under 40 words) of
  plain speech…" — the model evaluations' egg answer was 55–98 words with
  lists before, 12–17 after), and one longer than `SPOKEN_MAX_WORDS` (60)
  anyway is said short too, "The rest is in Chat." `stop`
  (■ on the capsule = `bar_action "assistant-stop"`, `assistant_stop`).
  The bar's capsule shows while `phase != Idle`, the bar on or off.
  Debug-only `e2e_assistant_ask {text, listenMs}` (`Robot::ask_assistant`:
  the press's own steps — barge-in, the target, "recording" for
  `listenMs` — with `text` as the transcript).

## The Chat view

- **`ChatView.svelte`** = the
  AI Chat surface (OpenWhispr `chat/ChatView.tsx` port): conversation sidebar
  (Today/Yesterday/Previous 7 Days/Older grouping, Ctrl+N, hover-delete;
  **several at once**: Shift+click a range, Ctrl+click one, Ctrl+A in the
  list → a "3 selected · Delete · Cancel" bar, Delete key / Esc; the list's
  ⋯ menu has Select all and "Delete all chats…", every delete asked first;
  `chat_delete_many` = one write, `chats::delete_many`) +
  thread; `chat_send` answers via the Chat scope with **eager keyword-RAG**
  over the notes library (top-5 `<note>` snippets; `chats.rs` persists
  conversations to `chats.json`) **plus the tool-calling agent loop**
  (`tools.rs`: search_notes/get_note/create_note/update_note/list_folders/
  copy_to_clipboard, plus search_in_note/read_note/edit_note for changing
  part of a note, executed locally, ≤20-step loop over the OpenAI tool
  protocol, gated to cloud or ≥4B local models — smaller models fall back to
  plain RAG chat; tool-activity chips render in the thread above the answer
  and are kept with it in chats.json; a question from
  the Meetings view's Ask bar arrives through `chatRequest` and is sent with
  `scope: 'meetings'`). **A web answer** (`web.rs`): its `[1]` citations are
  small pills that open their source (`renderMarkdown(md, {sources})` in
  `markdown.js`, where only the sources' links stay links — any other
  address the model wrote is plain text), a **Sources** row under it
  (number, title, site; `openExternalLink`), kept in chats.json; web chips
  carry a globe, a refused action (the safety rule) a shield. The composer (2026-10-20, after Claude
  Code's: one row — **+** (a "Chat options" popover: the Voice replies switch,
  the voice volume slider, Voice settings…), the message box (aria-label
  "Message"), the mic, and Send only once there's text; an amber dot on the +
  while voice replies are on; under the box on the left, quiet, the **model
  chip** (its `quiet` look; never inside +: always in view) — `ModelChip.svelte`, names in `aiModel.js` from
  `ppModels.js` + `providerIcons.js`) shows the model the next message goes
  to — the provider's mark (`ProviderMark.svelte`), "GPT-OSS 20B · Groq" /
  "Qwen3.5 4B · On this PC", "· no note tools" for a local model under 4B,
  where it runs in its tooltip (and, on the on-device model, "Context: 8K
  tokens": `ai_model_status.context`), "No AI model" when nothing is set up. The
  chip opens **the Chat model picker** (`ChatModelPicker.svelte`, data in
  `chatModels.svelte.js`; Nathan's "Concept A, provider rail",
  2026-10-19): a popover above the composer (`data-model-picker-above`;
  560 px, its rail a strip across the top below 520) — **search** across
  the top ("Search models and providers", Ctrl+K, typing anywhere searches,
  Enter picks the first result) over every provider's models, connected or
  not, and this PC's; a **rail** (`tablist`): Recent (the last 3 Chat models
  used, this webview's localStorage `yapChatRecentModels`), Groq / Gemini /
  Claude (Anthropic) / OpenAI / OpenRouter with their marks and a dot
  (green = a key, hollow = not set up), Custom when a server elsewhere is
  set up, and **On this PC**; it opens on the current model's provider (or
  the one the scope is set to). The **pane**: the provider's mark on a
  tile, its name, a status line from the live lists ("Connected · checked
  2 h ago", "Couldn't check: …", "Not set up"), the key masked + Change
  key; the rows are exactly Settings' list (`modelCatalog.js`'s merge:
  curated, then "New", the rest behind "Show N more"), each a button
  (`aria-pressed`) with a ✓ for the current one, the model family's mark
  (GPT-OSS → openai-oss.svg, Qwen, Gemma… `familyIcon`), a one-line
  description and ≤ 2 tags (New, Recommended for this PC, In use, no note
  tools, No longer offered — with "Groq no longer offers GPT-OSS 120B. Pick
  another model." above). A provider **without a key** shows "Connect
  Claude" in place (paste a key · Save key · Get a key ↗ via
  `openExternalLink`; `chat_provider_key`), its curated models greyed with
  "Add key" until then (a click on one goes to the key field); once saved
  its live list is fetched at once and its models are pickable. **On this
  PC** ("Private: nothing leaves this PC" + where it runs: "Runs on your
  graphics card (NVIDIA …)" from `gpu.rs` / the sidecar's placement, or
  the processor), then the on-device model's **context**
  (`LocalContext.svelte` `compact`, shared with Settings; words in
  `localContext.js`): "Context [8K tokens · ≈ 3.7 GB ▾]" — every choice up
  to the model's maximum with what it takes (`local_llm_status().context`,
  `gguf::estimate`), "· may not fit" and a warning line ("More than fits on
  your graphics card (about 13.6 GB to spare): part of it would run on the
  processor, slowly.") past the card's room, "Uses 3.7 GB on the graphics
  card" once the running model's log measured it, the one-line note
  ("Longer context: longer chats and meeting notes fit, more graphics
  memory, a little slower. This model goes up to 256K."), and "Restarting
  the model with the new context…" while `local_llm_set_context`'s restart
  waits for a dictation (`yap-local-llm-changed` re-reads,
  `followLocalChanges`); Settings' processor warning with "Use Qwen2.5 1.5B
  Instruct"; a note when cleanup uses the on-device model too
  (`local_llm_status.sharedOnDevice`: picking another switches it there as
  well); **Installed** (Yap's curated GGUFs on disk and the person's own,
  `local_llm_status.own`; In use / pick to use; each row starts with its
  maximum, "Up to 256K · …", `maxContext`), self-hosted servers on this
  PC (`llm_models`: their `/models`, "3 models on this server", and the
  context Ollama / LM Studio say: "8K context, set in Ollama · …"), and
  **Download** (the rest, "Recommended for this PC" first): a Download
  button with the size runs `local_llm_install` right in the row with a
  progress bar from `local-llm-download-progress` (which now names the
  curated `model`; one at a time, it carries on when the picker closes, no
  cancel — the install path has none), then the row is installed and
  pickable; "Open models folder". The footer: "Changes the model for Chat
  only. Dictation cleanup stays as it is." (or that Chat uses cleanup's
  model until one is picked) + **Language model settings…**
  (`cleanup#chat`). Keyboard: ↑/↓ through the rail (switching as it goes)
  or the rows (roving tab stops), ←/→ or Tab between them, Enter picks, Esc
  closes and gives the focus back to the chip; amber focus rings; the
  window's in-page hotkey fallbacks run first (capture phase). Monochrome
  marks (OpenAI, Anthropic, xAI, OpenRouter — Simple Icons' CC0 glyph) are
  CSS masks in `currentColor`, Claude's in its brand colour. Settings →
  Language Models keeps its own pickers. Each answer
  shows its model quietly under it, as do the Notes view's Ask bar and the
  notepad's Ask sheet (`nextModel(scope)` fetched beside the call; none for
  Yap's own "Nothing new" / "too little"). **Spoken replies** (`tts.rs`):
  **Voice replies** (🔊 / 🔇 pill beside the model chip, `aria-pressed`) is
  Settings' "Speak replies" (`voice_replies_set`; Settings adopts
  `yap-voice-replies`, the toggle re-reads on `yap-config-saved`); every
  answer has **▶ Read aloud** under it (■ Stop reading + "Speaking…" with
  three bars while it's read; also on the Notes view's Ask bar answers and
  the notepad's Ask sheet, not while that meeting records), via the shared
  `speech.svelte.js` store (`speakingId` from `yap-tts-state`; each window
  toasts failures for its own ids: `chat:` / `notes:` / `preview` /
  `notepad:`). With replies on, chat_send's `speak` reads an answer out by
  itself. The **mic** is `chat_dictate`, and the box reports its focus
  (`chat_input_focus`), so a question dictated into it arrives as
  `yap-chat-dictation` (inserted at the caret, never pasted) and is sent at
  once when `send` says so; such questions are sent with `voice: true` (a
  small mic on the bubble). Esc, leaving Chat, another conversation, the
  next question and any dictation starting stop the voice. **The Voice
  Assistant** (`assistant.rs`): the view reports what it shows
  (`chat_view_state`: the conversation, or "New chat" clicked) and follows
  `yap-assistant-turn` live — it switches to the question's conversation,
  the question with its mic, "Thinking…" (`assistantPending`), the answer
  (Rust reads it aloud); ControlPanel brings Chat up for one asked while the
  window is in front (`show`) or "Open in Chat" (`chatRequest {assistant}`).
  An empty chat says "Or press F8 in any app to ask out loud…", or offers
  **Set an assistant shortcut** (→ Settings `voice#assistant-shortcut`).
  No streaming or
  semantic vectors yet (ROADMAP step 3).
