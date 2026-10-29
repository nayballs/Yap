//! The **quick note**: a small window on its own shortcut for jotting or
//! dictating a note from any app (Wispr Flow's Scratchpad, adapted: local,
//! no sync, one note at a time). The page is `src/lib/quicknote/QuickNote.svelte`.
//!
//! - **One window**, `quick-note` in tauri.conf.json: created hidden and
//!   unfocused at startup like the notepad (the DWM-cloaked first show,
//!   `shell::window::init_hidden_webview`), always on top while it's up,
//!   never remembered by the window-state plugin. Closing hides it.
//! - **Opening it**: the Quick note shortcut (Settings → Shortcuts, unbound
//!   by default; `input_hook.rs`), the tray's "New quick note" (always a new
//!   one), the Notes view's "Quick note". The shortcut again while it's in
//!   front puts it away ([`toggle`]). It comes up on the monitor with the
//!   mouse cursor, at the bottom right of its work area; moved there by the
//!   person, it stays where they put it while it's on that monitor.
//! - **What it opens on** ([`pick`]): the last quick note, if it was
//!   changed less than a day ago ([`RESUME_SECS`]); else a new one. A note
//!   you came back to within the day is one you were still writing; one left
//!   alone longer is done and lives in Notes, and the window starts a clean
//!   page. "New note" is always a click (or Ctrl+N) away.
//! - **Saved as ordinary notes** in notes.json (`notes.rs`, written off its
//!   lock, in order), so Notes, search by meaning and Chat's note tools see
//!   them: `source` "quick" marks a quick note, filed in the "Quick notes"
//!   folder (made on first use; the person may rename, move or delete it),
//!   the title the note's first words while the person hasn't named it
//!   (`title_auto`, so the Notes view shows it as Yap's). Only fields an
//!   older Yap already reads and keeps: one sharing the folder saves a quick
//!   note back unchanged, and lists it in Notes like any note.
//! - **A blank note is never kept** ([`is_blank`]): nothing is made until
//!   there's a word, and a note emptied again goes ([`save`]).
//! - **Dictating into it**: while its window is in front (or its mic
//!   button asked), a dictation is the note's ([`takes_dictation`],
//!   `pipeline.rs` `Goes::QuickNote`): cleaned up as any dictation, then
//!   handed to the page (`yap-quick-note-dictation`), which puts it at the
//!   cursor. Never pasted into the app behind it, nor into Yap's own window
//!   through the clipboard. Undo of a cancelled one lands here too.
//!   **Holding the shortcut** opens it and dictates into it until the key
//!   comes up; a tap opens or hides it ([`key`]).
//! - **Tabs** ([`tabs`]): several quick notes open at once, remembered
//!   across restarts in a file of their own.
//! - **Send to {app}** ([`send`]): the note's text, or its selection, pasted
//!   into the app that was in front when it came up.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::notes::Note;

pub mod key;
pub mod send;
pub mod tabs;

/// The window's label (tauri.conf.json).
pub const LABEL: &str = "quick-note";
/// The folder quick notes are filed in (made on first use).
pub const FOLDER: &str = "Quick notes";
/// `Note::source` of a quick note: what marks it, wherever it's moved.
pub const SOURCE: &str = "quick";
/// The last quick note opens again if it was changed less than a day ago.
pub const RESUME_SECS: u64 = 24 * 60 * 60;
/// What the window shows next: `{noteId, title, titleAuto, content, folder,
/// fresh}` (noteId null: a new note).
const EVENT_OPEN: &str = "yap-quick-note-open";
/// Yap showed or hid the window: `true` / `false` (WebView2 reports a
/// hidden page as visible).
const EVENT_VISIBLE: &str = "yap-quick-note-visible";
/// A dictation for the note: `{text, newline}` (`pipeline/delivery.rs`).
pub const EVENT_DICTATION: &str = "yap-quick-note-dictation";
/// A title is the first line's first words, as the Notes view names an
/// untitled note…
const TITLE_WORDS: usize = 6;
/// …and never longer than this.
const TITLE_CHARS: usize = 60;
/// How far the window sits from the work area's edges (logical px).
const MARGIN: f64 = 24.0;

