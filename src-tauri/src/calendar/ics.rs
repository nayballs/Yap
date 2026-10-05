//! iCalendar (RFC 5545) feeds, the "private iCal address" kind: a lenient
//! parser for the text, and the events' occurrences in a window, with
//! recurrence expanded and every time resolved to UTC.
//!
//! **Parsing** is Yap's own, and forgiving on purpose: lines are unfolded
//! byte-wise (folds can split a UTF-8 character), a malformed line or a
//! missing END is skipped rather than failing the feed, so one odd line in
//! someone's calendar never hides the rest of it.
//!
//! **Recurrence** (RRULE) comes from the `rrule` crate, expanded on
//! wall-clock time (UTC standing in for "no zone"), then each occurrence is
//! read in the event's own zone ([`super::tz`]): a weekly 09:00 meeting stays
//! at 09:00 across a daylight-saving change, as calendars show it. EXDATEs
//! remove occurrences, RDATEs add them, and a RECURRENCE-ID override
//! replaces the occurrence it names (or cancels it). DTSTART always counts
//! as an occurrence (RFC 5545 §3.8.5.3).

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use super::tz::{Resolver, Zone};

/// One content line: `NAME;PARAM=value:VALUE`.
#[derive(Debug, Clone)]
pub struct Prop {
    /// Upper-cased.
    pub name: String,
    /// Names upper-cased, values unquoted (RFC 6868 `^` escapes decoded).
    pub params: Vec<(String, String)>,
    /// As written (TEXT escapes still in: see [`Component::text`]).
    pub value: String,
}

impl Prop {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// A BEGIN…END block (VCALENDAR, VEVENT, VTIMEZONE, …).
#[derive(Debug, Clone, Default)]
pub struct Component {
    /// Upper-cased.
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.prop(name).map(|p| p.value.as_str())
    }

    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }

    /// A TEXT property, unescaped and trimmed.
    pub fn text(&self, name: &str) -> Option<String> {
        self.value(name).map(|v| unescape_text(v).trim().to_string())
    }
}

/// Parse a feed. `None` unless it's an iCalendar file (a VCALENDAR).
pub fn parse_bytes(bytes: &[u8]) -> Option<Component> {
    let unfolded = unfold(bytes);
    let text = String::from_utf8_lossy(&unfolded);
    build(text.trim_start_matches('\u{feff}').lines())
}

/// [`parse_bytes`] for a string.
#[cfg(test)]
pub fn parse_text(text: &str) -> Option<Component> {
    parse_bytes(text.as_bytes())
}

/// RFC 5545 §3.1: a line break followed by a space or tab continues the
/// line. Done on bytes: a fold may fall inside a multi-byte character.
fn unfold(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let next_is_space = |at: usize| matches!(bytes.get(at), Some(b' ' | b'\t'));
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') && next_is_space(i + 2) {
            i += 3;
        } else if bytes[i] == b'\n' && next_is_space(i + 1) {
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn build<'a>(lines: impl Iterator<Item = &'a str>) -> Option<Component> {
    let mut stack: Vec<Component> = Vec::new();
    let mut root: Option<Component> = None;
    let mut close = |done: Component, stack: &mut Vec<Component>| match stack.last_mut() {
        Some(parent) => parent.children.push(done),
        None => {
            if root.is_none() {
                root = Some(done);
            }
        }
    };
    for line in lines {
        let Some(prop) = parse_line(line.trim_end_matches('\r')) else { continue };
        match prop.name.as_str() {
            "BEGIN" => {
                let name = prop.value.trim().to_ascii_uppercase();
                // No component nests inside its own kind: a BEGIN:VEVENT
                // inside a VEVENT means the first one's END went missing.
                if stack.last().is_some_and(|c| c.name == name) {
                    if let Some(done) = stack.pop() {
                        close(done, &mut stack);
                    }
                }
                stack.push(Component { name, ..Default::default() });
            }
            "END" => {
                let name = prop.value.trim().to_ascii_uppercase();
                if !stack.iter().any(|c| c.name == name) {
                    continue; // an END with no BEGIN: ignore it
                }
                // Close everything up to the matching BEGIN (a missing END
                // inside closes with it).
                while let Some(done) = stack.pop() {
                    let matched = done.name == name;
                    close(done, &mut stack);
                    if matched {
                        break;
                    }
                }
            }
            _ => {
                if let Some(current) = stack.last_mut() {
                    current.props.push(prop);
                }
            }
        }
    }
    while let Some(done) = stack.pop() {
        close(done, &mut stack);
    }
    root.filter(|r| r.name == "VCALENDAR")
}

