//! The checks of an answer's **speeds and distances** (`speeds`: "km/h" |
//! "mph", `distances`: "km" | "miles"; 2026-10-25), as `temperatures`
//! checks its temperatures (`scenario::temperature_fault`): the answer gives
//! at least one, every one in another unit is only a conversion right after
//! one in the unit wanted ("29 km/h (18 mph)", "9.9 miles / 16 km"), such a
//! pair agrees, and each figure in the unit wanted is one the pages give, or
//! one of theirs converted (within [`Kind::close`]).

use std::sync::LazyLock;

/// A figure: "12,500", "1.5" (a decimal comma isn't one).
const NUM: &str = r"\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?";
const MPH: &str = r"mph|mi/h|miles?\s+(?:per|an)\s+hour";
const KMH: &str = r"km/h|km/hr|kmh|kph|kmph|km\s+(?:per|an)\s+hour|kilomet(?:re|er)s?\s+(?:per|an)\s+hour";
const MS: &str = r"m/s|met(?:re|er)s?\s+(?:per|a)\s+second";
const MILES: &str = r"miles?|mi";
const KM: &str = r"km|kms|kilomet(?:re|er)s?";

/// What's measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Speed,
    Distance,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Speed => "speeds",
            Kind::Distance => "distances",
        }
    }

    /// The unit a scenario asks for, as written: "km/h" | "mph", "km" | "miles".
    fn unit(self, want: &str) -> Option<&'static str> {
        let w = want.trim().to_ascii_lowercase();
        match (self, w.as_str()) {
            (Kind::Speed, "km/h" | "kph" | "kmh") => Some("km/h"),
            (Kind::Speed, "mph") => Some("mph"),
            (Kind::Distance, "km" | "kilometres" | "kilometers") => Some("km"),
            (Kind::Distance, "miles" | "mi" | "mile") => Some("miles"),
            _ => None,
        }
    }

    /// Two figures (in km/h or km) close enough to be the same one, rounded
    /// or converted: 2 km/h or 10 %, 1 km or 6 %.
    fn close(self, a: f64, b: f64) -> bool {
        let (abs, rel): (f64, f64) = match self {
            Kind::Speed => (2.0, 0.10),
            Kind::Distance => (1.0, 0.06),
        };
        (a - b).abs() <= abs.max(rel * a.abs().max(b.abs()))
    }

    fn found(self, text: &str, ranges: bool) -> Vec<Measure> {
        match self {
            Kind::Speed => speeds_in(text, ranges),
            Kind::Distance => distances_in(text, ranges),
        }
    }
}

/// A speed or distance in a text: where, the figure, its unit ("mph",
/// "km/h", "m/s"; "miles", "km") and the figure in km/h or km.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub start: usize,
    pub end: usize,
    pub value: f64,
    pub unit: &'static str,
    pub base: f64,
}

fn number(s: &str) -> Option<f64> {
    s.replace(',', "").parse().ok()
}

/// A figure inside a bigger one ("1,5 km", "A9 miles"): not one. Pure.
fn inside_a_number(before: &str) -> bool {
    let mut back = before.chars().rev();
    match back.next() {
        Some(c) if c.is_alphanumeric() => true,
        Some('.' | ',') => back.next().is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    }
}

/// The speeds in `text`, in order: "18 mph", "29 km/h", "5 m/s", "60 miles
/// per hour"; with `ranges`, a range's low end too ("2–4 mph": 2 and 4).
/// Knots aren't read. Pure.
pub fn speeds_in(text: &str, ranges: bool) -> Vec<Measure> {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(&format!(
            r"(?i)(?:(?P<lo>{NUM})\s?(?:–|-|‑|to)\s?)?(?P<hi>{NUM})\s?(?P<unit>{MPH}|{KMH}|{MS})\b"
        ))
        .expect("static regex")
    });
    measures(text, &RE, ranges, None, |u| {
        let u = u.to_ascii_lowercase();
        if u.starts_with("m/") || u.starts_with("met") {
            ("m/s", 3.6)
        } else if u.starts_with('k') {
            ("km/h", 1.0)
        } else {
            ("mph", 1.609_344)
        }
    })
}

