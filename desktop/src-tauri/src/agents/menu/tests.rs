use super::*;
use crate::ai::claude_code::events;
use crate::ai::claude_code::session::{self, Files};

/// A run's `system/init` as 2.1.296 sends it (made-up names and paths).
fn init_line() -> Value {
    json!({
        "type": "system", "subtype": "init",
        "cwd": r"C:\Users\imogen\Projects\garden-planner", "session_id": "3f6c2a10-0000-4000-8000-00000000c0de",
        "tools": ["Read", "mcp__claude_ai_Example_Docs__read", "mcp__issue-tracker__list"],
        "mcp_servers": [
            {"name": "claude.ai Example Docs", "status": "connected", "source": "claudeai"},
            {"name": "issue-tracker", "status": "pending", "source": "user"},
            {"name": "plugin:garden-tools:garden", "status": "needs-auth", "source": "plugin"},
            {"name": "yap", "status": "connected", "source": "user"}
        ],
        "model": "claude-opus-5-5", "permissionMode": "default",
        "slash_commands": ["tidy-beds", "plant-plan", "garden-tools:water", "garden-tools:prune", "compact", "doctor", "__internal-thing", "review", "garden-kit:soil-test"],
        "terminal_slash_commands": ["doctor"],
        "skills": ["plant-plan", "garden-tools:prune"],
        "plugins": [
            {"name": "garden-tools", "path": r"C:\Users\imogen\.claude\plugins\cache\example-market\garden-tools\1.2.0", "source": "garden-tools@example-market", "version": "1.2.0"},
            {"name": "cc-plugin-example", "path": r"C:\x", "source": "cc-plugin-example@builtin", "version": "0.0.1"}
        ],
        "apiKeySource": "none", "claude_code_version": "2.1.296"
    })
}

/// The `commands_changed` after it.
fn details() -> Vec<Value> {
    serde_json::from_value(json!([
        {"name": "tidy-beds", "description": "Tidy the flower beds' notes", "argumentHint": "[bed]"},
        {"name": "plant-plan", "description": "Plan this season's planting.\n  Two lines.", "argumentHint": ""},
        {"name": "garden-tools:water", "description": "(garden-tools) Water the seedlings", "argumentHint": "<bed>"},
        {"name": "garden-tools:prune", "description": "(garden-tools) Prune what's overgrown", "argumentHint": "", "aliases": ["prune"]},
        {"name": "compact", "description": "Free up context by summarizing the conversation so far", "argumentHint": "<optional custom summarization instructions>", "builtin": true},
        {"name": "doctor", "description": "Health-check the setup", "builtin": true},
        {"name": "review", "description": "(removed) Use code-review instead", "builtin": true},
        {"name": "soil-test", "description": "Test the soil's pH", "argumentHint": "", "aliases": ["garden-kit:soil-test"]}
    ]))
    .unwrap()
}

fn init() -> Init {
    let events::Event::Init(i) = events::from_value(init_line()) else { panic!("an init") };
    i
}

#[test]
fn the_init_event_carries_its_commands_skills_plugins_and_servers() {
    let i = init();
    assert_eq!(i.slash_commands.len(), 9);
    assert_eq!(i.terminal_slash_commands, ["doctor"]);
    assert_eq!(i.skills, ["plant-plan", "garden-tools:prune"]);
    assert_eq!(i.plugins[0].name, "garden-tools");
    assert_eq!(i.plugins[0].source, "garden-tools@example-market");
    assert_eq!(i.plugins[0].version, "1.2.0");
    assert_eq!(i.mcp_servers[1].source, "user");
    // An older Claude Code's init (none of them): empty, never an error.
    let events::Event::Init(old) = events::from_value(json!({"type": "system", "subtype": "init", "session_id": "s"})) else { panic!() };
    assert!(old.slash_commands.is_empty() && old.plugins.is_empty() && old.skills.is_empty());
}

