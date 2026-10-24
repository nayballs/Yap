//! **Report a problem** (Settings → About): a diagnostic file the person
//! looks through first, then saves where they choose. Nothing is uploaded:
//! after saving, Yap offers GitHub's new-issue page with a title and a
//! template (its version and Windows' only), never the file.
//!
//! The file is a .zip of:
//! - `README.txt` — what's in it and what was left out;
//! - `system.txt` — Windows' version, the processor, memory, the graphics
//!   card, Yap's version and channel ([`system_info`]);
//! - `settings.json` — the settings **summary** ([`settings_summary`]): which
//!   providers and models, switches, shortcuts and numbers. An allow-list:
//!   a setting it doesn't name is left out (its name listed under
//!   `leftOut`), so a new setting never slips in. API keys show only as which
//!   services have one; About you, names, the dictionary's words, prompts
//!   the person wrote, device names, endpoints' addresses, memory, notes,
//!   chats and history are never in it;
//! - `connections.txt` — the last week of network receipts by purpose
//!   (`net.rs`), hosts only for Yap's known services ([`connections_text`]);
//! - `errors.txt` — the latest warnings and errors from the log;
//! - `logs/` — the last 3 days of the log, `info` and above only (a `debug`
//!   line — Debug logging's, which can hold what was said — and the lines
//!   under it are dropped), each line scrubbed ([`Scrub`]): the Windows user
//!   folder, the user and PC names, email addresses, key-like tokens and
//!   links' paths and queries masked.
//!
//! The page shows the file list and the summary before anything is saved
//! ([`diagnostics_preview`]); the file saved is the one shown.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::AppHandle;

/// Days of log in the file.
const LOG_DAYS: usize = 3;
/// Days of log the errors are read from.
const ERROR_DAYS: usize = 7;
/// The latest this many warnings and errors.
const MAX_ERRORS: usize = 200;
/// Of each day's log, at most its last this many bytes (read), and kept.
const LOG_READ_BYTES: u64 = 4 * 1024 * 1024;
const LOG_KEEP_BYTES: usize = 1024 * 1024;

const REPO_ISSUES: &str = "https://github.com/nayballs/Yap/issues/new";

// ---- the settings summary ---------------------------------------------------------------

/// Settings copied as they are: switches, choices, numbers, shortcuts, the
/// providers' and models' ids. (config.json's own names.)
const COPY: &[&str] = &[
    "hotkey",
    "editHotkey",
    "rewriteLastDictation",
    "assistantHotkey",
    "readHotkey",
    "asSaidHotkey",
    "asSaidOffer",
    "snipHotkey",
    "snipVoice",
    "chatImageDays",
    "modelSize",
    "useGpu",
    "sttDevice",
    "keepRecentAudio",
    "soundEnabled",
    "dictionaryFuzzy",
    "learnWords",
    "showTips",
    "recordingMode",
    "quietOtherApps",
    "pauseMedia",
    "keepMicReady",
    "appendTrailingSpace",
    "smartSpacing",
    "screenNames",
    "chatNoFullStop",
    "autoSubmit",
    "voiceEnter",
    "terminalMode",
    "restoreClipboard",
    "startHidden",
    "autostart",
    "audioFeedbackVolume",
    "selectedLanguage",
    "translateToEnglish",
    "modelUnloadTimeout",
    "overlayPosition",
    "barEnabled",
    "barHideFullscreen",
    "barTheme",
    "barDictationButtons",
    "theme",
    "readingFont",
    "readingSize",
    "motion",
    "autoSubmitKey",
    "updateChecksEnabled",
    "debugLogging",
    "postProcessEnabled",
    "ppModel",
    "ppPreset",
    "ppDisableThinking",
    "ppLocalModel",
    "localLlmUpgradeDismissed",
    "localContext",
    "localMtp",
    "routingScope",
    "speakReplies",
    "speakWhen",
    "talkBackHandsFree",
    "assistantAnswerCard",
    "assistantStopOnPause",
    "openApps",
    "webSearch",
    "webSearchNative",
    "webSearchService",
    "ttsEngine",
    "ttsVoice",
    "ttsWindowsVoice",
    "ttsRate",
    "ttsVolume",
    "ttsProvider",
    "streamingPartials",
    "historyEnabled",
    "bridgeEnabled",
    "mcpAllowWrites",
    "meetingOpenNotepad",
    "meetingSplitScreen",
    "meetingLiveTranscript",
    "meetingKeepAudio",
    "meetingAudioDays",
    "meetingDiarization",
    "diarizationModel",
    "meetingSpeakerNames",
    "meetingCallNames",
    "meetingDetection",
    "meetingAutoStart",
    "meetingDetectApps",
    "meetingDetectStyle",
    "meetingHideFromCapture",
    "meetingMaxMinutes",
    "meetingCallEnd",
    "meetingEndWhenQuiet",
    "meetingHotkey",
    "meetingReminder",
    "meetingBrief",
    "memorySearchChats",
    "memoryLearnChats",
    "memoryLearnMeetings",
    "memorySensitive",
    "regionSpelling",
    "configVersion",
];

/// Yap's own provider ids; any other string is "other".
const PROVIDERS: &[&str] = &["groq", "openai", "anthropic", "gemini", "openrouter", "ondevice", "local", "custom"];

fn provider(v: &Value) -> Value {
    match v.as_str() {
        Some("") | None => json!(""),
        Some(p) if PROVIDERS.contains(&p) => json!(p),
        Some(_) => json!("other"),
    }
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("").trim()
}

/// "set" / "not set": that the person filled something in, never what.
fn set_or_not(v: &Value) -> Value {
    json!(if text(v).is_empty() { "not set" } else { "set" })
}

