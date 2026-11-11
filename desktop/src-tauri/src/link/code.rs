//! **Remote Control: Yap Code from a paired phone** (2026-11-08, wave 1:
//! home Wi-Fi only; the private repo's `remote-control-research.md`). Off
//! by default: Settings → Phone → Remote Control (`link_remote_control`),
//! with each phone ticked for it (off for phones paired before) and each
//! workspace allowing it (on unless switched off in its Environment card).
//!
//! A phone that may (`gates.rs`, checked on **every** message, the page's
//! view never trusted) can:
//! - **see** the Code side ([`list_now`]: workspaces, Code chats, agents in
//!   brief), what the status dots are made from ([`status_now`], sent on
//!   change, 300 ms apart at most), an agent's snapshot and live events (one
//!   watched agent a phone, on the agent lane: `lanes.rs`), older pages of
//!   its log and a file's diff on request;
//! - **answer** its cards: a permission prompt or a plan (`approve`) and an
//!   ask-first card (`confirm`) — only a card the PC sent that phone, at
//!   least 400 ms after it was shown, once ([`gates::Shown`]); the PC's card
//!   closes saying who answered ("Allowed from Office phone");
//! - **act**: Stop, Continue, "Tell the agent…", Go ahead, Undo (after the
//!   phone's own confirm), a Code chat's Mode up to Auto under its cap, a new
//!   Code chat in an existing workspace;
//! - and ask into a Code chat as before (`ask` / `start`): **a phone's turn
//!   runs at most at Auto**, never Bypass (`agents/remote.rs`, the runner's
//!   last gate), and its report is said on that phone (`chat/turn/report.rs`).
//!
//! Every action is recorded (`agents/remote.rs` [`Audit`]; the agent's own
//! log line, "Allowed from Office phone · Run a command: npm test"). The
//! log at `info`: kinds, outcomes and ids — never a command, a file, a brief
//! or anyone's words.

pub mod gates;
pub mod view;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Emitter;

use self::gates::{Action, Card, Gate, Limits, Refusal, Shown};
use crate::agents::modes::{self, Mode};
use crate::agents::remote::{self, Audit, RemoteNote};
use crate::agents::store::{self, Agent};
use crate::agents::{code_chat, runner};
use crate::link::voice::{say, Tx};
use crate::link::wire::{self, ApprovalRef, ChatRef, CodeChat, CodeReady, CodeWorkspace, ConfirmRef, In, Out, ThreadCode};
use crate::link::{locked, Hub};

/// The app event a chat's Mode changing goes out as (`agents/commands.rs`
/// `agent_mode_set`, and a phone's `set_mode`): `{conversation, mode}`.
pub const MODE_EVENT: &str = "yap-agent-mode";
/// How long an ask-first card waits (`confirm::WAIT`).
const CONFIRM_WAIT: Duration = crate::chat::pages::act::confirm::WAIT;

// ---- the gates' inputs, as they are now ---------------------------------------------------

/// The switches, read from the config when it's saved (it's read from disk
/// otherwise: never per agent event), and whether Claude Code is on this PC.
#[derive(Debug, Clone, Copy)]
struct Flags {
    master: bool,
    yap_code: bool,
    claude: bool,
    at: Instant,
}

static FLAGS: Mutex<Option<Flags>> = Mutex::new(None);
/// How long the switches are taken as read (Claude Code installed meanwhile).
const FLAGS_FOR: Duration = Duration::from_secs(5);

/// The config was saved (or Yap started): the switches again.
pub fn refresh(cfg: &crate::config::YapConfig) {
    let flags = Flags { master: cfg.link_remote_control, yap_code: cfg.agents_enabled, claude: crate::agents::offered(cfg), at: Instant::now() };
    *locked(&FLAGS) = Some(flags);
}

fn flags() -> Flags {
    if let Some(f) = locked(&FLAGS).filter(|f| f.at.elapsed() < FLAGS_FOR) {
        return f;
    }
    refresh(&crate::config::load());
    locked(&FLAGS).unwrap_or(Flags { master: false, yap_code: false, claude: false, at: Instant::now() })
}

/// **The gates for phone `device`** now.
pub fn gate(device: &str) -> Gate {
    let f = flags();
    Gate { master: f.master, yap_code: f.yap_code, claude: f.claude, ticked: f.master && remote::phone_on(device) }
}

/// Remote Control is on, and phone `device` may use it.
pub fn on_for(device: &str) -> bool {
    gates::on(&gate(device)).is_ok()
}

/// `ready.code` for phone `device`.
pub fn ready_code(device: &str) -> CodeReady {
    match gates::on(&gate(device)) {
        Ok(()) => CodeReady { on: true, why: None },
        Err(r) => CodeReady { on: false, why: Some(r.words(&crate::link::lan::pc_name())) },
    }
}

/// Workspace `name` as the gates read it.
fn workspace(name: &str) -> gates::Workspace {
    let cap = crate::agents::workspaces::list().into_iter().find(|f| f.name == name).and_then(|f| f.cap_mode());
    gates::Workspace { name: name.to_string(), allowed: remote::workspace_allows(name), cap }
}

/// Does workspace `name` allow Remote Control?
fn allowed(name: &str) -> bool {
    remote::workspace_allows(name)
}

// ---- what the hub keeps -------------------------------------------------------------------

/// An ask-first card waiting (from the thread's stream: [`ConfirmTee`]).
#[derive(Debug, Clone)]
struct Waiting {
    conversation: u64,
    /// The card's agent part (`AgentCard`).
    agent: Value,
    folder: String,
    asked: u64,
}

/// A card's end: its outcome, and the phone that answered it (none: this
/// PC, or nobody).
type Ended = (String, Option<String>);

