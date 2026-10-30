//! Meetings as Yap keeps them: one shape for both sources (a Google
//! Calendar or an iCal feed), Wispr's filters, and the decisions made from
//! them: which event a recording or a call belongs to, and when a reminder
//! is due. Pure functions over plain data, so all of it is unit-tested.
//!
//! **Filters** (Wispr's: "meetings with invitees… no all-day events, nothing
//! over 6 hours"): all-day events never get this far (the sources skip
//! them); cancelled events, events you declined and events over 6 hours are
//! dropped here; and an event needs someone besides you on it, or a join
//! link (Yap's addition: an online meeting whose feed lists no guests, as
//! some published Outlook calendars do, is still a meeting).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::ics::{self, Component};
use super::links;

/// Wispr's limit: nothing over 6 hours is a meeting.
pub const MAX_LENGTH_SECS: i64 = 6 * 3600;
/// How much of an invite's description is kept as context.
pub const DESCRIPTION_CHARS: usize = 1_000;
/// "Join + Start" and the matching of recordings and calls begin this long
/// before an event starts (Wispr: "from 10 minutes before").
pub const EARLY_SECS: i64 = 10 * 60;
/// A reminder stays up until this long after the start (Wispr: 5 minutes).
pub const REMINDER_AFTER_SECS: i64 = 5 * 60;

/// Someone invited, besides you.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// One meeting occurrence (what `calendar.json` caches).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Stable per occurrence: `<connection>:<uid>` plus, for a recurring
    /// event, `:<original start>` (a moved occurrence keeps its key).
    pub key: String,
    pub connection: String,
    #[serde(default)]
    pub title: String,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    /// Everyone invited but you; rooms, resources and people who declined
    /// left out.
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub organizer: String,
    /// An HTTPS link on a known meeting service. Stays in Rust: the page asks
    /// Yap to open it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_url: Option<String>,
    /// The meeting service ("teams", "meet", "zoom"…: `links::Service` ids).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// The invite's description, cleaned of links, dial-ins and boilerplate.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// You answered "maybe".
    #[serde(default)]
    pub tentative: bool,
    /// Its join codes (`codes.rs`: "meet:abc-defg-hij", "zoom:81234567890"…),
    /// from its join link, location, conference data and description: a
    /// call showing one of them is this meeting. An older Yap saving
    /// calendar.json drops them until the next sync puts them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
}

impl Event {
    /// What a note or a card calls it: its title, else "Meeting with Tanay".
    pub fn display_title(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.trim().to_string();
        }
        match self.attendees.first() {
            Some(a) => format!("Meeting with {}", first_name(&a.name)),
            None => "Meeting".to_string(),
        }
    }

    /// "Tanay Kothari, Priya Shah +2".
    pub fn with_line(&self) -> String {
        let names: Vec<&str> = self.attendees.iter().map(|a| a.name.as_str()).collect();
        match names.len() {
            0 => String::new(),
            1 | 2 => names.join(", "),
            n => format!("{}, {} +{}", names[0], names[1], n - 2),
        }
    }

    /// The service's name, for "Teams meeting".
    pub fn service_label(&self) -> Option<&'static str> {
        self.service.as_deref().and_then(links::label)
    }
}

fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// The reply someone gave the invite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Accepted,
    Tentative,
    Declined,
    Unknown,
}

impl Reply {
    /// ICS PARTSTAT or Google's responseStatus.
    pub fn parse(s: &str) -> Reply {
        match s.trim().to_ascii_lowercase().as_str() {
            "accepted" => Reply::Accepted,
            "tentative" => Reply::Tentative,
            "declined" => Reply::Declined,
            _ => Reply::Unknown,
        }
    }
}

/// Someone on an event as the source lists them, you included.
#[derive(Debug, Clone)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub is_self: bool,
    /// A room or a piece of equipment.
    pub resource: bool,
    pub reply: Reply,
}

/// An event as read from its source, before the filters.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub key: String,
    pub connection: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub people: Vec<Person>,
    pub organizer: Option<Person>,
    /// Where a join link may be, best first (conference data, location…).
    pub link_sources: Vec<String>,
    /// The description as written (HTML or plain text).
    pub description: String,
    pub cancelled: bool,
}

