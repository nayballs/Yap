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
//!    start (`TextPattern`, else `ValuePattern`), so `smart_insert` can fit the
//!    dictation to them. The paste never waits for it: a read that hasn't
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
/// no-break spaces as spaces, a field showing only its placeholder empty.
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
mod uia {
    use windows::core::{Interface, BOOL};
    use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
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

    use super::{FieldWindow, SELECTION_CHARS, SELECTION_ENDS};
    use crate::smart_insert::{head_chars, tail_chars, FieldContext, CONTEXT_CHARS};

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
            element_context(&element)
        }
    }

    /// The text around the caret in `element`, a text field.
    unsafe fn element_context(element: &IUIAutomationElement) -> Option<FieldContext> {
        // Never a password box (and if it can't say, don't risk it).
        if element.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        let ctx = text_context(element).or_else(|| value_context(element))?;
        Some(without_placeholder(ctx, element))
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
            element_context(&element)
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

    /// TextPattern: the selection (or the caret) and the text either side.
    unsafe fn text_context(element: &IUIAutomationElement) -> Option<FieldContext> {
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
        let selection = selection_text(&range)?;

        let before = range.Clone().ok()?;
        before
            .MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start)
            .ok()?;
        before
            .MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(CONTEXT_CHARS as i32 + 1))
            .ok()?;
        let before = before.GetText((CONTEXT_CHARS * 2) as i32).ok()?.to_string();

        let after = range.Clone().ok()?;
        after
            .MoveEndpointByRange(TextPatternRangeEndpoint_Start, &range, TextPatternRangeEndpoint_End)
            .ok()?;
        after
            .MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, CONTEXT_CHARS as i32 + 1)
            .ok()?;
        let after = after.GetText((CONTEXT_CHARS * 2) as i32).ok()?.to_string();

        Some(FieldContext {
            before: tail_chars(&before, CONTEXT_CHARS),
            after: head_chars(&after, CONTEXT_CHARS),
            selection,
        })
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
    /// only, never read-only ones.
    unsafe fn value_context(element: &IUIAutomationElement) -> Option<FieldContext> {
        let pattern = editable_value(element)?;
        let value = pattern.CurrentValue().ok()?.to_string();
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

    /// A text field's ValuePattern, if it isn't read-only.
    unsafe fn editable_value(element: &IUIAutomationElement) -> Option<IUIAutomationValuePattern> {
        let kind = element.CurrentControlType().ok()?;
        if ![UIA_EditControlTypeId, UIA_DocumentControlTypeId, UIA_ComboBoxControlTypeId].contains(&kind) {
            return None;
        }
        let pattern: IUIAutomationValuePattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
        if pattern.CurrentIsReadOnly().ok()?.as_bool() {
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
        if hwnd.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        let rich = super::edit_class(&String::from_utf16_lossy(&buf[..n])).or_else(|| {
            let n = RealGetWindowClassW(hwnd, buf.as_mut_ptr(), buf.len() as u32) as usize;
            super::edit_class(&String::from_utf16_lossy(&buf[..n.min(buf.len())]))
        })?;
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

    /// The field a dictation was pasted into, for the post-paste watch.
    /// Fields drop in order: the COM objects, then COM itself.
    pub struct Watched {
        /// The client the element came from (its timeouts), kept with it.
        _automation: IUIAutomation,
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
                    _automation: automation,
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
            unsafe { whole_field(&self.element, limit) }
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
            whole_field(&element, limit)
        }
    }

    /// Line breaks as `\n`, no-break spaces as spaces.
    fn tidy(s: &str) -> String {
        crate::learn::normalise(s)
    }

    /// The whole text of a field (TextPattern, else ValuePattern), its
    /// placeholder counting as empty.
    unsafe fn whole_field(element: &IUIAutomationElement, limit: usize) -> Option<FieldWindow> {
        let w = whole_text(element, limit).or_else(|| whole_value(element, limit))?;
        let shown = w.text.trim();
        if shown.is_empty() {
            return Some(w);
        }
        Some(match element.CurrentHelpText() {
            Ok(hint) if hint.to_string().trim() == shown => FieldWindow {
                text: String::new(),
                caret: 0,
                ..w
            },
            _ => w,
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

    /// An empty field that reports its placeholder ("Message #general") as
    /// its text: it's empty.
    unsafe fn without_placeholder(ctx: FieldContext, element: &IUIAutomationElement) -> FieldContext {
        let whole = format!("{}{}{}", ctx.before, ctx.selection, ctx.after);
        let whole = whole.trim();
        if whole.is_empty() {
            return ctx;
        }
        match element.CurrentHelpText() {
            Ok(hint) if hint.to_string().trim() == whole => FieldContext::default(),
            _ => ctx,
        }
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
