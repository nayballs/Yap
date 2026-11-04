//! A small FLAC encoder for the meeting audio Yap keeps on disk
//! (`meeting_audio.rs`), and the reader that plays it back.
//!
//! **Why FLAC, and why this one.** A meeting is two sources (the mic and the
//! call) for up to a few hours; as 16 kHz mono 16-bit WAV that's ~115 MB an
//! hour each. FLAC is lossless — a later, better speech model hears exactly
//! what the live one did — and halves that on speech, while the silence each
//! side spends listening (the call side is digital zero whenever nobody on
//! the call talks) costs almost nothing. Yap already decodes FLAC (Symphonia's
//! default `flac` feature, used by Upload), so only the encoder is new: the
//! subset written here is fixed 4,096-sample blocks of FLAC's fixed
//! predictors (orders 0–4) with partitioned Rice coding — no LPC, no new
//! crate, no C — within a few per cent of libFLAC's fast settings on speech,
//! for a few microseconds of CPU per block.
//!
//! **Written as it's recorded.** Each 4,096-sample block (256 ms) is a frame
//! of its own, appended as it fills, so a crash leaves a file that decodes up
//! to its last whole frame. The header says "length unknown" until a clean
//! finish writes the real one ([`header`]); Symphonia reads either, and seeks
//! in both (a binary search for frame headers: no seek table needed).

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// 16 kHz: what the speech models take.
pub const RATE: u32 = 16_000;
/// Samples per frame (256 ms).
pub const BLOCK: usize = 4_096;
/// Bytes before the first frame: "fLaC" and the STREAMINFO block.
pub const HEADER_LEN: u64 = 42;

// ---- Writing ----

/// Bits, most significant first.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    acc: u64,
    /// Bits in `acc` not yet in `bytes` (always < 8 between calls).
    n: u32,
}

impl Bits {
    fn with_capacity(bytes: usize) -> Self {
        Bits { bytes: Vec::with_capacity(bytes), ..Default::default() }
    }

    /// The low `bits` (≤ 48) of `v`.
    fn put(&mut self, v: u64, bits: u32) {
        debug_assert!(bits <= 48);
        if bits == 0 {
            return;
        }
        self.acc = (self.acc << bits) | (v & ((1u64 << bits) - 1));
        self.n += bits;
        while self.n >= 8 {
            self.n -= 8;
            self.bytes.push((self.acc >> self.n) as u8);
        }
        self.acc &= (1u64 << self.n) - 1;
    }

    /// `count` zero bits.
    fn zeros(&mut self, mut count: u64) {
        while count > 0 {
            let b = count.min(32) as u32;
            self.put(0, b);
            count -= b as u64;
        }
    }

    /// Zero bits up to the next byte.
    fn align(&mut self) {
        if self.n > 0 {
            self.put(0, 8 - self.n);
        }
    }
}

/// CRC-8 (polynomial x⁸+x²+x+1), the frame header's.
fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// CRC-16 (polynomial x¹⁶+x¹⁵+x²+1), the whole frame's.
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x8005 } else { crc << 1 };
        }
    }
    crc
}

/// The stream's start: "fLaC" and its STREAMINFO, saying `total` samples
/// (0 = not known yet, while it's recorded). Always [`HEADER_LEN`] bytes.
pub fn header(total: u64) -> Vec<u8> {
    let mut w = Bits::with_capacity(HEADER_LEN as usize);
    w.bytes.extend_from_slice(b"fLaC");
    // The last metadata block: STREAMINFO, 34 bytes.
    w.bytes.extend_from_slice(&[0x80, 0, 0, 34]);
    w.put(BLOCK as u64, 16); // smallest block (the last one aside)
    w.put(BLOCK as u64, 16); // largest block
    w.put(0, 24); // smallest frame: unknown
    w.put(0, 24); // largest frame: unknown
    w.put(RATE as u64, 20);
    w.put(0, 3); // one channel
    w.put(15, 5); // 16 bits a sample
    w.put(total & ((1 << 36) - 1), 36);
    w.bytes.extend_from_slice(&[0; 16]); // no MD5
    w.bytes
}

