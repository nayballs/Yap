//! `--chips`: the notepad's newer chips and a Skill on one meeting
//! (`meetings/meeting_summary/chips.rs`, `meetings/meeting_assist.rs`) on
//! each `--model`, sent exactly as the app sends them, over
//! `evals/chips/cases.json`: each case a fictional meeting (title,
//! attendees, typed notes, Yap's summary, transcript lines) and what's
//! asked of it — `smart` (Make me sound smart), `skill` (a built-in Skill's
//! command, and the person's own words with it) or `longer` (Make notes
//! longer) — and what the answer, as the app shows it after its checks,
//! must be:
//!
//! - `contains_any`: one of these words (it's about what was said);
//! - `not_contains`: words that mean it made something up;
//! - `cites` (smart, skill): it cites at least one of the meeting's
//!   passages, and isn't marked as tied to none (made-up numbers are
//!   counted: Yap takes them out);
//! - `max_bullets`: at most this many "- " lines (sound smart: two);
//! - longer: longer than the summary by a fifth, and every task in it kept
//!   (the ones the model dropped before Yap's checks put them back are
//!   counted).
//!
//! `chips.md` in the run folder; the numbers in `summary.json`
//! (`models[].chips`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::meetings::meeting_summary::{self as summary, chips};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// "smart" | "skill" | "longer".
    pub kind: String,
    /// A skill case: the built-in Skill's command.
    #[serde(default)]
    pub skill: String,
    /// A skill case: the person's words after the command.
    #[serde(default)]
    pub words: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub attendees: Vec<String>,
    /// Notes typed beside it.
    #[serde(default)]
    pub typed: String,
    /// Yap's summary of it (longer: what's made longer).
    #[serde(default)]
    pub summary: String,
    /// `[side, words]`: "you" (the person recording) or "them".
    pub lines: Vec<(String, String)>,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub contains_any: Vec<String>,
    #[serde(default)]
    pub not_contains: Vec<String>,
    #[serde(default)]
    pub cites: bool,
    #[serde(default)]
    pub max_bullets: Option<usize>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.kind == *o));
    if cases.is_empty() {
        return Err(format!("no chip cases in {}", path.display()));
    }
    Ok(cases)
}

/// The case's meeting as a note.
pub fn note_of(c: &Case) -> crate::notes::Note {
    let mut note: crate::notes::Note = serde_json::from_value(json!({
        "id": 1, "title": c.title, "content": c.typed, "createdTs": 1_790_000_000, "updatedTs": 1_790_000_000,
        "noteType": "meeting", "participants": c.attendees, "enhancedContent": c.summary,
    }))
    .expect("a note");
    note.transcript = c
        .lines
        .iter()
        .enumerate()
        .map(|(i, (side, text))| crate::notes::TranscriptSegment {
            source: side.clone(),
            text: text.clone(),
            ts: 1_790_000_000 + i as u64 * 20,
            ..Default::default()
        })
        .collect();
    note
}

/// What the app sends for a case.
struct Asked {
    messages: Value,
    cap: u32,
    temperature: f32,
    /// The passages it may cite (none for longer notes).
    passages: Vec<crate::search::ask::Passage>,
    /// Longer notes: what the reply is checked against.
    longer: Option<chips::Longer>,
}

/// What the app sends for case `c`.
fn request(c: &Case, note: &crate::notes::Note) -> Result<Asked, String> {
    match c.kind.as_str() {
        "smart" => {
            let cited = chips::smart_input(note, None).ok_or("too little to go on (no model call)")?;
            let m = summary::helper_messages(crate::ai::llm::MEETING_SMART_PROMPT, &cited.text);
            Ok(Asked { messages: m, cap: chips::SMART_REPLY_TOKENS, temperature: 0.4, passages: cited.passages, longer: None })
        }
        "skill" => {
            let skill = crate::chat::skills::by_command(&c.skill).ok_or_else(|| format!("no Skill /{}", c.skill))?;
            let cited = chips::skill_input(note, &format!("{} {}", skill.instructions, c.words)).ok_or("nothing in the meeting")?;
            let m = crate::meetings::meeting_assist::skill_messages(&skill, &cited.text, &c.words);
            Ok(Asked { messages: m, cap: chips::SKILL_REPLY_TOKENS, temperature: 0.3, passages: cited.passages, longer: None })
        }
        "longer" => {
            let lang = summary::language::for_note(note, &[]);
            let l = chips::longer_request(note, summary::FinalBudget::scaled(1), &lang).ok_or("no summary")?;
            let m = summary::helper_messages(crate::ai::llm::MEETING_LONGER_PROMPT, &l.asked);
            Ok(Asked { messages: m, cap: chips::LONGER_REPLY_TOKENS, temperature: 0.3, passages: Vec::new(), longer: Some(l) })
        }
        k => Err(format!("unknown kind {k}")),
    }
}

/// The "- " lines at the top level.
fn bullets(s: &str) -> usize {
    s.lines().filter(|l| l.starts_with("- ") || l.starts_with("* ")).count()
}

