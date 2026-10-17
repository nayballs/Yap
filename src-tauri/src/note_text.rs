//! Reading, searching and editing a note's text precisely: the pure half of
//! the AI Chat's `read_note`, `search_in_note` and `edit_note` tools
//! (`tools.rs`). The behaviour follows the note tools of Wispr Flow's live
//! "Ask anything" (a bounded read, a search with context, a replacement that
//! must match exactly once); the code and wording are Yap's own.
//!
//! A note has four parts here ([`Field`]): its **content** (the notes the
//! person typed or dictated), its **enhanced** summary (the Enhanced tab /
//! the meeting's action plan), its **title**, and a meeting's
//! **transcript**: talk only (no echo, no "You dictated here" markers), one
//! `[m:ss] You: …` line per segment, and read-only.
//!
//! **Matching an edit.** `old_text` must appear exactly once, byte for byte,
//! with one allowance: a line break is a line break, `\r\n` or `\n` (notes
//! typed in Yap have `\n`; ones that came in through the local API may have
//! `\r\n`), and the replacement takes the text's own line endings. Anything
//! else (spaces, tabs, letter case, curly quotes, accents) has to be exact,
//! so an edit never lands somewhere that only looks alike. When nothing
//! matches, the miss lists the places that differ only in spacing, line
//! breaks, case or quote marks, quoted exactly as the note has them, so the
//! model can send that text instead. Searching is the forgiving kind (all of
//! those fold) and always returns the note's own text.

use serde::Serialize;

use crate::meeting_summary::estimate_tokens;
use crate::notes::Note;

/// Lines of context before and after a search hit.
const CONTEXT_LINES: usize = 2;
/// The first this many hits get context lines; the rest, their line alone.
const CONTEXT_HITS: usize = 5;
/// A search returns at most this many hits…
pub const MAX_HITS: usize = 20;
/// …and stops adding them past about this many characters.
pub const SEARCH_CHARS: usize = 4_000;
/// A hit on a line longer than this shows the part around the match.
const LINE_CHARS: usize = 300;
/// Characters kept either side of the match in that part.
const EXCERPT_SIDE: usize = 120;
/// A context line is cut after this many characters.
const CONTEXT_CHARS: usize = 160;
/// `read_note` pages: this many lines unless asked otherwise, at most…
pub const DEFAULT_PAGE_LINES: usize = 200;
pub const MAX_PAGE_LINES: usize = 400;
/// …and at most this many tokens, the bound `get_note` already puts on a
/// meeting (`meeting_summary::ask_context`), so one page can't fill a
/// small model's context.
pub const PAGE_TOKENS: usize = 3_000;
/// Lines of an edit shown back to the model, around the change.
const AROUND_LINES: usize = 12;
/// "Did you mean" places listed when an edit's text isn't found.
const NEAR_MAX: usize = 3;

// ---- Fields ----

/// A part of a note the chat's tools can read (all four) or edit (not the
/// transcript).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Content,
    Enhanced,
    Transcript,
    Title,
}

impl Field {
    /// The tool argument, with the names a model is likely to use instead.
    pub fn parse(s: &str) -> Option<Field> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "content" | "notes" | "note" | "raw" | "body" | "text" | "my_thoughts" => {
                Some(Field::Content)
            }
            "enhanced" | "enhanced_content" | "summary" | "ai_summary" | "action_plan" => {
                Some(Field::Enhanced)
            }
            "transcript" => Some(Field::Transcript),
            "title" | "name" => Some(Field::Title),
            _ => None,
        }
    }

    /// The tool argument's canonical value.
    pub fn name(self) -> &'static str {
        match self {
            Field::Content => "content",
            Field::Enhanced => "enhanced",
            Field::Transcript => "transcript",
            Field::Title => "title",
        }
    }

    /// What the person calls it (chips, messages).
    pub fn label(self) -> &'static str {
        match self {
            Field::Content => "notes",
            Field::Enhanced => "summary",
            Field::Transcript => "transcript",
            Field::Title => "title",
        }
    }
}

/// The field's text as the tools see it.
pub fn field_text(note: &Note, field: Field) -> String {
    match field {
        Field::Content => note.content.clone(),
        Field::Enhanced => note.enhanced_content.clone(),
        Field::Title => note.title.clone(),
        Field::Transcript => transcript_text(note),
    }
}

