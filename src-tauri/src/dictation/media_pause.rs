//! "Pause music and videos while you dictate" (`config.pause_media`, off by
//! default): media that's **playing** — anything that shows in Windows'
//! media controls (Spotify, a YouTube tab, the Media Player…) — is paused
//! once the start chime has played, and played again after the dictation:
//! only what Yap paused, and only if it's still paused then (a song you
//! stopped yourself stays stopped). `mute.rs` decides when; this does it.
//!
//! Over WinRT's `GlobalSystemMediaTransportControlsSessionManager`, with an
//! explicit **Pause** and an explicit **Play** — never the play/pause
//! toggle, which would start anything that happened to be paused. Never a
//! call app's session, and while a meeting records or a browser holds a
//! meeting, no browser's ([`pause_verdict`], pure and unit-tested).
//!
//! **Every answer is waited for a short while only** (2026-11-03): each of
//! the media controls' asynchronous calls at most [`ANSWER_WITHIN`], a
//! dictation's whole pass at most [`PASS_WITHIN`]. A session that doesn't
//! answer (a hung browser tab, a busy music app) is skipped — logged with
//! its app, never its title — and the dictation goes on. The sessions are
//! reached through [`Session`], so the waits are tested with stand-ins.
//!
//! In a test run nothing is touched; the real-app robot touches only media
//! whose title has the run's nonce (`realtest::media_permits`), so a test
//! never pauses the developer's own music.

use std::time::{Duration, Instant};

/// One media session as Windows' media controls show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaFacts {
    /// Its app (`SourceAppUserModelId`): "Spotify.exe", "MSEdge",
    /// "MSTeams_8wekyb3d8bbwe!MSTeams"…
    pub app: String,
    pub playing: bool,
}

/// What must never be paused right now (from `meeting_detect` and the
/// recorder; see `mute::Calls`).
#[derive(Debug, Clone, Default)]
pub(crate) struct Keep {
    /// Every call app's desktop exes and packaged names, lowercased.
    pub call_names: Vec<String>,
    /// No browser's media: a meeting records, or one is live in a browser
    /// (its tabs can't be told apart here).
    pub browsers: bool,
}

/// Browsers by the app id their media sessions carry (lowercased): Chromium
/// browsers name their exe or brand, Firefox a hash of its install path.
const BROWSER_APPS: &[&str] = &[
    "chrome", "msedge", "brave", "opera", "vivaldi", "arc", "firefox", "308046b0af4a39cb", "zen", "floorp",
    "librewolf", "waterfox", "chromium",
];

/// The part of an app id that names the app: "Spotify.exe" → "spotify",
/// "MSTeams_8wekyb3d8bbwe!MSTeams" → "msteams", "MSEdge" → "msedge".
fn app_key(app: &str) -> String {
    let name = app.rsplit(['\\', '/']).next().unwrap_or(app);
    let name = name.split(['_', '!']).next().unwrap_or(name);
    name.trim_end_matches(".exe").trim_end_matches(".EXE").to_ascii_lowercase()
}

/// Whether to pause `m`: it's playing, and it isn't a call app (or, while a
/// meeting is on, a browser).
pub(crate) fn pause_verdict(m: &MediaFacts, keep: &Keep) -> Result<(), &'static str> {
    if !m.playing {
        return Err("not playing");
    }
    let key = app_key(&m.app);
    if key.is_empty() {
        return Err("no app");
    }
    if keep.call_names.iter().any(|c| app_key(c) == key) {
        return Err("a call app");
    }
    if keep.browsers && BROWSER_APPS.contains(&key.as_str()) {
        return Err("a browser, while a meeting is on");
    }
    Ok(())
}

// ---- the waits -------------------------------------------------------------------------

/// How long one answer from a media session is waited for — the media
/// controls themselves, a title, a pause, a play: they answer in
/// milliseconds. One that doesn't (a hung browser tab, a busy music app) is
/// skipped (until 2026-11-03 `.get()` waited for ever, with the other apps'
/// sound and the next dictation behind it).
pub(crate) const ANSWER_WITHIN: Duration = Duration::from_millis(500);
/// …and a dictation's whole start or end: the sessions left after this
/// aren't asked.
pub(crate) const PASS_WITHIN: Duration = Duration::from_millis(1_500);