/// **What the hub keeps for the Code side**: which phone was sent which
/// card, the ask-first cards waiting, who answered what, the conversations
/// answering, the status's number.
#[derive(Default)]
pub struct Shared {
    shown: Mutex<Shown>,
    confirms: Mutex<BTreeMap<u64, Waiting>>,
    /// An ask-first card a phone answered: its id and name, for its end.
    confirm_by: Mutex<HashMap<u64, (String, String)>>,
    /// A card's end, for a second answer: "Already allowed from Office phone".
    answered: Mutex<HashMap<(Card, u64), Ended>>,
    answering: Mutex<Vec<u64>>,
    seq: AtomicU64,
    dirty: AtomicBool,
}

/// **One phone's Code side** (in the hub's `Live`).
#[derive(Default)]
pub struct PhoneCode {
    limits: Mutex<Limits>,
    /// The workspace a new Code chat waits on (`code_new`): its path.
    pub new_code: Arc<Mutex<Option<String>>>,
    /// Remote Control was on for it when last told (`ready`).
    on: AtomicBool,
}

/// What a Code message's handler works with.
pub struct Ctx<'a> {
    pub hub: &'a Arc<Hub>,
    pub tx: &'a Tx,
    pub device: &'a str,
    pub name: &'a str,
    pub phone: &'a PhoneCode,
    /// The conversation the phone's questions go to.
    pub chat: &'a Arc<Mutex<ChatRef>>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn refuse(tx: &Tx, what: &'static str, r: &Refusal) {
    tracing::info!(what, why = r.kind(), "Remote Control: a phone's message refused");
    say(tx, Out::Error { message: r.words(&crate::link::lan::pc_name()) });
}

fn audit(cx: &Ctx<'_>, action: &str, agent: Option<&Agent>, outcome: &str) -> Audit {
    let a = Audit {
        ts: now_secs(),
        device: cx.device.to_string(),
        name: cx.name.to_string(),
        action: action.to_string(),
        agent: agent.map(|a| a.id.clone()),
        turn: agent.and_then(|a| a.turn_list.last()).map(|t| t.n),
        conversation: agent.and_then(|a| a.conversation),
        workspace: agent.map(|a| a.folder.clone()),
        outcome: outcome.to_string(),
        ..Default::default()
    };
    remote::record(a.clone());
    let _ = cx.hub.app.emit(crate::link::CHANGED, ());
    a
}

// ---- the Code side's messages, from one phone ----------------------------------------------

/// Is `m` one of the Code side's messages ([`handle`] takes it)?
pub fn is_code(m: &In) -> bool {
    matches!(
        m,
        In::CodeList
            | In::CodeNew { .. }
            | In::AgentOpen { .. }
            | In::AgentClose
            | In::AgentLog { .. }
            | In::AgentDiff { .. }
            | In::AgentStop { .. }
            | In::AgentContinue { .. }
            | In::AgentMessage { .. }
            | In::AgentGoAhead { .. }
            | In::AgentUndo { .. }
            | In::Approve { .. }
            | In::Confirm { .. }
            | In::SetMode { .. }
    )
}

/// **A Code message from the phone**, through every gate in order.
pub fn handle(cx: &Ctx<'_>, m: In) {
    let g = gate(cx.device);
    // Gates 2 and 3 first, whatever the message (letting go of an agent
    // always works): Remote Control on, this phone ticked.
    if !matches!(m, In::AgentClose) {
        if let Err(r) = gates::on(&g) {
            cx.tx.lane().with(|l| l.unwatch());
            return refuse(cx.tx, "code", &r);
        }
    }
    match m {
        In::CodeList => match gates::allows(&g, None, Action::Read) {
            Ok(()) => say(cx.tx, list_now()),
            Err(r) => refuse(cx.tx, "code_list", &r),
        },
        In::CodeNew { workspace } => code_new(cx, &g, &workspace),
        In::AgentOpen { id } => agent_open(cx, &g, &id),
        In::AgentClose => cx.tx.lane().with(|l| l.unwatch()),
        In::AgentLog { id, before } => agent_page(cx, &g, &id, |lines| {
            let (events, more) = view::page_before(lines, before);
            Out::AgentLog { id: id.clone(), events, more }
        }),
        In::AgentDiff { id, file, turn } => agent_page(cx, &g, &id, |lines| {
            let (lines, more, added, removed) = view::diff_of(&lines, &file, turn);
            Out::AgentDiff { id: id.clone(), file: file.clone(), lines, more, added, removed }
        }),
        In::AgentStop { id } => act(cx, &g, &id, "stop", "stopped", |a| {
            runner::stop(&a.id).map(|_| format!("Stopping the agent in {}.", a.folder))
        }),
        In::AgentContinue { id } => {
            let device = cx.device.to_string();
            act(cx, &g, &id, "continue", "continued", move |a| runner::continue_from(&a.id, Some(&device)).map(|_| format!("Continuing the agent in {}.", a.folder)))
        }
        In::AgentMessage { id, text } => agent_message(cx, &g, &id, &text),
        In::AgentGoAhead { id } => {
            let device = cx.device.to_string();
            act(cx, &g, &id, "go_ahead", "go ahead", move |a| runner::go_ahead_from(&a.id, Some(&device)).map(|_| format!("The agent in {} is carrying out its plan.", a.folder)))
        }
        In::AgentUndo { id, turn, sure } => agent_undo(cx, &g, &id, turn, sure),
        In::Approve { id, allow, shown_ms } => approve(cx, &g, id, allow, shown_ms),
        In::Confirm { id, yes, shown_ms } => confirm(cx, &g, id, yes, shown_ms),
        In::SetMode { chat, mode } => set_mode(cx, &g, chat, &mode),
        _ => {}
    }
}

