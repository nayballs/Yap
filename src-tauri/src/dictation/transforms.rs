//! **Saved transforms** (2026-10-24; Wispr Flow's Transforms, OpenWhispr's
//! translation hotkey and Murmure's numbered rewrites, by behaviour): one-key
//! rewrites — Polish, Shorter, Fix grammar only, Translate to <language>,
//! Make it a prompt, and the person's own — applied without saying anything.
//!
//! **The list** (`config.transforms`; Settings → Commands → Transforms, which
//! Settings → Shortcuts links to): each has a name, an instruction and an
//! optional shortcut. Yap's five built-ins (`builtin`) keep Yap's own
//! instructions ([`builtin_instruction`]: a better prompt reaches everyone,
//! and the evals test what people run); their shortcut, place in the list
//! and Translate's language are the person's, and they can't be deleted. The
//! person's own have the name and instruction they wrote, and can be edited,
//! moved and deleted. At most [`MAX`].
//!
//! **Shortcuts**: `input_hook.rs` keeps [`MAX`] transform slots beside its
//! nine fixed bindings. [`assign`] (pure) gives each transform with a
//! shortcut a slot, in list order, unless its key would fire with one of the
//! nine or an earlier transform's ([`crate::dictation::input_hook::clashes`]): it's
//! left unbound and the log says why (Settings refuses such a key in the
//! first place, `hotkeysClash`). Only the press is used. No in-page fallback:
//! a transform works on another app's text.
//!
//! **What a press does** ([`run`], on its own thread, under the pipeline's
//! `processing` guard — never beside a dictation):
//! 1. the window in front (the tray's: the top app window, brought back), its
//!    kind, and what's selected in its focused field
//!    (`selection::editable_selection`: UI Automation; Ctrl+C only where UI
//!    Automation can't read the field);
//! 2. what to work on ([`pick`], pure): **a terminal never** (its line can't
//!    be selected and replaced) — said, nothing changed; **the selection**,
//!    in a field that takes typing (read-only text or a password box:
//!    refused); **nothing selected**: the **last dictation**, under the
//!    rewrite shortcut's exact rules (`as_said::rewrite_target`: Settings →
//!    Commands' "Rewrite your last dictation when nothing is selected", the
//!    same window, within a minute, still ending at the cursor —
//!    `as_said::rewrite_check`); else "Nothing to transform", said plainly;
//! 3. the Commands scope's model (`pipeline::commands_model`: its own
//!    provider when it's set up, else the cleanup model; on this PC or in the
//!    cloud) with `llm::TRANSFORM_BASE_PROMPT`'s guardrails and the
//!    transform's instruction — never the Commands prompt, which may answer
//!    questions; the bar says the transform's name meanwhile (`yap-transform`
//!    "running" + `yap-state` "processing");
//! 4. the result in place: over the selection, once Yap has checked it's
//!    still selected in the window in front ([`verify_selection`]; else the
//!    field is left alone and the result copied), with the spaces it had
//!    around it (`as_said::keep_spaces`); or over the last dictation, selected
//!    exactly again (`as_said::replace_last`). Either way it's one paste (the
//!    app's own Ctrl+Z takes it back) and the last paste from then on
//!    ("Shorter" after "Polish" works on the polished text);
//! 5. **Undo** on the bar for `as_said::OFFER_MS` ("Polish ✓ · Undo",
//!    `yap-transform` "done" + `yap-state` "cleaned"): the text it replaced
//!    put back, the same exact-selection way (`as_said::undo_transform`);
//!    where that can't be done, copied instead. A result the same as the text
//!    ("No changes needed") pastes nothing.
//!
//! Also in the tray's **Transforms** menu (`tray.rs`): the top app window is
//! brought back and its selection used, as "Read selection aloud" does. Not
//! in the Yap bar's ^ menu, which manages the bar and Yap itself: the bar
//! shows a transform running and its Undo instead.
//!
//! The log says which transform (a built-in's kind, or "custom": the name
//! of a person's own is theirs), the source, the outcome and lengths, never
//! the text. Test runs read, ask and paste through
//! the same steps in a stand-in window (`e2e_transform`); robot mode in the
//! test's own windows (`realtest_transform`).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::pasting::app_kind::AppKind;
use crate::config::YapConfig;
use crate::pasting::selection::FieldSelection;

/// The most transforms the list keeps (and the input hook's slots).
pub const MAX: usize = 12;

/// The built-ins, by `builtin`.
pub const POLISH: &str = "polish";
pub const SHORTER: &str = "shorter";
pub const GRAMMAR: &str = "grammar";
pub const TRANSLATE: &str = "translate";
pub const PROMPT: &str = "prompt";
const BUILTINS: [&str; 5] = [POLISH, SHORTER, GRAMMAR, TRANSLATE, PROMPT];

/// Translate's language until the person picks one.
pub const DEFAULT_LANGUAGE: &str = "Spanish";

/// The bar's event: `{ name, phase: "running" | "done" | "unchanged", offerMs }`.
const EVENT: &str = "yap-transform";
/// What the bar says when nothing changed, for this long.
const UNCHANGED_MS: u64 = 2_500;

/// One saved transform (see the module comment).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Transform {
    /// Stable id: a built-in's kind, else one Settings made ("t-…").
    pub id: String,
    /// Yap's own ([`BUILTINS`]): its name and instruction are Yap's. Empty
    /// for one the person wrote.
    pub builtin: String,
    /// The person's own transform's name ("Make it friendlier").
    pub name: String,
    /// The person's own transform's instruction, as written.
    pub instruction: String,
    /// Its shortcut (`input_hook` spec; empty: none).
    pub hotkey: String,
    /// Translate's language ("French"); empty: [`DEFAULT_LANGUAGE`].
    pub language: String,
}

