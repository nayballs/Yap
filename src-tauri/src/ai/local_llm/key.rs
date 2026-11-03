//! **A key for every server Yap starts on this PC** (2026-11-03). llama.cpp's
//! server — the on-device model ([`super`]), the search model's embedder
//! (`search/embedder.rs`), an engine trial (`engine/trial.rs`), a model
//! evaluation's and `yap.exe bench`'s — listens on 127.0.0.1, where anything
//! on this PC can reach it, a web page too: the page beside the chat refuses
//! requests to this PC, but WebView2 never shows Yap a WebSocket, and a public
//! name can resolve to 127.0.0.1 (`chat/pages/view.rs`). llama.cpp's CORS
//! doesn't stop a page either: it echoes any page's `Origin`, credentials
//! allowed, and answers every preflight. So each launch gets a key of its own,
//! and a request without it is refused.
//!
//! - **The key**: 32 bytes from the operating system's random numbers
//!   (`getrandom`), as 64 hex characters, made for one launch and forgotten
//!   when that server stops ([`Issued`]). Never in a file, config.json, a
//!   log, the network receipts or the frontend: llama.cpp's own log shows its
//!   last four characters only (`api_keys: ****1a2b`).
//! - **Handed over** in the `LLAMA_API_KEY` environment variable ([`ENV`]),
//!   which llama.cpp's builds and llamafile 0.10.6 both read for `--api-key`
//!   — never on the command line, which every program on the PC can list
//!   (Task Manager's column), nor in an `--api-key-file` on disk.
//! - **Sent** by every Yap request to it as `Authorization: Bearer <key>`
//!   ([`authorize`], by the request's own address: `ai/llm.rs`'s one chat
//!   call, the warm-up, `/props`, `/tokenize`, the embeddings, the trial's
//!   and the evaluations' requests).
//! - **What stays open** without it: `/health` (and `/v1/health`), which
//!   Yap's readiness checks use; llamafile 0.10.6 also lists its model at
//!   `/v1/models`, so the servers are started with `--alias` (the model's
//!   name, never its path, which holds the Windows user name). Checked against
//!   llama.cpp b11985 and llamafile 0.10.6 (the maintainer's builds, copied):
//!   401 for `/props`, `/slots`, `/tokenize`, chat and embeddings without the
//!   key or with a wrong one, whatever the `Host` header says (a name that
//!   resolves here gets no further).
//! - **Checked at each start** ([`refuses_without`]): a server that answers
//!   without its key (an engine that stopped reading [`ENV`]) is said in the
//!   log, and an engine trial counts it as not answering, so a build like
//!   that is never switched to.

use std::sync::Mutex;

use crate::privacy::net::Recorded as _;

/// The environment variable llama.cpp's server reads its key from (its
/// `--api-key`, `--help`: "(env: LLAMA_API_KEY)").
pub const ENV: &str = "LLAMA_API_KEY";

/// The variable for a file of more keys (`--api-key-file`): one inherited
/// from the person's own environment is taken off, so Yap's key is the only
/// one a server of Yap's takes.
const ENV_FILE: &str = "LLAMA_ARG_API_KEY_FILE";

/// A server's key. Its `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

