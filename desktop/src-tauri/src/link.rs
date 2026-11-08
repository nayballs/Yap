//! **Yap Link, home preview** (2026-10-31): talk to Yap's AI from your
//! **phone** while you walk round the house — a Bluetooth headset on the
//! phone, the answer in your ear. The phone opens a page Yap serves itself
//! over HTTPS on the home Wi-Fi; a WebSocket carries the question's audio
//! up and Yap's voice down, and the question goes the Voice Assistant's way
//! (`chat/assistant.rs`: Chat's model, the short spoken answer, kept in
//! Chat). Off by default: Settings → Phone → "Use Yap from your phone (on
//! your home Wi-Fi)" (`link_enabled`).
//!
//! Since 2026-11-03 the page is Yap's Chat on a phone: a question can be
//! typed too (`ask`, answered in the same conversation and not spoken; ▶
//! under an answer has it said, `read_aloud`, only an answer that phone was
//! given: [`Given`]), and the model chip opens Chat's own picker (its rows
//! built by `desktop/src/lib/core/chatPicker.js`, served beside the page;
//! `models.rs` `picker` sends what the desktop picker reads). Never
//! dictation: the phone types nothing into the PC's apps.
//!
//! Later that day, in the Claude app's layout: the phone keeps **its own
//! conversation** (a drawer of Chat's conversations, `chats.rs`; one opened,
//! or a new chat) and a question carries **pictures** taken or chosen on the
//! phone (`pictures.rs`), sent as Chat sends a typed question's screenshots.
//!
//! The parts:
//! - `server.rs` — HTTPS on TCP 51443, home-network peers only, the page's
//!   files (`page.rs`, embedded) and `/ws`;
//! - `cert.rs` — the self-signed certificate, made once (`<data>/link/`);
//! - `pairing.rs` — the QR's one-time token, the 4-digit code both screens
//!   show, who may say hello, hellos per address;
//! - `devices.rs` — paired phones (`link_devices.json`: only a hash of each
//!   phone's secret);
//! - `session.rs` — one phone's conversation, its questions one at a time;
//! - `voice.rs` — Yap's voice rendered for the phone, never this PC's speakers;
//! - `models.rs` — Chat's model, listed for and switched from the phone;
//! - `chats.rs` — Chat's conversations for the phone's drawer, a thread opened;
//! - `pictures.rs` — a question's pictures as they come from the phone;
//! - `lan.rs` — this PC's addresses, which peers may connect, Public networks;
//! - `wire.rs` — the protocol (v1, shared with the phone's page, `phone/web/page/`;
//!   the Code side's part defined once in `desktop/src/lib/core/codeWire.js`);
//! - `code.rs` (+ `code/gates.rs`, `code/view.rs`) — **Remote Control** (2026-11-08):
//!   Yap Code from a paired phone, every message through the gates;
//! - `lanes.rs` — a phone's way out in two lanes: the control lane (capped)
//!   and the watched agent's lane (coalesced, bounded, resync when full).
//!
//! The long-term design (a PWA on Yap's website over WebRTC, the private
//! repo's `phone-remote-design.md`) keeps these messages: moving there is a
//! change of transport. A signed-out Yap contacts no service for any of
//! this: it only answers phones on the home network.

pub mod cert;
pub mod chats;
pub mod code;
pub mod devices;
pub mod lan;
pub mod lanes;
pub mod models;
pub mod page;
pub mod pairing;
pub mod pictures;
pub mod server;
pub mod session;
pub mod voice;
pub mod wire;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener};

use crate::link::pairing::{Invite, Limiter};
use crate::link::voice::{say, Tx};
use crate::link::wire::Out;

/// The port Yap Link listens on.
pub const PORT: u16 = 51443;
/// Settings → Phone follows this (pairing, a phone connected or gone).
pub const CHANGED: &str = "yap-link-changed";
/// The main window asks "Allow ‘<name>’ to use Yap? Code 4821".
pub const PAIR: &str = "yap-link-pair";
/// That question went (answered, or the phone gave up).
pub const PAIR_DONE: &str = "yap-link-pair-done";
/// A phone paired already brought Settings' code (`{name}`): the code has
/// done its job (it's used up), and Settings → Phone says so.
pub const ALREADY_PAIRED: &str = "yap-link-already-paired";

