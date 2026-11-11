//! **Yap Code** (2026-11-07; called **Jobs** until the same day, so older
//! notes say "job"): from Chat or the Voice Assistant, any chat model hands
//! a task to the person's own Claude Code ("Update the readme in
//! garden-planner"). Each run is **an agent**: Yap runs it in that workspace
//! in the background, follows it, and the conversation's model reports back
//! when it ends — out loud when it was asked by voice. Yap "runs Claude
//! Code"; the feature is never named after it (Anthropic's rule: its name
//! never says "Claude Code" or "Anthropic"). What the person sees calls it
//! [`FEATURE`] (the frontend's `FEATURE_NAME`, `core/codeChats.js`), so a
//! new name is one line on each side. Internally it was `jobs` until the
//! same day: [`moved`] carries an older data folder's files and
//! `config::migrate_once` (v8) its settings over.
//!
//! - [`store`]: `agents.json` (every agent; each chat's Mode; the one-time
//!   Bypass OK), each agent's `agents/<id>/log.jsonl` and `backup/`; agents live
//!   when Yap starts are interrupted; backups and logs go after 7 days.
//! - [`workspaces`]: `workspaces.json`, the folders agents may work in (a cap
//!   each), and which one an agent means — never a guess.
//! - [`modes`]: Claude Code's modes as the Mode chip words them; the chat's
//!   model may only lower its chat's Mode, a folder's cap lowers it, Bypass
//!   needs its one-time OK.
//! - [`runner`]: at most 3 at once, one per folder; each a Claude Code
//!   session with the person's own setup plus Yap's run endpoint (its
//!   permission cards, the backup hook); followed as `yap-agent` events;
//!   stop, message, continue, undo, open in terminal.
//! - [`steps`]: each tool call in plain words (a command by its
//!   description, else tidied); Claude Code's to-do list.
//! - [`diff`]: an edit's lines removed and added, a written file's lines,
//!   within their caps, for the tab's "Files changed".
//! - [`usage`]: what each turn used — its tokens, its context's fill, an
//!   estimate of its cost while it works (the Agent panel's numbers, after
//!   savvy-progress's agents panel).
//! - [`consent`]: the person's own words ask for it (as opening an app).
//! - [`sizing`]: the chat's model sizes each agent (its model, effort and
//!   why); the person's own words win; Settings' optional ceiling.
//! - [`voice`]: the Voice Assistant asks out loud before an agent that needs
//!   asking; the next question is the answer.
//! - [`code_chat`]: Code chats — a conversation attached to one workspace,
//!   kept in `agents.json`; there the workspace defaults to the chat's and
//!   the person's own request is enough.
//! - [`commands`]: the page's commands (Yap Code's screens).
//! - [`approve_card`]: a running agent's permission prompt on the Yap bar
//!   when no Yap window is in front (Allow once / Deny).
//! - [`remote`]: Remote Control's records (`remote_control.json`, 2026-11-08):
//!   which phones and workspaces allow it, the turns a phone asked for (run
//!   at most at Auto, reported on that phone), the audit trail.
//!
//! Chat's side: `chat/tools/agents.rs` (`start_agent`, `agent_status`,
//! `message_agent`, `stop_agent`), `chat/turn/report.rs` (the report turn).
//! Offered only with Yap Code on and Claude Code on this PC
//! ([`offered`]).

pub mod approve_card;
pub mod code_chat;
pub mod commands;
pub mod consent;
pub mod diff;
pub mod direct;
pub mod workspaces;
pub mod modes;
pub mod remote;
pub mod runner;
pub mod sizing;
pub mod steps;
pub mod store;
pub mod usage;
pub mod voice;

pub use store::AgentRef;

/// **The feature's name** as the person sees it, in one place (the
/// frontend's is `FEATURE_NAME`, `core/codeChats.js`). Never "Claude Code"
/// nor "Anthropic" anything.
pub const FEATURE: &str = "Yap Code";

/// What Yap Code's files in the data folder were called before 7 Nov 2026,
/// and are now.
const MOVED: [(&str, &str); 3] = [("jobs.json", "agents.json"), ("job_folders.json", "workspaces.json"), ("jobs", "agents")];

/// **An older data folder's files under their new names** (`jobs.json` →
/// `agents.json`, `job_folders.json` → `workspaces.json`, `jobs/` →
/// `agents/`): each renamed only when its new name isn't taken (never
/// written over: an older file beside a new one stays where it is).
/// Returns how many moved. Logged with counts only.
pub fn moved(data: &std::path::Path) -> usize {
    let (mut moved, mut left) = (0, 0);
    for (old, new) in MOVED {
        let (from, to) = (data.join(old), data.join(new));
        if !from.exists() {
            continue;
        }
        if to.exists() {
            left += 1;
            continue;
        }
        match std::fs::rename(&from, &to) {
            Ok(()) => moved += 1,
            Err(e) => {
                left += 1;
                tracing::warn!(error = %e, "Yap Code: a file from before the rename couldn't take its new name");
            }
        }
    }
    if moved + left > 0 {
        tracing::info!(moved, left, "Yap Code: the data folder's files from before the rename");
    }
    moved
}

/// [`moved`] for Yap's data folder, once a run, before anything reads the
/// files: from setup (lib.rs) and from the stores' first read, whichever
/// comes first. (Unit tests keep their own folders: [`moved`] itself.)
pub fn ensure_moved() {
    #[cfg(not(test))]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            moved(&store::data());
        });
    }
}

