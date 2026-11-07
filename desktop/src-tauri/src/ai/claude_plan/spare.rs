//! **A warm spare per scope** (7 Nov 2026): Claude Code takes a second or
//! two to start (`claude -p` loads, reads its settings, checks its sign-in)
//! before it reads a message. A dictation's cleanup or a Command can't wait
//! that long every time, so for Dictation Cleanup, Commands and Note
//! Formatting on the plan, Yap keeps **one Claude Code already started and
//! waiting on stdin** (stream-json input, the one-shot call's own lean
//! flags: `oneshot::call_spec`). A call's prompt goes to it at once, and a
//! new spare starts behind it, so only the model's own time is left.
//!
//! - **One call each**: a spare serves exactly one call, then its stdin is
//!   closed and it ends. Never a second call in the same process: no
//!   dictation ever sees another's text.
//! - **Started for what's coming**: Claude Code reads its system prompt
//!   (`--system-prompt-file`) as it starts, so a spare is made for the
//!   system prompt the scope's calls have had most often lately ([`HISTORY`]
//!   calls, in memory only), with the scope's model and effort. A call with
//!   another prompt (another kind of app's style, a selection's rewrite)
//!   runs cold, exactly as before, and the next spare follows what's most
//!   likely. The parts of a prompt that change with every call (the
//!   dictionary words a dictation says, a rewrite's selection) go in the
//!   call's message instead (`oneshot::flatten`), so they never miss.
//! - **Only while it's wanted**: only while that scope is set to the plan
//!   and the plan is connected (the key store's marker). Ended when it's
//!   been idle [`IDLE`] (the next dictation's key press starts one again,
//!   as the on-device model loads on demand: [`warm`]), on sign-out, when
//!   the scope's model or way changes ([`config_changed`]), and when Yap
//!   quits ([`end_all`]; the kill-on-close job ends it anyway).
//! - **Quietly replaced**: a spare that died (or won't take the message)
//!   is dropped and the call runs cold; a call never waits for one.
//! - **No window, no folders of its own**: the one-shot calls' empty folder
//!   (`<data>/claude/ask/`), its run folder in `%TEMP%\yap-claude\` like any
//!   run's (named for the run-folder sweep, removed when it ends).
//! - The log has scopes, counts and why, never a prompt.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedReceiver;

use crate::ai::claude_code::events::Event;
use crate::ai::claude_code::session::{Effort, Session, Spec};

use super::Scope;

/// A spare unused this long ends (the next key press starts one again).
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// The recent calls a scope's spare is made from.
pub const HISTORY: usize = 8;
/// After a spare wouldn't start, no new one for this long.
const FAILED_PAUSE: Duration = Duration::from_secs(60);

/// What a spare was started for: a call takes it only when its own match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub model: String,
    pub effort: Option<Effort>,
    /// The system prompt's hash (the prompt itself is in its run folder).
    pub system: u64,
}

impl Key {
    pub fn of(model: &str, effort: Option<Effort>, system: &str) -> Key {
        Key { model: model.to_string(), effort, system: hash(system) }
    }
}

/// A system prompt's hash (in memory only: this run's). Pure.
pub fn hash(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// One Claude Code started and waiting.
pub struct Spare {
    pub key: Key,
    pub session: Session,
    pub rx: UnboundedReceiver<Event>,
}

impl Spare {
    /// Still running and able to take a message: the events it printed
    /// before any message (none, from Claude Code; a stand-in's notes) are
    /// dropped, and one that ended is gone.
    fn fit(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(Event::Exited(_)) | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return false,
                Ok(_) => continue,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            }
        }
        self.session.is_running()
    }
}

#[derive(Default)]
struct Slot {
    spare: Option<Spare>,
    /// The latest calls' system prompts (in memory only), newest last.
    history: VecDeque<(u64, Arc<String>)>,
    /// The last call or key press: the idle clock.
    used: Option<Instant>,
    /// A spare is being started (one at a time).
    starting: bool,
    /// The last one wouldn't start: none before this.
    paused_until: Option<Instant>,
}