/// `NAME;P1=a,"b:c";P2=d:value`. `None` for a line that isn't one.
fn parse_line(line: &str) -> Option<Prop> {
    let name_end = line.find([';', ':'])?;
    let name = line[..name_end].trim().to_ascii_uppercase();
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    let bytes = line.as_bytes();
    let mut params = Vec::new();
    let mut at = name_end;
    while bytes.get(at) == Some(&b';') {
        let rest = &line[at + 1..];
        let eq = rest.find('=')?;
        let pname = rest[..eq].trim().to_ascii_uppercase();
        let mut j = at + 1 + eq + 1;
        let mut value = String::new();
        loop {
            if bytes.get(j) == Some(&b'"') {
                let close = j + 1 + line[j + 1..].find('"')?;
                value.push_str(&line[j + 1..close]);
                j = close + 1;
            } else {
                let end = line[j..].find([',', ';', ':']).map_or(line.len(), |k| j + k);
                value.push_str(&line[j..end]);
                j = end;
            }
            if bytes.get(j) == Some(&b',') {
                value.push(',');
                j += 1;
            } else {
                break;
            }
        }
        params.push((pname, caret_decode(&value)));
        at = j;
    }
    if bytes.get(at) != Some(&b':') {
        return None;
    }
    Some(Prop { name, params, value: line[at + 1..].to_string() })
}

/// RFC 6868 parameter escapes: `^n` newline, `^'` double quote, `^^` caret.
fn caret_decode(s: &str) -> String {
    if !s.contains('^') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\'') => out.push('"'),
            Some('^') => out.push('^'),
            Some(other) => {
                out.push('^');
                out.push(other);
            }
            None => out.push('^'),
        }
    }
    out
}

/// RFC 5545 TEXT escapes: `\n` newline, `\,` `\;` `\\` themselves.
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `20261005` → a date.
pub fn parse_date(v: &str) -> Option<NaiveDate> {
    let v = v.trim();
    if v.len() != 8 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDate::from_ymd_opt(v[0..4].parse().ok()?, v[4..6].parse().ok()?, v[6..8].parse().ok()?)
}

/// `20261005T143000` (floating or zoned) or `…Z` (UTC) → (wall-clock time,
/// is UTC). Plain dates aren't date-times: `None`.
pub fn parse_date_time(v: &str) -> Option<(NaiveDateTime, bool)> {
    let v = v.trim();
    let (body, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(body) => (body, true),
        None => (v, false),
    };
    let (date, time) = body.split_once(['T', 't'])?;
    let date = parse_date(date)?;
    if !(time.len() == 4 || time.len() == 6) || !time.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |i: usize| time[i..i + 2].parse::<u32>().ok();
    let seconds = if time.len() == 6 { n(4)?.min(59) } else { 0 };
    Some((date.and_time(NaiveTime::from_hms_opt(n(0)?, n(2)?, seconds)?), utc))
}

/// An RFC 5545 DURATION (`PT1H30M`, `P1D`, `P2W`, `-PT15M`) in seconds.
pub fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim();
    let (sign, v) = match v.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, v.strip_prefix('+').unwrap_or(v)),
    };
    let mut rest = v.strip_prefix(['P', 'p'])?;
    let mut total = 0i64;
    let mut in_time = false;
    let mut any = false;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix(['T', 't']) {
            in_time = true;
            rest = r;
            continue;
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let n: i64 = rest[..digits].parse().ok()?;
        let unit = rest[digits..].chars().next()?.to_ascii_uppercase();
        total += n * match (unit, in_time) {
            ('W', false) => 7 * 86_400,
            ('D', false) => 86_400,
            ('H', true) => 3_600,
            ('M', true) => 60,
            ('S', true) => 1,
            _ => return None,
        };
        any = true;
        rest = &rest[digits + 1..];
    }
    any.then_some(sign * total)
}

