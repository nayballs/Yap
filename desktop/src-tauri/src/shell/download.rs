//! **One download helper for every big download** (2026-10-25): the
//! on-device AI models and their engine (`ai/local_llm/models.rs`), the
//! speech models (`dictation/stt.rs`), the meeting search model
//! (`search/embedder.rs`), Kokoro and eSpeak NG (`voice/tts/registry.rs`)
//! and the speaker engine (`speakers/diarize/registry.rs`) all fetch through
//! [`fetch`]. Before, each had its own loop: some resumed, some didn't, one
//! gave up on a silent server, none could be cancelled, and none said how
//! fast it was going.
//!
//! - **Resume**: the bytes already in `<file>.partial` are asked for no
//!   more (an HTTP `Range`); a server that answers with the whole file
//!   instead starts it again. A connection that drops or a server silent
//!   for [`STALL`] leaves the partial file for the next try.
//! - **Cancel** ([`Ticket`], the `download_cancel {id}` command): the
//!   download stops at once and **its partial file goes** — a cancelled
//!   download leaves nothing half-made behind ([`CANCELLED`] is the error,
//!   which the pages don't show as a failure).
//! - **Progress** ([`Progress`]): bytes so far and in all, the speed over
//!   the last few seconds ([`Meter`]) and the time left at that speed, at
//!   most every [`REPORT_EVERY`] (and the last one always).
//! - **Room**: a download that won't fit on the disk (with
//!   [`SPARE`] to spare) is refused before it starts.
//!
//! The SHA-256 check stays each caller's (it knows the hash), and so does
//! where the file goes once whole.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::privacy::net::Recorded as _;

/// The error a cancelled download ends with.
pub const CANCELLED: &str = "Download cancelled";

/// Is this error a cancel (not a failure to show)?
pub fn is_cancelled(e: &str) -> bool {
    e.contains(CANCELLED)
}

/// A server, or a connection, silent this long: the download stops (its
/// partial file kept, to resume).
pub const STALL: Duration = Duration::from_secs(60);
/// Room left on the disk past a download's own size.
pub const SPARE: u64 = 64 * 1_048_576;
/// Progress at most this often.
pub const REPORT_EVERY: Duration = Duration::from_millis(250);
/// The speed is measured over this long.
const SPEED_WINDOW: Duration = Duration::from_secs(5);

/// How a download is going: what the pages show ("1.2 GB of 2.8 GB · 24
/// MB/s · about 1 min left").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub downloaded: u64,
    /// 0 = not known (the server didn't say).
    pub total: u64,
    /// Over the last few seconds; 0 until it's known.
    pub bytes_per_sec: u64,
    /// At that speed; `None` until it's known.
    pub eta_secs: Option<u64>,
}

impl Progress {
    /// 0–100 (0 with no total).
    pub fn percent(&self) -> u8 {
        percent(self.downloaded, self.total)
    }
}

/// `done` of `total` as a whole percent, 0–100. Pure.
pub fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((u128::from(done.min(total)) * 100) / u128::from(total)) as u8
}

/// Seconds left for `remaining` bytes at `bytes_per_sec`, rounded up;
/// `None` when the speed isn't known. Pure.
pub fn eta(remaining: u64, bytes_per_sec: u64) -> Option<u64> {
    (bytes_per_sec > 0).then(|| remaining.div_ceil(bytes_per_sec))
}

/// The speed over a sliding window: samples of (when, bytes so far).
#[derive(Debug, Clone)]
pub struct Meter {
    window: Duration,
    samples: VecDeque<(Duration, u64)>,
}

impl Meter {
    pub fn new(window: Duration) -> Meter {
        Meter { window, samples: VecDeque::new() }
    }

    /// `bytes` downloaded by `at` (since the download began); the speed
    /// over the window, once it spans at least half a second. Pure.
    pub fn add(&mut self, at: Duration, bytes: u64) -> u64 {
        self.samples.push_back((at, bytes));
        while self.samples.len() > 2 && self.samples.front().is_some_and(|(t, _)| at.saturating_sub(*t) > self.window) {
            self.samples.pop_front();
        }
        let (t0, b0) = *self.samples.front().unwrap_or(&(at, bytes));
        let span = at.saturating_sub(t0);
        if span < Duration::from_millis(500) {
            return 0;
        }
        ((bytes.saturating_sub(b0)) as f64 / span.as_secs_f64()) as u64
    }
}

