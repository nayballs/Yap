//! Text prep (pure): an answer's markdown as something to say — marks
//! out, a code block as "(code block)", a link as its text, a bare URL
//! as its domain, headings and bullets each their own sentence — then
//! written as words (`normalize.rs`) and cut into sentences.

use std::sync::LazyLock;

use regex::Regex;

use super::normalize;
use super::{LONG_SENTENCE, MAX_CHARS};

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(```|~~~)").unwrap());
static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([-*_])(\s*[-*_]){2,}\s*$").unwrap());
static TABLE_RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$").unwrap());
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*#{1,6}\s+").unwrap());
static QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(>\s?)+").unwrap());
static TASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:[-*+•]\s+)?\[([ xX])\]\s+").unwrap());
static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([☐☑✅✔✓])\s*").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:[-*+•–]|\d{1,3}[.)])\s+").unwrap());
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)\s]+)[^)]*\)").unwrap());
static ANGLE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<((?:https?://|www\.)[^>\s]+)>").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:https?://|www\.)[^\s<>()\[\]]+").unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`+([^`]*)`+").unwrap());
static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+)\*\*|__([^_]+)__").unwrap());
static ITALIC_STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^*\s](?:[^*]*[^*\s])?)\*").unwrap());
static ITALIC_UNDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[^\w])_([^_\s](?:[^_]*[^_\s])?)_($|[^\w])").unwrap());
static STRIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~([^~]+)~~").unwrap());
static HTML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[a-zA-Z][a-zA-Z0-9]*(?:\s[^<>]*)?/?>").unwrap());
static ARROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:→|⟶|->|=>)\s*").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
/// A web answer's citation markers ("[1]", "[2, 3]", "[1][4]", gpt-oss's
/// "【2†L6-L10】"): read as nothing.
static CITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\[\d{1,2}(?:\s*[,–-]\s*\d{1,2})*\]|【[^】]{0,40}】)").unwrap());

/// A URL as it's worth hearing: its domain ("https://www.github.com/a/b" →
/// "github.com").
fn domain(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.trim_start_matches("www.").to_ascii_lowercase()
}

/// Characters a voice reads badly or not at all: emoji, pictographs and
/// symbols, the joiners and variation selectors that build them, arrows.
fn is_unspoken(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF   // emoji, pictographs, flags' letters
        | 0x2600..=0x27BF   // misc symbols, dingbats (✓ ☐ ★ ☎)
        | 0x2B00..=0x2BFF   // ⭐ ⬆ ⭕
        | 0x2190..=0x21FF   // arrows
        | 0x2300..=0x23FF   // ⌚ ⏰ ⏸
        | 0xFE00..=0xFE0F   // variation selectors
        | 0x200D | 0x20E3   // zero-width joiner, keycap
        | 0xE0020..=0xE007F // tags
        | 0x25A0..=0x25FF   // geometric shapes (■ ▶ ●)
    )
}

/// The first of the characters a formula is kept as while the markdown
/// rules run ([`mask`]): Unicode's supplementary private use area, which no
/// text has.
const MASK: u32 = 0xF_0000;

