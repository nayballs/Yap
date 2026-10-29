//! What's in the field Yap is about to type into, read over **UI Automation**.
//!
//! Two jobs:
//!
//! 1. **Edit/rewrite mode** ([`capture_selection`]): when the user triggers the
//!    edit hotkey, Yap grabs whatever text is selected in the foreground app
//!    *before* recording, so the spoken instruction can be applied to it. Two
//!    tiers (ported in spirit from FluidVoice, which reads the macOS
//!    Accessibility `AXSelectedText`): UIA `TextPattern` (clean, works in most
//!    native + WinUI apps), then the clipboard Ctrl+C trick
//!    (`text_injector::selection_via_copy`) for apps whose UIA text support is
//!    patchy (Electron/Chromium, terminals). Empty selection from both tiers →
//!    `None`, which the pipeline treats as "write mode".
//! 2. **The text around the cursor** for a dictation ([`begin_dictation`] /
//!    [`take_context`], `config.smart_spacing`): up to 500 characters before
//!    and after the caret, and any selection, read in the background at record
//!    start (`TextPattern`, else `ValuePattern` — with the caret asked of a
//!    Win32 edit control such as WinForms'), so `smart_insert` can fit the
//!    dictation to them. Only the focused field's own text: Chromium's
//!    ranges run on into the page around a text box, so they're kept inside
//!    the field's document range; and an empty field reads as empty,
//!    whatever stands in for its text (its label — its Name or the element
//!    that labels it, LabeledBy —, its placeholder, U+FFFC, a web editor's
//!    CSS placeholder — [`stands_in_for_empty`], [`class_says_empty`]). A
//!    focused element with no text of its own (a part of an editor) is read
//!    through the text field it's in, a few levels up in the same app
//!    ([`pick_field`]); a field read-only by its ValuePattern **or** by the
//!    legacy IAccessible state (`STATE_SYSTEM_READONLY`) is never read. The
//!    paste never waits for it: a read that hasn't
//!    finished by then (or failed, or found a password box) leaves the
//!    dictation exactly as it would be without it. The text is kept in memory
//!    for that one dictation only and never logged (lengths only).
//! 3. **The field after the paste** ([`WatchedField`], for `learn.rs`): the
//!    element a dictation was just pasted into, held for about a minute and
//!    read whole (up to 15,000 characters, focused or not) to see whether a
//!    word Yap typed gets corrected. Same rules: that window's process only,
//!    never a password box or a read-only field, never in a test run,
//!    time-boxed calls, in memory only.
//! 4. **Did the paste land?** ([`PasteCheck`], for `text_injector`): a Win32
//!    edit control's text length and selection just before the paste and
//!    after it (Win32 messages, not UI Automation; never its text).
//!
//! Also [`focused_element`]: what has the focus (its class and name), for
//! `app_kind` to tell an editor's integrated terminal from the editor.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::pasting::smart_insert::FieldContext;

/// How long one UI Automation call may take for the cursor context before
/// it's given up on (connection and transaction timeouts): an app that's
/// hung or slow never holds a dictation up.
const CONTEXT_TIMEOUT_MS: u32 = 200;
/// The same for edit mode's selection, which the pipeline waits for. Long
/// enough for a big selection in Word, far below UIA's own 20 s.
const SELECTION_TIMEOUT_MS: u32 = 2_000;
/// The same for the post-paste watch's reads (a thread of its own).
const WATCH_TIMEOUT_MS: u32 = 300;
/// The same for the look at what has the focus at record start
/// ([`focused_element`]: is it an editor's terminal?), which the recording
/// waits for: one round trip, cut short.
const PROBE_TIMEOUT_MS: u32 = 100;
/// The same for the check that a paste landed ([`PasteCheck`]), which the
/// paste waits for.
const CHECK_TIMEOUT_MS: u32 = 200;

/// A field whose whole text is longer than this holds real text: a label or
/// a placeholder standing in for an empty field is never this long.
#[cfg_attr(not(windows), allow(dead_code))]
const STAND_IN_CHARS: usize = 200;

/// Characters that take a place in a field's text without anything being
/// written there: U+FFFC (Chromium's text for an empty text box, or an
/// embedded object) and the zero-width ones (Slate's U+FEFF in an empty
/// line).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_marker(c: char) -> bool {
    matches!(c, '\u{FFFC}' | '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}')
}

/// `s` as it reads: no markers, whitespace runs as one space, trimmed.
#[cfg_attr(not(windows), allow(dead_code))]
fn bare(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || is_marker(c))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Is a field whose whole text over UI Automation is `text` really empty?
///
/// Chromium (Edge, Chrome, every Electron and WebView2 app) reads an empty
/// text box as holding its accessible name ("Type a message", from
/// `aria-label`) or U+FFFC when it has none; a rich editor's empty line is a
/// line break and a zero-width character, and an editor that draws its
/// placeholder inside itself (Slate) has it in its text. So: nothing but
/// markers and whitespace, or exactly the field's `placeholder` (HelpText,
/// shown only while it's empty), is empty. Exactly one of its `labels` (its
/// Name, and the name of the element labelling it — LabeledBy —, which
/// someone could also type) is empty unless the field's `value`
/// (ValuePattern, asked only then; `None` = it has none) says those words
/// are really there: an empty Chromium box's value is "", and a value of
/// the same words with a marker in the text is Slate's placeholder.
#[cfg_attr(not(windows), allow(dead_code))]
fn stands_in_for_empty(text: &str, labels: &[String], placeholder: &str, value: impl FnOnce() -> Option<String>) -> bool {
    let shown = bare(text);
    if shown.is_empty() {
        return true;
    }
    let same = |label: &str| {
        let label = bare(label);
        !label.is_empty() && label == shown
    };
    if same(placeholder) {
        return true;
    }
    if !labels.iter().any(|l| same(l)) {
        return false;
    }
    match value() {
        None => true,
        Some(v) => bare(&v).is_empty() || text.contains(is_marker),
    }
}

/// How far above the focused element Yap looks for the text field it's
/// part of (an editor's inner element with no text of its own).
const PARENT_LEVELS: usize = 3;

/// The legacy IAccessible state bit for read-only (`STATE_SYSTEM_READONLY`).
#[cfg_attr(not(windows), allow(dead_code))]
const STATE_SYSTEM_READONLY: u32 = 0x40;

/// A legacy IAccessible state (`LegacyIAccessiblePattern.CurrentState`)
/// saying the element is read-only.
#[cfg_attr(not(windows), allow(dead_code))]
fn legacy_read_only(state: u32) -> bool {
    state & STATE_SYSTEM_READONLY != 0
}

/// What an element on the way up from the focus offers ([`pick_field`]).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Offers {
    /// Its process.
    pid: u32,
    /// A TextPattern.
    text: bool,
    /// A ValuePattern on a text control (an edit, a document, a combo box).
    value: bool,
    /// Read-only, by its ValuePattern or its legacy IAccessible state.
    read_only: bool,
}

/// Which element of `chain` (the focused one first, then its parents) is
/// the text field to read: the first that offers text (a TextPattern or a
/// text control's ValuePattern) — the focused element itself, or one of its
/// first [`PARENT_LEVELS`] parents — unless it's read-only. Never one in
/// another process than the focused element's, and never past a read-only
/// element (a viewer's part is no field's).
#[cfg_attr(not(windows), allow(dead_code))]
fn pick_field(chain: &[Offers]) -> Option<usize> {
    let pid = chain.first()?.pid;
    for (i, e) in chain.iter().enumerate().take(PARENT_LEVELS + 1) {
        if e.pid != pid {
            return None;
        }
        if e.text || e.value {
            return (!e.read_only).then_some(i);
        }
        if e.read_only {
            return None;
        }
    }
    None
}

/// A web editor's class list saying the element shows no text of its own
/// (its text is a CSS placeholder, `::before { content: attr(…) }`, which
/// Chromium reads as text): Quill's `ql-blank` (Slack's message box),
/// ProseMirror/Tiptap's `is-empty` / `is-editor-empty` (Claude, ChatGPT),
/// CKEditor's `ck-placeholder` (Teams), and a plain `placeholder`.
#[cfg_attr(not(windows), allow(dead_code))]
fn class_says_empty(class: &str) -> bool {
    const EMPTY: &[&str] = &["ql-blank", "is-empty", "is-editor-empty", "ck-placeholder", "placeholder"];
    class.split_whitespace().any(|token| EMPTY.contains(&token))
}

/// Capture the current selection for edit mode: UIA first, Ctrl+C fallback
/// when `copy_fallback` (not in a terminal, where Ctrl+C with nothing
/// selected interrupts the program running there).
///
/// `target_hwnd` is the foreground window captured at hotkey-press (used to
/// aim the Ctrl+C fallback). Returns `None` if nothing is selected.
pub fn capture_selection(target_hwnd: Option<isize>, copy_fallback: bool) -> Option<String> {
    // A test run reads no other app's selection; robot mode only its own
    // windows' (as every other read here).
    if crate::shell::e2e::active() && !crate::shell::realtest::permits(target_hwnd) {
        tracing::debug!("e2e: not reading another app's selection (edit mode → no selection)");
        return None;
    }
    if let Some(text) = uia_selected_text() {
        let t = text.trim();
        if !t.is_empty() {
            tracing::debug!(len = t.len(), "Selection captured via UI Automation");
            return Some(t.to_string());
        }
    }
    if !copy_fallback {
        tracing::debug!("No selection found over UI Automation; no Ctrl+C in a terminal (edit mode → write mode)");
        return None;
    }
    match crate::pasting::text_injector::selection_via_copy(target_hwnd) {
        Some(t) => {
            tracing::debug!(len = t.len(), "Selection captured via clipboard Ctrl+C");
            Some(t)
        }
        None => {
            tracing::debug!("No selection found (edit mode → write mode)");
            None
        }
    }
}

/// What's selected in an app, for reading aloud (`read_aloud.rs`), and how
/// it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub text: Option<String>,
    /// "uia" (UI Automation's TextPattern: documents, web pages, PDFs —
    /// read-only text too), "edit" (a Win32 edit control's own selection in
    /// its ValuePattern text: WinForms has no TextPattern), "copy" (the
    /// Ctrl+C trick), "" (nothing found).
    pub via: &'static str,
}

