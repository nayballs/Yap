//! **A Yap window is never left blank** (19 Nov 2026): Rust's half.
//!
//! The pages look after themselves where they can (`index.html`'s head
//! script and `App.svelte`, [frontend.md]): a file of the app's that couldn't
//! be fetched, a view that threw as it started — loaded again, then "Yap
//! couldn't show this window" with a Reload button. What a page can't catch
//! is a page whose script never runs: the dev server down as it loads
//! (WebView2's own error page), a load that hangs, its renderer process
//! crashing or hung. So each window's page says how it's doing
//! ([`page_state`]: `alive` as its head script runs, `mounted` once its view
//! is in, `trouble` when its view failed), and Rust loads it again when it
//! hasn't mounted in time ([`allowance`]):
//! - its head script hasn't even run [`NOT_STARTED`] (10 s) after its load
//!   started;
//! - it ran, but the view isn't in [`STARTED`] (20 s) after: a page whose
//!   script runs looks after itself first, and a busy PC at sign-in can take
//!   a while;
//! - WebView2's own error page (the page's file couldn't be fetched):
//!   [`ERROR_PAGE`] (3 s) after;
//! - the page said its view failed: [`TROUBLE`] (10 s) after (it loads
//!   itself again first, at most once or twice a minute, then shows its
//!   message, and this is the slower retry behind that).
//!
//! **Renderer processes** (`ProcessFailed`, which neither wry nor Tauri
//! handles: checked in wry 0.57 and tauri-runtime-wry 2.12, Nov 2026): one
//! that exited (a crash, out of memory; WebView2 shows its "This page is
//! having a problem" page) is loaded again [`AFTER_CRASH`] later; one that
//! WebView2 keeps saying is unresponsive (it says so every few seconds while
//! input waits) is loaded again once that has lasted [`STUCK`]; a frame's
//! renderer (a chat's visual), the GPU's and the helpers' are WebView2's to
//! restart, only logged. The browser process itself exiting takes every
//! webview with it (WebView2 says to make them again, which Tauri can't):
//! logged as an error, once.
//!
//! **The cap**: at most [`MAX_RELOADS`] reloads of one window in [`BUDGET`]
//! (3 in 5 minutes, [`Budget`]), whatever the reason, then it's left alone
//! (logged once): the page's own message covers the rest. Hidden windows
//! (onboarding, the notepad and the quick note load hidden at start) count
//! too, and are loaded again quietly: navigating a webview never shows its
//! window or takes the focus.
//!
//! **The see-through windows** (the Yap bar, the split preview,
//! [`see_through`]) never get a box painted on them (yap-bar.md: a box over
//! the screen): WebView2's error page there is swapped for a blank page at
//! once, the watchdog loads Yap's page again after, and on giving up a
//! page that isn't running is left blank.
//!
//! Each reload is logged at warn: the window's label, the reason, the
//! count; never anything a page shows. Debug builds in test mode:
//! [`e2e_page_watch`] reports every window's state and shortens the
//! allowances for the e2e suite (`never-blank.spec.js`).
//!
//! [frontend.md]: ../../../../docs/architecture/frontend.md

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Url, Webview};

/// Reloads Rust makes of one window's page, at most, in [`BUDGET`].
pub const MAX_RELOADS: usize = 3;
/// The stretch [`MAX_RELOADS`] is counted over.
pub const BUDGET: Duration = Duration::from_secs(5 * 60);
/// A page whose head script hasn't run: loaded again this long after its load started.
pub const NOT_STARTED: Duration = Duration::from_secs(10);
/// A page whose head script ran but whose view isn't in: this long after its load started.
pub const STARTED: Duration = Duration::from_secs(20);
/// WebView2's own error page: this long after it showed.
pub const ERROR_PAGE: Duration = Duration::from_secs(3);
/// A page that said its view failed: this long after it said so.
pub const TROUBLE: Duration = Duration::from_secs(10);
/// A renderer that exited: its page loaded again this long after.
pub const AFTER_CRASH: Duration = Duration::from_millis(300);
/// Unresponsive this long (WebView2 says so again every few seconds): loaded again.
pub const STUCK: Duration = Duration::from_secs(15);
/// A run of "unresponsive" ends once WebView2 hasn't said so for this long.
pub const STREAK_GAP: Duration = Duration::from_secs(10);

