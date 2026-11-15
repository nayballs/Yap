//! **Find by description** (15 Nov 2026): `browser_find` given a plain
//! description — "the button that adds it to the basket", "the search box
//! at the top", "the second result's link" — as Claude Code's own browser's
//! find takes one ("search bar", "login button"). Built from what that tool
//! is documented to take, never from its code.
//!
//! **Deterministic**: no model call. The page's `scene` op (`act/script.rs`;
//! the stand-in browser's, `sim.rs`) lists what's there — its links,
//! buttons and fields, headings, parts (tables, forms, landmarks) and text
//! blocks, each with its ref, role, accessible name, label, placeholder,
//! description, class and id words, where it is on the page, the heading
//! it's under and the parts it's inside (a navigation, a form, a dialog, the
//! 2nd of 10 items like it) — through open shadow roots and same-origin
//! frames (`deep_js!`). [`parse`] reads the description into what's wanted
//! ([`Wanted`]): a role ("button", "search box", "link"), words ("adds",
//! "basket", stemmed, a few synonyms folded: basket = cart), a place ("at
//! the top", "on the left"), an ordinal ("the second", "the last"), the
//! parts it's in ("inside the navigation", "in the sign-up form", "the 2nd
//! result's", "under Delivery") and what it's next to. [`rank`] scores every
//! candidate and keeps the best few, each with **why** it matched — the
//! calling model judges between them, as it would between a read's lines.
//!
//! English descriptions (the words a model writes); the page itself may be
//! in any language — its words are matched as they are.
//!
//! The log has counts only, never a page's words or the description.

use serde::Deserialize;

/// The most a description's ranking lists.
pub const MAX: usize = 5;

/// **One thing on the page** `browser_find` may point at, as the `scene` op
/// reads it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Thing {
    #[serde(rename = "ref")]
    pub r: u32,
    /// Its role: link, button, textbox, searchbox, heading, img, table,
    /// navigation, paragraph… (as the accessibility tree has it).
    pub role: String,
    pub tag: String,
    /// "control" (a link, button or field), "heading", "part" (a table, a
    /// form, a landmark, a picture) or "text" (a block of text).
    pub kind: String,
    /// Its accessible name (a block's: its words).
    pub name: String,
    pub label: String,
    pub placeholder: String,
    /// Its title and `aria-describedby` words.
    pub desc: String,
    /// What's in it (never a sensitive field's: left out by the script).
    pub value: String,
    /// Its id, class, `name` and test id words ("add-to-cart").
    pub hints: String,
    pub href: String,
    /// A heading's level (1–6).
    pub level: u8,
    /// The page marks it as a search box.
    pub search: bool,
    pub disabled: bool,
    /// Where it is on the page (CSS pixels from the page's top left).
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// In view now.
    pub seen: bool,
    /// The heading it's under (the nearest before it).
    pub heading: String,
    /// The parts it's inside ([`Holder::id`]s), the nearest first.
    pub within: Vec<u32>,
}

/// **A part of the page something is inside**: a landmark, a form, a
/// dialog, a table's row, a list's item — or one of several like it (a
/// result, a product card: [`Holder::item`] of [`Holder::of`]).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Holder {
    /// Its number in this read.
    pub id: u32,
    /// navigation, banner, contentinfo, complementary, main, form, dialog,
    /// region, search, article, table, row, list, listitem, group, menu…
    pub role: String,
    /// Its label: `aria-label`, its heading, a form's legend or button.
    pub name: String,
    /// Its id and class words ("product-card", "search-result").
    pub words: String,
    /// One of several like it in its parent: its place (1 the first; 0: not one).
    pub item: u32,
    pub of: u32,
    /// The set of those like it (the same number for each).
    pub group: u32,
    pub y: f64,
}

/// **What the `scene` op reads**: the things and the parts they're in, the
/// page's height and the view's size.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub things: Vec<Thing>,
    pub holders: Vec<Holder>,
    /// The page's height (CSS pixels).
    pub h: f64,
    /// The view's width and height.
    pub w: f64,
    pub vh: f64,
}

/// **The kind of thing a description asks for.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Button,
    Link,
    Field,
    Checkbox,
    Radio,
    Dropdown,
    Tab,
    Switch,
    Slider,
    Heading,
    Image,
    Text,
    Option,
    Frame,
    /// A part of the page: a form, a table, a navigation, a dialog…
    Part(Part),
}

/// **A part of the page**, as a description names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Banner,
    Footer,
    Sidebar,
    Navigation,
    Main,
    Form,
    Dialog,
    Table,
    List,
    Region,
}

impl Part {
    /// Whether a holder's or a thing's role is this part. Pure.
    fn is(self, role: &str) -> bool {
        match self {
            Part::Banner => role == "banner",
            Part::Footer => role == "contentinfo",
            Part::Sidebar => role == "complementary",
            Part::Navigation => matches!(role, "navigation" | "menubar" | "menu"),
            Part::Main => role == "main",
            Part::Form => matches!(role, "form" | "search"),
            Part::Dialog => matches!(role, "dialog" | "alertdialog"),
            Part::Table => matches!(role, "table" | "grid" | "treegrid"),
            Part::List => matches!(role, "list" | "listbox"),
            Part::Region => matches!(role, "region" | "article" | "group" | "search" | "form" | "complementary" | "dialog" | "tabpanel"),
        }
    }
}

/// **Which one**: the nth (1 the first), or the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nth {
    At(u32),
    Last,
}

impl Nth {
    /// "2nd", "last". Pure.
    fn word(self) -> String {
        match self {
            Nth::Last => "last".into(),
            Nth::At(n) => format!("{n}{}", ordinal_suffix(n)),
        }
    }
}

fn ordinal_suffix(n: u32) -> &'static str {
    match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    }
}

/// **A part a description says it's in** ("inside the navigation", "in the
/// sign-up form", "the second result's", "under Delivery").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Within {
    pub part: Option<Part>,
    /// One of several like it: a result, an item, a product, a card, a row…
    pub item: bool,
    pub nth: Option<Nth>,
    /// Its words ("sign up", "delivery"; an item's own word, "result", too).
    pub words: Vec<String>,
    /// "under …": its heading as well as a part.
    pub under: bool,
}

/// **Something a description says it's beside** ("next to Quantity",
/// "below the price"): `after` it comes after those words on the page,
/// `before` it comes before them; neither: either.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Near {
    pub words: Vec<String>,
    pub after: bool,
    pub before: bool,
}

/// **Where on the page**: up (top) or down (bottom), left or right.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Place {
    /// -1 the top, 1 the bottom.
    pub v: i8,
    /// -1 the left, 1 the right.
    pub h: i8,
}

impl Place {
    pub fn any(self) -> bool {
        self.v != 0 || self.h != 0
    }
}

/// **What a description asks for** ([`parse`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wanted {
    pub kind: Option<Kind>,
    /// The thing is itself one of several like it ("the second result").
    pub item: bool,
    /// Its words, as [`terms`] makes them.
    pub words: Vec<String>,
    /// Words given in quotes: on it exactly (folded).
    pub quoted: Vec<String>,
    pub nth: Option<Nth>,
    pub place: Place,
    pub within: Vec<Within>,
    pub near: Vec<Near>,
    /// "the main heading": the page's first-level one.
    pub main: bool,
    /// It names a place, an ordinal, the parts it's in or what it's beside:
    /// a description whatever words the page has.
    pub described: bool,
}