/// A phone asking to pair, waiting for the person.
struct Ask {
    name: String,
    code: String,
    answer: tokio::sync::oneshot::Sender<bool>,
}

/// A connected phone.
pub struct Live {
    pub device: String,
    /// Its name as it said hello ("Android · Firefox").
    pub name: String,
    pub tx: Tx,
    /// Cuts it off (it was removed).
    pub kill: Arc<tokio::sync::Notify>,
    /// The conversation its questions go to (its session's).
    pub chat: Arc<Mutex<wire::ChatRef>>,
    /// Its Code side (Remote Control).
    pub code: Arc<code::PhoneCode>,
    /// Asks its session to do something (an agent's report said there).
    pub cmd: tokio::sync::mpsc::UnboundedSender<session::Cmd>,
}

/// **What the running server shares** with its connections.
pub struct Hub {
    pub app: AppHandle,
    pub tls: Arc<rustls::ServerConfig>,
    invite: Mutex<Option<Invite>>,
    limiter: Mutex<Limiter>,
    asks: Mutex<HashMap<u64, Ask>>,
    live: Mutex<HashMap<u64, Live>>,
    /// The answers each paired phone was given: ▶ on the phone reads one of
    /// these, never anything else in Chat.
    answers: Mutex<Given>,
    ids: AtomicU64,
    /// Remote Control's own (`code.rs`): which phone was sent which card…
    pub code: code::Shared,
}

/// The answers kept per phone for ▶.
const KEEP_ANSWERS: usize = 50;

/// **The answers each paired phone was given** (their ids in Chat, newest
/// last, [`KEEP_ANSWERS`] at most a phone, while Yap runs). Pure.
#[derive(Default)]
pub struct Given(HashMap<String, std::collections::VecDeque<String>>);

impl Given {
    /// Phone `device` was given answer `id`.
    pub fn gave(&mut self, device: &str, id: &str) {
        let list = self.0.entry(device.to_string()).or_default();
        list.retain(|x| x != id);
        list.push_back(id.to_string());
        while list.len() > KEEP_ANSWERS {
            list.pop_front();
        }
    }

    /// Was phone `device` given answer `id`?
    pub fn has(&self, device: &str, id: &str) -> bool {
        self.0.get(device).is_some_and(|l| l.iter().any(|x| x == id))
    }
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A pairing question, as the main window shows it.
#[derive(Debug, Clone, Serialize)]
pub struct AskView {
    pub id: u64,
    pub name: String,
    pub code: String,
}

impl Hub {
    pub fn next_id(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn limiter(&self) -> MutexGuard<'_, Limiter> {
        locked(&self.limiter)
    }

    pub fn invite(&self) -> MutexGuard<'_, Option<Invite>> {
        locked(&self.invite)
    }

    /// **Ask the person** whether phone `name` may pair (the main window's
    /// question, the window brought up): the answer comes on the receiver.
    pub fn ask_to_pair(&self, name: &str, code: &str) -> (u64, tokio::sync::oneshot::Receiver<bool>) {
        let id = self.next_id();
        let (answer, rx) = tokio::sync::oneshot::channel();
        locked(&self.asks).insert(id, Ask { name: name.to_string(), code: code.to_string(), answer });
        let _ = self.app.emit(PAIR, AskView { id, name: name.to_string(), code: code.to_string() });
        if let Err(e) = crate::shell::window::show_settings(&self.app) {
            tracing::warn!("Yap Link: couldn't bring up the main window ({e})");
        }
        (id, rx)
    }

    /// Question `id` is over: it goes from the main window.
    pub fn pair_done(&self, id: u64) {
        locked(&self.asks).remove(&id);
        let _ = self.app.emit(PAIR_DONE, serde_json::json!({ "id": id }));
    }

    /// The person answered question `id`. `false`: it's gone already.
    fn answer(&self, id: u64, allow: bool) -> bool {
        let ask = locked(&self.asks).remove(&id);
        match ask {
            Some(a) => a.answer.send(allow).is_ok(),
            None => false,
        }
    }

