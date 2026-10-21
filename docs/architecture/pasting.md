# Pasting: where the text goes

Putting a dictation into the app you're in: the clipboard paste and the submit key, apps running as administrator, UI Automation reads of the field, fitting the text to what's around the cursor, and the kind of app (terminals, coding CLIs, chat apps). The pipeline that calls all this is [dictation.md](./dictation.md).

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
  ("e2e: not pasting the last transcript"). No hotkey for it yet. (Verify-after-paste
  over UIA is still a follow-up.) `#[ignore]`d `private_clipboard_round_trip` runs
  the real clipboard code on a private window station (never the person's clipboard).
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
  on, where Ctrl+C would interrupt the program), 2 s UIA timeouts. **The text around the cursor** (`begin_dictation(target,
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
  logged as lengths, never text. `#[ignore]`d `reads_real_text_fields` reads hidden
  EDIT and RichEdit controls end to end, through TextPattern and through the
  ValuePattern + `EM_GETSEL` path (mid-text, a selection, over a line break),
  and from a child window inside the field (read through it; not through a
  read-only one).
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
  windows_terminal }`** (Windows Terminal — `CASCADIA_HOSTING_WINDOW_CLASS` —,
  conhost's `ConsoleWindowClass` whatever process it reports, cmd, PowerShell/
  pwsh, WezTerm, Alacritty, mintty/Git Bash, Tabby, Hyper, ConEmu, PuTTY/KiTTY,
  Warp…) | **`Messaging`** (Slack, Discord, WhatsApp, Telegram, Signal, Teams,
  Element, Mattermost…; or a browser whose title shows WhatsApp Web, Slack,
  Discord, Messenger, Microsoft Teams, Telegram, Google Chat/Messages — unread
  counts and Edge's "and 2 more pages" stripped) | **`Other`** (incl. Claude
  desktop, VS Code — its integrated terminal can't be told apart yet).
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
  kept, other lines joined with a space). `Terminal::keeps_lines` = Windows
  Terminal + Claude Code or Codex (bracketed paste / Codex's paste-burst check);
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
