//! Meeting search's commands: Settings → Privacy & data → "Search my
//! meetings by meaning" (status, download, on/off, rebuild, delete) and the
//! Meetings view's search ([`search_moments`]).

use tauri::AppHandle;

use super::index::{self, Status};
use super::{ask, embedder};

/// The index and its model, for Settings.
#[tauri::command]
pub fn search_status() -> Status {
    index::status()
}

/// Download the model (and the engine, if the on-device model hasn't
/// brought it), then switch search by meaning on: the index starts at once.
#[tauri::command]
pub async fn search_install(app: AppHandle) -> Result<Status, String> {
    index::emit(&app);
    let result = embedder::install(&app).await;
    if let Err(e) = &result {
        tracing::warn!("Meeting search: the download didn't finish: {e}");
        index::emit(&app);
        return Err(e.clone());
    }
    crate::shell::settings::change_config(&app, |c| {
        c.search_by_meaning = true;
        Ok(())
    })?;
    index::poke();
    index::emit(&app);
    Ok(index::status())
}

/// "Search my meetings by meaning" on or off (the model and the index stay;
/// off, the model's server stops).
#[tauri::command]
pub fn search_set_enabled(app: AppHandle, on: bool) -> Result<Status, String> {
    crate::shell::settings::change_config(&app, |c| {
        c.search_by_meaning = on;
        Ok(())
    })?;
    index::emit(&app);
    Ok(index::status())
}

/// Read every passage again (the index deleted; it fills in the background).
#[tauri::command]
pub fn search_rebuild(app: AppHandle) -> Result<Status, String> {
    index::forget()?;
    embedder::reset_failures();
    tracing::info!("Meeting search: the index is being rebuilt");
    index::poke();
    index::emit(&app);
    Ok(index::status())
}

/// Delete the model and the index, and switch search by meaning off. (The
/// engine stays: the on-device model may use it. Meeting search by keywords
/// goes on working.)
#[tauri::command]
pub fn search_delete(app: AppHandle) -> Result<Status, String> {
    crate::shell::settings::change_config(&app, |c| {
        c.search_by_meaning = false;
        Ok(())
    })?;
    embedder::delete_model()?;
    index::forget()?;
    index::emit(&app);
    Ok(index::status())
}

/// One moment a search found, for the Meetings view.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub note_id: u64,
    pub title: String,
    pub kind: &'static str,
    /// "transcript" | "content" | "summary".
    pub field: &'static str,
    pub line: usize,
    pub ts: u64,
    pub when: String,
    pub at: Option<String>,
    pub speaker: String,
    pub text: String,
}

/// The moments in meetings and notes that match `query` (the Meetings view's
/// search, by meaning when it's on): best first. `{moments, semantic,
/// range}` — `range`: the dates the query named, in words.
#[tauri::command]
pub async fn search_moments(query: String, limit: Option<usize>) -> serde_json::Value {
    let q = query.trim();
    if q.chars().count() < 2 {
        return serde_json::json!({ "moments": [], "semantic": false, "range": null });
    }
    let found = ask::find(q, limit.unwrap_or(6).clamp(1, 20)).await;
    let moments: Vec<Moment> = found
        .passages
        .iter()
        .map(|p| Moment {
            note_id: p.note,
            title: p.title.clone(),
            kind: p.kind,
            field: p.field.as_str(),
            line: p.line,
            ts: p.ts,
            when: p.when.clone(),
            at: p.at.clone(),
            speaker: p.speaker.clone(),
            text: p.text.clone(),
        })
        .collect();
    serde_json::json!({
        "moments": moments,
        "semantic": found.semantic,
        "range": found.range.map(|r| r.label),
    })
}
