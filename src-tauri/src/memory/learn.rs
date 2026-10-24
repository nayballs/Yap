//! The passes that change memory, all through the person's **Chat model**
//! (`commands::resolve_chat_endpoint`):
//! - **learning from a chat** that went quiet ([`learn_from_chat`]; the
//!   scheduler: 5 minutes without a new message, another chat starting, or a
//!   minute after the next start when Yap quit first);
//! - **learning from a meeting** whose action plan is written
//!   ([`learn_from_meeting`], "Learn from meetings");
//! - **"Tell Yap what to change"** and **imports** ([`propose`]): changes
//!   shown as a diff, applied when the person says so ([`apply_proposal`]);
//! - **"remember" / "forget"** from a chat or the Voice Assistant ([`tool`]
//!   for a model with tools, [`spoken`] for one without).
//!
//! A model on this PC follows the sidecar's rules: its work counts as busy
//! (`local_llm::hold`), it never runs during a dictation, and a dictation
//! starting drops the call (asked again after it). A pass that finds the
//! on-device model stopped waits for the next one rather than starting it
//! in the background.
//!
//! **Two paths** ([`path`], [`chat_path`]): the full path — the model reads
//! the chat and writes memory's changes itself — for a model that can call
//! tools (a cloud model; on this PC, Qwen3.5-4B for meetings, Tell and
//! imports); **the small path** (`small.rs`) — Yap picks out what the
//! person said about themselves, the model fills one small form a sentence
//! under llama.cpp's JSON grammar, and Yap checks every answer — for a
//! model that can't (Qwen2.5-1.5B, Qwen3.5-2B / 0.8B), and for learning
//! from chats with any local model (it measured better on the 4B too). On
//! this PC's processor it reads at most 3 sentences a pass and 12 passes a
//! day.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::ops::{self, Action, Change, Done, Item, Origin, Rules, Section};
use super::{prompt, small, Applied};

/// The Chat model's endpoint, as a pass uses it.
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub disable_thinking: bool,
}

impl Endpoint {
    fn local(&self) -> bool {
        crate::ai_model::runs_here(&self.provider, &self.base_url)
    }
}

/// The Chat model, or why there's none ("No AI model configured — …").
pub fn endpoint(cfg: &crate::config::YapConfig) -> Result<Endpoint, String> {
    let (base_url, api_key, model, provider, _persona, disable_thinking) = crate::commands::resolve_chat_endpoint(cfg)?;
    Ok(Endpoint { base_url, api_key, model, provider, disable_thinking })
}

/// A background pass that waited out dictations and gave up for now (the
/// scheduler asks again later).
pub const DEFERRED: &str = "Memory: put off for now";

/// A dictation recording or transcribing (or an Upload): a model on this PC
/// must be free then.
fn dictation_busy(app: &AppHandle) -> bool {
    crate::meetings::meeting_summary::dictation_busy(app)
}

/// One call to the Chat model for a pass: low temperature, a reply cap and
/// the changes' JSON schema for a model on this PC, and the sidecar's rules
/// (`background`: give up after one dictation got in the way).
async fn ask(ep: &Endpoint, system: &str, user: &str, max_changes: usize, background: bool) -> Result<String, String> {
    let local = ep.local();
    let opts = crate::llm::ChatOptions {
        temperature: 0.2,
        max_tokens: local.then_some(if max_changes > 8 { 2_000 } else { 800 }),
        // A 4B model on a processor writes ~10 tokens a second.
        timeout: Duration::from_secs(if local { 240 } else { 60 }),
        disable_thinking: ep.disable_thinking,
    };
    let messages = json!([{ "role": "system", "content": system }, { "role": "user", "content": user }]);
    // A small model on this PC (one that can't call tools) answers under the
    // changes' JSON schema as llama.cpp's grammar, every field required in a
    // fixed order: left to itself Qwen2.5-1.5B wrote JSON Yap couldn't read.
    // A bigger one writes better without it: under the strict schema
    // Qwen3.5-4B passed 36 of 42 memory scenarios, without it 41; with the
    // fields optional it stopped every change after its "section"; and
    // llamafile 0.10.6 ignores a bare `json_object` (the model evaluations,
    // 2026-10-22).
    let mut schema = serde_json::Map::new();
    if local {
        let mode = match json_mode() {
            "auto" if crate::chat::tools::supports_tools(&ep.provider, &ep.model) => "off",
            "auto" => "schema",
            m => m,
        };
        match mode {
            "schema" => {
                schema.insert(
                    "response_format".into(),
                    json!({ "type": "json_schema", "json_schema": { "name": "memory_changes", "schema": prompt::schema(max_changes) } }),
                );
            }
            "object" => {
                schema.insert("response_format".into(), json!({ "type": "json_object" }));
            }
            _ => {}
        }
    }
    if !local {
        return call(ep, &messages, &opts, serde_json::Map::new()).await;
    }
    // The on-device model is busy for the whole pass, waits included.
    let _sidecar = crate::local_llm::hold(&ep.base_url);
    beside_dictation(background, || call(ep, &messages, &opts, schema.clone())).await
}

/// One call to the Chat model with `extra` laid over the body (a
/// `response_format`); a server that won't take the schema is asked once
/// more without it.
async fn call(ep: &Endpoint, messages: &Value, opts: &crate::llm::ChatOptions, extra: serde_json::Map<String, Value>) -> Result<String, String> {
    let ask = crate::llm::chat_with(&ep.base_url, &ep.api_key, &ep.model, &ep.provider, messages.clone(), opts, extra.clone());
    let out = crate::privacy::net::within(crate::privacy::net::Feature::Memory, ask).await;
    match out {
        Err(e) if !extra.is_empty() && (e.contains("response_format") || e.contains("400") || e.contains("schema")) => {
            tracing::info!("Memory: the model's server refused the JSON schema; asking without it");
            let ask = crate::llm::chat_with(&ep.base_url, &ep.api_key, &ep.model, &ep.provider, messages.clone(), opts, serde_json::Map::new());
            crate::privacy::net::within(crate::privacy::net::Feature::Memory, ask).await
        }
        other => other,
    }
}

/// A call to a model on this PC, by the sidecar's rules: it waits out a
/// dictation, and a dictation starting drops it (asked again after it; a
/// `background` pass gives up after the second time: [`DEFERRED`]). No
/// app (a model evaluation): just the call.
async fn beside_dictation<F, Fut>(background: bool, make: F) -> Result<String, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let Some(app) = super::app() else {
        return make().await;
    };
    let tries = if background { 2 } else { 5 };
    for _ in 0..tries {
        crate::meetings::meeting_summary::wait_idle(|| dictation_busy(app), || true).await;
        match crate::meetings::meeting_summary::unless_busy(make(), || dictation_busy(app)).await {
            Some(out) => return out,
            None => tracing::info!("Memory pass set aside for a dictation"),
        }
    }
    Err(DEFERRED.to_string())
}

