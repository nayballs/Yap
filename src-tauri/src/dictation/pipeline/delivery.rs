//! A dictation's text, the rest of the way: AI cleanup for the app it's
//! going into, the corrections, fitting it to the app and the text
//! around the cursor, the paste and the submit key, history — or into
//! the Chat view's box, or to the Voice Assistant.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

use crate::config;

#[cfg(debug_assertions)]
use super::busy::CRASH_NEXT;
use super::finish::{apply_corrections, as_said_text, finish_dictation_with};
#[cfg(debug_assertions)]
use super::LAST_DELIVERED;
use super::{Delivered, Shared};

/// Undo couldn't get back to the window the dictation was for (it closed,
/// or Windows wouldn't bring it to the front): the text is on the clipboard
/// instead of in whatever has the focus now.
pub(super) fn undo_window_gone() -> crate::notices::alert::Alert {
    crate::notices::alert::Alert {
        kind: "pasteBlocked",
        icon: "clipboard",
        title: "Your text is copied: press Ctrl+V".to_string(),
        body: "Yap couldn't get back to the window you were dictating into, so your dictation is on the clipboard instead."
            .to_string(),
        action: None,
        timeout_ms: 12_000,
    }
}

impl Shared {
    /// Is this dictation (or Voice Assistant question) "snip" as a whole
    /// ("Say 'snip' to take a screenshot", `snip::is_snip_phrase`)?
    fn snip_said(&self, transcript: &str) -> bool {
        self.config.read().is_ok_and(|c| c.snip_voice) && crate::snip::is_snip_phrase(transcript)
    }
    /// A dictation's transcript (`text`, from the STT), the rest of the way:
    /// AI cleanup (routed per app), the dictionary, the "press enter"
    /// command and snippets, fitting to the kind of app and the text around
    /// the cursor, the paste into the window captured at record start, the
    /// submit key, history. Returns what went to the paste (the robot's
    /// report; a dictation ignores it).
    pub(super) async fn deliver(self: &Arc<Self>, text: String) -> Delivered {
        // Test mode's crash on purpose (`e2e_crash_next_dictation`).
        #[cfg(debug_assertions)]
        if crate::shell::e2e::active() && CRASH_NEXT.swap(false, Ordering::SeqCst) {
            panic!("e2e: a dictation crashing in its paste, on purpose");
        }
        // "Snip" (or "take a screenshot") as the whole dictation: Snip to
        // Chat instead of a paste (snip.rs). Nothing pasted, nothing kept.
        if self.snip_said(text.trim()) {
            tracing::info!("Voice command: snip");
            self.to_chat.store(false, Ordering::SeqCst);
            crate::snip::start(&self.app, crate::snip::Origin::Voice);
            return Delivered::default();
        }
        // Smart routing: which app were we dictating into? The
        // foreground window was captured at record-start; resolve
        // it to a process name (e.g. "slack.exe") so per-app
        // cleanup rules can pick a matching body.
        let target_hwnd = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };
        let target_app = crate::pasting::text_injector::app_name_for(target_hwnd);
        // …and what kind of app it is, for the style (`cleanup_styles`).
        let style_target = crate::ai::cleanup_styles::Target::of_window(target_hwnd, target_app.as_deref(), self.read_kind());

        let (dict, append_space, auto_submit, auto_submit_key, restore_clipboard) = self
            .config
            .read()
            .map(|c| {
                (
                    c.dictionary.clone(),
                    c.append_trailing_space,
                    c.auto_submit,
                    c.auto_submit_key.clone(),
                    c.restore_clipboard,
                )
            })
            .unwrap_or_else(|_| (Vec::new(), false, false, "enter".to_string(), true));