/// When something happens: a whole day, or a wall-clock time in a zone.
#[derive(Debug, Clone)]
pub enum Moment {
    Date(NaiveDate),
    At { local: NaiveDateTime, zone: Zone },
}

impl Moment {
    /// The instant, for a date-time.
    pub fn utc(&self) -> Option<i64> {
        match self {
            Moment::At { local, zone } => Some(zone.to_utc(*local)),
            Moment::Date(_) => None,
        }
    }
}

/// One value of a date or date-time property (`VALUE=DATE`, `TZID=`, `Z`).
pub fn moment(prop: &Prop, value: &str, resolver: &Resolver) -> Option<Moment> {
    let value = value.trim();
    // PERIOD values (RDATE): their start.
    let value = value.split('/').next().unwrap_or(value);
    let is_date = prop.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"))
        || (value.len() == 8 && !value.contains(['T', 't']));
    if is_date {
        return parse_date(value).map(Moment::Date);
    }
    let (local, utc) = parse_date_time(value)?;
    let zone = if utc { Zone::Utc } else { resolver.zone(prop.param("TZID")) };
    Some(Moment::At { local, zone })
}

/// RRULE parts the `rrule` crate understands. Others (`X-…`, typos) are
/// dropped, so one odd part doesn't lose a whole series; UNTIL is handled
/// apart (see [`expand_floating`]).
const RULE_PARTS: [&str; 13] = [
    "FREQ", "COUNT", "INTERVAL", "BYSECOND", "BYMINUTE", "BYHOUR", "BYDAY", "BYMONTHDAY",
    "BYYEARDAY", "BYWEEKNO", "BYMONTH", "BYSETPOS", "WKST",
];

/// An RRULE's UNTIL value, as written.
pub fn rule_until(rule: &str) -> Option<&str> {
    rule.split(';').find_map(|part| {
        let (k, v) = part.split_once('=')?;
        k.trim().eq_ignore_ascii_case("UNTIL").then(|| v.trim())
    })
}

/// Iterations of a recurrence before giving up (a daily rule from decades
/// ago is a few thousand; this only stops nonsense).
const MAX_ITERATIONS: usize = 100_000;

/// The occurrences of `rule` from `start`, on wall-clock time: those in
/// `[from, stop)`, at most `max`. `until` is the rule's UNTIL already turned
/// into wall-clock time by the caller (it knows the zone); `None` reads the
/// rule's own UNTIL as wall-clock time (VTIMEZONE observances, where the
/// difference never matters for the years Yap looks at). Nothing for a rule
/// the `rrule` crate rejects, or one repeating every hour or faster (no
/// meeting does, and they are costly to expand).
pub fn expand_floating(
    start: NaiveDateTime,
    rule: &str,
    until: Option<Option<NaiveDateTime>>,
    from: NaiveDateTime,
    stop: NaiveDateTime,
    max: usize,
) -> Vec<NaiveDateTime> {
    use rrule::{Frequency, RRule, RRuleSet, Unvalidated};
    let cleaned: Vec<String> = rule
        .split(';')
        .filter_map(|part| {
            let (k, v) = part.split_once('=')?;
            let k = k.trim().to_ascii_uppercase();
            RULE_PARTS.contains(&k.as_str()).then(|| format!("{k}={}", v.trim()))
        })
        .collect();
    let Ok(parsed) = cleaned.join(";").parse::<RRule<Unvalidated>>() else {
        return Vec::new();
    };
    if matches!(parsed.get_freq(), Frequency::Secondly | Frequency::Minutely | Frequency::Hourly) {
        return Vec::new();
    }
    let until = match until {
        Some(given) => given,
        None => rule_until(rule).and_then(|u| until_as_wall_clock(u, &Zone::Utc)),
    };
    let floating = |t: &NaiveDateTime| rrule::Tz::UTC.from_utc_datetime(t);
    let dt_start = floating(&start);
    let parsed = match until {
        Some(until) if until < start => return Vec::new(),
        Some(until) => parsed.until(floating(&until)),
        None => parsed,
    };
    let Ok(valid) = parsed.validate(dt_start) else {
        return Vec::new();
    };
    let set = RRuleSet::new(dt_start).rrule(valid).limit();
    let mut out = Vec::new();
    for (n, occurrence) in (&set).into_iter().enumerate() {
        let at = occurrence.naive_utc();
        if at >= stop || out.len() >= max || n >= MAX_ITERATIONS {
            break;
        }
        if at >= from {
            out.push(at);
        }
    }
    out
}

