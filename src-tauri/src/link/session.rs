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
//! Or a **dictation** (`start {mode: "dictate", target}`, `dictate.rs`):
//! the phone is the microphone of an ordinary dictation on this PC
//! (`pipeline::PhoneMic`): listening (its up frames into the pipeline) →
//! `stop` → transcribing → `heard` (and `pasted`, to the PC) → idle.
//! `cancel` calls it off: still recording, it's cancelled on the PC (kept
//! there for Undo); being typed, nothing is pasted. Refused (`error`)
//! without the phone's "May type into this PC" (to the PC), during a
//! meeting, or beside a dictation under way on the PC.
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

use crate::dictation::pipeline::{PhoneMic, PhoneSink, PhoneTarget};
use crate::link::dictate::{self, Dictating, Talk};
use crate::link::pairing::{self, Verdict};
use crate::link::voice::{self, say, Tx, Wire};
use crate::link::wire::{self, DeviceCreds, In, Out, Phase};
use crate::link::{devices, Hub, Live};
use crate::voice::tts::Playback as _;

/// How long a phone has to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// The longest question: 2 minutes of 16 kHz audio.
pub const MAX_SAMPLES: usize = wire::UP_RATE as usize * 120;

/// Words the phone shows.
const NOTHING_HEARD: &str = "Yap didn't catch a question. Try again.";
const MEETING: &str = "The Voice Assistant is off during meetings. While a meeting records or a call is on, Yap doesn't listen or talk.";
const NO_SPEECH: &str = "Yap's speech model isn't ready on the PC.";

/// **One question's turn**, shared by the session, its task and the Voice
/// Assistant's path (`assistant::Phone`).
pub struct Turn {
    id: u64,
    session: u64,
    latest: Arc<AtomicU64>,
    tx: Tx,
    /// The answer's id, once its voice started ("chat:<conversation>:<index>").
    message_id: Mutex<Option<String>>,
    playback: Mutex<Option<Arc<voice::PhonePlayback>>>,
    speaking: Arc<AtomicBool>,
    /// The checked answer is in (after the voice: idle, not thinking).
    answered: Arc<AtomicBool>,
    /// A voice was started for it (its end says idle).
    voiced: AtomicBool,
}

