//! **A dictation in steps** (2026-10-29): what goes into a chat app or an
//! AI chat box as more than one paste — **stacked messages**
//! (`stacked.rs`: each pasted and sent in turn), a **mention** picked from
//! a team chat's list (`mentions.rs`) and a **file** picked in an AI chat
//! box (`file_tags.rs`): the text pasted around them, the "@Sam" typed, Tab
//! once the list shows that person or file. Built on `text_injector`'s
//! own paste ([`text_injector::inject_text`], verified, private), typing,
//! Tab and submit key.
//!
//! - **Planned first** ([`plan`], pure): the whole dictation is ready —
//!   cleaned up, corrected, fitted — before the first message goes.
//! - **Stops at the first doubt**, never sends on regardless: before each
//!   message after the first, the window must still be in front and its
//!   chat box have emptied (the last one was sent:
//!   `selection::read_context`), and every send key goes only with the
//!   window in front; a paste, a key or the typing Windows didn't take ends
//!   it too. What's left goes on the clipboard (privately) and the person is
//!   told which message it stopped at ([`stopped_alert`]). A first paste
//!   that doesn't go in at all leaves the whole dictation there.
//! - **A key only on a sure sign**: Tab goes only when UI Automation shows
//!   the app's list highlighting the person or file said
//!   (`selection::picked_option`, `mentions::option_is`); else the typed
//!   "@Sam" stays as plain text.
//! - The log has counts and steps, never the words. Test mode pastes and
//!   presses nothing (`inject_text`, `type_text`, `press_step_key`,
//!   `press_submit` say so) and reports the steps it would take
//!   (`Delivered.steps`); a spec stages the window ([`e2e_chat_target`]).

use std::time::{Duration, Instant};

use crate::dictionary::screen_terms::code::CodeFile;
use crate::pasting::app_kind::AppKind;
use crate::pasting::file_tags::{self, Tagging};
use crate::pasting::mentions;
use crate::pasting::text_injector::{self, Delivery};

/// After a message's submit key, before the next message: the app sends it
/// and empties its box.
const SEND_GAP_MS: u64 = 350;
/// How long an app's list has to show the person or file said.
const PICK_WAIT_MS: u64 = 1_200;
/// How often it's looked at meanwhile.
const PICK_EVERY_MS: u64 = 60;
/// After Tab, before the text that follows: the app puts the mention in.
const AFTER_PICK_MS: u64 = 120;

/// A message's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// Pasted.
    Text(String),
    /// Typed ("@Sam", "#userService.ts"), then picked from the app's list
    /// with Tab once it shows `said` highlighted.
    Pick { typed: String, said: String },
}

impl Piece {
    /// What it reads as in the text (a pick as typed).
    fn plain(&self) -> &str {
        match self {
            Piece::Text(t) => t,
            Piece::Pick { typed, .. } => typed,
        }
    }
}

/// Where a dictation goes, as far as its pieces go.
#[derive(Clone, Debug, Default)]
pub struct Where {
    /// A team chat with mentions, and the people's names on screen.
    pub mentions: Option<mentions::App>,
    pub people: Vec<String>,
    /// How files are tagged there (an AI chat box, a coding CLI), and the
    /// files on screen.
    pub tagging: Option<Tagging>,
    pub files: Vec<CodeFile>,
}

/// A span of the text that becomes a pick: its byte range, what's typed
/// and what the list must show.
struct Span {
    start: usize,
    end: usize,
    typed: String,
    said: String,
}

