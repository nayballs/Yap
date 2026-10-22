//! Memory's items and the changes a model (or the person) proposes, applied
//! **as changes, never as a rewrite**: add, update (facts added or taken
//! out, a new "about"), remove. Everything here is pure: the store calls
//! [`apply`] on its items and keeps the result.
//!
//! The rules Yap keeps whatever the model wrote:
//! - **bounded**: names ≤ 60 characters, "about" ≤ 100, a fact ≤ 140, ≤ 8
//!   facts an item (16 for Profile and Preferences, the newest kept), ≤ 60
//!   people, 30 projects, 30 topics (a full section makes room by dropping
//!   what was updated longest ago, never something the person wrote);
//! - **one item per thing**: an "add" for a person, project or topic Yap
//!   already has (the same name, or the same first name) updates it instead,
//!   and a fact it already has isn't added twice;
//! - **no secrets** (`guard::secret_in`), and while sensitive topics are off,
//!   none of those in what Yap learned by itself (`guard::sensitive_in`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::guard;

/// A section of memory. Profile and Preferences ("You") are one item each;
/// the others one item per person, project or topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Profile,
    Preferences,
    People,
    Projects,
    Topics,
}

impl Section {
    /// A section as a model or a file names it ("person", "Projects", "you").
    pub fn parse(s: &str) -> Option<Section> {
        let s = s.trim().to_lowercase();
        Some(match s.trim_end_matches('s') {
            "profile" | "you" | "about" | "user" | "fact" => Section::Profile,
            "preference" | "style" => Section::Preferences,
            "people" | "person" | "peopl" | "contact" => Section::People,
            "project" | "work" => Section::Projects,
            "topic" | "interest" | "area" | "hobby" | "hobbie" => Section::Topics,
            _ => return None,
        })
    }

    pub fn key(self) -> &'static str {
        match self {
            Section::Profile => "profile",
            Section::Preferences => "preferences",
            Section::People => "people",
            Section::Projects => "projects",
            Section::Topics => "topics",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Profile => "Profile",
            Section::Preferences => "Preferences",
            Section::People => "People",
            Section::Projects => "Projects",
            Section::Topics => "Topics",
        }
    }

    /// "person", "project", "topic" (in a prompt's listing).
    pub fn singular(self) -> &'static str {
        match self {
            Section::Profile => "profile",
            Section::Preferences => "preferences",
            Section::People => "person",
            Section::Projects => "project",
            Section::Topics => "topic",
        }
    }

    /// Profile and Preferences: about the person themselves.
    pub fn is_you(self) -> bool {
        matches!(self, Section::Profile | Section::Preferences)
    }

    pub fn max_items(self) -> usize {
        match self {
            Section::Profile | Section::Preferences => 1,
            Section::People => 60,
            Section::Projects | Section::Topics => 30,
        }
    }

    pub fn max_facts(self) -> usize {
        if self.is_you() {
            16
        } else {
            8
        }
    }
}

pub const MAX_NAME: usize = 60;
pub const MAX_ABOUT: usize = 100;
pub const MAX_FACT: usize = 140;
/// "say" (the toast's words).
pub const MAX_SAY: usize = 90;

/// Where an item (or its latest change) came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    /// "chat" | "meeting" | "manual" | "import" | "tell" | "voice".
    pub kind: String,
    /// The conversation's or the meeting note's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Its title, as it was then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

impl Origin {
    pub fn new(kind: &str, id: Option<u64>, title: &str) -> Origin {
        Origin { kind: kind.to_string(), id, title: one_line(title, 80) }
    }

    /// The person wrote it (Settings, "Tell Yap", an import they reviewed):
    /// never dropped to make room, and sensitive topics are theirs to keep.
    pub fn by_the_person(&self) -> bool {
        matches!(self.kind.as_str(), "manual" | "tell" | "import" | "voice")
    }
}

fn yes() -> bool {
    true
}

/// One thing Yap remembers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: u64,
    pub section: Section,
    /// The person's, project's or topic's name ("Rochelle"); "Profile" /
    /// "Preferences" for those.
    pub title: String,
    /// Who or what it is to the person ("Sister", "Colleague at Acme").
    #[serde(default)]
    pub summary: String,
    /// Short facts ("Lives in Leeds"), oldest first.
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default)]
    pub source: Origin,
    #[serde(default)]
    pub created_ts: u64,
    #[serde(default)]
    pub updated_ts: u64,
    /// People and projects: "Spell this name in dictation" (`vocab.rs`).
    #[serde(default = "yes")]
    pub dictation: bool,
}

/// What a change does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Add,
    Update,
    Remove,
}

/// A proposed change, as read from a model's reply (or built by Yap).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub action: Action,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub section: Option<Section>,
    /// add: the name; update: a new name (rare).
    #[serde(default)]
    pub name: String,
    /// add: who or what it is; update: a new one (`None`: unchanged).
    #[serde(default)]
    pub about: Option<String>,
    /// add: its facts; update: facts to add.
    #[serde(default)]
    pub facts: Vec<String>,
    /// update: facts to take out, as written.
    #[serde(default)]
    pub remove_facts: Vec<String>,
    /// For the person, in a few words ("your sister is Rochelle").
    #[serde(default)]
    pub say: String,
}

/// Whitespace runs as one space, trimmed, at most `max` characters (cut at
/// a word when there's one in the last fifth).
pub fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    match cut.rfind(' ') {
        Some(i) if i > cut.len() * 4 / 5 => cut[..i].trim_end_matches([',', ';', ':']).to_string(),
        _ => cut,
    }
}

