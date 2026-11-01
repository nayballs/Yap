//! **The rules Yap acts by on a page** (pure): what a page's links, buttons
//! and fields are (as the element script reads them, `script.rs`, or the
//! stand-in browser, `pages/sim.rs`), which fields Yap never types into,
//! which forms are sign-in forms, which clicks ask the person first, and how
//! the elements and an action's outcome are told to the model.
//!
//! Enforced here, in Rust, whatever the model was told: a page or a model
//! that words things cleverly can't talk its way past them.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::words;

/// **One link, button or field** on a page beside the chat, as the element
/// script describes it. `r` is its ref: the number the model acts on, the
/// same until the page navigates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Element {
    #[serde(rename = "ref")]
    pub r: u32,
    /// link, button, textbox, searchbox, combobox, checkbox, radio, …
    pub role: String,
    /// Its accessible name (aria-label, its label, its words…).
    pub name: String,
    /// What's in it now (never a password's, a card's or a code's: the
    /// script leaves those out).
    pub value: String,
    /// An input's `type` (text, email, password, date…).
    #[serde(rename = "type")]
    pub kind: String,
    pub autocomplete: String,
    /// Its `name` attribute.
    pub field: String,
    pub id: String,
    /// Its `<label>`'s words.
    pub label: String,
    pub placeholder: String,
    /// A link's address (the page's own, a test run's stand-in turned back).
    pub href: String,
    pub disabled: bool,
    pub checked: Option<bool>,
    /// A select's options (their words, the first 20).
    pub options: Vec<String>,
    /// The form it's in (an index into [`Snapshot::forms`]; -1: none).
    pub form: i64,
    /// It submits its form when clicked.
    pub submit: bool,
    /// Its value was left out (a sensitive field).
    pub masked: bool,
    /// The page marks it as a search box whatever its words (in a
    /// `role="search"` region or a `<search>` element, `enterkeyhint` or
    /// `inputmode` "search").
    pub search: bool,
    /// An editable region (`contenteditable`): typing goes in at its caret.
    pub editable: bool,
}

/// A form on the page: whether it has a password field (a sign-in form),
/// where it goes, its submit button's words — and whether it takes a
/// payment (a card field in it, by `autocomplete` or its words, or a payment
/// provider's frame) or shows a price.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Form {
    pub form: i64,
    pub password: bool,
    pub action: String,
    pub method: String,
    pub submit: String,
    pub payment: bool,
    pub price: bool,
}

/// Where the page is scrolled to (CSS pixels): its top, the window's height
/// and the most it scrolls.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Scroll {
    pub y: i64,
    pub h: i64,
    pub max: i64,
}

/// **A page read with its elements** (`read_open_page { elements: true }`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Snapshot {
    pub title: String,
    pub url: String,
    pub text: String,
    pub headings: Vec<String>,
    pub elements: Vec<Element>,
    pub forms: Vec<Form>,
    /// A password field anywhere on the page.
    pub password: bool,
    /// A payment card field (or a payment provider's frame) anywhere on it.
    pub payment: bool,
    /// More elements than [`MAX_ELEMENTS`] were there.
    pub more: bool,
    pub scroll: Scroll,
}

/// One element looked up by its ref just before an action, with what the
/// rules need around it: its form, whether the page has a password field or
/// a payment field, the page's title and headings (a sign-in page says so).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Described {
    /// `None`: no element has that ref now (the page changed, or it's gone).
    pub el: Option<Element>,
    pub form: Option<Form>,
    pub password: bool,
    pub payment: bool,
    pub title: String,
    pub headings: Vec<String>,
    pub url: String,
}

/// The most elements a read lists.
pub const MAX_ELEMENTS: usize = 150;
/// The most a field gets typed into it at once (characters): enough for a
/// station, a date, a search; never a note's worth of the person's words.
pub const MAX_TYPED: usize = 200;

/// `s` as lower-case words, accents folded ("Réserver" → reserver): split at
/// anything not a letter or a digit, and at a lower-to-upper change
/// ("cardNumber" → card, number). Pure.
pub fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    let push = |cur: &mut String, out: &mut Vec<String>| out.push(words::fold(&std::mem::take(cur).to_lowercase()));
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !cur.is_empty() {
                push(&mut cur, &mut out);
            }
            prev_lower = c.is_lowercase() || c.is_ascii_digit();
            cur.push(c);
        } else {
            if !cur.is_empty() {
                push(&mut cur, &mut out);
            }
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        push(&mut cur, &mut out);
    }
    out
}

/// The words an element is known by for the rules: its name, label,
/// placeholder, `name` attribute and id.
fn known_by(e: &Element) -> Vec<String> {
    [&e.name, &e.label, &e.placeholder, &e.field, &e.id].iter().flat_map(|s| words(s)).collect()
}

