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
//! invitation, or `denied` ([`Refusal`]: what the phone is told, and whether
//! it was a guess). Hellos are rate-limited per address ([`Limiter`]): a few
//! a minute, and an address whose guesses keep failing waits — told how long
//! ("Try again in 4 minutes.").
//!
//! **A hello with both** (2026-11-03): the page sends its device and the
//! QR's token when it has both — a tab restored with a stale `#pair=` in its
//! address, or the code scanned again on a phone already paired. The device
//! is judged first: known, it's connected (and when the token is the live
//! invitation's, that's used up: Settings' code has done its job,
//! [`Verdict::Known`]'s `invited`); not known, the token pairs as before.
//! Only guesses are strikes ([`Refusal::strikes`]): a wrong secret, a token
//! that isn't Yap's — not one from the real QR (used, expired). A new
//! pairing link forgives every address's ([`Limiter::forgive`]), and a
//! paired phone's hello or the live code is never turned away for them
//! (`session.rs`: only a hello that's refused anyway waits).

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
    /// A paired phone: its id. `invited`: it carried the live invitation's
    /// token too (the QR scanned again on a phone already paired): the
    /// invitation is used up, and Settings' code is told it's done.
    Known { id: String, invited: bool },
    /// The QR's invitation: ask the person, with this code.
    Pair { code: String },
    Denied(Refusal),
}

pub const WRONG_VERSION: &str = "This page is for another version of Yap. Reload it, or pair again in Settings → Phone.";
pub const REMOVED: &str = "This phone was removed in Yap. Pair it again in Settings → Phone.";
pub const BAD_LINK: &str = "This pairing link isn't Yap's latest. Pair again in Settings → Phone on your PC.";
pub const EXPIRED: &str = "This pairing link has expired. Pair again in Settings → Phone on your PC.";
pub const USED: &str = "This pairing link was already used. Pair again in Settings → Phone on your PC.";
pub const NOT_PAIRED: &str = "Pair this phone first: on your PC, open Yap's Settings → Phone → Pair a phone.";
pub const DENIED: &str = "Yap on your PC said no to this phone.";
pub const NO_ANSWER: &str = "Nobody answered on the PC. Pair again in Settings → Phone.";
/// [`Refusal::TooMany`]'s words begin so; when to try again follows.
pub const TOO_MANY: &str = "Too many tries from this phone.";
pub const OFF: &str = "Yap on your PC isn't taking phones now. Turn on Settings → Phone → Use Yap from your phone.";

/// **Why a hello was refused**: the words the phone shows
/// ([`words`](Refusal::words)), whether it was a guess
/// ([`strikes`](Refusal::strikes)), and its kind for the log
/// ([`kind`](Refusal::kind): never an id, a secret, a token or an address).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Another protocol version (an old page).
    WrongVersion,
    /// A phone Yap doesn't know, or the wrong secret (the same words).
    Removed,
    /// Neither a phone's credentials nor a token.
    NotPaired,
    /// A token that isn't the invitation's (an older code, none made since
    /// Yap started, or no nonce with it).
    BadLink,
    /// The invitation's own token, after its 5 minutes.
    Expired,
    /// The invitation's own token, used already.
    Used,
    /// The address waits ([`Limiter`]): this long.
    TooMany(Duration),
}

impl Refusal {
    /// What the phone shows.
    pub fn words(&self) -> String {
        match self {
            Refusal::WrongVersion => WRONG_VERSION.into(),
            Refusal::Removed => REMOVED.into(),
            Refusal::NotPaired => NOT_PAIRED.into(),
            Refusal::BadLink => BAD_LINK.into(),
            Refusal::Expired => EXPIRED.into(),
            Refusal::Used => USED.into(),
            Refusal::TooMany(wait) => too_many(*wait),
        }
    }

    /// **A guess**: counts against the address ([`Limiter::strike`]) — a
    /// wrong secret, a token that isn't Yap's. A refusal whose token was the
    /// real invitation's (used, expired) came from the QR on the PC's
    /// screen, and an old page or a hello with nothing to check guessed
    /// nothing: those don't.
    pub fn strikes(&self) -> bool {
        matches!(self, Refusal::Removed | Refusal::BadLink)
    }

