//! **Plan usage limits** (2026-11-07): where the person's Claude plan's
//! limits stand, for Chat's usage ring (`views/chat/PlanUsageRing.svelte`)
//! and Settings' plan card, as the Claude desktop app shows them.
//!
//! - **Asked** with Claude Code's own experimental `get_usage`
//!   (`claude_code/usage.rs`), never on a timer and only while the plan is
//!   connected (the key store's marker):
//!   - after each plan turn, on the conversation's warm session
//!     ([`after_turn`]);
//!   - when a composer showing the plan chip mounts, or the window gets the
//!     focus back, or Settings' card shows (`claude_plan_usage {refresh}`):
//!     at most once a minute ([`EVERY`]), on a warm session that's free,
//!     else a one-off probe (15 s at most, always ended).
//! - **Live**: a turn's `rate_limit_event`s move the rows they name
//!   ([`live`]) until the next answer.
//! - **Kept in memory only**, never on disk: the latest answer, and each
//!   plan conversation's context (how full it is: its last model call's
//!   prompt and answer, against the model's window — [`note_context`]).
//! - Every change goes to the windows as `yap-claude-plan-usage`.
//! - **Not in Network connections**: it's Claude Code's own request to
//!   Anthropic, on the person's own sign-in, never Yap's
//!   (privacy-receipts.md).
//!
//! An answer Yap can't use (an API key's login, an older Claude Code that
//! refuses `get_usage`, a shape that changed) shows no ring at all. After
//! one, Yap waits [`QUIET`] before probing again; a session that didn't
//! answer in time isn't asked after a turn for [`QUIET`] either (it would
//! hold the next question up). Logs: counts and outcomes at `info`, the
//! numbers at `debug`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Emitter;

use crate::ai::claude_code::events::RateLimit;
use crate::ai::claude_code::usage::{self, Asked, PlanUsage};

use super::sessions;

/// The most often the page's asks (mount, focus) reach Claude Code.
pub const EVERY: Duration = Duration::from_secs(60);
/// How long to leave it after an answer with nothing to show, or none.
pub const QUIET: Duration = Duration::from_secs(30 * 60);
/// A one-off probe's longest wait (Claude Code starts first).
const PROBE_WAIT: Duration = Duration::from_secs(15);
/// A warm session's (the next question may be waiting behind it).
const SESSION_WAIT: Duration = Duration::from_secs(5);
/// The conversations whose context is kept.
const CONTEXTS: usize = 50;

/// How full a plan conversation is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    /// Its last model call's prompt and answer, in tokens.
    pub used: u64,
    /// The model's context window.
    pub window: u64,
    /// The window is the model's own word (Claude Code's `modelUsage`),
    /// else Anthropic's usual.
    pub known: bool,
}

/// What the page shows (`claude_plan_usage`, `yap-claude-plan-usage`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    /// The plan's limits; `None`: no ring.
    pub usage: Option<PlanUsage>,
    /// Each plan conversation's context, by its id.
    pub contexts: std::collections::BTreeMap<String, Fill>,
}

#[derive(Default)]
struct State {
    latest: Option<PlanUsage>,
    /// When the page's asks last reached Claude Code.
    asked: Option<Instant>,
    /// No probe before this (nothing to show, or no answer, last time).
    quiet_until: Option<Instant>,
    /// No ask after a turn before this (a session didn't answer in time).
    sessions_quiet_until: Option<Instant>,
    contexts: VecDeque<(u64, Fill)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// One ask at a time (a mount and a focus at once ask once).
static ASKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(State::default))
}

/// The plan is connected here: the key store's marker (`claude_plan::sync`).
pub fn connected() -> bool {
    crate::config::load().pp_api_keys.get(super::PROVIDER).is_some_and(|k| k == super::PLAN_KEY)
}

/// What the page shows now.
pub fn shown() -> Shown {
    with(|s| Shown {
        usage: s.latest.clone(),
        contexts: s.contexts.iter().map(|(id, f)| (id.to_string(), *f)).collect(),
    })
}

