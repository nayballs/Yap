//! **A project file's parts** (16 Nov 2026, the Claude app's project
//! knowledge): a file too big to send whole with every question is read in
//! parts of about [`TARGET`] tokens, each knowing where it is in the file —
//! its pages, slides, rows or lines, and the section it's in — so the model
//! can say where an answer comes from ("staff-handbook.pdf, pages 12–13").
//!
//! The parts come from the text the attachments' readers made
//! (`chat/attachments.rs` `read`), by its kind:
//!
//! - **A PDF** (`pdf.rs`, `scan.rs`): by its `[Page n]` lines (a scanned
//!   page's `[Page n, read with OCR]` too), whole pages together; a page too
//!   long for one part goes on in the next, "[Page n, continued]". Its
//!   sections are lines that look like headings, never a line that repeats
//!   at the top or foot of its pages (a running head, a footer).
//! - **A PowerPoint presentation** (`powerpoint.rs`): by its "Slide n:"
//!   lines, a slide's title its section.
//! - **A spreadsheet** (`sheet.rs`): by its sheets (`## Sheet n of m:`), a
//!   sheet's rows in parts, each part starting with the workbook's line, the
//!   sheet's line and the table's head (column letters) so its cells still
//!   read as "C7"; its formulas a part of their own.
//! - **Markdown**: by its headings, the section a heading's path ("Leave ›
//!   Annual leave"); never inside a fenced block of code.
//! - **A Word document** (`word.rs`, `doc.rs`): by its paragraphs, a heading
//!   its `#` (a `.docx`'s heading styles), else a line that looks like one.
//! - **Code**: by its blank lines, cited by line numbers.
//! - **Anything else**: by paragraphs, cited by lines, a heading as a Word
//!   document's.
//!
//! A new part starts at a section's start once the part has half its size,
//! and never shares a sheet. Pure.

use std::sync::LazyLock;

use regex::Regex;

/// A part's size it aims for, in tokens (`budget::estimate`): as Anthropic's
/// own retrieval write-up chunks documents (about 800), a little smaller so
/// two or three fit beside a small model's question.
pub const TARGET: u64 = 700;
/// …at most (a longer page, paragraph or line is cut at its lines, then its
/// sentences, then its words).
pub const MOST: u64 = 1_100;
/// …and a part smaller than this goes on into the next section.
pub const LEAST: u64 = 160;

/// **One part of a file.**
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Part {
    /// Its number in the file, from 1.
    pub n: usize,
    /// Where it is, as a reader cites it: "page 12", "pages 12–13",
    /// "slides 3–5", "sheet "Budget", rows 2–180", "lines 40–120"; "" when
    /// the file has nothing to count (a Word document's text).
    pub at: String,
    /// The section it's in: the first heading in it, else the one above it
    /// ("Leave › Annual leave"); "" when there's none.
    pub section: String,
    pub text: String,
    pub tokens: u64,
}

impl Part {
    /// **Where it is, in words**: its place and its section ("pages 12–13,
    /// "Annual leave""), its section alone ("section "Annual leave""), or
    /// its number ("part 3"). Pure.
    pub fn place(&self) -> String {
        match (self.at.is_empty(), self.section.is_empty()) {
            (false, false) => format!("{}, \u{201c}{}\u{201d}", self.at, self.section),
            (false, true) => self.at.clone(),
            (true, false) => format!("section \u{201c}{}\u{201d}", self.section),
            (true, true) => format!("part {}", self.n),
        }
    }

    /// **Where it is, told apart from the file's other parts** (a chip under
    /// the answer): its place, or its number and section when it has nothing
    /// to count ("part 86, "ID badges"": a section's parts all say the
    /// section). Pure.
    pub fn named(&self) -> String {
        match (self.at.is_empty(), self.section.is_empty()) {
            (true, false) => format!("part {}, \u{201c}{}\u{201d}", self.n, self.section),
            _ => self.place(),
        }
    }
}

/// What a file's places are counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Noun {
    Page,
    Slide,
    Row,
    Line,
    /// Nothing to count: a part is cited by its section, else its number.
    None,
}

impl Noun {
    fn said(self, from: u32, to: u32) -> String {
        let (one, many) = match self {
            Noun::Page => ("page", "pages"),
            Noun::Slide => ("slide", "slides"),
            Noun::Row => ("row", "rows"),
            Noun::Line => ("line", "lines"),
            Noun::None => return String::new(),
        };
        if from == to {
            format!("{one} {from}")
        } else {
            format!("{many} {from}\u{2013}{to}")
        }
    }
}

/// A piece of the file the parts are made of: a page, a slide, a sheet's
/// row, a paragraph, a heading.
#[derive(Debug, Clone, Default)]
struct Unit {
    text: String,
    tokens: u64,
    /// Its first and last page, slide, row or line.
    from: Option<u32>,
    to: Option<u32>,
    /// A section that starts here (its heading, as the part says it).
    heading: Option<String>,
    /// The section the next unit is in, when this one changes it (a page's
    /// last heading).
    then: Option<String>,
    /// Units of another group never share a part (a sheet each).
    group: usize,
}

