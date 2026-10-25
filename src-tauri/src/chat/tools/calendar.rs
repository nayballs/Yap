//! **Calendar tools in Chat** (2026-10-25): "what's on tomorrow?", "when's
//! my next meeting with Sam?", "am I free at 3 on Friday?" — answered from
//! the calendar Yap already keeps on this PC (`calendar.rs`: the meetings
//! from 12 hours ago to 7 days ahead, and the busy time of your own beside
//! them). No sync, no network: [`crate::calendar::agenda`] copies what's
//! kept and lets go of the calendar's lock before anything else runs.
//!
//! **One tool, three plain arguments** ([`definitions`], `check_calendar`):
//! small models do better with one tool than three, and every step of a
//! turn sends the tools again. `day` ("today", "tomorrow", "Friday",
//! "2026-10-30", "next week"), `time` ("15:00", "3pm", "afternoon") and
//! `with` (a person, or words in a meeting's title). **Yap reads the day and
//! the time** ([`resolve_day`], [`resolve_time`]) in the person's own zone —
//! About you's, else this PC's — so a model never has to work out which date
//! "Friday" is, and "tomorrow" asked at 23:50 is the day after the clock's.
//!
//! **Every meeting it gives is numbered** for the answer to cite ("[1]"),
//! in the same numbering as the turn's web sources; a number becomes a pill
//! that opens the meeting's note — made from the invite if there's none
//! yet, as the Meetings view's row does (`calendar_event` "open"). A meeting
//! the answer names but didn't cite gets its number from Yap
//! ([`cite_mentions`]). Busy time of your own is said, never numbered.
//!
//! **A model without tools** (Qwen2.5-1.5B, Qwen3.5-2B) gets the same
//! lookup done by Yap when the question is about the calendar
//! ([`asks_about_calendar`], [`question_args`]), as a block in its prompt
//! ([`block`]).

use std::collections::HashSet;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use serde_json::{json, Value};

use crate::calendar::{Agenda, Busy, Event};
use crate::chat::chats::Source;

use super::{tool_error, tool_result};

/// The tool's name.
pub const NAME: &str = "check_calendar";

/// Added to the tool instructions while a calendar is connected.
pub const CALENDAR_TOOL_INSTRUCTIONS: &[&str] = &[
    "Use check_calendar for the user's meetings and schedule: what's on a day, their next meeting (leave day empty), meetings with someone, whether they're free at a time. Pass the day and time as the user said them; Yap works out the dates. Cite each meeting you mention with its number, like [1].",
];

/// With the question when it's about the calendar and none is connected
/// (no tool offered): what to say instead of guessing.
pub const NOT_CONNECTED_NOTE: &str = "No calendar is connected to Yap, so you can't see the user's meetings: say so, and that they can connect Google Calendar or an iCal link in Settings → Connectors. Never make up meetings or times.";

/// At most this many meetings in one answer from the tool (a week of a
/// busy calendar would be too long for every later step to carry).
const MAX_MEETINGS: usize = 12;
/// "with Sam": the next few.
const MAX_WITH: usize = 5;
/// A time ("3pm") is checked for this long: a short meeting.
const SLOT_MINUTES: i64 = 30;

/// The calendar tool's definition (offered while a calendar is connected).
pub fn definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": NAME,
                "description": "Read the user's calendar (from 12 hours ago to 7 days ahead): meetings and busy times.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "day": { "type": "string", "description": "As the user said it: \"today\", \"tomorrow\", \"Friday\", \"next week\" or a date like \"2026-10-30\". Empty: the next 7 days." },
                        "time": { "type": "string", "description": "A time to check they're free, like \"15:00\", \"3pm\" or \"afternoon\"" },
                        "with": { "type": "string", "description": "A person's name or words in the meeting's title" }
                    },
                    "additionalProperties": false
                }
            }
        }
    ])
}

// ---- the person's zone ----------------------------------------------------------------------

/// Where "today" is: About you's time zone, else this PC's.
#[derive(Debug, Clone, Copy)]
pub enum Zone {
    Named(chrono_tz::Tz),
    Local,
}

impl Zone {
    /// The person's zone (`about_you::Region::time_zone`, an IANA name).
    pub fn of(iana: Option<&str>) -> Zone {
        iana.and_then(|z| z.parse::<chrono_tz::Tz>().ok()).map_or(Zone::Local, Zone::Named)
    }

    /// `ts` (unix seconds) on the wall clock here.
    pub fn wall(self, ts: i64) -> NaiveDateTime {
        let utc = DateTime::<Utc>::from_timestamp(ts, 0).unwrap_or_default();
        match self {
            Zone::Named(tz) => utc.with_timezone(&tz).naive_local(),
            Zone::Local => utc.with_timezone(&chrono::Local).naive_local(),
        }
    }

    /// A wall-clock time here as unix seconds: in a gap (the clocks went
    /// forward) the moment after it, in an overlap the first.
    pub fn at(self, wall: NaiveDateTime) -> i64 {
        fn pick<Z: TimeZone>(z: &Z, wall: NaiveDateTime) -> i64 {
            match z.from_local_datetime(&wall) {
                chrono::LocalResult::Single(t) => t.timestamp(),
                chrono::LocalResult::Ambiguous(a, _) => a.timestamp(),
                // In a gap: an hour on is past it.
                chrono::LocalResult::None => z
                    .from_local_datetime(&(wall + Duration::hours(1)))
                    .earliest()
                    .map_or_else(|| wall.and_utc().timestamp(), |t| t.timestamp()),
            }
        }
        match self {
            Zone::Named(tz) => pick(&tz, wall),
            Zone::Local => pick(&chrono::Local, wall),
        }
    }

    /// Midnight starting `day` here.
    pub fn day_start(self, day: NaiveDate) -> i64 {
        self.at(day.and_time(NaiveTime::MIN))
    }
}

// ---- reading the day and the time ------------------------------------------------------------

/// The days a question covers: `days` days from `from` (one for "Friday",
/// up to seven for "next week").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: NaiveDate,
    pub days: u32,
}

impl Span {
    fn day(d: NaiveDate) -> Span {
        Span { from: d, days: 1 }
    }
    /// The last day in it.
    pub fn to(&self) -> NaiveDate {
        self.from + Duration::days(i64::from(self.days.max(1)) - 1)
    }
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn weekday(word: &str) -> Option<Weekday> {
    let w = word.trim_end_matches(['s', '.', ',', '?']);
    Some(match w {
        "mon" | "monday" => Weekday::Mon,
        "tue" | "tues" | "tuesday" => Weekday::Tue,
        "wed" | "weds" | "wednesday" => Weekday::Wed,
        "thu" | "thur" | "thurs" | "thursday" => Weekday::Thu,
        "fri" | "friday" => Weekday::Fri,
        "sat" | "saturday" => Weekday::Sat,
        "sun" | "sunday" => Weekday::Sun,
        _ => return None,
    })
}

fn month(word: &str) -> Option<u32> {
    let w = word.trim_end_matches(['.', ',']);
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| w.starts_with(m) && (w.len() == 3 || "january february march april may june july august september october november december sept".split(' ').any(|full| full == w)))
        .map(|i| i as u32 + 1)
}

