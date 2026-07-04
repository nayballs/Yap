# Yap — manual test plan (edit/rewrite mode + smart routing)

Covers the features shipped this session. Work top to bottom — later sections
assume the setup in **0. Prerequisites** is done. Tick the boxes and jot the
result; anything that fails, note the exact behaviour so it's easy to fix.

> ⚠️ **You must be on the real engine build**, not the stub. If dictation types
> `[STT stub: received 1.6s of audio, engine=whisper]`, you're on the stub —
> relaunch with `scripts/dev.bat` (it adds `--features engines`; needs the Vulkan
> SDK). Edit/rewrite mode can't be tested on the stub.

---

## 0. Prerequisites

- [ ] Yap is running from `scripts/dev.bat` (real engine — Vulkan + DirectML).
- [ ] **Dictation sanity check** — focus a text box, press the dictation hotkey
      (default **F9**), say *"this is a test"*, press again. Real words appear
      (not the stub string). If this fails, stop — nothing else will work.
- [ ] **AI cleanup is configured** — Settings → AI Cleanup → provider + API key
      set, "Enable AI cleanup" on. Hit **Test**; it should return cleaned text.
      *(Edit/rewrite mode reuses these provider settings — no cleanup endpoint,
      no rewrite.)*
- [ ] **Edit hotkey bound** — Settings → General → **Edit / rewrite hotkey** →
      click it, press a key (suggest **F8**). It should show e.g. `F8`, not `None`.

---

## 1. Edit / rewrite mode — the main event

For each: **select the sample text**, **press-and-release (or hold) the edit
hotkey**, **speak the instruction**, **stop**. The selected text should be
**replaced in place** with the rewritten version.

Sample text lives right here — select it in any editor (Notepad, VS Code, a
browser text box) and rewrite it.

### 1a. "Make this a bullet list"
Select:

> • eggs
• milk
• bread
• coffee
• and
• butter

- [x] Say: **"make this a bulleted list"** → expect each item on its own bullet.

### 1b. "More concise"
Select:

> Let's reschedule for next week if that works for everyone.

- [ ] Say: **"make this more concise"** → expect a shorter, tighter sentence.

### 1c. "Fix the grammar"
Select:

> My brother and I were going to the store, but we didn't have enough time.

- [ ] Say: **"fix the grammar"** → expect correct grammar, meaning preserved.

### 1d. "Make it formal"
Select:

> Could you please send me that file as soon as you have a moment? Thank you.

- [ ] Say: **"rewrite this to be more formal"** → expect a polite/professional version.

### 1e. "Translate" (meaning-changing edit)
Select:

> Bon matin, j'espère que vous avez une journée merveilleuse.

- [ ] Say: **"translate this to French"** → expect French output.

**Pass criteria for 1a–1e:**
- [ ] The selection is **replaced** (not appended after it).
- [ ] Only the rewritten text lands — **no** preamble like "Sure, here's…".
- [ ] Your instruction ("make this a list") is **not** typed into the document.

---

## 2. Write mode (no selection)

With **nothing selected**, the spoken words generate new text at the cursor.

- [Thank you for taking the time to speak with me today, I appreciated learning more about the position and your team's work. ] Click into an empty text box (ensure no selection), press the edit hotkey,
      say **"write a one sentence thank-you note for a job interview"** → a new
      sentence is typed.
- [ ] Confirm it wrote *new* text rather than doing nothing.

---

## 3. Selection capture across app types (optional coverage)

Section 1 already proved edit mode works. This section is just *coverage* — it
checks the selection is read correctly in different kinds of app (which use
different capture tiers under the hood: UI Automation, or the Ctrl+C fallback).

**How:** in each app below, type a few words, select them, and run the **same
1a test** ("make this a bulleted list"). You only need to note the ones that
*fail* — where it rewrites the wrong text, or nothing happens.

- [ ] **Notepad** (simple, most likely to work) — this is the baseline.
- [ ] **VS Code** (optional) — different engine, uses the clipboard fallback.
- [ ] **A browser text box** (optional) — e.g. this box, Gmail, any `<textarea>`.

> Tip: skip an app if it's fiddly — the only thing that matters is telling me
> *which* app didn't work, if any. Don't grind through all of them.

**Clipboard hygiene (optional, one check):**
- [ ] Copy the word `KEEPSAKE` to your clipboard, run one edit-mode rewrite in
      VS Code, then paste — you should still get `KEEPSAKE` back (Yap restores
      your clipboard after the internal copy).

---

## 4. Smart routing (per-app cleanup rules)

Settings → AI Cleanup → **Smart routing**.

- [ ] **Add a rule** — type an app (e.g. `notepad.exe`) → **+ Add rule**. Give it
      a distinctive instruction, e.g. *"Rewrite everything in ALL CAPS."*
- [ ] **It applies in that app** — focus Notepad, dictate a normal sentence with
      AI cleanup on → output should be ALL CAPS (the rule won).
- [ ] **It does NOT apply elsewhere** — dictate the same sentence into a different
      app → normal cleanup (not all caps).
- [ ] **Recent-apps picker** — after dictating into a few apps, reopen the "Add
      rule" dropdown; it should suggest apps from your history.
- [ ] **Scope toggle** — turn on **"Only clean up in apps with a rule"**. Now
      dictate into an app *without* a rule → text should be injected **raw**
      (no cleanup). Dictate into your ruled app → still cleaned by its rule.

---

## 5. History icon (quick visual check)

- [ ] Settings → sidebar: the **History** item has a **clock icon** and lines up
      with the other items (General/Models/AI Cleanup/Advanced/About).

---

## 6. Edge cases / failure modes

- [ ] **Edit hotkey with no AI cleanup** — temporarily disable AI cleanup (or
      blank the provider), press the edit hotkey, speak → expect a friendly error
      ("Set up AI cleanup to use edit mode"), **not** a crash or raw instruction
      typed in.
- [ ] **Empty instruction** — press edit hotkey, stay silent, stop → nothing
      pasted, no error spam.
- [ ] **Focus moved mid-rewrite** — start an edit-mode recording, click into a
      different window while it's "processing" → result should still go to the
      **original** target window (it's captured at record-start).
- [ ] **Edit then plain dictation** — do a rewrite, then a normal F9 dictation →
      the normal one should be plain dictation again (not stuck in edit mode).

---

## Results / notes

| Section | Pass? | Notes |
|---|---|---|
| 0. Prereqs | | |
| 1. Edit/rewrite | | |
| 2. Write mode | | |
| 3. App types | | |
| 4. Smart routing | | |
| 5. History icon | | |
| 6. Edge cases | | |

Anything in the **Notes** column that isn't a clean pass — paste it back to me
and I'll fix it.
