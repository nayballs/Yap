//! **Streaming answers** (2026-10-22): a Chat turn's model calls — every
//! step of the tool loop, a small model's plain answer — the notepad's Ask
//! sheet and the Voice Assistant show the answer as it's written instead of
//! after it's done. On the on-device Qwen3.5-4B the first words came after
//! 0.49 s instead of 2.86 s for a long answer (the model evaluations,
//! docs/model-evals.md "Streaming answers").
//!
//! The layers:
//! - **The wire** ([`SseParser`], [`Assembler`]; pure): OpenAI's
//!   `chat.completion.chunk` server-sent events as every provider Yap talks
//!   to sends them — llama.cpp (the sidecar, LM Studio), Ollama, Groq,
//!   OpenAI, OpenRouter (its `: OPENROUTER PROCESSING` keep-alives),
//!   Anthropic's and Gemini's OpenAI-compatible endpoints (Gemini's tool
//!   calls come whole, sometimes without an `index`). Content deltas; a tool
//!   call as a header (index, id, name) then argument fragments, put back
//!   together and checked as JSON; usage (`usage`, or Groq's `x_groq.usage`)
//!   and llama.cpp's `timings` from the last chunks; an `error` object mid-
//!   stream. `llm::post_chat_streamed` drives them, and falls back to a
//!   whole answer when streaming fails.
//! - **A step** ([`Step`]): one model call's deltas → what the page sees.
//!   With tools offered, a step's opening text is held until it is either a
//!   tool call (→ a chip, the text never shown) or about a sentence of
//!   answer ([`releases`]); a search that must come first shows none of
//!   its text, only its call ([`ChipsOnly`]); reasoning blocks are hidden
//!   when the scope says so; the text goes out at most every [`MIN_GAP`];
//!   a question asked to think it through (`think.rs`) shows its thinking
//!   apart from the answer ([`Event::Thinking`]: `reasoning_content`, or
//!   `reasoning`, deltas).
//! - **The watcher** ([`Sink`]): the Chat view's channel (`chat_send`), the
//!   notepad's, the Voice Assistant (speaks sentence by sentence), a model
//!   evaluation (times the first words). [`Watch`] keeps what was shown, for
//!   a Stop.
//! - **Beside a dictation** ([`Dictations`]): a model on this PC never
//!   slows a dictation: a stream is dropped when a dictation starts (llama.cpp
//!   cancels on a closed connection), waited out and asked again
//!   ([`call`]).
//! - **Stop** ([`Stoppable`]): the page's ■ closes the stream; what was shown
//!   so far is kept (checked like any answer, `turn::chat_turn`). **A
//!   newer question** to the same conversation ends a turn still answering
//!   there the same way, with nothing saved ([`Halt::Superseded`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::chat::chats::Source;
use crate::ai::llm::Delta;

/// The shortest gap between two text updates a step sends (the page renders
/// markdown on each).
pub const MIN_GAP: Duration = Duration::from_millis(50);

/// With tools offered, a step's opening text this long is an answer even
/// without a sentence's end ([`releases`]).
const RELEASE_CHARS: usize = 200;

/// A dictation starting this many times in a row during one step gives up
/// (the person is dictating nonstop; they can ask again).
const YIELD_RETRIES: usize = 5;

// ── what the page sees ──────────────────────────────────────────────────

/// What a stream tells its watcher (the page's channel: `{kind, …}`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    /// The answer so far, whole ("" = nothing to show: a step's text taken
    /// back when it turned out to come before a tool call).
    Text { text: String },
    /// A tool call began: what Yap is doing for it ("Searching the web…").
    Tool { name: String, doing: String },
    /// The tools run so far: their chips, the web sources found so far (an
    /// answer's "[1]" draws as a pill while it's written), and whether the
    /// turn has read the web.
    Chips { chips: Vec<String>, sources: Vec<Source>, web: bool },
    /// A model on this PC stepped aside for a dictation (`true`), or goes on.
    Waiting { waiting: bool },
    /// The model's thinking so far, whole (a question asked to think it
    /// through, `think.rs`): shown folded, apart from the answer.
    Thinking { text: String },
    /// A document of this chat was made or changed (`documents.rs`): the
    /// panel beside the chat shows it, as it is now.
    Document {
        #[serde(rename = "noteId")]
        note_id: u64,
        title: String,
    },
    /// A document's section as it's written (a `write_section` call's
    /// arguments so far, [`LIVE_ARGS`]): the panel shows it in its
    /// placeholder's place until the call lands.
    Section {
        #[serde(rename = "noteId")]
        note_id: u64,
        key: String,
        text: String,
    },
}

/// Tool calls whose arguments the page sees as they come: a document's
/// section, written into the panel while it streams (`documents.rs`).
pub const LIVE_ARGS: &[&str] = &["write_section", "rewrite_section"];

/// Who watches a stream.
pub trait Sink: Send + Sync {
    fn send(&self, ev: Event);
}

/// What a tool call is doing, for the chip shown while it runs.
pub fn doing(name: &str) -> &'static str {
    match name {
        "search_notes" => "Searching your notes…",
        "get_note" | "read_note" | "search_in_note" => "Reading a note…",
        "edit_note" | "update_note" => "Changing a note…",
        "create_note" => "Writing a note…",
        "list_folders" => "Looking at your folders…",
        "copy_to_clipboard" => "Copying…",
        "web_search" => "Searching the web…",
        "read_page" => "Reading a page…",
        "open_app" | "list_apps" => "Looking for the app…",
        "remember" | "forget" => "Updating Yap's memory…",
        "search_chats" => "Searching your chats…",
        "create_document" => "Starting a document…",
        "write_section" | "rewrite_section" => "Writing the document…",
        _ => "Working…",
    }
}

/// **What a stream showed** (the answer's text, the chips, the sources
/// found), passed on to `inner` as it goes — what a Stop keeps, and when
/// the first words showed (a model evaluation's measure).
pub struct Watch<'a> {
    inner: &'a dyn Sink,
    seen: Mutex<Seen>,
    started: Instant,
}

#[derive(Default, Clone)]
pub struct Seen {
    pub text: String,
    pub chips: Vec<String>,
    pub sources: Vec<Source>,
    pub web: bool,
    /// When the answer's first words showed, after the watch began.
    pub first_words: Option<Duration>,
}

impl<'a> Watch<'a> {
    pub fn new(inner: &'a dyn Sink) -> Watch<'a> {
        Watch { inner, seen: Mutex::new(Seen::default()), started: Instant::now() }
    }

    pub fn seen(&self) -> Seen {
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl Sink for Watch<'_> {
    fn send(&self, ev: Event) {
        {
            let mut s = self.seen.lock().unwrap_or_else(|p| p.into_inner());
            match &ev {
                Event::Text { text } => {
                    if s.first_words.is_none() && !text.trim().is_empty() {
                        s.first_words = Some(self.started.elapsed());
                    }
                    s.text = text.clone();
                }
                Event::Chips { chips, sources, web } => {
                    s.chips = chips.clone();
                    s.sources = sources.clone();
                    s.web = *web;
                }
                _ => {}
            }
        }
        self.inner.send(ev);
    }
}

/// **A step whose words are never the answer**: a search that must come
/// first (`tools::FirstStep::MustSearch`). Its tool call's chip goes on to
/// the watcher, and so does a wait for a dictation; its text never does —
/// not to the page, not to the Voice Assistant's voice. llama.cpp's
/// `tool_choice: "required"` lets Qwen3.5 write before its call ("I can't
/// provide weather information for Kellbrook, as I don't have access to
/// real-time weather data…", 15 times in 150 on one question), and that
/// went out as the answer's first sentence and was said aloud before any
/// search had run (a tester's voice chat, 2026-10-23).
pub struct ChipsOnly<'a>(pub &'a dyn Sink);

impl Sink for ChipsOnly<'_> {
    fn send(&self, ev: Event) {
        if !matches!(ev, Event::Text { .. }) {
            self.0.send(ev);
        }
    }
}