/// Progress over several files (a package: Kokoro's model and voices, the
/// speaker engine and its model): one speed and one time left for the lot,
/// measured on the bytes of all of them.
#[derive(Debug)]
pub struct Overall {
    total: u64,
    started: Instant,
    meter: Mutex<Meter>,
}

impl Overall {
    pub fn new(total: u64) -> Overall {
        Overall { total, started: Instant::now(), meter: Mutex::new(Meter::new(SPEED_WINDOW)) }
    }

    /// `so_far` bytes of the lot.
    pub fn at(&self, so_far: u64) -> Progress {
        let bps = self.meter.lock().unwrap_or_else(|p| p.into_inner()).add(self.started.elapsed(), so_far);
        Progress { downloaded: so_far, total: self.total, bytes_per_sec: bps, eta_secs: eta(self.total.saturating_sub(so_far), bps) }
    }
}

// ---- cancelling -----------------------------------------------------------------------------

static TICKETS: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// A download that can be cancelled by its id ("llm:qwen3.5-4b",
/// "stt:parakeet-tdt-0.6b-v3", "search", "kokoro", "diarization:<id>"):
/// held for the download's whole run (every file of it), let go on drop.
#[derive(Debug)]
pub struct Ticket {
    id: String,
    flag: Arc<AtomicBool>,
}

impl Ticket {
    /// Start one: `None` when a download with this id is already running.
    pub fn begin(id: &str) -> Option<Ticket> {
        let mut all = TICKETS.lock().unwrap_or_else(|p| p.into_inner());
        if all.contains_key(id) {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        all.insert(id.to_string(), flag.clone());
        Some(Ticket { id: id.to_string(), flag })
    }

    /// A ticket nobody can cancel (a download no page started).
    pub fn unlisted() -> Ticket {
        Ticket { id: String::new(), flag: Arc::new(AtomicBool::new(false)) }
    }

    pub fn cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if self.id.is_empty() {
            return;
        }
        let mut all = TICKETS.lock().unwrap_or_else(|p| p.into_inner());
        if all.get(&self.id).is_some_and(|f| Arc::ptr_eq(f, &self.flag)) {
            all.remove(&self.id);
        }
    }
}

/// Cancel the download `id`: `true` when one was running.
pub fn cancel(id: &str) -> bool {
    match TICKETS.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            tracing::info!(id, "Download: cancelled");
            true
        }
        None => false,
    }
}

/// Is a download with this id running?
#[cfg(test)]
fn running(id: &str) -> bool {
    TICKETS.lock().unwrap_or_else(|p| p.into_inner()).contains_key(id)
}

/// Cancel a download (Settings' and the pickers' Cancel): its partial
/// file goes. `false`: none was running.
#[tauri::command]
pub fn download_cancel(id: String) -> bool {
    cancel(&id)
}

// ---- the download ---------------------------------------------------------------------------

/// One file to fetch.
pub struct Fetch<'a> {
    pub url: &'a str,
    /// Where the bytes go until the file is whole (and checked).
    pub partial: &'a Path,
    /// Its size when known (a pinned download): a server sending more is
    /// refused, and a partial file of this size is already whole.
    pub size: Option<u64>,
    /// The download, as the network receipts name it (`privacy/net.rs`).
    pub what: crate::privacy::net::Download,
    pub ticket: &'a Ticket,
}

/// The server's whole size from a 206's `Content-Range: bytes a-b/total`.
/// Pure.
pub fn range_total(content_range: &str) -> Option<u64> {
    content_range.rsplit('/').next().and_then(|t| t.trim().parse().ok())
}

/// Fetch `f.url` into `f.partial`, resuming what an earlier try left.
/// Returns the file's size. A cancel deletes the partial file ([`CANCELLED`]);
/// any other failure keeps it for the next try, unless the bytes can't be
/// right (more than the pinned size).
pub async fn fetch(f: Fetch<'_>, report: &(dyn Fn(Progress) + Sync)) -> Result<u64, String> {
    let result = fetch_inner(&f, report).await;
    if matches!(&result, Err(e) if is_cancelled(e)) {
        let _ = tokio::fs::remove_file(f.partial).await;
    }
    result
}