/// Apply the filters (see the module docs). `None`: not a meeting.
pub fn finish(d: Draft) -> Option<Event> {
    if d.cancelled || d.end - d.start > MAX_LENGTH_SECS {
        return None;
    }
    let me = d.people.iter().find(|p| p.is_self);
    if me.is_some_and(|p| p.reply == Reply::Declined) {
        return None;
    }
    let mut seen = HashSet::new();
    let attendees: Vec<Attendee> = d
        .people
        .iter()
        .filter(|p| !p.is_self && !p.resource && p.reply != Reply::Declined)
        .filter(|p| seen.insert(if p.email.is_empty() { p.name.to_lowercase() } else { p.email.to_lowercase() }))
        .map(|p| Attendee { name: display_name(&p.name, &p.email), email: p.email.clone() })
        .filter(|a| !a.name.is_empty())
        .collect();
    let description_text = links::html_to_text(&d.description);
    let join = links::first_join_link(
        d.link_sources.iter().map(String::as_str).chain([description_text.as_str()]),
    );
    if attendees.is_empty() && join.is_none() {
        return None;
    }
    let service = join.as_ref().map(|j| j.service.id);
    let codes = super::codes::in_event(
        join.iter().map(|j| j.url.as_str()).chain(d.link_sources.iter().map(String::as_str)).chain([description_text.as_str()]),
        service,
    );
    Some(Event {
        key: d.key,
        connection: d.connection,
        title: d.title.trim().to_string(),
        start: d.start,
        end: d.end.max(d.start),
        organizer: d.organizer.map(|o| display_name(&o.name, &o.email)).unwrap_or_default(),
        tentative: me.is_some_and(|p| p.reply == Reply::Tentative),
        service: join.as_ref().map(|j| j.service.id.to_string()),
        join_url: join.map(|j| j.url),
        description: links::clean_description(&d.description, DESCRIPTION_CHARS),
        attendees,
        codes,
    })
}

/// The longest event of your own kept as busy time ([`Busy`]).
pub const MAX_BUSY_SECS: i64 = 24 * 3600;

/// Time you're busy that isn't a meeting: an event of your own with nobody
/// else on it and no join link ("Dentist"), or one over 6 hours. Kept only
/// so Chat's calendar tool can answer "am I free at 3 on Friday?" truly;
/// never a reminder, a brief or a meeting note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Busy {
    pub key: String,
    pub connection: String,
    #[serde(default)]
    pub title: String,
    pub start: i64,
    pub end: i64,
}

/// What a draft from a source becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Kept {
    Meeting(Event),
    Busy(Busy),
    /// Cancelled, declined, or longer than [`MAX_BUSY_SECS`].
    Neither,
}

/// A draft sorted: a meeting ([`finish`]), else busy time of your own, else
/// nothing. Cancelled and declined events are never busy time.
pub fn sort(d: Draft) -> Kept {
    let declined = d.people.iter().any(|p| p.is_self && p.reply == Reply::Declined);
    if d.cancelled || declined || d.end - d.start > MAX_BUSY_SECS {
        return Kept::Neither;
    }
    let (key, connection, title, start, end) = (d.key.clone(), d.connection.clone(), d.title.trim().to_string(), d.start, d.end.max(d.start));
    match finish(d) {
        Some(e) => Kept::Meeting(e),
        None => Kept::Busy(Busy { key, connection, title, start, end }),
    }
}