// ── one step ────────────────────────────────────────────────────────────

/// Is a step's opening text (`text`, as shown) an answer yet? Without tools
/// offered, as soon as there's any. With them, the model may be writing a
/// line before a tool call ("I'll search your notes for that."), so it waits
/// for **a sentence and then more**: a sentence's end (". ", "! ", "? ", ": "
/// or a line break) followed by more text — a model that announces a call
/// goes straight from its full stop to the call — or [`RELEASE_CHARS`] of
/// text. Pure.
pub fn releases(text: &str, tools_offered: bool) -> bool {
    let t = text.trim_start();
    if t.trim().is_empty() {
        return false;
    }
    if !tools_offered || t.chars().count() >= RELEASE_CHARS {
        return true;
    }
    let mut chars = t.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let end = match c {
            '\n' => true,
            '.' | '!' | '?' | ':' | '…' => chars.peek().is_some_and(|&(_, n)| n.is_whitespace()),
            _ => false,
        };
        if end && !t[i + c.len_utf8()..].trim().is_empty() {
            return true;
        }
    }
    false
}

/// `raw` without reasoning blocks, as `llm::strip_thinking` leaves an answer
/// — and, while it's still being written, without a tag that's only half
/// there ("<thi" at the end). Pure.
pub fn without_thinking(raw: &str) -> String {
    let mut t = crate::ai::llm::strip_thinking(raw);
    for tag in ["<think>", "<thinking>", "</think>", "</thinking>"] {
        for n in (1..tag.len()).rev() {
            if t.to_ascii_lowercase().ends_with(&tag[..n]) {
                let keep = t.len() - n;
                t.truncate(keep);
                return t.trim_end().to_string();
            }
        }
    }
    t
}

/// One model call's stream → what the page sees ([`Event`]).
pub struct Step<'a> {
    sink: &'a dyn Sink,
    tools_offered: bool,
    disable_thinking: bool,
    raw: String,
    /// What this step showed last (`None`: nothing).
    shown: Option<String>,
    released: bool,
    tool_called: bool,
    last_sent: Option<Instant>,
    /// Its thinking (a question asked to think it through), and when it
    /// was last sent; `thought_sent`: all of it has been.
    thinking: String,
    thinking_at: Option<Instant>,
    thought_sent: bool,
    /// A document's section being written: when it last went to the page,
    /// and what (`Event::Section`, at most every [`MIN_GAP`]).
    section_at: Option<Instant>,
    section_sent: String,
}

impl<'a> Step<'a> {
    pub fn new(sink: &'a dyn Sink, tools_offered: bool, disable_thinking: bool) -> Step<'a> {
        Step {
            sink,
            tools_offered,
            disable_thinking,
            raw: String::new(),
            shown: None,
            released: false,
            tool_called: false,
            last_sent: None,
            thinking: String::new(),
            thinking_at: None,
            thought_sent: true,
            section_at: None,
            section_sent: String::new(),
        }
    }

    /// A `write_section` (or `rewrite_section`) call's arguments so far: the section as written
    /// so far to the page (the panel), at most every [`MIN_GAP`].
    fn send_section(&mut self, so_far: &str) {
        if self.section_at.is_some_and(|t| t.elapsed() < MIN_GAP) {
            return;
        }
        let Some((note_id, key, text)) = crate::chat::documents::live_section(so_far) else { return };
        if text == self.section_sent {
            return;
        }
        self.section_at = Some(Instant::now());
        self.section_sent = text.clone();
        self.sink.send(Event::Section { note_id, key, text });
    }

    /// The text as shown: reasoning out (when the scope says so — and a
    /// thinking budget's end tag, always), gpt-oss's "【1】" citations as
    /// Yap's "[1]".
    fn visible(&self) -> String {
        let t = if self.disable_thinking { without_thinking(&self.raw) } else { crate::chat::think::tidy(&self.raw) };
        crate::web::bracket_citations(t.trim())
    }

    /// The thinking so far to the page (at most every [`MIN_GAP`], unless
    /// `force`).
    fn send_thinking(&mut self, force: bool) {
        if self.thought_sent || (!force && self.thinking_at.is_some_and(|t| t.elapsed() < MIN_GAP)) {
            return;
        }
        self.thinking_at = Some(Instant::now());
        self.thought_sent = true;
        // After the turn's earlier steps' (a tool loop thinks at each).
        let before = crate::chat::think::so_far();
        let text = match (before.is_empty(), self.thinking.trim()) {
            (true, now) => now.to_string(),
            (false, "") => before,
            (false, now) => format!("{before}\n\n{now}"),
        };
        self.sink.send(Event::Thinking { text });
    }

    pub fn on(&mut self, d: Delta<'_>) {
        match d {
            Delta::Reasoning(s) => {
                // Only a question asked to think it through shows it: a model
                // that sends its reasoning anyway (Groq's gpt-oss, a
                // reasoning model on OpenRouter) shows none, as before Think.
                if !crate::chat::think::wanted() {
                    return;
                }
                crate::chat::think::tick();
                self.thinking.push_str(s);
                self.thought_sent = false;
                self.send_thinking(false);
            }
            Delta::Content(s) => {
                if self.tool_called {
                    return;
                }
                // The thinking's end, whole, before the answer's first words.
                self.send_thinking(true);
                self.raw.push_str(s);
                self.send_text(false);
            }
            Delta::ToolCall(name) => {
                self.send_thinking(true);
                if !self.tool_called {
                    self.tool_called = true;
                    // Text shown before the call wasn't the answer.
                    if self.shown.as_deref().is_some_and(|s| !s.is_empty()) {
                        self.shown = Some(String::new());
                        self.sink.send(Event::Text { text: String::new() });
                    }
                }
                self.sink.send(Event::Tool { name: name.to_string(), doing: doing(name).to_string() });
            }
            Delta::ToolArgs { name, so_far } => {
                if LIVE_ARGS.contains(&name) {
                    self.send_section(so_far);
                }
            }
            Delta::Restart => self.restart(),
        }
    }

    /// The call goes again (a dropped stream, a fallback): what came is void.
    /// What's on screen stays until the new call's text replaces it.
    pub fn restart(&mut self) {
        self.raw.clear();
        self.released = false;
        self.tool_called = false;
        self.thinking.clear();
        self.thought_sent = true;
        self.section_at = None;
        self.section_sent.clear();
    }

    fn send_text(&mut self, force: bool) {
        let text = self.visible();
        if !self.released {
            if !(force && !text.trim().is_empty()) && !releases(&text, self.tools_offered) {
                return;
            }
            self.released = true;
        } else if !force && self.last_sent.is_some_and(|t| t.elapsed() < MIN_GAP) {
            return;
        }
        if self.shown.as_deref() != Some(text.as_str()) {
            self.last_sent = Some(Instant::now());
            self.shown = Some(text.clone());
            self.sink.send(Event::Text { text });
        }
    }

    /// The call is over. `answered`: it was the answer (no tool calls), so
    /// all of it shows now.
    pub fn end(&mut self, answered: bool) {
        if answered && !self.tool_called {
            self.send_text(true);
        }
    }
}

// ── beside a dictation ──────────────────────────────────────────────────

/// What a stream to a model on this PC steps aside for.
pub trait Dictations: Send + Sync {
    /// Something to compare later (the dictations started so far).
    fn mark(&self) -> u64;
    /// The stream must make way now (`mark`: when it began).
    fn must_yield(&self, mark: u64) -> bool;
    /// A dictation is recording or being transcribed / cleaned up.
    fn busy(&self) -> bool;
    /// Wait for one to be done before the first call (the notepad's helpers,
    /// `meeting_summary::chat_beside_dictation`), not only during it.
    fn waits_first(&self) -> bool;
}

/// Chat's rule: a **new dictation** starting while a step streams drops it
/// — not one already under way when the step began (the Voice Assistant's
/// own question is still being finished then, and must not wait).
pub struct NewDictations(pub tauri::AppHandle);

impl Dictations for NewDictations {
    fn mark(&self) -> u64 {
        crate::dictation::pipeline::dictation_state().0
    }
    fn must_yield(&self, mark: u64) -> bool {
        crate::dictation::pipeline::dictation_state().0 != mark
    }
    fn busy(&self) -> bool {
        crate::meetings::meeting_summary::dictation_busy(&self.0)
    }
    fn waits_first(&self) -> bool {
        false
    }
}

/// The meeting helpers' rule (`chat_beside_dictation`): wait out any
/// dictation first, drop the call whenever one is busy.
pub struct AnyDictation(pub tauri::AppHandle);

impl Dictations for AnyDictation {
    fn mark(&self) -> u64 {
        0
    }
    fn must_yield(&self, _: u64) -> bool {
        crate::meetings::meeting_summary::dictation_busy(&self.0)
    }
    fn busy(&self) -> bool {
        crate::meetings::meeting_summary::dictation_busy(&self.0)
    }
    fn waits_first(&self) -> bool {
        true
    }
}

/// Where a call's stream goes: the watcher, and — for a model on this PC —
/// the dictations it steps aside for.
#[derive(Clone, Copy)]
pub struct Live<'a> {
    pub sink: &'a dyn Sink,
    pub beside: Option<&'a dyn Dictations>,
}