/// FLAC's UTF-8-style coding of a frame number.
fn coded_number(w: &mut Bits, v: u64) {
    if v < 0x80 {
        w.put(v, 8);
        return;
    }
    let len: u32 = match v {
        0..0x800 => 2,
        0x800..0x1_0000 => 3,
        0x1_0000..0x20_0000 => 4,
        0x20_0000..0x400_0000 => 5,
        _ => 6,
    };
    let prefix = (0xFF00u64 >> len) & 0xFF;
    w.put(prefix | (v >> (6 * (len - 1))), 8);
    for i in (0..len - 1).rev() {
        w.put(0x80 | ((v >> (6 * i)) & 0x3F), 8);
    }
}

/// Fixed predictor `order`'s residual of `s` (from sample `order` on).
fn residual(s: &[i16], order: usize) -> Vec<i32> {
    let x = |i: usize| s[i] as i32;
    (order..s.len())
        .map(|i| match order {
            0 => x(i),
            1 => x(i) - x(i - 1),
            2 => x(i) - 2 * x(i - 1) + x(i - 2),
            3 => x(i) - 3 * x(i - 1) + 3 * x(i - 2) - x(i - 3),
            _ => x(i) - 4 * x(i - 1) + 6 * x(i - 2) - 4 * x(i - 3) + x(i - 4),
        })
        .collect()
}

fn zigzag(r: i32) -> u64 {
    if r >= 0 {
        (r as u64) << 1
    } else {
        ((-(r as i64)) as u64 * 2) - 1
    }
}

/// The best Rice parameter (0–14) for `u` and its cost in bits.
fn rice_param(u: &[u64]) -> (u32, u64) {
    let cost = |k: u32| u.iter().map(|&v| v >> k).sum::<u64>() + u.len() as u64 * (k as u64 + 1);
    if u.is_empty() {
        return (0, 0);
    }
    let mean = u.iter().sum::<u64>() / u.len() as u64;
    let guess = if mean == 0 { 0 } else { (63 - mean.leading_zeros()).min(14) };
    (guess.saturating_sub(1)..=(guess + 1).min(14))
        .map(|k| (k, cost(k)))
        .min_by_key(|&(_, c)| c)
        .unwrap_or((0, cost(0)))
}

/// How to Rice-code a residual of a block of `n` samples at `order`: the
/// partition order and each partition's parameter, and its cost in bits.
fn partitions(u: &[u64], n: usize, order: usize) -> (u32, Vec<u32>, u64) {
    let mut best: Option<(u32, Vec<u32>, u64)> = None;
    for p in 0..=8u32 {
        let parts = 1usize << p;
        if !n.is_multiple_of(parts) || n / parts <= order {
            break;
        }
        let size = n / parts;
        let mut params = Vec::with_capacity(parts);
        let mut bits = 0u64;
        let mut at = 0;
        for i in 0..parts {
            let len = if i == 0 { size - order } else { size };
            let (k, c) = rice_param(&u[at..at + len]);
            params.push(k);
            bits += 4 + c;
            at += len;
        }
        if best.as_ref().is_none_or(|b| bits < b.2) {
            best = Some((p, params, bits));
        }
    }
    best.unwrap_or((0, vec![0], u64::MAX))
}

