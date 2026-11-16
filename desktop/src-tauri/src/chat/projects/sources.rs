//! **A big project's parts as an answer's numbered sources** (16 Nov 2026,
//! after the Claude app, whose answers cite a project's files with pills
//! that open them): the parts of a searched project (`knowledge.rs`) that
//! an answer cites are its sources, as a web answer's pages are — a "[2]"
//! pill in its words, a row under it, each opening the file at its page,
//! slide, row or heading beside the chat.
//!
//! - **One numbering with the turn's other sources** ([`Cites`]): the parts
//!   sent with the question first, [1] to [k] (each its `<document index>`),
//!   then a part a tool reads (`read_project_file`, `search_project_files`:
//!   its `"n"`) the next number the turn has — after the web's pages and the
//!   calendar's meetings in Chat's own tool loop (`web::Turn::reserve_number`,
//!   the web's numbering starting after the question's parts), from one
//!   numbering shared with Claude Code's pages on your Claude plan
//!   ([`Numbers`]: Yap numbers those itself as they come in,
//!   `claude_plan/map.rs`, while the project's tools answer apart, over the
//!   run endpoint). A part read again keeps its number.
//! - **Kept with the answer** ([`Cites::cited_in`]): the parts it cites, each
//!   a `chats::Source` with its `project` place (`ProjectPlace`: the file,
//!   its lines in Yap's text of it, its first page, slide or row, its sheet
//!   and section); never a number another of the answer's sources has (a
//!   small model's web pipeline writes its own prompt, without the parts,
//!   and numbers its pages from 1). An answer that cites none has none:
//!   which parts it was given stay its steps' chips.
//! - **Cited in words** ([`cite_mentions`]): "(staff-handbook.pdf, page
//!   12)" naming one of the turn's parts, as the instructions before these
//!   asked, becomes its "[n]".
//!
//! Pure but for the lock; the log has counts, never names or words.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use super::parts::{Part, Size};
use super::ProjectFile;
use crate::chat::chats::{ProjectPlace, Source};

/// **One numbering for a turn's sources**, shared: the last number given.
#[derive(Debug, Clone, Default)]
pub struct Numbers(Arc<AtomicUsize>);

