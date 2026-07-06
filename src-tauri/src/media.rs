//! Audio-file decode front-end for Upload / file transcription.
//!
//! Yap's STT engines (`transcribe-rs`) only accept **16 kHz mono f32** — they
//! never see compressed audio (the mic path decodes in the cpal callback). This
//! module bridges the gap for *files*: decode any supported container/codec via
//! pure-Rust **Symphonia**, downmix to mono, resample to 16 kHz, and hand the
//! samples to the warm engine (`pipeline::run_file_transcription`).
//!
//! Supported: mp3, wav, m4a/aac, flac, ogg/vorbis (Symphonia features in
//! Cargo.toml). Not supported (yet): opus in webm/ogg — Symphonia has no opus
//! decoder; surfaces a clear error instead.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The engine sample rate (same constant the mic path targets).
const TARGET_RATE: u32 = 16_000;

/// Decode `path` to 16 kHz mono f32 samples. Blocking (call from
/// `spawn_blocking`); memory-bounded by the decoded length (~230 MB/hour).
pub fn decode_to_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Can't open file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Unrecognized or unsupported audio format: {e}"))?;
    let mut format = probed.format;

    // First decodable audio track.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.sample_rate.is_some() || t.codec_params.channels.is_some())
        .or_else(|| format.tracks().first())
        .ok_or("No audio track in file")?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| {
            format!("Unsupported audio codec (opus isn't supported yet): {e}")
        })?;

    // Decode all packets, downmixing to mono as we go.
    let mut mono: Vec<f32> = Vec::new();
    let mut src_rate: u32 = track.codec_params.sample_rate.unwrap_or(0);
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(format!("Error reading audio: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A corrupt frame shouldn't kill the whole file.
            Err(SymError::DecodeError(_)) => continue,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("Decode error: {e}")),
        };

        let spec = *decoded.spec();
        if src_rate == 0 {
            src_rate = spec.rate;
        }
        let channels = spec.channels.count().max(1);

        let buf = sample_buf.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(decoded.capacity() as u64, spec)
        });
        // Frame counts vary per packet; recreate the buffer if a packet is
        // bigger than what we allocated for.
        if buf.capacity() < decoded.frames() * channels {
            *buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        buf.copy_interleaved_ref(decoded);

        let samples = buf.samples();
        if channels == 1 {
            mono.extend_from_slice(samples);
        } else {
            mono.extend(
                samples
                    .chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32),
            );
        }
    }

    if mono.is_empty() {
        return Err("No audio could be decoded from this file".to_string());
    }
    if src_rate == 0 {
        return Err("Unknown sample rate".to_string());
    }

    Ok(crate::pipeline::resample_linear(&mono, src_rate, TARGET_RATE))
}

/// Split long audio into ~`chunk_secs` windows for per-chunk progress, cutting
/// each boundary at the **lowest-energy sample** within the final `slack_secs`
/// of the window (poor-man's silence split — avoids slicing through a word).
/// Returns index ranges into the sample buffer.
pub fn chunk_ranges(
    len: usize,
    sample_rate: usize,
    chunk_secs: usize,
    slack_secs: usize,
    samples: &[f32],
) -> Vec<(usize, usize)> {
    let chunk = chunk_secs * sample_rate;
    let slack = slack_secs * sample_rate;
    if len <= chunk || chunk == 0 {
        return vec![(0, len)];
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while len - start > chunk {
        let hard_end = start + chunk;
        // Search the last `slack` samples of the window for the quietest point.
        let search_from = hard_end.saturating_sub(slack).max(start + 1);
        let mut best = hard_end;
        let mut best_amp = f32::MAX;
        let mut i = search_from;
        while i < hard_end {
            let amp = samples[i].abs();
            if amp < best_amp {
                best_amp = amp;
                best = i;
            }
            // Sampling every ~10 ms is plenty for a cut point.
            i += sample_rate / 100;
        }
        ranges.push((start, best));
        start = best;
    }
    ranges.push((start, len));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_audio_is_one_chunk() {
        let samples = vec![0.5f32; 16_000 * 10]; // 10 s
        let r = chunk_ranges(samples.len(), 16_000, 60, 5, &samples);
        assert_eq!(r, vec![(0, samples.len())]);
    }

    #[test]
    fn long_audio_chunks_cover_everything_in_order() {
        let n = 16_000 * 150; // 2.5 min → 3 chunks at 60 s
        let mut samples = vec![0.5f32; n];
        // Quiet dip near 55 s so the first boundary snaps to it.
        let dip = 16_000 * 55;
        samples[dip] = 0.0;
        let r = chunk_ranges(n, 16_000, 60, 5, &samples);
        assert!(r.len() >= 2);
        // Full coverage, no gaps or overlap.
        assert_eq!(r[0].0, 0);
        assert_eq!(r.last().unwrap().1, n);
        for w in r.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
        // The first cut used the quiet dip.
        assert_eq!(r[0].1, dip);
    }
}
