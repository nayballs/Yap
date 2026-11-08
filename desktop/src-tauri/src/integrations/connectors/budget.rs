//! **How many of the connectors' tools a model is given** (pure): their
//! schemas are sent with every step of a turn, so together they may take at
//! most a tenth of the model's context and never more than [`MAX`] tokens
//! ([`cap`]; 15% of a free plan's tokens a minute under `budget::rate_cap`,
//! about 1,200 on Groq's). Within it every tool is offered as it is
//! ([`Mode::All`]). **Past it, tool search instead of leaving tools out**
//! ([`mode`]):
//!
//! - any provider: two stable tools, `find_tools {query}` (the matching
//!   tools' names, descriptions and schemas, by BM25 over their text:
//!   [`find`]) and `use_tool {tool, arguments}` (one run through the same
//!   consent as a direct call) — [`Mode::Search`];
//! - Claude on Anthropic's Messages API: Anthropic's own tool search
//!   (`tool_search_tool_bm25`), every connector tool sent with
//!   `defer_loading: true`, so Claude loads only what it finds, as Claude
//!   Code does — [`Mode::Deferred`].
//!
//! Either way the list a step sends is the same on every step and every
//! turn (prompt caches, Qwen's checkpoints): the found tools' definitions
//! come back as a result, never as a change to the tools.

use crate::search::rank::{terms, Bm25};

/// The most the connectors' schemas may take, in tokens.
pub const MAX: u64 = 6_000;
/// How many tools `find_tools` returns at most.
pub const FOUND: usize = 5;

/// How the connectors' tools reach the model this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every tool, as it is.
    All,
    /// `find_tools` and `use_tool` in their place.
    Search,
    /// Every tool deferred behind Anthropic's tool search.
    Deferred,
}

/// The connectors' share of a model's `context` (tokens), under a free
/// plan's `rate_cap` when there's one. Pure.
pub fn cap(context: u64, rate_cap: Option<u64>) -> u64 {
    let mut cap = (context / 10).min(MAX);
    if let Some(r) = rate_cap {
        cap = cap.min(r * 15 / 100);
    }
    cap
}

/// How tools costing `total` tokens reach a model with `cap` for them
/// (`messages_api`: Claude on Anthropic's Messages API, which has its own
/// tool search). Pure.
pub fn mode(total: u64, cap: u64, messages_api: bool) -> Mode {
    if total <= cap {
        Mode::All
    } else if messages_api {
        Mode::Deferred
    } else {
        Mode::Search
    }
}

/// **The tools that match `query`**, best first, at most `limit`: BM25 over
/// each tool's text (`docs`: its connector's name, its name, title,
/// description and arguments' names), ties in the list's order. A tool
/// named exactly comes first. Pure.
pub fn find(docs: &[String], names: &[&str], query: &str, limit: usize) -> Vec<usize> {
    let q = query.trim();
    let mut out: Vec<usize> = names.iter().position(|n| n.eq_ignore_ascii_case(q)).into_iter().collect();
    let words = terms(q);
    if !words.is_empty() {
        let bm = Bm25::new(&docs.iter().map(|d| terms(&d.replace(['_', '-'], " "))).collect::<Vec<_>>());
        let mut scored: Vec<(usize, f32)> = (0..docs.len()).map(|i| (i, bm.score(i, &words))).filter(|(_, s)| *s > 0.0).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        out.extend(scored.into_iter().map(|(i, _)| i).filter(|i| !out.contains(i)).collect::<Vec<_>>());
    }
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_is_a_tenth_of_the_context_at_most_6000_and_less_on_a_free_plan() {
        assert_eq!(cap(32_768, None), 3_276);
        assert_eq!(cap(200_000, None), MAX);
        assert_eq!(cap(1_000_000, None), MAX);
        // Groq's free plan: 15% of its tokens a minute (9/10 of 8,000).
        assert_eq!(cap(131_072, Some(7_200)), 1_080);
    }

    #[test]
    fn past_the_cap_tool_search_instead_of_leaving_tools_out() {
        assert_eq!(mode(900, 3_276, false), Mode::All);
        assert_eq!(mode(3_276, 3_276, true), Mode::All);
        assert_eq!(mode(9_000, 3_276, false), Mode::Search);
        assert_eq!(mode(9_000, 3_276, true), Mode::Deferred);
    }

    #[test]
    fn find_ranks_the_tools_by_their_words() {
        let docs = vec![
            "Linear list_issues List issues assigned to someone. assignee state".to_string(),
            "Linear create_issue Create an issue. title description team".to_string(),
            "Notion search Search pages in the workspace. query".to_string(),
            "Notion create_pages Create pages. parent title body".to_string(),
        ];
        let names = ["linear__list_issues", "linear__create_issue", "notion__search", "notion__create_pages"];
        assert_eq!(find(&docs, &names, "create a notion page", 2)[0], 3);
        assert_eq!(find(&docs, &names, "create a notion page", 2).len(), 2);
        assert_eq!(find(&docs, &names, "issues assigned to me", 5)[0], 0);
        // A name asked for exactly comes first.
        assert_eq!(find(&docs, &names, "notion__search", 5)[0], 2);
        // Nothing matches: nothing.
        assert!(find(&docs, &names, "weather tomorrow", 5).is_empty());
        assert!(find(&docs, &names, "  ", 5).is_empty());
    }
}
