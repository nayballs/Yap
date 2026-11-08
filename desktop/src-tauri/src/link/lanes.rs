//! **A phone's way out, in two lanes** (Remote Control, 2026-11-08, design
//! §6.4). Until then every message went on one unbounded queue to the
//! socket's writer; an agent can write thousands of events, and a phone
//! that falls behind would have grown it without end.
//!
//! - **The control lane** ([`Tx::send`], as before, never dropped): `ready`,
//!   a question's flow and its voice, **cards and their ends**, errors. It
//!   always goes first. It's **capped** at [`CONTROL_CAP`] bytes waiting:
//!   past that the phone isn't reading, and the connection is closed
//!   ([`Rx::recv`] says [`Wire::Close`]) — the phone connects again and gets
//!   a fresh `ready`, `code_status` and every card still waiting, so no card
//!   is ever lost.
//! - **The agent lane** ([`AgentLane`], pure): the watched agent's events
//!   only, bounded ([`LANE_MESSAGES`], [`LANE_BYTES`]). State-like events
//!   (only `status`, `now`, `todos`, `cost`, `problem`) are **coalesced**,
//!   the latest wins, flushed every [`FLUSH`]; the rest (a step, its words, a
//!   file, a diff, a command) are queued in order. **Full means resync, not
//!   growth**: the lane is emptied and the phone gets `agent_resync {id}`,
//!   then asks `agent_open` again (a snapshot, then live). Each message on
//!   it is numbered (`seq`, from 1 after the snapshot's 0), so a gap is seen.
//!   Events the snapshot already had (their log line at most its last) are
//!   left out.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::link::wire::Out;

/// What a session's writer sends (`session.rs`).
#[derive(Debug)]
pub enum Wire {
    Text(String),
    /// A down frame of the voice of turn `turn` (dropped if that turn was
    /// interrupted meanwhile).
    Audio { turn: u64, bytes: Vec<u8> },
    /// Close the socket.
    Close,
}

impl Wire {
    fn size(&self) -> usize {
        match self {
            Wire::Text(t) => t.len(),
            Wire::Audio { bytes, .. } => bytes.len(),
            Wire::Close => 0,
        }
    }
}

/// The most the control lane holds before the phone is cut off (8 MiB: far
/// more than a sentence of voice ahead and every card).
pub const CONTROL_CAP: usize = 8 * 1024 * 1024;
/// The agent lane's bounds.
pub const LANE_MESSAGES: usize = 256;
pub const LANE_BYTES: usize = 1024 * 1024;
/// How long a state-like event waits for the next one to replace it.
pub const FLUSH: Duration = Duration::from_millis(250);

/// The fields an event changes in place: the latest wins.
const STATE: &[&str] = &["status", "now", "todos", "cost", "problem"];
/// The fields every event carries.
const ALWAYS: &[&str] = &["id", "status", "folder", "conversation", "turn", "ts"];

/// Only the agent's state changed (nothing to append): a step, its words, a
/// file, a diff or a command make it an event to keep. Pure.
pub fn state_only(event: &Value) -> bool {
    event.as_object().is_some_and(|o| o.keys().all(|k| ALWAYS.contains(&k.as_str()) || STATE.contains(&k.as_str())))
}

/// The agent a phone is watching, and the last log line its snapshot had.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Watch {
    id: String,
    from_line: u64,
    /// Emptied for a resync: nothing more until it's opened again.
    resyncing: bool,
}

/// **The agent lane**: the watched agent's events, coalesced and bounded.
/// Pure (the clock is passed in).
#[derive(Debug, Default)]
pub struct AgentLane {
    watch: Option<Watch>,
    queue: VecDeque<String>,
    bytes: usize,
    /// The state-like event waiting (merged), its log line, since when.
    pending: Option<(Value, Option<u64>, Instant)>,
    seq: u64,
}

impl AgentLane {
    /// **Watch agent `id` from now on** (its snapshot had log lines up to
    /// `from_line`): what was waiting for another goes.
    pub fn watch(&mut self, id: &str, from_line: u64) {
        self.clear();
        self.watch = Some(Watch { id: id.to_string(), from_line, resyncing: false });
        self.seq = 0;
    }

    /// No agent watched (`agent_close`).
    pub fn unwatch(&mut self) {
        self.clear();
        self.watch = None;
    }

    /// The agent watched, if any.
    pub fn watching(&self) -> Option<&str> {
        self.watch.as_ref().map(|w| w.id.as_str())
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.bytes = 0;
        self.pending = None;
    }