/// Where an endpoint is, never its address (it can carry a key, a name or
/// a server of the person's own).
fn endpoint(v: &Value) -> Value {
    let url = text(v);
    if url.is_empty() {
        return json!("the provider's own");
    }
    let Ok(u) = reqwest::Url::parse(url) else { return json!("set") };
    let standard = ["groq", "openai", "anthropic", "gemini", "openrouter"]
        .iter()
        .filter_map(|p| crate::ai::ai_model::standard_base_url(p))
        .any(|std| std.trim_end_matches('/') == url.trim_end_matches('/'));
    if standard {
        return json!("the provider's own");
    }
    json!(match crate::privacy::net::place_of(u.host_str().unwrap_or("")) {
        crate::privacy::net::Place::Pc => "a server on this PC",
        crate::privacy::net::Place::Network => "a server on your network",
        crate::privacy::net::Place::Internet => "a server on the internet",
    })
}

/// The services a key map has a key for, never the keys.
fn keyed(v: &Value) -> Value {
    let mut names: Vec<&str> = v
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, key)| !text(key).is_empty())
        .map(|(name, _)| if PROVIDERS.contains(&name.as_str()) || KNOWN_SERVICES.contains(&name.as_str()) { name.as_str() } else { "other" })
        .collect();
    names.sort_unstable();
    names.dedup();
    json!(names)
}

const KNOWN_SERVICES: &[&str] = &["exa", "tavily", "elevenlabs"];

/// The settings summary: config.json (as `YapConfig` serialises) through an
/// allow-list. Pure.
pub fn settings_summary(cfg: &Value) -> Value {
    let Some(cfg) = cfg.as_object() else { return json!({}) };
    let mut out = Map::new();
    let mut left_out: Vec<String> = Vec::new();
    for (key, value) in cfg {
        let k = key.as_str();
        let shown = if COPY.contains(&k) {
            Some(value.clone())
        } else {
            match k {
                "ppProvider" => Some(provider(value)),
                "inputDevice" | "outputDevice" => {
                    Some(json!(if value.is_null() || text(value).is_empty() { "default" } else { "chosen (its name left out)" }))
                }
                "ppPrompt" => Some(json!(if text(value).is_empty() { "Yap's" } else { "your own (left out)" })),
                "meetingConsentMessage" => Some(json!(if text(value).is_empty() { "Yap's" } else { "your own (left out)" })),
                "agentName" | "aboutName" | "aboutMe" => Some(set_or_not(value)),
                "regionCountry" => Some(json!(if text(value).is_empty() { "automatic" } else { "chosen (left out)" })),
                "ppApiKey" => Some(set_or_not(value)),
                "ppBaseUrl" => Some(endpoint(value)),
                "ppApiKeys" | "webSearchKeys" | "ttsCloudKeys" => Some(keyed(value)),
                "dictionary" => {
                    let entries = value.as_array().map(Vec::as_slice).unwrap_or_default();
                    let snippets = entries.iter().filter(|e| e["snippet"].as_bool() == Some(true)).count();
                    Some(json!({ "corrections": entries.len() - snippets, "snippets": snippets }))
                }
                "appRoutes" | "appNicknames" | "appsBlocked" => Some(json!(value.as_array().map_or(0, Vec::len))),
                "cleanupProfiles" => {
                    let profiles = value.as_array().map(Vec::as_slice).unwrap_or_default();
                    let mut providers: Vec<Value> = profiles.iter().map(|p| provider(&p["provider"])).filter(|p| p != "").collect();
                    providers.sort_by_key(|p| p.to_string());
                    providers.dedup();
                    Some(json!({ "count": profiles.len(), "providers": providers }))
                }
                "llmScopes" => Some(Value::Object(
                    value
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(scope, _)| matches!(scope.as_str(), "cleanup" | "voiceAgent" | "noteFormatting" | "chat" | "notes"))
                        .map(|(scope, s)| {
                            (
                                scope.clone(),
                                json!({
                                    "enabled": s["enabled"],
                                    "provider": provider(&s["provider"]),
                                    "model": s["model"],
                                    "endpoint": endpoint(&s["baseUrl"]),
                                    "key": set_or_not(&s["apiKey"]),
                                    "keysFor": keyed(&s["apiKeys"]),
                                    "prompt": if text(&s["prompt"]).is_empty() { "Yap's" } else { "your own (left out)" },
                                    "disableThinking": s["disableThinking"],
                                }),
                            )
                        })
                        .collect(),
                )),
                "ttsCloud" => Some(Value::Object(
                    value
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(service, _)| KNOWN_SERVICES.contains(&service.as_str()))
                        .map(|(service, c)| {
                            (
                                service.clone(),
                                json!({
                                    "model": c["model"],
                                    "voice": if text(&c["voice"]).is_empty() { "the default" } else { "chosen (left out)" },
                                }),
                            )
                        })
                        .collect(),
                )),
                _ => None,
            }
        };
        match shown {
            Some(v) => {
                out.insert(key.clone(), v);
            }
            None => left_out.push(key.clone()),
        }
    }
    left_out.sort();
    out.insert("leftOut".into(), json!(left_out));
    Value::Object(out)
}

// ---- scrubbing the log --------------------------------------------------------------------

