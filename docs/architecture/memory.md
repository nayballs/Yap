# Memory

What Yap remembers about the person (2026-10-22, after Claude's Settings →
Memory, kept on this PC): the store, learning from chats and meetings, what goes
to the models with a question, the `remember` / `forget` / `search_chats` tools,
the names it gives dictation, and Settings → Memory. The Chat it plugs into is
[chat-and-assistant.md](./chat-and-assistant.md); About you (region, name, note)
is [ai-models.md](./ai-models.md); the dictionary passes it feeds are
[dictionary.md](./dictionary.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules (`src-tauri/src/memory.rs` + `memory/`)

- **`memory.rs`** — the store (`<data>/memory.json`: `{items, nextId, read,
  pending, meetings}`, atomic writes, a corrupt file quarantined; never in
  config.json, which Settings' wholesale saves would revert; in memory only in
  a model evaluation, `eval_reset`; a store per thread in unit tests). Every
  change goes through `change(changes, rules)` (or `save_item` / `delete_item`
  / `clear` for the person's own edits) and is a **batch that can be undone**
  (`undo(batch)`: what it added goes, what it changed or removed comes back;
  the last 20, in memory). Each change emits `yap-memory-changed` (Settings
  re-reads); what Yap learned by itself, with the main window on screen,
  `yap-memory-learned {batch, title, description, count}` — the toast ("Yap
  remembered: your sister is Rochelle", Undo, Manage). **Never a Windows
  notification**; the window hidden, nothing is shown (it's in Settings →
  Memory). Blocks for prompts: `you_block()`, `answer_note()`,
  `relevant_block(question)`, `helper_block(cfg)` / `with_helper_block`.
  Commands: `memory_list` (items + `ai {ready, why}`), `memory_save_item`,
  `memory_delete_item`, `memory_set_dictation`, `memory_clear`, `memory_undo`,
  `memory_import_prompt`, `memory_tell`, `memory_import`, `memory_apply`,
  `memory_export`; debug-only `e2e_memory_learn {conversationId}` (a quiet
  chat's pass, now, toast included).
- **`memory/ops.rs`** — items and changes, pure. Five sections: **Profile**
  and **Preferences** ("You": one item each, facts only) and **People**,
  **Projects**, **Topics** (one item per thing: a name, "about" — who or what it
  is to the person — and facts). An item keeps its source (`chat` / `meeting` /
  `manual` / `tell` / `import` / `voice`, with the conversation's or note's id
  and title) and created/updated times, and people and projects their
  "Spell this name in dictation" switch (`dictation`, on). A model writes
  **changes, never a rewrite** (`parse_changes`: `{"changes": [{action:
  add|update|remove, id?, section?, name?, about?, facts?, remove_facts?,
  say?}]}`, lenient about keys, fences and words around the JSON), and `apply`
  keeps the rules whatever it wrote: bounded (names ≤ 60 characters, "about" ≤
  100, a fact ≤ 140; ≤ 8 facts an item, 16 for You, the newest kept; ≤ 60
  people, 30 projects, 30 topics — a full section drops what was updated
  longest ago, never something the person wrote); **one item per thing** (an
  add for someone Yap has — the same name, or for people the same first name
  — updates them; "Sarah" becomes "Sarah Jenkins"; a fact said again isn't
  added twice, a fuller one replaces it); "The user …" taken off facts;
  "about" and "A | B" facts on Profile/Preferences become facts; a nameless
  add to people/projects/topics goes to Profile; Profile and Preferences are
  never removed whole by a pass (only their facts); no secrets, and while
  sensitive topics are off none of those (below); a pass never keeps another
  person's private life or health, nor a one-off request written as a fact
  ("Wanted a recipe for lasagne", `passing`), nor a person not named by a
  name (`looks_like_a_name`: "Nurse at St James's", "Dog"); and **grounding**
  (`Rules::grounding`, `grounded`): a name, an "about" or a fact from a pass,
  a Tell or an import must share a word with what the person said (their
  messages, not the AI's), so a model can't add someone nobody mentioned (the
  1.5B copied the prompt's example "Tom… lives in Glasgow"). `route_preferences` moves
  preference-like facts headed for Profile ("Prefers concise answers…") to
  Preferences. `drop_about_you` leaves out of a chat pass's or an import's
  Profile and Preferences what About you already says (it goes to the AI with
  every question): a short fact naming the person ("Name is Declan", ≤ 5
  words) or one whose meaningful words are nearly all the note's ("Works
  weekends at a bakery in Norwich" next to "I work weekends at a bakery in
  Norwich."), never something new or more specific; a Tell is the person's
  own ask and keeps it. `say_for` writes the toast's words when the model gave none
  ("Rochelle, your sister", "your project Yap").
- **`memory/guard.rs`** — the backstops, pure: `secret_in` (keys and tokens by
  shape, anything about a password, PIN or key, one-time codes, card numbers
  with the Luhn check, account numbers, sort codes, IBANs, ID numbers, street
  addresses and postcodes, emails, phone numbers of 10+ digits — refused even
  when typed by hand, with the reason), `sensitive_in` (health, religion,
  politics, sexuality, ethnicity or immigration, criminal records, union
  membership; the item's own name left out first, so a colleague called
  Christian isn't a religion) and `private_in` (another person's
  relationships, family troubles, money, losses).
- **`memory/prompt.rs`** — what memory sends, pure. Every pass's prompt
  starts "You keep Yap's memory of the user" (the e2e fake model knows them by
  it): `learn_chat_system(sensitive)`, `learn_meeting_system`,
  `tell_system`, `import_system`, sharing one format with three short
  examples (≈ 950 tokens with them). `listing` (memory with ids, Profile,
  Preferences then the newest, ≤ 900 tokens), `chat_excerpt` (the person's
  words whole ≤ 600 characters each, the AI's answers ≤ 240, two earlier
  messages for context), `worth_a_pass` (no "I / my / we / remember…" in the
  new messages = no model call), `pass_input` (memory now, then what to read;
  About you isn't in it — told it, the 4B wrote About you back and left the
  chat's new fact out — `ops::drop_about_you` takes the repeats out after).
  For answers: `you_block` (Profile and
  Preferences, ≤ ~350 tokens), `answer_note` (Preferences for the end of a
  question's context), `relevant` / `relevant_block` (the people, projects and
  topics a question is about: the name ×3, "about" ×2, facts ×1, an item
  needing its name or two of the question's words — "When's my sister's
  birthday?" finds Rochelle — at most 3). `IMPORT_ASK`: the prompt to copy
  into ChatGPT / Claude / Gemini.
- **`memory/learn.rs`** — the passes, through the person's **Chat model**
  (`commands::resolve_chat_endpoint`): temperature 0.2; a model on this PC
  that can't call tools answers under the changes' JSON schema as llama.cpp's
  grammar, every field required (`prompt::schema`; retried without it if a
  server refuses), one that can writes freely (the 4B did worse under any
  grammar, and llamafile ignores a bare `json_object` — model-evals.md
  "Memory"). **Only a model that can call tools learns by itself**
  (`learns_by_itself`: Qwen2.5-1.5B kept junk and took up to two minutes of
  a processor a pass); a small one still does Tell, imports and "Remember
  that…". **The sidecar's rules**:
  the pass holds `local_llm::hold`, waits out a dictation and is dropped if one
  starts (`meeting_summary::wait_idle` / `unless_busy` / `dictation_busy`);
  a background pass never starts a stopped on-device model (it waits for the
  next). **Learning from a chat** (`learn_from_chat`, "Learn from my chats",
  on): only the messages since the last pass (`read`), only when the person
  said something about themselves; then `mark_read`. **The scheduler**
  (`after_turn`, from every `chat_turn`, Voice Assistant included; `start`):
  a chat is read once it's been quiet 5 minutes; another chat starting makes
  the others due now; a pass put off (a dictation, the model stopped, a rate
  limit) is tried again 2 minutes later; what's waiting is kept in
  memory.json (`pending`) so a quit doesn't lose it — read a minute after the
  next start (no model call on quit). Not in test mode or an evaluation (the
  e2e suite and the runner call the passes). **Learning from a meeting**
  (`meeting_ended`, from `meeting_end.rs` when an action plan is written;
  "Learn from meetings", off): its title, attendees and action plan (≤ 1,500
  tokens), people and projects only, once a note. **Proposals** (`propose`
  for "Tell Yap what to change" and imports — an import cut in parts an 8k
  model can read, each seeing the earlier parts' changes): the model's
  changes previewed on a copy (`Proposal {id, kind, changes: [Done]}` — the
  diff) and kept until `apply_proposal(id, include?)`. A Tell's removal of
  the whole Profile becomes taking out the facts that share a word with what
  was asked, and for a "forget …" an update that only takes Profile or
  Preferences facts out keeps the ones sharing a word with it — "Forget
  everything about my old job at Acme" takes "Works at Acme as a nurse", not
  "Lives in Norwich" or the dog (`narrow_you_removals`). Told by the person, sensitive topics
  are theirs to keep; an import follows the setting. **Remember / forget in
  a chat**: `tool(name, args)` for the tool loop (`remember {fact, section,
  name?}` — a new person's fact becomes who they are; `forget {what}` —
  `forget_changes`: the facts it names, as memory words them or nearly, else
  the person, project or topic by name), and for a model without tools
  `spoken(text)` — "Remember that…" / "Forget that…" as a whole message
  (`spoken_request`; not "Remember when…?" nor "Don't forget to…") runs the
  Tell pass and Yap answers itself ("Got it: I'll remember that.").
- **`memory/vocab.rs`** — **memory feeds dictation**, conservatively.
  `entries(items, dictionary, is_word)`: the people and projects whose
  "Spell this name in dictation" is on (the newest 40), as dictionary entries
  with no trigger, after the dictionary's own words, leaving out what it
  already spells; each word of a longer name too ("Sarah Jenkins":
  "Jenkins"). All of them go to the **Whisper `initial_prompt`**
  (`memory::with_names`, ranked by use like dictionary words — `dict_usage`
  counts them — in `pipeline::dictionary_prompt` and the meeting prompt); a
  name is a **near-miss target** for ONNX models (Parakeet) only when it can't
  swallow an ordinary word (`safe_for_fuzzy`): ≥ 4 letters, not
  `learn::is_frequent`, not a first name that's also a word (`NAME_WORDS`:
  Will, Rose, Grace, Mark…), not a word Windows' spell checker knows in lower
  case ("parakeet", "react"; a name like "rochelle" it doesn't). The
  names pass (`fuzzy::apply_names`, in `pipeline::apply_corrections` after the
  dictionary's own) also refuses a near-miss **heard as an ordinary word**
  (a single word the spell checker knows — "lucky" stays with a Lucy in
  memory, "olive" with an Olivia — or a run of them, "row shell"). **No spell
  checker on the PC: no name is a near-miss target** (`memory::Words`); only
  the Whisper prompt and a case fix. `name_hint` tells Settings which applies.
  Cached until memory changes.
- **`memory/export.rs`** — "Export memory": `You.md` (About you's name,
  region and note first, under "About you"; then Profile, Preferences),
  `People.md`, `Projects.md`, `Topics.md` in a .zip written by hand (Deflate,
  flate2's CRC-32, UTF-8 names), each item with its facts and "Updated 5 Oct
  2026 · from a chat (“Trip plans”)".

## What a question gets

`commands::chat_turn` (Chat, the Voice Assistant, the Meetings view's Ask bar):

- **The system prompt** (stable from turn to turn, so a model's cache keeps
  it): after About you (`profile_block`, or `prompt_block` for a model without
  tools), memory's **"You" block** — "What Yap remembers about the user from
  earlier chats… About them: …; How they want answers (follow it): …". The
  Notes view's Ask bar (`note_ask`) ends the same way; the notepad's helpers
  ("What did I miss?", questions, the follow-up email, gaps — `meeting_assist.rs`)
  get `helper_block`: About you's name and note, then the You block, after their
  immutable prompt.
- **The question's `<context>`** (a tool model) or the system prompt (one
  without tools, as the rest of its context): the people, projects and topics
  the question is about (`relevant_block`, ≤ 3), and for a tool model
  Preferences again last (`answer_note`: in the system prompt alone, Qwen3.5-4B
  answered "two or three sentences, no lists" with 138 words and a list).
- **Tools** (a model with tools, `tools::run_tool_loop_with` + `Extra`):
  `remember` and `forget` always (`MEMORY_TOOL_INSTRUCTIONS`), acting only when
  the person's own message asks (`tools::asked_to_remember`: remember, forget,
  keep in mind, "no longer"…; else refused with the chip "Didn't change Yap's
  memory: not asked to") and never after web content in a turn (they're
  `ACTION_TOOLS`); `search_chats` with "Search and reference past chats" on
  (`chats::search`: two of the query's words or one in a title, ≤ 5
  conversations, ≤ 2 lines each around the hit, the asking chat left out).
  The three add ≈ 400 tokens a step (`the_memory_tools_stay_light`). A
  `search_memory` tool isn't offered: the relevant items already come with the
  question (not measured as needed).
- **A model without tools**: "Remember that…" / "Forget that…" through
  `learn::spoken`; and with "Search and reference past chats" on, a question
  pointing back at an earlier chat (`chats::refers_to_past`: "last time", "what
  did we say about…", "you told me") gets the two best-matching chats' lines in
  its system prompt (`chats::past_chats_block`; the evaluations' 1.5B found
  the Lisbon restaurant with it).

## Settings → Memory

`MemorySection.svelte` (Settings → Assistant → Memory, after Commands and
the Voice Assistant; page id `memory`, which the toast's **Manage** opens;
store and toast in `memory.svelte.js`, started by ControlPanel), laid out like
Claude's in Settings' one row style (2026-10-22 review,
[frontend-settings.md](./frontend-settings.md)): the switches, You, "Tell Yap
what to change" and import / export / clear are flat rows; the people,
projects and topics are a card of objects each. Every row is in the Settings
search (`settingsIndex.js`, anchors `memory-search-chats` … `memory-clear`):

- **What Yap learns from**: "Search and reference past chats" (`memorySearchChats`, on),
  "Learn from my chats" (`memoryLearnChats`, on), "Learn from meetings"
  (`memoryLearnMeetings`, off: other people's words), "Include sensitive
  topics in memory" (`memorySensitive`, off) — config.json switches, saved by
  Settings' auto-save. With a Chat model too small to learn by itself
  (`memory_list`'s `ai.learns`), a note says so and points at Language Models.
- With no AI model, a note at the top says so ("Yap needs an AI model for
  memory") with **Language Models** (→ `cleanup#chat`); hand edits still
  work.
- **You**: Profile ("Who you are…"), Preferences ("How you want Yap's AI to
  answer you") — a row shows its facts and "Updated 5 Oct" — then **About
  you's three parts**, as Settings has them now (`about_you_status` with the
  unsaved fields, like About you itself): **Name**, **Region** ("United
  Kingdom (from Windows) · Prices in £ · miles and °C · British spelling · UK
  time") and **Note** (in quotes), or "Not set: …"; each a link row ("In
  About you ›") to its own row in General → About you (`about-name`, `region`,
  `about-me`, the keyboard on it). Memory never writes them, and
  keeps no second copy from a chat, a meeting or an import
  (`ops::drop_about_you`).
- **People / Projects / Topics**: a card each, a row per item (name, who or
  what it is, "Updated …"), newest first; a row opens in place to edit it (name, "Who they
  are to you" / "What it is", facts one per line, where it came from,
  **"Spell this name in dictation"** for people and projects with what it does
  for that name), Delete (Undo on the toast), Save; "Add a person / project /
  topic". A secret is refused with the reason ("Yap doesn't keep a phone
  number in memory…").
- **Tell Yap what to change** ("My dog's name is Beans · Forget everything
  about my old job"): **Suggest changes** → the model's changes as an inline
  diff (+ added, ~ changed with old facts struck and new ones marked, −
  forgotten) → Apply (toast "Memory updated" with Undo) or Discard; "Nothing in
  memory changes for that."
- **Import, export and clear**: **Import** (step 1: the prompt to copy, step 2:
  paste the answer → Turn into memory → each change with a checkbox → Save N),
  **Export** (the save dialog → the .zip), **Clear all** (asked first, Undo on
  the toast).
- The privacy line: kept on this PC; sent with questions to the AI model you
  use (a cloud provider sees it, a model on this PC keeps it here); never sent
  to Yap — and a link to Privacy & data, whose "What leaves this PC" lists
  Memory too.

## Tests

- Unit (`cargo test --lib memory chats:: tools:: fuzzy::`): the parser,
  merging and bounds, secrets / sensitive / private, undo, the toast's words,
  the prompt blocks and relevance, `worth_a_pass`, export (a real zip that
  inflates back), the names pass — **no everyday sentence changes** (≈ 200
  sentences × 90 names, with this PC's spell checker; it showed that names
  alone don't protect ordinary words: without the heard-side check "a rescue"
  became "Archie" and "olive oil" "Olivia oil") — the memory tools through the
  real loop, `search_chats`.
- e2e (`e2e/memory.spec.js`, `memory-no-ai.spec.js`,
  `memory-small-model.spec.js`): the switches, hand edits (a secret refused,
  dictation switch, delete + Undo), Tell's diff + Apply + Undo against the
  fake model, an import reviewed change by change, export, Clear all, a
  learned chat's toast with Undo and Manage, About you's rows under You (set
  there, followed here, not kept twice from an import, at the top of You.md); no AI model; a
  model too small to learn by itself.
- Model evaluations (`evals/scenarios/memory-*.json`, tag `memory`; the
  scenario format's memory fields and the results — Qwen3.5-4B 42/42, then 43/44 with About you, on the
  GPU, Groq gpt-oss-20b 8/8, Qwen2.5-1.5B — are in
  [`docs/model-evals.md`](../model-evals.md) "Memory").
- Real-app robot tests (`e2e-real/memory.real.js`): names Yap remembers fixed in a
  real dictation, everyday sentences untouched.
