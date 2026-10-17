//! `yap.exe diarize`: the speaker-diarization child process (see `diarize.rs`
//! for why it's a process of its own, and `protocol.rs` for the wire).
//!
//! It loads NeMo-Speech.cpp's C API from the runtime folder with
//! `LoadLibraryExW(…, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
//! LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)`, so the DLL's own ggml and MSVC runtime
//! come from that folder (never System32's or another app's), opens one
//! diarization stream on the model, adds −60 dBFS dither to everything it's
//! fed (Nemotron loses track of voices across exact digital silence, which a
//! loopback track is full of), and reports segments as JSON lines. It runs at
//! below-normal priority and writes nothing but lengths and counts to stderr.

use std::ffi::OsString;
use std::io::{self, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use super::protocol::{self, Frame, Message};
use super::timeline::SpeakerOrder;

/// The rate Yap's meeting audio is on (and the model's own).
pub const SAMPLE_RATE: u32 = 16_000;

/// What `yap.exe diarize` was asked to do.
#[derive(Debug, PartialEq)]
pub struct Args {
    /// The folder holding `nemo_speech_asr_c.dll` and its DLLs.
    pub runtime: PathBuf,
    /// The model (GGUF).
    pub model: PathBuf,
    /// NeMo-Speech.cpp's streaming geometry preset ("v3-offline" = 30 s
    /// chunks, the cheap one on a CPU).
    pub preset: String,
    /// Report the segments every this many ms of audio (0: only at the end).
    pub every_ms: u64,
    pub dither: bool,
    pub seed: u64,
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut runtime = None;
    let mut model = None;
    let mut preset = "v3-offline".to_string();
    let mut every_ms = 5_000;
    let mut dither = true;
    let mut seed = 0x5eed_d17e_u64;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        let mut value = || it.next().ok_or_else(|| format!("{name} needs a value"));
        match name.as_str() {
            "--runtime" => runtime = Some(PathBuf::from(value()?)),
            "--model" => model = Some(PathBuf::from(value()?)),
            "--preset" => preset = value()?.to_string_lossy().into_owned(),
            "--every-ms" => {
                every_ms = value()?.to_string_lossy().parse().map_err(|_| "--every-ms takes a number".to_string())?
            }
            "--seed" => seed = value()?.to_string_lossy().parse().map_err(|_| "--seed takes a number".to_string())?,
            "--no-dither" => dither = false,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        runtime: runtime.ok_or("--runtime is required")?,
        model: model.ok_or("--model is required")?,
        preset,
        every_ms,
        dither,
        seed,
    })
}

/// White noise at −60 dBFS RMS (σ = 0.001): about a quiet line's noise
/// floor, inaudible, and what the spike found keeps Nemotron's speaker cache
/// working across exact digital silence (32% → 0.3% error on a 30-minute
/// call). Seeded, so a file diarizes the same way every time.
pub struct Dither {
    state: u64,
    spare: Option<f32>,
}

/// The dither's standard deviation (full scale = 1.0).
pub const DITHER_RMS: f32 = 1e-3;

impl Dither {
    pub fn new(seed: u64) -> Self {
        Dither { state: seed, spare: None }
    }

    /// splitmix64.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1].
    fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// A standard normal sample (Box–Muller, two at a time).
    fn gaussian(&mut self) -> f32 {
        if let Some(g) = self.spare.take() {
            return g;
        }
        let (u, v) = (self.uniform(), self.uniform());
        let r = (-2.0 * u.ln()).sqrt();
        let t = std::f64::consts::TAU * v;
        self.spare = Some((r * t.sin()) as f32);
        (r * t.cos()) as f32
    }

    pub fn add(&mut self, samples: &mut [f32]) {
        for s in samples {
            *s += DITHER_RMS * self.gaussian();
        }
    }
}

/// How far the labels trail the audio, sampled once per second of audio.
#[derive(Default)]
struct Lag {
    samples: Vec<u64>,
    next_at_ms: u64,
}

impl Lag {
    fn record(&mut self, pushed_ms: u64, labelled_ms: u64) {
        if pushed_ms >= self.next_at_ms {
            self.samples.push(pushed_ms.saturating_sub(labelled_ms));
            self.next_at_ms = pushed_ms + 1_000;
        }
    }

    fn median(&self) -> Option<u64> {
        let mut s = self.samples.clone();
        s.sort_unstable();
        s.get(s.len() / 2).copied()
    }

