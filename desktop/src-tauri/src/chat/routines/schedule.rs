//! **When a routine is due**, pure: every function here takes the clock
//! (unix seconds and a time zone) rather than reading it, so the unit tests
//! walk schedules across days, weekends, a clock change and a PC that was
//! off.
//!
//! - **A schedule** ([`Schedule`]): every day, on weekdays (Monday to
//!   Friday) or once a week, at a time of day in the PC's own time zone.
//!   Its slots are [`latest_slot`] (the one to run now, if any) and
//!   [`next_slot`] (Settings' "Next: …").
//! - **Missed runs** ([`due`]): only the latest slot since the last one
//!   handled counts. Up to [`ON_TIME_SECS`] late it's on time; up to
//!   [`GRACE_SECS`] late (the PC was off or asleep, Yap wasn't running) it
//!   runs once, late; later than that it's skipped, and the runs list says
//!   so. Slots before it are never run.
//! - **Before a calendar meeting** ([`events_due`]): each meeting starting
//!   within the lead, once — never one that has started (Yap opened after
//!   its start: too late to prepare), nor an all-day event or a block with
//!   nobody invited and no call link.
//! - **After a meeting** ([`meeting_matches`]): any, or only meetings in a
//!   folder, or with someone from outside the person's own email domains.
//! - **The daily cap** ([`runs_today`], [`DAILY_CAP`]): every run that
//!   started today, whatever started it; a skipped one isn't counted.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Serialize};

/// A slot found up to this late is on time (the scheduler ticks every
/// half minute; a busy PC may be slower).
pub const ON_TIME_SECS: i64 = 5 * 60;
/// **A missed run** (the PC off or asleep at its time) still runs once when
/// Yap finds it up to this late; later, it's skipped.
pub const GRACE_SECS: i64 = 3 * 60 * 60;
/// **Runs a day at most**, every routine together (Run now included).
pub const DAILY_CAP: usize = 20;
/// A calendar meeting this many minutes ahead at most ("10 minutes before").
pub const LEAD_MAX: u32 = 60;
/// An event this long or longer is a day's (a holiday, a trip), not a meeting.
const ALL_DAY_SECS: i64 = 20 * 60 * 60;

/// Which days a schedule runs on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Days {
    #[default]
    Daily,
    /// Monday to Friday.
    Weekdays,
    /// One day a week (`Schedule::weekday`).
    Weekly,
}

/// A time of day on some days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub days: Days,
    /// For [`Days::Weekly`]: 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    pub hour: u32,
    pub minute: u32,
}

/// "09:00" → (9, 0); anything else `None`. Pure.
pub fn parse_time(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

impl Schedule {
    /// Whether it runs on `d`.
    fn on(&self, d: NaiveDate) -> bool {
        match self.days {
            Days::Daily => true,
            Days::Weekdays => !matches!(d.weekday(), Weekday::Sat | Weekday::Sun),
            Days::Weekly => d.weekday().num_days_from_monday() == u32::from(self.weekday.min(6)),
        }
    }

    /// Its time on `d` in `tz`, as unix seconds: on a day the clocks go
    /// forward through it, the first moment after the gap; on a day they go
    /// back, the first of the two.
    fn at<Tz: TimeZone>(&self, d: NaiveDate, tz: &Tz) -> Option<i64> {
        let t = NaiveTime::from_hms_opt(self.hour, self.minute, 0)?;
        let naive = d.and_time(t);
        if let Some(dt) = tz.from_local_datetime(&naive).earliest() {
            return Some(dt.timestamp());
        }
        // In the gap (02:30 on the night the clocks go forward): an hour on.
        tz.from_local_datetime(&(naive + Duration::hours(1))).earliest().map(|dt| dt.timestamp())
    }
}

/// The local date of unix second `t` in `tz`.
fn date_of<Tz: TimeZone>(t: i64, tz: &Tz) -> Option<NaiveDate> {
    Some(DateTime::from_timestamp(t, 0)?.with_timezone(tz).date_naive())
}

/// **The latest of the schedule's times in (`after`, `upto`]**, if any: the
/// one to run (or skip) now. Pure.
pub fn latest_slot<Tz: TimeZone>(s: &Schedule, after: i64, upto: i64, tz: &Tz) -> Option<i64> {
    if upto <= after {
        return None;
    }
    let mut d = date_of(upto, tz)?;
    // (A week and a day back finds a weekly one; nothing older matters.)
    for _ in 0..9 {
        if s.on(d) {
            if let Some(slot) = s.at(d, tz) {
                if slot <= after {
                    return None;
                }
                if slot <= upto {
                    return Some(slot);
                }
            }
        }
        d = d.pred_opt()?;
    }
    None
}

/// **The schedule's next time after `t`**: Settings' "Next: …". Pure.
pub fn next_slot<Tz: TimeZone>(s: &Schedule, t: i64, tz: &Tz) -> Option<i64> {
    let mut d = date_of(t, tz)?;
    for _ in 0..9 {
        if s.on(d) {
            if let Some(slot) = s.at(d, tz).filter(|&slot| slot > t) {
                return Some(slot);
            }
        }
        d = d.succ_opt()?;
    }
    None
}

/// What a schedule's latest slot calls for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// Nothing since the last slot handled.
    Not,
    /// Run it: on time, or `late` (missed, within [`GRACE_SECS`]).
    Run { slot: i64, late: bool },
    /// Missed by more than [`GRACE_SECS`]: skipped, and said so.
    Skip { slot: i64 },
}