/// The text selected in `target_hwnd`'s app, for reading aloud: UI
/// Automation's TextPattern on the focused element or the document it's in
/// (read-only text too: a web page, a PDF, a viewer), else a Win32 edit
/// control's own selection, else — only with `copy_fallback`, never in a
/// terminal — Ctrl+C. Never another process's element than the target's,
/// never a password box; robot mode only in the test's own windows, test
/// mode never. The text is never logged.
pub fn read_selected(target_hwnd: Option<isize>, copy_fallback: bool) -> Selected {
    let nothing = Selected { text: None, via: "" };
    let Some(hwnd) = target_hwnd else { return nothing };
    if crate::shell::e2e::active() && !crate::shell::realtest::permits(Some(hwnd)) {
        return nothing;
    }
    let Some(pid) = crate::pasting::text_injector::window_process(hwnd) else { return nothing };
    #[cfg(windows)]
    {
        if let Some(text) = uia::selection_in(pid) {
            tracing::debug!(len = text.len(), "Read aloud: the selection, over UI Automation");
            return Selected { text: Some(text), via: "uia" };
        }
        if let Some(text) = uia::edit_selection_in(pid) {
            tracing::debug!(len = text.len(), "Read aloud: the selection, from the edit control");
            return Selected { text: Some(text), via: "edit" };
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
    if copy_fallback {
        if let Some(text) = crate::pasting::text_injector::selection_via_copy(Some(hwnd)).map(|t| t.trim().to_string()) {
            if !text.is_empty() {
                tracing::debug!(len = text.len(), "Read aloud: the selection, by Ctrl+C");
                return Selected { text: Some(text), via: "copy" };
            }
        }
    }
    nothing
}

/// What a **saved transform** (`transforms.rs`) finds selected in the app
/// in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSelection {
    /// Text selected in a field that takes typing — a paste replaces it —
    /// as the field has it (spaces at its ends kept).
    Text(String),
    /// A selection Yap mustn't change: read-only text (a web page, a
    /// viewer) or a password box.
    ReadOnly,
    /// Nothing selected (or the focus is in another app).
    Nothing,
    /// The field couldn't be read over UI Automation (then Ctrl+C may tell).
    Unreadable,
}

/// The selection in `target_hwnd`'s focused field, for a transform: UI
/// Automation's TextPattern selection of the field the focus is in (one
/// range; not when the field or the selection is read-only), else a Win32
/// edit control's own selection (`EM_GETSEL` in its ValuePattern text);
/// only where neither can read the field, and `copy_fallback`, Ctrl+C —
/// never in a terminal. Never another process's element, never a password
/// box; robot mode only in the test's own windows, test mode never. The
/// text is never logged.
pub fn editable_selection(target_hwnd: Option<isize>, copy_fallback: bool) -> FieldSelection {
    let Some(hwnd) = target_hwnd else { return FieldSelection::Nothing };
    if crate::shell::e2e::active() && !crate::shell::realtest::permits(Some(hwnd)) {
        tracing::debug!("e2e: not reading another app's selection (a transform)");
        return FieldSelection::Nothing;
    }
    let Some(pid) = crate::pasting::text_injector::window_process(hwnd) else { return FieldSelection::Nothing };
    #[cfg(windows)]
    let read = uia::editable_selection_in(pid);
    #[cfg(not(windows))]
    let read = {
        let _ = pid;
        FieldSelection::Unreadable
    };
    match read {
        FieldSelection::Unreadable if copy_fallback => match crate::pasting::text_injector::selection_via_copy(Some(hwnd)) {
            Some(t) if !t.trim().is_empty() => {
                tracing::debug!(len = t.len(), "Transform: the selection, by Ctrl+C");
                FieldSelection::Text(t)
            }
            _ => FieldSelection::Nothing,
        },
        other => {
            if let FieldSelection::Text(t) = &other {
                tracing::debug!(len = t.len(), "Transform: the selection, over UI Automation");
            }
            other
        }
    }
}

/// Where the current dictation's context read stands.
enum Reading {
    /// Nothing asked for (smart spacing off, edit mode, no target window).
    Off,
    /// Being read in the background.
    Pending,
    /// Read: `None` when the field couldn't be read.
    Done(Option<FieldContext>),
}

struct Session {
    id: u64,
    reading: Reading,
    /// The text around the cursor is read for this dictation (smart
    /// spacing on, not edit mode or a terminal): read afresh where the
    /// person moves to ([`follow_focus`]).
    read_wanted: bool,
}

static SESSION: Mutex<Session> = Mutex::new(Session {
    id: 0,
    reading: Reading::Off,
    read_wanted: false,
});
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

fn session() -> std::sync::MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// A dictation started recording into `target_hwnd` (the window captured at
/// record start; `None` = one of Yap's own). With `read`, the text around
/// the caret is read on a background thread for [`take_context`]; either
/// way the previous dictation's context is forgotten. Never in a test run
/// (it would read whatever app the developer has focused) — in robot mode
/// only a window the test opened (`realtest::permits`).
pub fn begin_dictation(target_hwnd: Option<isize>, read: bool) {
    let id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    let hwnd = target_hwnd.filter(|&h| read && crate::shell::realtest::permits(Some(h)));
    {
        let mut s = session();
        s.id = id;
        s.reading = if hwnd.is_some() { Reading::Pending } else { Reading::Off };
        s.read_wanted = hwnd.is_some();
    }
    let Some(hwnd) = hwnd else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("field-context".into())
        .spawn(move || {
            let ctx = read_context(Some(hwnd));
            let mut s = session();
            if s.id == id {
                s.reading = Reading::Done(ctx);
            }
        });
    if spawned.is_err() {
        session().reading = Reading::Off;
    }
}

/// Robot mode: wait (at most `max`) for the read [`begin_dictation`]
/// started, as the seconds of a spoken dictation would.
#[cfg(debug_assertions)]
pub fn wait_for_context(max: std::time::Duration) {
    let until = std::time::Instant::now() + max;
    while matches!(session().reading, Reading::Pending) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The context read for the dictation being pasted, if it's ready — the
/// paste never waits for it. Takes it, so it's used once.
pub fn take_context() -> Option<FieldContext> {
    let mut s = session();
    match std::mem::replace(&mut s.reading, Reading::Off) {
        Reading::Done(ctx) => ctx,
        Reading::Pending => {
            tracing::debug!("The text around the cursor is still being read; pasting without it");
            None
        }
        Reading::Off => None,
    }
}

/// The text before the caret read for the dictation now, if the read is
/// done — left for [`take_context`] (AI cleanup's context,
/// `ai/cleanup_context.rs`; never waited for).
pub fn peek_before() -> Option<String> {
    match &session().reading {
        Reading::Done(Some(ctx)) => Some(ctx.before.clone()),
        _ => None,
    }
}

/// Read the text around the caret in the focused field now (on this thread,
/// each UI Automation call time-boxed). `target_hwnd`: the field must be in
/// that window's process, or it isn't the one Yap will paste into. `None`
/// when the field can't be read, is a password box, or is read-only.
pub fn read_context(target_hwnd: Option<isize>) -> Option<FieldContext> {
    let started = std::time::Instant::now();
    // The window gone (closed since): nothing of its to read, and the
    // focused field now is someone else's.
    let pid = match target_hwnd.map(crate::pasting::text_injector::window_process) {
        Some(None) => {
            tracing::debug!("The dictation's window is gone; not reading the text around the cursor");
            return None;
        }
        Some(pid) => pid,
        None => None,
    };
    let ctx = read_focused_context(pid);
    let ms = started.elapsed().as_millis() as u64;
    match &ctx {
        Some(c) => tracing::debug!(
            ms,
            before = c.before.chars().count(),
            after = c.after.chars().count(),
            selection = c.selection.chars().count(),
            "Read the text around the cursor"
        ),
        None => tracing::debug!(ms, "Couldn't read the text around the cursor"),
    }
    ctx
}

/// A field's text as the post-paste watch reads it: line breaks as `\n`,
/// no-break spaces as spaces, an empty field empty (not the label or
/// placeholder that stands in for its text: [`stands_in_for_empty`]).
/// [`WatchedField::read`] reads the whole field: `from_start` is always
/// true there, and `to_end` false only when the field was longer than asked
/// (then `text` is its start).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldWindow {
    pub text: String,
    /// Byte offset of the caret (or the selection's start) in `text`; its
    /// end when the field has none Yap can read.
    pub caret: usize,
    /// `text` begins at the field's start / ends at its end.
    pub from_start: bool,
    pub to_end: bool,
}

/// The field a dictation was just pasted into, held for the correction
/// watch (`learn.rs`). COM: it lives and dies on the thread that opened it.
pub struct WatchedField {
    #[cfg(windows)]
    inner: uia::Watched,
}

impl WatchedField {
    /// The focused element now, if it's a text field Yap may read in
    /// `target_hwnd`'s process: not a password box, not read-only. Never in
    /// a test run — in robot mode only in a window the test opened
    /// (`realtest::permits`).
    pub fn open(target_hwnd: isize) -> Option<WatchedField> {
        if !crate::shell::realtest::permits(Some(target_hwnd)) {
            return None;
        }
        #[cfg(windows)]
        {
            let pid = crate::pasting::text_injector::window_process(target_hwnd)?;
            uia::Watched::open(pid).map(|inner| WatchedField { inner })
        }
        #[cfg(not(windows))]
        {
            let _ = target_hwnd;
            None
        }
    }

    /// The whole field now, focused or not (you may have looked elsewhere
    /// before fixing a word): up to `limit` characters, and the caret in
    /// them. Longer → its first `limit` + 1 characters, `to_end` false.
    /// `None`: the element is gone or didn't answer in time.
    pub fn read(&self, limit: usize) -> Option<FieldWindow> {
        #[cfg(windows)]
        {
            self.inner.read(limit)
        }
        #[cfg(not(windows))]
        {
            let _ = limit;
            None
        }
    }
}

/// The focused element's UI Automation class and name, if it's in process
/// `pid` — for `app_kind` to tell an editor's integrated terminal (xterm.js's
/// input) from the editor. One round trip, cut short (`PROBE_TIMEOUT_MS`);
/// `None` when it can't say. The caller checks `realtest::permits` first.
pub fn focused_element(pid: u32) -> Option<(String, String)> {
    #[cfg(windows)]
    {
        uia::focused_element(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// The highlighted entry of an app's suggestion list, if the focus (as a
/// screen reader sees it) is on one in process `pid`: its name. An app's
/// "@" list moves the screen reader's focus to the entry it would insert
/// (`aria-activedescendant`, which Chromium reports as the focus) while the
/// keyboard stays in the box (`compose.rs` checks it's the one said before
/// pressing Tab). One round trip, cut short (`PROBE_TIMEOUT_MS`); `None`
/// when the focus is on anything else or it can't say. The caller checks
/// `realtest::permits` first.
pub fn picked_option(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        uia::picked_option(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

// ---- Does anything that takes text have the focus? ----

/// Whether what has the focus takes text ([`focus_editable`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editable {
    Yes,
    /// UI Automation says plainly that nothing there takes text: a button,
    /// a list item, a web page with no box focused, read-only text.
    No,
    /// Can't tell (a game, a canvas, an app UI Automation can't see, a
    /// timeout, the focus in another app): treated as yes.
    Unknown,
}

/// What UI Automation says about one element on the way up from the focus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusFacts {
    pub pid: u32,
    /// Its control type (`UIA_*ControlTypeId`).
    pub control_type: i32,
    /// The UI framework it's from ("Win32", "WinForm", "WPF", "XAML",
    /// "DirectUI", "Chrome", "Gecko"…; empty: it doesn't say).
    pub framework: String,
    /// A TextPattern, and whether its text is read-only.
    pub text: bool,
    pub text_read_only: bool,
    /// A ValuePattern (any control: a spreadsheet's cell takes a paste
    /// through one), and whether it's read-only.
    pub value: bool,
    pub value_read_only: bool,
    /// The legacy IAccessible state says read-only.
    pub legacy_read_only: bool,
}

const UIA_BUTTON: i32 = 50000;
const UIA_CHECK_BOX: i32 = 50002;
const UIA_EDIT: i32 = 50004;
const UIA_HYPERLINK: i32 = 50005;
const UIA_IMAGE: i32 = 50006;
const UIA_LIST_ITEM: i32 = 50007;
const UIA_LIST: i32 = 50008;
const UIA_MENU: i32 = 50009;
const UIA_MENU_BAR: i32 = 50010;
const UIA_MENU_ITEM: i32 = 50011;
const UIA_RADIO_BUTTON: i32 = 50013;
const UIA_SCROLL_BAR: i32 = 50014;
const UIA_SLIDER: i32 = 50015;
const UIA_TAB: i32 = 50018;
const UIA_TAB_ITEM: i32 = 50019;
const UIA_TOOL_BAR: i32 = 50021;
const UIA_TREE: i32 = 50023;
const UIA_TREE_ITEM: i32 = 50024;
const UIA_DOCUMENT: i32 = 50030;
const UIA_SPLIT_BUTTON: i32 = 50031;
const UIA_HEADER_ITEM: i32 = 50035;
const UIA_TITLE_BAR: i32 = 50037;

/// Controls that never take typed text, whatever they say.
const NOT_TEXT: [i32; 20] = [
    UIA_BUTTON,
    UIA_CHECK_BOX,
    UIA_HYPERLINK,
    UIA_IMAGE,
    UIA_LIST_ITEM,
    UIA_LIST,
    UIA_MENU,
    UIA_MENU_BAR,
    UIA_MENU_ITEM,
    UIA_RADIO_BUTTON,
    UIA_SCROLL_BAR,
    UIA_SLIDER,
    UIA_TAB,
    UIA_TAB_ITEM,
    UIA_TOOL_BAR,
    UIA_TREE,
    UIA_TREE_ITEM,
    UIA_SPLIT_BUTTON,
    UIA_HEADER_ITEM,
    UIA_TITLE_BAR,
];

/// UI frameworks whose UI Automation shows a text field as one: when
/// they say the focus is on a button, it is. (A game, a canvas, Java,
/// Qt… may not: they never count as "no".)
const PLAIN_FRAMEWORKS: [&str; 7] = ["Win32", "WinForm", "WPF", "XAML", "DirectUI", "Chrome", "Gecko"];

impl FocusFacts {
    /// It takes text: a TextPattern or a ValuePattern that isn't
    /// read-only, or an edit control that doesn't say it is.
    fn takes_text(&self) -> bool {
        let read_only = self.legacy_read_only;
        (self.text && !self.text_read_only && !read_only)
            || (self.value && !self.value_read_only && !read_only)
            || (self.control_type == UIA_EDIT && !read_only && !self.value_read_only && !self.text_read_only)
    }

    /// Text that says it's read-only (a web page with no box focused, a
    /// mail being read).
    fn read_only_text(&self) -> bool {
        (self.text || self.value || self.control_type == UIA_EDIT)
            && (self.text_read_only || self.value_read_only || self.legacy_read_only)
    }
}

/// Does what has the focus in process `pid` take text? `chain`: the focused
/// element, then up to [`PARENT_LEVELS`] parents. Yes when it or a parent
/// in the same app takes text; **no** only when a framework that shows
/// fields plainly ([`PLAIN_FRAMEWORKS`]) says the focus is on a control
/// that never takes text ([`NOT_TEXT`]) or on read-only text, with no field
/// around it; else unknown. Pure.
pub fn editable(chain: &[FocusFacts], pid: u32) -> Editable {
    let Some(first) = chain.first() else { return Editable::Unknown };
    if first.pid != pid {
        return Editable::Unknown;
    }
    if chain.iter().take(PARENT_LEVELS + 1).take_while(|f| f.pid == pid).any(FocusFacts::takes_text) {
        return Editable::Yes;
    }
    if !PLAIN_FRAMEWORKS.contains(&first.framework.as_str()) {
        return Editable::Unknown;
    }
    if NOT_TEXT.contains(&first.control_type) || (matches!(first.control_type, UIA_DOCUMENT | UIA_EDIT) && first.read_only_text()) {
        Editable::No
    } else {
        Editable::Unknown
    }
}

/// Does anything that takes text have the focus in `target_hwnd`'s app
/// (UI Automation, one time-boxed look)? For the paste: "Select a text box
/// first" only on a plain no. Test mode never reads (unknown); robot mode
/// only a window the test opened.
pub fn focus_editable(target_hwnd: Option<isize>) -> Editable {
    let Some(hwnd) = target_hwnd else { return Editable::Unknown };
    if !crate::shell::realtest::permits(Some(hwnd)) {
        return Editable::Unknown;
    }
    let Some(pid) = crate::pasting::text_injector::window_process(hwnd) else { return Editable::Unknown };
    #[cfg(windows)]
    {
        let verdict = uia::focus_facts().map_or(Editable::Unknown, |chain| editable(&chain, pid));
        tracing::debug!(?verdict, "What has the focus takes text?");
        verdict
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Editable::Unknown
    }
}

// ---- The text box the dictation started in ----

/// Where the focus is as a dictation is pasted, against the element it
/// started on (the keeper's look, [`keep_focus`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusNow {
    /// The same element.
    Same,
    /// Another element in the same app that takes text.
    OtherBox,
    /// Something in the same app that never takes text — a toolbar
    /// button, a list, a page with no box focused ([`editable`]'s plain no).
    NoBox,
    /// Can't tell (UI Automation can't see it, a timeout).
    Unknown,
    /// Another app.
    Elsewhere,
}

/// What's done about the focus before the paste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refocus {
    /// Nothing: it's still there, it's in another app (the window's own
    /// rules apply), or it can't be told.
    Stay,
    /// The person moved to another text box: the paste goes there.
    Follow,
    /// It wandered off to something that takes no text: put back on the
    /// text box the dictation started in.
    PutBack,
    /// It should have been put back, but the box was gone, disabled or
    /// off screen, or wouldn't take the focus.
    CouldNot,
}

impl Refocus {
    pub fn name(self) -> &'static str {
        match self {
            Refocus::Stay => "stay",
            Refocus::Follow => "follow",
            Refocus::PutBack => "putBack",
            Refocus::CouldNot => "couldNot",
        }
    }
}

/// What to do about where the focus is now (`now`), for a dictation that
/// started in a text box or not (`started_in_text`). Another text box →
/// follow it (the person clicked there on purpose; its text is read
/// afresh); something that takes no text, after starting in a box → put
/// it back; anything else → leave it. Pure.
pub fn refocus(started_in_text: bool, now: FocusNow) -> Refocus {
    match now {
        FocusNow::OtherBox => Refocus::Follow,
        FocusNow::NoBox if started_in_text => Refocus::PutBack,
        _ => Refocus::Stay,
    }
}

/// How long a dictation's keeper waits for its paste (a dictation stops at
/// 15 minutes, then it's transcribed).
#[cfg_attr(not(windows), allow(dead_code))]
const KEEP_FOR: std::time::Duration = std::time::Duration::from_secs(20 * 60);
/// How long the paste waits for the keeper's answer.
const ASK_WITHIN: std::time::Duration = std::time::Duration::from_millis(900);

/// What the paste asks the keeper.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Ask {
    /// At the dictation's delivery: did the person move to another text
    /// box? (Then that's the dictation's box now.)
    Follow(std::sync::mpsc::Sender<Refocus>),
    /// Just before the paste keys: put the focus back if it wandered off.
    Settle(std::sync::mpsc::Sender<Refocus>),
}

