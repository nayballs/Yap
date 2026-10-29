# Search by meaning: meetings, notes, chats and memory

Local search over every meeting, note, chat and memory item **by meaning**
(meetings and notes 2026-10-25, then the rest the same day), and answers
that cite the exact moment: the passages are found on this PC by a small
embedding model and keywords together, a question's dates narrow them, and
every result opens at the line it came from. Where it's used: **Ask across
meetings** and the Meetings view's moments (below), **the Notes view's
search**, **Chat's sidebar search**, Chat's own **`search_notes`** and
**`search_chats`**, and **memory's** people, projects and topics for a
question ([Beyond the meetings](#beyond-the-meetings-2026-10-25)). The Chat
it answers through is [chat-and-assistant.md](./chat-and-assistant.md); the
notes it searches are [notes.md](./notes.md) and
[meetings.md](./meetings.md); memory is [memory.md](./memory.md); the engine
it borrows is the on-device model's ([ai-models.md](./ai-models.md)).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules (`src-tauri/src/search.rs` + `search/`)

- **`search.rs`** — the root: `init(app)` (the background index),
  `on_config_saved(prev, cfg)` (the switch on: a pass soon; off: the model's
  server stopped), `shutdown()`, and the two calls the rest of Yap makes:
  **`changed()`** (something searched changed: a pass soon; chats.rs calls
  it on every save) and **`removed()`** (something was deleted — a note, a
  chat, a memory item, **a transcript's lines** (a meeting's retention):
  call it after the store has let go, and the vectors of every passage
  that's gone are dropped a few seconds on, search by meaning on or off).
- **`search/chunks.rs`** — notes, chats and memory items cut into
  **passages**, pure (below).
- **`search/embedder.rs`** — the embedding model, **named in one place**
  (`MODEL`), its download, its server, a question's vector
  (`query_vector`: cached, bounded wait).
- **`search/store.rs`** — the vector file, `<data>/search-index/vectors.bin`.
- **`search/index.rs`** — keeping the store up to date in the background
  over three `Kind`s (notes, chats, memory: `Library::now`), the sweep after
  a deletion; `Status` for Settings (`yap-search-status`).
- **`search/dates.rs`** — the dates a question names, pure.
- **`search/rank.rs`** — hybrid ranking and the line a passage opens at, pure.
- **`search/ask.rs`** — a question's passages (`find` / `find_waiting` /
  `find_in`), as the model reads them (`context`), as sources (`sources`,
  `shown`), the citation repair (`cite_missing`) and the answer without a
  model (`passages_answer`).
- **`search/library.rs`** — the search boxes' search (`search_typed`: every
  word typed, marked, and by meaning), Chat's `search_notes`
  (`notes_for`), `search_chats` (`find_chats` / `chats_for`) and memory's
  items for a question (`memory_relevant` / `memory_block`).
- **`search/commands.rs`** — `search_status`, `search_install`,
  `search_set_enabled`, `search_rebuild`, `search_delete`, the Meetings
  view's `search_moments(query, limit)` → `{moments, semantic, range}`,
  the Notes view's `notes_search(query, meaning?, limit?)` and the Chat
  sidebar's `chats_search(query, meaning?, limit?)` → `{hits, semantic}`.

## The model and its runtime