/// Each formula in `line` (`maths::find`, as Chat draws them) as one
/// private-use character, so no markdown rule reaches inside it — `$x_1 +
/// y_1$`'s underscores, `$[0, 1]$`'s brackets, `$a*b*c$`'s stars stay —
/// and the formulas, to put back ([`unmask`]).
fn mask(line: &str) -> (String, Vec<String>) {
    let found = super::maths::find(line);
    if found.is_empty() {
        return (line.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(line.len());
    let mut kept = Vec::new();
    let mut last = 0;
    for f in found.into_iter().take(0xFFFD) {
        out.push_str(&line[last..f.range.start]);
        out.push(char::from_u32(MASK + kept.len() as u32).unwrap_or(' '));
        kept.push(line[f.range.clone()].to_string());
        last = f.range.end;
    }
    out.push_str(&line[last..]);
    (out, kept)
}

fn unmask(s: &str, kept: &[String]) -> String {
    if kept.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + kept.iter().map(String::len).sum::<usize>());
    for c in s.chars() {
        match (c as u32).checked_sub(MASK).and_then(|k| kept.get(k as usize)) {
            Some(f) => out.push_str(f),
            None => out.push(c),
        }
    }
    out
}

/// One line's inline markdown out: images by their description, links by
/// their text, bare URLs by their domain, code by its text, emphasis marks,
/// HTML tags and emoji gone, arrows read as "to". Maths stays as written
/// (`normalize::for_speech` says it; the reading card draws it).
pub(super) fn inline_text(line: &str) -> String {
    let (masked, kept) = mask(line);
    unmask(&inline_marks(&masked), &kept)
}

fn inline_marks(line: &str) -> String {
    let s = IMAGE.replace_all(line, "$1");
    // A link whose text is its address: say the domain.
    let s = LINK.replace_all(&s, |c: &regex::Captures| {
        let text = c[1].trim();
        if text.contains("://") || text.starts_with("www.") {
            domain(text)
        } else {
            text.to_string()
        }
    });
    let s = ANGLE_URL.replace_all(&s, |c: &regex::Captures| domain(&c[1]));
    let s = URL.replace_all(&s, |c: &regex::Captures| {
        let url = c[0].trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']);
        let tail = &c[0][url.len()..];
        format!("{}{}", domain(url), tail)
    });
    let s = CITE.replace_all(&s, "");
    let s = CODE.replace_all(&s, "$1");
    let s = BOLD.replace_all(&s, |c: &regex::Captures| c.get(1).or(c.get(2)).map_or("", |m| m.as_str()).to_string());
    let s = ITALIC_STAR.replace_all(&s, "$1");
    let s = ITALIC_UNDER.replace_all(&s, "$1$2$3");
    let s = STRIKE.replace_all(&s, "$1");
    let s = HTML.replace_all(&s, " ");
    let s = ARROW.replace_all(&s, " to ");
    let s: String = s.chars().map(|c| if c == '•' { ',' } else { c }).filter(|&c| !is_unspoken(c)).collect();
    SPACES.replace_all(&s, " ").trim().to_string()
}

/// A block that doesn't end like a sentence gets a full stop, so a heading
/// or a bullet is its own sentence with its own pause.
fn end_sentence(mut s: String) -> String {
    if s.chars().last().is_some_and(|c| !matches!(c, '.' | '!' | '?' | '…' | ':' | ';')) {
        s.push('.');
    }
    s
}

/// An answer's markdown as blocks to say: one per heading, list item,
/// table row or paragraph line, each ending like a sentence; a fenced code
/// block as "(code block)". Pure.
pub fn speakable_blocks(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut paragraph = String::new();
    // Inside display maths on lines of its own: the `$$` or `\]` that ends it.
    let mut in_maths: Option<&str> = None;
    let flush = |paragraph: &mut String, out: &mut Vec<String>| {
        let text = std::mem::take(paragraph);
        let text = text.trim();
        if !text.is_empty() {
            out.push(end_sentence(text.to_string()));
        }
    };
    for raw in md.lines() {
        // Display maths over lines of its own (`$$` or `\[` to a line ending
        // `$$` / `\]`, as Chat draws it): its lines as written, in the
        // paragraph they belong to (so the Voice Assistant's short answer
        // keeps "The formula:" with its formula) — a `|x|` or a `- b` in it
        // is no table or list.
        if let Some(close) = in_maths {
            if raw.trim().is_empty() {
                in_maths = None;
                flush(&mut paragraph, &mut out);
                continue;
            }
            paragraph.push(' ');
            paragraph.push_str(raw.trim());
            if raw.trim_end().ends_with(close) {
                in_maths = None;
            }
            continue;
        }
        if !in_code {
            let t = raw.trim();
            if let Some(&(open, close)) = [("$$", "$$"), ("\\[", "\\]")].iter().find(|(o, _)| t.starts_with(o)) {
                if !t[open.len()..].contains(close) {
                    if !paragraph.is_empty() {
                        paragraph.push(' ');
                    }
                    paragraph.push_str(t);
                    in_maths = Some(close);
                    continue;
                }
            }
        }
        if FENCE.is_match(raw) {
            if !in_code {
                flush(&mut paragraph, &mut out);
                out.push("(code block)".to_string());
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if raw.trim().is_empty() {
            flush(&mut paragraph, &mut out);
            continue;
        }
        if RULE.is_match(raw) || TABLE_RULE.is_match(raw) {
            flush(&mut paragraph, &mut out);
            continue;
        }
        let line = QUOTE.replace(raw, "");
        // A table row: its cells, one after another.
        if line.trim_start().starts_with('|') {
            flush(&mut paragraph, &mut out);
            let cells: Vec<String> =
                line.trim().trim_matches('|').split('|').map(inline_text).filter(|c| !c.is_empty()).collect();
            // The |---|:---:| line under the header says nothing.
            if cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
                continue;
            }
            if !cells.is_empty() {
                out.push(end_sentence(cells.join(", ")));
            }
            continue;
        }
        // A task ("- [x] …", or ☐ / ☑ as Copy text writes them): a list
        // item, a done one said so.
        let (prefix, line, task) = if let Some(m) = TASK.captures(&line) {
            let done = !m[1].trim().is_empty();
            (if done { "Done: " } else { "" }, TASK.replace(&line, "").into_owned(), true)
        } else if let Some(m) = BOX.captures(&line) {
            let done = &m[1] != "☐";
            (if done { "Done: " } else { "" }, BOX.replace(&line, "").into_owned(), true)
        } else {
            ("", line.into_owned(), false)
        };
        let block = task || HEADING.is_match(&line) || BULLET.is_match(&line);
        let line = HEADING.replace(&line, "");
        let line = BULLET.replace(&line, "");
        let text = inline_text(&line);
        if text.is_empty() {
            continue;
        }
        if block {
            flush(&mut paragraph, &mut out);
            out.push(end_sentence(format!("{prefix}{text}")));
        } else {
            // A paragraph's lines run on, as they render.
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(&text);
        }
    }
    flush(&mut paragraph, &mut out);
    out
}

/// `text` cut into sentences: after `.`, `!`, `?` or `…` (a run of them,
/// with any closing quote or bracket) and a space — never after the full
/// stop of an abbreviation ("e.g.", "Dr.", "etc.") or an initial ("J.").
/// Pure.
pub fn sentences(text: &str) -> Vec<String> {
    let text = text.trim();
    let mut out = Vec::new();
    let mut start = 0;
    // Never inside a formula (a read-aloud's sentence keeps its maths).
    let formulas = super::maths::find(text);
    let inside = |i: usize| formulas.iter().any(|f| i > f.range.start && i < f.range.end);
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if !matches!(c, '.' | '?' | '!' | '…') || inside(i) {
            continue;
        }
        let mut single_dot = c == '.';
        let mut end = i + c.len_utf8();
        while let Some(&(j, n)) = iter.peek() {
            if matches!(n, '.' | '?' | '!' | '…') {
                single_dot = false;
            } else if !matches!(n, '"' | '\'' | '”' | '’' | ')' | ']') {
                break;
            }
            end = j + n.len_utf8();
            iter.next();
        }
        let at_space = iter.peek().is_some_and(|&(_, n)| n.is_whitespace());
        if !at_space {
            continue;
        }
        let word = crate::pasting::app_kind::word_before(text, i);
        let initial = word.chars().count() == 1 && word.chars().all(char::is_uppercase);
        if single_dot && (crate::pasting::app_kind::is_abbreviation(word) || initial) {
            continue;
        }
        let s = text[start..end].trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
        start = end;
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out.into_iter().flat_map(|s| cut_long(&s)).collect()
}

/// A sentence over [`LONG_SENTENCE`] characters, cut after a comma, a
/// semicolon or a dash (else a space) so each piece starts soon.
fn cut_long(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while rest.chars().count() > LONG_SENTENCE {
        let mut limit = rest.char_indices().nth(LONG_SENTENCE).map_or(rest.len(), |(i, _)| i);
        let floor = rest.char_indices().nth(60).map_or(0, |(i, _)| i);
        // Never inside a formula: before it, or after it when it's long.
        let formulas = super::maths::find(rest);
        if let Some(f) = formulas.iter().find(|f| limit > f.range.start && limit < f.range.end) {
            limit = if f.range.start > floor { f.range.start } else { f.range.end };
        }
        let head = &rest[..limit];
        let outside = |i: usize| !formulas.iter().any(|f| i > f.range.start && i < f.range.end);
        let cut = [", ", "; ", " – ", " — ", ": "]
            .iter()
            .filter_map(|p| head.match_indices(p).map(|(i, _)| i + p.trim_end().len()).filter(|&i| outside(i)).last())
            .filter(|&i| i > floor)
            .max()
            .or_else(|| head.match_indices(' ').map(|(i, _)| i).rfind(|&i| outside(i) && i > floor))
            .unwrap_or(limit);
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// The short answer: the first `n` sentences of an answer's first block,
/// without markdown or citation markers — what the Voice Assistant says (and
/// its card shows) when the details and sources are in Chat. Pure.
pub fn brief(md: &str, n: usize) -> String {
    let first = speakable_blocks(md).into_iter().find(|b| b.chars().any(char::is_alphanumeric)).unwrap_or_default();
    sentences(&first).into_iter().take(n.max(1)).collect::<Vec<_>>().join(" ")
}

/// The start of a long answer, said before "The rest is in Chat.": its
/// blocks (paragraphs, list items) from the first while they fit in
/// `max_words`, without markdown or citation markers — it stops at the end
/// of a thought, not mid-paragraph; a first block longer than that, its
/// sentences while they fit (two at least, as [`brief`]). Pure.
pub fn lead(md: &str, max_words: usize) -> String {
    let words = |s: &str| s.split_whitespace().count();
    let blocks: Vec<String> = speakable_blocks(md).into_iter().filter(|b| b.chars().any(char::is_alphanumeric)).collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut total = 0;
    for b in &blocks {
        if total + words(b) > max_words {
            break;
        }
        total += words(b);
        kept.push(b);
    }
    if !kept.is_empty() {
        return kept.join("\n\n");
    }
    let mut said: Vec<String> = Vec::new();
    let mut total = 0;
    for s in sentences(blocks.first().map_or("", String::as_str)) {
        if said.len() >= 2 && total + words(&s) > max_words {
            break;
        }
        total += words(&s);
        said.push(s);
    }
    said.join(" ")
}

/// What to say for an answer: its [`speakable_blocks`], each written for
/// the ear (`normalize::for_speech`) and cut into [`sentences`]. Empty when
/// there's nothing to say. Pure.
pub fn utterances(md: &str) -> Vec<String> {
    let md: String = md.chars().take(MAX_CHARS).collect();
    speakable_blocks(&md).iter().flat_map(|b| sentences(&normalize::for_speech(b))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reads_naturally() {
        let md = "## Launch plan\n\nThe launch is on **Friday**, see [the doc](https://docs.example.com/plan) or https://www.github.com/acme/app/issues/4.\n\n- Fix the *login* bug\n- [ ] Write the `README`\n- [x] Ship 🎉\n1. First step\n\n```rust\nfn main() {}\n```\n\n> Quoted line\n\n---\n| Name | Due |\n|---|---|\n| Sam | Monday |";
        assert_eq!(
            speakable_blocks(md),
            vec![
                "Launch plan.",
                "The launch is on Friday, see the doc or github.com.",
                "Fix the login bug.",
                "Write the README.",
                "Done: Ship.",
                "First step.",
                "(code block)",
                "Quoted line.",
                "Name, Due.",
                "Sam, Monday.",
            ]
        );
    }

    #[test]
    fn paragraph_lines_run_on_and_marks_go() {
        assert_eq!(speakable_blocks("It moved\nto Monday."), vec!["It moved to Monday."]);
        assert_eq!(speakable_blocks("__Bold__ and _italic_ but snake_case_name stays"), vec!["Bold and italic but snake_case_name stays."]);
        assert_eq!(speakable_blocks("~~old~~ new <br> line"), vec!["old new line."]);
        assert_eq!(speakable_blocks("Friday → Monday"), vec!["Friday to Monday."]);
        assert_eq!(speakable_blocks("☐ Call Sam\n☑ Email Jo"), vec!["Call Sam.", "Done: Email Jo."]);
        assert_eq!(speakable_blocks("Great idea 👍🏽 ✅ done!"), vec!["Great idea done!"]);
        assert_eq!(speakable_blocks("👍"), Vec::<String>::new());
        assert_eq!(speakable_blocks("Unclosed\n```\ncode\nmore"), vec!["Unclosed.", "(code block)"]);
        assert_eq!(speakable_blocks("A heading:\n- one\n- two"), vec!["A heading:", "one.", "two."]);
    }

    #[test]
    fn urls_become_their_domain() {
        assert_eq!(domain("https://www.GitHub.com/a/b?c=1#d"), "github.com");
        assert_eq!(domain("http://user@host.example.org:8080/x"), "host.example.org");
        assert_eq!(domain("www.example.co.uk/page"), "example.co.uk");
        assert_eq!(inline_text("Go to <https://yap.app/docs>."), "Go to yap.app.");
        assert_eq!(inline_text("[https://a.example.com/x](https://a.example.com/x)"), "a.example.com");
        assert_eq!(inline_text("(see https://x.example.com/y)."), "(see x.example.com).");
    }

    #[test]
    fn sentences_keep_abbreviations_together() {
        assert_eq!(
            sentences("Hi Dr. Smith. It's 5 p.m. now, e.g. later. J. R. Tolkien wrote it! Really? Yes… ok."),
            vec!["Hi Dr. Smith.", "It's 5 p.m. now, e.g. later.", "J. R. Tolkien wrote it!", "Really?", "Yes…", "ok."]
        );
        assert_eq!(sentences("Version 3.5 is out. \"Great,\" she said. (Yes.) Next"), vec!["Version 3.5 is out.", "\"Great,\" she said.", "(Yes.)", "Next"]);
        assert_eq!(sentences("Wait... what?! No."), vec!["Wait...", "what?!", "No."]);
        assert_eq!(sentences("   "), Vec::<String>::new());
    }

    #[test]
    fn sentences_after_a_no_break_space() {
        // A web page's &nbsp; before a sentence's last word (read aloud).
        assert_eq!(sentences("Le train part à 15\u{a0}h. Il arrive tard."), vec!["Le train part à 15\u{a0}h.", "Il arrive tard."]);
        assert_eq!(sentences("Ask Dr.\u{a0}Smith. Then J.\u{202f}Lee."), vec!["Ask Dr.\u{a0}Smith.", "Then J.\u{202f}Lee."]);
    }

    #[test]
    fn long_sentences_are_cut_at_a_comma() {
        let long = format!("{}, {}", "word ".repeat(30).trim(), "more ".repeat(30).trim());
        let parts = sentences(&long);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with(','));
        assert!(parts.iter().all(|p| p.chars().count() <= LONG_SENTENCE));
        // No comma: at a space.
        let words = "abcdefg ".repeat(60);
        let parts = sentences(&words);
        assert!(parts.len() >= 2 && parts.iter().all(|p| p.chars().count() <= LONG_SENTENCE));
        assert_eq!(parts.join(" "), words.trim());
    }

    #[test]
    fn utterances_split_blocks_into_sentences() {
        assert_eq!(
            utterances("# Plan\nIt's Friday. Sam owns it.\n- Ask Jo"),
            vec!["Plan.", "It's Friday.", "Sam owns it.", "Ask Jo."]
        );
        assert_eq!(utterances("```\nonly code\n```"), vec!["(code block)"]);
    }

    #[test]
    fn citation_markers_are_never_read_out() {
        assert_eq!(
            utterances("Gale is on the beach [1]. He joins you [2][3], or later [2, 4]. See 【2†L6-L10】 there."),
            vec!["Gale is on the beach.", "He joins you, or later.", "See there."]
        );
        // A link's text and a list's number are kept.
        assert_eq!(inline_text("[the wiki](https://bg3.wiki/wiki/Gale) says so [1]"), "the wiki says so");
        // The short answer: the first sentences of the first block, no markers.
        let md = "Gale is on the beach west of the crash site [1]. Pull him out of the portal [1]. He's a wizard [2].\n\n## More\n- He can be romanced [3].";
        assert_eq!(brief(md, 2), "Gale is on the beach west of the crash site. Pull him out of the portal.");
        assert_eq!(brief("- Only a list [1]", 2), "Only a list.");
        assert_eq!(brief("", 2), "");
    }

    #[test]
    fn maths_is_said_in_words_and_kept_whole_for_the_card() {
        // Spoken: words, never "dollar" or "backslash".
        assert_eq!(
            utterances("The area of a circle is $A = \\pi r^2$, where $r$ is the radius."),
            vec!["The area of a circle is A equals pi r squared, where r is the radius."]
        );
        assert_eq!(utterances("It costs $5 and $10 at most."), vec!["It costs five dollars and ten dollars at most."]);
        // The markdown rules never reach inside a formula: underscores,
        // stars, brackets and a citation-looking [1] stay for the card.
        assert_eq!(inline_text("**Sum** $x_1 + y_1$ and $a*b*c$ on $[0, 1]$ or $v[1]$"), "Sum $x_1 + y_1$ and $a*b*c$ on $[0, 1]$ or $v[1]$");
        assert_eq!(utterances("So $x_1 + x_2 = 3$."), vec!["So x sub one plus x sub two equals three."]);
        // Display maths on lines of its own: its lines as written, in its
        // paragraph (the `|x|` is no table row, the `- 1` no list item), a
        // breath after it when words follow.
        let md = "The formula:\n$$\n|x| = \\sqrt{x^2}\n- 1\n$$\nThat's all.";
        assert_eq!(speakable_blocks(md), vec!["The formula: $$ |x| = \\sqrt{x^2} - 1 $$ That's all."]);
        assert_eq!(utterances(md), vec!["The formula: the absolute value of x equals the square root of x squared minus one, That's all."]);
        assert_eq!(brief("The formula:\n$$\nA = \\pi r^2\n$$\n\n## More\nText.", 2), "The formula: $$ A = \\pi r^2 $$.");
        assert_eq!(speakable_blocks("$$ E = mc^2 $$"), vec!["$$ E = mc^2 $$."]);
        assert_eq!(speakable_blocks("\\[\nx = 1\n\\]"), vec!["\\[ x = 1 \\]."]);
        assert_eq!(speakable_blocks("$$\nx = 1\n\nNot closed."), vec!["$$ x = 1.", "Not closed."]);
        // In a code block it's code.
        assert_eq!(speakable_blocks("```\n$$\n```\nok"), vec!["(code block)", "ok."]);
        // A sentence is never cut inside a formula.
        assert_eq!(sentences("We get $x = 1. 5$ here. Then more."), vec!["We get $x = 1. 5$ here.", "Then more."]);
        let long = format!("{} $${}$$ end.", "word ".repeat(30).trim(), "a, b, ".repeat(40));
        for s in sentences(&long) {
            assert_eq!(s.matches("$$").count() % 2, 0, "{s}");
        }
    }

    #[test]
    fn a_table_is_read_row_by_row_without_its_rule_line() {
        let blocks = speakable_blocks("Plans:

| Plan | Price |
|:---|---:|
| Plus | $8 |
| Premium | $30 |");
        let said = blocks.join(" ");
        assert!(said.contains("Plan, Price"), "{said}");
        assert!(said.contains("Premium"), "{said}");
        assert!(!said.contains("---") && !said.contains(":-"), "{said}");
    }
}