/// The thread holding the element a dictation started on (COM: it lives
/// and dies there).
struct Keeper {
    target: isize,
    since: std::time::Instant,
    asks: std::sync::mpsc::Sender<Ask>,
}

static KEEPER: Mutex<Option<Keeper>> = Mutex::new(None);

fn keeper() -> std::sync::MutexGuard<'static, Option<Keeper>> {
    KEEPER.lock().unwrap_or_else(|p| p.into_inner())
}

/// A dictation that's pasted started recording into `target_hwnd`: the
/// element with the focus there is kept (on a thread of its own, held as
/// UI Automation's element, cheap: one look now), so the paste can put
/// the focus back on it ([`restore_focus`]) or follow the person to
/// another box ([`follow_focus`]). The previous dictation's goes. Never
/// in a test run; robot mode only a window the test opened.
pub fn keep_focus(target_hwnd: Option<isize>) {
    let mut kept = keeper();
    *kept = None;
    let Some(hwnd) = target_hwnd.filter(|&h| crate::shell::realtest::permits(Some(h))) else {
        return;
    };
    #[cfg(windows)]
    {
        let Some(pid) = crate::pasting::text_injector::window_process(hwnd) else { return };
        let (asks, answers) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("focus-keeper".into())
            .spawn(move || uia::keep(pid, answers));
        if spawned.is_ok() {
            *kept = Some(Keeper { target: hwnd, since: std::time::Instant::now(), asks });
        }
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Ask the keeper `ask` and wait (at most [`ASK_WITHIN`]) for its answer.
fn ask_keeper(asks: &std::sync::mpsc::Sender<Ask>, ask: impl FnOnce(std::sync::mpsc::Sender<Refocus>) -> Ask) -> Option<Refocus> {
    let (tx, rx) = std::sync::mpsc::channel();
    asks.send(ask(tx)).ok()?;
    rx.recv_timeout(ASK_WITHIN).ok()
}

/// At a dictation's delivery, before AI cleanup and the fit: the person
/// moved to another text box in the dictation's window while talking (or
/// while it was transcribed) — the paste goes there, and the text around
/// its cursor is read afresh, for smart spacing and cleanup's context (when
/// it's read at all). A look at the focus, nothing changed in the app.
pub fn follow_focus() {
    let Some((asks, target)) = keeper().as_ref().map(|k| (k.asks.clone(), k.target)) else {
        return;
    };
    match ask_keeper(&asks, Ask::Follow) {
        Some(Refocus::Follow) => {
            tracing::info!("The focus moved to another text box in the dictation's window: the paste goes there");
            let wanted = session().read_wanted;
            if wanted {
                let ctx = read_context(Some(target));
                let mut s = session();
                // A read of the box before still running: never over this one.
                s.id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
                s.reading = Reading::Done(ctx);
            }
        }
        other => tracing::debug!(?other, "The focus at the dictation's delivery"),
    }
}

/// Just before a dictation's paste keys (its window back in front): the
/// focus wandered off the text box the dictation started in to something
/// in the same window that takes no text (a click on a toolbar, a list) →
/// put back on it. Another text box → left there (the person moved on
/// purpose). Another app → left (the window rules come first). Once per
/// dictation: its paste takes the keeper; `None` when there's none for
/// `target_hwnd` (not a dictation's paste, a test run, nothing kept).
pub fn restore_focus(target_hwnd: Option<isize>) -> Option<Refocus> {
    let kept = {
        let mut k = keeper();
        let ours = k
            .as_ref()
            .is_some_and(|kp| Some(kp.target) == target_hwnd && kp.since.elapsed() < KEEP_FOR);
        if ours {
            k.take()
        } else {
            None
        }
    }?;
    let did = ask_keeper(&kept.asks, Ask::Settle)?;
    match did {
        Refocus::PutBack => tracing::info!("The focus had wandered off the dictation's text box in its window: put back"),
        Refocus::CouldNot => tracing::info!("The focus had wandered off the dictation's text box, and it couldn't be put back"),
        Refocus::Follow => tracing::debug!("The focus moved to another text box after the dictation was fitted"),
        Refocus::Stay => {}
    }
    Some(did)
}

// ---- Did the paste land? ----

/// A Win32 edit control's text length and selection (its own units): what
/// any paste changes — the text grows, or a selection is replaced and the
/// caret moves to the end of what came in. Nothing of the text itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditState {
    pub len: usize,
    pub sel: (usize, usize),
}

/// What became of a paste ([`PasteCheck`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landed {
    /// The field changed.
    Yes,
    /// The field stayed exactly as it was for the whole wait.
    No,
    /// A read failed (the app didn't answer in time, the focus moved…):
    /// no telling, so nothing more is done.
    Unknown,
}

/// One read after the paste (`now`; `None`: it failed) against the field as
/// it was just before (`before`): changed → it landed; a failed read →
/// unknown; the same once the wait is over (`waited_out`) → it didn't;
/// the same before then → `None`, read again.
pub fn paste_verdict(before: EditState, now: Option<EditState>, waited_out: bool) -> Option<Landed> {
    match now {
        None => Some(Landed::Unknown),
        Some(now) if now != before => Some(Landed::Yes),
        Some(_) if waited_out => Some(Landed::No),
        Some(_) => None,
    }
}

/// A paste about to go into a **Win32 edit control** (a WinForms text box,
/// a classic EDIT or a RichEdit: their text is all there is, nothing stands
/// in for it), and how the control stood just before — to check afterwards
/// that the paste landed (`text_injector`: else it types the text, once).
/// Only these controls: a web editor or a code editor takes a paste through
/// a hidden input it empties again (Google Docs, Ace, xterm.js…), so an
/// unchanged field there says nothing. Only the control with the focus in
/// the target window's own process, never a password box or a read-only
/// one; its length and selection are read, never its text.
pub struct PasteCheck {
    target: isize,
    control: isize,
    before: EditState,
}

impl PasteCheck {
    /// The edit control with the focus in `target_hwnd` now, and its state.
    /// `None` for anything else (or in a test run; robot mode: only a
    /// window the test opened).
    pub fn before(target_hwnd: Option<isize>) -> Option<PasteCheck> {
        let target = target_hwnd?;
        if !crate::shell::realtest::permits(Some(target)) {
            return None;
        }
        #[cfg(windows)]
        {
            let control = uia::focused_edit(target)?;
            let before = uia::edit_state(control)?;
            Some(PasteCheck { target, control, before })
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    /// The control now ([`paste_verdict`]): `None` while it's too early to
    /// tell. A control that lost the focus is a failed read.
    pub fn verdict(&self, waited_out: bool) -> Option<Landed> {
        #[cfg(windows)]
        let now = self.still_focused().then(|| uia::edit_state(self.control)).flatten();
        #[cfg(not(windows))]
        let now = None;
        paste_verdict(self.before, now, waited_out)
    }

    /// The control still has the focus in its window (typing goes there).
    pub fn still_focused(&self) -> bool {
        #[cfg(windows)]
        {
            uia::focused_edit(self.target) == Some(self.control)
        }
        #[cfg(not(windows))]
        {
            let _ = (self.target, self.control);
            false
        }
    }
}

/// A selection longer than this is read as its start and its end (all
/// that fitting needs: the first word, the last mark, the spaces).
const SELECTION_CHARS: usize = 400;
const SELECTION_ENDS: usize = 120;

/// A selected text as fitting reads it: a long one as its start and end.
#[cfg_attr(not(windows), allow(dead_code))]
fn shorten_selection(text: &str) -> String {
    let n = text.chars().count();
    if n <= SELECTION_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(SELECTION_ENDS).collect();
    let tail: String = text.chars().skip(n - SELECTION_ENDS).collect();
    format!("{head} … {tail}")
}

/// Whether a window class is a Win32 edit control Yap may ask for its
/// selection (EM_GETSEL): `Some(true)` for a RichEdit (2.0 and later),
/// `Some(false)` for a plain EDIT, also as WinForms names its own
/// ("WindowsForms10.EDIT.app.0.…", "WindowsForms10.RichEdit20W.app.0.…").
#[cfg_attr(not(windows), allow(dead_code))]
fn edit_class(class: &str) -> Option<bool> {
    let class = class.to_ascii_lowercase();
    let base = match class.strip_prefix("windowsforms10.") {
        Some(rest) => rest.split('.').next().unwrap_or_default(),
        None => class.as_str(),
    };
    if base == "edit" {
        Some(false)
    } else if base.starts_with("richedit") && base != "richedit" {
        // ("RICHEDIT" alone is RichEdit 1.0, which counts line breaks
        // differently.)
        Some(true)
    } else {
        None
    }
}

/// An edit control's selection (`start`..`end`, in its own UTF-16 units) as
/// byte offsets in `value`, its text as UI Automation gives it. A plain
/// EDIT counts "\r\n" as two units; a RichEdit any line break as one (it
/// keeps "\r" where the value may say "\r\n" or "\n"). Never between a "\r"
/// and its "\n" (moved past the pair). `None` when the selection ends
/// beyond the text: the control and the value don't agree.
#[cfg_attr(not(windows), allow(dead_code))]
fn units_to_bytes(value: &str, start: usize, end: usize, rich: bool) -> Option<(usize, usize)> {
    let (mut s, mut e) = (None, None);
    let mut units = 0usize;
    let mut prev = '\0';
    for (i, c) in value.char_indices() {
        let mid_break = prev == '\r' && c == '\n';
        if !mid_break {
            if s.is_none() && units >= start {
                s = Some(i);
            }
            if e.is_none() && units >= end {
                e = Some(i);
            }
        }
        if !(rich && mid_break) {
            units += c.len_utf16();
        }
        prev = c;
    }
    if units < end {
        return None;
    }
    let s = s.unwrap_or(value.len());
    let e = e.unwrap_or(value.len());
    Some((s, e.max(s)))
}

#[cfg(windows)]
pub(crate) mod uia {
    use windows::core::{Interface, BOOL};
    use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
        IUIAutomationLegacyIAccessiblePattern, IUIAutomationTextPattern, IUIAutomationTextPattern2,
        IUIAutomationTextRange, IUIAutomationValuePattern, TextPatternRangeEndpoint_End,
        TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_ClassNamePropertyId, UIA_ComboBoxControlTypeId,
        UIA_ControlTypePropertyId, UIA_DataItemControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
        UIA_IsReadOnlyAttributeId, UIA_LegacyIAccessiblePatternId, UIA_ListItemControlTypeId,
        UIA_MenuItemControlTypeId, UIA_NamePropertyId, UIA_ProcessIdPropertyId, UIA_TextPattern2Id,
        UIA_TextPatternId, UIA_TreeItemControlTypeId, UIA_ValuePatternId,
    };

    use super::{FieldWindow, Offers, SELECTION_CHARS, SELECTION_ENDS};
    use crate::pasting::smart_insert::{head_chars, tail_chars, FieldContext, CONTEXT_CHARS};

    /// COM on this thread for as long as it lives; undone on drop when this
    /// call did it. Declared first in a function, it drops after every COM
    /// object made there.
    pub(crate) struct Com(bool);

    impl Com {
        pub(crate) fn init() -> Option<Com> {
            // S_OK / S_FALSE (already initialised): ours to balance. A
            // different apartment (RPC_E_CHANGED_MODE) is usable too.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_ok() {
                Some(Com(true))
            } else if hr == RPC_E_CHANGED_MODE {
                Some(Com(false))
            } else {
                None
            }
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// The UI Automation client, with `timeout_ms` for connections and
    /// transactions where Windows supports it (8+, `CUIAutomation8`).
    pub(crate) fn automation(timeout_ms: u32) -> Option<IUIAutomation> {
        unsafe {
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))
                .ok()?;
            if let Ok(a2) = automation.cast::<IUIAutomation2>() {
                let _ = a2.SetConnectionTimeout(timeout_ms);
                let _ = a2.SetTransactionTimeout(timeout_ms);
            }
            Some(automation)
        }
    }

