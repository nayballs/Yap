//! **A routine's runs**: queued ([`queue`]) and run one at a time by one
//! worker ([`worker`]), each under the guards —
//!
//! - **never over a dictation or a meeting**: a run waits while a dictation
//!   records or transcribes, a meeting records, or its own chat is answering
//!   (the runs list says what it waits for); waited past
//!   [`super::schedule::GRACE_SECS`] it's skipped, and a "before" one whose
//!   meeting has begun is too;
//! - **the daily cap** ([`super::schedule::DAILY_CAP`], every run that
//!   started today);
//! - **on the chat's default model** (Chat's scope, `turn::resolve_chat_endpoint`),
//!   every call counted as "Routines" (`ai/usage.rs`), with no look at the
//!   screen, acting on pages, opening apps, agents, connectors or cards
//!   (`super::as_routine`, read by `chat/turn/gates.rs`);
//!
//! — then where it goes: **its own chat** (Chat's own turn, the Skill run
//! across the meetings, or on the meeting that ended: `SkillTurn::meeting`)
//! or **with the meeting's note** (the Skill on that meeting as its Ask box
//! runs it, a new Ask thread kept with the note: `note_chats.rs`). And
//! **one quiet notice** on the Yap bar ([`notice`]).

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

use tauri::{AppHandle, Emitter};

use super::{schedule, with_store, Cause, Dest, Routine, Run, State};

/// A run waiting its turn.
#[derive(Debug, Clone)]
struct Pending {
    run: u64,
    routine: String,
    cause: Cause,
    slot: i64,
    /// The meeting it's about (a meeting's routine; Run now on one).
    note: u64,
    words: String,
}

static QUEUE: Mutex<VecDeque<Pending>> = Mutex::new(VecDeque::new());
/// The routine running now ("": none).
static RUNNING: Mutex<String> = Mutex::new(String::new());
static WAKE: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// How often a waiting run looks again.
const WAIT_STEP: std::time::Duration = std::time::Duration::from_secs(2);
/// The bar card's id and kind (`notices/policy.rs`).
pub const NOTICE: &str = "routine";

/// **Queue a run** of routine `routine` (recorded at once as waiting, so the
/// runs list shows it). One already waiting for it isn't queued twice.
pub(super) fn queue(routine: &str, cause: Cause, slot: i64, note: u64, words: String, about: String) {
    if QUEUE.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|p| p.routine == routine && p.cause == cause && p.slot == slot && p.note == note) {
        return;
    }
    let Some(r) = with_store(|st| st.routines.iter().find(|r| r.id == routine).cloned()) else { return };
    let (command, name) = super::skill_names(&r.skill);
    let now = super::now_secs();
    let run = with_store(|st| {
        let run = Run {
            id: super::new_run_id(st),
            routine: routine.to_string(),
            command,
            name,
            cause,
            slot,
            queued: now,
            state: State::Waiting,
            note,
            about,
            ..Default::default()
        };
        super::record_in(st, run.clone());
        super::save(st);
        run
    });
    super::changed();
    QUEUE.lock().unwrap_or_else(|p| p.into_inner()).push_back(Pending { run: run.id, routine: routine.to_string(), cause, slot, note, words });
    WAKE.notify_one();
}

/// Routine `id` was deleted: its runs waiting go (recorded as skipped).
pub(super) fn forget_queued(id: &str) {
    let gone: Vec<u64> = {
        let mut q = QUEUE.lock().unwrap_or_else(|p| p.into_inner());
        let gone = q.iter().filter(|p| p.routine == id).map(|p| p.run).collect();
        q.retain(|p| p.routine != id);
        gone
    };
    for run in gone {
        end(run, State::Skipped, "The routine was deleted.", 0, 0, 0);
    }
}

/// The routines with a run waiting or running.
pub(super) fn busy_ids() -> Vec<String> {
    let mut out: Vec<String> = QUEUE.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|p| p.routine.clone()).collect();
    let running = RUNNING.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if !running.is_empty() {
        out.push(running);
    }
    out
}

/// A run's record changed.
fn update(run: u64, f: impl FnOnce(&mut Run)) {
    let found = with_store(|st| match st.runs.iter_mut().find(|r| r.id == run) {
        Some(r) => {
            f(r);
            super::save(st);
            true
        }
        None => false,
    });
    if found {
        super::changed();
    }
}