impl Transform {
    fn builtin(kind: &str) -> Self {
        Transform {
            id: kind.to_string(),
            builtin: kind.to_string(),
            language: if kind == TRANSLATE { DEFAULT_LANGUAGE.to_string() } else { String::new() },
            ..Default::default()
        }
    }

    fn language(&self) -> &str {
        match self.language.trim() {
            "" => DEFAULT_LANGUAGE,
            l => l,
        }
    }

    /// The name people see ("Translate to French").
    pub fn name(&self) -> String {
        match self.builtin.as_str() {
            "" => match self.name.trim() {
                "" => "Untitled transform".to_string(),
                n => n.chars().take(60).collect(),
            },
            TRANSLATE => format!("Translate to {}", self.language()),
            kind => builtin_name(kind).unwrap_or("Transform").to_string(),
        }
    }

    /// What the AI is asked to do.
    pub fn instruction(&self) -> String {
        if self.builtin.is_empty() {
            self.instruction.trim().to_string()
        } else {
            builtin_instruction(&self.builtin, self.language()).unwrap_or_default()
        }
    }

    /// It can run: a built-in Yap knows, or one of the person's with an
    /// instruction.
    pub fn usable(&self) -> bool {
        !self.id.trim().is_empty() && !self.instruction().is_empty()
    }
}

/// A built-in's name.
pub fn builtin_name(kind: &str) -> Option<&'static str> {
    Some(match kind {
        POLISH => "Polish",
        SHORTER => "Shorter",
        GRAMMAR => "Fix grammar only",
        TRANSLATE => "Translate",
        PROMPT => "Make it a prompt",
        _ => return None,
    })
}

/// A built-in's instruction (inside `llm::TRANSFORM_BASE_PROMPT`'s
/// guardrails; measured in docs/model-evals.md, "Saved transforms").
pub fn builtin_instruction(kind: &str, language: &str) -> Option<String> {
    Some(match kind {
        POLISH => "Polish the text: fix grammar, spelling and punctuation; remove filler words (um, uh, like, \
                   basically, you know, I mean), false starts and repetition; and smooth awkward or rambling \
                   phrasing, so it reads clearly and naturally. Keep the meaning, the tone, the person's own words \
                   where they work, and roughly the same length. Add nothing new."
            .to_string(),
        SHORTER => "Make the text shorter: say the same thing in about half as many words. Keep every fact, name, \
                    number, date and request in it; cut filler, repetition and pleasantries. Keep the tone and the \
                    point of view."
            .to_string(),
        GRAMMAR => "Fix only mistakes in grammar, spelling, punctuation and capitals. Change nothing else: keep the \
                    wording, word choice, tone, style, length and line breaks exactly as they are, and keep its \
                    spelling conventions (British or American). If there are no mistakes, return the text unchanged."
            .to_string(),
        TRANSLATE => format!(
            "Translate the text into {language}, all of it, keeping its meaning, tone and layout. Names of \
             people, places, companies and products, numbers, amounts, times, email addresses and links stay as \
             they are; days and months are written in {language}. A question or a request is translated, never \
             answered. The result is in {language}."
        ),
        PROMPT => "Rewrite the text as a clear, well-structured prompt for an AI assistant, written as the person \
                   asking (\"Write…\", \"I need…\"), its spelling and grammar fixed. Start with the task, then the \
                   context and every detail, constraint, name and number the text gives, then the output wanted \
                   (format, length, tone) where it says. Use short paragraphs or a few bullet points where they \
                   help. Don't invent requirements it doesn't give. Never carry out the task or answer the question \
                   yourself: return only the prompt."
            .to_string(),
        _ => return None,
    })
}

/// The list a new config starts with: Yap's five, no shortcuts.
pub fn defaults() -> Vec<Transform> {
    BUILTINS.iter().map(|k| Transform::builtin(k)).collect()
}

/// The transforms that can run, in the person's order: usable ones, each id
/// once, at most [`MAX`].
pub fn list(cfg: &YapConfig) -> Vec<Transform> {
    let mut seen = std::collections::HashSet::new();
    cfg.transforms
        .iter()
        .filter(|t| t.usable() && seen.insert(t.id.clone()))
        .take(MAX)
        .cloned()
        .collect()
}

/// The transform with `id`, if it can run.
pub fn find(cfg: &YapConfig, id: &str) -> Option<Transform> {
    list(cfg).into_iter().find(|t| t.id == id)
}

/// One transform's place in the input hook ([`assign`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    /// Its key, or "" — none, or one that clashes.
    pub spec: String,
    /// What it clashes with ("the dictation hotkey", "the Polish transform").
    pub clash: Option<String>,
}

/// The input hook's transform slots (pure): each listed transform with a
/// shortcut, in order, unless its key would fire with one of `fixed` (the
/// nine fixed shortcuts, by name) or an earlier transform's — then left
/// unbound, with what it clashes with.
pub fn assign(fixed: &[(&str, &str)], transforms: &[Transform]) -> Vec<Slot> {
    let mut out: Vec<Slot> = Vec::new();
    let mut taken: Vec<(String, String)> = Vec::new();
    for t in transforms.iter().filter(|t| !t.hotkey.trim().is_empty()).take(MAX) {
        let spec = t.hotkey.trim();
        let clash = fixed
            .iter()
            .find(|(_, other)| crate::dictation::input_hook::clashes(spec, other))
            .map(|(name, _)| format!("the {name}"))
            .or_else(|| {
                taken
                    .iter()
                    .find(|(_, other)| crate::dictation::input_hook::clashes(spec, other))
                    .map(|(name, _)| format!("the {name} transform"))
            });
        if clash.is_none() {
            taken.push((t.name(), spec.to_string()));
        }
        out.push(Slot { id: t.id.clone(), spec: if clash.is_some() { String::new() } else { spec.to_string() }, clash });
    }
    out
}