/// **The small path's calls**: one short extraction a candidate (`small.rs`:
/// ~300 tokens, the reply ≤ 80, temperature 0, under its JSON schema as
/// llama.cpp's grammar). A model on this PC is held busy for all of them;
/// a dictation starting drops the pass ([`DEFERRED`] for a background one:
/// nothing is applied, and the chat is read again later).
async fn extract(ep: &Endpoint, candidates: &[small::Candidate], background: bool) -> Result<Vec<small::Extracted>, String> {
    let local = ep.local();
    let opts = crate::llm::ChatOptions {
        temperature: 0.0,
        max_tokens: Some(80),
        timeout: Duration::from_secs(if local { 60 } else { 30 }),
        disable_thinking: ep.disable_thinking,
    };
    let mut schema = serde_json::Map::new();
    schema.insert("response_format".into(), json!({ "type": "json_schema", "json_schema": { "name": "memory_item", "schema": small::schema() } }));
    let _sidecar = (local && candidates.iter().any(small::needs_model)).then(|| crate::local_llm::hold(&ep.base_url));
    let mut out = Vec::new();
    for c in candidates {
        // A preference is the person's own words: no call.
        if !small::needs_model(c) {
            out.push(small::Extracted::default());
            continue;
        }
        let messages = small::messages(c);
        let reply = if local {
            beside_dictation(background, || call(ep, &messages, &opts, schema.clone())).await?
        } else {
            call(ep, &messages, &opts, schema.clone()).await?
        };
        out.push(small::parse(&reply));
    }
    Ok(out)
}

/// The changes the small path makes: what the person left taken out of
/// Profile (`small::left_changes`); for each candidate, a preference in
/// their own words (`small::preference_of`), or the model's answer checked
/// (`small::validate`), and the people the sentence names with who they
/// are (`small::people_changes`, found by Yap). Also how many answers were
/// left out, for the log. Pure.
fn small_changes(candidates: &[small::Candidate], answers: &[small::Extracted], left: &[String], items: &[Item]) -> (Vec<Change>, usize) {
    let mut changes = small::left_changes(left, items);
    let mut dropped = 0;
    for (c, x) in candidates.iter().zip(answers) {
        if let Some(p) = small::preference_of(c) {
            changes.push(p);
            continue;
        }
        // What Yap finds in the person's words by itself: a project named
        // with ownership, a birthday, the people named with who they are.
        let projects = small::project_changes(c);
        let birthdays = small::birthday_changes(c, items);
        let people = small::people_changes(c);
        // A Profile line naming someone Yap found ("Dog's name is Beans" next
        // to Beans, Dog) says nothing more.
        let names_found = |f: &String| people.iter().any(|p| ops::norm(f).split(' ').any(|w| ops::norm(&p.name).split(' ').any(|n| n == w)));
        if small::needs_model(c) {
            match small::validate(c, x, items) {
                // The project Yap found is the memory, not the model's
                // Profile line about building it; a birthday Yap read is
                // the one kept.
                Ok(change)
                    if !projects.is_empty() && change.section != Some(Section::People)
                        || !birthdays.is_empty() && change.facts.iter().any(|f| f.to_lowercase().contains("birthday"))
                        || change.section.is_some_and(|s| s.is_you()) && change.facts.iter().any(names_found) => {}
                Ok(change) => changes.push(change),
                Err(why) => {
                    dropped += 1;
                    tracing::debug!(kind = ?c.kind, "Memory (small model): left out: {why}");
                }
            }
        }
        let found = projects.into_iter().chain(birthdays).chain(people);
        for p in found {
            // Once: a person or project already in the batch, or the same
            // fact for them.
            let have = changes.iter().any(|x| {
                x.section == p.section
                    && x.section.is_some_and(|s| ops::same_thing(s, &x.name, &p.name))
                    && (p.facts.is_empty() || x.facts == p.facts)
            });
            if !have {
                changes.push(p);
            }
        }
    }
    (changes, dropped)
}

/// How a model on this PC is held to JSON: "auto" (the changes' schema as
/// llama.cpp's grammar for a model without tools, nothing for one with
/// them), or — a model evaluation comparing, `YAP_EVAL_MEMORY_JSON` —
/// "schema", "object" (`json_object`) or "off" for every model.
fn json_mode() -> &'static str {
    if crate::eval::active() {
        match std::env::var("YAP_EVAL_MEMORY_JSON").as_deref() {
            Ok("schema") => return "schema",
            Ok("object") => return "object",
            Ok("off") => return "off",
            _ => {}
        }
    }
    "auto"
}

/// No changes (nothing to learn, or nothing was asked).
fn nothing() -> Applied {
    Applied { batch: 0, done: Vec::new() }
}

/// How a Chat model changes memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// It reads the chat and writes memory's changes itself (a model that
    /// can call tools: a cloud model, Qwen3.5-4B…).
    Full,
    /// **The small path** (`small.rs`): Yap picks the sentences, the model
    /// fills one small form each, Yap checks it. Given the whole chat,
    /// Qwen2.5-1.5B kept what the person only asked about ("Australia:
    /// capital is Canberra"), filed "Nurse at St James's" as a person and a
    /// wifi password as a preference, and took up to two minutes of a
    /// processor a pass (2026-10-22).
    Small,
}

/// The path this Chat model takes: [`Path::Full`] when it can call tools,
/// else [`Path::Small`]. A model evaluation may choose
/// (`YAP_EVAL_MEMORY_PATH=full|small`).
pub fn path(ep: &Endpoint) -> Path {
    if crate::eval::active() {
        match std::env::var("YAP_EVAL_MEMORY_PATH").as_deref() {
            Ok("full") => return Path::Full,
            Ok("small") => return Path::Small,
            _ => {}
        }
    }
    if crate::chat::tools::supports_tools(&ep.provider, &ep.model) {
        Path::Full
    } else {
        Path::Small
    }
}

/// The path **learning from a chat** takes: the small path for a local
/// model (Yap's on-device model, or a self-hosted one — Ollama, LM Studio),
/// whatever its size — on Qwen3.5-4B (GPU, ×2) it kept no false memory
/// where the full path kept 15 (people named in passing, "Coming to stay
/// this weekend"…), and it missed less (42 of 44 checks against 38),
/// 2026-10-22. A cloud model or a custom endpoint keeps [`path`] (not
/// measured), and so do meetings, Tell and imports. A model evaluation may
/// choose (`YAP_EVAL_MEMORY_PATH`).
pub fn chat_path(ep: &Endpoint) -> Path {
    let forced = crate::eval::active() && matches!(std::env::var("YAP_EVAL_MEMORY_PATH").as_deref(), Ok("full" | "small"));
    if !forced && matches!(ep.provider.as_str(), crate::local_llm::PROVIDER_ONDEVICE | "local") {
        return Path::Small;
    }
    path(ep)
}

