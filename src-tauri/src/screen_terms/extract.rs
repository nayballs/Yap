//! **Names and terms in a screen's text** (pure): what in a window's visible
//! text looks like something a dictation should spell exactly as shown —
//! a name ("Rochelle", "Zentrix"), a full name ("Siobhan Ní Bhriain"), a
//! word with capitals inside ("GitHub", "iPhone"), an acronym ("NASA",
//! "GPT-OSS"), a word with digits ("Qwen3", "GPT-4o") or a handle
//! ("@zentrix_dev", an email address). Everyday words are left out however
//! they're written (`is_word`: the everyday list, then Windows' spell
//! checker), so "The", "Getting Started" or "WARNING" never count; so are
//! measures ("10kg", "3rd", "5pm"), paths, URLs and code identifiers.

use std::collections::HashMap;

/// What kind of term it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A capitalised word that isn't an everyday word ("Rochelle", "Zentrix").
    Name,
    /// Names side by side ("Siobhan Ní Bhriain", "Ludwig van Beethoven").
    FullName,
    /// Capitals inside a word ("GitHub", "iPhone", "PostgreSQL").
    MixedCase,
    /// All capitals, three letters or more ("NASA", "GPT-OSS").
    Acronym,
    /// Letters and digits ("Qwen3", "GPT-4o", "M365").
    WithDigits,
    /// "@handle", or an email address.
    Handle,
}

/// A term found on screen: as written there (its most frequent spelling),
/// what kind it is, and how often it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub text: String,
    pub kind: Kind,
    pub count: u32,
}

/// At most this many terms are kept (the most frequent first).
pub const MAX_TERMS: usize = 200;

/// Lower-case words that join the parts of a full name ("Ludwig van
/// Beethoven", "Siobhan Ní Bhriain" — written with a capital too).
const PARTICLES: &[&str] = &[
    "de", "da", "di", "du", "van", "von", "der", "den", "del", "della", "la", "le", "dos", "das", "bin", "ibn", "al",
    "el", "ní", "nic", "ó", "ui", "mac", "ap", "ter", "ten", "y",
];

/// "10kg", "3rd", "5pm", "100ms", "2GB": a number and its unit, not a term.
fn is_measure(core: &str) -> bool {
    const UNITS: &[&str] = &[
        "st", "nd", "rd", "th", "am", "pm", "s", "k", "m", "b", "x", "h", "d", "p", "kg", "km", "mb", "gb", "tb", "kb",
        "ms", "px", "pt", "em", "rem", "min", "mins", "hr", "hrs", "mph", "kph", "mm", "cm", "ml", "fps", "hz", "khz",
        "mhz", "ghz", "gbps", "mbps", "kbps", "v", "w", "kw", "kwh", "mah", "in", "ft", "lb", "lbs", "oz", "yr", "yrs",
        "mo", "wk", "wks", "sec", "secs", "g", "l", "dpi", "bit", "bits", "fr", "k+", "c", "f",
    ];
    let lower = core.to_lowercase();
    let digits_end = lower
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.' || *c == ','))
        .map_or(lower.len(), |(i, _)| i);
    digits_end > 0 && UNITS.contains(&&lower[digits_end..])
}

/// A token as it reads: its edges' punctuation off (a leading "@" kept for a
/// handle), a final possessive off ("Siobhan's"), and whether punctuation
/// was taken off before / after it (a full name never runs across that).
struct Token<'a> {
    core: &'a str,
    break_before: bool,
    break_after: bool,
}

fn token(raw: &str) -> Token<'_> {
    let start = raw
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric() || *c == '@')
        .map_or(raw.len(), |(i, _)| i);
    let end = raw
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(start, |(i, c)| i + c.len_utf8())
        .max(start);
    let mut core = &raw[start..end];
    for s in ["'s", "’s", "'S", "’S"] {
        if let Some(c) = core.strip_suffix(s) {
            if c.chars().any(char::is_alphabetic) {
                core = c;
            }
        }
    }
    Token {
        core,
        break_before: start > 0,
        break_after: end < raw.len(),
    }
}

/// An email address ("jane.doe@example.com") or an "@handle".
fn handle(core: &str) -> bool {
    let ok = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '+');
    if let Some(rest) = core.strip_prefix('@') {
        return rest.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && rest.chars().all(ok) && !rest.contains('@');
    }
    match core.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && local.chars().all(ok)
                && domain.contains('.')
                && domain.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'))
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
}

/// What a single token is: a term of a kind, a part of a full name only
/// (a capitalised short word, "Ní", an initial), or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Is {
    Term(Kind),
    /// Too short or too ordinary to be a term alone, but can join a full
    /// name ("Ní", "F", a particle).
    NamePart,
    Nothing,
}