/// The nine fixed shortcuts, by name (`input_hook.rs`): what the language
/// shortcuts give way to.
pub fn nine_shortcuts(cfg: &YapConfig) -> Vec<(&'static str, String)> {
    vec![
        ("dictation hotkey", cfg.hotkey.clone()),
        ("rewrite shortcut", cfg.edit_hotkey.clone()),
        ("meeting shortcut", cfg.meeting_hotkey.clone()),
        ("assistant shortcut", cfg.assistant_hotkey.clone()),
        ("read-aloud shortcut", cfg.read_hotkey.clone()),
        ("Use-what-I-said shortcut", cfg.as_said_hotkey.clone()),
        ("snip shortcut", cfg.snip_hotkey.clone()),
        ("Paste-last shortcut", cfg.paste_last_hotkey.clone()),
        ("Copy-last shortcut", cfg.copy_last_hotkey.clone()),
    ]
}

/// The shortcuts a transform's gives way to, by name, as [`assign`] takes
/// them: the nine fixed ones, then the language shortcuts' (My languages,
/// `languages.rs`: a dictation key comes first).
pub fn fixed_shortcuts(cfg: &YapConfig) -> Vec<(&'static str, String)> {
    let mut out = nine_shortcuts(cfg);
    out.extend(crate::dictation::languages::shortcuts(cfg).into_iter().map(|s| ("language shortcut", s.hotkey)));
    out
}

/// What was found where the transform is to work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The selected text, in a field that takes typing.
    Selection,
    /// Nothing selected: the last dictation, just pasted there.
    LastDictation,
    /// A terminal: never (its line can't be selected and replaced).
    Terminal,
    /// Selected text Yap mustn't change: read-only, or a password box.
    ReadOnly,
    /// Nothing selected, and no dictation to take.
    Nothing,
}

/// What a transform works on (pure): never a terminal; the selection; else
/// the last dictation when `as_said::rewrite_target` gave it (`last`); else
/// nothing.
pub fn pick(terminal: bool, found: &FieldSelection, last: bool) -> Pick {
    if terminal {
        return Pick::Terminal;
    }
    match found {
        FieldSelection::Text(t) if !t.trim().is_empty() => Pick::Selection,
        FieldSelection::ReadOnly => Pick::ReadOnly,
        _ if last => Pick::LastDictation,
        _ => Pick::Nothing,
    }
}

/// The model's answer as the text to put in place (pure): `<text>` tags it
/// echoed and a "Here is…:" line before the text taken off.
pub fn clean_output(out: &str) -> String {
    let mut t = out.trim();
    for _ in 0..2 {
        if let Some((first, rest)) = t.split_once('\n') {
            let f = first.trim().to_lowercase();
            let preamble = f.ends_with(':')
                && f.len() <= 90
                && ["here is", "here's", "sure", "okay", "ok,", "certainly", "the polished", "the translated", "the rewritten", "rewritten", "translation", "result", "prompt:"]
                    .iter()
                    .any(|p| f.starts_with(p));
            if preamble && !rest.trim().is_empty() {
                t = rest.trim();
                continue;
            }
        }
        break;
    }
    let t = t.strip_prefix("<text>").unwrap_or(t).trim();
    let t = t.strip_suffix("</text>").unwrap_or(t).trim();
    t.to_string()
}

// ---- A transform on every dictation ----------------------------------------------------

/// **A transform on every dictation** (`config.auto_transform`, Settings →
/// Commands → Transforms; optional, off by default): the transform to run
/// after AI cleanup on a dictation into `kind` (`cleanup_styles::category`),
/// or `None` — none picked, one that can't run, another kind than the one
/// picked (`auto_transform_kind`), or a terminal (its text is a command;
/// transforms never work there). Pure.
///
/// It runs **once**, in the dictation's cleanup step
/// (`pipeline::clean_up`), on what the style's cleanup wrote: its result is
/// never cleaned or transformed again, a transform's own shortcut never
/// starts it (no dictation is delivered), a language shortcut that
/// translates skips cleanup and it, and Run again runs it once on the fresh
/// transcript. "Use what I said" swaps the paste back to the words as said.
pub fn for_dictation(cfg: &YapConfig, kind: crate::ai::cleanup_styles::Category, terminal: bool) -> Option<Transform> {
    let id = cfg.auto_transform.trim();
    if id.is_empty() || terminal || kind == crate::ai::cleanup_styles::Category::Terminal {
        return None;
    }
    let only = cfg.auto_transform_kind.trim();
    if !only.is_empty() && only != kind.id() {
        return None;
    }
    find(cfg, id)
}

