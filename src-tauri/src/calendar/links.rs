//! Join links and invite text: which meeting service a link joins, the first
//! join link in an event (conference data, then location, then description),
//! and an invite's description cleaned down to what's worth keeping as
//! context. Pure functions, so all of it is unit-tested.
//!
//! Only HTTPS links on a known meeting service's host count (Wispr: "links
//! (HTTPS only)"): Yap opens them with one click, so a random link in a
//! description, a help page or a dial-in list must never qualify.

/// A meeting service: its id (the same ids as `meeting_detect::APPS`, so a
/// detected call can be matched to an event) and how Yap names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    pub id: &'static str,
    pub label: &'static str,
}

const fn service(id: &'static str, label: &'static str) -> Service {
    Service { id, label }
}

pub const TEAMS: Service = service("teams", "Teams");
pub const MEET: Service = service("meet", "Google Meet");
pub const ZOOM: Service = service("zoom", "Zoom");
pub const WEBEX: Service = service("webex", "Webex");
pub const SLACK: Service = service("slack", "Slack");
pub const GOTO: Service = service("goto", "GoTo Meeting");
pub const WHEREBY: Service = service("whereby", "Whereby");
pub const JITSI: Service = service("jitsi", "Jitsi Meet");
const CHIME: Service = service("chime", "Amazon Chime");
const BLUEJEANS: Service = service("bluejeans", "BlueJeans");
const SKYPE: Service = service("skype", "Skype");
const RINGCENTRAL: Service = service("ringcentral", "RingCentral");
const DIALPAD: Service = service("dialpad", "Dialpad");
const AROUND: Service = service("around", "Around");

/// A service's label by id ("teams" → "Teams").
pub fn label(id: &str) -> Option<&'static str> {
    [
        TEAMS, MEET, ZOOM, WEBEX, SLACK, GOTO, WHEREBY, JITSI, CHIME, BLUEJEANS, SKYPE, RINGCENTRAL,
        DIALPAD, AROUND,
    ]
    .iter()
    .find(|s| s.id == id)
    .map(|s| s.label)
}

