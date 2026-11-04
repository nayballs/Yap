//! `--replies`: **a Command that reads the window in front**
//! (`dictation/command_screen.rs`; Wispr Flow's Instruct Mode) on each
//! `--model`, through the app's own call — `command_screen::bounded` (the
//! window's text as the model gets it), `cleanup_styles::draft_line` (where
//! it goes and its tone), `llm::draft_from_screen` and
//! `command_screen::clean_draft` — over `evals/replies/cases.json`: each
//! case a made-up email or chat window as Yap reads it (its title, the text
//! before the box being written in, a message quoted below the cursor, its
//! other text), a spoken instruction, and what the draft must be:
//!
//! - `all_of`: groups of words, one of each in it (the facts the
//!   instruction and the window give: "Thursday", "the offsite");
//! - `not_contains`: words that mean it went wrong (an invented day, the
//!   window's own lines, an instruction hidden in the mail followed);
//! - `greets`: an email's greeting names the person replied to;
//! - `language` / `not_language`: the conversation's language (some of its
//!   words) and not the instruction's;
//! - `max_words`.
//!
//! Every draft also must not be empty, quote the earlier messages ("wrote:",
//! "From:", "Subject:"), copy a run of [`COPIED`] words from the window, or
//! hold a placeholder ("[Your Name]"); a preamble or quoted history Yap had
//! to strip is counted (`cleaned`).
//!
//! **Phases** (`--replies-phase without|with`): `with` is the Command as it
//! is now; `without` the same instruction from the words alone, as a
//! Command wrote until 31 Oct 2026 (`llm::rewrite_with_samples`, nothing
//! selected). **The body** (`--replies-body commands|none`): the Commands
//! scope's default prompt (`evals/replies/commands-prompt.txt`, a copy of
//! `desktop/src/lib/settings/scopes.js`'s, "Jarvis" for its name: "Hey Jarvis, …"
//! needs the scope on) or none (the rewrite shortcut on the cleanup
//! endpoint).
//!
//! `replies.md` in the run folder; the numbers in `summary.json`
//! (`models[].replies`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::record;
use crate::ai::cleanup_styles::Category;
use crate::dictation::command_screen::{self, Window};

/// A run of this many words from the window, in the draft: copied (a
/// confirmation restates the facts in shorter runs; quoted history copies
/// whole lines).
const COPIED: usize = 16;

/// The phases a run may pick (`--replies-phase`).
pub const ALL_PHASES: [&str; 2] = ["without", "with"];
/// The bodies (`--replies-body`).
pub const ALL_BODIES: [&str; 2] = ["commands", "none"];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowCase {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub quoted: String,
    #[serde(default)]
    pub other: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub instruction: String,
    /// "email" | "workChat" | "personalChat" | "" (anything else): where
    /// it goes (`cleanup_styles::draft_line`).
    #[serde(default)]
    pub kind: String,
    /// The user's name (About you), for an email's sign-off.
    #[serde(default)]
    pub user: String,
    pub window: WindowCase,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub all_of: Vec<Vec<String>>,
    #[serde(default)]
    pub not_contains: Vec<String>,
    #[serde(default)]
    pub greets: String,
    #[serde(default)]
    pub language: Vec<String>,
    #[serde(default)]
    pub not_language: Vec<String>,
    #[serde(default)]
    pub max_words: Option<usize>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.kind.to_lowercase() == *o));
    if cases.is_empty() {
        return Err(format!("no reply cases in {}", path.display()));
    }
    Ok(cases)
}

/// The Commands scope's default prompt, its name filled in ("Jarvis").
pub fn commands_body(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    Ok(text.trim().replace("{{agentName}}", "Jarvis"))
}

pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

fn window_of(c: &Case) -> Window {
    let w = &c.window;
    Window { title: w.title.clone(), before: w.before.clone(), after: w.after.clone(), quoted: w.quoted.clone(), other: w.other.clone() }
}

fn category_of(kind: &str) -> Category {
    match kind {
        "email" => Category::Email,
        "workChat" => Category::WorkChat,
        "personalChat" => Category::PersonalChat,
        _ => Category::Other,
    }
}

/// `needle` in `text` (both lower-cased): a phrase of plain letters as whole
/// words, anything else as it is.
fn found(text: &str, needle: &str) -> bool {
    let plain = needle.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '\'' || c == '-');
    if !plain || needle.trim() != needle || needle.is_empty() {
        return text.contains(needle);
    }
    text.match_indices(needle).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// A run of [`COPIED`] words of `out` that's in `window` and not in the
/// instruction. Pure.
pub fn copied(out: &str, window: &str, instruction: &str) -> Option<String> {
    let w = words(window);
    let said = words(instruction).join(" ");
    let runs: std::collections::HashSet<String> = w.windows(COPIED).map(|r| r.join(" ")).collect();
    words(out).windows(COPIED).map(|r| r.join(" ")).find(|r| runs.contains(r) && !said.contains(r.as_str()))
}