/// A meeting's talk, one line per segment, timed from the first segment
/// (as the notepad's timer counts): `[12:04] Them: …`.
fn transcript_text(note: &Note) -> String {
    let t0 = note.transcript.first().map(|s| s.ts).unwrap_or(0);
    note.transcript
        .iter()
        .filter(|s| s.is_talk() && !s.text.trim().is_empty())
        .map(|s| {
            let who = if s.source == "you" { "You" } else { "Them" };
            let text = s.text.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "[{}] {who}: {text}",
                crate::meeting_summary::clock(s.ts.saturating_sub(t0))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `12| text`: how the tools show a line (the number isn't part of the note).
fn numbered(n: usize, line: &str) -> String {
    format!("{n}| {line}")
}

/// Byte offset where each line starts (line 1 at 0).
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect()
}

/// The 1-based line holding byte `at`.
fn line_at(starts: &[usize], at: usize) -> usize {
    starts.partition_point(|&s| s <= at).max(1)
}

/// The first `n` chars of `s`, and whether that cut it.
fn first_chars(s: &str, n: usize) -> (&str, bool) {
    match s.char_indices().nth(n) {
        Some((i, _)) => (&s[..i], true),
        None => (s, false),
    }
}

/// A context line: cut after `CONTEXT_CHARS` chars, marked with "…".
fn shorten(s: &str, n: usize) -> String {
    match first_chars(s, n) {
        (head, true) => format!("{head}…"),
        (all, false) => all.to_string(),
    }
}

/// Byte offset `n` chars before `end` in `s` (or 0).
fn back_chars(s: &str, end: usize, n: usize) -> usize {
    if n == 0 {
        return end;
    }
    s[..end]
        .char_indices()
        .rev()
        .nth(n - 1)
        .map_or(0, |(i, _)| i)
}

/// Byte offset `n` chars after `start` in `s` (or its end).
fn fwd_chars(s: &str, start: usize, n: usize) -> usize {
    s[start..]
        .char_indices()
        .nth(n)
        .map_or(s.len(), |(i, _)| start + i)
}

/// The longest start of `s` that fits `budget` tokens (`estimate_tokens`'
/// counting, char by char).
fn take_tokens(s: &str, budget: usize) -> &str {
    let (mut ascii, mut other) = (0usize, 0usize);
    for (i, c) in s.char_indices() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
        if (ascii * 2).div_ceil(7) + other > budget {
            return &s[..i];
        }
    }
    s
}

/// `text` within `budget` tokens (whole lines while they fit, else the start
/// of the first), and whether it was cut. `get_note` bounds a long note's
/// content and summary this way; `read_note` reads the rest.
pub fn fit(text: &str, budget: usize) -> (String, bool) {
    if estimate_tokens(text) <= budget {
        return (text.to_string(), false);
    }
    let mut out = String::new();
    let mut used = 0;
    for line in text.lines() {
        let t = estimate_tokens(line) + 1;
        if used + t > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
        used += t;
    }
    if out.is_empty() {
        out = take_tokens(text, budget).to_string();
    }
    (out.trim_end().to_string(), true)
}

// ---- Matching ----

/// How [`view`] spells a text for matching.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fold {
    /// Only `\r\n` → `\n`.
    Exact,
    /// Also lower case, every run of whitespace (line breaks, tabs,
    /// no-break spaces…) one space, curly quotes straight, dashes `-`,
    /// "…" three dots.
    Loose,
}

/// A text re-spelled for matching, remembering where each byte came from.
struct View {
    text: String,
    /// For each byte of `text`: the byte range, in the original, of the
    /// char (or whitespace run, or `\r\n`) it came from.
    from: Vec<(usize, usize)>,
}

impl View {
    fn push(&mut self, c: char, range: (usize, usize)) {
        self.text.push(c);
        for _ in 0..c.len_utf8() {
            self.from.push(range);
        }
    }
}

/// `c` as [`Fold::Loose`] spells it.
fn fold_loose(c: char, out: &mut Vec<char>) {
    for l in c.to_lowercase() {
        match l {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => out.push('"'),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => out.push('-'),
            '\u{2026}' => out.extend(['.', '.', '.']),
            _ => out.push(l),
        }
    }
}

fn view(src: &str, fold: Fold) -> View {
    let mut v = View {
        text: String::with_capacity(src.len()),
        from: Vec::with_capacity(src.len()),
    };
    let mut space: Option<(usize, usize)> = None;
    let mut chars = src.char_indices().peekable();
    let mut folded = Vec::new();
    while let Some((i, c)) = chars.next() {
        let mut end = i + c.len_utf8();
        let c = if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
            end += 1;
            chars.next();
            '\n'
        } else {
            c
        };
        if fold == Fold::Exact {
            v.push(c, (i, end));
            continue;
        }
        if c.is_whitespace() {
            space = Some(space.map_or((i, end), |(s, _)| (s, end)));
            continue;
        }
        if let Some(run) = space.take() {
            v.push(' ', run);
        }
        folded.clear();
        fold_loose(c, &mut folded);
        for &f in &folded {
            v.push(f, (i, end));
        }
    }
    if let Some(run) = space {
        v.push(' ', run);
    }
    v
}

/// A search query (or an edit's text, for the "did you mean") as
/// [`Fold::Loose`] spells it, without spaces at either end.
fn loose_pattern(s: &str) -> String {
    view(s, Fold::Loose).text.trim_matches(' ').to_string()
}