/// Words a description has that say nothing about the thing.
const STOP: &[&str] = &[
    "the", "a", "an", "that", "which", "who", "whose", "it", "its", "this", "these", "those", "to", "and", "or", "me", "my", "our", "your", "you",
    "i", "we", "us", "one", "ones", "thing", "things", "element", "elements", "please", "click", "clicking", "press", "pressing", "tap", "choose",
    "pick", "find", "locate", "where", "is", "are", "be", "there", "here", "can", "could", "would", "should", "will", "just", "some", "any",
    "called", "named", "labelled", "labeled", "says", "say", "saying", "reads", "reading", "shows", "showing", "with", "has", "have", "having",
    "page", "screen", "website", "site", "icon", "symbol", "little", "small", "big", "large", "lets", "let", "so", "if", "when", "on", "at", "by",
    "into", "onto", "up", "out", "then", "also", "only", "own", "goes", "go", "takes", "take", "used", "use", "for", "of", "in",
];

/// Ordinal words.
fn ordinal(w: &str) -> Option<Nth> {
    let n = match w {
        "first" | "1st" | "top-most" => 1,
        "second" | "2nd" => 2,
        "third" | "3rd" => 3,
        "fourth" | "4th" => 4,
        "fifth" | "5th" => 5,
        "sixth" | "6th" => 6,
        "seventh" | "7th" => 7,
        "eighth" | "8th" => 8,
        "ninth" | "9th" => 9,
        "tenth" | "10th" => 10,
        "last" | "final" => return Some(Nth::Last),
        _ => return None,
    };
    Some(Nth::At(n))
}

/// A role word's kind ("button", "box", "dropdown"…). Pure.
fn kind_word(w: &str) -> Option<Kind> {
    Some(match w {
        "button" | "buttons" | "btn" => Kind::Button,
        "link" | "links" | "hyperlink" | "anchor" => Kind::Link,
        "field" | "fields" | "box" | "boxes" | "input" | "inputs" | "textbox" => Kind::Field,
        "checkbox" | "checkboxes" => Kind::Checkbox,
        "radio" | "radios" => Kind::Radio,
        "dropdown" | "dropdowns" | "select" | "combobox" | "picker" => Kind::Dropdown,
        "tab" | "tabs" => Kind::Tab,
        "toggle" | "switch" => Kind::Switch,
        "slider" | "range" => Kind::Slider,
        "heading" | "headings" | "headline" | "title" => Kind::Heading,
        "image" | "images" | "picture" | "pictures" | "photo" | "photos" | "img" | "figure" => Kind::Image,
        "paragraph" | "paragraphs" | "text" | "sentence" | "line" => Kind::Text,
        "option" | "options" => Kind::Option,
        "frame" | "iframe" => Kind::Frame,
        _ => return part_word(w).map(Kind::Part),
    })
}

/// A part word's part ("header", "nav", "form"…). Pure.
fn part_word(w: &str) -> Option<Part> {
    Some(match w {
        "header" | "masthead" => Part::Banner,
        "footer" => Part::Footer,
        "sidebar" | "aside" => Part::Sidebar,
        "nav" | "navigation" | "navbar" => Part::Navigation,
        "form" | "forms" => Part::Form,
        "dialog" | "modal" | "popup" | "popover" | "overlay" => Part::Dialog,
        "table" | "tables" | "grid" => Part::Table,
        "list" | "lists" => Part::List,
        "section" | "sections" | "region" | "panel" | "area" => Part::Region,
        _ => return None,
    })
}

/// Words for one of several things like it on a page.
const ITEMS: &[&str] = &[
    "result", "item", "product", "card", "entry", "row", "post", "story", "listing", "tile", "article", "review", "comment", "offer", "deal", "hit", "repo",
    "repository", "video", "track", "song", "episode",
];

/// Of [`ITEMS`], the words for any list's items (the others name a kind of thing).
const GENERIC: &[&str] = &["result", "item", "product", "card", "entry", "row", "post", "story", "listing", "tile", "article", "hit"];

/// Whether `w` is one of [`ITEMS`]. Pure.
fn item_word(w: &str) -> bool {
    let s = stem(w);
    ITEMS.iter().any(|x| stem(x) == s)
}

/// **A word's stem**: plurals, -ing, -ed taken off ("adds" → add, "boxes"
/// → box, "categories" → category, "added" → add). Pure.
pub fn stem(w: &str) -> String {
    let base = suffixless(w.trim_matches('\''));
    // (A last "e" off too: "save" and "saved" are one word.)
    match base.strip_suffix('e') {
        Some(s) if base.chars().count() > 3 && !s.ends_with('e') => s.to_string(),
        _ => base,
    }
}

/// `w` without its plural, -ing or -ed. Pure.
fn suffixless(w: &str) -> String {
    let n = w.chars().count();
    if n <= 3 || !w.chars().all(|c| c.is_alphabetic()) {
        return w.to_string();
    }
    // ("shopp" → shop, never "add" → ad.)
    let undouble = |s: &str| -> String {
        let c: Vec<char> = s.chars().collect();
        let last = c[c.len() - 1];
        if c.len() >= 4 && last == c[c.len() - 2] && !matches!(last, 'l' | 's' | 'z' | 'e' | 'o') && !"aeiou".contains(last) {
            c[..c.len() - 1].iter().collect()
        } else {
            s.to_string()
        }
    };
    if let Some(s) = w.strip_suffix("ies").filter(|s| s.chars().count() >= 2) {
        return format!("{s}y");
    }
    if let Some(s) = w.strip_suffix("sses") {
        return format!("{s}ss");
    }
    for end in ["xes", "zes", "ches", "shes"] {
        if let Some(s) = w.strip_suffix(end) {
            return format!("{s}{}", &end[..end.len() - 2]);
        }
    }
    if let Some(s) = w.strip_suffix("ing").filter(|s| s.chars().count() >= 3) {
        return undouble(s);
    }
    if let Some(s) = w.strip_suffix("ed").filter(|s| s.chars().count() >= 3) {
        return undouble(s);
    }
    match w.strip_suffix('s') {
        Some(s) if !s.ends_with('s') && !s.ends_with('u') && !s.ends_with('i') => s.to_string(),
        _ => w.to_string(),
    }
}

/// Words that mean the same on a page, the first standing for them all.
const SAME: &[&[&str]] = &[
    &["basket", "cart", "bag", "trolley"],
    &["signin", "login", "logon"],
    &["signup", "register", "join", "enrol", "enroll"],
    &["search", "find", "seek", "lookup"],
    &["buy", "purchase", "checkout"],
    &["close", "dismiss", "x", "×", "✕"],
    &["remove", "delete", "trash", "bin", "discard"],
    &["next", "forward", "›", "»", "→"],
    &["previous", "prev", "back", "‹", "«", "←"],
    &["setting", "preference", "option", "config"],
    &["account", "profile"],
    &["menu", "hamburger"],
    &["email", "mail"],
    &["phone", "telephone", "mobile", "tel"],
    &["quantity", "qty"],
    &["help", "support", "faq"],
    &["home", "homepage"],
    &["price", "cost"],
    &["submit", "send"],
    &["accept", "agree", "allow"],
    &["reject", "decline", "deny"],
    &["cookie", "consent"],
    &["picture", "image", "photo", "img"],
    &["add", "plus", "+"],
    &["more", "expand"],
    &["less", "collapse"],
    &["filter", "refine"],
    &["wishlist", "favourite", "favorite"],
    &["user", "username"],
    &["password", "passcode"],
    &["postcode", "zip", "postal"],
    &["surname", "lastname"],
    &["forename", "firstname"],
];