/// How a file's units are made into parts.
struct Shape {
    noun: Noun,
    /// Between two units in a part: a blank line, or a line's end.
    join: &'static str,
    /// What every part of a group starts with (a sheet's line and table head),
    /// and what its places are said with (`sheet "Budget"`).
    leads: std::collections::HashMap<usize, (String, String)>,
}

impl Shape {
    fn of(noun: Noun, join: &'static str) -> Shape {
        Shape { noun, join, leads: Default::default() }
    }
}

/// **A file's text in parts**, by its `kind` (`attachments::Read::kind`:
/// "pdf", "powerpoint", "sheet", "markdown", "word", "code", "text"). An
/// empty text has none. Pure.
pub fn split(text: &str, kind: &str) -> Vec<Part> {
    let text = text.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Vec::new();
    }
    let (units, shape) = match kind {
        "pdf" => pdf_units(&text),
        "powerpoint" => slide_units(&text),
        "sheet" => sheet_units(&text),
        "markdown" => markdown_units(&text),
        "word" => word_units(&text),
        "code" => (block_units(&text, false), Shape::of(Noun::Line, "\n\n")),
        _ => (block_units(&text, true), Shape::of(Noun::Line, "\n\n")),
    };
    pack(units.into_iter().flat_map(cut).collect(), &shape)
}

/// Its tokens, as Chat counts them.
fn tokens(text: &str) -> u64 {
    crate::chat::budget::estimate(text)
}

/// **The units packed into parts** of about [`TARGET`] tokens: a new one at
/// a section's start once the part has half that, before one would pass it
/// (with [`LEAST`] in it; a small bit of the same section still joins, up to
/// [`MOST`]), and at another group. A heading never ends a part: it starts
/// the next. A part's section is the one most of its words are in. Pure.
fn pack(units: Vec<Unit>, shape: &Shape) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let mut running = String::new();
    // The part being made: its units, each with the section it's in.
    let mut cur: Vec<(Unit, String)> = Vec::new();
    let flush = |cur: Vec<(Unit, String)>, parts: &mut Vec<Part>| {
        if cur.is_empty() {
            return;
        }
        let group = cur[0].0.group;
        let (lead, scope) = shape.leads.get(&group).cloned().unwrap_or_default();
        let body = cur.iter().map(|(u, _)| u.text.trim_end()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(shape.join);
        let text = if lead.is_empty() { body } else { format!("{lead}\n{body}") };
        let from = cur.iter().filter_map(|(u, _)| u.from).min();
        let to = cur.iter().filter_map(|(u, _)| u.to.or(u.from)).max();
        let counted = match (from, to) {
            (Some(a), Some(b)) => shape.noun.said(a, b.max(a)),
            _ => String::new(),
        };
        let at = match (scope.is_empty(), counted.is_empty()) {
            (true, _) => counted,
            (false, true) => scope,
            (false, false) => format!("{scope}, {counted}"),
        };
        // The section most of it is in (the first, on a tie).
        let mut shares: Vec<(&str, u64)> = Vec::new();
        for (u, sec) in &cur {
            match shares.iter_mut().find(|(s, _)| *s == sec.as_str()) {
                Some((_, n)) => *n += u.tokens.max(1),
                None => shares.push((sec.as_str(), u.tokens.max(1))),
            }
        }
        let best = shares.iter().map(|(_, n)| *n).max().unwrap_or(0);
        let section = shares.iter().find(|(_, n)| *n == best).map(|(s, _)| s.to_string()).unwrap_or_default();
        let tokens = tokens(&text);
        parts.push(Part { n: parts.len() + 1, at, section, text, tokens });
    };
    for u in units {
        let cur_tokens: u64 = cur.iter().map(|(u, _)| u.tokens).sum();
        if let Some((last, _)) = cur.last() {
            let other_group = u.group != last.group;
            let at_section = u.heading.is_some() && cur_tokens >= TARGET / 2;
            let small_tail = u.heading.is_none() && u.tokens < LEAST / 4;
            let too_big = cur_tokens + u.tokens > TARGET && cur_tokens >= LEAST && !small_tail;
            let past_most = cur_tokens + u.tokens > MOST;
            if other_group || at_section || too_big || past_most {
                // A heading at the part's end goes on with what it heads.
                let mut carried: Vec<(Unit, String)> = Vec::new();
                while !other_group && cur.len() > 1 && cur.last().is_some_and(|(l, _)| l.heading.is_some() && l.text.lines().count() == 1) {
                    carried.extend(cur.pop());
                }
                carried.reverse();
                flush(std::mem::take(&mut cur), &mut parts);
                cur = carried;
            }
        }
        if let Some(h) = u.heading.as_ref() {
            running = h.clone();
        }
        let sec = running.clone();
        if let Some(h) = u.then.as_ref() {
            running = h.clone();
        }
        cur.push((u, sec));
    }
    flush(cur, &mut parts);
    parts
}

