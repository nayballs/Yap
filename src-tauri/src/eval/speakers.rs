//! `yap.exe eval --speakers`: **real names on meeting voices**
//! (`speakers/naming.rs`) scored on the meetings in
//! `evals/speakers/*.json`. Each is a transcript whose call lines are
//! already told apart ("them-1", "them-2"… as the speaker engine labels
//! them; diarization mistakes included on purpose), the invite's
//! attendees, what Memory knows, and the truth: who each voice really is
//! (`null`: nobody Yap could know, or two people the engine mixed up).
//!
//! Every run scores **Rust alone** (no model), then, per `--model`, Rust
//! plus the AI step exactly as the app asks it (`naming::asks`,
//! `ask_messages`, the JSON schema as llama.cpp's grammar for a model on
//! this PC, `check_answer`). Per voice: **correct** (Yap's name is the
//! truth), **wrong** (a name that isn't — worse than "Speaker 2"),
//! **missed** (no name where there was one to find); coverage = correct /
//! voices with a name to find. Separately for the names Yap **puts on**
//! a voice as its guess (what the action plan reads before anyone
//! answers) and every name it **suggests** on a card.
//!
//! Scenario shape:
//! ```jsonc
//! { "name": "handover-standup", "description": "…", "tags": ["handover"],
//!   "me": "Imogen Hart",                       // About you's name
//!   "attendees": ["Priya Shah", "Sam Patel"],  // the invite (the note's participants)
//!   "known": ["Rochelle"],                     // Memory's People
//!   "calendar_guess": "Priya Shah",            // a 1:1 invite's guess on "Them"
//!   "lines": [["them-1", "Priya, can you start?"], ["you", "…"], ["them-2", "Sure…"]],
//!   "truth": { "them-1": "Tom Baker", "them-2": "Priya Shah", "them-3": null, "them-4": "you" } }
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use crate::notes::{Note, TranscriptSegment};
use crate::speakers::naming::{self, Context, ModelVote, Proposal};

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub me: String,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub known: Vec<String>,
    #[serde(default)]
    pub calendar_guess: Option<String>,
    pub lines: Vec<(String, String)>,
    pub truth: BTreeMap<String, Option<String>>,
}

/// Every scenario in `dir` (`*.json`, by name).
pub fn load(dir: &Path) -> Result<Vec<Scenario>, String> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let s: Scenario = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        if s.lines.is_empty() || s.truth.is_empty() {
            return Err(format!("{}: no lines or no truth", p.display()));
        }
        for voice in s.truth.keys() {
            if !s.lines.iter().any(|(v, _)| v == voice) {
                return Err(format!("{}: truth for {voice}, who never speaks", p.display()));
            }
        }
        out.push(s);
    }
    Ok(out)
}

/// The scenario as a meeting note: its lines one after another on the
/// recording's clock (a word every 400 ms, half a second between lines),
/// each with its words' times, as the recorder and the speaker engine leave
/// them.
pub fn note_of(s: &Scenario) -> Note {
    let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1, "noteType": "meeting" })).expect("a note");
    n.participants = s.attendees.clone();
    let mut at = 0u64;
    for (voice, text) in &s.lines {
        let count = text.split_whitespace().count().max(1) as u64;
        let dur = count * 400;
        let words: Vec<[u32; 2]> = (0..count).map(|k| [(k * 400) as u32, (k * 400 + 350) as u32]).collect();
        let source = if voice == "you" { "you" } else { "them" };
        n.transcript.push(TranscriptSegment {
            source: source.into(),
            speaker: if voice == source { String::new() } else { voice.clone() },
            text: text.clone(),
            ts: 1_700_000_000 + at / 1000,
            start_ms: Some(at),
            end_ms: Some(at + dur),
            words,
            ..Default::default()
        });
        at += dur + 500;
    }
    if let Some(name) = &s.calendar_guess {
        // The 1:1 invite's guess, handed to the voice that said the most
        // (`speakers::labels::settle_them`, which runs as lines get voices).
        crate::speakers::calendar_guess(&mut n, &[(name.clone(), String::new())]);
        let mut words: BTreeMap<&str, usize> = BTreeMap::new();
        for (v, t) in &s.lines {
            if v.starts_with("them-") {
                *words.entry(v.as_str()).or_default() += t.split_whitespace().count();
            }
        }
        let found: Vec<(String, usize)> = {
            let mut f: Vec<(String, usize)> = words.into_iter().map(|(v, w)| (v.to_string(), w)).collect();
            f.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
            // As the app: a voice repeating yours never takes the call's name.
            f.retain(|(v, _)| !naming::repeats_you(&n, v));
            f
        };
        crate::speakers::labels::settle_them(&mut n, &found, true);
    }
    n
}