/// **What the schedule calls for at `now`**, its last slot handled at
/// `handled` (a routine made or switched on: the moment it was, so nothing
/// before it runs). Pure.
pub fn due<Tz: TimeZone>(s: &Schedule, handled: i64, now: i64, tz: &Tz) -> Due {
    match latest_slot(s, handled, now, tz) {
        None => Due::Not,
        Some(slot) if now - slot <= ON_TIME_SECS => Due::Run { slot, late: false },
        Some(slot) if now - slot <= GRACE_SECS => Due::Run { slot, late: true },
        Some(slot) => Due::Skip { slot },
    }
}

/// A calendar meeting as [`events_due`] reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Upcoming<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub start: i64,
    pub end: i64,
    /// Someone's invited, or it has a call link: a meeting, not a block.
    pub meeting: bool,
}

/// **The meetings to run a "before" routine for at `now`**: starting within
/// `lead_min` minutes, not started yet, a meeting (not a day's event or a
/// block of time), and not run for already (`done`, by event key). Soonest
/// first. Pure.
pub fn events_due<'a>(events: &[Upcoming<'a>], lead_min: u32, now: i64, done: &[String]) -> Vec<Upcoming<'a>> {
    let lead = i64::from(lead_min.clamp(1, LEAD_MAX)) * 60;
    let mut out: Vec<Upcoming<'a>> = events
        .iter()
        .filter(|e| e.meeting && e.start > now && e.start - now <= lead && e.end - e.start < ALL_DAY_SECS && !done.iter().any(|d| d == e.key))
        .cloned()
        .collect();
    out.sort_by_key(|e| e.start);
    out
}

/// **Does a meeting that ended call for an "after" routine?** `folder`: only
/// meetings in that folder ("": any); `outside`: only meetings with someone
/// from outside the person's own email domains (`companies`: the meeting's
/// other companies). Both set: both. Pure.
pub fn meeting_matches(folder: &str, outside: bool, note_folder: &str, companies: &[String]) -> bool {
    let folder = folder.trim();
    (folder.is_empty() || folder.eq_ignore_ascii_case(note_folder.trim())) && (!outside || !companies.is_empty())
}

/// **How many runs started today** (`now`'s date in `tz`), from each run's
/// start (`started`, 0: never started — skipped). Pure.
pub fn runs_today<Tz: TimeZone>(started: impl IntoIterator<Item = i64>, now: i64, tz: &Tz) -> usize {
    let Some(today) = date_of(now, tz) else { return 0 };
    started.into_iter().filter(|&t| t > 0 && date_of(t, tz) == Some(today)).count()
}