/// Does the model run on this PC's processor (not its graphics card, not
/// in the cloud)? Then the small path reads less a pass and passes a day
/// are capped. Another server on this PC (Ollama, LM Studio) counts as
/// the processor: Yap can't tell.
fn on_processor(ep: &Endpoint) -> bool {
    if !ep.local() {
        return false;
    }
    if let Some(gpu) = crate::eval::local_on_gpu() {
        return !gpu;
    }
    !(ep.provider == crate::local_llm::PROVIDER_ONDEVICE && crate::local_llm::placement().is_some_and(|p| p.kind() == "gpu"))
}

/// The small path's sentences a pass: on the processor 3 (each ~1–3 s of it
/// for a 1.5B model), else 6.
fn small_cap(on_processor: bool) -> usize {
    if on_processor {
        3
    } else {
        6
    }
}

/// Small-path passes with a model call a day on the processor.
const PROCESSOR_PASSES_A_DAY: u32 = 12;
/// A pass that found today's passes used up.
pub const USED_UP: &str = "Memory: today's passes on the processor are used up";

/// (day number, passes) on the processor today.
static PROCESSOR_TODAY: Mutex<(u64, u32)> = Mutex::new((0, 0));

/// Take one of today's processor passes (`false`: none left). Not counted
/// in a model evaluation.
fn take_processor_pass() -> bool {
    if crate::eval::active() {
        return true;
    }
    let day = super::now_secs() / 86_400;
    let mut g = PROCESSOR_TODAY.lock().unwrap_or_else(|p| p.into_inner());
    if g.0 != day {
        *g = (day, 0);
    }
    if g.1 >= PROCESSOR_PASSES_A_DAY {
        return false;
    }
    g.1 += 1;
    true
}

/// Is the on-device model the Chat model, and stopped? A background pass
/// doesn't start it.
fn on_device_stopped(ep: &Endpoint) -> bool {
    ep.provider == crate::local_llm::PROVIDER_ONDEVICE && !crate::local_llm::is_running()
}

/// **Read a conversation's new messages for lasting facts** and change
/// memory to match ("Learn from my chats"). Only messages since the last
/// pass; nothing at all (no model call) when the person said nothing about
/// themselves (`prompt::worth_a_pass`). `announce`: the main window's toast.
pub async fn learn_from_chat(conversation: u64, announce: bool) -> Result<Applied, String> {
    let cfg = crate::config::load();
    let conv = crate::chat::chats::get(conversation).ok_or("Conversation not found")?;
    let len = conv.messages.len();
    let start = super::read_upto(conversation).min(len);
    let new = &conv.messages[start..];
    let user_texts: Vec<&str> = new.iter().filter(|m| m.role == "user").map(|m| m.text.as_str()).collect();
    let ep = endpoint(&cfg);
    let small = ep.as_ref().is_ok_and(|ep| chat_path(ep) == Path::Small);
    if user_texts.is_empty() || !small && !prompt::worth_a_pass(&user_texts) {
        super::mark_read(conversation, len);
        return Ok(nothing());
    }
    let ep = ep?;
    if small {
        return learn_small(conversation, &conv.title, &user_texts, len, announce, &cfg, &ep).await;
    }
    if on_device_stopped(&ep) && !crate::eval::active() {
        return Err(DEFERRED.to_string());
    }
    let pair = |m: &crate::chat::chats::ChatMessage| (m.role.clone(), m.text.clone());
    let before: Vec<(String, String)> = conv.messages[start.saturating_sub(2)..start].iter().map(pair).collect();
    let fresh: Vec<(String, String)> = new.iter().map(pair).collect();
    let budget = if ep.local() { 1_800 } else { 2_400 };
    let user = prompt::pass_input(
        &prompt::listing(&super::items(), 900),
        "The chat (only what the user says about themselves counts)",
        &prompt::chat_excerpt(&before, &fresh, budget),
    );
    let reply = ask(&ep, &prompt::learn_chat_system(cfg.memory_sensitive), &user, 5, !crate::eval::active()).await?;
    let changes = ops::route_preferences(ops::parse_changes(&reply), &super::items());
    let about_you = crate::about_you::profile(&cfg);
    let changes = ops::drop_about_you(changes, &super::items(), &about_you.name, &about_you.about);
    // Grounded in what the person said (theirs, not the AI's answers).
    let said: Vec<&str> = conv.messages[start.saturating_sub(2)..].iter().filter(|m| m.role == "user").map(|m| m.text.as_str()).collect();
    let applied = super::change(
        &changes,
        &Rules {
            sensitive_ok: cfg.memory_sensitive,
            origin: Origin::new("chat", Some(conversation), &conv.title),
            now: super::now_secs(),
            max_changes: 5,
            grounding: Some(said.join("\n")),
        },
    );
    super::mark_read(conversation, len);
    if announce {
        super::announce(&applied);
    }
    Ok(applied)
}

/// **The small path over a chat's new messages** (`small.rs`): the
/// sentences the person wrote about themselves (none: no model call), the
/// newest 3 on the processor (6 on a graphics card or in the cloud), one
/// short call each, every answer checked, then memory's own rules. Logs
/// counts and time only.
async fn learn_small(
    conversation: u64,
    title: &str,
    user_texts: &[&str],
    len: usize,
    announce: bool,
    cfg: &crate::config::YapConfig,
    ep: &Endpoint,
) -> Result<Applied, String> {
    // "Remember that…" as a whole message was done when it was said (`spoken`).
    let texts: Vec<&str> = user_texts.iter().copied().filter(|t| spoken_request(t).is_none()).collect();
    let known: Vec<String> = super::items().into_iter().filter(|i| i.section == Section::People).map(|i| i.title).collect();
    let found = small::candidates(&texts, cfg.memory_sensitive, &known);
    if found.is_empty() {
        super::mark_read(conversation, len);
        return Ok(nothing());
    }
    let processor = on_processor(ep);
    let mut candidates = found.candidates.clone();
    let cap = small_cap(processor);
    if candidates.len() > cap {
        candidates.drain(..candidates.len() - cap);
    }
    if !candidates.is_empty() {
        if on_device_stopped(ep) && !crate::eval::active() {
            return Err(DEFERRED.to_string());
        }
        if processor && !take_processor_pass() {
            return Err(USED_UP.to_string());
        }
    }
    let t0 = Instant::now();
    let answers = extract(ep, &candidates, !crate::eval::active()).await?;
    let items = super::items();
    let (changes, dropped) = small_changes(&candidates, &answers, &found.left, &items);
    let changes = ops::route_preferences(changes, &items);
    let about_you = crate::about_you::profile(cfg);
    let changes = ops::drop_about_you(changes, &items, &about_you.name, &about_you.about);
    let mut grounding: Vec<String> = candidates.iter().map(|c| c.text.clone()).collect();
    grounding.extend(found.left.iter().cloned());
    let applied = super::change(
        &changes,
        &Rules {
            sensitive_ok: cfg.memory_sensitive,
            origin: Origin::new("chat", Some(conversation), title),
            now: super::now_secs(),
            max_changes: 6,
            grounding: Some(grounding.join("\n")),
        },
    );
    tracing::info!(
        conversation,
        sentences = found.candidates.len(),
        asked = candidates.len(),
        left_out = dropped,
        changes = applied.done.len(),
        ms = t0.elapsed().as_millis() as u64,
        processor,
        "Memory (small model): read a chat"
    );
    super::mark_read(conversation, len);
    if announce {
        super::announce(&applied);
    }
    Ok(applied)
}

