//! **Network receipts** (Settings → Privacy & data → Network connections):
//! every connection Yap makes, by host and purpose, kept on this PC for a
//! week in `connections.json`.
//!
//! Every outbound HTTP request goes through one place, [`Recorded::send_for`]
//! (in place of reqwest's `.send()`), which records **when**, the **host**
//! (never the path, the query, a header or a body: nothing with content or a
//! key), **what it was for** ([`Purpose`] + the [`Feature`] it ran under:
//! "AI cleanup · Groq", "Web search · Exa", "Speech model download"…),
//! the **status**, and the **bytes** sent and received (the request's line,
//! headers and body; the answer's head and body, counted as it's read). The
//! recorder only observes: the request goes exactly as it would have, and
//! its answer comes back the same `reqwest::Response` (status, headers,
//! URL, size, timeouts), its body counted on the way through.
//!
//! What isn't reqwest 0.12 is recorded by hand ([`begin`]): the updater's
//! check and download (its own HTTP client), the Local API's answers to
//! apps on this PC (`bridge.rs`), and the account picture the page loads
//! (`connections_note`). Connections to this PC (the on-device model's
//! server at 127.0.0.1, a self-hosted server on `localhost`, the Local API)
//! are recorded as **on this PC**; a private address or `.local` name as
//! **your network**; the rest as **the internet** ([`place_of`]).
//!
//! The store ([`Store`]) folds requests of the same purpose, feature and
//! host within ten minutes into one entry (a count, failures, bytes), and
//! keeps 7 days and at most [`MAX_ENTRIES`] entries. It's written by a
//! background thread a few seconds after a change (and at exit, [`flush`]),
//! atomically like every other store. Child processes (`yap.exe mcp`,
//! `eval`, `diarize`) never call [`init`], so they record nothing to disk.
//! The model evaluations' own requests (`eval/`, debug builds, the
//! maintainer's tool) don't come through here.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// Requests of one purpose, feature and host within this long fold into one
/// entry.
const BUCKET_SECS: u64 = 600;
/// How far back an entry is looked for to fold a request into.
const LOOK_BACK: usize = 96;
/// Kept for a week.
pub const KEEP_SECS: u64 = 7 * 86_400;
/// At most this many entries (the oldest go first).
pub const MAX_ENTRIES: usize = 4_000;
/// Written this long after a change, at most.
const WRITE_EVERY: Duration = Duration::from_secs(3);
/// The page's live refresh.
pub const EVENT: &str = "yap-connections";
const FILE: &str = "connections.json";

// ---- what a request is for -----------------------------------------------------------

/// The part of Yap a language-model call (or a search it makes) belongs to.
/// Set around a call with [`within`]; an outer one wins (the Voice
/// Assistant's turn runs Chat's code, and stays the Voice Assistant's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Dictation,
    Commands,
    Chat,
    Assistant,
    Notes,
    Meetings,
    Brief,
    Memory,
}

impl Feature {
    fn name(self) -> &'static str {
        match self {
            Feature::Dictation => "Dictation",
            Feature::Commands => "Commands",
            Feature::Chat => "Chat",
            Feature::Assistant => "Voice Assistant",
            Feature::Notes => "Notes",
            Feature::Meetings => "Meetings",
            Feature::Brief => "Calendar",
            Feature::Memory => "Memory",
        }
    }
    /// What an AI call made for it does, for the label.
    fn activity(self) -> &'static str {
        match self {
            Feature::Dictation => "AI cleanup",
            Feature::Commands => "Commands",
            Feature::Chat => "Chat",
            Feature::Assistant => "Voice Assistant",
            Feature::Notes => "Notes",
            Feature::Meetings => "Meeting notes",
            Feature::Brief => "Meeting brief",
            Feature::Memory => "Memory",
        }
    }
}

tokio::task_local! {
    static FEATURE: Feature;
}

/// Run `fut` as part of `feature`: the AI calls and searches it makes are
/// recorded as that feature's. An enclosing [`within`] wins.
pub async fn within<F: Future>(feature: Feature, fut: F) -> F::Output {
    if FEATURE.try_with(|_| ()).is_ok() {
        fut.await
    } else {
        FEATURE.scope(feature, fut).await
    }
}

fn current_feature() -> Option<Feature> {
    FEATURE.try_with(|f| *f).ok()
}

/// A model, a voice or the like Yap downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Download {
    SpeechModel,
    OnDeviceAi,
    SpeakerModel,
    Voice,
    /// Meeting search's embedding model (`search/embedder.rs`).
    SearchModel,
}