#[test]
fn what_a_run_offers_leaves_out_the_terminals_commands_plumbing_and_yaps_own_server() {
    let o = from_init(&init(), None, 100);
    let names: Vec<&str> = o.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["tidy-beds", "plant-plan", "garden-tools:water", "garden-tools:prune", "compact", "review", "garden-kit:soil-test"]);
    let c = |n: &str| o.commands.iter().find(|c| c.name == n).unwrap().clone();
    assert!(c("plant-plan").skill && !c("tidy-beds").skill);
    assert_eq!(c("garden-tools:water").plugin, "garden-tools");
    assert_eq!(c("tidy-beds").plugin, "");
    // Claude Code's own plugins aren't the person's to see.
    assert_eq!(o.plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["garden-tools@example-market"]);
    // Yap's own endpoint is never one to switch off; the others by their own names.
    assert_eq!(o.servers.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Example Docs", "issue-tracker", "garden"]);
    assert_eq!(o.servers[2].status, "needs-auth");
    assert_eq!(o.from, "init");
    assert_eq!(o.at, 100);

    // Then commands_changed: each one's line and hint; one it says was removed goes.
    let o = with_details(o, &details());
    let c = |n: &str| o.commands.iter().find(|c| c.name == n).cloned();
    assert_eq!(c("tidy-beds").unwrap().hint, "[bed]");
    assert_eq!(c("plant-plan").unwrap().description, "Plan this season's planting. Two lines.", "one line");
    assert_eq!(c("garden-tools:water").unwrap().description, "Water the seedlings", "its plugin's name off");
    assert!(c("compact").unwrap().builtin);
    assert!(c("review").is_some() && !shown(o.clone()).commands.iter().any(|c| c.name == "review"), "removed: kept, never shown");
    assert!(c("plant-plan").unwrap().skill, "still a skill");
    // A name the init gives that the details know by another (`anthropic-skills:docs` is `docs`): its line all the same.
    assert_eq!(c("garden-kit:soil-test").unwrap().description, "Test the soil's pH");

    // The next run's init keeps what was known of each; the removed one stays hidden.
    let again = from_init(&init(), Some(&o), 200);
    assert_eq!(again.commands.iter().find(|c| c.name == "tidy-beds").unwrap().description, "Tidy the flower beds' notes");
    assert!(again.commands.iter().find(|c| c.name == "compact").unwrap().builtin);
    assert_eq!(again.commands.iter().find(|c| c.name == "garden-kit:soil-test").unwrap().description, "Test the soil's pH");
    assert!(!shown(again).commands.iter().any(|c| c.name == "review"));
}

#[test]
fn a_look_lists_initializes_commands_and_mcp_statuss_servers() {
    let servers: Vec<Value> = serde_json::from_value(json!([
        {"name": "claude.ai Example Docs", "status": "connected", "scope": "claudeai", "source": "claudeai", "config": {"type": "claudeai-proxy"}},
        {"name": "yap", "status": "connected", "scope": "user", "source": "user"},
        {"name": "issue-tracker", "status": "failed", "scope": "local"}
    ]))
    .unwrap();
    let plugins = vec![Plugin { name: "garden-tools".into(), id: "garden-tools@example-market".into(), ..Default::default() }];
    let o = from_probe(&details(), &servers, plugins, 50);
    // (A look's list has no terminal field: 2.1.296's four are left out until a run says which.)
    assert_eq!(shown(o.clone()).commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["tidy-beds", "plant-plan", "garden-tools:water", "garden-tools:prune", "compact", "soil-test"]);
    assert_eq!(o.commands[2].plugin, "garden-tools");
    assert_eq!(o.servers.iter().map(|s| (s.label.as_str(), s.source.as_str())).collect::<Vec<_>>(), [("Example Docs", "claudeai"), ("issue-tracker", "local")]);
    assert_eq!(o.from, "probe");
    // A run's init after a look: the list is the run's, the lines kept.
    let run = from_init(&init(), Some(&o), 60);
    assert_eq!(run.commands.iter().find(|c| c.name == "garden-tools:water").unwrap().description, "Water the seedlings");
}

#[test]
fn a_whole_servers_rule_is_its_name_as_claude_code_names_its_tools() {
    assert_eq!(tool_rule("issue-tracker"), "mcp__issue-tracker");
    assert_eq!(tool_rule("claude.ai Example Docs"), "mcp__claude_ai_Example_Docs");
    assert_eq!(tool_rule("plugin:garden-tools:garden"), "mcp__plugin_garden-tools_garden");
    assert_eq!(label_of("claude.ai Example Docs"), "Example Docs");
    assert_eq!(label_of("plugin:garden-tools:garden"), "garden");
    assert_eq!(label_of("issue-tracker"), "issue-tracker");
}