fn end(run: u64, state: State, why: &str, chat: u64, note: u64, thread: u64) {
    let now = super::now_secs();
    update(run, |r| {
        r.state = state;
        r.why = why.to_string();
        r.ended = now;
        if chat != 0 {
            r.chat = chat;
        }
        if note != 0 {
            r.note = note;
        }
        r.thread = thread;
    });
}

/// **What a run must wait for now**, in the runs list's words: a dictation,
/// a meeting recording, its own chat answering. Lock-free but for the
/// stream's list.
fn waits_for(chat: u64) -> Option<&'static str> {
    let m = crate::notices::policy::moment();
    if m.meeting {
        Some("Waiting for the meeting being recorded to end.")
    } else if m.dictating {
        Some("Waiting for the dictation to finish.")
    } else if chat != 0 && crate::chat::stream::is_answering(chat) {
        Some("Waiting for its chat to finish answering.")
    } else {
        None
    }
}

/// **The worker**: the queue's runs, one at a time, for as long as Yap runs.
pub(super) async fn worker(app: AppHandle) {
    loop {
        let next = QUEUE.lock().unwrap_or_else(|p| p.into_inner()).pop_front();
        let Some(p) = next else {
            WAKE.notified().await;
            continue;
        };
        *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = p.routine.clone();
        work(&app, &p).await;
        RUNNING.lock().unwrap_or_else(|p| p.into_inner()).clear();
        super::changed();
    }
}

/// One run, from waiting to its end.
async fn work(app: &AppHandle, p: &Pending) {
    // Waits out a dictation, a meeting, its chat answering.
    let mut said: Option<&'static str> = None;
    loop {
        let Some(r) = with_store(|st| st.routines.iter().find(|r| r.id == p.routine).cloned()) else {
            end(p.run, State::Skipped, "The routine was deleted.", 0, 0, 0);
            return;
        };
        if !r.on && p.cause != Cause::Now {
            end(p.run, State::Skipped, "The routine was switched off.", 0, 0, 0);
            return;
        }
        let now = super::now_secs();
        match waits_for(r.chat) {
            None => break,
            Some(why) => {
                if too_late(p, now) {
                    let why = if p.cause == Cause::Event { "Its meeting began before it could run." } else { "A dictation or a meeting went on past its time." };
                    tracing::info!(routine = %p.routine, "Routines: a run waited too long; skipped");
                    end(p.run, State::Skipped, why, 0, 0, 0);
                    return;
                }
                if said != Some(why) {
                    said = Some(why);
                    update(p.run, |run| run.why = why.to_string());
                }
            }
        }
        tokio::time::sleep(WAIT_STEP).await;
    }
    if too_late(p, super::now_secs()) && p.cause == Cause::Event {
        end(p.run, State::Skipped, "Its meeting began before it could run.", 0, 0, 0);
        return;
    }
    // Today's cap.
    let now = super::now_secs();
    let today = with_store(|st| schedule::runs_today(st.runs.iter().map(|r| r.started), now, &chrono::Local));
    if schedule::over_cap(today) {
        tracing::info!(routine = %p.routine, today, "Routines: today's cap reached; skipped");
        end(p.run, State::Skipped, &format!("Today's {} runs are done: routines run again tomorrow.", schedule::DAILY_CAP), 0, 0, 0);
        return;
    }
    let Some(routine) = with_store(|st| st.routines.iter().find(|r| r.id == p.routine).cloned()) else {
        end(p.run, State::Skipped, "The routine was deleted.", 0, 0, 0);
        return;
    };
    update(p.run, |run| {
        run.state = State::Running;
        run.started = now;
        run.why.clear();
    });
    tracing::info!(routine = %routine.id, cause = ?p.cause, "Routines: running");
    let out = super::as_routine(crate::ai::usage::within(crate::ai::usage::Feature::Routines, one(app, &routine, p))).await;
    match out {
        Ok(o) => {
            tracing::info!(routine = %routine.id, chat = o.chat, note = o.note, "Routines: ran");
            end(p.run, State::Done, "", o.chat, o.note, o.thread);
            let _ = app.emit(super::RAN, serde_json::json!({ "routine": routine.id, "conversationId": (o.chat != 0).then_some(o.chat), "noteId": (o.note != 0).then_some(o.note) }));
            notice(app, &routine, Ok(&o));
        }
        Err(why) => {
            tracing::warn!(routine = %routine.id, "Routines: a run failed ({} chars said)", why.chars().count());
            end(p.run, State::Failed, &why, 0, 0, 0);
            notice(app, &routine, Err(&why));
        }
    }
}