/// What a request is for. Its label never carries anything the person
/// wrote: only Yap's own words and the names of the services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// A language-model call (`llm.rs`): the provider id ("groq", "ondevice"…).
    Ai(String),
    /// The on-device model's server starting up (`local_llm.rs` /health).
    OnDeviceCheck,
    /// Its first, warm-up answer.
    OnDeviceWarmUp,
    /// Meeting search's embedding model on this PC (`search/embedder.rs`):
    /// its start and the passages and questions it reads.
    Embeddings,
    /// Yap's own search: "Exa" | "Tavily".
    WebSearch(&'static str),
    /// The keyless lookup: Wikipedia and the games' wikis.
    WikiSearch,
    /// A cloud model's own search (`web/native.rs`): the provider's name.
    ProviderSearch(&'static str),
    /// Chat reading a web page (`read_page`).
    WebPage,
    /// A provider's (or a server's) list of models: its name in words.
    ModelList(String),
    Download(Download),
    UpdateCheck,
    UpdateDownload,
    /// The optional account service (`auth.rs`).
    Account,
    /// The signed-in account's picture, which the page loads.
    AccountPicture,
    /// "Google" | "iCal link".
    Calendar(&'static str),
    /// Spoken replies from a cloud voice service: its name.
    Voice(&'static str),
    /// That service's voices and usage.
    VoiceList(&'static str),
    /// A voice's sample, played in Settings.
    VoiceSample(&'static str),
    /// An app on this PC using the Local API (`bridge.rs`).
    LocalApi,
}

/// What a request is, in the store's words: its kind (for the page's
/// icon), its label and the feature it ran under.
struct Described {
    kind: &'static str,
    label: String,
    feature: &'static str,
}

/// A provider id in words. Only Yap's own ids are named; anything else is
/// "AI provider" (a label never echoes a string it doesn't know).
pub fn provider_name(id: &str) -> &'static str {
    match id {
        "groq" => "Groq",
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "gemini" => "Gemini",
        "openrouter" => "OpenRouter",
        "ondevice" => "On-device model",
        "local" => "Your server",
        "custom" => "Custom endpoint",
        _ => "AI provider",
    }
}

impl Purpose {
    fn describe(&self, feature: Option<Feature>) -> Described {
        let f = |default: &'static str| feature.map_or(default, Feature::name);
        let (kind, label, feature) = match self {
            Purpose::Ai(p) => (
                "ai",
                format!("{} · {}", feature.map_or("AI model", Feature::activity), provider_name(p)),
                f("AI models"),
            ),
            Purpose::OnDeviceCheck => ("ai", "On-device AI · starting up".into(), "AI models"),
            Purpose::OnDeviceWarmUp => ("ai", "On-device AI · warming up".into(), "AI models"),
            Purpose::Embeddings => ("ai", "Meeting search · on-device model".into(), "Meeting search"),
            Purpose::WebSearch(s) => ("web", format!("Web search · {s}"), f("Chat")),
            Purpose::WikiSearch => ("web", "Web search · Wikipedia and wikis".into(), f("Chat")),
            Purpose::ProviderSearch(p) => ("web", format!("Web search · {p}"), f("Chat")),
            Purpose::WebPage => ("web", "Reading a web page".into(), f("Chat")),
            Purpose::ModelList(who) => ("models", format!("Model list · {who}"), "Settings"),
            Purpose::Download(d) => match d {
                Download::SpeechModel => ("download", "Speech model download".into(), "Speech-to-Text"),
                Download::OnDeviceAi => ("download", "On-device AI download".into(), "AI models"),
                Download::SpeakerModel => ("download", "Speaker model download".into(), "Meetings"),
                Download::Voice => ("download", "Voice download (Kokoro)".into(), "Spoken replies"),
                Download::SearchModel => ("download", "Meeting search model download".into(), "Meeting search"),
            },
            Purpose::UpdateCheck => ("update", "Update check".into(), "Updates"),
            Purpose::UpdateDownload => ("update", "Update download".into(), "Updates"),
            Purpose::Account => ("account", "Account".into(), "Account"),
            Purpose::AccountPicture => ("account", "Account picture".into(), "Account"),
            Purpose::Calendar(k) => ("calendar", format!("Calendar sync · {k}"), "Calendar"),
            Purpose::Voice(s) => ("voice", format!("Voice · {s}"), "Spoken replies"),
            Purpose::VoiceList(s) => ("voice", format!("Voice list · {s}"), "Spoken replies"),
            Purpose::VoiceSample(s) => ("voice", format!("Voice sample · {s}"), "Settings"),
            Purpose::LocalApi => ("localapi", "Local API · apps on this PC".into(), "AI apps (MCP)"),
        };
        Described { kind, label, feature }
    }
}

// ---- where it went -------------------------------------------------------------------

/// Where a host is, as far as Yap can tell from its name or address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// This PC: loopback, `localhost`.
    Pc,
    /// Your network: a private or link-local address, a `.local` name, a
    /// name with no dots.
    Network,
    /// Everything else.
    Internet,
}

/// Where `host` (a name, an IPv4 address, or an IPv6 one in brackets) is.
/// Pure.
pub fn place_of(host: &str) -> Place {
    let h = without_port(host.trim()).trim_end_matches('.').to_ascii_lowercase();
    let bare = h.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                let o = v4.octets();
                if v4.is_loopback() || v4.is_unspecified() {
                    Place::Pc
                } else if v4.is_private() || v4.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1])) {
                    Place::Network
                } else {
                    Place::Internet
                }
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback() || v6.is_unspecified() {
                    return Place::Pc;
                }
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return place_of(&v4.to_string());
                }
                let first = v6.segments()[0];
                if (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 {
                    Place::Network
                } else {
                    Place::Internet
                }
            }
        };
    }
    if h == "localhost" || h.ends_with(".localhost") {
        return Place::Pc;
    }
    if h.is_empty()
        || !h.contains('.')
        || [".local", ".lan", ".home", ".internal", ".home.arpa", ".intranet", ".corp"]
            .iter()
            .any(|s| h.ends_with(s))
    {
        return Place::Network;
    }
    Place::Internet
}

