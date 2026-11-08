//! **One phone's conversation with Yap** (Yap Link's WebSocket, protocol v1:
//! `wire.rs`). First its hello ([`run`]): a paired phone's secret, or the
//! QR's token and a yes on the PC (`pairing.rs`), else `denied` and the
//! socket closes. Then its questions, one at a time:
//!
//! `start` → listening (up frames, 16 kHz mono, at most 2 minutes) → `stop`
//! → transcribing (the warm engine and the dictionary, `PhoneEar`) →
//! `heard` → thinking (the Voice Assistant's path, `assistant::ask_from_phone`:
//! `step`s and the `answer` as it's written, then checked) → speaking (Yap's
//! voice as down frames between `speak_start` and `speak_end`,
//! `voice.rs`) → idle. `interrupt` (or a `start` meanwhile) abandons the
//! question at any point: its answer's stream stops, its voice goes quiet at
//! once (frames not yet sent are dropped), and it's back to idle — or
//! listening. Off during meetings, as the Voice Assistant is: `error`.
//!
//! A **typed** question (`ask`, 2026-11-03) skips the audio: `heard`,
//! thinking, the same path and conversation, its answer not said
//! (`Phone::by_voice` false). **▶ on an answer** (`read_aloud`) says the
//! whole of one this phone was given (`Hub::gave`, from Chat) on the phone.
//! **Chat's picker** sets the model as the desktop's does (`set_model` with
//! a row's provider and model, `set_effort`, `set_fast`: `models.rs`).
//!
//! **Its own conversation** (the Claude-app layout, 2026-11-03): the drawer
//! lists Chat's conversations (`chats`, `link/chats.rs`), `open_chat` puts
//! the phone in one (its messages back, their answers ones ▶ may read) and
//! `new_chat` starts afresh; a question's `chat` says the same. Once a phone
//! has said which, its questions stay in the conversation they went to
//! (`chat {id}`); a phone that never said (a page from before) is asked as
//! the Voice Assistant chooses. **Its pictures** come before the question
//! that takes them (`picture` and its frames, `link/pictures.rs`), are kept
//! as Chat keeps a pasted picture (`snip::attach::keep`) and go with it as a
//! typed question's screenshots do (`assistant::answer`).
//!
//! **Words for the phone's box** (`start {mode: "words"}`, the same day:
//! the Claude and ChatGPT apps' way, talk, check, send): recorded as a
//! question is, then on `stop` (or at 2 minutes) only transcribed — the
//! same `PhoneEar` with the dictionary — and sent back as `words`, then
//! idle ([`finish_question`], `words`). No `heard`, no thinking, nothing
//! written to Chat; `cancel`, `interrupt` or a new `start` drop it as they
//! drop a question. The page puts the words in its box; Send asks them
//! (`ask`) as the spoken question they were (`speak`, that evening: by
//! voice, the Voice Assistant's spoken answer, said on the phone) when any
//! of the box's words came from Talk, else as a typed one.
//!
//! **Remote Control** (2026-11-08, `code.rs`): the Code side's messages go
//! through its gates (`code::handle`); `ready` says whether this phone may
//! use them (`ready.code`); a phone's question asked into a Code chat is
//! remembered as the phone's (`agents/remote.rs`: its agent's turn runs at
//! most at Auto, its report is said here — [`Cmd::Say`]); `code_new` makes
//! the next new chat a Code chat in that workspace.
//!
//! **The hello** carries the phone's device, the QR's token, or both (a tab
//! restored with an old `#pair=`, the code scanned again): the device counts
//! first (`pairing::judge`). Only a hello that's refused waits out its
//! address's strikes, and only a guess is one ([`hello_verdict`]); the
//! refusal's kind is logged.
//!
//! Logs: phases, lengths and timings; never a question, an answer or audio.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tauri::{Emitter, Manager};
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;

use crate::link::pairing::{self, Verdict};
use crate::link::pictures::Pictures;
use crate::link::voice::{self, say, Tx, Wire};
use crate::link::wire::{self, ChatRef, DeviceCreds, In, Out, Phase, Talk};
use crate::link::{devices, Hub, Live};
use crate::voice::tts::Playback as _;

/// How long a phone has to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// The longest question: 2 minutes of 16 kHz audio.
pub const MAX_SAMPLES: usize = wire::UP_RATE as usize * 120;

/// Words the phone shows.
const NOTHING_HEARD: &str = "Yap didn't catch a question. Try again.";
/// Talk's words for the phone's box, when there were none.
const NO_WORDS: &str = "Yap didn't hear anything. Try again, or type it.";
const MEETING: &str = "The Voice Assistant is off during meetings. While a meeting records or a call is on, Yap doesn't listen or talk.";
const NO_SPEECH: &str = "Yap's speech model isn't ready on the PC.";

/// The longest typed question (characters).
const MAX_TYPED: usize = 4_000;
const TOO_LONG: &str = "That question is too long for Yap to take from the phone. Shorten it, then send it again.";
/// ▶ on an answer this phone wasn't given, or one gone from Chat.
const NOT_HERE: &str = "That answer isn't in Chat on your PC any more, so Yap can't read it out.";
/// A conversation the phone opened that's gone (deleted, archived).
const CHAT_GONE: &str = "That chat isn't in Chat on your PC any more. Your next question starts a new one.";

/// **What the PC asks a phone's session to do** (`link.rs`).
pub enum Cmd {
    /// Say an agent's report on this phone (its turn was asked from here):
    /// `reply` true once its voice started, false while the phone is busy
    /// with a question (asked again later).
    Say { text: String, message_id: String, reply: tokio::sync::oneshot::Sender<bool> },
}