fn classify(core: &str, is_word: &dyn Fn(&str) -> bool, spell_checker: bool) -> Is {
    if core.is_empty() {
        return Is::Nothing;
    }
    if core.contains('@') {
        return if handle(core) { Is::Term(Kind::Handle) } else { Is::Nothing };
    }
    // Paths, URLs, code: anything but letters, digits and a word's own
    // hyphens / apostrophes / dots.
    if !core.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '\'' | '’' | '.')) {
        return Is::Nothing;
    }
    let letters = core.chars().filter(|c| c.is_alphabetic()).count();
    let digits = core.chars().filter(|c| c.is_numeric()).count();
    let lower = core.to_lowercase();
    if letters == 0 {
        return Is::Nothing;
    }
    if digits > 0 {
        // "Qwen3", "GPT-4o", "M365" — not "Q3", "x86", "4K" or "10kg", nor a
        // code identifier or a username ("value12", "item2": no capital).
        let capital = core.chars().any(char::is_uppercase);
        return if capital && (letters >= 2 || digits >= 3) && !is_measure(core) && !core.contains('.') {
            Is::Term(Kind::WithDigits)
        } else {
            Is::Nothing
        };
    }
    let upper = core.chars().filter(|c| c.is_uppercase()).count();
    let first_upper = core.chars().next().is_some_and(char::is_uppercase);
    let key: String = lower.chars().filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-').collect();
    // All capitals: an acronym, unless it's a word shouted ("WARNING").
    if upper == letters {
        return if letters >= 3 && !is_word(&key.replace('-', "")) && !is_word(&key) {
            Is::Term(Kind::Acronym)
        } else if letters <= 2 {
            Is::NamePart
        } else {
            Is::Nothing
        };
    }
    // A capital right after a small letter: "GitHub", "iPhone", "macOS".
    let chars: Vec<char> = core.chars().collect();
    let mixed = chars.windows(2).any(|w| w[0].is_lowercase() && w[1].is_uppercase());
    if mixed {
        return Is::Term(Kind::MixedCase);
    }
    // A word with a dot inside that isn't an acronym or mixed case ("Node.js",
    // "readme.md", "e.g"): a file or an abbreviation, not a name.
    if core.contains('.') {
        return Is::Nothing;
    }
    if !first_upper {
        return if PARTICLES.contains(&lower.as_str()) { Is::NamePart } else { Is::Nothing };
    }
    // Capitalised: a name unless it's an everyday word ("The", "Monday",
    // "Getting"). Without a spell checker only the everyday list can say,
    // which leaves too many ordinary words: a capitalised word alone is
    // then only part of a full name.
    if letters < 3 || PARTICLES.contains(&lower.as_str()) {
        return Is::NamePart;
    }
    let plain = key.replace('’', "'");
    if is_word(&plain) || plain.split('-').all(|p| p.is_empty() || is_word(p)) {
        return Is::Nothing;
    }
    if spell_checker {
        Is::Term(Kind::Name)
    } else {
        Is::NamePart
    }
}

