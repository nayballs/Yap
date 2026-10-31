//! What every cloud voice's request shares (ElevenLabs, OpenAI, Gemini,
//! Groq): the key (never shown), what the service says back made fit to
//! show, audio decoded as it arrives ([`Decode`]: raw 16-bit PCM, or a WAV
//! as its bytes come), a stop that ends a sentence still being fetched, and
//! the loop that hands a sentence's audio to the speaker as it comes
//! ([`pump`]: ≥ 100 ms at a time, then a short pause; timed out when no
//! audio comes in time).

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use super::Failure;
use crate::voice::tts::Pcm;

/// Connecting (DNS, TCP, TLS) may take this long.
pub const CONNECT: Duration = Duration::from_secs(4);

/// Requests with a key: no redirects (a key header would follow one to
/// another host).
pub static API_HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(CONNECT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

// ---- the key -------------------------------------------------------------------------------

/// An API key: shows as `[redacted]` in any `Debug` output.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Key(String);

impl Key {
    pub fn new(key: &str) -> Key {
        Key(key.trim().to_string())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// For "the same key" checks ([`super::Health`]), never the key itself.
    pub fn fingerprint(&self) -> u64 {
        super::fingerprint(&["key", &self.0])
    }
    /// The key itself, for its header (and nowhere else).
    pub(crate) fn header(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "[none]" } else { "[redacted]" })
    }
}

/// What the service said, fit to show: the key cut out (should it ever echo
/// it), one line, ≤ 120 characters.
pub fn tidy(said: &str, key: &Key) -> String {
    let mut s = said.to_string();
    if !key.is_empty() {
        s = s.replace(key.header(), "[key]");
    }
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 120 {
        format!("{}…", s.chars().take(119).collect::<String>().trim_end())
    } else {
        s
    }
}

// ---- the audio -----------------------------------------------------------------------------

/// Raw 16-bit little-endian PCM as it arrives: a byte left over at the end
/// of one piece is the first half of the next sample.
#[derive(Debug, Default)]
pub struct S16Le {
    carry: Option<u8>,
}

impl S16Le {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<f32> {
        let mut out = Vec::with_capacity(bytes.len() / 2 + 1);
        let mut rest = bytes;
        if let Some(lo) = self.carry.take() {
            match rest.split_first() {
                Some((&hi, tail)) => {
                    out.push(f32::from(i16::from_le_bytes([lo, hi])) / 32_768.0);
                    rest = tail;
                }
                None => {
                    self.carry = Some(lo);
                    return out;
                }
            }
        }
        let (pairs, tail) = rest.as_chunks::<2>();
        out.extend(pairs.iter().map(|&p| f32::from(i16::from_le_bytes(p)) / 32_768.0));
        self.carry = tail.first().copied();
        out
    }
}

/// A speech answer's body, turned into audio as it arrives.
pub trait Decode: Send {
    /// More of the body: the audio it completes (interleaved samples).
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure>;
    /// The body ended: what's left.
    fn finish(&mut self) -> Result<Vec<f32>, Failure> {
        Ok(Vec::new())
    }
    /// The audio's rate and channels (a WAV's once its header is in).
    fn format(&self) -> (u32, u16);
    /// The tokens the service said the request took (prompt, answer), when
    /// it says (OpenAI's and Gemini's streams end with them).
    fn usage(&self) -> Option<(u64, u64)> {
        None
    }
}

/// Raw 16-bit mono PCM at `rate` (ElevenLabs' `pcm_24000`, OpenAI's `pcm`).
pub struct RawPcm {
    rate: u32,
    pcm: S16Le,
}

impl RawPcm {
    pub fn new(rate: u32) -> RawPcm {
        RawPcm { rate, pcm: S16Le::default() }
    }
}

impl Decode for RawPcm {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure> {
        Ok(self.pcm.push(bytes))
    }
    fn format(&self) -> (u32, u16) {
        (self.rate, 1)
    }
}

/// A WAV file as its bytes arrive (Groq's answer), or raw 16-bit PCM when it
/// doesn't start with a WAV header (Gemini's stream, or a service that left
/// the header off): the header read once it's all in, then the samples
/// handed on as they come. A `data` chunk of unknown length (0 or
/// 0xFFFFFFFF, as streamed WAVs say) runs to the end. 16-bit PCM only.
pub struct WavOrRaw {
    /// Bytes not yet past the header.
    head: Vec<u8>,
    /// Past the header (or there's none): samples now.
    in_data: bool,
    /// Bytes of the data chunk left (`None`: to the end).
    left: Option<usize>,
    rate: u32,
    channels: u16,
    pcm: S16Le,
    odd: &'static str,
}

