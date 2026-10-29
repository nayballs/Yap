//! Join codes: which meeting a call is, read from what its window shows
//! (a Meet tab's title, a browser's address) and from the event (its join
//! link, location, conference data and description), so a call is matched
//! to its own event rather than whichever meeting is on at the time
//! (`model::event_for_call`). Pure functions, so all of it is unit-tested;
//! the windows are read in `call_windows.rs`.
//!
//! A code is `<kind>:<value>`, lowercased:
//! - `meet:abc-defg-hij`: Google Meet's code. The tab's title shows it
//!   ("Meet - abc-defg-hij"), and so does its address.
//! - `zoom:81234567890`: a Zoom meeting id (9 to 11 digits), in a join link
//!   (`/j/`, `/s/`, `/w/`, `/wc/…`, `confno=`), an invite's "Meeting ID: 812
//!   3456 7890" and the web client's address (`app.zoom.us/wc/<id>/…`). The
//!   Zoom app titles its meeting window "Zoom Meeting", with no id (only
//!   older versions put "Meeting ID: …" there, which is read too).
//! - `teams:19:meeting_…@thread.v2`: a Teams meeting's thread, in its
//!   meetup-join link and in Teams on the web's address as you join.
//! - `teamsid:23615189355313`: a Teams meeting id, in the short links
//!   (`teams.microsoft.com/meet/<id>`, `teams.live.com/meet/<id>`) and an
//!   invite's "Meeting ID: 236 151 893 553 13". The Teams app shows neither
//!   (its windows are titled with the meeting's name), so a call in the app
//!   is matched by time and service, as before.
//!
//! Other services (Webex, Slack, GoTo…) have no code here: their calls
//! match by time and service.

/// A Zoom meeting id has this many digits…
const ZOOM_DIGITS: std::ops::RangeInclusive<usize> = 9..=11;
/// …and a Teams one at least this many.
const TEAMS_MIN_DIGITS: usize = 10;

/// What kind of code `code` is ("meet", "zoom", "teams", "teamsid"): codes
/// of one kind can be compared, a Teams thread and a Teams id can't.
pub fn kind(code: &str) -> &str {
    code.split(':').next().unwrap_or("")
}

/// `abc-defg-hij`: three, four and three lowercase letters.
fn is_meet_code(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && [3, 4, 3].iter().zip(&parts).all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_lowercase()))
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `%3a` → `:`, `%40` → `@` (any `%XX`); a broken escape left as it is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The code in one link or address: an event's join link, or what a
/// browser's address bar shows (often without `https://`).
pub fn in_link(link: &str) -> Option<String> {
    let lower = link.trim().to_ascii_lowercase();
    let rest = lower.split_once("://").map_or(lower.as_str(), |(_, r)| r);
    let (host, tail) = match rest.find(['/', '?', '#']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).trim_end_matches('.');
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    let path = tail.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if host == "meet.google.com" {
        return segments.first().filter(|s| is_meet_code(s)).map(|s| format!("meet:{s}"));
    }
    if on("zoom.us") || on("zoom.com") || on("zoomgov.com") {
        let id = match segments.as_slice() {
            ["j" | "s" | "w", id, ..] => Some(*id),
            ["wc", "join", id, ..] => Some(*id),
            ["wc", id, ..] => Some(*id),
            _ => None,
        }
        .filter(|id| all_digits(id))
        .map(str::to_string)
        .or_else(|| query_value(tail, "confno"));
        return id.filter(|id| ZOOM_DIGITS.contains(&id.len())).map(|id| format!("zoom:{id}"));
    }
    if on("teams.microsoft.com") || on("teams.live.com") || on("teams.microsoft.us") {
        // The thread, wherever the address names it (Teams on the web puts
        // it after a `#`).
        if let Some(at) = tail.find("meetup-join/") {
            let after = &tail[at + "meetup-join/".len()..];
            let raw = after.split(['/', '?', '&', '#']).next().unwrap_or("");
            let thread = percent_decode(raw);
            if thread.starts_with("19:") && thread.contains("@thread") {
                return Some(format!("teams:{thread}"));
            }
        }
        if let ["meet", id, ..] = segments.as_slice() {
            if all_digits(id) && id.len() >= TEAMS_MIN_DIGITS {
                return Some(format!("teamsid:{id}"));
            }
        }
    }
    None
}

/// `key`'s value in a link's query (`?confno=123&…`), digits only.
fn query_value(tail: &str, key: &str) -> Option<String> {
    let query = tail.split_once('?')?.1.split('#').next()?;
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
        .filter(|v| all_digits(v))
}