/// Every place `pat` appears in `v`, overlapping ones too ("aa" is twice in
/// "aaa": an edit there would be ambiguous), as byte ranges of the original.
fn occurrences(v: &View, pat: &str) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    if pat.is_empty() {
        return out;
    }
    let mut at = 0;
    while at < v.text.len() {
        let Some(p) = v.text[at..].find(pat) else {
            break;
        };
        let s = at + p;
        let range = (v.from[s].0, v.from[s + pat.len() - 1].1);
        if out.last() != Some(&range) {
            out.push(range);
        }
        at = s + v.text[s..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Line endings a text uses: `\r\n` when most of its line breaks are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Eol {
    Lf,
    Crlf,
}

fn eol_style(text: &str) -> Eol {
    let crlf = text.matches("\r\n").count();
    let bare = text.matches('\n').count() - crlf;
    if crlf > bare {
        Eol::Crlf
    } else {
        Eol::Lf
    }
}

fn with_eol(s: &str, eol: Eol) -> String {
    let lf = s.replace("\r\n", "\n");
    match eol {
        Eol::Lf => lf,
        Eol::Crlf => lf.replace('\n', "\r\n"),
    }
}

// ---- Searching ----

/// One line (or a few, for a match across a line break) where a search
/// matched.
#[derive(Debug, Serialize)]
pub struct Hit {
    pub field: &'static str,
    pub line: usize,
    /// The last line, when the match runs across lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_line: Option<usize>,
    /// The note's own text: the whole line, or (`partial`) the part around
    /// the match of a very long one.
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// How many matches start on this line, when more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times: Option<usize>,
    /// The lines around it, numbered (the first few hits only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
}

/// A search's result: the hits shown and how many lines matched per field.
#[derive(Debug, Default)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub lines: Vec<(Field, usize)>,
}

impl Found {
    /// Matching lines in every field searched, shown or not.
    pub fn total(&self) -> usize {
        self.lines.iter().map(|(_, n)| n).sum()
    }

    pub fn more(&self) -> bool {
        self.total() > self.hits.len()
    }
}

/// Find `query` in `fields` of `note` (in that order), forgivingly: letter
/// case, runs of whitespace (line breaks too), curly quotes and dashes all
/// fold. At most [`MAX_HITS`] hits and about [`SEARCH_CHARS`] characters.
pub fn search(note: &Note, query: &str, fields: &[Field]) -> Found {
    let pat = loose_pattern(query);
    let mut found = Found::default();
    let mut used = 0;
    let mut full = false;
    for &field in fields {
        let text = field_text(note, field);
        let ranges = occurrences(&view(&text, Fold::Loose), &pat);
        let starts = line_starts(&text);
        let lines: Vec<&str> = text.lines().collect();
        // One hit per line a match starts on.
        let mut groups: Vec<(usize, (usize, usize), usize)> = Vec::new();
        for r in ranges {
            let line = line_at(&starts, r.0);
            match groups.last_mut() {
                Some(g) if g.0 == line => g.2 += 1,
                _ => groups.push((line, r, 1)),
            }
        }
        found.lines.push((field, groups.len()));
        for (line, (s, e), times) in groups {
            if full {
                break;
            }
            let to = line_at(&starts, e - 1).max(line);
            let (excerpt, partial) = excerpt(&text, &starts, &lines, line, to, (s, e));
            let context = if found.hits.len() < CONTEXT_HITS {
                context_lines(&lines, line, to)
            } else {
                Vec::new()
            };
            let size = excerpt.len() + context.iter().map(|c| c.len() + 4).sum::<usize>() + 48;
            if !found.hits.is_empty() && used + size > SEARCH_CHARS {
                full = true;
                break;
            }
            used += size;
            found.hits.push(Hit {
                field: field.name(),
                line,
                to_line: (to > line).then_some(to),
                text: excerpt,
                partial,
                times: (times > 1).then_some(times),
                context,
            });
            full = found.hits.len() >= MAX_HITS;
        }
    }
    found
}

/// The text a hit shows: its line(s) whole, or the part around the match of
/// a line longer than [`LINE_CHARS`].
fn excerpt(
    text: &str,
    starts: &[usize],
    lines: &[&str],
    line: usize,
    to: usize,
    (s, e): (usize, usize),
) -> (String, bool) {
    if line == to {
        let row = lines.get(line - 1).copied().unwrap_or("");
        if row.chars().count() <= LINE_CHARS {
            return (row.to_string(), false);
        }
        let base = starts[line - 1];
        let rs = (s - base).min(row.len());
        let re = (e - base).min(row.len());
        let a = back_chars(row, rs, EXCERPT_SIDE);
        let b = fwd_chars(row, re, EXCERPT_SIDE);
        return (row[a..b].to_string(), a > 0 || b < row.len());
    }
    let block = lines[line - 1..to.min(lines.len())].join("\n");
    if block.chars().count() <= 2 * LINE_CHARS {
        return (block, false);
    }
    let matched = text[s..e].replace("\r\n", "\n");
    let (head, _) = first_chars(&matched, 2 * LINE_CHARS);
    (head.to_string(), true)
}

fn context_lines(lines: &[&str], line: usize, to: usize) -> Vec<String> {
    let before = line.saturating_sub(CONTEXT_LINES).max(1)..line;
    let after = to + 1..=(to + CONTEXT_LINES).min(lines.len());
    before
        .chain(after)
        .map(|n| numbered(n, &shorten(lines[n - 1], CONTEXT_CHARS)))
        .collect()
}

// ---- Reading ----

/// A page of a field, from `read_note`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Page {
    pub from: usize,
    pub to: usize,
    pub total_lines: usize,
    pub more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    /// A line longer than a whole page, shown only up to the page's end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut_line: Option<usize>,
    /// The lines, numbered (`12| text`).
    pub text: String,
}

/// Lines `offset..` of `text` (1 = the first), at most `limit` of them
/// (≤ [`MAX_PAGE_LINES`]) and `budget` tokens. An empty text is an empty
/// page; an offset past the end is an error.
pub fn read_page(text: &str, offset: usize, limit: usize, budget: usize) -> Result<Page, String> {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return Ok(Page {
            from: 0,
            to: 0,
            total_lines: 0,
            more: false,
            next_offset: None,
            cut_line: None,
            text: String::new(),
        });
    }
    let from = offset.max(1);
    if from > total {
        return Err(format!(
            "offset {from} is past the end: there are {total} lines"
        ));
    }
    let limit = limit.clamp(1, MAX_PAGE_LINES);
    let (mut out, mut used, mut to, mut cut_line) = (String::new(), 0, from - 1, None);
    for (n, line) in lines
        .iter()
        .enumerate()
        .skip(from - 1)
        .take(limit)
        .map(|(i, l)| (i + 1, l))
    {
        let row = numbered(n, line);
        let t = estimate_tokens(&row) + 1;
        if used + t > budget {
            if out.is_empty() {
                // One line longer than a page: its start, and say so.
                out = take_tokens(&row, budget).to_string();
                to = n;
                cut_line = Some(n);
            }
            break;
        }
        out.push_str(&row);
        out.push('\n');
        used += t;
        to = n;
    }
    let more = to < total;
    Ok(Page {
        from,
        to,
        total_lines: total,
        more,
        next_offset: more.then_some(to + 1),
        cut_line,
        text: out.trim_end_matches('\n').to_string(),
    })
}