impl WavOrRaw {
    /// `rate` is the raw audio's when there's no header (24 kHz mono for
    /// Gemini); `odd` is what to say when it's not audio Yap can play.
    pub fn new(rate: u32, odd: &'static str) -> WavOrRaw {
        WavOrRaw { head: Vec::new(), in_data: false, left: None, rate, channels: 1, pcm: S16Le::default(), odd }
    }

    fn samples(&mut self, bytes: &[u8]) -> Vec<f32> {
        let take = match self.left {
            Some(n) => {
                let k = n.min(bytes.len());
                self.left = Some(n - k);
                k
            }
            None => bytes.len(),
        };
        self.pcm.push(&bytes[..take])
    }

    /// The header so far: `Ok(Some(data start))` once it's all in,
    /// `Ok(None)` while more is needed.
    fn header(&mut self) -> Result<Option<usize>, Failure> {
        let b = &self.head;
        let bad = || Failure::Rejected(self.odd.to_string());
        if b.len() < 12 {
            return Ok(None);
        }
        if &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
            return Err(bad());
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut at = 12;
        let mut fmt = false;
        loop {
            if at + 8 > b.len() {
                return Ok(None);
            }
            let id = &b[at..at + 4];
            let len = u32_at(at + 4) as usize;
            let body = at + 8;
            if id == b"data" {
                if !fmt {
                    return Err(bad());
                }
                self.left = (len != 0 && len != u32::MAX as usize).then_some(len);
                return Ok(Some(body));
            }
            if body + len > b.len() {
                return Ok(None);
            }
            if id == b"fmt " {
                if len < 16 {
                    return Err(bad());
                }
                let mut format = u16_at(body);
                if format == 0xFFFE && len >= 26 {
                    format = u16_at(body + 24);
                }
                let (channels, rate, bits) = (u16_at(body + 2), u32_at(body + 4), u16_at(body + 14));
                if format != 1 || bits != 16 || channels == 0 || rate == 0 {
                    return Err(Failure::Rejected(format!("{} ({format}, {bits}-bit)", self.odd)));
                }
                self.channels = channels;
                self.rate = rate;
                fmt = true;
            }
            at = body + len + (len & 1);
        }
    }
}

impl Decode for WavOrRaw {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure> {
        if self.in_data {
            return Ok(self.samples(bytes));
        }
        self.head.extend_from_slice(bytes);
        if self.head.len() < 4 {
            return Ok(Vec::new());
        }
        if &self.head[0..4] != b"RIFF" {
            // No header: raw PCM at the rate given.
            self.in_data = true;
            let all = std::mem::take(&mut self.head);
            return Ok(self.samples(&all));
        }
        match self.header()? {
            None => Ok(Vec::new()),
            Some(start) => {
                self.in_data = true;
                let rest = self.head.split_off(start);
                self.head.clear();
                Ok(self.samples(&rest))
            }
        }
    }
    fn finish(&mut self) -> Result<Vec<f32>, Failure> {
        if !self.in_data && !self.head.is_empty() {
            if self.head.len() < 4 || &self.head[0..4] != b"RIFF" {
                let all = std::mem::take(&mut self.head);
                return Ok(self.pcm.push(&all));
            }
            return Err(Failure::Rejected(self.odd.to_string()));
        }
        Ok(Vec::new())
    }
    fn format(&self) -> (u32, u16) {
        (self.rate, self.channels)
    }
}

/// Server-sent events as they arrive: each `data:` line's text, a line
/// split across pieces kept (OpenAI's and Gemini's speech streams).
#[derive(Debug, Default)]
pub struct SseLines {
    buf: Vec<u8>,
}

impl SseLines {
    /// More of the stream: the `data:` payloads of every line it completes.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            if let Some(d) = Self::data(&line) {
                out.push(d);
            }
        }
        out
    }
    /// The stream ended: a last line without its line break.
    pub fn finish(&mut self) -> Vec<String> {
        let rest = std::mem::take(&mut self.buf);
        Self::data(&rest).into_iter().collect()
    }
    fn data(line: &[u8]) -> Option<String> {
        let line = String::from_utf8_lossy(line.trim_ascii()).into_owned();
        let d = line.strip_prefix("data:")?.trim();
        (!d.is_empty()).then(|| d.to_string())
    }
}

// ---- stopping a sentence being made ----------------------------------------------------------

static CANCELS: AtomicU64 = AtomicU64::new(0);
static CANCEL: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// End any cloud sentence being fetched now (a stop, a new answer, a
/// dictation).
pub fn cancel() {
    CANCELS.fetch_add(1, Ordering::SeqCst);
    CANCEL.notify_waiters();
}

/// The cancel count now, for [`cancelled`].
pub fn cancels() -> u64 {
    CANCELS.load(Ordering::SeqCst)
}

