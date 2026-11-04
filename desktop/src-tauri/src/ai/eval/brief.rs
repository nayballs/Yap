//! `--brief`: **pre-meeting briefs** on each `--model`, through the brief's
//! own code (`calendar/brief.rs`: gathering, the prompt, the checks, the
//! 150-word fit) over the scenarios in `evals/briefs/*.json` — calendars and
//! past notes: recurring 1:1s, a new attendee, a project meeting, a meeting
//! with no history, two people with the same first name…
//!
//! Each scenario is scored twice:
//!
//! - **the model's reply as written** (before Yap's checks): how many of its
//!   lines claim something, and how many of those cite no source, say a
//!   name, date or number their sources don't have, or give a task to the
//!   wrong person — and whether it said any of the scenario's planted
//!   never-say words;
//! - **the brief as shown**: every line cited, no invented owner, date or
//!   item (the scenario's `not_contains`, open items only from the plans and
//!   only You's or the attendees'), the right "first meeting", the notes it
//!   must and mustn't cite, and under 150 words. A failure here is a brief a
//!   person would have seen.
//!
//! `brief.md` in the run folder; the numbers in `summary.json`
//! (`models[].brief`).

use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::record;
use crate::calendar::brief::{self, Brief};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub meeting: MeetingSpec,
    #[serde(default)]
    pub notes: Vec<NoteSpec>,
    #[serde(default)]
    pub memory: Vec<Value>,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeetingSpec {
    pub title: String,
    /// A recurring meeting's series (its UID); none: a one-off.
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default = "ten")]
    pub in_minutes: i64,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub description: String,
}

fn ten() -> i64 {
    10
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteSpec {
    pub title: String,
    pub days_ago: i64,
    #[serde(default)]
    pub participants: Vec<String>,
    /// Another occurrence of this series.
    #[serde(default)]
    pub series: Option<String>,
    /// The action plan (the note's summary).
    #[serde(default)]
    pub plan: String,
    /// Notes the person typed.
    #[serde(default)]
    pub content: String,
    /// Rolling digests (`MeetingDigest` JSON), for a meeting with no plan.
    #[serde(default)]
    pub digests: Vec<Value>,
    /// "you: …" / "them: …" lines.
    #[serde(default)]
    pub transcript: Vec<String>,
}

/// What the brief as shown must be. Text matching is case-insensitive.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub first: Option<bool>,
    /// Earlier notes it must cite, by title…
    #[serde(default)]
    pub cites: Vec<String>,
    /// …and ones it mustn't.
    #[serde(default)]
    pub not_cited: Vec<String>,
    /// Open items it must list (words of each)…
    #[serde(default)]
    pub open_items: Vec<String>,
    /// …and mustn't.
    #[serde(default)]
    pub no_open_items: Vec<String>,
    /// Who an open item may belong to.
    #[serde(default)]
    pub owners: Vec<String>,
    /// Words the brief must have, every one…
    #[serde(default)]
    pub contains: Vec<String>,
    /// …and never (invented names, dates, items; another person's notes).
    #[serde(default)]
    pub not_contains: Vec<String>,
    #[serde(default = "max_words")]
    pub max_words: usize,
}

fn max_words() -> usize {
    brief::MAX_WORDS
}

pub fn load(dir: &Path, only: &[String]) -> Result<Vec<Scenario>, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("can't read briefs in {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let s: Scenario = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        out.push(s);
    }
    let only: Vec<String> = only.iter().map(|s| s.to_lowercase()).collect();
    out.retain(|s| only.is_empty() || only.iter().any(|o| s.name.to_lowercase().contains(o)));
    if out.is_empty() {
        return Err(format!("no briefs in {}", dir.display()));
    }
    Ok(out)
}

const DAY: i64 = 86_400;

