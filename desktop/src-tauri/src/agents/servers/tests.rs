use super::*;

#[test]
fn launch_json_reads_as_claude_codes_with_comments_and_trailing_commas() {
    let text = r#"{
      // The dev servers (comments are allowed)
      "version": "0.0.1",
      "configurations": [
        { "name": "web", "runtimeExecutable": "npm", "runtimeArgs": ["run", "dev"], "port": 5173, },
        /* a script run with node */
        { "name": "api", "program": "server.js", "args": ["--quiet"], "cwd": "api", "env": { "MODE": "dev" }, "autoPort": true },
        { "name": "running", "url": "http://localhost:8080" },
        { "name": "web", "runtimeExecutable": "npm" },
        { "runtimeExecutable": "npm" },
        { "name": "far", "runtimeExecutable": "npm", "url": "https://example.com/app" },
        { "name": "pathy", "url": "http://localhost:3000/admin" },
        { "name": "odd", "url": "http://127.0.0.1:4000", "port": 4001 },
      ],
    }"#;
    let l = parse(text).unwrap();
    assert!(l.exists && l.has_comments);
    let names: Vec<&str> = l.configs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["web", "api", "running"]);
    let web = &l.configs[0];
    assert_eq!((web.command.as_deref(), web.args.clone(), web.port), (Some("npm"), vec!["run".to_string(), "dev".into()], 5173));
    assert_eq!(web.line(), "npm run dev");
    assert_eq!(web.page(5173), "http://localhost:5173/");
    // A program: run with node, its args after it; the port by default.
    let api = &l.configs[1];
    assert_eq!((api.command.as_deref(), api.args.clone(), api.port, api.auto_port), (Some("node"), vec!["server.js".to_string(), "--quiet".into()], DEFAULT_PORT, Some(true)));
    assert_eq!((api.cwd.as_deref(), api.env.get("MODE").map(String::as_str)), (Some("api"), Some("dev")));
    // A url alone: attached to, its port from the url.
    let running = &l.configs[2];
    assert_eq!((running.command.clone(), running.port, running.line()), (None, 8080, "(attach to http://localhost:8080)".to_string()));
    // The rest, each said why.
    let skipped = l.skipped.join(" | ");
    for why in ["name is used twice", "it has no \"name\"", "can only be attached to", "origin alone", "on port 4000 but its \"port\" is 4001"] {
        assert!(skipped.contains(why), "{why}: {skipped}");
    }
    // Plain JSON has no comments; a slash inside a string isn't one.
    assert!(!parse(r#"{"configurations":[{"name":"a","runtimeExecutable":"npm","runtimeArgs":["run","dev","--base=/app//x"]}]}"#).unwrap().has_comments);
    assert!(parse("{").unwrap_err().contains("isn't valid JSON"));
    assert!(parse(r#"{"version":"0.0.1"}"#).unwrap_err().contains("no \"configurations\""));
}

#[test]
fn a_taken_port_moves_or_stays_stopped_as_claude_codes_docs_say() {
    let cfg = |args: &[&str], auto: Option<bool>, cmd: &str| Config { name: "web".into(), command: Some(cmd.into()), args: args.iter().map(|a| a.to_string()).collect(), port: 3000, auto_port: auto, ..Default::default() };
    // Free: as it is.
    assert_eq!(port_plan(&cfg(&["run", "dev"], None, "npm"), false, None).unwrap(), (3000, vec!["run".to_string(), "dev".into()]));
    // autoPort false: an error.
    assert!(port_plan(&cfg(&["run", "dev"], Some(false), "npm"), true, Some(3001)).unwrap_err().contains("\"autoPort\" is false"));
    // A --port in the arguments: moved, the argument rewritten.
    assert_eq!(port_plan(&cfg(&["run", "dev", "--", "--port", "3000"], None, "npm"), true, Some(3001)).unwrap(), (3001, ["run", "dev", "--", "--port", "3001"].map(String::from).to_vec()));
    assert_eq!(port_plan(&cfg(&["serve", "-p=3000"], None, "npx"), true, Some(3002)).unwrap().1, ["serve", "-p=3002"].map(String::from).to_vec());
    // A package manager's script whose port Yap can't see: stays stopped, unless autoPort says it reads PORT.
    assert!(port_plan(&cfg(&["run", "dev"], None, "npm"), true, Some(3001)).unwrap_err().contains("can't be moved"));
    assert!(port_plan(&cfg(&["run", "dev"], None, "C:/tools/pnpm.cmd"), true, Some(3001)).is_err());
    assert_eq!(port_plan(&cfg(&["run", "dev"], Some(true), "npm"), true, Some(3001)).unwrap().0, 3001);
    // Any other command: PORT set, the next free port.
    assert_eq!(port_plan(&cfg(&["server.js"], None, "node"), true, Some(3005)).unwrap().0, 3005);
    assert!(port_plan(&cfg(&["server.js"], None, "node"), true, None).unwrap_err().contains("no free port"));
}

#[test]
fn only_a_running_servers_own_origin_is_this_pc_allowed() {
    assert!(is_loopback_host("localhost") && is_loopback_host("app.localhost") && is_loopback_host("127.0.0.1") && is_loopback_host("[::1]"));
    assert!(!is_loopback_host("192.168.1.4") && !is_loopback_host("example.com"));
    let id = "srv-test-origin";
    servers().push(Server {
        info: Info { id: id.into(), name: "web".into(), workspace: "e:/w".into(), port: 41_973, url: "http://localhost:41973/".into(), state: "ready".into(), why: None, attached: false, moved_from: None },
        pid: None,
        since: 0,
        logs: VecDeque::new(),
        stopping: false,
        at: Instant::now(),
    });
    for ok in ["http://localhost:41973/", "http://127.0.0.1:41973/src/main.js", "ws://localhost:41973/@vite/client", "http://[::1]:41973"] {
        assert!(running_origin(ok), "{ok}");
    }
    for no in ["http://localhost:41974/", "http://localhost/", "http://192.168.1.4:41973/", "https://example.com:41973/", "file:///C:/x", "nonsense"] {
        assert!(!running_origin(no), "{no}");
    }
    // Stopped: no longer.
    servers().iter_mut().find(|s| s.info.id == id).unwrap().info.state = "stopped".into();
    assert!(!running_origin("http://localhost:41973/"));
    servers().retain(|s| s.info.id != id);
}

#[test]
fn output_is_kept_without_colours_and_read_by_errors_or_words() {
    assert_eq!(strip_ansi("\u{1b}[32m  VITE v6\u{1b}[39m ready\r"), "  VITE v6 ready");
    let id = "srv-test-logs";
    servers().push(Server {
        info: Info { id: id.into(), name: "web".into(), workspace: "e:/w".into(), port: 1, url: String::new(), state: "ready".into(), why: None, attached: false, moved_from: None },
        pid: None,
        since: 0,
        logs: VecDeque::new(),
        stopping: false,
        at: Instant::now(),
    });
    record(id, false, "\u{1b}[36mLocal:\u{1b}[0m http://localhost:5173/");
    record(id, false, "Build error in App.svelte: unexpected token");
    record(id, true, "warn: a deprecated option");
    record(id, false, "page reload src/App.svelte");
    assert_eq!(logs(id, false, 2, None).unwrap(), ["warn: a deprecated option", "page reload src/App.svelte"]);
    assert_eq!(logs(id, true, 50, None).unwrap(), ["Build error in App.svelte: unexpected token", "warn: a deprecated option"]);
    assert_eq!(logs(id, false, 50, Some("LOCAL")).unwrap(), ["Local: http://localhost:5173/"]);
    assert!(logs("srv-none", false, 5, None).is_err());
    servers().retain(|s| s.info.id != id);
}

#[test]
fn add_makes_launch_json_or_adds_to_it_and_never_drops_comments() {
    let dir = std::env::temp_dir().join(format!("yap-servers-add-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    add(&dir, "web", "npm", &["run".into(), "dev".into()], 5173).unwrap();
    add(&dir, "api", "node", &["server.js".into()], 8787).unwrap();
    let l = read(&dir).unwrap();
    assert_eq!(l.configs.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["web", "api"]);
    assert!(add(&dir, "WEB", "npm", &[], 1).unwrap_err().contains("already a server"));
    assert!(add(&dir, "", "npm", &[], 1).is_err() && add(&dir, "x", "npm", &[], 0).is_err());
    std::fs::write(dir.join(LAUNCH), "{ // mine\n \"configurations\": [] }").unwrap();
    assert!(add(&dir, "web", "npm", &[], 5173).unwrap_err().contains("ask Claude"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A real one: a tiny server with node (when it's installed), its page
/// answering, its output kept, then stopped — and started twice, one.
#[tokio::test]
async fn a_server_starts_answers_and_stops() {
    if resolve("node", Path::new(".")).is_none() {
        eprintln!("node isn't installed here: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("yap-servers-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(dir.join("server.js"), "const p=+process.env.PORT;require('http').createServer((q,r)=>r.end('hello from the test server')).listen(p,'127.0.0.1',()=>console.log('listening on '+p));").unwrap();
    let port = free_port(41_000).unwrap();
    std::fs::write(dir.join(LAUNCH), format!(r#"{{"version":"0.0.1","configurations":[{{"name":"tiny","program":"server.js","port":{port}}}]}}"#)).unwrap();
    let info = start(&dir, "tiny").await.unwrap();
    assert_eq!((info.state.as_str(), info.port, info.url.clone()), ("ready", port, format!("http://localhost:{port}/")));
    assert!(running_origin(&info.url));
    // Started again: the same one.
    assert_eq!(start(&dir, "TINY").await.unwrap().id, info.id);
    // Its output, kept.
    for _ in 0..50 {
        if logs(&info.id, false, 10, Some("listening")).unwrap().len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(logs(&info.id, false, 10, Some("listening")).unwrap(), [format!("listening on {port}")]);
    // Stopped: its port free again, its page no longer allowed.
    // The answer says stopped at once.
    assert_eq!(stop(&info.id).unwrap().state, "stopped");
    for _ in 0..50 {
        if get(&info.id).unwrap().state == "stopped" && !in_use(port) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(get(&info.id).unwrap().state, "stopped");
    assert!(!in_use(port) && !running_origin(&info.url));
    // A name that isn't there.
    assert!(start(&dir, "nope").await.unwrap_err().contains("“tiny”"));
    let _ = std::fs::remove_dir_all(&dir);
}