/// **One model call, streamed** to `live`: its deltas through a [`Step`]
/// (`tools_offered`: hold the opening text; `disable_thinking`: hide
/// reasoning), the whole message back as `llm::post_chat_request` returns
/// it. A model on this PC steps aside for a dictation: the stream is dropped
/// (llama.cpp stops generating), the page told it's waiting, and the call
/// goes again once the dictation is done — unless that dictation was a
/// newer question to the same conversation (the Voice Assistant asked
/// again, a question dictated into Chat and sent): saving it supersedes this
/// turn ([`Stoppable::answers`]), and `chat_turn` drops this call, waiting
/// or asking again, as ■ would.
pub async fn call(live: Live<'_>, req: &crate::ai::llm::ChatRequest, tools_offered: bool, disable_thinking: bool) -> Result<Value, String> {
    let mut step = Step::new(live.sink, tools_offered, disable_thinking);
    let Some(beside) = live.beside else {
        let out = {
            let mut on = |d: Delta<'_>| step.on(d);
            crate::ai::llm::post_chat_streamed(req, &mut on).await
        };
        step.end(out.as_ref().is_ok_and(answers));
        return out;
    };
    // Held busy while it waits out a dictation, not only while a request
    // runs (`local_llm::hold`).
    let _sidecar = crate::ai::local_llm::hold(&req.base_url);
    // Stepped aside for a dictation (the page shows it's waiting).
    let mut yielded = false;
    for attempt in 0..YIELD_RETRIES {
        if attempt > 0 || beside.waits_first() {
            let waiting = yielded || beside.busy();
            if waiting && !yielded {
                live.sink.send(Event::Waiting { waiting: true });
            }
            crate::meetings::meeting_summary::wait_idle(|| beside.busy(), || true).await;
            if waiting {
                live.sink.send(Event::Waiting { waiting: false });
            }
        }
        let mark = beside.mark();
        let out = {
            let mut on = |d: Delta<'_>| step.on(d);
            let request = crate::ai::llm::post_chat_streamed(req, &mut on);
            crate::meetings::meeting_summary::unless_busy(request, || beside.must_yield(mark)).await
        };
        match out {
            Some(out) => {
                step.end(out.as_ref().is_ok_and(answers));
                return out;
            }
            None => {
                tracing::info!("Streamed answer set aside for a dictation; asking again after it");
                live.sink.send(Event::Waiting { waiting: true });
                yielded = true;
                step.restart();
            }
        }
    }
    Err("Yap kept dictating, so the answer waited. Ask again in a moment.".to_string())
}

/// The message is an answer: no tool calls.
fn answers(msg: &Value) -> bool {
    msg["tool_calls"].as_array().is_none_or(|c| c.is_empty())
}

// ── Stop, and a newer question ──────────────────────────────────────────

/// Why a turn ended before its answer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    /// The page's ■: what was shown stays, checked and saved as stopped.
    Stopped,
    /// **A newer question to the same conversation** replaced it — the Voice
    /// Assistant asked again (a barge-in, a follow-up), a question typed in
    /// Chat, one dictated into Chat and sent while this was being answered:
    /// nothing is saved for it (`turn::chat_turn`); its question stays,
    /// and the model reads it with the newer one. Before, it was asked
    /// again once the dictation it had stepped aside for — the newer
    /// question — was done, and answered after the newer one: both answers
    /// showed under both questions (a tester's voice chat, 2026-10-23).
    Superseded,
}

/// A turn's off switch: fired once, with why (the first reason stays).
#[derive(Default)]
struct Signal {
    why: Mutex<Option<Halt>>,
    notify: tokio::sync::Notify,
}

impl Signal {
    fn fire(&self, why: Halt) {
        self.why.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert(why);
        // `notify_one` keeps the permit if nobody waits yet.
        self.notify.notify_one();
    }

    fn fired(&self) -> Option<Halt> {
        *self.why.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The streams a page can stop (its ■): by the id it sent with the call.
static STOPS: Mutex<Option<HashMap<String, Arc<Signal>>>> = Mutex::new(None);

/// The turn answering each conversation now (conversation id → the turn's
/// number, its switch): a newer question saved there ends the older one.
type Answering = HashMap<u64, (u64, Arc<Signal>)>;
static ANSWERING: Mutex<Option<Answering>> = Mutex::new(None);

/// Numbers the turns that answer a conversation ([`Stoppable::answers`]).
static TURNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A turn's answer that can be **stopped** (the page's ■, registered under
/// the page's id) and **superseded** (a newer question to its conversation,
/// once [`Stoppable::answers`] says which) until dropped.
pub struct Stoppable {
    id: Option<String>,
    signal: Arc<Signal>,
    /// The conversation it answers, and its turn's number there.
    answering: Mutex<Option<(u64, u64)>>,
}

impl Stoppable {
    /// `id`: the page's (none: no ■ reaches it; only a newer question can
    /// end it, once it [`answers`](Self::answers) a conversation).
    pub fn new(id: Option<&str>) -> Stoppable {
        let signal = Arc::new(Signal::default());
        let id = id.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(id) = &id {
            STOPS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(id.clone(), Arc::clone(&signal));
        }
        Stoppable { id, signal, answering: Mutex::new(None) }
    }

    /// Its question is saved in `conversation`: from now on this turn is the
    /// one answering there, and an older turn still answering there is
    /// **superseded** — it ends, and nothing is saved for it. The newest
    /// question decides, whichever surface asked it: the Voice Assistant's
    /// barge-in or follow-up, Chat's typed or dictated question, "open
    /// Teams". (A dictation into another app is no question here: a model on
    /// this PC still steps aside for it and asks again, [`call`].)
    pub fn answers(&self, conversation: u64) {
        let n = TURNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let older = ANSWERING
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(conversation, (n, Arc::clone(&self.signal)));
        *self.answering.lock().unwrap_or_else(|p| p.into_inner()) = Some((conversation, n));
        if let Some((_, older)) = older.filter(|(_, s)| !Arc::ptr_eq(s, &self.signal)) {
            tracing::info!(conversation, "A newer question in the conversation replaces the answer still coming");
            older.fire(Halt::Superseded);
        }
    }

    /// Fires when the page stops it or a newer question replaces it: why.
    pub async fn halted(&self) -> Halt {
        loop {
            if let Some(why) = self.signal.fired() {
                return why;
            }
            self.signal.notify.notified().await;
        }
    }
}

impl Drop for Stoppable {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            if let Some(map) = STOPS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                map.remove(id);
            }
        }
        let answering = *self.answering.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((conversation, n)) = answering {
            if let Some(map) = ANSWERING.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                // Only its own entry: a newer turn's stays.
                if map.get(&conversation).is_some_and(|(m, _)| *m == n) {
                    map.remove(&conversation);
                }
            }
        }
    }
}

