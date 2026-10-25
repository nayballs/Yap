//! **Speeds and distances in the person's units** (2026-10-25), as
//! temperatures have been since 2026-10-23 ([`super::convert_temperatures`],
//! [`super::temperature_note`]): the web text a model reads puts the
//! person's unit first, and a line beside it says so. A speed and a distance
//! have one right answer in each unit, like a temperature (unlike a price,
//! which Yap never converts).
//!
//! - **Speeds**: the UK (and the Crown Dependencies, `MILES`) and the US
//!   in mph — "30 km/h" → "19 mph (30 km/h)", "5 m/s" → "11 mph (5 m/s)";
//!   the rest of the metric world in km/h — "18 mph" → "29 km/h (18 mph)",
//!   a page's m/s left as it is (Scandinavia's forecasts give m/s). Knots
//!   are never converted (sea and air).
//! - **Distances**: miles for the UK and the US — "38 km" → "24 miles (38
//!   km)"; km elsewhere — "12 miles" → "19 km (12 miles)". Only km and
//!   miles: metres, feet and yards stay (the UK's short distances are
//!   metric, its heights often feet).
//!
//! As for temperatures: a range ("2–4 mph" → "3 to 6 km/h (2–4 mph)"); a
//! page's own conversion put first ("14 mph (23 km/h)" → "23 km/h (14
//! mph)"); a figure already after one of theirs ("23 km/h (14 mph)") or a
//! question that names the other unit ("…in km/h?") left alone. Never a
//! figure that's part of something else: a speed isn't a distance ("km/h",
//! "miles per hour"), nor an area ("km²"), fuel use ("miles per gallon"), a
//! decimal comma ("1,5 km") or a road ("A9 miles"); a hyphenated "a 5-mile
//! walk" stays. Speeds in whole numbers; distances to one decimal under 10.

use std::sync::LazyLock;

use super::Units;

/// A figure: "12,500", "1.5" (a decimal comma isn't one).
const NUM: &str = r"\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?";
const MPH: &str = r"mph|mi/h|miles?\s+(?:per|an)\s+hour";
const KMH: &str = r"km/h|km/hr|kmh|kph|kmph|km\s+(?:per|an)\s+hour|kilomet(?:re|er)s?\s+(?:per|an)\s+hour";
const MS: &str = r"m/s|met(?:re|er)s?\s+(?:per|a)\s+second";
const MILES: &str = r"miles?|mi";
const KM: &str = r"km|kms|kilomet(?:re|er)s?";
/// A page's own conversion's lead-in: "(about 23 km/h)".
const ABOUT: &str = r"(?:about\s|approx\.?\s?|~|≈)?";

const KM_PER_MILE: f64 = 1.609_344;

/// A figure or a range ("2–4", "10 to 15") in `unit`, then maybe the page's
/// own conversion into `own` in brackets or after a slash.
fn figures(unit: &str, own: &str) -> regex::Regex {
    let figure = format!(r"(?:(?P<lo>{NUM})\s?(?:–|-|‑|to)\s?)?(?P<hi>{NUM})");
    let owned = format!(r"(?:(?:{NUM})\s?(?:–|-|‑|to)\s?)?(?:{NUM})");
    regex::Regex::new(&format!(
        r"(?i)(?P<from>{figure}\s?(?P<unit>{unit})\b)(?:\s?[(/]\s?{ABOUT}(?P<own>{owned}\s?(?:{own}))\b\s?\)?)?"
    ))
    .expect("static regex")
}

/// The text before a figure ends in one of the person's: the figure is that
/// one's conversion ("23 km/h (14 mph)").
fn after(theirs: &str) -> regex::Regex {
    regex::Regex::new(&format!(r"(?i)(?:{theirs})\s?[(/]\s?{ABOUT}$")).expect("static regex")
}

/// What follows a distance that makes it something else: a speed ("km/h",
/// "miles per hour"), an area, fuel use.
static NOT_A_DISTANCE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)^(?:\s?/|[²³]|\s?(?:per|an|a)\s+(?:hour|hr|second|sec|minute|min|gallon|litre|liter)\b)").expect("static regex")
});

