//! `--strength`: **cleanup strength, smart formatting and the text before
//! the cursor** (`cleanup_strength.rs`, `cleanup_format.rs`,
//! `cleanup_context.rs`) on each `--model`: does Light keep the words, High
//! tighten without adding, a spoken list come out as a list (and a sentence
//! that only mentions numbers stay a sentence), an email's greeting and
//! sign-off land on lines of their own, and does the text before the cursor
//! help the dictation fit where it goes?
//!
//! Each case in `evals/cleanup/strength.json` is cleaned through the
//! dictation's own code — `cleanup_styles::resolve` for a stand-in app of
//! its kind (`sample_target`), then `cleanup_styles::clean` — twice:
//!
//! - **before**: the call every dictation made until 25 Oct 2026 (Medium,
//!   no formatting line, no text before the cursor);
//! - **after**: the case's strength, the formatting line when a list or an
//!   email frame is said, and the case's text before the cursor.
//!
//! A case with text before the cursor is judged as the field ends up:
//! after `smart_insert::fit_to_context`, which a dictation gets either way.
//! Per phase: the pass rate overall, per group and per strength, and the
//! wall time with llama.cpp's prompt tokens processed.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Value};

use super::cleanup::Endpoint;
use super::record;
use super::styles::Checks as StyleChecks;
use crate::ai::cleanup_strength::Strength;
use crate::ai::cleanup_styles::{self, Category, Tone};

#[derive(Debug, serde::Deserialize)]
pub struct Bench {
    pub cases: Vec<Case>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Case {
    pub name: String,
    pub group: String,
    #[serde(default)]
    pub strength: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tone: String,
    pub text: String,
    /// The text before the cursor (cursor cases).
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub quick: bool,
    #[serde(default)]
    pub checks: Checks,
}

/// What a cleaned text must (not) be: styles.json's checks and more (see
/// strength.json's description).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Checks {
    #[serde(flatten)]
    pub base: StyleChecks,
    pub keeps_words: bool,
    pub added_max: Option<usize>,
    pub ratio_max: Option<f64>,
    pub ratio_min: Option<f64>,
    pub list_items: Option<usize>,
    pub no_list: bool,
    pub ends_with: Option<String>,
    pub not_starts_with: Option<String>,
    pub first_line_max_words: Option<usize>,
    pub last_line_has_any: Vec<String>,
}

pub const PHASES: &[&str] = &["before", "after"];
pub const GROUPS: &[&str] = &["strength", "format", "cursor"];

impl Case {
    fn category(&self) -> Category {
        Category::from_id(&self.category).unwrap_or(Category::Other)
    }
    fn strength(&self) -> Strength {
        Strength::from_id(&self.strength).unwrap_or(Strength::Medium)
    }
}

pub fn load(path: &Path) -> Result<Bench, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let b: Bench = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for c in &b.cases {
        if !GROUPS.contains(&c.group.as_str()) {
            return Err(format!("{}: no group {}", c.name, c.group));
        }
        if !c.category.is_empty() && Category::from_id(&c.category).is_none() {
            return Err(format!("{}: no kind of app {}", c.name, c.category));
        }
        if !c.strength.is_empty()
            && matches!(Strength::from_id(&c.strength), None | Some(Strength::None))
        {
            return Err(format!("{}: no strength {}", c.name, c.strength));
        }
        if !c.tone.is_empty() {
            let tone =
                Tone::from_id(&c.tone).ok_or_else(|| format!("{}: no tone {}", c.name, c.tone))?;
            if !c.category().tones().contains(&tone) {
                return Err(format!(
                    "{}: {} doesn't offer {}",
                    c.name,
                    c.category().id(),
                    c.tone
                ));
            }
        }
    }
    if b.cases.is_empty() {
        return Err(format!("{}: no cases", path.display()));
    }
    Ok(b)
}

/// Filler sounds, as speech-to-text writes them: never required back.
const SOUNDS: &[&str] = &["um", "umm", "uh", "uhh", "er", "erm", "ah", "hmm", "mm"];

/// Lower-case words, apostrophes dropped ("doesn't" = "doesnt").
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}')
        .map(|w| w.replace(['\'', '\u{2019}'], "").to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// A number word as digits ("two" → "2"), for a cleanup that writes them so.
fn digits(w: &str) -> Option<&'static str> {
    Some(match w {
        "one" => "1",
        "two" => "2",
        "three" => "3",
        "four" => "4",
        "five" => "5",
        "six" => "6",
        "seven" => "7",
        "eight" => "8",
        "nine" => "9",
        "ten" => "10",
        _ => return None,
    })
}

