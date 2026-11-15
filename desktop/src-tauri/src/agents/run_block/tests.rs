use super::*;

#[test]
fn only_shell_blocks_get_a_run_button() {
    for lang in ["bash", "sh", "Shell", "console", "zsh"] {
        assert_eq!(shell_for(lang), Some(ShellKind::Bash), "{lang}");
    }
    for lang in ["powershell", "pwsh", "ps1", "PS"] {
        assert_eq!(shell_for(lang), Some(ShellKind::PowerShell), "{lang}");
    }
    assert_eq!(shell_for("cmd"), Some(ShellKind::Cmd));
    assert_eq!(shell_for("bat"), Some(ShellKind::Cmd));
    for lang in ["", "js", "python", "text", "diff", "json"] {
        assert_eq!(shell_for(lang), None, "{lang}");
    }
}

#[test]
fn a_block_runs_only_when_its_one_line() {
    assert_eq!(one_line("npm test\n").as_deref(), Some("npm test"));
    assert_eq!(one_line("  echo \"hi there\"  ").as_deref(), Some("echo \"hi there\""));
    // A prompt before it is the terminal's, not the command's.
    assert_eq!(one_line("$ npm run build").as_deref(), Some("npm run build"));
    assert_eq!(one_line("PS> Get-ChildItem").as_deref(), Some("Get-ChildItem"));
    assert_eq!(one_line(r"PS C:\Projects\garden-planner> npm test").as_deref(), Some("npm test"));
    assert_eq!(one_line("> dir").as_deref(), Some("dir"));
    // A redirect in the middle is kept as written.
    assert_eq!(one_line("echo hi > out.txt").as_deref(), Some("echo hi > out.txt"));
    // More than one line, none, too long, control characters.
    assert_eq!(one_line("cd app\nnpm test"), None);
    assert_eq!(one_line("echo a\r\necho b"), None);
    assert_eq!(one_line("   \n  "), None);
    assert_eq!(one_line("$ "), None);
    assert_eq!(one_line(&"x".repeat(COMMAND_MAX + 1)), None);
    assert_eq!(one_line("echo \u{7}bell"), None);
}

#[test]
fn output_keeps_its_newest_part() {
    let mut s = "abcdef".to_string();
    assert!(!keep_tail(&mut s, 10));
    assert_eq!(s, "abcdef");
    assert!(keep_tail(&mut s, 3));
    assert_eq!(s, "def");
    // Never cut inside a character.
    let mut s = "ééééé".to_string();
    assert!(keep_tail(&mut s, 2));
    assert_eq!(s, "éé");
}

#[test]
fn powershells_line_goes_encoded_with_utf8_out() {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD.decode(encoded("Write-Output \"héllo\"")).unwrap();
    let units: Vec<u16> = raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let script = String::from_utf16(&units).unwrap();
    assert!(script.contains("[Console]::OutputEncoding = [Text.Encoding]::UTF8"));
    assert!(script.ends_with("\nWrite-Output \"héllo\""));
}

fn code_chat_in(tag: &str, conversation: u64) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-run-block-{tag}-{}", store::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let folder = super::super::workspaces::add(&dir.to_string_lossy(), None, None).unwrap();
    assert!(code_chat::set(conversation, Some(&folder)));
    dir
}

#[test]
fn it_asks_first_but_in_bypass_and_never_outside_a_code_chat() {
    let dir = code_chat_in("ask", 71);
    store::set_mode(71, "auto");
    let c = check(71, "bash").unwrap();
    assert!(c.asks, "Auto asks first");
    assert_eq!(c.mode, "Auto");
    store::set_mode(71, "manual");
    assert!(check(71, "powershell").unwrap().asks);
    // Bypass, once confirmed: nothing asks.
    store::set_mode(71, "bypassPermissions");
    store::confirm_bypass();
    let c = check(71, "powershell").unwrap();
    assert!(!c.asks, "Bypass asks nothing");
    assert!(c.ran_in.contains("PowerShell"));
    // Not a Code chat, not a shell's block.
    assert!(check(72, "bash").is_err());
    assert!(check(71, "js").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_run_refuses_without_asking_then_runs_in_the_workspace_and_stops() {
    let dir = code_chat_in("run", 81);
    std::fs::write(dir.join("marker.txt"), "here").unwrap();
    store::set_mode(81, "auto");
    // Asked first, and not asked: refused.
    let err = start(81, "Write-Output hi", "powershell", false).await.unwrap_err();
    assert!(err.contains("Asked first"), "{err}");
    // Not one line: refused.
    assert!(start(81, "echo a\necho b", "powershell", true).await.is_err());
    // Not a Code chat: refused.
    assert!(start(82, "Write-Output hi", "powershell", true).await.is_err());
    if !cfg!(windows) {
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    // Run in the workspace: it sees the workspace's own file.
    let run = start(81, "Get-Content marker.txt; Write-Output done", "powershell", true).await.unwrap();
    assert_eq!(run.state, "running");
    let mut info = None;
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = list(81).into_iter().find(|r| r.id == run.id).unwrap();
        if now.state != "running" {
            info = Some(now);
            break;
        }
    }
    let info = info.expect("it ended");
    assert_eq!(info.state, "done", "{info:?}");
    assert_eq!(info.code, Some(0));
    let out = info.output.unwrap_or_default();
    assert!(out.contains("here") && out.contains("done"), "{out}");
    // A long one, stopped.
    let long = start(81, "Start-Sleep -Seconds 60", "powershell", true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    stop(&long.id).unwrap();
    let mut stopped = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if list(81).into_iter().any(|r| r.id == long.id && r.state == "stopped") {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "Stop ends it");
    let _ = std::fs::remove_dir_all(dir);
}