/// A word as it's compared: its stem, then the word standing for its
/// synonyms. Pure.
fn canon(w: &str) -> String {
    let s = stem(w);
    let with_e = format!("{s}e");
    for set in SAME {
        if set.iter().any(|x| *x == s || *x == w || *x == with_e) {
            return set[0].to_string();
        }
    }
    s
}

/// Phrases run into one word before matching ("log in" → signin,
/// "drop down" → dropdown, "search bar" → search box).
const PHRASES: &[(&[&str], &[&str])] = &[
    (&["sign", "in"], &["signin"]),
    (&["log", "in"], &["signin"]),
    (&["log", "on"], &["signin"]),
    (&["sign", "on"], &["signin"]),
    (&["sign", "up"], &["signup"]),
    (&["create", "an", "account"], &["signup"]),
    (&["create", "account"], &["signup"]),
    (&["check", "out"], &["checkout"]),
    (&["e", "mail"], &["email"]),
    (&["drop", "down"], &["dropdown"]),
    (&["pull", "down"], &["dropdown"]),
    (&["text", "box"], &["textbox"]),
    (&["text", "field"], &["textbox"]),
    (&["text", "area"], &["textbox"]),
    (&["textarea"], &["textbox"]),
    (&["input", "box"], &["box"]),
    (&["input", "field"], &["box"]),
    (&["searchbox"], &["search", "box"]),
    (&["searchbar"], &["search", "box"]),
    (&["check", "box"], &["checkbox"]),
    (&["tick", "box"], &["checkbox"]),
    (&["tickbox"], &["checkbox"]),
    (&["radio", "button"], &["radio"]),
    (&["menu", "bar"], &["navbar"]),
    (&["nav", "bar"], &["navbar"]),
    (&["navigation", "bar"], &["navbar"]),
    (&["tool", "bar"], &["toolbar"]),
    (&["pop", "up"], &["popup"]),
    (&["next", "to"], &["nextto"]),
    (&["close", "to"], &["nearby"]),
    (&["right", "hand"], &["right"]),
    (&["left", "hand"], &["left"]),
    (&["top", "most"], &["top"]),
    (&["post", "code"], &["postcode"]),
    (&["zip", "code"], &["postcode"]),
    (&["last", "name"], &["surname"]),
    (&["family", "name"], &["surname"]),
    (&["first", "name"], &["forename"]),
    (&["given", "name"], &["forename"]),
    (&["user", "name"], &["username"]),
    (&["wish", "list"], &["wishlist"]),
    (&["sign", "out"], &["signout"]),
    (&["log", "out"], &["signout"]),
    (&["logout"], &["signout"]),
];

/// Tokens with the phrases run together ([`PHRASES`]). Pure.
fn joined(tokens: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    'next: while i < tokens.len() {
        for (from, to) in PHRASES {
            if tokens.len() - i >= from.len() && from.iter().zip(&tokens[i..]).all(|(a, b)| *a == b) {
                out.extend(to.iter().map(|s| s.to_string()));
                i += from.len();
                continue 'next;
            }
        }
        out.push(tokens[i].clone());
        i += 1;
    }
    out
}

/// **Text as the words it's matched by**: split into words (camelCase and
/// hyphens too, accents folded: `rules::words`), phrases run together,
/// each word's stem and synonym ([`canon`]). Pure.
pub fn terms(s: &str) -> Vec<String> {
    let mut words = super::act::rules::words(s);
    // (Symbols that stand for a word: "×", "+", "›".)
    for c in s.chars() {
        if matches!(c, '×' | '✕' | '+' | '›' | '»' | '‹' | '«' | '→' | '←') {
            words.push(c.to_string());
        }
    }
    joined(words).into_iter().map(|w| canon(&w)).collect()
}

/// The words in quotes in a description (‘…’, “…”, "…", or '…' opened after
/// a space), folded, and the description without them. Pure.
fn quotes(s: &str) -> (Vec<String>, String) {
    let chars: Vec<char> = s.chars().collect();
    let mut quoted = Vec::new();
    let mut rest = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let close = match c {
            '“' => Some('”'),
            '‘' => Some('’'),
            '"' => Some('"'),
            '\'' if i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '(' => Some('\''),
            _ => None,
        };
        if let Some(close) = close {
            if let Some(end) = chars[i + 1..].iter().position(|x| *x == close || (close == '’' && *x == '\'')) {
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                let inner = super::see::fold(&inner);
                if !inner.is_empty() {
                    quoted.push(inner);
                }
                rest.push(' ');
                i += end + 2;
                continue;
            }
        }
        rest.push(c);
        i += 1;
    }
    (quoted, rest)
}

/// A description's words: lower-case, accents folded, split at anything
/// but letters, digits and an apostrophe; "result's" → `result` and a
/// possessive mark (`'s`). Pure.
fn tokens(s: &str) -> Vec<String> {
    let folded = super::see::fold(s).replace(['’', '‘'], "'");
    let mut out = Vec::new();
    for raw in folded.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '#')) {
        let raw = raw.trim_matches('\'');
        if raw.is_empty() {
            continue;
        }
        if let Some(base) = raw.strip_suffix("'s").or_else(|| raw.strip_suffix("s'")) {
            if !base.is_empty() && !matches!(base, "it" | "that" | "what" | "there" | "here" | "let") {
                out.push(base.to_string());
                out.push("'s".to_string());
                continue;
            }
        }
        out.push(raw.replace('\'', ""));
    }
    out
}

/// What a part of a description starts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    Target,
    Within { under: bool },
    Near { after: bool, before: bool },
    /// "for …" / "about …": its own words or what it's in.
    Loose,
}