/// One message (`text`, fitted) as pieces, and the text it reads as
/// (mentions "@Sam", files "@userService.ts" or "@path"). Pure.
pub fn pieces(text: &str, w: &Where) -> (Vec<Piece>, String) {
    let spans: Vec<Span> = match (w.tagging, w.mentions) {
        (Some(tagging), _) => {
            let tags = file_tags::find(text, &w.files);
            match tagging {
                Tagging::Path => {
                    let out = file_tags::as_paths(text, &tags);
                    return (vec![Piece::Text(out.clone())], out);
                }
                Tagging::Pick { trigger } => tags
                    .into_iter()
                    .map(|t| Span { start: t.start, end: t.end, typed: format!("{trigger}{}", t.name), said: t.name })
                    .collect(),
            }
        }
        (None, Some(app)) => {
            let found = mentions::find(text, &w.people);
            if !app.picks_with_tab() {
                let out = mentions::plain(text, &found);
                return (vec![Piece::Text(out.clone())], out);
            }
            found
                .into_iter()
                .map(|f| Span { start: f.start, end: f.end, typed: format!("@{}", f.name), said: f.name })
                .collect()
        }
        (None, None) => Vec::new(),
    };
    let mut out = Vec::new();
    let mut plain = String::with_capacity(text.len());
    let mut at = 0;
    for s in spans {
        if s.start > at {
            out.push(Piece::Text(text[at..s.start].to_string()));
        }
        plain.push_str(&text[at..s.start]);
        plain.push_str(&s.typed);
        out.push(Piece::Pick { typed: s.typed, said: s.said });
        at = s.end;
    }
    if at < text.len() {
        out.push(Piece::Text(text[at..].to_string()));
    }
    plain.push_str(&text[at..]);
    if out.is_empty() {
        out.push(Piece::Text(String::new()));
    }
    (out, plain)
}

/// A dictation's steps: its messages' pieces, and the key that sends each
/// (every one: stacked messages are only made for a dictation that's sent).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub messages: Vec<Vec<Piece>>,
    pub send: Option<String>,
}

impl Plan {
    /// More than one plain paste: several messages, or a pick.
    pub fn in_steps(&self) -> bool {
        self.messages.len() > 1 || self.messages.iter().flatten().any(|p| matches!(p, Piece::Pick { .. }))
    }

    fn picks(&self) -> usize {
        self.messages.iter().flatten().filter(|p| matches!(p, Piece::Pick { .. })).count()
    }
}

/// The plan for a dictation's fitted messages (one, or several stacked),
/// and the whole text it reads as (messages on lines of their own), for
/// history and the report. `send`: the submit key that follows. Pure.
pub fn plan(messages: &[String], w: &Where, send: Option<String>) -> (Plan, String) {
    let mut plan = Plan { messages: Vec::new(), send };
    let mut texts = Vec::new();
    for m in messages {
        let (p, text) = pieces(m, w);
        plan.messages.push(p);
        texts.push(text);
    }
    (plan, texts.join("\n"))
}

/// One step as it happened (the robot's and the e2e suite's report).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Text pasted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paste: Option<String>,
    /// Text typed ("@Sam")…
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typed: Option<String>,
    /// …and whether the app's list showed it and Tab picked it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picked: Option<bool>,
    /// A key pressed ("enter", "ctrlEnter").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Where and how the steps go.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub target: Option<isize>,
    pub restore_clipboard: bool,
    /// Into `target` or nowhere from the first paste on (an Undo, a
    /// dictation whose mic was unplugged…).
    pub strict: bool,
    /// The kind of app as read (its paste keys).
    pub kind: AppKind,
}

/// What became of a plan.
#[derive(Debug)]
pub struct Outcome {
    /// How the first paste went when nothing went in at all (the caller
    /// says so as for any paste); else `Pasted`.
    pub delivery: Result<Delivery, String>,
    pub steps: Vec<Step>,
    /// It stopped part way: why, and what's left (on the clipboard).
    pub stopped: Option<Stopped>,
}

/// Where a plan stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stopped {
    pub why: Why,
    /// Messages sent before it stopped, of how many.
    pub sent: usize,
    pub total: usize,
    /// What wasn't pasted, on the clipboard.
    pub rest: String,
}

/// Why a plan stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The chat box still had text after the submit key: not sent.
    NotSent,
    /// The window it was for isn't in front any more.
    Window,
    /// Windows didn't take a paste, the typing or a key.
    Keys,
}

impl Why {
    pub fn code(self) -> &'static str {
        match self {
            Why::NotSent => "notSent",
            Why::Window => "window",
            Why::Keys => "keys",
        }
    }
}

