//! The spoken words as an instruction to the AI: the rewrite shortcut
//! (the selection, or the last dictation, rewritten in place) and
//! Commands (a dictation that says the agent's name), through the
//! Commands scope's model. Words about a selection that ask rather than
//! edit are answered instead, on the Voice Assistant's path
//! (`ask_or_edit.rs`, `run_rewrite`).

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
    /// (a selection's or new text; "copied" when it was left on the
    /// clipboard instead), "refused" (nothing pasted), "nothing" (nothing
    /// said or written), "failed" (the AI), "answered" (a question about
    /// the selection: asked on the Voice Assistant's path, nothing pasted).
    pub outcome: String,
    /// Why it wasn't replaced or pasted.
    pub why: Option<String>,
    /// How the words about a selection were taken (`ask_or_edit`:
    /// "rule:edit", "rule:ask", "model:replace", "model:answer",
    /// "doubt:answer", "readOnly:answer"); empty: not decided (nothing
    /// selected, or "Answer questions about the selected text" off).
    pub decided: String,
    /// How the window in front went with it (`command_screen::label`:
    /// "rule:text", "model:text", "rule:picture", "rule:asked" — asked
    /// first, nothing written —, "rule:withheld", "rule:off",
    /// "rule:unread", "model:no"); empty: not needed.
    pub screen: String,
}

#[cfg(debug_assertions)]
pub(super) static LAST_REWRITE: Mutex<Option<Rewritten>> = Mutex::new(None);

/// How the current rewrite's words were taken, for its report (debug
/// builds only).
#[cfg(debug_assertions)]
static DECIDED: Mutex<String> = Mutex::new(String::new());

/// How the window in front went with the current Command (its report;
/// debug builds only).
#[cfg(debug_assertions)]
static SCREEN: Mutex<String> = Mutex::new(String::new());

/// Keep what the rewrite did for the robot (debug builds only).
fn report_rewrite(last: bool, text: &str, outcome: &str, why: Option<String>) {
    #[cfg(debug_assertions)]
    if let Ok(mut r) = LAST_REWRITE.lock() {
        let decided = std::mem::take(&mut *DECIDED.lock().unwrap_or_else(|p| p.into_inner()));
        let screen = std::mem::take(&mut *SCREEN.lock().unwrap_or_else(|p| p.into_inner()));
        *r = Some(Rewritten { last, text: text.to_string(), outcome: outcome.to_string(), why, decided, screen });
    }
    #[cfg(not(debug_assertions))]
    let _ = (last, text, outcome, why);
}

/// How the current rewrite's words were taken (its report; debug builds).
fn note_decided(label: &str) {
    #[cfg(debug_assertions)]
    {
        *DECIDED.lock().unwrap_or_else(|p| p.into_inner()) = label.to_string();
    }
    #[cfg(not(debug_assertions))]
    let _ = label;
}

/// How the window in front went with the current Command (its report;
/// debug builds).
fn note_screen(label: &str) {
    #[cfg(debug_assertions)]
    {
        *SCREEN.lock().unwrap_or_else(|p| p.into_inner()) = label.to_string();
    }
    #[cfg(not(debug_assertions))]
    let _ = label;
}

/// What a Command takes of the window in front ([`Shared::look_for_command`]).
enum Look {
    /// The words alone (the label says why; "" when the screen wasn't
    /// needed).
    Without(String),
    /// A cloud model met the first Command that needs the screen: asked
    /// first, nothing written (`command_screen::ask_once`).
    Asked,
    /// The window's text or its picture.
    With(crate::dictation::command_screen::Context, String),
}