/// Where a session's playback stands (a property Windows keeps: no wait).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Playback {
    Playing,
    Paused,
    Other,
}

/// An answer waited for: `None` when none came in time.
pub(crate) type Waited<T> = Option<Result<T, String>>;

/// One media session as the pause and the play reach it: Windows' own
/// (`WinSession`), or a test's stand-in. Every wait is bounded by `limit`.
pub(crate) trait Session {
    /// Its app (`SourceAppUserModelId`).
    fn app(&self) -> String;
    fn playback(&self) -> Playback;
    /// Its title, for the robot's guard (never logged).
    fn title(&self, limit: Duration) -> Waited<String>;
    /// An explicit Pause: `true` paused, `false` it wouldn't.
    fn pause(&self, limit: Duration) -> Waited<bool>;
    /// An explicit Play.
    fn play(&self, limit: Duration) -> Waited<bool>;
}

/// The time a pass has left for its next wait: at most [`ANSWER_WITHIN`],
/// none once [`PASS_WITHIN`] is up.
struct Budget(Instant);

impl Budget {
    fn start() -> Self {
        Budget(Instant::now() + PASS_WITHIN)
    }

    fn next(&self) -> Option<Duration> {
        let left = self.0.saturating_duration_since(Instant::now());
        (!left.is_zero()).then(|| left.min(ANSWER_WITHIN))
    }
}

// ---- pause and play ----------------------------------------------------------------------

/// Media Yap paused, for [`resume`].
pub(crate) struct Paused<S = Real> {
    session: S,
    pub app: String,
}

/// Pause what's playing (see [`pause_verdict`]); what was paused, for
/// [`resume`]. `permit(title)`: the robot's guard (always yes in a normal
/// run). A session that doesn't answer in time is left as it is.
pub(crate) fn pause_these<S: Session>(sessions: Vec<S>, keep: &Keep, robot: Option<&dyn Fn(&str) -> bool>) -> Vec<Paused<S>> {
    let budget = Budget::start();
    let mut out = Vec::new();
    let count = sessions.len();
    for (i, session) in sessions.into_iter().enumerate() {
        let app = session.app();
        let playing = session.playback() == Playback::Playing;
        let facts = MediaFacts { app: app.clone(), playing };
        if let Err(why) = pause_verdict(&facts, keep) {
            if playing {
                tracing::debug!(app, why, "other apps: media left playing");
            }
            continue;
        }
        let Some(limit) = budget.next() else {
            tracing::info!(left = count - i, "other apps: out of time; the media sessions left weren't paused");
            break;
        };
        if let Some(permit) = robot {
            let title = match session.title(limit) {
                Some(Ok(t)) => t,
                Some(Err(_)) => String::new(),
                None => {
                    tracing::info!(app, ms = limit.as_millis() as u64, "other apps: a media session didn't answer; left as it is");
                    continue;
                }
            };
            if !permit(&title) {
                tracing::debug!(app, "other apps: media left alone (the test didn't start it)");
                continue;
            }
        }
        let Some(limit) = budget.next() else {
            tracing::info!(left = count - i, "other apps: out of time; the media sessions left weren't paused");
            break;
        };
        match session.pause(limit) {
            Some(Ok(true)) => {
                tracing::info!(app, "other apps: paused media");
                out.push(Paused { session, app });
            }
            Some(Ok(false)) => tracing::info!(app, "other apps: media wouldn't pause"),
            Some(Err(e)) => tracing::info!(app, "other apps: media wouldn't pause ({e})"),
            None => tracing::info!(app, ms = limit.as_millis() as u64, "other apps: a media session didn't answer; left playing"),
        }
    }
    out
}

