//! Time zones for iCalendar feeds: every TZID a feed uses becomes a [`Zone`]
//! that turns its wall-clock times into UTC instants and back.
//!
//! A TZID resolves, in order, as:
//! 1. an IANA name ("Europe/London"; Google's feeds), also inside a path
//!    ("/mozilla.org/20050126_1/Europe/London"), via `chrono-tz`;
//! 2. a Windows zone name ("GMT Standard Time"; Outlook's feeds), mapped to
//!    IANA with CLDR's `windowsZones.xml` table ([`WINDOWS_ZONES`]);
//! 3. the feed's own VTIMEZONE definition for that TZID (Outlook's
//!    "Customized Time Zone", display names like "(UTC+01:00) Amsterdam…"):
//!    its STANDARD/DAYLIGHT observances, expanded into transitions;
//! 4. a fixed "(UTC±hh:mm)" offset at the start of the name;
//! 5. otherwise the PC's own zone, as for floating times.
//!
//! Wall-clock times that don't exist (the hour skipped when clocks go
//! forward) are read with the offset in effect before the gap, and times
//! that happen twice (clocks going back) as the first one, per RFC 5545
//! §3.3.5.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, Offset, TimeZone};

use super::ics::{expand_floating, parse_date_time, Component};

/// How to read a feed's wall-clock times.
#[derive(Debug, Clone)]
pub enum Zone {
    Utc,
    Iana(chrono_tz::Tz),
    /// A VTIMEZONE the feed defines itself.
    Rules(Arc<Rules>),
    /// A fixed offset, seconds east of UTC.
    Fixed(i32),
    /// The PC's own zone: floating times, "this time wherever you are".
    Local,
}

impl Zone {
    /// The instant (unix seconds) of wall-clock time `local` in this zone.
    pub fn to_utc(&self, local: NaiveDateTime) -> i64 {
        match self {
            Zone::Utc => local.and_utc().timestamp(),
            Zone::Fixed(offset) => local.and_utc().timestamp() - i64::from(*offset),
            Zone::Iana(tz) => from_local(tz, local),
            Zone::Local => from_local(&chrono::Local, local),
            Zone::Rules(rules) => rules.to_utc(local),
        }
    }

    /// The wall-clock time in this zone at instant `utc` (unix seconds).
    pub fn to_local(&self, utc: i64) -> NaiveDateTime {
        let at = DateTime::from_timestamp(utc, 0).unwrap_or_default();
        match self {
            Zone::Utc => at.naive_utc(),
            Zone::Fixed(offset) => at.naive_utc() + Duration::seconds(i64::from(*offset)),
            Zone::Iana(tz) => at.with_timezone(tz).naive_local(),
            Zone::Local => at.with_timezone(&chrono::Local).naive_local(),
            Zone::Rules(rules) => at.naive_utc() + Duration::seconds(i64::from(rules.offset_at(utc))),
        }
    }
}

/// `local` read in `tz`: the earlier instant when it happens twice, the
/// pre-gap offset when it doesn't exist at all.
fn from_local<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime) -> i64 {
    match tz.from_local_datetime(&local) {
        LocalResult::Single(at) => at.timestamp(),
        LocalResult::Ambiguous(first, _) => first.timestamp(),
        LocalResult::None => {
            let before = tz.offset_from_utc_datetime(&(local - Duration::days(1))).fix().local_minus_utc();
            local.and_utc().timestamp() - i64::from(before)
        }
    }
}

/// A feed's own VTIMEZONE: the offset changes it defines, as UTC instants.
#[derive(Debug)]
pub struct Rules {
    /// (instant, offset from then on), sorted by instant.
    transitions: Vec<(i64, i32)>,
    /// The offset before the first transition.
    initial: i32,
}

/// Recurring transitions are worked out for these years.
const RULES_FROM_YEAR: i32 = 1970;
const RULES_UNTIL_YEAR: i32 = 2100;

