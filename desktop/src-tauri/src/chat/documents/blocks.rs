//! **A document as blocks** (2026-10-31; the rich document tools,
//! `read_document` / `edit_document`, after the contract of Claude's own
//! document tools, simplified): a document's markdown cut into the blocks a
//! reader sees — a heading, a paragraph, a whole list, a whole table, a code
//! block, a quote, a rule, a section still to write — each with a short `id`
//! and a `hash` of what it says, and edits by block that never write over
//! what the person changed.
//!
//! - **Ids stay put while a block is unchanged** ([`identify`]): an id is
//!   made of the block's kind and its text — `p3fa9c` for a paragraph whose
//!   text hashes so — with the block's occurrence among blocks of the same id
//!   after it when there are several (`p3fa9c-2`). Nothing else goes in: not
//!   its place, not the blocks around it. So an edit elsewhere (a block
//!   added above, another rewritten, a section filled) leaves every other id
//!   as it was, and the same text always reads with the same ids, after a
//!   restart too. A changed block has a new id: the model's old one then
//!   names nothing, and [`resolve`] says what's there now instead.
//! - **The hash** is 8 hex characters of the block's text (lines' trailing
//!   spaces and `\r` aside), from other bits of the same 64-bit hash than the
//!   id: an edit names the hash it read (`if_hash`), so two blocks whose
//!   short ids collide can't be mistaken for each other.
//! - **The person's words win** ([`apply`]): an edit names the blocks it read;
//!   one the person changed or removed since the model's last look (its
//!   picture, `documents::saw`) is refused, with the block as it stands now —
//!   found by lining the picture's blocks up with the document's
//!   ([`super::hunks`] over their hashes). Ops run in order on a working copy
//!   and land together or not at all.
//!
//! Pure: no notes, no versions (those are `rich.rs`').

use std::collections::HashMap;

use super::{heading, hunks, parse_placeholder};

/// What a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Heading,
    Paragraph,
    List,
    Table,
    Code,
    Quote,
    Rule,
    /// A section still to write (`<!-- yap:section … -->`).
    Placeholder,
}

impl Kind {
    /// The name the model reads.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Heading => "heading",
            Kind::Paragraph => "paragraph",
            Kind::List => "list",
            Kind::Table => "table",
            Kind::Code => "code",
            Kind::Quote => "quote",
            Kind::Rule => "rule",
            Kind::Placeholder => "section_to_write",
        }
    }

    /// The first letter of its ids.
    fn letter(self) -> char {
        match self {
            Kind::Heading => 'h',
            Kind::Paragraph => 'p',
            Kind::List => 'l',
            Kind::Table => 't',
            Kind::Code => 'c',
            Kind::Quote => 'q',
            Kind::Rule => 'r',
            Kind::Placeholder => 's',
        }
    }
}

/// One block: its kind and its lines in the document, `[start, end)` (0 =
/// the first line), and its text (those lines, `\r` kept).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A block's id and hash ([`identify`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id {
    pub id: String,
    pub hash: String,
}

// ---- the lines --------------------------------------------------------------------------

fn blank(l: &str) -> bool {
    l.trim().is_empty()
}

/// Up to three spaces of indent, as a block's first line may have.
fn lead(l: &str) -> Option<&str> {
    let n = l.len() - l.trim_start_matches(' ').len();
    (n <= 3).then(|| &l[n..])
}

