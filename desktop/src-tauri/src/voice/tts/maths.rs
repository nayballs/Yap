//! **Maths said in words** (16 Nov 2026): an answer's formulas — `$…$`,
//! `\(…\)`, `$$…$$`, `\[…\]`, found exactly as Chat and the reading card find
//! them (`core/markdown.js` `mathAt`, so a price, "$5 and $10", stays money)
//! — written out as a person says them: "$\pi r^2$" → "pi r squared",
//! "$\frac{a}{b}$" → "a over b", "$\sum_{i=1}^{n} x_i$" → "the sum from i
//! equals one to n of x sub i". Every voice gets the words
//! (`normalize::for_speech` runs this first); the reading card still draws
//! the formula, lit as one while its words are said (`marks::align_maths`).
//!
//! Why our own: the one maintained engine for this, MathCAT (Rust, NVDA's
//! and Orca's maths speech, ClearSpeak and MathSpeak rules), reads MathML,
//! not TeX — a second crate to convert — and loads its rule files (YAML, a
//! folder per language) from disk at run time, made for a screen reader
//! stepping through a formula ("end fraction", "end root"). A voice reply
//! wants the short spoken form of the formulas a chat answer has. So: a
//! small TeX reader (the commands models write, Unicode maths too) and
//! ClearSpeak's default wording where it has one — "x squared", "x cubed",
//! "x to the fourth", "the square root of x", "the cube root of", simple
//! fractions "a over b", common ones "three quarters", "the sum from … to
//! … of", "the integral from … to … of", "the absolute value of" — with a
//! comma's pause where ClearSpeak would say "end fraction" or "end
//! exponent". Pure, and unit-tested on many formulas.
//!
//! **The person's region** (Settings → About you → Region, [`sync`]) where
//! the words differ: British (and everywhere else) "minus three", "three
//! quarters", "a plus b, all squared", "d y by d x", "one hundred and five";
//! American and Canadian "negative three", "three fourths", "the quantity a
//! plus b, squared", "d y d x", "one hundred five".
//!
//! **Anything it doesn't know is said simply**: an unknown symbol by its
//! name (`\aleph` → "aleph"), an unknown command with braces round its
//! words by those words; never a backslash, a brace, `^` or `_`.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::normalize::{cardinal, digits, ordinal};

/// The longest formula read as maths (a longer one stays as it is), as
/// `core/markdown.js` `MATH_MAX`.
const MATH_MAX: usize = 4000;

/// Where the words for maths differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variety {
    /// The UK's, and anywhere that isn't North America.
    British,
    /// The United States' and Canada's.
    American,
}

static AMERICAN: AtomicBool = AtomicBool::new(false);

/// The region Settings has now (About you → Region): its English, for the
/// words that differ. Called at start and on every save.
pub fn sync(cfg: &crate::config::YapConfig) {
    use crate::ai::about_you::Spelling;
    let american = matches!(crate::ai::about_you::profile(cfg).region.spelling, Some(Spelling::American | Spelling::Canadian));
    AMERICAN.store(american, Ordering::Relaxed);
}

/// The words maths is said in now ([`sync`]).
pub fn variety() -> Variety {
    if AMERICAN.load(Ordering::Relaxed) {
        Variety::American
    } else {
        Variety::British
    }
}

// ---- finding formulas (as core/markdown.js mathAt) ----------------------------------------

/// A formula in a text: `range` its bytes, delimiters included; `tex` what's
/// between them; `display` on lines of its own (`$$…$$`, `\[…\]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub tex: Range<usize>,
    pub display: bool,
}

fn is_space(c: Option<char>) -> bool {
    c.is_none_or(char::is_whitespace)
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// The formula starting at byte `pos` of `src`, if one does. `$…$` only as
/// Pandoc reads it — not right after a letter or digit, a non-space after
/// the opening `$`, a non-space before the closing one and no letter, digit
/// or `$` after it — so "$5 and $10", "US$5" and "$HOME/$USER" aren't maths.
/// A `\` escapes the next character. Pure.
pub fn at(src: &str, pos: usize) -> Option<Found> {
    let rest = src.get(pos..)?;
    let (open, close, display) = if rest.starts_with("$$") {
        ("$$", "$$", true)
    } else if rest.starts_with('$') {
        ("$", "$", false)
    } else if rest.starts_with("\\(") {
        ("\\(", "\\)", false)
    } else if rest.starts_with("\\[") {
        ("\\[", "\\]", true)
    } else {
        return None;
    };
    let before = src[..pos].chars().next_back();
    if open == "$" && (is_word(before) || before == Some('\\') || is_space(rest[1..].chars().next())) {
        return None;
    }
    let from = pos + open.len();
    let mut i = from;
    while i < src.len() {
        let tail = &src[i..];
        if tail.starts_with(close) {
            if open == "$" {
                let after = tail[1..].chars().next();
                if after == Some('$') || is_space(src[..i].chars().next_back()) || is_word(after) {
                    return None;
                }
            }
            let tex = &src[from..i];
            if tex.trim().is_empty() || tex.encode_utf16().count() > MATH_MAX {
                return None;
            }
            return Some(Found { range: pos..i + close.len(), tex: from..i, display });
        }
        let mut chars = tail.chars();
        let c = chars.next().map_or(1, char::len_utf8);
        i += c;
        // `\$` (and any other escaped character) never closes it.
        if tail.starts_with('\\') {
            i += chars.next().map_or(0, char::len_utf8);
        }
    }
    None
}

/// Every formula in `text`, left to right, as the reading card finds them
/// (`bar/readingPieces.js`). Pure.
pub fn find(text: &str) -> Vec<Found> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        if c == '$' || c == '\\' {
            if let Some(f) = at(text, i) {
                i = f.range.end;
                out.push(f);
                continue;
            }
        }
        i += c.len_utf8();
    }
    out
}

/// `text` with every formula in it written out in words. Pure.
pub fn in_words(text: &str, v: Variety) -> String {
    in_words_mapped(text, v).0
}

/// Where a formula went: `(shown, said)`, each in **characters** — the
/// formula in the text (its delimiters included), and its words in the text
/// with every formula written out.
pub type Placed = (Range<usize>, Range<usize>);

/// [`in_words`], and where each formula went ([`Placed`]). Pure.
pub fn in_words_mapped(text: &str, v: Variety) -> (String, Vec<Placed>) {
    let found = find(text);
    if found.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(text.len());
    let mut map = Vec::with_capacity(found.len());
    let (mut last, mut shown_chars, mut out_chars) = (0usize, 0usize, 0usize);
    for f in found {
        let between = &text[last..f.range.start];
        out.push_str(between);
        let n = between.chars().count();
        shown_chars += n;
        out_chars += n;
        let mut words = say(&text[f.tex.clone()], v);
        // Display maths with words after it: a breath between.
        if f.display && !words.is_empty() && text[f.range.end..].trim_start().starts_with(char::is_alphanumeric) {
            words.push(',');
        }
        let shown_len = text[f.range.clone()].chars().count();
        let said_len = words.chars().count();
        out.push_str(&words);
        map.push((shown_chars..shown_chars + shown_len, out_chars..out_chars + said_len));
        shown_chars += shown_len;
        out_chars += said_len;
        last = f.range.end;
    }
    out.push_str(&text[last..]);
    (out, map)
}

// ---- maths written as plain text ----------------------------------------------------------

/// Characters that make a run of plain text maths ("πd²/4", "≈ 78.5").
/// Not μ or Ω (units: "5 μm", "10 Ω"), not arrows (prose: "Friday → Monday").
fn plain_trigger(c: char) -> bool {
    vulgar(c).is_some()
        || matches!(c, 'π' | 'θ' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹' | '⁰' | '¹' | 'ⁿ' | '×' | '÷' | '±' | '∓' | '≈' | '≤' | '≥' | '≠' | '√' | '∛' | '∞' | '∑' | '∫' | '∂' | '∝' | '≡')
}

/// A fraction written as one character ("⅓"): its numerator and denominator.
fn vulgar(c: char) -> Option<(u8, u8)> {
    Some(match c {
        '½' => (1, 2),
        '⅓' => (1, 3),
        '⅔' => (2, 3),
        '¼' => (1, 4),
        '¾' => (3, 4),
        '⅕' => (1, 5),
        '⅖' => (2, 5),
        '⅗' => (3, 5),
        '⅘' => (4, 5),
        '⅙' => (1, 6),
        '⅚' => (5, 6),
        '⅐' => (1, 7),
        '⅛' => (1, 8),
        '⅜' => (3, 8),
        '⅝' => (5, 8),
        '⅞' => (7, 8),
        '⅑' => (1, 9),
        '⅒' => (1, 10),
        _ => return None,
    })
}

/// `\frac{n}{d}`'s tokens.
fn frac_tokens(n: &str, d: &str) -> [Tok; 7] {
    [Tok::Cmd("frac".into()), Tok::Open, Tok::Num(n.into()), Tok::Close, Tok::Open, Tok::Num(d.into()), Tok::Close]
}

/// A token's edges: (leading punctuation, the token, trailing punctuation).
fn edges(token: &str) -> (&str, &str, &str) {
    let lead = token.len() - token.trim_start_matches(['(', '[', '"', '“', '\'']).len();
    let rest = &token[lead..];
    let core = rest.trim_end_matches([',', '.', ';', ':', '!', '?', ')', ']', '"', '”', '\'']);
    (&token[..lead], core, &rest[core.len()..])
}

/// An operand of plain-text maths: a number, a lone letter, or a term of
/// them with maths characters ("25π", "πd²/4", "5²") — never a word.
fn plain_operand(core: &str) -> bool {
    if core.is_empty() || core.chars().count() > 16 {
        return false;
    }
    // Letters side by side: one ("x", "2x"), or up to three with a maths
    // character beside them ("mc²", "πr²", "sin²θ").
    let most = if core.chars().any(plain_trigger) { 3 } else { 1 };
    let mut letters = 0;
    for c in core.chars() {
        if c.is_ascii_alphabetic() {
            letters += 1;
            if letters > most {
                return false;
            }
            continue;
        }
        letters = 0;
        if !(c.is_ascii_digit() || matches!(c, '.' | ',' | '/' | '^' | '(' | ')' | '+' | '-' | '−') || plain_trigger(c)) {
            return false;
        }
    }
    // Not just punctuation ("-", "/").
    core.chars().any(|c| c.is_ascii_alphanumeric() || plain_trigger(c))
}

/// An operator or relation standing alone between operands ("=", "×", "≈").
fn plain_operator(core: &str) -> bool {
    matches!(core, "=" | "≈" | "≤" | "≥" | "≠" | "<" | ">" | "±" | "×" | "÷" | "+" | "-" | "−" | "/" | "*")
}

/// **Maths written as plain text** (16 Nov 2026), outside any `$…$`: models
/// often write "A = πd²/4" or "π × 5² = 25π ≈ 78.54" as Unicode. A run of
/// operands and operators ([`plain_operand`], [`plain_operator`]) is said
/// through the same converter when it holds a maths character (π, ², ≈, ×,
/// √…) or an "=" between two operands: "A equals pi d squared over four".
/// Conservative: a word ends a run, a comma or full stop after a token ends
/// it there, and "3 × 4" alone (a size, `normalize.rs`: "three by four") is
/// left as it is. Pure.
pub fn plain_in_words(text: &str, v: Variety) -> String {
    if !text.chars().any(|c| plain_trigger(c) || c == '=') {
        return text.to_string();
    }
    // Tokens with their byte offsets.
    let mut toks: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                toks.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        toks.push((s, &text[s..]));
    }
    let kind = |t: &str| {
        let (_, core, _) = edges(t);
        (plain_operand(core), plain_operator(core))
    };
    let ends_clause = |t: &str| !edges(t).2.is_empty();
    let opens = |t: &str| !edges(t).0.is_empty();
    let mut out = String::with_capacity(text.len() + 32);
    let mut last = 0;
    let mut i = 0;
    while i < toks.len() {
        let (operand, operator) = kind(toks[i].1);
        let (_, core, _) = edges(toks[i].1);
        // A maths character, or "=" between two operands ("r = d/2"); never
        // "-" or "/" alone ("10 - 15" is a range, "and/or" words).
        let trigger = (operand && core.chars().any(plain_trigger))
            || (operator
                && core == "="
                && i > 0
                && i + 1 < toks.len()
                && kind(toks[i - 1].1).0
                && !ends_clause(toks[i - 1].1)
                && kind(toks[i + 1].1).0
                && !opens(toks[i + 1].1));
        if !trigger {
            i += 1;
            continue;
        }
        // The run round it. A lone "a", "A" or "I" is a word ("a 3 × 4 grid")
        // unless an operator is beside it ("a = 5", "x + a").
        let article = |t: &str| matches!(edges(t).1, "a" | "A" | "I");
        let mut a = i;
        while a > 0 && a > last_token(&toks, last) {
            let (o, op) = kind(toks[a - 1].1);
            if !(o || op) || ends_clause(toks[a - 1].1) || opens(toks[a].1) || (article(toks[a - 1].1) && !kind(toks[a].1).1) {
                break;
            }
            a -= 1;
        }
        let mut b = i;
        while b + 1 < toks.len() && !ends_clause(toks[b].1) {
            let (o, op) = kind(toks[b + 1].1);
            if !(o || op) || opens(toks[b + 1].1) || (article(toks[b + 1].1) && !kind(toks[b].1).1) {
                break;
            }
            b += 1;
        }
        // Never starting or ending on an operator.
        while a < b && kind(toks[a].1).1 {
            a += 1;
        }
        while b > a && kind(toks[b].1).1 {
            b -= 1;
        }
        let run: Vec<&str> = toks[a..=b].iter().map(|t| t.1).collect();
        let (lead, _, _) = edges(run[0]);
        let (_, _, trail) = edges(run[run.len() - 1]);
        let from = toks[a].0 + lead.len();
        let to = toks[b].0 + toks[b].1.len() - trail.len();
        let body = &text[from..to];
        // "3 × 4" alone is a size.
        let size = run.len() == 3 && edges(run[1]).1 == "×" && [run[0], run[2]].iter().all(|t| edges(t).1.chars().all(|c| c.is_ascii_digit()));
        let words = if size || from >= to { String::new() } else { say(body, v) };
        if !words.is_empty() {
            out.push_str(&text[last..from]);
            out.push_str(&words);
            last = to;
        }
        i = b + 1;
    }
    out.push_str(&text[last..]);
    out
}

/// The index of the first token at or after byte `at` (a run never reaches
/// back over text already said).
fn last_token(toks: &[(usize, &str)], at: usize) -> usize {
    toks.iter().position(|t| t.0 >= at).unwrap_or(toks.len())
}