/// `host` without a `:port` ("[::1]:8080" → "[::1]", "nas.local:11434" →
/// "nas.local"; a bare IPv6 address stays whole). Pure.
fn without_port(host: &str) -> &str {
    if let Some(end) = host.strip_prefix('[').and_then(|rest| rest.find(']')) {
        return &host[..end + 2];
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

/// The host a URL names, as the store keeps it: the name (lower case) or
/// address, with a port only when it isn't the scheme's own and the host
/// isn't this PC (the on-device server takes a new port each start). Never
/// the path, the query, a user name or a password. Pure.
pub fn host_of(url: &reqwest::Url) -> String {
    let host = match url.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(url::Host::Ipv4(ip)) => ip.to_string(),
        Some(url::Host::Ipv6(ip)) => format!("[{ip}]"),
        None => return String::new(),
    };
    match url.port() {
        Some(port) if place_of(&host) != Place::Pc => format!("{host}:{port}"),
        _ => host,
    }
}

/// A test run's stand-ins for cloud services (debug builds, test mode
/// only): `YAP_E2E_NET_ALIASES="127.0.0.1:5123=api.groq.com;…"` records a
/// request to the fake server as one to the service it stands in for, so
/// the suite can show a cloud call without going on the internet.
fn logical_host(url: &reqwest::Url) -> String {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        static ALIASES: OnceLock<Vec<(String, String)>> = OnceLock::new();
        let aliases = ALIASES.get_or_init(|| {
            std::env::var("YAP_E2E_NET_ALIASES")
                .unwrap_or_default()
                .split(';')
                .filter_map(|pair| pair.split_once('='))
                .map(|(from, to)| (from.trim().to_ascii_lowercase(), to.trim().to_ascii_lowercase()))
                .filter(|(from, to)| !from.is_empty() && !to.is_empty())
                .collect()
        });
        if let (Some(h), Some(port)) = (url.host_str(), url.port_or_known_default()) {
            let actual = format!("{}:{port}", h.to_ascii_lowercase());
            if let Some((_, to)) = aliases.iter().find(|(from, _)| *from == actual) {
                return to.clone();
            }
        }
    }
    host_of(url)
}

// ---- a request on its way --------------------------------------------------------------

/// Unix seconds now.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How a request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// It was answered: the HTTP status (0 when it isn't known).
    Answered(u16),
    /// No answer (offline, a timeout), or the answer broke off.
    Failed(u16),
    /// Yap stopped waiting (a stop button, a newer request).
    Stopped,
}

/// One request, finished: what goes into the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub at: u64,
    pub kind: String,
    pub label: String,
    pub feature: String,
    pub host: String,
    pub place: Place,
    pub status: u16,
    pub failed: bool,
    pub sent: u64,
    pub received: u64,
}

/// A request that went out: recorded once, when it ends — answered, failed,
/// or dropped unanswered (then "stopped").
struct Pending {
    at: u64,
    kind: &'static str,
    label: String,
    feature: &'static str,
    host: String,
    place: Place,
    sent: u64,
    received: u64,
    done: bool,
}

impl Pending {
    fn new(purpose: &Purpose, host: String, sent: u64) -> Pending {
        let d = purpose.describe(current_feature());
        let place = if matches!(purpose, Purpose::LocalApi) { Place::Pc } else { place_of(&host) };
        Pending { at: now(), kind: d.kind, label: d.label, feature: d.feature, host, place, sent, received: 0, done: false }
    }

    fn finish(&mut self, outcome: Outcome) {
        if std::mem::replace(&mut self.done, true) {
            return;
        }
        let (status, failed) = match outcome {
            Outcome::Answered(s) => (s, s >= 400),
            Outcome::Failed(s) => (s, true),
            Outcome::Stopped => (0, false),
        };
        record(Receipt {
            at: self.at,
            kind: self.kind.to_string(),
            label: std::mem::take(&mut self.label),
            feature: self.feature.to_string(),
            host: std::mem::take(&mut self.host),
            place: self.place,
            status,
            failed,
            sent: self.sent,
            received: self.received,
        });
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.finish(Outcome::Stopped);
    }
}

/// The bytes of a request's line and headers (as HTTP/1.1 would send them)
/// and its body, when it's in memory.
fn request_bytes(req: &reqwest::Request) -> u64 {
    let url = req.url();
    let mut n = req.method().as_str().len() + 1 + url.path().len() + url.query().map_or(0, |q| q.len() + 1) + 11;
    n += 8 + url.host_str().map_or(0, str::len);
    for (k, v) in req.headers() {
        n += k.as_str().len() + 2 + v.len() + 2;
    }
    n += 2;
    n as u64 + req.body().and_then(|b| b.as_bytes()).map_or(0, |b| b.len() as u64)
}