impl Key {
    /// A new key: 32 random bytes from the operating system, as hex.
    pub fn new() -> Result<Key, String> {
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).map_err(|e| format!("No random numbers on this PC for the on-device engine's key: {e}"))?;
        Ok(Key(raw.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// `cmd` (a llama.cpp server about to start) takes this key, through
    /// its environment.
    pub fn give(&self, cmd: &mut std::process::Command) {
        cmd.env(ENV, &self.0).env_remove(ENV_FILE);
    }

    /// `req` carrying this key.
    pub fn on(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.bearer_auth(&self.0)
    }

    /// The `Authorization` header's value. Tests only: nothing else needs it
    /// as text.
    #[cfg(test)]
    pub fn header(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

/// The keys of the servers Yap runs now, by port.
static ISSUED: Mutex<Vec<(u16, Key)>> = Mutex::new(Vec::new());

/// A key held for a server on `port`, until dropped (the server stopped):
/// requests to that port carry it ([`for_url`]) while it's held.
#[derive(Debug)]
pub struct Issued {
    port: u16,
    key: Key,
}

impl Issued {
    #[cfg(test)]
    pub fn key(&self) -> &Key {
        &self.key
    }

    /// `cmd` takes the key ([`Key::give`]).
    pub fn give(&self, cmd: &mut std::process::Command) {
        self.key.give(cmd);
    }
}

impl Drop for Issued {
    fn drop(&mut self) {
        let mut g = ISSUED.lock().unwrap_or_else(|p| p.into_inner());
        g.retain(|(p, k)| !(*p == self.port && *k == self.key));
    }
}

/// **A new key for the server about to start on `port`**, held until the
/// [`Issued`] drops. A port held before (a server that stopped without its
/// key dropped) takes the new one.
pub fn issue(port: u16) -> Result<Issued, String> {
    let key = Key::new()?;
    let mut g = ISSUED.lock().unwrap_or_else(|p| p.into_inner());
    g.retain(|(p, _)| *p != port);
    g.push((port, key.clone()));
    Ok(Issued { port, key })
}

/// The port of `url` when it's on 127.0.0.1 (how Yap addresses its own
/// servers; `None` for anything else). Pure.
fn local_port(url: &str) -> Option<u16> {
    let u = url::Url::parse(url.trim()).ok()?;
    if u.scheme() != "http" || u.host_str() != Some("127.0.0.1") {
        return None;
    }
    u.port().filter(|p| *p != 0)
}

/// The key of Yap's own server at `url` (`http://127.0.0.1:<port>/…`, a
/// port it holds a key for); `None` for any other address — a server of
/// the person's own on this PC, the cloud — which never sees it.
pub fn for_url(url: &str) -> Option<Key> {
    let port = local_port(url)?;
    ISSUED.lock().ok()?.iter().find(|(p, _)| *p == port).map(|(_, k)| k.clone())
}

/// `req` to `url`, carrying the key when `url` is one of Yap's own servers
/// ([`for_url`]); unchanged otherwise.
pub fn authorize(req: reqwest::RequestBuilder, url: &str) -> reqwest::RequestBuilder {
    match for_url(url) {
        Some(k) => k.on(req),
        None => req,
    }
}

/// Did the server on `port` refuse a request without its key (`GET /props`:
/// 401, as llama.cpp answers before it looks at the path)? `false` when it
/// answered anything else, or not at all.
pub async fn refuses_without(client: &reqwest::Client, port: u16, purpose: crate::privacy::net::Purpose) -> bool {
    match client.get(format!("http://127.0.0.1:{port}/props")).send_for(purpose).await {
        Ok(r) => refused(r.status().as_u16()),
        Err(_) => false,
    }
}

/// A status that says the key was wanted. Pure.
fn refused(status: u16) -> bool {
    matches!(status, 401 | 403)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_is_new_long_and_never_shown() {
        let (a, b) = (Key::new().unwrap(), Key::new().unwrap());
        assert_ne!(a, b, "a key per launch");
        assert_eq!(a.0.len(), 64, "32 bytes, as hex");
        assert!(a.0.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(format!("{a:?}"), "Key(…)", "Debug never shows it");
        let held = issue(47_100).unwrap();
        assert!(!format!("{held:?}").contains(&held.key().0));
    }

    #[test]
    fn a_server_takes_its_key_from_its_environment_never_its_command_line() {
        let k = Key::new().unwrap();
        let mut cmd = std::process::Command::new("llama-server.exe");
        cmd.arg("--port").arg("5000").env(ENV_FILE, "C:\\keys.txt");
        k.give(&mut cmd);
        let env: Vec<(String, Option<String>)> =
            cmd.get_envs().map(|(n, v)| (n.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert!(env.contains(&(ENV.to_string(), Some(k.0.clone()))));
        assert!(env.contains(&(ENV_FILE.to_string(), None)), "a key file from the person's environment is taken off");
        assert!(!cmd.get_args().any(|a| a.to_string_lossy().contains(&k.0)), "never on the command line");
        assert!(!cmd.get_args().any(|a| a == "--api-key" || a == "--api-key-file"));
    }

    #[test]
    fn only_requests_to_yaps_own_servers_carry_a_key() {
        let issued = issue(47_101).unwrap();
        let key = issued.key().clone();
        assert_eq!(for_url("http://127.0.0.1:47101/v1"), Some(key.clone()));
        assert_eq!(for_url("http://127.0.0.1:47101/v1/chat/completions"), Some(key.clone()));
        assert_eq!(for_url("http://127.0.0.1:47101"), Some(key.clone()), "a server's root (/tokenize, /props)");
        // Anything else never sees it: another port, another host, a name
        // that may resolve anywhere, https, the stand-by address.
        for other in [
            "http://127.0.0.1:47102/v1",
            "http://localhost:47101/v1",
            "http://localtest.me:47101/v1",
            "http://192.168.1.20:47101/v1",
            "https://127.0.0.1:47101/v1",
            "https://api.groq.com/openai/v1",
            "http://127.0.0.1:0/v1",
            "not a url",
        ] {
            assert_eq!(for_url(other), None, "{other}");
        }
        // The header on the request.
        let client = reqwest::Client::new();
        let req = authorize(client.post("http://127.0.0.1:47101/v1/chat/completions"), "http://127.0.0.1:47101/v1").build().unwrap();
        assert_eq!(req.headers().get("authorization").and_then(|v| v.to_str().ok()), Some(key.header().as_str()));
        let plain = authorize(client.post("http://127.0.0.1:47102/v1/chat/completions"), "http://127.0.0.1:47102/v1").build().unwrap();
        assert!(plain.headers().get("authorization").is_none());
        // Stopped: forgotten.
        drop(issued);
        assert_eq!(for_url("http://127.0.0.1:47101/v1"), None);
    }

    #[test]
    fn a_new_launch_on_a_port_takes_a_new_key_and_the_old_one_never_comes_back() {
        let first = issue(47_103).unwrap();
        let second = issue(47_103).unwrap();
        assert_ne!(first.key(), second.key());
        assert_eq!(for_url("http://127.0.0.1:47103/v1").as_ref(), Some(second.key()));
        // The first one's drop (late) leaves the second's key alone.
        drop(first);
        assert_eq!(for_url("http://127.0.0.1:47103/v1").as_ref(), Some(second.key()));
        drop(second);
        assert_eq!(for_url("http://127.0.0.1:47103/v1"), None);
    }

    #[test]
    fn a_refusal_is_a_401_or_403() {
        assert!(refused(401) && refused(403));
        assert!(!refused(200) && !refused(404) && !refused(500));
    }

    /// **The real thing**: an engine on this PC started as Yap starts its
    /// servers, its key in its environment, and asked with and without it —
    /// `YAP_KEY_ENGINE`: a llama.cpp build's folder (`llama-server.exe`), or
    /// a `llamafile.exe`; `YAP_KEY_CUDART`: NVIDIA's runtime folder for a
    /// CUDA build; `YAP_KEY_MODEL`: a small GGUF, read in place;
    /// `YAP_KEY_DATA`: a scratch data folder (never the person's own). On
    /// the processor, at the default 8K context (the cleanup's prompt is ~2K).
    /// `cargo test real_server_refuses -- --ignored --nocapture --test-threads 1`.
    #[test]
    #[ignore]
    fn real_server_refuses_requests_without_its_key() {
        use crate::ai::local_llm::engine::{Engine, Installed, Variant, FLOOR};
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let (Some(at), Some(model), Some(data)) = (var("YAP_KEY_ENGINE"), var("YAP_KEY_MODEL"), var("YAP_KEY_DATA")) else {
            eprintln!("set YAP_KEY_ENGINE, YAP_KEY_MODEL and YAP_KEY_DATA");
            return;
        };
        assert!(crate::shell::portable::use_data_dir(std::path::PathBuf::from(&data)), "a scratch data folder");
        let at = std::path::PathBuf::from(at);
        let engine = if at.is_file() {
            Engine::Llamafile { exe: at, version: crate::ai::local_llm::runtime::Version(0, 10, 6) }
        } else {
            let installed = Installed { build: FLOOR, variant: Variant::Cuda13, sha256: String::new(), cudart: None, device: None };
            Engine::LlamaCpp { dir: at, installed, cudart: var("YAP_KEY_CUDART").map(Into::into) }
        };
        let model = std::path::PathBuf::from(model);
        let id = crate::ai::local_llm::model_id(&model);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build().unwrap();
        let check = || crate::privacy::net::Purpose::OnDeviceCheck;
        let status = |req: reqwest::RequestBuilder| rt.block_on(async { req.send_for(check()).await.map(|r| r.status().as_u16()).unwrap_or(0) });
        let body = |req: reqwest::RequestBuilder| rt.block_on(async { req.send_for(check()).await.unwrap().text().await.unwrap_or_default() });
        let wait_up = |port: u16, child: &mut std::process::Child| {
            for _ in 0..240 {
                assert!(child.try_wait().unwrap().is_none(), "the server exited");
                if status(client.get(format!("http://127.0.0.1:{port}/health"))) == 200 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            panic!("not up");
        };

        // The on-device model, as the sidecar starts it.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let issued = issue(port).unwrap();
        let plan = crate::ai::local_llm::Plan { runtime: engine.runtime(), mtp: false, kv_q8: false, per_slot: false };
        let mut cmd = engine.command();
        cmd.args(crate::ai::local_llm::server_args(&model, port, 0, 8_192, plan)).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        issued.give(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        crate::shell::procs::end_with_yap(&child);
        let base = format!("http://127.0.0.1:{port}");
        wait_up(port, &mut child);
        println!("{} ({}): /health answers without the key (Yap's readiness check)", engine.label(), engine.exe().display());
        // Without the key, with a wrong one, and from a name that resolves here.
        assert!(rt.block_on(refuses_without(&client, port, crate::privacy::net::Purpose::OnDeviceCheck)));
        for path in ["/props", "/slots", "/v1/chat/completions", "/tokenize"] {
            assert_eq!(status(client.post(format!("{base}{path}")).body("{}")), 401, "POST {path} without the key");
            assert_eq!(status(client.get(format!("{base}{path}")).bearer_auth("not-the-key")), 401, "{path} with a wrong key");
        }
        assert_eq!(status(client.get(format!("{base}/props")).header("Host", "localtest.me")), 401, "a rebound name gets no further");
        assert_eq!(status(client.get(format!("{base}/props")).header("Origin", "https://page.example.com")), 401, "a page's fetch");
        // What's open without the key never names the model's folder.
        let listed = body(client.get(format!("{base}/v1/models")));
        let folder = model.parent().unwrap().to_string_lossy().replace('\\', "/");
        assert!(!listed.replace('\\', "/").contains(&folder) && !listed.contains(&*model.to_string_lossy()), "{listed}");
        println!("  /v1/models without the key: {}", if listed.contains("Invalid API Key") { "refused" } else { "the model's name only" });
        // Yap's own callers: `/props`, the warm-up, a dictation's cleanup.
        let url = format!("{base}/props");
        assert_eq!(status(authorize(client.get(&url), &url)), 200, "/props with the key");
        let v1 = format!("{base}/v1");
        let w = rt.block_on(crate::ai::local_llm::warm_up(&v1, &id));
        assert_eq!(w.error, None, "the warm-up");
        let cfg = crate::config::YapConfig::default();
        let cleaned = rt.block_on(crate::ai::llm::cleanup("so um the meeting is on thursday", &v1, "", &id, crate::ai::local_llm::PROVIDER_ONDEVICE, &cfg.pp_prompt, None, &[], false));
        assert!(cleaned.as_ref().is_ok_and(|t| !t.trim().is_empty()), "Yap's chat client: {cleaned:?}");
        println!("  Yap's client: /props, the warm-up and a cleanup answered with the key");
        let _ = child.kill();
        let _ = child.wait();
        drop(issued);

        // The search model's server, as the embedder starts it (the chat
        // model in embedding mode stands in for EmbeddingGemma).
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let issued = issue(port).unwrap();
        let mut cmd = engine.command();
        let mut args = crate::search::embedder::server_args(&model, port, 4, engine.runtime());
        if let Some(i) = args.iter().position(|a| a == "-ngl") {
            args[i + 1] = "0".into();
        }
        // A chat model pools nothing by itself; EmbeddingGemma's GGUF says mean.
        args.extend(["--pooling".to_string(), "mean".to_string()]);
        cmd.args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        issued.give(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        crate::shell::procs::end_with_yap(&child);
        wait_up(port, &mut child);
        let url = format!("http://127.0.0.1:{port}/v1");
        let ask = serde_json::json!({ "input": ["a passage"] });
        assert_eq!(status(client.post(format!("{url}/embeddings")).json(&ask)), 401, "embeddings without the key");
        crate::search::embedder::use_server(Some(url));
        let v = rt.block_on(crate::search::embedder::embed(&["title: none | text: Thursday at 9:40".to_string()]));
        crate::search::embedder::use_server(None);
        assert!(v.as_ref().is_ok_and(|v| v.len() == 1), "Yap's embedder with the key: {v:?}");
        println!("  the embedder: 401 without the key, vectors with it");
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn a_server_that_answers_without_its_key_is_caught() {
        // A stand-in server: 401 without the key on `/props` (llama.cpp
        // with one), 200 for the other (a server that ignored its key).
        let rt = tokio::runtime::Runtime::new().unwrap();
        for (status, refuses) in [(401u16, true), (200, false)] {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let port = server.server_addr().to_ip().unwrap().port();
            let t = std::thread::spawn(move || {
                if let Ok(req) = server.recv() {
                    let ok = req.url() == "/props" && !req.headers().iter().any(|h| h.field.equiv("Authorization"));
                    let _ = req.respond(tiny_http::Response::from_string("{}").with_status_code(if ok { status } else { 500 }));
                }
            });
            let client = reqwest::Client::new();
            assert_eq!(rt.block_on(refuses_without(&client, port, crate::privacy::net::Purpose::OnDeviceCheck)), refuses, "{status}");
            t.join().unwrap();
        }
    }
}
