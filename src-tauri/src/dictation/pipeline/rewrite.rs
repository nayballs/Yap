//! The spoken words as an instruction to the AI: the rewrite shortcut
//! (the selection, or the last dictation, rewritten in place) and
//! Commands (a dictation that says the agent's name), through the
//! Commands scope's model.

use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::Mutex;

use tauri::Emitter;

use crate::config::YapConfig;
use crate::pasting::app_kind::AppKind;

use super::delivery::undo_window_gone;
use super::{Shared, DICTATION_ERROR};

/// Where the **Commands** scope's calls go: its own provider, model and
/// prompt when it's on and set up, else the global AI-cleanup endpoint with
/// no prompt body (the original edit-mode behaviour); "ondevice" goes to
/// the sidecar either way (`local_llm::effective_endpoint_for`). Commands
/// (`run_agent`) and saved transforms (`transforms.rs`, which bring their
/// own instructions and leave `body` out) share it.
#[derive(Debug, Clone, Default)]
pub(crate) struct CommandsModel {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    /// The Commands prompt, `{{agentName}}` filled in ("" on the cleanup
    /// endpoint).
    pub body: String,
    pub disable_thinking: bool,
}

/// See [`CommandsModel`].
pub(crate) fn commands_model(c: &YapConfig) -> CommandsModel {
    match c.llm_scopes.get("voiceAgent") {
        Some(s) if s.enabled && !s.provider.is_empty() => {
            // Standard-provider keys are global (shared with the Cleanup
            // tab); an empty scope key falls back to them.
            let key = c.provider_api_key(&s.provider, &s.api_key);
            let (base_url, api_key, model, provider) =
                crate::ai::local_llm::effective_endpoint_for(&s.provider, &s.base_url, &key, &s.model);
            // The agent prompt is templated on the wake word (OpenWhispr's
            // `{{agentName}}`); "Yap" is the unnamed default, matching the
            // tab's copy.
            let name = match c.agent_name.trim() {
                "" => "Yap",
                n => n,
            };
            let body = s.prompt.replace("{{agentName}}", name);
            CommandsModel { base_url, api_key, model, provider, body, disable_thinking: s.disable_thinking }
        }
        _ => {
            let (base_url, api_key, model, provider) = crate::ai::local_llm::effective_endpoint(c);
            CommandsModel { base_url, api_key, model, provider, body: String::new(), disable_thinking: c.pp_disable_thinking }
        }
    }
}

/// What the rewrite shortcut (or a Command) did — the robot's report
/// (`realtest.rs`); debug builds only.
#[cfg(debug_assertions)]
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rewritten {
    /// It was the last dictation's rewrite (nothing was selected).
    pub last: bool,
    /// The AI's text; empty when it never got that far.
    pub text: String,
    /// "replaced" / "copied" / "testMode" (the last dictation's), "pasted"
    /// (a selection's or new text), "refused" (nothing pasted), "nothing"
    /// (nothing said or written), "failed" (the AI).
    pub outcome: String,
    /// Why it wasn't replaced or pasted.
    pub why: Option<String>,
}

#[cfg(debug_assertions)]
pub(super) static LAST_REWRITE: Mutex<Option<Rewritten>> = Mutex::new(None);

/// Keep what the rewrite did for the robot (debug builds only).
fn report_rewrite(last: bool, text: &str, outcome: &str, why: Option<String>) {
    #[cfg(debug_assertions)]
    if let Ok(mut r) = LAST_REWRITE.lock() {
        *r = Some(Rewritten { last, text: text.to_string(), outcome: outcome.to_string(), why });
    }
    #[cfg(not(debug_assertions))]
    let _ = (last, text, outcome, why);
}

