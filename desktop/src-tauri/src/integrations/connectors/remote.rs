//! **A server at a link** (Streamable HTTP, or the older HTTP+SSE): rmcp's
//! transport on Yap's own reqwest, so every request goes through `net.rs`
//! with its connector's purpose ("Connector · A connector") and lands in
//! the network receipts.
//!
//! The rules, checked before anything is sent and on every request:
//!
//! - **The link** ([`check_url`]): `https://`, or `http://` only to this PC
//!   or the person's network (`net::place_of`); no user name or password in
//!   it; never a cloud metadata address.
//! - **Private addresses** only when the person typed one: a link whose
//!   host is on this PC or the network (an address, `localhost`, a `.local`
//!   or dotless name) reaches it; a link to a name on the internet never
//!   connects to an address on this PC or the network, checked on the
//!   addresses the name resolves to as the connection is made (a DNS answer
//!   that changes, "rebinding", can't slip one in), and refused when any of
//!   them is private ([`Guard`], the web page reader's rule,
//!   `web::is_public`). This is MCP's SSRF advice: the person's own
//!   home server works, a stranger's name can't point Yap at it.
//! - **Redirects**: only `307`/`308` to the same origin (scheme, host,
//!   port) are followed, at most 5; any other redirect off the server's
//!   host is refused ([`HttpError::OffHost`]), so a token never goes
//!   anywhere but its own server.
//! - **Sizes**: a JSON answer over [`MAX_BODY`] (1 MB) is refused, as is
//!   one event of a stream over it; an error's body is read to 64 KB.
//! - **Secrets**: the token (`Authorization: Bearer`) and the secret
//!   headers live in [`Http`], from the vault, only ever sent to this
//!   server; never in a URL, a log or an error.
//! - **401**: with a token or a header sent, the server turned it down
//!   ([`HttpError::TokenRefused`]); without, it wants a browser sign-in
//!   (rmcp's `AuthRequired`, which the sign-in starts from, `oauth.rs`).
//! - **A browser sign-in** ([`Http::with_sign_in`]): every request carries
//!   the sign-in's access token, refreshed 30 s before it runs out
//!   (`oauth::Source`); a 401 to it refreshes once and sends the request
//!   again (the server did nothing with a request it refused), and a second
//!   401 ends the sign-in ("Sign in again"). A `403 insufficient_scope`
//!   records the scopes the server wants for the next sign-in (step-up).
//!
//! The older HTTP+SSE transport (`legacy.rs`) for a link that says so, or
//! after a `404`/`405` to the first POST ([`HttpError::NotStreamable`]).

pub mod legacy;

use std::borrow::Cow;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{BoxStream, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE, RETRY_AFTER, WWW_AUTHENTICATE};
use rmcp::model::{ClientJsonRpcMessage, JsonRpcMessage, ServerJsonRpcMessage};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, HttpStatusError, InsufficientScopeError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Error as SseError, Sse, SseStream};
use url::Url;

use crate::privacy::net::{self, Place, Recorded};

/// The most of a JSON answer, or of one event of a stream, Yap reads.
pub const MAX_BODY: usize = 1024 * 1024;
/// The most of an error's body read (for a JSON-RPC error in it).
const MAX_ERROR_BODY: usize = 64 * 1024;
/// Same-origin redirects followed, at most.
const MAX_REDIRECTS: usize = 5;
/// No connection in this long: unreachable.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_HEADER: &str = "Mcp-Session-Id";
const LAST_EVENT_HEADER: &str = "Last-Event-Id";
const EVENTS: &str = "text/event-stream";
const JSON: &str = "application/json";

/// Cloud metadata services: never a connector, whoever typed them.
pub const METADATA: &[&str] =&["169.254.169.254", "metadata.google.internal", "metadata", "[fd00:ec2::254]", "100.100.100.200"];

