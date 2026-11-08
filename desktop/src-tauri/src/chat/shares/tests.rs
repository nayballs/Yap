use super::*;
use crate::chat::pictures::Files;

/// A fresh folder of this test's own under the temp folder.
fn folder(name: &str) -> PathBuf {
    let mut raw = [0u8; 6];
    let _ = getrandom::fill(&mut raw);
    let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join(format!("yap-shares-{tag}")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, 140, 80, 255]).collect();
    crate::snip::images::encode_png(&rgba, w, h, true).unwrap()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A workspace with a picture, notes and a to-do list, and the turn's
/// picture files for a Code chat on it (conversation `conv`).
fn garden(conv: u64) -> (PathBuf, Files) {
    let garden = folder("garden-planner");
    let docs = garden.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("concept.png"), png(48, 32)).unwrap();
    std::fs::write(docs.join("beds.png"), png(20, 30)).unwrap();
    std::fs::write(docs.join("notes.md"), "# Garden plan\n\nTomatoes by the south fence.\n").unwrap();
    std::fs::write(docs.join("todo.txt"), "Buy seeds\nTurn the compost\n").unwrap();
    crate::agents::workspaces::add(&s(&garden), None, None).unwrap();
    crate::agents::store::set_code_chat(conv, Some(&s(&garden)));
    let files = Files::new(local::Context { conversation: Some(conv), said: vec!["Show me what you made".into()] });
    (garden, files)
}

fn ids(shares: &[Share]) -> Vec<String> {
    image_ids(shares).collect()
}

#[tokio::test]
async fn a_picture_in_a_workspace_is_copied_and_shown_inline() {
    let (garden, files) = garden(61);
    let concept = s(&garden.join("docs").join("concept.png"));
    let r = files.share(&json!({ "files": [concept], "caption": "The first concept" })).await;
    assert_eq!(r.result["success"], true, "{}", r.result);
    assert_eq!(r.result["displayText"], "Shared concept.png");
    assert_eq!(r.result["data"]["shared"], "Shared concept.png in the chat");
    let share = r.share.expect("a share for the page");
    assert_eq!(share.caption, "The first concept");
    let item = &share.items[0];
    assert!(item.renders() && crate::snip::images::is_id(&item.id), "{item:?}");
    assert!(crate::snip::images::exists(&item.id), "copied into chat-images");
    assert_eq!((item.w, item.h, item.kind.as_str(), item.name.as_str()), (48, 32, "image", "concept.png"));
    assert!(item.path.to_lowercase().ends_with("docs\\concept.png") && item.folder.to_lowercase().ends_with("garden-planner\\docs"), "{item:?}");
    // The turn keeps it for the answer.
    assert_eq!(files.shared(), vec![share.clone()]);
    // The same file again (by a path inside the Code chat's workspace): the
    // copy there is, never a second one.
    let again = files.share(&json!({ "files": ["docs/concept.png"] })).await;
    assert_eq!(again.share.unwrap().items[0].id, item.id);
    // "attach": a card, linked, not copied.
    let card = files.share(&json!({ "files": [garden.join("docs").join("beds.png").to_string_lossy()], "display": "attach" })).await.share.unwrap();
    assert!(!card.items[0].renders() && card.items[0].kind == "image" && card.items[0].opens == "app", "{card:?}");
    crate::snip::images::delete(&ids(&files.shared()));
    let _ = std::fs::remove_dir_all(garden.parent().unwrap());
}