/// Play again what Yap paused, with an explicit Play: only what's still
/// paused, never waiting past [`PASS_WITHIN`] in all.
pub(crate) fn resume_these<S: Session>(paused: Vec<Paused<S>>) {
    let budget = Budget::start();
    for p in paused {
        let still_paused = p.session.playback() == Playback::Paused;
        if !resume_verdict(still_paused) {
            tracing::info!(app = p.app, "other apps: media left as it is (no longer paused)");
            continue;
        }
        let Some(limit) = budget.next() else {
            tracing::info!(app = p.app, "other apps: out of time; media left paused");
            continue;
        };
        match p.session.play(limit) {
            Some(Ok(true)) => tracing::info!(app = p.app, "other apps: media playing again"),
            Some(Ok(false)) => tracing::info!(app = p.app, "other apps: media wouldn't play again"),
            Some(Err(e)) => tracing::info!(app = p.app, "other apps: media wouldn't play again ({e})"),
            None => tracing::info!(app = p.app, ms = limit.as_millis() as u64, "other apps: a media session didn't answer; left paused"),
        }
    }
}

/// Whether paused media should play again: only if it's still paused (not
/// stopped, closed, or playing again by itself).
pub(crate) fn resume_verdict(still_paused: bool) -> bool {
    still_paused
}

/// Pause what's playing now ([`pause_these`] over Windows' sessions).
pub(crate) fn pause(keep: &Keep, robot: Option<&dyn Fn(&str) -> bool>) -> Vec<Paused> {
    pause_these(sessions(), keep, robot)
}

/// Play again what Yap paused ([`resume_these`]).
pub(crate) fn resume(paused: Vec<Paused>) {
    resume_these(paused);
}

// ---- Windows' sessions -------------------------------------------------------------------

/// The sessions [`pause`] and [`resume`] reach.
#[cfg(windows)]
pub(crate) type Real = WinSession;
#[cfg(not(windows))]
pub(crate) type Real = NoSession;

/// One of Windows' media sessions: each asynchronous call waited for at
/// most its limit (`shell::com::wait_within`).
#[cfg(windows)]
pub(crate) struct WinSession(windows::Media::Control::GlobalSystemMediaTransportControlsSession);

/// `AsyncStatus::Started`: still running.
#[cfg(windows)]
const STARTED: i32 = 0;

/// `op` (a `windows::core::Result` of a WinRT `IAsyncOperation`)'s answer
/// within `limit` ([`Waited`]): its `Status()` looked at until it's past
/// `Started`, then its `GetResults()` (`shell::com::wait_within`). A macro:
/// the operation's type is the `windows_future` crate's, not named here.
#[cfg(windows)]
macro_rules! answer {
    ($op:expr, $limit:expr) => {
        match $op {
            Ok(op) => crate::shell::com::wait_within(
                $limit,
                || op.Status().map_or(true, |s| s.0 != STARTED),
                || op.GetResults().map_err(|e| e.to_string()),
            ),
            Err(e) => Some(Err(e.to_string())),
        }
    };
}

#[cfg(windows)]
impl Session for WinSession {
    fn app(&self) -> String {
        self.0.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default()
    }

    fn playback(&self) -> Playback {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
        match self.0.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()) {
            Ok(s) if s == Status::Playing => Playback::Playing,
            Ok(s) if s == Status::Paused => Playback::Paused,
            _ => Playback::Other,
        }
    }

    fn title(&self, limit: Duration) -> Waited<String> {
        let props: Waited<_> = answer!(self.0.TryGetMediaPropertiesAsync(), limit);
        props.map(|r| r.and_then(|p| p.Title().map(|t| t.to_string()).map_err(|e| e.to_string())))
    }

    fn pause(&self, limit: Duration) -> Waited<bool> {
        answer!(self.0.TryPauseAsync(), limit)
    }

    fn play(&self, limit: Duration) -> Waited<bool> {
        answer!(self.0.TryPlayAsync(), limit)
    }
}

/// The sessions Windows' media controls list now (none when they don't
/// answer in time).
#[cfg(windows)]
fn sessions() -> Vec<WinSession> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
    let manager: Waited<Manager> = answer!(Manager::RequestAsync(), ANSWER_WITHIN);
    match manager {
        Some(Ok(m)) => match m.GetSessions() {
            Ok(list) => list.into_iter().map(WinSession).collect(),
            Err(e) => {
                tracing::info!("other apps: no media sessions ({e})");
                Vec::new()
            }
        },
        Some(Err(e)) => {
            tracing::info!("other apps: no media controls ({e})");
            Vec::new()
        }
        None => {
            tracing::info!(ms = ANSWER_WITHIN.as_millis() as u64, "other apps: the media controls didn't answer; nothing paused");
            Vec::new()
        }
    }
}