/// An agent a phone may see, by its id: the agent and its workspace's gate.
fn agent_for(id: &str) -> Option<(Agent, gates::Workspace)> {
    let a = store::valid_id(id).then(|| store::get(id)).flatten()?;
    let ws = workspace(&a.folder);
    Some((a, ws))
}

fn code_new(cx: &Ctx<'_>, g: &Gate, name: &str) {
    let Some(folder) = code_chat::find(name).filter(|f| f.name.eq_ignore_ascii_case(name.trim()) || crate::agents::workspaces::norm(&f.path) == crate::agents::workspaces::norm(name)) else {
        return refuse(cx.tx, "code_new", &Refusal::NotFound);
    };
    if let Err(r) = gates::allows(g, Some(&workspace(&folder.name)), Action::Act) {
        return refuse(cx.tx, "code_new", &r);
    }
    if let Err(r) = locked(&cx.phone.limits).act(Instant::now()) {
        return refuse(cx.tx, "code_new", &r);
    }
    *locked(&cx.phone.new_code) = Some(folder.path.clone());
    *locked(cx.chat) = ChatRef::New;
    tracing::info!("Remote Control: a new Code chat waits for the phone's first question");
    say(cx.tx, Out::Thread { id: None, title: String::new(), messages: Vec::new(), more: false, code: Some(thread_code(&folder, None)), agents: Vec::new(), cards: Vec::new() });
}

/// **A Code chat's part of its thread** (`folder`: its workspace).
pub fn thread_code(folder: &crate::agents::workspaces::Folder, conversation: Option<u64>) -> ThreadCode {
    let cfg_mode = crate::config::load().agents_mode;
    let mode = modes::or_auto(&store::mode_of(conversation, &cfg_mode));
    let cap = folder.cap_mode();
    let max = cap.map_or(remote::PHONE_MAX, |c| c.min(remote::PHONE_MAX));
    ThreadCode {
        workspace: folder.name.clone(),
        path: folder.path.clone(),
        mode: mode.as_str().into(),
        mode_label: mode.label().into(),
        cap: cap.map(|c| c.as_str().to_string()),
        allowed: allowed(&folder.name),
        phone_max: max.as_str().into(),
    }
}

/// **`thread`'s Code parts** for conversation `id` and phone `device`: its
/// workspace, Mode and agents, and the cards waiting in it (each recorded
/// as sent to this phone) — when it's a Code chat in a workspace that
/// allows Remote Control, for a phone that may use it.
pub fn thread_parts(hub: &Hub, device: &str, id: u64) -> (Option<ThreadCode>, Vec<wire::Brief>, Vec<Value>) {
    if !on_for(device) {
        return (None, Vec::new(), Vec::new());
    }
    let Some(folder) = code_chat::folder_of(Some(id)).filter(|f| allowed(&f.name)) else { return (None, Vec::new(), Vec::new()) };
    let agents: Vec<wire::Brief> = store::of_conversation(id).iter().map(|a| view::brief(a, None)).collect();
    let mut cards = Vec::new();
    let now = Instant::now();
    for p in crate::integrations::claude_live::pending().into_iter().filter(|p| p.conversation == Some(id)) {
        locked(&hub.code.shown).sent(Card::Approve, p.id, device, now);
        cards.push(json!({ "kind": "approve", "card": view::card(serde_json::to_value(&p).unwrap_or(Value::Null)), "expiresTs": p.asked_ts + crate::integrations::claude_live::APPROVE_WAIT.as_secs() }));
    }
    for (cid, w) in locked(&hub.code.confirms).iter().filter(|(_, w)| w.conversation == id) {
        locked(&hub.code.shown).sent(Card::Confirm, *cid, device, now);
        cards.push(json!({ "kind": "confirm", "id": cid, "agent": w.agent, "expiresTs": w.asked + CONFIRM_WAIT.as_secs() }));
    }
    (Some(thread_code(&folder, Some(id))), agents, cards)
}

/// The drawer's items, with a Code chat's workspace named (Remote Control
/// on for the phone, its workspace allowing it).
pub fn drawer(device: &str, mut items: Vec<wire::ChatItem>) -> Vec<wire::ChatItem> {
    if !on_for(device) {
        return items;
    }
    let (chats, _) = code_chat::listing();
    for i in &mut items {
        i.code = chats.get(&i.id).filter(|f| allowed(&f.name)).map(|f| f.name.clone());
    }
    items
}

fn agent_open(cx: &Ctx<'_>, g: &Gate, id: &str) {
    let Some((agent, ws)) = agent_for(id) else { return refuse(cx.tx, "agent_open", &Refusal::NotFound) };
    if let Err(r) = gates::allows(g, Some(&ws), Action::Read) {
        return refuse(cx.tx, "agent_open", &r);
    }
    if let Err(r) = locked(&cx.phone.limits).page(Instant::now()) {
        return refuse(cx.tx, "agent_open", &r);
    }
    // Read and watched under the lane's lock: an event appended meanwhile
    // is in the snapshot, or comes after it, never neither (`lanes.rs`).
    let id = agent.id.clone();
    let events = cx.tx.lane().with(|l| {
        let lines = store::log_read_numbered(&id);
        let last = lines.last().map_or(0, |(n, _)| *n);
        let (record, events, more) = view::snapshot(&agent, lines);
        let n = events.len();
        l.watch(&id, last);
        l.page(Out::AgentSnapshot { agent: record, events, more, seq: 0 }.json());
        n
    });
    tracing::info!(events, "Remote Control: a phone opened an agent");
}

