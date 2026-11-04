//! `--summaries`: **a meeting's action plan** (the summary "End meeting &
//! summarise" writes) on each `--model`, through the app's own call, over
//! `evals/summaries/cases.json` — fictional meetings in English, French and
//! Spanish, each with the tasks, owners and decisions (and their reasons)
//! its notes must have.
//!
//! Each case is written twice:
//!
//! - **before**: the call every meeting made until 25 Oct 2026 — the
//!   earlier default layout (`llm::ACTION_PLAN_EARLIER_FRAGMENTS[0]`: a
//!   summary sentence, no recap, bare decisions) and nothing about the
//!   language;
//! - **after**: today's — `ACTION_PLAN_DEFAULT_FRAGMENT` (the "## Recap",
//!   a reason per decision) and the meeting's language line after the input
//!   (`meeting_summary::final_request`, `language.rs`).
//!
//! Both under `ACTION_PLAN_BASE_PROMPT`, then `postcheck_action_plan` as the
//! app runs it. Judged on the notes as shown: **language** (the prose's
//! words, `language::detect`), **tasks** (each expected task under its
//! owner, `notes/tasks.rs` reading it as the Tasks view does), **headings**
//! (the reply's own "## " headings, before the checks put them right: the
//! layout's, in English), **never-say** words (an invented owner), and —
//! after only, as before never asked for them — the **recap** and the
//! **reasons**. `summaries.md` in the run folder; the numbers in
//! `summary.json` (`models[].summaries`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::meetings::meeting_summary::{self as summary, language};