// ---- Editing ----

/// A replacement made by [`replace_once`].
#[derive(Debug, PartialEq)]
pub struct Replaced {
    /// The whole text afterwards.
    pub text: String,
    /// Where the new text starts in it (bytes).
    pub at: usize,
    /// What was replaced, exactly as it was.
    pub old: String,
    /// What went in, in the text's line endings.
    pub new: String,
}

/// Why [`replace_once`] changed nothing.
#[derive(Debug, PartialEq)]
pub enum Miss {
    /// Not there. `near`: up to three places that differ from it only in
    /// spacing, line breaks, letter case or quote marks (line, the text's own
    /// words). `numbered`: it starts with line numbers (`12| `), copied from
    /// a read or a search.
    NotFound {
        near: Vec<(usize, String)>,
        numbered: bool,
    },
    /// There more than once: how many times, and on which lines (the first
    /// ten).
    Ambiguous { times: usize, lines: Vec<usize> },
    /// `old` is empty, but the text isn't: only an empty text can be filled.
    NotEmpty,
}

/// Replace the one place `old` appears in `text` with `new`. Exact, but for
/// line breaks (`\r\n` = `\n`, and `new` takes the text's own); `old` = ""
/// fills an empty (or blank) text.
pub fn replace_once(text: &str, old: &str, new: &str) -> Result<Replaced, Miss> {
    if old.is_empty() {
        if !text.trim().is_empty() {
            return Err(Miss::NotEmpty);
        }
        let new = with_eol(new, Eol::Lf);
        return Ok(Replaced {
            text: new.clone(),
            at: 0,
            old: text.to_string(),
            new,
        });
    }
    let exact = occurrences(&view(text, Fold::Exact), &old.replace("\r\n", "\n"));
    match exact.as_slice() {
        [] => {
            let unnumbered = strip_line_numbers(old);
            let near = near_misses(text, unnumbered.as_deref().unwrap_or(old));
            Err(Miss::NotFound {
                near,
                numbered: unnumbered.is_some(),
            })
        }
        [(s, e)] => {
            let new = with_eol(new, eol_style(text));
            let mut out = String::with_capacity(text.len() + new.len());
            out.push_str(&text[..*s]);
            out.push_str(&new);
            out.push_str(&text[*e..]);
            Ok(Replaced {
                text: out,
                at: *s,
                old: text[*s..*e].to_string(),
                new,
            })
        }
        many => {
            let starts = line_starts(text);
            let mut lines: Vec<usize> = many.iter().map(|r| line_at(&starts, r.0)).collect();
            lines.dedup();
            lines.truncate(10);
            Err(Miss::Ambiguous {
                times: many.len(),
                lines,
            })
        }
    }
}

/// The places in `text` that match `old` once spacing, line breaks, case and
/// quote marks are ignored, as the text has them.
fn near_misses(text: &str, old: &str) -> Vec<(usize, String)> {
    let starts = line_starts(text);
    let mut near: Vec<(usize, String)> = Vec::new();
    for (s, e) in occurrences(&view(text, Fold::Loose), &loose_pattern(old)) {
        let found = text[s..e].to_string();
        if !near.iter().any(|(_, t)| *t == found) {
            near.push((line_at(&starts, s), found));
        }
        if near.len() == NEAR_MAX {
            break;
        }
    }
    near
}

/// `old` without the `12| ` line numbers `read_note` and `search_in_note`
/// show, when every line has one; else None.
fn strip_line_numbers(old: &str) -> Option<String> {
    fn strip(line: &str) -> Option<&str> {
        let rest = line.trim_start();
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return None;
        }
        let rest = rest[digits..].strip_prefix('|')?;
        Some(rest.strip_prefix(' ').unwrap_or(rest))
    }
    let lines: Vec<&str> = old.split('\n').collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut any = false;
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            out.push(line);
            continue;
        }
        out.push(strip(line)?);
        any = true;
    }
    any.then(|| out.join("\n"))
}

/// An edit made by [`apply_edit`].
#[derive(Debug)]
pub struct Edit {
    pub field: Field,
    /// The text replaced, as the note had it ("" or blank when filled).
    pub old: String,
    /// The text that went in.
    pub new: String,
    /// An empty field was filled (`old_text` "").
    pub filled: bool,
    /// Whether the note changed (not when `new` was the same as `old`).
    pub changed: bool,
    /// The line the change starts on, afterwards.
    pub line: usize,
    /// The changed lines afterwards with a line either side, numbered.
    pub now: String,
}

/// Why [`apply_edit`] changed nothing.
#[derive(Debug, PartialEq)]
pub enum EditError {
    /// The transcript is what was said.
    ReadOnly,
    /// A title is one line.
    TitleLines,
    Miss(Miss),
}

impl EditError {
    /// What the model is told.
    pub fn message(&self, field: Field) -> String {
        let label = field.label();
        match self {
            EditError::ReadOnly => "The transcript is what was said in the meeting, so it can't be edited. Edit the notes (field \"content\") or the summary (field \"enhanced\") instead.".to_string(),
            EditError::TitleLines => "A title is one line: new_text can't contain a line break. Nothing was changed.".to_string(),
            EditError::Miss(Miss::NotEmpty) => format!(
                "old_text is empty, but the {label} already has text, so nothing was changed. An empty old_text only fills an empty field. To add text, use a line that's already there as old_text and that line plus the addition as new_text."
            ),
            EditError::Miss(Miss::Ambiguous { times, lines }) => {
                let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
                format!(
                    "old_text appears {times} times in the {label} (lines {}), so nothing was changed. Include more of the text around the place you mean, so that old_text appears only once.",
                    lines.join(", ")
                )
            }
            EditError::Miss(Miss::NotFound { near, numbered }) => {
                let mut msg = format!(
                    "old_text wasn't found in the {label}, so nothing was changed. It has to be copied exactly from the note: same spaces, letter case and punctuation."
                );
                if *numbered {
                    msg.push_str(" Leave out the line numbers (\"12| \") that read_note and search_in_note show in front of each line.");
                }
                if near.is_empty() {
                    msg.push_str(" Use search_in_note with a few distinctive words of it to find the exact text.");
                } else {
                    msg.push_str(" The note has text that differs from it only in spacing, line breaks, letter case or quote marks (listed in `near`); if that's what you meant, send it exactly as old_text.");
                }
                msg
            }
        }
    }