fn agent_page(cx: &Ctx<'_>, g: &Gate, id: &str, make: impl FnOnce(Vec<(u64, Value)>) -> Out) {
    let Some((agent, ws)) = agent_for(id) else { return refuse(cx.tx, "agent_page", &Refusal::NotFound) };
    if let Err(r) = gates::allows(g, Some(&ws), Action::Read) {
        return refuse(cx.tx, "agent_page", &r);
    }
    if let Err(r) = locked(&cx.phone.limits).page(Instant::now()) {
        return refuse(cx.tx, "agent_page", &r);
    }
    let out = make(store::log_read_numbered(&agent.id));
    cx.tx.lane().with(|l| l.page(out.json()));
}

/// **An action on an agent** (Stop, Continue, Go ahead): its gates, its rate,
/// then `run` (what to say back), recorded either way.
fn act(cx: &Ctx<'_>, g: &Gate, id: &str, what: &'static str, action: &str, run: impl FnOnce(&Agent) -> Result<String, String>) {
    let Some((agent, ws)) = agent_for(id) else { return refuse(cx.tx, what, &Refusal::NotFound) };
    if let Err(r) = gates::allows(g, Some(&ws), Action::Act) {
        return refuse(cx.tx, what, &r);
    }
    if let Err(r) = locked(&cx.phone.limits).act(Instant::now()) {
        return refuse(cx.tx, what, &r);
    }
    match run(&agent) {
        Ok(message) => {
            audit(cx, action, Some(&agent), "ok");
            runner::note_remote(&agent.id, 0, format!("{} from {}", past(action), cx.name), note(cx, action));
            say(cx.tx, Out::Acted { action: what.into(), id: agent.id.clone(), message });
        }
        Err(e) => {
            audit(cx, action, Some(&agent), "failed");
            say(cx.tx, Out::Error { message: e });
        }
    }
}

/// An action as the agent's tab says it: "Stopped from Office phone".
fn past(action: &str) -> &'static str {
    match action {
        "stopped" => "Stopped",
        "continued" => "Continued",
        "go ahead" => "Went ahead with the plan",
        "message" => "Told the agent",
        "undid" => "Undid its changes",
        "undid turn" => "Undid a turn",
        "approved" => "Allowed",
        "denied" => "Denied",
        "plan go ahead" => "Went ahead with the plan",
        "plan not yet" => "Kept it a plan",
        _ => "Done",
    }
}

fn note(cx: &Ctx<'_>, action: &str) -> RemoteNote {
    RemoteNote { device: cx.device.to_string(), name: cx.name.to_string(), action: action.to_string() }
}

fn agent_message(cx: &Ctx<'_>, g: &Gate, id: &str, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return say(cx.tx, Out::Error { message: "Say what to tell the agent.".into() });
    }
    if text.chars().count() > wire::MAX_CODE_TEXT {
        return say(cx.tx, Out::Error { message: "That's too long to send from the phone. Shorten it, then send it again.".into() });
    }
    let device = cx.device.to_string();
    let text = text.to_string();
    act(cx, g, id, "message", "message", move |a| {
        let go = crate::agents::consent::go_ahead(&text);
        // The phone's words: its turn at most at Auto (the runner holds it there too).
        let ask = runner::Ask { phone: Some(device), mode: Some(remote::PHONE_MAX), ..runner::Ask::of(&text, go) };
        runner::follow_up(&a.id, ask).map(|(j, how)| match how {
            runner::FollowedUp::Passed => crate::chat::tools::agents::passed_words(&j.folder),
            runner::FollowedUp::Queued => format!("The agent in {} is still working: it gets that when it finishes this step.", j.folder),
            _ => format!("Passed that to the agent in {}.", j.folder),
        })
    });
}

