//! **A page's picture, for its card in the thread** (14 Nov 2026; the
//! maintainer, from the Claude Code app: a page Claude opens shows as a card —
//! a small picture of it, its title, Open and ⋯ — where Yap had a pill). Taken
//! once a page brought up beside the chat has loaded and drawn — what it
//! looked like then: its top, [`ASPECT`] wide —, kept in `chat-images/` like a
//! picture an answer showed (swept with them: `chats::image_ids`), and set on
//! the answer's [`ShownPage::thumb`]: as its answer is saved ([`fill`]), or on
//! the saved answer when it's taken after (`chats::set_page_thumb`). The
//! page's own title comes with it (its tab's then): an answer that brought a
//! page up unread knows only its site ([`apply`]). Live, [`EVENT`] tells the
//! thread. Never sent to a model, never logged but by size.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use super::ShownPage;

/// The event the thread hears a page's picture by: `{url, thumb, title}`.
pub const EVENT: &str = "yap-page-thumb";
/// Its shape: the top of the page, this wide to this tall.
pub const ASPECT: (u32, u32) = (2, 1);
/// At most this many pixels wide (a card is 360 CSS px wide: twice that for a fine screen).
pub const MAX_W: u32 = 720;
/// The page's tab is made within this (by the open, or by the Chat view's).
const TAB_MAX: Duration = Duration::from_secs(8);
/// It may take this long to load; its picture is taken then anyway.
const LOAD_MAX: Duration = Duration::from_secs(12);
/// Loaded: a moment more to draw.
const SETTLE: Duration = Duration::from_millis(900);
/// Pictures this run keeps at hand for answers not saved yet.
const REMEMBER: usize = 40;

/// **A page's picture**: its id in `chat-images/` and its size.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageThumb {
    pub id: String,
    pub w: u32,
    pub h: u32,
}

/// **What was taken of a page**: its picture, and its own title then (its
/// tab's; empty when it had none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Taken {
    pub thumb: PageThumb,
    pub title: String,
}

/// What was taken this run, by page (`web::normal_url`), newest last.
static TAKEN: Mutex<Vec<(String, Taken)>> = Mutex::new(Vec::new());

fn with_taken<R>(f: impl FnOnce(&mut Vec<(String, Taken)>) -> R) -> R {
    f(&mut TAKEN.lock().unwrap_or_else(|e| e.into_inner()))
}

/// What was taken of `url`'s page this run, if anything.
pub fn taken(url: &str) -> Option<Taken> {
    let key = crate::web::normal_url(url);
    with_taken(|t| t.iter().rev().find(|(k, _)| *k == key).map(|(_, p)| p.clone()))
}

fn remember(url: &str, taken: &Taken) {
    let key = crate::web::normal_url(url);
    with_taken(|t| {
        t.retain(|(k, _)| *k != key);
        t.push((key, taken.clone()));
        if t.len() > REMEMBER {
            let extra = t.len() - REMEMBER;
            t.drain(..extra);
        }
    });
}

fn forget(url: &str) {
    let key = crate::web::normal_url(url);
    with_taken(|t| t.retain(|(k, _)| *k != key));
}

/// **A page as taken, on an answer's page**: its picture where it has none,
/// and the page's own title where the answer knew only its site (or
/// nothing) — never an address for a title. Whether it changed. Pure.
pub fn apply(p: &mut ShownPage, t: &Taken) -> bool {
    let mut changed = false;
    if p.thumb.is_none() {
        p.thumb = Some(t.thumb.clone());
        changed = true;
    }
    let title = t.title.trim();
    let only_site = p.title.trim().is_empty() || p.title == p.site;
    let an_address = title.contains("://") || crate::web::normal_url(title) == crate::web::normal_url(&p.url);
    if only_site && !title.is_empty() && !an_address && p.title != title {
        p.title = title.chars().take(200).collect();
        changed = true;
    }
    changed
}