/// **A description read** into what it asks for. Pure.
pub fn parse(query: &str) -> Wanted {
    let (quoted, rest) = quotes(query);
    let toks = joined(tokens(&rest));
    let mut w = Wanted { quoted, ..Default::default() };
    // Each part of it: what kind, its words, its ordinal.
    let mut segs: Vec<(Seg, Vec<String>, Option<Nth>)> = vec![(Seg::Target, Vec::new(), None)];
    let place_word = |t: &str| matches!(t, "top" | "bottom" | "upper" | "lower" | "uppermost" | "lowest" | "bottommost" | "topmost" | "left" | "right" | "leftmost" | "rightmost" | "middle" | "centre" | "center");
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        let next = toks.get(i + 1).map(String::as_str);
        let prev = i.checked_sub(1).map(|j| toks[j].as_str());
        // A place: "at the top", "top-left", "the top search box" — not "back to top".
        if place_word(t) && !matches!(prev, Some("to" | "back" | "scroll")) {
            match t {
                "top" | "upper" | "uppermost" | "topmost" => w.place.v = -1,
                "bottom" | "lower" | "lowest" | "bottommost" => w.place.v = 1,
                "left" | "leftmost" => w.place.h = -1,
                "right" | "rightmost" => w.place.h = 1,
                _ => {}
            }
            // "… of the page", "… side", "… corner": said.
            let mut j = i + 1;
            while j < toks.len() && matches!(toks[j].as_str(), "side" | "corner" | "edge" | "end" | "part" | "hand") {
                j += 1;
            }
            if toks.get(j).map(String::as_str) == Some("of") && matches!(toks.get(j + 1).map(String::as_str), Some("the")) && matches!(toks.get(j + 2).map(String::as_str), Some("page" | "screen" | "window" | "view" | "site")) {
                j += 3;
            }
            // A place a part is at ("at the top of the basket"): the part's words follow as a part.
            i = j;
            continue;
        }
        // A part it's in, beside, or about.
        let starts = match t {
            "in" | "inside" | "within" | "from" | "of" | "on" | "among" | "amongst" => Some(Seg::Within { under: false }),
            "under" | "beneath" => Some(Seg::Within { under: true }),
            "nextto" | "beside" | "near" | "nearby" | "alongside" | "by" if t != "by" || matches!(next, Some("the")) => Some(Seg::Near { after: false, before: false }),
            "below" | "after" | "following" => Some(Seg::Near { after: true, before: false }),
            "above" | "before" | "over" => Some(Seg::Near { after: false, before: true }),
            "for" | "about" => Some(Seg::Loose),
            _ => None,
        };
        if let Some(seg) = starts {
            // "of the page": nothing.
            if matches!(next, Some("the")) && matches!(toks.get(i + 2).map(String::as_str), Some("page" | "screen" | "site" | "website")) && toks.len() == i + 3 {
                break;
            }
            segs.push((seg, Vec::new(), None));
            i += 1;
            continue;
        }
        // "result's": what came before is a part it's in; the thing follows.
        if t == "'s" {
            let last = segs.last_mut().expect("a segment");
            if last.0 == Seg::Target {
                last.0 = Seg::Within { under: false };
            }
            segs.push((Seg::Target, Vec::new(), None));
            i += 1;
            continue;
        }
        let last = segs.last_mut().expect("a segment");
        if let Some(n) = ordinal(t) {
            // ("first name" was run together already.)
            last.2 = Some(n);
        } else if matches!(t, "number" | "no" | "#") && next.is_some_and(|n| n.parse::<u32>().is_ok()) {
            last.2 = next.and_then(|n| n.parse().ok()).map(Nth::At);
            i += 1;
        } else if let Some(n) = t.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
            last.2 = Some(Nth::At(n));
        } else {
            last.1.push(t.to_string());
        }
        i += 1;
    }
    for (seg, words, nth) in segs {
        match seg {
            // (Words after a second possessive, "the shop's basket's button": still the thing's.)
            Seg::Target => read_target(&mut w, &words, nth),
            Seg::Within { under } => {
                let mut part = None;
                let mut item = false;
                let mut ws = Vec::new();
                for t in &words {
                    if STOP.contains(&t.as_str()) {
                        continue;
                    }
                    if let Some(p) = part_word(t) {
                        part = Some(p);
                        continue;
                    }
                    // "the main content": the main part; "the main navigation": a word of the navigation's.
                    if t == "main" && !words.iter().any(|x| part_word(x).is_some()) {
                        part = Some(Part::Main);
                        continue;
                    }
                    if item_word(t) {
                        item = true;
                    }
                    ws.extend(terms(t));
                }
                if part.is_none() && !item && ws.is_empty() && nth.is_none() {
                    continue;
                }
                w.within.push(Within { part, item, nth, words: ws, under });
            }
            Seg::Near { after, before } => {
                let ws: Vec<String> = words.iter().filter(|t| !STOP.contains(&t.as_str()) && kind_word(t).is_none()).flat_map(|t| terms(t)).collect();
                if !ws.is_empty() {
                    w.near.push(Near { words: ws, after, before });
                }
            }
            Seg::Loose => {
                let ws: Vec<String> = words.iter().filter(|t| !STOP.contains(&t.as_str())).flat_map(|t| terms(t)).collect();
                w.words.extend(ws);
            }
        }
    }
    // "the second result": the item itself — the 2nd of its kind, its title link.
    if w.item && w.kind.is_none() {
        let words = std::mem::take(&mut w.words);
        w.within.insert(0, Within { part: None, item: true, nth: w.nth.take(), words, under: false });
    }
    w.words.dedup();
    let parts = w.nth.is_some() || w.place.any() || !w.within.is_empty() || !w.near.is_empty() || w.item;
    // (Words alone, "the button that adds it to the basket", are found as they are when they're on the page so,
    // and by description when they aren't: `see::find`.)
    w.described = parts;
    w
}

/// The thing's own part of a description: its role (the last role word), its words.
fn read_target(w: &mut Wanted, words: &[String], nth: Option<Nth>) {
    if nth.is_some() {
        w.nth = nth;
    }
    // The last role word is its role; one before it is a word of its ("the menu button").
    let role_at = words.iter().rposition(|t| kind_word(t).is_some() || item_word(t));
    for (i, t) in words.iter().enumerate() {
        if STOP.contains(&t.as_str()) {
            continue;
        }
        if Some(i) == role_at {
            if let Some(k) = kind_word(t) {
                w.kind = Some(k);
                // "search box": a field with "search".
                if k == Kind::Field && matches!(words.get(i.wrapping_sub(1)).map(String::as_str), Some("search")) {
                    w.words.push("search".into());
                }
            } else {
                w.item = true;
                w.words.extend(terms(t));
            }
            continue;
        }
        if t == "main" && matches!(words.get(i + 1).map(String::as_str), Some("heading" | "title" | "headline")) {
            w.main = true;
            continue;
        }
        // (A role word before the role: its word — "the link button", "the image link".)
        if t == "search" && words.get(i + 1).is_some_and(|n| matches!(kind_word(n), Some(Kind::Field))) {
            continue;
        }
        w.words.extend(terms(t));
    }
}

/// How well a thing is the kind asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Full,
    Partly,
    Not,
}

/// Whether thing `t` is kind `k`. Pure.
fn fit(k: Kind, t: &Thing) -> Fit {
    let r = t.role.as_str();
    let full = |yes: bool| if yes { Fit::Full } else { Fit::Not };
    match k {
        Kind::Button => {
            // (A link styled as one, `class="btn"`, is one to the eye.)
            if matches!(r, "button" | "menuitem") || (r == "link" && terms(&t.hints).iter().any(|h| matches!(h.as_str(), "btn" | "button"))) {
                Fit::Full
            } else if matches!(r, "link" | "tab" | "switch" | "menuitemcheckbox" | "menuitemradio" | "option") {
                Fit::Partly
            } else {
                Fit::Not
            }
        }
        Kind::Link => match r {
            "link" => Fit::Full,
            "button" | "menuitem" | "tab" => Fit::Partly,
            _ => Fit::Not,
        },
        Kind::Field => match r {
            "textbox" | "searchbox" | "spinbutton" => Fit::Full,
            "combobox" if t.tag == "input" => Fit::Full,
            "combobox" | "listbox" | "slider" => Fit::Partly,
            _ => Fit::Not,
        },
        Kind::Checkbox => match r {
            "checkbox" | "menuitemcheckbox" => Fit::Full,
            "switch" => Fit::Partly,
            _ => Fit::Not,
        },
        Kind::Radio => full(matches!(r, "radio" | "menuitemradio")),
        Kind::Dropdown => match r {
            "combobox" | "listbox" => Fit::Full,
            "button" | "menuitem" => Fit::Partly,
            _ => Fit::Not,
        },
        Kind::Tab => full(r == "tab"),
        Kind::Switch => match r {
            "switch" => Fit::Full,
            "checkbox" => Fit::Partly,
            _ => Fit::Not,
        },
        Kind::Slider => full(r == "slider"),
        Kind::Heading => full(r == "heading"),
        Kind::Image => full(matches!(r, "img" | "figure" | "image")),
        Kind::Text => full(matches!(r, "paragraph" | "listitem" | "cell" | "columnheader" | "rowheader" | "caption" | "blockquote" | "code" | "label" | "term" | "definition" | "text" | "note")),
        Kind::Option => full(matches!(r, "option" | "menuitem" | "radio")),
        Kind::Frame => full(matches!(r, "frame" | "iframe")),
        Kind::Part(p) => full(p.is(r)),
    }
}