/// Where the bar says "Reading the window…" (`yap-command-screen`).
const READING: &str = "yap-command-screen";

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
    ///
    /// **A question about the selection is answered instead** (2026-10-31,
    /// `ask_or_edit.rs`; "Answer questions about the selected text", on):
    /// with something selected, words that ask ("what does this mean?",
    /// "summarise this") go to the Voice Assistant's path with the
    /// selection as their quote (`assistant::ask_about`) and nothing is
    /// pasted; only a clear edit replaces it, and read-only text never is.
    /// Nothing selected: as ever.
    pub(super) async fn run_rewrite(self: &Arc<Self>, instruction: String) {
        // Take the selection captured when the edit hotkey was pressed.
        let selection = self
            .selection
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or_default();
        let read_only = self.selection_read_only.swap(false, Ordering::SeqCst);
        // Nothing selected, right after a paste into this window: that paste
        // — the last dictation — is what the instruction is for.
        let last = self
            .rewrite_last
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .filter(|_| selection.trim().is_empty());
        note_decided("");
        note_screen("");
        let (on, langs, model, screen_on) = self
            .config
            .read()
            .map(|c| (c.ask_about_selection, crate::dictation::languages::my_languages(&c), commands_model(&c), c.commands_screen))
            .unwrap_or_else(|_| (true, Vec::new(), CommandsModel::default(), false));
        // Right after a dictation, words that reply to what's on screen
        // ("reply saying Thursday works", not "make the reply shorter"):
        // new text, drafted from the window in front (`command_screen.rs`),
        // the dictation left where it is.
        let last = match last {
            Some(_) if screen_on && crate::dictation::command_screen::replies_instead_of_rewriting(&instruction, &langs) => {
                tracing::info!("Rewrite shortcut: a reply to what's on screen, not the last dictation's rewrite");
                None
            }
            last => last,
        };
        if crate::dictation::ask_or_edit::route(on, &selection, last.is_some(), &instruction) == crate::dictation::ask_or_edit::Route::Decide {
            let decision = match crate::dictation::ask_or_edit::settled(&instruction, &langs, read_only) {
                Some(d) => d,
                None => {
                    let ep = crate::dictation::ask_or_edit::Endpoint {
                        base_url: &model.base_url,
                        api_key: &model.api_key,
                        model: &model.model,
                        provider: &model.provider,
                    };
                    crate::dictation::ask_or_edit::decide(&instruction, &selection, &langs, Some(ep)).await
                }
            };
            tracing::info!(decided = decision.label(), read_only, "Rewrite shortcut on a selection: an edit or a question");
            note_decided(decision.label());
            if decision.choice == crate::dictation::ask_or_edit::Choice::Answer {
                self.answer_about_selection(instruction, selection);
                return;
            }
        }
        self.run_agent(instruction, selection, last).await
    }

    /// **The words about the selection were a question**: corrected (the
    /// dictionary, as the Voice Assistant's question is), then asked on the
    /// Voice Assistant's path with the selection and its app
    /// (`assistant::ask_about`) — spoken when spoken replies are on, the
    /// bar's card, kept in Chat with the selection as its quote. Nothing is
    /// pasted: the selection, and the clipboard edit mode's Ctrl+C put back,
    /// stay as they were.
    fn answer_about_selection(self: &Arc<Self>, instruction: String, selection: String) {
        let question = self
            .config
            .read()
            .map(|c| super::apply_corrections(&instruction, &c))
            .unwrap_or_else(|_| instruction.clone());
        let target = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };
        let app = crate::pasting::text_injector::app_name_for(target).unwrap_or_default();
        // A test run's stand-in window has no process: it says so.
        #[cfg(debug_assertions)]
        let app = if target.is_some() && target == super::test_target() { "e2e-stand-in.exe".to_string() } else { app };
        report_rewrite(false, "", "answered", None);
        crate::chat::assistant::ask_about(
            &self.app,
            question.trim().to_string(),
            crate::chat::assistant::selected::Selected { text: selection, app },
        );
        let _ = self.app.emit("yap-state", "idle");
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
            self.say_failed();
            return;
        }

        // Cloud providers reject keyless requests with a confusing 401 (e.g.
        // "Invalid Anthropic API Key" while the user is looking at the Groq
        // cleanup tab) — fail fast with a message that names the provider.
        if api_key.is_empty() && crate::ai::ai_model::KEYED_PROVIDERS.contains(&provider.as_str()) {
            tracing::warn!(provider = %provider, "Rewrite skipped: no API key for provider");
            report_rewrite(last.is_some(), "", "failed", Some("no API key".into()));
            let _ = self.app.emit(
                DICTATION_ERROR,
                format!("Commands: no {provider} API key — add one in Language Models"),
            );
            self.say_failed();
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

        // The kind of app it's going into, and the person's writing samples
        // for it (`ai/writing_samples.rs`; none in a terminal or an AI
        // prompt).
        let category = {
            let app_name = crate::pasting::text_injector::app_name_for(target);
            let style_target = crate::ai::cleanup_styles::Target::of_window(target, app_name.as_deref(), self.read_kind());
            self.config.read().ok().map(|c| crate::ai::cleanup_styles::category(&c, &style_target))
        };
        let samples = category.and_then(|category| crate::ai::writing_samples::for_rewrite(category, &base_url));

        // New text that replies to what's on screen: the window in front
        // goes with it (`command_screen.rs`) — or, a cloud model never
        // asked, nothing yet: asked first.
        let look = if selection.trim().is_empty() && last.is_none() {
            let ep = crate::dictation::ask_or_edit::Endpoint { base_url: &base_url, api_key: &api_key, model: &model, provider: &provider };
            self.look_for_command(&instruction, target, &ep).await
        } else {
            Look::Without(String::new())
        };
        let screen = match look {
            Look::Asked => {
                note_screen(&crate::dictation::command_screen::label(false, "asked"));
                report_rewrite(false, "", "asked", None);
                let _ = self.app.emit("yap-state", "idle");
                return;
            }
            Look::Without(label) => {
                note_screen(&label);
                None
            }
            Look::With(ctx, label) => {
                note_screen(&label);
                Some(ctx)
            }
        };

        // Recorded as Commands' in the network receipts (net.rs), saying
        // when the window in front and the writing samples went too.
        let asked = match &screen {
            Some(ctx) => {
                let (window, parts): (Option<&str>, &[serde_json::Value]) = match ctx {
                    crate::dictation::command_screen::Context::Text(t) => (Some(t.as_str()), &[]),
                    crate::dictation::command_screen::Context::Picture { parts, .. } => (None, parts.as_slice()),
                };
                let style = self
                    .config
                    .read()
                    .ok()
                    .and_then(|c| category.and_then(|cat| crate::ai::cleanup_styles::draft_line(&c, cat, &c.about_name)));
                let draft = crate::ai::llm::ScreenDraft {
                    instruction: &instruction,
                    window,
                    parts,
                    samples: samples.as_deref(),
                    body: &body,
                    style: style.as_deref(),
                };
                let out = crate::privacy::net::within(
                    crate::privacy::net::Feature::commands_screen(samples.is_some(), ctx.is_picture()),
                    crate::ai::llm::draft_from_screen(&draft, &base_url, &api_key, &model, &provider, disable_thinking),
                )
                .await
                .map(|text| crate::dictation::command_screen::clean_draft(&text));
                crate::dictation::command_screen::forget(ctx);
                out
            }
            None => {
                crate::privacy::net::within(
                    crate::privacy::net::Feature::commands(samples.is_some()),
                    crate::ai::llm::rewrite_with_samples(
                        &instruction,
                        &selection,
                        samples.as_deref(),
                        &base_url,
                        &api_key,
                        &model,
                        &provider,
                        &body,
                        disable_thinking,
                    ),
                )
                .await
            }
        };
        match asked {
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
                // As a dictation's paste: an Undo, or a Command whose mic was
                // disconnected or which stopped at its length limit, goes
                // into its own window or onto the clipboard, never into
                // whatever has the focus now (review 2026-10-30).
                let strict = self.own_window_only();
                let read = self.read_kind();
                match crate::pasting::text_injector::inject_text(&out, restore_clipboard, target, strict, read).await {
                    // The last paste now: "now shorter", with nothing
                    // selected, rewrites it.
                    Ok(crate::pasting::text_injector::Delivery::Pasted | crate::pasting::text_injector::Delivery::Typed) => {
                        crate::pasting::as_said::remember(target, read, &out, None, restore_clipboard);
                        self.tell_unplugged(super::unplugged::Outcome::Pasted);
                        report_rewrite(false, &out, "pasted", None);
                    }
                    // Not pasted (an app running as administrator…): on the
                    // clipboard, said once (the mic's alert says it, if that went).
                    Ok(crate::pasting::text_injector::Delivery::LeftOnClipboard(left)) => {
                        crate::pasting::as_said::forget();
                        crate::dictation::sound::cue(crate::dictation::sound::Cue::Error);
                        if !self.tell_unplugged(super::unplugged::Outcome::OnClipboard) {
                            crate::notices::alert::show_soon(&self.app, left.alert());
                        }
                        report_rewrite(false, &out, "copied", Some(left.code().to_string()));
                    }
                    // Its window is gone (or won't come to the front): on the clipboard.
                    Ok(crate::pasting::text_injector::Delivery::WindowGone) => {
                        crate::pasting::as_said::forget();
                        if !self.tell_unplugged(super::unplugged::Outcome::OnClipboard) {
                            crate::notices::alert::show_soon(&self.app, undo_window_gone());
                        }
                        report_rewrite(false, &out, "copied", Some("its window was gone".into()));
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
                // Its kind at warn; the error itself (a server's can quote
                // what the model wrote) at debug only.
                tracing::warn!(error = %crate::ai::llm::error_kind(&e), "Rewrite failed");
                tracing::debug!("Rewrite failed: {e}");
                report_rewrite(last.is_some(), "", "failed", Some(e.clone()));
                let _ = self.app.emit(
                    DICTATION_ERROR,
                    format!("Rewrite failed via {provider} — check its key in Language Models"),
                );
                self.say_failed();
            }
        }
    }

    /// **What a Command writing new text takes of the window in front**
    /// (`command_screen.rs`): nothing unless its words reply to what's on
    /// screen (the rule; a maybe, the model's yes); never in Yap's own
    /// window or a terminal; then the settings' gate — a cloud model never
    /// asked: asked first, nothing written — and the read, "Reading the
    /// window…" on the bar meanwhile: its text, bounded, or, next to none
    /// read, a screenshot for a model that reads images.
    async fn look_for_command(self: &Arc<Self>, instruction: &str, target: Option<isize>, ep: &crate::dictation::ask_or_edit::Endpoint<'_>) -> Look {
        use crate::dictation::command_screen as cs;
        let langs = self.config.read().map(|c| crate::dictation::languages::my_languages(&c)).unwrap_or_default();
        let need = cs::needs(instruction, &langs);
        if need == cs::Need::No {
            return Look::Without(String::new());
        }
        // The switches as config.json has them now: the ask and its Allow
        // save them from Rust (`command_screen::ask_once`, `allow_cloud`),
        // which the live config only hears of at Settings' next save.
        let saved = crate::config::load();
        let (on, cloud_on, asked) = (saved.commands_screen, saved.commands_screen_cloud, saved.commands_screen_asked);
        // Yap's own window, or a terminal (its screen is the command line's):
        // never.
        let Some(hwnd) = target else {
            return Look::Without(String::new());
        };
        if matches!(self.paste_kind(), AppKind::Terminal(_)) {
            tracing::info!("Command: a terminal's window is never read");
            return Look::Without(String::new());
        }
        let maybe = need == cs::Need::Maybe;
        let leaves = crate::chat::assistant::screen::leaves_pc(ep.provider, ep.base_url);
        let gate = cs::gate(on, cloud_on, asked, leaves);
        tracing::info!(?need, ?gate, "Command: one that may need the window in front");
        // (A maybe isn't worth asking about when it couldn't go.)
        let quiet = |what: &str| Look::Without(if maybe { String::new() } else { cs::label(false, what) });
        match gate {
            cs::Gate::Off => return quiet("off"),
            cs::Gate::NotToTheCloud => return quiet("withheld"),
            cs::Gate::AskFirst if maybe => return quiet("withheld"),
            cs::Gate::AskFirst => {
                cs::ask_once(&self.app, ep.provider, instruction, hwnd);
                return Look::Asked;
            }
            cs::Gate::Read => {}
        }
        if maybe && !cs::model_says(instruction, ep).await {
            return Look::Without(cs::label(true, "no"));
        }
        let _ = self.app.emit(READING, true);
        let t = std::time::Instant::now();
        let read = tokio::task::spawn_blocking(move || cs::read(hwnd)).await.ok().flatten();
        let max = if leaves { cs::MAX_CLOUD } else { cs::MAX_LOCAL };
        let look = match read {
            Some(w) if !w.little() => {
                let text = cs::bounded(&w, max);
                tracing::info!(read = w.chars(), chars = text.chars().count(), ms = t.elapsed().as_millis() as u64, "Command: read the window in front");
                Look::With(cs::Context::Text(text), cs::label(maybe, "text"))
            }
            read => {
                // Next to nothing read (an app UI Automation sees little
                // of): a screenshot, for a model that reads images.
                let picture = match crate::snip::vision::reads_images(ep.provider, ep.model) {
                    Some(true) => cs::picture(&self.app, hwnd, ep.provider, ep.base_url).await,
                    _ => None,
                };
                let text = read.as_ref().map(|w| cs::bounded(w, max)).unwrap_or_default();
                match picture {
                    Some(p) => {
                        tracing::info!(ms = t.elapsed().as_millis() as u64, "Command: took the window in front (next to no text read)");
                        Look::With(p, cs::label(maybe, "picture"))
                    }
                    // The title, at least: who and what.
                    None if !text.is_empty() => Look::With(cs::Context::Text(text), cs::label(maybe, "text")),
                    None => {
                        tracing::info!(ms = t.elapsed().as_millis() as u64, "Command: nothing read from the window in front");
                        Look::Without(cs::label(maybe, "unread"))
                    }
                }
            }
        };
        let _ = self.app.emit(READING, false);
        look
    }

    /// **A Command that asked first, allowed** (`command_screen::allow_cloud`):
    /// written now, as if just said — when its window is still the one in
    /// front and Yap isn't busy with a dictation; else nothing (logged).
    pub(super) async fn command_again(self: &Arc<Self>, instruction: String, hwnd: isize) {
        let front = crate::pasting::text_injector::current_foreground().unwrap_or(0);
        #[cfg(debug_assertions)]
        let front = super::test_target().unwrap_or(front);
        if front != hwnd {
            tracing::info!("Commands: allowed, but the Command's window isn't in front any more; not written");
            return;
        }
        let busy = (!self.recording.load(Ordering::SeqCst))
            .then(|| super::busy::Busy::take(self, super::busy::Job::Dictation))
            .flatten();
        let Some(busy) = busy else {
            tracing::info!("Commands: allowed, but a dictation is under way; not written");
            return;
        };
        tracing::info!("Commands: allowed; writing the Command that asked");
        self.capture_target(false);
        let _ = self.app.emit("yap-state", "processing");
        self.run_agent(instruction, String::new(), None).await;
        busy.done();
    }
}