/// Whether today's runs (`today`) reached the cap.
pub fn over_cap(today: usize) -> bool {
    today >= DAILY_CAP
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    /// 2026-11-16 is a Monday.
    fn at(day: u32, h: u32, m: u32) -> i64 {
        Utc.with_ymd_and_hms(2026, 11, day, h, m, 0).unwrap().timestamp()
    }
    fn every(days: Days, weekday: u8, hour: u32, minute: u32) -> Schedule {
        Schedule { days, weekday, hour, minute }
    }

    #[test]
    fn times_parse() {
        assert_eq!(parse_time("09:00"), Some((9, 0)));
        assert_eq!(parse_time("9:05"), Some((9, 5)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
        for bad in ["24:00", "12:60", "12", "12:5", "ab:cd", "", "123:00"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_daily_schedule_runs_once_a_day_on_time() {
        let s = every(Days::Daily, 0, 9, 0);
        // Made at 08:00: due at 09:00, on time for five minutes.
        let made = at(16, 8, 0);
        assert_eq!(due(&s, made, at(16, 8, 59), &Utc), Due::Not);
        assert_eq!(due(&s, made, at(16, 9, 0), &Utc), Due::Run { slot: at(16, 9, 0), late: false });
        assert_eq!(due(&s, made, at(16, 9, 4), &Utc), Due::Run { slot: at(16, 9, 0), late: false });
        // Handled: not again that day.
        assert_eq!(due(&s, at(16, 9, 0), at(16, 23, 0), &Utc), Due::Not);
        assert_eq!(due(&s, at(16, 9, 0), at(17, 9, 0), &Utc), Due::Run { slot: at(17, 9, 0), late: false });
        // Made after today's time: tomorrow's is the first.
        assert_eq!(due(&s, at(16, 10, 0), at(16, 12, 0), &Utc), Due::Not);
        assert_eq!(next_slot(&s, at(16, 10, 0), &Utc), Some(at(17, 9, 0)));
        assert_eq!(next_slot(&s, at(16, 8, 0), &Utc), Some(at(16, 9, 0)));
    }

    #[test]
    fn weekdays_skip_the_weekend_and_weekly_its_one_day() {
        let s = every(Days::Weekdays, 0, 9, 0);
        // Friday 20 Nov at 10:00 → Monday 23rd.
        assert_eq!(next_slot(&s, at(20, 10, 0), &Utc), Some(at(23, 9, 0)));
        assert_eq!(latest_slot(&s, at(20, 10, 0), at(22, 23, 0), &Utc), None, "nothing on the weekend");
        assert_eq!(latest_slot(&s, at(20, 10, 0), at(23, 9, 1), &Utc), Some(at(23, 9, 0)));
        // Fridays (4) at 16:00.
        let w = every(Days::Weekly, 4, 16, 0);
        assert_eq!(next_slot(&w, at(16, 12, 0), &Utc), Some(at(20, 16, 0)));
        assert_eq!(next_slot(&w, at(20, 16, 0), &Utc), Some(at(27, 16, 0)));
        assert_eq!(latest_slot(&w, at(16, 12, 0), at(20, 16, 2), &Utc), Some(at(20, 16, 0)));
        // Sunday is 6.
        assert_eq!(next_slot(&every(Days::Weekly, 6, 18, 30), at(16, 12, 0), &Utc), Some(at(22, 18, 30)));
    }

    #[test]
    fn a_missed_run_runs_once_within_a_few_hours_else_is_skipped() {
        let s = every(Days::Daily, 0, 9, 0);
        let handled = at(16, 9, 0);
        // The PC was off at 09:00 on the 17th: Yap starts at 11:30 — late, once.
        assert_eq!(due(&s, handled, at(17, 11, 30), &Utc), Due::Run { slot: at(17, 9, 0), late: true });
        // At exactly the grace: still runs; a minute later: skipped.
        assert_eq!(due(&s, handled, at(17, 12, 0), &Utc), Due::Run { slot: at(17, 9, 0), late: true });
        assert_eq!(due(&s, handled, at(17, 12, 1), &Utc), Due::Skip { slot: at(17, 9, 0) });
        // Off for three days: only the latest counts (the 19th's), never three runs.
        assert_eq!(due(&s, handled, at(19, 10, 0), &Utc), Due::Run { slot: at(19, 9, 0), late: true });
        assert_eq!(due(&s, handled, at(19, 8, 0), &Utc), Due::Skip { slot: at(18, 9, 0) });
        // Once the skip is handled, nothing more until the next slot.
        assert_eq!(due(&s, at(18, 9, 0), at(19, 8, 0), &Utc), Due::Not);
    }

    #[test]
    fn its_time_is_the_pcs_own_across_a_clock_change() {
        // A zone an hour ahead of UTC: 09:00 there is 08:00 UTC.
        let tz = FixedOffset::east_opt(3600).unwrap();
        let s = every(Days::Daily, 0, 9, 0);
        assert_eq!(next_slot(&s, at(16, 0, 0), &tz), Some(at(16, 8, 0)));
        // A real zone's spring-forward gap (London, 29 Mar 2026, 01:00 → 02:00):
        // 01:30 that night runs at 02:30 (the first moment after the gap, an
        // hour on), once.
        let london = chrono_tz::Europe::London;
        let s = every(Days::Daily, 0, 1, 30);
        let before = Utc.with_ymd_and_hms(2026, 3, 28, 12, 0, 0).unwrap().timestamp();
        let slot = next_slot(&s, before, &london).unwrap();
        assert_eq!(slot, Utc.with_ymd_and_hms(2026, 3, 29, 1, 30, 0).unwrap().timestamp());
        // …and on the night they go back (25 Oct 2026, 02:00 → 01:00) 01:30
        // comes twice: the first of them, once.
        let before = Utc.with_ymd_and_hms(2026, 10, 24, 12, 0, 0).unwrap().timestamp();
        let slot = next_slot(&s, before, &london).unwrap();
        assert_eq!(slot, Utc.with_ymd_and_hms(2026, 10, 25, 0, 30, 0).unwrap().timestamp());
        assert_eq!(next_slot(&s, slot, &london), Some(Utc.with_ymd_and_hms(2026, 10, 26, 1, 30, 0).unwrap().timestamp()));
    }

    #[test]
    fn before_a_meeting_once_within_the_lead_and_never_after_it_began() {
        let now = at(16, 9, 50);
        let ev = |key: &'static str, start: i64, end: i64, meeting: bool| Upcoming { key, title: key, start, end, meeting };
        let events = [
            ev("standup", at(16, 10, 0), at(16, 10, 15), true),
            ev("review", at(16, 10, 30), at(16, 11, 0), true),
            ev("started", at(16, 9, 45), at(16, 10, 15), true),
            ev("focus", at(16, 9, 55), at(16, 11, 0), false),
            ev("holiday", at(16, 0, 0) + 60 * 15, at(17, 0, 0), true),
        ];
        let due_now: Vec<&str> = events_due(&events, 10, now, &[]).iter().map(|e| e.key).collect();
        assert_eq!(due_now, ["standup"], "ten minutes ahead, a meeting, not started");
        assert!(events_due(&events, 10, now, &["standup".to_string()]).is_empty(), "once");
        let longer: Vec<&str> = events_due(&events, 45, now, &[]).iter().map(|e| e.key).collect();
        assert_eq!(longer, ["standup", "review"], "soonest first");
        // A lead over the most is the most.
        assert_eq!(events_due(&events, 500, now, &[]).len(), 2);
        // Yap opened after the meeting began: too late.
        assert!(events_due(&events, 10, at(16, 10, 1), &[]).iter().all(|e| e.key != "standup"));
    }

    #[test]
    fn after_a_meeting_any_or_its_folder_or_with_outsiders() {
        let outside = vec!["example.org".to_string()];
        assert!(meeting_matches("", false, "Meetings", &[]));
        assert!(meeting_matches("Clients", false, "clients", &[]));
        assert!(!meeting_matches("Clients", false, "Meetings", &outside));
        assert!(meeting_matches("", true, "Meetings", &outside));
        assert!(!meeting_matches("", true, "Meetings", &[]));
        assert!(meeting_matches("Clients", true, "Clients", &outside));
        assert!(!meeting_matches("Clients", true, "Clients", &[]));
    }

    #[test]
    fn the_cap_counts_todays_runs_that_started() {
        let now = at(16, 15, 0);
        let started = [at(16, 9, 0), at(16, 12, 0), at(15, 23, 0), 0, at(16, 14, 59)];
        assert_eq!(runs_today(started, now, &Utc), 3, "yesterday's and a skipped one aren't today's");
        assert!(!over_cap(DAILY_CAP - 1));
        assert!(over_cap(DAILY_CAP));
        let many = vec![at(16, 9, 0); DAILY_CAP];
        assert!(over_cap(runs_today(many, now, &Utc)));
        // Midnight starts a new day's count.
        assert_eq!(runs_today(vec![at(16, 9, 0); DAILY_CAP], at(17, 0, 1), &Utc), 0);
    }
}