/// What a stopped answer with nothing written yet ends in (the page, which
/// asked for the stop, shows "Stopped").
pub const STOPPED: &str = "Stopped before the answer began.";

/// What a superseded turn ends in ([`Halt::Superseded`]): no answer saved,
/// the question kept. The pages take its bubble away, quietly.
pub const SUPERSEDED: &str = "A newer question replaced this one.";

/// `work`, unless `stop` fires first (`Err`: why — `work` is dropped, which
/// closes its stream).
pub async fn unless_stopped<T>(work: impl std::future::Future<Output = T>, stop: Option<&Stoppable>) -> Result<T, Halt> {
    match stop {
        None => Ok(work.await),
        Some(s) => tokio::select! {
            out = work => Ok(out),
            why = s.halted() => Err(why),
        },
    }
}

/// Stop the stream the page called `id` (its ■; `why` Superseded: a newer
/// question replaces it). `true` if one was running.
pub fn stop(id: &str, why: Halt) -> bool {
    let signal = STOPS.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(id).cloned());
    match signal {
        Some(s) => {
            s.fire(why);
            true
        }
        None => false,
    }
}

/// ■ on a streamed answer (Chat, the notepad's Ask sheet): the stream closes
/// (a model on this PC stops generating) and what was shown is kept.
/// `superseded`: the Chat view is about to send a newer question (one
/// dictated and sent by itself while this answer came): the turn ends with
/// nothing saved ([`Halt::Superseded`]), the newer question goes next.
#[tauri::command]
pub fn stream_stop(id: String, superseded: Option<bool>) -> bool {
    let why = if superseded.unwrap_or(false) { Halt::Superseded } else { Halt::Stopped };
    let stopped = stop(&id, why);
    tracing::info!(stopped, superseded = why == Halt::Superseded, "A streamed answer was stopped");
    stopped
}

/// The page's channel for a streamed answer, when it sent one (a command
/// argument; a call without one — the e2e suite's direct invokes — gets
/// its answer whole, as before). Tauri's own `Channel` argument can't be
/// left out.
pub struct PageChannel(pub Option<tauri::ipc::Channel<Value>>);

impl<'de, R: tauri::Runtime> tauri::ipc::CommandArg<'de, R> for PageChannel {
    fn from_command(command: tauri::ipc::CommandItem<'de, R>) -> Result<Self, tauri::ipc::InvokeError> {
        let webview = command.message.webview();
        let value: Option<String> =
            serde::Deserialize::deserialize(command).map_err(|e| tauri::ipc::InvokeError::from(e.to_string()))?;
        match value {
            None => Ok(PageChannel(None)),
            Some(v) => v
                .parse::<tauri::ipc::JavaScriptChannelId>()
                .map(|id| PageChannel(Some(id.channel_on(webview))))
                .map_err(|_| tauri::ipc::InvokeError::from(format!("not a channel: {v}"))),
        }
    }
}

impl PageChannel {
    /// Send `v` down the channel, if there is one.
    pub fn send_json(&self, v: Value) {
        if let Some(ch) = &self.0 {
            let _ = ch.send(v);
        }
    }
}

impl Sink for PageChannel {
    fn send(&self, ev: Event) {
        self.send_json(serde_json::to_value(ev).unwrap_or(Value::Null));
    }
}

// ── the wire ────────────────────────────────────────────────────────────

/// Server-sent events → their `data`, as bytes come. A line split across
/// chunks (or a character split mid-way) waits for its end; comments
/// (`: keep-alive`, OpenRouter's `: OPENROUTER PROCESSING`) and other fields
/// are skipped; a bare JSON line counts as data (a server writing one JSON
/// object per line).
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
    data: String,
    has_data: bool,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.line(&line, &mut out);
        }
        out
    }

    /// The stream ended: anything left.
    pub fn finish(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_string();
            self.line(&line, &mut out);
        }
        self.line("", &mut out);
        out
    }

    fn line(&mut self, line: &str, out: &mut Vec<String>) {
        if line.is_empty() {
            if self.has_data {
                out.push(std::mem::take(&mut self.data));
                self.has_data = false;
            }
            return;
        }
        if line.starts_with(':') {
            return;
        }
        if line.starts_with('{') {
            if self.has_data {
                out.push(std::mem::take(&mut self.data));
                self.has_data = false;
            }
            out.push(line.to_string());
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        if field == "data" {
            if self.has_data {
                self.data.push('\n');
            }
            self.data.push_str(value);
            self.has_data = true;
        }
    }
}

/// One tool call as its pieces come.
#[derive(Debug, Default)]
struct PartialCall {
    index: Option<u64>,
    id: String,
    name: String,
    arguments: String,
    /// A server that sends the arguments as an object.
    object: Option<Value>,
    announced: bool,
}

/// What a chunk adds ([`Assembler::push`]).
#[derive(Debug, PartialEq)]
pub enum Piece {
    Content(String),
    /// A tool call began (its name).
    Call(String),
    /// More of the model's thinking, apart from the answer.
    Reasoning(String),
    /// A [`LIVE_ARGS`] call's arguments so far, whole (a document's
    /// section as it's written).
    Args { name: String, so_far: String },
}

/// `chat.completion.chunk`s → the whole assistant message, as
/// `llm::post_chat_request` returns a non-streamed one.
#[derive(Debug, Default)]
pub struct Assembler {
    content: String,
    /// The thinking it sent apart (`reasoning_content`: llama.cpp; or
    /// `reasoning`: Groq, OpenRouter) — never part of the message.
    reasoning: String,
    calls: Vec<PartialCall>,
    finish: Option<String>,
    usage: Value,
    timings: Value,
    model: Option<String>,
    done: bool,
    chunks: usize,
    /// The content's length at the last look for a loop ([`repeating_from`]).
    checked: usize,
    /// The answer began repeating itself: cut there, and the stream ends.
    looped: bool,
}

/// Look for a loop again after this much more text.
const LOOP_CHECK_EVERY: usize = 48;