/// The digits of a "Meeting ID: 812 3456 7890" in `text` (spaces and dashes
/// between them), each time it's there.
fn meeting_ids(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("meeting id") {
        let start = from + at + "meeting id".len();
        let rest = &lower[start..];
        let rest = rest.trim_start_matches([' ', ':', '#', '\u{a0}']);
        let digits: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '\u{a0}'))
            .filter(char::is_ascii_digit)
            .collect();
        if digits.len() >= *ZOOM_DIGITS.start() {
            out.push(digits);
        }
        from = start;
    }
    out
}

/// A "Meeting ID" number as a code: Zoom's or Teams', by the service the
/// event or the call is on, else by its length.
fn id_code(digits: &str, service: Option<&str>) -> Option<String> {
    match service {
        Some("zoom") => ZOOM_DIGITS.contains(&digits.len()).then(|| format!("zoom:{digits}")),
        Some("teams") => (digits.len() >= TEAMS_MIN_DIGITS).then(|| format!("teamsid:{digits}")),
        Some(_) => None,
        None if ZOOM_DIGITS.contains(&digits.len()) => Some(format!("zoom:{digits}")),
        None if digits.len() >= 12 => Some(format!("teamsid:{digits}")),
        None => None,
    }
}

/// Every code in an event's texts (its join link, location, conference
/// data, description), once each, in order. `service`: the meeting service
/// its join link is on, which says whose a "Meeting ID" is.
pub fn in_event<'a>(texts: impl IntoIterator<Item = &'a str>, service: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |code: String| {
        if !out.contains(&code) {
            out.push(code);
        }
    };
    for text in texts {
        for link in super::links::links_in(text) {
            if let Some(code) = in_link(&link) {
                push(code);
            }
        }
        for digits in meeting_ids(text) {
            if let Some(code) = id_code(&digits, service) {
                push(code);
            }
        }
    }
    out
}

/// The code a call window's title shows, for call app `app` ("meet",
/// "zoom", "teams"…): Meet's tab ("Meet - abc-defg-hij - Google Chrome"),
/// or a "Meeting ID: …" an older Zoom app (or Teams, should it ever) puts
/// there. Teams' and today's Zoom's titles carry none.
pub fn in_title(app: &str, title: &str) -> Option<String> {
    let t = title.trim().to_lowercase();
    match app {
        "meet" => {
            let rest = ["meet - ", "meet \u{2013} ", "meet \u{2014} "].iter().find_map(|p| t.strip_prefix(p))?;
            let code: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || *c == '-').collect();
            is_meet_code(&code).then(|| format!("meet:{code}"))
        }
        "zoom" | "teams" => meeting_ids(&t).into_iter().find_map(|d| id_code(&d, Some(app))),
        _ => None,
    }
    // A title that is an address (a browser showing its URL as the title).
    .or_else(|| in_link(&t).filter(|c| matches_app(c, app)))
}