/// The note the window is on, as far as the next open goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Current {
    /// Not opened since Yap started: the newest quick note, if recent.
    Unset,
    /// A new note, not written yet ("New note", the tray's "New quick note").
    New,
    /// This note.
    Note(u64),
}

static CURRENT: Mutex<Current> = Mutex::new(Current::Unset);
/// The window's handle (Windows), for "is it in front".
static HWND: AtomicIsize = AtomicIsize::new(0);
/// Yap showed it and hasn't hidden it since.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// Its mic button asked for the next dictation (taken by the recording it
/// starts, cleared right after).
static ARMED: AtomicBool = AtomicBool::new(false);
/// Placed once since Yap started (the first open always places it).
static PLACED: AtomicBool = AtomicBool::new(false);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn current() -> Current {
    *CURRENT.lock().unwrap_or_else(|p| p.into_inner())
}

fn set_current(c: Current) {
    *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = c;
}

// ---- the note (pure) ---------------------------------------------------------------------------

/// A quick note's title (pure): its first line's first words, as the Notes
/// view names an untitled note (six, "…" when there were more), at most
/// 60 characters, without a heading's `#` or a list's bullet. Empty for a
/// blank note.
pub fn title_from(content: &str) -> String {
    let Some(line) = content.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return String::new();
    };
    let words: Vec<&str> = strip_marks(line).split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }
    let mut cut = words.len() > TITLE_WORDS;
    let mut title = String::new();
    for word in words.iter().take(TITLE_WORDS) {
        let len = title.chars().count() + usize::from(!title.is_empty()) + word.chars().count();
        if len > TITLE_CHARS {
            cut = true;
            if title.is_empty() {
                title = word.chars().take(TITLE_CHARS).collect();
            }
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if cut {
        title.push('…');
    }
    title
}

/// A line without a heading's `#`s or a list item's mark.
fn strip_marks(line: &str) -> &str {
    let line = line.trim_start_matches('#').trim_start();
    for mark in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ ", "• "] {
        if let Some(rest) = line.strip_prefix(mark) {
            return rest.trim_start();
        }
    }
    line
}

/// A quick note (pure): one the window made, and not a meeting since.
pub fn is_quick(n: &Note) -> bool {
    n.source == SOURCE && n.note_type != "meeting"
}

/// A note with nothing in it (pure): no words, no title the person gave
/// it, nothing a meeting or the AI added. Never kept.
pub fn is_blank(n: &Note) -> bool {
    n.content.trim().is_empty()
        && (n.title_auto || n.title.trim().is_empty())
        && n.transcript.is_empty()
        && n.enhanced_content.trim().is_empty()
        && n.note_type != "meeting"
}

/// The window's text into note `n` (pure): its content, and — while the
/// title is Yap's (empty, or made from the words) — the title from its
/// first words. A title the person typed stays. Whether anything changed.
pub fn apply_text(n: &mut Note, content: &str) -> bool {
    let mut changed = false;
    if n.content != content {
        n.content = content.to_string();
        changed = true;
    }
    if n.note_type != "meeting" && (n.title_auto || n.title.trim().is_empty()) {
        let title = title_from(content);
        let auto = !title.is_empty();
        if n.title != title || n.title_auto != auto {
            n.title = title;
            n.title_auto = auto;
            changed = true;
        }
    }
    changed
}