    /// A few words for the chat's activity chip.
    pub fn short(&self) -> String {
        match self {
            EditError::ReadOnly => "the transcript can't be edited".to_string(),
            EditError::TitleLines => "a title is one line".to_string(),
            EditError::Miss(Miss::NotEmpty) => "it already has text".to_string(),
            EditError::Miss(Miss::Ambiguous { times, .. }) => {
                format!("the text is in {times} places")
            }
            EditError::Miss(Miss::NotFound { .. }) => "the text wasn't found".to_string(),
        }
    }
}

/// Replace `old` with `new` in `field` of `note` ([`replace_once`]'s rules),
/// keeping the note's bookkeeping right: a new title is the person's (the AI
/// meeting title keeps off it), a content edit leaves the summary to show as
/// stale, and a summary written into an empty field counts as made from the
/// notes as they are now (one emptied is no summary at all). Changes nothing
/// on an error. `updated_ts` is the caller's (`notes::modify`).
pub fn apply_edit(note: &mut Note, field: Field, old: &str, new: &str) -> Result<Edit, EditError> {
    if field == Field::Transcript {
        return Err(EditError::ReadOnly);
    }
    let current = field_text(note, field);
    let r = replace_once(&current, old, new).map_err(EditError::Miss)?;
    if field == Field::Title && r.text.contains(['\n', '\r']) {
        return Err(EditError::TitleLines);
    }
    let changed = r.text != current;
    if changed {
        match field {
            Field::Content => note.content = r.text.clone(),
            Field::Title => {
                note.title = r.text.clone();
                note.title_auto = false;
            }
            Field::Enhanced => {
                if r.text.trim().is_empty() {
                    note.enhanced_content.clear();
                    note.enhanced_at_hash.clear();
                } else {
                    if current.trim().is_empty() {
                        note.enhanced_at_hash = crate::notes::content_hash(&note.content);
                    }
                    note.enhanced_content = r.text.clone();
                }
            }
            Field::Transcript => {} // refused above
        }
    }
    let starts = line_starts(&r.text);
    let line = line_at(&starts, r.at);
    let last = line_at(&starts, (r.at + r.new.len()).saturating_sub(1).max(r.at));
    Ok(Edit {
        field,
        old: r.old,
        new: r.new,
        filled: old.is_empty(),
        changed,
        line,
        now: around(&r.text, line, last),
    })
}