/// **Read a finished meeting** — its title, attendees and action plan — for
/// who the attendees are to the person and the projects they work on
/// ("Learn from meetings", off by default: other people's words).
pub async fn learn_from_meeting(note_id: u64) -> Result<Applied, String> {
    let cfg = crate::config::load();
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if note.enhanced_content.trim().is_empty() && note.participants.is_empty() {
        return Ok(nothing());
    }
    let ep = endpoint(&cfg)?;
    // A meeting is other people's words in the third person: the small
    // path, which reads only what the person said about themselves, has
    // nothing to go on.
    if path(&ep) == Path::Small {
        super::meeting_read(note_id, true);
        return Ok(nothing());
    }
    if on_device_stopped(&ep) && !crate::eval::active() {
        return Err(DEFERRED.to_string());
    }
    let (plan, _) = crate::chat::note_text::fit(&note.enhanced_content, 1_500);
    let mut body = format!("Title: {}\n", note.title);
    if !note.participants.is_empty() {
        body.push_str(&format!("Attendees: {}\n", note.participants.join(", ")));
    }
    body.push_str(&format!("\nYap's notes after the meeting:\n{}", plan.trim()));
    let user = prompt::pass_input(&prompt::listing(&super::items(), 900), "The meeting", &body);
    let reply = ask(&ep, &prompt::learn_meeting_system(cfg.memory_sensitive), &user, 5, !crate::eval::active()).await?;
    let changes: Vec<Change> = ops::parse_changes(&reply)
        .into_iter()
        // People and projects only (and updates of them).
        .filter(|c| !matches!(c.section, Some(Section::Profile | Section::Preferences | Section::Topics)))
        .collect();
    let applied = super::change(
        &changes,
        &Rules {
            sensitive_ok: cfg.memory_sensitive,
            origin: Origin::new("meeting", Some(note_id), &note.title),
            now: super::now_secs(),
            max_changes: 5,
            grounding: Some(body.clone()),
        },
    );
    super::meeting_read(note_id, true);
    super::announce(&applied);
    Ok(applied)
}

/// A meeting's action plan is written (`meeting_end.rs`): learn from it,
/// once, when "Learn from meetings" is on.
pub fn meeting_ended(note_id: u64) {
    let cfg = crate::config::load();
    if !cfg.memory_learn_meetings || super::meeting_read(note_id, false) || crate::eval::active() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        match learn_from_meeting(note_id).await {
            Ok(a) => tracing::info!(note_id, changes = a.done.len(), "Memory: read a meeting"),
            Err(e) => tracing::info!(note_id, "Memory: didn't read the meeting: {e}"),
        }
    });
}

// ---- the scheduler ----------------------------------------------------------------------------------

/// A chat is read this long after its last message.
const QUIET: Duration = Duration::from_secs(5 * 60);
/// … and this long when a small model on the processor reads it: the
/// person is more likely done with the PC by then.
const QUIET_PROCESSOR: Duration = Duration::from_secs(10 * 60);
/// A pass put off (a dictation, the model stopped, a rate limit) is tried
/// again this much later.
const RETRY: Duration = Duration::from_secs(2 * 60);
/// A pass that found today's processor passes used up: an hour later.
const RETRY_USED_UP: Duration = Duration::from_secs(60 * 60);

/// How long a chat must be quiet before the Chat model reads it.
fn quiet_for(cfg: &crate::config::YapConfig) -> Duration {
    match endpoint(cfg) {
        Ok(ep) if chat_path(&ep) == Path::Small && on_processor(&ep) => QUIET_PROCESSOR,
        _ => QUIET,
    }
}

/// Conversations waiting for a pass, and when each was last active (or
/// when it's due, back-dated by [`QUIET`]).
static ACTIVITY: Mutex<Option<HashMap<u64, Instant>>> = Mutex::new(None);

fn activity<R>(f: impl FnOnce(&mut HashMap<u64, Instant>) -> R) -> R {
    let mut g = ACTIVITY.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// The scheduler runs in the app only (not a test run, not an evaluation:
/// those call the passes themselves).
fn scheduled() -> bool {
    !crate::e2e::active() && !crate::eval::active() && super::app().is_some()
}

/// A chat turn was answered in `conversation`: it's read once it's been
/// quiet for 5 minutes; any other conversation waiting is read now
/// (another chat started).
pub fn after_turn(conversation: u64) {
    if !scheduled() || !crate::config::load().memory_learn_chats {
        return;
    }
    let now = Instant::now();
    // Due now, however long the quiet time is.
    let due_now = now.checked_sub(QUIET_PROCESSOR.max(QUIET)).unwrap_or(now);
    let waiting: Vec<u64> = activity(|a| {
        for (id, t) in a.iter_mut() {
            if *id != conversation {
                *t = (*t).min(due_now);
            }
        }
        a.insert(conversation, now);
        a.keys().copied().collect()
    });
    super::set_pending(&waiting);
}

/// Start the scheduler: what a quit left waiting is due a minute after
/// start; then every 20 s the quiet conversations are read, one at a time.
pub fn start(_app: &AppHandle) {
    if !scheduled() {
        return;
    }
    let left = super::pending();
    if !left.is_empty() {
        let quiet = quiet_for(&crate::config::load());
        let due = Instant::now().checked_sub(quiet).map(|t| t + Duration::from_secs(60)).unwrap_or_else(Instant::now);
        activity(|a| {
            for id in left {
                a.insert(id, due);
            }
        });
    }
    tauri::async_runtime::spawn(async {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            tick().await;
        }
    });
}