/// Is `name` the person `truth` names (a full name, or its first name)?
fn same_person(name: &str, truth: &str) -> bool {
    let a = name.trim().to_lowercase();
    let b = truth.trim().to_lowercase();
    a == b || b.split_whitespace().next() == Some(a.as_str()) || a.split_whitespace().next() == Some(b.as_str()) && !b.contains(' ')
}

/// One scenario's score at one level ("named" or "suggested").
#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct Score {
    pub correct: usize,
    pub wrong: usize,
    pub missed: usize,
    /// Voices with a name to find (the truth isn't null).
    pub findable: usize,
    /// Voices with nothing to find that got nothing.
    pub held_back: usize,
}

impl Score {
    fn add(&mut self, o: Score) {
        self.correct += o.correct;
        self.wrong += o.wrong;
        self.missed += o.missed;
        self.findable += o.findable;
        self.held_back += o.held_back;
    }

    pub fn coverage(&self) -> f64 {
        if self.findable == 0 {
            0.0
        } else {
            self.correct as f64 / self.findable as f64
        }
    }
}

/// What Yap ends up saying about each voice: the name it put on, and the
/// name on its card.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Outcome {
    /// voice → name put on it ("you" for "It was me" never: that's asked).
    pub named: BTreeMap<String, String>,
    /// voice → the card's name ("you" for "is it you?").
    pub suggested: BTreeMap<String, String>,
}

fn outcome(note: &Note, proposals: &[Proposal]) -> Outcome {
    let mut o = Outcome::default();
    for p in proposals {
        let name = if p.me { "you".to_string() } else { p.name.clone() };
        o.suggested.insert(p.voice.clone(), name.clone());
        if p.apply && !p.me {
            o.named.insert(p.voice.clone(), name);
        }
    }
    // A 1:1 invite's guess sits on a voice with or without a card.
    for (id, s) in &note.speakers {
        if s.merged_into.is_empty() && !s.name.trim().is_empty() && id != "them" && !o.named.contains_key(id) {
            o.named.insert(id.clone(), s.name.clone());
        }
    }
    o
}

fn score(truth: &BTreeMap<String, Option<String>>, said: &BTreeMap<String, String>) -> (Score, Vec<String>) {
    let mut s = Score::default();
    let mut notes = Vec::new();
    for (voice, t) in truth {
        let got = said.get(voice);
        match (t, got) {
            (Some(t), Some(g)) if same_person(g, t) => s.correct += 1,
            (Some(t), Some(g)) => {
                s.wrong += 1;
                notes.push(format!("{voice}: said {g}, is {t}"));
            }
            (Some(t), None) => {
                s.missed += 1;
                notes.push(format!("{voice}: missed {t}"));
            }
            (None, Some(g)) => {
                s.wrong += 1;
                notes.push(format!("{voice}: said {g}, is nobody to name"));
            }
            (None, None) => s.held_back += 1,
        }
        if t.is_some() {
            s.findable += 1;
        }
    }
    (s, notes)
}