/// How a window's page is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Its load started; its head script hasn't run yet.
    Loading,
    /// Its head script ran; its view isn't in yet.
    Alive,
    /// Its view is in.
    Mounted,
    /// It said its view failed (or couldn't load its code).
    Trouble,
    /// WebView2's own error page: the page's file couldn't be fetched.
    ErrorPage,
}

/// How long a page may stay at `stage` before it's loaded again (from the
/// start of its load for [`Stage::Loading`] and [`Stage::Alive`], from when
/// it got there for the others); `None`: it's fine. Pure.
pub fn allowance(stage: Stage) -> Option<Duration> {
    match stage {
        Stage::Loading => Some(NOT_STARTED),
        Stage::Alive => Some(STARTED),
        Stage::Mounted => None,
        Stage::Trouble => Some(TROUBLE),
        Stage::ErrorPage => Some(ERROR_PAGE),
    }
}

/// [`allowance`], with the e2e suite's shorter one in its place (debug
/// builds; never for [`Stage::Alive`]: a page that runs is on its way).
fn allowance_now(stage: Stage) -> Option<Duration> {
    let normal = allowance(stage)?;
    #[cfg(debug_assertions)]
    {
        let ms = TEST_ALLOWANCE_MS.load(std::sync::atomic::Ordering::Relaxed);
        if ms > 0 && stage != Stage::Alive {
            return Some(Duration::from_millis(ms));
        }
    }
    Some(normal)
}

/// The e2e suite's allowance, ms (0: the real ones).
#[cfg(debug_assertions)]
static TEST_ALLOWANCE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// **The cap**: the reloads of one window in the last [`BUDGET`], at most
/// [`MAX_RELOADS`] of them.
#[derive(Debug, Default, Clone)]
pub struct Budget(VecDeque<Instant>);

impl Budget {
    /// The reloads in the [`BUDGET`] up to `now`.
    pub fn spent(&mut self, now: Instant) -> usize {
        self.forget(now);
        self.0.len()
    }

    /// One more reload at `now`, if the cap allows it.
    pub fn spend(&mut self, now: Instant) -> bool {
        self.forget(now);
        if self.0.len() >= MAX_RELOADS {
            return false;
        }
        self.0.push_back(now);
        true
    }

    fn forget(&mut self, now: Instant) {
        while self.0.front().is_some_and(|t| now.saturating_duration_since(*t) >= BUDGET) {
            self.0.pop_front();
        }
    }
}

/// What a `ProcessFailed` of this kind (`COREWEBVIEW2_PROCESS_FAILED_KIND`) calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// The page's renderer exited: load the page again.
    Reload,
    /// The page's renderer is unresponsive: load it again once it's been so a while.
    IfStuck,
    /// WebView2's to deal with (a frame's renderer, the GPU, a helper): logged.
    Log,
    /// The browser process exited: every webview is gone.
    AllGone,
}

/// Pure: a `ProcessFailed` kind's [`Response`].
pub fn response(kind: i32) -> Response {
    match kind {
        0 => Response::AllGone,
        1 => Response::Reload,
        2 => Response::IfStuck,
        _ => Response::Log,
    }
}

/// A `ProcessFailed` kind in words, for the log.
pub fn kind_words(kind: i32) -> &'static str {
    match kind {
        0 => "the browser process exited",
        1 => "its renderer exited",
        2 => "its renderer is unresponsive",
        3 => "a frame's renderer exited",
        4 => "a utility process exited",
        5 => "the sandbox helper exited",
        6 => "the GPU process exited",
        7 | 8 => "a plugin process exited",
        _ => "a process exited",
    }
}

/// A `ProcessFailed` reason (`COREWEBVIEW2_PROCESS_FAILED_REASON`) in words.
pub fn reason_words(reason: i32) -> &'static str {
    match reason {
        0 => "unexpectedly",
        1 => "unresponsive",
        2 => "terminated",
        3 => "crashed",
        4 => "failed to launch",
        5 => "out of memory",
        6 => "profile deleted",
        _ => "no reason given",
    }
}

/// A run of "unresponsive" reports: the first and the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Streak {
    pub first: Instant,
    pub last: Instant,
}

