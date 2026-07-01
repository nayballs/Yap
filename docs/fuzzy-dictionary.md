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
