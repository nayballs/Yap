//! **A Chat run's turn** (2026-11-07): a plan chat's Claude Code session
//! answers turn after turn (`chat/turn/plan.rs`), and Yap's tools that act
//! — Yap Code's (`chat/tools/agents.rs`, [`super::agents`]) — need to know whose turn
//! a call belongs to: the person's question this turn, the answer just
//! before it, the conversation, where it was asked, and whether web content
//! is in the turn. What they show (the card before an agent starts, its tab)
//! goes to that turn's stream.
//!
//! - **The plan turn** opens one with [`super::RunHandle::begin_turn`]
//!   before its question goes in ([`Context`]), again on each turn of a
//!   warm session, and ends it by dropping the [`Turn`]. A call outside a
//!   turn does nothing; one waiting on the person (a card) stops when its
//!   turn ends (the turn's stream closes: [`Call::events`]).
//! - **The web flag**: set from the turn's start (text selected in another
//!   app), once Claude Code begins a `WebSearch` or `WebFetch` in the turn
//!   ([`Marks::web`], from the plan turn's events), or once `agent_status`
//!   gave an agent's own words ([`Slot::set_web`]).
//! - **Two paths, one order**: Claude Code's events (its stdout, read by
//!   the plan turn) and its MCP calls (HTTP, read here) arrive apart. A call
//!   that acts (`start_agent`, `message_agent`) first waits for its own tool call
//!   to show in the turn's events ([`Slot::seen`], at most [`SEEN_WAIT`]):
//!   Claude Code writes a tool call's start (`content_block_start`, Yap runs
//!   it with `--include-partial-messages`) before it runs it, and its stdout
//!   is in order, so every web read in an earlier step — or earlier in the
//!   same step — has been counted by then. Not seen in time (a stalled
//!   stream, a call Claude Code never showed): it counts as after the web,
//!   and is refused — on doubt, never let through.
//!
//! The log has the turn's number and counts, never the question or a call's
//! arguments.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::chat::stream::Event;
use crate::chat::tools::AgentsTurn;

/// How long a call that acts waits for its own tool call to show in the
/// turn's events (a few milliseconds when all is well).
pub const SEEN_WAIT: Duration = Duration::from_secs(5);

/// **A turn's context**, as the plan turn gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Context {
    /// The person's message this turn.
    pub question: String,
    /// The answer just before it (a yes to its offer asks for an agent).
    pub previous: Option<String>,
    pub conversation: Option<u64>,
    /// Yap Code's tools this turn, and where it was asked (`None`: not offered).
    pub agents: Option<AgentsTurn>,
    /// Web content is in the turn from its start (text selected in another
    /// app, as the tool loop's `Extra::web_question`).
    pub web: bool,
}

/// The turn open now.
struct Now {
    n: u64,
    ctx: Arc<Context>,
    /// Its stream: what a call shows goes to the plan turn (closed when
    /// the turn ends).
    events: UnboundedSender<Event>,
    web: bool,
    /// Claude Code's calls to each of Yap's tools that this turn's events
    /// showed, and how many of them a call has taken ([`Slot::seen`]).
    began: HashMap<String, usize>,
    taken: HashMap<String, usize>,
}

#[derive(Default)]
struct State {
    /// How many turns this run has had (each one's number).
    turns: u64,
    now: Option<Now>,
}

/// **A run's turn**, shared by its handle (the plan turn) and the endpoint
/// (its calls).
#[derive(Default)]
pub struct Slot {
    state: Mutex<State>,
    changed: Condvar,
}

impl Slot {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Turn `n`'s state, if it's the one open now.
    fn with<R>(&self, n: u64, f: impl FnOnce(&mut Now) -> R) -> Option<R> {
        let mut s = self.lock();
        s.now.as_mut().filter(|t| t.n == n).map(f)
    }

    /// **A turn begins** (one still open ends): its context, its stream.
    pub(super) fn begin(self: &Arc<Self>, ctx: Context) -> Turn {
        let (tx, rx) = mpsc::unbounded_channel();
        let n = {
            let mut s = self.lock();
            s.turns += 1;
            let n = s.turns;
            s.now = Some(Now { n, web: ctx.web, ctx: Arc::new(ctx), events: tx, began: HashMap::new(), taken: HashMap::new() });
            n
        };
        self.changed.notify_all();
        tracing::debug!(turn = n, "Claude Code endpoint: a chat turn began");
        Turn { marks: Marks { slot: Arc::clone(self), n }, events: rx }
    }

    /// Turn `n` is over (a newer one may be open already: left alone).
    fn end(&self, n: u64) {
        {
            let mut s = self.lock();
            if s.now.as_ref().is_some_and(|t| t.n == n) {
                s.now = None;
            }
        }
        self.changed.notify_all();
    }

    /// **The turn a call belongs to**: the one open now (`None` between
    /// turns).
    pub fn call(&self) -> Option<Call> {
        self.lock().now.as_ref().map(|t| Call { n: t.n, ctx: Arc::clone(&t.ctx), events: t.events.clone() })
    }

    /// Is web content in turn `n`? A turn that's over: yes (on doubt).
    pub fn web(&self, n: u64) -> bool {
        self.with(n, |t| t.web).unwrap_or(true)
    }

    /// Web content came into turn `n` (an agent's own words).
    pub fn set_web(&self, n: u64) {
        self.with(n, |t| t.web = true);
    }

