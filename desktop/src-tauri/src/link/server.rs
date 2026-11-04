//! **Yap Link's HTTPS server**: TCP port 51443 on every interface, while
//! Settings → Phone's switch is on. Each connection:
//! 1. **Who it's from**: a peer that isn't on a private, link-local or
//!    loopback address is dropped before a byte is read
//!    (`lan::allowed_peer`), and so is one that came in on a network Windows
//!    marks **Public** (`lan::public_network`).
//! 2. **TLS** (rustls, Yap's self-signed certificate, `cert.rs`).
//! 3. **One HTTP request at a time** (kept alive a few seconds for the
//!    page's next file): `GET /…` serves the page (`page.rs`), with its
//!    security headers; `GET /ws` with an upgrade becomes the WebSocket
//!    (`session.rs`) — only from the page itself (its `Origin` is this
//!    host's) and only for a host that's an address or a local name (not a
//!    rebound domain). Anything else: 400, 403, 404 or 405.
//!
//! Every connection is in the network receipts (`net::phone`): the phone's
//! address, its status, bytes each way.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::link::{lan, page, Hub};

/// The longest a request's head may be.
const MAX_HEAD: usize = 8 * 1024;
/// How long a phone has for TLS and its request, and to send the next one.
const HEAD_WAIT: Duration = Duration::from_secs(10);
const KEEP_ALIVE: Duration = Duration::from_secs(5);
/// Requests on one connection at most.
const MAX_REQUESTS: usize = 64;

/// An HTTP request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// A header's value (the first; names ignore case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// A comma-separated header has `token` (ignoring case).
    fn has_token(&self, name: &str, token: &str) -> bool {
        self.header(name).is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
    }

    /// The phone asked to keep the connection.
    fn keep_alive(&self) -> bool {
        !self.has_token("connection", "close")
    }
}

/// Parse a request head (up to and without its blank line). Pure.
pub fn parse_head(text: &str) -> Option<Head> {
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let version = first.next()?;
    if !version.starts_with("HTTP/1.") || method.is_empty() || !path.starts_with('/') {
        return None;
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    Some(Head { method, path, headers })
}

/// **Is `host` (a request's `Host`) one Yap answers to?** An IP address
/// (with its port), `localhost`, a `.local` name or this PC's own name: the
/// ways a phone at home reaches it. A name from the internet that resolves
/// here (DNS rebinding) is refused. Pure.
pub fn good_host(host: &str, pc: &str) -> bool {
    if host.is_empty() || host.len() > 255 || !host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c)) {
        return false;
    }
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [v6]:port
        match rest.split_once(']') {
            Some((v6, _)) => return v6.parse::<std::net::Ipv6Addr>().is_ok(),
            None => return false,
        }
    } else {
        host.rsplit_once(':').map_or(host, |(h, port)| if port.chars().all(|c| c.is_ascii_digit()) { h } else { host })
    };
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name.parse::<IpAddr>().is_ok() || name == "localhost" || name.ends_with(".local") || (!pc.is_empty() && name == pc.to_ascii_lowercase())
}

