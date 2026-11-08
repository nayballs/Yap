//! **The browser's part of a sign-in**: the authorize address
//! ([`authorize_url`]: PKCE with S256, `state`, the resource indicator,
//! the scopes) and the loopback listener the browser comes back to
//! ([`listen`]), after the ChatGPT sign-in's (`ai/chatgpt/oauth.rs`):
//!
//! - `127.0.0.1` only, on the fixed port [`PORT`] (51439, beside Vite's
//!   51437 and Link's 51443; registered in Yap's client documents), else any
//!   free port (registered too, for servers that allow any port on the
//!   loopback, RFC 8252); the path [`CALLBACK`].
//! - It answers only a `GET` from this PC, with this listener's `Host`, at
//!   the callback path, carrying this attempt's `state` once (and one code);
//!   anything else is a 404. It ends after one callback, a Cancel, or
//!   [`TTL`] (10 minutes), and closes the browser's connection each time.
//! - **`iss`** (RFC 9207, [`iss_ok`]): an answer naming another issuer, or
//!   none when the server says it always names itself, is refused before
//!   its code is used, and nothing from it is shown (not even its error).
//! - The page says how it went, in Yap's words ([`page`]).

use std::collections::HashMap;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use base64::Engine as _;
use sha2::{Digest, Sha256};
use tiny_http::Server;

/// The loopback's fixed port (Yap's client documents list it).
pub const PORT: u16 = 51439;
/// Where the browser comes back to.
pub const CALLBACK: &str = "/connectors/callback";
/// How long a sign-in waits for the browser.
pub const TTL: Duration = Duration::from_secs(10 * 60);

/// The redirect address on `port`. Pure.
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK}")
}

/// RFC 7636: 32 random bytes as the verifier, the S256 challenge of it.
pub fn pkce() -> Result<(String, String), String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    let challenge = challenge_of(&verifier);
    Ok((verifier, challenge))
}

/// The S256 challenge of `verifier` (base64url of its SHA-256). Pure.
pub fn challenge_of(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// What one authorize request carries.
pub struct Ask<'a> {
    pub authorize: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub challenge: &'a str,
    pub resource: &'a str,
    pub scopes: &'a [String],
}