impl Rules {
    /// Read a VTIMEZONE's STANDARD and DAYLIGHT observances. `None` when it
    /// defines no usable offsets.
    pub fn from_component(vtimezone: &Component) -> Option<Rules> {
        let mut transitions: Vec<(i64, i32)> = Vec::new();
        let mut first: Option<(i64, i32)> = None; // (instant, offset before it)
        let year = |y: i32| chrono::NaiveDate::from_ymd_opt(y, 1, 1).and_then(|d| d.and_hms_opt(0, 0, 0));
        let (since, until) = (year(RULES_FROM_YEAR)?, year(RULES_UNTIL_YEAR)?);
        for obs in vtimezone.children.iter().filter(|c| c.name == "STANDARD" || c.name == "DAYLIGHT") {
            let Some(from) = obs.value("TZOFFSETFROM").and_then(parse_offset) else { continue };
            let Some(to) = obs.value("TZOFFSETTO").and_then(parse_offset) else { continue };
            let Some((start, _)) = obs.value("DTSTART").and_then(parse_date_time) else { continue };
            // Onsets are wall-clock times in the offset that applied before.
            let mut onsets = match obs.value("RRULE") {
                Some(rule) => expand_floating(start, rule, None, since, until, 1_000),
                None => vec![start],
            };
            for rdate in obs.props_named("RDATE") {
                onsets.extend(rdate.value.split(',').filter_map(|v| parse_date_time(v.trim()).map(|(t, _)| t)));
            }
            for onset in onsets {
                let instant = onset.and_utc().timestamp() - i64::from(from);
                if first.is_none_or(|(at, _)| instant < at) {
                    first = Some((instant, from));
                }
                transitions.push((instant, to));
            }
        }
        let (_, initial) = first?;
        transitions.sort_by_key(|(at, _)| *at);
        transitions.dedup_by_key(|(at, _)| *at);
        Some(Rules { transitions, initial })
    }

    /// The offset in effect at instant `utc`.
    pub fn offset_at(&self, utc: i64) -> i32 {
        match self.transitions.partition_point(|(at, _)| *at <= utc) {
            0 => self.initial,
            n => self.transitions[n - 1].1,
        }
    }

    fn to_utc(&self, local: NaiveDateTime) -> i64 {
        let wall = local.and_utc().timestamp();
        let mut offsets: Vec<i32> = self.transitions.iter().map(|(_, o)| *o).collect();
        offsets.push(self.initial);
        offsets.sort_unstable();
        offsets.dedup();
        // The instants whose offset agrees with themselves: one normally,
        // two when the clocks went back (take the first), none in a gap.
        let fits = offsets
            .iter()
            .map(|o| wall - i64::from(*o))
            .filter(|utc| i64::from(self.offset_at(*utc)) == wall - utc)
            .min();
        fits.unwrap_or_else(|| wall - i64::from(self.offset_at(wall - 86_400)))
    }
}

/// "+0100", "-0500", "+053000" → seconds east of UTC.
pub fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |i: usize| digits[i..i + 2].parse::<i32>().ok();
    let secs = n(0)? * 3600 + n(2)? * 60 + if digits.len() == 6 { n(4)? } else { 0 };
    (secs < 24 * 3600).then_some(sign * secs)
}

/// Resolves a feed's TZIDs (see the module docs), each once.
pub struct Resolver {
    vtimezones: HashMap<String, Component>,
    floating: Zone,
    cache: std::cell::RefCell<HashMap<String, Zone>>,
}

impl Resolver {
    /// `calendar`'s VTIMEZONEs; `floating` is how to read times without a
    /// zone (the PC's own zone in the app; a fixed one in tests).
    pub fn new(calendar: &Component, floating: Zone) -> Resolver {
        let vtimezones = calendar
            .children
            .iter()
            .filter(|c| c.name == "VTIMEZONE")
            .filter_map(|c| c.value("TZID").map(|id| (id.trim().to_string(), c.clone())))
            .collect();
        Resolver { vtimezones, floating, cache: Default::default() }
    }

    /// The zone for a TZID parameter (`None`: a floating time).
    pub fn zone(&self, tzid: Option<&str>) -> Zone {
        let Some(tzid) = tzid.map(|t| t.trim().trim_matches('"')).filter(|t| !t.is_empty()) else {
            return self.floating.clone();
        };
        if let Some(zone) = self.cache.borrow().get(tzid) {
            return zone.clone();
        }
        let zone = self.resolve(tzid);
        self.cache.borrow_mut().insert(tzid.to_string(), zone.clone());
        zone
    }