/// A code fence's mark and length ("```", "~~~~").
fn fence(l: &str) -> Option<(char, usize)> {
    let t = lead(l)?;
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

fn closes(l: &str, (c, n): (char, usize)) -> bool {
    lead(l).is_some_and(|t| {
        let run = t.chars().take_while(|x| *x == c).count();
        run >= n && t[run * c.len_utf8()..].trim().is_empty()
    })
}

/// A thematic break: three or more of one of `-`, `*`, `_`, spaces between.
fn rule(l: &str) -> bool {
    let Some(t) = lead(l) else { return false };
    let t = t.trim_end();
    let Some(c) = t.chars().next().filter(|c| matches!(c, '-' | '*' | '_')) else { return false };
    t.chars().all(|x| x == c || x == ' ' || x == '\t') && t.chars().filter(|x| *x == c).count() >= 3
}

/// A list item's first line: `-`, `*` or `+`, or a number with `.` or `)`,
/// then a space (or nothing).
fn list_item(l: &str) -> bool {
    let Some(t) = lead(l) else { return false };
    let mut chars = t.chars();
    match chars.next() {
        Some('-' | '*' | '+') => matches!(chars.next(), None | Some(' ' | '\t')),
        Some(d) if d.is_ascii_digit() => {
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            let rest = &t[digits..];
            digits <= 9 && (rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")")
        }
        _ => false,
    }
}

fn quote(l: &str) -> bool {
    lead(l).is_some_and(|t| t.starts_with('>'))
}

/// A table's delimiter row: `|---|:--:|`, `---|---`.
fn delimiter_row(l: &str) -> bool {
    let t = l.trim();
    !t.is_empty() && t.contains('-') && (t.contains('|') || t.contains(":-")) && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// Line `i` starts a table: a row of cells (`| a | b |`), or any line with a
/// `|` over a delimiter row.
fn table_start(lines: &[&str], i: usize) -> bool {
    let l = lines[i];
    lead(l).is_some_and(|t| t.starts_with('|')) || (l.contains('|') && lines.get(i + 1).is_some_and(|n| delimiter_row(n)))
}

/// A line that starts a block of another kind (it ends a paragraph).
fn starts_block(lines: &[&str], i: usize) -> bool {
    let l = lines[i];
    fence(l).is_some()
        || parse_placeholder(l).is_some()
        || heading(l).is_some()
        || rule(l)
        || quote(l)
        || list_item(l)
        || table_start(lines, i)
}

/// Indented as a list item's continuation (two spaces or a tab).
fn indented(l: &str) -> bool {
    l.starts_with("  ") || l.starts_with('\t')
}

// ---- parsing ----------------------------------------------------------------------------

/// **`content`'s blocks**, in order. Blank lines part them (and are no
/// block); a code block keeps its blank lines; a list goes on over a blank
/// line when the next line is another item or indented under one. Pure.
pub fn parse(content: &str) -> Vec<Block> {
    let lines: Vec<&str> = content.split('\n').collect();
    let n = lines.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let l = lines[i];
        if blank(l) {
            i += 1;
            continue;
        }
        let start = i;
        let (kind, end) = if let Some(f) = fence(l) {
            let mut j = i + 1;
            while j < n && !closes(lines[j], f) {
                j += 1;
            }
            (Kind::Code, (j + 1).min(n))
        } else if parse_placeholder(l).is_some() {
            (Kind::Placeholder, i + 1)
        } else if heading(l).is_some() {
            (Kind::Heading, i + 1)
        } else if rule(l) {
            (Kind::Rule, i + 1)
        } else if table_start(&lines, i) {
            let mut j = i + 1;
            while j < n && !blank(lines[j]) && lines[j].contains('|') {
                j += 1;
            }
            (Kind::Table, j)
        } else if quote(l) {
            let mut j = i + 1;
            while j < n && quote(lines[j]) {
                j += 1;
            }
            (Kind::Quote, j)
        } else if list_item(l) {
            let mut j = i + 1;
            loop {
                if j >= n {
                    break;
                }
                if blank(lines[j]) {
                    // Over blank lines only to another item or an indented line.
                    let k = (j..n).find(|k| !blank(lines[*k]));
                    match k {
                        Some(k) if list_item(lines[k]) || indented(lines[k]) => j = k + 1,
                        _ => break,
                    }
                } else if list_item(lines[j]) || indented(lines[j]) || !starts_block(&lines, j) {
                    j += 1;
                } else {
                    break;
                }
            }
            (Kind::List, j)
        } else {
            let mut j = i + 1;
            while j < n && !blank(lines[j]) && !starts_block(&lines, j) {
                j += 1;
            }
            (Kind::Paragraph, j)
        };
        let end = end.max(start + 1);
        out.push(Block { kind, start, end, text: lines[start..end].join("\n") });
        i = end;
    }
    out
}

// ---- ids and hashes ---------------------------------------------------------------------

/// A block's text as its hash reads it: `\r` and trailing spaces left out.
fn normal(text: &str) -> String {
    text.split('\n').map(|l| l.trim_end()).collect::<Vec<_>>().join("\n")
}

/// FNV-1a, 64 bits (the same on every PC and every run: ids must outlive a
/// restart).
fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// **Every block's id and hash** (see the module notes): the kind's letter
/// and 5 hex characters of its text (the low bits), then `-2`, `-3`… for the
/// second and later blocks with that id. Pure.
pub fn identify(blocks: &[Block]) -> Vec<Id> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    blocks
        .iter()
        .map(|b| {
            let h = fnv(&format!("{}\n{}", b.kind.letter(), normal(&b.text)));
            let base = format!("{}{:05x}", b.kind.letter(), h & 0xf_ffff);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let id = if *n == 1 { base } else { format!("{base}-{n}") };
            Id { id, hash: format!("{:08x}", h >> 32) }
        })
        .collect()
}

/// A document as blocks, with their ids.
#[derive(Debug, Clone)]
pub struct Doc {
    pub lines: Vec<String>,
    pub blocks: Vec<Block>,
    pub ids: Vec<Id>,
}

impl Doc {
    pub fn of(content: &str) -> Doc {
        let blocks = parse(content);
        let ids = identify(&blocks);
        Doc { lines: content.split('\n').map(str::to_string).collect(), blocks, ids }
    }

    /// The block with id `id`.
    pub fn find(&self, id: &str) -> Option<usize> {
        let id = id.trim();
        self.ids.iter().position(|x| x.id == id)
    }

    /// The blocks' hashes, in order (what two copies are lined up by).
    fn hashes(&self) -> Vec<&str> {
        self.ids.iter().map(|x| x.hash.as_str()).collect()
    }
}

/// A block as the model reads it: its id, hash, kind and text (a section
/// still to write: its key and what it's for, not the comment's syntax).
pub fn view(doc: &Doc, i: usize) -> serde_json::Value {
    let b = &doc.blocks[i];
    let id = &doc.ids[i];
    match parse_placeholder(&b.text).filter(|_| b.kind == Kind::Placeholder) {
        Some((key, intent)) => serde_json::json!({ "id": id.id, "hash": id.hash, "type": b.kind.name(), "key": key, "intent": intent }),
        None => serde_json::json!({ "id": id.id, "hash": id.hash, "type": b.kind.name(), "text": b.text.replace('\r', "") }),
    }
}