    fn asks(&self) -> Vec<AskView> {
        let mut v: Vec<AskView> = locked(&self.asks).iter().map(|(id, a)| AskView { id: *id, name: a.name.clone(), code: a.code.clone() }).collect();
        v.sort_by_key(|a| a.id);
        v
    }

    pub fn connected(&self, session: u64, live: Live) {
        locked(&self.live).insert(session, live);
        let _ = self.app.emit(CHANGED, ());
        crate::bar::refresh(&self.app);
    }

    pub fn disconnected(&self, session: u64) {
        locked(&self.live).remove(&session);
        let _ = self.app.emit(CHANGED, ());
        crate::bar::refresh(&self.app);
    }

    fn connected_devices(&self) -> Vec<String> {
        locked(&self.live).values().map(|l| l.device.clone()).collect()
    }

    /// `ready` for phone `device`: this PC, Chat's model and the others,
    /// what it answers beyond v1, what Chat's picker shows, and whether
    /// Remote Control is on for it (`code.rs`).
    pub fn ready_for(&self, device: &str) -> Out {
        let cfg = crate::config::load();
        let (model, models) = models::listed(&cfg);
        Out::Ready { pc: lan::pc_name(), model, models, can: wire::CAN.to_vec(), picker: Some(models::picker(&cfg)), code: Some(code::ready_code(device)) }
    }

    /// Phone `device` was given answer `id` (▶ may read it).
    pub fn gave(&self, device: &str, id: &str) {
        locked(&self.answers).gave(device, id);
    }

    /// Was phone `device` given answer `id`?
    pub fn was_given(&self, device: &str, id: &str) -> bool {
        locked(&self.answers).has(device, id)
    }

    /// Chat's model changed (here, in Chat, from a phone): every phone hears.
    fn broadcast_ready(&self) {
        let live: Vec<(Tx, String)> = locked(&self.live).values().map(|l| (l.tx.clone(), l.device.clone())).collect();
        for (tx, device) in live {
            say(&tx, self.ready_for(&device));
        }
    }

    /// Phone `device` was removed: cut off now.
    fn cut_off(&self, device: &str) {
        for l in locked(&self.live).values().filter(|l| l.device == device) {
            l.kill.notify_one();
        }
    }

    /// Link went off: the pairing questions go (each phone hears `stop`'s
    /// signal itself, is told and closed: `session.rs`).
    fn close_all(&self) {
        let ids: Vec<u64> = locked(&self.asks).drain().map(|(id, _)| id).collect();
        for id in ids {
            let _ = self.app.emit(PAIR_DONE, serde_json::json!({ "id": id }));
        }
    }
}

/// The server while it runs.
struct Running {
    hub: Arc<Hub>,
    stop: tokio::sync::watch::Sender<bool>,
    port: u16,
    listening: Vec<tauri::EventId>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Why the server didn't start, for Settings.
static ERROR: Mutex<Option<String>> = Mutex::new(None);

/// The port to listen on: 51443; a test run's own (`YAP_E2E_LINK_PORT`, 0 =
/// any free one, `link_status` says which).
fn port() -> u16 {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return std::env::var("YAP_E2E_LINK_PORT").ok().and_then(|p| p.trim().parse().ok()).unwrap_or(0);
    }
    PORT
}

/// On or off, as Settings → Phone says (at start and on every save).
pub fn sync(app: &AppHandle, enabled: bool) {
    if enabled {
        start(app);
    } else {
        stop();
    }
}

