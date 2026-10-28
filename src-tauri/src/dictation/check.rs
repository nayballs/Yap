//! `yap.exe dictation-check` (debug builds): **your own dictations as a test
//! set**, for any change to speech-to-text or cleanup. Two bugs only real
//! use found (2026-10-27/28): Parakeet writing "definitive d d d d d d
//! edition" in about one dictation in three, and a cleanup that turned 114
//! words into "I've." — the test audio and the stand-in models never did.
//!
//! - `collect <dir> [--from <folder>…]`: every dictation whose audio Yap
//!   still has (`recent-audio/`: a day, or kept) — or a `--from` folder has,
//!   as `<audio id>.wav` — with its history entry, into `<dir>/cases/` as
//!   `<id>.wav` + `<id>.json` ([`Case`]). One already there is left as it is
//!   (its corrections kept). `said`, the reference, starts as what the model
//!   heard then: correct it by hand and set `checked`.
//! - `run <dir> [--model <id>] [--cpu] [--cleanup [<local model>]]
//!   [--only <id,…>] [--baseline <file>] [--set-baseline]`: each case through
//!   today's speech model (`--model`, else the one it was dictated with), the
//!   final pass as a dictation's (the live preview: `partials-dump`), scored
//!   ([`score`]): the word error rate against `said`, and output broken
//!   whatever was said (a word on repeat, nothing heard). `--cleanup`: then
//!   through the cleanup's own code (`cleanup_styles::clean`, the person's
//!   settings, the app each was dictated into) on the on-device model
//!   (`--cleanup`'s model, else the one set for cleanup), served by Yap's
//!   llamafile for the run; a cleanup that errs or lost the dictation is
//!   flagged. Compared with `<dir>/baseline.json` (or `--baseline`): a case
//!   5 points worse, or newly flagged, is a regression. The run is written to
//!   `<dir>/runs/<unix secs>.json`; `--set-baseline` makes it the baseline.
//!   Exit 1 on anything flagged or worse.
//!
//! **Read only for Yap**: Yap's history, audio, settings and models are read
//! from their files; anything Yap's code would write while it runs goes to a
//! scratch data folder (`portable::use_data_dir`, as `yap.exe bench` does).
//! The folder holds a person's voice and words: keep it on their PC, never in
//! the public repo (the maintainer's lives in the private docs repo, ignored
//! by git: `dictation-tests/`).

// Scoring is `run`'s, which needs the speech engines.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub mod score;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const USAGE: &str = "usage: yap.exe dictation-check collect <dir> [--from <folder>…]\n       \
yap.exe dictation-check run <dir> [--model <id>] [--cpu] [--cleanup [<local model>]] [--only <id,…>] [--baseline <file>] [--set-baseline]";

#[cfg(feature = "engines")]
const RATE: u32 = 16_000;

/// One dictation of the set: `cases/<audio>.json`, its audio beside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    /// Its audio's id (`recent_audio.rs`): `cases/<audio>.wav`.
    pub audio: String,
    /// When it was pasted (unix secs, its history entry's `ts`).
    pub recorded: u64,
    /// The speech model it was dictated with.
    pub model: String,
    /// The app it went into (cleanup's style follows it).
    #[serde(default)]
    pub app: String,
    /// What the speech model wrote then.
    pub heard: String,
    /// What was pasted then (after cleanup, the dictionary…).
    #[serde(default)]
    pub pasted: String,
    /// What was said: the reference. Starts as `heard`; correct it by hand.
    pub said: String,
    /// `said` has been read and corrected.
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub note: String,
}

/// One case in a run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outcome {
    pub audio: String,
    pub model: String,
    pub seconds: f64,
    /// What the speech model wrote this run.
    pub heard: String,
    /// Against the case's `said` ([`score::wer`]).
    pub wer: f64,
    #[serde(default)]
    pub flags: Vec<score::Flag>,
    /// The transcription's time (ms).
    pub ms: u64,
    /// `--cleanup`: what cleanup made of `heard`…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned: Option<String>,
    /// …or why it didn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_error: Option<String>,
}