/// Where a text that has begun repeating itself should end: the length to
/// keep (one copy of what repeats), or `None`. A block of 160+ bytes said
/// twice in a row at the end, or a shorter one (20+) said four times over
/// 480+ bytes. Small models do this (Qwen3.5-4B wrote one answer's two
/// paragraphs 12 times over until a new question stopped it, 26 Oct 2026);
/// nothing a person wants repeats a whole paragraph back to back. Pure.
pub fn repeating_from(text: &str) -> Option<usize> {
    const KEY: usize = 24;
    const LONG: usize = 160;
    const SHORT: usize = 20;
    const MAX: usize = 6_000;
    let b = text.as_bytes();
    let n = b.len();
    if n < 2 * SHORT + KEY {
        return None;
    }
    let key = &b[n - KEY..];
    // Each earlier place the text's last bytes were written: a period.
    let mut end = n - 1;
    while let Some(at) = find_last(&b[..end], key) {
        let p = n - (at + KEY);
        end = at + KEY - 1;
        if p > MAX || 2 * p > n {
            break;
        }
        if p < SHORT {
            continue;
        }
        let mut copies = 1;
        while (copies + 1) * p <= n && b[n - (copies + 1) * p..n - copies * p] == b[n - p..] {
            copies += 1;
        }
        if (p >= LONG && copies >= 2) || (copies >= 4 && copies * p >= 480) {
            // Keep one copy: what came before the repeats, and the first.
            let mut keep = n - (copies - 1) * p;
            while !text.is_char_boundary(keep) {
                keep -= 1;
            }
            // End on a sentence or line when one is near.
            let cut = text[..keep].rfind(['\n', '.', '!', '?']).filter(|&i| keep - i <= p).map_or(keep, |i| i + 1);
            return Some(cut);
        }
    }
    None
}

/// The last place `needle` starts in `hay`. Pure.
fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).rev().find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Why a stream's message can't be used.
#[derive(Debug, PartialEq)]
pub enum Unusable {
    /// A tool call's arguments aren't JSON: the model is told and asked
    /// again (`llm::TOOL_CALL_REJECTED`).
    BadArguments(String),
}