fn start(app: &AppHandle) {
    let mut running = locked(&RUNNING);
    if running.is_some() {
        return;
    }
    *locked(&ERROR) = None;
    let addresses: Vec<IpAddr> = lan::addresses().into_iter().map(|a| a.ip).collect();
    let primary = addresses.first().copied().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let tls = match cert::load_or_make(primary, &addresses, &lan::pc_name()).and_then(|(c, k)| cert::server_config(c, k)) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Yap Link: no certificate ({e})");
            *locked(&ERROR) = Some(e);
            return;
        }
    };
    let wanted = port();
    // Every interface; a test run's on this PC only (its phone is too, and
    // Windows' firewall never asks about a test copy).
    let any = if crate::shell::e2e::active() { "127.0.0.1" } else { "0.0.0.0" };
    let listener = match std::net::TcpListener::bind((any, wanted)) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(port = wanted, "Yap Link: couldn't listen ({e})");
            *locked(&ERROR) = Some(format!(
                "Yap couldn't use port {wanted}: another program may be using it. Turn the switch off and on to try again."
            ));
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(wanted);
    // IPv6 too where it can (the same port; a test run: IPv4 loopback only).
    let v6 = (wanted != 0 && any == "0.0.0.0").then(|| std::net::TcpListener::bind(("::", port)).ok()).flatten();
    let _ = listener.set_nonblocking(true);
    if let Some(l) = &v6 {
        let _ = l.set_nonblocking(true);
    }
    let hub = Arc::new(Hub {
        app: app.clone(),
        tls,
        invite: Mutex::new(None),
        limiter: Mutex::new(Limiter::default()),
        asks: Mutex::new(HashMap::new()),
        live: Mutex::new(HashMap::new()),
        answers: Mutex::new(Given::default()),
        ids: AtomicU64::new(0),
        code: code::Shared::default(),
    });
    let (stop, stopped) = tokio::sync::watch::channel(false);
    for l in std::iter::once(listener).chain(v6) {
        let hub = Arc::clone(&hub);
        let stopped = stopped.clone();
        tauri::async_runtime::spawn(async move {
            match tokio::net::TcpListener::from_std(l) {
                Ok(l) => server::serve(l, hub, stopped).await,
                Err(e) => tracing::warn!("Yap Link: couldn't listen ({e})"),
            }
        });
    }
    let listen_hub = Arc::clone(&hub);
    let mut listening = vec![app.listen("yap-chat-model-changed", move |_| listen_hub.broadcast_ready())];
    // Remote Control (`code.rs`): what Yap Code says goes to the phones
    // that may follow it (each filtered there: the switches, the workspace).
    type OnEvent = fn(&Arc<Hub>, &serde_json::Value);
    let code_events: [(&str, OnEvent); 5] = [
        (crate::integrations::claude_live::APPROVE_EVENT, code::on_approve),
        (crate::integrations::claude_live::APPROVE_DONE_EVENT, code::on_approve_done),
        (crate::chat::turn::AGENT_REPORT_EVENT, code::on_report),
        (code::MODE_EVENT, code::on_mode),
        (crate::chat::stream::ANSWERING_EVENT, code::on_answering),
    ];
    for (event, on) in code_events {
        let h = Arc::clone(&hub);
        listening.push(app.listen(event, move |e| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(e.payload()) {
                on(&h, &v);
            }
        }));
    }
    code::on_answering(&hub, &crate::chat::stream::chat_answering());
    code::refresh(&crate::config::load());
    tracing::info!(port, addresses = addresses.len(), "Yap Link: listening for phones on the home network");
    *running = Some(Running { hub, stop, port, listening });
    drop(running);
    let _ = app.emit(CHANGED, ());
}

/// Off: no more phones, the connected ones told and cut off.
pub fn stop() {
    let Some(r) = locked(&RUNNING).take() else { return };
    let _ = r.stop.send(true);
    r.hub.close_all();
    for id in r.listening {
        r.hub.app.unlisten(id);
    }
    tracing::info!("Yap Link: off");
    let _ = r.hub.app.emit(CHANGED, ());
}

fn hub() -> Option<Arc<Hub>> {
    locked(&RUNNING).as_ref().map(|r| Arc::clone(&r.hub))
}

/// Conversation `id` was renamed in Chat: every connected phone hears
/// (its drawer, and its header when it's in that one).
pub fn titled(id: u64, title: &str) {
    let Some(h) = hub() else { return };
    let out = Out::Title { id, title: title.to_string() };
    for l in locked(&h.live).values() {
        say(&l.tx, out.clone());
    }
}

// ---- Remote Control (2026-11-08, `code.rs`) -----------------------------------------------

/// **An agent's event** (`agents::runner::emit`, `line` its log line): the
/// phones watching that agent hear it (Remote Control).
pub fn agent_event(id: &str, line: Option<u64>, event: serde_json::Value) {
    if let Some(h) = hub() {
        code::agent_event(&h, id, line, event);
    }
}