async fn tick() {
    let cfg = crate::config::load();
    if !cfg.memory_learn_chats {
        return;
    }
    let quiet = quiet_for(&cfg);
    let now = Instant::now();
    let due: Vec<u64> = activity(|a| a.iter().filter(|(_, t)| now.duration_since(**t) >= quiet).map(|(id, _)| *id).collect());
    for id in due {
        match learn_from_chat(id, true).await {
            Ok(a) => {
                tracing::info!(conversation = id, changes = a.done.len(), "Memory: read a quiet chat");
                activity(|m| m.remove(&id));
            }
            Err(e) if e == USED_UP => {
                tracing::info!(conversation = id, "Memory: chat pass put off: {e}");
                let later = Instant::now().checked_sub(quiet).map(|t| t + RETRY_USED_UP).unwrap_or_else(Instant::now);
                activity(|m| m.insert(id, later));
            }
            Err(e) if e == DEFERRED || e.contains("limit") || e.contains("timed out") || e.contains("didn't start") => {
                tracing::info!(conversation = id, "Memory: chat pass put off: {e}");
                let later = Instant::now().checked_sub(quiet).map(|t| t + RETRY).unwrap_or_else(Instant::now);
                activity(|m| m.insert(id, later));
            }
            Err(e) => {
                tracing::info!(conversation = id, "Memory: chat not read: {e}");
                activity(|m| m.remove(&id));
            }
        }
    }
    let waiting: Vec<u64> = activity(|a| a.keys().copied().collect());
    super::set_pending(&waiting);
}

// ---- "Tell Yap what to change", imports ---------------------------------------------------------

/// What a proposal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tell,
    Import,
}

/// Changes proposed for the person to confirm: as a diff (`changes`, each
/// with its index for `include`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: u64,
    pub kind: &'static str,
    pub changes: Vec<Done>,
}

struct Pending {
    changes: Vec<Change>,
    kind: Kind,
    sensitive_ok: bool,
    /// What the person wrote: every change must come from it (`Rules::grounding`).
    grounding: String,
}

