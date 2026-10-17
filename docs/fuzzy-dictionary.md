# Dictionary & "fuzzy" correction — how FluidVoice does it, and Yap's options

Research + design notes for improving Yap's custom-word correction. TL;DR: the
name "fuzzy dictionary" is a bit of a trap — **FluidVoice does not do fuzzy string
matching at all.** Understanding what it *actually* does reframes what Yap should
build.

---

## What FluidVoice actually does

FluidVoice's "Custom Dictionary" settings page is **two separate features**:

| Feature | Mechanism | Fuzzy? |
|---|---|---|
| **Instant Replacement** | Post-transcription regex find→replace | **No** — exact, `\b`-word-boundary, case-insensitive |
| **Custom Words (Vocabulary Boosting)** | Biases the Parakeet **ASR decoder** at decode time (CTC rescoring) | **Acoustic only** — not string matching |

Key facts (verified against the FluidVoice source):

- **No Levenshtein / edit-distance / Soundex / Metaphone / Jaro anywhere.** Zero hits
  in application code. The only string comparison on the transcript is an exact,
  space-delimited **substring check used purely for analytics** (counting which
  boosted terms appeared), never for correction.
- **Instant Replacement** = `NSRegularExpression`, pattern `\b<escaped trigger>\b`,
  `.caseInsensitive`, replacement inserted verbatim (no capitalization preservation).
  Many `triggers[] → one replacement`. Applied right after transcription. **This is
  essentially what Yap already has** (minus the word boundaries — see below).
- **Custom Words** = the "fuzzy" part, but it's **acoustic, not textual**. Correct
  spellings (with a Mild/Balanced/Strong weight = 5/10/13) are tokenized through a CTC
  tokenizer and fed to the ASR beam search, biasing decoding toward those spellings.
  Gated by acoustic-confidence knobs (`minCtcScore`, `minSimilarity 0.72`,
  `minCombinedConfidence`, `minTermLength 3`). This lives inside **FluidAudio**, a
  closed-source **ARM64 / CoreML** library — **not portable to Windows.**
- Nice detail: an Instant-Replacement rule is *also* auto-registered as a boost term
  (replacement→`text`, triggers→`aliases`), so one rule both post-replaces *and*
  biases the model.

**So FluidVoice fixes mis-hearings at the source (the speech model) rather than
fuzzy-matching text afterwards.** It sidesteps the whole "fuzzy string" problem.

---

## What Yap has today

`config::apply_dictionary` (`src-tauri/src/config.rs`): case-insensitive regex
replace, `{from → to}` pairs, applied after transcription (and after AI cleanup):

```rust
let pattern = format!("(?i){}", regex::escape(from));   // NO \b word boundary
re.replace_all(&out, |_| to.clone())
```

Differences vs FluidVoice's exact-replace:
- **No word boundaries** — Yap would replace `cat` inside `category`. FluidVoice wraps
  triggers in `\b…\b`. Easy, safe improvement.
- One `from` per entry (FluidVoice allows multiple triggers → one replacement).
- Otherwise equivalent (case-insensitive, verbatim replacement, sequential order).

---

## Options for Yap (ranked)

Since we **can't** replicate FluidVoice's acoustic decoder boosting (it's their
proprietary Apple-Silicon ASR), "getting near-miss robustness" means picking a
*different* strategy. Three, roughly cheapest→deepest:

### 1. Quick win — tighten the exact path (do this regardless)
- Add `\b…\b` word boundaries so corrections don't fire mid-word.
- Allow multiple triggers per entry (`from` → `triggers[]`), matching FluidVoice's
  many-to-one model.
- Low risk, small, improves the feature we already ship.

### 2. Recommended differentiator — vocabulary as **AI-cleanup context**
Yap has something FluidVoice's dictionary path does **not**: an LLM cleanup stage.
We can hand the user's custom terms to that model as a bias list, e.g. append to the
cleanup system prompt:

> Known correct spellings — if the transcript contains an obvious mis-hearing of one
> of these, fix it: **Parakeet, Kubernetes, Grafana, nginx, …**

- The LLM does the "fuzzy" work (it already knows "power to keep" ≈ "Parakeet"),
  with real context — no brittle edit-distance thresholds.
