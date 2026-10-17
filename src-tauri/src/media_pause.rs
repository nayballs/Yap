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
//! In a test run nothing is touched; the real-app robot touches only media
//! whose title has the run's nonce (`realtest::media_permits`), so a test
//! never pauses the developer's own music.

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

/// Media Yap paused, for [`resume`].
pub(crate) struct Paused {
    #[cfg(windows)]
    session: windows::Media::Control::GlobalSystemMediaTransportControlsSession,
    pub app: String,
}

/// Pause what's playing (see [`pause_verdict`]); what was paused, for
/// [`resume`]. `permit(title)`: the robot's guard (always yes in a normal
/// run).
#[cfg(windows)]
pub(crate) fn pause(keep: &Keep, robot: Option<&dyn Fn(&str) -> bool>) -> Vec<Paused> {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    let mut out = Vec::new();
    let sessions = match Manager::RequestAsync().and_then(|op| op.get()).and_then(|m| m.GetSessions()) {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("other apps: no media controls ({e})");
            return out;
        }
    };
    for session in sessions {
        let app = session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
        let playing = session
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .is_ok_and(|s| s == Status::Playing);
        let facts = MediaFacts { app: app.clone(), playing };
        if let Err(why) = pause_verdict(&facts, keep) {
            if playing {
                tracing::debug!(app, why, "other apps: media left playing");
            }
            continue;
        }
        if let Some(permit) = robot {
            let title = session
                .TryGetMediaPropertiesAsync()
                .and_then(|op| op.get())
                .and_then(|p| p.Title())
                .map(|t| t.to_string())
                .unwrap_or_default();
            if !permit(&title) {
                tracing::debug!(app, "other apps: media left alone (the test didn't start it)");
                continue;
            }
        }
        match session.TryPauseAsync().and_then(|op| op.get()) {
            Ok(true) => {
                tracing::info!(app, "other apps: paused media");
                out.push(Paused { session, app });
            }
            Ok(false) => tracing::info!(app, "other apps: media wouldn't pause"),
            Err(e) => tracing::info!(app, "other apps: media wouldn't pause ({e})"),
        }
    }
    out
}

#[cfg(not(windows))]
pub(crate) fn pause(_keep: &Keep, _robot: Option<&dyn Fn(&str) -> bool>) -> Vec<Paused> {
    Vec::new()
}

/// Whether paused media should play again: only if it's still paused (not
/// stopped, closed, or playing again by itself).
pub(crate) fn resume_verdict(still_paused: bool) -> bool {
    still_paused
}

/// Play again what Yap paused, with an explicit Play.
#[cfg(windows)]
pub(crate) fn resume(paused: Vec<Paused>) {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;
    for p in paused {
        let still_paused = p
            .session
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .is_ok_and(|s| s == Status::Paused);
        if !resume_verdict(still_paused) {
            tracing::info!(app = p.app, "other apps: media left as it is (no longer paused)");
            continue;
        }
        match p.session.TryPlayAsync().and_then(|op| op.get()) {
            Ok(true) => tracing::info!(app = p.app, "other apps: media playing again"),
            Ok(false) => tracing::info!(app = p.app, "other apps: media wouldn't play again"),
            Err(e) => tracing::info!(app = p.app, "other apps: media wouldn't play again ({e})"),
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn resume(_paused: Vec<Paused>) {}

/// The media sessions Windows lists (app, title, "playing" | "paused" | …),
/// for the robot's checks; `permit(title)` filters to the test's own.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn list(permit: &dyn Fn(&str) -> bool) -> Vec<serde_json::Value> {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    let Ok(sessions) = Manager::RequestAsync().and_then(|op| op.get()).and_then(|m| m.GetSessions()) else {
        return Vec::new();
    };
    sessions
        .into_iter()
        .filter_map(|session| {
            let title = session
                .TryGetMediaPropertiesAsync()
                .and_then(|op| op.get())
                .and_then(|p| p.Title())
                .map(|t| t.to_string())
                .unwrap_or_default();
            if !permit(&title) {
                return None;
            }
            let status = match session.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()) {
                Ok(s) if s == Status::Playing => "playing",
                Ok(s) if s == Status::Paused => "paused",
                Ok(s) if s == Status::Stopped => "stopped",
                Ok(_) => "other",
                Err(_) => "unknown",
            };
            let app = session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
            Some(serde_json::json!({ "app": app, "title": title, "status": status }))
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
}
