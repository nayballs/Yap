//! **Snip to Chat**: drag a box over any screen, and the image goes into
//! Chat for the selected AI model (docs/architecture/snip.md).
//!
//! **How it starts**: the snip shortcut (Win+Alt+S by default, Settings →
//! Shortcuts; `input_hook.rs`), ✂ on the Yap bar, the tray's "Snip to
//! Chat", Chat's + menu, or saying "snip" ("snip it", "take a snip",
//! "screenshot", "take a screenshot") as a whole dictation or Voice
//! Assistant question ([`is_snip_phrase`], `pipeline.rs`; "Say 'snip' to
//! take a screenshot", on by default). Pressing the shortcut again while
//! the overlay is up cancels it. Not during a dictation (the bar says
//! "Finish the dictation first").
//!
//! **What happens** ([`start`], on its own thread): Yap's windows leave the
//! capture (`capture::for_snip`: WDA_EXCLUDEFROMCAPTURE for the moment of
//! the capture, so no Yap window is in the shot), every monitor is captured
//! at once (`snip/capture.rs`: Windows.Graphics.Capture, FP16 converted
//! with the SDR white level on an HDR monitor, GDI as the fallback), and the
//! native overlay comes up over the frozen pictures (`snip/overlay.rs`), one
//! window per monitor at native pixels. **Release = done**: the box is cut
//! out of its monitor's frame (`snip/geom.rs`), saved as a PNG in
//! `<data>/chat-images/` (`snip/images.rs`), and the main window comes up
//! on Chat with it in the message box (`yap-snip`): in the conversation
//! Chat was showing, if the main window was on screen on one; else a new
//! chat. Nothing is ever sent by itself. Esc, right-click or the shortcut
//! again cancel, and the focus goes back to the app you were in.
//!
//! **To the model**: a copy fitted to the provider's size limit is made in
//! the background as the image lands ([`images::prepare`]), so Send is
//! instant; the turn carries it as an `image_url` part ([`turn`]). Which
//! models read images, and the one-click switch to one that can: [`vision`].
//!
//! **Timings on the maintainer's PC** (`yap.exe snip-dump`, an optimised
//! build, two 2560×1440 monitors, one HDR at 240 nits): every monitor
//! captured in ~26 ms (the FP16 conversion 1.5 ms of it), the overlay up
//! ~23 ms after: **~50–60 ms from the shortcut to the overlay**; the
//! Direct3D device (~120 ms) is made in the background at launch.
//!
//! Logs say counts, sizes and timings only — never a window, an app, a
//! pixel or a path.

pub mod capture;
#[cfg(debug_assertions)]
pub mod dump;
pub mod geom;
pub mod hdr;
pub mod images;
pub mod overlay;
pub mod turn;
pub mod vision;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use capture::{Frame, Method};
use geom::Rect;

/// The page event: a snip landed (`{ image, conversationId }`).
pub const EVENT: &str = "yap-snip";

/// Where a snip was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    Shortcut,
    Bar,
    Tray,
    Voice,
    Chat,
}

impl Origin {
    pub fn parse(s: Option<&str>) -> Origin {
        match s {
            Some("bar") => Origin::Bar,
            Some("tray") => Origin::Tray,
            Some("voice") => Origin::Voice,
            Some("chat") => Origin::Chat,
            _ => Origin::Shortcut,
        }
    }
}

/// A snip under way: its frozen pictures until it ends.
struct Snip {
    generation: u64,
    frames: Vec<Frame>,
    /// The conversation it goes to (`None`: a new chat).
    conversation: Option<u64>,
    /// The window to give the focus back to on a cancel.
    foreground: isize,
    started: Instant,
    origin: Origin,
}

static BUSY: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static CURRENT: Mutex<Option<Snip>> = Mutex::new(None);

