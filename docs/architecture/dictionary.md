# The dictionary

The correction dictionary: near-miss (fuzzy) matching, several ways one word is heard, your own name and names spelled far from how they're said, the Whisper vocabulary's order, learning words from your corrections (dictations, meeting lines, summaries), snippets offered for text you use often, plus the Dictionary view. The exact pass, snippets and "press enter" are in [dictation.md](./dictation.md) (the pipeline order); see also [`docs/fuzzy-dictionary.md`](../fuzzy-dictionary.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`dictionary/variants.rs`** — **one word, several ways it's heard**
  (2026-10-25): "Kubernetes" ← "cooper netties", "cube an eighties".
  **On disk it's rows**: one `{from, to, fuzzy, snippet, starred}` per way,
  the shape config.json always had, so an older Yap sharing
  `%APPDATA%\yap\config.json` (the installed nightly beside Yap - Dev)
  reads and writes every one back — no migration, nothing it can drop
  (unit-tested against a stand-in older Yap that keeps only `{from, to,
  fuzzy, snippet}`). `DictionaryEntry::also_heard` is never written
  (`skip_serializing`): an entry that arrives with `alsoHeard` is saved as
  rows of its own (`rows`, in `config::save`); in memory it carries how
  memory's names and yours are said (`spoken_names.rs`). **The Dictionary
  view groups the rows** that write the same word exactly into one entry,
  its star and ≈ from its first row (`dictionaryList.js` `groupRows`), and
  saves it back as rows, the entry's star and ≈ on each, together where
  its first row was (`entryRows`). The exact and near-miss passes take
  every row as ever; AI cleanup is told `"cooper netties" or "cube an
  eighties" → "Kubernetes"` (`llm::dictionary_suffix` groups by the word);
  the **Whisper prompt** has the written word once
  (`config::dictionary_prompt`). **New ways in** are new rows for the word
  (`add_heard`, with its star): a word offered from your corrections; an
  import's rows join the entry in the view.
- **`dictionary/snippet_offer.rs`** — **"Type that often? Save it as a
  snippet"** (Wispr Flow's nudge, by behaviour; 2026-10-25). **What counts**
  (`phrases`, pure): in a dictation's text, every link, email address and
  phone number (9–15 digits); an address up to its UK postcode or US ZIP
  ("12 Example Road, Anytown ZZ1 1ZZ" wherever in the sentence, from the
  house number, with a "Flat 3" before it); each sentence or paragraph (and
  a short dictation of up to three, whole) that's a sign-off with a name
  ("Best wishes,\nSam"), has lines of its own, or is long (≥ 40 characters
  and 6 words) — compared by their words alone (`key`). **Where from**: the
  week's dictation history (`history::dictation_texts_since`; history off,
  none) and what was typed right after a dictation on its line (`learn.rs`'
  watch, `Watch::typed_after`; only with "Suggest words from your
  corrections" on — a line break ends that watch, so a typed sign-off on
  new lines isn't seen). Typed text is never kept: a fingerprint of its
  words and when, a week (`snippet_suggestions.json`); the third time it's
  offered from the text in hand. **When** (`verdict`, pure): seen 3 times in
  7 days, not a snippet's text (nor inside one: a snippet that expanded in
  those dictations), never offered before (each phrase once, whatever the
  answer; one left to fade may come back), one offer a day, "Suggest
  snippets for text you use often" on. Links, addresses and the like beat a
  plain long phrase, then the longest. Worked out first, then it waits out
  a dictation (≤ a minute; notice kind `snippet-suggestion`, held and
  dropped unseen after a minute). **The offer**, in one place
  (`notice::place`): a toast in the main window while it's focused
  (`yap-snippet-suggestion`, `notices/snippetSuggestions.js`: Save snippet
  / Not now / "Don't suggest this again"), else a Yap bar card (✕ = Not
  now, 30 s fade = no answer); never a Windows notification. **Save
  snippet** adds `{from: the phrase to say, to: the text, snippet: true}`
  (`settings::change_config`, then `yap-dictionary-learned` for the
  Dictionary view) and says "Say “my address” and Yap types it" with Open
  Dictionary. **The phrase to say** (`trigger`): "my address", "my email",
  "my number", "my <site> link", "my sign-off", else "my" and the text's
  first two words; "2", "3"… after it when a snippet says that already.
  "Don't suggest this again" turns them off (the Dictionary view's switch,
  `snippet_suggestions_enabled` / `_set`, its own store, not config.json).
  Logs: counts, lengths and kinds. Debug-only `e2e_snippet_offer_reset`.
