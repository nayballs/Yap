//! **Set up your mouse's side buttons** (2026-10-31; Wispr Flow's "Use your
//! mouse's side buttons?", by behaviour): Dictate on one, Send on the other,
//! in one step — Settings → Shortcuts' dialog (`settings/SideButtons.svelte`),
//! and a tip the first time Yap sees a side button pressed.
//!
//! - **Which button** ([`heard`]): the dialog listens through the input hook
//!   (`input_hook::listen_side_buttons`; XBUTTON1 is Back, XBUTTON2 Forward)
//!   and is told which it heard (`yap-side-button`); the page hears one too
//!   when the hook doesn't (a Yap window in front).
//! - **The bindings** ([`plan`], pure): each button goes on an action that
//!   exists already — Dictate (`hotkey`) and Send (`sendHotkey`) — as one more
//!   key (`moreHotkeys`; the first shortcut when the action has none), never
//!   taking the person's keyboard shortcut away. A button on another shortcut
//!   (a fixed action's, a language shortcut's, a transform's) is named and
//!   moved only when asked (`take`) — between Dictate and Send themselves (a
//!   swap) without asking —, the dictation's only key never; an action with
//!   four keys has no room. Push-to-talk (hold, double-tap for hands-free:
//!   `gesture.rs`) or toggle is the dictation's recording mode, every
//!   dictation key's (the plan says so when it changes another key's).
//! - **What it says** ([`Plan`]): the summary ("Back: hold to dictate,
//!   double-tap for hands-free · Forward: send"), each button's note, and the
//!   browser's: the hook swallows a bound mouse button both ways and leaves
//!   an unbound one alone, so "Back won't go back in your browser while it's
//!   set to dictate", and a button left out keeps its meaning.
//! - **Undo / Remove** ([`undo`], [`remove`], pure): the last setup or removal
//!   put back where nothing has changed it since; the side buttons taken off
//!   Dictate and Send (the dictation's only key stays).
//! - **The tip** ([`seen`]): an unbound side button pressed (the hook says
//!   so until the tip has had its turn): `tips::side_buttons`, once ever,
//!   with Set up ([`open_setup`]); never once a side button is bound to
//!   anything.
//!
//! The log says which buttons and jobs, never more.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tracing::info;

use crate::config::YapConfig;
use crate::dictation::input_hook::{clashes, first_spec, more_specs, set_first_spec, DICTATION, FIXED_COUNT, FIXED_KEYS, MORE, SEND};

/// The back side button (XBUTTON1), as the input hook numbers it.
pub const BACK: u32 = 4;
/// The forward side button (XBUTTON2).
pub const FORWARD: u32 = 5;
/// The page's event: a side button heard for the setup (`{ button, name }`).
pub const EVENT: &str = "yap-side-button";
/// How long one ask of the dialog's listens (renewed while it asks).
const LISTEN_MS: u64 = 30_000;
/// An unbound side button pressed when the tip couldn't show (a meeting, a
/// dictation): asked again no sooner than this.
const RETRY_MS: u64 = 60_000;

/// Each fixed action as the Shortcuts page names it (`input_hook`'s order):
/// "Forward is set to Copy last dictation".
const LABELS: [&str; FIXED_COUNT] = [
    "Dictate",
    "Rewrite selected text",
    "Meeting notes",
    "Ask the Voice Assistant",
    "Read aloud",
    "Use what I said",
    "Snip to Chat",
    "Paste last dictation",
    "Copy last dictation",
    "Quick note",
    "Send",
    "Ask about the selection",
];

/// A side button's name: "Back", "Forward".
pub fn name(button: u32) -> &'static str {
    match button {
        BACK => "Back",
        FORWARD => "Forward",
        _ => "That button",
    }
}

/// A side button as a shortcut ("mouse:4").
pub fn spec(button: u32) -> String {
    format!("mouse:{button}")
}

/// What a side button does in a browser ("go back").
fn browser_verb(button: u32) -> &'static str {
    if button == BACK {
        "go back"
    } else {
        "go forward"
    }
}

/// What the dialog asks for: the button for each job (`None`: skipped),
/// push-to-talk or toggle, and whether a button another shortcut has moves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choice {
    pub dictate: Option<u32>,
    pub send: Option<u32>,
    /// Push-to-talk: hold to dictate, double-tap for hands-free. Else toggle.
    pub hold: bool,
    /// Move a button that's on another shortcut.
    pub take: bool,
}

/// A side button's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Dictate,
    Send,
}

impl Role {
    fn action(self) -> usize {
        match self {
            Role::Dictate => DICTATION,
            Role::Send => SEND,
        }
    }
}

/// What the setup does with a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Fate {
    /// On no shortcut yet: it goes on its job.
    New,
    /// On its job already.
    Already,
    /// On another shortcut, and not asked to move: left there.
    Taken,
    /// On another shortcut: moved to its job.
    Moved,
    /// Its job has four keys already: no room.
    Full,
    /// The dictation's only key: it stays there.
    Only,
}

impl Fate {
    /// The button does its job after the setup.
    fn sets(self) -> bool {
        matches!(self, Fate::New | Fate::Already | Fate::Moved)
    }
}

/// Where a button is bound now.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    /// Fixed action `action`'s first shortcut, or one of its more.
    Fixed { action: usize, more: bool },
    /// A language shortcut (its id).
    Language(String),
    /// A saved transform (its id).
    Transform(String),
}