    /// Its kind, for the log.
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::WrongVersion => "wrong version",
            Refusal::Removed => "removed",
            Refusal::NotPaired => "not paired",
            Refusal::BadLink => "bad link",
            Refusal::Expired => "expired",
            Refusal::Used => "used",
            Refusal::TooMany(_) => "too many",
        }
    }
}

/// "Too many tries from this phone. Try again in 4 minutes." — the wait in
/// whole minutes, rounded up ("in a minute" for one or less). Pure.
pub fn too_many(wait: Duration) -> String {
    let minutes = wait.as_secs().div_ceil(60);
    if minutes <= 1 {
        format!("{TOO_MANY} Try again in a minute.")
    } else {
        format!("{TOO_MANY} Try again in {minutes} minutes.")
    }
}

/// **Judge a hello** (pure): a paired phone whose secret matches its hash
/// (`known(id)` gives the stored SHA-256 hex) is known, whatever token it
/// brings too (the live invitation's is used up then: `invited`); else the
/// QR's token, while the invitation is live, is a pairing (the invitation is
/// used up now: once only, whatever the person answers); else denied, why
/// in a [`Refusal`]. A wrong secret and an unknown id read the same.
pub fn judge(hello: &Hello, invite: Option<&mut Invite>, known: &dyn Fn(&str) -> Option<String>, now: Instant) -> Verdict {
    if hello.v != crate::link::wire::VERSION {
        return Verdict::Denied(Refusal::WrongVersion);
    }
    let token = hello.token.filter(|t| !t.is_empty());
    // The invitation's own token (their hashes compared in constant time).
    let is_its = |invite: &Invite, token: &str| same(&sha256_hex(&invite.token), &sha256_hex(token));
    let device_failed = match hello.device {
        Some((id, secret)) => {
            if !id.is_empty() && !secret.is_empty() && known(id).is_some_and(|hash| same(&hash, &sha256_hex(secret))) {
                // Paired already: the live code it brought is spent here, so
                // Settings stops waiting for a phone to pair with it.
                let invited = match (invite, token) {
                    (Some(invite), Some(token)) if invite.live(now) && is_its(invite, token) => {
                        invite.used = true;
                        true
                    }
                    _ => false,
                };
                return Verdict::Known { id: id.to_string(), invited };
            }
            true
        }
        None => false,
    };
    let Some(token) = token else {
        return Verdict::Denied(if device_failed { Refusal::Removed } else { Refusal::NotPaired });
    };
    let nonce = hello.nonce.trim();
    if nonce.is_empty() || nonce.len() > 128 {
        return Verdict::Denied(Refusal::BadLink);
    }
    let Some(invite) = invite else {
        return Verdict::Denied(Refusal::BadLink);
    };
    if !is_its(invite, token) {
        return Verdict::Denied(Refusal::BadLink);
    }
    if invite.used {
        return Verdict::Denied(Refusal::Used);
    }
    if !invite.live(now) {
        return Verdict::Denied(Refusal::Expired);
    }
    invite.used = true;
    Verdict::Pair { code: code(token, nonce) }
}

/// **Hellos per address**: at most [`PER_MINUTE`] in a minute
/// ([`admit`](Limiter::admit)); an address with [`STRIKES`] guesses refused
/// in 10 minutes waits until the oldest of them that counts is 10 minutes
/// old ([`struck_out`](Limiter::struck_out)) — for a hello that's refused
/// anyway: a paired phone's and the live code's go on (`session.rs`). A new
/// pairing link forgives them all ([`forgive`](Limiter::forgive)).
#[derive(Debug, Default)]
pub struct Limiter {
    seen: HashMap<IpAddr, (Vec<Instant>, Vec<Instant>)>,
}

/// Hellos a minute from one address: a bound on how fast anything can be
/// tried (each hello is judged), with room for a phone whose tabs reconnect
/// together, a hello each (10 until 3 Nov: seven restored tabs and a scan
/// came close).
pub const PER_MINUTE: usize = 20;
pub const STRIKES: usize = 5;
pub const STRIKE_WINDOW: Duration = Duration::from_secs(10 * 60);
const MINUTE: Duration = Duration::from_secs(60);

