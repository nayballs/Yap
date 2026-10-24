//! No browser context menu in Yap's windows.
//!
//! Right-clicking a WebView2 page shows Edge's own menu: Back, Refresh, Save
//! as, Print, Inspect… — none of which belongs in an app. Every Yap webview
//! (the main window, onboarding, the meeting notepad, the Yap bar, the split
//! preview — any window, hooked as its first page starts loading, from
//! `on_page_load` in lib.rs) gets a `ContextMenuRequested` handler
//! (`ICoreWebView2_11`) that keeps only the editing commands ([`keep`]):
//! undo/redo, cut/copy/paste, paste as plain text, select all, emoji and the
//! spelling items (suggestions, add to dictionary), with the separators
//! between them that still separate something. Nothing left — a right-click
//! on plain UI, a button, the bar — and the event is marked handled: no menu
//! at all. A failure reading the menu shows none either (fail closed).
//!
//! So: plain UI → nothing; a text field → its edit menu (spelling
//! suggestions first); selected text → Copy (+ Select all). Items with
//! actions of their own (Notes' folders and notes, Chat's conversations and
//! messages, Home's dictations, Meetings, Dictionary rows, model rows…) open
//! Yap's own menu (`ui/ContextMenu.svelte`, through `ui/contextMenu.svelte.js`)
//! and cancel the `contextmenu` event first, so WebView2 never asks.
//!
//! **Inspect is for developers only:** a debug build keeps Inspect
//! (`inspectElement`) when Shift is held as the menu is asked for — a
//! Shift+right-click, anywhere (in a dev build the pages let a
//! Shift+right-click through to here: `import.meta.env.DEV`). A plain
//! right-click never shows it, in any build. Release builds (stable,
//! nightly) compile the Shift check out — [`offers_inspect`] is `false`
//! there whatever Shift says — and their webviews have DevTools off besides
//! (wry's `devtools` attribute is false without debug assertions or tauri's
//! `devtools` feature, which Yap doesn't enable): no Inspect, no F12 or
//! Ctrl+Shift+I.
//!
//! Item names are WebView2's unlocalized `ICoreWebView2ContextMenuItem::Name`
//! — "the English label of the menu item in lower camel case" ("saveAs",
//! "copy", "paste"), per Microsoft's reference — so an allowlist matches them
//! whatever the UI language. Where the runtime differs from the reference
//! (spelling items are "spellcheck", not "spellCheck"; "Paste as plain text"
//! is "pasteAndMatchStyle"), the names it really reported are what counts:
//! see [`EDIT_ITEMS`].
//!
//! Test mode (`YAP_E2E=1`, not robot mode) never lets a native menu reach the
//! screen: it records what the menu would have held and marks every event
//! handled. The debug-only [`e2e_context_menus`] reports which webviews are
//! hooked and each one's last menu (every item's name and kind, whether it
//! stayed, and whether Shift offered Inspect), for the e2e suite and for
//! checking the names on a new WebView2.

// Elsewhere there's no WebView2 to hook: the filter and its report sit unused.
#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

/// A menu item's kind (`COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Command,
    CheckBox,
    Radio,
    Separator,
    Submenu,
}

impl Kind {
    fn from_raw(kind: i32) -> Kind {
        match kind {
            1 => Kind::CheckBox,
            2 => Kind::Radio,
            3 => Kind::Separator,
            4 => Kind::Submenu,
            _ => Kind::Command,
        }
    }
}

/// The editing commands a text field's menu keeps, by the names this PC's
/// WebView2 runtime reported (October 2026, read back through
/// `e2e_context_menus`): "Paste as plain text" is `pasteAndMatchStyle`
/// (Chromium's name) and the spelling suggestions are `spellcheck`. The
/// reference's own spelling (`spellCheck`) and the naming rule's
/// (`pasteAsPlainText`, `addToDictionary`) stay in case a runtime uses them.
const EDIT_ITEMS: &[&str] = &[
    "undo",
    "redo",
    "cut",
    "copy",
    "paste",
    "pasteAndMatchStyle",
    "pasteAsPlainText",
    "selectAll",
    "emoji",
    "spellcheck",
    "spellCheck",
    "addToDictionary",
];