    /// **Wait for Claude Code's own call to `name`** to show in turn `n`'s
    /// events — one an earlier call hasn't taken — and take it: `true`.
    /// `false`: not within `wait`, or the turn ended.
    pub fn seen(&self, n: u64, name: &str, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        let mut s = self.lock();
        loop {
            let Some(t) = s.now.as_mut().filter(|t| t.n == n) else { return false };
            let began = t.began.get(name).copied().unwrap_or(0);
            let taken = t.taken.entry(name.to_string()).or_insert(0);
            if began > *taken {
                *taken += 1;
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                tracing::info!(turn = n, began, "Claude Code endpoint: a call didn't show in its turn's events in time");
                return false;
            }
            s = self.changed.wait_timeout(s, left).unwrap_or_else(|p| p.into_inner()).0;
        }
    }
}

/// **A call's turn**: its number, its context, its stream.
pub struct Call {
    pub n: u64,
    pub ctx: Arc<Context>,
    /// The turn's stream. Closed once the turn ends (`closed().await`).
    pub events: UnboundedSender<Event>,
}

/// **What the plan turn tells its run** as Claude Code's events come in.
#[derive(Clone)]
pub struct Marks {
    slot: Arc<Slot>,
    n: u64,
}

impl Marks {
    /// Claude Code began a `WebSearch` or `WebFetch`.
    pub fn web(&self) {
        self.slot.set_web(self.n);
        self.slot.changed.notify_all();
    }

    /// Claude Code began a call to Yap's tool `name` ("start_agent").
    pub fn began(&self, name: &str) {
        self.slot.with(self.n, |t| *t.began.entry(name.to_string()).or_insert(0) += 1);
        self.slot.changed.notify_all();
    }
}

/// **A turn open on a run**: its calls belong to it until it's dropped.
pub struct Turn {
    marks: Marks,
    /// What its calls show (a card, an agent's tab), for the turn's stream.
    pub events: UnboundedReceiver<Event>,
}

impl Turn {
    /// What to tell the run as the turn's events come in.
    pub fn marks(&self) -> Marks {
        self.marks.clone()
    }

    /// Its number on the run.
    #[cfg(test)]
    pub fn number(&self) -> u64 {
        self.marks.n
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.marks.slot.end(self.marks.n);
    }
}

/// A turn on a slot of a test's own (no run registered).
#[cfg(test)]
pub fn test_begin(slot: &Arc<Slot>, ctx: Context) -> Turn {
    slot.begin(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(q: &str) -> Context {
        Context { question: q.into(), conversation: Some(3), agents: Some(AgentsTurn::default()), ..Default::default() }
    }

    #[test]
    fn a_call_belongs_to_the_turn_open_now_and_none_between_turns() {
        let slot = Arc::new(Slot::default());
        assert!(slot.call().is_none(), "no turn yet");
        let first = slot.begin(ctx("Start an agent in garden-planner"));
        let call = slot.call().unwrap();
        assert_eq!((call.n, call.ctx.question.as_str(), call.ctx.conversation), (1, "Start an agent in garden-planner", Some(3)));
        assert!(!slot.web(1));
        // What a call shows reaches the turn's stream; once it ends, it's closed.
        let mut first = first;
        call.events.send(Event::Waiting { waiting: true }).unwrap();
        assert_eq!(first.events.try_recv().ok(), Some(Event::Waiting { waiting: true }));
        drop(first);
        assert!(call.events.is_closed());
        assert!(slot.call().is_none(), "between turns");
        assert!(slot.web(1), "a turn that's over: web, on doubt");
        // The next turn of a warm session: its own context and number.
        let second = slot.begin(Context { web: true, ..ctx("Look it up") });
        assert_eq!(second.number(), 2);
        assert!(slot.web(2), "web from its start (text selected in another app)");
        // An older turn's end leaves the newer one open.
        slot.end(1);
        assert_eq!(slot.call().map(|c| c.n), Some(2));
    }

    #[test]
    fn web_reads_and_an_agents_words_mark_the_turn() {
        let slot = Arc::new(Slot::default());
        let turn = slot.begin(ctx("q"));
        assert!(!slot.web(turn.number()));
        turn.marks().web();
        assert!(slot.web(turn.number()));
        let turn2 = slot.begin(ctx("q2"));
        assert!(!slot.web(turn2.number()), "each turn starts clean");
        slot.set_web(turn2.number());
        assert!(slot.web(turn2.number()));
        // A mark for a turn that's over changes nothing now.
        turn.marks().web();
        turn.marks().began("start_agent");
        assert!(!slot.seen(turn2.number(), "start_agent", Duration::from_millis(1)));
    }

    #[test]
    fn a_call_that_acts_waits_for_its_own_tool_call_in_the_turns_events() {
        let slot = Arc::new(Slot::default());
        let turn = slot.begin(ctx("q"));
        let n = turn.number();
        // Not shown (yet): not in time.
        assert!(!slot.seen(n, "start_agent", Duration::from_millis(20)));
        // Shown first: taken at once, each call once.
        turn.marks().began("start_agent");
        assert!(slot.seen(n, "start_agent", Duration::ZERO));
        assert!(!slot.seen(n, "start_agent", Duration::from_millis(5)), "one call, one tool call");
        assert!(!slot.seen(n, "message_agent", Duration::from_millis(5)), "by name");
        // Shown while the call waits (the events' path is a little behind):
        // the web read before it is counted by then.
        let waiting = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || {
                let seen = slot.seen(n, "start_agent", Duration::from_secs(10));
                (seen, slot.web(n))
            })
        };
        std::thread::sleep(Duration::from_millis(30));
        let marks = turn.marks();
        marks.web();
        marks.began("start_agent");
        assert_eq!(waiting.join().unwrap(), (true, true));
        // The turn ends while a call waits: it stops waiting.
        let waiting = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || slot.seen(n, "message_agent", Duration::from_secs(10)))
        };
        std::thread::sleep(Duration::from_millis(30));
        let started = Instant::now();
        drop(turn);
        assert!(!waiting.join().unwrap());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