/// The bytes of an answer's status line and headers.
fn response_head_bytes(resp: &reqwest::Response) -> u64 {
    let mut n = 17 + resp.status().canonical_reason().map_or(0, str::len);
    for (k, v) in resp.headers() {
        n += k.as_str().len() + 2 + v.len() + 2;
    }
    (n + 2) as u64
}

/// An answer's body, counted as it's read; the receipt is written when it
/// ends or is dropped.
struct Counting {
    inner: reqwest::Body,
    pending: Pending,
    status: u16,
}

impl http_body::Body for Counting {
    type Data = <reqwest::Body as http_body::Body>::Data;
    type Error = reqwest::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.pending.received += data.len() as u64;
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                this.pending.finish(Outcome::Failed(this.status));
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                this.pending.finish(Outcome::Answered(this.status));
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for Counting {
    fn drop(&mut self) {
        // Answered: whatever of the body was read (a caller that only wanted
        // the status reads none of it).
        self.pending.finish(Outcome::Answered(self.status));
    }
}

/// The same answer, its body counted on the way through: status, headers,
/// URL, extensions and size hint as they were.
fn counted(resp: reqwest::Response, mut pending: Pending) -> reqwest::Response {
    use reqwest::ResponseBuilderExt;
    let status = resp.status().as_u16();
    pending.received = response_head_bytes(&resp);
    let url = resp.url().clone();
    let (mut parts, body) = tauri::http::Response::<reqwest::Body>::from(resp).into_parts();
    // reqwest keeps the response's URL in an extension its `From` drops.
    if let Ok(marker) = tauri::http::response::Builder::new().url(url).body(()) {
        parts.extensions.extend(marker.into_parts().0.extensions);
    }
    let body = reqwest::Body::wrap(Counting { inner: body, pending, status });
    reqwest::Response::from(tauri::http::Response::from_parts(parts, body))
}

/// Send `builder`'s request, recorded as `purpose`. A request that can't
/// be built never went out, so isn't recorded.
pub async fn send(builder: reqwest::RequestBuilder, purpose: Purpose) -> reqwest::Result<reqwest::Response> {
    let (client, request) = builder.build_split();
    let request = request?;
    let mut pending = Pending::new(&purpose, logical_host(request.url()), request_bytes(&request));
    match client.execute(request).await {
        Ok(resp) => Ok(counted(resp, pending)),
        Err(e) => {
            pending.finish(Outcome::Failed(e.status().map_or(0, |s| s.as_u16())));
            Err(e)
        }
    }
}

/// reqwest's `.send()`, recorded: `client.get(url).send_for(Purpose::…)`.
pub trait Recorded {
    fn send_for(self, purpose: Purpose) -> impl Future<Output = reqwest::Result<reqwest::Response>> + Send;
}

impl Recorded for reqwest::RequestBuilder {
    fn send_for(self, purpose: Purpose) -> impl Future<Output = reqwest::Result<reqwest::Response>> + Send {
        send(self, purpose)
    }
}

/// A connection made some other way (the updater, the Local API, the page),
/// recorded by hand: [`Manual::answered`] or [`Manual::failed`] when it's
/// over; dropped unfinished, it's recorded as stopped.
pub struct Manual(Pending);

/// Start recording a connection to `url` (only its host is kept).
pub fn begin(purpose: Purpose, url: &str) -> Manual {
    let host = reqwest::Url::parse(url).map(|u| logical_host(&u)).unwrap_or_default();
    Manual(Pending::new(&purpose, host, 0))
}

impl Manual {
    /// It was answered (`status` 0 when it isn't known).
    pub fn answered(mut self, status: u16, sent: u64, received: u64) {
        self.0.sent = sent;
        self.0.received = received;
        self.0.finish(Outcome::Answered(status));
    }
    pub fn failed(mut self, sent: u64, received: u64) {
        self.0.sent = sent;
        self.0.received = received;
        self.0.finish(Outcome::Failed(0));
    }
}

/// An app on this PC asked the Local API, and Yap answered `status` with
/// `sent` bytes for its `received` (`bridge.rs`).
pub fn local_api(status: u16, received: u64, sent: u64) {
    let mut p = Pending::new(&Purpose::LocalApi, "127.0.0.1".into(), sent);
    p.received = received;
    p.finish(Outcome::Answered(status));
}

// ---- the store -----------------------------------------------------------------------

/// Requests of one purpose, feature and host, folded together (within
/// [`BUCKET_SECS`] of each other).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The first one's start (unix seconds).
    pub at: u64,
    /// The last one's start.
    pub last: u64,
    pub kind: String,
    pub label: String,
    pub feature: String,
    pub host: String,
    pub place: Place,
    pub count: u32,
    /// How many got no answer, broke off or were refused (HTTP 400 and up).
    pub failed: u32,
    /// The last one's HTTP status (0: none, or not known).
    pub status: u16,
    /// Bytes Yap sent.
    pub sent: u64,
    /// Bytes Yap received.
    pub received: u64,
}

/// `connections.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub entries: Vec<Entry>,
}