#[tokio::test]
async fn other_files_are_linked_by_their_path_whatever_their_size() {
    let (garden, files) = garden(62);
    let docs = garden.join("docs");
    // Big: linked all the same, never read.
    let big = docs.join("survey.bin");
    std::fs::File::create(&big).unwrap().set_len(local::MAX_BYTES + 10).unwrap();
    let r = files
        .share(&json!({ "files": [s(&docs.join("concept.png")), s(&docs.join("beds.png")), s(&docs.join("notes.md")), s(&docs.join("todo.txt")), s(&big)], "caption": "Garden files" }))
        .await;
    assert_eq!(r.result["success"], true, "{}", r.result);
    assert_eq!(r.result["displayText"], "Shared 2 pictures and 3 files");
    let share = r.share.unwrap();
    let card = |name: &str| share.items.iter().find(|i| i.name == name).cloned().unwrap();
    let notes = card("notes.md");
    assert!(!notes.renders() && notes.id.is_empty());
    assert_eq!((notes.kind.as_str(), notes.opens.as_str()), ("markdown", "yap"));
    assert_eq!(notes.bytes, std::fs::metadata(docs.join("notes.md")).unwrap().len());
    assert_eq!((card("todo.txt").kind.as_str(), card("todo.txt").opens.as_str()), ("text", "app"));
    assert_eq!((card("survey.bin").kind.as_str(), card("survey.bin").bytes), ("file", local::MAX_BYTES + 10));
    // The model is told what it shared, each as it shows.
    let listed = r.result["data"]["files"].as_array().unwrap();
    assert_eq!(listed[0], json!({ "name": "concept.png", "kind": "image", "shown": "in the chat" }));
    assert_eq!(listed[2], json!({ "name": "notes.md", "kind": "markdown", "shown": "as a card" }));
    // Two of them: named.
    let two = files.share(&json!({ "files": [s(&docs.join("notes.md")), s(&docs.join("todo.txt"))] })).await;
    assert_eq!(two.result["displayText"], "Shared notes.md and todo.txt");
    // A card's preview: read when it's unfolded, never kept.
    assert_eq!(preview(&docs.join("notes.md")).unwrap(), Preview { text: "# Garden plan\n\nTomatoes by the south fence.".into(), cut: false });
    assert!(!serde_json::to_string(&share).unwrap().contains("Tomatoes"), "no file's words in chats.json");
    // The page's copy: still there, what Open does; then moved.
    let mut page = vec![share.clone()];
    for_page(&mut page);
    assert!(page[0].items.iter().all(|i| !i.missing && !i.gone));
    std::fs::remove_file(docs.join("todo.txt")).unwrap();
    let mut page = vec![share.clone()];
    for_page(&mut page);
    assert!(page[0].items.iter().find(|i| i.name == "todo.txt").unwrap().missing);
    assert!(!page[0].items.iter().find(|i| i.name == "notes.md").unwrap().missing);
    crate::snip::images::delete(&ids(&files.shared()));
    let _ = std::fs::remove_dir_all(garden.parent().unwrap());
}

#[tokio::test]
async fn only_the_places_a_picture_file_may_come_from_and_nothing_else() {
    let (garden, files) = garden(63);
    let elsewhere = folder("elsewhere");
    std::fs::write(elsewhere.join("secret.txt"), "not yours").unwrap();
    // Not a workspace, an agent's or the person's: refused, the model told how.
    let r = files.share(&json!({ "files": [s(&elsewhere.join("secret.txt"))] })).await;
    assert_eq!(r.result["success"], false);
    assert_eq!(r.result["error"], local::NOT_ALLOWED_FILE);
    assert_eq!(r.result["displayText"], "Didn't share ‘secret.txt’: not a place Yap shares files from");
    assert!(r.share.is_none());
    // …however it's written.
    let out = format!("{}\\..\\..\\elsewhere\\secret.txt", s(&garden.join("docs")));
    assert_eq!(files.share(&json!({ "files": [out] })).await.result["error"], local::NOT_ALLOWED_FILE);
    // A folder; a file that isn't there; an id without the web; no files; too many.
    assert_eq!(files.share(&json!({ "files": [s(&garden.join("docs"))] })).await.result["displayText"], "Didn't share ‘docs’: a folder");
    assert_eq!(files.share(&json!({ "files": ["docs/missing.md"] })).await.result["displayText"], "Didn't share ‘missing.md’: not there");
    let id = files.share(&json!({ "files": ["p2"] })).await.result;
    assert!(id["error"].as_str().unwrap().contains("isn't a file's path") && !id["error"].as_str().unwrap().contains("web"), "{id}");
    assert_eq!(files.share(&json!({ "files": [] })).await.result["displayText"], "Didn't share anything: no files named");
    let nine: Vec<String> = (0..9).map(|i| format!("docs/{i}.txt")).collect();
    assert_eq!(files.share(&json!({ "files": nine })).await.result["displayText"], "Didn't share 9 files: 8 a call");
    // Some shared, some not: the ones it could, and why not the rest.
    let mixed = files.share(&json!({ "files": ["docs/notes.md", s(&elsewhere.join("secret.txt"))] })).await;
    assert_eq!(mixed.result["success"], true);
    assert_eq!(mixed.share.unwrap().items.len(), 1);
    assert_eq!(mixed.result["data"]["notShared"][0]["why"], local::NOT_ALLOWED_FILE);
    // Its folder written by the person: theirs to share, every file under it.
    let said = Files::new(local::Context { conversation: Some(63), said: vec![format!("share {} please", s(&elsewhere))] });
    assert_eq!(said.share(&json!({ "files": [s(&elsewhere.join("secret.txt"))] })).await.result["success"], true);
    // A file's path: that file only, never its neighbours (8 Nov 2026).
    std::fs::write(elsewhere.join("diary.txt"), "mine").unwrap();
    let one = Files::new(local::Context { conversation: Some(63), said: vec![format!("share {} please", s(&elsewhere.join("secret.txt")))] });
    assert_eq!(one.share(&json!({ "files": [s(&elsewhere.join("secret.txt"))] })).await.result["success"], true);
    let next_to_it = one.share(&json!({ "files": [s(&elsewhere.join("diary.txt"))] })).await.result;
    assert_eq!(next_to_it["error"], local::NOT_ALLOWED_FILE, "{next_to_it}");
    let _ = std::fs::remove_dir_all(garden.parent().unwrap());
    let _ = std::fs::remove_dir_all(elsewhere.parent().unwrap());
}