impl Owner {
    fn action(&self) -> Option<usize> {
        match self {
            Owner::Fixed { action, .. } => Some(*action),
            _ => None,
        }
    }
}

/// One button in the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonPlan {
    pub button: u32,
    pub name: &'static str,
    pub role: Role,
    pub fate: Fate,
    /// The shortcut that has it now, as the page names it ("Copy last
    /// dictation", "the Polish transform").
    pub by: Option<String>,
    /// It's on a shortcut other than Dictate and Send: moving it needs the
    /// person's yes (`take`).
    pub asked: bool,
    /// What the dialog says about it, if anything.
    pub note: Option<String>,
}

/// A line naming one of the person's keys: `{key}` in `text` is `spec`'s
/// name, which the page writes (`fillHotkey`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyNote {
    pub text: String,
    pub spec: String,
}

/// What the setup would do ([`plan`]).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// "Back: hold to dictate, double-tap for hands-free · Forward: send".
    pub summary: String,
    pub buttons: Vec<ButtonPlan>,
    /// "Back won't go back in your browser while it's set to dictate." …
    pub browser: Vec<String>,
    /// The recording mode changing another dictation key too.
    pub mode_note: Option<KeyNote>,
    /// A button another shortcut has that `take` moves (the dialog's box).
    pub can_take: bool,
    /// Anything changes.
    pub changes: bool,
    /// The config after.
    #[serde(skip)]
    pub after: YapConfig,
}

/// The parts of a config the setup touches: what Undo puts back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shortcuts {
    firsts: Vec<String>,
    more: BTreeMap<String, Vec<String>>,
    mode: String,
    languages: BTreeMap<String, String>,
    transforms: BTreeMap<String, String>,
}

/// `cfg`'s shortcuts and recording mode (pure).
pub fn shortcuts(cfg: &YapConfig) -> Shortcuts {
    Shortcuts {
        firsts: (0..FIXED_COUNT).map(|i| first_spec(cfg, i).to_string()).collect(),
        more: cfg.more_hotkeys.clone(),
        mode: cfg.recording_mode.clone(),
        languages: cfg.language_shortcuts.iter().map(|l| (l.id.clone(), l.hotkey.clone())).collect(),
        transforms: cfg.transforms.iter().map(|t| (t.id.clone(), t.hotkey.clone())).collect(),
    }
}

// ---- where a button is (pure) -------------------------------------------------------------------

/// Every shortcut `button` is on in `cfg`.
fn owners(cfg: &YapConfig, button: u32) -> Vec<Owner> {
    let s = spec(button);
    let mut out = Vec::new();
    for action in 0..FIXED_COUNT {
        if clashes(first_spec(cfg, action), &s) {
            out.push(Owner::Fixed { action, more: false });
        }
        if more_specs(cfg, action).iter().any(|m| clashes(m, &s)) {
            out.push(Owner::Fixed { action, more: true });
        }
    }
    out.extend(cfg.language_shortcuts.iter().filter(|l| clashes(&l.hotkey, &s)).map(|l| Owner::Language(l.id.clone())));
    out.extend(cfg.transforms.iter().filter(|t| clashes(&t.hotkey, &s)).map(|t| Owner::Transform(t.id.clone())));
    out
}

/// A shortcut as the page names it.
fn owner_name(cfg: &YapConfig, owner: &Owner) -> String {
    match owner {
        Owner::Fixed { action, .. } => LABELS[*action].to_string(),
        Owner::Language(id) => cfg
            .language_shortcuts
            .iter()
            .find(|l| l.id == *id)
            .map(|l| format!("the {} shortcut", l.name()))
            .unwrap_or_else(|| "a language shortcut".into()),
        Owner::Transform(id) => cfg
            .transforms
            .iter()
            .find(|t| t.id == *id)
            .map(|t| format!("the {} transform", t.name()))
            .unwrap_or_else(|| "a transform".into()),
    }
}

/// Fixed action `action`'s keys, first and more.
fn keys(cfg: &YapConfig, action: usize) -> Vec<String> {
    let first = first_spec(cfg, action).trim();
    let mut out: Vec<String> = if first.is_empty() { Vec::new() } else { vec![first.to_string()] };
    out.extend(more_specs(cfg, action));
    out
}

/// The side button on fixed action `action`, if one is (Back first).
fn side_on(cfg: &YapConfig, action: usize) -> Option<u32> {
    [BACK, FORWARD].into_iter().find(|&b| keys(cfg, action).iter().any(|k| clashes(k, &spec(b))))
}

/// A side button is on some shortcut: the person knows them already.
pub fn in_use(cfg: &YapConfig) -> bool {
    [BACK, FORWARD].into_iter().any(|b| !owners(cfg, b).is_empty())
}

fn set_more(cfg: &mut YapConfig, action: usize, list: Vec<String>) {
    if list.is_empty() {
        cfg.more_hotkeys.remove(FIXED_KEYS[action]);
    } else {
        cfg.more_hotkeys.insert(FIXED_KEYS[action].to_string(), list);
    }
}