impl Assembler {
    /// One event's data. `Err`: the provider sent an error object (the
    /// whole data, for `llm::tool_call_rejection` and the log).
    pub fn push(&mut self, data: &str) -> Result<Vec<Piece>, String> {
        let data = data.trim();
        if data == "[DONE]" {
            self.done = true;
            return Ok(Vec::new());
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return Ok(Vec::new());
        };
        self.chunks += 1;
        if v.get("error").is_some_and(|e| !e.is_null()) {
            return Err(data.to_string());
        }
        if let Some(m) = v["model"].as_str() {
            self.model.get_or_insert_with(|| m.to_string());
        }
        if v["usage"].is_object() {
            self.usage = v["usage"].clone();
        } else if v["x_groq"]["usage"].is_object() && !self.usage.is_object() {
            self.usage = v["x_groq"]["usage"].clone();
        }
        if v["timings"].is_object() {
            self.timings = v["timings"].clone();
        }
        let mut out = Vec::new();
        for choice in v["choices"].as_array().into_iter().flatten() {
            // A server that sends `message` in its chunks, not `delta`.
            let delta = if choice["delta"].is_object() { &choice["delta"] } else { &choice["message"] };
            // Thinking, apart from the answer (a question asked to think it
            // through).
            if let Some(r) = delta["reasoning_content"].as_str().or_else(|| delta["reasoning"].as_str()).filter(|r| !r.is_empty()) {
                self.reasoning.push_str(r);
                out.push(Piece::Reasoning(r.to_string()));
            }
            if let Some(s) = delta["content"].as_str() {
                if !s.is_empty() && !self.looped {
                    self.content.push_str(s);
                    out.push(Piece::Content(s.to_string()));
                    // An answer that has begun repeating itself ends here:
                    // one copy kept, the stream closed (llama.cpp stops
                    // writing when it's gone).
                    if self.content.len() >= self.checked + LOOP_CHECK_EVERY {
                        self.checked = self.content.len();
                        if let Some(cut) = repeating_from(&self.content) {
                            tracing::info!(written = self.content.len(), kept = cut, "Chat: the answer began repeating itself; it stops there");
                            self.content.truncate(cut);
                            self.looped = true;
                            self.done = true;
                            self.finish.get_or_insert_with(|| "stop".to_string());
                        }
                    }
                }
            }
            for tc in delta["tool_calls"].as_array().into_iter().flatten() {
                let at = self.call_for(tc);
                let call = &mut self.calls[at];
                if let Some(id) = tc["id"].as_str().filter(|s| !s.is_empty()) {
                    if call.id.is_empty() {
                        call.id = id.to_string();
                    }
                }
                if let Some(name) = tc["function"]["name"].as_str().filter(|s| !s.is_empty()) {
                    if call.name.is_empty() {
                        call.name = name.to_string();
                    }
                }
                let more = match &tc["function"]["arguments"] {
                    Value::String(s) => {
                        call.arguments.push_str(s);
                        !s.is_empty()
                    }
                    obj @ Value::Object(_) => {
                        call.object = Some(obj.clone());
                        true
                    }
                    _ => false,
                };
                if !call.announced && !call.name.is_empty() {
                    call.announced = true;
                    out.push(Piece::Call(call.name.clone()));
                }
                // A document's section: the page sees it as it's written.
                if more && LIVE_ARGS.contains(&call.name.as_str()) {
                    let so_far = call.object.as_ref().map_or_else(|| call.arguments.clone(), Value::to_string);
                    out.push(Piece::Args { name: call.name.clone(), so_far });
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                self.finish = Some(f.to_string());
            }
        }
        Ok(out)
    }

    /// Which call a tool-call delta belongs to: by its `index`; without one
    /// (Gemini), a new call when it brings a new id, or a name while the last
    /// call is whole; else the last.
    fn call_for(&mut self, tc: &Value) -> usize {
        if let Some(index) = tc["index"].as_u64() {
            if let Some(at) = self.calls.iter().position(|c| c.index == Some(index)) {
                return at;
            }
            self.calls.push(PartialCall { index: Some(index), ..Default::default() });
            return self.calls.len() - 1;
        }
        let id = tc["id"].as_str().unwrap_or("");
        let name = tc["function"]["name"].as_str().unwrap_or("");
        let new = match self.calls.last() {
            None => true,
            Some(last) => {
                (!id.is_empty() && !last.id.is_empty() && id != last.id)
                    || (!name.is_empty()
                        && !last.name.is_empty()
                        && (last.object.is_some() || serde_json::from_str::<Value>(&last.arguments).is_ok()))
            }
        };
        if new {
            self.calls.push(PartialCall::default());
        }
        self.calls.len() - 1
    }

    /// The stream said it was done ("[DONE]", or a finish reason).
    pub fn complete(&self) -> bool {
        self.done || self.finish.is_some()
    }

    /// "[DONE]" came: nothing more follows.
    pub fn finished(&self) -> bool {
        self.done
    }

    /// Chunks read so far.
    pub fn chunks(&self) -> usize {
        self.chunks
    }

    /// The thinking it sent apart from the answer ("" when none).
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    /// The assistant message, as a non-streamed answer has it: content
    /// (`null` beside tool calls with none), tool calls with their
    /// arguments as a JSON string — each checked to be JSON.
    pub fn message(&self) -> Result<Value, Unusable> {
        let mut calls = Vec::new();
        for (i, c) in self.calls.iter().enumerate() {
            if c.name.is_empty() {
                continue;
            }
            let arguments = match &c.object {
                Some(obj) => obj.to_string(),
                None if c.arguments.trim().is_empty() => "{}".to_string(),
                None => {
                    if serde_json::from_str::<Value>(&c.arguments).is_err() {
                        return Err(Unusable::BadArguments(c.name.clone()));
                    }
                    c.arguments.clone()
                }
            };
            let id = if c.id.is_empty() { format!("call_{}", i + 1) } else { c.id.clone() };
            calls.push(json!({ "id": id, "type": "function", "function": { "name": c.name, "arguments": arguments } }));
        }
        let mut msg = json!({ "role": "assistant" });
        msg["content"] = if self.content.is_empty() && !calls.is_empty() { Value::Null } else { json!(self.content) };
        if !calls.is_empty() {
            msg["tool_calls"] = Value::Array(calls);
        }
        Ok(msg)
    }

    /// The response a non-streamed call would have returned (for usage and
    /// a model evaluation's log), around `message`.
    pub fn response(&self, message: &Value) -> Value {
        let mut v = json!({
            "object": "chat.completion",
            "streamed": true,
            "choices": [{ "index": 0, "message": message, "finish_reason": self.finish }],
        });
        if let Some(m) = &self.model {
            v["model"] = json!(m);
        }
        if self.usage.is_object() {
            v["usage"] = self.usage.clone();
        }
        if self.timings.is_object() {
            v["timings"] = self.timings.clone();
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "You can find a spade very early in the game, in the first area you explore.\nSpecific Locations:\n- The First Area (Caves): look in the caves near the start.\n- The Ruins: the spade is in the ruins area.\n";

    #[test]
    fn an_answer_that_repeats_a_paragraph_stops_after_one_copy() {
        let intro = "Here is what I found.\n";
        // Said once: nothing to cut.
        assert_eq!(repeating_from(&format!("{intro}{BLOCK}")), None);
        // Twice, and part of a third: keep the intro and one copy.
        let looped = format!("{intro}{BLOCK}{BLOCK}{}", &BLOCK[..30]);
        let cut = repeating_from(&looped).expect("a loop");
        assert_eq!(&looped[..cut], format!("{intro}{BLOCK}"));
        // Twelve times over (the 26 Oct answer): the same.
        let many = format!("{intro}{}", BLOCK.repeat(12));
        assert_eq!(&many[..repeating_from(&many).unwrap()], format!("{intro}{BLOCK}"));
    }

    #[test]
    fn a_short_phrase_said_over_and_over_stops_too_but_lists_and_normal_text_do_not() {
        let phrase = "I'm sorry, I can't help. ";
        let looped = format!("Well. {}", phrase.repeat(24));
        let cut = repeating_from(&looped).expect("a loop");
        assert!(looped[..cut].matches("I'm sorry").count() <= 4, "{}", &looped[..cut]);
        // Twice is fine for a short phrase.
        assert_eq!(repeating_from(&format!("Thanks. {phrase}{phrase}")), None);
        // A list whose lines share words but differ.
        let list: String = (1..=30).map(|i| format!("- Step {i}: check the settings page for option {i}.\n")).collect();
        assert_eq!(repeating_from(&list), None);
        // Ordinary prose.
        let prose = "The meeting moved to Thursday. Sam will send the agenda, and the budget stays at the figure we agreed last week. ";
        assert_eq!(repeating_from(prose), None);
        // Non-ASCII text cut on a character boundary.
        let accented = format!("Voilà. {}", "Le café est fermé le lundi, mais ouvert tous les autres jours de la semaine dès huit heures du matin, même les jours fériés. ".repeat(5));
        let cut = repeating_from(&accented).expect("a loop");
        assert!(accented.is_char_boundary(cut));
    }

    #[test]
    fn a_streamed_loop_ends_the_stream_with_one_copy() {
        let mut asm = Assembler::default();
        let chunk = |s: &str| json!({ "choices": [{ "delta": { "content": s } }] }).to_string();
        asm.push(&chunk("Here is what I found.\n")).unwrap();
        // Fed in small pieces, as a server streams it.
        for piece in BLOCK.repeat(5).as_bytes().chunks(9) {
            let s = String::from_utf8_lossy(piece).to_string();
            asm.push(&chunk(&s)).unwrap();
            if asm.finished() {
                break;
            }
        }
        assert!(asm.finished() && asm.complete());
        let msg = asm.message().unwrap();
        assert_eq!(msg["content"].as_str().unwrap(), format!("Here is what I found.\n{BLOCK}"));
        // Anything after it is ignored.
        assert!(asm.push(&chunk("more")).unwrap().is_empty());
    }

    #[tokio::test]
    async fn thinking_sent_apart_is_never_the_answer() {
        // llama.cpp's `reasoning_content`, then the answer; Groq's and
        // OpenRouter's `reasoning`.
        let mut asm = Assembler::default();
        let think = |s: &str| json!({ "choices": [{ "delta": { "reasoning_content": s } }] }).to_string();
        let say = |s: &str| json!({ "choices": [{ "delta": { "content": s } }] }).to_string();
        assert_eq!(asm.push(&think("The ball is x. ")).unwrap(), vec![Piece::Reasoning("The ball is x. ".into())]);
        asm.push(&think("x + x + 1 = 1.10")).unwrap();
        asm.push(&say("The ball costs 5 cents.")).unwrap();
        asm.push(&json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()).unwrap();
        let msg = asm.message().unwrap();
        assert_eq!(msg["content"], "The ball costs 5 cents.");
        assert!(msg.get("reasoning_content").is_none());
        assert_eq!(asm.reasoning(), "The ball is x. x + x + 1 = 1.10");
        let mut groq = Assembler::default();
        let pieces = groq.push(&json!({ "choices": [{ "delta": { "reasoning": "Hmm." } }] }).to_string()).unwrap();
        assert_eq!(pieces, vec![Piece::Reasoning("Hmm.".into())]);

        // A step of a question asked to think it through shows it as
        // thinking, then the answer — and a thinking budget's end tag,
        // should it slip in, never.
        let log = Log::default();
        crate::chat::think::with(true, async {
            let mut s = Step::new(&log, false, false);
            s.on(Delta::Reasoning("First, "));
            s.on(Delta::Reasoning("the ball."));
            s.on(Delta::Content("</think>\n\nThe ball costs 5 cents."));
            s.end(true);
        })
        .await;
        let events = log.0.lock().unwrap().clone();
        let thinking: Vec<&str> = events.iter().filter_map(|e| if let Event::Thinking { text } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(thinking.last(), Some(&"First, the ball."));
        assert_eq!(log.texts().last().map(String::as_str), Some("The ball costs 5 cents."));
        let first_text = events.iter().position(|e| matches!(e, Event::Text { .. })).unwrap();
        let last_thought = events.iter().rposition(|e| matches!(e, Event::Thinking { .. })).unwrap();
        assert!(last_thought < first_text, "the thinking, whole, before the answer");
        assert!(events.iter().any(|e| matches!(e, Event::Thinking { .. })));
        // Any other question: a model that sends its reasoning anyway (Groq's
        // gpt-oss does, unasked) shows none of it — only the answer.
        let plain = Log::default();
        let mut s = Step::new(&plain, false, false);
        s.on(Delta::Reasoning("The user wants the weather."));
        s.on(Delta::Content("It's sunny."));
        s.end(true);
        let events = plain.0.lock().unwrap().clone();
        assert!(!events.iter().any(|e| matches!(e, Event::Thinking { .. })), "{events:?}");
        assert_eq!(plain.texts().last().map(String::as_str), Some("It's sunny."));
        // On the page's wire.
        assert_eq!(serde_json::to_value(Event::Thinking { text: "x".into() }).unwrap(), json!({ "kind": "thinking", "text": "x" }));
    }

    /// A question asked to think it through, answered whole by a server
    /// asked to stream (one that won't, or a broken stream asked again):
    /// its thinking shows once, not twice.
    #[tokio::test]
    async fn a_whole_answers_thinking_shows_once() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a local port");
        let port = server.server_addr().to_ip().expect("ip").port();
        std::thread::spawn(move || {
            if let Ok(mut req) = server.recv() {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                let message = json!({ "role": "assistant", "content": "The ball costs 5p.", "reasoning_content": "The bat costs a pound more." });
                let reply = json!({ "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }] });
                let header = tiny_http::Header::from_bytes("content-type", "application/json").unwrap();
                let _ = req.respond(tiny_http::Response::from_string(reply.to_string()).with_header(header));
            }
        });
        let base = format!("http://127.0.0.1:{port}/v1");
        let log = Log::default();
        let (out, thought) = crate::chat::think::with(true, async {
            let question = json!([{ "role": "user", "content": "How much is the ball?" }]);
            let req = crate::ai::llm::ChatRequest::chat(&base, "", "openai/gpt-oss-20b", "groq", 0.5, question, None, None);
            assert_eq!(req.extra["reasoning_effort"], "high");
            call(Live { sink: &log, beside: None }, &req, false, false).await
        })
        .await;
        assert_eq!(out.unwrap()["content"], "The ball costs 5p.");
        let shown: Vec<String> =
            log.0.lock().unwrap().iter().filter_map(|e| if let Event::Thinking { text } = e { Some(text.clone()) } else { None }).collect();
        assert_eq!(shown, ["The bat costs a pound more."]);
        assert_eq!(log.texts().last().map(String::as_str), Some("The ball costs 5p."));
        assert_eq!(thought.unwrap().text, "The bat costs a pound more.");
    }

    /// Every event a sink got.
    #[derive(Default)]
    struct Log(Mutex<Vec<Event>>);

    impl Sink for Log {
        fn send(&self, ev: Event) {
            self.0.lock().unwrap().push(ev);
        }
    }

    impl Log {
        fn texts(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| match e {
                    Event::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    #[test]
    fn sse_lines_split_anywhere_come_back_whole() {
        let wire = "data: {\"a\":1}\n\n: keep-alive\r\n\r\ndata: {\"b\":\"é\"}\n\ndata: [DONE]\n\n";
        let bytes = wire.as_bytes();
        // Every split point, even inside the two bytes of "é".
        for cut in 0..bytes.len() {
            let mut p = SseParser::default();
            let mut got = p.feed(&bytes[..cut]);
            got.extend(p.feed(&bytes[cut..]));
            got.extend(p.finish());
            assert_eq!(got, ["{\"a\":1}", "{\"b\":\"é\"}", "[DONE]"], "cut at {cut}");
        }
        // Multi-line data, no space after the colon, an event name, a bare JSON line.
        let mut p = SseParser::default();
        let mut got = p.feed(b"event: message\ndata:line one\ndata: line two\n\n{\"bare\":true}\ndata: {\"tail\":1}");
        got.extend(p.finish());
        assert_eq!(got, ["line one\nline two", "{\"bare\":true}", "{\"tail\":1}"]);
    }

    fn chunk(delta: Value) -> String {
        json!({ "object": "chat.completion.chunk", "choices": [{ "index": 0, "delta": delta, "finish_reason": null }] }).to_string()
    }

    #[test]
    fn streamed_text_and_usage_make_the_whole_message() {
        let mut a = Assembler::default();
        assert_eq!(a.push(&chunk(json!({ "role": "assistant", "content": "" }))).unwrap(), []);
        assert_eq!(a.push(&chunk(json!({ "content": "Hello" }))).unwrap(), [Piece::Content("Hello".into())]);
        a.push(&chunk(json!({ "content": " there." }))).unwrap();
        a.push(&json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }], "timings": { "prompt_ms": 100.0 } }).to_string()).unwrap();
        a.push(&json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13 } }).to_string()).unwrap();
        assert!(a.complete() && !a.finished());
        a.push("[DONE]").unwrap();
        assert!(a.complete());
        let msg = a.message().unwrap();
        assert_eq!(msg, json!({ "role": "assistant", "content": "Hello there." }));
        let whole = a.response(&msg);
        assert_eq!(whole["usage"]["total_tokens"], 13);
        assert_eq!(whole["timings"]["prompt_ms"], 100.0);
        assert_eq!(whole["choices"][0]["finish_reason"], "stop");
        // Groq's usage, in x_groq.
        let mut g = Assembler::default();
        g.push(&json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }], "x_groq": { "usage": { "total_tokens": 42 } } }).to_string()).unwrap();
        assert_eq!(g.response(&g.message().unwrap())["usage"]["total_tokens"], 42);
    }

    #[test]
    fn a_tool_call_in_fragments_comes_back_whole_and_checked() {
        // llama.cpp / OpenAI: a header (index, id, name), then the arguments in pieces.
        let mut a = Assembler::default();
        let header = a
            .push(&chunk(json!({ "tool_calls": [{ "index": 0, "id": "call_a", "type": "function", "function": { "name": "search_notes", "arguments": "" } }] })))
            .unwrap();
        assert_eq!(header, [Piece::Call("search_notes".into())]);
        for frag in ["{\"que", "ry\": \"bu", "dget\"}"] {
            assert_eq!(a.push(&chunk(json!({ "tool_calls": [{ "index": 0, "function": { "arguments": frag } }] }))).unwrap(), []);
        }
        // A second call at index 1.
        a.push(&chunk(json!({ "tool_calls": [{ "index": 1, "id": "call_b", "function": { "name": "list_folders", "arguments": "{}" } }] }))).unwrap();
        let msg = a.message().unwrap();
        assert!(msg["content"].is_null());
        let calls = msg["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "call_a");
        assert_eq!(calls[0]["function"]["name"], "search_notes");
        assert_eq!(serde_json::from_str::<Value>(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap(), json!({ "query": "budget" }));
        assert_eq!(calls[1]["function"]["name"], "list_folders");

        // Cut off mid-string: not JSON, so the model is asked again.
        let mut cut = Assembler::default();
        cut.push(&chunk(json!({ "tool_calls": [{ "index": 0, "id": "c", "function": { "name": "web_search", "arguments": "{\"query\": \"arse" } }] }))).unwrap();
        assert_eq!(cut.message(), Err(Unusable::BadArguments("web_search".into())));
    }

    #[test]
    fn gemini_style_whole_calls_without_an_index_stay_apart() {
        let mut a = Assembler::default();
        a.push(&chunk(json!({ "content": "Let me check." }))).unwrap();
        a.push(&chunk(json!({ "tool_calls": [{ "id": "", "type": "function", "function": { "name": "web_search", "arguments": "{\"query\":\"a\"}" } }] }))).unwrap();
        a.push(&chunk(json!({ "tool_calls": [{ "id": "", "type": "function", "function": { "name": "search_notes", "arguments": "{\"query\":\"b\"}" } }] }))).unwrap();
        // An object for arguments (some servers).
        a.push(&chunk(json!({ "tool_calls": [{ "id": "x9", "function": { "name": "get_note", "arguments": { "note_id": 3 } } }] }))).unwrap();
        let msg = a.message().unwrap();
        assert_eq!(msg["content"], "Let me check.");
        let names: Vec<&str> = msg["tool_calls"].as_array().unwrap().iter().map(|c| c["function"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["web_search", "search_notes", "get_note"]);
        assert_eq!(msg["tool_calls"][2]["function"]["arguments"], "{\"note_id\":3}");
        assert_eq!(msg["tool_calls"][0]["id"], "call_1");
    }

    #[test]
    fn an_error_mid_stream_is_handed_back_whole() {
        let mut a = Assembler::default();
        a.push(&chunk(json!({ "content": "Half an ans" }))).unwrap();
        let err = r#"{"error":{"message":"Tool call validation failed: missing note_id","code":"tool_use_failed","failed_generation":"{}"}}"#;
        assert_eq!(a.push(err), Err(err.to_string()));
        // OpenRouter's: an error beside a chunk.
        let or = r#"{"id":"x","object":"chat.completion.chunk","error":{"code":502,"message":"Provider disconnected"},"choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]}"#;
        assert!(Assembler::default().push(or).is_err());
        // Junk is skipped.
        assert_eq!(Assembler::default().push("not json"), Ok(Vec::new()));
    }

    #[test]
    fn opening_text_waits_for_a_sentence_and_more_when_tools_are_offered() {
        // No tools: at once.
        assert!(releases("Sure", false));
        assert!(!releases("   ", false));
        // Tools: a line announcing a call never shows…
        assert!(!releases("I'll search your notes for that.", true));
        assert!(!releases("I'll search your notes for that. ", true));
        // …a sentence with more after it does, as does a line break or a list.
        assert!(releases("The budget is £15,000. Sam", true));
        assert!(releases("Here's what I found:\n- one", true));
        assert!(!releases("Two things:\n", true));
        assert!(releases(&"word ".repeat(50), true));
        // An abbreviation's full stop counts too: a sentence-ish is enough.
        assert!(releases("Version 3.12 is out. It", true));
        assert!(!releases("Version 3.12 is", true));
    }

    #[test]
    fn reasoning_never_shows_even_half_written() {
        assert_eq!(without_thinking("<think>let me see"), "");
        assert_eq!(without_thinking("<think>a</think>The answer"), "The answer");
        assert_eq!(without_thinking("The answer <thi"), "The answer");
        assert_eq!(without_thinking("Fine <"), "Fine");
        assert_eq!(without_thinking("x < y"), "x < y");
    }

    #[test]
    fn a_step_shows_the_answer_and_never_a_line_before_a_tool_call() {
        // A preamble, then a call: nothing shown but the chip.
        let log = Log::default();
        let mut s = Step::new(&log, true, false);
        for piece in ["I'll ", "search your notes", " for that."] {
            s.on(Delta::Content(piece));
        }
        s.on(Delta::ToolCall("search_notes"));
        s.end(false);
        assert!(log.texts().is_empty(), "{:?}", log.texts());
        assert_eq!(
            log.0.lock().unwrap().last(),
            Some(&Event::Tool { name: "search_notes".into(), doing: "Searching your notes…".into() })
        );

        // An answer: shown once it's a sentence and more, then whole at the end.
        let log = Log::default();
        let mut s = Step::new(&log, true, false);
        for piece in ["The budget", " is £15,000.", " Sam checks", " the quotes."] {
            s.on(Delta::Content(piece));
        }
        s.end(true);
        let texts = log.texts();
        assert_eq!(texts.first().map(String::as_str), Some("The budget is £15,000. Sam checks"));
        assert_eq!(texts.last().map(String::as_str), Some("The budget is £15,000. Sam checks the quotes."));

        // Shown, then a call after all: taken back.
        let log = Log::default();
        let mut s = Step::new(&log, true, false);
        s.on(Delta::Content("Here is a start. And more"));
        s.on(Delta::ToolCall("web_search"));
        assert_eq!(log.texts(), ["Here is a start. And more", ""]);

        // A short answer with no tools offered: shown at once; gpt-oss's
        // citations as Yap's.
        let log = Log::default();
        let mut s = Step::new(&log, false, false);
        s.on(Delta::Content("Yes【1】"));
        s.end(true);
        assert_eq!(log.texts(), ["Yes [1]"]);

        // A whole answer under the release length still shows at the end.
        let log = Log::default();
        let mut s = Step::new(&log, true, false);
        s.on(Delta::Content("Friday."));
        assert!(log.texts().is_empty());
        s.end(true);
        assert_eq!(log.texts(), ["Friday."]);
    }

    #[test]
    fn a_restarted_step_replaces_what_it_showed() {
        let log = Log::default();
        let mut s = Step::new(&log, false, false);
        s.on(Delta::Content("First try"));
        s.on(Delta::Restart);
        s.on(Delta::Content("Second"));
        s.end(true);
        assert_eq!(log.texts(), ["First try", "Second"]);
        // A redo that turns into a tool call takes the old text back.
        let log = Log::default();
        let mut s = Step::new(&log, false, false);
        s.on(Delta::Content("Old answer"));
        s.on(Delta::Restart);
        s.on(Delta::ToolCall("web_search"));
        assert_eq!(log.texts(), ["Old answer", ""]);
    }

    #[test]
    fn a_watch_keeps_what_was_shown_and_when_the_first_words_came() {
        let log = Log::default();
        let w = Watch::new(&log);
        w.send(Event::Text { text: String::new() });
        assert!(w.seen().first_words.is_none());
        w.send(Event::Chips { chips: vec!["Searched the web".into()], sources: Vec::new(), web: true });
        w.send(Event::Text { text: "Partial".into() });
        let seen = w.seen();
        assert_eq!(seen.text, "Partial");
        assert_eq!(seen.chips, ["Searched the web"]);
        assert!(seen.web && seen.first_words.is_some());
        assert_eq!(log.texts(), ["", "Partial"]);
    }

    #[test]
    fn a_stop_reaches_its_stream_and_nothing_else() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async {
            let s = Stoppable::new(Some("turn-1"));
            assert!(!stop("turn-2", Halt::Stopped));
            assert!(stop("turn-1", Halt::Stopped));
            // The permit waits for the watcher.
            let why = tokio::time::timeout(Duration::from_secs(1), s.halted()).await.expect("stopped");
            assert_eq!(why, Halt::Stopped);
            drop(s);
            assert!(!stop("turn-1", Halt::Stopped), "gone once dropped");
            // No id: never stopped.
            let none = Stoppable::new(None);
            assert!(tokio::time::timeout(Duration::from_millis(30), none.halted()).await.is_err());
            // The page's "a newer question replaces it" (a dictated question
            // sent by itself while this one is answered).
            let s = Stoppable::new(Some("turn-3"));
            assert!(stop("turn-3", Halt::Superseded));
            assert_eq!(unless_stopped(std::future::pending::<()>(), Some(&s)).await, Err(Halt::Superseded));
        });
    }

    // A tester's voice chat (2026-10-23): a spoken question was still being
    // answered when a barge-in asked a newer one in the same conversation;
    // the older was asked again after the newer's answer, and both answers
    // were saved under both questions. Saving the newer question ends it.
    #[test]
    fn a_newer_question_in_the_same_conversation_supersedes_the_older_turn() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        rt.block_on(async {
            // Conversation ids of their own: other tests run beside this one.
            let (talk, other) = (90_001, 90_002);
            let older = Stoppable::new(None);
            older.answers(talk);
            let elsewhere = Stoppable::new(Some("turn-elsewhere"));
            elsewhere.answers(other);
            // The newer question is saved in the same conversation: the
            // older ends (while it waits out a dictation, or asks again).
            let newer = Stoppable::new(None);
            newer.answers(talk);
            let waiting = std::future::pending::<&str>();
            assert_eq!(unless_stopped(waiting, Some(&older)).await, Err(Halt::Superseded));
            // The newer one and another conversation's turn go on.
            let done = unless_stopped(async { "answered" }, Some(&newer)).await;
            assert_eq!(done, Ok("answered"));
            assert!(tokio::time::timeout(Duration::from_millis(30), newer.halted()).await.is_err());
            assert!(tokio::time::timeout(Duration::from_millis(30), elsewhere.halted()).await.is_err());
            // The older one gone: the newer is still the one answering, and
            // the next question supersedes it in turn.
            drop(older);
            let next = Stoppable::new(None);
            next.answers(talk);
            assert_eq!(tokio::time::timeout(Duration::from_secs(1), newer.halted()).await, Ok(Halt::Superseded));
            // A turn that ended leaves nothing behind: a question after it
            // supersedes nobody.
            drop(next);
            drop(newer);
            let after = Stoppable::new(None);
            after.answers(talk);
            assert!(tokio::time::timeout(Duration::from_millis(30), after.halted()).await.is_err());
            // ■ first: it stays a stop, whatever comes after.
            let third = 90_003;
            let stopped = Stoppable::new(Some("turn-stopped"));
            stopped.answers(third);
            assert!(stop("turn-stopped", Halt::Stopped));
            Stoppable::new(None).answers(third);
            assert_eq!(stopped.halted().await, Halt::Stopped);
        });
    }
}