/// The names and terms in `text` (a window's visible text, its lines and
/// pieces joined by line breaks), the most frequent first (then the one
/// seen first), at most [`MAX_TERMS`]. `is_word`: is this lower-case word
/// an everyday word? `spell_checker`: does `is_word` have Windows' spell
/// checker behind it (else a plain capitalised word only counts as part of
/// a full name).
pub fn extract(text: &str, is_word: &dyn Fn(&str) -> bool, spell_checker: bool) -> Vec<Term> {
    // Spelling → (kind, count, first seen).
    let mut seen: HashMap<String, (Kind, u32, usize)> = HashMap::new();
    let mut order = 0usize;
    // The spell checker is asked once a word.
    let words_cache: std::cell::RefCell<HashMap<String, bool>> = Default::default();
    let cached = |w: &str| -> bool {
        if let Some(&v) = words_cache.borrow().get(w) {
            return v;
        }
        let v = is_word(w);
        words_cache.borrow_mut().insert(w.to_string(), v);
        v
    };
    let mut add = |text: String, kind: Kind, order: usize| {
        let e = seen.entry(text).or_insert((kind, 0, order));
        e.1 += 1;
    };
    // A full name in the making: (token, is it a name of its own). It ends
    // at punctuation, a line's end or anything but a name or a part of one;
    // trimmed of parts at its ends, 2 to 4 words make a full name.
    fn flush(run: &mut Vec<(&str, bool)>, add: &mut dyn FnMut(String, Kind, usize), order: usize) {
        while run.last().is_some_and(|(_, name)| !name) {
            run.pop();
        }
        while run.first().is_some_and(|(_, name)| !name) {
            run.remove(0);
        }
        if run.len() >= 2 && run.len() <= 4 {
            add(run.iter().map(|(t, _)| *t).collect::<Vec<_>>().join(" "), Kind::FullName, order);
        }
        run.clear();
    }
    for line in text.split(['\n', '\r', '\t', '|', '•', '·']) {
        // Brackets, quotes and code's punctuation part words as a comma
        // does: `useState('Zentrix')` holds "Zentrix", `<jane@example.com>`
        // the address. (A path or a URL stays one token: it's no term.)
        let mut parted = String::with_capacity(line.len() + 8);
        for c in line.chars() {
            if matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '“' | '”' | ';' | '=' | ',') {
                parted.push_str(" , ");
            } else {
                parted.push(c);
            }
        }
        let line = parted;
        let mut run: Vec<(&str, bool)> = Vec::new();
        for raw in line.split_whitespace() {
            let t = token(raw);
            if t.break_before {
                flush(&mut run, &mut add, order);
            }
            order += 1;
            match classify(t.core, &cached, spell_checker) {
                Is::Term(kind) => {
                    add(t.core.to_string(), kind, order);
                    if kind == Kind::Name {
                        run.push((t.core, true));
                    } else {
                        flush(&mut run, &mut add, order);
                    }
                }
                Is::NamePart => {
                    // Without a spell checker a capitalised word is only a
                    // part, but it may still start or end a full name.
                    let capitalised = t.core.chars().next().is_some_and(char::is_uppercase)
                        && t.core.chars().filter(|c| c.is_alphabetic()).count() >= 3;
                    run.push((t.core, capitalised && !spell_checker && !cached(&t.core.to_lowercase())));
                }
                Is::Nothing => flush(&mut run, &mut add, order),
            }
            if t.break_after {
                flush(&mut run, &mut add, order);
            }
        }
        flush(&mut run, &mut add, order);
    }

    // One spelling per term (case aside): the most frequent, then a
    // capitalised or mixed one over all capitals ("Zentrix", not a heading's
    // "ZENTRIX"), then the first seen; the counts add up.
    let mut by_key: HashMap<String, Vec<(String, Kind, u32, usize)>> = HashMap::new();
    for (text, (kind, count, first)) in seen {
        by_key.entry(text.to_lowercase()).or_default().push((text, kind, count, first));
    }
    let mut terms: Vec<(Term, usize)> = by_key
        .into_values()
        .map(|mut v| {
            let total: u32 = v.iter().map(|e| e.2).sum();
            let first = v.iter().map(|e| e.3).min().unwrap_or(0);
            v.sort_by(|a, b| {
                b.2.cmp(&a.2)
                    .then_with(|| all_caps(&a.0).cmp(&all_caps(&b.0)))
                    .then_with(|| a.3.cmp(&b.3))
            });
            let (text, kind, _, _) = v.swap_remove(0);
            (Term { text, kind, count: total }, first)
        })
        .collect();
    terms.sort_by(|a, b| b.0.count.cmp(&a.0.count).then_with(|| a.1.cmp(&b.1)));
    terms.truncate(MAX_TERMS);
    terms.into_iter().map(|(t, _)| t).collect()
}