/// One thing scored: its index in the scene, its score, why.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub i: usize,
    pub score: f64,
    pub why: Vec<String>,
}

/// Words, clipped for a reason ("‘Your basket’").
fn quoted_short(s: &str) -> String {
    format!("‘{}’", super::clip(s.trim(), 50))
}

/// A list of words as a reason says it: ‘add’, ‘basket’.
fn said(words: &[&str]) -> String {
    words.iter().map(|w| format!("‘{w}’")).collect::<Vec<_>>().join(", ")
}

/// The holders of the scene by id.
fn holder(scene: &Scene, id: u32) -> Option<&Holder> {
    scene.holders.iter().find(|h| h.id == id)
}

/// **The set of items an item part means** ("the second result's"): the
/// group of holders like each other whose words or name say the item's word
/// (`search-result`), else the page's biggest set of like items in its main
/// content (not its navigation, header or footer) — and the one wanted in
/// it by `nth`. Its group and the instance's holder id. Pure.
fn item_instance(scene: &Scene, c: &Within) -> Option<(u32, u32, u32)> {
    let mut groups: Vec<(u32, f64)> = Vec::new();
    let said: Vec<String> = c.words.clone();
    for h in scene.holders.iter().filter(|h| h.item > 0 && h.of >= 2) {
        if groups.iter().any(|(g, _)| *g == h.group) {
            continue;
        }
        let own: Vec<String> = terms(&format!("{} {} {}", h.words, h.role, h.name));
        let named = !said.is_empty() && said.iter().all(|w| own.contains(w));
        // Where the group is: inside the main content, or the page's navigation, header or footer.
        let outer: Vec<&Holder> = scene
            .things
            .iter()
            .find(|t| t.within.contains(&h.id))
            .map(|t| t.within.iter().skip_while(|id| **id != h.id).filter_map(|id| holder(scene, *id)).collect())
            .unwrap_or_default();
        let in_main = outer.iter().any(|o| o.role == "main");
        let aside = outer.iter().any(|o| matches!(o.role.as_str(), "navigation" | "banner" | "contentinfo" | "menu" | "menubar"));
        let links = scene.things.iter().filter(|t| t.within.contains(&h.id) && matches!(t.role.as_str(), "link" | "heading")).count();
        let size = f64::from(h.of);
        // (A word for any list's items — a result, an item, a card — fits the page's main list; one for a
        // kind of thing — a video, a song, a review — only a list that says it.)
        if !named && !said.iter().all(|w| GENERIC.iter().any(|g| stem(g) == *w)) {
            continue;
        }
        let score = if named { 1000.0 } else { 0.0 } + size + if in_main { 10.0 } else { 0.0 } - if aside { 25.0 } else { 0.0 } + if links > 0 { 5.0 } else { -5.0 };
        groups.push((h.group, score));
    }
    let (group, _) = groups.into_iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let mut items: Vec<&Holder> = scene.holders.iter().filter(|h| h.group == group && h.item > 0).collect();
    items.sort_by_key(|h| h.item);
    let of = items.len() as u32;
    let pick = match c.nth {
        Some(Nth::At(n)) => items.iter().find(|h| h.item == n).copied()?,
        Some(Nth::Last) => *items.last()?,
        None => return Some((group, 0, of)),
    };
    Some((group, pick.id, of))
}

/// Whether holder `h` is the part `c` asks for: its role, and most of its
/// words in its name, id or classes. Pure.
fn holds(h: &Holder, c: &Within) -> bool {
    if let Some(p) = c.part {
        if !p.is(&h.role) && !terms(&h.words).iter().any(|w| w == &part_name(p)) {
            return false;
        }
    }
    if c.item && h.item == 0 {
        return false;
    }
    if c.words.is_empty() {
        return c.part.is_some() || c.item;
    }
    let own = terms(&format!("{} {}", h.name, h.words));
    let hits = c.words.iter().filter(|w| own.contains(w)).count();
    hits * 2 >= c.words.len().max(1) && hits > 0
}

/// A part's own word, as a page's classes say it ("header", "footer").
fn part_name(p: Part) -> String {
    match p {
        Part::Banner => "header",
        Part::Footer => "footer",
        Part::Sidebar => "sidebar",
        Part::Navigation => "nav",
        Part::Main => "main",
        Part::Form => "form",
        Part::Dialog => "dialog",
        Part::Table => "table",
        Part::List => "list",
        Part::Region => "section",
    }
    .to_string()
}

/// How a reason names a holder: "the navigation ‘Main’", "the form ‘Sign up’".
fn holder_words(h: &Holder) -> String {
    let what = match h.role.as_str() {
        "navigation" | "menubar" | "menu" => "the navigation",
        "banner" => "the header",
        "contentinfo" => "the footer",
        "complementary" => "the sidebar",
        "main" => "the main content",
        "form" | "search" => "the form",
        "dialog" | "alertdialog" => "the dialog",
        "table" | "grid" => "the table",
        "row" => "the row",
        "list" | "listbox" => "the list",
        "listitem" => "the item",
        "article" => "the article",
        _ => "the part",
    };
    if h.name.trim().is_empty() {
        what.to_string()
    } else {
        format!("{what} {}", quoted_short(&h.name))
    }
}