impl Store {
    /// Fold `r` into a recent entry like it, or add one; then roll over.
    pub fn add(&mut self, r: &Receipt, now: u64) {
        let same = |e: &Entry| {
            e.kind == r.kind && e.label == r.label && e.feature == r.feature && e.host == r.host && e.place == r.place
        };
        // The nearest entry like it within the window (a request that ran
        // long can finish after a newer one started an entry of its own).
        let from = self.entries.len().saturating_sub(LOOK_BACK);
        let hit = self.entries[from..]
            .iter_mut()
            .filter(|e| same(e) && r.at.abs_diff(e.at) < BUCKET_SECS)
            .min_by_key(|e| r.at.abs_diff(e.at));
        match hit {
            Some(e) => {
                if r.at >= e.last {
                    e.status = r.status;
                    e.last = r.at;
                }
                e.at = e.at.min(r.at);
                e.count = e.count.saturating_add(1);
                e.failed = e.failed.saturating_add(u32::from(r.failed));
                e.sent = e.sent.saturating_add(r.sent);
                e.received = e.received.saturating_add(r.received);
            }
            None => self.entries.push(Entry {
                at: r.at,
                last: r.at,
                kind: r.kind.clone(),
                label: r.label.clone(),
                feature: r.feature.clone(),
                host: r.host.clone(),
                place: r.place,
                count: 1,
                failed: u32::from(r.failed),
                status: r.status,
                sent: r.sent,
                received: r.received,
            }),
        }
        self.roll(now);
    }

    /// A week, and at most [`MAX_ENTRIES`] (the oldest go first).
    pub fn roll(&mut self, now: u64) {
        let cutoff = now.saturating_sub(KEEP_SECS);
        self.entries.retain(|e| e.last >= cutoff);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.sort_by_key(|e| e.last);
            let extra = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..extra);
        }
    }
}

struct State {
    store: Store,
    /// Read from disk yet (once [`init`] named the file).
    loaded: bool,
    dirty: bool,
}

static STATE: Mutex<State> = Mutex::new(State { store: Store { version: 1, entries: Vec::new() }, loaded: false, dirty: false });
static FILE_PATH: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

fn lock() -> std::sync::MutexGuard<'static, State> {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if !s.loaded {
        if let Some(path) = FILE_PATH.get() {
            s.loaded = true;
            let earlier = std::mem::take(&mut s.store.entries);
            s.store = read(path);
            // What was recorded before the file was read (startup).
            let t = now();
            for e in earlier {
                s.store.entries.push(e);
            }
            s.store.entries.sort_by_key(|e| e.at);
            s.store.roll(t);
        }
    }
    s
}

fn read(path: &std::path::Path) -> Store {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Store>(&text) {
            Ok(mut store) => {
                store.version = 1;
                store
            }
            Err(_) => {
                crate::config::quarantine_corrupt(path);
                Store { version: 1, entries: Vec::new() }
            }
        },
        Err(_) => Store { version: 1, entries: Vec::new() },
    }
}

/// Into the store (and, a few seconds later, the file).
fn record(r: Receipt) {
    let mut s = lock();
    s.store.add(&r, now());
    s.dirty = true;
}

/// The store's file from now on, and the thread that writes it. Called once
/// from `lib.rs` setup.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if FILE_PATH.set(crate::config::data_dir().join(FILE)).is_err() {
        return;
    }
    drop(lock()); // read it now, off the request path
    let _ = std::thread::Builder::new().name("yap-net-receipts".into()).spawn(|| loop {
        std::thread::sleep(WRITE_EVERY);
        flush();
    });
}

/// Write the store if it changed (the writer thread, and at exit).
pub fn flush() {
    let Some(path) = FILE_PATH.get() else { return };
    let text = {
        let mut s = lock();
        if !s.dirty {
            return;
        }
        s.dirty = false;
        match serde_json::to_string(&s.store) {
            Ok(t) => t,
            Err(_) => return,
        }
    };
    if let Err(e) = crate::config::atomic_write(path, &text) {
        tracing::warn!(error = %e, "connections: couldn't save the store");
        lock().dirty = true;
        return;
    }
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

/// Every entry kept (the last week), oldest first.
pub fn entries() -> Vec<Entry> {
    let mut s = lock();
    s.store.roll(now());
    s.store.entries.clone()
}

/// Forget every entry (the page's Clear), and write that at once.
pub fn clear() {
    {
        let mut s = lock();
        s.store.entries.clear();
        s.dirty = true;
    }
    flush();
}

// ---- the page's summary ---------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub count: u64,
    pub failed: u64,
    pub sent: u64,
    pub received: u64,
    /// Of `count`, to your network or the internet.
    pub leaving: u64,
    /// Of `count`, on this PC.
    pub local: u64,
}

impl Totals {
    fn add(&mut self, e: &Entry) {
        self.count += u64::from(e.count);
        self.failed += u64::from(e.failed);
        self.sent += e.sent;
        self.received += e.received;
        if e.place == Place::Pc {
            self.local += u64::from(e.count);
        } else {
            self.leaving += u64::from(e.count);
        }
    }
}

