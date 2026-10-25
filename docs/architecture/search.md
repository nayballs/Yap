# Meeting search and Ask across meetings

Local search over every meeting and note **by meaning** (2026-10-25), and
answers that cite the exact moment: the passages are found on this PC by a
small embedding model and keywords together, a question's dates narrow them,
and every citation opens its meeting at the line it came from. The Chat it
answers through is [chat-and-assistant.md](./chat-and-assistant.md); the
notes it searches are [notes.md](./notes.md) and
[meetings.md](./meetings.md); the engine it borrows is the on-device model's
([ai-models.md](./ai-models.md)).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules (`src-tauri/src/search.rs` + `search/`)

- **`search.rs`** — the root: `init(app)` (the background index),
  `on_config_saved(prev, cfg)` (the switch on: a pass soon; off: the model's
  server stopped), `shutdown()`.
- **`search/chunks.rs`** — notes cut into **passages**, pure (below).
- **`search/embedder.rs`** — the embedding model: EmbeddingGemma-300M on
  Yap's own llamafile, its download, its server (below).
- **`search/store.rs`** — the vector file, `<data>/search-index/vectors.bin`.
- **`search/index.rs`** — keeping the store up to date in the background;
  `Status` for Settings (`yap-search-status`).
- **`search/dates.rs`** — the dates a question names, pure.
- **`search/rank.rs`** — hybrid ranking and the line a passage opens at, pure.
- **`search/ask.rs`** — a question's passages (`find` / `find_in`), as the
  model reads them (`context`), as sources (`sources`, `shown`), the
  citation repair (`cite_missing`) and the answer without a model
  (`passages_answer`).
- **`search/commands.rs`** — `search_status`, `search_install`,
  `search_set_enabled`, `search_rebuild`, `search_delete`, and the Meetings
  view's `search_moments(query, limit)` → `{moments, semantic, range}`.

## The model and its runtime

**EmbeddingGemma-300M, Q8_0 GGUF** (334 MB, 318 MiB; 768 dimensions, 100+
languages; Gemma Terms of Use, a `NOTICE.txt` beside it), from ggml-org's
Hugging Face repo pinned to a commit, size and SHA-256 checked
(`embedder::MODEL_*`). It runs on **Yap's own llamafile 0.10.6** — the
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
registry's resumable, verified download ("Meeting search model download" in
the network receipts), progress on `yap-search-download {stage, percent,
downloadedBytes, totalBytes}`. Then the setting is switched on
(`config.search_by_meaning`) and a pass starts. **Delete** removes the GGUF
and the index; the runtime stays (the on-device model may use it).

**Never in the cloud**: the server is on 127.0.0.1 and every request goes
through `privacy/net.rs` as `Purpose::Embeddings` ("Meeting search ·
on-device model", on this PC). Retrieval is always local; a cloud Chat
model sees only the passages chosen for its question.

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
`vectors.bin.old` and the index starts again. **Compaction**
(`Store::compact`: a new file swapped in) drops the vectors of passages
that are gone — at once when a note was deleted (or on a run's first pass,
for one deleted while Yap or the search was off), else once they're over
30 % of the file (an edit leaves its old passages' vectors until then).
Brute force: tens of thousands of passages are a few ms.

**When** (`index::init`): 45 s after Yap starts, then on Yap's own events
(`yap-note-changed`, `yap-notes-changed`, `yap-note-deleted`,
`yap-meeting-state`; a burst settles 8 s first) or, every 2 minutes, a
cheap fingerprint of the notes (`notes::stamp`: ids, updated times and
counts — the local API's and Chat's edits too). A failure waits, doubling
to 30 minutes.

**Never in a dictation's way**: a pass waits while a dictation or a meeting
recording runs (`meeting_summary::wait_idle`), and a batch under way is
dropped the moment one starts (`meeting_summary::unless_busy`: the
connection closes) and read again after. Batches of 16 on the card, 4 on
the processor (a few seconds at most). Passages already read are kept, so
a pass cut off loses nothing.

`Status` (`search_status`, `yap-search-status`): `enabled`, `installed`,
`runtimeInstalled`, `installing`, `phase` (`off` · `notInstalled` ·
`waiting` · `indexing` · `ready` · `error`), `error`, `passages`,
`indexed`, `notes`, `indexBytes`, `modelBytes`, `downloadBytes` (the model,
plus the engine when it isn't here), `runsOn` (`gpu` · `partial` · `cpu`,
from the server's log), `modelName`, `license`, `licenseUrl`.
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
meetings" button on an empty chat. Both send with `scope: "meetings"`, and
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

## Settings → Privacy & data → "Search my meetings by meaning"

`settings/MeetingSearchSettings.svelte` (anchors `meeting-search`,
`meeting-search-model`, in `settingsIndex.js`): the switch (disabled until
the model is here), **Search model** ("EmbeddingGemma 300M: a one-time
download of 318 MB", plus ", the on-device AI engine included" and its size
when that isn't here → Download, its progress, then name · size · where it
runs and the licence link), **Search
index** (passages from notes · size, or "Reading your meetings and notes: 12
of 40 passages", "Waiting for your dictation or meeting to finish…") with
Rebuild and Delete… (asked first: `askConfirm`). "What leaves this PC" has a
**Meeting search** row: "Searched on this PC", or "Read and searched by
meaning on this PC".

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

## Tests

- Unit (`cargo test search::`): passages (talk, notes, headings, long
  lines, tails, keys), the store (round trip, int8 accuracy, a torn tail,
  another model's file, compaction), dates (every form, at a fixed clock),
  ranking (stems, BM25, fusion, speakers, per-note cap, the line), Ask
  (dates, the last meeting, the fill rule, context, sources, `cite_missing`
  and its refusals, the not-found words, the answer without a model), the
  index (passages cached per note, a deleted note sweeping the store), the
  embedder's response parsing; an ignored test against the real model
  (`YAP_SEARCH_LLAMAFILE` + `YAP_SEARCH_MODEL`: dentist·teeth 0.49 against
  dentist·beta 0.12).
- e2e (`e2e/meeting-search.spec.js`, a stand-in embedding server
  `e2e/support/fake-embed.js` via the debug-only `YAP_E2E_EMBED_URL`, the
  downloads from `startFakeLocalModels`): Settings' download and index;
  the Meetings view's moments by meaning, each opening at its line; Ask
  across meetings' pills, sources, stripped citation, the prompt's passages,
  the scope kept for a follow-up; no model → the passages; Rebuild, Delete,
  and the network receipts.