/// What's masked in every line of the log that goes in the file: this PC's
/// user folder, user and PC names and data folder (exact, any case), and by
/// pattern any Windows user folder, email address, link (its path and query,
/// unless it's one of Yap's known services: then just the query) and
/// key-like token.
pub struct Scrub {
    /// (lower-cased text, its mask, whole words only)
    exact: Vec<(String, &'static str, bool)>,
}

static USER_DIR: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r#"(?i)([a-z]:[\\/]+users[\\/]+)[^\\/\r\n'"`<>|]+"#).unwrap());
static EMAIL: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}").unwrap());
static LINK: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r#"(?i)\b(https?|wss?|webcal)://([^/\s"'<>?#]+)([^\s"'<>]*)"#).unwrap());
static PREFIXED_KEY: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"(?i)\b(sk[-_][A-Za-z0-9_\-]{8,}|gsk_[A-Za-z0-9]{8,}|AIza[0-9A-Za-z_\-]{10,}|xai-[A-Za-z0-9]{8,}|tvly-[A-Za-z0-9_\-]{8,}|bearer\s+[A-Za-z0-9._\-]{8,})").unwrap()
});
static LONG_TOKEN: once_cell::sync::Lazy<regex::Regex> =
    once_cell::sync::Lazy::new(|| regex::Regex::new(r"[A-Za-z0-9_\-+/=.]{24,}").unwrap());

/// Hosts of Yap's own services and providers: a link to one keeps its path
/// (a model's file name helps), never its query. Anything else is only its
/// host. (Also the hosts `connections.txt` names.)
const KNOWN_HOSTS: &[&str] = &[
    "groq.com",
    "openai.com",
    "anthropic.com",
    "googleapis.com",
    "openrouter.ai",
    "exa.ai",
    "tavily.com",
    "elevenlabs.io",
    "github.com",
    "githubusercontent.com",
    "huggingface.co",
    "hf.co",
    "handy.computer",
    "contextmirror.com",
];

fn known_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    let h = h.split(':').next().unwrap_or("");
    KNOWN_HOSTS.iter().any(|k| h == *k || h.ends_with(&format!(".{k}")))
}

impl Scrub {
    /// This PC's: the user folder, the data folder, the user and PC names.
    pub fn this_pc() -> Scrub {
        let mut exact = Vec::new();
        if let Some(home) = dirs::home_dir() {
            exact.push((home.to_string_lossy().into_owned(), "%USERPROFILE%"));
        }
        exact.push((crate::config::data_dir().to_string_lossy().into_owned(), "<data>"));
        for (var, mask) in [("USERNAME", "<user>"), ("COMPUTERNAME", "<pc>"), ("USERDOMAIN", "<pc>")] {
            if let Ok(v) = std::env::var(var) {
                exact.push((v, mask));
            }
        }
        Scrub::new(exact)
    }