/// The one notice for a plan that stopped part way: what went, and that
/// the rest is on the clipboard.
pub fn stopped_alert(s: &Stopped) -> crate::notices::alert::Alert {
    let title = if s.total > 1 {
        format!("Sent {} of {} messages · the rest is copied", s.sent, s.total)
    } else {
        "Couldn't finish typing it · the rest is copied".to_string()
    };
    let body = match s.why {
        Why::NotSent => "The chat box still had your message in it after the send key, so Yap stopped before sending more. Press Ctrl+V to paste the rest.",
        Why::Window => "Another window came to the front, so Yap stopped. Click back into the chat and press Ctrl+V to paste the rest.",
        Why::Keys => "Windows didn't let Yap press keys just then, so it stopped. Press Ctrl+V to paste the rest.",
    };
    crate::notices::alert::Alert {
        kind: "pasteBlocked",
        icon: "clipboard",
        title,
        body: body.to_string(),
        action: None,
        timeout_ms: 12_000,
    }
}

/// A test run stands in for the window and its UI Automation.
fn test_run() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

/// Is `target` in front (bringing it back once if not)? A test run: yes.
async fn front(target: Option<isize>) -> bool {
    let Some(hwnd) = target else { return true };
    if test_run() {
        return true;
    }
    if text_injector::current_foreground() == Some(hwnd) {
        return true;
    }
    tokio::task::spawn_blocking(move || {
        text_injector::focus(hwnd);
        std::thread::sleep(Duration::from_millis(60));
        text_injector::current_foreground() == Some(hwnd)
    })
    .await
    .unwrap_or(false)
}

/// The text around the caret in `target`'s focused field, read now (`None`:
/// can't tell, or a test run).
async fn field(target: Option<isize>) -> Option<crate::pasting::smart_insert::FieldContext> {
    if test_run() || !crate::shell::realtest::permits(target) {
        return None;
    }
    tokio::task::spawn_blocking(move || crate::pasting::selection::read_context(target)).await.ok().flatten()
}

/// After a send: did the chat box empty? Unknown counts as yes.
async fn box_emptied(target: Option<isize>) -> bool {
    match field(target).await {
        Some(ctx) => ctx.before.trim().is_empty() && ctx.after.trim().is_empty() && ctx.selection.trim().is_empty(),
        None => true,
    }
}

/// Test mode: the app's list shows the one said ([`e2e_chat_target`]).
#[cfg(debug_assertions)]
static TEST_PICKS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Does the app's list (in `target`'s process) highlight `said` within
/// [`PICK_WAIT_MS`]?
async fn list_shows(target: Option<isize>, said: &str) -> bool {
    if test_run() {
        #[cfg(debug_assertions)]
        return TEST_PICKS.load(std::sync::atomic::Ordering::SeqCst);
        #[cfg(not(debug_assertions))]
        return false;
    }
    let Some(pid) = target.and_then(text_injector::window_process) else { return false };
    let said = said.to_string();
    tokio::task::spawn_blocking(move || {
        let until = Instant::now() + Duration::from_millis(PICK_WAIT_MS);
        loop {
            if let Some(option) = crate::pasting::selection::picked_option(pid) {
                if mentions::option_is(&option, &said) {
                    return true;
                }
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(PICK_EVERY_MS));
        }
    })
    .await
    .unwrap_or(false)
}

/// What's left of a plan from message `mi`, piece `pi` on: its text, the
/// messages on lines of their own.
fn rest(plan: &Plan, mi: usize, pi: usize) -> String {
    let mut parts = Vec::new();
    for (i, m) in plan.messages.iter().enumerate().skip(mi) {
        let from = if i == mi { pi } else { 0 };
        let text: String = m.iter().skip(from).map(Piece::plain).collect();
        if !text.trim().is_empty() {
            parts.push(text.trim().to_string());
        }
    }
    parts.join("\n")
}

