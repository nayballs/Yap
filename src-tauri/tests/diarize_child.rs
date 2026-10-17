//! `yap.exe diarize` end to end with the real engine: the binary started the
//! way Yap starts it, a WAV fed over the wire a second at a time, the final
//! segments scored against reference turns (frame-level error, 10 ms frames,
//! optimal speaker mapping, overlap scored, 0.25 s collar — the spike's
//! `der.py`).
//!
//! Needs the model and runtime on disk, so it's ignored by default:
//!
//! ```text
//! YAP_DIARIZATION_MODELS=<folder with nemotron-3-diarization/ and nemo-speech-0.2.0-cpu/>
//! YAP_DIARIZATION_AUDIO=<folder of <name>.wav + <name>.rttm>
//! YAP_DIARIZATION_FILES=s2_clean,s3_clean   (optional; the default set below)
//! cargo test --test diarize_child -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::Value;

const STEP: f64 = 0.01;

fn read_wav(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    let mut i = 12;
    let mut fmt_ok = false;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        if id == b"fmt " {
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            let bits = u16::from_le_bytes([body[14], body[15]]);
            assert_eq!((channels, rate, bits), (1, 16_000, 16), "the spike's WAVs are 16 kHz mono 16-bit");
            fmt_ok = true;
        } else if id == b"data" {
            assert!(fmt_ok);
            return body.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
        }
        i += 8 + len + (len & 1);
    }
    panic!("no data chunk in {}", path.display());
}

fn load_rttm(path: &Path) -> Vec<(f64, f64, String)> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (p.first() == Some(&"SPEAKER")).then(|| {
                let s: f64 = p[3].parse().unwrap();
                let d: f64 = p[4].parse().unwrap();
                (s, s + d, p[7].to_string())
            })
        })
        .collect()
}

/// Frame-level diarization error rate with a collar (der.py's method).
fn der(reference: &[(f64, f64, String)], hyp: &[(f64, f64, String)], collar: f64) -> f64 {
    let end = reference.iter().chain(hyp).map(|s| s.1).fold(0.0, f64::max) + 1.0;
    let n = (end / STEP) as usize + 1;
    let matrix = |segs: &[(f64, f64, String)]| {
        let mut names: Vec<String> = segs.iter().map(|s| s.2.clone()).collect();
        names.sort();
        names.dedup();
        let mut m = vec![vec![false; names.len()]; n];
        for (a, b, k) in segs {
            let col = names.iter().position(|x| x == k).unwrap();
            let (from, to) = (((a / STEP).round() as usize).min(n), ((b / STEP).round() as usize).min(n));
            for row in m.iter_mut().take(to).skip(from) {
                row[col] = true;
            }
        }
        (m, names.len())
    };
    let (r, nr) = matrix(reference);
    let (h, nh) = matrix(hyp);
    let mut scored = vec![true; n];
    let c = (collar / STEP).round() as usize;
    for (a, b, _) in reference {
        for t in [a, b] {
            let i = (t / STEP).round() as usize;
            for s in scored.iter_mut().take((i + c).min(n)).skip(i.saturating_sub(c)) {
                *s = false;
            }
        }
    }
    let frames: Vec<usize> = (0..n).filter(|&i| scored[i]).collect();
    // Co-occurrence, then the best 1:1 mapping (small: brute force).
    let mut co = vec![vec![0u64; nh]; nr];
    for &f in &frames {
        for i in 0..nr {
            for j in 0..nh {
                if r[f][i] && h[f][j] {
                    co[i][j] += 1;
                }
            }
        }
    }
    fn best(i: usize, co: &[Vec<u64>], used: &mut Vec<bool>, map: &mut Vec<Option<usize>>) -> (u64, Vec<Option<usize>>) {
        if i == co.len() {
            return (0, map.clone());
        }
        let mut top = best(i + 1, co, used, map);
        for j in 0..used.len() {
            if !used[j] && co[i][j] > 0 {
                used[j] = true;
                map[i] = Some(j);
                let (score, m) = best(i + 1, co, used, map);
                if score + co[i][j] > top.0 {
                    top = (score + co[i][j], m);
                }
                map[i] = None;
                used[j] = false;
            }
        }
        top
    }
    let (_, mapping) = best(0, &co, &mut vec![false; nh], &mut vec![None; nr]);
    let (mut total, mut miss, mut fa, mut conf) = (0u64, 0u64, 0u64, 0u64);
    for &f in &frames {
        let refs = r[f].iter().filter(|&&x| x).count() as u64;
        let hyps = h[f].iter().filter(|&&x| x).count() as u64;
        let correct = (0..nr).filter(|&i| r[f][i] && mapping[i].is_some_and(|j| h[f][j])).count() as u64;
        total += refs;
        miss += refs.saturating_sub(hyps);
        fa += hyps.saturating_sub(refs);
        conf += refs.min(hyps) - correct;
    }
    (miss + fa + conf) as f64 / total.max(1) as f64
}

#[test]
fn der_scoring_matches_by_hand() {
    let r = vec![(0.0, 10.0, "a".to_string()), (10.0, 20.0, "b".to_string())];
    // Perfect, whatever the hypothesis calls its speakers.
    let h = vec![(0.0, 10.0, "S2".to_string()), (10.0, 20.0, "S1".to_string())];
    assert!(der(&r, &h, 0.25) < 1e-9);
    // One voice for everything: half the speech is confused.
    let one = vec![(0.0, 20.0, "S1".to_string())];
    let e = der(&r, &one, 0.0);
    assert!((e - 0.5).abs() < 0.01, "{e}");
    // Nothing found: all missed.
    assert!((der(&r, &[], 0.0) - 1.0).abs() < 1e-9);
}

