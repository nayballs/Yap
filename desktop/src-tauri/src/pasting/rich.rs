//! **Rich snippets** (2026-10-31; Wispr Flow's richer snippets, by
//! behaviour): a snippet's text may carry formatting — bold, italic, links,
//! bulleted and numbered lists, line breaks — written in a small markdown
//! subset of Yap's own, so an older Yap (and config.json read by eye) still
//! has readable text. Nothing new is stored: a snippet is rich when its
//! text uses that formatting ([`is_rich`]); a plain one pastes exactly as
//! it always did.
//!
//! - **The subset** ([`parse`]): `**bold**`, `*italic*` / `_italic_` (never
//!   inside a word: `snake_case`, `5*3*2` stay as typed), `[text](url)`
//!   with an http(s) or mailto link only (anything else stays as typed),
//!   lines starting `- `, `* `, `+ ` or `• ` (a bulleted list) or `1. ` /
//!   `1) ` (a numbered one, from its first number), a backslash before
//!   punctuation for the character itself, and every line break a line
//!   break. No raw HTML: `<b>` in a snippet is the four characters.
//! - **Plain text** ([`to_plain`]): what a dictation's text gets (history,
//!   the words as said, terminals, code editors, AI chat boxes): the
//!   markers gone, bullets as "• ", numbers counted from the first, a link
//!   as "text (url)" (just the address when that's its text).
//! - **HTML** ([`dictation_html`]): built here from the parsed
//!   subset only — every text node escaped, links only http(s)/mailto —,
//!   inline (`<br>` between lines, `<b>`, `<i>`, `<a>`; `<ul>`/`<ol>` for
//!   lists) so a paste mid-paragraph joins it; a dictation that mixes
//!   spoken words and rich snippets is one fragment, the spoken words
//!   escaped. On the clipboard as **CF_HTML** ([`cf_html`], "HTML Format",
//!   its header's offsets in UTF-8 bytes) beside the plain text, in the
//!   same private, promised paste (`read_check.rs`: the app reads either).
//! - **Which apps** ([`takes_html`]): everything but terminals, code
//!   editors and AI prompt boxes (cleanup's AI prompts kind: ChatGPT,
//!   Claude, Cursor…), which get the plain text only. Apps that can't take
//!   HTML read the text as before.
//!
//! Pure and unit-tested; the frontend's twin is
//! `desktop/src/lib/views/dictionary/richSnippet.js` (the editor's preview), held
//! to the same cases.

use std::borrow::Cow;

use crate::ai::cleanup_styles::Category;
use crate::pasting::app_kind::AppKind;

// ---- The markdown subset ------------------------------------------------------------

/// A run of text inside a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Bold(Vec<Inline>),
    Italic(Vec<Inline>),
    /// `url`: http://, https:// or mailto: (checked when parsed).
    Link { text: Vec<Inline>, url: String },
}

/// A line of the snippet, or a list (consecutive items of one kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Line(Vec<Inline>),
    List { ordered: bool, start: u32, items: Vec<Vec<Inline>> },
}

/// Yap's markdown subset, line by line (see the module's notes).
pub fn parse(md: &str) -> Vec<Block> {
    let md = md.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks: Vec<Block> = Vec::new();
    for line in md.split('\n') {
        match list_item(line) {
            Some((ordered, n, rest)) => {
                let item = inlines(rest, true);
                if let Some(Block::List { ordered: o, items, .. }) = blocks.last_mut() {
                    if *o == ordered {
                        items.push(item);
                        continue;
                    }
                }
                blocks.push(Block::List { ordered, start: n, items: vec![item] });
            }
            None => blocks.push(Block::Line(inlines(line, true))),
        }
    }
    blocks
}

/// A list item's line: (numbered, its number, the text after the marker).
fn list_item(line: &str) -> Option<(bool, u32, &str)> {
    let t = line.trim_start_matches([' ', '\t']);
    let first = t.chars().next()?;
    let gap = |rest: &str| rest.starts_with([' ', '\t']);
    if matches!(first, '-' | '*' | '+' | '•') {
        let rest = &t[first.len_utf8()..];
        return gap(rest).then(|| (false, 0, rest.trim_start_matches([' ', '\t'])));
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=9).contains(&digits) {
        return None;
    }
    let rest = t[digits..].strip_prefix(['.', ')'])?;
    if !gap(rest) {
        return None;
    }
    Some((true, t[..digits].parse().ok()?, rest.trim_start_matches([' ', '\t'])))
}

/// Whether the character at `j` is escaped (an odd run of backslashes before it).
fn escaped(chars: &[char], j: usize) -> bool {
    chars[..j].iter().rev().take_while(|&&c| c == '\\').count() % 2 == 1
}