/// The words a Light cleanup must keep: the dictation's, filler sounds,
/// "you know" and a word said twice in a row out (the same for its output:
/// a kept "you know" isn't an added word). Pure.
fn said_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let w: Vec<String> = words(text)
        .into_iter()
        .filter(|w| !SOUNDS.contains(&w.as_str()))
        .collect();
    let mut i = 0;
    while i < w.len() {
        if w[i] == "you" && w.get(i + 1).is_some_and(|n| n == "know") {
            i += 2;
            continue;
        }
        if out.last() != Some(&w[i]) {
            out.push(w[i].clone());
        }
        i += 1;
    }
    out
}

/// (missing, added): the said words not found in order in `out`, and the
/// words `out` has beyond them. Pure.
fn kept(said: &[String], out: &[String]) -> (usize, usize) {
    let mut at = 0;
    let mut matched = 0;
    let mut missing = 0;
    for w in said {
        let alt = digits(w);
        match out[at..]
            .iter()
            .position(|o| o == w || alt.is_some_and(|d| o == d))
        {
            Some(p) => {
                at += p + 1;
                matched += 1;
            }
            None => missing += 1,
        }
    }
    (missing, out.len().saturating_sub(matched))
}

/// A line that starts a list item: "1.", "1)", "-", "*", "•".
fn list_line(line: &str) -> bool {
    let l = line.trim_start();
    if let Some(rest) = l.strip_prefix(['-', '*', '•']) {
        return rest.starts_with(' ');
    }
    let digits_end = l.find(|c: char| !c.is_ascii_digit()).unwrap_or(l.len());
    digits_end > 0 && (l[digits_end..].starts_with(". ") || l[digits_end..].starts_with(") "))
}

/// The checks `output` fails (empty: it passes); `input`: the dictation.
/// Pure.
pub fn failures(checks: &Checks, input: &str, output: &str) -> Vec<String> {
    let mut f = super::styles::failures(&checks.base, output);
    let out = output.trim();
    if checks.base.empty || out.is_empty() {
        return f;
    }
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    if checks.keeps_words {
        let (missing, added) = kept(&said_words(input), &said_words(out));
        if missing > 0 {
            f.push(format!("{missing} word(s) said not kept in order"));
        }
        let max = checks.added_max.unwrap_or(1);
        if added > max {
            f.push(format!("{added} word(s) added"));
        }
    }
    let content = words(input)
        .into_iter()
        .filter(|w| !SOUNDS.contains(&w.as_str()))
        .count()
        .max(1);
    let ratio = words(out).len() as f64 / content as f64;
    if let Some(max) = checks.ratio_max {
        if ratio > max {
            f.push(format!("{:.2} of the words said, over {max}", ratio));
        }
    }
    if let Some(min) = checks.ratio_min {
        if ratio < min {
            f.push(format!("{:.2} of the words said, under {min}", ratio));
        }
    }
    let items = lines.iter().filter(|l| list_line(l)).count();
    if let Some(n) = checks.list_items {
        if items < n {
            f.push(format!("{items} list line(s), not {n}+"));
        }
    }
    if checks.no_list && (items > 0 || lines.len() > 1) {
        f.push(format!(
            "laid out: {} line(s), {items} list line(s)",
            lines.len()
        ));
    }
    if let Some(e) = &checks.ends_with {
        if !out.ends_with(e.as_str()) {
            f.push(format!("doesn't end \"{e}\""));
        }
    }
    if let Some(s) = &checks.not_starts_with {
        if out.to_lowercase().starts_with(&s.to_lowercase()) {
            f.push(format!("starts \"{s}\""));
        }
    }
    if let Some(n) = checks.first_line_max_words {
        let w = lines
            .first()
            .map(|l| l.split_whitespace().count())
            .unwrap_or(0);
        if w > n {
            f.push(format!("first line {w} words, over {n}"));
        }
    }
    if !checks.last_line_has_any.is_empty() {
        let last = lines.last().map(|l| l.to_lowercase()).unwrap_or_default();
        if !checks
            .last_line_has_any
            .iter()
            .any(|h| last.contains(&h.to_lowercase()))
        {
            f.push(format!(
                "last line has none of {:?}",
                checks.last_line_has_any
            ));
        }
    }
    f
}

