//! The quick note's **tabs** (Wispr Flow's Scratchpad tabs, adapted;
//! 2026-10-29): several quick notes open at once in the one window. The
//! page keeps them (New, Close, switching: buttons, Ctrl+T, Ctrl+W,
//! Ctrl+Tab) and tells Rust the open notes in order after every change
//! ([`quick_note_tabs`]); they're remembered across restarts in
//! `quick_note_tabs.json` — a file of its own, never notes.json or
//! config.json, so an older Yap sharing the folder never sees it, and
//! losing it loses only which notes were open (they're all in Notes).
//!
//! **What the window opens on** follows the quick note's rule
//! (`quick_note::pick`): the note it was on — the tab in front — or, the
//! first time since Yap started, the newest quick note, while it changed
//! less than a day ago; else a new note (a new tab, or the blank one
//! there). **A tab left alone a day closes** when the window next comes up
//! ([`for_open`]): the note is done, and in Notes, as a note left alone a day
//! was never reopened before tabs. A new note not written yet is never
//! remembered: nothing is kept until there's a word.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The most tabs remembered (the oldest beyond it let go).
pub const MAX_TABS: usize = 20;

/// `quick_note_tabs.json`: the notes open as tabs, in order, and the one in
/// front (none: a new note not written yet).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tabs {
    #[serde(default)]
    pub tabs: Vec<u64>,
    #[serde(default)]
    pub active: Option<u64>,
}

/// The tabs as remembered, read from disk the first time.
static TABS: Mutex<Option<Tabs>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("quick_note_tabs.json")
}

fn loaded(slot: &mut Option<Tabs>) -> &mut Tabs {
    slot.get_or_insert_with(|| {
        let path = path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                crate::config::quarantine_corrupt(&path);
                Tabs::default()
            }),
            Err(_) => Tabs::default(),
        }
    })
}

