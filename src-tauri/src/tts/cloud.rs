//! **Cloud voices** — what any cloud speech service shares (ElevenLabs
//! today, `tts/elevenlabs.rs`; OpenAI's or Azure's could slot in beside it):
//! the providers Settings lists, why a request failed ([`Failure`], said
//! plainly), and **when the local voice steps in** ([`Health`]).
//!
//! The rules (pure, unit-tested on a simulated clock):
//! - A cloud voice that fails mid-answer hands the rest of that answer —
//!   the sentence that failed included — to the local voice (Kokoro, or
//!   Windows voices when those were chosen), if there's one; else that
//!   answer goes unsaid. Never a retry within an answer.
//! - Said **once**: a notice names the reason ("ElevenLabs didn't answer
//!   (quota used up)"); the same reason again within [`ANNOUNCE_EVERY`] goes
//!   unsaid.
//! - After a failure, while a local voice can stand in, the next answers go
//!   straight to it for a while ([`Failure::skip_for`]: 30 s after a rate
//!   limit, 15 minutes when the quota's used up, until the key changes for a
//!   bad key…), so no answer waits on a service that just said no. A
//!   different key, voice or model starts afresh, and so does any request
//!   that works (Settings' voice list or ▶ included).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The local voices' provider id (`config.tts_provider`): Kokoro or
/// Windows voices, on this PC.
pub const LOCAL: &str = "local";

/// The providers Settings lists: `(id, name)`.
pub const PROVIDERS: &[(&str, &str)] = &[(LOCAL, "On this PC"), (super::elevenlabs::ID, super::elevenlabs::NAME)];

/// A known cloud provider's display name ("ElevenLabs").
pub fn name(provider: &str) -> &'static str {
    PROVIDERS.iter().find(|(id, _)| *id == provider).map_or("The voice service", |(_, n)| n)
}

/// Whether `provider` is a cloud voice Yap knows.
pub fn is_cloud(provider: &str) -> bool {
    provider != LOCAL && PROVIDERS.iter().any(|(id, _)| *id == provider)
}

/// The same reason isn't said twice within this.
pub const ANNOUNCE_EVERY: Duration = Duration::from_secs(10 * 60);

/// Why a cloud voice didn't speak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The key is wrong, missing, expired or revoked.
    BadKey,
    /// The key works but may not do this (a key limited to other
    /// endpoints, a voice or model the plan doesn't include).
    NotAllowed,
    /// The month's characters (or credits) are used up.
    Quota,
    /// Too many requests at once, or the service is busy.
    RateLimited,
    /// The service had a problem of its own (5xx).
    ServiceDown,
    /// No connection (no network, DNS, refused, TLS).
    Offline,
    /// Asked, but no audio in time.
    TimedOut,
    /// The chosen voice is gone from the account.
    VoiceGone,
    /// The chosen model isn't offered.
    ModelGone,
    /// The request was refused as it was (the service's own words, short).
    Rejected(String),
}

impl Failure {
    /// The reason in a few words, for "didn't answer (…)".
    pub fn reason(&self) -> String {
        match self {
            Failure::BadKey => "API key not accepted".into(),
            Failure::NotAllowed => "the key isn't allowed to do this".into(),
            Failure::Quota => "quota used up".into(),
            Failure::RateLimited => "too many requests".into(),
            Failure::ServiceDown => "the service is having problems".into(),
            Failure::Offline => "no connection".into(),
            Failure::TimedOut => "timed out".into(),
            Failure::VoiceGone => "voice not found".into(),
            Failure::ModelGone => "model not available".into(),
            Failure::Rejected(why) if why.is_empty() => "request refused".into(),
            Failure::Rejected(why) => why.clone(),
        }
    }

    /// What the person can do about it, if anything.
    pub fn advice(&self, provider: &str) -> String {
        let n = name(provider);
        match self {
            Failure::BadKey => format!("Check your {n} API key in Settings → Voice Assistant."),
            Failure::NotAllowed => format!("Give the key text-to-speech access in {n}, or pick another voice or model."),
            Failure::Quota => format!("Your {n} plan has no characters left this month."),
            Failure::VoiceGone => "Pick another voice in Settings → Voice Assistant.".into(),
            Failure::ModelGone => "Pick another model in Settings → Voice Assistant.".into(),
            _ => String::new(),
        }
    }