/// Every scope's spare and recent prompts. The app's is [`SPARES`]; tests
/// make their own.
#[derive(Default)]
pub struct Spares {
    slots: HashMap<Scope, Slot>,
}

impl Spares {
    fn slot(&mut self, scope: Scope) -> &mut Slot {
        self.slots.entry(scope).or_default()
    }

    /// **The spare for a call with `key`**, taken (never to serve another):
    /// `None` when there's none, it was made for another call, or it died
    /// (then it's gone).
    pub fn take(&mut self, scope: Scope, key: &Key) -> Option<Spare> {
        let slot = self.slot(scope);
        slot.used = Some(Instant::now());
        if slot.spare.as_ref().is_none_or(|s| &s.key != key) {
            return None;
        }
        let mut spare = slot.spare.take()?;
        if spare.fit() {
            return Some(spare);
        }
        tracing::info!(scope = scope.id(), "Claude plan: a spare had ended; this call starts its own");
        None
    }

    /// A call of `scope` with this system prompt: kept for what the next
    /// spare is made for.
    pub fn note(&mut self, scope: Scope, system: &str) {
        let slot = self.slot(scope);
        slot.used = Some(Instant::now());
        slot.history.push_back((hash(system), Arc::new(system.to_string())));
        while slot.history.len() > HISTORY {
            slot.history.pop_front();
        }
    }

    /// A key press that may lead to a call of `scope`: the idle clock
    /// starts again.
    pub fn touch(&mut self, scope: Scope) {
        self.slot(scope).used = Some(Instant::now());
    }

    /// **The system prompt the next call of `scope` most likely has**: the
    /// one its recent calls had most often (the latest of equals). Pure.
    pub fn likely(&self, scope: Scope) -> Option<Arc<String>> {
        let history = &self.slots.get(&scope)?.history;
        let mut best: Option<(usize, usize, &Arc<String>)> = None;
        for (i, (h, text)) in history.iter().enumerate() {
            let count = history.iter().filter(|(o, _)| o == h).count();
            if best.is_none_or(|(c, at, _)| count > c || (count == c && i > at)) {
                best = Some((count, i, text));
            }
        }
        best.map(|(_, _, t)| t.clone())
    }

    /// **What to start for `scope` now**, if anything: the likely prompt
    /// with `model` and `effort`, unless a spare for it is already waiting
    /// (a dead one or one for another call is ended), one is being started,
    /// or the last wouldn't start. Marks it as starting.
    pub fn plan_start(&mut self, scope: Scope, model: &str, effort: Option<Effort>) -> Option<(Key, Arc<String>)> {
        let system = self.likely(scope)?;
        let key = Key::of(model, effort, &system);
        let slot = self.slot(scope);
        if slot.starting || slot.paused_until.is_some_and(|t| Instant::now() < t) {
            return None;
        }
        if let Some(s) = slot.spare.as_mut() {
            if s.key == key && s.fit() {
                return None;
            }
            slot.spare = None;
        }
        slot.starting = true;
        Some((key, system))
    }

    /// A spare started for `scope` (or `None`: it wouldn't start). Kept
    /// only while it's still what [`plan_start`] would start; else it ends.
    pub fn started(&mut self, scope: Scope, spare: Option<Spare>, still: Option<&Key>) {
        let slot = self.slot(scope);
        slot.starting = false;
        match spare {
            None => slot.paused_until = Some(Instant::now() + FAILED_PAUSE),
            Some(s) if still == Some(&s.key) => {
                slot.paused_until = None;
                slot.spare = Some(s);
            }
            Some(_) => {}
        }
    }

    /// Every spare ended; the scopes that had one.
    pub fn end_all(&mut self) -> Vec<Scope> {
        let mut ended: Vec<Scope> = self.slots.iter_mut().filter_map(|(scope, s)| s.spare.take().map(|_| *scope)).collect();
        ended.sort_by_key(|s| s.id());
        ended
    }