/// Run a plan: each message's pieces pasted or typed and picked, then its
/// submit key; checked all the way ([`Outcome`]). Logs counts only.
pub async fn run(plan: &Plan, r: Run) -> Outcome {
    let total = plan.messages.len();
    let mut steps: Vec<Step> = Vec::new();
    let mut sent = 0usize;
    tracing::info!(messages = total, picks = plan.picks(), send = plan.send.is_some(), "Pasting in steps");
    let stop = |why: Why, mi: usize, pi: usize, sent: usize, steps: Vec<Step>| {
        let rest = rest(plan, mi, pi);
        tracing::warn!(why = why.code(), sent, total, rest = rest.chars().count(), "Stopped part way; the rest is on the clipboard");
        if !rest.is_empty() && !test_run() && !text_injector::copy_privately(&rest) {
            tracing::warn!("The rest couldn't be put on the clipboard");
        }
        Outcome { delivery: Ok(Delivery::Pasted), steps, stopped: Some(Stopped { why, sent, total, rest }) }
    };
    for (mi, pieces) in plan.messages.iter().enumerate() {
        if mi > 0 {
            // The window is still the one it was for (its box is the one
            // read: with another app in front nothing can be read, which
            // would pass for empty), and the last message was sent (its box
            // emptied).
            if !front(r.target).await {
                return stop(Why::Window, mi, 0, sent, steps);
            }
            if !box_emptied(r.target).await {
                // The last message's key didn't send it: it's still there.
                return stop(Why::NotSent, mi, 0, sent.saturating_sub(1), steps);
            }
        }
        let mut picked = false;
        for (pi, piece) in pieces.iter().enumerate() {
            let first = mi == 0 && pi == 0;
            match piece {
                Piece::Text(t) => {
                    let mut t = t.clone();
                    // The app put a space after the mention it inserted
                    // (Slack, Discord): not another, and none before a
                    // comma or a full stop ("@Sam Example, can you…").
                    let spaced = picked && field(r.target).await.is_some_and(|c| c.before.ends_with(char::is_whitespace));
                    if picked {
                        tracing::debug!(spaced, "After a picked mention or file: a space of the app's before the text that follows");
                    }
                    if spaced && t.starts_with(' ') {
                        t.remove(0);
                    } else if spaced && t.starts_with([',', '.', ';', ':', '!', '?', ')']) {
                        if let Err(e) = text_injector::press_step_key(text_injector::StepKey::Backspace, r.target).await {
                            tracing::warn!("Backspace didn't go: {e}");
                            return stop(Why::Keys, mi, pi, sent, steps);
                        }
                        steps.push(Step { key: Some("backspace".into()), ..Step::default() });
                    }
                    picked = false;
                    if t.is_empty() {
                        continue;
                    }
                    let strict = r.strict || !first;
                    match text_injector::inject_text(&t, r.restore_clipboard, r.target, strict, r.kind).await {
                        Ok(Delivery::Pasted | Delivery::Typed) => steps.push(Step { paste: Some(t), ..Step::default() }),
                        Ok(d) if first => {
                            // Nothing went in, and the paste left only its own
                            // piece on the clipboard: the whole dictation goes
                            // there, as the notice the caller shows says — not
                            // the first message alone.
                            let all = rest(plan, 0, 0);
                            if all != t.trim() && !test_run() && !text_injector::copy_privately(&all) {
                                tracing::warn!("The whole dictation couldn't be put on the clipboard");
                            }
                            return Outcome { delivery: Ok(d), steps, stopped: None };
                        }
                        Err(e) if first => return Outcome { delivery: Err(e), steps, stopped: None },
                        Ok(Delivery::WindowGone) => return stop(Why::Window, mi, pi, sent, steps),
                        _ => return stop(Why::Keys, mi, pi, sent, steps),
                    }
                }
                Piece::Pick { typed, said } => {
                    if !front(r.target).await {
                        return stop(Why::Window, mi, pi, sent, steps);
                    }
                    if let Err(e) = text_injector::type_text(typed, r.target).await {
                        tracing::warn!("Typing a mention or a file's name didn't go: {e}");
                        return stop(Why::Keys, mi, pi, sent, steps);
                    }
                    let shows = list_shows(r.target, said).await;
                    if shows {
                        if let Err(e) = text_injector::press_step_key(text_injector::StepKey::Tab, r.target).await {
                            tracing::warn!("Tab didn't go: {e}");
                            return stop(Why::Keys, mi, pi + 1, sent, steps);
                        }
                        tokio::time::sleep(Duration::from_millis(AFTER_PICK_MS)).await;
                    }
                    tracing::info!(picked = shows, "A mention or a file typed");
                    picked = shows;
                    steps.push(Step { typed: Some(typed.clone()), picked: Some(shows), ..Step::default() });
                }
            }
        }
        if let Some(key) = &plan.send {
            // The send key goes to the window in front: only the chat's.
            // (Another window that came up meanwhile — a dialog, a call —
            // would take the Enter.)
            if !front(r.target).await {
                return stop(Why::Window, mi + 1, 0, sent, steps);
            }
            if let Err(e) = text_injector::press_submit(key, r.target).await {
                tracing::warn!("The send key didn't go: {e}");
                return stop(Why::Keys, mi + 1, 0, sent, steps);
            }
            steps.push(Step { key: Some(key.clone()), ..Step::default() });
            sent += 1;
            if mi + 1 < total {
                tokio::time::sleep(Duration::from_millis(SEND_GAP_MS)).await;
            }
        }
    }
    tracing::info!(sent, steps = steps.len(), "Pasted in steps");
    Outcome { delivery: Ok(Delivery::Pasted), steps, stopped: None }
}