    fn push_text(&mut self, text: String) {
        self.bytes += text.len();
        self.queue.push_back(text);
    }

    fn numbered(&mut self, event: Value, line: Option<u64>) -> String {
        self.seq += 1;
        let id = self.watch.as_ref().map(|w| w.id.clone()).unwrap_or_default();
        Out::AgentEvent { id, seq: self.seq, line, event }.json()
    }

    /// The state-like event waiting, into the queue.
    pub fn flush(&mut self) {
        if let Some((event, line, _)) = self.pending.take() {
            let text = self.numbered(event, line);
            self.push_text(text);
            self.check();
        }
    }

    /// Full: emptied, and the phone told to open the agent again.
    fn check(&mut self) {
        if self.queue.len() <= LANE_MESSAGES && self.bytes <= LANE_BYTES {
            return;
        }
        self.clear();
        if let Some(w) = self.watch.as_mut() {
            w.resyncing = true;
            let id = w.id.clone();
            self.push_text(Out::AgentResync { id }.json());
        }
    }

    /// **One `yap-agent` event** of agent `id` (`line`: its log line): for
    /// the agent watched only, not one the snapshot had; coalesced when only
    /// its state changed.
    pub fn event(&mut self, id: &str, line: Option<u64>, event: Value, now: Instant) {
        let Some(w) = self.watch.as_ref() else { return };
        if w.id != id || w.resyncing || line.is_some_and(|l| l <= w.from_line) {
            return;
        }
        if state_only(&event) {
            match self.pending.as_mut() {
                Some((merged, at, _)) => {
                    if let (Some(m), Some(o)) = (merged.as_object_mut(), event.as_object()) {
                        for (k, v) in o {
                            m.insert(k.clone(), v.clone());
                        }
                    }
                    *at = line.or(*at);
                }
                None => self.pending = Some((event, line, now)),
            }
            return;
        }
        // A step (or anything to keep): the state before it goes first, in order.
        self.flush();
        if self.watch.as_ref().is_some_and(|w| w.resyncing) {
            return;
        }
        let text = self.numbered(event, line);
        self.push_text(text);
        self.check();
    }

    /// A page the phone asked for (a snapshot, older events, a diff): queued
    /// as it is.
    pub fn page(&mut self, text: String) {
        self.push_text(text);
        self.check();
    }

    /// The next message to send, if any.
    pub fn pop(&mut self) -> Option<String> {
        let t = self.queue.pop_front()?;
        self.bytes = self.bytes.saturating_sub(t.len());
        Some(t)
    }

    /// How long until the state waiting is due (`None`: nothing waits).
    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        self.pending.as_ref().map(|(_, _, since)| FLUSH.saturating_sub(now.saturating_duration_since(*since)))
    }

    /// How many messages wait.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.queue.len() + usize::from(self.pending.is_some())
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The agent lane, shared by the session and its writer.
#[derive(Debug, Default)]
pub struct Lane {
    state: Mutex<AgentLane>,
    wake: tokio::sync::Notify,
}

impl Lane {
    /// Change the lane (`f`), and wake the writer.
    pub fn with<R>(&self, f: impl FnOnce(&mut AgentLane) -> R) -> R {
        let out = f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()));
        self.wake.notify_one();
        out
    }

    /// Is agent `id` the one watched?
    pub fn watches(&self, id: &str) -> bool {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).watching() == Some(id)
    }
}

/// **The session's way out**, from any thread: the control lane
/// ([`Tx::send`]) and the agent lane ([`Tx::lane`]).
#[derive(Debug, Clone)]
pub struct Tx {
    inner: tokio::sync::mpsc::UnboundedSender<Wire>,
    queued: Arc<AtomicUsize>,
    over: Arc<AtomicBool>,
    lane: Arc<Lane>,
}

/// The writer's end.
pub struct Rx {
    inner: tokio::sync::mpsc::UnboundedReceiver<Wire>,
    queued: Arc<AtomicUsize>,
    over: Arc<AtomicBool>,
    lane: Arc<Lane>,
}

/// A session's two lanes.
pub fn channel() -> (Tx, Rx) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let queued = Arc::new(AtomicUsize::new(0));
    let over = Arc::new(AtomicBool::new(false));
    let lane = Arc::new(Lane::default());
    (
        Tx { inner: tx, queued: Arc::clone(&queued), over: Arc::clone(&over), lane: Arc::clone(&lane) },
        Rx { inner: rx, queued, over, lane },
    )
}