/// The meeting service `link` joins, if it's an HTTPS join link (not a help
/// page, an options page or a dial-in list on the same host).
pub fn service_of(link: &str) -> Option<Service> {
    let url = url::Url::parse(link).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    let segment = |p: &str| path.starts_with(p) && path.len() > p.len();

    if on("teams.microsoft.com") || on("teams.live.com") || on("teams.microsoft.us") {
        // Not "meetingOptions", the help pages or the download page.
        let join = segment("/l/meetup-join/") || segment("/meet/") || path.contains("/l/meetup-join/");
        return join.then_some(TEAMS);
    }
    if host == "meet.google.com" {
        // A meeting code ("abc-defg-hij") or a lookup link; not the bare host.
        let code = path.trim_start_matches('/');
        let meeting = !code.is_empty() && (code.starts_with("lookup/") || code.contains('-'));
        return meeting.then_some(MEET);
    }
    if on("zoom.us") || on("zoomgov.com") || on("zoom.com") {
        // /j/ meetings, /w/ webinars, /my/ personal rooms; not /u/ (the
        // "find your local number" list) or the support pages.
        let join = segment("/j/") || segment("/w/") || segment("/my/") || segment("/wc/join/");
        return join.then_some(ZOOM);
    }
    if on("webex.com") {
        let join = path.contains("/meet/")
            || path.contains("/join/")
            || path.ends_with("j.php")
            || path.contains("/joinservice/");
        return (join && !host.starts_with("help.")).then_some(WEBEX);
    }
    if host == "app.slack.com" {
        return segment("/huddle/").then_some(SLACK);
    }
    if host == "meet.goto.com" || host == "gotomeet.me" {
        return (path.len() > 1).then_some(GOTO);
    }
    if on("gotomeeting.com") {
        let join = segment("/join/") || query.contains("meetingid=");
        return join.then_some(GOTO);
    }
    if host == "whereby.com" {
        // A room: one path segment that isn't one of the site's own pages.
        let room = path.trim_matches('/');
        let site = ["", "information", "pricing", "login", "user", "org", "blog", "terms", "privacy"];
        return (!room.contains('/') && !site.contains(&room)).then_some(WHEREBY);
    }
    if host == "meet.jit.si" {
        return (path.len() > 1).then_some(JITSI);
    }
    if host == "chime.aws" {
        let id = path.trim_matches('/');
        return (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(CHIME);
    }
    if on("bluejeans.com") {
        let id = path.trim_matches('/').split('/').next().unwrap_or("");
        return (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(BLUEJEANS);
    }
    if host == "join.skype.com" {
        return (path.len() > 1).then_some(SKYPE);
    }
    if host == "meetings.ringcentral.com" || host == "v.ringcentral.com" {
        return (segment("/j/") || segment("/join/")).then_some(RINGCENTRAL);
    }
    if host == "meetings.dialpad.com" {
        return (path.len() > 1).then_some(DIALPAD);
    }
    if host == "around.co" {
        return (path.len() > 1).then_some(AROUND);
    }
    None
}

/// A join link found in an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinLink {
    pub url: String,
    pub service: Service,
}

/// The first join link in `texts`, searched in order (conference data before
/// location before description), each scanned for every `https://` link.
pub fn first_join_link<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<JoinLink> {
    texts.into_iter().find_map(|text| {
        links_in(text)
            .into_iter()
            .find_map(|url| service_of(&url).map(|service| JoinLink { url, service }))
    })
}

/// Every `https://…` link in `text`, as written (an HTML `&amp;` decoded, the
/// trailing punctuation of a sentence or a `<…>` wrapper left off).
pub fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("https://") {
        let start = from + at;
        let rest = &text[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`' | '|' | '\\' | '^'))
            .unwrap_or(rest.len());
        let mut link = rest[..end].replace("&amp;", "&");
        // A sentence's full stop, a closing bracket the link didn't open…
        loop {
            let Some(last) = link.chars().last() else { break };
            let unbalanced = |open: char, close: char| {
                last == close && link.matches(open).count() < link.matches(close).count()
            };
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '*')
                || unbalanced('(', ')')
                || unbalanced('[', ']')
                || unbalanced('{', '}')
            {
                link.pop();
            } else {
                break;
            }
        }
        if link.len() > "https://".len() {
            out.push(link);
        }
        from = start + end.max(1);
    }
    out
}

/// HTML (Google's descriptions often are) as plain text: line breaks for
/// `<br>`, paragraphs and list items, other tags dropped, entities decoded.
/// Plain text comes back unchanged.
pub fn html_to_text(s: &str) -> String {
    let looks_html = ["<br", "<p", "<div", "<a ", "<li", "<span", "<b>", "<ul", "<html"]
        .iter()
        .any(|t| s.to_ascii_lowercase().contains(t));
    if !looks_html {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if matches!(name, "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "ul" | "ol") {
            out.push('\n');
        }
        // Keep a link's address: an invite's "Join" is often only an href.
        if name == "a" {
            if let Some(href) = attr(&rest[open..open + close], "href") {
                out.push_str(&href);
                out.push(' ');
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    decode_entities(&out)
}

/// An attribute's value in an HTML tag (`href="…"` or `href='…'`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let quote = rest.chars().next()?;
    let value = if quote == '"' || quote == '\'' {
        let body = &rest[1..];
        &body[..body.find(quote)?]
    } else {
        rest.split(|c: char| c.is_whitespace()).next()?
    };
    Some(decode_entities(value))
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}

/// Invite boilerplate a meeting app adds around the link, matched
/// case-insensitively anywhere in a line.
const BOILERPLATE: &[&str] = &[
    "microsoft teams meeting",
    "microsoft teams need help",
    "join on your computer",
    "join the meeting now",
    "click here to join",
    "join microsoft teams meeting",
    "download teams",
    "join on the web",
    "meeting id",
    "passcode",
    "password:",
    "dial-in",
    "dial in",
    "dial by your location",
    "find your local number",
    "join by phone",
    "join by video system",
    "join by sip",
    "or call in",
    "one tap mobile",
    "join zoom meeting",
    "join with google meet",
    "more phone numbers",
    "phone conference id",
    "conference id",
    "access code",
    "learn more",
    "meeting options",
    "need help?",
    "system reference",
    "for organizers",
    "invitation from google calendar",
    "you are receiving this",
    "forwarding this invitation",
    "to stop receiving",
    "view all guest info",
    "reply for ",
    "join webex meeting",
    "tap to join",
];

/// What's worth keeping from an invite's description as meeting context: the
/// agenda and notes, without links, dial-in lists or the meeting app's
/// boilerplate. At most `max_chars`, on a word.
pub fn clean_description(raw: &str, max_chars: usize) -> String {
    let text = html_to_text(raw).replace("\r\n", "\n");
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let lower = line.to_lowercase();
        let rule = !line.is_empty() && line.chars().all(|c| matches!(c, '_' | '-' | '=' | '*' | '.' | '~' | ' '));
        let digits = line.chars().filter(|c| c.is_ascii_digit()).count();
        let letters = line.chars().filter(|c| c.is_alphabetic()).count();
        let phoneish = digits >= 7 && digits >= letters;
        let boilerplate = BOILERPLATE.iter().any(|b| lower.contains(b));
        let link = lower.contains("http://") || lower.contains("https://") || lower.contains("mailto:");
        if rule || phoneish || boilerplate || link {
            continue;
        }
        if line.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let joined = lines.join("\n");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let cut: String = joined.chars().take(max_chars).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(at) if at > max_chars / 2 => &cut[..at],
        _ => &cut[..],
    };
    format!("{}\u{2026}", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_join_links_from_help_and_options_pages() {
        let cases = [
            ("https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%7d", Some("teams")),
            ("https://teams.microsoft.com/meet/2534567890123?p=AbCdEf", Some("teams")),
            ("https://teams.live.com/meet/9876543210987?p=xyz", Some("teams")),
            ("https://teams.microsoft.com/meetingOptions/?organizerId=1&tenantId=2", None),
            ("https://aka.ms/JoinTeamsMeeting", None),
            ("https://meet.google.com/abc-defg-hij", Some("meet")),
            ("https://meet.google.com/", None),
            ("https://support.google.com/a/users/answer/9282720", None),
            ("https://us02web.zoom.us/j/81234567890?pwd=abc", Some("zoom")),
            ("https://zoom.us/my/nathan", Some("zoom")),
            ("https://us02web.zoom.us/u/kdXyZ", None),
            ("https://acme.zoomgov.com/j/1601234567", Some("zoom")),
            ("https://acme.webex.com/acme/j.php?MTID=m123", Some("webex")),
            ("https://acme.webex.com/meet/nathan", Some("webex")),
            ("https://help.webex.com/en-us/article/nkzs6f1/Join-a-meeting", None),
            ("https://app.slack.com/huddle/T012/C034", Some("slack")),
            ("https://meet.goto.com/123456789", Some("goto")),
            ("https://global.gotomeeting.com/join/123456789", Some("goto")),
            ("https://whereby.com/team-standup", Some("whereby")),
            ("https://whereby.com/pricing", None),
            ("https://meet.jit.si/YapPlanning", Some("jitsi")),
            ("https://chime.aws/1234567890", Some("chime")),
            ("https://example.com/j/123", None),
        ];
        for (link, want) in cases {
            assert_eq!(service_of(link).map(|s| s.id), want, "{link}");
        }
    }

    #[test]
    fn only_https_links_count() {
        assert_eq!(service_of("http://meet.google.com/abc-defg-hij"), None);
        assert_eq!(service_of("javascript:alert(1)"), None);
        assert_eq!(service_of("ftp://zoom.us/j/1"), None);
    }

    #[test]
    fn finds_links_in_text_without_trailing_punctuation() {
        let text = "Join here: https://meet.google.com/abc-defg-hij. Or (https://zoom.us/j/123?pwd=x), \
                    <https://teams.microsoft.com/meet/123?p=a&amp;b=c>";
        assert_eq!(
            links_in(text),
            [
                "https://meet.google.com/abc-defg-hij",
                "https://zoom.us/j/123?pwd=x",
                "https://teams.microsoft.com/meet/123?p=a&b=c",
            ]
        );
        // A link that legitimately ends in a bracket keeps it.
        assert_eq!(links_in("see https://en.wikipedia.org/wiki/Foo_(bar)"), ["https://en.wikipedia.org/wiki/Foo_(bar)"]);
    }

    #[test]
    fn the_first_join_link_wins_in_source_order() {
        let conference = "";
        let location = "Room 4 / https://zoom.us/j/111";
        let description = "Agenda https://example.com/doc then https://meet.google.com/xyz-abcd-efg";
        let found = first_join_link([conference, location, description]).unwrap();
        assert_eq!(found.service, ZOOM);
        assert_eq!(found.url, "https://zoom.us/j/111");
        // Only an unknown link: nothing to join.
        assert_eq!(first_join_link(["https://example.com/doc"]), None);
    }

    #[test]
    fn hrefs_survive_html_descriptions() {
        let html = "<p>Agenda:<br>1. Q3 numbers</p><a href=\"https://teams.microsoft.com/l/meetup-join/19%3a1?ctx=a&amp;b=c\">Join</a>";
        let text = html_to_text(html);
        assert!(text.contains("Agenda:\n1. Q3 numbers"));
        let link = first_join_link([text.as_str()]).unwrap();
        assert_eq!(link.url, "https://teams.microsoft.com/l/meetup-join/19%3a1?ctx=a&b=c");
    }

    #[test]
    fn invite_boilerplate_is_dropped_from_the_description() {
        let teams = "Let's go through the launch checklist.\n\n\
            ________________________________________________________________________________\n\
            Microsoft Teams meeting\n\
            Join on your computer, mobile app or room device\n\
            Click here to join the meeting<https://teams.microsoft.com/l/meetup-join/19%3a1>\n\
            Meeting ID: 312 456 789 012\n\
            Passcode: Ab3dEf\n\
            Download Teams<https://www.microsoft.com/microsoft-teams/download-app> | Join on the web\n\
            Or call in (audio only)\n\
            +44 20 3443 0000,,123456789# United Kingdom, London\n\
            Phone Conference ID: 123 456 789#\n\
            Learn More<https://aka.ms/JoinTeamsMeeting> | Meeting options<https://teams.microsoft.com/meetingOptions/>\n\
            ________________________________________________________________________________";
        assert_eq!(clean_description(teams, 600), "Let's go through the launch checklist.");

        let zoom = "Agenda: pricing, then the beta list\nJoin Zoom Meeting\nhttps://us02web.zoom.us/j/812?pwd=x\n\
            Meeting ID: 812 3456 7890\nOne tap mobile\n+16469313860,,81234567890# US";
        assert_eq!(clean_description(zoom, 600), "Agenda: pricing, then the beta list");
    }

    #[test]
    fn long_descriptions_are_cut_on_a_word() {
        let text = "word ".repeat(100);
        let cut = clean_description(&text, 42);
        assert!(cut.ends_with('\u{2026}'));
        assert!(cut.chars().count() <= 43);
        assert!(!cut.contains("wor\u{2026}"));
    }
}