/// One cleaned case.
#[derive(Debug, Clone, serde::Serialize)]
struct Shot {
    phase: &'static str,
    case: String,
    group: String,
    strength: String,
    input: String,
    before: String,
    output: String,
    error: Option<String>,
    failures: Vec<String>,
    ms: u64,
    asked: bool,
    processed_tokens: u64,
    gen_tokens: u64,
    total_tokens: u64,
}

/// The settings a phase runs on: the defaults with every kind's style
/// (styles on, as a default install has them), the case's tone for its
/// kind, and its strength (after) or Medium (before).
fn settings(case: &Case, after: bool) -> crate::config::YapConfig {
    let mut cfg = crate::config::YapConfig::default();
    cleanup_styles::on_load(&mut cfg);
    cfg.cleanup_styles = Some(true);
    if !case.tone.is_empty() {
        if let Some(p) = cfg
            .cleanup_profiles
            .iter_mut()
            .find(|p| p.category == case.category().id())
        {
            p.tone = case.tone.clone();
        }
    }
    cfg.cleanup_strength = if after {
        case.strength().id().to_string()
    } else {
        "medium".to_string()
    };
    cfg
}

async fn clean(phase: &'static str, case: &Case, ep: &Endpoint<'_>) -> Shot {
    let after = phase == "after";
    let cfg = settings(case, after);
    let spelling = crate::ai::about_you::cleanup_spelling(&cfg);
    record::begin_turn();
    let paced0 = record::paced_ms();
    let t0 = Instant::now();
    let asked;
    let out = match cleanup_styles::resolve(&cfg, &cleanup_styles::sample_target(case.category())) {
        Some(mut plan) => {
            plan.formatting = after;
            asked = !cleanup_styles::filler_only(&case.text);
            let before = if after {
                crate::ai::cleanup_context::bounded(&case.before)
            } else {
                None
            };
            let cursor = before.as_deref().map(crate::ai::cleanup_context::line);
            cleanup_styles::clean(
                &case.text,
                ep.base_url,
                ep.key,
                ep.model,
                ep.provider,
                &plan,
                spelling.as_deref(),
                cursor.as_deref(),
                &[],
                false,
            )
            .await
            .map(|out| match &before {
                // As a dictation does: what it wrote out again goes.
                Some(b) => crate::ai::cleanup_context::trim_echo(b, &out),
                None => out,
            })
        }
        None => {
            asked = false;
            Ok(case.text.clone())
        }
    };
    let ms =
        (t0.elapsed().as_millis() as u64).saturating_sub(record::paced_ms().saturating_sub(paced0));
    let st = record::end_turn();
    let (output, error) = match out {
        Ok(o) => (o, None),
        Err(e) => (String::new(), Some(e)),
    };
    // What the field ends up with: fitted to the text before the cursor,
    // as a dictation is either way ("Match the text around the cursor").
    let output = if case.before.is_empty() || error.is_some() {
        output
    } else {
        let ctx = crate::pasting::smart_insert::FieldContext {
            before: case.before.clone(),
            ..Default::default()
        };
        crate::pasting::smart_insert::fit_to_context(&output, &ctx, &[])
            .trim()
            .to_string()
    };
    let failures = if error.is_some() {
        vec!["error".to_string()]
    } else {
        failures(&case.checks, &case.text, &output)
    };
    Shot {
        phase,
        case: case.name.clone(),
        group: case.group.clone(),
        strength: case.strength().id().to_string(),
        input: case.text.clone(),
        before: case.before.clone(),
        output,
        error,
        failures,
        ms,
        asked,
        processed_tokens: st.prompt_eval_tokens,
        gen_tokens: st.gen_tokens,
        total_tokens: st.total_tokens,
    }
}