/// A person's name for notes: the one the invite gives, else one made from
/// their address ("tanay.kothari@…" → "Tanay Kothari").
pub fn display_name(name: &str, email: &str) -> String {
    let name = name.trim().trim_matches('"').trim();
    if !name.is_empty() && !name.eq_ignore_ascii_case(email) && !name.contains('@') {
        return name.to_string();
    }
    let local = email.split('@').next().unwrap_or("").trim();
    local
        .split(['.', '_', '-', '+'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- iCal feeds ----------------------------------------------------------------------------

/// `mailto:x@y` → `x@y`, lowercased.
fn address(value: &str) -> String {
    let v = value.trim();
    let v = v.strip_prefix("mailto:").or_else(|| v.strip_prefix("MAILTO:")).unwrap_or(v);
    v.trim().to_ascii_lowercase()
}

fn is_address(s: &str) -> bool {
    let s = s.trim();
    s.contains('@') && !s.contains(char::is_whitespace) && s.split('@').nth(1).is_some_and(|d| d.contains('.'))
}

/// Who "you" are in a feed that doesn't say: the calendar's name when it's
/// an address (Google's secret iCal address is named after its owner), else
/// the one person invited to (nearly) every meeting in it, as the feed's
/// owner is. Two such people (all your meetings are with Sam) is a tie, and
/// nobody: taking Sam for you would leave your meetings with Sam without
/// anyone else in them, and drop them.
pub fn self_addresses(calendar: &Component) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(name) = ics::calendar_name(calendar).filter(|n| is_address(n)) {
        out.insert(name.to_ascii_lowercase());
        return out;
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut meetings = 0;
    for event in calendar.children.iter().filter(|c| c.name == "VEVENT") {
        let people: HashSet<String> =
            event.props_named("ATTENDEE").map(|p| address(&p.value)).filter(|a| is_address(a)).collect();
        if people.len() < 2 {
            continue;
        }
        meetings += 1;
        for a in people {
            *counts.entry(a).or_default() += 1;
        }
    }
    if meetings >= 3 {
        let regulars: Vec<String> = counts.into_iter().filter(|(_, n)| *n * 10 >= meetings * 9).map(|(a, _)| a).collect();
        if let [owner] = regulars.as_slice() {
            out.insert(owner.clone());
        }
    }
    out
}

/// One iCal occurrence as a draft (see [`finish`]).
pub fn draft_from_ics(connection: &str, occ: &ics::Occurrence, me: &HashSet<String>) -> Draft {
    let event = occ.source;
    let person = |p: &ics::Prop| {
        let email = address(&p.value);
        Person {
            name: p.param("CN").unwrap_or("").to_string(),
            is_self: me.contains(&email),
            resource: p
                .param("CUTYPE")
                .is_some_and(|c| matches!(c.to_ascii_uppercase().as_str(), "ROOM" | "RESOURCE"))
                || p.param("ROLE").is_some_and(|r| r.eq_ignore_ascii_case("NON-PARTICIPANT")),
            reply: p.param("PARTSTAT").map(Reply::parse).unwrap_or(Reply::Unknown),
            email,
        }
    };
    let key = match occ.recurrence_id {
        Some(rid) => format!("{connection}:{}:{rid}", occ.uid),
        None => format!("{connection}:{}", occ.uid),
    };
    let conference = [
        "X-GOOGLE-CONFERENCE",
        "X-MICROSOFT-SKYPETEAMSMEETINGURL",
        "X-MICROSOFT-ONLINEMEETINGEXTERNALLINK",
        "URL",
        "LOCATION",
    ];
    let mut link_sources: Vec<String> = conference.iter().filter_map(|name| event.text(name)).collect();
    // Outlook's HTML description sits apart from the plain one.
    if let Some(html) = event.text("X-ALT-DESC") {
        link_sources.push(links::html_to_text(&html));
    }
    Draft {
        key,
        connection: connection.to_string(),
        title: event.text("SUMMARY").unwrap_or_default(),
        start: occ.start,
        end: occ.end,
        people: event.props_named("ATTENDEE").map(person).collect(),
        organizer: event.prop("ORGANIZER").map(person),
        link_sources,
        description: event.text("DESCRIPTION").unwrap_or_default(),
        cancelled: event.value("STATUS").is_some_and(|s| s.trim().eq_ignore_ascii_case("CANCELLED")),
    }
}

// ---- decisions -------------------------------------------------------------------------------

/// Keys of the events that overlap another one ("Conflict").
pub fn conflicts(events: &[Event]) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by_key(|e| e.start);
    for (i, a) in sorted.iter().enumerate() {
        for b in &sorted[i + 1..] {
            if b.start >= a.end {
                break;
            }
            if b.start < a.end && a.start < b.end {
                out.insert(a.key.clone());
                out.insert(b.key.clone());
            }
        }
    }
    out
}

/// Is `e` on now (or about to start: from [`EARLY_SECS`] before)?
pub fn is_current(e: &Event, now: i64) -> bool {
    e.start - EARLY_SECS <= now && now < e.end.max(e.start + 60)
}

/// A call's join code finds its event from this long before the start…
pub const CODE_EARLY_SECS: i64 = 30 * 60;
/// …until this long after the end (a meeting running over).
pub const CODE_LATE_SECS: i64 = 30 * 60;

/// The event a recording or a call at `now` belongs to. When the call's
/// window showed join code `code` (`codes.rs`), **the code beats time and
/// service**: the event with that code from [`CODE_EARLY_SECS`] before it
/// starts until [`CODE_LATE_SECS`] after it ends, the one starting nearest
/// `now` (a recurring meeting keeps its code every week). Else by time and
/// service: one that's on or about to start, on `service` when a call's
/// app is known (events with no join link fit any call), the one starting
/// nearest `now` (back-to-back meetings: at 14:58 a recording is for the
/// 15:00 one, not the 14:30 one) — but never a meeting whose own codes of
/// that kind don't include the call's (an ad-hoc Zoom call during a Zoom
/// meeting isn't it).
pub fn event_for_call<'a>(events: &'a [Event], now: i64, service: Option<&str>, code: Option<&str>) -> Option<&'a Event> {
    if let Some(found) = code.and_then(|c| event_by_code(events, now, c)) {
        return Some(found);
    }
    events
        .iter()
        .filter(|e| is_current(e, now))
        .filter(|e| match (service, e.service.as_deref()) {
            (Some(call), Some(link)) => call == link,
            _ => true,
        })
        .filter(|e| code.is_none_or(|c| !has_other_code(e, c)))
        .min_by_key(|e| ((e.start - now).abs(), e.start))
}

/// The event whose join codes include `code`, around `now` (see
/// [`event_for_call`]).
pub fn event_by_code<'a>(events: &'a [Event], now: i64, code: &str) -> Option<&'a Event> {
    events
        .iter()
        .filter(|e| e.codes.iter().any(|c| c == code))
        .filter(|e| e.start - CODE_EARLY_SECS <= now && now < e.end.max(e.start + 60) + CODE_LATE_SECS)
        .min_by_key(|e| ((e.start - now).abs(), e.start))
}

/// Whether a call of `service` at `now` (its join code `code`) is another
/// meeting than the one a note is tied to (`linked`: its key, and when it
/// ends): the note's meeting ended over [`CODE_LATE_SECS`] ago, or the
/// calendar puts the call in another of its meetings. Call detection
/// carries a call that came back on in that note only when it isn't (two
/// meetings back to back on one app; the next morning's first call after a
/// night asleep). No meeting of the calendar's for the call: not another.
pub fn another_meeting(events: &[Event], now: i64, service: Option<&str>, code: Option<&str>, linked_key: &str, linked_end: i64) -> bool {
    now > linked_end + CODE_LATE_SECS || event_for_call(events, now, service, code).is_some_and(|e| e.key != linked_key)
}