/// The note the window opens on (pure; `quick`: every quick note's id and
/// when it last changed, `now` in unix seconds): the one it was on, or —
/// the first time since Yap started — the newest, if it changed less than
/// [`RESUME_SECS`] ago; else none (a new note).
pub fn pick(current: Current, quick: &[(u64, u64)], now: u64) -> Option<u64> {
    let recent = |updated: u64| now.saturating_sub(updated) < RESUME_SECS;
    match current {
        Current::New => None,
        Current::Note(id) => quick.iter().find(|(q, _)| *q == id).filter(|(_, u)| recent(*u)).map(|(q, _)| *q),
        Current::Unset => quick.iter().max_by_key(|(id, u)| (*u, *id)).filter(|(_, u)| recent(*u)).map(|(q, _)| *q),
    }
}

/// Where the window goes on a monitor (pure, physical px): the bottom right
/// of the work area `(x, y, width, height)`, `margin` in from its edges, no
/// bigger than the room left inside them. `(x, y, width, height)`.
pub fn corner(work: (i32, i32, u32, u32), size: (u32, u32), margin: u32) -> (i32, i32, u32, u32) {
    let (ax, ay, aw, ah) = work;
    let room_w = aw.saturating_sub(2 * margin).max(1);
    let room_h = ah.saturating_sub(2 * margin).max(1);
    let (w, h) = (size.0.min(room_w), size.1.min(room_h));
    let x = ax + aw as i32 - margin as i32 - w as i32;
    let y = ay + ah as i32 - margin as i32 - h as i32;
    (x.max(ax), y.max(ay), w, h)
}

// ---- the notes it keeps -----------------------------------------------------------------------

/// What the window shows: a note, or a new one (`noteId` null).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub note_id: Option<u64>,
    pub title: String,
    pub title_auto: bool,
    pub content: String,
    pub folder: String,
    /// A new note asked for ("New quick note"): the window starts one
    /// whatever it had.
    pub fresh: bool,
    /// The notes open as tabs, in order ([`tabs::for_open`]; the page builds
    /// its tabs from them when it loads)…
    pub tabs: Vec<TabNote>,
    /// …and the remembered tabs that close now (left alone a day, gone).
    pub close: Vec<u64>,
}

/// One tab's note.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabNote {
    pub note_id: u64,
    pub title: String,
    pub title_auto: bool,
    pub content: String,
    pub folder: String,
}

fn shown_of(note: Option<Note>, fresh: bool) -> Shown {
    match note {
        Some(n) => Shown {
            note_id: Some(n.id),
            title: n.title,
            title_auto: n.title_auto,
            content: n.content,
            folder: n.folder,
            fresh,
            ..Shown::default()
        },
        None => Shown {
            fresh,
            ..Shown::default()
        },
    }
}

/// What the window shows now: the note it opens on ([`pick`], remembered as
/// the window's; none for a `fresh` one — the page starts it), and its tabs.
fn shown_now(fresh: bool) -> Shown {
    let quick: Vec<(u64, u64)> = crate::notes::with_all(|notes| {
        notes.iter().filter(|n| is_quick(n)).map(|n| (n.id, n.updated_ts)).collect()
    });
    let now = now_secs();
    let picked = if fresh { None } else { pick(current(), &quick, now) };
    if !fresh {
        set_current(picked.map_or(Current::New, Current::Note));
    }
    let (open, close) = tabs::for_open(&tabs::remembered().tabs, &quick, picked, now);
    let mut shown = shown_of(picked.and_then(crate::notes::get), fresh);
    shown.tabs = open
        .into_iter()
        .filter_map(crate::notes::get)
        .map(|n| TabNote { note_id: n.id, title: n.title, title_auto: n.title_auto, content: n.content, folder: n.folder })
        .collect();
    shown.close = close;
    shown
}

/// What a save did, for the window.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The note now (null: none — nothing written yet, or emptied and gone).
    pub note_id: Option<u64>,
    pub title: String,
    pub title_auto: bool,
    pub folder: String,
    /// The note was emptied and taken away.
    pub removed: bool,
    /// A save that failed ("Couldn't save your note: the disk is full."):
    /// the note is kept in memory and saved with the next change. Here
    /// rather than as the command's error, so a new note's id still
    /// reaches the window (its next save mustn't make another).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsaved: Option<String>,
}