/// An RRULE's UNTIL as wall-clock time in the event's `zone`: a UTC value
/// (`…Z`, what RFC 5545 asks for with a zoned DTSTART) converted, a
/// floating one taken as is, a date-only one as the end of that day.
pub fn until_as_wall_clock(until: &str, zone: &Zone) -> Option<NaiveDateTime> {
    if let Some(date) = parse_date(until) {
        return date.and_hms_opt(23, 59, 59);
    }
    let (at, utc) = parse_date_time(until)?;
    Some(if utc { zone.to_local(at.and_utc().timestamp()) } else { at })
}

/// One occurrence of an event within the window.
#[derive(Debug)]
pub struct Occurrence<'a> {
    pub uid: String,
    /// A recurring event's occurrence: its original start (what a
    /// RECURRENCE-ID names), unix seconds. `None` for a one-off.
    pub recurrence_id: Option<i64>,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    /// Where its details come from: the series, or the override that moved
    /// or changed this one occurrence.
    pub source: &'a Component,
}

fn cancelled(event: &Component) -> bool {
    event.value("STATUS").is_some_and(|s| s.trim().eq_ignore_ascii_case("CANCELLED"))
}

/// An event's start (a date-time; all-day events are `None`) and length.
fn start_and_length(event: &Component, resolver: &Resolver, fallback_len: i64) -> Option<(NaiveDateTime, Zone, i64, i64)> {
    let prop = event.prop("DTSTART")?;
    let Moment::At { local, zone } = moment(prop, &prop.value, resolver)? else {
        return None; // all-day
    };
    let start = zone.to_utc(local);
    let length = match event.prop("DTEND").and_then(|p| moment(p, &p.value, resolver)) {
        Some(end) => end.utc().map(|e| e - start),
        None => event.value("DURATION").and_then(parse_duration),
    }
    .unwrap_or(fallback_len)
    .max(0);
    Some((local, zone, start, length))
}