    fn max(&self) -> Option<u64> {
        self.samples.iter().max().copied()
    }
}

/// A diarization stream: NeMo-Speech.cpp's, or a fake in the tests.
pub trait Engine {
    fn push(&mut self, samples: &[f32]) -> Result<(), String>;
    fn finish(&mut self) -> Result<(), String>;
    /// How much of the audio is labelled so far.
    fn labelled_ms(&self) -> u64;
    /// Every segment so far: (start s, end s, the engine's speaker id).
    fn segments(&self) -> Result<Vec<(f64, f64, i32)>, String>;
}

/// Audio is handed to the engine in pieces of at least this much (a meeting
/// sends ~10 ms device callbacks).
const PUSH_MIN: usize = SAMPLE_RATE as usize / 4;
/// Silence frames are pushed in pieces of at most this much.
const SILENCE_PIECE: usize = SAMPLE_RATE as usize;

fn ms(samples: u64) -> u64 {
    samples * 1000 / SAMPLE_RATE as u64
}

/// What the loop keeps between frames.
struct Feeder {
    order: SpeakerOrder,
    pending: Vec<f32>,
    pushed: u64,
    compute: std::time::Duration,
    lag: Lag,
    every_ms: u64,
    reported_at: u64,
    reported_label: u64,
    dither: Option<Dither>,
}

impl Feeder {
    /// Push what's pending; report when the labels moved on and it's time.
    fn flush(&mut self, engine: &mut impl Engine, out: &mut impl Write) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if let Some(d) = self.dither.as_mut() {
            d.add(&mut self.pending);
        }
        let t = Instant::now();
        engine.push(&self.pending)?;
        self.compute += t.elapsed();
        self.pushed += self.pending.len() as u64;
        self.pending.clear();
        let audio = ms(self.pushed);
        let labelled = engine.labelled_ms().min(audio);
        self.lag.record(audio, labelled);
        if self.every_ms > 0 && labelled > self.reported_label && audio >= self.reported_at + self.every_ms {
            self.reported_at = audio;
            self.reported_label = labelled;
            let segments = self.order.convert(&engine.segments()?);
            let msg = Message::Segments {
                is_final: false,
                audio_ms: audio,
                labelled_ms: labelled,
                compute_ms: self.compute.as_millis() as u64,
                lag_ms_median: None,
                lag_ms_max: None,
                segments,
            };
            protocol::write_message(out, &msg).map_err(|e| format!("stdout: {e}"))?;
        }
        Ok(())
    }
}

/// The child's loop: read frames until stdin closes, feed the engine,
/// report. Returns an error for a garbled stream or a failing engine (the
/// caller reports it as an `error` message).
pub fn serve(
    engine: &mut impl Engine,
    input: impl Read,
    out: &mut impl Write,
    every_ms: u64,
    dither: Option<Dither>,
) -> Result<(), String> {
    let mut input = BufReader::with_capacity(1 << 16, input);
    let mut f = Feeder {
        order: SpeakerOrder::default(),
        pending: Vec::with_capacity(PUSH_MIN * 2),
        pushed: 0,
        compute: std::time::Duration::ZERO,
        lag: Lag::default(),
        every_ms,
        reported_at: 0,
        reported_label: 0,
        dither,
    };
    loop {
        match protocol::read_frame(&mut input).map_err(|e| format!("stdin: {e}"))? {
            Some(Frame::Audio(samples)) => {
                f.pending.extend_from_slice(&samples);
                if f.pending.len() >= PUSH_MIN {
                    f.flush(engine, out)?;
                }
            }
            Some(Frame::Silence(n)) => {
                let mut left = n as usize;
                while left > 0 {
                    let take = left.min(SILENCE_PIECE);
                    f.pending.resize(f.pending.len() + take, 0.0);
                    left -= take;
                    if f.pending.len() >= PUSH_MIN {
                        f.flush(engine, out)?;
                    }
                }
            }
            None => break,
        }
    }
    f.flush(engine, out)?;

    let t = Instant::now();
    engine.finish()?;
    f.compute += t.elapsed();
    let segments = f.order.convert(&engine.segments()?);
    let msg = Message::Segments {
        is_final: true,
        audio_ms: ms(f.pushed),
        labelled_ms: ms(f.pushed),
        compute_ms: f.compute.as_millis() as u64,
        lag_ms_median: f.lag.median(),
        lag_ms_max: f.lag.max(),
        segments,
    };
    protocol::write_message(out, &msg).map_err(|e| format!("stdout: {e}"))
}