/// Resolves when [`cancel`] is called after `since` (the count then).
pub async fn cancelled(since: u64) {
    loop {
        // Made before the check, so a cancel in between still wakes it.
        let woken = CANCEL.notified();
        if CANCELS.load(Ordering::SeqCst) != since {
            return;
        }
        woken.await;
    }
}

// ---- one request's audio -----------------------------------------------------------------------

/// How a sentence (or a part of one) went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streamed {
    /// Samples handed on (the pause after included).
    pub samples: usize,
    /// From asking to the first audio handed on.
    pub first_audio: Option<Duration>,
    /// Stopped part-way (a stop, or `out` said so): not a failure.
    pub stopped: bool,
}

/// How long a service may take: to the first audio (or its error), and
/// between two pieces after that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub first_audio: Duration,
    pub gap: Duration,
}

/// A request that never got an answer.
pub fn send_failure(e: &reqwest::Error) -> Failure {
    if e.is_timeout() {
        Failure::TimedOut
    } else {
        Failure::Offline
    }
}

/// An error answer's body (bounded in time and size), for `classify`.
pub async fn error_body(resp: reqwest::Response) -> (u16, String) {
    let status = resp.status().as_u16();
    let body = tokio::time::timeout(Duration::from_secs(3), resp.bytes())
        .await
        .ok()
        .and_then(Result::ok)
        .map(|b| String::from_utf8_lossy(&b[..b.len().min(8 * 1024)]).into_owned())
        .unwrap_or_default();
    (status, body)
}

/// Send a speech request: its answer when it's a success, `None` when a
/// stop came first, else why it failed (`classify` reads an error answer).
pub async fn ask<F>(send: F, since: u64, limits: Limits, classify: impl FnOnce(u16, &str) -> Failure) -> Result<Option<reqwest::Response>, Failure>
where
    F: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    let stop = cancelled(since);
    tokio::pin!(stop);
    let resp = tokio::select! {
        _ = &mut stop => return Ok(None),
        r = tokio::time::timeout(limits.first_audio, send) => match r {
            Err(_) => return Err(Failure::TimedOut),
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(resp)) => resp,
        },
    };
    if !resp.status().is_success() {
        let (status, body) = error_body(resp).await;
        return Err(classify(status, &body));
    }
    Ok(Some(resp))
}

/// One request's turn: when it was asked and the cancel count then, its
/// limits, the quiet after its audio, and the service's name (for "sent no
/// audio").
#[derive(Debug, Clone, Copy)]
pub struct Turn<'a> {
    pub started: Instant,
    pub since: u64,
    pub limits: Limits,
    pub gap_ms: usize,
    pub name: &'a str,
}