/// **A unit too long for a part, cut** at its lines, then its sentences,
/// then its words, each piece at most [`TARGET`]: the pieces keep its
/// places and its heading (the first); a page's go on as "[Page n,
/// continued]". Pure.
fn cut(u: Unit) -> Vec<Unit> {
    if u.tokens <= MOST {
        return vec![u];
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut piece = String::new();
    let push = |piece: &mut String, pieces: &mut Vec<String>| {
        if !piece.trim().is_empty() {
            pieces.push(std::mem::take(piece));
        }
        piece.clear();
    };
    for bit in small_bits(&u.text) {
        if !piece.is_empty() && tokens(&piece) + tokens(&bit) > TARGET {
            push(&mut piece, &mut pieces);
        }
        piece.push_str(&bit);
    }
    push(&mut piece, &mut pieces);
    let page_mark = u.text.lines().next().and_then(|l| PAGE.captures(l.trim())).map(|c| c[1].to_string());
    let mut out: Vec<Unit> = pieces
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let text = match (&page_mark, i) {
                (Some(n), i) if i > 0 => format!("[Page {n}, continued]\n{}", text.trim_start()),
                _ => text,
            };
            Unit { tokens: tokens(&text), text, heading: if i == 0 { u.heading.clone() } else { None }, then: None, ..u.clone() }
        })
        .collect();
    // (The section the next unit is in: the last piece's to say.)
    if let Some(last) = out.last_mut() {
        last.then = u.then.clone();
    }
    out
}

/// A text in pieces small enough to pack: its lines (each with its end), a
/// line too long cut at its sentences, a sentence too long at its words.
fn small_bits(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split_inclusive('\n') {
        if tokens(line) <= TARGET {
            out.push(line.to_string());
            continue;
        }
        for sentence in line.split_inclusive(['.', '!', '?']) {
            if tokens(sentence) <= TARGET {
                out.push(sentence.to_string());
                continue;
            }
            let mut words = String::new();
            for w in sentence.split_inclusive(' ') {
                if !words.is_empty() && tokens(&words) + tokens(w) > TARGET {
                    out.push(std::mem::take(&mut words));
                }
                words.push_str(w);
            }
            if !words.is_empty() {
                out.push(words);
            }
        }
    }
    out
}

// ---- headings ------------------------------------------------------------------------------------

/// **A line that looks like a heading** in text that doesn't mark them (a
/// PDF's, an older Word document's, a text file's): short (at most 80
/// characters, 10 words), starting with a capital or a number ("4.2 Annual
/// leave", "1. Introduction"), not a table's row or a sentence (no full
/// stop, comma or colon at its end), not a page's number or a date, not a
/// list's item (another follows it) — and followed by something longer than
/// itself (a paragraph). Pure.
pub fn heading_like(line: &str, next: Option<&str>) -> bool {
    let t = line.trim();
    let chars = t.chars().count();
    if t.is_empty() || chars > 80 || t.split_whitespace().count() > 10 {
        return false;
    }
    let Some(first) = t.chars().next() else { return false };
    if !(first.is_uppercase() || first.is_ascii_digit()) || !t.chars().any(char::is_alphabetic) {
        return false;
    }
    if t.ends_with(['.', ',', ';', ':', '!', '?']) || t.contains(['\t', '|']) || NOT_A_HEADING.is_match(t) {
        return false;
    }
    let Some(next) = next.map(str::trim) else { return false };
    // "1) Buy seeds" then "2) Sow them…": a list.
    if LIST_ITEM.is_match(t) && LIST_ITEM.is_match(next) {
        return false;
    }
    next.chars().count() > chars.max(40)
}

/// A page's number ("Page 3 of 60", "3", "3 / 60"), a date alone.
static NOT_A_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:page\s+\d+(?:\s+of\s+\d+)?|\d+(?:\s*/\s*\d+)?|\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4})$").expect("static regex"));
/// A numbered list's item ("1) …", "2. …", "a) …").
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:\d+|[a-z])[).]\s").expect("static regex"));

/// A Markdown heading's level and words (`## Annual leave` → 2).
fn md_heading(line: &str) -> Option<(usize, String)> {
    let t = line.trim_end();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) || !t[hashes..].starts_with(' ') {
        return None;
    }
    let words = t[hashes..].trim().trim_end_matches('#').trim();
    (!words.is_empty()).then(|| (hashes, words.to_string()))
}

/// Headings as a path: the last two of them, the document's title left out
/// (`titled`: its only `#`, over deeper ones: [`titled`]). Pure.
fn path_of(stack: &[(usize, String)], titled: bool) -> String {
    let skip_title = titled && stack.len() > 1 && stack[0].0 == 1;
    let kept: Vec<&str> = stack.iter().skip(usize::from(skip_title)).map(|(_, w)| w.as_str()).collect();
    let from = kept.len().saturating_sub(2);
    kept[from..].join(" \u{203a} ")
}

