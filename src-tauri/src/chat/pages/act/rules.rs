//! **The rules Yap acts by on a page** (pure): what a page's links, buttons
//! and fields are (as the element script reads them, `script.rs`, or the
//! stand-in browser, `pages/sim.rs`), which fields Yap never types into,
//! which forms are sign-in forms, which clicks ask the person first, and how
//! the elements and an action's outcome are told to the model.
//!
//! Enforced here, in Rust, whatever the model was told: a page or a model
//! that words things cleverly can't talk its way past them.

use serde::{Deserialize, Serialize};

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
}

/// A form on the page: whether it has a password field (a sign-in form),
/// where it goes, and its submit button's words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Form {
    pub form: i64,
    pub password: bool,
    pub action: String,
    pub method: String,
    pub submit: String,
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
    /// More elements than [`MAX_ELEMENTS`] were there.
    pub more: bool,
    pub scroll: Scroll,
}

/// One element looked up by its ref just before an action, with what the
/// rules need around it: its form, whether the page has a password field,
/// the page's title and headings (a sign-in page says so).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Described {
    /// `None`: no element has that ref now (the page changed, or it's gone).
    pub el: Option<Element>,
    pub form: Option<Form>,
    pub password: bool,
    pub title: String,
    pub headings: Vec<String>,
    pub url: String,
}

/// The most elements a read lists.
pub const MAX_ELEMENTS: usize = 150;
/// The most a field gets typed into it at once (characters): enough for a
/// station, a date, a search; never a note's worth of the person's words.
pub const MAX_TYPED: usize = 200;

/// `s` as lower-case words: split at anything not a letter or a digit, and
/// at a lower-to-upper change ("cardNumber" → card, number). Pure.
pub fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
            prev_lower = c.is_lowercase() || c.is_ascii_digit();
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

/// Whether `ws` holds `pair` side by side ("sort", "code"), or the two run
/// together as one word ("sortcode"). Pure.
fn has_pair(ws: &[String], pair: (&str, &str)) -> bool {
    let joined = format!("{}{}", pair.0, pair.1);
    ws.contains(&joined) || ws.windows(2).any(|p| p[0] == pair.0 && p[1] == pair.1)
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
/// (card, CVV, CVC, IBAN, sort code, account number, security code, PIN).
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
    let any = |list: &[&str]| ws.iter().any(|w| list.contains(&w.as_str()));
    if any(&["password", "passwd", "passcode", "passphrase"]) {
        return Some("a password");
    }
    if any(&["card", "cardnumber", "ccnumber", "ccnum", "creditcard", "debitcard", "cvv", "cvv2", "cvc", "cvc2", "csc", "cardholder"])
        || has_pair(&ws, ("card", "number"))
        || has_pair(&ws, ("expiry", "date"))
    {
        return Some("payment card details");
    }
    if any(&["iban", "bic", "swift", "routing"]) || has_pair(&ws, ("sort", "code")) || has_pair(&ws, ("account", "number")) || has_pair(&ws, ("account", "no")) {
        return Some("bank details");
    }
    if any(&["pin", "otp", "2fa", "totp"]) || has_pair(&ws, ("security", "code")) || has_pair(&ws, ("verification", "code")) {
        return Some("a security code");
    }
    None
}

/// Words that make a page a sign-in page (its title or a heading).
fn says_sign_in(text: &str) -> bool {
    let ws = words(text);
    has_pair(&ws, ("sign", "in")) || has_pair(&ws, ("log", "in")) || ws.iter().any(|w| w == "login" || w == "logon")
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
    let ws = known_by(e);
    let userish = e.kind.eq_ignore_ascii_case("email")
        || auto.split_whitespace().any(|a| a == "email")
        || ws.iter().any(|w| matches!(w.as_str(), "username" | "user" | "login" | "email" | "userid"))
        || has_pair(&ws, ("user", "name"))
        || has_pair(&ws, ("e", "mail"));
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

/// **What a click would do that asks the person first**: a click (or a
/// submit) that would buy, pay, book, confirm, place an order, subscribe,
/// send, delete or sign in — by the button's or link's words, and a link's
/// address (…/checkout, …/pay). The kind, for the log and the evaluation;
/// `None`: it goes ahead. Pure.
pub fn needs_ok(label: &str, href: &str) -> Option<&'static str> {
    let ws = words(label);
    let any = |list: &[&str]| ws.iter().any(|w| list.contains(&w.as_str()));
    if has_pair(&ws, ("sign", "in")) || has_pair(&ws, ("log", "in")) || has_pair(&ws, ("log", "on")) || any(&["login", "logon", "signin"]) {
        return Some("sign in");
    }
    if any(&["buy", "purchase", "checkout", "basket", "cart"]) || has_pair(&ws, ("check", "out")) {
        return Some("buy");
    }
    if any(&["pay", "payment", "paynow", "donate", "transfer", "withdraw", "deposit", "bid"]) {
        return Some("pay");
    }
    if any(&["book", "booking", "reserve", "reservation"]) {
        return Some("book");
    }
    if any(&["order", "checkout"]) {
        return Some("place an order");
    }
    if any(&["confirm", "confirmation"]) {
        return Some("confirm");
    }
    if any(&["subscribe", "unsubscribe", "subscription", "register", "signup", "enrol", "enroll"]) || has_pair(&ws, ("sign", "up")) {
        return Some("subscribe");
    }
    if any(&["send", "submit", "post", "publish", "reply", "share"]) {
        return Some("send");
    }
    if any(&["delete", "remove", "erase", "discard"]) {
        return Some("delete");
    }
    // A link's address says what it does (a "Continue" to /checkout).
    let path = reqwest::Url::parse(href.trim()).map(|u| u.path().to_string()).unwrap_or_default();
    let pw = words(&path);
    let pany = |list: &[&str]| pw.iter().any(|w| list.contains(&w.as_str()));
    if pany(&["checkout", "buy", "purchase", "basket", "cart"]) {
        return Some("buy");
    }
    if pany(&["pay", "payment", "payments"]) {
        return Some("pay");
    }
    if pany(&["booking", "book"]) {
        return Some("book");
    }
    if pany(&["login", "signin", "logon"]) {
        return Some("sign in");
    }
    None
}

/// What clicking `e` would do that asks first ([`needs_ok`]): its words, and
/// a link's address; a form's submit button in a sign-in form signs in.
/// Pure.
pub fn click_needs_ok(d: &Described, e: &Element) -> Option<&'static str> {
    if e.submit && d.form.as_ref().is_some_and(|f| f.password) {
        return Some("sign in");
    }
    needs_ok(&e.name, &e.href).or_else(|| (e.name.trim().is_empty()).then(|| needs_ok(&e.label, "")).flatten())
}

/// What submitting `e`'s form (Enter in it) would do that asks first: its
/// submit button's words and where the form goes. Pure.
pub fn submit_needs_ok(d: &Described) -> Option<&'static str> {
    let f = d.form.as_ref()?;
    if f.password {
        return Some("sign in");
    }
    needs_ok(&f.submit, &f.action)
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
}