/// The tabs remembered now.
pub fn remembered() -> Tabs {
    loaded(&mut TABS.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

/// quick_note_tabs.json's saves (`saving.rs`): a failed one keeps the tabs
/// in memory and is tried again with the next change; only logged (which
/// notes were open is all it costs).
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::quiet("quickNoteTabs", "your open quick notes", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(t) = TABS.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let _ = SAVER.write_json(&path(), t, true);
    }
    SAVER.check()
}

/// Remember `tabs` (the page's, in order; ids only), written if they changed.
fn remember(tabs: Tabs) {
    let tabs = tidy(tabs);
    let mut guard = TABS.lock().unwrap_or_else(|p| p.into_inner());
    if *loaded(&mut guard) == tabs {
        return;
    }
    let _ = SAVER.write_json(&path(), &tabs, true);
    *guard = Some(tabs);
}

/// No id twice, at most [`MAX_TABS`] (the last ones kept), the one in front
/// among them (pure).
pub fn tidy(t: Tabs) -> Tabs {
    let mut tabs: Vec<u64> = Vec::with_capacity(t.tabs.len());
    for id in t.tabs {
        if !tabs.contains(&id) {
            tabs.push(id);
        }
    }
    if tabs.len() > MAX_TABS {
        tabs.drain(..tabs.len() - MAX_TABS);
    }
    let active = t.active.filter(|a| tabs.contains(a));
    Tabs { tabs, active }
}

/// The tabs as the window comes up (pure; `quick`: every quick note's id and
/// when it last changed, `picked`: the note it opens on — `quick_note::pick`
/// — `now` in unix seconds): `(open, close)`. A remembered tab stays open
/// while its note is a quick note that changed less than a day ago, or is
/// the picked one; the rest close (deleted, a meeting now, or done). The
/// picked note is added at the end if it wasn't open.
pub fn for_open(remembered: &[u64], quick: &[(u64, u64)], picked: Option<u64>, now: u64) -> (Vec<u64>, Vec<u64>) {
    let recent = |id: u64| {
        quick
            .iter()
            .find(|(q, _)| *q == id)
            .is_some_and(|(_, updated)| now.saturating_sub(*updated) < super::RESUME_SECS)
    };
    let (mut open, close): (Vec<u64>, Vec<u64>) = remembered
        .iter()
        .copied()
        .partition(|&id| Some(id) == picked || recent(id));
    if let Some(p) = picked {
        if !open.contains(&p) {
            open.push(p);
        }
    }
    (open, close)
}

/// Reset app (`privacy/reset.rs`): the notes are gone, so no tab is
/// remembered and the window starts a new note.
pub fn reset() {
    let mut guard = TABS.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(Tabs::default());
    let _ = std::fs::remove_file(path());
    SAVER.nothing_to_save();
    drop(guard);
    super::set_current(super::Current::New);
}

/// The page's tabs changed: the open notes in order (`ids`, notes written
/// so far) and the one in front (`active`; none: a new note). The window
/// opens on that one next time (`quick_note::Current`).
#[tauri::command]
pub fn quick_note_tabs(ids: Vec<u64>, active: Option<u64>) {
    super::set_current(active.map_or(super::Current::New, super::Current::Note));
    remember(Tabs { tabs: ids, active });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::quick_note::RESUME_SECS;

    #[test]
    fn tabs_left_alone_a_day_close_as_the_window_comes_up() {
        let now = 1_000_000;
        let quick = [(1, now - 60), (2, now - 2 * RESUME_SECS), (3, now - 3_600), (4, now - RESUME_SECS)];
        // 2 and 4 are a day old; 9 is gone (deleted, or a meeting now).
        let (open, close) = for_open(&[1, 2, 9, 3, 4], &quick, Some(3), now);
        assert_eq!(open, vec![1, 3]);
        assert_eq!(close, vec![2, 9, 4]);
        // The picked note stays even when it's old (the window was on it).
        let (open, close) = for_open(&[2, 1], &quick, Some(2), now);
        assert_eq!((open, close), (vec![2, 1], vec![]));
        // A picked note that wasn't open: a tab at the end.
        let (open, _) = for_open(&[1], &quick, Some(3), now);
        assert_eq!(open, vec![1, 3]);
        // A new note: the recent tabs stay, nothing added.
        let (open, close) = for_open(&[1, 2], &quick, None, now);
        assert_eq!((open, close), (vec![1], vec![2]));
        assert_eq!(for_open(&[], &quick, None, now), (vec![], vec![]));
    }

    #[test]
    fn remembered_tabs_are_tidy() {
        let t = tidy(Tabs { tabs: vec![4, 5, 4, 6], active: Some(5) });
        assert_eq!(t, Tabs { tabs: vec![4, 5, 6], active: Some(5) });
        // The one in front must be one of them.
        assert_eq!(tidy(Tabs { tabs: vec![1], active: Some(7) }).active, None);
        // At most MAX_TABS, the newest kept.
        let many: Vec<u64> = (1..=(MAX_TABS as u64 + 5)).collect();
        let t = tidy(Tabs { tabs: many, active: Some(1) });
        assert_eq!(t.tabs.len(), MAX_TABS);
        assert_eq!(t.tabs[0], 6);
        assert_eq!(t.active, None);
    }

    /// The file an older Yap never reads: its shape, and one with fields
    /// from a newer Yap or none at all still loads.
    #[test]
    fn the_tabs_file_reads_loosely() {
        let t: Tabs = serde_json::from_str(r#"{"tabs":[3,1],"active":1}"#).unwrap();
        assert_eq!(t, Tabs { tabs: vec![3, 1], active: Some(1) });
        let t: Tabs = serde_json::from_str(r#"{"tabs":[2],"pinned":[2]}"#).unwrap();
        assert_eq!(t, Tabs { tabs: vec![2], active: None });
        assert_eq!(serde_json::from_str::<Tabs>("{}").unwrap(), Tabs::default());
        assert_eq!(serde_json::to_string(&Tabs { tabs: vec![8], active: None }).unwrap(), r#"{"tabs":[8],"active":null}"#);
    }
}
