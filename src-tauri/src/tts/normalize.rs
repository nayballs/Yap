//! **Text for the ear** (every engine): numbers, money, dates, times, units,
//! abbreviations, emails and web addresses written out as words, so a voice
//! says "twelve thousand four hundred pounds" for "£12,400" (the research
//! measured Kokoro saying "pound twelve thousand four hundred" without it).
//! UK style: "the eighteenth of October", "two thirty p.m.", "one hundred
//! and five".
//!
//! Conservative on purpose: a wrong expansion is worse than none, so every
//! rule needs a clear pattern (a currency sign, a month name, am/pm, a known
//! unit after a number), and anything else is left as it is. Pure, and runs
//! on one block of plain text (markdown already out, `tts::speakable_blocks`)
//! before it's cut into sentences.

use std::sync::LazyLock;

use regex::{Captures, Regex};

const ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const SCALES: [&str; 5] = ["", "thousand", "million", "billion", "trillion"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Below a hundred: "forty-five".
fn under_hundred(n: u64) -> String {
    match n {
        0..=19 => ONES[n as usize].to_string(),
        _ if n.is_multiple_of(10) => TENS[(n / 10) as usize].to_string(),
        _ => format!("{}-{}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]),
    }
}

/// Below a thousand, UK style: "three hundred and forty-five".
fn under_thousand(n: u64) -> String {
    let (h, rest) = (n / 100, n % 100);
    match (h, rest) {
        (0, _) => under_hundred(rest),
        (_, 0) => format!("{} hundred", ONES[h as usize]),
        _ => format!("{} hundred and {}", ONES[h as usize], under_hundred(rest)),
    }
}

/// A whole number in words, UK style: 12400 → "twelve thousand four
/// hundred", 2005 → "two thousand and five", 1234 → "one thousand two
/// hundred and thirty-four".
pub fn cardinal(n: u64) -> String {
    if n < 1000 {
        return under_thousand(n);
    }
    let mut groups = Vec::new();
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    if groups.len() > SCALES.len() {
        return digits(&n.to_string());
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, &g) in groups.iter().enumerate().rev() {
        if g == 0 {
            continue;
        }
        let words = under_thousand(g);
        if i == 0 {
            // "two thousand and five": the last group, under a hundred, after a bigger one.
            if g < 100 && !parts.is_empty() {
                parts.push(format!("and {words}"));
            } else {
                parts.push(words);
            }
        } else {
            parts.push(format!("{words} {}", SCALES[i]));
        }
    }
    parts.join(" ")
}

/// Digit by digit: "007" → "zero zero seven".
pub fn digits(s: &str) -> String {
    s.chars().filter_map(|c| c.to_digit(10)).map(|d| ONES[d as usize]).collect::<Vec<_>>().join(" ")
}

/// "twenty-one" → "twenty-first", "twelve" → "twelfth".
pub fn ordinal(n: u64) -> String {
    let words = cardinal(n);
    let (head, last) = match words.rfind([' ', '-']) {
        Some(i) => (&words[..=i], &words[i + 1..]),
        None => ("", words.as_str()),
    };
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    format!("{head}{last}")
}

/// A year as it's said: 1999 → "nineteen ninety-nine", 2005 → "two
/// thousand and five", 2026 → "twenty twenty-six", 1905 → "nineteen oh
/// five", 1900 → "nineteen hundred".
pub fn year(n: u64) -> String {
    if !(1000..=2999).contains(&n) || (2000..=2009).contains(&n) {
        return cardinal(n);
    }
    let (hi, lo) = (n / 100, n % 100);
    match lo {
        0 => format!("{} hundred", under_hundred(hi)),
        1..=9 => format!("{} oh {}", under_hundred(hi), ONES[lo as usize]),
        _ => format!("{} {}", under_hundred(hi), under_hundred(lo)),
    }
}

/// "3.14" → "three point one four" (the whole part as a number, the rest
/// digit by digit). Separators in the whole part are fine.
pub fn decimal(s: &str) -> String {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let whole: String = whole.chars().filter(char::is_ascii_digit).collect();
    let mut out = whole.parse::<u64>().map(cardinal).unwrap_or_else(|_| digits(&whole));
    if !frac.is_empty() {
        out.push_str(" point ");
        out.push_str(&digits(frac));
    }
    out
}

/// A number as written ("12,400", "3.5", "45") in words.
fn number(s: &str) -> String {
    if s.contains('.') {
        return decimal(s);
    }
    let plain: String = s.chars().filter(char::is_ascii_digit).collect();
    plain.parse::<u64>().map(cardinal).unwrap_or_else(|_| digits(&plain))
}

fn month_index(name: &str) -> Option<usize> {
    let lower = name.trim_end_matches('.').to_ascii_lowercase();
    if lower.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.to_ascii_lowercase().starts_with(&lower) && (lower.len() == 3 || m.eq_ignore_ascii_case(&lower) || lower == "sept"))
}

const MONTH_RE: &str = r"(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?|Sept?(?:ember)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)";

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(EMAIL, r"\b([A-Za-z0-9._%+-]+)@([A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,})\b");
re!(
    DOMAIN,
    r"(?i)\b((?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+(?:com|org|net|io|ai|app|dev|co|uk|gov|edu|info|me|tv|so|sh|xyz|tech|cloud|page|site|us|eu|de|fr|ca|au|nz|ie|in))\b"
);
re!(
    CURRENCY,
    r"(^|[^\w.])(-)?([£$€])\s?(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?(?:\s?(k|K|m|bn|b|tn|million|billion|thousand|trillion))?\b"
);
re!(PENCE, r"\b(\d{1,2})p\b");
re!(ISO_DATE, r"\b(\d{4})-(\d{2})-(\d{2})\b");
re!(SLASH_DATE, r"\b(\d{1,2})/(\d{1,2})/(\d{4}|\d{2})\b");
static DAY_MONTH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i:\b(the)\s+)?\b(\d{{1,2}})(?:st|nd|rd|th)?\s+(?:of\s+)?{MONTH_RE}\b\.?(?:,?\s+(\d{{4}})\b)?")).unwrap()
});
static MONTH_DAY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\b{MONTH_RE}\.?\s+(\d{{1,2}})(?:st|nd|rd|th)?\b(?:,?\s+(\d{{4}})\b)?")).unwrap());
re!(WEEKDAY, r"\b(Mon|Tue|Tues|Wed|Thu|Thur|Thurs|Fri|Sat|Sun)\b\.?(,?\s+(?:the\s+)?\d)");
re!(TIME_AMPM, r"(?i)\b(\d{1,2})(?:[:.](\d{2}))?\s?(a\.?m\.?|p\.?m\.?)(\W|$)");
re!(TIME_24, r"\b([01]?\d|2[0-3]):([0-5]\d)\b");
re!(TIME_RANGE_AMPM, r"(?i)\b(\d{1,2})\s?[–—-]\s?(\d{1,2})\s?(a\.?m\.?|p\.?m\.?)(\W|$)");
re!(PERCENT, r"(^|[^\w.])(-)?(\d+(?:,\d{3})*(?:\.\d+)?)\s?%");
re!(ORDINAL, r"\b(\d{1,4})(st|nd|rd|th)\b");
re!(VERSION_V, r"\bv(\d+(?:\.\d+)+)\b");
re!(VERSION_3, r"\b(\d+)\.(\d+)\.(\d+)\b");
re!(FRACTION, r"(^|\s)(1/2|1/3|2/3|1/4|3/4)(\s|$|[.,;:!?])");
re!(RANGE, r"\b(\d{1,4})\s?(–|—|-)\s?(\d{1,4})\b");
re!(DECADE, r"(?:\b(1[1-9]|20)(\d)0s|'(\d)0s)\b");
re!(
    UNIT,
    r"\b(\d+(?:,\d{3})*(?:\.\d+)?)\s?(km/h|km|kg|cm|mm|ml|mph|kph|kWh|kW|GHz|MHz|kHz|Hz|GB|MB|KB|TB|ms|°C|°F)(\b|$|[^\w])"
);
re!(ALNUM_LETTER_DIGIT, r"\b([A-Z])(\d{1,2})\b");
re!(ALNUM_DIGIT_LETTERS, r"\b(\d{1,3})([A-Z]{1,2})\b");
re!(NUMBER, r"(^|[^\w.])(-)?(\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+\.\d+|\d+)");
re!(ETC, r"\betc\.(\s+[A-Z]|\s*$)?");
re!(EG, r"(?i)\b(e\.g|i\.e)\.?(,)?");
re!(VERSUS, r"\bvs\.?(\s)");
re!(APPROX, r"\bapprox\.(\s)");
re!(TITLE, r"\b(Dr|Mr|Mrs|Ms|Prof)\.?(\s+[A-Z])");
re!(NUMBER_SIGN, r"(^|\s)(?:No\.\s?|#)(\d)");
re!(AMPERSAND, r"(\s)&(\s)");
re!(WITH, r"(^|\s)w/(o)?(\s)");
re!(ABOUT, r"(^|\s)~\s?(\d)");
re!(PLUS, r"(\d)\s?\+\s?(\d)");
re!(TIMES, r"(\d)\s?×\s?(\d)");