/// A block's text cut to `n` characters, on one line (a refusal's "was").
pub fn clip(text: &str, n: usize) -> String {
    let one = text.replace('\r', "").split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

// ---- what changed between two copies ----------------------------------------------------

/// One stretch where `after` differs from `before`: `before`'s blocks
/// `[old.0, old.1)` became `after`'s `[new.0, new.1)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub old: (usize, usize),
    pub new: (usize, usize),
}

/// **What changed, block by block**, from `before` to `after`, in order
/// (lined up by hash: a block that says the same is the same block). Pure.
pub fn changes(before: &Doc, after: &Doc) -> Vec<Change> {
    let (b, a) = (before.hashes(), after.hashes());
    let mut out = Vec::new();
    let mut shift: isize = 0;
    for h in hunks(&b, &a) {
        let new_start = (h.start as isize + shift) as usize;
        let new_end = new_start + h.lines.len();
        shift += h.lines.len() as isize - (h.end - h.start) as isize;
        out.push(Change { old: (h.start, h.end), new: (new_start, new_end) });
    }
    out
}

/// Where each of `before`'s blocks is in `after` (`None`: changed or gone).
fn line_up(before: &Doc, after: &Doc) -> Vec<Option<usize>> {
    let mut map: Vec<Option<usize>> = (0..before.blocks.len()).map(Some).collect();
    let mut shift: isize = 0;
    let mut last = 0;
    for c in changes(before, after) {
        for (i, m) in map.iter_mut().enumerate().take(c.old.0).skip(last) {
            *m = Some((i as isize + shift) as usize);
        }
        for m in map.iter_mut().take(c.old.1).skip(c.old.0) {
            *m = None;
        }
        last = c.old.1;
        shift = c.new.1 as isize - c.old.1 as isize;
    }
    for (i, m) in map.iter_mut().enumerate().skip(last) {
        *m = Some((i as isize + shift) as usize);
    }
    map
}

// ---- finding the block an edit names ----------------------------------------------------

/// What an id an edit names is now ([`resolve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// This block of the document now.
    At(usize),
    /// The model saw it, and the person changed it since: what's there now
    /// (the document's blocks in its place; none: it was taken out).
    Changed(Vec<usize>),
    /// No block of the document has it, nor of the model's picture.
    Unknown,
}

/// **The block `id` names** in `now`, by the model's own picture of the
/// document (`picture`: what it last saw): a block it saw that's unchanged
/// is found wherever it is now — even if its id shifted (an identical block
/// added above it) —, one the person changed or removed is
/// [`Found::Changed`], with what stands in its place. Without a picture, or
/// an id the picture hasn't got (read since from a page), the id itself.
/// Pure.
pub fn resolve(now: &Doc, picture: Option<&Doc>, id: &str) -> Found {
    if let Some(p) = picture {
        if let Some(i) = p.find(id) {
            let map = line_up(p, now);
            if let Some(j) = map[i] {
                return Found::At(j);
            }
            // In its place now: the blocks between its neighbours' places.
            let before = (0..i).rev().find_map(|k| map[k]).map_or(0, |k| k + 1);
            let after = (i + 1..p.blocks.len()).find_map(|k| map[k]).unwrap_or(now.blocks.len());
            return Found::Changed((before..after.max(before)).collect());
        }
    }
    match now.find(id) {
        Some(i) => Found::At(i),
        None => Found::Unknown,
    }
}

// ---- edits ------------------------------------------------------------------------------

/// Where an insert or a move goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    After(String),
    Before(String),
    End,
}

/// One edit ([`apply`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Insert { markdown: String, at: Place },
    Replace { id: String, if_hash: String, markdown: String },
    Delete { id: String, if_hash: String },
    Move { ids: Vec<(String, String)>, to: Place },
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::Insert { .. } => "insert",
            Op::Replace { .. } => "replace",
            Op::Delete { .. } => "delete",
            Op::Move { .. } => "move",
        }
    }
}

/// Why an edit didn't land ([`apply`]): the op (0 = the first) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub op: usize,
    pub why: Why,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The person changed (or removed) the block since the model saw it:
    /// the document's blocks in its place now (indexes into the document
    /// as it was before this edit).
    Changed { id: String, now: Vec<usize> },
    /// Its hash isn't what the model said (`if_hash`): the block now.
    Hash { id: String, at: usize },
    /// No such block.
    Unknown(String),
    /// The block was changed or removed by an earlier op of this edit.
    Earlier(String),
    /// A move to a place inside the blocks it moves.
    IntoItself(String),
    /// Missing: what's needed.
    Missing(&'static str),
}

/// A block of the working copy: its lines, the blank lines before it, the
/// block of the document it was (`None`: new), and whether this edit made
/// or moved it.
#[derive(Debug, Clone)]
struct Seg {
    lines: Vec<String>,
    gap: Vec<String>,
    orig: Option<usize>,
    touched: bool,
}

/// An edit that landed ([`apply`]): the document now, and per op the
/// document's new blocks it made (indexes into [`Applied::doc`]).
#[derive(Debug, Clone)]
pub struct Applied {
    pub content: String,
    pub doc: Doc,
    pub made: Vec<Vec<usize>>,
}