/// Lines `line..=last` of `text` with one either side, numbered, at most
/// [`AROUND_LINES`] of them.
fn around(text: &str, line: usize, last: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let first = line.saturating_sub(1).max(1);
    let end = (last + 1).min(lines.len()).min(first + AROUND_LINES - 1);
    (first..=end)
        .filter_map(|n| {
            lines
                .get(n - 1)
                .map(|l| numbered(n, &shorten(l, LINE_CHARS)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What an edit changed, for the chat's chip: the words of `old` and `new`
/// without the words they share at either end, on one line, each at most 40
/// chars. ("Ship on Friday." → "Ship on Monday." gives "Friday." and
/// "Monday."; an insertion gives an empty `old`, a deletion an empty `new`.)
pub fn change_preview(old: &str, new: &str) -> (String, String) {
    let o: Vec<&str> = old.split_whitespace().collect();
    let n: Vec<&str> = new.split_whitespace().collect();
    let p = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let s = o[p..]
        .iter()
        .rev()
        .zip(n[p..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let tidy = |words: &[&str]| shorten(&words.join(" "), 39);
    (tidy(&o[p..o.len() - s]), tidy(&n[p..n.len() - s]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;
    use serde_json::json;

    fn note(content: &str) -> Note {
        let mut n: Note = serde_json::from_value(json!({
            "id": 1, "title": "Launch plan", "createdTs": 1, "updatedTs": 1
        }))
        .unwrap();
        n.content = content.to_string();
        n
    }

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.into(),
            text: text.into(),
            ts,
            echo: false,
            dictated: false,
        }
    }

    // ---- replace_once ----

    #[test]
    fn a_unique_match_is_replaced_and_nothing_else_moves() {
        let text = "Ship the beta on Friday.\nAsk Dave about the venue.\n";
        let r = replace_once(text, "on Friday", "on Monday").unwrap();
        assert_eq!(
            r.text,
            "Ship the beta on Monday.\nAsk Dave about the venue.\n"
        );
        assert_eq!(r.old, "on Friday");
        assert_eq!(r.new, "on Monday");
        assert_eq!(&r.text[r.at..r.at + r.new.len()], "on Monday");
        // Deleting: an empty new_text.
        let r = replace_once(text, "Ask Dave about the venue.\n", "").unwrap();
        assert_eq!(r.text, "Ship the beta on Friday.\n");
    }

    #[test]
    fn several_matches_are_refused_with_their_lines() {
        let text = "Ship the beta.\nDocs.\nShip the beta docs.";
        assert_eq!(
            replace_once(text, "Ship the beta", "x"),
            Err(Miss::Ambiguous {
                times: 2,
                lines: vec![1, 3]
            })
        );
        // Twice on one line: one line, two times.
        assert_eq!(
            replace_once("to do, to do", "to do", "x"),
            Err(Miss::Ambiguous {
                times: 2,
                lines: vec![1]
            })
        );
        // Overlapping counts too: "aa" could be either half of "aaa".
        assert_eq!(
            replace_once("aaa", "aa", "b"),
            Err(Miss::Ambiguous {
                times: 2,
                lines: vec![1]
            })
        );
        // More text around it makes it unique.
        let r = replace_once(text, "Ship the beta docs", "Ship the docs").unwrap();
        assert_eq!(r.text, "Ship the beta.\nDocs.\nShip the docs.");
    }

    #[test]
    fn whitespace_only_differences_never_match_silently() {
        let cases = [
            ("Ship  the fix", "Ship the fix"),     // a double space
            ("Ship\tthe fix", "Ship the fix"),     // a tab
            ("Ship\u{a0}the fix", "Ship the fix"), // a no-break space
            ("Ship the\nfix", "Ship the fix"),     // a line break
            ("Ship the fix", "Ship the fix\n"),    // a break the note doesn't have
            ("Ship the fix", " Ship  the fix"),    // spaces the note doesn't have
        ];
        for (text, old) in cases {
            match replace_once(text, old, "X") {
                Err(Miss::NotFound { near, numbered }) => {
                    assert!(!numbered, "{text:?}");
                    // …but the error points at the note's own text.
                    assert_eq!(
                        near,
                        vec![(1, text.trim().to_string())],
                        "{text:?} / {old:?}"
                    );
                }
                other => panic!("{text:?} / {old:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn case_and_quote_marks_must_match_too_but_are_pointed_out() {
        assert_eq!(
            replace_once("Don’t forget Café Rouge", "don't forget café rouge", "x"),
            Err(Miss::NotFound {
                near: vec![(1, "Don’t forget Café Rouge".to_string())],
                numbered: false,
            })
        );
        // Nothing alike: no suggestions.
        assert_eq!(
            replace_once("Ship the fix", "Order pizza", "x"),
            Err(Miss::NotFound {
                near: vec![],
                numbered: false
            })
        );
    }

    #[test]
    fn line_breaks_match_either_way_and_keep_the_notes_own() {
        // A CRLF note, an LF edit: matched, and the new text gets CRLF.
        let crlf = "Agenda\r\n- budget\r\n- venue\r\n";
        let r = replace_once(crlf, "- budget\n- venue", "- budget\n- venue\n- catering").unwrap();
        assert_eq!(r.text, "Agenda\r\n- budget\r\n- venue\r\n- catering\r\n");
        assert_eq!(r.old, "- budget\r\n- venue");
        // An edit that starts at a line break takes the whole "\r\n".
        let r = replace_once("a\r\nb", "\nb", "\nc").unwrap();
        assert_eq!(r.text, "a\r\nc");
        // An LF note, a CRLF edit: the note stays LF.
        let lf = "Agenda\n- budget\n- venue";
        let r = replace_once(lf, "- budget\r\n- venue", "- budget\r\n- rooms").unwrap();
        assert_eq!(r.text, "Agenda\n- budget\n- rooms");
        // Mixed endings: a line break is a line break wherever it is.
        let mixed = "one\r\ntwo\nthree";
        let r = replace_once(mixed, "one\ntwo\nthree", "1\n2\n3").unwrap();
        assert_eq!(r.text, "1\n2\n3");
        // A lone "\r" is not a line break.
        assert!(matches!(
            replace_once("a\rb", "a\nb", "x"),
            Err(Miss::NotFound { .. })
        ));
    }

    #[test]
    fn unicode_is_replaced_whole_and_composed_forms_stay_distinct() {
        let text = "Café ☕ meeting in 東京 👩‍💻 at 10";
        let r = replace_once(text, "☕", "🍵").unwrap();
        assert_eq!(r.text, "Café 🍵 meeting in 東京 👩‍💻 at 10");
        let r = replace_once(text, "東京 👩‍💻", "大阪").unwrap();
        assert_eq!(r.text, "Café ☕ meeting in 大阪 at 10");
        // "é" as e + a combining accent isn't the "é" in the note.
        assert!(matches!(
            replace_once(text, "Cafe\u{301}", "Bar"),
            Err(Miss::NotFound { .. })
        ));
        // Case folding that changes length (İ → i̇) still quotes the note.
        match replace_once("\u{130}stanbul trip", "i\u{307}stanbul trip", "x") {
            Err(Miss::NotFound { near, .. }) => {
                assert_eq!(near, vec![(1, "\u{130}stanbul trip".to_string())])
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_empty_field_is_filled_only_with_an_empty_old_text() {
        let r = replace_once("", "", "First line\r\nSecond").unwrap();
        assert_eq!(r.text, "First line\nSecond");
        assert_eq!(r.at, 0);
        // Blank counts as empty.
        assert_eq!(replace_once("  \n", "", "x").unwrap().text, "x");
        assert_eq!(replace_once("Already here", "", "x"), Err(Miss::NotEmpty));
    }

    #[test]
    fn copied_line_numbers_are_spotted() {
        let text = "Ship the beta on Friday.\nAsk Dave.";
        match replace_once(text, "1| Ship the beta on Friday.\n2| Ask Dave.", "x") {
            Err(Miss::NotFound { near, numbered }) => {
                assert!(numbered);
                assert_eq!(near, vec![(1, text.to_string())]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            strip_line_numbers("  12| a\n\n13|b"),
            Some("a\n\nb".to_string())
        );
        assert_eq!(strip_line_numbers("10: Doors open"), None);
        assert_eq!(strip_line_numbers("12| a\nb"), None);
    }

    // ---- apply_edit ----

    #[test]
    fn edits_keep_the_notes_bookkeeping_right() {
        let mut n = note("Ship on Friday.");
        n.enhanced_content = "## Summary\n- Ship on Friday".into();
        n.enhanced_at_hash = crate::notes::content_hash(&n.content);
        let fresh = |n: &Note| n.enhanced_at_hash == crate::notes::content_hash(&n.content);
        assert!(fresh(&n));

        // A content edit leaves the summary as it was, so it shows as stale.
        let e = apply_edit(&mut n, Field::Content, "Friday", "Monday").unwrap();
        assert!(e.changed && !e.filled);
        assert_eq!(n.content, "Ship on Monday.");
        assert!(!fresh(&n));

        // A summary edit doesn't make it fresh (it was made from other notes)…
        apply_edit(&mut n, Field::Enhanced, "Friday", "Monday").unwrap();
        assert_eq!(n.enhanced_content, "## Summary\n- Ship on Monday");
        assert!(!fresh(&n));

        // …emptying it leaves no summary at all…
        apply_edit(&mut n, Field::Enhanced, "## Summary\n- Ship on Monday", "").unwrap();
        assert!(n.enhanced_content.is_empty() && n.enhanced_at_hash.is_empty());

        // …and one written into the empty field is made from the notes as they are.
        let e = apply_edit(&mut n, Field::Enhanced, "", "- Ship on Monday").unwrap();
        assert!(e.filled);
        assert!(fresh(&n));

        // The transcript is never touched.
        n.transcript = vec![seg("them", "Ship on Friday", 10)];
        assert_eq!(
            apply_edit(&mut n, Field::Transcript, "Friday", "Monday").unwrap_err(),
            EditError::ReadOnly
        );
        assert_eq!(n.transcript[0].text, "Ship on Friday");
    }

    #[test]
    fn a_title_edit_is_the_persons_and_stays_one_line() {
        let mut n = note("");
        n.title = "Teams call · 5 Oct, 14:30".into();
        n.title_auto = true;
        assert_eq!(
            apply_edit(&mut n, Field::Title, "Teams call", "Budget\nreview").unwrap_err(),
            EditError::TitleLines
        );
        assert!(n.title_auto, "a refused edit changes nothing");
        apply_edit(
            &mut n,
            Field::Title,
            "Teams call · 5 Oct, 14:30",
            "Budget review",
        )
        .unwrap();
        assert_eq!(n.title, "Budget review");
        assert!(!n.title_auto, "the AI meeting title keeps off it now");
    }

    #[test]
    fn a_miss_changes_nothing_and_says_why() {
        let mut n = note("Ship the beta.\nShip the beta docs.");
        let before = n.content.clone();
        let err = apply_edit(&mut n, Field::Content, "Ship the beta", "x").unwrap_err();
        assert_eq!(n.content, before);
        assert!(err
            .message(Field::Content)
            .contains("appears 2 times in the notes (lines 1, 2)"));
        assert_eq!(err.short(), "the text is in 2 places");
        let err = apply_edit(&mut n, Field::Content, "ship  the beta.", "x").unwrap_err();
        assert!(err.message(Field::Content).contains("listed in `near`"));
        let err = apply_edit(&mut n, Field::Content, "", "x").unwrap_err();
        assert_eq!(err, EditError::Miss(Miss::NotEmpty));
    }

    #[test]
    fn the_same_text_again_is_no_change() {
        let mut n = note("Ship on Friday.");
        let e = apply_edit(&mut n, Field::Content, "Friday", "Friday").unwrap();
        assert!(!e.changed);
    }

    #[test]
    fn an_edit_shows_its_lines_afterwards() {
        let mut n = note("one\ntwo\nthree\nfour\nfive");
        let e = apply_edit(&mut n, Field::Content, "three", "3\n3b").unwrap();
        assert_eq!(e.line, 3);
        assert_eq!(e.now, "2| two\n3| 3\n4| 3b\n5| four");
    }

    // ---- search ----

    #[test]
    fn search_finds_lines_forgivingly_and_quotes_the_note() {
        let mut n = note("Agenda\nShip the BETA on Friday.\nAsk Dave about the venue.\nShip the beta docs too.\nLunch");
        n.enhanced_content = "- Beta ships Friday".into();
        n.transcript = vec![
            seg("you", "When does the beta ship?", 100),
            seg("them", "Friday, I hope", 130),
        ];
        let all = [Field::Content, Field::Enhanced, Field::Transcript];
        let found = search(&n, "  the   beta ", &all);
        assert_eq!(
            found.lines,
            vec![
                (Field::Content, 2),
                (Field::Enhanced, 0),
                (Field::Transcript, 1)
            ]
        );
        let first = &found.hits[0];
        assert_eq!((first.field, first.line), ("content", 2));
        assert_eq!(first.text, "Ship the BETA on Friday.");
        assert_eq!(
            first.context,
            vec![
                "1| Agenda",
                "3| Ask Dave about the venue.",
                "4| Ship the beta docs too."
            ]
        );
        let said = &found.hits[2];
        assert_eq!((said.field, said.line), ("transcript", 1));
        assert_eq!(said.text, "[0:00] You: When does the beta ship?");
        // Curly quotes and dashes fold; a query across a line break finds both lines.
        let n = note("Don’t ship — yet\nvenue: TBD");
        let found = search(&n, "don't ship - yet venue", &[Field::Content]);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].to_line, Some(2));
        assert_eq!(found.hits[0].text, "Don’t ship — yet\nvenue: TBD");
        // Nothing to look for, nothing found.
        assert_eq!(search(&n, "   ", &[Field::Content]).total(), 0);
    }

    #[test]
    fn search_is_capped() {
        let long: String = (1..=200).map(|i| format!("item {i}: the plan\n")).collect();
        let found = search(&note(&long), "plan", &[Field::Content]);
        assert_eq!(found.total(), 200);
        assert!(found.more());
        assert!(found.hits.len() <= MAX_HITS);
        assert!(found
            .hits
            .iter()
            .skip(CONTEXT_HITS)
            .all(|h| h.context.is_empty()));
        let size: usize = serde_json::to_string(&found.hits).unwrap().len();
        assert!(size < SEARCH_CHARS + 1_500, "{size}");
        // A match in a very long line shows the part around it, the note's own text.
        let line = format!("{}needle{}", "a ".repeat(400), " b".repeat(400));
        let found = search(&note(&line), "NEEDLE", &[Field::Content]);
        let hit = &found.hits[0];
        assert!(hit.partial);
        assert!(line.contains(&hit.text));
        assert!(hit.text.contains("needle") && hit.text.chars().count() <= 2 * EXCERPT_SIDE + 6);
        // Several on one line are one hit.
        let found = search(&note("plan, plan, plan"), "plan", &[Field::Content]);
        assert_eq!(found.hits.len(), 1);
        assert_eq!(found.hits[0].times, Some(3));
    }

    #[test]
    fn transcript_lines_are_talk_only_and_timed() {
        let mut n = note("");
        let mut echo = seg("you", "echoed", 70);
        echo.echo = true;
        n.transcript = vec![
            seg("them", "Hello  there", 1_000),
            TranscriptSegment::dictation_marker(1_030),
            echo,
            seg("you", "Hi", 1_075),
            seg("them", "   ", 1_080),
        ];
        assert_eq!(
            field_text(&n, Field::Transcript),
            "[0:00] Them: Hello there\n[1:15] You: Hi"
        );
    }

    // ---- read_page / fit ----

    #[test]
    fn pages_are_bounded_and_say_where_to_go_on() {
        let text: String = (1..=10).map(|i| format!("line {i}\r\n")).collect();
        let p = read_page(&text, 1, 4, PAGE_TOKENS).unwrap();
        assert_eq!(
            (p.from, p.to, p.total_lines, p.more, p.next_offset),
            (1, 4, 10, true, Some(5))
        );
        assert_eq!(p.text, "1| line 1\n2| line 2\n3| line 3\n4| line 4");
        let last = read_page(&text, 9, 4, PAGE_TOKENS).unwrap();
        assert_eq!((last.to, last.more, last.next_offset), (10, false, None));
        assert!(read_page(&text, 11, 4, PAGE_TOKENS).is_err());
        // Offset 0 reads from the start; the limit is capped.
        assert_eq!(read_page(&text, 0, 10_000, PAGE_TOKENS).unwrap().to, 10);
        // The token budget ends a page early…
        let p = read_page(&text, 1, 10, 9).unwrap();
        assert!(p.to < 10 && p.more);
        // …and one line longer than a page shows its start and says so.
        let huge = format!("short\n{}\nafter", "word ".repeat(5_000));
        let p = read_page(&huge, 2, 10, 100).unwrap();
        assert_eq!((p.to, p.cut_line, p.next_offset), (2, Some(2), Some(3)));
        assert!(estimate_tokens(&p.text) <= 100);
        // An empty field is an empty page.
        assert_eq!(read_page("", 1, 10, PAGE_TOKENS).unwrap().total_lines, 0);
    }

    #[test]
    fn fit_keeps_whole_lines_within_the_budget() {
        assert_eq!(fit("short", 10), ("short".to_string(), false));
        let text = "first line here\nsecond line here\nthird line here";
        let (head, cut) = fit(text, 12);
        assert!(cut);
        assert_eq!(head, "first line here\nsecond line here");
        assert_eq!(fit(text, 10).0, "first line here");
        let (head, cut) = fit(&"x".repeat(100), 7);
        assert!(cut && head.len() == 24);
    }

    // ---- change_preview / Field ----

    #[test]
    fn change_previews_show_what_changed() {
        let p = |o, n| change_preview(o, n);
        assert_eq!(
            p("Ship the beta on Friday.", "Ship the beta on Monday."),
            ("Friday.".into(), "Monday.".into())
        );
        assert_eq!(
            p("Ship the beta.", "Ship the beta and docs."),
            ("beta.".into(), "beta and docs.".into())
        );
        assert_eq!(p("a b", "a x b"), ("".into(), "x".into()));
        assert_eq!(
            p("- [ ] Book the venue\n", ""),
            ("- [ ] Book the venue".into(), "".into())
        );
        assert_eq!(p("x", "x"), ("".into(), "".into()));
        assert_eq!(p("a  b", "a b"), ("".into(), "".into())); // spacing only
        assert_eq!(p("a b c", "a c"), ("b".into(), "".into()));
        let many = "word ".repeat(30);
        let (_, long) = change_preview("a", &many);
        assert!(long.ends_with('…') && long.chars().count() == 40);
        assert_eq!(p("Café", "Cafés"), ("Café".into(), "Cafés".into()));
    }

    #[test]
    fn fields_take_the_names_models_use() {
        assert_eq!(Field::parse("Content"), Some(Field::Content));
        assert_eq!(Field::parse("summary"), Some(Field::Enhanced));
        assert_eq!(Field::parse("enhanced_content"), Some(Field::Enhanced));
        assert_eq!(Field::parse("my thoughts"), Some(Field::Content));
        assert_eq!(Field::parse("transcript"), Some(Field::Transcript));
        assert_eq!(Field::parse("title"), Some(Field::Title));
        assert_eq!(Field::parse("folder"), None);
    }
}
