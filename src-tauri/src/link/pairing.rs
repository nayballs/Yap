//! **Pairing a phone, and who may say hello** (Yap Link). Settings → Phone
//! → Pair a phone makes an invitation ([`Invite`]): a 128-bit token, good
//! for 5 minutes and once, in the QR's `#pair=<token>`. A phone that says
//! hello with it waits for a yes on the PC — "Allow ‘Pixel’ to use Yap? Code
//! 4821" — where the code ([`code`]) is 4 digits from a hash of the token
//! and the phone's nonce: the phone works it out too, so the two screens
//! show the same code. Allowed, the phone gets a device id and a 256-bit
//! secret (`devices.rs` keeps only its hash).
//!
//! Every hello is judged here ([`judge`], pure): a known phone's secret, an
//! invitation, or `denied`. Hellos are rate-limited per address
//! ([`Limiter`]): a few a minute, and an address that keeps failing waits.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// How long an invitation lasts.
pub const INVITE_FOR: Duration = Duration::from_secs(5 * 60);
/// How long a phone waits for the yes on the PC.
pub const ANSWER_FOR: Duration = Duration::from_secs(2 * 60);

/// Random bytes as URL-safe base64 without padding (`n` bytes).
pub fn random_text(n: usize) -> Result<String, String> {
    use base64::Engine;
    let mut raw = vec![0u8; n];
    getrandom::fill(&mut raw).map_err(|e| format!("No random numbers on this PC: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw))
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Equal without saying where they differ (secrets' hashes).
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// **The pairing code both screens show**: SHA-256 of the UTF-8 text
/// `"<token>:<nonce>"`, its first 4 bytes as a big-endian number, modulo
/// 10,000, as 4 digits ("0042"). The phone's page works it out the same
/// way (WebCrypto's SHA-256), so a code that doesn't match says another
/// phone is asking. Pure.
pub fn code(token: &str, nonce: &str) -> String {
    let h = Sha256::digest(format!("{token}:{nonce}").as_bytes());
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 10_000;
    format!("{n:04}")
}

/// A phone's name as Yap shows it: one line, at most 40 characters, "Phone"
/// when it gave none. Pure.
pub fn clean_name(name: &str) -> String {
    let one: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = one.chars().take(40).collect();
    let short = short.trim().to_string();
    if short.is_empty() {
        "Phone".to_string()
    } else {
        short
    }
}

/// Settings' invitation: the QR's token, until it's used or 5 minutes pass.
#[derive(Debug, Clone)]
pub struct Invite {
    pub token: String,
    pub made: Instant,
    pub used: bool,
}

impl Invite {
    pub fn new() -> Result<Invite, String> {
        Ok(Invite { token: random_text(16)?, made: Instant::now(), used: false })
    }

    pub fn live(&self, now: Instant) -> bool {
        !self.used && now.saturating_duration_since(self.made) < INVITE_FOR
    }
}

/// What a hello says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello<'a> {
    pub v: u32,
    pub nonce: &'a str,
    pub device: Option<(&'a str, &'a str)>,
    pub token: Option<&'a str>,
}

/// What becomes of a hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A paired phone: its id.
    Known(String),
    /// The QR's invitation: ask the person, with this code.
    Pair { code: String },
    Denied(&'static str),
}

pub const WRONG_VERSION: &str = "This page is for another version of Yap. Reload it, or pair again in Settings → Phone.";
pub const REMOVED: &str = "This phone was removed in Yap. Pair it again in Settings → Phone.";
pub const BAD_LINK: &str = "This pairing link isn't Yap's latest. Pair again in Settings → Phone on your PC.";
pub const EXPIRED: &str = "This pairing link has expired. Pair again in Settings → Phone on your PC.";
pub const USED: &str = "This pairing link was already used. Pair again in Settings → Phone on your PC.";
pub const NOT_PAIRED: &str = "Pair this phone first: on your PC, open Yap's Settings → Phone → Pair a phone.";
pub const DENIED: &str = "Yap on your PC said no to this phone.";
pub const NO_ANSWER: &str = "Nobody answered on the PC. Pair again in Settings → Phone.";
pub const TOO_MANY: &str = "Too many tries from this phone. Wait a few minutes, then try again.";
pub const OFF: &str = "Yap on your PC isn't taking phones now. Turn on Settings → Phone → Use Yap from your phone.";

/// **Judge a hello** (pure): a paired phone whose secret matches its hash
/// (`known(id)` gives the stored SHA-256 hex) is known; else the QR's token,
/// while the invitation is live, is a pairing (the invitation is used up
/// now: once only, whatever the person answers); else denied, in words the
/// phone can show. A wrong secret and an unknown id read the same.
pub fn judge(hello: &Hello, invite: Option<&mut Invite>, known: &dyn Fn(&str) -> Option<String>, now: Instant) -> Verdict {
    if hello.v != crate::link::wire::VERSION {
        return Verdict::Denied(WRONG_VERSION);
    }
    let device_failed = match hello.device {
        Some((id, secret)) => {
            if !id.is_empty() && !secret.is_empty() && known(id).is_some_and(|hash| same(&hash, &sha256_hex(secret))) {
                return Verdict::Known(id.to_string());
            }
            true
        }
        None => false,
    };
    let Some(token) = hello.token.filter(|t| !t.is_empty()) else {
        return Verdict::Denied(if device_failed { REMOVED } else { NOT_PAIRED });
    };
    let nonce = hello.nonce.trim();
    if nonce.is_empty() || nonce.len() > 128 {
        return Verdict::Denied(BAD_LINK);
    }
    let Some(invite) = invite else {
        return Verdict::Denied(BAD_LINK);
    };
    if !same(&sha256_hex(&invite.token), &sha256_hex(token)) {
        return Verdict::Denied(BAD_LINK);
    }
    if invite.used {
        return Verdict::Denied(USED);
    }
    if !invite.live(now) {
        return Verdict::Denied(EXPIRED);
    }
    invite.used = true;
    Verdict::Pair { code: code(token, nonce) }
}

/// **Hellos per address**: at most [`PER_MINUTE`] in a minute; an address
/// with [`STRIKES`] refused hellos in 10 minutes waits out those 10.
#[derive(Debug, Default)]
pub struct Limiter {
    seen: HashMap<IpAddr, (Vec<Instant>, Vec<Instant>)>,
}

pub const PER_MINUTE: usize = 10;
pub const STRIKES: usize = 5;
const STRIKE_WINDOW: Duration = Duration::from_secs(10 * 60);

impl Limiter {
    /// May `ip` say hello now? Counts this one.
    pub fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        if self.seen.len() > 1_000 {
            self.seen.retain(|_, (tries, strikes)| {
                tries.iter().chain(strikes.iter()).any(|t| now.saturating_duration_since(*t) < STRIKE_WINDOW)
            });
        }
        let (tries, strikes) = self.seen.entry(ip).or_default();
        tries.retain(|t| now.saturating_duration_since(*t) < Duration::from_secs(60));
        strikes.retain(|t| now.saturating_duration_since(*t) < STRIKE_WINDOW);
        if strikes.len() >= STRIKES || tries.len() >= PER_MINUTE {
            return false;
        }
        tries.push(now);
        true
    }

    /// `ip`'s hello was refused.
    pub fn strike(&mut self, ip: IpAddr, now: Instant) {
        self.seen.entry(ip).or_default().1.push(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known_one(secret: &str) -> impl Fn(&str) -> Option<String> {
        let hash = sha256_hex(secret);
        move |id: &str| (id == "dev1").then(|| hash.clone())
    }

    fn hello<'a>(device: Option<(&'a str, &'a str)>, token: Option<&'a str>) -> Hello<'a> {
        Hello { v: 1, nonce: "n0nce", device, token }
    }

    #[test]
    fn a_paired_phone_is_known_by_its_secret_and_only_by_it() {
        let known = known_one("s3cret");
        let now = Instant::now();
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), None), None, &known, now), Verdict::Known("dev1".into()));
        // A wrong secret, an unknown phone, an empty one: removed (the same words).
        assert_eq!(judge(&hello(Some(("dev1", "guess")), None), None, &known, now), Verdict::Denied(REMOVED));
        assert_eq!(judge(&hello(Some(("dev2", "s3cret")), None), None, &known, now), Verdict::Denied(REMOVED));
        assert_eq!(judge(&hello(Some(("dev1", "")), None), None, &known, now), Verdict::Denied(REMOVED));
        // Nothing at all: pair first.
        assert_eq!(judge(&hello(None, None), None, &known, now), Verdict::Denied(NOT_PAIRED));
        // Another protocol version.
        let old = Hello { v: 0, ..hello(Some(("dev1", "s3cret")), None) };
        assert_eq!(judge(&old, None, &known, now), Verdict::Denied(WRONG_VERSION));
    }

    #[test]
    fn an_invitation_pairs_once_within_five_minutes() {
        let known = |_: &str| None;
        let now = Instant::now();
        let mut invite = Invite { token: "tok-A".into(), made: now, used: false };
        // The wrong token: not this invitation, and it's still good after.
        assert_eq!(judge(&hello(None, Some("tok-B")), Some(&mut invite), &known, now), Verdict::Denied(BAD_LINK));
        assert!(!invite.used);
        // No invitation at all (none made, or Yap restarted).
        assert_eq!(judge(&hello(None, Some("tok-A")), None, &known, now), Verdict::Denied(BAD_LINK));
        // The right one: a pairing with its code, and it's used up.
        let first = judge(&hello(None, Some("tok-A")), Some(&mut invite), &known, now);
        assert_eq!(first, Verdict::Pair { code: code("tok-A", "n0nce") });
        assert!(invite.used);
        // Replayed: refused.
        assert_eq!(judge(&hello(None, Some("tok-A")), Some(&mut invite), &known, now), Verdict::Denied(USED));
        // Expired: refused, and not used up.
        let mut old = Invite { token: "tok-C".into(), made: now, used: false };
        let later = now + INVITE_FOR + Duration::from_secs(1);
        assert_eq!(judge(&hello(None, Some("tok-C")), Some(&mut old), &known, later), Verdict::Denied(EXPIRED));
        let mut fresh = Invite { token: "tok-D".into(), made: now, used: false };
        let just = now + INVITE_FOR - Duration::from_secs(1);
        assert!(matches!(judge(&hello(None, Some("tok-D")), Some(&mut fresh), &known, just), Verdict::Pair { .. }));
        // A pairing needs the phone's nonce.
        let mut inv = Invite { token: "tok-E".into(), made: now, used: false };
        let no_nonce = Hello { nonce: " ", ..hello(None, Some("tok-E")) };
        assert_eq!(judge(&no_nonce, Some(&mut inv), &known, now), Verdict::Denied(BAD_LINK));
        // A phone whose stored secret was removed, scanning a new QR, pairs.
        let mut inv = Invite { token: "tok-F".into(), made: now, used: false };
        assert!(matches!(judge(&hello(Some(("gone", "x")), Some("tok-F")), Some(&mut inv), &known, now), Verdict::Pair { .. }));
    }

    #[test]
    fn the_pairing_code_is_four_digits_from_the_token_and_the_nonce() {
        let c = code("tok-A", "n0nce");
        assert_eq!(c.len(), 4);
        assert!(c.chars().all(|ch| ch.is_ascii_digit()));
        assert_eq!(c, code("tok-A", "n0nce"), "the same on both screens");
        assert_ne!(code("tok-A", "n0nce"), code("tok-A", "other"), "another phone's nonce, another code");
        // The formula (the phone's page computes the same): SHA-256 of
        // "<token>:<nonce>", its first 4 bytes big-endian, modulo 10,000.
        let h = Sha256::digest(b"abc:xyz");
        let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 10_000;
        assert_eq!(code("abc", "xyz"), format!("{n:04}"));
    }

    #[test]
    fn tokens_are_long_random_and_url_safe() {
        let a = random_text(16).unwrap();
        let b = random_text(16).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 22, "128 bits");
        assert_eq!(random_text(32).unwrap().len(), 43, "256 bits");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    #[test]
    fn a_phones_name_is_one_short_line() {
        assert_eq!(clean_name("  Sam's\nPixel  9 "), "Sam's Pixel 9");
        assert_eq!(clean_name(""), "Phone");
        assert_eq!(clean_name("\u{7}\t"), "Phone");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), 40);
    }

    #[test]
    fn hellos_are_limited_per_address() {
        let mut l = Limiter::default();
        let a: IpAddr = "192.168.1.9".parse().unwrap();
        let b: IpAddr = "192.168.1.10".parse().unwrap();
        let now = Instant::now();
        for _ in 0..PER_MINUTE {
            assert!(l.allow(a, now));
        }
        assert!(!l.allow(a, now), "too many in a minute");
        assert!(l.allow(b, now), "another address is its own");
        assert!(l.allow(a, now + Duration::from_secs(61)), "a minute later");
        // Refused hellos: the address waits.
        let c: IpAddr = "10.0.0.3".parse().unwrap();
        for _ in 0..STRIKES {
            assert!(l.allow(c, now));
            l.strike(c, now);
        }
        assert!(!l.allow(c, now + Duration::from_secs(120)), "it keeps failing: it waits");
        assert!(l.allow(c, now + Duration::from_secs(601)), "ten minutes on");
    }
}