/// **A field Yap never types into**: why, in a few words for the model and
/// the chip — a password, payment card details, bank details, a security
/// code. By its input type, its `autocomplete` (`cc-*`, `current-password`,
/// `new-password`, `one-time-code`), and its name, id, label or placeholder
/// in any of Yap's languages (`words.rs`: card, Kartennummer, numéro de
/// carte, CVV, IBAN, sort code, Kontonummer, mot de passe, contraseña, PIN…).
/// `None`: Yap may type there (if it's a field at all). Pure.
pub fn sensitive(e: &Element) -> Option<&'static str> {
    let kind = e.kind.trim().to_ascii_lowercase();
    let auto = e.autocomplete.to_ascii_lowercase();
    let auto: Vec<&str> = auto.split_whitespace().collect();
    if kind == "password" || auto.iter().any(|a| matches!(*a, "current-password" | "new-password")) {
        return Some("a password");
    }
    if auto.iter().any(|a| a.starts_with("cc-")) {
        return Some("payment card details");
    }
    if auto.contains(&"one-time-code") {
        return Some("a security code");
    }
    let ws = known_by(e);
    let by = |pick: words::Pick| words::in_any(&ws, pick);
    if by(|l| l.password) {
        return Some("a password");
    }
    if by(|l| l.card) {
        return Some("payment card details");
    }
    if by(|l| l.bank) {
        return Some("bank details");
    }
    if by(|l| l.code_field) {
        return Some("a security code");
    }
    None
}

/// Words that make a page a sign-in page (its title or a heading), in any
/// of Yap's languages.
fn says_sign_in(text: &str) -> bool {
    words::in_any(&words(text), |l| l.sign_in_page)
}

/// **A sign-in field**: a field of a form with a password field, a
/// `username` field, or a user name / email field on a page that has a
/// password field or says it's for signing in. Yap never fills those in:
/// the person signs in themselves, in the panel. Pure.
pub fn sign_in_field(d: &Described, e: &Element) -> bool {
    if d.form.as_ref().is_some_and(|f| f.password) {
        return true;
    }
    let auto = e.autocomplete.to_ascii_lowercase();
    if auto.split_whitespace().any(|a| a == "username") {
        return true;
    }
    let userish = e.kind.eq_ignore_ascii_case("email") || auto.split_whitespace().any(|a| a == "email") || words::in_any(&known_by(e), |l| l.user);
    userish && (d.password || says_sign_in(&d.title) || d.headings.iter().any(|h| says_sign_in(h)))
}

/// Whether an element takes typing (a text box, a search box, a text area,
/// a select — its option picked by its words — or an editable region).
pub fn takes_text(e: &Element) -> bool {
    let kind = e.kind.to_ascii_lowercase();
    if matches!(kind.as_str(), "checkbox" | "radio" | "submit" | "button" | "reset" | "image" | "file" | "hidden" | "range" | "color") {
        return false;
    }
    matches!(e.role.as_str(), "textbox" | "searchbox" | "combobox" | "spinbutton" | "listbox")
}

/// **A search box**: Enter there searches, so it needs no asking — the page
/// says so (`type="search"`, `role="searchbox"`, a search region,
/// `enterkeyhint`), its `name` is a search's (`q`, `s`, `query`…), or its
/// words are a search's in any of Yap's languages ("Search", "Suche",
/// "Rechercher", "Buscar", "Cerca", "Pesquisar", "Zoeken"). Pure.
pub fn search_box(e: &Element) -> bool {
    if e.search || e.role == "searchbox" || e.kind.eq_ignore_ascii_case("search") {
        return true;
    }
    if matches!(e.field.to_ascii_lowercase().as_str(), "q" | "s" | "query" | "search_query" | "keyword" | "keywords") {
        return true;
    }
    words::in_any(&known_by(e), |l| l.search)
}

/// **A price in `s`**: a currency sign or code next to a number ("£42.10",
/// "19,99 €", "USD 30", "kr 99"). Language-independent. Pure.
pub fn has_price(s: &str) -> bool {
    const SIGNS: &[char] = &['£', '€', '$', '¥', '₹', '₩', '₽', '₺', '₪', '¢', '₱', '₫', '฿'];
    const CODES: &[&str] = &["usd", "eur", "gbp", "chf", "jpy", "cad", "aud", "sek", "nok", "dkk", "pln", "brl", "mxn", "kr", "zl", "r$"];
    let chars: Vec<char> = s.chars().collect();
    let digit_near = |i: usize, step: isize| -> bool {
        // A digit within two places (a space, a non-breaking space between).
        let mut j = i as isize + step;
        for _ in 0..2 {
            if j < 0 || j as usize >= chars.len() {
                return false;
            }
            let c = chars[j as usize];
            if c.is_ascii_digit() {
                return true;
            }
            if !c.is_whitespace() {
                return false;
            }
            j += step;
        }
        false
    };
    if chars.iter().enumerate().any(|(i, c)| SIGNS.contains(c) && (digit_near(i, 1) || digit_near(i, -1))) {
        return true;
    }
    // A currency code as its own word beside a number.
    let lower = s.to_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let numeric = |t: &str| t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.chars().all(|c| c.is_ascii_digit() || ",.-".contains(c));
    tokens.windows(2).any(|w| (CODES.contains(&w[0]) && numeric(w[1])) || (numeric(w[0]) && CODES.contains(&w[1].trim_end_matches(['.', ',', ')']))))
}