/// A figure inside a bigger one ("1,5 km", "A9 miles"): not one. Pure.
fn inside_a_number(before: &str) -> bool {
    let mut back = before.chars().rev();
    match back.next() {
        Some(c) if c.is_alphanumeric() => true,
        Some('.' | ',') => back.next().is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn number(s: &str) -> Option<f64> {
    s.replace(',', "").parse().ok()
}

/// A whole number, with thousands separated ("1,243").
fn whole(v: f64) -> String {
    let n = v.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// A distance: one decimal under 10 ("3.1", "9.9", not "2.0"), else whole.
fn distance(v: f64) -> String {
    let tenths = (v * 10.0).round() / 10.0;
    if tenths < 10.0 && tenths.fract() != 0.0 {
        format!("{tenths:.1}")
    } else {
        whole(v)
    }
}

/// `text` with every figure `re` finds in the person's unit first. `factor`:
/// the matched unit → how many of theirs one is; `write(lo, hi)`: their
/// figure. `None` when nothing changes. Pure.
fn convert(
    text: &str,
    re: &regex::Regex,
    after_theirs: &regex::Regex,
    not_before: Option<&regex::Regex>,
    factor: impl Fn(&str) -> f64,
    write: impl Fn(Option<f64>, f64) -> String,
) -> Option<String> {
    let mut changed = false;
    let out = re.replace_all(text, |c: &regex::Captures| {
        let all = c.get(0).expect("match");
        let from = c.name("from").expect("from");
        let keep = all.as_str().to_string();
        let before = &text[..all.start()];
        if inside_a_number(before) || after_theirs.is_match(before) || not_before.is_some_and(|n| n.is_match(&text[from.end()..])) {
            return keep;
        }
        let theirs = match c.name("own") {
            Some(own) => Some(own.as_str().to_string()),
            None => {
                let k = factor(&c["unit"]);
                let hi = number(&c["hi"]).map(|v| v * k);
                let lo = c.name("lo").map(|lo| number(lo.as_str()).map(|v| v * k));
                match (lo, hi) {
                    (Some(None), _) | (_, None) => None,
                    (Some(Some(lo)), Some(hi)) => Some(write(Some(lo), hi)),
                    (None, Some(hi)) => Some(write(None, hi)),
                }
            }
        };
        match theirs {
            Some(t) => {
                changed = true;
                // A trailing space or bracket the match took stays after it.
                let rest = &text[from.end()..all.end()];
                let tail = if c.name("own").is_some() { "" } else { rest };
                format!("{t} ({}){tail}", from.as_str())
            }
            None => keep,
        }
    });
    changed.then(|| out.into_owned())
}

/// **The pages' speeds in the person's unit first**: mph for the UK and the
/// US ("30 km/h" → "19 mph (30 km/h)", "5 m/s" → "11 mph (5 m/s)"), km/h
/// elsewhere ("18 mph" → "29 km/h (18 mph)"). `None` when nothing changes
/// or the units aren't known. Pure.
pub fn convert_speeds(units: Option<Units>, text: &str) -> Option<String> {
    static TO_MPH: LazyLock<regex::Regex> = LazyLock::new(|| figures(&format!("{KMH}|{MS}"), MPH));
    static TO_KMH: LazyLock<regex::Regex> = LazyLock::new(|| figures(MPH, KMH));
    static AFTER_MPH: LazyLock<regex::Regex> = LazyLock::new(|| after(MPH));
    static AFTER_KMH: LazyLock<regex::Regex> = LazyLock::new(|| after(&format!("{KMH}|{MS}")));
    let write = |unit: &'static str| {
        move |lo: Option<f64>, hi: f64| match lo {
            Some(lo) => format!("{} to {} {unit}", whole(lo), whole(hi)),
            None => format!("{} {unit}", whole(hi)),
        }
    };
    match units? {
        Units::Metric => convert(text, &TO_KMH, &AFTER_KMH, None, |_| KM_PER_MILE, write("km/h")),
        Units::MetricMiles | Units::Us => convert(
            text,
            &TO_MPH,
            &AFTER_MPH,
            None,
            |u| {
                let u = u.to_ascii_lowercase();
                if u.starts_with("m/") || u.starts_with("met") {
                    3.6 / KM_PER_MILE
                } else {
                    1.0 / KM_PER_MILE
                }
            },
            write("mph"),
        ),
    }
}

/// **The pages' distances in the person's unit first**: miles for the UK
/// and the US ("38 km" → "24 miles (38 km)"), km elsewhere ("12 miles" →
/// "19 km (12 miles)"). Never a speed, an area or fuel use. `None` when
/// nothing changes or the units aren't known. Pure.
pub fn convert_distances(units: Option<Units>, text: &str) -> Option<String> {
    static TO_MILES: LazyLock<regex::Regex> = LazyLock::new(|| figures(KM, MILES));
    static TO_KM: LazyLock<regex::Regex> = LazyLock::new(|| figures(MILES, KM));
    static AFTER_MILES: LazyLock<regex::Regex> = LazyLock::new(|| after(&format!(r"\b(?:{MILES})")));
    static AFTER_KM: LazyLock<regex::Regex> = LazyLock::new(|| after(&format!(r"\b(?:{KM})")));
    let write = |many: &'static str, one: &'static str| {
        move |lo: Option<f64>, hi: f64| match lo {
            Some(lo) => format!("{} to {} {many}", distance(lo), distance(hi)),
            None if distance(hi) == "1" => format!("1 {one}"),
            None => format!("{} {many}", distance(hi)),
        }
    };
    match units? {
        Units::Metric => convert(text, &TO_KM, &AFTER_KM, Some(&NOT_A_DISTANCE), |_| KM_PER_MILE, write("km", "km")),
        Units::MetricMiles | Units::Us => {
            convert(text, &TO_MILES, &AFTER_MILES, Some(&NOT_A_DISTANCE), |_| 1.0 / KM_PER_MILE, write("miles", "mile"))
        }
    }
}

/// The line beside web text whose speeds Yap put in the person's unit
/// first: which unit they use, and to give every speed in it. Its example
/// is no figure the evaluations' pages give. `None` when nothing was
/// converted. Pure.
pub fn speed_note(units: Option<Units>, text: &str) -> Option<String> {
    convert_speeds(units, text)?;
    let (theirs, other, example) = match units? {
        Units::Metric => ("km/h", "mph", "64 km/h (40 mph)"),
        Units::MetricMiles | Units::Us => ("mph", "km/h", "40 mph (64 km/h)"),
    };
    Some(format!(
        "The user uses {theirs} for speeds: Yap has put {theirs} before these pages' {other} figures, like \"{example}\". Give every speed in {theirs} (the {other} in brackets after it at most), converting any still in {other}."
    ))
}

/// The same for distances. Pure.
pub fn distance_note(units: Option<Units>, text: &str) -> Option<String> {
    convert_distances(units, text)?;
    let (theirs, other, example) = match units? {
        Units::Metric => ("km", "miles", "16 km (10 miles)"),
        Units::MetricMiles | Units::Us => ("miles", "km", "6.2 miles (10 km)"),
    };
    Some(format!(
        "The user uses {theirs} for distances: Yap has put {theirs} before these pages' {other} figures, like \"{example}\". Give every distance in {theirs} (the {other} in brackets after it at most), converting any still in {other}."
    ))
}

/// The unit a web answer's speeds go in: the person's, unless the question
/// names the other ("…in km/h?" from the UK, "in mph" from Ireland). Pure.
pub fn speed_units(units: Option<Units>, question: &str) -> Option<Units> {
    static NAMES_MPH: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(&format!(r"(?i)\b(?:{MPH})\b")).expect("static regex"));
    static NAMES_KMH: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(&format!(r"(?i)\b(?:{KMH})\b|kilomet(?:re|er)s?\s+an?\s+hour")).expect("static regex"));
    let other = match units? {
        Units::Metric => NAMES_MPH.is_match(question),
        Units::MetricMiles | Units::Us => NAMES_KMH.is_match(question),
    };
    units.filter(|_| !other)
}