fn emit() {
    if let Some(app) = super::APP.get() {
        let _ = app.emit("yap-claude-plan-usage", shown());
    }
}

/// The plan disconnected (signed out, Claude Code gone): no ring.
pub fn clear() {
    let had = with(|s| {
        let had = s.latest.is_some();
        s.latest = None;
        s.asked = None;
        had
    });
    if had {
        emit();
    }
}

/// An answer, kept (and said). `false`: there was nothing to keep.
fn keep(asked: &Asked, how: &'static str) -> bool {
    let now = Instant::now();
    let (usage, quiet_probe, quiet_sessions) = match asked {
        Asked::Answered(Some(u)) => (Some(u.clone()), None, None),
        Asked::Answered(None) | Asked::Refused => (None, Some(now + QUIET), None),
        Asked::NoAnswer => (None, Some(now + QUIET), (how == "session").then_some(now + QUIET)),
    };
    let outcome = match asked {
        Asked::Answered(Some(_)) => "numbers",
        Asked::Answered(None) => "none to show",
        Asked::Refused => "refused",
        Asked::NoAnswer => "no answer",
    };
    tracing::info!(how, outcome, rows = usage.as_ref().map_or(0, |u| u.limits.len()), "Claude plan: usage asked");
    if let Some(u) = &usage {
        let numbers: Vec<String> = u.limits.iter().map(|l| format!("{} {}: {:.0}%", l.title, l.scope.as_deref().unwrap_or(""), l.percent)).collect();
        tracing::debug!(?numbers, extra = u.extra.is_some(), "Claude plan: usage");
    }
    with(|s| {
        // An answer with nothing in it replaces what was shown; none at all
        // (a timeout) leaves the last numbers up.
        if !matches!(asked, Asked::NoAnswer) {
            s.latest = usage.clone();
        }
        if quiet_probe.is_some() {
            s.quiet_until = quiet_probe;
        } else {
            s.quiet_until = None;
        }
        if quiet_sessions.is_some() {
            s.sessions_quiet_until = quiet_sessions;
        } else if how == "session" {
            s.sessions_quiet_until = None;
        }
    });
    emit();
    usage.is_some()
}

/// **A turn's `rate_limit_event`**: the rows it names move now.
pub fn live(r: &RateLimit) {
    let changed = with(|s| s.latest.as_mut().is_some_and(|u| u.apply(r)));
    if changed {
        tracing::debug!(status = %r.status, kind = r.kind.as_deref().unwrap_or(""), "Claude plan: usage moved by a rate-limit event");
        emit();
    }
}

/// **How full plan conversation `conv` is**, after a turn: its last model
/// call's prompt and answer against the model's `window`. Kept for the
/// latest [`CONTEXTS`] conversations, in memory only.
pub fn note_context(conv: u64, fill: Fill) {
    with(|s| keep_context(&mut s.contexts, conv, fill));
}

/// [`note_context`] on `list`: the conversation's latest fill last, the
/// oldest dropped past [`CONTEXTS`]; a fill with no known window isn't kept.
/// Pure.
fn keep_context(list: &mut std::collections::VecDeque<(u64, Fill)>, conv: u64, fill: Fill) {
    if fill.window == 0 {
        return;
    }
    list.retain(|(id, _)| *id != conv);
    list.push_back((conv, fill));
    while list.len() > CONTEXTS {
        list.pop_front();
    }
}

/// A conversation deleted: its context goes.
pub fn forget_context(conv: u64) {
    with(|s| s.contexts.retain(|(id, _)| *id != conv));
}

/// Every conversation's context goes (Delete some of your data → Chats).
pub fn forget_contexts() {
    with(|s| s.contexts.clear());
}