/// The everyday-word test the app uses (the built-in list, then Windows'
/// spell checker).
fn with_ordinary<T>(f: impl FnOnce(&dyn Fn(&str) -> bool) -> T) -> T {
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    f(&ordinary)
}

fn context<'a>(s: &Scenario, ordinary: &'a dyn Fn(&str) -> bool, model: Vec<ModelVote>) -> Context<'a> {
    let mut c = Context::bare(ordinary);
    c.me = s.me.clone();
    c.known = s.known.clone();
    c.model = model;
    c
}

/// Rust alone on one scenario.
pub fn rust_only(s: &Scenario) -> (Note, Vec<Proposal>) {
    let note = note_of(s);
    let proposals = with_ordinary(|o| naming::propose(&note, &context(s, o, Vec::new())).0);
    (note, proposals)
}

/// One scenario's row: both levels, and what was said.
fn row(s: &Scenario, note: &Note, proposals: &[Proposal], extra: Value) -> Value {
    let o = outcome(note, proposals);
    let (named, named_notes) = score(&s.truth, &o.named);
    let (suggested, suggested_notes) = score(&s.truth, &o.suggested);
    let mut r = json!({
        "scenario": s.name,
        "description": s.description,
        "tags": s.tags,
        "named": named,
        "suggested": suggested,
        "named_notes": named_notes,
        "suggested_notes": suggested_notes,
        "outcome": o,
        "proposals": proposals.iter().map(|p| json!({
            "voice": p.voice, "name": p.name, "me": p.me, "score": p.score, "confidence": p.confidence,
            "why": p.why.kind, "apply": p.apply,
        })).collect::<Vec<_>>(),
    });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            r[k] = v;
        }
    }
    r
}

fn totals(rows: &[Value]) -> Value {
    let mut named = Score::default();
    let mut suggested = Score::default();
    for r in rows {
        let get = |k: &str| serde_json::from_value::<ScoreIn>(r[k].clone()).unwrap_or_default().into();
        named.add(get("named"));
        suggested.add(get("suggested"));
    }
    json!({
        "named": named, "named_coverage": named.coverage(),
        "suggested": suggested, "suggested_coverage": suggested.coverage(),
    })
}

#[derive(Default, Deserialize)]
struct ScoreIn {
    correct: usize,
    wrong: usize,
    missed: usize,
    findable: usize,
    held_back: usize,
}

impl From<ScoreIn> for Score {
    fn from(s: ScoreIn) -> Score {
        Score { correct: s.correct, wrong: s.wrong, missed: s.missed, findable: s.findable, held_back: s.held_back }
    }
}

/// Rust alone over every scenario (printed, and returned for the report).
pub fn run_rust(scenarios: &[Scenario]) -> Value {
    let rows: Vec<Value> = scenarios
        .iter()
        .map(|s| {
            let (note, p) = rust_only(s);
            let r = row(s, &note, &p, json!({}));
            print_row(s, &r, None);
            r
        })
        .collect();
    let t = totals(&rows);
    print_totals("Rust only", &t);
    json!({ "rows": rows, "totals": t })
}

/// Reply tokens for one question (as the app).
const REPLY_TOKENS: u32 = 120;