/// One subframe for the block `s`: constant, a fixed predictor, or the
/// samples as they are — whichever is smallest.
fn subframe(w: &mut Bits, s: &[i16]) {
    if s.iter().all(|&x| x == s[0]) {
        w.put(0, 8); // CONSTANT
        w.put(s[0] as u16 as u64, 16);
        return;
    }
    let n = s.len();
    let (order, res) = (0..=4usize.min(n - 1))
        .map(|order| (order, residual(s, order)))
        .min_by_key(|(_, r)| r.iter().map(|v| v.unsigned_abs() as u64).sum::<u64>())
        .unwrap_or((0, residual(s, 0)));
    let u: Vec<u64> = res.iter().map(|&r| zigzag(r)).collect();
    let (p, params, bits) = partitions(&u, n, order);
    let fixed = 16 * order as u64 + 6 + bits;
    if fixed >= 16 * n as u64 {
        w.put(0b0000_0010, 8); // VERBATIM
        for &x in s {
            w.put(x as u16 as u64, 16);
        }
        return;
    }
    w.put((0b00_1000 | order as u64) << 1, 8); // FIXED (001xxx), this order
    for &x in &s[..order] {
        w.put(x as u16 as u64, 16);
    }
    w.put(0, 2); // Rice coding with 4-bit parameters
    w.put(p as u64, 4);
    let size = n >> p;
    let mut at = 0;
    for (i, &k) in params.iter().enumerate() {
        let len = if i == 0 { size - order } else { size };
        w.put(k as u64, 4);
        for &v in &u[at..at + len] {
            w.zeros(v >> k);
            w.put(1, 1);
            w.put(v, k);
        }
        at += len;
    }
}

/// Frame `number` of the stream: the block `s` (1 to [`BLOCK`] samples;
/// only the last frame may be shorter).
pub fn frame(s: &[i16], number: u64) -> Vec<u8> {
    debug_assert!(!s.is_empty() && s.len() <= BLOCK);
    let n = s.len();
    let mut w = Bits::with_capacity(n * 2 + 32);
    w.put(0xFFF8, 16); // sync, fixed blocks
    w.put(if n == BLOCK { 0b1100 } else { 0b0111 }, 4); // 4,096, or said below
    w.put(0b0101, 4); // 16 kHz
    w.put(0b0000, 4); // one channel
    w.put(0b100, 3); // 16 bits
    w.put(0, 1);
    coded_number(&mut w, number);
    if n != BLOCK {
        w.put(n as u64 - 1, 16);
    }
    let crc = crc8(&w.bytes);
    w.put(crc as u64, 8);
    subframe(&mut w, s);
    w.align();
    let crc = crc16(&w.bytes);
    w.put(crc as u64, 16);
    w.bytes
}

/// A sample as 16 bits (as the dictation audio's WAV has it).
pub fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32_767.0) as i16
}

/// One source's FLAC file, written as the audio comes: whole frames go to
/// the file at once, the rest waits for the next push ([`Writer::finish`]
/// writes it as a short last frame).
pub struct Writer {
    file: File,
    pending: Vec<i16>,
    frames: u64,
    samples: u64,
    bytes: u64,
}

impl Writer {
    /// A new file at `path` (never over an existing one). Other programs
    /// may read it while it's written, but not write to it or delete it:
    /// that's also how another Yap tells a recording that's going on from
    /// one a crash left behind (`meeting_audio::in_use`).
    pub fn create(path: &Path) -> std::io::Result<Writer> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(FILE_SHARE_READ);
        }
        let mut file = options.open(path)?;
        file.write_all(&header(0))?;
        Ok(Writer { file, pending: Vec::with_capacity(BLOCK), frames: 0, samples: 0, bytes: HEADER_LEN })
    }

    /// Append audio; whole frames are written now.
    pub fn push(&mut self, samples: &[f32]) -> std::io::Result<()> {
        let mut out = Vec::new();
        for &s in samples {
            self.pending.push(to_i16(s));
            if self.pending.len() == BLOCK {
                out.extend_from_slice(&frame(&self.pending, self.frames));
                self.frames += 1;
                self.samples += BLOCK as u64;
                self.pending.clear();
            }
        }
        if !out.is_empty() {
            self.file.write_all(&out)?;
            self.bytes += out.len() as u64;
        }
        Ok(())
    }

    /// Onto the disk itself (a power cut keeps it too).
    pub fn sync(&mut self) -> std::io::Result<()> {
        self.file.sync_data()
    }

    /// Samples in the file so far (whole frames).
    #[cfg(test)]
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// The end: what's left as a last short frame, the length in the
    /// header, onto the disk.
    pub fn finish(mut self) -> std::io::Result<u64> {
        if !self.pending.is_empty() {
            let last = frame(&self.pending, self.frames);
            self.file.write_all(&last)?;
            self.bytes += last.len() as u64;
            self.samples += self.pending.len() as u64;
            self.pending.clear();
        }
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(self.samples))?;
        self.file.sync_data()?;
        Ok(self.samples)
    }
}