        // Optional AI cleanup pass (best-effort, `clean_up`). Runs
        // before the dictionary so the user's corrections always have
        // the final say. The state stays `processing` meanwhile.
        let raw = text.trim().to_string();
        // The names on screen, read at record start (`screen_terms`): this
        // dictation's only, and only if the read is done. Taken once, for
        // the cleanup, the corrections and the words as said alike.
        let screen = crate::dictionary::screen_terms::take();
        // A language shortcut that translates (My languages, languages.rs):
        // the AI translates it in cleanup's place; the words as said stay
        // one "Use what I said" away.
        let cfg = self.config.read().map(|c| c.clone()).unwrap_or_default();
        let (cleaned, style) = match crate::dictation::languages::translate_dictation(&self.app, &raw, &cfg).await {
            Some(translated) => (translated, None),
            None => self.clean_up(&raw, &style_target, screen.as_deref()).await,
        };

        // The correction dictionary (exact, then fuzzy for
        // ONNX models), exactly as meetings and Upload get it,
        // then the names on screen, the "press enter" voice command
        // and snippets (dictations only).
        let (corrected, voice_enter) = self
            .config
            .read()
            .map(|c| finish_dictation_with(cleaned.trim(), &raw, &c, screen.as_deref()))
            .unwrap_or_else(|_| (config::apply_dictionary(cleaned.trim(), &dict), false));
        // …and the words as said, when cleanup changed them: what "Use
        // what I said" swaps the paste for (`as_said.rs`) — the same
        // corrections and names on screen, no cleanup.
        let said = (cleaned.trim() != raw)
            .then(|| self.config.read().ok().map(|c| as_said_text(&raw, &c, screen.as_deref())))
            .flatten()
            .filter(|s| !s.trim().is_empty() && *s != corrected);
        let mut delivered = self
            .deliver_text(raw, corrected, voice_enter, said, dict, append_space, auto_submit, auto_submit_key, restore_clipboard)
            .await;
        delivered.style = style.map(crate::ai::cleanup_styles::Style::describe);
        delivered
    }

    /// AI cleanup of a transcript (`raw`) for the app it goes into
    /// (`target`), best-effort: any error or timeout gives the transcript
    /// back, so dictation is never blocked; an empty answer is a deliberate
    /// one (filler only). `screen`: the names on screen the dictation read
    /// (`screen_terms`), for the line telling the model how they're
    /// spelled. Also returns the style it was written in, if any
    /// (`cleanup_styles`).
    pub(super) async fn clean_up(
        &self,
        raw: &str,
        target: &crate::ai::cleanup_styles::Target,
        screen: Option<&crate::dictionary::screen_terms::ScreenTerms>,
    ) -> (String, Option<crate::ai::cleanup_styles::Style>) {
        // Snapshot the cleanup-related config under one read lock.
        // `plan` is the routed cleanup (`cleanup_styles::resolve`): the body
        // and style to clean with, or `None` to skip cleanup (selected-apps-
        // only scope + no matching rule, or the kind of app's style is "As
        // said"). The
        // endpoint comes from the matched profile's LLM override when it
        // has one, else the global settings — either way "ondevice" routes
        // through the sidecar when it's up.
        let (dict, pp_enabled, pp_base_url, pp_api_key, pp_model, pp_provider, plan, pp_disable_thinking) = self
            .config
            .read()
            .map(|c| {
                let plan = crate::ai::cleanup_styles::resolve(&c, target);
                let (pp_base_url, pp_api_key, pp_model, pp_provider) =
                    match plan.as_ref().and_then(|p| p.endpoint.as_ref()) {
                        Some((provider, base_url, api_key, model)) => {
                            // Profile keys fall back to the global
                            // per-provider store, like scopes do.
                            let key = c.provider_api_key(provider, api_key);
                            crate::ai::local_llm::effective_endpoint_for(
                                provider, base_url, &key, model,
                            )
                        }
                        None => crate::ai::local_llm::effective_endpoint(&c),
                    };
                (
                    c.dictionary.clone(),
                    c.post_process_enabled,
                    pp_base_url,
                    pp_api_key,
                    pp_model,
                    pp_provider,
                    plan,
                    c.pp_disable_thinking,
                )
            })
            .unwrap_or_else(|_| (Vec::new(), false, String::new(), String::new(), String::new(), String::new(), None, false));

        // `plan` is `None` when smart routing says "skip cleanup for this
        // app" (selected-apps-only scope, unbound app) or its style is
        // "As said".
        match (&plan, pp_enabled && !pp_base_url.is_empty() && !raw.is_empty()) {
            (Some(plan), true) => {
                // "Write in my region's spelling" (Dictation cleanup): British "colour"
                // for en-GB, American "color" for en-US.
                // English only: none for a dictation known to be in another
                // language (My languages, languages.rs).
                let spelling = self.config.read().ok().and_then(|c| crate::dictation::languages::cleanup_spelling(&c));
                // The names on screen the transcript seems to hold (after
                // the style's line, just before the transcript), after the
                // dictation's language: kept, never translated.
                let names = crate::dictionary::screen_terms::cleanup_line(screen, raw, &dict);
                let names = match self.config.read() {
                    Ok(c) => crate::dictation::languages::with_language_line(names, &c),
                    Err(_) => names,
                };
                if let Some(s) = plan.style {
                    tracing::debug!(style = %s.describe(), "AI cleanup's style");
                }
                match crate::privacy::net::within(crate::privacy::net::Feature::Dictation, crate::ai::cleanup_styles::clean(
                    raw,
                    &pp_base_url,
                    &pp_api_key,
                    &pp_model,
                    &pp_provider,
                    plan,
                    spelling.as_deref(),
                    names.as_deref(),
                    &dict,
                    pp_disable_thinking,
                ))
                .await
                {
                    // Ok(empty) is a DELIBERATE empty result (filler-only
                    // input) — keep it empty so nothing is injected. Only a
                    // real request error falls back to the raw transcript.
                    // A filler a model left at the start of another
                    // language's dictation comes off (languages.rs).
                    Ok(c) => (
                        self.config.read().map(|cfg| crate::dictation::languages::tidy_cleaned(&c, &cfg)).unwrap_or(c),
                        plan.style,
                    ),
                    Err(e) => {
                        tracing::warn!("AI cleanup failed, using raw: {}", e);
                        (raw.to_string(), None)
                    }
                }
            }
            _ => (raw.to_string(), None),
        }
    }

    /// The rest of [`Shared::deliver`]: a finished dictation (`corrected`,
    /// and its words as said when cleanup changed them) to the Chat view's
    /// box, or fitted and pasted, the submit key, history.
    #[allow(clippy::too_many_arguments)]
    async fn deliver_text(
        self: &Arc<Self>,
        raw: String,
        mut corrected: String,
        voice_enter: bool,
        said: Option<String>,
        dict: Vec<config::DictionaryEntry>,
        append_space: bool,
        auto_submit: bool,
        auto_submit_key: String,
        restore_clipboard: bool,
    ) -> Delivered {
        // A question for the Chat view: its page puts the text in the box
        // (and sends it, hands-free or after "press enter"); nothing is
        // pasted, no key pressed.
        if self.to_chat.swap(false, Ordering::SeqCst) {
            return self.deliver_to_chat(raw, corrected, voice_enter);
        }
        // A terminal or a chat app (`app_kind.rs`), as the
        // settings have it: how the text is fitted and which
        // key, if any, follows the paste.
        let kind = self.paste_kind();
        let submit =
            crate::pasting::app_kind::submit_key(kind, auto_submit, voice_enter, &auto_submit_key)
                .map(str::to_string);
        if voice_enter {
            tracing::info!("Voice command: press {}", submit.as_deref().unwrap_or("enter"));
        } else if auto_submit && submit.is_none() {
            tracing::info!("Auto-submit: not pressing Enter at a terminal's prompt");
        }
        // The paste keys follow the window as read, whatever the settings
        // (Git Bash takes Shift+Insert with "Safer pasting" off too).
        let read = self.read_kind();
        let mut report = Delivered {
            raw: raw.clone(),
            kind: kind.describe(),
            submit: submit.clone(),
            paste: read.paste_key().name().to_string(),
            ..Delivered::default()
        };
        if !corrected.is_empty() {
            // Fitted to the kind of app and the text around
            // the cursor (smart spacing), else the trailing
            // space as before — the words as said too, to the
            // same place.
            let said = match said {
                Some(said) => {
                    let (text, said) = crate::pasting::text_injector::prepare_dictation_as_said(&corrected, &said, append_space, &dict, kind);
                    corrected = text;
                    Some(said)
                }
                None => {
                    corrected = crate::pasting::text_injector::prepare_dictation(&corrected, append_space, &dict, kind);
                    None
                }
            };
            report.text = corrected.clone();
            report.as_said = said.clone();
            // Why the paste didn't land, for history (a failed paste is
            // listed so it can be copied or run again).
            let mut not_pasted: Option<&'static str> = None;
            // The log says how much, never what (the text only in Debug logging).
            tracing::info!(chars = corrected.chars().count(), "Transcript");
            tracing::debug!(text = %corrected, "Transcript text");
            let _ = self.app.emit("yap-transcript", corrected.clone());
            let target = match self.target_hwnd.load(Ordering::Relaxed) {
                0 => None,
                h => Some(h),
            };
            match crate::pasting::text_injector::inject_text(
                &corrected,
                restore_clipboard,
                target,
                self.undoing.load(Ordering::SeqCst),
                read,
            )
            .await
            {
                // An app running as administrator took no keys: the
                // text is on the clipboard, and the person is told
                // to paste it (no submit key, no correction watch).
                Ok(crate::pasting::text_injector::Delivery::LeftOnClipboard) => {
                    // Nothing in the field to swap or rewrite.
                    crate::pasting::as_said::forget();
                    report.left_on_clipboard = true;
                    not_pasted = Some("Left on the clipboard: the app runs as administrator");
                    if submit.is_some() {
                        tracing::info!("Not pressing the submit key: the app runs as administrator");
                    }
                    crate::notices::alert::show_soon(&self.app, crate::pasting::elevated::alert());
                }
                // An Undo whose window is gone (or won't come to the
                // front): never into another one; on the clipboard.
                Ok(crate::pasting::text_injector::Delivery::WindowGone) => {
                    crate::pasting::as_said::forget();
                    report.left_on_clipboard = true;
                    report.error = Some("the dictation's window is gone".into());
                    not_pasted = Some("Left on the clipboard: its window was gone");
                    crate::notices::alert::show_soon(&self.app, undo_window_gone());
                }
                Ok(delivery @ (crate::pasting::text_injector::Delivery::Pasted | crate::pasting::text_injector::Delivery::Typed)) => {
                    // The paste didn't land and the text was typed instead.
                    report.typed = delivery == crate::pasting::text_injector::Delivery::Typed;
                    // Once, whether auto-submit, the voice
                    // command or both asked for it.
                    if let Some(key) = &submit {
                        if let Err(e) = crate::pasting::text_injector::press_submit(key).await {
                            tracing::warn!("Auto-submit failed: {}", e);
                            report.error = Some(e);
                        }
                        // Sent: nothing left in the field to swap.
                        crate::pasting::as_said::forget();
                    } else {
                        // The last paste (as_said.rs): it can be swapped for
                        // the words as said — the bar offers it when cleanup
                        // changed the words — or rewritten by the rewrite
                        // shortcut with nothing selected.
                        let seq = crate::pasting::as_said::remember(target, read, &corrected, said.as_deref(), restore_clipboard);
                        let offer = self.config.read().map(|c| c.as_said_offer).unwrap_or(false);
                        report.offer = seq.filter(|_| {
                            offer && said.as_deref().is_some_and(|s| crate::pasting::as_said::words_differ(&corrected, s))
                        });
                        // "Suggest words from your corrections":
                        // watch the field for a fix (learn.rs).
                        // Not a terminal's: its text is the
                        // program's output too.
                        let terminal =
                            self.target_kind.lock().map(|k| k.is_terminal()).unwrap_or(true);
                        let learn = !terminal
                            && self.config.read().map(|c| c.learn_words).unwrap_or(false);
                        crate::dictionary::learn::after_paste(&self.app, &corrected, target, learn);
                    }
                }
                Err(e) => {
                    tracing::warn!("Inject failed: {}", e);
                    report.error = Some(e);
                    not_pasted = Some("The paste didn't go through");
                    crate::pasting::as_said::forget();
                }
            }

            // Local-only history for the stats dashboard
            // (best-effort, gated by `history_enabled`), with the
            // dictation's audio (recent_audio.rs) — and why the paste
            // didn't land, if it didn't.
            let (history_enabled, model) = self
                .config
                .read()
                .map(|c| (c.history_enabled, c.model_size.clone()))
                .unwrap_or((false, String::new()));
            let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
            if history_enabled {
                let app_name =
                    crate::pasting::text_injector::app_name_for(target).unwrap_or_default();
                // With its words as said when cleanup changed them (Home's
                // "AI edited · N changes" and "Paste what I said").
                let ts = crate::dictation::history::record_dictation(crate::dictation::history::Dictation {
                    raw: &raw,
                    text: corrected.trim(),
                    said: said.as_deref(),
                    model: &model,
                    app: &app_name,
                    audio: audio.as_deref(),
                    status: not_pasted.map(|_| crate::dictation::history::NOT_PASTED),
                    error: not_pasted,
                });
                // The last paste is this entry (Home can swap it there).
                if let Some(ts) = ts {
                    crate::pasting::as_said::recorded(ts, corrected.trim());
                }
                // Its audio written (Home lists it with Run again).
                if let Some(id) = &audio {
                    crate::dictation::recent_audio::finished(id);
                }
                // After the save: `yap-transcript` went out
                // before the paste, so a view that re-pulled
                // history on it could miss this dictation.
                let _ = self.app.emit("yap-history-changed", ());
            } else if let Some(id) = audio {
                crate::dictation::recent_audio::discard(&id);
            }
        } else if voice_enter {
            // Only "press enter" was said: nothing to paste
            // (which would have refocused the dictation's
            // window), so press it only while that window
            // still has the focus — never in another one.
            let target = match self.target_hwnd.load(Ordering::Relaxed) {
                0 => None,
                h => Some(h),
            };
            let key = submit.as_deref().unwrap_or(&auto_submit_key);
            if target.is_none() || crate::pasting::text_injector::current_foreground() == target {
                if let Err(e) = crate::pasting::text_injector::press_submit(key).await {
                    tracing::warn!("Voice command submit failed: {}", e);
                    report.error = Some(e);
                }
            } else {
                tracing::info!("Voice command: the dictation's window lost the focus, not pressing");
                report.error = Some("the dictation's window lost the focus".into());
            }
            report.submit = Some(key.to_string());
        }
        #[cfg(debug_assertions)]
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = Some(report.clone());
        }
        report
    }

    /// A dictated question for the Chat view (`to_chat`): `yap-chat-dictation
    /// {text, send}` — the page inserts `text` at its caret and, when `send`
    /// ("Talk back hands-free", or "press enter" said at the end;
    /// `tts::sends_by_itself`), sends it as asked by voice. Kept in history
    /// like any dictation.
    fn deliver_to_chat(&self, raw: String, text: String, said_enter: bool) -> Delivered {
        let text = text.trim().to_string();
        let send = self
            .config
            .read()
            .map(|c| crate::voice::tts::sends_by_itself(c.speak_replies, c.talk_back_hands_free, said_enter))
            .unwrap_or(said_enter);
        tracing::info!(len = text.len(), send, "Dictation: into the Chat view's box");
        let _ = self.app.emit("yap-chat-dictation", serde_json::json!({ "text": text, "send": send }));
        if !text.is_empty() {
            let _ = self.app.emit("yap-transcript", text.clone());
            let (history_enabled, model) = self
                .config
                .read()
                .map(|c| (c.history_enabled, c.model_size.clone()))
                .unwrap_or((false, String::new()));
            if history_enabled {
                let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
                crate::dictation::history::record_with(&raw, &text, &model, "yap.exe", audio.as_deref(), None, None);
                if let Some(id) = audio {
                    crate::dictation::recent_audio::finished(&id);
                }
                let _ = self.app.emit("yap-history-changed", ());
            }
        }
        let report = Delivered {
            text,
            raw,
            kind: "the Chat view".into(),
            submit: send.then(|| "enter".to_string()),
            ..Delivered::default()
        };
        #[cfg(debug_assertions)]
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = Some(report.clone());
        }
        report
    }

    /// A question for the **Voice Assistant**, transcribed: the correction
    /// dictionary as for any dictation (no cleanup, voice command or
    /// snippets: the AI reads it as asked), then `assistant::heard` — Chat
    /// answers and Yap reads it aloud. `false`: nothing was said.
    pub(super) fn ask_assistant(&self, transcript: &str) -> bool {
        let question = self
            .config
            .read()
            .map(|c| apply_corrections(transcript, &c))
            .unwrap_or_else(|_| transcript.to_string());
        let question = question.trim().to_string();
        if question.is_empty() {
            tracing::info!("Voice Assistant: nothing heard");
            crate::chat::assistant::nothing(&self.app);
            return false;
        }
        // "Snip" said as the whole question: a screenshot, not a question.
        if self.snip_said(&question) {
            crate::chat::assistant::nothing(&self.app);
            crate::snip::start(&self.app, crate::snip::Origin::Voice);
            return true;
        }
        tracing::info!(len = question.len(), "Voice Assistant: a question");
        crate::chat::assistant::heard(&self.app, question);
        true
    }
    /// The dictation's last state: "idle", or "cleaned" — the bar's "Cleaned
    /// up · Use what I said" — for [`as_said::OFFER_MS`] when it offers the
    /// swap (`offer`), then "idle" unless something came after it.
    pub(super) fn end_state(self: &Arc<Self>, offer: Option<u64>) {
        let Some(seq) = offer else {
            let _ = self.app.emit("yap-state", "idle");
            return;
        };
        let _ = self.app.emit("yap-state", "cleaned");
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new().name("as-said-offer".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(crate::pasting::as_said::OFFER_MS));
            // Still this dictation's offer (not swapped, no newer one) and
            // nothing recording or transcribing: the capsule goes.
            let quiet = !shared.recording.load(Ordering::SeqCst) && !shared.processing.load(Ordering::SeqCst);
            if crate::pasting::as_said::offered(seq) && quiet {
                let _ = shared.app.emit("yap-state", "idle");
            }
        });
    }

    /// The dictation never got transcribed (no model, the model failed):
    /// listed in history as failed with its audio, so it can be run again.
    pub(super) fn list_failed(&self, why: &str) {
        let Some(id) = self.audio_id.lock().ok().and_then(|mut a| a.take()) else {
            return;
        };
        let (history, model) = self
            .config
            .read()
            .map(|c| (c.history_enabled, c.model_size.clone()))
            .unwrap_or((false, String::new()));
        if !history {
            crate::dictation::recent_audio::discard(&id);
            return;
        }
        let target = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };
        let app_name = crate::pasting::text_injector::app_name_for(target).unwrap_or_default();
        crate::dictation::history::record_failed(None, &model, &app_name, &id, why);
        crate::dictation::recent_audio::finished(&id);
        tracing::info!(id = %id, "A dictation that failed is in history with its audio (Run again)");
        let _ = self.app.emit("yap-history-changed", ());
    }
    /// Audio kept for a dictation that ended up nowhere (a Command, nothing
    /// heard): deleted now.
    pub(super) fn drop_kept_audio(&self) {
        if let Some(id) = self.audio_id.lock().ok().and_then(|mut a| a.take()) {
            crate::dictation::recent_audio::discard(&id);
        }
    }
}