/// `e` has codes of `code`'s kind, and `code` isn't one of them: it's
/// another meeting on the same service.
fn has_other_code(e: &Event, code: &str) -> bool {
    let kind = super::codes::kind(code);
    e.codes.iter().any(|c| super::codes::kind(c) == kind) && !e.codes.iter().any(|c| c == code)
}

/// The meeting the tray names ("Next: Weekly sync in 12 min"), at `now`:
/// the one being recorded while it's on (`recording`: its key); else one
/// starting within [`EARLY_SECS`] (the soonest); else one on now (the one
/// that started last: of two overlapping, the one you most likely went
/// into); else the next one to start. Never one that's over. The calendar
/// keeps meetings only — all-day events and ones you declined never get
/// this far ([`finish`]).
pub fn next_meeting<'a>(events: &'a [Event], now: i64, recording: Option<&str>) -> Option<&'a Event> {
    let on = |e: &&Event| e.start <= now && now < e.end.max(e.start + 60);
    if let Some(e) = recording.and_then(|key| events.iter().filter(on).find(|e| e.key == key)) {
        return Some(e);
    }
    let by_start = |e: &&Event| (e.start, e.end, e.key.clone());
    if let Some(e) = events.iter().filter(|e| e.start > now && e.start - now <= EARLY_SECS).min_by_key(by_start) {
        return Some(e);
    }
    if let Some(e) = events.iter().filter(on).max_by_key(|e| (e.start, std::cmp::Reverse(e.end), std::cmp::Reverse(e.key.clone()))) {
        return Some(e);
    }
    events.iter().filter(|e| e.start > now).min_by_key(by_start)
}

/// The id a reminder answer is kept under: the occurrence and its start, so
/// a moved meeting gets a fresh reminder.
pub fn answer_key(e: &Event) -> String {
    format!("{}@{}", e.key, e.start)
}

/// The meeting whose reminder is due at `now` with lead time `lead` seconds:
/// from `lead` before its start until [`REMINDER_AFTER_SECS`] after (and
/// while it's on), not answered, not snoozed, and not the one being
/// recorded. The earliest first.
pub fn due_reminder<'a>(
    events: &'a [Event],
    now: i64,
    lead: i64,
    answered: &HashSet<String>,
    snoozed: &HashMap<String, i64>,
    recording_event: Option<&str>,
) -> Option<&'a Event> {
    events
        .iter()
        .filter(|e| e.start - lead <= now && now < e.start + REMINDER_AFTER_SECS && now < e.end.max(e.start + 60))
        .filter(|e| !answered.contains(&answer_key(e)))
        .filter(|e| snoozed.get(&answer_key(e)).is_none_or(|until| now >= *until))
        .filter(|e| recording_event != Some(e.key.as_str()))
        .min_by_key(|e| (e.start, e.key.clone()))
}

/// When a reminder becomes due next (for the scheduler's sleep): the
/// soonest `start - lead` still ahead, or a snooze running out.
pub fn next_reminder_at(events: &[Event], now: i64, lead: i64, snoozed: &HashMap<String, i64>) -> Option<i64> {
    let starts = events.iter().map(|e| e.start - lead).filter(|t| *t > now);
    let snoozes = snoozed.values().copied().filter(|t| *t > now);
    starts.chain(snoozes).min()
}