fn agent_undo(cx: &Ctx<'_>, g: &Gate, id: &str, turn: Option<u32>, sure: bool) {
    let Some((agent, ws)) = agent_for(id) else { return refuse(cx.tx, "agent_undo", &Refusal::NotFound) };
    if let Err(r) = gates::allows(g, Some(&ws), Action::Act) {
        return refuse(cx.tx, "agent_undo", &r);
    }
    if !sure {
        return say(cx.tx, Out::Error { message: "Confirm it on the phone first: Undo puts the agent's files back.".into() });
    }
    if let Err(r) = locked(&cx.phone.limits).undo(Instant::now()) {
        return refuse(cx.tx, "agent_undo", &r);
    }
    // Off the session's task: files are put back.
    let (tx, device, name, hub) = (cx.tx.clone(), cx.device.to_string(), cx.name.to_string(), Arc::clone(cx.hub));
    let chat = Arc::clone(cx.chat);
    tauri::async_runtime::spawn(async move {
        let id = agent.id.clone();
        let done = tauri::async_runtime::spawn_blocking(move || match turn {
            Some(n) => runner::undo_turn(&id, n),
            None => runner::undo(&id),
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        let phone = PhoneCode::default();
        let cx = Ctx { hub: &hub, tx: &tx, device: &device, name: &name, phone: &phone, chat: &chat };
        let action = if turn.is_some() { "undid turn" } else { "undid" };
        match done {
            Ok(report) => {
                audit(&cx, action, Some(&agent), "ok");
                let n = report.restored.len() + report.deleted.len();
                let message = match n {
                    0 => "Nothing to put back.".to_string(),
                    1 => format!("Put back 1 file in {}.", agent.folder),
                    n => format!("Put back {n} files in {}.", agent.folder),
                };
                runner::note_remote(&agent.id, turn.unwrap_or(0), format!("{} from {name}", past(action)), note(&cx, action));
                say(&tx, Out::Acted { action: "undo".into(), id: agent.id.clone(), message });
            }
            Err(e) => {
                audit(&cx, action, Some(&agent), "failed");
                say(&tx, Out::Error { message: e });
            }
        }
    });
}

/// "Already allowed from Office phone", "Already denied on your PC".
fn already(hub: &Hub, card: Card, id: u64) -> String {
    match locked(&hub.code.answered).get(&(card, id)) {
        Some((outcome, who)) => {
            let did = match outcome.as_str() {
                "allowed" => "allowed",
                "denied" | "refused" => "answered no",
                _ => "over",
            };
            match who {
                Some(name) => format!("Already {did} from {name}."),
                None if did == "over" => "That card isn't waiting any more.".into(),
                None => format!("Already {did} on your PC."),
            }
        }
        None => Refusal::NotShown.words(""),
    }
}

/// **Allow once / Deny on a phone**: the card waiting, in a workspace the
/// phone may answer for, sent to that phone, a fresh tap, the first answer.
fn approve(cx: &Ctx<'_>, g: &Gate, id: u64, allow: bool, shown_ms: u64) {
    let Some(card) = crate::integrations::claude_live::waiting(id) else {
        locked(&cx.hub.code.shown).close(Card::Approve, id);
        tracing::info!(why = "not waiting", "Remote Control: a phone's answer refused");
        return say(cx.tx, Out::Error { message: already(cx.hub, Card::Approve, id) });
    };
    let agent = store::get(&card.run);
    let ws = workspace(agent.as_ref().map_or(card.label.as_str(), |a| a.folder.as_str()));
    if let Err(r) = gates::allows(g, Some(&ws), Action::Answer) {
        return refuse(cx.tx, "approve", &r);
    }
    if let Err(r) = locked(&cx.hub.code.shown).tap(Card::Approve, id, cx.device, Instant::now(), shown_ms) {
        return refuse(cx.tx, "approve", &r);
    }
    let by = crate::integrations::claude_live::By::Phone { device: cx.device.to_string(), name: cx.name.to_string() };
    if !crate::integrations::claude_live::answer_by(id, allow, by) {
        return say(cx.tx, Out::Error { message: already(cx.hub, Card::Approve, id) });
    }
    let plan = card.tool == crate::integrations::claude_live::EXIT_PLAN;
    let action = match (plan, allow) {
        (true, true) => "plan go ahead",
        (true, false) => "plan not yet",
        (false, true) => "approved",
        (false, false) => "denied",
    };
    audit(cx, action, agent.as_ref(), "ok");
    if let Some(a) = &agent {
        let line = if plan { format!("{} from {}", past(action), cx.name) } else { format!("{} from {} · {}", past(action), cx.name, card.summary) };
        runner::note_remote(&a.id, 0, line, note(cx, action));
    }
}

/// **Start / Don't on a phone**: an ask-first card waiting, sent to that
/// phone, a fresh tap.
fn confirm(cx: &Ctx<'_>, g: &Gate, id: u64, yes: bool, shown_ms: u64) {
    let Some(w) = locked(&cx.hub.code.confirms).get(&id).cloned() else {
        locked(&cx.hub.code.shown).close(Card::Confirm, id);
        return say(cx.tx, Out::Error { message: already(cx.hub, Card::Confirm, id) });
    };
    if let Err(r) = gates::allows(g, Some(&workspace(&w.folder)), Action::Answer) {
        return refuse(cx.tx, "confirm", &r);
    }
    if let Err(r) = locked(&cx.hub.code.shown).tap(Card::Confirm, id, cx.device, Instant::now(), shown_ms) {
        return refuse(cx.tx, "confirm", &r);
    }
    locked(&cx.hub.code.confirm_by).insert(id, (cx.device.to_string(), cx.name.to_string()));
    if !crate::chat::pages::act::confirm::chat_page_confirm(id, yes) {
        locked(&cx.hub.code.confirm_by).remove(&id);
        return say(cx.tx, Out::Error { message: already(cx.hub, Card::Confirm, id) });
    }
    let a = Audit {
        ts: now_secs(),
        device: cx.device.to_string(),
        name: cx.name.to_string(),
        action: if yes { "started" } else { "not started" }.into(),
        conversation: Some(w.conversation),
        workspace: Some(w.folder.clone()),
        outcome: "ok".into(),
        ..Default::default()
    };
    remote::record(a);
    let _ = cx.hub.app.emit(crate::link::CHANGED, ());
}

/// **A Code chat's Mode from a phone**: up to Auto, under its workspace's
/// cap; never Bypass permissions (nor its one-time OK).
fn set_mode(cx: &Ctx<'_>, g: &Gate, chat: u64, mode: &str) {
    let Some(m) = Mode::parse(mode) else {
        return say(cx.tx, Out::Error { message: "That isn't one of the modes.".into() });
    };
    let Some(folder) = code_chat::folder_of(Some(chat)) else { return refuse(cx.tx, "set_mode", &Refusal::NotFound) };
    if let Err(r) = gates::allows(g, Some(&workspace(&folder.name)), Action::SetMode(m)) {
        return refuse(cx.tx, "set_mode", &r);
    }
    if let Err(r) = locked(&cx.phone.limits).act(Instant::now()) {
        return refuse(cx.tx, "set_mode", &r);
    }
    let was = modes::or_auto(&store::mode_of(Some(chat), &crate::config::load().agents_mode));
    store::set_mode(chat, m.as_str());
    let a = Audit {
        ts: now_secs(),
        device: cx.device.to_string(),
        name: cx.name.to_string(),
        action: "mode".into(),
        conversation: Some(chat),
        workspace: Some(folder.name.clone()),
        from: Some(was.as_str().into()),
        to: Some(m.as_str().into()),
        outcome: "ok".into(),
        ..Default::default()
    };
    remote::record(a);
    let _ = cx.hub.app.emit(MODE_EVENT, json!({ "conversation": chat, "mode": m.as_str() }));
    let _ = cx.hub.app.emit(crate::link::CHANGED, ());
}

// ---- what every phone hears ---------------------------------------------------------------

/// **The Code side now** (`code_list`): the workspaces that allow it, most
/// recently used first; their Code chats, newest first; their agents.
pub fn list_now() -> Out {
    let (chats, workspaces) = code_chat::listing();
    let ws: Vec<CodeWorkspace> = workspaces
        .iter()
        .filter(|w| allowed(&w.folder.name))
        .map(|w| CodeWorkspace { name: w.folder.name.clone(), path: w.folder.path.clone(), allowed: true, used_ts: w.used_ts, cap: w.folder.cap.clone() })
        .collect();
    let mut code_chats: Vec<CodeChat> = crate::chat::chats::with_all(|all| {
        all.iter()
            .filter(|c| crate::link::chats::usable(c))
            .filter_map(|c| chats.get(&c.id).filter(|f| allowed(&f.name)).map(|f| CodeChat { id: c.id, title: crate::link::chats::title(c), ts: c.updated_ts, workspace: f.name.clone() }))
            .collect()
    });
    code_chats.sort_by_key(|c| std::cmp::Reverse((c.ts, c.id)));
    code_chats.truncate(crate::link::chats::MAX_RECENTS);
    Out::CodeList { workspaces: ws, chats: code_chats, agents: agents_now() }
}

/// The agents a phone's lists and dots need, in workspaces that allow it.
fn agents_now() -> Vec<wire::Brief> {
    let all = store::list();
    view::worth_listing(&all, now_secs()).into_iter().filter(|a| allowed(&a.folder)).map(|a| view::brief(a, None)).collect()
}

/// **What the status dots are made from** (`code_status`).
pub fn status_now(hub: &Hub) -> Out {
    let approvals = crate::integrations::claude_live::pending()
        .into_iter()
        .filter(|p| store::get(&p.run).is_some_and(|a| allowed(&a.folder)))
        .map(|p| ApprovalRef { id: p.id, conversation: p.conversation, agent: p.run })
        .collect();
    let confirms = locked(&hub.code.confirms).iter().filter(|(_, w)| allowed(&w.folder)).map(|(id, w)| ConfirmRef { id: *id, conversation: w.conversation }).collect();
    Out::CodeStatus {
        seq: hub.code.seq.fetch_add(1, Ordering::SeqCst) + 1,
        answering: locked(&hub.code.answering).clone(),
        agents: agents_now(),
        approvals,
        confirms,
    }
}

/// Every connected phone with Remote Control on: its `tx` and device.
fn phones_on(hub: &Hub) -> Vec<(Tx, String, Arc<PhoneCode>)> {
    locked(&hub.live).values().filter(|l| on_for(&l.device)).map(|l| (l.tx.clone(), l.device.clone(), Arc::clone(&l.code))).collect()
}

/// **The dots changed**: every phone with Remote Control hears, 300 ms after
/// the first change (the desktop's dots wait as long).
pub fn dirty(hub: &Arc<Hub>) {
    if hub.code.dirty.swap(true, Ordering::SeqCst) {
        return;
    }
    let hub = Arc::clone(hub);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        hub.code.dirty.store(false, Ordering::SeqCst);
        let phones = phones_on(&hub);
        if phones.is_empty() {
            return;
        }
        let out = status_now(&hub);
        for (tx, _, _) in phones {
            say(&tx, out.clone());
        }
    });
}

