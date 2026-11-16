use super::*;

/// A made-up PC in a folder of its own: `LOCALAPPDATA`, Program Files, Windows, and PATH's folders.
struct Pc {
    base: PathBuf,
}

impl Pc {
    fn new(tag: &str) -> Pc {
        let base = std::env::temp_dir().join(format!("yap-editors-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Pc { base }
    }
    fn at(&self, rel: &str) -> PathBuf {
        self.base.join(rel)
    }
    /// An empty file at `rel` (a program needs only to be there).
    fn file(&self, rel: &str) -> PathBuf {
        let p = self.at(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"").unwrap();
        p
    }
    fn sources(&self) -> Sources {
        Sources {
            local_app_data: Some(self.at("Local")),
            program_files: vec![self.at("Program Files"), self.at("Program Files (x86)")],
            system_root: Some(self.at("Windows")),
            path: Vec::new(),
            app_paths: Vec::new(),
            installed: Vec::new(),
        }
    }
}

impl Drop for Pc {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn ids(list: &[Editor]) -> Vec<&str> {
    list.iter().map(|e| e.id.as_str()).collect()
}

#[test]
fn every_argument_is_quoted_by_the_c_runtimes_rules() {
    assert_eq!(quote(r"C:\Projects\garden-planner\src\convert.js"), r#""C:\Projects\garden-planner\src\convert.js""#);
    assert_eq!(quote(r"C:\My Projects\a b.js:12"), r#""C:\My Projects\a b.js:12""#);
    // A trailing backslash doubled, so it doesn't escape the closing quote; a quote escaped.
    assert_eq!(quote(r"C:\Projects\garden planner\"), r#""C:\Projects\garden planner\\""#);
    assert_eq!(quote(r#"say "hi"\now"#), r#""say \"hi\"\now""#);
    assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
    assert_eq!(quote(""), r#""""#);
}

#[test]
fn each_editor_gets_the_line_its_own_way_and_paths_are_always_quoted() {
    let ed = |id: &str, line: Line| Editor { id: id.into(), name: name_of(id), program: PathBuf::from(r"C:\Tools\editor.exe"), line, console: false };
    let file = r"C:\Work\garden planner\src\convert.js";
    let ws = r"C:\Work\garden planner";
    assert_eq!(args(&ed("vscode", Line::Goto), file, Some(5), ws), vec![r#""C:\Work\garden planner""#.to_string(), "-g".into(), r#""C:\Work\garden planner\src\convert.js:5""#.into()]);
    assert_eq!(args(&ed("cursor", Line::Goto), file, None, ws), vec![r#""C:\Work\garden planner""#.to_string(), r#""C:\Work\garden planner\src\convert.js""#.into()]);
    assert_eq!(args(&ed("zed", Line::Suffix), file, Some(12), ws), vec![r#""C:\Work\garden planner\src\convert.js:12""#.to_string()]);
    assert_eq!(args(&ed("sublime", Line::Suffix), file, None, ws), vec![r#""C:\Work\garden planner\src\convert.js""#.to_string()]);
    assert_eq!(args(&ed("notepadpp", Line::Npp), file, Some(3), ws), vec!["-n3".to_string(), r#""C:\Work\garden planner\src\convert.js""#.into()]);
    assert_eq!(args(&ed("idea", Line::Flag), file, Some(42), ws), vec!["--line".to_string(), "42".into(), r#""C:\Work\garden planner\src\convert.js""#.into()]);
    assert_eq!(args(&ed("notepad", Line::None), file, Some(9), ws), vec![r#""C:\Work\garden planner\src\convert.js""#.to_string()]);
    // The whole line: the program quoted too.
    let e = Editor { program: PathBuf::from(r"C:\Program Files\Microsoft VS Code\Code.exe"), ..ed("vscode", Line::Goto) };
    assert_eq!(
        command_line(&e.program, &args(&e, file, Some(5), ws)),
        r#""C:\Program Files\Microsoft VS Code\Code.exe" "C:\Work\garden planner" -g "C:\Work\garden planner\src\convert.js:5""#
    );
}

#[test]
fn editors_are_found_in_their_folders_the_registry_and_path_and_listed_in_order() {
    let pc = Pc::new("found");
    let mut src = pc.sources();
    // VS Code, a per-user install.
    pc.file(r"Local\Programs\Microsoft VS Code\Code.exe");
    // Cursor, by its launcher on PATH (resources\app\bin\cursor.cmd), installed somewhere unusual.
    pc.file(r"Tools\cursor\Cursor.exe");
    pc.file(r"Tools\cursor\resources\app\bin\cursor.cmd");
    src.path.push(pc.at(r"Tools\cursor\resources\app\bin"));
    // Zed with its command line beside it (run instead, with no window, at the line).
    pc.file(r"Local\Programs\Zed\Zed.exe");
    pc.file(r"Local\Programs\Zed\bin\zed.exe");
    // Sublime Text by App Paths; Windsurf by the Uninstall registry's folder.
    let subl = pc.file(r"Apps\Sublime\sublime_text.exe");
    src.app_paths.push(("sublime_text.exe".into(), subl));
    pc.file(r"D\Windsurf\Windsurf.exe");
    src.installed.push(Installed { name: "Windsurf (User)".into(), location: Some(pc.at(r"D\Windsurf")), icon: None });
    // Notepad++ in Program Files (x86); Notepad in Windows.
    pc.file(r"Program Files (x86)\Notepad++\notepad++.exe");
    pc.file(r"Windows\System32\notepad.exe");
    // JetBrains: IntelliJ Community, two PyCharms (the newest wins), WebStorm by a Toolbox script.
    pc.file(r"Program Files\JetBrains\IntelliJ IDEA Community Edition 2025.2\bin\idea64.exe");
    pc.file(r"Program Files\JetBrains\PyCharm 2025.1\bin\pycharm64.exe");
    let newest = pc.file(r"Program Files\JetBrains\PyCharm 2025.2\bin\pycharm64.exe");
    let ws = pc.file(r"E\IDEs\WebStorm\bin\webstorm64.exe");
    let script = pc.file(r"Local\JetBrains\Toolbox\scripts\webstorm.cmd");
    std::fs::write(&script, format!("@echo off\r\nstart \"\" \"{}\" %*\r\n", ws.display())).unwrap();
    // Nothing for a launcher whose program isn't there, an App Paths entry for a missing file, or an install that isn't an editor's.
    pc.file(r"Stale\bin\code-insiders.cmd");
    src.path.push(pc.at(r"Stale\bin"));
    src.app_paths.push(("vscodium.exe".into(), pc.at(r"Gone\VSCodium.exe")));
    pc.file(r"Other\Code.exe");
    src.installed.push(Installed { name: "Garden Planner".into(), location: Some(pc.at("Other")), icon: None });

    let found = detect(&src);
    assert_eq!(ids(&found), vec!["vscode", "cursor", "windsurf", "zed", "sublime", "notepadpp", "idea-ce", "pycharm", "webstorm", "notepad"]);
    let by = |id: &str| found.iter().find(|e| e.id == id).unwrap();
    assert_eq!(by("vscode").program, pc.at(r"Local\Programs\Microsoft VS Code\Code.exe"));
    assert_eq!(by("cursor").program, pc.at(r"Tools\cursor\Cursor.exe"), "the launcher's way to its program, tidied");
    assert_eq!((by("zed").program.clone(), by("zed").line, by("zed").console), (pc.at(r"Local\Programs\Zed\bin\zed.exe"), Line::Suffix, true));
    assert_eq!(by("idea-ce").name, "IntelliJ IDEA Community");
    assert_eq!(by("pycharm").program, newest);
    assert_eq!(by("webstorm").program, ws);
    assert_eq!(by("notepadpp").line, Line::Npp);
    assert_eq!(by("notepad").line, Line::None);
}

#[test]
fn zed_without_its_command_line_opens_the_file_with_no_line() {
    let pc = Pc::new("zed");
    let zed = pc.file(r"Local\Programs\Zed\Zed.exe");
    let found = detect(&pc.sources());
    assert_eq!(ids(&found), vec!["zed"]);
    assert_eq!((found[0].program.clone(), found[0].line, found[0].console), (zed, Line::None, false));
}

#[test]
fn an_empty_pc_has_no_editors() {
    let pc = Pc::new("empty");
    assert!(detect(&pc.sources()).is_empty());
}

#[test]
fn open_in_takes_the_settings_editor_else_the_first_that_isnt_notepad() {
    let e = |id: &str| Editor { id: id.into(), name: name_of(id), program: PathBuf::new(), line: Line::None, console: false };
    let list = vec![e("cursor"), e("zed"), e("notepad")];
    assert_eq!(preferred(&list, "zed").map(|e| e.id.as_str()), Some("zed"));
    assert_eq!(preferred(&list, "yap").map(|e| e.id.as_str()), Some("cursor"));
    assert_eq!(preferred(&list, "vscode").map(|e| e.id.as_str()), Some("cursor"), "the setting's editor gone: the first found");
    assert_eq!(preferred(&[e("notepad")], "default").map(|e| e.id.as_str()), Some("notepad"));
    assert!(preferred(&[], "vscode").is_none());
}

#[test]
fn ids_and_names() {
    assert!(known("vscode") && known("notepadpp") && known("idea") && known("pycharm-ce") && known("android-studio"));
    assert!(!known("yap") && !known("default") && !known("cmd") && !known(r"C:\Windows\System32\cmd.exe"));
    assert_eq!(name_of("vscode"), "VS Code");
    assert_eq!(name_of("idea-ce"), "IntelliJ IDEA Community");
    assert_eq!(name_of("rider"), "Rider");
    assert_eq!(name_of("nope"), "That editor");
}

#[test]
fn a_toolbox_script_names_its_ide() {
    let text = "@echo off\r\n:: made by JetBrains Toolbox\r\nstart \"\" \"C:\\Users\\sam\\AppData\\Local\\Programs\\IntelliJ IDEA Ultimate\\bin\\idea64.exe\" %*\r\n";
    assert_eq!(toolbox_script_targets(text), vec![PathBuf::from(r"C:\Users\sam\AppData\Local\Programs\IntelliJ IDEA Ultimate\bin\idea64.exe")]);
    assert!(toolbox_script_targets("start \"\" \"C:\\Tools\\other.exe\" %*").is_empty());
}

#[test]
fn installs_looked_in_are_only_editors() {
    assert!(may_be_editor("Microsoft Visual Studio Code (User)") && may_be_editor("Cursor (User)") && may_be_editor("PyCharm Community Edition 2025.2") && may_be_editor("Notepad++ (64-bit x64)"));
    assert!(!may_be_editor("Garden Planner") && !may_be_editor("Mozilla Firefox"));
}

#[test]
fn a_file_windows_would_run_never_goes_to_its_default_app() {
    for name in ["build.ps1", "setup.EXE", "tool.js", "page.html", "run.py", "go.bat", "report.pdf.exe", "deploy.sh", "x.vbs.", "link.lnk"] {
        assert!(runs_when_opened(Path::new(name)).is_some(), "{name}");
    }
    let why = runs_when_opened(Path::new(r"C:\w\build.ps1")).unwrap();
    assert!(why.contains(".ps1") && why.contains("shown here"), "{why}");
    for name in ["notes.txt", "README.md", "costs.csv", "plan.pdf", "Makefile"] {
        assert!(runs_when_opened(Path::new(name)).is_none(), "{name}");
    }
}

#[test]
fn script_hosts_and_interpreters_are_runners() {
    for p in [r"C:\Windows\System32\wscript.exe", "python.exe", r#""C:\Python313\python3.13.exe""#, "pwsh.exe", r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe", "node"] {
        assert!(is_runner(p), "{p}");
    }
    for p in [r"C:\Windows\System32\notepad.exe", r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE", "Code.exe"] {
        assert!(!is_runner(p), "{p}");
    }
}

#[test]
fn paths_are_tidied_as_written() {
    assert_eq!(tidy_path(Path::new(r"C:\Tools\cursor\resources\app\bin\..\..\..\Cursor.exe")), PathBuf::from(r"C:\Tools\cursor\Cursor.exe"));
    assert_eq!(tidy_path(Path::new(r"C:\Zed\bin\.\..\Zed.exe")), PathBuf::from(r"C:\Zed\Zed.exe"));
}