impl<'a> Turn<'a> {
    /// Asked now (the cancel count now), with `limits`, `gap_ms` of quiet after.
    pub fn now(limits: Limits, gap_ms: usize, name: &'a str) -> Turn<'a> {
        Turn { started: Instant::now(), since: cancels(), limits, gap_ms, name }
    }
}

/// Hand a speech answer's audio to `out` as it arrives — at least 100 ms a
/// piece (the end excepted), then the turn's `gap_ms` of quiet — through
/// `decode`. No audio within the first-audio limit (from `started`), or a
/// pause over the gap: timed out. A stop ([`cancel`] since `since`), or
/// `out` saying false, ends it (not a failure).
pub async fn pump(mut resp: reqwest::Response, decode: &mut dyn Decode, turn: &Turn<'_>, out: &mut dyn FnMut(Pcm) -> bool) -> Result<Streamed, Failure> {
    let Turn { started, since, limits, gap_ms, name } = *turn;
    let stop = cancelled(since);
    tokio::pin!(stop);
    let mut done = Streamed::default();
    let mut piece: Vec<f32> = Vec::new();
    let mut heard = false;
    let mut hand_on = |samples: Vec<f32>, format: (u32, u16), done: &mut Streamed| -> bool {
        done.samples += samples.len();
        if done.first_audio.is_none() {
            done.first_audio = Some(started.elapsed());
        }
        out(Pcm { sample_rate: format.0, channels: format.1, samples })
    };
    loop {
        let wait = if heard { limits.gap } else { limits.first_audio.saturating_sub(started.elapsed()) };
        let next = tokio::select! {
            _ = &mut stop => return Ok(Streamed { stopped: true, ..done }),
            r = tokio::time::timeout(wait, resp.chunk()) => r,
        };
        match next {
            Err(_) => return Err(Failure::TimedOut),
            // The connection dropped mid-answer.
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => {
                piece.extend(decode.push(&bytes)?);
                heard |= !piece.is_empty();
                let (rate, channels) = decode.format();
                let min = (rate as usize / 10).max(1) * usize::from(channels.max(1));
                if piece.len() >= min && !hand_on(std::mem::take(&mut piece), (rate, channels), &mut done) {
                    return Ok(Streamed { stopped: true, ..done });
                }
            }
        }
    }
    piece.extend(decode.finish()?);
    if done.samples == 0 && piece.is_empty() {
        return Err(Failure::Rejected(format!("{name} sent no audio")));
    }
    let (rate, channels) = decode.format();
    piece.extend(std::iter::repeat_n(0.0, rate as usize * gap_ms / 1000 * usize::from(channels.max(1))));
    if !piece.is_empty() && !hand_on(piece, (rate, channels), &mut done) {
        done.stopped = true;
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16], data_len: Option<u32>) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVE");
        // A LIST chunk before fmt, as some writers add.
        w.extend_from_slice(b"LIST");
        w.extend_from_slice(&4u32.to_le_bytes());
        w.extend_from_slice(b"INFO");
        w.extend_from_slice(b"fmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&channels.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        w.extend_from_slice(&(channels * 2).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.unwrap_or(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    }

    #[test]
    fn a_wav_decodes_as_its_bytes_come_in_any_split() {
        let samples: Vec<i16> = (0..400).map(|i| (i * 37 % 2000 - 1000) as i16).collect();
        let want: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32_768.0).collect();
        let bytes = wav(24_000, 1, &samples, None);
        for step in [1usize, 3, 7, 44, 45, 100, 10_000] {
            let mut d = WavOrRaw::new(16_000, "odd");
            let mut got = Vec::new();
            for c in bytes.chunks(step) {
                got.extend(d.push(c).unwrap());
            }
            got.extend(d.finish().unwrap());
            assert_eq!(got, want, "split every {step}");
            assert_eq!(d.format(), (24_000, 1));
        }
        // A streamed WAV's unknown length: to the end.
        let mut d = WavOrRaw::new(16_000, "odd");
        let got = d.push(&wav(22_050, 2, &samples, Some(u32::MAX))).unwrap();
        assert_eq!((got.len(), d.format()), (400, (22_050, 2)));
        // A data chunk shorter than what follows: only its own bytes.
        let mut d = WavOrRaw::new(16_000, "odd");
        let mut bytes = wav(24_000, 1, &samples, Some(200));
        bytes.extend_from_slice(b"junkjunk");
        assert_eq!(d.push(&bytes).unwrap().len(), 100);
    }

    #[test]
    fn no_header_is_raw_pcm_and_a_bad_one_is_said() {
        let mut d = WavOrRaw::new(24_000, "odd");
        let raw: Vec<u8> = [100i16, -100, 200].iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut got = d.push(&raw[..1]).unwrap();
        got.extend(d.push(&raw[1..]).unwrap());
        got.extend(d.finish().unwrap());
        assert_eq!(got.len(), 3);
        assert_eq!(d.format(), (24_000, 1));
        // A RIFF that isn't a WAV, or one in a format Yap doesn't read.
        assert!(WavOrRaw::new(24_000, "odd").push(b"RIFF\0\0\0\0AVI LIST").is_err());
        let mut float = wav(24_000, 1, &[0, 0], None);
        float[32] = 3; // the fmt chunk's format tag → IEEE float
        assert!(WavOrRaw::new(24_000, "odd").push(&float).is_err());
        // A header cut short at the end.
        let mut d = WavOrRaw::new(24_000, "odd");
        d.push(b"RIFF\x10\0\0\0WAVEfm").unwrap();
        assert!(d.finish().is_err());
    }

    #[test]
    fn sse_lines_survive_any_split() {
        let text = "event: step.delta\ndata: {\"a\":1}\n\ndata: {\"b\":2}\r\n\nevent: done\ndata: [DONE]";
        for step in [1usize, 2, 5, 13, 1_000] {
            let mut s = SseLines::default();
            let mut got = Vec::new();
            for c in text.as_bytes().chunks(step) {
                got.extend(s.push(c));
            }
            got.extend(s.finish());
            assert_eq!(got, vec!["{\"a\":1}", "{\"b\":2}", "[DONE]"], "split every {step}");
        }
    }

    #[test]
    fn the_key_never_shows() {
        let k = Key::new("  sk-secret-0123456789 ");
        assert_eq!(format!("{k:?}"), "[redacted]");
        assert_eq!(format!("{:?}", Key::new("")), "[none]");
        assert_eq!(tidy("bad key sk-secret-0123456789 here", &k), "bad key [key] here");
        assert_ne!(k.fingerprint(), Key::new("other").fingerprint());
        assert_eq!(k.fingerprint(), Key::new("sk-secret-0123456789").fingerprint());
    }
}