/// Pure: an "unresponsive" report at `now` after `streak`: the run it makes
/// (a new one after a gap of [`STREAK_GAP`]), and whether it's lasted
/// [`STUCK`], so the page is loaded again.
pub fn unresponsive(streak: Option<Streak>, now: Instant) -> (Streak, bool) {
    let run = match streak {
        Some(s) if now.saturating_duration_since(s.last) < STREAK_GAP => Streak { first: s.first, last: now },
        _ => Streak { first: now, last: now },
    };
    (run, now.saturating_duration_since(run.first) >= STUCK)
}

/// Pure: a window that's transparent, where nothing may ever be painted (a
/// box over the screen): the Yap bar and the split preview.
pub fn see_through(label: &str) -> bool {
    label == crate::bar::overlay::LABEL || label == "split-preview"
}

/// Pure: one of Yap's own pages (the app's files: the embedded frontend or
/// the dev server), not WebView2's error page or a blank one.
pub fn is_ours(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "http" | "https" => matches!(url.host_str(), Some("tauri.localhost" | "localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

/// One window's page, as Rust sees it.
#[derive(Debug)]
struct Page {
    /// Each load (and each reload Rust asks for) a new one: a check armed for
    /// an older one does nothing.
    generation: u64,
    stage: Stage,
    /// When its load started, or when it got to its stage ([`allowance`]).
    since: Instant,
    budget: Budget,
    /// Where Yap's page is (its last load of one of Yap's own).
    home: Option<Url>,
    loads: u32,
    reloads: u32,
    /// The cap was reached (logged once until the page mounts again).
    gave_up: bool,
    /// Why Rust last loaded it again (until it mounts).
    reason: Option<&'static str>,
    streak: Option<Streak>,
    crashes: u32,
    /// The latest top-level navigation (WebView2's id), with how the page
    /// was doing before it, until it completes.
    navigation: Option<(u64, Stage, Instant)>,
}

impl Page {
    fn new() -> Self {
        Self {
            generation: 0,
            stage: Stage::Loading,
            since: Instant::now(),
            budget: Budget::default(),
            home: None,
            loads: 0,
            reloads: 0,
            gave_up: false,
            reason: None,
            streak: None,
            crashes: 0,
            navigation: None,
        }
    }
}

static PAGES: LazyLock<Mutex<HashMap<String, Page>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn pages() -> MutexGuard<'static, HashMap<String, Page>> {
    PAGES.lock().unwrap_or_else(|p| p.into_inner())
}

/// A stage's reason for loading the page again, for the log.
fn stage_words(stage: Stage) -> &'static str {
    match stage {
        Stage::Loading => "its page never started",
        Stage::Alive => "its view never came in",
        Stage::Mounted => "it was fine",
        Stage::Trouble => "its view failed",
        Stage::ErrorPage => "its page couldn't load",
    }
}

/// **Every webview watched from its creation** (lib.rs adds this plugin, no
/// commands of its own): its WebView2 events hooked and a check armed at
/// once, so a window whose very first load never gets anywhere (no content,
/// no error page: a load that hangs as Yap starts) is loaded again too.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("yap-page-watch")
        .on_webview_ready(|webview| {
            #[cfg(windows)]
            win::install(&webview);
            loading(webview.app_handle(), webview.label(), None);
        })
        // A window gone (a Markdown file's, closed): forgotten.
        .on_event(|_, event| {
            if let tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::Destroyed, .. } = event {
                pages().remove(label);
                #[cfg(windows)]
                win::forget(label);
            }
        })
        .build()
}

/// From `on_page_load` in lib.rs, for every webview: a new document's
/// content is coming (WebView2's `ContentLoading`; the old one is gone).
/// Where Yap's page is noted, the bar's clickable regions forgotten, the
/// webview's WebView2 events hooked (once). On Windows the load was
/// watched from its `NavigationStarting` already: never from here, as the
/// page's own first word (`alive`, even `mounted`) can reach Rust before
/// this event does — a load seen to start after its page said it was in
/// would be loaded again for nothing.
pub fn on_page_load(webview: &Webview, payload: &PageLoadPayload<'_>) {
    if payload.event() != PageLoadEvent::Started {
        return;
    }
    let url = payload.url();
    // The blank page a see-through window gets instead of an error page.
    if url.scheme() == "about" {
        return;
    }
    let label = webview.label();
    {
        let mut map = pages();
        let p = map.entry(label.to_string()).or_insert_with(Page::new);
        p.loads += 1;
        if is_ours(url) {
            p.home = Some(url.clone());
        }
    }
    // The bar's clickable spots were the last page's: none until the new
    // page says where its own are (a page that never comes leaves the bar
    // click-through, never an invisible spot that swallows clicks).
    if label == crate::bar::overlay::LABEL {
        crate::bar::bar_regions(Vec::new());
    }
    #[cfg(windows)]
    win::install(webview);
    #[cfg(not(windows))]
    loading(webview.app_handle(), label, None);
}