impl Numbers {
    /// The next number.
    pub fn next(&self) -> usize {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// A part numbered this turn.
#[derive(Debug, Clone)]
struct Kept {
    file: String,
    part: usize,
    source: Source,
}

/// **One turn's project parts, numbered** (made with the question's parts,
/// `knowledge::for_question`).
#[derive(Debug)]
pub struct Cites {
    /// The parts' size for the turn's model (`knowledge::Room::size`): the
    /// tools read parts of the same size as the list the model was given.
    pub size: Size,
    /// The turn's own numbering: the question's parts, and on your Claude
    /// plan the tools' parts and Claude Code's pages too.
    pub numbers: Numbers,
    parts: Mutex<Vec<Kept>>,
}

impl Cites {
    pub fn new(size: Size) -> Cites {
        Cites { size, numbers: Numbers::default(), parts: Mutex::new(Vec::new()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Kept>> {
        self.parts.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// **Part `p` of project `project`'s file `f`, numbered**: the number it
    /// has this turn, else `next()`'s.
    pub fn number(&self, project: &str, f: &ProjectFile, p: &Part, next: &mut dyn FnMut() -> usize) -> usize {
        let had = |kept: &[Kept]| kept.iter().find(|k| k.file == f.id && k.part == p.n).map(|k| k.source.n);
        if let Some(n) = had(&self.lock()) {
            return n;
        }
        // (Not under the lock: the next number may count the parts, `len`.)
        let n = next();
        let mut kept = self.lock();
        if let Some(had) = had(&kept) {
            return had;
        }
        kept.push(Kept { file: f.id.clone(), part: p.n, source: source_of(project, f, p, n) });
        n
    }

    /// The same, from the turn's own numbering ([`Cites::numbers`]).
    pub fn number_own(&self, project: &str, f: &ProjectFile, p: &Part) -> usize {
        let numbers = self.numbers.clone();
        self.number(project, f, p, &mut || numbers.next())
    }

    /// How many parts are numbered.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every part numbered, as sources, by number (the page draws a "[2]"
    /// as it's written against these).
    pub fn sources(&self) -> Vec<Source> {
        let mut out: Vec<Source> = self.lock().iter().map(|k| k.source.clone()).collect();
        out.sort_by_key(|s| s.n);
        out
    }

    /// **The parts `reply` cites**, marked cited — none whose number is
    /// `taken` (another of the answer's sources has it).
    pub fn cited_in(&self, reply: &str, taken: &[usize]) -> Vec<Source> {
        let cited = crate::web::citations(reply);
        self.sources()
            .into_iter()
            .filter(|s| cited.contains(&s.n) && !taken.contains(&s.n))
            .map(|mut s| {
                s.cited = true;
                s
            })
            .collect()
    }
}

/// A part's words for its tooltip: its text without the page lines and
/// the sheet's head, on one line, at most 280 characters. Pure.
fn quote_of(p: &Part) -> String {
    let words: Vec<&str> = p
        .text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("[Page ") && !l.starts_with("|---") && !l.starts_with("| |") && !l.starts_with("## Sheet "))
        .collect();
    let one = words.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(279) {
        Some((i, _)) => format!("{}\u{2026}", one[..i].trim_end()),
        None => one,
    }
}

/// **Where a part is, short** (the row's chip, after the file's name): what
/// it counts ("page 12", "slides 3–5", "sheet "Orders", rows 2–180"), else
/// its section's own heading in quotes, else "part 3". Pure.
pub fn site_of(p: &Part) -> String {
    if !p.at.is_empty() {
        return p.at.clone();
    }
    match p.section.rsplit(" \u{203a} ").next().map(str::trim).filter(|s| !s.is_empty()) {
        Some(h) => format!("\u{201c}{h}\u{201d}"),
        None => format!("part {}", p.n),
    }
}

/// **Part `p` of file `f` as source `n`**: the file's name its title, where
/// the part is its site, its words its quote, and where it opens. Pure.
pub fn source_of(project: &str, f: &ProjectFile, p: &Part, n: usize) -> Source {
    Source {
        n,
        title: f.name.clone(),
        site: site_of(p),
        quote: quote_of(p),
        project: Some(ProjectPlace {
            project: project.to_string(),
            file: f.id.clone(),
            kind: f.kind.clone(),
            part: p.n,
            from: p.lines.0,
            to: p.lines.1,
            at: p.span.map(|s| s.0),
            sheet: p.sheet.clone(),
            section: p.section.clone(),
        }),
        ..Default::default()
    }
}

/// A part's place a citation in words may name: its pages, slides, rows or
/// lines, as numbers. Pure.
fn spans(s: &Source) -> Option<(u32, u32)> {
    let at = s.project.as_ref()?.at?;
    let to = s.site.rsplit(['\u{2013}', '-']).next().and_then(|t| t.trim().parse().ok()).filter(|t: &u32| *t >= at).unwrap_or(at);
    Some((at, to))
}

/// A citation in words: "(staff-handbook.pdf, page 12)", "(see handbook.md,
/// section “Annual leave”)".
static MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]?\(([^()\n]{3,200})\)").expect("static regex"));
/// A page, slide, row or line it names.
static NUMBERED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:pages?|pp?\.|slides?|rows?|lines?)\s*(\d{1,6})").expect("static regex"));

/// **Citations in words, as numbers**: a parenthesis naming one of the
/// turn's parts — its file's name, then its page, slide, row or line (or
/// its section's heading, or the file's only part numbered) — becomes that
/// part's "[n]" (a short one wholly; a longer one keeps its words, the
/// number after it). Never in code; one naming no part, or two, stays.
/// Pure.
pub fn cite_mentions(reply: &str, cites: &Cites) -> String {
    let sources = cites.sources();
    if sources.is_empty() || !reply.contains('(') {
        return reply.to_string();
    }
    let code = code_ranges(reply);
    let mut out = String::with_capacity(reply.len());
    let mut at = 0;
    for m in MENTION.captures_iter(reply) {
        let whole = m.get(0).expect("match");
        if code.iter().any(|&(s, e)| whole.start() >= s && whole.start() < e) {
            continue;
        }
        let inside = m[1].trim();
        let lower = inside.to_lowercase();
        // The file it names (the longest name, so "plan.md" never wins over "garden-plan.md").
        let Some(name) = sources.iter().map(|s| s.title.to_lowercase()).filter(|t| !t.is_empty() && lower.contains(t.as_str())).max_by_key(|t| t.len()) else { continue };
        let of_file: Vec<&Source> = sources.iter().filter(|s| s.title.to_lowercase() == name).collect();
        let numbered = NUMBERED.captures(inside).and_then(|c| c[1].parse::<u32>().ok());
        let by_place: Vec<&&Source> = match numbered {
            Some(k) => of_file.iter().filter(|s| spans(s).is_some_and(|(a, b)| a <= k && k <= b)).collect(),
            None => of_file
                .iter()
                .filter(|s| {
                    let heading = s.project.as_ref().map(|p| p.section.rsplit(" \u{203a} ").next().unwrap_or("").trim().to_lowercase()).unwrap_or_default();
                    !heading.is_empty() && lower.contains(&heading)
                })
                .collect(),
        };
        let chosen = match (by_place.len(), numbered, of_file.len()) {
            (1, _, _) => by_place[0].n,
            // (Its only part this turn, the place not said otherwise.)
            (0, None, 1) => of_file[0].n,
            _ => continue,
        };
        out.push_str(&reply[at..whole.start()]);
        // Only the file and its place (a "see", "from"): the number alone.
        let rest = lower.trim_start_matches("see ").trim_start_matches("from ").trim_start_matches("in ");
        if rest.starts_with(&name) && inside.chars().count() <= 90 {
            out.push_str(&format!(" [{chosen}]"));
        } else {
            out.push_str(whole.as_str());
            out.push_str(&format!(" [{chosen}]"));
        }
        at = whole.end();
    }
    out.push_str(&reply[at..]);
    out
}

/// Byte ranges of code in markdown: fenced blocks, and inline spans. Pure.
fn code_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<usize> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            match fence.take() {
                Some(start) => out.push((start, pos + line.len())),
                None => fence = Some(pos),
            }
        }
        pos += line.len();
    }
    if let Some(start) = fence {
        out.push((start, text.len()));
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' && !out.iter().any(|&(s, e)| i >= s && i < e) {
            if let Some(end) = text[i + 1..].find('`') {
                out.push((i, i + 1 + end + 1));
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str, kind: &str) -> ProjectFile {
        ProjectFile { id: id.into(), name: name.into(), kind: kind.into(), ..Default::default() }
    }

    fn part(n: usize, at: &str, span: Option<(u32, u32)>, section: &str, lines: (u32, u32)) -> Part {
        Part { n, at: at.into(), section: section.into(), text: format!("[Page {}]\nThe policy for part {n}, at Example Ltd.", span.map_or(0, |s| s.0)), tokens: 20, lines, span, sheet: String::new() }
    }

    #[test]
    fn parts_are_numbered_once_with_the_turns_numbering() {
        let pdf = file("pf_00000000000000b1", "staff-handbook.pdf", "pdf");
        let md = file("pf_00000000000000b2", "staff-handbook.md", "markdown");
        let c = Cites::new(Size::SMALL);
        assert_eq!(c.number_own("p-1", &pdf, &part(8, "page 12", Some((12, 12)), "4.1 Annual leave", (300, 340))), 1);
        assert_eq!(c.number_own("p-1", &md, &part(3, "", None, "Leave \u{203a} Annual leave", (40, 52))), 2);
        // The web's next number (the tool loop's), a part read again keeps its own.
        let mut web = 5;
        let mut next = || {
            web += 1;
            web
        };
        assert_eq!(c.number("p-1", &pdf, &part(9, "pages 13\u{2013}14", Some((13, 14)), "", (341, 400)), &mut next), 6);
        assert_eq!(c.number("p-1", &pdf, &part(8, "page 12", Some((12, 12)), "", (300, 340)), &mut next), 1);
        assert_eq!(c.len(), 3);
        let s = c.sources();
        assert_eq!(s.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 2, 6]);
        assert_eq!((s[0].title.as_str(), s[0].site.as_str()), ("staff-handbook.pdf", "page 12"));
        assert_eq!(s[1].site, "\u{201c}Annual leave\u{201d}");
        let place = s[0].project.as_ref().unwrap();
        assert_eq!((place.project.as_str(), place.file.as_str(), place.kind.as_str(), place.part, place.from, place.to, place.at), ("p-1", "pf_00000000000000b1", "pdf", 8, 300, 340, Some(12)));
        assert!(s[0].url.is_empty() && s[0].quote.starts_with("The policy for part 8"), "{:?}", s[0]);
        // Kept: the ones the answer cites, none another source has.
        let kept = c.cited_in("Staff get 27 days [1]. Up to five carry over [2][6].", &[2]);
        assert_eq!(kept.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 6]);
        assert!(kept.iter().all(|s| s.cited));
        assert!(c.cited_in("No citations here.", &[]).is_empty());
        // On your Claude plan, one numbering with Claude Code's pages.
        let plan = Cites::new(Size::LARGE);
        plan.number_own("p-1", &pdf, &part(1, "page 1", Some((1, 1)), "", (1, 20)));
        assert_eq!(plan.numbers.next(), 2, "a page found after the question's parts");
        assert_eq!(plan.number_own("p-1", &md, &part(4, "", None, "Parking", (60, 70))), 3);
    }

    #[test]
    fn a_citation_in_words_becomes_its_number() {
        let pdf = file("pf_00000000000000b3", "staff-handbook.pdf", "pdf");
        let md = file("pf_00000000000000b4", "staff-handbook.md", "markdown");
        let c = Cites::new(Size::MEDIUM);
        c.number_own("p-1", &pdf, &part(8, "pages 12\u{2013}13", Some((12, 13)), "4.1 Annual leave", (300, 340)));
        c.number_own("p-1", &pdf, &part(20, "page 40", Some((40, 40)), "9 Fire safety", (900, 930)));
        c.number_own("p-1", &md, &part(3, "", None, "Leave \u{203a} Annual leave", (40, 52)));
        assert_eq!(cite_mentions("Staff get 27 days (staff-handbook.pdf, page 13).", &c), "Staff get 27 days [1].");
        assert_eq!(cite_mentions("Meet at the car park (see staff-handbook.pdf, p. 40).", &c), "Meet at the car park [2].");
        assert_eq!(cite_mentions("Five days carry over (staff-handbook.md, section \u{201c}Annual leave\u{201d}).", &c), "Five days carry over [3].");
        // A longer parenthesis keeps its words, the number after it.
        assert_eq!(
            cite_mentions("It's 27 days (the handbook's staff-handbook.pdf says so on page 12, for full-time staff only).", &c),
            "It's 27 days (the handbook's staff-handbook.pdf says so on page 12, for full-time staff only) [1]."
        );
        // No part there, two to choose from, another file, code: left alone.
        for alone in ["See (staff-handbook.pdf, page 77).", "See (staff-handbook.pdf).", "From (rota.txt, line 2).", "Run `f(staff-handbook.pdf, page 12)` now."] {
            assert_eq!(cite_mentions(alone, &c), alone);
        }
    }
}
