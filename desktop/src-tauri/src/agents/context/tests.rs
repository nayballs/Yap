use super::*;

// A recording's shape (2.1.296's `/context` with `--output-format
// stream-json --verbose`), with a fictional setup: Sam's garden-planner.
const STREAM: &str = include_str!("fixtures/stream.jsonl");
const MARKDOWN: &str = include_str!("fixtures/context.md");
const BROKEN: &str = include_str!("fixtures/broken.md");

fn names(b: &Breakdown) -> Vec<&str> {
    b.categories.iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn the_answers_own_object_is_read_first_with_its_exact_counts() {
    let a = parse_output(STREAM);
    assert!(a.local, "no model turn, no cost");
    let b = a.breakdown.expect("a breakdown");
    assert_eq!((b.model.as_str(), b.used, b.window, b.percent), ("claude-opus-5-5", 156_408, 1_000_000, 16.0));
    assert_eq!(b.categories.len(), 12);
    let messages = b.categories.iter().find(|c| c.name == "Messages").unwrap();
    assert_eq!((messages.tokens, messages.kind), (129_046, Kind::Used));
    let kinds: Vec<(&str, Kind)> = b.categories.iter().filter(|c| c.kind != Kind::Used).map(|c| (c.name.as_str(), c.kind)).collect();
    assert_eq!(kinds, [("MCP tools (deferred)", Kind::Deferred), ("System tools (deferred)", Kind::Deferred), ("Autocompact buffer", Kind::Buffer), ("Free space", Kind::Free)]);
    assert_eq!(b.mcp_tools[0], McpTool { name: "mcp__notes__search".into(), server: "notes".into(), tokens: 640 });
    assert_eq!(b.memory_files[1], MemoryFile { kind: "Project".into(), path: r"C:\Projects\garden-planner\CLAUDE.md".into(), tokens: 2912 });
    // Sources worded as the markdown words them.
    assert_eq!(b.agents.iter().map(|a| (a.name.as_str(), a.source.as_str())).collect::<Vec<_>>(), [("reviewer", "User"), ("docs-writer", "Project")]);
    assert_eq!(b.skills.iter().map(|s| (s.source.as_str(), s.tokens)).collect::<Vec<_>>(), [("User", 237), ("Plugin", 904), ("Built-in", 18)]);
}

#[test]
fn the_markdown_is_read_when_the_object_is_missing() {
    // Each line's `context_usage` taken out: the result's markdown is what's left.
    let stripped: String = STREAM
        .lines()
        .map(|l| match serde_json::from_str::<Value>(l) {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.remove("context_usage");
                }
                v.to_string()
            }
            Err(_) => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let a = parse_output(&stripped);
    assert!(a.local);
    let b = a.breakdown.expect("read from the markdown");
    assert_eq!(b, parse_markdown(MARKDOWN).unwrap());
}

#[test]
fn the_markdown_reads_every_table() {
    let b = parse_markdown(MARKDOWN).expect("a breakdown");
    assert_eq!((b.model.as_str(), b.used, b.window, b.percent), ("claude-opus-5-5", 156_400, 1_000_000, 16.0));
    assert_eq!(
        names(&b),
        [
            "System prompt",
            "System tools",
            "MCP tools",
            "MCP server instructions",
            "MCP tools (deferred)",
            "System tools (deferred)",
            "Custom agents",
            "Memory files",
            "Skills",
            "Messages",
            "Free space",
            "Autocompact buffer"
        ]
    );
    let free = b.categories.iter().find(|c| c.kind == Kind::Free).unwrap();
    assert_eq!(free.tokens, 810_600);
    assert_eq!(b.categories.iter().filter(|c| c.kind == Kind::Deferred).count(), 2);
    assert_eq!(b.mcp_tools.len(), 3);
    assert_eq!(b.mcp_tools[2], McpTool { name: "mcp__calendar__list_events".into(), server: "calendar".into(), tokens: 1000 });
    // Its type is the first column, its path the second.
    assert_eq!(b.memory_files[0], MemoryFile { kind: "User".into(), path: r"C:\Users\sam\.claude\CLAUDE.md".into(), tokens: 700 });
    assert_eq!(b.agents[1], CustomAgent { name: "docs-writer".into(), source: "Project".into(), tokens: 100 });
    assert_eq!(b.skills.iter().map(|s| (s.name.as_str(), s.source.as_str(), s.tokens)).collect::<Vec<_>>(), [("release-notes", "User", 240), ("pdf", "Plugin (docs)", 900), ("init", "Built-in", 20)]);
}

#[test]
fn a_reordered_table_is_read_by_its_headers() {
    let md = "## Context Usage\n\n**Tokens:** 10k / 200k (5%)\n\n### Estimated usage by category\n\n| Percentage | Tokens | Category |\n|---|---|---|\n| 4.0% | 8k | Messages |\n| 1.0% | 2k | System prompt |\n";
    let b = parse_markdown(md).unwrap();
    assert_eq!(b.categories[0], Category { name: "Messages".into(), tokens: 8000, kind: Kind::Used });
    assert_eq!((b.used, b.window, b.percent), (10_000, 200_000, 5.0));
    assert!(b.mcp_tools.is_empty() && b.skills.is_empty());
}

#[test]
fn a_broken_answer_gives_nothing_never_a_guess() {
    assert_eq!(parse_markdown(BROKEN), None);
    assert_eq!(parse_markdown(""), None);
    assert_eq!(parse_markdown("## Context Usage\n\n**Tokens:** 1k / 0 (0%)\n"), None, "no window");
    assert_eq!(parse_markdown("## Context Usage\n\n**Tokens:** 1k / 200k (1%)\n"), None, "no categories");
    let out = format!("{}\n", serde_json::json!({"type": "result", "subtype": "success", "num_turns": 0, "total_cost_usd": 0, "result": BROKEN}));
    assert_eq!(parse_output(&out), Answer { breakdown: None, local: true });
    assert_eq!(parse_output("garbage\n{not json"), Answer::default());
    // An object without a window or categories: the markdown, else nothing.
    assert_eq!(from_usage(&serde_json::json!({"total_tokens": 5, "categories": [{"name": "Messages", "tokens": 5}]})), None);
    assert_eq!(from_usage(&serde_json::json!({"raw_max_tokens": 200000, "categories": []})), None);
}

#[test]
fn an_answer_that_took_a_model_turn_or_cost_anything_isnt_local() {
    let turn = serde_json::json!({"type": "result", "subtype": "success", "num_turns": 1, "total_cost_usd": 0.0123, "result": MARKDOWN});
    let a = parse_output(&turn.to_string());
    assert!(!a.local);
    assert!(a.breakdown.is_some(), "read, but not used: `run` throws it away");
    let errored = serde_json::json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "num_turns": 0, "total_cost_usd": 0});
    assert!(!parse_output(&errored.to_string()).local);
    assert!(!parse_output(&serde_json::json!({"type": "result", "result": MARKDOWN}).to_string()).local, "no turns or cost said: not known to be free");
}

