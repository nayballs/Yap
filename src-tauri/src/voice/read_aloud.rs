//! **Read aloud, anywhere**: select text in any app — Word, Notepad, a web
//! page, a PDF, a chat — and press the Read aloud shortcut (or 🔊 on the Yap
//! bar, or the tray's "Read selection aloud"), and Yap reads it in the voice
//! Settings picked, with a **reading card** on the Yap bar: the sentence
//! being read with its current word lit up (karaoke), ⏮ ⏯ ⏭, the speed,
//! Stop, Esc. In Yap itself, notes and Chat's answers have Read aloud too
//! (the same card).
//!
//! - **The text**: the selection, read over UI Automation
//!   (`selection::read_selected`: TextPattern, read-only text too; a Win32
//!   edit control's own selection; Ctrl+C last, and never in a terminal,
//!   where Ctrl+C with nothing selected stops the program). A note's or an
//!   answer's markdown is read as spoken replies read it
//!   (`tts::speakable_blocks`); plain text by its paragraphs, a list item or a
//!   short line its own, a paragraph's wrapped lines joined ([`plain_blocks`]).
//!   Up to [`MAX_CHARS`] (a book's worth): the rest is left, and the card says
//!   so.
//! - **The voice**: `tts::read` — the speaker one sentence ahead of the voice
//!   (never all of it at once), the volume and speed of spoken replies, its
//!   stop; a dictation or a meeting stops it, as it does any of Yap's voice.
//! - **The karaoke**: every sentence made comes with when its words start
//!   (`tts/marks.rs`: Windows voices' word cues, ElevenLabs' character
//!   timing, else from Kokoro's audio); a ticker here follows the voice
//!   (`tts::position`, the samples the output has taken) and says which word
//!   is being said (`yap-read-word`), the card its sentence (`yap-read`).
//!
//! Nothing read is ever logged above `debug` — lengths, counts and how it was
//! read only. Reading with ElevenLabs sends the text there, as spoken replies
//! do (Settings → Voice Assistant says so).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::voice::tts::{self, marks};

/// The longest text read in one go (~200,000 characters, a short book): the
/// rest is left, and the card says so.
pub const MAX_CHARS: usize = 200_000;
/// The card's snapshot, on every change of sentence or state.
const EVENT: &str = "yap-read";
/// The word being said, as it changes.
const EVENT_WORD: &str = "yap-read-word";
/// How often the voice's place is looked at.
const TICK: Duration = Duration::from_millis(40);
/// A note ("Nothing selected") stays this long.
const NOTE_MS: u64 = 4_500;
/// The speeds ⊖ / ⊕ step through.
pub const SPEEDS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
/// ⏮ goes back to the start of the sentence once it's this far in, else to
/// the one before (a music player's ⏮).
const BACK_RESTARTS_AFTER: Duration = Duration::from_millis(1_500);

/// Where a read-aloud came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Selected text in another app (the shortcut, the bar, the tray).
    Selection,
    /// A Chat answer (and the Notes view's and notepad's Ask answers).
    Chat,
    /// A note.
    Note,
    /// Text selected in one of Yap's own windows.
    Yap,
}

impl Source {
    /// Why it can't read is said on the reading card (a selection: nothing
    /// else shows it), not by the page (a note's, an answer's: a toast).
    fn says_on_card(self) -> bool {
        matches!(self, Source::Selection | Source::Yap)
    }

    fn from_name(name: &str) -> Source {
        match name {
            "chat" | "answer" | "notepad" => Source::Chat,
            "note" => Source::Note,
            "selection" => Source::Selection,
            _ => Source::Yap,
        }
    }
}

/// How the read was started, for a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The Read aloud shortcut (the app you're in is in front).
    Shortcut,
    /// 🔊 on the Yap bar (never focused: the app is still in front).
    Bar,
    /// The tray's "Read selection aloud" (the tray menu had the front: the
    /// app window on top is the one).
    Tray,
}

/// One piece of the sentence on the card: a word (`w`: its index) or what's
/// between words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Part {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w: Option<usize>,
}