/// How long until fewer than `n` of `times` (oldest first, at least `n`)
/// are within `window` of now: until the `n`-th newest is that old. At
/// least a second. Pure.
fn wait_for(times: &[Instant], n: usize, window: Duration, now: Instant) -> Duration {
    let counts_from = times[times.len() - n];
    (counts_from + window).saturating_duration_since(now).max(Duration::from_secs(1))
}

impl Limiter {
    /// The address's hellos (this minute's) and strikes (these 10 minutes').
    fn entry(&mut self, ip: IpAddr, now: Instant) -> &mut (Vec<Instant>, Vec<Instant>) {
        if self.seen.len() > 1_000 {
            self.seen.retain(|_, (tries, strikes)| {
                tries.iter().chain(strikes.iter()).any(|t| now.saturating_duration_since(*t) < STRIKE_WINDOW)
            });
        }
        let e = self.seen.entry(ip).or_default();
        e.0.retain(|t| now.saturating_duration_since(*t) < MINUTE);
        e.1.retain(|t| now.saturating_duration_since(*t) < STRIKE_WINDOW);
        e
    }

    /// **May `ip` say hello now?** At most [`PER_MINUTE`] a minute, this one
    /// counted; `Err`: how long until it may.
    pub fn admit(&mut self, ip: IpAddr, now: Instant) -> Result<(), Duration> {
        let (tries, _) = self.entry(ip, now);
        if tries.len() >= PER_MINUTE {
            return Err(wait_for(tries, PER_MINUTE, MINUTE, now));
        }
        tries.push(now);
        Ok(())
    }

    /// **Is `ip` waiting out its strikes?** How long until it may try again.
    pub fn struck_out(&mut self, ip: IpAddr, now: Instant) -> Option<Duration> {
        let (_, strikes) = self.entry(ip, now);
        (strikes.len() >= STRIKES).then(|| wait_for(strikes, STRIKES, STRIKE_WINDOW, now))
    }

    /// `ip`'s hello was refused, and it was a guess ([`Refusal::strikes`]).
    pub fn strike(&mut self, ip: IpAddr, now: Instant) {
        self.entry(ip, now).1.push(now);
    }

