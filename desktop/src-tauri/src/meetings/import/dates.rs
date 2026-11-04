//! When an imported meeting was: the dates transcript files carry, in their
//! names (Zoom's `GMT20261005-143012`, Teams' `-20261005_143012-`, a
//! recording folder's `2026-10-05 14.30.12`) and in their header lines
//! (Teams' "October 5, 2026, 2:30PM", Otter's "Mon, Oct 5, 2026 2:30PM •
//! 32:15", Google Meet's "(2026-10-05 14:30 GMT+01:00)", Granola's "Date:
//! 5 Oct 2026"). English month names only; a date written with slashes
//! (05/10/2026) is left alone, since it reads two ways. Pure.

use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;

/// A date a file says, its time when it says one, and its UTC offset when
/// it says that (`None`: this PC's time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dated {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    /// Minutes east of UTC (Zoom's "GMT" names: 0).
    pub offset_min: Option<i32>,
}

impl Dated {
    /// Unix seconds, at `time` when the file gave none; `local` turns a
    /// local date and time into unix seconds (this PC's zone; a test's own).
    pub fn unix(&self, fallback: NaiveTime, local: impl Fn(NaiveDateTime) -> Option<i64>) -> Option<i64> {
        let at = self.date.and_time(self.time.unwrap_or(fallback));
        match self.offset_min {
            Some(off) => Some(at.and_utc().timestamp() - i64::from(off) * 60),
            None => local(at),
        }
    }
}

const MONTHS: &str = r"jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?";

static ZOOM_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"GMT((?:19|20)\d{2})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})").unwrap());
static PACKED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\D)((?:19|20)\d{2})(\d{2})(\d{2})(?:[_T\-](\d{2})(\d{2})(\d{2})?)?(?:\D|$)").unwrap());
static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\D)((?:19|20)\d{2})-(\d{2})-(\d{2})(?:[ T_]+(\d{1,2})[.:\-](\d{2})(?:[.:\-](\d{2}))?)?").unwrap()
});
static MONTH_DAY_YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\b({MONTHS})\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?,?\s+((?:19|20)\d{{2}})\b")).unwrap()
});
static DAY_MONTH_YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\b(\d{{1,2}})(?:st|nd|rd|th)?\s+(?:of\s+)?({MONTHS})\.?,?\s+((?:19|20)\d{{2}})\b")).unwrap()
});
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d{1,2})[:.](\d{2})(?:[:.](\d{2}))?(?:\s*([ap])\.?\s?m\b\.?)?|\b(\d{1,2})\s*([ap])\.?m\b\.?").unwrap()
});
static OFFSET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:GMT|UTC)\s*(?:([+\-−])\s*(\d{1,2})(?::?(\d{2}))?)?").unwrap());

fn month_of(name: &str) -> Option<u32> {
    let m = name.to_ascii_lowercase();
    let n = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|p| m.starts_with(p))?;
    Some(n as u32 + 1)
}

fn num(s: Option<regex::Match<'_>>) -> Option<u32> {
    s?.as_str().parse().ok()
}

fn time_of(h: Option<u32>, m: Option<u32>, s: Option<u32>) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(h?, m.unwrap_or(0), s.unwrap_or(0))
}

/// The date in a file's name: Zoom's GMT stamp (in UTC), a packed
/// `20261005_143012` or an ISO `2026-10-05 14.30.12` (this PC's time), or a
/// date written out ("Weekly sync 5 October 2026").
pub fn in_name(name: &str) -> Option<Dated> {
    if let Some(c) = ZOOM_NAME.captures(name) {
        let date = NaiveDate::from_ymd_opt(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)?;
        let time = time_of(num(c.get(4)), num(c.get(5)), num(c.get(6)));
        return Some(Dated { date, time, offset_min: Some(0) });
    }
    if let Some(d) = iso(name) {
        return Some(d);
    }
    for c in PACKED.captures_iter(name) {
        let Some(date) = NaiveDate::from_ymd_opt(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?) else {
            continue;
        };
        let time = time_of(num(c.get(4)), num(c.get(5)), num(c.get(6)));
        return Some(Dated { date, time, offset_min: None });
    }
    in_line(name)
}

fn iso(s: &str) -> Option<Dated> {
    let c = ISO.captures(s)?;
    let date = NaiveDate::from_ymd_opt(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?)?;
    let time = time_of(num(c.get(4)), num(c.get(5)), num(c.get(6)));
    let rest = &s[c.get(0)?.end()..];
    Some(Dated { date, time, offset_min: time.and(offset_in(rest)) })
}

/// A UTC offset said just after a date or time ("GMT+01:00", "UTC",
/// "GMT-5").
fn offset_in(rest: &str) -> Option<i32> {
    let near: String = rest.chars().take(24).collect();
    let c = OFFSET.captures(&near)?;
    let Some(h) = num(c.get(2)) else {
        return Some(0);
    };
    let minutes = h as i32 * 60 + num(c.get(3)).unwrap_or(0) as i32;
    let sign = if matches!(c.get(1).map(|m| m.as_str()), Some("-") | Some("−")) { -1 } else { 1 };
    (minutes <= 14 * 60).then_some(sign * minutes)
}

