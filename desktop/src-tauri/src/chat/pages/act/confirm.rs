//! **The person's OK before an action that needs it**: a click (or Enter)
//! that would buy, pay, book, confirm, order, subscribe, send, delete or
//! sign in (`rules::needs_ok`), a link with such an address `go_to` would
//! open, and — after web content — typing words that aren't in what the
//! person said nor on the page. A card — "Yap wants to press ‘Book now’ on
//! rail.example.com" with **Allow once** and **Don't** ([`Card`],
//! `stream::Event::Confirm`) — shows in the Chat thread and in the page
//! tab's strip (the thread may be covered by the panel), and the tool waits
//! for the answer (`chat_page_confirm`), at most [`WAIT`]; no answer is a
//! no. Each OK is for that one action. The cards are kept with the answer
//! (`ChatMessage::confirms`), so a conversation opened again shows what was
//! asked and answered.
//!
//! Nobody watching the turn (no page to show the card on): refused at once.
//! A ■ Stop drops the waiting turn, and with it the question ([`Pending`]
//! takes itself out). A model evaluation answers as its scenario says
//! (`eval::confirm_answer`).
//!
//! **An agent's card** too (act "agent", `chat/tools/agents.rs`): before an agent
//! starts in a workspace new to Yap Code, or after the chat read the web —
//! "Send an agent to garden-planner?" with its folder, mode and brief ([`AgentCard`]),
//! Allow once / Don't, answered the same way.
//!
//! **A connector's card** (act "connector", `chat/tools/connectors.rs`): before
//! a connector's tool that asks first — "Use Notion: Create page?" with its
//! arguments as key: value lines, secrets masked ([`ConnectorCard`]) —
//! **Allow once · Always allow · Deny** (no Always allow for a tool that can
//! delete), answered with `chat_connector_confirm(id, answer)`; Always allow
//! is [`Answer::AlwaysAllowed`] (state "always").
//!
//! **Asked out loud** (the Voice Assistant, `chat/assistant/spoken_ok.rs`,
//! 8 Nov 2026 for every card): a page's, an agent's or a connector's question
//! is also said in one line, put on the Yap bar with Allow once / Don't, and
//! answered by a spoken yes or no (`chat_page_confirm`: Allow once, never
//! Always allow).
//!
//! The log has the question's id and the answer, never the button's words,
//! the typed text or the page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long an action waits for the person's answer.
pub const WAIT: Duration = Duration::from_secs(120);

static PENDING: Mutex<Option<HashMap<u64, tokio::sync::oneshot::Sender<Reply>>>> = Mutex::new(None);

/// What the person pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// Allow once (or a page's and an agent's Allow / Send).
    Once,
    /// Always allow (a connector's card only).
    Always,
    /// Don't / Deny.
    Deny,
}
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Answer {
    Allowed,
    /// Always allow (a connector's card): allowed, and from now on.
    AlwaysAllowed,
    Refused,
    /// No answer in [`WAIT`].
    TimedOut,
    /// Nobody to ask (no page watching the turn).
    NoOne,
}

impl Answer {
    /// The card's state as the page shows it.
    pub fn state(self) -> &'static str {
        match self {
            Answer::Allowed => "allowed",
            Answer::AlwaysAllowed => "always",
            Answer::Refused => "refused",
            Answer::TimedOut => "timedOut",
            Answer::NoOne => "refused",
        }
    }
}

/// **One question's card**, as the page shows it (`stream::Event::Confirm`)
/// and as the answer keeps it (`ChatMessage::confirms`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    pub id: u64,
    /// The button's, link's or field's words ("Book now", "Email").
    pub label: String,
    pub site: String,
    /// What it would do ("buy", "sign in", "send"…; "type": words from
    /// nowhere the person can see).
    pub does: String,
    /// "asking", then "allowed", "always" (a connector's Always allow),
    /// "refused" or "timedOut".
    pub state: String,
    /// What Yap would do: "press" (a button or a link; empty in an older
    /// card), "enter" (Enter in a field), "open" (go to a link's address),
    /// "type" (typed words: `text`), "drag" (what's dragged, onto `text`),
    /// "agent" (an agent starting; "job" in a
    /// card kept before the rename to Yap Code, read as "agent"),
    /// "connector" (a connector's tool: `connector`).
    #[serde(skip_serializing_if = "String::is_empty", deserialize_with = "crate::agents::renamed")]
    pub act: String,
    /// The words Yap would type (`act` "type"), or what a drag drops onto
    /// (`act` "drag").
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The page tab it's about (its key), so its strip asks too; never kept
    /// with the answer.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// An agent's card (act "agent"): what would start. An older Yap's save
    /// drops it (the card stays, with its folder as its label). (`job` in
    /// a card kept before the rename.)
    #[serde(skip_serializing_if = "Option::is_none", alias = "job")]
    pub agent: Option<AgentCard>,
    /// A connector's card (act "connector"): the tool and its arguments. An
    /// older Yap's save drops it (the card stays, with the tool as its label).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<Box<ConnectorCard>>,
}