/// The words for a currency: (one, many, small one, small many).
fn currency_words(sign: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match sign {
        "£" => ("pound", "pounds", "penny", "pence"),
        "€" => ("euro", "euros", "cent", "cents"),
        _ => ("dollar", "dollars", "cent", "cents"),
    }
}

fn scale_word(s: &str) -> Option<&'static str> {
    match s {
        "k" | "K" | "thousand" => Some("thousand"),
        "m" | "million" => Some("million"),
        "b" | "bn" | "billion" => Some("billion"),
        "tn" | "trillion" => Some("trillion"),
        _ => None,
    }
}

/// "£12,400" → "twelve thousand four hundred pounds", "$3.50" → "three
/// dollars fifty", "£0.50" → "fifty pence", "£1.5m" → "one point five
/// million pounds".
fn money(sign: &str, whole: &str, cents: Option<&str>, scale: Option<&str>) -> String {
    let (one, many, small_one, small_many) = currency_words(sign);
    let whole_n: u64 = whole.replace(',', "").parse().unwrap_or(0);
    if let Some(scale) = scale.and_then(scale_word) {
        let amount = match cents {
            Some(c) => format!("{} point {}", cardinal(whole_n), digits(c)),
            None => cardinal(whole_n),
        };
        return format!("{amount} {scale} {many}");
    }
    // "£12.5" is twelve pounds fifty.
    let cents_n: u64 = cents.map(|c| if c.len() == 1 { format!("{c}0") } else { c.to_string() }).and_then(|c| c.parse().ok()).unwrap_or(0);
    let unit = if whole_n == 1 { one } else { many };
    match (whole_n, cents_n) {
        (0, 0) => format!("zero {many}"),
        (0, c) => format!("{} {}", cardinal(c), if c == 1 { small_one } else { small_many }),
        (w, 0) => format!("{} {unit}", cardinal(w)),
        (w, c) if c < 10 => format!("{} {unit} and {} {}", cardinal(w), cardinal(c), if c == 1 { small_one } else { small_many }),
        (w, c) => format!("{} {unit} {}", cardinal(w), cardinal(c)),
    }
}