    /// `exact`: (text, its mask). A folder (it has a `\` or `/`) is masked
    /// wherever it is, either slash; a name only as a whole word ("Dan" in
    /// "dance" stays).
    pub fn new(exact: Vec<(String, &'static str)>) -> Scrub {
        let mut exact: Vec<(String, &'static str, bool)> = exact
            .into_iter()
            .filter(|(s, _)| s.trim().chars().count() >= 3)
            .flat_map(|(s, mask)| {
                let word = !s.contains(['\\', '/']);
                let forward = s.replace('\\', "/");
                [(s.to_lowercase(), mask, word), (forward.to_lowercase(), mask, word)]
            })
            .collect();
        // The longest first (the user folder before the user name in it).
        exact.sort_by_key(|(s, _, _)| std::cmp::Reverse(s.len()));
        exact.dedup();
        Scrub { exact }
    }

    /// One line, masked. Pure.
    pub fn line(&self, line: &str) -> String {
        let mut s = line.to_string();
        for (needle, mask, word) in &self.exact {
            s = replace_ignore_case(&s, needle, mask, *word);
        }
        let s = USER_DIR.replace_all(&s, "${1}<user>");
        let s = LINK.replace_all(&s, |c: &regex::Captures| {
            let (scheme, host, rest) = (&c[1], &c[2], &c[3]);
            // No user:pass@ in front of the host.
            let host = host.rsplit('@').next().unwrap_or(host);
            if known_host(host) {
                let path = rest.split(['?', '#']).next().unwrap_or("");
                let cut = if path.len() < rest.len() { "?…" } else { "" };
                format!("{scheme}://{host}{path}{cut}")
            } else if rest.is_empty() || rest == "/" {
                format!("{scheme}://{host}{rest}")
            } else {
                format!("{scheme}://{host}/…")
            }
        });
        let s = EMAIL.replace_all(&s, "<email>");
        let s = PREFIXED_KEY.replace_all(&s, "<key>");
        let s = LONG_TOKEN.replace_all(&s, |c: &regex::Captures| {
            let t = &c[0];
            // A long run of letters AND digits is a key or a token; a path
            // or a long word is left.
            let letters = t.chars().any(|ch| ch.is_ascii_alphabetic());
            let digits = t.chars().filter(|ch| ch.is_ascii_digit()).count();
            if letters && digits >= 3 && !t.contains('/') && !t.contains('.') {
                "<redacted>".to_string()
            } else {
                t.to_string()
            }
        });
        s.into_owned()
    }
}

/// `hay` with each `needle_lower` (any case; `word`: only where no letter
/// or digit touches it) replaced by `with`.
fn replace_ignore_case(hay: &str, needle_lower: &str, with: &str, word: bool) -> String {
    if needle_lower.is_empty() {
        return hay.to_string();
    }
    let lower = hay.to_lowercase();
    // Only when lower-casing kept the byte offsets (ASCII and most text).
    if lower.len() != hay.len() {
        return hay.replace(needle_lower, with);
    }
    let alnum = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut out = String::with_capacity(hay.len());
    let mut i = 0;
    let mut from = 0;
    while let Some(pos) = lower[from..].find(needle_lower) {
        let at = from + pos;
        let end = at + needle_lower.len();
        if !hay.is_char_boundary(at) || !hay.is_char_boundary(end) {
            break;
        }
        from = end;
        if word && (alnum(hay[..at].chars().next_back()) || alnum(hay[end..].chars().next())) {
            continue;
        }
        out.push_str(&hay[i..at]);
        out.push_str(with);
        i = end;
    }
    out.push_str(&hay[i..]);
    out
}

/// A log line's level, when it starts an event (`2026-10-24T09:15:02.12Z
/// INFO yap_lib::…`); `None` for the lines an event goes on over.
fn level_of(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    if b.len() < 20 || !b[..4].iter().all(u8::is_ascii_digit) || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    line.split_whitespace().nth(1)
}

/// A day's log for the file: `info` and above (a `debug` or `trace` event
/// and the lines it goes on over are dropped), scrubbed; and its warnings
/// and errors. Pure.
pub fn filter_log(text: &str, scrub: &Scrub) -> (String, Vec<String>) {
    let mut kept = String::new();
    let mut errors = Vec::new();
    let mut keep = false;
    let mut problem = false;
    for line in text.lines() {
        if let Some(level) = level_of(line) {
            keep = matches!(level, "INFO" | "WARN" | "ERROR");
            problem = matches!(level, "WARN" | "ERROR");
        }
        if !keep {
            continue;
        }
        let clean = scrub.line(line);
        if problem {
            errors.push(clean.clone());
        }
        kept.push_str(&clean);
        kept.push('\n');
    }
    if kept.len() > LOG_KEEP_BYTES {
        let mut cut = kept.len() - LOG_KEEP_BYTES;
        while !kept.is_char_boundary(cut) {
            cut += 1;
        }
        let from = kept[cut..].find('\n').map_or(cut, |n| cut + n + 1);
        kept = format!("[… the start of this day's log is left out …]\n{}", &kept[from..]);
    }
    (kept, errors)
}

// ---- the connections, for the file -------------------------------------------------------

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 * 1024 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
        b if b >= 1024 * 1024 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

/// The last week's connections by purpose: counts, failures, sizes, and the
/// hosts of Yap's known services (others: only where they are). Pure.
pub fn connections_text(entries: &[crate::privacy::net::Entry], now: u64) -> String {
    let sum = crate::privacy::net::summarize(entries, now, now.saturating_sub(86_400));
    let mut out = String::from(
        "Network connections Yap made in the last 7 days, by purpose (from connections.json on this PC).\n\
         Hosts are named only for Yap's own services and AI providers; web pages, wikis, calendar\n\
         links and your own servers show only where they are.\n\n",
    );
    out.push_str(&format!(
        "In all: {} requests, {} failed, {} sent, {} received ({} on this PC, {} left it).\n\n",
        sum.week.count,
        sum.week.failed,
        size(sum.week.sent),
        size(sum.week.received),
        sum.week.local,
        sum.week.leaving
    ));
    if sum.groups.is_empty() {
        out.push_str("None recorded.\n");
    }
    for g in &sum.groups {
        let count: u64 = g.hosts.iter().map(|h| h.count).sum();
        let failed: u64 = g.hosts.iter().map(|h| h.failed).sum();
        let sent: u64 = g.hosts.iter().map(|h| h.sent).sum();
        let received: u64 = g.hosts.iter().map(|h| h.received).sum();
        out.push_str(&format!(
            "{}: {count} requests, {failed} failed, {} sent, {} received\n",
            g.label,
            size(sent),
            size(received)
        ));
        let mut wheres: Vec<String> = g
            .hosts
            .iter()
            .map(|h| {
                if known_host(&h.host) && h.place == crate::privacy::net::Place::Internet {
                    h.host.clone()
                } else {
                    match h.place {
                        crate::privacy::net::Place::Pc => "this PC".to_string(),
                        crate::privacy::net::Place::Network => "your network".to_string(),
                        crate::privacy::net::Place::Internet => "a host left out".to_string(),
                    }
                }
            })
            .collect();
        wheres.sort();
        wheres.dedup();
        out.push_str(&format!("  to: {}\n", wheres.join(", ")));
    }
    out
}

// ---- system information ----------------------------------------------------------------

#[cfg(windows)]
fn reg(subkey: &str, value: &str) -> Option<Value> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, REG_DWORD, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    let mut buf = [0u16; 512];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(subkey),
            &HSTRING::from(value),
            RRF_RT_REG_SZ | RRF_RT_REG_DWORD,
            Some(&mut kind),
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    if kind == REG_DWORD {
        let bytes = [buf[0].to_le_bytes(), buf[1].to_le_bytes()].concat();
        return Some(json!(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])));
    }
    if kind == REG_SZ {
        let units = (size as usize / 2).min(buf.len());
        return Some(json!(String::from_utf16_lossy(&buf[..units]).trim_end_matches('\0').trim().to_string()));
    }
    None
}

#[cfg(windows)]
fn memory_bytes() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    Some(m.ullTotalPhys)
}