/// A document whose one `#` heading is its title: the others are deeper
/// (`## …`). Pure.
fn titled(lines: &[&str]) -> bool {
    // (Never a line in a fenced block of code.)
    let mut fenced = false;
    let levels: Vec<usize> = lines
        .iter()
        .filter(|l| {
            if l.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            !fenced
        })
        .filter_map(|l| md_heading(l).map(|(level, _)| level))
        .collect();
    levels.iter().filter(|l| **l == 1).count() == 1 && levels.iter().any(|l| *l > 1)
}

/// A heading's place in the stack: deeper ones under it go.
fn push_heading(stack: &mut Vec<(usize, String)>, level: usize, words: String) {
    while stack.last().is_some_and(|(l, _)| *l >= level) {
        stack.pop();
    }
    stack.push((level, words));
}

// ---- by kind ----------------------------------------------------------------------------------

/// A PDF's page line (`pdf.rs`): "[Page 3]", "[Page 3, read with OCR]".
static PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[Page (\d+)(?:, read with OCR)?\]$").expect("static regex"));

/// A PDF's pages, each a unit.
fn pdf_units(text: &str) -> (Vec<Unit>, Shape) {
    let mut pages: Vec<(Option<u32>, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        match PAGE.captures(line.trim()) {
            Some(c) => pages.push((c[1].parse().ok(), vec![line])),
            None => match pages.last_mut() {
                Some((_, lines)) => lines.push(line),
                None => pages.push((None, vec![line])),
            },
        }
    }
    // A running head or footer: a line at the top or foot of at least 3
    // pages and a third of them.
    let mut edges: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, lines) in &pages {
        let body: Vec<&str> = lines.iter().skip(1).map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        let mut seen = std::collections::HashSet::new();
        for l in body.iter().take(2).chain(body.iter().rev().take(2)) {
            let key = l.trim_matches(|c: char| c.is_ascii_digit() || c.is_whitespace()).to_lowercase();
            if !key.is_empty() && seen.insert(key.clone()) {
                *edges.entry(key).or_default() += 1;
            }
        }
    }
    let count = pages.len();
    let running = |l: &str| {
        let key = l.trim().trim_matches(|c: char| c.is_ascii_digit() || c.is_whitespace()).to_lowercase();
        edges.get(&key).is_some_and(|&n| n >= 3 && n * 3 >= count)
    };
    let units = pages
        .into_iter()
        .map(|(page, lines)| {
            // Its lines that say something (not blank, not a running head or footer).
            let body: Vec<&str> = lines.iter().skip(usize::from(page.is_some())).copied().filter(|l| !l.trim().is_empty() && !running(l)).collect();
            let headings: Vec<(usize, String)> = (0..body.len()).filter(|&i| heading_like(body[i], body.get(i + 1).copied())).map(|i| (i, body[i].trim().to_string())).collect();
            let text = lines.join("\n");
            // A section starts with the page when its heading tops it; one
            // further down is the next page's section.
            let heading = headings.first().filter(|(i, _)| *i == 0).map(|(_, h)| h.clone());
            Unit { tokens: tokens(&text), text, from: page, to: page, heading, then: headings.last().map(|(_, h)| h.clone()), group: 0 }
        })
        .collect();
    (units, Shape::of(Noun::Page, "\n\n"))
}

/// A presentation's slide line (`powerpoint.rs`): "Slide 3: Spring planting".
static SLIDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Slide (\d+)(?: \(hidden\))?:(.*)$").expect("static regex"));

/// A presentation's slides, each a unit, its title its section.
fn slide_units(text: &str) -> (Vec<Unit>, Shape) {
    let mut slides: Vec<(Option<u32>, String, Vec<&str>)> = Vec::new();
    for line in text.lines() {
        match SLIDE.captures(line) {
            Some(c) => {
                let title = c[2].trim().trim_start_matches(':').trim();
                let title = if title == "(no text)" { "" } else { title };
                slides.push((c[1].parse().ok(), title.to_string(), vec![line]));
            }
            None => match slides.last_mut() {
                Some((_, _, lines)) => lines.push(line),
                None => slides.push((None, String::new(), vec![line])),
            },
        }
    }
    let units = slides
        .into_iter()
        .map(|(n, title, lines)| {
            let text = lines.join("\n").trim_end().to_string();
            Unit { tokens: tokens(&text), text, from: n, to: n, heading: (!title.is_empty()).then_some(title), then: None, group: 0 }
        })
        .collect();
    (units, Shape::of(Noun::Slide, "\n\n"))
}

/// A sheet's line (`attachments/sheet.rs`): `## Sheet 2 of 3: "Budget"`.
static SHEET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^## Sheet \d+ of \d+: "(.*)"(?: \(hidden\))?$"#).expect("static regex"));
/// A table's row, its number first: `| 12 | Beans | 4 |`.
static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\| (\d+) \|").expect("static regex"));