/// **The connector's tool a card asks about** (act "connector"): "Use
/// `connector`: `tool`?", its arguments, and why Yap asks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectorCard {
    /// The person's name for the connector ("Notion").
    pub connector: String,
    /// The tool's title, else its name ("Create page").
    pub tool: String,
    /// Its arguments as the card shows them: secrets masked, long values cut.
    pub args: Vec<CardArg>,
    /// Always allow is on the card (not for a tool that can delete, nor one
    /// that's Always allow already).
    pub always: bool,
    /// Why Yap asks: "ask" (set to Ask first), "delete" (it can delete),
    /// "outside" (outside content is in the turn), "risky" (its name says it
    /// sends, pays, unlocks…), "meeting" (a meeting is on).
    pub why: String,
    /// "changes" | "deletes" | "reads".
    pub kind: String,
    /// Asked out loud (the Voice Assistant): Yap says one line and the Yap
    /// bar asks too (`assistant/connector_ask.rs`), beside this card.
    pub voice: bool,
    /// A spoken yes may answer it (asked out loud, a tool that can't
    /// delete, no meeting on); else a click only.
    pub spoken: bool,
}

/// One argument on a connector's card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CardArg {
    pub key: String,
    /// As shown: its first lines, "••••" for a secret.
    pub value: String,
    /// Cut to fit: the card says there's more.
    pub cut: bool,
    pub secret: bool,
}

/// **The agent a card asks about** (act "agent"): its folder, mode and brief.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentCard {
    /// The folder's name, and its path.
    pub folder: String,
    pub path: String,
    /// `auto` | `manual` | `acceptEdits` | `plan` | `bypassPermissions`, and
    /// as the chip says it ("Auto").
    pub mode: String,
    pub mode_label: String,
    /// What Claude Code would be asked.
    pub brief: String,
    /// Not a workspace yet: allowing it lets agents work there from now on.
    pub new_folder: bool,
    /// Asked because the chat read the web (a page could have slipped
    /// words into the brief).
    pub web: bool,
    /// A follow-up for the chat's agent (its next turn), not a new agent.
    pub continues: bool,
    /// **What this turn read from outside** ("the web", "a connector's
    /// result"; empty: nothing this turn): why the card asks, in one line —
    /// "This chat just read the web, which could have put words in the
    /// agent's brief." The one card Bypass permissions still shows.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub outside: String,
}

impl Card {
    /// The card as the answer keeps it: its tab left out, and a question
    /// still waiting when the turn ended (stopped) a no.
    pub fn kept(mut self) -> Card {
        self.key.clear();
        if self.state == "asking" {
            self.state = "refused".into();
        }
        self
    }
}

/// **What to ask**: the action ([`Card::act`]), its words, the site, what it
/// would do, the typed words (for "type") and the page tab.
#[derive(Debug, Clone, Default)]
pub struct Ask<'a> {
    pub act: &'a str,
    pub label: &'a str,
    pub site: &'a str,
    pub does: &'a str,
    pub text: &'a str,
    pub key: &'a str,
    /// An agent's card (act "agent").
    pub agent: Option<AgentCard>,
}

/// A question waiting: taken out of the waiting list when dropped (answered,
/// timed out, or the turn stopped).
struct Pending(u64);

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(map) = PENDING.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            map.remove(&self.0);
        }
    }
}

/// **The card's answer** (its Allow once / Don't, in the thread or the page
/// tab's strip): `true` if that question was still waiting.
#[tauri::command]
pub fn chat_page_confirm(id: u64, allow: bool) -> bool {
    answered(id, if allow { Reply::Once } else { Reply::Deny })
}

/// **A connector's card's answer** (`answer`: "once", "always" or
/// "deny"): `true` if that question was still waiting. ("always" on a card
/// without it is "once".)
#[tauri::command]
pub fn chat_connector_confirm(id: u64, answer: String) -> bool {
    let reply = match answer.as_str() {
        "always" => Reply::Always,
        "once" => Reply::Once,
        _ => Reply::Deny,
    };
    answered(id, reply)
}