/// A load of `label`'s page starts: watched from now, its allowance
/// [`NOT_STARTED`]. `navigation`: WebView2's id for it, when known (its
/// completion is matched to it).
fn loading(app: &AppHandle, label: &str, navigation: Option<u64>) {
    let generation = {
        let mut map = pages();
        let p = map.entry(label.to_string()).or_insert_with(Page::new);
        p.generation += 1;
        if let Some(id) = navigation {
            let before = (p.stage, p.since);
            p.navigation = Some((id, before.0, before.1));
        }
        p.stage = Stage::Loading;
        p.since = Instant::now();
        p.generation
    };
    tracing::debug!(window = %label, generation, "A window's page started loading");
    arm(app, label, generation, Stage::Loading);
}

/// A top-level navigation of `label`'s webview ended (WebView2's
/// `NavigationCompleted`, `id` its `NavigationId`): only the latest one
/// counts (one another replaced reports a failure too). Cancelled — a page
/// that stays as it was: back to how it was doing. Failed — WebView2's own
/// error page ([`load_failed`]).
fn navigation_ended(app: &AppHandle, label: &str, id: u64, ok: bool, status: i32, cancelled: bool) {
    tracing::debug!(window = %label, id, ok, status, "A window's navigation ended");
    let restored = {
        let mut map = pages();
        let Some(p) = map.get_mut(label) else { return };
        let latest = p.navigation.map(|(latest, _, _)| latest);
        match ended(latest, id, ok, cancelled) {
            Ended::Older => return,
            Ended::Loaded => {
                p.navigation = None;
                return;
            }
            Ended::Cancelled => {
                // The page it was going to leave is still there.
                match p.navigation.take() {
                    Some((_, before, since)) if p.stage == Stage::Loading => {
                        p.stage = before;
                        p.since = since;
                        p.generation += 1;
                        Some((p.generation, before))
                    }
                    _ => None,
                }
            }
            Ended::Failed => {
                p.navigation = None;
                None
            }
        }
    };
    match restored {
        // (Watched again as it was: a page still on its way, or in trouble.)
        Some((generation, before)) => arm(app, label, generation, before),
        None if !cancelled => load_failed(app, label, status),
        None => {}
    }
}

/// How a top-level navigation ended, for the watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Not the latest one (another replaced it; it reports a failure too),
    /// or none was seen to start: nothing to go by.
    Older,
    /// Its page is coming: the page says the rest.
    Loaded,
    /// Cancelled: the page it would have replaced is still there.
    Cancelled,
    /// WebView2's own error page.
    Failed,
}

/// Pure: navigation `id` ended (`ok`, or `cancelled`), the latest one seen
/// to start being `latest`.
pub fn ended(latest: Option<u64>, id: u64, ok: bool, cancelled: bool) -> Ended {
    if latest != Some(id) {
        Ended::Older
    } else if ok {
        Ended::Loaded
    } else if cancelled {
        Ended::Cancelled
    } else {
        Ended::Failed
    }
}

/// Check `label`'s page once its stage's allowance is up.
fn arm(app: &AppHandle, label: &str, generation: u64, stage: Stage) {
    let Some(after) = allowance_now(stage) else { return };
    let (app, label) = (app.clone(), label.to_string());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(after).await;
        check(&app, &label, generation);
    });
}