/// Whether `code` is a code of call app `app`'s meetings.
pub fn matches_app(code: &str, app: &str) -> bool {
    match kind(code) {
        "meet" => app == "meet",
        "zoom" => app == "zoom",
        "teams" | "teamsid" => app == "teams",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meet_codes_from_links_titles_and_addresses() {
        assert_eq!(in_link("https://meet.google.com/abc-defg-hij").as_deref(), Some("meet:abc-defg-hij"));
        assert_eq!(in_link("https://meet.google.com/abc-defg-hij?authuser=1&hs=122").as_deref(), Some("meet:abc-defg-hij"));
        // A browser's address bar often shows no scheme.
        assert_eq!(in_link("meet.google.com/abc-defg-hij").as_deref(), Some("meet:abc-defg-hij"));
        assert_eq!(in_link("https://meet.google.com/landing"), None);
        assert_eq!(in_link("https://meet.google.com/abcd-efg-hij"), None);
        // The tab's title, as Chrome and Edge title the window.
        assert_eq!(in_title("meet", "Meet - abc-defg-hij - Google Chrome").as_deref(), Some("meet:abc-defg-hij"));
        assert_eq!(in_title("meet", "Meet \u{2013} abc-defg-hij and 2 more pages - Work - Microsoft Edge").as_deref(), Some("meet:abc-defg-hij"));
        // The lobby, or a title with the meeting's name: no code.
        assert_eq!(in_title("meet", "Google Meet - Google Chrome"), None);
        assert_eq!(in_title("meet", "Meet - Weekly sync - Google Chrome"), None);
        // Another app's title never gives a Meet code.
        assert_eq!(in_title("zoom", "Meet - abc-defg-hij"), None);
    }

    #[test]
    fn zoom_ids_from_links_invites_and_the_web_client() {
        for (link, id) in [
            ("https://us02web.zoom.us/j/81234567890?pwd=e2e", "81234567890"),
            ("https://zoom.us/j/123456789", "123456789"),
            ("https://example.zoom.us/s/9876543210", "9876543210"),
            ("https://zoom.us/w/81234567890?tk=x", "81234567890"),
            ("app.zoom.us/wc/81234567890/join?fromPWA=1", "81234567890"),
            ("https://zoom.us/wc/join/81234567890", "81234567890"),
            ("zoommtg://zoom.us/join?action=join&confno=81234567890&pwd=x", "81234567890"),
            ("https://agency.zoomgov.com/j/1612345678", "1612345678"),
        ] {
            assert_eq!(in_link(link), Some(format!("zoom:{id}")), "{link}");
        }
        // A personal link's name, a too-short number, another site: none.
        assert_eq!(in_link("https://zoom.us/my/imogen"), None);
        assert_eq!(in_link("https://zoom.us/j/12345"), None);
        assert_eq!(in_link("https://notzoom.us/j/81234567890"), None);
        // An older Zoom app's window; today's names no id.
        assert_eq!(in_title("zoom", "Zoom Meeting ID: 812-3456-7890").as_deref(), Some("zoom:81234567890"));
        assert_eq!(in_title("zoom", "Zoom Meeting"), None);
        assert_eq!(in_title("zoom", "Zoom Workplace"), None);
    }

    #[test]
    fn teams_threads_and_ids() {
        let link = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_NjE3ZTk1%40thread.v2/0?context=%7b%22Tid%22%7d";
        assert_eq!(in_link(link).as_deref(), Some("teams:19:meeting_nje3ztk1@thread.v2"));
        // Teams on the web, as you join: the same thread after a `#`.
        let web = "teams.microsoft.com/v2/?meetingjoin=true#/l/meetup-join/19:meeting_NjE3ZTk1@thread.v2/0?context=x&anon=true";
        assert_eq!(in_link(web), in_link(link));
        // The short links carry the meeting id.
        assert_eq!(in_link("https://teams.microsoft.com/meet/23615189355313?p=Ab12").as_deref(), Some("teamsid:23615189355313"));
        assert_eq!(in_link("https://teams.live.com/meet/9876543210123").as_deref(), Some("teamsid:9876543210123"));
        // Its app's windows are titled with the meeting's name: nothing.
        assert_eq!(in_title("teams", "Weekly sync | Microsoft Teams"), None);
        assert_eq!(in_link("https://teams.microsoft.com/v2/"), None);
    }

    #[test]
    fn an_events_codes_come_from_every_text_once() {
        let description = "Agenda: the Q4 plan\n\nJoin: https://us02web.zoom.us/j/81234567890?pwd=x\n\
                           Meeting ID: 812 3456 7890\nPasscode: 123456";
        assert_eq!(in_event([description], Some("zoom")), ["zoom:81234567890"]);
        // A Teams invite: the thread from the link, the id from the text.
        let teams = "Join: https://teams.microsoft.com/l/meetup-join/19%3ameeting_e2e%40thread.v2/0\n\
                     Meeting ID: 312 456 789 012\nPasscode: x";
        assert_eq!(in_event([teams], Some("teams")), ["teams:19:meeting_e2e@thread.v2", "teamsid:312456789012"]);
        // Location first, then the description; no service: by its length.
        let codes = in_event(["https://meet.google.com/abc-defg-hij", "Meeting ID: 123 456 789"], None);
        assert_eq!(codes, ["meet:abc-defg-hij", "zoom:123456789"]);
        // Words that only look like a code, outside a Meet link, aren't one.
        assert!(in_event(["Please pre-read-doc before we meet"], None).is_empty());
        // A Zoom id under a Teams meeting isn't Teams'.
        assert!(in_event(["Meeting ID: 123 456 789"], Some("teams")).is_empty());
    }

    #[test]
    fn codes_belong_to_their_app() {
        assert!(matches_app("meet:abc-defg-hij", "meet"));
        assert!(matches_app("teamsid:123456789012", "teams"));
        assert!(!matches_app("zoom:123456789", "teams"));
        assert_eq!(kind("teams:19:meeting_x@thread.v2"), "teams");
        assert_eq!(percent_decode("19%3ameeting%40x%zz%4"), "19:meeting@x%zz%4");
    }
}