/// A short message on the card instead of a reading ("Nothing selected").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Note {
    /// "empty" (nothing selected) | "quiet" (a meeting or a call) | "error".
    pub kind: &'static str,
    pub title: String,
    pub body: String,
}

/// The reading card (`yap-read`, `read_aloud_state`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The card is up.
    pub active: bool,
    pub session: u64,
    /// "reading" | "paused" | "note" | "idle".
    pub phase: &'static str,
    /// What the pages call it (an answer's id, so its ▶ shows ■).
    pub message_id: Option<String>,
    pub source: Option<Source>,
    /// The sentence being read, of `count`.
    pub index: usize,
    pub count: usize,
    pub parts: Vec<Part>,
    pub word: Option<usize>,
    pub rate: f32,
    /// How the sentence's word times were found (`marks::Timing`), once it's made.
    pub timing: Option<marks::Timing>,
    /// The engine that made it ("kokoro" | "windows" | "elevenlabs").
    pub engine: Option<&'static str>,
    pub note: Option<Note>,
    /// The text was longer than [`MAX_CHARS`]: only its start is read.
    pub cut: bool,
    /// Sentences made so far (a sentence ahead of the voice, never all).
    pub made: usize,
    /// The Read aloud shortcut's spec ("" unbound), for the card's words.
    pub hotkey: String,
}

struct Session {
    id: u64,
    message_id: String,
    source: Source,
    track: Arc<tts::Track>,
    index: usize,
    word: Option<usize>,
    paused: bool,
    rate: f32,
    cut: bool,
    /// The voice is being restarted (⏭, ⏮, a speed): the old voice's end
    /// isn't this reading's.
    restarting: bool,
}

#[derive(Default)]
struct State {
    session: Option<Session>,
    /// A note on the card, with its id (a newer one replaces it).
    note: Option<(u64, Note)>,
}

static STATE: Mutex<State> = Mutex::new(State { session: None, note: None });
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Never held while emitting, speaking or calling another module.
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

// ---- the text (pure) -------------------------------------------------------------------

/// A list item's marker ("- ", "• ", "1. ", "a) ") at the start of `line`:
/// the rest of the line after it.
fn list_item(line: &str) -> Option<&str> {
    let t = line.trim_start();
    for m in ["- ", "* ", "• ", "– ", "◦ ", "▪ ", "‣ ", "· "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(rest.trim_start());
        }
    }
    let marker: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let rest = &t[marker.len()..];
    let numbered = !marker.is_empty()
        && (marker.chars().all(|c| c.is_ascii_digit()) && marker.len() <= 3
            || marker.len() == 1 && marker.chars().all(|c| c.is_ascii_lowercase()));
    if numbered && (rest.starts_with(". ") || rest.starts_with(") ")) {
        return Some(rest[2..].trim_start());
    }
    None
}

/// A line under this many characters ends its block (a heading, a title, a
/// paragraph's last line): a longer one is a paragraph wrapped by its app.
const SHORT_LINE: usize = 40;

/// Plain text (a selection) as blocks to read: paragraphs (a blank line
/// between), a list item or a short line each its own, a paragraph's lines
/// wrapped by its app (a PDF, an email) joined again — a word hyphenated
/// across a line break whole again. List markers out. Pure.
pub fn plain_blocks(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n").replace(['\r', '\u{2028}', '\u{2029}', '\u{000B}', '\u{000C}'], "\n");
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    // The last line added to `cur`, how long it was.
    let mut last_len = 0usize;
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let block = cur.split_whitespace().collect::<Vec<_>>().join(" ");
        if !block.is_empty() {
            out.push(block);
        }
        cur.clear();
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut cur, &mut out);
            continue;
        }
        let (line, item) = match list_item(line) {
            Some(rest) => (rest, true),
            None => (line, false),
        };
        if item || cur.is_empty() || last_len < SHORT_LINE {
            flush(&mut cur, &mut out);
            cur.push_str(line);
        } else if cur.ends_with('-')
            && cur.chars().rev().nth(1).is_some_and(char::is_alphabetic)
            && line.chars().next().is_some_and(char::is_lowercase)
        {
            cur.pop();
            cur.push_str(line);
        } else {
            cur.push(' ');
            cur.push_str(line);
        }
        last_len = raw.trim().chars().count();
    }
    flush(&mut cur, &mut out);
    out
}