/// "30", "30th", "1st" → 30, 30, 1.
fn day_number(word: &str) -> Option<u32> {
    let w = word.trim_end_matches(['.', ',']);
    let digits = w.trim_end_matches("st").trim_end_matches("nd").trim_end_matches("rd").trim_end_matches("th");
    let n: u32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

/// The next date with this month and day on or after `today` (this year,
/// else next), or in `year` when it's said.
fn dated(today: NaiveDate, m: u32, d: u32, year: Option<i32>) -> Option<NaiveDate> {
    if let Some(y) = year {
        return NaiveDate::from_ymd_opt(y, m, d);
    }
    let this = NaiveDate::from_ymd_opt(today.year(), m, d);
    match this {
        // A date more than a fortnight gone is next year's.
        Some(t) if t >= today - Duration::days(14) => Some(t),
        _ => NaiveDate::from_ymd_opt(today.year() + 1, m, d),
    }
}

/// The days `text` names, seen from `today` (the person's own date):
/// "today", "tonight", "tomorrow", "yesterday", "day after tomorrow", "in 3
/// days", a weekday ("Friday": the next one, today included; "next Friday":
/// the next one after today; "last Friday"), a date ("2026-10-30", "30
/// October", "Oct 30th", "the 30th"), "this week" (today to Sunday), "next
/// week" (Monday to Sunday), "the weekend". `Ok(None)`: no day given (the
/// next 7 days); `Err`: words Yap can't read as a day. Pure.
pub fn resolve_day(text: &str, today: NaiveDate) -> Result<Option<Span>, String> {
    let t = text.trim().to_lowercase().replace(['’', '\''], "");
    let t = t.trim_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
    if t.is_empty() || matches!(t, "upcoming" | "soon" | "any" | "anytime" | "next 7 days" | "this coming week" | "the next week" | "the coming week" | "coming week") {
        return Ok(None);
    }
    // ISO, as the model is asked for: 2026-10-30 (or 2026/10/30).
    let iso = regex_lite_iso(t);
    if let Some((y, m, d)) = iso {
        return NaiveDate::from_ymd_opt(y, m, d).map(|d| Some(Span::day(d))).ok_or_else(|| format!("\"{text}\" isn't a date"));
    }
    let words: Vec<&str> = t.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let has = |w: &str| words.contains(&w);
    if has("today") || has("tonight") || t == "this morning" || t == "this afternoon" || t == "this evening" || t == "now" {
        return Ok(Some(Span::day(today)));
    }
    if t.contains("day after tomorrow") {
        return Ok(Some(Span::day(today + Duration::days(2))));
    }
    if has("tomorrow") || has("tmrw") || has("tomorow") {
        return Ok(Some(Span::day(today + Duration::days(1))));
    }
    if has("yesterday") {
        return Ok(Some(Span::day(today - Duration::days(1))));
    }
    // "in 3 days", "in a week".
    if let Some(i) = words.iter().position(|w| *w == "in") {
        let n = words.get(i + 1).and_then(|w| if matches!(*w, "a" | "one") { Some(1) } else { w.parse::<i64>().ok() });
        match (n, words.get(i + 2).map(|w| w.trim_end_matches('s'))) {
            (Some(n), Some("day")) if (0..=60).contains(&n) => return Ok(Some(Span::day(today + Duration::days(n)))),
            (Some(n), Some("week")) if (0..=8).contains(&n) => return Ok(Some(Span::day(today + Duration::days(7 * n)))),
            _ => {}
        }
    }
    // A week.
    let monday_after = |d: NaiveDate| d + Duration::days(7 - i64::from(d.weekday().num_days_from_monday()));
    if has("week") && !words.iter().any(|w| weekday(w).is_some()) {
        if has("next") {
            return Ok(Some(Span { from: monday_after(today), days: 7 }));
        }
        if has("this") || has("rest") || t == "week" || t == "the week" {
            let left = 7 - today.weekday().num_days_from_monday();
            return Ok(Some(Span { from: today, days: left }));
        }
        if has("last") {
            return Ok(Some(Span { from: monday_after(today) - Duration::days(14), days: 7 }));
        }
    }
    if has("weekend") {
        let sat = today + Duration::days((i64::from(Weekday::Sat.num_days_from_monday()) - i64::from(today.weekday().num_days_from_monday())).rem_euclid(7));
        // On a Sunday, "the weekend" is today; "next weekend" a week on.
        let (from, days) = if today.weekday() == Weekday::Sun { (today, 1) } else { (sat, 2) };
        let from = if has("next") && today.weekday() != Weekday::Sun { from + Duration::days(7) } else { from };
        return Ok(Some(Span { from, days }));
    }
    // A date by its month: "30 October", "Oct 30th", "30th of October 2026".
    let year = words.iter().filter_map(|w| w.parse::<i32>().ok()).find(|y| (2000..=2100).contains(y));
    for (i, w) in words.iter().enumerate() {
        if let Some(m) = month(w) {
            let before = i.checked_sub(1).and_then(|j| words.get(j)).and_then(|w| day_number(w))
                .or_else(|| i.checked_sub(2).filter(|_| words.get(i - 1) == Some(&"of")).and_then(|j| words.get(j)).and_then(|w| day_number(w)));
            let after = words.get(i + 1).and_then(|w| day_number(w));
            if let Some(d) = before.or(after) {
                return dated(today, m, d, year).map(|d| Some(Span::day(d))).ok_or_else(|| format!("\"{text}\" isn't a date"));
            }
        }
    }
    // A weekday: "Friday", "this Friday", "next Friday", "last Friday",
    // "Friday the 30th" (the weekday is what's checked).
    if let Some(wd) = words.iter().find_map(|w| weekday(w)) {
        let ahead = (i64::from(wd.num_days_from_monday()) - i64::from(today.weekday().num_days_from_monday())).rem_euclid(7);
        let day = if has("last") || has("past") {
            today - Duration::days(if ahead == 0 { 7 } else { 7 - ahead })
        } else if has("next") && ahead == 0 {
            today + Duration::days(7)
        } else {
            today + Duration::days(ahead)
        };
        return Ok(Some(Span::day(day)));
    }
    // "the 30th": this month's, else next month's.
    if let Some(d) = words.iter().find_map(|w| (w.ends_with("st") || w.ends_with("nd") || w.ends_with("rd") || w.ends_with("th")).then(|| day_number(w)).flatten()) {
        let this = NaiveDate::from_ymd_opt(today.year(), today.month(), d).filter(|x| *x >= today);
        let next = || {
            let (y, m) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
            NaiveDate::from_ymd_opt(y, m, d)
        };
        if let Some(day) = this.or_else(next) {
            return Ok(Some(Span::day(day)));
        }
    }
    Err(format!("Yap couldn't read \"{text}\" as a day: use today, tomorrow, a weekday, \"next week\" or a date like 2026-10-30"))
}

/// "2026-10-30" / "2026/10/30" anywhere in the text.
fn regex_lite_iso(t: &str) -> Option<(i32, u32, u32)> {
    static ISO: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b").expect("static regex"));
    let c = ISO.captures(t)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))
}

/// A stretch of a day to check: from `start`, `minutes` long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: NaiveTime,
    pub minutes: i64,
}