/// A run waited too long: a schedule's past the grace from its time, a
/// "before" one's meeting begun, anything else a few hours on.
fn too_late(p: &Pending, now: i64) -> bool {
    match p.cause {
        Cause::Event => p.slot > 0 && now >= p.slot,
        Cause::Schedule | Cause::Missed => p.slot > 0 && now - p.slot > schedule::GRACE_SECS,
        Cause::Now | Cause::Meeting => {
            let queued = with_store(|st| st.runs.iter().find(|r| r.id == p.run).map(|r| r.queued)).unwrap_or(now);
            now - queued > schedule::GRACE_SECS
        }
    }
}

/// What a run made.
pub(super) struct Outcome {
    pub chat: u64,
    pub note: u64,
    pub thread: u64,
    /// The answer, for the notice's line.
    pub reply: String,
}

/// **Run routine `r` once**: its Skill (deleted: said), on the chat's
/// default model (none set up: said), with the meeting's note or in its own
/// chat.
async fn one(app: &AppHandle, r: &Routine, p: &Pending) -> Result<Outcome, String> {
    let skill = crate::chat::skills::all().into_iter().find(|s| s.id == r.skill).ok_or(super::SKILL_GONE)?;
    let cfg = crate::config::load();
    crate::chat::turn::resolve_chat_endpoint(&cfg)?;
    if p.note != 0 && r.to == Dest::Note && skill.runs_on.on_one() {
        return on_note(app, &skill, p.note, &p.words).await;
    }
    in_chat(app, r, skill, p).await
}

/// The question a run asks: the Skill's command and its words.
fn question(command: &str, words: &str) -> String {
    format!("/{command} {}", words.trim()).trim().to_string()
}

/// **In its own chat**: Chat's own turn, the Skill run as typed (across the
/// meetings, or on the meeting that ended), into the routine's chat — made
/// at its first run and named after the Skill; archived, it comes back.
async fn in_chat(app: &AppHandle, r: &Routine, skill: crate::chat::skills::Skill, p: &Pending) -> Result<Outcome, String> {
    let (name, command) = (skill.name.clone(), skill.command.clone());
    let mut turn = crate::chat::assistant::skill::turn(skill, p.words.clone());
    turn.meeting = (p.note != 0).then_some(p.note);
    // One with nothing to run on (/prep with nothing coming up): said, no model called.
    if let Some(asks) = crate::chat::skills::ask_first(&turn, false) {
        return Err(asks);
    }
    let chat = crate::chat::chats::get(r.chat).filter(|c| c.note.is_none() && !c.incognito);
    if let Some(c) = &chat {
        if c.archived_ts > 0 {
            crate::chat::chats::set_archived(&[c.id], false);
        }
    }
    let text = question(&command, &p.words);
    let fut = crate::chat::turn::chat_turn(Some(app), chat.as_ref().map(|c| c.id), &text, None, false, |_, _| {}, None, None);
    let fut = crate::chat::skills::with(Some(turn), fut);
    // (Type-erased and boxed: a turn's future is big, and proving it Send
    // through every layer is past the compiler's limit otherwise.)
    let fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<crate::chat::turn::ChatTurn, String>> + Send + '_>> = Box::pin(fut);
    let done = fut.await.map_err(|e| {
        if e.contains(crate::chat::stream::SUPERSEDED) {
            "You asked something in its chat while it ran.".to_string()
        } else {
            e
        }
    })?;
    let conv = done.conversation_id;
    if chat.as_ref().map(|c| c.id) != Some(conv) {
        // Its first run (or its chat was deleted): the new chat is its own.
        let _ = crate::chat::chats::rename(conv, &name);
        with_store(|st| {
            if let Some(rt) = st.routines.iter_mut().find(|x| x.id == r.id) {
                rt.chat = conv;
                super::save(st);
            }
        });
    }
    let _ = crate::chat::chats::saved();
    Ok(Outcome { chat: conv, note: 0, thread: 0, reply: done.reply })
}