/// **A workbook's sheets**: each a group, its rows units after its line and
/// the table's head (every part of it starts with them), its formulas and
/// notes a unit of their own (the sheet's line before them). The book's own
/// first line goes with the first part.
fn sheet_units(text: &str) -> (Vec<Unit>, Shape) {
    let lines: Vec<&str> = text.lines().collect();
    // Where each sheet starts (a CSV's one table: at its start).
    let mut starts: Vec<usize> = (0..lines.len()).filter(|&i| SHEET.is_match(lines[i])).collect();
    let book_head = match starts.first() {
        Some(&s) => lines[..s].join("\n").trim().to_string(),
        None => {
            starts.push(0);
            String::new()
        }
    };
    let mut units = Vec::new();
    let mut leads = std::collections::HashMap::new();
    for (g, &s) in starts.iter().enumerate() {
        let end = starts.get(g + 1).copied().unwrap_or(lines.len());
        let sheet = &lines[s..end];
        let name = SHEET.captures(sheet[0]).map(|c| c[1].to_string());
        let scope = name.as_ref().map(|n| format!("sheet \u{201c}{n}\u{201d}")).unwrap_or_default();
        // The sheet's line and facts, then the table's head (`| |`, `|---|`).
        let head_at = sheet.iter().position(|l| l.starts_with("| |"));
        let (intro, table_head, body) = match head_at {
            Some(h) => (sheet[..h].join("\n").trim().to_string(), sheet[h..(h + 2).min(sheet.len())].join("\n"), &sheet[(h + 2).min(sheet.len())..]),
            None => (sheet.join("\n").trim().to_string(), String::new(), &sheet[sheet.len()..]),
        };
        let sheet_line = sheet[0].trim().to_string();
        let mut lead = if table_head.is_empty() { intro.clone() } else { format!("{intro}\n\n{table_head}") };
        // (The book's own line, saying what it is and its sheets, too: one line.)
        if !book_head.is_empty() {
            lead = format!("{book_head}\n\n{lead}");
        }
        leads.insert(g, (lead, scope.clone()));
        let mut rows: Vec<Unit> = Vec::new();
        let mut rest: Vec<&str> = Vec::new();
        for l in body {
            match ROW.captures(l) {
                Some(c) if rest.iter().all(|r| r.trim().is_empty()) => {
                    let n = c[1].parse().ok();
                    rows.push(Unit { text: l.to_string(), tokens: tokens(l), from: n, to: n, heading: None, then: None, group: g });
                }
                _ => rest.push(l),
            }
        }
        if rows.is_empty() && table_head.is_empty() {
            // (An empty sheet: its line alone.)
            rows.push(Unit { text: String::new(), tokens: 0, group: g, ..Unit::default() });
        }
        units.extend(rows);
        let rest = rest.join("\n").trim().to_string();
        if !rest.is_empty() {
            // Its formulas and notes: a group of their own, the sheet's line before them.
            let group = 1000 + g;
            let what = if rest.contains("Formulas") { "formulas" } else { "notes" };
            leads.insert(group, (sheet_line.clone(), if scope.is_empty() { what.to_string() } else { format!("{scope}, {what}") }));
            units.push(Unit { tokens: tokens(&rest), text: rest, group, ..Unit::default() });
        }
    }
    (units, Shape { noun: Noun::Row, join: "\n", leads })
}

/// Lines in blocks (a blank line between): each block a unit, its lines its
/// places; inside a Markdown fence (```), blank lines don't end one.
fn blocks(text: &str) -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut start = 0u32;
    let mut fenced = false;
    for (i, line) in text.lines().enumerate() {
        let n = i as u32 + 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if line.trim().is_empty() && !fenced {
            if !cur.is_empty() {
                out.push((start, n - 1, cur.join("\n")));
                cur.clear();
            }
            continue;
        }
        if cur.is_empty() {
            start = n;
        }
        cur.push(line);
    }
    if !cur.is_empty() {
        let n = text.lines().count() as u32;
        out.push((start, n, cur.join("\n")));
    }
    out
}

/// A text's (or code's) blocks; a one-line block that looks like a heading
/// starts a section (`headings`: not code).
fn block_units(text: &str, headings: bool) -> Vec<Unit> {
    let bs = blocks(text);
    (0..bs.len())
        .map(|i| {
            let (from, to, text) = &bs[i];
            let one_line = !text.contains('\n');
            let next = bs.get(i + 1).map(|b| b.2.lines().next().unwrap_or(""));
            let heading = (headings && one_line && (heading_like(text, next) || md_heading(text).is_some())).then(|| md_heading(text).map(|(_, w)| w).unwrap_or_else(|| text.trim().to_string()));
            Unit { tokens: tokens(text), text: text.clone(), from: Some(*from), to: Some(*to), heading, then: None, group: 0 }
        })
        .collect()
}