/// The kinds of click that ask first, in the order they're told apart (a
/// "Confirm booking" books), each with its words in every language.
const KINDS: &[(&str, words::Pick)] = &[
    ("sign in", |l| l.sign_in),
    ("buy", |l| l.buy),
    ("pay", |l| l.pay),
    ("book", |l| l.book),
    ("place an order", |l| l.order),
    ("cancel", |l| l.cancel),
    ("subscribe", |l| l.subscribe),
    ("confirm", |l| l.confirm),
    ("send", |l| l.send),
    ("delete", |l| l.delete),
    ("agree", |l| l.agree),
];

/// The kinds an address's path is read for (a GET link that does something:
/// "…/checkout", "…/unsubscribe?t=…", "…/booking/confirm"): never "send"
/// nor "agree" ("/post/123" is a blog's), and only single words.
const PATH_KINDS: &[&str] = &["sign in", "buy", "pay", "book", "place an order", "cancel", "subscribe", "confirm", "delete"];

/// **What a click would do that asks the person first**: a click (or a
/// submit) that would buy, pay, book, place an order, cancel, subscribe,
/// confirm, send, delete, agree to terms or sign in — by the button's or
/// link's words in any of Yap's languages ("Buy now", "Jetzt kaufen",
/// "Commander", "Pagar"…), then a link's (or a form's) address
/// (…/checkout, …/kasse, …/unsubscribe). The kind, for the card, the log
/// and the evaluation; `None`: it goes ahead. Pure.
pub fn needs_ok(label: &str, href: &str) -> Option<&'static str> {
    let ws = words(label);
    if let Some((kind, _)) = KINDS.iter().find(|(_, pick)| words::in_any(&ws, *pick)) {
        return Some(kind);
    }
    // A link's address says what it does (a "Continue" to /checkout).
    let path = reqwest::Url::parse(href.trim()).map(|u| u.path().to_string()).unwrap_or_default();
    let pw = words(&path);
    if pw.is_empty() {
        return None;
    }
    KINDS
        .iter()
        .filter(|(kind, _)| PATH_KINDS.contains(kind))
        .find(|(_, pick)| words::LANGS.iter().any(|l| pick(l).iter().filter(|e| !e.contains(' ')).any(|e| words::holds(&pw, e))))
        .map(|(kind, _)| *kind)
}

/// What a form's own signals say about submitting it, whatever its words:
/// a payment form (a card field in it, or one on the page) pays, a posted
/// form that shows a price buys. Pure.
fn form_signals(d: &Described, f: &Form) -> Option<&'static str> {
    if f.payment || d.payment {
        return Some("pay");
    }
    if f.price && f.method.eq_ignore_ascii_case("post") {
        return Some("buy");
    }
    None
}

/// What clicking `e` would do that asks first ([`needs_ok`]): its words and
/// a link's address; for a form's submit button, also where the form goes,
/// whether it takes a payment or shows a price (a "Continue" in a form
/// posting to …/place-order asks, as Enter in it does); a button with a
/// price on it pays; a sign-in form's submit signs in. Pure.
pub fn click_needs_ok(d: &Described, e: &Element) -> Option<&'static str> {
    if e.submit {
        if let Some(f) = d.form.as_ref() {
            if f.password {
                return Some("sign in");
            }
            if let Some(kind) = needs_ok(&e.name, &f.action).or_else(|| form_signals(d, f)) {
                return Some(kind);
            }
        }
    }
    if let Some(kind) = needs_ok(&e.name, &e.href).or_else(|| (e.name.trim().is_empty()).then(|| needs_ok(&e.label, "")).flatten()) {
        return Some(kind);
    }
    (e.role == "button" && has_price(&e.name)).then_some("pay")
}

/// What submitting `e`'s form (Enter in it) would do that asks first: its
/// submit button's words, where the form goes, whether it takes a payment
/// or shows a price. Pure.
pub fn submit_needs_ok(d: &Described) -> Option<&'static str> {
    let f = d.form.as_ref()?;
    if f.password {
        return Some("sign in");
    }
    needs_ok(&f.submit, &f.action).or_else(|| form_signals(d, f)).or_else(|| has_price(&f.submit).then_some("pay"))
}

/// **What Enter in `e` would do that asks first**: in a form, what
/// submitting it would ([`submit_needs_ok`]); outside one, it may send — a
/// chat's message box (a web mail's, WhatsApp's, Slack's) sends on Enter —
/// unless it's a search box ([`search_box`]). Pure.
pub fn enter_needs_ok(d: &Described, e: &Element) -> Option<&'static str> {
    if d.form.is_some() {
        return submit_needs_ok(d);
    }
    (!search_box(e)).then_some("send")
}