/// **A word an older Yap saved for Yap Code**, read as today's: a chat
/// message's `kind` "jobReport" (an agent's report in the chat) and a
/// card's `act` "job" (the card before an agent starts). Anything else as
/// it is.
pub fn new_word(word: String) -> String {
    match word.as_str() {
        "jobReport" => crate::chat::turn::AGENT_REPORT.into(),
        "job" => "agent".into(),
        _ => word,
    }
}

/// `#[serde(deserialize_with = "crate::agents::renamed")]`: [`new_word`].
pub fn renamed<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    <String as serde::Deserialize>::deserialize(d).map(new_word)
}

/// **Agents are offered**: switched on (Settings → Yap Code), and Claude Code is
/// on this PC (found, not run: cheap enough for every prompt).
pub fn offered(cfg: &crate::config::YapConfig) -> bool {
    cfg.agents_enabled && crate::ai::claude_code::cli::find().is_some()
}

/// At start (lib.rs setup): the app's handle; agents the last Yap left
/// running are interrupted; old backups and logs swept, and again daily.
pub fn init(app: &tauri::AppHandle) {
    runner::init(app);
    let interrupted = store::at_start();
    if interrupted > 0 {
        tracing::info!(interrupted, "Agents: interrupted when Yap last quit");
    }
    let _ = std::thread::Builder::new().name("agents-sweep".into()).spawn(|| loop {
        store::sweep(store::now_secs());
        std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
    });
}

/// Quitting (lib.rs `shutdown_cleanup`): running agents end with Yap.
pub fn shutdown() {
    runner::shutdown();
    store::flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-agents-moved-{tag}-{}", store::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_data_folder_from_before_the_rename_moves_over_with_nothing_lost() {
        let data = scratch("old");
        // Jobs' files, as an older Yap left them.
        let old = r#"{"jobs":[{"id":"ab12","folder":"garden-planner","status":"done","text":"Tidied notes.md."}],"modes":{"7":"plan"},"bypassConfirmed":true}"#;
        std::fs::write(data.join("jobs.json"), old).unwrap();
        std::fs::write(data.join("job_folders.json"), r#"{"folders":[{"name":"garden-planner","path":"D:\\Projects\\garden-planner","addedTs":1}]}"#).unwrap();
        std::fs::create_dir_all(data.join("jobs").join("ab12").join("backup")).unwrap();
        std::fs::write(data.join("jobs").join("ab12").join("log.jsonl"), "{\"id\":\"ab12\",\"step\":\"Reading notes.md\"}\n").unwrap();

        assert_eq!(moved(&data), 3);
        assert!(!data.join("jobs.json").exists() && !data.join("job_folders.json").exists() && !data.join("jobs").exists());
        // The store reads the moved file, its `jobs` as its agents.
        let read: store::Store = serde_json::from_str(&std::fs::read_to_string(data.join("agents.json")).unwrap()).unwrap();
        assert_eq!(read.agents.len(), 1);
        assert_eq!((read.agents[0].id.as_str(), read.agents[0].text.as_str()), ("ab12", "Tidied notes.md."));
        assert_eq!(read.modes.get(&7).map(String::as_str), Some("plan"));
        assert!(read.bypass_confirmed);
        assert!(std::fs::read_to_string(data.join("workspaces.json")).unwrap().contains("garden-planner"));
        assert!(data.join("agents").join("ab12").join("log.jsonl").is_file() && data.join("agents").join("ab12").join("backup").is_dir());
        // Saved again, under the new field's name.
        assert!(serde_json::to_value(&read).unwrap().get("agents").is_some());
        // A second start: nothing to move.
        assert_eq!(moved(&data), 0);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_new_file_is_never_written_over() {
        let data = scratch("both");
        std::fs::write(data.join("jobs.json"), r#"{"jobs":[]}"#).unwrap();
        std::fs::write(data.join("agents.json"), r#"{"agents":[{"id":"cd34"}]}"#).unwrap();
        assert_eq!(moved(&data), 0);
        assert!(std::fs::read_to_string(data.join("agents.json")).unwrap().contains("cd34"), "the new one stays");
        assert!(data.join("jobs.json").is_file(), "the old one stays where it was");
        // A folder with nothing from before: nothing happens.
        let empty = scratch("empty");
        assert_eq!(moved(&empty), 0);
        assert!(!empty.join("agents.json").exists());
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn words_an_older_yap_saved_read_as_todays() {
        assert_eq!(new_word("jobReport".into()), crate::chat::turn::AGENT_REPORT);
        assert_eq!(new_word("job".into()), "agent");
        assert_eq!(new_word("press".into()), "press");
        assert_eq!(new_word(String::new()), "");
        // A chat message and a card kept before the rename.
        let m: crate::chat::chats::ChatMessage = serde_json::from_str(r#"{"role":"user","text":"The job in garden-planner: it's done.","kind":"jobReport","ts":1}"#).unwrap();
        assert_eq!(m.kind, crate::chat::turn::AGENT_REPORT);
        let c: crate::chat::pages::act::confirm::Card =
            serde_json::from_str(r#"{"id":1,"label":"garden-planner","state":"allowed","act":"job","job":{"folder":"garden-planner","brief":"Tidy notes.md"}}"#).unwrap();
        assert_eq!(c.act, "agent");
        assert_eq!(c.agent.as_ref().map(|a| a.folder.as_str()), Some("garden-planner"));
        let saved = serde_json::to_value(&c).unwrap();
        assert!(saved.get("job").is_none() && saved["agent"]["brief"] == "Tidy notes.md");
    }
}