/// Why `out` (as the app would type it) fails `c`, if it does. `window`:
/// what the model was given of the window (none: the words alone).
pub fn judge(c: &Case, out: &str, window: &str) -> Vec<String> {
    let mut why = Vec::new();
    let e = &c.expect;
    if out.trim().is_empty() {
        why.push("empty".into());
        return why;
    }
    let lower = out.to_lowercase();
    for group in &e.all_of {
        if !group.iter().any(|w| found(&lower, &w.to_lowercase())) {
            why.push(format!("none of {group:?}"));
        }
    }
    for n in &e.not_contains {
        if found(&lower, &n.to_lowercase()) {
            why.push(format!("says {n:?}"));
        }
    }
    for n in ["wrote:", "-----original message", "subject:", "[your name]", "<window>", "[name]"] {
        if lower.contains(n) {
            why.push(format!("says {n:?}"));
        }
    }
    if !e.greets.is_empty() {
        let first: String = lower.lines().take(2).collect::<Vec<_>>().join(" ");
        if !found(&first, &e.greets.to_lowercase()) {
            why.push(format!("doesn't greet {}", e.greets));
        }
    }
    if !e.language.is_empty() && !e.language.iter().any(|w| found(&lower, &w.to_lowercase())) {
        why.push(format!("not in the conversation's language (none of {:?})", e.language));
    }
    for n in &e.not_language {
        if found(&lower, &n.to_lowercase()) {
            why.push(format!("in another language ({n:?})"));
        }
    }
    let n = out.split_whitespace().count();
    if let Some(max) = e.max_words {
        if n > max {
            why.push(format!("{n} words (at most {max})"));
        }
    }
    if let Some(run) = copied(out, window, &c.instruction) {
        why.push(format!("copied the window: {run:?}"));
    }
    why
}

/// Every case, in each phase, `repeat` times on one model; its results for
/// summary.json and replies.md.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, phases: &[&'static str], body: &str, repeat: u32) -> Value {
    let mut cfg = crate::config::YapConfig::default();
    crate::ai::cleanup_styles::on_load(&mut cfg);
    let here = crate::ai::ai_model::runs_here(ep.provider, ep.base_url);
    let max = if here { command_screen::MAX_LOCAL } else { command_screen::MAX_CLOUD };
    let mut out_phases = serde_json::Map::new();
    for &phase in phases {
        let mut rows = Vec::new();
        let (mut passed, mut cleaned) = (0usize, 0usize);
        let mut ms_all: Vec<u64> = Vec::new();
        let (mut tokens, mut prompt_tokens) = (0u64, 0u64);
        for c in cases {
            let window = command_screen::bounded(&window_of(c), max);
            let style = crate::ai::cleanup_styles::draft_line(&cfg, category_of(&c.kind), &c.user);
            let rule = format!("{:?}", command_screen::needs(&c.instruction, &[]));
            for rep in 0..repeat.max(1) {
                let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("{phase}/{name}") }));
                record::begin_turn();
                let t0 = Instant::now();
                let asked = if phase == "with" {
                    let d = crate::ai::llm::ScreenDraft {
                        instruction: &c.instruction,
                        window: Some(&window),
                        parts: &[],
                        samples: None,
                        body,
                        style: style.as_deref(),
                    };
                    crate::ai::llm::draft_from_screen(&d, ep.base_url, ep.key, ep.model, ep.provider, false).await
                } else {
                    crate::ai::llm::rewrite_with_samples(&c.instruction, "", None, ep.base_url, ep.key, ep.model, ep.provider, body, false).await
                };
                let ms = t0.elapsed().as_millis() as u64;
                let stats = record::end_turn();
                tokens += stats.total_tokens;
                prompt_tokens += stats.prompt_tokens;
                let (raw, error) = match asked {
                    Ok(r) => (r, None),
                    Err(e) => (String::new(), Some(record::redact(&e))),
                };
                let out = if phase == "with" { command_screen::clean_draft(&raw) } else { raw.trim().to_string() };
                let mut why = judge(c, &out, if phase == "with" { &window } else { "" });
                if let Some(e) = &error {
                    why.insert(0, format!("the call failed: {e}"));
                }
                let was_cleaned = !raw.trim().is_empty() && out != raw.trim();
                cleaned += usize::from(was_cleaned);
                let pass = why.is_empty();
                passed += usize::from(pass);
                if error.is_none() {
                    ms_all.push(ms);
                }
                println!(
                    "  {:<4} {:<8} {:<44} {:.1} s{}{}",
                    if pass { "PASS" } else { "FAIL" },
                    phase,
                    name,
                    ms as f64 / 1000.0,
                    if was_cleaned { " (cleaned)" } else { "" },
                    why.first().map(|w| format!(" | {w}")).unwrap_or_default()
                );
                rows.push(json!({
                    "case": name,
                    "rule": rule,
                    "instruction": c.instruction,
                    "status": if pass { "pass" } else { "fail" },
                    "reasons": why,
                    "ms": ms,
                    "prompt_tokens": stats.prompt_tokens,
                    "completion_tokens": stats.completion_tokens,
                    "cleaned": was_cleaned,
                    "window": if phase == "with" { json!(window) } else { Value::Null },
                    "output": record::redact(&out),
                    "raw": if was_cleaned { json!(record::redact(&raw)) } else { Value::Null },
                }));
            }
        }
        ms_all.sort_unstable();
        let median = ms_all.get(ms_all.len() / 2).copied().unwrap_or(0);
        out_phases.insert(
            phase.to_string(),
            json!({
                "cases": rows.len(),
                "passed": passed,
                "cleaned": cleaned,
                "tokens": tokens,
                "prompt_tokens": prompt_tokens,
                "median_ms": median,
                "max_ms": ms_all.last().copied().unwrap_or(0),
                "rows": rows,
            }),
        );
    }
    Value::Object(out_phases)
}

