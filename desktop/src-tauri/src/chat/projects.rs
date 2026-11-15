//! **Projects** (15 Nov 2026; the Claude app's projects, from Anthropic's
//! help pages): a named group of chats that share **instructions** and
//! **files** — the project's knowledge — which every chat in it gets. Chat's
//! side only (a Code chat has its workspace).
//!
//! - **A project** ([`Project`]): its name, a line saying what it's for, its
//!   instructions (how to answer in it, at most [`INSTRUCTIONS_MAX`]
//!   characters) and its files. Made and changed on its own page in Chat
//!   (Projects in the conversation list); a chat joins one by starting there
//!   ("New chat in …") or from its ⋯ (Move to project ▸), and leaves it the
//!   same way. Deleting a project keeps its chats (they leave it) and
//!   deletes its files' copies.
//! - **Its files**: read with the same rules as a question's attachments
//!   (`chat/attachments.rs` `read`: text, code, Markdown, a PDF's or a Word
//!   document's text, read on this PC; anything else said plainly), their
//!   copy and their text kept in `<data>/projects/<project>/`. At most
//!   [`MAX_FILES`] a project.
//! - **In every chat of the project** ([`prompt_block`]): one block in the
//!   system prompt — the project's name, its instructions and its files'
//!   text in a `<documents>` block, as attachments go — stable from turn to
//!   turn (a model re-uses its cache), before the chat's style and About
//!   you. Sized to the model ([`knowledge_budget`]): the files take at most
//!   half its context; those that don't fit are named, with a line saying
//!   so (never cut mid-file). The fixed part a question must fit beside
//!   counts it (`fit::Parts::texts`). On your Claude plan, the session's own
//!   prompt has it; a question asked out loud into the chat too.
//! - **Where it lives**: `projects.json` in Yap's data folder, a file of
//!   its own (an older Yap never reads or writes it): the projects and each
//!   chat's project by id; fields a newer Yap adds are kept. A deleted
//!   chat's place goes with it.
//!
//! The log says counts, sizes and tokens, never names or text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// A project's name: at most this many characters.
pub const NAME_MAX: usize = 60;
/// What it's for: at most this many characters.
pub const DESCRIPTION_MAX: usize = 300;
/// Its instructions: at most this many characters.
pub const INSTRUCTIONS_MAX: usize = 8_000;
/// Files a project keeps, at most.
pub const MAX_FILES: usize = 50;
/// Projects, at most.
pub const MAX_PROJECTS: usize = 100;
/// The folder under Yap's data folder.
pub const FOLDER: &str = "projects";

/// One file of a project's: what projects.json keeps (never its text).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectFile {
    /// `pf_` + 16 hex digits.
    pub id: String,
    pub name: String,
    /// "text" | "code" | "markdown" | "pdf" | "word".
    pub kind: String,
    pub bytes: u64,
    /// Its text's tokens, estimated.
    pub tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    pub added_ms: u64,
}

/// A project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    /// `p-` + its making's milliseconds.
    pub id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub files: Vec<ProjectFile>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

/// projects.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Store {
    projects: Vec<Project>,
    /// Each chat's project, by conversation id.
    chats: BTreeMap<String, String>,
    /// Fields a newer Yap wrote, kept as they were.
    #[serde(flatten)]
    other: Map<String, Value>,
}

impl Store {
    fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
    fn of(&self, conv: u64) -> Option<&Project> {
        self.chats.get(&conv.to_string()).and_then(|id| self.project(id))
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---- the file -----------------------------------------------------------------------------------

#[cfg(not(test))]
static STORE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> PathBuf {
    crate::config::data_dir().join("projects.json")
}

/// projects.json's saves (`saving.rs`): the person's own projects, so a
/// failed save is said, and tried again with the next.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("projects", "your projects", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| write(s));
    SAVER.check()
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        write(store);
    }
    f(store)
}

