//! Gating a call: the guards every tool call passes before it runs.

use serde_json::Value;

use crate::chat::tools::{refused_after_web, Extra};

/// **The guards before a call runs**, in order: a new document's outline
/// before any search (`hold_web`: this step's searches and page reads
/// wait for `create_document`), the safety rule once web content is in
/// the turn (`web_seen`, [`refused_after_web`]), a model evaluation's
/// intercept (what would leave Yap: the clipboard), and this turn's
/// documents' own rules (the one already made, asked for again; a section
/// rewritten only in this chat's own documents). `Some`: the call doesn't
/// run, and this is its result; `None`: it runs ([`super::dispatch`]).
pub(super) fn gate(name: &str, args: &Value, hold_web: bool, web_seen: bool, extra: &Extra<'_>) -> Option<Value> {
    if hold_web && (crate::web::TOOL_NAMES.contains(&name) || name == crate::chat::pictures::FIND) {
        tracing::info!(tool = %name, "Chat: a new document's outline comes before any search; refused");
        crate::ai::eval::post_fix("outline_first");
        Some(crate::chat::documents::outline_first_refusal())
    } else if let Some(refusal) = refused_after_web(name, web_seen, extra.strong) {
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
    use serde_json::json;

    #[test]
    fn a_new_documents_outline_holds_the_web_before_the_safety_rule() {
        let extra = Extra::default();
        let held = gate("web_search", &json!({ "query": "a build" }), true, false, &extra).expect("refused");
        assert_eq!(held, crate::chat::documents::outline_first_refusal());
        // Held for the outline, whatever the safety rule says.
        assert_eq!(gate("read_page", &json!({}), true, true, &extra), Some(crate::chat::documents::outline_first_refusal()));
        assert_eq!(gate(crate::chat::pictures::FIND, &json!({}), true, false, &extra), Some(crate::chat::documents::outline_first_refusal()));
        // Not the web's tools: no hold.
        assert_eq!(gate("search_notes", &json!({}), true, false, &extra), None);
        assert_eq!(gate("web_search", &json!({}), false, false, &extra), None);
    }

    #[test]
    fn after_the_web_only_the_locked_tools_are_refused() {
        let extra = Extra::default();
        assert_eq!(gate("open_app", &json!({ "name": "Teams" }), false, true, &extra), refused_after_web("open_app", true, false));
        assert!(gate("create_note", &json!({}), false, true, &extra).is_some());
        assert_eq!(gate("search_notes", &json!({}), false, true, &extra), None);
        // Before the web: everything runs.
        assert_eq!(gate("open_app", &json!({ "name": "Teams" }), false, false, &extra), None);
        // A strong model keeps changing its notes after the web; apps stay off.
        let strong = Extra { strong: true, ..Default::default() };
        assert_eq!(gate("create_note", &json!({}), false, true, &strong), None);
        assert!(gate("open_app", &json!({}), false, true, &strong).is_some());
    }
}