/// How many `chars[i]` there are in a row from `i`.
fn run_len(chars: &[char], i: usize) -> usize {
    chars[i..].iter().take_while(|&&c| c == chars[i]).count()
}

/// The inline runs of `s` (one line); `links`: links may start here (not
/// inside a link's own text).
fn inlines(s: &str, links: bool) -> Vec<Inline> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<Inline> = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Inline>| {
        if !buf.is_empty() {
            out.push(Inline::Text(std::mem::take(buf)));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(char::is_ascii_punctuation) {
            buf.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '[' && links {
            if let Some((label, url, end)) = link_at(&chars, i) {
                flush(&mut buf, &mut out);
                out.push(Inline::Link { text: inlines(&label, false), url });
                i = end;
                continue;
            }
        }
        if c == '*' || c == '_' {
            if let Some((node, end)) = emphasis_at(&chars, i, links) {
                flush(&mut buf, &mut out);
                out.push(node);
                i = end;
                continue;
            }
            let run = run_len(&chars, i);
            buf.extend(std::iter::repeat_n(c, run));
            i += run;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

/// `**…**` (bold) or `*…*` / `_…_` (italic) opening at `i`: the node and
/// where it ends. Never inside a word, never around spaces only.
fn emphasis_at(chars: &[char], i: usize, links: bool) -> Option<(Inline, usize)> {
    let d = chars[i];
    let run = run_len(chars, i);
    let before_ok = i == 0 || !chars[i - 1].is_alphanumeric();
    let opens = |len: usize| before_ok && chars.get(i + len).is_some_and(|n| !n.is_whitespace());
    if run >= 2 && opens(2) {
        if let Some((j, len)) = closer(chars, i + 2, d, 2) {
            // A run of three or more closes with its last two ("***x***":
            // bold around "*x*").
            let k = j + len - 2;
            if k > i + 2 {
                let inner: String = chars[i + 2..k].iter().collect();
                return Some((Inline::Bold(inlines(&inner, links)), j + len));
            }
        }
    }
    if run == 1 && opens(1) {
        if let Some((j, _)) = closer(chars, i + 1, d, 1) {
            if j > i + 1 {
                let inner: String = chars[i + 1..j].iter().collect();
                return Some((Inline::Italic(inlines(&inner, links)), j + 1));
            }
        }
    }
    None
}

/// The first run of `d` from `from` that can close `need` of them: after
/// a non-space, not followed by a letter or digit, not escaped; for one
/// (italic), a lone `d` (a `**` inside is bold's).
fn closer(chars: &[char], from: usize, d: char, need: usize) -> Option<(usize, usize)> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] != d || escaped(chars, j) {
            j += 1;
            continue;
        }
        let run = run_len(chars, j);
        let after_ok = chars.get(j + run).is_none_or(|n| !n.is_alphanumeric());
        let before_ok = !chars[j - 1].is_whitespace();
        let fits = if need == 1 { run == 1 } else { run >= need };
        if fits && after_ok && before_ok {
            return Some((j, run));
        }
        j += run;
    }
    None
}

/// `[text](url)` at `i`: its text, its link and where it ends — only for
/// an http(s) or mailto link without spaces.
fn link_at(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let close = (i + 1..chars.len()).find(|&j| chars[j] == ']' && !escaped(chars, j))?;
    if chars[i + 1..close].contains(&'[') || chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut end = None;
    for (j, &c) in chars.iter().enumerate().skip(close + 2) {
        match c {
            c if c.is_whitespace() || c.is_control() => return None,
            '(' => depth += 1,
            ')' if depth == 0 => {
                end = Some(j);
                break;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    let end = end?;
    let raw: String = chars[close + 2..end].iter().collect();
    let url = raw.strip_prefix('<').and_then(|u| u.strip_suffix('>')).unwrap_or(&raw).to_string();
    if !safe_link(&url) {
        return None;
    }
    let label: String = chars[i + 1..close].iter().collect();
    Some((label, url, end + 1))
}

/// A link a snippet may carry: http://, https:// (with somewhere to go) or
/// mailto:, nothing else (no `javascript:`, no file paths).
pub fn safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = ["http://", "https://", "mailto:"].iter().find_map(|p| lower.strip_prefix(p));
    rest.is_some_and(|r| !r.is_empty()) && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether a snippet's text uses the formatting: bold, italic, a link or a
/// list. Escapes alone don't count: such a snippet pastes as typed.
pub fn is_rich(md: &str) -> bool {
    // (Quick: none of the markers, none of it.)
    if !md.contains(['*', '_', '[', '-', '+', '•']) && !md.bytes().any(|b| b.is_ascii_digit()) {
        return false;
    }
    fn formatted(nodes: &[Inline]) -> bool {
        nodes.iter().any(|n| !matches!(n, Inline::Text(_)))
    }
    parse(md).iter().any(|b| match b {
        Block::List { .. } => true,
        Block::Line(nodes) => formatted(nodes),
    })
}

/// The text a snippet types as plain text: a rich one's [`to_plain`], a
/// plain one's own text, verbatim.
pub fn snippet_text(to: &str) -> Cow<'_, str> {
    if is_rich(to) {
        Cow::Owned(to_plain(to))
    } else {
        Cow::Borrowed(to)
    }
}

// ---- Plain text ---------------------------------------------------------------------

/// `md` as clean plain text (see the module's notes).
pub fn to_plain(md: &str) -> String {
    plain_of(&parse(md))
}

fn plain_of(blocks: &[Block]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for b in blocks {
        match b {
            Block::Line(nodes) => lines.push(plain_inlines(nodes)),
            Block::List { ordered, start, items } => {
                for (n, item) in items.iter().enumerate() {
                    let marker = if *ordered { format!("{}. ", start.saturating_add(n as u32)) } else { "• ".to_string() };
                    lines.push(format!("{marker}{}", plain_inlines(item)));
                }
            }
        }
    }
    lines.join("\n")
}

/// A link's address as a person reads it ("mailto:" off).
fn shown_url(url: &str) -> &str {
    if url.len() > 7 && url[..7].eq_ignore_ascii_case("mailto:") {
        &url[7..]
    } else {
        url
    }
}

fn plain_inlines(nodes: &[Inline]) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            Inline::Text(t) => out.push_str(t),
            Inline::Bold(inner) | Inline::Italic(inner) => out.push_str(&plain_inlines(inner)),
            Inline::Link { text, url } => {
                let label = plain_inlines(text);
                let shown = shown_url(url);
                if label.trim().is_empty() || label == shown || label == *url {
                    out.push_str(shown);
                } else {
                    out.push_str(&format!("{label} ({shown})"));
                }
            }
        }
    }
    out
}

