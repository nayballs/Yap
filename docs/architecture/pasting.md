# Pasting: where the text goes

Putting a dictation into the app you're in: the clipboard paste and the submit key, apps running as administrator, UI Automation reads of the field, fitting the text to what's around the cursor, the kind of app (terminals, coding CLIs, chat apps), and the last paste afterwards (swapped for the words as said, or rewritten). The pipeline that calls all this is [dictation.md](./dictation.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`. **An app
  running as administrator** (`elevated.rs`, checked first — the paste's target,
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
  remembers the dictation and fits it to the kind of app (`kind`, `app_kind.rs`,
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
  `pipeline::as_said_text`). `copy_privately` puts text on the clipboard
  as a paste would and leaves it (Use what I said's refusals).
  `app_window()` / `focus(hwnd)`: the same window-finding and re-focusing
  for the tray's "Read selection aloud" (`read_aloud.rs`). **The paste
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
  moves the caret). The clipboard restore still waits its 500 ms from the
  keys. `#[ignore]`d `private_clipboard_round_trip` runs
  the real clipboard code on a private window station (never the person's clipboard).
- **`as_said.rs`** — **"Use what I said"** (2026-10-22; Windows' fluid
  dictation's Revert, TypeWhisper's Restore Raw Transcript): right after a
  dictation AI cleanup changed, swap what Yap pasted for the words as said
  (the transcript with corrections, the voice command and snippets, no
  cleanup — `pipeline::as_said_text` — fitted to the same place). `deliver`
  `remember`s the paste (window, kind, both texts) unless a submit key sent
  it; three ways in: the shortcut (`as_said_hotkey`, unbound by default —
  `input_hook`'s sixth binding, `as-said-key-pressed`), the Yap bar's
  "Cleaned up · Use what I said" for `OFFER_MS` (6 s; `as_said_offer`, on;
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
  here — copied the raw text instead" (an `alert.rs` "pasteBlocked" alert,
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
  `as-said.real.js`.
- **`elevated.rs`** — apps running as administrator: Windows (UIPI) drops keys
  sent to a higher integrity level, and `SendInput` doesn't say so. The target's
  level is compared with Yap's (`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
  → its token → `TokenIntegrityLevel`; a token Windows won't show plus a refused
  `PROCESS_QUERY_INFORMATION` = above Yap; anything unknown pastes as before);
  pure `blocks_input`, unit-tested (and against Yap's own token). Above Yap →
  the text stays on the clipboard (`text_injector`) and the dictation, the
  rewrite or "Paste last transcript" shows "Your text is copied: press Ctrl+V"
  (`alert()`, through `alert.rs`) — no submit key, no correction watch;
  history keeps it. Test mode reads no other process: debug-only
  `e2e_paste_elevated { on }` makes every target count as one (plain test
  mode just says so; robot mode really leaves the text on the clipboard). ⚠ A
  real elevated window can't be robot-tested: opening one takes a UAC prompt.
- **`selection.rs`** — UI Automation reads of the field Yap types into. **After
  the paste** (`WatchedField`, for `learn.rs`): the focused element held on
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
  for `read_aloud.rs`): the focus must be in the target's process and not a
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
  at `debug`, never the text. **The text around the cursor** (`begin_dictation(target,
  read)` from `start_recording`, `take_context()` at paste): when `smart_spacing` is
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
  request, 100 ms timeout), only if it's in `pid`. **`PasteCheck`** (for
  `text_injector`'s verify-after-paste): the focused control of the target's
  thread (`GetGUIThreadInfo`), same process, an `edit_class` control without
  `ES_PASSWORD`/`ES_READONLY`; its state = `WM_GETTEXTLENGTH` + `EM_GETSEL`
  (`SendMessageTimeoutW`, 200 ms, `SMTO_ABORTIFHUNG`), never its text;
  `paste_verdict` (pure, unit-tested) says landed / didn't / unknown.
  Its COM set-up (`uia::Com`, `uia::automation` with timeouts) is shared
  with `screen_terms.rs`, which reads the target window's visible text for
  its names at the same moment (same refusals; see
  [dictionary.md](./dictionary.md)).
  `#[ignore]`d `reads_real_text_fields` reads hidden
  EDIT and RichEdit controls end to end, through TextPattern and through the
  ValuePattern + `EM_GETSEL` path (mid-text, a selection, over a line break),
  the paste check's state before and after an `EM_REPLACESEL`, and from a
  child window inside the field (read through it; not through a read-only one).
- **`smart_insert.rs`** — `fit_to_context(text, &FieldContext, names)`, pure and
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
- **`app_kind.rs`** — **what kind of app a dictation goes into**, and what that
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
  Hyper (`editor:paste`) and Rio; Ctrl+V in Windows Terminal, a console
  ("Enable Ctrl key shortcuts"), ConEmu/Cmder (Shift+Ins pastes only the
  first line there), VS Code's terminal and anything unknown.
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