/// The JSON object in a model's reply (fenced or not, words around it).
fn json_in(reply: &str) -> Option<Value> {
    let t = reply.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    let unfenced = t
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<Value>(unfenced) {
        return Some(v);
    }
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (t.find(open), t.rfind(close)) {
            if a < b {
                if let Ok(v) = serde_json::from_str::<Value>(&t[a..=b]) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// A whole number under any of `keys`: 12, 12.0, "12", "[12]", "#12".
fn id_of(v: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| match &v[*k] {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().trim_matches(|c: char| !c.is_ascii_digit()).parse().ok(),
        _ => None,
    })
}

fn text_of(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| v[*k].as_str().map(str::to_string))
}

fn list_of(v: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .flat_map(|k| match &v[*k] {
            Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            Value::String(s) if !s.trim().is_empty() => vec![s.clone()],
            _ => Vec::new(),
        })
        .collect()
}

/// The changes in a model's reply: `{"changes": [...]}` (or `ops`,
/// `memory`, a bare array), each `{action, id?, section?, name?, about?,
/// facts?, add_facts?, remove_facts?, say?}` — lenient about the keys and
/// shapes small models write. Unreadable → empty. Pure.
pub fn parse_changes(reply: &str) -> Vec<Change> {
    let Some(v) = json_in(reply) else {
        return Vec::new();
    };
    let list = match &v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => ["changes", "ops", "memory", "updates", "items"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_array).cloned())
            .unwrap_or_else(|| if o.contains_key("action") { vec![v.clone()] } else { Vec::new() }),
        _ => Vec::new(),
    };
    list.iter()
        .filter_map(|c| {
            let action = match text_of(c, &["action", "op", "type", "kind"])?.trim().to_lowercase().as_str() {
                "add" | "create" | "new" | "remember" => Action::Add,
                "update" | "edit" | "change" | "merge" => Action::Update,
                "remove" | "delete" | "forget" => Action::Remove,
                _ => return None,
            };
            let about = text_of(c, &["about", "summary", "relation", "description"]);
            let mut facts = list_of(c, &["facts", "details", "add_facts", "add_details"]);
            if action == Action::Add && facts.is_empty() {
                facts = list_of(c, &["fact"]);
            }
            Some(Change {
                action,
                id: id_of(c, &["id", "item", "item_id"]),
                section: text_of(c, &["section", "category"]).and_then(|s| Section::parse(&s)),
                name: text_of(c, &["name", "title"]).unwrap_or_default(),
                about,
                facts,
                remove_facts: list_of(c, &["remove_facts", "remove_details", "remove"]),
                say: text_of(c, &["say", "note", "message"]).unwrap_or_default(),
            })
        })
        .collect()
}

/// Does a fact say how the person wants answers ("Prefers concise answers
/// with British spelling", "Always use metric units")? Pure.
pub fn looks_like_preference(fact: &str) -> bool {
    static PREF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^\s*(?:prefers?|likes?|wants?|always|never|uses?|keep|no |avoid|loves?|hates?|dislikes?|asked (?:me |you )?to)\b.*\b(?:answers?|repl(?:y|ies)|responses?|tone|spelling|format(?:ting)?|concise|brief|short|detailed|bullet(?: points)?|lists?|emojis?|metric|imperial|units|formal|casual|friendly)\b")
            .expect("static regex")
    });
    PREF.is_match(fact)
}

/// A pass's changes with preference-like facts headed for Profile moved to
/// Preferences: Qwen3.5-4B filed an import's "Prefers concise answers with
/// British spelling" under Profile, or as an update of an item that wasn't
/// there (2026-10-22). Pure.
pub fn route_preferences(changes: Vec<Change>, items: &[Item]) -> Vec<Change> {
    let mut out = Vec::new();
    for c in changes {
        let target = c.id.and_then(|id| items.iter().find(|i| i.id == id)).map(|i| i.section);
        let to_profile = c.action != Action::Remove
            && match target {
                Some(s) => s == Section::Profile,
                None => c.section.is_none_or(|s| s == Section::Profile),
            };
        if !to_profile {
            out.push(c);
            continue;
        }
        let (prefs, rest): (Vec<String>, Vec<String>) = c.facts.iter().cloned().partition(|f| looks_like_preference(f));
        if prefs.is_empty() {
            out.push(c);
            continue;
        }
        let about_left = c.about.as_deref().is_some_and(|a| !a.trim().is_empty());
        if !rest.is_empty() || about_left || !c.remove_facts.is_empty() {
            out.push(Change { facts: rest, ..c.clone() });
        }
        out.push(Change {
            action: Action::Add,
            id: None,
            section: Some(Section::Preferences),
            name: String::new(),
            about: None,
            facts: prefs,
            remove_facts: vec![],
            say: String::new(),
        });
    }
    out
}

/// **What About you already says, left out** of a pass's or an import's
/// changes to Profile and Preferences: About you's `name` and `note` go to
/// the AI with every question, so memory keeps no second copy (Qwen3.5-4B
/// kept "Works day shifts at a warehouse in Leeds" from a chat that
/// repeated the note, though its input said About you had it, 2026-10-22).
/// A fact goes when it's a short one naming the person (≤ 5 words with the
/// name: "Name is Declan") or nearly all its words are the note's ("I work
/// day shifts at a warehouse in Leeds." ⊇ "Works day shifts at a warehouse
/// in Leeds"); anything new or more specific stays ("Plays PC games on a
/// Steam Deck" next to "I play PC games"). A change left with nothing goes.
/// Pure.
pub fn drop_about_you(changes: Vec<Change>, items: &[Item], name: &str, note: &str) -> Vec<Change> {
    let name = norm(name);
    let note_words: std::collections::HashSet<String> = content_words(note).into_iter().collect();
    if name.is_empty() && note_words.is_empty() {
        return changes;
    }
    let repeats = |fact: &str| {
        let words = norm(fact);
        if !name.is_empty() && words.split(' ').count() <= 5 && format!(" {words} ").contains(&format!(" {name} ")) {
            return true;
        }
        let content = content_words(fact);
        content.len() >= 2 && content.iter().filter(|w| note_words.contains(*w)).count() * 5 >= content.len() * 4
    };
    let mut out = Vec::new();
    for c in changes {
        let target = c.id.and_then(|id| items.iter().find(|i| i.id == id)).map(|i| i.section);
        let you = c.action != Action::Remove
            && match target {
                Some(s) => s.is_you(),
                None => c.section.map_or(c.name.trim().is_empty(), |s| s.is_you()),
            };
        if !you {
            out.push(c);
            continue;
        }
        let facts: Vec<String> = c.facts.iter().filter(|f| !repeats(f)).cloned().collect();
        let about = c.about.clone().filter(|a| !a.trim().is_empty() && !repeats(a));
        if facts.is_empty() && about.is_none() && c.remove_facts.is_empty() {
            continue;
        }
        out.push(Change { facts, about, ..c });
    }
    out
}

/// A text's words that carry meaning, for [`drop_about_you`]: lowercased,
/// ≥ 3 characters, not a function word, a plural's or a verb's "s" off
/// ("works" = "work", "shifts" = "shift").
fn content_words(text: &str) -> Vec<String> {
    const FUNCTION: &[&str] = &[
        "the", "and", "for", "with", "from", "that", "this", "they", "them", "their", "theirs", "user", "users", "are",
        "was", "were", "has", "have", "had", "who", "its", "our", "you", "your", "his", "her", "him", "she", "about",
        "into", "onto", "but", "not", "also", "very", "just", "currently", "now",
    ];
    norm(text)
        .split(' ')
        .filter(|w| w.chars().count() >= 3 && !FUNCTION.contains(w))
        .map(|w| if w.chars().count() > 3 && w.ends_with('s') && !w.ends_with("ss") { w[..w.len() - 1].to_string() } else { w.to_string() })
        .collect()
}

