# Wispr Flow features: Yap's checklist

**How to read this:** every Wispr Flow feature we know of, grouped by area, with Yap's
status. Tick a box when Yap has it. Each line says what the feature does in Wispr (in our own
words), then where it lives in Yap or would land, then the effort and the source note.

**Legend:** ✅ in Yap · 🟡 partly · ⬜ not yet · 🚫 won't do (the line says why: cloud-only,
paid-only, against Yap's privacy promise, or off-mission).

**Effort** (for items not done): S small · M medium · L large.

**Sources** (the notes are in `E:\Projects\references\wispr-flow\`, outside this repo):
- `01`–`09`: the REA teardown notes in `rea-teardown\` (Wispr 1.6.1144 and 1.6.1174). A later
  note wins where they disagree.
- `HO`: the hands-on `README.md` and its `screenshots\` (1.6.1034).
- `FB` / `NP`: `flowbar-spec.md` / `notepad-spec.md`.
- `UI`: Wispr's own UI strings and helper messages in `copies\split\` (1.6.1174).
- `SS`: Nathan's screenshots of the current hub (2026-10-17).
- `T`: Yap's public-docs teardown, [`wispr-notetaker-teardown.md`](./wispr-notetaker-teardown.md).

Last checked against Yap's code: 2026-10-17. macOS-only features (dock icon, Secure Keyboard
Entry, permission panels) are left out.

## Summary

**290 features:** 99 ✅ · 46 🟡 · 108 ⬜ · 37 🚫.

| Area | ✅ | 🟡 | ⬜ | 🚫 |
|---|---|---|---|---|
| Dictation and hotkeys | 6 | 9 | 10 | 3 |
| Inserting text | 8 | 1 | 14 | 2 |
| Dictionary, snippets and learning | 6 | 1 | 7 | 2 |
| Styles, tones and Transforms | 4 | 6 | 4 | 3 |
| Scratchpad and notes | 4 | 1 | 8 | 3 |
| Notetaker: detection and recording | 15 | 4 | 11 | 4 |
| Notetaker: transcript and speakers | 4 | 1 | 7 | 2 |
| Notetaker: AI summaries and Ask | 12 | 8 | 7 | 2 |
| Meeting notepad and Flow Bar | 11 | 4 | 3 | 0 |
| Calendar | 8 | 2 | 3 | 0 |
| Home, history and Insights | 4 | 2 | 9 | 3 |
| Settings and system | 6 | 2 | 12 | 1 |
| Integrations and MCP | 4 | 0 | 1 | 4 |
| Accounts, team, billing and referrals | 2 | 1 | 0 | 5 |
| Privacy and data | 3 | 2 | 2 | 2 |
| Onboarding and help | 2 | 2 | 10 | 1 |
| **Total (290)** | **99** | **46** | **108** | **37** |

## Next up

The best not-yet and partial items for a Windows dictation and meetings user: high value,
small effort and fully local first. Round 2's ranking (`00-yap-plan.md`) is the input,
re-checked against what Yap has today.

1. **Spot pastes into admin windows.** Today a paste into an elevated app fails silently. Check
   the target before sending keys, then say Yap can't type into apps running as administrator
   and leave the text copied. `text_injector.rs` (S · 07)
2. **Fix how Yap reads text boxes.** Treat a box whose text equals its own label as empty, look
   up the parent elements for a text pattern, and check the legacy read-only flag. This closes
   the Edge/Chrome robot-test gap (a web chat message keeps its full stop and gets a trailing
   space). `selection.rs` (S · 07)
3. **End quiet meetings and check in.** After 10 minutes with no speech, show a 30-second
   "Ending notes…" countdown that any speech cancels. Check in 10 minutes after the calendar
   end and at 1 hour, unless someone spoke in the last 3 minutes. `meeting_guard.rs` + bar
   card (S · 06)
4. **Don't hold a Bluetooth mic open between dictations.** The idle pre-roll keeps a headset in
   its call-quality mode while Yap runs. Keep the pre-roll for wired and USB mics only, and show
   a one-time tip to use a wired mic. `pipeline.rs` `open_stream` (S–M · 08)
5. **Fix "Mute while recording" and add "Pause media".** Play the start chime before muting,
   and never silence a live call. Add an option that pauses a playing media app and resumes
   only what Yap paused. `mute.rs`, `sound.rs` (S–M · 07, 08)
6. **Undo a cancelled dictation, and add ✕ / ✓ on the bar.** Keep the cancelled audio for a few
   seconds behind "Dictation cancelled · Undo", and let the dictation capsule cancel or finish
   with a click. `pipeline.rs`, `bar/DictationCapsule.svelte` (S · 08, UI)
7. **Check Windows' microphone privacy switch.** When desktop apps are blocked, say so with an
   Open settings button instead of "No sound from…". `mic_watch.rs` (S · 07)
8. **Show the raw words.** History already keeps the raw transcript: add "Undo AI edit" (paste
   or copy the raw text) and a "See what changed" diff. `history.rs`, `HomeView.svelte`
   (S · 08)
9. **A paste that knows it worked.** Put the text on the clipboard without its data and supply
   it when the target app asks; restore the clipboard after the app reads it. If no app reads
   it within a second, it failed: keep the text copied and show "Couldn't paste, press Ctrl+V".
   This is Yap's missing verify-after-paste. `text_injector.rs` `clip` (M · 07)
10. **Editable transcript lines that teach the dictionary.** Fixing a meeting line keeps the
    original, shows an "Edited" badge, and sends the before/after through `learn.rs` like a
    post-paste edit. `notes.rs`, `Notepad.svelte`, `learn.rs` (S–M · 09)
11. **Rewrite the last dictation with nothing selected.** If the last dictation is under a
    minute old, select it (or undo it) and paste the rewrite. Saved rewrite prompts then get
    their own hotkeys. Edit path in `pipeline.rs`, `selection.rs` (M · 08)
12. **Mic management.** Keep a ranked list of mics (the first one present wins). Ask "X detected ·
    Switch?" after a recording, and say "Using X" after an automatic switch. If the mic is
    unplugged mid-dictation, stop and offer to insert what was heard. `pipeline.rs`, `config.rs`,
    Settings (M · 08)

---

## Dictation and hotkeys

- [x] ✅ **Push-to-talk**: hold the key, talk, let go to paste. Yap: `gesture.rs`, `recording_mode`. _(04)_
- [x] ✅ **Hands-free toggle**: press to start and press again to stop. Yap: toggle mode (the default). _(UI)_
- [x] ✅ **Double-tap locks hands-free**: a quick tap and a second press keep it recording until the next press. Yap: `gesture.rs`, the lock on the overlay. _(04)_
- [x] ✅ **Tap and long-hold coaching**: a lone tap gets "hold it, don't tap", and a minute-long hold gets a hands-free hint. Yap: `coaching.rs` (Tip toast or bar card, ≤ 3 each). _(04, UI)_
- [x] ✅ **Rebindable shortcut**: single keys, combos, modifier-only chords and extra mouse buttons. Yap: `input_hook.rs` specs, `HotkeyInput.svelte`. _(08)_
- [x] ✅ **Start/stop sounds**: a chime when the mic opens and closes. Yap: `sound.rs`, with volume and output device (Wispr has no volume). _(08)_
- [ ] 🟡 **Paste last transcript**: pastes your last dictation again, from a hotkey that fires when all its keys are up. Yap: tray item + `paste_last_transcript`; missing: a hotkey. _(S · 08)_
- [ ] 🟡 **Cancel or finish from the bar**: while you dictate, the bar shows ✕ (cancel) and a "finish and paste" button. Yap: Cancel in the tray only; the capsule has no buttons. _(S · UI)_
- [ ] 🟡 **Dictation length cap with a warning**: warns at 19 minutes, stops at 20, and offers "Recover text". Yap: a 15-minute buffer cap that only writes a log line. _(S · 04, UI)_
- [ ] 🟡 **Mouse shortcut setup**: a guided setup for mouse buttons (with an MX Master guide). Yap: `mouse:ID` bindings work; missing: the guide. _(S · UI)_
- [ ] 🟡 **Stale-key checks**: drops keys that are stuck "down" and checks a chord before firing it. Yap: a 30 s re-hook and a GetAsyncKeyState self-heal; missing: the periodic stale-key sweep. _(S · 07, 08)_
- [ ] 🟡 **Hotkey hook in a helper process**: Wispr's hook runs in a separate process, so its own focused windows never block the hotkey. Yap: an in-process hook plus in-page fallbacks in every window; a spike is pending. _(M · 07, 08)_
- [ ] 🟡 **Extra sounds and sound packs**: sounds for the hands-free lock, a slow paste and notifications, in selectable packs. Yap: one start/stop pair. _(S · 08)_
- [ ] 🟡 **My languages**: pick every language you speak (instead of one, or auto-detect). Yap: one language or auto (`selected_language`). _(M · 06, 08)_
- [ ] 🟡 **Command Mode** (formerly "Hey Flow"): select text and ask a question about it, or ask the web. Yap: the Voice Agent wake word (`agent_detect.rs`) and the edit hotkey, run locally; web answers through Perplexity are cloud-only, so not those. _(08, UI)_
- [ ] ⬜ **Several shortcuts per action**: "Add another" binding for the same action. → `input_hook.rs`, config. _(S–M · 08)_
- [ ] ⬜ **Copy last transcript**: its own shortcut and menu item, which copies without pasting. → `text_injector.rs`, tray. _(S · 08, UI)_
- [ ] ⬜ **A held shortcut that grows into a chord is cancelled**: a push-to-talk under 1 s old is dropped when other keys join, and a "hold only X" tip shows at most once a minute. → `gesture.rs`, `input_hook.rs`. _(S · 08)_
- [ ] ⬜ **Dictation cancelled · Undo**: keeps a cancelled dictation's audio for a few seconds to bring it back. → `pipeline.rs`, toast and bar card. _(S · 08, UI)_
- [ ] ⬜ **"Taking longer than usual…"**: a notice after 4 s of processing. → overlay. _(S · 08, UI)_
- [ ] ⬜ **Long dictation → try meeting notes**: a 10-minute dictation suggests recording it as a meeting instead. → `coaching.rs`. _(S · 04, UI)_
- [ ] ⬜ **Triple-tap for meeting notes**: three quick taps of the dictation key start a meeting note. Yap chose the Win+Alt+M shortcut instead (T); this one is optional. → `gesture.rs`. _(S · UI, T)_
- [ ] ⬜ **Meter that moves for whispers**: an adaptive floor so quiet speech still animates the bars. → `pipeline.rs` `yap-amp`. _(S · 08)_
- [ ] ⬜ **Wrong-language nudge**: if you speak a language you didn't select, it suggests adding it (plus a one-time "we auto-detected your language"). → `stt.rs`, needs Whisper's language probabilities. _(M · 08, UI)_
- [ ] ⬜ **Regional variants**: Canadian or British spelling, and Hindi in Latin letters ("Hinglish"). → languages + cleanup prompt. _(M · 08, UI)_
- [ ] 🚫 **Instruct Mode**: dictate instructions that draft a reply from what's on screen, with tools. Won't do: it sends screenshots and screen text to a cloud model (privacy). _(08, UI)_
- [ ] 🚫 **Focus Mode**: a hotkey that blocks distracting apps and sites. Won't do: off-mission. _(09, UI)_
- [ ] 🚫 **Mic ring and Jabra wear detection**: a Bluetooth ring with PTT buttons, and skipping a Jabra headset nobody's wearing. Won't do: vendor-specific hardware SDKs (off-mission). _(08)_

## Inserting text

- [x] ✅ **Paste through the clipboard, then restore it**: brings your clipboard back after the paste. Yap: `text_injector.rs` (it also restores images and copied files). _(04)_
- [x] ✅ **Dictations kept out of clipboard history**: private clipboard formats, so a dictation never reaches Win+V history, cloud clipboard or clipboard managers. Yap: `text_injector.rs` `clip`. _(04)_
- [x] ✅ **Verified clipboard write**: checks the write landed and retries. Yap: sequence-number check, 4 tries, typed fallback. _(04)_
- [x] ✅ **Back to the right window**: refocuses the app you started in before pasting. Yap: `capture_target`, `focus_window`. _(07)_
- [x] ✅ **Fit to the text around the cursor**: lowercase mid-sentence, drop a full stop before following text, space only where needed. Yap: `smart_insert.rs`, `selection.rs`. _(04)_
- [x] ✅ **No full stop on short chat messages**: in chat apps, a one- or two-sentence message loses its final full stop. Yap: `app_kind.rs`. _(04)_
- [x] ✅ **Terminals and coding CLIs**: plain one-line text in a terminal; detects Claude Code and Codex from the window title. Yap: `app_kind.rs` (`terminal_mode`). _(04)_
- [x] ✅ **"Press enter" voice command**: ending with "press enter" sends the message. Yap: `pipeline::take_voice_enter` (`voice_enter`). _(04)_
- [ ] 🟡 **App categories**: AI, email, work chat, personal chat, documents, terminal; drives styles and paste rules. Yap: Terminal / Messaging / Other in `app_kind.rs`; missing: email, documents and AI buckets. _(S–M · 04)_
- [ ] ⬜ **A paste that knows it worked**: the clipboard is filled only when the target reads it; the read times the restore, and no read means the paste failed. → `text_injector.rs` `clip`. _(M · 07)_
- [ ] ⬜ **"Select a text box first" card**: when a paste lands nowhere, the text stays copied and a card with Copy says so. → bar card, with the item above. _(S · 08)_
- [ ] ⬜ **Admin windows**: detects an elevated target and says it can't type there, instead of failing silently. → `text_injector.rs`. _(S · 07, 08)_
- [ ] ⬜ **Release held Alt/Win before pasting**: so Ctrl+V isn't read as Alt+Ctrl+V and no menu or Start opens. → `text_injector.rs`. _(S · 07)_
- [ ] ⬜ **Text-box reading fixes**: a box whose text is its own label counts as empty; walks up to a parent for a text pattern; checks the legacy read-only flag. → `selection.rs`. _(S · 07)_
- [ ] ⬜ **Element-level refocus**: remembers the exact text box (not just the window), and re-reads it if you switch boxes mid-dictation. → `selection.rs`, `text_injector.rs`. _(M · 07)_
- [ ] ⬜ **Shift+Insert for terminals that ignore Ctrl+V**: mintty/Git Bash, Alacritty, WezTerm, PuTTY. → `text_injector.rs`, `app_kind.rs`. _(S · 04, 07)_
- [ ] ⬜ **Chunked paste for Claude Code and Codex**: long pastes go in slices so the CLI doesn't treat them as one blob. → `text_injector.rs`. _(S · 08)_
- [ ] ⬜ **Stacked messages**: splits a dictation into separate chat messages. → `app_kind.rs`, `text_injector.rs`. _(M · 04, UI)_
- [ ] ⬜ **Slack mentions**: "at Sam" becomes "@Sam" when Sam's name is on screen. → `selection.rs`. _(M · 04)_
- [ ] ⬜ **IDE variable recognition**: reads open file names and symbols in VS Code, Cursor and Windsurf (with their screen-reader mode) into the vocabulary. → `selection.rs`, `stt.rs`. _(M · 04, 07, 08)_
- [ ] ⬜ **File tagging in Cursor and Windsurf**: saying a file name types `@file` + Tab in the chat. → new IDE module. _(M · 08)_
- [ ] ⬜ **A bindable Enter key**: an extra key or mouse button that presses Enter to send. → `input_hook.rs`. _(S · 08, UI)_
- [ ] ⬜ **Recent links**: the bar's menu lists links you copied lately (search, pin, open, copy) to drop one in. → bar menu; it would mean watching the clipboard for links. _(M · UI)_
- [ ] 🚫 **On-screen text for proper nouns**: sends nearby screen text or screenshots to the server to spell names right. Won't do: privacy. _(04, 05)_
- [ ] 🚫 **Admin-disabled apps**: an organisation turns dictation off in chosen apps. Won't do: enterprise/cloud. _(UI)_

## Dictionary, snippets and learning

- [x] ✅ **Dictionary corrections**: a misheard word becomes its right spelling. Yap: `DictionaryEntry`, `DictionaryView.svelte`. _(04, UI)_
- [x] ✅ **Vocabulary hints for the recognizer**: dictionary words bias transcription. Yap: Whisper `initial_prompt` (`config::dictionary_prompt`); the fuzzy pass for ONNX models. _(04)_
- [x] ✅ **Ranked vocabulary**: the words used most lately go first. Yap: `dict_usage.rs`. _(04)_
- [x] ✅ **Snippets**: a spoken phrase turns into saved text. Yap: `config::apply_snippets`, Dictionary → Snippets. _(04)_
- [x] ✅ **Learn words from your corrections**: fix a word after a paste and it's offered for the dictionary. Yap: `learn.rs` asks first (Wispr adds it and offers Undo). _(05)_
- [x] ✅ **Removed words stay removed**: a word you deleted is never learned again. Yap: `learn.rs` `removed`. _(05)_
- [ ] 🟡 **Richer snippets**: rich-text expansions, a length counter, and a warning when a new snippet is close to an existing one. Yap: plain-text snippets with line breaks. _(S · 04, UI)_
- [ ] ⬜ **Starred words and sorting**: star a word to pin it first; sort A–Z, newest, oldest or starred. → `DictionaryView.svelte`, `dict_usage.rs`. _(S · 04, UI)_
- [ ] ⬜ **Search the dictionary**. → `DictionaryView.svelte`. _(S · UI)_
- [ ] ⬜ **Select several and delete**. → `DictionaryView.svelte`. _(S · UI)_
- [ ] ⬜ **Import words and snippets from a file**, with a preview first. → `DictionaryView.svelte`. _(S · UI)_
- [ ] ⬜ **Your own name in the dictionary**: your first name is added automatically. → `auth.rs` profile into config. _(S · 04)_
- [ ] ⬜ **Learn from meeting edits**: fixing a transcript line or the summary teaches the dictionary too. → `learn.rs`, after editable transcript lines. _(S · 09)_
- [ ] ⬜ **"Type that often? Save it as a snippet"**: a nudge to make a snippet (its trigger wasn't traced). → `coaching.rs`. _(S · UI)_
- [ ] 🚫 **Team and department dictionaries and snippets**, with admin approval. Won't do: needs a shared server and a paid team plan. _(UI)_
- [ ] 🚫 **Dictionary sync across devices**. Won't do: cloud-only. _(UI)_

## Styles, tones and Transforms

- [x] ✅ **AI cleanup of every dictation**: removes filler words and fixes punctuation and grammar. Yap: `llm.rs` (cloud key or the on-device model). _(08)_
- [x] ✅ **Self-corrections resolved**: "six, no seven" keeps only "seven". Yap: the cleanup prompt (`llm.rs`). _(UI)_
- [x] ✅ **Named styles for chosen apps**: a style with its own apps and cleanup. Yap: cleanup profiles with app routes and a per-profile model. _(08)_
- [x] ✅ **Rewrite selected text with a hotkey**: select text, press the key, say how to change it. Yap: edit mode (`selection.rs`, `EDIT_BASE_PROMPT`). _(08)_
- [ ] 🟡 **Cleanup level None / Light / Medium / High**. Yap: on/off and presets (Default, Email, Notes, Slack, Code, Custom); missing: a strength setting. _(S · 08, UI)_
- [ ] 🟡 **Styles per app category × tone**: work chat, personal chat, email and other, each formal, casual, very casual or excited, set from example cards. Yap: profiles and per-app routes set by hand; missing: built-in categories and tone cards. _(M · 04, 08)_
- [ ] 🟡 **Smart formatting**: spoken numbers become a list; a greeting lays the text out as an email. Yap: only through the Email/Notes presets; missing: list and email rules in the default prompt. _(S · UI)_
- [ ] 🟡 **Translate**: a transform that translates into any language. Yap: Whisper's translate-to-English only. _(S · UI)_
- [ ] 🟡 **Auto-apply a transform after dictation**: runs a chosen rewrite on everything you dictate, and auto-polishes long dictations. Yap: the cleanup body or profile does this on every dictation; missing: picking a saved transform for it. _(S · 08, UI)_
- [ ] 🟡 **Note transforms**: AI rewrites inside notes (Polish, Turn into a list, Translate, custom). Yap: the Actions engine on notes (`note_enhance`, ActionManager); missing: rewrite built-ins like Polish, list and translate. _(S · 03)_
- [ ] ⬜ **Rewrite the last dictation with nothing selected**: if the last dictation is under a minute old, it's selected (or undone with Ctrl+Z) and rewritten. → edit path in `pipeline.rs`, `selection.rs`. _(M · 08)_
- [ ] ⬜ **Saved transforms on their own hotkeys**: up to 9, including Polish and a "Prompt Engineer" that turns rambling into a structured AI prompt. → config, `input_hook.rs`, `llm.rs`. _(M · 08, UI)_
- [ ] ⬜ **See what changed**: after a big cleanup or a transform, a "cleaned up, see what changed" card opens a diff (Undo AI edit is under History). → `history.rs`, a diff view, bar card. _(S–M · 08, UI)_
- [ ] ⬜ **Writing samples**: paste examples of your own writing so rewrites sound like you. → `llm.rs` (few-shot), Settings. _(S–M · UI)_
- [ ] 🚫 **Style detection**: learns your formality from your past messages. Won't do: a cloud classifier. _(04, 08)_
- [ ] 🚫 **Email preferences from Gmail**: reads your sent emails to learn how you write. Won't do: cloud, and it reads your mailbox (privacy). _(UI)_
- [ ] 🚫 **Lens web answers**: Command Mode questions routed to Perplexity. Won't do: cloud-only. _(08)_

## Scratchpad and notes

- [x] ✅ **Notes with folders**: create, rename and delete folders; move notes. Yap: `NotesView.svelte`, `notes.rs`. _(03)_
- [x] ✅ **Dictate into notes**: the hotkey types into a note like any app. Yap: note fields + the Ask bar's mic. _(UI)_
- [x] ✅ **Copy or export a note as Markdown**. Yap: Copy markdown / Copy text, `note_export`. _(03)_
- [x] ✅ **Chat that creates and edits notes**. Yap: `tools.rs` (`create_note`, `edit_note`). _(03)_
- [ ] 🟡 **Search with filters**: search personal and shared notes by attendee, date range and folder, or ask the AI about the search. Yap: Meetings searches titles, attendees, notes, summaries and transcripts, but Notes only searches titles and previews in the open folder; missing: full text in Notes, and the filters. _(S · UI, T)_
- [ ] ⬜ **Quick-capture Scratchpad window**: a floating notes window on its own hotkey that opens on the last note, a pinned note or a new tab. → a new window like `notepad.rs`. _(M · 03, 09, UI)_
- [ ] ⬜ **Scratchpad button on the Flow Bar**. → `bar.rs`, with the window. _(S · HO, UI)_
- [ ] ⬜ **Rich-text editor**: bold, lists, checklists, tables, links, code and dropped images. Yap: a plain textarea. → `NotesView.svelte`. _(L · 03, UI)_
- [ ] ⬜ **Note tabs** in the Scratchpad. → with the window. _(M · 03)_
- [ ] ⬜ **Pinned notes**. → `notes.rs`, `NotesView.svelte`. _(S · 09)_
- [ ] ⬜ **Version history**: typed, dictated and AI versions; saves within 10 minutes fold together; 50 kept. → `notes.rs`. _(S · 09)_
- [ ] ⬜ **Send in {app}**: sends the note's text into the app you came from. → `text_injector.rs`. _(S · 03, UI)_
- [ ] ⬜ **My Tasks**: action items from every meeting by date, "From <meeting>", add, archive with Undo, open the meeting. → a new view over action plans. _(M · 09, UI)_
- [ ] 🚫 **Task automations**: a task becomes a Google Calendar event or a Gmail email. Won't do: needs write access to your calendar and mail (Yap's calendar is read-only by promise). _(UI)_
- [ ] 🚫 **Notes sync across desktop and phone**. Won't do: cloud-only. _(UI)_
- [ ] 🚫 **Shared folders**: share with people, a department or the company. Won't do: cloud and team plan. _(UI)_

## Notetaker: detection and recording

- [x] ✅ **Records you and the call**: mic as "You", system audio as "Them". Yap: `meeting.rs` (WASAPI loopback). _(02)_
- [x] ✅ **Records offline**: Wispr records and transcribes later in the cloud; Yap is always local. Yap: `meeting.rs`. _(02)_
- [x] ✅ **Detects calls**: notices an app taking the mic and offers to take notes. Yap: `meeting_detect.rs` (consent-store registry). _(01)_
- [x] ✅ **Browser meetings**: Meet, Teams and Zoom tabs count. Yap: by the window title. _(01)_
- [x] ✅ **Per-app choice**: work apps ask, chat apps like Discord don't. Yap: `meeting_detect_apps`, "Don't ask for X". _(01)_
- [x] ✅ **"Meeting detected" card**: the app's own icon, Start, Not now, snooze. Yap: bar `CallCard.svelte`, `app_icons.rs`. _(HO)_
- [x] ✅ **Opt-in auto-start countdown**. Yap: `meeting_auto_start`. _(01)_
- [x] ✅ **Stop when the call ends**: asks, or stops automatically. Yap: `meeting_call_end`. _(01)_
- [x] ✅ **Maximum length with a 5-minute warning and Keep going**. Yap: `meeting_guard.rs` (1–4 h or no limit; Wispr 30 min–3 h). _(01, UI)_
- [x] ✅ **Started by mistake?**: under 20 words gives Keep / Discard. Yap: `meeting_end.rs`. _(01, HO)_
- [x] ✅ **Dictations kept out of the meeting**, with a "dictated here" marker. Yap: `meeting.rs` `DictationGate`. _(T)_
- [x] ✅ **Quiet-side warnings**: no sound from your mic, or from the call, for 3 minutes. Yap: `meeting.rs` `QuietWatch`. _(01, UI)_
- [x] ✅ **Follows the default output device mid-call**. Yap: `OutputFollower`. _(T)_
- [x] ✅ **Hidden from screen capture**, with a tip when that's off. Yap: `capture.rs`. _(01, HO)_
- [x] ✅ **Meeting shortcut Win+Alt+M**: joins the live call or starts a note. Yap: `meeting_guard.rs`. _(HO)_
- [ ] 🟡 **Prompt fatigue rules**: closing the card 3 times offers to turn prompts off; ignoring it 5 times asks too; 35 unanswered cards turn detection off with a "turn back on". Yap: 5-minute snooze, 30 s fade, "Don't ask for X"; missing: the fatigue prompts. _(S · 01, UI)_
- [ ] 🟡 **Sure about the call ending**: a grace period, corroborated by the call window, tab or a click on Leave, "Stop taking notes?" when unsure, and auto-resume into the same note if the app retakes the mic within 2 minutes. Yap: a 15 s debounce, and a rejoin within it carries on; missing: corroboration and resume after an auto-stop. _(M · 01)_
- [ ] 🟡 **Switch to a new meeting**: a different call starting while recording offers "Switch?". Yap: the calendar's switch card for back-to-back meetings; detection never asks while recording. _(S · UI)_
- [ ] 🟡 **Echo and bleed**: a frame-level gate between mic and call audio, with a conference-room mode. Yap: whole chunks flagged as echo and hidden. _(M · 02)_
- [ ] ⬜ **Silence auto-end**: 10 minutes with no speech starts a 30 s "Ending notes…" countdown, which speech cancels. → `meeting_guard.rs`, bar card. _(S · 06)_
- [ ] ⬜ **Check-ins**: "still in the meeting?" 10 minutes after the calendar end and at 1 hour, unless someone spoke in the last 3 minutes. → `meeting_guard.rs`, `calendar.rs`. _(S · 06)_
- [ ] ⬜ **Offer auto-start once**: after you stop a detected call by hand (3+ meetings), asks whether to start automatically next time. → `meeting_detect.rs`. _(S · 06, 09)_
- [ ] ⬜ **"Never miss meeting notes"**: with detection off, a new call offers to turn it back on (once). Nathan's call: it means reading the mic record while detection is off. → `meeting_detect.rs`. _(S · 06, 09)_
- [ ] ⬜ **Mic mismatch**: "Teams is using X but Yap is using Y", once per call and mic. → `meeting.rs`, `meeting_detect.rs`. _(M · 07, 09)_
- [ ] ⬜ **The call's mute state**: warnings stay quiet while you're muted in the call. → with the mismatch check. _(M · 09)_
- [ ] ⬜ **Meeting audio on disk and crash recovery**: audio streams to a file, a crash resumes from it, and local audio is kept for days. → `meeting.rs` (today it's in memory). _(S–M · 02)_
- [ ] ⬜ **Low-disk guard**: "about N minutes of recording left". → `meeting.rs`, `stt.rs` downloads. _(S · 06, 09)_
- [ ] ⬜ **Sleep and offline gaps**: "your device slept for 12 min" in the transcript, and a stop on sleep with Resume. → `meeting.rs`, `Notepad.svelte`. _(S · UI)_
- [ ] ⬜ **Capture-time placement**: audio chunks placed by their capture timestamps instead of a catch-up. → `meeting.rs`. _(M · 06)_
- [ ] ⬜ **Consent notice posted in the call's chat**: types the message into Teams, Zoom or Meet chat (opt-in, Copy fallback). Fragile UI automation. → a new UIA module. _(L · 07, 09)_
- [ ] 🚫 **Calendar notice**: adds "taking notes with…" (and a notes link) to events you organise. Won't do: writes to your calendar; Yap's calendar access is read-only by promise. _(UI, T)_
- [ ] 🚫 **Organisation consent and blocks**: org-required consent before starting, one colleague posts the notice, blocked attendee domains. Won't do: enterprise/cloud. _(06, UI)_
- [ ] 🚫 **Weekly meeting-note limit** on the free plan. Won't do: paid-only (Yap's meetings are free and local). _(UI)_
- [ ] 🚫 **HIPAA/BAA gates**: Notetaker blocked under a signed BAA. Won't do: enterprise/cloud. _(UI)_

## Notetaker: transcript and speakers

- [x] ✅ **Live transcript in bubbles**: You / Them, grouped like a chat. Yap: `Notepad.svelte`, `NotesView.svelte`. _(HO, NP)_
- [x] ✅ **Live transcript toggle**. Yap: `meeting_live_transcript`. _(HO)_
- [x] ✅ **Search, copy all, copy a line**. Yap: the notepad's timer box and bubble copy. _(NP)_
- [x] ✅ **Paused dividers** where the recording stopped. Yap: `notes.breaks`. _(NP)_
- [ ] 🟡 **Speaker names from the calendar**: attendees name the voices. Yap: attendees go into the prompts and the Whisper hint, not onto lines; needs diarization first (see 10-team-and-speakers.md, in progress). _(M · 02, UI)_
- [ ] ⬜ **Edit a transcript line**: keeps the original, shows "Edited", can revert; delete a line with Undo. → `notes.rs`, `Notepad.svelte`. _(S–M · 09, UI)_
- [ ] ⬜ **A better transcript after the meeting**: the whole recording re-transcribed once it ends (Wispr in the cloud; Yap could re-run its local model). → `media::chunk_ranges`, needs audio on disk. _(M · 02)_
- [ ] ⬜ **Tell the voices in "Them" apart**: Speaker 1, Speaker 2… (Wispr: the cloud provider's diarization; Yap: local speaker embeddings). → `meeting.rs` (see 10-team-and-speakers.md, in progress). _(L · 02)_
- [ ] ⬜ **Names from the call app**: reads the active speaker from Meet, Teams or Zoom with UI Automation. Fragile, needs updatable rules. → a new UIA module (see 10-team-and-speakers.md, in progress). _(L · 02)_
- [ ] ⬜ **Assign speakers**: name each voice, merge "same person", update one block or everywhere, pick from contacts. → after diarization. _(M · 02, UI)_
- [ ] ⬜ **Rooms**: combine the voices of a meeting room into one label, or split them. → after diarization. _(M · 02, UI)_
- [ ] ⬜ **Play a speaker's clip**. → needs audio on disk. _(S · 02, UI)_
- [ ] 🚫 **Live translation of the transcript**. Won't do: cloud-only in Wispr (a local-model version could come later). _(06, UI)_
- [ ] 🚫 **Report a line, or send a meeting for ML training**. Won't do: sends your meeting to the vendor (privacy). _(UI)_

## Notetaker: AI summaries and Ask

- [x] ✅ **Summary when the meeting ends**. Yap: the Action Plan, written in Rust (`meeting_end.rs`), on a local model too. _(03)_
- [x] ✅ **Progress "Step N of 3"** with friendly lines. Yap: the notepad's runline and "catching up" line. _(03, HO)_
- [x] ✅ **Retry after a failure**. Yap: the error line + Retry. _(HO)_
- [x] ✅ **Too short to summarise**. Yap: the `nothing` state. _(03)_
- [x] ✅ **Owners only when someone committed**: attendees in the prompt; no invented owners. Yap: `meeting_summary::postcheck_action_plan`. _(03)_
- [x] ✅ **AI meeting title** when the title is blank. Yap: `meeting_assist::maybe_title`. _(02, HO)_
- [x] ✅ **What did I miss?** (since you last looked). Yap: `meeting_catch_up`. _(03, HO)_
- [x] ✅ **Suggest questions, Follow-up email, Identify blind spots**. Yap: `meeting_assist` helpers. _(03)_
- [x] ✅ **Ask anything about the meeting**. Yap: the notepad's Ask sheet, the Notes view's Ask bar. _(03)_
- [x] ✅ **Chat edits notes safely**: search, then replace an exact unique snippet. Yap: `tools.rs`, `note_text.rs`. _(03)_
- [x] ✅ **Ask across all meetings**. Yap: the Meetings view's Ask bar → Chat (scope `meetings`). _(HO)_
- [x] ✅ **Local share**: copy, save as .md, email. Yap: the notepad's Share popover (no links). _(NP)_
- [ ] 🟡 **When to write the summary**: automatic only for meetings over 5 minutes; shorter ones wait for "Generate summary". Yap: automatic for any meeting with 20+ words; decide whether short ones should wait. _(S · 03)_
- [ ] 🟡 **"New audio since this summary" → Update**, keeping the old summary if the update fails. Yap: Resume and re-run via Action plan; the stale dot only tracks typed notes. _(S · 03, UI)_
- [ ] 🟡 **Edit the summary in place**. Yap: read-only Enhanced view; the chat's `edit_note` can change it. _(S–M · UI, T)_
- [ ] 🟡 **Saved chats per meeting**: New chat, switch, rename, delete. Yap: the notepad's thread is page state (New chat clears it); Chat saves conversations but can't rename. _(S · 03, UI)_
- [ ] 🟡 **Hub prompt chips**: Coach me, List recent to-dos, Write a weekly recap. Yap: cycling example questions; missing: one-tap prompts. _(S · 03, UI)_
- [ ] 🟡 **Answers with sources**: citations, jump to the moment, date questions ("yesterday"). Yap: keyword RAG + tools; missing: citations and dates. _(M · T)_
- [ ] 🟡 **Docs tab**: documents the chat makes for a meeting. Yap: the chat can create notes; no per-meeting Docs tab. _(M · 03, UI)_
- [ ] 🟡 **Rate a summary**: 👍 👎 with a note. Yap: local ratings on Ask answers only. _(S · 03, NP)_
- [ ] ⬜ **"Make notes longer" chip**. → `meeting_assist.rs`. _(S · 03)_
- [ ] ⬜ **"Make me sound smart" chip**: something insightful to say right now. → `meeting_assist.rs`. _(S · 03)_
- [ ] ⬜ **Tasks tab in a meeting**: action items with done/total, add, complete all. → `Notepad.svelte`, `notes.rs`. _(S–M · 03, UI)_
- [ ] ⬜ **Skills**: saved instructions run with `/command` in a meeting's chat or across meetings; built-ins can be copied. → Actions (`notes.rs`) + `chats.rs`/`meeting_assist.rs`. _(M · 06, 09)_
- [ ] ⬜ **Brief before a meeting**: who you're meeting and what happened last time. Wispr builds it in the cloud from Gmail, Slack and Notion; Yap could build it from its own notes. → `calendar.rs`, `meeting_summary.rs`. _(M · 03, T)_
- [ ] ⬜ **"Your summary is ready" notice** when it finishes in the background. → bar card. _(S · UI)_
- [ ] ⬜ **Search inside the summary**. → `Notepad.svelte`. _(S · UI)_
- [ ] 🚫 **Share links, auto-share with attendees, "Shared with me"**. Won't do: cloud-only. _(03, HO, UI)_
- [ ] 🚫 **"Which summary do you prefer?"**: an A/B test. Won't do: vendor-side model testing. _(03)_

## Meeting notepad and Flow Bar

- [x] ✅ **Notepad docked to the right** when a meeting starts. Yap: `notepad.rs`. _(HO, NP)_
- [x] ✅ **Split screen with the call**, with a preview outline. Yap: `notepad_split`, `SplitPreview.svelte`. _(HO, NP)_
- [x] ✅ **Tabs My thoughts / Transcript / Summary**. Yap: `Notepad.svelte`. _(NP)_
- [x] ✅ **Stop / Resume, Generate summary, Ask pill** at the bottom. Yap: `Notepad.svelte`. _(NP)_
- [x] ✅ **⋯ menu**: Copy as Markdown, Audio settings, Delete. Yap: `Notepad.svelte`. _(NP)_
- [x] ✅ **Back to the main window**. Yap: back button → Notes. _(NP)_
- [x] ✅ **The Flow Bar**: a pill above the taskbar on the cursor's monitor, never focused. Yap: `bar.rs`, `Overlay.svelte`. _(FB, HO)_
- [x] ✅ **Hover opens Dictate / Notetaker + a ^ menu**. Yap: `bar.rs` `bar_action`. _(HO)_
- [x] ✅ **Recording pill**: waveform and stop, half opacity until hovered. Yap: `Overlay.svelte`. _(FB)_
- [x] ✅ **Notices as cards on the bar**. Yap: `bar::show_card`. _(FB)_
- [x] ✅ **Hide the bar for an hour, or turn it off**. Yap: `bar_action` hide/off, `bar_enabled`. _(04, UI)_
- [ ] 🟡 **Notepad view as one choice**: hidden / open / side by side, plus an offer to keep it hidden if you keep closing it. Yap: two switches; missing: the offer. _(S · 06, 09)_
- [ ] 🟡 **Audio panel in the notepad**: input level. Yap: Audio settings opens Settings → General. _(S · UI)_
- [ ] 🟡 **Move the bar** by dragging it (left, right or bottom). Yap: top or bottom (`overlay_position`). _(S–M · 04, UI)_
- [ ] 🟡 **Bar menu extras**: mic in use and picker, languages, paste last, transcript history, formatting options, reminder settings. Yap: Open Yap, New meeting note, Settings, Hide 1 h, Turn off. _(S · UI)_
- [ ] ⬜ **First-meeting coach marks**: "take notes here", "try What did I miss?". → `Notepad.svelte`. _(S · UI)_
- [ ] ⬜ **Typing reminder**: the bar nudges you to dictate when you type a lot in chosen kinds of apps (mute for 12 h). → `bar.rs`, `input_hook.rs`. _(S–M · 04, UI)_
- [ ] ⬜ **First-use pointers on the bar**: "click a text box, then hold…". → `bar.rs`. _(S · UI)_

## Calendar

- [x] ✅ **Google Calendar**, read-only. Yap: `calendar/google.rs`. _(01, UI)_
- [x] ✅ **iCloud and other calendars** through a private iCal link (Wispr: "coming soon"). Yap: `calendar/ics.rs`. _(HO)_
- [x] ✅ **Upcoming meetings**: today and 7 days; Conflict / Maybe tags; declined left out. Yap: `MeetingsView.svelte`. _(UI)_
- [x] ✅ **Reminder card before a meeting** (15 s, 1 or 2 min, never): Join & take notes, Start, Snooze 2 min. Yap: `calendar.rs`, bar card. _(01, HO)_
- [x] ✅ **"Connect your calendar" dialog**. Yap: `ConnectCalendarDialog.svelte`. _(HO)_
- [x] ✅ **Notes take the meeting's title, attendees and invite**. Yap: `calendar::on_meeting_started`. _(02)_
- [x] ✅ **Back-to-back switch card**. Yap: `calendar.rs` switch. _(UI)_
- [x] ✅ **Joining a meeting reopens its note**. Yap: `notes::find_by_event`. _(09)_
- [ ] 🟡 **Outlook Calendar**. Yap: a published ICS link; missing: one-click Microsoft sign-in. _(M · UI, HO)_
- [ ] 🟡 **Match a call to its event by the join code** (Meet code, Zoom id). Yap: matches by time and service. _(S · 01)_
- [ ] ⬜ **Folder rules for recurring meetings**: file this meeting or its whole series in a folder. → `calendar.rs`, `notes.rs`, `MeetingsView.svelte`. _(S–M · 06, 09)_
- [ ] ⬜ **Next meeting in the tray**: name and countdown. → `tray.rs`. _(S · UI)_
- [ ] ⬜ **Day view touches**: "done for today" wrap-up and free gaps between meetings. → `MeetingsView.svelte`. _(S · UI)_

## Home, history and Insights

- [x] ✅ **Greeting with your hotkey**. Yap: `HomeView.svelte` (amber keycaps). _(SS)_
- [x] ✅ **Total words and day streak**. Yap: `history.rs` `get_stats`. _(SS)_
- [x] ✅ **Activity heatmap and top apps**. Yap: `InsightsView.svelte` (30 days). _(09)_
- [x] ✅ **Dictation history**: day groups, copy, delete, search. Yap: `HomeView.svelte` (Ctrl+K). _(SS)_
- [ ] 🟡 **Home banners**: "make it sound like you" (styles), snippets, dictionary, Transforms. Yap: a rotating tip card; missing: a style set-up banner. _(S · SS, UI)_
- [ ] 🟡 **Undo AI edit in history**: paste or copy the raw words. Yap: `history.json` keeps the raw text; nothing shows it. _(S · 08, UI)_
- [ ] ⬜ **Words per minute**, measured over speaking time. Yap shows time saved from assumed rates. → `history.rs`. _(S · 04, SS)_
- [ ] ⬜ **Week streak and longest streak**. → `history.rs`. _(S · 09, UI)_
- [ ] ⬜ **Fix counts**: words corrected, dictionary fixes, % vs last month. → `history.rs`, `InsightsView.svelte`. _(S · 09, UI)_
- [ ] ⬜ **Cancelled dictation with Recover**: a dismissed entry stays in history and can be brought back. → `pipeline.rs`, `history.rs` (needs the audio kept). _(S–M · SS, 08)_
- [ ] ⬜ **Retry, play or save a dictation's audio** from history. → `history.rs` (opt-in audio, kept N days). _(M · UI)_
- [ ] ⬜ **Home meetings widget**: Upcoming / Past, See all. → `HomeView.svelte`. _(S · 06, 09)_
- [ ] ⬜ **Notifications centre**: a bell with tips, milestones and news, mark read, archive. → main window. _(M · UI)_
- [ ] ⬜ **Milestones**: 100 / 500 / 1k / 2k words. → `coaching.rs`. _(S · 04)_
- [ ] ⬜ **Share cards**: Insights as images to share. → `InsightsView.svelte`. _(S · UI)_
- [ ] 🚫 **Voice Profile** ("unlocks in 1.7K words": persona, peak time, catchphrase). Won't do: generated on Wispr's server (a local fun-facts card could come later). _(09, SS)_
- [ ] 🚫 **Flag a dictation**: sends it to the vendor to improve the model. Won't do: privacy. _(SS, UI)_
- [ ] 🚫 **Team leaderboard**. Won't do: cloud and team plan. _(UI)_

## Settings and system

- [x] ✅ **Launch at login**. Yap: `tauri-plugin-autostart`. _(UI)_
- [x] ✅ **Microphone picker**. Yap: Settings → General. _(UI)_
- [x] ✅ **Dead-mic watchdog**: reopens the mic, then says no sound is coming from it. Yap: `mic_watch.rs`. _(04)_
- [x] ✅ **No microphone found**. Yap: `pipeline.rs` `has_mic`. _(UI)_
- [x] ✅ **Updates in the background**: check, download, "restart to update", never mid-dictation. Yap: `updates.rs`. _(04, UI)_
- [x] ✅ **Run the setup again**. Yap: About → "Show setup guide again". _(UI)_
- [ ] 🟡 **Mute all audio while dictating**: mutes other apps after the start sound, skips the call app, and restores each app's volume. Yap: mutes the whole output, so its own chime and a live call are muted too. _(M · 07)_
- [ ] 🟡 **Install when you step away**: "Later" installs the update next time you're idle. Yap: waits for the dictation to end; no idle install (and no install-on-quit, by design). _(S · 04, UI)_
- [ ] ⬜ **Pause music while dictating**: pauses a playing media app and resumes only what it paused. → `mute.rs`. _(S · 07, 08)_
- [ ] ⬜ **Ranked microphones**: drag to rank, first present wins, forget a device, show virtual devices. → `pipeline.rs`, `config.rs`, Settings. _(M · 08, UI)_
- [ ] ⬜ **New mic detected · Switch?** and "Using X" after an automatic switch. → with the ranking. _(M · 08, UI)_
- [ ] ⬜ **Mic unplugged mid-dictation**: stops and offers to insert what was heard. → `pipeline.rs`. _(S–M · 08, UI)_
- [ ] ⬜ **Windows microphone privacy check**: "Windows is blocking the mic" with Open settings. → `mic_watch.rs`. _(S · 07)_
- [ ] ⬜ **Bluetooth mic not held open** between dictations, and a "use a wired mic" tip. → `pipeline.rs` `open_stream`. _(S–M · 08)_
- [ ] ⬜ **Mic-quality tips**: low signal ("we didn't hear you clearly"), AirPods advice. → `mic_watch.rs`. _(M · 04, UI)_
- [ ] ⬜ **Laptop lid closed**: says the built-in mic can't be used. → `pipeline.rs`. _(S · 08, UI)_
- [ ] ⬜ **Dark mode** for the main window (the bar already has Light/Dark). → `app.css` tokens. _(M · UI)_
- [ ] ⬜ **App language**: the interface in other languages. → i18n across `src/`. _(L · UI)_
- [ ] ⬜ **Notification settings and one nudge engine**: categories (suggestions, news, milestones) and shared caps for every tip. → `coaching.rs` + config. _(M · 04, UI)_
- [ ] ⬜ **Reset app**: clears local data and keeps settings. → `commands.rs`. _(S · UI)_
- [ ] 🚫 **Service status notices**: outages and slow networks. Won't do: cloud-only (there's no Yap service to report on). _(UI)_

## Integrations and MCP

- [x] ✅ **MCP server for AI apps**: meetings and notes, never dictations. Yap: `mcp.rs`, local stdio (Wispr's is hosted and paid). _(03, HO)_
- [x] ✅ **One-click Add to Claude / ChatGPT / Gemini / Cursor**, plus other apps. Yap: `mcp_clients.rs`, Settings → MCP. _(HO)_
- [x] ✅ **AI apps can save notes** (opt-in). Yap: `mcp_allow_writes`. _(UI)_
- [x] ✅ **"Set up MCP" prompts** in the app. Yap: Connectors row, Integrations card. _(UI)_
- [ ] ⬜ **Import from Granola and Otter**: past notes, summaries and transcripts. → Upload / `notes.rs`. _(M · 03, 09)_
- [ ] 🚫 **Slack, Notion, Gmail, GitHub and Linear connectors** for context in summaries and briefs. Won't do: server-side connectors (cloud). _(03, UI)_
- [ ] 🚫 **LinkedIn "Apps" badge**. Won't do: cloud (and marketing). _(UI)_
- [ ] 🚫 **Mobile app** and "text yourself the link". Won't do: needs a cloud sync; Windows-first. _(UI)_
- [ ] 🚫 **Claude CLI bridge** that launches the CLI with permissions skipped. Won't do: unsafe. _(03)_

## Accounts, team, billing and referrals

- [x] ✅ **Sign in**. Yap: `auth.rs` (Google, GitHub, Discord, email code, phone QR). _(UI)_
- [x] ✅ **Sign out and delete account**. Yap: `AccountSection.svelte`. _(UI)_
- [ ] 🟡 **Profile**: edit your name and picture. Yap: shows the provider's name and avatar; no editing. _(S · UI)_
- [ ] 🚫 **Invite your team**: email fields, an invite link, a 2-week Pro trial for invitees. Won't do: needs a team backend and paid plan (see 10-team-and-speakers.md, in progress, for whether teams help name speakers). _(SS, UI)_
- [ ] 🚫 **Teams**: create a team, auto-join by email domain, join requests, departments, admin approval. Won't do: cloud and team plan. _(UI)_
- [ ] 🚫 **Plans and billing**: Free / Pro, trials, annual plans, a weekly word limit. Won't do: paid-only; Yap's core stays free (revisit with the planned paid tier). _(UI)_
- [ ] 🚫 **Referrals and student offers** ("Get a free month"). Won't do: paid-only. _(SS, UI)_
- [ ] 🚫 **Enterprise**: HIPAA/BAA, IP allowlist, org insights, org-managed settings. Won't do: enterprise/cloud. _(UI)_

## Privacy and data

- [x] ✅ **Your data stays on your PC**: Wispr stores dictations in its cloud unless you opt out. Yap: everything local (`%APPDATA%\yap`). _(UI)_
- [x] ✅ **Context awareness switch**: whether the text around the cursor is read. Yap: "Match the text around the cursor" (`smart_spacing`). _(UI)_
- [x] ✅ **Delete all transcripts**. Yap: Settings → History → Clear. _(UI)_
- [ ] 🟡 **Keep history: store / auto-delete after 24 h / never**. Yap: history on/off and Clear; missing: 24 h auto-delete. _(S · 09, UI)_
- [ ] 🟡 **Download your data**: one copy of dictations, meetings, notes and settings. Yap: per-note .md export; the data folder is plain JSON; missing: a one-click export. _(S · UI)_
- [ ] ⬜ **Transcript retention**: meeting transcripts deleted after N days. → `notes.rs`, config. _(S · 09, UI)_
- [ ] ⬜ **Stats survive deletion**: a content-free usage row (words, time, app) is kept when history isn't. → `history.rs`. _(S · 09)_
- [ ] 🚫 **Cloud storage and sync**: the switches, plus a sync icon by the version number. Won't do: cloud-only. _(UI, SS)_
- [ ] 🚫 **"Improve the model for everyone"**: a data-sharing switch. Won't do: Yap collects nothing, so there's nothing to opt into (privacy). _(UI)_

## Onboarding and help

- [x] ✅ **Guided setup**: mic test, shortcut, try it. Yap: `Onboarding.svelte` (5 steps; also picks a model and installs local AI). _(09)_
- [x] ✅ **What's new**: release notes after an update. Yap: the update toast and About. _(UI)_
- [ ] 🟡 **Try it in mock apps**: Slack, Gmail, Notion and WhatsApp look-alikes. Yap: one try-it box. _(S–M · 09)_
- [ ] 🟡 **Help menu**: help centre, shortcuts, mic, languages, talk to support. Yap: About links to GitHub; missing: a Help menu and a shortcut sheet. _(S · UI)_
- [ ] ⬜ **"Set up Notetaker" checklist** in the sidebar: take the 2-minute tour, connect a calendar, import from Granola or Otter, set up MCP. → `ControlPanel.svelte`. _(S · SS, UI)_
- [ ] ⬜ **2-minute Notetaker tour** with a scripted demo meeting. → Onboarding + `e2e_meeting_feed`-style feed. _(M · 09)_
- [ ] ⬜ **Language step** in onboarding. → `Onboarding.svelte`. _(S · 09)_
- [ ] ⬜ **Style step**: how you write emails and messages, picked from cards. → with styles. _(S · 09)_
- [ ] ⬜ **Typing-speed test**: your typing vs speaking speed. → `Onboarding.svelte`. _(S · 09)_
- [ ] ⬜ **Mic quiz**: helps pick the right mic. → `Onboarding.svelte`. _(S · 08, UI)_
- [ ] ⬜ **Connect calendar and "how do you record meetings?"** steps (the second leads to import). → `Onboarding.svelte`. _(S · 09)_
- [ ] ⬜ **Feature tips**: "self-correction caught", "listed", "email formatted" the first time each happens. → `coaching.rs`. _(S · 04, UI)_
- [ ] ⬜ **In-app feedback**: report an issue or share an idea, with screenshots (Yap could prefill a GitHub issue). → Settings → About. _(S · UI)_
- [ ] ⬜ **Welcome back** for people who haven't used it in a while. → `HomeView.svelte`. _(S · 09)_
- [ ] 🚫 **"About you" and product-intent surveys**. Won't do: marketing data sent to the vendor (privacy). _(09)_
