//! **Styles by app** (2026-10-24): AI cleanup that sounds right for where
//! you're writing, with no set-up — an email formal, a work or personal
//! chat casual, a document polished, an AI prompt and a terminal command as
//! said, everything else tidied in the speaker's own words (Wispr Flow's
//! styles per kind of app, OpenWhispr's Tone & Style of 21 Oct 2026, by
//! behaviour).
//!
//! - **Kinds of app** ([`Category`]): Email, Work chat, Personal chat,
//!   Documents, AI prompts, Terminal, Everything else. Each has built-in
//!   apps ([`built_in`]: a program by its process name, a web app by the
//!   site its browser tab's title ends with — "Inbox - Gmail" — and, for
//!   AI prompts and Terminal, what `app_kind.rs` reads of a terminal: a
//!   coding CLI like Claude Code, or a shell), which the person can take
//!   out or add to (Settings).
//! - **A tone per kind** ([`Tone`]): Formal, Casual, Very casual, Natural,
//!   and for AI prompts **Literal** (never a word added — no "please" —,
//!   never rephrased, never answered), for terminals **Command** (the words
//!   as said, no capital or full stop added) or **As said** (no AI at all).
//! - **They are cleanup profiles** (`config::CleanupProfile` with a
//!   `category`): one per kind, made at load ([`ensure_profiles`]), holding
//!   its tone, the apps added and taken out, extra instructions (added to
//!   the main cleanup body, never replacing it) and the same per-profile
//!   model override any profile has. **The person's own app rules win**
//!   (Smart routing: an app bound to a profile of theirs is cleaned exactly
//!   as before, [`resolve`]), and "only clean up in apps with a rule" still
//!   means only those.
//! - **Immutable guardrails + editable body, as ever**: the system prompt is
//!   `llm::BASE_PROMPT` + the main body (+ the kind's additions); the tone
//!   is Yap's own layer, one line just before the transcript ([`line`],
//!   beside the names on screen) — so the long prompt every kind shares
//!   stays the same, and the server's cache with it: a kind's dictation
//!   reads only its line and the transcript afresh (model-evals.md).
//! - **Off for someone who chose their own body**: "Match the style to the
//!   app" (`config.cleanup_styles`) is decided once for a config that
//!   doesn't have it ([`on_load`]): on, unless the main body isn't the
//!   default one (a preset or text they picked for everywhere).
//!
//! The window's title is read only for a browser (and Windows' app frame
//! host), at the paste, never logged; the kind and tone may be (`debug`).

use serde_json::{json, Value};

use crate::app_kind::{self, AppKind};
use crate::config::{CleanupProfile, YapConfig};

/// The kinds of app a dictation's style follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Email,
    WorkChat,
    PersonalChat,
    Documents,
    AiPrompts,
    Terminal,
    Other,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Email,
        Category::WorkChat,
        Category::PersonalChat,
        Category::Documents,
        Category::AiPrompts,
        Category::Terminal,
        Category::Other,
    ];

    /// Its id in config.json (`CleanupProfile.category`) and the page.
    pub fn id(self) -> &'static str {
        match self {
            Category::Email => "email",
            Category::WorkChat => "workChat",
            Category::PersonalChat => "personalChat",
            Category::Documents => "documents",
            Category::AiPrompts => "aiPrompts",
            Category::Terminal => "terminal",
            Category::Other => "other",
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Category::Email => "Email",
            Category::WorkChat => "Work chat",
            Category::PersonalChat => "Personal chat",
            Category::Documents => "Documents",
            Category::AiPrompts => "AI prompts",
            Category::Terminal => "Terminal",
            Category::Other => "Everything else",
        }
    }

    /// Its profile's id ("style-email").
    pub fn profile_id(self) -> String {
        format!("style-{}", self.id())
    }

    /// The tones it offers, in the order the page shows them.
    pub fn tones(self) -> &'static [Tone] {
        use Tone::*;
        match self {
            Category::Email | Category::Documents => &[Formal, Casual, Natural],
            Category::WorkChat | Category::PersonalChat => &[Formal, Casual, VeryCasual],
            Category::AiPrompts => &[Literal, Natural],
            Category::Terminal => &[Command, Off],
            Category::Other => &[Natural, Formal, Casual],
        }
    }

    /// The tone it starts with.
    pub fn default_tone(self) -> Tone {
        match self {
            Category::Email | Category::Documents => Tone::Formal,
            Category::WorkChat | Category::PersonalChat => Tone::Casual,
            Category::AiPrompts => Tone::Literal,
            Category::Terminal => Tone::Command,
            Category::Other => Tone::Natural,
        }
    }

    /// What the model is told the text is (none for everything else).
    fn kind_line(self) -> Option<&'static str> {
        Some(match self {
            // "The body of an email": told "an email", Qwen3.5-4B wrapped a
            // note in "Dear Team," and "Best regards, [Your Name]", even
            // when told not to (model-evals.md "Styles by app").
            Category::Email => "This dictation is the body of an email.",
            Category::WorkChat => "This dictation is a work chat message.",
            Category::PersonalChat => "This dictation is a personal chat message.",
            Category::Documents => "This dictation is text for a document.",
            Category::AiPrompts => "This dictation is a prompt for an AI.",
            Category::Terminal => "This dictation is a terminal command.",
            Category::Other => return None,
        })
    }
}

/// How a kind's dictations are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// The main cleanup alone: the speaker's own words, tidied.
    Natural,
    Formal,
    Casual,
    VeryCasual,
    /// The words as said, in their order: fillers out, punctuation in,
    /// nothing added or rephrased (AI prompts).
    Literal,
    /// A command line: the words as said, no capital or full stop added.
    Command,
    /// No AI cleanup for this kind (the corrections and snippets still run).
    Off,
}

impl Tone {
    pub const ALL: [Tone; 7] =
        [Tone::Natural, Tone::Formal, Tone::Casual, Tone::VeryCasual, Tone::Literal, Tone::Command, Tone::Off];

    pub fn id(self) -> &'static str {
        match self {
            Tone::Natural => "natural",
            Tone::Formal => "formal",
            Tone::Casual => "casual",
            Tone::VeryCasual => "veryCasual",
            Tone::Literal => "literal",
            Tone::Command => "command",
            Tone::Off => "off",
        }
    }

    pub fn from_id(id: &str) -> Option<Tone> {
        Tone::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tone::Natural => "Natural",
            Tone::Formal => "Formal",
            Tone::Casual => "Casual",
            Tone::VeryCasual => "Very casual",
            Tone::Literal => "Literal",
            Tone::Command => "Command",
            Tone::Off => "As said",
        }
    }

    /// The tone's own sentences (none for Natural and As said).
    fn rule(self) -> Option<&'static str> {
        Some(match self {
            Tone::Formal => {
                "Tone: formal and polished: complete sentences, full punctuation, contractions written out (I will, do not), no slang."
            }
            Tone::Casual => {
                "Tone: casual and friendly: everyday words, contractions (I'll, don't), light punctuation, no semicolons."
            }
            Tone::VeryCasual => {
                "Tone: very casual, like a text to a friend: all lower case, light punctuation, no full stop at the end."
            }
            Tone::Literal => {
                "Tone: literal: keep the speaker's words in their order. Only remove fillers and false starts and fix punctuation; never add words (no please or thanks), and never rephrase, answer or carry it out."
            }
            Tone::Command => {
                "Keep it as a command line: the words as said, nothing reworded, no capital or full stop added; only remove fillers."
            }
            Tone::Natural | Tone::Off => return None,
        })
    }

    /// Whether the tone may change the speaker's wording (and so says it
    /// wins over the main body's "keep the speaker's voice").
    fn restyles(self) -> bool {
        matches!(self, Tone::Formal | Tone::Casual | Tone::VeryCasual)
    }
}