impl Shared {
    /// Is this dictation a **Command** (it says the agent's name; was "the
    /// Voice Agent's wake word")? Port of OpenWhispr's
    /// `resolveDictationAgentReachability` + `detectAgentName` gate: the
    /// Commands scope (`voiceAgent`) must be enabled with a provider (BYOK/custom providers
    /// also need a model; ondevice/self-hosted have defaults), and the
    /// transcript must address the agent by name — fuzzy-matched, so STT
    /// mis-hearings still wake it. Default name "Yap" (the tab's copy).
    pub(super) fn wake_word_hit(&self, transcript: &str) -> bool {
        let Ok(c) = self.config.read() else {
            return false;
        };
        let Some(s) = c.llm_scopes.get("voiceAgent") else {
            return false;
        };
        if !s.enabled || s.provider.is_empty() {
            return false;
        }
        if !(s.provider == "ondevice" || s.provider == "local") && s.model.trim().is_empty() {
            return false;
        }
        let name = match c.agent_name.trim() {
            "" => "Yap",
            n => n,
        };
        crate::dictation::agent_detect::detect_agent_name(transcript, name)
    }
    /// Edit/rewrite-mode finish: apply the spoken `instruction` to the selection
    /// captured at record-start and paste the result back into the target window.
    /// Uses the **Commands** scope's own provider/model/prompt when that scope is
    /// enabled + configured, otherwise the global AI-cleanup endpoint; either way it
    /// needs a base URL configured. Emits the final `yap-state` itself.
    pub(super) async fn run_rewrite(self: &Arc<Self>, instruction: String) {
        // Take the selection captured when the edit hotkey was pressed.
        let selection = self
            .selection
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or_default();
        // Nothing selected, right after a paste into this window: that paste
        // — the last dictation — is what the instruction is for.
        let last = self
            .rewrite_last
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .filter(|_| selection.trim().is_empty());
        self.run_agent(instruction, selection, last).await
    }

    /// Shared runner for **Commands**: apply `instruction` via the Commands
    /// scope (`voiceAgent`) (or
    /// the global cleanup endpoint) and paste the result. `selection` empty =
    /// write mode — unless `last`, the last dictation to rewrite in place
    /// (`as_said.rs`: checked still there before the AI is asked, replaced
    /// exactly, or nothing pasted). Two callers: the edit hotkey
    /// (`run_rewrite`, selection captured at key-press) and the **wake-word
    /// path** (no selection — the whole transcript is the instruction,
    /// OpenWhispr-style). What it pasted is the last paste from then on.
    pub(super) async fn run_agent(self: &Arc<Self>, instruction: String, selection: String, last: Option<crate::pasting::as_said::Pasted>) {
        // Nothing heard (the edit key tapped without speaking): go quietly back
        // to idle like an empty dictation does. Falling through used to surface
        // "Rewrite failed via <provider> — check its key", blaming the API key.
        if instruction.trim().is_empty() {
            tracing::info!("Edit mode: no speech detected, nothing to rewrite");
            report_rewrite(last.is_some(), "", "nothing", None);
            let _ = self.app.emit("yap-state", "idle");
            return;
        }

        let (m, restore_clipboard) = self
            .config
            .read()
            .map(|c| (commands_model(&c), c.restore_clipboard))
            .unwrap_or_default();
        let CommandsModel { base_url, api_key, model, provider, body, disable_thinking } = m;

        if base_url.is_empty() {
            // The on-device model, down for a moment while Yap restarts it.
            let restarting = provider == crate::ai::local_llm::PROVIDER_ONDEVICE && crate::ai::local_llm::restart_under_way();
            tracing::warn!(restarting, "Edit mode needs an AI cleanup endpoint (none configured)");
            report_rewrite(last.is_some(), "", "failed", Some("no AI model".into()));
            let _ = self.app.emit(
                DICTATION_ERROR,
                if restarting {
                    "The on-device AI model is restarting. Try again in a moment."
                } else {
                    "Set up AI cleanup to use edit mode"
                },
            );
            let _ = self.app.emit("yap-state", "error");
            return;
        }

        // Cloud providers reject keyless requests with a confusing 401 (e.g.
        // "Invalid Anthropic API Key" while the user is looking at the Groq
        // cleanup tab) — fail fast with a message that names the provider.
        const KEYED_PROVIDERS: [&str; 5] =
            ["groq", "anthropic", "openai", "gemini", "openrouter"];
        if api_key.is_empty() && KEYED_PROVIDERS.contains(&provider.as_str()) {
            tracing::warn!(provider = %provider, "Rewrite skipped: no API key for provider");
            report_rewrite(last.is_some(), "", "failed", Some("no API key".into()));
            let _ = self.app.emit(
                DICTATION_ERROR,
                format!("Commands: no {provider} API key — add one in Language Models"),
            );
            let _ = self.app.emit("yap-state", "error");
            return;
        }

        let target = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };

        // The last dictation: still in its field exactly as pasted, the
        // cursor right after it? Else nothing to rewrite — said, nothing
        // pasted, the AI not asked.
        if let Some(p) = &last {
            let p = p.clone();
            let refused = match tokio::task::spawn_blocking(move || crate::pasting::as_said::rewrite_check(&p)).await {
                Ok(Ok(())) => None,
                Ok(Err(why)) => Some(why),
                Err(e) => Some(format!("Yap couldn't look: {e}")),
            };
            if let Some(why) = refused {
                crate::pasting::as_said::cant_rewrite(&self.app, &why);
                report_rewrite(true, "", "refused", Some(why));
                let _ = self.app.emit("yap-state", "idle");
                return;
            }
        }
        let selection = match &last {
            Some(p) => p.text().to_string(),
            None => selection,
        };