/// `button` off `owner` (a first shortcut taken away: the next moves up).
fn take_off(cfg: &mut YapConfig, button: u32, owner: &Owner) {
    let s = spec(button);
    match owner {
        Owner::Fixed { action, more: false } => {
            let mut rest = more_specs(cfg, *action);
            let next = if rest.is_empty() { String::new() } else { rest.remove(0) };
            set_first_spec(cfg, *action, next);
            set_more(cfg, *action, rest);
        }
        Owner::Fixed { action, more: true } => {
            let rest = more_specs(cfg, *action).into_iter().filter(|m| !clashes(m, &s)).collect();
            set_more(cfg, *action, rest);
        }
        Owner::Language(id) => {
            for l in cfg.language_shortcuts.iter_mut().filter(|l| l.id == *id && clashes(&l.hotkey, &s)) {
                l.hotkey.clear();
            }
        }
        Owner::Transform(id) => {
            for t in cfg.transforms.iter_mut().filter(|t| t.id == *id && clashes(&t.hotkey, &s)) {
                t.hotkey.clear();
            }
        }
    }
}

/// `button` onto fixed action `action`: its first shortcut when it has none,
/// else one more. False: no room (four keys already).
fn put_on(cfg: &mut YapConfig, button: u32, action: usize) -> bool {
    if first_spec(cfg, action).trim().is_empty() {
        set_first_spec(cfg, action, spec(button));
        return true;
    }
    let mut more = more_specs(cfg, action);
    if more.len() >= MORE {
        return false;
    }
    more.push(spec(button));
    set_more(cfg, action, more);
    true
}

// ---- the plan (pure) ----------------------------------------------------------------------------

/// What a button does, for the summary.
fn job(role: Role, button: u32, hold: bool) -> String {
    let b = name(button);
    match (role, hold) {
        (Role::Dictate, true) => format!("{b}: hold to dictate, double-tap for hands-free"),
        (Role::Dictate, false) => format!("{b}: press to start and stop dictating"),
        (Role::Send, _) => format!("{b}: send"),
    }
}