// ---- test mode -----------------------------------------------------------------------------------

/// Test mode's stand-in for the window a dictation goes into
/// ([`e2e_chat_target`]): its process and title.
#[cfg(debug_assertions)]
static TEST_APP: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// Test mode: the kind of app the stand-in window is (`None`: none staged).
#[cfg(debug_assertions)]
pub fn test_kind() -> Option<AppKind> {
    if !test_run() {
        return None;
    }
    let app = TEST_APP.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
    Some(crate::pasting::app_kind::classify(Some(&app.0), "", &app.1))
}

/// The window's process and title, for which team chat it is: test mode's
/// stand-in, else the window's own (the title only for a browser's).
pub fn app_and_title(target: Option<isize>) -> (String, String) {
    #[cfg(debug_assertions)]
    if test_run() {
        return TEST_APP.lock().unwrap_or_else(|p| p.into_inner()).clone().unwrap_or_default();
    }
    let exe = text_injector::app_name_for(target).unwrap_or_default();
    let stem = crate::pasting::app_kind::exe_stem(&exe);
    let title = match target {
        Some(h) if crate::pasting::app_kind::is_browser(&stem) && crate::shell::realtest::permits(Some(h)) => {
            crate::pasting::app_kind::window_title(h)
        }
        _ => String::new(),
    };
    (exe, title)
}