/// **A phone that may use Remote Control now**: the dots, and every card
/// waiting it may answer (each recorded as sent to it).
pub fn welcome(hub: &Hub, tx: &Tx, device: &str) {
    say(tx, status_now(hub));
    let now = Instant::now();
    for p in crate::integrations::claude_live::pending() {
        if store::get(&p.run).is_some_and(|a| allowed(&a.folder)) {
            send_card(hub, tx, device, &p, now);
        }
    }
    let confirms: Vec<(u64, Waiting)> = locked(&hub.code.confirms).iter().map(|(id, w)| (*id, w.clone())).collect();
    for (id, w) in confirms.into_iter().filter(|(_, w)| allowed(&w.folder)) {
        locked(&hub.code.shown).sent(Card::Confirm, id, device, now);
        say(tx, Out::ConfirmCard { id, conversation: w.conversation, agent: w.agent, expires_ts: w.asked + CONFIRM_WAIT.as_secs() });
    }
}

fn send_card(hub: &Hub, tx: &Tx, device: &str, p: &crate::integrations::claude_live::Pending, now: Instant) {
    locked(&hub.code.shown).sent(Card::Approve, p.id, device, now);
    let card = view::card(serde_json::to_value(p).unwrap_or(Value::Null));
    say(tx, Out::ApproveCard { card, expires_ts: p.asked_ts + crate::integrations::claude_live::APPROVE_WAIT.as_secs() });
}

/// **`ready` again for every phone** (the switches changed: Remote Control,
/// Yap Code, a phone's tick): one that may use it now hears the dots and
/// the cards; one that may not any more stops watching.
pub fn ready_all(hub: &Hub) {
    let live: Vec<(Tx, String, Arc<PhoneCode>)> = locked(&hub.live).values().map(|l| (l.tx.clone(), l.device.clone(), Arc::clone(&l.code))).collect();
    for (tx, device, code) in live {
        say(&tx, hub.ready_for(&device));
        let on = on_for(&device);
        let was = code.on.swap(on, Ordering::SeqCst);
        if on && !was {
            welcome(hub, &tx, &device);
        } else if !on {
            tx.lane().with(|l| l.unwatch());
        }
    }
}

