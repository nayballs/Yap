//! **Code names from an editor or a terminal** (2026-10-29; Wispr Flow's IDE
//! variable and file recognition, by behaviour, done locally): in VS Code,
//! Cursor, Windsurf (and the editors built on VS Code), Visual Studio, a
//! JetBrains IDE or a terminal, the window's text that `screen_terms.rs`
//! reads at record start also gives that dictation's **file names**
//! ("userService.ts", from the editor's tabs, its file tree, its title, a
//! terminal's screen) and **symbols** ("getUserById", "MAX_RETRY_COUNT",
//! from the code the editor shows a screen reader).
//!
//! - **Spoken forms** ([`CodeNames::respell`], pure): "user service dot ts"
//!   → `userService.ts`, "get user by id" → `getUserById`, "max retry
//!   count" → `MAX_RETRY_COUNT` — the words said, in order, spelling the
//!   name's parts (camelCase, PascalCase, snake_case, a file's extension
//!   after "dot"), joined or apart ("userservice dot ts"), one part ≥ 5
//!   letters a letter off at most. Only for a name of three parts or more,
//!   or one with a part that isn't an everyday word ("kube config" →
//!   `kubeConfig`): "user name", "file name" or "for each" stay as said
//!   in a sentence. A file is always written as named when its extension
//!   is said.
//! - **AI cleanup** is told the names a transcript seems to hold, in the
//!   line it already gets ("Spell these names as written: userService.ts.").
//! - **Bounded**: names come out of the ≤ 20,000 characters the read
//!   already has (plus, in an editor, one time-boxed UI Automation call for
//!   its tabs and tree items, `screen_terms::uia`); ≤ 100 files and 150
//!   symbols, the most frequent first.
//! - **The editor's text** is there only with its screen reader mode on
//!   (VS Code's "Screen Reader Optimized": Monaco gives UI Automation a page
//!   of code around the cursor then, an empty box otherwise). Yap doesn't
//!   ask for it: the mode makes the editor play sounds and changes a few of
//!   its ways for everyone who doesn't use a screen reader. Without it, the
//!   file names (tabs, the file tree, the title) and the symbols in the
//!   breadcrumbs and an open Outline are still read.
//!
//! [`focus_of`] tells, from the focused element's labels, whether the
//! dictation goes into the editor, its AI chat box or its terminal: file
//! tagging (`pasting/file_tags.rs`) happens only in the chat box.

use std::collections::HashMap;

use crate::pasting::app_kind::{AppKind, CodingCli};

/// Files at most, and symbols.
const MAX_FILES: usize = 100;
const MAX_SYMBOLS: usize = 150;
/// A file name's length at most.
const MAX_NAME: usize = 80;

/// The code apps whose window gives code names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    VsCode,
    Cursor,
    Windsurf,
    /// Another editor built on VS Code (Positron, Trae, Kiro, Antigravity…)
    /// or Zed.
    Editor,
    VisualStudio,
    /// A JetBrains IDE: Swing shows UI Automation nothing but its title
    /// ("Yap – userService.ts"), so the file it shows is all it gives.
    JetBrains,
    /// A terminal, and the coding CLI in it (`app_kind.rs`).
    Terminal(Option<CodingCli>),
}

impl App {
    pub fn name(self) -> &'static str {
        match self {
            App::VsCode => "VS Code",
            App::Cursor => "Cursor",
            App::Windsurf => "Windsurf",
            App::Editor => "an editor",
            App::VisualStudio => "Visual Studio",
            App::JetBrains => "a JetBrains IDE",
            App::Terminal(_) => "a terminal",
        }
    }

    /// An editor (not a terminal): its tabs and file tree are worth one
    /// more UI Automation call.
    pub fn is_editor(self) -> bool {
        !matches!(self, App::Terminal(_))
    }
}

const JETBRAINS: &[&str] = &[
    "idea", "idea64", "pycharm", "pycharm64", "webstorm", "webstorm64", "phpstorm", "phpstorm64", "rider", "rider64",
    "clion", "clion64", "goland", "goland64", "rubymine", "rubymine64", "datagrip", "datagrip64", "rustrover",
    "rustrover64", "studio", "studio64", "aqua64", "dataspell64", "fleet",
];