impl From<tokio::sync::mpsc::UnboundedSender<Wire>> for Tx {
    /// A control lane alone (tests: a receiver read directly).
    fn from(inner: tokio::sync::mpsc::UnboundedSender<Wire>) -> Tx {
        Tx { inner, queued: Arc::new(AtomicUsize::new(0)), over: Arc::new(AtomicBool::new(false)), lane: Arc::new(Lane::default()) }
    }
}

impl Tx {
    /// **On the control lane** (never dropped): `Err` once the session is
    /// over, or when the phone has fallen [`CONTROL_CAP`] behind (it's cut
    /// off; it connects again to a fresh state).
    pub fn send(&self, w: Wire) -> Result<(), Wire> {
        let size = w.size();
        if !matches!(w, Wire::Close) && self.queued.load(Ordering::Relaxed) + size > CONTROL_CAP {
            if !self.over.swap(true, Ordering::SeqCst) {
                tracing::info!("Yap Link: a phone fell too far behind; its connection is closed");
            }
            self.lane.wake.notify_one();
            return Err(w);
        }
        self.queued.fetch_add(size, Ordering::Relaxed);
        self.inner.send(w).map_err(|e| e.0)
    }

    /// The agent lane.
    pub fn lane(&self) -> &Lane {
        &self.lane
    }
}

impl Rx {
    /// **The next message to write**: the control lane first, then the agent
    /// lane; the coalesced state once it's due; `Close` when the phone fell
    /// too far behind; `None` once every sender is gone.
    pub async fn recv(&mut self) -> Option<Wire> {
        loop {
            if self.over.load(Ordering::SeqCst) {
                return Some(Wire::Close);
            }
            match self.inner.try_recv() {
                Ok(w) => return Some(self.took(w)),
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return None,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            }
            let (next, due) = {
                let mut l = self.lane.state.lock().unwrap_or_else(|p| p.into_inner());
                let now = Instant::now();
                if l.due_in(now) == Some(Duration::ZERO) {
                    l.flush();
                }
                (l.pop(), l.due_in(now))
            };
            if let Some(t) = next {
                return Some(Wire::Text(t));
            }
            let wait = due.unwrap_or(Duration::from_secs(3600));
            tokio::select! {
                biased;
                m = self.inner.recv() => match m {
                    Some(w) => return Some(self.took(w)),
                    None => return None,
                },
                _ = self.lane.wake.notified() => {}
                _ = tokio::time::sleep(wait) => {}
            }
        }
    }