    /// Read the current text selection from the focused UI element via UI
    /// Automation. `Some(text)` if a non-empty selection is found; `None` on no
    /// selection, an unsupported control, or any COM error (never panics).
    pub fn uia_selected_text() -> Option<String> {
        let _com = Com::init()?;
        unsafe {
            let automation = automation(super::SELECTION_TIMEOUT_MS)?;
            let focused = automation.GetFocusedElement().ok()?;
            // The focused element, or the text field it's part of (its
            // selection is the field's); read-only or not, it's only read.
            let element = field_for(&automation, &focused, false)?;

            // Missing TextPattern → Err / null; the cast resolves both to None.
            let unknown = element.GetCurrentPattern(UIA_TextPatternId).ok()?;
            let text_pattern: IUIAutomationTextPattern = unknown.cast().ok()?;

            let selection = text_pattern.GetSelection().ok()?;
            if selection.Length().ok()? == 0 {
                return None;
            }
            let range = selection.GetElement(0).ok()?;
            let bstr = range.GetText(-1).ok()?; // -1 = full selection, no cap
            let text = bstr.to_string();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
    }

    /// The focused element's selection for reading aloud, if the focus is
    /// in process `pid` and not a password box: the TextPattern selection of
    /// the focused element or of the first one above it that has one (a web
    /// page's document, when the focus is on a link in it) — read-only text
    /// too. Several selections (Word) one after another.
    pub fn selection_in(pid: u32) -> Option<String> {
        let _com = Com::init()?;
        unsafe {
            let automation = automation(super::SELECTION_TIMEOUT_MS)?;
            let focused = automation.GetFocusedElement().ok()?;
            if focused.CurrentProcessId().ok()? as u32 != pid || focused.CurrentIsPassword().ok()?.as_bool() {
                return None;
            }
            let walker = automation.ControlViewWalker().ok()?;
            let mut e = focused;
            for _ in 0..12 {
                if e.CurrentProcessId().ok()? as u32 != pid {
                    return None;
                }
                let pattern = e
                    .GetCurrentPattern(UIA_TextPatternId)
                    .ok()
                    .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok());
                if let Some(pattern) = pattern {
                    let ranges = pattern.GetSelection().ok()?;
                    let mut parts = Vec::new();
                    for i in 0..ranges.Length().unwrap_or(0) {
                        if let Some(t) = ranges.GetElement(i).ok().and_then(|r| r.GetText(-1).ok()) {
                            let t = t.to_string();
                            if !t.trim().is_empty() {
                                parts.push(t);
                            }
                        }
                    }
                    let text = parts.join("\n");
                    let text = text.trim();
                    // A document with nothing selected: nothing to read.
                    return (!text.is_empty()).then(|| text.to_string());
                }
                e = walker.GetParentElement(&e).ok()?;
            }
            None
        }
    }