/// A few of a host's latest moments, newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub at: u64,
    pub last: u64,
    pub count: u32,
    pub failed: u32,
    pub status: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRow {
    pub host: String,
    pub place: Place,
    pub count: u64,
    pub failed: u64,
    pub sent: u64,
    pub received: u64,
    pub first: u64,
    pub last: u64,
    pub status: u16,
    pub features: Vec<String>,
    pub recent: Vec<Moment>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub kind: String,
    pub label: String,
    pub hosts: Vec<HostRow>,
    pub last: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub now: u64,
    pub today_start: u64,
    pub today: Totals,
    pub week: Totals,
    /// By purpose, the latest first.
    pub groups: Vec<Group>,
}

/// A purpose's entries by host.
type ByHost<'a> = BTreeMap<(String, Place), Vec<&'a Entry>>;

/// The page's view of `entries`: totals for today and the week, and each
/// purpose with its hosts (the latest first). Pure.
pub fn summarize(entries: &[Entry], now: u64, today_start: u64) -> Summary {
    let mut today = Totals::default();
    let mut week = Totals::default();
    // (kind, label) → host → entries
    let mut groups: BTreeMap<(String, String), ByHost> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.last + KEEP_SECS >= now) {
        week.add(e);
        if e.last >= today_start {
            today.add(e);
        }
        groups
            .entry((e.kind.clone(), e.label.clone()))
            .or_default()
            .entry((e.host.clone(), e.place))
            .or_default()
            .push(e);
    }
    let mut out: Vec<Group> = groups
        .into_iter()
        .map(|((kind, label), hosts)| {
            let mut rows: Vec<HostRow> = hosts
                .into_iter()
                .map(|((host, place), mut es)| {
                    es.sort_by_key(|e| std::cmp::Reverse(e.last));
                    let mut features: Vec<String> = es.iter().map(|e| e.feature.clone()).collect();
                    features.sort();
                    features.dedup();
                    HostRow {
                        host,
                        place,
                        count: es.iter().map(|e| u64::from(e.count)).sum(),
                        failed: es.iter().map(|e| u64::from(e.failed)).sum(),
                        sent: es.iter().map(|e| e.sent).sum(),
                        received: es.iter().map(|e| e.received).sum(),
                        first: es.iter().map(|e| e.at).min().unwrap_or(0),
                        last: es.first().map_or(0, |e| e.last),
                        status: es.first().map_or(0, |e| e.status),
                        features,
                        recent: es
                            .iter()
                            .take(6)
                            .map(|e| Moment { at: e.at, last: e.last, count: e.count, failed: e.failed, status: e.status })
                            .collect(),
                    }
                })
                .collect();
            rows.sort_by_key(|r| std::cmp::Reverse(r.last));
            let last = rows.first().map_or(0, |r| r.last);
            Group { kind, label, hosts: rows, last }
        })
        .collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.last));
    Summary { now, today_start, today, week, groups: out }
}

/// The start of today on this PC's clock (unix seconds).
pub fn today_start() -> u64 {
    use chrono::{Local, TimeZone};
    let today = Local::now().date_naive();
    today
        .and_hms_opt(0, 0, 0)
        .and_then(|t| Local.from_local_datetime(&t).earliest())
        .map_or_else(|| now() - now() % 86_400, |t| t.timestamp().max(0) as u64)
}

// ---- commands ------------------------------------------------------------------------

/// Settings → Privacy & data → Network connections: the last week.
#[tauri::command]
pub fn connections_summary() -> Summary {
    summarize(&entries(), now(), today_start())
}

/// Its Clear.
#[tauri::command]
pub fn connections_clear() {
    clear();
    tracing::info!("connections: cleared");
}