// ---- HTML ---------------------------------------------------------------------------

/// Text escaped for HTML: `& < > " '`, a tab as a space, other control
/// characters dropped, a space after a space as a no-break space (HTML
/// would fold them into one).
fn escape_into(s: &str, out: &mut String) {
    let mut prev_space = out.ends_with(' ');
    for c in s.chars() {
        let c = if c == '\t' { ' ' } else { c };
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ' ' if prev_space => out.push_str("&nbsp;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
        prev_space = c == ' ';
    }
}

/// Text with its bare http(s) addresses as links (a rich snippet's own
/// text only; never the spoken words).
fn text_html(s: &str, autolink: bool, out: &mut String) {
    let mut rest = s;
    while let Some((start, end)) = autolink.then(|| bare_url(rest)).flatten() {
        escape_into(&rest[..start], out);
        let url = &rest[start..end];
        out.push_str("<a href=\"");
        escape_into(url, out);
        out.push_str("\">");
        escape_into(url, out);
        out.push_str("</a>");
        rest = &rest[end..];
    }
    escape_into(rest, out);
}

/// The first bare http(s) address in `s` (byte range): up to a space or a
/// `<`, `>`, `"`, without the punctuation that ends a sentence after it.
fn bare_url(s: &str) -> Option<(usize, usize)> {
    let lower = s.to_ascii_lowercase();
    let mut from = 0;
    loop {
        let at = from + lower[from..].find("http")?;
        let tail = &lower[at..];
        let scheme = if tail.starts_with("https://") {
            8
        } else if tail.starts_with("http://") {
            7
        } else {
            from = at + 4;
            continue;
        };
        if s[..at].chars().next_back().is_some_and(char::is_alphanumeric) {
            from = at + scheme;
            continue;
        }
        let len = s[at..].find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(s.len() - at);
        let mut url = &s[at..at + len];
        loop {
            let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'']);
            let trimmed = if trimmed.ends_with(')') && trimmed.matches('(').count() < trimmed.matches(')').count() {
                &trimmed[..trimmed.len() - 1]
            } else {
                trimmed
            };
            if trimmed.len() == url.len() {
                break;
            }
            url = trimmed;
        }
        if url.len() > scheme {
            return Some((at, at + url.len()));
        }
        from = at + scheme;
    }
}

fn inline_html(nodes: &[Inline], autolink: bool, out: &mut String) {
    for n in nodes {
        match n {
            Inline::Text(t) => text_html(t, autolink, out),
            Inline::Bold(inner) => {
                out.push_str("<b>");
                inline_html(inner, autolink, out);
                out.push_str("</b>");
            }
            Inline::Italic(inner) => {
                out.push_str("<i>");
                inline_html(inner, autolink, out);
                out.push_str("</i>");
            }
            Inline::Link { text, url } => {
                out.push_str("<a href=\"");
                escape_into(url, out);
                out.push_str("\">");
                if plain_inlines(text).trim().is_empty() {
                    escape_into(shown_url(url), out);
                } else {
                    inline_html(text, false, out);
                }
                out.push_str("</a>");
            }
        }
    }
}