/// Run `t` on a dictation's `text` (after its cleanup) on the Commands
/// scope's model, as a dictation's AI call: the result, or why not (no
/// model set up, the AI failed or wrote nothing) — the caller pastes `text`
/// then. The log says which transform and lengths, never the text.
pub async fn run_on_dictation(cfg: &YapConfig, t: &Transform, text: &str) -> Result<String, String> {
    let m = crate::dictation::pipeline::commands_model(cfg);
    let keyed = crate::ai::ai_model::KEYED_PROVIDERS.contains(&m.provider.as_str());
    if m.base_url.is_empty() || (keyed && m.api_key.is_empty()) {
        return Err("no AI model is set up".into());
    }
    let asked = crate::privacy::net::within(
        crate::privacy::net::Feature::Dictation,
        crate::ai::llm::transform(&t.instruction(), text, &m.base_url, &m.api_key, &m.model, &m.provider, m.disable_thinking),
    )
    .await?;
    let out = clean_output(&asked);
    if out.trim().is_empty() {
        return Err("the AI wrote nothing".into());
    }
    tracing::info!(transform = %kind_of(t), chars = out.chars().count(), "A transform on every dictation: done");
    Ok(out)
}

/// Where a transform was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Its shortcut, in the app in front.
    Shortcut,
    /// The tray's Transforms menu: the top app window, brought back.
    Tray,
}

/// What a transform did (the e2e and robot specs' report; the log says the
/// same without the text).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transformed {
    /// The transform's name.
    pub name: String,
    /// Which it is, for the log: a built-in's kind, or "custom" (the name of
    /// a person's own transform is theirs, never logged).
    #[serde(skip)]
    kind: String,
    /// "selection" | "last" (the last dictation) | "" (neither).
    pub source: String,
    /// "pasted" (over the selection) | "replaced" (the last dictation, in
    /// place) | "copied" (the field left alone, the result on the
    /// clipboard) | "testMode" (a test run: nothing touched) | "unchanged" |
    /// "terminal" | "readOnly" | "nothing" | "refused" (the last dictation
    /// edited since, or the selection moved) | "failed" (the AI) | "busy".
    pub outcome: String,
    /// What the AI was given (test runs: the specs check it).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input: String,
    /// The result.
    pub text: String,
    pub why: Option<String>,
}

impl Transformed {
    fn ended(mut self, outcome: &str, why: Option<String>) -> Self {
        self.outcome = outcome.to_string();
        self.why = why;
        tracing::info!(
            transform = %self.kind,
            source = %self.source,
            outcome = %self.outcome,
            chars = self.text.chars().count(),
            "Transform"
        );
        self
    }
}

/// A test run's stand-ins for what a real one reads (debug builds).
#[derive(Debug, Clone, Default)]
pub struct StandIn {
    /// Selected in the stand-in window.
    pub selection: Option<String>,
    /// The window is a terminal.
    pub terminal: bool,
}

/// The transform in input-hook slot `slot` was pressed: run it, on its own
/// thread.
pub fn on_key(app: &AppHandle, slot: usize) {
    let Some(id) = crate::dictation::input_hook::transform_at(slot) else {
        tracing::warn!(slot, "A transform shortcut with no transform");
        return;
    };
    run_soon(app, &id, Origin::Shortcut);
}

/// Run transform `id` from `origin` on its own thread.
pub fn run_soon(app: &AppHandle, id: &str, origin: Origin) {
    let app = app.clone();
    let id = id.to_string();
    let _ = std::thread::Builder::new().name("transform".into()).spawn(move || {
        let _ = run_by_id(&app, &id, origin, None);
    });
}

/// Run transform `id` (blocking). `Err`: no such transform, or no pipeline.
pub fn run_by_id(app: &AppHandle, id: &str, origin: Origin, stand_in: Option<StandIn>) -> Result<Transformed, String> {
    let transformer = {
        let state = app.state::<crate::AppState>();
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.transformer()
    };
    let cfg = transformer.config();
    let t = find(&cfg, id).ok_or_else(|| format!("No transform {id:?}"))?;
    let held = transformer.hold(|cfg| run(app, cfg, &t, origin, stand_in));
    Ok(held.unwrap_or_else(|busy| {
        let _ = app.emit("yap-error", busy);
        Transformed { name: t.name(), kind: kind_of(&t), ..Default::default() }.ended("busy", Some(busy.to_string()))
    }))
}

/// What the log calls a transform: a built-in's kind, else "custom".
fn kind_of(t: &Transform) -> String {
    if t.builtin.is_empty() {
        "custom".to_string()
    } else {
        t.builtin.clone()
    }
}