/// The words of every task in `md` (lower case), for "every task kept".
fn task_words(md: &str) -> Vec<String> {
    crate::notes::tasks::parse(md).into_iter().map(|t| t.task.to_lowercase()).collect()
}

/// Why `shown` (the answer as the app shows it) fails `c`, if it does.
/// `raw`: as the model wrote it; `passages`: what it could cite.
pub fn judge(c: &Case, raw: &str, shown: &str, cited: usize, passages: usize) -> Vec<String> {
    let mut why = Vec::new();
    if shown.trim().is_empty() {
        return vec!["empty".into()];
    }
    let lower = shown.to_lowercase();
    if !c.expect.contains_any.is_empty() && !c.expect.contains_any.iter().any(|w| lower.contains(&w.to_lowercase())) {
        why.push(format!("none of {:?}", c.expect.contains_any));
    }
    for n in &c.expect.not_contains {
        if lower.contains(&n.to_lowercase()) {
            why.push(format!("says {n:?}"));
        }
    }
    if c.expect.cites {
        if cited == 0 {
            why.push(format!("cites none of the {passages} passages"));
        }
        if shown.contains(chips::UNCITED_NOTE) {
            why.push("marked as tied to no moment".into());
        }
    }
    if let Some(max) = c.expect.max_bullets {
        let n = bullets(shown);
        if n > max {
            why.push(format!("{n} bullets (at most {max})"));
        }
    }
    if c.kind == "longer" {
        let before = summary::estimate_tokens(&c.summary);
        let after = summary::estimate_tokens(shown);
        if after * 5 < before * 6 {
            why.push(format!("not longer: {after} tokens from {before}"));
        }
        let kept = task_words(shown);
        for t in task_words(&c.summary) {
            if !kept.contains(&t) {
                why.push(format!("lost the task {t:?}"));
            }
        }
        // The meeting copied back into the notes.
        if shown.contains("<meeting>") || shown.lines().any(|l| l.starts_with("You: ") || l.starts_with("Them: ") || l.starts_with("Attendees:")) {
            why.push("echoed the meeting".into());
        }
        // Every heading the notes had, still there.
        for h in c.summary.lines().filter(|l| l.starts_with("## ")) {
            if !shown.lines().any(|l| l.trim().eq_ignore_ascii_case(h.trim())) {
                why.push(format!("lost the heading {h:?}"));
            }
        }
    }
    let _ = raw;
    why
}

/// Every case `repeat` times on one model; its results for summary.json and chips.md.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, repeat: u32) -> Value {
    let mut rows = Vec::new();
    let (mut passed, mut invented, mut dropped_tasks) = (0usize, 0usize, 0usize);
    let mut by_kind: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();
    let mut ms_all: Vec<u64> = Vec::new();
    let mut tokens = 0u64;
    let mep = summary::Endpoint {
        base_url: ep.base_url.to_string(),
        api_key: ep.key.to_string(),
        model: ep.model.to_string(),
        provider: ep.provider.to_string(),
        fragment: String::new(),
        disable_thinking: false,
    };
    for c in cases {
        let note = note_of(c);
        for rep in 0..repeat.max(1) {
            let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
            record::set_context(json!({ "model": ep.model, "scenario": name }));
            record::begin_turn();
            let t0 = Instant::now();
            let (raw, shown, cited, passages, error, made_up, lost) = match request(c, &note) {
                Err(e) => (String::new(), String::new(), 0, 0, Some(e), 0, 0),
                Ok(Asked { messages, cap, temperature, passages, longer }) => {
                    let opts = mep.options(cap, temperature);
                    match crate::ai::llm::chat(ep.base_url, ep.key, ep.model, ep.provider, messages, &opts).await {
                        Err(e) => (String::new(), String::new(), 0, passages.len(), Some(record::redact(&e)), 0, 0),
                        Ok(raw) => match longer {
                            Some(l) => {
                                let mut shown = chips::clean_longer(&raw).unwrap_or_default();
                                let lost = task_words(&c.summary).iter().filter(|t| !task_words(&shown).contains(t)).count();
                                if chips::is_action_plan(&c.summary) {
                                    shown = summary::postcheck_action_plan(&shown, &crate::speakers::known_people(&note), &l.source, &l.tasks);
                                }
                                (raw, shown, 0, 0, None, 0, lost)
                            }
                            None => {
                                let allowed: Vec<usize> = passages.iter().map(|p| p.n).collect();
                                let made_up = crate::search::ask::cited_numbers(&raw).iter().filter(|n| !allowed.contains(n)).count();
                                let (shown, sources) = chips::checked(&raw, &passages, true);
                                (raw, shown, sources.len(), passages.len(), None, made_up, 0)
                            }
                        },
                    }
                }
            };
            let ms = t0.elapsed().as_millis() as u64;
            let stats = record::end_turn();
            tokens += stats.total_tokens;
            let mut why = judge(c, &raw, &shown, cited, passages);
            if let Some(e) = &error {
                why.insert(0, format!("no answer: {e}"));
            }
            invented += made_up;
            dropped_tasks += lost;
            let pass = why.is_empty();
            passed += usize::from(pass);
            let k = by_kind.entry(c.kind.clone()).or_default();
            k.1 += 1;
            k.0 += usize::from(pass);
            if error.is_none() {
                ms_all.push(ms);
            }
            println!(
                "  {:<4} {:<34} {:.1} s{}{}",
                if pass { "PASS" } else { "FAIL" },
                name,
                ms as f64 / 1000.0,
                if made_up + lost > 0 { format!(" ({made_up} made-up citations, {lost} tasks dropped: fixed by Yap)") } else { String::new() },
                why.first().map(|w| format!(" | {w}")).unwrap_or_default()
            );
            rows.push(json!({
                "case": name,
                "kind": c.kind,
                "skill": c.skill,
                "status": if pass { "pass" } else { "fail" },
                "reasons": why,
                "ms": ms,
                "prompt_tokens": stats.prompt_tokens,
                "completion_tokens": stats.completion_tokens,
                "cited": cited,
                "passages": passages,
                "made_up_citations": made_up,
                "tasks_dropped": lost,
                "shown": record::redact(&shown),
                "raw": record::redact(&raw),
            }));
        }
    }
    ms_all.sort_unstable();
    json!({
        "cases": rows.len(),
        "passed": passed,
        "made_up_citations": invented,
        "tasks_dropped": dropped_tasks,
        "tokens": tokens,
        "median_ms": ms_all.get(ms_all.len() / 2).copied().unwrap_or(0),
        "max_ms": ms_all.last().copied().unwrap_or(0),
        "by_kind": by_kind.iter().map(|(k, (p, n))| (k.clone(), json!(format!("{p}/{n}")))).collect::<serde_json::Map<_, _>>(),
        "rows": rows,
    })
}