fn html_of(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        match b {
            Block::Line(nodes) => {
                if i > 0 && matches!(blocks[i - 1], Block::Line(_)) {
                    out.push_str("<br>");
                }
                inline_html(nodes, true, &mut out);
            }
            Block::List { ordered, start, items } => {
                match (ordered, start) {
                    (false, _) => out.push_str("<ul>"),
                    (true, 1) => out.push_str("<ol>"),
                    (true, n) => out.push_str(&format!("<ol start=\"{n}\">")),
                }
                for item in items {
                    out.push_str("<li>");
                    inline_html(item, true, &mut out);
                    out.push_str("</li>");
                }
                out.push_str(if *ordered { "</ol>" } else { "</ul>" });
            }
        }
    }
    out
}

/// `md` as an HTML fragment (see the module's notes; a dictation's paste
/// goes through [`dictation_html`]).
#[cfg(test)]
pub fn to_html(md: &str) -> String {
    html_of(&parse(md))
}

// ---- A dictation's HTML -------------------------------------------------------------

/// How a snippet's plain text was found in the fitted dictation: the
/// fit may have changed its first letter's case (mid-sentence) or taken
/// its final full stop off (a chat message's, before text after the
/// cursor).
#[derive(Clone, Copy)]
struct Found {
    at: usize,
    len: usize,
    case: bool,
    no_stop: bool,
}

/// `plain` in `hay`: as it is, then with its first letter's case the
/// other way, then without its final full stop (both ways).
fn find_fitted(hay: &str, plain: &str) -> Option<Found> {
    if plain.is_empty() {
        return None;
    }
    let flipped = |s: &str| -> Option<String> {
        let mut chars = s.chars();
        let first = chars.next().filter(|c| c.is_alphabetic())?;
        let other: String = if first.is_uppercase() { first.to_lowercase().collect() } else { first.to_uppercase().collect() };
        (other != first.to_string()).then(|| format!("{other}{}", chars.as_str()))
    };
    let stopless = plain.strip_suffix('.').filter(|s| !s.is_empty() && !s.ends_with('.'));
    let mut tries: Vec<(String, bool, bool)> = vec![(plain.to_string(), false, false)];
    if let Some(f) = flipped(plain) {
        tries.push((f, true, false));
    }
    if let Some(s) = stopless {
        tries.push((s.to_string(), false, true));
        if let Some(f) = flipped(s) {
            tries.push((f, true, true));
        }
    }
    tries
        .into_iter()
        .find_map(|(t, case, no_stop)| hay.find(&t).map(|at| Found { at, len: t.len(), case, no_stop }))
}

/// The first (or last) non-empty text node, to fit as the plain text was.
fn edge_text(blocks: &mut [Block], first: bool) -> Option<&mut String> {
    fn has_text(n: &Inline) -> bool {
        match n {
            Inline::Text(t) => !t.is_empty(),
            Inline::Bold(inner) | Inline::Italic(inner) | Inline::Link { text: inner, .. } => inner.iter().any(has_text),
        }
    }
    fn in_nodes(nodes: &mut [Inline], first: bool) -> Option<&mut String> {
        let at = if first { nodes.iter().position(has_text)? } else { nodes.iter().rposition(has_text)? };
        match &mut nodes[at] {
            Inline::Text(t) => Some(t),
            Inline::Bold(inner) | Inline::Italic(inner) | Inline::Link { text: inner, .. } => in_nodes(inner, first),
        }
    }
    let block = if first { blocks.first_mut()? } else { blocks.last_mut()? };
    match block {
        Block::Line(nodes) => in_nodes(nodes, first),
        Block::List { items, .. } => {
            let item = if first { items.first_mut()? } else { items.last_mut()? };
            in_nodes(item, first)
        }
    }
}

/// The parsed snippet changed as `found` says the fit changed its plain
/// text; `None` when it then doesn't read as `expect` (the HTML would say
/// something else than the text).
fn fitted(mut blocks: Vec<Block>, found: Found, expect: &str) -> Option<Vec<Block>> {
    if found.case {
        let t = edge_text(&mut blocks, true)?;
        let mut chars = t.chars();
        let first = chars.next()?;
        let other: String = if first.is_uppercase() { first.to_lowercase().collect() } else { first.to_uppercase().collect() };
        *t = format!("{other}{}", chars.as_str());
    }
    if found.no_stop {
        let t = edge_text(&mut blocks, false)?;
        t.strip_suffix('.')?;
        t.pop();
    }
    (plain_of(&blocks) == expect).then_some(blocks)
}