/// The window the transform works in: the one in front, or (the tray) the
/// top app window brought back. A test run's stand-in.
fn target_window(origin: Origin, stand_in: bool) -> Option<isize> {
    #[cfg(debug_assertions)]
    if stand_in {
        return crate::dictation::pipeline::test_target();
    }
    let _ = stand_in;
    match origin {
        Origin::Shortcut => crate::pasting::text_injector::current_foreground(),
        Origin::Tray => {
            let t = crate::pasting::text_injector::app_window();
            if let Some(h) = t {
                crate::pasting::text_injector::focus(h);
                // The app puts its focus back in its field on activation.
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            t
        }
    }
}

/// The transform `t`, run (see the module comment): the caller holds the
/// pipeline's `processing` guard. Blocking.
pub fn run(app: &AppHandle, cfg: &YapConfig, t: &Transform, origin: Origin, stand_in: Option<StandIn>) -> Transformed {
    let name = t.name();
    let mut r = Transformed { name: name.clone(), kind: kind_of(t), ..Default::default() };
    // The shortcut's own modifiers let go first: Ctrl+C or Ctrl+V with Alt
    // still down is another key in most apps.
    if origin == Origin::Shortcut {
        crate::pasting::as_said::wait_for_modifiers_up();
    }
    let target = target_window(origin, stand_in.is_some());
    let kind = match &stand_in {
        Some(s) if s.terminal => AppKind::Terminal(crate::pasting::app_kind::Terminal { coding_cli: None, host: crate::pasting::app_kind::Host::WindowsTerminal }),
        _ => crate::pasting::app_kind::of_window(target),
    };
    let terminal = kind.is_terminal();

    // What's selected: a field that takes typing only. (Never Ctrl+C in a
    // terminal: with nothing selected it stops the program running there.)
    let found = match (&stand_in, target) {
        _ if terminal => FieldSelection::Nothing,
        (Some(s), _) => s.selection.clone().map_or(FieldSelection::Nothing, FieldSelection::Text),
        (None, None) => FieldSelection::Nothing,
        (None, Some(_)) => crate::pasting::selection::editable_selection(target, true),
    };
    let last = match found {
        FieldSelection::Text(_) | FieldSelection::ReadOnly => None,
        _ if terminal => None,
        _ => crate::pasting::as_said::rewrite_target(cfg.rewrite_last_dictation, target),
    };
    let input = match pick(terminal, &found, last.is_some()) {
        Pick::Terminal => {
            say(app, "Transforms don't work in a terminal", "Yap can't select and replace text on a terminal's line. Select the text in another app, then try again.");
            return r.ended("terminal", Some("a terminal".into()));
        }
        Pick::ReadOnly => {
            say(app, &format!("Can't {} that text", lower_first(&name)), "It can't be changed here (it's read-only, or a password box). Select text in a field you can type in.");
            return r.ended("readOnly", Some("read-only".into()));
        }
        Pick::Nothing => {
            let body = if cfg.rewrite_last_dictation {
                "Select some text first — or use it within a minute of a dictation, in the same field."
            } else {
                "Select some text first, then press it again."
            };
            say(app, "Nothing to transform", body);
            return r.ended("nothing", None);
        }
        Pick::Selection => {
            r.source = "selection".into();
            match &found {
                FieldSelection::Text(t) => t.clone(),
                _ => String::new(),
            }
        }
        Pick::LastDictation => {
            r.source = "last".into();
            let Some(p) = last.clone() else {
                return r.ended("nothing", None);
            };
            if let Err(why) = crate::pasting::as_said::rewrite_check(&p) {
                say(
                    app,
                    &format!("Couldn't {} your last dictation", lower_first(&name)),
                    &format!("Yap left it as it is ({why}). Select the text you want to change, then try again."),
                );
                return r.ended("refused", Some(why));
            }
            p.text().to_string()
        }
    };
    r.input = input.trim().to_string();
    tracing::debug!(chars = r.input.chars().count(), "Transform: the text");

    // The model: the Commands scope's, else the cleanup model.
    let m = crate::dictation::pipeline::commands_model(cfg);
    if m.base_url.is_empty() {
        let restarting = m.provider == crate::ai::local_llm::PROVIDER_ONDEVICE && crate::ai::local_llm::restart_under_way();
        failed(
            app,
            if restarting {
                "The on-device AI model is restarting. Try again in a moment.".to_string()
            } else {
                "Set up an AI model to use transforms (Language Models)".to_string()
            },
        );
        return r.ended("failed", Some("no AI model".into()));
    }
    if m.api_key.is_empty() && crate::ai::ai_model::KEYED_PROVIDERS.contains(&m.provider.as_str()) {
        failed(app, format!("Transforms: no {} API key — add one in Language Models", m.provider));
        return r.ended("failed", Some("no API key".into()));
    }

    let _ = app.emit(EVENT, serde_json::json!({ "name": name, "phase": "running" }));
    let _ = app.emit("yap-state", "processing");
    let asked = tauri::async_runtime::block_on(crate::ai::llm::transform(
        &t.instruction(),
        &r.input,
        &m.base_url,
        &m.api_key,
        &m.model,
        &m.provider,
        m.disable_thinking,
    ));
    let out = match asked {
        Ok(out) => clean_output(&out),
        Err(e) => {
            tracing::warn!(transform = %kind_of(t), "Transform failed: {e}");
            failed(app, format!("{name} failed via {} — check its key in Language Models", m.provider));
            return r.ended("failed", Some(e));
        }
    };
    if out.is_empty() {
        failed(app, format!("{name}: the AI gave back nothing"));
        return r.ended("failed", Some("empty answer".into()));
    }
    r.text = out.clone();
    tracing::debug!(text = %out, "Transform result");
    if crate::pasting::as_said::same_text(out.trim(), r.input.trim()) {
        let _ = app.emit(EVENT, serde_json::json!({ "name": name, "phase": "unchanged" }));
        let _ = app.emit("yap-state", "cleaned");
        back_to_idle_after(app, UNCHANGED_MS, None);
        return r.ended("unchanged", None);
    }
    let _ = app.emit("yap-transcript", out.clone());

    let restore = cfg.restore_clipboard;
    let undo = match (&found, last) {
        (FieldSelection::Text(sel), _) => {
            if let Err(why) = verify_selection(target, sel, stand_in.is_some()) {
                copied(app, &name, &why, &out, true);
                idle(app);
                return r.ended("copied", Some(why));
            }
            let pasted = crate::pasting::as_said::keep_spaces(sel, &out);
            // Back into its own window or nowhere (a test run's stand-in is
            // no window).
            let strict = stand_in.is_none();
            match tauri::async_runtime::block_on(crate::pasting::text_injector::inject_text(&pasted, restore, target, strict, kind)) {
                Ok(crate::pasting::text_injector::Delivery::Pasted | crate::pasting::text_injector::Delivery::Typed) => {
                    crate::pasting::as_said::remember(target, kind, &pasted, None, restore);
                    r = r.ended(if stand_in.is_some() { "testMode" } else { "pasted" }, None);
                    crate::pasting::as_said::undoable(sel, &pasted)
                }
                Ok(crate::pasting::text_injector::Delivery::LeftOnClipboard) => {
                    crate::pasting::as_said::forget();
                    crate::notices::alert::show_soon(app, crate::pasting::elevated::alert());
                    idle(app);
                    return r.ended("copied", Some("the app runs as administrator".into()));
                }
                Ok(crate::pasting::text_injector::Delivery::WindowGone) => {
                    crate::pasting::as_said::forget();
                    copied(app, &name, "you've moved on to another window", &out, false);
                    idle(app);
                    return r.ended("copied", Some("the window couldn't be reached".into()));
                }
                Err(e) => {
                    crate::pasting::as_said::forget();
                    failed(app, format!("{name}: Yap couldn't paste the result"));
                    return r.ended("failed", Some(e));
                }
            }
        }
        (_, Some(p)) => match crate::pasting::as_said::replace_last(app, &p, &out) {
            Ok(crate::pasting::as_said::Rewrote::Replaced { text, .. }) => {
                r = r.ended("replaced", None);
                crate::pasting::as_said::undoable(p.pasted_text(), &text)
            }
            Ok(crate::pasting::as_said::Rewrote::Copied { why, .. }) => {
                idle(app);
                return r.ended("copied", Some(why));
            }
            Ok(crate::pasting::as_said::Rewrote::TestMode { .. }) => {
                idle(app);
                return r.ended("testMode", None);
            }
            Err(why) => {
                idle(app);
                return r.ended("refused", Some(why));
            }
        },
        _ => None,
    };
    match undo {
        Some(seq) => {
            let _ = app.emit(EVENT, serde_json::json!({ "name": name, "phase": "done", "offerMs": crate::pasting::as_said::OFFER_MS }));
            let _ = app.emit("yap-state", "cleaned");
            back_to_idle_after(app, crate::pasting::as_said::OFFER_MS, Some(seq));
        }
        None => idle(app),
    }
    r
}

/// Before the result goes over the selection: the window is still in front
/// and the same text still selected there — read again over UI Automation
/// (a field it can't read was read by Ctrl+C and is taken as it was). A
/// test run's stand-in: nothing to read.
fn verify_selection(target: Option<isize>, selected: &str, stand_in: bool) -> Result<(), String> {
    if stand_in {
        return Ok(());
    }
    let Some(hwnd) = target else { return Err("the window is gone".into()) };
    if crate::pasting::text_injector::current_foreground() != Some(hwnd) {
        return Err("you've moved on to another window".into());
    }
    match crate::pasting::selection::editable_selection(Some(hwnd), false) {
        FieldSelection::Text(now) if crate::pasting::as_said::same_text(now.trim(), selected.trim()) => Ok(()),
        FieldSelection::Nothing => Err("the text isn't selected any more".into()),
        FieldSelection::Text(_) => Err("a different text is selected now".into()),
        FieldSelection::ReadOnly => Err("the selection is in text that can't be changed".into()),
        FieldSelection::Unreadable => Ok(()),
    }
}

/// "Polish" → "polish"; a name in capitals ("AI-ify") stays as it is.
fn lower_first(name: &str) -> String {
    let mut c = name.chars();
    match (c.next(), c.clone().next()) {
        (Some(f), Some(s)) if f.is_uppercase() && !s.is_uppercase() => f.to_lowercase().chain(c).collect(),
        _ => name.to_string(),
    }
}

/// Said plainly, nothing changed (a toast, a bar card or a Windows
/// notification: `alert.rs`).
fn say(app: &AppHandle, title: &str, body: &str) {
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: title.to_string(),
            body: body.to_string(),
            action: None,
            timeout_ms: 10_000,
        },
    );
}