/// The sentences to read for `text` (markdown for a note or an answer,
/// plain for a selection), and whether it was cut at [`MAX_CHARS`]. A
/// "sentence" with no letter or digit in it isn't read. Pure.
pub fn sentences_for(text: &str, markdown: bool) -> (Vec<String>, bool) {
    let cut = text.chars().count() > MAX_CHARS;
    let text: String = if cut { text.chars().take(MAX_CHARS).collect() } else { text.to_string() };
    let blocks = if markdown { tts::speakable_blocks(&text) } else { plain_blocks(&text) };
    let sentences = blocks
        .iter()
        .flat_map(|b| tts::sentences(b))
        .filter(|s| s.chars().any(char::is_alphanumeric))
        .collect();
    (sentences, cut)
}

/// A sentence as the card draws it: its words (numbered) and what's
/// between them. Pure.
pub fn parts(sentence: &str) -> Vec<Part> {
    let chars: Vec<char> = sentence.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    for (w, (a, b)) in marks::words(sentence).into_iter().enumerate() {
        if a > at {
            out.push(Part { text: chars[at..a].iter().collect(), w: None });
        }
        out.push(Part { text: chars[a..b].iter().collect(), w: Some(w) });
        at = b;
    }
    if at < chars.len() {
        out.push(Part { text: chars[at..].iter().collect(), w: None });
    }
    out
}

/// The speed one step faster (`up`) or slower than `rate`. Pure.
pub fn step_speed(rate: f32, up: bool) -> f32 {
    let rate = if rate.is_finite() { rate } else { 1.0 };
    if up {
        SPEEDS.iter().copied().find(|&s| s > rate + 0.01).unwrap_or(SPEEDS[SPEEDS.len() - 1])
    } else {
        SPEEDS.iter().rev().copied().find(|&s| s < rate - 0.01).unwrap_or(SPEEDS[0])
    }
}

/// Where ⏮ goes from sentence `index`, `into` it: its start once it's well
/// under way, else the one before (the first stays the first). Pure.
pub fn back_to(index: usize, into: Duration) -> usize {
    if into > BACK_RESTARTS_AFTER {
        index
    } else {
        index.saturating_sub(1)
    }
}

// ---- the card -------------------------------------------------------------------------

fn hotkey() -> String {
    crate::config::load().read_hotkey
}

fn view_of(s: &State, hotkey: String) -> View {
    let mut v = View {
        active: false,
        session: 0,
        phase: "idle",
        message_id: None,
        source: None,
        index: 0,
        count: 0,
        parts: Vec::new(),
        word: None,
        rate: 1.0,
        timing: None,
        engine: None,
        note: None,
        cut: false,
        made: 0,
        hotkey,
    };
    if let Some(r) = &s.session {
        let made = r.track.made(r.index);
        v.active = true;
        v.session = r.id;
        v.phase = if r.paused { "paused" } else { "reading" };
        v.message_id = Some(r.message_id.clone());
        v.source = Some(r.source);
        v.index = r.index;
        v.count = r.track.len();
        v.parts = r.track.shown(r.index).map(parts).unwrap_or_default();
        v.word = r.word;
        v.rate = r.rate;
        v.timing = made.as_ref().map(|m| m.timing);
        v.engine = made.as_ref().map(|m| m.engine);
        v.cut = r.cut;
        v.made = r.track.made_count();
    } else if let Some((id, note)) = &s.note {
        v.active = true;
        v.session = *id;
        v.phase = "note";
        v.note = Some(note.clone());
    }
    v
}

/// The card now.
pub fn view() -> View {
    let hotkey = hotkey();
    view_of(&lock(), hotkey)
}

/// Is a read-aloud's card up (a reading, or its note)?
pub fn active() -> bool {
    let s = lock();
    s.session.is_some() || s.note.is_some()
}