/// `system.txt`: this PC and this Yap, in a few lines.
pub fn system_info(app: &AppHandle) -> String {
    let version = app.package_info().version.to_string();
    let channel = if cfg!(debug_assertions) {
        "dev build"
    } else if version.contains("nightly") {
        "nightly"
    } else {
        "stable"
    };
    let engines = if cfg!(feature = "engines") { "speech engines included" } else { "no speech engines (stub build)" };
    let install = if crate::shell::portable::is_portable() { "portable" } else { "installed" };
    let mut lines = vec![format!("Yap: {version} ({channel}, {install}, {engines})")];
    #[cfg(windows)]
    {
        const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let s = |v: &str| reg(NT, v).and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let build = s("CurrentBuild");
        let mut product = s("ProductName");
        // Windows 11 still says "Windows 10" here.
        if build.parse::<u32>().is_ok_and(|b| b >= 22_000) {
            product = product.replacen("Windows 10", "Windows 11", 1);
        }
        let ubr = reg(NT, "UBR").and_then(|v| v.as_u64()).map(|u| format!(".{u}")).unwrap_or_default();
        lines.push(format!("Windows: {product} {}, build {build}{ubr}", s("DisplayVersion")).replace("  ", " "));
        let cpu = reg(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".into());
        let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
        lines.push(format!("Processor: {cpu} · {cores} logical cores"));
        if let Some(m) = memory_bytes() {
            lines.push(format!("Memory: {:.0} GB", m as f64 / 1_073_741_824.0));
        }
    }
    match crate::ai::gpu::this_pc() {
        Some(g) => lines.push(format!("Graphics card: {} · {:.1} GB of its own", g.name, g.vram_mib as f64 / 1024.0)),
        None => lines.push("Graphics card: none Yap can use (or only the processor's)".into()),
    }
    let cfg = crate::config::load();
    lines.push(format!("Speech model: {} · runs on: {}", cfg.model_size, cfg.stt_device));
    lines.push(format!("Made: {}", chrono::Local::now().format("%Y-%m-%d %H:%M (this PC's time)")));
    lines.join("\n") + "\n"
}

// ---- the file --------------------------------------------------------------------------

/// What goes into the file.
pub struct Inputs {
    pub system: String,
    /// config.json as `YapConfig` serialises.
    pub config: Value,
    /// The days of log, newest first: (file name, its text).
    pub logs: Vec<(String, String)>,
    pub entries: Vec<crate::privacy::net::Entry>,
    pub now: u64,
    pub made: String,
}

/// The file's contents, in order.
pub struct Bundle {
    pub files: Vec<(String, Vec<u8>)>,
    pub settings: String,
    pub connections: String,
    pub errors: String,
    pub system: String,
    pub name: String,
}

const README: &str = "This is a diagnostic file from Yap, made on {made}. Nothing in it was sent anywhere: \
you saved it yourself. Look through it before you share it.\n\n\
What's in it\n\
- system.txt: Windows' version, the processor, memory, the graphics card, Yap's version and channel.\n\
- settings.json: which features, providers and models are on, switches and shortcuts. Not in it: API keys \
(only which services have one), About you, names, your dictionary's words, prompts you wrote, device names, \
your servers' addresses, memory, notes, chats, meetings and history. `leftOut` names the settings left out.\n\
- connections.txt: the network connections Yap made in the last 7 days, by purpose (counts and sizes).\n\
- errors.txt: the latest warnings and errors from Yap's log.\n\
- logs/: Yap's log for the last {days} days, info and above. Debug lines (which can hold what you said) are \
left out; your user name, PC name, email addresses, keys and the details of links are masked.\n";

/// Build the file's contents from `inputs`. Pure.
pub fn build(inputs: &Inputs, scrub: &Scrub) -> Bundle {
    let settings = serde_json::to_string_pretty(&settings_summary(&inputs.config)).unwrap_or_default();
    let connections = connections_text(&inputs.entries, inputs.now);
    let mut errors: Vec<String> = Vec::new();
    let mut logs: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, (name, text)) in inputs.logs.iter().enumerate() {
        let (kept, errs) = filter_log(text, scrub);
        // Newest first: the newest day's errors come last, as in the log.
        let mut errs = errs;
        errs.extend(std::mem::take(&mut errors));
        errors = errs;
        if i < LOG_DAYS {
            let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
            logs.push((format!("logs/{safe}.txt"), kept.into_bytes()));
        }
    }
    let skip = errors.len().saturating_sub(MAX_ERRORS);
    let errors = if errors.is_empty() {
        "No warnings or errors in the last 7 days of the log.\n".to_string()
    } else {
        format!("The latest {} warnings and errors from Yap's log (scrubbed like the logs):\n\n{}\n", errors.len() - skip, errors[skip..].join("\n"))
    };
    let system = scrub.line(&inputs.system);
    let mut files = vec![
        ("README.txt".to_string(), README.replace("{made}", &inputs.made).replace("{days}", &LOG_DAYS.to_string()).into_bytes()),
        ("system.txt".to_string(), system.clone().into_bytes()),
        ("settings.json".to_string(), settings.clone().into_bytes()),
        ("connections.txt".to_string(), connections.clone().into_bytes()),
        ("errors.txt".to_string(), errors.clone().into_bytes()),
    ];
    files.extend(logs);
    let day = inputs.made.get(..10).unwrap_or("today").to_string();
    Bundle { files, settings, connections, errors, system, name: format!("Yap diagnostics {day}.zip") }
}

impl Bundle {
    /// The .zip.
    pub fn zip(&self) -> Result<Vec<u8>, String> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &self.files {
            z.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            z.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(z.finish().map_err(|e| e.to_string())?.into_inner())
    }
}

/// The newest `days` daily logs (`yap.log.YYYY-MM-DD`), newest first: each
/// one's last [`LOG_READ_BYTES`].
fn read_logs(dir: &Path, days: usize) -> Vec<(String, String)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("yap.log")))
        .collect();
    files.sort();
    files.reverse();
    files
        .into_iter()
        .take(days)
        .filter_map(|p| {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(&p).ok()?;
            let len = f.metadata().ok()?.len();
            if len > LOG_READ_BYTES {
                f.seek(SeekFrom::Start(len - LOG_READ_BYTES)).ok()?;
            }
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).ok()?;
            let name = p.file_name()?.to_string_lossy().into_owned();
            Some((name, String::from_utf8_lossy(&bytes).into_owned()))
        })
        .collect()
}

// ---- commands --------------------------------------------------------------------------