/// `yap.exe diarize …` (main.rs → `diarize::run_child`). Returns the exit
/// code: 0 done, 1 the engine or the stream failed, 2 bad arguments.
pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let fail = |out: &mut dyn Write, message: String| {
        eprintln!("diarize: {message}");
        let _ = protocol::write_message(out, &Message::Error { message });
    };
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => {
            fail(&mut out, e);
            return 2;
        }
    };
    below_normal_priority();

    let started = Instant::now();
    let opened = nemo::open(&args);
    let (mut engine, info) = match opened {
        Ok(e) => e,
        Err(e) => {
            fail(&mut out, e);
            return 1;
        }
    };
    let ready = Message::Ready {
        protocol: protocol::PROTOCOL,
        speakers: info.speakers,
        frame_ms: info.frame_ms,
        runtime: info.version,
        load_ms: started.elapsed().as_millis() as u64,
    };
    if protocol::write_message(&mut out, &ready).is_err() {
        return 1; // nobody's listening
    }
    eprintln!("diarize: ready ({} speaker slots, preset {})", info.speakers, args.preset);

    let dither = args.dither.then(|| Dither::new(args.seed));
    let stdin = io::stdin();
    match serve(&mut engine, stdin.lock(), &mut out, args.every_ms, dither) {
        Ok(()) => 0,
        Err(e) => {
            fail(&mut out, e);
            1
        }
    }
}

/// Below-normal priority: Yap's dictation and the meeting's own
/// transcription come first. (The parent also asks for it when it spawns.)
fn below_normal_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS};
        let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
    }
}

/// What the loaded model says about itself.
pub struct Info {
    pub speakers: u32,
    pub frame_ms: f64,
    pub version: String,
}

#[cfg(windows)]
mod nemo {
    //! NeMo-Speech.cpp's diarization C ABI (`nemo_speech/diar.h`, v0.2.0),
    //! loaded at run time.

    use std::ffi::{c_char, c_void, CStr, CString};
    use std::mem::ManuallyDrop;

    use libloading::os::windows::{Library, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};

    use super::{Args, Engine, Info, SAMPLE_RATE};

    /// `nemo_speech_asr_status`: 0 = OK.
    type Status = i32;
    type Handle = *mut c_void;

    /// `nemo_speech_diar_model_config` (size-prefixed, append-only).
    #[repr(C)]
    struct ModelConfig {
        size: usize,
        model_path: *const c_char,
        gpu: i32,
        preset: *const c_char,
        chunk_frames: i32,
        right_context_frames: i32,
        left_context_frames: i32,
        fifo_frames: i32,
        spkcache_frames: i32,
        update_period_frames: i32,
    }