    /// How long the next answers go straight to the local voice after this.
    pub fn skip_for(&self) -> Duration {
        match self {
            Failure::RateLimited => Duration::from_secs(30),
            Failure::ServiceDown | Failure::Offline | Failure::TimedOut => Duration::from_secs(60),
            Failure::Rejected(_) => Duration::from_secs(5 * 60),
            Failure::Quota => Duration::from_secs(15 * 60),
            Failure::BadKey | Failure::NotAllowed | Failure::VoiceGone | Failure::ModelGone => {
                Duration::from_secs(30 * 60)
            }
        }
    }

    /// Two failures are "the same reason" for [`ANNOUNCE_EVERY`].
    fn same_kind(&self, other: &Failure) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// A fingerprint of a secret or a choice, to tell "the same key / voice /
/// model" from another without keeping the key itself. In memory only.
pub fn fingerprint(parts: &[&str]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
    }
    h.finish()
}

#[derive(Debug, Clone)]
struct Down {
    provider: String,
    /// The key's fingerprint, and the key + voice + model's.
    key: u64,
    choice: u64,
    failure: Failure,
    at: Instant,
    /// When this reason was last said.
    announced: Option<Instant>,
}

/// The last failure of a cloud voice, and whether it was said. Pure: every
/// method takes the time.
#[derive(Debug, Default)]
pub struct Health {
    down: Option<Down>,
}

impl Health {
    /// A request with `key` + `choice` failed.
    pub fn failed(&mut self, provider: &str, key: u64, choice: u64, failure: Failure, now: Instant) {
        // The same reason again keeps when it was said.
        let announced = self
            .down
            .as_ref()
            .filter(|d| d.provider == provider && d.key == key && d.failure.same_kind(&failure))
            .and_then(|d| d.announced);
        self.down = Some(Down { provider: provider.to_string(), key, choice, failure, at: now, announced });
    }

    /// A request with `key` worked: whatever went wrong with it is over.
    pub fn worked(&mut self, provider: &str, key: u64) {
        if self.down.as_ref().is_some_and(|d| d.provider == provider && d.key == key) {
            self.down = None;
        }
    }

    /// The failure to say now, if it should be said (not the same reason
    /// within [`ANNOUNCE_EVERY`]); marks it said.
    pub fn announce(&mut self, provider: &str, now: Instant) -> Option<Failure> {
        let d = self.down.as_mut().filter(|d| d.provider == provider)?;
        if d.announced.is_some_and(|t| now.saturating_duration_since(t) < ANNOUNCE_EVERY) {
            return None;
        }
        d.announced = Some(now);
        Some(d.failure.clone())
    }

    /// Whether the next answer should skip `provider` (with this key and
    /// choice) and go to the local voice: it failed under
    /// [`Failure::skip_for`] ago.
    pub fn skipping(&self, provider: &str, key: u64, choice: u64, now: Instant) -> bool {
        self.down.as_ref().is_some_and(|d| {
            d.provider == provider
                && d.key == key
                && d.choice == choice
                && now.saturating_duration_since(d.at) < d.failure.skip_for()
        })
    }

    /// The failure on record for `provider`, if any.
    pub fn last(&self, provider: &str) -> Option<Failure> {
        self.down.as_ref().filter(|d| d.provider == provider).map(|d| d.failure.clone())
    }
}

static HEALTH: Mutex<Option<Health>> = Mutex::new(None);

/// The shared [`Health`].
pub fn with_health<T>(f: impl FnOnce(&mut Health) -> T) -> T {
    let mut h = HEALTH.lock().unwrap_or_else(|p| p.into_inner());
    f(h.get_or_insert_with(Health::default))
}

/// Forget every failure (a test run between tests).
#[cfg(debug_assertions)]
pub fn reset() {
    with_health(|h| *h = Health::default());
}