/// The first time of day in `rest` (what follows a date on its line):
/// "2:30PM", "14:30", "2 pm". A duration that can't be a time ("32:15")
/// isn't one.
fn clock_in(rest: &str) -> Option<NaiveTime> {
    for c in CLOCK.captures_iter(rest) {
        let (h, m, s, ap) = match c.get(1) {
            Some(_) => (num(c.get(1)), num(c.get(2)), num(c.get(3)), c.get(4)),
            None => (num(c.get(5)), Some(0), None, c.get(6)),
        };
        let Some(mut h) = h else { continue };
        if let Some(ap) = ap {
            if !(1..=12).contains(&h) {
                continue;
            }
            let pm = ap.as_str().eq_ignore_ascii_case("p");
            h = match (h, pm) {
                (12, false) => 0,
                (12, true) => 12,
                (h, true) => h + 12,
                (h, false) => h,
            };
        }
        if let Some(t) = time_of(Some(h), m, s) {
            return Some(t);
        }
    }
    None
}

/// The date on a header line, with the time after it when there's one:
/// "October 5, 2026, 2:30PM", "Mon, Oct 5, 2026 2:30PM • 32:15", "5
/// October 2026, 14:30", "2026-10-05 14:30 GMT+01:00".
pub fn in_line(line: &str) -> Option<Dated> {
    if let Some(d) = iso(line) {
        return Some(d);
    }
    let (date, end) = match MONTH_DAY_YEAR.captures(line) {
        Some(c) => {
            let date = NaiveDate::from_ymd_opt(c[3].parse().ok()?, month_of(&c[1])?, c[2].parse().ok()?)?;
            (date, c.get(0)?.end())
        }
        None => {
            let c = DAY_MONTH_YEAR.captures(line)?;
            let date = NaiveDate::from_ymd_opt(c[3].parse().ok()?, month_of(&c[2])?, c[1].parse().ok()?)?;
            (date, c.get(0)?.end())
        }
    };
    let rest = &line[end..];
    let time = clock_in(rest);
    Some(Dated { date, time, offset_min: time.and(offset_in(rest)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }
    fn hm(h: u32, m: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, m, 0)
    }

    #[test]
    fn dates_in_file_names() {
        let zoom = in_name("GMT20261005-143012_Recording.transcript.vtt").unwrap();
        assert_eq!(zoom.date, ymd(2026, 10, 5));
        assert_eq!(zoom.time, NaiveTime::from_hms_opt(14, 30, 12));
        assert_eq!(zoom.offset_min, Some(0));
        let teams = in_name("Weekly sync-20261005_143012-Meeting Recording.vtt").unwrap();
        assert_eq!((teams.date, teams.time, teams.offset_min), (ymd(2026, 10, 5), NaiveTime::from_hms_opt(14, 30, 12), None));
        let folder = in_name("2026-10-05 14.30.12 Supplier call.txt").unwrap();
        assert_eq!((folder.date, folder.time), (ymd(2026, 10, 5), NaiveTime::from_hms_opt(14, 30, 12)));
        assert_eq!(in_name("notes 2026-10-05.md").unwrap().time, None);
        assert_eq!(in_name("Design review 5 October 2026.docx").unwrap().date, ymd(2026, 10, 5));
        // Not dates: a phone-like number, an impossible day, nothing.
        assert_eq!(in_name("call 20261345.vtt"), None);
        assert_eq!(in_name("transcript.vtt"), None);
    }

    #[test]
    fn dates_on_header_lines() {
        let teams = in_line("October 5, 2026, 2:30PM").unwrap();
        assert_eq!((teams.date, teams.time), (ymd(2026, 10, 5), hm(14, 30)));
        let otter = in_line("Mon, Oct 5, 2026 2:30PM • 32:15").unwrap();
        assert_eq!((otter.date, otter.time), (ymd(2026, 10, 5), hm(14, 30)));
        let british = in_line("Monday 5th October 2026, 09:05").unwrap();
        assert_eq!((british.date, british.time), (ymd(2026, 10, 5), hm(9, 5)));
        let meet = in_line("Supplier call (2026-10-05 14:30 GMT+01:00) - Transcript").unwrap();
        assert_eq!((meet.date, meet.time, meet.offset_min), (ymd(2026, 10, 5), hm(14, 30), Some(60)));
        let noon = in_line("Date: Oct 5, 2026 at 12 pm").unwrap();
        assert_eq!(noon.time, hm(12, 0));
        let midnight = in_line("5 Oct 2026 12:10 a.m.").unwrap();
        assert_eq!(midnight.time, hm(0, 10));
        // A duration is never the time.
        assert_eq!(in_line("Oct 5, 2026 • 32:15").unwrap().time, None);
        assert_eq!(in_line("We meet on the fifth"), None);
        assert_eq!(in_line("05/10/2026"), None);
    }

    #[test]
    fn to_unix_in_utc_or_the_local_zone() {
        let utc = Dated { date: ymd(2026, 10, 5), time: hm(14, 30), offset_min: Some(60) };
        // 14:30 at UTC+1 is 13:30 UTC.
        assert_eq!(utc.unix(NaiveTime::MIN, |_| None), Some(1_791_207_000));
        let local = Dated { date: ymd(2026, 10, 5), time: None, offset_min: None };
        let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        assert_eq!(local.unix(noon, |t| Some(t.and_utc().timestamp())), Some(1_791_201_600));
    }
}