/// A title that's only a placeholder, which a calendar title may replace:
/// empty, or what Yap names a meeting note it starts by itself ("Teams call
/// · 5 Oct, 14:30" from call detection, "Meeting · 5 Oct, 14:30" from the
/// meeting shortcut).
pub fn is_placeholder_title(title: &str) -> bool {
    let t = title.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("untitled note") || t.eq_ignore_ascii_case("untitled") {
        return true;
    }
    let Some((what, when)) = t.split_once(" \u{b7} ") else { return false };
    let what_ok = what == "Meeting" || what.ends_with(" call") || what.ends_with(" huddle");
    let mut parts = when.splitn(2, ", ");
    let (Some(day_month), Some(time)) = (parts.next(), parts.next()) else { return false };
    let day_month_ok = day_month
        .split_once(' ')
        .is_some_and(|(d, m)| d.parse::<u8>().is_ok() && m.len() == 3 && m.chars().all(char::is_alphabetic));
    let time_ok = time.len() == 5 && time.as_bytes()[2] == b':' && time.chars().filter(char::is_ascii_digit).count() == 4;
    what_ok && day_month_ok && time_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, email: &str) -> Person {
        Person { name: name.into(), email: email.into(), is_self: false, resource: false, reply: Reply::Accepted }
    }

    fn me(reply: Reply) -> Person {
        Person { is_self: true, reply, ..person("Imogen", "imogen@example.com") }
    }

    fn draft(people: Vec<Person>) -> Draft {
        Draft {
            key: "c:1".into(),
            connection: "c".into(),
            title: "Design review".into(),
            start: 1_000,
            end: 1_000 + 3_600,
            people,
            ..Default::default()
        }
    }

    pub(crate) fn event(key: &str, start: i64, end: i64, service: Option<&str>) -> Event {
        Event {
            key: key.into(),
            connection: "c".into(),
            title: key.into(),
            start,
            end,
            attendees: vec![Attendee { name: "Tanay".into(), email: String::new() }],
            organizer: String::new(),
            join_url: None,
            service: service.map(str::to_string),
            description: String::new(),
            tentative: false,
            codes: Vec::new(),
        }
    }

    fn coded(key: &str, start: i64, end: i64, service: &str, codes: &[&str]) -> Event {
        Event { codes: codes.iter().map(|c| c.to_string()).collect(), ..event(key, start, end, Some(service)) }
    }

    #[test]
    fn a_join_code_beats_time_and_service() {
        let now = 10_000;
        // Two Zoom meetings on now; the call's window shows the second one's id.
        let sync = coded("sync", now - 300, now + 1_500, "zoom", &["zoom:111222333"]);
        let interview = coded("interview", now - 120, now + 1_680, "zoom", &["zoom:444555666"]);
        let events = [sync.clone(), interview];
        assert_eq!(event_for_call(&events, now, Some("zoom"), None).unwrap().key, "interview"); // nearest start
        assert_eq!(event_for_call(&events, now, Some("zoom"), Some("zoom:111222333")).unwrap().key, "sync");
        // Joined 25 minutes early, or 20 minutes over: still found by its code…
        let later = [coded("standup", now + 1_500, now + 2_400, "meet", &["meet:abc-defg-hij"])];
        assert!(event_for_call(&later, now, Some("meet"), None).is_none());
        assert_eq!(event_for_call(&later, now, Some("meet"), Some("meet:abc-defg-hij")).unwrap().key, "standup");
        let over = [coded("review", now - 3_000, now - 1_200, "meet", &["meet:abc-defg-hij"])];
        assert_eq!(event_for_call(&over, now, None, Some("meet:abc-defg-hij")).unwrap().key, "review");
        // …but not an hour early.
        let far = [coded("standup", now + 3_600, now + 4_500, "meet", &["meet:abc-defg-hij"])];
        assert!(event_for_call(&far, now, Some("meet"), Some("meet:abc-defg-hij")).is_none());
        // An ad-hoc call with another code isn't the meeting on now…
        assert!(event_for_call(std::slice::from_ref(&sync), now, Some("zoom"), Some("zoom:999888777")).is_none());
        // …a code of another kind says nothing against it (a Teams id and a
        // Teams thread can't be compared)…
        let teams = coded("t", now - 60, now + 1_800, "teams", &["teams:19:meeting_a@thread.v2"]);
        assert_eq!(event_for_call(&[teams], now, Some("teams"), Some("teamsid:312456789012")).unwrap().key, "t");
        // …and a meeting with no codes takes the call by time and service.
        let plain = [event("plain", now - 60, now + 1_800, Some("zoom"))];
        assert_eq!(event_for_call(&plain, now, Some("zoom"), Some("zoom:999888777")).unwrap().key, "plain");
        // A recurring meeting keeps its code: today's occurrence, not last week's.
        let weekly = [
            coded("w:1", now - 7 * 86_400, now - 7 * 86_400 + 1_800, "meet", &["meet:abc-defg-hij"]),
            coded("w:2", now + 60, now + 1_860, "meet", &["meet:abc-defg-hij"]),
        ];
        assert_eq!(event_for_call(&weekly, now, Some("meet"), Some("meet:abc-defg-hij")).unwrap().key, "w:2");
    }

    #[test]
    fn a_call_back_soon_after_is_another_meeting_when_the_calendar_says_so() {
        let now = 10_000;
        // Back to back on Teams: the stand-up ended a minute ago, the 1:1 is on.
        let standup = event("standup", now - 1_860, now - 60, Some("teams"));
        let one_on_one = event("one-on-one", now - 60, now + 1_740, Some("teams"));
        let events = [standup.clone(), one_on_one];
        assert!(another_meeting(&events, now, Some("teams"), None, "standup", standup.end));
        // The stand-up still on (a dropped connection): the same meeting.
        let running = event("standup", now - 1_740, now + 60, Some("teams"));
        assert!(!another_meeting(std::slice::from_ref(&running), now, Some("teams"), None, "standup", running.end));
        // Nothing on the calendar for the call: not another, as before.
        assert!(!another_meeting(&[], now, Some("teams"), None, "standup", now - 60));
        // A join code beats the time: the call shows the stand-up's code.
        let coded_standup = coded("standup", now - 1_860, now - 60, "meet", &["meet:abc-defg-hij"]);
        let next = coded("design", now - 60, now + 1_740, "meet", &["meet:kkk-llll-mmm"]);
        let both = [coded_standup.clone(), next];
        assert!(!another_meeting(&both, now, Some("meet"), Some("meet:abc-defg-hij"), "standup", coded_standup.end));
        assert!(another_meeting(&both, now, Some("meet"), Some("meet:kkk-llll-mmm"), "standup", coded_standup.end));
        // Yesterday's meeting, long over: another, whatever is on now.
        assert!(another_meeting(&[], now + 16 * 3_600, Some("teams"), None, "standup", standup.end));
        assert!(!another_meeting(&[], standup.end + CODE_LATE_SECS, Some("teams"), None, "standup", standup.end));
    }

    #[test]
    fn the_next_meeting_for_the_tray() {
        let now = 50_000;
        // Nothing, or only meetings that are over: none.
        assert!(next_meeting(&[], now, None).is_none());
        assert!(next_meeting(&[event("done", now - 3_600, now - 60, None)], now, None).is_none());
        // The next one to start.
        let later = event("later", now + 7_200, now + 9_000, None);
        let soon = event("soon", now + 1_200, now + 3_000, None);
        assert_eq!(next_meeting(&[later.clone(), soon.clone()], now, None).unwrap().key, "soon");
        // One on now beats one later…
        let on = event("on", now - 600, now + 1_200, None);
        assert_eq!(next_meeting(&[soon.clone(), on.clone()], now, None).unwrap().key, "on");
        // …but one starting within 10 minutes beats it (back to back, or an
        // overlapping 1:1 during a long workshop).
        let in_five = event("in-five", now + 300, now + 2_100, None);
        assert_eq!(next_meeting(&[on.clone(), in_five.clone()], now, None).unwrap().key, "in-five");
        // Two on now (overlapping): the one that started last.
        let block = event("block", now - 3_600, now + 3_600, None);
        let one_on_one = event("one-on-one", now - 300, now + 1_500, None);
        assert_eq!(next_meeting(&[block.clone(), one_on_one.clone()], now, None).unwrap().key, "one-on-one");
        // The meeting being recorded wins while it's on.
        assert_eq!(next_meeting(&[block.clone(), one_on_one, in_five], now, Some("block")).unwrap().key, "block");
        // Two starting together: the shorter, then by key, every time.
        let a = event("a", now + 900, now + 2_700, None);
        let b = event("b", now + 900, now + 1_800, None);
        assert_eq!(next_meeting(&[a, b], now, None).unwrap().key, "b");
    }

    #[test]
    fn all_day_and_declined_events_are_never_the_next_meeting() {
        // Through the feed, as a sync reads it: an all-day offsite and a
        // meeting you declined come before the one that counts.
        let cal = ics::parse_text(
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:imogen@example.com\r\n\
             BEGIN:VEVENT\r\nUID:offsite\r\nSUMMARY:Offsite\r\nDTSTART;VALUE=DATE:20261005\r\nDTEND;VALUE=DATE:20261006\r\n\
             ATTENDEE:mailto:tanay@example.com\r\nATTENDEE:mailto:imogen@example.com\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:vendor\r\nSUMMARY:Vendor demo\r\nDTSTART:20261005T090000Z\r\nDTEND:20261005T093000Z\r\n\
             ATTENDEE:mailto:tanay@example.com\r\nATTENDEE;PARTSTAT=DECLINED:mailto:imogen@example.com\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:review\r\nSUMMARY:Design review\r\nDTSTART:20261005T100000Z\r\nDTEND:20261005T103000Z\r\n\
             ATTENDEE:mailto:tanay@example.com\r\nATTENDEE:mailto:imogen@example.com\r\n\
             LOCATION:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let resolver = super::super::tz::Resolver::new(&cal, super::super::tz::Zone::Utc);
        let me = self_addresses(&cal);
        let events: Vec<Event> = ics::occurrences(&cal, &resolver, 0, i64::MAX / 2)
            .iter()
            .filter_map(|o| finish(draft_from_ics("1", o, &me)))
            .collect();
        assert_eq!(events.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["1:review"]);
        assert_eq!(events[0].codes, ["meet:abc-defg-hij"]);
        let at_8 = 1_791_187_200; // 2026-10-05 08:00 UTC
        assert_eq!(next_meeting(&events, at_8, None).unwrap().title, "Design review");
    }

    #[test]
    fn only_meetings_with_someone_else_or_a_link_pass() {
        let tanay = person("Tanay Kothari", "tanay@example.com");
        let ev = finish(draft(vec![me(Reply::Accepted), tanay.clone()])).unwrap();
        assert_eq!(ev.attendees, [Attendee { name: "Tanay Kothari".into(), email: "tanay@example.com".into() }]);
        // Just you: not a meeting…
        assert!(finish(draft(vec![me(Reply::Accepted)])).is_none());
        // …unless it has a join link.
        let mut solo = draft(vec![]);
        solo.link_sources = vec!["https://zoom.us/j/123".into()];
        let ev = finish(solo).unwrap();
        assert_eq!(ev.service.as_deref(), Some("zoom"));
        assert!(ev.attendees.is_empty());
    }

    #[test]
    fn declined_cancelled_and_long_events_are_dropped() {
        let tanay = person("Tanay", "tanay@example.com");
        assert!(finish(draft(vec![me(Reply::Declined), tanay.clone()])).is_none());
        let mut gone = draft(vec![tanay.clone()]);
        gone.cancelled = true;
        assert!(finish(gone).is_none());
        let mut workshop = draft(vec![tanay.clone()]);
        workshop.end = workshop.start + MAX_LENGTH_SECS + 60;
        assert!(finish(workshop).is_none());
        let mut six = draft(vec![tanay.clone()]);
        six.end = six.start + MAX_LENGTH_SECS;
        assert!(finish(six).is_some());
        // "Maybe" is kept, labelled.
        assert!(finish(draft(vec![me(Reply::Tentative), tanay])).unwrap().tentative);
    }

    #[test]
    fn events_of_your_own_are_kept_as_busy_time() {
        let tanay = person("Tanay", "tanay@example.com");
        assert!(matches!(sort(draft(vec![me(Reply::Accepted), tanay.clone()])), Kept::Meeting(_)));
        // Just you, no link: busy, not a meeting.
        let mut dentist = draft(vec![me(Reply::Accepted)]);
        dentist.title = " Dentist ".into();
        match sort(dentist) {
            Kept::Busy(b) => assert_eq!((b.title.as_str(), b.start, b.end), ("Dentist", 1_000, 4_600)),
            other => panic!("{other:?}"),
        }
        // A day-long workshop with others: too long for a meeting, still busy.
        let mut workshop = draft(vec![tanay.clone()]);
        workshop.end = workshop.start + 8 * 3600;
        assert!(matches!(sort(workshop), Kept::Busy(_)));
        // Cancelled, declined, or longer than a day: neither.
        let mut gone = draft(vec![tanay.clone()]);
        gone.cancelled = true;
        assert_eq!(sort(gone), Kept::Neither);
        assert_eq!(sort(draft(vec![me(Reply::Declined), tanay.clone()])), Kept::Neither);
        let mut week = draft(vec![]);
        week.end = week.start + MAX_BUSY_SECS + 1;
        assert_eq!(sort(week), Kept::Neither);
    }

    #[test]
    fn rooms_and_people_who_declined_are_not_attendees() {
        let room = Person { resource: true, ..person("Room 4", "room4@resource.example.com") };
        let no = Person { reply: Reply::Declined, ..person("Bob", "bob@example.com") };
        let anon = person("", "priya.shah@example.com");
        let ev = finish(draft(vec![room, no, anon, me(Reply::Accepted)])).unwrap();
        assert_eq!(ev.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Priya Shah"]);
    }

    #[test]
    fn names_are_made_from_addresses_when_the_invite_has_none() {
        assert_eq!(display_name("", "tanay.kothari@example.com"), "Tanay Kothari");
        assert_eq!(display_name("priya@example.com", "priya@example.com"), "Priya");
        assert_eq!(display_name("\"Sam Lee\"", "sam@example.com"), "Sam Lee");
        assert_eq!(display_name("", "jo_ann-smith@example.com"), "Jo Ann Smith");
    }

    #[test]
    fn titles_and_with_lines() {
        let mut ev = event("x", 0, 60, None);
        ev.title = String::new();
        ev.attendees = vec![
            Attendee { name: "Tanay Kothari".into(), email: String::new() },
            Attendee { name: "Priya Shah".into(), email: String::new() },
            Attendee { name: "Sam".into(), email: String::new() },
            Attendee { name: "Alex".into(), email: String::new() },
        ];
        assert_eq!(ev.display_title(), "Meeting with Tanay");
        assert_eq!(ev.with_line(), "Tanay Kothari, Priya Shah +2");
    }

    #[test]
    fn the_feeds_owner_is_found() {
        let cal = ics::parse_text(
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Imogen@Example.com\r\n\
             BEGIN:VEVENT\r\nUID:1\r\nATTENDEE:mailto:imogen@example.com\r\nATTENDEE:mailto:a@example.com\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n",
        )
        .unwrap();
        assert!(self_addresses(&cal).contains("imogen@example.com"));
        // No name: whoever is on every meeting.
        let mut body = String::from("BEGIN:VCALENDAR\r\nX-WR-CALNAME:Calendar\r\n");
        for (i, other) in ["a", "b", "c", "d"].iter().enumerate() {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:{i}\r\nATTENDEE:mailto:owner@example.com\r\nATTENDEE:mailto:{other}@example.com\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("END:VCALENDAR\r\n");
        let found = self_addresses(&ics::parse_text(&body).unwrap());
        assert_eq!(found, HashSet::from(["owner@example.com".to_string()]));
        // Every meeting with Sam: Sam and the owner tie, so it's nobody (Sam
        // taken for "you" would leave these meetings with no one else in them).
        let mut body = String::from("BEGIN:VCALENDAR\r\nX-WR-CALNAME:Calendar\r\n");
        for i in 0..4 {
            body.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:{i}\r\nATTENDEE:mailto:owner@example.com\r\nATTENDEE:mailto:sam@example.com\r\nEND:VEVENT\r\n"
            ));
        }
        body.push_str("END:VCALENDAR\r\n");
        assert!(self_addresses(&ics::parse_text(&body).unwrap()).is_empty());
    }

    #[test]
    fn conflicts_are_overlaps() {
        let events = [event("a", 0, 1_800, None), event("b", 1_200, 3_600, None), event("c", 3_600, 4_000, None)];
        let c = conflicts(&events);
        assert!(c.contains("a") && c.contains("b"));
        assert!(!c.contains("c")); // starts as b ends
    }

    #[test]
    fn a_recording_belongs_to_the_meeting_starting_nearest() {
        let now = 10_000;
        let standup = event("standup", now - 1_740, now + 60, Some("teams")); // 14:30–15:00, now 14:59
        let planning = event("planning", now + 60, now + 1_860, Some("teams")); // 15:00
        let events = [standup, planning];
        assert_eq!(event_for_call(&events, now, None, None).unwrap().key, "planning");
        assert_eq!(event_for_call(&events, now, Some("teams"), None).unwrap().key, "planning");
        // A Zoom call doesn't belong to Teams meetings…
        assert!(event_for_call(&events, now, Some("zoom"), None).is_none());
        // …but a meeting without a link takes any call.
        let lunch = [event("lunch", now - 600, now + 600, None)];
        assert_eq!(event_for_call(&lunch, now, Some("zoom"), None).unwrap().key, "lunch");
        // Too early (more than 10 minutes before) or over: nothing.
        assert!(event_for_call(&[event("later", now + 700, now + 2_000, None)], now, None, None).is_none());
        assert!(event_for_call(&[event("done", now - 2_000, now - 1, None)], now, None, None).is_none());
    }

    #[test]
    fn reminders_come_due_and_stay_until_five_minutes_after_the_start() {
        let ev = [event("design", 1_000, 4_600, None)];
        let none = HashSet::new();
        let no_snooze = HashMap::new();
        assert!(due_reminder(&ev, 984, 15, &none, &no_snooze, None).is_none()); // 16 s before
        assert!(due_reminder(&ev, 985, 15, &none, &no_snooze, None).is_some()); // 15 s before
        assert!(due_reminder(&ev, 1_299, 15, &none, &no_snooze, None).is_some());
        assert!(due_reminder(&ev, 1_300, 15, &none, &no_snooze, None).is_none()); // +5 min
        // Answered, snoozed or being recorded: no reminder.
        let answered = HashSet::from([answer_key(&ev[0])]);
        assert!(due_reminder(&ev, 990, 15, &answered, &no_snooze, None).is_none());
        let snoozed = HashMap::from([(answer_key(&ev[0]), 1_100)]);
        assert!(due_reminder(&ev, 990, 15, &none, &snoozed, None).is_none());
        assert!(due_reminder(&ev, 1_100, 15, &none, &snoozed, None).is_some());
        assert!(due_reminder(&ev, 990, 15, &none, &no_snooze, Some("design")).is_none());
        assert_eq!(next_reminder_at(&ev, 900, 15, &snoozed), Some(985));
    }

    #[test]
    fn placeholder_titles_are_recognised() {
        for t in [
            "",
            "  ",
            "Untitled note",
            "Teams call \u{b7} 5 Oct, 14:30",
            "Slack huddle \u{b7} 12 Jan, 09:05",
            "Meeting \u{b7} 5 Oct, 14:30",
        ] {
            assert!(is_placeholder_title(t), "{t:?}");
        }
        for t in ["Design review", "Teams call notes", "Call \u{b7} with Sam", "Zoom call \u{b7} 5 October, 14:30"] {
            assert!(!is_placeholder_title(t), "{t:?}");
        }
    }

    #[test]
    fn ics_occurrences_become_events() {
        let cal = ics::parse_text(
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:imogen@example.com\r\nBEGIN:VEVENT\r\nUID:dr-1\r\n\
             SUMMARY:Design review\r\nDTSTART:20261005T140000Z\r\nDTEND:20261005T143000Z\r\n\
             ORGANIZER;CN=Tanay Kothari:mailto:tanay@example.com\r\n\
             ATTENDEE;CN=Tanay Kothari;PARTSTAT=ACCEPTED:mailto:tanay@example.com\r\n\
             ATTENDEE;CN=Priya Shah;PARTSTAT=NEEDS-ACTION:mailto:priya@example.com\r\n\
             ATTENDEE;CN=Imogen;PARTSTAT=TENTATIVE:mailto:imogen@example.com\r\n\
             ATTENDEE;CUTYPE=ROOM;CN=Room 4:mailto:room4@example.com\r\n\
             DESCRIPTION:Agenda: the Q4 plan\\n\\nJoin: https://teams.microsoft.com/l/meetup-join/19%3a1\r\n\
             END:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let resolver = super::super::tz::Resolver::new(&cal, super::super::tz::Zone::Utc);
        let me = self_addresses(&cal);
        let occ = ics::occurrences(&cal, &resolver, 0, i64::MAX / 2);
        let ev = finish(draft_from_ics("2", &occ[0], &me)).unwrap();
        assert_eq!(ev.key, "2:dr-1");
        assert_eq!(ev.title, "Design review");
        assert_eq!(ev.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Tanay Kothari", "Priya Shah"]);
        assert_eq!(ev.organizer, "Tanay Kothari");
        assert!(ev.tentative);
        assert_eq!(ev.service.as_deref(), Some("teams"));
        assert_eq!(ev.join_url.as_deref(), Some("https://teams.microsoft.com/l/meetup-join/19%3a1"));
        assert_eq!(ev.description, "Agenda: the Q4 plan");
    }
}