static PROPOSALS: Mutex<Option<HashMap<u64, Pending>>> = Mutex::new(None);
static PROPOSAL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn proposals<R>(f: impl FnOnce(&mut HashMap<u64, Pending>) -> R) -> R {
    let mut g = PROPOSALS.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn origin_of(kind: Kind) -> Origin {
    match kind {
        Kind::Tell => Origin::new("tell", None, ""),
        Kind::Import => Origin::new("import", None, "Imported"),
    }
}

/// What `changes` would do to memory now, without changing it.
fn preview(changes: &[Change], kind: Kind, sensitive_ok: bool, grounding: &str) -> (Vec<Item>, Vec<Done>) {
    let mut items = super::items();
    let mut next = items.iter().map(|i| i.id).max().unwrap_or(0) + 1_000_000;
    let out = ops::apply(
        &mut items,
        &mut next,
        changes,
        &Rules { sensitive_ok, origin: origin_of(kind), now: super::now_secs(), max_changes: max_changes(kind), grounding: Some(grounding.to_string()) },
    );
    (items, out.done)
}

fn max_changes(kind: Kind) -> usize {
    match kind {
        Kind::Tell => 8,
        Kind::Import => 30,
    }
}

/// An import's text in parts that fit an 8k model with memory beside them.
fn parts(text: &str, budget: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if !cur.is_empty() && crate::meetings::meeting_summary::estimate_tokens(&cur) + crate::meetings::meeting_summary::estimate_tokens(line) > budget {
            out.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Ask the Chat model what `text` changes ("Tell Yap what to change", an
/// import), and keep its changes as a proposal for the person to confirm.
pub async fn propose(text: &str, kind: Kind) -> Result<Proposal, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(match kind {
            Kind::Tell => "Type what Yap should change first".to_string(),
            Kind::Import => "Paste what the other assistant said first".to_string(),
        });
    }
    let cfg = crate::config::load();
    crate::ai_model::wake_on_device(&cfg).await?;
    let ep = endpoint(&cfg)?;
    // Told by the person themselves: theirs to keep. An import, reviewed,
    // follows the setting.
    let sensitive_ok = kind == Kind::Tell || cfg.memory_sensitive;
    // "Forget …" naming what's to go is Yap's own, for every model: the 4B's
    // full path took the sister out with "my old job at Acme" in 2 of 4
    // runs (2026-10-22), Yap's never. Nothing found by name: the model's.
    if kind == Kind::Tell {
        if let Some(what) = small::forget_request(text) {
            let changes = small::forget(&super::items(), &what);
            if !changes.is_empty() || path(&ep) == Path::Small {
                return Ok(keep_proposal(changes, kind, sensitive_ok, text));
            }
        }
    }
    if path(&ep) == Path::Small {
        return propose_small(text, kind, sensitive_ok, &cfg, &ep).await;
    }
    let (system, heading, chunks) = match kind {
        Kind::Tell => (prompt::tell_system(), "What the user wants changed", vec![super::ops::one_line(text, 1_500)]),
        Kind::Import => (
            prompt::import_system(),
            "What the other assistant remembers about the user",
            parts(text, if ep.local() { 1_600 } else { 3_000 }).into_iter().take(6).collect(),
        ),
    };
    let mut changes: Vec<Change> = Vec::new();
    for chunk in &chunks {
        // Each part sees memory with the earlier parts' changes.
        let (now, _) = preview(&changes, kind, sensitive_ok, text);
        let user = prompt::pass_input(&prompt::listing(&now, 900), heading, chunk);
        let reply = ask(&ep, &system, &user, max_changes(kind), false).await?;
        changes.extend(ops::parse_changes(&reply));
    }
    if kind == Kind::Tell {
        changes = narrow_you_removals(changes, &super::items(), text);
    }
    let mut changes = ops::route_preferences(changes, &super::items());
    // An import doesn't copy what About you says; a Tell is the person's
    // own ask.
    if kind == Kind::Import {
        let about_you = crate::about_you::profile(&cfg);
        changes = ops::drop_about_you(changes, &super::items(), &about_you.name, &about_you.about);
    }
    Ok(keep_proposal(changes, kind, sensitive_ok, text))
}

/// "Tell Yap what to change" and imports with a small model (`small.rs`),
/// "Forget …" aside (`propose`: Yap's own for every model): read a
/// sentence (an import: a line) at a time like a chat, each answer checked.
/// ≤ 8 sentences for a Tell, ≤ 30 lines for an import.
async fn propose_small(text: &str, kind: Kind, sensitive_ok: bool, cfg: &crate::config::YapConfig, ep: &Endpoint) -> Result<Proposal, String> {
    static REMEMBER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)^\s*remember\s*[:,]?\s*(?:that\s+)?").expect("static regex"));
    // ("Forget …" was Yap's own already: `propose`.)
    let items = super::items();
    let asked = REMEMBER.replace(text, "").to_string();
    let found = small::told(&asked, kind == Kind::Import);
    let candidates: Vec<small::Candidate> = found.candidates.into_iter().take(max_changes(kind)).collect();
    let t0 = Instant::now();
    let answers = extract(ep, &candidates, false).await?;
    let (changes, dropped) = small_changes(&candidates, &answers, &found.left, &items);
    let mut changes = ops::route_preferences(changes, &items);
    if kind == Kind::Import {
        let about_you = crate::about_you::profile(cfg);
        changes = ops::drop_about_you(changes, &items, &about_you.name, &about_you.about);
    }
    tracing::info!(kind = ?kind, asked = candidates.len(), left_out = dropped, changes = changes.len(), ms = t0.elapsed().as_millis() as u64, "Memory (small model): proposed changes");
    Ok(keep_proposal(changes, kind, sensitive_ok, text))
}

/// Keep `changes` as a proposal for the person to confirm, and say what
/// they would do (the diff).
fn keep_proposal(changes: Vec<Change>, kind: Kind, sensitive_ok: bool, text: &str) -> Proposal {
    let (_, done) = preview(&changes, kind, sensitive_ok, text);
    let id = PROPOSAL_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    proposals(|p| {
        p.insert(id, Pending { changes, kind, sensitive_ok, grounding: text.to_string() });
        // Only the latest few are kept.
        if p.len() > 5 {
            if let Some(oldest) = p.keys().min().copied() {
                p.remove(&oldest);
            }
        }
    });
    Proposal { id, kind: if kind == Kind::Tell { "tell" } else { "import" }, changes: done }
}

/// A model asked to "forget everything about my old job at Acme" removed
/// the whole Profile (Qwen3.5-4B, 2026-10-22), which memory never lets a
/// pass do: such a removal becomes taking out the facts that share a word
/// with what the person asked ("Works at Acme as a nurse"), or nothing.
/// For a "forget …" request, an update that only takes Profile or
/// Preferences facts out keeps the ones sharing a word with it too (the 4B
/// took "Lives in Norwich" and the dog out with the job, 2026-10-22); a
/// request with no such word ("Forget it") narrows nothing there.
/// Pure.
pub fn narrow_you_removals(changes: Vec<Change>, items: &[Item], asked: &str) -> Vec<Change> {
    const VAGUE: &[&str] = &["forget", "remember", "everything", "anything", "about", "please", "that", "this", "with", "from", "they", "them", "there", "were", "what", "which", "also"];
    const SMALL: &[&str] = &["the", "and", "for", "you", "your", "have", "has", "had", "not", "now", "all", "any", "can", "its", "out", "too", "was", "are", "but", "who"];
    let asked = ops::norm(asked);
    let words: Vec<&str> = asked.split(' ').filter(|w| w.len() >= 4 && !VAGUE.contains(w) && !crate::learn::is_frequent(w)).collect();
    let forgetting = asked.split(' ').any(|w| matches!(w, "forget" | "remove" | "delete" | "erase"));
    let fact_words: Vec<&str> = asked.split(' ').filter(|w| w.chars().count() >= 3 && !VAGUE.contains(w) && !SMALL.contains(w)).collect();
    let in_you = |c: &Change| c.id.and_then(|id| items.iter().find(|i| i.id == id)).is_some_and(|i| i.section.is_you());
    changes
        .into_iter()
        .filter_map(|c| {
            let only_removes = c.action == Action::Update
                && c.facts.is_empty()
                && c.about.as_deref().is_none_or(|a| a.trim().is_empty())
                && !c.remove_facts.is_empty();
            if forgetting && only_removes && !fact_words.is_empty() && in_you(&c) {
                let kept: Vec<String> =
                    c.remove_facts.iter().filter(|f| ops::norm(f).split(' ').any(|w| fact_words.contains(&w))).cloned().collect();
                return (!kept.is_empty()).then_some(Change { remove_facts: kept, ..c });
            }
            let you = c.action == Action::Remove && c.remove_facts.is_empty() && in_you(&c);
            if !you {
                return Some(c);
            }
            let item = items.iter().find(|i| Some(i.id) == c.id)?;
            let facts: Vec<String> = item
                .details
                .iter()
                .filter(|f| ops::norm(f).split(' ').any(|w| words.contains(&w)))
                .cloned()
                .collect();
            (!facts.is_empty()).then_some(Change { remove_facts: facts, ..c })
        })
        .collect()
}

/// Apply a proposal: all of it, or the changes `include` names (by their
/// index). Returns what was done (Undo: its batch).
pub fn apply_proposal(id: u64, include: Option<&[usize]>) -> Result<Applied, String> {
    let p = proposals(|p| p.remove(&id)).ok_or("Those changes are gone: ask again")?;
    let changes: Vec<Change> = p
        .changes
        .iter()
        .enumerate()
        .filter(|(i, _)| include.is_none_or(|inc| inc.contains(i)))
        .map(|(_, c)| c.clone())
        .collect();
    Ok(super::change(
        &changes,
        &Rules {
            sensitive_ok: p.sensitive_ok,
            origin: origin_of(p.kind),
            now: super::now_secs(),
            max_changes: max_changes(p.kind),
            grounding: Some(p.grounding),
        },
    ))
}

// ---- remember / forget in a chat ------------------------------------------------------------------

/// "Remember that…" / "Forget that…" as a whole message, for a model
/// without tools: `Some((forget, what))`. Not a question ("Remember when
/// we…?").
pub fn spoken_request(text: &str) -> Option<(bool, String)> {
    static ASK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?is)^\s*(?:hey\s+yap[,.]?\s+)?(?:(?:please|can you|could you|will you|yap)[,]?\s+)*(remember|forget|don't forget|do not forget)\b(?:\s+(?:that|this|about))?[:,]?\s+(.+?)\s*[.!]?\s*$")
            .expect("static regex")
    });
    let t = text.trim();
    if t.ends_with('?') {
        return None;
    }
    let c = ASK.captures(t)?;
    let verb = c[1].to_lowercase();
    let what = c[2].trim().to_string();
    let first = what.split_whitespace().next().unwrap_or("").to_lowercase();
    if ["when", "what", "how", "who", "where", "why", "if", "whether", "the time"].contains(&first.as_str()) || what.len() < 3 {
        return None;
    }
    // "Don't forget to call Mum" is a reminder, not memory.
    if verb != "remember" && verb != "forget" {
        return None;
    }
    if verb == "forget" && first == "to" {
        return None;
    }
    if verb == "remember" && first == "to" {
        return None;
    }
    Some((verb == "forget", what))
}