/// The code app a window is: by its kind (a terminal, an editor's terminal)
/// then its process name (`stem`: lower case, no ".exe"). Pure.
pub fn app_of(stem: &str, kind: AppKind) -> Option<App> {
    if let AppKind::Terminal(t) = kind {
        return Some(App::Terminal(t.coding_cli));
    }
    Some(match stem {
        "code" | "code - insiders" | "code - oss" | "vscodium" | "codium" => App::VsCode,
        "cursor" => App::Cursor,
        "windsurf" => App::Windsurf,
        "positron" | "trae" | "kiro" | "antigravity" | "azuredatastudio" | "zed" => App::Editor,
        "devenv" => App::VisualStudio,
        s if JETBRAINS.contains(&s) => App::JetBrains,
        // Robot mode's stand-in editor (an Edge page, `YAP_REALTEST_EDITORS`)
        // is Cursor.
        s if crate::shell::realtest::stand_in_editors().iter().any(|e| e == s) => App::Cursor,
        _ => return None,
    })
}

/// What the focus is in, in a code app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Focus {
    /// The code editor.
    Editor,
    /// An AI chat box (Cursor's chat and agent, Windsurf's Cascade, VS
    /// Code's Copilot Chat).
    Chat,
    /// An integrated terminal.
    Terminal,
    #[default]
    Other,
}

/// The focused element's labels, as UI Automation gives them (never
/// logged): its class (an HTML class in Chromium), name (`aria-label`),
/// help text (a placeholder) and automation id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Labels {
    pub class: String,
    pub name: String,
    pub help: String,
    pub id: String,
}

/// Words in an AI chat box's labels (lower case): VS Code's "Chat Input",
/// Cursor's chat (its Lexical box's class "aislash-editor-input"),
/// Windsurf's Cascade, an agent's or composer's box.
const CHAT_WORDS: &[&str] = &[
    "chat", "aislash", "composer", "cascade", "copilot", "ask anything", "plan, search", "@ to mention", "agent",
    "type your request",
];

/// Where the focus is (`l`: its labels) in code app `app`. Monaco's own
/// input (class "inputarea") is the code editor unless its label says it's
/// a chat's (VS Code's chat input is a Monaco editor too); xterm.js's is a
/// terminal. Pure.
pub fn focus_of(app: App, l: &Labels) -> Focus {
    if matches!(app, App::Terminal(_)) || l.class.split_whitespace().any(|c| c == "xterm-helper-textarea") {
        return Focus::Terminal;
    }
    let all = format!("{} {} {} {}", l.class, l.name, l.help, l.id).to_lowercase();
    let chat = CHAT_WORDS.iter().any(|w| all.contains(w));
    let monaco = l.class.split_whitespace().any(|c| c == "inputarea");
    if monaco {
        return if l.name.to_lowercase().contains("chat") { Focus::Chat } else { Focus::Editor };
    }
    if chat {
        Focus::Chat
    } else {
        Focus::Other
    }
}

/// A file on screen: its name, and the paths it was shown with (a
/// terminal's "src/services/userService.ts"), first seen first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeFile {
    pub name: String,
    pub paths: Vec<String>,
}

impl CodeFile {
    /// The one path it was shown with, if just one.
    pub fn path(&self) -> Option<&str> {
        match self.paths.as_slice() {
            [one] => Some(one),
            _ => None,
        }
    }
}

/// A dictation's code names (in memory for that dictation only; the log
/// has counts, never a name).
#[derive(Clone, Debug)]
pub struct CodeNames {
    pub app: App,
    pub focus: Focus,
    pub files: Vec<CodeFile>,
    pub symbols: Vec<String>,
    /// Spoken forms, longest first: what [`CodeNames::respell`] matches.
    spoken: Vec<Spoken>,
}