    /// A Win32 edit control's selection (a WinForms TextBox: a ValuePattern
    /// and no TextPattern), cut from its value where the control says
    /// (EM_GETSEL), if the focus is in process `pid` and not a password box.
    pub fn edit_selection_in(pid: u32) -> Option<String> {
        let _com = Com::init()?;
        unsafe {
            let automation = automation(super::SELECTION_TIMEOUT_MS)?;
            let focused = automation.GetFocusedElement().ok()?;
            if focused.CurrentProcessId().ok()? as u32 != pid || focused.CurrentIsPassword().ok()?.as_bool() {
                return None;
            }
            let pattern = text_value_pattern(&focused)?;
            let value = pattern.CurrentValue().ok()?.to_string();
            let (value, at) = value_selection(&focused, value, super::SELECTION_TIMEOUT_MS);
            let (start, end) = at?;
            let text = value.get(start..end)?.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
    }

    /// A transform's selection in process `pid` ([`super::editable_selection`]):
    /// the field the focus is in, not read-only, not a password box; its
    /// TextPattern selection (one range, itself not read-only), else a Win32
    /// edit control's own (`EM_GETSEL`); `Unreadable` when neither answers.
    pub fn editable_selection_in(pid: u32) -> super::FieldSelection {
        use super::FieldSelection as F;
        let Some(_com) = Com::init() else { return F::Unreadable };
        unsafe {
            let Some(automation) = automation(super::SELECTION_TIMEOUT_MS) else { return F::Unreadable };
            let Ok(focused) = automation.GetFocusedElement() else { return F::Unreadable };
            if focused.CurrentProcessId().map(|p| p as u32).ok() != Some(pid) {
                return F::Nothing;
            }
            if focused.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
                return F::ReadOnly;
            }
            let Some(field) = field_for(&automation, &focused, false) else { return F::Unreadable };
            if offers(&field, true).read_only {
                return F::ReadOnly;
            }
            let pattern = field
                .GetCurrentPattern(UIA_TextPatternId)
                .ok()
                .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok());
            if let Some(pattern) = pattern {
                let Ok(ranges) = pattern.GetSelection() else { return F::Unreadable };
                match ranges.Length().unwrap_or(0) {
                    0 => return F::Nothing,
                    1 => {}
                    // Several selections (Word): one paste can't replace them.
                    _ => return F::Nothing,
                }
                let Ok(range) = ranges.GetElement(0) else { return F::Unreadable };
                if let Ok(v) = range.GetAttributeValue(UIA_IsReadOnlyAttributeId) {
                    if bool::try_from(&v).unwrap_or(false) {
                        return F::ReadOnly;
                    }
                }
                let text = range.GetText(-1).map(|b| b.to_string()).unwrap_or_default();
                return if text.trim().is_empty() { F::Nothing } else { F::Text(text) };
            }
            if let Some(p) = text_value_pattern(&field) {
                let Ok(value) = p.CurrentValue() else { return F::Unreadable };
                let (value, at) = value_selection(&field, value.to_string(), super::SELECTION_TIMEOUT_MS);
                return match at.and_then(|(s, e)| value.get(s..e)) {
                    Some(t) if !t.trim().is_empty() => F::Text(t.to_string()),
                    Some(_) => F::Nothing,
                    None => F::Unreadable,
                };
            }
            F::Unreadable
        }
    }

    /// The focused element's class and name, if it's in process `pid`: one
    /// round trip (a cache request), at most [`super::PROBE_TIMEOUT_MS`].
    pub fn focused_element(pid: u32) -> Option<(String, String)> {
        let _com = Com::init()?;
        let automation = automation(super::PROBE_TIMEOUT_MS)?;
        unsafe {
            let cache = automation.CreateCacheRequest().ok()?;
            for property in [UIA_ProcessIdPropertyId, UIA_ClassNamePropertyId, UIA_NamePropertyId] {
                cache.AddProperty(property).ok()?;
            }
            let element = automation.GetFocusedElementBuildCache(&cache).ok()?;
            if element.CachedProcessId().ok()? as u32 != pid {
                return None;
            }
            let class = element.CachedClassName().map(|b| b.to_string()).unwrap_or_default();
            let name = element.CachedName().map(|b| b.to_string()).unwrap_or_default();
            Some((class, name))
        }
    }

    /// The focused element's name when it's a list's entry (a list item,
    /// a menu item, a data item, a tree item) in process `pid`: an app's
    /// "@" list's highlighted entry. One round trip (a cache request), at
    /// most [`super::PROBE_TIMEOUT_MS`].
    pub fn picked_option(pid: u32) -> Option<String> {
        let _com = Com::init()?;
        let automation = automation(super::PROBE_TIMEOUT_MS)?;
        unsafe {
            let cache = automation.CreateCacheRequest().ok()?;
            for property in [UIA_ProcessIdPropertyId, UIA_ControlTypePropertyId, UIA_NamePropertyId] {
                cache.AddProperty(property).ok()?;
            }
            let element = automation.GetFocusedElementBuildCache(&cache).ok()?;
            if element.CachedProcessId().ok()? as u32 != pid {
                return None;
            }
            let kind = element.CachedControlType().ok()?;
            let entry = [UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId, UIA_DataItemControlTypeId, UIA_TreeItemControlTypeId]
                .contains(&kind);
            entry.then(|| element.CachedName().map(|b| b.to_string()).unwrap_or_default()).filter(|n| !n.trim().is_empty())
        }
    }

    /// The focused element and up to [`super::PARENT_LEVELS`] parents, as
    /// [`super::editable`] reads them; each within [`super::CHECK_TIMEOUT_MS`].
    pub fn focus_facts() -> Option<Vec<super::FocusFacts>> {
        let _com = Com::init()?;
        let automation = automation(super::CHECK_TIMEOUT_MS)?;
        unsafe {
            let focused = automation.GetFocusedElement().ok()?;
            Some(facts_chain(&automation, &focused))
        }
    }

    /// `focused` and up to [`super::PARENT_LEVELS`] parents, as
    /// [`super::editable`] reads them.
    unsafe fn facts_chain(automation: &IUIAutomation, focused: &IUIAutomationElement) -> Vec<super::FocusFacts> {
        let walker = automation.ControlViewWalker().ok();
        let mut chain = Vec::new();
        let mut e = focused.clone();
        loop {
            let facts = focus_facts_of(&e);
            let stop = chain.first().is_some_and(|f: &super::FocusFacts| f.pid != facts.pid);
            chain.push(facts);
            if stop || chain.len() > super::PARENT_LEVELS {
                break;
            }
            match walker.as_ref().and_then(|w| w.GetParentElement(&e).ok()) {
                Some(parent) => e = parent,
                None => break,
            }
        }
        chain
    }

    /// The keeper ([`super::keep_focus`]): the element with the focus now,
    /// if it's in process `pid`, held until the paste asks — at the
    /// delivery whether the person moved to another text box (then that one
    /// is held), at the paste keys whether to put the focus back — or for
    /// [`super::KEEP_FOR`] at most.
    pub fn keep(pid: u32, asks: std::sync::mpsc::Receiver<super::Ask>) {
        use super::{refocus, Ask, Editable, Refocus};
        let Some(_com) = Com::init() else { return };
        let Some(automation) = automation(super::CHECK_TIMEOUT_MS) else { return };
        unsafe {
            let Ok(start) = automation.GetFocusedElement() else { return };
            if start.CurrentProcessId().map(|p| p as u32).ok() != Some(pid) {
                tracing::debug!("The focus isn't in the dictation's app: no text box kept");
                return;
            }
            let mut kept = start;
            let mut in_text = super::editable(&facts_chain(&automation, &kept), pid) == Editable::Yes;
            tracing::debug!(in_text, "The element the dictation started on, kept");
            while let Ok(ask) = asks.recv_timeout(super::KEEP_FOR) {
                let (reply, settle) = match ask {
                    Ask::Follow(reply) => (reply, false),
                    Ask::Settle(reply) => (reply, true),
                };
                let (now, focused) = look(&automation, &kept, pid);
                let mut did = refocus(in_text, now);
                tracing::debug!(?now, ?did, settle, "Where the focus is, against where the dictation started");
                match did {
                    Refocus::Follow => {
                        if let Some(f) = focused {
                            kept = f;
                            in_text = true;
                        }
                    }
                    // Only at the paste: at the delivery it may yet come back.
                    Refocus::PutBack if !settle => did = Refocus::Stay,
                    Refocus::PutBack => {
                        if !put_back(&automation, &kept) {
                            did = Refocus::CouldNot;
                        }
                    }
                    Refocus::Stay | Refocus::CouldNot => {}
                }
                let _ = reply.send(did);
                if settle {
                    return;
                }
            }
        }
    }

    /// Where the focus is now against `kept` (in process `pid`), and the
    /// focused element when it's another text box.
    unsafe fn look(
        automation: &IUIAutomation,
        kept: &IUIAutomationElement,
        pid: u32,
    ) -> (super::FocusNow, Option<IUIAutomationElement>) {
        use super::{Editable, FocusNow};
        let Ok(focused) = automation.GetFocusedElement() else { return (FocusNow::Unknown, None) };
        if focused.CurrentProcessId().map(|p| p as u32).ok() != Some(pid) {
            return (FocusNow::Elsewhere, None);
        }
        if automation.CompareElements(kept, &focused).is_ok_and(|same| same.as_bool()) {
            return (FocusNow::Same, None);
        }
        match super::editable(&facts_chain(automation, &focused), pid) {
            Editable::Yes => (FocusNow::OtherBox, Some(focused)),
            Editable::No => (FocusNow::NoBox, None),
            Editable::Unknown => (FocusNow::Unknown, None),
        }
    }

    /// The focus onto `kept`, if it's still there, enabled and on screen;
    /// `true` once UI Automation says it has it.
    unsafe fn put_back(automation: &IUIAutomation, kept: &IUIAutomationElement) -> bool {
        if !kept.CurrentIsEnabled().is_ok_and(|b| b.as_bool()) || kept.CurrentIsOffscreen().is_ok_and(|b| b.as_bool()) {
            return false;
        }
        if kept.SetFocus().is_err() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        automation
            .GetFocusedElement()
            .is_ok_and(|f| automation.CompareElements(kept, &f).is_ok_and(|same| same.as_bool()))
    }

    unsafe fn focus_facts_of(e: &IUIAutomationElement) -> super::FocusFacts {
        let text = e
            .GetCurrentPattern(UIA_TextPatternId)
            .ok()
            .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok());
        let text_read_only = text
            .as_ref()
            .and_then(|p| p.DocumentRange().ok())
            .and_then(|r| r.GetAttributeValue(UIA_IsReadOnlyAttributeId).ok())
            .and_then(|v| bool::try_from(&v).ok())
            .unwrap_or(false);
        let value = e
            .GetCurrentPattern(UIA_ValuePatternId)
            .ok()
            .and_then(|u| u.cast::<IUIAutomationValuePattern>().ok());
        super::FocusFacts {
            pid: e.CurrentProcessId().map(|p| p as u32).unwrap_or(0),
            control_type: e.CurrentControlType().map(|t| t.0).unwrap_or(0),
            framework: e.CurrentFrameworkId().map(|b| b.to_string()).unwrap_or_default(),
            text: text.is_some(),
            text_read_only,
            value: value.is_some(),
            value_read_only: value.as_ref().is_some_and(|p| p.CurrentIsReadOnly().is_ok_and(|r| r.as_bool())),
            legacy_read_only: legacy_state(e).is_some_and(super::legacy_read_only),
        }
    }

    /// The text around the caret in the focused element (see
    /// [`super::read_context`]). `pid`: the process it must belong to.
    pub fn focused_context(pid: Option<u32>) -> Option<FieldContext> {
        let _com = Com::init()?;
        let automation = automation(super::CONTEXT_TIMEOUT_MS)?;
        unsafe {
            let element = automation.GetFocusedElement().ok()?;
            if let Some(pid) = pid {
                if element.CurrentProcessId().ok()? as u32 != pid {
                    tracing::debug!("The focus is in another app than the dictation's; not reading it");
                    return None;
                }
            }
            element_context(&automation, &element)
        }
    }

    /// The text around the caret in the text field `element` (the focused
    /// one) is, or is part of.
    unsafe fn element_context(automation: &IUIAutomation, element: &IUIAutomationElement) -> Option<FieldContext> {
        // Never a password box (and if it can't say, don't risk it).
        if element.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        let field = field_for(automation, element, true)?;
        if field.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        text_context(automation, &field).or_else(|| value_context(&field))
    }

    /// What `e` offers on the walk up from the focus ([`super::pick_field`]).
    /// `read_only`: also ask whether it's read-only (else taken not to be).
    unsafe fn offers(e: &IUIAutomationElement, read_only: bool) -> Offers {
        let pid = e.CurrentProcessId().map(|p| p as u32).unwrap_or(0);
        let text = e
            .GetCurrentPattern(UIA_TextPatternId)
            .ok()
            .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok())
            .is_some();
        let value = text_value_pattern(e);
        let read_only = read_only
            && (value.as_ref().is_some_and(|p| p.CurrentIsReadOnly().is_ok_and(|r| r.as_bool()))
                || legacy_state(e).is_some_and(super::legacy_read_only));
        Offers {
            pid,
            text,
            value: value.is_some(),
            read_only,
        }
    }

    /// The legacy IAccessible state (`STATE_SYSTEM_*` bits), if it has one.
    unsafe fn legacy_state(e: &IUIAutomationElement) -> Option<u32> {
        let p: IUIAutomationLegacyIAccessiblePattern =
            e.GetCurrentPattern(UIA_LegacyIAccessiblePatternId).ok()?.cast().ok()?;
        p.CurrentState().ok()
    }

    /// The element to read for the focused `element`: itself when it's a
    /// text field, else the text field it's part of — up to
    /// [`super::PARENT_LEVELS`] parents, in its own process
    /// ([`super::pick_field`]). `None` when there's none, or the field is
    /// read-only (`check_read_only`: by its ValuePattern or its legacy
    /// IAccessible state).
    unsafe fn field_for(
        automation: &IUIAutomation,
        element: &IUIAutomationElement,
        check_read_only: bool,
    ) -> Option<IUIAutomationElement> {
        let walker = automation.ControlViewWalker().ok();
        let mut chain: Vec<(IUIAutomationElement, Offers)> = Vec::new();
        let mut e = element.clone();
        loop {
            let o = offers(&e, check_read_only);
            let first_pid = chain.first().map_or(o.pid, |(_, f)| f.pid);
            chain.push((e.clone(), o));
            let decided = o.text || o.value || o.read_only || o.pid != first_pid;
            if decided || chain.len() > super::PARENT_LEVELS {
                break;
            }
            match walker.as_ref().and_then(|w| w.GetParentElement(&e).ok()) {
                Some(parent) => e = parent,
                None => break,
            }
        }
        let list: Vec<Offers> = chain.iter().map(|(_, o)| *o).collect();
        let i = super::pick_field(&list)?;
        if i > 0 {
            tracing::debug!(levels = i, "The focused element has no text of its own; reading the field it's part of");
        }
        Some(chain.swap_remove(i).0)
    }

    /// [`element_context`] for a window's own element (the tests' edit
    /// control), by handle rather than by focus.
    #[cfg(test)]
    pub fn window_context(hwnd: isize) -> Option<FieldContext> {
        let _com = Com::init()?;
        let automation = automation(super::CONTEXT_TIMEOUT_MS)?;
        unsafe {
            let element = automation
                .ElementFromHandle(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void))
                .ok()?;
            element_context(&automation, &element)
        }
    }

    /// The ValuePattern reads alone, the caret asked of the control, for a
    /// window's own element (as a WinForms text box, which has no
    /// TextPattern, is read): [`value_context`], EM_GETSEL's answer and the
    /// post-paste watch's [`whole_value`].
    #[cfg(test)]
    pub fn window_value_reads(
        hwnd: isize,
    ) -> (Option<FieldContext>, Option<EditSelection>, Option<FieldWindow>) {
        let Some(_com) = Com::init() else {
            return (None, None, None);
        };
        let Some(automation) = automation(super::CONTEXT_TIMEOUT_MS) else {
            return (None, None, None);
        };
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            let Ok(element) = automation.ElementFromHandle(hwnd) else {
                return (None, None, None);
            };
            (
                value_context(&element),
                edit_selection(hwnd, super::CONTEXT_TIMEOUT_MS),
                whole_value(&element, 2_000),
            )
        }
    }

    /// TextPattern: the selection (or the caret) and the text either side,
    /// all of it inside the field: Chromium's ranges run on past a text box
    /// into the page around it, so every range is kept within the field's
    /// own document range. An empty field reads as empty, whatever stands
    /// in for its text ([`shows_nothing`]).
    unsafe fn text_context(automation: &IUIAutomation, element: &IUIAutomationElement) -> Option<FieldContext> {
        let pattern: IUIAutomationTextPattern = element.GetCurrentPattern(UIA_TextPatternId).ok()?.cast().ok()?;
        let range = match pattern.GetSelection() {
            Ok(ranges) if ranges.Length().unwrap_or(0) > 0 => ranges.GetElement(0).ok()?,
            // No selection reported: the caret (TextPattern2, Windows 8+).
            _ => {
                let p2: IUIAutomationTextPattern2 =
                    element.GetCurrentPattern(UIA_TextPattern2Id).ok()?.cast().ok()?;
                let mut active = BOOL(0);
                p2.GetCaretRange(&mut active).ok()?
            }
        };
        // A page or a viewer, not a field: nothing would be pasted anyway.
        if let Ok(v) = range.GetAttributeValue(UIA_IsReadOnlyAttributeId) {
            if bool::try_from(&v).unwrap_or(false) {
                return None;
            }
        }
        let document = pattern.DocumentRange().ok()?;
        if !keep_within(&range, &document) {
            tracing::debug!("The selection isn't in the focused field; not reading it");
            return None;
        }
        if shows_nothing(automation, element, &document) {
            return Some(FieldContext::default());
        }
        let selection = selection_text(&range)?;

        let before = range.Clone().ok()?;
        before
            .MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start)
            .ok()?;
        before
            .MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(CONTEXT_CHARS as i32 + 1))
            .ok()?;
        keep_within(&before, &document);
        let before = before.GetText((CONTEXT_CHARS * 2) as i32).ok()?.to_string();

        let after = range.Clone().ok()?;
        after
            .MoveEndpointByRange(TextPatternRangeEndpoint_Start, &range, TextPatternRangeEndpoint_End)
            .ok()?;
        after
            .MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, CONTEXT_CHARS as i32 + 1)
            .ok()?;
        keep_within(&after, &document);
        let after = after.GetText((CONTEXT_CHARS * 2) as i32).ok()?.to_string();

        Some(FieldContext {
            before: tail_chars(&before, CONTEXT_CHARS),
            after: head_chars(&after, CONTEXT_CHARS),
            selection,
        })
    }

    /// Keep `range` inside `document` (the field's own text): its start no
    /// earlier, its end no later. `false` when it lies wholly outside it (a
    /// selection elsewhere on the page). A provider that can't compare
    /// ranges leaves it as it was.
    unsafe fn keep_within(range: &IUIAutomationTextRange, document: &IUIAutomationTextRange) -> bool {
        let (start, end) = (TextPatternRangeEndpoint_Start, TextPatternRangeEndpoint_End);
        // CompareEndpoints' sign says which comes first (its size varies).
        let cmp = |mine, theirs| range.CompareEndpoints(mine, document, theirs).ok();
        if cmp(end, start).is_some_and(|c| c < 0) || cmp(start, end).is_some_and(|c| c > 0) {
            return false;
        }
        if cmp(start, start).is_some_and(|c| c < 0) {
            let _ = range.MoveEndpointByRange(start, document, start);
        }
        if cmp(end, end).is_some_and(|c| c > 0) {
            let _ = range.MoveEndpointByRange(end, document, end);
        }
        true
    }

    /// Is the field (its whole text: `document`) empty, whatever its text
    /// says? Its text is only a stand-in ([`super::stands_in_for_empty`]:
    /// the field's label, its placeholder, U+FFFC…), or it's a web editor
    /// showing a CSS placeholder: its own class says so (Quill), or that of
    /// its one line (ProseMirror, CKEditor). Never for a field with more
    /// than a line's worth of text.
    unsafe fn shows_nothing(
        automation: &IUIAutomation,
        element: &IUIAutomationElement,
        document: &IUIAutomationTextRange,
    ) -> bool {
        let Ok(text) = document.GetText(super::STAND_IN_CHARS as i32 + 1) else {
            return false;
        };
        let text = text.to_string();
        text.chars().count() <= super::STAND_IN_CHARS
            && (text_stands_in(element, &text) || placeholder_class(automation, element))
    }

    /// A field's labels: its Name, and the name of the element that labels
    /// it (LabeledBy), when there is one.
    unsafe fn labels(element: &IUIAutomationElement) -> Vec<String> {
        let mut out = vec![element.CurrentName().map(|b| b.to_string()).unwrap_or_default()];
        if let Some(by) = element.CurrentLabeledBy().ok().and_then(|l| l.CurrentName().ok()) {
            out.push(by.to_string());
        }
        out
    }

    /// [`super::stands_in_for_empty`] for a TextPattern field showing `text`.
    unsafe fn text_stands_in(element: &IUIAutomationElement, text: &str) -> bool {
        let placeholder = element.CurrentHelpText().map(|b| b.to_string()).unwrap_or_default();
        super::stands_in_for_empty(text, &labels(element), &placeholder, || {
            let pattern: IUIAutomationValuePattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
            Some(pattern.CurrentValue().ok()?.to_string())
        })
    }

    /// A web editor whose class (or its one and only line's) says it shows
    /// a placeholder, not text ([`super::class_says_empty`]).
    unsafe fn placeholder_class(automation: &IUIAutomation, element: &IUIAutomationElement) -> bool {
        let class = |e: &IUIAutomationElement| e.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
        if super::class_says_empty(&class(element)) {
            return true;
        }
        // The raw view: a paragraph is no control, so the control view skips it.
        let Ok(walker) = automation.RawViewWalker() else {
            return false;
        };
        let Ok(line) = walker.GetFirstChildElement(element) else {
            return false;
        };
        // No next sibling = a null element (an error with no code), not a failed call.
        super::class_says_empty(&class(&line))
            && walker.GetNextSiblingElement(&line).is_err_and(|e| e.code().is_ok())
    }

    /// [`super::stands_in_for_empty`] for a ValuePattern-only field: its
    /// placeholder, nothing but markers, or — in an edit or a document, whose
    /// value is its text — exactly its label (its Name, or LabeledBy's) stand
    /// in for empty, as a field showing its label when empty does. (A combo
    /// box's Name is often its value: only its placeholder counts.)
    unsafe fn value_stands_in(element: &IUIAutomationElement, value: &str) -> bool {
        let placeholder = element.CurrentHelpText().map(|b| b.to_string()).unwrap_or_default();
        let text_control = element
            .CurrentControlType()
            .is_ok_and(|k| k == UIA_EditControlTypeId || k == UIA_DocumentControlTypeId);
        let labels = if text_control { labels(element) } else { Vec::new() };
        super::stands_in_for_empty(value, &labels, &placeholder, || None)
    }

    /// The selected text; a long selection as its start and end.
    unsafe fn selection_text(range: &IUIAutomationTextRange) -> Option<String> {
        let text = range.GetText(SELECTION_CHARS as i32 + 1).ok()?.to_string();
        if text.chars().count() <= SELECTION_CHARS {
            return Some(text);
        }
        let end = range.Clone().ok()?;
        end.MoveEndpointByRange(TextPatternRangeEndpoint_Start, range, TextPatternRangeEndpoint_End)
            .ok()?;
        end.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(SELECTION_ENDS as i32))
            .ok()?;
        let tail = end.GetText(SELECTION_ENDS as i32 * 2).ok()?.to_string();
        let head: String = text.chars().take(SELECTION_ENDS).collect();
        Some(format!("{head} … {tail}"))
    }

    /// ValuePattern (fields with no TextPattern: WinForms text boxes, some
    /// Electron inputs): the whole value, and the caret in it when the field
    /// is a Win32 edit control that can say ([`value_selection`]); else the
    /// caret is taken to be at the end, where it is after typing. Text fields
    /// only, never read-only ones; a field showing only its placeholder is
    /// empty.
    unsafe fn value_context(element: &IUIAutomationElement) -> Option<FieldContext> {
        let pattern = editable_value(element)?;
        let value = pattern.CurrentValue().ok()?.to_string();
        if value_stands_in(element, &value) {
            return Some(FieldContext::default());
        }
        let (value, at) = value_selection(element, value, super::CONTEXT_TIMEOUT_MS);
        let (start, end) = at.unwrap_or_else(|| {
            tracing::debug!("No caret position for this field; taking it to be at the end of its text");
            (value.len(), value.len())
        });
        Some(FieldContext {
            before: tail_chars(&value[..start], CONTEXT_CHARS),
            after: head_chars(&value[end..], CONTEXT_CHARS),
            selection: super::shorten_selection(&value[start..end]),
        })
    }

    /// A text control's ValuePattern (an edit, a document, a combo box).
    unsafe fn text_value_pattern(element: &IUIAutomationElement) -> Option<IUIAutomationValuePattern> {
        let kind = element.CurrentControlType().ok()?;
        if ![UIA_EditControlTypeId, UIA_DocumentControlTypeId, UIA_ComboBoxControlTypeId].contains(&kind) {
            return None;
        }
        element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()
    }

    /// A text field's ValuePattern, if it isn't read-only (by the pattern,
    /// or by the legacy IAccessible state).
    unsafe fn editable_value(element: &IUIAutomationElement) -> Option<IUIAutomationValuePattern> {
        let pattern = text_value_pattern(element)?;
        if pattern.CurrentIsReadOnly().ok()?.as_bool() || legacy_state(element).is_some_and(super::legacy_read_only) {
            return None;
        }
        Some(pattern)
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn RealGetWindowClassW(hwnd: HWND, buf: *mut u16, max: u32) -> u32;
        fn SendMessageTimeoutW(
            hwnd: HWND,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout_ms: u32,
            result: *mut usize,
        ) -> isize;
    }

    /// An edit control's selection: its start and end in the control's own
    /// units, and whether it's a RichEdit.
    type EditSelection = (usize, usize, bool);

    const EM_GETSEL: u32 = 0x00B0;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    const SMTO_ERRORONEXIT: u32 = 0x0020;

    /// `value`, a field's ValuePattern text, and where the selection (or the
    /// caret) is in it as byte offsets — for a field that is a Win32 edit
    /// control (a WinForms TextBox or RichTextBox, whose UI Automation has a
    /// value but no TextPattern): asked of the control itself. `None` for
    /// any other field, or when it doesn't answer within `timeout_ms`. A
    /// RichEdit's value can end with the paragraph mark every RichEdit text
    /// ends with (its TextPattern leaves it out): left out here too.
    unsafe fn value_selection(
        element: &IUIAutomationElement,
        mut value: String,
        timeout_ms: u32,
    ) -> (String, Option<(usize, usize)>) {
        let Some((start, end, rich)) = element
            .CurrentNativeWindowHandle()
            .ok()
            .and_then(|hwnd| edit_selection(hwnd, timeout_ms))
        else {
            return (value, None);
        };
        if rich && value.ends_with('\r') {
            value.pop();
        }
        let at = super::units_to_bytes(&value, start, end, rich);
        if at.is_some() {
            tracing::debug!("Found the caret by asking the edit control (EM_GETSEL)");
        }
        (value, at)
    }

    /// The selection of `hwnd` if it's an edit control ([`super::edit_class`]
    /// by its class or its base class): its start and end in the control's
    /// own units, and whether it's a RichEdit. EM_GETSEL with no pointers
    /// (the answer comes back packed in the result, so nothing is written
    /// into the other process), at most `timeout_ms`, never to a hung app.
    unsafe fn edit_selection(hwnd: HWND, timeout_ms: u32) -> Option<EditSelection> {
        let rich = edit_kind(hwnd)?;
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            hwnd,
            EM_GETSEL,
            0,
            0,
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            timeout_ms,
            &mut result,
        );
        if sent == 0 {
            tracing::debug!("The edit control didn't say where its caret is in time");
            return None;
        }
        // -1: a position past 65,535, which the packed answer can't carry.
        let packed = result as u32;
        if packed == u32::MAX {
            return None;
        }
        let (start, end) = ((packed & 0xFFFF) as usize, (packed >> 16) as usize);
        Some((start.min(end), start.max(end), rich))
    }

    /// `hwnd` is a Win32 edit control ([`super::edit_class`], by its class
    /// or its base class): `Some(true)` for a RichEdit.
    unsafe fn edit_kind(hwnd: HWND) -> Option<bool> {
        if hwnd.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        super::edit_class(&String::from_utf16_lossy(&buf[..n])).or_else(|| {
            let n = RealGetWindowClassW(hwnd, buf.as_mut_ptr(), buf.len() as u32) as usize;
            super::edit_class(&String::from_utf16_lossy(&buf[..n.min(buf.len())]))
        })
    }

    const WM_GETTEXTLENGTH: u32 = 0x000E;
    const ES_PASSWORD: i32 = 0x0020;
    const ES_READONLY: i32 = 0x0800;

    /// The control with the keyboard focus in `target`'s (a top-level
    /// window's) thread, if it's a Win32 edit control of the same process
    /// that takes text: not a password box, not read-only (by its styles).
    pub fn focused_edit(target: isize) -> Option<isize> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetGUIThreadInfo, GetWindowLongW, GetWindowThreadProcessId, GUITHREADINFO, GWL_STYLE,
        };
        unsafe {
            let top = HWND(target as *mut core::ffi::c_void);
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(top, Some(&mut pid));
            if thread == 0 {
                return None;
            }
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            GetGUIThreadInfo(thread, &mut info).ok()?;
            let focus = info.hwndFocus;
            if focus.is_invalid() {
                return None;
            }
            let mut focus_pid = 0u32;
            GetWindowThreadProcessId(focus, Some(&mut focus_pid));
            if focus_pid != pid {
                return None;
            }
            edit_kind(focus)?;
            if GetWindowLongW(focus, GWL_STYLE) & (ES_PASSWORD | ES_READONLY) != 0 {
                return None;
            }
            Some(focus.0 as isize)
        }
    }

    /// An edit control's text length and selection now, each asked within
    /// [`super::CHECK_TIMEOUT_MS`] (never of a hung app). `None` when it
    /// doesn't answer, or its selection is past what EM_GETSEL can carry.
    pub fn edit_state(control: isize) -> Option<super::EditState> {
        let hwnd = HWND(control as *mut core::ffi::c_void);
        unsafe {
            let (start, end, _) = edit_selection(hwnd, super::CHECK_TIMEOUT_MS)?;
            let mut len = 0usize;
            let sent = SendMessageTimeoutW(
                hwnd,
                WM_GETTEXTLENGTH,
                0,
                0,
                SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
                super::CHECK_TIMEOUT_MS,
                &mut len,
            );
            (sent != 0).then_some(super::EditState { len, sel: (start, end) })
        }
    }

    /// The field a dictation was pasted into, for the post-paste watch.
    /// Fields drop in order: the COM objects, then COM itself.
    pub struct Watched {
        /// The client the element came from (its timeouts), kept with it.
        automation: IUIAutomation,
        element: IUIAutomationElement,
        _com: Com,
    }

    impl Watched {
        /// The focused element, if it's in process `pid`, not a password box
        /// and a text field that reads (not read-only).
        pub fn open(pid: u32) -> Option<Watched> {
            let com = Com::init()?;
            let automation = automation(super::WATCH_TIMEOUT_MS)?;
            unsafe {
                let focused = automation.GetFocusedElement().ok()?;
                if focused.CurrentProcessId().ok()? as u32 != pid || focused.CurrentIsPassword().ok()?.as_bool() {
                    return None;
                }
                // The text field the focus is (in), not read-only.
                let element = field_for(&automation, &focused, true)?;
                if element.CurrentIsPassword().ok()?.as_bool() {
                    return None;
                }
                let watched = Watched {
                    automation,
                    element,
                    _com: com,
                };
                watched.read(16)?;
                Some(watched)
            }
        }

        /// The whole field, up to `limit` characters (see
        /// [`super::WatchedField::read`]).
        pub fn read(&self, limit: usize) -> Option<FieldWindow> {
            unsafe { whole_field(&self.automation, &self.element, limit) }
        }
    }

    /// [`Watched::read`] for a window's own element (the tests' edit
    /// control), by handle rather than by focus.
    #[cfg(test)]
    pub fn window_watch(hwnd: isize, limit: usize) -> Option<FieldWindow> {
        let _com = Com::init()?;
        let automation = automation(super::WATCH_TIMEOUT_MS)?;
        unsafe {
            let element = automation
                .ElementFromHandle(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void))
                .ok()?;
            if element.CurrentIsPassword().ok()?.as_bool() {
                return None;
            }
            whole_field(&automation, &element, limit)
        }
    }

    /// Line breaks as `\n`, no-break spaces as spaces.
    fn tidy(s: &str) -> String {
        crate::dictionary::learn::normalise(s)
    }

    /// The whole text of a field (TextPattern, else ValuePattern); a field
    /// whose text only stands in for "empty" — its label, its placeholder,
    /// U+FFFC ([`shows_nothing`]'s rules) — reads as empty.
    unsafe fn whole_field(automation: &IUIAutomation, element: &IUIAutomationElement, limit: usize) -> Option<FieldWindow> {
        // Text that's only whitespace is kept as it is; text this long is real.
        let maybe = |w: &FieldWindow| !w.text.trim().is_empty() && w.text.chars().count() <= super::STAND_IN_CHARS;
        let (w, empty) = match whole_text(element, limit) {
            Some(w) => {
                let empty = maybe(&w) && (text_stands_in(element, &w.text) || placeholder_class(automation, element));
                (w, empty)
            }
            None => {
                let w = whole_value(element, limit)?;
                let empty = maybe(&w) && value_stands_in(element, &w.text);
                (w, empty)
            }
        };
        Some(if empty {
            FieldWindow {
                text: String::new(),
                caret: 0,
                ..w
            }
        } else {
            w
        })
    }

    /// TextPattern: the document's text (its first `limit` + 1 characters
    /// when longer: `to_end` false), and where the caret (or the selection)
    /// starts in it.
    unsafe fn whole_text(element: &IUIAutomationElement, limit: usize) -> Option<FieldWindow> {
        let pattern: IUIAutomationTextPattern = element.GetCurrentPattern(UIA_TextPatternId).ok()?.cast().ok()?;
        let caret: Option<IUIAutomationTextRange> = match pattern.GetSelection() {
            Ok(ranges) if ranges.Length().unwrap_or(0) > 0 => ranges.GetElement(0).ok(),
            _ => element
                .GetCurrentPattern(UIA_TextPattern2Id)
                .ok()
                .and_then(|p| p.cast::<IUIAutomationTextPattern2>().ok())
                .and_then(|p2| {
                    let mut active = BOOL(0);
                    p2.GetCaretRange(&mut active).ok()
                }),
        };
        let document = pattern.DocumentRange().ok()?;
        if let Ok(v) = caret.as_ref().unwrap_or(&document).GetAttributeValue(UIA_IsReadOnlyAttributeId) {
            if bool::try_from(&v).unwrap_or(false) {
                return None;
            }
        }
        let n = i32::try_from(limit.saturating_add(1)).unwrap_or(i32::MAX);
        let raw = document.GetText(n).ok()?.to_string();
        let to_end = raw.chars().count() <= limit;
        let text = tidy(&raw);
        let caret = caret
            .and_then(|range| {
                let before = document.Clone().ok()?;
                before
                    .MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start)
                    .ok()?;
                Some(tidy(&before.GetText(n).ok()?.to_string()).len())
            })
            .filter(|&c| c <= text.len() && text.is_char_boundary(c))
            .unwrap_or(text.len());
        Some(FieldWindow {
            text,
            caret,
            from_start: true,
            to_end,
        })
    }

    /// ValuePattern: a text field's whole value, if it isn't read-only, and
    /// the caret in it as [`value_context`] finds it (else its end).
    unsafe fn whole_value(element: &IUIAutomationElement, limit: usize) -> Option<FieldWindow> {
        let pattern = editable_value(element)?;
        let (raw, at) = value_selection(element, pattern.CurrentValue().ok()?.to_string(), super::WATCH_TIMEOUT_MS);
        let caret = at.map(|(start, _)| tidy(&raw[..start]).len());
        let value = tidy(&raw);
        let to_end = value.chars().count() <= limit;
        let text: String = if to_end { value } else { value.chars().take(limit + 1).collect() };
        Some(FieldWindow {
            caret: caret
                .filter(|&c| c <= text.len() && text.is_char_boundary(c))
                .unwrap_or(text.len()),
            text,
            from_start: true,
            to_end,
        })
    }
}

