//! `yap.exe partials-dump <audio.wav>… [--out <ticks.json>] [--model <id>] [--cpu]`
//! (debug builds): the live preview replayed. Each recording is fed to the
//! speech model the way `pipeline/preview.rs` feeds a dictation — the
//! window, its advances, a tick every half second plus the time the model
//! took — and every line the Yap bar would have shown is written down,
//! with the model's raw text for each tick. `--ticks <ticks.json>` replays
//! ticks written before through today's stitching, no model needed, to
//! compare one stitching with another on the same words.
//!
//! Prints, per recording, how much the line moved: lines shown, lines
//! shorter than the one before (the bar snaps back), words already shown
//! that changed. Reads the model and the audio; writes only `--out`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::PartialSession;

/// The preview worker's timing (`preview.rs`), for a recording.
#[cfg(feature = "engines")]
mod timing {
    pub const RATE: usize = 16_000;
    /// The worker's sleep between ticks (`BASE_INTERVAL`).
    pub const BASE_MS: f64 = 500.0;
    /// Its longest sleep, once the model is slow (`MAX_INTERVAL`).
    pub const MAX_MS: f64 = 3_000.0;
    /// No tick until the window has this much audio (`STREAM_MIN_SAMPLES`).
    pub const MIN_SAMPLES: usize = 8_000;
}
#[cfg(feature = "engines")]
use timing::*;

/// One tick: what the model said for the window, and what the bar showed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    /// Seconds of audio recorded when the tick took its window.
    pub t: f64,
    /// The window's text (`Normal`), or `None` for an advance.
    #[serde(default)]
    pub text: Option<String>,
    /// An advance: the frozen part's text, the new window's, and the cut.
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub tail: Option<String>,
    #[serde(default)]
    pub cut: Option<usize>,
    /// How long the model took (ms).
    pub ms: f64,
    /// The line after this tick (filled in by a replay).
    #[serde(default)]
    pub display: String,
}

/// One recording's ticks and the final transcription (the reference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub file: String,
    pub seconds: f64,
    pub ticks: Vec<Tick>,
    #[serde(default)]
    pub final_text: String,
}

/// How much the line moved.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Churn {
    /// Lines the bar was sent.
    pub shown: usize,
    /// Lines with fewer words than the one before (the bar snaps back).
    pub shrank: usize,
    /// Words already on the bar that changed or went.
    pub words_changed: usize,
}

/// The lines a stitching makes of `ticks`, each with the line after it (pure).
pub fn replay(ticks: &[Tick]) -> Vec<String> {
    let mut s = PartialSession::default();
    ticks
        .iter()
        .map(|t| {
            match (&t.text, t.cut) {
                (Some(text), _) => s.apply_normal(text),
                (None, Some(cut)) => s.apply_advance(t.commit.as_deref().unwrap_or(""), t.tail.as_deref().unwrap_or(""), cut),
                _ => {}
            }
            s.display()
        })
        .collect()
}

/// [`Churn`] of the lines sent to the bar, as the bar compares them (word by
/// word, exactly: `bar/dictation.svelte.js` `onPartial`). Pure.
pub fn churn(lines: &[String]) -> Churn {
    let mut c = Churn::default();
    let mut shown: Vec<&str> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() || words == shown {
            continue;
        }
        c.shown += 1;
        if words.len() < shown.len() {
            c.shrank += 1;
        }
        let same = shown.iter().zip(&words).take_while(|(a, b)| a == b).count();
        c.words_changed += shown.len() - same;
        shown = words;
    }
    c
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn print_run(run: &Run, lines: &[String]) {
    let c = churn(lines);
    println!(
        "{}: {:.1} s, {} ticks, {} lines shown, {} shorter than the last, {} shown words changed",
        run.file,
        run.seconds,
        run.ticks.len(),
        c.shown,
        c.shrank,
        c.words_changed
    );
}

/// Feeds `samples` to `engine` as the preview worker would (pure timing, real model).
#[cfg(feature = "engines")]
fn record(engine: &crate::dictation::stt::SttAdapter, samples: &[f32]) -> Vec<Tick> {
    use super::TickPlan;
    use std::time::Instant;
    let mut session = PartialSession::default();
    let mut ticks = Vec::new();
    let mut at_ms = 0.0f64;
    let mut sleep_ms = BASE_MS;
    loop {
        at_ms += sleep_ms;
        let len = ((at_ms / 1000.0) * RATE as f64) as usize;
        let len = len.min(samples.len());
        if len.saturating_sub(session.window_start) >= MIN_SAMPLES {
            let win = &samples[session.window_start..len];
            let started = Instant::now();
            let mut tick = Tick { t: len as f64 / RATE as f64, text: None, commit: None, tail: None, cut: None, ms: 0.0, display: String::new() };
            match session.plan(len, &samples[..len]) {
                TickPlan::Normal => {
                    let text = engine.transcribe(win, None, false, None).unwrap_or_default();
                    session.apply_normal(&text);
                    tick.text = Some(text);
                }
                TickPlan::Advance { cut } => {
                    let c = cut - session.window_start;
                    let commit = engine.transcribe(&win[..c], None, false, None).unwrap_or_default();
                    let tail = engine.transcribe(&win[c..], None, false, None).unwrap_or_default();
                    session.apply_advance(&commit, &tail, cut);
                    tick.commit = Some(commit);
                    tick.tail = Some(tail);
                    tick.cut = Some(cut);
                }
            }
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            tick.ms = (ms * 10.0).round() / 10.0;
            tick.display = session.display();
            ticks.push(tick);
            // The model's time is audio that kept coming in.
            at_ms += ms;
            sleep_ms = if ms > 1000.0 { (ms * 2.0).min(MAX_MS) } else { BASE_MS };
        }
        if len >= samples.len() {
            return ticks;
        }
    }
}