/// A connection the page made itself, which Rust can't see: the signed-in
/// account's picture (`<img>` from the sign-in provider). Only purposes on
/// this list, and only the URL's host is kept.
#[tauri::command]
pub fn connections_note(purpose: String, url: String) -> Result<(), String> {
    let purpose = match purpose.as_str() {
        "account-picture" => Purpose::AccountPicture,
        _ => return Err("unknown purpose".into()),
    };
    let parsed = reqwest::Url::parse(&url).map_err(|_| "not a link".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("not a web link".into());
    }
    begin(purpose, &url).answered(0, 0, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(at: u64, label: &str, host: &str) -> Receipt {
        Receipt {
            at,
            kind: "ai".into(),
            label: label.into(),
            feature: "Dictation".into(),
            host: host.into(),
            place: place_of(host),
            status: 200,
            failed: false,
            sent: 100,
            received: 50,
        }
    }

    #[test]
    fn places() {
        for h in ["127.0.0.1", "127.3.2.1", "localhost", "LOCALHOST", "app.localhost", "[::1]", "0.0.0.0"] {
            assert_eq!(place_of(h), Place::Pc, "{h}");
        }
        assert_eq!(place_of("127.0.0.1:8080"), Place::Pc);
        assert_eq!(place_of("[::1]:8080"), Place::Pc);
        assert_eq!(place_of("nas.local:11434"), Place::Network);
        assert_eq!(place_of("api.example.com:8443"), Place::Internet);
        for h in ["192.168.1.20", "10.0.0.5", "172.16.4.4", "169.254.1.1", "100.64.0.9", "nas.local", "my-server", "[fd00::1]", "[fe80::1]", "box.home.arpa"] {
            assert_eq!(place_of(h), Place::Network, "{h}");
        }
        for h in ["api.groq.com", "8.8.8.8", "[2606:4700::1111]", "en.wikipedia.org", "172.32.0.1"] {
            assert_eq!(place_of(h), Place::Internet, "{h}");
        }
    }

    #[test]
    fn a_host_is_only_its_name_never_the_path_query_or_credentials() {
        let u = reqwest::Url::parse("https://user:pass@API.Example.com/search?q=my+secret+question&key=sk-123#frag").unwrap();
        assert_eq!(host_of(&u), "api.example.com");
        let u = reqwest::Url::parse("http://127.0.0.1:54321/v1/chat/completions?x=1").unwrap();
        assert_eq!(host_of(&u), "127.0.0.1", "this PC's ports aren't kept");
        let u = reqwest::Url::parse("http://192.168.1.20:11434/api/tags").unwrap();
        assert_eq!(host_of(&u), "192.168.1.20:11434");
        let u = reqwest::Url::parse("https://example.com:443/x").unwrap();
        assert_eq!(host_of(&u), "example.com");
        let u = reqwest::Url::parse("http://[::1]:8080/").unwrap();
        assert_eq!(host_of(&u), "[::1]");
    }

    #[test]
    fn labels_name_the_feature_and_service_and_never_an_unknown_string() {
        let d = Purpose::Ai("groq".into()).describe(Some(Feature::Dictation));
        assert_eq!((d.kind, d.label.as_str(), d.feature), ("ai", "AI cleanup · Groq", "Dictation"));
        let d = Purpose::Ai("ondevice".into()).describe(Some(Feature::Chat));
        assert_eq!(d.label, "Chat · On-device model");
        let d = Purpose::Ai("something the person typed".into()).describe(None);
        assert_eq!(d.label, "AI model · AI provider");
        assert_eq!(Purpose::WebSearch("Exa").describe(Some(Feature::Assistant)).feature, "Voice Assistant");
        assert_eq!(Purpose::Download(Download::SpeechModel).describe(None).label, "Speech model download");
        assert_eq!(Purpose::ModelList("OpenRouter".into()).describe(None).label, "Model list · OpenRouter");
    }

    #[tokio::test]
    async fn the_feature_comes_from_the_outermost_scope() {
        assert_eq!(current_feature(), None);
        let inner = within(Feature::Assistant, async { within(Feature::Chat, async { current_feature() }).await }).await;
        assert_eq!(inner, Some(Feature::Assistant));
        assert_eq!(within(Feature::Chat, async { current_feature() }).await, Some(Feature::Chat));
    }

    #[test]
    fn requests_fold_into_ten_minute_entries_with_counts_and_bytes() {
        let mut s = Store::default();
        let t = 1_800_000_000;
        s.add(&receipt(t, "AI cleanup · Groq", "api.groq.com"), t);
        s.add(&receipt(t + 60, "AI cleanup · Groq", "api.groq.com"), t + 60);
        let mut failed = receipt(t + 120, "AI cleanup · Groq", "api.groq.com");
        failed.failed = true;
        failed.status = 503;
        s.add(&failed, t + 120);
        // Another host, another purpose, later: entries of their own.
        s.add(&receipt(t + 130, "AI cleanup · Groq", "127.0.0.1"), t + 130);
        s.add(&receipt(t + 140, "Chat · Groq", "api.groq.com"), t + 140);
        s.add(&receipt(t + BUCKET_SECS + 5, "AI cleanup · Groq", "api.groq.com"), t + BUCKET_SECS + 5);
        assert_eq!(s.entries.len(), 4);
        let e = &s.entries[0];
        assert_eq!((e.count, e.failed, e.sent, e.received, e.status), (3, 1, 300, 150, 503));
        assert_eq!((e.at, e.last), (t, t + 120));
        assert_eq!(s.entries[1].place, Place::Pc);
        // A request that started earlier but finished later (a long
        // download) folds in without moving `last` back.
        s.add(&receipt(t + 30, "AI cleanup · Groq", "api.groq.com"), t + 200);
        assert_eq!((s.entries[0].count, s.entries[0].at, s.entries[0].last, s.entries[0].status), (4, t, t + 120, 503));
    }

    #[test]
    fn the_store_rolls_over_after_a_week_and_at_its_cap() {
        let mut s = Store::default();
        let t = 1_800_000_000;
        s.add(&receipt(t, "Update check", "github.com"), t);
        s.add(&receipt(t + KEEP_SECS - 10, "Account", "auth.example.com"), t + KEEP_SECS - 10);
        assert_eq!(s.entries.len(), 2);
        s.roll(t + KEEP_SECS + 1);
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].label, "Account");
        // At the cap the oldest go.
        let mut s = Store::default();
        for i in 0..(MAX_ENTRIES as u64 + 25) {
            s.add(&receipt(t + i, "Reading a web page", &format!("site{i}.example")), t + i);
        }
        assert_eq!(s.entries.len(), MAX_ENTRIES);
        assert_eq!(s.entries[0].host, "site25.example");
    }

    #[test]
    fn the_store_file_holds_hosts_and_labels_only() {
        let mut s = Store::default();
        let u = reqwest::Url::parse("https://api.groq.com/openai/v1/chat/completions?key=sk-FAKE-123").unwrap();
        let mut r = receipt(1_800_000_000, "AI cleanup · Groq", &host_of(&u));
        r.place = place_of(&r.host);
        s.add(&r, 1_800_000_000);
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("api.groq.com"));
        for leak in ["chat/completions", "openai/v1", "sk-FAKE", "key=", "?"] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }

    #[test]
    fn the_summary_groups_by_purpose_then_host_with_today_and_the_week() {
        let t = 1_800_000_000;
        let mut s = Store::default();
        s.add(&receipt(t - 2 * 86_400, "AI cleanup · Groq", "api.groq.com"), t);
        s.add(&receipt(t - 100, "AI cleanup · Groq", "api.groq.com"), t);
        let mut local = receipt(t - 50, "AI cleanup · On-device model", "127.0.0.1");
        local.kind = "ai".into();
        s.add(&local, t);
        let mut old = receipt(t - KEEP_SECS - 5, "Update check", "github.com");
        old.kind = "update".into();
        s.entries.push(Entry {
            at: old.at,
            last: old.at,
            kind: old.kind,
            label: old.label,
            feature: old.feature,
            host: old.host,
            place: Place::Internet,
            count: 1,
            failed: 0,
            status: 200,
            sent: 1,
            received: 1,
        });
        let sum = summarize(&s.entries, t, t - 3_600);
        assert_eq!(sum.week.count, 3, "a week only");
        assert_eq!((sum.today.count, sum.today.local, sum.today.leaving), (2, 1, 1));
        assert_eq!(sum.groups.len(), 2);
        assert_eq!(sum.groups[0].label, "AI cleanup · On-device model", "the latest first");
        let groq = &sum.groups[1];
        assert_eq!(groq.hosts.len(), 1);
        assert_eq!((groq.hosts[0].count, groq.hosts[0].sent, groq.hosts[0].recent.len()), (2, 200, 2));
        assert_eq!(groq.hosts[0].first, t - 2 * 86_400);
    }

    /// Every request Yap makes goes through [`Recorded::send_for`]: no
    /// `.send()` / `.execute(` on a reqwest builder or client anywhere else
    /// (the model evaluations, `eval/`, are the maintainer's tool and
    /// record nothing).
    #[test]
    fn every_request_goes_through_the_recorder() {
        fn visit(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n == "eval") {
                        continue;
                    }
                    visit(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "net.rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (i, line) in text.lines().enumerate() {
                        let code = if line.trim_start().starts_with("//") { "" } else { line };
                        if code.contains(".send()") || code.contains("client.execute(") || code.contains(".execute(req") {
                            out.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        visit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
        assert!(found.is_empty(), "requests that skip the recorder (use `.send_for(net::Purpose::…)`):\n{}", found.join("\n"));
    }

    /// A real request through [`send`] to a server on this PC: recorded with
    /// its host only, its status, and the bytes both ways; the answer reads
    /// as it would have (its URL, length and body).
    #[tokio::test]
    async fn a_request_is_recorded_by_host_status_and_bytes() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2).flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = "{\"ok\":true,\"words\":\"the answer\"}";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions?secret=sk-FAKE-xyz");
        let client = reqwest::Client::new();
        let before = entries().iter().filter(|e| e.label == "Memory · Your server").map(|e| e.count).sum::<u32>();
        let resp = within(Feature::Memory, client.post(&url).json(&serde_json::json!({"q": "private words"})).send_for(Purpose::Ai("local".into())))
            .await
            .unwrap();
        assert_eq!(resp.content_length(), Some(32));
        assert_eq!(resp.url().as_str(), url);
        assert_eq!(resp.text().await.unwrap(), "{\"ok\":true,\"words\":\"the answer\"}");
        let mine: Vec<Entry> = entries().into_iter().filter(|e| e.label == "Memory · Your server").collect();
        let e = mine.last().unwrap();
        assert_eq!(mine.iter().map(|e| e.count).sum::<u32>(), before + 1);
        assert_eq!((e.host.as_str(), e.place, e.status, e.feature.as_str()), ("127.0.0.1", Place::Pc, 200, "Memory"));
        assert!(e.sent > 17, "the request's body and head");
        assert!(e.received >= 32 + 17, "the answer's body and head");
        let text = serde_json::to_string(&mine).unwrap();
        assert!(!text.contains("secret") && !text.contains("private words") && !text.contains("chat/completions"));

        // Only the status read, the body dropped: recorded all the same.
        let resp = client.get(&url).send_for(Purpose::OnDeviceCheck).await.unwrap();
        assert!(resp.status().is_success());
        drop(resp);
        assert!(entries().iter().any(|e| e.label == "On-device AI · starting up" && e.status == 200));

        // No answer at all: a failure, its status 0.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = client
            .get(format!("http://127.0.0.1:{closed}/"))
            .send_for(Purpose::Download(Download::Voice))
            .await;
        assert!(err.is_err());
        assert!(entries().iter().any(|e| e.label == "Voice download (Kokoro)" && e.failed >= 1 && e.status == 0));
    }
}