/// **Typed text Yap refuses whatever the field is called**: a payment card
/// number (13–19 digits that pass the Luhn check, spaces and dashes aside),
/// an IBAN (its mod-97 check), or a 3–4 digit code typed into a form that
/// takes a card (a security code beside the card field). Why, for the
/// model and the chip. Pure.
pub fn typed_secret(text: &str, d: &Described) -> Option<&'static str> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '.').collect();
    // A card number anywhere in it (a run of digits, spaces and dashes).
    let mut run = String::new();
    let mut runs = Vec::new();
    for c in text.chars().chain(std::iter::once('x')) {
        if c.is_ascii_digit() {
            run.push(c);
        } else if !(c == ' ' || c == '-') && !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if runs.iter().any(|r| (13..=19).contains(&r.len()) && luhn(r)) {
        return Some("payment card details");
    }
    if iban(&compact) {
        return Some("bank details");
    }
    let card_form = d.form.as_ref().is_some_and(|f| f.payment);
    if card_form && (3..=4).contains(&compact.len()) && compact.chars().all(|c| c.is_ascii_digit()) {
        return Some("payment card details");
    }
    None
}

/// The Luhn check of a run of digits. Pure.
fn luhn(digits: &str) -> bool {
    let mut sum = 0u32;
    for (i, c) in digits.chars().rev().enumerate() {
        let Some(mut d) = c.to_digit(10) else { return false };
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum.is_multiple_of(10)
}

/// An IBAN (two letters, two check digits, 11–30 letters and digits, its
/// mod-97 check), spaces already out. Pure.
fn iban(s: &str) -> bool {
    let s = s.to_ascii_uppercase();
    let b = s.as_bytes();
    if !(15..=34).contains(&b.len()) || !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() || !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return false;
    }
    if !b.iter().all(u8::is_ascii_alphanumeric) {
        return false;
    }
    let moved = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u64 = 0;
    for c in moved.chars() {
        let v = if c.is_ascii_digit() { c as u64 - '0' as u64 } else { c as u64 - 'A' as u64 + 10 };
        rem = if v >= 10 { (rem * 100 + v) % 97 } else { (rem * 10 + v) % 97 };
    }
    rem == 1
}

/// **The words of `text` that come from nowhere the person can see**: not
/// in what they said in this conversation, nor on the page (`known`: those
/// words, `words`'). A number of one or two digits (a day, an hour, a
/// count) and a year (1900–2099) need no source: a date the model works out
/// is fine. Empty: every word is accounted for. Pure.
pub fn unknown_words(text: &str, known: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words(text) {
        let digits = w.chars().all(|c| c.is_ascii_digit());
        let small = digits && w.len() <= 2;
        let year = digits && w.len() == 4 && matches!(&w[..2], "19" | "20");
        if small || year || known.contains(&w) || out.contains(&w) {
            continue;
        }
        out.push(w);
    }
    out
}

/// The words of `texts`, for [`unknown_words`]. Pure.
pub fn known_words<'a>(texts: impl IntoIterator<Item = &'a str>) -> HashSet<String> {
    texts.into_iter().flat_map(words).collect()
}

/// An element's name as the model and the chip see it: its accessible name,
/// else its label, placeholder, `name` or id; at most 80 characters. Pure.
pub fn name_of(e: &Element) -> String {
    let n = [&e.name, &e.label, &e.placeholder, &e.field, &e.id].into_iter().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).find(|s| !s.is_empty()).unwrap_or_default();
    clip(&n, 80)
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
pub fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// **The elements as the model reads them**, one a line:
///
/// ```text
/// [3] textbox "From" = "Kellbrook"
/// [5] button "Find trains" (submits the form)
/// [7] link "Buy tickets" → rail.example.com/buy?train=1 (asks the user first)
/// [9] textbox "Card number" (Yap never types here: payment card details)
/// ```
///
/// What Yap won't do or will ask about is said on the line, so the model
/// knows before it tries. Pure.
pub fn element_lines(snap: &Snapshot) -> String {
    let mut out = Vec::new();
    for e in &snap.elements {
        let d = Described {
            el: Some(e.clone()),
            form: (e.form >= 0).then(|| snap.forms.iter().find(|f| f.form == e.form).cloned()).flatten(),
            password: snap.password,
            payment: snap.payment,
            title: snap.title.clone(),
            headings: snap.headings.clone(),
            url: snap.url.clone(),
        };
        let mut line = format!("[{}] {} \"{}\"", e.r, if e.role.is_empty() { "element" } else { &e.role }, name_of(e).replace('"', "'"));
        if e.role == "link" && !e.href.is_empty() {
            line.push_str(&format!(" → {}", crate::web::short_url(&e.href)));
        }
        let why = sensitive(e);
        if !e.value.is_empty() && why.is_none() && !e.masked {
            line.push_str(&format!(" = \"{}\"", clip(&e.value, 60).replace('"', "'")));
        }
        if let Some(c) = e.checked {
            line.push_str(if c { " (checked)" } else { " (not checked)" });
        }
        if !e.options.is_empty() {
            let shown: Vec<String> = e.options.iter().take(12).map(|o| clip(o, 30)).collect();
            line.push_str(&format!(" options: {}", shown.join(" | ")));
        }
        if e.disabled {
            line.push_str(" (disabled)");
        }
        if e.submit {
            line.push_str(" (submits the form)");
        }
        if let Some(w) = why {
            line.push_str(&format!(" (Yap never types here: {w})"));
        } else if takes_text(e) && sign_in_field(&d, e) {
            line.push_str(" (a sign-in field: the user signs in themselves)");
        } else if !takes_text(e) && click_needs_ok(&d, e).is_some() {
            line.push_str(" (asks the user first)");
        }
        out.push(line);
    }
    if snap.more {
        out.push(format!("(more than {MAX_ELEMENTS}: scroll and read again for the rest)"));
    }
    out.join("\n")
}