/// The result couldn't go in place: on the clipboard (privately; `copy`:
/// not there yet), said.
fn copied(app: &AppHandle, name: &str, why: &str, text: &str, copy: bool) {
    let on = !copy || crate::pasting::text_injector::copy_privately(text.trim());
    let (title, body) = if on {
        (
            format!("Couldn't {} it in place — copied the result instead", lower_first(name)),
            format!("Yap left your text as it was ({why}). The result is on the clipboard: press Ctrl+V."),
        )
    } else {
        (
            format!("Couldn't {} it in place", lower_first(name)),
            format!("Yap left your text as it was ({why}), and the clipboard couldn't take the result."),
        )
    };
    say(app, &title, &body);
}

/// The AI or the paste failed: on the bar, as a rewrite's failure is.
fn failed(app: &AppHandle, message: String) {
    let _ = app.emit("yap-dictation-error", message);
    let _ = app.emit("yap-state", "error");
}

fn idle(app: &AppHandle) {
    let _ = app.emit("yap-state", "idle");
}

/// Bumped by every transform that reaches the bar: an older one's timer
/// leaves a newer one's state alone.
static BAR_TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The bar back to idle after `ms` — unless something else came since: a
/// newer transform, another paste (a dictation's own offer), the Undo
/// offered (`seq`) gone, or the pipeline busy again.
fn back_to_idle_after(app: &AppHandle, ms: u64, seq: Option<u64>) {
    use std::sync::atomic::Ordering;
    let app = app.clone();
    let turn = BAR_TURN.fetch_add(1, Ordering::SeqCst) + 1;
    let paste = crate::pasting::as_said::last_seq();
    let _ = std::thread::Builder::new().name("transform-offer".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        let moved_on = BAR_TURN.load(Ordering::SeqCst) != turn
            || crate::pasting::as_said::last_seq() != paste
            || seq.is_some_and(|s| !crate::pasting::as_said::undo_offered(s));
        if moved_on {
            return;
        }
        let busy = app
            .try_state::<crate::AppState>()
            .and_then(|s| s.pipeline.lock().ok().map(|g| g.as_ref().is_some_and(|p| p.is_busy())))
            .unwrap_or(false);
        if !busy {
            idle(&app);
        }
    });
}