/// The "Quick notes" folder, made the first time it's needed; as the list
/// spells it.
fn folder() -> String {
    if let Some(f) = crate::notes::find_folder(FOLDER) {
        return f;
    }
    crate::notes::folder_create(FOLDER);
    crate::notes::find_folder(FOLDER).unwrap_or_else(|| FOLDER.to_string())
}

fn saved_of(n: &Note) -> Saved {
    Saved {
        note_id: Some(n.id),
        title: n.title.clone(),
        title_auto: n.title_auto,
        folder: n.folder.clone(),
        removed: false,
        unsaved: None,
    }
}

/// A new quick note holding `content` in the store: in the Quick notes
/// folder, titled by its first words (Yap's title).
fn new_note(content: &str) -> Note {
    let folder = folder();
    let note = crate::notes::create(&title_from(content), content, SOURCE, &folder);
    let _ = crate::notes::modify_quietly(note.id, |n| {
        n.title_auto = !n.title.is_empty();
        Ok(((), true))
    });
    crate::notes::get(note.id).unwrap_or(note)
}

/// A new quick note holding `content`, the window's from now on.
fn create(app: &AppHandle, content: &str) -> Saved {
    let note = new_note(content);
    set_current(Current::Note(note.id));
    tracing::info!(id = note.id, chars = content.chars().count(), "Quick note: a new note");
    let _ = app.emit("yap-notes-changed", ());
    crate::notes::commands::note_changed(app, note.id, LABEL);
    crate::notes::get(note.id).map(|n| saved_of(&n)).unwrap_or_default()
}

/// The window's text for note `id` (none: a new note): saved as typed;
/// a new note made at its first word, an emptied one taken away (never the
/// one a meeting records into). Every window is told. A save that failed
/// is said in `unsaved` (the note is kept in memory and saved with the
/// next change, `saving.rs`), after the windows are told.
pub fn save(app: &AppHandle, id: Option<u64>, content: &str) -> Result<Saved, String> {
    let mut saved = match id {
        None if content.trim().is_empty() => Saved::default(),
        None => create(app, content),
        Some(id) => {
            let recording = crate::meetings::meeting::recording_note() == Some(id);
            let mut changed = false;
            match crate::notes::modify_or_remove(
                id,
                |n| {
                    changed = apply_text(n, content);
                    Ok((saved_of(n), changed))
                },
                |n| !recording && is_blank(n),
            ) {
                Ok(Some(saved)) => {
                    if changed {
                        crate::notes::commands::note_changed(app, id, LABEL);
                    }
                    saved
                }
                Ok(None) => {
                    tracing::info!(id, "Quick note: emptied, so not kept");
                    if current() == Current::Note(id) {
                        set_current(Current::New);
                    }
                    let _ = app.emit("yap-notes-changed", ());
                    let _ = app.emit(crate::meetings::meeting_end::EVENT_DELETED, serde_json::json!({ "id": id }));
                    Saved {
                        removed: true,
                        ..Saved::default()
                    }
                }
                // Deleted meanwhile (from Notes): what's typed isn't lost.
                Err(_) if crate::notes::get(id).is_none() => {
                    if content.trim().is_empty() {
                        Saved::default()
                    } else {
                        create(app, content)
                    }
                }
                Err(e) => return Err(e),
            }
        }
    };
    saved.unsaved = crate::notes::saved().err().map(|e| e.to_string());
    Ok(saved)
}

// ---- the window --------------------------------------------------------------------------------

fn window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Closing the window hides it; its handle kept for "is it in front"
/// (app setup).
pub fn init(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    #[cfg(windows)]
    if let Ok(h) = w.hwnd() {
        HWND.store(h.0 as isize, Ordering::SeqCst);
    }
    let handle = app.clone();
    w.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            hide(&handle);
        }
    });
}

