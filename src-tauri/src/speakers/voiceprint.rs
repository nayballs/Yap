//! **Voiceprints of one meeting's voices** — how alike two stretches of
//! speech sound, for the speaker engine's three blind spots: a voice heard
//! again after a pause (or an engine restart) gets a new number
//! (`labels.rs`), two people who sound alike come out as one, and one
//! person sometimes comes out as two (`voices.rs`'s flags).
//!
//! The model is 3D-Speaker's **ERes2Net** (VoxCeleb, English; 26.5 MB,
//! Apache-2.0; `diarize/registry.rs`'s voice-matching package): a clip's
//! Kaldi fbank (`fbank.rs`) in, 192 numbers out; two clips of one voice
//! point the same way (cosine near 1), two voices apart. It runs in
//! `engines` builds on Yap's own ONNX Runtime, on the processor, ~50 ms per
//! 3 s of speech. A stub build has none: everything here says "not sure",
//! and nothing is matched or flagged.
//!
//! **Nothing is kept.** A print is made from the meeting's own audio on
//! disk when it's needed (a resumed recording, Find speakers, a meeting's
//! end) and dropped after: no voiceprint is stored, and none leaves the
//! meeting it came from. (Known voices across meetings would be biometric
//! data: the spike's step 6, opt-in, not this.)
//!
//! Test mode can stage prints (`e2e_voice_prints`): the e2e suite runs a
//! stub build, so the flags and the matching are tested on prints it
//! chooses, by recording and time.

#[cfg(any(feature = "engines", test))]
mod fbank;
#[cfg(feature = "engines")]
mod eres2net;

use std::sync::Mutex;

/// A voiceprint: unit length.
pub type Print = Vec<f32>;

/// Clips shorter than this make no print (too little voice to say).
pub const MIN_CLIP_MS: u64 = 1_000;
/// A clip is cut to this for one print (longer ones are several).
pub const MAX_CLIP_MS: u64 = 10_000;

/// Cosine similarity of two prints (1: the same voice, near 0: unrelated).
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
    for (&x, &y) in a.iter().zip(b) {
        ab += x as f64 * y as f64;
        aa += x as f64 * x as f64;
        bb += y as f64 * y as f64;
    }
    if aa <= 0.0 || bb <= 0.0 {
        return 0.0;
    }
    (ab / (aa.sqrt() * bb.sqrt())) as f32
}

/// `v` scaled to unit length (`None`: all zeros).
pub fn normalized(mut v: Vec<f32>) -> Option<Print> {
    let n = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if n <= 1e-9 || !n.is_finite() {
        return None;
    }
    v.iter_mut().for_each(|x| *x = (*x as f64 / n) as f32);
    Some(v)
}

/// The average direction of `prints`, each weighted (by its seconds of
/// speech, say): one voice's print from many clips.
pub fn centroid<'a>(prints: impl IntoIterator<Item = (&'a [f32], f32)>) -> Option<Print> {
    let mut sum: Vec<f64> = Vec::new();
    for (p, w) in prints {
        if p.is_empty() || w <= 0.0 {
            continue;
        }
        if sum.is_empty() {
            sum = vec![0.0; p.len()];
        }
        if p.len() != sum.len() {
            continue;
        }
        let n = p.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt().max(1e-9);
        for (s, x) in sum.iter_mut().zip(p) {
            *s += w as f64 * *x as f64 / n;
        }
    }
    normalized(sum.into_iter().map(|x| x as f32).collect())
}

// ---- The model ----

/// The voice-matching model, loaded.
pub struct Model {
    #[cfg(feature = "engines")]
    inner: eres2net::Model,
}

impl Model {
    /// Load it (installed, and this build runs ONNX models).
    pub fn open() -> Result<Model, String> {
        let path = crate::speakers::diarize::registry::voice_matching_file().ok_or("Voice matching isn't downloaded")?;
        Self::open_file(&path)
    }

    #[cfg(feature = "engines")]
    fn open_file(path: &std::path::Path) -> Result<Model, String> {
        let threads = std::thread::available_parallelism().map(|n| (n.get() / 4).clamp(1, 2)).unwrap_or(1);
        Ok(Model { inner: eres2net::Model::load(path, threads)? })
    }

    #[cfg(not(feature = "engines"))]
    fn open_file(_: &std::path::Path) -> Result<Model, String> {
        Err("Voice matching is in the full Yap build".to_string())
    }

    /// The print of one clip (16 kHz), at most [`MAX_CLIP_MS`] of it.
    pub fn embed(&self, samples: &[f32]) -> Option<Print> {
        if (samples.len() as u64) < MIN_CLIP_MS * 16 {
            return None;
        }
        let samples = &samples[..samples.len().min(MAX_CLIP_MS as usize * 16)];
        #[cfg(feature = "engines")]
        {
            self.inner.embed(samples).and_then(normalized)
        }
        #[cfg(not(feature = "engines"))]
        {
            let _ = samples;
            None
        }
    }
}