// ---- reading TeX ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// `\name`, or `\` and one other character (`\,` → ",").
    Cmd(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    Prime,
    /// Digits, maybe with a decimal point ("3.14", ".5", "1,000").
    Num(String),
    /// A run of ASCII letters written together ("Area", "dx", "sin").
    Letters(String),
    /// `\text{…}` and its kin: the words as written.
    Text(String),
    Ch(char),
}

/// Commands whose braces hold words, not maths.
const TEXT_COMMANDS: &[&str] = &[
    "text", "textrm", "textbf", "textit", "textsf", "texttt", "textup", "textnormal", "textmd", "emph", "mbox", "hbox",
    "operatorname", "mathrm",
];

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        'ⁿ' => 'n',
        'ⁱ' => 'i',
        '⁺' => '+',
        '⁻' => '-',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '₀' => '0',
        '₁' => '1',
        '₂' => '2',
        '₃' => '3',
        '₄' => '4',
        '₅' => '5',
        '₆' => '6',
        '₇' => '7',
        '₈' => '8',
        '₉' => '9',
        'ₙ' => 'n',
        'ᵢ' => 'i',
        'ⱼ' => 'j',
        'ₖ' => 'k',
        _ => return None,
    })
}

/// A braced group's raw text at `chars[i]` (`{` there): (its text, the index
/// after its `}`).
fn raw_group(chars: &[char], i: usize) -> (String, usize) {
    let mut depth = 0;
    let mut j = i;
    let mut s = String::new();
    while j < chars.len() {
        let c = chars[j];
        if c == '\\' && j + 1 < chars.len() {
            if depth > 0 {
                s.push(c);
                s.push(chars[j + 1]);
            }
            j += 2;
            continue;
        }
        if c == '{' {
            depth += 1;
            if depth > 1 {
                s.push(c);
            }
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return (s, j + 1);
            }
            s.push(c);
        } else if depth > 0 {
            s.push(c);
        }
        j += 1;
    }
    (s, j)
}

/// Text as written in `\text{…}`: its escapes undone, inline maths in it said.
fn plain_text(s: &str, v: Variety) -> String {
    let s = s.replace("\\ ", " ").replace("\\%", "%").replace("\\$", "$").replace("\\&", "&").replace("\\_", "_").replace("\\#", "#");
    let s = if s.contains('$') { in_words(&s, v) } else { s };
    s.replace(['{', '}', '\\', '~'], " ")
}

fn tokens(tex: &str, v: Variety) -> Vec<Tok> {
    let chars: Vec<char> = tex.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j == i + 1 {
                    // `\,`, `\{`, `\\`…
                    if let Some(&n) = chars.get(i + 1) {
                        out.push(Tok::Cmd(n.to_string()));
                    }
                    i += 2;
                    continue;
                }
                let mut name: String = chars[i + 1..j].iter().collect();
                if chars.get(j) == Some(&'*') && matches!(name.as_str(), "operatorname" | "text") {
                    j += 1;
                }
                if name == "operatorname" || TEXT_COMMANDS.contains(&name.as_str()) {
                    let mut k = j;
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    if chars.get(k) == Some(&'{') {
                        let (raw, end) = raw_group(&chars, k);
                        // `\mathrm{d}`, `\mathrm{e}`: maths in an upright face.
                        if name == "mathrm" && raw.chars().all(|c| c.is_ascii_alphabetic()) && raw.chars().count() <= 2 {
                            out.extend(tokens(&raw, v));
                        } else if name == "operatorname" {
                            let name: String = raw.chars().filter(char::is_ascii_alphabetic).collect();
                            if !name.is_empty() {
                                out.push(Tok::Letters(name));
                            }
                        } else {
                            out.push(Tok::Text(plain_text(&raw, v)));
                        }
                        i = end;
                        continue;
                    }
                    name = String::new();
                }
                if !name.is_empty() {
                    out.push(Tok::Cmd(name));
                }
                i = j;
            }
            '{' => {
                out.push(Tok::Open);
                i += 1;
            }
            '}' => {
                out.push(Tok::Close);
                i += 1;
            }
            '^' => {
                out.push(Tok::Sup);
                i += 1;
            }
            '_' => {
                out.push(Tok::Sub);
                i += 1;
            }
            '&' => {
                out.push(Tok::Amp);
                i += 1;
            }
            '\'' | '′' => {
                out.push(Tok::Prime);
                i += 1;
            }
            '″' => {
                out.extend([Tok::Prime, Tok::Prime]);
                i += 1;
            }
            '~' => i += 1,
            _ if c.is_whitespace() => i += 1,
            // Two characters written together as one relation: "<=", "!=",
            // ":=", "->", "=>" (never across a space: `n! = 1` is n factorial).
            _ if matches!((c, chars.get(i + 1).copied()), ('<' | '>' | '!' | ':' | '=', Some('=')) | ('-' | '=', Some('>'))) => {
                let name = match (c, chars[i + 1]) {
                    ('<', _) => "le",
                    ('>', _) => "ge",
                    ('!', _) => "neq",
                    (':', _) => "coloneqq",
                    ('=', '=') => "eq",
                    ('-', _) => "to",
                    _ => "implies",
                };
                out.push(Tok::Cmd(name.to_string()));
                i += 2;
            }
            _ if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) => {
                let mut s = String::new();
                let mut j = i;
                let mut point = false;
                loop {
                    match chars.get(j) {
                        Some(d) if d.is_ascii_digit() => {
                            s.push(*d);
                            j += 1;
                        }
                        Some('.') if !point && chars.get(j + 1).is_some_and(char::is_ascii_digit) => {
                            point = true;
                            s.push('.');
                            j += 1;
                        }
                        // "1{,}000" and "1,000": a thousands separator, three digits after it.
                        Some(',') | Some('{')
                            if !point && {
                                let k = if chars[j] == '{' { j + 3 } else { j + 1 };
                                let sep_ok = chars[j] == ',' || (chars.get(j + 1) == Some(&',') && chars.get(j + 2) == Some(&'}'));
                                sep_ok
                                    && chars.get(k..k + 3).is_some_and(|d| d.iter().all(char::is_ascii_digit))
                                    && !chars.get(k + 3).is_some_and(char::is_ascii_digit)
                                    && s.split(',').next_back().is_some_and(|g| (1..=3).contains(&g.len()))
                            } =>
                        {
                            j = if chars[j] == '{' { j + 3 } else { j + 1 };
                            s.push(',');
                        }
                        _ => break,
                    }
                }
                // "4/3" written together: a fraction ("four thirds").
                let over = chars.get(j) == Some(&'/') && chars.get(j + 1).is_some_and(char::is_ascii_digit) && !point && !s.contains(',');
                if over {
                    let mut k = j + 1;
                    while chars.get(k).is_some_and(char::is_ascii_digit) {
                        k += 1;
                    }
                    if chars.get(k) != Some(&'.') {
                        let d: String = chars[j + 1..k].iter().collect();
                        out.extend(frac_tokens(&s, &d));
                        i = k;
                        continue;
                    }
                }
                out.push(Tok::Num(s));
                i = j;
            }
            _ if c.is_ascii_alphabetic() => {
                let mut j = i;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                out.push(Tok::Letters(chars[i..j].iter().collect()));
                i = j;
            }
            _ => {
                if let Some((n, d)) = vulgar(c) {
                    out.extend(frac_tokens(&n.to_string(), &d.to_string()));
                    i += 1;
                } else if let Some(s) = superscript(c) {
                    // Superscript digits run together: "¹⁰".
                    let mut run = String::from(s);
                    let mut j = i + 1;
                    while let Some(n) = chars.get(j).and_then(|&c| superscript(c)) {
                        run.push(n);
                        j += 1;
                    }
                    out.push(Tok::Sup);
                    out.push(Tok::Open);
                    out.extend(tokens(&run, v));
                    out.push(Tok::Close);
                    i = j;
                } else if let Some(s) = subscript(c) {
                    let mut run = String::from(s);
                    let mut j = i + 1;
                    while let Some(n) = chars.get(j).and_then(|&c| subscript(c)) {
                        run.push(n);
                        j += 1;
                    }
                    out.push(Tok::Sub);
                    out.push(Tok::Open);
                    out.extend(tokens(&run, v));
                    out.push(Tok::Close);
                    i = j;
                } else {
                    out.push(Tok::Ch(c));
                    i += 1;
                }
            }
        }
    }
    out
}

/// What an operator is to the words round it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Between two terms: "plus", "times".
    Bin,
    /// A relation: "equals", "is less than".
    Rel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Big {
    Sum,
    Product,
    Integral(u8),
    Contour,
    Limit,
    Max,
    Min,
    Sup,
    Inf,
    Union,
    Intersection,
    ArgMax,
    ArgMin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Paren,
    Abs,
    Norm,
    Floor,
    Ceiling,
    Set,
    Angle,
    /// `\left. … \right|_{x=0}`: "evaluated at".
    At,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Env {
    Cases,
    Matrix { determinant: bool },
    Lines,
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// Nothing (a script with no base: `^2` alone).
    Empty,
    Num(String),
    /// A letter or symbol, as it's said ("x", "pi", "capital gamma").
    Ident(String),
    /// Words (`\text{…}`, a word written in maths).
    Word(String),
    Op(String, Class),
    /// "-": minus, or negative/minus before a term.
    Minus,
    /// "±".
    PlusMinus,
    /// `|`, `\mid`: "given", or "such that" in a set.
    Mid,
    /// ":" — a ratio between numbers, "such that" in a set.
    Colon,
    Punct(char),
    Factorial,
    Dots,
    Pause,
    Func(String),
    Big(Big),
    Frac(Vec<Atom>, Vec<Atom>),
    Binom(Vec<Atom>, Vec<Atom>),
    Root(Option<Vec<Atom>>, Vec<Atom>),
    Group(Vec<Atom>),
    Fenced(Fence, Vec<Atom>),
    /// An accent: its word, said after ("x hat") or before ("vector v").
    Accent(&'static str, bool, Vec<Atom>),
    Rows(Env, Vec<Vec<Vec<Atom>>>),
}

#[derive(Debug, Clone, PartialEq)]
struct Atom {
    kind: Kind,
    sub: Option<Vec<Atom>>,
    sup: Option<Vec<Atom>>,
    primes: u8,
}

impl Atom {
    fn new(kind: Kind) -> Atom {
        Atom { kind, sub: None, sup: None, primes: 0 }
    }
    fn op(words: &str, class: Class) -> Atom {
        Atom::new(Kind::Op(words.to_string(), class))
    }
    fn ident(words: &str) -> Atom {
        Atom::new(Kind::Ident(words.to_string()))
    }
    fn plain(&self) -> bool {
        self.sub.is_none() && self.sup.is_none() && self.primes == 0
    }
}

const GREEK: &[(&str, &str)] = &[
    ("alpha", "alpha"),
    ("beta", "beta"),
    ("gamma", "gamma"),
    ("delta", "delta"),
    ("epsilon", "epsilon"),
    ("varepsilon", "epsilon"),
    ("zeta", "zeta"),
    ("eta", "eta"),
    ("theta", "theta"),
    ("vartheta", "theta"),
    ("iota", "iota"),
    ("kappa", "kappa"),
    ("varkappa", "kappa"),
    ("lambda", "lambda"),
    ("mu", "mu"),
    ("nu", "nu"),
    ("xi", "xi"),
    ("omicron", "omicron"),
    ("pi", "pi"),
    ("varpi", "pi"),
    ("rho", "rho"),
    ("varrho", "rho"),
    ("sigma", "sigma"),
    ("varsigma", "sigma"),
    ("tau", "tau"),
    ("upsilon", "upsilon"),
    ("phi", "phi"),
    ("varphi", "phi"),
    ("chi", "chi"),
    ("psi", "psi"),
    ("omega", "omega"),
    ("Gamma", "capital gamma"),
    ("Delta", "capital delta"),
    ("Theta", "capital theta"),
    ("Lambda", "capital lambda"),
    ("Xi", "capital xi"),
    ("Pi", "capital pi"),
    ("Sigma", "capital sigma"),
    ("Upsilon", "capital upsilon"),
    ("Phi", "capital phi"),
    ("Psi", "capital psi"),
    ("Omega", "capital omega"),
    ("varGamma", "capital gamma"),
    ("varDelta", "capital delta"),
    ("varOmega", "capital omega"),
];

fn greek_char(c: char) -> Option<&'static str> {
    Some(match c {
        'α' => "alpha",
        'β' => "beta",
        'γ' => "gamma",
        'δ' => "delta",
        'ε' | 'ϵ' => "epsilon",
        'ζ' => "zeta",
        'η' => "eta",
        'θ' | 'ϑ' => "theta",
        'ι' => "iota",
        'κ' => "kappa",
        'λ' => "lambda",
        'μ' | 'µ' => "mu",
        'ν' => "nu",
        'ξ' => "xi",
        'ο' => "omicron",
        'π' => "pi",
        'ρ' => "rho",
        'σ' | 'ς' => "sigma",
        'τ' => "tau",
        'υ' => "upsilon",
        'φ' | 'ϕ' => "phi",
        'χ' => "chi",
        'ψ' => "psi",
        'ω' => "omega",
        'Γ' => "capital gamma",
        'Δ' => "capital delta",
        'Θ' => "capital theta",
        'Λ' => "capital lambda",
        'Ξ' => "capital xi",
        'Π' => "capital pi",
        'Σ' => "capital sigma",
        'Φ' => "capital phi",
        'Ψ' => "capital psi",
        'Ω' => "capital omega",
        _ => return None,
    })
}

/// Symbols said as one thing (letters' kin): `\infty`, `\hbar`…
const SYMBOLS: &[(&str, &str)] = &[
    ("infty", "infinity"),
    ("partial", "partial"),
    ("nabla", "del"),
    ("hbar", "h bar"),
    ("ell", "l"),
    ("emptyset", "the empty set"),
    ("varnothing", "the empty set"),
    ("forall", "for all"),
    ("exists", "there exists"),
    ("nexists", "there is no"),
    ("neg", "not"),
    ("lnot", "not"),
    ("angle", "angle"),
    ("triangle", "triangle"),
    ("degree", "degrees"),
    ("prime", "prime"),
    ("aleph", "aleph"),
    ("Re", "the real part of"),
    ("Im", "the imaginary part of"),
    ("top", "transpose"),
    ("dagger", "dagger"),
    ("ast", "star"),
    ("star", "star"),
    ("checkmark", "tick"),
];

/// Operators: their words and class.
const OPS: &[(&str, &str, Class)] = &[
    ("times", "times", Class::Bin),
    ("cdot", "times", Class::Bin),
    ("div", "divided by", Class::Bin),
    ("mp", "minus or plus", Class::Bin),
    ("cup", "union", Class::Bin),
    ("cap", "intersection", Class::Bin),
    ("setminus", "minus", Class::Bin),
    ("wedge", "and", Class::Bin),
    ("land", "and", Class::Bin),
    ("vee", "or", Class::Bin),
    ("lor", "or", Class::Bin),
    ("oplus", "plus", Class::Bin),
    ("otimes", "tensor", Class::Bin),
    ("circ", "composed with", Class::Bin),
    ("bmod", "mod", Class::Bin),
    ("mod", "mod", Class::Bin),
    ("eq", "equals", Class::Rel),
    ("ne", "is not equal to", Class::Rel),
    ("neq", "is not equal to", Class::Rel),
    ("lt", "is less than", Class::Rel),
    ("gt", "is greater than", Class::Rel),
    ("le", "is less than or equal to", Class::Rel),
    ("leq", "is less than or equal to", Class::Rel),
    ("leqslant", "is less than or equal to", Class::Rel),
    ("ge", "is greater than or equal to", Class::Rel),
    ("geq", "is greater than or equal to", Class::Rel),
    ("geqslant", "is greater than or equal to", Class::Rel),
    ("ll", "is much less than", Class::Rel),
    ("gg", "is much greater than", Class::Rel),
    ("approx", "is approximately", Class::Rel),
    ("simeq", "is approximately", Class::Rel),
    ("cong", "is congruent to", Class::Rel),
    ("equiv", "is equivalent to", Class::Rel),
    ("sim", "is similar to", Class::Rel),
    ("propto", "is proportional to", Class::Rel),
    ("coloneqq", "is defined as", Class::Rel),
    ("triangleq", "is defined as", Class::Rel),
    ("in", "is in", Class::Rel),
    ("notin", "is not in", Class::Rel),
    ("ni", "contains", Class::Rel),
    ("subset", "is a subset of", Class::Rel),
    ("subseteq", "is a subset of", Class::Rel),
    ("supset", "is a superset of", Class::Rel),
    ("supseteq", "is a superset of", Class::Rel),
    ("perp", "is perpendicular to", Class::Rel),
    ("parallel", "is parallel to", Class::Rel),
    ("to", "to", Class::Rel),
    ("rightarrow", "to", Class::Rel),
    ("longrightarrow", "to", Class::Rel),
    ("mapsto", "maps to", Class::Rel),
    ("gets", "gets", Class::Rel),
    ("leftarrow", "gets", Class::Rel),
    ("Rightarrow", "implies", Class::Rel),
    ("implies", "implies", Class::Rel),
    ("Longrightarrow", "implies", Class::Rel),
    ("Leftarrow", "is implied by", Class::Rel),
    ("iff", "if and only if", Class::Rel),
    ("Leftrightarrow", "if and only if", Class::Rel),
    ("Longleftrightarrow", "if and only if", Class::Rel),
    ("leftrightarrow", "if and only if", Class::Rel),
];

fn op_char(c: char) -> Option<(&'static str, Class)> {
    Some(match c {
        '+' => ("plus", Class::Bin),
        '×' | '*' | '·' | '⋅' | '∗' => ("times", Class::Bin),
        '÷' => ("divided by", Class::Bin),
        '/' => ("over", Class::Bin),
        '∓' => ("minus or plus", Class::Bin),
        '∪' => ("union", Class::Bin),
        '∩' => ("intersection", Class::Bin),
        '∘' => ("composed with", Class::Bin),
        '=' => ("equals", Class::Rel),
        '<' => ("is less than", Class::Rel),
        '>' => ("is greater than", Class::Rel),
        '≤' | '⩽' => ("is less than or equal to", Class::Rel),
        '≥' | '⩾' => ("is greater than or equal to", Class::Rel),
        '≠' => ("is not equal to", Class::Rel),
        '≈' | '≃' => ("is approximately", Class::Rel),
        '≅' => ("is congruent to", Class::Rel),
        '≡' => ("is equivalent to", Class::Rel),
        '∼' => ("is similar to", Class::Rel),
        '∝' => ("is proportional to", Class::Rel),
        '∈' => ("is in", Class::Rel),
        '∉' => ("is not in", Class::Rel),
        '⊂' | '⊆' => ("is a subset of", Class::Rel),
        '⊃' | '⊇' => ("is a superset of", Class::Rel),
        '⊥' => ("is perpendicular to", Class::Rel),
        '∥' => ("is parallel to", Class::Rel),
        '→' | '⟶' => ("to", Class::Rel),
        '↦' => ("maps to", Class::Rel),
        '⇒' | '⟹' => ("implies", Class::Rel),
        '⇔' | '⟺' | '↔' => ("if and only if", Class::Rel),
        _ => return None,
    })
}

fn symbol_char(c: char) -> Option<&'static str> {
    Some(match c {
        '∞' => "infinity",
        '∂' => "partial",
        '∇' => "del",
        '∅' => "the empty set",
        '∀' => "for all",
        '∃' => "there exists",
        '¬' => "not",
        '∠' => "angle",
        '°' => "degrees",
        'ℏ' => "h bar",
        'ℓ' => "l",
        'ℝ' => "the real numbers",
        'ℕ' => "the natural numbers",
        'ℤ' => "the integers",
        'ℚ' => "the rational numbers",
        'ℂ' => "the complex numbers",
        '%' => "percent",
        _ => return None,
    })
}