#[cfg(windows)]
use uia::uia_selected_text;

#[cfg(windows)]
fn read_focused_context(pid: Option<u32>) -> Option<FieldContext> {
    uia::focused_context(pid)
}

#[cfg(not(windows))]
fn uia_selected_text() -> Option<String> {
    None
}

#[cfg(not(windows))]
fn read_focused_context(_pid: Option<u32>) -> Option<FieldContext> {
    None
}

/// Whether a paste reached a Win32 edit control, from its length and
/// selection before and after.
#[cfg(test)]
mod paste_check_tests {
    use super::*;

    fn at(len: usize, start: usize, end: usize) -> EditState {
        EditState { len, sel: (start, end) }
    }

    #[test]
    fn a_paste_that_landed_changed_the_field() {
        // Typed at the caret: longer, the caret after it.
        assert_eq!(paste_verdict(at(10, 4, 4), Some(at(25, 19, 19)), false), Some(Landed::Yes));
        // A selection replaced by text as long as it: same length, the caret moved.
        assert_eq!(paste_verdict(at(10, 2, 7), Some(at(10, 7, 7)), false), Some(Landed::Yes));
        // …and a selection replaced by the very same words: the selection is gone.
        assert_eq!(paste_verdict(at(5, 0, 5), Some(at(5, 5, 5)), true), Some(Landed::Yes));
        // An empty field.
        assert_eq!(paste_verdict(at(0, 0, 0), Some(at(9, 9, 9)), false), Some(Landed::Yes));
    }