/// replies.md: a table per model and phase, then each case's instruction,
/// window and draft.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from(
        "# Replies from the window in front\n\n| Model | Phase | Passed | Cleaned | Median / slowest | Prompt tokens | Tokens |\n|---|---|---|---|---|---|---|\n",
    );
    for (label, r) in results {
        for (phase, p) in r.as_object().into_iter().flatten() {
            out.push_str(&format!(
                "| {label} | {phase} | {}/{} | {} | {:.1} / {:.1} s | {} | {} |\n",
                p["passed"],
                p["cases"],
                p["cleaned"],
                p["median_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                p["max_ms"].as_f64().unwrap_or(0.0) / 1000.0,
                p["prompt_tokens"],
                p["tokens"],
            ));
        }
    }
    for (label, r) in results {
        for (phase, p) in r.as_object().into_iter().flatten() {
            out.push_str(&format!("\n## {label} — {phase}\n"));
            for row in p["rows"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "\n### {} — {} (rule: {})\n\nInstruction: \"{}\"\n",
                    row["case"].as_str().unwrap_or(""),
                    row["status"].as_str().unwrap_or("").to_uppercase(),
                    row["rule"].as_str().unwrap_or(""),
                    row["instruction"].as_str().unwrap_or(""),
                ));
                for why in row["reasons"].as_array().into_iter().flatten() {
                    out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
                }
                if let Some(w) = row["window"].as_str() {
                    out.push_str(&format!("\nWindow:\n\n```\n{w}\n```\n"));
                }
                out.push_str(&format!("\nDraft:\n\n```\n{}\n```\n", row["output"].as_str().unwrap_or("")));
                if let Some(raw) = row["raw"].as_str() {
                    out.push_str(&format!("\nThe model wrote (before Yap cleaned it):\n\n```\n{raw}\n```\n"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_file_reads_and_every_case_needs_the_screen_by_the_rule_or_the_model() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/replies");
        let cases = load(&dir.join("cases.json"), &[]).unwrap();
        assert!(cases.len() >= 12, "{}", cases.len());
        for c in &cases {
            assert_ne!(command_screen::needs(&c.instruction, &[]), command_screen::Need::No, "{}: {}", c.name, c.instruction);
            assert!(!command_screen::bounded(&window_of(c), command_screen::MAX_LOCAL).is_empty(), "{}", c.name);
        }
        for kind in ["email", "workChat", "personalChat"] {
            assert!(cases.iter().any(|c| c.kind == kind), "{kind}");
        }
        let body = commands_body(&dir.join("commands-prompt.txt")).unwrap();
        assert!(body.starts_with("You are \"Jarvis\""), "{body}");
    }

    fn case(expect: Expect) -> Case {
        Case { name: "t".into(), instruction: "reply saying Thursday works".into(), kind: "email".into(), user: String::new(), window: WindowCase::default(), expect }
    }

    #[test]
    fn a_draft_is_judged_by_its_facts_greeting_language_and_what_it_copied() {
        let e = Expect {
            all_of: vec![vec!["thursday".into()]],
            not_contains: vec!["friday".into()],
            greets: "Sam".into(),
            language: vec!["thanks".into(), "works".into()],
            not_language: vec!["jeudi".into()],
            max_words: Some(20),
        };
        let c = case(e);
        assert!(judge(&c, "Hi Sam,\n\nThursday works for me.\n\nBest,\nAlex", "").is_empty());
        let why = judge(&c, "Bonjour,\n\nJeudi me va, vendredi aussi. Friday.\n\nOn Tue, Sam wrote:", "");
        assert!(why.iter().any(|w| w.starts_with("none of")), "{why:?}");
        assert!(why.iter().any(|w| w.contains("friday")), "{why:?}");
        assert!(why.iter().any(|w| w.contains("wrote:")), "{why:?}");
        assert!(why.iter().any(|w| w.starts_with("doesn't greet")), "{why:?}");
        assert!(why.iter().any(|w| w.starts_with("in another language")), "{why:?}");
        assert_eq!(judge(&c, "  ", ""), ["empty"]);
        // A run copied from the window, not from the instruction.
        let window = "Could we move the Q3 review to Thursday afternoon instead of Wednesday please, the finance pack is late again";
        assert!(copied("Sure: could we move the Q3 review to Thursday afternoon instead of Wednesday please, the finance pack is late?", window, "").is_some());
        assert!(copied("Thursday afternoon works for me, see you then.", window, "").is_none());
    }
}