/// Rust plus the AI step, on one model.
pub async fn run_model(scenarios: &[Scenario], ep: &Endpoint<'_>) -> Value {
    let local = ep.provider == "local" || ep.base_url.contains("127.0.0.1") || ep.base_url.contains("localhost");
    let mut rows = Vec::new();
    let (mut calls, mut kept, mut rejected, mut failed, mut total_ms) = (0usize, 0usize, 0usize, 0usize, 0u64);
    let (mut prompt_tokens, mut gen_tokens) = (0u64, 0u64);
    for s in scenarios {
        record::set_context(json!({ "model": ep.model, "scenario": format!("speakers-{}", s.name) }));
        let note = note_of(s);
        let questions = with_ordinary(|o| naming::asks(&note, &context(s, o, Vec::new())));
        let mut votes = Vec::new();
        let mut answers = Vec::new();
        for ask in &questions {
            let opts = crate::llm::ChatOptions {
                temperature: 0.0,
                max_tokens: local.then_some(REPLY_TOKENS),
                timeout: crate::eval::timeout(std::time::Duration::from_secs(if local { 240 } else { 60 })),
                disable_thinking: false,
            };
            let mut extra = serde_json::Map::new();
            if local {
                extra.insert(
                    "response_format".into(),
                    json!({ "type": "json_schema", "json_schema": { "name": "speaker", "schema": naming::ask_schema(ask) } }),
                );
            }
            record::begin_turn();
            let t0 = Instant::now();
            let out = crate::llm::chat_with(ep.base_url, ep.key, ep.model, ep.provider, naming::ask_messages(ask), &opts, extra).await;
            let ms = t0.elapsed().as_millis() as u64;
            let st = record::end_turn();
            calls += 1;
            total_ms += ms;
            prompt_tokens += st.prompt_tokens;
            gen_tokens += st.gen_tokens;
            match out {
                Ok(reply) => {
                    let vote = naming::check_answer(ask, &reply);
                    let truth = s.truth.get(&ask.voice).cloned().flatten();
                    let verdict = match (&vote, &truth) {
                        (Some(v), Some(t)) if same_person(&v.name, t) => "kept, right",
                        (Some(_), _) => "kept, WRONG",
                        (None, _) if reply.contains("unknown") => "unknown",
                        (None, _) => "rejected by Rust",
                    };
                    if vote.is_some() {
                        kept += 1;
                    } else if verdict == "rejected by Rust" {
                        rejected += 1;
                    }
                    answers.push(json!({ "voice": ask.voice, "reply": reply, "verdict": verdict, "ms": ms, "prompt_tokens": st.prompt_tokens }));
                    if let Some(v) = vote {
                        votes.push(v);
                    }
                }
                Err(e) => {
                    failed += 1;
                    answers.push(json!({ "voice": ask.voice, "error": e, "ms": ms }));
                }
            }
        }
        let proposals = with_ordinary(|o| naming::propose(&note, &context(s, o, votes.clone())).0);
        let r = row(s, &note, &proposals, json!({ "asked": questions.len(), "answers": answers }));
        print_row(s, &r, Some(questions.len()));
        rows.push(r);
    }
    let t = totals(&rows);
    print_totals(ep.model, &t);
    json!({
        "rows": rows,
        "totals": t,
        "calls": calls, "kept": kept, "rejected": rejected, "failed": failed,
        "mean_ms": if calls > 0 { total_ms / calls as u64 } else { 0 },
        "mean_prompt_tokens": if calls > 0 { prompt_tokens / calls as u64 } else { 0 },
        "mean_gen_tokens": if calls > 0 { gen_tokens / calls as u64 } else { 0 },
    })
}