/// **An answer's pages, each with its picture** (and the page's own title)
/// where one's been taken; one taken after the answer is saved is set on it
/// then (`chats::set_page_thumb`).
pub fn fill(pages: &mut [ShownPage]) {
    for p in pages.iter_mut().filter(|p| p.thumb.is_none()) {
        if let Some(t) = taken(&p.url) {
            apply(p, &t);
        }
    }
}

/// **The top of a page's shot**: [`ASPECT`] of its width from the top (all
/// of it when it's shorter), at most [`MAX_W`] wide. Pure.
pub fn top_of(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    let tall = (w * ASPECT.1 / ASPECT.0).clamp(1, h);
    let top = crate::chat::pages::see::crop(rgba, w, 0, 0, w, tall);
    if w <= MAX_W {
        return (top, w, tall);
    }
    let dh = ((tall as u64 * MAX_W as u64) / w as u64).max(1) as u32;
    (crate::snip::images::shrink(&top, w, tall, MAX_W, dh), MAX_W, dh)
}

/// **Nothing drawn yet**: every pixel the same colour (a page still white,
/// or blank). Pure.
pub fn blank(rgba: &[u8]) -> bool {
    let (px, _) = rgba.as_chunks::<4>();
    let Some((first, rest)) = px.split_first() else { return true };
    rest.iter().all(|p| p == first)
}

/// **A picture of `url`'s page for its card**, taken in the background once
/// it has loaded (the turn goes on meanwhile). Nothing with the stand-in
/// browser of a test or an evaluation, or without Yap's window.
pub fn take_soon(url: &str) {
    #[cfg(any(test, debug_assertions))]
    if super::sim::with(|_| ()).is_some() {
        return;
    }
    if super::view::app().is_none() {
        return;
    }
    // (A picture from before is the page as it was then: this one's new.)
    forget(url);
    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        match take(&url).await {
            Ok(Some(taken)) => {
                remember(&url, &taken);
                if let Some(app) = super::view::app() {
                    let _ = app.emit(EVENT, serde_json::json!({ "url": url, "thumb": taken.thumb, "title": taken.title }));
                }
                let set = crate::chat::chats::set_page_thumb(&url, &taken);
                tracing::info!(w = taken.thumb.w, h = taken.thumb.h, answers = set, "Chat: a page's picture taken for its card");
            }
            Ok(None) => tracing::debug!("Chat: no picture for a page's card (no tab, or nothing drawn)"),
            Err(e) => tracing::debug!("Chat: a page's picture wasn't taken: {e}"),
        }
    });
}