/// The phases.
pub const PHASES: &[&str] = &["before", "after"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    /// The language its notes must be in.
    pub language: String,
    /// My languages (the person's order).
    #[serde(default)]
    pub mine: Vec<String>,
    #[serde(default)]
    pub attendees: Vec<String>,
    /// `[side, words]`: "you" (the person recording) or "them".
    pub lines: Vec<(String, String)>,
    /// Notes typed beside it.
    #[serde(default)]
    pub typed: String,
    pub expect: Expect,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Tasks its notes must list under their owner.
    #[serde(default)]
    pub tasks: Vec<ExpectTask>,
    /// Decisions, with the words their reason must have (after).
    #[serde(default)]
    pub decisions: Vec<ExpectDecision>,
    /// Words that must not appear (an invented owner, a made-up date).
    #[serde(default)]
    pub not_contains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectTask {
    /// "You", "Unassigned", or an attendee as listed.
    pub owner: String,
    /// One of these words in the task (any case, accents as written).
    pub any: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectDecision {
    /// One of these in the decision's line…
    pub any: Vec<String>,
    /// …and one of these (its reason).
    #[serde(default)]
    pub reason: Vec<String>,
}

pub fn load(path: &Path, only: &[String]) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut cases: Vec<Case> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    cases.retain(|c| only.is_empty() || only.iter().any(|o| c.name.to_lowercase().contains(o) || c.language == *o));
    if cases.is_empty() {
        return Err(format!("no summary cases in {}", path.display()));
    }
    Ok(cases)
}

/// The case as a meeting note (in memory: never stored), its lines 15 s
/// apart.
pub fn note_of(c: &Case) -> crate::notes::Note {
    let mut note: crate::notes::Note = serde_json::from_value(json!({
        "id": 1, "title": "", "content": c.typed, "createdTs": 1, "updatedTs": 1, "noteType": "meeting",
        "participants": c.attendees,
    }))
    .expect("a note");
    note.transcript = c
        .lines
        .iter()
        .enumerate()
        .map(|(i, (side, text))| crate::notes::TranscriptSegment {
            source: side.clone(),
            text: text.clone(),
            ts: 1_700_000_000 + i as u64 * 15,
            ..Default::default()
        })
        .collect();
    note
}

/// What the checks read a reply against: the meeting's input (without the
/// language line) and the digests' tasks.
pub struct Parts {
    pub source: String,
    pub digest_actions: Vec<crate::notes::DigestAction>,
}

/// What a phase asks: the layout and the input, and the input alone (the
/// checks' source).
pub fn request(c: &Case, phase: &str) -> (&'static str, String, Parts) {
    let note = note_of(c);
    let budget = summary::FinalBudget::scaled(1);
    if phase == "before" {
        let input = summary::compose_meeting_input_within(&note, budget);
        let parts = Parts { source: input.text.clone(), digest_actions: input.digest_actions };
        return (crate::ai::llm::ACTION_PLAN_EARLIER_FRAGMENTS[0], input.text, parts);
    }
    let lang = language::for_note(&note, &c.mine);
    let (asked, input) = summary::final_request(&note, budget, &lang, true);
    let parts = Parts { source: input.text, digest_actions: input.digest_actions };
    (crate::ai::llm::ACTION_PLAN_DEFAULT_FRAGMENT, asked, parts)
}

/// One case in one phase: the notes as shown, the reply as written, an error.
async fn ask(c: &Case, phase: &str, ep: &Endpoint<'_>) -> (String, String, Option<String>) {
    let (fragment, asked, parts) = request(c, phase);
    let mep = summary::Endpoint {
        base_url: ep.base_url.to_string(),
        api_key: ep.key.to_string(),
        model: ep.model.to_string(),
        provider: ep.provider.to_string(),
        fragment: fragment.to_string(),
        disable_thinking: false,
    };
    let reply = crate::ai::llm::enhance_note(
        &asked,
        crate::ai::llm::ACTION_PLAN_BASE_PROMPT,
        fragment,
        &mep.base_url,
        &mep.api_key,
        &mep.model,
        &mep.provider,
        &mep.final_options(),
    )
    .await;
    match reply {
        Ok(raw) => {
            let note = note_of(c);
            let shown = summary::postcheck_action_plan(&raw, &crate::speakers::known_people(&note), &parts.source, &parts.digest_actions);
            (shown, raw, None)
        }
        Err(e) => (String::new(), String::new(), Some(record::redact(&e))),
    }
}

/// The layout's headings, in English.
const HEADINGS: &[&str] = &["recap", "action plan", "decisions", "open questions", "unassigned"];

/// The notes' own words: no headings, owners, checkboxes or deadlines.
fn prose(md: &str) -> String {
    md.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let l = l.trim_start_matches(['-', '*', '•']).trim_start();
            let l = l.strip_prefix("[ ]").or_else(|| l.strip_prefix("[x]")).unwrap_or(l);
            match l.find("(due:") {
                Some(at) => l[..at].to_string(),
                None => l.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_any(text: &str, words: &[String]) -> bool {
    let t = text.to_lowercase();
    words.iter().any(|w| t.contains(&w.to_lowercase()))
}

/// The lines under "## <title>" (to the next "## ").
fn section<'a>(md: &'a str, title: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut on = false;
    for l in md.lines() {
        if let Some(h) = l.trim().strip_prefix("## ") {
            on = h.trim().eq_ignore_ascii_case(title);
            continue;
        }
        if on && !l.trim().is_empty() {
            out.push(l);
        }
    }
    out
}

/// Each check on `shown` (the notes after Yap's checks) and `raw` (the
/// reply as written): `(name, passed, why)`. Recap and reasons only
/// after (before never asked for them). Pure.
pub fn judge(c: &Case, phase: &str, raw: &str, shown: &str) -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();
    // Sure, or — notes too terse to be sure of ("Tell support team about
    // new release date") — most of their little words.
    let text = prose(shown);
    let heard = language::detect(&text).or_else(|| language::leading(&text));
    out.push((
        "language",
        heard == Some(c.language.as_str()),
        format!("the notes read as {} (want {})", heard.unwrap_or("unclear"), c.language),
    ));
    let tasks = crate::notes::tasks::parse(shown);
    let missing: Vec<String> = c
        .expect
        .tasks
        .iter()
        .filter(|t| !tasks.iter().any(|x| x.owner.eq_ignore_ascii_case(&t.owner) && has_any(&x.task, &t.any)))
        .map(|t| format!("{}: {}", t.owner, t.any.join("/")))
        .collect();
    out.push(("tasks", missing.is_empty(), format!("missing {}", missing.join("; "))));
    let foreign: Vec<String> = raw
        .lines()
        .filter_map(|l| l.trim().strip_prefix("## "))
        .map(|h| h.trim().trim_end_matches(':').to_string())
        .filter(|h| !HEADINGS.contains(&h.to_lowercase().as_str()))
        .collect();
    out.push(("headings", foreign.is_empty(), format!("headings not the layout's: {}", foreign.join(", "))));
    let said: Vec<&String> = c.expect.not_contains.iter().filter(|w| shown.to_lowercase().contains(&w.to_lowercase())).collect();
    out.push(("never_say", said.is_empty(), format!("says {said:?}")));
    if phase == "after" {
        let recap = section(shown, "Recap");
        let ok = !recap.is_empty() && recap.iter().filter(|l| l.trim_start().starts_with("- ")).count() <= 4;
        out.push(("recap", ok, "no \"## Recap\" with a sentence and up to four topics".to_string()));
        let decisions = section(shown, "Decisions");
        let unreasoned: Vec<String> = c
            .expect
            .decisions
            .iter()
            .filter(|d| !decisions.iter().any(|l| has_any(l, &d.any) && (d.reason.is_empty() || has_any(l, &d.reason))))
            .map(|d| d.any.join("/"))
            .collect();
        out.push(("reasons", unreasoned.is_empty(), format!("no decision with its reason: {}", unreasoned.join("; "))));
    }
    out
}

/// Every case `repeat` times in each of `phases` on one model; its results
/// for summary.json and summaries.md.
pub async fn run(cases: &[Case], ep: &Endpoint<'_>, repeat: u32, phases: &[&'static str]) -> Value {
    let mut rows = Vec::new();
    // phase → check → (passed, of)
    let mut tally: std::collections::BTreeMap<String, std::collections::BTreeMap<String, (usize, usize)>> = Default::default();
    let mut ms_all: Vec<u64> = Vec::new();
    let mut tokens = 0u64;
    for c in cases {
        for phase in phases {
            for rep in 0..repeat.max(1) {
                let name = if repeat > 1 { format!("{}#{}", c.name, rep + 1) } else { c.name.clone() };
                record::set_context(json!({ "model": ep.model, "scenario": format!("summaries:{name}:{phase}") }));
                record::begin_turn();
                let paced0 = record::paced_ms();
                let t0 = Instant::now();
                let (shown, raw, error) = ask(c, phase, ep).await;
                let ms = (t0.elapsed().as_millis() as u64).saturating_sub(record::paced_ms().saturating_sub(paced0));
                let stats = record::end_turn();
                tokens += stats.total_tokens;
                let checks = if error.is_some() { Vec::new() } else { judge(c, phase, &raw, &shown) };
                let mut why: Vec<String> = checks.iter().filter(|(_, ok, _)| !ok).map(|(n, _, w)| format!("{n}: {w}")).collect();
                if let Some(e) = &error {
                    why.insert(0, format!("the call failed: {e}"));
                }
                let pass = why.is_empty();
                let t = tally.entry(phase.to_string()).or_default();
                let all = t.entry("all".to_string()).or_default();
                all.1 += 1;
                all.0 += usize::from(pass);
                for (n, ok, _) in &checks {
                    let e = t.entry(n.to_string()).or_default();
                    e.1 += 1;
                    e.0 += usize::from(*ok);
                }
                if error.is_none() {
                    ms_all.push(ms);
                }
                println!(
                    "  {:<4} {:<7} {:<26} {:.1} s{}",
                    if pass { "PASS" } else { "FAIL" },
                    phase,
                    name,
                    ms as f64 / 1000.0,
                    why.first().map(|w| format!(" | {w}")).unwrap_or_default()
                );
                rows.push(json!({
                    "case": name,
                    "language": c.language,
                    "phase": phase,
                    "status": if pass { "pass" } else { "fail" },
                    "reasons": why,
                    "checks": checks.iter().map(|(n, ok, _)| json!({ "check": n, "ok": ok })).collect::<Vec<_>>(),
                    "ms": ms,
                    "prompt_tokens": stats.prompt_tokens,
                    "completion_tokens": stats.completion_tokens,
                    "output": record::redact(&shown),
                    "raw": record::redact(&raw),
                }));
            }
        }
    }
    ms_all.sort_unstable();
    json!({
        "cases": rows.len(),
        "passed": rows.iter().filter(|r| r["status"] == "pass").count(),
        "tokens": tokens,
        "median_ms": ms_all.get(ms_all.len() / 2).copied().unwrap_or(0),
        "by_phase": tally
            .iter()
            .map(|(p, checks)| (p.clone(), json!(checks.iter().map(|(k, (a, n))| (k.clone(), json!(format!("{a}/{n}")))).collect::<serde_json::Map<_, _>>())))
            .collect::<serde_json::Map<_, _>>(),
        "rows": rows,
    })
}

