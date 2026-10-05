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
    })
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
/// an address (Google's secret iCal address is named after its owner), and
/// anyone invited to (nearly) every meeting in it, as the feed's owner is.
pub fn self_addresses(calendar: &Component) -> HashSet<String> {
    let mut out = HashSet::new();
    if let Some(name) = ics::calendar_name(calendar).filter(|n| is_address(n)) {
        out.insert(name.to_ascii_lowercase());
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
        out.extend(counts.into_iter().filter(|(_, n)| *n * 10 >= meetings * 9).map(|(a, _)| a));
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

/// The event a recording or a call at `now` belongs to: one that's on or
/// about to start, on `service` when a call's app is known (events with no
/// join link fit any call), the one starting nearest `now` (back-to-back
/// meetings: at 14:58 a recording is for the 15:00 one, not the 14:30 one).
pub fn event_at<'a>(events: &'a [Event], now: i64, service: Option<&str>) -> Option<&'a Event> {
    events
        .iter()
        .filter(|e| is_current(e, now))
        .filter(|e| match (service, e.service.as_deref()) {
            (Some(call), Some(link)) => call == link,
            _ => true,
        })
        .min_by_key(|e| ((e.start - now).abs(), e.start))
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
        Person { is_self: true, reply, ..person("Nathan", "nathan@example.com") }
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
        }
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
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Nathan@Example.com\r\n\
             BEGIN:VEVENT\r\nUID:1\r\nATTENDEE:mailto:nathan@example.com\r\nATTENDEE:mailto:a@example.com\r\nEND:VEVENT\r\n\
             END:VCALENDAR\r\n",
        )
        .unwrap();
        assert!(self_addresses(&cal).contains("nathan@example.com"));
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
        assert_eq!(event_at(&events, now, None).unwrap().key, "planning");
        assert_eq!(event_at(&events, now, Some("teams")).unwrap().key, "planning");
        // A Zoom call doesn't belong to Teams meetings…
        assert!(event_at(&events, now, Some("zoom")).is_none());
        // …but a meeting without a link takes any call.
        let lunch = [event("lunch", now - 600, now + 600, None)];
        assert_eq!(event_at(&lunch, now, Some("zoom")).unwrap().key, "lunch");
        // Too early (more than 10 minutes before) or over: nothing.
        assert!(event_at(&[event("later", now + 700, now + 2_000, None)], now, None).is_none());
        assert!(event_at(&[event("done", now - 2_000, now - 1, None)], now, None).is_none());
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
            "BEGIN:VCALENDAR\r\nX-WR-CALNAME:nathan@example.com\r\nBEGIN:VEVENT\r\nUID:dr-1\r\n\
             SUMMARY:Design review\r\nDTSTART:20261005T140000Z\r\nDTEND:20261005T143000Z\r\n\
             ORGANIZER;CN=Tanay Kothari:mailto:tanay@example.com\r\n\
             ATTENDEE;CN=Tanay Kothari;PARTSTAT=ACCEPTED:mailto:tanay@example.com\r\n\
             ATTENDEE;CN=Priya Shah;PARTSTAT=NEEDS-ACTION:mailto:priya@example.com\r\n\
             ATTENDEE;CN=Nathan;PARTSTAT=TENTATIVE:mailto:nathan@example.com\r\n\
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