#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x1;

/// The length a file's header gives (0: not known — it was cut off).
#[cfg(test)]
pub fn header_samples(path: &Path) -> Option<u64> {
    use std::io::Read;
    let mut head = [0u8; HEADER_LEN as usize];
    File::open(path).ok()?.read_exact(&mut head).ok()?;
    if &head[..4] != b"fLaC" {
        return None;
    }
    // The low 36 bits of STREAMINFO's bytes 13–17 (file bytes 21–25).
    let b = &head[21..26];
    Some(((b[0] as u64 & 0x0F) << 32) | (b[1] as u64) << 24 | (b[2] as u64) << 16 | (b[3] as u64) << 8 | b[4] as u64)
}

/// Write the length a cut-off file really has into its header (after a
/// crash). Nothing else in the file changes.
pub fn set_header_samples(path: &Path, total: u64) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.write_all(&header(total))?;
    file.sync_data()
}

// ---- Reading ----

/// One source's audio read back (Symphonia's FLAC decoder): from a
/// position on, as 16 kHz samples. A frame cut off by a crash ends it.
pub struct Reader {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track: u32,
    /// Decoded, not yet read; `skip` of them still to drop after a seek.
    buf: std::collections::VecDeque<f32>,
    skip: u64,
    done: bool,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Reader, String> {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        let file = File::open(path).map_err(|e| format!("Couldn't open the audio: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("flac");
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("Couldn't read the audio: {e}"))?;
        let format = probed.format;
        let track = format.tracks().first().ok_or("The audio file has no audio")?;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| format!("Couldn't decode the audio: {e}"))?;
        let track = track.id;
        Ok(Reader { format, decoder, track, buf: Default::default(), skip: 0, done: false })
    }

    /// Go to sample `pos` (the frame it's in, then the samples before it
    /// dropped). Past the end: nothing more to read.
    pub fn seek(&mut self, pos: u64) {
        use symphonia::core::formats::{SeekMode, SeekTo};
        self.buf.clear();
        self.skip = 0;
        self.done = false;
        match self.format.seek(SeekMode::Accurate, SeekTo::TimeStamp { ts: pos, track_id: self.track }) {
            Ok(to) => {
                self.decoder.reset();
                self.skip = pos.saturating_sub(to.actual_ts);
            }
            // Past the end (or a file too short to search).
            Err(_) => self.done = true,
        }
    }