async fn fetch_inner(f: &Fetch<'_>, report: &(dyn Fn(Progress) + Sync)) -> Result<u64, String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let gone = || Err(CANCELLED.to_string());
    if f.ticket.cancelled() {
        return gone();
    }
    let mut have = tokio::fs::metadata(f.partial).await.map(|m| m.len()).unwrap_or(0);
    if let Some(size) = f.size {
        if have > size {
            let _ = tokio::fs::remove_file(f.partial).await;
            have = 0;
        }
        if have == size && size > 0 {
            report(Progress { downloaded: size, total: size, bytes_per_sec: 0, eta_secs: Some(0) });
            return Ok(size); // whole already, not checked yet
        }
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut req = client.get(f.url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        tracing::info!(have, "Download: resuming");
    }
    let resp = tokio::select! {
        r = tokio::time::timeout(STALL, req.send_for(crate::privacy::net::Purpose::Download(f.what))) => r
            .map_err(|_| "Download failed: the server didn't answer".to_string())?
            .map_err(|e| format!("Download failed: {e}"))?,
        _ = wait_cancel(f.ticket) => return gone(),
    };
    let status = resp.status();
    // A Range the server can't serve (416: the partial is stale): again from
    // the start, next time.
    if have > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let _ = tokio::fs::remove_file(f.partial).await;
        return Err("Download failed: the server couldn't resume it. Try again.".to_string());
    }
    if !status.is_success() {
        return Err(format!("Download failed: HTTP {} from {}", status.as_u16(), host(f.url)));
    }
    let resumed = have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed {
        have = 0;
    }
    let total = if resumed {
        resp.headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(range_total)
            .or_else(|| resp.content_length().map(|n| n + have))
    } else {
        resp.content_length()
    }
    .or(f.size)
    .unwrap_or(0);
    if let (Some(size), true) = (f.size, total > 0) {
        if total > size {
            return Err(format!("Download from {} is bigger than expected", host(f.url)));
        }
    }
    if total > have {
        let free = f.partial.parent().and_then(crate::ai::local_llm::runtime::free_bytes);
        if !crate::ai::local_llm::runtime::room_for(free, total - have + SPARE) {
            return Err(format!(
                "Not enough disk space: it needs {} MB, {} MB are free",
                (total - have + SPARE) / 1_048_576,
                free.unwrap_or(0) / 1_048_576
            ));
        }
    }
    if let Some(dir) = f.partial.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("Couldn't save the download: {e}"))?;
    }
    let mut file = if resumed {
        tokio::fs::OpenOptions::new().append(true).open(f.partial).await
    } else {
        tokio::fs::File::create(f.partial).await
    }
    .map_err(|e| format!("Couldn't save the download: {e}"))?;

    let started = Instant::now();
    let mut meter = Meter::new(SPEED_WINDOW);
    let mut got = have;
    let mut last_report: Option<Instant> = None;
    let progress = |got: u64, bps: u64| Progress {
        downloaded: got,
        total,
        bytes_per_sec: bps,
        eta_secs: if total > 0 { eta(total.saturating_sub(got), bps) } else { None },
    };
    meter.add(Duration::ZERO, got);
    report(progress(got, 0));
    let mut stream = resp.bytes_stream();
    loop {
        let next = tokio::select! {
            n = tokio::time::timeout(STALL, stream.next()) => n,
            _ = wait_cancel(f.ticket) => return gone(),
        };
        let chunk = match next {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("Download interrupted: {e}"))?,
            Ok(None) => break,
            Err(_) => return Err("Download interrupted: the connection stalled".to_string()),
        };
        got += chunk.len() as u64;
        if f.size.is_some_and(|size| got > size) {
            drop(file);
            let _ = tokio::fs::remove_file(f.partial).await;
            return Err(format!("Download from {} is bigger than expected", host(f.url)));
        }
        file.write_all(&chunk).await.map_err(|e| format!("Couldn't save the download: {e}"))?;
        let bps = meter.add(started.elapsed(), got - have);
        if last_report.is_none_or(|t| t.elapsed() >= REPORT_EVERY) {
            last_report = Some(Instant::now());
            report(progress(got, bps));
        }
    }
    file.flush().await.map_err(|e| format!("Couldn't save the download: {e}"))?;
    drop(file);
    if f.ticket.cancelled() {
        return gone();
    }
    report(Progress { downloaded: got, total: total.max(got), bytes_per_sec: 0, eta_secs: Some(0) });
    Ok(got)
}