**The model is named in one place**: `embedder::MODEL` (a `Model`: name,
index tag, GGUF file, pinned URL, SHA-256, size, dimensions, licence and its
link, the `NOTICE.txt` text, its passage and question prompts, its own
server arguments, and its meaning-only floors — each model's cosines sit
on a scale of their own). Everything reads it through `embedder::model()`: the
download, the server, the prompts, the store's tag and dimensions, Settings'
name and licence link. Two are defined: **`EMBEDDINGGEMMA_300M`** (Yap's)
and **`QWEN3_EMBEDDING_0_6B`** (Apache-2.0, 1,024 dimensions, `--pooling
last`, an instruction before a question), the other candidate while the
licence is decided; switching is `pub const MODEL: Model = …` (another tag:
a new download, and the index read again — the old file is set aside). Its
URL points at `main` and must be pinned to a commit before it ships (the
hash is the file measured in `yap-private`). The model evaluations compare
the two (`--embed-model qwen3`).

**EmbeddingGemma-300M, Q8_0 GGUF** (334 MB, 318 MiB; 768 dimensions, 100+
languages; Gemma Terms of Use, a `NOTICE.txt` beside it), from ggml-org's
Hugging Face repo pinned to a commit, size and SHA-256 checked. It runs on
**Yap's own llamafile 0.10.6** — the
on-device chat model's engine — as a second, small server in
`--embedding` mode (`server_args`: 127.0.0.1, a port of its own, `-c 4096
-b 2048 -ub 2048 -ngl 999`, half the processor's threads), so nothing new
ships: CUDA on NVIDIA, Vulkan elsewhere (~390 MiB on the card), the
processor without one. Below-normal priority, started when the index or a
question needs it (`ensure`), stopped after 2 minutes unused (`IDLE`),
killed with Yap (`procs::end_with_yap`, the orphan sweep), stopped before
the runtime's upgrade swaps the exe (`local_llm/runtime.rs`). Three failed
starts in a row and it stops trying until the setting is switched on again,
the model is downloaded again or the index rebuilt (`reset_failures`).

**Why**: measured in `yap-private/docs/local-ai-research.md` #7 and here
(below). ONNX through `ort` was the other way, but `ort` is only in
`engines` builds and Gemma's tokenizer would have been a new dependency;
llamafile tokenizes itself and is the same in every build.
Qwen3-Embedding-0.6B read as well at twice the cost.

Prompts are the model's own: a passage is `title: <note title> | text:
<passage>` (`embedder::document`), a question `task: search result | query:
<question>` (`embedder::query`).

**Install** (`search_install` → `embedder::install`): the runtime first if
the on-device model hasn't brought it (`local_llm::models::ensure_runtime
("search")`, its progress on `local-llm-download-progress` with `model:
"search"`), then the GGUF into `<data>/models/search/` through the
registry's resumable, verified download ("Search model download" in the
network receipts), progress on `yap-search-download {stage, percent,
downloadedBytes, totalBytes, bytesPerSec, etaSecs, cancelId: "search"}`
(Settings' row says how fast and how long; Cancel = `download_cancel {id:
"search"}`: app-shell.md `shell/download.rs`, 2026-10-25). Then the setting is switched on
(`config.search_by_meaning`) and a pass starts. **Delete** removes the GGUF
and the index; the runtime stays (the on-device model may use it).

**Never in the cloud**: the server is on 127.0.0.1 and every request goes
through `privacy/net.rs` as `Purpose::Embeddings` ("Search by meaning ·
on-device model", on this PC). Retrieval is always local; a cloud Chat
model sees only the passages chosen for its question.

**A question's vector** (`embedder::query_vector(q, wait)`): `None` (the
words alone answer) with search by meaning off, the model not here, the
server failing, or not ready within `wait`; the request runs as its own
task, so a start that outlasts `wait` goes on and the next search has the
model up; the last 32 questions' vectors are kept (a search box typed letter
by letter). The waits: Ask and the search boxes 20 s (`ask::QUERY_WAIT`,
`library::BOX_WAIT`; a box shows its words' results first anyway), Chat's
tools 8 s (`TOOL_WAIT`), memory with every Chat question 1.5 s
(`MEMORY_WAIT`: it's on the way to every answer).

## Passages (`chunks.rs`)

- A meeting's **transcript** by talk: consecutive lines ("Speaker: words",
  the speaker map's names via `speakers::prompt_label`) up to 110 words,
  ending at a change of speaker once it has 60, always at a stop
  (`Note::breaks`) or a 5-minute pause; echo lines, dictation markers and
  failed stretches left out; a line longer than a passage split at its
  sentences; a short tail (< 25 words) merged into the passage before it.
- The person's **notes** (`content`) and the **summary**
  (`enhanced_content`: digests, the action plan) by paragraphs and list runs
  up to 110 words; a heading starts a passage and goes with the lines under
  it ("Decisions: …").
- A note with only a title: one passage, its title.
- A **chat** (`chunk_chat`, kind "chat", field `Chat`): its messages as
  lines ("You: …", "Yap: …", each by its index in the conversation), a
  question and its answer together where they fit — up to 110 words, the
  person's next question starting a passage once it has 60; an answer
  longer than a passage cut at its sentences, its question going with the
  first piece when 40 or more of its words fit; a few words of an answer
  left at the end joining their question's passage (a new question never
  does). Tool chips, sources and screenshots aren't searched. Archived
  chats too.
- A **memory item** (`chunk_memory`, kind "memory"): a person, project or
  topic as one passage — its name the title, who or what it is and its
  facts a line each. Profile and Preferences aren't (they go with every
  question).

Each passage keeps its lines (index, speaker, time), so a citation opens
the meeting at one. The same note always gives the same passages; each is
keyed by `sha256(model tag + embedded text)[..16]` (`chunks::key`), so the
index finds what it has already read and never reads it twice.

## The index (`store.rs`, `index.rs`)

**The file**: `<data>/search-index/vectors.bin` — a header (`YAPVEC01`, the
dimensions, the model's tag), then per passage its 16-byte key, a scale
(f32) and the vector as signed bytes (`round(v / scale)`, scale = max |v| /
127): 784 bytes a passage, a quarter of f32, cosines within ~0.01. **No
text**: passages are cut from the notes as they are whenever a search runs,
so nothing in the index outlives a note's words. Appended as it goes (a
crash loses only the record being written: a short tail is cut off on
read); a file for another model, or one that doesn't read, is set aside as
`vectors.bin.old` and the index starts again. **Three kinds in one file**:
notes (meetings too), chats and memory items (`index::Kind`; each kind's
passages are prepared and cached apart, `prepared` / `prepared_chats` /
`prepared_memory`, by the item's fingerprint; `Library::now` reads all
three, the notes and chats in place under their stores' locks:
`notes::with_all`, `chats::with_all`). **Compaction** (`Store::compact`: a
new file swapped in) drops the vectors of passages that are gone — **at
once when something was deleted**: a note (`notes::delete`), a chat
(`chats::delete_many`), a memory item (`yap-memory-changed`), or lines of
a transcript (the caller's `search::removed()`), the next look a few
seconds on — **with search by meaning off too** (`index::sweep`, as long
as an index is on disk), and on a run's first pass for anything deleted
while Yap was closed; else once dead vectors are over 30 % of the file (an
edit leaves its old passages' vectors until then). Brute force: tens of
thousands of passages are a few ms.

**When** (`index::init`): 45 s after Yap starts, then on Yap's own events
(`yap-note-changed`, `yap-notes-changed`, `yap-meeting-state`, a chat
saved — `search::changed()` from `chats.rs` — and the deletions above; a
burst settles 8 s first) or, every 2 minutes, a cheap fingerprint of the
notes and chats (`notes::stamp`, `chats::stamp`: ids, updated times and
counts — the local API's edits too). A failure waits, doubling to 30
minutes. A chat turn costs a passage or two (the question's, then with its
answer).

**Never in a dictation's way**: a pass waits while a dictation or a meeting
recording runs (`meeting_summary::wait_idle`), and a batch under way is
dropped the moment one starts (`meeting_summary::unless_busy`: the
connection closes) and read again after. Batches of 16 on the card, 4 on
the processor (a few seconds at most). Passages already read are kept, so
a pass cut off loses nothing.

`Status` (`search_status`, `yap-search-status`): `enabled`, `installed`,
`runtimeInstalled`, `installing`, `phase` (`off` · `notInstalled` ·
`waiting` · `indexing` · `ready` · `error`), `error`, `passages`,
`indexed`, `notes`, `chats`, `memories` (the items with passages), `kinds`
(`{notes, chats, memories, notePassages, chatPassages, memoryPassages}`),
`indexBytes`, `modelBytes`, `downloadBytes` (the model, plus the engine
when it isn't here), `runsOn` (`gpu` · `partial` · `cpu`, from the
server's log), `modelName`, `license`, `licenseUrl`.
**Rebuild** (`search_rebuild`) deletes the index and reads everything again.

## Finding passages (`dates.rs`, `rank.rs`, `ask.rs`)

1. **Dates** (`dates::parse`, the person's local time, weeks from Monday):
   today, yesterday, the day before yesterday; this / last / past week,
   month, year; the last 3 days; 3 days / a week ago; the week before last;
   a weekday ("on Monday", "last Tuesday"); a day ("14 October", "October
   14th"); a month ("in September", "September 2025"; "may", "march" and
   the like only with a lead-in or a year); a year; any of these after
   since / before / after. **"My last meeting"** (latest, most recent,
   previous; meeting, call, sync…) means the most recent meeting itself.
   The date's words leave the keyword search. Anything vaguer searches
   everything.
2. **Candidates**: every passage within the dates (or of the last meeting).
3. **Two lists**: keywords — BM25 over each passage's words (stemmed, stop
   words out), the title's words at half weight, so names and numbers,
   rare in a library, weigh most — and meaning — the question's cosine with
   each passage's vector, kept within 0.22 of the best and above 0.2.
4. **Weighted reciprocal rank fusion** (`rank::TUNING`: K = 10, each list's
   `1 / (K + rank)` weighed by that match's strength: the keyword list by
   much of the question's word rarity it holds, the meaning list by where
   its cosine sits between the floor and the best), a passage where a voice
   the question names talks
   ("what did Priya say…", "what did I promise…") lifted ×1.6, at most 3
   passages per note.
5. **A time with no matching words** ("what happened yesterday?",
   "summarise my last meeting"): what's from then fills in — summaries
   first, then notes, then the talk in order.
6. **The line each passage opens at** (`rank::best_line`): the line sharing
   the question's rarest words, the subject's voice preferred, a question
   moved on to its answer when the next speaker replies; with no shared
   word, the passage's first line.

Without the model (switched off, not downloaded, or slow to start: 20 s),
the keyword list alone answers — still BM25 per passage, better than the
whole-note match Chat's tools use.

## Ask across meetings in Chat

**Where**: the Meetings view's Ask bar, and Chat's "Ask across your
meetings" button on an empty chat — and, since 2026-10-29, a Skill run in
Chat or from the Meetings view's one-tap prompts, which brings passages of
its own (the open tasks, a week of meetings, the latest meeting's talk) or
searches for its words (`chat/skills.rs`, [chat-and-assistant.md](./chat-and-assistant.md#skills-and-one-tap-prompts-2026-10-29)),
answered and checked as below. The first two send with `scope: "meetings"`, and
**the conversation keeps it** (`chats::Conversation.scope`, `set_scope`):
its follow-ups are asked across the meetings too, and the thread says
"Asking across your meetings and notes" above it.

**The turn** (`chat/turn.rs`): no web search, no eager note RAG; the 8 best
passages (`ASK_PASSAGES`) go in the message's `<context>` (or the system
prompt, as Chat lays out context for that model) as numbered blocks:

```
Their latest meetings, newest first: …
<passage n="1" title="Q3 budget review" date="Fri 16 Oct 2026, 14:12" at="12:40" from="transcript">
Priya Shah: There's a hiring freeze until January, except the support role.
</passage>
```

with `ASK_INSTRUCTIONS`: answer only from them, cite each statement's
passage by number in square brackets before the full stop ("… by Friday
[2]."), say plainly when they don't answer it. A chip goes out first:
"Searched your meetings by meaning · 4 passages[ from last week]".

**The checks** on the answer, after Chat's own (`chats::checked_answer`):
a citation that's none of this turn's passages is stripped; a statement
plainly from one passage that the model left uncited is cited by Yap
(`ask::cite_missing`: "Passage 2" → [2], a quoted line, or ≥ 3 shared words
covering half the sentence with a clear winner; a two-way tie at ≥ 80 %
cites both; never in code, headings or a "that isn't in your meetings"),
counted as `cited_by_yap`; an answer that still cites nothing while
passages were there, and says something (12 words, or a short one with a
number or a date), gets `UNCITED_NOTE` ("Yap couldn't tie this answer to a
passage from your meetings: check it against the passages below.") — unless
it opens by saying the meetings don't cover it (`says_not_found`).

**Sources**: each cited passage is a `chats::Source` with `note`, `field`,
`line`, `ts` and `quote` (only the cited ones, `ask::shown`). In the thread,
`[1]` is a pill (`core/markdown.js`: `a.md-cite.md-passage`,
`href="#passage-N"`, allowed only with that class), and under the answer a
"From your meetings" row of the cited passages (title, day, time into the
meeting, speaker). Either opens the note in Notes (below).

**No AI model** (`passages_turn`): the passages themselves, each cited so
its pill opens the moment, and "Set up an AI model in Settings → Language
Models to get answers written from them."

## The Meetings view's moments

Typing in "Search meetings" (Past meeting notes) also asks
`search_moments(query, 5)`: a **Moments that match** box over the list
("· by meaning" when the model helped, "· last week" when the query named
a time), each moment its note's title, day, time and speaker, and its line
(the speaker's "Name: " left off). Clicking one opens it. With moments
there, the list's "No meeting notes match that." isn't shown.

## Opening a moment (frontend)

`ControlPanel.openNoteAt({id, line, ts, field})` → `noteRequest {id, line,
ts, tab}`: a transcript passage opens the meeting at its line (scrolled
to, focused and lit, as from the Tasks view; `ts` finds the line again if
the transcript was done again since); a summary passage opens the Summary
tab, a notes passage the notes. `chatState.openSource(s)` and
`MeetingsView.openMoment(m)` both go through it.

## Beyond the meetings (2026-10-25)

`search/library.rs`. Everything below works by the words alone without the
model; with "Search by meaning" on, by meaning too. Measured below
([Measured: beyond the meetings](#measured-beyond-the-meetings-yapexe-eval---search-library-25-oct-2026)).

**The search boxes** (`search_typed`, pure, over `Doc`s: an item's id,
title, prepared passages and last change). **Full text**: an item is found
when it holds **every word typed**, each at the start of a word ("budg"
finds "budget", "avenue" isn't "venue"), case aside, in its title or any
passage (`holds`, over each passage's lower-cased words, `Prepared.lower`).
Ranked by Ask's ranking (`rank::rank`, BM25 + meaning, weighted RRF) over
those items' passages, the query's terms plus each typed word's
completions in them ("planni" → "plan", so a half-typed word ranks too);
then a title holding every word goes first (finding a note by its name is
what the box is used most for), the ranking's order kept within. **By
meaning**: a passage close to the query (the model's `floors.typed`: 0.33 for EmbeddingGemma, and
within Ask's window of the best) brings its item without the words — at
most `MEANING_ONLY` (5), tagged "by meaning". Each result: the line to open
at (the one holding most of the words, a word the title lacks counting
double; else the passage's best line), clipped to ~180 characters around
the first word, the words marked (`marked` → `Part {text, hit}`), its
field ("title" when only the title holds them), line index, time, speaker,
and for a transcript how far into the meeting (`at`). A thousand notes
(4,000 passages): ~36 ms in a debug build (`a_thousand_notes_are_searched_quickly`).

- **The Notes view** (`NoteList` → `NoteSearch.svelte`, loaded the first
  time a search is typed): "Search all notes…" searches every note
  whatever its folder — title, My thoughts, summary, transcript — the words'
  results at once (`notes_search(meaning: false)`), then again with meaning
  when it's on; ↓/Enter from the box. A result opens its note **at the
  match** (`notesState.openRequested({…, find: {words, line}})`): a
  transcript line scrolled to, focused, lit and its words marked; My
  thoughts with the words selected on their line (`core/findWords.js`
  `selectWords`); the Summary tab with its words marked (`markWords`: the
  CSS Custom Highlight API, `::highlight(yap-found)` in `app.css` — no
  change to the rendered markdown).
- **Chat's sidebar** (`ConversationList` → `chatLibrary.svelte.js` +
  `ChatLibraryList.svelte`): "Search chats…" over every conversation's
  title and messages, archived ones too (tagged); a result opens its
  conversation at the message (`[data-msg-id]`), focused, the words marked.

**Chat's `search_notes`** (`notes_for` → `chat/tools/notes.rs`
`search_notes_tool`; the agent loop runs it): Ask across meetings' own
search (`ask::find_waiting`: BM25 per passage, meaning, the dates and
voices the query names) instead of the whole-note keyword count, grouped by
note (≤ 3 passages each); each result's `content` is its matched passages
with where each is from ("(transcript, 12:40) Priya Shah: …", "(notes)",
"(summary)", ≤ 600 characters), so the model reads the part that answers.
Its chip says "… by meaning" when meaning took part. The eager RAG
(`rag_notes`) stays keyword-only and strict (a loose match pulled
follow-ups off the conversation), as does the local API's search.

**Chat's `search_chats`**, and a past chat for a model without tools
(`chats_for` / `find_chats`; `past_chats_block`): the question's words
(BM25; a chat needs two of them or one in its title, as `chats::search`
did) or its meaning (`floors.chats`, 0.3), every conversation but the
one asking; each chat's lines: the best one and its question or answer.

**Memory's items for a question** (`memory_block`, from `turn::chat_turn`):
memory's own keyword rule (`memory::prompt::relevant_indexes`: the name, or
two words) and, with meaning, items close to the question
(`floors.memory`, 0.3 — it goes into every answer's prompt, so only a
close one), fused by weighted RRF (a tie: the words' first), ≤ 3.

**Model evaluations' baseline**: `library::keyword_only()` (debug builds,
`YAP_EVAL_SEARCH=keyword`, `--search-keyword`) makes all three Chat
searches what they were before.

## Settings → Privacy & data → "Search by meaning"

`settings/MeetingSearchSettings.svelte` (anchors `meeting-search`,
`meeting-search-model`, `meeting-search-index`, in `settingsIndex.js`): the
switch (disabled until the model is here), **Search model** ("EmbeddingGemma
300M: a one-time download of 318 MB", plus ", the on-device AI engine
included" and its size when that isn't here → Download, its progress, then
name · size · where it runs and the licence link), **Search index** ("1,240
passages from 40 notes, 12 chats and 8 memory items · 2.1 MB", or "Reading
your meetings, notes and chats: 12 of 40 passages", "Waiting for your
dictation or meeting to finish…") with Rebuild and Delete… (asked first:
`askConfirm`). "What leaves this PC" has a **Search** row: "Searched on this
PC by their words", or "Your meetings, notes, chats and memory: read and
searched by meaning on this PC".

## Measured (`yap.exe eval --search`, 25 Oct 2026)

`evals/search/meetings.json`: a made-up team's 11 meetings and 5 notes (29
passages, ~1,400 words), 35 questions with their gold lines (33 answerable;
paraphrases, names, numbers, dates, "my last meeting", notes). RTX 5070 Ti:
embedded in 0.3 s, a question 8 ms (median); its processor (`--embed-ngl
0`): 2.0 s (14 passages/s), a question 14 ms. A year of meetings (~10,000
passages) is ~12 minutes of background reading on a processor like that,
once.

| Search | Recall@1 | @3 | @5 | MRR | Line exact @1 |
|---|---|---|---|---|---|
| Chat's note search (whole notes, keywords) | 0.303 | 0.667 | 0.848 | 0.484 | — |
| keyword (BM25 per passage) | 0.667 | 0.909 | 0.939 | 0.779 | 0.485 |
| meaning | 0.818 | 1.0 | 1.0 | 0.904 | 0.545 |
| **hybrid (Yap's)** | **0.818** | **1.0** | **1.0** | 0.899 | 0.515 |
| hybrid, plain RRF K 60 | 0.818 | 0.97 | 0.97 | 0.888 | 0.606 |

Plain RRF let weak keyword-and-meaning matches above a meaning-only gold
("new staff"); weighting by strength fixed it. Picking the line by meaning
too would lift "line exact" to 0.606, at the cost of embedding a passage's
lines at question time: not done (yet).

Answers (each question through Chat's own turn, `chat_turn` scope
"meetings"): **Qwen3.5-4B on the card: 33/35** in two runs (31–33 cite a
passage, 31–32 a gold line; Yap added the citation in 8–10 answers; a
median 0.4 s); Groq `openai/gpt-oss-20b` (5 questions, ~17k tokens): 4/5.
Details: [`docs/model-evals.md`](../model-evals.md#meeting-search-25-oct-2026).

## Measured: beyond the meetings (`yap.exe eval --search-library`, 25 Oct 2026)

`evals/search/library.json`: one made-up person's 9 notes, 2 meetings, 6
past chats and 7 memory items (25 passages), and 85 questions: 18 typed
into the Notes view's box (10 whose words aren't in the note), 15 as Chat's
`search_notes` is called (7 paraphrases), 9 about past chats (6
paraphrases), 15 for memory (6 about nothing in memory), 17 asked in Chat.
"Before" is Yap's search until this change. EmbeddingGemma-300M on an RTX
5070 Ti (25 passages in 0.4–0.9 s).

| Search | Before | Words (after) | Words + meaning (after) |
|---|---|---|---|
| Notes view's box, recall@1 (n 18) | 0.333 (title + first 120 characters) | 0.444 (every note, every part) | **0.944** |
| Chat's `search_notes`, recall@1 / @5 (n 15) | 0.667 / 0.933 (whole notes) | 0.867 / 0.867 (passages, BM25) | **1.0 / 1.0** |
| Chat's `search_chats`, recall@1 (n 9) | 0.444 (`chats::search`) | 0.556 (BM25) | **1.0** |
| Memory: gold items with their question (9) | 0.444 | — | **1.0** |
| Memory: questions about nothing in memory given an item (6) | 0 | — | **0** |

The floors: the box's meaning-only notes came at 0.39–0.51 with the closest
other note at most 0.26 (`floors.typed` 0.33; the one miss, "plane
tickets" → the Porto trip, at 0.27); memory's gold items at 0.31–0.52, the
questions about nothing in memory at most 0.16 (`floors.memory` 0.3:
0.42 found 0.667). **The floors are the model's** (`embedder::Floors`):
**Qwen3-Embedding-0.6B** (`--embed-model qwen3`) puts everything higher —
the closest other note at 0.46, unrelated questions' memory items up to
0.48 against gold from 0.37 — so with Gemma's floors 4 of the 6 unrelated
questions got a memory item; with its own (0.48 / 0.5 / 0.45): the box
0.833, `search_notes` 1.0, past chats 1.0, memory 0.778 with none for
unrelated questions — on this corpus a little behind EmbeddingGemma, at
twice the size.

**Chat's answers** (each question in Chat, no scope; chats and memory seeded
afresh; pass = the answer holds the fact): **Qwen3.5-4B on the card 16/17
with the new searches, 12/17 with the old** (`--search-keyword`), in 92k
against 117k tokens; of the 7 questions whose words the eager RAG doesn't
match (so the model must search), 6/7 against 4/7 — the old search looped
up to 10 `search_notes` calls before giving up. Groq `openai/gpt-oss-20b`,
those 7 questions: **6/6 with the new searches** (18k tokens; the 7th past
the token cap), **2/3 with the old** (42k tokens: "the joinery firm's
offer for the hold-up" ran 11 empty searches, then an empty answer). Two
changes beside the search came from these runs: the eager RAG's transcript
snippet now names each line's speaker ("who fixed the form?" named the
wrong person from the bare lines), and `search_chats`' instruction covers
"something you said or suggested before" (the model searched notes
instead). The existing notes and memory scenarios (13, `--only notes-,
memory-search-past-chats, memory-use`, keyword-only as a scenario has no
index): 13/13 on Qwen3.5-4B.
Details: [`docs/model-evals.md`](../model-evals.md#search-beyond-the-meetings-25-oct-2026).

## Tests

- Unit (`cargo test search::`): passages (talk, notes, headings, long
  lines, tails, keys), the store (round trip, int8 accuracy, a torn tail,
  another model's file, compaction), dates (every form, at a fixed clock),
  ranking (stems, BM25, fusion, speakers, per-note cap, the line), Ask
  (dates, the last meeting, the fill rule, context, sources, `cite_missing`
  and its refusals, the not-found words, the answer without a model), the
  index (passages cached per note, a deleted note sweeping the store; the
  three kinds in one cache, a chat's new message reading it again, a
  deleted chat's vectors compacted away, on disk too), the chat chunking (a
  question with its answer, the next question past 60 words, a long answer
  starting with its question and every word once) and memory's passage, the
  model in one place (both models pinned, their prompts and server
  arguments), the embedder's response parsing; the library
  (`search::library`: words at the start of words and marked, a long line
  clipped, every note with all the words opening at its line — transcript,
  notes, summary, title — half a word, by meaning with its floor, a thousand
  notes searched in ~36 ms debug, past chats by words or meaning with the
  question and its answer, memory's fusion and floor); chats' archive
  (`chats::tests`: out of the list and back, the Voice Assistant skipping
  one, asked again restores, deleted from the archive); an ignored test
  against the real model (`YAP_SEARCH_LLAMAFILE` + `YAP_SEARCH_MODEL`:
  dentist·teeth 0.49 against dentist·beta 0.12).
- e2e (`e2e/meeting-search.spec.js`, a stand-in embedding server
  `e2e/support/fake-embed.js` via the debug-only `YAP_E2E_EMBED_URL`, the
  downloads from `startFakeLocalModels`): Settings' download and index;
  the Meetings view's moments by meaning, each opening at its line; Ask
  across meetings' pills, sources, stripped citation, the prompt's passages,
  the scope kept for a follow-up; no model → the passages; Rebuild, Delete,
  and the network receipts. `e2e/library-search.spec.js` (the same
  stand-ins): the Notes view's search by words (every folder, marked,
  opening the transcript line, My thoughts' words selected, the summary's
  marked; all the words or nothing) and by meaning; Chat's sidebar search by
  words (opening at the message, marked) and by meaning; archive, Archived,
  restore, several at once, delete from the archive; a deleted chat's
  vectors gone from `vectors.bin` at once; memory's person found by
  meaning in the prompt; `search_notes` by meaning with its passage; Privacy
  & data's counts by kind.

## Two Yaps on one data folder

The installed nightly and a dev build share `%APPDATA%\yap`, and an older
Yap may not know what a newer one keeps. **Searches never show what's
gone**: their passages are cut from the notes and chats as they are now,
and a vector is only looked up by its passage's key. **The index heals
itself**: the first look after a start sweeps the vectors of everything
that's gone (`SWEEP` starts true, with search by meaning on or off), and
the first pass reads every passage the store lacks — what another Yap
added, edited or (an older one, without chats and memory in its index)
compacted away. An older Yap's save of `chats.json` drops `archivedTs`: an
archived chat comes back to the list, never lost. Nothing is migrated: the
config, `notes.json` and `memory.json` gained no field.