/// A spoken part of a dictation as HTML: escaped, line breaks as `<br>`;
/// a space at the very start or end of the paste a no-break space (an
/// app's paste would drop it, and smart spacing put it there).
fn spoken_html(s: &str, at_start: bool, at_end: bool, out: &mut String) {
    let lines: Vec<&str> = s.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("<br>");
        }
        let line = line.trim_end_matches('\r');
        let lead = at_start && i == 0 && line.starts_with(' ');
        let mut body = line;
        if lead {
            out.push_str("&nbsp;");
            body = &body[1..];
        }
        let trail = at_end && i == lines.len() - 1 && body.ends_with(' ');
        if trail {
            body = &body[..body.len() - 1];
        }
        escape_into(body, out);
        if trail {
            out.push_str("&nbsp;");
        }
    }
}

/// The HTML for a fitted dictation (`text`, exactly what's pasted as
/// plain text) that has rich snippets in it (`used`: their markdown, in
/// the order they were expanded): one fragment, the spoken words escaped
/// around each snippet's HTML. `None` when a snippet's text can't be found
/// in it as the fit left it (the paste is then plain text only, as ever).
pub fn dictation_html(text: &str, used: &[String]) -> Option<String> {
    if used.is_empty() {
        return None;
    }
    enum Seg<'a> {
        Spoken(&'a str),
        Snippet(Vec<Block>),
    }
    let mut segs: Vec<Seg> = Vec::new();
    let mut rest = text;
    for md in used {
        let blocks = parse(md);
        let plain = plain_of(&blocks);
        let found = find_fitted(rest, &plain)?;
        let expect = &rest[found.at..found.at + found.len];
        let blocks = fitted(blocks, found, expect)?;
        segs.push(Seg::Spoken(&rest[..found.at]));
        segs.push(Seg::Snippet(blocks));
        rest = &rest[found.at + found.len..];
    }
    segs.push(Seg::Spoken(rest));
    let starts_list = |s: Option<&Seg>| matches!(s, Some(Seg::Snippet(b)) if matches!(b.first(), Some(Block::List { .. })));
    let ends_list = |s: Option<&Seg>| matches!(s, Some(Seg::Snippet(b)) if matches!(b.last(), Some(Block::List { .. })));
    let mut out = String::new();
    let last = segs.len() - 1;
    for (i, seg) in segs.iter().enumerate() {
        match seg {
            Seg::Snippet(blocks) => out.push_str(&html_of(blocks)),
            Seg::Spoken(s) => {
                // A list is a block of its own: the line break (and spaces)
                // between it and the words is the list's edge.
                let mut s: &str = s;
                if i > 0 && ends_list(segs.get(i - 1)) {
                    s = s.trim_start_matches(' ');
                    s = s.strip_prefix("\r\n").or_else(|| s.strip_prefix('\n')).unwrap_or(s);
                }
                if starts_list(segs.get(i + 1)) {
                    s = s.trim_end_matches(' ');
                    s = s.strip_suffix("\r\n").or_else(|| s.strip_suffix('\n')).unwrap_or(s);
                }
                spoken_html(s, i == 0, i == last, &mut out);
            }
        }
    }
    Some(out)
}

// ---- CF_HTML ------------------------------------------------------------------------

const CF_PRE: &str = "<html>\r\n<head><meta charset=\"utf-8\"></head>\r\n<body>\r\n<!--StartFragment-->";
const CF_POST: &str = "<!--EndFragment-->\r\n</body>\r\n</html>";

/// `fragment` as the clipboard's "HTML Format" (CF_HTML): the header with
/// the byte offsets of the HTML and of the fragment (UTF-8, ten digits
/// each, as Chromium and Word write them), the page around the fragment,
/// and a NUL after it (outside the offsets).
pub fn cf_html(fragment: &str) -> Vec<u8> {
    let header = |a: usize, b: usize, c: usize, d: usize| {
        format!("Version:0.9\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n")
    };
    let start_html = header(0, 0, 0, 0).len();
    let start_fragment = start_html + CF_PRE.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + CF_POST.len();
    let mut s = header(start_html, end_html, start_fragment, end_fragment);
    s.push_str(CF_PRE);
    s.push_str(fragment);
    s.push_str(CF_POST);
    let mut bytes = s.into_bytes();
    bytes.push(0);
    bytes
}

// ---- Which apps take it -------------------------------------------------------------