/// The answer to "Remember that …" / "Forget that …" for a model without
/// tools: the request through "Tell Yap what to change", applied, and
/// Yap's own words for it. `None` when it isn't such a request.
pub async fn spoken(text: &str) -> Option<Result<(String, Vec<String>), String>> {
    let (forget, what) = spoken_request(text)?;
    let ask = if forget { format!("Forget: {what}") } else { format!("Remember: {what}") };
    Some(
        async {
            let p = propose(&ask, Kind::Tell).await?;
            let applied = apply_proposal(p.id, None)?;
            if applied.done.is_empty() {
                let reply = if forget {
                    "I couldn't find that in what I remember. You can see everything in Settings → Memory.".to_string()
                } else {
                    "I didn't find anything to remember there. You can add it yourself in Settings → Memory.".to_string()
                };
                return Ok((reply, vec![]));
            }
            super::announce(&applied);
            let chips: Vec<String> = applied.done.iter().map(chip).collect();
            let reply = if forget { "Done: I've forgotten that." } else { "Got it: I'll remember that." };
            Ok((reply.to_string(), chips))
        }
        .await,
    )
}

/// A change as a chat chip: "Remembered: Rochelle, your sister".
fn chip(d: &Done) -> String {
    let what = if d.say.is_empty() { d.title.clone() } else { d.say.clone() };
    match d.kind {
        "removed" => format!("Forgot: {what}"),
        _ if d.facts_added.is_empty() && !d.facts_removed.is_empty() => format!("Forgot: {what}"),
        _ => format!("Remembered: {what}"),
    }
}

/// The memory tools' results ({success, data, displayText}, like every
/// Chat tool).
fn result(success: bool, data: Value, display: String) -> Value {
    json!({ "success": success, "data": data, "displayText": display })
}

/// Run `remember` or `forget` for the Chat's tool loop (`tools.rs`): the
/// person asked in their own message, so it's theirs to keep (sensitive
/// topics included); secrets never. The main window's toast says so, Undo
/// with it.
pub fn tool(name: &str, args: &Value) -> Value {
    let text = |k: &str| args[k].as_str().unwrap_or("").trim().to_string();
    let now = super::now_secs();
    let rules = Rules { sensitive_ok: true, origin: Origin::new("voice", None, "Asked in a chat"), now, max_changes: 3, grounding: None };
    match name {
        "remember" => {
            let fact = text("fact");
            let mut name = text("name");
            // Not a name ("user", "me", "dog": Qwen3.5-4B, 2026-10-22): a
            // fact about the person themselves.
            if !name.is_empty()
                && (!ops::looks_like_a_name(&name) || matches!(ops::norm(&name).as_str(), "me" | "myself" | "i" | "the user" | "yourself" | "them" | "him" | "her"))
            {
                name.clear();
            }
            let mut section = Section::parse(&text("section")).unwrap_or(if name.is_empty() { Section::Profile } else { Section::People });
            // A fact about someone with no name given: about the person.
            if !section.is_you() && name.is_empty() {
                section = Section::Profile;
            }
            if fact.is_empty() && name.is_empty() {
                return json!({ "success": false, "error": "Say what to remember in fact.", "displayText": "Couldn't remember that" });
            }
            // Someone or something new: the fact says who or what it is
            // ("Sister"); one Yap has: the fact goes with the rest.
            let known = super::items().iter().any(|i| i.section == section && ops::same_thing(section, &i.title, &name));
            let (about, facts) = if !section.is_you() && !known && fact.split_whitespace().count() <= 6 {
                (Some(fact), vec![])
            } else {
                (None, if fact.is_empty() { vec![] } else { vec![fact] })
            };
            let change = Change { action: Action::Add, id: None, section: Some(section), name, about, facts, remove_facts: vec![], say: String::new() };
            let applied = super::change(&[change], &rules);
            match applied.done.first() {
                Some(d) => {
                    super::announce(&applied);
                    result(true, json!({ "remembered": d.say, "section": d.section }), chip(d))
                }
                None => json!({
                    "success": false,
                    "error": "Nothing was saved: memory has that already, or it holds something Yap never keeps (passwords, numbers, addresses).",
                    "displayText": "Nothing new to remember",
                }),
            }
        }
        "forget" => {
            let what = text("what");
            let changes = forget_changes(&super::items(), &what);
            if changes.is_empty() {
                let listing = prompt::listing(&super::items(), 400);
                return json!({
                    "success": false,
                    "error": format!("Nothing in memory matches \"{what}\". Memory has:\n{listing}\nCall forget again with a name or a fact as memory words it, or tell the user it isn't there."),
                    "displayText": format!("Nothing in memory about \"{}\"", ops::one_line(&what, 40)),
                });
            }
            let applied = super::change(&changes, &rules);
            super::announce(&applied);
            let chips: Vec<String> = applied.done.iter().map(chip).collect();
            result(true, json!({ "forgot": applied.done.iter().map(|d| d.title.clone()).collect::<Vec<_>>() }), chips.join(" · "))
        }
        other => result(false, Value::Null, format!("Unknown tool: {other}")),
    }
}