/// The unit a web answer's distances go in: the person's, unless the
/// question names the other ("how many km…" from the UK). Pure.
pub fn distance_units(units: Option<Units>, question: &str) -> Option<Units> {
    static NAMES_MILES: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?i)\bmiles?\b").expect("static regex"));
    static NAMES_KM: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(&format!(r"(?i)\b(?:{KM})\b")).expect("static regex"));
    let other = match units? {
        Units::Metric => NAMES_MILES.is_match(question),
        Units::MetricMiles | Units::Us => NAMES_KM.is_match(question),
    };
    units.filter(|_| !other)
}

/// **One web answer's figures in the person's units**: temperatures, speeds
/// and distances, each unless the question names the other unit of it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TheirUnits {
    pub temperatures: Option<Units>,
    pub speeds: Option<Units>,
    pub distances: Option<Units>,
}

impl TheirUnits {
    /// For `question`, from someone who measures in `units`.
    pub fn for_question(units: Option<Units>, question: &str) -> TheirUnits {
        TheirUnits {
            temperatures: super::temperature_units(units, question),
            speeds: speed_units(units, question),
            distances: distance_units(units, question),
        }
    }

    /// `text` with its temperatures, speeds and distances in the person's
    /// units first; `None` when nothing changes. Pure.
    pub fn convert(&self, text: &str) -> Option<String> {
        let t = super::convert_temperatures(self.temperatures, text);
        let s = convert_speeds(self.speeds, t.as_deref().unwrap_or(text));
        let d = convert_distances(self.distances, s.as_deref().or(t.as_deref()).unwrap_or(text));
        d.or(s).or(t)
    }