/// Why a request to a connector's server failed, in plain words
/// ([`std::fmt::Display`]); the host only, never a path or a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// No connection, or it broke.
    Unreachable { host: String },
    /// Waited too long for an answer.
    TimedOut { host: String },
    /// A redirect to another server, refused.
    OffHost { host: String, to: String },
    /// A name on the internet that points at this PC or the network.
    Private { host: String },
    /// An answer bigger than [`MAX_BODY`].
    TooBig { host: String },
    /// A 401 to the token or header Yap sent; a 403 without a scope to ask
    /// for.
    TokenRefused { host: String, status: u16 },
    /// The first POST answered 404 or 405: not Streamable HTTP here.
    NotStreamable { host: String, status: u16 },
    /// Any other status Yap can't use.
    Status { host: String, status: u16 },
    /// A same-origin redirect that would turn the POST into a GET.
    Redirected { host: String },
    /// A browser sign-in that can't be used any more.
    SignInEnded { host: String },
    /// Something else, said as it is (Yap's own words).
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Unreachable { host } => write!(f, "Couldn't reach {host}: is the address right, and is this PC online?"),
            HttpError::TimedOut { host } => write!(f, "No answer from {host} in time."),
            HttpError::OffHost { host, to } => write!(f, "{host} sent Yap on to {to}. Yap only talks to the server you added, so it stopped there."),
            HttpError::Private { host } => write!(f, "{host} points at an address on this PC or your network, which Yap only connects to when you type that address yourself."),
            HttpError::TooBig { host } => write!(f, "{host} sent more than 1 MB at once, more than Yap reads."),
            HttpError::TokenRefused { host, status } => write!(f, "{host} turned the token down ({status}). Make a new one, then paste it in again."),
            HttpError::NotStreamable { host, status } => write!(f, "{host} doesn't speak MCP at that address ({status})."),
            HttpError::Status { host, status } => write!(f, "{host} answered HTTP {status}."),
            HttpError::Redirected { host } => write!(f, "{host} redirected the request in a way MCP can't follow."),
            HttpError::SignInEnded { host } => write!(f, "Your sign-in to {host} has ended. Sign in again."),
            HttpError::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for HttpError {}

type Result<T> = std::result::Result<T, StreamableHttpError<HttpError>>;

/// A link Yap may connect to ([module docs](self)), and where its host is.
/// Pure.
pub fn check_url(raw: &str) -> std::result::Result<(Url, Place), String> {
    let url = Url::parse(raw.trim()).map_err(|_| "That isn't a web address.".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Put a token in the token field, not in the address.".into());
    }
    let host = url.host_str().ok_or_else(|| "That isn't a web address.".to_string())?.to_ascii_lowercase();
    if METADATA.contains(&host.as_str()) {
        return Err("That's a cloud metadata address, which Yap never connects to.".into());
    }
    let place = net::place_of(&host);
    match url.scheme() {
        "https" => {}
        "http" if place != Place::Internet => {}
        "http" => return Err("A server on the internet needs an https:// address.".into()),
        _ => return Err("A connector's address starts with https:// (or http:// on this PC or your network).".into()),
    }
    Ok((url, place))
}

/// The resolver for a link to a name on the internet: its addresses all
/// public, else refused ([module docs](self)).
struct Guard;

/// A name that resolved to an address on this PC or the network.
#[derive(Debug)]
struct PrivateAnswer;

impl std::fmt::Display for PrivateAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the name points at a private address")
    }
}

impl std::error::Error for PrivateAnswer {}

/// Test stand-ins for DNS: a name → this PC's fake server.
#[cfg(test)]
static TEST_DNS: std::sync::Mutex<Vec<(String, SocketAddr)>> = std::sync::Mutex::new(Vec::new());

/// Every address `addrs` holds is public (none on this PC or a network). Pure.
pub fn all_public(addrs: &[IpAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|ip| crate::web::is_public(*ip))
}