/// The scenario's meeting, notes and memory, as of `now`.
fn build(s: &Scenario, now: i64) -> Result<(brief::Meeting, Vec<crate::notes::Note>, Vec<crate::memory::ops::Item>), String> {
    let m = &s.meeting;
    let start = now + m.in_minutes * 60;
    let key = match &m.series {
        Some(series) => format!("1:{series}:{start}"),
        None => format!("1:{}-next@example.com", s.name),
    };
    let meeting = brief::Meeting {
        key,
        title: m.title.clone(),
        start,
        attendees: m.attendees.clone(),
        description: m.description.clone(),
    };
    let mut notes = Vec::new();
    for (i, n) in s.notes.iter().enumerate() {
        let when = now - n.days_ago * DAY;
        let transcript: Vec<Value> = n
            .transcript
            .iter()
            .enumerate()
            .map(|(j, l)| {
                let (source, text) = l.split_once(": ").unwrap_or(("them", l));
                json!({ "source": source.to_lowercase(), "text": text, "ts": when + j as i64 * 15 })
            })
            .collect();
        let event = n.series.as_ref().map(|series| {
            json!({ "key": format!("1:{series}:{when}"), "title": n.title, "start": when, "end": when + 1800 })
        });
        let note: crate::notes::Note = serde_json::from_value(json!({
            "id": i + 1,
            "title": n.title,
            "createdTs": when,
            "updatedTs": when + 3600,
            "noteType": "meeting",
            "folder": "Meetings",
            "participants": n.participants,
            "enhancedContent": n.plan,
            "content": n.content,
            "digests": n.digests,
            "transcript": transcript,
            "event": event,
        }))
        .map_err(|e| format!("{}: note \"{}\": {e}", s.name, n.title))?;
        notes.push(note);
    }
    let mut memory = Vec::new();
    for (i, item) in s.memory.iter().enumerate() {
        let mut v = item.clone();
        v["id"] = json!(i + 1);
        v["updatedTs"] = json!(1);
        memory.push(serde_json::from_value(v).map_err(|e| format!("{}: memory: {e}", s.name))?);
    }
    Ok((meeting, notes, memory))
}