/// **The config was saved**: Remote Control's switches again, and every
/// phone told when they changed what it may do; the bar's phone mark; the
/// PC kept awake or not.
pub fn config_changed(prev: &crate::config::YapConfig, cfg: &crate::config::YapConfig) {
    code::refresh(cfg);
    let changed = prev.link_remote_control != cfg.link_remote_control || prev.agents_enabled != cfg.agents_enabled || prev.link_keep_awake != cfg.link_keep_awake;
    if !changed {
        return;
    }
    tracing::info!(on = cfg.link_remote_control, "Remote Control: its switches changed");
    if let Some(h) = hub() {
        code::ready_all(&h);
        crate::bar::refresh(&h.app);
    }
    crate::shell::awake::poke();
}

/// **The phone the bar's mark is for**: the first connected phone that may
/// use Remote Control now (its name).
pub fn remote_phone() -> Option<String> {
    let h = hub()?;
    let live: Vec<(String, String)> = locked(&h.live).values().map(|l| (l.device.clone(), l.name.clone())).collect();
    live.into_iter().find(|(d, _)| code::on_for(d)).map(|(_, n)| n)
}

/// Is phone `device` connected now?
pub fn phone_connected(device: &str) -> bool {
    hub().is_some_and(|h| locked(&h.live).values().any(|l| l.device == device))
}

/// How long an agent's report waits to be said on its phone.
const REPORT_WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// **An agent's report, said on the phone that asked for it** (never this
/// PC's speakers): once Yap's voice and the phone are free (up to 5
/// minutes). `false`: that phone isn't connected (or stayed busy), so it
/// isn't said at all.
pub async fn say_report(device: &str, text: &str, message_id: &str) -> bool {
    let since = std::time::Instant::now();
    loop {
        if crate::voice::tts::meeting_quiet().is_some() {
            tracing::info!("Remote Control: a report not said on the phone (a meeting or a call)");
            return false;
        }
        let Some(cmd) = hub().and_then(|h| locked(&h.live).values().find(|l| l.device == device).map(|l| l.cmd.clone())) else {
            tracing::info!("Remote Control: a report not said (its phone isn't connected)");
            return false;
        };
        if crate::voice::tts::speaking_id().is_none() {
            let (reply, said) = tokio::sync::oneshot::channel();
            let sent = cmd.send(session::Cmd::Say { text: text.to_string(), message_id: message_id.to_string(), reply }).is_ok();
            if sent && said.await.unwrap_or(false) {
                tracing::info!(chars = text.chars().count(), "Remote Control: a report said on the phone that asked");
                return true;
            }
        }
        if since.elapsed() > REPORT_WAIT {
            tracing::info!("Remote Control: a report not said on the phone (it stayed busy)");
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// A phone in Remote Control's lists (Settings → Phone, the Environment card).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePhone {
    pub id: String,
    pub name: String,
    pub connected: bool,
    /// Ticked for Remote Control.
    pub remote: bool,
}

/// **Remote Control's state** for a page: the switch and what it needs,
/// workspace `workspace`'s switch, the paired phones.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    /// Settings → Phone → Remote Control.
    pub on: bool,
    /// Yap Link on (Use Yap from your phone).
    pub link: bool,
    /// Yap Code on.
    pub yap_code: bool,
    /// Its workspace allows it (none asked: true).
    pub workspace_allowed: bool,
    pub phones: Vec<RemotePhone>,
}

/// The Environment card's and Settings' Remote Control (`workspace`: a
/// workspace's name, for its switch).
#[tauri::command]
pub fn link_remote_status(workspace: Option<String>) -> RemoteStatus {
    let cfg = crate::config::load();
    let connected = hub().map(|h| h.connected_devices()).unwrap_or_default();
    RemoteStatus {
        on: cfg.link_remote_control,
        link: cfg.link_enabled,
        yap_code: cfg.agents_enabled,
        workspace_allowed: workspace.as_deref().is_none_or(crate::agents::remote::workspace_allows),
        phones: devices::list()
            .into_iter()
            .map(|d| RemotePhone { connected: connected.contains(&d.id), remote: crate::agents::remote::phone_on(&d.id), id: d.id, name: d.name })
            .collect(),
    }
}

/// **Tick a phone for Remote Control** (Settings → Phone), or not: it's
/// told at once what it may do now.
#[tauri::command]
pub fn link_phone_remote(app: AppHandle, id: String, on: bool) -> bool {
    if !devices::list().iter().any(|d| d.id == id) {
        return false;
    }
    let changed = crate::agents::remote::set_phone(&id, on);
    tracing::info!(on, changed, "Remote Control: a phone's tick");
    if let Some(h) = hub() {
        code::ready_all(&h);
    }
    let _ = app.emit(CHANGED, ());
    crate::bar::refresh(&app);
    changed
}

/// **A workspace's Remote Control switch** (its Environment card): off, no
/// phone sees or acts on it.
#[tauri::command]
pub fn link_workspace_remote(app: AppHandle, name: String, on: bool) -> bool {
    let changed = crate::agents::remote::set_workspace(&name, on);
    tracing::info!(on, changed, "Remote Control: a workspace's switch");
    if let Some(h) = hub() {
        // Its agents' watchers let go at their next event; the dots now.
        code::dirty(&h);
    }
    let _ = app.emit(CHANGED, ());
    changed
}

// ---- commands ---------------------------------------------------------------------------

/// One of this PC's addresses, for Settings' choice.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressView {
    pub ip: String,
    pub adapter: String,
}