/// **Every thing scored** against what's wanted, the best first, those with
/// nothing asked for matched left out. Pure.
pub fn rank(w: &Wanted, scene: &Scene) -> Vec<Ranked> {
    let page_h = scene.things.iter().map(|t| t.y + t.h).fold(scene.h.max(scene.vh).max(1.0), f64::max);
    let view_w = if scene.w > 0.0 { scene.w } else { scene.things.iter().map(|t| t.x + t.w).fold(1.0, f64::max) };
    // Item parts: the instance each means.
    let instances: Vec<Option<(u32, u32, u32)>> = w.within.iter().map(|c| if c.item { item_instance(scene, c) } else { None }).collect();
    let mut out: Vec<(Ranked, bool)> = Vec::new();
    for (i, t) in scene.things.iter().enumerate() {
        let mut s = 0.0;
        let mut why: Vec<String> = Vec::new();
        let mut asked = false;
        // Its role.
        let kind_fit = w.kind.map(|k| fit(k, t));
        match kind_fit {
            Some(Fit::Full) => s += 4.0,
            Some(Fit::Partly) => {
                s += 1.5;
                why.push(format!("a {} (not quite the kind asked for)", t.role));
            }
            Some(Fit::Not) => s -= 2.5,
            None => {
                // (Asked for nothing in particular: a control or a heading before a block of text.)
                s += match t.kind.as_str() {
                    "control" => 0.5,
                    "heading" => 0.3,
                    _ => 0.0,
                }
            }
        }
        // Its words.
        let fields: [(&str, &str); 6] = [("name", &t.name), ("label", &t.label), ("placeholder", &t.placeholder), ("value", &t.value), ("description", &t.desc), ("id or class", &t.hints)];
        let fields: Vec<(&str, Vec<String>)> = fields.iter().map(|(k, v)| (*k, terms(v))).collect();
        let href = terms(&crate::web::short_url(&t.href));
        let heading = terms(&t.heading);
        let holders: Vec<&Holder> = t.within.iter().filter_map(|id| holder(scene, *id)).collect();
        let around: Vec<String> = holders.iter().flat_map(|h| terms(&h.name)).collect();
        let classes: Vec<String> = holders.iter().flat_map(|h| terms(&h.words)).collect();
        let mut own_hits: Vec<(&str, &str)> = Vec::new();
        let mut near_hits: Vec<&str> = Vec::new();
        for q in &w.words {
            let searchy = q == "search" && (t.search || t.role == "searchbox");
            if let Some((field, _)) = fields.iter().find(|(_, ws)| ws.contains(q)) {
                s += if *field == "id or class" { 1.5 } else { 3.0 };
                own_hits.push((field, q.as_str()));
            } else if searchy {
                s += 3.0;
                own_hits.push(("role", q.as_str()));
            } else if href.contains(q) {
                s += 1.2;
                own_hits.push(("address", q.as_str()));
            } else if heading.contains(q) || around.contains(q) {
                s += 1.2;
                near_hits.push(q.as_str());
            } else if classes.contains(q) {
                s += 0.6;
                near_hits.push(q.as_str());
            }
        }
        if !w.words.is_empty() && own_hits.len() == w.words.len() {
            s += 1.5;
        }
        // All of them in order in its name: a little more ("Add to basket" for "add basket").
        let name_terms = &fields[0].1;
        if w.words.len() >= 2 && is_in_order(&w.words, name_terms) {
            s += 0.5;
        }
        if !own_hits.is_empty() {
            asked = true;
            // Grouped by where: ‘add’, ‘basket’ in its name; ‘search’ in its placeholder.
            let mut by: Vec<(&str, Vec<&str>)> = Vec::new();
            for &(f, q) in &own_hits {
                match by.iter_mut().find(|(k, _)| *k == f) {
                    Some((_, v)) => v.push(q),
                    None => by.push((f, vec![q])),
                }
            }
            for (f, qs) in by {
                why.push(match f {
                    "role" => "a search box".to_string(),
                    "address" => format!("{} in its address", said(&qs)),
                    f => format!("{} in its {f}", said(&qs)),
                });
            }
        }
        if !near_hits.is_empty() {
            if own_hits.is_empty() && kind_fit == Some(Fit::Full) {
                asked = true;
            }
            let at = if !t.heading.is_empty() && near_hits.iter().all(|q| heading.contains(&q.to_string())) { format!("under {}", quoted_short(&t.heading)) } else { "in what it's inside".to_string() };
            why.push(format!("{} {at}", said(&near_hits)));
        }
        // Words in quotes: on it exactly.
        for q in &w.quoted {
            let on = [&t.name, &t.label, &t.placeholder, &t.value, &t.desc].iter().any(|v| super::see::fold(v).contains(q.as_str()));
            if on {
                s += 5.0;
                asked = true;
                why.push(format!("says {}", quoted_short(q)));
            } else if super::see::fold(&t.heading).contains(q.as_str()) || holders.iter().any(|h| super::see::fold(&h.name).contains(q.as_str())) {
                s += 1.5;
                why.push(format!("{} is in what it's under", quoted_short(q)));
            } else {
                s -= 2.0;
            }
        }
        // The main heading.
        if w.main {
            if t.role == "heading" && t.level == 1 {
                s += 2.5;
                why.push("the page's main heading".into());
            } else if t.role == "heading" {
                s -= 0.5;
            }
        }
        // The parts it's in.
        let mut parts_ok = true;
        for (c, inst) in w.within.iter().zip(&instances) {
            if c.item {
                match inst {
                    Some((group, id, of)) if *id > 0 => {
                        let n = holder(scene, *id).map_or(0, |h| h.item);
                        if t.within.contains(id) {
                            s += 6.0;
                            asked = true;
                            let item = c.words.first().map_or("item".to_string(), |w| w.clone());
                            why.push(format!("in the {} of {of} {}s like it", Nth::At(n).word(), item));
                        } else if holders.iter().any(|h| h.group == *group) {
                            s -= 1.0;
                            parts_ok = false;
                        } else {
                            s -= 3.0;
                            parts_ok = false;
                        }
                    }
                    Some((group, _, _)) => {
                        if let Some(h) = holders.iter().find(|h| h.group == *group) {
                            // Any of them ("the link in a result about…"): by the item's words.
                            let words: Vec<String> = c.words.iter().filter(|w| !item_word(w)).cloned().collect();
                            let own = terms(&format!("{} {}", h.name, h.words));
                            let inner: Vec<String> = scene.things.iter().filter(|x| x.within.contains(&h.id)).flat_map(|x| terms(&x.name)).collect();
                            if words.is_empty() || words.iter().all(|w| own.contains(w) || inner.contains(w)) {
                                s += 3.0;
                                asked = true;
                                why.push(format!("in item {} of {}", h.item, h.of));
                            } else {
                                s -= 1.0;
                                parts_ok = false;
                            }
                        } else {
                            s -= 3.0;
                            parts_ok = false;
                        }
                    }
                    None => {
                        s -= 1.0;
                        parts_ok = false;
                    }
                }
                continue;
            }
            if let Some(h) = holders.iter().find(|h| holds(h, c)) {
                s += 3.0;
                asked = true;
                why.push(format!("inside {}", holder_words(h)));
            } else if (c.under || c.part.is_none()) && !c.words.is_empty() && c.words.iter().all(|q| heading.contains(q)) {
                s += 2.5;
                asked = true;
                why.push(format!("under {}", quoted_short(&t.heading)));
            } else {
                s -= 2.5;
                parts_ok = false;
            }
        }
        // What it's beside: a thing just before or after it on the page with those words.
        for n in &w.near {
            let lo = i.saturating_sub(3);
            let hi = (i + 4).min(scene.things.len());
            let hit = (lo..hi).filter(|j| *j != i).find(|j| {
                let o = &scene.things[*j];
                // (Not what holds it or what it holds: a card's whole text.)
                let holds_it = o.x <= t.x && o.y <= t.y && o.x + o.w >= t.x + t.w && o.y + o.h >= t.y + t.h;
                let held = t.x <= o.x && t.y <= o.y && t.x + t.w >= o.x + o.w && t.y + t.h >= o.y + o.h;
                let side = (!n.after || *j < i) && (!n.before || *j > i);
                let ws = terms(&format!("{} {} {}", o.name, o.label, o.placeholder));
                side && !holds_it && !held && n.words.iter().filter(|q| ws.contains(q)).count() * 2 >= n.words.len()
            });
            match hit {
                Some(j) => {
                    s += 2.5;
                    asked = true;
                    let rel = if n.after { "after" } else if n.before { "before" } else { "next to" };
                    why.push(format!("{rel} {}", quoted_short(&scene.things[j].name)));
                }
                None => s -= 1.0,
            }
        }
        // Where it is.
        if w.place.v != 0 {
            let at = ((t.y + t.h / 2.0) / page_h).clamp(0.0, 1.0);
            let up = if w.place.v < 0 { 1.0 - at } else { at };
            s += 3.0 * up;
            let edge = if w.place.v < 0 { "banner" } else { "contentinfo" };
            if holders.iter().any(|h| h.role == edge) {
                s += 1.0;
            }
            let near_edge = if w.place.v < 0 { t.y < scene.vh.max(page_h * 0.15) } else { t.y + t.h > page_h - scene.vh.max(page_h * 0.15) };
            if near_edge {
                why.push(if w.place.v < 0 { "at the top of the page".into() } else { "at the bottom of the page".into() });
            }
        }
        if w.place.h != 0 {
            let cx = ((t.x + t.w / 2.0) / view_w).clamp(0.0, 1.0);
            s += 2.0 * if w.place.h < 0 { 1.0 - cx } else { cx };
            if (w.place.h < 0 && cx < 0.4) || (w.place.h > 0 && cx > 0.6) {
                why.push(if w.place.h < 0 { "on the left".into() } else { "on the right".into() });
            }
        }
        // Nothing asked but a role (an ordinal, a place, a part): the role itself.
        if !asked && w.words.is_empty() && w.quoted.is_empty() {
            asked = kind_fit == Some(Fit::Full) || (w.kind.is_none() && (w.place.any() || !w.within.is_empty() || !w.near.is_empty()));
        }
        if t.seen {
            s += 0.2;
        }
        if t.disabled {
            s -= 0.5;
        }
        out.push((Ranked { i, score: s, why }, asked && parts_ok_or_scored(parts_ok, s)));
    }
    // **Which one** ("the second link"): among those of its kind, in its parts, with its words — by
    // where they are on the page, top to bottom, left to right.
    if let Some(nth) = w.nth {
        // (Those with the most of its words: "the third add to basket button" counts no "Add to wishlist".)
        let covered = |t: &Thing| -> usize {
            let own: Vec<String> = [&t.name, &t.label, &t.placeholder, &t.value, &t.desc, &t.hints].iter().flat_map(|v| terms(v)).collect();
            w.words.iter().filter(|q| own.contains(q) || (q.as_str() == "search" && t.search)).count()
        };
        let base: Vec<usize> = out.iter().filter(|(r, ok)| *ok && w.kind.is_none_or(|k| fit(k, &scene.things[r.i]) == Fit::Full)).map(|(r, _)| r.i).collect();
        let most = base.iter().map(|i| covered(&scene.things[*i])).max().unwrap_or(0);
        let mut set: Vec<usize> = base.into_iter().filter(|i| covered(&scene.things[*i]) == most).collect();
        // (Within 8 px: one row, left to right.)
        set.sort_by_key(|i| ((scene.things[*i].y / 8.0).round() as i64, scene.things[*i].x as i64));
        let chosen = match nth {
            Nth::At(n) => n.checked_sub(1).and_then(|k| set.get(k as usize)).copied(),
            Nth::Last => set.last().copied(),
        };
        let what = w.kind.map_or("like it".to_string(), |_| "of its kind".to_string());
        for (r, _) in out.iter_mut() {
            if Some(r.i) == chosen {
                r.score += 6.0;
                let n = set.iter().position(|x| *x == r.i).map_or(0, |p| p as u32 + 1);
                r.why.push(format!("the {} of {} {what}, top to bottom", Nth::At(n).word(), set.len()));
            }
        }
    }
    // The item itself ("the second result"): its title link first.
    if w.item && w.kind.is_none() {
        if let Some(Some((_, id, _))) = instances.first() {
            let heading_link = |t: &Thing| t.role == "link" && t.within.iter().filter_map(|h| holder(scene, *h)).any(|h| h.id == *id) && !t.heading.is_empty() && super::see::fold(&t.heading) == super::see::fold(&t.name);
            let first_link = scene.things.iter().position(|t| t.role == "link" && t.within.contains(id));
            for (r, _) in out.iter_mut() {
                let t = &scene.things[r.i];
                if !t.within.contains(id) {
                    continue;
                }
                if heading_link(t) || t.role == "heading" {
                    r.score += 2.0;
                    r.why.push("its title".into());
                } else if Some(r.i) == first_link {
                    r.score += 1.5;
                    r.why.push("its first link".into());
                }
            }
        }
    }
    let mut kept: Vec<Ranked> = out.into_iter().filter(|(r, ok)| *ok && r.score > 0.0).map(|(r, _)| r).collect();
    kept.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| {
        let (ta, tb) = (&scene.things[a.i], &scene.things[b.i]);
        ta.y.total_cmp(&tb.y).then(ta.x.total_cmp(&tb.x))
    }));
    kept
}