    fn took(&self, w: Wire) -> Wire {
        self.queued.fetch_sub(w.size().min(self.queued.load(Ordering::Relaxed)), Ordering::Relaxed);
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(extra: Value) -> Value {
        let mut base = json!({ "id": "ab12", "status": "running", "folder": "garden-planner", "turn": 1, "ts": 5 });
        for (k, v) in extra.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    fn drain(l: &mut AgentLane) -> Vec<Value> {
        let mut out = Vec::new();
        while let Some(t) = l.pop() {
            out.push(serde_json::from_str(&t).unwrap());
        }
        out
    }

    #[test]
    fn state_alone_is_coalesced_and_a_step_goes_in_order() {
        assert!(state_only(&ev(json!({ "now": "Adding a watering schedule" }))));
        assert!(state_only(&ev(json!({ "todos": [], "cost": 0.02 }))));
        assert!(!state_only(&ev(json!({ "step": "Reading notes.md" }))));
        assert!(!state_only(&ev(json!({ "now": "x", "diff": {} }))));
        let t0 = Instant::now();
        let mut l = AgentLane::default();
        l.watch("ab12", 10);
        l.event("ab12", Some(11), ev(json!({ "now": "one" })), t0);
        l.event("ab12", Some(12), ev(json!({ "now": "two", "cost": 0.01 })), t0);
        l.event("ab12", Some(13), ev(json!({ "todos": [{"content": "Water", "status": "in_progress"}] })), t0);
        assert_eq!(l.len(), 1, "three state changes: one message");
        assert_eq!(l.due_in(t0), Some(FLUSH));
        assert_eq!(l.due_in(t0 + FLUSH), Some(Duration::ZERO));
        // A step: the state before it first, then the step.
        l.event("ab12", Some(14), ev(json!({ "step": "Editing notes.md" })), t0);
        let out = drain(&mut l);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["type"], "agent_event");
        assert_eq!((out[0]["seq"].as_u64(), out[0]["line"].as_u64()), (Some(1), Some(13)));
        assert_eq!(out[0]["event"]["now"], "two", "the latest wins");
        assert_eq!(out[0]["event"]["cost"], 0.01);
        assert_eq!(out[0]["event"]["todos"][0]["content"], "Water", "merged");
        assert_eq!((out[1]["seq"].as_u64(), out[1]["event"]["step"].as_str()), (Some(2), Some("Editing notes.md")));
        // The flush on its own.
        l.event("ab12", Some(15), ev(json!({ "status": "asking" })), t0);
        l.flush();
        assert_eq!(drain(&mut l)[0]["seq"], 3, "numbered without a gap");
    }

    #[test]
    fn only_the_watched_agent_and_nothing_the_snapshot_had() {
        let t0 = Instant::now();
        let mut l = AgentLane::default();
        l.event("ab12", Some(1), ev(json!({ "step": "x" })), t0);
        assert!(l.is_empty(), "nothing watched");
        l.watch("ab12", 20);
        l.event("cd34", Some(30), ev(json!({ "step": "another agent" })), t0);
        l.event("ab12", Some(20), ev(json!({ "step": "in the snapshot already" })), t0);
        l.event("ab12", Some(19), ev(json!({ "step": "older" })), t0);
        assert!(l.is_empty());
        l.event("ab12", Some(21), ev(json!({ "step": "new" })), t0);
        l.event("ab12", None, ev(json!({ "step": "past the log's cap" })), t0);
        assert_eq!(drain(&mut l).len(), 2);
        l.unwatch();
        l.event("ab12", Some(22), ev(json!({ "step": "x" })), t0);
        assert!(l.is_empty() && l.watching().is_none());
    }

    #[test]
    fn a_full_lane_is_emptied_and_the_phone_told_to_open_it_again() {
        let t0 = Instant::now();
        let mut l = AgentLane::default();
        l.watch("ab12", 0);
        for i in 1..=(LANE_MESSAGES as u64 + 1) {
            l.event("ab12", Some(i), ev(json!({ "step": format!("Step {i}") })), t0);
        }
        let out = drain(&mut l);
        assert_eq!(out, vec![json!({ "type": "agent_resync", "id": "ab12" })]);
        // Until it's opened again, nothing more.
        l.event("ab12", Some(999), ev(json!({ "step": "late" })), t0);
        assert!(l.is_empty());
        l.watch("ab12", 999);
        l.event("ab12", Some(1000), ev(json!({ "step": "live again" })), t0);
        assert_eq!(drain(&mut l)[0]["seq"], 1);
        // By bytes too (a few big diffs).
        let big = "x".repeat(LANE_BYTES / 3);
        for i in 0..4 {
            l.event("ab12", Some(1001 + i), ev(json!({ "said": big })), t0);
        }
        assert_eq!(drain(&mut l)[0]["type"], "agent_resync");
    }

    #[tokio::test]
    async fn the_control_lane_goes_first_and_is_capped() {
        let (tx, mut rx) = channel();
        tx.lane().with(|l| {
            l.watch("ab12", 0);
            l.event("ab12", Some(1), ev(json!({ "step": "a step" })), Instant::now());
        });
        tx.send(Wire::Text("card".into())).unwrap();
        assert!(matches!(rx.recv().await, Some(Wire::Text(t)) if t == "card"), "control first");
        assert!(matches!(rx.recv().await, Some(Wire::Text(t)) if t.contains("a step")));
        // The coalesced state, once due.
        tx.lane().with(|l| l.event("ab12", Some(2), ev(json!({ "now": "later" })), Instant::now()));
        let started = Instant::now();
        assert!(matches!(rx.recv().await, Some(Wire::Text(t)) if t.contains("later")));
        assert!(started.elapsed() >= FLUSH - Duration::from_millis(20));
        // A phone that stopped reading: past the cap it's closed.
        let big = "y".repeat(CONTROL_CAP / 2);
        tx.send(Wire::Text(big.clone())).unwrap();
        assert!(tx.send(Wire::Text(big.clone())).is_ok(), "just under");
        assert!(tx.send(Wire::Text(big)).is_err(), "over");
        assert!(matches!(rx.recv().await, Some(Wire::Close)));
        drop(tx);
    }
}