/// The window is the one in front (Windows' foreground window). A test run
/// never has the focus: there, it's in front while it's shown.
pub fn in_front() -> bool {
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return SHOWN.load(Ordering::SeqCst);
    }
    let own = HWND.load(Ordering::SeqCst);
    own != 0 && foreground() == own
}

#[cfg(windows)]
fn foreground() -> isize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize }
}

#[cfg(not(windows))]
fn foreground() -> isize {
    0
}

/// Its mic button: the next dictation is the note's (or not any more).
pub fn arm(on: bool) {
    ARMED.store(on, Ordering::SeqCst);
}

/// Whether a dictation starting now is the note's (`own_window`: one of
/// Yap's windows is in front): its mic asked, or its window is the one in
/// front. Atomics and one Win32 call: safe under the pipeline's locks.
pub fn takes_dictation(own_window: bool) -> bool {
    let armed = ARMED.swap(false, Ordering::SeqCst);
    let test_run = crate::shell::e2e::active() && !crate::shell::realtest::on();
    armed || ((own_window || test_run) && in_front())
}

/// Bring the window up on the note it should show (`fresh`: a new one,
/// whatever it had) and give it the focus: the shortcut, the tray, Notes.
/// Main thread (window calls); never under a lock.
pub fn open(app: &AppHandle, fresh: bool) {
    // The app in front, for Send (before the window takes the front).
    send::remember_before(in_front());
    // Fresh: the page saves what it has, then starts the new note
    // (`quick_note_new`), so a save on its way can't take its place.
    let shown = shown_now(fresh);
    tracing::info!(fresh, resumed = shown.note_id.is_some(), tabs = shown.tabs.len(), closed = shown.close.len(), "Quick note: opened");
    let _ = app.emit(EVENT_OPEN, &shown);
    let Some(w) = window(app) else { return };
    let up = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
    if !up {
        place(app, &w);
    }
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    let _ = w.show();
    SHOWN.store(true, Ordering::SeqCst);
    let _ = app.emit(EVENT_VISIBLE, true);
    crate::shell::window::brought_forward();
    if !crate::shell::e2e::active() {
        let _ = w.set_focus();
    }
    crate::shell::window::behind_in_test_runs(&w);
}

/// Put the window away (Esc, ✕, the shortcut while it's in front). The
/// page has saved what was typed (and saves on as it's told).
pub fn hide(app: &AppHandle) {
    SHOWN.store(false, Ordering::SeqCst);
    if let Some(w) = window(app) {
        let _ = w.hide();
    }
    let _ = app.emit(EVENT_VISIBLE, false);
}

/// The shortcut: away if it's in front, else up (and in front).
pub fn toggle(app: &AppHandle) {
    if in_front() {
        hide(app);
    } else {
        open(app, false);
    }
}

/// [`open`] on the main thread (the shortcut's tap or hold).
pub fn open_soon(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || open(&handle, false)) {
        tracing::warn!("Quick note: couldn't open it: {e}");
    }
}

/// [`hide`] on the main thread (the shortcut's tap while it's in front).
pub fn hide_soon(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || hide(&handle)) {
        tracing::warn!("Quick note: couldn't put it away: {e}");
    }
}

/// [`toggle`] on the main thread (the Yap bar's button).
pub fn toggle_soon(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || toggle(&handle)) {
        tracing::warn!("Quick note: couldn't open it: {e}");
    }
}

