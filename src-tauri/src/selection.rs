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
//!    start (`TextPattern`, else `ValuePattern` — with `EM_GETSEL` for the
//!    caret in a Win32 text box such as WinForms'), so `smart_insert` can fit
//!    the dictation to them. Only the focused field's own text: Chromium's
//!    ranges run on into the page around a text box, so they're kept inside
//!    the field's document range; and an empty field reads as empty,
//!    whatever stands in for its text (its label, its placeholder, U+FFFC, a
//!    web editor's CSS placeholder — [`stands_in_for_empty`],
//!    [`class_says_empty`]). The paste never waits for it: a read that hasn't
//!    finished by then (or failed, or found a password box) leaves the
//!    dictation exactly as it would be without it. The text is kept in memory
//!    for that one dictation only and never logged (lengths only).
//! 3. **The field after the paste** ([`WatchedField`], for `learn.rs`): the
//!    element a dictation was just pasted into, held for about a minute and
//!    read whole (up to 15,000 characters, focused or not) to see whether a
//!    word Yap typed gets corrected. Same rules: that window's process only,
//!    never a password box or a read-only field, never in a test run,
//!    time-boxed calls, in memory only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::smart_insert::FieldContext;

/// How long one UI Automation call may take for the cursor context before
/// it's given up on (connection and transaction timeouts): an app that's
/// hung or slow never holds a dictation up.
const CONTEXT_TIMEOUT_MS: u32 = 200;
/// The same for edit mode's selection, which the pipeline waits for. Long
/// enough for a big selection in Word, far below UIA's own 20 s.
const SELECTION_TIMEOUT_MS: u32 = 2_000;
/// The same for the post-paste watch's reads (a thread of its own).
const WATCH_TIMEOUT_MS: u32 = 300;

/// A field whose whole text is longer than this holds real text: a label or
/// a placeholder standing in for an empty field is never this long.
const STAND_IN_CHARS: usize = 200;

/// Characters that take a place in a field's text without anything being
/// written there: U+FFFC (Chromium's text for an empty text box, or an
/// embedded object) and the zero-width ones (Slate's U+FEFF in an empty
/// line).
fn is_marker(c: char) -> bool {
    matches!(c, '\u{FFFC}' | '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}')
}

/// `s` as it reads: no markers, whitespace runs as one space, trimmed.
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
/// shown only while it's empty), is empty. Exactly its `name` (a label, which
/// someone could also type) is empty unless the field's `value`
/// (ValuePattern, asked only then; `None` = it has none) says those words
/// are really there: an empty Chromium box's value is "", and a value of
/// the same words with a marker in the text is Slate's placeholder.
fn stands_in_for_empty(text: &str, name: &str, placeholder: &str, value: impl FnOnce() -> Option<String>) -> bool {
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
    if !same(name) {
        return false;
    }
    match value() {
        None => true,
        Some(v) => bare(&v).is_empty() || text.contains(is_marker),
    }
}

/// A web editor's class list saying the element shows no text of its own
/// (its text is a CSS placeholder, `::before { content: attr(…) }`, which
/// Chromium reads as text): Quill's `ql-blank` (Slack's message box),
/// ProseMirror/Tiptap's `is-empty` / `is-editor-empty` (Claude, ChatGPT),
/// CKEditor's `ck-placeholder` (Teams), and a plain `placeholder`.
fn class_says_empty(class: &str) -> bool {
    const EMPTY: &[&str] = &["ql-blank", "is-empty", "is-editor-empty", "ck-placeholder", "placeholder"];
    class.split_whitespace().any(|token| EMPTY.contains(&token))
}

/// A Win32 text box (the `Edit` class, or WinForms' `WindowsForms10.EDIT.…`
/// superclass of it), which answers `EM_GETSEL`.
fn is_edit_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("edit")
        || class
            .strip_prefix("WindowsForms10.")
            .is_some_and(|rest| rest.split('.').next().is_some_and(|base| base.eq_ignore_ascii_case("edit")))
}