    fn resolve(&self, tzid: &str) -> Zone {
        if let Some(zone) = named_zone(tzid) {
            return zone;
        }
        if let Some(rules) = self.vtimezones.get(tzid).and_then(Rules::from_component) {
            return Zone::Rules(Arc::new(rules));
        }
        if let Some(offset) = display_name_offset(tzid) {
            return Zone::Fixed(offset);
        }
        tracing::debug!(tzid, "calendar: unknown time zone, reading it as the PC's own");
        self.floating.clone()
    }
}

/// A zone known by name alone: UTC, IANA (also at the end of a path) or a
/// Windows zone name.
pub fn named_zone(tzid: &str) -> Option<Zone> {
    let t = tzid.trim();
    if ["UTC", "GMT", "Z", "Etc/UTC", "tzone://Microsoft/Utc", "Coordinated Universal Time"]
        .iter()
        .any(|u| t.eq_ignore_ascii_case(u))
    {
        return Some(Zone::Utc);
    }
    if let Ok(tz) = t.parse::<chrono_tz::Tz>() {
        return Some(Zone::Iana(tz));
    }
    // "/mozilla.org/20050126_1/Europe/London", "/citadel.org/…/America/New_York".
    if t.contains('/') {
        let parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).collect();
        for take in [3, 2] {
            if parts.len() >= take {
                if let Ok(tz) = parts[parts.len() - take..].join("/").parse::<chrono_tz::Tz>() {
                    return Some(Zone::Iana(tz));
                }
            }
        }
    }
    windows_to_iana(t).and_then(|iana| iana.parse::<chrono_tz::Tz>().ok()).map(Zone::Iana)
}

/// The IANA zone CLDR maps a Windows zone name to.
pub fn windows_to_iana(name: &str) -> Option<&'static str> {
    WINDOWS_ZONES
        .iter()
        .find(|(windows, _)| windows.eq_ignore_ascii_case(name))
        .map(|(_, iana)| *iana)
}

/// "(UTC+01:00) Amsterdam, Berlin, …" → +3600; "(UTC) Dublin…" → 0.
fn display_name_offset(name: &str) -> Option<i32> {
    let name = name.trim();
    let rest = name.strip_prefix("(UTC").or_else(|| name.strip_prefix("(GMT"))?;
    let inner = &rest[..rest.find(')')?];
    if inner.is_empty() {
        return Some(0);
    }
    parse_offset(&inner.replace(':', ""))
}

