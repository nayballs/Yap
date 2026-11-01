//! **The person's OK before a click that buys, pays, books, confirms,
//! orders, subscribes, sends, deletes or signs in** (`rules::needs_ok`): a
//! card in the Chat thread — "Yap wants to press ‘Book now’ on
//! rail.example.com" with **Allow once** and **Don't** (`stream::Event::
//! Confirm`) — and the tool waits for the answer (`chat_page_confirm`), at
//! most [`WAIT`]; no answer is a no. Each OK is for that one click.
//!
//! Nobody watching the turn (no page to show the card on): refused at once.
//! A ■ Stop drops the waiting turn, and with it the question ([`Pending`]
//! takes itself out). A model evaluation answers as its scenario says
//! (`eval::confirm_answer`).
//!
//! The log has the question's id and the answer, never the button's words or
//! the page.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

/// How long a click waits for the person's answer.
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

/// **The card's answer** (its Allow once / Don't): `true` if that question
/// was still waiting.
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

/// **Ask the person** about `label` on `site` (`kind`: what it would do,
/// for the log), showing the card through `live`, and wait for the answer.
pub async fn ask(live: Option<crate::chat::stream::Live<'_>>, label: &str, site: &str, kind: &str) -> Answer {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    #[cfg(test)]
    if let Some(a) = TEST_ANSWER.with(|a| a.get()) {
        return if a { Answer::Allowed } else { Answer::Refused };
    }
    if let Some(a) = crate::ai::eval::confirm_answer(label, kind) {
        return if a { Answer::Allowed } else { Answer::Refused };
    }
    let Some(live) = live else {
        tracing::info!(id, "Chat: a page action needed the user's OK, but nobody watches the turn; refused");
        return Answer::NoOne;
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    PENDING.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(id, tx);
    let _pending = Pending(id);
    let card = |state: &str| crate::chat::stream::Event::Confirm { id, label: label.to_string(), site: site.to_string(), does: kind.to_string(), state: state.to_string() };
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

    #[tokio::test]
    async fn the_card_waits_for_allow_or_dont_and_nobody_watching_is_a_no() {
        let log = Log::default();
        let live = Live { sink: &log, beside: None };
        // The card goes out; the page's Allow once answers it.
        let fut = ask(Some(live), "Book now", "rail.example.com", "book");
        let answering = async {
            // The question is waiting: find its id from the card sent.
            loop {
                let id = log.0.lock().unwrap().iter().find_map(|e| match e {
                    Event::Confirm { id, state, .. } if state == "asking" => Some(*id),
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
        let states: Vec<String> = log.0.lock().unwrap().iter().filter_map(|e| match e { Event::Confirm { state, label, site, .. } => Some(format!("{state}:{label}:{site}")), _ => None }).collect();
        assert_eq!(states, ["asking:Book now:rail.example.com", "allowed:Book now:rail.example.com"]);
        // Nobody watching: no.
        assert_eq!(ask(None, "Pay now", "shop.example.com", "pay").await, Answer::NoOne);
        // A stopped turn takes its question out of the waiting list.
        let log2 = Log::default();
        let stopped = tokio::time::timeout(Duration::from_millis(20), ask(Some(Live { sink: &log2, beside: None }), "Delete", "x.example", "delete")).await;
        assert!(stopped.is_err());
        let id = log2.0.lock().unwrap().iter().find_map(|e| match e { Event::Confirm { id, .. } => Some(*id), _ => None }).unwrap();
        assert!(!chat_page_confirm(id, true), "nothing waits after the turn stopped");
    }
}