/// Markdown going in: line endings as the document's, no placeholder lines
/// (a copied one would be a section nobody asked for), blank lines at its
/// ends left out.
fn incoming(markdown: &str, crlf: bool) -> Vec<String> {
    let kept: Vec<&str> = markdown.split('\n').map(|l| l.trim_end_matches('\r')).filter(|l| parse_placeholder(l).is_none()).collect();
    let first = kept.iter().position(|l| !blank(l));
    let last = kept.iter().rposition(|l| !blank(l));
    match (first, last) {
        (Some(f), Some(l)) => kept[f..=l].iter().map(|x| if crlf { format!("{x}\r") } else { x.to_string() }).collect(),
        _ => Vec::new(),
    }
}

/// **`content` with `ops` made, in order, all or none** (see the module
/// notes). `picture`: what the model last saw of it. A `replace`, `delete`
/// or `move` names each block by its id and the hash it read; one the
/// person changed since is refused with what's there now. Pure.
pub fn apply(content: &str, picture: Option<&str>, ops: &[Op]) -> Result<Applied, Refused> {
    let now = Doc::of(content);
    let pic = picture.map(Doc::of);
    let crlf = content.contains("\r\n");
    let lines = &now.lines;
    // The working copy: the lines before the first block, each block with
    // the blank lines before it, the lines after the last.
    // (An empty document: nothing around what goes in.)
    let (lead, tail): (Vec<String>, Vec<String>) = match (now.blocks.first(), now.blocks.last()) {
        (Some(f), Some(l)) => (lines[..f.start].to_vec(), lines[l.end..].to_vec()),
        _ => (Vec::new(), Vec::new()),
    };
    let blank_line = || if crlf { "\r".to_string() } else { String::new() };
    let mut segs: Vec<Seg> = now
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| Seg {
            lines: lines[b.start..b.end].to_vec(),
            gap: if i == 0 { Vec::new() } else { lines[now.blocks[i - 1].end..b.start].to_vec() },
            orig: Some(i),
            touched: false,
        })
        .collect();
    // Which op made which segs (by a serial: a seg's place moves).
    let mut serial = 0usize;
    let mut made_by: Vec<(usize, usize)> = Vec::new(); // (seg serial, op)
    let mut serials: Vec<Option<usize>> = vec![None; segs.len()];

    // The block an op names, in the working copy.
    let place_of = |segs: &[Seg], op: usize, id: &str, if_hash: Option<&str>| -> Result<usize, Refused> {
        if id.trim().is_empty() {
            return Err(Refused { op, why: Why::Missing("id") });
        }
        let orig = match resolve(&now, pic.as_ref(), id) {
            Found::At(i) => i,
            Found::Changed(at) => return Err(Refused { op, why: Why::Changed { id: id.trim().to_string(), now: at } }),
            Found::Unknown => return Err(Refused { op, why: Why::Unknown(id.trim().to_string()) }),
        };
        if let Some(h) = if_hash {
            if h.trim().is_empty() {
                return Err(Refused { op, why: Why::Missing("if_hash") });
            }
            if !now.ids[orig].hash.eq_ignore_ascii_case(h.trim()) {
                return Err(Refused { op, why: Why::Hash { id: id.trim().to_string(), at: orig } });
            }
        }
        // (A block an earlier op replaced or deleted isn't there as it was.)
        segs.iter().position(|s| s.orig == Some(orig)).ok_or(Refused { op, why: Why::Earlier(id.trim().to_string()) })
    };

    for (k, op) in ops.iter().enumerate() {
        match op {
            Op::Insert { markdown, at } => {
                let new = incoming(markdown, crlf);
                if new.is_empty() {
                    return Err(Refused { op: k, why: Why::Missing("markdown") });
                }
                let pos = match at {
                    Place::End => segs.len(),
                    Place::After(id) => place_of(&segs, k, id, None)? + 1,
                    Place::Before(id) => place_of(&segs, k, id, None)?,
                };
                segs.insert(pos, Seg { lines: new, gap: vec![blank_line()], orig: None, touched: true });
                serials.insert(pos, Some(serial));
                made_by.push((serial, k));
                serial += 1;
            }
            Op::Replace { id, if_hash, markdown } => {
                let pos = place_of(&segs, k, id, Some(if_hash))?;
                let new = incoming(markdown, crlf);
                if new.is_empty() {
                    return Err(Refused { op: k, why: Why::Missing("markdown") });
                }
                let seg = &mut segs[pos];
                seg.lines = new;
                seg.orig = None;
                seg.touched = true;
                serials[pos] = Some(serial);
                made_by.push((serial, k));
                serial += 1;
            }
            Op::Delete { id, if_hash } => {
                let pos = place_of(&segs, k, id, Some(if_hash))?;
                segs.remove(pos);
                serials.remove(pos);
                // The block after it keeps a blank line before it.
                if let Some(next) = segs.get_mut(pos) {
                    next.touched = true;
                }
            }
            Op::Move { ids, to } => {
                if ids.is_empty() {
                    return Err(Refused { op: k, why: Why::Missing("ids") });
                }
                let mut picked: Vec<usize> = Vec::new();
                for (id, h) in ids {
                    let pos = place_of(&segs, k, id, Some(h))?;
                    if !picked.contains(&pos) {
                        picked.push(pos);
                    }
                }
                // The anchor, before the moved blocks are taken out.
                let anchor = match to {
                    Place::End => None,
                    Place::After(id) | Place::Before(id) => {
                        let a = place_of(&segs, k, id, None)?;
                        if picked.contains(&a) {
                            return Err(Refused { op: k, why: Why::IntoItself(id.trim().to_string()) });
                        }
                        Some(a)
                    }
                };
                // Taken out in the order named, put back together.
                let mut moving: Vec<(Seg, Option<usize>)> = picked.iter().map(|p| (segs[*p].clone(), serials[*p])).collect();
                let mut desc = picked.clone();
                desc.sort_unstable_by(|a, b| b.cmp(a));
                for p in &desc {
                    segs.remove(*p);
                    serials.remove(*p);
                    if let Some(next) = segs.get_mut(*p) {
                        next.touched = true;
                    }
                }
                let below = |p: usize| desc.iter().filter(|d| **d < p).count();
                let pos = match (to, anchor) {
                    (Place::After(_), Some(a)) => a - below(a) + 1,
                    (Place::Before(_), Some(a)) => a - below(a),
                    _ => segs.len(),
                };
                for (i, (mut s, ser)) in moving.drain(..).enumerate() {
                    s.touched = true;
                    if s.gap.is_empty() {
                        s.gap = vec![blank_line()];
                    }
                    segs.insert(pos + i, s);
                    serials.insert(pos + i, ser);
                }
                if let Some(next) = segs.get_mut(pos + picked.len()) {
                    next.touched = true;
                }
            }
        }
    }

    // Put together: a blank line wherever this edit put a block next to
    // another (two paragraphs would run into one); each seg's lines noted.
    let mut out: Vec<String> = lead;
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        if i > 0 {
            if s.gap.is_empty() && (s.touched || segs[i - 1].touched) {
                out.push(blank_line());
            } else {
                out.extend(s.gap.iter().cloned());
            }
        }
        let from = out.len();
        out.extend(s.lines.iter().cloned());
        spans.push((from, out.len()));
    }
    out.extend(tail);
    let content = out.join("\n");
    let doc = Doc::of(&content);
    // Each op's new blocks: the document's blocks within its segs' lines.
    let mut made: Vec<Vec<usize>> = vec![Vec::new(); ops.len()];
    for (i, ser) in serials.iter().enumerate() {
        let Some(ser) = ser else { continue };
        let Some(&(_, k)) = made_by.iter().find(|(s, _)| s == ser) else { continue };
        let (from, to) = spans[i];
        for (bi, b) in doc.blocks.iter().enumerate() {
            if b.start >= from && b.end <= to {
                made[k].push(bi);
            }
        }
    }
    Ok(Applied { content, doc, made })
}