impl Turn {
    fn new(id: u64, session: u64, latest: Arc<AtomicU64>, tx: Tx) -> Arc<Turn> {
        Arc::new(Turn {
            id,
            session,
            latest,
            tx,
            message_id: Mutex::new(None),
            playback: Mutex::new(None),
            speaking: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(false)),
            voiced: AtomicBool::new(false),
        })
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
        self.tell(Out::Answer { text: text.to_string(), done });
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
#[derive(Default)]
struct Asking {
    recording: bool,
    audio: Vec<f32>,
    turn: Option<Arc<Turn>>,
    started: Option<Instant>,
    /// The phone's dictation (recording, or being typed on the PC), and the
    /// pipeline's way in for it.
    dictation: Option<(Arc<Dictating>, PhoneMic)>,
}

impl Asking {
    /// The dictation still recording, if one is: the phone's frames are its.
    fn dictating(&self) -> Option<&(Arc<Dictating>, PhoneMic)> {
        self.dictation.as_ref().filter(|(d, _)| d.listening())
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
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Wire>();
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
        Verdict::Known(id) => Some(id),
        Verdict::Pair { code } => pair_phone(&hub, &tx, &mut incoming, &mut stop, &name, &code).await,
        Verdict::Denied(reason) => {
            say(&tx, Out::Denied { reason: reason.into() });
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
    hub.connected(session, Live { device: device_id.clone(), tx: tx.clone(), kill: Arc::clone(&kill) });
    say(&tx, hub.ready(&device_id));
    say(&tx, Out::State { phase: Phase::Idle });
    tracing::info!(session, "Yap Link: a phone connected");

    let mut asking = Asking::default();
    loop {
        let next = tokio::select! {
            m = incoming.next() => m,
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
                    Ok(m) => on_message(&hub, &tx, session, &device_id, &latest, &mut asking, m),
                    Err(e) => say(&tx, Out::Error { message: e }),
                }
            }
            Message::Binary(b) => {
                received += b.len() as u64;
                on_audio(&hub, &tx, session, &latest, &mut asking, &b);
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    // The phone left (or was removed): its question goes with it; a
    // dictation it was still talking into is cancelled on the PC (kept there
    // for Undo), one being typed finishes.
    if let Some(turn) = asking.turn.take() {
        turn.abandon();
    }
    if let Some((d, mic)) = asking.dictation.take() {
        if d.listening() {
            d.call_off();
            mic.cancel(d.session());
            tracing::info!(session, "Yap Link: the phone left mid-dictation; cancelled on the PC");
        }
    }
    hub.disconnected(session);
    let _ = tx.send(Wire::Close);
    drop(tx);
    let _ = writer.await;
    tracing::info!(session, "Yap Link: a phone disconnected");
    crate::privacy::net::phone(peer.ip(), 101, received, sent.load(Ordering::Relaxed));
}

/// The hello, judged (and rate-limited per address).
fn hello_verdict(hub: &Hub, peer: SocketAddr, v: u32, nonce: &str, device: Option<&DeviceCreds>, token: Option<&str>) -> Verdict {
    let now = Instant::now();
    if !hub.limiter().allow(peer.ip(), now) {
        tracing::info!("Yap Link: too many hellos from one address");
        return Verdict::Denied(pairing::TOO_MANY);
    }
    let hello = pairing::Hello { v, nonce, device: device.map(|d| (d.id.as_str(), d.secret.as_str())), token };
    let verdict = {
        let mut invite = hub.invite();
        pairing::judge(&hello, invite.as_mut(), &devices::secret_hash, now)
    };
    match &verdict {
        Verdict::Denied(_) => {
            hub.limiter().strike(peer.ip(), now);
            tracing::info!("Yap Link: a hello was refused");
        }
        Verdict::Pair { .. } => tracing::info!("Yap Link: a phone asks to pair"),
        Verdict::Known(_) => {}
    }
    verdict
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
/// cancel): one still being recorded is dropped, one being answered stops.
fn abandon(latest: &AtomicU64, asking: &mut Asking) {
    if asking.recording {
        asking.recording = false;
        asking.audio = Vec::new();
        latest.fetch_add(1, Ordering::SeqCst);
    }
    let Some(turn) = asking.turn.take() else { return };
    let was = crate::chat::assistant::Phone::current(turn.as_ref());
    turn.abandon();
    if was {
        tracing::info!("Yap Link: the phone interrupted its question");
    }
}

/// Call off the phone's dictation, if there is one: still recording, it's
/// cancelled on the PC (kept there a moment for Undo); being typed, nothing
/// is pasted. Its late news never reaches the phone.
fn call_off_dictation(asking: &mut Asking) {
    let Some((d, mic)) = asking.dictation.take() else { return };
    let recording = d.listening();
    d.call_off();
    if recording {
        mic.cancel(d.session());
    }
    tracing::info!(recording, "Yap Link: the phone called its dictation off");
}

/// A dictation still recording when the phone starts another talk: called
/// off (one being typed carries on, and is told about while it's current).
fn end_listening_dictation(asking: &mut Asking) {
    if asking.dictating().is_some() {
        call_off_dictation(asking);
    }
}

/// The pipeline's way in for a phone's dictation, if the pipeline runs.
fn phone_mic(hub: &Hub) -> Option<PhoneMic> {
    let state = hub.app.state::<crate::AppState>();
    let guard = state.pipeline.lock().unwrap_or_else(|p| p.into_inner());
    guard.as_ref().map(|p| p.phone_mic())
}

/// **A dictation starts** (`start {mode: "dictate"}`): refused in words
/// (`dictate::refusal`) — "Dictate to PC" without this phone's permission,
/// during a meeting, beside a dictation on the PC — else recording, the
/// phone's frames going into it.
fn start_dictation(hub: &Arc<Hub>, tx: &Tx, device: &str, latest: &Arc<AtomicU64>, asking: &mut Asking, to: PhoneTarget) {
    let refuse = |why: &str| {
        say(tx, Out::Error { message: why.to_string() });
        say(tx, Out::State { phase: Phase::Idle });
    };
    let Some(mic) = phone_mic(hub) else { return refuse(dictate::NO_SPEECH) };
    let may_type = devices::may_type(device);
    if let Some(why) = dictate::refusal(to, may_type, crate::meetings::meeting::is_recording(), mic.busy()) {
        tracing::info!(to = dictate::target_name(to), may_type, "Yap Link: a dictation from the phone refused");
        return refuse(why);
    }
    // Its own talk: an older one's news is dropped from here.
    let id = latest.fetch_add(1, Ordering::SeqCst) + 1;
    let d = Dictating::new(id, Arc::clone(latest), tx.clone(), to);
    match mic.start(to, Arc::clone(&d) as Arc<dyn PhoneSink>) {
        Ok(rec) => {
            d.session.store(rec, Ordering::SeqCst);
            asking.dictation = Some((d, mic));
            tracing::info!(to = dictate::target_name(to), "Yap Link: a dictation from the phone");
            say(tx, Out::State { phase: Phase::Listening });
        }
        // A dictation started on the PC in between.
        Err(busy) => refuse(dictate::refusal(to, true, false, Some(busy)).unwrap_or(dictate::FINISHING)),
    }
}

fn on_message(hub: &Arc<Hub>, tx: &Tx, session: u64, device: &str, latest: &Arc<AtomicU64>, asking: &mut Asking, m: In) {
    match m {
        In::Ping => say(tx, Out::Pong),
        In::Hello { .. } => say(tx, Out::Error { message: "Already connected.".into() }),
        In::Start { mode, target } => {
            // A start while it speaks (or thinks) is an interrupt, then a
            // start; a dictation still being said is called off.
            abandon(latest, asking);
            end_listening_dictation(asking);
            let talk = match dictate::talk(&mode, &target) {
                Ok(t) => t,
                Err(e) => {
                    say(tx, Out::Error { message: e.into() });
                    say(tx, Out::State { phase: Phase::Idle });
                    return;
                }
            };
            if let Talk::Dictate(to) = talk {
                return start_dictation(hub, tx, device, latest, asking, to);
            }
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
        In::Stop => {
            if let Some((d, mic)) = asking.dictating() {
                // Transcribed and typed, as the shortcut's second press does.
                mic.stop(d.session());
            } else if asking.recording {
                finish_question(hub, tx, session, latest, asking);
            }
        }
        // Idle at once, whatever there was to drop: the page waits for it
        // before it takes the next question's news. (A cancel calls a
        // dictation off; an interrupt is about an answer.)
        In::Cancel => {
            abandon(latest, asking);
            call_off_dictation(asking);
            say(tx, Out::State { phase: Phase::Idle });
        }
        In::Interrupt => {
            abandon(latest, asking);
            say(tx, Out::State { phase: Phase::Idle });
        }
        In::SetModel { id } => {
            // A fresh `ready` either way: the page shows the pick at once and
            // takes this as the truth (every other phone hears it too, from
            // `yap-chat-model-changed`).
            if let Err(e) = crate::link::models::set(&hub.app, &id) {
                say(tx, Out::Error { message: e });
            }
            say(tx, hub.ready(device));
        }
    }
}

fn on_audio(hub: &Arc<Hub>, tx: &Tx, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking, bytes: &[u8]) {
    // A dictation: its frames go into the pipeline's recording.
    if let Some((d, mic)) = asking.dictating() {
        if let Ok(frame) = wire::decode(bytes) {
            if frame.kind == wire::UP {
                let samples: Vec<f32> = wire::to_f32(&frame.samples).collect();
                mic.audio(d.session(), &samples);
            }
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
        finish_question(hub, tx, session, latest, asking);
    }
}

/// The question's audio is in: transcribe it and ask.
fn finish_question(hub: &Arc<Hub>, tx: &Tx, session: u64, latest: &Arc<AtomicU64>, asking: &mut Asking) {
    asking.recording = false;
    let audio = std::mem::take(&mut asking.audio);
    let id = latest.load(Ordering::SeqCst);
    let turn = Turn::new(id, session, Arc::clone(latest), tx.clone());
    asking.turn = Some(Arc::clone(&turn));
    let held = asking.started.take().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
    tracing::info!(samples = audio.len(), held_ms = held, "Yap Link: a question from the phone");
    let app = hub.app.clone();
    tauri::async_runtime::spawn(async move { ask(app, turn, audio).await });
}

/// One question: transcribed, heard, answered (the Voice Assistant's path).
async fn ask(app: tauri::AppHandle, turn: Arc<Turn>, audio: Vec<f32>) {
    use crate::chat::assistant::Phone;
    turn.tell(Out::State { phase: Phase::Transcribing });
    let ear = {
        let state = app.state::<crate::AppState>();
        let guard = state.pipeline.lock().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().map(|p| p.phone_ear())
    };
    let heard = match ear {
        Some(ear) => ear.question(audio).await,
        None => Err(NO_SPEECH.to_string()),
    };
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
    turn.tell(Out::Heard { text: question.clone(), to: None });
    if crate::chat::assistant::is_off() {
        turn.tell(Out::Error { message: MEETING.into() });
        turn.tell(Out::State { phase: Phase::Idle });
        return;
    }
    turn.tell(Out::State { phase: Phase::Thinking });
    let started = Instant::now();
    crate::chat::assistant::ask_from_phone(app, question, Arc::clone(&turn) as Arc<dyn Phone>).await;
    tracing::info!(ms = started.elapsed().as_millis() as u64, spoken = turn.voiced.load(Ordering::SeqCst), "Yap Link: the phone's question answered");
    // No voice (nothing to say, or it couldn't start): idle now; else its end says so.
    if !turn.voiced.load(Ordering::SeqCst) {
        turn.tell(Out::State { phase: Phase::Idle });
    }
}