/// A paired phone, for Settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    pub id: String,
    pub name: String,
    pub paired: u64,
    pub last_seen: u64,
    pub connected: bool,
    /// Ticked for Remote Control.
    pub remote: bool,
    /// Its last remote action ("Allowed a step in garden-planner") and when
    /// (unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_action_ts: Option<u64>,
}

/// Settings → Phone's state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub on: bool,
    pub running: bool,
    pub port: u16,
    pub error: Option<String>,
    pub pc: String,
    pub addresses: Vec<AddressView>,
    /// The network of the QR's address is one Windows marks Public (`None`:
    /// Windows didn't say): phones are refused there until it's Private.
    pub public: Option<bool>,
    pub phones: Vec<PhoneView>,
    pub asks: Vec<AskView>,
}

/// Settings → Phone: on or off, listening where, the paired phones. (Off
/// the main thread: Windows' network lists are COM.)
#[tauri::command]
pub async fn link_status() -> Status {
    tokio::task::spawn_blocking(status).await.unwrap_or_else(|_| status_without_networks())
}

fn status() -> Status {
    let mut s = status_without_networks();
    let addresses = lan::addresses();
    s.public = addresses.first().and_then(|a| lan::public_network(a.ip));
    s.addresses = addresses.into_iter().map(|a| AddressView { ip: a.ip.to_string(), adapter: a.adapter }).collect();
    s
}

fn status_without_networks() -> Status {
    let cfg = crate::config::load();
    let (running, port, hub) = {
        let r = locked(&RUNNING);
        (r.is_some(), r.as_ref().map(|r| r.port).unwrap_or(PORT), r.as_ref().map(|r| Arc::clone(&r.hub)))
    };
    let connected = hub.as_ref().map(|h| h.connected_devices()).unwrap_or_default();
    Status {
        on: cfg.link_enabled,
        running,
        port,
        error: locked(&ERROR).clone(),
        pc: lan::pc_name(),
        addresses: Vec::new(),
        public: None,
        phones: devices::list()
            .into_iter()
            .map(|d| {
                let last = crate::agents::remote::last_of(&d.id);
                PhoneView {
                    connected: connected.contains(&d.id),
                    remote: crate::agents::remote::phone_on(&d.id),
                    last_action: last.as_ref().map(|(w, _)| w.clone()),
                    last_action_ts: last.map(|(_, ts)| ts),
                    id: d.id,
                    name: d.name,
                    paired: d.paired,
                    last_seen: d.last_seen,
                }
            })
            .collect(),
        asks: hub.map(|h| h.asks()).unwrap_or_default(),
    }
}

/// Windows' network settings, where a network is made Private (Settings →
/// Phone's warning on a Public one).
#[tauri::command]
pub fn link_network_settings(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url("ms-settings:network-status", None::<&str>).map_err(|e| format!("Couldn't open Windows' network settings: {e}"))
}