/// Waits for `url`'s tab and its load, then its picture (kept) and its
/// title; `None`: no tab came, or nothing was drawn.
async fn take(url: &str) -> Result<Option<Taken>, String> {
    let key = super::view::key_of(url);
    let started = Instant::now();
    while !super::backend::tabs().await.iter().any(|t| t.key == key) {
        if started.elapsed() > TAB_MAX {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let started = Instant::now();
    while !super::backend::loaded(&key).await && started.elapsed() < LOAD_MAX {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    tokio::time::sleep(SETTLE).await;
    let png = super::backend::shot(&key).await?;
    let title = super::backend::tabs().await.into_iter().find(|t| t.key == key).map(|t| t.title).unwrap_or_default();
    let thumb = tauri::async_runtime::spawn_blocking(move || -> Result<Option<PageThumb>, String> {
        let (rgba, w, h) = crate::snip::images::decode_png(&png)?;
        if blank(&rgba) {
            return Ok(None);
        }
        let (top, tw, th) = top_of(&rgba, w, h);
        let png = crate::snip::images::encode_png(&top, tw, th, true)?;
        let saved = crate::snip::images::save(&png, tw, th)?;
        Ok(Some(PageThumb { id: saved.id, w: saved.w, h: saved.h }))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(thumb.map(|thumb| Taken { thumb, title }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        (0..w * h).flat_map(|i| [(i % 251) as u8, (i / w % 251) as u8, 90, 255]).collect()
    }

    #[test]
    fn a_pages_picture_is_its_top_twice_as_wide_as_tall_and_never_wider_than_the_card_needs() {
        // A panel's page, 1050 × 1300 device pixels: its top 1050 × 525, made 720 × 360.
        let (top, w, h) = top_of(&picture(1050, 1300), 1050, 1300);
        assert_eq!((w, h, top.len()), (720, 360, 720 * 360 * 4));
        // A narrow one stays its own pixels: 600 × 300 of 600 × 900.
        let full = picture(600, 900);
        let (top, w, h) = top_of(&full, 600, 900);
        assert_eq!((w, h), (600, 300));
        assert_eq!(&top[..600 * 4], &full[..600 * 4], "its first row is the page's first");
        // Shorter than that: all of it.
        let (_, w, h) = top_of(&picture(600, 200), 600, 200);
        assert_eq!((w, h), (600, 200));
    }

    #[test]
    fn a_page_with_nothing_drawn_has_no_picture() {
        assert!(blank(&[255, 255, 255, 255].repeat(64)));
        assert!(blank(&[]));
        let mut drawn = [255, 255, 255, 255].repeat(64);
        drawn[40] = 10;
        assert!(!blank(&drawn));
    }

    #[test]
    fn an_answers_pages_take_the_pictures_taken_by_their_address_a_newer_one_in_place_of_the_old() {
        let url = format!("https://wiki.example.com/thumb-{}", std::process::id());
        let mut pages = vec![ShownPage { url: format!("{url}/"), title: "Fort Joy".into(), ..Default::default() }, ShownPage { url: "https://other.example/".into(), ..Default::default() }];
        fill(&mut pages);
        assert_eq!(pages[0].thumb, None);
        let shot = |id: &str| Taken { thumb: PageThumb { id: id.into(), w: 720, h: 360 }, title: "Fort Joy - Example Wiki".into() };
        remember(&url, &shot("img_aaaaaaaaaaaaaaaa"));
        remember(&url, &shot("img_bbbbbbbbbbbbbbbb"));
        fill(&mut pages);
        assert_eq!(pages[0].thumb.as_ref().map(|t| t.id.clone()), Some("img_bbbbbbbbbbbbbbbb".into()));
        assert_eq!(pages[1].thumb, None);
        // One already there is kept.
        remember(&url, &shot("img_cccccccccccccccc"));
        fill(&mut pages);
        assert_eq!(pages[0].thumb.as_ref().map(|t| t.id.clone()), Some("img_bbbbbbbbbbbbbbbb".into()));
        forget(&url);
        assert_eq!(taken(&url), None);
    }

    #[test]
    fn the_pages_own_title_replaces_only_a_title_that_was_just_its_site() {
        let t = |title: &str| Taken { thumb: PageThumb { id: "img_aaaaaaaaaaaaaaaa".into(), w: 720, h: 360 }, title: title.into() };
        let page = |title: &str| ShownPage { url: "https://wiki.example.com/wiki/Fort_Joy".into(), title: title.into(), site: "wiki.example.com".into(), ..Default::default() };
        // Brought up unread: its site for a title, then the page's own.
        let mut p = page("wiki.example.com");
        assert!(apply(&mut p, &t("Fort Joy - Example Wiki")));
        assert_eq!((p.title.as_str(), p.thumb.is_some()), ("Fort Joy - Example Wiki", true));
        // A title the answer knew (a search result's, "Search: …") stays.
        let mut p = page("Search: fort joy");
        apply(&mut p, &t("fort joy - Search"));
        assert_eq!(p.title, "Search: fort joy");
        // An address or nothing for a title is never a title.
        let mut p = page("");
        apply(&mut p, &t("https://wiki.example.com/wiki/Fort_Joy"));
        assert_eq!(p.title, "");
        let mut p = page("wiki.example.com");
        apply(&mut p, &t("  "));
        assert_eq!(p.title, "wiki.example.com");
        // Nothing new: nothing changed.
        let mut p = page("Fort Joy - Example Wiki");
        p.thumb = Some(t("").thumb);
        assert!(!apply(&mut p, &t("Fort Joy - Example Wiki")));
    }
}