/// The summary of what the side buttons do (pure): "Back: hold to dictate,
/// double-tap for hands-free · Forward: send"; "" for neither.
pub fn summary(dictate: Option<u32>, send: Option<u32>, hold: bool) -> String {
    [dictate.map(|b| job(Role::Dictate, b, hold)), send.map(|b| job(Role::Send, b, hold))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The browser's lines (pure): a button doing a job won't do its own; one
/// on no shortcut still does.
fn browser_lines(cfg: &YapConfig, dictate: Option<u32>, send: Option<u32>) -> Vec<String> {
    let mut out = Vec::new();
    for (role, button) in [(Role::Dictate, dictate), (Role::Send, send)] {
        if let Some(b) = button {
            let job = if role == Role::Dictate { "dictate" } else { "send" };
            out.push(format!("{} won't {} in your browser while it's set to {job}.", name(b), browser_verb(b)));
        }
    }
    for b in [BACK, FORWARD] {
        if dictate != Some(b) && send != Some(b) && owners(cfg, b).is_empty() {
            out.push(format!("{} still goes {} in your browser.", name(b), &browser_verb(b)[3..]));
        }
    }
    out
}

/// What the dialog says about a button.
fn note(b: &ButtonPlan) -> Option<String> {
    let (n, job) = (b.name, LABELS[b.role.action()]);
    match b.fate {
        Fate::New => None,
        Fate::Already => Some(match b.role {
            Role::Dictate => format!("{n} dictates already."),
            Role::Send => format!("{n} sends already."),
        }),
        Fate::Taken => Some(format!("{n} is set to {}.", b.by.as_deref().unwrap_or("another shortcut"))),
        Fate::Moved => Some(format!("{n} moves from {} to {job}.", b.by.as_deref().unwrap_or("another shortcut"))),
        Fate::Full => Some(format!("{job} has four shortcuts already. Remove one to add {n}.")),
        Fate::Only => Some(format!("{n} is your only key for Dictate, so it stays there.")),
    }
}

/// What the setup would do to `cfg` for `choice` (pure): each button's fate
/// and the config after — see the module's notes. `Err`: a choice that
/// can't be (one button for both jobs, a button that isn't a side button).
pub fn plan(cfg: &YapConfig, choice: &Choice) -> Result<Plan, String> {
    let roles: Vec<(Role, u32)> = [(Role::Dictate, choice.dictate), (Role::Send, choice.send)]
        .into_iter()
        .filter_map(|(role, b)| b.map(|b| (role, b)))
        .collect();
    if let Some((_, b)) = roles.iter().find(|(_, b)| *b != BACK && *b != FORWARD) {
        return Err(format!("Not a side button: mouse button {b}"));
    }
    if choice.dictate.is_some() && choice.dictate == choice.send {
        return Err(format!("{} can't both dictate and send: pick the other button for one of them.", name(choice.dictate.unwrap_or(BACK))));
    }
    // A job found with no room, or the dictation left with no key: that
    // button's fate is settled, and the plan made again without it.
    let mut settled: Vec<(Role, Fate)> = Vec::new();
    loop {
        match attempt(cfg, choice, &roles, &settled) {
            Ok(plan) => return Ok(plan),
            Err(fixed) if !settled.contains(&fixed) => settled.push(fixed),
            Err(_) => return Err("The side buttons can't be set up that way".into()),
        }
    }
}

/// One go at the plan, with some fates `settled`; `Err`: one more settled.
fn attempt(cfg: &YapConfig, choice: &Choice, roles: &[(Role, u32)], settled: &[(Role, Fate)]) -> Result<Plan, (Role, Fate)> {
    let jobs: Vec<usize> = roles.iter().map(|(r, _)| r.action()).collect();
    let mut buttons = Vec::new();
    for &(role, button) in roles {
        let owners = owners(cfg, button);
        let mine = owners.iter().any(|o| o.action() == Some(role.action()));
        let others: Vec<&Owner> = owners.iter().filter(|o| o.action() != Some(role.action())).collect();
        // Between Dictate and Send themselves (a swap): no need to ask.
        let asked = others.iter().any(|o| !o.action().is_some_and(|a| jobs.contains(&a)));
        let fate = match settled.iter().find(|(r, _)| *r == role) {
            Some((_, fate)) => *fate,
            None if others.is_empty() && mine => Fate::Already,
            None if others.is_empty() => Fate::New,
            None if asked && !choice.take => Fate::Taken,
            None => Fate::Moved,
        };
        let by = others.first().map(|o| owner_name(cfg, o));
        buttons.push(ButtonPlan { button, name: name(button), role, fate, by, asked, note: None });
    }

    let mut after = cfg.clone();
    for b in buttons.iter().filter(|b| b.fate == Fate::Moved) {
        for o in owners(cfg, b.button).iter().filter(|o| o.action() != Some(b.role.action())) {
            take_off(&mut after, b.button, o);
        }
    }
    for b in buttons.iter().filter(|b| matches!(b.fate, Fate::New | Fate::Moved)) {
        if !put_on(&mut after, b.button, b.role.action()) {
            return Err((b.role, Fate::Full));
        }
    }
    // The dictation keeps a key: the button that left it stays.
    if keys(&after, DICTATION).is_empty() {
        if let Some(b) = buttons.iter().find(|b| b.fate == Fate::Moved && owners(cfg, b.button).iter().any(|o| o.action() == Some(DICTATION))) {
            return Err((b.role, Fate::Only));
        }
    }

    let set = |role: Role| buttons.iter().find(|b| b.role == role && b.fate.sets()).map(|b| b.button);
    let (dictate, send) = (set(Role::Dictate), set(Role::Send));
    let mode = if choice.hold { "pushToTalk" } else { "toggle" };
    let mut mode_note = None;
    if dictate.is_some() {
        if after.recording_mode != mode {
            // Another dictation key changes with it.
            mode_note = keys(&after, DICTATION).into_iter().find(|k| !clashes(k, &spec(BACK)) && !clashes(k, &spec(FORWARD))).map(|spec| KeyNote {
                text: if choice.hold {
                    "{key} changes too: hold it to dictate, double-tap for hands-free.".into()
                } else {
                    "{key} changes too: press it to start and stop dictating.".into()
                },
                spec,
            });
        }
        after.recording_mode = mode.to_string();
    }
    for b in &mut buttons {
        b.note = note(b);
    }
    let can_take = buttons.iter().any(|b| b.asked && matches!(b.fate, Fate::Taken | Fate::Moved));
    Ok(Plan {
        summary: summary(dictate, send, choice.hold),
        browser: browser_lines(&after, dictate, send),
        mode_note,
        can_take,
        changes: shortcuts(&after) != shortcuts(cfg),
        buttons,
        after,
    })
}

/// `before` put back where `now` is still as `after` left it (pure): each
/// shortcut, each list of more, the recording mode, each language
/// shortcut's and transform's key on its own — a change made since stays.
pub fn undo(now: &YapConfig, before: &Shortcuts, after: &Shortcuts) -> YapConfig {
    let mut out = now.clone();
    let cur = shortcuts(now);
    for i in 0..FIXED_COUNT {
        if cur.firsts.get(i) == after.firsts.get(i) && before.firsts.get(i) != after.firsts.get(i) {
            set_first_spec(&mut out, i, before.firsts.get(i).cloned().unwrap_or_default());
        }
    }
    let lists: std::collections::BTreeSet<&String> = before.more.keys().chain(after.more.keys()).collect();
    for key in lists {
        let (c, a, b) = (cur.more.get(key), after.more.get(key), before.more.get(key));
        if c == a && a != b {
            match b {
                Some(list) => out.more_hotkeys.insert(key.clone(), list.clone()),
                None => out.more_hotkeys.remove(key),
            };
        }
    }
    if cur.mode == after.mode && before.mode != after.mode {
        out.recording_mode = before.mode.clone();
    }
    for l in out.language_shortcuts.iter_mut() {
        if let (Some(a), Some(b)) = (after.languages.get(&l.id), before.languages.get(&l.id)) {
            if l.hotkey == *a && a != b {
                l.hotkey = b.clone();
            }
        }
    }
    for t in out.transforms.iter_mut() {
        if let (Some(a), Some(b)) = (after.transforms.get(&t.id), before.transforms.get(&t.id)) {
            if t.hotkey == *a && a != b {
                t.hotkey = b.clone();
            }
        }
    }
    out
}

/// The side buttons taken off Dictate and Send (pure); the dictation's only
/// key stays.
pub fn remove(cfg: &YapConfig) -> YapConfig {
    let mut out = cfg.clone();
    for action in [DICTATION, SEND] {
        for button in [BACK, FORWARD] {
            for o in owners(&out, button).into_iter().filter(|o| o.action() == Some(action)) {
                if action == DICTATION && keys(&out, DICTATION).len() <= 1 {
                    continue;
                }
                take_off(&mut out, button, &o);
            }
        }
    }
    out
}

/// What the side buttons do now: the Shortcuts page's row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub dictate: Option<u32>,
    pub send: Option<u32>,
    pub hold: bool,
    /// [`summary`]; "" when neither does anything.
    pub summary: String,
    pub browser: Vec<String>,
    /// The last setup (or removal) can be undone.
    pub undo: bool,
}

/// What the side buttons do in `cfg` (pure; `undo` false).
pub fn state_of(cfg: &YapConfig) -> State {
    let (dictate, send) = (side_on(cfg, DICTATION), side_on(cfg, SEND).filter(|b| side_on(cfg, DICTATION) != Some(*b)));
    let hold = cfg.recording_mode == "pushToTalk";
    State {
        dictate,
        send,
        hold,
        summary: summary(dictate, send, hold),
        browser: if dictate.is_some() || send.is_some() { browser_lines(cfg, dictate, send) } else { Vec::new() },
        undo: false,
    }
}

// ---- the app ------------------------------------------------------------------------------------

/// The last setup's (or removal's) shortcuts, before and after: Undo.
static LAST: std::sync::Mutex<Option<(Shortcuts, Shortcuts)>> = std::sync::Mutex::new(None);
/// When the tip was last asked for (`gesture::now_ms`; 0: never).
static TIP_ASKED: AtomicU64 = AtomicU64::new(0);

fn last() -> std::sync::MutexGuard<'static, Option<(Shortcuts, Shortcuts)>> {
    LAST.lock().unwrap_or_else(|p| p.into_inner())
}