/// Developer tools: kept only when [`offers_inspect`] says so ("Inspect" is
/// `inspectElement`).
const INSPECT_ITEMS: &[&str] = &["inspectElement", "inspect"];

/// Whether an item stays: an editing command (or, with `inspect`, Inspect).
pub fn allowed(name: &str, inspect: bool) -> bool {
    EDIT_ITEMS.contains(&name) || (inspect && INSPECT_ITEMS.contains(&name))
}

/// Whether a menu keeps Inspect: only in a debug build (`debug`), and only
/// for a Shift+right-click (`shift`: held as WebView2 asks). A plain
/// right-click never offers it, and a release build never does at all.
pub const fn offers_inspect(debug: bool, shift: bool) -> bool {
    debug && shift
}

/// Which items of a menu stay, as their indices in order: the [`allowed`]
/// ones, and a separator only where it still separates two kept items (no
/// separator first, last, or next to another). Empty: no menu at all.
pub fn keep(items: &[(&str, Kind)], inspect: bool) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    // A separator waiting for a kept item after it.
    let mut pending: Option<usize> = None;
    for (i, (name, kind)) in items.iter().enumerate() {
        if *kind == Kind::Separator {
            if !kept.is_empty() {
                pending = Some(i);
            }
            continue;
        }
        if allowed(name, inspect) {
            if let Some(sep) = pending.take() {
                kept.push(sep);
            }
            kept.push(i);
        }
    }
    kept
}

/// This menu keeps Inspect: a debug build with Shift held right now.
#[cfg(all(windows, debug_assertions))]
fn keep_inspect() -> bool {
    offers_inspect(true, win::shift_down())
}

/// A release build never reads Shift: no Inspect, compiled in.
#[cfg(not(debug_assertions))]
fn keep_inspect() -> bool {
    offers_inspect(false, false)
}

/// Whether native menus stay off the screen entirely (plain test mode: a
/// test never leaves a menu on the desktop; robot mode shows them).
fn suppress_all() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

// ---- what the debug command reports ----------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Hook {
    /// A handler is on this webview.
    hooked: bool,
    /// Why it couldn't be hooked (an older WebView2 runtime without
    /// `ICoreWebView2_11`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// The last menu it asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<Menu>,
    /// How many menus it asked for.
    menus: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Menu {
    items: Vec<Item>,
    /// No menu was shown.
    handled: bool,
    /// The right-click was on an editable element.
    editable: bool,
    /// What was right-clicked: "page", "image", "selectedText", "audio",
    /// "video".
    target: String,
    /// Inspect was on offer (a debug build's Shift+right-click).
    inspect: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    name: String,
    /// The localized label (debug builds only: a spelling suggestion is
    /// made from what was typed).
    #[serde(skip_serializing_if = "String::is_empty")]
    label: String,
    kind: Kind,
    kept: bool,
}

static HOOKS: Mutex<BTreeMap<String, Hook>> = Mutex::new(BTreeMap::new());

fn note_hook(label: &str, f: impl FnOnce(&mut Hook)) {
    let mut hooks = HOOKS.lock().unwrap_or_else(|p| p.into_inner());
    f(hooks.entry(label.to_string()).or_default());
}

/// Debug builds: every webview's hook and the last menu it asked for,
/// `{ label: { hooked, error?, menus, last?: { items: [{name, kind, kept}],
/// handled, editable, target } } }`.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_context_menus() -> serde_json::Value {
    let hooks = HOOKS.lock().unwrap_or_else(|p| p.into_inner());
    serde_json::to_value(&*hooks).unwrap_or_default()
}

// ---- the WebView2 hook ---------------------------------------------------------------------

/// Hook `webview`'s context menu, once per WebView2 instance (called on
/// every page load; a reload keeps its hook).
#[cfg(windows)]
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    let label = webview.label().to_string();
    let result = webview.with_webview(move |platform| {
        if let Err(e) = win::hook(&label, &platform.controller()) {
            tracing::warn!(window = %label, "Couldn't filter the context menu: {e}");
            note_hook(&label, |h| h.error = Some(e.to_string()));
        }
    });
    if let Err(e) = result {
        tracing::warn!("Couldn't reach a webview to filter its context menu: {e}");
    }
}