/// summaries.md: a table per model and phase, then each case's notes.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from(
        "# Meeting summaries\n\n| Model | Phase | Passed | Language | Tasks | Headings | Never-say | Recap | Reasons |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    for (label, r) in results {
        for (phase, checks) in r["by_phase"].as_object().into_iter().flatten() {
            let get = |k: &str| checks[k].as_str().unwrap_or("–").to_string();
            out.push_str(&format!(
                "| {label} | {phase} | {} | {} | {} | {} | {} | {} | {} |\n",
                get("all"),
                get("language"),
                get("tasks"),
                get("headings"),
                get("never_say"),
                get("recap"),
                get("reasons")
            ));
        }
    }
    for (label, r) in results {
        out.push_str(&format!(
            "\n## {label}\n\n{} tokens; median {:.1} s a summary.\n",
            r["tokens"],
            r["median_ms"].as_f64().unwrap_or(0.0) / 1000.0
        ));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} ({}, {}) — {}\n",
                row["case"].as_str().unwrap_or(""),
                row["language"].as_str().unwrap_or(""),
                row["phase"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("").to_uppercase()
            ));
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!("\n```\n{}\n```\n", row["output"].as_str().unwrap_or("")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<Case> {
        load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/summaries/cases.json"), &[]).unwrap()
    }

    #[test]
    fn the_cases_read_and_each_meeting_is_in_its_language() {
        let all = cases();
        for lang in ["en", "fr", "es"] {
            assert!(all.iter().any(|c| c.language == lang), "{lang}");
        }
        for c in &all {
            let note = note_of(c);
            assert_eq!(language::for_note(&note, &c.mine).code, c.language, "{}", c.name);
            assert!(!c.expect.tasks.is_empty(), "{}: checks no task", c.name);
            // Before: the earlier layout, no line. After: today's, and the
            // line for a meeting in another language.
            let (before, asked_before, _) = request(c, "before");
            let (after, asked_after, parts) = request(c, "after");
            assert!(!before.contains("## Recap") && after.contains("## Recap"));
            assert!(!asked_before.contains("Write the notes in"));
            assert_eq!(asked_after.contains("Write the notes in"), c.language != "en", "{}", c.name);
            assert!(!parts.source.contains("Write the notes in"));
        }
    }

    #[test]
    fn the_judge_reads_the_notes_as_shown() {
        let c = cases().into_iter().find(|c| c.language == "fr").unwrap();
        // Notes written as asked pass every check.
        let good = "## Recap\nLe salon de Lyon : le budget et le traiteur.\n- Le traiteur\n- Les chambres\n\n## Action plan\n\n### Alice Martin\n- [ ] Demander deux autres devis au traiteur (due: vendredi)\n\n### You\n- [ ] Envoyer le plan de salle à l'équipe\n\n## Decisions\n- On garde le lieu mais on change de traiteur — le devis est trop cher\n- Pas de catalogue cette année — tout est en ligne\n\n## Unassigned\n- [ ] Réserver les chambres pour les exposants";
        let checks = judge(&c, "after", good, good);
        assert!(checks.iter().all(|(_, ok, _)| *ok), "{checks:?}");
        // The same notes in English: the language check fails.
        let english = "## Recap\nThe Lyon fair: the budget and the caterer, and we will check the rooms for the team.\n\n## Action plan\n\n### Alice Martin\n- [ ] Ask the caterer for two more quotes and send them to the team (due: vendredi)\n\n### You\n- [ ] Send the floor plan to the team, and it should be done this week\n\n## Decisions\n- Keep the venue but change the caterer — the quote is too expensive\n\n## Unassigned\n- [ ] Book the rooms for the exhibitors";
        let checks = judge(&c, "after", english, english);
        assert!(!checks.iter().find(|(n, _, _)| *n == "language").unwrap().1, "{checks:?}");
        // A heading translated: the headings check fails (as written).
        let translated = good.replace("## Decisions", "## Décisions");
        assert!(!judge(&c, "before", &translated, good).iter().find(|(n, _, _)| *n == "headings").unwrap().1);
        // Before asks no recap or reasons.
        assert!(judge(&c, "before", good, good).iter().all(|(n, _, _)| *n != "recap" && *n != "reasons"));
        // Terse English notes (Groq's, 25 Oct) are English, if not "surely".
        let en = cases().into_iter().find(|c| c.name == "en-release-first-language-fr").unwrap();
        let terse = "## Recap\nQuick sync on the website release, testing status, release date, and tasks.\n\n- Testing status and mobile payment bug\n\n## Action plan\n\n### You\n- [ ] Tell support team about new release date\n\n### Tom\n- [ ] Fix mobile payment bug (due: Wednesday)\n\n## Decisions\n- Release held until Thursday — can't ship broken checkout.\n\n## Open questions\n- Who updates the release notes?\n\n## Unassigned\n- [ ] Update release notes this week";
        assert_eq!(language::detect(&prose(terse)), None);
        assert!(judge(&en, "after", terse, terse).iter().all(|(_, ok, _)| *ok), "{:?}", judge(&en, "after", terse, terse));
    }
}