/// A dictation's style: its kind of app and that kind's tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub category: Category,
    pub tone: Tone,
}

impl Style {
    /// "email/formal", for the log and the robot's report.
    pub fn describe(self) -> String {
        format!("{}/{}", self.category.id(), self.tone.id())
    }
}

/// Said after a tone that rewrites: it wins over the main body's "preserve
/// the speaker's voice", never over the meaning.
const TONE_WINS: &str =
    "The tone wins over keeping the speaker's wording; names keep their spelling. Never change the meaning or add information.";

/// The line just before the transcript for `style` (`None`: nothing to
/// say — everything else, in its own words). Pure.
pub fn line(style: Option<Style>) -> Option<String> {
    let s = style?;
    let pick = |key: &str, ours: Option<&str>| eval_wording(key).or(ours.map(str::to_string)).filter(|t| !t.trim().is_empty());
    let mut parts: Vec<String> = Vec::new();
    parts.extend(pick(s.category.id(), s.category.kind_line()));
    parts.extend(pick(s.tone.id(), s.tone.rule()));
    if s.tone.restyles() {
        parts.extend(pick("wins", Some(TONE_WINS)));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A model evaluation's own wording for a kind's or a tone's sentence
/// (`YAP_EVAL_STYLE_LINES`: a JSON file of `{"email": "…", "formal": "…",
/// "wins": "…"}`, "" for none), to compare wordings without a rebuild.
/// Never outside a run.
fn eval_wording(key: &str) -> Option<String> {
    if !crate::eval::active() {
        return None;
    }
    let path = std::env::var_os("YAP_EVAL_STYLE_LINES")?;
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

/// The lines before the transcript: the style's, then the names on screen
/// (`screen_terms::cleanup_line`), as one (`llm::with_names_line` puts it in
/// place). Pure.
pub fn before_transcript(style: Option<&str>, names: Option<&str>) -> Option<String> {
    let lines: Vec<&str> = [style, names].into_iter().flatten().map(str::trim).filter(|l| !l.is_empty()).collect();
    (!lines.is_empty()).then(|| lines.join("\n\n"))
}

/// The cleaned text fitted to its style, for what a model may still get
/// wrong: a command (one line) without the full stop or the capital a
/// sentence would have ("Git status." → "git status"), a very casual
/// message without its final full stop, an email without a greeting or
/// sign-off the model made up ([`drop_unsaid_frame`]; `raw`: the
/// transcript). Pure.
pub fn fit(style: Option<Style>, raw: &str, text: &str) -> String {
    let Some(s) = style else { return text.to_string() };
    let text = if s.category == Category::Email { drop_unsaid_frame(raw, text) } else { text.to_string() };
    match s.tone {
        Tone::Command if !text.trim().contains('\n') => {
            let t = drop_full_stop(text.trim());
            lower_first_word(&t)
        }
        Tone::VeryCasual => drop_full_stop(no_semicolons(&text).trim_end()),
        Tone::Casual => no_semicolons(&text),
        _ => text,
    }
}

/// A casual message's semicolons as commas (the tone asks for none, and
/// the models write them anyway: "late to standup; the train's delayed") —
/// unless the text looks like code.
fn no_semicolons(text: &str) -> String {
    if text.contains(['(', ')', '=', '{', '}', '<', '>', '`']) {
        return text.to_string();
    }
    text.replace("; ", ", ")
}

/// Lower-case words (letters, digits, apostrophes).
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// An email's frame the model made up, taken off: Qwen3.5-4B told "this
/// is an email" wrote "Dear Team," over a note and "Best regards, [Your
/// Name]" under it (the model evaluations, model-evals.md "Styles by
/// app"). From the bottom, short lines (≤ 4 words) none of whose words were
/// said, and "[placeholders]"; at the top, a short greeting line none of
/// whose words were said. A greeting or sign-off that was said, restyled
/// ("hi sarah" → "Dear Sarah,", "thanks" → "Thank you,"), stays; so does
/// a text that's one line. Pure.
pub fn drop_unsaid_frame(raw: &str, text: &str) -> String {
    let said = words_of(raw);
    let was_said = |line: &str| {
        words_of(line).iter().any(|w| {
            said.iter().any(|s| s == w || (w.len() >= 4 && s.len() >= 4 && (s.starts_with(w.as_str()) || w.starts_with(s.as_str()))))
        })
    };
    let placeholder = |line: &str| {
        let l = line.trim();
        l.starts_with('[') && l.ends_with(']') && !raw.contains('[')
    };
    let made_up = |line: &str| placeholder(line) || (words_of(line).len() <= 4 && !was_said(line));
    let mut lines: Vec<&str> = text.trim().lines().collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    while lines.len() > 1 {
        match lines.last() {
            Some(l) if l.trim().is_empty() => {
                lines.pop();
            }
            Some(l) if made_up(l) => {
                lines.pop();
            }
            _ => break,
        }
    }
    let greeting = |l: &str| {
        let w = words_of(l);
        let first = w.first().map(String::as_str).unwrap_or("");
        w.len() <= 4 && (l.trim_end().ends_with(',') || matches!(first, "dear" | "hi" | "hello" | "hey" | "good"))
    };
    if lines.len() > 1 && greeting(lines[0]) && !was_said(lines[0]) {
        lines.remove(0);
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
    }
    lines.join("\n").trim().to_string()
}

/// Hesitation sounds, as speech-to-text writes them.
const FILLERS: &[&str] = &[
    "um", "umm", "ummm", "uhm", "uh", "uhh", "uhhh", "er", "erm", "err", "ah", "ahh", "eh", "hmm", "hmmm", "hm", "mm", "mmm", "mhm",
];

/// A dictation of filler alone ("Um, uh.", "uh you know um"): nothing to
/// type. "you know" counts only beside a hesitation sound. Pure.
pub fn filler_only(text: &str) -> bool {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut i = 0;
    let mut sound = false;
    while i < words.len() {
        if FILLERS.contains(&words[i].as_str()) {
            sound = true;
            i += 1;
        } else if words[i] == "you" && words.get(i + 1).is_some_and(|w| w == "know") {
            i += 2;
        } else {
            return false;
        }
    }
    sound
}

/// A text without its one final full stop (an ellipsis stays).
fn drop_full_stop(text: &str) -> String {
    match text.strip_suffix('.') {
        Some(rest) if !rest.ends_with('.') => rest.trim_end().to_string(),
        _ => text.to_string(),
    }
}

/// "Git push" → "git push": a first word written as a sentence's start
/// (one capital, then lower-case letters). "README", "I" and "McKenzie"
/// stay.
fn lower_first_word(text: &str) -> String {
    let word: &str = text.split_whitespace().next().unwrap_or("");
    let mut chars = word.chars();
    let sentence_case = chars.next().is_some_and(char::is_uppercase)
        && word.chars().count() >= 2
        && chars.all(|c| c.is_lowercase() || !c.is_alphabetic());
    if !sentence_case {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for c in text.chars() {
        if first {
            out.extend(c.to_lowercase());
            first = false;
        } else {
            out.push(c);
        }
    }
    out
}

// ---- The apps of each kind ----------------------------------------------------

/// How a built-in app is recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum By {
    /// Its process names or its site (the end of a browser tab's title).
    Name,
    /// A terminal running a shell (`app_kind.rs`).
    Terminals,
    /// A terminal running a coding CLI: Claude Code, Codex, Gemini CLI…
    CodingClis,
}

/// A built-in app of a kind.
#[derive(Debug)]
pub struct App {
    /// What the page shows, and what "take it out" keeps
    /// (`CleanupProfile.removed_apps`).
    pub name: &'static str,
    /// Process names, lower case, without ".exe".
    exes: &'static [&'static str],
    /// Site names a browser tab's title ends with, lower case.
    sites: &'static [&'static str],
    by: By,
}

const fn app(name: &'static str, exes: &'static [&'static str], sites: &'static [&'static str]) -> App {
    App { name, exes, sites, by: By::Name }
}

const EMAIL: &[App] = &[
    app("Outlook", &["outlook", "olk", "hxoutlook"], &["outlook", "outlook.com", "microsoft outlook"]),
    app("Gmail", &[], &["gmail"]),
    app("Thunderbird", &["thunderbird", "betterbird"], &[]),
    app("Yahoo Mail", &[], &["yahoo mail"]),
    app("Proton Mail", &["proton mail", "protonmail"], &["proton mail"]),
    app("iCloud Mail", &[], &["icloud mail"]),
    app("Fastmail", &[], &["fastmail"]),
    app("Zoho Mail", &[], &["zoho mail"]),
    app("eM Client", &["mailclient"], &[]),
    app("Mailspring", &["mailspring"], &[]),
    app("Superhuman", &["superhuman"], &["superhuman"]),
    app("Postbox", &["postbox"], &[]),
    app("BlueMail", &["bluemail"], &[]),
];

const WORK_CHAT: &[App] = &[
    app("Slack", &["slack"], &["slack"]),
    app("Microsoft Teams", &["ms-teams", "teams", "msteams"], &["microsoft teams"]),
    app("Google Chat", &[], &["google chat"]),
    app("Zoom", &["zoom"], &["zoom"]),
    app("Webex", &["webex", "ciscocollabhost"], &["webex"]),
    app("Mattermost", &["mattermost"], &["mattermost"]),
    app("Rocket.Chat", &["rocket.chat"], &["rocket.chat"]),
    app("Zulip", &["zulip"], &["zulip"]),
    app("Lark", &["lark", "feishu"], &["lark"]),
    app("DingTalk", &["dingtalk"], &[]),
    app("RingCentral", &["ringcentral"], &[]),
    app("LinkedIn", &[], &["linkedin"]),
];

const PERSONAL_CHAT: &[App] = &[
    app("WhatsApp", &["whatsapp", "whatsapp.root"], &["whatsapp", "whatsapp web", "whatsapp business"]),
    app("Messenger", &["messenger"], &["messenger"]),
    app("Telegram", &["telegram"], &["telegram", "telegram web"]),
    app("Signal", &["signal"], &[]),
    app("Discord", &["discord", "discordptb", "discordcanary"], &["discord"]),
    app("Instagram", &[], &["instagram"]),
    app("Google Messages", &[], &["google messages", "messages for web"]),
    app("Phone Link", &["phoneexperiencehost"], &["phone link"]),
    app("Viber", &["viber"], &[]),
    app("LINE", &["line"], &[]),
    app("WeChat", &["wechat", "weixin"], &[]),
    app("KakaoTalk", &["kakaotalk"], &[]),
    app("Element", &["element"], &["element"]),
    app("Beeper", &["beeper"], &[]),
    app("Wire", &["wire"], &[]),
    app("Session", &["session"], &[]),
    app("Threema", &["threema"], &[]),
    app("Skype", &["skype"], &[]),
    app("Ferdium, Rambox and Franz", &["ferdium", "rambox", "franz"], &[]),
];

const DOCUMENTS: &[App] = &[
    app("Word", &["winword"], &["word", "microsoft word"]),
    app("Google Docs", &[], &["google docs"]),
    app("Notion", &["notion"], &["notion"]),
    app("Obsidian", &["obsidian"], &[]),
    app("OneNote", &["onenote", "onenoteim"], &["onenote", "microsoft onenote"]),
    app("LibreOffice", &["soffice.bin", "swriter"], &[]),
    app("WordPad", &["wordpad"], &[]),
    app("Evernote", &["evernote"], &["evernote"]),
    app("Joplin", &["joplin"], &[]),
    app("Logseq", &["logseq"], &[]),
    app("Typora", &["typora"], &[]),
    app("Confluence", &[], &["confluence"]),
    app("Dropbox Paper", &[], &["dropbox paper"]),
    app("Coda", &[], &["coda"]),
];

const AI_PROMPTS: &[App] = &[
    App { name: "Claude Code, Codex and other coding CLIs", exes: &[], sites: &[], by: By::CodingClis },
    app("ChatGPT", &["chatgpt"], &["chatgpt"]),
    app("Claude", &["claude"], &["claude"]),
    app("Gemini", &[], &["gemini", "google gemini"]),
    app("Copilot", &["copilot", "microsoft.copilot"], &["copilot", "microsoft copilot"]),
    app("Perplexity", &["perplexity"], &["perplexity"]),
    app("Grok", &[], &["grok"]),
    app("DeepSeek", &[], &["deepseek"]),
    app("Cursor", &["cursor"], &[]),
    app("VS Code", &["code", "code - insiders", "code - oss", "vscodium", "codium"], &[]),
    app("Windsurf", &["windsurf"], &[]),
    app("Kiro, Trae and Antigravity", &["kiro", "trae", "antigravity"], &[]),
    app("Zed", &["zed"], &[]),
];

const TERMINAL: &[App] = &[App {
    name: "Terminals: Windows Terminal, PowerShell, Git Bash…",
    exes: &[],
    sites: &[],
    by: By::Terminals,
}];

/// A kind's built-in apps.
pub fn built_in(c: Category) -> &'static [App] {
    match c {
        Category::Email => EMAIL,
        Category::WorkChat => WORK_CHAT,
        Category::PersonalChat => PERSONAL_CHAT,
        Category::Documents => DOCUMENTS,
        Category::AiPrompts => AI_PROMPTS,
        Category::Terminal => TERMINAL,
        Category::Other => &[],
    }
}

impl App {
    fn matches(&self, t: &Target) -> bool {
        match self.by {
            By::Terminals => matches!(t.kind, AppKind::Terminal(term) if term.coding_cli.is_none()),
            By::CodingClis => matches!(t.kind, AppKind::Terminal(term) if term.coding_cli.is_some()),
            By::Name => {
                (!t.exe.is_empty() && self.exes.contains(&t.exe.as_str()))
                    || self.sites.iter().any(|s| t.sites.iter().any(|x| x == s))
            }
        }
    }
}

// ---- The app a dictation goes into ---------------------------------------------

/// What a dictation's style is decided from: the app's process, the site
/// its browser tab shows, and the kind `app_kind.rs` read. Never logged.
#[derive(Clone, Debug, Default)]
pub struct Target {
    /// The process's file name ("slack.exe"), for the person's app rules.
    process: Option<String>,
    /// The same, lower case, without ".exe".
    exe: String,
    /// The site names a browser's title may be showing, last part first,
    /// lower case.
    sites: Vec<String>,
    kind: AppKind,
}

/// Browsers' own names at the end of a window title.
const BROWSER_TITLES: &[&str] = &[
    "google chrome",
    "chromium",
    "microsoft edge",
    "mozilla firefox",
    "firefox",
    "mozilla firefox private browsing",
    "firefox private browsing",
    "brave",
    "opera",
    "vivaldi",
    "arc",
    "zen browser",
    "librewolf",
    "waterfox",
    "floorp",
    "thorium",
];

impl Target {
    /// From the process (`process`: "chrome.exe"), its window's title and
    /// the kind `app_kind.rs` read. Only a browser's title is looked at (and
    /// that of Windows' frame host for store apps, whose title is the app's
    /// name): the site it ends with ("Inbox (3) - Gmail - Google Chrome" →
    /// "gmail"; Edge puts the profile before its name, so there the last
    /// two parts), or every part for a chat page `app_kind` found anywhere
    /// in it. Pure.
    pub fn new(process: Option<&str>, title: &str, kind: AppKind) -> Target {
        let exe = app_kind::exe_stem(process.unwrap_or(""));
        let sites = if reads_title(&exe) { sites_in(title, kind == AppKind::Messaging) } else { Vec::new() };
        Target {
            process: process.map(str::to_string).filter(|p| !p.trim().is_empty()),
            exe,
            sites,
            kind,
        }
    }

    /// The app of a history entry (Run again): its process only.
    pub fn of_app(process: &str) -> Target {
        Target::new(Some(process), "", AppKind::Other)
    }

    /// The window a dictation pastes into (`hwnd`, its process `process`
    /// and the kind read at record start). Its title only for a browser,
    /// and never in a test run (robot mode: only a window the test opened).
    pub fn of_window(hwnd: Option<isize>, process: Option<&str>, kind: AppKind) -> Target {
        #[cfg(debug_assertions)]
        if crate::e2e::active() && !crate::realtest::on() {
            return match test_app() {
                Some((exe, title)) => {
                    let kind = app_kind::classify(Some(&exe), "", &title);
                    Target::new(Some(&exe), &title, kind)
                }
                None => Target::default(),
            };
        }
        let exe = app_kind::exe_stem(process.unwrap_or(""));
        let title = match hwnd {
            Some(h) if reads_title(&exe) && crate::realtest::permits(Some(h)) => app_kind::window_title(h),
            _ => String::new(),
        };
        Target::new(process, &title, kind)
    }
}

/// A process whose window title names what it shows.
fn reads_title(exe: &str) -> bool {
    app_kind::is_browser(exe) || exe == "applicationframehost"
}

/// The site names in a browser's title (see [`Target::new`]). Pure.
fn sites_in(title: &str, every_part: bool) -> Vec<String> {
    let mut t = title.replace('\u{200b}', "");
    for sep in [" - ", " – ", " — ", " | ", " • ", " · "] {
        t = t.replace(sep, "\u{1}");
    }
    let mut parts: Vec<String> = t
        .split('\u{1}')
        .map(|p| app_kind::page_name(p).trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let mut edge = false;
    if parts.last().is_some_and(|l| BROWSER_TITLES.contains(&l.as_str())) {
        edge = parts.last().is_some_and(|l| l == "microsoft edge");
        parts.pop();
    }
    parts.reverse();
    if !every_part {
        parts.truncate(if edge { 2 } else { 1 });
    }
    parts
}

/// An app the person added ("Basecamp", "myapp.exe") is this target: its
/// process or its site. Pure.
fn added_matches(entry: &str, t: &Target) -> bool {
    let e = app_kind::exe_stem(entry);
    !e.is_empty() && (e == t.exe || t.sites.contains(&e))
}

/// The kind of app `t` is, as the person has the lists: a terminal's
/// (shell or coding CLI) first, then an app they added, then the built-in
/// apps they kept; else everything else. Pure.
pub fn category(cfg: &YapConfig, t: &Target) -> Category {
    let kept = |c: Category, by_kind: bool| {
        let removed: &[String] = profile(cfg, c).map(|p| &p.removed_apps[..]).unwrap_or(&[]);
        built_in(c)
            .iter()
            .filter(|a| (a.by != By::Name) == by_kind)
            .filter(|a| !removed.iter().any(|r| r.trim().eq_ignore_ascii_case(a.name)))
            .any(|a| a.matches(t))
    };
    if let Some(c) = Category::ALL.into_iter().find(|&c| kept(c, true)) {
        return c;
    }
    if let Some(c) = Category::ALL
        .into_iter()
        .find(|&c| profile(cfg, c).is_some_and(|p| p.apps.iter().any(|a| added_matches(a, t))))
    {
        return c;
    }
    Category::ALL.into_iter().find(|&c| kept(c, false)).unwrap_or(Category::Other)
}

// ---- Styles as profiles ---------------------------------------------------------

/// The kind's profile, if the config has it.
pub fn profile(cfg: &YapConfig, c: Category) -> Option<&CleanupProfile> {
    cfg.cleanup_profiles.iter().find(|p| p.category == c.id())
}

/// The kind's tone: its profile's, when it's one the kind offers, else its
/// default. Pure.
pub fn tone_of(cfg: &YapConfig, c: Category) -> Tone {
    profile(cfg, c)
        .and_then(|p| Tone::from_id(&p.tone))
        .filter(|t| c.tones().contains(t))
        .unwrap_or_else(|| c.default_tone())
}

/// "Match the style to the app" (`config.cleanup_styles`): missing = on
/// unless the main body is the person's own.
pub fn styles_on(cfg: &YapConfig) -> bool {
    cfg.cleanup_styles.unwrap_or(cfg.pp_preset == "default")
}

/// At load: "Match the style to the app" decided for a config without it
/// (on, unless the person picked a body of their own for everywhere — they
/// keep exactly what they had), and each kind's profile there to edit.
pub fn on_load(cfg: &mut YapConfig) {
    if cfg.cleanup_styles.is_none() {
        cfg.cleanup_styles = Some(cfg.pp_preset == "default");
    }
    ensure_profiles(cfg);
}

/// Each kind's profile, added with its default tone where it's missing
/// (after the person's own profiles, in the page's order). Pure.
pub fn ensure_profiles(cfg: &mut YapConfig) {
    for c in Category::ALL {
        if profile(cfg, c).is_none() {
            cfg.cleanup_profiles.push(CleanupProfile {
                id: c.profile_id(),
                name: c.name().to_string(),
                category: c.id().to_string(),
                tone: c.default_tone().id().to_string(),
                ..CleanupProfile::default()
            });
        }
    }
}

/// A dictation's cleanup: the body for the system prompt, the profile's
/// model when it names one, and the style (`None`: the main cleanup, in the
/// speaker's own words).
#[derive(Debug, Clone)]
pub struct Plan {
    pub body: String,
    /// `(provider, base_url, api_key, model)`.
    pub endpoint: Option<(String, String, String, String)>,
    pub style: Option<Style>,
}

/// How a dictation into `t` is cleaned (`None`: not at all — "only clean
/// up in apps with a rule" and none for this app, or its kind's tone is As
/// said). In order: the person's own app rule for its process (exactly as
/// before styles; a rule may name a style too), then "only apps with a
/// rule", then the kind's style while "Match the style to the app" is on,
/// else the main cleanup. Pure.
pub fn resolve(cfg: &YapConfig, t: &Target) -> Option<Plan> {
    if let Some(proc) = t.process.as_deref().map(str::trim) {
        let route = cfg.app_routes.iter().find(|r| !r.app.trim().is_empty() && r.app.trim().eq_ignore_ascii_case(proc));
        if let Some(route) = route {
            // A rule naming a style: that style, its tone and additions.
            let style = cfg
                .cleanup_profiles
                .iter()
                .find(|p| !route.profile_id.is_empty() && p.id == route.profile_id)
                .and_then(|p| Category::from_id(&p.category));
            if let Some(c) = style {
                return style_plan(cfg, c);
            }
            return cfg.resolve_cleanup(Some(proc)).map(|p| Plan { body: p.body, endpoint: p.endpoint, style: None });
        }
    }
    if cfg.routing_scope == "selected_apps_only" {
        return None;
    }
    if styles_on(cfg) {
        return style_plan(cfg, category(cfg, t));
    }
    Some(Plan { body: cfg.pp_prompt.clone(), endpoint: None, style: None })
}

/// Clean a dictation's `text` with its `plan`: the style's line, then the
/// names on screen (`names`), just before the transcript; the answer fitted
/// to the style ([`fit`]). What a dictation does (`pipeline::clean_up`) and
/// the model evaluations time (`eval/styles.rs`). An error is the caller's
/// to fall back from (the raw transcript).
#[allow(clippy::too_many_arguments)]
pub async fn clean(
    text: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    provider: &str,
    plan: &Plan,
    spelling: Option<&str>,
    names: Option<&str>,
    dictionary: &[crate::config::DictionaryEntry],
    disable_thinking: bool,
) -> Result<String, String> {
    // Nothing but "um"s: nothing to type, and no model asked (small ones
    // answer "Nothing to clean." or give the "um" back).
    if filler_only(text) {
        return Ok(String::new());
    }
    let style_line = line(plan.style);
    let before = before_transcript(style_line.as_deref(), names);
    let out = crate::llm::cleanup_with_names(
        text,
        base_url,
        api_key,
        model,
        provider,
        &plan.body,
        spelling,
        before.as_deref(),
        dictionary,
        disable_thinking,
    )
    .await?;
    Ok(fit(plan.style, text, &out))
}

/// An app of each kind, as a dictation would find it (the model
/// evaluations' stand-ins: Outlook, Slack, WhatsApp, Word, ChatGPT, a shell
/// in Windows Terminal, Notepad). Pure.
#[cfg(debug_assertions)]
pub fn sample_target(c: Category) -> Target {
    let (exe, class) = match c {
        Category::Email => ("OUTLOOK.EXE", ""),
        Category::WorkChat => ("slack.exe", ""),
        Category::PersonalChat => ("WhatsApp.Root.exe", ""),
        Category::Documents => ("WINWORD.EXE", ""),
        Category::AiPrompts => ("ChatGPT.exe", ""),
        Category::Terminal => ("WindowsTerminal.exe", "CASCADIA_HOSTING_WINDOW_CLASS"),
        Category::Other => ("notepad.exe", ""),
    };
    Target::new(Some(exe), "", app_kind::classify(Some(exe), class, "PowerShell"))
}

/// The kind's plan: the main body plus its additions, its model override,
/// its tone (`None` for As said).
fn style_plan(cfg: &YapConfig, c: Category) -> Option<Plan> {
    let tone = tone_of(cfg, c);
    if tone == Tone::Off {
        return None;
    }
    let p = profile(cfg, c);
    let extra = p.map(|p| p.prompt.trim()).unwrap_or("");
    let body = if extra.is_empty() { cfg.pp_prompt.clone() } else { format!("{}\n\n{extra}", cfg.pp_prompt.trim_end()) };
    let endpoint = p
        .filter(|p| !p.provider.trim().is_empty())
        .map(|p| (p.provider.clone(), p.base_url.clone(), p.api_key.clone(), p.model.clone()));
    Some(Plan { body, endpoint, style: Some(Style { category: c, tone }) })
}

// ---- The one offer -----------------------------------------------------------------

/// Dictations before the offer: enough to have seen what raw dictation
/// looks like.
pub const OFFER_AFTER: usize = 5;

/// The one gentle offer of AI cleanup (a tip in the main window): cleanup
/// off, never offered before, tips on, a PC the on-device model fits
/// (`fits`: a graphics card that runs the recommended model, or the model
/// already here) and a few dictations done. Pure.
pub fn offer(cfg: &YapConfig, fits: bool, dictations: usize) -> bool {
    !cfg.post_process_enabled && !cfg.cleanup_offer_seen && cfg.show_tips && fits && dictations >= OFFER_AFTER
}

/// Should the main window offer AI cleanup now? Only as the notice policy
/// allows a tip there (`notice.rs`): the person looking at the main window,
/// no dictation, meeting or first-run setup under way. With what it'd run
/// on (the graphics card's name).
#[tauri::command]
pub async fn cleanup_offer_status(app: tauri::AppHandle) -> Value {
    let cfg = crate::config::load();
    let gpu = crate::gpu::this_pc();
    let capable = crate::gpu::can_run_big_model(gpu.as_ref());
    let installed = crate::local_llm::is_installed(&cfg);
    let dictations = crate::history::recent_dictation_secs(OFFER_AFTER).len();
    let (_, focused) = crate::notices::notice::window_view(&app);
    let m = crate::notices::notice::moment();
    // A test run offers it only when the spec asks (the rest of the suite
    // never meets a tip it didn't expect).
    let test_ok = !crate::e2e::active() || std::env::var_os("YAP_E2E_CLEANUP_OFFER").is_some();
    json!({
        "offer": test_ok && offer(&cfg, capable || installed, dictations) && focused && !m.dictating && !m.meeting,
        "gpu": gpu.filter(|_| capable).map(|g| g.name),
        "installed": installed,
    })
}

/// The offer was shown (or answered): never again.
#[tauri::command]
pub fn cleanup_offer_seen() -> Result<(), String> {
    crate::config::update(|c| c.cleanup_offer_seen = true).map(|_| ())
}

/// The page's catalogue: each kind, its tones, its default tone and its
/// built-in apps.
#[tauri::command]
pub fn cleanup_styles_catalog() -> Value {
    let kinds: Vec<Value> = Category::ALL
        .into_iter()
        .map(|c| {
            json!({
                "id": c.id(),
                "name": c.name(),
                "profileId": c.profile_id(),
                "tones": c.tones().iter().map(|t| json!({ "id": t.id(), "label": t.label() })).collect::<Vec<_>>(),
                "defaultTone": c.default_tone().id(),
                // Each built-in app: its name, and the process and site
                // names it goes by (the page knows "ms-teams.exe" is Teams).
                "apps": built_in(c)
                    .iter()
                    .map(|a| json!({ "name": a.name, "match": a.exes.iter().chain(a.sites).collect::<Vec<_>>() }))
                    .collect::<Vec<_>>(),
                "line": line(Some(Style { category: c, tone: c.default_tone() })),
            })
        })
        .collect();
    json!({ "categories": kinds })
}

// ---- Test mode ------------------------------------------------------------------------

#[cfg(debug_assertions)]
static TEST_APP: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

#[cfg(debug_assertions)]
fn test_app() -> Option<(String, String)> {
    TEST_APP.lock().ok().and_then(|g| g.clone())
}

/// Debug-only, test mode: dictations go into a stand-in app — `exe`
/// ("outlook.exe") with the window title `title` — for their style
/// (`None`: no app, everything else). Nothing is read from a real window.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_style_app(exe: Option<String>, title: Option<String>) -> Result<(), String> {
    if !crate::e2e::active() || crate::realtest::on() {
        return Err("e2e_style_app is for test runs (not robot mode)".into());
    }
    if let Ok(mut g) = TEST_APP.lock() {
        *g = exe.map(|e| (e, title.unwrap_or_default()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_kind::{CodingCli, Host, Terminal};
    use crate::config::AppRoute;

    fn cfg() -> YapConfig {
        let mut c = YapConfig::default();
        on_load(&mut c);
        c
    }

    fn browser(title: &str) -> Target {
        Target::new(Some("chrome.exe"), title, app_kind::classify(Some("chrome.exe"), "Chrome_WidgetWin_1", title))
    }

    fn of(exe: &str) -> Target {
        Target::new(Some(exe), "", app_kind::classify(Some(exe), "", ""))
    }

    fn set_tone(c: &mut YapConfig, cat: Category, tone: Tone) {
        let id = cat.profile_id();
        c.cleanup_profiles.iter_mut().find(|p| p.id == id).unwrap().tone = tone.id().into();
    }

    #[test]
    fn programs_by_their_process() {
        let c = cfg();
        for (exe, want) in [
            ("OUTLOOK.EXE", Category::Email),
            ("olk.exe", Category::Email),
            ("thunderbird.exe", Category::Email),
            ("slack.exe", Category::WorkChat),
            ("ms-teams.exe", Category::WorkChat),
            ("WhatsApp.Root.exe", Category::PersonalChat),
            ("Discord.exe", Category::PersonalChat),
            ("Telegram.exe", Category::PersonalChat),
            ("WINWORD.EXE", Category::Documents),
            ("Notion.exe", Category::Documents),
            ("Obsidian.exe", Category::Documents),
            ("ChatGPT.exe", Category::AiPrompts),
            ("Cursor.exe", Category::AiPrompts),
            ("Code.exe", Category::AiPrompts),
            ("notepad.exe", Category::Other),
            ("explorer.exe", Category::Other),
        ] {
            assert_eq!(category(&c, &of(exe)), want, "{exe}");
        }
    }

    #[test]
    fn web_apps_by_the_site_their_title_ends_with() {
        let c = cfg();
        for (title, want) in [
            ("Inbox (3) - sam@example.com - Gmail - Google Chrome", Category::Email),
            ("Mail - Sam Lee - Outlook", Category::Email),
            ("general - Acme - Slack", Category::WorkChat),
            ("Chat | Sam | Microsoft Teams", Category::WorkChat),
            ("(2) WhatsApp", Category::PersonalChat),
            ("Quarterly plan - Google Docs - Google Chrome", Category::Documents),
            ("Reverse a string - Claude", Category::AiPrompts),
            ("ChatGPT", Category::AiPrompts),
            ("Gemini - Google Chrome", Category::AiPrompts),
            // Edge: its profile before its name.
            ("Inbox - Gmail - Personal - Microsoft\u{200b} Edge", Category::Email),
            // A page that only mentions a site stays everything else.
            ("Claude - Wikipedia", Category::Other),
            ("Slack notes - Google Docs", Category::Documents),
            ("Gmail tips and tricks | Blog", Category::Other),
            ("New Tab - Google Chrome", Category::Other),
        ] {
            assert_eq!(category(&c, &browser(title)), want, "{title}");
        }
        // A program's title never counts: only a browser shows a site.
        assert_eq!(category(&c, &Target::new(Some("notepad.exe"), "Gmail", AppKind::Other)), Category::Other);
        // A chat page app_kind found anywhere in the title (the robot's
        // "WhatsApp - <nonce>").
        assert_eq!(category(&c, &browser("WhatsApp - yaprobot-0123456789ab")), Category::PersonalChat);
    }

    #[test]
    fn terminals_and_coding_clis_by_what_app_kind_read() {
        let c = cfg();
        let shell = AppKind::Terminal(Terminal { coding_cli: None, host: Host::WindowsTerminal });
        let cli = AppKind::Terminal(Terminal { coding_cli: Some(CodingCli::ClaudeCode), host: Host::WindowsTerminal });
        assert_eq!(category(&c, &Target::new(Some("WindowsTerminal.exe"), "", shell)), Category::Terminal);
        assert_eq!(category(&c, &Target::new(Some("WindowsTerminal.exe"), "", cli)), Category::AiPrompts);
        // VS Code's integrated terminal is a terminal, not the editor.
        let editor_term = AppKind::Terminal(Terminal { coding_cli: None, host: Host::Editor });
        assert_eq!(category(&c, &Target::new(Some("Code.exe"), "", editor_term)), Category::Terminal);
    }

    #[test]
    fn every_chat_app_app_kind_knows_has_a_kind() {
        // app_kind's chat apps (desktop and web) all land in a chat kind.
        let c = cfg();
        for exe in [
            "slack", "discord", "discordptb", "discordcanary", "whatsapp", "whatsapp.root", "telegram", "signal",
            "messenger", "ms-teams", "teams", "element", "mattermost", "rocket.chat", "beeper", "ferdium", "rambox",
            "franz", "viber", "line", "wechat", "weixin", "kakaotalk", "wire", "session", "zulip", "skype",
        ] {
            let k = category(&c, &of(&format!("{exe}.exe")));
            assert!(matches!(k, Category::WorkChat | Category::PersonalChat), "{exe}: {k:?}");
        }
        for page in ["WhatsApp Web", "Slack", "Discord", "Messenger", "Microsoft Teams", "Telegram", "Google Chat", "Google Messages"] {
            let k = category(&c, &browser(&format!("x - {page}")));
            assert!(matches!(k, Category::WorkChat | Category::PersonalChat), "{page}: {k:?}");
        }
    }

    #[test]
    fn the_person_moves_and_adds_apps() {
        let mut c = cfg();
        // Discord to work chat: taken out of personal chat, added to work.
        let personal = Category::PersonalChat.profile_id();
        let work = Category::WorkChat.profile_id();
        c.cleanup_profiles.iter_mut().find(|p| p.id == personal).unwrap().removed_apps.push("Discord".into());
        c.cleanup_profiles.iter_mut().find(|p| p.id == work).unwrap().apps.push("discord.exe".into());
        assert_eq!(category(&c, &of("Discord.exe")), Category::WorkChat);
        // Taken out and added nowhere: everything else.
        c.cleanup_profiles.iter_mut().find(|p| p.id == work).unwrap().apps.clear();
        assert_eq!(category(&c, &of("Discord.exe")), Category::Other);
        // A site added by its name.
        let docs = Category::Documents.profile_id();
        c.cleanup_profiles.iter_mut().find(|p| p.id == docs).unwrap().apps.push("Basecamp".into());
        assert_eq!(category(&c, &browser("Project X - Basecamp")), Category::Documents);
        // An added app wins over a built-in one elsewhere.
        c.cleanup_profiles.iter_mut().find(|p| p.id == docs).unwrap().apps.push("slack".into());
        assert_eq!(category(&c, &of("slack.exe")), Category::Documents);
    }

    #[test]
    fn a_new_config_gets_every_style_once() {
        let mut c = YapConfig::default();
        on_load(&mut c);
        on_load(&mut c);
        assert_eq!(c.cleanup_profiles.len(), Category::ALL.len());
        assert_eq!(c.cleanup_styles, Some(true));
        for k in Category::ALL {
            assert_eq!(tone_of(&c, k), k.default_tone());
            assert!(k.tones().contains(&k.default_tone()));
        }
        // A tone a kind doesn't offer falls back to its default.
        set_tone(&mut c, Category::Terminal, Tone::VeryCasual);
        assert_eq!(tone_of(&c, Category::Terminal), Tone::Command);
    }

    #[test]
    fn someone_with_their_own_body_keeps_it_everywhere() {
        // Their own preset or body (pp_preset not "default"): styles off,
        // decided once; the main body everywhere, as before.
        let mut c = YapConfig { pp_preset: "custom".into(), pp_prompt: "Write like a pirate.".into(), ..YapConfig::default() };
        on_load(&mut c);
        assert_eq!(c.cleanup_styles, Some(false));
        let p = resolve(&c, &of("outlook.exe")).unwrap();
        assert_eq!(p.body, "Write like a pirate.");
        assert!(p.style.is_none());
        // A saved choice is never changed.
        let mut c = YapConfig { cleanup_styles: Some(true), pp_preset: "email".into(), ..YapConfig::default() };
        on_load(&mut c);
        assert_eq!(c.cleanup_styles, Some(true));
    }

    #[test]
    fn existing_app_rules_win_as_before() {
        let mut c = cfg();
        c.cleanup_profiles.push(CleanupProfile {
            id: "mine".into(),
            name: "Terse".into(),
            prompt: "Be terse.".into(),
            provider: "groq".into(),
            model: "openai/gpt-oss-120b".into(),
            ..CleanupProfile::default()
        });
        c.app_routes.push(AppRoute { app: "slack.exe".into(), label: "Slack".into(), profile_id: "mine".into(), prompt: String::new() });
        c.app_routes.push(AppRoute { app: "legacy.exe".into(), label: String::new(), profile_id: String::new(), prompt: "Old body.".into() });
        let p = resolve(&c, &of("Slack.exe")).unwrap();
        assert_eq!(p.body, "Be terse.");
        assert!(p.style.is_none());
        assert_eq!(p.endpoint.unwrap().3, "openai/gpt-oss-120b");
        assert_eq!(resolve(&c, &of("legacy.exe")).unwrap().body, "Old body.");
        // A rule naming a style: that style.
        c.app_routes.push(AppRoute { app: "mail.exe".into(), label: String::new(), profile_id: Category::Email.profile_id(), prompt: String::new() });
        let p = resolve(&c, &of("mail.exe")).unwrap();
        assert_eq!(p.style, Some(Style { category: Category::Email, tone: Tone::Formal }));
        assert_eq!(p.body, c.pp_prompt);
        // Only apps with a rule: everything else is left as said.
        c.routing_scope = "selected_apps_only".into();
        assert!(resolve(&c, &of("outlook.exe")).is_none());
        assert_eq!(resolve(&c, &of("slack.exe")).unwrap().body, "Be terse.");
    }

    #[test]
    fn a_styles_plan() {
        let mut c = cfg();
        // Everything else: the main cleanup, no line — exactly as before.
        let p = resolve(&c, &of("notepad.exe")).unwrap();
        assert_eq!(p.body, c.pp_prompt);
        assert_eq!(p.style, Some(Style { category: Category::Other, tone: Tone::Natural }));
        assert!(line(p.style).is_none());
        // Its additions follow the main body; its model is its own.
        let id = Category::Email.profile_id();
        {
            let e = c.cleanup_profiles.iter_mut().find(|p| p.id == id).unwrap();
            e.prompt = "Sign off with Best wishes.".into();
            e.provider = "ondevice".into();
        }
        let p = resolve(&c, &of("outlook.exe")).unwrap();
        assert!(p.body.starts_with(c.pp_prompt.trim_end()) && p.body.ends_with("\n\nSign off with Best wishes."));
        assert_eq!(p.endpoint.unwrap().0, "ondevice");
        // As said: no cleanup at all.
        set_tone(&mut c, Category::Terminal, Tone::Off);
        let shell = AppKind::Terminal(Terminal { coding_cli: None, host: Host::Console });
        assert!(resolve(&c, &Target::new(Some("cmd.exe"), "", shell)).is_none());
        // Styles off: the main cleanup everywhere.
        c.cleanup_styles = Some(false);
        let p = resolve(&c, &of("outlook.exe")).unwrap();
        assert!(p.style.is_none());
        assert_eq!(p.body, c.pp_prompt);
    }

    #[test]
    fn the_lines() {
        let s = |category, tone| line(Some(Style { category, tone })).unwrap_or_default();
        let email = s(Category::Email, Tone::Formal);
        assert!(email.starts_with("This dictation is the body of an email") && email.contains("formal") && email.ends_with(TONE_WINS));
        let prompt = s(Category::AiPrompts, Tone::Literal);
        assert!(prompt.contains("no please") && !prompt.contains(TONE_WINS));
        assert!(s(Category::Terminal, Tone::Command).contains("command line"));
        // Natural: what the text is, no tone.
        assert_eq!(s(Category::Email, Tone::Natural), Category::Email.kind_line().unwrap());
        assert!(line(Some(Style { category: Category::Other, tone: Tone::Natural })).is_none());
        assert!(line(None).is_none());
        // Short enough to read for every dictation (~4 characters a token).
        for c in Category::ALL {
            for &t in c.tones() {
                assert!(s(c, t).len() < 520, "{c:?}/{t:?}: {}", s(c, t).len());
            }
        }
        // With the names on screen.
        assert_eq!(before_transcript(Some("A."), Some("B.")).as_deref(), Some("A.\n\nB."));
        assert_eq!(before_transcript(None, Some("B.")).as_deref(), Some("B."));
        assert_eq!(before_transcript(Some(" "), None), None);
    }

    #[test]
    fn fitting_a_command_and_a_very_casual_message() {
        let st = |category, tone| Some(Style { category, tone });
        let cmd = st(Category::Terminal, Tone::Command);
        assert_eq!(fit(cmd, "", "Git status."), "git status");
        assert_eq!(fit(cmd, "", "git push origin main"), "git push origin main");
        assert_eq!(fit(cmd, "", "README.md is there..."), "README.md is there...");
        assert_eq!(fit(cmd, "", "Npm run build."), "npm run build");
        assert_eq!(fit(cmd, "", "I think so."), "I think so");
        assert_eq!(fit(cmd, "", "line one.\nline two."), "line one.\nline two.");
        let casual = st(Category::WorkChat, Tone::Casual);
        assert_eq!(fit(casual, "", "I'll be late to standup; the train's delayed."), "I'll be late to standup, the train's delayed.");
        assert_eq!(fit(casual, "", "Use for (i = 0; i < n; i++) here."), "Use for (i = 0; i < n; i++) here.");
        let vc = st(Category::PersonalChat, Tone::VeryCasual);
        assert_eq!(fit(vc, "", "ok see you at six."), "ok see you at six");
        assert_eq!(fit(vc, "", "running late; sorry."), "running late, sorry");
        assert_eq!(fit(vc, "", "are you around?"), "are you around?");
        assert_eq!(fit(st(Category::Email, Tone::Formal), "hi sam", "Hi Sam."), "Hi Sam.");
        assert_eq!(fit(None, "", "Git status."), "Git status.");
    }

    #[test]
    fn an_emails_made_up_greeting_and_sign_off_come_off() {
        let email = Some(Style { category: Category::Email, tone: Tone::Formal });
        // The model evaluations' outputs (Qwen3.5-4B).
        let raw = "hi sarah um thanks for your time today i'll send over the updated proposal by thursday let me know if you need anything before then thanks";
        let out = "Hi Sarah,\n\nThank you for your time today. I will send over the updated proposal by Thursday.\n\nBest regards,\n\n[Speaker Name]";
        assert_eq!(fit(email, raw, out), "Hi Sarah,\n\nThank you for your time today. I will send over the updated proposal by Thursday.");
        let raw = "just a quick note to say the invoice was paid this morning so we're all square";
        let out = "Dear Team,\n\nThis is to confirm that the invoice was paid this morning, so we are all square.\n\nBest regards,\n[Your Name]";
        assert_eq!(fit(email, raw, out), "This is to confirm that the invoice was paid this morning, so we are all square.");
        let out = "Hi Tom,\n\nThanks for the notes.\n\nBest,\nAlex";
        assert_eq!(fit(email, "hi tom thanks for the notes", out), "Hi Tom,\n\nThanks for the notes.");
        // Said, restyled: kept.
        let raw = "hi sam the figures are attached thanks";
        let out = "Dear Sam,\n\nThe figures are attached.\n\nThank you,";
        assert_eq!(fit(email, raw, out), out);
        let raw = "hello team the build is green cheers jo";
        assert_eq!(fit(email, raw, "Hello team,\n\nThe build is green.\n\nCheers,\nJo"), "Hello team,\n\nThe build is green.\n\nCheers,\nJo");
        // A list's short items were said.
        let raw = "we need milk eggs and bread";
        assert_eq!(fit(email, raw, "We need:\n- milk\n- eggs\n- bread"), "We need:\n- milk\n- eggs\n- bread");
        // One line: as it is.
        assert_eq!(fit(email, "x", "Best regards"), "Best regards");
        // Other kinds: never.
        let chat = Some(Style { category: Category::WorkChat, tone: Tone::Casual });
        assert_eq!(fit(chat, "hi", "Hi,\n\nOK.\n\nBest,\nAlex"), "Hi,\n\nOK.\n\nBest,\nAlex");
    }

    #[test]
    fn filler_alone_is_nothing() {
        for t in ["um", "Um, uh.", "uh you know um", "Hmm...", "UMM"] {
            assert!(filler_only(t), "{t}");
        }
        for t in ["you know", "um yes", "Okay.", "", "...", "um like this", "er, no"] {
            assert!(!filler_only(t), "{t}");
        }
    }

    #[test]
    fn the_offer() {
        let mut c = YapConfig::default();
        assert!(offer(&c, true, OFFER_AFTER));
        assert!(!offer(&c, false, OFFER_AFTER));
        assert!(!offer(&c, true, OFFER_AFTER - 1));
        c.show_tips = false;
        assert!(!offer(&c, true, 50));
        c.show_tips = true;
        c.cleanup_offer_seen = true;
        assert!(!offer(&c, true, 50));
        c.cleanup_offer_seen = false;
        c.post_process_enabled = true;
        assert!(!offer(&c, true, 50));
    }

    #[test]
    fn the_samples_are_their_kinds() {
        let c = cfg();
        for k in Category::ALL {
            assert_eq!(category(&c, &sample_target(k)), k);
        }
    }

    #[test]
    fn the_catalogue_names_every_kind() {
        let v = cleanup_styles_catalog();
        let kinds = v["categories"].as_array().unwrap();
        assert_eq!(kinds.len(), 7);
        assert_eq!(kinds[0]["id"], "email");
        assert_eq!(kinds[4]["defaultTone"], "literal");
        assert!(kinds[6]["apps"].as_array().unwrap().is_empty());
    }
}