fn state_now() -> State {
    let cfg = crate::config::load();
    let mut state = state_of(&cfg);
    state.undo = last().as_ref().is_some_and(|(before, after)| shortcuts(&undo(&cfg, before, after)) != shortcuts(&cfg));
    state
}

/// The input hook heard `button` for the setup: the page is told.
pub fn heard(app: &AppHandle, button: u32) {
    use tauri::Emitter;
    info!(button = name(button), "Side buttons: heard one for the setup");
    let _ = app.emit(EVENT, serde_json::json!({ "button": button, "name": name(button) }));
}

/// An unbound side button was pressed (the input hook, until the tip has
/// had its turn): "Use your mouse's side buttons?" once, unless it showed
/// already or a side button is on a shortcut (then the hook stops saying).
pub fn seen(app: &AppHandle, button: u32) {
    let now = crate::dictation::gesture::now_ms().max(1);
    let asked = TIP_ASKED.load(Ordering::SeqCst);
    if asked != 0 && now.saturating_sub(asked) < RETRY_MS {
        return;
    }
    TIP_ASKED.store(now, Ordering::SeqCst);
    let shown = crate::dictation::coaching::record().features.contains_key(crate::dictation::tips::Feature::SideButtons.key());
    if shown || in_use(&crate::config::load()) {
        crate::dictation::input_hook::side_tip_done(true);
        return;
    }
    tracing::debug!(button = name(button), "Side buttons: an unbound one pressed");
    crate::dictation::tips::side_buttons(app);
}

/// The tip's Set up: Settings → Shortcuts with the dialog open.
pub fn open_setup(app: &AppHandle) {
    use tauri::Emitter;
    if let Err(e) = crate::shell::window::show_settings(app) {
        tracing::warn!("Side buttons: couldn't show the main window ({e})");
    }
    let _ = app.emit("yap-open-settings", "shortcuts#side-buttons-setup");
}

/// Test mode: the tip wanted again (`tips::e2e_feature_tips`).
#[cfg(debug_assertions)]
pub fn forget_tip() {
    TIP_ASKED.store(0, Ordering::SeqCst);
    crate::dictation::input_hook::side_tip_done(false);
}

/// The dialog listens for a side button (`on`), or stops: renewed while it
/// asks ([`LISTEN_MS`] each time) — only while the main window is on
/// screen, so a window hidden with the dialog open stops swallowing Back
/// within [`LISTEN_MS`].
#[tauri::command]
pub async fn side_buttons_listen(app: AppHandle, on: bool) {
    let on = on && crate::notices::notice::window_view(&app).0;
    tracing::debug!(on, "Side buttons: listening");
    crate::dictation::input_hook::listen_side_buttons(if on { LISTEN_MS } else { 0 });
}

/// What setting up `choice` would do, on the config as saved.
#[tauri::command]
pub fn side_buttons_preview(choice: Choice) -> Result<Plan, String> {
    plan(&crate::config::load(), &choice)
}

/// What the side buttons do now (the Shortcuts page's row).
#[tauri::command]
pub fn side_buttons_state() -> State {
    state_now()
}

/// Set them up: `choice`'s plan saved and applied in one step (the hook's
/// bindings with it), kept for Undo.
#[tauri::command]
pub fn side_buttons_apply(app: AppHandle, choice: Choice) -> Result<State, String> {
    let mut kept = None;
    crate::shell::settings::change_config(&app, |c| {
        let plan = plan(c, &choice)?;
        if plan.changes {
            kept = Some((shortcuts(c), shortcuts(&plan.after)));
            *c = plan.after;
        }
        Ok(())
    })?;
    if let Some(kept) = kept {
        *last() = Some(kept);
    }
    let state = state_now();
    info!(
        dictate = state.dictate.map(name),
        send = state.send.map(name),
        hold = state.hold,
        "Side buttons: set up"
    );
    if state.dictate.is_some() || state.send.is_some() {
        crate::dictation::input_hook::side_tip_done(true);
    }
    Ok(state)
}

