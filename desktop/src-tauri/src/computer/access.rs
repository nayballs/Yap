//! **"May Yap's AI use these apps?"** (`request_access`, the desktop
//! contract's permission dialog): one card in the thread listing every app
//! asked for — its icon, how far it may be used ([`super::grants::Tier`]) —
//! the model's reason, and the extras it asked for (reading or writing the
//! clipboard, system shortcuts such as Alt+Tab), each a box the person may
//! untick. Allow lets the whole set in, Don't lets none.
//!
//! - **One card, Chat's own**: it goes through the cards that ask first
//!   (`chat/pages/act/confirm.rs`, act [`ACT`]), so it shows on the answer
//!   being written, comes back answered with the conversation
//!   (`ChatMessage::confirms`), and a ■ Stop drops it (a no).
//! - **Never offered**: Yap itself, a password manager, an app Yap can't tell
//!   the windows of, a name the Start menu doesn't have — listed on the card
//!   and in the result as not allowed, with why.
//! - **A Code chat in Bypass permissions** (nothing asks there, the
//!   maintainer's rule): the apps are allowed at once, the card shown already
//!   answered so the person still sees what was let in.
//! - **Nobody to ask** (an evaluation, a turn no page watches): refused; the
//!   model is told to ask in words.
//!
//! The log has the card's id, how many apps and how it ended, never an app's
//! name or the reason.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::grants::{self, Granted, Tier};
use crate::apps::index::App;
use crate::chat::pages::act::confirm::{self, Card, Reply};
use crate::chat::stream::Event;
use crate::config::AppNickname;

/// The card's act (`confirm::Card::act`).
pub const ACT: &str = "access";
/// How long the card waits: a few apps to read through, well inside a plan
/// run's tool timeout.
pub const WAIT: Duration = Duration::from_secs(300);
/// Apps one request may name.
pub const APPS_MAX: usize = 20;
/// The reason's most characters.
const REASON_MAX: usize = 300;

/// One app on the card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccessApp {
    pub name: String,
    /// Its id in Yap's app index (its icon: `apps_icons`).
    pub id: String,
    /// "read" | "click" | "full".
    pub tier: String,
}

/// An app asked for that isn't allowed, and why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeniedApp {
    pub name: String,
    pub why: String,
}

/// **An access request's part** of a card that asks (`confirm::Card::access`,
/// act [`ACT`]): the apps, why, the extras asked for — and once answered,
/// the extras given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccessCard {
    pub apps: Vec<AccessApp>,
    pub reason: String,
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub system_key_combos: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub denied: Vec<DeniedApp>,
    /// Allowed without asking: a Code chat in Bypass permissions.
    pub bypass: bool,
}

/// What the call asks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub apps: Vec<String>,
    pub reason: String,
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub system_key_combos: bool,
}