#[test]
fn a_preview_is_a_few_lines_of_plain_text_never_more() {
    // At most 40 lines…
    let long: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    let p = preview_of(long.as_bytes(), false).unwrap();
    assert_eq!(p.text.lines().count(), PREVIEW_LINES);
    assert!(p.cut && p.text.ends_with("line 40"));
    // …and 4 KB, a character never cut in half.
    let wide = format!("a{}", "é".repeat(5_000));
    let p = preview_of(wide.as_bytes(), true).unwrap();
    assert!(p.text.len() < PREVIEW_BYTES && p.cut && p.text.chars().skip(1).all(|c| c == 'é'), "{}", p.text.len());
    // Windows' line ends, a BOM.
    assert_eq!(preview_of(b"\xEF\xBB\xBFa\r\nb\r\n", false).unwrap(), Preview { text: "a\nb".into(), cut: false });
    // HTML stays its text: the page shows it as words, never renders it.
    assert_eq!(preview_of(b"<script>alert(1)</script>", false).unwrap().text, "<script>alert(1)</script>");
    // Not text: none.
    assert_eq!(preview_of(b"PK\x03\x04\x00\x00binary", false), None);
    assert_eq!(preview_of(b"\xff\xfe\x41\x00\x42\x00", false), None, "UTF-16 has NULs");
    assert_eq!(preview_of(b"caf\xe9 au lait", false), None, "not UTF-8");
    assert_eq!(preview_of(b"  \n \n", false), None);
    // From a file: its first 4 KB read.
    let dir = folder("preview");
    std::fs::write(dir.join("big.log"), "x".repeat(20_000)).unwrap();
    let p = preview(&dir.join("big.log")).unwrap();
    assert!(p.cut && p.text.len() == PREVIEW_BYTES);
    assert!(previews("markdown") && previews("code") && !previews("pdf") && !previews("image"));
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn a_pdfs_pages_are_counted_when_its_easy() {
    let root = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R >> endobj\n";
    assert_eq!(pdf_pages_in(root), Some(3));
    // A tree: the root's count is the largest.
    let tree = b"<< /Type /Pages /Count 12 /Kids [] >> << /Type/Pages /Count 4 /Parent 1 0 R >>";
    assert_eq!(pdf_pages_in(tree), Some(12));
    // No Pages dictionary to read: the page objects counted.
    assert_eq!(pdf_pages_in(b"<< /Type /Page >> << /Type /Page >> << /Type /Pages >>"), Some(2));
    // Compressed objects: none to see.
    assert_eq!(pdf_pages_in(b"%PDF-1.7 stream x\x9c\x01\x02 endstream"), None);
    let dir = folder("pdf");
    std::fs::write(dir.join("plan.pdf"), root).unwrap();
    std::fs::write(dir.join("fake.pdf"), "not a pdf /Type /Pages /Count 9").unwrap();
    assert_eq!(pdf_pages(&dir.join("plan.pdf")), Some(3));
    assert_eq!(pdf_pages(&dir.join("fake.pdf")), None);
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn kinds_and_what_open_does() {
    for (name, kind) in [("notes.md", "markdown"), ("todo.TXT", "text"), ("main.rs", "code"), ("plan.pdf", "pdf"), ("concept.svg", "image"), ("archive.zip", "file"), ("Makefile", "file")] {
        assert_eq!(kind_of(Path::new(name)), kind, "{name}");
    }
    // Markdown in Yap's own window; documents and pictures in their app.
    assert_eq!(opens(Path::new("C:\\p\\notes.md")), Opens::Yap);
    for app in ["plan.pdf", "concept.png", "todo.txt", "budget.xlsx", "archive.zip", "Makefile"] {
        assert_eq!(opens(Path::new(app)), Opens::App, "{app}");
    }
    // Never what runs a program, a script or a web page — whatever comes before its ending.
    for no in ["setup.exe", "run.bat", "tidy.ps1", "build.js", "do.vbs", "install.msi", "Shortcut.lnk", "page.html", "drawing.svg", "report.pdf.exe", "sneaky.exe.", "tool.py", "x.hta"] {
        assert_eq!(opens(Path::new(no)), Opens::No, "{no}");
    }
}

#[test]
fn the_tool_is_offered_where_show_picture_takes_a_path() {
    assert_eq!(Offer::of(Gate::Look, Gate::Off, true), Offer::Web);
    assert_eq!(Offer::of(Gate::Off, Gate::Show, true), Offer::Files);
    assert_eq!(Offer::of(Gate::Off, Gate::Off, true), Offer::Off);
    assert_eq!(Offer::of(Gate::Look, Gate::Off, false), Offer::Off, "a small context without it");
    assert!(Offer::Files.offers(NAME) && !Offer::Off.offers(NAME) && !Offer::Web.offers("show_picture"));
    assert_eq!(definitions(Offer::Off), json!([]));
    for offer in [Offer::Files, Offer::Web] {
        let d = &definitions(offer)[0]["function"];
        assert_eq!(d["name"], NAME);
        // Two sentences: for showing the person files in the conversation.
        let words = d["description"].as_str().unwrap();
        assert_eq!(words.matches(". ").count() + 1, 2, "{words}");
        assert!(words.contains("files in the conversation"), "{words}");
        assert_eq!(d["parameters"]["required"], json!(["files"]));
        assert_eq!(d["parameters"]["properties"]["display"]["enum"], json!(["render", "attach"]));
    }
    // Small: every step sends it.
    let est = crate::meetings::meeting_summary::estimate_tokens;
    let weight = est(&definitions(Offer::Web).to_string());
    assert!(weight <= 220, "{weight} tokens");
    // Its words in the summary.
    let f = |name: &str, id: &str| SharedFile { name: name.into(), id: id.into(), ..Default::default() };
    assert_eq!(summary(&[f("a.png", "img_1")]), "a.png");
    assert_eq!(summary(&[f("a.png", "img_1"), f("b.png", "img_2"), f("notes.md", "")]), "2 pictures and notes.md");
    assert_eq!(summary(&[f("a.png", "img_1"), f("x.md", ""), f("y.md", "")]), "a.png, x.md and y.md");
    assert_eq!(summary(&[f("x.md", ""), f("y.md", ""), f("z.md", "")]), "3 files");
}

#[test]
fn an_older_yaps_save_loses_no_share() {
    use crate::chat::chats::{ChatMessage, Conversation};
    let share = Share { caption: "Garden".into(), items: vec![SharedFile { name: "notes.md".into(), path: "C:\\Projects\\garden-planner\\notes.md".into(), kind: "markdown".into(), ..Default::default() }], ..Default::default() };
    let msg = |role: &str, ts: u64, files: Vec<Share>| ChatMessage { role: role.into(), text: "x".into(), ts, files, ..Default::default() };
    let conv = |messages: Vec<ChatMessage>| Conversation { id: 5, title: "Garden".into(), created_ts: 1, updated_ts: 1, messages, scope: String::new(), note: None, archived_ts: 0, summary: None };
    let ours = vec![conv(vec![msg("user", 100, vec![]), msg("assistant", 101, vec![share.clone()])])];
    let mirror = mirror_of(&ours);
    assert_eq!(mirror.messages.len(), 1);
    // The field as chats.json keeps it, and an older Yap's: without it.
    let kept: Value = serde_json::to_value(&ours[0].messages[1]).unwrap();
    assert_eq!(kept["files"][0]["items"][0]["path"], "C:\\Projects\\garden-planner\\notes.md");
    let mut theirs = vec![conv(vec![msg("user", 100, vec![]), msg("assistant", 101, vec![])])];
    assert_eq!(restore_from(&mut theirs, &mirror), 1);
    assert_eq!(theirs[0].messages[1].files, vec![share.clone()]);
    // Never twice, never onto another message.
    assert_eq!(restore_from(&mut theirs, &mirror), 0);
    let mut moved = vec![conv(vec![msg("assistant", 101, vec![])])];
    assert_eq!(restore_from(&mut moved, &mirror), 1, "found by its time when its place moved");
    let mut other = vec![conv(vec![msg("user", 100, vec![]), msg("assistant", 999, vec![])])];
    assert_eq!(restore_from(&mut other, &mirror), 0);
    // And a picture's copy is the sweep's to keep.
    let pic = Share { items: vec![SharedFile { name: "a.png".into(), id: "img_0123456789abcdef".into(), ..Default::default() }], ..Default::default() };
    assert_eq!(msg("assistant", 1, vec![pic]).image_ids().collect::<Vec<_>>(), ["img_0123456789abcdef"]);
}

#[test]
fn an_agents_report_shows_the_pictures_it_made_and_its_new_files() {
    let art = folder("garden-planner");
    for i in 0..6 {
        std::fs::write(art.join(format!("concept-{i}.png")), png(10 + i, 10)).unwrap();
    }
    std::fs::write(art.join("README.md"), "changed").unwrap();
    std::fs::write(art.join("plan.md"), "new").unwrap();
    std::fs::write(art.join("layout.txt"), "new").unwrap();
    let mut files: Vec<String> = (0..6).map(|i| format!("concept-{i}.png")).collect();
    files.extend(["README.md".to_string(), "plan.md".into(), "layout.txt".into()]);
    crate::agents::store::insert(crate::agents::store::Agent { id: "r1".into(), conversation: Some(77), path: s(&art), files: files.clone(), ..Default::default() });
    let places = local::Context { conversation: Some(77), said: Vec::new() }.places();
    let created = vec![crate::agents::workspaces::norm(&s(&art.join("plan.md"))), crate::agents::workspaces::norm(&s(&art.join("layout.txt")))];
    let share = of_files(&art, &files, &created, &places).expect("a share");
    let pictures: Vec<&SharedFile> = share.items.iter().filter(|i| i.renders()).collect();
    assert_eq!(pictures.len(), REPORT_MAX);
    assert_eq!(share.more_pictures, 2);
    // Its new files as cards; a file it only changed isn't one.
    let cards: Vec<&str> = share.items.iter().filter(|i| !i.renders()).map(|i| i.name.as_str()).collect();
    assert_eq!(cards, ["plan.md", "layout.txt"]);
    assert_eq!(share.more_files, 0);
    // Pictures first.
    assert!(share.items[..REPORT_MAX].iter().all(SharedFile::renders));
    // Nothing it made: none.
    assert_eq!(of_files(&art, &["README.md".to_string()], &[], &places), None);
    // Another chat's agent's files aren't this chat's.
    let others = local::Context { conversation: Some(78), said: Vec::new() }.places();
    assert_eq!(of_files(&art, &files, &created, &others), None);
    crate::snip::images::delete(&ids(&[share]));
    let _ = std::fs::remove_dir_all(art.parent().unwrap());
}