#[test]
fn token_counts_as_written() {
    for (raw, want) in [("628", 628), ("4,324", 4324), ("15k", 15_000), ("32.2k", 32_200), ("934.8k", 934_800), ("1m", 1_000_000), ("1M", 1_000_000), ("~240", 240), ("< 20", 20), (" 3.5K ", 3500)] {
        assert_eq!(count(raw), Some(want), "{raw}");
    }
    for raw in ["", "lots", "-5", "k"] {
        assert_eq!(count(raw), None, "{raw}");
    }
}

#[test]
fn a_category_kind_from_its_word_or_its_name() {
    assert_eq!(Kind::of(Some("deferred"), "MCP tools"), Kind::Deferred);
    assert_eq!(Kind::of(Some("something new"), "Free space"), Kind::Free);
    assert_eq!(Kind::of(None, "System tools (deferred)"), Kind::Deferred);
    assert_eq!(Kind::of(None, "Autocompact buffer"), Kind::Buffer);
    assert_eq!(Kind::of(None, "Messages"), Kind::Used);
}

#[test]
fn the_run_resumes_the_session_without_writing_to_it() {
    let a = args("3f6c2a10-0000-4000-8000-00000000c0de", Some("claude-opus-5-5"), Some(Path::new(r"C:\Temp\yap-claude\x\settings.json")));
    assert_eq!(&a[..2], ["-p", "/context"]);
    let at = a.iter().position(|x| x == "--resume").unwrap();
    assert_eq!(a[at + 1], "3f6c2a10-0000-4000-8000-00000000c0de");
    assert!(a.iter().any(|x| x == "--no-session-persistence"), "never written into the session");
    assert!(!a.iter().any(|x| x == "--disable-slash-commands" || x == "--fork-session"));
    let m = a.iter().position(|x| x == "--model").unwrap();
    assert_eq!(a[m + 1], "claude-opus-5-5");
    assert_eq!(a.last().unwrap(), r"C:\Temp\yap-claude\x\settings.json");
    // No model: Claude Code's own default; nothing on the line but text (an npm shim takes it).
    let plain = args("s", Some("  "), None);
    assert!(!plain.iter().any(|x| x == "--model" || x == "--settings"));
    assert!(plain.iter().all(|x| cli::batch_safe(x)));
    assert_eq!(settings(), serde_json::json!({"disableAllHooks": true}));
}

#[test]
fn busy_while_a_turn_is_queued_running_or_asking() {
    let mut a = Agent { session: "s".into(), ..Default::default() };
    for (status, want) in [(Status::Queued, true), (Status::Running, true), (Status::Asking, true), (Status::Done, false), (Status::Failed, false), (Status::Stopped, false), (Status::Interrupted, false)] {
        a.status = status;
        assert_eq!(busy(&a), want, "{status:?}");
    }
    // A turn's end is a new breakdown.
    let before = key_of(&a);
    a.turns += 1;
    a.ended_ts = 42;
    assert_ne!(key_of(&a), before);
}