/// The authorize address (the server's own query kept). Pure.
pub fn authorize_url(ask: &Ask) -> Result<String, String> {
    let mut url = url::Url::parse(ask.authorize).map_err(|_| "The sign-in server's address isn't one Yap can open.".to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", ask.client_id)
            .append_pair("redirect_uri", ask.redirect_uri)
            .append_pair("state", ask.state)
            .append_pair("code_challenge", ask.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", ask.resource);
        if !ask.scopes.is_empty() {
            q.append_pair("scope", &ask.scopes.join(" "));
        }
    }
    Ok(url.to_string())
}

/// Whether an answer's `iss` is the issuer's (RFC 9207): the same when
/// given; given when the server says it always is. Pure.
pub fn iss_ok(iss: Option<&str>, issuer: &str, supported: bool) -> bool {
    match iss {
        Some(i) => super::discover::same_issuer(i, issuer),
        None => !supported,
    }
}

/// What the browser brought back.
pub enum Handback {
    /// A code to exchange, with the `iss` it came with.
    Code { code: String, iss: Option<String>, reply: mpsc::Sender<Done> },
    /// The server said no: its error code, and the `iss` it came with.
    Denied { error: String, iss: Option<String>, reply: mpsc::Sender<Done> },
}

/// How it went, for the browser's page.
pub enum Done {
    /// Signed in to the connector (its name).
    SignedIn(String),
    /// Not signed in: why, in Yap's words.
    Failed(String),
}

/// A listener waiting for the browser. Dropping it stops it.
pub struct Listener {
    pub port: u16,
    pub redirect_uri: String,
    pub state: String,
    pub handback: mpsc::Receiver<Handback>,
    server: Arc<Server>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// The port a listener tries first: [`PORT`]. A unit test's listener takes
/// any free one instead (0), so `cargo test` never holds the fixed port a
/// running Yap's sign-in or the e2e suite's needs — unless the test asks for
/// the fixed one (`tests::the_real_listener_tries_the_fixed_port_first`,
/// the only one, which copes with it being busy).
fn first_port() -> u16 {
    #[cfg(test)]
    if !FIXED_IN_TEST.with(std::cell::Cell::get) {
        return 0;
    }
    PORT
}

#[cfg(test)]
thread_local! {
    /// This test thread's listener tries [`PORT`] first, as the app's does.
    static FIXED_IN_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Start listening: [`PORT`], else any free one.
pub fn listen() -> Result<Listener, String> {
    let server = Server::http(("127.0.0.1", first_port()))
        .or_else(|_| Server::http("127.0.0.1:0"))
        .map(Arc::new)
        .map_err(|_| "Yap couldn't start listening for the browser's sign-in.".to_string())?;
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("Yap couldn't start listening for the browser's sign-in.")?;
    let state = crate::auth::random_alnum(32)?;
    let (tx, rx) = mpsc::channel();
    let srv = server.clone();
    let host = format!("127.0.0.1:{port}");
    let st = state.clone();
    std::thread::Builder::new()
        .name("yap-connector-loopback".into())
        .spawn(move || {
            for request in srv.incoming_requests() {
                if handle(request, &host, &st, &tx) {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    tracing::info!(port, fixed = port == PORT, "connectors: waiting for the browser's sign-in on the loopback");
    Ok(Listener { port, redirect_uri: redirect_uri(port), state, handback: rx, server })
}

/// The callback's parameters, if they're this attempt's: one `state`,
/// matching; at most one code and one `iss`. Pure.
pub fn ours(query: &str, state: &str) -> Option<HashMap<String, String>> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let states: Vec<&String> = pairs.iter().filter(|(k, _)| k == "state").map(|(_, v)| v).collect();
    if states.len() != 1 || !crate::auth::same_secret(states[0], state) {
        return None;
    }
    for k in ["code", "iss", "error"] {
        if pairs.iter().filter(|(n, _)| n == k).count() > 1 {
            return None;
        }
    }
    Some(pairs.into_iter().collect())
}

/// One request; `true` once the sign-in's answer has come.
fn handle(request: tiny_http::Request, host: &str, state: &str, tx: &mpsc::Sender<Handback>) -> bool {
    let from_loopback = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
    let right_host = request.headers().iter().any(|h| h.field.equiv("Host") && h.value.as_str() == host);
    let (path, query) = request.url().split_once('?').map(|(p, q)| (p.to_string(), q.to_string())).unwrap_or((request.url().to_string(), String::new()));
    let get = *request.method() == tiny_http::Method::Get;
    let Some(params) = (from_loopback && right_host && get && path == CALLBACK).then(|| ours(&query, state)).flatten() else {
        reply(request, 404, None);
        return false;
    };
    let iss = params.get("iss").cloned();
    let (done_tx, done_rx) = mpsc::channel();
    let sent = match (params.get("error"), params.get("code").filter(|c| !c.is_empty())) {
        (Some(error), _) => tx.send(Handback::Denied { error: error.clone(), iss, reply: done_tx }),
        (None, Some(code)) => tx.send(Handback::Code { code: code.clone(), iss, reply: done_tx }),
        (None, None) => {
            reply(request, 400, None);
            return false;
        }
    };
    if sent.is_err() {
        reply(request, 200, Some(page(false, "Yap stopped waiting for this sign-in. Start again from Yap.")));
        return true;
    }
    let html = match done_rx.recv_timeout(Duration::from_secs(45)) {
        Ok(Done::SignedIn(name)) => page(true, &format!("You're signed in to {name}. You can close this tab and go back to Yap.")),
        Ok(Done::Failed(why)) => page(false, &why),
        Err(_) => page(true, "Yap is finishing the sign-in. You can close this tab."),
    };
    reply(request, 200, Some(html));
    true
}

/// The answer, and the connection closed (a browser keeps one open, and the
/// next attempt's callback would go down it to a listener that's gone: the
/// ChatGPT sign-in's lesson, 1 Nov 2026).
fn reply(request: tiny_http::Request, status: u16, html: Option<String>) {
    use std::io::Write as _;
    let body = html.unwrap_or_default();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut w = request.into_writer();
    let _ = w.write_all(head.as_bytes()).and_then(|_| w.write_all(body.as_bytes())).and_then(|_| w.flush());
}

/// The page the browser shows after the server's consent screen.
pub fn page(ok: bool, message: &str) -> String {
    let title = if ok { "Signed in" } else { "Not signed in" };
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Yap · {title}</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
         background:#f6f3ee;color:#26231c;font:16px/1.5 'Segoe UI',system-ui,sans-serif\">\
         <main style=\"max-width:440px;padding:32px;border-radius:18px;background:#fff;\
         box-shadow:0 8px 30px rgba(38,35,28,.08);text-align:center\">\
         <h1 style=\"margin:0 0 8px;font:600 26px Georgia,serif\">{title}</h1><p style=\"margin:0;color:#6b6457\">{}</p>\
         </main></body></html>",
        esc(message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_is_s256_of_a_fresh_verifier() {
        // RFC 7636, appendix B.
        assert_eq!(challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let (v, c) = pkce().unwrap();
        assert_eq!(v.len(), 43, "32 bytes, base64url without padding");
        assert!(v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(c, challenge_of(&v));
        assert_ne!(pkce().unwrap().0, v);
    }

    #[test]
    fn the_authorize_address_carries_pkce_state_the_resource_and_the_scopes() {
        let scopes = vec!["read".to_string(), "offline_access".to_string()];
        let ask = Ask {
            authorize: "https://auth.example.com/authorize?tenant=t1",
            client_id: "https://example.com/oauth/desktop-client.json",
            redirect_uri: "http://127.0.0.1:51439/connectors/callback",
            state: "s1",
            challenge: "c1",
            resource: "https://mcp.example.com/mcp",
            scopes: &scopes,
        };
        let url = url::Url::parse(&authorize_url(&ask).unwrap()).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["tenant"], "t1", "the server's own query kept");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "https://example.com/oauth/desktop-client.json");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:51439/connectors/callback");
        assert_eq!((q["code_challenge"].as_str(), q["code_challenge_method"].as_str()), ("c1", "S256"));
        assert_eq!((q["state"].as_str(), q["resource"].as_str(), q["scope"].as_str()), ("s1", "https://mcp.example.com/mcp", "read offline_access"));
        let none = Ask { scopes: &[], ..ask };
        assert!(!url::Url::parse(&authorize_url(&none).unwrap()).unwrap().query_pairs().any(|(k, _)| k == "scope"));
    }

    #[test]
    fn only_this_attempts_answer_from_its_own_issuer_is_taken() {
        assert!(ours("code=abc&state=s1&iss=https%3A%2F%2Fauth.example.com", "s1").is_some());
        assert!(ours("code=abc&state=s2", "s1").is_none());
        assert!(ours("code=abc", "s1").is_none());
        assert!(ours("code=abc&state=s1&state=s1", "s1").is_none());
        assert!(ours("code=a&code=b&state=s1", "s1").is_none());
        assert!(ours("code=a&iss=x&iss=y&state=s1", "s1").is_none());
        assert!(iss_ok(Some("https://auth.example.com/"), "https://auth.example.com", true));
        assert!(!iss_ok(Some("https://evil.example.com"), "https://auth.example.com", false), "another issuer, said or not");
        assert!(!iss_ok(None, "https://auth.example.com", true), "the server says it always names itself");
        assert!(iss_ok(None, "https://auth.example.com", false));
        assert_eq!(redirect_uri(PORT), "http://127.0.0.1:51439/connectors/callback");
        assert!(page(false, "<b>x</b>").contains("&lt;b&gt;x&lt;/b&gt;"));
    }

    use super::super::super::fake::raw_get as browser_get;

    #[test]
    fn the_listener_takes_one_answer_for_this_attempt_and_closes() {
        let l = listen().unwrap();
        assert!(l.redirect_uri.starts_with("http://127.0.0.1:") && l.redirect_uri.ends_with(CALLBACK));
        // Another site's page naming this PC under another name: refused.
        let wrong_host = browser_get(&format!("{}?code=c&state={}", l.redirect_uri, l.state), Some("evil.example.com"));
        assert!(wrong_host.starts_with("HTTP/1.1 404"), "{wrong_host}");
        let wrong_state = browser_get(&format!("{}?code=c&state=nope", l.redirect_uri), None);
        assert!(wrong_state.starts_with("HTTP/1.1 404"), "{wrong_state}");
        let back = format!("{}?code=c1&state={}&iss=https%3A%2F%2Fauth.example.com", l.redirect_uri, l.state);
        let browser = std::thread::spawn(move || browser_get(&back, None));
        match l.handback.recv_timeout(Duration::from_secs(5)).expect("the callback reached Yap") {
            Handback::Code { code, iss, reply } => {
                assert_eq!((code.as_str(), iss.as_deref()), ("c1", Some("https://auth.example.com")));
                reply.send(Done::SignedIn("Lights".into())).unwrap();
            }
            Handback::Denied { .. } => panic!("not denied"),
        }
        let page = browser.join().unwrap();
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("Connection: close"), "{page}");
        assert!(page.contains("You're signed in to Lights."), "{page}");
        let port = l.port;
        assert_ne!(port, PORT, "a unit test never holds the fixed port");
        drop(l);
        std::thread::sleep(Duration::from_millis(100));
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err(), "the listener is gone");
    }

    /// The app's listener: the fixed port first, else any free one. The only
    /// test that takes the fixed port, and only for a moment; something else
    /// holding it (a running Yap's sign-in, the e2e suite's) is the other
    /// half of the same rule, so it passes either way.
    #[test]
    fn the_real_listener_tries_the_fixed_port_first() {
        let was_free = std::net::TcpListener::bind(("127.0.0.1", PORT)).map(drop).is_ok();
        FIXED_IN_TEST.with(|f| f.set(true));
        let l = listen();
        FIXED_IN_TEST.with(|f| f.set(false));
        let l = l.unwrap();
        assert_ne!(l.port, 0);
        if was_free {
            // (Unless something took it in the moment between.)
            assert!(l.port == PORT || std::net::TcpListener::bind(("127.0.0.1", PORT)).is_err(), "port {}", l.port);
        } else {
            assert_ne!(l.port, PORT, "busy: any free port instead");
        }
        assert_eq!(l.redirect_uri, redirect_uri(l.port));
        drop(l);
    }
}