/// **Markdown**: its blocks, a heading its own (its path the section).
fn markdown_units(text: &str) -> (Vec<Unit>, Shape) {
    let title = titled(&text.lines().collect::<Vec<_>>());
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut units = Vec::new();
    let mut fenced = false;
    for (from, _, block) in blocks(text) {
        // (A block's lines may hold a heading after a line of text: each
        // heading line starts a unit of its own.)
        let mut lines: Vec<(u32, &str)> = Vec::new();
        let flush = |lines: &mut Vec<(u32, &str)>, units: &mut Vec<Unit>| {
            if lines.is_empty() {
                return;
            }
            let text = lines.iter().map(|(_, l)| *l).collect::<Vec<_>>().join("\n");
            units.push(Unit { tokens: tokens(&text), text, from: Some(lines[0].0), to: Some(lines[lines.len() - 1].0), heading: None, then: None, group: 0 });
            lines.clear();
        };
        for (k, line) in block.lines().enumerate() {
            let n = from + k as u32;
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            match md_heading(line).filter(|_| !fenced) {
                Some((level, words)) => {
                    flush(&mut lines, &mut units);
                    push_heading(&mut stack, level, words);
                    units.push(Unit { tokens: tokens(line), text: line.to_string(), from: Some(n), to: Some(n), heading: Some(path_of(&stack, title)), then: None, group: 0 });
                }
                None => lines.push((n, line)),
            }
        }
        flush(&mut lines, &mut units);
    }
    // Cited by section; by lines only when there are no headings.
    let noun = if units.iter().any(|u| u.heading.is_some()) { Noun::None } else { Noun::Line };
    (units, Shape::of(noun, "\n\n"))
}