/// Run the cases on one model: `phases` × `rounds`. `only`: case names (or
/// groups) containing one of these (`--only`; empty = all).
pub async fn run(
    bench: &Bench,
    ep: &Endpoint<'_>,
    rounds: u32,
    phases: &[&'static str],
    quick: bool,
    only: &[String],
) -> Value {
    let cases: Vec<&Case> = bench
        .cases
        .iter()
        .filter(|c| !quick || c.quick)
        .filter(|c| {
            only.is_empty()
                || only
                    .iter()
                    .any(|o| c.name.contains(o.as_str()) || c.group == *o)
        })
        .collect();
    let mut shots: Vec<Shot> = Vec::new();
    for round in 0..rounds.max(1) {
        for case in &cases {
            for &phase in phases {
                let s = clean(phase, case, ep).await;
                if round == 0 {
                    println!(
                        "  {phase:<6} {:<38} {:>5} ms  {}",
                        case.name,
                        s.ms,
                        if s.failures.is_empty() {
                            "pass".to_string()
                        } else {
                            format!("FAIL: {}", s.failures.join("; "))
                        }
                    );
                }
                shots.push(s);
            }
        }
    }
    let mut summary: Vec<Value> = Vec::new();
    for &phase in phases {
        let mine: Vec<&Shot> = shots.iter().filter(|s| s.phase == phase).collect();
        let passed = mine.iter().filter(|s| s.failures.is_empty()).count();
        let tally = |of: Vec<&&Shot>| json!({ "n": of.len(), "passed": of.iter().filter(|s| s.failures.is_empty()).count() });
        let groups: Vec<Value> = GROUPS
            .iter()
            .map(|g| json!({ "group": g, "tally": tally(mine.iter().filter(|s| s.group == *g).collect()) }))
            .collect();
        let strengths: Vec<Value> = ["light", "medium", "high"]
            .iter()
            .map(|st| json!({ "strength": st, "tally": tally(mine.iter().filter(|s| s.group == "strength" && s.strength == *st).collect()) }))
            .collect();
        let mut ms: Vec<u64> = mine.iter().filter(|s| s.asked).map(|s| s.ms).collect();
        ms.sort_unstable();
        let n = ms.len().max(1) as u64;
        let tokens: u64 = mine.iter().map(|s| s.total_tokens).sum();
        println!(
            "  {phase}: {passed}/{} pass ({:.0}%), mean {} ms, {tokens} tokens",
            mine.len(),
            if mine.is_empty() {
                0.0
            } else {
                passed as f64 * 100.0 / mine.len() as f64
            },
            ms.iter().sum::<u64>() / n
        );
        summary.push(json!({
            "phase": phase,
            "n": mine.len(),
            "passed": passed,
            "mean_ms": ms.iter().sum::<u64>() / n,
            "median_ms": ms.get(ms.len() / 2).copied().unwrap_or(0),
            "mean_processed_tokens": mine.iter().map(|s| s.processed_tokens).sum::<u64>() / n,
            "mean_gen_tokens": mine.iter().map(|s| s.gen_tokens).sum::<u64>() / n,
            "tokens": tokens,
            "groups": groups,
            "strengths": strengths,
        }));
    }
    json!({ "phases": summary, "shots": shots })
}

/// strength.md: pass rates per model, phase, group and strength, then every
/// output.
pub fn markdown(models: &[(String, Value)]) -> String {
    let mut md = String::from(
        "# Cleanup strength, formatting and the text before the cursor\n\nEach case cleaned as before (Medium, no formatting line, no text before the cursor) and after (its strength, the formatting line when one is said, the text before the cursor); a cursor case judged after `smart_insert::fit_to_context`. Pass = every check held.\n\n| Model | Phase | Passed | Strength | Format | Cursor | Light | Medium | High | Mean ms | Prompt tok processed | Tokens |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let t = |v: &Value| format!("{}/{}", v["passed"], v["n"]);
    for (label, r) in models {
        for p in r["phases"].as_array().into_iter().flatten() {
            let g = |name: &str| {
                p["groups"]
                    .as_array()
                    .and_then(|a| a.iter().find(|x| x["group"] == name))
                    .map(|x| t(&x["tally"]))
                    .unwrap_or_default()
            };
            let s = |name: &str| {
                p["strengths"]
                    .as_array()
                    .and_then(|a| a.iter().find(|x| x["strength"] == name))
                    .map(|x| t(&x["tally"]))
                    .unwrap_or_default()
            };
            md.push_str(&format!(
                "| {label} | {} | {}/{} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                p["phase"].as_str().unwrap_or(""),
                p["passed"],
                p["n"],
                g("strength"),
                g("format"),
                g("cursor"),
                s("light"),
                s("medium"),
                s("high"),
                p["mean_ms"],
                p["mean_processed_tokens"],
                p["tokens"]
            ));
        }
    }
    md.push_str("\n## Outputs\n");
    for (label, r) in models {
        md.push_str(&format!("\n### {label}\n\n"));
        for s in r["shots"].as_array().into_iter().flatten() {
            let out = match s["error"].as_str() {
                Some(e) => format!("(error: {e})"),
                None => s["output"].as_str().unwrap_or("").replace('\n', " / "),
            };
            let verdict = match s["failures"].as_array().filter(|f| !f.is_empty()) {
                Some(f) => format!(
                    "FAIL: {}",
                    f.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                None => "pass".to_string(),
            };
            let before = s["before"]
                .as_str()
                .filter(|b| !b.is_empty())
                .map(|b| format!(" after \"{}\"", b.replace('\n', " / ")))
                .unwrap_or_default();
            md.push_str(&format!(
                "- {} · {} ({}, {}): \"{}\"{before} → \"{}\" — {verdict} ({} ms)\n",
                s["phase"].as_str().unwrap_or(""),
                s["case"].as_str().unwrap_or(""),
                s["group"].as_str().unwrap_or(""),
                s["strength"].as_str().unwrap_or(""),
                s["input"].as_str().unwrap_or(""),
                out,
                s["ms"]
            ));
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_parse_and_each_group_has_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/cleanup/strength.json");
        let b = load(&path).expect("strength.json");
        for g in GROUPS {
            assert!(b.cases.iter().filter(|c| c.group == *g).count() >= 5, "{g}");
        }
        for st in ["light", "medium", "high"] {
            assert!(
                b.cases
                    .iter()
                    .any(|c| c.group == "strength" && c.strength().id() == st),
                "{st}"
            );
        }
        // A cloud model's run keeps to ~11 cleanups (~20k Groq tokens).
        assert!(b.cases.iter().filter(|c| c.quick).count() <= 11);
        // Every list case is one Rust sends the list line for, and so is
        // every "no list" case with a list's words (the model decides).
        for c in b.cases.iter().filter(|c| c.name.starts_with("list-")) {
            assert!(
                crate::ai::cleanup_format::list_cue(&c.text).is_some(),
                "{}",
                c.name
            );
        }
    }

    #[test]
    fn the_checks() {
        let c = |v: Value| serde_json::from_value::<Checks>(v).unwrap();
        // Light: every word kept in order (fillers and a stutter out).
        let keep = c(json!({ "keeps_words": true }));
        let said = "so um i was thinking we should uh move the the meeting";
        assert!(failures(
            &keep,
            said,
            "So, I was thinking we should move the meeting."
        )
        .is_empty());
        assert_eq!(
            failures(&keep, said, "I think we should move the meeting."),
            vec!["3 word(s) said not kept in order"]
        );
        assert!(
            failures(&keep, "it were two of them", "It were 2 of them.").is_empty(),
            "a number word as digits"
        );
        assert_eq!(
            failures(&keep, "send it", "Please send it now."),
            vec!["2 word(s) added"]
        );
        // Lists.
        let list = c(json!({ "list_items": 3 }));
        assert!(failures(&list, "x", "We need:\n1. Venue\n2. Catering\n3. Invites").is_empty());
        assert!(failures(&list, "x", "- Milk\n- Eggs\n• Bread").is_empty());
        assert_eq!(
            failures(&list, "x", "1. Venue, 2. Catering, 3. Invites"),
            vec!["1 list line(s), not 3+"]
        );
        let none = c(json!({ "no_list": true }));
        assert!(failures(&none, "x", "I have one cat, two dogs and three fish.").is_empty());
        assert_eq!(
            failures(&none, "x", "I have:\n- one cat"),
            vec!["laid out: 2 line(s), 1 list line(s)"]
        );
        // Ratios, ends, starts, an email's lines.
        assert_eq!(
            failures(&c(json!({ "ratio_max": 0.5 })), "a b c d", "a b c"),
            vec!["0.75 of the words said, over 0.5"]
        );
        assert!(failures(
            &c(json!({ "ends_with": "?" })),
            "x",
            "the venue has parking?"
        )
        .is_empty());
        assert_eq!(
            failures(
                &c(json!({ "not_starts_with": "send me" })),
                "x",
                "Send me the report."
            ),
            vec!["starts \"send me\""]
        );
        let email = c(
            json!({ "first_line_max_words": 3, "last_line_has_any": ["Tom", "regards"], "lines_min": 3 }),
        );
        assert!(failures(
            &email,
            "x",
            "Hi Priya,\n\nThanks for the contract.\n\nBest regards,\nTom"
        )
        .is_empty());
        assert_eq!(
            failures(
                &email,
                "x",
                "Hi Priya, thanks for the contract. Best regards, Tom"
            )
            .len(),
            2
        );
    }
}
