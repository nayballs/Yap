//! **The bar's ^ menu extras** (Wispr Flow's Flow Bar menu, by behaviour;
//! 2026-10-29): beside Open Yap, New meeting note, Settings, Hide and Turn
//! off, the menu shows
//!
//! - **the microphone in use**, with the others connected in the order Yap
//!   tries them (Settings → Dictation → Microphone, `dictation/mics.rs`):
//!   picking one puts it first in that order, as "Use it for dictation"
//!   does ([`bar_use_mic`]);
//! - **Paste last transcript** (`pasting/last.rs`), into the app you're in —
//!   the bar never takes the focus;
//! - **the language shortcuts' languages** (My languages,
//!   `dictation/languages.rs`), when any are set: a dictation in that
//!   language, as its shortcut starts ([`dictate_in`]).
//!
//! The page asks for them when the menu opens ([`bar_menu`]): Windows' list
//! of microphones is read then, never polled.

use serde::Serialize;
use tauri::AppHandle;

use crate::dictation::mics::{self, Endpoint, Target};

/// A microphone in the menu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicRow {
    pub id: String,
    /// Its short name ("Contoso H2", `mics::short_name`).
    pub name: String,
    /// The one a dictation starting now would use.
    pub in_use: bool,
}

/// A language shortcut in the menu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageRow {
    pub id: String,
    /// "French", "English → Spanish".
    pub name: String,
}

/// What the menu shows besides its fixed items.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extras {
    pub mics: Vec<MicRow>,
    /// A dictation to paste again.
    pub paste_last: bool,
    pub languages: Vec<LanguageRow>,
}

/// The connected microphones in the order Yap tries them (pure; `order`
/// the person's, `present` Windows' list): the order's first, then
/// Windows' default, then the rest; the first is the one in use. Software
/// devices only when they're in the order or in use.
pub fn mic_rows(order: &[mics::MicChoice], present: &[Endpoint]) -> Vec<MicRow> {
    let mut rows: Vec<MicRow> = Vec::new();
    let push = |e: &Endpoint, rows: &mut Vec<MicRow>| {
        if !rows.iter().any(|r| r.id == e.id) {
            rows.push(MicRow { id: e.id.clone(), name: mics::short_name(&e.name), in_use: rows.is_empty() });
        }
    };
    for t in mics::candidates(order, Some(present), &[]) {
        if let Target::Mic(e) = t {
            push(&e, &mut rows);
        }
    }
    for e in present.iter().filter(|e| !e.is_virtual) {
        push(e, &mut rows);
    }
    rows
}

/// The menu's extras, read as it opens.
#[tauri::command]
pub async fn bar_menu() -> Result<Extras, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let cfg = crate::config::load();
        let present = mics::present().unwrap_or_default();
        Extras {
            mics: mic_rows(&cfg.mic_order, &present),
            paste_last: crate::pasting::text_injector::has_last_transcript(),
            languages: crate::dictation::languages::shortcuts(&cfg)
                .into_iter()
                .map(|s| LanguageRow { name: s.name(), id: s.id })
                .collect(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// A microphone picked in the menu: first in the order (and off "Not now"),
/// so the next dictation uses it. Settings follows (`yap-config-saved`).
/// Off the main thread (as `mic_offer_respond`): reading Windows' list,
/// saving and applying the config take the pipeline's lock, which a
/// shortcut's start holds while it reads the window in front.
#[tauri::command]
pub async fn bar_use_mic(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || use_mic(&app, &id)).await.map_err(|e| e.to_string())?
}

fn use_mic(app: &AppHandle, id: &str) -> Result<(), String> {
    let mic = mics::present()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("That microphone isn't connected any more")?;
    crate::shell::settings::change_config(app, |c| {
        mics::offer::put_first(&mut c.mic_order, &mut c.mic_not_now, &mic);
        Ok(())
    })?;
    tracing::info!("bar: a microphone picked from the menu, first in the order now");
    let _ = tauri::Emitter::emit(app, mics::EVENT_CHANGED, ());
    Ok(())
}

/// A language picked in the menu: a dictation in it, into the app you're in
/// — as its shortcut's press starts one (toggle: the next press of the
/// dictation key, or ✓, ends it).
pub fn dictate_in(app: &AppHandle, id: &str) -> Result<(), String> {
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
    let pipeline = guard.as_ref().ok_or("pipeline not started")?;
    crate::dictation::languages::arm(Some(id));
    pipeline.toggle();
    crate::dictation::languages::arm(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mic(id: &str, name: &str, default: bool) -> Endpoint {
        Endpoint { id: id.into(), name: name.into(), default, ..Endpoint::default() }
    }

    #[test]
    fn the_menu_lists_the_mics_in_the_order_yap_tries_them() {
        let present = vec![
            mic("a", "Microphone Array (Realtek Audio)", true),
            mic("b", "Headset Microphone (Contoso H2)", false),
            Endpoint { is_virtual: true, ..mic("v", "Voice Changer", false) },
            mic("c", "Microphone (Webcam C9)", false),
        ];
        // The order says the headset: it's in use, then Windows' default, then the rest.
        let order = vec![mics::MicChoice { id: "b".into(), name: String::new() }];
        let rows = mic_rows(&order, &present);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "c"], "no software device it doesn't use");
        assert!(rows[0].in_use && !rows[1].in_use && !rows[2].in_use);
        assert_eq!(rows[0].name, "Contoso H2");
        // No order: Windows' default first.
        let rows = mic_rows(&[], &present);
        assert_eq!(rows[0].id, "a");
        assert!(rows[0].in_use);
        assert!(mic_rows(&[], &[]).is_empty());
    }
}