fn all_caps(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests' "spell checker": the everyday list and a few more words.
    fn words(w: &str) -> bool {
        crate::learn::is_frequent(w)
            || [
                "getting", "started", "warning", "projects", "settings", "inbox", "draft", "reply", "forward", "archive",
                "meeting", "notes", "launch", "plan", "review", "dashboard", "welcome", "new", "sale", "explorer",
                "terminal", "search", "hopper", "grace", "kindle",
            ]
            .contains(&w)
    }

    fn texts(text: &str) -> Vec<String> {
        extract(text, &words, true).into_iter().map(|t| t.text).collect()
    }

    fn has(text: &str, term: &str) -> bool {
        texts(text).iter().any(|t| t == term)
    }

    #[test]
    fn names_and_terms_are_found() {
        let screen = "Meeting with Siobhan Ní Bhriain about Zentrix.\nRochelle asked whether Kubernetes \
                      can run GPT-OSS on the M365 tenant. Ping @zentrix_dev or jane.doe@example.com. \
                      We moved to PostgreSQL and the iPhone app ships with Qwen3 and GPT-4o.";
        for t in [
            "Siobhan Ní Bhriain",
            "Siobhan",
            "Bhriain",
            "Zentrix",
            "Rochelle",
            "Kubernetes",
            "GPT-OSS",
            "M365",
            "@zentrix_dev",
            "jane.doe@example.com",
            "PostgreSQL",
            "iPhone",
            "Qwen3",
            "GPT-4o",
        ] {
            assert!(has(screen, t), "{t} in {:?}", texts(screen));
        }
    }

    #[test]
    fn everyday_words_are_never_terms_however_written() {
        let screen = "Getting Started\nWARNING: The meeting moved to Monday. Thanks, and Welcome!\n\
                      NEW SALE. Projects | Settings | Inbox · Draft · Reply · Forward\n\
                      It is 5pm, the 3rd of May, 10kg and 100ms, 2GB at 60fps. OK? Yes. I am here.";
        assert_eq!(texts(screen), Vec::<String>::new());
    }

    #[test]
    fn paths_urls_and_code_are_not_terms() {
        let screen = "See https://example.com/Zentrix/docs and C:\\Users\\Jo\\Zentrix.txt, \
                      the README.md and node.js, my_variable and foo(bar) e.g. this.";
        assert_eq!(texts(screen), Vec::<String>::new());
    }

    #[test]
    fn code_on_screen_gives_its_names_not_its_identifiers() {
        let editor = "const value12 = useState('Zentrix');\nsetOwner(\"Rochelle\"); item2.save(); // Siobhan's review";
        let found = texts(editor);
        for t in ["Zentrix", "Rochelle", "Siobhan", "useState", "setOwner"] {
            assert!(found.contains(&t.to_string()), "{t} in {found:?}");
        }
        assert!(!found.iter().any(|t| t.contains("value") || t.contains("item")), "{found:?}");
        // An address in angle brackets.
        assert!(has("From: Jane Doe <jane.doe@example.com>", "jane.doe@example.com"));
    }

    #[test]
    fn full_names_run_through_particles_but_not_across_punctuation_or_words() {
        assert!(has("Symphony by Ludwig van Beethoven tonight", "Ludwig van Beethoven"));
        // A comma ends a name; an everyday word between two names isn't one.
        let list = "Rochelle, Siobhan and Priyanka";
        assert!(!texts(list).iter().any(|t| t.contains(' ')), "{:?}", texts(list));
        // A name ending with a particle is cut back to the names.
        assert!(has("Thanks Siobhan de", "Siobhan"));
        assert!(!texts("Thanks Siobhan de").iter().any(|t| t.contains(' ')));
        // An everyday capitalised word next to a name isn't part of it.
        assert!(!has("Reply Rochelle", "Reply Rochelle"));
        assert!(has("Reply Rochelle", "Rochelle"));
    }

    #[test]
    fn possessives_and_quotes_come_off() {
        assert_eq!(texts("Siobhan's notes (Zentrix) \"Rochelle\""), ["Siobhan", "Zentrix", "Rochelle"]);
        assert!(has("O'Brien and Jean-Luc", "O'Brien"));
        assert!(has("O'Brien and Jean-Luc", "Jean-Luc"));
    }

    #[test]
    fn the_most_frequent_spelling_and_term_come_first() {
        let screen = "ZENTRIX launch\nZentrix plan. Zentrix review. Rochelle. Zentrix dashboard. Kubernetes. Kubernetes.";
        let found = extract(screen, &words, true);
        // "ZENTRIX" in a heading and "Zentrix" three times: one term, the
        // spelling it mostly has.
        assert_eq!(found[0].text, "Zentrix");
        assert_eq!(found[0].count, 4);
        assert_eq!(found[1].text, "Kubernetes");
        assert_eq!(found[2].text, "Rochelle");
    }

    #[test]
    fn short_acronyms_and_measures_are_left_out() {
        assert_eq!(texts("OK, the US and UK TV, a 4K 3D screen at 120Hz, v2 x86"), Vec::<String>::new());
        assert!(has("Call NASA about the API", "NASA"));
        assert!(has("Call NASA about the API", "API"));
    }

    #[test]
    fn without_a_spell_checker_only_strong_signals_count() {
        // A capitalised word alone could be any word the everyday list
        // misses ("Kindle", "Hopper"): not a term then. Capitals inside,
        // acronyms, digits and full names still are.
        let found: Vec<String> = extract(
            "Zentrix and GitHub, NASA, Qwen3, with Rochelle Okonkwo-Bassey",
            &|w| crate::learn::is_frequent(w),
            false,
        )
        .into_iter()
        .map(|t| t.text)
        .collect();
        assert!(!found.contains(&"Zentrix".to_string()), "{found:?}");
        for t in ["GitHub", "NASA", "Qwen3", "Rochelle Okonkwo-Bassey"] {
            assert!(found.contains(&t.to_string()), "{t} in {found:?}");
        }
    }

    #[test]
    fn a_long_screen_stays_bounded() {
        let mut screen = String::new();
        for i in 0..2_000 {
            screen.push_str(&format!("Zentrix{i}x Rochelle "));
        }
        let found = extract(&screen, &words, true);
        assert_eq!(found.len(), MAX_TERMS);
        assert_eq!(found[0].text, "Rochelle");
    }
}
