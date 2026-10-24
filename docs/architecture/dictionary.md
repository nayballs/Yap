# The dictionary

The correction dictionary: near-miss (fuzzy) matching, the Whisper vocabulary's order, and learning words from your corrections, plus the Dictionary view. The exact pass, snippets and "press enter" are in [dictation.md](./dictation.md) (the pipeline order); see also [`docs/fuzzy-dictionary.md`](../fuzzy-dictionary.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

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
  snippets are never candidates. **`apply_names`** is the same loop for the
  names Yap remembers (`memory/vocab.rs`, run after the dictionary's own pass
  in `pipeline::apply_corrections`), refusing a near-miss heard as an
  ordinary word ("lucky" with a Lucy in memory, "row shell" for Rochelle); the
  Whisper prompt gets those names after the dictionary's words
  (`memory::with_names`). See [memory.md](./memory.md).
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
  of Handy's score), and is never a spelling the dictionary or memory gives
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
- **`dictionary/dict_usage.rs`** — how much each dictionary word is used, for the Whisper
  prompt's order (Wispr Flow ranks its vocabulary the same way):
  `<data>/dictionary_usage.json`, word (lowercased `to`) → `{uses, lastUsed}`
  (unix secs) — its own store, never config.json (Settings' wholesale saves
  would put old counts back); cached in memory, atomic write, corrupt file
  quarantined. `record(dict, text)` (from `apply_corrections`: dictations,
  meeting lines, Upload) counts every dictionary word in the corrected text —
  a correction that fired, or a word the recognizer (with the prompt) spelled
  right by itself; a no-op in unit tests. `rank` (pure) / `ranked`: used in
  the last day, then 14 days, then the rest; within each most uses first; then
  the order added. The meeting prompt ranks the dictionary but keeps the
  attendees ahead of it. Best-effort everywhere.
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
  8-char fallback when those were edited), then every 700 ms (reads ≥ 400 ms
  apart) for **60 s** finds the span between them again — **focused or
  not** (no end on a focus/app change: a fix often follows a glance
  elsewhere). Each read's span within the **drift gate** (`within_drift`:
  char Levenshtein, lowercased, ≤ `0.4/(len/30+1)+0.2` of the paste's
  length) becomes the **last good edit**. It ends (`End`, named in the
  debug log: `time_up`, `next_dictation` (`pipeline::dictation_state`),
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
  function words, terms (or triggers) already in the dictionary, and edits
  rewriting most of the dictation. One per spelling (case-blind), terms
  with a capital first, ≤ 3. **The prompt** (`offer`): "Add
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
  `word_suggestion_respond` like any other. Answers in
  `<data>/word_suggestions.json` (not config.json: Settings' wholesale
  saves). Debug-only `e2e_learn_correction {pasted, edited?, before?,
  reads?, fresh?}` stages an edit for the e2e suite (skips the 3-min gap):
  the edited span, or `reads` — the field's whole text read by read (`null`
  = a failed read) — which the real `Watch` runs over (`staged_watch`).

## The Dictionary view

- **`DictionaryView.svelte`** =
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
  shows when either is on.