/// On the monitor with the mouse cursor (a test run: the primary one, never
/// the cursor), at its bottom right — the first time since Yap started, or
/// when it's on another monitor; else where the person put it, inside the
/// screen.
fn place(app: &AppHandle, w: &tauri::WebviewWindow) {
    let target = if crate::shell::e2e::active() {
        w.primary_monitor().ok().flatten()
    } else {
        app.cursor_position()
            .ok()
            .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
            .or_else(|| w.primary_monitor().ok().flatten())
    };
    let Some(target) = target else { return };
    let here = w.current_monitor().ok().flatten();
    let same = here.as_ref().is_some_and(|m| m.position() == target.position() && m.size() == target.size());
    let area = target.work_area();
    let work = (area.position.x, area.position.y, area.size.width, area.size.height);
    if same && PLACED.load(Ordering::SeqCst) {
        if let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) {
            if let Some((x, y, width, height)) = crate::shell::window::fit_rect((pos.x, pos.y, size.width, size.height), work) {
                let _ = w.set_size(tauri::PhysicalSize::new(width, height));
                let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
        return;
    }
    // Onto that monitor first: one with another DPI scales the window on
    // the way, and its corner is worked out from the size it has there.
    if !same {
        let _ = w.set_position(tauri::PhysicalPosition::new(area.position.x, area.position.y));
    }
    let Ok(size) = w.outer_size() else { return };
    let margin = (MARGIN * target.scale_factor()).round().max(0.0) as u32;
    let (x, y, width, height) = corner(work, (size.width, size.height), margin);
    if (width, height) != (size.width, size.height) {
        let _ = w.set_size(tauri::PhysicalSize::new(width, height));
    }
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    PLACED.store(true, Ordering::SeqCst);
}

// ---- commands ----------------------------------------------------------------------------------

/// The Quick note shortcut from one of Yap's own windows (their in-page
/// fallback: the hook never sees keys while a Yap webview has the focus):
/// away if it's in front, else up.
#[tauri::command]
pub fn quick_note_toggle(app: AppHandle) {
    toggle(&app);
}

/// Bring it up (the Notes view's "Quick note"; `fresh`: on a new note).
#[tauri::command]
pub fn quick_note_open(app: AppHandle, fresh: Option<bool>) {
    open(&app, fresh.unwrap_or(false));
}

/// Put it away (Esc, ✕, "Open in Notes").
#[tauri::command]
pub fn quick_note_hide(app: AppHandle) {
    hide(&app);
}

/// What the window shows (its page loading): the note it's on, picked as
/// an open would.
#[tauri::command]
pub fn quick_note_state() -> Shown {
    shown_now(false)
}

/// The window's text, as typed (see [`save`]).
#[tauri::command]
pub fn quick_note_save(app: AppHandle, id: Option<u64>, content: String) -> Result<Saved, String> {
    save(&app, id, &content)
}

/// "New note": the window starts a new one (what it had is saved already).
#[tauri::command]
pub fn quick_note_new() {
    set_current(Current::New);
}

/// "Add to the Yap bar" on its empty page (and Settings → Yap bar, which
/// saves it as any setting): the bar's quick-note button on or off
/// (`bar_quick_note`). Settings follows (`yap-bar-changed`).
#[tauri::command]
pub fn quick_note_bar_button(app: AppHandle, on: bool) -> Result<(), String> {
    crate::shell::settings::change_config(&app, |c| {
        c.bar_quick_note = on;
        Ok(())
    })?;
    tracing::info!(on, "Quick note: the Yap bar's button switched from the quick note");
    let _ = app.emit("yap-bar-changed", serde_json::json!({ "quickNote": on }));
    Ok(())
}

/// Its mic button: a dictation into the note, or stop the one recording.
#[tauri::command]
pub fn quick_note_dictate(state: tauri::State<'_, crate::AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.toggle_for_quick_note();
        }
    }
}

