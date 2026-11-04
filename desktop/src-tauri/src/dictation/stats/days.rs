//! The stats' calendar: the PC's own dates and weeks, the day and week
//! streaks, and this month so far against the same days of last month.
//! Pure but for [`today`], [`local_date`] and [`first_weekday`].

use std::collections::BTreeSet;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// A day on the PC's own calendar.
pub type Date = NaiveDate;

/// "2026-10-29": how a day is written in `dictation_stats.json`.
pub fn key(d: Date) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// A day as written ([`key`]).
pub fn parse(s: &str) -> Option<Date> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// Today, on the PC's clock and time zone.
pub fn today() -> Date {
    chrono::Local::now().date_naive()
}

/// The PC's date at unix seconds `ts`.
pub fn local_date(ts: u64) -> Date {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|t| t.with_timezone(&chrono::Local).date_naive())
        .unwrap_or_default()
}

/// The first day of `d`'s week, the week starting on `first`.
pub fn week_start(d: Date, first: Weekday) -> Date {
    let back = (d.weekday().num_days_from_monday() + 7 - first.num_days_from_monday()) % 7;
    d - Duration::days(i64::from(back))
}

/// Days in a row with a dictation, back from today — or from yesterday:
/// a day you haven't dictated in *yet* doesn't break the streak.
pub fn day_streak(active: &BTreeSet<Date>, today: Date) -> u64 {
    let mut cursor = if active.contains(&today) { today } else { today - Duration::days(1) };
    let mut streak = 0;
    while active.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }
    streak
}

/// Weeks in a row with at least one dictation: (now, the longest ever).
/// "Now" counts back from this week — or from last week, while this one
/// has none yet — as [`day_streak`] does with days.
pub fn week_streaks(active: &BTreeSet<Date>, today: Date, first: Weekday) -> (u64, u64) {
    let weeks: BTreeSet<Date> = active.iter().filter(|d| **d <= today).map(|d| week_start(*d, first)).collect();
    let mut longest = 0;
    let mut run = 0;
    let mut prev: Option<Date> = None;
    for w in &weeks {
        run = if prev.is_some_and(|p| *w - p == Duration::days(7)) { run + 1 } else { 1 };
        longest = longest.max(run);
        prev = Some(*w);
    }
    let this = week_start(today, first);
    let mut cursor = if weeks.contains(&this) { this } else { this - Duration::days(7) };
    let mut now = 0;
    while weeks.contains(&cursor) {
        now += 1;
        cursor -= Duration::days(7);
    }
    (now, longest)
}

/// This month so far, and the same days of last month (as far as that
/// month goes): `((from, to), (from, to))`, both ends included. A month
/// just begun is never set against all of the last one.
pub fn month_spans(today: Date) -> ((Date, Date), (Date, Date)) {
    let this_start = today.with_day(1).unwrap_or(today);
    let last_end_of_month = this_start - Duration::days(1);
    let last_start = last_end_of_month.with_day(1).unwrap_or(last_end_of_month);
    let same_day = last_start + Duration::days(i64::from(today.day()) - 1);
    ((this_start, today), (last_start, same_day.min(last_end_of_month)))
}

/// The day the person's week starts on: Windows' regional format (Settings
/// → Time & language → Region), Monday where it can't say.
pub fn first_weekday() -> Weekday {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_IFIRSTDAYOFWEEK};
        let mut out = [0u16; 4];
        // SAFETY: a stack buffer; Windows writes a NUL-terminated digit
        // ("0" = Monday … "6" = Sunday) and returns its length with the NUL.
        let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IFIRSTDAYOFWEEK, Some(&mut out)) };
        if n >= 2 {
            if let Some(d) = char::from_u32(u32::from(out[0])).and_then(|c| c.to_digit(10)) {
                return weekday_from_monday(d);
            }
        }
    }
    Weekday::Mon
}