    /// The spares no longer wanted (`wanted`: the key a scope's spare must
    /// have now, `None`: none at all), ended; the scopes that had one.
    pub fn keep_only(&mut self, wanted: impl Fn(Scope) -> Option<(String, Option<Effort>)>) -> Vec<Scope> {
        let mut ended = Vec::new();
        for (scope, slot) in self.slots.iter_mut() {
            let Some(s) = slot.spare.as_ref() else { continue };
            let fits = wanted(*scope).is_some_and(|(model, effort)| s.key.model == model && s.key.effort == effort);
            if !fits {
                slot.spare = None;
                ended.push(*scope);
            }
        }
        ended.sort_by_key(|s| s.id());
        ended
    }

    /// The spares idle [`IDLE`] by `now` (or `idle`), ended; the scopes.
    pub fn sweep(&mut self, now: Instant, idle: Duration) -> Vec<Scope> {
        let mut ended = Vec::new();
        for (scope, slot) in self.slots.iter_mut() {
            if slot.spare.is_some() && slot.used.is_none_or(|t| now.saturating_duration_since(t) >= idle) {
                slot.spare = None;
                ended.push(*scope);
            }
        }
        ended.sort_by_key(|s| s.id());
        ended
    }

    /// Whether `scope` has a spare waiting (tests, the e2e report: debug builds).
    #[cfg(debug_assertions)]
    pub fn has(&self, scope: Scope) -> bool {
        self.slots.get(&scope).is_some_and(|s| s.spare.is_some())
    }
}

/// The app's spares.
static SPARES: Mutex<Option<Spares>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Spares) -> R) -> R {
    let mut g = SPARES.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(Spares::default))
}

/// The scopes with spares (Chat's one-off calls run cold, as before: its
/// turns have their own warm sessions).
pub fn spares_for(scope: Scope) -> bool {
    scope != Scope::Chat
}

/// **The spare for a call**, taken: `None` and the call starts its own.
pub fn take(scope: Scope, key: &Key) -> Option<Spare> {
    if !spares_for(scope) {
        return None;
    }
    with(|s| s.take(scope, key))
}

/// **After a call of `scope`** (its prompt sent): its system prompt kept,
/// and the next spare started behind it, in the background.
pub fn after_call(scope: Scope, system: &str) {
    if !spares_for(scope) {
        return;
    }
    with(|s| s.note(scope, system));
    refill(scope);
}

/// **A key press that may lead to a call of `scope`** (a dictation's, the
/// rewrite shortcut's): a spare started now if the scope's on the plan and
/// none is waiting, as the on-device model loads when a dictation starts.
/// Nothing the first time (no prompt seen yet this run).
pub fn warm(scope: Scope) {
    if !spares_for(scope) {
        return;
    }
    with(|s| s.touch(scope));
    refill(scope);
}

/// The model and effort `scope`'s spare runs with in `cfg`, when it should
/// have one: on the plan, the plan connected. Pure.
pub fn wanted(cfg: &crate::config::YapConfig, scope: Scope) -> Option<(String, Option<Effort>)> {
    if !spares_for(scope) || !cfg.pp_api_keys.get(super::PROVIDER).is_some_and(|k| k == super::PLAN_KEY) {
        return None;
    }
    let model = super::scope_model(cfg, scope)?;
    let effort = super::oneshot::effort_for_scope(scope, &model);
    Some((model, effort))
}

/// Start `scope`'s next spare in the background, when it should have one.
fn refill(scope: Scope) {
    let cfg = crate::config::load();
    let Some((model, effort)) = wanted(&cfg, scope) else { return };
    let Some((key, system)) = with(|s| s.plan_start(scope, &model, effort)) else { return };
    let spawned = std::thread::Builder::new().name("claude-plan-spare".into()).spawn(move || {
        let spare = start(&key, &system);
        let ok = spare.is_some();
        // Still wanted? (A save, a sign-out or a call may have come since.)
        let still = wanted(&crate::config::load(), scope).map(|(m, e)| Key { model: m, effort: e, system: key.system });
        with(|s| s.started(scope, spare, still.as_ref()));
        if ok {
            tracing::info!(scope = scope.id(), "Claude plan: a spare is waiting");
        } else {
            tracing::info!(scope = scope.id(), "Claude plan: a spare wouldn't start");
        }
    });
    if spawned.is_err() {
        with(|s| s.started(scope, None, None));
    }
}