/// The time `text` names: "15:00", "15.30", "3pm", "3:30 p.m.", "noon",
/// "half past 3", "quarter to 4", a bare hour ("3": 1–7 are afternoon
/// hours, as people say them for meetings), or a part of the day
/// ("morning" 08:00–12:00, "afternoon" 12:00–17:00, "evening" 17:00–21:00,
/// "lunchtime" 12:00–14:00). A time is checked for [`SLOT_MINUTES`].
/// `Ok(None)`: no time given. Pure.
pub fn resolve_time(text: &str) -> Result<Option<Window>, String> {
    let t = text.trim().to_lowercase().replace('.', ":").replace("::", ":");
    let t = t.trim_matches(|c: char| c == '?' || c == ',' || c.is_whitespace()).trim_start_matches("at ").trim();
    if t.is_empty() || matches!(t, "any" | "anytime" | "all day" | "whole day") {
        return Ok(None);
    }
    let part = |h: u32, minutes: i64| Ok(Some(Window { start: NaiveTime::from_hms_opt(h, 0, 0).unwrap_or(NaiveTime::MIN), minutes }));
    if t.contains("morning") {
        return part(8, 4 * 60);
    }
    if t.contains("lunch") {
        return part(12, 2 * 60);
    }
    if t.contains("afternoon") {
        return part(12, 5 * 60);
    }
    if t.contains("evening") || t.contains("tonight") || t == "night" {
        return part(17, 4 * 60);
    }
    if t == "noon" || t == "midday" || t == "12 noon" {
        return part(12, SLOT_MINUTES);
    }
    if t == "midnight" {
        return part(0, SLOT_MINUTES);
    }
    let slot = |h: u32, m: u32| {
        NaiveTime::from_hms_opt(h, m, 0)
            .map(|start| Some(Window { start, minutes: SLOT_MINUTES }))
            .ok_or_else(|| format!("\"{text}\" isn't a time"))
    };
    // A bare hour as people say it for a meeting: 1–7 in the afternoon.
    let hour_of = |h: u32| if (1..=7).contains(&h) { h + 12 } else { h };
    // "half past 3", "quarter past 10", "quarter to 4".
    for (lead, minutes, back) in [("half past ", 30, false), ("quarter past ", 15, false), ("quarter to ", 45, true)] {
        if let Some(rest) = t.strip_prefix(lead) {
            let (h, pm, am) = (rest.trim_end_matches("pm").trim_end_matches("am").trim(), rest.ends_with("pm"), rest.ends_with("am"));
            if let Ok(h) = h.parse::<u32>() {
                let h = if back { h.saturating_sub(1) } else { h };
                let h = if pm && h < 12 { h + 12 } else if am { h % 12 } else { hour_of(h) };
                return slot(h, minutes);
            }
        }
    }
    static CLOCK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(\d{1,2})(?::(\d{2}))?\s*(a:?m:?|p:?m:?|o'?clock|h)?$").expect("static regex")
    });
    let compact = t.replace(' ', "");
    let compact = compact.trim_end_matches(':');
    if let Some(c) = CLOCK.captures(compact).or_else(|| CLOCK.captures(t)) {
        let h: u32 = c[1].parse().map_err(|_| format!("\"{text}\" isn't a time"))?;
        let m: u32 = c.get(2).map_or(Ok(0), |m| m.as_str().parse()).map_err(|_| format!("\"{text}\" isn't a time"))?;
        let suffix = c.get(3).map(|s| s.as_str().replace(':', "")).unwrap_or_default();
        let h = match suffix.as_str() {
            "pm" if h < 12 => h + 12,
            "pm" => h,
            "am" if h == 12 => 0,
            "am" => h,
            // "15:00", "09:30": a 24-hour clock; "3", "3:30", "3 o'clock": as said.
            _ if c.get(2).is_some() && (h >= 13 || c[1].len() == 2) => h,
            _ => hour_of(h),
        };
        if h > 23 || m > 59 {
            return Err(format!("\"{text}\" isn't a time"));
        }
        return slot(h, m);
    }
    Err(format!("Yap couldn't read \"{text}\" as a time: use one like 15:00 or 3pm"))
}

// ---- finding meetings -----------------------------------------------------------------------

/// Words a "with" leaves out ("my meeting with Sam" → "sam").
const FILLER: &[&str] = &["with", "the", "a", "an", "my", "meeting", "meetings", "call", "calls", "and", "next", "to", "about", "of", "for", "on", "in"];

/// Does `event` match `query` — every word of it the start of a word in
/// an attendee's name or address, the organiser's, or the title? An empty
/// query (only filler) matches everything. Pure.
pub fn matches(event: &Event, query: &str) -> bool {
    let wanted = words_of(query);
    if wanted.is_empty() {
        return true;
    }
    let mut hay: Vec<String> = Vec::new();
    for a in &event.attendees {
        hay.extend(words_of_all(&a.name));
        hay.extend(words_of_all(a.email.split('@').next().unwrap_or("")));
    }
    hay.extend(words_of_all(&event.organizer));
    hay.extend(words_of_all(&event.title));
    wanted.iter().all(|w| hay.iter().any(|h| h.starts_with(w.as_str())))
}

fn busy_matches(busy: &Busy, query: &str) -> bool {
    let wanted = words_of(query);
    let hay = words_of_all(&busy.title);
    !wanted.is_empty() && wanted.iter().all(|w| hay.iter().any(|h| h.starts_with(w.as_str())))
}

fn words_of_all(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// The query's words, filler and possessives left out ("Sam's" → "sam").
fn words_of(text: &str) -> Vec<String> {
    words_of_all(&text.replace(['’', '\''], " "))
        .into_iter()
        .filter(|w| !(FILLER.contains(&w.as_str()) || w == "s"))
        .collect()
}

/// "Fri 30 Oct, 10:00–10:30" (the start's day; an end on another day
/// says its own). Pure.
pub fn when(zone: Zone, start: i64, end: i64) -> String {
    let (s, e) = (zone.wall(start), zone.wall(end));
    let day = s.format("%a %-d %b").to_string();
    if e.date() == s.date() || e <= s {
        format!("{day}, {}–{}", s.format("%H:%M"), e.format("%H:%M"))
    } else {
        format!("{day}, {} – {}, {}", s.format("%H:%M"), e.format("%a %-d %b"), e.format("%H:%M"))
    }
}

/// "Friday 30 October 2026", or "Monday 2 – Sunday 8 November 2026".
fn span_words(span: Span) -> String {
    if span.days <= 1 {
        return span.from.format("%A %-d %B %Y").to_string();
    }
    let to = span.to();
    if to.month() == span.from.month() {
        format!("{} – {}", span.from.format("%A %-d"), to.format("%A %-d %B %Y"))
    } else {
        format!("{} – {}", span.from.format("%A %-d %B"), to.format("%A %-d %B %Y"))
    }
}

/// What the tool was asked, as Yap read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ask {
    pub day: String,
    pub time: String,
    pub with: String,
}

impl Ask {
    pub fn of(args: &Value) -> Ask {
        let s = |k: &str| args[k].as_str().unwrap_or("").trim().to_string();
        Ask { day: s("day"), time: s("time"), with: s("with") }
    }
}

/// One meeting the tool gives, numbered: what the turn's sources need.
#[derive(Debug, Clone, PartialEq)]
pub struct Cited {
    pub n: usize,
    pub key: String,
    pub title: String,
    pub when: String,
    pub note: Option<u64>,
    /// Who's on it (an answer may name the meeting by them).
    pub people: Vec<String>,
}

impl Cited {
    /// The answer's source for it: opens its note (or makes one from the
    /// invite), "Fri 30 Oct, 10:00–10:30" beside its title.
    pub fn source(&self) -> Source {
        Source {
            n: self.n,
            title: self.title.clone(),
            site: self.when.clone(),
            note: self.note,
            field: "calendar".into(),
            event: Some(self.key.clone()),
            ..Default::default()
        }
    }
}

