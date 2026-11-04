//! **Paste last transcript** and **Copy last transcript** on shortcuts of
//! their own (Settings → Shortcuts; Wispr Flow's, by behaviour), beside the
//! tray's items. Both unbound by default (`config.paste_last_hotkey`,
//! `copy_last_hotkey`; `input_hook.rs`: only the press, never a key another
//! shortcut has).
//!
//! - **Paste last**: once the shortcut's keys are up (Ctrl+V with Alt still
//!   down is another key in most apps), the latest dictation is pasted where
//!   you're typing, fitted to the cursor — exactly the tray's item
//!   (`text_injector::paste_last_blocking`: an app running as administrator
//!   gets it on the clipboard, and the person is told).
//! - **Copy last**: the latest dictation (before it was fitted to a cursor)
//!   onto the clipboard, privately — out of Win+V history and cloud sync, as
//!   Yap's pastes are (`text_injector::copy_privately`) — and a quiet
//!   "Copied your last dictation" (the main window's toast while you look at
//!   it, else a bar card; never a Windows notification). Also the tray's
//!   "Copy last transcript" and Yap's own windows' in-page fallback (the
//!   global hook never sees keys while a Yap window has the focus).
//!
//! Nothing dictated yet: said the same quiet way. The log says lengths,
//! never the text. A test run copies nothing (the developer's clipboard is
//! theirs): it logs and returns the text; robot mode copies for real.

use tauri::AppHandle;
use tracing::{info, warn};

use crate::notices::alert::{self, Alert};
use crate::pasting::text_injector;

/// The quiet notes' kind (a bar card's id, `notice.rs`'s rule).
pub const KIND: &str = "lastTranscript";

const NOTHING_TO_COPY: &str = "Nothing to copy yet — dictate something first";

/// A quiet note about the last transcript (`alert::confirm`).
fn note(app: &AppHandle, title: &str, body: &str) {
    alert::confirm(
        app,
        Alert { kind: KIND, icon: "clipboard", title: title.into(), body: body.into(), action: None, timeout_ms: 3_500 },
    );
}

/// The Paste last shortcut: on a thread of its own, once its keys are up.
pub fn paste_soon(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("paste-last".into()).spawn(move || {
        crate::pasting::as_said::wait_for_modifiers_up();
        match text_injector::paste_last_blocking(&app, false) {
            Ok(text) => info!(len = text.len(), "Paste last transcript: its shortcut"),
            Err(e) if e == text_injector::NOTHING_TO_PASTE => {
                info!("Paste last transcript: nothing dictated yet");
                note(&app, "Nothing to paste yet", "Dictate something first: the shortcut pastes your latest dictation again.");
            }
            Err(e) => {
                warn!("Paste last transcript: {e}");
                note(&app, "Couldn't paste your last dictation", &e);
            }
        }
    });
    if let Err(e) = spawned {
        warn!("Paste last transcript: no thread ({e})");
    }
}

/// The latest dictation onto the clipboard, privately; returns it.
fn copy(app: &AppHandle) -> Result<String, String> {
    let Some(text) = text_injector::last_transcript().map(|t| t.trim().to_string()) else {
        info!("Copy last transcript: nothing dictated yet");
        note(app, "Nothing to copy yet", "Dictate something first: the shortcut copies your latest dictation.");
        return Err(NOTHING_TO_COPY.into());
    };
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        info!(len = text.len(), "e2e: not copying the last transcript");
    } else if text_injector::copy_privately(&text) {
        info!(len = text.len(), "Copied the last transcript");
    } else {
        warn!("Copy last transcript: the clipboard couldn't be written");
        note(app, "Couldn't copy your last dictation", "The clipboard couldn't be written. Try again in a moment.");
        return Err("The clipboard couldn't be written".into());
    }
    note(app, "Copied your last dictation", "It's on the clipboard, kept out of your clipboard history.");
    Ok(text)
}

/// The Copy last shortcut (and the tray's item): on a thread of its own.
pub fn copy_soon(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("copy-last".into()).spawn(move || {
        let _ = copy(&app);
    });
    if let Err(e) = spawned {
        warn!("Copy last transcript: no thread ({e})");
    }
}

/// "Copy last transcript" from a Yap window (the in-page fallback of its
/// shortcut): copies it and returns it. Errors when there's nothing yet.
#[tauri::command]
pub async fn copy_last_transcript(app: AppHandle) -> Result<String, String> {
    tokio::task::spawn_blocking(move || copy(&app)).await.map_err(|e| format!("Copy task failed: {e}"))?
}