/// **One question's turn**, shared by the session, its task and the Voice
/// Assistant's path (`assistant::Phone`).
pub struct Turn {
    id: u64,
    session: u64,
    latest: Arc<AtomicU64>,
    tx: Tx,
    /// The phone's hub and its device id: the answers it was given (▶).
    hub: Arc<Hub>,
    device: String,
    /// Asked out loud (`false`: typed, not said unless ▶ asks).
    by_voice: bool,
    /// The conversation it goes to, as the phone said, and the phone's own
    /// (the one its questions go to from now on).
    chat: ChatRef,
    own: Arc<Mutex<ChatRef>>,
    /// Its pictures: their bytes as they came, then their ids once kept.
    raw: Mutex<Vec<Vec<u8>>>,
    pictures: Mutex<Vec<String>>,
    /// Their text instead ("Send its text instead").
    as_text: bool,
    /// The answer's id, once its voice started ("chat:<conversation>:<index>").
    message_id: Mutex<Option<String>>,
    playback: Mutex<Option<Arc<voice::PhonePlayback>>>,
    speaking: Arc<AtomicBool>,
    /// The checked answer is in (after the voice: idle, not thinking).
    answered: Arc<AtomicBool>,
    /// A voice was started for it (its end says idle).
    voiced: AtomicBool,
    /// A new Code chat's workspace (`code_new`), taken by its first question.
    new_code: Arc<Mutex<Option<String>>>,
}

impl Turn {
    fn new(id: u64, session: u64, latest: Arc<AtomicU64>, asking: &Asking, by_voice: bool, carries: Carries) -> Arc<Turn> {
        Arc::new(Turn {
            id,
            session,
            latest,
            tx: asking.tx.clone(),
            hub: Arc::clone(&asking.hub),
            device: asking.device.clone(),
            by_voice,
            chat: *asking.chat.lock().unwrap_or_else(|p| p.into_inner()),
            own: Arc::clone(&asking.chat),
            raw: Mutex::new(carries.pictures),
            pictures: Mutex::new(Vec::new()),
            as_text: carries.as_text,
            message_id: Mutex::new(None),
            playback: Mutex::new(None),
            speaking: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(false)),
            voiced: AtomicBool::new(false),
            new_code: Arc::clone(&asking.code.new_code),
        })
    }

    /// Still under way: its answer not in yet, or its voice going.
    fn busy(&self) -> bool {
        crate::chat::assistant::Phone::current(self) && (!self.answered.load(Ordering::SeqCst) || self.speaking.load(Ordering::SeqCst))
    }

    /// **Abandon it**: no longer current (its frames still queued are
    /// dropped), its answer's stream stopped, its voice quiet.
    fn abandon(&self) {
        let _ = self.latest.compare_exchange(self.id, self.id + 1, Ordering::SeqCst, Ordering::SeqCst);
        crate::chat::stream::stop(&crate::chat::assistant::Phone::stop_id(self), crate::chat::stream::Halt::Stopped);
        let play = self.playback.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(p) = play {
            p.stop();
        }
        let id = self.message_id.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if id.is_some() && crate::voice::tts::speaking_id() == id {
            crate::voice::tts::stop();
        }
        self.speaking.store(false, Ordering::SeqCst);
    }
}

impl crate::chat::assistant::Phone for Turn {
    fn current(&self) -> bool {
        self.latest.load(Ordering::SeqCst) == self.id
    }

    fn stop_id(&self) -> String {
        format!("link-{}-{}", self.session, self.id)
    }

    fn step(&self, label: &str) {
        self.tell(Out::Step { label: label.to_string() });
    }

    fn answer(&self, text: &str, done: bool) {
        if done {
            self.answered.store(true, Ordering::SeqCst);
        }
        self.tell(Out::Answer { text: text.to_string(), done, id: None });
    }

    fn checked(&self, text: &str, message_id: &str) {
        self.answered.store(true, Ordering::SeqCst);
        if crate::chat::assistant::Phone::current(self) {
            // ▶ on the phone may read it from now on.
            self.hub.gave(&self.device, message_id);
        }
        self.tell(Out::Answer { text: text.to_string(), done: true, id: Some(message_id.to_string()) });
    }

    fn by_voice(&self) -> bool {
        self.by_voice
    }

    fn voice(&self, message_id: &str) -> Result<Arc<crate::voice::tts::Feed>, String> {
        if !self.current() {
            return Err("The phone moved on".into());
        }
        let cfg = crate::config::load();
        let (open, play) = voice::opener(self.tx.clone(), self.id, Arc::clone(&self.latest), Arc::clone(&self.speaking), Arc::clone(&self.answered));
        *self.message_id.lock().unwrap_or_else(|p| p.into_inner()) = Some(message_id.to_string());
        *self.playback.lock().unwrap_or_else(|p| p.into_inner()) = Some(play);
        self.voiced.store(true, Ordering::SeqCst);
        crate::voice::tts::speak_streamed_to(Some(message_id.to_string()), crate::voice::tts::phone_voicing(&cfg), open).inspect_err(|_| {
            self.voiced.store(false, Ordering::SeqCst);
            self.playback.lock().unwrap_or_else(|p| p.into_inner()).take();
        })
    }

    fn failed(&self, message: &str) {
        self.tell(Out::Error { message: message.to_string() });
    }

    fn chat(&self) -> ChatRef {
        self.chat
    }

    fn asked(&self, conversation: u64) {
        // A phone that keeps its own conversation stays in this one; one
        // that never said is asked as the Voice Assistant chooses next time
        // too (a page from before).
        {
            let mut own = self.own.lock().unwrap_or_else(|p| p.into_inner());
            if *own != ChatRef::Choose {
                *own = ChatRef::Id(conversation);
            }
        }
        let title = crate::chat::chats::get(conversation).map(|c| crate::link::chats::title(&c)).unwrap_or_default();
        self.tell(Out::Chat { id: conversation, title });
    }

    fn question_saved(&self, conversation: u64, index: usize) {
        // This phone's question (Remote Control: an agent it asks for runs at
        // most at Auto, and its report is said here).
        crate::agents::remote::asked(&format!("chat:{conversation}:{index}"), &self.device);
    }