/// The phone just connected: told whether Remote Control is on for it,
/// then (when it is) the dots and the cards.
pub fn connected(hub: &Hub, tx: &Tx, device: &str, code: &PhoneCode) {
    let on = on_for(device);
    code.on.store(on, Ordering::SeqCst);
    if on {
        welcome(hub, tx, device);
    }
}

/// **An agent's event** (`runner::emit`, its log line): the phones watching
/// it hear it on their agent lane; the dots change.
pub fn agent_event(hub: &Arc<Hub>, id: &str, line: Option<u64>, event: Value) {
    let live: Vec<(Tx, String)> = locked(&hub.live).values().map(|l| (l.tx.clone(), l.device.clone())).collect();
    let watchers: Vec<(Tx, String)> = live.into_iter().filter(|(tx, _)| tx.lane().watches(id)).collect();
    if !watchers.is_empty() {
        let ws = store::get(id).map(|a| a.folder);
        let event = view::live(event);
        let now = Instant::now();
        for (tx, device) in watchers {
            if on_for(&device) && ws.as_deref().is_some_and(allowed) {
                tx.lane().with(|l| l.event(id, line, event.clone(), now));
            } else {
                // Remote Control went off for it (or its workspace's): no more.
                tx.lane().with(|l| l.unwatch());
            }
        }
    }
    dirty(hub);
}

/// **A permission prompt** (`yap-claude-approve`): to every phone with
/// Remote Control whose workspace allows it, on its control lane.
pub fn on_approve(hub: &Arc<Hub>, payload: &Value) {
    let Ok(p) = serde_json::from_value::<PendingIn>(payload.clone()) else { return };
    let Some(folder) = store::get(&p.run).map(|a| a.folder) else { return };
    if allowed(&folder) {
        let now = Instant::now();
        let pending = crate::integrations::claude_live::waiting(p.id);
        for (tx, device, _) in phones_on(hub) {
            if let Some(card) = &pending {
                send_card(hub, &tx, &device, card, now);
            }
        }
    }
    dirty(hub);
}

/// `yap-claude-approve`'s id and run.
#[derive(serde::Deserialize)]
struct PendingIn {
    id: u64,
    run: String,
}

/// `by` for phone `device`: "you" when it answered, "phone" (named) for
/// another, "pc", or none. Pure.
pub fn by_for(device: &str, by: Option<&str>, by_device: Option<&str>) -> Option<&'static str> {
    match by? {
        "pc" => Some("pc"),
        "phone" if by_device == Some(device) => Some("you"),
        "phone" => Some("phone"),
        _ => None,
    }
}

/// **A card's end** (`yap-claude-approve-done`): every phone with Remote
/// Control hears who answered.
pub fn on_approve_done(hub: &Arc<Hub>, payload: &Value) {
    let Some(id) = payload.get("id").and_then(Value::as_u64) else { return };
    let outcome = payload.get("outcome").and_then(Value::as_str).unwrap_or("ended").to_string();
    let by = payload.get("by").and_then(Value::as_str).map(str::to_string);
    let name = payload.get("byName").and_then(Value::as_str).map(str::to_string);
    let device = payload.get("byDevice").and_then(Value::as_str).map(str::to_string);
    locked(&hub.code.shown).close(Card::Approve, id);
    let who = if by.as_deref() == Some("phone") { name.clone() } else { None };
    locked(&hub.code.answered).insert((Card::Approve, id), (outcome.clone(), who));
    for (tx, d, _) in phones_on(hub) {
        let b = by_for(&d, by.as_deref(), device.as_deref());
        let name = (b == Some("phone")).then(|| name.clone()).flatten();
        say(&tx, Out::ApproveDone { id, outcome: outcome.clone(), by: b, name });
    }
    dirty(hub);
}

/// **An ask-first card in a turn's stream** ([`ConfirmTee`]): asking, it's
/// kept and sent to the phones; answered, every phone hears who.
fn confirm_seen(conversation: Option<u64>, card: &crate::chat::pages::act::confirm::Card) {
    let Some(hub) = crate::link::hub() else { return };
    let Some(agent) = card.agent.as_ref() else { return };
    if card.state == "asking" {
        let Some(conversation) = conversation else { return };
        let w = Waiting { conversation, agent: serde_json::to_value(agent).unwrap_or(Value::Null), folder: agent.folder.clone(), asked: now_secs() };
        locked(&hub.code.confirms).insert(card.id, w.clone());
        if allowed(&w.folder) {
            let now = Instant::now();
            for (tx, device, _) in phones_on(&hub) {
                locked(&hub.code.shown).sent(Card::Confirm, card.id, &device, now);
                say(&tx, Out::ConfirmCard { id: card.id, conversation, agent: w.agent.clone(), expires_ts: w.asked + CONFIRM_WAIT.as_secs() });
            }
        }
    } else {
        locked(&hub.code.confirms).remove(&card.id);
        locked(&hub.code.shown).close(Card::Confirm, card.id);
        let phone = locked(&hub.code.confirm_by).remove(&card.id);
        let by = match (&phone, card.state.as_str()) {
            (Some(_), _) => Some("phone"),
            (None, "allowed" | "refused") => Some("pc"),
            _ => None,
        };
        locked(&hub.code.answered).insert((Card::Confirm, card.id), (card.state.clone(), phone.as_ref().map(|(_, n)| n.clone())));
        for (tx, d, _) in phones_on(&hub) {
            let b = by_for(&d, by, phone.as_ref().map(|(dev, _)| dev.as_str()));
            let name = (b == Some("phone")).then(|| phone.as_ref().map(|(_, n)| n.clone())).flatten();
            say(&tx, Out::ConfirmDone { id: card.id, outcome: card.state.clone(), by: b, name });
        }
    }
    dirty(&hub);
}