/// A run: `runs/<when>.json`, and `baseline.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Run {
    pub when: u64,
    /// `git describe` of the build, when it could be read.
    #[serde(default)]
    pub commit: String,
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_model: Option<String>,
    pub cases: Vec<Outcome>,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).filter(|v| !v.starts_with("--")).cloned()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The command (after "dictation-check").
pub fn main(args: Vec<String>) -> i32 {
    // Yap's own folder, read from; anything Yap's code writes goes to a
    // scratch one (before anything asks where the data folder is).
    let yap = crate::shell::portable::would_use();
    let scratch = std::env::temp_dir().join("yap-dictation-check");
    let _ = std::fs::create_dir_all(scratch.join("data"));
    crate::shell::portable::use_data_dir(scratch.join("data"));
    let dir = args.get(1).filter(|a| !a.starts_with("--")).map(PathBuf::from);
    match (args.first().map(String::as_str), dir) {
        (Some("collect"), Some(dir)) => collect(&yap, &dir, &args[2..]),
        (Some("run"), Some(dir)) => run(&yap, &dir, &args[2..], &scratch),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

// ---- collect ---------------------------------------------------------------------------

/// Where Yap keeps a dictation's audio (`recent_audio.rs`: the day's, or kept).
fn yap_audio(yap: &Path, id: &str) -> Option<Vec<u8>> {
    let dir = yap.join("recent-audio");
    std::fs::read(dir.join(format!("{id}.wav"))).or_else(|_| std::fs::read(dir.join("kept").join(format!("{id}.wav")))).ok()
}

/// A history entry worth a case: transcribed (pasted or not), with its
/// audio's id and words. Pure.
fn usable(e: &crate::dictation::history::HistoryEntry) -> Option<&str> {
    let transcribed = e.status.as_deref().is_none_or(|s| s == crate::dictation::history::NOT_PASTED);
    e.audio.as_deref().filter(|id| transcribed && !e.raw.trim().is_empty() && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
}

fn collect(yap: &Path, dir: &Path, args: &[String]) -> i32 {
    let from: Vec<PathBuf> = args.windows(2).filter(|w| w[0] == "--from").map(|w| PathBuf::from(&w[1])).collect();
    let history: Vec<crate::dictation::history::HistoryEntry> =
        match std::fs::read_to_string(yap.join("history.json")).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("Couldn't read Yap's history: {e}");
                return 1;
            }
        };
    let cases = dir.join("cases");
    if let Err(e) = std::fs::create_dir_all(&cases) {
        eprintln!("Couldn't make {}: {e}", cases.display());
        return 1;
    }
    let (mut added, mut there, mut gone) = (0, 0, 0);
    for e in &history {
        let Some(id) = usable(e) else { continue };
        let json_path = cases.join(format!("{id}.json"));
        if json_path.exists() {
            there += 1;
            continue;
        }
        let Some(wav) = yap_audio(yap, id).or_else(|| from.iter().find_map(|d| std::fs::read(d.join(format!("{id}.wav"))).ok())) else {
            gone += 1;
            continue;
        };
        let case = Case {
            audio: id.to_string(),
            recorded: e.ts,
            model: e.model.clone(),
            app: e.app.clone(),
            heard: e.raw.clone(),
            pasted: e.text.clone(),
            said: e.raw.clone(),
            checked: false,
            note: String::new(),
        };
        let written = std::fs::write(cases.join(format!("{id}.wav")), &wav)
            .and_then(|()| std::fs::write(&json_path, serde_json::to_string_pretty(&case).unwrap_or_default()));
        if let Err(err) = written {
            eprintln!("Couldn't write case {id}: {err}");
            return 1;
        }
        added += 1;
    }
    println!("{added} dictations added to {}, {there} already there, {gone} without their audio any more", cases.display());
    0
}

// ---- run -------------------------------------------------------------------------------

fn load_cases(dir: &Path, only: Option<&str>) -> Result<Vec<Case>, String> {
    let wanted: Option<Vec<&str>> = only.map(|o| o.split(',').map(str::trim).filter(|s| !s.is_empty()).collect());
    let mut cases = Vec::new();
    let entries = std::fs::read_dir(dir.join("cases")).map_err(|e| format!("{}: {e}", dir.join("cases").display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let case: Case = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if wanted.as_ref().is_none_or(|w| w.contains(&case.audio.as_str())) {
            cases.push(case);
        }
    }
    cases.sort_by(|a, b| a.audio.cmp(&b.audio));
    Ok(cases)
}

/// A case's audio as 16 kHz mono.
#[cfg(feature = "engines")]
fn samples(dir: &Path, case: &Case) -> Result<Vec<f32>, String> {
    let path = dir.join("cases").join(format!("{}.wav", case.audio));
    let pcm = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| crate::voice::tts::parse_wav(&b))?;
    Ok(crate::dictation::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, RATE))
}