/// No media sessions off Windows.
#[cfg(not(windows))]
pub(crate) struct NoSession;

#[cfg(not(windows))]
impl Session for NoSession {
    fn app(&self) -> String {
        String::new()
    }
    fn playback(&self) -> Playback {
        Playback::Other
    }
    fn title(&self, _limit: Duration) -> Waited<String> {
        None
    }
    fn pause(&self, _limit: Duration) -> Waited<bool> {
        None
    }
    fn play(&self, _limit: Duration) -> Waited<bool> {
        None
    }
}

#[cfg(not(windows))]
fn sessions() -> Vec<NoSession> {
    Vec::new()
}

/// The media sessions Windows lists (app, title, "playing" | "paused" | …),
/// for the robot's checks; `permit(title)` filters to the test's own.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn list(permit: &dyn Fn(&str) -> bool) -> Vec<serde_json::Value> {
    let budget = Budget::start();
    sessions()
        .into_iter()
        .filter_map(|session| {
            let title = match session.title(budget.next()?) {
                Some(Ok(t)) => t,
                _ => String::new(),
            };
            if !permit(&title) {
                return None;
            }
            let status = match session.playback() {
                Playback::Playing => "playing",
                Playback::Paused => "paused",
                Playback::Other => "other",
            };
            Some(serde_json::json!({ "app": session.app(), "title": title, "status": status }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(app: &str) -> MediaFacts {
        MediaFacts { app: app.into(), playing: true }
    }

    fn keep(browsers: bool) -> Keep {
        Keep {
            call_names: ["ms-teams.exe", "zoom.exe", "MSTeams", "91750D7E.Slack", "slack.exe", "discord.exe"]
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
            browsers,
        }
    }

    #[test]
    fn only_media_that_is_playing_and_not_a_call() {
        assert_eq!(pause_verdict(&playing("Spotify.exe"), &keep(false)), Ok(()));
        assert_eq!(pause_verdict(&playing("MSEdge"), &keep(false)), Ok(()));
        let paused = MediaFacts { playing: false, ..playing("Spotify.exe") };
        assert_eq!(pause_verdict(&paused, &keep(false)), Err("not playing"));
        // Call apps, desktop or packaged, never.
        assert_eq!(pause_verdict(&playing("Zoom.exe"), &keep(false)), Err("a call app"));
        assert_eq!(pause_verdict(&playing("MSTeams_8wekyb3d8bbwe!MSTeams"), &keep(false)), Err("a call app"));
        assert_eq!(pause_verdict(&playing("91750D7E.Slack_8she8kybcnzg4!Slack"), &keep(false)), Err("a call app"));
        assert_eq!(pause_verdict(&playing(""), &keep(false)), Err("no app"));
    }

    #[test]
    fn no_browser_media_while_a_meeting_is_on() {
        for app in ["Chrome", "MSEdge", "308046B0AF4A39CB", "Brave"] {
            assert_eq!(pause_verdict(&playing(app), &keep(true)), Err("a browser, while a meeting is on"), "{app}");
            assert_eq!(pause_verdict(&playing(app), &keep(false)), Ok(()), "{app}");
        }
        // Music apps carry on being paused during a meeting.
        assert_eq!(pause_verdict(&playing("Spotify.exe"), &keep(true)), Ok(()));
    }

    #[test]
    fn app_ids_come_down_to_the_app() {
        assert_eq!(app_key("Spotify.exe"), "spotify");
        assert_eq!(app_key("MSTeams_8wekyb3d8bbwe!MSTeams"), "msteams");
        assert_eq!(app_key("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"), "vlc");
        assert_eq!(app_key("ms-teams.exe"), "ms-teams");
    }

    #[test]
    fn only_what_is_still_paused_plays_again() {
        assert!(resume_verdict(true));
        assert!(!resume_verdict(false));
    }

    /// A stand-in media session: its answers come after `answer_ms`, or
    /// never (`None`: a hung browser tab), each wait as `wait_within` does it.
    struct Fake {
        app: &'static str,
        answer_ms: Option<u64>,
        state: std::sync::Mutex<Playback>,
    }

    impl Fake {
        fn new(app: &'static str, answer_ms: Option<u64>) -> Self {
            Fake { app, answer_ms, state: std::sync::Mutex::new(Playback::Playing) }
        }

        /// Its answer `then`, done after `answer_ms` (WinRT's `Status()`
        /// past Started), waited for as Windows' sessions are.
        fn wait<T>(&self, limit: Duration, then: T) -> Waited<T> {
            let asked = Instant::now();
            let after = self.answer_ms.map(Duration::from_millis);
            crate::shell::com::wait_within(limit, || after.is_some_and(|a| asked.elapsed() >= a), || Ok(then))
        }
    }

    impl Session for Fake {
        fn app(&self) -> String {
            self.app.into()
        }
        fn playback(&self) -> Playback {
            *self.state.lock().unwrap()
        }
        fn title(&self, limit: Duration) -> Waited<String> {
            self.wait(limit, format!("{} song", self.app))
        }
        fn pause(&self, limit: Duration) -> Waited<bool> {
            let done = self.wait(limit, true);
            if done.is_some() {
                *self.state.lock().unwrap() = Playback::Paused;
            }
            done
        }
        fn play(&self, limit: Duration) -> Waited<bool> {
            self.wait(limit, true)
        }
    }

    /// A media session that never answers is skipped at [`ANSWER_WITHIN`],
    /// the others paused: the pass ends, the dictation goes on. Until
    /// 2026-11-03 the pass waited on it for ever (`.get()`).
    #[test]
    fn a_hung_media_session_is_skipped_and_the_rest_paused() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let sessions = vec![Fake::new("Spotify.exe", Some(5)), Fake::new("MSEdge", None), Fake::new("vlc.exe", Some(5))];
            let started = Instant::now();
            let paused = pause_these(sessions, &keep(false), None);
            let _ = tx.send((paused.iter().map(|p| p.app.clone()).collect::<Vec<_>>(), started.elapsed()));
        });
        let (paused, took) = rx.recv_timeout(Duration::from_secs(5)).expect("the pause pass hung on a media session");
        assert_eq!(paused, ["Spotify.exe", "vlc.exe"]);
        assert!(took >= ANSWER_WITHIN && took < PASS_WITHIN + Duration::from_millis(500), "{took:?}");

        // The robot's look at a hung session's title: left alone too.
        let robot: &dyn Fn(&str) -> bool = &|_| true;
        let paused = pause_these(vec![Fake::new("MSEdge", None), Fake::new("Spotify.exe", Some(5))], &keep(false), Some(robot));
        assert_eq!(paused.iter().map(|p| p.app.as_str()).collect::<Vec<_>>(), ["Spotify.exe"]);
    }

    /// Many hung sessions cost [`PASS_WITHIN`] at most, not half a second each.
    #[test]
    fn a_pass_never_waits_past_its_budget() {
        let sessions: Vec<Fake> = (0..8).map(|_| Fake::new("MSEdge", None)).collect();
        let started = Instant::now();
        assert!(pause_these(sessions, &keep(false), None).is_empty());
        let took = started.elapsed();
        assert!(took < PASS_WITHIN + Duration::from_millis(500), "{took:?}");
    }

    /// Playing again: a session that doesn't answer is left paused, the
    /// rest play; only what's still paused is asked.
    #[test]
    fn resuming_skips_a_session_that_does_not_answer() {
        let paused = pause_these(vec![Fake::new("Spotify.exe", Some(5)), Fake::new("vlc.exe", Some(5))], &keep(false), None);
        assert_eq!(paused.len(), 2);
        let hung = Paused { session: Fake::new("MSEdge", None), app: "MSEdge".into() };
        *hung.session.state.lock().unwrap() = Playback::Paused;
        let mut all = vec![hung];
        all.extend(paused);
        let started = Instant::now();
        resume_these(all);
        assert!(started.elapsed() < PASS_WITHIN + Duration::from_millis(500));
    }
}