/// **What changed after an action**, briefly: the page went somewhere else
/// (its address), or the lines of text that appeared, or nothing. Pure.
pub fn what_changed(before_url: &str, before_text: &str, after_url: &str, after_text: &str) -> String {
    let same = |a: &str, b: &str| crate::web::normal_url(a) == crate::web::normal_url(b);
    if !same(before_url, after_url) {
        return format!("went to {}", crate::web::short_url(after_url));
    }
    let had: std::collections::HashSet<&str> = sentences(before_text).into_iter().collect();
    let new: Vec<&str> = sentences(after_text).into_iter().filter(|s| !had.contains(s)).collect();
    if new.is_empty() {
        let gone = sentences(before_text).len().saturating_sub(sentences(after_text).len());
        return if gone > 0 { "some of the page's text went away".to_string() } else { "nothing visible changed".to_string() };
    }
    format!("new on the page: {}", clip(&new.join(" "), 300))
}

/// A text's sentences (and lines), for [`what_changed`]. Pure.
fn sentences(text: &str) -> Vec<&str> {
    text.split(['\n', '.', '!', '?']).map(str::trim).filter(|s| s.chars().count() > 2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: &str, name: &str) -> Element {
        Element { r: 1, role: "textbox".into(), name: name.into(), kind: kind.into(), form: -1, ..Default::default() }
    }

    #[test]
    fn words_split_at_case_and_punctuation() {
        assert_eq!(words("cardNumber"), ["card", "number"]);
        assert_eq!(words("cc-number"), ["cc", "number"]);
        assert_eq!(words("Sort code"), ["sort", "code"]);
        assert_eq!(words("billing_IBAN"), ["billing", "iban"]);
        assert_eq!(words("Railcard"), ["railcard"]);
        assert_eq!(words(""), Vec::<String>::new());
    }

    #[test]
    fn yap_never_types_passwords_card_or_bank_details() {
        // By type and autocomplete.
        assert_eq!(sensitive(&field("password", "Enter it")), Some("a password"));
        assert_eq!(sensitive(&Element { autocomplete: "current-password".into(), ..field("text", "x") }), Some("a password"));
        assert_eq!(sensitive(&Element { autocomplete: "new-password".into(), ..field("text", "x") }), Some("a password"));
        assert_eq!(sensitive(&Element { autocomplete: "cc-number".into(), ..field("text", "Number") }), Some("payment card details"));
        assert_eq!(sensitive(&Element { autocomplete: "billing cc-exp".into(), ..field("text", "x") }), Some("payment card details"));
        assert_eq!(sensitive(&Element { autocomplete: "one-time-code".into(), ..field("text", "x") }), Some("a security code"));
        // By its name, id, label or placeholder.
        for (name, why) in [
            ("Card number", "payment card details"),
            ("Name on card", "payment card details"),
            ("CVV", "payment card details"),
            ("Security code (CVC)", "payment card details"),
            ("IBAN", "bank details"),
            ("Sort code", "bank details"),
            ("Account number", "bank details"),
            ("Security code", "a security code"),
            ("PIN", "a security code"),
            ("Your password", "a password"),
        ] {
            assert_eq!(sensitive(&field("text", name)), Some(why), "{name}");
        }
        assert_eq!(sensitive(&Element { field: "cardNumber".into(), ..field("tel", "") }), Some("payment card details"));
        assert_eq!(sensitive(&Element { id: "sortCode".into(), ..field("text", "") }), Some("bank details"));
        assert_eq!(sensitive(&Element { placeholder: "1234 5678 9012 3456".into(), field: "ccnum".into(), ..field("text", "") }), Some("payment card details"));
        // Not these: a railcard, a station, a date, shipping, a search.
        for name in ["Railcard", "From", "Travel date", "Shipping address", "Search the wiki", "Spinning top", "Postcode"] {
            assert_eq!(sensitive(&field("text", name)), None, "{name}");
        }
    }

    #[test]
    fn a_sign_in_form_is_the_persons_to_fill() {
        let email = Element { kind: "email".into(), name: "Email".into(), form: 0, ..field("email", "Email") };
        let in_form = Described { form: Some(Form { form: 0, password: true, ..Default::default() }), password: true, ..Default::default() };
        assert!(sign_in_field(&in_form, &email));
        // Any field of a form with a password field.
        assert!(sign_in_field(&in_form, &field("text", "Remember me as")));
        // A username field, anywhere (a two-step sign-in).
        assert!(sign_in_field(&Described::default(), &Element { autocomplete: "username".into(), ..field("text", "Phone or email") }));
        // An email field on a page that says it's for signing in.
        let page = Described { title: "Sign in - Example Rail".into(), ..Default::default() };
        assert!(sign_in_field(&page, &email));
        // A newsletter's email box on any other page isn't; nor a search box.
        assert!(!sign_in_field(&Described { title: "Timetables".into(), ..Default::default() }, &email));
        assert!(!sign_in_field(&page, &field("search", "Search")));
    }

    #[test]
    fn a_click_that_buys_pays_books_or_signs_in_asks_first() {
        for (label, kind) in [
            ("Buy tickets", "buy"),
            ("Proceed to checkout", "buy"),
            ("Check out", "buy"),
            ("Pay now", "pay"),
            ("Book now", "book"),
            ("Reserve a table", "book"),
            ("Place order", "place an order"),
            ("Confirm", "confirm"),
            ("Subscribe", "subscribe"),
            ("Sign up for free", "subscribe"),
            ("Send message", "send"),
            ("Submit", "send"),
            ("Delete account", "delete"),
            ("Remove", "delete"),
            ("Sign in", "sign in"),
            ("Log in", "sign in"),
            ("LOGIN", "sign in"),
        ] {
            assert_eq!(needs_ok(label, ""), Some(kind), "{label}");
        }
        // A link whose address says so.
        assert_eq!(needs_ok("Continue", "https://rail.example.com/checkout?basket=4"), Some("buy"));
        assert_eq!(needs_ok("Next", "https://shop.example.com/payment/card"), Some("pay"));
        // Moving about, searching, reading: no.
        for label in ["Find trains", "Search", "Next page", "Timetables", "Fort Joy", "Facebook", "Books", "Reject non-essential cookies", "Show more"] {
            assert_eq!(needs_ok(label, "https://rail.example.com/timetable"), None, "{label}");
        }
        // A sign-in form's submit button signs in, whatever it says.
        let d = Described { form: Some(Form { form: 0, password: true, submit: "Continue".into(), ..Default::default() }), ..Default::default() };
        let go = Element { role: "button".into(), name: "Continue".into(), submit: true, form: 0, ..Default::default() };
        assert_eq!(click_needs_ok(&d, &go), Some("sign in"));
        assert_eq!(submit_needs_ok(&d), Some("sign in"));
        // A search form's Enter goes ahead; a payment form's asks.
        let search = Described { form: Some(Form { form: 0, submit: "Find trains".into(), action: "https://rail.example.com/results".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(submit_needs_ok(&search), None);
        let pay = Described { form: Some(Form { form: 0, submit: "Pay £42.10".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(submit_needs_ok(&pay), Some("pay"));
        assert_eq!(submit_needs_ok(&Described::default()), None);
    }

    #[test]
    fn the_elements_are_told_one_a_line_with_what_yap_wont_do() {
        let snap = Snapshot {
            title: "Example Rail".into(),
            url: "https://rail.example.com/".into(),
            elements: vec![
                Element { r: 1, role: "link".into(), name: "Timetables".into(), href: "https://rail.example.com/timetables".into(), form: -1, ..Default::default() },
                Element { r: 2, role: "textbox".into(), name: "From".into(), value: "Kellbrook".into(), form: 0, ..Default::default() },
                Element { r: 3, role: "combobox".into(), name: "When".into(), options: vec!["Today".into(), "Tomorrow".into()], value: "Today".into(), form: 0, ..Default::default() },
                Element { r: 4, role: "button".into(), name: "Find trains".into(), submit: true, form: 0, ..Default::default() },
                Element { r: 5, role: "link".into(), name: "Buy tickets".into(), href: "https://rail.example.com/buy?train=1".into(), form: -1, ..Default::default() },
                Element { r: 6, role: "textbox".into(), name: "Card number".into(), value: "4111".into(), form: 1, ..Default::default() },
                Element { r: 7, role: "checkbox".into(), name: "Return".into(), checked: Some(false), form: 0, ..Default::default() },
            ],
            forms: vec![Form { form: 0, submit: "Find trains".into(), ..Default::default() }, Form { form: 1, submit: "Pay".into(), ..Default::default() }],
            more: true,
            ..Default::default()
        };
        let lines = element_lines(&snap);
        assert_eq!(
            lines,
            [
                "[1] link \"Timetables\" → rail.example.com/timetables",
                "[2] textbox \"From\" = \"Kellbrook\"",
                "[3] combobox \"When\" = \"Today\" options: Today | Tomorrow",
                "[4] button \"Find trains\" (submits the form)",
                "[5] link \"Buy tickets\" → rail.example.com/buy?train=1 (asks the user first)",
                "[6] textbox \"Card number\" (Yap never types here: payment card details)",
                "[7] checkbox \"Return\" (not checked)",
                "(more than 150: scroll and read again for the rest)",
            ]
            .join("\n")
        );
        // A card's number never reaches the model, even if the script sent one.
        assert!(!lines.contains("4111"));
    }

    #[test]
    fn what_changed_is_said_briefly() {
        assert_eq!(what_changed("https://rail.example.com/", "a", "https://rail.example.com/results?from=A", "b"), "went to rail.example.com/results?from=A");
        assert_eq!(what_changed("https://a.example/x", "Hello there. Pick a day.", "https://a.example/x/", "Hello there. Pick a day. No trains on Sunday."), "new on the page: No trains on Sunday");
        assert_eq!(what_changed("https://a.example/x", "Same words.", "https://a.example/x", "Same words."), "nothing visible changed");
        assert_eq!(what_changed("https://a.example/x", "One. We use cookies to track you.", "https://a.example/x", "One."), "some of the page's text went away");
        assert!(takes_text(&field("text", "x")) && !takes_text(&field("checkbox", "x")));
        assert!(!takes_text(&Element { role: "button".into(), ..Default::default() }));
        assert_eq!(name_of(&Element { placeholder: "  Search  the wiki ".into(), ..Default::default() }), "Search the wiki");
    }

    #[test]
    fn shop_buttons_ask_first_in_every_language_yap_speaks() {
        for (label, kind) in [
            // German, French, Spanish, Italian, Portuguese, Dutch shops.
            ("Jetzt kaufen", "buy"),
            ("In den Warenkorb", "buy"),
            ("Zur Kasse", "buy"),
            ("Zahlungspflichtig bestellen", "pay"),
            ("Jetzt bestellen", "place an order"),
            ("Bestätigen", "confirm"),
            ("Kündigen", "cancel"),
            ("Löschen", "delete"),
            ("Anmelden", "sign in"),
            ("Commander", "place an order"),
            ("Ajouter au panier", "buy"),
            ("Payer maintenant", "pay"),
            ("Réserver", "book"),
            ("Se connecter", "sign in"),
            ("S'abonner", "subscribe"),
            ("Envoyer", "send"),
            ("Supprimer", "delete"),
            ("Comprar", "buy"),
            ("Pagar", "pay"),
            ("Realizar pedido", "place an order"),
            ("Iniciar sesión", "sign in"),
            ("Darse de baja", "cancel"),
            ("Acquista ora", "buy"),
            ("Aggiungi al carrello", "buy"),
            ("Prenota", "book"),
            ("Finalizar compra", "buy"),
            ("Adicionar ao carrinho", "buy"),
            ("Afrekenen", "buy"),
            ("In winkelwagen", "buy"),
            ("Nu betalen", "pay"),
            ("Ik ga akkoord", "agree"),
            // The English gaps.
            ("Add to bag", "buy"),
            ("Start free trial", "subscribe"),
            ("Cancel membership", "cancel"),
            ("Close account", "delete"),
            ("I agree", "agree"),
        ] {
            assert_eq!(needs_ok(label, ""), Some(kind), "{label}");
        }
        // Moving about, searching, a dialog's Cancel, a cookie banner's buttons: no.
        for label in ["Cancel", "Accept all", "Weiter", "Suche", "Rechercher", "Continuer", "Siguiente", "Avanti", "Próximo", "Volgende", "Alle akzeptieren", "Tout refuser"] {
            assert_eq!(needs_ok(label, "https://shop.example.de/produkt/42"), None, "{label}");
        }
        // An address in another language says so too.
        assert_eq!(needs_ok("Weiter", "https://shop.example.de/kasse/adresse"), Some("buy"));
        assert_eq!(needs_ok("Continuer", "https://boutique.example.fr/panier"), Some("buy"));
        assert_eq!(needs_ok("Unsubscribe me", ""), Some("subscribe"));
        assert_eq!(needs_ok("Click here", "https://news.example/newsletter/unsubscribe?t=abc"), Some("subscribe"));
        assert_eq!(needs_ok("Read more", "https://news.example/post/123"), None, "a blog's /post is no send");
    }

    #[test]
    fn a_plainly_named_submit_asks_by_its_form_and_enter_outside_a_form_asks() {
        // "Continue" in a form posting to …/place-order: asked, as Enter in the form is.
        let order = Form { form: 0, action: "https://shop.example.com/checkout/place-order".into(), method: "post".into(), submit: "Continue".into(), ..Default::default() };
        let d = Described { form: Some(order), ..Default::default() };
        let go = Element { role: "button".into(), name: "Continue".into(), submit: true, form: 0, ..Default::default() };
        assert_eq!(click_needs_ok(&d, &go), Some("buy"));
        assert_eq!(submit_needs_ok(&d), Some("buy"));
        // A form that takes a card, whatever its button says.
        let pay = Described { form: Some(Form { form: 0, payment: true, submit: "Weiter".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(click_needs_ok(&pay, &Element { name: "Weiter".into(), ..go.clone() }), Some("pay"));
        // A posted form that shows a price buys; a button with a price on it pays.
        let priced = Described { form: Some(Form { form: 0, price: true, method: "post".into(), submit: "Weiter".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(submit_needs_ok(&priced), Some("buy"));
        let button = Element { role: "button".into(), name: "19,99 €".into(), form: -1, ..Default::default() };
        assert_eq!(click_needs_ok(&Described::default(), &button), Some("pay"));
        // …but a search form's button, a GET form with prices (a filter), goes ahead.
        let search = Described { form: Some(Form { form: 0, price: true, method: "get".into(), submit: "Find trains".into(), action: "https://rail.example.com/results".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(click_needs_ok(&search, &Element { name: "Find trains".into(), ..go.clone() }), None);
        // Enter in a field outside any form: a chat's message box sends — asked;
        // a search box (by type, role, region, name or words) doesn't.
        let none = Described::default();
        let message = Element { role: "textbox".into(), name: "Type a message".into(), editable: true, form: -1, ..Default::default() };
        assert_eq!(enter_needs_ok(&none, &message), Some("send"));
        for search in [
            Element { kind: "search".into(), ..message.clone() },
            Element { role: "searchbox".into(), ..message.clone() },
            Element { search: true, ..message.clone() },
            Element { name: String::new(), field: "q".into(), ..message.clone() },
            Element { name: "Suche".into(), ..message.clone() },
            Element { name: "Rechercher sur le site".into(), ..message.clone() },
        ] {
            assert_eq!(enter_needs_ok(&none, &search), None, "{search:?}");
        }
        // In a form: the form's own rules.
        assert_eq!(enter_needs_ok(&d, &message), Some("buy"));
    }

    #[test]
    fn a_card_number_an_iban_or_a_code_beside_a_card_is_refused_whatever_the_field() {
        let any = Described::default();
        assert_eq!(typed_secret("4111 1111 1111 1111", &any), Some("payment card details"));
        assert_eq!(typed_secret("my card is 4000-0566-5566-5556 thanks", &any), Some("payment card details"));
        assert_eq!(typed_secret("4111 1111 1111 1112", &any), None, "not a Luhn number");
        assert_eq!(typed_secret("GB82 WEST 1234 5698 7654 32", &any), Some("bank details"));
        assert_eq!(typed_secret("DE89370400440532013000", &any), Some("bank details"));
        // A phone number, a train, a date: fine.
        for ok in ["07700 900123", "0942", "2026-11-02", "Port Ellis"] {
            assert_eq!(typed_secret(ok, &any), None, "{ok}");
        }
        // Three or four digits into a form that takes a card: its security code.
        let card_form = Described { form: Some(Form { form: 0, payment: true, ..Default::default() }), ..Default::default() };
        assert_eq!(typed_secret("123", &card_form), Some("payment card details"));
        assert_eq!(typed_secret("123", &any), None);
    }

    #[test]
    fn card_password_and_bank_fields_are_known_in_every_language() {
        for (name, why) in [
            ("Kartennummer", "payment card details"),
            ("Kreditkartennummer", "payment card details"),
            ("Numéro de carte", "payment card details"),
            ("Número de tarjeta", "payment card details"),
            ("Numero della carta", "payment card details"),
            ("Número do cartão", "payment card details"),
            ("Kaartnummer", "payment card details"),
            ("Passwort", "a password"),
            ("Mot de passe", "a password"),
            ("Contraseña", "a password"),
            ("Senha", "a password"),
            ("Wachtwoord", "a password"),
            ("Kontonummer", "bank details"),
            ("Numéro de compte", "bank details"),
            ("Rekeningnummer", "bank details"),
            ("Sicherheitscode", "a security code"),
            ("Código de seguridad", "a security code"),
        ] {
            assert_eq!(sensitive(&field("text", name)), Some(why), "{name}");
        }
        // Their sign-in pages and user fields too.
        let page = Described { title: "Anmelden - Beispielbahn".into(), ..Default::default() };
        assert!(sign_in_field(&page, &field("text", "Benutzername")));
        assert!(sign_in_field(&Described { headings: vec!["Iniciar sesión".into()], ..Default::default() }, &field("text", "Usuario")));
        assert!(!sign_in_field(&Described { title: "Fahrplan".into(), ..Default::default() }, &field("text", "Benutzername")));
        // A search for a keyword isn't a password.
        assert_eq!(sensitive(&field("text", "Palabra clave")), None);
    }

    #[test]
    fn prices_and_words_from_nowhere_are_spotted() {
        for p in ["Pay £42.10", "19,99 €", "€ 5", "USD 30", "30 USD", "kr 99", "$1"] {
            assert!(has_price(p), "{p}");
        }
        for p in ["Find trains", "Page 2", "£", "Top 10", "€uro"] {
            assert!(!has_price(p), "{p}");
        }
        let known = known_words(["Find the next train from Kellbrook to Port Ellis", "Kellbrook to Port Ellis: trains today"]);
        assert!(unknown_words("Kellbrook", &known).is_empty());
        assert!(unknown_words("port ellis", &known).is_empty());
        assert!(unknown_words("2026-11-02 09:42", &known).is_empty(), "a date and a time need no source");
        assert_eq!(unknown_words("sam@example.com", &known), ["sam", "example", "com"]);
        assert_eq!(unknown_words("Réserver Kellbrook", &known), ["reserver"]);
    }
}