        // Recorded as Commands' in the network receipts (net.rs).
        match crate::privacy::net::within(crate::privacy::net::Feature::Commands, crate::ai::llm::rewrite(
            &instruction,
            &selection,
            &base_url,
            &api_key,
            &model,
            &provider,
            &body,
            disable_thinking,
        ))
        .await
        {
            Ok(result) => {
                let mut out = result.trim().to_string();
                if out.is_empty() {
                    report_rewrite(last.is_some(), "", "nothing", None);
                    let _ = self.app.emit("yap-state", "idle");
                    return;
                }
                // The last dictation's rewrite: put in its place, exactly,
                // or the field left alone and the rewrite copied.
                if let Some(p) = last {
                    tracing::info!(chars = out.chars().count(), "Rewrite of the last dictation");
                    tracing::debug!(text = %out, "Rewrite text");
                    let _ = self.app.emit("yap-transcript", out.clone());
                    let app = self.app.clone();
                    let text = out.clone();
                    match tokio::task::spawn_blocking(move || crate::pasting::as_said::replace_last(&app, &p, &text)).await {
                        Ok(Ok(rewrote)) => {
                            let (outcome, why) = match &rewrote {
                                crate::pasting::as_said::Rewrote::Replaced { .. } => ("replaced", None),
                                crate::pasting::as_said::Rewrote::Copied { why, .. } => ("copied", Some(why.clone())),
                                crate::pasting::as_said::Rewrote::TestMode { .. } => ("testMode", None),
                            };
                            report_rewrite(true, &out, outcome, why);
                        }
                        // Robot mode refused the window (said already).
                        Ok(Err(why)) => {
                            tracing::warn!("Rewrite of the last dictation refused: {why}");
                            report_rewrite(true, &out, "refused", Some(why));
                        }
                        Err(e) => tracing::warn!("Rewrite of the last dictation failed: {e}"),
                    }
                    let _ = self.app.emit("yap-state", "idle");
                    return;
                }
                // A terminal gets the AI's text safe for it too (one line at
                // a shell); anywhere else it's pasted as the AI wrote it.
                if let AppKind::Terminal(t) = self.paste_kind() {
                    out = crate::pasting::app_kind::terminal_text(&out, t.keeps_lines());
                }
                // The log says how much, never what (the text only in Debug logging).
                tracing::info!(chars = out.chars().count(), "Rewrite");
                tracing::debug!(text = %out, "Rewrite text");
                let _ = self.app.emit("yap-transcript", out.clone());
                let strict = self.undoing.load(Ordering::SeqCst);
                let read = self.read_kind();
                match crate::pasting::text_injector::inject_text(&out, restore_clipboard, target, strict, read).await {
                    // The last paste now: "now shorter", with nothing
                    // selected, rewrites it.
                    Ok(crate::pasting::text_injector::Delivery::Pasted | crate::pasting::text_injector::Delivery::Typed) => {
                        crate::pasting::as_said::remember(target, read, &out, None, restore_clipboard);
                        report_rewrite(false, &out, "pasted", None);
                    }
                    // An app running as administrator: on the clipboard.
                    Ok(crate::pasting::text_injector::Delivery::LeftOnClipboard) => {
                        crate::pasting::as_said::forget();
                        crate::notices::alert::show_soon(&self.app, crate::pasting::elevated::alert());
                    }
                    // An Undo whose window is gone: on the clipboard.
                    Ok(crate::pasting::text_injector::Delivery::WindowGone) => {
                        crate::pasting::as_said::forget();
                        crate::notices::alert::show_soon(&self.app, undo_window_gone());
                    }
                    Err(e) => {
                        crate::pasting::as_said::forget();
                        tracing::warn!("Rewrite inject failed: {}", e);
                    }
                }
                let _ = self.app.emit("yap-state", "idle");
            }
            Err(e) => {
                // Don't fall back to typing the raw instruction — that would paste
                // "make this a list" into the doc. Surface an error instead.
                tracing::warn!("Rewrite failed: {}", e);
                report_rewrite(last.is_some(), "", "failed", Some(e.clone()));
                let _ = self.app.emit(
                    DICTATION_ERROR,
                    format!("Rewrite failed via {provider} — check its key in Language Models"),
                );
                let _ = self.app.emit("yap-state", "error");
            }
        }
    }
}