/// A one-off request written as a fact ("Wanted a recipe for lasagne on 3
/// March", from an import, Qwen2.5-1.5B, 2026-10-22): a passing detail,
/// never kept from a pass or an import. Pure.
pub fn passing(fact: &str) -> bool {
    static ONE_OFF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^\s*(?:wanted|requested|looked up|searched(?: for)?|needed|was looking for|asked (?:for|about|how|what|where|when|which))\b")
            .expect("static regex")
    });
    ONE_OFF.is_match(fact)
}

/// A fact as memory keeps it: one line, no "The user …" in front, a
/// capital first, no full stop at the end, ≤ [`MAX_FACT`]. Empty when
/// there's nothing in it. Pure.
pub fn clean_fact(s: &str) -> String {
    let mut t = one_line(s, 400);
    for lead in ["- ", "• ", "* "] {
        if let Some(rest) = t.strip_prefix(lead) {
            t = rest.to_string();
        }
    }
    let lower = t.to_lowercase();
    for lead in ["the user's ", "the user ", "user's ", "user ", "they're ", "they are ", "they "] {
        if lower.starts_with(lead) {
            let rest = &t[lead.len()..];
            // "The user's sister lives in York" → "Sister lives in York";
            // "The user is a nurse" → "Is a nurse" → "A nurse" reads oddly,
            // so "is" stays as the verb ("Is a nurse").
            t = rest.to_string();
            break;
        }
    }
    let t = t.trim().trim_end_matches('.').trim().to_string();
    let mut chars = t.chars();
    let out = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    one_line(&out, MAX_FACT)
}

/// Could `name` be a person's name? At most four words, none of them a
/// small joining word ("Nurse at St James's in Leeds" isn't one: Qwen2.5-1.5B
/// filed that under people, 2026-10-22), and not a role alone. Pure.
pub fn looks_like_a_name(name: &str) -> bool {
    const JOINING: &[&str] = &["at", "in", "on", "from", "the", "a", "an", "who", "with", "for", "and", "of", "is", "my", "your", "their", "to"];
    let words: Vec<String> = name.split_whitespace().map(|w| w.to_lowercase()).collect();
    !words.is_empty()
        && words.len() <= 4
        && !words.iter().any(|w| JOINING.contains(&w.as_str()))
        && !matches!(words.as_slice(), [w] if NEEDS_A_NAME.contains(&w.as_str()))
}

/// Who someone is, which isn't a name by itself ("Mum" and "Grandad" are
/// how the person names them, so they can be).
const NEEDS_A_NAME: &[&str] = &[
    "sister", "brother", "colleague", "coworker", "friend", "boss", "manager", "dog", "cat", "pet", "partner", "wife",
    "husband", "girlfriend", "boyfriend", "son", "daughter", "cousin", "neighbour", "neighbor", "client", "customer",
    "teacher", "doctor", "user", "you", "person", "someone", "nurse", "child", "kid", "baby",
];

/// For comparing names and facts: lowercase letters, digits and spaces.
pub fn norm(s: &str) -> String {
    s.to_lowercase()
        .replace(['’', '\''], "")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The same fact, written a little differently: equal once normalised, or
/// one holds the other whole.
fn same_fact(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a == b || (a.len() >= 8 && b.contains(&a)) || (b.len() >= 8 && a.contains(&b)))
}

/// Is `name` the item already kept as `title` in its section? The same
/// name, or — for people — the same first name when one of the two is only
/// a first name ("Sarah" and "Sarah Jenkins").
pub fn same_thing(section: Section, title: &str, name: &str) -> bool {
    let (a, b) = (norm(title), norm(name));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    if section == Section::People {
        let (fa, fb) = (a.split(' ').next().unwrap_or(""), b.split(' ').next().unwrap_or(""));
        let single = !a.contains(' ') || !b.contains(' ');
        return single && fa == fb && fa.chars().count() >= 2;
    }
    false
}

/// How a batch of changes is applied.
#[derive(Debug, Clone)]
pub struct Rules {
    /// Sensitive topics may be kept (the setting is on, or the person asked
    /// for this change themselves).
    pub sensitive_ok: bool,
    pub origin: Origin,
    pub now: u64,
    /// At most this many changes are applied (the rest are dropped).
    pub max_changes: usize,
    /// What the change must come from: the person's own words in the chat
    /// (a pass), what they told Yap, the import's text. A name, an "about"
    /// or a fact with none of its words in it is left out (`grounded`):
    /// Qwen2.5-1.5B copied the prompt's example ("Tom, brother, lives in
    /// Glasgow") into a chat that never mentioned him (2026-10-22). `None`:
    /// not checked (the person's own edits, the chat's remember tool).
    pub grounding: Option<String>,
}

/// The words of `text` for grounding: lowercased letters and digits, ≥ 3
/// characters.
fn ground_words(text: &str) -> Vec<String> {
    norm(text).split(' ').filter(|w| w.chars().count() >= 3).map(str::to_string).collect()
}

/// One word in the source, or the same word with an ending ("dog" in
/// "dogs", "shift" in "shifts"), or one with the same first four letters
/// ("working" for "works").
fn word_in(w: &str, src: &[String]) -> bool {
    src.iter().any(|s| {
        s == w
            || (w.len().min(s.len()) >= 3 && (s.starts_with(w) || w.starts_with(s.as_str())))
            || (s.len() >= 4 && w.len() >= 4 && s.is_char_boundary(4) && w.is_char_boundary(4) && s[..4] == w[..4])
    })
}

/// Is `text` grounded in `source` ([`Rules::grounding`])? Its words that
/// aren't everyday ones: one of them is there ("Lives in Glasgow" needs
/// "Glasgow"); a text of everyday words only: half of them. Pure.
pub fn grounded(text: &str, source: &str) -> bool {
    let src = ground_words(source);
    let words = ground_words(text);
    if words.is_empty() {
        return true;
    }
    let content: Vec<&String> = words.iter().filter(|w| !crate::learn::is_frequent(w)).collect();
    if !content.is_empty() {
        return content.iter().any(|w| word_in(w, &src));
    }
    words.iter().filter(|w| word_in(w, &src)).count() * 2 >= words.len()
}