fn no_files() -> Files {
    Files { settings: None, mcp_config: None, system_prompt: None, own_prompt: None }
}

#[test]
fn a_chats_servers_off_and_folders_reach_its_runs_arguments() {
    let mut spec = Spec::new(r"C:\Users\imogen\Projects\garden-planner", Pay::Plan);
    // (A read-only turn's own, first: kept.)
    spec.disallowed_tools = vec!["Edit".into()];
    let choices = Choices { folders: vec![r"D:\Shared\seeds".into(), r"D:\Gone".into()], servers_off: vec!["claude.ai Example Docs".into(), "issue-tracker".into()], clears: Vec::new() };
    apply_choices(&mut spec, &choices, &|p| !p.ends_with("Gone"));
    let a = session::args(&spec, &no_files());
    let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].as_str());
    assert_eq!(after("--disallowedTools"), Some("Edit,mcp__claude_ai_Example_Docs,mcp__issue-tracker"));
    let at = a.iter().position(|x| x == "--add-dir").unwrap();
    assert_eq!(a[at + 1], r"D:\Shared\seeds");
    assert!(!a.iter().any(|x| x == r"D:\Gone"), "a folder gone from the PC isn't given");
    // Twice: nothing doubled.
    apply_choices(&mut spec, &choices, &|p| !p.ends_with("Gone"));
    assert_eq!(spec.disallowed_tools.len(), 3);
    assert_eq!(spec.add_dirs.len(), 1);
    // Nothing chosen: nothing added.
    let mut plain = Spec::new(r"C:\x", Pay::Plan);
    apply_choices(&mut plain, &Choices::default(), &|_| true);
    let a = session::args(&plain, &no_files());
    assert!(!a.iter().any(|x| x == "--add-dir" || x == "--disallowedTools"));
}

fn guard() -> Guard {
    Guard {
        system: vec![norm(r"C:\Windows"), norm(r"C:\Program Files"), norm(r"C:\Program Files (x86)"), norm(r"C:\ProgramData")],
        data: norm(r"C:\Users\imogen\AppData\Roaming\yap"),
        users: Some(norm(r"C:\Users")),
        workspace: norm(r"C:\Users\imogen\Projects\garden-planner"),
    }
}