- Reuses infrastructure we already have; near-zero new code.
- Only active when AI cleanup is on (fine — it's the "smart" path).
- This is arguably **better than FluidVoice**, which never feeds vocabulary to its LLM.

### 3. Optional — real fuzzy string matching (goes *beyond* FluidVoice)
Transcript-level Levenshtein / phonetic (e.g. Double Metaphone) match of each token
against the dictionary `from` terms, correcting near-misses even with cleanup off.
- This is genuinely **not** copying FluidVoice — it's new ground. Legitimate, but the
  danger is over-triggering.
- **Guardrails (borrowed from FluidVoice's boost tuning):** minimum term length
  (≥ ~4 chars), a tight edit-distance threshold (e.g. ≤1 for ≤6 chars, ≤2 longer),
  word-boundary tokens only, and never fuzzy-match very common words. Ship it as an
  **opt-in per-entry "fuzzy" flag**, not global, to bound the blast radius.

### 4. Deepest — ASR-level biasing (closest to FluidVoice, engine-dependent)
If `transcribe-rs` exposes it: Whisper's `initial_prompt` (seed the correct spellings)
or an ONNX/Parakeet hotword/keyword-boost hook. This is the true analog of FluidVoice's
approach — fix it at the source — but depends on what our STT crate supports and is the
most work to wire per-engine.

---

## Recommendation

- **Now:** (1) word-boundaries + multi-trigger — cleanup the exact path.
- **Then:** (2) feed dictionary terms into the AI-cleanup prompt — biggest quality win
  per line of code, and it out-does FluidVoice's own dictionary.
- **Later / opt-in:** (3) per-entry fuzzy flag with strict guards, for users who want
  correction without the LLM.
- (4) ASR biasing only if `transcribe-rs` gives us a clean hook — investigate before
  committing.

> Naming note for the roadmap: what we'd build is **not** "copy FluidVoice's fuzzy
> dictionary" (it has none). It's "exact-path polish + LLM-context correction, with
> optional true fuzzy matching" — which lands us at parity on the exact path and
> *ahead* on smart correction.

---

## 2026-07-10 update — the rest of the field, checked from source, and SHIPPED

Follow-up source audit of the other top apps (OpenWhispr clone, Wispr Flow's
extracted `app.asar`, Handy clone):

| App | Dictionary model | Fuzzy text matching? | How corrections actually happen |
|---|---|---|---|
| **Handy** | word list + threshold setting | **YES** — `audio_toolkit/text.rs` `apply_custom_words`: 1–3 word n-grams, normalized Levenshtein + Soundex boost (×0.3), 25% length gate, threshold 0.18. ONNX models only | Whisper models get the words as `initial_prompt` instead (decoder bias) |
| **OpenWhispr** | word list (manual + **auto-learned**) | no post-hoc fuzzy | Whisper `initial_prompt` / Mistral `contextBias` (ASR bias) + LLM prompt suffix; `correctionLearner.js` diffs the user's edits vs the pasted transcript (word-LCS + Levenshtein ≤0.65) to auto-add words; `dictionaryEchoFilter.js` guards prompt echo on silence |
| **Wispr Flow** | word list, auto-learning (`DictionaryItemAutoAdded`, undo toast) | no client-side fuzzy (only `diff_levenshtein` inside the bundled diff-match-patch lib) | words go to their **cloud** ASR/LLM; all biasing is server-side |
| **FluidVoice** | (above) | no | acoustic CTC boosting, Apple-Silicon-only |

**Shipped in Yap (2026-07-10):** options 1+2 were already in; this pass added
**3 and 4** — the full Handy split, ported from source:

- `src-tauri/src/fuzzy.rs` — Handy's `apply_custom_words` (Levenshtein +
  `natural`-crate Soundex replicated exactly — it's a NONSTANDARD variant: raw
  first char, strip h/w → dedup → strip vowels — verified against the real
  crates in a scratch build). Adapted to Yap's `{from → to}` entries (n-grams
  match both spellings, always replace with `to`; ≥3-char terms only). One
  deliberate fix over Handy: n-grams are chosen shortest-first and a longer
  n-gram must **strictly reduce absolute edit distance**, because Handy's
  longest-first greedy provably swallows an unrelated adjacent word
  ("Charge B, che" → "ChargeBee" eating "che"; their own unit test can't pass
  as committed). Its mirror image (2026-10-17, found by the real-app robot):
  a run is never taken when its first word adds nothing — when the later
  words make the same term at most as many edits away by themselves — so
  "to Kubernetes" (2/12 from "kubernetes") no longer eats the "to", nor
  "1. kubernetes" its number, nor "- kubernetes" its space. Gated by `config.dictionary_fuzzy` (default ON, toggle in the
  Dictionary view), ONNX models only. **Per-entry opt-out** (the ≈ button on a
  Dictionary row, `entry.fuzzy`, default on): exempts one correction from the
  near-miss pass for entries whose near-misses are real words — live dogfooding
  found `json → JSON` eating the name "Jason" (Soundex-identical, so no
  threshold can separate them). Exact replacement + the Whisper prompt bias
  still apply to exempted entries.