/// Each case through the speech model, scored.
#[cfg(feature = "engines")]
fn transcribe(yap: &Path, dir: &Path, cases: &[Case], model: Option<&str>, cpu: bool) -> Result<Vec<Outcome>, String> {
    use crate::dictation::stt::{create_stt_engine_on, Device};
    let mut models: Vec<&str> = Vec::new();
    for c in cases {
        let m = model.unwrap_or(&c.model);
        if !models.contains(&m) {
            models.push(m);
        }
    }
    let mut out = Vec::new();
    for m in models {
        let engine = create_stt_engine_on(yap, m, if cpu { Device::Cpu } else { Device::Gpu }).map_err(|e| format!("Couldn't load {m}: {e}"))?;
        // Warm, as a dictation's engine is.
        let _ = engine.transcribe(&vec![0.0; RATE as usize], None, false, None);
        for case in cases.iter().filter(|c| model.unwrap_or(&c.model) == m) {
            let audio = samples(dir, case).map_err(|e| format!("case {}: {e}", case.audio))?;
            let started = std::time::Instant::now();
            let heard = engine.transcribe(&audio, None, false, None).unwrap_or_default();
            let ms = started.elapsed().as_millis() as u64;
            let (said, words) = (score::words(&case.said), score::words(&heard));
            out.push(Outcome {
                audio: case.audio.clone(),
                model: m.to_string(),
                seconds: (audio.len() as f64 / RATE as f64 * 10.0).round() / 10.0,
                wer: (score::wer(&said, &words) * 1000.0).round() / 1000.0,
                flags: score::flags(&said, &words),
                heard,
                ms,
                ..Outcome::default()
            });
        }
    }
    out.sort_by(|a, b| a.audio.cmp(&b.audio));
    Ok(out)
}

#[cfg(not(feature = "engines"))]
fn transcribe(_yap: &Path, _dir: &Path, _cases: &[Case], _model: Option<&str>, _cpu: bool) -> Result<Vec<Outcome>, String> {
    Err("dictation-check run needs the speech engines: build with --features engines".into())
}

/// The context the run's cleanup server gets: a cleanup is ~1k tokens.
const CLEANUP_CTX: u32 = 8_192;