    fn new_code_folder(&self) -> Option<String> {
        if self.chat != ChatRef::New {
            return None;
        }
        self.new_code.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    fn pictures(&self) -> (Vec<String>, bool) {
        (self.pictures.lock().unwrap_or_else(|p| p.into_inner()).clone(), self.as_text)
    }

    fn cant_read(&self, message: &str) {
        self.tell(Out::CantRead { message: message.to_string() });
    }
}

/// What a question carries besides its words: its pictures' bytes, whether
/// their text goes instead, and (from the composer) whether its words were
/// said.
#[derive(Default)]
struct Carries {
    pictures: Vec<Vec<u8>>,
    as_text: bool,
    /// Talk's words, checked in the phone's box and sent (`ask {speak}`):
    /// asked as said, and answered out loud.
    said: bool,
}

impl Turn {
    /// Say `out` to the phone while this is its question: an abandoned
    /// question's late news never reaches it.
    fn tell(&self, out: Out) {
        if crate::chat::assistant::Phone::current(self) {
            say(&self.tx, out);
        }
    }
}

/// What the session is doing with the phone's question.
struct Asking {
    hub: Arc<Hub>,
    tx: Tx,
    /// The connected phone's device id.
    device: String,
    recording: bool,
    /// What the question being recorded is for: answering, or its words
    /// for the phone's box (`start`'s `mode`).
    talk: Talk,
    audio: Vec<f32>,
    turn: Option<Arc<Turn>>,
    started: Option<Instant>,
    /// The conversation this phone's questions go to.
    chat: Arc<Mutex<ChatRef>>,
    /// The pictures the next question takes, as they come.
    pictures: Pictures,
    /// Its name as it said hello, and its Code side (Remote Control).
    name: String,
    code: Arc<crate::link::code::PhoneCode>,
}

impl Asking {
    /// The phone said which conversation (an `open_chat`, a `new_chat`, a
    /// question's `chat`). Another than a new one: no new Code chat waits.
    fn set_chat(&self, chat: ChatRef) {
        *self.chat.lock().unwrap_or_else(|p| p.into_inner()) = chat;
        if chat != ChatRef::New {
            self.code.new_code.lock().unwrap_or_else(|p| p.into_inner()).take();
        }
    }

    /// A question being recorded, or one being answered or said.
    fn busy(&self) -> bool {
        self.recording || self.turn.as_ref().is_some_and(|t| t.busy())
    }
}

/// **Run the conversation** on a socket that has just been upgraded (`part`:
/// what the phone sent after its request, if anything).
pub async fn run<S>(stream: S, part: Vec<u8>, peer: SocketAddr, hub: Arc<Hub>, mut stop: tokio::sync::watch::Receiver<bool>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let config = WebSocketConfig::default().max_message_size(Some(wire::MAX_FRAME)).max_frame_size(Some(wire::MAX_FRAME));
    let ws = tokio_tungstenite::WebSocketStream::from_partially_read(stream, part, Role::Server, Some(config)).await;
    let (mut sink, mut incoming) = ws.split();
    // Two lanes out (`lanes.rs`): the control lane first and capped, the
    // watched agent's events bounded and coalesced.
    let (tx, mut rx) = crate::link::lanes::channel();
    let latest = Arc::new(AtomicU64::new(0));
    let sent = Arc::new(AtomicU64::new(0));
    // The writer: text as it comes, a turn's audio only while it's the latest.
    let writer = {
        let latest = Arc::clone(&latest);
        let sent = Arc::clone(&sent);
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                let msg = match m {
                    Wire::Text(t) => Message::text(t),
                    Wire::Audio { turn, bytes } => {
                        if turn != latest.load(Ordering::SeqCst) {
                            continue;
                        }
                        Message::binary(bytes)
                    }
                    Wire::Close => {
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                };
                sent.fetch_add(msg.len() as u64, Ordering::Relaxed);
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        })
    };
    let session = hub.next_id();
    let mut received = 0u64;

    // ---- the hello ----
    let first = tokio::time::timeout(HELLO_WAIT, incoming.next()).await;
    let hello = match first {
        Ok(Some(Ok(Message::Text(t)))) => {
            received += t.len() as u64;
            wire::parse(t.as_str()).ok()
        }
        _ => None,
    };
    let Some(In::Hello { v, name, nonce, device, pair }) = hello else {
        say(&tx, Out::Denied { reason: "Say hello first.".into() });
        let _ = tx.send(Wire::Close);
        let _ = writer.await;
        crate::privacy::net::phone(peer.ip(), 403, received, sent.load(Ordering::Relaxed));
        return;
    };
    let name = pairing::clean_name(&name);
    let device_id = match hello_verdict(&hub, peer, v, &nonce, device.as_ref(), pair.as_ref().map(|p| p.token.as_str())) {
        Verdict::Known { id, invited } => {
            if invited {
                // The code on the PC's screen brought by a phone paired
                // already: Settings' code has done its job (said there).
                let _ = hub.app.emit(crate::link::ALREADY_PAIRED, serde_json::json!({ "name": name }));
            }
            Some(id)
        }
        Verdict::Pair { code } => pair_phone(&hub, &tx, &mut incoming, &mut stop, &name, &code).await,
        Verdict::Denied(refusal) => {
            say(&tx, Out::Denied { reason: refusal.words() });
            None
        }
    };
    let Some(device_id) = device_id else {
        let _ = tx.send(Wire::Close);
        let _ = writer.await;
        crate::privacy::net::phone(peer.ip(), 403, received, sent.load(Ordering::Relaxed));
        return;
    };