/// `s` on one line, at most `max` characters, "…" when cut. Pure.
fn one_line(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// **The call's arguments**: the apps named (trimmed, each once, at most
/// [`APPS_MAX`]), the reason, the extras. Pure.
pub fn request_of(args: &Value) -> Request {
    let mut apps: Vec<String> = Vec::new();
    let named: Vec<String> = match &args["apps"] {
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Value::String(s) => s.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    };
    for n in named {
        let n = one_line(&n, 80);
        if !n.is_empty() && !apps.iter().any(|a| a.eq_ignore_ascii_case(&n)) && apps.len() < APPS_MAX {
            apps.push(n);
        }
    }
    let flag = |k: &str| args[k].as_bool().unwrap_or(false);
    Request {
        apps,
        reason: one_line(args["reason"].as_str().unwrap_or(""), REASON_MAX),
        clipboard_read: flag("clipboardRead"),
        clipboard_write: flag("clipboardWrite"),
        system_key_combos: flag("systemKeyCombos"),
    }
}

/// **The apps the names stand for** among `apps` (the Start menu's): each
/// one found, or not allowed with why — not in the Start menu, Yap itself, a
/// password manager, an app whose windows Yap can't tell. Pure.
pub fn resolve(names: &[String], apps: &[App], nicknames: &[AppNickname], own_exe: &str) -> (Vec<Granted>, Vec<DeniedApp>) {
    use crate::apps::matching::{find, Found};
    let mut found: Vec<Granted> = Vec::new();
    let mut denied = Vec::new();
    for name in names {
        let app = match find(apps, name, nicknames, &[]) {
            Found::One(a) | Found::Blocked(a) => a,
            Found::Several(v) => v.iter().find(|a| a.name.eq_ignore_ascii_case(name)).cloned().unwrap_or_else(|| v[0].clone()),
            Found::Nothing(close) => {
                let why = match close.first() {
                    Some(c) => format!("Not in the Start menu (the closest is \"{}\").", c.name),
                    None => "Not in the Start menu.".to_string(),
                };
                denied.push(DeniedApp { name: name.clone(), why });
                continue;
            }
        };
        if let Some(why) = grants::refused(&app, own_exe) {
            denied.push(DeniedApp { name: app.name.clone(), why: why.into() });
            continue;
        }
        let g = Granted::of(&app);
        if !g.knows_its_windows() {
            denied.push(DeniedApp { name: app.name.clone(), why: "Yap can't tell this app's windows apart from others'.".into() });
            continue;
        }
        if !found.iter().any(|f| f.id == g.id) {
            found.push(g);
        }
    }
    (found, denied)
}

/// The card's app line for each allowed app. Pure.
fn card_apps(apps: &[Granted]) -> Vec<AccessApp> {
    apps.iter().map(|g| AccessApp { name: g.name.clone(), id: g.id.clone(), tier: g.tier.as_str().into() }).collect()
}

/// **How the call ended, for the model.** `allowed`: the apps let in
/// (empty: refused, timed out or nobody to ask, `state` says which). Pure.
pub fn result_of(state: &str, allowed: &[Granted], denied: &[DeniedApp], every: &grants::Session) -> Value {
    let tier_note = |t: Tier| match t {
        Tier::Read => "read only: seen in screenshots, never clicked or typed in (use the browser_* tools for web pages)",
        Tier::Click => "clicks only: a left click and scrolling, never typing or keys",
        Tier::Full => "full",
    };
    let granted: Vec<Value> = every.apps.iter().map(|g| json!({ "name": g.name, "id": g.id, "tier": g.tier.as_str(), "means": tier_note(g.tier) })).collect();
    let denied_v: Vec<Value> = denied.iter().map(|d| json!({ "name": d.name, "why": d.why })).collect();
    let data = json!({
        "granted": granted,
        "denied": denied_v,
        "clipboardRead": every.clipboard_read,
        "clipboardWrite": every.clipboard_write,
        "systemKeyCombos": every.system_keys,
        "screenshotFiltering": "mask",
    });
    let names = |v: &[Granted]| v.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", ");
    match state {
        "allowed" => json!({
            "success": true,
            "data": data,
            "note": "Allowed. Take a screenshot to see their windows; anything else on screen is masked.",
            "displayText": format!("Allowed to use {}", names(allowed)),
        }),
        "already" => json!({ "success": true, "data": data, "note": "Already allowed in this chat.", "displayText": "Apps already allowed" }),
        "timedOut" => json!({ "success": false, "error": format!("Nobody answered within {} minutes: nothing was allowed. Ask the user in words whether they want you to use those apps.", WAIT.as_secs() / 60), "data": data, "displayText": "Not allowed: no answer" }),
        "none" => json!({ "success": false, "error": "None of those apps can be allowed (see denied). Say why in a few words.", "data": data, "displayText": "Not allowed" }),
        _ => json!({ "success": false, "error": "The user didn't allow these apps. Don't ask again this turn; say what you'd have done, and they can do it.", "data": data, "displayText": "Not allowed to use those apps" }),
    }
}

/// Nobody can be asked (`why`). Pure.
fn nobody(why: &str) -> Value {
    json!({ "success": false, "error": format!("{why} Ask the user in words instead whether you may use those apps."), "data": Value::Null, "displayText": "Didn't ask to use apps: no card here" })
}

/// What a reply lets in: allowed, and the extras kept ticked. Pure.
fn settle(reply: Option<Reply>, asked: &Request) -> (&'static str, bool, bool, bool) {
    match reply {
        None => ("timedOut", false, false, false),
        Some(Reply::Picked(v)) if v["allow"].as_bool() == Some(true) => {
            // An extra is given only where it was asked for and left ticked.
            let kept = |k: &str, was: bool| was && v[k].as_bool().unwrap_or(true);
            ("allowed", kept("clipboardRead", asked.clipboard_read), kept("clipboardWrite", asked.clipboard_write), kept("systemKeyCombos", asked.system_key_combos))
        }
        Some(Reply::Once | Reply::Always) => ("allowed", asked.clipboard_read, asked.clipboard_write, asked.system_key_combos),
        Some(_) => ("refused", false, false, false),
    }
}