/// How the last snip went (the e2e suite and the robot tests read it).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Last {
    /// "snipped" | "cancelled" | "failed" | "refused".
    pub outcome: String,
    pub reason: String,
    pub monitors: usize,
    pub hdr_monitors: usize,
    /// The monitors snipped over (where, how big, HDR): the robot tests
    /// put their swatches on each.
    pub screens: Vec<capture::Monitor>,
    /// How each monitor was captured.
    pub methods: Vec<Method>,
    pub capture_ms: f64,
    /// From the request to the overlay on screen.
    pub overlay_ms: f64,
    /// From the release to the image saved.
    pub save_ms: f64,
    pub focused: bool,
    pub image: Option<crate::chat::chats::ChatImage>,
    pub conversation_id: Option<u64>,
}

static LAST: Mutex<Option<Last>> = Mutex::new(None);

fn set_last(last: Last) {
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(last);
}

pub fn last() -> Option<Last> {
    LAST.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// What a test run captures instead of the screen (`e2e.rs`): fake
/// monitors' frames. Never read in a real run.
static FAKE: Mutex<Option<Vec<Frame>>> = Mutex::new(None);

/// Debug builds, test mode: the next snip "captures" these frames.
#[cfg(debug_assertions)]
pub fn set_fake_frames(frames: Vec<Frame>) {
    *FAKE.lock().unwrap_or_else(|p| p.into_inner()) = Some(frames);
}

/// Whether this run snips fake monitors (test mode, not the robot tests,
/// which capture the real screen).
fn faking() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

// ---- saying "snip" ------------------------------------------------------------------------

/// Whether a whole dictation (or Voice Assistant question) asks for a snip:
/// "snip", "snip it", "snip this", "take a snip", "screenshot", "take a
/// screenshot", "screen shot", with "please" or the punctuation a
/// transcript adds allowed around it. Only the whole utterance counts: "snip
/// the ends off" or "take a screenshot of the error and send it" are
/// dictated as they are. Pure.
pub fn is_snip_phrase(text: &str) -> bool {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    let mut w: Vec<&str> = words.iter().map(String::as_str).collect();
    // Politeness either side.
    if w.first() == Some(&"please") {
        w.remove(0);
    }
    if w.last() == Some(&"please") {
        w.pop();
    }
    matches!(
        w.as_slice(),
        ["snip"]
            | ["snip", "it"]
            | ["snip", "this"]
            | ["take", "a", "snip"]
            | ["screenshot"]
            | ["take", "a", "screenshot"]
            | ["screen", "shot"]
            | ["take", "a", "screen", "shot"]
    )
}

// ---- starting -------------------------------------------------------------------------------

/// The window in front now (a cancel gives the focus back to it).
#[cfg(windows)]
fn foreground() -> isize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize }
}
#[cfg(not(windows))]
fn foreground() -> isize {
    0
}

#[cfg(windows)]
fn give_focus_back(hwnd: isize) {
    // (A test run moves no focus; the robot tests' snips do, to the test's
    // own window it came from.)
    if hwnd == 0 || faking() {
        return;
    }
    use windows::Win32::Foundation::HWND;
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(hwnd as *mut _));
    }
}
#[cfg(not(windows))]
fn give_focus_back(_hwnd: isize) {}

/// Which conversation a snip goes to: the one Chat shows, when the main
/// window is on screen on one; else a new chat (`None`). Window getters:
/// never under a lock.
fn chat_target(app: &AppHandle) -> Option<u64> {
    let (visible, _) = crate::notices::notice::window_view(app);
    match crate::chat::assistant::chat_viewing() {
        crate::chat::assistant::Viewing::Conversation(id) if visible => Some(id),
        _ => None,
    }
}