    /// Decode the next frame into `buf`; false at the end.
    fn fill(&mut self) -> bool {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::errors::Error;
        while !self.done {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => {
                    // The end, or a frame a crash cut off.
                    self.done = true;
                    break;
                }
            };
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(Error::DecodeError(_)) => continue,
                Err(_) => {
                    self.done = true;
                    break;
                }
            };
            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            samples.copy_interleaved_ref(decoded);
            let mut s = samples.samples();
            if self.skip > 0 {
                let k = (self.skip as usize).min(s.len());
                s = &s[k..];
                self.skip -= k as u64;
            }
            if !s.is_empty() {
                self.buf.extend(s.iter().copied());
                return true;
            }
        }
        false
    }

    /// Up to `n` more samples; fewer (or none) at the end.
    pub fn read(&mut self, n: usize) -> Vec<f32> {
        while self.buf.len() < n && self.fill() {}
        let k = n.min(self.buf.len());
        self.buf.drain(..k).collect()
    }

    /// How many samples are left from here (decodes them all).
    pub fn count_rest(&mut self) -> u64 {
        let mut total = self.buf.len() as u64;
        self.buf.clear();
        while self.fill() {
            total += self.buf.len() as u64;
            self.buf.clear();
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-flac-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Speech-like test audio: tones that come and go, over a room's noise,
    /// with stretches of digital silence.
    fn audio(secs: f32, seed: u32) -> Vec<f32> {
        let n = (secs * RATE as f32) as usize;
        let mut x = seed | 1;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let noise = (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.002;
                let t = i as f32 / RATE as f32;
                if (t as u32) % 5 == 4 {
                    return 0.0; // a pause: digital silence
                }
                let voice = (t * 3.0).sin().abs() * 0.3;
                voice * ((t * 2.0 * std::f32::consts::PI * 180.0).sin() + 0.4 * (t * 2.0 * std::f32::consts::PI * 540.0).sin())
                    + noise
            })
            .collect()
    }

    fn write(path: &Path, samples: &[f32], pieces: usize) -> u64 {
        let mut w = Writer::create(path).unwrap();
        for part in samples.chunks(samples.len().div_ceil(pieces).max(1)) {
            w.push(part).unwrap();
        }
        w.finish().unwrap()
    }

    #[test]
    fn what_goes_in_comes_back_out() {
        let path = temp("roundtrip.flac");
        let samples = audio(9.3, 7);
        let total = write(&path, &samples, 37);
        assert_eq!(total, samples.len() as u64);
        assert_eq!(header_samples(&path), Some(samples.len() as u64));
        let back = Reader::open(&path).unwrap().read(usize::MAX >> 8);
        assert_eq!(back.len(), samples.len());
        // Lossless at 16 bits: every sample within one step.
        for (a, b) in samples.iter().zip(&back) {
            assert!((to_i16(*a) as f32 / 32_768.0 - b).abs() < 1e-6, "{a} vs {b}");
        }
        // Smaller than 16-bit WAV, the silences nearly free.
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert!(bytes < samples.len() as u64 * 2 * 7 / 10, "{bytes} bytes for {} samples", samples.len());
    }

    #[test]
    fn every_kind_of_block_decodes() {
        let path = temp("kinds.flac");
        let mut samples = vec![0.0f32; BLOCK]; // constant
        samples.extend(vec![0.25f32; BLOCK]); // constant, not zero
        // Full-scale noise: only verbatim fits.
        let mut x = 99u32;
        samples.extend((0..BLOCK).map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as f32 / u32::MAX as f32 * 2.0 - 1.0
        }));
        // Clipped extremes, then a short last block.
        samples.extend((0..BLOCK).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }));
        samples.extend(audio(0.01, 3));
        write(&path, &samples, 1);
        let back = Reader::open(&path).unwrap().read(usize::MAX >> 8);
        assert_eq!(back.len(), samples.len());
        for (i, (a, b)) in samples.iter().zip(&back).enumerate() {
            assert!((to_i16(*a) as f32 / 32_768.0 - b).abs() < 1e-6, "sample {i}: {a} vs {b}");
        }
    }

    #[test]
    fn a_file_cut_off_by_a_crash_reads_up_to_its_last_whole_frame() {
        let path = temp("crash.flac");
        let samples = audio(5.0, 11);
        // Written as it's recorded, never finished: the header says unknown.
        let mut w = Writer::create(&path).unwrap();
        w.push(&samples).unwrap();
        let whole = w.samples();
        assert_eq!(whole, (samples.len() / BLOCK * BLOCK) as u64);
        drop(w);
        assert_eq!(header_samples(&path), Some(0));
        assert_eq!(Reader::open(&path).unwrap().count_rest(), whole);
        // And half a frame more on the end (the crash mid-write): the whole
        // frames all come back.
        let clean = std::fs::read(&path).unwrap();
        let next: Vec<i16> = audio(0.3, 12)[..BLOCK].iter().map(|&s| to_i16(s)).collect();
        let tail = frame(&next, whole / BLOCK as u64);
        let mut bytes = clean.clone();
        bytes.extend_from_slice(&tail[..tail.len() / 2]);
        std::fs::write(&path, &bytes).unwrap();
        let mut r = Reader::open(&path).unwrap();
        let back = r.read(usize::MAX >> 8);
        assert_eq!(back.len() as u64, whole);
        assert!((back[1234] - to_i16(samples[1234]) as f32 / 32_768.0).abs() < 1e-6);
        // Its real length can go into the header afterwards.
        set_header_samples(&path, whole).unwrap();
        assert_eq!(header_samples(&path), Some(whole));
        assert_eq!(Reader::open(&path).unwrap().count_rest(), whole);
        // A write torn inside the next frame's header costs the frame
        // before it too (its end can't be told), and no more.
        let mut bytes = clean;
        bytes.extend_from_slice(&tail[..4]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(Reader::open(&path).unwrap().count_rest(), whole - BLOCK as u64);
    }

    #[test]
    fn seeking_lands_on_the_sample() {
        let path = temp("seek.flac");
        let samples = audio(30.0, 5);
        write(&path, &samples, 3);
        let mut r = Reader::open(&path).unwrap();
        for pos in [0u64, 1, 4_095, 4_096, 123_457, 300_000, 479_000] {
            r.seek(pos);
            let got = r.read(500);
            let want = &samples[pos as usize..(pos as usize + 500).min(samples.len())];
            assert_eq!(got.len(), want.len(), "at {pos}");
            for (a, b) in want.iter().zip(&got) {
                assert!((to_i16(*a) as f32 / 32_768.0 - b).abs() < 1e-6, "at {pos}");
            }
        }
        // Past the end: nothing.
        r.seek(10_000_000);
        assert!(r.read(10).is_empty());
    }

    #[test]
    fn frame_numbers_are_coded_as_flac_says() {
        let coded = |v: u64| {
            let mut w = Bits::default();
            coded_number(&mut w, v);
            w.bytes
        };
        assert_eq!(coded(0x7F), vec![0x7F]);
        assert_eq!(coded(0x80), vec![0xC2, 0x80]);
        assert_eq!(coded(0x7FF), vec![0xDF, 0xBF]);
        assert_eq!(coded(0x800), vec![0xE0, 0xA0, 0x80]);
        assert_eq!(coded(28_125), vec![0xE6, 0xB7, 0x9D]);
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
    }

    /// The sizes the docs give (`cargo test -- --ignored --nocapture
    /// flac_sizes`; `YAP_FLAC_SAMPLE` = a WAV of real speech to measure too).
    #[test]
    #[ignore]
    fn flac_sizes() {
        let report = |name: &str, samples: &[f32]| {
            let path = temp("size.flac");
            write(&path, samples, 100);
            let bytes = std::fs::metadata(&path).unwrap().len() as f64;
            let hours = samples.len() as f64 / RATE as f64 / 3600.0;
            println!(
                "{name}: {:.1} MB an hour ({:.0}% of 16-bit WAV)",
                bytes / hours / 1e6,
                bytes / (samples.len() as f64 * 2.0) * 100.0
            );
        };
        report("silence", &vec![0.0; RATE as usize * 60]);
        let mut x = 5u32;
        let noise: Vec<f32> = (0..RATE as usize * 60)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.001
            })
            .collect();
        report("room noise at -60 dBFS", &noise);
        report("tones over noise", &audio(60.0, 1));
        if let Ok(p) = std::env::var("YAP_FLAC_SAMPLE") {
            let speech = crate::dictation::media::decode_to_16k_mono(Path::new(&p)).unwrap();
            report("speech", &speech);
            let noisy: Vec<f32> = speech.iter().zip(noise.iter().cycle()).map(|(s, n)| s + n).collect();
            report("speech over room noise", &noisy);
        }
    }
}