/// Debug-only, test mode: `text` as if the note's mic had just heard it —
/// the dictation's own steps from cleanup on, into the note
/// (`yap-quick-note-dictation`). Stands in for a microphone.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_quick_note_dictation(
    state: tauri::State<'_, crate::AppState>,
    text: String,
) -> Result<crate::dictation::pipeline::Delivered, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_quick_note_dictation is for test runs".into());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.robot()
    };
    robot.dictate_to_quick_note(text).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(content: &str) -> Note {
        serde_json::from_value(serde_json::json!({
            "id": 1,
            "content": content,
            "source": SOURCE,
            "folder": FOLDER,
            "createdTs": 1,
            "updatedTs": 1,
        }))
        .unwrap()
    }

    #[test]
    fn a_quick_notes_title_is_its_first_words() {
        assert_eq!(title_from("Buy oat milk"), "Buy oat milk");
        // The first line with words on it, six of them, "…" for the rest.
        assert_eq!(title_from("\n\n  Call the garage about the van on Friday morning\nand the tyres"), "Call the garage about the van…");
        // A heading's # and a list's mark aren't words.
        assert_eq!(title_from("## Ideas for the talk"), "Ideas for the talk");
        assert_eq!(title_from("- [ ] Book the venue"), "Book the venue");
        assert_eq!(title_from("• eggs\n• flour"), "eggs");
        // Never longer than 60 characters, cut at a word.
        let long = "Supercalifragilistic expialidocious antidisestablishmentarianism pneumonoultramicroscopic words";
        let t = title_from(long);
        assert!(t.chars().count() <= TITLE_CHARS + 1, "{t}");
        assert_eq!(t, "Supercalifragilistic expialidocious…");
        assert_eq!(title_from(&"x".repeat(90)).chars().count(), TITLE_CHARS + 1);
        // Nothing to say: no title.
        assert_eq!(title_from("   \n \t"), "");
        assert_eq!(title_from("#"), "");
    }

    #[test]
    fn the_title_follows_the_words_until_the_person_names_it() {
        let mut n = note("");
        assert!(apply_text(&mut n, "Pick up the dry cleaning"));
        assert_eq!((n.title.as_str(), n.title_auto), ("Pick up the dry cleaning", true));
        assert!(apply_text(&mut n, "Pick up the parcel"));
        assert_eq!(n.title, "Pick up the parcel");
        // The same text again: nothing changed.
        assert!(!apply_text(&mut n, "Pick up the parcel"));
        // Named in Notes (the title edit clears title_auto): it stays.
        n.title = "Errands".into();
        n.title_auto = false;
        assert!(apply_text(&mut n, "Pick up the parcel and stamps"));
        assert_eq!(n.title, "Errands");
        // Emptied: no title of Yap's left.
        let mut m = note("Ring the dentist");
        apply_text(&mut m, "Ring the dentist");
        apply_text(&mut m, "  ");
        assert_eq!((m.title.as_str(), m.title_auto), ("", false));
    }

    #[test]
    fn a_blank_note_is_one_with_nothing_of_the_persons_in_it() {
        let mut n = note("   \n");
        assert!(is_blank(&n));
        n.title = "Pick up the parcel".into();
        n.title_auto = true;
        assert!(is_blank(&n), "a title Yap made from words that are gone");
        n.title_auto = false;
        assert!(!is_blank(&n), "a title the person typed");
        let mut m = note("");
        m.enhanced_content = "## Summary".into();
        assert!(!is_blank(&m));
        let mut r = note("");
        r.note_type = "meeting".into();
        assert!(!is_blank(&r), "a meeting starting from it");
        assert!(!is_blank(&note("milk")));
    }

    #[test]
    fn it_opens_on_the_last_quick_note_within_a_day_else_a_new_one() {
        let now = 1_000_000;
        let quick = [(3, now - 600), (5, now - 3_600), (2, now - 2 * RESUME_SECS)];
        // Yap just started: the newest, changed ten minutes ago.
        assert_eq!(pick(Current::Unset, &quick, now), Some(3));
        // The one it was on, even if another is newer.
        assert_eq!(pick(Current::Note(5), &quick, now), Some(5));
        // "New note" was pressed: a new one.
        assert_eq!(pick(Current::New, &quick, now), None);
        // Left alone more than a day: a new one.
        assert_eq!(pick(Current::Note(2), &quick, now), None);
        assert_eq!(pick(Current::Unset, &[(2, now - RESUME_SECS)], now), None);
        assert_eq!(pick(Current::Unset, &[(2, now - RESUME_SECS + 1)], now), Some(2));
        // Gone (deleted in Notes, or a meeting now): a new one.
        assert_eq!(pick(Current::Note(9), &quick, now), None);
        assert_eq!(pick(Current::Unset, &[], now), None);
    }

    #[test]
    fn it_comes_up_at_the_bottom_right_of_the_work_area() {
        // 1920 × 1032 above the taskbar, 420 × 380 at 100 %, 24 px in.
        assert_eq!(corner((0, 0, 1920, 1032), (420, 380), 24), (1476, 628, 420, 380));
        // A second monitor left of the primary, at 150 %.
        assert_eq!(corner((-2560, 0, 2560, 1400), (630, 570), 36), (-666, 794, 630, 570));
        // A small screen: no bigger than the room inside the margins.
        assert_eq!(corner((0, 0, 400, 300), (630, 570), 24), (24, 24, 352, 252));
    }

    /// The installed nightly and a dev build share one data folder, and an
    /// older Yap saves notes.json in the shape it knows. A quick note is
    /// made only of fields every older Yap reads and keeps (`source`,
    /// `folder`, `titleAuto` since 2026-10-05, and the folder in the list):
    /// saved back by one, it's still a quick note in the Quick notes folder.
    #[test]
    fn an_older_yap_saving_notes_json_keeps_a_quick_note_a_quick_note() {
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldNote {
            id: u64,
            title: String,
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            title_auto: bool,
            content: String,
            folder: String,
            source: String,
            note_type: String,
            created_ts: u64,
            updated_ts: u64,
        }
        #[derive(serde::Serialize, serde::Deserialize)]
        struct OldStore {
            folders: Vec<String>,
            notes: Vec<OldNote>,
        }
        let id = new_note("Ask Sam about the boiler").id;
        let old: OldStore = serde_json::from_str(&crate::notes::store_json()).unwrap();
        crate::notes::load_json_for_test(&serde_json::to_string(&old).unwrap());
        let n = crate::notes::get(id).unwrap();
        assert!(is_quick(&n));
        assert_eq!((n.folder.as_str(), n.title.as_str(), n.title_auto), (FOLDER, "Ask Sam about the boiler", true));
        assert!(crate::notes::folders().iter().any(|f| f == FOLDER));
    }

    #[test]
    fn a_quick_note_is_made_at_its_first_word_and_goes_when_emptied() {
        // The store's own steps (`save` minus the windows): a note made with
        // the folder, then emptied under the lock.
        assert!(crate::notes::find_folder(FOLDER).is_none());
        let n = new_note("Plant the bulbs");
        assert_eq!((n.folder.as_str(), n.source.as_str(), n.title.as_str(), n.title_auto), (FOLDER, SOURCE, "Plant the bulbs", true));
        assert_eq!(crate::notes::folders().iter().filter(|f| *f == FOLDER).count(), 1);
        assert_eq!(folder(), FOLDER, "made once");
        assert_eq!(crate::notes::folders().iter().filter(|f| *f == FOLDER).count(), 1);
        // Its name in another case (renamed in Notes): found, not made again.
        crate::notes::folder_rename(FOLDER, "quick notes").unwrap();
        let second = new_note("Water the tomatoes");
        assert_eq!(second.folder, "quick notes");
        assert!(!crate::notes::folders().iter().any(|f| f == FOLDER));
        let kept = crate::notes::modify_or_remove(n.id, |n| Ok(((), apply_text(n, "Plant the bulbs in March"))), is_blank).unwrap();
        assert!(kept.is_some());
        assert_eq!(crate::notes::get(n.id).unwrap().title, "Plant the bulbs in March");
        let gone = crate::notes::modify_or_remove(n.id, |n| Ok(((), apply_text(n, ""))), is_blank).unwrap();
        assert!(gone.is_none());
        assert!(crate::notes::get(n.id).is_none());
    }
}
