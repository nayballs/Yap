//! What Yap says when a microphone connects (Wispr Flow's "X detected ·
//! Switch?", by behaviour; `mics::on_arrival` decides):
//!
//! - **new to Yap** — "Contoso H2 connected · Use it for dictation?" with
//!   **Use it** (first in the order from now on) and **Not now** (never
//!   asked about again; Settings can still add it). One quiet notice in one
//!   place (`notice::place`: the main window's toast while the person is
//!   looking at it, else a card on the Yap bar; never a Windows
//!   notification), at most once per [`mics::ASK_GAP_MS`] and once per
//!   microphone a run (`mics::Gate`);
//! - **higher in the order than the one in use** — the order says use it,
//!   so it's used from the next dictation, and said: "Using Contoso H2 for
//!   dictation" (an alert's notice, kind `micSwitched`: the same words not
//!   twice in ten minutes).
//!
//! Never during a dictation: an arrival waits until it's over (and the
//! notice policy's settle after it, `notice::moment`), and is dropped if
//! the microphone has gone again meanwhile.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::{Arrival, Endpoint, Gate, MicChoice};

/// The main window's event: an [`Offer`].
const EVENT: &str = "yap-mic-offer";
/// The bar card's id and its rule in `notice.rs`.
pub const CARD: &str = "micOffer";
/// "Using … for dictation" (an alert's kind; its rule in `notice.rs`).
pub const SWITCHED: &str = "micSwitched";
/// How long the question stays up.
const CARD_MS: u64 = 30_000;
/// Windows may make a new device its default a moment after it arrives:
/// decide once that's settled.
const SETTLE_MS: u64 = 800;
/// A dictation going on: look again this often…
const WAIT_STEP_MS: u64 = 500;
/// …for this long at most.
const WAIT_MAX: Duration = Duration::from_secs(10 * 60);

/// What the main window gets (`yap-mic-offer`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    id: u64,
    title: String,
    body: String,
}

/// The question up now, waiting for an answer.
struct Pending {
    id: u64,
    mic: Endpoint,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static GATE: Mutex<Option<Gate>> = Mutex::new(None);

fn pending() -> std::sync::MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|p| p.into_inner())
}

/// The question's words: "Contoso H2 connected" · "Use it for dictation?"
pub fn ask_text(mic: &Endpoint) -> (String, String) {
    (format!("{} connected", super::short_name(&mic.name)), "Use it for dictation?".to_string())
}

/// "Using Contoso H2 for dictation".
pub fn using_alert(mic: &Endpoint) -> crate::notices::alert::Alert {
    crate::notices::alert::Alert {
        kind: SWITCHED,
        icon: "mic",
        title: format!("Using {} for dictation", super::short_name(&mic.name)),
        body: "It's higher in your microphone order than the one Yap was using.".to_string(),
        action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_MIC_ORDER, label: "Change" }),
        timeout_ms: 8_000,
    }
}