    // ---- connected ----
    devices::seen(&device_id, &name);
    let kill = Arc::new(tokio::sync::Notify::new());
    let chat = Arc::new(Mutex::new(ChatRef::Choose));
    let code = Arc::new(crate::link::code::PhoneCode::default());
    let (cmd, mut cmds) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    say(&tx, hub.ready_for(&device_id));
    say(&tx, Out::State { phase: Phase::Idle });
    hub.connected(
        session,
        Live { device: device_id.clone(), name: name.clone(), tx: tx.clone(), kill: Arc::clone(&kill), chat: Arc::clone(&chat), code: Arc::clone(&code), cmd },
    );
    // Remote Control on for it: the status dots and the cards waiting.
    crate::link::code::connected(&hub, &tx, &device_id, &code);
    tracing::info!(session, "Yap Link: a phone connected");

    let mut asking = Asking {
        hub: Arc::clone(&hub),
        tx: tx.clone(),
        device: device_id.clone(),
        recording: false,
        talk: Talk::Ask,
        audio: Vec::new(),
        turn: None,
        started: None,
        chat,
        pictures: Pictures::default(),
        name: name.clone(),
        code,
    };
    loop {
        let next = tokio::select! {
            m = incoming.next() => m,
            Some(c) = cmds.recv() => {
                on_cmd(&hub, session, &latest, &mut asking, c);
                continue;
            }
            _ = kill.notified() => {
                say(&tx, Out::Denied { reason: pairing::REMOVED.into() });
                break;
            }
            _ = stop.changed() => {
                say(&tx, Out::Denied { reason: pairing::OFF.into() });
                break;
            }
        };
        let Some(Ok(msg)) = next else { break };
        match msg {
            Message::Text(t) => {
                received += t.len() as u64;
                match wire::parse(t.as_str()) {
                    Ok(m) => on_message(&hub, &tx, session, &latest, &mut asking, m),
                    Err(e) => say(&tx, Out::Error { message: e }),
                }
            }
            Message::Binary(b) => {
                received += b.len() as u64;
                on_audio(&hub, session, &latest, &mut asking, &b);
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    // The phone left (or was removed): its question goes with it.
    if let Some(turn) = asking.turn.take() {
        turn.abandon();
    }
    hub.disconnected(session);
    let _ = tx.send(Wire::Close);
    drop(tx);
    let _ = writer.await;
    tracing::info!(session, "Yap Link: a phone disconnected");
    crate::privacy::net::phone(peer.ip(), 101, received, sent.load(Ordering::Relaxed));
}

/// **The hello, judged** (and rate-limited per address): first the minute's
/// cap (`Limiter::admit`), then the hello itself (`pairing::judge`: the
/// device first, then the QR's token); a refused one waits when its address
/// has guessed too often (`struck_out`, said with when to try again), and
/// counts as a strike only when it was a guess (`Refusal::strikes`). A
/// paired phone's hello, and the live code's, are never turned away for an
/// address's strikes: they aren't guesses (a 256-bit secret, a 128-bit
/// token), and its other tabs' stale codes mustn't lock the phone out.
/// Logs the refusal's kind, never the hello's ids, secrets or address.
fn hello_verdict(hub: &Hub, peer: SocketAddr, v: u32, nonce: &str, device: Option<&DeviceCreds>, token: Option<&str>) -> Verdict {
    let now = Instant::now();
    let ip = peer.ip();
    if let Err(wait) = hub.limiter().admit(ip, now) {
        tracing::info!(kind = "too many", "Yap Link: a hello was refused (too many from one address this minute)");
        return Verdict::Denied(pairing::Refusal::TooMany(wait));
    }
    let hello = pairing::Hello { v, nonce, device: device.map(|d| (d.id.as_str(), d.secret.as_str())), token };
    let verdict = {
        let mut invite = hub.invite();
        pairing::judge(&hello, invite.as_mut(), &devices::secret_hash, now)
    };
    match verdict {
        Verdict::Denied(refusal) => {
            let mut limiter = hub.limiter();
            if let Some(wait) = limiter.struck_out(ip, now) {
                drop(limiter);
                tracing::info!(kind = "too many", was = refusal.kind(), "Yap Link: a hello was refused (its address waits)");
                return Verdict::Denied(pairing::Refusal::TooMany(wait));
            }
            let strike = refusal.strikes();
            if strike {
                limiter.strike(ip, now);
            }
            drop(limiter);
            tracing::info!(kind = refusal.kind(), strike, "Yap Link: a hello was refused");
            Verdict::Denied(refusal)
        }
        Verdict::Pair { code } => {
            tracing::info!(with_device = device.is_some(), "Yap Link: a phone asks to pair");
            Verdict::Pair { code }
        }
        Verdict::Known { id, invited } => {
            if invited {
                tracing::info!("Yap Link: a paired phone brought the pairing code again (it's used up)");
            } else if token.is_some() {
                tracing::info!("Yap Link: a paired phone brought an old pairing code (its device counted)");
            }
            Verdict::Known { id, invited }
        }
    }
}

/// **A phone asking to pair**: `pair_wait` with its code, the question on
/// the PC, and the person's answer — Allow pairs it (`paired`), Deny, no
/// answer in 2 minutes, the phone leaving or Link going off don't.
async fn pair_phone<R>(
    hub: &Arc<Hub>,
    tx: &Tx,
    incoming: &mut R,
    stop: &mut tokio::sync::watch::Receiver<bool>,
    name: &str,
    code: &str,
) -> Option<String>
where
    R: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    say(tx, Out::PairWait { code: code.to_string() });
    let (id, answer) = hub.ask_to_pair(name, code);
    let deadline = tokio::time::sleep(pairing::ANSWER_FOR);
    tokio::pin!(deadline);
    tokio::pin!(answer);
    let allowed = loop {
        tokio::select! {
            a = &mut answer => break a.unwrap_or(false),
            _ = &mut deadline => {
                say(tx, Out::Denied { reason: pairing::NO_ANSWER.into() });
                hub.pair_done(id);
                return None;
            }
            _ = stop.changed() => {
                say(tx, Out::Denied { reason: pairing::OFF.into() });
                hub.pair_done(id);
                return None;
            }
            m = incoming.next() => match m {
                Some(Ok(Message::Text(t))) if matches!(wire::parse(t.as_str()), Ok(In::Ping)) => say(tx, Out::Pong),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                    // The phone gave up: the question on the PC goes.
                    hub.pair_done(id);
                    return None;
                }
                _ => {}
            },
        }
    };
    hub.pair_done(id);
    if !allowed {
        say(tx, Out::Denied { reason: pairing::DENIED.into() });
        return None;
    }
    match devices::add(name) {
        Ok((device, secret)) => {
            tracing::info!("Yap Link: a phone was paired");
            // Paired while Remote Control is on: ticked for it (one paired
            // before stays off until ticked in Settings → Phone).
            crate::agents::remote::paired(&device, crate::config::load().link_remote_control);
            say(tx, Out::Paired { device: DeviceCreds { id: device.clone(), secret } });
            let _ = hub.app.emit(crate::link::CHANGED, ());
            Some(device)
        }
        Err(e) => {
            say(tx, Out::Denied { reason: e });
            None
        }
    }
}

/// Abandon the question under way, if any (an interrupt, a new start, a
/// cancel, a typed question, ▶): one still being recorded is dropped, one
/// being answered stops. `true`: there was one.
fn abandon(latest: &AtomicU64, asking: &mut Asking) -> bool {
    let recording = asking.recording;
    if recording {
        asking.recording = false;
        asking.audio = Vec::new();
        latest.fetch_add(1, Ordering::SeqCst);
    }
    let Some(turn) = asking.turn.take() else { return recording };
    let was = crate::chat::assistant::Phone::current(turn.as_ref());
    turn.abandon();
    if was {
        tracing::info!("Yap Link: the phone interrupted its question");
    }
    recording || was
}

/// **What the PC asked of this session** ([`Cmd`]).
fn on_cmd(hub: &Arc<Hub>, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, c: Cmd) {
    match c {
        Cmd::Say { text, message_id, reply } => {
            // Never over a question being asked, answered or said.
            if asking.busy() || crate::chat::assistant::is_off() {
                let _ = reply.send(false);
                return;
            }
            let _ = reply.send(true);
            speak_on_phone(hub, session, latest, asking, &message_id, text);
        }
    }
}

/// **Say `text` on the phone** (answer `message_id` in Chat), whole, in
/// Yap's voice, as its own turn: ▶ on an answer, and an agent's report said
/// on the phone that asked for it.
fn speak_on_phone(hub: &Arc<Hub>, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, message_id: &str, text: String) {
    hub.gave(&asking.device, message_id);
    let turn_id = latest.fetch_add(1, Ordering::SeqCst) + 1;
    let turn = Turn::new(turn_id, session, Arc::clone(latest), asking, true, Carries::default());
    // Its answer is in: the voice's end says idle.
    turn.answered.store(true, Ordering::SeqCst);
    asking.turn = Some(Arc::clone(&turn));
    let id = message_id.to_string();
    tauri::async_runtime::spawn(async move {
        use crate::chat::assistant::Phone;
        match turn.voice(&id) {
            Ok(feed) => {
                feed.push(crate::voice::tts::utterances(&text));
                feed.close();
            }
            Err(e) => {
                tracing::info!("Yap Link: couldn't say it on the phone ({e})");
                turn.tell(Out::Error { message: format!("Yap couldn't read it out. {e}") });
                turn.tell(Out::State { phase: Phase::Idle });
            }
        }
    });
}

fn on_message(hub: &Arc<Hub>, tx: &Tx, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, m: In) {
    if crate::link::code::is_code(&m) {
        // Remote Control: through its gates, every one, on every message.
        let cx = crate::link::code::Ctx { hub, tx, device: &asking.device, name: &asking.name, phone: &asking.code, chat: &asking.chat };
        return crate::link::code::handle(&cx, m);
    }
    match m {
        In::Ping => say(tx, Out::Pong),
        In::Hello { .. } => say(tx, Out::Error { message: "Already connected.".into() }),
        In::Start { mode, chat } => {
            // A start while it speaks (or thinks) is an interrupt, then a start.
            abandon(latest, asking);
            asking.talk = Talk::of(&mode);
            if let Some(c) = wire::chat_ref(chat.as_ref()) {
                asking.set_chat(c);
            }
            // Pictures come just before the question's end (`stop`): any
            // from before are stale.
            asking.pictures = Pictures::default();
            if crate::chat::assistant::is_off() {
                say(tx, Out::Error { message: MEETING.into() });
                say(tx, Out::State { phase: Phase::Idle });
                return;
            }
            latest.fetch_add(1, Ordering::SeqCst);
            asking.recording = true;
            asking.audio.clear();
            asking.started = Some(Instant::now());
            say(tx, Out::State { phase: Phase::Listening });
        }
        In::Stop { pictures, as_text } => {
            if asking.recording && asking.talk == Talk::Words {
                // Words for the phone's box: its pictures stay there, for
                // the question it sends (none come with this).
                asking.pictures = Pictures::default();
                finish_question(hub, session, latest, asking, Carries::default());
            } else if asking.recording {
                match asking.pictures.take(pictures) {
                    Ok(pictures) => finish_question(hub, session, latest, asking, Carries { pictures, as_text, said: true }),
                    Err(why) => {
                        // Its pictures didn't come whole: nothing goes, said why.
                        abandon(latest, asking);
                        say(tx, Out::Error { message: why.into() });
                        say(tx, Out::State { phase: Phase::Idle });
                    }
                }
            }
        }
        // Idle at once, whatever there was to drop: the page waits for it
        // before it takes the next question's news.
        In::Cancel | In::Interrupt => {
            abandon(latest, asking);
            say(tx, Out::State { phase: Phase::Idle });
        }
        In::Ask { text, chat, pictures, as_text, speak } => {
            if let Some(c) = wire::chat_ref(chat.as_ref()) {
                asking.set_chat(c);
            }
            match asking.pictures.take(pictures) {
                Ok(pictures) => ask_typed(hub, tx, session, latest, asking, &text, Carries { pictures, as_text, said: speak }),
                Err(why) => {
                    if abandon(latest, asking) {
                        say(tx, Out::State { phase: Phase::Idle });
                    }
                    say(tx, Out::Error { message: why.into() });
                }
            }
        }
        In::Picture { n, size } => {
            // Refused (too big, too many, out of turn): the question that
            // would take it says why (`Pictures::take`).
            if let Err(why) = asking.pictures.announce(n, size) {
                tracing::info!(n, size, "Yap Link: a picture from the phone refused ({why})");
            }
        }
        In::Chats => {
            let items = crate::chat::chats::with_all(crate::link::chats::recents);
            // A Code chat's workspace named (Remote Control on for it).
            let items = crate::link::code::drawer(&asking.device, items);
            say(tx, Out::Chats { items });
        }
        In::OpenChat { id } => open_chat(hub, tx, asking, id),
        In::NewChat => {
            asking.set_chat(ChatRef::New);
            asking.code.new_code.lock().unwrap_or_else(|p| p.into_inner()).take();
            say(tx, empty_thread());
        }
        In::ReadAloud { id } => read_aloud(hub, tx, session, latest, asking, &id),
        In::SetModel { id, provider, model, base_url, file } => {
            // A row of Chat's picker (`ready.picker`), as the desktop picker
            // picks it; else one of `ready.models` by its id (a first page).
            let set = if provider.is_empty() {
                crate::link::models::set(&hub.app, &id)
            } else {
                crate::link::models::set_row(&hub.app, &provider, &model, base_url.as_deref(), file.as_deref())
            };
            // A fresh `ready` either way: the page shows the pick at once and
            // takes this as the truth (every other phone hears it too, from
            // `yap-chat-model-changed`).
            if let Err(e) = set {
                say(tx, Out::Error { message: e });
            }
            say(tx, hub.ready_for(&asking.device));
        }
        In::SetEffort { level } => {
            let set = crate::link::models::set_effort(&hub.app, level.as_deref());
            settled(hub, tx, &asking.device, set);
        }
        In::SetFast { on } => {
            let set = crate::link::models::set_fast(&hub.app, on);
            settled(hub, tx, &asking.device, set);
        }
        // (The Code side's: `code::handle`, above.)
        _ => {}
    }
}

/// **`open_chat`**: the phone's questions go to conversation `id` from now
/// on, and its latest messages go back (their answers ones ▶ may read). One
/// gone from Chat (deleted, archived, a meeting's thread): said, and the
/// next question starts a new one.
fn open_chat(hub: &Arc<Hub>, tx: &Tx, asking: &mut Asking, id: u64) {
    let conv = crate::chat::chats::get(id).filter(crate::link::chats::usable);
    let Some(conv) = conv else {
        asking.set_chat(ChatRef::New);
        say(tx, Out::Error { message: CHAT_GONE.into() });
        say(tx, empty_thread());
        return;
    };
    asking.set_chat(ChatRef::Id(id));
    let (title, messages, more) = crate::link::chats::thread(&conv);
    for m in &messages {
        if let Some(answer) = &m.id {
            hub.gave(&asking.device, answer);
        }
    }
    // A Code chat's own parts (Remote Control on for this phone).
    let (code, agents, cards) = crate::link::code::thread_parts(hub, &asking.device, id);
    tracing::info!(messages = messages.len(), more, code = code.is_some(), "Yap Link: the phone opened a chat");
    say(tx, Out::Thread { id: Some(id), title, messages, more, code, agents, cards });
}

/// A new chat's thread: nothing in it yet.
fn empty_thread() -> Out {
    Out::Thread { id: None, title: String::new(), messages: Vec::new(), more: false, code: None, agents: Vec::new(), cards: Vec::new() }
}

/// The effort or fast mode set from the phone: the chips on the PC read
/// again, and every phone hears (`yap-chat-model-changed` with no scope, so
/// Settings has nothing to adopt); refused, why, and the PC's own word.
fn settled(hub: &Arc<Hub>, tx: &Tx, device: &str, set: Result<(), String>) {
    match set {
        Ok(()) => {
            let _ = hub.app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
        }
        Err(e) => {
            say(tx, Out::Error { message: e });
            say(tx, hub.ready_for(device));
        }
    }
}

/// **A question from the composer** (`ask`): whatever was under way is
/// abandoned (said as `state idle`, so the page knows the old question's
/// news has ended), then it's asked as a spoken one is once it's
/// transcribed — `heard`, thinking, the Voice Assistant's path — typed and
/// not said; or, with `speak` (`Carries::said`: Talk's words, checked in the
/// box), as the spoken question it was: by voice, its answer said on the
/// phone.
fn ask_typed(hub: &Arc<Hub>, tx: &Tx, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, text: &str, carries: Carries) {
    if abandon(latest, asking) {
        say(tx, Out::State { phase: Phase::Idle });
    }
    let question = text.trim().to_string();
    if question.is_empty() {
        say(tx, Out::Error { message: NOTHING_HEARD.into() });
        return;
    }
    if question.chars().count() > MAX_TYPED {
        say(tx, Out::Error { message: TOO_LONG.into() });
        return;
    }
    if crate::chat::assistant::is_off() {
        say(tx, Out::Error { message: MEETING.into() });
        say(tx, Out::State { phase: Phase::Idle });
        return;
    }
    let id = latest.fetch_add(1, Ordering::SeqCst) + 1;
    let pictures = carries.pictures.len();
    let said = carries.said;
    let turn = Turn::new(id, session, Arc::clone(latest), asking, said, carries);
    asking.turn = Some(Arc::clone(&turn));
    if said {
        tracing::info!(chars = question.chars().count(), pictures, "Yap Link: a spoken question from the phone's box (Talk's words, checked)");
    } else {
        tracing::info!(chars = question.chars().count(), pictures, "Yap Link: a typed question from the phone");
    }
    let app = hub.app.clone();
    tauri::async_runtime::spawn(async move { answer(app, turn, question).await });
}

/// **▶ on an answer** (`read_aloud`): the whole of it said on the phone in
/// Yap's voice — only an answer this phone was given, as Chat keeps it.
/// Whatever was under way is abandoned first (`state idle`).
fn read_aloud(hub: &Arc<Hub>, tx: &Tx, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, id: &str) {
    abandon(latest, asking);
    say(tx, Out::State { phase: Phase::Idle });
    if crate::chat::assistant::is_off() {
        say(tx, Out::Error { message: MEETING.into() });
        return;
    }
    let text = if hub.was_given(&asking.device, id) { answer_text(id) } else { None };
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
        say(tx, Out::Error { message: NOT_HERE.into() });
        return;
    };
    tracing::info!(chars = text.chars().count(), "Yap Link: an answer read aloud on the phone");
    speak_on_phone(hub, session, latest, asking, id, text);
}