    /// **A new pairing link** (Settings → Phone): the person at the PC is
    /// pairing on purpose, so every address's strikes are forgiven (the
    /// minute's cap stays). How many there were.
    pub fn forgive(&mut self) -> usize {
        self.seen.values_mut().map(|(_, strikes)| std::mem::take(strikes).len()).sum()
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
        let dev1 = Verdict::Known { id: "dev1".into(), invited: false };
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), None), None, &known, now), dev1);
        // A wrong secret, an unknown phone, an empty one: removed (the same words).
        assert_eq!(judge(&hello(Some(("dev1", "guess")), None), None, &known, now), Verdict::Denied(Refusal::Removed));
        assert_eq!(judge(&hello(Some(("dev2", "s3cret")), None), None, &known, now), Verdict::Denied(Refusal::Removed));
        assert_eq!(judge(&hello(Some(("dev1", "")), None), None, &known, now), Verdict::Denied(Refusal::Removed));
        // Nothing at all: pair first.
        assert_eq!(judge(&hello(None, None), None, &known, now), Verdict::Denied(Refusal::NotPaired));
        // Another protocol version.
        let old = Hello { v: 0, ..hello(Some(("dev1", "s3cret")), None) };
        assert_eq!(judge(&old, None, &known, now), Verdict::Denied(Refusal::WrongVersion));
    }

    #[test]
    fn a_paired_phone_with_a_stale_code_too_is_known_and_a_live_one_is_spent() {
        let known = known_one("s3cret");
        let now = Instant::now();
        let dev1 = |invited| Verdict::Known { id: "dev1".into(), invited };
        // A tab restored with an older code in its address: the device decides.
        let mut invite = Invite { token: "tok-NEW".into(), made: now, used: false };
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), Some("tok-OLD")), Some(&mut invite), &known, now), dev1(false));
        assert!(!invite.used, "another code: the invitation still waits for its phone");
        // The code it paired with, restored after it was used: known.
        let mut used = Invite { token: "tok-USED".into(), made: now, used: true };
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), Some("tok-USED")), Some(&mut used), &known, now), dev1(false));
        // One gone by, and none made since Yap started: known.
        let mut lapsed = Invite { token: "tok-LAPSED".into(), made: now, used: false };
        let later = now + INVITE_FOR + Duration::from_secs(1);
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), Some("tok-LAPSED")), Some(&mut lapsed), &known, later), dev1(false));
        assert!(!lapsed.used);
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), Some("tok-OLD")), None, &known, now), dev1(false));
        // The live code scanned again on a phone already paired: known, and
        // the invitation spent (Settings' code has done its job).
        assert_eq!(judge(&hello(Some(("dev1", "s3cret")), Some("tok-NEW")), Some(&mut invite), &known, now), dev1(true));
        assert!(invite.used);
        // …so it pairs no other phone after.
        assert_eq!(judge(&hello(None, Some("tok-NEW")), Some(&mut invite), &known, now), Verdict::Denied(Refusal::Used));
        // A phone Yap no longer knows, with the live code: it pairs afresh.
        let mut fresh = Invite { token: "tok-F".into(), made: now, used: false };
        assert!(matches!(judge(&hello(Some(("dev1", "wrong")), Some("tok-F")), Some(&mut fresh), &known, now), Verdict::Pair { .. }));
        // …with a stale one: the link's words, not "removed".
        let mut other = Invite { token: "tok-G".into(), made: now, used: false };
        assert_eq!(judge(&hello(Some(("gone", "x")), Some("tok-OLD")), Some(&mut other), &known, now), Verdict::Denied(Refusal::BadLink));
    }

    #[test]
    fn only_guesses_are_strikes_and_every_refusal_says_why() {
        // A wrong secret, a token that isn't Yap's: guesses.
        assert!(Refusal::Removed.strikes() && Refusal::BadLink.strikes());
        // From the real QR (used, expired), an old page, nothing to check,
        // or already waiting: not guesses.
        for r in [Refusal::Used, Refusal::Expired, Refusal::WrongVersion, Refusal::NotPaired, Refusal::TooMany(Duration::from_secs(60))] {
            assert!(!r.strikes(), "{r:?}");
        }
        assert_eq!(Refusal::Removed.words(), REMOVED);
        assert_eq!(Refusal::BadLink.words(), BAD_LINK);
        assert_eq!(Refusal::Used.words(), USED);
        assert_eq!(Refusal::Expired.words(), EXPIRED);
        assert_eq!(Refusal::NotPaired.words(), NOT_PAIRED);
        assert_eq!(Refusal::WrongVersion.words(), WRONG_VERSION);
        // Their kinds, for the log: words, never anything of the hello's.
        let kinds: Vec<&str> = [
            Refusal::Removed,
            Refusal::NotPaired,
            Refusal::Used,
            Refusal::Expired,
            Refusal::BadLink,
            Refusal::WrongVersion,
            Refusal::TooMany(Duration::ZERO),
        ]
        .iter()
        .map(Refusal::kind)
        .collect();
        assert_eq!(kinds, ["removed", "not paired", "used", "expired", "bad link", "wrong version", "too many"]);
    }

    #[test]
    fn too_many_says_when_to_try_again() {
        let words = |s| Refusal::TooMany(Duration::from_secs(s)).words();
        assert_eq!(words(4 * 60), "Too many tries from this phone. Try again in 4 minutes.");
        assert_eq!(words(3 * 60 + 1), "Too many tries from this phone. Try again in 4 minutes.", "rounded up");
        assert_eq!(words(10 * 60), "Too many tries from this phone. Try again in 10 minutes.");
        assert_eq!(words(61), "Too many tries from this phone. Try again in 2 minutes.");
        assert_eq!(words(60), "Too many tries from this phone. Try again in a minute.");
        assert_eq!(words(5), "Too many tries from this phone. Try again in a minute.");
        assert_eq!(words(0), "Too many tries from this phone. Try again in a minute.");
    }

    #[test]
    fn an_invitation_pairs_once_within_five_minutes() {
        let known = |_: &str| None;
        let now = Instant::now();
        let mut invite = Invite { token: "tok-A".into(), made: now, used: false };
        // The wrong token: not this invitation, and it's still good after.
        assert_eq!(judge(&hello(None, Some("tok-B")), Some(&mut invite), &known, now), Verdict::Denied(Refusal::BadLink));
        assert!(!invite.used);
        // No invitation at all (none made, or Yap restarted).
        assert_eq!(judge(&hello(None, Some("tok-A")), None, &known, now), Verdict::Denied(Refusal::BadLink));
        // The right one: a pairing with its code, and it's used up.
        let first = judge(&hello(None, Some("tok-A")), Some(&mut invite), &known, now);
        assert_eq!(first, Verdict::Pair { code: code("tok-A", "n0nce") });
        assert!(invite.used);
        // Replayed: refused.
        assert_eq!(judge(&hello(None, Some("tok-A")), Some(&mut invite), &known, now), Verdict::Denied(Refusal::Used));
        // Expired: refused, and not used up.
        let mut old = Invite { token: "tok-C".into(), made: now, used: false };
        let later = now + INVITE_FOR + Duration::from_secs(1);
        assert_eq!(judge(&hello(None, Some("tok-C")), Some(&mut old), &known, later), Verdict::Denied(Refusal::Expired));
        let mut fresh = Invite { token: "tok-D".into(), made: now, used: false };
        let just = now + INVITE_FOR - Duration::from_secs(1);
        assert!(matches!(judge(&hello(None, Some("tok-D")), Some(&mut fresh), &known, just), Verdict::Pair { .. }));
        // A pairing needs the phone's nonce.
        let mut inv = Invite { token: "tok-E".into(), made: now, used: false };
        let no_nonce = Hello { nonce: " ", ..hello(None, Some("tok-E")) };
        assert_eq!(judge(&no_nonce, Some(&mut inv), &known, now), Verdict::Denied(Refusal::BadLink));
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
        // Pinned, so a phone's page and a paired phone's stored hash keep
        // matching whatever sha2 Yap builds on (Python's hashlib gives these).
        assert_eq!(n, 1075);
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
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
        for i in 0..PER_MINUTE {
            assert_eq!(l.admit(a, now + Duration::from_secs(i as u64)), Ok(()));
        }
        // Too many in a minute: until the first of them is a minute old.
        assert_eq!(l.admit(a, now + Duration::from_secs(20)), Err(Duration::from_secs(40)));
        assert_eq!(l.admit(b, now), Ok(()), "another address is its own");
        assert_eq!(l.admit(a, now + Duration::from_secs(61)), Ok(()), "a minute later");
        // Refused guesses: the address waits.
        let c: IpAddr = "10.0.0.3".parse().unwrap();
        for i in 0..STRIKES {
            let at = now + Duration::from_secs(i as u64 * 30);
            assert_eq!(l.struck_out(c, at), None);
            assert_eq!(l.admit(c, at), Ok(()));
            l.strike(c, at);
        }
        // Waiting is for refusals; the minute's hellos still count apart.
        assert_eq!(l.admit(c, now + Duration::from_secs(150)), Ok(()));
        // Until the first strike is 10 minutes old.
        assert_eq!(l.struck_out(c, now + Duration::from_secs(120)), Some(STRIKE_WINDOW - Duration::from_secs(120)));
        assert_eq!(l.struck_out(c, now + Duration::from_secs(599)), Some(Duration::from_secs(1)));
        assert_eq!(l.struck_out(c, now + Duration::from_secs(601)), None, "ten minutes on");
        // More strikes than five: until the fifth newest is that old.
        let d: IpAddr = "10.0.0.4".parse().unwrap();
        for i in 0..7u64 {
            l.strike(d, now + Duration::from_secs(i * 60));
        }
        assert_eq!(l.struck_out(d, now + Duration::from_secs(400)), Some(Duration::from_secs(320)));
    }

    #[test]
    fn a_new_pairing_link_forgives_every_addresss_strikes() {
        let mut l = Limiter::default();
        let now = Instant::now();
        let phone: IpAddr = "192.168.1.9".parse().unwrap();
        let other: IpAddr = "192.168.1.30".parse().unwrap();
        for _ in 0..STRIKES {
            l.strike(phone, now);
        }
        l.strike(other, now);
        assert!(l.struck_out(phone, now).is_some());
        assert_eq!(l.forgive(), STRIKES + 1);
        assert_eq!(l.struck_out(phone, now), None);
        // It starts again from nothing.
        for _ in 0..STRIKES - 1 {
            l.strike(phone, now);
        }
        assert_eq!(l.struck_out(phone, now), None);
        assert_eq!(l.forgive(), STRIKES - 1);
        // The minute's cap isn't forgiven.
        for _ in 0..PER_MINUTE {
            assert_eq!(l.admit(other, now), Ok(()));
        }
        l.forgive();
        assert!(l.admit(other, now).is_err());
    }

    /// A phone's log of 3 Nov 2026, replayed (times in seconds): a tab whose
    /// phone was just removed refused, the phone paired, Firefox closed,
    /// then its tabs restored at once, each with an older code in its
    /// address. Before, those were strikes and locked the phone out.
    #[test]
    fn restored_tabs_with_stale_codes_never_lock_their_paired_phone_out() {
        let known = known_one("s3cret");
        let mut l = Limiter::default();
        let ip: IpAddr = "192.168.1.9".parse().unwrap();
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        // As session.rs judges a hello: the minute's cap, the verdict, and
        // the wait only for one refused anyway (a guess then a strike).
        let mut hello_at = |h: &Hello, invite: Option<&mut Invite>, now: Instant| {
            l.admit(ip, now).map_err(Refusal::TooMany)?;
            match judge(h, invite, &known, now) {
                Verdict::Denied(r) => {
                    if let Some(wait) = l.struck_out(ip, now) {
                        return Err(Refusal::TooMany(wait));
                    }
                    if r.strikes() {
                        l.strike(ip, now);
                    }
                    Err(r)
                }
                v => Ok(v),
            }
        };
        let mut invite = Invite { token: "tok-1".into(), made: at(0), used: false };
        // A tab with a phone removed a moment before: a strike.
        assert_eq!(hello_at(&hello(Some(("old", "x")), None), Some(&mut invite), at(6)), Err(Refusal::Removed));
        // The QR scanned: a pairing (Allow, and it's dev1 from now on).
        assert!(matches!(hello_at(&hello(None, Some("tok-1")), Some(&mut invite), at(7)), Ok(Verdict::Pair { .. })));
        // Firefox closed, then its tabs restored at once: the one that
        // paired (its old address, `#pair=tok-1`), and others with older
        // codes. Each sends the device it has and the code in its address.
        for (i, code) in ["tok-1", "tok-0", "tok-b", "tok-a", "tok-1", "tok-0"].iter().enumerate() {
            let v = hello_at(&hello(Some(("dev1", "s3cret")), Some(code)), Some(&mut invite), at(174 + i as u64));
            assert_eq!(v, Ok(Verdict::Known { id: "dev1".into(), invited: false }), "tab {i}");
        }
        // A tab with no phone and an old code is refused, as before; four of
        // them, and the address waits for guesses...
        for i in 0..4 {
            assert_eq!(hello_at(&hello(None, Some("tok-0")), Some(&mut invite), at(240 + i)), Err(Refusal::BadLink));
        }
        let Err(Refusal::TooMany(wait)) = hello_at(&hello(None, Some("tok-0")), Some(&mut invite), at(245)) else {
            panic!("a fifth guess waits")
        };
        // (Until the first strike, at 6 s, is 10 minutes old: 361 s.)
        assert_eq!(wait, Duration::from_secs(361));
        assert_eq!(Refusal::TooMany(wait).words(), "Too many tries from this phone. Try again in 7 minutes.");
        // ...but never its paired phone,
        let v = hello_at(&hello(Some(("dev1", "s3cret")), None), Some(&mut invite), at(246));
        assert_eq!(v, Ok(Verdict::Known { id: "dev1".into(), invited: false }));
        // nor a fresh code from Settings (making it forgives them, too).
        let mut fresh = Invite { token: "tok-2".into(), made: at(247), used: false };
        assert!(matches!(hello_at(&hello(None, Some("tok-2")), Some(&mut fresh), at(250)), Ok(Verdict::Pair { .. })));
    }
}
