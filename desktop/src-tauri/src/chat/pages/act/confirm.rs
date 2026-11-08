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
//! The log has the question's id and the answer, never the button's words,
//! the typed text or the page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long an action waits for the person's answer.
pub const WAIT: Duration = Duration::from_secs(120);

static PENDING: Mutex<Option<HashMap<u64, tokio::sync::oneshot::Sender<bool>>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Answer {
    Allowed,
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
    /// "asking", then "allowed", "refused" or "timedOut".
    pub state: String,
    /// What Yap would do: "press" (a button or a link; empty in an older
    /// card), "enter" (Enter in a field), "open" (go to a link's address),
    /// "type" (typed words: `text`), "agent" (an agent starting; "job" in a
    /// card kept before the rename to Yap Code, read as "agent").
    #[serde(skip_serializing_if = "String::is_empty", deserialize_with = "crate::agents::renamed")]
    pub act: String,
    /// The words Yap would type (`act` "type").
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
    let tx = PENDING.lock().unwrap_or_else(|p| p.into_inner()).as_mut().and_then(|m| m.remove(&id));
    match tx {
        Some(tx) => {
            tracing::info!(id, allow, "Chat: the user answered a page action's question");
            tx.send(allow).is_ok()
        }
        None => false,
    }
}

#[cfg(test)]
thread_local! {
    /// A unit test's answer, given at once (`None`: as in the app).
    static TEST_ANSWER: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// A unit test answers every question this way from now on (on its thread).
#[cfg(test)]
pub fn answer_in_test(allow: Option<bool>) {
    TEST_ANSWER.with(|a| a.set(allow));
}

/// **Ask the person** (`what`), showing the card through `live`, and wait
/// for the answer.
pub async fn ask(live: Option<crate::chat::stream::Live<'_>>, what: Ask<'_>) -> Answer {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    #[cfg(test)]
    if let Some(a) = TEST_ANSWER.with(|a| a.get()) {
        return if a { Answer::Allowed } else { Answer::Refused };
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
        })
    };
    live.sink.send(card("asking"));
    tracing::info!(id, "Chat: a page action waits for the user's OK");
    let answer = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(true)) => Answer::Allowed,
        Ok(Ok(false)) | Ok(Err(_)) => Answer::Refused,
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