    /// `nemo_speech_diar_segment`.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CSegment {
        start_time: f64,
        end_time: f64,
        speaker: i32,
    }

    struct Fns {
        create: unsafe extern "C" fn(*const ModelConfig, *mut Handle) -> Status,
        destroy: unsafe extern "C" fn(Handle),
        num_speakers: unsafe extern "C" fn(Handle) -> i32,
        seconds_per_frame: unsafe extern "C" fn(Handle) -> f64,
        stream_open: unsafe extern "C" fn(Handle, *mut Handle) -> Status,
        push_f32: unsafe extern "C" fn(Handle, *const f32, usize, i32) -> Status,
        finish: unsafe extern "C" fn(Handle) -> Status,
        stream_close: unsafe extern "C" fn(Handle),
        frame_count: unsafe extern "C" fn(Handle) -> i64,
        segments: unsafe extern "C" fn(Handle, *const c_void, *mut CSegment, usize, *mut usize) -> Status,
        last_error: unsafe extern "C" fn() -> *const c_char,
        version: unsafe extern "C" fn() -> *const c_char,
    }

    pub struct Nemo {
        // Never unloaded: the process exits right after, and unloading ggml
        // with its OpenMP threads around buys nothing but risk.
        _lib: ManuallyDrop<Library>,
        fns: Fns,
        model: Handle,
        stream: Handle,
        seconds_per_frame: f64,
    }

    impl Nemo {
        fn error(&self, what: &str, status: Status) -> String {
            format!("{what} failed ({status}): {}", unsafe { text((self.fns.last_error)()) })
        }
    }

    unsafe fn text(p: *const c_char) -> String {
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    /// Load the runtime, the model and open a stream.
    pub fn open(args: &Args) -> Result<(Nemo, Info), String> {
        let dll = args.runtime.join("nemo_speech_asr_c.dll");
        if !dll.is_file() {
            return Err(format!("the diarization runtime isn't installed ({} missing)", dll.display()));
        }
        // The model path goes over the C ABI as a narrow string, which a
        // C++ library reads in the ANSI code page: run in the model's folder
        // and pass its (ASCII) file name, so a profile folder with any name
        // works.
        let (dir, file) = match (args.model.parent(), args.model.file_name()) {
            (Some(dir), Some(file)) => (dir, file),
            _ => return Err(format!("bad model path {}", args.model.display())),
        };
        if !args.model.is_file() {
            return Err("the diarization model isn't installed".to_string());
        }
        std::env::set_current_dir(dir).map_err(|e| format!("model folder: {e}"))?;
        let file = file.to_str().and_then(|f| CString::new(f).ok()).ok_or("model file name isn't plain text")?;
        let preset = CString::new(args.preset.as_str()).map_err(|_| "bad preset")?;

        let lib = unsafe { Library::load_with_flags(&dll, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }
            .map_err(|e| format!("couldn't load {}: {e}", dll.display()))?;
        let fns = unsafe { load_fns(&lib) }?;

        let cfg = ModelConfig {
            size: std::mem::size_of::<ModelConfig>(),
            model_path: file.as_ptr(),
            gpu: -1,
            preset: preset.as_ptr(),
            chunk_frames: 0,
            right_context_frames: 0,
            left_context_frames: -1,
            fifo_frames: 0,
            spkcache_frames: 0,
            update_period_frames: 0,
        };
        let mut model: Handle = std::ptr::null_mut();
        let status = unsafe { (fns.create)(&cfg, &mut model) };
        if status != 0 || model.is_null() {
            return Err(format!("loading the model failed ({status}): {}", unsafe { text((fns.last_error)()) }));
        }
        let mut stream: Handle = std::ptr::null_mut();
        let status = unsafe { (fns.stream_open)(model, &mut stream) };
        if status != 0 || stream.is_null() {
            let e = format!("opening a stream failed ({status}): {}", unsafe { text((fns.last_error)()) });
            unsafe { (fns.destroy)(model) };
            return Err(e);
        }
        let seconds_per_frame = unsafe { (fns.seconds_per_frame)(model) };
        let info = Info {
            speakers: unsafe { (fns.num_speakers)(model) }.max(0) as u32,
            frame_ms: seconds_per_frame * 1000.0,
            version: unsafe { text((fns.version)()) },
        };
        let nemo = Nemo {
            _lib: ManuallyDrop::new(lib),
            fns,
            model,
            stream,
            seconds_per_frame,
        };
        Ok((nemo, info))
    }

    unsafe fn load_fns(lib: &Library) -> Result<Fns, String> {
        macro_rules! sym {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes())
                    .map_err(|e| format!("the runtime has no {}: {e}", $name))?
            };
        }
        Ok(Fns {
            create: sym!("nemo_speech_diar_create"),
            destroy: sym!("nemo_speech_diar_destroy"),
            num_speakers: sym!("nemo_speech_diar_num_speakers"),
            seconds_per_frame: sym!("nemo_speech_diar_seconds_per_frame"),
            stream_open: sym!("nemo_speech_diar_stream_open"),
            push_f32: sym!("nemo_speech_diar_stream_push_f32"),
            finish: sym!("nemo_speech_diar_stream_finish"),
            stream_close: sym!("nemo_speech_diar_stream_close"),
            frame_count: sym!("nemo_speech_diar_frame_count"),
            segments: sym!("nemo_speech_diar_segments"),
            last_error: sym!("nemo_speech_asr_last_error"),
            version: sym!("nemo_speech_asr_version"),
        })
    }

    impl Engine for Nemo {
        fn push(&mut self, samples: &[f32]) -> Result<(), String> {
            let status = unsafe { (self.fns.push_f32)(self.stream, samples.as_ptr(), samples.len(), SAMPLE_RATE as i32) };
            if status != 0 {
                return Err(self.error("push", status));
            }
            Ok(())
        }

        fn finish(&mut self) -> Result<(), String> {
            let status = unsafe { (self.fns.finish)(self.stream) };
            if status != 0 {
                return Err(self.error("finish", status));
            }
            Ok(())
        }

        fn labelled_ms(&self) -> u64 {
            let frames = unsafe { (self.fns.frame_count)(self.stream) }.max(0);
            (frames as f64 * self.seconds_per_frame * 1000.0) as u64
        }

        fn segments(&self) -> Result<Vec<(f64, f64, i32)>, String> {
            let mut count = 0usize;
            let status = unsafe { (self.fns.segments)(self.stream, std::ptr::null(), std::ptr::null_mut(), 0, &mut count) };
            if status != 0 {
                return Err(self.error("segments", status));
            }
            let mut buf = vec![CSegment::default(); count.max(1)];
            let status = unsafe { (self.fns.segments)(self.stream, std::ptr::null(), buf.as_mut_ptr(), buf.len(), &mut count) };
            if status != 0 {
                return Err(self.error("segments", status));
            }
            buf.truncate(count.min(buf.len()));
            Ok(buf.iter().map(|s| (s.start_time, s.end_time, s.speaker)).collect())
        }
    }

    impl Drop for Nemo {
        fn drop(&mut self) {
            unsafe {
                (self.fns.stream_close)(self.stream);
                (self.fns.destroy)(self.model);
            }
        }
    }
}

