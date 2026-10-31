//! **Yap Link, home preview** (2026-10-31): talk to Yap's AI from your
//! **phone** while you walk round the house — a Bluetooth headset on the
//! phone, the answer in your ear. The phone opens a page Yap serves itself
//! over HTTPS on the home Wi-Fi; a WebSocket carries the question's audio
//! up and Yap's voice down, and the question goes the Voice Assistant's way
//! (`chat/assistant.rs`: Chat's model, the short spoken answer, kept in
//! Chat). Off by default: Settings → Phone → "Use Yap from your phone (on
//! your home Wi-Fi)" (`link_enabled`).
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
//! - `lan.rs` — this PC's addresses, which peers may connect, Public networks;
//! - `wire.rs` — the protocol (v1, shared with `src-tauri/link-page/`).
//!
//! The long-term design (a PWA on Yap's website over WebRTC, the private
//! repo's `phone-remote-design.md`) keeps these messages: moving there is a
//! change of transport. A signed-out Yap contacts no service for any of
//! this: it only answers phones on the home network.

pub mod cert;
pub mod devices;
pub mod lan;
pub mod models;
pub mod page;
pub mod pairing;
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

/// A phone asking to pair, waiting for the person.
struct Ask {
    name: String,
    code: String,
    answer: tokio::sync::oneshot::Sender<bool>,
}

/// A connected phone.
pub struct Live {
    pub device: String,
    pub tx: Tx,
    /// Cuts it off (it was removed).
    pub kill: Arc<tokio::sync::Notify>,
}

/// **What the running server shares** with its connections.
pub struct Hub {
    pub app: AppHandle,
    pub tls: Arc<rustls::ServerConfig>,
    invite: Mutex<Option<Invite>>,
    limiter: Mutex<Limiter>,
    asks: Mutex<HashMap<u64, Ask>>,
    live: Mutex<HashMap<u64, Live>>,
    ids: AtomicU64,
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
    }

    pub fn disconnected(&self, session: u64) {
        locked(&self.live).remove(&session);
        let _ = self.app.emit(CHANGED, ());
    }

    fn connected_devices(&self) -> Vec<String> {
        locked(&self.live).values().map(|l| l.device.clone()).collect()
    }

    /// `ready`: this PC, Chat's model and the others.
    pub fn ready(&self) -> Out {
        let (model, models) = models::listed(&crate::config::load());
        Out::Ready { pc: lan::pc_name(), model, models }
    }

    /// Chat's model changed (here, in Chat, from a phone): every phone hears.
    fn broadcast_ready(&self) {
        let ready = self.ready();
        for l in locked(&self.live).values() {
            say(&l.tx, ready.clone());
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
    listening: tauri::EventId,
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
    let listener = match std::net::TcpListener::bind(("0.0.0.0", wanted)) {
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
    // IPv6 too where it can (the same port; a test run's any-port: IPv4 only).
    let v6 = (wanted != 0).then(|| std::net::TcpListener::bind(("::", port)).ok()).flatten();
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
        ids: AtomicU64::new(0),
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
    let listening = app.listen("yap-chat-model-changed", move |_| listen_hub.broadcast_ready());
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
    r.hub.app.unlisten(r.listening);
    tracing::info!("Yap Link: off");
    let _ = r.hub.app.emit(CHANGED, ());
}

fn hub() -> Option<Arc<Hub>> {
    locked(&RUNNING).as_ref().map(|r| Arc::clone(&r.hub))
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
    pub phones: Vec<PhoneView>,
    pub asks: Vec<AskView>,
}

/// Settings → Phone: on or off, listening where, the paired phones.
#[tauri::command]
pub fn link_status() -> Status {
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
        addresses: lan::addresses().into_iter().map(|a| AddressView { ip: a.ip.to_string(), adapter: a.adapter }).collect(),
        phones: devices::list()
            .into_iter()
            .map(|d| PhoneView { connected: connected.contains(&d.id), id: d.id, name: d.name, paired: d.paired, last_seen: d.last_seen })
            .collect(),
        asks: hub.map(|h| h.asks()).unwrap_or_default(),
    }
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
    tracing::info!("Yap Link: a pairing link made");
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
    if let Some(h) = hub() {
        h.cut_off(&id);
    }
    tracing::info!(gone, "Yap Link: a phone removed");
    let _ = app.emit(CHANGED, ());
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pairing_link_carries_the_address_port_and_token() {
        assert_eq!(pair_url("192.168.1.20".parse().unwrap(), 51443, "abc"), "https://192.168.1.20:51443/#pair=abc");
        assert_eq!(pair_url("fd00::20".parse().unwrap(), 51443, "abc"), "https://[fd00::20]:51443/#pair=abc");
    }
}