/// What an app's tier lets Yap do, in a few words.
fn tier_words(tier: &str) -> &'static str {
    match tier {
        "read" => "looks only",
        "click" => "clicks only",
        _ => "clicks and typing",
    }
}

/// The apps asked for, as a list said out loud ("Notepad and Paint"). Pure.
fn said_list(apps: &[AccessApp]) -> String {
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    match names.as_slice() {
        [] => "your apps".into(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// **The line Yap says** for an access card asked out loud (the Voice
/// Assistant from any app; `spoken`: a spoken yes may answer it): "Let me
/// use Notepad and Paint, to write your shopping list? Say yes, or press
/// Allow." Pure.
pub fn spoken_line(a: &AccessCard, spoken: bool) -> String {
    let why = a.reason.trim().trim_end_matches('.');
    let why = if why.is_empty() { String::new() } else { format!(", {}", lower_first(why)) };
    let ending = if spoken { " Say yes, or press Allow." } else { " It needs a click: press Allow on the Yap bar." };
    format!("Let me use {}{why}?{ending}", said_list(&a.apps))
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        // ("To write…" reads on as "to write…"; a name stays as it is.)
        Some(f) if c.clone().next().is_some_and(char::is_lowercase) => f.to_lowercase().chain(c).collect(),
        Some(f) => std::iter::once(f).chain(c).collect(),
        None => String::new(),
    }
}

/// **The bar's card** for an access card asked out loud: "Let Yap use your
/// apps?", each app with what it lets Yap do, the reason, the extras;
/// **Allow** and **Don't**. Pure.
pub fn bar_card(id: u64, a: &AccessCard, spoken: bool) -> crate::bar::Card {
    let mut body: Vec<String> = a.apps.iter().map(|app| format!("{} · {}", app.name, tier_words(&app.tier))).collect();
    if !a.reason.trim().is_empty() {
        body.push(a.reason.trim().to_string());
    }
    let extras: Vec<&str> = [(a.clipboard_read, "read the clipboard"), (a.clipboard_write, "write the clipboard"), (a.system_key_combos, "use system shortcuts")].iter().filter(|(on, _)| *on).map(|(_, w)| *w).collect();
    if !extras.is_empty() {
        body.push(format!("Also: {}.", extras.join(", ")));
    }
    body.push("Esc or your own mouse stops it.".into());
    let mut card = crate::chat::assistant::spoken_ok::bar_card(id, "Let Yap use your apps?".into(), body.join("\n"), spoken);
    card.primary = Some(crate::bar::CardAction::new(format!("allow:{id}"), "Allow"));
    card
}

/// **`request_access`** for a chat (`ctx`): resolve the apps, ask on the
/// card (or not, in Bypass), and keep what's allowed in the chat's session.
pub async fn request(args: &Value, ctx: &super::Ctx<'_>) -> Value {
    let asked = request_of(args);
    if asked.apps.is_empty() {
        return json!({ "success": false, "error": "request_access needs apps: the Start menu's names of the apps you need, and a reason.", "data": Value::Null, "displayText": "Didn't ask: no apps named" });
    }
    let cfg = crate::config::load();
    let names = asked.apps.clone();
    let nicknames = cfg.app_nicknames.clone();
    let own = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let look_up = move || {
        let apps = crate::apps::index::apps(false);
        resolve(&names, &apps, &nicknames, &own)
    };
    // (A unit test's apps are its own thread's.)
    let (found, denied) = if cfg!(test) { look_up() } else { tauri::async_runtime::spawn_blocking(look_up).await.unwrap_or_default() };
    let conversation = ctx.conversation;
    let (new, extras_new) = grants::with(conversation, |s| {
        let new: Vec<Granted> = found.iter().filter(|g| !s.apps.iter().any(|a| a.id == g.id)).cloned().collect();
        let extras = (asked.clipboard_read && !s.clipboard_read) || (asked.clipboard_write && !s.clipboard_write) || (asked.system_key_combos && !s.system_keys);
        (new, extras)
    });
    let session_now = || grants::with(conversation, |s| s.allowed());
    if new.is_empty() && !extras_new {
        let state = if found.is_empty() { "none" } else { "already" };
        return result_of(state, &[], &denied, &session_now());
    }
    let card = |id: u64, state: &str, cr: bool, cw: bool, sk: bool, bypass: bool| {
        Event::Confirm(Card {
            id,
            label: new.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", "),
            site: String::new(),
            does: ACT.into(),
            state: state.into(),
            act: ACT.into(),
            access: Some(Box::new(AccessCard { apps: card_apps(&new), reason: asked.reason.clone(), clipboard_read: cr, clipboard_write: cw, system_key_combos: sk, denied: denied.clone(), bypass })),
            ..Default::default()
        })
    };
    // A Code chat in Bypass permissions: nothing asks; allowed at once, shown answered.
    if ctx.unattended {
        grants::with(conversation, |s| s.grant(new.clone(), asked.clipboard_read, asked.clipboard_write, asked.system_key_combos));
        if let Some(live) = ctx.live {
            let (id, _rx, _pending) = confirm::waiting();
            live.sink.send(card(id, "allowed", asked.clipboard_read, asked.clipboard_write, asked.system_key_combos, true));
        }
        tracing::info!(apps = new.len(), "Computer use: apps allowed without a card (Bypass permissions)");
        return result_of("allowed", &new, &denied, &session_now());
    }
    if crate::ai::eval::active() {
        return nobody("Nobody answers cards in this evaluation.");
    }
    #[cfg(test)]
    if let Some(r) = confirm::test_reply() {
        let (state, cr, cw, sk) = settle(Some(r), &asked);
        if state == "allowed" {
            grants::with(conversation, |s| s.grant(new.clone(), cr, cw, sk));
        }
        return result_of(state, &new, &denied, &session_now());
    }
    let Some(live) = ctx.live else { return nobody("Nobody can see a card for this turn.") };
    let (id, rx, _pending) = confirm::waiting();
    live.sink.send(card(id, "asking", asked.clipboard_read, asked.clipboard_write, asked.system_key_combos, false));
    tracing::info!(id, apps = new.len(), denied = denied.len(), "Computer use: an access card waits for the user");
    let reply = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(r)) => Some(r),
        Ok(Err(_)) => Some(Reply::Deny),
        Err(_) => None,
    };
    let (state, cr, cw, sk) = settle(reply, &asked);
    if state == "allowed" {
        grants::with(conversation, |s| s.grant(new.clone(), cr, cw, sk));
    }
    live.sink.send(card(id, state, cr, cw, sk, false));
    tracing::info!(id, outcome = state, "Computer use: an access card ended");
    result_of(state, &new, &denied, &session_now())
}