/// Whether voices can be matched on this PC now (the model downloaded, in
/// a build that runs it; or prints staged by a test).
pub fn available() -> bool {
    staged_any() || (cfg!(feature = "engines") && crate::speakers::diarize::registry::voice_matching_file().is_some())
}

// ---- Prints of a meeting's stretches ----

/// A stretch of one side of one of a note's recordings: what a print is
/// made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// The recording (`meeting_audio::Meta::recording`).
    pub recording: u64,
    /// Its place among the note's recordings, oldest first (what a test's
    /// staged prints name).
    pub index: usize,
    /// "you" | "them".
    pub side: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Span {
    pub fn ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Makes prints of a note's stretches: from its audio on disk through the
/// model, or (test mode) from the prints a test staged.
pub struct Printer {
    note_id: u64,
    model: Option<Model>,
}

impl Printer {
    /// For note `note_id`: `None` when voices can't be matched here.
    pub fn for_note(note_id: u64) -> Option<Printer> {
        if staged_for(note_id) {
            return Some(Printer { note_id, model: None });
        }
        match Model::open() {
            Ok(model) => Some(Printer { note_id, model: Some(model) }),
            Err(e) => {
                tracing::debug!("Voice matching: not here ({e})");
                None
            }
        }
    }

    /// The print of one stretch (`None`: too short, no audio, or no voice).
    pub fn print(&self, span: &Span) -> Option<Print> {
        if span.ms() < MIN_CLIP_MS {
            return None;
        }
        match &self.model {
            Some(model) => {
                let from = span.start_ms * 16;
                let n = (span.ms().min(MAX_CLIP_MS) * 16) as usize;
                let audio = crate::meetings::meeting_audio::read_side(self.note_id, span.recording, &span.side, from, n).ok()?;
                // Silence isn't a voice (a stretch the engine placed on a
                // pause, a muted side).
                let rms = (audio.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / audio.len().max(1) as f64).sqrt();
                if rms < 1e-3 {
                    return None;
                }
                model.embed(&audio)
            }
            None => staged_print(self.note_id, span),
        }
    }

    /// One voice's print from its stretches: the longest first, up to
    /// `budget_ms` of speech, each print weighted by its length.
    pub fn voice(&self, spans: &[Span], budget_ms: u64) -> Option<Print> {
        let mut order: Vec<&Span> = spans.iter().filter(|s| s.ms() >= MIN_CLIP_MS).collect();
        order.sort_by_key(|s| std::cmp::Reverse(s.ms()));
        let mut used = 0;
        let mut prints: Vec<(Print, f32)> = Vec::new();
        for s in order {
            if used >= budget_ms {
                break;
            }
            if let Some(p) = self.print(s) {
                let ms = s.ms().min(MAX_CLIP_MS);
                used += ms;
                prints.push((p, ms as f32));
            }
        }
        centroid(prints.iter().map(|(p, w)| (p.as_slice(), *w)))
    }
}

// ---- Test mode: prints a test staged ----

/// One staged print: what a stretch of a note's recording sounds like.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub note_id: u64,
    /// The recording's place among the note's recordings (0: the first).
    #[serde(default)]
    pub recording: usize,
    #[serde(default = "them")]
    pub side: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub print: Vec<f32>,
}

fn them() -> String {
    "them".to_string()
}

static STAGED: Mutex<Vec<Staged>> = Mutex::new(Vec::new());

fn staged() -> std::sync::MutexGuard<'static, Vec<Staged>> {
    STAGED.lock().unwrap_or_else(|p| p.into_inner())
}

fn staged_any() -> bool {
    !staged().is_empty()
}

fn staged_for(note_id: u64) -> bool {
    staged().iter().any(|s| s.note_id == note_id)
}

/// The staged print overlapping `span` most (a quarter of it at least).
fn staged_print(note_id: u64, span: &Span) -> Option<Print> {
    let all = staged();
    all.iter()
        .filter(|s| s.note_id == note_id && s.recording == span.index && s.side == span.side)
        .map(|s| (s, span.end_ms.min(s.end_ms).saturating_sub(span.start_ms.max(s.start_ms))))
        .filter(|(_, overlap)| *overlap * 4 >= span.ms())
        .max_by_key(|(_, overlap)| *overlap)
        .and_then(|(s, _)| normalized(s.print.clone()))
}

/// Test mode: what stretches of a note's recordings sound like, as prints
/// (replacing any staged before for those notes).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_voice_prints(prints: Vec<Staged>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let mut all = staged();
    let notes: Vec<u64> = prints.iter().map(|p| p.note_id).collect();
    all.retain(|s| !notes.contains(&s.note_id));
    all.extend(prints);
    Ok(())
}