    #[test]
    fn a_field_unchanged_for_the_whole_wait_didnt_get_it() {
        // Too early to say: read again.
        assert_eq!(paste_verdict(at(10, 4, 4), Some(at(10, 4, 4)), false), None);
        // The wait is over and nothing moved: it never arrived.
        assert_eq!(paste_verdict(at(10, 4, 4), Some(at(10, 4, 4)), true), Some(Landed::No));
        assert_eq!(paste_verdict(at(0, 0, 0), Some(at(0, 0, 0)), true), Some(Landed::No));
    }

    #[test]
    fn a_read_that_fails_decides_nothing() {
        // The app didn't answer in time, or the control lost the focus:
        // never typed on a guess, whenever it happens.
        assert_eq!(paste_verdict(at(10, 4, 4), None, false), Some(Landed::Unknown));
        assert_eq!(paste_verdict(at(10, 4, 4), None, true), Some(Landed::Unknown));
    }
}

/// What a field's text over UI Automation says about whether it's empty
/// (Chromium's empty boxes, rich editors' placeholders).
#[cfg(test)]
mod focus_tests {
    use super::*;

    fn el(control_type: i32, framework: &str) -> FocusFacts {
        FocusFacts { pid: 7, control_type, framework: framework.into(), ..Default::default() }
    }

    #[test]
    fn a_text_box_or_anything_in_one_takes_text() {
        let edit = FocusFacts { value: true, ..el(UIA_EDIT, "Win32") };
        assert_eq!(editable(&[edit], 7), Editable::Yes);
        // A web text box: a TextPattern that isn't read-only.
        let web = FocusFacts { text: true, value: true, ..el(UIA_EDIT, "Chrome") };
        assert_eq!(editable(std::slice::from_ref(&web), 7), Editable::Yes);
        // A spreadsheet's cell: a ValuePattern that takes a value.
        let cell = FocusFacts { value: true, ..el(50029, "Win32") };
        assert_eq!(editable(&[cell], 7), Editable::Yes);
        // An editor's inner element with no text of its own, in its field.
        let inner = el(50025, "Chrome");
        assert_eq!(editable(&[inner, web], 7), Editable::Yes);
        // An edit control that says nothing more.
        assert_eq!(editable(&[el(UIA_EDIT, "WinForm")], 7), Editable::Yes);
    }

    #[test]
    fn a_button_a_list_or_read_only_text_takes_none() {
        assert_eq!(editable(&[el(UIA_BUTTON, "WinForm"), el(50032, "WinForm")], 7), Editable::No);
        assert_eq!(editable(&[el(UIA_LIST_ITEM, "DirectUI")], 7), Editable::No); // Explorer's files
        assert_eq!(editable(&[el(UIA_BUTTON, "Chrome")], 7), Editable::No);
        assert_eq!(editable(&[el(UIA_TAB_ITEM, "XAML")], 7), Editable::No);
        // A web page with no box focused: its document, read-only.
        let page = FocusFacts { text: true, text_read_only: true, value: true, value_read_only: true, ..el(UIA_DOCUMENT, "Chrome") };
        assert_eq!(editable(&[page], 7), Editable::No);
        // A read-only text box, by its legacy state alone.
        let viewer = FocusFacts { value: true, legacy_read_only: true, ..el(UIA_EDIT, "Win32") };
        assert_eq!(editable(&[viewer], 7), Editable::No);
    }

    #[test]
    fn what_ui_automation_cant_show_is_never_a_no() {
        // A game or a canvas: a pane or a custom control.
        assert_eq!(editable(&[el(50033, "Win32")], 7), Editable::Unknown);
        assert_eq!(editable(&[el(50025, "Chrome")], 7), Editable::Unknown);
        assert_eq!(editable(&[el(50032, "Win32")], 7), Editable::Unknown);
        // A framework that may hide its fields (Java, Qt, none said).
        assert_eq!(editable(&[el(UIA_BUTTON, "")], 7), Editable::Unknown);
        assert_eq!(editable(&[el(UIA_BUTTON, "Qt")], 7), Editable::Unknown);
        // A document that doesn't say it's read-only takes text.
        let doc = FocusFacts { text: true, ..el(UIA_DOCUMENT, "Win32") };
        assert_eq!(editable(&[doc], 7), Editable::Yes);
        // The focus in another app, or no answer.
        assert_eq!(editable(&[FocusFacts { pid: 9, ..el(UIA_BUTTON, "Win32") }], 7), Editable::Unknown);
        assert_eq!(editable(&[], 7), Editable::Unknown);
        // A field in another process above a button doesn't count.
        let other = FocusFacts { pid: 9, value: true, ..el(UIA_EDIT, "Win32") };
        assert_eq!(editable(&[el(UIA_BUTTON, "Win32"), other], 7), Editable::No);
    }
}

/// Where the focus goes back to before a dictation's paste.
#[cfg(test)]
mod refocus_tests {
    use super::*;

    #[test]
    fn a_click_on_something_that_takes_no_text_is_undone_for_the_paste() {
        assert_eq!(refocus(true, FocusNow::NoBox), Refocus::PutBack);
        // Started on a button: there's no box to go back to.
        assert_eq!(refocus(false, FocusNow::NoBox), Refocus::Stay);
    }

    #[test]
    fn another_text_box_is_where_the_person_wants_it() {
        assert_eq!(refocus(true, FocusNow::OtherBox), Refocus::Follow);
        assert_eq!(refocus(false, FocusNow::OtherBox), Refocus::Follow);
    }

    #[test]
    fn still_there_another_app_or_no_telling_leaves_the_focus_alone() {
        for started_in_text in [true, false] {
            for now in [FocusNow::Same, FocusNow::Unknown, FocusNow::Elsewhere] {
                assert_eq!(refocus(started_in_text, now), Refocus::Stay, "{started_in_text} {now:?}");
            }
        }
    }

    #[test]
    fn without_a_keeper_nothing_is_asked() {
        // No such window: nothing kept, so the paste asks nothing and never waits.
        keep_focus(Some(1));
        assert_eq!(restore_focus(Some(1)), None);
        assert_eq!(restore_focus(None), None);
        follow_focus();
    }
}

#[cfg(test)]
mod field_text_tests {
    use super::*;

    /// A value that must not be asked for.
    fn unasked() -> Option<String> {
        panic!("the value was asked for")
    }

    #[test]
    fn bare_drops_markers_and_folds_whitespace() {
        assert_eq!(bare("  Type a\n message \u{FEFF}\n"), "Type a message");
        assert_eq!(bare("\u{FFFC}"), "");
        assert_eq!(bare("\u{200B}\n\t "), "");
        assert_eq!(bare("one\u{FFFC}two"), "one two");
    }

    /// A field's labels (its Name, then LabeledBy's), as `labels` reads them.
    fn l(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn what_chromium_reads_for_an_empty_box_is_empty() {
        // A textarea or contenteditable labelled by aria-label: its label
        // as its text, an empty value.
        assert!(stands_in_for_empty("Type a message", &l(&["Type a message"]), "", || Some(String::new())));
        // No label: U+FFFC. With a placeholder: U+FFFC, the placeholder as
        // name and HelpText.
        assert!(stands_in_for_empty("\u{FFFC}", &l(&[""]), "", unasked));
        assert!(stands_in_for_empty("\u{FFFC}", &l(&["Write a reply"]), "Write a reply", unasked));
        // A rich editor's empty line (Gmail's body, WhatsApp's Lexical box).
        assert!(stands_in_for_empty("\n", &l(&["Message Body"]), "", unasked));
        // Slate's placeholder inside the editor, beside its U+FEFF line: the
        // value has the same words, but the marker gives it away.
        assert!(stands_in_for_empty(
            "Message #random\n\u{FEFF}\n",
            &l(&["Message #random"]),
            "",
            || Some("Message #random\n\u{FEFF}\n".into())
        ));
        // A field showing its placeholder (HelpText) as its text.
        assert!(stands_in_for_empty("Search mail", &l(&[""]), "Search mail", unasked));
        // Exactly the label, and no ValuePattern to say otherwise.
        assert!(stands_in_for_empty("Notes ", &l(&["Notes"]), "", || None));
    }

    #[test]
    fn the_element_labelling_a_field_is_its_label_too() {
        // Its own Name says something else (or nothing); the label next to
        // it (LabeledBy, a dialog's static text, an HTML <label>) is what
        // an empty field shows.
        assert!(stands_in_for_empty("Your reply", &l(&["", "Your reply"]), "", || None));
        assert!(stands_in_for_empty("Your reply:", &l(&["Reply box", "Your reply:"]), "", || Some(String::new())));
        // Really typed: the value says so.
        assert!(!stands_in_for_empty("Your reply", &l(&["", "Your reply"]), "", || Some("Your reply".into())));
        // Neither label: real text, nothing asked.
        assert!(!stands_in_for_empty("Hello", &l(&["Reply box", "Your reply"]), "", unasked));
    }

    #[test]
    fn real_text_is_never_a_stand_in() {
        assert!(!stands_in_for_empty("Hello world", &l(&["Notes"]), "Write here", unasked));
        assert!(!stands_in_for_empty("\u{FFFC} a photo", &l(&[""]), "", unasked));
        // The label's words, really typed: the value says they're there.
        assert!(!stands_in_for_empty("Notes", &l(&["Notes"]), "", || Some("Notes".into())));
        // Part of the label isn't the label.
        assert!(!stands_in_for_empty("Type a", &l(&["Type a message"]), "", unasked));
        // No label, no placeholder: nothing to match.
        assert!(!stands_in_for_empty("Hi", &[], "", unasked));
    }

    fn el(pid: u32, text: bool, value: bool, read_only: bool) -> Offers {
        Offers { pid, text, value, read_only }
    }

    #[test]
    fn the_focused_field_itself_comes_first() {
        assert_eq!(pick_field(&[el(7, true, false, false)]), Some(0));
        assert_eq!(pick_field(&[el(7, false, true, false)]), Some(0));
        // Read-only (by its ValuePattern or its legacy state): never read,
        // and not the field above it either.
        assert_eq!(pick_field(&[el(7, true, false, true), el(7, true, false, false)]), None);
        assert_eq!(pick_field(&[]), None);
    }

    #[test]
    fn an_element_with_no_text_is_read_through_the_field_its_in() {
        // An editor's inner element (a span, a pane) → its parent editor.
        let chain = [el(7, false, false, false), el(7, false, false, false), el(7, true, false, false)];
        assert_eq!(pick_field(&chain), Some(2));
        // Up to three levels…
        let deep = [
            el(7, false, false, false),
            el(7, false, false, false),
            el(7, false, false, false),
            el(7, true, false, false),
        ];
        assert_eq!(pick_field(&deep), Some(3));
        // …and no further.
        let deeper = [
            el(7, false, false, false),
            el(7, false, false, false),
            el(7, false, false, false),
            el(7, false, false, false),
            el(7, true, false, false),
        ];
        assert_eq!(pick_field(&deeper), None);
    }

    #[test]
    fn the_walk_stays_in_the_app_and_stops_at_a_viewer() {
        // The parent is another process's window (a host): never.
        assert_eq!(pick_field(&[el(7, false, false, false), el(9, true, false, false)]), None);
        // A read-only part of a page (static text) → its document: no.
        assert_eq!(pick_field(&[el(7, false, false, true), el(7, true, false, false)]), None);
        // A read-only field found on the way up: no.
        assert_eq!(pick_field(&[el(7, false, false, false), el(7, true, true, true)]), None);
    }

    #[test]
    fn the_legacy_state_says_read_only() {
        // STATE_SYSTEM_READONLY (0x40), among others (focusable 0x100000,
        // focused 0x4).
        assert!(legacy_read_only(0x40));
        assert!(legacy_read_only(0x0010_0044));
        assert!(!legacy_read_only(0x0010_0004));
        assert!(!legacy_read_only(0));
    }

    #[test]
    fn web_editors_say_they_are_empty_in_their_classes() {
        assert!(class_says_empty("ql-editor ql-blank"));
        assert!(class_says_empty("is-empty is-editor-empty"));
        assert!(class_says_empty("ck ck-placeholder"));
        assert!(class_says_empty("placeholder"));
        assert!(!class_says_empty("ql-editor"));
        assert!(!class_says_empty("ProseMirror"));
        assert!(!class_says_empty("has-placeholder"));
        assert!(!class_says_empty("RichEditD2DPT"));
        assert!(!class_says_empty("WindowsForms10.EDIT.app.0.34f5582_r6_ad1"));
        assert!(!class_says_empty(""));
    }
}

/// Reads real Win32 text fields over UI Automation, the way a dictation
/// reads the field it pastes into: a plain EDIT control and a RichEdit (its
/// own TextPattern), created hidden on a thread of their own that pumps
/// messages (the UIA proxies talk to them by message). Never shown, never
/// focused, never anyone else's app. `#[ignore]`d — it makes windows, which
/// a CI runner may not allow:
/// `cargo test --lib -- --ignored reads_real_text_fields`.
#[cfg(all(test, windows))]
mod tests {
    use std::ffi::c_void;
    use std::sync::mpsc;

    use super::{paste_verdict, EditState, Landed};
    use crate::pasting::smart_insert::FieldContext;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    /// Laid out as `input_hook`'s MSG (one `GetMessageW` declaration).
    #[repr(C)]
    struct Msg {
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
    }