/// An answer's words in Chat, by its id (`chat:<conversation>:<index>`).
fn answer_text(id: &str) -> Option<String> {
    let (conversation, index) = parse_message_id(id)?;
    let c = crate::chat::chats::get(conversation)?;
    c.messages.get(index).filter(|m| m.role == "assistant").map(|m| m.text.clone())
}

/// `chat:<conversation>:<index>` → its parts. Pure.
fn parse_message_id(id: &str) -> Option<(u64, usize)> {
    let mut parts = id.strip_prefix("chat:")?.splitn(2, ':');
    let conversation = parts.next()?.parse().ok()?;
    let index = parts.next()?.parse().ok()?;
    Some((conversation, index))
}

fn on_audio(hub: &Arc<Hub>, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, bytes: &[u8]) {
    // A piece of a picture for the next question.
    if let Some((seq, piece)) = wire::picture_piece(bytes) {
        if let Err(why) = asking.pictures.piece(seq, piece) {
            tracing::info!(seq, "Yap Link: a picture's piece out of turn ({why})");
        }
        return;
    }
    if !asking.recording {
        return;
    }
    let Ok(frame) = wire::decode(bytes) else { return };
    if frame.kind != wire::UP {
        return;
    }
    let room = MAX_SAMPLES.saturating_sub(asking.audio.len());
    asking.audio.extend(wire::to_f32(&frame.samples).take(room));
    // Two minutes: it's asked as it is.
    if asking.audio.len() >= MAX_SAMPLES {
        tracing::info!("Yap Link: a question reached 2 minutes; asked as it is");
        finish_question(hub, session, latest, asking, Carries::default());
    }
}