/// The lookup itself (pure, given the calendar and the clock): the tool's
/// result for the model (`{success, data, displayText}`) and the meetings
/// it numbered. `number(key)` gives a meeting its number in the turn's
/// citations (the same one again for the same meeting).
pub fn look_up(ask: &Ask, agenda: &Agenda, zone: Zone, now: i64, number: &mut dyn FnMut(&str) -> usize) -> (Value, Vec<Cited>) {
    if !agenda.connected {
        return (
            tool_error(
                "No calendar is connected to Yap. Tell the user they can connect Google Calendar or an iCal link in Settings → Connectors, and Yap will see their meetings.".into(),
                Value::Null,
                "No calendar connected".into(),
            ),
            Vec::new(),
        );
    }
    let today = zone.wall(now).date();
    let day = match resolve_day(&ask.day, today) {
        Ok(d) => d,
        Err(e) => return (tool_error(e, Value::Null, format!("Couldn't read the day \"{}\"", short(&ask.day))), Vec::new()),
    };
    let time = match resolve_time(&ask.time) {
        Ok(t) => t,
        Err(e) => return (tool_error(e, Value::Null, format!("Couldn't read the time \"{}\"", short(&ask.time))), Vec::new()),
    };
    let kept_from = now - crate::calendar::KEPT_BACK;
    let kept_to = now + crate::calendar::KEPT_AHEAD;
    // What's asked about, as unix seconds [from, to).
    let (from, to, checked) = match (day, time) {
        (span, Some(w)) => {
            // A time with no day: today's, unless it has gone (then tomorrow's).
            let d = match span {
                Some(s) => s.from,
                None => {
                    let t = zone.at(today.and_time(w.start));
                    if t + w.minutes * 60 <= now { today + Duration::days(1) } else { today }
                }
            };
            let start = zone.at(d.and_time(w.start));
            let end = start + w.minutes * 60;
            (start, end, Some(when(zone, start, end)))
        }
        (Some(s), None) => (zone.day_start(s.from), zone.day_start(s.to() + Duration::days(1)), None),
        (None, None) if ask.with.trim().is_empty() => (now, kept_to, None),
        // "with Sam": the meetings with them still to come (or on now).
        (None, None) => (now, kept_to, None),
    };
    let asked = match (day, &checked) {
        (_, Some(c)) => c.clone(),
        (Some(s), None) => span_words(s),
        (None, None) => "the next 7 days".to_string(),
    };
    let outside = to <= kept_from || from >= kept_to;
    let window_note = "Yap keeps the calendar from 12 hours ago to 7 days ahead".to_string();
    if outside {
        return (
            tool_result(
                true,
                json!({ "asked": asked, "meetings": [], "note": format!("{window_note}: {asked} is outside it, so Yap can't see that day. Say so.") }),
                format!("Checked your calendar: {asked} is past what Yap keeps"),
            ),
            Vec::new(),
        );
    }
    let with = ask.with.trim();
    let mut meetings: Vec<&Event> = agenda
        .events
        .iter()
        .filter(|e| e.start < to && e.end.max(e.start + 60) > from)
        .filter(|e| with.is_empty() || matches(e, with))
        .collect();
    meetings.sort_by_key(|e| (e.start, e.key.clone()));
    let total = meetings.len();
    let cap = if !with.is_empty() && day.is_none() && time.is_none() { MAX_WITH } else { MAX_MEETINGS };
    meetings.truncate(cap);
    let busy: Vec<&Busy> = agenda
        .busy
        .iter()
        .filter(|b| b.start < to && b.end.max(b.start + 60) > from)
        .filter(|b| with.is_empty() || busy_matches(b, with))
        .collect();
    let owned: Vec<Event> = meetings.iter().map(|e| (*e).clone()).collect();
    let conflicts = crate::calendar::conflicts_of(&owned);
    let mut cited = Vec::new();
    // One meeting for the model, numbered for the answer.
    let mut item_of = |e: &Event, cited: &mut Vec<Cited>| {
        let n = number(&e.key);
        let w = when(zone, e.start, e.end);
        cited.push(Cited {
            n,
            key: e.key.clone(),
            title: e.display_title(),
            when: w.clone(),
            note: agenda.notes.get(&e.key).copied(),
            people: e.attendees.iter().map(|a| a.name.clone()).collect(),
        });
        let mut item = json!({ "n": n, "title": e.display_title(), "when": w });
        let with_line = e.attendees.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
        if !with_line.is_empty() {
            item["with"] = json!(with_line);
        }
        if let Some(s) = e.service_label() {
            item["on"] = json!(s);
        }
        if e.tentative {
            item["maybe"] = json!(true);
        }
        if conflicts.contains(&e.key) {
            item["overlaps_another"] = json!(true);
        }
        if e.start <= now && now < e.end {
            item["on_now"] = json!(true);
        } else if e.end <= now {
            // "What's on today?" late in the day: not the next one.
            item["over"] = json!(true);
        }
        item
    };
    let items: Vec<Value> = meetings.iter().map(|e| item_of(e, &mut cited)).collect();
    // Nothing (still to come) in what was asked, and no time asked: the
    // next meeting after it — "what's my next meeting?" asked as "today"
    // late in the day (the Voice Assistant's evaluation, 2026-10-25).
    let nothing_ahead = meetings.iter().all(|e| e.end <= now);
    let next_after = (checked.is_none() && nothing_ahead)
        .then(|| {
            agenda
                .events
                .iter()
                .filter(|e| e.start >= to.max(now) && (with.is_empty() || matches(e, with)))
                .min_by_key(|e| (e.start, e.key.clone()))
        })
        .flatten()
        .map(|e| item_of(e, &mut cited));
    let busy_items: Vec<Value> = busy
        .iter()
        .map(|b| json!({ "title": if b.title.is_empty() { "Busy" } else { b.title.as_str() }, "when": when(zone, b.start, b.end), "own_event": true }))
        .collect();
    let mut data = json!({ "asked": asked, "meetings": items });
    if !busy_items.is_empty() {
        data["busy"] = json!(busy_items);
    }
    if total > meetings.len() {
        data["more"] = json!(format!("{} more not shown: ask about a narrower day or time", total - meetings.len()));
    }
    if !with.is_empty() {
        data["with"] = json!(with);
    }
    // Only what Yap knows: a calendar it hasn't synced for a while says so.
    if agenda.synced_ts > 0 && now - agenda.synced_ts > 2 * 3600 {
        data["note"] = json!(format!("The calendar was last synced {} hours ago: newer changes may be missing.", (now - agenda.synced_ts) / 3600));
    }
    let display = if let Some(c) = &checked {
        let free = items.is_empty() && busy_items.is_empty();
        data["free"] = json!(free);
        if free {
            format!("Free {c}")
        } else {
            format!("Busy {c}")
        }
    } else if !with.is_empty() {
        match total {
            0 => format!("No meetings with \"{}\" in your calendar", short(with)),
            1 => format!("Found 1 meeting with \"{}\"", short(with)),
            n => format!("Found {n} meetings with \"{}\"", short(with)),
        }
    } else {
        match total {
            0 => format!("Checked your calendar: nothing on {asked}"),
            1 => format!("Checked your calendar: 1 meeting, {asked}"),
            n => format!("Checked your calendar: {n} meetings, {asked}"),
        }
    };
    if items.is_empty() && busy_items.is_empty() && checked.is_none() {
        let note = if with.is_empty() { format!("No meetings or busy times {}.", in_phrase(&asked)) } else { format!("No meetings matching \"{with}\" {}. {window_note}.", in_phrase(&asked)) };
        data["note"] = json!(note);
    }
    if let Some(next) = next_after {
        data["next_meeting_after"] = next;
    }
    (tool_result(true, data, display), cited)
}

fn in_phrase(asked: &str) -> String {
    if asked.starts_with("the next") { format!("in {asked}") } else { format!("on {asked}") }
}

fn short(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 40 { format!("{}…", s.chars().take(40).collect::<String>()) } else { s.to_string() }
}

// ---- in a turn ------------------------------------------------------------------------------

/// The turn's calendar: what the lookups numbered, for the answer's
/// sources. One per turn (`turn::chat_turn` → the tool loop's `Extra`).
#[derive(Debug, Default)]
pub struct Log {
    cited: std::sync::Mutex<Vec<Cited>>,
}

impl Log {
    pub fn add(&self, more: Vec<Cited>) {
        let mut all = self.cited.lock().unwrap_or_else(|p| p.into_inner());
        for c in more {
            if !all.iter().any(|x| x.key == c.key) {
                all.push(c);
            }
        }
    }