/// A page's allowance is up: mounted (or loaded again since) → nothing;
/// moved on to a stage with longer → checked again then; else loaded again.
fn check(app: &AppHandle, label: &str, generation: u64) {
    if app.get_webview_window(label).is_none() {
        // (A Markdown file's window, closed.)
        pages().remove(label);
        return;
    }
    let (stage, wait) = {
        let map = pages();
        let Some(p) = map.get(label) else { return };
        if p.generation != generation {
            return;
        }
        let Some(allow) = allowance_now(p.stage) else { return };
        let due = p.since + allow;
        (p.stage, due.checked_duration_since(Instant::now()))
    };
    match wait {
        Some(wait) if !wait.is_zero() => {
            let (app, label) = (app.clone(), label.to_string());
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(wait).await;
                check(&app, &label, generation);
            });
        }
        _ => reload(app, label, stage_words(stage), Some(generation)),
    }
}

/// Load `label`'s page again, within the cap: Yap's own page (a crashed
/// page, an error page or a blank one all go back to it), quietly. `of`: the
/// load a check was armed for (two checks of one load — its start's and its
/// trouble's — reload it once).
fn reload(app: &AppHandle, label: &str, why: &'static str, of: Option<u64>) {
    let now = Instant::now();
    let (home, count, generation) = {
        let mut map = pages();
        let Some(p) = map.get_mut(label) else { return };
        if of.is_some_and(|g| g != p.generation) {
            return;
        }
        if !p.budget.spend(now) {
            let first = !p.gave_up;
            p.gave_up = true;
            // A see-through window whose page isn't running shows
            // WebView2's own page (an error, a crashed renderer's): blank
            // is better than a box over the screen.
            let blank = see_through(label) && !matches!(p.stage, Stage::Alive | Stage::Trouble | Stage::Mounted);
            drop(map);
            if first {
                tracing::warn!(window = %label, reason = why, "Not reloading the window's page again: {MAX_RELOADS} reloads in 5 minutes already");
            }
            if blank {
                blank_page(app, label);
            }
            return;
        }
        // A new generation now: a check armed for the old one does nothing
        // (no second reload before this one's load starts).
        p.generation += 1;
        p.stage = Stage::Loading;
        p.since = now;
        p.reloads += 1;
        p.reason = Some(why);
        (p.home.clone(), p.budget.spent(now), p.generation)
    };
    tracing::warn!(window = %label, reason = why, count, "Reloading a window's page ({count} of {MAX_RELOADS} in 5 minutes)");
    // (Should its load never start, this checks again.)
    arm(app, label, generation, Stage::Loading);
    let Some(webview) = app.get_webview_window(label) else { return };
    let result = match home.or_else(|| default_home(app)) {
        Some(url) => webview.navigate(url),
        None => webview.reload(),
    };
    if let Err(e) = result {
        tracing::warn!(window = %label, "Couldn't reload a window's page: {e}");
    }
}

/// Where Yap's page is when no load of it was seen: the dev server, or the
/// embedded frontend.
fn default_home(app: &AppHandle) -> Option<Url> {
    if tauri::is_dev() {
        if let Some(url) = app.config().build.dev_url.clone() {
            return Some(url);
        }
    }
    Url::parse("http://tauri.localhost/").ok()
}

/// A see-through window's page swapped for a blank one (transparent: no box).
fn blank_page(app: &AppHandle, label: &str) {
    let Some(webview) = app.get_webview_window(label) else { return };
    if let Ok(url) = Url::parse("about:blank") {
        if let Err(e) = webview.navigate(url) {
            tracing::warn!(window = %label, "Couldn't blank a see-through window's page: {e}");
        }
    }
}

/// **A page says how it's doing** (index.html's head script): `alive` as
/// its head script runs, `mounted` once its view is in, `trouble` when its
/// view failed or its code couldn't load (it shows its message; it says
/// why itself through `frontend_log`, at warn), `reloading` as it loads
/// itself again (its own retry, the message's Reload): watched from then, a
/// load that hangs before any content included.
#[tauri::command]
pub fn page_state(webview: Webview, state: String) {
    let label = webview.label().to_string();
    if state == "reloading" {
        loading(webview.app_handle(), &label, None);
        return;
    }
    let now = Instant::now();
    let (arm_for, back) = {
        let mut map = pages();
        let p = map.entry(label.clone()).or_insert_with(Page::new);
        match state.as_str() {
            "alive" => {
                if matches!(p.stage, Stage::Loading | Stage::ErrorPage) {
                    p.stage = Stage::Alive;
                }
                (None, None)
            }
            "mounted" => {
                p.stage = Stage::Mounted;
                p.gave_up = false;
                p.streak = None;
                (None, p.reason.take())
            }
            "trouble" => {
                p.stage = Stage::Trouble;
                p.since = now;
                (Some(p.generation), None)
            }
            _ => (None, None),
        }
    };
    if let Some(why) = back {
        tracing::info!(window = %label, reason = why, "A window's page is back after a reload");
    }
    if let Some(generation) = arm_for {
        arm(webview.app_handle(), &label, generation, Stage::Trouble);
    }
    tracing::debug!(window = %label, state = %state, "A window's page said how it's doing");
}