/// Whether the card was up when last said (the tray's item follows it).
static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Tell the pages, and keep the bar up while the card is.
fn publish(app: &AppHandle) {
    let v = view();
    crate::bar::set_reading(app, v.active);
    let _ = app.emit(EVENT, &v);
    if SHOWN.swap(v.active, Ordering::SeqCst) != v.active {
        crate::tray::refresh(app);
    }
}

/// A note on the card for a few seconds ("Nothing selected", "Yap doesn't
/// read aloud during a call").
fn note(app: &AppHandle, kind: &'static str, title: &str, body: &str) {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    {
        let mut s = lock();
        if s.session.is_some() {
            return;
        }
        s.note = Some((id, Note { kind, title: title.into(), body: body.into() }));
    }
    tracing::info!(kind, "Read aloud: a note on the card");
    publish(app);
    let app = app.clone();
    let _ = std::thread::Builder::new().name("read-aloud-note".into()).spawn(move || {
        std::thread::sleep(Duration::from_millis(NOTE_MS));
        let gone = {
            let mut s = lock();
            match &s.note {
                Some((n, _)) if *n == id => {
                    s.note = None;
                    true
                }
                _ => false,
            }
        };
        if gone {
            publish(&app);
        }
    });
}

/// "Nothing selected": how to read something. `{key}` is the shortcut,
/// named by the card (`View::hotkey`, as the coaching cards' `{key}`).
fn nothing_selected(app: &AppHandle) {
    let how = if hotkey().trim().is_empty() {
        "Select some text in any app, then click 🔊 on the Yap bar."
    } else {
        "Select some text in any app, then press {key} or click 🔊 on the Yap bar."
    };
    note(app, "empty", "Nothing selected", how);
}

// ---- reading ----------------------------------------------------------------------------

/// What to read, and how.
pub struct Start {
    pub text: String,
    pub markdown: bool,
    pub message_id: Option<String>,
    pub source: Source,
}

/// **Read `start.text` aloud**: the card comes up on the Yap bar, the voice
/// starts with the first sentence. Whatever Yap was saying stops. Returns
/// the reading's message id. Nothing to read, or a meeting or a call on:
/// a note on the card, and why.
pub fn start(app: &AppHandle, start: Start) -> Result<String, String> {
    // A note or an answer's Read aloud says why on its page (a toast); a
    // selection, here on the card.
    let on_card = start.source.says_on_card();
    if start.text.trim().is_empty() {
        if on_card {
            nothing_selected(app);
        }
        return Err("Nothing to read aloud".into());
    }
    if let Some(why) = tts::meeting_quiet() {
        if on_card {
            note(app, "quiet", "Not now", &format!("{why}. Read aloud is back once it's over."));
        }
        return Err(why.into());
    }
    let (sentences, cut) = sentences_for(&start.text, start.markdown);
    if sentences.is_empty() {
        if on_card {
            note(app, "empty", "Nothing to read", "There are no words in that to read aloud.");
        }
        return Err("Nothing to read aloud".into());
    }
    let chars = start.text.chars().count().min(MAX_CHARS);
    let track = tts::Track::new(tts::prepare_for_reading(sentences));
    let rate = crate::config::load().tts_rate;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let message_id = start.message_id.clone().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| format!("read:{id}"));
    {
        let mut s = lock();
        s.note = None;
        s.session = Some(Session {
            id,
            message_id: message_id.clone(),
            source: start.source,
            track: Arc::clone(&track),
            index: 0,
            word: None,
            paused: false,
            rate,
            cut,
            restarting: true,
        });
    }
    let started = tts::read(Arc::clone(&track), 0, message_id.clone(), rate);
    if let Err(e) = started {
        let mine = {
            let mut s = lock();
            let mine = s.session.as_ref().is_some_and(|r| r.id == id);
            if mine {
                s.session = None;
            }
            mine
        };
        tracing::info!("Read aloud: couldn't start ({e})");
        if mine && on_card {
            note(app, "error", "Couldn't read it aloud", &e);
        } else if mine {
            publish(app);
        }
        return Err(e);
    }
    settled(app, id);
    tracing::info!(chars, sentences = track.len(), cut, source = ?start.source, "Read aloud: reading");
    publish(app);
    let ticker = app.clone();
    let _ = std::thread::Builder::new().name("read-aloud".into()).spawn(move || follow(&ticker, id));
    Ok(message_id)
}