// ---- commands -----------------------------------------------------------------------

/// Live-apply one transform's shortcut (Settings pauses it with "" while a
/// new key is picked, then puts the picked one); a save applies them all.
#[tauri::command]
pub fn configure_transform_hotkey(id: String, spec: String) {
    crate::dictation::input_hook::configure_transform(&id, &spec);
}

/// Debug-only, test mode: transform `id` as its shortcut would run it, in
/// the window a dictation goes to (`e2e_target_window`'s stand-in): with
/// `selection` standing in for text selected there (a test run reads none),
/// else the last dictation when the rules take it; `terminal`: the window is
/// a terminal. The AI is asked; nothing is read or pasted. Returns what it
/// did.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_transform(
    app: AppHandle,
    id: String,
    selection: Option<String>,
    terminal: Option<bool>,
) -> Result<Transformed, String> {
    if !crate::shell::e2e::active() || crate::shell::realtest::on() {
        return Err("e2e_transform is for test runs (not robot mode)".into());
    }
    if crate::dictation::pipeline::test_target().is_none() {
        return Err("e2e_transform needs e2e_target_window on".into());
    }
    let stand_in = StandIn { selection, terminal: terminal.unwrap_or(false) };
    tauri::async_runtime::spawn_blocking(move || run_by_id(&app, &id, Origin::Shortcut, Some(stand_in)))
        .await
        .map_err(|e| format!("the transform task failed: {e}"))?
}