/// One change that went through.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    /// The change's index in the batch.
    pub change: usize,
    /// "added" | "updated" | "removed".
    pub kind: &'static str,
    pub id: u64,
    pub section: Section,
    pub title: String,
    /// Before → after ("" when there was none / is none).
    pub summary_before: String,
    pub summary: String,
    pub facts_added: Vec<String>,
    pub facts_removed: Vec<String>,
    /// The item's facts before the change (an update's diff, a removal's).
    pub facts_before: Vec<String>,
    /// For the person, in a few words.
    pub say: String,
}

/// The result of [`apply`].
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub done: Vec<Done>,
    /// What was left out and why, for the log (never the text itself).
    pub dropped: Vec<String>,
}

/// Words that say who someone is to the person ("Sister", "Colleague at
/// Acme"): "your sister".
const RELATIONS: &[&str] = &[
    "sister", "brother", "mum", "mom", "mother", "dad", "father", "wife", "husband", "partner", "son", "daughter", "friend",
    "best", "colleague", "coworker", "co-worker", "boss", "manager", "cousin", "aunt", "auntie", "uncle", "grandma",
    "grandmother", "nan", "nana", "gran", "grandad", "granddad", "grandfather", "grandpa", "niece", "nephew",
    "girlfriend", "boyfriend", "fiancé", "fiancée", "fiance", "fiancee", "flatmate", "housemate", "roommate",
    "neighbour", "neighbor", "teacher", "doctor", "dog", "cat", "pet", "stepmother", "stepfather", "stepson",
    "stepdaughter", "child", "kid", "baby", "twin", "client", "customer", "mentor", "team", "landlord", "dentist",
    "brother-in-law", "sister-in-law", "mother-in-law", "father-in-law", "ex", "godmother", "godfather", "godson",
    "goddaughter", "line", "direct", "report", "classmate", "teammate", "tutor", "coach", "vet", "parent", "parents",
];

fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        // "BBC" stays "BBC": only a capital followed by a lowercase letter.
        Some(c) if chars.clone().next().is_some_and(char::is_lowercase) => c.to_lowercase().collect::<String>() + chars.as_str(),
        Some(c) => c.to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// The person-facing words for a change Yap made, when the model gave none:
/// "Rochelle, your sister", "your project Yap", "works night shifts".
pub fn say_for(kind: &str, section: Section, title: &str, summary: &str, facts: &[String]) -> String {
    let first_fact = facts.first().map(|f| lower_first(f)).unwrap_or_default();
    let out = match section {
        _ if kind == "removed" && !section.is_you() => title.to_string(),
        Section::Profile | Section::Preferences => first_fact,
        Section::People => {
            let rel = norm(summary);
            let first = rel.split(' ').next().unwrap_or("");
            if summary.is_empty() {
                if first_fact.is_empty() { title.to_string() } else { format!("{title}: {first_fact}") }
            } else if RELATIONS.contains(&first) {
                format!("{title}, your {}", lower_first(summary))
            } else {
                format!("{title}, {}", lower_first(summary))
            }
        }
        Section::Projects if kind == "added" => format!("your project {title}"),
        Section::Projects | Section::Topics if !first_fact.is_empty() && kind == "updated" => format!("{title}: {first_fact}"),
        Section::Projects | Section::Topics => title.to_string(),
    };
    one_line(&out, MAX_SAY)
}

/// The cleaned text of `s`, or why it can't be kept (a secret, or a
/// sensitive topic while those aren't allowed).
fn kept(s: &str, name: &str, sensitive_ok: bool) -> Result<(), String> {
    if let Some(why) = guard::secret_in(s) {
        return Err(format!("left out {why}"));
    }
    if !sensitive_ok {
        if let Some(topic) = guard::sensitive_in(s, name) {
            return Err(format!("left out a sensitive topic ({topic})"));
        }
    }
    Ok(())
}

/// Add `facts` to `item` (deduplicated, newest kept within the cap).
/// Returns the facts actually added. `rules`: a pass (not the person) never
/// keeps another person's private life or health, whatever the setting.
fn add_facts(item: &mut Item, facts: &[String], rules: &Rules, dropped: &mut Vec<String>) -> Vec<String> {
    let sensitive_ok = rules.sensitive_ok;
    let someone_else = item.section == Section::People && !rules.origin.by_the_person();
    let mut added = Vec::new();
    for f in facts {
        let f = clean_fact(f);
        if f.is_empty() {
            continue;
        }
        if let Err(why) = kept(&f, &item.title, sensitive_ok && !someone_else) {
            dropped.push(why);
            continue;
        }
        if someone_else {
            if let Some(why) = guard::private_in(&f) {
                dropped.push(format!("left out {why}"));
                continue;
            }
        }
        if !matches!(rules.origin.kind.as_str(), "manual" | "tell" | "voice") && passing(&f) {
            dropped.push("left out a passing detail".into());
            continue;
        }
        if rules.grounding.as_deref().is_some_and(|src| !grounded(&f, src)) {
            dropped.push("left out something the user didn't say".into());
            continue;
        }
        // A fact that says the same thing more fully replaces the shorter.
        if let Some(i) = item.details.iter().position(|d| same_fact(d, &f)) {
            if norm(&f).len() > norm(&item.details[i]).len() {
                item.details[i] = f.clone();
                added.push(f);
            }
            continue;
        }
        if norm(&f) == norm(&item.title) || (!item.summary.is_empty() && norm(&f) == norm(&item.summary)) {
            continue;
        }
        item.details.push(f.clone());
        added.push(f);
    }
    let max = item.section.max_facts();
    if item.details.len() > max {
        let extra = item.details.len() - max;
        item.details.drain(..extra);
    }
    added
}

/// Take out the facts `remove` names (as written, or the same fact a little
/// differently) — or who or what the item is, when it names that: a pass
/// reads it among the facts ("Colleague at Acme | Product manager"), and
/// Qwen3.5-4B took "Colleague at Acme" out as one for "forget everything
/// about my old job at Acme" (2026-10-22). Returns what was removed.
fn remove_facts(item: &mut Item, remove: &[String]) -> Vec<String> {
    let mut removed = Vec::new();
    for r in remove {
        let r = clean_fact(r);
        if r.is_empty() {
            continue;
        }
        if let Some(i) = item.details.iter().position(|d| same_fact(d, &r)) {
            removed.push(item.details.remove(i));
        } else if !item.section.is_you() && !item.summary.is_empty() && same_fact(&item.summary, &r) {
            removed.push(std::mem::take(&mut item.summary));
        }
    }
    removed
}