/// One spare, started on the person's Claude Code.
fn start(key: &Key, system: &str) -> Option<Spare> {
    let spec: Spec = super::oneshot::call_spec(&key.model, key.effort, system).ok()?;
    let (session, rx) = Session::start(spec).ok()?;
    Some(Spare { key: key.clone(), session, rx })
}

/// Every spare ended (`why`: the log's).
pub fn end_all(why: &'static str) {
    let ended = with(|s| s.end_all());
    if !ended.is_empty() {
        tracing::info!(count = ended.len(), why, "Claude plan: spares ended");
    }
}

/// **A config just saved**: the spares of scopes no longer on the plan, or
/// on another model or effort, end now.
pub fn config_changed(cfg: &crate::config::YapConfig) {
    let ended = with(|s| s.keep_only(|scope| wanted(cfg, scope)));
    if !ended.is_empty() {
        tracing::info!(count = ended.len(), "Claude plan: spares ended (the scope changed)");
    }
}

/// lib.rs: the idle sweep, every half a minute.
pub fn start_sweeping() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("claude-plan-spares".into()).spawn(|| loop {
            std::thread::sleep(Duration::from_secs(30));
            let ended = with(|s| s.sweep(Instant::now(), idle()));
            if !ended.is_empty() {
                tracing::info!(count = ended.len(), "Claude plan: idle spares ended");
            }
        });
    });
}

/// [`IDLE`], or a test run's (`e2e_claude_spares {idleMs}`).
fn idle() -> Duration {
    #[cfg(debug_assertions)]
    {
        let ms = TEST_IDLE_MS.load(std::sync::atomic::Ordering::SeqCst);
        if ms > 0 {
            return Duration::from_millis(ms);
        }
    }
    IDLE
}