/// **The access card's answer** (Allow with the extras left ticked, or
/// Don't): `true` if that card was still waiting.
#[tauri::command]
pub fn chat_computer_access(id: u64, allow: bool, clipboard_read: Option<bool>, clipboard_write: Option<bool>, system_key_combos: Option<bool>) -> bool {
    if !allow {
        return confirm::reply(id, Reply::Deny);
    }
    let picked = json!({ "allow": true, "clipboardRead": clipboard_read.unwrap_or(true), "clipboardWrite": clipboard_write.unwrap_or(true), "systemKeyCombos": system_key_combos.unwrap_or(true) });
    confirm::reply(id, Reply::Picked(picked))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, target: &str) -> App {
        App::linked(id, name, target, "")
    }

    /// The Voice Assistant from any app: the access card said in one line
    /// and put on the bar (Allow / Don't), a spoken yes answering it.
    #[test]
    fn an_access_card_asked_out_loud() {
        let a = AccessCard {
            apps: vec![AccessApp { name: "Notepad".into(), id: "n".into(), tier: "full".into() }, AccessApp { name: "Terminal".into(), id: "t".into(), tier: "click".into() }],
            reason: "To write your shopping list.".into(),
            clipboard_write: true,
            ..Default::default()
        };
        assert_eq!(spoken_line(&a, true), "Let me use Notepad and Terminal, to write your shopping list? Say yes, or press Allow.");
        assert!(spoken_line(&a, false).ends_with("It needs a click: press Allow on the Yap bar."));
        let bar = bar_card(7, &a, true);
        assert_eq!(bar.title, "Let Yap use your apps?");
        assert!(bar.body.contains("Notepad · clicks and typing") && bar.body.contains("Terminal · clicks only") && bar.body.contains("Also: write the clipboard."), "{}", bar.body);
        assert_eq!(bar.primary.as_ref().map(|p| (p.id.as_str(), p.label.as_str())), Some(("allow:7", "Allow")));
        assert_eq!(bar.secondary.as_ref().map(|p| p.id.as_str()), Some("deny:7"));
        let card = crate::chat::pages::act::confirm::Card { id: 7, act: ACT.into(), state: "asking".into(), access: Some(Box::new(a.clone())), ..Default::default() };
        let asking = crate::chat::assistant::spoken_ok::asking(&card, false).expect("asked out loud");
        assert!(asking.spoken && asking.line.starts_with("Let me use Notepad"));
        let bypass = crate::chat::pages::act::confirm::Card { access: Some(Box::new(AccessCard { bypass: true, ..a })), ..card };
        assert!(crate::chat::assistant::spoken_ok::asking(&bypass, false).is_none(), "allowed without asking: nothing to say");
    }

    #[test]
    fn a_request_names_each_app_once() {
        let r = request_of(&json!({ "apps": ["Notepad", " notepad ", "", "File Explorer"], "reason": "To tidy\nthe   folder", "clipboardWrite": true }));
        assert_eq!(r.apps, ["Notepad", "File Explorer"]);
        assert_eq!(r.reason, "To tidy the folder");
        assert!(r.clipboard_write && !r.clipboard_read && !r.system_key_combos);
        assert_eq!(request_of(&json!({ "apps": "Paint, Notepad" })).apps, ["Paint", "Notepad"]);
        let many: Vec<String> = (0..30).map(|i| format!("App {i}")).collect();
        assert_eq!(request_of(&json!({ "apps": many })).apps.len(), APPS_MAX);
    }

    #[test]
    fn names_resolve_to_the_start_menus_apps_and_some_never_do() {
        let own = r"c:\program files\yap\yap.exe";
        let apps = vec![
            app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", ""),
            app("Microsoft.Windows.Explorer", "File Explorer", ""),
            app(own, "Yap", own),
            app(r"C:\Program Files\Bitwarden\Bitwarden.exe", "Bitwarden", r"C:\Program Files\Bitwarden\Bitwarden.exe"),
            app("steam://rungameid/1", "A Game", "steam://rungameid/1"),
        ];
        let names: Vec<String> = ["notepad", "File Explorer", "Yap", "Bitwarden", "A Game", "Nothing Like It"].iter().map(|s| s.to_string()).collect();
        let (found, denied) = resolve(&names, &apps, &[], own);
        assert_eq!(found.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Notepad", "File Explorer"]);
        let why = |n: &str| denied.iter().find(|d| d.name == n).map(|d| d.why.clone()).unwrap_or_default();
        assert!(why("Yap").contains("own windows"));
        assert!(why("Bitwarden").contains("password manager"));
        assert!(why("A Game").contains("can't tell"));
        assert!(why("Nothing Like It").contains("Not in the Start menu"));
    }

    #[test]
    fn the_extras_given_are_those_asked_for_and_left_ticked() {
        let asked = Request { apps: vec!["Notepad".into()], clipboard_read: true, clipboard_write: true, ..Default::default() };
        assert_eq!(settle(Some(Reply::Picked(json!({ "allow": true, "clipboardRead": false }))), &asked), ("allowed", false, true, false));
        assert_eq!(settle(Some(Reply::Picked(json!({ "allow": true, "systemKeyCombos": true }))), &asked), ("allowed", true, true, false), "never one not asked for");
        assert_eq!(settle(Some(Reply::Deny), &asked).0, "refused");
        assert_eq!(settle(None, &asked).0, "timedOut");
    }

    /// The card answered (a unit test's reply): the apps are in the chat's
    /// session, the result says so; refused, nothing is.
    // (The lock is held across awaits on purpose: one test at a time with the sessions.)
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn allowing_on_the_card_lets_the_apps_in_for_that_chat() {
        let _scene = super::super::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        grants::forget_all();
        crate::apps::tests::set_stand_ins(vec![app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", "")]);
        let ctx = super::super::Ctx { conversation: Some(41), live: None, provider: "anthropic", base_url: "https://api.anthropic.com/v1", unattended: false, turn: "t" };
        confirm::answer_in_test(Some(false));
        let no = request(&json!({ "apps": ["Notepad"], "reason": "To write a note" }), &ctx).await;
        assert_eq!(no["success"], false);
        assert!(grants::with(Some(41), |s| s.apps.is_empty()));
        confirm::answer_in_test(Some(true));
        let yes = request(&json!({ "apps": ["Notepad"], "reason": "To write a note" }), &ctx).await;
        assert_eq!(yes["success"], true, "{yes}");
        assert!(grants::with(Some(41), |s| s.apps.len() == 1));
        assert_eq!(request(&json!({ "apps": ["Notepad"] }), &ctx).await["displayText"], "Apps already allowed");
        confirm::answer_in_test(None);
        // In Bypass permissions: allowed at once, nothing asked.
        let bypass = super::super::Ctx { conversation: Some(42), unattended: true, ..ctx };
        assert_eq!(request(&json!({ "apps": ["Notepad"] }), &bypass).await["success"], true);
        grants::forget_all();
    }
}