- **`dictionary/spoken_names.rs`** — **names a speech model spells far from
  how they're written** (2026-10-25): "Shivon" for "Siobhan", "Neev" for
  "Niamh", "Seersha" for "Saoirse". By their letters, and their Soundex and
  sound keys, "Spain" is nearer "Siobhan" than "Shivon" is: no distance
  finds them. So a built-in list (`said_as`: Irish, Scottish Gaelic, Welsh
  and a few other names, accents folded) gives each a few respellings of how
  it's said, which memory's names and your own carry as `alsoHeard`
  (`memory/vocab.rs` `add_name`; one-word names that are near-miss targets
  only). The names pass (`fuzzy::apply_names`) takes a match through one of
  them only when what was heard isn't an ordinary word and it's that
  respelling exactly or — three sounds or more — a letter off and sounding
  the same (`near_how_said`); a near-miss of the name's own spelling is
  Handy's score as before. Left out: respellings that are a name of their
  own ("Owen", "Rory", "Killian", "Reece"; Siân altogether: "Shan" is a
  letter from "Shane"), and some only for your own name ("Dermot" said by a
  Diarmuid is him; another person's Dermot isn't). Measured: memory's
  `no_everyday_sentence_changes` with 109 names (these 20 among them) and
  ~220 sentences, other people's names (Sharon, Shane, Shona, Owen, Rory,
  Dermot, Sean…) and near words (naive, sieve, Spain) included — none
  changes with this PC's spell checker. (Found while measuring, not new:
  the names pass takes another name one letter off a remembered one —
  "Sean" with a Sian remembered — as Handy's score allows.)
