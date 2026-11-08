//! **Whether a connector's tool runs, asks first or is refused** (pure):
//! one decision per call, from the tool's setting and kind, whether outside
//! content is in the turn, whose words its arguments are, what its name
//! says it does, who's there to ask and whether a meeting is on.
//!
//! | The tool's setting | Before outside content | After outside content |
//! |---|---|---|
//! | Off | refused (never offered) | the same |
//! | Always allow, Reads | runs | runs if its words are the person's own, else asks |
//! | Always allow, Changes things | runs; risky words ask | asks |
//! | Ask first | asks | asks |
//! | Can delete (unless Off) | asks; no Always allow | the same |
//! | Nobody to ask (no page watching the turn) | what would run runs; what would ask is refused | the same |
//! | During a meeting (connectors allowed then) | reads as set; every write asks | the same |
//!
//! **Outside content** is anything in the turn the person didn't write: a
//! connector's result, a web page, the screen, an agent's words. After it,
//! a write a server's text could have asked for always waits for the
//! person, Always allow or not (the connector's own write tools need the
//! card), and a read whose arguments aren't the person's own words asks too
//! (a result's words carried into another call: exfiltration). **Own
//! words** ([`own_words`]): every string argument, and every number of
//! three digits or more, is in what the person said in this conversation
//! (case, spaces and punctuation aside); a value from the schema's `enum`
//! counts as theirs; booleans and small numbers are free. **Risky words**
//! ([`risky`]): the tool's name or title says it sends, pays, buys, books,
//! deletes, signs in… in Yap's seven languages (`chat/pages/act/words.rs`,
//! the page rules' own tables), or does what a home's locks and alarms do
//! (unlock, disarm, a door, a gate, a valve). Annotations only set a tool's
//! kind and its default; they never loosen any of this.
//!
//! **Always allow** is offered on a card only for a tool that can't delete
//! and isn't Always allow already (a card after outside content for one
//! that is: Allow once or Deny). It sets the tool to Always allow, pinned
//! to its definition's hash (a changed definition asks again: `cache.rs`).

use serde_json::Value;

use super::cache::Kind;
use super::store::Setting;

/// What a call is decided on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    pub setting: Setting,
    pub kind: Kind,
    /// Outside content is in the turn.
    pub outside: bool,
    /// Every word of the call's arguments is the person's own ([`own_words`]).
    pub own_words: bool,
    /// The tool's name says it sends, pays, deletes, unlocks… ([`risky`]).
    pub risky: bool,
    /// Someone watches the turn: a card can be shown.
    pub watching: bool,
    /// A meeting records or a call is on (connectors were allowed then).
    pub meeting: bool,
}

/// Why a call asks first (the card's line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Set to Ask first.
    AskFirst,
    /// It can delete (it always asks).
    CanDelete,
    /// Outside content is in the turn: a write, or a read with words that
    /// aren't the person's.
    Outside,
    /// Its name says it sends, pays, deletes, unlocks…
    Risky,
    /// A meeting is on: every write asks.
    Meeting,
}

/// Why a call doesn't run at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The tool is Off.
    Off,
    /// It would ask, and nobody watches the turn.
    NobodyToAsk,
}

/// The decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Run,
    /// The card, with Always allow on it or not.
    Ask { why: Why, always: bool },
    Refuse(Refusal),
}

/// **The decision for one call** ([module docs](self)). Pure.
pub fn decide(i: Input) -> Decision {
    if i.setting == Setting::Off {
        return Decision::Refuse(Refusal::Off);
    }
    let deletes = i.kind == Kind::Deletes;
    let ask = |why: Why| {
        if i.watching {
            Decision::Ask { why, always: !deletes && i.setting != Setting::Allow }
        } else {
            Decision::Refuse(Refusal::NobodyToAsk)
        }
    };
    if deletes {
        return ask(Why::CanDelete);
    }
    if i.setting == Setting::Ask {
        return ask(Why::AskFirst);
    }
    match i.kind {
        Kind::Reads if i.outside && !i.own_words => ask(Why::Outside),
        Kind::Reads => Decision::Run,
        _ if i.meeting => ask(Why::Meeting),
        _ if i.outside => ask(Why::Outside),
        _ if i.risky => ask(Why::Risky),
        _ => Decision::Run,
    }
}