impl reqwest::dns::Resolve for Guard {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            #[cfg(test)]
            let stand_in: Option<SocketAddr> = TEST_DNS.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(h, _)| *h == host).map(|(_, a)| *a);
            #[cfg(not(test))]
            let stand_in: Option<SocketAddr> = None;
            let addrs: Vec<SocketAddr> = match stand_in {
                Some(a) => vec![a],
                None => tokio::net::lookup_host((host.as_str(), 0)).await?.collect(),
            };
            let ips: Vec<IpAddr> = addrs.iter().map(SocketAddr::ip).collect();
            if !all_public(&ips) {
                return Err(Box::new(PrivateAnswer) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A redirect the policy refused: where it went.
#[derive(Debug)]
struct OffHost(String);

impl std::fmt::Display for OffHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a redirect to {}", self.0)
    }
}

impl std::error::Error for OffHost {}

#[derive(Clone)]
struct Inner {
    client: reqwest::Client,
    host: String,
    /// The receipts' word for the connector ("A connector").
    kind: String,
    token: Option<String>,
    headers: Vec<(HeaderName, HeaderValue)>,
    /// A browser sign-in's tokens ([`Http::with_sign_in`]).
    sign_in: Option<Arc<super::oauth::Source>>,
}

/// A connector's HTTP side: Yap's reqwest, its rules, its secrets.
#[derive(Clone)]
pub struct Http {
    inner: Arc<Inner>,
}

/// A client for a connector whose server is at `place`: names on the
/// internet resolved by the guard (never to this PC or the network), this
/// PC and the network reached straight (no proxy); Yap's user agent and
/// connect timeout. The sign-in's requests use it too (`oauth/tokens.rs`).
pub fn client_builder(place: Place) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).user_agent(concat!("Yap/", env!("CARGO_PKG_VERSION")));
    if place == Place::Internet {
        builder.dns_resolver(Arc::new(Guard))
    } else {
        // This PC or the network: straight there, never through a proxy.
        builder.no_proxy()
    }
}

/// Whether reqwest's error `e` came from the guard: a name on the internet
/// that resolved to this PC or the network.
pub fn was_private(e: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(s) = source {
        if s.downcast_ref::<PrivateAnswer>().is_some() {
            return true;
        }
        source = s.source();
    }
    false
}