fn has(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

/// What's wrong with the brief as shown (empty: nothing).
fn judge(b: &Brief, e: &Expect, facts: &brief::Facts) -> Vec<String> {
    let attendees = &facts.meeting.attendees;
    let mut why = Vec::new();
    let text = brief::plain_text(b);
    let lines: Vec<(&str, &brief::Line)> = b.parts.iter().flat_map(|p| p.lines.iter().map(move |l| (p.kind.as_str(), l))).collect();
    for (kind, l) in &lines {
        let absence = l.text.starts_with("First meeting") || l.text.starts_with("No earlier notes");
        if l.cites.is_empty() && !absence {
            why.push(format!("uncited {kind} line: {}", l.text));
        }
        if l.cites.iter().any(|n| !b.sources.iter().any(|s| s.n == *n)) {
            why.push(format!("a citation with no source: {}", l.text));
        }
        if *kind == "summary" {
            if let Some(reason) = brief::ungrounded(&l.text, &l.cites, facts) {
                why.push(format!("a summary line strays from its sources ({reason}): {}", l.text));
            }
            if brief::claims_done(&l.text, &l.cites, facts) {
                why.push(format!("a summary line says an open task was done: {}", l.text));
            }
            if brief::settles_question(&l.text, &l.cites, facts) {
                why.push(format!("a summary line settles an open question: {}", l.text));
            }
            if brief::swaps_owner(&l.text, &l.cites, facts) {
                why.push(format!("a summary line moves a task to someone else: {}", l.text));
            }
        }
    }
    if b.words > e.max_words {
        why.push(format!("{} words (≤ {})", b.words, e.max_words));
    }
    if let Some(first) = e.first {
        if b.first != first {
            why.push(format!("first meeting: {} (expected {first})", b.first));
        }
    }
    let cited: Vec<&str> = b.sources.iter().filter(|s| s.kind == "note").map(|s| s.title.as_str()).collect();
    for t in &e.cites {
        if !cited.iter().any(|c| c.eq_ignore_ascii_case(t)) {
            why.push(format!("doesn't cite \"{t}\""));
        }
    }
    for t in &e.not_cited {
        if cited.iter().any(|c| c.eq_ignore_ascii_case(t)) {
            why.push(format!("cites \"{t}\""));
        }
    }
    let open: Vec<&brief::Line> = lines.iter().filter(|(k, _)| *k == "open").map(|(_, l)| *l).collect();
    for w in &e.open_items {
        if !open.iter().any(|l| has(&l.text, w)) {
            why.push(format!("open items miss \"{w}\""));
        }
    }
    for w in &e.no_open_items {
        if open.iter().any(|l| has(&l.text, w)) {
            why.push(format!("open items have \"{w}\""));
        }
    }
    if !e.owners.is_empty() {
        for l in &open {
            let who = l.who.as_deref().unwrap_or("");
            let ok = e.owners.iter().any(|o| brief::same_person(who, o, attendees.as_slice()) || who.eq_ignore_ascii_case(o));
            if !ok {
                why.push(format!("open item owned by \"{who}\": {}", l.text));
            }
        }
    }
    for w in &e.contains {
        if !has(&text, w) {
            why.push(format!("missing \"{w}\""));
        }
    }
    for w in &e.not_contains {
        if has(&text, w) {
            why.push(format!("says \"{w}\""));
        }
    }
    why
}

pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

/// Run every scenario `repeat` times on one model; its results for
/// summary.json and brief.md.
pub async fn run(scenarios: &[Scenario], ep: &Endpoint<'_>, repeat: u32) -> Value {
    let endpoint = crate::meetings::meeting_summary::Endpoint {
        base_url: ep.base_url.to_string(),
        api_key: ep.key.to_string(),
        model: ep.model.to_string(),
        provider: ep.provider.to_string(),
        fragment: String::new(),
        disable_thinking: false,
    };
    let mut rows = Vec::new();
    let (mut claims, mut uncited, mut ungrounded, mut swapped, mut done) = (0, 0, 0, 0, 0);
    let (mut planted, mut kept, mut passed, mut calls) = (0, 0, 0, 0);
    let mut ms_all: Vec<u64> = Vec::new();
    for s in scenarios {
        for rep in 0..repeat.max(1) {
            let name = if repeat > 1 { format!("{}#{}", s.name, rep + 1) } else { s.name.clone() };
            record::set_context(json!({ "model": ep.model, "scenario": name }));
            let now = chrono::Utc::now().timestamp();
            let (meeting, notes, memory) = match build(s, now) {
                Ok(x) => x,
                Err(e) => {
                    println!("  ERR  {name}: {e}");
                    rows.push(json!({ "scenario": name, "status": "error", "error": e }));
                    continue;
                }
            };
            let facts = brief::gather(&meeting, &notes, &memory, now);
            record::begin_turn();
            let t0 = Instant::now();
            let called = brief::messages(&facts).is_some();
            let result = brief::write(None, &facts, &endpoint).await;
            let ms = t0.elapsed().as_millis() as u64;
            let stats = record::end_turn();
            let (checked, reply, error) = match result {
                Ok((c, r)) => (c, r, None),
                Err(e) => (brief::Checked::default(), String::new(), Some(record::redact(&e))),
            };
            let b = brief::assemble(&facts, checked.lines.clone(), true, "", now);
            let mut why = judge(&b, &s.expect, &facts);
            if let Some(e) = &error {
                why.push(format!("the call failed: {e}"));
            }
            let raw_planted: Vec<&String> = s.expect.not_contains.iter().filter(|w| has(&reply, w)).collect();
            let summary_kept = b.parts.iter().filter(|p| p.kind == "summary").map(|p| p.lines.len()).sum::<usize>();
            claims += checked.claims;
            uncited += checked.uncited;
            ungrounded += checked.ungrounded;
            swapped += checked.swapped;
            done += checked.done + checked.settled;
            planted += raw_planted.len();
            kept += summary_kept;
            if called {
                calls += 1;
                ms_all.push(ms);
            }
            let pass = why.is_empty();
            if pass {
                passed += 1;
            }
            println!(
                "  {:<4} {:<34} {} words, {} kept of {} lines{}{}",
                if pass { "PASS" } else { "FAIL" },
                name,
                b.words,
                summary_kept,
                checked.claims,
                if called { format!(", {:.1} s", ms as f64 / 1000.0) } else { ", no call".to_string() },
                why.first().map(|w| format!(" | {w}")).unwrap_or_default()
            );
            rows.push(json!({
                "scenario": name,
                "description": s.description,
                "status": if pass { "pass" } else { "fail" },
                "reasons": why,
                "called": called,
                "ms": ms,
                "prompt_tokens": stats.prompt_tokens,
                "completion_tokens": stats.completion_tokens,
                "raw": { "claims": checked.claims, "uncited": checked.uncited, "ungrounded": checked.ungrounded,
                         "swapped": checked.swapped, "done": checked.done, "settled": checked.settled, "too_long": checked.too_long,
                         "nothing": checked.nothing, "planted": raw_planted },
                "kept": summary_kept,
                "words": b.words,
                "first": b.first,
                "reply": record::redact(&reply),
                "brief": record::redact(&brief::plain_text(&b)),
            }));
        }
    }
    ms_all.sort_unstable();
    let mean = if ms_all.is_empty() { 0 } else { ms_all.iter().sum::<u64>() / ms_all.len() as u64 };
    json!({
        "scenarios": rows.len(),
        "passed": passed,
        "calls": calls,
        "claims": claims,
        "uncited": uncited,
        "ungrounded": ungrounded,
        "swapped": swapped,
        "done": done,
        "planted": planted,
        "kept": kept,
        "mean_ms": mean,
        "max_ms": ms_all.last().copied().unwrap_or(0),
        "rows": rows,
    })
}

/// brief.md: a table per model, then each brief as shown and the reply as written.
pub fn markdown(results: &[(String, Value)]) -> String {
    let mut out = String::from("# Pre-meeting briefs\n\n");
    out.push_str("| Model | Passed | Model lines | Uncited | Not in sources | Wrong owner | Says done / settled | Never-say words | Kept | Mean / slowest |\n|---|---|---|---|---|---|---|---|---|---|\n");
    for (label, r) in results {
        out.push_str(&format!(
            "| {label} | {}/{} | {} | {} | {} | {} | {} | {} | {} | {:.1} / {:.1} s |\n",
            r["passed"], r["scenarios"], r["claims"], r["uncited"], r["ungrounded"], r["swapped"], r["done"], r["planted"], r["kept"],
            r["mean_ms"].as_f64().unwrap_or(0.0) / 1000.0,
            r["max_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        ));
    }
    for (label, r) in results {
        out.push_str(&format!("\n## {label}\n"));
        for row in r["rows"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "\n### {} — {}\n",
                row["scenario"].as_str().unwrap_or(""),
                row["status"].as_str().unwrap_or("").to_uppercase()
            ));
            if let Some(d) = row["description"].as_str().filter(|d| !d.is_empty()) {
                out.push_str(&format!("\n{d}\n\n"));
            }
            for why in row["reasons"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", why.as_str().unwrap_or("")));
            }
            out.push_str(&format!("\nAs shown ({} words):\n\n```\n{}```\n", row["words"], row["brief"].as_str().unwrap_or("")));
            if let Some(reply) = row["reply"].as_str().filter(|r| !r.is_empty()) {
                out.push_str(&format!("\nThe model wrote:\n\n```\n{reply}\n```\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines the models wrote in the evaluation runs that the first checks
    /// let through, against the scenario they came from.
    #[test]
    fn what_slipped_through_before_is_caught() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("evals").join("briefs");
        let now = chrono::Utc::now().timestamp();
        let facts_of = |name: &str| {
            let s = load(&dir, &[name.to_string()]).unwrap().remove(0);
            let (meeting, notes, memory) = build(&s, now).unwrap();
            brief::gather(&meeting, &notes, &memory, now)
        };
        let f = facts_of("brief-recurring-1to1");
        // The free tier is in the newer note, not the one cited.
        assert!(brief::ungrounded("The free tier is still at three projects, and the pricing FAQ is due for draft.", &[2], &f).is_some());
        assert!(brief::settles_question("The annual plan should get a discount.", &[1, 4], &f));
        assert!(brief::claims_done("You sent Priya the revised pricing table, and she drafted the pricing FAQ.", &[1, 2], &f));
        assert!(brief::ungrounded("You must send the revised pricing table before the next meeting.", &[1], &f).is_some());
        let f = facts_of("brief-long-history");
        assert!(brief::swaps_owner(
            "You need to write the programme's one-page summary for the leadership team and check the translation status for the regional pages.",
            &[1, 2, 3],
            &f
        ));
        assert!(brief::swaps_owner(
            "The onboarding checklist is due from Daniel, and the regional launch timeline is due from you.",
            &[1, 2, 3],
            &f
        ));
        let f = facts_of("brief-transcript-only");
        assert!(brief::claims_done("Rochelle Adams has sent quotes for both the lakeside hotel and the old mill by Thursday.", &[1, 2], &f));
        assert!(brief::swaps_owner("You have been asked to send quotes for the old mill and the lakeside hotel by Thursday.", &[1], &f));
        // The third run (with the held-out scenarios).
        let f = facts_of("brief-long-history");
        assert!(brief::claims_done("Last time, you got the partner marketing budget approved by the leadership team.", &[6], &f));
        assert!(brief::settles_question("The premier tier includes co-branded materials, but this is still open.", &[6], &f));
        let f = facts_of("brief-recurring-1to1");
        assert!(brief::ungrounded("The free tier is still at three projects, and the annual plan is on the agenda.", &[1, 3], &f).is_some());
        let f = facts_of("brief-overlapping-names");
        assert!(brief::ungrounded(
            "You need to update the forecast spreadsheet, and the agenda covers the quarter's budget, travel, and forecast.",
            &[1, 2],
            &f
        )
        .is_some());
        // An agenda claim the invite backs keeps its bullet, the invite cited.
        let f = facts_of("brief-client-check-in");
        let c = brief::check_summary("- Brightwater is moving to annual billing from January, and the Q1 roadmap is on the agenda. [1]", &f);
        assert_eq!(c.lines.len(), 1, "{c:?}");
        let invite = f.sources.iter().find(|s| s.kind == "invite").unwrap().n;
        assert_eq!(c.lines[0].cites, [1, invite]);
        let f = facts_of("brief-transcript-only");
        assert!(brief::ungrounded("The agenda focuses on picking the venue once the budget and dates are aligned.", &[1, 2], &f).is_none());
        // And what they wrote well stays.
        let f = facts_of("brief-project-meeting");
        assert!(brief::ungrounded("The plan is to migrate invoices first, then customer records, with a spring cut-over.", &[1, 3], &f).is_none());
        assert!(!brief::swaps_owner("You need finance sign-off for the cut-over weekend, while Tanay writes the rollback runbook.", &[1], &f));
    }

    /// Every scenario in evals/briefs builds, and its brief without a model
    /// (Yap's own part) already passes what it can.
    #[test]
    fn the_scenarios_build_and_yaps_own_part_passes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("evals").join("briefs");
        let scenarios = load(&dir, &[]).unwrap();
        assert!(scenarios.len() >= 5);
        let now = chrono::Utc::now().timestamp();
        for s in &scenarios {
            let (meeting, notes, memory) = build(s, now).unwrap();
            let facts = brief::gather(&meeting, &notes, &memory, now);
            let b = brief::assemble(&facts, vec![], false, "", now);
            let why = judge(&b, &s.expect, &facts);
            assert!(why.is_empty(), "{}: {why:?}\n{}", s.name, brief::plain_text(&b));
        }
    }
}