/// Debug-only: the input hook's transform slots as set now (`{ id, key }`,
/// the key in words) — what a press of each would run.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_transform_slots() -> Vec<serde_json::Value> {
    crate::dictation::input_hook::transform_slots()
        .into_iter()
        .map(|(id, key)| serde_json::json!({ "id": id, "key": key }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(id: &str, name: &str, instruction: &str, hotkey: &str) -> Transform {
        Transform { id: id.into(), name: name.into(), instruction: instruction.into(), hotkey: hotkey.into(), ..Default::default() }
    }

    fn with_key(mut t: Transform, hotkey: &str) -> Transform {
        t.hotkey = hotkey.into();
        t
    }

    #[test]
    fn a_transform_on_every_dictation_is_off_by_default_and_never_in_a_terminal() {
        use crate::ai::cleanup_styles::Category;
        let cfg: YapConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.auto_transform, "");
        assert!(for_dictation(&cfg, Category::Email, false).is_none(), "off by default");
        let mut cfg = YapConfig { auto_transform: POLISH.into(), ..Default::default() };
        assert_eq!(for_dictation(&cfg, Category::Email, false).map(|t| t.name()).as_deref(), Some("Polish"));
        assert_eq!(for_dictation(&cfg, Category::Other, false).map(|t| t.name()).as_deref(), Some("Polish"));
        // Never a terminal's command, by its kind or by what was read.
        assert!(for_dictation(&cfg, Category::Terminal, false).is_none());
        assert!(for_dictation(&cfg, Category::Other, true).is_none());
        // One kind of app only.
        cfg.auto_transform_kind = "email".into();
        assert!(for_dictation(&cfg, Category::Email, false).is_some());
        assert!(for_dictation(&cfg, Category::WorkChat, false).is_none());
        // One that can't run (deleted): none.
        cfg.auto_transform = "t-gone".into();
        assert!(for_dictation(&cfg, Category::Email, false).is_none());
    }

    #[test]
    fn a_new_config_has_yaps_five_with_no_shortcuts() {
        let cfg: YapConfig = serde_json::from_str("{}").unwrap();
        let names: Vec<String> = list(&cfg).iter().map(Transform::name).collect();
        assert_eq!(names, ["Polish", "Shorter", "Fix grammar only", "Translate to Spanish", "Make it a prompt"]);
        assert!(cfg.transforms.iter().all(|t| t.hotkey.is_empty()));
        assert_eq!(YapConfig::default().transforms, defaults());
    }

    #[test]
    fn a_saved_list_round_trips_and_keeps_its_order() {
        let mut french = Transform::builtin(TRANSLATE);
        french.language = "French".into();
        let cfg = YapConfig {
            transforms: vec![custom("t-1", "Friendlier", "Make it warmer and friendlier.", "kb:ctrl+alt+70"), french, Transform::builtin(POLISH)],
            ..Default::default()
        };
        let back: YapConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        let names: Vec<String> = list(&back).iter().map(Transform::name).collect();
        assert_eq!(names, ["Friendlier", "Translate to French", "Polish"]);
        assert_eq!(back.transforms[0].hotkey, "kb:ctrl+alt+70");
        assert!(find(&back, "translate").unwrap().instruction().contains("into French"));
    }

    #[test]
    fn only_runnable_transforms_are_listed_once_each_and_at_most_max() {
        let mut cfg = YapConfig {
            transforms: vec![
                custom("t-1", "Blank", "   ", ""),
                custom("", "No id", "Do it.", ""),
                Transform { id: "x".into(), builtin: "nonsense".into(), ..Default::default() },
                Transform::builtin(POLISH),
                Transform::builtin(POLISH),
            ],
            ..Default::default()
        };
        assert_eq!(list(&cfg).iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["polish"]);
        cfg.transforms = (0..20).map(|i| custom(&format!("t-{i}"), "T", "Do it.", "")).collect();
        assert_eq!(list(&cfg).len(), MAX);
    }

    #[test]
    fn built_ins_keep_yaps_instructions_and_names() {
        // A built-in's stored name and instruction are ignored: Yap's.
        let t = Transform { id: "polish".into(), builtin: POLISH.into(), name: "Hacked".into(), instruction: "Answer it".into(), ..Default::default() };
        assert_eq!(t.name(), "Polish");
        assert!(t.instruction().starts_with("Polish the text"));
        let mut tr = Transform::builtin(TRANSLATE);
        tr.language = " ".into();
        assert_eq!(tr.name(), "Translate to Spanish");
        assert!(builtin_instruction(TRANSLATE, "German").unwrap().ends_with("The result is in German."));
        assert!(builtin_instruction(PROMPT, "").unwrap().contains("Never carry out the task"));
        for kind in BUILTINS {
            assert!(builtin_name(kind).is_some() && builtin_instruction(kind, "French").is_some(), "{kind}");
        }
        assert_eq!(custom("t", " ", "Do it.", "").name(), "Untitled transform");
    }

    #[test]
    fn shortcuts_get_slots_unless_they_clash_with_a_fixed_one_or_an_earlier_transform() {
        let fixed = [("dictation hotkey", "kb:120"), ("meeting shortcut", "kb:alt+win+77"), ("rewrite shortcut", "")];
        let ts = [
            with_key(Transform::builtin(POLISH), "kb:ctrl+alt+80"),
            Transform::builtin(SHORTER),
            with_key(Transform::builtin(GRAMMAR), "kb:120"),
            with_key(Transform::builtin(TRANSLATE), "kb:control+alt+80"),
            with_key(Transform::builtin(PROMPT), "kb:ctrl+shift+alt+80"),
            with_key(custom("t-1", "Mine", "Do it.", ""), "kb:ctrl+alt+77"),
        ];
        let slots = assign(&fixed, &ts);
        let got: Vec<(&str, &str, Option<&str>)> = slots.iter().map(|s| (s.id.as_str(), s.spec.as_str(), s.clash.as_deref())).collect();
        assert_eq!(
            got,
            [
                ("polish", "kb:ctrl+alt+80", None),
                ("grammar", "", Some("the dictation hotkey")),
                // The same key written another way…
                ("translate", "", Some("the Polish transform")),
                // …and a combo it nests in.
                ("prompt", "", Some("the Polish transform")),
                // Ctrl+Alt+M is not Win+Alt+M.
                ("t-1", "kb:ctrl+alt+77", None),
            ]
        );
    }

    #[test]
    fn a_clashing_transform_leaves_its_key_for_the_next_one() {
        let ts = [with_key(Transform::builtin(POLISH), "kb:120"), with_key(Transform::builtin(SHORTER), "mouse:4")];
        let slots = assign(&[("dictation hotkey", "kb:120")], &ts);
        assert_eq!(slots[0].spec, "");
        assert_eq!(slots[1].spec, "mouse:4", "a mouse button is a shortcut too");
    }

    #[test]
    fn what_a_transform_works_on() {
        let text = FieldSelection::Text("send it by friday ".into());
        // A terminal never, whatever is selected or was dictated.
        assert_eq!(pick(true, &text, true), Pick::Terminal);
        assert_eq!(pick(true, &FieldSelection::Nothing, true), Pick::Terminal);
        // A selection wins over the last dictation.
        assert_eq!(pick(false, &text, true), Pick::Selection);
        assert_eq!(pick(false, &text, false), Pick::Selection);
        // Read-only text is refused, not swapped for the dictation.
        assert_eq!(pick(false, &FieldSelection::ReadOnly, true), Pick::ReadOnly);
        // Nothing selected: the last dictation when the rules took it.
        assert_eq!(pick(false, &FieldSelection::Nothing, true), Pick::LastDictation);
        assert_eq!(pick(false, &FieldSelection::Unreadable, true), Pick::LastDictation);
        assert_eq!(pick(false, &FieldSelection::Text("  \n".into()), true), Pick::LastDictation);
        assert_eq!(pick(false, &FieldSelection::Nothing, false), Pick::Nothing);
        assert_eq!(pick(false, &FieldSelection::Unreadable, false), Pick::Nothing);
    }

    #[test]
    fn a_models_preamble_and_echoed_tags_come_off() {
        assert_eq!(clean_output("Here is the polished text:\nSend it by Friday."), "Send it by Friday.");
        assert_eq!(clean_output("<text>\nEnvíalo el viernes.\n</text>"), "Envíalo el viernes.");
        assert_eq!(clean_output("Sure! Here's the translation:\n\nHola."), "Hola.");
        // A first line that's the text itself stays.
        assert_eq!(clean_output("Agenda:\n- budget\n- hiring"), "Agenda:\n- budget\n- hiring");
        assert_eq!(clean_output("Here is the plan: ship on Friday."), "Here is the plan: ship on Friday.");
        assert_eq!(clean_output("  Done.  "), "Done.");
    }

    #[test]
    fn names_read_naturally_in_messages() {
        assert_eq!(lower_first("Polish"), "polish");
        assert_eq!(lower_first("Translate to French"), "translate to French");
        assert_eq!(lower_first("AI-ify"), "AI-ify");
        assert_eq!(lower_first("x"), "x");
    }
}