/// The pairing link: its address, and its QR code.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairView {
    pub url: String,
    pub address: String,
    pub qr: Option<crate::auth::Qr>,
    /// How long it lasts (ms).
    pub expires_ms: u64,
}

/// The URL a phone opens: `https://<address>:<port>/#pair=<token>`. Pure.
pub fn pair_url(ip: IpAddr, port: u16, token: &str) -> String {
    match ip {
        IpAddr::V6(v6) => format!("https://[{v6}]:{port}/#pair={token}"),
        IpAddr::V4(v4) => format!("https://{v4}:{port}/#pair={token}"),
    }
}

/// **Pair a phone**: a new invitation (any earlier one is no more) and its
/// link and QR, on `address` (one of this PC's; else the best).
#[tauri::command]
pub fn link_pair_start(address: Option<String>) -> Result<PairView, String> {
    let (hub, port) = {
        let r = locked(&RUNNING);
        let r = r.as_ref().ok_or("Turn on Use Yap from your phone first.")?;
        (Arc::clone(&r.hub), r.port)
    };
    let all = lan::addresses();
    let ip = address
        .and_then(|a| a.parse::<IpAddr>().ok())
        .filter(|ip| all.iter().any(|a| a.ip == *ip))
        .or_else(|| all.first().map(|a| a.ip))
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let invite = Invite::new()?;
    let url = pair_url(ip, port, &invite.token);
    *hub.invite() = Some(invite);
    // The person at the PC is pairing on purpose: earlier refusals (a
    // phone's tabs with old codes, say) don't stand in the new code's way.
    let forgiven = hub.limiter().forgive();
    tracing::info!(forgiven, "Yap Link: a pairing link made");
    Ok(PairView { qr: crate::auth::qr_for(&url), url, address: ip.to_string(), expires_ms: pairing::INVITE_FOR.as_millis() as u64 })
}

/// The pairing link is closed: it pairs nothing now.
#[tauri::command]
pub fn link_pair_cancel() {
    if let Some(h) = hub() {
        *h.invite() = None;
    }
}

/// The person's answer to "Allow ‘<name>’ to use Yap?".
#[tauri::command]
pub fn link_pair_answer(id: u64, allow: bool) -> bool {
    tracing::info!(allow, "Yap Link: the person answered a pairing");
    hub().is_some_and(|h| h.answer(id, allow))
}

/// Forget a paired phone, and cut it off if it's connected.
#[tauri::command]
pub fn link_remove(app: AppHandle, id: String) -> bool {
    let gone = devices::remove(&id);
    // Its Remote Control tick and marks go with it (its trail stays).
    crate::agents::remote::forget_phone(&id);
    if let Some(h) = hub() {
        h.cut_off(&id);
        code::forget(&h, &id);
    }
    tracing::info!(gone, "Yap Link: a phone removed");
    let _ = app.emit(CHANGED, ());
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_reads_aloud_only_the_answers_it_was_given() {
        let mut given = Given::default();
        given.gave("p-one", "chat:3:1");
        given.gave("p-one", "chat:3:3");
        assert!(given.has("p-one", "chat:3:1") && given.has("p-one", "chat:3:3"));
        // Another phone's, and anything else in Chat: no.
        assert!(!given.has("p-two", "chat:3:1"));
        assert!(!given.has("p-one", "chat:4:1"));
        // The newest kept, the oldest forgotten; a repeat moves to the end.
        for i in 0..KEEP_ANSWERS {
            given.gave("p-one", &format!("chat:9:{i}"));
        }
        assert!(!given.has("p-one", "chat:3:1"));
        given.gave("p-one", "chat:9:0");
        given.gave("p-one", "chat:10:1");
        assert!(given.has("p-one", "chat:9:0"), "given again: kept");
        assert!(!given.has("p-one", "chat:9:1"));
    }

    #[test]
    fn the_pairing_link_carries_the_address_port_and_token() {
        assert_eq!(pair_url("192.168.1.20".parse().unwrap(), 51443, "abc"), "https://192.168.1.20:51443/#pair=abc");
        assert_eq!(pair_url("fd00::20".parse().unwrap(), 51443, "abc"), "https://[fd00::20]:51443/#pair=abc");
    }
}