/// Debug-only, test mode: dictations go into a stand-in window — process
/// `exe` with the title `title` (`None`: none, as before) — that shows
/// `screen` (its text, read as a real window's: the names, and in a code
/// app the files and symbols) with the focus labelled `focus` (an AI chat
/// box: `{class, name}`); `picks`: an app's list shows the person or file
/// typed. Nothing is read, pasted or pressed.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_chat_target(
    exe: Option<String>,
    title: Option<String>,
    screen: Option<String>,
    focus_class: Option<String>,
    focus_name: Option<String>,
    picks: Option<bool>,
) -> Result<(), String> {
    if !test_run() {
        return Err("e2e_chat_target is for test runs (not robot mode)".into());
    }
    let app = exe.clone().map(|e| (e, title.clone().unwrap_or_default()));
    *TEST_APP.lock().unwrap_or_else(|p| p.into_inner()) = app.clone();
    // Cleanup's style follows the same stand-in.
    crate::ai::cleanup_styles::e2e_style_app(exe.clone(), title.clone())?;
    let code_app = app.as_ref().and_then(|(e, t)| {
        let kind = crate::pasting::app_kind::classify(Some(e), "", t);
        crate::dictionary::screen_terms::code::app_of(&crate::pasting::app_kind::exe_stem(e), kind)
    });
    let focus = (focus_class.is_some() || focus_name.is_some()).then(|| crate::dictionary::screen_terms::code::Labels {
        class: focus_class.unwrap_or_default(),
        name: focus_name.unwrap_or_default(),
        ..Default::default()
    });
    crate::dictionary::screen_terms::stage_for_test(screen.as_deref(), code_app, focus);
    TEST_PICKS.store(picks.unwrap_or(false), std::sync::atomic::Ordering::SeqCst);
    tracing::info!(app = exe.is_some(), screen = screen.is_some(), "e2e: dictations go to a stand-in chat or editor");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::screen_terms::code::CodeFile;

    fn slack() -> Where {
        Where { mentions: Some(mentions::App::Slack), people: vec!["Sam".into(), "Priya".into()], ..Where::default() }
    }

    #[test]
    fn a_mention_in_slack_is_typed_and_picked() {
        let (p, plain) = pieces("At Sam, can you check the build?", &slack());
        assert_eq!(
            p,
            [
                Piece::Pick { typed: "@Sam".into(), said: "Sam".into() },
                Piece::Text(", can you check the build?".into()),
            ]
        );
        assert_eq!(plain, "@Sam, can you check the build?");
        let (p, _) = pieces("Thanks at Priya", &slack());
        assert_eq!(p, [Piece::Text("Thanks ".into()), Piece::Pick { typed: "@Priya".into(), said: "Priya".into() }]);
    }

    #[test]
    fn a_mention_where_the_list_needs_enter_is_plain_text() {
        let teams = Where { mentions: Some(mentions::App::Teams), ..slack() };
        let (p, plain) = pieces("At Sam, can you check?", &teams);
        assert_eq!(p, [Piece::Text("@Sam, can you check?".into())]);
        assert_eq!(plain, "@Sam, can you check?");
        // No mentions: as it is.
        let (p, _) = pieces("At Sam, can you check?", &Where::default());
        assert_eq!(p, [Piece::Text("At Sam, can you check?".into())]);
    }

    #[test]
    fn a_file_in_an_ai_chat_box_or_a_coding_cli() {
        let files = vec![CodeFile { name: "userService.ts".into(), paths: vec!["src/userService.ts".into()] }];
        let cursor = Where { tagging: Some(Tagging::Pick { trigger: '@' }), files: files.clone(), ..Where::default() };
        let (p, plain) = pieces("Fix userService.ts please", &cursor);
        assert_eq!(
            p,
            [
                Piece::Text("Fix ".into()),
                Piece::Pick { typed: "@userService.ts".into(), said: "userService.ts".into() },
                Piece::Text(" please".into()),
            ]
        );
        assert_eq!(plain, "Fix @userService.ts please");
        let vscode = Where { tagging: Some(Tagging::Pick { trigger: '#' }), files: files.clone(), ..Where::default() };
        assert_eq!(pieces("Fix userService.ts", &vscode).1, "Fix #userService.ts");
        let claude = Where { tagging: Some(Tagging::Path), files, ..Where::default() };
        assert_eq!(pieces("Fix userService.ts", &claude), (vec![Piece::Text("Fix @src/userService.ts".into())], "Fix @src/userService.ts".into()));
    }

    #[test]
    fn stacked_messages_are_steps_and_a_lone_plain_paste_isnt() {
        let (p, text) = plan(&["Hi Sam".into(), "Are you free?".into()], &Where::default(), Some("enter".into()));
        assert!(p.in_steps());
        assert_eq!(text, "Hi Sam\nAre you free?");
        let (p, _) = plan(&["Hi Sam".into()], &Where::default(), Some("enter".into()));
        assert!(!p.in_steps());
        let (p, _) = plan(&["At Sam, hi".into()], &slack(), None);
        assert!(p.in_steps());
    }

    /// An older Yap sharing the folder drops the new settings when it
    /// saves: read back, they're their defaults — stacked messages off,
    /// mentions and file tags on, the Send shortcut unbound. Nothing else.
    #[test]
    fn an_older_yaps_save_leaves_the_defaults() {
        let older: crate::config::YapConfig = serde_json::from_str(r#"{"hotkey":"kb:121","autoSubmit":true}"#).unwrap();
        assert!(!older.stacked_messages);
        assert!(older.chat_mentions && older.file_tags);
        assert_eq!(older.send_hotkey, "");
        assert!(older.auto_submit);
        let saved = serde_json::to_value(crate::config::YapConfig { stacked_messages: true, send_hotkey: "mouse:5".into(), ..Default::default() }).unwrap();
        assert_eq!(saved["stackedMessages"], true);
        assert_eq!(saved["sendHotkey"], "mouse:5");
        assert_eq!(saved["chatMentions"], true);
        assert_eq!(saved["fileTags"], true);
    }

    #[test]
    fn what_is_left_when_it_stops() {
        let (p, _) = plan(&["At Sam, hi".into(), "Second".into(), "Third".into()], &slack(), Some("enter".into()));
        assert_eq!(rest(&p, 0, 0), "@Sam, hi\nSecond\nThird");
        assert_eq!(rest(&p, 0, 1), ", hi\nSecond\nThird");
        assert_eq!(rest(&p, 1, 0), "Second\nThird");
        assert_eq!(rest(&p, 3, 0), "");
        let s = Stopped { why: Why::NotSent, sent: 1, total: 3, rest: "x".into() };
        assert_eq!(stopped_alert(&s).title, "Sent 1 of 3 messages · the rest is copied");
    }
}