/// A start or restart is done: the old voice's end can't be mistaken for
/// this one's any more. If the voice has already ended (a very short text),
/// the reading ends too.
fn settled(app: &AppHandle, id: u64) {
    let over = {
        let mut s = lock();
        match s.session.as_mut() {
            Some(r) if r.id == id => {
                r.restarting = false;
                tts::speaking_id().as_deref() != Some(r.message_id.as_str())
            }
            _ => false,
        }
    };
    if over {
        end(app, id);
    }
}

/// The reading `id` is over: the card goes.
fn end(app: &AppHandle, id: u64) {
    let ended = {
        let mut s = lock();
        let mine = s.session.as_ref().is_some_and(|r| r.id == id);
        if mine {
            s.session = None;
        }
        mine
    };
    if ended {
        tracing::info!("Read aloud: done");
        publish(app);
    }
}

/// Follow the voice for the reading `id`: which sentence, which word.
fn follow(app: &AppHandle, id: u64) {
    loop {
        std::thread::sleep(TICK);
        let (track, index, word, paused) = {
            let s = lock();
            match &s.session {
                Some(r) if r.id == id => (Arc::clone(&r.track), r.index, r.word, r.paused),
                _ => return,
            }
        };
        if paused {
            continue;
        }
        let Some((at, into)) = tts::position() else { continue };
        if at >= track.len() {
            continue;
        }
        let now = track.made(at).and_then(|m| marks::word_at(&m.starts, into));
        let (new_sentence, new_word) = {
            let mut s = lock();
            let Some(r) = s.session.as_mut().filter(|r| r.id == id) else { return };
            if r.restarting {
                continue;
            }
            let new_sentence = at != index;
            let new_word = now != word || new_sentence;
            r.index = at;
            r.word = now;
            (new_sentence, new_word)
        };
        if new_sentence {
            publish(app);
        } else if new_word {
            let _ = app.emit(EVENT_WORD, serde_json::json!({ "session": id, "index": at, "word": now }));
        }
    }
}

/// Start the voice again from sentence `from` (⏭, ⏮, a new speed), playing.
fn restart(app: &AppHandle, from: usize, rate: Option<f32>) -> Result<(), String> {
    let (id, track, message_id, rate) = {
        let mut s = lock();
        let r = s.session.as_mut().ok_or("Nothing is being read")?;
        r.restarting = true;
        r.index = from.min(r.track.len().saturating_sub(1));
        r.word = None;
        r.paused = false;
        if let Some(rate) = rate {
            r.rate = rate;
        }
        (r.id, Arc::clone(&r.track), r.message_id.clone(), r.rate)
    };
    if track.is_empty() {
        end(app, id);
        return Err("Nothing to read aloud".into());
    }
    let from = from.min(track.len() - 1);
    let started = tts::read(track, from, message_id, rate);
    if let Err(e) = started {
        end(app, id);
        return Err(e);
    }
    settled(app, id);
    publish(app);
    Ok(())
}

/// Stop reading (■, Esc, ✕, the shortcut again): the voice stops, the card
/// goes. A note on the card goes too.
pub fn stop(app: &AppHandle) {
    let (had, note) = {
        let mut s = lock();
        (s.session.take().is_some(), s.note.take().is_some())
    };
    if had {
        tts::stop();
        tracing::info!("Read aloud: stopped");
    }
    if had || note {
        publish(app);
    }
}