/// Text as the own-words check compares it: lower case, commas inside
/// numbers dropped ("1,200" = "1200"), everything but letters and digits a
/// single space, with a space at each end (so a value matches whole
/// words: "it" isn't in "kitchen"). Pure.
fn norm(s: &str) -> String {
    let mut out = String::from(" ");
    for c in s.chars().filter(|c| *c != ',').flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// **Every word of `args` is the person's own** (`said`: what they wrote in
/// this conversation, this message first): each string (unless it's one of
/// its schema's `enum`) and each number of three digits or more found
/// there as whole words; booleans, nulls, small numbers and empty strings
/// are free. Objects and arrays by their parts (and their parts' schemas).
/// Pure.
pub fn own_words(args: &Value, schema: &Value, said: &[&str]) -> bool {
    let said: String = said.iter().map(|s| norm(s)).collect();
    fn walk(v: &Value, schema: &Value, said: &str) -> bool {
        match v {
            Value::String(s) => {
                let in_enum = schema["enum"].as_array().is_some_and(|e| e.iter().any(|x| x.as_str() == Some(s.as_str())));
                let n = norm(s);
                in_enum || n.trim().is_empty() || said.contains(&n)
            }
            Value::Number(n) => {
                let digits = n.to_string().chars().filter(char::is_ascii_digit).count();
                digits < 3 || said.contains(&norm(&n.to_string()))
            }
            Value::Array(items) => items.iter().all(|x| walk(x, &schema["items"], said)),
            Value::Object(map) => map.iter().all(|(k, x)| walk(x, &schema["properties"][k], said)),
            Value::Bool(_) | Value::Null => true,
        }
    }
    walk(args, schema, &said)
}

/// What a home's devices do that asks first: open a lock, a door or a gate,
/// turn an alarm off, a valve.
const HOME: &[&str] = &["unlock", "disarm", "alarm", "valve", "garage", "gate", "door"];

/// **The tool's name or title says it does something that asks first**:
/// sends, pays, buys, books, orders, confirms, subscribes, deletes, agrees
/// or signs in (the page rules' words, in Yap's seven languages), or opens a
/// home's locks and alarms. Pure.
pub fn risky(name: &str, title: Option<&str>) -> bool {
    let said = format!("{name} {}", title.unwrap_or(""));
    if crate::chat::pages::act::rules::needs_ok(&said, "").is_some() {
        return true;
    }
    let words = crate::chat::pages::act::rules::words(&said);
    words.iter().any(|w| HOME.contains(&w.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(setting: Setting, kind: Kind) -> Input {
        Input { setting, kind, outside: false, own_words: true, risky: false, watching: true, meeting: false }
    }

    const ASK_FIRST: Decision = Decision::Ask { why: Why::AskFirst, always: true };

    #[test]
    fn every_setting_and_kind_before_and_after_outside_content() {
        use Kind::*;
        use Setting::*;
        // Off: never, whatever else.
        for kind in [Reads, Changes, Deletes] {
            for outside in [false, true] {
                for watching in [false, true] {
                    let i = Input { outside, watching, ..input(Off, kind) };
                    assert_eq!(decide(i), Decision::Refuse(Refusal::Off), "{i:?}");
                }
            }
        }
        // Always allow, Reads: runs; after outside content, only with the person's words.
        assert_eq!(decide(input(Allow, Reads)), Decision::Run);
        assert_eq!(decide(Input { outside: true, ..input(Allow, Reads) }), Decision::Run);
        assert_eq!(decide(Input { outside: true, own_words: false, ..input(Allow, Reads) }), Decision::Ask { why: Why::Outside, always: false });
        // (Before outside content, a read's words needn't be the person's.)
        assert_eq!(decide(Input { own_words: false, ..input(Allow, Reads) }), Decision::Run);
        // Always allow, Changes: runs; risky words ask; after outside content it always asks.
        assert_eq!(decide(input(Allow, Changes)), Decision::Run);
        assert_eq!(decide(Input { risky: true, ..input(Allow, Changes) }), Decision::Ask { why: Why::Risky, always: false });
        assert_eq!(decide(Input { outside: true, ..input(Allow, Changes) }), Decision::Ask { why: Why::Outside, always: false });
        assert_eq!(decide(Input { outside: true, own_words: true, risky: false, ..input(Allow, Changes) }), Decision::Ask { why: Why::Outside, always: false });
        // Ask first: asks, with Always allow on the card, before and after.
        for kind in [Reads, Changes] {
            assert_eq!(decide(input(Ask, kind)), ASK_FIRST);
            assert_eq!(decide(Input { outside: true, own_words: false, ..input(Ask, kind) }), ASK_FIRST);
        }
        // Can delete: asks, never with Always allow, however it's set.
        for setting in [Allow, Ask] {
            for outside in [false, true] {
                let i = Input { outside, ..input(setting, Deletes) };
                assert_eq!(decide(i), Decision::Ask { why: Why::CanDelete, always: false }, "{i:?}");
            }
        }
    }

    #[test]
    fn with_nobody_to_ask_what_would_ask_is_refused_and_reads_still_run() {
        use Kind::*;
        use Setting::*;
        let away = |i: Input| Input { watching: false, ..i };
        assert_eq!(decide(away(input(Allow, Reads))), Decision::Run);
        assert_eq!(decide(away(input(Allow, Changes))), Decision::Run);
        for i in [input(Ask, Reads), input(Ask, Changes), input(Allow, Deletes), Input { outside: true, ..input(Allow, Changes) }, Input { risky: true, ..input(Allow, Changes) }] {
            assert_eq!(decide(away(i)), Decision::Refuse(Refusal::NobodyToAsk), "{i:?}");
        }
    }

    #[test]
    fn during_a_meeting_reads_go_as_set_and_every_write_asks() {
        use Kind::*;
        use Setting::*;
        let meeting = |i: Input| Input { meeting: true, ..i };
        assert_eq!(decide(meeting(input(Allow, Reads))), Decision::Run);
        assert_eq!(decide(meeting(input(Allow, Changes))), Decision::Ask { why: Why::Meeting, always: false });
        assert_eq!(decide(meeting(input(Ask, Changes))), ASK_FIRST);
        assert_eq!(decide(meeting(input(Allow, Deletes))), Decision::Ask { why: Why::CanDelete, always: false });
    }

    #[test]
    fn own_words_are_the_persons_whole_words_enums_and_small_numbers_free() {
        let said = ["Turn off the kitchen lights", "and make a page called Launch notes, budget 1,200"];
        let schema = json!({ "type": "object", "properties": { "mode": { "type": "string", "enum": ["fast", "slow"] }, "tags": { "type": "array", "items": { "type": "string" } } } });
        assert!(own_words(&json!({ "area": "Kitchen" }), &schema, &said));
        assert!(own_words(&json!({ "title": "launch notes", "budget": 1200, "count": 12, "draft": true, "mode": "slow" }), &schema, &said));
        // Words from nowhere the person said (a result's): not theirs.
        assert!(!own_words(&json!({ "to": "someone@example.com" }), &schema, &said));
        assert!(!own_words(&json!({ "area": "hall" }), &schema, &said));
        // Whole words only: "it" isn't in "kitchen"; a big number must be said.
        assert!(!own_words(&json!({ "q": "it" }), &schema, &said));
        assert!(!own_words(&json!({ "id": 4821 }), &schema, &said));
        // Nested, by their parts.
        assert!(own_words(&json!({ "filter": { "room": "kitchen" }, "tags": ["launch", "notes"] }), &schema, &said));
        assert!(!own_words(&json!({ "tags": ["launch", "secret"] }), &schema, &said));
        // Nothing to say: free.
        assert!(own_words(&json!({}), &schema, &said));
        assert!(own_words(&json!({ "note": "  " }), &schema, &said));
    }

    #[test]
    fn risky_names_say_send_pay_delete_or_unlock_in_any_of_yaps_languages() {
        for (name, title) in [("send_message", None), ("create_payment", None), ("HassUnlock", None), ("lock_door", None), ("x", Some("Jetzt kaufen")), ("post_comment", None), ("disarm_alarm", None)] {
            assert!(risky(name, title), "{name} {title:?}");
        }
        for (name, title) in [("create_page", Some("Create page")), ("turn_off", None), ("HassTurnOn", None), ("search", None), ("list_lights", Some("List lights"))] {
            assert!(!risky(name, title), "{name} {title:?}");
        }
    }
}
