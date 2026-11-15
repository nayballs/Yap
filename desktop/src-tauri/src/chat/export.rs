//! **Export a chat** (15 Nov 2026; the Claude app's Share, made local): a
//! conversation's ⋯ → **Export…** writes it as a Markdown file where the
//! person chooses, its pictures beside it — never uploaded, nothing to
//! share a link to.
//!
//! - **The file** ([`render`]): the title, when it was exported, then each
//!   message under who wrote it and when ("**You** · 15 Nov 2026, 10:42";
//!   an answer also names its model), its words as they were written — an
//!   answer's markdown kept, the person's own line breaks kept — then its
//!   pictures, its Sources as numbered links and the files it shared (a
//!   link to each one's path on this PC). An agent's report says it's one;
//!   an answer stopped says so. Never its thinking or its steps' chips:
//!   what the conversation said, not how.
//! - **Its pictures**: the question's screenshots, the pictures an answer
//!   showed or shared, and its pages' pictures, copied into a folder beside
//!   the file ("Trip plan pictures/1.png", numbered in the order they
//!   show) and linked from where they were. A picture whose copy is gone
//!   (kept past "Keep chat screenshots") is a line that says so; a shot
//!   taken for the model only (a step's) never goes in. No folder when
//!   there are none.
//! - Written in the background ([`chat_export`]); the toast says where and
//!   how many pictures. The log says counts only.

use serde_json::{json, Value};

use crate::chat::chats::{ChatMessage, Conversation};

/// One picture the file links to: its `chat-images` id and the name its
/// copy gets in the folder beside the file ("3.png").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub id: String,
    pub file: String,
}

/// What [`render`] makes: the Markdown, and the pictures to copy beside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Export {
    pub markdown: String,
    pub pictures: Vec<Picture>,
}

/// A picture a message shows, in the order it shows them.
struct Shown {
    id: String,
    alt: String,
}

/// The pictures message `m` shows, in order: its screenshots, then the
/// pictures it showed beside the chat (never a shot for the model: a
/// step's), the ones it shared, and its pages' pictures.
fn shown(m: &ChatMessage) -> Vec<Shown> {
    let mut out = Vec::new();
    for (i, img) in m.images.iter().enumerate() {
        out.push(Shown { id: img.id.clone(), alt: format!("Screenshot {}", i + 1) });
    }
    for p in m.pictures.iter().filter(|p| p.chip.is_empty()) {
        let alt = [p.caption.trim(), p.alt.trim()].into_iter().find(|s| !s.is_empty()).unwrap_or("Picture");
        out.push(Shown { id: p.id.clone(), alt: alt.to_string() });
    }
    for item in m.files.iter().flat_map(|s| s.items.iter()).filter(|i| i.renders()) {
        out.push(Shown { id: item.id.clone(), alt: if item.name.trim().is_empty() { "Picture".into() } else { item.name.trim().to_string() } });
    }
    for page in &m.pages {
        if let Some(t) = &page.thumb {
            let alt = if page.title.trim().is_empty() { "Page" } else { page.title.trim() };
            out.push(Shown { id: t.id.clone(), alt: alt.to_string() });
        }
    }
    out
}

/// Who wrote message `m`, as the file says it.
fn who(m: &ChatMessage) -> &'static str {
    if m.kind == "agentReport" {
        return "Agent report";
    }
    match m.role.as_str() {
        "user" => "You",
        _ => "Yap",
    }
}

/// The model an answer names: its name when the model has one (the
/// on-device model's), else its id. Pure.
fn model_label(m: &ChatMessage) -> Option<String> {
    let x = m.model.as_ref()?;
    let name = if x.name.trim().is_empty() { x.id.trim() } else { x.name.trim() };
    (!name.is_empty()).then(|| name.to_string())
}

/// The person's own words as Markdown keeps them: each line break a line
/// break ("  " before it), never two lines run together. Pure.
fn keep_breaks(text: &str) -> String {
    text.trim().replace("\r\n", "\n").lines().map(str::trim_end).collect::<Vec<_>>().join("  \n")
}

/// Text for inside a link's brackets or an image's alt: one line, its
/// brackets escaped. Pure.
fn label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace('[', "\\[").replace(']', "\\]")
}