/// CLDR `windowsZones.xml` (territory "001"): Windows zone → IANA zone.
/// Some IANA names here are older aliases ("Asia/Calcutta"), which
/// `chrono-tz` still knows (a test checks every one resolves).
pub const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("Dateline Standard Time", "Etc/GMT+12"),
    ("UTC-11", "Etc/GMT+11"),
    ("Aleutian Standard Time", "America/Adak"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Marquesas Standard Time", "Pacific/Marquesas"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("UTC-09", "Etc/GMT+9"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("UTC-08", "Etc/GMT+8"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
    ("Mountain Standard Time", "America/Denver"),
    ("Yukon Standard Time", "America/Whitehorse"),
    ("Central America Standard Time", "America/Guatemala"),
    ("Central Standard Time", "America/Chicago"),
    ("Easter Island Standard Time", "Pacific/Easter"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Canada Central Standard Time", "America/Regina"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Eastern Standard Time (Mexico)", "America/Cancun"),
    ("Eastern Standard Time", "America/New_York"),
    ("Haiti Standard Time", "America/Port-au-Prince"),
    ("Cuba Standard Time", "America/Havana"),
    ("US Eastern Standard Time", "America/Indianapolis"),
    ("Turks And Caicos Standard Time", "America/Grand_Turk"),
    ("Paraguay Standard Time", "America/Asuncion"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("Central Brazilian Standard Time", "America/Cuiaba"),
    ("SA Western Standard Time", "America/La_Paz"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("Tocantins Standard Time", "America/Araguaina"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("SA Eastern Standard Time", "America/Cayenne"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
    ("Greenland Standard Time", "America/Godthab"),
    ("Montevideo Standard Time", "America/Montevideo"),
    ("Magallanes Standard Time", "America/Punta_Arenas"),
    ("Saint Pierre Standard Time", "America/Miquelon"),
    ("Bahia Standard Time", "America/Bahia"),
    ("UTC-02", "Etc/GMT+2"),
    ("Azores Standard Time", "Atlantic/Azores"),
    ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
    ("UTC", "Etc/UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("Sao Tome Standard Time", "Africa/Sao_Tome"),
    ("Morocco Standard Time", "Africa/Casablanca"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("Jordan Standard Time", "Asia/Amman"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("Middle East Standard Time", "Asia/Beirut"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("Syria Standard Time", "Asia/Damascus"),
    ("West Bank Standard Time", "Asia/Hebron"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("FLE Standard Time", "Europe/Kiev"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("South Sudan Standard Time", "Africa/Juba"),
    ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
    ("Sudan Standard Time", "Africa/Khartoum"),
    ("Libya Standard Time", "Africa/Tripoli"),
    ("Namibia Standard Time", "Africa/Windhoek"),
    ("Arabic Standard Time", "Asia/Baghdad"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Arab Standard Time", "Asia/Riyadh"),
    ("Belarus Standard Time", "Europe/Minsk"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Iran Standard Time", "Asia/Tehran"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Astrakhan Standard Time", "Europe/Astrakhan"),
    ("Azerbaijan Standard Time", "Asia/Baku"),
    ("Russia Time Zone 3", "Europe/Samara"),
    ("Mauritius Standard Time", "Indian/Mauritius"),
    ("Saratov Standard Time", "Europe/Saratov"),
    ("Georgian Standard Time", "Asia/Tbilisi"),
    ("Volgograd Standard Time", "Europe/Volgograd"),
    ("Caucasus Standard Time", "Asia/Yerevan"),
    ("Afghanistan Standard Time", "Asia/Kabul"),
    ("West Asia Standard Time", "Asia/Tashkent"),
    ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("Qyzylorda Standard Time", "Asia/Qyzylorda"),
    ("India Standard Time", "Asia/Calcutta"),
    ("Sri Lanka Standard Time", "Asia/Colombo"),
    ("Nepal Standard Time", "Asia/Katmandu"),
    ("Central Asia Standard Time", "Asia/Bishkek"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("Omsk Standard Time", "Asia/Omsk"),
    ("Myanmar Standard Time", "Asia/Rangoon"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("Altai Standard Time", "Asia/Barnaul"),
    ("W. Mongolia Standard Time", "Asia/Hovd"),
    ("North Asia Standard Time", "Asia/Krasnoyarsk"),
    ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
    ("Tomsk Standard Time", "Asia/Tomsk"),
    ("China Standard Time", "Asia/Shanghai"),
    ("North Asia East Standard Time", "Asia/Irkutsk"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
    ("Aus Central W. Standard Time", "Australia/Eucla"),
    ("Transbaikal Standard Time", "Asia/Chita"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("North Korea Standard Time", "Asia/Pyongyang"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Yakutsk Standard Time", "Asia/Yakutsk"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("West Pacific Standard Time", "Pacific/Port_Moresby"),
    ("Tasmania Standard Time", "Australia/Hobart"),
    ("Vladivostok Standard Time", "Asia/Vladivostok"),
    ("Lord Howe Standard Time", "Australia/Lord_Howe"),
    ("Bougainville Standard Time", "Pacific/Bougainville"),
    ("Russia Time Zone 10", "Asia/Srednekolymsk"),
    ("Magadan Standard Time", "Asia/Magadan"),
    ("Norfolk Standard Time", "Pacific/Norfolk"),
    ("Sakhalin Standard Time", "Asia/Sakhalin"),
    ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
    ("Russia Time Zone 11", "Asia/Kamchatka"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("UTC+12", "Etc/GMT-12"),
    ("Fiji Standard Time", "Pacific/Fiji"),
    ("Chatham Islands Standard Time", "Pacific/Chatham"),
    ("UTC+13", "Etc/GMT-13"),
    ("Tonga Standard Time", "Pacific/Tongatapu"),
    ("Samoa Standard Time", "Pacific/Apia"),
    ("Line Islands Standard Time", "Pacific/Kiritimati"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        at(y, m, d, h, min).and_utc().timestamp()
    }

    #[test]
    fn every_windows_zone_maps_to_a_known_iana_zone() {
        for (windows, iana) in WINDOWS_ZONES {
            assert!(iana.parse::<chrono_tz::Tz>().is_ok(), "{windows} → {iana}");
            assert!(matches!(named_zone(windows), Some(Zone::Iana(_) | Zone::Utc)), "{windows}");
        }
    }

    #[test]
    fn names_resolve_through_iana_paths_and_windows() {
        let london = named_zone("Europe/London").unwrap();
        // BST in summer, GMT in winter.
        assert_eq!(london.to_utc(at(2026, 7, 1, 14, 30)), utc(2026, 7, 1, 13, 30));
        assert_eq!(london.to_utc(at(2026, 12, 1, 14, 30)), utc(2026, 12, 1, 14, 30));
        let mozilla = named_zone("/mozilla.org/20050126_1/Europe/London").unwrap();
        assert_eq!(mozilla.to_utc(at(2026, 7, 1, 14, 30)), utc(2026, 7, 1, 13, 30));
        let pacific = named_zone("Pacific Standard Time").unwrap();
        assert_eq!(pacific.to_utc(at(2026, 7, 1, 9, 0)), utc(2026, 7, 1, 16, 0));
        assert!(matches!(named_zone("tzone://Microsoft/Utc"), Some(Zone::Utc)));
        assert!(named_zone("Nowhere Standard Time").is_none());
    }

    #[test]
    fn gaps_and_overlaps_follow_rfc5545() {
        let london = named_zone("Europe/London").unwrap();
        // 29 Mar 2026: 01:00 GMT jumps to 02:00 BST, so 01:30 doesn't exist;
        // read with the offset before the gap (GMT) = 01:30 UTC.
        assert_eq!(london.to_utc(at(2026, 3, 29, 1, 30)), utc(2026, 3, 29, 1, 30));
        // 25 Oct 2026: 01:30 happens twice; the first (BST) wins.
        assert_eq!(london.to_utc(at(2026, 10, 25, 1, 30)), utc(2026, 10, 25, 0, 30));
        assert_eq!(london.to_local(utc(2026, 7, 1, 13, 30)), at(2026, 7, 1, 14, 30));
    }

    fn component(ics: &str) -> Component {
        super::super::ics::parse_text(ics).unwrap()
    }

    const CUSTOM: &str = "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Customized Time Zone\r\n\
        BEGIN:STANDARD\r\nDTSTART:16010101T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\n\
        RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\n\
        BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
        RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n\
        END:VCALENDAR\r\n";

    #[test]
    fn a_feeds_own_vtimezone_is_used_for_unknown_names() {
        let cal = component(CUSTOM);
        let resolver = Resolver::new(&cal, Zone::Utc);
        let zone = resolver.zone(Some("Customized Time Zone"));
        assert!(matches!(zone, Zone::Rules(_)));
        // Central European rules: +2 in summer, +1 in winter.
        assert_eq!(zone.to_utc(at(2026, 7, 1, 10, 0)), utc(2026, 7, 1, 8, 0));
        assert_eq!(zone.to_utc(at(2026, 12, 1, 10, 0)), utc(2026, 12, 1, 9, 0));
        // The change-over days: 29 Mar 2026 02:30 is in the gap → pre-gap
        // offset (+1); 25 Oct 2026 02:30 happens twice → the first (+2).
        assert_eq!(zone.to_utc(at(2026, 3, 29, 2, 30)), utc(2026, 3, 29, 1, 30));
        assert_eq!(zone.to_utc(at(2026, 10, 25, 2, 30)), utc(2026, 10, 25, 0, 30));
        assert_eq!(zone.to_local(utc(2026, 7, 1, 8, 0)), at(2026, 7, 1, 10, 0));
    }

    #[test]
    fn unknown_names_fall_back_to_a_display_offset_then_floating() {
        let cal = component("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n");
        let resolver = Resolver::new(&cal, Zone::Fixed(3600));
        let display = resolver.zone(Some("(UTC+05:30) Chennai, Kolkata, Mumbai, New Delhi"));
        assert_eq!(display.to_utc(at(2026, 1, 1, 12, 0)), utc(2026, 1, 1, 6, 30));
        let unknown = resolver.zone(Some("Somewhere"));
        assert_eq!(unknown.to_utc(at(2026, 1, 1, 12, 0)), utc(2026, 1, 1, 11, 0));
        let floating = resolver.zone(None);
        assert_eq!(floating.to_utc(at(2026, 1, 1, 12, 0)), utc(2026, 1, 1, 11, 0));
    }

    #[test]
    fn offsets_parse() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0530"), Some(-19_800));
        assert_eq!(parse_offset("+053045"), Some(19_845));
        assert_eq!(parse_offset("0100"), None);
        assert_eq!(parse_offset("+25"), None);
    }
}