/// The changes `forget(what)` makes: the facts it names (as memory words
/// them, or nearly), else the person, project or topic it names, whole.
pub fn forget_changes(items: &[Item], what: &str) -> Vec<Change> {
    let key = ops::norm(what);
    if key.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = key.split(' ').filter(|w| w.len() > 2).collect();
    let mut changes = Vec::new();
    for i in items {
        // Facts: the same fact, or most of its words.
        let facts: Vec<String> = i
            .details
            .iter()
            .filter(|f| {
                let fk = ops::norm(f);
                let fw: Vec<&str> = fk.split(' ').filter(|w| w.len() > 2).collect();
                // "work" and "works" are one word here.
                let alike = |a: &str, b: &str| a == b || (a.len().min(b.len()) >= 4 && (a.starts_with(b) || b.starts_with(a)));
                let shared = fw.iter().filter(|f| words.iter().any(|w| alike(f, w))).count();
                fk == key || key.contains(&fk) || fk.contains(&key) || (!fw.is_empty() && shared * 10 >= fw.len() * 6 && shared >= 2)
            })
            .cloned()
            .collect();
        if !facts.is_empty() {
            changes.push(Change {
                action: Action::Update,
                id: Some(i.id),
                section: None,
                name: String::new(),
                about: None,
                facts: vec![],
                remove_facts: facts,
                say: String::new(),
            });
        }
    }
    if !changes.is_empty() {
        return changes;
    }
    // Whole items, by name ("Sam", "my colleague Sam", "the allotment").
    for i in items.iter().filter(|i| !i.section.is_you()) {
        let title = ops::norm(&i.title);
        if title == key || (words.len() <= 4 && words.iter().any(|w| title.split(' ').next() == Some(w))) {
            changes.push(Change {
                action: Action::Remove,
                id: Some(i.id),
                section: None,
                name: String::new(),
                about: None,
                facts: vec![],
                remove_facts: vec![],
                say: String::new(),
            });
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_and_forget_requests_are_told_from_questions() {
        assert_eq!(spoken_request("Remember that my dog is called Beans"), Some((false, "my dog is called Beans".into())));
        assert_eq!(spoken_request("Please remember: I prefer short answers."), Some((false, "I prefer short answers".into())));
        assert_eq!(spoken_request("Forget that I work at Acme"), Some((true, "I work at Acme".into())));
        assert_eq!(spoken_request("forget everything about my old job"), Some((true, "everything about my old job".into())));
        assert_eq!(spoken_request("Remember when we went to Paris?"), None);
        assert_eq!(spoken_request("Remember to call Mum"), None);
        assert_eq!(spoken_request("Don't forget to call Mum"), None);
        assert_eq!(spoken_request("Can you remember what I said about the trip?"), None);
        assert_eq!(spoken_request("What's the weather like?"), None);
    }

    fn item(id: u64, section: Section, title: &str, details: &[&str]) -> Item {
        Item {
            id,
            section,
            title: title.into(),
            summary: String::new(),
            details: details.iter().map(|d| d.to_string()).collect(),
            source: Origin::new("chat", None, ""),
            created_ts: 0,
            updated_ts: 0,
            dictation: true,
        }
    }

    #[test]
    fn forget_takes_out_what_it_names() {
        let items = vec![
            item(1, Section::Profile, "Profile", &["Works at Acme as a nurse", "Lives in Norwich"]),
            item(2, Section::People, "Sam Patel", &["Colleague at Acme"]),
            item(3, Section::People, "Rochelle", &["Lives in York"]),
        ];
        // A fact as memory words it, or nearly.
        let c = forget_changes(&items, "works at Acme as a nurse");
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].id, c[0].remove_facts.clone()), (Some(1), vec!["Works at Acme as a nurse".to_string()]));
        // A person by name: the whole item.
        let c = forget_changes(&items, "Sam");
        assert_eq!((c.len(), c[0].action, c[0].id), (1, Action::Remove, Some(2)));
        // Nearly as memory words it.
        let c = forget_changes(&items, "I work at Acme as a nurse");
        assert_eq!(c[0].remove_facts, ["Works at Acme as a nurse"]);
        // Nothing matching: nothing.
        assert!(forget_changes(&items, "my old car").is_empty());
    }

    #[test]
    fn forgetting_a_job_never_takes_the_whole_profile() {
        let items = vec![
            item(1, Section::Profile, "Profile", &["Works at Acme as a nurse", "Lives in Norwich", "Has a dog called Beans"]),
            item(2, Section::People, "Sam Patel", &["Colleague at Acme"]),
        ];
        let remove = |id| Change { action: Action::Remove, id: Some(id), section: None, name: String::new(), about: None, facts: vec![], remove_facts: vec![], say: String::new() };
        let out = narrow_you_removals(vec![remove(1), remove(2)], &items, "Forget everything about my old job at Acme");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].remove_facts, ["Works at Acme as a nurse"]);
        assert!(out[1].remove_facts.is_empty(), "a person goes whole");
        // Nothing shared: the profile stays as it is.
        assert!(narrow_you_removals(vec![remove(1)], &items, "Forget everything").is_empty());
        // The 4B's fact-by-fact removals for the job: only the job's fact goes.
        let take_out = |facts: &[&str]| Change {
            action: Action::Update,
            remove_facts: facts.iter().map(|f| f.to_string()).collect(),
            ..remove(1)
        };
        let asked = "Forget everything about my old job at Acme";
        let out = narrow_you_removals(vec![take_out(&["Works at Acme as a nurse"]), take_out(&["Lives in Norwich"]), take_out(&["Has a dog called Beans"])], &items, asked);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].remove_facts, ["Works at Acme as a nurse"]);
        // "Forget that I have a dog": the dog goes (a short everyday word counts here).
        let out = narrow_you_removals(vec![take_out(&["Has a dog called Beans", "Lives in Norwich"])], &items, "Forget that I have a dog");
        assert_eq!(out[0].remove_facts, ["Has a dog called Beans"]);
        // Not a "forget": a correction's removal stays as the model wrote it.
        let out = narrow_you_removals(vec![take_out(&["Lives in Norwich"])], &items, "Actually I moved to York last month");
        assert_eq!(out[0].remove_facts, ["Lives in Norwich"]);
    }

    #[test]
    fn remember_with_a_name_that_isnt_one_is_about_the_person() {
        // Qwen3.5-4B's remember calls for "Remember that my dog's name is Beans".
        tool("remember", &json!({ "fact": "Dog's name is Beans", "name": "user", "section": "people" }));
        tool("remember", &json!({ "fact": "Dog's name is Beans", "name": "dog", "section": "people" }));
        let items = super::super::items();
        assert!(!items.iter().any(|i| i.section == Section::People), "{items:?}");
        assert_eq!(items.iter().find(|i| i.section == Section::Profile).map(|i| i.details.clone()), Some(vec!["Dog's name is Beans".to_string()]));
    }

    #[test]
    fn the_small_path_adds_what_yap_finds_to_what_the_model_says() {
        let found = small::candidates(
            &["My manager, Mr Okafor, wants a report by Friday. Please keep your answers short from now on. I've left Acme!"],
            false,
            &[],
        );
        assert_eq!(found.left, ["Acme"]);
        // The model skipped the manager's sentence; the preference had no call.
        let answers = vec![small::Extracted::default(); found.candidates.len()];
        let items = vec![item(1, Section::Profile, "Profile", &["Works at Acme as a nurse", "Lives in Norwich"])];
        let (changes, dropped) = small_changes(&found.candidates, &answers, &found.left, &items);
        assert_eq!(dropped, 1);
        // Acme's fact out; the preference in the person's words; Mr Okafor, found by Yap.
        assert_eq!((changes[0].id, changes[0].remove_facts.clone()), (Some(1), vec!["Works at Acme as a nurse".to_string()]));
        assert!(changes.iter().any(|c| c.section == Some(Section::Preferences) && c.facts == ["Keep your answers short"]));
        assert!(changes.iter().any(|c| c.name == "Mr Okafor" && c.about.as_deref() == Some("Manager")));
        // The model naming the same person: once.
        let mut answers = answers;
        answers[0] = small::Extracted { section: Some(Section::People), title: "Mr Okafor".into(), fact: String::new() };
        let (changes, _) = small_changes(&found.candidates, &answers, &found.left, &items);
        assert_eq!(changes.iter().filter(|c| c.name == "Mr Okafor").count(), 1);
    }

    #[test]
    fn an_import_comes_in_parts_an_8k_model_can_read() {
        let text = (0..400).map(|i| format!("- Memory line number {i} about the user\n")).collect::<String>();
        let p = parts(&text, 1_600);
        assert!(p.len() > 1);
        assert!(p.iter().all(|c| crate::meetings::meeting_summary::estimate_tokens(c) <= 1_700));
        assert_eq!(p.concat().lines().count(), 400);
    }
}