/// The notice for a failure: (title, body). `took_over`: the local voice is
/// reading the answer instead. Pure.
pub fn notice_text(provider: &str, failure: &Failure, took_over: bool) -> (String, String) {
    let title = format!("{} didn't answer ({})", name(provider), failure.reason());
    let then = if took_over {
        "Speaking with Yap's own voice.".to_string()
    } else {
        "Yap's own voice isn't downloaded, so this answer wasn't read aloud.".to_string()
    };
    let advice = failure.advice(provider);
    let body = if advice.is_empty() { then } else { format!("{then} {advice}") };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn a_failure_is_said_once_then_again_after_ten_minutes() {
        let t0 = Instant::now();
        let mut h = Health::default();
        h.failed("elevenlabs", 1, 11, Failure::Quota, t0);
        assert_eq!(h.announce("elevenlabs", t0), Some(Failure::Quota));
        // The next answers fail the same way: not said again.
        h.failed("elevenlabs", 1, 11, Failure::Quota, at(t0, 60));
        assert_eq!(h.announce("elevenlabs", at(t0, 60)), None);
        h.failed("elevenlabs", 1, 11, Failure::Quota, at(t0, 9 * 60));
        assert_eq!(h.announce("elevenlabs", at(t0, 9 * 60)), None);
        // Ten minutes after it was said: said again.
        h.failed("elevenlabs", 1, 11, Failure::Quota, at(t0, 11 * 60));
        assert_eq!(h.announce("elevenlabs", at(t0, 11 * 60)), Some(Failure::Quota));
        // Another reason is news.
        h.failed("elevenlabs", 1, 11, Failure::RateLimited, at(t0, 12 * 60));
        assert_eq!(h.announce("elevenlabs", at(t0, 12 * 60)), Some(Failure::RateLimited));
        // Nothing on record for another provider.
        assert_eq!(h.announce("openai", at(t0, 12 * 60)), None);
    }

    #[test]
    fn a_new_key_or_a_working_request_starts_afresh() {
        let t0 = Instant::now();
        let mut h = Health::default();
        h.failed("elevenlabs", 1, 11, Failure::BadKey, t0);
        assert!(h.announce("elevenlabs", t0).is_some());
        assert!(h.skipping("elevenlabs", 1, 11, at(t0, 60)));
        // Another key, or the same key with another voice: tried again.
        assert!(!h.skipping("elevenlabs", 2, 22, at(t0, 60)));
        assert!(!h.skipping("elevenlabs", 1, 12, at(t0, 60)));
        // The new key fails too: said (it's a different key).
        h.failed("elevenlabs", 2, 22, Failure::BadKey, at(t0, 61));
        assert!(h.announce("elevenlabs", at(t0, 61)).is_some());
        // A request with it that works clears it; one with another key doesn't.
        h.worked("elevenlabs", 1);
        assert!(h.last("elevenlabs").is_some());
        h.worked("elevenlabs", 2);
        assert_eq!(h.last("elevenlabs"), None);
        assert!(!h.skipping("elevenlabs", 2, 22, at(t0, 62)));
    }

    #[test]
    fn how_long_answers_skip_the_cloud() {
        let t0 = Instant::now();
        let cases = [
            (Failure::RateLimited, 30),
            (Failure::Offline, 60),
            (Failure::TimedOut, 60),
            (Failure::ServiceDown, 60),
            (Failure::Quota, 15 * 60),
            (Failure::BadKey, 30 * 60),
            (Failure::VoiceGone, 30 * 60),
        ];
        for (failure, secs) in cases {
            let mut h = Health::default();
            h.failed("elevenlabs", 1, 11, failure.clone(), t0);
            assert!(h.skipping("elevenlabs", 1, 11, at(t0, secs - 1)), "{failure:?}");
            assert!(!h.skipping("elevenlabs", 1, 11, at(t0, secs)), "{failure:?}");
        }
    }

    #[test]
    fn the_notice_says_why_and_what_happens_next() {
        let (title, body) = notice_text("elevenlabs", &Failure::Quota, true);
        assert_eq!(title, "ElevenLabs didn't answer (quota used up)");
        assert_eq!(body, "Speaking with Yap's own voice. Your ElevenLabs plan has no characters left this month.");
        let (title, body) = notice_text("elevenlabs", &Failure::BadKey, false);
        assert_eq!(title, "ElevenLabs didn't answer (API key not accepted)");
        assert!(body.starts_with("Yap's own voice isn't downloaded, so this answer wasn't read aloud."), "{body}");
        assert!(body.ends_with("Check your ElevenLabs API key in Settings → Voice Assistant."), "{body}");
        let (_, body) = notice_text("elevenlabs", &Failure::Offline, true);
        assert_eq!(body, "Speaking with Yap's own voice.");
    }

    #[test]
    fn providers() {
        assert!(is_cloud("elevenlabs"));
        assert!(!is_cloud(LOCAL));
        assert!(!is_cloud("nonsense"));
        assert_eq!(name("elevenlabs"), "ElevenLabs");
        assert_ne!(fingerprint(&["a", "b"]), fingerprint(&["ab", ""]));
    }
}