/// The WebSocket handshake's `Sec-WebSocket-Accept` for `key`. Pure.
pub fn accept_key(key: &str) -> String {
    use base64::Engine;
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

/// What a request gets. Pure: the decision, not the writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The WebSocket.
    Socket { accept: String },
    /// A file of the page.
    File { path: String, head_only: bool },
    Refuse { status: u16, reason: &'static str },
}

/// Where a request goes. `pc`: this PC's name (a good host).
pub fn route(head: &Head, pc: &str) -> Route {
    let host = head.header("host").unwrap_or("");
    if !good_host(host, pc) {
        return Route::Refuse { status: 403, reason: "Forbidden" };
    }
    let path = head.path.split(['?', '#']).next().unwrap_or("/");
    if path == "/ws" {
        if head.method != "GET" {
            return Route::Refuse { status: 405, reason: "Method Not Allowed" };
        }
        let upgrade = head.has_token("upgrade", "websocket") && head.has_token("connection", "upgrade");
        let key = head.header("sec-websocket-key").unwrap_or("");
        if !upgrade || head.header("sec-websocket-version") != Some("13") || key.is_empty() {
            return Route::Refuse { status: 400, reason: "Bad Request" };
        }
        // Only the page itself: a page from another site may open a
        // WebSocket anywhere, so its Origin must be this host's.
        let origin = head.header("origin").unwrap_or("");
        if !origin.eq_ignore_ascii_case(&format!("https://{host}")) {
            return Route::Refuse { status: 403, reason: "Forbidden" };
        }
        return Route::Socket { accept: accept_key(key) };
    }
    match head.method.as_str() {
        "GET" => Route::File { path: path.to_string(), head_only: false },
        "HEAD" => Route::File { path: path.to_string(), head_only: true },
        _ => Route::Refuse { status: 405, reason: "Method Not Allowed" },
    }
}

/// A response's bytes. `extra`: headers beyond the usual.
fn response(status: u16, reason: &str, extra: &[(&str, String)], body: &[u8], head_only: bool, keep: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n", body.len());
    for (n, v) in extra {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str("X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-cache\r\n");
    out.push_str(if keep { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
    let mut bytes = out.into_bytes();
    if !head_only {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Read a request's head into `buf` (which may hold some already): its
/// length with the blank line, or `None` when the connection ended, took
/// too long or sent too much.
async fn read_head<S: AsyncRead + Unpin>(s: &mut S, buf: &mut Vec<u8>, wait: Duration) -> Option<usize> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some(at + 4);
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
        let mut chunk = [0u8; 2048];
        let n = tokio::time::timeout_at(deadline, s.read(&mut chunk)).await.ok()?.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Accept connections on `listener` until `stop` says so.
pub async fn serve(listener: tokio::net::TcpListener, hub: Arc<Hub>, mut stop: tokio::sync::watch::Receiver<bool>) {
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::clone(&hub.tls));
    loop {
        let accepted = tokio::select! {
            a = listener.accept() => a,
            _ = stop.changed() => break,
        };
        let (tcp, peer) = match accepted {
            Ok(a) => a,
            Err(e) => {
                tracing::debug!("Yap Link: a connection failed to arrive ({e})");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        if !lan::allowed_peer(peer.ip()) {
            // Never a byte to an address off the home network.
            tracing::info!("Yap Link: refused a connection from outside the home network");
            drop(tcp);
            continue;
        }
        let hub = Arc::clone(&hub);
        let acceptor = acceptor.clone();
        let stop = stop.clone();
        tokio::spawn(async move { connection(tcp, peer, acceptor, hub, stop).await });
    }
    tracing::info!("Yap Link: stopped listening");
}

async fn connection(
    tcp: tokio::net::TcpStream,
    peer: SocketAddr,
    acceptor: tokio_rustls::TlsAcceptor,
    hub: Arc<Hub>,
    stop: tokio::sync::watch::Receiver<bool>,
) {
    // A network Windows calls Public (a café's): no phones, whatever the address.
    if let Ok(local) = tcp.local_addr() {
        let ip = local.ip();
        let public = tokio::task::spawn_blocking(move || lan::public_network(ip)).await.ok().flatten();
        if public == Some(true) {
            tracing::info!("Yap Link: refused a phone on a network Windows marks Public");
            return;
        }
    }
    let _ = tcp.set_nodelay(true);
    let Ok(Ok(mut tls)) = tokio::time::timeout(HEAD_WAIT, acceptor.accept(tcp)).await else {
        // Most often the phone's first visit: Chrome's "not private" page.
        tracing::debug!("Yap Link: a TLS handshake didn't finish");
        return;
    };
    let pc = lan::pc_name();
    let mut buf: Vec<u8> = Vec::new();
    let (mut received, mut sent) = (0u64, 0u64);
    let mut status = 0u16;
    for n in 0..MAX_REQUESTS {
        let wait = if n == 0 { HEAD_WAIT } else { KEEP_ALIVE };
        let Some(end) = read_head(&mut tls, &mut buf, wait).await else { break };
        received += end as u64;
        let text = String::from_utf8_lossy(&buf[..end - 4]).into_owned();
        let rest = buf.split_off(end);
        buf = rest;
        let Some(head) = parse_head(&text) else {
            let r = response(400, "Bad Request", &[], b"", false, false);
            sent += r.len() as u64;
            let _ = tls.write_all(&r).await;
            status = 400;
            break;
        };
        match route(&head, &pc) {
            Route::Socket { accept } => {
                let r = format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
                if tls.write_all(r.as_bytes()).await.is_err() {
                    break;
                }
                let _ = tls.flush().await;
                sent += r.len() as u64;
                // The receipt for the page's files so far, then the conversation's own.
                if status != 0 {
                    crate::privacy::net::phone(peer.ip(), status, received, sent);
                }
                crate::link::session::run(tls, std::mem::take(&mut buf), peer, hub, stop).await;
                return;
            }
            Route::File { path, head_only } => {
                let keep = head.keep_alive();
                let host = head.header("host").unwrap_or("").to_string();
                let r = match page::file(&path) {
                    Some((body, kind)) => {
                        status = 200;
                        let mut extra = vec![("Content-Type", kind.to_string())];
                        if kind.starts_with("text/html") {
                            extra.push(("Content-Security-Policy", page::csp(&host)));
                            extra.push(("Permissions-Policy", "microphone=(self), camera=(), geolocation=()".to_string()));
                            extra.push(("Cross-Origin-Opener-Policy", "same-origin".to_string()));
                        }
                        response(200, "OK", &extra, body, head_only, keep)
                    }
                    None => {
                        status = 404;
                        response(404, "Not Found", &[("Content-Type", "text/plain; charset=utf-8".to_string())], b"Not found", head_only, keep)
                    }
                };
                sent += r.len() as u64;
                if tls.write_all(&r).await.is_err() {
                    break;
                }
                let _ = tls.flush().await;
                if !keep {
                    break;
                }
            }
            Route::Refuse { status: s, reason } => {
                status = s;
                let r = response(s, reason, &[("Content-Type", "text/plain; charset=utf-8".to_string())], reason.as_bytes(), false, false);
                sent += r.len() as u64;
                let _ = tls.write_all(&r).await;
                tracing::info!(status = s, "Yap Link: refused a request");
                break;
            }
        }
    }
    let _ = tls.shutdown().await;
    if status != 0 {
        crate::privacy::net::phone(peer.ip(), status, received, sent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(lines: &[&str]) -> Head {
        parse_head(&lines.join("\r\n")).unwrap()
    }

    #[test]
    fn requests_parse_and_go_where_they_should() {
        let h = head(&["GET /app.js?v=2 HTTP/1.1", "Host: 192.168.1.20:51443", "Connection: keep-alive"]);
        assert_eq!(h.method, "GET");
        assert_eq!(h.path, "/app.js?v=2");
        assert_eq!(h.header("HOST"), Some("192.168.1.20:51443"));
        assert!(h.keep_alive());
        assert_eq!(route(&h, "DESK"), Route::File { path: "/app.js".into(), head_only: false });
        assert!(parse_head("GET").is_none());
        assert!(parse_head("GET http://evil/ HTTP/1.1").is_none());
        assert!(parse_head("GET / SPDY/3").is_none());
        let post = head(&["POST / HTTP/1.1", "Host: 192.168.1.20:51443"]);
        assert_eq!(route(&post, "DESK"), Route::Refuse { status: 405, reason: "Method Not Allowed" });
    }

    #[test]
    fn the_socket_opens_only_for_the_page_itself() {
        let ws = |origin: &str, host: &str| {
            head(&[
                "GET /ws HTTP/1.1",
                &format!("Host: {host}"),
                "Upgrade: websocket",
                "Connection: keep-alive, Upgrade",
                "Sec-WebSocket-Version: 13",
                "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
                &format!("Origin: {origin}"),
            ])
        };
        // RFC 6455's own example key and answer.
        assert_eq!(
            route(&ws("https://192.168.1.20:51443", "192.168.1.20:51443"), "DESK"),
            Route::Socket { accept: "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".into() }
        );
        // Another site's page: refused.
        assert_eq!(route(&ws("https://evil.example", "192.168.1.20:51443"), "DESK"), Route::Refuse { status: 403, reason: "Forbidden" });
        assert_eq!(route(&ws("http://192.168.1.20:51443", "192.168.1.20:51443"), "DESK"), Route::Refuse { status: 403, reason: "Forbidden" });
        // A rebound name: refused before anything else.
        assert_eq!(route(&ws("https://evil.example", "evil.example"), "DESK"), Route::Refuse { status: 403, reason: "Forbidden" });
        // Not an upgrade.
        let plain = head(&["GET /ws HTTP/1.1", "Host: 192.168.1.20:51443"]);
        assert_eq!(route(&plain, "DESK"), Route::Refuse { status: 400, reason: "Bad Request" });
    }

    #[test]
    fn only_home_names_are_answered() {
        for ok in ["192.168.1.20", "192.168.1.20:51443", "[fd00::20]:51443", "localhost:51443", "desk.local", "DESK.local:51443", "desk", "127.0.0.1:0"] {
            assert!(good_host(ok, "desk"), "{ok}");
        }
        for no in ["", "evil.example", "evil.example:51443", "192.168.1.20.nip.io", "[nope]:1", "a b", "desk.example"] {
            assert!(!good_host(no, "desk"), "{no}");
        }
    }

    #[test]
    fn responses_carry_their_length_and_safety_headers() {
        let r = String::from_utf8(response(200, "OK", &[("Content-Type", "text/plain".into())], b"hello", false, true)).unwrap();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n"));
        assert!(r.contains("X-Content-Type-Options: nosniff"));
        assert!(r.contains("Connection: keep-alive"));
        assert!(r.ends_with("\r\n\r\nhello"));
        let h = String::from_utf8(response(200, "OK", &[], b"hello", true, false)).unwrap();
        assert!(h.ends_with("Connection: close\r\n\r\n"), "HEAD: no body");
    }
}