- **Whisper `initial_prompt`** — `config::dictionary_prompt` (distinct `to`
  spellings, ", "-joined, ~600-char cap at a comma) threaded through
  `SttEngine::transcribe` at every call site (dictation, partials, upload,
  meeting). Echo guard: `fuzzy::is_prompt_echo` (OpenWhispr's
  `matchesDictionaryPrompt` port) drops a transcript that is just the prompt
  hallucinated back — whisper-only, and only when the dictionary has ≥3 words
  so a real one-word dictation of a dictionary term can't be eaten.

**2026-10-05:** the exact + fuzzy passes are one function,
`pipeline::apply_corrections(text, &cfg)`, and run on meeting segments (both
sides, before they're saved) and on an Upload's whole text as well as on a
dictation. Before, meetings and Upload got the dictionary only as Whisper's
`initial_prompt`, so with the default Parakeet model they got no corrections.

Still open from the original list: multi-trigger entries (`triggers[] → to`).
(OpenWhispr-style auto-learning shipped 2026-10-17 as word *suggestions*, see
the last section.)
(The per-entry fuzzy flag from option 3's guardrails shipped 2026-07-10,
see above.)

---

## 2026-10-17 — snippets, "press enter", and a ranked vocabulary

Three behaviours from the Wispr Flow teardown
(`references/wispr-flow/rea-teardown/04-dictation-and-other.md`, behaviour and
numbers only — Yap's own code and words):

**Snippets** (voice text expansion). A dictionary entry with `snippet: true`:
`from` is the phrase you say, `to` the text Yap types (it may span lines).
`config::apply_snippets` runs on a **dictation's** final text only:

| Step | What runs |
|---|---|
| 1 | AI cleanup (told to keep snippet phrases word for word; never sees their text) |
| 2 | corrections: exact, then fuzzy (`pipeline::apply_corrections` — snippets skipped) |
| 3 | the "press enter" voice command (below) |
| 4 | snippets |

Matching: the phrase's words (runs of letters/digits, lowercased) must appear
in order as whole words, with any non-letter/digit run between them, any case
— so cleanup's "My sign-off," still matches `my sign off`. One regex, one
pass, the longest phrase first (most words, then most letters), so "my email
address" beats "my email" and an inserted text is never expanded again. If
nothing but punctuation and spaces is left around a single match (the phrase
was all you said), the result is exactly the snippet's text, without the
capital or full stop cleanup added; inside a sentence only the phrase is
replaced. A full stop cleanup put straight after the phrase is dropped where
it would spoil the snippet's own text: after a text that ends in punctuation
("Many thanks," never becomes "Many thanks,."), and as the dictation's last
character after a text of several lines, so a sign-off or an address ends as
saved ("…see you soon. Speak soon,⏎Nathan", not "Nathan."). A one-line
snippet mid-sentence keeps the sentence's punctuation ("Mail it to
sam@example.com."), and an ellipsis, "?" or "!" always stays. The text goes
in verbatim (`$1` is text). Snippets are never in the
exact or fuzzy correction passes or the Whisper prompt, so meetings and Upload
(which run `apply_corrections` only) keep what was said. Corrections run first,
so the snippet's text is never fuzzed; the cost is that a correction could, in
theory, change a phrase's word before step 4.

**"Press enter"** (`voice_enter`, default on; `pipeline::take_voice_enter`).
Only the LAST two words count ("press enter" / "press return", any case or
punctuation): the phrase goes, with a comma/semicolon/dash before it, and a
full stop or question mark after it moves onto the text ("Can you check,
press enter?" → "Can you check?"). Checked in the final text first, then the
raw transcript (cleanup may drop or reword it; the final text is then pasted
as it is). The submit key is `auto_submit_key`, pressed once even with
auto-submit on. A dictation of just "press enter" pastes nothing and presses
only while the dictation's window still has the focus. Before snippets, so
"my address, press enter" still gives exactly the address. Test mode presses
nothing (`text_injector::press_submit`).

**Ranked Whisper vocabulary** (`dict_usage.rs`). The prompt is capped at ~600
chars, so with a long dictionary the order decides which words Whisper hears
about. `<data>/dictionary_usage.json` keeps `{uses, lastUsed}` per word
(lowercased `to`), outside config.json (Settings auto-saves its whole config
copy and would put old counts back). A word counts as used whenever it's in
a corrected dictation, meeting line or Upload — which covers its correction
firing, and also a Whisper decode that spelled it right thanks to the prompt
(with Whisper, the fuzzy pass is off and a well-biased decode never fires the
exact rule, so counting only firings would never learn anything). Order: used
within the last day, then the last 14 days, then the rest; inside each tier
most uses first; then the order you added them. Wispr also ranks by an entry's
*source* (manual > learned from edits > …); Yap has only manual entries so far.
Meetings keep their attendees ahead of the ranked dictionary.

---

## 2026-10-17 — words suggested from your corrections

"Suggest words from your corrections" (`learn.rs`, `config.learn_words`,
default on, a switch in the Dictionary view): Yap pastes a dictation, you fix
a word it got wrong in place, and it asks — "Add “Kubernetes” to your
dictionary? Yap heard “cube and eighties”." Behaviour after Wispr Flow's
dictionary auto-learn and OpenWhispr's `correctionLearner.js`. The same day
it was aligned with Wispr's real mechanics, read from its shipped code and
its native helper (teardown `05-dictionary-learning.md`; behaviour and
numbers only, none of its code or wording).

### What Wispr Flow measurably does

- **The watch.** Every paste is watched for **60 s** (one-shot). The helper
  reads the **whole field** over UI Automation (TextPattern, else
  ValuePattern) on a UIA TextChanged event for that element plus a 1 s poll,
  reads ≥ 400 ms apart; password and non-editable fields are never watched.
  **No end on a focus or app change.** It ends on: the window elapsing, the
  next paste, the field emptied, the transcript's region emptied or removed,
  a **trailing line break added** (Enter: a message sent), an anchor
  mismatch, the field unreadable, or the field > 15,000 characters.
- **The span.** The first occurrence of the pasted text splits the field
  into *before* and *after*; every read must still start with *before* and
  end with *after*, and the middle is the edited text.
- **The drift gate.** Character Levenshtein (lowercased) between pasted and
  edited text, over the pasted length, must be ≤ `0.4/(len/30+1)+0.2`
  (≈ 0.6 for a few words, 0.4 at 30 characters, → 0.2 for long text). Each
  read within it is saved; **learning uses the last read within the gate**,
  whatever ended the watch.
- **The candidates.** Token alignment (substitution only when < 50 %
  different); a run of changes → a phrase, dropped when either side is empty
  (pure insertion/deletion) or the **edited side is > 4 words**; case-only
  changes never; tokens ending in `'s 't 'll 've 're 'd 'm` or a bare `'`
  skipped ("O'Brien" fine); deduped case-insensitively with capitalised
  forms first; at most 4.
- **The decision.** A **cloud classifier** accepts or rejects each candidate
  (offline → nothing); accepted words are **added silently** with a 6 s Undo.
  A word you removed (or undid) is in the duplicate check as a soft-deleted
  row, so it's **never learned again**.

### What Yap does

| | Yap (`learn.rs`) | vs Wispr |
|---|---|---|
| Watch | 60 s; the whole field (≤ 15,000 chars) read every 700 ms (≥ 400 ms apart) over UIA, focused or not; never a password or read-only field, never a terminal, never after a submit key, never in a test run | same window and reads; polling only (no TextChanged handler — a poll is enough at this cadence) |
| Ends (`End`, named in the debug log) | `time_up`, `next_dictation`, `field_emptied`, `region_emptied`, `region_gone`, `anchor_mismatch`, `enter_pressed`, `unreadable` (3 failed reads in a row), `too_long` | same set; Yap tolerates two failed reads (a slow app) |
| Finding the span | the occurrence at the caret right after the paste; ~24 chars either side remembered (8-char fallback when those were edited) | **differs on purpose**: Wispr takes the first occurrence and pins the whole prefix/suffix, so typing anywhere else in the field ends its watch; Yap's nearest-neighbour anchors keep watching |
| Drift gate | the same formula, char Levenshtein lowercased; the last read within it is the edit judged, whatever the end ("fix a word, press Enter" — even in one read — still counts) | same |
| Candidates | LCS word alignment (case- and edge-punctuation-blind); 1–4 words → 1–4 words; insertions/deletions, punctuation-only and case-only changes of ordinary words skipped; contractions/possessives skipped; one per spelling, capitals first, **≤ 3** | Yap also caps what it heard at 4 words, and keeps its rule that a case fix making a term of a non-everyday word counts ("github" → "GitHub") |
| Decision | **local**: the new text must look like a term (a capital inside, an acronym, digits, not an everyday word — the built-in list, then Windows' spell checker — a capital mid-sentence, or several words heard as one) and spell or sound like what Yap typed (`fuzzy::spelling_distance` / `sound_distance`); then it **asks** | **differs on purpose**: no cloud classifier, nothing added without a click |
| Removed words | `save_config` diffs the dictionary; a correction whose trigger and spelling are both gone goes into `word_suggestions.json`'s `removed`, and that term is never offered again, however it's heard | same effect (Wispr: soft-deleted rows) |

**What counts** (`learn::suggestions`, pure, unit-tested):

| Step | Rule |
|---|---|
| Align | pasted vs edited words, longest common subsequence, ignoring case and the punctuation at word edges; the gaps between matches are the changes |
| Size | 1–4 words replaced by 1–4 words (spelled-out letters, "G P T", count as one); insertions and deletions never count |
| What Yap heard | not a single everyday word — `motion → Notion` would rewrite every "motion" you say (the `json → JSON`/"Jason" problem again) — and not only function words ("and then") |
| The new text | looks like a term: a capital inside a word ("GitHub"), an acronym ("NASA"), digits ("GPT-4"), a word that isn't an everyday one, a capital mid-sentence, or several words heard as one word that isn't an everyday one ("power to keep" → "Parakeet"); never a word ending in a contraction or possessive ("Sinead's", "it'll", "students'") |
| Alike | spells or sounds like what Yap typed: `fuzzy::spelling_distance` (Handy's score on the letters alone) or `fuzzy::sound_distance` (Levenshtein over a consonant `sound_key`: c/k/q/g, b/p, d/t, f/v/ph, s/z merged; gh, vowels, h/w/y dropped — "kpnts"/"kprnts") ≤ 0.5; ≤ 0.3 when the keys are 3 or shorter, so "Sam"/"Pam" or "Sarah"/"Priya" aren't mishearings |
| Case alone | "github" → "GitHub", "nasa" → "NASA", "sinead" → "Sinead" mid-sentence — never for an everyday word ("slack" → "Slack" would capitalise every "slack") |
| Skipped | words already a correction's spelling or trigger, words you removed from the dictionary, punctuation-only edits, and edits that rewrite most of the dictation |
| Cap | one per spelling (case-blind), capitalised terms first, 3 per dictation |

"Everyday word" = the built-in list of frequent English words (with days,
months, numbers, contractions; plain suffixes stripped: "meetings", "stopped",
"happily") **or** Windows' own spell checker (`ISpellChecker`, the user's
locale plus English, read-only) accepting the word in lower case. Names fail
that ("jason", "sinead" are only right with a capital), so they stay
learnable.

**Asking.** When the watch ends with an edit worth learning: a toast in the
main window while it's on screen, else a Yap bar card (never a Windows
notification): Add / Not now / "Don't suggest this". Add saves `{from:
heard, to: term, fuzzy: true}` — what Yap typed after cleanup, which is what
the dictionary runs on — to config.json and the live pipeline, and tells the
windows (`yap-dictionary-learned`) so Settings' and the Dictionary view's own
saves keep it. "Not now" twice (✕ counts) or "Don't suggest this" and that
pair is never offered again; a word you remove from the dictionary is never
offered again at all; at most one prompt every 3 minutes. The answers live in
`word_suggestions.json`, never the text read. Since the edit is judged when
the watch ends, the question comes when you send the message, press Enter,
start the next dictation or the minute is up — not while you're still
fixing.

Not (yet): learning from edits made after the 60 s or in fields UI Automation
can't read (some Electron apps, terminals), ending the watch on a "Paste last
transcript" (only a new dictation ends it), and the per-source vocabulary
ranking Wispr does (a learned entry ranks like a manual one).