/// The file last shown: what Save writes.
static LAST: Mutex<Option<Bundle>> = Mutex::new(None);
/// Where it was saved last (Show in folder).
static SAVED: Mutex<Option<PathBuf>> = Mutex::new(None);
/// A test run's save path, in place of Windows' Save dialog
/// ([`e2e_save_path`]).
#[cfg(debug_assertions)]
static E2E_SAVE: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub name: String,
    pub files: Vec<FileInfo>,
    pub total: u64,
    pub system: String,
    pub settings: String,
    pub connections: String,
    pub errors: String,
}

fn make(app: &AppHandle) -> Bundle {
    let cfg = serde_json::to_value(crate::config::load()).unwrap_or(Value::Null);
    let inputs = Inputs {
        system: system_info(app),
        config: cfg,
        logs: read_logs(&crate::config::data_dir().join("logs"), ERROR_DAYS),
        entries: crate::privacy::net::entries(),
        now: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        made: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
    };
    build(&inputs, &Scrub::this_pc())
}

/// Report a problem: build the file and show what's in it. Saving writes
/// this very file.
#[tauri::command]
pub async fn diagnostics_preview(app: AppHandle) -> Result<Preview, String> {
    let bundle = tauri::async_runtime::spawn_blocking(move || make(&app)).await.map_err(|e| e.to_string())?;
    let files: Vec<FileInfo> = bundle.files.iter().map(|(n, b)| FileInfo { name: n.clone(), bytes: b.len() as u64 }).collect();
    let preview = Preview {
        name: bundle.name.clone(),
        total: files.iter().map(|f| f.bytes).sum(),
        files,
        system: bundle.system.clone(),
        settings: bundle.settings.clone(),
        connections: bundle.connections.clone(),
        errors: bundle.errors.clone(),
    };
    tracing::info!(files = preview.files.len(), bytes = preview.total, "diagnostics: preview made");
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(bundle);
    Ok(preview)
}

/// Save the file shown where the person picks (Windows' Save dialog); the
/// path, or `None` when they cancelled. Never uploaded.
#[tauri::command]
pub async fn diagnostics_save(app: AppHandle, window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let (bytes, name) = {
        let last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        let bundle = last.as_ref().ok_or("Make the file first (Report a problem).")?;
        (bundle.zip()?, bundle.name.clone())
    };
    let path = pick_path(&app, window, &name).await?;
    let Some(path) = path else { return Ok(None) };
    let to = path.clone();
    tauri::async_runtime::spawn_blocking(move || std::fs::write(&to, &bytes))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Couldn't save the file: {e}"))?;
    tracing::info!("diagnostics: saved");
    *SAVED.lock().unwrap_or_else(|p| p.into_inner()) = Some(path.clone());
    Ok(Some(path.to_string_lossy().into_owned()))
}

