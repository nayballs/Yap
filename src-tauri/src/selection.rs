//! Selected-text capture for **edit/rewrite mode**.
//!
//! When the user triggers the edit hotkey, Yap grabs whatever text is selected
//! in the foreground app *before* recording, so the spoken instruction can be
//! applied to it. Two tiers (ported in spirit from FluidVoice, which reads the
//! macOS Accessibility `AXSelectedText`):
//!
//! 1. **UI Automation `TextPattern`** — the clean, non-destructive path: read the
//!    focused element's selection directly. Works in most native + WinUI apps.
//! 2. **Clipboard Ctrl+C trick** (`text_injector::selection_via_copy`) — fallback
//!    for apps whose UIA text support is patchy (Electron/Chromium, terminals).
//!
//! Empty selection from both tiers → `None`, which the pipeline treats as
//! "write mode" (generate new text from the instruction alone).

/// Capture the current selection for edit mode: UIA first, Ctrl+C fallback.
///
/// `target_hwnd` is the foreground window captured at hotkey-press (used to
/// aim the Ctrl+C fallback). Returns `None` if nothing is selected.
pub fn capture_selection(target_hwnd: Option<isize>) -> Option<String> {
    if let Some(text) = uia_selected_text() {
        let t = text.trim();
        if !t.is_empty() {
            tracing::debug!(len = t.len(), "Selection captured via UI Automation");
            return Some(t.to_string());
        }
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

#[cfg(windows)]
mod uia {
    use windows::core::Interface;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
    };

    /// Read the current text selection from the focused UI element via UI
    /// Automation. `Some(text)` if a non-empty selection is found; `None` on no
    /// selection, an unsupported control, or any COM error (never panics).
    pub fn uia_selected_text() -> Option<String> {
        unsafe {
            // Initialize COM. Tolerate "already initialized" (S_FALSE) and a
            // differing apartment mode (RPC_E_CHANGED_MODE) — both mean COM is
            // usable; only bail on a hard error.
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
                if hr != RPC_E_CHANGED_MODE {
                    return None;
                }
            }

            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
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
}

#[cfg(windows)]
use uia::uia_selected_text;

#[cfg(not(windows))]
fn uia_selected_text() -> Option<String> {
    None
}