/// "the eighteenth of October twenty twenty-six".
fn day_month(day: u64, month: usize, year_n: Option<u64>, with_the: bool) -> String {
    let mut s = format!("{}{} of {}", if with_the { "the " } else { "" }, ordinal(day), MONTHS[month]);
    if let Some(y) = year_n {
        s.push(' ');
        s.push_str(&year(y));
    }
    s
}

fn valid_date(day: u64, month: u64) -> bool {
    (1..=31).contains(&day) && (1..=12).contains(&month)
}

/// "2:30 pm" → "two thirty p.m.", "3pm" → "three p.m.", "10:05" → "ten
/// oh five", "14:00" → "fourteen hundred", "9:00" → "nine o'clock".
fn clock(hour: u64, minute: Option<u64>, suffix: Option<&str>) -> String {
    let h = cardinal(hour);
    let m = match minute {
        None => None,
        Some(0) if suffix.is_some() => None,
        Some(0) if hour >= 13 || hour == 0 => Some("hundred".to_string()),
        Some(0) => Some("o'clock".to_string()),
        Some(m @ 1..=9) => Some(format!("oh {}", ONES[m as usize])),
        Some(m) => Some(cardinal(m)),
    };
    let mut s = match m {
        Some(m) => format!("{h} {m}"),
        None => h,
    };
    if let Some(suf) = suffix {
        s.push(' ');
        s.push_str(if suf.to_ascii_lowercase().starts_with('a') { "a.m." } else { "p.m." });
    }
    s
}