/// WebView2's `ProcessFailed` for `label`'s webview.
fn process_failed(app: &AppHandle, label: &str, kind: i32, reason: i32, exit_code: i32) {
    let what = kind_words(kind);
    let why = reason_words(reason);
    match response(kind) {
        Response::AllGone => {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| {
                tracing::error!(reason = why, exit_code, "WebView2's browser process exited: every Yap window's page is gone until Yap restarts");
            });
        }
        Response::Log => {
            tracing::info!(window = %label, reason = why, exit_code, "WebView2: {what}");
        }
        Response::Reload => {
            let crashes = {
                let mut map = pages();
                let p = map.entry(label.to_string()).or_insert_with(Page::new);
                p.crashes += 1;
                p.stage = Stage::Loading;
                p.crashes
            };
            tracing::warn!(window = %label, reason = why, exit_code, crashes, "A window's page stopped: {what}");
            let (app, label) = (app.clone(), label.to_string());
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(AFTER_CRASH).await;
                reload(&app, &label, "its renderer exited", None);
            });
        }
        Response::IfStuck => {
            let now = Instant::now();
            let stuck = {
                let mut map = pages();
                let p = map.entry(label.to_string()).or_insert_with(Page::new);
                let (run, stuck) = unresponsive(p.streak, now);
                p.streak = if stuck { None } else { Some(run) };
                stuck
            };
            tracing::warn!(window = %label, stuck, "A window's page isn't responding");
            if stuck {
                reload(app, label, "its renderer stopped responding", None);
            }
        }
    }
}

/// The latest load of `label`'s page ended on WebView2's error page
/// (`status`: `COREWEBVIEW2_WEB_ERROR_STATUS`): loaded again
/// [`ERROR_PAGE`] later; a see-through window's swapped for a blank page at
/// once.
fn load_failed(app: &AppHandle, label: &str, status: i32) {
    let generation = {
        let mut map = pages();
        let p = map.entry(label.to_string()).or_insert_with(Page::new);
        p.stage = Stage::ErrorPage;
        p.since = Instant::now();
        p.generation
    };
    tracing::warn!(window = %label, status, "A window's page couldn't load (WebView2's error page)");
    if see_through(label) {
        blank_page(app, label);
    }
    arm(app, label, generation, Stage::ErrorPage);
}

/// Debug builds in test mode (`never-blank.spec.js`): every window's page
/// as Rust sees it; `allowance_ms` (0: the real ones) puts one allowance in
/// place of every stage's but [`Stage::Alive`]'s, `reset` forgets the
/// reloads counted against the cap.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_page_watch(allowance_ms: Option<u64>, reset: Option<bool>) -> Result<serde_json::Value, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in test mode.".into());
    }
    if let Some(ms) = allowance_ms {
        TEST_ALLOWANCE_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
    }
    let now = Instant::now();
    let mut map = pages();
    if reset == Some(true) {
        for p in map.values_mut() {
            p.budget = Budget::default();
            p.gave_up = false;
            p.reloads = 0;
            p.crashes = 0;
            p.reason = None;
            p.streak = None;
        }
    }
    let windows: serde_json::Map<String, serde_json::Value> = map
        .iter_mut()
        .map(|(label, p)| {
            let spent = p.budget.spent(now);
            (
                label.clone(),
                serde_json::json!({
                    "stage": p.stage,
                    "loads": p.loads,
                    "reloads": p.reloads,
                    "spent": spent,
                    "gaveUp": p.gave_up,
                    "reason": p.reason,
                    "crashes": p.crashes,
                    "home": p.home.as_ref().map(|u| u.as_str().to_string()),
                }),
            )
        })
        .collect();
    Ok(serde_json::json!({
        "allowanceMs": TEST_ALLOWANCE_MS.load(std::sync::atomic::Ordering::Relaxed),
        "windows": windows,
    }))
}