    #[link(name = "user32")]
    extern "system" {
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: *mut c_void,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> *mut c_void;
        fn DestroyWindow(hwnd: *mut c_void) -> i32;
        fn SendMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
        fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
    }

    const WS_POPUP: u32 = 0x8000_0000;
    const WS_CHILD: u32 = 0x4000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const ES_MULTILINE: u32 = 0x0004;
    const ES_PASSWORD: u32 = 0x0020;
    const ES_READONLY: u32 = 0x0800;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const EM_SETSEL: u32 = 0x00B1;
    const EM_REPLACESEL: u32 = 0x00C2;
    const WM_QUIT: u32 = 0x0012;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A hidden text field on its own message-pumping thread (and, for the
    /// walk up from the focus, a child window inside it).
    struct Field {
        hwnd: isize,
        child: isize,
        thread: u32,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl Field {
        fn new(class: &str, style: u32, text: &str, selection: (usize, usize)) -> Option<Field> {
            Field::make(class, style, text, selection, None)
        }

        /// The field with a `child_class` window inside it (no text of its own).
        fn with_child(class: &str, style: u32, text: &str, selection: (usize, usize), child_class: &str) -> Option<Field> {
            Field::make(class, style, text, selection, Some(child_class))
        }

        fn make(class: &str, style: u32, text: &str, selection: (usize, usize), child_class: Option<&str>) -> Option<Field> {
            let (tx, rx) = mpsc::channel();
            let (class, text) = (wide(class), wide(text));
            let child_class = child_class.map(wide);
            let join = std::thread::spawn(move || unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    text.as_ptr(),
                    WS_POPUP | style,
                    -20_000,
                    -20_000,
                    600,
                    300,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                if hwnd.is_null() {
                    let _ = tx.send(None);
                    return;
                }
                SendMessageW(hwnd, EM_SETSEL, selection.0, selection.1 as isize);
                let child = match &child_class {
                    Some(c) => CreateWindowExW(
                        0,
                        c.as_ptr(),
                        wide("").as_ptr(),
                        WS_CHILD | WS_VISIBLE,
                        0,
                        0,
                        40,
                        20,
                        hwnd,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    ),
                    None => std::ptr::null_mut(),
                };
                let _ = tx.send(Some((hwnd as isize, child as isize, GetCurrentThreadId())));
                let mut msg: Msg = std::mem::zeroed();
                while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                    DispatchMessageW(&msg);
                }
                DestroyWindow(hwnd);
            });
            let (hwnd, child, thread) = rx.recv().ok()??;
            Some(Field {
                hwnd,
                child,
                thread,
                join: Some(join),
            })
        }

        fn context(&self) -> Option<FieldContext> {
            super::uia::window_context(self.hwnd)
        }
    }

    impl Drop for Field {
        fn drop(&mut self) {
            unsafe {
                PostThreadMessageW(self.thread, WM_QUIT, 0, 0);
            }
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    /// The ValuePattern reads with the caret asked of the control, as a
    /// WinForms text box (no TextPattern) is read: EM_GETSEL must say `sel`;
    /// → (before, selection, after) and the watch's (text, caret).
    fn read_by_value(field: &Field, class: &str, rich: bool, sel: (usize, usize)) -> ((String, String, String), (String, usize)) {
        let (ctx, asked, watch) = super::uia::window_value_reads(field.hwnd);
        assert_eq!(asked, Some((sel.0, sel.1, rich)), "{class}: EM_GETSEL");
        let ctx = ctx.expect("the context from the value");
        let watch = watch.expect("the watch's read of the value");
        assert!(watch.from_start && watch.to_end, "{class}: the whole value");
        ((ctx.before, ctx.selection, ctx.after), (watch.text, watch.caret))
    }

    fn check(class: &str, rich: bool) {
        let text = "Hello world. we went home";
        // A caret after "Hello world. ".
        let field = Field::new(class, ES_MULTILINE, text, (13, 13)).expect("a field");
        let ctx = field.context().expect("the context");
        assert_eq!(ctx.before, "Hello world. ", "{class}: before");
        assert_eq!(ctx.after, "we went home", "{class}: after");
        assert_eq!(ctx.selection, "", "{class}: selection");
        let by_value = |field: &Field, sel| read_by_value(field, class, rich, sel);
        assert_eq!(
            by_value(&field, (13, 13)),
            (("Hello world. ".into(), String::new(), "we went home".into()), (text.into(), 13)),
            "{class}: the value, the caret mid-text"
        );
        // The post-paste watch's read: the whole field, the caret, both ends.
        let w = super::uia::window_watch(field.hwnd, 2_000).expect("the watch's read");
        assert_eq!(w.text, text, "{class}: the watch's text");
        assert_eq!(w.caret, 13, "{class}: the watch's caret");
        assert!(w.from_start && w.to_end, "{class}: the whole field");
        drop(field);

        // "world" selected.
        let field = Field::new(class, ES_MULTILINE, text, (6, 11)).expect("a field");
        let ctx = field.context().expect("the context");
        assert_eq!(ctx.before, "Hello ", "{class}: before a selection");
        assert_eq!(ctx.selection, "world", "{class}: the selection");
        assert_eq!(ctx.after, ". we went home", "{class}: after a selection");
        let w = super::uia::window_watch(field.hwnd, 2_000).expect("the watch's read");
        assert_eq!((w.text.as_str(), w.caret), (text, 6), "{class}: the watch, a selection");
        assert_eq!(
            by_value(&field, (6, 11)),
            (("Hello ".into(), "world".into(), ". we went home".into()), (text.into(), 6)),
            "{class}: the value, a selection"
        );
        // The paste check's read (its length and selection, never its
        // text), before and after "a paste" replaces the selection.
        let before = super::uia::edit_state(field.hwnd).expect("the paste check's read");
        assert_eq!(before, EditState { len: text.len(), sel: (6, 11) }, "{class}: the paste check");
        let pasted = wide("there");
        unsafe { SendMessageW(field.hwnd as _, EM_REPLACESEL, 1, pasted.as_ptr() as isize) };
        let after = super::uia::edit_state(field.hwnd).expect("the paste check's read, after");
        assert_eq!(after, EditState { len: text.len(), sel: (11, 11) }, "{class}: the paste check, after");
        assert_eq!(paste_verdict(before, Some(after), false), Some(Landed::Yes), "{class}: it landed");
        drop(field);

        // Over a line break: a plain EDIT counts "\r\n" as two, a RichEdit
        // its paragraph mark as one. The caret after "we ".
        let lines = "Hello world.\r\nwe went home";
        let caret = if rich { 16 } else { 17 };
        let field = Field::new(class, ES_MULTILINE, lines, (caret, caret)).expect("a field");
        let ((before, selection, after), (watched, at)) = by_value(&field, (caret, caret));
        // (The value's own line break: "\r\n" from an EDIT, "\r" from a RichEdit.)
        let line_break = if rich { "\r" } else { "\r\n" };
        assert_eq!(before, format!("Hello world.{line_break}we "), "{class}: before, over a line break");
        assert_eq!((selection.as_str(), after.as_str()), ("", "went home"), "{class}: after a line break");
        assert_eq!((watched.as_str(), at), ("Hello world.\nwe went home", 16), "{class}: the watch over a line break");
        drop(field);

        // Long text: 500 characters each side at most, cut between words.
        let long = "word ".repeat(300);
        let field = Field::new(class, ES_MULTILINE, &long, (750, 750)).expect("a field");
        let ctx = field.context().expect("the context");
        assert!(ctx.before.chars().count() <= 500, "{class}: {} before", ctx.before.len());
        assert!(ctx.after.chars().count() <= 500, "{class}: {} after", ctx.after.len());
        assert!(ctx.before.ends_with("word "), "{class}: before ends at the caret");
        assert!(ctx.before.starts_with(' '), "{class}: before cut between words: {:?}", &ctx.before[..10]);
        assert!(ctx.after.starts_with("word"), "{class}: after starts at the caret");
        // The watch's read of a field longer than it takes: its start, and
        // it says so (too long to watch).
        let w = super::uia::window_watch(field.hwnd, 100).expect("the watch's read");
        assert!(w.from_start && !w.to_end, "{class}: cut short");
        assert_eq!(w.text.chars().count(), 101, "{class}: the first 101 characters");
        assert!(w.text.starts_with("word word"), "{class}: {}", w.text.len());
        let w = super::uia::window_watch(field.hwnd, 2_000).expect("the watch's read");
        assert!(w.to_end && w.text == long && w.caret == 750, "{class}: the whole field");
        drop(field);

        // An element with no text of its own inside the field (a button in
        // it): read through the field it's part of.
        let field = Field::with_child(class, ES_MULTILINE, text, (13, 13), "BUTTON").expect("a field");
        assert_ne!(field.child, 0, "{class}: the child window");
        let ctx = super::uia::window_context(field.child).expect("the field's context, from inside it");
        assert_eq!((ctx.before.as_str(), ctx.after.as_str()), ("Hello world. ", "we went home"), "{class}: from a child");
        drop(field);
        // …but not through a read-only one.
        let field = Field::with_child(class, ES_MULTILINE | ES_READONLY, text, (13, 13), "BUTTON").expect("a field");
        assert_eq!(super::uia::window_context(field.child), None, "{class}: a read-only field, from a child");
        drop(field);

        // Never a password box; never a read-only field.
        let field = Field::new(class, ES_PASSWORD, "secret", (6, 6)).expect("a field");
        assert_eq!(field.context(), None, "{class}: a password box");
        assert_eq!(super::uia::window_watch(field.hwnd, 2_000), None, "{class}: the watch, a password box");
        drop(field);
        let field = Field::new(class, ES_MULTILINE | ES_READONLY, text, (13, 13)).expect("a field");
        assert_eq!(field.context(), None, "{class}: a read-only field");
        assert_eq!(super::uia::window_watch(field.hwnd, 2_000), None, "{class}: the watch, read-only");
    }

    #[test]
    #[ignore = "makes (hidden) windows: cargo test --lib -- --ignored reads_real_text_fields"]
    fn reads_real_text_fields() {
        check("EDIT", false);
        // RichEdit 4.1+ has a TextPattern of its own.
        unsafe { LoadLibraryW(wide("Msftedit.dll").as_ptr()) };
        check("RICHEDIT50W", true);
    }
}

/// The caret of a field with only a ValuePattern (a WinForms text box):
/// which windows are asked, and their answer placed in the value.
#[cfg(test)]
mod caret_tests {
    use super::{edit_class, shorten_selection, units_to_bytes};

    #[test]
    fn edit_controls_by_class() {
        assert_eq!(edit_class("Edit"), Some(false));
        assert_eq!(edit_class("EDIT"), Some(false));
        assert_eq!(edit_class("WindowsForms10.EDIT.app.0.1a4c162_r8_ad1"), Some(false));
        assert_eq!(edit_class("RichEdit20W"), Some(true));
        assert_eq!(edit_class("RICHEDIT50W"), Some(true));
        assert_eq!(edit_class("WindowsForms10.RichEdit20W.app.0.1a4c162_r8_ad1"), Some(true));
        // Not an edit control, or one that counts differently: never asked.
        for class in [
            "RICHEDIT",
            "WindowsForms10.Window.8.app.0.1a4c162_r8_ad1",
            "WindowsForms10.",
            "Chrome_RenderWidgetHostHWND",
            "ComboBox",
            "Scintilla",
            "TextEditPane",
            "EditWnd",
            "",
        ] {
            assert_eq!(edit_class(class), None, "{class}");
        }
    }

    #[test]
    fn units_placed_in_the_value() {
        // "I want | please".
        assert_eq!(units_to_bytes("I want  please", 7, 7, false), Some((7, 7)));
        assert_eq!(units_to_bytes("I want  please", 0, 0, false), Some((0, 0)));
        assert_eq!(units_to_bytes("I want  please", 14, 14, false), Some((14, 14)));
        // A selection.
        assert_eq!(units_to_bytes("Hello world", 6, 11, false), Some((6, 11)));
        // A plain EDIT counts "\r\n" as two units; a RichEdit any break as one.
        let v = "One.\r\nTwo";
        assert_eq!(units_to_bytes(v, 6, 6, false), Some((6, 6)));
        assert_eq!(units_to_bytes(v, 5, 5, true), Some((6, 6)));
        assert_eq!(units_to_bytes("One.\nTwo", 5, 5, true), Some((5, 5)));
        assert_eq!(units_to_bytes("One.\rTwo", 5, 5, true), Some((5, 5)));
        assert_eq!(units_to_bytes(v, 9, 9, false), Some((9, 9)));
        assert_eq!(units_to_bytes(v, 8, 8, true), Some((9, 9)));
        // Never between "\r" and "\n": past the pair.
        assert_eq!(units_to_bytes(v, 5, 5, false), Some((6, 6)));
        // UTF-16 units, not bytes or chars: "é" is one unit, "😀" two.
        assert_eq!(units_to_bytes("café | ok", 5, 5, false), Some((6, 6)));
        assert_eq!(units_to_bytes("😀 hi", 3, 3, false), Some((5, 5)));
        // Past the end of the text: the control and the value disagree.
        assert_eq!(units_to_bytes("short", 9, 9, false), None);
        assert_eq!(units_to_bytes("short", 2, 9, false), None);
        assert_eq!(units_to_bytes("", 0, 0, false), Some((0, 0)));
    }

    #[test]
    fn a_long_selection_is_its_ends() {
        assert_eq!(shorten_selection("world"), "world");
        let long = format!("{}{}", "a".repeat(300), "b".repeat(300));
        let short = shorten_selection(&long);
        assert_eq!(short, format!("{} … {}", "a".repeat(120), "b".repeat(120)));
    }
}