/// Start a snip (or, with one up, cancel it: the shortcut again). Returns
/// at once; the work runs on its own thread.
pub fn start(app: &AppHandle, origin: Origin) {
    if overlay::is_open() {
        cancel("the shortcut again");
        return;
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return; // already capturing
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("snip".into()).spawn(move || {
        if let Err(e) = run(&app, origin) {
            tracing::warn!("snip: couldn't take it ({e})");
            set_last(Last { outcome: "failed".into(), reason: e.clone(), ..Default::default() });
            crate::notices::alert::snip_failed(&app, &e);
            BUSY.store(false, Ordering::SeqCst);
        }
    });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Cancel the snip under way (the shortcut again, Esc through the hook).
pub fn cancel(why: &'static str) {
    if overlay::is_open() {
        overlay::close_because(why);
    }
}

fn run(app: &AppHandle, origin: Origin) -> Result<(), String> {
    let started = Instant::now();
    // Not over a dictation: one thing at a time. (A spoken "snip" ends its
    // own dictation first.)
    if origin != Origin::Voice && crate::dictation::pipeline::dictation_state().1 {
        tracing::info!("snip: not during a dictation");
        set_last(Last { outcome: "refused".into(), reason: "dictating".into(), ..Default::default() });
        crate::notices::alert::snip_dictating(app);
        BUSY.store(false, Ordering::SeqCst);
        return Ok(());
    }
    // The tray's menu fades out first (it'd be in the shot).
    if origin == Origin::Tray && !faking() {
        std::thread::sleep(std::time::Duration::from_millis(220));
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let foreground = foreground();
    let conversation = chat_target(app);

    // The frozen pictures: fake monitors in a test run, else every real one
    // with Yap's own windows left out for the moment of the capture.
    let (frames, capture_ms) = if faking() {
        let frames = FAKE.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or("no fake monitors set (e2e_snip)")?;
        (frames, 0.0)
    } else {
        let monitors = capture::monitors();
        if monitors.is_empty() {
            return Err("No monitors found".into());
        }
        crate::calls::capture::for_snip(app, true);
        let (results, timings) = capture::capture(&monitors, capture::Force::Auto);
        crate::calls::capture::for_snip(app, false);
        let frames: Vec<Frame> = results.into_iter().filter_map(Result::ok).collect();
        if frames.is_empty() {
            return Err("Windows didn't let Yap capture the screen".into());
        }
        (frames, timings.total_ms)
    };
    let hdr_monitors = frames.iter().filter(|f| f.monitor.hdr).count();
    let methods: Vec<Method> = frames.iter().map(|f| f.method).collect();
    // An HDR monitor only GDI could read: its colours may be washed out.
    if frames.iter().any(|f| f.monitor.hdr && f.method == Method::Gdi) {
        tracing::info!("snip: an HDR monitor was captured with GDI (colours may be washed out)");
    }

    // The overlay: the monitor under the cursor takes the focus.
    let rects: Vec<Rect> = frames.iter().map(|f| Rect::new(f.monitor.x, f.monitor.y, f.monitor.width as i32, f.monitor.height as i32)).collect();
    let focus = overlay::cursor().and_then(|(x, y)| geom::monitor_at(&rects, x, y)).unwrap_or(0);
    let screens: Vec<overlay::Screen> = frames
        .iter()
        .map(|f| overlay::Screen { monitor: f.monitor.clone(), width: f.width, height: f.height, bgra: Arc::new(f.bgra.clone()) })
        .collect();
    let monitors = frames.len();
    *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) =
        Some(Snip { generation, frames, conversation, foreground, started, origin });
    let done_app = app.clone();
    let opts = overlay::Options { focus, interactive: !faking(), hidden: faking() };
    let shown = overlay::open(
        screens,
        opts,
        Box::new(move |outcome| {
            let app = done_app.clone();
            // Off the overlay's thread: saving and showing Chat take a moment.
            std::thread::spawn(move || finish(&app, generation, outcome));
        }),
    );
    let shown = match shown {
        Ok(s) => s,
        Err(e) => {
            CURRENT.lock().unwrap_or_else(|p| p.into_inner()).take();
            return Err(e);
        }
    };
    // Esc cancels through Yap's keyboard hook too (if Windows kept the
    // focus from the overlay).
    crate::dictation::input_hook::snip_escape(overlay::is_open());
    let overlay_ms = started.elapsed().as_secs_f64() * 1000.0;
    tracing::info!(
        monitors,
        hdr = hdr_monitors,
        capture_ms = capture_ms.round() as u64,
        overlay_ms = overlay_ms.round() as u64,
        focused = shown.focused,
        origin = ?origin,
        "snip: the overlay is up"
    );
    let screens_seen: Vec<capture::Monitor> = CURRENT
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|s| s.frames.iter().map(|f| f.monitor.clone()).collect())
        .unwrap_or_default();
    set_last(Last {
        outcome: "selecting".into(),
        monitors,
        hdr_monitors,
        screens: screens_seen,
        methods,
        capture_ms,
        overlay_ms,
        focused: shown.focused,
        conversation_id: conversation,
        ..Default::default()
    });
    Ok(())
}

/// The overlay closed: save the box and hand it to Chat, or give the focus
/// back.
fn finish(app: &AppHandle, generation: u64, outcome: overlay::Outcome) {
    crate::dictation::input_hook::snip_escape(false);
    let snip = {
        let mut current = CURRENT.lock().unwrap_or_else(|p| p.into_inner());
        match current.take() {
            Some(s) if s.generation == generation => Some(s),
            other => {
                *current = other;
                None
            }
        }
    };
    BUSY.store(false, Ordering::SeqCst);
    let Some(snip) = snip else { return };
    let mut last = last().unwrap_or_default();
    match outcome {
        overlay::Outcome::Cancelled(why) => {
            tracing::info!(why, "snip: cancelled");
            give_focus_back(snip.foreground);
            last.outcome = "cancelled".into();
            last.reason = why.into();
            set_last(last);
        }
        overlay::Outcome::Snipped { screen, rect } => {
            let t = Instant::now();
            match save(&snip, screen, &rect) {
                Ok(image) => {
                    last.save_ms = t.elapsed().as_secs_f64() * 1000.0;
                    tracing::info!(
                        w = image.w,
                        h = image.h,
                        kb = image.bytes / 1024,
                        ms = last.save_ms.round() as u64,
                        total_ms = snip.started.elapsed().as_millis() as u64,
                        "snip: kept"
                    );
                    last.outcome = "snipped".into();
                    last.image = Some(image.clone());
                    last.conversation_id = snip.conversation;
                    set_last(last);
                    hand_to_chat(app, image, snip.conversation, snip.origin);
                }
                Err(e) => {
                    tracing::warn!("snip: couldn't save it ({e})");
                    last.outcome = "failed".into();
                    last.reason = e.clone();
                    set_last(last);
                    crate::notices::alert::snip_failed(app, &e);
                    give_focus_back(snip.foreground);
                }
            }
        }
    }
}

/// The box cut out of its monitor's frame and saved as a PNG.
fn save(snip: &Snip, screen: usize, rect: &Rect) -> Result<crate::chat::chats::ChatImage, String> {
    let frame = snip.frames.get(screen).ok_or("that monitor's picture is gone")?;
    let r = geom::to_frame(rect, (frame.monitor.width, frame.monitor.height), (frame.width, frame.height));
    if !geom::big_enough(&r) {
        return Err("the box was too small".into());
    }
    let rgba = geom::crop_rgba(&frame.bgra, frame.width, &r);
    let png = images::encode_png(&rgba, r.w as u32, r.h as u32, true)?;
    let saved = images::save(&png, r.w as u32, r.h as u32)?;
    Ok(crate::chat::chats::ChatImage { id: saved.id, w: saved.w, h: saved.h, bytes: saved.bytes, source: "snip".into(), gone: false })
}

/// The main window up on Chat with the image in its message box, and the
/// copy for the selected model made meanwhile.
fn hand_to_chat(app: &AppHandle, image: crate::chat::chats::ChatImage, conversation: Option<u64>, origin: Origin) {
    let cfg = crate::config::load();
    if let Ok((base_url, _key, _model, provider, _p, _t)) = crate::chat::turn::resolve_chat_endpoint(&cfg) {
        let here = crate::ai::ai_model::runs_here(&provider, &base_url);
        images::prepare(vec![image.id.clone()], images::long_edge(&provider, here));
    }
    let payload = serde_json::json!({ "image": image, "conversationId": conversation, "origin": origin });
    if let Some(main) = app.get_webview_window("settings") {
        let _ = main.emit(EVENT, payload);
    }
    if let Err(e) = crate::shell::window::show_settings(app) {
        tracing::warn!("snip: couldn't bring the main window up ({e})");
    }
}

// ---- the page's way in ----------------------------------------------------------------------

/// Chat's + menu ("Take a screenshot") and the main window's in-page
/// shortcut fallback (`origin`: "chat" | "shortcut" | "bar" | "tray").
#[tauri::command]
pub fn snip_start(app: AppHandle, origin: Option<String>) {
    start(&app, Origin::parse(origin.as_deref()));
}

/// The copies of these screenshots for the Chat model now (the page asks
/// after a model switch, so Send stays instant).
#[tauri::command]
pub fn chat_images_prepare(ids: Vec<String>) {
    let cfg = crate::config::load();
    if let Ok((base_url, _key, _model, provider, _p, _t)) = crate::chat::turn::resolve_chat_endpoint(&cfg) {
        let here = crate::ai::ai_model::runs_here(&provider, &base_url);
        images::prepare(ids.into_iter().filter(|i| images::is_id(i)).collect(), images::long_edge(&provider, here));
    }
}

/// The chat screenshots folder's count and size (Settings → Privacy &
/// data).
#[tauri::command]
pub fn chat_images_usage() -> serde_json::Value {
    let (count, bytes) = images::usage();
    serde_json::json!({ "count": count, "bytes": bytes })
}

/// `yapsnip://localhost/<id>` (`http://yapsnip.localhost/<id>` in WebView2;
/// `/img/<id>` too): a chat screenshot's PNG, by id only — anything else
/// is a 404.
pub fn serve(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let path = request.uri().path().trim_start_matches('/');
    let id = path.strip_prefix("img/").or_else(|| path.strip_prefix("img%2F")).unwrap_or(path);
    let found = images::is_id(id).then(|| images::read(id)).flatten();
    let builder = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*").header("Cache-Control", "no-store");
    match found {
        Some(png) => builder.status(200).header("Content-Type", "image/png").body(png),
        None => builder.status(404).header("Content-Type", "text/plain").body(b"not found".to_vec()),
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// At launch: the capture device made in the background (the first snip
/// then skips ~120 ms), and old screenshots swept now and once a day.
pub fn init(app: &AppHandle) {
    let cfg = crate::config::load();
    if !crate::shell::e2e::active() && !cfg.snip_hotkey.trim().is_empty() {
        std::thread::Builder::new()
            .name("snip-warm".into())
            .spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(8));
                capture::warm_up();
            })
            .ok();
    }
    let _ = app;
    std::thread::Builder::new()
        .name("snip-sweep".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            images::sweep(crate::config::load().chat_image_days);
            std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saying_snip_takes_one() {
        for yes in [
            "snip",
            "Snip.",
            "Snip!",
            "snip it",
            "Snip it.",
            "Snip this",
            "Take a snip.",
            "take a snip",
            "Screenshot.",
            "screenshot",
            "Take a screenshot.",
            "Take a screen shot",
            "Screen shot.",
            "Please snip it.",
            "Take a screenshot, please.",
            "  SNIP  ",
        ] {
            assert!(is_snip_phrase(yes), "{yes:?}");
        }
    }

    #[test]
    fn snip_inside_a_sentence_is_dictated() {
        for no in [
            "",
            "snip the ends off",
            "Snip the ends off the beans.",
            "take a screenshot of the error and send it to Sam",
            "screenshots",
            "a snippet",
            "snippet",
            "snipe",
            "I took a screenshot",
            "can you take a screenshot",
            "snip snip",
            "screenshot it please now",
            "please",
        ] {
            assert!(!is_snip_phrase(no), "{no:?}");
        }
    }

    #[test]
    fn origins_read_from_the_page() {
        assert_eq!(Origin::parse(Some("chat")), Origin::Chat);
        assert_eq!(Origin::parse(Some("bar")), Origin::Bar);
        assert_eq!(Origin::parse(None), Origin::Shortcut);
        assert_eq!(Origin::parse(Some("anything")), Origin::Shortcut);
    }
}