/// The voice changed (`tts.rs`): a reading whose voice ended — read to
/// the end, stopped elsewhere (Chat's ■, a dictation, a meeting) or failed —
/// is over; a failure is said on the card.
pub fn on_tts_state(app: &AppHandle, state: &tts::TtsState) {
    if state.speaking {
        return;
    }
    let ended = {
        let mut s = lock();
        match &s.session {
            Some(r) if !r.restarting && state.message_id.as_deref() == Some(r.message_id.as_str()) => {
                let on_card = r.source.says_on_card();
                s.session = None;
                Some(on_card)
            }
            _ => None,
        }
    };
    let Some(on_card) = ended else { return };
    match &state.error {
        // (A note's or an answer's page toasts it: `speech.svelte.js`.)
        Some(e) if on_card => {
            tracing::info!("Read aloud: the voice failed");
            note(app, "error", "Couldn't read it aloud", e);
        }
        _ => publish(app),
    }
}

/// A button on the card: "pause" | "resume" | "toggle" | "stop" | "next" |
/// "back" | "faster" | "slower" | "dismiss" (a note's ✕).
pub fn act(app: &AppHandle, action: &str) -> Result<(), String> {
    match action {
        "stop" | "close" | "dismiss" => stop(app),
        "pause" | "resume" | "toggle" => {
            let paused = {
                let s = lock();
                let r = s.session.as_ref().ok_or("Nothing is being read")?;
                r.paused
            };
            let pause = match action {
                "pause" => true,
                "resume" => false,
                _ => !paused,
            };
            if pause == paused {
                return Ok(());
            }
            if pause {
                tts::pause();
            } else {
                tts::resume();
            }
            if let Some(r) = lock().session.as_mut() {
                r.paused = pause;
            }
            tracing::info!(paused = pause, "Read aloud: {}", if pause { "paused" } else { "resumed" });
            publish(app);
        }
        "next" => {
            let (index, count) = {
                let s = lock();
                let r = s.session.as_ref().ok_or("Nothing is being read")?;
                (r.index, r.track.len())
            };
            if index + 1 >= count {
                stop(app);
            } else {
                restart(app, index + 1, None)?;
            }
        }
        "back" => {
            let index = lock().session.as_ref().map(|r| r.index).ok_or("Nothing is being read")?;
            let into = tts::position().filter(|(at, _)| *at == index).map_or(Duration::ZERO, |(_, d)| d);
            restart(app, back_to(index, into), None)?;
        }
        "faster" | "slower" => {
            let (index, rate) = {
                let s = lock();
                let r = s.session.as_ref().ok_or("Nothing is being read")?;
                (r.index, r.rate)
            };
            let new = step_speed(rate, action == "faster");
            if (new - rate).abs() < 0.01 {
                return Ok(());
            }
            save_speed(app, new);
            restart(app, index, Some(new))?;
        }
        other => return Err(format!("Unknown read-aloud action: {other}")),
    }
    Ok(())
}

/// Keep a speed picked on the card as the voice's speed (Settings → Voice
/// Assistant's), and tell Settings' copy (`yap-voice-rate`) so its
/// auto-save keeps it.
fn save_speed(app: &AppHandle, rate: f32) {
    // In one step: never over a change saved meanwhile.
    let saved = crate::commands::change_config(app, |cfg| {
        cfg.tts_rate = rate;
        Ok(())
    });
    if let Err(e) = saved {
        tracing::warn!("Read aloud: couldn't save the speed ({e})");
        return;
    }
    let _ = app.emit("yap-voice-rate", serde_json::json!({ "rate": rate }));
}

// ---- the selection ---------------------------------------------------------------------

/// What reading the selection found (the robot tests check it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picked {
    /// "uia" | "edit" | "copy" | "" (`selection::Selected::via`).
    pub via: &'static str,
    pub chars: usize,
    /// The app is a terminal: no Ctrl+C there.
    pub terminal: bool,
    /// Ctrl+C was allowed as the last resort.
    pub copy_allowed: bool,
    /// "reading" | "stopped" (the shortcut again) | "nothing" | "refused" | "error".
    pub outcome: &'static str,
    /// The text read — robot mode only (the test's own windows' text).
    pub text: Option<String>,
}