/// Undo the last setup (or removal): what it changed put back, where nothing
/// has changed it since.
#[tauri::command]
pub fn side_buttons_undo(app: AppHandle) -> Result<State, String> {
    let Some((before, after)) = last().take() else {
        return Ok(state_now());
    };
    crate::shell::settings::change_config(&app, |c| {
        *c = undo(c, &before, &after);
        Ok(())
    })?;
    info!("Side buttons: undone");
    Ok(state_now())
}

/// Take the side buttons off Dictate and Send (Undo puts them back).
#[tauri::command]
pub fn side_buttons_remove(app: AppHandle) -> Result<State, String> {
    let mut kept = None;
    crate::shell::settings::change_config(&app, |c| {
        let after = remove(c);
        if shortcuts(&after) != shortcuts(c) {
            kept = Some((shortcuts(c), shortcuts(&after)));
            *c = after;
        }
        Ok(())
    })?;
    if let Some(kept) = kept {
        *last() = Some(kept);
    }
    info!("Side buttons: removed from Dictate and Send");
    Ok(state_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> YapConfig {
        YapConfig {
            hotkey: "kb:120".into(),
            send_hotkey: String::new(),
            recording_mode: "toggle".into(),
            more_hotkeys: BTreeMap::new(),
            ..Default::default()
        }
    }

    fn choice(dictate: Option<u32>, send: Option<u32>, hold: bool) -> Choice {
        Choice { dictate, send, hold, take: false }
    }

    fn fates(p: &Plan) -> Vec<(u32, Fate)> {
        p.buttons.iter().map(|b| (b.button, b.fate)).collect()
    }

    #[test]
    fn back_dictates_and_forward_sends_beside_the_keyboard_shortcut() {
        let before = cfg();
        let p = plan(&before, &choice(Some(BACK), Some(FORWARD), true)).unwrap();
        assert_eq!(fates(&p), vec![(BACK, Fate::New), (FORWARD, Fate::New)]);
        // F9 stays the dictation's first shortcut; Back is one more.
        assert_eq!(p.after.hotkey, "kb:120");
        assert_eq!(p.after.more_hotkeys.get("hotkey"), Some(&vec!["mouse:4".to_string()]));
        // Send had none: Forward is its first.
        assert_eq!(p.after.send_hotkey, "mouse:5");
        assert!(!p.after.more_hotkeys.contains_key("sendHotkey"));
        assert_eq!(p.after.recording_mode, "pushToTalk");
        assert!(p.changes);
        assert_eq!(p.summary, "Back: hold to dictate, double-tap for hands-free · Forward: send");
        assert_eq!(
            p.browser,
            vec![
                "Back won't go back in your browser while it's set to dictate.".to_string(),
                "Forward won't go forward in your browser while it's set to send.".to_string(),
            ]
        );
        // F9 turned push-to-talk too: said, with the key for the page to name.
        let note = p.mode_note.unwrap();
        assert_eq!(note.spec, "kb:120");
        assert_eq!(note.text, "{key} changes too: hold it to dictate, double-tap for hands-free.");
        // The hook binds both as the clash rules allow.
        assert_eq!(crate::dictation::input_hook::bound_more(&p.after)[DICTATION], vec!["mouse:4".to_string()]);
        assert_eq!(crate::dictation::input_hook::bound_first(&p.after, SEND), "mouse:5");
    }

    #[test]
    fn toggle_keeps_the_mode_and_says_press_to_start_and_stop() {
        let p = plan(&cfg(), &choice(Some(BACK), Some(FORWARD), false)).unwrap();
        assert_eq!(p.after.recording_mode, "toggle");
        assert!(p.mode_note.is_none(), "F9 doesn't change");
        assert_eq!(p.summary, "Back: press to start and stop dictating · Forward: send");
        // From push-to-talk back to toggle, F9 changes too.
        let mut ptt = cfg();
        ptt.recording_mode = "pushToTalk".into();
        let p = plan(&ptt, &choice(Some(FORWARD), None, false)).unwrap();
        assert_eq!(p.mode_note.unwrap().text, "{key} changes too: press it to start and stop dictating.");
    }

    #[test]
    fn swapped_and_skipped() {
        // Swapped: Forward dictates, Back sends.
        let p = plan(&cfg(), &choice(Some(FORWARD), Some(BACK), true)).unwrap();
        assert_eq!(p.after.more_hotkeys.get("hotkey"), Some(&vec!["mouse:5".to_string()]));
        assert_eq!(p.after.send_hotkey, "mouse:4");
        assert_eq!(p.summary, "Forward: hold to dictate, double-tap for hands-free · Back: send");
        // Send skipped: Forward is left alone, and still goes forward.
        let p = plan(&cfg(), &choice(Some(BACK), None, true)).unwrap();
        assert_eq!(p.after.send_hotkey, "");
        assert_eq!(p.summary, "Back: hold to dictate, double-tap for hands-free");
        assert_eq!(p.browser.last().unwrap(), "Forward still goes forward in your browser.");
        // Dictate skipped: the recording mode is the dictation's, untouched.
        let p = plan(&cfg(), &choice(None, Some(FORWARD), true)).unwrap();
        assert_eq!(p.after.recording_mode, "toggle");
        assert!(!p.after.more_hotkeys.contains_key("hotkey"));
        assert_eq!(p.summary, "Forward: send");
        assert_eq!(p.browser.last().unwrap(), "Back still goes back in your browser.");
        // Both skipped: nothing at all.
        let p = plan(&cfg(), &choice(None, None, true)).unwrap();
        assert!(!p.changes && p.summary.is_empty() && p.buttons.is_empty());
        // One button for both: refused.
        assert!(plan(&cfg(), &choice(Some(BACK), Some(BACK), true)).is_err());
        assert!(plan(&cfg(), &choice(Some(3), None, true)).is_err());
    }

    #[test]
    fn set_up_again_changes_nothing_and_swapping_moves_them_without_asking() {
        let once = plan(&cfg(), &choice(Some(BACK), Some(FORWARD), true)).unwrap().after;
        let again = plan(&once, &choice(Some(BACK), Some(FORWARD), true)).unwrap();
        assert_eq!(fates(&again), vec![(BACK, Fate::Already), (FORWARD, Fate::Already)]);
        assert!(!again.changes);
        assert_eq!(again.buttons[0].note.as_deref(), Some("Back dictates already."));
        // Swap them: each moves from the other job, no question asked.
        let swap = plan(&once, &choice(Some(FORWARD), Some(BACK), true)).unwrap();
        assert_eq!(fates(&swap), vec![(FORWARD, Fate::Moved), (BACK, Fate::Moved)]);
        assert!(!swap.can_take);
        assert_eq!(swap.buttons[0].note.as_deref(), Some("Forward moves from Send to Dictate."));
        assert_eq!(swap.after.hotkey, "kb:120");
        assert_eq!(swap.after.more_hotkeys.get("hotkey"), Some(&vec!["mouse:5".to_string()]));
        assert_eq!(swap.after.send_hotkey, "mouse:4");
    }

    #[test]
    fn a_button_another_shortcut_has_is_named_and_moved_only_when_asked() {
        let mut c = cfg();
        c.copy_last_hotkey = "mouse:5".into();
        c.transforms = vec![crate::dictation::transforms::Transform { id: "polish".into(), builtin: "polish".into(), hotkey: "mouse:4".into(), ..Default::default() }];
        let p = plan(&c, &choice(Some(BACK), Some(FORWARD), false)).unwrap();
        assert_eq!(fates(&p), vec![(BACK, Fate::Taken), (FORWARD, Fate::Taken)]);
        assert!(p.can_take);
        assert_eq!(p.buttons[0].by.as_deref(), Some("the Polish transform"));
        assert_eq!(p.buttons[1].note.as_deref(), Some("Forward is set to Copy last dictation."));
        assert!(!p.changes, "nothing moves unasked");
        assert_eq!(p.summary, "");
        assert!(p.browser.is_empty());
        // Asked: both move, their old shortcuts left without them.
        let p = plan(&c, &Choice { take: true, ..choice(Some(BACK), Some(FORWARD), false) }).unwrap();
        assert_eq!(fates(&p), vec![(BACK, Fate::Moved), (FORWARD, Fate::Moved)]);
        assert_eq!(p.buttons[1].note.as_deref(), Some("Forward moves from Copy last dictation to Send."));
        assert!(p.can_take);
        assert_eq!(p.after.copy_last_hotkey, "");
        assert_eq!(p.after.transforms[0].hotkey, "");
        assert_eq!(p.after.send_hotkey, "mouse:5");
        // A language shortcut's key, moved the same way.
        let mut c = cfg();
        c.language_shortcuts = vec![crate::dictation::languages::LanguageShortcut { id: "l-fr".into(), language: "fr".into(), translate_to: String::new(), hotkey: "mouse:4".into() }];
        let p = plan(&c, &Choice { take: true, ..choice(Some(BACK), None, true) }).unwrap();
        assert_eq!(p.buttons[0].by.as_deref(), Some("the French shortcut"));
        assert_eq!(p.after.language_shortcuts[0].hotkey, "");
        assert_eq!(p.after.more_hotkeys.get("hotkey"), Some(&vec!["mouse:4".to_string()]));
        // An extra key of another action: moved out of its list.
        let mut c = cfg();
        c.read_hotkey = "kb:121".into();
        c.more_hotkeys.insert("readHotkey".into(), vec!["mouse:5".into(), "kb:122".into()]);
        let p = plan(&c, &Choice { take: true, ..choice(None, Some(FORWARD), true) }).unwrap();
        assert_eq!(p.after.more_hotkeys.get("readHotkey"), Some(&vec!["kb:122".to_string()]));
        assert_eq!(p.after.read_hotkey, "kb:121");
        // Another action's first: its next moves up.
        let mut c = cfg();
        c.read_hotkey = "mouse:5".into();
        c.more_hotkeys.insert("readHotkey".into(), vec!["kb:122".into()]);
        let p = plan(&c, &Choice { take: true, ..choice(None, Some(FORWARD), true) }).unwrap();
        assert_eq!(p.after.read_hotkey, "kb:122");
        assert!(!p.after.more_hotkeys.contains_key("readHotkey"));
    }

    #[test]
    fn no_room_on_four_keys_and_the_dictations_only_key_stays() {
        let mut c = cfg();
        c.more_hotkeys.insert("hotkey".into(), vec!["kb:121".into(), "kb:122".into(), "kb:123".into()]);
        let p = plan(&c, &choice(Some(BACK), Some(FORWARD), true)).unwrap();
        assert_eq!(fates(&p), vec![(BACK, Fate::Full), (FORWARD, Fate::New)]);
        assert_eq!(p.buttons[0].note.as_deref(), Some("Dictate has four shortcuts already. Remove one to add Back."));
        assert_eq!(p.after.more_hotkeys.get("hotkey").unwrap().len(), 3);
        assert_eq!(p.after.recording_mode, "toggle", "no dictation button: the mode stays");
        assert_eq!(p.summary, "Forward: send");
        // Back is the dictation's only key: Send can't have it.
        let mut c = cfg();
        c.hotkey = "mouse:4".into();
        let p = plan(&c, &Choice { take: true, ..choice(None, Some(BACK), true) }).unwrap();
        assert_eq!(fates(&p), vec![(BACK, Fate::Only)]);
        assert_eq!(p.after.hotkey, "mouse:4");
        assert!(!p.changes);
        // …unless the dictation gets Forward in its place (a swap).
        let p = plan(&c, &choice(Some(FORWARD), Some(BACK), true)).unwrap();
        assert_eq!(fates(&p), vec![(FORWARD, Fate::New), (BACK, Fate::Moved)]);
        assert_eq!(p.after.hotkey, "mouse:5");
        assert_eq!(p.after.send_hotkey, "mouse:4");
        assert!(p.mode_note.is_none(), "no other dictation key to name");
    }

    #[test]
    fn undo_puts_back_only_what_nothing_changed_since() {
        let mut c = cfg();
        c.copy_last_hotkey = "mouse:5".into();
        let p = plan(&c, &Choice { take: true, ..choice(Some(BACK), Some(FORWARD), true) }).unwrap();
        let (before, after) = (shortcuts(&c), shortcuts(&p.after));
        let back = undo(&p.after, &before, &after);
        assert_eq!(shortcuts(&back), before);
        // A change made since (the meeting shortcut, the mode) stays.
        let mut later = p.after.clone();
        later.meeting_hotkey = "kb:ctrl+alt+77".into();
        later.recording_mode = "toggle".into();
        let back = undo(&later, &before, &after);
        assert_eq!(back.meeting_hotkey, "kb:ctrl+alt+77");
        assert_eq!(back.copy_last_hotkey, "mouse:5");
        assert!(!back.more_hotkeys.contains_key("hotkey"));
        assert_eq!(back.send_hotkey, "");
        // Undo of nothing changes nothing.
        assert_eq!(shortcuts(&undo(&c, &before, &before)), before);
    }

    #[test]
    fn remove_takes_the_buttons_off_dictate_and_send_only() {
        let mut c = plan(&cfg(), &choice(Some(BACK), Some(FORWARD), true)).unwrap().after;
        c.more_hotkeys.insert("sendHotkey".into(), vec!["kb:130".into()]);
        c.read_hotkey = "mouse:3".into();
        let out = remove(&c);
        assert_eq!(out.hotkey, "kb:120");
        assert!(!out.more_hotkeys.contains_key("hotkey"));
        assert_eq!(out.send_hotkey, "kb:130", "Send's keyboard key moves up");
        assert_eq!(out.read_hotkey, "mouse:3");
        assert_eq!(out.recording_mode, "pushToTalk");
        assert_eq!(state_of(&out), State { hold: true, ..State::default() });
        // The dictation's only key stays.
        let mut only = cfg();
        only.hotkey = "mouse:4".into();
        assert_eq!(remove(&only).hotkey, "mouse:4");
    }

    #[test]
    fn the_state_says_what_they_do() {
        assert_eq!(state_of(&cfg()), State::default());
        let c = plan(&cfg(), &choice(Some(BACK), Some(FORWARD), true)).unwrap().after;
        let s = state_of(&c);
        assert_eq!((s.dictate, s.send, s.hold), (Some(BACK), Some(FORWARD), true));
        assert_eq!(s.summary, "Back: hold to dictate, double-tap for hands-free · Forward: send");
        assert_eq!(s.browser.len(), 2);
        assert!(in_use(&c) && !in_use(&cfg()));
        // A legacy spelling of a side button counts.
        let mut legacy = cfg();
        legacy.send_hotkey = "MouseButton5".into();
        assert_eq!(state_of(&legacy).send, Some(FORWARD));
    }

    #[test]
    fn the_choice_reads_from_the_page() {
        let c: Choice = serde_json::from_str(r#"{"dictate":4,"send":null,"hold":true}"#).unwrap();
        assert_eq!(c, Choice { dictate: Some(4), send: None, hold: true, take: false });
        let p = plan(&cfg(), &c).unwrap();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["buttons"][0]["fate"], "new");
        assert_eq!(json["buttons"][0]["role"], "dictate");
        assert!(json.get("after").is_none());
        assert_eq!(json["modeNote"]["spec"], "kb:120");
    }
}