/// Each outcome's words through the cleanup's own code on the on-device
/// model `spec` (else the one set for cleanup), served for the run. Returns
/// the model's name.
async fn clean_up(yap: &Path, cases: &[Case], outcomes: &mut [Outcome], spec: Option<&str>, scratch: &Path) -> Result<String, String> {
    use crate::ai::local_llm;
    let cfg = crate::config::peek(yap).unwrap_or_default();
    let spec = spec.map(str::to_string).unwrap_or_else(|| cfg.pp_local_model.clone());
    let models_dir = scratch.join("models");
    let runtime = crate::ai::eval::local::runtime(None, &models_dir).await?;
    let gguf = crate::ai::eval::local::model(&spec, &models_dir).await?;
    // As Yap's sidecar would run it here (MTP and the cache as it decides),
    // from the person's settings as read — never `config::load`, which may
    // save a migration.
    let version = local_llm::runtime::probe_version(&runtime)
        .await
        .or_else(|| local_llm::runtime::Version::parse(local_llm::RUNTIME_VERSION))
        .ok_or("the llamafile runtime's version")?;
    let yap_plan = local_llm::plan_for(&cfg, &gguf, CLEANUP_CTX, version);
    let plan = local_llm::Plan { runtime: version, mtp: yap_plan.mtp, kv_q8: yap_plan.kv_q8, per_slot: false };
    let server = crate::ai::eval::local::start(&runtime, &gguf, 999, CLEANUP_CTX, plan, &[], scratch, false).await?;
    let model = local_llm::model_id(&gguf);
    let spelling = crate::dictation::languages::cleanup_spelling(&cfg);
    for o in outcomes.iter_mut() {
        let Some(case) = cases.iter().find(|c| c.audio == o.audio) else { continue };
        let kind = crate::pasting::app_kind::classify(Some(&case.app), "", "");
        let target = crate::ai::cleanup_styles::Target::new(Some(&case.app), "", kind);
        let Some(plan) = crate::ai::cleanup_styles::resolve(&cfg, &target) else {
            o.cleanup_error = Some("no cleanup for this app".into());
            continue;
        };
        let result = crate::ai::cleanup_styles::clean(
            &o.heard,
            &server.base_url,
            "",
            &model,
            local_llm::PROVIDER_ONDEVICE,
            &plan,
            spelling.as_deref(),
            None,
            &cfg.dictionary,
            cfg.pp_disable_thinking,
        )
        .await;
        match result {
            Ok(text) => o.cleaned = Some(text),
            Err(e) => o.cleanup_error = Some(e),
        }
    }
    Ok(model)
}