fn unit_words(unit: &str, one: bool) -> &'static str {
    let (s, p) = match unit {
        "km" => ("kilometre", "kilometres"),
        "km/h" | "kph" => ("kilometre an hour", "kilometres an hour"),
        "kg" => ("kilogram", "kilograms"),
        "cm" => ("centimetre", "centimetres"),
        "mm" => ("millimetre", "millimetres"),
        "ml" => ("millilitre", "millilitres"),
        "mph" => ("mile an hour", "miles an hour"),
        "kWh" => ("kilowatt hour", "kilowatt hours"),
        "kW" => ("kilowatt", "kilowatts"),
        "GHz" => ("gigahertz", "gigahertz"),
        "MHz" => ("megahertz", "megahertz"),
        "kHz" => ("kilohertz", "kilohertz"),
        "Hz" => ("hertz", "hertz"),
        "GB" => ("gigabyte", "gigabytes"),
        "MB" => ("megabyte", "megabytes"),
        "KB" => ("kilobyte", "kilobytes"),
        "TB" => ("terabyte", "terabytes"),
        "ms" => ("millisecond", "milliseconds"),
        "°C" => ("degree Celsius", "degrees Celsius"),
        "°F" => ("degree Fahrenheit", "degrees Fahrenheit"),
        _ => ("", ""),
    };
    if one {
        s
    } else {
        p
    }
}