/// The distances in `text`, in order: "16 km", "9.9 miles", "a 10-mile
/// walk" — not a speed ("km/h", "miles per hour"), an area ("km²") or fuel
/// use ("miles per gallon"). With `ranges`, a range's low end too. Pure.
pub fn distances_in(text: &str, ranges: bool) -> Vec<Measure> {
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(&format!(r"(?i)(?:(?P<lo>{NUM})\s?(?:–|-|‑|to)\s?)?(?P<hi>{NUM})(?:\s?|-)(?P<unit>{MILES}|{KM})\b"))
            .expect("static regex")
    });
    static NOT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)^(?:\s?/|[²³]|\s?(?:per|an|a)\s+(?:hour|hr|second|sec|minute|min|gallon|litre|liter)\b)")
            .expect("static regex")
    });
    measures(text, &RE, ranges, Some(&NOT), |u| {
        if u.to_ascii_lowercase().starts_with('k') {
            ("km", 1.0)
        } else {
            ("miles", 1.609_344)
        }
    })
}

fn measures(
    text: &str,
    re: &regex::Regex,
    ranges: bool,
    not_before: Option<&regex::Regex>,
    unit: impl Fn(&str) -> (&'static str, f64),
) -> Vec<Measure> {
    let mut out = Vec::new();
    for c in re.captures_iter(text) {
        let (Some(all), Some(hi)) = (c.get(0), c.name("hi")) else { continue };
        if inside_a_number(&text[..all.start()]) || not_before.is_some_and(|n| n.is_match(&text[all.end()..])) {
            continue;
        }
        let (name, factor) = unit(&c["unit"]);
        if ranges {
            if let Some(lo) = c.name("lo").and_then(|lo| number(lo.as_str()).map(|v| (lo, v))) {
                let (lo, v) = lo;
                out.push(Measure { start: lo.start(), end: lo.end(), value: v, unit: name, base: v * factor });
            }
        }
        let Some(v) = number(hi.as_str()) else { continue };
        out.push(Measure { start: hi.start(), end: all.end(), value: v, unit: name, base: v * factor });
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &one[..i]),
        None => one,
    }
}