/// **An agent's turn reported** (`yap-agent-report`): every phone with
/// Remote Control hears it; a phone with that chat open gets its marker and
/// answer as messages.
pub fn on_report(hub: &Arc<Hub>, payload: &Value) {
    let (Some(agent), Some(conversation)) = (payload.get("agent").and_then(Value::as_str), payload.get("conversation").and_then(Value::as_u64)) else { return };
    let Some(folder) = store::get(agent).map(|a| a.folder) else { return };
    if !allowed(&folder) {
        return;
    }
    let turn = payload.get("turn").and_then(Value::as_u64).unwrap_or(0) as u32;
    let status = payload.get("status").and_then(Value::as_str).unwrap_or("done").to_string();
    let ids: Vec<&str> = ["markerId", "messageId"].iter().filter_map(|k| payload.get(*k).and_then(Value::as_str)).collect();
    let conv = crate::chat::chats::get(conversation);
    let messages: Vec<wire::ThreadMessage> = ids
        .iter()
        .filter_map(|id| id.rsplit(':').next()?.parse::<usize>().ok())
        .filter_map(|i| conv.as_ref().and_then(|c| crate::link::chats::message(c, i)))
        .collect();
    let live: Vec<(Tx, String, Arc<Mutex<ChatRef>>)> = locked(&hub.live).values().map(|l| (l.tx.clone(), l.device.clone(), Arc::clone(&l.chat))).collect();
    for (tx, device, chat) in live {
        if !on_for(&device) {
            continue;
        }
        say(&tx, Out::Report { conversation, agent: agent.to_string(), turn, status: status.clone() });
        if *locked(&chat) == ChatRef::Id(conversation) {
            for m in &messages {
                if let Some(id) = &m.id {
                    hub.gave(&device, id);
                }
                say(&tx, Out::ChatMessage { conversation, message: m.clone() });
            }
        }
    }
    dirty(hub);
}

/// **A Code chat's Mode changed** (`yap-agent-mode`): every phone with
/// Remote Control hears it (a workspace that allows it).
pub fn on_mode(hub: &Arc<Hub>, payload: &Value) {
    let Some(chat) = payload.get("conversation").and_then(Value::as_u64) else { return };
    let Some(folder) = code_chat::folder_of(Some(chat)).filter(|f| allowed(&f.name)) else { return };
    let mode = modes::or_auto(payload.get("mode").and_then(Value::as_str).unwrap_or("auto"));
    let out = Out::Mode { chat, mode: mode.as_str().into(), label: mode.label().into(), cap: folder.cap.clone() };
    for (tx, _, _) in phones_on(hub) {
        say(&tx, out.clone());
    }
}

/// **The conversations answering** (`yap-chat-answering`): kept for the dots.
pub fn on_answering(hub: &Arc<Hub>, payload: &Value) {
    let ids: Vec<u64> = payload.get("conversations").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    *locked(&hub.code.answering) = ids;
    dirty(hub);
}

/// A phone removed: its cards go.
pub fn forget(hub: &Hub, device: &str) {
    locked(&hub.code.shown).forget(device);
}

// ---- an ask-first card's way to the phones ------------------------------------------------

/// **The stream an ask-first card goes through** (`chat/tools/agents.rs`
/// `allowed`): the thread's own, and the phones with Remote Control
/// ([`confirm_seen`]) as it passes.
pub struct ConfirmTee<'a> {
    inner: Option<crate::chat::stream::Live<'a>>,
    conversation: Option<u64>,
}

impl<'a> ConfirmTee<'a> {
    pub fn new(inner: Option<crate::chat::stream::Live<'a>>, conversation: Option<u64>) -> ConfirmTee<'a> {
        ConfirmTee { inner, conversation }
    }

    /// The thread's stream, through this (none: nobody watches the turn).
    pub fn live(&self) -> Option<crate::chat::stream::Live<'_>> {
        self.inner.map(|l| crate::chat::stream::Live { sink: self, beside: l.beside })
    }
}

impl crate::chat::stream::Sink for ConfirmTee<'_> {
    fn send(&self, ev: crate::chat::stream::Event) {
        if let crate::chat::stream::Event::Confirm(card) = &ev {
            if card.act == "agent" {
                confirm_seen(self.conversation, card);
            }
        }
        if let Some(l) = self.inner {
            l.sink.send(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cards_end_says_you_another_phone_or_the_pc() {
        assert_eq!(by_for("p-office", Some("phone"), Some("p-office")), Some("you"));
        assert_eq!(by_for("p-kitchen", Some("phone"), Some("p-office")), Some("phone"));
        assert_eq!(by_for("p-kitchen", Some("pc"), None), Some("pc"));
        assert_eq!(by_for("p-kitchen", None, None), None, "it timed out: nobody");
    }

    #[test]
    fn the_code_sides_messages_are_told_apart() {
        for t in wire::CODE_IN {
            let m = wire::parse(&json!({ "type": t, "id": "ab12", "workspace": "x", "file": "notes.md", "text": "t", "allow": true, "yes": true, "chat": 1, "mode": "auto" }).to_string());
            let m = match (t, m) {
                (&"approve", _) => wire::parse(r#"{"type":"approve","id":1,"allow":true,"shownMs":500}"#),
                (&"confirm", _) => wire::parse(r#"{"type":"confirm","id":1,"yes":true,"shownMs":500}"#),
                (_, m) => m,
            }
            .unwrap_or_else(|e| panic!("{t}: {e}"));
            assert!(is_code(&m), "{t}");
        }
        assert!(!is_code(&In::Ping) && !is_code(&In::Chats));
    }
}