fn print_row(s: &Scenario, r: &Value, asked: Option<usize>) {
    let named: Score = serde_json::from_value::<ScoreIn>(r["named"].clone()).unwrap_or_default().into();
    let suggested: Score = serde_json::from_value::<ScoreIn>(r["suggested"].clone()).unwrap_or_default().into();
    let notes: Vec<String> = r["suggested_notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
    println!(
        "  {:<30} {}named {}/{} wrong {}  suggested {}/{} wrong {}  {}",
        s.name,
        asked.map_or(String::new(), |a| format!("asked {a:<2} ")),
        named.correct,
        named.findable,
        named.wrong,
        suggested.correct,
        suggested.findable,
        suggested.wrong,
        notes.join("; ")
    );
}

fn print_totals(label: &str, t: &Value) {
    let g = |k: &str, f: &str| t[k][f].as_u64().unwrap_or(0);
    println!(
        "  {label}: named {} correct, {} wrong, coverage {:.0}%; suggested {} correct, {} wrong, coverage {:.0}%",
        g("named", "correct"),
        g("named", "wrong"),
        t["named_coverage"].as_f64().unwrap_or(0.0) * 100.0,
        g("suggested", "correct"),
        g("suggested", "wrong"),
        t["suggested_coverage"].as_f64().unwrap_or(0.0) * 100.0,
    );
}

/// `speakers.md`: one table per run (Rust only, then each model).
pub fn markdown(runs: &[(String, Value)]) -> String {
    let mut out = String::from("# Speaker names\n\nPer voice: correct / wrong / missed. *Named*: put on the voice as Yap's guess (the action plan reads it). *Suggested*: every card.\n\n");
    out.push_str("| Run | Named correct | Named wrong | Named coverage | Suggested correct | Suggested wrong | Suggested coverage | AI calls (kept / rejected) | Mean call |\n|---|---|---|---|---|---|---|---|---|\n");
    for (label, v) in runs {
        let t = &v["totals"];
        let g = |k: &str, f: &str| t[k][f].as_u64().unwrap_or(0);
        out.push_str(&format!(
            "| {label} | {} / {} | {} | {:.0}% | {} / {} | {} | {:.0}% | {} | {} |\n",
            g("named", "correct"),
            g("named", "findable"),
            g("named", "wrong"),
            t["named_coverage"].as_f64().unwrap_or(0.0) * 100.0,
            g("suggested", "correct"),
            g("suggested", "findable"),
            g("suggested", "wrong"),
            t["suggested_coverage"].as_f64().unwrap_or(0.0) * 100.0,
            if v["calls"].is_null() { "—".to_string() } else { format!("{} ({} / {})", v["calls"], v["kept"], v["rejected"]) },
            if v["calls"].is_null() { "—".to_string() } else { format!("{} ms", v["mean_ms"]) },
        ));
    }
    for (label, v) in runs {
        out.push_str(&format!("\n## {label}\n\n| Scenario | Named | Suggested | Notes |\n|---|---|---|---|\n"));
        for r in v["rows"].as_array().into_iter().flatten() {
            let f = |k: &str| {
                format!(
                    "{}/{} ✗{}",
                    r[k]["correct"].as_u64().unwrap_or(0),
                    r[k]["findable"].as_u64().unwrap_or(0),
                    r[k]["wrong"].as_u64().unwrap_or(0)
                )
            };
            let notes: Vec<String> = r["suggested_notes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect();
            out.push_str(&format!("| {} | {} | {} | {} |\n", r["scenario"].as_str().unwrap_or(""), f("named"), f("suggested"), notes.join("; ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenarios() -> Vec<Scenario> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("evals").join("speakers");
        load(&dir).expect("the speaker scenarios load")
    }

    /// Rust alone never puts a wrong name on a voice, nor suggests one, in
    /// any scenario — the bar for every change to `naming.rs`.
    #[test]
    fn rust_alone_names_no_voice_wrongly() {
        let all = scenarios();
        assert!(all.len() >= 20, "{} scenarios", all.len());
        let v = run_rust(&all);
        let bad: Vec<String> = v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["named"]["wrong"].as_u64() != Some(0) || r["suggested"]["wrong"].as_u64() != Some(0))
            .map(|r| format!("{}: {}", r["scenario"], r["suggested_notes"]))
            .collect();
        assert!(bad.is_empty(), "wrong names: {bad:#?}");
        // And it finds most of what there is to find.
        assert!(v["totals"]["suggested_coverage"].as_f64().unwrap() >= 0.55, "{}", v["totals"]);
    }

    #[test]
    fn same_person_takes_a_first_name() {
        assert!(same_person("Priya Shah", "Priya Shah"));
        assert!(same_person("priya", "Priya Shah"));
        assert!(!same_person("Priya Patel", "Priya Shah"));
        assert!(!same_person("Sam", "Priya Shah"));
    }
}
