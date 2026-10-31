# Chat and the Voice Assistant

The AI Chat surface (conversations, the tool-calling loop and its note tools) and the Voice Assistant that asks it out loud. Web search and opening apps are [web-and-apps.md](./web-and-apps.md); reading answers aloud is [spoken-replies.md](./spoken-replies.md); the models are [ai-models.md](./ai-models.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`chat/chats.rs`** — the AI Chat surface's conversation store (`chats.json`): a
  `Conversation` (title, messages, timestamps) with `list`/`get`/`create`/
  `append`/`delete`/`rename` (`chat_rename`: one trimmed line ≤ 80 chars,
  `updated_ts` untouched so the list order stays; unit tests use an
  in-memory store per thread, never `chats.json`); **archive**
  (2026-10-25: `Conversation.archived_ts`, 0 when it isn't; `set_archived(ids,
  on)` in one write, its place in the list kept; `list` leaves archived ones
  out, `list_archived` has them, archived last first; `latest` — the Voice
  Assistant's — skips them; a message added to an archived conversation
  brings it back; commands `chats_archived`, `chat_archive(ids, archived?)`);
  every save tells search by meaning (`search::changed`), a deletion
  `search::removed` (its vectors go, [search.md](./search.md)); `with_all` /
  `stamp` for the index; `chat_send` (`chat/commands.rs`; the turn itself is
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
  some). An answer asked across the meetings keeps its passages the same way
  (`Source` with `note`, `field`, `line`, `ts`, `quote`: the moment its pill
  opens), and the conversation keeps `scope: "meetings"` (`set_scope` /
  `scope_of`), so its follow-ups are asked across the meetings too — the
  passages, the citation rules and checks are [search.md](./search.md#ask-across-meetings-in-chat).
  **Every answer is checked before it's shown and saved**
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
  (`web_history_note`). **Answers another model wrote** (a switch in the
  model picker; each answer keeps its `model`) are named in one line in the
  same place (`other_models_note`, 2026-10-28): the history's answers go to
  the model as its own, so Claude Haiku 5.5, picked after Qwen3.5-4B, said
  "I wrote those earlier replies, and the last one was wrong" of the 4B's.
  Never a note in the answers' text (a model copies it, above). **Every**
  system prompt ends with the **About you
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
  points back at one (`chats::refers_to_past`). The question's memory, and
  past chats, are found by their words and — search by meaning on — their
  meaning (`search::library::memory_block`, `past_chats_block`,
  [search.md](./search.md#beyond-the-meetings-2026-10-25)). Every turn tells memory's
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
  the history window go again, older ones as a line (every one a line to a
  model known not to read images); a server's refusal becomes
  `vision::COULDNT_READ` (that model a "no" this session). A refusal of the
  earlier ones alone, on a follow-up, makes the model a "no" too and the
  step goes again with them as lines (`snip::turn::without_images` in the
  tool loop, 2026-10-30).
  `chat_send(images, asText: true)` ("Send its text instead", 2026-10-25):
  the screenshots' text, read on this PC by Windows' OCR (`snip::ocr`), goes
  after the question instead of the pictures, which stay with it marked
  `ChatImage.asText`; the reply's `question` is the text as saved, and a new
  conversation is named by the question's own words. Pictures pasted or
  dropped into the box are stored like snips (`snip::attach`). **The
  screenshots waiting in the box** (2026-10-25) are kept in Rust
  (`snip/waiting.rs`, `yap-chat-waiting`; the page's `waitingShots` follows):
  `chat_send` takes the ids it sends (back in the box if it fails before the
  answer), and a **Voice Assistant question takes them all** — to the
  conversation they wait in, through the same `vision` / `turn::prepare` /
  `turn::with` steps; a model known not to read images gets nothing, the
  question goes into the box beside them and Chat comes up with its line
  (`assistant::cant_read`). After a spoken "snip" lands, Yap listens for the
  question by itself ([snip.md](./snip.md#after-saying-snip-yap-listens-for-the-question-2026-10-25)).
  **The passage quoted over the box** (a document's "Change this…") is kept
  in Rust the same way (`documents::WaitingQuote`, `chat_quote_set`), and a
  Voice Assistant question asked with Chat in view, to the conversation it's
  quoted in, takes it (`documents.rs` above).
  **The history** is sized to the model in tokens since 2026-10-25
  ([below](#long-chats-sized-to-the-model-2026-10-25): `chat/budget.rs`,
  `chat/compact.rs`); until then it was the last 19 messages
  (`chats::history_start` with `CHAT_HISTORY` (19, 6), the Voice
  Assistant's `ASSISTANT_HISTORY` (8, 4)), which a model evaluation can
  still ask for (`YAP_EVAL_HISTORY=messages`). Its start still moves a few
  messages at a time, not every turn, so a long chat keeps its prompt's
  start. A conversation keeps its **summary** of older messages
  (`Conversation.summary` = `ChatSummary {text, upto, after, model, ts}`,
  `set_summary` only over the one it extends; `usable_summary`; an older
  Yap's save drops it — a cache, the messages stay). **An answer's
  tokens** (2026-10-25): `chat_turn` runs inside `usage::tallied`, and the
  turn's last model call — the step that wrote the answer, which read the
  whole prompt — is kept with the answer (`ChatMessage.tokens` =
  `MessageTokens {prompt, answer, perSecond?}`, only when the model said;
  none for a stopped answer, whose call never finished) and returned by
  `chat_send` (`tokens`): Chat's context ring ([below](#the-context-ring-2026-10-25)).
  An older Yap's save drops it; the ring then estimates. **A note's Ask
  threads** (2026-10-25) are conversations here too, with `note` set
  (`create_for_note`, `for_note`, `delete_for_note`, `append_whole`; an
  answer to show as plain text keeps `plain`): Chat never shows them — not
  in `list`, `search` (past chats) or `latest` (the Voice Assistant) — and
  deleting the note deletes them. An older Yap on the same data drops
  `note` and `plain` when it saves: the thread is then an ordinary chat,
  its messages whole (unit-tested).
- **`chat/note_chats.rs`** — **a meeting's Ask thread, kept** (Wispr Flow's
  "New chat" / "Past chats"; the private roadmap's R15): what was asked about
  a meeting in the notepad's Ask sheet or its note's "Ask anything…" bar
  stays with the note and reopens in both. Chat's own store rather than a
  store of its own — its messages (model, stopped), saving (`SAVER`),
  quarantine and deletes come with it. `note_chat(noteId)` → `{current,
  threads}` (the newest thread whole, every thread newest first),
  `note_chat_open(noteId, conversationId)` (never another note's or a
  Chat conversation), `note_chat_add(noteId, conversationId|null, messages,
  origin)` → the thread's id (a new one when there's none, or it isn't this
  note's: named by its first question, ≤ 50 characters) and
  `yap-note-chat {noteId, conversationId, origin}` to every window. The
  windows keep each turn as it's shown (`core/noteChat.js`). **The next
  question carries the thread** (2026-10-25): the Notes view's bar
  (`note_ask {conversationId}`) and the notepad's Ask sheet
  (`meeting_catch_up {question, conversationId}`; "What did I miss?" stays
  without) send the thread's id, and Rust adds its earlier turns sized to
  the model beside the note's own prompt (`history_for`: every turn when
  they fit, else its summary and the turns after it, else the latest that
  fit; a question too long for the model alone is the plain sentence;
  `with_thread` lays them into the notepad's one-off request). A kept turn
  (`note_chat_add`) looks for a summary in the background with the model
  that window asks (`compact::after_thread_turn`: the notepad's Note
  Formatting model, else Chat's). Logs: ids and counts. Since 2026-10-29: a
  kept answer keeps the meeting's lines it cites (`NewMessage::sources` →
  the message's `sources`, Chat's own field, which an older Yap keeps
  too), so its pills open the line when the thread reopens; and
  `note_chat_rename(noteId, conversationId, title, origin)` (`rename`:
  this note's thread only, Chat's `clean_title`) names one from Past chats
  (`yap-note-chat` with `renamed`).
- **`chat/skills.rs`** — **Skills** (2026-10-29; Wispr Flow's Skills): saved
  instructions run with `/name`. See
  [Skills and one-tap prompts](#skills-and-one-tap-prompts-2026-10-29).
- **`chat/budget.rs`** — **a chat sized to its model** (2026-10-25): the
  context of the model a call goes to (`context_window`, shared with the
  ring), the budget (`budget`), the token counter (`Counter`, `estimate`),
  the window (`plan`), when a summary is due (`due`), a turn's tool results
  trimmed to the room left (`fit_result`), a plan's tokens a minute
  (`rate_cap`), a model's cheaper rate (`price_cap`; `request_cap` both) and "too long" said plainly (`too_long`, `plain_error`).
  [Below](#long-chats-sized-to-the-model-2026-10-25).
- **`chat/compact.rs`** — **long chats remember their start** (2026-10-25):
  the older messages summarised in the background after an answer
  (`after_answer`, `after_thread_turn`), incrementally, by the chat's own
  model; the summariser's prompt (`prompt`), its input (`request`, `line`,
  `parts`) and what's kept (`tidy`); `yap-chat-summary {conversationId,
  upto}`. [Below](#long-chats-sized-to-the-model-2026-10-25).
- **`chat/tools.rs`** — the AI Chat tool-calling agent loop: six tools ported
  near-verbatim from OpenWhispr's `services/tools/*` (`search_notes`,
  `get_note`, `create_note`, `update_note`, `list_folders`,
  `copy_to_clipboard` — executed locally over `notes.rs`/`arboard`;
  **`search_notes` searches as Ask across meetings does** since 2026-10-25 —
  passages by BM25 and meaning, the query's dates and voices, each note with
  the passages that matched: `search_notes_tool`, run by the loop,
  [search.md](./search.md#beyond-the-meetings-2026-10-25); `search_chats`
  likewise, by words and meaning) and
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
  `local_model_uses_tools`: a family that can't never
  (`NO_TOOL_FAMILIES`: Llama 3.2, which loops; Gemma 2, whose chat format
  has no place for tools — 2026-10-27, a 9B was offered them and llama.cpp
  refused the replayed search), one the evaluations saw succeed from its
  size (`TOOL_FAMILIES`: Qwen3.5 ≥ 4B, thinking off), else ≥4B params
  (`LOCAL_TOOL_MIN_PARAMS_B`; the size read from `-4B`, `_4B`, `:8b`) — so
  smaller local models fall back to plain keyword-RAG chat instead. The
  on-device model's own chat format has the last word once it has loaded
  (`local_llm::template_takes_tools`, llama.cpp's `/props`): one with no
  place for tools gets none, so a GGUF dropped into `llm/` is right too
  (ai-models.md). A strict format that wants the person and the model to
  take turns is asked again so (`llm::taking_turns`, ai-models.md); tested
  in `e2e/chat-format.spec.js`. **A tool
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
  (`a_web_page_can_never_make_the_chat_act`). **A strong model**
  (`documents::rich::strong`, 2026-10-31) is refused only `STRONG_LOCKED`
  (open_app, copy_to_clipboard, remember, forget): its note and document
  changes go through after the web (see "Strong models' documents" below;
  `a_strong_model_edits_after_the_web_and_never_acts_outside_its_notes`).
- **`chat/tools/calendar.rs`** — **calendar tools in Chat** (2026-10-25):
  "what's on tomorrow?", "when's my next meeting with Sam?", "am I free at
  3 on Friday?" answered from the calendar Yap already keeps
  ([calendar.md](./calendar.md): meetings from 12 h back to 7 days ahead,
  and since this day **busy time of your own** — an event nobody else is
  on, or over 6 h — kept only for this). **No sync, no network**:
  `calendar::agenda()` copies the store and lets go of its lock before the
  notes store (the meetings' notes) is read. **One tool, three plain
  arguments** (`check_calendar(day, time, with)`, offered while a calendar
  is connected, `Extra.calendar`; ~200 tokens with its instruction line,
  `the_tool_stays_light`): small models do better with one tool than three.
  **Yap reads the day and the time** (`resolve_day`, `resolve_time`, pure,
  in the person's zone — About you's IANA zone, else this PC's: `Zone`):
  today / tonight / tomorrow / yesterday / "day after tomorrow" / "in 3
  days", a weekday (the next one, today included; "next Friday" never
  today; "last Friday"), a date ("2026-10-30", "30 October", "Oct 30th",
  "the 30th"; one long gone is next year's), "this week" (to Sunday), "next
  week" (Monday–Sunday), "the weekend"; "15:00", "3pm", "half past 3", a
  bare "3" as an afternoon hour (1–7 → pm), "morning" / "afternoon" /
  "evening" / "lunchtime" as stretches. A time is checked for 30 minutes
  (`free: true|false`, what's in the way); a time with no day is today's,
  or tomorrow's once it has gone. Midnight is the person's own (a day of 25
  hours when the clocks go back stays whole: `Zone::day_start`). `with`
  matches every word of it to the start of an invitee's name or address
  word, the organiser's or the title's ("Sam", "Sam Lee", "Priya's",
  "roadmap"); with no day, the next 5 meetings with them. The result: the
  meetings (`n`, title, "Fri 30 Oct, 10:00–10:30", who, the call app,
  maybe, overlaps, on now, **over** — a meeting earlier today), the busy
  time (`own_event`, never numbered), and **the next meeting after** a day
  with nothing (still) on — "what's my next meeting?" asked as "today" late
  in the day (the Voice Assistant's evaluation). A day outside what Yap
  keeps says so; a calendar unsynced for 2 h+ says when it was. Chips:
  "Checked your calendar: 2 meetings, Monday 26 October 2026", "Found 1
  meeting with "Sam"", "Free Fri 30 Oct, 11:00–11:30" (a calendar mark in
  Chat). **Citations**: every meeting it gives is numbered in **the
  turn's one numbering** with the web's (`web::Turn::reserve_number`: a
  page and a meeting never share a number; without the web, the turn's
  `calendar::Log`), and becomes a `Source` with `event` (its key), `note`
  (its note, if any), `field: "calendar"` and its time as `site`; a
  meeting the answer names but didn't cite gets its number from Yap
  (`cite_mentions`: by its title — a hyphen any dash, gpt-oss's
  non-breaking one included —, else someone only it has, else its start
  time when no other meeting starts then). The pills and the **From your
  calendar** row open the meeting's notes — made from the invite if there
  are none yet, as the Meetings view's row does (`calendar_event` "open").
  **A model without tools** (Qwen2.5-1.5B, Qwen3.5-2B): Yap spots a
  calendar question (`asks_about_calendar`), reads its day, time and person
  from the question itself (`question_args`), looks it up and gives the
  model the result in its prompt (`block`) as plain lines with **the
  verdict first** (`plain`: "The answer: NO, the user is not free then;
  they have: - [1] Roadmap planning — …"; given JSON, the 1.5B and the 2B
  said "Yes, you are free" over `"free": false`), numbered and cited the
  same way. Still unreliable on them (model-evals.md "The processor
  tier"). An answer citing calendar meetings isn't one "written from the
  web" (`chats::from_the_web` counts only sources with an address). **No calendar connected**: the tool isn't offered, and a calendar
  question carries `NOT_CONNECTED_NOTE` (say so, and Settings → Connectors;
  never make up meetings). **The Voice Assistant** says a calendar answer
  whole (`assistant::web_sources` counts only web pages: it isn't cut short
  with "The details and sources are in Chat."). Not offered across the
  meetings (their passages are numbered on their own). Unit-tested (the
  dates near midnight in London and New York, the clocks going back, every
  phrasing above, `with`, free/busy, citations); e2e
  `chat-calendar.spec.js` (a local iCal feed, the fake AI's tool call, the
  chip, the pills, the note opened, the voice); model evaluations
  `calendar-*` (model-evals.md "Calendar tools").
- **`chat/documents.rs`** — **documents beside the chat** (2026-10-30,
  phase 1; the idea after Claude's own app, nothing of its code): when the
  answer is something to keep — a game build, a plan, a guide, a comparison
  — Chat writes it as a document that opens in a panel to the right of the
  conversation, fills it section by section while you watch, and later
  questions change it. **Three tools** beside the note tools, offered
  wherever they are (`notes::definitions`, run by `notes::execute`,
  `documents::TOOL_NAMES`): `create_document(title, summary?, sections:
  [{key, intent}])` makes a note in the
  **Chat documents** folder (made on first use; `source` "chat") whose body
  is `# title`, the summary (a sentence or two, a paragraph: `skeleton`)
  and one **placeholder line** per section — `<!-- yap:section
  Gear | what to wear and where to find it -->`: an HTML comment nobody
  types by accident, which the renderer draws as a quiet line and any other
  markdown viewer hides; key and intent cleaned of `|`, `<`, `>`, `--` and
  line breaks (`placeholder`, `parse_placeholder`; ≤ 12 sections, each key
  once, plain strings taken as keys) — and returns the note id, the keys
  and `next` ("Now call write_section for "Gear"."); `write_section(note_id,
  key, markdown)` puts `## heading` and the body in that placeholder's
  place (`fill_section`: the outline's own spelling of the key; the
  model's first `#`/`##` heading is the section's; placeholder lines it
  copied are dropped), `left` and `next` say what's still to write ("Every
  section is written. Tell the user in a line or two…"). **Never over the
  person's words**: it only replaces its own placeholder line, exactly as
  Yap wrote it; one the person deleted or typed into is **refused** — "the
  user removed or changed it… Don't write it again or add it back", chip
  `Didn't write "Costs": it's no longer in "Weekend…"` — and one written
  already says to use rewrite_section. **Later changes** (2026-10-30):
  `rewrite_section(note_id, heading, markdown)` replaces one `##` section
  whole — its `###` parts with it, up to the next `#`/`##` heading or
  placeholder (`section_range`) — or, a heading the document never had,
  adds one at its end; the model's own `##` heading renames it. **The
  person's words win here too** (`rewrite_section`, pure): the section must
  be exactly what the model last saw (its picture, below); one the person
  changed or wrote is refused ("read_note first, then rewrite it keeping
  their changes"), one the model saw that's gone was removed ("Don't add it
  back"), and a placeholder is filled as `write_section` would. Only a note
  Chat made (`source` "chat"), and in a turn only one of this chat's
  documents (`Turn::refuses`). Chips "Rewrote "Gear" in "…"" / "Added
  "Tips" to "…"" (rewrites fold: "Rewrote 3 sections in …"). `edit_note`
  (its exact match already refuses changed text) is for a few words,
  `update_note` for the whole. **Every
  change is a version** (`versions::by` with `kind::CHAT`), and **a turn's
  versions are one change** (2026-10-31: `chat_turn` runs inside
  `versions::in_turn`, so each version it makes carries the turn's number,
  `Version::turn`): the panel's Undo goes back past all of them, never to
  half a document (below). The sections' chips fold into one
  (`push_chip`: "Wrote 3 sections in "Lone wolf ranger""). **Not action
  tools** (`tools::ACTION_TOOLS`): a build is researched on the web first,
  and these are the answer's own words — a new note, sections filled where
  the outline left room, a section of this chat's own document rewritten
  only while it's what the model saw; `edit_note` on a document stays
  refused after the web. **Its sources, linked**: after a search, a
  document's `[1]` citations (in the summary or a section) become links to
  the pages themselves, `([site](address))` — a note has no Sources row
  under it — before the call runs (`link_citations`, `agent.rs`); an
  unknown number stays as written. **Live**: a `write_section`'s or a
  `rewrite_section`'s arguments stream to the page
  (`stream::LIVE_ARGS`; the `Assembler`'s `Piece::Args` → `Delta::ToolArgs`
  → the step's `Event::Section {noteId, key, text}` at most every 50 ms,
  `live_section` reading the JSON still coming — the id and key (or
  heading) once whole, the text so far with a half escape left out); a
  document made or changed (`create_document`, `write_section`,
  `rewrite_section`, or `edit_note`/`update_note` on one of the
  conversation's) is `Event::Document {noteId, title}`
  (`tools::Extra::docs`, a `documents::Turn`), which opens the panel;
  answered whole, the panel follows when the call lands. **The turn keeps
  them**: `Turn::touched` → `ChatMessage.documents` (`[{id, title}]`, the
  title when written; `chats::set_documents`) and `chat_send`'s
  `documents` — the pill under the answer. An older Yap's save drops the
  field: the pill goes, the note stays in Notes. **What the person changed
  since the model last saw a document** (`context`, in the next question's
  `<context>` for a model that calls tools): "Documents in this chat…" with
  each one's `note_id` and the sections still to write (the newest 3), and
  for each the person changed, a short diff ("- took out: "…"" / "+ put
  in: "…"", ≤ 10 lines of ≤ 160 characters, then how many more: `changes`)
  — against **the model's own picture** of it (`saw`/`seen`: kept as its
  calls go, `after_call` — what it wrote, its edits replayed, a whole
  `read_note`/`get_note`; in memory, after a restart the newest Chat
  version). Told once: then it has seen it. **The panel's saves**:
  `chat_document_save(id, base, content, origin)` — the person's edit laid
  over the newer text (`merge3`, line by line: what one side changed kept,
  two insertions at one place both, the person's first; where both changed
  the same lines the person's words win; changes side by side never clash,
  so a line typed under a section being written is kept), saved as typed
  (`kind::TYPED`, folding with their other typing). **The prompt rule**
  (`INSTRUCTIONS`, with `TOOL_INSTRUCTIONS`, the stable part of the system
  prompt): something to keep (a build, a plan, a guide, a comparison, a
  long table) goes straight into a document — **never the chat, never
  asked about first** (the 4B wrote a build in the chat and ended "Would
  you like me to create a more detailed document?", the maintainer's
  first try) — the title, a one- or two-sentence summary and every
  section, then each section in order; **how it reads**, after Claude's
  own documents: short bullets, a markdown table for anything with rows
  (per level, per item, side by side), **bold** lead-ins, and for a game
  build race and origin, character creation, skills, a level-by-level
  table, gear and tips; a line in the chat ("It's open on the right."),
  never the content; a change to a document from this chat, or more depth,
  goes in it (`rewrite_section`, `edit_note`); an option picked from one
  is a new document; never `create_note` for any of these. **A document is
  a note** (30 Oct 2026): Claude Haiku 5.5, asked to "put it into the notes
  or the Markdown file", made one and said "I can't write to your notes or a
  Markdown file from here". The model is told where it's kept — the rule
  ("A document is a Markdown note, saved in Notes under Chat documents."),
  `create_document`'s description ("shown beside the chat, saved in
  Notes"), its result's `saved` (`SAVED`: "Saved as a Markdown
  note in Notes, in the Chat documents folder: the user can open it there,
  or export it as a .md file (Export as Markdown)." — the Notes view's
  per-note Export as Markdown), the last section's `next` ("asked for it in
  their notes or a file: "It's open on the right, and saved in your Notes
  under Chat documents.""), and the next question's documents line ("open
  beside it and saved in Notes (Chat documents) as Markdown notes").
  **`create_note`
  refuses an empty note** (a tool error saying to use `create_document`):
  Claude Haiku 5.5 answered "Let's do option A" with `read_note` and an
  empty `create_note` in Personal, then asked the person to delete it.
  About 700 tokens with the three schemas (`a_tool_step_stays_light`:
  2,922 now: 2,898 before a document was said to be a note in Notes,
  trimmed from 3,050). **Kept on course** (tuned on the
  on-device Qwen3.5-4B, `--documents`): the model is told in its
  context what the question means for documents (`steer`) — a model on
  this PC in full, **a cloud model in one short line** (`brief`, 30 Oct
  2026: Claude Haiku 5.5 answered "Could you find a mage build instead…?"
  in the chat, ending "Want me to build a level-by-level plan for it?", and
  made the document only after "Yes, please."; "This asks for something to
  keep: make the document now, not an answer in the chat or an offer to
  make one.", `ASKS_BRIEF`, and the pick's and the change's own line with
  the document's `note_id`), always in the question's context, never the
  system prompt, so the prompt cache stays whole: **an option
  picked** from the open one (`picks_option`: "Let's do option A", "I'll
  go with the second one") → a new document for the pick, built out in
  full, the options left as they are — worth keeping, and the pick needs
  an outline of its own (the 4B edited the options, or copied them to the
  clipboard); **a change** to it or more depth on part of it
  (`changes_document`: a change word — change, edit, add, swap, go deeper,
  more detail… — and either "it"/"the build" or no ask for a new one) →
  that document reworked, `rewrite_section` per section and `edit_note`
  for a few words, not the chat (it answered "go deeper on day two" in
  the chat, 115–173 words); else a question that asks for something
  to keep (`asks_for_document`: "make me a … build / plan / guide / packing
  list", "plan a …", "step-by-step", "compare … and …"; never "what's the
  plan for tomorrow?") → a document now, not an answer with an offer
  (`ASKS_NOTE`; "look up / find … a build" counts: the maintainer's way of
  asking) — on the rule alone it answered a guide and a trip plan in
  the chat; a result's `next` asks for the section after as a table when
  its key or intent has rows — levels, day by day, costs, a comparison,
  attributes (`next_for`, `table_hint`: on the rule alone 2 tables in 9
  builds, then 9 in 9); the last section's result asks for one short sentence, no
  summary (it wrote 84 and 121 words under "It's open beside the chat");
  an answer with this turn's document half written is sent back once or
  twice to finish it (`Turn::unfinished`, `finish_note`, `FINISH_NUDGES`;
  the text it showed is taken back); and `create_document` again for this
  turn's document (same title) gives back the one there is
  (`Turn::made_already`, no chip) — it once made two in one step and filled
  one. **The Voice Assistant**: the same
  tools and system prompt (its cache), and in its question's context
  `SPOKEN_NOTE` — a document only when one is asked for, or a long build,
  plan or guide, then one sentence saying it's in Chat. **Asked with Chat
  in view** (Yap's main window in front when the shortcut was pressed —
  Chat comes up there with the question — or on screen beside the app in
  front showing Chat: `assistant::answer_in_view`, below;
  `assistant::asked_in_chat`, a task-local the assistant sets around its
  turn) the line points at the
  panel instead, "It's open on the right." (`SPOKEN_IN_CHAT_NOTE`,
  `spoken_note`): the maintainer asked by the assistant's shortcut while
  reading Chat and heard "It's in Chat." A spoken "build me
  a…" makes the document (too long to say, and asked to keep), the voice
  says the line, the bar's card has Open in Chat (which opens the panel on
  it), and Chat, when it's open, shows it filling; Yap never brings Chat up
  over the app you're in for it, as the assistant never takes the focus.
  **A question dictated into Chat's box is a Chat question** (30 Oct
  2026): what the model is told changes only when the answer is read
  aloud — the Voice Assistant's, or one dictated into Chat with spoken
  replies on (`turn.rs` `spoken`: the "asked out loud" lines, the small
  model's web prompt, `grounded_prompt`); the question's `voice` flag alone
  changes nothing.
  Logs: ids, counts, lengths. Unit-tested (placeholders, the tools,
  refusals, the merge, the diff, the model's picture, the live arguments,
  the loop end to end: `a_document_opens_fills_as_it_streams…`); e2e
  `chat-documents.spec.js`; model evaluations `--documents`
  ([model-evals.md](../model-evals.md#documents-beside-the-chat-30-oct-2026)).
- **`chat/documents/rich.rs` + `chat/documents/blocks.rs`** — **the rich
  document tools** (2026-10-31; the contract of Claude's own document tools,
  simplified for Yap: nothing of its code). **Which models**
  (`ToolSet::for_model`, `rich::handles_rich`, pure): never one on this PC
  (the on-device model and a local server keep the simple set: the
  evaluations decide, model-evals.md); a cloud or self-hosted model by its
  family in its id — Claude, GPT-5 and GPT-4.1 (not nano), o3 / o4, Gemini
  2.5 and 3 (not Flash-Lite), Grok 3/4, Kimi K2, DeepSeek V3, GLM 4.5/4.6,
  Qwen3 Coder / Max, Mistral Large, gpt-oss-120b, through any provider —
  or by the size its id names (≥ 70B); a model evaluation's `--doc-tools
  rich|simple` overrides it. The set is fixed per model, so the system
  prompt stays the same from turn to turn (the cache): `tools::
  tool_instructions(set)` swaps the documents' line for `rich::INSTRUCTIONS`,
  `notes::definitions_for(set)` the schemas (`Extra::doc_tools`). **The
  rich set** keeps `create_document` and `write_section` (the outline first,
  as before) and has, in place of `rewrite_section`: **`read_document(note_id,
  since?, offset?, limit?)`** — the document as blocks (`blocks::parse`: a
  heading, a paragraph, a whole list over its blank lines, a whole table, a
  code block with its blank lines, a quote, a rule, a section still to
  write shown as `{type: "section_to_write", key, intent}`, never the
  comment's syntax), each with an **`id`** and a **`hash`**, and the
  document's **`rev`**; a page at a time within `note_text::PAGE_TOKENS`
  (≤ 80 blocks, `next_offset`); **`since`** a rev gives only what changed,
  block by block (`blocks::changes`, LCS over the hashes): `{unchanged: n,
  first, last}` runs and `{change: added | removed | replaced, was: [{id,
  text}], now: [blocks]}`, or `unchanged: true`; a rev not kept, the whole
  document. **How the ids stay put** (`blocks::identify`): an id is the
  block's kind letter and 5 hex characters of an FNV-1a hash of its text
  (`p3fa9c`; trailing spaces and `\r` aside), then `-2`, `-3` for a second
  block with the same id — nothing of its place or its neighbours, so an
  edit elsewhere leaves every other id as it was, the same text reads with
  the same ids after a restart, and a changed block gets a new id. The hash
  is 8 hex characters from the hash's other bits (two blocks whose short
  ids collide never pass for each other). **The rev** is the number of the
  newest version of the note's notes, and a read **seals** it
  (`versions::seal`): the person's typing after it is a version of its own,
  so `since` compares with what Chat saw (the text read is also kept in
  memory per rev, the newest 6 a document). **`edit_document(note_id,
  ops)`** (`blocks::apply`, pure; `ops_arg` takes the names models use):
  `insert` (markdown after or before a block id, or at the end), `replace`
  (id, `if_hash`, markdown), `delete` (id, `if_hash`), `move` (`ids`,
  `if_hashes`, after or before an id) — in order on a working copy, **all
  or none**, one `chat` version; inserted markdown keeps the document's line
  endings, never carries a placeholder line, and gets a blank line wherever
  it would run into a block; a renamed `# title` heading renames the note.
  **The person's words win**: an op names a block by the model's own
  picture of the document (`saw`): `blocks::resolve` lines the picture's
  blocks up with the document's by hash, so a block it saw that's unchanged
  is found even when its id shifted (an identical block deleted above it),
  and one the person changed or removed is **refused, with what stands in
  its place now** (`data.now`: those blocks with their ids and hashes:
  "Op 1 (replace): the user changed block p3fa9c since you read it… Their
  words stay"); an id the picture hasn't got (read from a page since) is
  taken as it is; a wrong `if_hash` is refused with the block as it is;
  a block Chat's own earlier edit replaced or deleted (the same edit sent
  twice: the 4B did) is "done already", with what stands for it now
  (`rich::retire`, the newest 40 a document, in memory); each refusal
  names its op, and nothing changes. The result gives the new
  rev and each op's new blocks (`now: [{id, hash}]`); the model's picture
  becomes what it knew with its own edit made (`merge3` over the
  document, when the person changed something it hasn't seen), so the next
  question still hears the person's changes. A table row added or changed
  is a `replace` of the whole table (the strong models rewrite a table
  well, and a row op would need cell parsing for nothing; not added).
  **`document_history(note_id)`** — the versions of its notes, newest first,
  ≤ 20 (`{version, at, by: "Chat" | "the user typed" | "Chat restored
  version 3" | …, blocks, change: "blocks: 1 changed, 2 added", current}`);
  **`restore_document(note_id, version)`** — `versions::restore_by(…,
  "Chat")`, itself a version (`restored`, labelled Chat: the panel's Undo
  takes it back), refused while the person has changed the document since
  Chat last read it ("read_document first"); an **action tool** for a model
  that isn't a strong one (refused after web content), while a strong model
  — every model the rich set goes to — may restore after the web, as it may
  edit (2026-10-31: the restore is shown and is itself undoable); and
  `edit_document` is the answer's own writing like `rewrite_section`.
  `edit_document` and `restore_document` change only this chat's documents
  (`Turn::refuses`); all four work only on a note Chat made (`source`
  "chat"); `read_note` / `edit_note` stay for any note. Chips: "Read the
  document "…"", "Changed a table in "…"", "Changed a part of …", "Added
  to …", "Took a part out of …", "Moved a part of …", "Made 2 changes in
  …", "Didn't change "…": the user changed that part", "Looked at the
  history of …", "Restored version 3 of …". **What the model is told**:
  the rule (`rich::INSTRUCTIONS`: read, then edit by id with the hash read;
  a table row is a replace of the table; a refusal means the user changed
  that block, build on it; history and restore when asked); the next
  question's documents line with each one's `rev` (sealed then too) and
  "read_document (since a rev…), then edit_document"; `steer`'s change line
  saying so (`set`), and **an earlier version asked back**
  (`restores_version`: "put it back the way it was", "undo that change",
  "go back to the previous version") → "document_history, then
  restore_document … not a rewrite from memory" (the rich set only).
  **A passage selected in the panel** ("Change this…", "Ask about this";
  `documents::Selection {noteId, text, fromLine, toLine}`, `chat_send`'s
  `selection`, a task-local around the turn: `with_selection`):
  `selection_note` in the question's context in place of `steer`'s line —
  the document, the words, and the blocks they're in (`rich::
  selected_blocks`: by the page's lines, else by the words, markdown marks
  aside), for the rich set their ids and hashes and their markdown ("change
  only those blocks (edit_document…)"), for the simple set their markdown
  to copy `edit_note`'s old_text from, for a model without tools the words.
  The question keeps it (`ChatMessage.quote {noteId, text}`, ≤ 600
  characters; `chats::set_quote`), shown above it, and the history the
  model reads later says it (`ChatMessage::sent_text`: "(About this passage
  of "…": "…")"). An older Yap's save drops `quote`: the question stays,
  without its quote. **However the question comes** (2026-10-31): typed,
  or dictated into the box — the dictation shortcut with the box focused,
  its mic, sent by "Talk back hands-free" or a spoken "press enter"
  (`deliver_to_chat` → `yap-chat-dictation` → the page's `send`) — it takes
  the quote and the chip goes, as a typed one does; and **asked out loud
  with the Voice Assistant's shortcut** too: the page reports the quote
  over its box (`chat_quote_set`, `documents::WaitingQuote`, in memory;
  `chat_send` with a `selection` clears it), and a spoken question takes it
  (`take_waiting_quote` → `with_selection` around its turn; `yap-chat-quote
  {quote: null}` and the chip goes; no answer → `put_back_quote`, back over
  the box) **when Chat is in view as it's asked and the question goes to
  the conversation it's quoted in** (`quote_goes`, pure: `in_view` is
  `assistant::answer_in_view`). Why: the person who selected a row, chose
  "Change this…" and then says "make this level 21 to 22 only" is looking
  at the quote and asking about it — the maintainer's try (31 Oct 2026,
  Claude Haiku 5.5) went without it, the model read the whole document to
  find the row, and the chip stayed over the box after the answer; asked
  from another app, the question is about something else, the person
  can't see the chip, and the quote stays for the box. With the quote, the
  window in front isn't looked at either (the passage is what it's about:
  no "is this about the screen?" call). A tool step with the rich set weighs 3,525
  tokens (`the_rich_document_tools_are_named_and_stay_light`). Unit-tested
  (`blocks`: the cut, ids staying put, what changed, every op, all or none,
  the person's change refused, a shifted id found; `rich`: the gating, a
  read, pages, `since`, a table row, a stale edit, a move and the title,
  history and restore, a selection's blocks, `edit_document` never bringing
  back or making up a placeholder; `documents`: the sets, the context,
  `steer`, the selection line, `refuses`, when a waiting quote goes); e2e
  `chat-documents-rich.spec.js`; evaluations `--documents --tag
  documents-edits --doc-tools rich|simple`
  ([model-evals.md](../model-evals.md#the-rich-document-tools-31-oct-2026)).
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
  written), `waiting {waiting}`, `thinking {text}` (a question asked to
  think it through, `chat/think.rs`: the thinking so far, apart from the
  answer), `document {noteId, title}` and `section {noteId, key, text}`
  (documents beside the chat, `chat/documents.rs` above); `chat_send` adds `turn {conversationId,
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
  `stream::STOPPED`, the question kept (as after a failure). **An answer
  that starts repeating itself stops there** (`Assembler`'s loop guard,
  `repeating_from`, 2026-10-26: a small on-device model answered a
  spoken question with the same paragraphs over and over for half a
  minute): every 48 bytes of content the end of the answer is checked
  for a block repeated back to back — two copies of 160+ bytes, or four
  of a short phrase covering 480+ — and the answer ends after its first
  copy, cut at a sentence or line end, as if the model had finished
  (`finish_reason` "stop"; logged with lengths only). Lists whose lines
  share words but differ, and ordinary prose, never trip it (tests).
  **On-device answers are capped** at `llm::ON_DEVICE_ANSWER_TOKENS`
  (4,096 `max_tokens`, Chat's calls to the sidecar only): a model that
  runs on anyway can't hold the graphics card for minutes. **A newer
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
- **`chat/think.rs`** — **Think it through** (2026-10-29): Chat's **Think**
  button (`views/chat/ThinkButton.svelte`, beside the mic; `chatThink.svelte.js`)
  asks **one question** with the model thinking before it answers, then goes
  off again; it shows only for a model that can (`chat_think_info {can,
  place}`, asked again when Chat's model changes). **On by itself for a
  strong model's document** (2026-10-31, `think::auto_on`): every Chat turn
  runs inside the task-local (`think::scoped`, off unless pressed), and a
  turn where a strong model writes a document (`documents::writes_document`)
  switches it on — high effort, adaptive thinking for Claude — and tells
  the page (`{kind: "think", auto: true}`: the button shows lit, `think.auto`,
  until the answer is in). Never the Voice Assistant (no scoped turn around
  it), never the on-device model (not a strong one). `chat_send(think: true)`
  runs the turn with it on from the start, and **only Chat's own
  requests read it** (`llm::ChatRequest::chat`: `extra_for`): cleanup,
  Commands, the meeting notes, memory, a summary and the Voice Assistant
  never think. **On this PC** the sidecar keeps `--reasoning off`
  (`local_llm::server_args`), and one request still thinks **without a
  restart** — measured on llamafile 0.10.6 with Qwen3.5-4B (Q4_K_M, MTP,
  `--reasoning off`, 29 Oct 2026): `chat_template_kwargs: {"enable_thinking":
  true}` turns it on for that request alone (the next request without it
  answered in 0.1 s as before); `thinking_budget_tokens` caps it (uncapped,
  the 4B thought for 4,096 tokens about a bat and a ball and never answered;
  capped at 300 it answered in 1.1 s; at 1,500 a logic puzzle took ~1,350
  tokens, 5.3 s, MTP drafting throughout); `reasoning_budget_message` is the
  line it gets when the budget runs out (without it the answer began
  "</think>"; `think::tidy` drops one anyway). `reasoning_effort` changes
  nothing there and `reasoning_budget_tokens` didn't cap it. So: 1,500
  tokens (`ON_DEVICE_BUDGET`), the answer's cap raised by as much, only for
  Qwen3.5 (`on_device_thinks`; not the 1.5B, nor Qwen3's 2507 instruct). **In
  the cloud** (`request_extra`): OpenAI's reasoning models and Groq's
  gpt-oss `reasoning_effort: "high"`, Gemini 2.5 and later the same (its
  OpenAI-compatible endpoint maps it to a thinking budget), OpenRouter's
  `reasoning: {effort: "high"}` (its list's `thinks`, else the families),
  Claude adaptive thinking with `output_config: {effort: "high"}` — sent on
  Anthropic's Messages API since 31 Oct 2026 (`llm/anthropic.rs`, every
  strong Claude's Chat step, its thinking's summary shown: `display:
  "summarized"`): Anthropic's docs now say its OpenAI-compatible endpoint
  answers `{type: "adaptive"}` with HTTP 400 and never returns the thinking
  — and a 4,096-token budget on a Claude before 4.6. A
  thinking request waits up to 150 s (`think::TIMEOUT`); the GPT-6 rule
  (tools only with reasoning off) still asks again with `"none"`. **What
  shows**: the thinking comes apart from the answer (`reasoning_content`,
  or Groq's and OpenRouter's `reasoning`; `stream::Assembler` keeps it out
  of the message, `llm::post_chat_request` hands it to `think::heard`), as
  `{kind: "thinking", text}` events (each step's after the turn's earlier
  ones, `think::so_far`; only in a turn thinking it through: Groq's gpt-oss
  sends its reasoning unasked, and any other question shows none of it, as
  before Think): "Thinking…" with "Show its thinking" folded under
  it, then "Thought for 6 s" folded above the answer (`ChatMessage.svelte`
  `.thought`). Kept with the answer (`ChatMessage.thinking` = `Thought {text,
  ms}`, `chats::set_thinking`; the reply's `thinking`), never sent back to a
  model; an older Yap's save drops it, the answer stays. Anthropic's and
  OpenAI's compatible endpoints never send the thinking: only the time
  shows. Unit-tested (each provider's fields, the turn's task-local, a
  stray end tag, the stream's thinking); e2e `chat-think.spec.js`.
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
  scope "assistant" (its own smaller history budget, `budget::Kind::Assistant`:
  35% of the context, at most 6K tokens, with the conversation's summary
  when it has one); otherwise no Chat model
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
  **Unless the answer is in view in Chat**, a card on the Yap bar
  (`style: "answer"`, id `assistant`: the question, the answer's markdown
  ≤ 1,200 chars, a note "Not read aloud during a meeting", Open in Chat;
  ✕ / Esc = `stop`) — **"Show the answer on the Yap bar"**
  (`assistant_answer_card`, on) decides it for a spoken answer; one not
  read aloud always shows (`card_on_bar`, pure), also when the voice
  fails —, shown before the voice starts; it goes when the
  voice ends (`on_tts_state`), or after `read_ms` when shown only. **In
  view** (`answer_in_view(seen, viewing, conversation)`, pure, 30 Oct
  2026): the main window in front (`notice::window_seen` = `Front`: Chat
  comes up there with the question), or on screen beside the app in front
  (`Beside`: another monitor, at least half of it uncovered) with the Chat
  view on the question's conversation (or a new chat, which follows it);
  minimised, hidden, behind another app (`No`) or on another view, the
  card. The card is for someone who can't see Chat: the maintainer asked
  out loud with Chat maximised and the answer streaming in front of him,
  and the card still came up — Windows' real state now, never the page's
  `document.visibilityState` (WebView2 says "visible" while hidden), nor
  tao's `is_focused` (the webview's keyboard focus). Decided as the answer
  comes, not when it was asked: someone who switches to another app before
  it comes gets the card (`speak_ready`; the whole answer's
  `finish_streamed` shows one then if none went up while it was said and
  Chat is out of view now). A note on an answer not read aloud ("Not read
  aloud during a meeting") goes with its card: with Chat in view, the
  answer's there and no voice says it either. **The assistant's alerts**
  ("The Voice Assistant couldn't answer", "Pick an AI model…", "The Voice
  Assistant is off during meetings", "can't read images") keep the notice
  policy's one place (`notice::place`: the window's toast while it's in
  front, else the bar): they ask for something (Open settings), and Chat
  beside another app doesn't show them. E2e: `voice-assistant.spec.js`
  ("the answer's card shows only when Chat isn't in view…": beside on that
  chat none; behind, on another view, or left before the answer came, the
  card; `e2e_focus {focused: false, beside: true}`). **A web
  answer** (`turn.sources`, `web.rs`): the model is told a spoken question
  wants the answer first. **Any other spoken answer** is asked for short
  (`SPOKEN_ANSWER_NOTE`, last in Chat's system prompt for the Voice
  Assistant, or a question dictated into Chat with spoken replies on: "one
  or two short sentences (under 40 words) of plain speech…" — the model
  evaluations' egg answer was 55–98 words with lists before, 12–17 after).
  **How much of an answer is said is the person's choice** (2026-10-26):
  Settings → Voice Assistant → Spoken replies → **How much to read aloud**
  (`spoken_length`, `SpokenLength`): "A sentence or two" (`short`:
  `tts::brief`, its first two sentences), "About 30 seconds" / "About a
  minute" (the default) / "About 2 minutes" (`30s` / `1m` / `2m`: 75 / 150
  / 300 words, ~150 a minute — all of it up to that, else its paragraphs
  from the first while they fit, `tts::lead`: whole paragraphs or list
  items, so it stops at the end of a thought; a first paragraph longer than
  that, its sentences while they fit, two at least), or "The whole answer"
  (`whole`). Cut short, it ends "The rest is in Chat." (a web answer: "The
  details and sources are in Chat."); no `[n]` is ever said; the card
  shows what's said (a web answer's without its markers) and "2 sources in
  Chat". Chat's own voice replies follow the same setting (`chat_send`'s
  `spokenText`, `spoken_in_chat`: "The rest is on screen."); ▶ Read aloud
  on a message always reads all of it. (Until 2026-10-26 every answer over
  60 words was said in two sentences, and every web answer short: a
  95-word answer in two paragraphs stopped mid-thought.) `stop`
  (■ on the capsule = `bar_action "assistant-stop"`, `assistant_stop`;
  Chat's ■ and Esc too): the voice stops **and the answer's stream
  closes** (each question's turn has a `Stoppable` called
  `assistant-<generation>`, 2026-10-26; before, ■ only silenced the voice
  and a model on this PC went on writing) — what was written stays in
  Chat, as after Chat's own ■; stopped before anything came, the question
  stays and no alert shows. `assistant_phase` tells a Chat view opened
  after the question where the assistant is, so its ■ shows.
  The bar's capsule shows while `phase != Idle`, the bar on or off.
  Debug-only `e2e_assistant_ask {text, listenMs}` (`Robot::ask_assistant`:
  the press's own steps — barge-in, the target, "recording" for
  `listenMs` — with `text` as the transcript). The question ends when
  the shortcut is pressed again, or let go in push-to-talk, never on its own.
  ("Stop listening when I pause", added 2026-10-23, was **removed on
  2026-10-25**: a pause to think ended questions half-asked; the maintainer
  asked for it gone. Its `dictation/pause.rs`, the bar's run-down line, the
  setting, `e2e_assistant_audio` and the specs went with it.)
  **Screenshots** (2026-10-25, [snip.md](./snip.md)): a question asked while
  screenshots wait in Chat's box takes them (`snip::waiting::take_for_question`)
  — sent as a typed question's are, saved with it (its bubble's thumbnails),
  the box cleared, the answer spoken as usual; a model that can't read
  images: nothing goes, the question waits in the box with them
  (`cant_read`). **After a spoken "snip"** lands in Chat a question starts
  by itself (`pipeline::listen_after_snip`; "Then listen for my question",
  `snip_listen`): `yap-assistant {about: "screenshot"}` makes the capsule
  say "Ask about this screenshot…"; either key or ✓ ends it, a push-to-talk
  key joins it and its release ends it (`gesture::State::Joinable`); Esc in
  Chat (`assistantListening`) or ✕ cancel it; never while the assistant is
  off (`is_off`). Test runs stage its transcript (`e2e_snip_question`).
- **`chat/assistant/screen.rs`** — **the Voice Assistant sees the window in
  front** (2026-10-29): a spoken question about the screen ("what's this
  error?", "summarise this page", "what does this chart say?") takes a
  screenshot of the window that was in front when the shortcut was pressed
  (`window_in_front`, from `pipeline::capture_target`), and the model answers
  from it. **Never for every question**: cheap rules first (`about_in`,
  pure, in the person's own languages — My languages' codes, English always:
  Dutch "dit" is French for "said"): phrases that point at the screen ("on
  my screen", "what's this?", "qu'est-ce que c'est", "was ist das?",
  "summarise this") and a demonstrative before a screen word ("this
  error", "cette page", "dieser Fehler") say **yes**; "this" alone, or
  "that error", is a **maybe**; anything else **no** ("this weekend" isn't
  the screen). A maybe is the model's call (`model_says`: one short request
  to Chat's own model, `ASK_PROMPT`, with its last answer, so "is this
  right?" after an answer is about the answer; 4 tokens on this PC, gpt-oss
  at low effort; anything but a clear yes is a no). On the real Qwen3.5-4B
  (29 Oct 2026): 14 of 16 such questions right, median 180 ms, slowest 0.5 s.
  **What's taken** (`capture`): the window's visible frame
  (`snip::pick::window_frame`: DWM's frame; not one of Yap's own, minimised,
  cloaked or gone), cut from every monitor captured at once as a snip is
  (`snip::cut_frames`, HDR right), every Yap window left out for the moment
  (`calls::capture::for_snip`: the bar's capsule too; meeting windows as the
  meeting setting wants — and the assistant is off during meetings anyway),
  saved as a chat screenshot (`snip::keep_as`, source "window"): it shows on
  the question in Chat and is kept as Chat's screenshots are — sent again
  with a later question only where the window itself may go now
  (`window_may_go`: Chat's model switched to a cloud one, it's a line, not
  a picture, without "With cloud models too"). **Where it
  goes** (`allowed`, `leaves_pc`: the cloud providers always leave the PC,
  any other server unless it's on this PC): "Let the Voice Assistant see the
  window in front when you ask about it" (`assistant_screen`, on) for a
  model on this PC; a cloud model only with "With cloud models too"
  (`assistant_screen_cloud`, off). The first time a question about the
  screen meets a cloud model the alert asks once (`ask_once`, kind
  `assistantScreen`, **Allow** = `alert::ALLOW_SCREEN_CLOUD`;
  `assistant_screen_asked`), and the model is told it can't see the screen
  (`not_sent_note`), so it says so rather than guess; a maybe isn't asked
  about when nothing could go. **With it**: the picture through the typed
  path's steps (`turn::prepare`; the on-device model loads its projector
  first, `vision::ensure`) and a line in the question's context saying what
  it is (`PICTURE_NOTE`, `snip::turn::TurnImages.note`, read by `chat_turn`);
  a model that can't read images (`vision::reads_images` false, or the
  projector won't load, or the picture couldn't be fitted) gets the window's
  text, read on this PC (`snip::ocr::question_with_text`, the screenshot
  marked `asText`, `TEXT_NOTE`), and the answer names it in its sources
  (`text_source`: `Source.screen`, "The window's text, read on this PC", a
  chip that opens nothing; `snip::turn::with_source`); no window or no text:
  `NO_WINDOW_NOTE`. A server that turns the window's picture down makes that
  model a "no" (`vision::refused`). Not beside screenshots already waiting
  in Chat's box (they're what it's about), nor when Yap opens an app. The
  capsule says "Looking at the window…" meanwhile (`yap-assistant {about:
  "window"}` while thinking). **Network receipts**: the turn runs as
  `net::Feature::AssistantScreen` — "Voice Assistant + the window in front ·
  Groq". Logs: the rule's verdict, sizes and timings, never the question or
  the screen. Unit-tested (every rule in seven languages, the person's
  languages, the yes/no request and reply, where it may go); e2e
  `assistant-screen.spec.js` (fake monitors and a fake window in front
  through `e2e_snip {start: false}`, `snip::take_fake`: never the real
  screen).

## Strong models' documents, researched as the Claude app does (2026-10-31)

What the maintainer saw: Claude Opus 5.5, asked in Yap and in the Claude app
for the same detailed level-by-level mage guide, wrote a complete guide in
the Claude app and, in Yap, a table of "Level 1", "Endgame" and "Reference
only" rows ("the levels in between aren't covered by a source I found"); asked
for more detail, it said "That search only turned up forum threads… It
didn't open any of them", "Didn't search for …: 3 searches a turn" and
"Since I've just been on the web, I can't edit the document this turn".
Yap's own rules held it back. For **strong models only**
(`documents::rich::strong`, one place: the rich tools' list — Claude, GPT-5,
Gemini, the large open models; never one on this PC), and nothing changes
for the on-device model and small ones, which make things up more readily:

- **Edits after the web** (`tools::refused_after_web(name, web_seen,
  strong)`). Notes and documents change after the web as in the Claude app:
  `create_note`, `update_note`, `edit_note`, `create_document`,
  `write_section`, `rewrite_section`, `edit_document`, `restore_document` —
  each shown (its chip, the panel's highlight) and undoable (a version; the
  panel's Undo takes the whole turn back). **Kept locked** (`STRONG_LOCKED`):
  `open_app` (it runs something on the PC), `copy_to_clipboard` (it replaces
  what the person copied and goes wherever they paste next), `remember` and
  `forget` (memory goes with every later chat, and `forget` deletes) — effects
  outside the notes that the person can't see or undo as they can a note.
  Yap has no tool that deletes a note or sends anything anywhere; one added
  later belongs in the list. The prompt says so (`STRONG_WEB_TOOL_INSTRUCTIONS`:
  "writing and changing notes and documents goes on as usual"); the old line
  ("Yap won't … change notes") stays for every other model.
- **Research depth** (`web::Reach`). A strong model's turn that writes a
  document — something to keep, a change or more depth on this chat's
  document, an option picked, a change to a selected passage
  (`documents::writes_document`; and any turn once it calls a document tool,
  `Turn::go_deep` from the loop) — may make 10 searches (`DEEP_SEARCHES`) and
  as many page reads as two-fifths of the model's request holds, 3 to 10
  (`Reach::deep`, sized by `budget::budget`'s request: Claude Haiku 5.5, held
  to its cheaper rate's 80K, gets 10); every result still fits what's left
  of the request (`agent::Room::fit`). Its context says so
  (`documents::research_note`: "research it properly first — up to 10
  searches and 10 page reads … read_page the best pages in full"), and the
  cap's refusal says to write the document, the rest from its own knowledge.
  Every other turn keeps 3 and 3.
- **Its own search, in its own turn.** A strong **Claude** on Anthropic
  (`llm::anthropic::speaks_messages`) speaks the **Messages API** for Chat's
  steps (`ai/llm/anthropic.rs`, see ai-models.md): Anthropic's **web search
  and web fetch go in Chat's request** beside Yap's function tools
  (`web::native::NativeSearch::in_chat`, `server_tools`) — Claude searches
  and opens pages itself, as in the Claude app, and Yap's `web_search` /
  `read_page` aren't offered (`CLAUDE_WEB_TOOL_INSTRUCTIONS`). Its searches
  and pages become the turn's chips ("Searched the web with Anthropic: …",
  "Read emberfall.example/…", "Couldn't read …: the page didn't open") and
  sources, its text's citations Yap's `[n]` pills (`Turn::took_server`, before
  any of the reply's tool calls run: the safety rule sees the web first); a
  document cites a page as a link to it. Other providers keep the sub-call
  (web-and-apps.md), deeper for a document's turn.
- **The model's own knowledge, marked** (`rich::INSTRUCTIONS`). A guide or
  plan is complete: "a level-by-level table has a row for every level asked
  for (never "endgame" or "reference only" rows in their place)". Cited
  where the web gave it; where sources run out, filled in from the model's
  own knowledge and **marked with one italic line under that table or
  section** ("*Levels 5–19: my suggestions, not from a source.*") — not a
  column (the panel is narrow and the cited rows already carry their links),
  not a footnote (Yap's markdown has none). The checks against made-up
  sources stay: an answer's unsourced `[n]` goes, a document's `[n]` becomes
  a link only to a page the turn found (`documents::link_citations`).
- **Think first** (`think::auto_on`, above): the turn thinks before it
  writes — high effort, adaptive for Claude — the Think button lit for it.

Measured by `yap.exe eval --guides` (docs/model-evals.md, "Guides"): every
level, the parts asked for, own suggestions marked, citations real, the
document changed after the web. e2e: `chat-documents-research.spec.js` (an
edit after the web for a strong model, refused for a 4B-sized one, an app
still refused, the cap a document's turn only, Claude's own search in its
request with Think on).

## The Chat view

`src/lib/views/chat/` (split 2026-10-24): `ChatView.svelte` is the view —
it makes `chatState.svelte.js` (the conversations, sending and the streamed
answer with ■ and a newer question replacing one, the Voice Assistant's
questions, Snip to Chat and whether the model reads images, spoken replies,
several picked at once), follows Rust's events while Chat is open, takes
its keys and shows a screenshot large. Its parts: `ConversationList` (the
list, its menus, rename in place, the selection bar, the search box and the
Archived button; its search results and the archive are
`ChatLibraryList.svelte`, loaded the first time one shows, over
`chatLibrary.svelte.js`), `ChatThread` (the
empty state, "Thinking…") with `ChatMessage` (tool chips, the bubble,
Stopped, Sources, ▶ and the model under an answer) and `Composer` (the
screenshots and their prompts, +, the box, the mic, Send / ■, `ModelChip`).
[frontend.md](./frontend.md#the-folder-layout-2026-10-24).

**The document panel** (2026-10-30, `DocPanel.svelte` + `chatDoc.svelte.js`;
`chat/documents.rs` above): a document a turn makes or changes opens to
the right of the conversation. **Opens** on the turn's `document` event
(the outline at once: each section still to come a quiet grey line, its
name and what it will say — "[**Gear**: what to wear and where to find
it]", after Claude's own documents' grey intents; `markdown.js`'
`md-pending`, the brackets drawn by the panel's CSS — never the
placeholder's syntax), **fills** as `section` events stream a
`write_section`'s text into its placeholder's place, or a
`rewrite_section`'s over the section it rewrites (a new one at the end;
`docStream.js` `shownSegments`, `docSections.js` `sectionRange`;
half-written markdown closed off), and takes the note's text as each
section lands (`yap-note-changed`, origin "chat"). **Stream-in**
(2026-10-31, after Claude's documents filling while you watch): the text
comes in bursts — Rust sends a call's arguments so far at most every
50 ms, the fake model's in thirds, a provider that won't stream the
section whole — and the page lets it **flow in line by line** at a pace
that follows how far behind it is (`revealStep`: at least 90 characters a
second, else the backlog over 0.3 s, so a burst is caught up in about a
second; a frame at a time, a timer standing in when frames don't come; a
section's own heading shown whole from the start, `revealFrom`; never
inside an emoji, `revealCut`). Each section on its way is `chatDoc`'s
`live` (key → its text as it came); one that **lands** stays live as
saved (`settled`, found by its own heading, `sectionText`) until the
panel has all of it shown (`release`), so the end of a section never
jumps in; while a turn writes, a section a provider sent whole flows in
the same way; one changed again after it landed (a later edit) is shown
at once with the change marked. A **caret** sits at the end of the text
being written: a thin bar in the accent with a small tab over it,
"Chat" (Claude's documents label theirs "Claude"; in Yap the writer is
Chat whichever model answers, as "Undo Chat's last change" says), one
element moved as the text grows, its label drawn by CSS
(`::after { content: attr(data-who) }`) so find and a copy never meet
it (`docPieces.js` `placeCaret`); the page **follows** it while it's in
view, and stays put once you scroll away. **The page is drawn a piece
at a time** (`docPieces.js`: `splitSections` cuts it at each `#`/`##`
heading and placeholder outside code — where a block always starts, so
each piece draws as it would in the whole — and `drawParts` keeps every
piece whose HTML is unchanged, setting its blocks' `data-line` to the
page's): only the section being written is drawn again as it grows, so
a long document costs nothing per frame and a selection elsewhere stays.
**Motion reduced** (Settings or Windows): the text shows as it comes,
the caret still there. **A page to read** (2026-10-30, measured against Claude's own
document view, none of its code or CSS): a reading-width column (68ch,
centred when the panel is wide or expanded); the title large in EB
Garamond (`--yap-font-display`, 27px × Text size) and each section's
`##` (rendered as h3: `markdown.js` shifts headings a level down) in it
too, 21px with 28px above — the serif scale, chosen over Claude's sans
file view for Yap's paper look — `###` and below in the reading font
(15px, then the body size, semibold); 15px between blocks; the summary
under the title a touch larger, in the second ink; **bold** lead-ins in
the ink; links in `--yap-link`; tables of filled cells, no rules (the
header row `--yap-paper-s2`, the body `--yap-paper-s1`, 2px apart, 3px
corners; a header row of empty cells hidden; a wide table scrolls inside
itself); light and dark from the `--yap-paper-*` tokens. **The sections'
rail** (after Claude's document outline): a document of three sections
or more has a thin rail of ticks in its left margin, one per section, the
one being read longer and in the accent; hovered or clicked ("Sections"),
it lists them, and a click scrolls there (`data-testid="chat-doc-outline"`).
The Notes view shows a document's text as its plain markdown in My
thoughts' text box (no rendered view there yet). Its header: the title
as a small label (the page has it large; the body's own `# title` line
left out), **Undo** when the newest change is
Chat's — its writing, or a restore it did (`kind` "restored", `label`
"Chat") — back to the version before **Chat's last turn** (`undoTarget`:
every version with the newest one's `turn`; `note_version_restore`, itself
a version). A turn writes a document a call at a time — the outline, then
each section, each a version — and Undo used to go back one call: after
"Wrote 5 sections" it brought the last section's placeholder back ("1
section not written yet" and "Tips and Caveats: not written yet", the
maintainer's Claude Haiku 5.5 document, 31 Oct 2026; the log showed all 5
written, then a restore of the version before the last one, then the next
turn's table edit keeping the placeholder it found). A document the last
turn made has nothing before it: no Undo (delete it in Notes). A version
from an older Yap (no `turn`) is a turn of its own. **Preview / Source** (2026-10-31, below), **expand** (covers the
whole of Chat; Esc or the button puts it back), **⋯** (Find in the
document, Copy as Markdown, Export as Markdown…, Open in Notes) and ✕. A
status line under it: "Writing: Gear…" while a section streams, "Editing…"
while Chat changes it by block (`edit_document`, `restore_document`: the
`tool` event; a pulsing dot, still under reduced motion), else "2 sections
not written yet" for a document Chat stopped short of finishing.

**After Claude Code's markdown file view** (2026-10-31; measured from the
Claude desktop app's file panel and its document page, `.epitaxy-*`, none
of its code or CSS). **Two views**: **Preview**, the page (editable once
Chat has written it: below), and **Source**,
the markdown itself, coloured (`core/markdownSource.js`: heading marks,
list and quote marks, a table's pipes, code, **bold** underlined, links, a
section still to write — colour only, never a bolder or slanted face, and
spans that add no character) under a clear text box that takes the typing
— the two share every measure, so the caret and the selection sit on the
coloured words. **Source took the Edit box's place** rather than sitting
beside it: two raw views would say the same thing twice, the old box was
already the markdown, and Claude's file view has exactly Preview and
Source; it saves as the box did (600 ms after typing, `chat_document_save
{id, base, content}`: Rust lays the person's edit over whatever Chat wrote
meanwhile, `documents::merge3`; a save that comes back while more was typed
keeps the newer typing and saves again from what it was typed over; it
follows Chat's writing while nothing is waiting to be saved, the caret
kept). The switch is a pair of buttons (`aria-pressed`, ←/→ between them);
Source takes the focus. **Find** (Ctrl+F in the panel, or ⋯): the shared
find bar (`SummaryFind`, "Find in the document"), every match lit in either
view, across the text's pieces, "2 of 5", Enter / Shift+Enter, Esc closes
it (not the expanded panel). **Copy as Markdown** (the whole document, its
sections still to write left out: `copyableText`) and **Export as
Markdown…** (the Notes view's `note_export`, a .md where you choose).
**Selected words** on the page (it's selectable, unlike the app's chrome)
bring a small popover over them, **"Ask about this"** and **"Change
this…"**: the passage goes over Chat's box as a quote ("Change in Lone
wolf ranger: …", ✕ takes it away; the box asks "How should it change?")
and is sent with the next question (`selection {noteId, text, fromLine,
toLine}`: the lines from the page's `data-line`, `renderMarkdown(…, {lines:
true})`, plus `shownOffset` for the title line; not while a section
streams in, when Rust finds the blocks by the words), then shown above the
question in the thread — typed, dictated into the box, or asked out loud
with Chat in view (the page reports it, `chat_quote_set`, and follows
`yap-chat-quote`; `chatState.svelte.js` `reportQuote` / `onQuote`). **A change that lands** (Chat's edit, Undo) has
its blocks washed in the accent a moment (`data-changed`, 1.6 s; the wash
held, no fade, under reduced motion) and the first scrolled into view if
it's off screen (the blocks new since the last drawing, by their HTML).
**Each document's scroll** is kept per view while Yap runs (closed and
opened again, switched to Source and back). (A change that lands the
moment a turn's `document` event reloads the note, before the note's own
`yap-note-changed`, is marked too: `load` counts it.) Lighter details from the same
study: two headings in a row sit closer, paragraphs and list items wrap
`pretty`, a code block gets a filled box, the table's header row balances
its wrap. **Not yet: pinned comment threads** (Claude's comments on a
passage). They'd build on the same pieces: a comment anchored to a block
id plus the words selected (the id finds it while the block is unchanged;
the words find it again after an edit, else it's marked "outdated"), kept
beside the note (`note-comments/<id>.json`, never notes.json, which an
older Yap rewrites), shown in the page's margin; the open threads would go
to the model in the question's context like the selection does, with a
`reply_to_comment` / `resolve_comment` pair, and "Change this…" would
become one kind of thread.

**A page you can edit, with a toolbar** (2026-10-31, after Claude's own
documents, which edit on TipTap and ProseMirror — nothing of their code;
the toolbar's commands read from their bundle: text style, code, table,
image, checklist, + insert). **Preview is an editor** once nothing's
flowing in (`doc.streaming` false: no turn writing, no section still
flowing in); while Chat writes, the page is drawn as stage 1 draws it and
the toolbar waits ("Chat is writing…", its buttons off) — typing while
Chat writes goes in Source, merged as before. **The editor**
(`docEditor.js`, TipTap 3 on ProseMirror, MIT; its own chunk, imported
the first time it's wanted — the panel opens on the drawn page and swaps
the editor in, ~130 kB gzipped on top of the panel's 10): its root is the
page's own `.docmd` (`element: {mount}`), so it reads exactly as the page
does (headings a level down with `data-level`, tables in
`.tableWrapper` styled as `.md-table`, a checklist item's box where its
bullet was, the sections still to write as their grey lines); TipTap's
own stylesheet is left out (its colours aren't Yap's: the gap cursor,
selected node and cells are drawn from tokens). **Why TipTap, not
Milkdown**: Milkdown's markdown goes through remark, a second markdown
reader beside markdown-it that writes markdown back its own way (lists
re-marked, tables re-padded), and Yap's own rules (placeholders, tasks,
merged lists, `breaks`) would need porting to it; TipTap is the
ProseMirror layer Claude's documents use, its nodes and commands came
ready (lists and their keys, tables from prosemirror-tables, input rules
like "## "), and the markdown in and out is Yap's (below). **The note
stays markdown, exactly** (`docMarkdown.js`): `readMarkdown` reads it with
markdown.js' own markdown-it (`markdownTokens`: Yap's rules and all) and
each block keeps the lines it came from (`md`) and the lines before it
(`before`): the document's blocks, every list item (its lines without
their indent), every table row; how each was written is kept too (a
list's `*`, a fence's `~~~`, a rule's `***`, `__bold__`, `_italic_`, a
code span's backticks, a bare link). `writeMarkdown` writes a block that's
as it was read (equal to the block read from those lines: `isKept`) as
those lines, character for character, and one the person changed
afresh — the way it was written where that's known, its unchanged items
and rows still verbatim (a numbered list numbered in order stays so) —
then reads it back to check it says the same (`sameBlock`: words, marks,
links; spaces markdown drops aside) and escapes its words only when it
doesn't ("1. " at a line's start, a `*` that would turn bold, `|` in a
cell); a new block is plain markdown, a blank line before it. Read and
written back untouched, every document comes out character for character
(CRLF lines, `• ` bullets, setext headings, reference links, escapes and
entities included); a word typed changes its line, a cell its row, a
tick its item. What the editor can't hold as such (a list item that
starts with a heading…) is a **raw block**: drawn as the page draws it,
kept exactly, edited in Source. The title line (`# Title`, shown above
the page) stays outside the editor (`splitHead`). **Saving**: every change
goes through `doc.input` as Source's typing does (600 ms, `merge3` with
Chat's writing, a typed version: Undo of Chat's last change and Version
history know it); a change from outside (Chat's writing merged, Undo,
another window) is `load`ed by replacing only the blocks that differ
(`findDiffStart`/`End`), never in the editor's own undo history, the
caret kept, its blocks washed a moment as the page marks them (a wash
the page showed as the turn ended carries on in the editor). Ctrl+Z /
Ctrl+Y are the editor's own (the person's typing); the header's Undo is
still Chat's whole last turn. **The toolbar** (`DocToolbar.svelte`, a
`role="toolbar"` row under the header in Preview, 28 px buttons, the ones
on at the caret pressed): **text style** (Paragraph, Heading 1–3 — `#`,
`##`, `###` — a menu with the current one ticked), **code** (Ctrl+E),
**a table** (3 × 3 with a header row; in a table its menu: add a row
below, a column to the right, delete the row, the column, the table),
**checklist** (`- [ ] `; a numbered list or none becomes a bulleted one;
its box ticks with a click or Ctrl+Enter) and **+** (Divider — the caret
then on a line of its own under it —, Quote, Bulleted list, Numbered
list, Code block); TipTap's keys too (Ctrl+B / I, Ctrl+Alt+1–6, "## ",
"- ", "1. ", "> ", "```"). **No image button**: markdown.js draws no
images (a model's would make Yap fetch an address), and the person's own
pictures would need a store beside the note (`note-images/`, a URL scheme
to draw them, Export and Copy as Markdown carrying them) — not a toolbar
button. **No tab switcher**: Claude's tabs are pages inside one document;
a Chat document is one note, an answer's (a second "tab" is a second
document, with its own pill). Links: a click puts the caret in one,
Ctrl+click opens it. **Find, "Ask about this" / "Change this…", the
sections' rail and the scroll** work on the editor as on the page (a
selection's lines from `selectionLines`, what's typed saved first). Not
yet: typing on the page while Chat writes (it would need the editor to
take Chat's streaming sections as it takes a landed change), an input
rule for "[ ] ", column alignment from the toolbar, pasted pictures.
Unit-tested without the app (`doc-markdown.spec.js`: the round trip,
every block's kind, a change touching only its block, escapes, new
blocks); e2e `chat-documents-editor.spec.js` (a word, a cell, an item typed
— the note otherwise unchanged —, the editor's Ctrl+Z, every toolbar
action as plain markdown, the toolbar waiting while Chat writes, Chat's
change landing with the person's words kept, Undo marked, find, dark).

**The split** (`role="separator"`): dragged, or ←/→ (Shift:
further), Home/End on its handle; at least 320 px, the thread keeps 340;
its width remembered in this window's `localStorage` (`yapChatDocWidth`).
**Room**: the conversation list steps aside while a document is open
unless Chat is 1,240 px wide or more (a "Show your chats" button at the
thread's top left brings it back until the document closes); a window too
narrow for the panel beside the thread has it cover the conversation (✕
goes back). **Under the answer**, a pill per document it made or changed
("Lone wolf ranger · Open", `ChatMessage.svelte` `.docpill`; while it's
written, from the `document` events); a click opens it beside the chat.
Opening another conversation or a new chat closes the panel; the pills
bring a document back, after a restart too (`ChatMessage.documents`).
Tool chips for a document carry a document mark. Its own chunk, loaded
the first time a document opens (the main window's stays near 710 kB).
E2e: `chat-documents.spec.js` (the fake model's scripted calls; a step's
`gate` holds it mid-turn while the spec types; Undo of a whole turn, none
for a document the last turn made; the grey intent lines; a section
flowing in frame by frame — many small steps, none near a burst — under
the "Chat" caret, ending exactly as saved; motion reduced, in its
bursts), `doc-stream.spec.js` (no app: the shown pieces, the pace, the
page cut and drawn a piece at a time, the caret) and
`chat-documents-rich.spec.js` (a fake named like a GPT-5 model gets the
rich set: a table row added, a stale edit refused, "Change this…" from a
selection — typed, dictated with test mode's `e2e_chat_dictation`, asked
out loud with `e2e_assistant_ask` in view and from another app — a restore
and its Undo, Preview / Source, find, copy, light and dark).

- **`ChatView.svelte`** = the
  AI Chat surface (OpenWhispr `chat/ChatView.tsx` port): conversation sidebar
  (Today/Yesterday/Previous 7 Days/Older grouping, Ctrl+N, hover-delete;
  **several at once**: Shift+click a range, Ctrl+click one, Ctrl+A in the
  list → a "3 selected · Delete · Cancel" bar, Delete key / Esc; the list's
  ⋯ menu has Select all and "Delete all chats…", every delete asked first;
  `chat_delete_many` = one write, `chats::delete_many`; **search**
  (2026-10-25): "Search chats…" under New chat — every conversation's title
  and messages, archived ones too, by the words typed and then by meaning
  (`chats_search`), each result its title and the message it found with
  the words marked ("by meaning", "Archived" tags); a click opens the
  conversation at that message (`data-msg-id`), focused and its words
  marked; Esc clears; **archive**: a conversation's menu (and the selection's
  menu and bar) has **Archive** (toast with Undo); "Archived N" at the foot
  of the list opens the archive — click one to open it, ↺ Restore, Delete…
  (asked first), the same in its menu; asking in an archived conversation
  brings it back) +
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
  `scope: 'meetings'`, as is one in a chat started with **Ask across your
  meetings** on the empty thread; such a chat says "Asking across your
  meetings and notes" over its thread, its `[1]` pills and its **From your
  meetings** row open the cited moment in Notes — `chatState.openSource` →
  ControlPanel's `openNoteAt`; [search.md](./search.md)). Answers render as markdown — headings, lists,
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
  "Message"), **Think** for a model that can think on request (2026-10-29,
  `ThinkButton.svelte`: the next question only, `chat/think.rs` above; apart
  from the prompt chips and Skills), the mic, and Send only once there's text; an amber dot on the +
  while voice replies are on; under the box on the left, quiet, the **model
  chip** (its `quiet` look; never inside +: always in view) — `ModelChip.svelte`, names in `aiModel.js` from
  `ppModels.js` + `providerIcons.js`) shows the model the next message goes
  to — the provider's mark (`ProviderMark.svelte`), "GPT-OSS 20B · Groq" /
  "Qwen3.5 4B · On this PC", "· no note tools" for a local model under 4B,
  where it runs in its tooltip (and, on the on-device model, "Context: 8K
  tokens": `ai_model_status.context`), "No AI model" when nothing is set up;
  bottom right of the same row, **the context ring** ([below](#the-context-ring-2026-10-25)). The
  chip opens **the Chat model picker** (`ChatModelPicker.svelte`, its own
  chunk loaded the first time it opens since 2026-10-25, data in
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
  maximum, "Up to 256K · …", `maxContext`; every row's description
  wraps, never cut off (2026-10-27: an ellipsis with no way to read the
  rest), the name in full on hover; **right-click → Use /
  Uninstall…**, 2026-10-27, also the ContextMenu key or Shift+F10 on the
  row: `core/localModels.js` — off while it's in use (`local_llm_status`'s
  `inUse`) with what uses it under it: "Dictation cleanup and note
  formatting use it. Switch them to another model first" (`inUseNote`;
  a bare "Switch to another model first" made no sense with Chat on
  Claude); else Yap's confirm,
  "Uninstall Qwen3.5 9B? This deletes its {size} file from this PC. You
  can download it again." (the file's own size, as Windows counts it) — a GGUF not on Yap's
  list (the person's own, or one taken off it since, like Gemma 2 9B) is
  named by its file: "…It isn't on Yap's download list, so Yap can't
  download it again." — then
  `local_llm_uninstall` (ai-models.md `ai/local_llm/uninstall.rs`); a
  refusal in a toast, the dialog left open. The picker stays open while
  the menu and the dialog are up (`ModelChip` ignores a click outside then,
  `menuOrDialogOpen`): the row goes back under Download, the focus on its
  Download button, or away for a model of the person's own), self-hosted servers on this
  PC (`llm_models`: their `/models`, "3 models on this server", and the
  context Ollama / LM Studio say: "8K context, set in Ollama · …"), and
  **Download** (the rest, "Recommended for this PC" first): a Download
  button with the size runs `local_llm_install` right in the row with a
  progress bar from `local-llm-download-progress` (which names the curated
  `model`; one at a time, it carries on when the picker closes), "2 MB of
  4 MB · 24 MB/s · about 1 min left" and **Cancel** (2026-10-25, the shared
  download helper: app-shell.md `shell/download.rs`), then the row is installed and
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
  comes — a Voice Assistant answer being written or said in this chat
  too, `assistantAnswering` from `yap-assistant`'s phase; Esc) stops it:
  what came stays, "· Stopped" under it; stopped
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
  Search by meaning — local embeddings, Ask across meetings with cited
  passages — is [search.md](./search.md) (2026-10-25; ROADMAP step 3).

## Skills and one-tap prompts (2026-10-29)

Wispr Flow's Notetaker has one-tap prompts in its hub (Coach me, List
recent to-dos, Write a weekly recap) and **Skills**: saved instructions run
with `/command` in a meeting's chat or across meetings, the built-ins
copyable. Yap's, in `chat/skills.rs`:

- **A Skill**: a name, a `/command` (2–32 lowercase letters and digits,
  single hyphens; not a built-in's, not another of yours: `command_problem`),
  a line on what it's for, the instructions (≤ 4,000 characters) and where
  it runs (`runsOn`: `meeting` — a meeting's Ask box, on that meeting —,
  `meetings` — Chat and Ask across meetings, on all of them — or `both`).
  **Built in** (`builtins`, in code, never stored): /followup-email (one
  meeting), /decisions, /risks, /coach (both), /todos, /weekly-recap
  (across). **Your own** in `skills.json` (`SkillStore`, `Saver` "your
  Skills", a corrupt file set aside; ≤ 50): `skill_save` (new, a change, or
  a copy of a built-in: `basedOn`; what only a built-in carries — `gather`,
  `query`, `chip`, `question` — never comes with a copy), `skill_delete`,
  `skills_list` (built-ins first); `yap-skills-changed` to every window.
- **Not the Actions engine** (`notes.rs` `Action`, ActionManager): an
  action rewrites the note's summary under the note guardrails; a Skill
  answers in a chat and never changes the note, and has a command and a
  scope. And actions live in notes.json, which an older Yap rewrites
  without the fields it doesn't know: a Skill stored there would come back
  in its action picker as an action that writes over the summary. So
  Skills have their own file, which an older Yap never opens.
- **On one meeting**: `meeting_assist::meeting_skill` ([meetings.md](./meetings.md)):
  fixed guardrails, the meeting as numbered passages, then the
  instructions last, the answer cited and checked as Ask across meetings'
  are.
- **Across the meetings, in Chat**: `chat_send`'s `skill` (its command)
  → `skills::turn_for` (`Err` for one that runs on one meeting) → the turn
  runs under `skills::with` (a task-local, as the screenshots' `snip::turn`
  is, so `chat_turn`'s signature and its other callers stay as they were);
  `chat_turn` reads `skills::current()`: the turn is asked across the
  meetings whatever the conversation's scope (a new chat keeps that scope;
  one about something else stays so), no web or calendar tools, and its
  passages come from `skills::find`: **`tasks`** (/todos: every open task
  that's the person's — "You", or one they added — then the ones nobody
  took on, newest meeting first, each a passage that opens the moment it
  was said, else the summary's line; tasks added with no meeting aren't
  numbered), **`you`** (/coach: the talk of the meeting the words name —
  its title as whole words, or its day: "/coach the budget review",
  "/coach yesterday" — else the latest meeting's; the passages where the
  person talks most, in order), **`week`** (/weekly-recap: each
  meeting of the week the words name, this one by default, in order — its
  summary's first passages, else the start of its talk; ≤ 14), else **a
  search** (`ask::find`) for the words typed with it, else the Skill's own
  `query`, else "my last meeting". After Ask across meetings' own lines the
  system prompt adds `llm::SKILL_ACROSS_LEAD` and the Skill's name, command
  and instructions; the chip says "Ran /todos · 2 open tasks". The answer's
  checks, sources and pills are Ask across meetings' ([search.md](./search.md)).
  No model: the passages themselves (`passages_turn`, the Skill's own).
- **The `/` menu** (`components/SkillMenu.svelte`, `stores/skills.svelte.js`
  `SkillPicker(where)`, `core/skills.js`): in the notepad's Ask box and
  the Notes view's Ask bar on a meeting note (`meeting`), Chat's composer
  and the Meetings view's Ask bar (`meetings`): typing "/" at the start
  lists the Skills that run there, the command typed so far first, Yap's
  before yours (`matchSkills`); ↑ ↓, Enter runs the highlighted one, Tab
  puts it in the box to add words, Esc closes it; the box's
  `aria-activedescendant` follows the highlight. A sent "/command words"
  runs it (`splitCommand`, `skillFor`); in Chat the question is saved as
  typed.
- **One-tap prompts**: the built-ins with a `chip` — **Coach me**, **List
  my open to-dos**, **Write a weekly recap** — on Chat's empty thread
  ("Try asking", then "Or type / for Skills…") and over the Meetings view's
  Ask bar; a tap sends the Skill's `question` as that Skill (`runPrompt` /
  `chatRequest {skill}`), an empty chat asked across the meetings from then
  on. The Meetings view's cycling example questions stay, in its box.
- **Settings → Assistant → Skills** (`settings/SkillsPage.svelte`): Yap's
  (Open shows the instructions; **Save a copy** opens the form as yours,
  "Based on Yap's /decisions", command `my-decisions`), then yours (Edit,
  Delete behind Yap's confirm dialog), **New Skill** (the command follows
  the name until you type one; why it can't be one said as you type; Runs
  on: One meeting / Across meetings / Both). Saved at once to skills.json,
  not through Settings' auto-saved config.
- **Tests**: unit (`chat/skills.rs`: the built-ins, commands, save / copy /
  delete, an older file, each gathering; `meeting_summary/chips.rs`:
  passages, checks), e2e `meeting-chips.spec.js` (the menu, Enter / Tab /
  Esc, the built-ins and your own, Settings, Chat's prompts and "/", the
  Meetings view); prompts measured with `yap.exe eval --chips`
  ([model-evals.md](../model-evals.md#meeting-chips-and-skills-29-oct-2026)).

## The context ring (2026-10-25)

After Claude's own chat box (the maintainer's ask): bottom right under the
message box, beside the model chip's row, a small ring that fills as the
open conversation uses up the Chat model's context.

- **`ai/usage/context.rs`** — `chat_context {conversationId}` → `{used,
  estimate, lastAnswer, perSecond, context, contextKnown, trimmed,
  summarised, messages, questionMax, model}`. **Used**: the last counted
  answer's prompt + answer (`ChatMessage.tokens`, the provider's own
  counts), plus an estimate of any messages after it (a question whose
  answer failed); with none counted (before the first answer, a model
  that doesn't say, an older Yap's save) — or **a summary written after
  it** (`ChatSummary.after`: the next request is smaller, so the ring
  drops) — **an estimate** of what the next question sends: the parts a
  turn measures (`turn::stable_prompt`, the tools' schemas) and the
  history as the turn chooses it (`budget::plan`: the summary and the
  messages after it, or the latest that fit), with the same counter
  (`budget::Counter`). **The context** of the model the chip names
  (`ai_model::target`, so a switch follows): `budget::context_window` —
  the on-device model's (`local_llm::context::context_now`, what the
  sidecar serves), a self-hosted server's word (Ollama, LM Studio,
  llama.cpp's `/props`: `model_catalog::server_context_here`), a cloud
  model's from its provider's list (`model_catalog::listed_model(..).context`);
  none of those: the provider's usual (`budget::default_context`: Groq
  128K, Claude 200K, Gemini 1M, OpenAI and OpenRouter 128K, a server 8K)
  with `contextKnown` false. **Summarised**: the next question carries the
  summary. **Trimmed**: it leaves older messages out with nothing in their
  place (no summary yet). Unit-tested (the counted answer and what's after
  it, a newer summary, the estimate).
- **`ContextRing.svelte`** (`views/chat/`, in the composer's foot): an SVG
  ring (r 7 in 18 px) over `--yap-raised`, filled in `--yap-primary`,
  warmer from 80 % (`--yap-danger` mixed in) and `--yap-danger` from 95 %
  (`core/aiUsage.js ringColor`); light and dark from the tokens. Hidden on
  a new chat (nothing to show). Hover or focus: a tooltip "38% of context
  used" ("About 38%…" for an estimate or an approximate context); the
  button's accessible name says the same; `data-percent`. Read again after
  each answer, stop or newer question (`chatState`'s `contextStamp`), when
  another conversation opens, and on `yap-chat-model-changed`,
  `yap-config-saved`, `yap-local-llm-changed`. Click (Enter / Space):
  **`ContextPanel.svelte`** (its own chunk, loaded the first time), a
  popover above it (Claude's panel as the model: rows with thin bars;
  `role="dialog"`, focused, Esc closes it and gives the focus back):
  **This chat** — "1.6K of 4K tokens" (≈ for an estimate or an approximate
  context), a bar (`role="meter"`), the percent and the provider, "Last
  answer 400 tokens · 120 tokens/s" (the speed when the server said:
  llama.cpp's timings), a line when it's an estimate or the context is
  approximate; when the older messages go as their summary, one quiet line
  "Summarised · the chat stays within the model's context" (no warning);
  else **from 80 %** one quiet line with **New chat** — "Long chat:
  only the latest messages go to the model. A new chat answers best."
  when older messages are already left out (no summary yet), else "Nearly
  full: a long answer may be cut short. A new chat answers best."; **Today** and **Last 7 days** (`ai_usage_summary`,
  re-read on `yap-ai-usage`) — the tokens, the top 3 models (name ·
  where, a thin bar, tokens; "free, on your PC" on this PC, "≈ $0.02" for
  a cloud model with a known price, "no price known" otherwise), "4.4K on
  this PC · 1.2K cloud · ≈ $0.01" under several models; a fine line ("Costs
  are estimates at list prices (checked 25 Oct 2026); free plans cost
  nothing. Counted on this PC: nothing leaves it.") and **See all** →
  Insights' AI usage card in view (ControlPanel's `onopenusage`,
  `InsightsView focus="ai-usage"`; [frontend.md](./frontend.md)).
- E2e `chat-usage.spec.js` ([e2e-tests.md](../e2e-tests.md)).

## Long chats, sized to the model (2026-10-25)

The maintainer: "something people would pay money for". Until this day Chat
sent the last 19 messages whatever the model — an 8K model on this PC could
overflow, a 64K or a 1M one forgot a long chat's start for nothing — and a
request too long came back as "Couldn't send · HTTP 400 {…}". Now a chat
sizes itself to its model, remembers its start, and never hits a wall with
a technical error.

**What a long chat does, turn by turn** (`turn::chat_turn`):

1. **The context** of the model the question goes to (`budget::context_window`,
   the ring's too): the on-device model's real `n_ctx` (the sidecar's log,
   else what it starts with), a self-hosted server's word (Ollama, LM
   Studio, llama.cpp's `/props`), a cloud model's from its provider's list,
   else the provider's usual. A model evaluation may stand in a small one
   (`YAP_EVAL_CONTEXT`). A server on this PC is looked for first
   (`budget::there`: its port takes a connection within 300 ms, nothing
   sent); one that's off gets no probes and is left alone 15 s
   (`DOWN_FOR`, `/tokenize` too) — Windows retries a refused local
   connection for ~2 s, and with the model list, `/props` and `/tokenize`
   each paying it, "Couldn't reach the AI model on this PC" took ~8 s
   (2026-10-26; the e2e `polish.spec.js` test caught it).
2. **The question alone** must fit: measured against the parts that stay
   (`turn::stable_prompt` — the persona, the instructions, the prompt's end,
   the clock — and the tools' schemas, `tools::offered_tools`). Too long
   (a huge paste): **not sent and not saved**, the plain sentence back —
   "That's too long for Qwen3.5 4B (64K context)." The Chat view says so
   before sending (below).
3. **The budget** (`budget::budget`): the parts measured for this turn —
   the system prompt as built, the `<context>` block, the schemas — and
   room for the answer (an eighth of the context, 512 to 8K) and, for a
   model with tools, the tools' results (an eighth of the request, 512 to
   16K); the history and the question get what's left, at most **70%** of
   the context (**35%**, at most 6K tokens, for the Voice Assistant: every
   tool step sends it again; 70% for a note's Ask thread, beside the note).
   A plan that counts tokens a minute caps the whole request at 90% of it
   (`rate_cap`: Groq's `x-ratelimit-limit-tokens` once it said, its free
   plan's 8,000 until then; a 413 "Request too large … tokens per minute"
   teaches it too). **A model whose longer prompts cost more** stays at its
   cheaper rate (`price_cap`, 2026-10-27: 80% of `usage::prices::price_line`
   — Claude Haiku 5.5's 100K tokens, after which a prompt costs five times
   as much; OpenAI's 272K; Gemini Pro's 200K —, 80% since Yap estimates and
   Claude's newer tokenizer counts up to ~30% more): the history is sized to
   `request_cap` (the lower of the two) in a Chat turn, a note's Ask thread,
   the context ring and each summariser call (`compact`'s reading), so a long
   chat's summary takes over before a request would cost more. Before, a
   long chat on Haiku 5.5 (1M of context) sent up to ~700K tokens a turn.
   The "too long" check keeps `rate_cap` alone: a question longer than the
   line on its own still goes (the person's own paste).
4. **Counting** (`budget::Counter`): about four characters a token with a
   margin (`estimate`: ASCII at 3.6, any other character one — accents,
   other alphabets, CJK), each message +6; a long text (≥ 1,500 characters)
   sent to a llama.cpp server on this PC (the on-device model once loaded,
   a self-hosted one) is counted by its own `/tokenize`, cached; a server
   without it isn't asked again. An earlier web answer's replayed search
   counts (`chats::history_message`), and the newest three screenshots sent
   again (1,000 tokens each).
5. **The window** (`budget::plan`): every message when they fit — with a
   summary written, only while they take half the room or less (a much
   bigger model since); else **the summary and the messages after it**;
   else the latest that fit (and the summary beside them when it fits).
   The start moves 6 messages at a time (the Voice Assistant's 4), so a
   hybrid local model re-reads only from the first change. The summary goes
   in the system prompt before its end ("Earlier in this chat (Yap's notes
   on the older messages, which aren't shown; the latest messages follow in
   full): …", `compact::block`). Logged: counts and tokens only.
6. **Mid-turn**: each tool result (a page read, a note, search passages,
   Yap's own search for the model) is trimmed to what the request has left
   (`budget::fit_result`, `agent.rs` `Room`): its longest texts shortened
   and marked "… [cut to fit the model's context]", then its longest lists
   halved, with a note for the model; no room at all — "no room left …
   answer with what you have". The existing bounds stay (`get_note` 2,000
   tokens, `read_note` pages of 3,000, `search_in_note` ~4k characters,
   `read_page` parts): this is the last line for a small context.
7. **A provider's "too long"** (`llm::says_too_long`, every provider's own
   words: llama.cpp `exceed_context_size_error` / "exceeds the available
   context size", OpenAI `context_length_exceeded`, Anthropic "prompt is
   too long" / "exceed context limit", Groq "reduce the length of the
   messages", Gemini "exceeds the maximum number of tokens", OpenRouter
   "maximum context length", LM Studio "context length of only") comes back
   as `llm::CONTEXT_EXCEEDED`, said plainly by `budget::plain_error` — never
   the JSON. Chat, the Voice Assistant (its alert), the Notes view's bar and
   the notepad's sheet all say it this way.
8. **After the answer** (`compact::after_answer`, in the background; a model
   evaluation waits for it): when the history fills **75%** of its room —
   or the ring is 75% full and the history fills half the room — and
   there's enough older to be worth it (a tenth of the room, ≥ 300
   tokens), the older messages are **summarised**, keeping the latest ~40%
   of the room whole (at least the last turn, starting on a question):
   `budget::due`. **Incremental**: from the summary there is (covering
   `..upto`) and the newly dropped messages, never the whole chat again; a
   long stretch in parts that fit half the context, a paste read by its
   start and end. **The model**: the chat's own (it already reads the
   history it summarises). A model on this PC waits out a dictation and
   gives way to the next question (dropped, tried after the next answer).
   A failure leaves the window in charge; not tried again for a minute.
   **Kept** on the conversation (`chats.json` `summary`), only over the one
   it extends; `yap-chat-summary` to every window.
9. **The prompt** (`compact::prompt`, ~180 tokens, short and firm for small
   local models): keep every fact the user gave — names, numbers, amounts,
   dates, places, decisions, preferences — exactly; what was asked and
   told; what's open; "- " lines; nothing added; the chat's language; at
   most N words (15% of the room, 250 to 1,500 tokens).

**In the Chat view**:

- **The divider** (`ChatThread.svelte`) before the first message the
  summary doesn't cover: a thin rule, "Earlier messages summarised to keep
  this chat within the model's memory · **Show summary**"; Show summary
  opens it read-only (`SummaryDialog.svelte`, its own chunk: the notes, what
  they cover, "Summarised by GPT-OSS 20B · Groq"; Esc gives the focus back).
  It follows `yap-chat-summary` live.
- **The ring drops** (the next request's estimate, above) and its panel says
  "Summarised · the chat stays within the model's context".
- **A paste too long** (`chat_limits` → `{questionMax, context, label,
  tooLong, more}`, read on opening and when the model or its context
  changes; the page counts with Rust's estimate, `chatBudget.js`): a line
  over the box — "That's too long for GPT-OSS 20B (8K context). It's about
  11.1K tokens; there's room for 3.7K." — Send and Enter wait.
- **A provider's "too long"** after sending: the same line with Rust's
  sentence, the question back in the box. **Buttons**: **New chat** (in a
  chat with messages; the question goes with it), **Give it more room**
  (the on-device model only, when a bigger context fits the graphics card:
  `turn::more_room` over `gguf.rs`' estimates — `local_llm_set_context` to
  the next size, the toast "Giving Qwen3.5 4B 16K of context"), **Shorten
  what you pasted** (the message selected in the box).

E2e `chat-long.spec.js` and `chat-room.spec.js`
([e2e-tests.md](../e2e-tests.md)); the model evaluations' `chat-long-memory`
([model-evals.md](../model-evals.md#long-chats-the-window-and-the-summary-25-oct-2026)).