/// File extensions a file name may have (lower case): code, config, docs.
const EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "json", "jsonc", "rs", "py", "pyi", "go", "java", "kt",
    "kts", "swift", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "fs", "vb", "rb", "php", "html", "htm", "css",
    "scss", "sass", "less", "vue", "svelte", "astro", "md", "mdx", "txt", "yml", "yaml", "toml", "ini", "cfg",
    "conf", "xml", "sql", "sh", "bash", "zsh", "ps1", "psm1", "bat", "cmd", "lua", "dart", "scala", "clj", "ex",
    "exs", "erl", "hs", "ml", "jl", "zig", "nim", "gradle", "csproj", "sln", "lock", "proto", "graphql", "gql", "tf",
    "ipynb", "prisma", "r", "env", "xaml", "razor", "cshtml", "props", "targets",
];
/// Dot files ("dot env" → ".env").
const DOT_FILES: &[&str] = &[
    "env", "gitignore", "gitattributes", "editorconfig", "eslintrc", "prettierrc", "npmrc", "nvmrc", "dockerignore",
    "babelrc",
];
/// How an extension may be said besides its letters.
const EXT_SAID: &[(&str, &[&str])] = &[
    ("md", &["markdown"]),
    ("py", &["pie", "pi", "python"]),
    ("ts", &["typescript"]),
    ("js", &["javascript"]),
    ("json", &["jason", "jaysen"]),
    ("yaml", &["yamel"]),
    ("yml", &["yaml", "yamel"]),
    ("rs", &["rust"]),
];

/// `token`'s file, if it names one: its last path segment ("src/a/userService.ts:12:4"
/// → "userService.ts"), and the path as shown (without a line number)
/// when it had folders.
fn file_in(token: &str) -> Option<(String, Option<String>)> {
    if token.contains("://") || token.starts_with("www.") || token.contains('@') {
        return None;
    }
    // A line and column after it ("userService.ts:12:4"), not a drive ("C:\…").
    let without_line = match token.char_indices().filter(|&(_, c)| c == ':').find(|&(i, _)| i != 1) {
        Some((i, _)) if token[i + 1..].starts_with(|c: char| c.is_ascii_digit()) || i + 1 == token.len() => &token[..i],
        _ => token,
    };
    let trimmed = without_line.trim_end_matches(['.', ',', ';', ':', ')']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return None;
    }
    let ok_chars = name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if !ok_chars {
        return None;
    }
    let file = if let Some(rest) = name.strip_prefix('.') {
        DOT_FILES.contains(&rest.to_lowercase().as_str())
    } else {
        match name.rsplit_once('.') {
            Some((stem, ext)) => {
                !stem.is_empty()
                    && stem.chars().any(char::is_alphabetic)
                    && !stem.ends_with('.')
                    && EXTENSIONS.contains(&ext.to_lowercase().as_str())
            }
            None => false,
        }
    };
    if !file {
        return None;
    }
    let path = (trimmed.len() > name.len()).then(|| trimmed.to_string());
    Some((name.to_string(), path))
}