/// Named functions: their spoken name.
const FUNCS: &[(&str, &str)] = &[
    ("sin", "sine"),
    ("cos", "cosine"),
    ("tan", "tangent"),
    ("sec", "secant"),
    ("csc", "cosecant"),
    ("cosec", "cosecant"),
    ("cot", "cotangent"),
    ("arcsin", "arc sine"),
    ("arccos", "arc cosine"),
    ("arctan", "arc tangent"),
    ("sinh", "hyperbolic sine"),
    ("cosh", "hyperbolic cosine"),
    ("tanh", "hyperbolic tangent"),
    ("log", "log"),
    ("lg", "log"),
    ("ln", "natural log"),
    ("exp", "the exponential of"),
    ("det", "the determinant of"),
    ("dim", "the dimension of"),
    ("ker", "the kernel of"),
    ("deg", "the degree of"),
    ("gcd", "the greatest common divisor of"),
    ("lcm", "the lowest common multiple of"),
    ("arg", "the argument of"),
    ("Pr", "the probability of"),
    ("Var", "the variance of"),
    ("Cov", "the covariance of"),
    ("sgn", "the sign of"),
    ("tr", "the trace of"),
    ("rank", "the rank of"),
];

fn big_of(name: &str) -> Option<Big> {
    Some(match name {
        "sum" => Big::Sum,
        "prod" => Big::Product,
        "int" => Big::Integral(1),
        "iint" => Big::Integral(2),
        "iiint" => Big::Integral(3),
        "oint" => Big::Contour,
        "lim" => Big::Limit,
        "max" => Big::Max,
        "min" => Big::Min,
        "sup" => Big::Sup,
        "inf" => Big::Inf,
        "bigcup" => Big::Union,
        "bigcap" => Big::Intersection,
        "argmax" => Big::ArgMax,
        "argmin" => Big::ArgMin,
        _ => return None,
    })
}

fn big_char(c: char) -> Option<Big> {
    Some(match c {
        '∑' => Big::Sum,
        '∏' => Big::Product,
        '∫' => Big::Integral(1),
        '∬' => Big::Integral(2),
        '∭' => Big::Integral(3),
        '∮' => Big::Contour,
        '⋃' => Big::Union,
        '⋂' => Big::Intersection,
        _ => return None,
    })
}

/// Commands that change nothing said: sizes, styles, spacing.
const IGNORED: &[&str] = &[
    "displaystyle", "textstyle", "scriptstyle", "scriptscriptstyle", "limits", "nolimits", "big", "Big", "bigg", "Bigg", "bigl",
    "bigr", "Bigl", "Bigr", "biggl", "biggr", "Biggl", "Biggr", "bigm", "Bigm", "middle", ",", ";", ":", "!", " ", "enspace",
    "thinspace", "medspace", "thickspace", "negthinspace", "nonumber", "notag", "hline", "left", "right", "allowbreak",
    "quad", "qquad",
];

/// Commands whose first braced group is skipped (a colour, a label, a space).
const SKIP_ONE: &[&str] = &["hspace", "vspace", "color", "label", "tag", "phantom", "hphantom", "vphantom", "kern", "mkern", "mspace"];

/// Commands that just draw their group differently: said as the group.
const FONTS: &[&str] = &[
    "mathbf", "mathit", "mathsf", "mathtt", "mathcal", "mathscr", "mathfrak", "boldsymbol", "bm", "pmb", "mathnormal", "boxed",
    "cancel", "bcancel", "xcancel", "underline", "mathop", "mathrel", "mathbin", "mathord", "mathopen", "mathclose", "smash",
    "bold",
];

/// Accents: the word, and whether it goes first ("vector v").
fn accent(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "hat" | "widehat" => ("hat", false),
        "bar" | "overline" => ("bar", false),
        "tilde" | "widetilde" => ("tilde", false),
        "dot" => ("dot", false),
        "ddot" => ("double dot", false),
        "vec" | "overrightarrow" => ("vector", true),
        _ => return None,
    })
}

struct Parser {
    toks: Vec<Tok>,
    i: usize,
}