/// **Read the selection aloud** (the shortcut, 🔊, the tray), off the
/// calling thread: UI Automation calls can take a moment.
pub fn read_selection(app: &AppHandle, origin: Origin) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("read-selection".into()).spawn(move || {
        read_selection_now(&app, origin);
    });
}

/// [`read_selection`], on this thread. While Yap reads, the same selection
/// again (or none) stops it — the shortcut is its own Stop — and another
/// one is read instead.
pub fn read_selection_now(app: &AppHandle, origin: Origin) -> Picked {
    let mut picked = Picked { via: "", chars: 0, terminal: false, copy_allowed: false, outcome: "nothing", text: None };
    let target = match origin {
        Origin::Tray => {
            let t = crate::pasting::text_injector::app_window();
            if let Some(h) = t {
                crate::pasting::text_injector::focus(h);
                // The app puts its own focus back in its field on activation.
                std::thread::sleep(Duration::from_millis(80));
            }
            t
        }
        Origin::Shortcut | Origin::Bar => crate::pasting::text_injector::current_foreground(),
    };
    let reading = lock().session.is_some();
    // One of Yap's own windows in front: its page reads its own selection
    // (the in-page shortcut); here there's nothing.
    let Some(hwnd) = target else {
        if reading {
            stop(app);
            picked.outcome = "stopped";
        } else {
            nothing_selected(app);
        }
        return picked;
    };
    if crate::e2e::active() && !crate::realtest::permits(Some(hwnd)) {
        tracing::info!("e2e: not reading another app's selection");
        nothing_selected(app);
        picked.outcome = "refused";
        return picked;
    }
    // Never Ctrl+C in a terminal (whatever "Safer pasting into terminals"
    // says): with nothing selected it stops the program running there.
    let kind = crate::pasting::app_kind::of_window(Some(hwnd));
    picked.terminal = kind.is_terminal();
    picked.copy_allowed = !picked.terminal;
    let got = crate::pasting::selection::read_selected(Some(hwnd), picked.copy_allowed);
    picked.via = got.via;
    picked.chars = got.text.as_deref().map_or(0, |t| t.chars().count());
    tracing::info!(via = got.via, chars = picked.chars, terminal = picked.terminal, ?origin, "Read aloud: the selection");
    let text = got.text.filter(|t| !t.trim().is_empty());
    if crate::realtest::on() {
        picked.text = text.clone();
    }
    let Some(text) = text else {
        if reading {
            stop(app);
            picked.outcome = "stopped";
        } else {
            nothing_selected(app);
        }
        return picked;
    };
    // The same selection again: the shortcut is its own Stop.
    if reading && same_text(&text) {
        stop(app);
        picked.outcome = "stopped";
        return picked;
    }
    *LAST_SELECTION.lock().unwrap_or_else(|p| p.into_inner()) = Some(fingerprint(&text));
    picked.outcome = match start(app, Start { text, markdown: false, message_id: None, source: Source::Selection }) {
        Ok(_) => "reading",
        Err(_) => "error",
    };
    picked
}

/// The selection being read (a fingerprint, never the text).
static LAST_SELECTION: Mutex<Option<u64>> = Mutex::new(None);

fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.trim().hash(&mut h);
    h.finish()
}

fn same_text(text: &str) -> bool {
    *LAST_SELECTION.lock().unwrap_or_else(|p| p.into_inner()) == Some(fingerprint(text))
}

// ---- commands ----------------------------------------------------------------------------

/// Read `text` aloud with the reading card: a note or a Chat answer
/// (`markdown`, the default), or text selected in one of Yap's own windows.
/// `messageId`: the page's id for it (an answer's keeps its ▶ / ■).
#[tauri::command]
pub async fn read_aloud_text(
    app: AppHandle,
    text: String,
    message_id: Option<String>,
    markdown: Option<bool>,
    source: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = Source::from_name(source.as_deref().unwrap_or("yap"));
        start(&app, Start { text, markdown: markdown.unwrap_or(true), message_id, source })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Read the selection of the app in front aloud (what the shortcut does).
#[tauri::command]
pub fn read_aloud_selection(app: AppHandle) {
    read_selection(&app, Origin::Shortcut);
}

/// A button on the reading card (see [`act`]).
#[tauri::command]
pub async fn read_aloud_action(app: AppHandle, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || act(&app, &action)).await.map_err(|e| e.to_string())?
}