/// An identifier's parts: camelCase and PascalCase humps, snake_case's
/// words, an acronym's run ("HTTPServer" → HTTP, Server), digits apart.
pub fn parts(ident: &str) -> Vec<String> {
    let chars: Vec<char> = ident.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (k, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some(p) = cur.chars().last() {
            let next_lower = chars.get(k + 1).is_some_and(|n| n.is_lowercase());
            let split = (p.is_lowercase() && c.is_uppercase())
                || (p.is_uppercase() && c.is_uppercase() && next_lower)
                || (p.is_ascii_digit() != c.is_ascii_digit());
            if split {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// An identifier on screen worth a spoken form: two parts or more, four
/// letters or more, not an acronym alone.
fn identifier(token: &str) -> Option<Vec<String>> {
    let t = token.trim_matches(|c: char| c == '$' || c == '_');
    if t.is_empty() || !t.starts_with(|c: char| c.is_alphabetic()) {
        return None;
    }
    if !t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
        return None;
    }
    if t.chars().filter(|c| c.is_alphabetic()).count() < 4 {
        return None;
    }
    let p = parts(t);
    (p.len() >= 2).then_some(p)
}

/// The everyday-word test for a symbol's parts.
fn everyday(part: &str, is_word: &dyn Fn(&str) -> bool) -> bool {
    let lower = part.to_lowercase();
    lower.chars().any(|c| c.is_ascii_digit()) || lower.chars().count() < 3 || crate::dictionary::learn::is_frequent(&lower) || is_word(&lower)
}

/// The file names and symbols in a code app's text (`text`: the window's,
/// as read), the most frequent first. `is_word`: is this lower-case word an
/// everyday one? Pure.
pub fn extract(text: &str, is_word: &dyn Fn(&str) -> bool) -> (Vec<CodeFile>, Vec<String>) {
    let mut files: Vec<CodeFile> = Vec::new();
    let mut file_counts: HashMap<String, (usize, u32)> = HashMap::new();
    let mut symbols: HashMap<String, (u32, usize)> = HashMap::new();
    let mut order = 0usize;
    let breaks = |c: char| c.is_whitespace() || "()[]{}<>\"'`,;=|*&!?+#%^~".contains(c);
    for token in text.split(breaks).filter(|t| !t.is_empty()) {
        order += 1;
        if let Some((name, path)) = file_in(token) {
            let key = name.to_lowercase();
            match file_counts.get_mut(&key) {
                Some((i, n)) => {
                    *n += 1;
                    if let Some(p) = path {
                        if !files[*i].paths.contains(&p) {
                            files[*i].paths.push(p);
                        }
                    }
                }
                None => {
                    file_counts.insert(key, (files.len(), 1));
                    files.push(CodeFile { name, paths: path.into_iter().collect() });
                }
            }
            continue;
        }
        // Not a file: its identifiers ("this.userService.getUser",
        // "crate::config::load", "user-service").
        for piece in token.split(['.', '/', '\\', ':', '-']) {
            let Some(p) = identifier(piece) else { continue };
            let worth = p.len() >= 3 || p.iter().any(|part| !everyday(part, is_word));
            if !worth {
                continue;
            }
            let e = symbols.entry(piece.trim_matches(|c: char| c == '$' || c == '_').to_string()).or_insert((0, order));
            e.0 += 1;
        }
    }
    // The most frequent first, then the first seen.
    let mut ranked: Vec<(CodeFile, u32, usize)> =
        files.into_iter().enumerate().map(|(i, f)| {
            let n = file_counts.get(&f.name.to_lowercase()).map(|c| c.1).unwrap_or(1);
            (f, n, i)
        }).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    let files: Vec<CodeFile> = ranked.into_iter().take(MAX_FILES).map(|(f, _, _)| f).collect();
    let mut syms: Vec<(String, u32, usize)> = symbols.into_iter().map(|(s, (n, o))| (s, n, o)).collect();
    syms.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    let symbols = syms.into_iter().take(MAX_SYMBOLS).map(|(s, _, _)| s).collect();
    (files, symbols)
}

// ---- spoken forms -----------------------------------------------------------------------------

/// A name as it may be said: its parts in lower case and, for a file, the
/// extension after "dot".
#[derive(Clone, Debug)]
struct Spoken {
    term: String,
    parts: Vec<String>,
    ext: Option<String>,
    /// A dot file (".env"): "dot" first.
    dot_first: bool,
    /// The stem has dots of its own ("vite.config"): "dot" may come between
    /// its parts.
    stem_dots: bool,
    /// One part may be a letter off.
    tolerant: bool,
}

impl Spoken {
    fn file(name: &str) -> Option<Spoken> {
        if let Some(rest) = name.strip_prefix('.') {
            return Some(Spoken {
                term: name.to_string(),
                parts: Vec::new(),
                ext: Some(rest.to_lowercase()),
                dot_first: true,
                stem_dots: false,
                tolerant: false,
            });
        }
        let (stem, ext) = name.rsplit_once('.')?;
        let parts: Vec<String> = parts(stem).into_iter().map(|p| p.to_lowercase()).collect();
        if parts.is_empty() {
            return None;
        }
        let letters: usize = parts.iter().map(|p| p.len()).sum();
        Some(Spoken {
            term: name.to_string(),
            tolerant: parts.len() >= 2 && letters >= 8,
            parts,
            ext: Some(ext.to_lowercase()),
            dot_first: false,
            stem_dots: stem.contains('.'),
        })
    }

    fn symbol(name: &str) -> Option<Spoken> {
        let parts: Vec<String> = parts(name).into_iter().map(|p| p.to_lowercase()).collect();
        if parts.len() < 2 {
            return None;
        }
        let letters: usize = parts.iter().map(|p| p.len()).sum();
        Some(Spoken { term: name.to_string(), tolerant: letters >= 8, parts, ext: None, dot_first: false, stem_dots: false })
    }

    /// How many things must be said for it (its weight: longest first).
    fn weight(&self) -> usize {
        self.parts.len() + usize::from(self.ext.is_some()) * 2
    }
}

/// A word of a transcript: its byte range and its letters and digits in
/// lower case.
#[derive(Debug)]
struct Word {
    start: usize,
    end: usize,
    key: String,
}

fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(Word { start: s, end: i, key: text[s..i].to_lowercase() });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(Word { start: s, end: text.len(), key: text[s..].to_lowercase() });
    }
    out
}

const NUMBERS: &[&str] = &["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];

fn levenshtein_at_most_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

/// A match of a spoken form: the words it spans (`from..to`).
struct Hit {
    from: usize,
    to: usize,
}

/// Does the gap before word `k` hold only what may come between a name's
/// parts said aloud: spaces, a hyphen or an underscore (and, in a file's
/// stem, a dot)? Never a line break or a sentence's punctuation.
fn gap_ok(text: &str, ws: &[Word], k: usize, dots: bool) -> bool {
    let gap = &text[ws[k - 1].end..ws[k].start];
    !gap.is_empty() && gap.chars().all(|c| c == ' ' || c == '-' || c == '_' || (dots && c == '.'))
}

/// Single-letter words from `k` spelling `want` ("i d" → "id", "t s" → "ts").
fn spelled(text: &str, ws: &[Word], k: usize, want: &str) -> Option<usize> {
    let mut s = String::new();
    let mut kk = k;
    while kk < ws.len() && ws[kk].key.chars().count() == 1 && s.len() < want.len() {
        if kk > k && !gap_ok(text, ws, kk, false) {
            return None;
        }
        s.push_str(&ws[kk].key);
        kk += 1;
    }
    (s == want && kk - k >= 2).then_some(kk)
}

fn match_at(text: &str, ws: &[Word], i: usize, sp: &Spoken, tolerant: bool) -> Option<Hit> {
    let n = ws.len();
    let mut k = i;
    let mut fuzz = u8::from(tolerant && sp.tolerant);
    if sp.dot_first {
        if ws.get(k)?.key == "dot" {
            k += 1;
        } else if !text[..ws[k].start].ends_with('.') {
            return None;
        }
    }
    let mut p = 0usize;
    while p < sp.parts.len() {
        if k >= n || (k > i && !gap_ok(text, ws, k, sp.stem_dots)) {
            return None;
        }
        let w = &ws[k].key;
        if sp.stem_dots && p > 0 && w == "dot" {
            k += 1;
            continue;
        }
        // One word for one or more parts ("user", "userservice").
        let mut acc = String::new();
        let mut q = p;
        let mut joined = false;
        while q < sp.parts.len() && acc.len() < w.len() {
            acc.push_str(&sp.parts[q]);
            q += 1;
            if acc == *w {
                joined = true;
                break;
            }
        }
        if joined {
            p = q;
            k += 1;
            continue;
        }
        let part = &sp.parts[p];
        // A number said as a word.
        if part.chars().all(|c| c.is_ascii_digit()) && part.parse::<usize>().ok().and_then(|d| NUMBERS.get(d)) == Some(&w.as_str()) {
            p += 1;
            k += 1;
            continue;
        }
        // Spelled out ("i d").
        if let Some(next) = spelled(text, ws, k, part) {
            p += 1;
            k = next;
            continue;
        }
        // A letter off, once, in a long part.
        if fuzz > 0 && part.chars().count() >= 5 && levenshtein_at_most_one(w, part) {
            fuzz -= 1;
            p += 1;
            k += 1;
            continue;
        }
        return None;
    }
    if let Some(ext) = &sp.ext {
        if !sp.dot_first {
            // "dot", or a dot in the text ("userservice.ts").
            let dotted = k < n && k > i && text[ws[k - 1].end..ws[k].start] == *".";
            if k < n && ws[k].key == "dot" && (k == i || gap_ok(text, ws, k, false)) {
                k += 1;
            } else if !dotted {
                return None;
            }
        }
        if k >= n || (k > i && !gap_ok(text, ws, k, true)) {
            return None;
        }
        let said = EXT_SAID.iter().find(|(e, _)| e == ext).map(|(_, s)| *s).unwrap_or(&[]);
        if ws[k].key == *ext || said.contains(&ws[k].key.as_str()) {
            k += 1;
        } else {
            k = spelled(text, ws, k, ext)?;
        }
    }
    (k > i).then_some(Hit { from: i, to: k })
}

impl CodeNames {
    /// The code names in a code app's text (`text`, as read), with where
    /// the focus is. `is_word`: is this lower-case word an everyday one?
    pub fn new(app: App, focus: Focus, text: &str, is_word: &dyn Fn(&str) -> bool) -> CodeNames {
        let (files, symbols) = extract(text, is_word);
        CodeNames::from_lists(app, focus, files, symbols)
    }

    /// From lists already made (the tests', a test run's).
    pub fn from_lists(app: App, focus: Focus, files: Vec<CodeFile>, symbols: Vec<String>) -> CodeNames {
        let mut spoken: Vec<Spoken> = files
            .iter()
            .filter_map(|f| Spoken::file(&f.name))
            .chain(symbols.iter().filter_map(|s| Spoken::symbol(s)))
            .collect();
        // The longest first: "user service dot ts" is the file, not the class.
        spoken.sort_by_key(|s| std::cmp::Reverse(s.weight()));
        CodeNames { app, focus, files, symbols, spoken }
    }

    /// `text` with every name said aloud written as the screen writes it
    /// ("fix user service dot ts" → "fix userService.ts"). Pure.
    /// `tolerant`: one long part may be a letter off ("Catch near-misses").
    pub fn respell(&self, text: &str, tolerant: bool) -> String {
        let ws = words(text);
        let mut out = String::with_capacity(text.len());
        let mut at = 0usize;
        let mut i = 0usize;
        while i < ws.len() {
            let hit = self.spoken.iter().find_map(|sp| match_at(text, &ws, i, sp, tolerant).map(|h| (h, sp)));
            match hit {
                Some((h, sp)) => {
                    let (start, end) = (ws[h.from].start, ws[h.to - 1].end);
                    // A dot file said "dot env": the dot is in the term.
                    let start = if sp.dot_first && text[..start].ends_with('.') { start - 1 } else { start };
                    if text[start..end] != *sp.term.as_str() {
                        out.push_str(&text[at..start]);
                        out.push_str(&sp.term);
                        at = end;
                    }
                    i = h.to;
                }
                None => i += 1,
            }
        }
        out.push_str(&text[at..]);
        out
    }

    /// The names a transcript (`raw`, before cleanup) seems to say, for AI
    /// cleanup's line: at most `max`, in the order said.
    pub fn said_in(&self, raw: &str, max: usize) -> Vec<String> {
        let ws = words(raw);
        let mut found: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < ws.len() && found.len() < max {
            match self.spoken.iter().find_map(|sp| match_at(raw, &ws, i, sp, true).map(|h| (h, sp))) {
                Some((h, sp)) => {
                    if !found.contains(&sp.term) {
                        found.push(sp.term.clone());
                    }
                    i = h.to;
                }
                None => i += 1,
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests' everyday words: the built-in list and a few more.
    fn word(w: &str) -> bool {
        crate::dictionary::learn::is_frequent(w)
            || ["user", "service", "name", "file", "state", "count", "value", "data", "max", "retry", "client", "config", "each"]
                .contains(&w)
    }

    fn names(files: &[&str], symbols: &[&str]) -> CodeNames {
        CodeNames::from_lists(
            App::Cursor,
            Focus::Chat,
            files.iter().map(|f| CodeFile { name: f.to_string(), paths: Vec::new() }).collect(),
            symbols.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn identifiers_split_into_their_parts() {
        assert_eq!(parts("getUserById"), ["get", "User", "By", "Id"]);
        assert_eq!(parts("HTTPServer"), ["HTTP", "Server"]);
        assert_eq!(parts("MAX_RETRY_COUNT"), ["MAX", "RETRY", "COUNT"]);
        assert_eq!(parts("userID"), ["user", "ID"]);
        assert_eq!(parts("parseJSON2Html"), ["parse", "JSON", "2", "Html"]);
        assert_eq!(parts("user_service"), ["user", "service"]);
    }

    #[test]
    fn files_and_symbols_come_off_the_screen() {
        let screen = "userService.ts - Yap - Visual Studio Code\n\
            userService.ts, preview\nindex.ts\nREADME.md\n.env\n\
            src/services/userService.ts:12:4 error TS2339\n\
            export async function getUserById(id: string) { return this.userRepository.findOne(id); }\n\
            const MAX_RETRY_COUNT = 3; const userName = ''; items.forEach(x => x); kubeConfig.load();\n\
            See https://example.com/docs.html and jane@example.com, version 1.2.3, e.g. this.";
        let (files, symbols) = extract(screen, &word);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names[0], "userService.ts", "the most frequent first: {names:?}");
        for f in ["userService.ts", "index.ts", "README.md", ".env"] {
            assert!(names.contains(&f), "{f} in {names:?}");
        }
        assert!(!names.iter().any(|n| n.contains("example") || n.contains("docs") || n.starts_with("1.")), "{names:?}");
        let svc = files.iter().find(|f| f.name == "userService.ts").unwrap();
        assert_eq!(svc.path(), Some("src/services/userService.ts"));
        for s in ["getUserById", "MAX_RETRY_COUNT", "userRepository", "kubeConfig"] {
            assert!(symbols.contains(&s.to_string()), "{s} in {symbols:?}");
        }
        // Two everyday words make no symbol of their own: they'd catch a sentence.
        for s in ["userName", "forEach", "findOne"] {
            assert!(!symbols.contains(&s.to_string()), "{s} in {symbols:?}");
        }
    }

    #[test]
    fn a_file_said_aloud_is_written_as_named() {
        let n = names(&["userService.ts", "README.md", ".env", "vite.config.ts", "App.tsx"], &[]);
        assert_eq!(n.respell("Fix the bug in user service dot ts please.", true), "Fix the bug in userService.ts please.");
        assert_eq!(n.respell("fix userservice dot TS", true), "fix userService.ts");
        assert_eq!(n.respell("Fix user service.ts now", true), "Fix userService.ts now");
        assert_eq!(n.respell("open userservice.ts", true), "open userService.ts");
        assert_eq!(n.respell("Update the readme dot md.", true), "Update the README.md.");
        assert_eq!(n.respell("add it to the dot env file", true), "add it to the .env file");
        assert_eq!(n.respell("check vite dot config dot ts", true), "check vite.config.ts");
        assert_eq!(n.respell("check vite config dot ts", true), "check vite.config.ts");
        assert_eq!(n.respell("the app dot t s x file", true), "the App.tsx file");
        // A letter off in a long part ("Catch near-misses"; off: exact only).
        assert_eq!(n.respell("fix user servise dot ts", true), "fix userService.ts");
        assert_eq!(n.respell("fix user servise dot ts", false), "fix user servise dot ts");
        // No extension said: not the file.
        assert_eq!(n.respell("the user service is down", true), "the user service is down");
        // A sentence's end between them: not one name.
        assert_eq!(n.respell("Ask the user. Service dot ts is fine.", true), "Ask the user. Service dot ts is fine.");
    }

    #[test]
    fn a_symbol_said_aloud_is_written_as_the_code_has_it() {
        let n = names(&[], &["getUserById", "MAX_RETRY_COUNT", "kubeConfig", "parseJSON2Html"]);
        assert_eq!(n.respell("Call get user by id first.", true), "Call getUserById first.");
        assert_eq!(n.respell("set max retry count to five", true), "set MAX_RETRY_COUNT to five");
        assert_eq!(n.respell("load the kube config", true), "load the kubeConfig");
        assert_eq!(n.respell("use parse json two html", true), "use parseJSON2Html");
        assert_eq!(n.respell("use parse json 2 html", true), "use parseJSON2Html");
        assert_eq!(n.respell("get user by I D", true), "getUserById");
        // Already written so: unchanged.
        assert_eq!(n.respell("Call getUserById first.", true), "Call getUserById first.");
        // Everyday sentences stay.
        for s in ["Get the user by Friday.", "The kube is broken.", "Retry count is fine."] {
            assert_eq!(n.respell(s, true), s);
        }
    }

    #[test]
    fn the_file_wins_over_its_class() {
        let n = names(&["UserService.cs"], &["UserServiceFactory"]);
        assert_eq!(n.respell("open user service dot cs", true), "open UserService.cs");
        assert_eq!(n.respell("call user service factory", true), "call UserServiceFactory");
    }

    #[test]
    fn cleanup_hears_the_names_said() {
        let n = names(&["userService.ts"], &["getUserById"]);
        assert_eq!(n.said_in("so in user service dot ts call get user by id", 8), ["userService.ts", "getUserById"]);
        assert!(n.said_in("nothing here", 8).is_empty());
    }

    #[test]
    fn where_the_focus_is() {
        let l = |class: &str, name: &str| Labels { class: class.into(), name: name.into(), ..Labels::default() };
        // VS Code: Monaco's input is the editor, unless it's the chat's.
        assert_eq!(focus_of(App::VsCode, &l("inputarea monaco-mouse-cursor-text", "Editor content;Press Alt+F1 for Accessibility Options.")), Focus::Editor);
        assert_eq!(focus_of(App::VsCode, &l("inputarea monaco-mouse-cursor-text", "Chat Input, Type your request")), Focus::Chat);
        // Cursor's chat box (Lexical), Windsurf's Cascade.
        assert_eq!(focus_of(App::Cursor, &l("aislash-editor-input", "")), Focus::Chat);
        assert_eq!(focus_of(App::Windsurf, &Labels { help: "Ask anything (Ctrl+L), @ to mention".into(), ..Labels::default() }), Focus::Chat);
        // The integrated terminal; anything else.
        assert_eq!(focus_of(App::Cursor, &l("xterm-helper-textarea", "Terminal 1, pwsh")), Focus::Terminal);
        assert_eq!(focus_of(App::Cursor, &l("monaco-list", "Files Explorer")), Focus::Other);
    }

    #[test]
    fn the_code_apps() {
        use crate::pasting::app_kind::{Host, Terminal};
        assert_eq!(app_of("code", AppKind::Other), Some(App::VsCode));
        assert_eq!(app_of("cursor", AppKind::Other), Some(App::Cursor));
        assert_eq!(app_of("windsurf", AppKind::Other), Some(App::Windsurf));
        assert_eq!(app_of("devenv", AppKind::Other), Some(App::VisualStudio));
        assert_eq!(app_of("idea64", AppKind::Other), Some(App::JetBrains));
        assert_eq!(app_of("notepad", AppKind::Other), None);
        let t = AppKind::Terminal(Terminal { coding_cli: Some(CodingCli::ClaudeCode), host: Host::WindowsTerminal });
        assert_eq!(app_of("windowsterminal", t), Some(App::Terminal(Some(CodingCli::ClaudeCode))));
    }

    #[test]
    fn a_long_screen_stays_bounded() {
        let mut screen = String::new();
        for i in 0..3_000 {
            screen.push_str(&format!("file{i}Thing.ts someLongName{i}Here "));
        }
        let (files, symbols) = extract(&screen, &word);
        assert_eq!(files.len(), MAX_FILES);
        assert_eq!(symbols.len(), MAX_SYMBOLS);
    }
}