#[cfg(not(test))]
fn write(s: &Store) {
    let _ = SAVER.write_json(&path(), s, true);
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
    static TEST_DIR: PathBuf = {
        let mut raw = [0u8; 8];
        let _ = getrandom::fill(&mut raw);
        std::env::temp_dir().join(format!("yap-projects-test-{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())).join(FOLDER)
    };
}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(test)]
fn write(_: &Store) {}

/// Where projects' files live.
pub fn dir() -> PathBuf {
    #[cfg(test)]
    {
        TEST_DIR.with(|d| d.clone())
    }
    #[cfg(not(test))]
    {
        crate::config::data_dir().join(FOLDER)
    }
}

/// Whether `id` is a project id (and nothing else: no path hides in one).
fn is_project_id(id: &str) -> bool {
    id.len() > 2 && id.starts_with("p-") && id[2..].bytes().all(|b| b.is_ascii_digit() || b == b'x')
}

/// Whether `id` is a project file's id.
fn is_file_id(id: &str) -> bool {
    id.len() == 19 && id.starts_with("pf_") && id[3..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn new_file_id() -> String {
    let mut raw = [0u8; 8];
    if getrandom::fill(&mut raw).is_err() {
        raw = now_ms().to_le_bytes();
    }
    format!("pf_{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// A file's text, as kept.
fn text_path(project: &str, file: &str) -> Option<PathBuf> {
    (is_project_id(project) && is_file_id(file)).then(|| dir().join(project).join(format!("{file}.txt")))
}

/// A project file's text. Reads it.
pub fn file_text(project: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(text_path(project, file)?).ok()
}

// ---- the prompt ---------------------------------------------------------------------------------

/// **How much of a model's context a project's files may take**: half of
/// it (the rest: the chat, the question, the answer). Pure.
pub fn knowledge_budget(context: u64) -> u64 {
    context / 2
}

/// **The block** for project `p`, its files' texts given (`None`: gone)
/// and at most `budget` tokens of them (files that don't fit are named,
/// never cut). Pure.
pub fn block_of(p: &Project, texts: &[(ProjectFile, Option<String>)], budget: u64) -> String {
    let mut out = format!("This chat is in the user's project \"{}\".", p.name.trim());
    if !p.description.trim().is_empty() {
        out.push_str(&format!(" It's for: {}", p.description.trim()));
    }
    if !p.instructions.trim().is_empty() {
        out.push_str(&format!("\nThe project's instructions (follow them in every answer in this chat):\n<project_instructions>\n{}\n</project_instructions>", p.instructions.trim()));
    }
    let mut left = budget;
    let mut kept: Vec<(&ProjectFile, &str)> = Vec::new();
    let mut out_of_room: Vec<&str> = Vec::new();
    let mut gone: Vec<&str> = Vec::new();
    for (f, text) in texts {
        match text {
            None => gone.push(&f.name),
            Some(t) if f.tokens <= left => {
                left -= f.tokens;
                kept.push((f, t));
            }
            Some(_) => out_of_room.push(&f.name),
        }
    }
    if !kept.is_empty() {
        out.push_str("\nThe project's files (its knowledge: use them when they help, and say which file you used):\n<documents>\n");
        for (i, (f, text)) in kept.iter().enumerate() {
            let source = match f.kind.as_str() {
                "pdf" => format!("{} (a PDF: its text)", f.name),
                "word" => format!("{} (a Word document: its text)", f.name),
                _ => f.name.clone(),
            };
            out.push_str(&format!("<document index=\"{}\">\n<source>{source}</source>\n<document_content>\n{}\n</document_content>\n</document>\n", i + 1, text.trim_end()));
        }
        out.push_str("</documents>");
    }
    if !out_of_room.is_empty() {
        out.push_str(&format!("\nThe project also has {} that didn't fit in this model's memory: {}. Say so if the answer may be in them.", if out_of_room.len() == 1 { "a file" } else { "files" }, out_of_room.join(", ")));
    }
    if !gone.is_empty() {
        out.push_str(&format!("\n(The project's {} {} gone from this PC.)", gone.join(", "), if gone.len() == 1 { "is" } else { "are" }));
    }
    out
}

tokio::task_local! {
    /// The project a new chat's first question starts in (`chat_send`'s
    /// `project`), if any.
    static NEW: Option<String>;
}

/// `f` run as a new chat's first question, in project `project`.
pub async fn with_new<F: std::future::Future>(project: Option<String>, f: F) -> F::Output {
    NEW.scope(project, f).await
}

fn new_chat() -> Option<String> {
    NEW.try_with(Clone::clone).ok().flatten()
}

/// The project conversation `conv` is in (`None`: the new chat being made, its page's).
pub fn of_conversation(conv: Option<u64>) -> Option<Project> {
    with_store(|s| match conv {
        Some(id) => s.of(id).cloned(),
        None => new_chat().and_then(|p| s.project(&p).cloned()),
    })
}

/// **The project block for conversation `conv`'s system prompt**, sized
/// to a model of `context` tokens; `None` when it's in none. Reads the
/// files' texts.
pub fn prompt_block(conv: Option<u64>, context: u64) -> Option<String> {
    let p = of_conversation(conv)?;
    let texts: Vec<(ProjectFile, Option<String>)> = p.files.iter().map(|f| (f.clone(), file_text(&p.id, &f.id))).collect();
    Some(block_of(&p, &texts, knowledge_budget(context)))
}

/// **A new chat was made** (`turn_conversation`): it's in the project its
/// page started it in.
pub fn adopt_new(conv: u64) {
    let Some(project) = new_chat() else { return };
    with_store(|s| {
        if s.project(&project).is_some() {
            s.chats.insert(conv.to_string(), project);
            write(s);
        }
    });
}

/// Chats `ids` were deleted (or an incognito one ended): they leave their projects.
pub fn chats_deleted(ids: &[u64]) {
    with_store(|s| {
        let before = s.chats.len();
        for id in ids {
            s.chats.remove(&id.to_string());
        }
        if s.chats.len() != before {
            write(s);
        }
    });
}

/// Reset app / Delete some of your data → Chats: every chat leaves its
/// project (the projects stay).
pub fn reset_chats() {
    with_store(|s| {
        if !s.chats.is_empty() {
            s.chats.clear();
            write(s);
        }
    });
}

/// **Download your data**: projects.json as it is, when there's a project.
pub fn snapshot() -> Option<Value> {
    with_store(|s| (!s.projects.is_empty()).then(|| serde_json::to_value(&*s).ok()).flatten())
}

// ---- tidying what the person typed ---------------------------------------------------------------

/// Whitespace runs as one space, at most `max` characters. Pure.
fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// Text kept as written (its line breaks too), at most `max` characters. Pure.
fn tidy(s: &str, max: usize) -> String {
    let t = s.replace("\r\n", "\n");
    t.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().chars().take(max).collect()
}

// ---- the commands -------------------------------------------------------------------------------

/// A project as the list shows it (and a chat's ⋯, which ticks the one it's
/// in): no instructions or file names.
fn summary_of(p: &Project, chat_ids: Vec<u64>) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "description": p.description,
        "files": p.files.len(),
        "chats": chat_ids.len(),
        "chatIds": chat_ids,
        "updatedMs": p.updated_ms,
        "createdMs": p.created_ms,
    })
}

/// **The projects** (Chat's Projects page), last changed first.
#[tauri::command]
pub fn projects_list() -> Value {
    with_store(|s| {
        let mut ps: Vec<&Project> = s.projects.iter().collect();
        ps.sort_by_key(|p| std::cmp::Reverse(p.updated_ms));
        let rows: Vec<Value> = ps.iter().map(|p| summary_of(p, s.chats.iter().filter(|(_, v)| **v == p.id).filter_map(|(k, _)| k.parse().ok()).collect())).collect();
        json!(rows)
    })
}

/// **One project** (its page): the whole project, and its chats' ids.
#[tauri::command]
pub fn project_get(id: String) -> Result<Value, String> {
    with_store(|s| {
        let p = s.project(&id).ok_or("That project isn't there any more.")?;
        let chats: Vec<u64> = s.chats.iter().filter(|(_, v)| **v == id).filter_map(|(k, _)| k.parse().ok()).collect();
        Ok(json!({ "project": p, "chats": chats }))
    })
}

/// **A new project**: its name and what it's for.
#[tauri::command]
pub fn project_create(name: String, description: Option<String>) -> Result<Project, String> {
    let name = one_line(&name, NAME_MAX);
    if name.is_empty() {
        return Err("Give the project a name.".to_string());
    }
    let made = with_store(|s| {
        if s.projects.len() >= MAX_PROJECTS {
            return Err(format!("You have {MAX_PROJECTS} projects: delete one first."));
        }
        let now = now_ms();
        let mut id = format!("p-{now}");
        while s.project(&id).is_some() {
            id.push('x');
        }
        let p = Project { id, name, description: one_line(description.as_deref().unwrap_or(""), DESCRIPTION_MAX), instructions: String::new(), files: Vec::new(), created_ms: now, updated_ms: now };
        s.projects.push(p.clone());
        write(s);
        Ok(p)
    })?;
    tracing::info!("Chat: a project made");
    SAVER.check()?;
    Ok(made)
}

/// **A project changed**: its name, what it's for, its instructions (each
/// only when given).
#[tauri::command]
pub fn project_update(id: String, name: Option<String>, description: Option<String>, instructions: Option<String>) -> Result<Project, String> {
    let out = with_store(|s| {
        let p = s.projects.iter_mut().find(|p| p.id == id).ok_or("That project isn't there any more.")?;
        if let Some(n) = name {
            let n = one_line(&n, NAME_MAX);
            if n.is_empty() {
                return Err("Give the project a name.".to_string());
            }
            p.name = n;
        }
        if let Some(d) = description {
            p.description = one_line(&d, DESCRIPTION_MAX);
        }
        if let Some(i) = instructions {
            p.instructions = tidy(&i, INSTRUCTIONS_MAX);
        }
        p.updated_ms = now_ms();
        let out = p.clone();
        write(s);
        Ok(out)
    })?;
    tracing::info!(instructions = out.instructions.chars().count(), "Chat: a project changed");
    SAVER.check()?;
    Ok(out)
}

/// **Delete a project**: its chats stay (they leave it), its files' copies go.
#[tauri::command]
pub fn project_delete(id: String) -> Result<(), String> {
    let gone = with_store(|s| {
        let before = s.projects.len();
        s.projects.retain(|p| p.id != id);
        if s.projects.len() == before {
            return false;
        }
        s.chats.retain(|_, v| *v != id);
        write(s);
        true
    });
    if gone && is_project_id(&id) {
        let _ = std::fs::remove_dir_all(dir().join(&id));
        tracing::info!("Chat: a project deleted");
    }
    Ok(SAVER.check()?)
}

/// A file Yap couldn't add, and why (a plain sentence).
#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub name: String,
    pub why: String,
}

/// **A file added to project `project`** from its bytes: read (the
/// attachments' rules), its copy and its text kept. Blocking.
pub fn add_bytes(project: &str, name: &str, bytes: Vec<u8>) -> Result<ProjectFile, String> {
    if !is_project_id(project) {
        return Err("That project isn't there any more.".to_string());
    }
    let full = with_store(|s| s.project(project).map(|p| p.files.len() >= MAX_FILES));
    match full {
        None => return Err("That project isn't there any more.".to_string()),
        Some(true) => return Err(format!("A project keeps {MAX_FILES} files at most.")),
        Some(false) => {}
    }
    let size = bytes.len() as u64;
    let copy = bytes.clone();
    let read = crate::chat::attachments::read(name, bytes)?;
    let f = ProjectFile {
        id: new_file_id(),
        name: name.to_string(),
        kind: read.kind.to_string(),
        bytes: size,
        tokens: crate::chat::budget::estimate(&read.text),
        pages: read.pages,
        added_ms: now_ms(),
    };
    let folder = dir().join(project);
    let kept = std::fs::create_dir_all(folder.join(&f.id))
        .map_err(|e| e.to_string())
        .and_then(|_| crate::config::atomic_write(&folder.join(format!("{}.txt", f.id)), &read.text).map_err(|e| e.to_string()))
        .and_then(|_| crate::config::atomic_write_bytes(&folder.join(&f.id).join(crate::chat::attachments::safe_name(name)), &copy).map_err(|e| e.to_string()));
    if let Err(e) = kept {
        remove_files(project, std::slice::from_ref(&f.id));
        return Err(format!("Yap couldn't keep {name}: {e}"));
    }
    let added = with_store(|s| {
        let Some(p) = s.projects.iter_mut().find(|p| p.id == project) else { return false };
        p.files.push(f.clone());
        p.updated_ms = now_ms();
        write(s);
        true
    });
    if !added {
        remove_files(project, std::slice::from_ref(&f.id));
        return Err("That project isn't there any more.".to_string());
    }
    tracing::info!(kind = f.kind, bytes = f.bytes, tokens = f.tokens, "Chat: a file added to a project");
    Ok(f)
}

/// A file from its path. Blocking.
fn add_path(project: &str, path: &Path) -> Result<ProjectFile, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "That file".into());
    let meta = std::fs::metadata(path).map_err(|_| format!("{name} isn't there any more."))?;
    if meta.is_dir() {
        return Err(format!("{name} is a folder: add the files in it instead."));
    }
    if meta.len() > crate::chat::attachments::MAX_BYTES {
        return Err(format!("{name} is over {} MB.", crate::chat::attachments::MAX_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("Yap couldn't open {name}: another app may have it open."))?;
    add_bytes(project, &name, bytes)
}

/// Delete files' copies and texts. Their ids are checked.
fn remove_files(project: &str, ids: &[String]) {
    if !is_project_id(project) {
        return;
    }
    for id in ids.iter().filter(|i| is_file_id(i)) {
        let folder = dir().join(project);
        let _ = std::fs::remove_file(folder.join(format!("{id}.txt")));
        let _ = std::fs::remove_dir_all(folder.join(id));
    }
}

/// **Add files to a project** (its page's Add files…, or files dropped on
/// it): each read and kept, or why not.
#[tauri::command]
pub async fn project_files_add(id: String, paths: Vec<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut added = Vec::new();
        let mut skipped = Vec::new();
        for p in paths {
            let path = PathBuf::from(&p);
            match add_path(&id, &path) {
                Ok(f) => added.push(f),
                Err(why) => skipped.push(Skipped { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(p), why }),
            }
        }
        let _ = SAVER.check();
        Ok(json!({ "added": added, "skipped": skipped }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Take a file out of a project**: its copy and text go.
#[tauri::command]
pub fn project_file_remove(id: String, file: String) -> Result<(), String> {
    let had = with_store(|s| {
        let Some(p) = s.projects.iter_mut().find(|p| p.id == id) else { return false };
        let before = p.files.len();
        p.files.retain(|f| f.id != file);
        if p.files.len() == before {
            return false;
        }
        p.updated_ms = now_ms();
        write(s);
        true
    });
    if had {
        remove_files(&id, std::slice::from_ref(&file));
    }
    Ok(SAVER.check()?)
}

/// **Chat `conversation`'s project** (`None`: a new chat's, none): `{id, name}`.
#[tauri::command]
pub fn chat_project_of(conversation: Option<u64>) -> Option<Value> {
    let id = conversation?;
    with_store(|s| s.of(id).map(|p| json!({ "id": p.id, "name": p.name })))
}

/// **Move a chat into a project**, or out of its project (`project` `None`).
/// Never a Code chat, nor an incognito one.
#[tauri::command]
pub fn chat_project_set(conversation: u64, project: Option<String>) -> Result<(), String> {
    if crate::agents::code_chat::folder_of(Some(conversation)).is_some() {
        return Err("A Code chat has its workspace: it can't go in a project.".to_string());
    }
    if crate::chat::chats::is_incognito(conversation) {
        return Err("An incognito chat isn't kept, so it can't go in a project.".to_string());
    }
    with_store(|s| {
        let key = conversation.to_string();
        match project {
            Some(p) => {
                if s.project(&p).is_none() {
                    return Err("That project isn't there any more.".to_string());
                }
                s.chats.insert(key, p);
            }
            None => {
                s.chats.remove(&key);
            }
        }
        write(s);
        Ok(())
    })?;
    Ok(SAVER.check()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, tokens: u64) -> ProjectFile {
        ProjectFile { id: "pf_0000000000000001".into(), name: name.into(), kind: "text".into(), tokens, ..Default::default() }
    }

    #[test]
    fn the_block_has_the_instructions_and_the_files_that_fit() {
        let p = Project { name: "Garden 2027".into(), description: "Planning the allotment".into(), instructions: "Use metric units.\nKeep it short.".into(), ..Default::default() };
        let texts = vec![(file("sowing.md", 40), Some("Beans in May.".to_string())), (file("big.pdf", 900), Some("…".to_string())), (file("old.txt", 5), None)];
        let b = block_of(&p, &texts, 100);
        assert!(b.starts_with("This chat is in the user's project \"Garden 2027\". It's for: Planning the allotment"), "{b}");
        assert!(b.contains("<project_instructions>\nUse metric units.\nKeep it short.\n</project_instructions>"), "{b}");
        assert!(b.contains("<source>sowing.md</source>\n<document_content>\nBeans in May.\n</document_content>"), "{b}");
        assert!(b.contains("didn't fit in this model's memory: big.pdf."), "{b}");
        assert!(b.contains("(The project's old.txt is gone from this PC.)"), "{b}");
        assert!(!b.contains("…"), "a file that doesn't fit is never cut into: {b}");
        // No instructions, no files: the name alone.
        assert_eq!(block_of(&Project { name: "Bare".into(), ..Default::default() }, &[], 100), "This chat is in the user's project \"Bare\".");
        assert_eq!(knowledge_budget(200_000), 100_000);
    }

    #[test]
    fn a_project_is_made_changed_and_its_files_read_with_the_attachments_rules() {
        let p = project_create("  Garden   2027 ".into(), Some("The allotment".into())).unwrap();
        assert_eq!(p.name, "Garden 2027");
        assert!(project_create("   ".into(), None).is_err());
        let p = project_update(p.id.clone(), None, None, Some("Use metric units.\r\n".into())).unwrap();
        assert_eq!(p.instructions, "Use metric units.");
        let f = add_bytes(&p.id, "sowing.md", b"# Sowing\nBeans in May.".to_vec()).unwrap();
        assert_eq!(f.kind, "markdown");
        assert_eq!(file_text(&p.id, &f.id).as_deref(), Some("# Sowing\nBeans in May."));
        // A file the attachments can't read is said, never added.
        assert!(add_bytes(&p.id, "seeds.xlsx", vec![0x50, 0x4b, 3, 4, 0, 0]).is_err());
        // A chat joins it; its block has the file; deleting the chat takes it out.
        chat_project_set(5, Some(p.id.clone())).unwrap();
        let b = prompt_block(Some(5), 10_000).unwrap();
        assert!(b.contains("Use metric units.") && b.contains("Beans in May."), "{b}");
        assert_eq!(prompt_block(Some(6), 10_000), None);
        chats_deleted(&[5]);
        assert_eq!(prompt_block(Some(5), 10_000), None);
        // A file taken out goes from the disk too.
        project_file_remove(p.id.clone(), f.id.clone()).unwrap();
        assert_eq!(file_text(&p.id, &f.id), None);
        // Deleted: its chats leave it.
        chat_project_set(7, Some(p.id.clone())).unwrap();
        project_delete(p.id.clone()).unwrap();
        assert!(chat_project_of(Some(7)).is_none());
        assert!(project_get(p.id).is_err());
        let _ = std::fs::remove_dir_all(dir());
    }

    #[tokio::test]
    async fn a_new_chat_joins_the_project_its_page_started_it_in() {
        let p = project_create("Trip".into(), None).unwrap();
        with_new(Some(p.id.clone()), async { adopt_new(11) }).await;
        assert_eq!(chat_project_of(Some(11)).and_then(|v| v["name"].as_str().map(String::from)).as_deref(), Some("Trip"));
        with_new(None, async { adopt_new(12) }).await;
        assert!(chat_project_of(Some(12)).is_none());
    }

    #[test]
    fn ids_never_hide_a_path() {
        assert!(is_project_id("p-1731661234567") && is_project_id("p-17x"));
        assert!(!is_project_id("p-..\\x") && !is_project_id("../p-1"));
        assert!(is_file_id("pf_0123456789abcdef") && !is_file_id("pf_../../x"));
        assert_eq!(text_path("p-1", "../x"), None);
    }

    #[test]
    fn the_file_keeps_a_newer_yaps_fields() {
        let s: Store = serde_json::from_str(r#"{"projects":[{"id":"p-1","name":"Garden"}],"chats":{"3":"p-1"},"future":true}"#).unwrap();
        assert_eq!(s.of(3).map(|p| p.name.as_str()), Some("Garden"));
        assert_eq!(serde_json::to_value(&s).unwrap()["future"], true);
    }
}
