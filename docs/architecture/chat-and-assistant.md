# Chat and the Voice Assistant

The AI Chat surface (conversations, the tool-calling loop and its note tools) and the Voice Assistant that asks it out loud. Web search and opening apps are [web-and-apps.md](./web-and-apps.md); reading answers aloud is [spoken-replies.md](./spoken-replies.md); the models are [ai-models.md](./ai-models.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`chat/chats.rs`** — the AI Chat surface's conversation store (`chats.json`): a
  `Conversation` (title, messages, timestamps) with `list`/`get`/`create`/
  `append`/`delete`/`rename` (`chat_rename`: one trimmed line ≤ 80 chars,
  `updated_ts` untouched so the list order stays; unit tests use an
  in-memory store per thread, never `chats.json`); `chat_send` (`chat/commands.rs`; the turn itself is
  `chat/turn.rs`'s `chat_turn`) creates a conversation on the first message using a first-50-chars title
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
  some). **Every answer is checked before it's shown and saved**
  (`checked_answer`, in `chat_turn`, every model; each fix logged and, in a
  model evaluation, counted — `eval::post_fix`): a sources list or "(Yap
  searched the web for this answer…)" note the model wrote itself goes
  (`web::strip_sources_note`: Yap shows the real sources; a list the
  question asked for — sources for an essay, references, reading — stays,
  and the looser list shapes count only after a web search); a search it claims
  when none ran this turn ("I did look it up on the web",
  `web::claims_browsing`) gets "*Yap didn't search the web for this answer: it
  comes from the AI model's own knowledge, which may be out of date.*" — or
  "for that answer…" when the question asks how the answer before it was made
  (`web::about_the_answer`: "did you look that up or guess?"), in which case
  that answer's own search makes the claim true; and **a citation with no
  source behind it** goes (`strip_unsourced_citations`: a small model
  answering from memory wrote "Python 3.12.0 [1]" with no search): kept are
  the numbers of this turn's sources, else — for a follow-up — of the latest
  earlier answer that had some (`earlier_source_numbers`, what ChatView's
  `citedSources` draws a follow-up's "[1]" against), but not for a new
  question that needs the web (`web::needs_web` and not `web::refers_back`:
  a tester's "best RPG games coming out in 2026" cited [1][4][5] that drew as
  the indie games' sources) nor after a search that found nothing; code, a
  link's text and "[0]" are left alone. Then, for someone outside the US
  (About you's region; 2026-10-22), a price question that searched loses
  any **offer to look for local prices** ("Let me know if you need help
  finding the UK pricing…": Yap looked, and looked again —
  `about_you::strip_price_offer`), and **a price in their money the web
  gave only in another** ("£8" where the pages said "$8") gets its page's
  sign back (`about_you::restore_price_signs`, over the turn's web text);
  and for someone who doesn't pay in
  dollars, **bare dollar prices** — "$8/month" with nothing
  saying whose dollars — get "*Prices in $ are as the sources give them
  (most likely US dollars), not in £.*" (`about_you::foreign_price_note`, in
  `chat_turn`; 2026-10-21). **History**: never a note in an
  answer's text (2026-10-21, the day it went in: Qwen3.5-4B copied "(Yap
  searched the web for this answer. Its sources: …)" word for word, with made-up
  sources, into an answer that searched nothing, then said it had looked it
  up). A tool-calling model with the web on gets each earlier web answer's
  search **replayed** before it (`history_messages`: the `web_search` call
  with its query — read back from the chip, `searched_queries` — and a short
  result: the numbered titles, sites and addresses, no page text, and a note
  to answer follow-ups from the answer after it — told "the text isn't
  kept", Qwen3.5-4B said it couldn't list "the top five in that list" —, or
  to read one of those pages, which `read_page` allows); any other model gets
  one line (in the question's context for a tool model, else the system
  prompt) naming the questions Yap searched the web to answer
  (`web_history_note`). **Every** system prompt ends with the **About you
  block** (ai-models.md: where the person is and their English, prices in
  their currency — or a foreign price said as such —, times, units, their
  name and note) — a tool model's without its clock
  (`about_you::profile_block`), the date and their time with its UTC offset
  going in the question's context (`about_you::clock_line`), a model
  without tools' whole (`prompt_block`); a web answer's tool lines say to convert
  a page's time zone to theirs (`web::time_note`), so a page's "3:00 PM ET"
  is converted, not mislabelled, and "$8/month" isn't passed off as the UK
  price. **The prompt's two parts** (2026-10-22, for the model's cache), for
  a model that calls tools (Qwen3.5-4B, the cloud models): the system prompt
  holds only what stays the same from turn to turn — the persona, the
  instructions, `chats::CONTEXT_NOTE`, About you without the clock — and
  everything that changes with the question goes in a `<context>` block in
  the question's own message (`chats::with_context`): the clock, the notes
  the eager RAG found (`rag_context` under `RAG_LEAD`, strict:
  `tools::rag_notes`, only notes matching two of the question's words or one
  in their title, words like "what" and "and" left out — next to the
  question, a note that only shared "time" pulled a follow-up off the
  conversation), the Meetings view's meetings, the earlier-searches line,
  the spoken-answer notes. Only the latest question carries it (the history
  keeps what was typed). A hybrid local model (Qwen3.5) resumes its cache
  only from a checkpoint, so a system prompt that changed every turn made
  every Chat request read the conversation again; now a turn reads its own
  message (model-evals.md "Local AI, phase 1"); cloud providers' prefix
  caches like a stable start too. **A model without tools** (Qwen2.5-1.5B)
  keeps it all in the system prompt, the loose RAG and the whole About you
  block with the clock, as before: it's dense (re-reads only from the first
  change) and answered no better with the notes next to the question. The
  Notes view's Ask bar (`note_ask`) does the same by the same rule: the note
  in the system prompt, the clock with the question for a tool model.
  **Memory** ([memory.md](./memory.md)): memory's "You" block ends the
  system prompt after About you (stable); the people, projects and topics a
  question is about, and the person's answer preferences, go in its
  `<context>` (a model without tools: its system prompt); a tool model gets
  `remember` / `forget` (only when the person's own message asks) and, with
  "Search and reference past chats", `search_chats` (`tools::Extra`,
  `run_tool_loop_with`); a model without tools gets "Remember that…" done
  by Yap (`memory::learn::spoken`) and a past chat's lines when the question
  points back at one (`chats::refers_to_past`). Every turn tells memory's
  scheduler (`memory::learn::after_turn`): the chat is read for lasting
  facts once it's gone quiet.
  **Screenshots** (Snip to Chat, [snip.md](./snip.md)): a question may carry
  up to 4 (`ChatMessage.images` = `ChatImage {id, w, h, bytes, source}`,
  only written when there are some — the PNGs are files in
  `<data>/chat-images/`, deleted with their conversation by `delete_many`;
  `append_question`; `image_ids` for the sweep; `for_page` marks one past
  "Keep chat screenshots" `gone`). `chat_send(images)` checks the model reads
  them (`snip::vision`) and runs the turn with them (`snip::turn`): each
  model call's question becomes a content array (the image parts, then the
  text; the text first for OpenRouter), the newest 3 earlier screenshots in
  the history window go again, older ones as a line; a server's refusal
  becomes `vision::COULDNT_READ` (that model a "no" this session).
  **The history window** moves three turns at a time
  (`chats::history_start`: 14–19 messages, the Voice Assistant's 5–8), not
  every turn, so a long chat keeps its prompt's start.
- **`chat/tools.rs`** — the AI Chat tool-calling agent loop: six tools ported
  near-verbatim from OpenWhispr's `services/tools/*` (`search_notes`,
  `get_note`, `create_note`, `update_note`, `list_folders`,
  `copy_to_clipboard` — executed locally over `notes.rs`/`arboard`) and
  three of Yap's own for **changing part of a note safely** (after the note
  tools of Wispr Flow's live Ask — behaviour only; the pure part is
  **`chat/note_text.rs`**): `search_in_note(note_id, query, field?)` (the lines a
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
  `run_tool_loop`'s `web` argument). `run_tool_loop` takes the history as
  OpenAI messages (`chats::history_messages`: earlier web answers' searches
  replayed), the question's context (`chats::with_context`) and a
  **`FirstStep`**: `Model` (it decides), `MustSearch` (a model on this PC
  asked something that needs the web — `tool_choice: "required"`
  (`must_search_choice`; a named choice is ignored by llama.cpp) over all
  the tools, and `MUST_SEARCH_NOTE` in the question's context: search this
  message, not earlier messages' words; no `web_search` made (another
  tool, or words), or the server refuses →
  Yap's own search for it, `search_for_the_model`; **the step's words are
  never the answer** (2026-10-23): llama.cpp lets Qwen3.5 write before its
  forced call, and the Voice Assistant said "I can't provide weather
  information…" before any search (a tester's voice chat), so the step
  streams through `stream::ChipsOnly` (its chip only, no text to the page
  or the voice), its words are left out of the call kept in the
  conversation, and it's held to `MUST_SEARCH_TOKENS` (128) — the grammar
  won't let it stop without a call, and one wrote on for 1,961 tokens;
  until 2026-10-22 only
  `web_search` was offered, so the next step's tools differed and a local
  model read the whole prompt again) or `YapSearches(query)`
  (Yap searches before the first step; the model evaluations' comparison,
  `YAP_EVAL_FIRST`). **Nothing in the notes** (`EmptyNotes`, 2026-10-21):
  with the web on and a question not about the person's own things
  (`web::about_own_stuff`), an empty `search_notes` result carries
  `NOTHING_IN_NOTES_HINT` (search the web next), and for a model on this PC
  a step of only empty notes searches is followed by Yap's search
  (`search_for_the_model`, once a turn; `YAP_EVAL_EMPTY_NOTES` = hint |
  search | none compares). **The safety rule**
  (`refused_after_web`, `ACTION_TOOLS` = open_app, create_note,
  update_note, edit_note, copy_to_clipboard): once web content is in a turn
  (`Turn::seen`), a call to one is refused with a reason for the model (chip
  `Didn't open an app: not after reading the web`) — they stay offered:
  **every step of a turn offers the same tools and system prompt**
  (2026-10-22; taking them away changed the prompt's start, and a local
  model read it all again); the person's next message is a clean turn.
  Unit-tested through the real loop against a local fake model + Exa
  (`a_web_page_can_never_make_the_chat_act`).