/// Words that put a four-digit number before them in a year's voice.
fn year_context(before: &str) -> bool {
    let word = before.trim_end().rsplit(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_ascii_lowercase();
    matches!(
        word.as_str(),
        "in" | "since" | "by" | "from" | "until" | "till" | "of" | "year" | "before" | "after" | "during" | "around"
            | "circa" | "early" | "late" | "mid" | "spring" | "summer" | "autumn" | "winter" | "fall" | "q1" | "q2"
            | "q3" | "q4" | "fy"
    ) || MONTHS.iter().any(|m| m.eq_ignore_ascii_case(&word))
}

/// Text written for the eye, made ready to say. See the module docs.
pub fn for_speech(text: &str) -> String {
    let s = EMAIL.replace_all(text, |c: &Captures| {
        let local = c[1].replace('.', " dot ").replace('_', " underscore ");
        format!("{local} at {}", c[2].to_ascii_lowercase().replace('.', " dot "))
    });
    let s = DOMAIN.replace_all(&s, |c: &Captures| c[1].replace('.', " dot "));
    let s = CURRENCY.replace_all(&s, |c: &Captures| {
        let minus = if c.get(2).is_some() { "minus " } else { "" };
        format!("{}{minus}{}", &c[1], money(&c[3], &c[4], c.get(5).map(|m| m.as_str()), c.get(6).map(|m| m.as_str())))
    });
    let s = PENCE.replace_all(&s, |c: &Captures| {
        let n: u64 = c[1].parse().unwrap_or(0);
        format!("{} {}", cardinal(n), if n == 1 { "penny" } else { "pence" })
    });
    // Dates, before any number rule takes their digits.
    let s = ISO_DATE.replace_all(&s, |c: &Captures| {
        let (y, m, d) = (c[1].parse::<u64>().unwrap_or(0), c[2].parse::<u64>().unwrap_or(0), c[3].parse::<u64>().unwrap_or(0));
        if valid_date(d, m) && (1000..=2999).contains(&y) {
            day_month(d, (m - 1) as usize, Some(y), true)
        } else {
            c[0].to_string()
        }
    });
    let s = SLASH_DATE.replace_all(&s, |c: &Captures| {
        let (a, b) = (c[1].parse::<u64>().unwrap_or(0), c[2].parse::<u64>().unwrap_or(0));
        let y: u64 = c[3].parse().unwrap_or(0);
        let y = if c[3].len() == 2 { 2000 + y } else { y };
        // Day first (UK) unless only month-first makes sense.
        let (d, m) = if a <= 12 && b > 12 { (b, a) } else { (a, b) };
        if valid_date(d, m) {
            day_month(d, (m - 1) as usize, Some(y), true)
        } else {
            c[0].to_string()
        }
    });
    let s = WEEKDAY.replace_all(&s, |c: &Captures| {
        let day = match &c[1] {
            "Mon" => "Monday",
            "Tue" | "Tues" => "Tuesday",
            "Wed" => "Wednesday",
            "Thu" | "Thur" | "Thurs" => "Thursday",
            "Fri" => "Friday",
            "Sat" => "Saturday",
            _ => "Sunday",
        };
        format!("{day}{}", &c[2])
    });
    let s = DAY_MONTH.replace_all(&s, |c: &Captures| {
        let day: u64 = c[2].parse().unwrap_or(0);
        match month_index(&c[3]) {
            Some(m) if (1..=31).contains(&day) => {
                let y = c.get(4).and_then(|y| y.as_str().parse().ok());
                // "the 18th of Oct" keeps its "the"; "18 Oct" gets one.
                let lead = c.get(1).map_or("", |m| m.as_str());
                let text = day_month(day, m, y, true);
                if lead.is_empty() {
                    text
                } else {
                    format!("{lead}{}", &text[3..])
                }
            }
            _ => c[0].to_string(),
        }
    });
    let s = MONTH_DAY.replace_all(&s, |c: &Captures| {
        let day: u64 = c[2].parse().unwrap_or(0);
        match month_index(&c[1]) {
            Some(m) if (1..=31).contains(&day) => {
                let mut out = format!("{} the {}", MONTHS[m], ordinal(day));
                if let Some(y) = c.get(3).and_then(|y| y.as_str().parse::<u64>().ok()) {
                    out.push(' ');
                    out.push_str(&year(y));
                }
                out
            }
            _ => c[0].to_string(),
        }
    });
    // Times.
    let s = TIME_RANGE_AMPM.replace_all(&s, |c: &Captures| {
        let (a, b) = (c[1].parse::<u64>().unwrap_or(0), c[2].parse::<u64>().unwrap_or(0));
        if (1..=12).contains(&a) && (1..=12).contains(&b) {
            format!("{} to {}{}", cardinal(a), clock(b, None, Some(&c[3])), &c[4])
        } else {
            c[0].to_string()
        }
    });
    let s = TIME_AMPM.replace_all(&s, |c: &Captures| {
        let hour: u64 = c[1].parse().unwrap_or(99);
        let minute = c.get(2).and_then(|m| m.as_str().parse::<u64>().ok());
        if (1..=12).contains(&hour) && minute.is_none_or(|m| m < 60) {
            format!("{}{}", clock(hour, minute, Some(&c[3])), &c[4])
        } else {
            c[0].to_string()
        }
    });
    let s = TIME_24.replace_all(&s, |c: &Captures| {
        let (h, m) = (c[1].parse::<u64>().unwrap_or(0), c[2].parse::<u64>().unwrap_or(0));
        if h == 0 && m == 0 {
            "midnight".to_string()
        } else {
            clock(h, Some(m), None)
        }
    });
    let s = PERCENT.replace_all(&s, |c: &Captures| {
        let minus = if c.get(2).is_some() { "minus " } else { "" };
        format!("{}{minus}{} percent", &c[1], number(&c[3]))
    });
    let s = ORDINAL.replace_all(&s, |c: &Captures| c[1].parse::<u64>().map(ordinal).unwrap_or_else(|_| c[0].to_string()));
    let s = VERSION_V.replace_all(&s, |c: &Captures| {
        let parts: Vec<String> = c[1].split('.').map(number).collect();
        format!("version {}", parts.join(" point "))
    });
    let s = VERSION_3.replace_all(&s, |c: &Captures| format!("{} point {} point {}", number(&c[1]), number(&c[2]), number(&c[3])));
    let s = FRACTION.replace_all(&s, |c: &Captures| {
        let words = match &c[2] {
            "1/2" => "a half",
            "1/3" => "a third",
            "2/3" => "two thirds",
            "1/4" => "a quarter",
            _ => "three quarters",
        };
        format!("{}{words}{}", &c[1], &c[3])
    });
    let s = DECADE.replace_all(&s, |c: &Captures| {
        let tens = c.get(2).or(c.get(3)).map_or(0, |m| m.as_str().parse::<u64>().unwrap_or(0));
        let decade = if tens == 0 { "hundreds".to_string() } else { format!("{}ies", &TENS[tens as usize][..TENS[tens as usize].len() - 1]) };
        match c.get(1) {
            Some(century) if century.as_str() == "20" && tens == 0 => "two thousands".to_string(),
            Some(century) => format!("{} {decade}", under_hundred(century.as_str().parse().unwrap_or(19))),
            None => decade,
        }
    });
    let s = UNIT.replace_all(&s, |c: &Captures| {
        let n = &c[1];
        format!("{} {}{}", number(n), unit_words(&c[2], n == "1"), &c[3])
    });
    let s = RANGE.replace_all(&s, |c: &Captures| {
        let (a, b) = (c[1].parse::<u64>().unwrap_or(0), c[3].parse::<u64>().unwrap_or(0));
        let coded = |n: &str| n.len() > 1 && n.starts_with('0');
        let is_year = |n: u64| (1100..=2099).contains(&n);
        // A hyphen between two numbers is a range only when it climbs
        // ("10-15 minutes"); "2-1" (a score) and "0123-4567" stay.
        if coded(&c[1]) || coded(&c[3]) || (&c[2] == "-" && a >= b) {
            c[0].to_string()
        } else if c[1].len() == 4 && c[3].len() == 4 && is_year(a) && is_year(b) {
            format!("{} to {}", year(a), year(b))
        } else {
            format!("{} to {}", cardinal(a), cardinal(b))
        }
    });
    let s = ALNUM_LETTER_DIGIT.replace_all(&s, |c: &Captures| format!("{} {}", &c[1], number(&c[2])));
    let s = ALNUM_DIGIT_LETTERS.replace_all(&s, |c: &Captures| format!("{} {}", number(&c[1]), &c[2]));
    let s = ETC.replace_all(&s, |c: &Captures| match c.get(1) {
        // A sentence ends there: keep its full stop.
        Some(next) => format!("et cetera.{}", next.as_str()),
        None => "et cetera".to_string(),
    });
    let s = EG.replace_all(&s, |c: &Captures| {
        let words = if c[1].eq_ignore_ascii_case("e.g") { "for example" } else { "that is" };
        format!("{words}{}", c.get(2).map_or("", |m| m.as_str()))
    });
    let s = VERSUS.replace_all(&s, "versus$1");
    let s = APPROX.replace_all(&s, "approximately$1");
    let s = TITLE.replace_all(&s, |c: &Captures| {
        let word = match &c[1] {
            "Dr" => "Doctor",
            "Mr" => "Mister",
            "Mrs" => "Missus",
            "Ms" => "Miz",
            _ => "Professor",
        };
        format!("{word}{}", &c[2])
    });
    let s = NUMBER_SIGN.replace_all(&s, "${1}number $2");
    let s = AMPERSAND.replace_all(&s, "${1}and$2");
    let s = WITH.replace_all(&s, |c: &Captures| format!("{}with{}{}", &c[1], if c.get(2).is_some() { "out" } else { "" }, &c[3]));
    let s = ABOUT.replace_all(&s, "${1}about $2");
    let s = PLUS.replace_all(&s, "$1 plus $2");
    let s = TIMES.replace_all(&s, "$1 by $2");
    let s = numbers(&s);
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whatever numbers are left: "12,400", "3.14", "45", a year after "in",
/// a code digit by digit. A word written straight after one ("45min")
/// gets a space.
fn numbers(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut last = 0;
    for c in NUMBER.captures_iter(s) {
        let (Some(all), Some(num)) = (c.get(0), c.get(3)) else {
            continue;
        };
        out.push_str(&s[last..all.start()]);
        out.push_str(&c[1]);
        let minus = c.get(2).is_some();
        let n = num.as_str();
        let words = if n.contains('.') || n.contains(',') {
            number(n)
        } else if (n.len() > 1 && n.starts_with('0')) || n.len() >= 7 {
            // A code, a phone number, an id: digit by digit.
            digits(n)
        } else if n.len() == 4 && !minus && year_context(&s[..all.start() + c[1].len()]) {
            n.parse().map(year).unwrap_or_else(|_| digits(n))
        } else {
            number(n)
        };
        if minus {
            out.push_str("minus ");
        }
        out.push_str(&words);
        if s[all.end()..].starts_with(|ch: char| ch.is_alphabetic()) {
            out.push(' ');
        }
        last = all.end();
    }
    out.push_str(&s[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_words() {
        assert_eq!(cardinal(0), "zero");
        assert_eq!(cardinal(7), "seven");
        assert_eq!(cardinal(45), "forty-five");
        assert_eq!(cardinal(100), "one hundred");
        assert_eq!(cardinal(105), "one hundred and five");
        assert_eq!(cardinal(345), "three hundred and forty-five");
        assert_eq!(cardinal(1000), "one thousand");
        assert_eq!(cardinal(2005), "two thousand and five");
        assert_eq!(cardinal(1234), "one thousand two hundred and thirty-four");
        assert_eq!(cardinal(12_400), "twelve thousand four hundred");
        assert_eq!(cardinal(1_000_000), "one million");
        assert_eq!(cardinal(3_200_001), "three million two hundred thousand and one");
        assert_eq!(ordinal(1), "first");
        assert_eq!(ordinal(2), "second");
        assert_eq!(ordinal(3), "third");
        assert_eq!(ordinal(12), "twelfth");
        assert_eq!(ordinal(14), "fourteenth");
        assert_eq!(ordinal(20), "twentieth");
        assert_eq!(ordinal(21), "twenty-first");
        assert_eq!(ordinal(103), "one hundred and third");
        assert_eq!(year(1999), "nineteen ninety-nine");
        assert_eq!(year(1905), "nineteen oh five");
        assert_eq!(year(1900), "nineteen hundred");
        assert_eq!(year(2005), "two thousand and five");
        assert_eq!(year(2026), "twenty twenty-six");
        assert_eq!(year(2010), "twenty ten");
        assert_eq!(decimal("3.14"), "three point one four");
        assert_eq!(decimal("0.5"), "zero point five");
        assert_eq!(digits("007"), "zero zero seven");
    }

    #[test]
    fn money() {
        assert_eq!(for_speech("It came to £12,400."), "It came to twelve thousand four hundred pounds.");
        assert_eq!(for_speech("$3.50 each"), "three dollars fifty each");
        assert_eq!(for_speech("€1 or £1"), "one euro or one pound");
        assert_eq!(for_speech("only £0.50"), "only fifty pence");
        assert_eq!(for_speech("$0.01"), "one cent");
        assert_eq!(for_speech("£12.5"), "twelve pounds fifty");
        assert_eq!(for_speech("$3.05"), "three dollars and five cents");
        assert_eq!(for_speech("a £1.5m deal"), "a one point five million pounds deal");
        assert_eq!(for_speech("$3k a month"), "three thousand dollars a month");
        assert_eq!(for_speech("€2bn"), "two billion euros");
        assert_eq!(for_speech("down -£5"), "down minus five pounds");
        assert_eq!(for_speech("costs 50p"), "costs fifty pence");
        // Not money: resolutions, and a dollar sign in code.
        assert_eq!(for_speech("1080p video"), "one thousand and eighty p video");
    }

    #[test]
    fn dates_uk_style() {
        assert_eq!(for_speech("on 18 Oct"), "on the eighteenth of October");
        assert_eq!(for_speech("Due 18 October 2026."), "Due the eighteenth of October twenty twenty-six.");
        assert_eq!(for_speech("the 1st of May"), "the first of May");
        assert_eq!(for_speech("notes from October 14th."), "notes from October the fourteenth.");
        assert_eq!(for_speech("Sat 18 Oct"), "Saturday the eighteenth of October");
        assert_eq!(for_speech("on 2026-10-18"), "on the eighteenth of October twenty twenty-six");
        assert_eq!(for_speech("14/10/2026"), "the fourteenth of October twenty twenty-six");
        // Day first, unless only month first makes sense.
        assert_eq!(for_speech("3/4/2026"), "the third of April twenty twenty-six");
        assert_eq!(for_speech("10/25/2026"), "the twenty-fifth of October twenty twenty-six");
        // Not dates.
        assert_eq!(for_speech("13/13/2026"), "thirteen/thirteen/two thousand and twenty-six");
        assert_eq!(for_speech("You may 5 times"), "You may five times");
        assert_eq!(for_speech("in 2026"), "in twenty twenty-six");
        assert_eq!(for_speech("since 1999, about 1999 people"), "since nineteen ninety-nine, about one thousand nine hundred and ninety-nine people");
        assert_eq!(for_speech("the 1990s"), "the nineteen nineties");
        assert_eq!(for_speech("the '80s"), "the eighties");
    }

    #[test]
    fn times() {
        assert_eq!(for_speech("Thursday at 2:30 pm, for"), "Thursday at two thirty p.m., for");
        assert_eq!(for_speech("at 2:30pm."), "at two thirty p.m.");
        assert_eq!(for_speech("at 2.30pm"), "at two thirty p.m.");
        assert_eq!(for_speech("at 3pm"), "at three p.m.");
        assert_eq!(for_speech("10 AM"), "ten a.m.");
        assert_eq!(for_speech("10:05 a.m."), "ten oh five a.m.");
        assert_eq!(for_speech("2:00 pm"), "two p.m.");
        assert_eq!(for_speech("at 14:30"), "at fourteen thirty");
        assert_eq!(for_speech("at 14:00"), "at fourteen hundred");
        assert_eq!(for_speech("at 9:00"), "at nine o'clock");
        assert_eq!(for_speech("2-3pm"), "two to three p.m.");
        // Not a time: a ratio.
        assert_eq!(for_speech("a 3:1 ratio"), "a three:one ratio");
    }

    #[test]
    fn percents_ordinals_versions_units() {
        assert_eq!(for_speech("up 45%"), "up forty-five percent");
        assert_eq!(for_speech("3.5% growth"), "three point five percent growth");
        assert_eq!(for_speech("the 2nd and 23rd"), "the second and twenty-third");
        assert_eq!(for_speech("v2.1 is out"), "version two point one is out");
        assert_eq!(for_speech("Yap 1.2.3"), "Yap one point two point three");
        assert_eq!(for_speech("Python 3.12"), "Python three point one two");
        assert_eq!(for_speech("5 km away"), "five kilometres away");
        assert_eq!(for_speech("1 km"), "one kilometre");
        assert_eq!(for_speech("2.5kg"), "two point five kilograms");
        assert_eq!(for_speech("16 GB of RAM"), "sixteen gigabytes of RAM");
        assert_eq!(for_speech("21°C"), "twenty-one degrees Celsius");
        assert_eq!(for_speech("120 ms"), "one hundred and twenty milliseconds");
        assert_eq!(for_speech("half 1/2 done"), "half a half done");
        assert_eq!(for_speech("10-15 minutes"), "ten to fifteen minutes");
        assert_eq!(for_speech("pages 3–5"), "pages three to five");
        assert_eq!(for_speech("won 2-1"), "won two-one");
    }

    #[test]
    fn plain_numbers() {
        assert_eq!(for_speech("about 45 minutes"), "about forty-five minutes");
        assert_eq!(for_speech("12,400 people"), "twelve thousand four hundred people");
        assert_eq!(for_speech("code 0042"), "code zero zero four two");
        assert_eq!(for_speech("call 07946095800"), "call zero seven nine four six zero nine five eight zero zero");
        assert_eq!(for_speech("-3 degrees"), "minus three degrees");
        assert_eq!(for_speech("the Q3 budget"), "the Q three budget");
        assert_eq!(for_speech("a 4K screen"), "a four K screen");
        assert_eq!(for_speech("2 + 2 × 3"), "two plus two by three");
    }

    #[test]
    fn abbreviations_emails_domains() {
        assert_eq!(for_speech("fruit, e.g. apples"), "fruit, for example apples");
        assert_eq!(for_speech("one, i.e., this"), "one, that is, this");
        assert_eq!(for_speech("apples, pears, etc. Then more"), "apples, pears, et cetera. Then more");
        assert_eq!(for_speech("apples, pears, etc."), "apples, pears, et cetera.");
        assert_eq!(for_speech("apples etc. and more"), "apples et cetera and more");
        assert_eq!(for_speech("Yap vs. Wispr"), "Yap versus Wispr");
        assert_eq!(for_speech("Ask Dr. Patel"), "Ask Doctor Patel");
        assert_eq!(for_speech("Mr Smith and Mrs. Jones"), "Mister Smith and Missus Jones");
        assert_eq!(for_speech("approx. 3"), "approximately three");
        assert_eq!(for_speech("No. 5 and #1"), "number five and number one");
        assert_eq!(for_speech("Sam & Jo"), "Sam and Jo");
        assert_eq!(for_speech("tea w/ milk, w/o sugar"), "tea with milk, without sugar");
        assert_eq!(for_speech("~5 minutes"), "about five minutes");
        assert_eq!(for_speech("Email priya.shah@example.co.uk now"), "Email priya dot shah at example dot co dot uk now");
        assert_eq!(for_speech("see github.com"), "see github dot com");
        // The doctor, not a word ending "dr".
        assert_eq!(for_speech("the dr. said"), "the dr. said");
    }

    #[test]
    fn the_research_answer() {
        let raw = "Sure! Your meeting with Priya Shah is on Thursday at 2:30 pm, for about 45 minutes. Before then, three things. One, send her the Q3 budget, which came to £12,400. Two, book a room for four people. Three, reread your notes from October 14th. Want me to draft the email?";
        assert_eq!(
            for_speech(raw),
            "Sure! Your meeting with Priya Shah is on Thursday at two thirty p.m., for about forty-five minutes. Before then, three things. One, send her the Q three budget, which came to twelve thousand four hundred pounds. Two, book a room for four people. Three, reread your notes from October the fourteenth. Want me to draft the email?"
        );
        // Text without anything to change comes back as it was.
        let plain = "Nothing to change here, really.";
        assert_eq!(for_speech(plain), plain);
    }
}