/// **With the meeting's note**: the Skill on meeting `note_id` as its Ask
/// box runs it (its passages, guardrails, citations checked), on the chat's
/// default model, kept as a new Ask thread with the note.
async fn on_note(app: &AppHandle, skill: &crate::chat::skills::Skill, note_id: u64, words: &str) -> Result<Outcome, String> {
    use crate::meetings::summary;
    let note = crate::notes::get(note_id).ok_or("The meeting's note isn't there any more.")?;
    let cited = crate::meetings::assist::skill_input_for(skill, &note, words).ok_or_else(|| format!("There was nothing in the meeting for /{} to work on.", skill.command))?;
    let cfg = crate::config::load();
    let (base_url, api_key, model, provider, _persona, disable_thinking) = crate::chat::turn::resolve_chat_endpoint(&cfg)?;
    let ep = summary::Endpoint { base_url, api_key, model, provider, fragment: String::new(), disable_thinking };
    let opts = ep.options(summary::chips::SKILL_REPLY_TOKENS, 0.3);
    let by = crate::ai::ai_model::who_answers(&cfg, &ep.provider, &ep.model, &ep.base_url);
    let asked = crate::ai::ai_model::messages_with_answering(crate::meetings::assist::skill_messages(skill, &cited.text, words), &by);
    let messages = crate::memory::with_helper_block(asked, &cfg);
    let ask = summary::chat_beside_dictation_live(app, &ep, messages, &opts, None, false);
    let ask: std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + '_>> = Box::pin(ask);
    let reply = ask.await?;
    let reply = crate::chat::skills::tidy_answer(skill, &reply);
    let (answer, sources) = summary::chips::checked(&reply, &cited.passages, true);
    if answer.trim().is_empty() {
        return Err("The model returned an empty answer.".into());
    }
    let named = crate::ai::ai_model::describe(&ep.provider, &ep.model, &ep.base_url, &crate::ai::local_llm::active_model_display(&cfg)).chat_model();
    let messages = [
        crate::chat::note_chats::NewMessage { role: "user".into(), text: question(&skill.command, words), plain: false, stopped: false, model: None, sources: Vec::new() },
        crate::chat::note_chats::NewMessage { role: "assistant".into(), text: answer.clone(), plain: false, stopped: false, model: Some(named), sources },
    ];
    let thread = crate::chat::note_chats::add(note_id, None, &messages)?;
    crate::chat::chats::saved().map_err(|e| e.to_string())?;
    let _ = app.emit(crate::chat::note_chats::EVENT, serde_json::json!({ "noteId": note_id, "conversationId": thread, "origin": "routine" }));
    Ok(Outcome { chat: 0, note: note_id, thread, reply: answer })
}

/// The notice's title. Pure.
pub fn notice_title(command: &str, ok: bool) -> String {
    if ok {
        format!("Routine ran: /{command}")
    } else {
        format!("Routine didn't run: /{command}")
    }
}