- **`chat/stream.rs`** — **streaming answers** (2026-10-22): every model call
  of a Chat turn — each step of the tool loop, a small model's plain
  answer, the loop's last no-tools call — the notepad's Ask sheet and the
  Voice Assistant show the answer as it's written. On the on-device
  Qwen3.5-4B the first words of a long answer came after ~0.6 s instead of
  ~2.6 s (model-evals.md "Streaming answers"). **The wire**: OpenAI's
  `stream: true` server-sent events, with `stream_options: {include_usage:
  true}` (`llm::post_chat_streamed`, the same function as every call,
  `post_chat_request`, so the retries, rate-limit waits, temperature and
  reasoning fallbacks and the timeout are today's — the timeout covers the
  whole stream, as it covered the whole answer). `SseParser` (lines split
  anywhere, even mid-character; `:` comments such as OpenRouter's `:
  OPENROUTER PROCESSING`; a bare JSON line) and `Assembler` (content
  deltas; a tool call as a header — index, id, name — then argument
  fragments, or whole and without an `index` as Gemini's OpenAI-compatible
  endpoint sends it, or arguments as an object; usage from `usage` or
  Groq's `x_groq.usage`; llama.cpp's `timings`; the message rebuilt as a
  non-streamed one, every tool call's arguments **checked as JSON** — not
  JSON is a rejected call, `llm::TOOL_CALL_REJECTED`, which the loop asks
  again, as llama.cpp's 500 for it). The providers as Yap talks to them:
  the sidecar and other llama.cpp servers, Ollama and LM Studio (custom,
  self-hosted), Groq, OpenAI, OpenRouter, and Anthropic's and Gemini's
  OpenAI-compatible endpoints — all the same shape. **Falling back**, per
  endpoint (`llm::streams_to`): a server that refuses `stream_options`
  (400/422 naming it) is asked again without, and never with them again
  this session; one that refuses `stream` itself ("stream"/"streaming" as
  a word in a 400/422 — not "upstream") is asked whole and never streamed
  to again this session; one that answers whole anyway (a JSON body) is
  read whole; **a stream that breaks** (the connection drops, an `error`
  object mid-stream — OpenRouter's, llama.cpp's —, no finish reason or
  `[DONE]`) is asked again whole at once (the page is told to drop what
  came, `Delta::Restart`), and an endpoint whose streams broke twice isn't
  streamed again this session. Groq's `tool_use_failed` mid-stream is a
  rejected tool call, as its HTTP 400 is. **A step** (`Step`): with tools
  offered, a step's opening text is **held until it's an answer** —
  `releases`: a sentence's end (`. ! ? :` + space, or a line break)
  followed by more text, or 200 characters — so the line a model writes
  before a tool call ("I'll search your notes for that.") never shows; a
  tool-call delta turns the step into its chip (`Event::Tool`, "Searching
  the web…" from `doing`), and text shown before it is taken back (`text`
  "", on the page — and a Voice Assistant already saying it stops,
  `Streamed::taken_back` below, 2026-10-25); without
  tools (a small model, the last call) text shows at once. **A search
  that must come first shows no text at all** (`ChipsOnly`, a sink around
  the watcher that lets everything but `text` through; 2026-10-23): its
  words are never the answer, and a long one — "I can't provide weather
  information for Kellbrook, as I don't have access to real-time weather
  data…" — passed `releases` and the Voice Assistant said it before any
  search (`chat/tools.rs` above). Reasoning blocks
  never show (`without_thinking`, even a half-written `<thi`, when the
  scope hides thinking); gpt-oss's `【1】` shows as `[1]`; the text goes out
  at most every 50 ms (`MIN_GAP`), whole each time. **The events** (the
  page's channel, `stream::Event`, tagged `kind`): `text {text}` (the
  answer so far; "" takes it back), `tool {name, doing}`, `chips {chips,
  sources, web}` (after each tool and each of Yap's own searches: the chips
  so far and the web sources found, so a "[1]" is a pill while it's
  written), `waiting {waiting}`; `chat_send` adds `turn {conversationId,
  userMessageId}` once the question is saved. The command's own reply is
  still the **checked** answer (`chats::checked_answer` and the price
  fixes), which the page puts in place of what streamed. **The channel**:
  `PageChannel`, an optional Tauri channel argument (`onStream`; Tauri's
  own `Channel` can't be left out, and the e2e suite's direct invokes
  don't send one — no channel, a whole answer as before). **Stop**
  (`Stoppable`, `stream_stop {id}`, `unless_stopped` → `Halt`): the page
  sends a `turnId`; ■ drops the turn's future, which closes the stream
  (llama.cpp stops generating on a closed connection); `chat_turn` keeps
  what was shown (`Watch`), checks it like any answer and saves it with
  `stopped: true` (`chats::append_message`); nothing shown yet → the error
  `stream::STOPPED`, the question kept (as after a failure). **A newer
  question replaces an older one** (`Halt::Superseded`, 2026-10-23): every
  `chat_turn` has a `Stoppable` (its own when no page sent one: the Voice
  Assistant, an evaluation), and once its question is saved
  (`Stoppable::answers(conversation)`, also in `app_turn`) it's the turn
  answering that conversation — one still answering there ends the way ■
  ends it, but with **nothing saved** (the error `stream::SUPERSEDED`): its
  question stays, the model reads it with the newer one, and the pages
  take its bubble away without an error. The newest question decides,
  from any surface: the Voice Assistant's barge-in or follow-up, a
  question typed in Chat while a spoken one is answered, one dictated into
  Chat and sent by itself while Chat's answer comes (the page drops it
  first: `stream_stop {id, superseded: true}`, then sends). A tester's voice
  chat: the first question, set aside for the barge-in that asked the
  second, was asked again after the second's answer, and chats.json had
  both questions, then both answers. **Never
  slowing a dictation** (`Dictations`, a model on this PC only:
  `ai_model::runs_here`): a Chat stream is dropped when a **new**
  dictation starts during it (`NewDictations`: the dictations-started
  count moved — not one under way when the step began, which is the Voice
  Assistant's own question being finished), the page told it's waiting
  ("Waiting for your dictation to finish…"), and the step asked again once
  the dictation is done (`meeting_summary::wait_idle`, ≤ 5 times) — unless
  that dictation was a newer question to the same conversation, which
  supersedes the turn while it waits (above); the
  notepad's helpers keep their rule (`AnyDictation`, as
  `chat_beside_dictation`: wait one out first, drop the call whenever one
  is busy — `chat_beside_dictation_live`). The sidecar's Busy guard
  (`local_llm::request_starting` for each request, `hold` for the run)
  covers a stream to its last chunk. Unit-tested (the wire, every split
  point; a step's release, retraction and restart; the stop) and through
  the real tool loop against a local SSE stand-in (`chat/tools.rs` tests: a
  preamble never shown, fragments reassembled, a broken stream asked again
  whole then not streamed again, a refused `stream`/`stream_options`, cut
  arguments and Groq's mid-stream refusal asked again). Model evaluations
  stream like the Chat view (`YAP_EVAL_STREAM=off`: whole) and measure the
  first words (`first_words_ms`).
- **`chat/assistant.rs`** — the **Voice Assistant** (2026-10-18; Settings →
  Voice Assistant): ask Yap's Chat out loud from any app with its own
  shortcut (`assistant_hotkey`, unbound by default) and hear the answer;
  it never takes the focus or types. Not **Commands** (the agent's name in a
  dictation, the rewrite shortcut: the AI types for you — Wispr's Command
  Mode and old "Hey Flow"; Wispr has no spoken-answer assistant). **Off
  during meetings** (the maintainer: Bella talking over a meeting "would be too
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
  (`turn::resolve_chat_endpoint` errs) → the `assistantNoModel` alert
  with **Open settings** (`alert::OPEN_AI_SETTINGS` → Language Models →
  Chat & Voice Assistant; a model that fails → `assistantFailed`, the
  reason + "Your question is in Chat."); else **`choose_conversation(viewing,
  open_exists, latest, now)`** (pure): the conversation open in the Chat
  view (`chat_view_state {open, conversationId, newChat}`: ChatView
  reports it), a new one if "New chat" was clicked there, else the last
  updated if under `RECENT_SECS` (30 min) — so pressing again asks a
  follow-up — else a new one; then `turn::chat_turn` (the Chat view's
  own turn: RAG, tools, `voice: true`), `yap-assistant-turn {phase:
  asked|answered|failed|superseded|open, conversationId, userMessageId,
  show}` for the main window (`show` = the window was in front:
  ControlPanel brings Chat up on it; `userMessageId` = which question).
  **Streamed** (`Streamed`, a `stream::Sink`, 2026-10-22): the Chat view
  follows the answer as it's written (`yap-assistant-stream
  {conversationId, userMessageId, event}`: an older question's events never
  land in a newer one's bubble), and the voice **starts at its first whole
  sentence** (`tts::speak_streamed` → a `tts::Feed`; `tts::ready_sentences`:
  the first block's sentences, never the one still being written, at most
  `STREAM_SAY` = 2 while it's written — the short version's), the capsule
  going to Speaking and the card coming up then; once the answer is whole
  and checked, the rest of what `spoken_answer` says goes after the
  sentences already handed over and the feed closes (`finish_streamed`).
  In a web answer a sentence with a price waits for the check
  (`mentions_price`: `about_you::restore_price_signs` may change "£8"
  back to "$8"). The answer's id is known before it's saved (the question's
  index + 1). **Words taken back** (`Streamed::taken_back`, 2026-10-25): a
  step's opening words long enough to show (`stream::releases`) that turn
  out to come before a tool call are taken back (`text` "") — the voice may
  have started on them: it stops, the sentences it hadn't said go (the feed
  closed), the capsule goes back to Thinking and their card goes, and the
  answer after the tool starts the voice again from its first sentence.
  Before, the voice went on ("Let me look through your notes for that.")
  and the answer's first sentences were skipped as already said (`said`
  counted the words taken back). E2e-tested with the fake model scripting
  words, then a call (`voice-assistant.spec.js`); a forced search's words
  never get this far (`ChipsOnly`). Barge-in, ■, a newer question, a meeting starting: the voice
  stops and no more goes in (`Feed::push` says so); a meeting started
  meanwhile leaves the answer on its card for a reading time. **A newer
  question in the same conversation** (a barge-in or follow-up asked while
  the last answer is still coming — or still waiting out that very
  question's recording, as a model on this PC does) **replaces** the older
  turn once it's saved (`stream::Halt::Superseded`, above): no answer saved
  for it, nothing more said (its feed closed, its voice stopped if it's
  still the one speaking), no alert, `yap-assistant-turn {phase:
  superseded}`.
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
  `listenMs` — with `text` as the transcript). **A pause ends the
  question** (`dictation/pause.rs`, below; 2026-10-23): no second press needed.
- **`dictation/pause.rs`** — **"Stop listening when I pause"** (Settings → Voice
  Assistant, `assistant_stop_on_pause`, on; 2026-10-23, after a log where
  the maintainer asked a question in 2.5 s, waited 6 s in silence, and his
  grumble became part of the question). Once you've said something to the
  Voice Assistant, a pause of **1.5 s** (`PAUSE_MS`) ends the question as
  a second press would: the stop chime, the transcription, the answer.
  **Never before you've spoken**, never under a held push-to-talk key
  (letting go ends it): toggle mode and the double-tap lock only
  (`gesture::hands_free`). Dictation never ends by itself. Pure, timed in
  audio (samples), never on the clock, unit-tested like `dictation/gesture.rs`:
  `Pause::new(at)` (the buffer's length: the pre-roll), `speak_now` (the
  start chime: `fire_pending_start`), `audio(samples)` (`take_audio`),
  `words(covered, text)` (each live-preview tick, `stream_partials`), and
  `verdict()` → `Listen | Ending {left_ms} | End(Pause | NoNewWords)`.
  **The signal**: the mic's level, 20 ms frames (RMS, dBFS) against a
  **noise floor** the room teaches it — down fast in the quiet between
  words, up slowly (`FLOOR_RISE_DB_PER_S` 4) under a sound that stays, so
  a fan, a hum or steady music becomes the floor while speech, which dips,
  doesn't; speech = `MARGIN_DB` (10) over the floor and ≥ `SPEECH_MIN_DB`
  (−50, OpenWhispr's speech gate: RMS 0.003); digital silence teaches it
  nothing; a sound counts from `ONSET_MS` (60: clicks and keys don't);
  nothing counts for `SETTLE_MS` (400) after "speak now" (the chime); a
  pause can end it once `HEARD_MS` (300) of speech was heard. **Checked by
  the live preview's words** while it runs (`dictation/partials.rs`, on by default):
  no words yet ("", `[BLANK_AUDIO]`, `(music)` — `word_count`) → a pause
  ends nothing (a cough, a door); more words than ever in a stretch the
  level called quiet = a voice it missed, the pause counts from there; and
  **no new words for `STALL_MS` (4 s) of audio** ends it though the level
  never went quiet (music, a TV, a noise not learned yet) — only once the
  words grew twice. **Why the level and not a VAD model**: Handy's Silero
  (via ONNX) is a model to ship or download, absent from the stub build,
  and says "voice", not "finished"; the level never ends a question early
  because of noise (noise only keeps it open; the shortcut still ends it),
  and the words catch what it can't. **Why 1.5 s**: none of Handy (Silero
  VAD, 450 ms hangover, only trims silence), FluidVoice (a level meter's
  noise gate) or OpenWhispr (stops on the key; its only silence setting is
  the 600 ms server VAD it asks of OpenAI's Realtime API to cut a live
  transcript into turns) ends a recording on a pause; 600 ms would cut a
  question at its first thinking pause, and 1.5 s ends one promptly while
  letting a short "um" pause through. **What it can't tell**: a sound the
  floor first takes for talk and then learns (music that starts as you
  start listening) can end it before you speak when the preview is off,
  or when a model makes words up from it; the preview's words otherwise
  hold it. **In the pipeline**: `arm_pause` (`start_recording`, an
  assistant recording with the setting on: a `PauseSlot` for its session),
  `spawn_pause_watch` (thread `assistant-pause`, every 50 ms: `End` →
  `stop_and_transcribe` under the gesture lock — which now does nothing
  for a recording already stopped, so a press at the same moment can't
  stop it twice; `Ending` → `yap-assistant-pause {leftMs}` once, `Listen`
  after it → `{leftMs: null}`), logged as "Voice Assistant: you paused, so
  the question ends" with the reason and how much was heard (no text).
  **The bar**: an amber hairline at the capsule's foot runs down over the
  last second (`.pauseline`, `dictation.pauseMs`; it moves with Motion
  reduced: it says how long is left). Debug-only `e2e_assistant_audio
  {parts, press, noiseDb}` (`shell/e2e.rs` → `Robot::ask_assistant_audio`):
  tones, quiet and WAVs over a room's hiss played in the mic's place, the
  shortcut pressed through the real key path (`toggle | hold |
  handsFree`) → `{endedByItself, listenedMs}`.

## The Chat view

`src/lib/views/chat/` (split 2026-10-24): `ChatView.svelte` is the view —
it makes `chatState.svelte.js` (the conversations, sending and the streamed
answer with ■ and a newer question replacing one, the Voice Assistant's
questions, Snip to Chat and whether the model reads images, spoken replies,
several picked at once), follows Rust's events while Chat is open, takes
its keys and shows a screenshot large. Its parts: `ConversationList` (the
list, its menus, rename in place, the selection bar), `ChatThread` (the
empty state, "Thinking…") with `ChatMessage` (tool chips, the bubble,
Stopped, Sources, ▶ and the model under an answer) and `Composer` (the
screenshots and their prompts, +, the box, the mic, Send / ■, `ModelChip`).
[frontend.md](./frontend.md#the-folder-layout-2026-10-24).

- **`ChatView.svelte`** = the
  AI Chat surface (OpenWhispr `chat/ChatView.tsx` port): conversation sidebar
  (Today/Yesterday/Previous 7 Days/Older grouping, Ctrl+N, hover-delete;
  **several at once**: Shift+click a range, Ctrl+click one, Ctrl+A in the
  list → a "3 selected · Delete · Cancel" bar, Delete key / Esc; the list's
  ⋯ menu has Select all and "Delete all chats…", every delete asked first;
  `chat_delete_many` = one write, `chats::delete_many`) +
  thread; `chat_send` answers via the Chat scope with **eager keyword-RAG**
  over the notes library (top-5 `<note>` snippets; `chat/chats.rs` persists
  conversations to `chats.json`) **plus the tool-calling agent loop**
  (`chat/tools.rs`: search_notes/get_note/create_note/update_note/list_folders/
  copy_to_clipboard, plus search_in_note/read_note/edit_note for changing
  part of a note, executed locally, ≤20-step loop over the OpenAI tool
  protocol, gated to cloud or ≥4B local models — smaller models fall back to
  plain RAG chat; tool-activity chips render in the thread above the answer
  and are kept with it in chats.json; a question from
  the Meetings view's Ask bar arrives through `chatRequest` and is sent with
  `scope: 'meetings'`). Answers render as markdown — headings, lists,
  tables, code, quotes, links — through `renderMarkdown` (`markdown.js`:
  markdown-it + DOMPurify, never the model's own HTML; see
  [notes.md](./notes.md#the-markdown-renderer)). **A web answer** (`web.rs`): its `[1]` citations are
  small pills that open their source (`renderMarkdown(md, {sources})`,
  even inside a table cell, where only the sources' links stay links — any other
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
  `chatModels.svelte.js`; the maintainer's "Concept A, provider rail",
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
  graphics card (NVIDIA …)" from `ai/gpu.rs` / the sidecar's placement, or
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
  Yap's own "Nothing new" / "too little"). **Spoken replies** (`voice/tts.rs`):
  **Voice replies** (🔊 / 🔇 pill beside the model chip, `aria-pressed`) is
  Settings' "Speak replies" (`voice_replies_set`; Settings adopts
  `yap-voice-replies`, the toggle re-reads on `yap-config-saved`); every
  answer has **▶ Read aloud** under it (■ Stop reading + "Speaking…" with
  three bars while it's read; also on the Notes view's Ask bar answers and
  the notepad's Ask sheet, not while that meeting records), via the shared
  `speech.svelte.js` store (`speakingId` from `yap-tts-state`; each window
  toasts failures for its own ids: `chat:` / `notes:` / `note:` / `preview` /
  `notepad:`). ▶ is **Read aloud** (`read_aloud_text`, since 2026-10-22): the
  answer read with the reading card on the Yap bar, its words lit up, ⏸ ⏭
  and the speed there, as any selection is ([spoken-replies.md](./spoken-replies.md),
  `voice/read_aloud.rs`); an answer read out by itself (Voice replies) is the
  voice alone (`speakReply` → `tts_speak`), no card. With replies on, chat_send's `speak` reads an answer out by
  itself. The **mic** is `chat_dictate`, and the box reports its focus
  (`chat_input_focus`), so a question dictated into it arrives as
  `yap-chat-dictation` (inserted at the caret, never pasted) and is sent at
  once when `send` says so; such questions are sent with `voice: true` (a
  small mic on the bubble). Esc, leaving Chat, another conversation, the
  next question and any dictation starting stop the voice. **The Voice
  Assistant** (`chat/assistant.rs`): the view reports what it shows
  (`chat_view_state`: the conversation, or "New chat" clicked) and follows
  `yap-assistant-turn` live — it switches to the question's conversation,
  the question with its mic, "Thinking…" (`assistantPending`), the answer
  (Rust reads it aloud); ControlPanel brings Chat up for one asked while the
  window is in front (`show`) or "Open in Chat" (`chatRequest {assistant}`).
  An empty chat says "Or press F8 in any app to ask out loud…", or offers
  **Set an assistant shortcut** (→ Settings `voice#assistant-shortcut`).
  **Streamed answers** (`chat/stream.rs`, `streaming.js`; 2026-10-22): `send`
  calls `chat_send` through `streamCall` (a Tauri `Channel` as `onStream`,
  a `turnId`); the answer's bubble appears with its first text or tool
  call ("Thinking…" until then) and **grows in place** — markdown rendered
  as it comes through `renderMarkdown(closePartial(text))` (an open code
  fence closed, a half-written `**bold**` or `` `code` `` on the last line
  closed, a half-typed "[1" or link left out until whole), a caret at the
  end of its last line, the chip of the tool call under way (spinner,
  "Searching the web…"), "Waiting for your dictation to finish…" while a
  model on this PC steps aside; the thread follows the newest words unless
  you scrolled up. **■** (the composer's Send becomes Stop while an answer
  comes; Esc) stops it: what came stays, "· Stopped" under it; stopped
  before anything came, the question stays with "Stopped". **The checked
  answer** (`chat_send`'s reply) takes the streamed one's place in the
  same bubble: the same text stays put; a changed one (a sources list
  taken out, a note added, an unsourced "[1]" gone) glides from the
  streamed height to the new one and fades in (`settle`,
  `streamingHeight`; nothing while motion is reduced) — an unchecked claim
  never stays after the end. A failure removes the streamed bubble with
  the question, as before. A question asked out loud streams the same way
  while its conversation is open (`yap-assistant-stream`). **A newer
  question replaces the answer still coming** (2026-10-23): a question
  dictated into the box and sent by itself (hands-free, "press enter")
  while an answer comes no longer waits for it (`sendWhenFree` alone): the
  page drops the answer first (`replaceAnswer` → `streamCall`'s
  `supersede()`, `stream_stop {id, superseded: true}`) and sends at once; a
  turn a newer question replaced — that, or one asked out loud — ends with
  `SUPERSEDED`: its bubble goes, its question stays (with its saved id), no
  toast, nothing handed back to the box. A spoken question's bubble is its
  own (`spoken`), the send's carries its `turn`, and events and phases for
  an older spoken question (`userMessageId` ≠ the one followed) are left
  alone, so they never land in the newer one's bubble or end its
  "Thinking…". A typed Enter still waits while an answer comes (type
  ahead), as before. **A snip** ([snip.md](./snip.md), 2026-10-23) lands in
  the box (`yap-snip`, or ControlPanel's `chatRequest {snip}`): the
  conversation Rust chose — the one open here when the window was on screen
  on it, else a new chat —, the thumbnail and its size with ✕, the three
  one-tap prompts (What's this? · Explain this error · Summarise), "Ask about
  this screenshot…", the caret there; Enter on an empty box asks "What's
  this?"; never sent by itself; it waits while another view shows
  (`chatShots.svelte.js`). A model that can't read images: one amber line
  with "Use <model> on <provider>" (`chat_set_model`), "Choose another
  model…" (the picker, `ModelChip.openPicker`) and "Send without the
  screenshot", Send waiting. A question's screenshots show in its bubble
  (click: large) or "Screenshot removed after 30 days". + → "Take a
  screenshot" (`snip_start {origin: "chat"}`).
  No semantic vectors yet (ROADMAP step 3).