/// Question `id` answered `reply`, if it was still waiting.
fn answered(id: u64, reply: Reply) -> bool {
    let tx = PENDING.lock().unwrap_or_else(|p| p.into_inner()).as_mut().and_then(|m| m.remove(&id));
    match tx {
        Some(tx) => {
            let pressed = match reply {
                Reply::Once => "once",
                Reply::Always => "always",
                Reply::Deny => "deny",
            };
            tracing::info!(id, pressed, "Chat: the user answered a question before an action");
            tx.send(reply).is_ok()
        }
        None => false,
    }
}

#[cfg(test)]
thread_local! {
    /// A unit test's answer, given at once (`None`: as in the app).
    static TEST_ANSWER: std::cell::Cell<Option<Reply>> = const { std::cell::Cell::new(None) };
}

/// A unit test answers every question this way from now on (on its thread).
#[cfg(test)]
pub fn answer_in_test(allow: Option<bool>) {
    reply_in_test(allow.map(|a| if a { Reply::Once } else { Reply::Deny }));
}

/// [`answer_in_test`] with any of the replies (Always allow too).
#[cfg(test)]
pub fn reply_in_test(reply: Option<Reply>) {
    TEST_ANSWER.with(|a| a.set(reply));
}

/// **Ask the person** (`what`), showing the card through `live`, and wait
/// for the answer.
pub async fn ask(live: Option<crate::chat::stream::Live<'_>>, what: Ask<'_>) -> Answer {
    ask_with(live, what, None).await
}

/// **Ask before a connector's tool** ([`ConnectorCard`]): [`ask`]'s card
/// with the tool and its arguments, and Always allow when `card.always`.
pub async fn ask_connector(live: Option<crate::chat::stream::Live<'_>>, what: Ask<'_>, card: ConnectorCard) -> Answer {
    ask_with(live, what, Some(card)).await
}

/// What a reply means on a card (`always`: it has Always allow).
fn answer_of(reply: Reply, always: bool) -> Answer {
    match reply {
        Reply::Always if always => Answer::AlwaysAllowed,
        Reply::Always | Reply::Once => Answer::Allowed,
        Reply::Deny => Answer::Refused,
    }
}