/// `value` cut at a text box's selection, `start`…`end` in UTF-16 units
/// (what `EM_GETSEL` gives): before, selected, after. `None` when the
/// positions don't fit the value (it changed meanwhile, or isn't the box's
/// text) or would split a character.
fn split_utf16(value: &str, start: usize, end: usize) -> Option<(String, String, String)> {
    let units: Vec<u16> = value.encode_utf16().collect();
    if start > end || end > units.len() {
        return None;
    }
    let part = |p: &[u16]| String::from_utf16(p).ok();
    Some((part(&units[..start])?, part(&units[start..end])?, part(&units[end..])?))
}

/// Capture the current selection for edit mode: UIA first, Ctrl+C fallback
/// when `copy_fallback` (not in a terminal, where Ctrl+C with nothing
/// selected interrupts the program running there).
///
/// `target_hwnd` is the foreground window captured at hotkey-press (used to
/// aim the Ctrl+C fallback). Returns `None` if nothing is selected.
pub fn capture_selection(target_hwnd: Option<isize>, copy_fallback: bool) -> Option<String> {
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
    match crate::text_injector::selection_via_copy(target_hwnd) {
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
}

static SESSION: Mutex<Session> = Mutex::new(Session {
    id: 0,
    reading: Reading::Off,
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
    let hwnd = target_hwnd.filter(|&h| read && crate::realtest::permits(Some(h)));
    {
        let mut s = session();
        s.id = id;
        s.reading = if hwnd.is_some() { Reading::Pending } else { Reading::Off };
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

/// Read the text around the caret in the focused field now (on this thread,
/// each UI Automation call time-boxed). `target_hwnd`: the field must be in
/// that window's process, or it isn't the one Yap will paste into. `None`
/// when the field can't be read, is a password box, or is read-only.
pub fn read_context(target_hwnd: Option<isize>) -> Option<FieldContext> {
    let started = std::time::Instant::now();
    let pid = target_hwnd.and_then(crate::text_injector::window_process);
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
        if !crate::realtest::permits(Some(target_hwnd)) {
            return None;
        }
        #[cfg(windows)]
        {
            let pid = crate::text_injector::window_process(target_hwnd)?;
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

#[cfg(windows)]
mod uia {
    use windows::core::{Interface, BOOL};
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement,
        IUIAutomationTextPattern, IUIAutomationTextPattern2, IUIAutomationTextRange,
        IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
        TextUnit_Character, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
        UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId, UIA_TextPattern2Id, UIA_TextPatternId,
        UIA_ValuePatternId,
    };

    use super::FieldWindow;
    use crate::smart_insert::{head_chars, tail_chars, FieldContext, CONTEXT_CHARS};

    /// A selection longer than this is read as its start and its end (all
    /// that fitting needs: the first word, the last mark, the spaces).
    const SELECTION_CHARS: usize = 400;
    const SELECTION_ENDS: usize = 120;

    /// COM on this thread for as long as it lives; undone on drop when this
    /// call did it. Declared first in a function, it drops after every COM
    /// object made there.
    struct Com(bool);

    impl Com {
        fn init() -> Option<Com> {
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
    fn automation(timeout_ms: u32) -> Option<IUIAutomation> {
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
            let element = automation.GetFocusedElement().ok()?;

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

    /// The text around the caret in `element`, a text field.
    unsafe fn element_context(automation: &IUIAutomation, element: &IUIAutomationElement) -> Option<FieldContext> {
        // Never a password box (and if it can't say, don't risk it).
        if element.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        text_context(automation, element).or_else(|| value_context(element, super::CONTEXT_TIMEOUT_MS))
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

    /// [`value_context`] for a window's own element (the tests' edit
    /// control: the ValuePattern path, as for a WinForms text box).
    #[cfg(test)]
    pub fn window_value_context(hwnd: isize) -> Option<FieldContext> {
        let _com = Com::init()?;
        let automation = automation(super::CONTEXT_TIMEOUT_MS)?;
        unsafe {
            let element = automation
                .ElementFromHandle(windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void))
                .ok()?;
            value_context(&element, super::CONTEXT_TIMEOUT_MS)
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

    /// [`super::stands_in_for_empty`] for a TextPattern field showing `text`.
    unsafe fn text_stands_in(element: &IUIAutomationElement, text: &str) -> bool {
        let name = element.CurrentName().map(|b| b.to_string()).unwrap_or_default();
        let placeholder = element.CurrentHelpText().map(|b| b.to_string()).unwrap_or_default();
        super::stands_in_for_empty(text, &name, &placeholder, || {
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

    /// [`selection_text`] for a selection Yap has as a string.
    fn shorten_selection(text: &str) -> String {
        let n = text.chars().count();
        if n <= SELECTION_CHARS {
            return text.to_string();
        }
        let head: String = text.chars().take(SELECTION_ENDS).collect();
        let tail: String = text.chars().skip(n - SELECTION_ENDS).collect();
        format!("{head} … {tail}")
    }

    /// ValuePattern (fields with no TextPattern, e.g. some Electron inputs,
    /// WinForms' TextBox): the whole value, the caret where a Win32 text box
    /// says it is ([`edit_selection`]), else taken to be at the end, where
    /// it is after typing. Text fields only, never read-only ones; a field
    /// showing only its placeholder is empty.
    unsafe fn value_context(element: &IUIAutomationElement, timeout_ms: u32) -> Option<FieldContext> {
        let kind = element.CurrentControlType().ok()?;
        if ![UIA_EditControlTypeId, UIA_DocumentControlTypeId, UIA_ComboBoxControlTypeId].contains(&kind) {
            return None;
        }
        let pattern: IUIAutomationValuePattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
        if pattern.CurrentIsReadOnly().ok()?.as_bool() {
            return None;
        }
        let value = pattern.CurrentValue().ok()?.to_string();
        if value_stands_in(element, &value) {
            return Some(FieldContext::default());
        }
        let (before, selection, after) =
            edit_selection(element, &value, timeout_ms).unwrap_or((value, String::new(), String::new()));
        Some(FieldContext {
            before: tail_chars(&before, CONTEXT_CHARS),
            after: head_chars(&after, CONTEXT_CHARS),
            selection: shorten_selection(&selection),
        })
    }

    /// [`super::stands_in_for_empty`] for a ValuePattern-only field: its
    /// value is what's in it, so only its placeholder (or nothing but
    /// markers) stands in for empty.
    unsafe fn value_stands_in(element: &IUIAutomationElement, value: &str) -> bool {
        let placeholder = element.CurrentHelpText().map(|b| b.to_string()).unwrap_or_default();
        super::stands_in_for_empty(value, "", &placeholder, || None)
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetClassNameW(hwnd: *mut core::ffi::c_void, buf: *mut u16, max: i32) -> i32;
        fn SendMessageTimeoutW(
            hwnd: *mut core::ffi::c_void,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout_ms: u32,
            result: *mut usize,
        ) -> isize;
    }

    const WM_GETTEXTLENGTH: u32 = 0x000E;
    const EM_GETSEL: u32 = 0x00B0;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;

    /// A Win32 text box's selection, which UI Automation doesn't give for
    /// WinForms' TextBox (a value, no TextPattern): `EM_GETSEL` (no pointers,
    /// so nothing to marshal; read-only), time-boxed, cutting `value` there
    /// ([`super::split_utf16`]). Only when the box's text is as long as
    /// `value` (else the positions would mean nothing) and under 65,536
    /// characters (`EM_GETSEL`'s limit). `None`: not such a box, or no answer.
    unsafe fn edit_selection(
        element: &IUIAutomationElement,
        value: &str,
        timeout_ms: u32,
    ) -> Option<(String, String, String)> {
        let hwnd = element.CurrentNativeWindowHandle().ok()?.0;
        if hwnd.is_null() {
            return None;
        }
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if n <= 0 || !super::is_edit_class(&String::from_utf16_lossy(&class[..n as usize])) {
            return None;
        }
        let ask = |msg: u32| {
            let mut out = 0usize;
            (SendMessageTimeoutW(hwnd, msg, 0, 0, SMTO_ABORTIFHUNG, timeout_ms, &mut out) != 0).then_some(out)
        };
        if ask(WM_GETTEXTLENGTH)? != value.encode_utf16().count() {
            return None;
        }
        // The start in the low word, the end in the high one; -1 past 65,535.
        let sel = ask(EM_GETSEL)? as u32;
        if sel == u32::MAX {
            return None;
        }
        super::split_utf16(value, (sel & 0xFFFF) as usize, (sel >> 16) as usize)
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
                let element = automation.GetFocusedElement().ok()?;
                if element.CurrentProcessId().ok()? as u32 != pid || element.CurrentIsPassword().ok()?.as_bool() {
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
        crate::learn::normalise(s)
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

    /// ValuePattern: a text field's whole value, if it isn't read-only; the
    /// caret where a Win32 text box says it is ([`edit_selection`]), else
    /// at the end.
    unsafe fn whole_value(element: &IUIAutomationElement, limit: usize) -> Option<FieldWindow> {
        let kind = element.CurrentControlType().ok()?;
        if ![UIA_EditControlTypeId, UIA_DocumentControlTypeId, UIA_ComboBoxControlTypeId].contains(&kind) {
            return None;
        }
        let pattern: IUIAutomationValuePattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
        if pattern.CurrentIsReadOnly().ok()?.as_bool() {
            return None;
        }
        let raw = pattern.CurrentValue().ok()?.to_string();
        let value = tidy(&raw);
        let to_end = value.chars().count() <= limit;
        let text: String = if to_end { value } else { value.chars().take(limit + 1).collect() };
        let caret = edit_selection(element, &raw, super::WATCH_TIMEOUT_MS)
            .map(|(before, _, _)| tidy(&before).len())
            .filter(|&c| c <= text.len() && text.is_char_boundary(c))
            .unwrap_or(text.len());
        Some(FieldWindow {
            caret,
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

    #[test]
    fn what_chromium_reads_for_an_empty_box_is_empty() {
        // A textarea or contenteditable labelled by aria-label: its label
        // as its text, an empty value.
        assert!(stands_in_for_empty("Type a message", "Type a message", "", || Some(String::new())));
        // No label: U+FFFC. With a placeholder: U+FFFC, the placeholder as
        // name and HelpText.
        assert!(stands_in_for_empty("\u{FFFC}", "", "", unasked));
        assert!(stands_in_for_empty("\u{FFFC}", "Write a reply", "Write a reply", unasked));
        // A rich editor's empty line (Gmail's body, WhatsApp's Lexical box).
        assert!(stands_in_for_empty("\n", "Message Body", "", unasked));
        // Slate's placeholder inside the editor, beside its U+FEFF line: the
        // value has the same words, but the marker gives it away.
        assert!(stands_in_for_empty(
            "Message #random\n\u{FEFF}\n",
            "Message #random",
            "",
            || Some("Message #random\n\u{FEFF}\n".into())
        ));
        // A field showing its placeholder (HelpText) as its text.
        assert!(stands_in_for_empty("Search mail", "", "Search mail", unasked));
        // Exactly the label, and no ValuePattern to say otherwise.
        assert!(stands_in_for_empty("Notes ", "Notes", "", || None));
    }

    #[test]
    fn real_text_is_never_a_stand_in() {
        assert!(!stands_in_for_empty("Hello world", "Notes", "Write here", unasked));
        assert!(!stands_in_for_empty("\u{FFFC} a photo", "", "", unasked));
        // The label's words, really typed: the value says they're there.
        assert!(!stands_in_for_empty("Notes", "Notes", "", || Some("Notes".into())));
        // Part of the label isn't the label.
        assert!(!stands_in_for_empty("Type a", "Type a message", "", unasked));
        // No label, no placeholder: nothing to match.
        assert!(!stands_in_for_empty("Hi", "", "", unasked));
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

    #[test]
    fn win32_text_boxes_by_class() {
        assert!(is_edit_class("Edit"));
        assert!(is_edit_class("EDIT"));
        assert!(is_edit_class("WindowsForms10.EDIT.app.0.34f5582_r6_ad1"));
        assert!(is_edit_class("WindowsForms10.Edit.app.0.2bf8098_r6_ad1"));
        assert!(!is_edit_class("WindowsForms10.RichEdit20W.app.0.34f5582_r6_ad1"));
        assert!(!is_edit_class("RICHEDIT50W"));
        assert!(!is_edit_class("TEdit"));
        assert!(!is_edit_class("Chrome_RenderWidgetHostHWND"));
    }

    #[test]
    fn a_text_box_value_cut_at_its_selection() {
        let s = |a: &str, b: &str, c: &str| Some((a.to_string(), b.to_string(), c.to_string()));
        assert_eq!(split_utf16("I want  please", 7, 7), s("I want ", "", " please"));
        assert_eq!(split_utf16("We can meet Tuesday", 12, 19), s("We can meet ", "Tuesday", ""));
        // UTF-16 units: a line break is two, an emoji two.
        assert_eq!(split_utf16("one\r\ntwo", 5, 5), s("one\r\n", "", "two"));
        assert_eq!(split_utf16("a😀b", 1, 3), s("a", "😀", "b"));
        assert_eq!(split_utf16("é", 1, 1), s("é", "", ""));
        // Positions that don't fit the value.
        assert_eq!(split_utf16("a😀b", 2, 2), None);
        assert_eq!(split_utf16("abc", 2, 4), None);
        assert_eq!(split_utf16("abc", 2, 1), None);
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

    use crate::smart_insert::FieldContext;

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
    const ES_MULTILINE: u32 = 0x0004;
    const ES_PASSWORD: u32 = 0x0020;
    const ES_READONLY: u32 = 0x0800;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const EM_SETSEL: u32 = 0x00B1;
    const WM_QUIT: u32 = 0x0012;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A hidden text field on its own message-pumping thread.
    struct Field {
        hwnd: isize,
        thread: u32,
        join: Option<std::thread::JoinHandle<()>>,
    }

    impl Field {
        fn new(class: &str, style: u32, text: &str, selection: (usize, usize)) -> Option<Field> {
            let (tx, rx) = mpsc::channel();
            let (class, text) = (wide(class), wide(text));
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
                let _ = tx.send(Some((hwnd as isize, GetCurrentThreadId())));
                let mut msg: Msg = std::mem::zeroed();
                while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                    DispatchMessageW(&msg);
                }
                DestroyWindow(hwnd);
            });
            let (hwnd, thread) = rx.recv().ok()??;
            Some(Field {
                hwnd,
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

    fn check(class: &str) {
        let text = "Hello world. we went home";
        // A caret after "Hello world. ".
        let field = Field::new(class, ES_MULTILINE, text, (13, 13)).expect("a field");
        let ctx = field.context().expect("the context");
        assert_eq!(ctx.before, "Hello world. ", "{class}: before");
        assert_eq!(ctx.after, "we went home", "{class}: after");
        assert_eq!(ctx.selection, "", "{class}: selection");
        // The post-paste watch's read: the whole field, the caret, both ends.
        let w = super::uia::window_watch(field.hwnd, 2_000).expect("the watch's read");
        assert_eq!(w.text, text, "{class}: the watch's text");
        assert_eq!(w.caret, 13, "{class}: the watch's caret");
        assert!(w.from_start && w.to_end, "{class}: the whole field");
        // The value-only path (a WinForms text box's): the caret from
        // EM_GETSEL in a Win32 text box.
        if class == "EDIT" {
            let v = super::uia::window_value_context(field.hwnd).expect("the value's context");
            assert_eq!((v.before.as_str(), v.after.as_str()), ("Hello world. ", "we went home"), "{class}: value path");
        }
        drop(field);

        // "world" selected.
        let field = Field::new(class, ES_MULTILINE, text, (6, 11)).expect("a field");
        let ctx = field.context().expect("the context");
        assert_eq!(ctx.before, "Hello ", "{class}: before a selection");
        assert_eq!(ctx.selection, "world", "{class}: the selection");
        assert_eq!(ctx.after, ". we went home", "{class}: after a selection");
        let w = super::uia::window_watch(field.hwnd, 2_000).expect("the watch's read");
        assert_eq!((w.text.as_str(), w.caret), (text, 6), "{class}: the watch, a selection");
        if class == "EDIT" {
            let v = super::uia::window_value_context(field.hwnd).expect("the value's context");
            assert_eq!(
                (v.before.as_str(), v.selection.as_str(), v.after.as_str()),
                ("Hello ", "world", ". we went home"),
                "{class}: value path, a selection"
            );
        }
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
        check("EDIT");
        // RichEdit 4.1+ has a TextPattern of its own.
        unsafe { LoadLibraryW(wide("Msftedit.dll").as_ptr()) };
        check("RICHEDIT50W");
    }
}