    /// The number a meeting already has this turn.
    pub fn number_of(&self, key: &str) -> Option<usize> {
        self.cited.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|c| c.key == key).map(|c| c.n)
    }

    pub fn len(&self) -> usize {
        self.cited.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn all(&self) -> Vec<Cited> {
        self.cited.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// Run the tool in a turn: the calendar read (no lock held after), the
/// lookup in the person's zone, each meeting numbered in the turn's
/// numbering (`next`: the turn's next free number), kept in `log`.
pub fn run(args: &Value, log: &Log, zone: Zone, now: i64, next: &mut dyn FnMut() -> usize) -> Value {
    let agenda = crate::calendar::agenda();
    let ask = Ask::of(args);
    let mut fresh: Vec<(String, usize)> = Vec::new();
    let mut number = |key: &str| {
        if let Some(n) = log.number_of(key).or_else(|| fresh.iter().find(|(k, _)| k == key).map(|(_, n)| *n)) {
            return n;
        }
        let n = next();
        fresh.push((key.to_string(), n));
        n
    };
    let (result, cited) = look_up(&ask, &agenda, zone, now, &mut number);
    tracing::info!(meetings = cited.len(), day = !ask.day.is_empty(), time = !ask.time.is_empty(), with = !ask.with.is_empty(), "Chat: calendar looked up");
    log.add(cited);
    result
}

/// The answer's calendar sources: the meetings it cites. Pure.
pub fn sources_for(reply: &str, cited: &[Cited]) -> Vec<Source> {
    let numbers = crate::web::citations(reply);
    let mut out: Vec<Source> = cited.iter().filter(|c| numbers.contains(&c.n)).map(Cited::source).collect();
    out.sort_by_key(|s| s.n);
    out
}

/// A meeting the answer names (by its title) without citing it gets its
/// number after the first mention, so it opens from the answer; a title
/// too plain to tell ("Meeting", one short word) is left alone. Pure.
pub fn cite_mentions(reply: &str, cited: &[Cited]) -> String {
    let numbers: HashSet<usize> = crate::web::citations(reply).into_iter().collect();
    let mut out = reply.to_string();
    for c in cited {
        if numbers.contains(&c.n) {
            continue;
        }
        let title = c.title.trim();
        let distinctive = title.chars().count() >= 5 && !title.eq_ignore_ascii_case("meeting") && !title.eq_ignore_ascii_case("busy");
        // Named by its title, else by someone on it no other meeting here
        // has ("a Teams call with Ravi Menon").
        let by_person = || {
            c.people.iter().find(|p| {
                p.split_whitespace().count() >= 2 && !cited.iter().any(|o| o.n != c.n && o.people.iter().any(|q| q.eq_ignore_ascii_case(p)))
            })
        };
        // …else by its start ("on Tuesday at 15:00"), when no other meeting
        // here starts at that time.
        let by_start = || {
            let hm = c.when.split(", ").nth(1).and_then(|t| t.get(..5)).filter(|t| t.len() == 5 && t.as_bytes()[2] == b':')?;
            let shared = cited.iter().any(|o| o.n != c.n && o.when.split(", ").nth(1).and_then(|t| t.get(..5)) == Some(hm));
            (!shared).then_some(hm)
        };
        let found = distinctive
            .then(|| find_word(&out, title))
            .flatten()
            .or_else(|| by_person().and_then(|p| find_word(&out, p)))
            .or_else(|| by_start().and_then(|hm| find_word(&out, hm)));
        let Some(end) = found else { continue };
        // Past a closing quote or bold marker that wraps it.
        let rest = &out[end..];
        let skip = rest.len() - rest.trim_start_matches(['*', '"', '\'', '’', '”', '_']).len();
        let at_end = end + skip;
        if out[at_end..].trim_start().starts_with('[') {
            continue; // another citation there already
        }
        out.insert_str(at_end, &format!(" [{}]", c.n));
    }
    out
}

/// Where `needle` first ends in `hay`, matched whole words and any case, a
/// hyphen any dash ("catch‑up" with a non-breaking hyphen, as gpt-oss
/// writes it) and a space any run of spaces.
fn find_word(hay: &str, needle: &str) -> Option<usize> {
    let pattern: String = needle
        .split_whitespace()
        .map(|w| w.split('-').map(regex::escape).collect::<Vec<_>>().join(r"[-\u{2010}-\u{2015}]"))
        .collect::<Vec<_>>()
        .join(r"\s+");
    let re = regex::Regex::new(&format!(r"(?i)\b{pattern}\b")).ok()?;
    re.find(hay).map(|m| m.end())
}

// ---- a model without tools ------------------------------------------------------------------

/// Is the question about the person's calendar ("what's on tomorrow?",
/// "when's my next meeting with Sam?", "am I free at 3 on Friday?")? Pure.
pub fn asks_about_calendar(question: &str) -> bool {
    static ASK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?ix)\b(?:
                calendar|schedule|diary|my\s+agenda
              | meetings?\b(?:\s+\w+){0,3}\s+(?:today|tomorrow|tonight|this|next|on|at|with)
              | (?:next|first|upcoming)\s+(?:meeting|call|appointment)s?\b
              | (?:meeting|call|appointment)s?\s+with
              | (?:am\s+i|are\s+we|is\s+\w+)\s+(?:free|busy|available)
              | (?:free|busy)\s+(?:at|on|this|tomorrow|today|next)
              | what(?:'s|s|\s+is)?\s+on\s+(?:today|tomorrow|tonight|this|next|on|for|mon|tue|wed|thu|fri|sat|sun)
              | what\s+(?:have|do)\s+i\s+(?:got|have)\s+(?:on\s+)?(?:today|tomorrow|this|next|on)
              | when(?:'s|s|\s+is)\s+(?:my|the)\s+(?:next\s+)?(?:meeting|call|appointment|catch.?up|1:1|one.to.one|standup|stand.up)
            )",
        )
        .expect("static regex")
    });
    ASK.is_match(&question.replace('’', "'"))
}

/// The lookup a question asks for, read from its own words (a model
/// without tools can't pass them): the day ("tomorrow", "on Friday", "30
/// October"), the time ("at 3", "3pm", "this afternoon") and who with
/// ("with Sam"). Pure.
pub fn question_args(question: &str) -> Ask {
    let q = question.replace('’', "'");
    let lower = q.to_lowercase();
    // The day: the first phrase that reads as one.
    static DAY: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(?:(?:next|this|last)\s+)?(?:today|tonight|tomorrow|yesterday|day after tomorrow|week|weekend|mon(?:day)?|tues?(?:day)?|wed(?:nesday)?|thu(?:rs?(?:day)?)?|fri(?:day)?|sat(?:urday)?|sun(?:day)?)\b|\b\d{4}-\d{1,2}-\d{1,2}\b|\b\d{1,2}(?:st|nd|rd|th)?\s+(?:of\s+)?(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b|\b(?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\s+\d{1,2}(?:st|nd|rd|th)?\b|\bin\s+(?:\d+|a|one)\s+(?:days?|weeks?)\b",
        )
        .expect("static regex")
    });
    let day = DAY
        .find_iter(&lower)
        .map(|m| m.as_str().to_string())
        // "this afternoon" is a time today, not a week.
        .find(|d| !matches!(d.as_str(), "this" | "next"))
        .unwrap_or_default();
    static TIME: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(?:at|around|by|from)\s+(\d{1,2}(?::\d{2})?\s*(?:am|pm|a\.m\.|p\.m\.)?|noon|midday)\b|\b(\d{1,2}(?::\d{2})?\s*(?:am|pm|a\.m\.|p\.m\.))|\b(\d{1,2}:\d{2})\b|\b(morning|afternoon|evening|lunchtime|lunch|tonight)\b|\b(half past \d{1,2}|quarter (?:past|to) \d{1,2})\b",
        )
        .expect("static regex")
    });
    let time = TIME
        .captures(&lower)
        .and_then(|c| (1..=5).find_map(|i| c.get(i)).map(|m| m.as_str().trim().to_string()))
        .unwrap_or_default();
    static WITH: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\bwith\s+((?:[A-Z][\w'-]*)(?:\s+[A-Z][\w'-]*)?)").expect("static regex"));
    let with = WITH
        .captures(&q)
        .map(|c| c[1].trim_end_matches("'s").to_string())
        .filter(|w| !matches!(w.to_lowercase().as_str(), "me" | "you" | "us" | "them" | "him" | "her"))
        .unwrap_or_default();
    // "this afternoon" / "tonight": today.
    let day = if day.is_empty() && (lower.contains("this afternoon") || lower.contains("this morning") || lower.contains("this evening")) {
        "today".to_string()
    } else {
        day
    };
    Ask { day, time, with }
}

/// For a model without tools: the calendar lookup the question asks for,
/// done by Yap, as a block for its prompt and its chip — or `None` when the
/// question isn't about the calendar. The meetings are numbered from 1 (no
/// web sources in such a turn); `cited` gets them.
pub fn block(question: &str, zone: Zone, now: i64, cited: &mut Vec<Cited>) -> Option<(String, String)> {
    if !asks_about_calendar(question) {
        return None;
    }
    let agenda = crate::calendar::agenda();
    let ask = question_args(question);
    let mut n = 0;
    let mut number = |_: &str| {
        n += 1;
        n
    };
    let (result, found) = look_up(&ask, &agenda, zone, now, &mut number);
    tracing::info!(meetings = found.len(), "Chat: calendar looked up for a model without tools");
    *cited = found;
    let lead = "Yap looked up the user's calendar for the user's latest question (times are the user's). Answer it from this, not from earlier answers, and cite each meeting you mention with its number, like [1].";
    let body = if result["success"].as_bool() == Some(false) {
        result["error"].as_str().unwrap_or("Yap couldn't read the calendar.").to_string()
    } else {
        plain(&result["data"])
    };
    let chip = result["displayText"].as_str().unwrap_or("Checked your calendar").to_string();
    Some((format!("{lead}\n<calendar>\n{body}\n</calendar>"), chip))
}