/// Apply `changes` to `items` (in place) under `rules`; `next_id` gives new
/// items their ids. Returns what was done and what was dropped. Pure.
pub fn apply(items: &mut Vec<Item>, next_id: &mut u64, changes: &[Change], rules: &Rules) -> Outcome {
    let mut out = Outcome::default();
    for (index, c) in changes.iter().enumerate().take(rules.max_changes) {
        let say = one_line(&c.say, MAX_SAY);
        let say = if kept(&say, &c.name, rules.sensitive_ok).is_ok() { say } else { String::new() };
        // The item a change points at: its id, else (an add, or an update
        // without a known id) the same thing by name.
        let by_id = c.id.and_then(|id| items.iter().position(|i| i.id == id));
        // An update or removal is of the item its id names, whatever section
        // the model wrote (the grammar makes it write one); an add goes where
        // it says.
        let section = match c.action {
            Action::Add => c.section.or_else(|| by_id.map(|i| items[i].section)),
            _ => by_id.map(|i| items[i].section).or(c.section),
        };
        let by_name = || -> Option<usize> {
            let section = section?;
            if section.is_you() {
                return items.iter().position(|i| i.section == section);
            }
            items.iter().position(|i| i.section == section && same_thing(section, &i.title, &c.name))
        };
        match c.action {
            Action::Remove => {
                let Some(at) = by_id.or_else(|| (!c.name.trim().is_empty()).then(by_name).flatten()) else {
                    out.dropped.push("a removal of something Yap doesn't have".into());
                    continue;
                };
                // Profile and Preferences go fact by fact: a model removing
                // the whole Profile for "forget my old job" took where they
                // live and their dog with it (Qwen3.5-4B, 2026-10-22).
                if items[at].section.is_you() && c.remove_facts.is_empty() && rules.origin.kind != "manual" {
                    out.dropped.push(format!("kept {}: only its facts can be taken out", items[at].section.key()));
                    continue;
                }
                // A removal that names facts only takes those out (one whose
                // facts aren't there — a grammar-filled list naming the item's
                // "about" — removes the item, as "remove" says).
                let named_facts = items[at].details.iter().any(|d| c.remove_facts.iter().any(|r| same_fact(d, &clean_fact(r))));
                if named_facts || (items[at].section.is_you() && !c.remove_facts.is_empty()) {
                    let before = items[at].details.clone();
                    let removed = remove_facts(&mut items[at], &c.remove_facts);
                    if removed.is_empty() {
                        continue;
                    }
                    items[at].updated_ts = rules.now;
                    let it = &items[at];
                    out.done.push(Done {
                        change: index,
                        kind: "updated",
                        id: it.id,
                        section: it.section,
                        title: it.title.clone(),
                        summary_before: it.summary.clone(),
                        summary: it.summary.clone(),
                        facts_added: Vec::new(),
                        facts_removed: removed.clone(),
                        facts_before: before,
                        say: if say.is_empty() { lower_first(&removed[0]) } else { say },
                    });
                    continue;
                }
                let it = items.remove(at);
                out.done.push(Done {
                    change: index,
                    kind: "removed",
                    id: it.id,
                    section: it.section,
                    title: it.title.clone(),
                    summary_before: it.summary.clone(),
                    summary: String::new(),
                    facts_added: Vec::new(),
                    facts_removed: it.details.clone(),
                    facts_before: it.details.clone(),
                    say: if say.is_empty() { say_for("removed", it.section, &it.title, &it.summary, &it.details) } else { say },
                });
            }
            Action::Add | Action::Update => {
                let Some(mut section) = section.or(if c.name.trim().is_empty() { Some(Section::Profile) } else { None }) else {
                    out.dropped.push("a change with no section".into());
                    continue;
                };
                let name = one_line(c.name.trim().trim_matches(['"', '\'']), MAX_NAME);
                // A new person, project or topic with no name but facts
                // ("Plays Baldur's Gate 3" under topics): facts about the
                // person themselves.
                if c.action == Action::Add && !section.is_you() && name.is_empty() && !c.facts.is_empty() && by_id.is_none() {
                    section = Section::Profile;
                }
                // Facts written as one ("Nurse | Lives in Leeds"), and for
                // Profile and Preferences an "about", are facts each: the 4B
                // put "Nurse at St James's in Leeds" there (2026-10-22).
                let split = |f: &String| f.split(" | ").flat_map(|p| p.split("; ")).map(str::to_string).collect::<Vec<_>>();
                let mut facts: Vec<String> = c.facts.iter().flat_map(split).collect();
                let about_text = c.about.clone().filter(|a| !a.trim().is_empty());
                if section.is_you() {
                    // (A one-word "about" — "Name" — is a label, not a fact.)
                    if let Some(a) = about_text.as_ref().filter(|a| a.split_whitespace().count() > 1) {
                        facts.splice(0..0, split(a));
                    }
                }
                let about = about_text
                    .filter(|_| !section.is_you())
                    .map(|a| clean_fact(&one_line(&a, MAX_ABOUT * 2)))
                    .map(|a| one_line(&a, MAX_ABOUT));
                // The name and "about" must be keepable, or the change goes
                // (a topic called "Type 2 diabetes" is the topic itself; a
                // person's name is never a topic).
                if !section.is_you() {
                    let own = if section == Section::People { name.as_str() } else { "" };
                    if let Err(why) = kept(&name, own, rules.sensitive_ok) {
                        out.dropped.push(why);
                        continue;
                    }
                }
                if let Some(a) = &about {
                    if let Err(why) = kept(a, &name, rules.sensitive_ok) {
                        out.dropped.push(why);
                        continue;
                    }
                }
                // Who someone is, never their private life or health (a pass's).
                let about = about.filter(|a| {
                    let private = section == Section::People
                        && !rules.origin.by_the_person()
                        && (guard::private_in(a).is_some() || guard::sensitive_in(a, &name).is_some());
                    if private {
                        out.dropped.push("left out another person's private life".into());
                    }
                    !private
                });
                // A person is named by a name ("Dog", "Sister" or "Nurse at
                // St James's" aren't), from a pass.
                if section == Section::People && !name.is_empty() && !rules.origin.by_the_person() && !looks_like_a_name(&name) {
                    out.dropped.push("left out a person with no name".into());
                    continue;
                }
                // A name and an "about" the source never mentions: made up
                // (an update of an item Yap has keeps its name).
                if let Some(src) = rules.grounding.as_deref() {
                    let known = by_id.is_some() && c.action == Action::Update;
                    if !section.is_you() && !name.is_empty() && !known && !grounded(&name, src) {
                        out.dropped.push("left out someone the user didn't mention".into());
                        continue;
                    }
                }
                let about = about.filter(|a| {
                    let ok = rules.grounding.as_deref().is_none_or(|src| grounded(a, src));
                    if !ok {
                        out.dropped.push("left out a description the user didn't give".into());
                    }
                    ok
                });
                let existing = match by_id {
                    Some(at) if c.action == Action::Update => Some(at),
                    _ if section.is_you() => items.iter().position(|i| i.section == section),
                    _ => by_name(),
                };
                match existing {
                    Some(at) => {
                        let item = &mut items[at];
                        let before_summary = item.summary.clone();
                        let before_facts = item.details.clone();
                        let before_title = item.title.clone();
                        // A fuller name ("Sarah Jenkins" for "Sarah") or one
                        // the update gives.
                        if !section.is_you() && !name.is_empty() && norm(&name) != norm(&item.title) {
                            let fuller = norm(&name).starts_with(&format!("{} ", norm(&item.title)));
                            if fuller || (c.action == Action::Update && by_id.is_some()) {
                                item.title = name.clone();
                            }
                        }
                        if let Some(a) = about.filter(|a| !a.is_empty() && !section.is_you()) {
                            let (old, new) = (norm(&item.summary), norm(&a));
                            // An update says it anew; an add only fills a
                            // gap or says more of the same.
                            if c.action == Action::Update || old.is_empty() || (new.contains(&old) && new.len() > old.len()) {
                                item.summary = a;
                            }
                        }
                        let removed = remove_facts(item, &c.remove_facts);
                        let added = add_facts(item, &facts, rules, &mut out.dropped);
                        let changed = item.summary != before_summary || item.title != before_title || !added.is_empty() || !removed.is_empty();
                        if !changed {
                            continue;
                        }
                        item.updated_ts = rules.now;
                        if !item.source.by_the_person() || rules.origin.by_the_person() {
                            item.source = rules.origin.clone();
                        }
                        let say = if !say.is_empty() {
                            say
                        } else if added.is_empty() && !removed.is_empty() && item.summary == before_summary {
                            // Only taken out: what went.
                            one_line(&lower_first(&removed[0]), MAX_SAY)
                        } else {
                            say_for("updated", section, &item.title, &item.summary, if added.is_empty() { &item.details } else { &added })
                        };
                        out.done.push(Done {
                            change: index,
                            kind: "updated",
                            id: item.id,
                            section,
                            title: item.title.clone(),
                            summary_before: before_summary,
                            summary: item.summary.clone(),
                            facts_added: added,
                            facts_removed: removed,
                            facts_before: before_facts,
                            say,
                        });
                    }
                    None if c.action == Action::Update && !section.is_you() && name.is_empty() => {
                        out.dropped.push("an update of something Yap doesn't have".into());
                    }
                    None => {
                        if !section.is_you() && name.is_empty() {
                            out.dropped.push("an addition with no name".into());
                            continue;
                        }
                        // Room in the section: drop what was updated longest
                        // ago (never something the person wrote).
                        let count = items.iter().filter(|i| i.section == section).count();
                        if count >= section.max_items() {
                            let oldest = items
                                .iter()
                                .enumerate()
                                .filter(|(_, i)| i.section == section && !i.source.by_the_person())
                                .min_by_key(|(_, i)| i.updated_ts)
                                .map(|(at, _)| at);
                            match oldest {
                                Some(at) => {
                                    items.remove(at);
                                    out.dropped.push(format!("made room in {}", section.key()));
                                }
                                None => {
                                    out.dropped.push(format!("{} is full", section.key()));
                                    continue;
                                }
                            }
                        }
                        *next_id += 1;
                        let mut item = Item {
                            id: *next_id,
                            section,
                            title: if section.is_you() { section.label().to_string() } else { name.clone() },
                            summary: if section.is_you() { String::new() } else { about.unwrap_or_default() },
                            details: Vec::new(),
                            source: rules.origin.clone(),
                            created_ts: rules.now,
                            updated_ts: rules.now,
                            dictation: true,
                        };
                        let added = add_facts(&mut item, &facts, rules, &mut out.dropped);
                        if section.is_you() && added.is_empty() {
                            *next_id -= 1;
                            continue;
                        }
                        let say = if say.is_empty() { say_for("added", section, &item.title, &item.summary, &added) } else { say };
                        out.done.push(Done {
                            change: index,
                            kind: "added",
                            id: item.id,
                            section,
                            title: item.title.clone(),
                            summary_before: String::new(),
                            summary: item.summary.clone(),
                            facts_added: added,
                            facts_removed: Vec::new(),
                            facts_before: Vec::new(),
                            say,
                        });
                        items.push(item);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(kind: &str) -> Rules {
        Rules { sensitive_ok: false, origin: Origin::new(kind, Some(1), "Chat"), now: 100, max_changes: 5, grounding: None }
    }

    #[test]
    fn a_models_reply_is_read_leniently() {
        let reply = "Sure! Here you go:\n```json\n{\"changes\": [{\"action\": \"add\", \"section\": \"person\", \"name\": \"Rochelle\", \"about\": \"Sister\", \"facts\": [\"Lives in York\"], \"say\": \"your sister is Rochelle\"}, {\"op\": \"update\", \"id\": \"[3]\", \"add_facts\": \"Works nights\"}, {\"action\": \"forget\", \"item\": 7}, {\"action\": \"dance\"}]}\n```";
        let c = parse_changes(reply);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].action, c[0].section, c[0].name.as_str()), (Action::Add, Some(Section::People), "Rochelle"));
        assert_eq!(c[0].facts, ["Lives in York"]);
        assert_eq!((c[1].action, c[1].id), (Action::Update, Some(3)));
        assert_eq!(c[1].facts, ["Works nights"]);
        assert_eq!((c[2].action, c[2].id), (Action::Remove, Some(7)));
        assert!(parse_changes("{\"changes\": []}").is_empty());
        assert!(parse_changes("Nothing to remember.").is_empty());
        assert_eq!(parse_changes("[{\"action\":\"add\",\"section\":\"profile\",\"facts\":[\"Nurse\"]}]").len(), 1);
    }

    #[test]
    fn facts_are_kept_short_and_about_the_person() {
        assert_eq!(clean_fact("The user works night shifts."), "Works night shifts");
        assert_eq!(clean_fact("- the user's sister lives in York"), "Sister lives in York");
        assert_eq!(clean_fact("  lives   in Leeds "), "Lives in Leeds");
        assert!(clean_fact(&"word ".repeat(80)).chars().count() <= MAX_FACT);
    }

    fn add(section: Section, name: &str, about: &str, facts: &[&str]) -> Change {
        Change {
            action: Action::Add,
            id: None,
            section: Some(section),
            name: name.into(),
            about: (!about.is_empty()).then(|| about.into()),
            facts: facts.iter().map(|f| f.to_string()).collect(),
            remove_facts: vec![],
            say: String::new(),
        }
    }

    #[test]
    fn an_add_for_someone_known_updates_them() {
        let mut items = Vec::new();
        let mut next = 0;
        let o = apply(&mut items, &mut next, &[add(Section::People, "Rochelle", "Sister", &["Lives in York"])], &rules("chat"));
        assert_eq!(o.done[0].kind, "added");
        assert_eq!(o.done[0].say, "Rochelle, your sister");
        // Again, with her surname and one more fact: the same item.
        let o = apply(
            &mut items,
            &mut next,
            &[add(Section::People, "Rochelle Smith", "Sister", &["lives in York.", "Birthday 3 May"])],
            &rules("chat"),
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Rochelle Smith");
        assert_eq!(items[0].details, ["Lives in York", "Birthday 3 May"]);
        assert_eq!(o.done[0].kind, "updated");
        assert_eq!(o.done[0].facts_added, ["Birthday 3 May"]);
        // Nothing new: nothing done.
        let o = apply(&mut items, &mut next, &[add(Section::People, "Rochelle", "", &["Lives in York"])], &rules("chat"));
        assert!(o.done.is_empty());
        // Profile facts go into the one Profile item.
        apply(&mut items, &mut next, &[add(Section::Profile, "", "", &["Works as a nurse"])], &rules("chat"));
        apply(&mut items, &mut next, &[add(Section::Profile, "Nathan", "", &["Lives in Leeds"])], &rules("chat"));
        let profile: Vec<&Item> = items.iter().filter(|i| i.section == Section::Profile).collect();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].title, "Profile");
        assert_eq!(profile[0].details, ["Works as a nurse", "Lives in Leeds"]);
    }

    #[test]
    fn secrets_and_sensitive_topics_are_left_out() {
        let mut items = Vec::new();
        let mut next = 0;
        let o = apply(
            &mut items,
            &mut next,
            &[add(Section::Profile, "", "", &["Works as a nurse", "Wifi password is tulip99", "Has type 2 diabetes", "Lives at 12 Oak Street"])],
            &rules("chat"),
        );
        assert_eq!(items[0].details, ["Works as a nurse"]);
        assert_eq!(o.dropped.len(), 3);
        // A person whose "about" is a secret goes whole.
        apply(&mut items, &mut next, &[add(Section::People, "Dave", "Email dave@example.com", &[])], &rules("chat"));
        assert!(!items.iter().any(|i| i.title == "Dave"));
        // Sensitive topics: the person's own request may keep them.
        let mut asked = rules("tell");
        asked.sensitive_ok = true;
        apply(&mut items, &mut next, &[add(Section::Profile, "", "", &["Has type 2 diabetes"])], &asked);
        assert!(items[0].details.contains(&"Has type 2 diabetes".to_string()));
    }

    #[test]
    fn updates_and_removals_change_only_what_they_name() {
        let mut items = Vec::new();
        let mut next = 0;
        apply(&mut items, &mut next, &[add(Section::Profile, "", "", &["Works at Acme as a nurse", "Lives in Leeds"])], &rules("chat"));
        apply(&mut items, &mut next, &[add(Section::People, "Sam", "Colleague at Acme", &[])], &rules("chat"));
        let profile = items[0].id;
        let sam = items[1].id;
        let o = apply(
            &mut items,
            &mut next,
            &[
                Change {
                    action: Action::Update,
                    id: Some(profile),
                    section: None,
                    name: String::new(),
                    about: None,
                    facts: vec!["Works at St James's as a nurse".into()],
                    remove_facts: vec!["works at acme as a nurse".into()],
                    say: String::new(),
                },
                Change { action: Action::Remove, id: Some(sam), section: None, name: String::new(), about: None, facts: vec![], remove_facts: vec![], say: String::new() },
                Change { action: Action::Remove, id: Some(999), section: None, name: String::new(), about: None, facts: vec![], remove_facts: vec![], say: String::new() },
            ],
            &rules("tell"),
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].details, ["Lives in Leeds", "Works at St James's as a nurse"]);
        assert_eq!(o.done.len(), 2);
        assert_eq!(o.done[0].facts_removed, ["Works at Acme as a nurse"]);
        assert_eq!(o.done[1].kind, "removed");
        assert_eq!(o.done[1].say, "Sam");
        assert_eq!(o.dropped.len(), 1);

        // Who someone is, taken out as a fact (the 4B's "forget my old job
        // at Acme"): it goes, the rest stays.
        apply(&mut items, &mut next, &[add(Section::People, "Sam Patel", "Colleague at Acme", &["Product manager"])], &rules("chat"));
        let sam = items.iter().find(|i| i.title == "Sam Patel").unwrap().id;
        let take_out = Change { action: Action::Update, id: Some(sam), remove_facts: vec!["Colleague at Acme".into()], ..add(Section::People, "", "", &[]) };
        let o = apply(&mut items, &mut next, &[take_out], &rules("tell"));
        let sam = items.iter().find(|i| i.id == sam).unwrap();
        assert_eq!((sam.summary.as_str(), sam.details.clone()), ("", vec!["Product manager".to_string()]));
        assert_eq!((o.done[0].summary_before.as_str(), o.done[0].facts_removed.clone()), ("Colleague at Acme", vec!["Colleague at Acme".to_string()]));
    }

    #[test]
    fn memory_stays_bounded() {
        let mut items = Vec::new();
        let mut next = 0;
        let many: Vec<String> = (0..30).map(|i| format!("Fact number {i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        apply(&mut items, &mut next, &[add(Section::Profile, "", "", &refs)], &rules("chat"));
        assert_eq!(items[0].details.len(), Section::Profile.max_facts());
        assert_eq!(items[0].details.last().unwrap(), "Fact number 29");
        // A full section drops what was updated longest ago, never the person's own.
        let mut manual = rules("manual");
        manual.now = 1;
        apply(&mut items, &mut next, &[add(Section::Topics, "Gardening", "", &[])], &manual);
        for i in 0..40 {
            let mut r = rules("chat");
            r.now = 10 + i;
            apply(&mut items, &mut next, &[add(Section::Topics, &format!("Topic {i}"), "", &[])], &r);
        }
        let topics: Vec<&Item> = items.iter().filter(|i| i.section == Section::Topics).collect();
        assert_eq!(topics.len(), Section::Topics.max_items());
        assert!(topics.iter().any(|i| i.title == "Gardening"));
        assert!(!topics.iter().any(|i| i.title == "Topic 0"));
        // At most `max_changes` a batch.
        let batch: Vec<Change> = (0..9).map(|i| add(Section::Projects, &format!("Project {i}"), "", &[])).collect();
        let o = apply(&mut items, &mut next, &batch, &rules("chat"));
        assert_eq!(o.done.len(), 5);
    }

    #[test]
    fn preferences_filed_under_profile_move_to_preferences() {
        assert!(looks_like_preference("Prefers concise answers with British spelling"));
        assert!(looks_like_preference("Always use metric units"));
        assert!(looks_like_preference("Likes short replies"));
        assert!(!looks_like_preference("Lives in Leeds"));
        assert!(!looks_like_preference("Likes gardening"));
        assert!(!looks_like_preference("Works as a nurse"));
        assert!(looks_like_preference("Asked me to always reply in a friendly tone"));
        assert!(passing("Wanted a recipe for lasagne on 3 March"));
        assert!(passing("Asked about flights to Rome"));
        assert!(!passing("Works as a nurse"));
        assert!(!passing("Wants short answers"));
        let mut c = add(Section::Profile, "", "", &["Lives in Leeds", "Prefers concise answers"]);
        let out = route_preferences(vec![c.clone()], &[]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].facts, ["Lives in Leeds"]);
        assert_eq!((out[1].section, out[1].facts.clone()), (Some(Section::Preferences), vec!["Prefers concise answers".to_string()]));
        // An update of an item that isn't there, preferences only: one add.
        c = Change { action: Action::Update, id: Some(42), section: None, facts: vec!["Wants a friendly tone".into()], ..c };
        let out = route_preferences(vec![c], &[]);
        assert_eq!((out.len(), out[0].section), (1, Some(Section::Preferences)));
    }

    #[test]
    fn what_about_you_says_is_not_kept_twice() {
        let (name, note) = ("Declan", "I work day shifts at a warehouse in Leeds. I play PC games.");
        // Qwen3.5-4B's Profile from a chat that repeated the note.
        let c = add(Section::Profile, "", "", &["Works day shifts at a warehouse in Leeds", "Learning Spanish for a trip to Seville", "Name is Declan"]);
        let out = drop_about_you(vec![c], &[], name, note);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].facts, ["Learning Spanish for a trip to Seville"]);
        // More specific than the note: kept. A longer fact with the name: kept.
        let c = add(Section::Profile, "", "", &["Plays PC games on a Steam Deck", "Declan is saving up for a car next year"]);
        assert_eq!(drop_about_you(vec![c.clone()], &[], name, note)[0].facts.len(), 2);
        // Nothing left: the change goes. People, and removals, untouched.
        let only = add(Section::Profile, "", "", &["Plays PC games"]);
        assert!(drop_about_you(vec![only], &[], name, note).is_empty());
        let person = add(Section::People, "Sam Patel", "Colleague at the warehouse", &["Works day shifts at a warehouse in Leeds"]);
        assert_eq!(drop_about_you(vec![person.clone()], &[], name, note)[0].facts.len(), 1);
        let remove = Change { action: Action::Remove, id: Some(1), section: None, remove_facts: vec!["Works day shifts".into()], ..c.clone() };
        assert_eq!(drop_about_you(vec![remove], &[], name, note).len(), 1);
        // An update of the Profile item by id is checked like an add.
        let profile = Item {
            id: 7,
            section: Section::Profile,
            title: "Profile".into(),
            summary: String::new(),
            details: vec![],
            source: Origin::new("chat", None, ""),
            created_ts: 1,
            updated_ts: 1,
            dictation: true,
        };
        let update = Change { action: Action::Update, id: Some(7), section: None, facts: vec!["Works day shifts in Leeds".into()], ..c };
        assert!(drop_about_you(vec![update], &[profile], name, note).is_empty());
        // About you empty: everything as it was.
        assert_eq!(drop_about_you(vec![person], &[], "", "").len(), 1);
    }

    #[test]
    fn what_the_person_never_said_is_left_out() {
        let said = "I'm a nurse at St James's and I work nights. My sister Rochelle is visiting.";
        assert!(grounded("Works night shifts", said));
        assert!(grounded("Nurse at St James's", said));
        assert!(grounded("Sister", said));
        assert!(!grounded("Lives in Glasgow", said));
        assert!(!grounded("Works at the BBC", said));
        // The 1.5B's copy of the prompt's example: Tom, never mentioned.
        let mut items = Vec::new();
        let mut next = 0;
        let mut r = rules("chat");
        r.grounding = Some(said.to_string());
        let o = apply(
            &mut items,
            &mut next,
            &[add(Section::People, "Tom", "Brother", &["Lives in Glasgow"]), add(Section::People, "Rochelle", "Sister", &["Visiting", "Lives in Glasgow"])],
            &r,
        );
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].title.as_str(), items[0].summary.as_str()), ("Rochelle", "Sister"));
        assert_eq!(items[0].details, ["Visiting"]);
        assert!(o.dropped.iter().any(|d| d.contains("didn't mention")), "{:?}", o.dropped);
    }

    #[test]
    fn a_person_is_named_by_a_name() {
        for ok in ["Rochelle", "Sam Patel", "Dr Patel", "Ana Sofia Ruiz", "Mum"] {
            assert!(looks_like_a_name(ok), "{ok}");
        }
        for not in ["Nurse at St James's in Leeds", "Sister", "Dog", "User", "my colleague", "The guy from the gym who"] {
            assert!(!looks_like_a_name(not), "{not}");
        }
    }

    #[test]
    fn what_the_toast_says() {
        assert_eq!(say_for("added", Section::People, "Rochelle", "Sister", &[]), "Rochelle, your sister");
        assert_eq!(say_for("added", Section::People, "Sam", "Colleague at Acme, product manager", &[]), "Sam, your colleague at Acme, product manager");
        assert_eq!(say_for("added", Section::People, "Dr Patel", "GP at the Leeds practice", &[]), "Dr Patel, GP at the Leeds practice");
        assert_eq!(say_for("added", Section::Profile, "Profile", "", &["Works night shifts".into()]), "works night shifts");
        assert_eq!(say_for("added", Section::Projects, "Yap", "", &[]), "your project Yap");
        assert_eq!(say_for("removed", Section::People, "Sam", "Colleague", &[]), "Sam");
    }
}
