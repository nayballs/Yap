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
use super::steps::Steps;
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
            self.to_note.store(false, Ordering::SeqCst);
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
        // The person moved to another text box in that window while
        // talking: the paste goes there, the text around its cursor read
        // afresh for the fit and cleanup (`selection::follow_focus`).
        if !self.to_chat.load(Ordering::SeqCst) && !self.to_note.load(Ordering::SeqCst) {
            let _ = tokio::task::spawn_blocking(crate::pasting::selection::follow_focus).await;
        }
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
        let cfg = self.config.read().map(|c| c.clone()).unwrap_or_default();
        // Hindi written as Hinglish (My languages, `languages/hinglish.rs`):
        // Whisper's Devanagari in Latin letters, here on this PC, so the
        // cleanup, the words as said and history all have it so.
        let raw = crate::dictation::languages::hinglish_text(text.trim(), &cfg);
        // The names on screen, read at record start (`screen_terms`): this
        // dictation's only, and only if the read is done. Taken once, for
        // the cleanup, the corrections and the words as said alike.
        let screen = crate::dictionary::screen_terms::take();
        // A language shortcut that translates (My languages, languages.rs):
        // the AI translates it in cleanup's place; the words as said stay
        // one "Use what I said" away.
        let translation = crate::dictation::languages::translate_dictation(&self.app, &raw, &cfg).await;
        let translated = translation.is_some();
        let (cleaned, style) = match translation {
            Some(translated) => (translated, None),
            None => self.clean_up(&raw, &style_target, screen.as_deref(), true).await,
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
        // A chat app (Slack, WhatsApp…, desktop or by its tab's title, as
        // cleanup's styles have them): where stacked messages go.
        let category = crate::ai::cleanup_styles::category(&cfg, &style_target);
        let chat = self.read_kind() == crate::pasting::app_kind::AppKind::Messaging
            || matches!(
                category,
                crate::ai::cleanup_styles::Category::WorkChat | crate::ai::cleanup_styles::Category::PersonalChat
            );
        // Translated, or a transform on every dictation rewrote it: its words
        // as said aren't this text (another language, perhaps), so stacked
        // messages never split them in its place.
        let rewritten = translated
            || crate::dictation::transforms::for_dictation(&cfg, category, style_target.is_terminal()).is_some();
        // What Yap did by itself, for the feature tips (`tips.rs`).
        let seen = crate::dictation::tips::Seen::of(&raw, &cleaned, &cfg.dictionary, style);
        let mut delivered = self
            .deliver_text(
                raw,
                corrected,
                voice_enter,
                said,
                dict,
                append_space,
                auto_submit,
                auto_submit_key,
                restore_clipboard,
                Steps { chat, screen, rewritten },
            )
            .await;
        delivered.style = style.map(crate::ai::cleanup_styles::Style::describe);
        // A first dictation ends the bar's first-use pointers; a feature tip
        // once it's pasted.
        crate::bar::pointer::dictated();
        if !delivered.text.is_empty() && !delivered.left_on_clipboard && delivered.error.is_none() {
            crate::dictation::tips::after_dictation(&self.app, seen);
        }
        delivered
    }

    /// AI cleanup of a transcript (`raw`) for the app it goes into
    /// (`target`), best-effort: any error or timeout gives the transcript
    /// back, so dictation is never blocked; an empty answer is a deliberate
    /// one (filler only). `screen`: the names on screen the dictation read
    /// (`screen_terms`), for the line telling the model how they're
    /// spelled. Also returns the style it was written in, if any
    /// (`cleanup_styles`). `watch`: a dictation's (not Run again's) — past
    /// its usual time the bar says so and offers Skip cleanup, which gives
    /// the transcript back at once (`pipeline/slow.rs`).
    pub(super) async fn clean_up(
        &self,
        raw: &str,
        target: &crate::ai::cleanup_styles::Target,
        screen: Option<&crate::dictionary::screen_terms::ScreenTerms>,
        watch: bool,
    ) -> (String, Option<crate::ai::cleanup_styles::Style>) {
        match self.cleanup_pass(raw, target, screen, watch).await {
            // Skip cleanup (the bar): the words as transcribed, nothing more.
            (text, style, true) => (text, style),
            // Then a transform on every dictation, if one is picked.
            (text, style, false) => (self.transform_after(text, target, watch).await, style),
        }
    }

    /// **A transform on every dictation** (`transforms::for_dictation`):
    /// after the cleanup, once, on its result — watched as the cleanup is
    /// (Skip gives the cleaned text back); no model or a failure pastes the
    /// cleaned text. Not for the Chat view's box (a question for Yap's AI).
    async fn transform_after(&self, text: String, target: &crate::ai::cleanup_styles::Target, watch: bool) -> String {
        if text.trim().is_empty() || self.to_chat.load(Ordering::SeqCst) {
            return text;
        }
        let Ok(cfg) = self.config.read().map(|c| c.clone()) else { return text };
        let kind = crate::ai::cleanup_styles::category(&cfg, target);
        let Some(t) = crate::dictation::transforms::for_dictation(&cfg, kind, target.is_terminal()) else { return text };
        let call = crate::dictation::transforms::run_on_dictation_in(&cfg, &t, &text, Some(kind));
        let result = if watch {
            let m = super::commands_model(&cfg);
            let key = format!("transform:{}", crate::dictation::slow::cleanup_key(&m.provider, &m.base_url, &m.model));
            match self.watched_cleanup(call, &key, text.chars().count() as u64).await {
                Some(r) => r,
                None => return text,
            }
        } else {
            call.await
        };
        result.unwrap_or_else(|e| {
            tracing::warn!("A transform on every dictation didn't run: {e}");
            text
        })
    }

    /// The AI cleanup itself ([`Shared::clean_up`]): the text, the style it
    /// was written in, and whether the bar's Skip cleanup cut it short.
    async fn cleanup_pass(
        &self,
        raw: &str,
        target: &crate::ai::cleanup_styles::Target,
        screen: Option<&crate::dictionary::screen_terms::ScreenTerms>,
        watch: bool,
    ) -> (String, Option<crate::ai::cleanup_styles::Style>, bool) {
        // Snapshot the cleanup-related config under one read lock.
        // `plan` is the routed cleanup (`cleanup_styles::resolve`): the body
        // and style to clean with, or `None` to skip cleanup (selected-apps-
        // only scope + no matching rule, or the kind of app's style is "As
        // said"). The
        // endpoint comes from the matched profile's LLM override when it
        // has one, else the global settings — either way "ondevice" routes
        // through the sidecar when it's up.
        let snapshot = || {
            self.config
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
                .unwrap_or_else(|_| (Vec::new(), false, String::new(), String::new(), String::new(), String::new(), None, false))
        };
        let (mut dict, mut pp_enabled, mut pp_base_url, mut pp_api_key, mut pp_model, mut pp_provider, mut plan, mut pp_disable_thinking) = snapshot();
        // Yap's on-device model isn't loaded (yet: it started loading when
        // the dictation did, `local_llm::on_demand`): wait for it — "Loading
        // the AI model…" on the bar, with Skip cleanup — then clean with it.
        // It won't start: the words as said, and the person told once.
        if pp_enabled && plan.is_some() && !raw.trim().is_empty() && crate::ai::local_llm::on_demand::is_standby(&pp_base_url) {
            match Box::pin(self.wait_for_model(watch)).await {
                None => return (raw.to_string(), None, true),
                Some(Err(e)) => {
                    tracing::warn!("AI cleanup went without the on-device model: {e}");
                    crate::ai::local_llm::on_demand::say_failed_once(&self.app);
                    return (raw.to_string(), None, false);
                }
                Some(Ok(_)) => {
                    (dict, pp_enabled, pp_base_url, pp_api_key, pp_model, pp_provider, plan, pp_disable_thinking) = snapshot();
                }
            }
        }

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
                let about_name = self.config.read().map(|c| c.about_name.clone()).unwrap_or_default();
                let names = crate::dictionary::screen_terms::cleanup_line(screen, raw, &dict, &about_name);
                let names = match self.config.read() {
                    Ok(c) => crate::dictation::languages::with_language_line(names, &c),
                    Err(_) => names,
                };
                // The text before the cursor (`cleanup_context.rs`), last
                // before the transcript: a live dictation's only, to a model
                // on this PC, or a cloud one with its switch on.
                let cursor = if watch {
                    self.config.read().ok().and_then(|c| crate::ai::cleanup_context::for_cleanup(&c, &pp_base_url))
                } else {
                    None
                };
                let names = match &cursor {
                    Some(before) => {
                        crate::ai::cleanup_styles::before_transcript(names.as_deref(), Some(&crate::ai::cleanup_context::line(before)))
                    }
                    None => names,
                };
                // The receipts say when the text before the cursor went too.
                let feature = if cursor.is_some() {
                    crate::privacy::net::Feature::DictationContext
                } else {
                    crate::privacy::net::Feature::Dictation
                };
                if let Some(s) = plan.written_style() {
                    tracing::debug!(style = %s.describe(), strength = plan.strength.id(), "AI cleanup's style");
                }
                let call = crate::privacy::net::within(feature, crate::ai::cleanup_styles::clean(
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
                ));
                let result = if watch {
                    let key = crate::dictation::slow::cleanup_key(&pp_provider, &pp_base_url, &pp_model);
                    match self.watched_cleanup(call, &key, raw.chars().count() as u64).await {
                        Some(result) => result,
                        // Skip cleanup (the bar): the transcript as it is.
                        None => return (raw.to_string(), None, true),
                    }
                } else {
                    call.await
                };
                match result {
                    // Ok(empty) is a DELIBERATE empty result (filler-only
                    // input) — keep it empty so nothing is injected. Only a
                    // real request error falls back to the raw transcript.
                    // A filler a model left at the start of another
                    // language's dictation comes off (languages.rs).
                    Ok(c) => {
                        // What it wrote out again of the text before the
                        // cursor goes.
                        let c = match &cursor {
                            Some(before) => crate::ai::cleanup_context::trim_echo(before, &c),
                            None => c,
                        };
                        // Hinglish: a cleanup that translated it gives way
                        // to the words as spelt out; anything the model
                        // still wrote in Devanagari, in Latin letters too.
                        let c = crate::dictation::languages::hinglish_kept(raw, c);
                        let c = self
                            .config
                            .read()
                            .map(|cfg| crate::dictation::languages::hinglish_text(&crate::dictation::languages::tidy_cleaned(&c, &cfg), &cfg))
                            .unwrap_or(c);
                        (c, plan.written_style(), false)
                    }
                    Err(e) => {
                        tracing::warn!("AI cleanup failed, using raw: {}", e);
                        (raw.to_string(), None, false)
                    }
                }
            }
            _ => (raw.to_string(), None, false),
        }
    }

    /// The rest of [`Shared::deliver`]: a finished dictation (`corrected`,
    /// and its words as said when cleanup changed them) to the Chat view's
    /// box, or fitted and pasted — in steps when it's several messages or
    /// has a mention or a file to pick (`steps`, `pasting/compose.rs`) —,
    /// the submit key, history.
    #[allow(clippy::too_many_arguments)]
    async fn deliver_text(
        self: &Arc<Self>,
        raw: String,
        mut corrected: String,
        voice_enter: bool,
        mut said: Option<String>,
        dict: Vec<config::DictionaryEntry>,
        append_space: bool,
        auto_submit: bool,
        auto_submit_key: String,
        restore_clipboard: bool,
        steps: Steps,
    ) -> Delivered {
        // A question for the Chat view: its page puts the text in the box
        // (and sends it, hands-free or after "press enter"); nothing is
        // pasted, no key pressed.
        if self.to_chat.swap(false, Ordering::SeqCst) {
            return self.deliver_to_chat(raw, corrected, voice_enter);
        }
        // The quick note's: its page puts the text at its cursor.
        if self.to_note.swap(false, Ordering::SeqCst) {
            return self.deliver_to_note(raw, corrected, voice_enter);
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
        // Stacked messages (`stacked.rs`): "new message" said, in a chat app.
        let messages = self.stacked(&mut corrected, &mut said, steps.chat, submit.is_some(), steps.rewritten);
        if !corrected.is_empty() {
            // Fitted to the kind of app and the text around
            // the cursor (smart spacing), else the trailing
            // space as before — the words as said too, to the
            // same place. Stacked messages each to their own box.
            let (fitted, said) = match (&messages, said) {
                (Some(m), _) => (crate::pasting::text_injector::prepare_messages(m, &dict, kind), None),
                (None, Some(said)) => {
                    let (text, said) = crate::pasting::text_injector::prepare_dictation_as_said(&corrected, &said, append_space, &dict, kind);
                    (vec![text], Some(said))
                }
                (None, None) => (vec![crate::pasting::text_injector::prepare_dictation(&corrected, append_space, &dict, kind)], None),
            };
            // Mentions and file tags (`mentions.rs`, `file_tags.rs`): the
            // pieces to paste, type and pick (`compose.rs`).
            let place = self.compose_place(steps.screen.as_deref());
            let (plan, text) = crate::pasting::compose::plan(&fitted, &place, submit.clone());
            corrected = text;
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
            // An Undo, a dictation whose mic was disconnected (`unplugged.rs`)
            // or one stopped at its length limit (`cap.rs`), with nobody
            // pressing anything: into its own window or onto the clipboard,
            // never into whatever has the focus now.
            let unplugged = self.unplugged();
            let capped = self.capped.load(Ordering::SeqCst);
            let strict = self.undoing.load(Ordering::SeqCst) || unplugged || capped;
            // Several messages, or a mention or a file to pick: in steps,
            // the submit key after each message (`compose.rs`).
            let in_steps = plan.in_steps();
            let pasted = if in_steps {
                let run = crate::pasting::compose::Run { target, restore_clipboard, strict, kind: read };
                let outcome = crate::pasting::compose::run(&plan, run).await;
                report.steps = outcome.steps;
                if let Some(stopped) = &outcome.stopped {
                    report.error = Some(format!("stopped after {} of {} messages ({})", stopped.sent, stopped.total, stopped.why.code()));
                    report.left_on_clipboard = !stopped.rest.is_empty();
                    crate::notices::alert::show_soon(&self.app, crate::pasting::compose::stopped_alert(stopped));
                }
                outcome.delivery
            } else {
                crate::pasting::text_injector::inject_text(&corrected, restore_clipboard, target, strict, read).await
            };
            match pasted {
                // Not pasted (an app running as administrator, keys
                // Windows didn't take, no text box with the focus): the
                // text is on the clipboard, and the person is told once
                // to paste it (no submit key, no correction watch).
                Ok(crate::pasting::text_injector::Delivery::LeftOnClipboard(left)) => {
                    // Nothing in the field to swap or rewrite.
                    crate::pasting::as_said::forget();
                    report.left_on_clipboard = true;
                    report.left_why = Some(left.code());
                    not_pasted = Some(left.not_pasted());
                    if submit.is_some() {
                        tracing::info!(why = left.code(), "Not pressing the submit key: nothing was pasted");
                    }
                    if !self.tell_unplugged(super::unplugged::Outcome::OnClipboard) {
                        crate::notices::alert::show_soon(&self.app, left.alert());
                    }
                }
                // An Undo whose window is gone (or won't come to the
                // front): never into another one; on the clipboard.
                Ok(crate::pasting::text_injector::Delivery::WindowGone) => {
                    crate::pasting::as_said::forget();
                    report.left_on_clipboard = true;
                    report.error = Some("the dictation's window is gone".into());
                    not_pasted = Some(if unplugged {
                        super::unplugged::NOT_PASTED
                    } else {
                        "Left on the clipboard: its window was gone"
                    });
                    // One alert: the mic's, saying where the text is (a
                    // dictation stopped at its limit says so itself, below).
                    if !self.tell_unplugged(super::unplugged::Outcome::OnClipboard) && !capped {
                        crate::notices::alert::show_soon(&self.app, undo_window_gone());
                    }
                }
                Ok(delivery @ (crate::pasting::text_injector::Delivery::Pasted | crate::pasting::text_injector::Delivery::Typed)) => {
                    // The paste didn't land and the text was typed instead.
                    report.typed = delivery == crate::pasting::text_injector::Delivery::Typed;
                    // Its mic was unplugged: said, now it's in its window.
                    self.tell_unplugged(super::unplugged::Outcome::Pasted);
                    if in_steps {
                        // The plan pressed the submit key after each
                        // message; a picked mention or file can't be
                        // selected back exactly: nothing to swap or rewrite.
                        crate::pasting::as_said::forget();
                    } else if let Some(key) = &submit {
                        // Once, whether auto-submit, the voice
                        // command or both asked for it.
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
            // Stopped at its length limit: said, with Recover text (`cap.rs`).
            if capped {
                crate::dictation::cap::stopped(&self.app, &corrected, not_pasted.is_none());
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
            // How long it was spoken for (words a minute).
            let secs = self.audio_secs.lock().ok().and_then(|mut s| s.take());
            let app_name = crate::pasting::text_injector::app_name_for(target).unwrap_or_default();
            if history_enabled {
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
                    secs,
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
                // "Type that often? Save it as a snippet" (counted over the
                // week's history: dictionary/snippet_offer.rs).
                crate::dictionary::snippet_offer::after_dictation(&self.app, corrected.trim());
            } else if let Some(id) = audio {
                crate::dictation::recent_audio::discard(&id);
            }
            // The stats (Home, Insights), whether or not history keeps it.
            crate::dictation::stats::dictated(
                &self.app,
                crate::dictation::stats::Counted { raw: &raw, text: corrected.trim(), said: said.as_deref(), app: &app_name, secs, dict: &dict, at: None },
            );
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
            let secs = self.audio_secs.lock().ok().and_then(|mut s| s.take());
            if history_enabled {
                let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
                self.record_in_yap(&raw, &text, &model, audio.as_deref(), secs);
                if let Some(id) = audio {
                    crate::dictation::recent_audio::finished(&id);
                }
                let _ = self.app.emit("yap-history-changed", ());
            }
            self.count_in_yap(&raw, &text, secs);
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

    /// A dictation for the **quick note** (`to_note`): `yap-quick-note-dictation
    /// {text, newline}` — its page puts `text` at its cursor (a new line
    /// after it when "press enter" was said), and saves the note. Nothing
    /// is pasted or pressed. Kept in history like any dictation.
    fn deliver_to_note(&self, raw: String, text: String, said_enter: bool) -> Delivered {
        let text = text.trim().to_string();
        tracing::info!(len = text.len(), said_enter, "Dictation: into the quick note");
        let payload = serde_json::json!({ "text": text, "newline": said_enter });
        let _ = self.app.emit(crate::notes::quick_note::EVENT_DICTATION, payload);
        if !text.is_empty() {
            let _ = self.app.emit("yap-transcript", text.clone());
            let (history_enabled, model) = self
                .config
                .read()
                .map(|c| (c.history_enabled, c.model_size.clone()))
                .unwrap_or((false, String::new()));
            let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
            let secs = self.audio_secs.lock().ok().and_then(|mut s| s.take());
            if history_enabled {
                self.record_in_yap(&raw, &text, &model, audio.as_deref(), secs);
                if let Some(id) = audio {
                    crate::dictation::recent_audio::finished(&id);
                }
                let _ = self.app.emit("yap-history-changed", ());
            } else if let Some(id) = audio {
                crate::dictation::recent_audio::discard(&id);
            }
            self.count_in_yap(&raw, &text, secs);
        }
        let report = Delivered {
            text,
            raw,
            kind: "the quick note".into(),
            submit: said_enter.then(|| "enter".to_string()),
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
        if let Ok(mut s) = self.audio_secs.lock() {
            *s = None;
        }
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
        // Its length too: the next dictation's is its own.
        if let Ok(mut s) = self.audio_secs.lock() {
            *s = None;
        }
    }

    /// A dictation into one of Yap's own windows (Chat's box, the quick
    /// note) in history, with how long it was spoken for.
    fn record_in_yap(&self, raw: &str, text: &str, model: &str, audio: Option<&str>, secs: Option<f32>) {
        crate::dictation::history::record_dictation(crate::dictation::history::Dictation {
            raw,
            text,
            said: None,
            model,
            app: "yap.exe",
            audio,
            status: None,
            error: None,
            secs,
        });
    }

    /// …and in the stats (Home, Insights), whether or not history keeps it.
    fn count_in_yap(&self, raw: &str, text: &str, secs: Option<f32>) {
        let dict = self.config.read().map(|c| c.dictionary.clone()).unwrap_or_default();
        crate::dictation::stats::dictated(
            &self.app,
            crate::dictation::stats::Counted { raw, text, said: None, app: "yap.exe", secs, dict: &dict, at: None },
        );
    }
}