/// Code editors and IDEs, by process name (lower case, no ".exe"): they
/// get a rich snippet's plain text only.
const CODE_EDITORS: &[&str] = &[
    "code",
    "code - insiders",
    "code - oss",
    "vscodium",
    "codium",
    "cursor",
    "windsurf",
    "positron",
    "trae",
    "kiro",
    "antigravity",
    "azuredatastudio",
    "devenv",
    "zed",
    "sublime_text",
    "notepad++",
    "idea",
    "idea64",
    "pycharm",
    "pycharm64",
    "webstorm64",
    "phpstorm64",
    "clion64",
    "goland64",
    "rider64",
    "rubymine64",
    "datagrip64",
    "dataspell64",
    "rustrover64",
    "studio64",
    "fleet",
    "eclipse",
    "netbeans64",
    "gvim",
    "nvim-qt",
    "neovide",
    "emacs",
    "runemacs",
    "lapce",
    "kate",
    "geany",
    "uedit64",
];

/// Whether a rich snippet's HTML goes on the clipboard for this app:
/// never a terminal (`kind`, as read), a code editor (`exe`: the process
/// name, lower case, no ".exe") or an AI prompt box (`category`, cleanup's
/// kind of app: ChatGPT, Claude, Copilot… on the desktop or in a browser
/// tab, and the coding CLIs) — they get the plain text. Everything else
/// does: Outlook, Word, OneNote, Teams, Slack, Gmail and other web mail,
/// and any app that only reads text reads the text as before.
pub fn takes_html(kind: AppKind, exe: &str, category: Category) -> bool {
    if kind.is_terminal() || matches!(category, Category::AiPrompts | Category::Terminal) {
        return false;
    }
    let stem = crate::pasting::app_kind::exe_stem(exe);
    !CODE_EDITORS.contains(&stem.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pasting::app_kind::{Host, Terminal};

    fn t(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn the_subset_parses_bold_italic_links_and_lists() {
        assert_eq!(
            parse("**Thanks**, *Sam* and _you_"),
            vec![Block::Line(vec![
                Inline::Bold(vec![t("Thanks")]),
                t(", "),
                Inline::Italic(vec![t("Sam")]),
                t(" and "),
                Inline::Italic(vec![t("you")]),
            ])]
        );
        assert_eq!(
            parse("[Book a call](https://example.com/book)"),
            vec![Block::Line(vec![Inline::Link { text: vec![t("Book a call")], url: "https://example.com/book".into() }])]
        );
        assert_eq!(
            parse("Agenda:\n- one\n* two\n1. three\n2) four"),
            vec![
                Block::Line(vec![t("Agenda:")]),
                Block::List { ordered: false, start: 0, items: vec![vec![t("one")], vec![t("two")]] },
                Block::List { ordered: true, start: 1, items: vec![vec![t("three")], vec![t("four")]] },
            ]
        );
        // Bold and italic together, either way round.
        assert_eq!(parse("***both***"), vec![Block::Line(vec![Inline::Bold(vec![Inline::Italic(vec![t("both")])])])]);
        assert_eq!(
            parse("*a **b** c*"),
            vec![Block::Line(vec![Inline::Italic(vec![t("a "), Inline::Bold(vec![t("b")]), t(" c")])])]
        );
        // A link's text may be bold; a link inside a link's text isn't one.
        assert_eq!(
            parse("[**Join**](https://example.com/room)"),
            vec![Block::Line(vec![Inline::Link {
                text: vec![Inline::Bold(vec![t("Join")])],
                url: "https://example.com/room".into()
            }])]
        );
        // Parentheses inside an address.
        assert_eq!(
            parse("[wiki](https://example.com/a_(b))"),
            vec![Block::Line(vec![Inline::Link { text: vec![t("wiki")], url: "https://example.com/a_(b)".into() }])]
        );
    }

    #[test]
    fn what_isnt_formatting_stays_as_typed() {
        for plain in [
            "snake_case_name",
            "5*3*2 = 30",
            "2 * 3 * 4",
            "Terms apply*",
            "** not bold **",
            "x * not italic*",
            "x *not italic *",
            "a lone [bracket] (and a note)",
            "1 Long Lane\nLeeds",
            "Speak soon,\nRobin",
            "-- \nSam",
            "-5 degrees",
            "+44 7700 900123",
            "£5 each",
            "C:\\Users\\sam",
            "",
        ] {
            assert!(!is_rich(plain), "{plain:?} isn't rich");
            assert_eq!(snippet_text(plain), plain, "{plain:?} types as it is");
        }
        // Links that aren't http(s) or mailto are text.
        for bad in ["[x](javascript:alert(1))", "[x](file:///C:/a)", "[x](ftp://example.com)", "[x](https://)", "[x](https://a b)"] {
            assert!(!is_rich(bad), "{bad:?}");
            assert_eq!(to_plain(bad), bad);
        }
        // Escapes: the character itself, and alone they're not formatting.
        assert!(!is_rich("\\*not italic\\*"));
        assert_eq!(to_plain("**a\\*b** and \\[c\\]"), "a*b and [c]");
    }

    #[test]
    fn plain_text_drops_the_markers_and_writes_bullets_and_links_out() {
        assert_eq!(
            to_plain("Hi **Sam**,\n\nThe plan:\n- *Monday*: [the agenda](https://example.com/agenda)\n- Friday\n\nBest,\nRobin"),
            "Hi Sam,\n\nThe plan:\n• Monday: the agenda (https://example.com/agenda)\n• Friday\n\nBest,\nRobin"
        );
        // Numbers counted from the first.
        assert_eq!(to_plain("3. c\n3. d\n3. e"), "3. c\n4. d\n5. e");
        // A link whose text is its address, or an email address.
        assert_eq!(to_plain("[https://example.com](https://example.com)"), "https://example.com");
        assert_eq!(to_plain("[sam@example.com](mailto:sam@example.com)"), "sam@example.com");
        assert_eq!(to_plain("[Email Sam](mailto:sam@example.com)"), "Email Sam (sam@example.com)");
        assert_eq!(to_plain("[](https://example.com)"), "https://example.com");
        // `•` lines (pasted from somewhere) are a list too.
        assert_eq!(to_plain("• **a**\n• b"), "• a\n• b");
    }

    #[test]
    fn html_is_built_from_the_subset_and_every_text_escaped() {
        assert_eq!(
            to_html("**Thanks**, *Sam*\n[Book](https://example.com/book?a=1&b=2)"),
            "<b>Thanks</b>, <i>Sam</i><br><a href=\"https://example.com/book?a=1&amp;b=2\">Book</a>"
        );
        assert_eq!(to_html("Plan:\n- a\n- b\nBye"), "Plan:<ul><li>a</li><li>b</li></ul>Bye");
        assert_eq!(to_html("2. b\n3. c"), "<ol start=\"2\"><li>b</li><li>c</li></ol>");
        assert_eq!(to_html("1. a"), "<ol><li>a</li></ol>");
        // Raw HTML in a snippet is text.
        assert_eq!(
            to_html("**<script>alert(\"x\")</script>** & 'more'"),
            "<b>&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;</b> &amp; &#39;more&#39;"
        );
        assert_eq!(to_html("- <img src=x onerror=alert(1)>"), "<ul><li>&lt;img src=x onerror=alert(1)&gt;</li></ul>");
        // An address's quotes can't leave the attribute.
        let html = to_html("[x](https://example.com/\"onmouseover=\"alert(1))");
        assert!(html.starts_with("<a href=\"https://example.com/&quot;onmouseover=&quot;alert(1)\">"), "{html}");
        // A bare address in a rich snippet is a link; a sentence's full stop
        // after it isn't part of it.
        assert_eq!(
            to_html("**Join**: https://example.com/room."),
            "<b>Join</b>: <a href=\"https://example.com/room\">https://example.com/room</a>."
        );
        // Blank lines and spaces kept.
        assert_eq!(to_html("**a**\n\nb  c"), "<b>a</b><br><br>b &nbsp;c");
    }

    #[test]
    fn a_dictation_is_one_fragment_with_the_spoken_words_escaped() {
        let sign_off = "Best wishes,\n**Robin**\n[example.com](https://example.com)".to_string();
        let plain = to_plain(&sign_off);
        let text = format!("Thanks for <today> & \"all\". {plain}");
        assert_eq!(
            dictation_html(&text, std::slice::from_ref(&sign_off)).as_deref(),
            Some(
                "Thanks for &lt;today&gt; &amp; &quot;all&quot;. Best wishes,<br><b>Robin</b><br><a href=\"https://example.com\">example.com</a>"
            )
        );
        // Two snippets, and the same one twice, in order.
        let bold = "**Zentrix**".to_string();
        let text = "Zentrix and Zentrix, said".to_string();
        assert_eq!(
            dictation_html(&text, &[bold.clone(), bold.clone()]).as_deref(),
            Some("<b>Zentrix</b> and <b>Zentrix</b>, said")
        );
        // A list snippet after words on their own line: the list is the break.
        let list = "- **one**\n- two".to_string();
        assert_eq!(
            dictation_html("Here you go:\n• one\n• two\nThanks", std::slice::from_ref(&list)).as_deref(),
            Some("Here you go:<ul><li><b>one</b></li><li>two</li></ul>Thanks")
        );
        // Smart spacing's space before it, at the paste's start, is kept.
        assert_eq!(dictation_html(" Zentrix ", std::slice::from_ref(&bold)).as_deref(), Some("&nbsp;<b>Zentrix</b>&nbsp;"));
        // Nothing rich used: no HTML.
        assert_eq!(dictation_html("plain words", &[]), None);
    }

    #[test]
    fn the_fit_may_change_a_snippets_first_letter_or_take_its_full_stop() {
        let md = "**Thanks** for *everything*.".to_string();
        // Mid-sentence: lower case.
        assert_eq!(
            dictation_html("and thanks for everything.", std::slice::from_ref(&md)).as_deref(),
            Some("and <b>thanks</b> for <i>everything</i>.")
        );
        // A chat message's full stop left off.
        assert_eq!(dictation_html("Thanks for everything", std::slice::from_ref(&md)).as_deref(), Some("<b>Thanks</b> for <i>everything</i>"));
        assert_eq!(dictation_html("ok thanks for everything", std::slice::from_ref(&md)).as_deref(), Some("ok <b>thanks</b> for <i>everything</i>"));
        // Not there as it was (something else rewrote it): no HTML, the
        // plain text pastes as ever.
        assert_eq!(dictation_html("Thanks for nothing", std::slice::from_ref(&md)), None);
        // The second one missing: none of it.
        assert_eq!(dictation_html("Thanks for everything.", &[md.clone(), md]), None);
    }

    /// The header's numbers, read back.
    fn offsets(bytes: &[u8]) -> (usize, usize, usize, usize) {
        let text = std::str::from_utf8(bytes).unwrap();
        let get = |name: &str| -> usize {
            let at = text.find(&format!("{name}:")).unwrap() + name.len() + 1;
            text[at..at + 10].parse().unwrap()
        };
        (get("StartHTML"), get("EndHTML"), get("StartFragment"), get("EndFragment"))
    }

    #[test]
    fn cf_html_header_offsets_point_at_the_html_and_the_fragment() {
        for fragment in ["<b>Robin</b>", "Café — naïve 日本語 👋 <i>x</i>", ""] {
            let bytes = cf_html(fragment);
            assert_eq!(bytes.last(), Some(&0), "NUL-terminated");
            let body = &bytes[..bytes.len() - 1];
            assert!(body.starts_with(b"Version:0.9\r\n"));
            let (start_html, end_html, start_fragment, end_fragment) = offsets(body);
            assert_eq!(end_html, body.len(), "EndHTML is the end");
            assert!(body[start_html..].starts_with(b"<html>"), "StartHTML at <html>");
            assert!(body[..end_html].ends_with(b"</html>"));
            assert_eq!(&body[start_fragment..end_fragment], fragment.as_bytes(), "the fragment, byte for byte (UTF-8)");
            assert!(body[..start_fragment].ends_with(b"<!--StartFragment-->"));
            assert!(body[end_fragment..].starts_with(b"<!--EndFragment-->"));
            assert!(start_html < start_fragment && start_fragment <= end_fragment && end_fragment < end_html);
        }
    }

    #[test]
    fn a_snippet_with_script_or_quotes_pastes_as_text() {
        let md = "**Hi** <script>alert('x')</script> \"quoted\"".to_string();
        let plain = to_plain(&md);
        assert_eq!(plain, "Hi <script>alert('x')</script> \"quoted\"");
        let html = dictation_html(&format!("<b>said</b> {plain}"), &[md]).unwrap();
        assert_eq!(
            html,
            "&lt;b&gt;said&lt;/b&gt; <b>Hi</b> &lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &quot;quoted&quot;"
        );
        let cf = cf_html(&html);
        assert!(!String::from_utf8_lossy(&cf).contains("<script"), "no script element on the clipboard");
    }

    #[test]
    fn html_goes_to_mail_documents_and_chats_never_terminals_editors_or_ai_boxes() {
        let term = AppKind::Terminal(Terminal { coding_cli: None, host: Host::WindowsTerminal });
        for (exe, category) in [
            ("OUTLOOK.EXE", Category::Email),
            ("olk.exe", Category::Email),
            ("WINWORD.EXE", Category::Documents),
            ("ONENOTE.EXE", Category::Documents),
            ("ms-teams.exe", Category::WorkChat),
            ("slack.exe", Category::WorkChat),
            ("msedge.exe", Category::Email), // Gmail in a tab
            ("chrome.exe", Category::Other),
            ("notepad.exe", Category::Other),
        ] {
            assert!(takes_html(AppKind::Other, exe, category), "{exe}");
        }
        assert!(takes_html(AppKind::Messaging, "slack.exe", Category::WorkChat));
        assert!(!takes_html(term, "WindowsTerminal.exe", Category::Terminal));
        assert!(!takes_html(term, "Code.exe", Category::Other), "VS Code's terminal");
        for exe in ["Code.exe", "Cursor.exe", "devenv.exe", "idea64.exe", "notepad++.exe", "sublime_text.exe", "zed.exe"] {
            assert!(!takes_html(AppKind::Other, exe, Category::Other), "{exe}");
        }
        // ChatGPT or Claude, on the desktop or in a browser tab.
        assert!(!takes_html(AppKind::Other, "chatgpt.exe", Category::AiPrompts));
        assert!(!takes_html(AppKind::Other, "msedge.exe", Category::AiPrompts));
    }
}