/// Resolves once the ticket is cancelled (looked at every 100 ms).
async fn wait_cancel(ticket: &Ticket) {
    while !ticket.cancelled() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The host of an address, the only part of it an error shows.
fn host(url: &str) -> &str {
    url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_maths() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(50, 200), 25);
        assert_eq!(percent(199, 200), 99);
        assert_eq!(percent(300, 200), 100);
        // Big files don't overflow.
        assert_eq!(percent(6_000_000_000, 6_169_341_984), 97);
        assert_eq!(eta(1_000, 0), None);
        assert_eq!(eta(1_000, 400), Some(3)); // 2.5 s, rounded up
        assert_eq!(eta(0, 400), Some(0));
        assert_eq!(Progress { downloaded: 1, total: 4, bytes_per_sec: 0, eta_secs: None }.percent(), 25);
        assert_eq!(range_total("bytes 100-999/1000"), Some(1000));
        assert_eq!(range_total("bytes 0-9/*"), None);
    }

    #[test]
    fn the_speed_is_the_last_few_seconds() {
        let s = |ms: u64| Duration::from_millis(ms);
        let mut m = Meter::new(Duration::from_secs(5));
        assert_eq!(m.add(s(0), 0), 0);
        assert_eq!(m.add(s(200), 100_000), 0, "too soon to say");
        assert_eq!(m.add(s(1_000), 1_000_000), 1_000_000);
        // Steady at 1 MB/s for ten seconds…
        for t in 2..=10 {
            m.add(s(t * 1_000), t * 1_000_000);
        }
        assert_eq!(m.add(s(10_500), 10_500_000), 1_000_000);
        // …then twice as fast: the window forgets the old pace.
        let mut bytes = 10_500_000;
        let mut last = 0;
        for t in 1..=6 {
            bytes += 2_000_000;
            last = m.add(s(10_500 + t * 1_000), bytes);
        }
        assert!((1_900_000..=2_000_000).contains(&last), "{last}");
    }

    #[test]
    fn a_ticket_cancels_its_download_once_and_lets_go() {
        let t = Ticket::begin("test:one").expect("first");
        assert!(Ticket::begin("test:one").is_none(), "one at a time");
        assert!(running("test:one"));
        assert!(!t.cancelled());
        assert!(cancel("test:one"));
        assert!(t.cancelled());
        drop(t);
        assert!(!running("test:one"));
        assert!(!cancel("test:one"), "nothing to cancel");
        // Unlisted: never cancelled by anyone.
        let u = Ticket::unlisted();
        assert!(!cancel(""));
        assert!(!u.cancelled());
        assert!(is_cancelled(&format!("x: {CANCELLED}")));
    }

    /// A tiny HTTP server for the download tests: `body`, honouring a Range,
    /// optionally slowly; counts the requests and the ranges asked for.
    fn serve(body: Vec<u8>, ranges: bool, delay_ms: u64) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let url = format!("http://{}/model.gguf", server.server_addr().to_ip().expect("ip"));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let range = req.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.to_string());
                log.lock().unwrap().push(range.clone());
                let from = range
                    .as_deref()
                    .filter(|_| ranges)
                    .and_then(|r| r.strip_prefix("bytes=")?.trim_end_matches('-').parse::<usize>().ok())
                    .unwrap_or(0);
                let part = body[from.min(body.len())..].to_vec();
                if delay_ms > 0 {
                    // Slow: in pieces.
                    let mut resp = tiny_http::Response::new(
                        if from > 0 { tiny_http::StatusCode(206) } else { tiny_http::StatusCode(200) },
                        if from > 0 {
                            vec![tiny_http::Header::from_bytes("Content-Range", format!("bytes {from}-{}/{}", body.len() - 1, body.len())).unwrap()]
                        } else {
                            vec![]
                        },
                        SlowReader { data: part.clone(), at: 0, delay_ms },
                        Some(part.len()),
                        None,
                    );
                    resp = resp.with_chunked_threshold(usize::MAX);
                    let _ = req.respond(resp);
                } else {
                    let mut resp = tiny_http::Response::from_data(part.clone());
                    if from > 0 {
                        resp = resp
                            .with_status_code(206)
                            .with_header(tiny_http::Header::from_bytes("Content-Range", format!("bytes {from}-{}/{}", body.len() - 1, body.len())).unwrap());
                    }
                    let _ = req.respond(resp);
                }
            }
        });
        (url, asked)
    }

    struct SlowReader {
        data: Vec<u8>,
        at: usize,
        delay_ms: u64,
    }

    impl std::io::Read for SlowReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.at >= self.data.len() {
                return Ok(0);
            }
            std::thread::sleep(Duration::from_millis(self.delay_ms));
            let n = buf.len().min(4096).min(self.data.len() - self.at);
            buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        }
    }

    fn body(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i % 251) as u8).collect()
    }

    /// A fresh folder of the test's own under the system's temp folder.
    fn scratch() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("yap-download-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn a_download_resumes_where_it_stopped() {
        let data = body(300_000);
        let (url, asked) = serve(data.clone(), true, 0);
        let dir = scratch();
        let partial = dir.join("model.gguf.partial");
        // An earlier try got the first 100,000 bytes.
        std::fs::write(&partial, &data[..100_000]).unwrap();
        let ticket = Ticket::unlisted();
        let seen = Mutex::new(Vec::new());
        let got = fetch(
            Fetch { url: &url, partial: &partial, size: Some(300_000), what: crate::privacy::net::Download::OnDeviceAi, ticket: &ticket },
            &|p| seen.lock().unwrap().push(p),
        )
        .await
        .unwrap();
        assert_eq!(got, 300_000);
        assert_eq!(std::fs::read(&partial).unwrap(), data, "the rest appended, not the whole again");
        assert_eq!(asked.lock().unwrap().as_slice(), [Some("bytes=100000-".to_string())]);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.first().unwrap().downloaded, 100_000, "starts where it was");
        assert_eq!(seen.last().unwrap().percent(), 100);
        // Whole already: no request at all.
        fetch(
            Fetch { url: &url, partial: &partial, size: Some(300_000), what: crate::privacy::net::Download::OnDeviceAi, ticket: &ticket },
            &|_| {},
        )
        .await
        .unwrap();
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_server_that_sends_it_all_again_starts_over() {
        let data = body(50_000);
        let (url, _) = serve(data.clone(), false, 0);
        let dir = scratch();
        let partial = dir.join("x.partial");
        std::fs::write(&partial, vec![9u8; 20_000]).unwrap();
        let ticket = Ticket::unlisted();
        fetch(Fetch { url: &url, partial: &partial, size: None, what: crate::privacy::net::Download::SpeechModel, ticket: &ticket }, &|_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&partial).unwrap(), data);
    }

    #[tokio::test]
    async fn a_cancelled_download_leaves_no_half_file() {
        let data = body(400_000);
        let (url, _) = serve(data, true, 20);
        let dir = scratch();
        let partial = dir.join("slow.partial");
        let ticket = Ticket::begin("test:slow").unwrap();
        let seen = Mutex::new(0u64);
        let note = |p: Progress| *seen.lock().unwrap() = p.downloaded;
        let run = fetch(
            Fetch { url: &url, partial: &partial, size: Some(400_000), what: crate::privacy::net::Download::SpeakerModel, ticket: &ticket },
            &note,
        );
        let canceller = async {
            // Once some of it has come.
            while *seen.lock().unwrap() < 8_192 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(partial.exists());
            assert!(cancel("test:slow"));
        };
        let (result, _) = tokio::join!(run, canceller);
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert!(!partial.exists(), "the partial file went");
    }

    #[tokio::test]
    async fn more_than_the_pinned_size_is_refused() {
        let (url, _) = serve(body(10_000), true, 0);
        let dir = scratch();
        let partial = dir.join("big.partial");
        let ticket = Ticket::unlisted();
        let e = fetch(Fetch { url: &url, partial: &partial, size: Some(5_000), what: crate::privacy::net::Download::SpeakerModel, ticket: &ticket }, &|_| {})
            .await
            .unwrap_err();
        assert!(e.contains("bigger than expected"), "{e}");
        assert!(!partial.exists());
    }
}