    /// The lines that say so, by the key a web tool's result gives each
    /// ("temperatures", "speeds", "distances"), for `text` as the pages give
    /// it. Pure.
    pub fn notes(&self, text: &str) -> Vec<(&'static str, String)> {
        [
            ("temperatures", super::temperature_note(self.temperatures, text)),
            ("speeds", speed_note(self.speeds, text)),
            ("distances", distance_note(self.distances, text)),
        ]
        .into_iter()
        .filter_map(|(key, note)| note.map(|n| (key, n)))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UK: Option<Units> = Some(Units::MetricMiles);
    const IE: Option<Units> = Some(Units::Metric);
    const US: Option<Units> = Some(Units::Us);

    #[test]
    fn speeds_come_in_their_unit_first() {
        // Ireland's pages (the evaluations' Kilcrannagh): km/h first.
        assert_eq!(
            convert_speeds(IE, "Wind W 18 mph, gusts to 35 mph, easing to 9–12 mph in the evening.").as_deref(),
            Some("Wind W 29 km/h (18 mph), gusts to 56 km/h (35 mph), easing to 14 to 19 km/h (9–12 mph) in the evening.")
        );
        assert_eq!(convert_speeds(IE, "60 miles per hour, 2-4 mph").as_deref(), Some("97 km/h (60 miles per hour), 3 to 6 km/h (2-4 mph)"));
        // The page's own conversion first; one already after theirs, m/s
        // (metric too) and knots stay.
        assert_eq!(convert_speeds(IE, "Wind 14 mph (23 km/h).").as_deref(), Some("Wind 23 km/h (14 mph)."));
        assert_eq!(convert_speeds(IE, "Wind 14 mph (about 23 km/h).").as_deref(), Some("Wind 23 km/h (14 mph)."));
        assert_eq!(convert_speeds(IE, "Wind 23 km/h (14 mph), 5 m/s (11 mph), 15 knots"), None);
        // The UK and the US: mph first, from km/h, kph or m/s.
        for u in [UK, US] {
            assert_eq!(
                convert_speeds(u, "Wind E 30 km/h, gusts 55 kph, then 10–15 km/h; 5 m/s offshore.").as_deref(),
                Some("Wind E 19 mph (30 km/h), gusts 34 mph (55 kph), then 6 to 9 mph (10–15 km/h); 11 mph (5 m/s) offshore.")
            );
            assert_eq!(convert_speeds(u, "Wind NW 2–4 mph, 9 mph (14 km/h), 120 kilometres per hour"), Some("Wind NW 2–4 mph, 9 mph (14 km/h), 75 mph (120 kilometres per hour)".into()));
        }
        assert_eq!(convert_speeds(UK, "Wind SW 14 mph"), None);
        assert_eq!(convert_speeds(None, "Wind 30 km/h"), None);
    }

    #[test]
    fn distances_come_in_their_unit_first() {
        // The UK and the US: miles.
        for u in [UK, US] {
            assert_eq!(
                convert_distances(u, "Valdorrena is 38 km from the airport; the beach is a 1.5 km walk, the trail 6 km return, Madrid 1,200 km away.").as_deref(),
                Some("Valdorrena is 24 miles (38 km) from the airport; the beach is a 0.9 miles (1.5 km) walk, the trail 3.7 miles (6 km) return, Madrid 746 miles (1,200 km) away.")
            );
            assert_eq!(convert_distances(u, "10 km (6.2 miles), 2 km, 1.6 km").as_deref(), Some("6.2 miles (10 km), 1.2 miles (2 km), 1 mile (1.6 km)"));
        }
        // Elsewhere: km; ranges; a page's own conversion first.
        assert_eq!(
            convert_distances(IE, "The path runs 12 miles; the first 3 miles are level; 5 to 6 miles, visibility 6 mi.").as_deref(),
            Some("The path runs 19 km (12 miles); the first 4.8 km (3 miles) are level; 8 to 9.7 km (5 to 6 miles), visibility 9.7 km (6 mi).")
        );
        assert_eq!(convert_distances(IE, "12 miles (19 km)").as_deref(), Some("19 km (12 miles)"));
        // Not a distance, or not one Yap converts: speeds, an area, fuel
        // use, a decimal comma, a road, a hyphenated word, a name, metres,
        // minutes, nautical miles — or already after theirs.
        assert_eq!(
            convert_distances(UK, "100 km/h, 30 km per hour, 5 km², 20 km per litre, 1,5 km, the A9 km, a 5-km run, 300 m, 5 min"),
            None
        );
        assert_eq!(convert_distances(IE, "60 miles per hour, 40 miles per gallon, 200 nautical miles, Miles Davis, 19 km (12 miles)"), None);
        assert_eq!(convert_distances(UK, "24 miles (38 km), 24 miles / 38 km"), None);
    }

    #[test]
    fn a_question_naming_the_other_unit_leaves_it() {
        assert_eq!(speed_units(UK, "How windy is it in Valdorrena tomorrow?"), UK);
        assert_eq!(speed_units(UK, "What's the wind in Valdorrena tomorrow, in km/h?"), None);
        assert_eq!(speed_units(UK, "wind in kilometres an hour please"), None);
        assert_eq!(speed_units(IE, "How fast is the wind in mph?"), None);
        assert_eq!(speed_units(US, "How windy will it be?"), US);
        assert_eq!(distance_units(UK, "How far is it from the airport?"), UK);
        assert_eq!(distance_units(UK, "How many km is it from the airport?"), None);
        assert_eq!(distance_units(IE, "How many miles is the coastal path?"), None);
        assert_eq!(speed_units(None, "How windy?"), None);
    }

    #[test]
    fn one_answer_gets_every_kind_with_its_line() {
        let page = "Valdorrena: 75°F, wind E 30 km/h; 38 km from the airport.";
        let uk = TheirUnits::for_question(UK, "What's it like in Valdorrena?");
        assert_eq!(
            uk.convert(page).as_deref(),
            Some("Valdorrena: 24°C (75°F), wind E 19 mph (30 km/h); 24 miles (38 km) from the airport.")
        );
        let keys: Vec<&str> = uk.notes(page).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["temperatures", "speeds", "distances"]);
        assert_eq!(
            uk.notes(page)[1].1,
            "The user uses mph for speeds: Yap has put mph before these pages' km/h figures, like \"40 mph (64 km/h)\". Give every speed in mph (the km/h in brackets after it at most), converting any still in km/h."
        );
        // Ireland: the °F and the km/h stay apart — only what's theirs to convert.
        let ie = TheirUnits::for_question(IE, "Weather in Kilcrannagh?");
        assert_eq!(
            ie.convert("52°F, wind W 18 mph, 30 km/h gusts").as_deref(),
            Some("11°C (52°F), wind W 29 km/h (18 mph), 30 km/h gusts")
        );
        assert_eq!(
            ie.notes("wind W 18 mph")[0].1,
            "The user uses km/h for speeds: Yap has put km/h before these pages' mph figures, like \"64 km/h (40 mph)\". Give every speed in km/h (the mph in brackets after it at most), converting any still in mph."
        );
        // Nothing to convert, nothing known: no change, no line.
        assert_eq!(uk.convert("12°C, wind 9 mph, 4 miles"), None);
        assert!(uk.notes("12°C, wind 9 mph, 4 miles").is_empty());
        assert_eq!(TheirUnits::for_question(None, "x").convert(page), None);
    }
}