- **`dictionary/fuzzy.rs`** — fuzzy dictionary correction (Handy `audio_toolkit/text.rs` port):
  1–3-word n-grams vs the dictionary's `from`/`to` spellings, normalized Levenshtein
  + Soundex phonetic boost (the `natural` crate's nonstandard variant, replicated
  exactly), threshold 0.18, ≥3-char terms, case/punctuation preserved; a single
  everyday word (`learn::is_frequent`) is never a near-miss, only its exact
  spelling ("could" stays "could" with "cloud → Claude" in the dictionary,
  "motion" never becomes "Notion"; 2026-10-21); shortest-first
  n-gram choice (deliberate fix over Handy's word-swallowing longest-first greedy),
  and never a run whose first word adds nothing (`first_word_adds_nothing`: the
  later words make the same term as closely alone), so the word or list marker
  before a term survives ("to Kubernetes", "1. kubernetes"; found by the real-app
  robot, 2026-10-17). Max 3 words per n-gram, so "Chat G P T" (4) leaves a "T".
  The text between the words is kept as it was, line breaks included, and a
  run never reaches over a line break (an email's "Hi Sam," keeps its own
  line: until 2026-10-24 every pass here — the dictionary's, memory's names,
  the names on screen — put the words back together with single spaces, and
  the real-app robot's Gmail page got "Hi Sam, Thank you for the figures.").
  Runs after `apply_dictionary` for ONNX models (`dictionary_fuzzy`, default on);
  snippets are never candidates; every way an entry is heard is one
  (`variants.rs`). **`apply_names`** is the same loop for **your own name**
  (About you's "What should Yap call you?", `memory/vocab.rs` `own_name`,
  2026-10-25: a hidden term — never an entry to manage — by the same rules
  as memory's names, unless the dictionary spells it; About you's row says
  "Dictation spells it this way too, and fixes near-misses of it" or, where
  it can only go in the prompt, "Whisper models are told how to spell it",
  `about_you_status` `nameHint`) and the names Yap remembers
  (`memory/vocab.rs`, run after the dictionary's own pass in
  `pipeline::apply_corrections`), refusing a near-miss heard as an ordinary
  word ("lucky" with a Lucy in memory, "row shell" for Rochelle), and one
  through how a name is said only by `near_how_said`
  (`spoken_names.rs`); the Whisper prompt gets your name, then memory's,
  after the dictionary's words (`memory::with_names(dictionary,
  about_name)`), and the screen's pass never changes them
  (`screen_terms::protected`). See [memory.md](./memory.md).
  Also `is_prompt_echo` (OpenWhispr `dictionaryEchoFilter` port) used by `dictation/stt.rs`
  against Whisper hallucinating the dictionary prompt on silence, and, for
  `dictionary/learn.rs`, `spelling_distance` (Handy's score on the letters alone, no
  length gate) and `sound_distance` over `sound_key` (a Soundex/Metaphone-
  style consonant key: alike sounds merged, silent letters and vowels
  dropped — "cube and eighties" ≈ "Kubernetes", "kpnts"/"kprnts").
- **`dictionary/screen_terms.rs`** (+ `dictionary/screen_terms/extract.rs`) — **"Spell names the way
  they're written on screen"** (`config.screen_names`, default on; Aqua
  Voice's "Deep Context", local and without screenshots; 2026-10-22).
  **The read** (`begin_dictation(target, read)` from
  `pipeline::capture_target`, a thread of its own): the target window's
  visible text over UI Automation — its title; the document the focus is in
  (the outermost element with a TextPattern above it: a web page, an editor,
  a mail being written) through its **visible ranges**, or for a web page
  (a `Chrome_WidgetWin` window or a "Chrome" document) **around the focus**
  (± 6,000 characters from the caret: Chromium works out a page's visible
  ranges or its whole tree on its UI thread — 5+ s on a long page, and it
  kept Edge busy after the call timed out —, while text around the focus
  costs what it reads: ~50–100 ms on a 3,000-paragraph page or a
  5,000-line code view); then, unless a web page gave enough, a **bounded
  walk** of the window's visible elements (one `FindAllBuildCache` of
  children a parent, ≤ 250 calls / 2,500 elements: names of labels, links,
  list/tree items, tabs, headers, buttons, cells; other text boxes' and
  documents' text ≤ 2,000 characters each). ≤ 20,000 characters, 400 ms per
  call, 1.5 s in all. Refusals as `pasting/selection.rs`: the target window's
  process only, never a password box, never Yap's own windows, never a
  password manager's (`NEVER_READ`), never in a test run (robot mode: the
  test's own windows, `realtest::permits`). **The terms** (`extract`, pure):
  capitalised names that aren't everyday words, full names ("Siobhan Ní
  Bhriain", through particles, never across punctuation), CamelCase,
  acronyms of 3+ capitals, words with digits and a capital ("Qwen3",
  "M365"; not "value12", "10kg", "3rd"), "@handles" and email addresses;
  brackets, quotes and code punctuation split words (`useState('Zentrix')`);
  paths, URLs, identifiers with `_` and dotted words aren't terms; everyday
  words dropped however written (`learn::is_frequent`, then Windows' spell
  checker — without one a plain capitalised word only counts inside a full
  name); one spelling per term (the most frequent; "Zentrix" over a
  heading's "ZENTRIX"), the most frequent first, ≤ 200. Kept in memory for
  that dictation only (`peek` at transcription, `take` once at
  delivery, `forget` on a cancel; a read still going is never waited for
  and its late result is dropped); the log (debug) has counts and times,
  never a term. **The uses**: a **near-miss pass** after the corrections,
  for every model (`correct`, gated by "Catch near-misses" too): full names
  first, then single terms, each a target only when it can't swallow an
  ordinary word (`memory::vocab::safe_for_fuzzy`: ≥ 4 letters a word, not
  everyday, not a name-word, not spell-checker known; a full name: no word
  ordinary, ≥ 8 letters), and `fuzzy::apply_screen` takes a near-miss only
  when what was heard isn't ordinary words only ("row shell", "zen tricks",
  "hash map" with a "HashMap" on screen stay as said), spells **and**
  sounds close (plain edit distance ≤ 0.34, `sound_distance` ≤ 0.4, on top
  of Handy's score), never ends with an everyday word the name doesn't end
  with (2026-10-29, found by the chat robot: "Priyanka can you check?" had
  become "Priyanka Raman you check?" with Priyanka Raman on screen; "Priyanka
  Ramen" is still fixed), and is never a spelling the dictionary or memory gives
  (`protected`: they have the final say; those terms are no targets
  either); and only when the speech model wrote what it heard as a name (a
  capital: "Rachelle", "Zentrics", "GPTOSS" — Parakeet wrote a spoken "row
  shell" as "rouchell", which it then took for "Rochelle"); one word in
  another case ("kubernetes") is always fixed. **Not in the Whisper
  prompt** (`whisper_prompt`, `WhisperNames::Off`): measured with real
  speech through large-v3, the names in its prompt — as a list or ending
  as a sentence — made it hear "row shell" as "Rochelle" with Rochelle on
  screen (and as a list dropped full stops, "Zen Tricks"), while the pass
  alone got every name the prompt did ("Siobhan Nibrian" → "Siobhan Ní
  Bhriain") and no everyday sentence changed; robot mode can put them
  back to measure again (`YAP_REALTEST_SCREEN_WHISPER` = list | sentence |
  off, `screen-names-stt.real.js`). **AI
  cleanup** gets "Spell these names as written: Zentrix, Kubernetes." in
  the dictation's own message, never the system prompt (`llm::
  cleanup_with_names` / `with_names_line`: before "Transcript:", at the
  start of the transcript's message when it's split, so a hybrid model's
  cached instruction stays the same) — ≤ 8 terms the raw transcript seems
  to contain (`in_transcript`: the same letters, or spelled or sounding
  alike, looser than the pass — the model has the context; a handle only
  as it's said, "at zentrix dev"), never one the dictionary or memory
  spells. That line is all that can leave the PC (with the transcript, to
  a cloud cleanup provider; Settings and Privacy & data say so); the
  screen's text never does.
  **Code names** (`dictionary/screen_terms/code.rs`, 2026-10-29; Wispr
  Flow's IDE variable and file recognition, by behaviour, done locally): in
  VS Code, Cursor, Windsurf and the editors built on VS Code, Zed, Visual
  Studio, a JetBrains IDE (its title only: Swing shows UI Automation
  nothing else) or a terminal (`app_of`), the same read gives that
  dictation's **file names** (`name.ext` with a code, config or docs
  extension, or a dot file; a terminal's paths kept as shown, "Read(src/
  services/userService.ts)"; URLs, emails, versions and "e.g." left out)
  and **symbols** (identifiers of two parts or more — camelCase,
  PascalCase, snake_case, an acronym's run, digits apart — kept only with
  three parts or a part that isn't an everyday word: `getUserById`,
  `MAX_RETRY_COUNT`, `kubeConfig`, never `userName`, `forEach`), ≤ 100
  and 150, the most frequent first (`extract`, pure). An editor's window
  gets one more time-boxed call for its **tabs and tree items** (one
  `FindAllBuildCache` of TabItem/TreeItem on screen, ≤ 400: a walk from the
  window's top may not reach them), and every read now keeps the focused
  element's **labels** (class, name, help text, automation id; never
  logged) for `focus_of`: the code editor (Monaco's input, class
  "inputarea", unless its label says chat), an **AI chat box** (VS Code's
  "Chat Input", Cursor's "aislash-editor-input", Windsurf's Cascade, an
  agent's or composer's box) or the integrated terminal (xterm.js's input).
  **Spoken forms** (`CodeNames::respell`, run first in `screen_terms::
  correct`, for every model, with "Spell names the way they're written on
  screen"): the words said, in order, spelling a name's parts — joined or
  apart ("userservice"), a part spelled out ("I D"), a digit as a number
  word, a file's extension after "dot" or a dot in the text (or said as
  "typescript", "markdown"…), "dot" between a stem's own dots
  ("vite dot config dot ts") — never across a sentence's punctuation or a
  line break; one part of ≥ 5 letters may be a letter off with "Catch
  near-misses" on; the longest name first (the file over its class).
  "user service dot ts" → `userService.ts`, "get user by id" →
  `getUserById`. **AI cleanup** gets the names a transcript seems to say
  first in its "Spell these names as written" line (`said_in`; ≤ 8 in
  all). **The editor's own text** reaches UI Automation only with its
  screen reader mode on (VS Code's "Screen Reader Optimized": Monaco
  exposes a page of code around the cursor then, an empty box otherwise).
  Yap doesn't ask for it: the mode makes the editor play sounds and change
  a few of its ways for everyone who doesn't use a screen reader; without
  it the tabs, file tree, title, breadcrumbs and an open Outline still
  give file names and symbols. File tagging in AI chat boxes and coding
  CLIs uses these names (`pasting/file_tags.rs`, [pasting.md](./pasting.md));
  @mentions in team chats use the screen's people's names
  (`pasting/mentions.rs`). Test mode reads no window: `stage_for_test`
  (debug) stands in. Unit tests: `code.rs` (parts, extraction, spoken
  forms, the focus, bounds), `screen_terms.rs`
  (`an_editors_code_names_said_aloud`).
- **`dictionary/dict_usage.rs`** — how much each dictionary word is used, for the Whisper
  prompt's order (Wispr Flow ranks its vocabulary the same way):
  `<data>/dictionary_usage.json`, word (lowercased `to`) → `{uses, lastUsed}`
  (unix secs) — its own store, never config.json (Settings' wholesale saves
  would put old counts back); cached in memory, atomic write, corrupt file
  quarantined. `record(dict, text)` (from `apply_corrections`: dictations,
  meeting lines, Upload) counts every dictionary word in the corrected text —
  a correction that fired, or a word the recognizer (with the prompt) spelled
  right by itself; a no-op in unit tests. `rank` (pure) / `ranked`: the
  words you starred first (the Dictionary view's ☆, `DictionaryEntry.starred`;
  Wispr Flow's context puts starred words first too, 2026-10-25), then used
  in the last day, then 14 days, then the rest; within each most uses first;
  then the order added. The meeting prompt ranks the dictionary but keeps the
  attendees ahead of it. `dictionary_usage` hands the counts to the view's
  "Most used" sort. Best-effort everywhere.
- **`dictionary/files.rs`** — the Dictionary view's files (2026-10-25).
  `dictionary_import_pick` opens Windows' Open dialog **in Rust** (CSV, text,
  TSV; no path ever crosses the IPC boundary, so the page can't name a file
  to read) and returns `{name, text}`: ≤ 5 MB ("too big to be a dictionary"
  past that), decoded as UTF-8 (BOM or not), UTF-16 with a BOM (Excel's and
  Notepad's "Unicode"), else Windows-1252 byte for byte (Excel's plain
  "CSV"). `dictionary_export_save {text, name, kind}` opens the Save dialog
  (CSV or text) and writes the page's text; the saved file's name back, or
  `null` on Cancel. The log has sizes only, never a file's name or words.
  Debug-only `e2e_dictionary_file` names the file the next Import… reads or
  Export… writes (a test run never opens a dialog).
- **`dictionary/learn.rs`** — **"Suggest words from your corrections"**
  (`config.learn_words`, default on; Wispr Flow's dictionary auto-learn as
  measured from its shipped code — `references/wispr-flow/rea-teardown/
  05-dictionary-learning.md` — and OpenWhispr's `correctionLearner.js`
  alignment, but local and asked, never assumed). **The watch**
  (`after_paste`, from `run_stt` after a paste with no submit key, not into
  a terminal (`app_kind`: its text is the program's output too); never in a
  test run; the rules are `Watch`, pure): a thread holds the focused field
  (`selection::WatchedField`: the target window's process, not a password
  box, not read-only) and reads it **whole** (≤ 15,000 chars, the caret in
  it), finds the pasted text right after the paste (the occurrence at the
  caret) and remembers ~24 chars either side of it (`Anchors`, with an
  8-char fallback when those were edited; a right neighbour that was the
  rest of the field — a space or a line break after a paste at its end,
  which the paste has too — ends the span where the field's end still is),
  then every 700 ms (reads ≥ 400 ms
  apart) for **60 s** finds the span between them again — **focused or
  not** (no end on a focus/app change: a fix often follows a glance
  elsewhere). Each read's span within the **drift gate** (`within_drift`:
  char Levenshtein, lowercased, ≤ `0.4/(len/30+1)+0.2` of the paste's
  length) becomes the **last good edit**. It ends (`End`, named in the
  debug log: `time_up`, `next_dictation` (`pipeline::dictation_state`, or
  any paste of Yap's — "Use what I said", a transform, a rewrite, Paste
  last transcript: `yap_pastes`, from `text_injector` — so Yap's own text
  is never judged as the person's fix),
  `field_emptied` (a chat message sent), `region_emptied` (the dictation
  deleted), `region_gone` (its neighbours deleted), `anchor_mismatch`,
  `enter_pressed` (a line break added right after it), `unreadable` (3
  failed reads in a row), `too_long`) and, **whatever the end**, the last
  good edit is judged — "fix a word, press Enter" still counts (a fix and
  the Enter in one read too). In memory only; logs carry lengths, counts
  and the end's name. **The candidates** (`suggestions`, pure,
  unit-tested): pasted vs edited
  words aligned by LCS (case- and edge-punctuation-blind); the gaps between
  matches are the changes. Kept: 1–4 words → 1–4 words (spelled-out letters
  count as one) whose new text looks like a term — a capital inside a word,
  an acronym, digits, a word that isn't an everyday one (the built-in
  `FREQUENT` list with simple stemming, then **Windows' spell checker**,
  `ISpellChecker` for the user's locale + English, read-only), a capital
  mid-sentence, or several words heard as one non-everyday word ("power to
  keep" → "Parakeet") — and that spells or sounds like what Yap typed
  (`fuzzy::spelling_distance`/`sound_distance` ≤ 0.5, ≤ 0.3 for keys of ≤ 3);
  plus a case change that makes a term of a non-everyday word ("github" →
  "GitHub"). Skipped: insertions/deletions, punctuation, a new word ending
  in a contraction/possessive (`'s 't 'll 've 're 'd 'm` or a bare `'`;
  "O'Brien" is fine), a single everyday
  word heard ("motion" → "Notion" would rewrite every "motion"), only
  function words, what a correction is heard as already (any of its ways),
  a word the dictionary spells another way ("JSON" there, "json" typed),
  and edits rewriting most of the dictation. A word the dictionary writes
  exactly so, heard a new way, is offered as **another way it's heard**
  ("Add another way you say “Kubernetes”? Yap heard “cube an eighties”.";
  Add puts it in that entry, `variants::add_heard`; 2026-10-25). One per
  spelling (case-blind), terms with a capital first, ≤ 3. **The prompt** (`offer`): "Add
  “Kubernetes” to your dictionary? Yap heard “cube and eighties”." — Add /
  Not now / "Don't suggest this"; in one place (`notice::place`): a toast
  (`yap-word-suggestion`, `wordSuggestions.js`, kind `tip`) while the main
  window is focused, else a Yap bar card (`word-suggestion`, icon
  `dictionary`; ✕ = Not now, 30 s fade = no answer; it waits out a
  dictation, up to a minute), else the window's toast if it's on screen;
  neither → nothing (never a Windows notification). Add
  (`word_suggestion_respond`) appends `{from: heard, to: term, fuzzy: true}`,
  saves config.json, updates the pipeline's live config and emits
  `yap-dictionary-learned {entries, added}`, which Settings adopts (and
  passes to DictionaryView as `yap-dictionary-external`). Not now twice (✕
  counts) or "Don't suggest this" retires the pair; one prompt per 3 min.
  **A word you remove** from the dictionary is never suggested again,
  however it's heard (Wispr's implicit blocklist): `settings::save_config`
  (every surface's save) hands the old and new dictionaries to
  `on_dictionary_saved`, and a correction whose trigger and spelling are
  both gone (an edit of one isn't a removal; snippets don't count) goes
  into the store's `removed` terms. **A meeting line the person fixed**
  (`offer_for_line`, from `notes/note_edits.rs`; the notepad and the Notes view):
  the same `suggestions` over the line's words before and after the edit
  (its start a sentence's), the same answers left out, no watch and no
  three-minute gap before it (they just fixed it; it still counts as the
  last prompt), and the prompt handed back to the window that made the edit,
  which asks in its own toast (`wordSuggestions.js` `askWordSuggestion`;
  Open Dictionary only where there's a sidebar) — answered with
  `word_suggestion_respond` like any other. **A meeting summary the person
  edited** (`offer_for_summary`, 2026-10-25; `summary_word_offer {before,
  after}` from the Summary tab's editor at Done — the notepad's
  `summaryEditor.svelte.js` and the Notes view's `notesState.svelte.js`,
  through `noteEdits.js` `offerSummaryWords`; the summary as it was at Edit
  against what Done leaves): `summary_swaps` (pure) aligns the two as a
  dictation's edit, task boxes left out, and offers only when **every**
  change is a swap of 1–3 words for 1–3 words that `judge` (or
  `judge_case`) takes — a name or a term, spelled or sounding alike, never
  one everyday word heard — and there are at most two different ones (the
  same swap in several places counts once); one added, deleted or longer
  change anywhere is a rewrite and nothing is offered ("Shivon" →
  "Siobhan" yes; Friday → Monday, Sam → Priya, a sentence added: no). The
  same prompt and answers as a line's. **Typed right after a dictation**
  (`Watch::typed_after`: the span is the pasted text unchanged, then more,
  on its line) goes to `snippet_offer.rs`. Answers in
  `<data>/word_suggestions.json` (not config.json: Settings' wholesale
  saves). Debug-only `e2e_learn_correction {pasted, edited?, before?,
  reads?, fresh?}` stages an edit for the e2e suite (skips the 3-min gap):
  the edited span, or `reads` — the field's whole text read by read (`null`
  = a failed read) — which the real `Watch` runs over (`staged_watch`).

## The Dictionary view

- **`views/dictionary/DictionaryView.svelte`** =
  the dictionary (promoted out of Settings → Advanced; syncs with
  Settings' cfg copy via `yap-dictionary-changed`/`-external` events) in two
  sections over the one list: **Corrections** (heard → replace with, the ≈
  near-miss button, "Catch near-misses", and "Suggest words from your
  corrections" — `learnWords`, `dictionary/learn.rs`; it rides along in the
  `yap-dictionary-changed` detail with `entries` and `fuzzy` so Settings'
  copy keeps it) and **Snippets** ("You say" pill →
  "Yap types" growing text box, line breaks kept; nothing seeded). Settings'
  `cleanConfig` (`settings/configDefaults.js`) keeps each entry's `snippet`
  flag — drop it and an auto-save
  turns snippets into corrections. Settings → Dictation → Typing into apps has "Say
  “press enter” to send" (`voiceEnter`) under Auto-submit; the key picker
  shows when either is on. The view saves half a second after an edit,
  onto config.json as it is then (`save_config` with `base`); a save that
  fails (a full disk) is said with Try again (`notices/saveErrors.js`,
  "Couldn't save your settings: the disk is full.", 2026-10-30; it was
  swallowed until then): the list keeps the edit, the next change saves it
  too, and Try again saves it now.
- **One correction, several ways it's heard** (2026-10-25, `variants.rs`):
  "+" beside "Heard" (and "Add another way it's heard" in the row's menu)
  adds a box under it, "Also heard as", each with its ×; the list shows
  the word once. config.json's rows writing the same word are grouped as
  the view loads them (`dictionaryList.js` `groupRows`; Settings' copy
  stays rows) and written back as rows (`entryRows`); the search matches
  every way, a delete's Undo brings them back, and Delete… says "Yap stops
  changing “a” or “b” to “K”".
- **Snippets' length and warnings** (2026-10-25, Wispr Flow's richer
  snippets): under a snippet's text, "N characters" (at most 5,000, as an
  import takes; the box stops there); under its phrase, a warning
  (`snippetWarnings`, the corrections indexed once per change so 2,000
  entries stay quick) when it's another snippet's phrase, close to one
  ("my adress" beside "my address": a fifth of the letters apart), has words
  a correction changes before snippets are looked for ("the jaison file"
  with "jaison → JSON": Yap writes "the JSON file" first and the snippet
  never fires; a one-word way heard with ≈ on, near-misses of it too), or
  is a dictionary word (said in any sentence, it types the snippet). Not
  done: rich text (bold, links, lists pasted as HTML beside plain) — it
  needs a second clipboard format through the paste path
  (`text_injector.rs`, the robot tests).
- **"Suggest snippets for text you use often"** under Snippets
  (`snippet_offer.rs`; its own store, read and set by
  `snippet_suggestions_enabled` / `_set`).
- **For a dictionary of hundreds** (2026-10-25; ROADMAP's "The Dictionary
  view", Wispr Flow's dictionary). A sticky bar over the list:
  - **Search** (Ctrl+F, Esc clears): filters as you type over what's heard
    and what's written — a word's every heard-as variant, a snippet's
    phrase and text —, every word of the query, any case or accent
    (`dictionaryList.js` `matches`); "N of M match". Ctrl+F is the page's
    only when nothing's over it: with Settings open it's Settings' search
    (any visible `aria-modal`, the right-click menu, the confirm dialog or
    the import preview takes it), and the in-page hotkey fallback
    (`settings/hotkeyFallback.svelte.js`, capture phase) still wins a
    binding of the same keys.
  - **Stars and sorts**: ☆ on every row (and Star in its right-click menu)
    sets `starred` (config.json keeps it only when set; Settings'
    `cleanConfig` keeps it too). Sort: **Starred first** (the default; then
    the order added), **A–Z** (a correction by its word, a snippet by its
    phrase), **Most used** (`dictionary_usage`: uses, then the last use; a
    Used column shows the count) and **Recently added** (the list's own
    order backwards: + Add, a suggested word and an import all append).
    The sort is remembered in the window's `localStorage`. The order is
    worked out when the search, the sort or the entries change — never as
    a row is typed in, so a row doesn't jump from under the cursor; a row
    just added with + Add stays at the bottom whatever the search or sort.
  - **Select several**: a checkbox on every row (and one per section for
    every row shown), Shift-click for a range, Ctrl+A in the list (outside
    a text box) for everything shown, drawn or not; Esc clears, Delete asks
    to delete. The bar for the selection: Star / Unstar, ≈ Catch
    near-misses, Exact only, Export…, Delete… (asked with
    `ui/ConfirmDialog.svelte`: "Delete 3 entries? … You can undo it
    straight after.") and ✕; ← → move between its buttons. A **delete has
    Undo** in its toast — the bulk one and a row's × alike: the entries go
    back where they were (`restoreRemoved`, matched by what they say), from
    the view or, after leaving it, on config.json.
  - **Import…** (`dictionary/files.rs` picks and reads; `dictionaryFile.js`
    reads what's in it): **CSV** — comma, semicolon or tab; quotes, `""`
    and line breaks inside quotes; a header naming the columns (Heard,
    Written, Kind, Near-misses, Starred, or the obvious other names) or
    none (heard, written; one column = words) — or **text**, one entry a
    line: `heard → written` (or `->`, `=>`) or a word alone; `# notes`,
    bullets and quotes around words are left out. A word alone becomes a
    correction of itself ("Zentrix → Zentrix"). Rows writing the same word
    (≈ alike) become one entry with several ways it's heard, or join the
    entry already writing it ("Another way it's heard" in the preview;
    `importChanges` `extend`, and the Undo takes those ways off again).
    Then a **preview**
    (`ImportPreview.svelte`) before anything changes: **to add** (each
    ticked, untick to leave out), **spelled differently** — the same thing
    heard written another way, or a word you have spelled another way
    ("JSON" here, "Json" there) — with Keep yours (the default) / Use the
    file's each, **already in your dictionary**, and **left out** (lines
    said twice, and lines that aren't an entry, with their line number and
    why). Add appends what's new, respells what you chose (every
    correction spelling that word), and its toast has Undo (`undoImport`).
  - **Export…** — everything (the bar's Export…) or the selection (the
    selection's): **CSV** (snippets, stars and ≈ kept; a BOM so Excel
    reads it as UTF-8; a cell starting with `=`, `+`, `-`, `@`, a tab or a
    line break gets an apostrophe first, so a spreadsheet never runs it as
    a formula, and Import… takes it off; Import… reads it back the same) or **text**
    (`heard → written` a line, corrections only: "Snippets need CSV" says
    how many were left out). A word heard several ways is a row (a line) a
    way, the written word on each.
  - **2,000 entries**: 200 rows drawn at a time, more as the end of the
    list scrolls into view (or "Show 200 more"), and off-screen rows skip
    layout (`content-visibility`). Measured in the stub debug build
    (`e2e/dictionary.spec.js`): the view on screen ~150–200 ms, a search
    ~10 ms, a keystroke in a row ~10 ms. Import… / Export… load when first
    used (the main window's chunk stays under 700 KB).
  - Leaving the view with an edit not saved yet (the half-second wait)
    saves it at once.
  - Tests: `e2e/dictionary-list.spec.js` (the pure parts, no app: search,
    sorts, CSV and text parsing, the import's plan and choices, export
    round trips — several ways heard included —, both Undos, the snippet
    warnings and their speed at 2,200 entries), `e2e/dictionary.spec.js`
    (the view, in light and dark; several ways heard added, saved,
    corrected in a dictation, exported, imported and undone; a snippet's
    length and warnings) and `e2e/dictionary-learning.spec.js` (your own
    name in a dictation; the snippet offer — toast, bar card, "Don't
    suggest this again" and the switch; a summary edit's word offer). Rust
    unit tests: `variants.rs` (an older Yap reading and re-saving the rows
    keeps every phrase; rows out of an entry; the passes and the prompt), `snippet_offer.rs` (what counts, three in a week, once, a
    day, the phrase to say), `learn.rs` (`summary_swaps`, `typed_after`,
    another way heard), `spoken_names.rs`, `memory/vocab.rs` (your own
    name; the respellings against everyday sentences).
