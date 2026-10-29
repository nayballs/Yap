//! A call's join code, read from its windows (`codes.rs` says what each app
//! shows): the call app's own windows' titles (an older Zoom's "Meeting ID:
//! …"), and for a meeting in a browser, the tab's title (Meet's "Meet -
//! abc-defg-hij") or else the address in the browser's address bar (Zoom's
//! web client, Teams on the web), read with UI Automation.
//!
//! Touches windows and UI Automation: never under a lock (call detection
//! calls it before it takes its own, `meeting_detect::on_edges`), and it
//! skips Yap's own windows, whose titles would ask the main thread. Bounded:
//! one look at the windows on screen when a call starts, and at most
//! [`ADDRESS_MS`] in a browser's tree. Logs counts and the kind of code,
//! never the code or a title.

#[cfg(debug_assertions)]
use std::collections::HashMap;
#[cfg(debug_assertions)]
use std::sync::Mutex;

/// Reading one browser window's address gives up after this long.
#[cfg(windows)]
const ADDRESS_MS: u64 = 1_500;

/// Test runs: what a call app's window "shows" (title, address), set by
/// the e2e suite instead of real windows (`calendar_e2e_call_window`).
#[cfg(debug_assertions)]
static E2E: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);

/// The join code call app `app_id`'s windows show now, if any.
pub fn read(app_id: &str) -> Option<String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        let shown = E2E.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(app_id).cloned());
        let (title, address) = shown?;
        return super::codes::in_title(app_id, &title)
            .or_else(|| super::codes::in_link(&address).filter(|c| super::codes::matches_app(c, app_id)));
    }
    #[cfg(windows)]
    {
        if crate::shell::e2e::active() {
            return None;
        }
        win::read(app_id)
    }
    #[cfg(not(windows))]
    {
        let _ = app_id;
        None
    }
}

/// Test runs: call app `app_id`'s window shows `title` and, in a browser,
/// `address` (`None` takes it away).
#[cfg(debug_assertions)]
pub fn e2e_set(app_id: &str, shown: Option<(String, String)>) {
    let mut map = E2E.lock().unwrap_or_else(|p| p.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    match shown {
        Some(s) => {
            map.insert(app_id.to_string(), s);
        }
        None => {
            map.remove(app_id);
        }
    }
}

#[cfg(windows)]
mod win {
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationCacheRequest, TreeScope_Children, TreeScope_Element, UIA_ComboBoxControlTypeId,
        UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_ValueValuePropertyId,
    };

    use super::super::codes;
    use super::ADDRESS_MS;
    use crate::pasting::selection::uia::{automation, Com};

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn GetWindowTextW(hwnd: *mut c_void, text: *mut u16, max: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
    }

    unsafe extern "system" fn collect(hwnd: *mut c_void, lparam: isize) -> i32 {
        let list = &mut *(lparam as *mut Vec<isize>);
        if IsWindowVisible(hwnd) != 0 {
            list.push(hwnd as isize);
        }
        1
    }

    pub fn read(app_id: &str) -> Option<String> {
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
            EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize);
        }
        let own = std::process::id();
        let browsers = crate::calls::meeting_detect::browser_exes();
        // Browser windows showing the meeting, for their addresses after.
        let mut tabs: Vec<isize> = Vec::new();
        for hwnd in hwnds {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd as *mut c_void, &mut pid) };
            if pid == 0 || pid == own {
                continue;
            }
            let mut buf = [0u16; 512];
            let len = unsafe { GetWindowTextW(hwnd as *mut c_void, buf.as_mut_ptr(), buf.len() as i32) };
            if len <= 0 {
                continue;
            }
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            let Some(exe) = crate::pasting::text_injector::app_name_for(Some(hwnd)) else { continue };
            let own_app = crate::calls::meeting_detect::exe_is(app_id, &exe);
            let browser = browsers.iter().any(|b| b.eq_ignore_ascii_case(&exe))
                && crate::calls::meeting_detect::title_shows(app_id, &title);
            if !own_app && !browser {
                continue;
            }
            if let Some(code) = codes::in_title(app_id, &title) {
                tracing::info!(app = app_id, kind = codes::kind(&code), "calendar: the call's window shows its join code");
                return Some(code);
            }
            if browser {
                tabs.push(hwnd);
            }
        }
        for hwnd in tabs {
            let until = Instant::now() + Duration::from_millis(ADDRESS_MS);
            let code = address_of(hwnd, until).and_then(|a| codes::in_link(&a)).filter(|c| codes::matches_app(c, app_id));
            if let Some(code) = code {
                tracing::info!(app = app_id, kind = codes::kind(&code), "calendar: the call's tab address has its join code");
                return Some(code);
            }
        }
        None
    }

    /// What a browser window's address bar holds: the first text box in its
    /// toolbars (a breadth-first look that never goes into the page) whose
    /// value reads like an address.
    fn address_of(hwnd: isize, until: Instant) -> Option<String> {
        let _com = Com::init()?;
        let automation = automation(ADDRESS_MS as u32)?;
        unsafe {
            let root = automation.ElementFromHandle(HWND(hwnd as *mut c_void)).ok()?;
            let cache = cache(&automation)?;
            let condition = automation.ControlViewCondition().ok()?;
            let mut queue = VecDeque::from([root]);
            let (mut calls, mut seen) = (0usize, 0usize);
            while let Some(parent) = queue.pop_front() {
                if calls >= 80 || seen >= 1_500 || Instant::now() >= until {
                    break;
                }
                calls += 1;
                let Ok(children) = parent.FindAllBuildCache(TreeScope_Children, &condition, &cache) else { continue };
                for i in 0..children.Length().unwrap_or(0) {
                    seen += 1;
                    let Ok(e) = children.GetElement(i) else { continue };
                    let kind = e.CachedControlType().unwrap_or_default();
                    if kind == UIA_DocumentControlTypeId {
                        continue; // the page itself: never read
                    }
                    if kind == UIA_EditControlTypeId {
                        let value = e
                            .GetCachedPropertyValue(UIA_ValueValuePropertyId)
                            .ok()
                            .and_then(|v| windows::core::BSTR::try_from(&v).ok())
                            .map(|b| b.to_string())
                            .unwrap_or_default();
                        let value = value.trim();
                        if value.contains('.') && !value.contains(char::is_whitespace) {
                            return Some(value.to_string());
                        }
                        continue;
                    }
                    if kind == UIA_ComboBoxControlTypeId || queue.len() < 400 {
                        queue.push_back(e);
                    }
                }
            }
        }
        None
    }

    unsafe fn cache(automation: &IUIAutomation) -> Option<IUIAutomationCacheRequest> {
        let cache = automation.CreateCacheRequest().ok()?;
        cache.AddProperty(UIA_ControlTypePropertyId).ok()?;
        cache.AddProperty(UIA_ValueValuePropertyId).ok()?;
        cache.SetTreeScope(TreeScope_Element).ok()?;
        Some(cache)
    }
}