impl Http {
    /// For the link `url` (checked by [`check_url`]; `place` where its host
    /// is), recorded as `kind`, sending `token` and `headers` (name, value).
    pub fn new(url: &Url, place: Place, kind: String, token: Option<String>, headers: &[(String, String)]) -> std::result::Result<Http, String> {
        let origin = url.origin();
        let host = net::host_of(url);
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error(OffHost("too many redirects".into()));
            }
            if attempt.url().origin() != origin {
                let to = attempt.url().host_str().unwrap_or("another address").to_string();
                return attempt.error(OffHost(to));
            }
            if matches!(attempt.status().as_u16(), 307 | 308) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let client = client_builder(place).redirect(policy).build().map_err(|_| "Yap couldn't set up the connection.".to_string())?;
        let mut parsed = Vec::new();
        for (name, value) in headers {
            let n = HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| format!("{name} isn't a header name."))?;
            if [ACCEPT, CONTENT_TYPE].contains(&n) || n.as_str().eq_ignore_ascii_case(SESSION_HEADER) || n.as_str().to_ascii_lowercase().starts_with("mcp-") {
                return Err(format!("{name} is a header MCP sets itself."));
            }
            let mut v = HeaderValue::from_str(value.trim()).map_err(|_| format!("The value for {name} has characters a header can't carry."))?;
            v.set_sensitive(true);
            parsed.push((n, v));
        }
        Ok(Http { inner: Arc::new(Inner { client, host, kind, token: token.filter(|t| !t.trim().is_empty()), headers: parsed, sign_in: None }) })
    }

    /// The same, sending a browser sign-in's access token ([module
    /// docs](self)) instead of a pasted one.
    pub fn with_sign_in(mut self, source: Arc<super::oauth::Source>) -> Http {
        let inner = Arc::make_mut(&mut self.inner);
        inner.token = None;
        inner.sign_in = Some(source);
        self
    }

    /// Whether a pasted token or a header is sent (a 401 then means it was
    /// turned down, never a browser sign-in).
    pub fn sends_credentials(&self) -> bool {
        self.inner.token.is_some() || !self.inner.headers.is_empty()
    }

    pub fn host(&self) -> &str {
        &self.inner.host
    }

    fn purpose(&self) -> net::Purpose {
        net::Purpose::Connector(self.inner.kind.clone())
    }

    /// The bearer token a request carries: rmcp's own `auth` if it gives one,
    /// else the sign-in's (refreshed first when it's nearly out), else the
    /// pasted one. A sign-in with no usable tokens: rmcp's `AuthRequired`
    /// (the manager says "Needs sign-in").
    async fn bearer(&self, auth: Option<String>) -> Result<Option<String>> {
        if auth.is_some() {
            return Ok(auth);
        }
        match &self.inner.sign_in {
            Some(source) => source.bearer().await.map(Some).map_err(|t| self.trouble(t)),
            None => Ok(self.inner.token.clone()),
        }
    }

    /// A sign-in's trouble as rmcp's error.
    fn trouble(&self, t: super::oauth::Trouble) -> StreamableHttpError<HttpError> {
        match t {
            super::oauth::Trouble::SignIn => StreamableHttpError::AuthRequired(AuthRequiredError::new("Bearer".into())),
            super::oauth::Trouble::Failed(why) => StreamableHttpError::Client(HttpError::Other(why)),
        }
    }

    /// The token and the secret headers on `req`.
    fn credentials(&self, mut req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
        if let Some(token) = token {
            if let Ok(mut v) = HeaderValue::from_str(&format!("Bearer {}", token.trim())) {
                v.set_sensitive(true);
                req = req.header(reqwest::header::AUTHORIZATION, v);
            }
        }
        for (n, v) in &self.inner.headers {
            req = req.header(n.clone(), v.clone());
        }
        req
    }

    /// `make(token)` sent; a 401 to a sign-in's token refreshes it once and
    /// sends again ([module docs](self)).
    async fn send_signed(&self, auth: Option<String>, make: impl Fn(Option<&str>) -> Result<reqwest::RequestBuilder>) -> Result<reqwest::Response> {
        let token = self.bearer(auth).await?;
        let resp = self.send(make(token.as_deref())?).await.map_err(StreamableHttpError::Client)?;
        let (Some(source), Some(used)) = (&self.inner.sign_in, token.as_deref()) else { return Ok(resp) };
        if resp.status().as_u16() != 401 {
            return Ok(resp);
        }
        let fresh = source.refused(used).await.map_err(|t| self.trouble(t))?;
        let again = self.send(make(Some(&fresh))?).await.map_err(StreamableHttpError::Client)?;
        if again.status().as_u16() == 401 {
            source.ended().await;
        }
        Ok(again)
    }

    /// rmcp's protocol headers (`MCP-Protocol-Version`, `Mcp-Method`…).
    fn protocol(&self, mut req: reqwest::RequestBuilder, headers: HashMap<HeaderName, HeaderValue>) -> Result<reqwest::RequestBuilder> {
        for (name, value) in headers {
            let lower = name.as_str().to_ascii_lowercase();
            if lower == "accept" || lower == SESSION_HEADER.to_ascii_lowercase() || lower == LAST_EVENT_HEADER.to_ascii_lowercase() {
                return Err(StreamableHttpError::ReservedHeaderConflict(name.to_string()));
            }
            req = req.header(name, value);
        }
        Ok(req)
    }

    /// A request's send, through the receipts.
    async fn send(&self, req: reqwest::RequestBuilder) -> std::result::Result<reqwest::Response, HttpError> {
        req.send_for(self.purpose()).await.map_err(|e| self.net_error(&e))
    }

    /// A reqwest error in plain words.
    fn net_error(&self, e: &reqwest::Error) -> HttpError {
        let host = self.inner.host.clone();
        let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
        while let Some(s) = source {
            if let Some(OffHost(to)) = s.downcast_ref::<OffHost>() {
                return HttpError::OffHost { host, to: to.clone() };
            }
            if s.downcast_ref::<PrivateAnswer>().is_some() {
                return HttpError::Private { host };
            }
            source = s.source();
        }
        if e.is_timeout() {
            HttpError::TimedOut { host }
        } else {
            HttpError::Unreachable { host }
        }
    }

    /// A 401 or 403 as rmcp's errors (a challenge without credentials: a
    /// browser sign-in; else the token turned down). `None`: neither.
    fn refused(&self, status: u16, headers: &HeaderMap) -> Option<StreamableHttpError<HttpError>> {
        let challenge = headers.get(WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).map(str::to_string);
        match status {
            401 if self.sends_credentials() => Some(StreamableHttpError::Client(HttpError::TokenRefused { host: self.inner.host.clone(), status })),
            // A sign-in's token refused after its refresh: sign in again.
            401 if self.inner.sign_in.is_some() => Some(StreamableHttpError::AuthRequired(AuthRequiredError::new(challenge.unwrap_or_else(|| "Bearer".into())))),
            401 => Some(match challenge {
                Some(c) => StreamableHttpError::AuthRequired(AuthRequiredError::new(c)),
                None => StreamableHttpError::Client(HttpError::TokenRefused { host: self.inner.host.clone(), status }),
            }),
            403 => Some(match challenge.filter(|c| c.to_ascii_lowercase().contains("insufficient_scope")) {
                Some(c) => {
                    let scope = scope_of(&c);
                    if let Some(source) = &self.inner.sign_in {
                        source.step_up(scope.as_deref());
                    }
                    StreamableHttpError::InsufficientScope(InsufficientScopeError::new(c, scope))
                }
                None => StreamableHttpError::Client(HttpError::TokenRefused { host: self.inner.host.clone(), status }),
            }),
            _ => None,
        }
    }

    /// An answer's body, at most `max` bytes (more: [`HttpError::TooBig`]).
    async fn read(&self, mut resp: reqwest::Response, max: usize) -> std::result::Result<Vec<u8>, HttpError> {
        if resp.content_length().is_some_and(|n| n > max as u64) {
            return Err(HttpError::TooBig { host: self.inner.host.clone() });
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| self.net_error(&e))? {
            if body.len() + chunk.len() > max {
                return Err(HttpError::TooBig { host: self.inner.host.clone() });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// An answer's stream of events, each at most `max` bytes.
    fn events(&self, resp: reqwest::Response, max: usize) -> BoxStream<'static, std::result::Result<Sse, SseError>> {
        let host = self.inner.host.clone();
        let mut limit = Limit::new(max);
        let bytes = resp.bytes_stream().map(move |chunk| match chunk {
            Ok(b) => match limit.observe(&b) {
                Ok(()) => Ok(b),
                Err(()) => Err(StreamFailed(HttpError::TooBig { host: host.clone() })),
            },
            Err(_) => Err(StreamFailed(HttpError::Unreachable { host: host.clone() })),
        });
        SseStream::from_bytes_stream(bytes).boxed()
    }

    /// The `GET` that opens a stream (Streamable HTTP's server channel, or
    /// the older transport's whole conversation).
    async fn open_stream(
        &self,
        uri: &str,
        session: Option<&str>,
        last_event: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        max: usize,
    ) -> Result<BoxStream<'static, std::result::Result<Sse, SseError>>> {
        let resp = self
            .send_signed(auth, |token| {
                let mut req = self.inner.client.get(uri).header(ACCEPT, format!("{EVENTS}, {JSON}"));
                if let Some(s) = session {
                    req = req.header(SESSION_HEADER, s);
                }
                if let Some(id) = &last_event {
                    req = req.header(LAST_EVENT_HEADER, id.clone());
                }
                req = self.credentials(req, token);
                self.protocol(req, headers.clone())
            })
            .await?;
        let status = resp.status().as_u16();
        if status == 405 {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        if let Some(e) = self.refused(status, resp.headers()) {
            return Err(e);
        }
        if status == 429 || status >= 500 {
            return Err(StreamableHttpError::HttpStatus(HttpStatusError::new(status, retry_after(resp.headers()))));
        }
        if !resp.status().is_success() {
            return Err(StreamableHttpError::Client(HttpError::Status { host: self.inner.host.clone(), status }));
        }
        let kind = content_type(resp.headers());
        if !kind.starts_with(EVENTS) && !kind.starts_with(JSON) {
            return Err(StreamableHttpError::UnexpectedContentType(Some(kind)));
        }
        Ok(self.events(resp, max))
    }
}

/// A broken or oversized stream, as the SSE parser carries it.
#[derive(Debug)]
struct StreamFailed(HttpError);

impl std::fmt::Display for StreamFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for StreamFailed {}

/// Bytes of the event being read: more than `max` before a blank line ends
/// it, and the stream stops.
struct Limit {
    max: usize,
    count: usize,
    newline: bool,
}

impl Limit {
    fn new(max: usize) -> Limit {
        Limit { max, count: 0, newline: false }
    }

    fn observe(&mut self, chunk: &[u8]) -> std::result::Result<(), ()> {
        for &b in chunk {
            match b {
                b'\r' => {}
                b'\n' => {
                    if self.newline {
                        self.count = 0;
                    }
                    self.newline = true;
                }
                _ => {
                    self.newline = false;
                    self.count += 1;
                    if self.count > self.max {
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }
}

fn content_type(headers: &HeaderMap) -> String {
    headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase()
}

fn retry_after(headers: &HeaderMap) -> Option<String> {
    headers.get(RETRY_AFTER).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// `scope="…"` of a `WWW-Authenticate` challenge. Pure.
fn scope_of(challenge: &str) -> Option<String> {
    let lower = challenge.to_ascii_lowercase();
    let at = lower.find("scope=")? + "scope=".len();
    let rest = &challenge[at..];
    match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().map(str::to_string),
        None => rest.split([',', ' ']).next().map(str::to_string),
    }
}

/// Is `message` a request (one that needs an answer)?
fn wants_answer(message: &ClientJsonRpcMessage) -> bool {
    matches!(message, JsonRpcMessage::Request(_))
}

/// A `server/discover` a 2025 server refused with a 4xx (not 401/403) and
/// no session: as a JSON-RPC error with the request's id, so rmcp's Auto
/// lifecycle falls back to `initialize` (rmcp's own client does the same).
fn refused_discover(message: &ClientJsonRpcMessage, session: bool, status: u16, body: &str) -> Option<StreamableHttpPostResponse> {
    if session || !(400..500).contains(&status) || status == 401 || status == 403 {
        return None;
    }
    let v = serde_json::to_value(message).ok()?;
    if v.get("method").and_then(|m| m.as_str()) != Some("server/discover") {
        return None;
    }
    let id = v.get("id")?.clone();
    let error = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|b| b.get("error").cloned())
        .filter(|e| e.get("code").is_some())
        .unwrap_or_else(|| serde_json::json!({ "code": -32600, "message": format!("server/discover answered HTTP {status}") }));
    let answer = serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error });
    serde_json::from_value::<ServerJsonRpcMessage>(answer).ok().map(|m| StreamableHttpPostResponse::Json(m, None))
}

impl StreamableHttpClient for Http {
    type Error = HttpError;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse> {
        self.post_message_with_max_sse_event_size(uri, message, session_id, auth, headers, MAX_BODY).await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse> {
        let max = max_sse_event_size.min(MAX_BODY);
        let body = serde_json::to_vec(&message)?;
        let session = session_id.is_some();
        let resp = self
            .send_signed(auth, |token| {
                let mut req = self.inner.client.post(uri.as_ref()).header(ACCEPT, format!("{EVENTS}, {JSON}")).header(CONTENT_TYPE, JSON);
                req = self.credentials(req, token);
                req = self.protocol(req, headers.clone())?;
                if let Some(s) = &session_id {
                    req = req.header(SESSION_HEADER, s.as_ref());
                }
                Ok(req.body(body.clone()))
            })
            .await?;
        let status = resp.status().as_u16();
        if let Some(e) = self.refused(status, resp.headers()) {
            return Err(e);
        }
        if status == 202 || status == 204 {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if status == 404 && session {
            return Err(StreamableHttpError::SessionExpired);
        }
        if resp.status().is_redirection() {
            return Err(StreamableHttpError::Client(HttpError::Redirected { host: self.inner.host.clone() }));
        }
        let kind = content_type(resp.headers());
        let new_session = resp.headers().get(SESSION_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
        if !resp.status().is_success() {
            let throttled = status == 429 || status >= 500;
            let retry = retry_after(resp.headers());
            let text = String::from_utf8_lossy(&self.read(resp, MAX_ERROR_BODY).await.unwrap_or_default()).into_owned();
            if let Some(answer) = refused_discover(&message, session, status, &text) {
                return Ok(answer);
            }
            if kind.starts_with(JSON) && status != 429 {
                if let Ok(m @ JsonRpcMessage::Error(_)) = serde_json::from_str::<ServerJsonRpcMessage>(&text) {
                    return Ok(StreamableHttpPostResponse::Json(m, new_session));
                }
            }
            if throttled {
                return Err(StreamableHttpError::HttpStatus(HttpStatusError::new(status, retry)));
            }
            let host = self.inner.host.clone();
            return Err(StreamableHttpError::Client(if (status == 404 || status == 405) && !session {
                HttpError::NotStreamable { host, status }
            } else {
                HttpError::Status { host, status }
            }));
        }
        if !wants_answer(&message) && resp.content_length() == Some(0) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if kind.starts_with(EVENTS) {
            return Ok(StreamableHttpPostResponse::Sse(self.events(resp, max), new_session));
        }
        if kind.starts_with(JSON) {
            let bytes = self.read(resp, MAX_BODY).await.map_err(StreamableHttpError::Client)?;
            return match serde_json::from_slice::<ServerJsonRpcMessage>(&bytes) {
                Ok(m) => Ok(StreamableHttpPostResponse::Json(m, new_session)),
                Err(_) if !wants_answer(&message) => Ok(StreamableHttpPostResponse::Accepted),
                Err(_) => Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(format!("{} sent an answer Yap couldn't read.", self.inner.host)))),
            };
        }
        if !wants_answer(&message) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        Err(StreamableHttpError::UnexpectedContentType(Some(kind)))
    }

    async fn delete_session(&self, uri: Arc<str>, session: Arc<str>, auth: Option<String>, headers: HashMap<HeaderName, HeaderValue>) -> Result<()> {
        let token = self.bearer(auth).await?;
        let mut req = self.inner.client.delete(uri.as_ref()).header(SESSION_HEADER, session.as_ref());
        req = self.credentials(req, token.as_deref());
        req = self.protocol(req, headers)?;
        let resp = self.send(req).await.map_err(StreamableHttpError::Client)?;
        let status = resp.status().as_u16();
        if status == 405 || resp.status().is_success() {
            return Ok(());
        }
        Err(StreamableHttpError::Client(HttpError::Status { host: self.inner.host.clone(), status }))
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, std::result::Result<Sse, SseError>>> {
        self.open_stream(&uri, session_id.as_deref(), last_event_id, auth, headers, MAX_BODY).await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxStream<'static, std::result::Result<Sse, SseError>>> {
        self.open_stream(&uri, session_id.as_deref(), last_event_id, auth, headers, max_sse_event_size.min(MAX_BODY)).await
    }
}

/// rmcp's Streamable HTTP transport for `url` on `http`.
pub fn transport(http: Http, url: &Url) -> rmcp::transport::StreamableHttpClientTransport<Http> {
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
    let config = StreamableHttpClientTransportConfig::with_uri(url.as_str()).max_sse_event_size(MAX_BODY);
    rmcp::transport::StreamableHttpClientTransport::with_client(http, config)
}

/// The [`HttpError`] somewhere in `e`'s sources, if there is one.
pub fn http_error_in<'a>(e: &'a (dyn std::error::Error + 'static)) -> Option<&'a HttpError> {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(s) = source {
        if let Some(h) = s.downcast_ref::<HttpError>() {
            return Some(h);
        }
        if let Some(StreamFailed(h)) = s.downcast_ref::<StreamFailed>() {
            return Some(h);
        }
        if let Some(StreamableHttpError::Client(h)) = s.downcast_ref::<StreamableHttpError<HttpError>>() {
            return Some(h);
        }
        source = s.source();
    }
    None
}

#[cfg(test)]
pub fn stand_in_dns(name: &str, addr: SocketAddr) {
    TEST_DNS.lock().unwrap_or_else(|p| p.into_inner()).push((name.to_string(), addr));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_need_https_on_the_internet_and_never_a_metadata_address() {
        assert_eq!(check_url("https://mcp.example.com/mcp").unwrap().1, Place::Internet);
        assert_eq!(check_url("http://127.0.0.1:8123/api/mcp").unwrap().1, Place::Pc);
        assert_eq!(check_url("http://homeassistant.local:8123/api/mcp").unwrap().1, Place::Network);
        assert_eq!(check_url("http://192.168.1.20/mcp").unwrap().1, Place::Network);
        assert!(check_url("http://mcp.example.com/mcp").unwrap_err().contains("https"));
        assert!(check_url("ftp://mcp.example.com").is_err());
        assert!(check_url("https://user:pw@mcp.example.com/mcp").unwrap_err().contains("token field"));
        assert!(check_url("http://169.254.169.254/latest/meta-data").unwrap_err().contains("metadata"));
        assert!(check_url("http://metadata.google.internal/").unwrap_err().contains("metadata"));
        assert!(check_url("not a link").is_err());
    }

    #[test]
    fn a_name_on_the_internet_must_resolve_to_public_addresses_only() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(all_public(&[ip("93.184.215.14"), ip("2606:4700::1111")]));
        assert!(!all_public(&[ip("93.184.215.14"), ip("127.0.0.1")]), "one private address is enough to refuse");
        for private in ["10.0.0.5", "192.168.1.1", "169.254.169.254", "::1", "fd00::1", "100.64.0.1", "0.0.0.0"] {
            assert!(!all_public(&[ip(private)]), "{private}");
        }
        assert!(!all_public(&[]));
    }

    #[test]
    fn a_scope_is_read_from_its_challenge_and_headers_are_checked() {
        assert_eq!(scope_of(r#"Bearer error="insufficient_scope", scope="files:read files:write""#).as_deref(), Some("files:read files:write"));
        assert_eq!(scope_of("Bearer scope=read:data, error=x").as_deref(), Some("read:data"));
        assert_eq!(scope_of("Bearer realm=x"), None);
        let (url, place) = check_url("https://mcp.example.com/mcp").unwrap();
        assert!(Http::new(&url, place, "A connector".into(), None, &[("Mcp-Session-Id".into(), "x".into())]).is_err());
        assert!(Http::new(&url, place, "A connector".into(), None, &[("Bad Header".into(), "x".into())]).is_err());
        assert!(Http::new(&url, place, "A connector".into(), None, &[("X-Api-Key".into(), "a\nb".into())]).is_err());
        let h = Http::new(&url, place, "A connector".into(), Some(" ".into()), &[]).unwrap();
        assert!(!h.sends_credentials(), "a blank token isn't one");
        let h = Http::new(&url, place, "A connector".into(), None, &[("X-Api-Key".into(), "k".into())]).unwrap();
        assert!(h.sends_credentials());
        assert_eq!(h.host(), "mcp.example.com");
    }

    #[test]
    fn an_events_limit_counts_one_event_at_a_time() {
        let mut l = Limit::new(12);
        assert!(l.observe(b"data: 12345\n\n").is_ok());
        assert!(l.observe(b"data: 12345\r\n\r\ndata: 1").is_ok(), "each event counted on its own");
        assert!(l.observe(b"234567890123").is_err());
    }

    #[test]
    fn errors_name_the_host_and_never_a_path() {
        let e = HttpError::OffHost { host: "127.0.0.1".into(), to: "localhost".into() };
        assert_eq!(e.to_string(), "127.0.0.1 sent Yap on to localhost. Yap only talks to the server you added, so it stopped there.");
        assert!(HttpError::TokenRefused { host: "api.example.com".into(), status: 401 }.to_string().contains("turned the token down"));
    }
}