/// A thing out of its part still listed when it scored well enough by its
/// words. Pure.
fn parts_ok_or_scored(parts_ok: bool, score: f64) -> bool {
    parts_ok || score >= 6.0
}

/// Whether `want` is in `have` in order (not side by side). Pure.
fn is_in_order(want: &[String], have: &[String]) -> bool {
    let mut it = have.iter();
    want.iter().all(|w| it.any(|h| h == w))
}

/// **A ranked thing as the model reads it**: `[ref] role "name" → address
/// — why`. Pure.
pub fn line(t: &Thing, why: &[String]) -> String {
    let name = [&t.name, &t.label, &t.placeholder].into_iter().find(|v| !v.trim().is_empty()).map_or(String::new(), |v| super::clip(v.trim(), 100));
    let role = if t.role.is_empty() { "element" } else { &t.role };
    let mut line = format!("[{}] {role} \"{}\"", t.r, name.replace('"', "'"));
    if t.role == "link" && !t.href.is_empty() {
        line.push_str(&format!(" → {}", crate::web::short_url(&t.href)));
    }
    if t.disabled {
        line.push_str(" (disabled)");
    }
    if !why.is_empty() {
        line.push_str(" — ");
        line.push_str(&why.join("; "));
    }
    line
}

/// **The best few** for description `query` on `scene`: their lines, and
/// whether the first two scored the same (the model tells them apart).
/// Pure.
pub fn best(query: &str, scene: &Scene) -> (Vec<String>, bool) {
    let w = parse(query);
    let ranked = rank(&w, scene);
    let tied = ranked.len() >= 2 && (ranked[0].score - ranked[1].score).abs() < 0.05;
    let lines = ranked
        .iter()
        .take(MAX)
        .map(|r| {
            let t = &scene.things[r.i];
            // What it's under, to tell alike ones apart (three "Add to basket" buttons).
            let mut why = r.why.clone();
            if !t.heading.trim().is_empty() && t.role != "heading" && !why.iter().any(|w| w.starts_with("under ") || w.contains(" under ")) {
                why.push(format!("under {}", quoted_short(&t.heading)));
            }
            line(t, &why)
        })
        .collect();
    (lines, tied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_stemmed_and_folded_with_their_synonyms() {
        assert_eq!(stem("adds"), "add");
        assert_eq!(stem("adding"), "add");
        assert_eq!(stem("added"), "add");
        assert_eq!(stem("boxes"), "box");
        assert_eq!(stem("categories"), "category");
        assert_eq!(stem("results"), "result");
        assert_eq!(stem("address"), "address");
        assert_eq!(stem("glasses"), "glass");
        assert_eq!(stem("shopping"), "shop");
        assert_eq!(terms("Add to Cart"), ["add", "to", "basket"]);
        assert_eq!(terms("addToBasket"), ["add", "to", "basket"]);
        assert_eq!(terms("Log in"), ["signin"]);
        assert_eq!(terms("Sign In"), ["signin"]);
        assert_eq!(terms("search-results"), ["search", "result"]);
        assert_eq!(terms("×"), ["close"]);
    }

    #[test]
    fn a_description_reads_as_its_role_words_place_ordinal_and_parts() {
        let w = parse("the button that adds it to the basket");
        assert_eq!(w.kind, Some(Kind::Button));
        assert_eq!(w.words, ["add", "basket"]);
        assert!(!w.described && w.within.is_empty() && w.nth.is_none());

        let w = parse("the search box at the top");
        assert_eq!(w.kind, Some(Kind::Field));
        assert_eq!(w.words, ["search"]);
        assert_eq!(w.place, Place { v: -1, h: 0 });
        assert!(w.described);

        let w = parse("the second result's link");
        assert_eq!(w.kind, Some(Kind::Link));
        assert!(w.words.is_empty() && w.nth.is_none());
        assert_eq!(w.within, [Within { part: None, item: true, nth: Some(Nth::At(2)), words: vec!["result".into()], under: false }]);

        let w = parse("the second result");
        assert!(w.item && w.kind.is_none());
        assert_eq!(w.within[0].nth, Some(Nth::At(2)));

        let w = parse("the email field in the sign-up form");
        assert_eq!(w.kind, Some(Kind::Field));
        assert_eq!(w.words, ["email"]);
        assert_eq!(w.within, [Within { part: Some(Part::Form), item: false, nth: None, words: vec!["signup".into()], under: false }]);

        let w = parse("the third link in the navigation");
        assert_eq!((w.kind, w.nth), (Some(Kind::Link), Some(Nth::At(3))));
        assert_eq!(w.within[0].part, Some(Part::Navigation));

        let w = parse("the link under Delivery");
        assert!(w.within[0].under && w.within[0].words == ["delivery"]);

        let w = parse("the field next to Quantity");
        assert_eq!(w.near, [Near { words: vec!["quantity".into()], after: false, before: false }]);

        let w = parse("the 'Add to basket' button on the right");
        assert_eq!(w.quoted, ["add to basket"]);
        assert_eq!(w.place.h, 1);
        assert_eq!(w.kind, Some(Kind::Button));

        // "Back to top" is words, not a place.
        let w = parse("back to top link");
        assert_eq!(w.place, Place::default());
        assert_eq!(w.words, ["previous", "top"]);

        // Words as they are: not a description.
        assert!(!parse("Search the docs").described && !parse("Harbour").described && !parse("button Buy").described);
        assert!(parse("the main heading").main);
    }

    fn thing(r: u32, role: &str, name: &str, y: f64) -> Thing {
        Thing { r, role: role.into(), kind: if role == "heading" { "heading".into() } else { "control".into() }, name: name.into(), tag: if role == "textbox" || role == "searchbox" { "input".into() } else { String::new() }, y, h: 30.0, w: 120.0, ..Default::default() }
    }

    /// A small shop: a header with search and a basket link, three products each with Add to basket, a newsletter form at the bottom.
    fn shop() -> Scene {
        let holders = vec![
            Holder { id: 1, role: "banner".into(), ..Default::default() },
            Holder { id: 2, role: "navigation".into(), name: "Main".into(), ..Default::default() },
            Holder { id: 3, role: "main".into(), ..Default::default() },
            Holder { id: 4, role: "listitem".into(), name: "Trail runner".into(), words: "product card".into(), item: 1, of: 3, group: 9, y: 300.0 },
            Holder { id: 5, role: "listitem".into(), name: "Road racer".into(), words: "product card".into(), item: 2, of: 3, group: 9, y: 500.0 },
            Holder { id: 6, role: "listitem".into(), name: "Hill walker".into(), words: "product card".into(), item: 3, of: 3, group: 9, y: 700.0 },
            Holder { id: 7, role: "form".into(), name: "Newsletter".into(), ..Default::default() },
            Holder { id: 8, role: "contentinfo".into(), ..Default::default() },
            Holder { id: 10, role: "listitem".into(), item: 1, of: 2, group: 11, ..Default::default() },
            Holder { id: 12, role: "listitem".into(), item: 2, of: 2, group: 11, ..Default::default() },
        ];
        let mut things = vec![
            Thing { placeholder: "Search products".into(), search: true, within: vec![1], ..thing(1, "searchbox", "Search products", 20.0) },
            Thing { href: "https://shop.example.com/basket".into(), within: vec![1], x: 1100.0, ..thing(2, "link", "Basket (2)", 20.0) },
            Thing { within: vec![10, 2, 1], href: "https://shop.example.com/men".into(), ..thing(3, "link", "Men", 60.0) },
            Thing { within: vec![12, 2, 1], href: "https://shop.example.com/women".into(), x: 200.0, ..thing(4, "link", "Women", 60.0) },
            thing(5, "heading", "Running shoes", 200.0),
        ];
        let mut r = 6;
        for (holder, name, y) in [(4, "Trail runner", 300.0), (5, "Road racer", 500.0), (6, "Hill walker", 700.0)] {
            things.push(Thing { within: vec![holder, 3], heading: name.into(), href: format!("https://shop.example.com/p/{r}"), ..thing(r, "link", name, y) });
            things.push(Thing { within: vec![holder, 3], heading: name.into(), ..thing(r + 1, "button", "Add to wishlist", y + 60.0) });
            things.push(Thing { within: vec![holder, 3], heading: name.into(), hints: "add-to-cart btn".into(), ..thing(r + 2, "button", "Add to basket", y + 100.0) });
            r += 3;
        }
        things.push(Thing { within: vec![7, 8], heading: "Newsletter".into(), label: "Email".into(), ..thing(20, "textbox", "Email", 1000.0) });
        things.push(Thing { within: vec![7, 8], heading: "Newsletter".into(), ..thing(21, "button", "Sign up", 1040.0) });
        things.push(Thing { within: vec![8], heading: "Newsletter".into(), placeholder: "Search help".into(), search: true, ..thing(22, "searchbox", "Search help", 1100.0) });
        Scene { things, holders, h: 1200.0, w: 1280.0, vh: 800.0 }
    }

    fn top(query: &str) -> (u32, Vec<String>) {
        let s = shop();
        let r = rank(&parse(query), &s);
        assert!(!r.is_empty(), "nothing for {query}");
        (s.things[r[0].i].r, r[0].why.clone())
    }

    #[test]
    fn the_shop_is_found_by_description() {
        // The three "Add to basket" buttons tie on their words: the first on the page is listed first, and each says what it's under.
        let s = shop();
        let (lines, tied) = best("the button that adds it to the basket", &s);
        assert!(tied, "{lines:?}");
        assert!(lines[0].starts_with("[8] button \"Add to basket\" — ‘add’, ‘basket’ in its name"), "{lines:?}");
        assert!(lines.iter().take(3).all(|l| l.contains("Add to basket")), "{lines:?}");
        // Its words with what it's under: the one for the Road racer.
        assert_eq!(top("the add to basket button for the Road racer").0, 11);
        assert_eq!(top("the Add to basket button under Hill walker").0, 14);
        // The search box at the top, not the help search in the footer.
        let (r, why) = top("the search box at the top");
        assert_eq!(r, 1);
        assert!(why.iter().any(|w| w == "at the top of the page"), "{why:?}");
        assert_eq!(top("the search field at the bottom").0, 22);
        // The second result's link: the second product's own link.
        let (r, why) = top("the second product's link");
        assert_eq!(r, 9, "{why:?}");
        assert!(why.iter().any(|w| w.starts_with("in the 2nd of 3")), "{why:?}");
        assert_eq!(top("the second result's link").0, 9);
        assert_eq!(top("the last result").0, 12);
        // Inside a part.
        assert_eq!(top("the email field in the newsletter form").0, 20);
        assert_eq!(top("the second link in the navigation").0, 4);
        assert_eq!(top("the basket link on the right").0, 2);
        // Ordinals over a kind.
        assert_eq!(top("the third add to basket button").0, 14);
        assert_eq!(top("the first button").0, 7);
        // In quotes: exactly.
        assert_eq!(top("the 'Sign up' button").0, 21);
    }

    #[test]
    fn nothing_matching_lists_nothing() {
        let s = shop();
        assert!(rank(&parse("the video player's volume slider"), &s).is_empty());
        assert!(rank(&parse("the checkout button"), &s).iter().all(|r| s.things[r.i].role != "searchbox"));
    }
}