async fn pick_path(app: &AppHandle, window: tauri::WebviewWindow, name: &str) -> Result<Option<PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        // A test run never opens Windows' dialog: it names the path first.
        return E2E_SAVE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(Some)
            .ok_or_else(|| "A test run names the save path first (e2e_save_path).".to_string());
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Save the diagnostic file")
        .set_file_name(name)
        .add_filter("Zip archive", &["zip"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| e.to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

/// Show the saved file in File Explorer.
#[tauri::command]
pub fn diagnostics_reveal(app: AppHandle) -> Result<(), String> {
    let Some(path) = SAVED.lock().unwrap_or_else(|p| p.into_inner()).clone() else { return Ok(()) };
    if crate::shell::e2e::active() {
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(&path).map_err(|e| e.to_string())
}

/// GitHub's new-issue page for Yap, with a title and the template: Yap's
/// version and Windows' only. Never the file, nothing the person wrote.
#[tauri::command]
pub fn diagnostics_issue_url(app: AppHandle) -> String {
    let system = LAST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|b| b.system.clone())
        .unwrap_or_else(|| system_info(&app));
    issue_url(&system)
}

/// Pure: the issue page for `system` (its Yap and Windows lines only).
pub fn issue_url(system: &str) -> String {
    let pick = |prefix: &str| system.lines().find(|l| l.starts_with(prefix)).unwrap_or("").to_string();
    let body = format!(
        "**What happened?**\n\n\n**What did you expect to happen?**\n\n\n**Steps to reproduce**\n1. \n\n---\n{}\n{}\n\n\
         <!-- If you saved a diagnostic file (Settings → About → Report a problem), look through it first, \
         then drag it here if you're happy to share it. Yap never uploads it. -->\n",
        pick("Yap:"),
        pick("Windows:")
    );
    reqwest::Url::parse_with_params(REPO_ISSUES, &[("title", "Problem: "), ("body", body.as_str())])
        .map(|u| u.to_string())
        .unwrap_or_else(|_| REPO_ISSUES.to_string())
}

/// Test mode: the path the next Save writes to, in place of the dialog.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_save_path(path: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    *E2E_SAVE.lock().unwrap_or_else(|p| p.into_inner()) = Some(PathBuf::from(path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fictional people and made-up secrets only.
    const KEY_GROQ: &str = "gsk_FAKEfake1234567890abcdefGHIJ";
    const KEY_EXA: &str = "exa-fake-0000-1111-2222-333344445555";
    const KEY_ELEVEN: &str = "sk_fake_eleven_998877665544332211";
    const KEY_SCOPE: &str = "sk-proj-FAKEscopeKEY0123456789";
    const SECRETS: &[&str] = &[
        KEY_GROQ,
        KEY_EXA,
        KEY_ELEVEN,
        KEY_SCOPE,
        "Robin Example",
        "I look after the allotment on Example Lane",
        "Kubernetes-Fakeword",
        "kubernets",
        "Jarvisfake",
        "Robin's Earbuds",
        "Speakers (Studio Fakebox)",
        "Always write like a pirate",
        "We record this call for our notes, Robin",
        "192.168.7.77",
        "fakebox.local",
        "Voice of Robin",
        "robin@example.com",
        "C:\\Users\\robinx",
        "ROBINX-PC",
        "transcript words that must stay private",
        "secret question about the allotment",
        "private-0123456789abcdef0123",
    ];

    fn seeded_config() -> Value {
        let mut cfg = crate::config::YapConfig {
            pp_provider: "groq".into(),
            pp_api_key: KEY_GROQ.into(),
            about_name: "Robin Example".into(),
            about_me: "I look after the allotment on Example Lane".into(),
            region_country: "GB".into(),
            agent_name: "Jarvisfake".into(),
            input_device: Some("Headset (Robin's Earbuds)".into()),
            output_device: Some("Speakers (Studio Fakebox)".into()),
            pp_prompt: "Always write like a pirate".into(),
            meeting_consent_message: "We record this call for our notes, Robin".into(),
            pp_base_url: "http://192.168.7.77:8080/v1?key=abc".into(),
            ..Default::default()
        };
        cfg.pp_api_keys.insert("groq".into(), KEY_GROQ.into());
        cfg.pp_api_keys.insert("openai".into(), String::new());
        cfg.web_search_keys.insert("exa".into(), KEY_EXA.into());
        cfg.tts_cloud_keys.insert("elevenlabs".into(), KEY_ELEVEN.into());
        cfg.dictionary.push(serde_json::from_value(json!({ "from": "kubernets", "to": "Kubernetes-Fakeword" })).unwrap());
        cfg.dictionary.push(serde_json::from_value(json!({ "from": "sig", "to": "Robin Example, Example Lane", "snippet": true })).unwrap());
        cfg.llm_scopes.insert(
            "chat".into(),
            serde_json::from_value(json!({ "enabled": true, "provider": "custom", "baseUrl": "http://fakebox.local:1234/v1", "model": "fake-model", "apiKey": KEY_SCOPE, "prompt": "Always write like a pirate" })).unwrap(),
        );
        cfg.tts_cloud.insert(
            "elevenlabs".into(),
            serde_json::from_value(json!({ "voice": "fakeVoiceId123", "voiceName": "Voice of Robin", "model": "eleven_flash_v2_5" })).unwrap(),
        );
        serde_json::to_value(cfg).unwrap()
    }

    fn scrub() -> Scrub {
        Scrub::new(vec![
            ("C:\\Users\\robinx".into(), "%USERPROFILE%"),
            ("robinx".into(), "<user>"),
            ("ROBINX-PC".into(), "<pc>"),
            ("D:\\Apps\\Yap\\Data".into(), "<data>"),
        ])
    }

    fn seeded_log() -> String {
        [
            "2026-10-24T09:00:00.000001Z  INFO yap_lib::lib: Yap started version=0.1.1",
            "2026-10-24T09:00:01.000001Z DEBUG yap_lib::pipeline: cleaned text=\"transcript words that must stay private\"",
            "    continued: secret question about the allotment",
            "2026-10-24T09:00:02.000001Z  INFO yap_lib::config: data dir path=C:\\Users\\robinx\\AppData\\Roaming\\yap on ROBINX-PC",
            "2026-10-24T09:00:03.000001Z  WARN yap_lib::auth: sign-in for robin@example.com failed with Bearer sk-proj-FAKEscopeKEY0123456789",
            "2026-10-24T09:00:04.000001Z ERROR yap_lib::calendar: couldn't read https://calendar.example.com/ical/private-0123456789abcdef0123/basic.ics?token=abc",
            "2026-10-24T09:00:05.000001Z  INFO yap_lib::stt: Downloading model url=https://blob.handy.computer/parakeet-v3.tar.gz?sig=FAKE123token456",
            "2026-10-24T09:00:06.000001Z  INFO yap_lib::llm: key in a list keys=gsk_FAKEfake1234567890abcdefGHIJ",
            "2026-10-24T09:00:07.000001Z TRACE yap_lib::web: query=\"secret question about the allotment\"",
        ]
        .join("\n")
    }

    fn seeded_entries() -> Vec<crate::privacy::net::Entry> {
        let mk = |label: &str, host: &str, kind: &str| crate::privacy::net::Entry {
            at: 1_800_000_000,
            last: 1_800_000_000,
            kind: kind.into(),
            label: label.into(),
            feature: "Chat".into(),
            host: host.into(),
            place: crate::privacy::net::place_of(host),
            count: 2,
            failed: 0,
            status: 200,
            sent: 2048,
            received: 4096,
        };
        vec![
            mk("Chat · Groq", "api.groq.com", "ai"),
            mk("Reading a web page", "allotment-forum.example", "web"),
            mk("Chat · Custom endpoint", "fakebox.local:1234", "ai"),
            mk("Calendar sync · iCal link", "calendar.example.com", "calendar"),
            mk("AI cleanup · On-device model", "127.0.0.1", "ai"),
        ]
    }

    fn bundle() -> Bundle {
        let inputs = Inputs {
            system: "Yap: 0.1.1 (dev build, portable, no speech engines (stub build))\nWindows: Windows 11 Home 24H2, build 26200.1\nProcessor: Fake CPU · 8 logical cores\n".into(),
            config: seeded_config(),
            logs: vec![("yap.log.2026-10-24".into(), seeded_log())],
            entries: seeded_entries(),
            now: 1_800_000_100,
            made: "2026-10-24 21:05".into(),
        };
        build(&inputs, &scrub())
    }

    /// The test in the spirit of `log_privacy.rs`: a bundle made from a
    /// config, a log and connections seeded with fake keys and personal
    /// details holds none of them, in any file, zipped or not.
    #[test]
    fn a_bundle_holds_no_keys_or_personal_details() {
        let b = bundle();
        let names: Vec<&str> = b.files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["README.txt", "system.txt", "settings.json", "connections.txt", "errors.txt", "logs/yap.log.2026-10-24.txt"]);
        let all: String = b.files.iter().map(|(_, bytes)| String::from_utf8_lossy(bytes).into_owned()).collect::<Vec<_>>().join("\n");
        for secret in SECRETS {
            assert!(!all.to_lowercase().contains(&secret.to_lowercase()), "{secret:?} is in the bundle");
        }
        // Nor in the zip itself (stored text would show; deflated can't).
        let zip = b.zip().unwrap();
        assert_eq!(&zip[..4], b"PK\x03\x04");
        for secret in SECRETS {
            assert!(!zip.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
    }

    #[test]
    fn the_settings_summary_keeps_choices_and_says_what_is_set() {
        let s = settings_summary(&seeded_config());
        assert_eq!(s["ppProvider"], "groq");
        assert_eq!(s["hotkey"], "kb:120");
        assert_eq!(s["postProcessEnabled"], false);
        assert_eq!(s["ppApiKeys"], json!(["groq"]));
        assert_eq!(s["webSearchKeys"], json!(["exa"]));
        assert_eq!(s["ttsCloudKeys"], json!(["elevenlabs"]));
        assert_eq!(s["aboutName"], "set");
        assert_eq!(s["regionCountry"], "chosen (left out)");
        assert_eq!(s["inputDevice"], "chosen (its name left out)");
        assert_eq!(s["dictionary"], json!({ "corrections": 1, "snippets": 1 }));
        assert_eq!(s["ppBaseUrl"], "a server on your network");
        assert_eq!(s["llmScopes"]["chat"]["provider"], "custom");
        assert_eq!(s["llmScopes"]["chat"]["endpoint"], "a server on your network");
        assert_eq!(s["llmScopes"]["chat"]["key"], "set");
        assert_eq!(s["ttsCloud"]["elevenlabs"]["voice"], "chosen (left out)");
        assert!(!s["leftOut"].as_array().unwrap().contains(&json!("hotkey")));
    }

    /// A setting the summary doesn't know is left out (by name), never copied.
    #[test]
    fn an_unknown_setting_is_left_out() {
        let mut cfg = seeded_config();
        cfg["aNewSettingHoldingText"] = json!("Robin Example's secret");
        let s = settings_summary(&cfg);
        assert!(s.get("aNewSettingHoldingText").is_none());
        assert!(s["leftOut"].as_array().unwrap().contains(&json!("aNewSettingHoldingText")));
        assert!(!s.to_string().contains("secret"));
    }

    /// Every name the summary copies is a real setting (no typos).
    #[test]
    fn every_copied_setting_exists() {
        let cfg = serde_json::to_value(crate::config::YapConfig::default()).unwrap();
        for k in COPY {
            assert!(cfg.get(*k).is_some(), "{k} isn't a setting");
        }
    }

    #[test]
    fn the_log_keeps_info_and_above_scrubbed() {
        let (kept, errors) = filter_log(&seeded_log(), &scrub());
        assert!(kept.contains("Yap started version=0.1.1"));
        assert!(!kept.contains("DEBUG") && !kept.contains("TRACE") && !kept.contains("continued"));
        assert!(kept.contains("path=%USERPROFILE%\\AppData\\Roaming\\yap on <pc>"), "{kept}");
        assert!(kept.contains("sign-in for <email> failed with <key>"), "{kept}");
        assert!(kept.contains("couldn't read https://calendar.example.com/…"), "{kept}");
        assert!(kept.contains("url=https://blob.handy.computer/parakeet-v3.tar.gz?…"), "{kept}");
        assert!(kept.contains("keys=<key>"), "{kept}");
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn any_windows_user_folder_is_masked() {
        let s = Scrub::new(Vec::new());
        assert_eq!(s.line("model at C:\\Users\\Someone Else\\models"), "model at C:\\Users\\<user>\\models");
        assert_eq!(s.line("d:/users/alex/yap"), "d:/users/<user>/yap");
    }

    #[test]
    fn a_name_is_masked_as_a_whole_word_only() {
        let s = Scrub::new(vec![("Dan".into(), "<user>")]);
        assert_eq!(s.line("Dan's dance, DAN."), "<user>'s dance, <user>.");
    }

    #[test]
    fn connections_name_known_services_only() {
        let text = connections_text(&seeded_entries(), 1_800_000_100);
        assert!(text.contains("Chat · Groq: 2 requests, 0 failed, 2.0 KB sent, 4.0 KB received"));
        assert!(text.contains("to: api.groq.com"));
        assert!(text.contains("Reading a web page: 2 requests"));
        assert!(!text.contains("allotment-forum") && !text.contains("fakebox") && !text.contains("calendar.example.com"));
        assert!(text.contains("to: your network") && text.contains("to: this PC") && text.contains("to: a host left out"));
    }

    #[test]
    fn the_issue_page_carries_the_version_and_windows_only() {
        let b = bundle();
        let url = issue_url(&b.system);
        assert!(url.starts_with("https://github.com/nayballs/Yap/issues/new?title=Problem"));
        let parsed = reqwest::Url::parse(&url).unwrap();
        let body: String = parsed.query_pairs().find(|(k, _)| k == "body").unwrap().1.into_owned();
        assert!(body.contains("Yap: 0.1.1 (dev build"));
        assert!(body.contains("Windows: Windows 11 Home 24H2"));
        assert!(!body.contains("Processor"));
    }
}