struct Run {
    segments: Vec<(f64, f64, String)>,
    audio_ms: u64,
    compute_ms: u64,
    lag_median: u64,
    lag_max: u64,
    load_ms: u64,
}

fn diarize(models: &Path, wav: &Path) -> Run {
    let audio = read_wav(wav);
    let mut child = Command::new(env!("CARGO_BIN_EXE_yap"))
        .arg("diarize")
        .arg("--runtime")
        .arg(models.join("nemo-speech-0.2.0-cpu"))
        .arg("--model")
        .arg(models.join("nemotron-3-diarization").join("Nemotron-3-Diarization.q8_0.gguf"))
        .arg("--every-ms")
        .arg("0")
        .env("OMP_NUM_THREADS", "4")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
            .collect::<Vec<Value>>()
    });
    {
        let mut stdin = child.stdin.take().unwrap();
        for second in audio.chunks(16_000) {
            let mut frame = vec![b'A'];
            frame.extend_from_slice(&((second.len() * 4) as u32).to_le_bytes());
            for s in second {
                frame.extend_from_slice(&s.to_le_bytes());
            }
            stdin.write_all(&frame).unwrap();
        }
    }
    let lines = reader.join().unwrap();
    assert!(child.wait().unwrap().success());
    let ready = lines.iter().find(|v| v["type"] == "ready").expect("ready");
    assert_eq!(ready["protocol"], 1);
    let fin = lines.iter().find(|v| v["type"] == "segments" && v["final"] == true).expect("final");
    let segments = fin["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["start_ms"].as_f64().unwrap() / 1000.0,
                s["end_ms"].as_f64().unwrap() / 1000.0,
                format!("S{}", s["speaker"]),
            )
        })
        .collect();
    Run {
        segments,
        audio_ms: fin["audio_ms"].as_u64().unwrap(),
        compute_ms: fin["compute_ms"].as_u64().unwrap(),
        lag_median: fin["lag_ms_median"].as_u64().unwrap_or(0),
        lag_max: fin["lag_ms_max"].as_u64().unwrap_or(0),
        load_ms: ready["load_ms"].as_u64().unwrap_or(0),
    }
}

#[test]
#[ignore]
fn the_real_engine_on_the_spikes_audio() {
    let (Some(models), Some(audio)) = (std::env::var_os("YAP_DIARIZATION_MODELS"), std::env::var_os("YAP_DIARIZATION_AUDIO")) else {
        eprintln!("set YAP_DIARIZATION_MODELS and YAP_DIARIZATION_AUDIO");
        return;
    };
    let (models, audio) = (PathBuf::from(models), PathBuf::from(audio));
    let files = std::env::var("YAP_DIARIZATION_FILES")
        .unwrap_or_else(|_| "s2_clean,s3_clean,s4_clean,s4_hard,s3_noisy,s3_dialin,s4_dialin,s2_phone,s3_phone".into());
    // The spike's §4.4 numbers for the same files (30 s preset, dithered):
    // what this run should land close to.
    let spike: HashMap<&str, f64> = [
        ("s2_clean", 0.027),
        ("s3_clean", 0.004),
        ("s4_clean", 0.144),
        ("s4_hard", 0.023),
        ("s3_noisy", 0.006),
        ("s3_dialin", 0.004),
        ("s4_dialin", 0.079),
        ("s2_phone", 0.481),
        ("s3_phone", 0.627),
        ("long30_opus", 0.003),
        ("long60_opus", 0.007),
    ]
    .into_iter()
    .collect();
    eprintln!("file          ref found   DER25  spike   audio  compute    RTF   lag med/max   load");
    for name in files.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let reference = load_rttm(&audio.join(format!("{name}.rttm")));
        let wall = Instant::now();
        let run = diarize(&models, &audio.join(format!("{name}.wav")));
        let mut found: Vec<&String> = run.segments.iter().map(|s| &s.2).collect();
        found.sort();
        found.dedup();
        let mut refs: Vec<&String> = reference.iter().map(|s| &s.2).collect();
        refs.sort();
        refs.dedup();
        let e = der(&reference, &run.segments, 0.25);
        eprintln!(
            "{name:<13} {:>3} {:>5} {:>6.1}% {:>5.1}% {:>6.0}s {:>7.1}s {:>6.3} {:>5.1}/{:<5.1}s {:>5}ms  (wall {:.1}s)",
            refs.len(),
            found.len(),
            e * 100.0,
            spike.get(name).copied().unwrap_or(f64::NAN) * 100.0,
            run.audio_ms as f64 / 1000.0,
            run.compute_ms as f64 / 1000.0,
            run.compute_ms as f64 / run.audio_ms as f64,
            run.lag_median as f64 / 1000.0,
            run.lag_max as f64 / 1000.0,
            run.load_ms,
            wall.elapsed().as_secs_f64(),
        );
        // Wideband audio: never far off. Phone-only audio is the spike's
        // known failure (the fallback's job), so it isn't held to it.
        if !name.contains("phone") {
            assert!(e < 0.25, "{name}: {e}");
        }
    }
}