#[test]
fn a_chats_folder_follows_the_workspace_rules() {
    let g = guard();
    assert_eq!(refused(r"D:\Shared\seeds", &g), None);
    assert_eq!(refused(r"C:\Users\imogen\Projects\allotment", &g), None, "a sibling of the workspace");
    assert_eq!(refused(r"C:\Users\imogen\AppData\Roaming\yap-notes", &g), None, "a name that only starts like Yap's");
    assert!(refused("seeds", &g).is_some(), "a full path only");
    assert!(refused(r"D:\", &g).unwrap().contains("whole drive"));
    assert!(refused("D:", &g).is_some());
    assert!(refused(r"\\nas\share", &g).unwrap().contains("whole drive"));
    assert_eq!(refused(r"\\nas\share\seeds", &g), None);
    assert!(refused(r"C:\Windows", &g).unwrap().contains("Windows' own"));
    assert!(refused(r"c:/windows/system32", &g).unwrap().contains("Windows' own"));
    assert!(refused(r"C:\Program Files\Example", &g).unwrap().contains("Windows' own"));
    assert!(refused(r"C:\ProgramData\x", &g).is_some());
    assert!(refused(r"C:\Users\imogen\AppData\Roaming\yap", &g).unwrap().contains("Yap's own data"));
    assert!(refused(r"C:\Users\imogen\AppData\Roaming\yap\agents", &g).unwrap().contains("Yap's own data"), "inside it");
    assert!(refused(r"C:\Users\imogen\AppData", &g).unwrap().contains("Yap's own data"), "holding it");
    assert!(refused(r"C:\Users\imogen", &g).unwrap().contains("Yap's own data"), "the profile holds it");
    assert!(refused(r"C:\Users", &g).is_some());
    assert!(refused(r"C:\Users\imogen\Projects\garden-planner\src", &g).unwrap().contains("workspace already"));
    assert!(refused(r"C:\Users\imogen\Projects\garden-planner", &g).unwrap().contains("workspace already"));
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut raw = [0u8; 4];
    let _ = getrandom::fill(&mut raw);
    let d = std::env::temp_dir().join(format!("yap-menu-test-{tag}-{:02x}{:02x}{:02x}{:02x}", raw[0], raw[1], raw[2], raw[3]));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_new_chats_choices_wait_for_its_conversation_and_go_with_it() {
    let ws = temp_dir("ws");
    let seeds = temp_dir("seeds");
    let wsp = ws.display().to_string();
    // Before its first question: kept for the workspace.
    add_folder(None, &wsp, &seeds).unwrap();
    set_server(None, Some(&wsp), "issue-tracker", false).unwrap();
    assert_eq!(choices(None, Some(&wsp)).servers_off, ["issue-tracker"]);
    assert!(set_server(None, Some(&wsp), "yap", false).is_err(), "Yap's own endpoint");
    // Added twice: once.
    add_folder(None, &wsp, &seeds).unwrap();
    assert_eq!(choices(None, Some(&wsp)).folders.len(), 1);
    // Inside the workspace: refused.
    let inside = ws.join("src");
    std::fs::create_dir_all(&inside).unwrap();
    assert!(add_folder(None, &wsp, &inside).is_err());
    // The conversation made: its own now, the workspace's slot empty again.
    adopt_new(41, &wsp);
    assert_eq!(choices(Some(41), None).folders, [seeds.display().to_string()]);
    assert_eq!(choices(Some(41), None).servers_off, ["issue-tracker"]);
    assert_eq!(choices(None, Some(&wsp)), Choices::default());
    // A fork starts with them; the server back on in one leaves the other.
    forked(41, 42);
    set_server(Some(42), Some(&wsp), "issue-tracker", true).unwrap();
    assert!(choices(Some(42), None).servers_off.is_empty());
    assert_eq!(choices(Some(41), None).servers_off, ["issue-tracker"]);
    // A folder taken off; a chat deleted loses them.
    remove_folder(Some(41), Some(&wsp), &seeds.display().to_string().to_uppercase()).unwrap();
    assert!(choices(Some(41), None).folders.is_empty());
    chat_deleted(41);
    assert_eq!(choices(Some(41), None), Choices::default());
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&seeds);
}

#[test]
fn a_workspaces_menu_is_kept_and_looked_at_again_once_stale_or_old() {
    let ws = r"C:\Users\imogen\Projects\garden-planner";
    let (none, again) = known(ws, 1_000);
    assert!(none.is_none() && again, "nothing known: look");
    learn_init(ws, &init());
    let (found, _) = known(&ws.to_uppercase(), super::super::store::now_secs());
    let found = found.unwrap();
    assert_eq!(found.commands.len(), 7);
    learn_commands(ws, &json!({"type": "system", "subtype": "commands_changed", "commands": details()}));
    let (found, again) = known(ws, super::super::store::now_secs());
    assert!(!again, "fresh");
    assert_eq!(found.unwrap().commands.iter().find(|c| c.name == "tidy-beds").unwrap().hint, "[bed]");
    // A day on, or a plugin switched in Settings: looked at again.
    assert!(known(ws, super::super::store::now_secs() + KEEP_SECS + 1).1);
    stale();
    assert!(known(ws, super::super::store::now_secs()).1);
}

#[test]
fn a_clear_sets_its_agents_aside_and_never_goes_with_a_fork() {
    assert!(!cleared(51, "a1"));
    set_server(Some(51), Some(r"C:\Users\imogen\Projects\garden-planner"), "issue-tracker", false).unwrap();
    add_clear(51, 4, vec!["a1".into()]);
    assert!(cleared(51, "a1"));
    assert!(!cleared(51, "a2"), "an agent after the clear is the chat's own");
    assert!(!cleared(52, "a1"), "another chat's");
    let ch = choices(Some(51), None);
    assert_eq!(ch.clears.len(), 1);
    assert_eq!(ch.clears[0].at, 4);
    // A second clear later: both kept, in order.
    add_clear(51, 9, vec!["a2".into()]);
    assert!(cleared(51, "a2"));
    assert_eq!(choices(Some(51), None).clears.iter().map(|c| c.at).collect::<Vec<_>>(), [4, 9]);
    // A fork gets the servers off, never the clears (they're this chat's thread and agents).
    forked(51, 53);
    let fork = choices(Some(53), None);
    assert_eq!(fork.servers_off, ["issue-tracker"]);
    assert!(fork.clears.is_empty());
    chat_deleted(51);
    assert!(!cleared(51, "a1"));
}