/// `mic` just connected (the list's change, no lock held): what it deserves
/// is decided once things settle and no dictation is going on, on a thread
/// of its own.
pub fn arrived(app: &AppHandle, mic: Endpoint) {
    if mic.is_virtual {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("mic-offer".into()).spawn(move || {
        std::thread::sleep(Duration::from_millis(SETTLE_MS));
        let since = Instant::now();
        loop {
            let here = super::present().is_some_and(|l| l.iter().any(|e| e.id == mic.id));
            if !here {
                tracing::debug!("Microphone offer: it went again; nothing to say");
                return;
            }
            if !crate::notices::notice::moment().dictating {
                break;
            }
            if since.elapsed() > WAIT_MAX {
                return;
            }
            std::thread::sleep(Duration::from_millis(WAIT_STEP_MS));
        }
        let cfg = crate::config::load();
        let Some(present) = super::present() else { return };
        let Some(mic) = present.iter().find(|e| e.id == mic.id).cloned() else { return };
        match super::on_arrival(&mic, &cfg.mic_order, &cfg.mic_not_now, &present) {
            Arrival::Ask => {
                let now = crate::notices::notice::now_ms();
                let admitted = GATE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Gate::default).admit(&mic.id, now);
                if admitted {
                    ask(&app, mic);
                } else {
                    tracing::info!("Microphone offer: asked a moment ago; not now");
                }
            }
            Arrival::Using => {
                tracing::info!("Microphone offer: the order puts the new microphone to use");
                crate::notices::alert::show(&app, using_alert(&mic));
            }
            Arrival::Quiet => tracing::debug!("Microphone offer: nothing to say"),
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("Microphone offer: no thread for it ({e})");
    }
}

/// Ask about `mic`: a toast in the main window while the person is looking
/// at it, else a Yap bar card. Blocking (window queries): no locks held.
fn ask(app: &AppHandle, mic: Endpoint) {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (title, body) = ask_text(&mic);
    let place = crate::notices::notice::place(app, false);
    if place == crate::notices::notice::Place::Nowhere {
        tracing::info!("Microphone offer: nowhere to ask");
        return;
    }
    *pending() = Some(Pending { id, mic });
    let shown = if place == crate::notices::notice::Place::Window {
        app.emit(EVENT, Offer { id, title, body }).is_ok()
    } else {
        use crate::bar::{Card, CardAction};
        let card = Card {
            id: CARD.into(),
            icon: "mic",
            title,
            body,
            primary: Some(CardAction::new("use", "Use it")),
            secondary: Some(CardAction::new("later", "Not now")),
            timeout_ms: Some(CARD_MS),
            expire_action: Some("expire".into()),
            close_action: Some("later".into()),
            ..Default::default()
        };
        crate::bar::show_card(
            app,
            card,
            Box::new(move |app, action| {
                if let Err(e) = respond(app, id, action) {
                    tracing::warn!("Microphone offer: {e}");
                }
            }),
        )
    };
    if shown {
        tracing::info!(on_bar = place != crate::notices::notice::Place::Window, "Microphone offer: asked about a new microphone");
    } else {
        let mut p = pending();
        if p.as_ref().is_some_and(|p| p.id == id) {
            *p = None;
        }
    }
}

/// The same microphone (by id, or by name for one kept without an id).
fn same(choice: &MicChoice, mic: &Endpoint) -> bool {
    (!choice.id.is_empty() && choice.id == mic.id) || ((choice.id.is_empty() || mic.id.is_empty()) && choice.name == mic.name)
}

/// `mic` first in the order (and off the "Not now" list).
pub fn put_first(order: &mut Vec<MicChoice>, not_now: &mut Vec<MicChoice>, mic: &Endpoint) {
    order.retain(|c| !same(c, mic));
    order.insert(0, MicChoice { id: mic.id.clone(), name: mic.name.clone() });
    not_now.retain(|c| !same(c, mic));
}

/// `mic` on the "Not now" list (once).
pub fn add_not_now(not_now: &mut Vec<MicChoice>, mic: &Endpoint) {
    if !not_now.iter().any(|c| same(c, mic)) {
        not_now.push(MicChoice { id: mic.id.clone(), name: mic.name.clone() });
    }
}

/// The answer to question `id`: "use" | "later" | "expire" (a card that ran
/// out: no answer). An answer to a question that isn't up any more is
/// ignored.
pub fn respond(app: &AppHandle, id: u64, answer: &str) -> Result<(), String> {
    let mic = {
        let mut p = pending();
        match p.take() {
            Some(q) if q.id == id => q.mic,
            other => {
                *p = other;
                return Ok(());
            }
        }
    };
    match answer {
        "use" => {
            crate::shell::settings::change_config(app, |c| {
                put_first(&mut c.mic_order, &mut c.mic_not_now, &mic);
                Ok(())
            })?;
            tracing::info!("Microphone offer: Use it (first in the order now)");
        }
        "later" => {
            crate::shell::settings::change_config(app, |c| {
                add_not_now(&mut c.mic_not_now, &mic);
                Ok(())
            })?;
            tracing::info!("Microphone offer: Not now (won't ask about it again)");
        }
        _ => tracing::info!("Microphone offer: left to fade"),
    }
    Ok(())
}

/// The main window's answer to its toast (`yap-mic-offer`).
#[tauri::command]
pub async fn mic_offer_respond(app: AppHandle, id: u64, answer: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || respond(&app, id, &answer))
        .await
        .map_err(|e| e.to_string())?
}

/// Test mode: forget the rate limit and any question up.
#[cfg(debug_assertions)]
pub(crate) fn e2e_reset() {
    *GATE.lock().unwrap_or_else(|p| p.into_inner()) = None;
    *pending() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mic(id: &str, name: &str) -> Endpoint {
        Endpoint { id: id.into(), name: name.into(), ..Default::default() }
    }

    #[test]
    fn use_it_puts_the_mic_first_once_and_takes_it_off_not_now() {
        let headset = mic("headset", "Headset Microphone (Contoso H2)");
        let mut order = vec![
            MicChoice { id: "usb".into(), name: "Microphone (Fabrikam USB Mic)".into() },
            MicChoice { id: String::new(), name: "Headset Microphone (Contoso H2)".into() },
        ];
        let mut not_now = vec![MicChoice { id: "headset".into(), name: "x".into() }];
        put_first(&mut order, &mut not_now, &headset);
        assert_eq!(order.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["headset", "usb"]);
        assert!(not_now.is_empty());
    }

    #[test]
    fn not_now_is_kept_once() {
        let headset = mic("headset", "Headset Microphone (Contoso H2)");
        let mut not_now = Vec::new();
        add_not_now(&mut not_now, &headset);
        add_not_now(&mut not_now, &headset);
        assert_eq!(not_now.len(), 1);
    }

    #[test]
    fn the_question_names_the_device() {
        let (title, body) = ask_text(&mic("h", "Headset Microphone (Contoso H2)"));
        assert_eq!(title, "Contoso H2 connected");
        assert_eq!(body, "Use it for dictation?");
        assert_eq!(using_alert(&mic("h", "Microphone (Fabrikam USB Mic)")).title, "Using Fabrikam USB Mic for dictation");
    }
}
