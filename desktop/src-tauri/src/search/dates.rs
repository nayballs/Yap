//! **Dates in a question**: "what did Priya say about the budget last
//! month?" searches last month's meetings only. Pure (the clock is given).
//!
//! Understood, in the person's local time, weeks from Monday: today,
//! yesterday, the day before yesterday; this / last / past week, month,
//! year; the last 3 days / 2 weeks; 3 days / a week / 2 months ago; the week
//! before last; a weekday ("on Monday", "last Tuesday"); a day ("14
//! October", "October 14th", "14 Oct 2025"); a month ("in September",
//! "September 2025", "last May"); a year ("in 2025"); any of those after
//! "since", "before" or "after". "My last meeting" (latest, most recent,
//! previous; meeting, call, sync…) means the most recent meeting itself.
//! The first date found counts; its words leave the keyword search
//! ([`Dated::rest`]). Anything else — "recently", "a while back" — isn't a
//! date: the whole library is searched.

use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Weekday};
use regex::Regex;

/// A stretch of time: unix seconds `from` (in) to `to` (out), and how the
/// answer's prompt names it ("last week (Mon 13 – Sun 19 Oct 2026)").
#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub from: u64,
    pub to: u64,
    pub label: String,
}

impl Range {
    pub fn holds(&self, ts: u64) -> bool {
        ts >= self.from && ts < self.to
    }
}

/// What a question says about when.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dated {
    pub range: Option<Range>,
    /// "My last meeting": only the most recent meeting.
    pub latest: bool,
    /// The question without its date words, for the keyword search.
    pub rest: String,
}

const MONTHS: &str = "january|february|march|april|may|june|july|august|september|october|november|december|jan|feb|mar|apr|jun|jul|aug|sept|sep|oct|nov|dec";
const WEEKDAYS: &str = "monday|tuesday|wednesday|thursday|friday|saturday|sunday";
const COUNT: &str = r"\d{1,2}|a|an|one|two|three|four|five|six|seven|eight|nine|ten|couple of|few";

fn re(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("static regex")
}

static LATEST: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\b(?:(?:my|our|the)\s+)?(?:very\s+)?(?:last|latest|most\s+recent|previous|newest)\s+(?:meeting|call|standup|stand-up|sync|catch-up|catchup|1:1|one-to-one)s?\b")
});
static TODAY: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b(?:earlier\s+)?(?:today|tonight|this\s+morning|this\s+afternoon|this\s+evening)(?:'s)?\b"));
static DAY_BEFORE: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:the\s+)?day\s+before\s+yesterday\b"));
static YESTERDAY: LazyLock<Regex> = LazyLock::new(|| re(r"\byesterday(?:'s)?\b"));
static WEEK_BEFORE_LAST: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:the\s+)?week\s+before\s+last\b"));
static ROLLING: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"\b(?:in\s+|over\s+|during\s+)?(?:the\s+)?(?:last|past)\s+({COUNT})\s+(day|week|month)s?\b")));
static AGO: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"\b({COUNT})\s+(day|week|month|year)s?\s+ago\b")));
static PERIOD: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b(?:(?:in|during|over|from)\s+)?(this|last|past|previous)\s+(week|month|year)(?:'s)?\b"));
static DAY_MONTH: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(r"\b(?:on\s+)?(?:the\s+)?(\d{{1,2}})(?:st|nd|rd|th)?\s+(?:of\s+)?({MONTHS})\.?(?:,?\s+(\d{{4}}))?\b"))
});
static MONTH_DAY: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"\b(?:on\s+)?({MONTHS})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?(?:,?\s+(\d{{4}}))?\b")));
static MONTH: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"\b(?:(in|during|over|from|of|for|since|before|after|last|this)\s+)?(?:(?:early|late|mid-?)\s*|the\s+(?:start|end|beginning|middle)\s+of\s+)?({MONTHS})\.?(?:\s+(\d{{4}}))?\b"
    ))
});
static WEEKDAY: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"\b(?:(last|this|on|past)\s+)?({WEEKDAYS})(?:'s)?\b")));
static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\b(in|during|from|since|before|after)\s+(20\d\d)\b"));
static MODIFIER: LazyLock<Regex> = LazyLock::new(|| re(r"\b(since|before|after)\s*$"));