/// Where a list stops.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// The end of the formula.
    End,
    /// A `}`.
    Close,
    /// `\right`.
    Right,
    /// A closing bracket, `)` or `]`.
    Bracket,
    /// A `|` (an absolute value's end).
    Bar,
    /// `\|` or `\rVert`.
    DoubleBar,
    /// A named closer (`\rfloor`, `\}`…).
    Cmd(&'static str),
    /// A cell's end: `&`, `\\`, `\end`.
    Cell,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn at_stop(&self, stop: Stop) -> bool {
        let Some(t) = self.peek() else { return true };
        match (stop, t) {
            (Stop::Right, Tok::Cmd(c)) => c == "right",
            (Stop::DoubleBar, Tok::Cmd(c)) => c == "|" || c == "rVert" || c == "Vert",
            (Stop::Cmd(name), Tok::Cmd(c)) => c == name,
            (Stop::Cell, Tok::Cmd(c)) => c == "\\" || c == "end" || c == "cr",
            // A stray `}` ends any list inside a group.
            (s, t) => matches!(
                (s, t),
                (Stop::Close, Tok::Close)
                    | (Stop::Bracket, Tok::Ch(')' | ']'))
                    | (Stop::Bar, Tok::Ch('|'))
                    | (Stop::DoubleBar, Tok::Ch('‖'))
                    | (Stop::Cell, Tok::Amp)
                    | (Stop::Bracket | Stop::Bar | Stop::DoubleBar | Stop::Cmd(_) | Stop::Right, Tok::Close)
            ),
        }
    }

    /// Atoms until `stop` (not consumed).
    fn list(&mut self, stop: Stop) -> Vec<Atom> {
        let mut out: Vec<Atom> = Vec::new();
        let mut guard = 0;
        while !self.at_stop(stop) {
            guard += 1;
            if guard > 20_000 {
                break;
            }
            let Some(t) = self.peek().cloned() else { break };
            match t {
                Tok::Close => {
                    // A stray `}` at the top: nothing.
                    self.i += 1;
                }
                Tok::Sup | Tok::Sub => {
                    self.i += 1;
                    let arg = self.arg();
                    if out.is_empty() {
                        out.push(Atom::new(Kind::Empty));
                    }
                    let last = out.last_mut().expect("an atom");
                    let slot = if t == Tok::Sup { &mut last.sup } else { &mut last.sub };
                    match slot {
                        // `x^a^b`: TeX refuses it; the second joins the first.
                        Some(have) => have.extend(arg),
                        None => *slot = Some(arg),
                    }
                }
                Tok::Prime => {
                    self.i += 1;
                    match out.last_mut() {
                        Some(a) => a.primes = a.primes.saturating_add(1),
                        None => out.push(Atom::ident("prime")),
                    }
                }
                Tok::Cmd(ref c) if matches!(c.as_str(), "over" | "choose" | "atop") => {
                    self.i += 1;
                    let top = std::mem::take(&mut out);
                    let bottom = self.list(stop);
                    let kind = if c == "choose" { Kind::Binom(top, bottom) } else { Kind::Frac(top, bottom) };
                    return vec![Atom::new(kind)];
                }
                _ => {
                    let before = self.i;
                    if let Some(atoms) = self.atom() {
                        out.extend(atoms);
                    }
                    if self.i == before {
                        self.i += 1;
                    }
                }
            }
        }
        out
    }

    /// One argument: a group, or the next single thing (`\frac12`, `x^\alpha`).
    fn arg(&mut self) -> Vec<Atom> {
        match self.peek().cloned() {
            None => Vec::new(),
            Some(Tok::Open) => {
                self.i += 1;
                let inner = self.list(Stop::Close);
                if self.peek() == Some(&Tok::Close) {
                    self.i += 1;
                }
                inner
            }
            // One digit only: `x^23` is x squared, then 3.
            Some(Tok::Num(n)) if n.chars().count() > 1 => {
                let mut chars = n.chars();
                let first = chars.next().map(String::from).unwrap_or_default();
                self.toks[self.i] = Tok::Num(chars.as_str().trim_start_matches(',').to_string());
                vec![Atom::new(Kind::Num(first))]
            }
            // One letter only: `x^ab`.
            Some(Tok::Letters(l)) if l.chars().count() > 1 => {
                let mut chars = l.chars();
                let first = chars.next().map(String::from).unwrap_or_default();
                self.toks[self.i] = Tok::Letters(chars.as_str().to_string());
                vec![Atom::ident(&first)]
            }
            Some(_) => {
                let before = self.i;
                let got = self.atom().unwrap_or_default();
                if self.i == before {
                    self.i += 1;
                }
                got
            }
        }
    }

    /// A delimiter after `\left` / `\right`, or met by itself.
    fn delimiter(&mut self) -> Option<Fence> {
        let t = self.peek().cloned()?;
        let f = match t {
            Tok::Ch('(' | '[' | ')' | ']') => Fence::Paren,
            Tok::Ch('|') => Fence::Abs,
            Tok::Ch('‖') => Fence::Norm,
            Tok::Ch('.') => {
                self.i += 1;
                return None;
            }
            Tok::Ch('⌊' | '⌋') => Fence::Floor,
            Tok::Ch('⌈' | '⌉') => Fence::Ceiling,
            Tok::Ch('⟨' | '⟩' | '<' | '>') => Fence::Angle,
            Tok::Cmd(c) => match c.as_str() {
                "{" | "}" | "lbrace" | "rbrace" => Fence::Set,
                "|" | "Vert" | "lVert" | "rVert" => Fence::Norm,
                "vert" | "lvert" | "rvert" => Fence::Abs,
                "lfloor" | "rfloor" => Fence::Floor,
                "lceil" | "rceil" => Fence::Ceiling,
                "langle" | "rangle" => Fence::Angle,
                "lbrack" | "rbrack" | "lparen" | "rparen" => Fence::Paren,
                _ => return None,
            },
            _ => return None,
        };
        self.i += 1;
        Some(f)
    }

    /// `\left( … \right)`.
    fn left_right(&mut self) -> Atom {
        let open = self.delimiter();
        let inner = self.list(Stop::Right);
        if matches!(self.peek(), Some(Tok::Cmd(c)) if c == "right") {
            self.i += 1;
            let close = self.delimiter();
            let fence = match (open, close) {
                // `\left\{ \begin{array}… \right.`: cases written by hand.
                (Some(Fence::Set), None) => {
                    let inner = inner
                        .into_iter()
                        .map(|a| match a.kind {
                            Kind::Rows(Env::Lines, rows) => Atom { kind: Kind::Rows(Env::Cases, rows), ..a },
                            _ => a,
                        })
                        .collect();
                    return Atom::new(Kind::Group(inner));
                }
                // `\left. \frac{df}{dx} \right|_{x=0}`: evaluated there.
                (None, Some(Fence::Abs)) => Fence::At,
                (Some(f), _) | (None, Some(f)) => f,
                (None, None) => return Atom::new(Kind::Group(inner)),
            };
            return Atom::new(Kind::Fenced(fence, inner));
        }
        match open {
            Some(f) => Atom::new(Kind::Fenced(f, inner)),
            None => Atom::new(Kind::Group(inner)),
        }
    }

    /// A fence opened by itself: its content to `stop`, which is consumed.
    fn fenced(&mut self, fence: Fence, stop: Stop) -> Atom {
        let inner = self.list(stop);
        let closes = !matches!((stop, self.peek()), (Stop::Close, _) | (_, Some(Tok::Close) | None));
        if closes {
            self.i += 1;
        }
        Atom::new(Kind::Fenced(fence, inner))
    }

    /// Whether a `|` at the parser's place has a partner before the end of
    /// its group: then it's an absolute value.
    fn bar_has_partner(&self) -> bool {
        let mut depth = 0i32;
        for t in &self.toks[self.i + 1..] {
            match t {
                Tok::Open => depth += 1,
                Tok::Close => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                Tok::Ch('(' | '[') => depth += 1,
                Tok::Ch(')' | ']') => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                Tok::Ch('|') if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// `\begin{…} … \end{…}`.
    fn environment(&mut self) -> Atom {
        let name = self.group_name();
        let env = match name.trim_end_matches('*') {
            "cases" | "dcases" | "rcases" => Env::Cases,
            "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "smallmatrix" | "Vmatrix" => Env::Matrix { determinant: false },
            "vmatrix" => Env::Matrix { determinant: true },
            _ => Env::Lines,
        };
        // `array`'s and `alignat`'s column spec.
        if matches!(name.as_str(), "array" | "alignat" | "alignat*") && self.peek() == Some(&Tok::Open) {
            self.group_name();
        }
        let mut rows: Vec<Vec<Vec<Atom>>> = Vec::new();
        let mut row: Vec<Vec<Atom>> = Vec::new();
        loop {
            let cell = self.list(Stop::Cell);
            row.push(cell);
            match self.peek().cloned() {
                Some(Tok::Amp) => {
                    self.i += 1;
                }
                Some(Tok::Cmd(c)) if c == "\\" || c == "cr" => {
                    self.i += 1;
                    // `\\[4pt]`: a row's spacing.
                    if self.peek() == Some(&Tok::Ch('[')) {
                        while let Some(t) = self.peek().cloned() {
                            self.i += 1;
                            if t == Tok::Ch(']') {
                                break;
                            }
                        }
                    }
                    rows.push(std::mem::take(&mut row));
                }
                Some(Tok::Cmd(c)) if c == "end" => {
                    self.i += 1;
                    self.group_name();
                    break;
                }
                _ => break,
            }
        }
        if row.iter().any(|c| !c.is_empty()) {
            rows.push(row);
        }
        rows.retain(|r| r.iter().any(|c| !c.is_empty()));
        Atom::new(Kind::Rows(env, rows))
    }

    /// A `{name}` group's letters (an environment's name, a spec): consumed.
    fn group_name(&mut self) -> String {
        if self.peek() != Some(&Tok::Open) {
            return String::new();
        }
        self.i += 1;
        let mut s = String::new();
        let mut depth = 0;
        while let Some(t) = self.peek().cloned() {
            self.i += 1;
            match t {
                Tok::Open => depth += 1,
                Tok::Close if depth == 0 => break,
                Tok::Close => depth -= 1,
                Tok::Letters(l) => s.push_str(&l),
                Tok::Ch(c) => s.push(c),
                Tok::Text(t) => s.push_str(&t),
                _ => {}
            }
        }
        s
    }

    /// The next thing, as atoms (none: nothing to say).
    fn atom(&mut self) -> Option<Vec<Atom>> {
        let t = self.peek()?.clone();
        self.i += 1;
        let one = |k: Kind| Some(vec![Atom::new(k)]);
        match t {
            Tok::Num(n) => one(Kind::Num(n)),
            Tok::Text(s) => {
                let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
                if s.is_empty() {
                    None
                } else {
                    one(Kind::Word(s))
                }
            }
            Tok::Letters(l) => Some(letters(&l)),
            Tok::Open => {
                let inner = self.list(Stop::Close);
                if self.peek() == Some(&Tok::Close) {
                    self.i += 1;
                }
                one(Kind::Group(inner))
            }
            Tok::Close | Tok::Sup | Tok::Sub | Tok::Prime => None,
            Tok::Amp => None,
            Tok::Ch(c) => self.char_atom(c),
            Tok::Cmd(name) => self.command(&name),
        }
    }

    fn char_atom(&mut self, c: char) -> Option<Vec<Atom>> {
        let one = |k: Kind| Some(vec![Atom::new(k)]);
        match c {
            '-' | '−' | '–' => one(Kind::Minus),
            '±' => one(Kind::PlusMinus),
            '(' | '[' => Some(vec![self.fenced(Fence::Paren, Stop::Bracket)]),
            ')' | ']' => None,
            '|' => {
                if self.bar_has_partner() {
                    Some(vec![self.fenced(Fence::Abs, Stop::Bar)])
                } else {
                    one(Kind::Mid)
                }
            }
            '‖' => Some(vec![self.fenced(Fence::Norm, Stop::DoubleBar)]),
            '⌊' => Some(vec![self.fenced_to_char(Fence::Floor, '⌋')]),
            '⌈' => Some(vec![self.fenced_to_char(Fence::Ceiling, '⌉')]),
            '⟨' => Some(vec![self.fenced_to_char(Fence::Angle, '⟩')]),
            ':' => one(Kind::Colon),
            ',' | ';' => one(Kind::Punct(c)),
            '!' => one(Kind::Factorial),
            '.' | '?' | '"' | '`' | '@' | '#' | '$' => None,
            '…' | '⋯' | '⋮' => one(Kind::Dots),
            '√' => {
                let arg = self.arg();
                one(Kind::Root(None, arg))
            }
            '∛' => {
                let arg = self.arg();
                one(Kind::Root(Some(vec![Atom::new(Kind::Num("3".into()))]), arg))
            }
            _ => {
                if let Some(b) = big_char(c) {
                    return one(Kind::Big(b));
                }
                if let Some((w, class)) = op_char(c) {
                    return Some(vec![Atom::op(w, class)]);
                }
                if let Some(w) = greek_char(c) {
                    return Some(vec![Atom::ident(w)]);
                }
                if let Some(w) = symbol_char(c) {
                    return Some(vec![Atom::ident(w)]);
                }
                if c.is_alphanumeric() {
                    return Some(vec![Atom::ident(&c.to_string())]);
                }
                None
            }
        }
    }

    /// A fence to a closing character (`⌊ … ⌋`), which is consumed.
    fn fenced_to_char(&mut self, fence: Fence, close: char) -> Atom {
        let start = self.i;
        let end = self.toks[start..].iter().position(|t| *t == Tok::Ch(close)).map(|k| start + k);
        let Some(end) = end else { return self.fenced(fence, Stop::End) };
        let mut inner_parser = Parser { toks: self.toks[start..end].to_vec(), i: 0 };
        let inner = inner_parser.list(Stop::End);
        self.i = end + 1;
        Atom::new(Kind::Fenced(fence, inner))
    }

    fn command(&mut self, name: &str) -> Option<Vec<Atom>> {
        let one = |k: Kind| Some(vec![Atom::new(k)]);
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let top = self.arg();
                let bottom = self.arg();
                return one(Kind::Frac(top, bottom));
            }
            "binom" | "dbinom" | "tbinom" => {
                let top = self.arg();
                let bottom = self.arg();
                return one(Kind::Binom(top, bottom));
            }
            "sqrt" => {
                let index = if self.peek() == Some(&Tok::Ch('[')) {
                    self.i += 1;
                    let inner = self.list(Stop::Bracket);
                    if self.peek().is_some_and(|t| matches!(t, Tok::Ch(']' | ')'))) {
                        self.i += 1;
                    }
                    Some(inner)
                } else {
                    None
                };
                let arg = self.arg();
                return one(Kind::Root(index, arg));
            }
            "left" => return Some(vec![self.left_right()]),
            "right" => {
                self.delimiter();
                return None;
            }
            "begin" => return Some(vec![self.environment()]),
            "end" => {
                self.group_name();
                return None;
            }
            "{" | "lbrace" => return Some(vec![self.fenced(Fence::Set, Stop::Cmd("}"))]),
            "}" | "rbrace" => return None,
            "lvert" => return Some(vec![self.fenced(Fence::Abs, Stop::Cmd("rvert"))]),
            "lVert" => return Some(vec![self.fenced(Fence::Norm, Stop::Cmd("rVert"))]),
            "|" | "Vert" => return Some(vec![self.fenced(Fence::Norm, Stop::DoubleBar)]),
            "lfloor" => return Some(vec![self.fenced(Fence::Floor, Stop::Cmd("rfloor"))]),
            "lceil" => return Some(vec![self.fenced(Fence::Ceiling, Stop::Cmd("rceil"))]),
            "langle" => return Some(vec![self.fenced(Fence::Angle, Stop::Cmd("rangle"))]),
            "rvert" | "rVert" | "rfloor" | "rceil" | "rangle" => return None,
            "mid" | "vert" => return one(Kind::Mid),
            "pm" => return one(Kind::PlusMinus),
            "ldots" | "cdots" | "dots" | "dotsc" | "dotsb" | "dotso" | "dotsm" | "vdots" | "ddots" => return one(Kind::Dots),
            "quad" | "qquad" | "\\" | "cr" => return one(Kind::Pause),
            "%" => return Some(vec![Atom::ident("percent")]),
            "$" => return Some(vec![Atom::ident("dollars")]),
            "&" => return Some(vec![Atom::new(Kind::Word("and".into()))]),
            "#" | "_" => return None,
            "not" => {
                let next = self.atom().unwrap_or_default();
                return Some(next.into_iter().map(negate).collect());
            }
            "mathbb" | "Bbb" | "mathbbm" => {
                let arg = self.arg();
                let letter = match arg.as_slice() {
                    [a] => match &a.kind {
                        Kind::Ident(l) => Some(l.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let word = match letter.as_deref() {
                    Some("R") => "the real numbers",
                    Some("N") => "the natural numbers",
                    Some("Z") => "the integers",
                    Some("Q") => "the rational numbers",
                    Some("C") => "the complex numbers",
                    _ => return Some(arg),
                };
                return Some(vec![Atom::ident(word)]);
            }
            "textcolor" => {
                self.group_name();
                let arg = self.arg();
                return one(Kind::Group(arg));
            }
            // `\arg\max_x`: one operator.
            "arg" if matches!(self.peek(), Some(Tok::Cmd(c)) if c == "max" || c == "min") => {
                let max = matches!(self.peek(), Some(Tok::Cmd(c)) if c == "max");
                self.i += 1;
                return one(Kind::Big(if max { Big::ArgMax } else { Big::ArgMin }));
            }
            // A brace's label is a note: its content is what's said.
            "underbrace" | "overbrace" => {
                let arg = self.arg();
                if matches!(self.peek(), Some(Tok::Sup | Tok::Sub)) {
                    self.i += 1;
                    self.arg();
                }
                return one(Kind::Group(arg));
            }
            // The thing above or below is a note; the base is what's said —
            // under an operator, its limit (`\underset{x}{\arg\max}`).
            "overset" | "underset" | "stackrel" => {
                let note = self.arg();
                let mut base = self.arg();
                if name == "underset" {
                    if let [b @ Atom { kind: Kind::Big(_), .. }] = base.as_mut_slice() {
                        b.sub = Some(note);
                    }
                }
                return Some(base);
            }
            _ => {}
        }
        if let Some((_, w)) = GREEK.iter().find(|(n, _)| *n == name) {
            return Some(vec![Atom::ident(w)]);
        }
        if let Some((_, w)) = SYMBOLS.iter().find(|(n, _)| *n == name) {
            return Some(vec![Atom::ident(w)]);
        }
        if let Some((_, w, class)) = OPS.iter().find(|(n, _, _)| *n == name) {
            return Some(vec![Atom::op(w, *class)]);
        }
        if let Some((_, w)) = FUNCS.iter().find(|(n, _)| *n == name) {
            return one(Kind::Func((*w).to_string()));
        }
        if let Some(b) = big_of(name) {
            return one(Kind::Big(b));
        }
        if let Some((word, before)) = accent(name) {
            let arg = self.arg();
            return one(Kind::Accent(word, before, arg));
        }
        if name == "pmod" {
            let arg = self.arg();
            let mut out = vec![Atom::op("mod", Class::Bin)];
            out.extend(arg);
            return Some(out);
        }
        if IGNORED.contains(&name) {
            return None;
        }
        if SKIP_ONE.contains(&name) {
            self.group_name();
            return None;
        }
        if FONTS.contains(&name) {
            let arg = self.arg();
            return one(Kind::Group(arg));
        }
        // Not known: a symbol is said by its name ("aleph"); a command round
        // braces by what's in them.
        if self.peek() == Some(&Tok::Open) || name.chars().count() < 2 || !name.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        Some(vec![Atom::new(Kind::Word(name.to_lowercase()))])
    }
}

/// `\not` before a relation.
fn negate(a: Atom) -> Atom {
    let Kind::Op(w, Class::Rel) = &a.kind else { return a };
    let words = if w == "equals" {
        "is not equal to".to_string()
    } else if let Some(rest) = w.strip_prefix("is ") {
        format!("is not {rest}")
    } else {
        format!("not {w}")
    };
    Atom { kind: Kind::Op(words, Class::Rel), ..a }
}

/// A run of letters written together: a function's name ("sin"), a limit's
/// ("lim"), a word (four letters or more: "Area"), else a letter each.
fn letters(run: &str) -> Vec<Atom> {
    if let Some((_, w)) = FUNCS.iter().find(|(n, _)| *n == run) {
        return vec![Atom::new(Kind::Func((*w).to_string()))];
    }
    if let Some(b) = big_of(run) {
        return vec![Atom::new(Kind::Big(b))];
    }
    if run == "mod" {
        return vec![Atom::op("mod", Class::Bin)];
    }
    if run.chars().count() >= 4 {
        return vec![Atom::new(Kind::Word(run.to_string()))];
    }
    run.chars().map(|c| Atom::ident(&c.to_string())).collect()
}

fn parse(tex: &str, v: Variety) -> Vec<Atom> {
    let mut p = Parser { toks: tokens(tex, v), i: 0 };
    let mut out = Vec::new();
    while p.i < p.toks.len() {
        let before = p.i;
        out.extend(p.list(Stop::End));
        if p.i == before {
            p.i += 1;
        }
    }
    out
}

// ---- saying it ----------------------------------------------------------------------------

/// A formula's TeX (what's between its delimiters) in words: "\pi r^2" →
/// "pi r squared". Never a backslash, a brace, `^` or `_`. Pure.
pub fn say(tex: &str, v: Variety) -> String {
    let atoms = parse(tex, v);
    let mut s = Say { v, out: Vec::new() };
    s.list(&atoms, Ctx::default());
    tidy(&s.out)
}

/// The words, with the pauses (",", ";") against the word before, none at
/// either end and never two together.
fn tidy(parts: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if p == "," || p == ";" {
            match out.last_mut() {
                None => {}
                Some(last) if last.ends_with(',') || last.ends_with(';') => {
                    if p == ";" {
                        last.pop();
                        last.push(';');
                    }
                }
                Some(last) => last.push_str(p),
            }
            continue;
        }
        out.push(p.to_string());
    }
    let mut s = out.join(" ");
    while s.ends_with(',') || s.ends_with(';') {
        s.pop();
    }
    // Nothing left that a voice would read as a symbol.
    s.chars().map(|c| if matches!(c, '\\' | '{' | '}' | '^' | '_' | '$' | '&' | '#') { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Copy, Default)]
struct Ctx {
    /// Inside a big operator's limits or a script: "in", not "is in".
    limits: bool,
    /// Inside a limit's subscript: "approaches".
    approach: bool,
    /// Inside a set: `|` and `:` are "such that".
    set: bool,
    /// Inside an integral's body: "d x" after a pause.
    integral: bool,
}

struct Say {
    v: Variety,
    out: Vec<String>,
}

/// Function-like letters: "f of x", not "f times x".
fn function_letter(a: &Atom) -> bool {
    match &a.kind {
        Kind::Ident(l) => {
            matches!(l.as_str(), "f" | "g" | "h" | "F" | "G" | "H" | "P" | "E" | "phi" | "psi" | "sigma" | "capital gamma") || a.primes > 0
        }
        _ => false,
    }
}

/// Said in a word or two, nothing to group: a number, a letter, a word.
fn simple(atoms: &[Atom]) -> bool {
    match atoms {
        [a] => match &a.kind {
            Kind::Num(_) | Kind::Ident(_) | Kind::Word(_) => a.sup.as_deref().is_none_or(simple) && a.sub.as_deref().is_none_or(simple),
            Kind::Group(g) => a.plain() && simple(g),
            Kind::Accent(_, _, g) => a.plain() && simple(g),
            _ => false,
        },
        _ => false,
    }
}

/// A whole number's value, if `atoms` is one.
fn integer(atoms: &[Atom]) -> Option<u64> {
    match atoms {
        [a] if a.plain() => match &a.kind {
            Kind::Num(n) if !n.contains('.') => n.replace(',', "").parse().ok(),
            Kind::Group(g) => integer(g),
            _ => None,
        },
        _ => None,
    }
}

fn ident_of(atoms: &[Atom]) -> Option<&str> {
    match atoms {
        [a] if a.plain() => match &a.kind {
            Kind::Ident(l) => Some(l.as_str()),
            Kind::Group(g) => ident_of(g),
            _ => None,
        },
        _ => None,
    }
}

/// A fence's content (nothing for anything else).
fn fenced_inner(a: &Atom) -> &[Atom] {
    match &a.kind {
        Kind::Fenced(_, inner) => inner,
        _ => &[],
    }
}

fn is_op(a: &Atom) -> bool {
    matches!(a.kind, Kind::Op(..) | Kind::Minus | Kind::PlusMinus | Kind::Mid | Kind::Colon | Kind::Punct(_) | Kind::Pause)
}

fn is_rel(a: &Atom) -> bool {
    matches!(&a.kind, Kind::Op(_, Class::Rel))
}

/// Whether "times" goes before `n`, a term after a function's argument
/// ("sine of x times cosine of x"); an integral's "d x" excepted.
fn multiplies(n: &Atom, ctx: Ctx) -> bool {
    if ctx.integral && matches!(&n.kind, Kind::Ident(d) if d == "d") {
        return false;
    }
    matches!(n.kind, Kind::Num(_) | Kind::Ident(_) | Kind::Group(_) | Kind::Fenced(..) | Kind::Frac(..) | Kind::Root(..) | Kind::Accent(..) | Kind::Func(_))
}

impl Say {
    fn w(&mut self, s: &str) {
        self.out.push(s.to_string());
    }

    fn pause(&mut self) {
        self.out.push(",".into());
    }

    fn minus(&self) -> &'static str {
        match self.v {
            Variety::British => "minus",
            Variety::American => "negative",
        }
    }

    fn number(&mut self, n: &str) {
        let words = number_words(n, self.v);
        self.w(&words);
    }

    fn list(&mut self, atoms: &[Atom], ctx: Ctx) {
        let mut i = 0;
        while i < atoms.len() {
            i = self.item(atoms, i, ctx);
        }
    }

    /// Says `atoms[i]` (and what belongs to it); the index after.
    fn item(&mut self, atoms: &[Atom], i: usize, ctx: Ctx) -> usize {
        let a = &atoms[i];
        let prev = i.checked_sub(1).map(|k| &atoms[k]);
        let next = atoms.get(i + 1);
        let more = i + 1 < atoms.len();
        match &a.kind {
            Kind::Big(kind) => return self.big(*kind, a, atoms, i, ctx),
            Kind::Func(name) => return self.func(name, a, atoms, i, ctx),
            Kind::Minus | Kind::PlusMinus => {
                // After an operator, an opening or a joining word ("or −1").
                let joining = |p: &Atom| matches!(&p.kind, Kind::Word(w) if ["if", "or", "and", "for", "when", "where", "otherwise", "else", "unless", "then"].contains(&w.to_lowercase().as_str()));
                let unary = prev.is_none_or(|p| is_op(p) || matches!(p.kind, Kind::Big(_)) || joining(p));
                let word = match (&a.kind, unary) {
                    (Kind::PlusMinus, _) => "plus or minus",
                    (_, true) => self.minus(),
                    _ => "minus",
                };
                self.w(word);
                return i + 1;
            }
            Kind::Op(w, _) => {
                let w = match (w.as_str(), ctx) {
                    ("to", Ctx { approach: true, .. }) => "approaches",
                    ("is in", Ctx { limits: true, .. }) => "in",
                    ("is not in", Ctx { limits: true, .. }) => "not in",
                    (w, _) => w,
                };
                let w = w.to_string();
                self.w(&w);
                return i + 1;
            }
            Kind::Mid => {
                self.w(if ctx.set { "such that" } else { "given" });
                return i + 1;
            }
            Kind::Colon => {
                let numbers = prev.is_some_and(|p| matches!(p.kind, Kind::Num(_))) && next.is_some_and(|n| matches!(n.kind, Kind::Num(_)));
                if ctx.set {
                    self.w("such that");
                } else if numbers {
                    self.w("to");
                } else {
                    self.pause();
                }
                return i + 1;
            }
            Kind::Punct(c) => {
                self.out.push(if *c == ';' { ";".into() } else { ",".into() });
                return i + 1;
            }
            Kind::Pause => {
                self.pause();
                return i + 1;
            }
            Kind::Factorial => {
                self.w("factorial");
                return i + 1;
            }
            Kind::Dots => {
                self.w("dot dot dot");
                return i + 1;
            }
            _ => {}
        }
        // A function letter, or a letter before one variable: "f of x" (a
        // bracket raised to a power is a term: "P times, one plus r, all
        // to the n").
        if let Some(fence @ Atom { kind: Kind::Fenced(Fence::Paren, inner), .. }) = next {
            let of = function_letter(a) || (matches!(a.kind, Kind::Ident(_)) && ident_of(inner).is_some() && inner.len() == 1);
            if of && matches!(a.kind, Kind::Ident(_)) && fence.sup.is_none() {
                self.term(a, ctx, true);
                self.w("of");
                self.fence_body(Fence::Paren, inner, ctx, false);
                self.scripts(fence, ctx, !simple(inner), i + 2 < atoms.len());
                if !simple(inner) && i + 2 < atoms.len() {
                    self.pause();
                }
                // "P of B given A, times P of A".
                if atoms.get(i + 2).is_some_and(|n| multiplies(n, ctx)) {
                    self.w("times");
                }
                return i + 2;
            }
        }
        // In an integral, "d x" after a pause.
        if ctx.integral && matches!(&a.kind, Kind::Ident(d) if d == "d") && next.is_some_and(|n| matches!(n.kind, Kind::Ident(_))) && i > 0 {
            self.pause();
        }
        self.term(a, ctx, more);
        // Two terms side by side where "times" helps: before or after a
        // bracket ("two times, x plus one", "a plus b, times c"), before a
        // root ("two pi times the square root of"), a fraction before a sum
        // ("one over N times the sum"). Never straight after "of" (`\frac{d}{dx}[x^n]`).
        let ordinary = |x: &Atom| matches!(x.kind, Kind::Num(_) | Kind::Ident(_) | Kind::Group(_) | Kind::Fenced(..) | Kind::Frac(..) | Kind::Root(..) | Kind::Accent(..) | Kind::Binom(..));
        let paren = |x: &Atom| matches!(&x.kind, Kind::Fenced(Fence::Paren, _));
        let after_of = self.out.last().is_some_and(|w| w == "of");
        let times = next.filter(|n| {
            !after_of
                && ordinary(a)
                && ((ordinary(n) && (paren(n) || (paren(a) && a.sup.is_none() && !simple(fenced_inner(a)))))
                    || matches!(n.kind, Kind::Root(..))
                    || (matches!(a.kind, Kind::Frac(..)) && matches!(n.kind, Kind::Big(_))))
        });
        if let Some(n) = times {
            self.w("times");
            if paren(n) && !simple(fenced_inner(n)) && n.sup.is_none() && self.v == Variety::American {
                self.w("the quantity");
            }
        }
        i + 1
    }

    /// One atom and its scripts.
    fn term(&mut self, a: &Atom, ctx: Ctx, more: bool) {
        let complex_base = match &a.kind {
            Kind::Num(n) => {
                self.number(n);
                false
            }
            Kind::Ident(l) => {
                let l = l.clone();
                self.w(&l);
                false
            }
            Kind::Word(t) => {
                let t = t.clone();
                self.w(&t);
                false
            }
            Kind::Empty => false,
            Kind::Group(g) => {
                self.list(g, ctx);
                g.len() > 1 && !simple(g)
            }
            Kind::Frac(top, bottom) => {
                self.frac(top, bottom, ctx, more || a.sup.is_some());
                true
            }
            Kind::Binom(n, k) => {
                self.list(n, ctx);
                self.w("choose");
                self.list(k, ctx);
                if more && !simple(k) {
                    self.pause();
                }
                true
            }
            Kind::Root(index, body) => {
                self.root(index.as_deref(), body, ctx);
                if (more || a.sup.is_some()) && !simple(body) {
                    self.pause();
                }
                false
            }
            Kind::Fenced(f, inner) => {
                let complex = !simple(inner);
                if *f == Fence::Paren && complex && a.sup.is_some() && self.v == Variety::American {
                    self.w("the quantity");
                }
                // A breath only after an operator ("two times, x plus one",
                // "a minus, b plus c"), never after "of" or "the quantity".
                let lead = self.out.last().is_some_and(|w| matches!(w.as_str(), "plus" | "minus" | "times" | "divided by" | "over" | "plus or minus" | "minus or plus"));
                self.fence_body(*f, inner, ctx, lead);
                if complex && more && a.sup.is_none() && a.sub.is_none() {
                    self.pause();
                }
                complex
            }
            Kind::Accent(word, before, g) => {
                if *before {
                    self.w(word);
                    self.list(g, ctx);
                } else {
                    self.list(g, ctx);
                    if !simple(g) {
                        self.pause();
                    }
                    self.w(word);
                }
                false
            }
            Kind::Rows(env, rows) => {
                self.rows(*env, rows, ctx);
                if more {
                    self.pause();
                }
                false
            }
            // Said by `item`.
            _ => false,
        };
        self.scripts(a, ctx, complex_base, more);
    }

    /// A subscript, primes and a superscript, after their base.
    fn scripts(&mut self, a: &Atom, ctx: Ctx, complex_base: bool, more: bool) {
        let inner = Ctx { limits: true, ..ctx };
        // `\left. … \right|_{0}^{1}`: "evaluated from zero to one".
        if matches!(a.kind, Kind::Fenced(Fence::At, _)) {
            if let Some(sub) = &a.sub {
                self.pause();
                self.w("evaluated");
                if let Some(sup) = &a.sup {
                    self.w("from");
                    self.list(sub, inner);
                    self.w("to");
                    self.list(sup, inner);
                } else {
                    self.w("at");
                    self.list(sub, inner);
                }
                if more {
                    self.pause();
                }
            }
            return;
        }
        if let Some(sub) = &a.sub {
            match sub.as_slice() {
                [Atom { kind: Kind::Word(t), .. }] => {
                    let t = t.clone();
                    self.w(&t);
                }
                _ => {
                    self.w("sub");
                    self.list(sub, inner);
                    if !simple(sub) && (more || a.sup.is_some()) {
                        self.pause();
                    }
                }
            }
        }
        match a.primes {
            0 => {}
            1 => self.w("prime"),
            2 => self.w("double prime"),
            _ => self.w("triple prime"),
        }
        let Some(sup) = &a.sup else { return };
        if complex_base && matches!(a.kind, Kind::Fenced(..) | Kind::Group(_)) {
            self.pause();
            if self.v == Variety::British {
                self.w("all");
            }
        }
        self.power(sup, a, inner, more);
    }

    /// "squared", "cubed", "to the fourth", "to the n", "to the power of …".
    fn power(&mut self, sup: &[Atom], base: &Atom, ctx: Ctx, more: bool) {
        if let Some(n) = integer(sup) {
            match n {
                2 => self.w("squared"),
                3 => self.w("cubed"),
                0..=100 => {
                    let o = ordinal_words(n, self.v);
                    self.w("to the");
                    self.w(&o);
                }
                _ => {
                    self.w("to the power of");
                    self.number(&n.to_string());
                }
            }
            return;
        }
        // "−1", "−x": "to the minus one".
        if let [Atom { kind: Kind::Minus, .. }, rest] = sup {
            if rest.plain() && matches!(rest.kind, Kind::Num(_) | Kind::Ident(_)) {
                if matches!(base.kind, Kind::Func(_)) {
                    return;
                }
                let m = self.minus();
                self.w("to the");
                self.w(m);
                self.term(rest, ctx, false);
                return;
            }
        }
        if let Some(l) = ident_of(sup) {
            let letter_base = matches!(&base.kind, Kind::Ident(b) if b.chars().count() == 1 && b != "e");
            match l {
                "T" if letter_base || matches!(base.kind, Kind::Fenced(..) | Kind::Group(_)) => self.w("transpose"),
                "transpose" | "dagger" | "star" | "prime" => {
                    let l = l.to_string();
                    self.w(&l);
                }
                "degrees" => self.w("degrees"),
                _ => {
                    self.w("to the");
                    let l = l.to_string();
                    self.w(&l);
                }
            }
            return;
        }
        match sup {
            [Atom { kind: Kind::Op(w, Class::Bin), .. }] if w == "plus" || w == "composed with" => {
                // `0^+`, `90^\circ`.
                self.w(if w == "plus" { "plus" } else { "degrees" });
                return;
            }
            [Atom { kind: Kind::Minus, .. }] => {
                self.w("minus");
                return;
            }
            _ => {}
        }
        self.w("to the power of");
        self.list(sup, ctx);
        if more {
            self.pause();
        }
    }

    fn frac(&mut self, top: &[Atom], bottom: &[Atom], ctx: Ctx, more: bool) {
        // "three quarters", "one half".
        if let (Some(n), Some(d)) = (integer(top), integer(bottom)) {
            if let Some(words) = common_fraction(n, d, self.v) {
                self.w(&words);
                return;
            }
        }
        // Leibniz: "d y by d x" (British), "d y d x" (American).
        let leading_d = |side: &[Atom]| {
            side.first().is_some_and(|a| matches!(&a.kind, Kind::Ident(d) | Kind::Word(d) if d == "d" || d == "partial" || d == "capital delta" || d == "delta"))
        };
        if leading_d(top) && leading_d(bottom) {
            self.list(top, ctx);
            if self.v == Variety::British {
                self.w("by");
            }
            self.list(bottom, ctx);
            // `\frac{d}{dx} x^2`: "of x squared".
            if top.len() == 1 && more {
                self.w("of");
            }
            return;
        }
        // "a over b"; a product over one term too ("pi d squared over
        // four": πd²/4 whichever way it's heard).
        let product = (2..=4).contains(&top.len()) && top.iter().all(|a| simple(std::slice::from_ref(a)));
        if (simple(top) || product) && simple(bottom) {
            self.list(top, ctx);
            self.w("over");
            self.list(bottom, ctx);
            return;
        }
        self.w("the fraction");
        self.list(top, ctx);
        if !simple(top) {
            self.pause();
        }
        self.w("over");
        self.list(bottom, ctx);
        if more && !simple(bottom) {
            self.pause();
        }
    }

    fn root(&mut self, index: Option<&[Atom]>, body: &[Atom], ctx: Ctx) {
        match index.and_then(integer).or(if index.is_none_or(<[Atom]>::is_empty) { Some(2) } else { None }) {
            Some(2) => self.w("the square root of"),
            Some(3) => self.w("the cube root of"),
            Some(n) => {
                let o = ordinal_words(n, self.v);
                self.w("the");
                self.w(&o);
                self.w("root of");
            }
            None => {
                let index = index.unwrap_or_default();
                self.w("the");
                match ident_of(index) {
                    Some("n") => self.w("nth"),
                    _ => {
                        self.list(index, Ctx { limits: true, ..ctx });
                        self.w("th");
                    }
                }
                self.w("root of");
            }
        }
        self.list(body, ctx);
    }

    /// What's inside a fence, said; `lead`: a breath before a bracket of
    /// more than one thing.
    fn fence_body(&mut self, f: Fence, inner: &[Atom], ctx: Ctx, lead: bool) {
        let complex = !simple(inner);
        match f {
            Fence::Paren | Fence::Angle | Fence::At => {
                if complex && lead {
                    self.pause();
                }
                self.list(inner, ctx);
            }
            Fence::Abs => {
                self.w("the absolute value of");
                self.list(inner, ctx);
            }
            Fence::Norm => {
                self.w("the norm of");
                self.list(inner, ctx);
            }
            Fence::Floor => {
                self.w("the floor of");
                self.list(inner, ctx);
            }
            Fence::Ceiling => {
                self.w("the ceiling of");
                self.list(inner, ctx);
            }
            Fence::Set => {
                let builder = inner.iter().any(|a| matches!(a.kind, Kind::Mid | Kind::Colon));
                self.w(if builder { "the set of" } else { "the set" });
                self.list(inner, Ctx { set: true, ..ctx });
            }
        }
    }

    /// A big operator, its limits, and what it's of: up to the next
    /// relation or comma.
    fn big(&mut self, kind: Big, a: &Atom, atoms: &[Atom], i: usize, ctx: Ctx) -> usize {
        let name = match kind {
            Big::Sum => "the sum",
            Big::Product => "the product",
            Big::Integral(1) => "the integral",
            Big::Integral(2) => "the double integral",
            Big::Integral(_) => "the triple integral",
            Big::Contour => "the contour integral",
            Big::Limit => "the limit",
            Big::Max => "the maximum",
            Big::Min => "the minimum",
            Big::Sup => "the supremum",
            Big::Inf => "the infimum",
            Big::Union => "the union",
            Big::Intersection => "the intersection",
            Big::ArgMax => "the arg max",
            Big::ArgMin => "the arg min",
        };
        self.w(name);
        let lim = Ctx { limits: true, approach: kind == Big::Limit, ..ctx };
        match (&a.sub, &a.sup) {
            (Some(lo), _) if kind == Big::Limit => {
                self.w("as");
                self.list(lo, lim);
            }
            (Some(lo), Some(hi)) => {
                self.w("from");
                self.list(lo, lim);
                self.w("to");
                self.list(hi, lim);
            }
            (Some(lo), None) => {
                self.w("over");
                self.list(lo, lim);
            }
            (None, Some(hi)) => {
                self.w("to");
                self.list(hi, lim);
            }
            (None, None) => {}
        }
        let end = atoms[i + 1..].iter().position(|x| is_rel(x) || matches!(x.kind, Kind::Punct(_) | Kind::Pause)).map_or(atoms.len(), |k| i + 1 + k);
        let body = &atoms[i + 1..end];
        if !body.is_empty() {
            self.w("of");
            let integral = matches!(kind, Big::Integral(_) | Big::Contour);
            self.list(body, Ctx { integral: integral || ctx.integral, ..ctx });
            // "… of k, equals …".
            if end < atoms.len() {
                self.pause();
            }
        }
        end
    }

    /// A named function: "sine of x", "log base 2 of x", "inverse sine of x".
    fn func(&mut self, name: &str, a: &Atom, atoms: &[Atom], i: usize, ctx: Ctx) -> usize {
        let inverse = a.sup.as_deref().is_some_and(|s| matches!(s, [Atom { kind: Kind::Minus, .. }, one] if matches!(&one.kind, Kind::Num(n) if n == "1")));
        if inverse {
            self.w("inverse");
        }
        let name = name.to_string();
        self.w(&name);
        if let Some(sub) = &a.sub {
            self.w("base");
            self.list(sub, Ctx { limits: true, ..ctx });
        }
        if a.primes > 0 {
            self.w("prime");
        }
        if let Some(sup) = a.sup.as_deref().filter(|_| !inverse) {
            self.power(sup, a, Ctx { limits: true, ..ctx }, true);
        }
        let takes_of = !name.ends_with(" of");
        let next = atoms.get(i + 1);
        if let Some(Atom { kind: Kind::Fenced(Fence::Paren, inner), .. }) = next {
            if takes_of {
                self.w("of");
            }
            self.list(inner, ctx);
            let fence = &atoms[i + 1];
            self.scripts(fence, ctx, !simple(inner), i + 2 < atoms.len());
            if !simple(inner) && i + 2 < atoms.len() {
                self.pause();
            }
            if atoms.get(i + 2).is_some_and(|n| multiplies(n, ctx)) {
                self.w("times");
            }
            return i + 2;
        }
        // A bare argument: the terms up to the next operator or function
        // (`\sin x \cos x`: "sine of x times cosine of x").
        let end = atoms[i + 1..].iter().position(|x| is_op(x) || matches!(x.kind, Kind::Big(_) | Kind::Func(_))).map_or(atoms.len(), |k| i + 1 + k);
        if end > i + 1 {
            if takes_of {
                self.w("of");
            }
            let arg = &atoms[i + 1..end];
            self.list(arg, ctx);
            if arg.len() > 1 && end < atoms.len() {
                self.pause();
            }
            if atoms.get(end).is_some_and(|n| matches!(n.kind, Kind::Func(_))) {
                self.w("times");
            }
        }
        end
    }

    fn rows(&mut self, env: Env, rows: &[Vec<Vec<Atom>>], ctx: Ctx) {
        if rows.is_empty() {
            return;
        }
        match env {
            Env::Cases => {
                for (k, row) in rows.iter().enumerate() {
                    if k > 0 {
                        self.out.push(";".into());
                    }
                    let mut cells = row.iter().filter(|c| !c.is_empty());
                    if let Some(value) = cells.next() {
                        self.list(value, ctx);
                    }
                    for cond in cells {
                        let has_word = matches!(cond.first(), Some(Atom { kind: Kind::Word(w), .. })
                            if ["if", "otherwise", "else", "when", "for", "where", "unless"].iter().any(|s| w.to_lowercase().starts_with(s)));
                        if !has_word {
                            self.w("if");
                        }
                        self.list(cond, ctx);
                    }
                }
            }
            Env::Matrix { determinant } => {
                let (r, c) = (rows.len(), rows.iter().map(Vec::len).max().unwrap_or(0));
                if determinant {
                    self.w("the determinant of");
                }
                let size = format!("{} by {}", number_words(&r.to_string(), self.v), number_words(&c.to_string(), self.v));
                if c == 1 && r > 1 {
                    self.w("the column vector");
                } else if r == 1 {
                    self.w("the row vector");
                } else {
                    self.w(&format!("the {size} matrix"));
                }
                if r * c > 16 {
                    return;
                }
                if r > 1 && c > 1 {
                    self.w("with rows");
                }
                for (k, row) in rows.iter().enumerate() {
                    if k > 0 {
                        self.out.push(if c == 1 { ",".into() } else { ";".into() });
                        if k + 1 == r && c > 1 {
                            self.w("and");
                        }
                    }
                    for (j, cell) in row.iter().enumerate() {
                        if j > 0 {
                            self.pause();
                        }
                        self.list(cell, ctx);
                    }
                }
            }
            Env::Lines => {
                for (k, row) in rows.iter().enumerate() {
                    if k > 0 {
                        self.out.push(";".into());
                    }
                    for cell in row {
                        self.list(cell, ctx);
                    }
                }
            }
        }
    }
}

/// A number as said: "12,400" → "twelve thousand four hundred", "3.14" →
/// "three point one four"; American English without the "and".
fn number_words(n: &str, v: Variety) -> String {
    let plain = n.replace(',', "");
    let words = if let Some((whole, frac)) = plain.split_once('.') {
        let whole = if whole.is_empty() { String::new() } else { whole_words(whole) };
        let point = format!("point {}", digits(frac));
        if whole.is_empty() {
            point
        } else {
            format!("{whole} {point}")
        }
    } else {
        whole_words(&plain)
    };
    american(words, v)
}

fn whole_words(s: &str) -> String {
    if (s.len() > 1 && s.starts_with('0')) || s.len() > 15 {
        return digits(s);
    }
    s.parse::<u64>().map(cardinal).unwrap_or_else(|_| digits(s))
}

fn american(words: String, v: Variety) -> String {
    match v {
        Variety::British => words,
        Variety::American => words.replace(" and ", " "),
    }
}

fn ordinal_words(n: u64, v: Variety) -> String {
    american(ordinal(n), v)
}

/// "one half", "three quarters" / "three fourths", "two thirds": a numerator
/// under 20 over 2 to 10.
fn common_fraction(n: u64, d: u64, v: Variety) -> Option<String> {
    if !(1..20).contains(&n) || !(2..=10).contains(&d) {
        return None;
    }
    let (one, many) = match (d, v) {
        (2, _) => ("half".to_string(), "halves".to_string()),
        (4, Variety::British) => ("quarter".to_string(), "quarters".to_string()),
        _ => {
            let o = ordinal(d);
            (o.clone(), format!("{o}s"))
        }
    };
    Some(format!("{} {}", cardinal(n), if n == 1 { one } else { many }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uk(tex: &str) -> String {
        say(tex, Variety::British)
    }

    fn us(tex: &str) -> String {
        say(tex, Variety::American)
    }

    #[test]
    fn finds_formulas_as_the_reading_card_does() {
        let spans = |t: &str| find(t).into_iter().map(|f| t[f.range].to_string()).collect::<Vec<_>>();
        assert_eq!(spans("The area is $A = \\pi r^2$, for any $r$."), ["$A = \\pi r^2$", "$r$"]);
        assert_eq!(spans("Display: $$\\sum_{k=1}^n k$$ and \\(x\\) and \\[y\\]"), ["$$\\sum_{k=1}^n k$$", "\\(x\\)", "\\[y\\]"]);
        // Prices, a currency and shell variables are no maths.
        assert!(find("It costs $5 and $10 at most.").is_empty());
        assert!(find("From $5–$10, or US$5.").is_empty());
        assert!(find("echo $HOME/$USER").is_empty());
        assert!(find("$5 now; then").is_empty());
        assert_eq!(spans("$5 now; then $x^2$"), ["$x^2$"]);
        // An escaped dollar never closes it.
        assert_eq!(spans("$a \\$ b$"), ["$a \\$ b$"]);
        assert!(find("$ x$").is_empty());
        assert!(find("$x $").is_empty());
        assert!(find("$$").is_empty());
        let f = &find("a $$x$$ b")[0];
        assert!(f.display);
    }

    #[test]
    fn powers() {
        assert_eq!(uk("x^2"), "x squared");
        assert_eq!(uk("x^3"), "x cubed");
        assert_eq!(uk("x^4"), "x to the fourth");
        assert_eq!(uk("x^{10}"), "x to the tenth");
        assert_eq!(uk("x^n"), "x to the n");
        assert_eq!(uk("e^x"), "e to the x");
        assert_eq!(uk("e^{-x}"), "e to the minus x");
        assert_eq!(us("e^{-x}"), "e to the negative x");
        assert_eq!(uk("x^{-1}"), "x to the minus one");
        assert_eq!(uk("x^{n+1}"), "x to the power of n plus one");
        assert_eq!(uk("x^{n+1} + 1"), "x to the power of n plus one, plus one");
        assert_eq!(uk("x^n + 1"), "x to the n plus one");
        assert_eq!(uk("2^{10}"), "two to the tenth");
        assert_eq!(uk("10^{6}"), "ten to the sixth");
        assert_eq!(uk("x^{1/2}"), "x to the power of one half");
        assert_eq!(uk("x^{\\frac{1}{2}}"), "x to the power of one half");
        assert_eq!(uk("x^\\alpha"), "x to the alpha");
        assert_eq!(uk("A^T"), "A transpose");
        assert_eq!(uk("A^\\top"), "A transpose");
        assert_eq!(uk("90^\\circ"), "ninety degrees");
        assert_eq!(uk("x^23"), "x squared three");
        assert_eq!(uk("x²"), "x squared");
        assert_eq!(uk("x³ + y⁴"), "x cubed plus y to the fourth");
        assert_eq!(uk("10¹⁰"), "ten to the tenth");
        assert_eq!(uk("x^{200}"), "x to the power of two hundred");
    }

    #[test]
    fn brackets_raised_to_a_power() {
        assert_eq!(uk("(a+b)^2"), "a plus b, all squared");
        assert_eq!(us("(a+b)^2"), "the quantity a plus b, squared");
        assert_eq!(uk("(a+b)^2 = a^2 + 2ab + b^2"), "a plus b, all squared equals a squared plus two a b plus b squared");
        assert_eq!(uk("(x)^2"), "x squared");
        assert_eq!(uk("(1+r)^n"), "one plus r, all to the n");
    }

    #[test]
    fn fractions() {
        assert_eq!(uk("\\frac{a}{b}"), "a over b");
        assert_eq!(uk("\\frac{1}{2}"), "one half");
        assert_eq!(uk("\\frac{3}{4}"), "three quarters");
        assert_eq!(us("\\frac{3}{4}"), "three fourths");
        assert_eq!(uk("\\frac{1}{4}"), "one quarter");
        assert_eq!(us("\\frac14"), "one fourth");
        assert_eq!(uk("\\frac{2}{3}"), "two thirds");
        assert_eq!(uk("\\frac{5}{8}"), "five eighths");
        assert_eq!(uk("\\tfrac{1}{10}"), "one tenth");
        assert_eq!(uk("\\frac{22}{7}"), "twenty-two over seven");
        assert_eq!(uk("\\frac{1}{12}"), "one over twelve");
        assert_eq!(uk("\\frac12"), "one half");
        assert_eq!(uk("\\dfrac{x^2}{2}"), "x squared over two");
        assert_eq!(uk("\\frac{a+b}{2}"), "the fraction a plus b, over two");
        assert_eq!(uk("\\frac{a+b}{c+d} + 1"), "the fraction a plus b, over c plus d, plus one");
        assert_eq!(uk("\\frac{-b \\pm \\sqrt{b^2-4ac}}{2a}"), "the fraction minus b plus or minus the square root of b squared minus four a c, over two a");
        assert_eq!(us("\\frac{-b \\pm \\sqrt{b^2-4ac}}{2a}"), "the fraction negative b plus or minus the square root of b squared minus four a c, over two a");
        assert_eq!(uk("{a \\over b}"), "a over b");
        assert_eq!(uk("a/b"), "a over b");
    }

    #[test]
    fn derivatives_in_leibniz_notation() {
        assert_eq!(uk("\\frac{dy}{dx}"), "d y by d x");
        assert_eq!(us("\\frac{dy}{dx}"), "d y d x");
        assert_eq!(uk("\\frac{d}{dx} x^2 = 2x"), "d by d x of x squared equals two x");
        assert_eq!(uk("\\frac{\\partial f}{\\partial x}"), "partial f by partial x");
        assert_eq!(uk("\\frac{d^2y}{dx^2}"), "d squared y by d x squared");
        assert_eq!(uk("\\frac{\\mathrm{d}y}{\\mathrm{d}x}"), "d y by d x");
    }

    #[test]
    fn roots() {
        assert_eq!(uk("\\sqrt{x}"), "the square root of x");
        assert_eq!(uk("\\sqrt2"), "the square root of two");
        assert_eq!(uk("\\sqrt[3]{8} = 2"), "the cube root of eight equals two");
        assert_eq!(uk("\\sqrt[4]{x}"), "the fourth root of x");
        assert_eq!(uk("\\sqrt[n]{x}"), "the nth root of x");
        assert_eq!(uk("\\sqrt{x+1} + 2"), "the square root of x plus one, plus two");
        assert_eq!(uk("\\sqrt{a^2 + b^2}"), "the square root of a squared plus b squared");
        assert_eq!(uk("√2"), "the square root of two");
        assert_eq!(uk("√{x}"), "the square root of x");
    }

    #[test]
    fn greek_letters_and_symbols() {
        assert_eq!(uk("\\pi r^2"), "pi r squared");
        assert_eq!(uk("A = \\pi r^2"), "A equals pi r squared");
        assert_eq!(uk("2\\pi r"), "two pi r");
        assert_eq!(uk("\\alpha + \\beta = \\gamma"), "alpha plus beta equals gamma");
        assert_eq!(uk("\\theta \\approx 0.785"), "theta is approximately zero point seven eight five");
        assert_eq!(uk("\\Delta x"), "capital delta x");
        assert_eq!(uk("\\varepsilon > 0"), "epsilon is greater than zero");
        assert_eq!(uk("\\lambda \\mu \\sigma \\omega \\phi \\varphi \\chi \\psi"), "lambda mu sigma omega phi phi chi psi");
        assert_eq!(uk("π r²"), "pi r squared");
        assert_eq!(uk("θ = 2π"), "theta equals two pi");
        assert_eq!(uk("\\Omega"), "capital omega");
        assert_eq!(uk("x \\to \\infty"), "x to infinity");
        assert_eq!(uk("\\hbar \\omega"), "h bar omega");
    }

    #[test]
    fn operators_and_relations() {
        assert_eq!(uk("x \\pm 1"), "x plus or minus one");
        assert_eq!(uk("\\pm\\sqrt{2}"), "plus or minus the square root of two");
        assert_eq!(uk("3 \\times 4 = 12"), "three times four equals twelve");
        assert_eq!(uk("a \\cdot b"), "a times b");
        assert_eq!(uk("6 \\div 2"), "six divided by two");
        assert_eq!(uk("x \\le 5"), "x is less than or equal to five");
        assert_eq!(uk("x \\leq 5"), "x is less than or equal to five");
        assert_eq!(uk("y \\ge 0"), "y is greater than or equal to zero");
        assert_eq!(uk("y \\geq 0"), "y is greater than or equal to zero");
        assert_eq!(uk("a < b > c"), "a is less than b is greater than c");
        assert_eq!(uk("a \\neq b"), "a is not equal to b");
        assert_eq!(uk("a \\ne b"), "a is not equal to b");
        assert_eq!(uk("a \\not= b"), "a is not equal to b");
        assert_eq!(uk("x \\not\\in A"), "x is not in A");
        assert_eq!(uk("x \\in \\mathbb{R}"), "x is in the real numbers");
        assert_eq!(uk("n \\in \\mathbb{N}"), "n is in the natural numbers");
        assert_eq!(uk("A \\subseteq B"), "A is a subset of B");
        assert_eq!(uk("A \\cup B"), "A union B");
        assert_eq!(uk("p \\Rightarrow q"), "p implies q");
        assert_eq!(uk("p \\iff q"), "p if and only if q");
        assert_eq!(uk("F = ma"), "F equals m a");
        assert_eq!(uk("E = mc^2"), "E equals m c squared");
        assert_eq!(uk("a \\equiv b \\pmod{n}"), "a is equivalent to b mod n");
        assert_eq!(uk("y \\propto x"), "y is proportional to x");
        assert_eq!(uk("x := 5"), "x is defined as five");
        assert_eq!(uk("x <= 5"), "x is less than or equal to five");
        assert_eq!(uk("n!"), "n factorial");
        assert_eq!(uk("5\\%"), "five percent");
        assert_eq!(uk("1:2"), "one to two");
    }

    #[test]
    fn minus_signs() {
        assert_eq!(uk("-3"), "minus three");
        assert_eq!(us("-3"), "negative three");
        assert_eq!(uk("5 - 3"), "five minus three");
        assert_eq!(us("5 - 3"), "five minus three");
        assert_eq!(uk("x = -2"), "x equals minus two");
        assert_eq!(us("x = -2"), "x equals negative two");
        assert_eq!(uk("a − b"), "a minus b");
        assert_eq!(uk("a - (b + c)"), "a minus, b plus c");
        assert_eq!(uk("y = (a + b)"), "y equals a plus b");
        assert_eq!(uk("(-1)^n"), "minus one, all to the n");
        assert_eq!(us("x = 1 \\text{ or } -1"), "x equals one or negative one");
        assert_eq!(us("\\text{Area} - 2"), "Area minus two");
    }

    #[test]
    fn subscripts() {
        assert_eq!(uk("x_1"), "x sub one");
        assert_eq!(uk("x_1 + x_2"), "x sub one plus x sub two");
        assert_eq!(uk("a_n"), "a sub n");
        assert_eq!(uk("x_{i+1} = x_i"), "x sub i plus one, equals x sub i");
        assert_eq!(uk("x_i^2"), "x sub i squared");
        assert_eq!(uk("v_{\\text{max}}"), "v max");
        assert_eq!(uk("x₁ + x₂"), "x sub one plus x sub two");
        assert_eq!(uk("H_2O"), "H sub two O");
        assert_eq!(uk("\\log_2 8 = 3"), "log base two of eight equals three");
        assert_eq!(uk("\\log_{10} x"), "log base ten of x");
    }

    #[test]
    fn sums_integrals_and_limits() {
        assert_eq!(uk("\\sum_{i=1}^{n} x_i"), "the sum from i equals one to n of x sub i");
        assert_eq!(uk("\\sum_{k=1}^n k = \\frac{n(n+1)}{2}"), "the sum from k equals one to n of k, equals the fraction n times, n plus one, over two");
        assert_eq!(us("\\sum_{k=1}^n k = \\frac{n(n+1)}{2}"), "the sum from k equals one to n of k, equals the fraction n times the quantity n plus one, over two");
        assert_eq!(uk("\\sum_{i} a_i"), "the sum over i of a sub i");
        assert_eq!(uk("\\sum_{x \\in S} f(x)"), "the sum over x in S of f of x");
        assert_eq!(uk("\\prod_{i=1}^{n} i = n!"), "the product from i equals one to n of i, equals n factorial");
        assert_eq!(uk("\\int_0^1 x^2 \\, dx = \\frac{1}{3}"), "the integral from zero to one of x squared, d x, equals one third");
        assert_eq!(uk("\\int f(x)\\,dx"), "the integral of f of x, d x");
        assert_eq!(uk("\\int_{-\\infty}^{\\infty} e^{-x^2} dx = \\sqrt{\\pi}"), "the integral from minus infinity to infinity of e to the power of minus x squared, d x, equals the square root of pi");
        assert_eq!(uk("\\iint_D f \\, dA"), "the double integral over D of f, d A");
        assert_eq!(uk("\\lim_{x \\to 0} \\frac{\\sin x}{x} = 1"), "the limit as x approaches zero of the fraction sine of x, over x, equals one");
        assert_eq!(uk("\\lim_{n\\to\\infty} (1 + \\frac{1}{n})^n = e"), "the limit as n approaches infinity of one plus one over n, all to the n, equals e");
        assert_eq!(uk("\\max_{x} f(x)"), "the maximum over x of f of x");
        assert_eq!(uk("∑_{i=1}^{n} i"), "the sum from i equals one to n of i");
        assert_eq!(uk("\\sum x"), "the sum of x");
        assert_eq!(uk("\\displaystyle\\sum\\limits_{i=1}^{n} i"), "the sum from i equals one to n of i");
    }

    #[test]
    fn functions() {
        assert_eq!(uk("\\sin x"), "sine of x");
        assert_eq!(uk("\\sin(x)"), "sine of x");
        assert_eq!(uk("\\sin^2 x + \\cos^2 x = 1"), "sine squared of x plus cosine squared of x equals one");
        assert_eq!(uk("\\sin^{-1} x"), "inverse sine of x");
        assert_eq!(uk("\\tan \\theta"), "tangent of theta");
        assert_eq!(uk("\\ln(x+1)"), "natural log of x plus one");
        assert_eq!(uk("\\ln x"), "natural log of x");
        assert_eq!(uk("\\exp(x)"), "the exponential of x");
        assert_eq!(uk("\\det A"), "the determinant of A");
        assert_eq!(uk("f(x) = x^2"), "f of x equals x squared");
        assert_eq!(uk("f'(x)"), "f prime of x");
        assert_eq!(uk("f''(x)"), "f double prime of x");
        assert_eq!(uk("g(x, y)"), "g of x, y");
        assert_eq!(uk("P(A \\mid B)"), "P of A given B");
        assert_eq!(uk("P(A|B)"), "P of A given B");
        assert_eq!(uk("y(t)"), "y of t");
        assert_eq!(uk("2(x+1)"), "two times, x plus one");
        assert_eq!(us("2(x+1)"), "two times the quantity x plus one");
        assert_eq!(uk("(a+b)(a-b)"), "a plus b, times, a minus b");
        assert_eq!(uk("sin x"), "sine of x");
        assert_eq!(uk("\\operatorname{sgn}(x)"), "the sign of x");
    }

    #[test]
    fn fences() {
        assert_eq!(uk("|x|"), "the absolute value of x");
        assert_eq!(uk("|x - 1| < 2"), "the absolute value of x minus one, is less than two");
        assert_eq!(uk("\\left| x \\right|"), "the absolute value of x");
        assert_eq!(uk("\\lvert x \\rvert"), "the absolute value of x");
        assert_eq!(uk("\\|v\\|"), "the norm of v");
        assert_eq!(uk("\\lfloor x \\rfloor"), "the floor of x");
        assert_eq!(uk("\\lceil x \\rceil"), "the ceiling of x");
        assert_eq!(uk("\\{1, 2, 3\\}"), "the set one, two, three");
        assert_eq!(uk("\\{x \\mid x > 0\\}"), "the set of x such that x is greater than zero");
        assert_eq!(uk("\\{ x : x > 0 \\}"), "the set of x such that x is greater than zero");
        assert_eq!(uk("\\left( \\frac{a}{b} \\right)^2"), "a over b, all squared");
        assert_eq!(uk("\\left. \\frac{df}{dx} \\right|_{x=0}"), "d f by d x, evaluated at x equals zero");
        assert_eq!(uk("\\left. x^2 \\right|_0^1"), "x squared, evaluated from zero to one");
        assert_eq!(uk("[0, 1]"), "zero, one");
    }

    #[test]
    fn accents_fonts_and_text() {
        assert_eq!(uk("\\hat{x}"), "x hat");
        assert_eq!(uk("\\bar{x} = \\frac{1}{n}\\sum x_i"), "x bar equals one over n times the sum of x sub i");
        assert_eq!(uk("\\vec{v}"), "vector v");
        assert_eq!(uk("\\dot{x}"), "x dot");
        assert_eq!(uk("\\mathbf{v} \\cdot \\mathbf{w}"), "v times w");
        assert_eq!(uk("\\text{Area} = \\pi r^2"), "Area equals pi r squared");
        assert_eq!(uk("Area = \\pi r^2"), "Area equals pi r squared");
        assert_eq!(uk("\\text{speed} = \\frac{\\text{distance}}{\\text{time}}"), "speed equals distance over time");
        assert_eq!(uk("x = 1 \\text{ or } x = 2"), "x equals one or x equals two");
        assert_eq!(uk("x = 1 \\quad \\text{or} \\quad x = 2"), "x equals one, or, x equals two");
        assert_eq!(uk("\\boxed{x = 3}"), "x equals three");
        assert_eq!(uk("{\\color{red} x}"), "x");
        assert_eq!(uk("\\textcolor{blue}{y}"), "y");
        assert_eq!(uk("\\mathcal{L}"), "L");
    }

    #[test]
    fn numbers_are_said_as_numbers() {
        assert_eq!(uk("1000000"), "one million");
        assert_eq!(uk("1{,}000"), "one thousand");
        assert_eq!(uk("1,000"), "one thousand");
        assert_eq!(uk("3.14159"), "three point one four one five nine");
        assert_eq!(uk("0.5"), "zero point five");
        assert_eq!(uk(".5"), "point five");
        assert_eq!(uk("105"), "one hundred and five");
        assert_eq!(us("105"), "one hundred five");
        assert_eq!(uk("(1, 2)"), "one, two");
        assert_eq!(uk("007"), "zero zero seven");
    }

    #[test]
    fn environments() {
        assert_eq!(
            uk("f(x) = \\begin{cases} x & x \\ge 0 \\\\ -x & x < 0 \\end{cases}"),
            "f of x equals x if x is greater than or equal to zero; minus x if x is less than zero"
        );
        assert_eq!(uk("|x| = \\begin{cases} x & \\text{if } x \\geq 0 \\\\ -x & \\text{otherwise} \\end{cases}"), "the absolute value of x equals x if x is greater than or equal to zero; minus x otherwise");
        assert_eq!(uk("\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}"), "the two by two matrix with rows one, two; and three, four");
        assert_eq!(uk("\\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix} = ad - bc"), "the determinant of the two by two matrix with rows a, b; and c, d, equals a d minus b c");
        assert_eq!(uk("\\begin{bmatrix} x \\\\ y \\\\ z \\end{bmatrix}"), "the column vector x, y, z");
        assert_eq!(uk("\\begin{aligned} a &= b + c \\\\ &= d \\end{aligned}"), "a equals b plus c; equals d");
        assert_eq!(uk("\\begin{array}{cc} 1 & 2 \\end{array}"), "one two");
        assert_eq!(
            uk("f(x) = \\left\\{ \\begin{array}{ll} 1 & x > 0 \\\\ 0 & \\text{otherwise} \\end{array} \\right."),
            "f of x equals one if x is greater than zero; zero otherwise"
        );
        assert_eq!(uk("\\binom{n}{k}"), "n choose k");
        assert_eq!(uk("{n \\choose k}"), "n choose k");
    }

    #[test]
    fn what_it_does_not_know_is_said_simply() {
        // An unknown symbol by its name; a command round braces by its words.
        assert_eq!(uk("\\aleph_0"), "aleph sub zero");
        assert_eq!(uk("\\weirdsymbol + 1"), "weirdsymbol plus one");
        assert_eq!(uk("\\mathring{A}"), "A");
        assert_eq!(uk("\\hspace{1em} x"), "x");
        assert_eq!(uk("\\label{eq:1} x = 1"), "x equals one");
        // Broken TeX: still words.
        assert_eq!(uk("\\frac{a"), "the fraction a over");
        assert_eq!(uk("x^"), "x to the power of");
        assert_eq!(uk("}x{"), "x");
        assert_eq!(uk("\\left( x"), "x");
        assert_eq!(uk("a \\\\ b"), "a, b");
    }

    #[test]
    fn never_a_backslash_or_a_brace() {
        let formulas = [
            "\\frac{\\partial^2 u}{\\partial t^2} = c^2 \\nabla^2 u",
            "e^{i\\pi} + 1 = 0",
            "\\sum_{n=0}^{\\infty} \\frac{x^n}{n!}",
            "\\int_a^b f'(x)\\,\\mathrm{d}x = f(b) - f(a)",
            "\\mathbb{E}[X] = \\sum_x x\\,P(X=x)",
            "\\nabla \\times \\mathbf{B} = \\mu_0 \\mathbf{J}",
            "\\oint_C \\vec{F} \\cdot d\\vec{r}",
            "\\overbrace{1+1+\\cdots+1}^{n}",
            "\\underset{x}{\\arg\\max}\\; f(x)",
            "\\xrightarrow{f} \\Big( \\big[ x \\big] \\Big)",
            "{}^{14}_{6}\\mathrm{C}",
            "\\$5 \\& \\# \\_ \\{ \\}",
            "a_{b_{c_{d}}}^{e^{f^{g}}}",
            "\\begin{matrix}\\end{matrix}",
            "\\left. \\frac{df}{dx} \\right|_{x=0}",
            "\\stackrel{?}{=}",
            "\\sqrt[]{x}",
            "x^{^{2}}",
            "\\\\\\\\",
            "\\",
            "{",
            "^_^",
        ];
        for f in formulas {
            for v in [Variety::British, Variety::American] {
                let s = say(f, v);
                assert!(!s.contains(['\\', '{', '}', '^', '_', '$', '&', '#']), "{f:?} → {s:?}");
                assert!(!s.starts_with(',') && !s.ends_with(','), "{f:?} → {s:?}");
                assert!(!s.contains(",,") && !s.contains(" ,"), "{f:?} → {s:?}");
            }
        }
        assert_eq!(uk("e^{i\\pi} + 1 = 0"), "e to the power of i pi, plus one equals zero");
        assert_eq!(uk("\\sum_{n=0}^{\\infty} \\frac{x^n}{n!}"), "the sum from n equals zero to infinity of the fraction x to the n over n factorial");
        assert_eq!(uk("\\underset{x}{\\arg\\max}\\; f(x)"), "the arg max over x of f of x");
        assert_eq!(uk("\\operatorname*{argmax}_x f(x)"), "the arg max over x of f of x");
        assert_eq!(uk("\\overbrace{1+1+\\cdots+1}^{n}"), "one plus one plus dot dot dot plus one");
    }

    #[test]
    fn formulas_in_a_sentence_become_their_words() {
        let v = Variety::British;
        assert_eq!(in_words("The area is $A = \\pi r^2$, where $r$ is the radius.", v), "The area is A equals pi r squared, where r is the radius.");
        assert_eq!(in_words("It costs $5 and $10 at most.", v), "It costs $5 and $10 at most.");
        assert_eq!(in_words("So \\(x^2\\) and \\[\\frac{1}{2}\\].", v), "So x squared and one half.");
        assert_eq!(in_words("$$ E = mc^2 $$", v), "E equals m c squared");
        // A letter straight after the closing `$`: not maths (as Chat draws it).
        assert_eq!(in_words("the $n$th term", v), "the $n$th term");
        let (text, map) = in_words_mapped("Is $x^2$ here?", v);
        assert_eq!(text, "Is x squared here?");
        assert_eq!(map, vec![(3..8, 3..12)]);
        // Character offsets, not bytes.
        let (text, map) = in_words_mapped("Café: $π$ ok", v);
        assert_eq!(text, "Café: pi ok");
        assert_eq!(map, vec![(6..9, 6..8)]);
    }

    #[test]
    fn maths_written_as_plain_text() {
        let v = Variety::British;
        // Opus's circle answer, 16 Nov 2026 (Yap Lab), its plain-text lines.
        assert_eq!(plain_in_words("so the formula becomes A = πd²/4.", v), "so the formula becomes A equals pi d squared over four.");
        assert_eq!(plain_in_words("half the diameter (r = d/2), so", v), "half the diameter (r equals d over two), so");
        assert_eq!(
            plain_in_words("an area of π × 5² = 25π ≈ 78.54 square centimetres.", v),
            "an area of pi times five squared equals twenty-five pi is approximately seventy-eight point five four square centimetres."
        );
        assert_eq!(plain_in_words("π (pi) is about 3.14159.", v), "pi (pi) is about 3.14159.");
        assert_eq!(plain_in_words("E = mc² says it", v), "E equals m c squared says it");
        assert_eq!(plain_in_words("so a = 5 and x ≤ 3", v), "so a equals five and x is less than or equal to three");
        assert_eq!(plain_in_words("about √2, or 1.41", v), "about the square root of two, or 1.41");
        assert_eq!(us_plain("x = −3"), "x equals negative three");
        // The Lab's sphere and cone answers: "4/3" and "⅓" are fractions.
        assert_eq!(
            plain_in_words("a volume of 4/3 × π × 27 = 36π, which is about 113.1 cubic centimetres.", v),
            "a volume of four thirds times pi times twenty-seven equals thirty-six pi, which is about 113.1 cubic centimetres."
        );
        assert_eq!(
            plain_in_words("a volume of ⅓ × π × 9 × 4 = 12π, which is", v),
            "a volume of one third times pi times nine times four equals twelve pi, which is"
        );
        assert_eq!(plain_in_words("add ½ a cup", v), "add one half a cup");
        assert_eq!(uk("22/7"), "twenty-two over seven");
        // Left alone: prose, a size, a range, code, units, arrows.
        for s in ["Set width = 100 in the file.", "a 3 × 4 grid", "from 10 - 15 people", "and/or", "Friday → Monday", "5 μm and 10 Ω", "x=1 in code", "It's 5 = good"] {
            assert_eq!(plain_in_words(s, v), s);
        }
    }

    fn us_plain(s: &str) -> String {
        plain_in_words(s, Variety::American)
    }

    /// Formulas as chat answers write them (physics, finance, statistics,
    /// calculus), said in full.
    #[test]
    fn formulas_from_real_answers() {
        let cases = [
            ("a^2 + b^2 = c^2", "a squared plus b squared equals c squared"),
            ("C = 2\\pi r", "C equals two pi r"),
            ("V = \\frac{4}{3}\\pi r^3", "V equals four thirds pi r cubed"),
            ("E = \\frac{1}{2}mv^2", "E equals one half m v squared"),
            ("s = ut + \\tfrac{1}{2}at^2", "s equals u t plus one half a t squared"),
            ("y = mx + c", "y equals m x plus c"),
            ("F = G\\frac{m_1 m_2}{r^2}", "F equals G m sub one m sub two over r squared"),
            ("A = \\frac{\\pi d^2}{4}", "A equals pi d squared over four"),
            ("V = \\frac{\\pi d^3}{6}", "V equals pi d cubed over six"),
            ("\\vec{F} = m\\vec{a}", "vector F equals m vector a"),
            ("T = 2\\pi\\sqrt{\\frac{L}{g}}", "T equals two pi times the square root of L over g"),
            ("\\theta = 45^\\circ", "theta equals forty-five degrees"),
            ("\\Delta G = \\Delta H - T\\Delta S", "capital delta G equals capital delta H minus T capital delta S"),
            ("pH = -\\log_{10}[\\text{H}^+]", "p H equals minus log base ten of H plus"),
            ("A = P\\left(1 + \\frac{r}{n}\\right)^{nt}", "A equals P times, one plus r over n, all to the power of n t"),
            ("z = \\frac{x - \\mu}{\\sigma}", "z equals the fraction x minus mu, over sigma"),
            (
                "\\sigma = \\sqrt{\\frac{1}{N}\\sum_{i=1}^{N}(x_i - \\mu)^2}",
                "sigma equals the square root of one over N times the sum from i equals one to N of x sub i minus mu, all squared",
            ),
            ("P(A \\mid B) = \\frac{P(B \\mid A)\\,P(A)}{P(B)}", "P of A given B, equals the fraction P of B given A, times P of A, over P of B"),
            ("e^{i\\theta} = \\cos\\theta + i\\sin\\theta", "e to the power of i theta, equals cosine of theta plus i sine of theta"),
            ("\\sin x \\cos x", "sine of x times cosine of x"),
            ("\\sin(x)\\cos(x)", "sine of x times cosine of x"),
            ("\\frac{d}{dx}\\left[x^n\\right] = nx^{n-1}", "d by d x of x to the n equals n x to the power of n minus one"),
            ("\\int x^n \\, dx = \\frac{x^{n+1}}{n+1} + C", "the integral of x to the n, d x, equals the fraction x to the power of n plus one, over n plus one, plus C"),
            ("\\lim_{h \\to 0} \\frac{f(x+h) - f(x)}{h}", "the limit as h approaches zero of the fraction f of x plus h, minus f of x, over h"),
            ("n! = n \\times (n-1) \\times \\cdots \\times 1", "n factorial equals n times, n minus one, times dot dot dot times one"),
            ("x != y", "x is not equal to y"),
            ("a_n = a_1 + (n-1)d", "a sub n equals a sub one plus, n minus one, times d"),
            ("S_n = \\frac{n}{2}(a_1 + a_n)", "S sub n equals n over two times, a sub one plus a sub n"),
            ("\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}", "the sum from n equals one to infinity of one over n squared, equals pi squared over six"),
            ("\\det(A - \\lambda I) = 0", "the determinant of A minus lambda I, equals zero"),
            ("\\log(ab) = \\log a + \\log b", "log of a b, equals log of a plus log of b"),
            ("\\text{BMI} = \\frac{\\text{mass}}{\\text{height}^2}", "BMI equals mass over height squared"),
        ];
        for (tex, words) in cases {
            assert_eq!(uk(tex), words, "{tex}");
        }
        assert_eq!(us("A = P\\left(1 + \\frac{r}{n}\\right)^{nt}"), "A equals P times the quantity one plus r over n, to the power of n t");
        assert_eq!(us("pH = -\\log_{10}[\\text{H}^+]"), "p H equals negative log base ten of H plus");
    }
}