/// The notice's line: the answer's first words (its markdown plain), or why
/// it failed. Pure.
pub fn notice_body(reply: &str, max: usize) -> String {
    let lines: Vec<&str> = reply.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    // A heading says what the answer is, not what it says: left out when
    // there's more.
    let body: Vec<&str> = lines.iter().copied().filter(|l| !l.starts_with('#')).collect();
    let lines = if body.is_empty() { lines } else { body };
    let plain = lines
        .iter()
        .map(|l| {
            // (Its citations, "[1]" or "[1, 3]", are for the chat: the card has no pills.)
            let line = drop_citations(&l.trim_start_matches(['#', '-', '*', '>', ' ']).replace("**", "")).split_whitespace().collect::<Vec<_>>().join(" ");
            // Each bullet a sentence of its own.
            if line.is_empty() || line.ends_with(['.', '!', '?', ':', ';', ',']) {
                line
            } else {
                format!("{line}.")
            }
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if plain.chars().count() <= max {
        return plain;
    }
    let cut: String = plain.chars().take(max).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(a, _)| a).trim_end_matches([',', ';', ':', '.']);
    format!("{cut}…")
}

/// **`s` without its citations** ("[1]", "[1, 3]"), each with the space
/// before it; what follows one stays, so a sentence it ends keeps its full
/// stop ("send it [1]. The reply" → "send it. The reply": Yap Lab, 16 Nov
/// 2026, read "send it The reply"). Other brackets stay. Pure.
fn drop_citations(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        let after = &rest[i + 1..];
        let cite = after.find(']').filter(|&e| {
            let inner = &after[..e];
            inner.chars().any(|c| c.is_ascii_digit()) && inner.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ')
        });
        match cite {
            Some(e) => {
                out.push_str(rest[..i].trim_end_matches(' '));
                rest = &after[e + 1..];
            }
            None => {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// **The one quiet notice**: a card on the Yap bar under the notice policy
/// (kind [`NOTICE`]: held over a dictation and a meeting, dropped if unseen
/// for ten minutes; a newer one takes its place), never with Yap's main
/// window in front, nor while its own chat is in view beside it. Open
/// brings up its chat or note (failed: Settings → Skills).
fn notice(app: &AppHandle, r: &Routine, out: Result<&Outcome, &String>) {
    let (command, _) = super::skill_names(&r.skill);
    let (title, body, open) = match out {
        Ok(o) => (notice_title(&command, true), notice_body(&o.reply, 140), if o.chat != 0 { format!("chat:{}", o.chat) } else { format!("note:{}", o.note) }),
        Err(why) => (notice_title(&command, false), notice_body(why, 140), "settings".to_string()),
    };
    let failed = out.is_err();
    let chat = out.ok().map_or(0, |o| o.chat);
    let app = app.clone();
    // Window getters wait on the main thread: off it.
    std::thread::spawn(move || {
        if crate::notices::policy::yap_in_front(&app) {
            return;
        }
        let its_chat = chat != 0 && crate::chat::assistant::chat_viewing() == crate::chat::assistant::Viewing::Conversation(chat);
        if its_chat && crate::notices::policy::window_seen(&app) == crate::notices::policy::Seen::Beside {
            return;
        }
        let card = crate::bar::Card {
            id: NOTICE.into(),
            icon: if failed { "error" } else { "routine" },
            title,
            body,
            primary: Some(crate::bar::CardAction::new(open, if failed { "Settings" } else { "Open" })),
            timeout_ms: Some(12_000),
            ..Default::default()
        };
        if !crate::bar::show_card(&app, card, Box::new(card_action)) {
            tracing::info!("Routines: no bar to say it on");
        }
    });
}

fn card_action(app: &AppHandle, action: &str) {
    if let Some(id) = action.strip_prefix("chat:").and_then(|s| s.parse().ok()) {
        open(app, id, 0);
    } else if let Some(id) = action.strip_prefix("note:").and_then(|s| s.parse().ok()) {
        open(app, 0, id);
    } else if action == "settings" {
        let _ = crate::shell::window::show_settings(app);
        let _ = app.emit("yap-open-settings", "skills#routines");
    }
}

/// **Open a run's result**: the main window, on its chat (as the Voice
/// Assistant's Open in Chat brings one up) or on the meeting's note.
pub(super) fn open(app: &AppHandle, chat: u64, note: u64) {
    if let Err(e) = crate::shell::window::show_settings(app) {
        tracing::warn!("Routines: couldn't open the main window ({e})");
        return;
    }
    if chat != 0 {
        let _ = app.emit("yap-assistant-turn", serde_json::json!({ "phase": "open", "conversationId": chat, "show": true }));
    } else if note != 0 {
        let _ = app.emit("yap-open-note", serde_json::json!({ "id": note }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_says_what_ran_in_a_line() {
        assert_eq!(notice_title("standup", true), "Routine ran: /standup");
        assert_eq!(notice_title("prep", false), "Routine didn't run: /prep");
        let reply = "**Yesterday:**\n- I finished the finance request\n\n**Today:**\n- Call the supplier";
        assert_eq!(notice_body(reply, 140), "Yesterday: I finished the finance request. Today: Call the supplier.");
        // A heading left out; citations too; each bullet a sentence.
        assert_eq!(notice_body("### Follow-up email\n- Draft the finance request, due Wednesday [1].\n- Send the quote[2]", 140), "Draft the finance request, due Wednesday. Send the quote.");
        assert_eq!(notice_body("## Only a heading", 140), "Only a heading.");
        // A citation that ends a sentence mid-line: its full stop stays.
        assert_eq!(notice_body("- Revised budget: you asked Alice to send it [1]. The reply came on Friday [1, 2].", 140), "Revised budget: you asked Alice to send it. The reply came on Friday.");
        assert_eq!(notice_body("Sent it [1], then[2]: done. See [the list] too.", 140), "Sent it, then: done. See [the list] too.");
        let long = "word ".repeat(60);
        let body = notice_body(&long, 40);
        assert!(body.ends_with('…') && body.chars().count() <= 41, "{body}");
        assert_eq!(question("standup", ""), "/standup");
        assert_eq!(question("project-update", " Atlas "), "/project-update Atlas");
    }
}