/// chips.md: a table per model, then each case's answer.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from("# Meeting chips and Skills\n\n| Model | Passed | Per kind | Made-up citations | Tasks dropped | Median / slowest | Tokens |\n|---|---|---|---|---|---|---|\n");
    for (label, r) in results {
        let per: Vec<String> = r["by_kind"].as_object().into_iter().flatten().map(|(k, v)| format!("{k} {}", v.as_str().unwrap_or(""))).collect();
        out.push_str(&format!(
            "| {label} | {}/{} | {} | {} | {} | {:.1} / {:.1} s | {} |\n",
            r["passed"],
            r["cases"],
            per.join(", "),
            r["made_up_citations"],
            r["tasks_dropped"],
            r["median_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["max_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["tokens"],
        ));
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}{}) — {}\n",
                row["case"].as_str().unwrap_or(""),
                row["kind"].as_str().unwrap_or(""),
                row["skill"].as_str().filter(|s| !s.is_empty()).map(|s| format!(" /{s}")).unwrap_or_default(),
                row["status"].as_str().unwrap_or("").to_uppercase()
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!("\nShown:\n\n```\n{}\n```\n", row["shown"].as_str().unwrap_or("")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_read_and_each_builds_the_request_the_app_sends() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/chips/cases.json");
        let cases = load(&path, &[]).unwrap();
        for kind in ["smart", "skill", "longer"] {
            assert!(cases.iter().filter(|c| c.kind == kind).count() >= 2, "{kind}");
        }
        for c in &cases {
            let note = note_of(c);
            let messages = request(c, &note).unwrap_or_else(|e| panic!("{}: {e}", c.name)).messages;
            let all: String = messages.as_array().unwrap().iter().map(|m| m["content"].as_str().unwrap_or("").to_string()).collect();
            // Every request fits an 8k model with its reply.
            assert!(summary::estimate_tokens(&all) < 5_500, "{}: {}", c.name, summary::estimate_tokens(&all));
        }
    }

    #[test]
    fn an_answer_is_judged_on_what_it_says_cites_and_keeps() {
        let c = Case {
            name: "t".into(),
            kind: "smart".into(),
            skill: String::new(),
            words: String::new(),
            title: String::new(),
            attendees: vec![],
            typed: String::new(),
            summary: String::new(),
            lines: vec![],
            expect: Expect { contains_any: vec!["budget".into()], not_contains: vec!["million".into()], cites: true, max_bullets: Some(2) },
        };
        assert!(judge(&c, "", "- Ask about the budget [2].", 1, 3).is_empty());
        assert_eq!(judge(&c, "", "- One million [2].\n- b\n- c", 1, 3).len(), 3);
        assert!(!judge(&c, "", "- The budget.", 0, 3).is_empty());
        let longer = Case { kind: "longer".into(), summary: "## Action plan\n- [ ] Book the venue".into(), expect: Expect::default(), ..c };
        assert!(judge(&longer, "", "## Action plan\n- [ ] Book the venue\n  - With the deposit paid in March, as Alice said.", 0, 0).is_empty());
        assert!(!judge(&longer, "", "## Action plan\n- [ ] Order the badges, with the logo the team chose last week.", 0, 0).is_empty());
    }
}