/// **A Word document**: a paragraph a line (`word.rs`), each a unit; a
/// heading its `#` marks, else a line that looks like one.
fn word_units(text: &str) -> (Vec<Unit>, Shape) {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let marked = lines.iter().any(|l| md_heading(l).is_some());
    let title = titled(&lines);
    let mut stack: Vec<(usize, String)> = Vec::new();
    let units = (0..lines.len())
        .map(|i| {
            let line = lines[i];
            let heading = if marked {
                md_heading(line).map(|(level, words)| {
                    push_heading(&mut stack, level, words);
                    path_of(&stack, title)
                })
            } else {
                heading_like(line, lines.get(i + 1).copied()).then(|| line.trim().to_string())
            };
            Unit { tokens: tokens(line), text: line.to_string(), from: None, to: None, heading, then: None, group: 0 }
        })
        .collect();
    (units, Shape::of(Noun::None, "\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paragraph of about `words` words about `topic` (fictional).
    fn para(topic: &str, words: usize) -> String {
        let mut s = format!("{topic} matters here.");
        while s.split_whitespace().count() < words {
            s.push_str(&format!(" The {topic} rule applies to every member of staff at Example Ltd, and the team lead checks it each quarter."));
        }
        s
    }

    #[test]
    fn a_short_text_is_one_part_and_an_empty_one_none() {
        let parts = split("Beans in May.\n\nPeas in March.", "text");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].n, 1);
        assert_eq!(parts[0].at, "lines 1\u{2013}3");
        assert_eq!(parts[0].text, "Beans in May.\n\nPeas in March.");
        assert!(split("  \n ", "text").is_empty());
    }

    #[test]
    fn a_pdf_goes_by_its_pages_and_a_long_page_goes_on() {
        let mut text = String::new();
        for p in 1..=12 {
            text.push_str(&format!("[Page {p}]\nStaff handbook\n{}\n\n", para(&format!("Topic {p}"), 180)));
        }
        text.push_str("[Page 13, read with OCR]\nScanned notice\nThe car park closes at 7pm on Fridays from next month onwards, for resurfacing.\n\n");
        text.push_str(&format!("[Page 14]\n{}\n\n", para("Long page", 1400)));
        let parts = split(&text, "pdf");
        assert!(parts.len() >= 6, "{}", parts.len());
        assert_eq!(parts[0].at, "pages 1\u{2013}2");
        assert!(parts[0].text.starts_with("[Page 1]\n"));
        for p in &parts {
            assert!(p.tokens <= MOST + 40, "part {} is {} tokens", p.n, p.tokens);
            assert!(p.at.starts_with("page"), "{}", p.at);
        }
        // The scanned page is cited by its page too.
        assert!(parts.iter().any(|p| p.text.contains("[Page 13, read with OCR]") && p.at.contains("13")));
        // A page longer than a part: its next part says it goes on.
        let long: Vec<&Part> = parts.iter().filter(|p| p.at.contains("14")).collect();
        assert!(long.len() >= 2, "{long:?}");
        assert!(long[1].text.starts_with("[Page 14, continued]\n"), "{}", long[1].text);
        assert!(long[0].at.ends_with("14") && long.iter().skip(1).all(|p| p.at == "page 14"), "{:?}", long.iter().map(|p| &p.at).collect::<Vec<_>>());
        // "Staff handbook" tops every page: a running head, never a section.
        assert!(parts.iter().all(|p| p.section != "Staff handbook"), "{:?}", parts.iter().map(|p| &p.section).collect::<Vec<_>>());
    }

    #[test]
    fn a_pdfs_sections_are_its_heading_lines() {
        let text = format!(
            "[Page 1]\n4 Leave\n{}\n4.1 Annual leave\n{}\n\n[Page 2]\n{}\n4.2 Sick leave\n{}",
            para("Leave", 120),
            para("Annual leave", 300),
            para("Carry over", 250),
            para("Sick leave", 400)
        );
        let parts = split(&text, "pdf");
        assert_eq!(parts[0].section, "4 Leave");
        // Page 2 starts in 4.1's section (its heading was on page 1).
        let p2 = parts.iter().find(|p| p.at.starts_with("page 2")).unwrap();
        assert_eq!(p2.section, "4.1 Annual leave", "{parts:?}");
        assert!(heading_like("4.2 Sick leave", Some(&para("x", 40))));
        assert!(!heading_like("Page 3 of 60", Some(&para("x", 40))));
        assert!(!heading_like("1) Buy seeds", Some("2) Sow them in trays on the windowsill, a fortnight before the frost")));
        assert!(heading_like("1. Introduction", Some(&para("x", 40))));
        assert!(!heading_like("It rained.", Some(&para("x", 40))));
        assert!(!heading_like("Short line", Some("Also short")));
    }

    #[test]
    fn markdown_goes_by_its_headings_never_inside_code() {
        let text = format!(
            "# Staff handbook\n\nIntro.\n\n## Leave\n\n{}\n\n### Annual leave\n\n{}\n\n```\n# not a heading\n\nstill code\n```\n\n### Sick leave\n\n{}\n\n## Pay\n\n{}",
            para("Leave", 100),
            para("Annual leave", 500),
            para("Sick leave", 500),
            para("Pay", 400)
        );
        let parts = split(&text, "markdown");
        let sections: Vec<&str> = parts.iter().map(|p| p.section.as_str()).collect();
        assert!(sections.contains(&"Leave \u{203a} Annual leave"), "{sections:?}");
        assert!(sections.contains(&"Leave \u{203a} Sick leave"), "{sections:?}");
        assert!(sections.contains(&"Pay"), "{sections:?}");
        assert!(!sections.iter().any(|s| s.contains("not a heading")));
        // A heading never ends a part: it starts the one it heads.
        assert!(parts.iter().all(|p| md_heading(p.text.lines().last().unwrap()).is_none()), "{parts:?}");
        // Cited by section: no lines.
        assert!(parts.iter().all(|p| p.at.is_empty()));
        let annual = parts.iter().find(|p| p.section.ends_with("Annual leave")).unwrap();
        assert_eq!(annual.place(), "section \u{201c}Leave \u{203a} Annual leave\u{201d}");
        // The fence stays whole in one part.
        assert!(parts.iter().any(|p| p.text.contains("```\n# not a heading\n\nstill code\n```")));
    }

    #[test]
    fn slides_go_by_their_numbers_their_titles_the_sections() {
        let mut text = String::new();
        for s in 1..=9 {
            text.push_str(&format!("Slide {s}: Quarter {s} plan\n{}\nSpeaker notes: Mention the dates.\n\n", para(&format!("Slide {s}"), 160)));
        }
        let parts = split(text.trim_end(), "powerpoint");
        assert!(parts.len() >= 3);
        // (About 270 tokens a slide: two a part, a new part at a slide once it has half.)
        assert_eq!(parts[0].at, "slides 1\u{2013}2");
        assert_eq!(parts[0].section, "Quarter 1 plan");
        assert_eq!(parts[0].place(), "slides 1\u{2013}2, \u{201c}Quarter 1 plan\u{201d}");
        assert!(parts[1].text.starts_with("Slide 3: Quarter 3 plan"), "{}", parts[1].text);
        assert_eq!(parts[1].section, "Quarter 3 plan");
    }

    #[test]
    fn a_sheets_rows_go_in_parts_each_with_its_head() {
        let mut text = String::from("An Excel workbook with 2 sheets: \"Orders\", \"Notes\". Values as the file formats them; formulas' results calculated on this PC.\n\n## Sheet 1 of 2: \"Orders\"\nUsed range A1:C400 (400 rows × 3 columns).\n\n| | A | B | C |\n|---|---|---|---|");
        for r in 1..=400 {
            text.push_str(&format!("\n| {r} | Order {r} for the garden centre | {} | Paid |", r * 3));
        }
        text.push_str("\n\nFormulas (results above calculated on this PC):\nB2:B400: =A2*3 (the same in each row, filled down)");
        text.push_str("\n\n## Sheet 2 of 2: \"Notes\"\nUsed range A1:A2 (2 rows × 1 columns).\n\n| | A |\n|---|---|\n| 1 | Ring the supplier |\n| 2 | Check the van |");
        let parts = split(&text, "sheet");
        let orders: Vec<&Part> = parts.iter().filter(|p| p.at.starts_with("sheet \u{201c}Orders\u{201d}, rows")).collect();
        assert!(orders.len() >= 3, "{:?}", parts.iter().map(|p| &p.at).collect::<Vec<_>>());
        assert_eq!(orders[0].at, format!("sheet \u{201c}Orders\u{201d}, rows 1\u{2013}{}", orders[0].text.lines().filter(|l| ROW.is_match(l)).count()));
        // Each starts with the book's line, the sheet's and the table's head.
        for p in &orders {
            assert!(p.text.starts_with("An Excel workbook with 2 sheets"), "{}", p.text);
            assert!(p.text.contains("## Sheet 1 of 2: \"Orders\"") && p.text.contains("| | A | B | C |\n|---|---|---|---|\n| "), "{}", p.text);
        }
        // Every row is in one part, once.
        let rows: usize = orders.iter().map(|p| p.text.lines().filter(|l| ROW.is_match(l)).count()).sum();
        assert_eq!(rows, 400);
        // The formulas: a part of their own, never with another sheet's rows.
        let f = parts.iter().find(|p| p.at == "sheet \u{201c}Orders\u{201d}, formulas").expect("the formulas' part");
        assert!(f.text.starts_with("## Sheet 1 of 2: \"Orders\"\nFormulas"), "{}", f.text);
        let notes = parts.iter().find(|p| p.at.starts_with("sheet \u{201c}Notes\u{201d}")).unwrap();
        assert_eq!(notes.at, "sheet \u{201c}Notes\u{201d}, rows 1\u{2013}2");
        assert!(!notes.text.contains("Order 1 "));
    }

    #[test]
    fn a_csvs_one_table_is_cited_by_rows() {
        let mut text = String::from("One table; its values as written in the file.\n\n300 rows × 2 columns (A1:B300).\n\n| | A | B |\n|---|---|---|");
        for r in 1..=300 {
            text.push_str(&format!("\n| {r} | Plot {r} on the north allotment site | {r} m² |"));
        }
        let parts = split(&text, "sheet");
        assert!(parts.len() >= 2);
        assert!(parts[0].at.starts_with("rows 1\u{2013}"), "{}", parts[0].at);
        assert!(parts[1].text.starts_with("One table; its values as written in the file.\n\n300 rows × 2 columns (A1:B300).\n\n| | A | B |\n|---|---|---|\n| "), "{}", parts[1].text);
    }

    #[test]
    fn a_word_documents_marked_headings_are_its_sections() {
        let text = format!("# Leave policy\n{}\n## Annual leave\n{}\n{}\n## Parental leave\n{}", para("Policy", 60), para("Annual", 400), para("Carry", 300), para("Parental", 500));
        let parts = split(&text, "word");
        let sections: Vec<&str> = parts.iter().map(|p| p.section.as_str()).collect();
        assert!(sections.contains(&"Annual leave") && sections.contains(&"Parental leave"), "{sections:?}");
        assert!(parts.iter().all(|p| p.at.is_empty()));
        // A Title over several Heading 1s: each section's path keeps its own heading.
        let guide = format!("# Facilities guide\n# 9. Lockers\n{}\n## In practice\n{}\n# 10. Showers\n{}\n## In practice\n{}", para("Lockers", 450), para("Keys", 300), para("Showers", 450), para("Towels", 300));
        let sections: Vec<String> = split(&guide, "word").into_iter().map(|p| p.section).collect();
        assert!(sections.iter().any(|s| s == "10. Showers \u{203a} In practice"), "{sections:?}");
        assert!(sections.iter().any(|s| s == "9. Lockers"), "{sections:?}");
        // No marks (an older .doc): lines that look like headings.
        let plain = format!("Leave policy\n{}\nAnnual leave\n{}", para("Policy", 400), para("Annual", 500));
        let parts = split(&plain, "word");
        assert_eq!(parts[0].section, "Leave policy");
        assert!(parts.iter().any(|p| p.section == "Annual leave"));
    }

    #[test]
    fn code_goes_by_lines_and_every_part_stays_in_size() {
        let mut text = String::new();
        for f in 0..40 {
            text.push_str(&format!("fn step_{f}() {{\n    let x = {f};\n    println!(\"{{x}} seeds sown in bed {f}\");\n    // Water the bed after sowing, then mark the label.\n}}\n\n"));
        }
        let parts = split(&text, "code");
        assert!(parts.len() >= 2);
        assert!(parts[0].at.starts_with("lines 1\u{2013}"));
        assert!(parts.iter().all(|p| p.tokens <= MOST));
        // Everything's somewhere, in order.
        let joined: String = parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n\n");
        assert_eq!(joined.matches("fn step_").count(), 40);
        // One line far too long: cut at its words, never lost.
        let long = "word ".repeat(5_000);
        let parts = split(&long, "text");
        assert!(parts.len() >= 4 && parts.iter().all(|p| p.tokens <= MOST));
        assert_eq!(parts.iter().map(|p| p.text.matches("word").count()).sum::<usize>(), 5_000);
    }
}