/// The reading card now.
#[tauri::command]
pub fn read_aloud_state() -> View {
    view()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_reads_by_paragraph_and_list_item() {
        let text = "Launch plan\n\nThe launch moved to Friday because the venue was double booked by the events team,\nso everyone needs to update their calendars before the end of the week.\n\n- Book the room\n- Send the invites\n1. First step\n2) Second step\n\nA short line\nAnother short line";
        assert_eq!(
            plain_blocks(text),
            vec![
                "Launch plan",
                "The launch moved to Friday because the venue was double booked by the events team, so everyone needs to update their calendars before the end of the week.",
                "Book the room",
                "Send the invites",
                "First step",
                "Second step",
                "A short line",
                "Another short line",
            ]
        );
        // A word hyphenated at a wrapped line's end is whole again; CRLF too.
        assert_eq!(
            plain_blocks("Reading aloud helps anyone who finds long docu-\r\nments hard going, and it never gets tired."),
            vec!["Reading aloud helps anyone who finds long documents hard going, and it never gets tired."]
        );
        assert!(plain_blocks(" \n\n \t ").is_empty());
    }

    #[test]
    fn sentences_for_a_selection_and_for_markdown() {
        let (s, cut) = sentences_for("It's Friday. Sam owns it.\n\n— \n\nAsk Jo!", false);
        assert_eq!(s, vec!["It's Friday.", "Sam owns it.", "Ask Jo!"]);
        assert!(!cut);
        let (s, _) = sentences_for("## Plan\n- **Book** the [room](https://example.com)\n```\ncode\n```", true);
        assert_eq!(s, vec!["Plan.", "Book the room.", "(code block)"]);
        // A book's worth is cut at MAX_CHARS.
        let long = "This sentence is read aloud. ".repeat(MAX_CHARS / 20);
        let (s, cut) = sentences_for(&long, false);
        assert!(cut);
        assert!(s.iter().map(|x| x.chars().count() + 1).sum::<usize>() <= MAX_CHARS + 1);
    }

    #[test]
    fn a_sentence_is_drawn_word_by_word() {
        let p = parts("Hi, Sam (again)!");
        let words: Vec<(&str, Option<usize>)> = p.iter().map(|x| (x.text.as_str(), x.w)).collect();
        assert_eq!(words, vec![("Hi", Some(0)), (", ", None), ("Sam", Some(1)), (" (", None), ("again", Some(2)), (")!", None)]);
        assert_eq!(p.iter().map(|x| x.text.as_str()).collect::<String>(), "Hi, Sam (again)!");
        assert!(parts("").is_empty());
    }

    #[test]
    fn speed_and_back_steps() {
        assert_eq!(step_speed(1.0, true), 1.25);
        assert_eq!(step_speed(1.0, false), 0.75);
        assert_eq!(step_speed(1.1, true), 1.25);
        assert_eq!(step_speed(1.1, false), 1.0);
        assert_eq!(step_speed(2.0, true), 2.0);
        assert_eq!(step_speed(0.5, false), 0.5);
        assert_eq!(step_speed(f32::NAN, true), 1.25);
        assert_eq!(back_to(4, Duration::from_millis(300)), 3);
        assert_eq!(back_to(4, Duration::from_secs(3)), 4);
        assert_eq!(back_to(0, Duration::ZERO), 0);
    }

    #[test]
    fn list_markers() {
        assert_eq!(list_item("- milk"), Some("milk"));
        assert_eq!(list_item("12. Twelfth"), Some("Twelfth"));
        assert_eq!(list_item("b) second"), Some("second"));
        assert_eq!(list_item("2026. A year"), None);
        assert_eq!(list_item("Dr. Smith"), None);
        assert_eq!(list_item("-5 degrees"), None);
    }
}