fn month_of(s: &str) -> Option<u32> {
    let s = s.to_lowercase();
    let i = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| s.starts_with(m))?;
    Some(i as u32 + 1)
}

fn weekday_of(s: &str) -> Option<Weekday> {
    Some(match s.to_lowercase().get(..3)? {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

fn count_of(s: &str) -> u32 {
    let s = s.to_lowercase();
    match s.trim() {
        "a" | "an" | "one" => 1,
        "two" | "couple of" => 2,
        "three" | "few" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        n => n.parse().unwrap_or(1),
    }
}

/// Midnight starting `d`, in `tz`, as unix seconds.
fn start_of<Tz: TimeZone>(tz: &Tz, d: NaiveDate) -> u64 {
    let naive = d.and_hms_opt(0, 0, 0).expect("midnight");
    let t = tz.from_local_datetime(&naive).earliest().map(|t| t.timestamp()).unwrap_or_else(|| naive.and_utc().timestamp());
    t.max(0) as u64
}

fn first_of_month(year: i32, month: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, 1).expect("a month")
}

fn next_month(d: NaiveDate) -> NaiveDate {
    if d.month() == 12 {
        first_of_month(d.year() + 1, 1)
    } else {
        first_of_month(d.year(), d.month() + 1)
    }
}

fn months_back(d: NaiveDate, n: u32) -> NaiveDate {
    let total = d.year() * 12 + d.month() as i32 - 1 - n as i32;
    first_of_month(total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn day_label(d: NaiveDate) -> String {
    d.format("%a %-d %b %Y").to_string()
}

fn span_label(from: NaiveDate, to_incl: NaiveDate) -> String {
    if from == to_incl {
        day_label(from)
    } else if from.year() == to_incl.year() && from.month() == to_incl.month() {
        format!("{} – {}", from.format("%a %-d"), day_label(to_incl))
    } else if from.year() == to_incl.year() {
        format!("{} – {}", from.format("%a %-d %b"), day_label(to_incl))
    } else {
        format!("{} – {}", day_label(from), day_label(to_incl))
    }
}

/// Days `from` (in) to `to` (out) as a [`Range`], named `what`.
fn days<Tz: TimeZone>(tz: &Tz, what: &str, from: NaiveDate, to: NaiveDate) -> Range {
    let last = to - Duration::days(1);
    Range { from: start_of(tz, from), to: start_of(tz, to), label: format!("{what} ({})", span_label(from, last)) }
}

/// What a question says about when (`now`: the clock, in the person's
/// time zone). Pure.
pub fn parse<Tz: TimeZone>(question: &str, now: &DateTime<Tz>) -> Dated {
    let q = question.replace('’', "'");
    let mut out = Dated { rest: q.clone(), ..Default::default() };
    if let Some(m) = LATEST.find(&q) {
        out.latest = true;
        out.rest = cut(&q, m.start(), m.end());
        return out;
    }
    let tz = now.timezone();
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    // Each kind of date in turn: its match and the range it names.
    let found: Option<(usize, usize, Range)> = (|| {
        if let Some(m) = TODAY.find(&q) {
            return Some((m.start(), m.end(), days(&tz, "today", today, tomorrow)));
        }
        if let Some(m) = DAY_BEFORE.find(&q) {
            let d = today - Duration::days(2);
            return Some((m.start(), m.end(), days(&tz, "the day before yesterday", d, d + Duration::days(1))));
        }
        if let Some(m) = YESTERDAY.find(&q) {
            return Some((m.start(), m.end(), days(&tz, "yesterday", today - Duration::days(1), today)));
        }
        if let Some(m) = WEEK_BEFORE_LAST.find(&q) {
            let from = monday_of(today) - Duration::days(14);
            return Some((m.start(), m.end(), days(&tz, "the week before last", from, from + Duration::days(7))));
        }
        if let Some(c) = ROLLING.captures(&q) {
            let m = c.get(0)?;
            let n = count_of(&c[1]).max(1);
            let unit = c[2].to_lowercase();
            let from = match unit.as_str() {
                "day" => today - Duration::days(n as i64 - 1),
                "week" => today - Duration::days(7 * n as i64 - 1),
                _ => months_back(today, n) + Duration::days(today.day() as i64 - 1),
            };
            let what = format!("the last {n} {unit}{}", if n == 1 { "" } else { "s" });
            return Some((m.start(), m.end(), days(&tz, &what, from, tomorrow)));
        }
        if let Some(c) = AGO.captures(&q) {
            let m = c.get(0)?;
            let n = count_of(&c[1]).max(1);
            let unit = c[2].to_lowercase();
            let what = format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
            let range = match unit.as_str() {
                "day" => {
                    let d = today - Duration::days(n as i64);
                    days(&tz, &what, d, d + Duration::days(1))
                }
                "week" => {
                    let from = monday_of(today) - Duration::days(7 * n as i64);
                    days(&tz, &what, from, from + Duration::days(7))
                }
                "month" => {
                    let from = months_back(first_of_month(today.year(), today.month()), n);
                    days(&tz, &what, from, next_month(from))
                }
                _ => {
                    let y = today.year() - n as i32;
                    days(&tz, &what, first_of_month(y, 1), first_of_month(y + 1, 1))
                }
            };
            return Some((m.start(), m.end(), range));
        }
        if let Some(c) = PERIOD.captures(&q) {
            let m = c.get(0)?;
            let which = c[1].to_lowercase();
            let unit = c[2].to_lowercase();
            let this = which == "this";
            let range = match (unit.as_str(), this) {
                ("week", true) => days(&tz, "this week", monday_of(today), monday_of(today) + Duration::days(7)),
                ("week", false) if which == "past" => days(&tz, "the past week", today - Duration::days(6), tomorrow),
                ("week", false) => {
                    let from = monday_of(today) - Duration::days(7);
                    days(&tz, "last week", from, from + Duration::days(7))
                }
                ("month", true) => {
                    let from = first_of_month(today.year(), today.month());
                    days(&tz, "this month", from, next_month(from))
                }
                ("month", false) if which == "past" => days(&tz, "the past month", today - Duration::days(30), tomorrow),
                ("month", false) => {
                    let from = months_back(first_of_month(today.year(), today.month()), 1);
                    days(&tz, "last month", from, next_month(from))
                }
                (_, true) => days(&tz, "this year", first_of_month(today.year(), 1), first_of_month(today.year() + 1, 1)),
                (_, false) if which == "past" => days(&tz, "the past year", today - Duration::days(365), tomorrow),
                (_, false) => days(&tz, "last year", first_of_month(today.year() - 1, 1), first_of_month(today.year(), 1)),
            };
            return Some((m.start(), m.end(), range));
        }
        for (pattern, day_first) in [(&*DAY_MONTH, true), (&*MONTH_DAY, false)] {
            for c in pattern.captures_iter(&q) {
                let m = c.get(0)?;
                let (day, month) = if day_first { (&c[1], &c[2]) } else { (&c[2], &c[1]) };
                let (Some(month), Ok(day)) = (month_of(month), day.parse::<u32>()) else { continue };
                // "may" and "march" alone are words, not months.
                let year = c.get(3).and_then(|y| y.as_str().parse::<i32>().ok());
                let date = match year {
                    Some(y) => NaiveDate::from_ymd_opt(y, month, day),
                    None => NaiveDate::from_ymd_opt(today.year(), month, day)
                        .filter(|d| *d <= today)
                        .or_else(|| NaiveDate::from_ymd_opt(today.year() - 1, month, day)),
                };
                if let Some(d) = date {
                    let what = d.format("%-d %B").to_string();
                    return Some((m.start(), m.end(), days(&tz, &what, d, d + Duration::days(1))));
                }
            }
        }
        for c in MONTH.captures_iter(&q) {
            let m = c.get(0)?;
            let word = c[2].to_lowercase();
            let lead = c.get(1).map(|l| l.as_str().to_lowercase());
            let year = c.get(3).and_then(|y| y.as_str().parse::<i32>().ok());
            // "may" and "march" are words too, "June" and "April" names, "Jan"
            // a name and an abbreviation: those only with a lead-in or a year.
            let plain = matches!(word.as_str(), "january" | "february" | "july" | "september" | "october" | "november" | "december");
            if !plain && lead.is_none() && year.is_none() {
                continue;
            }
            let Some(month) = month_of(&word) else { continue };
            let y = match (year, lead.as_deref()) {
                (Some(y), _) => y,
                (None, Some("last")) if month >= today.month() => today.year() - 1,
                (None, Some("this")) => today.year(),
                (None, _) if month > today.month() => today.year() - 1,
                (None, _) => today.year(),
            };
            let from = first_of_month(y, month);
            let label = from.format("%B %Y").to_string();
            let range = Range { from: start_of(&tz, from), to: start_of(&tz, next_month(from)), label };
            // "this"/"last" belong to the date; "since"/"before"/"after" are
            // the modifier's, read below.
            let start = match lead.as_deref() {
                Some("since" | "before" | "after") => c.get(2)?.start(),
                _ => m.start(),
            };
            return Some((start, m.end(), range));
        }
        if let Some(c) = WEEKDAY.captures(&q) {
            let m = c.get(0)?;
            let wd = weekday_of(&c[2])?;
            let lead = c.get(1).map(|l| l.as_str().to_lowercase());
            let back = (today.weekday().num_days_from_monday() as i64 - wd.num_days_from_monday() as i64).rem_euclid(7);
            let d = match lead.as_deref() {
                // "last Monday": the latest Monday before today.
                Some("last" | "past") => today - Duration::days(if back == 0 { 7 } else { back }),
                // "this Monday": this week's.
                Some("this") => monday_of(today) + Duration::days(wd.num_days_from_monday() as i64),
                // "on Monday", "Monday": the latest, today's included.
                _ => today - Duration::days(back),
            };
            let what = d.format("%A").to_string();
            return Some((m.start(), m.end(), days(&tz, &what, d, d + Duration::days(1))));
        }
        if let Some(c) = YEAR.captures(&q) {
            let y: i32 = c[2].parse().ok()?;
            let lead = c[1].to_lowercase();
            let start = if matches!(lead.as_str(), "since" | "before" | "after") { c.get(2)?.start() } else { c.get(0)?.start() };
            let range = Range {
                from: start_of(&tz, first_of_month(y, 1)),
                to: start_of(&tz, first_of_month(y + 1, 1)),
                label: y.to_string(),
            };
            return Some((start, c.get(0)?.end(), range));
        }
        None
    })();
    let Some((mut start, end, mut range)) = found else { return out };
    // "since", "before" or "after" just before it.
    if let Some(m) = MODIFIER.captures(&q[..start]) {
        let word = m[1].to_lowercase();
        let end_of_today = start_of(&tz, tomorrow);
        range = match word.as_str() {
            "since" => Range { from: range.from, to: end_of_today, label: format!("since {}", range.label) },
            "before" => Range { from: 0, to: range.from, label: format!("before {}", range.label) },
            _ => Range { from: range.to, to: end_of_today, label: format!("after {}", range.label) },
        };
        start = m.get(0).map_or(start, |w| w.start());
    }
    out.rest = cut(&q, start, end);
    out.range = Some(range);
    out
}

/// `q` without `q[start..end]`, its spaces tidied.
fn cut(q: &str, start: usize, end: usize) -> String {
    format!("{} {}", &q[..start], &q[end..]).split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone};

    /// Sunday 25 October 2026, 15:00, an hour ahead of UTC.
    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600).unwrap().with_ymd_and_hms(2026, 10, 25, 15, 0, 0).unwrap()
    }

    fn day(y: i32, m: u32, d: u32) -> u64 {
        FixedOffset::east_opt(3600).unwrap().with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap().timestamp() as u64
    }

    fn range(q: &str) -> (u64, u64, String) {
        let r = parse(q, &now()).range.unwrap_or_else(|| panic!("no date in {q:?}"));
        (r.from, r.to, r.label)
    }

    #[test]
    fn days_and_weeks() {
        assert_eq!(range("what did we decide today?"), (day(2026, 10, 25), day(2026, 10, 26), "today (Sun 25 Oct 2026)".into()));
        assert_eq!(range("yesterday's standup").0, day(2026, 10, 24));
        assert_eq!(range("the day before yesterday").0, day(2026, 10, 23));
        // Weeks start on Monday: this week is Mon 19 – Sun 25.
        assert_eq!(range("this week").0, day(2026, 10, 19));
        let (from, to, label) = range("what did Priya say last week?");
        assert_eq!((from, to), (day(2026, 10, 12), day(2026, 10, 19)));
        assert_eq!(label, "last week (Mon 12 – Sun 18 Oct 2026)");
        assert_eq!(range("the week before last").0, day(2026, 10, 5));
        assert_eq!(range("in the past week").0, day(2026, 10, 19));
        assert_eq!(range("over the last 3 days"), (day(2026, 10, 23), day(2026, 10, 26), "the last 3 days (Fri 23 – Sun 25 Oct 2026)".into()));
        assert_eq!(range("two weeks ago").0, day(2026, 10, 5));
        assert_eq!(range("5 days ago").0, day(2026, 10, 20));
    }

    #[test]
    fn months_and_years() {
        let (from, to, label) = range("what did Priya say about the budget last month?");
        assert_eq!((from, to, label.as_str()), (day(2026, 9, 1), day(2026, 10, 1), "last month (Tue 1 – Wed 30 Sep 2026)"));
        assert_eq!(range("this month").0, day(2026, 10, 1));
        assert_eq!(range("in September"), (day(2026, 9, 1), day(2026, 10, 1), "September 2026".into()));
        // A month still to come this year means last year's.
        assert_eq!(range("in December").0, day(2025, 12, 1));
        assert_eq!(range("Dec 2024").0, day(2024, 12, 1));
        assert_eq!(range("the hiring plan from early March").0, day(2026, 3, 1));
        assert_eq!(range("last October").0, day(2025, 10, 1));
        assert_eq!(range("a month ago").0, day(2026, 9, 1));
        assert_eq!(range("this year").0, day(2026, 1, 1));
        assert_eq!(range("last year"), (day(2025, 1, 1), day(2026, 1, 1), "last year (Wed 1 Jan – Wed 31 Dec 2025)".into()));
        assert_eq!(range("the roadmap in 2025").0, day(2025, 1, 1));
    }

    #[test]
    fn a_day_by_its_date_or_its_name() {
        assert_eq!(range("on 14 October").0, day(2026, 10, 14));
        assert_eq!(range("October 14th, 2025").0, day(2025, 10, 14));
        assert_eq!(range("the 3rd of March").0, day(2026, 3, 3));
        // Later in the year than today: last year's.
        assert_eq!(range("on 2 November").0, day(2025, 11, 2));
        // Sunday 25 Oct: Monday = the 19th; "last Sunday" = the 18th; "on
        // Sunday" = today.
        assert_eq!(range("what happened on Monday").0, day(2026, 10, 19));
        assert_eq!(range("last Sunday").0, day(2026, 10, 18));
        assert_eq!(range("on Sunday").0, day(2026, 10, 25));
        assert_eq!(range("Friday's call").0, day(2026, 10, 23));
    }

    #[test]
    fn since_before_and_after() {
        let (from, to, label) = range("everything about pricing since September");
        assert_eq!((from, to), (day(2026, 9, 1), day(2026, 10, 26)));
        assert_eq!(label, "since September 2026");
        let (from, to, _) = range("before last week");
        assert_eq!((from, to), (0, day(2026, 10, 12)));
        assert_eq!(range("after 14 October").0, day(2026, 10, 15));
        assert_eq!(range("since 2025").0, day(2025, 1, 1));
    }

    #[test]
    fn the_last_meeting_and_no_date() {
        let d = parse("what did we decide in my last meeting?", &now());
        assert!(d.latest && d.range.is_none());
        assert_eq!(d.rest, "what did we decide in ?");
        assert!(parse("summarise the most recent call", &now()).latest);
        // Not dates: "may" the word, "recently", a budget's numbers.
        for q in ["may I see the budget", "what did we say recently", "the march to production", "the 2026 budget was £400"] {
            let d = parse(q, &now());
            assert_eq!(d.range, None, "{q}");
            assert!(!d.latest);
            assert_eq!(d.rest, q);
        }
    }

    #[test]
    fn the_date_words_leave_the_keyword_search() {
        assert_eq!(parse("What did Priya say about the budget last month?", &now()).rest, "What did Priya say about the budget ?");
        assert_eq!(parse("pricing since September", &now()).rest, "pricing");
        assert_eq!(parse("the launch date in the last 2 weeks", &now()).rest, "the launch date");
    }

    #[test]
    fn ranges_hold_their_start_not_their_end() {
        let r = Range { from: 10, to: 20, label: String::new() };
        assert!(r.holds(10) && r.holds(19) && !r.holds(20) && !r.holds(9));
    }
}