#[cfg(test)]
pub(crate) fn stage_for_tests(prints: Vec<Staged>) {
    let mut all = staged();
    let notes: Vec<u64> = prints.iter().map(|p| p.note_id).collect();
    all.retain(|s| !notes.contains(&s.note_id));
    all.extend(prints);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_and_centroids() {
        assert!((similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(similarity(&[1.0, 0.0], &[0.0, 3.0]).abs() < 1e-6);
        assert!((similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        assert_eq!(similarity(&[1.0], &[1.0, 2.0]), 0.0, "different models' prints never match");
        assert_eq!(similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        assert!(normalized(vec![0.0, 0.0]).is_none());
        let c = centroid([(&[1.0f32, 0.0][..], 3.0), (&[0.0f32, 1.0][..], 1.0)]).unwrap();
        assert!((c[0] - 0.948_683).abs() < 1e-4 && (c[1] - 0.316_228).abs() < 1e-4, "{c:?}");
        // Weights, not lengths: a long print counts as much as a short one.
        let c = centroid([(&[10.0f32, 0.0][..], 1.0), (&[0.0f32, 1.0][..], 1.0)]).unwrap();
        assert!((c[0] - c[1]).abs() < 1e-5);
        assert!(centroid(std::iter::empty()).is_none());
    }

    #[test]
    fn staged_prints_answer_for_the_stretch_they_overlap_most() {
        stage_for_tests(vec![
            Staged { note_id: 41, recording: 0, side: "them".into(), start_ms: 0, end_ms: 10_000, print: vec![1.0, 0.0] },
            Staged { note_id: 41, recording: 0, side: "them".into(), start_ms: 10_000, end_ms: 20_000, print: vec![0.0, 2.0] },
            Staged { note_id: 41, recording: 1, side: "them".into(), start_ms: 0, end_ms: 5_000, print: vec![0.0, 0.0, 1.0] },
        ]);
        let span = |index, start_ms, end_ms| Span { recording: 7, index, side: "them".into(), start_ms, end_ms };
        let p = Printer::for_note(41).expect("staged");
        assert_eq!(p.print(&span(0, 8_000, 14_000)), Some(vec![0.0, 1.0]), "most of it in the second");
        assert_eq!(p.print(&span(1, 1_000, 4_000)), Some(vec![0.0, 0.0, 1.0]));
        assert_eq!(p.print(&span(0, 30_000, 34_000)), None, "nothing staged there");
        assert_eq!(p.print(&span(0, 1_000, 1_500)), None, "too short to say");
        let you = Span { side: "you".into(), ..span(0, 0, 5_000) };
        assert_eq!(p.print(&you), None);
        // A voice from its stretches: the longest first.
        let v = p.voice(&[span(0, 0, 9_000), span(0, 11_000, 12_500)], 60_000).unwrap();
        assert!(v[0] > v[1] && v[1] > 0.0, "{v:?}");
    }

    /// ERes2Net here against sherpa-onnx's own embeddings of the same
    /// stretches (the spike's tool; `scratchpad`-made `ref.json`):
    /// `YAP_VOICEPRINT_REF=<ref.json> YAP_DIARIZATION_AUDIO=<audio/conv>
    /// YAP_VOICEPRINT_MODEL=<eres2net .onnx> cargo test --features engines
    /// voiceprint::tests::matches_sherpa -- --ignored --nocapture`.
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn matches_sherpa() {
        let (Some(reference), Some(audio), Some(model)) = (
            std::env::var_os("YAP_VOICEPRINT_REF"),
            std::env::var_os("YAP_DIARIZATION_AUDIO"),
            std::env::var_os("YAP_VOICEPRINT_MODEL"),
        ) else {
            eprintln!("set YAP_VOICEPRINT_REF, YAP_DIARIZATION_AUDIO and YAP_VOICEPRINT_MODEL");
            return;
        };
        let model = Model::open_file(std::path::Path::new(&model)).unwrap();
        let refs: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(reference).unwrap()).unwrap();
        let mut wavs: std::collections::HashMap<String, Vec<f32>> = Default::default();
        let mut worst = 1f32;
        for r in &refs {
            let file = r["file"].as_str().unwrap().to_string();
            let wav = wavs.entry(file.clone()).or_insert_with(|| {
                crate::dictation::media::decode_to_16k_mono(&std::path::Path::new(&audio).join(format!("{file}.wav"))).unwrap()
            });
            let (a, b) = (r["a"].as_u64().unwrap() as usize, r["b"].as_u64().unwrap() as usize);
            let theirs: Vec<f32> = r["emb"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            let t = std::time::Instant::now();
            let ours = model.inner.embed(&wav[a..b.min(wav.len())]).unwrap();
            let sim = similarity(&ours, &theirs);
            eprintln!("{file} {} {:.1}s  cos {sim:.4}  ({} ms)", r["speaker"], (b - a) as f64 / 16_000.0, t.elapsed().as_millis());
            worst = worst.min(sim);
        }
        assert!(worst > 0.98, "worst {worst}");
    }
}
