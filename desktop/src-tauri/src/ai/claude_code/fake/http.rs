//! The fake's HTTP: one POST at a time over plain HTTP/1.1, to this PC only
//! (Yap's run endpoint in a test). Content-Length, chunked or read-to-close
//! bodies; an event stream can be cut short once it holds what's wanted.
//! Never Yap's own network path (`privacy/net.rs`): a test tool's loopback
//! calls aren't the person's traffic.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Response {
    pub status: u16,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// A header's value (the name ignoring case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// POST `body` (JSON) to `url` with `headers`; `done` is asked after each
/// piece of the body arrives and ends the read early when it says so.
pub fn post(url: &str, headers: &[(String, String)], body: &[u8], done: impl Fn(&[u8]) -> bool) -> Result<Response, String> {
    let u = url::Url::parse(url).map_err(|e| format!("not a URL ({url}): {e}"))?;
    if u.scheme() != "http" {
        return Err("the fake only speaks plain http, to this PC".into());
    }
    let ip = match u.host() {
        Some(url::Host::Ipv4(ip)) if ip.is_loopback() => std::net::IpAddr::V4(ip),
        Some(url::Host::Ipv6(ip)) if ip.is_loopback() => std::net::IpAddr::V6(ip),
        Some(url::Host::Domain("localhost")) => std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        _ => return Err("the fake only talks to this PC (127.0.0.1)".into()),
    };
    let port = u.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect_timeout(&SocketAddr::new(ip, port), TIMEOUT).map_err(|e| format!("can't connect: {e}"))?;
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    let path = &u[url::Position::BeforePath..];
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {}:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        u.host_str().unwrap_or("127.0.0.1"),
        body.len()
    );
    let ours = ["host", "content-type", "accept", "content-length", "connection"];
    for (k, v) in headers.iter().filter(|(k, _)| !ours.contains(&k.to_ascii_lowercase().as_str())) {
        if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
            return Err("a header with a line break".into());
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).map_err(|e| format!("can't send: {e}"))?;
    let mut r = BufReader::new(stream);
    let status_line = read_line(&mut r)?;
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("not an HTTP answer")?;
    let mut headers = Vec::new();
    loop {
        let line = read_line(&mut r)?;
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut res = Response { status, headers, body: Vec::new() };
    let chunked = res.header("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    let length = res.header("content-length").and_then(|l| l.parse::<usize>().ok());
    if status == 204 || status == 304 {
        return Ok(res);
    }
    if chunked {
        loop {
            let size_line = read_line(&mut r)?;
            let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "a bad chunk")?;
            if size == 0 {
                break;
            }
            let mut chunk = vec![0u8; size];
            r.read_exact(&mut chunk).map_err(|e| format!("the answer stopped: {e}"))?;
            res.body.extend_from_slice(&chunk);
            let _ = read_line(&mut r);
            if done(&res.body) {
                break;
            }
        }
    } else if let Some(len) = length {
        res.body = vec![0u8; len];
        r.read_exact(&mut res.body).map_err(|e| format!("the answer stopped: {e}"))?;
    } else {
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    res.body.extend_from_slice(&buf[..n]);
                    if done(&res.body) {
                        break;
                    }
                }
                Err(e) if res.body.is_empty() => return Err(format!("the answer stopped: {e}")),
                Err(_) => break,
            }
        }
    }
    Ok(res)
}

fn read_line(r: &mut impl BufRead) -> Result<String, String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| format!("the answer stopped: {e}"))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_pc() {
        for url in ["https://127.0.0.1/x", "http://example.com/mcp", "http://10.0.0.2/mcp", "ftp://127.0.0.1/"] {
            assert!(post(url, &[], b"{}", |_| false).is_err(), "{url}");
        }
    }

    #[test]
    fn reads_chunked_and_stops_early() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2).flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
                let _ = stream.write_all(head.as_bytes());
                for part in ["data: one\n\n", "data: two\n\n"] {
                    let _ = write!(stream, "{:x}\r\n{part}\r\n", part.len());
                    let _ = stream.flush();
                }
                // Then nothing more for a long while (an open stream).
                std::thread::sleep(Duration::from_secs(5));
                let _ = stream.write_all(b"0\r\n\r\n");
            }
        });
        let url = format!("http://127.0.0.1:{port}/mcp");
        let started = std::time::Instant::now();
        let r = post(&url, &[("Authorization".into(), "Bearer k".into())], b"{}", |b| String::from_utf8_lossy(b).contains("two")).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.header("content-type"), Some("text/event-stream"));
        assert_eq!(String::from_utf8_lossy(&r.body), "data: one\n\ndata: two\n\n");
        assert!(started.elapsed() < Duration::from_secs(4), "didn't wait for the stream to end");
        assert!(post(&url, &[("Bad\r\nHeader".into(), "x".into())], b"{}", |_| false).is_err());
    }
}