/// Ops from a model's arguments (`edit_document`'s `ops`: an array, or a
/// JSON string of one), under the names models give them. `Err`: what's
/// wrong with which (0 = the first).
pub fn ops_arg(args: &serde_json::Value) -> Result<Vec<Op>, (usize, String)> {
    use serde_json::Value;
    let parsed;
    let list = match &args["ops"] {
        Value::String(s) => {
            parsed = serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
            &parsed
        }
        Value::Null => match &args["edits"] {
            Value::Null => &args["operations"],
            v => v,
        },
        v => v,
    };
    let Some(items) = list.as_array() else {
        return Err((0, "ops is required: a list of edits, each {\"op\": \"insert\" | \"replace\" | \"delete\" | \"move\", …}.".to_string()));
    };
    if items.is_empty() {
        return Err((0, "ops is empty: nothing to change.".to_string()));
    }
    let s = |v: &Value, keys: &[&str]| keys.iter().find_map(|k| v[*k].as_str().map(str::to_string)).unwrap_or_default();
    let place = |v: &Value| {
        let after = s(v, &["after", "after_id"]);
        let before = s(v, &["before", "before_id"]);
        if !after.trim().is_empty() && after.trim() != "end" {
            Place::After(after)
        } else if !before.trim().is_empty() {
            Place::Before(before)
        } else {
            Place::End
        }
    };
    let mut out = Vec::new();
    for (k, v) in items.iter().enumerate() {
        let name = s(v, &["op", "type", "action"]).to_ascii_lowercase();
        let id = s(v, &["id", "block", "block_id"]);
        let hash = s(v, &["if_hash", "hash"]);
        let markdown = s(v, &["markdown", "text", "content"]);
        let op = match name.as_str() {
            "insert" | "add" => Op::Insert { markdown, at: place(v) },
            "replace" | "update" | "edit" | "rewrite" => Op::Replace { id, if_hash: hash, markdown },
            "delete" | "remove" => Op::Delete { id, if_hash: hash },
            "move" => {
                let ids: Vec<String> = match &v["ids"] {
                    Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                    _ if !id.is_empty() => vec![id.clone()],
                    _ => Vec::new(),
                };
                let hashes: Vec<String> = match &v["if_hashes"] {
                    Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                    _ if !hash.is_empty() => vec![hash.clone()],
                    _ => Vec::new(),
                };
                if ids.is_empty() {
                    return Err((k, "a move needs ids: the blocks to move, as read_document gave them.".to_string()));
                }
                if hashes.len() != ids.len() {
                    return Err((k, "a move needs if_hashes: each block's hash from read_document, in the order of ids.".to_string()));
                }
                Op::Move { ids: ids.into_iter().zip(hashes).collect(), to: place(v) }
            }
            "" => return Err((k, "each op needs \"op\": insert, replace, delete or move.".to_string())),
            other => return Err((k, format!("unknown op \"{other}\": use insert, replace, delete or move."))),
        };
        out.push(op);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Lone wolf build\n\nA fast, quiet ranger.\n\n## Gear\n\n- Two rings\n- A short bow\n\n## Levels\n\n| Level | Pick |\n|---|---|\n| 1 | Quick step |\n| 2 | Hide |\n\n```\ncode here\n\nstill code\n```\n\n> A quote\n> goes on\n\n---\n\n<!-- yap:section Tips | the tips -->\n";

    fn kinds(content: &str) -> Vec<Kind> {
        parse(content).iter().map(|b| b.kind).collect()
    }

    #[test]
    fn a_document_cuts_into_the_blocks_a_reader_sees() {
        use Kind::*;
        assert_eq!(kinds(DOC), [Heading, Paragraph, Heading, List, Heading, Table, Code, Quote, Rule, Placeholder]);
        let b = parse(DOC);
        // A whole list, a whole table, a code block with its blank line.
        assert_eq!(b[3].text, "- Two rings\n- A short bow");
        assert_eq!(b[5].text, "| Level | Pick |\n|---|---|\n| 1 | Quick step |\n| 2 | Hide |");
        assert_eq!(b[6].text, "```\ncode here\n\nstill code\n```");
        // A heading straight on top of its text, and **bold** lead-ins aren't items.
        assert_eq!(kinds("## Gear\nBoots and a cloak.\n**Note:** dry socks."), [Heading, Paragraph]);
        // A loose list stays one list; an indented line under an item is its own.
        assert_eq!(kinds("- a\n\n- b\n  more of b\n\nAfter."), [List, Paragraph]);
        // A list's numbers, and a table without a leading pipe.
        assert_eq!(kinds("1. one\n2) two"), [List]);
        assert_eq!(kinds("a | b\n--|--\n1 | 2"), [Table]);
        // A paragraph ends where a list or a heading starts.
        assert_eq!(kinds("Intro line\n- item\n# Head"), [Paragraph, List, Heading]);
        // Nothing, and only blank lines.
        assert!(parse("").is_empty());
        assert!(parse("\n\n  \n").is_empty());
    }

    #[test]
    fn ids_stay_while_a_block_is_unchanged_and_edits_elsewhere_leave_them() {
        let before = Doc::of(DOC);
        // A block added near the top, another rewritten, a section filled.
        let after = DOC
            .replace("A fast, quiet ranger.", "A fast, quiet ranger.\n\nA new line of my own.")
            .replace("- A short bow", "- A long bow")
            .replace("<!-- yap:section Tips | the tips -->", "## Tips\n\nRest often.");
        let after = Doc::of(&after);
        let kept: Vec<&str> = before.ids.iter().map(|i| i.id.as_str()).filter(|id| after.find(id).is_some()).collect();
        // Every block but the list (changed) and the placeholder (filled).
        assert_eq!(kept.len(), before.ids.len() - 2, "{:?}", before.ids);
        assert!(after.find(&before.ids[3].id).is_none());
        // The same text, the same ids, every time.
        assert_eq!(Doc::of(DOC).ids, before.ids);
        // Ids say the kind; hashes are 8 hex.
        assert!(before.ids[0].id.starts_with('h') && before.ids[5].id.starts_with('t') && before.ids[9].id.starts_with('s'));
        assert!(before.ids.iter().all(|i| i.hash.len() == 8 && i.hash.chars().all(|c| c.is_ascii_hexdigit())));
        // Two blocks saying the same: the second's id says so; unique all the same.
        let twice = Doc::of("TBD.\n\nOther.\n\nTBD.");
        assert_eq!(twice.ids[2].id, format!("{}-2", twice.ids[0].id));
        // Line endings and trailing spaces don't change a block.
        assert_eq!(Doc::of("Same text.  \r\n\r\n- a\r\n").ids, Doc::of("Same text.\n\n- a\n").ids);
    }

    #[test]
    fn what_changed_lines_up_by_block() {
        let before = Doc::of("# T\n\nOne.\n\nTwo.\n\nThree.");
        let after = Doc::of("# T\n\nOne.\n\nTwo, changed.\n\nThree.\n\nFour.");
        assert_eq!(changes(&before, &after), [Change { old: (2, 3), new: (2, 3) }, Change { old: (4, 4), new: (4, 5) }]);
        assert_eq!(line_up(&before, &after), [Some(0), Some(1), None, Some(3)]);
    }

    fn ids(d: &Doc) -> Vec<String> {
        d.ids.iter().map(|i| i.id.clone()).collect()
    }

    #[test]
    fn a_table_row_is_added_by_replacing_the_table() {
        let d = Doc::of(DOC);
        let t = d.blocks.iter().position(|b| b.kind == Kind::Table).unwrap();
        let new = format!("{}\n| 3 | Shadow step |", d.blocks[t].text);
        let ops = [Op::Replace { id: d.ids[t].id.clone(), if_hash: d.ids[t].hash.clone(), markdown: new }];
        let done = apply(DOC, Some(DOC), &ops).unwrap();
        assert!(done.content.contains("| 2 | Hide |\n| 3 | Shadow step |\n\n```"), "{}", done.content);
        // Only the table changed: every other id as it was.
        let after = ids(&done.doc);
        assert_eq!(after.len(), d.ids.len());
        assert_eq!(after.iter().zip(ids(&d)).filter(|(a, b)| *a != b).count(), 1);
        // The op's new block: the table, with its new id.
        assert_eq!(done.made[0], [t]);
        assert_eq!(done.doc.blocks[t].kind, Kind::Table);
    }

    #[test]
    fn inserts_deletes_and_moves_land_in_order() {
        let base = "# Plan\n\nIntro.\n\n## A\n\nAlpha.\n\n## B\n\nBeta.\n";
        let d = Doc::of(base);
        let id = |i: usize| d.ids[i].id.clone();
        let h = |i: usize| d.ids[i].hash.clone();
        // Insert after the intro, before a heading, at the end.
        let ops = [
            Op::Insert { markdown: "Inserted after intro.".into(), at: Place::After(id(1)) },
            Op::Insert { markdown: "\n\nBefore B.\n\n".into(), at: Place::Before(id(4)) },
            Op::Insert { markdown: "The end.".into(), at: Place::End },
        ];
        let done = apply(base, Some(base), &ops).unwrap();
        assert_eq!(done.content, "# Plan\n\nIntro.\n\nInserted after intro.\n\n## A\n\nAlpha.\n\nBefore B.\n\n## B\n\nBeta.\n\nThe end.\n");
        assert_eq!(done.made.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 1]);
        // Delete a paragraph: its blank line goes with it.
        let done = apply(base, None, &[Op::Delete { id: id(3), if_hash: h(3) }]).unwrap();
        assert_eq!(done.content, "# Plan\n\nIntro.\n\n## A\n\n## B\n\nBeta.\n");
        // Move section B (its heading and text) before A.
        let ops = [Op::Move { ids: vec![(id(4), h(4)), (id(5), h(5))], to: Place::Before(id(2)) }];
        let done = apply(base, Some(base), &ops).unwrap();
        assert_eq!(done.content, "# Plan\n\nIntro.\n\n## B\n\nBeta.\n\n## A\n\nAlpha.\n");
        // …or to the end.
        let ops = [Op::Move { ids: vec![(id(2), h(2)), (id(3), h(3))], to: Place::End }];
        let done = apply(base, Some(base), &ops).unwrap();
        assert_eq!(done.content, "# Plan\n\nIntro.\n\n## B\n\nBeta.\n\n## A\n\nAlpha.\n");
        // Into itself: refused.
        let ops = [Op::Move { ids: vec![(id(2), h(2))], to: Place::After(id(2)) }];
        assert!(matches!(apply(base, None, &ops).unwrap_err().why, Why::IntoItself(_)));
        // A heading straight on top of its text: a block put between them
        // gets its own blank lines.
        let tight = "## Gear\nBoots.";
        let t = Doc::of(tight);
        let done = apply(tight, None, &[Op::Insert { markdown: "New first line.".into(), at: Place::After(t.ids[0].id.clone()) }]).unwrap();
        assert_eq!(done.content, "## Gear\n\nNew first line.\n\nBoots.");
        // Windows line endings stay the document's.
        let crlf = "# T\r\n\r\nOne.\r\n";
        let c = Doc::of(crlf);
        let done = apply(crlf, None, &[Op::Insert { markdown: "Two.".into(), at: Place::After(c.ids[1].id.clone()) }]).unwrap();
        assert_eq!(done.content, "# T\r\n\r\nOne.\r\n\r\nTwo.\r\n");
    }

    #[test]
    fn an_edit_is_all_or_nothing_and_the_person_s_change_is_refused() {
        let base = "# Plan\n\nIntro.\n\nAlpha.\n\nBeta.\n";
        let d = Doc::of(base);
        let (alpha, alpha_h) = (d.ids[2].id.clone(), d.ids[2].hash.clone());
        let (beta, beta_h) = (d.ids[3].id.clone(), d.ids[3].hash.clone());
        // The person changed Alpha since the model read it.
        let theirs = base.replace("Alpha.", "Alpha, as I'd put it.");
        let ops = [
            Op::Replace { id: beta.clone(), if_hash: beta_h.clone(), markdown: "Beta two.".into() },
            Op::Replace { id: alpha.clone(), if_hash: alpha_h.clone(), markdown: "Alpha two.".into() },
        ];
        let r = apply(&theirs, Some(base), &ops).unwrap_err();
        assert_eq!(r.op, 1);
        // Refused, with what stands in its place now.
        let now = Doc::of(&theirs);
        match &r.why {
            Why::Changed { id, now: at } => {
                assert_eq!(id, &alpha);
                assert_eq!(at.iter().map(|i| now.blocks[*i].text.as_str()).collect::<Vec<_>>(), ["Alpha, as I'd put it."]);
            }
            other => panic!("{other:?}"),
        }
        // Removed by the person: nothing in its place.
        let gone = base.replace("Alpha.\n\n", "");
        let r = apply(&gone, Some(base), &[Op::Delete { id: alpha.clone(), if_hash: alpha_h.clone() }]).unwrap_err();
        assert_eq!(r.why, Why::Changed { id: alpha.clone(), now: Vec::new() });
        // A wrong hash: refused, the block as it is.
        let r = apply(base, Some(base), &[Op::Delete { id: beta.clone(), if_hash: "00000000".into() }]).unwrap_err();
        assert_eq!(r.why, Why::Hash { id: beta.clone(), at: 3 });
        // No hash: asked for.
        let r = apply(base, None, &[Op::Delete { id: beta.clone(), if_hash: " ".into() }]).unwrap_err();
        assert_eq!(r.why, Why::Missing("if_hash"));
        // An id nobody has.
        let r = apply(base, Some(base), &[Op::Delete { id: "p00000".into(), if_hash: "x".into() }]).unwrap_err();
        assert_eq!(r.why, Why::Unknown("p00000".into()));
        // A block an earlier op of the same edit replaced.
        let ops = [
            Op::Replace { id: beta.clone(), if_hash: beta_h.clone(), markdown: "Beta two.".into() },
            Op::Delete { id: beta.clone(), if_hash: beta_h.clone() },
        ];
        assert_eq!(apply(base, Some(base), &ops).unwrap_err().why, Why::Earlier(beta.clone()));
        // A block the model read on a page since (not in its picture), as it is now: found.
        let added = format!("{theirs}\nTheir new line.\n");
        let n = Doc::of(&added);
        let last = n.ids.last().unwrap().clone();
        assert!(apply(&added, Some(base), &[Op::Delete { id: last.id, if_hash: last.hash }]).is_ok());
    }

    #[test]
    fn a_block_the_model_saw_is_found_even_when_its_id_shifted() {
        // Two identical blocks; the person deletes the first. The model's
        // "TBD.-2" is the one still there (now plain "TBD."'s id).
        let base = "# T\n\nTBD.\n\nMiddle.\n\nTBD.\n";
        let d = Doc::of(base);
        let second = d.ids[3].clone();
        assert!(second.id.ends_with("-2"));
        let theirs = "# T\n\nMiddle.\n\nTBD.\n";
        assert_eq!(resolve(&Doc::of(theirs), Some(&d), &second.id), Found::At(2));
        // The first one, which they deleted: changed (gone), not the second.
        assert_eq!(resolve(&Doc::of(theirs), Some(&d), &d.ids[1].id), Found::Changed(Vec::new()));
        let done = apply(theirs, Some(base), &[Op::Replace { id: second.id, if_hash: second.hash, markdown: "Done.".into() }]).unwrap();
        assert_eq!(done.content, "# T\n\nMiddle.\n\nDone.\n");
    }

    #[test]
    fn placeholders_never_come_in_through_an_edit() {
        let base = "# T\n\nOne.";
        let d = Doc::of(base);
        let ops = [Op::Insert { markdown: "Two.\n<!-- yap:section Extra | more -->".into(), at: Place::After(d.ids[1].id.clone()) }];
        let done = apply(base, None, &ops).unwrap();
        assert_eq!(done.content, "# T\n\nOne.\n\nTwo.");
        // Nothing left to put in.
        let ops = [Op::Insert { markdown: "<!-- yap:section Extra -->".into(), at: Place::End }];
        assert_eq!(apply(base, None, &ops).unwrap_err().why, Why::Missing("markdown"));
    }

    #[test]
    fn ops_are_taken_as_models_send_them() {
        use serde_json::json;
        let ops = ops_arg(&json!({ "ops": [
            { "op": "insert", "markdown": "x", "after": "p1" },
            { "op": "replace", "id": "t1", "if_hash": "ab", "markdown": "y" },
            { "op": "delete", "block": "p2", "hash": "cd" },
            { "op": "move", "ids": ["h1", "p3"], "if_hashes": ["1", "2"], "before": "h2" },
            { "op": "insert", "text": "end" },
        ] }))
        .unwrap();
        assert_eq!(ops[0], Op::Insert { markdown: "x".into(), at: Place::After("p1".into()) });
        assert_eq!(ops[2], Op::Delete { id: "p2".into(), if_hash: "cd".into() });
        assert_eq!(ops[3], Op::Move { ids: vec![("h1".into(), "1".into()), ("p3".into(), "2".into())], to: Place::Before("h2".into()) });
        assert_eq!(ops[4], Op::Insert { markdown: "end".into(), at: Place::End });
        // A JSON string of them (some servers), and what's wrong said.
        assert_eq!(ops_arg(&json!({ "ops": "[{\"op\":\"delete\",\"id\":\"p1\",\"if_hash\":\"a\"}]" })).unwrap().len(), 1);
        assert_eq!(ops_arg(&json!({})).unwrap_err().0, 0);
        assert!(ops_arg(&json!({ "ops": [{ "op": "squash" }] })).unwrap_err().1.contains("unknown op"));
        assert!(ops_arg(&json!({ "ops": [{ "op": "move", "ids": ["a"] }] })).unwrap_err().1.contains("if_hashes"));
    }
}