/// Every occurrence of `calendar`'s events overlapping `[from, to)` (unix
/// seconds). All-day events and cancelled ones are left out here already.
pub fn occurrences<'a>(calendar: &'a Component, resolver: &Resolver, from: i64, to: i64) -> Vec<Occurrence<'a>> {
    let events: Vec<&Component> = calendar.children.iter().filter(|c| c.name == "VEVENT").collect();
    let uid_of = |e: &Component| e.text("UID").filter(|u| !u.is_empty());

    // Overrides of single occurrences, by series: (original start, override).
    let mut overrides: HashMap<String, Vec<(i64, &Component)>> = HashMap::new();
    for event in &events {
        let (Some(uid), Some(rid)) = (uid_of(event), event.prop("RECURRENCE-ID")) else { continue };
        if let Some(original) = moment(rid, &rid.value, resolver).and_then(|m| m.utc()) {
            overrides.entry(uid).or_default().push((original, event));
        }
    }

    let overlaps = |start: i64, end: i64| start < to && (end > from || (end == start && start >= from));
    let mut out = Vec::new();
    let mut used: HashSet<(String, i64)> = HashSet::new();
    let mut series_lengths: HashMap<String, i64> = HashMap::new();

    for (index, event) in events.iter().enumerate() {
        if event.prop("RECURRENCE-ID").is_some() {
            continue;
        }
        // A UID is required; a feed without one still gets a stable stand-in.
        let uid = uid_of(event).unwrap_or_else(|| format!("no-uid-{index}"));
        let Some((local, zone, start, length)) = start_and_length(event, resolver, 0) else { continue };
        series_lengths.insert(uid.clone(), length);
        if cancelled(event) {
            // The whole series is off, overrides included.
            if let Some(list) = overrides.get(&uid) {
                used.extend(list.iter().map(|(rid, _)| (uid.clone(), *rid)));
            }
            continue;
        }
        let rule = event.value("RRULE");
        let rdates: Vec<i64> = event
            .props_named("RDATE")
            .flat_map(|p| p.value.split(',').filter_map(move |v| moment(p, v, resolver)?.utc()))
            .collect();
        let recurring = rule.is_some() || !rdates.is_empty();
        if !recurring && start > to {
            continue; // the quick way past old and far-future one-offs
        }

        let mut starts: Vec<i64> = Vec::new();
        if let Some(rule) = rule {
            let until = Some(rule_until(rule).and_then(|u| until_as_wall_clock(u, &zone)));
            let window_from = zone.to_local(from - length - 86_400);
            let window_to = zone.to_local(to + 86_400);
            starts.extend(
                expand_floating(local, rule, until, window_from, window_to, 1_000)
                    .into_iter()
                    .map(|wall| zone.to_utc(wall)),
            );
        }
        // DTSTART is always an occurrence, synchronised with the rule or not.
        if !starts.contains(&start) {
            starts.push(start);
        }
        starts.extend(rdates);
        starts.sort_unstable();
        starts.dedup();

        // EXDATE: exact instants, or whole days for a date-only value.
        let mut excluded: HashSet<i64> = HashSet::new();
        let mut excluded_days: HashSet<NaiveDate> = HashSet::new();
        for prop in event.props_named("EXDATE") {
            for value in prop.value.split(',') {
                match moment(prop, value, resolver) {
                    Some(Moment::At { local, zone }) => {
                        excluded.insert(zone.to_utc(local));
                    }
                    Some(Moment::Date(day)) => {
                        excluded_days.insert(day);
                    }
                    None => {}
                }
            }
        }

        for occurrence in starts {
            if excluded.contains(&occurrence) || excluded_days.contains(&zone.to_local(occurrence).date()) {
                continue;
            }
            let rid = recurring.then_some(occurrence);
            let replaced = rid.and_then(|rid| {
                overrides.get(&uid).and_then(|list| list.iter().find(|(original, _)| *original == rid))
            });
            if let Some((original, changed)) = replaced {
                used.insert((uid.clone(), *original));
                if cancelled(changed) {
                    continue;
                }
                if let Some((_, _, s, len)) = start_and_length(changed, resolver, length) {
                    if overlaps(s, s + len) {
                        out.push(Occurrence { uid: uid.clone(), recurrence_id: rid, start: s, end: s + len, source: changed });
                    }
                }
                continue;
            }
            if overlaps(occurrence, occurrence + length) {
                out.push(Occurrence {
                    uid: uid.clone(),
                    recurrence_id: rid,
                    start: occurrence,
                    end: occurrence + length,
                    source: event,
                });
            }
        }
    }

    // Overrides whose occurrence wasn't generated here: moved into the
    // window from outside it, or a series the feed doesn't include.
    for (uid, list) in &overrides {
        for (original, changed) in list {
            if used.contains(&(uid.clone(), *original)) || cancelled(changed) {
                continue;
            }
            let fallback = series_lengths.get(uid).copied().unwrap_or(0);
            if let Some((_, _, s, len)) = start_and_length(changed, resolver, fallback) {
                if overlaps(s, s + len) {
                    out.push(Occurrence { uid: uid.clone(), recurrence_id: Some(*original), start: s, end: s + len, source: changed });
                }
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.uid.cmp(&b.uid)));
    out
}

/// Whole-day events in the window, for the record (Yap leaves them out of
/// meetings; tests check that they're seen and skipped).
#[cfg(test)]
pub fn all_day_count(calendar: &Component) -> usize {
    calendar
        .children
        .iter()
        .filter(|c| c.name == "VEVENT")
        .filter(|c| c.prop("DTSTART").is_some_and(|p| {
            p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || p.value.trim().len() == 8
        }))
        .count()
}