/// Why `reply`'s speeds or distances don't lead with `want` ("km/h" |
/// "mph", "km" | "miles"), if they don't: it gives none; one in another unit
/// isn't a conversion right after one in `want` ("29 km/h (18 mph)" and "29
/// km/h / 18 mph" pass, "18 mph (29 km/h)" doesn't); such a pair disagrees;
/// or — with `pages`, the web text the model read — a figure in `want` is
/// neither one the pages give nor one of theirs converted. `Err`: `want`
/// isn't a unit of that kind. Pure.
pub fn fault(reply: &str, want: &str, pages: &[String], kind: Kind) -> Result<Option<String>, String> {
    let Some(want) = kind.unit(want) else {
        return Err(format!("{}: \"{want}\" isn't a unit Yap checks", kind.name()));
    };
    let found = kind.found(reply, false);
    let given: Vec<Measure> = pages.iter().flat_map(|p| kind.found(p, true)).collect();
    let quote = |t: &Measure| {
        let before: String = reply[..t.start].chars().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect();
        clip(&format!("{before}{}", &reply[t.start..t.end]), 60)
    };
    let what = match kind {
        Kind::Speed => "speed",
        Kind::Distance => "distance",
    };
    if found.is_empty() {
        return Ok(Some(format!("gives no {what} with its unit (want {want})")));
    }
    for (i, t) in found.iter().enumerate().filter(|(_, t)| t.unit != want) {
        let pair = i
            .checked_sub(1)
            .map(|j| &found[j])
            .filter(|p| p.unit == want && super::scenario::joins_a_conversion(&reply[p.end..t.start]));
        match pair {
            None => return Ok(Some(format!("leads with {}, not {want}: \"{}\"", t.unit, quote(t)))),
            Some(p) if !kind.close(p.base, t.base) => {
                return Ok(Some(format!("{} and {} disagree", &reply[p.start..p.end], &reply[t.start..t.end])));
            }
            Some(_) => {}
        }
    }
    if !given.is_empty() {
        if let Some(t) = found.iter().filter(|t| t.unit == want).find(|t| !given.iter().any(|g| kind.close(g.base, t.base))) {
            return Ok(Some(format!("{} is neither in the pages nor one of theirs converted", &reply[t.start..t.end])));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_and_distances_are_read_with_their_units() {
        let speeds = |t: &str| speeds_in(t, false).iter().map(|m| (m.value, m.unit)).collect::<Vec<_>>();
        assert_eq!(
            speeds("Wind W 18 mph, gusts to 35 mph; 29 km/h, 50 kph, 5 m/s, 60 miles per hour, 2–4 mph"),
            [(18.0, "mph"), (35.0, "mph"), (29.0, "km/h"), (50.0, "km/h"), (5.0, "m/s"), (60.0, "mph"), (4.0, "mph")]
        );
        assert!(speeds("15 knots, 10 km away, 6 mi").is_empty());
        let ranged: Vec<f64> = speeds_in("Winds 2–4 mph, 12 to 16 km/h", true).iter().map(|m| m.value).collect();
        assert_eq!(ranged, [2.0, 4.0, 12.0, 16.0]);
        let distances = |t: &str| distances_in(t, false).iter().map(|m| (m.value, m.unit)).collect::<Vec<_>>();
        assert_eq!(
            distances("38 km to the airport, a 10-mile walk, 1,200 kilometres, 9.9 miles, visibility 6 mi"),
            [(38.0, "km"), (10.0, "miles"), (1200.0, "km"), (9.9, "miles"), (6.0, "miles")]
        );
        // Not a distance: a speed, an area, fuel use, part of a bigger
        // figure, a name, minutes.
        assert!(distances("100 km/h, 60 miles per hour, 5 km², 30 miles per gallon, 1,5 km, A9 miles, Miles Davis, 5 min").is_empty());
    }

    #[test]
    fn an_answer_leads_with_the_unit_asked_for() {
        let pages = [r#"{"results":[{"text":"Wind W 18 mph, gusts to 35 mph. Winds 2–4 mph. The path is 12 miles one way."}]}"#.to_string()];
        let speed = |r: &str, w: &str| fault(r, w, &pages, Kind::Speed).unwrap();
        // Ireland's answers before (Qwen3.5-4B, 2026-10-23): the pages' mph.
        assert_eq!(speed("Showers, wind 18 mph with gusts to 35 mph.", "km/h").as_deref(), Some("leads with mph, not km/h: \"Showers, wind 18 mph\""));
        // km/h first, the mph in brackets or after a slash.
        assert_eq!(speed("Wind 29 km/h (18 mph), gusts to 56 km/h (35 mph).", "km/h"), None);
        assert_eq!(speed("Wind 29 km/h / 18 mph; light winds of 3 to 6 km/h (2–4 mph).", "km/h"), None);
        // A pair that disagrees; a figure from nowhere; none at all.
        assert_eq!(speed("Wind 18 km/h (18 mph).", "km/h").as_deref(), Some("18 km/h and 18 mph disagree"));
        assert_eq!(speed("Wind 80 km/h.", "km/h").as_deref(), Some("80 km/h is neither in the pages nor one of theirs converted"));
        assert!(speed("Showers all day.", "km/h").unwrap().starts_with("gives no speed"));
        // The UK keeps mph.
        assert_eq!(speed("Wind 18 mph, gusts 35 mph.", "mph"), None);
        assert!(speed("Wind 29 km/h.", "mph").unwrap().starts_with("leads with km/h, not mph"));
        let distance = |r: &str, w: &str| fault(r, w, &pages, Kind::Distance).unwrap();
        assert_eq!(distance("It's 19 km (12 miles) one way.", "km"), None);
        assert!(distance("It's 12 miles one way.", "km").unwrap().starts_with("leads with miles, not km"));
        assert_eq!(distance("It's 12 miles one way.", "miles"), None);
        assert!(fault("12 miles", "furlongs", &pages, Kind::Distance).is_err());
    }
}