/// **After a plan turn**: asked on the conversation's warm session once the
/// turn has let go of it, in the background (the answer isn't held up).
pub fn after_turn(w: Arc<sessions::Warm>) {
    if !connected() {
        clear();
        return;
    }
    if with(|s| s.sessions_quiet_until.is_some_and(|t| Instant::now() < t)) {
        // Still say the conversation's context.
        emit();
        return;
    }
    tauri::async_runtime::spawn(async move {
        let _one = ASKING.lock().await;
        let asked = ask_warm(&w).await;
        match asked {
            Some(asked) => {
                keep(&asked, "session");
                with(|s| s.asked = Some(Instant::now()));
            }
            None => emit(),
        }
    });
}

/// Ask on `w` if no turn is on it: `None` when one is (that turn asks after).
async fn ask_warm(w: &sessions::Warm) -> Option<Asked> {
    let mut rx = w.events.try_lock().ok()?;
    if !w.session.is_running() {
        return None;
    }
    Some(usage::ask_session(&w.session, &mut rx, SESSION_WAIT).await)
}

/// **The page's ask** (a composer with the plan chip mounted, the window's
/// focus back, Settings' card): Claude Code asked when the last ask is a
/// minute old — on a free warm session, else a one-off probe — then what
/// to show.
pub async fn refresh() -> Shown {
    if !connected() {
        clear();
        return shown();
    }
    let _one = ASKING.lock().await;
    let due = with(|s| {
        let now = Instant::now();
        s.asked.is_none_or(|t| now.duration_since(t) >= EVERY) && s.quiet_until.is_none_or(|t| now >= t)
    });
    if !due {
        return shown();
    }
    with(|s| s.asked = Some(Instant::now()));
    // A warm session that's free answers fastest, with no process to
    // start; every one busy with a turn: that turn asks after it.
    let warm = sessions::all_warm();
    if !warm.is_empty() {
        for w in &warm {
            if let Some(asked) = ask_warm(w).await {
                keep(&asked, "session");
                break;
            }
        }
        return shown();
    }
    let asked = match super::oneshot::folder("ask") {
        Ok(cwd) => usage::probe(cwd, PROBE_WAIT).await,
        Err(_) => Asked::NoAnswer,
    };
    keep(&asked, "probe");
    shown()
}

/// **`claude_plan_usage {refresh?}`**: the plan's limits and each plan
/// conversation's context, as shown; `refresh` asks Claude Code when it's
/// due (at most once a minute).
#[tauri::command]
pub async fn claude_plan_usage(refresh: Option<bool>) -> Shown {
    if refresh.unwrap_or(false) {
        self::refresh().await
    } else {
        if !connected() {
            clear();
        }
        shown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // On a list of its own: the app-wide one is shared with every test that
    // forgets the plan's sessions (Delete some of your data's tests among
    // them), running at the same time.
    #[test]
    fn contexts_are_kept_for_the_latest_conversations_only() {
        let mut list = std::collections::VecDeque::new();
        keep_context(&mut list, 1, Fill { used: 32_000, window: 200_000, known: true });
        keep_context(&mut list, 2, Fill { used: 5, window: 0, known: false });
        keep_context(&mut list, 1, Fill { used: 41_000, window: 200_000, known: true });
        assert_eq!(list.len(), 1, "no window: not kept");
        assert_eq!(list[0].1.used, 41_000, "the latest turn's");
        for id in 10..10 + CONTEXTS as u64 + 5 {
            keep_context(&mut list, id, Fill { used: id, window: 1_000, known: false });
        }
        assert_eq!(list.len(), CONTEXTS);
        assert!(!list.iter().any(|(id, _)| *id == 1) && list.back().is_some_and(|(id, _)| *id == 9 + CONTEXTS as u64 + 5));
        let v = serde_json::to_value(Shown { usage: None, contexts: [("7".to_string(), Fill { used: 1, window: 2, known: true })].into() }).unwrap();
        assert_eq!(v, serde_json::json!({"usage": null, "contexts": {"7": {"used": 1, "window": 2, "known": true}}}));
    }
}