/// 0 = Monday … 6 = Sunday (Windows' numbering).
pub fn weekday_from_monday(n: u32) -> Weekday {
    match n % 7 {
        0 => Weekday::Mon,
        1 => Weekday::Tue,
        2 => Weekday::Wed,
        3 => Weekday::Thu,
        4 => Weekday::Fri,
        5 => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        parse(s).unwrap()
    }
    fn set(days: &[&str]) -> BTreeSet<Date> {
        days.iter().map(|s| d(s)).collect()
    }

    #[test]
    fn days_are_written_and_read_the_same_way() {
        assert_eq!(key(d("2026-01-05")), "2026-01-05");
        assert_eq!(parse(" 2026-10-29 "), Some(d("2026-10-29")));
        assert_eq!(parse("29/10/2026"), None);
    }

    #[test]
    fn a_week_starts_on_the_persons_first_day() {
        // Thursday 29 October 2026.
        assert_eq!(week_start(d("2026-10-29"), Weekday::Mon), d("2026-10-26"));
        assert_eq!(week_start(d("2026-10-29"), Weekday::Sun), d("2026-10-25"));
        assert_eq!(week_start(d("2026-10-29"), Weekday::Sat), d("2026-10-24"));
        // The first day itself starts its own week.
        assert_eq!(week_start(d("2026-10-26"), Weekday::Mon), d("2026-10-26"));
        assert_eq!(week_start(d("2026-10-25"), Weekday::Mon), d("2026-10-19"));
        assert_eq!(weekday_from_monday(6), Weekday::Sun);
    }

    #[test]
    fn a_day_streak_survives_a_day_not_dictated_in_yet() {
        let active = set(&["2026-10-26", "2026-10-27", "2026-10-28"]);
        assert_eq!(day_streak(&active, d("2026-10-28")), 3);
        assert_eq!(day_streak(&active, d("2026-10-29")), 3, "today not dictated in yet");
        assert_eq!(day_streak(&active, d("2026-10-30")), 0, "a whole day missed");
        assert_eq!(day_streak(&BTreeSet::new(), d("2026-10-30")), 0);
    }

    #[test]
    fn week_streaks_follow_the_week_edges() {
        // Sunday 25 Oct and Monday 26 Oct: two weeks when weeks start on
        // Monday, one when they start on Sunday.
        let edge = set(&["2026-10-25", "2026-10-26"]);
        assert_eq!(week_streaks(&edge, d("2026-10-29"), Weekday::Mon), (2, 2));
        assert_eq!(week_streaks(&edge, d("2026-10-29"), Weekday::Sun), (1, 1));

        // Three weeks in a row, a gap, then two: the longest is three, and
        // now is two — still two before this week's first dictation.
        let runs = set(&[
            "2026-09-01", "2026-09-09", "2026-09-17", // weeks of 31 Aug, 7 Sep, 14 Sep
            "2026-10-13", "2026-10-21", // weeks of 12 Oct, 19 Oct
        ]);
        assert_eq!(week_streaks(&runs, d("2026-10-22"), Weekday::Mon), (2, 3));
        assert_eq!(week_streaks(&runs, d("2026-10-28"), Weekday::Mon), (2, 3), "this week not dictated in yet");
        assert_eq!(week_streaks(&runs, d("2026-11-04"), Weekday::Mon), (0, 3), "a whole week missed");

        // Across a year's end: 28 Dec 2026 and 4 Jan 2027 are a week apart.
        let new_year = set(&["2026-12-29", "2027-01-05"]);
        assert_eq!(week_streaks(&new_year, d("2027-01-06"), Weekday::Mon), (2, 2));
        // Days after today (a clock moved back) don't count.
        assert_eq!(week_streaks(&set(&["2027-03-01"]), d("2026-10-29"), Weekday::Mon), (0, 0));
    }

    #[test]
    fn this_month_is_set_against_the_same_days_of_the_last() {
        let ((a, b), (c, e)) = month_spans(d("2026-10-29"));
        assert_eq!((a, b, c, e), (d("2026-10-01"), d("2026-10-29"), d("2026-09-01"), d("2026-09-29")));
        // The 31st against a 30-day month: all of it.
        let (_, (c, e)) = month_spans(d("2026-10-31"));
        assert_eq!((c, e), (d("2026-09-01"), d("2026-09-30")));
        // January against December.
        let (_, (c, e)) = month_spans(d("2027-01-03"));
        assert_eq!((c, e), (d("2026-12-01"), d("2026-12-03")));
        // 30 March against February.
        let (_, (_, e)) = month_spans(d("2027-03-30"));
        assert_eq!(e, d("2027-02-28"));
    }
}