fn commit() -> String {
    std::process::Command::new("git")
        .args(["describe", "--always", "--dirty"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// A line of `text`, at most `n` characters.
fn short(text: &str, n: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > n {
        format!("{}…", t.chars().take(n).collect::<String>())
    } else {
        t
    }
}

/// Prints the run (and what's worse than `baseline`); true when anything
/// is flagged or worse.
fn report(cases: &[Case], run: &Run, baseline: Option<&Run>) -> bool {
    let mut bad = false;
    let (mut said_words, mut errors) = (0.0, 0.0);
    println!("{:<15} {:>6} {:>7} {:>7}", "case", "secs", "WER", "before");
    for o in &run.cases {
        let case = cases.iter().find(|c| c.audio == o.audio);
        let n = case.map_or(0, |c| score::words(&c.said).len()) as f64;
        said_words += n;
        errors += o.wer * n;
        let was = baseline.and_then(|b| b.cases.iter().find(|p| p.audio == o.audio));
        let mut notes: Vec<String> = o.flags.iter().map(score::Flag::describe).collect();
        if let Some(w) = was.filter(|w| score::worse(w.wer, o.wer)) {
            notes.push(format!("worse than the baseline ({:.0}% → {:.0}%)", w.wer * 100.0, o.wer * 100.0));
        }
        if let Some(w) = was {
            for f in o.flags.iter().filter(|f| !w.flags.contains(f)) {
                notes.push(format!("new since the baseline: {}", f.describe()));
            }
        }
        if let Some(e) = &o.cleanup_error {
            notes.push(format!("cleanup: {e}"));
        }
        let flagged = !o.flags.is_empty() || o.cleanup_error.as_deref().is_some_and(|e| e != "no cleanup for this app") || notes.iter().any(|n| n.starts_with("worse"));
        bad |= flagged;
        let before = was.map_or("—".to_string(), |w| format!("{:.0}%", w.wer * 100.0));
        println!(
            "{:<15} {:>6.1} {:>6.0}% {:>7}  {}{}",
            o.audio,
            o.seconds,
            o.wer * 100.0,
            before,
            if case.is_some_and(|c| c.checked) { "" } else { "(unchecked) " },
            notes.join("; ")
        );
        if flagged {
            println!("    heard: {}", short(&o.heard, 160));
            if let Some(c) = case {
                println!("    said:  {}", short(&c.said, 160));
            }
            if let Some(c) = &o.cleaned {
                println!("    clean: {}", short(c, 160));
            }
        }
    }
    let overall = if said_words > 0.0 { errors / said_words * 100.0 } else { 0.0 };
    let flagged = run.cases.iter().filter(|o| !o.flags.is_empty()).count();
    println!(
        "\n{} cases, {:.0} words said: {:.1}% of words wrong overall; {} with broken output{}",
        run.cases.len(),
        said_words,
        overall,
        flagged,
        if bad { "; see above" } else { "" }
    );
    bad
}

fn run(yap: &Path, dir: &Path, args: &[String], scratch: &Path) -> i32 {
    let cases = match load_cases(dir, arg_value(args, "--only").as_deref()) {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            eprintln!("No cases in {}: `yap.exe dictation-check collect {}` first", dir.join("cases").display(), dir.display());
            return 2;
        }
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let cpu = args.iter().any(|a| a == "--cpu");
    let mut outcomes = match transcribe(yap, dir, &cases, arg_value(args, "--model").as_deref(), cpu) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut cleanup_model = None;
    if args.iter().any(|a| a == "--cleanup") {
        let spec = arg_value(args, "--cleanup");
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        };
        match rt.block_on(clean_up(yap, &cases, &mut outcomes, spec.as_deref(), scratch)) {
            Ok(m) => cleanup_model = Some(m),
            Err(e) => {
                eprintln!("Cleanup didn't run: {e}");
                return 1;
            }
        }
    }
    let run = Run { when: now_secs(), commit: commit(), device: if cpu { "cpu" } else { "gpu" }.into(), cleanup_model, cases: outcomes };
    let baseline_path = arg_value(args, "--baseline").map(PathBuf::from).unwrap_or_else(|| dir.join("baseline.json"));
    let baseline: Option<Run> = std::fs::read_to_string(&baseline_path).ok().and_then(|s| serde_json::from_str(&s).ok());
    let bad = report(&cases, &run, baseline.as_ref());
    let json = serde_json::to_string_pretty(&run).unwrap_or_default();
    let runs = dir.join("runs");
    let written = std::fs::create_dir_all(&runs).and_then(|()| std::fs::write(runs.join(format!("{}.json", run.when)), &json));
    if let Err(e) = written {
        eprintln!("Couldn't write the run: {e}");
        return 1;
    }
    if args.iter().any(|a| a == "--set-baseline") {
        if let Err(e) = std::fs::write(dir.join("baseline.json"), &json) {
            eprintln!("Couldn't write the baseline: {e}");
            return 1;
        }
        println!("This run is the baseline now.");
    }
    i32::from(bad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::history::HistoryEntry;

    fn entry(raw: &str, audio: Option<&str>, status: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            ts: 1,
            raw: raw.into(),
            text: raw.into(),
            said: None,
            model: "parakeet-tdt-0.6b-v2".into(),
            app: "notepad.exe".into(),
            audio: audio.map(str::to_string),
            status: status.map(str::to_string),
            error: None,
        }
    }

    #[test]
    fn a_case_needs_its_audio_and_its_words() {
        assert_eq!(usable(&entry("hello there", Some("1793212393453"), None)), Some("1793212393453"));
        assert_eq!(usable(&entry("hello there", Some("1793212393453"), Some(crate::dictation::history::NOT_PASTED))), Some("1793212393453"));
        assert_eq!(usable(&entry("hello there", None, None)), None);
        assert_eq!(usable(&entry("", Some("1793212393453"), None)), None);
        assert_eq!(usable(&entry("", Some("1793212393453"), Some(crate::dictation::history::CANCELLED))), None);
        // An id that isn't one of Yap's never names a file.
        assert_eq!(usable(&entry("hello", Some("../config"), None)), None);
    }

    #[test]
    fn a_long_line_is_cut() {
        assert_eq!(short("one  two\nthree", 20), "one two three");
        assert_eq!(short("abcdef", 3), "abc…");
    }
}