#[cfg(debug_assertions)]
static TEST_IDLE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Debug-only, test mode: which scopes have a spare waiting now; `idleMs`
/// sets the idle limit (0: the usual) and `sweep` ends the idle ones now.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_claude_spares(idle_ms: Option<u64>, sweep: Option<bool>) -> Result<serde_json::Value, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_claude_spares is for test runs".into());
    }
    if let Some(ms) = idle_ms {
        TEST_IDLE_MS.store(ms, std::sync::atomic::Ordering::SeqCst);
    }
    if sweep.unwrap_or(false) {
        with(|s| s.sweep(Instant::now(), idle()));
    }
    let waiting: Vec<&str> = with(|s| Scope::ALL.iter().filter(|sc| s.has(**sc)).map(|sc| sc.id()).collect());
    Ok(serde_json::json!({ "waiting": waiting }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::claude_code::cli::Program;
    use crate::ai::claude_code::events::Event;
    use crate::ai::claude_code::session::Pay;
    use std::path::{Path, PathBuf};

    /// The test binary as Claude Code's stand-in (session.rs' own way).
    fn stand_in(script: &Path, start_ms: u64) -> (Program, Vec<(String, String)>) {
        let prefix = vec![
            "ai::claude_code::session::tests::fake_claude_stand_in".to_string(),
            "--exact".into(),
            "--nocapture".into(),
            "--quiet".into(),
            "--test-threads=1".into(),
            "--".into(),
        ];
        let program = Program::new(std::env::current_exe().unwrap()).with_prefix(prefix);
        (program, vec![("YAP_FAKE_CLAUDE_STAND_IN".into(), script.display().to_string()), ("YAP_FAKE_CLAUDE_START_MS".into(), start_ms.to_string())])
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yap-spare-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One call each: its pid and what it was asked (once a message came:
    /// a spare's earlier output is dropped), then a result.
    const SCRIPT: &str = r#"{"_wait_user":true}
{"_dump_args":true}
{"type":"result","subtype":"success","is_error":false,"result":"answered: {{user_text}}","session_id":"{{session_id}}","total_cost_usd":0,"usage":{"input_tokens":3,"output_tokens":2}}
{"_wait_user":true}
{"type":"result","subtype":"success","is_error":false,"result":"a second call in one process: never","session_id":"{{session_id}}"}
"#;

    fn spare_with(program: &Program, env: &[(String, String)], dir: &Path, key: &Key, system: &str) -> Spare {
        let mut spec = Spec::new(dir, Pay::Plan);
        spec.model = Some(key.model.clone());
        spec.effort = key.effort;
        spec.system_prompt = Some(system.to_string());
        spec.persist = false;
        spec.tools = Some(Vec::new());
        spec.env = env.to_vec();
        let (session, rx) = Session::start_with(program, spec).unwrap();
        Spare { key: key.clone(), session, rx }
    }

    async fn pid_and_answer(spare: &mut Spare, text: &str) -> (u64, String) {
        spare.session.send_user(text, &[]).unwrap();
        spare.session.end();
        let mut pid = 0;
        let got = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(e) = spare.rx.recv().await {
                match e {
                    Event::Other(v) if v["subtype"] == "fake_args" => pid = v["pid"].as_u64().unwrap_or(0),
                    Event::Result(r) => return r.text,
                    _ => {}
                }
            }
            String::new()
        })
        .await
        .unwrap();
        (pid, got)
    }

    #[test]
    fn the_likely_prompt_is_the_most_frequent_lately() {
        let mut s = Spares::default();
        assert!(s.likely(Scope::Cleanup).is_none(), "nothing seen yet: nothing to start");
        for p in ["email style", "chat style", "chat style", "email style", "chat style"] {
            s.note(Scope::Cleanup, p);
        }
        assert_eq!(s.likely(Scope::Cleanup).as_deref().map(String::as_str), Some("chat style"));
        // Equals: the latest.
        s.note(Scope::Commands, "a");
        s.note(Scope::Commands, "b");
        assert_eq!(s.likely(Scope::Commands).as_deref().map(String::as_str), Some("b"));
        // Only the last HISTORY calls count.
        for _ in 0..HISTORY {
            s.note(Scope::Cleanup, "email style");
        }
        assert_eq!(s.likely(Scope::Cleanup).as_deref().map(String::as_str), Some("email style"));
        assert!(spares_for(Scope::Cleanup) && spares_for(Scope::Commands) && spares_for(Scope::Notes) && !spares_for(Scope::Chat));
    }

    #[tokio::test]
    async fn each_spare_serves_one_call_then_a_new_one_starts_behind_it() {
        let dir = temp("one-each");
        let script = dir.join("one-each.jsonl");
        std::fs::write(&script, SCRIPT).unwrap();
        let (program, env) = stand_in(&script, 0);
        let mut spares = Spares::default();
        spares.note(Scope::Cleanup, "Clean up dictations.");

        let mut pids = Vec::new();
        for said in ["first dictation", "second dictation"] {
            let (key, system) = spares.plan_start(Scope::Cleanup, "claude-haiku-5-5", Some(Effort::Low)).expect("a spare to start");
            assert!(spares.plan_start(Scope::Cleanup, "claude-haiku-5-5", Some(Effort::Low)).is_none(), "one at a time");
            let spare = spare_with(&program, &env, &dir, &key, &system);
            spares.started(Scope::Cleanup, Some(spare), Some(&key));
            assert!(spares.has(Scope::Cleanup));
            // Waiting and fit: nothing more to start.
            assert!(spares.plan_start(Scope::Cleanup, "claude-haiku-5-5", Some(Effort::Low)).is_none());
            let mut spare = spares.take(Scope::Cleanup, &key).expect("the spare for this call");
            assert!(!spares.has(Scope::Cleanup), "taken: never another call's");
            let (pid, answer) = pid_and_answer(&mut spare, said).await;
            assert_eq!(answer, format!("answered: {said}"), "its own call's words only");
            pids.push(pid);
            spares.note(Scope::Cleanup, "Clean up dictations.");
            // Its stdin closed: it ends after the one call.
            let ended = tokio::time::timeout(Duration::from_secs(20), async {
                while let Some(e) = spare.rx.recv().await {
                    if matches!(e, Event::Exited(_)) {
                        return true;
                    }
                }
                true
            })
            .await
            .unwrap_or(false);
            assert!(ended && !spare.session.is_running());
        }
        assert!(pids[0] > 0 && pids[1] > 0 && pids[0] != pids[1], "each dictation its own process: {pids:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn another_calls_spare_or_a_dead_one_is_never_used() {
        let dir = temp("mismatch");
        let script = dir.join("mismatch.jsonl");
        std::fs::write(&script, SCRIPT).unwrap();
        let (program, env) = stand_in(&script, 0);
        let mut spares = Spares::default();
        spares.note(Scope::Commands, "Rewrite.");
        let (key, system) = spares.plan_start(Scope::Commands, "claude-haiku-5-5", None).unwrap();
        spares.started(Scope::Commands, Some(spare_with(&program, &env, &dir, &key, &system)), Some(&key));
        // Another prompt, another model: not this spare (the call runs cold).
        assert!(spares.take(Scope::Commands, &Key::of("claude-haiku-5-5", None, "Write a reply.")).is_none());
        assert!(spares.take(Scope::Commands, &Key::of("claude-sonnet-5-5", None, "Rewrite.")).is_none());
        assert!(spares.has(Scope::Commands), "still waiting for its own kind of call");
        // Died while waiting: dropped, and the call runs cold.
        if let Some(s) = spares.slots.get(&Scope::Commands).and_then(|s| s.spare.as_ref()) {
            s.session.kill();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(spares.take(Scope::Commands, &key).is_none());
        assert!(!spares.has(Scope::Commands));
        // A spare that wouldn't start: none tried again for a while.
        assert!(spares.plan_start(Scope::Commands, "claude-haiku-5-5", None).is_some());
        spares.started(Scope::Commands, None, None);
        assert!(spares.plan_start(Scope::Commands, "claude-haiku-5-5", None).is_none());
        // Started for what's no longer wanted (the scope changed meanwhile): ended.
        let mut other = Spares::default();
        other.note(Scope::Notes, "Summarise.");
        let (key, system) = other.plan_start(Scope::Notes, "claude-haiku-5-5", None).unwrap();
        other.started(Scope::Notes, Some(spare_with(&program, &env, &dir, &key, &system)), None);
        assert!(!other.has(Scope::Notes));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spares_end_when_idle_on_sign_out_and_when_the_scope_changes() {
        let dir = temp("ending");
        let script = dir.join("ending.jsonl");
        std::fs::write(&script, SCRIPT).unwrap();
        let (program, env) = stand_in(&script, 0);
        let mut spares = Spares::default();
        let start = |spares: &mut Spares, scope: Scope, model: &str| {
            spares.note(scope, "Prompt.");
            let (key, system) = spares.plan_start(scope, model, None).unwrap();
            spares.started(scope, Some(spare_with(&program, &env, &dir, &key, &system)), Some(&key));
        };
        start(&mut spares, Scope::Cleanup, "claude-haiku-5-5");
        start(&mut spares, Scope::Commands, "claude-haiku-5-5");
        start(&mut spares, Scope::Notes, "claude-sonnet-5-5");
        // Idle: unused past the limit.
        assert!(spares.sweep(Instant::now(), IDLE).is_empty(), "just used");
        assert_eq!(spares.sweep(Instant::now() + IDLE + Duration::from_secs(1), IDLE), vec![Scope::Cleanup, Scope::Commands, Scope::Notes]);
        assert!(!spares.has(Scope::Cleanup));
        // The scope changed: Commands off the plan, Notes on another model.
        start(&mut spares, Scope::Cleanup, "claude-haiku-5-5");
        start(&mut spares, Scope::Commands, "claude-haiku-5-5");
        start(&mut spares, Scope::Notes, "claude-sonnet-5-5");
        let ended = spares.keep_only(|scope| match scope {
            Scope::Cleanup => Some(("claude-haiku-5-5".into(), None)),
            Scope::Notes => Some(("claude-opus-5-5".into(), None)),
            _ => None,
        });
        assert_eq!(ended, vec![Scope::Commands, Scope::Notes]);
        assert!(spares.has(Scope::Cleanup));
        // Signed out: every one.
        assert_eq!(spares.end_all(), vec![Scope::Cleanup]);
        assert!(!spares.has(Scope::Cleanup));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scope_has_a_spare_only_while_its_on_the_plan_and_connected() {
        let mut cfg = crate::config::YapConfig { post_process_enabled: true, pp_provider: super::super::PROVIDER.into(), pp_model: "claude-haiku-5-5".into(), ..Default::default() };
        assert_eq!(wanted(&cfg, Scope::Cleanup), None, "the plan not connected");
        cfg.pp_api_keys.insert(super::super::PROVIDER.into(), super::super::PLAN_KEY.into());
        assert_eq!(wanted(&cfg, Scope::Cleanup).map(|(m, _)| m).as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(wanted(&cfg, Scope::Commands), None, "Commands isn't on the plan");
        cfg.llm_scopes.insert("voiceAgent".into(), crate::config::LlmScope { enabled: true, provider: super::super::PROVIDER.into(), model: "claude-sonnet-5-5".into(), ..Default::default() });
        assert_eq!(wanted(&cfg, Scope::Commands).map(|(m, _)| m).as_deref(), Some("claude-sonnet-5-5"));
        cfg.llm_scopes.insert("chat".into(), crate::config::LlmScope { enabled: true, provider: super::super::PROVIDER.into(), model: "claude-opus-5-5".into(), ..Default::default() });
        assert_eq!(wanted(&cfg, Scope::Chat), None, "Chat's one-off calls run cold");
        cfg.post_process_enabled = false;
        assert_eq!(wanted(&cfg, Scope::Cleanup), None, "cleanup off");
    }

    /// **The gain, measured on the stand-in** with Claude Code's start-up
    /// simulated (`YAP_FAKE_CLAUDE_START_MS`): a call on a cold process
    /// waits for it, a call on a spare doesn't.
    #[tokio::test]
    async fn a_spare_hides_claude_codes_start_up() {
        const START_MS: u64 = 1500;
        let dir = temp("gain");
        let script = dir.join("gain.jsonl");
        std::fs::write(&script, SCRIPT).unwrap();
        let (program, env) = stand_in(&script, START_MS);
        let key = Key::of("claude-haiku-5-5", Some(Effort::Low), "Clean up dictations.");
        // Cold: started when the call comes.
        let cold_at = Instant::now();
        let mut cold = spare_with(&program, &env, &dir, &key, "Clean up dictations.");
        let (_, answer) = pid_and_answer(&mut cold, "cold").await;
        let cold_ms = cold_at.elapsed().as_millis();
        assert_eq!(answer, "answered: cold");
        // Warm: started earlier (after the last call), ready when this one comes.
        let mut warm = spare_with(&program, &env, &dir, &key, "Clean up dictations.");
        tokio::time::sleep(Duration::from_millis(START_MS + 1500)).await;
        let warm_at = Instant::now();
        let (_, answer) = pid_and_answer(&mut warm, "warm").await;
        let warm_ms = warm_at.elapsed().as_millis();
        assert_eq!(answer, "answered: warm");
        println!("Claude plan spare, stand-in with a {START_MS} ms start: cold call {cold_ms} ms, on a spare {warm_ms} ms");
        assert!(cold_ms >= START_MS as u128, "cold waits for the start: {cold_ms} ms");
        assert!(warm_ms + 1000 < cold_ms, "a spare hides it: {warm_ms} ms against {cold_ms} ms");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
