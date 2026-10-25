//! 3D-Speaker's **ERes2Net** (VoxCeleb, English; Apache-2.0), k2-fsa's ONNX
//! export, on Yap's own ONNX Runtime (`ort`, the one transcribe-rs and
//! Kokoro use) on the processor: a clip's fbank in, a 192-number voiceprint
//! out. ~50 ms for 3 s of speech on one thread (the spike's §4.5).

use std::path::Path;
use std::sync::Mutex;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

use super::fbank::{Fbank, BINS};

pub struct Model {
    session: Mutex<Session>,
    input: String,
    fbank: Fbank,
}

impl Model {
    pub fn load(path: &Path, threads: usize) -> Result<Model, String> {
        let err = |e: ort::Error<ort::session::builder::SessionBuilder>| format!("the voice-matching model didn't load: {e}");
        let session = Session::builder()
            .map_err(|e| format!("the voice-matching model didn't load: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(err)?
            .with_intra_threads(threads.max(1))
            .map_err(err)?
            .with_inter_threads(1)
            .map_err(err)?
            .with_intra_op_spinning(false)
            .map_err(err)?
            .commit_from_file(path)
            .map_err(|e| format!("the voice-matching model didn't load: {e}"))?;
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .ok_or("the voice-matching model has no input")?;
        Ok(Model { session: Mutex::new(session), input, fbank: Fbank::new() })
    }

    /// The voiceprint of `samples` (16 kHz), not normalised; `None` when
    /// the clip has no frames or the model fails.
    pub fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let feats = self.fbank.features(samples);
        if feats.is_empty() {
            return None;
        }
        let frames = feats.len();
        let flat: Vec<f32> = feats.iter().flat_map(|r| r.iter().copied()).collect();
        let x = Tensor::from_array((vec![1i64, frames as i64, BINS as i64], flat)).ok()?;
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let outputs = session.run(vec![(self.input.clone(), ort::session::SessionInputValue::from(x))]).ok()?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>().ok()?;
        (!data.is_empty() && data.iter().all(|v| v.is_finite())).then(|| data.to_vec())
    }
}
