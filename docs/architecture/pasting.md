# Pasting: where the text goes

Putting a dictation into the app you're in: the clipboard paste and the submit key, apps running as administrator, UI Automation reads of the field, fitting the text to what's around the cursor, the kind of app (terminals, coding CLIs, chat apps), and the last paste afterwards (swapped for the words as said, or rewritten). The pipeline that calls all this is [dictation.md](./dictation.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`pasting/text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`, only
  while the dictation's window is still in front (`submit_goes`, pure: a
  dialog that came up over it after the paste never gets the Enter; the
  dictation's `error` says so). **An app
  running as administrator** (`pasting/elevated.rs`, checked first — the paste's target,
  "Paste last transcript"'s, the submit key's window) gets no keys: the text is
  left on the clipboard (`leave_on_clipboard`, as privately as a paste) and
  `inject_text` returns `Delivery::LeftOnClipboard` for the caller to say so.
  Captures the
  dictation **target window** at record-start (`current_foreground`, skipping Yap's
  own windows) and **re-focuses** it before pasting (`focus_window`, via the
  `AttachThreadInput` workaround) so focus changes mid-transcription don't misfire.
  **Paste privacy** (`clip`, Win32 via the `windows` crate — arboard can't add
  formats): the dictation goes on the clipboard with
  `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0,
  `CanUploadToCloudClipboard` = 0 and `Clipboard Viewer Ignore`, so it never lands in
  Win+V history, cloud clipboard sync or a well-behaved clipboard manager; the restore
  of the user's own clipboard carries them too (it's in their history already). A
  remote-desktop/VM target (`syncs_clipboard`: mstsc, msrdc, vmconnect, Citrix,
  VMware, VirtualBox, Parsec, AnyDesk…) gets a plain write — its clipboard sync is a
  monitor too. **Every write is verified** (`GetClipboardSequenceNumber` must move;
  4 tries, 0/15/40/80 ms), else the text is typed (`type_unicode`); outcomes at
  `debug`. The snapshot keeps **text, an image** (CF_DIBV5 + PNG, raw bytes) **or
  copied files** (CF_HDROP + drop effect), ≤ 64 MB, and an empty clipboard is left
  empty; `selection_via_copy` (edit-mode fallback) empties the clipboard, polls for
  the Ctrl+C result instead of a fixed sleep, and restores privately.
  `prepare_dictation(text, append_space, dictionary, kind)` (run_stt's one call)
  remembers the dictation and fits it to the kind of app (`kind`, `pasting/app_kind.rs`,
  already filtered by the settings: a terminal → `terminal_text`, nothing else; a
  chat app → `drop_chat_full_stop` first) and to the text around the cursor read
  at record start (`selection::take_context` → `smart_insert::fit_to_context`;
  dictionary spellings never lower-cased), or, with no context, appends the space
  as before. **"Paste
  last transcript"** (tray item + `paste_last_transcript` command → the text it
  pasted; errors "Nothing to paste yet"): this session's last dictation (unfitted),
  else history's newest dictation (`history::latest_dictation` — app an `.exe` or
  empty, so never an Upload); forgotten when history entries are deleted/cleared.
  It pastes into the foreground app, or — the tray menu holds the foreground — the
  top real app window (`pick_paste_window`, unit-tested: not Yap's, minimised, tool,
  no-activate, cloaked, untitled or a shell window), classifying it
  (`app_kind::of_window`) and re-reading the cursor's context there
  (`selection::read_context`, not in a terminal). Test mode returns the text without pasting
  ("e2e: not pasting the last transcript"). No hotkey for it yet. **"Paste
  last transcript (as said)"** (the tray's next item; `paste_last_transcript
  {asSaid: true}`): the same with the words before AI cleanup
  (`last_said_transcript`: this session's, kept by
  `prepare_dictation_as_said`, else history's newest transcript through
  `pipeline::as_said_text`); after a dictation cleanup left alone (cleanup
  off, the default) it's the dictation itself. Until 2026-10-23 that case
  froze Yap: the last dictation's lock was taken again inside the statement
  that held it (a std `Mutex` never returns), so the tray's worker hung and
  the tray menu and the next dictation behind it. Each lock is now let go
  at the end of its own statement (unit test
  `paste_last_as_said_after_a_dictation_cleanup_left_alone_never_hangs`;
  e2e `dictation-trust.spec.js`, robot `as-said.real.js`). `copy_privately` puts text on the clipboard
  as a paste would and leaves it (Use what I said's refusals).
  `app_window()` / `focus(hwnd)`: the same window-finding and re-focusing
  for the tray's "Read selection aloud" (`voice/read_aloud.rs`). **The paste
  keys** (`simulate_paste(target, key)`; `inject_text`'s `kind` is the window's
  kind as read, before the settings): Ctrl+V as always (virtual keys only), or a
  terminal's own (`app_kind::Host::paste_key`: Shift+Insert, Ctrl+Shift+V) sent
  as a keyboard sends them — each key's scan code in the target thread's layout
  (`MapVirtualKeyExW`), Insert flagged extended (not the keypad's 0), so winit
  apps (Alacritty, Rio) match their bindings and mintty/PuTTY see the real
  Insert. **Did it land?** (verify-after-paste): before the keys,
  `selection::PasteCheck::before` takes a **Win32 edit control** with the focus
  in the target's process (a WinForms box, EDIT, RichEdit — not a password box,
  not read-only, not in a terminal) and its text length + selection; after
  them `wait_for_paste` reads it every 40 ms: a change → landed; a read that
  fails (no answer in 200 ms, the focus moved) → nothing more; the same for
  `PASTE_LANDS_MS` (1 s) → the text is typed (`type_unicode`), once, only while
  that control still has the focus (and robot mode's foreground re-check), logged
  "The paste didn't reach the field", `Delivery::Typed` (the dictation goes on
  as pasted: submit key, correction watch). Only edit controls: a web or code
  editor takes a paste through a hidden input it empties again (Google Docs,
  Ace, xterm.js), so "unchanged" means nothing there; length + selection, not
  the text — any paste changes one (a selection replaced by the same words
  moves the caret). Everywhere else (web pages, Electron apps, code
  editors, terminals) **the clipboard read says** (`pasting/read_check.rs`,
  below): the text goes on the clipboard *promised* and the app's read of
  it is the proof; nothing read it in time → `Delivery::LeftOnClipboard
  (Left::NotRead)`. The clipboard restore is timed by that read, not the
  keys. `#[ignore]`d `private_clipboard_round_trip` runs
  the real clipboard code on a private window station (never the person's clipboard).
  **When it can't be pasted** (2026-10-25): `inject_text` returns
  `Delivery::LeftOnClipboard(Left)` — `Elevated` (above), `KeysBlocked`,
  `NoTextBox` or `NotRead` (2026-10-29: no app read the text off the
  clipboard in time; "Couldn't paste · it's copied, press Ctrl+V", "The app
  didn't take the paste…") — the text on the clipboard, and the caller says it
  **once** with `Left::alert()` (kind "pasteBlocked", through
  `notices/alert.rs`; history keeps the dictation "not pasted" with
  `Left::not_pasted()`; the robot's report says `leftWhy`). **Did Windows
  take the keys?** Every `SendInput` is checked (`platform::send`): fewer
  events put in the input stream than asked (a secure desktop — a UAC
  prompt, the lock screen —, another app blocking input, UIPI) is a
  failure, logged with the counts and `GetLastError`; any key the events
  that did go left down is let go. The paste keys go again once after
  150 ms (`press_paste`); still refused → `KeysBlocked`: the dictation stays
  on the clipboard (the person's own isn't put back) and "Couldn't paste ·
  it's copied, press Ctrl+V". Typing (`type_unicode`), the submit key and
  edit mode's Ctrl+C are checked the same way (a submit key refused is the
  dictation's `error`; typing refused after a lost paste → `KeysBlocked`).
  **"Select a text box first"**: before the paste keys (not in a terminal,
  not when a Win32 edit control is being checked) `selection::focus_editable`
  asks UI Automation whether what has the focus takes text; on a plain
  **no** the keys still go (Discord and Slack move the focus to their box on
  a paste), and 250 ms after them it's asked again: still no → `NoTextBox`,
  the text left on the clipboard and "Select a text box first · …click
  where it goes and press Ctrl+V" (calm, not blaming). Unknown is never a
  no (a game, a canvas, an app UI Automation can't see). **The modifier
  keys** around every key sequence Yap sends: `pasting/modifiers.rs` (below).
  **The console's own paste**: a classic console window (conhost,
  `ConsoleWindowClass`) gets no keys — `PasteKey::ConsoleMenu` posts its
  Edit → Paste (`WM_COMMAND` 0xFFF1): its Ctrl+V pastes only for a program
  reading lines (cmd, PowerShell); one reading the keyboard raw (Claude Code,
  Codex, vim) gets Ctrl+V as a ^V and Shift+Insert as an escape sequence,
  where the console's paste reaches it, in bracketed paste when it asked
  (the robot, 2026-10-25). **Long pastes in slices** (`pasting/slices.rs`,
  below): `inject_text_sync` pastes a long text into Claude Code, Codex or
  anything in Windows Terminal slice by slice, each put on the clipboard and
  pasted once the terminal has read the one before (2026-10-29), and at
  least `slices::PACE_MS` (250 ms) after it; a slice that can't go leaves
  the rest of the text on the clipboard (`KeysBlocked`), one not read in
  time leaves it from that slice on (`NotRead`). **The text box itself**
  (2026-10-29): after re-focusing the window, `selection::restore_focus`
  puts the focus back on the element the dictation started in when it
  wandered off within the window to something that takes no text
  (`selection.rs`, below). **Edit
  mode's Ctrl+C** (`copy_selection`, `selection_via_copy`; Trouble said once
  through `notices/alert.rs`, kind "clipboardBusy"): everything on the
  clipboard is kept first, **every format byte for byte**
  (`clip::snapshot_exact`: memory formats as bytes, an enhanced metafile by
  its bits; the bitmap, palette and old metafile picture Windows makes again
  from a DIB / an enhanced metafile left out; OLE's own `DataObject` / `Ole
  Private Data` never put back; anything else held as a handle — owner
  display, GDI or private handles — or over 64 MB → `CantKeep`), waiting up
  to 1 s for an app that holds it (`Busy`); either → nothing pressed,
  nothing touched, "Couldn't read your selection". Ctrl+C's result is told
  by the clipboard's sequence number (the clipboard is never emptied
  first: no selection, nothing changes, nothing to put back); the text read
  with up to 1 s's patience; everything put back (`clip::restore_exact`,
  privately, verified by the sequence number) within 2 s, else
  "Your clipboard couldn't be put back" (`NotRestored`: it holds the
  selection). Test mode never presses Ctrl+C; robot mode only when its spec
  asks (`realtest_copy_selection`).
- **`pasting/read_check.rs`** — **did the app read the paste?** (2026-10-29;
  Wispr Flow's delayed clipboard rendering, by behaviour). `Offer::put`:
  the dictation goes on the clipboard *promised* —
  `SetClipboardData(CF_UNICODETEXT, NULL)` from Yap's own clipboard window
  (a message-only window on its thread, "clipboard-owner", made once:
  `owner()`) — with the privacy formats written at once (a careful clipboard
  manager sees them without asking for the text); checked by the sequence
  number and retried like a plain write; `None` (no window, a busy
  clipboard) → `clip::put_text_verified` as before, and no read to go by.
  Windows sends the window `WM_RENDERFORMAT` the moment an app reads the
  text, and `on_render` decides:
  **who read it** (`classify`, pure): the window that has the clipboard
  open (`GetOpenClipboardWindow`) and its process, against `targets_of
  (receiver)` — the target window and the window with the focus in its
  thread, their processes, every child window's process (a WebView2 app's
  browser process — new Outlook, Teams —, a UWP app inside
  ApplicationFrameHost's frame), and the same program by name (not a
  generic host: python, java, node, cmd, PowerShell, AutoHotkey,
  msedgewebview2…) → `Target`; Yap's own process → `Yap`; anyone else →
  `Other`; no window at all → `Unknown` (Windows Terminal reads on a
  thread of its own with `OpenClipboard(NULL)`, and so does Edge's
  browser process — the robot, 2026-10-29; with no target known,
  everyone but Yap). Yap's own reads open the clipboard with no window
  too: they're told apart by Yap holding the clipboard open then
  (`clip::yap_holds_it`, one opener at a time) and kept out of the
  report. **Who gets it** (`answer`, pure): the target and Yap
  always; before the paste keys nobody else — a reader then isn't pasting
  (a clipboard manager reacting to the change; Win+V history and cloud
  sync honour the privacy formats and never ask) and gets nothing, the
  text stays promised (Windows asks again on the next read: the target's
  still shows), counted as `refused`; from the keys on (`keys_going`,
  set just before them) everyone, so a wrong guess never breaks a paste;
  a remote desktop or VM (`text_injector::syncs_clipboard`, written plain)
  is open to all from the start (its sync may read from a process of its
  own). **What it says** (`seen`, pure, on the first read that got the
  text): the target's after the keys → `Target { sure }` (before them only
  a remote desktop's sync counts: that's how its paste gets there — a
  browser looking at the clipboard first isn't the paste); one nobody can
  name after the keys → `Target { sure: false }`; anything else → `Taken`
  (the text is no longer only promised: no telling); none → `Nothing`.
  **The verdict** (`combine`, pure, in `inject_text_sync`): a Win32 edit
  control's own check (`PasteCheck`) first — changed → pasted, unchanged
  for a second → typed, as before; else the target's read, named by its
  window → pasted (even where UI Automation said no text box: it took it
  somewhere); "Select a text box first" before and after the keys →
  `NoTextBox` (looked at again 250 ms after the keys unless the target,
  named, read it by then) — a nameless read never overrides it: Edge's
  browser reads the clipboard on any Ctrl+V, a button focused or a page
  that swallows the key (the robot, 2026-10-29), so in Edge the read says
  the keys arrived, not that the page took them; then a nameless read →
  pasted; `Taken` → pasted; nothing within `Timing::read_within` →
  `NotRead`: the text stays (promised) on the clipboard, the person's own
  isn't put back, "Couldn't paste · it's copied, press Ctrl+V". **How
  long** (`timing(exe)`, pure; Wispr Flow's figures): read within 1 s,
  the clipboard back 0.5 s after the read; Outlook (both), Word and
  WhatsApp 3 s and 3 s; a remote viewer (`syncs_clipboard`, rdcman) 3 s
  and 5 s. **The restore** (`end_for_restore`): only while the clipboard
  is still Yap's (`GetClipboardOwner`; someone who copied since keeps
  theirs — `WM_DESTROYCLIPBOARD` ends the promise), `restore_after` from
  the target's read (from the keys when it was `Taken`, or there's no
  promise); due within `WAIT_FOR_RESTORE_MS` (600 ms) it's waited for (the
  submit key still goes after it, as before), later — a slow app's — in
  the background (`restore_later`, the window's timer), and a paste before
  then takes that snapshot over (`take_pending`: the person's clipboard,
  not the last dictation). "Restore clipboard" off → the text written out
  (`leave(0)`). **A late read** (`leave(LATE_MS)`, 5 s): with no read in
  time nothing is put back, so an app reading late gets the dictation,
  never the old clipboard; the promise is kept 5 s more and a read then is
  noted (`Report::late`, "The paste was read late…" in the log), then the
  text is written out so it outlasts Yap (`WM_RENDERALLFORMATS` writes it
  out too). Watching the field for the old clipboard instead was
  rejected: the old clipboard can be anything (an image, files, words
  already in the field) and only some fields can be read. **Terminals**:
  no edit-control check or UI Automation there, so the read is the check —
  Windows Terminal's nameless read after the keys counts, mintty and
  conhost (its own Edit → Paste, posted after `keys_going`) open with
  their windows; long pastes wait for each slice's read. **Reliability**:
  a reader is named only by the window it opened the clipboard with —
  Win32 controls, OLE/WinRT readers (OLE's clipboard window in their
  process), Chromium/Electron (its clipboard window in the browser
  process), Java and conhost do; an app opening it with none is
  `Unknown`, counted only after the keys (a clipboard manager reading
  without a window right then would be taken for the target — it reads
  when the clipboard changes, before the keys, and is refused there). The
  log says the reader's kind, the ms after the keys and how many reads
  were refused, never the text. Test mode never touches the clipboard
  (`e2e_paste_fails "notRead"` stands in); robot mode's "notRead" refuses
  every app's read until the paste is judged; `realtest_clipboard
  "lastPaste"` reports the last paste (`Report`: reader, sure, readMs,
  refused, late, verdict, restored, refocus). Unit-tested (each decision,
  the timings); `#[ignore]`d `private_clipboard_promise_round_trip` runs
  the real window on a private window station and desktop (a manager's
  read refused twice, the target's counted, a nameless one, another app's
  after the keys, ANSI text made from the promise, nobody, a late read, a
  copy before the restore, a background restore, a remote desktop's);
  robot: `paste-read.real.js` (the pad, a stand-in clipboard manager —
  a program of its own name: one sharing the target's counts as it —
  before and after the keys, an Edge textarea, Notepad, Windows Terminal,
  conhost, Claude Code's stand-in in slices, an Electron app and VS Code
  when present), `paste-late.real.js` (the pad's slow box: read at 0.7 s,
  at 2 s, never).
- **`pasting/as_said.rs`** — **"Use what I said"** (2026-10-22; Windows' fluid
  dictation's Revert, TypeWhisper's Restore Raw Transcript): right after a
  dictation AI cleanup changed, swap what Yap pasted for the words as said
  (the transcript with corrections, the voice command and snippets, no
  cleanup — `pipeline::as_said_text` — fitted to the same place). `deliver`
  `remember`s the paste (window, kind, both texts) unless a submit key sent
  it; three ways in: the shortcut (`as_said_hotkey`, unbound by default —
  `input_hook`'s sixth binding, `as-said-key-pressed`), the Yap bar's
  "Cleaned up · Use what I said" for `OFFER_MS` (6 s; `as_said_offer`, off by default since 2026-10-25;
  only when the words differ, `words_differ`), the command `as_said_swap`.
  **The swap** (`swap`, within `SWAP_WITHIN` = 2 min, once): the dictation's
  window must still be in front; never a terminal (its line can't be
  selected), an app running as administrator, a password box or a read-only
  field. Then Yap **selects exactly what it pasted**: in a **Win32 edit
  control** whose ValuePattern gives its text (WinForms, EDIT) the pasted text
  must end at the caret (`EM_GETSEL`) — else be in the field exactly once —
  and `EM_SETSEL` selects it, `EM_GETSEL` confirming; in a **TextPattern**
  field (Edge/Chrome, Windows 11 Notepad, Word…) the text just before the
  caret must end with it (else `FindText` must find it exactly once), a range
  is built back from the caret counting the field's own characters
  (`unit_counts`: code points, "\r\n" as one, UTF-16 — the one whose range
  reads back exactly the paste), `Select()`ed, and the field's selection must
  read back the same. Line breaks, no-break spaces and the trailing space Yap
  added compare loosely (`match_before`, pure, unit-tested). The words as
  said are then pasted over the selection (the dictation's own private,
  verified paste: back into its window or nowhere) and the field read again
  (`Outcome::Swapped { checked }`). It never uses the app's Undo: Yap can't
  tell what that would take back. **Where it refuses** (`Err(why)` from
  `replace`): nothing in the app is touched — the words as said go on the
  clipboard (`copy_privately`) and the person is told "Couldn't replace it
  here — copied the raw text instead" (an `notices/alert.rs` "pasteBlocked" alert,
  with why). Test runs only log ("e2e: not swapping the paste for the words
  as said"); robot mode only in the test's windows (`e2e-real/as-said.real.js`:
  the test pad, Notepad, an Edge box, a chat app and its contenteditable,
  swapped; a terminal, an edited paste and another window in front, refused).
  **The last paste** (2026-10-23): `remember` keeps the last text Yap pasted
  into an app's window — a dictation, or a Command's / rewrite's answer
  (`pipeline::run_agent`) — with the words as said only when cleanup changed
  them (the swap needs them; `offered` too), and its history entry once
  recorded (`recorded(ts, text)`); `forget` when a paste didn't land or a
  submit key sent it. **Home's Paste what I said** (`as_said_paste_entry
  {ts, text}` → `paste_entry`): the entry the last paste is, within
  `SWAP_WITHIN` and not swapped yet → its window brought back to the front
  (`bring_back`: Yap's own window has it after the click; ≤ 0.8 s, then the
  app's moment to put its focus back) and the very same swap, every check
  included (`swap_now`); any other entry, or a window that won't come back
  → its words as said (history's `said`) copied privately, `Outcome::Copied
  { why, told }` (`told`: an alert said it already; else Home's toast says
  "Copied what you said" and why). A test run neither swaps nor copies (the
  log says which it would). **Rewriting the last paste** (the rewrite
  shortcut with nothing selected): `rewritable(on, pasted_in, kind, age,
  target)` (pure: the switch on — Settings → Commands, `config.
  rewrite_last_dictation`, default on; off = write mode as before —, the
  same window, not a terminal, ≤ `REWRITE_WITHIN` = 60 s, Wispr Flow's
  minute) and `rewrite_target` at the key press (a test run only its
  stand-in window, where nothing is read; robot mode only the test's
  windows); `rewrite_check` before the
  AI is asked — `usable` (in front, not a terminal, not an admin app) and
  the paste ending **at the caret** (`uia::ends_at_caret`; no "there once"
  fallback: typed after it, or the cursor moved, refuses — the swap still
  finds it there) — else `cant_rewrite` ("Couldn't rewrite your last
  dictation", nothing pasted); `replace_last` then selects it again at the
  caret (`select_pasted(…, anywhere: false)`), pastes the answer with the
  selection's own spaces around it (`keep_spaces`, pure), reads it back and
  remembers it as the last paste (`Rewrote::Replaced`); refused → nothing
  changed in the field, the rewrite copied privately with "Couldn't replace
  your last dictation — copied the rewrite instead" (`Rewrote::Copied`).
  Robot tests: `e2e-real/rewrite-last.real.js`, and Home's paste in
  `as-said.real.js`. **Saved transforms** (`dictation/transforms.rs`,
  [dictation.md](./dictation.md)) take the last dictation by these same
  rules (`rewrite_target`, `rewrite_check`, `replace_last`), and their
  result is the last paste too, with the text it replaced (`undoable(before,
  pasted)`, `Pasted.before`; `pasted_text` is the paste with Yap's spaces).
  **The bar's Undo after a transform** (`undo_transform`, once, within
  `SWAP_WITHIN`; `undo_offered(seq)` while it's still the last paste):
  that result selected exactly, still ending at the caret
  (`replace_in_field(…, anywhere: false)`), and the text from before pasted
  back with the spaces around it kept, read back (`Undid::Restored`); where
  it can't, that text is copied privately and said ("Couldn't undo it here
  — copied your text instead", `Undid::Copied`); a test run only logs
  ("e2e: not undoing the transform"). `wait_for_modifiers_up` is shared
  with the transforms and Paste last's shortcut (a shortcut's Alt still
  down turns Ctrl+V into another key).
- **`pasting/last.rs`** — **Paste last transcript** and **Copy last
  transcript** on shortcuts of their own (2026-10-25; Wispr Flow's, by
  behaviour; Settings → Shortcuts, `paste_last_hotkey` /
  `copy_last_hotkey`, unbound by default — `input_hook.rs`, only the press,
  never a key another shortcut has). **Paste last** (`paste_soon`, its own
  thread): once the shortcut's keys are up (`as_said::wait_for_modifiers_up`),
  exactly the tray's item — `text_injector::paste_last_blocking`: the
  latest dictation (this session's, else history's newest) into the window
  in front, fitted to its cursor and kind of app; an app running as
  administrator gets it on the clipboard and the alert. **Copy last**
  (`copy_soon`; the tray's new "Copy last transcript"; Yap's own windows'
  in-page fallback through `copy_last_transcript`, which also returns it):
  the latest dictation as it was before fitting, onto the clipboard
  privately (`text_injector::copy_privately`: out of Win+V history, cloud
  sync and clipboard managers, as a paste), nothing typed anywhere. Either
  with nothing dictated yet: "Nothing to paste yet" / "Nothing to copy yet".
  Those and "Copied your last dictation" are **quiet notes**
  (`alert::confirm`, kind `lastTranscript`: the main window's toast while
  you look at it, else a bar card; never a Windows notification; shown now
  or within 5 s, else dropped). Paste last has no in-page fallback (it
  pastes into another app, as "Use what I said" does). The log says
  lengths, never the text. Test mode copies and pastes nothing ("e2e: not
  copying the last transcript"); debug-only `e2e_shortcut { name:
  "paste-last" | "copy-last" }` sends a press as the hook would. e2e
  `dictation-polish.spec.js`; robot `e2e-real/last-transcript.real.js`
  (the pad, Notepad, an Edge text box; the clipboard's private formats).
- **`pasting/modifiers.rs`** — **the modifier keys around Yap's own**
  (2026-10-25). A dictation ended by a chord can leave Alt, Win, Shift or
  Ctrl down as far as Windows knows (a key-up the hook swallowed, Win+L in
  between, Sticky Keys), and then Ctrl+V is Ctrl+Alt+V, Win+V opens
  clipboard history and typed letters are menu keys. Every key sequence
  Yap sends (`text_injector::platform::send_keys`: the paste keys, typing,
  the submit key, Ctrl+C) goes out with them sorted (`plan`, `around`,
  pure, unit-tested over every combination): every modifier Windows says is
  down (`GetAsyncKeyState`, left and right apart) is let go first; those the
  person **holds** are pressed again straight after the keys, **in the same
  `SendInput` call** (Windows never interleaves other input with one), so
  none is ever logically up under a finger; letting go of Alt or Win taps
  the menu mask key (vkE8, as `input_hook.rs`) first, and pressing one
  again taps it after, so no menu bar or Start opens. What the person holds
  comes from the keyboard hook: `input_hook.rs` calls `note_key` for every
  key event (one line; its rules untouched), and only events without
  `LLKHF_INJECTED` set or clear a key's bit; the hook's word counts only
  for a key Windows also says is down (a release missed while one of Yap's
  own windows had the focus). `wait_until_let_go` (`as_said::
  wait_for_modifiers_up`: a shortcut's keys before it pastes) waits only
  for keys the person holds, not stuck ones. The log says which were stuck
  and which held (names only). Test mode has no hook: nothing counts as
  held unless debug-only `e2e_held_keys { keys }` says so (robot:
  `paste-safety.real.js`, an Alt or Shift pressed as an injected key).
- **`pasting/slices.rs`** — **long pastes in slices** (2026-10-25; Wispr
  Flow's chunked paste, by behaviour). Claude Code folds a paste of more
  than 800 characters or more than 2 line breaks into "[Pasted text #N +L
  lines]" (2.1's own code), Codex one of more than 1,000 characters into
  "[Pasted Content N chars]": the text goes in, but as a blob the person
  can't read before sending. And Windows Terminal stops any paste over 5 KiB
  with its `largePasteWarning` dialog — nothing arrives until it's answered
  (the robot: 4,004 characters arrived, 16,019 nothing). So `limits(kind)`
  (the kind as read): Claude Code 800 / 2 breaks, Codex 1,000, Windows
  Terminal 4,000 — the stricter of the CLI's and the host's; everything
  else one paste. `cut` (pure, unit-tested): each slice as long as it can
  be, in UTF-16 units as the CLIs count, ending after the last space or
  line break (after the last line break when it's the breaks that ran
  out), a word cut only when it alone is longer than a slice, never inside
  a character or a "\r\n"; joined, exactly the text. Measured in real
  terminals (`long-paste.real.js`): 1,000 to 64,000 characters arrive whole
  in Windows Terminal (in slices) and conhost (one paste, its own Edit →
  Paste); Claude Code's stand-in gets slices of ≤ 800 / ≤ 2 breaks, all of
  it, no Enter. VS Code's terminal isn't installed here (unmeasured: only
  the CLI's limits apply there).
- **`pasting/stacked.rs`** — **stacked messages** (2026-10-29; Wispr
  Flow's, by behaviour, on a spoken separator; `config.stacked_messages`,
  **off** by default — a message can't be taken back, and the phrase is
  one to know). In a chat app (`app_kind`'s Messaging as read, or
  cleanup's Work chat / Personal chat by process or tab title), "new
  message" or "next message" said **as a sentence or clause of its own**
  ("Hi Sam. New message. Are you free?", "…, new message, …") cuts the
  dictation into messages (`split`, pure: never inside a sentence — "a new
  message from Sam", "New message from Sam: hi" (a word that carries the
  sentence on), "hi sam new message" with no punctuation; empty parts
  dropped, the comma or dash that tied a part to it off, a lower-case first
  word capitalised). **Only where it's sent anyway** (`decide`):
  auto-submit on, or "press enter" said (`app_kind::submit_key` gives a
  key) → `Stacked::Send` — each message fitted (`text_injector::
  prepare_messages`: the first to the cursor's text, the rest to the empty
  box a send leaves, a chat's short message without its full stop) and
  pasted and sent in turn (`compose.rs`); not sent → `Lines`, one paste,
  the messages on lines of their own; not a chat app or nothing said →
  as it is. **Wispr's trigger** (read in its shipped code): every sentence
  of a personal-chat dictation a message, shown in its scratchpad for a
  click to send; Yap has no such window, so it never guesses where a
  message ends — an AI splitting would send a half-thought now and then,
  and the on-device model can't be relied on for it. AI cleanup that took
  the separators out (or rewrote them): the **words as said** are split
  instead (corrections and snippets, no cleanup), so the messages asked
  for still go. **Undo**: a sent message can't be taken back by Yap (it's
  in the chat); "Use what I said" and rewriting the last dictation don't
  apply (as after any dictation a submit key sent); a cancelled
  dictation's Undo runs it again as a new one, into its own window or
  onto the clipboard (strict, as ever).
- **`pasting/mentions.rs`** — **@mentions** (2026-10-29; Wispr Flow's Slack
  tagging, by behaviour; `config.chat_mentions`, on; needs `screen_names`):
  "at <a name on screen>" (`find`, pure: a clause's start — the text's
  start, punctuation, a line break, "hey", "thanks", "thank you", "cc",
  "and"… before "at" —, the longest name on screen said, never followed by
  "'s": "see you at Rochelle's", "lunch at Zizzi" stay), or "@Name" as AI
  cleanup may write it. The names: the screen's names and full names
  (`people`: not the app's own). **What each app does** (their help pages,
  2026): **Slack** and **Discord** pick from their "@" list with Tab (as
  Enter — but Enter sends when the list has closed): "@Sam" typed
  (`text_injector::type_text`, keystrokes: a paste doesn't open the list
  everywhere), and Tab only once UI Automation shows the list's highlighted
  entry is that person (`selection::picked_option`: the screen reader's
  focus on a list item, from `aria-activedescendant`; `option_is`: every
  word said a whole word of it — "Sam" isn't "Samantha"), within 1.2 s;
  else no key and "@Sam" stays as text. **Teams, Google Chat, Mattermost,
  Rocket.Chat, Zulip, Element**: their lists take Enter (Teams' Tab isn't
  documented), so plain "@Sam" in the one paste. **WhatsApp, Telegram,
  Messenger, Signal** and other personal chats: nothing changes. By
  process, or a browser's tab title (`which`).
- **`pasting/file_tags.rs`** — **file tagging** (2026-10-29; Wispr Flow's
  for Cursor/Windsurf, by behaviour; `config.file_tags`, on; needs
  `screen_names`): the dictation's code names (`dictionary/screen_terms/
  code.rs`, [dictionary.md](./dictionary.md)) give the files on screen and
  where the focus is. **Cursor** and **Windsurf**'s AI chat box: "@" + the
  name typed, Tab once the list shows that file; **VS Code**'s chat (Copilot
  Chat): "#" + the name ("@" is for its chat participants); **Claude Code**
  and **Codex** in a terminal: "@" + the path the screen showed
  ("@src/services/userService.ts"; a file shown with several paths or none
  keeps its name) written into the one paste — both read a file named so
  on send. Never in the code editor, a plain shell or Visual Studio /
  JetBrains (`tagging`). `find` (pure): a file's whole name as a word of
  its own (not inside a path), optionally after "@", "tag" or a
  clause-starting "at" (taken off); its stem alone only after one of
  those, and only when one file has it.
- **`pasting/compose.rs`** — **a dictation in steps** (2026-10-29): stacked
  messages, a picked mention, a picked file — `plan` (pure: each message's
  `Piece`s, `Text` to paste and `Pick { typed, said }`; the text it reads
  as, for history) and `run`, on `text_injector`'s public paste
  (`inject_text`, verified and private; `strict` after the first piece:
  into its window or nowhere), typing (`type_text`), keys
  (`press_step_key`: Tab, Backspace) and `press_submit`. **The whole text
  is ready first** (cleaned, corrected, fitted). **It stops at the first
  doubt**: before each message after the first the box must have emptied
  (`selection::read_context`: the last one was sent) and the window be in
  front; a paste, the typing or a key Windows didn't take ends it too.
  What's left goes on the clipboard privately and one alert says how far
  it got ("Sent 1 of 3 messages · the rest is copied", `stopped_alert`,
  kind "pasteBlocked"). After a picked mention the app's own space goes
  before a comma or a full stop (one Backspace, only when the field's text
  before the caret ends with a space). The log has counts and steps, never
  words. A plan of one plain paste goes the ordinary way (`in_steps`),
  so a dictation that needs no steps pastes exactly as before. Test mode
  pastes, types and presses nothing and reports the steps
  (`Delivered.steps`); `e2e_chat_target` (debug) stages the window: its
  process and title, the text on screen, the focus's labels, and whether
  an app's list shows the one typed.
- **`pasting/send_key.rs`** — **the Send shortcut** (2026-10-29; Wispr
  Flow's bindable "Press Enter"; `config.send_hotkey`, Settings →
  Shortcuts, unbound by default; `input_hook.rs`'s eleventh binding, its
  press only): once its keys are up, the send key in the window in front —
  `auto_submit_key` (Enter, Ctrl+Enter, Shift+Enter), Enter in a terminal
  (`app_kind::submit_key`). Never Enter itself (`is_enter`: the hook hears
  Yap's own keys; first or "+ Add another", refused in Settings and left
  unbound by `input_hook`), never another shortcut's key. An app running
  as administrator: said once. No in-page fallback, as Paste last has none.
  Test mode presses nothing ("e2e: not pressing the send key"; debug
  `e2e_shortcut { name: "send" }`).
- **`pasting/elevated.rs`** — apps running as administrator: Windows (UIPI) drops keys
  sent to a higher integrity level, and `SendInput` doesn't say so. The target's
  level is compared with Yap's (`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
  → its token → `TokenIntegrityLevel`; a token Windows won't show plus a refused
  `PROCESS_QUERY_INFORMATION` = above Yap; anything unknown pastes as before);
  pure `blocks_input`, unit-tested (and against Yap's own token). Above Yap →
  the text stays on the clipboard (`text_injector`) and the dictation, the
  rewrite or "Paste last transcript" shows "Your text is copied: press Ctrl+V"
  (`alert()`, through `notices/alert.rs`) — no submit key, no correction watch;
  history keeps it. Test mode reads no other process: debug-only
  `e2e_paste_elevated { on }` makes every target count as one (plain test
  mode just says so; robot mode really leaves the text on the clipboard). ⚠ A
  real elevated window can't be robot-tested: opening one takes a UAC prompt.
- **`pasting/selection.rs`** — UI Automation reads of the field Yap types into. **After
  the paste** (`WatchedField`, for `dictionary/learn.rs`): the focused element held on
  the watch's thread, read whether it still has the focus or not;
  `read(limit)` → a `FieldWindow` of the **whole field** (TextPattern's
  document range, else the ValuePattern value; line breaks as `\n`, the
  caret's offset in it, an empty field empty (the stand-in rules below);
  longer than `limit` → its start with `to_end` false); same
  refusals (other processes, passwords, read-only, test runs), 300 ms
  timeouts. **Edit
  mode** (`capture_selection(target, copy_fallback)`): the selected text
  (TextPattern, else the Ctrl+C trick — never in a terminal with terminal mode
  on, where Ctrl+C would interrupt the program), 2 s UIA timeouts. **Read
  aloud** (`read_selected(target, copy_fallback)` → `Selected {text, via}`,
  for `voice/read_aloud.rs`): the focus must be in the target's process and not a
  password box; then UI Automation's TextPattern selection of the focused
  element or the first of up to 12 parents that has a TextPattern (a web
  page's document when the focus is on a link; **read-only text too**: a
  page, a PDF, a viewer — it's only read), several ranges (Word) joined
  (`uia::selection_in`, `via` "uia"); else a Win32 edit control's own
  selection cut from its ValuePattern text where `EM_GETSEL` says
  (`uia::edit_selection_in`, "edit": a WinForms TextBox has no TextPattern);
  else, only with `copy_fallback` — which read aloud never gives in a
  terminal, whatever terminal mode says — the Ctrl+C trick ("copy"). Test
  mode never; robot mode only in the test's own windows. Lengths and `via`
  at `debug`, never the text. **A transform's selection**
  (`editable_selection(target, copy_fallback)` → `FieldSelection` `Text` |
  `ReadOnly` | `Nothing` | `Unreadable`, for `dictation/transforms.rs`): only a
  field that takes typing — its selection is what a paste replaces — the
  focus in the target's process, never a password box (`ReadOnly`); the
  field the focus is in (`field_for`, up to 3 parents), refused when it's
  read-only (`offers`' ValuePattern / legacy state); its TextPattern
  selection — one range (Word's several: nothing), itself not read-only
  (`UIA_IsReadOnlyAttributeId`), the text as the field has it (spaces at
  its ends kept, for `keep_spaces`) — else a Win32 edit control's own
  selection (`value_selection`'s `EM_GETSEL`); `Unreadable` only when
  neither answers, and only then Ctrl+C (`copy_fallback`; a transform never
  asks in a terminal). Read again before the result goes over it
  (`transforms::verify_selection`). **The text around the cursor** (`begin_dictation(target,
  read)` from `start_recording`, `take_context()` at paste; `peek_before()`, 2026-10-25,
  gives AI cleanup the text before the caret without taking it — `ai/cleanup_context.rs`,
  [ai-models.md](./ai-models.md)): when `smart_spacing` is
  on and it isn't edit mode, a background thread reads ≤ 500 characters before and
  after the caret and the selection (a long selection as its start + end) —
  TextPattern (`GetSelection`, else TextPattern2's caret), else ValuePattern for
  Edit/Document/ComboBox fields — cut at word boundaries,
  with 200 ms UIA connection/transaction timeouts (`CUIAutomation8`). A
  ValuePattern has no caret: when the element's own window is a Win32 edit
  control (`edit_class`: `Edit`, `RichEdit20W`/`RICHEDIT50W`…, WinForms'
  `WindowsForms10.EDIT…`/`…RichEdit20W…` — a WinForms TextBox/RichTextBox has no
  TextPattern) the control is asked (`EM_GETSEL` via `SendMessageTimeoutW`, no
  pointers, `SMTO_ABORTIFHUNG`, the same time box; positions past 65,535 → unknown)
  and its UTF-16 units placed in the value (`units_to_bytes`: an EDIT's `\r\n` is
  two, a RichEdit's break one; a RichEdit's final paragraph mark dropped); any
  other value-only field (older Chromium/Electron, Firefox through the MSAA proxy)
  → caret taken to be at the end, where it is after typing. The watch's read finds
  its caret the same way. **Only the field's own text**: every range is kept inside the focused element's
  `DocumentRange` (`keep_within`; Chromium's ranges are page positions and ran on
  past a text box into the page), a selection outside it isn't read. **An empty
  field reads as empty** (`stands_in_for_empty`, pure): Chromium (Edge/Chrome/
  Electron/WebView2) gives an empty textarea/input/contenteditable an empty
  ValuePattern value but its accessible name ("Type a message") or U+FFFC as its
  TextPattern text — so a field's whole text (≤ 200 chars) that's only markers
  (U+FFFC, zero-widths)/whitespace, its placeholder (HelpText), or its label —
  its Name or the name of the element labelling it (LabeledBy) — unless
  the ValuePattern says those words are really there (Slate's in-editor
  placeholder: same words + U+FEFF) is empty; so is a web editor whose class says
  it shows a CSS placeholder, itself or its one line (`class_says_empty`: Quill's
  `ql-blank` = Slack, `is-empty`/`is-editor-empty` = ProseMirror/Tiptap,
  `ck-placeholder`, `placeholder`); a value-only field by its placeholder,
  markers or — an edit or a document, not a combo box — exactly its label. The
  post-paste watch applies the same rules. **A focused element with no text of
  its own** (an editor's inner part) is read through the text field it's part
  of: `field_for` walks up to 3 parents (`ControlViewWalker`), in the same
  process, never past a read-only element (`pick_field`, pure); edit mode's
  selection and the post-paste watch walk the same way. **Read-only** = the
  ValuePattern's flag **or** the legacy IAccessible state's
  `STATE_SYSTEM_READONLY` (`legacy_read_only`). Never: another
  process's element than the target window's, a password box, read-only text, or in
  a test run. The paste
  never waits: still reading → no context. Kept in memory for that dictation only;
  logged as lengths, never text. **`focused_element(pid)`** (for `app_kind`):
  the focused element's ClassName and Name in one round trip (a UIA cache
  request, 100 ms timeout), only if it's in `pid`. **`picked_option(pid)`**
  (for `compose.rs`, 2026-10-29): the same round trip's name when the focus
  a screen reader sees is a list's entry (a list item, menu item, data item
  or tree item: an app's "@" list's highlighted entry, which Chromium reports
  as the focus through `aria-activedescendant` while the keyboard stays in
  the box). **`focus_editable(target)`**
  (for the paste, 2026-10-25): the focused element and up to 3 parents
  (`FocusFacts`: control type, framework, TextPattern / ValuePattern and
  whether read-only, the legacy read-only bit; 200 ms) → `editable` (pure,
  unit-tested): **Yes** when it or a parent in the same app takes text (a
  TextPattern or a ValuePattern not read-only — a spreadsheet's cell —, an
  edit control); **No** only when a framework that shows fields plainly
  (Win32, WinForm, WPF, XAML, DirectUI, Chrome, Gecko) says the focus is on
  a control that never takes text (a button, a list item, a tab, a menu
  item, a link…) or on read-only text (a web page with no box focused, a
  mail being read); else **Unknown** (a pane, a custom control, Java, Qt,
  the focus in another app, a timeout). Test mode never reads; robot mode
  only the test's windows. **The exact text box** (2026-10-29; Wispr
  Flow's element-level refocus, by behaviour): `keep_focus(target)` (from
  `capture_target`, for a dictation that's pasted — a rewrite too) starts
  a "focus-keeper" thread holding UI Automation's element with the focus
  in the target's process (one look; COM objects stay on their thread;
  the previous dictation's goes) and whether it takes text (`editable`).
  At the delivery, before AI cleanup and the fit, `follow_focus()` asks
  it where the focus is now (`look`: `CompareElements`, then `editable`
  of the new one → `FocusNow` Same / OtherBox / NoBox / Unknown /
  Elsewhere); `refocus(started_in_text, now)` (pure, unit-tested):
  another text box → **Follow** — that's the dictation's box now, and the
  text around its cursor is read afresh (`read_context`, when smart
  spacing reads it at all; a read of the old box still running never
  lands over it) for smart spacing and cleanup's context; something that
  takes no text after starting in a box → **PutBack**, but only at the
  paste; anything else → **Stay**. At the paste (`restore_focus(target)`,
  `inject_text_sync` after `focus_window`, once: the paste takes the
  keeper; only a keeper for that window, ≤ 20 min old): PutBack → the
  element, if it's still there, enabled and on screen, gets
  `SetFocus()` and must then have the focus (`CouldNot` otherwise: the
  paste goes on, "Select a text box first" can still say so); another box
  now → left there. Another app → left: the window rules come first
  (today's: the dictation's window is brought back). Each answer waited
  for ≤ 0.9 s (200 ms UIA timeouts). Logged without content; the robot's
  report says `refocus`. Robot: `paste-refocus.real.js` (`dictateMoving`:
  `realtest_dictate_text { pauseMs }` with the focus moved meanwhile — a
  button, another box fitted to its own text, nothing, an Edge page's
  button, another window). **`PasteCheck`** (for
  `text_injector`'s verify-after-paste): the focused control of the target's
  thread (`GetGUIThreadInfo`), same process, an `edit_class` control without
  `ES_PASSWORD`/`ES_READONLY`; its state = `WM_GETTEXTLENGTH` + `EM_GETSEL`
  (`SendMessageTimeoutW`, 200 ms, `SMTO_ABORTIFHUNG`), never its text;
  `paste_verdict` (pure, unit-tested) says landed / didn't / unknown.
  Its COM set-up (`uia::Com`, `uia::automation` with timeouts) is shared
  with `dictionary/screen_terms.rs`, which reads the target window's visible text for
  its names at the same moment (same refusals; see
  [dictionary.md](./dictionary.md)).
  `#[ignore]`d `reads_real_text_fields` reads hidden
  EDIT and RichEdit controls end to end, through TextPattern and through the
  ValuePattern + `EM_GETSEL` path (mid-text, a selection, over a line break),
  the paste check's state before and after an `EM_REPLACESEL`, and from a
  child window inside the field (read through it; not through a read-only one).
- **`pasting/smart_insert.rs`** — `fit_to_context(text, &FieldContext, names)`, pure and
  unit-tested (conservative: a wrong fix is worse than the old capital). Mid-sentence
  (before ends in a letter/digit/`,`/`;`/spaced dash — not `.?!`, `:`, a new line or
  a bullet) → lower-case the first letter only for a plainly sentence-case word
  (never "I"/"I'm", all-caps, "iPhone", a dictionary `to` term, or a word written
  capitalised mid-sentence nearby) that's a known ordinary word (`COMMON_WORDS`:
  English + common fr/de/es/it/pt/nl function words, no names like Will/May, no
  "El"/"Las"/"Los") or that the surrounding text writes in lower case — so names keep
  their capital. Text after the caret starting lower-case or with `,.;:!?)…` → drop
  one final `.`. Leading space only after a letter/digit/closing punctuation (a
  straight quote after a word, a spaced dash), never after whitespace, an opening
  bracket/quote or at the field start; trailing space only before a letter/digit;
  none next to Chinese/Japanese (`is_cjk`; Hangul uses spaces). Replacing a
  selection: its first word's case (same safety rules, or the same word again), its
  end punctuation, and any space it began/ended with. Edit mode and Commands
  by name paste the AI's text untouched, as before.
- **`pasting/app_kind.rs`** — **what kind of app a dictation goes into**, and what that
  changes about the paste (Wispr Flow's app categories + terminal/coding-CLI
  handling + chat full stops, by behaviour). `of_window(hwnd)` (at record start
  beside `target_hwnd`, → `Shared.target_kind`; also "Paste last transcript"):
  the window's process name (`text_injector::app_name_for`), class and title
  (`GetWindowTextW`, which never messages another process's window) — no UIA;
  logs the process + `describe()` at `debug`, never the title; a test run reads
  nothing (`Other`). Pure `classify(exe, class, title)` → **`Terminal { coding_cli,
  host }`** (`terminal_host`, by class first, whatever process it reports, then
  process: `WindowsTerminal` — `CASCADIA_HOSTING_WINDOW_CLASS` —, `Console` —
  conhost's `ConsoleWindowClass`, cmd, PowerShell/pwsh, and terminals Yap knows
  by name only (Warp, Extraterm…) —, `Mintty` (Git Bash, MSYS2, Cygwin), `Putty`
  (PuTTY, KiTTY), `MobaXterm`, `Alacritty`, `WezTerm`, `Tabby`, `Hyper`, `Rio`,
  `ConEmu` (Cmder), `Editor`) | **`Messaging`** (Slack, Discord, WhatsApp,
  Telegram, Signal, Teams, Element, Mattermost…; or a browser whose title shows
  WhatsApp Web, Slack, Discord, Messenger, Microsoft Teams, Telegram, Google
  Chat/Messages — unread counts and Edge's "and 2 more pages" stripped) |
  **`Other`** (incl. Claude desktop). **An editor's integrated terminal**: in
  VS Code and the editors built on it (`EDITOR_EXES`: Code, Code - Insiders,
  VSCodium, Cursor, Windsurf, Positron, Trae, Kiro, Antigravity, Azure Data
  Studio) the window is the editor's whatever has the focus, so `of_window`
  asks `selection::focused_element` (one UIA round trip, 100 ms) and
  `editor_terminal(class, name)` (pure) makes it a terminal (`Host::Editor`)
  when the focus is xterm.js's input — class `xterm-helper-textarea` — with
  the coding CLI from VS Code's label "Terminal 1, <title>" (first line; the
  number and title survive translation: "ターミナル 1、claude"; Chromium folds
  the label's line breaks, so the help text often follows the title on its
  line — a title the whole line doesn't name is tried by its first word). One line, no
  Enter of Yap's own, no cursor read, no Ctrl+C in edit mode; never
  `keeps_lines`. Robot mode can name stand-in editors
  (`realtest::stand_in_editors`, `YAP_REALTEST_EDITORS`). **The paste keys**
  (`AppKind::paste_key`, asked of the kind as read — "Safer pasting" off still
  needs them; `Host::paste_key`, each from the terminal's own defaults):
  Shift+Insert in mintty (`ClipShortcuts`; Ctrl+Shift+V needs
  `CtrlShiftShortcuts`, off), PuTTY/KiTTY and MobaXterm; Ctrl+Shift+V in
  Alacritty (its `Paste`; Shift+Insert is `PasteSelection`), WezTerm
  (`PasteFrom "Clipboard"`; Shift+Insert is the primary selection), Tabby,
  Hyper (`editor:paste`) and Rio; Ctrl+V in Windows Terminal, ConEmu/Cmder
  (Shift+Ins pastes only the first line there), VS Code's terminal, a
  browser's and Visual Studio's, and anything unknown; a classic console
  window its own Edit → Paste (`PasteKey::ConsoleMenu`, no keys: a raw
  keyboard program there gets Ctrl+V as ^V; Ctrl+V when the window turns
  out not to be a console). **More terminals** (2026-10-25): **a browser's
  terminal** (`Host::Browser`) — a browser window whose title names a page
  that may hold one (`web_terminal_page`: "Visual Studio Code", vscode.dev,
  github.dev, Codespaces, code-server, Gitpod, Jupyter, Colab, a cloud shell,
  Replit, StackBlitz, CodeSandbox, "terminal", "shell", "ssh", "console") is
  asked, as an editor is, whether xterm.js's input has the focus, with the
  coding CLI from VS Code's label; only then (one UIA call, 100 ms).
  **Visual Studio's terminal** (`Host::VisualStudio`) — Windows Terminal's
  own control as `Microsoft.Terminal.Wpf` hosts it: the focused window's
  class `HwndTerminalClass` (`terminal_control`, Win32 `GetGUIThreadInfo`,
  no UIA; also ConEmu's `VirtualConsoleClass` inside another app). Untried
  in a real Visual Studio (not installed here). **JetBrains IDEs' terminal
  isn't told apart**: Swing draws the editor and the terminal in one window
  and shows UI Automation nothing (Java Access Bridge isn't UIA), so a
  JetBrains IDE stays an ordinary app — Ctrl+V pastes in both, but lines
  aren't joined and edit mode's Ctrl+C trick isn't held back there.
  **Coding CLIs** from the terminal's title, as they set it on Windows (2026):
  Claude Code "✳ <session>" idle / "◐"/"◑" working / "Claude Code" / "claude";
  Gemini CLI "◇  Ready (…)", "✦  …", "✋  …", "⏲  …", "Gemini CLI (…)"; Qwen Code;
  Codex a braille spinner + project while working (idle it's only the project:
  undetected → treated as a plain shell, the safe side); OpenCode "OpenCode" /
  "OC | …"; else the program as the title or cmd's "<title> - <command>" (aider,
  codex, gemini, copilot, cursor-agent, crush…; a folder path never counts).
  `with_settings(terminal_mode, chat_no_full_stop)` turns a kind off.
  **`terminal_text(text, keep_lines)`**: `\r\n`/`\r`/U+2028… → `\n`, tabs → a
  space, control characters out, trimmed; unless `keep_lines` one line
  (`join_lines`: bullets "- • * –" dropped and items joined "; " — "Fix these:
  the login bug; the search" —, a header without punctuation gets ": ", numbers
  kept, other lines joined with a space). `Terminal::keeps_lines` = `Host::WindowsTerminal`
  + Claude Code or Codex (bracketed paste / Codex's paste-burst check);
  every other CLI and host is joined until known safe. **`submit_key(kind,
  auto_submit, voice_enter, key)`**: in a terminal "enter", and only for the
  voice command or auto-submit in a detected coding CLI (never auto-submit at a
  plain shell's prompt); elsewhere as configured. **`drop_chat_full_stop(text,
  ctx, dictionary)`**: a final "." off a message of ≤ 2 sentences in one
  paragraph (`sentence_count`: abbreviations — "e.g.", "a.m.", "Dr.", "etc." —
  and initials don't end one), only at the end of the draft (nothing after the
  cursor, no selection being replaced, the draft before it counted, no line
  break in it); never "?", "!", "…", "...", an abbreviation's own full stop or a
  snippet saved ending with one. In a terminal with terminal mode on, the text
  around the cursor isn't read, edit mode never sends Ctrl+C
  (`selection::capture_selection(target, false)`) and the rewrite / Command
  reply goes through `terminal_text` too. An `#[ignore]`d `this_machines_windows`
  classifies the PC's real windows (`YAP_APP_KIND_TITLES=1` prints titles).