/// The question's audio is in: transcribe it and ask — or, for words for
/// the phone's box, only transcribe it.
fn finish_question(hub: &Arc<Hub>, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, carries: Carries) {
    asking.recording = false;
    let audio = std::mem::take(&mut asking.audio);
    let id = latest.load(Ordering::SeqCst);
    let pictures = carries.pictures.len();
    // A turn either way: `cancel`, `interrupt` and a new `start` drop it as
    // they drop a question (its late news never reaches the phone).
    let turn = Turn::new(id, session, Arc::clone(latest), asking, true, carries);
    asking.turn = Some(Arc::clone(&turn));
    let held = asking.started.take().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
    let app = hub.app.clone();
    if asking.talk == Talk::Words {
        tracing::info!(samples = audio.len(), held_ms = held, "Yap Link: words from the phone, for its box");
        tauri::async_runtime::spawn(async move { words(app, turn, audio).await });
        return;
    }
    tracing::info!(samples = audio.len(), held_ms = held, pictures, "Yap Link: a question from the phone");
    tauri::async_runtime::spawn(async move { ask(app, turn, audio).await });
}

/// The phone's audio through the warm engine and the dictionary (`PhoneEar`)
/// — and AI cleanup for a question going into a Code chat with "Clean up
/// what you say in Code chats" on (`turn`'s conversation, as the Voice
/// Assistant's path will choose it: [`code_chat_cleans`]). A plain chat's
/// stay as said, as before.
async fn transcribe(app: &tauri::AppHandle, turn: &Turn, audio: Vec<f32>) -> Result<String, String> {
    let ear = {
        let state = app.state::<crate::AppState>();
        let guard = state.pipeline.lock().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().map(|p| p.phone_ear())
    };
    match ear {
        Some(ear) => ear.question(audio, code_chat_cleans(turn)).await,
        None => Err(NO_SPEECH.to_string()),
    }
}