// ---- WebView2's events ---------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use tauri::{AppHandle, Manager, Webview};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2ProcessFailedEventArgs2, COREWEBVIEW2_PROCESS_FAILED_KIND,
        COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED,
    };
    use webview2_com::{take_pwstr, NavigationCompletedEventHandler, NavigationStartingEventHandler, ProcessFailedEventHandler};
    use windows::core::{IUnknown, Interface, BOOL, PWSTR};

    thread_local! {
        /// The webviews hooked so far, by label (WebView2 lives on the main
        /// thread, as does this): a reload keeps its hooks.
        static HOOKED: RefCell<HashMap<String, IUnknown>> = RefCell::new(HashMap::new());
    }

    /// A window gone: its webview no longer held (on the main thread, as
    /// every run event is).
    pub fn forget(label: &str) {
        HOOKED.with(|h| h.borrow_mut().remove(label));
    }

    /// Hook `webview`'s `NavigationStarting`, `NavigationCompleted` and
    /// `ProcessFailed`, once per WebView2 instance (called as it's made and
    /// on every load).
    pub fn install(webview: &Webview) {
        let label = webview.label().to_string();
        let app = webview.app_handle().clone();
        let result = webview.with_webview(move |platform| {
            if let Err(e) = hook(&app, &label, &platform.controller()) {
                tracing::warn!(window = %label, "Couldn't watch a window's page for crashes: {e}");
            }
        });
        if let Err(e) = result {
            tracing::warn!("Couldn't reach a webview to watch it for crashes: {e}");
        }
    }

    fn hook(app: &AppHandle, label: &str, controller: &ICoreWebView2Controller) -> windows::core::Result<()> {
        let core: ICoreWebView2 = unsafe { controller.CoreWebView2()? };
        let identity: IUnknown = core.cast()?;
        if HOOKED.with(|h| h.borrow().get(label) == Some(&identity)) {
            return Ok(());
        }
        let mut token = 0i64;
        let (a, l) = (app.clone(), label.to_string());
        let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            unsafe { args.ProcessFailedKind(&mut kind)? };
            let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON(-1);
            let mut code = 0i32;
            if let Ok(more) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                unsafe {
                    let _ = more.Reason(&mut reason);
                    let _ = more.ExitCode(&mut code);
                }
            }
            super::process_failed(&a, &l, kind.0, reason.0, code);
            Ok(())
        }));
        unsafe { core.add_ProcessFailed(&failed, &mut token)? };
        // A load starts: watched from here. WebView2 waits for this handler
        // before the navigation goes on, so no word of the new page's can
        // reach Rust ahead of it (wry's handler, made first, has said
        // whether it's cancelled).
        let (a, l) = (app.clone(), label.to_string());
        let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut cancel = BOOL(0);
            let mut id = 0u64;
            let mut uri = PWSTR::null();
            unsafe {
                let _ = args.Cancel(&mut cancel);
                let _ = args.NavigationId(&mut id);
                let _ = args.Uri(&mut uri);
            }
            let uri = take_pwstr(uri);
            // (The blank page a see-through window gets for an error page.)
            if !cancel.as_bool() && !uri.starts_with("about:") {
                super::loading(&a, &l, Some(id));
            }
            Ok(())
        }));
        unsafe { core.add_NavigationStarting(&starting, &mut token)? };
        let (a, l) = (app.clone(), label.to_string());
        let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut ok = BOOL(1);
            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
            let mut id = 0u64;
            unsafe {
                let _ = args.IsSuccess(&mut ok);
                let _ = args.WebErrorStatus(&mut status);
                let _ = args.NavigationId(&mut id);
            }
            let cancelled = status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED;
            super::navigation_ended(&a, &l, id, ok.as_bool(), status.0, cancelled);
            Ok(())
        }));
        unsafe { core.add_NavigationCompleted(&completed, &mut token)? };
        HOOKED.with(|h| h.borrow_mut().insert(label.to_string(), identity));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_reloads_in_five_minutes_then_none_until_the_first_is_five_minutes_old() {
        let start = Instant::now();
        let mut b = Budget::default();
        assert!(b.spend(start));
        assert!(b.spend(start + Duration::from_secs(10)));
        assert!(b.spend(start + Duration::from_secs(20)));
        assert!(!b.spend(start + Duration::from_secs(30)), "a fourth within 5 minutes");
        assert!(!b.spend(start + Duration::from_secs(299)));
        assert_eq!(b.spent(start + Duration::from_secs(299)), 3);
        // The first is 5 minutes old: one more.
        assert!(b.spend(start + Duration::from_secs(300)));
        assert!(!b.spend(start + Duration::from_secs(305)));
        assert_eq!(b.spent(start + Duration::from_secs(321)), 1, "the 10 s and 20 s ones gone too: the 300 s one left");
    }

    #[test]
    fn a_page_that_never_starts_waits_less_than_one_whose_script_runs() {
        assert_eq!(allowance(Stage::Loading), Some(Duration::from_secs(10)));
        assert_eq!(allowance(Stage::Alive), Some(Duration::from_secs(20)));
        assert_eq!(allowance(Stage::ErrorPage), Some(Duration::from_secs(3)));
        assert_eq!(allowance(Stage::Trouble), Some(Duration::from_secs(10)));
        assert_eq!(allowance(Stage::Mounted), None, "a mounted page is never reloaded");
        assert!(allowance(Stage::ErrorPage) < allowance(Stage::Loading));
        assert!(allowance(Stage::Loading) < allowance(Stage::Alive));
    }

    #[test]
    fn only_the_pages_own_renderer_is_reloaded() {
        assert_eq!(response(1), Response::Reload, "its renderer exited");
        assert_eq!(response(2), Response::IfStuck, "unresponsive: only once it lasts");
        assert_eq!(response(0), Response::AllGone, "the browser process");
        for kind in [3, 4, 5, 6, 7, 8, 9, 42] {
            assert_eq!(response(kind), Response::Log, "kind {kind} is WebView2's to restart");
        }
        assert_eq!(kind_words(3), "a frame's renderer exited");
        assert_eq!(reason_words(3), "crashed");
        assert_eq!(reason_words(-1), "no reason given");
    }

    #[test]
    fn unresponsive_reloads_only_once_it_has_lasted() {
        let t0 = Instant::now();
        let (run, stuck) = unresponsive(None, t0);
        assert!(!stuck);
        let (run, stuck) = unresponsive(Some(run), t0 + Duration::from_secs(5));
        assert!(!stuck);
        let (run, stuck) = unresponsive(Some(run), t0 + Duration::from_secs(10));
        assert!(!stuck);
        let (_, stuck) = unresponsive(Some(run), t0 + Duration::from_secs(15));
        assert!(stuck, "15 s of reports");
        // A gap of 10 s or more starts a new run.
        let (run, _) = unresponsive(None, t0);
        let (run, stuck) = unresponsive(Some(run), t0 + Duration::from_secs(12));
        assert!(!stuck);
        assert_eq!(run.first, t0 + Duration::from_secs(12));
    }

    #[test]
    fn only_the_latest_navigation_ending_counts() {
        // One another replaced reports a failure (aborted): never an error page.
        assert_eq!(ended(Some(7), 6, false, false), Ended::Older);
        assert_eq!(ended(None, 6, false, false), Ended::Older, "none seen to start (before the hooks)");
        assert_eq!(ended(Some(7), 7, true, false), Ended::Loaded);
        assert_eq!(ended(Some(7), 7, false, true), Ended::Cancelled, "the old page stays");
        assert_eq!(ended(Some(7), 7, false, false), Ended::Failed);
    }

    #[test]
    fn yaps_own_pages_and_the_see_through_windows() {
        for ok in ["http://tauri.localhost/", "http://tauri.localhost/index.html", "http://localhost:51437/", "tauri://localhost/"] {
            assert!(is_ours(&Url::parse(ok).unwrap()), "{ok}");
        }
        for not in ["about:blank", "chrome-error://chromewebdata/", "https://example.com/", "file:///C:/index.html"] {
            assert!(!is_ours(&Url::parse(not).unwrap()), "{not}");
        }
        assert!(see_through("overlay"));
        assert!(see_through("split-preview"));
        for opaque in ["settings", "onboarding", "notepad", "quick-note", "doc-0123456789ab"] {
            assert!(!see_through(opaque), "{opaque}");
        }
    }
}