async fn ask_with(live: Option<crate::chat::stream::Live<'_>>, what: Ask<'_>, connector: Option<ConnectorCard>) -> Answer {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let always = connector.as_ref().is_some_and(|c| c.always);
    #[cfg(test)]
    if let Some(r) = TEST_ANSWER.with(|a| a.get()) {
        return answer_of(r, always);
    }
    if let Some(a) = crate::ai::eval::confirm_answer(what.label, what.does) {
        return if a { Answer::Allowed } else { Answer::Refused };
    }
    let Some(live) = live else {
        tracing::info!(id, "Chat: a page action needed the user's OK, but nobody watches the turn; refused");
        return Answer::NoOne;
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    PENDING.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(id, tx);
    let _pending = Pending(id);
    let card = |state: &str| {
        crate::chat::stream::Event::Confirm(Card {
            id,
            label: what.label.to_string(),
            site: what.site.to_string(),
            does: what.does.to_string(),
            state: state.to_string(),
            act: what.act.to_string(),
            text: what.text.to_string(),
            key: what.key.to_string(),
            agent: what.agent.clone(),
            connector: connector.clone().map(Box::new),
        })
    };
    live.sink.send(card("asking"));
    tracing::info!(id, "Chat: a page action waits for the user's OK");
    let answer = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(reply)) => answer_of(reply, always),
        Ok(Err(_)) => Answer::Refused,
        Err(_) => Answer::TimedOut,
    };
    live.sink.send(card(answer.state()));
    tracing::info!(id, outcome = answer.state(), "Chat: a page action's question answered");
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::stream::{Event, Live, Sink};

    #[derive(Default)]
    struct Log(Mutex<Vec<Event>>);
    impl Sink for Log {
        fn send(&self, ev: Event) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn book() -> Ask<'static> {
        Ask { act: "press", label: "Book now", site: "rail.example.com", does: "book", text: "", key: "page:https://rail.example.com", agent: None }
    }

    #[tokio::test]
    async fn the_card_waits_for_allow_or_dont_and_nobody_watching_is_a_no() {
        let log = Log::default();
        let live = Live { sink: &log, beside: None };
        // The card goes out; the page's Allow once answers it.
        let fut = ask(Some(live), book());
        let answering = async {
            // The question is waiting: find its id from the card sent.
            loop {
                let id = log.0.lock().unwrap().iter().find_map(|e| match e {
                    Event::Confirm(c) if c.state == "asking" => Some(c.id),
                    _ => None,
                });
                if let Some(id) = id {
                    assert!(chat_page_confirm(id, true));
                    // Answered once: a second answer finds nothing waiting.
                    assert!(!chat_page_confirm(id, false));
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (answer, ()) = tokio::join!(fut, answering);
        assert_eq!(answer, Answer::Allowed);
        let states: Vec<String> = log.0.lock().unwrap().iter().filter_map(|e| match e { Event::Confirm(c) => Some(format!("{}:{}:{}:{}:{}", c.state, c.act, c.label, c.site, c.key)), _ => None }).collect();
        assert_eq!(states, ["asking:press:Book now:rail.example.com:page:https://rail.example.com", "allowed:press:Book now:rail.example.com:page:https://rail.example.com"]);
        // The page hears it flat, as before: {kind: "confirm", id, label, …}.
        let sent = serde_json::to_value(log.0.lock().unwrap()[0].clone()).unwrap();
        assert_eq!((sent["kind"].as_str(), sent["label"].as_str(), sent["act"].as_str()), (Some("confirm"), Some("Book now"), Some("press")));
        // Nobody watching: no.
        assert_eq!(ask(None, Ask { label: "Pay now", does: "pay", ..book() }).await, Answer::NoOne);
        // A stopped turn takes its question out of the waiting list.
        let log2 = Log::default();
        let stopped = tokio::time::timeout(Duration::from_millis(20), ask(Some(Live { sink: &log2, beside: None }), Ask { label: "Delete", does: "delete", ..book() })).await;
        assert!(stopped.is_err());
        let id = log2.0.lock().unwrap().iter().find_map(|e| match e { Event::Confirm(c) => Some(c.id), _ => None }).unwrap();
        assert!(!chat_page_confirm(id, true), "nothing waits after the turn stopped");
    }

    /// A connector's card, answered `reply` from the page: the answer, and
    /// the card's states as the page heard them.
    async fn connector_card(always: bool, reply: &str) -> (Answer, Vec<String>) {
        let log = Log::default();
        let live = Live { sink: &log, beside: None };
        let card = ConnectorCard { connector: "Notion".into(), tool: "Create page".into(), always, why: "ask".into(), kind: "changes".into(), ..Default::default() };
        let fut = ask_connector(Some(live), Ask { act: "connector", label: "Create page", site: "Notion", does: "connector", ..Default::default() }, card);
        let answering = async {
            loop {
                let id = log.0.lock().unwrap().iter().find_map(|e| match e {
                    Event::Confirm(c) if c.state == "asking" => Some(c.id),
                    _ => None,
                });
                if let Some(id) = id {
                    assert!(chat_connector_confirm(id, reply.into()));
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (answer, ()) = tokio::join!(fut, answering);
        let states = log.0.lock().unwrap().iter().filter_map(|e| match e { Event::Confirm(c) => Some(c.state.clone()), _ => None }).collect();
        (answer, states)
    }

    #[tokio::test]
    async fn a_connectors_card_answers_allow_once_always_allow_or_deny() {
        assert_eq!(connector_card(true, "once").await, (Answer::Allowed, vec!["asking".into(), "allowed".into()]));
        assert_eq!(connector_card(true, "always").await, (Answer::AlwaysAllowed, vec!["asking".into(), "always".into()]));
        assert_eq!(connector_card(true, "deny").await, (Answer::Refused, vec!["asking".into(), "refused".into()]));
        // A card without Always allow (a tool that can delete): "always" is once.
        assert_eq!(connector_card(false, "always").await.0, Answer::Allowed);
        // The page hears the connector's part of the card.
        let c = Card { act: "connector".into(), connector: Some(Box::new(ConnectorCard { connector: "Notion".into(), tool: "Create page".into(), always: true, ..Default::default() })), ..Default::default() };
        let v = serde_json::to_value(Event::Confirm(c)).unwrap();
        assert_eq!((v["kind"].as_str(), v["connector"]["connector"].as_str(), v["connector"]["always"].as_bool()), (Some("confirm"), Some("Notion"), Some(true)));
        // A page's card never has one.
        assert!(serde_json::to_value(Card::default()).unwrap().get("connector").is_none());
    }

    #[test]
    fn a_kept_card_has_no_tab_and_a_question_left_waiting_is_a_no() {
        let c = Card { id: 3, label: "Book now".into(), state: "asking".into(), act: "press".into(), key: "page:https://rail.example.com".into(), ..Default::default() };
        let kept = c.kept();
        assert_eq!((kept.state.as_str(), kept.key.as_str()), ("refused", ""));
        let v = serde_json::to_value(&kept).unwrap();
        assert!(v.get("key").is_none() && v.get("text").is_none(), "{v}");
        // An older card (no act) reads back as a press.
        let old: Card = serde_json::from_str(r#"{"id":1,"label":"Buy tickets","site":"rail.example.com","does":"buy","state":"refused"}"#).unwrap();
        assert_eq!((old.act.as_str(), old.state.as_str()), ("", "refused"));
    }
}