/// The lookup's result in plain lines, for a model without tools: Qwen2.5-1.5B
/// and Qwen3.5-2B, given it as JSON, answered "Yes, you are free at 3 on
/// Friday" over `"free": false` and the meeting at 15:00 (the evaluations,
/// 2026-10-25), so the verdict comes first, in words. Pure.
pub fn plain(data: &Value) -> String {
    let mut out = Vec::new();
    let asked = data["asked"].as_str().unwrap_or("");
    match data["free"].as_bool() {
        Some(true) => out.push(format!("Asked about {asked}. The answer: YES, the user is free then (nothing on their calendar).")),
        Some(false) => out.push(format!("Asked about {asked}. The answer: NO, the user is not free then; they have:")),
        None if !asked.is_empty() => out.push(format!("Asked about {asked}:")),
        None => {}
    }
    let line = |m: &Value| {
        let mut s = format!("{} — {}", m["title"].as_str().unwrap_or("Meeting"), m["when"].as_str().unwrap_or(""));
        if let Some(w) = m["with"].as_str() {
            s.push_str(&format!(", with {w}"));
        }
        if let Some(o) = m["on"].as_str() {
            s.push_str(&format!(", on {o}"));
        }
        if m["maybe"] == json!(true) {
            s.push_str(" (they said maybe)");
        }
        if m["on_now"] == json!(true) {
            s.push_str(" (on now)");
        } else if m["over"] == json!(true) {
            s.push_str(" (already over)");
        }
        s
    };
    let meetings = data["meetings"].as_array().cloned().unwrap_or_default();
    for m in &meetings {
        out.push(format!("- [{}] {}", m["n"], line(m)));
    }
    for b in data["busy"].as_array().into_iter().flatten() {
        out.push(format!("- {} (their own event, not a meeting: no number)", line(b)));
    }
    if meetings.is_empty() && data["busy"].as_array().is_none_or(Vec::is_empty) && data["free"].is_null() {
        out.push("- No meetings.".to_string());
    }
    if let Some(next) = data["next_meeting_after"].as_object() {
        let next = Value::Object(next.clone());
        out.push(format!("Their next meeting after that: [{}] {}", next["n"], line(&next)));
    }
    for k in ["more", "note"] {
        if let Some(s) = data[k].as_str() {
            out.push(s.to_string());
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Attendee;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // Sunday 25 October 2026.
    fn sunday() -> NaiveDate {
        d(2026, 10, 25)
    }

    #[test]
    fn days_are_read_as_people_say_them() {
        let today = sunday();
        let one = |s: &str| resolve_day(s, today).unwrap().map(|s| (s.from, s.days));
        assert_eq!(one(""), None);
        assert_eq!(one("today"), Some((today, 1)));
        assert_eq!(one("Tonight"), Some((today, 1)));
        assert_eq!(one("tomorrow"), Some((d(2026, 10, 26), 1)));
        assert_eq!(one("tomorrow morning"), Some((d(2026, 10, 26), 1)));
        assert_eq!(one("the day after tomorrow"), Some((d(2026, 10, 27), 1)));
        assert_eq!(one("yesterday"), Some((d(2026, 10, 24), 1)));
        // A weekday: the next one, today included.
        assert_eq!(one("Friday"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("on Friday"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("this friday"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("Sunday"), Some((today, 1)));
        assert_eq!(one("next Sunday"), Some((d(2026, 11, 1), 1)));
        assert_eq!(one("next Friday"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("last Friday"), Some((d(2026, 10, 23), 1)));
        assert_eq!(one("Mon"), Some((d(2026, 10, 26), 1)));
        assert_eq!(one("Thursday's"), Some((d(2026, 10, 29), 1)));
        // Dates.
        assert_eq!(one("2026-10-30"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("2026/11/2"), Some((d(2026, 11, 2), 1)));
        assert_eq!(one("30 October"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("Oct 30th"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("the 2nd of November"), Some((d(2026, 11, 2), 1)));
        assert_eq!(one("Friday 30 October"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("the 30th"), Some((d(2026, 10, 30), 1)));
        assert_eq!(one("the 3rd"), Some((d(2026, 11, 3), 1)), "this month's has gone: next month's");
        // A date long gone this year is next year's.
        assert_eq!(one("2 January"), Some((d(2027, 1, 2), 1)));
        assert_eq!(one("in 3 days"), Some((d(2026, 10, 28), 1)));
        assert_eq!(one("in a week"), Some((d(2026, 11, 1), 1)));
        // Weeks and weekends.
        assert_eq!(one("next week"), Some((d(2026, 10, 26), 7)));
        assert_eq!(one("this week"), Some((today, 1)), "Sunday: the week's last day");
        assert_eq!(resolve_day("this week", d(2026, 10, 21)).unwrap().map(|s| (s.from, s.days)), Some((d(2026, 10, 21), 5)));
        assert_eq!(resolve_day("the weekend", d(2026, 10, 21)).unwrap().map(|s| (s.from, s.days)), Some((d(2026, 10, 24), 2)));
        assert_eq!(one("next 7 days"), None);
        assert!(resolve_day("the blue moon", today).is_err());
        assert!(resolve_day("2026-02-30", today).is_err());
    }

    #[test]
    fn times_are_read_as_people_say_them() {
        let one = |s: &str| resolve_time(s).unwrap().map(|w| (w.start.format("%H:%M").to_string(), w.minutes));
        let at = |h: &str| Some((h.to_string(), SLOT_MINUTES));
        assert_eq!(one(""), None);
        assert_eq!(one("15:00"), at("15:00"));
        assert_eq!(one("15.30"), at("15:30"));
        assert_eq!(one("09:30"), at("09:30"));
        assert_eq!(one("3pm"), at("15:00"));
        assert_eq!(one("3 PM"), at("15:00"));
        assert_eq!(one("3:30 p.m."), at("15:30"));
        assert_eq!(one("11am"), at("11:00"));
        assert_eq!(one("12am"), at("00:00"));
        assert_eq!(one("12pm"), at("12:00"));
        assert_eq!(one("at 3"), at("15:00"), "a bare 3 is the afternoon's");
        assert_eq!(one("3 o'clock"), at("15:00"));
        assert_eq!(one("10"), at("10:00"));
        assert_eq!(one("noon"), at("12:00"));
        assert_eq!(one("half past 3"), at("15:30"));
        assert_eq!(one("quarter to 4"), at("15:45"));
        assert_eq!(one("afternoon"), Some(("12:00".into(), 300)));
        assert_eq!(one("this morning"), Some(("08:00".into(), 240)));
        assert!(resolve_time("25:00").is_err());
        assert!(resolve_time("teatime-ish").is_err());
    }

    fn london() -> Zone {
        Zone::Named("Europe/London".parse().unwrap())
    }

    fn ts(zone: Zone, y: i32, m: u32, day: u32, h: u32, min: u32) -> i64 {
        zone.at(d(y, m, day).and_hms_opt(h, min, 0).unwrap())
    }

    fn meeting(key: &str, title: &str, start: i64, minutes: i64, with: &[&str]) -> Event {
        Event {
            key: key.into(),
            connection: "1".into(),
            title: title.into(),
            start,
            end: start + minutes * 60,
            attendees: with.iter().map(|n| Attendee { name: (*n).into(), email: format!("{}@example.com", n.to_lowercase().replace(' ', ".")) }).collect(),
            organizer: String::new(),
            join_url: None,
            service: Some("teams".into()),
            description: String::new(),
            tentative: false,
        }
    }

    /// Sunday 25 October 2026, 22:00 in London, and a week of meetings.
    fn week() -> (Agenda, Zone, i64) {
        let z = london();
        let now = ts(z, 2026, 10, 25, 22, 0);
        let events = vec![
            meeting("1:standup", "Standup", ts(z, 2026, 10, 26, 9, 30), 15, &["Priya Shah", "Sam Lee"]),
            meeting("1:design", "Design review", ts(z, 2026, 10, 26, 14, 0), 60, &["Tanay Kothari"]),
            meeting("1:sam", "Budget catch-up", ts(z, 2026, 10, 28, 11, 0), 30, &["Sam Lee"]),
            meeting("1:fri", "Roadmap planning", ts(z, 2026, 10, 30, 15, 0), 60, &["Priya Shah"]),
        ];
        let busy = vec![Busy { key: "1:dentist".into(), connection: "1".into(), title: "Dentist".into(), start: ts(z, 2026, 10, 29, 15, 0), end: ts(z, 2026, 10, 29, 16, 0) }];
        let mut notes = std::collections::HashMap::new();
        notes.insert("1:design".to_string(), 42);
        (Agenda { connected: true, events, busy, notes, synced_ts: now - 600 }, z, now)
    }

    fn ask(day: &str, time: &str, with: &str) -> Ask {
        Ask { day: day.into(), time: time.into(), with: with.into() }
    }

    fn run_on(a: &Ask, agenda: &Agenda, zone: Zone, now: i64) -> (Value, Vec<Cited>) {
        let mut n = 0;
        let mut number = |_: &str| {
            n += 1;
            n
        };
        look_up(a, agenda, zone, now, &mut number)
    }

    #[test]
    fn whats_on_tomorrow() {
        let (agenda, z, now) = week();
        let (r, cited) = run_on(&ask("tomorrow", "", ""), &agenda, z, now);
        assert_eq!(r["success"], true);
        let titles: Vec<&str> = r["data"]["meetings"].as_array().unwrap().iter().map(|m| m["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Standup", "Design review"]);
        assert_eq!(r["data"]["asked"], "Monday 26 October 2026");
        assert_eq!(r["data"]["meetings"][1]["when"], "Mon 26 Oct, 14:00–15:00");
        assert_eq!(r["data"]["meetings"][1]["with"], "Tanay Kothari");
        assert_eq!(r["data"]["meetings"][1]["on"], "Teams");
        assert_eq!(r["displayText"], "Checked your calendar: 2 meetings, Monday 26 October 2026");
        // Numbered for the answer, the note known where there is one.
        assert_eq!(cited.iter().map(|c| (c.n, c.note)).collect::<Vec<_>>(), [(1, None), (2, Some(42))]);
        let src = cited[1].source();
        assert_eq!((src.event.as_deref(), src.note, src.site.as_str(), src.url.as_str()), (Some("1:design"), Some(42), "Mon 26 Oct, 14:00–15:00", ""));
    }

    #[test]
    fn tomorrow_near_midnight_is_the_persons_tomorrow() {
        let (agenda, z, _) = week();
        // 23:50 on Sunday in London is 22:50 UTC: tomorrow is Monday either way…
        let late = ts(z, 2026, 10, 25, 23, 50);
        let (r, _) = run_on(&ask("tomorrow", "", ""), &agenda, z, late);
        assert_eq!(r["data"]["asked"], "Monday 26 October 2026");
        // …and ten minutes later it's Monday: tomorrow is Tuesday, with nothing on.
        let after = ts(z, 2026, 10, 26, 0, 10);
        let (r, _) = run_on(&ask("tomorrow", "", ""), &agenda, z, after);
        assert_eq!(r["data"]["asked"], "Tuesday 27 October 2026");
        assert_eq!(r["data"]["meetings"].as_array().unwrap().len(), 0);
        // In New York it's still Sunday evening at that moment: tomorrow is Monday,
        // and the 09:30 London standup is at 05:30 there.
        let ny = Zone::Named("America/New_York".parse().unwrap());
        let (r, _) = run_on(&ask("tomorrow", "", ""), &agenda, ny, after);
        assert_eq!(r["data"]["asked"], "Monday 26 October 2026");
        assert_eq!(r["data"]["meetings"][0]["when"], "Mon 26 Oct, 05:30–05:45");
    }

    #[test]
    fn the_clocks_going_back_keeps_the_day_whole() {
        // 25 October 2026: the UK's clocks went back at 02:00 (a 25-hour day).
        let z = london();
        let now = ts(z, 2026, 10, 24, 20, 0);
        let early = meeting("1:early", "Early call", ts(z, 2026, 10, 25, 1, 30), 30, &["Sam Lee"]);
        let late = meeting("1:late", "Late call", ts(z, 2026, 10, 25, 23, 30), 30, &["Sam Lee"]);
        let agenda = Agenda { connected: true, events: vec![early, late], ..Default::default() };
        let (r, _) = run_on(&ask("tomorrow", "", ""), &agenda, z, now);
        assert_eq!(r["data"]["meetings"].as_array().unwrap().len(), 2);
        assert_eq!(z.day_start(d(2026, 10, 26)) - z.day_start(d(2026, 10, 25)), 25 * 3600);
    }

    #[test]
    fn an_empty_day_gives_the_next_meeting_after_it() {
        let (agenda, z, now) = week();
        // Sunday 22:00, "today": nothing left — the next one is Monday's standup.
        let (r, cited) = run_on(&ask("today", "", ""), &agenda, z, now);
        assert_eq!(r["data"]["meetings"].as_array().unwrap().len(), 0);
        assert_eq!(r["data"]["next_meeting_after"]["title"], "Standup");
        assert_eq!(r["data"]["next_meeting_after"]["n"], 1);
        assert_eq!(cited.len(), 1);
        // A meeting earlier today is over, and the next one is still given.
        let earlier = meeting("1:early", "Early sync", now - 3 * 3600, 30, &["Sam Lee"]);
        let with_early = Agenda { events: [vec![earlier], agenda.events.clone()].concat(), ..agenda.clone() };
        let (r, _) = run_on(&ask("today", "", ""), &with_early, z, now);
        assert_eq!(r["data"]["meetings"][0]["over"], true);
        assert_eq!(r["data"]["next_meeting_after"]["title"], "Standup");
        // A time asked: no "next" (free or busy is the answer).
        let (r, _) = run_on(&ask("today", "23:00", ""), &agenda, z, now);
        assert!(r["data"]["next_meeting_after"].is_null());
    }

    #[test]
    fn next_meeting_with_someone() {
        let (agenda, z, now) = week();
        let (r, cited) = run_on(&ask("", "", "Sam"), &agenda, z, now);
        let titles: Vec<&str> = r["data"]["meetings"].as_array().unwrap().iter().map(|m| m["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Standup", "Budget catch-up"]);
        assert_eq!(r["displayText"], "Found 2 meetings with \"Sam\"");
        assert_eq!(cited.len(), 2);
        // By full name, by surname, possessive, filler, or the title.
        assert_eq!(run_on(&ask("", "", "my meeting with Sam Lee"), &agenda, z, now).1.len(), 2);
        assert_eq!(run_on(&ask("", "", "Lee"), &agenda, z, now).1.len(), 2);
        assert_eq!(run_on(&ask("", "", "Priya's"), &agenda, z, now).1.len(), 2);
        assert_eq!(run_on(&ask("", "", "roadmap"), &agenda, z, now).1[0].title, "Roadmap planning");
        // Nobody: says so, and what Yap keeps.
        let (r, _) = run_on(&ask("", "", "Bartholomew"), &agenda, z, now);
        assert_eq!(r["displayText"], "No meetings with \"Bartholomew\" in your calendar");
        assert!(r["data"]["note"].as_str().unwrap().contains("12 hours ago to 7 days ahead"));
        // Someone's first name alone doesn't match the start of another word in the middle.
        assert!(!matches(&agenda.events[2], "am"));
    }

    #[test]
    fn free_at_three_on_friday() {
        let (agenda, z, now) = week();
        let (r, cited) = run_on(&ask("Friday", "3pm", ""), &agenda, z, now);
        assert_eq!(r["data"]["free"], false);
        assert_eq!(r["data"]["asked"], "Fri 30 Oct, 15:00–15:30");
        assert_eq!(r["data"]["meetings"][0]["title"], "Roadmap planning");
        assert_eq!(r["displayText"], "Busy Fri 30 Oct, 15:00–15:30");
        assert_eq!(cited.len(), 1);
        // Thursday at 3: the dentist, an event of their own, never numbered.
        let (r, cited) = run_on(&ask("Thursday", "15:00", ""), &agenda, z, now);
        assert_eq!(r["data"]["free"], false);
        assert_eq!(r["data"]["busy"][0]["title"], "Dentist");
        assert!(cited.is_empty());
        // Friday at 11: free.
        let (r, _) = run_on(&ask("Friday", "11:00", ""), &agenda, z, now);
        assert_eq!(r["data"]["free"], true);
        assert_eq!(r["displayText"], "Free Fri 30 Oct, 11:00–11:30");
        // A meeting ending as the time starts isn't in the way.
        let (r, _) = run_on(&ask("Friday", "16:00", ""), &agenda, z, now);
        assert_eq!(r["data"]["free"], true);
        // A time with no day, already gone today: tomorrow's.
        let (r, _) = run_on(&ask("", "2pm", ""), &agenda, z, now);
        assert_eq!(r["data"]["asked"], "Mon 26 Oct, 14:00–14:30");
        assert_eq!(r["data"]["free"], false);
    }

    #[test]
    fn outside_what_yap_keeps_or_nothing_connected() {
        let (agenda, z, now) = week();
        let (r, cited) = run_on(&ask("2026-11-20", "", ""), &agenda, z, now);
        assert_eq!(r["success"], true);
        assert!(r["data"]["note"].as_str().unwrap().contains("outside it"));
        assert!(cited.is_empty());
        let (r, _) = run_on(&ask("tomorrow", "", ""), &Agenda::default(), z, now);
        assert_eq!(r["success"], false);
        assert_eq!(r["displayText"], "No calendar connected");
        assert!(r["error"].as_str().unwrap().contains("Settings → Connectors"));
        let (r, _) = run_on(&ask("whenever the moon is full", "", ""), &agenda, z, now);
        assert_eq!(r["success"], false);
        // A stale calendar says so.
        let stale = Agenda { synced_ts: now - 5 * 3600, ..agenda.clone() };
        let (r, _) = run_on(&ask("tomorrow", "", ""), &stale, z, now);
        assert!(r["data"]["note"].as_str().unwrap().contains("5 hours ago"));
    }

    #[test]
    fn the_answer_cites_the_meetings_it_names() {
        let people = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cited = vec![
            Cited { n: 1, key: "1:design".into(), title: "Design review".into(), when: "Mon 26 Oct, 14:00–15:00".into(), note: Some(42), people: people(&["Tanay Kothari"]) },
            Cited { n: 2, key: "1:standup".into(), title: "Standup".into(), when: "Mon 26 Oct, 09:30–09:45".into(), note: None, people: people(&["Priya Shah", "Sam Lee"]) },
            Cited { n: 3, key: "1:x".into(), title: "Sync".into(), when: String::new(), note: None, people: people(&["Sam Lee"]) },
        ];
        let reply = "Tomorrow you have the **Design review** at 14:00 and Standup [2] at 09:30. Sync too.";
        let fixed = cite_mentions(reply, &cited);
        assert_eq!(fixed, "Tomorrow you have the **Design review** [1] at 14:00 and Standup [2] at 09:30. Sync too.");
        let sources = sources_for(&fixed, &cited);
        assert_eq!(sources.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(sources[0].event.as_deref(), Some("1:design"));
        // Already cited, or a word inside another: left alone.
        assert_eq!(cite_mentions("Standups [2] are short.", &cited), "Standups [2] are short.");
        // Named by someone only it has: cited after the name; someone two
        // meetings share (Sam Lee) says nothing about which.
        assert_eq!(cite_mentions("Your next one is a Teams call with Tanay Kothari at 2.", &cited), "Your next one is a Teams call with Tanay Kothari [1] at 2.");
        assert_eq!(cite_mentions("A call with Sam Lee.", &cited), "A call with Sam Lee.");
        // Any case, and words with accents around it.
        assert_eq!(cite_mentions("The design review — café first.", &cited), "The design review [1] — café first.");
        // A non-breaking hyphen (gpt-oss) is a hyphen.
        let catch_up = vec![Cited { n: 4, key: "1:b".into(), title: "Budget catch-up".into(), when: "Mon 26 Oct, 14:30–15:00".into(), note: None, people: vec![] }];
        assert_eq!(cite_mentions("2. **14:30** – Budget catch\u{2011}up with Sam", &catch_up), "2. **14:30** – Budget catch\u{2011}up [4] with Sam");
        // Named only by when it starts, no other meeting starting then.
        assert_eq!(cite_mentions("Your next meeting with Sam is on Monday at 14:30.", &catch_up), "Your next meeting with Sam is on Monday at 14:30 [4].");
        assert_eq!(cite_mentions("At 09:30 you're with Priya.", &cited), "At 09:30 [2] you're with Priya.");
    }

    #[test]
    fn a_question_about_the_calendar_for_a_model_without_tools() {
        for q in [
            "What's on tomorrow?",
            "whats on today",
            "When's my next meeting with Sam?",
            "Am I free at 3 on Friday?",
            "Do I have any meetings tomorrow?",
            "what's on my calendar this week",
            "Is Priya free on Thursday?",
            "what have I got on Monday",
        ] {
            assert!(asks_about_calendar(q), "{q}");
        }
        for q in ["What's the capital of France?", "Summarise my meeting notes from the design review", "I'm free to help", "Write a meeting agenda for the offsite"] {
            assert!(!asks_about_calendar(q), "{q}");
        }
        assert_eq!(question_args("Am I free at 3 on Friday?"), ask("friday", "3", ""));
        assert_eq!(question_args("When's my next meeting with Sam?"), ask("", "", "Sam"));
        assert_eq!(question_args("What's on tomorrow afternoon?"), ask("tomorrow", "afternoon", ""));
        assert_eq!(question_args("Anything this afternoon?"), ask("today", "afternoon", ""));
        assert_eq!(question_args("Do I have a call with Priya Shah on 30 October at 10:30?"), ask("30 october", "10:30", "Priya Shah"));
        assert_eq!(question_args("What's on next week?").day, "next week");
    }

    #[test]
    fn a_model_without_tools_reads_the_verdict_first_in_words() {
        let (agenda, z, now) = week();
        let (r, _) = run_on(&ask("Friday", "3pm", ""), &agenda, z, now);
        let text = plain(&r["data"]);
        assert!(text.starts_with("Asked about Fri 30 Oct, 15:00–15:30. The answer: NO, the user is not free then; they have:"), "{text}");
        assert!(text.contains("- [1] Roadmap planning — Fri 30 Oct, 15:00–16:00, with Priya Shah, on Teams"), "{text}");
        let (r, _) = run_on(&ask("Thursday", "15:00", ""), &agenda, z, now);
        assert!(plain(&r["data"]).contains("- Dentist — Thu 29 Oct, 15:00–16:00 (their own event, not a meeting: no number)"));
        let (r, _) = run_on(&ask("Friday", "11:00", ""), &agenda, z, now);
        assert!(plain(&r["data"]).contains("The answer: YES, the user is free then"));
        let (r, _) = run_on(&ask("today", "", ""), &agenda, z, now);
        let text = plain(&r["data"]);
        assert!(text.contains("- No meetings.") && text.contains("Their next meeting after that: [1] Standup"), "{text}");
    }

    #[test]
    fn the_tool_stays_light() {
        let est = crate::meetings::meeting_summary::estimate_tokens;
        let t = est(&definitions().to_string()) + est(&CALENDAR_TOOL_INSTRUCTIONS.join(" "));
        println!("the calendar tool adds {t} tokens");
        assert!(t <= 260, "{t} tokens");
    }
}