/// A relative path as a link's address: each part percent-encoded where
/// Markdown needs it (spaces, brackets, `#`, `%`), `/` between them. Pure.
pub fn link_path(path: &str) -> String {
    path.split('/')
        .map(|part| {
            let mut out = String::new();
            for c in part.chars() {
                match c {
                    ' ' => out.push_str("%20"),
                    '(' => out.push_str("%28"),
                    ')' => out.push_str("%29"),
                    '<' => out.push_str("%3C"),
                    '>' => out.push_str("%3E"),
                    '#' => out.push_str("%23"),
                    '%' => out.push_str("%25"),
                    '[' => out.push_str("%5B"),
                    ']' => out.push_str("%5D"),
                    c => out.push(c),
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A path on this PC as a `file:` link ("C:\\Notes\\a b.md" →
/// "file:///C:/Notes/a%20b.md"). Pure.
fn file_link(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.trim_start_matches('/');
    format!("file:///{}", link_path(p))
}

/// **The file**: conversation `c` as Markdown, its pictures linked inside
/// `folder` (the folder beside the file, by name: "Trip plan pictures").
/// `when` says a unix time as the file shows it; `exists` whether a
/// picture's copy is still kept; `today` the export's own date line. Pure.
pub fn render(c: &Conversation, folder: &str, today: &str, when: &dyn Fn(u64) -> String, exists: &dyn Fn(&str) -> bool) -> Export {
    let title = if c.title.trim().is_empty() { "Untitled chat" } else { c.title.trim() };
    let shown_messages: Vec<&ChatMessage> = c.messages.iter().filter(|m| !m.text.trim().is_empty() || !shown(m).is_empty()).collect();
    let count = shown_messages.len();
    let mut out = format!("# {title}\n\n_Exported from Yap · {today} · {count} message{}_\n", if count == 1 { "" } else { "s" });
    let mut pictures: Vec<Picture> = Vec::new();
    for m in shown_messages {
        let mut head = format!("**{}**", who(m));
        if m.role == "assistant" && m.kind.is_empty() {
            if let Some(model) = model_label(m) {
                head.push_str(&format!(" · {model}"));
            }
        }
        if m.ts > 0 {
            head.push_str(&format!(" · {}", when(m.ts)));
        }
        out.push_str(&format!("\n{head}\n\n"));
        // The passage a question was about, quoted above it.
        if let Some(q) = m.quote.as_ref().filter(|q| !q.text.trim().is_empty()) {
            for line in q.text.trim().lines() {
                out.push_str(&format!("> {}\n", line.trim_end()));
            }
            out.push('\n');
        }
        let text = if m.role == "user" { keep_breaks(&m.text) } else { m.text.trim().to_string() };
        if !text.is_empty() {
            out.push_str(&text);
            out.push('\n');
        }
        if m.stopped {
            out.push_str("\n_Stopped before the answer was finished._\n");
        }
        // Its pictures, numbered across the file, each copied once.
        let mut lines = Vec::new();
        for s in shown(m) {
            if !exists(&s.id) {
                lines.push("_A picture that's no longer kept._".to_string());
                continue;
            }
            let file = match pictures.iter().find(|p| p.id == s.id) {
                Some(p) => p.file.clone(),
                None => {
                    let file = format!("{}.png", pictures.len() + 1);
                    pictures.push(Picture { id: s.id.clone(), file: file.clone() });
                    file
                }
            };
            lines.push(format!("![{}]({})", label(&s.alt), link_path(&format!("{folder}/{file}"))));
        }
        if !lines.is_empty() {
            out.push('\n');
            out.push_str(&lines.join("\n\n"));
            out.push('\n');
        }
        // Its sources, as the answer numbers them (a web page's address; a
        // passage of a meeting has none to link).
        let mut sources: Vec<&crate::chat::chats::Source> = m.sources.iter().collect();
        sources.sort_by_key(|s| s.n);
        if !sources.is_empty() {
            out.push_str("\nSources:\n\n");
            for s in sources {
                let name = if s.title.trim().is_empty() { s.url.trim() } else { s.title.trim() };
                let line = if s.url.trim().starts_with("http") { format!("[{}]({})", label(name), s.url.trim()) } else { label(name) };
                let site = if s.site.trim().is_empty() || s.url.contains(s.site.trim()) { String::new() } else { format!(" — {}", s.site.trim()) };
                out.push_str(&format!("{}. {line}{site}\n", s.n.max(1)));
            }
        }
        // The files it shared that aren't pictures: a link to each one's path.
        let files: Vec<String> = m
            .files
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|i| !i.renders() && !i.path.trim().is_empty())
            .map(|i| format!("[{}]({})", label(if i.name.trim().is_empty() { i.path.trim() } else { i.name.trim() }), file_link(i.path.trim())))
            .collect();
        if !files.is_empty() {
            out.push_str(&format!("\nFiles: {}\n", files.join(", ")));
        }
    }
    Export { markdown: out, pictures }
}

/// The folder beside `md` that its pictures go in: "<its name> pictures".
pub fn pictures_folder(md: &std::path::Path) -> String {
    let stem = md.file_stem().and_then(|s| s.to_str()).unwrap_or("Chat").trim();
    format!("{} pictures", if stem.is_empty() { "Chat" } else { stem })
}

/// A unix time as the file says it: "15 Nov 2026, 10:42", this PC's time.
fn local_time(secs: u64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(secs as i64, 0).single() {
        Some(t) => t.format("%-d %b %Y, %H:%M").to_string(),
        None => String::new(),
    }
}

/// Write `export` at `path`, its pictures from `images` into the folder
/// beside it. Returns how many pictures were copied.
pub fn write(path: &std::path::Path, export: &Export, images: &std::path::Path) -> Result<usize, String> {
    let parent = path.parent().ok_or("That isn't a place to save a file.")?;
    let mut copied = 0;
    if !export.pictures.is_empty() {
        let folder = parent.join(pictures_folder(path));
        std::fs::create_dir_all(&folder).map_err(|e| format!("Couldn't make the pictures' folder: {e}"))?;
        for p in &export.pictures {
            if std::fs::copy(images.join(format!("{}.png", p.id)), folder.join(&p.file)).is_ok() {
                copied += 1;
            }
        }
    }
    std::fs::write(path, export.markdown.as_bytes()).map_err(|e| format!("Couldn't write the file: {e}"))?;
    Ok(copied)
}

/// **Export…** (a conversation's ⋯): conversation `id` as Markdown at
/// `path` (the person picked it in Save as), its pictures in the folder
/// beside it. `{ path, pictures, folder }`.
#[tauri::command]
pub async fn chat_export(id: u64, path: String) -> Result<Value, String> {
    let conv = crate::chat::chats::get(id).ok_or("That chat isn't there any more.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&path);
        let folder = pictures_folder(&path);
        let today = chrono::Local::now().format("%-d %B %Y").to_string();
        let export = render(&conv, &folder, &today, &local_time, &crate::snip::images::exists);
        let copied = write(&path, &export, &crate::snip::images::dir())?;
        tracing::info!(messages = conv.messages.len(), pictures = copied, "chat: exported as Markdown");
        Ok(json!({ "path": path.to_string_lossy(), "pictures": copied, "folder": if copied > 0 { folder } else { String::new() } }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::chats::{ChatImage, ChatModel, Source};
    use crate::chat::shares::{Share, SharedFile};

    fn when(secs: u64) -> String {
        format!("{secs} s")
    }

    fn conv(messages: Vec<ChatMessage>) -> Conversation {
        Conversation { id: 7, title: "Trip plan".into(), created_ts: 1, updated_ts: 1, messages, scope: String::new(), note: None, archived_ts: 0, summary: None, closed_tabs: Vec::new(), branches: Vec::new(), forks: Vec::new(), incognito: false }
    }

    fn user(text: &str) -> ChatMessage {
        ChatMessage { role: "user".into(), text: text.into(), ts: 10, ..Default::default() }
    }

    fn answer(text: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".into(),
            text: text.into(),
            ts: 20,
            model: Some(ChatModel { provider: "anthropic".into(), id: "claude-opus-5-5".into(), name: String::new(), on_this_pc: false }),
            ..Default::default()
        }
    }

    #[test]
    fn each_message_says_who_and_when_and_keeps_its_words() {
        let c = conv(vec![user("Where should we stop?\nWe leave at 9"), answer("## Stops\n\n- **York**, for lunch\n- Durham")]);
        let e = render(&c, "Trip plan pictures", "15 November 2026", &when, &|_| true);
        assert!(e.markdown.starts_with("# Trip plan\n\n_Exported from Yap · 15 November 2026 · 2 messages_\n"), "{}", e.markdown);
        // The person's line breaks kept; the answer's markdown as it was.
        assert!(e.markdown.contains("\n**You** · 10 s\n\nWhere should we stop?  \nWe leave at 9\n"), "{}", e.markdown);
        assert!(e.markdown.contains("\n**Yap** · claude-opus-5-5 · 20 s\n\n## Stops\n\n- **York**, for lunch\n- Durham\n"), "{}", e.markdown);
        assert!(e.pictures.is_empty());
    }

    #[test]
    fn pictures_are_numbered_copied_once_and_a_gone_one_says_so() {
        let mut q = user("What's this?");
        q.images = vec![ChatImage { id: "img_0000000000000001".into(), w: 10, h: 10, ..Default::default() }, ChatImage { id: "img_0000000000000002".into(), w: 10, h: 10, ..Default::default() }];
        let mut a = answer("A heron.");
        a.files = vec![Share {
            items: vec![
                SharedFile { name: "heron (big).png".into(), id: "img_0000000000000001".into(), kind: "image".into(), w: 10, h: 10, ..Default::default() },
                SharedFile { name: "notes.md".into(), path: "C:\\Birds\\my notes.md".into(), kind: "markdown".into(), ..Default::default() },
            ],
            ..Default::default()
        }];
        let c = conv(vec![q, a]);
        let e = render(&c, "Trip plan pictures", "today", &when, &|id| id != "img_0000000000000002");
        assert_eq!(e.pictures, vec![Picture { id: "img_0000000000000001".into(), file: "1.png".into() }]);
        assert!(e.markdown.contains("![Screenshot 1](Trip%20plan%20pictures/1.png)\n\n_A picture that's no longer kept._\n"), "{}", e.markdown);
        // The same picture shared again links to the one copy.
        assert!(e.markdown.contains("![heron (big).png](Trip%20plan%20pictures/1.png)"), "{}", e.markdown);
        // A file that isn't a picture: a link to its path on this PC.
        assert!(e.markdown.contains("\nFiles: [notes.md](file:///C:/Birds/my%20notes.md)\n"), "{}", e.markdown);
    }

    #[test]
    fn a_shot_for_the_model_stays_out_and_sources_are_numbered_links() {
        let mut a = answer("It opens at 9 [1].");
        a.pictures = vec![crate::chat::pictures::ShownPicture { id: "img_00000000000000aa".into(), chip: "Looked at the page · example.com".into(), ..Default::default() }];
        a.sources = vec![
            Source { n: 2, title: "Opening times".into(), url: "https://museum.example.com/times".into(), site: "museum.example.com".into(), ..Default::default() },
            Source { n: 1, title: "Visit [us]".into(), url: "https://museum.example.com/".into(), site: "museum.example.com".into(), ..Default::default() },
        ];
        let e = render(&conv(vec![a]), "x", "today", &when, &|_| true);
        assert!(e.pictures.is_empty(), "a step's shot is the model's, not the chat's");
        assert!(e.markdown.contains("\nSources:\n\n1. [Visit \\[us\\]](https://museum.example.com/)\n2. [Opening times](https://museum.example.com/times)\n"), "{}", e.markdown);
    }

    #[test]
    fn an_agent_report_and_a_stopped_answer_say_so() {
        let mut report = answer("Done: the readme is tidied.");
        report.kind = "agentReport".into();
        let mut stopped = answer("The first part");
        stopped.stopped = true;
        let e = render(&conv(vec![report, stopped, user("   ")]), "x", "today", &when, &|_| true);
        assert!(e.markdown.contains("\n**Agent report** · 20 s\n\nDone: the readme is tidied.\n"), "{}", e.markdown);
        assert!(e.markdown.contains("The first part\n\n_Stopped before the answer was finished._\n"), "{}", e.markdown);
        // An empty message isn't one.
        assert!(e.markdown.contains("· 2 messages_"), "{}", e.markdown);
    }

    #[test]
    fn the_folder_is_named_for_the_file_and_links_are_encoded() {
        assert_eq!(pictures_folder(std::path::Path::new("C:\\Exports\\Trip plan.md")), "Trip plan pictures");
        assert_eq!(link_path("A (draft) #2/1.png"), "A%20%28draft%29%20%232/1.png");
    }

    #[test]
    fn write_puts_the_pictures_beside_the_file() {
        let dir = std::env::temp_dir().join(format!("yap-chat-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let images = dir.join("images");
        std::fs::create_dir_all(&images).unwrap();
        std::fs::write(images.join("img_0000000000000001.png"), b"png").unwrap();
        let out = dir.join("out").join("Trip plan.md");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let export = Export { markdown: "# Trip plan\n".into(), pictures: vec![Picture { id: "img_0000000000000001".into(), file: "1.png".into() }] };
        assert_eq!(write(&out, &export, &images).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "# Trip plan\n");
        assert_eq!(std::fs::read(dir.join("out").join("Trip plan pictures").join("1.png")).unwrap(), b"png");
        // No pictures: no folder.
        let plain = dir.join("out").join("Plain.md");
        assert_eq!(write(&plain, &Export { markdown: "# Plain\n".into(), pictures: vec![] }, &images).unwrap(), 0);
        assert!(!dir.join("out").join("Plain pictures").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