/// Is the phone's question for a Code chat, cleaned up there
/// (`cleanup_styles::cleans_question`, the Code chats' switch)?
fn code_chat_cleans(turn: &Turn) -> bool {
    use crate::chat::assistant::Phone;
    let cfg = crate::config::load();
    // (A new Code chat waiting for this question: `code_new`.)
    let new_code = turn.chat() == ChatRef::New && turn.new_code.lock().unwrap_or_else(|p| p.into_inner()).is_some();
    (crate::chat::assistant::phone_in_code_chat(&cfg, turn.chat()) || (cfg.agents_enabled && new_code)) && crate::ai::cleanup_styles::cleans_question(&cfg, true)
}

/// **Words for the phone's box**: transcribed, said back (`words`, or why
/// not), then idle. Nothing asked, nothing written to Chat.
async fn words(app: tauri::AppHandle, turn: Arc<Turn>, audio: Vec<f32>) {
    use crate::chat::assistant::Phone;
    turn.tell(Out::State { phase: Phase::Transcribing });
    let heard = transcribe(&app, &turn, audio).await;
    if !turn.current() {
        return;
    }
    if let Err(e) = &heard {
        tracing::warn!("Yap Link: the phone's words weren't transcribed ({e})");
    }
    let out = words_out(heard);
    if let Out::Words { text } = &out {
        tracing::info!(chars = text.chars().count(), "Yap Link: the phone's words, back to its box");
    }
    turn.tell(out);
    turn.tell(Out::State { phase: Phase::Idle });
}