/// The command.
pub fn main(args: Vec<String>) -> i32 {
    let out = arg_value(&args, "--out").map(PathBuf::from);
    if let Some(path) = arg_value(&args, "--ticks") {
        let runs: Vec<Run> = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string())) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Couldn't read {path}: {e}");
                return 1;
            }
        };
        let mut replayed = runs.clone();
        for run in &mut replayed {
            let lines = replay(&run.ticks);
            for (t, l) in run.ticks.iter_mut().zip(&lines) {
                t.display = l.clone();
            }
            print_run(run, &lines);
        }
        if let Some(out) = out {
            let _ = std::fs::write(out, serde_json::to_string_pretty(&replayed).unwrap_or_default());
        }
        return 0;
    }
    record_files(&args, out)
}

#[cfg(feature = "engines")]
fn record_files(args: &[String], out: Option<PathBuf>) -> i32 {
    use crate::dictation::stt::{create_stt_engine_on, Device};
    let flags = ["--out", "--model", "--ticks"];
    let files: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && flags.contains(&args[i - 1].as_str())))
        .map(|(_, a)| a)
        .collect();
    if files.is_empty() {
        eprintln!("usage: yap.exe partials-dump <audio.wav>… [--out ticks.json] [--model id] [--cpu] | --ticks ticks.json [--out replayed.json]");
        return 2;
    }
    let model = arg_value(args, "--model").unwrap_or_else(|| "parakeet-tdt-0.6b-v2".to_string());
    let device = if args.iter().any(|a| a == "--cpu") { Device::Cpu } else { Device::Gpu };
    let engine = match create_stt_engine_on(&crate::config::data_dir(), &model, device) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Couldn't load {model}: {e}");
            return 1;
        }
    };
    // Warm it, as a dictation's engine is.
    let _ = engine.transcribe(&vec![0.0; RATE], None, false, None);
    let mut runs = Vec::new();
    for file in files {
        let samples = match std::fs::read(file)
            .map_err(|e| e.to_string())
            .and_then(|b| crate::voice::tts::parse_wav(&b))
        {
            Ok(pcm) => crate::dictation::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, RATE as u32),
            Err(e) => {
                eprintln!("Couldn't read {file}: {e}");
                continue;
            }
        };
        let ticks = record(&engine, &samples);
        let final_text = engine.transcribe(&samples, None, false, None).unwrap_or_default();
        let name = std::path::Path::new(file).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let run = Run { file: name, seconds: samples.len() as f64 / RATE as f64, ticks, final_text };
        let lines: Vec<String> = run.ticks.iter().map(|t| t.display.clone()).collect();
        print_run(&run, &lines);
        runs.push(run);
    }
    if let Some(out) = out {
        if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&runs).unwrap_or_default()) {
            eprintln!("Couldn't write {}: {e}", out.display());
            return 1;
        }
    }
    0
}

#[cfg(not(feature = "engines"))]
fn record_files(_args: &[String], _out: Option<PathBuf>) -> i32 {
    eprintln!("partials-dump needs the speech engines: build with --features engines (or use --ticks)");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal(t: f64, text: &str) -> Tick {
        Tick { t, text: Some(text.into()), commit: None, tail: None, cut: None, ms: 100.0, display: String::new() }
    }

    #[test]
    fn churn_counts_what_the_bar_sees_move() {
        let lines: Vec<String> = ["the plan", "the plan is ready", "the plan was", "the plan was ready now"].iter().map(|s| s.to_string()).collect();
        // "is ready" → "was": one line shorter, two shown words changed.
        assert_eq!(churn(&lines), Churn { shown: 4, shrank: 1, words_changed: 2 });
    }

    #[test]
    fn a_replay_runs_the_ticks_through_the_stitching() {
        let ticks = vec![normal(1.0, "the plan"), normal(1.6, "the plan is"), normal(2.2, "the plan is ready")];
        assert_eq!(replay(&ticks).last().map(String::as_str), Some("the plan is ready"));
    }
}