#[cfg(not(windows))]
pub fn install<R: tauri::Runtime>(_webview: &tauri::Webview<R>) {}

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2ContextMenuRequestedEventArgs,
        ICoreWebView2_11, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND,
        COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND, COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO,
        COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE, COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_PAGE,
        COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_SELECTED_TEXT,
        COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO,
    };
    use webview2_com::{take_pwstr, ContextMenuRequestedEventHandler};
    use windows::core::{IUnknown, Interface, BOOL, PWSTR};

    use super::{keep, note_hook, suppress_all, Item, Kind, Menu};

    /// Debug builds: Shift is down as WebView2 asks for the menu (this
    /// thread's key state, or the keyboard's right now: the menu is asked for
    /// a moment after the click, with Shift still held).
    #[cfg(debug_assertions)]
    pub fn shift_down() -> bool {
        #[link(name = "user32")]
        extern "system" {
            fn GetKeyState(vkey: i32) -> i16;
            fn GetAsyncKeyState(vkey: i32) -> i16;
        }
        const VK_SHIFT: i32 = 0x10;
        unsafe { GetKeyState(VK_SHIFT) < 0 || GetAsyncKeyState(VK_SHIFT) < 0 }
    }

    thread_local! {
        /// The webviews hooked so far, by window label (WebView2 lives on the
        /// main thread, as does this). Holding each one keeps its identity
        /// unique while it's remembered.
        static HOOKED: RefCell<HashMap<String, IUnknown>> = RefCell::new(HashMap::new());
    }

    pub fn hook(label: &str, controller: &ICoreWebView2Controller) -> windows::core::Result<()> {
        let core: ICoreWebView2 = unsafe { controller.CoreWebView2()? };
        let identity: IUnknown = core.cast()?;
        if HOOKED.with(|h| h.borrow().get(label) == Some(&identity)) {
            return Ok(());
        }
        let core11: ICoreWebView2_11 = core.cast()?;
        let name = label.to_string();
        let handler = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                if let Err(e) = filter(&name, &args) {
                    // Fail closed: no menu rather than the browser's.
                    tracing::warn!(window = %name, "Context menu filter failed, showing none: {e}");
                    unsafe {
                        let _ = args.SetHandled(true);
                    }
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe { core11.add_ContextMenuRequested(&handler, &mut token)? };
        HOOKED.with(|h| h.borrow_mut().insert(label.to_string(), identity));
        note_hook(label, |h| {
            h.hooked = true;
            h.error = None;
        });
        tracing::debug!(window = %label, "Context menu filtered to editing commands");
        Ok(())
    }

    fn filter(label: &str, args: &ICoreWebView2ContextMenuRequestedEventArgs) -> windows::core::Result<()> {
        let items = unsafe { args.MenuItems()? };
        let mut count = 0u32;
        unsafe { items.Count(&mut count)? };
        let mut read: Vec<(String, Kind, String)> = Vec::with_capacity(count as usize);
        for i in 0..count {
            let item = unsafe { items.GetValueAtIndex(i)? };
            let mut name = PWSTR::null();
            unsafe { item.Name(&mut name)? };
            let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND(0);
            unsafe { item.Kind(&mut kind)? };
            let mut text = String::new();
            if cfg!(debug_assertions) {
                let mut label = PWSTR::null();
                if unsafe { item.Label(&mut label) }.is_ok() {
                    text = take_pwstr(label);
                }
            }
            read.push((take_pwstr(name), Kind::from_raw(kind.0), text));
        }
        let names: Vec<(&str, Kind)> = read.iter().map(|(n, k, _)| (n.as_str(), *k)).collect();
        let inspect = super::keep_inspect();
        let kept = keep(&names, inspect);
        // Remove from the end, so the indices still to remove stay put.
        for i in (0..count).rev() {
            if !kept.contains(&(i as usize)) {
                unsafe { items.RemoveValueAtIndex(i)? };
            }
        }
        let handled = kept.is_empty() || suppress_all();
        if handled {
            unsafe { args.SetHandled(true)? };
        }
        let (editable, target) = target_of(args);
        let menu = Menu {
            items: read
                .into_iter()
                .enumerate()
                .map(|(i, (name, kind, label))| Item { name, label, kind, kept: kept.contains(&i) })
                .collect(),
            handled,
            editable,
            target,
            inspect,
        };
        tracing::debug!(
            window = %label,
            items = ?menu.items.iter().map(|i| (i.name.as_str(), i.kept)).collect::<Vec<_>>(),
            handled,
            inspect,
            "Context menu"
        );
        note_hook(label, |h| {
            h.menus += 1;
            h.last = Some(menu);
        });
        Ok(())
    }

    /// (editable, what was right-clicked), best-effort: only for the report.
    fn target_of(args: &ICoreWebView2ContextMenuRequestedEventArgs) -> (bool, String) {
        let Ok(target) = (unsafe { args.ContextMenuTarget() }) else {
            return (false, String::new());
        };
        let mut editable = BOOL(0);
        let _ = unsafe { target.IsEditable(&mut editable) };
        let mut kind = COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND(0);
        let _ = unsafe { target.Kind(&mut kind) };
        let kind = match kind {
            COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_PAGE => "page",
            COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_IMAGE => "image",
            COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_SELECTED_TEXT => "selectedText",
            COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_AUDIO => "audio",
            COREWEBVIEW2_CONTEXT_MENU_TARGET_KIND_VIDEO => "video",
            _ => "other",
        };
        (editable.as_bool(), kind.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Kind::*;

    /// The names of what [`keep`] keeps.
    fn kept<'a>(items: &[(&'a str, Kind)], inspect: bool) -> Vec<&'a str> {
        keep(items, inspect).into_iter().map(|i| items[i].0).collect()
    }

    // The menus below are what WebView2 offered on this PC (October 2026,
    // read back through `e2e_context_menus`; "other" is its name for
    // separators and an unnamed submenu).

    #[test]
    fn plain_ui_gets_no_menu_at_all() {
        // Back, Forward, Refresh, Save as, Print, More tools, Inspect.
        let page = [
            ("back", Command),
            ("forward", Command),
            ("reload", Command),
            ("other", Separator),
            ("saveAs", Command),
            ("print", Command),
            ("other", Separator),
            ("moreTools", Submenu),
            ("other", Separator),
            ("inspectElement", Command),
        ];
        assert!(keep(&page, false).is_empty());
        // A debug build's Shift+right-click keeps Inspect, and only that (no
        // stray separator).
        assert_eq!(kept(&page, true), ["inspectElement"]);
    }

    #[test]
    fn inspect_needs_a_debug_build_and_shift() {
        let page = [
            ("back", Command),
            ("reload", Command),
            ("other", Separator),
            ("saveAs", Command),
            ("other", Separator),
            ("inspectElement", Command),
        ];
        // A plain right-click in a debug build: nothing.
        assert!(!offers_inspect(true, false));
        assert!(keep(&page, offers_inspect(true, false)).is_empty());
        // A debug build's Shift+right-click: Inspect.
        assert!(offers_inspect(true, true));
        assert_eq!(kept(&page, offers_inspect(true, true)), ["inspectElement"]);
        // A release build (stable, nightly): never, whatever Shift says.
        for shift in [false, true] {
            assert!(!offers_inspect(false, shift));
            assert!(keep(&page, offers_inspect(false, shift)).is_empty());
        }
    }

    #[test]
    fn a_release_build_never_keeps_inspect() {
        // Whatever WebView2 offers and whatever Shift says, a release build
        // keeps no developer item: in a text field, the edit commands only.
        let field = [
            ("copy", Command),
            ("paste", Command),
            ("other", Separator),
            ("inspectElement", Command),
            ("inspect", Command),
        ];
        for shift in [false, true] {
            let kept = kept(&field, offers_inspect(false, shift));
            assert_eq!(kept, ["copy", "paste"]);
        }
        // This build's own answer, compiled in: a release build (`cargo test
        // --release`) never offers it.
        #[cfg(not(debug_assertions))]
        assert!(!keep_inspect());
    }

    #[test]
    fn a_text_field_keeps_its_edit_menu() {
        // Right-click in a text box: Emoji, Undo, Redo, Cut, Copy, Paste,
        // Paste as plain text, Select all, a writing-direction submenu, More
        // tools, Inspect.
        let field = [
            ("emoji", Command),
            ("other", Separator),
            ("undo", Command),
            ("redo", Command),
            ("other", Separator),
            ("cut", Command),
            ("copy", Command),
            ("paste", Command),
            ("pasteAndMatchStyle", Command),
            ("selectAll", Command),
            ("other", Separator),
            ("other", Submenu),
            ("other", Separator),
            ("moreTools", Submenu),
            ("other", Separator),
            ("inspectElement", Command),
        ];
        assert_eq!(
            kept(&field, false),
            [
                "emoji",
                "other",
                "undo",
                "redo",
                "other",
                "cut",
                "copy",
                "paste",
                "pasteAndMatchStyle",
                "selectAll",
            ]
        );
        // Debug + Shift: Inspect after one separator, the dropped submenus'
        // separators folded into one.
        let debug = keep(&field, true);
        assert_eq!(&debug[debug.len() - 3..], [9, 14, 15]);
    }

    #[test]
    fn a_misspelt_word_keeps_its_suggestions() {
        // Right-click on a misspelt word (it gets selected): the spelling
        // items, Cut, Copy, Paste, Paste as plain text, More tools, Inspect.
        let word = [
            ("spellcheck", Command),
            ("spellcheck", Command),
            ("spellcheck", Command),
            ("other", Separator),
            ("cut", Command),
            ("copy", Command),
            ("paste", Command),
            ("pasteAndMatchStyle", Command),
            ("other", Separator),
            ("moreTools", Submenu),
            ("other", Separator),
            ("inspectElement", Command),
        ];
        assert_eq!(
            kept(&word, false),
            ["spellcheck", "spellcheck", "spellcheck", "other", "cut", "copy", "paste", "pasteAndMatchStyle"]
        );
        // The reference's spellings are taken too.
        assert!(allowed("spellCheck", false) && allowed("addToDictionary", false));
    }

    #[test]
    fn selected_text_keeps_copy_and_select_all() {
        let selection = [
            ("copy", Command),
            ("other", Separator),
            ("searchWeb", Command),
            ("readAloud", Command),
            ("translate", Command),
            ("other", Separator),
            ("selectAll", Command),
            ("other", Separator),
            ("share", Submenu),
            ("print", Command),
        ];
        assert_eq!(kept(&selection, false), ["copy", "other", "selectAll"]);
    }

    #[test]
    fn separators_never_lead_trail_or_double() {
        let items = [
            ("other", Separator),
            ("reload", Command),
            ("other", Separator),
            ("other", Separator),
            ("copy", Command),
            ("other", Separator),
            ("print", Command),
            ("other", Separator),
        ];
        assert_eq!(kept(&items, false), ["copy"]);
        assert!(keep(&[("other", Separator)], true).is_empty());
        assert!(keep(&[], true).is_empty());
    }

    #[test]
    fn links_images_and_unknown_items_go() {
        let items = [
            ("openLinkInNewWindow", Command),
            ("copyLinkLocation", Command),
            ("saveLinkAs", Command),
            ("saveImageAs", Command),
            ("copyImage", Command),
            ("copyImageLocation", Command),
            ("webCapture", Command),
            ("createQrCode", Command),
            ("custom", Command),
            ("extension", Command),
            ("", Command),
        ];
        assert!(keep(&items, true).is_empty());
        // Names are matched exactly (they're unlocalized lower camel case).
        assert!(!allowed("Copy", false) && !allowed("copy ", false));
        assert!(allowed("inspectElement", true) && !allowed("inspectElement", false));
    }

    #[test]
    fn kinds_map_from_webview2() {
        assert_eq!(Kind::from_raw(0), Command);
        assert_eq!(Kind::from_raw(3), Separator);
        assert_eq!(Kind::from_raw(4), Submenu);
        assert_eq!(Kind::from_raw(1), CheckBox);
        assert_eq!(Kind::from_raw(2), Radio);
    }
}