/// What the phone's box gets for its transcription: the words, trimmed, or
/// why there are none. Pure.
fn words_out(heard: Result<String, String>) -> Out {
    match heard {
        Ok(text) if !text.trim().is_empty() => Out::Words { text: text.trim().to_string() },
        Ok(_) => Out::Error { message: NO_WORDS.into() },
        Err(e) => Out::Error { message: format!("Yap couldn't make out what you said. {e}") },
    }
}

/// One question: transcribed, heard, answered (the Voice Assistant's path).
async fn ask(app: tauri::AppHandle, turn: Arc<Turn>, audio: Vec<f32>) {
    use crate::chat::assistant::Phone;
    turn.tell(Out::State { phase: Phase::Transcribing });
    let heard = transcribe(&app, &turn, audio).await;
    if !turn.current() {
        return;
    }
    let question = match heard {
        Ok(q) if !q.trim().is_empty() => q,
        Ok(_) => {
            turn.tell(Out::Error { message: NOTHING_HEARD.into() });
            turn.tell(Out::State { phase: Phase::Idle });
            return;
        }
        Err(e) => {
            tracing::warn!("Yap Link: the phone's question wasn't transcribed ({e})");
            turn.tell(Out::Error { message: format!("Yap couldn't make out the question. {e}") });
            turn.tell(Out::State { phase: Phase::Idle });
            return;
        }
    };
    answer(app, turn, question).await;
}

/// A question heard (or typed): `heard`, thinking, the Voice Assistant's
/// path, then idle unless a voice is going (its end says idle).
async fn answer(app: tauri::AppHandle, turn: Arc<Turn>, question: String) {
    use crate::chat::assistant::Phone;
    turn.tell(Out::Heard { text: question.clone() });
    if crate::chat::assistant::is_off() {
        turn.tell(Out::Error { message: MEETING.into() });
        turn.tell(Out::State { phase: Phase::Idle });
        return;
    }
    turn.tell(Out::State { phase: Phase::Thinking });
    // Its pictures, kept as Chat keeps a pasted one (decoded, stored again
    // as PNGs: nothing that isn't a picture is kept).
    let raw = std::mem::take(&mut *turn.raw.lock().unwrap_or_else(|p| p.into_inner()));
    if !raw.is_empty() {
        match keep_pictures(raw).await {
            Ok(ids) => *turn.pictures.lock().unwrap_or_else(|p| p.into_inner()) = ids,
            Err(e) => {
                tracing::info!("Yap Link: a picture from the phone wasn't kept ({e})");
                turn.tell(Out::Error { message: format!("Yap couldn't read one of the pictures on your PC: {e}.") });
                turn.tell(Out::State { phase: Phase::Idle });
                return;
            }
        }
        if !turn.current() {
            return;
        }
    }
    let started = Instant::now();
    crate::chat::assistant::ask_from_phone(app, question, Arc::clone(&turn) as Arc<dyn Phone>).await;
    tracing::info!(
        ms = started.elapsed().as_millis() as u64,
        typed = !turn.by_voice,
        spoken = turn.voiced.load(Ordering::SeqCst),
        "Yap Link: the phone's question answered"
    );
    // No voice (typed, nothing to say, or it couldn't start): idle now; else its end says so.
    if !turn.voiced.load(Ordering::SeqCst) {
        turn.tell(Out::State { phase: Phase::Idle });
    }
}

/// A question's pictures kept for Chat (`snip::attach::keep`, as a pasted
/// picture is): their ids, or why one couldn't be.
async fn keep_pictures(raw: Vec<Vec<u8>>) -> Result<Vec<String>, String> {
    let kept = tauri::async_runtime::spawn_blocking(move || {
        raw.iter().map(|bytes| crate::snip::attach::keep(bytes, "phone").map(|image| image.id)).collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|e| e.to_string())??;
    tracing::info!(pictures = kept.len(), "Yap Link: the phone's pictures kept");
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_aloud_names_an_answer_by_its_place_in_chat() {
        assert_eq!(parse_message_id("chat:12:5"), Some((12, 5)));
        assert_eq!(parse_message_id("chat:12"), None);
        assert_eq!(parse_message_id("note:12:5"), None);
        assert_eq!(parse_message_id("chat:x:5"), None);
        assert_eq!(parse_message_id("chat:12:-1"), None);
        assert_eq!(parse_message_id(""), None);
    }

    #[test]
    fn the_phones_words_go_back_to_its_box_trimmed_or_say_why_not() {
        let j = |o: Out| serde_json::from_str::<serde_json::Value>(&o.json()).unwrap();
        assert_eq!(
            j(words_out(Ok("  Book the hall for Saturday afternoon \n".into()))),
            serde_json::json!({"type":"words","text":"Book the hall for Saturday afternoon"})
        );
        // Nothing heard: the box keeps what it has, and the phone says so.
        for nothing in ["", "   \n"] {
            assert_eq!(j(words_out(Ok(nothing.into()))), serde_json::json!({"type":"error","message":NO_WORDS}));
        }
        // A transcription that failed: why, in the PC's words.
        assert_eq!(
            j(words_out(Err("No speech model was ready: still loading".into()))),
            serde_json::json!({"type":"error","message":"Yap couldn't make out what you said. No speech model was ready: still loading"})
        );
    }
}
