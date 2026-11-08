//! Gating a call: the guards every tool call passes before it runs.

use serde_json::Value;

use crate::chat::tools::{refused_after_outside, Extra, Seen};

/// **The guards before a call runs**, in order: a new document's outline
/// before any search (`hold_web`: this step's searches and page reads
/// wait for `create_document`), the safety rule once outside content is in
/// the turn (`seen`: the web, the screen, a connector's result,
/// [`refused_after_outside`]), a model evaluation's
/// intercept (what would leave Yap: the clipboard), and this turn's
/// documents' own rules (the one already made, asked for again; a section
/// rewritten only in this chat's own documents). `Some`: the call doesn't
/// run, and this is its result; `None`: it runs ([`super::dispatch`]).
pub(super) fn gate(name: &str, args: &Value, hold_web: bool, seen: Seen, extra: &Extra<'_>) -> Option<Value> {
    if hold_web && (crate::web::TOOL_NAMES.contains(&name) || name == crate::chat::pictures::FIND) {
        tracing::info!(tool = %name, "Chat: a new document's outline comes before any search; refused");
        crate::ai::eval::post_fix("outline_first");
        Some(crate::chat::documents::outline_first_refusal())
    } else if let Some(refusal) = refused_after_outside(name, seen, extra.strong) {
        Some(refusal)
    } else if let Some(r) = crate::ai::eval::intercept_tool(name, args) {
        Some(r) // a model evaluation records what would leave Yap (the clipboard)
    } else if let Some(r) = extra.docs.and_then(|d| d.made_already(name, args)) {
        Some(r) // this turn's document, asked for again: the one there is
    } else {
        extra.docs.and_then(|d| d.refuses(name, args)) // a section rewritten only in this chat's own documents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tools::refused_after_web;
    use serde_json::json;

    const NONE: Seen = Seen { web: false, screen: false, connector: false };
    const WEB: Seen = Seen { web: true, screen: false, connector: false };
    const CONNECTOR: Seen = Seen { web: false, screen: false, connector: true };

    #[test]
    fn a_new_documents_outline_holds_the_web_before_the_safety_rule() {
        let extra = Extra::default();
        let held = gate("web_search", &json!({ "query": "a build" }), true, NONE, &extra).expect("refused");
        assert_eq!(held, crate::chat::documents::outline_first_refusal());
        // Held for the outline, whatever the safety rule says.
        assert_eq!(gate("read_page", &json!({}), true, WEB, &extra), Some(crate::chat::documents::outline_first_refusal()));
        assert_eq!(gate(crate::chat::pictures::FIND, &json!({}), true, NONE, &extra), Some(crate::chat::documents::outline_first_refusal()));
        // Not the web's tools: no hold.
        assert_eq!(gate("search_notes", &json!({}), true, NONE, &extra), None);
        assert_eq!(gate("web_search", &json!({}), false, NONE, &extra), None);
    }

    #[test]
    fn after_the_web_only_the_locked_tools_are_refused() {
        let extra = Extra::default();
        assert_eq!(gate("open_app", &json!({ "name": "Teams" }), false, WEB, &extra), refused_after_web("open_app", true, false));
        assert!(gate("create_note", &json!({}), false, WEB, &extra).is_some());
        assert_eq!(gate("search_notes", &json!({}), false, WEB, &extra), None);
        // Before the web: everything runs.
        assert_eq!(gate("open_app", &json!({ "name": "Teams" }), false, NONE, &extra), None);
        // A strong model keeps changing its notes after the web; apps stay off.
        let strong = Extra { strong: true, ..Default::default() };
        assert_eq!(gate("create_note", &json!({}), false, WEB, &strong), None);
        assert!(gate("open_app", &json!({}), false, WEB, &strong).is_some());
    }

    #[test]
    fn a_connectors_result_locks_yaps_own_actions_as_the_web_does() {
        let strong = Extra { strong: true, ..Default::default() };
        for name in ["open_app", "copy_to_clipboard", "remember", "forget"] {
            let r = gate(name, &json!({}), false, CONNECTOR, &strong).expect(name);
            assert!(r["displayText"].as_str().unwrap().ends_with("not after a connector's result"), "{r}");
            assert!(r["error"].as_str().unwrap().contains("a connector's result came in"), "{r}");
        }
        // An agent gets past the gate: its own card asks (tools/agents.rs).
        for name in ["start_agent", "message_agent"] {
            assert_eq!(gate(name, &json!({}), false, CONNECTOR, &strong), None, "{name}");
            assert_eq!(gate(name, &json!({}), false, WEB, &Extra::default()), None, "{name}");
        }
        // Its notes and documents still change for a strong model; reads run.
        assert_eq!(gate("create_note", &json!({}), false, CONNECTOR, &strong), None);
        assert_eq!(gate("search_notes", &json!({}), false, CONNECTOR, &strong), None);
        // Any other model: every action tool, as after the web.
        assert!(gate("create_note", &json!({}), false, CONNECTOR, &Extra::default()).is_some());
        // The web and a connector both: the connector's said.
        let both = Seen { web: true, connector: true, ..NONE };
        assert!(gate("open_app", &json!({}), false, both, &strong).unwrap()["displayText"].as_str().unwrap().contains("connector"));
    }
}