/// The calendar's name (`X-WR-CALNAME`), if it gives one.
pub fn calendar_name(calendar: &Component) -> Option<String> {
    calendar.text("X-WR-CALNAME").filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap().and_utc().timestamp()
    }

    fn feed(body: &str) -> Component {
        let text = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Yap tests//EN\r\n{body}END:VCALENDAR\r\n");
        parse_text(&text).expect("a calendar")
    }

    fn starts(cal: &Component, from: i64, to: i64) -> Vec<(String, i64)> {
        let resolver = Resolver::new(cal, Zone::Utc);
        occurrences(cal, &resolver, from, to)
            .into_iter()
            .map(|o| (o.source.text("SUMMARY").unwrap_or_default(), o.start))
            .collect()
    }

    #[test]
    fn lines_unfold_and_parameters_parse() {
        let mut bytes = b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Planning\\, part 2\\n(budget)\r\n\
             ATTENDEE;CN=\"Kothari, Tanay\";PARTSTAT=ACCEPTED;ROLE=REQ-PARTICIPANT:mai\r\n lto:tanay@example.com\r\n\
             DESCRIPTION:caf"
            .to_vec();
        // "é" is C3 A9; the fold falls between its two bytes.
        bytes.extend_from_slice(b"\xC3\r\n \xA9 menu\r\nDTSTART:20261005T140000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let cal = parse_bytes(&bytes).unwrap();
        let event = &cal.children[0];
        assert_eq!(event.text("SUMMARY").unwrap(), "Planning, part 2\n(budget)");
        let attendee = event.prop("ATTENDEE").unwrap();
        assert_eq!(attendee.param("CN"), Some("Kothari, Tanay"));
        assert_eq!(attendee.param("PARTSTAT"), Some("ACCEPTED"));
        assert_eq!(attendee.value, "mailto:tanay@example.com");
        // A fold inside a two-byte character still reads as "café".
        assert_eq!(event.text("DESCRIPTION").unwrap(), "café menu");
    }

    #[test]
    fn broken_lines_and_missing_ends_are_survived() {
        let text = "BEGIN:VCALENDAR\nthis is not a content line\nBEGIN:VEVENT\nUID:a\nSUMMARY:First\n\
                    DTSTART:20261005T090000Z\nBEGIN:VEVENT\nUID:b\nSUMMARY:Second\nDTSTART:20261005T100000Z\n\
                    END:VEVENT\nEND:NOTHING\nEND:VCALENDAR";
        let cal = parse_text(text).unwrap();
        let got = starts(&cal, utc(2026, 10, 5, 0, 0), utc(2026, 10, 6, 0, 0));
        assert_eq!(got.len(), 2);
        assert!(parse_text("<html>not a calendar</html>").is_none());
    }

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration("PT1H30M"), Some(5_400));
        assert_eq!(parse_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("P2W"), Some(1_209_600));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("PT"), None);
        assert_eq!(parse_duration("1H"), None);
    }

    const WEEKLY: &str = "BEGIN:VEVENT\r\nUID:weekly@yap\r\nSUMMARY:Weekly sync\r\n\
        DTSTART;TZID=Europe/London:20260907T093000\r\nDTEND;TZID=Europe/London:20260907T100000\r\n\
        RRULE:FREQ=WEEKLY;BYDAY=MO;UNTIL=20261130T093000Z\r\n\
        EXDATE;TZID=Europe/London:20261012T093000\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:weekly@yap\r\nRECURRENCE-ID;TZID=Europe/London:20261019T093000\r\n\
        SUMMARY:Weekly sync (moved)\r\nDTSTART;TZID=Europe/London:20261020T150000\r\n\
        DTEND;TZID=Europe/London:20261020T153000\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:weekly@yap\r\nRECURRENCE-ID;TZID=Europe/London:20261026T093000\r\n\
        SUMMARY:Weekly sync\r\nSTATUS:CANCELLED\r\nDTSTART;TZID=Europe/London:20261026T093000\r\nEND:VEVENT\r\n";

    #[test]
    fn weekly_series_with_exdate_moved_and_cancelled_occurrences() {
        let cal = feed(WEEKLY);
        let got = starts(&cal, utc(2026, 10, 1, 0, 0), utc(2026, 11, 10, 0, 0));
        assert_eq!(
            got,
            [
                ("Weekly sync".to_string(), utc(2026, 10, 5, 8, 30)),        // BST: 09:30 = 08:30 UTC
                ("Weekly sync (moved)".to_string(), utc(2026, 10, 20, 14, 0)), // the override's own time
                ("Weekly sync".to_string(), utc(2026, 11, 2, 9, 30)),        // GMT after 25 Oct: 09:30 UTC
                ("Weekly sync".to_string(), utc(2026, 11, 9, 9, 30)),
            ]
        );
        // 12 Oct is EXDATE'd, 26 Oct cancelled; the moved one ends 30 min later.
        let resolver = Resolver::new(&cal, Zone::Utc);
        let moved = occurrences(&cal, &resolver, utc(2026, 10, 20, 0, 0), utc(2026, 10, 21, 0, 0));
        assert_eq!(moved[0].end - moved[0].start, 1_800);
        assert_eq!(moved[0].recurrence_id, Some(utc(2026, 10, 19, 8, 30)));
    }

    #[test]
    fn until_ends_the_series_and_count_limits_it() {
        let cal = feed(WEEKLY);
        // UNTIL 30 Nov: nothing in December.
        assert!(starts(&cal, utc(2026, 12, 1, 0, 0), utc(2026, 12, 31, 0, 0)).is_empty());
        let counted = feed(
            "BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Daily\r\nDTSTART:20261005T080000Z\r\nDURATION:PT15M\r\n\
             RRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\n",
        );
        let got = starts(&counted, utc(2026, 10, 1, 0, 0), utc(2026, 10, 31, 0, 0));
        assert_eq!(got.iter().map(|(_, s)| *s).collect::<Vec<_>>(), [
            utc(2026, 10, 5, 8, 0),
            utc(2026, 10, 6, 8, 0),
            utc(2026, 10, 7, 8, 0),
        ]);
    }

    #[test]
    fn a_date_only_until_and_an_old_series_still_reach_today() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Standup\r\nDTSTART;TZID=America/New_York:20200106T090000\r\n\
             DTEND;TZID=America/New_York:20200106T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20261231\r\n\
             END:VEVENT\r\n",
        );
        // Monday 5 Oct 2026 09:00 EDT = 13:00 UTC; Wednesday 7th too.
        let got = starts(&cal, utc(2026, 10, 5, 0, 0), utc(2026, 10, 8, 0, 0));
        assert_eq!(got.iter().map(|(_, s)| *s).collect::<Vec<_>>(), [
            utc(2026, 10, 5, 13, 0),
            utc(2026, 10, 7, 13, 0),
        ]);
    }

    #[test]
    fn a_series_keeps_its_wall_clock_time_across_daylight_saving() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:e\r\nSUMMARY:Review\r\nDTSTART;TZID=Europe/Berlin:20261020T100000\r\n\
             DURATION:PT1H\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\n",
        );
        let got = starts(&cal, utc(2026, 10, 19, 0, 0), utc(2026, 11, 1, 0, 0));
        // 10:00 CEST (08:00 UTC), then 10:00 CET (09:00 UTC) after 25 Oct.
        assert_eq!(got.iter().map(|(_, s)| *s).collect::<Vec<_>>(), [
            utc(2026, 10, 20, 8, 0),
            utc(2026, 10, 27, 9, 0),
        ]);
    }

    #[test]
    fn windows_zone_names_and_custom_vtimezones_resolve() {
        let cal = feed(
            "BEGIN:VTIMEZONE\r\nTZID:Customized Time Zone\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\n\
             TZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\n\
             BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
             RRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
             BEGIN:VEVENT\r\nUID:f\r\nSUMMARY:Outlook\r\nDTSTART;TZID=GMT Standard Time:20261005T143000\r\n\
             DTEND;TZID=GMT Standard Time:20261005T150000\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:g\r\nSUMMARY:Custom\r\nDTSTART;TZID=Customized Time Zone:20261005T143000\r\n\
             DTEND;TZID=Customized Time Zone:20261005T150000\r\nEND:VEVENT\r\n",
        );
        let got = starts(&cal, utc(2026, 10, 5, 0, 0), utc(2026, 10, 6, 0, 0));
        assert_eq!(got, [
            ("Custom".to_string(), utc(2026, 10, 5, 12, 30)),  // +02:00
            ("Outlook".to_string(), utc(2026, 10, 5, 13, 30)), // BST
        ]);
    }

    #[test]
    fn floating_times_use_the_pcs_zone_and_all_day_events_are_skipped() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:h\r\nSUMMARY:Floating\r\nDTSTART:20261005T140000\r\nDTEND:20261005T143000\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:i\r\nSUMMARY:Offsite\r\nDTSTART;VALUE=DATE:20261005\r\nDTEND;VALUE=DATE:20261006\r\nEND:VEVENT\r\n",
        );
        let resolver = Resolver::new(&cal, Zone::Fixed(2 * 3600));
        let got = occurrences(&cal, &resolver, utc(2026, 10, 5, 0, 0), utc(2026, 10, 6, 0, 0));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].start, utc(2026, 10, 5, 12, 0));
        assert_eq!(all_day_count(&cal), 1);
    }

    #[test]
    fn an_override_moved_into_the_window_from_outside_it_shows() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:j\r\nSUMMARY:Monthly\r\nDTSTART:20260901T100000Z\r\nDURATION:PT1H\r\n\
             RRULE:FREQ=MONTHLY\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:j\r\nRECURRENCE-ID:20261101T100000Z\r\nSUMMARY:Monthly (early)\r\n\
             DTSTART:20261015T100000Z\r\nDURATION:PT1H\r\nEND:VEVENT\r\n",
        );
        let got = starts(&cal, utc(2026, 10, 10, 0, 0), utc(2026, 10, 20, 0, 0));
        assert_eq!(got, [("Monthly (early)".to_string(), utc(2026, 10, 15, 10, 0))]);
        // …and the original 1 Nov occurrence is gone.
        assert!(starts(&cal, utc(2026, 10, 31, 0, 0), utc(2026, 11, 2, 0, 0)).is_empty());
    }

    #[test]
    fn a_cancelled_series_and_unknown_rule_parts() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:k\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nDTSTART:20261005T100000Z\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:l\r\nSUMMARY:Odd rule\r\nDTSTART:20261005T110000Z\r\nDURATION:PT30M\r\n\
             RRULE:FREQ=DAILY;X-NAME=whatever;COUNT=2\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:m\r\nSUMMARY:Too often\r\nDTSTART:20261005T120000Z\r\nRRULE:FREQ=MINUTELY\r\nEND:VEVENT\r\n",
        );
        let got = starts(&cal, utc(2026, 10, 5, 0, 0), utc(2026, 10, 7, 0, 0));
        assert_eq!(got, [
            ("Odd rule".to_string(), utc(2026, 10, 5, 11, 0)),
            ("Too often".to_string(), utc(2026, 10, 5, 12, 0)), // DTSTART only
            ("Odd rule".to_string(), utc(2026, 10, 6, 11, 0)),
        ]);
    }

    #[test]
    fn rdates_add_occurrences() {
        let cal = feed(
            "BEGIN:VEVENT\r\nUID:n\r\nSUMMARY:Extra\r\nDTSTART:20261005T100000Z\r\nDURATION:PT1H\r\n\
             RDATE:20261008T150000Z,20261009T150000Z\r\nEND:VEVENT\r\n",
        );
        let got = starts(&cal, utc(2026, 10, 5, 0, 0), utc(2026, 10, 9, 0, 0));
        assert_eq!(got.iter().map(|(_, s)| *s).collect::<Vec<_>>(), [utc(2026, 10, 5, 10, 0), utc(2026, 10, 8, 15, 0)]);
    }
}