#[cfg(not(windows))]
mod nemo {
    use super::{Args, Engine, Info};

    pub struct Nemo;
    impl Engine for Nemo {
        fn push(&mut self, _: &[f32]) -> Result<(), String> {
            unreachable!()
        }
        fn finish(&mut self) -> Result<(), String> {
            unreachable!()
        }
        fn labelled_ms(&self) -> u64 {
            0
        }
        fn segments(&self) -> Result<Vec<(f64, f64, i32)>, String> {
            Ok(Vec::new())
        }
    }
    pub fn open(_: &Args) -> Result<(Nemo, Info), String> {
        Err("speaker diarization runs on Windows only".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diarize::protocol::{write_audio, write_silence, RawSegment};

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn arguments() {
        let a = parse_args(os(&["--runtime", "C:\\rt", "--model", "C:\\m\\x.gguf"])).unwrap();
        assert_eq!(a.runtime, PathBuf::from("C:\\rt"));
        assert_eq!(a.preset, "v3-offline");
        assert_eq!(a.every_ms, 5_000);
        assert!(a.dither);
        let b = parse_args(os(&["--model", "m", "--runtime", "r", "--every-ms", "0", "--no-dither", "--preset", "v3-streaming", "--seed", "7"])).unwrap();
        assert_eq!((b.every_ms, b.dither, b.preset.as_str(), b.seed), (0, false, "v3-streaming", 7));
        assert!(parse_args(os(&["--model", "m"])).is_err(), "runtime required");
        assert!(parse_args(os(&["--runtime", "r", "--model"])).is_err(), "value missing");
        assert!(parse_args(os(&["--runtime", "r", "--model", "m", "--loud"])).is_err());
        assert!(parse_args(os(&["--runtime", "r", "--model", "m", "--every-ms", "soon"])).is_err());
    }

    #[test]
    fn dither_is_minus_60_dbfs_white_noise() {
        let mut d = Dither::new(3);
        let mut x = vec![0.0f32; 160_000];
        d.add(&mut x);
        let n = x.len() as f64;
        let mean = x.iter().map(|&v| v as f64).sum::<f64>() / n;
        let rms = (x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / n).sqrt();
        assert!(mean.abs() < 2e-5, "mean {mean}");
        assert!((rms - 1e-3).abs() < 2e-5, "rms {rms}");
        let dbfs = 20.0 * rms.log10();
        assert!((dbfs + 60.0).abs() < 0.2, "{dbfs} dBFS");
        assert!(x.iter().all(|v| v.abs() < 0.01), "no wild samples");
        // Neighbouring samples are uncorrelated (white).
        let lag1 = x.windows(2).map(|w| (w[0] * w[1]) as f64).sum::<f64>() / n / (rms * rms);
        assert!(lag1.abs() < 0.02, "lag-1 correlation {lag1}");
        // Seeded: the same file always gets the same noise.
        let (mut a, mut b) = (vec![0.5f32; 64], vec![0.5f32; 64]);
        Dither::new(9).add(&mut a);
        Dither::new(9).add(&mut b);
        assert_eq!(a, b);
        assert!(a.iter().all(|&v| v != 0.5), "added to the audio");
    }

    /// Labels whole 2 s chunks: every chunk is one speaker, alternating
    /// 7, 3, 7, 3… (its own ids) — enough to watch the loop.
    #[derive(Default)]
    struct Fake {
        pushed: Vec<f32>,
        pushes: usize,
        finished: bool,
    }

    impl Engine for Fake {
        fn push(&mut self, samples: &[f32]) -> Result<(), String> {
            self.pushes += 1;
            self.pushed.extend_from_slice(samples);
            Ok(())
        }
        fn finish(&mut self) -> Result<(), String> {
            self.finished = true;
            Ok(())
        }
        fn labelled_ms(&self) -> u64 {
            let chunk = 2 * SAMPLE_RATE as usize;
            (self.pushed.len() / chunk * chunk) as u64 * 1000 / SAMPLE_RATE as u64
        }
        fn segments(&self) -> Result<Vec<(f64, f64, i32)>, String> {
            let upto = if self.finished { self.pushed.len() as f64 / 16_000.0 } else { self.labelled_ms() as f64 / 1000.0 };
            let mut out = Vec::new();
            let mut t = 0.0;
            let mut i = 0;
            while t < upto {
                out.push((t, (t + 2.0).min(upto), if i % 2 == 0 { 7 } else { 3 }));
                t += 2.0;
                i += 1;
            }
            Ok(out)
        }
    }

    fn messages(out: &[u8]) -> Vec<Message> {
        String::from_utf8_lossy(out).lines().filter_map(protocol::parse_line).collect()
    }

    #[test]
    fn the_loop_feeds_the_engine_and_reports_as_labels_move_on() {
        let mut wire = Vec::new();
        // 5 s of audio as 10 ms callbacks, then 1.5 s of silence.
        for _ in 0..500 {
            write_audio(&mut wire, &[0.25; 160]).unwrap();
        }
        write_silence(&mut wire, 24_000).unwrap();
        let mut engine = Fake::default();
        let mut out = Vec::new();
        serve(&mut engine, &wire[..], &mut out, 1_000, Some(Dither::new(1))).unwrap();

        // Every sample arrived, on the same clock, in pieces of ≥ 0.25 s.
        assert_eq!(engine.pushed.len(), 5 * 16_000 + 24_000);
        assert!(engine.pushes <= 30, "{} pushes", engine.pushes);
        // Dithered: the audio and the silence alike.
        assert!(engine.pushed.iter().all(|&s| s != 0.25 && s != 0.0));
        assert!(engine.finished);

        let msgs = messages(&out);
        let (live, last) = msgs.split_at(msgs.len() - 1);
        // Live reports only when the labels moved: at 2 s and 4 s and 6 s.
        let labelled: Vec<u64> = live
            .iter()
            .map(|m| match m {
                Message::Segments { is_final: false, labelled_ms, .. } => *labelled_ms,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(labelled, vec![2_000, 4_000, 6_000]);
        match &last[0] {
            Message::Segments { is_final, audio_ms, labelled_ms, segments, lag_ms_median, lag_ms_max, .. } => {
                assert!(is_final);
                assert_eq!((*audio_ms, *labelled_ms), (6_500, 6_500));
                // Engine ids 7, 3 → speakers 1, 2 by first appearance.
                assert_eq!(segments[0], RawSegment { start_ms: 0, end_ms: 2_000, speaker: 1 });
                assert_eq!(segments[1].speaker, 2);
                assert_eq!(segments.last().unwrap().end_ms, 6_500);
                assert!(lag_ms_median.is_some() && lag_ms_max.unwrap() <= 2_000);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_the_final_list_when_asked_and_a_garbled_stream_fails() {
        let mut wire = Vec::new();
        write_audio(&mut wire, &vec![0.1; 48_000]).unwrap();
        let mut out = Vec::new();
        serve(&mut Fake::default(), &wire[..], &mut out, 0, None).unwrap();
        assert_eq!(messages(&out).len(), 1, "every_ms 0: the final list only");

        wire.push(b'Z');
        wire.extend_from_slice(&[0, 0, 0, 0]);
        let mut out = Vec::new();
        assert!(serve(&mut Fake::default(), &wire[..], &mut out, 0, None).is_err());
    }

    #[test]
    fn nothing_in_is_an_empty_final_list() {
        let mut engine = Fake::default();
        let mut out = Vec::new();
        serve(&mut engine, &[][..], &mut out, 1_000, None).unwrap();
        assert!(engine.finished);
        assert!(matches!(
            &messages(&out)[..],
            [Message::Segments { is_final: true, audio_ms: 0, segments, .. }] if segments.is_empty()
        ));
    }
}
