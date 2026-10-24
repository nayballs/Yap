//! **Who the call app shows talking** (experimental, behind
//! `config.meeting_call_names`, off, no setting shows it yet): while a
//! meeting records during a detected call, Yap reads the call window's UI
//! Automation tree — the speaking tile in Google Meet and Teams (in Edge or
//! Chrome, or the new Teams app, all Chromium underneath), Zoom's "Active
//! speaker" — about once a second, and dates each reading on the meeting's
//! clock (the "Them" side's sample position, the speaker engine's clock).
//! Lined up with the engine's voices ([`votes_for`]), a name the app showed
//! again and again while one voice spoke names that voice (`naming.rs`:
//! source "the call app", above the calendar and the AI). Granola and Wispr
//! Flow read speakers this way; Wispr's defaults are the markers we start
//! from (Meet's `kssMZb` / `oZRSLe` classes, Teams' `vdi-frame-occlusion`,
//! Zoom's "Active speaker").
//!
//! **Read only.** No click, no focus, no pattern invoked, no window moved,
//! no screenshot: one cached UI Automation read of the window a reading
//! ([`win::snapshot`]). Nothing is kept past the meeting but the names it
//! put on voices. Names never reach the log above debug (counts only).
//!
//! **Not yet checked against a real call** (nobody can join one from the
//! build machine): the reading rules ([`Profile`], built in from
//! `call_ui/profiles.json`, overridable by `<data>/call_speakers.json`
//! without a rebuild) are tested on fixtures built from what's known
//! (`call_ui/fixtures/`). `yap.exe call-dump` ([`dump_main`]) records a
//! real call window's tree to a file — the capture steps are in
//! `docs/architecture/speakers.md`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::naming::PlatformVote;

// ---- a UI Automation tree, as read (and as the dump writes it) ----

/// One element of a call window's UI Automation tree.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    /// The control type ("Button", "Group", "Text", "ListItem", "Image"…).
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// UIA's ClassName (in a Chromium page: the element's `class`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// UIA's AutomationId (in a Chromium page: the element's `id`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// ItemStatus.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// HelpText.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    /// AriaRole.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// AriaProperties ("pressed=true;…").
    #[serde(skip_serializing_if = "String::is_empty")]
    pub aria: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub offscreen: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

impl Node {
    fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }

    fn classes(&self) -> impl Iterator<Item = &str> {
        self.class.split_whitespace()
    }

    /// Every text this element shows or says about itself.
    fn texts(&self) -> [&str; 4] {
        [&self.name, &self.status, &self.help, &self.aria]
    }
}

/// A dump file (`yap.exe call-dump`, and the test fixtures): the call app,
/// and the window's tree read every so often.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Dump {
    pub version: u32,
    /// "teams" | "zoom" | "meet" | … (`meeting_detect`'s app ids).
    pub app: String,
    /// What made it ("yap.exe call-dump 0.1.1 …", or how a fixture was built).
    pub source: String,
    pub snapshots: Vec<Snapshot>,
}

/// One read of the window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Snapshot {
    /// Since the dump started.
    pub at_ms: u64,
    /// How long the read took.
    pub read_ms: u64,
    pub root: Node,
    /// Fixtures: what a correct reader finds (who's speaking; `null` = not
    /// checked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect: Option<Expect>,
    /// The dump: what this reader found, to compare by eye.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<Reading>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Expect {
    pub speaking: Vec<String>,
    pub self_speaking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_name: Option<String>,
}

// ---- reading it: the rules per call app ----

/// How one call app shows who's talking.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    /// Class tokens on the element that lights up for whoever is talking.
    pub speaking_class: Vec<String>,
    /// Class tokens on a participant's tile (where their name is).
    pub tile_class: Vec<String>,
    /// Texts saying someone is talking, `{name}` where the name is ("{name},
    /// speaking", "Active speaker: {name}").
    pub speaking_text: Vec<String>,
    /// Marks of yourself on a tile ("(You)", "(me)").
    pub self_mark: Vec<String>,
    /// Texts saying you are talking ("you are speaking").
    pub self_speaking_text: Vec<String>,
    /// Bits of a tile's name that aren't the name ("(Guest)", ", Muted").
    pub strip: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ProfileFile {
    apps: BTreeMap<String, Profile>,
}

const BUILT_IN: &str = include_str!("call_ui/profiles.json");

/// The rules for `app` ("teams", "zoom", "meet"…; anything else: the
/// generic ones): the built-in file's, unless `<data>/call_speakers.json`
/// has an entry for it (a new marker found in a capture, without a rebuild).
pub fn profile(app: &str) -> Profile {
    let read = |text: &str| serde_json::from_str::<ProfileFile>(text).ok();
    let built_in = read(BUILT_IN).unwrap_or_default();
    let local = std::fs::read_to_string(crate::config::data_dir().join("call_speakers.json"))
        .ok()
        .and_then(|t| read(&t));
    let pick = |file: &ProfileFile| file.apps.get(app).or_else(|| file.apps.get("generic")).cloned();
    local
        .as_ref()
        .and_then(|f| f.apps.get(app).cloned())
        .or_else(|| pick(&built_in))
        .unwrap_or_default()
}

/// A profile with its texts made into matchers.
pub struct Rules {
    profile: Profile,
    speaking: Vec<regex::Regex>,
}

impl Rules {
    pub fn new(profile: Profile) -> Rules {
        let speaking = profile
            .speaking_text
            .iter()
            .filter_map(|t| {
                let pattern = regex::escape(t).replace(r"\{name\}", r"(?P<name>[^,;:\n]{2,60}?)");
                regex::RegexBuilder::new(&format!(r"(?:^|[\s,;(]){pattern}(?:$|[\s,;.)])"))
                    .case_insensitive(true)
                    .build()
                    .ok()
            })
            .collect();
        Rules { profile, speaking }
    }
}

/// What one read of the window says.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Reading {
    /// Who the app shows talking (not you).
    pub speaking: Vec<String>,
    /// You're talking (by the app's marks).
    pub self_speaking: bool,
    /// Your name in the app, when it marks your tile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_name: Option<String>,
    /// Elements read.
    pub nodes: usize,
}

/// Words that are a call app's controls, never a person.
const UI_WORDS: [&str; 46] = [
    "mute", "unmute", "microphone", "mic", "camera", "video", "share", "screen", "chat", "people", "participants",
    "raise", "hand", "reactions", "react", "more", "options", "leave", "end", "call", "settings", "record",
    "recording", "captions", "apps", "view", "layout", "pin", "spotlight", "gallery", "meeting", "join", "close",
    "minimize", "maximize", "toolbar", "menu", "button", "audio", "speaker", "speakers", "notifications", "host",
    "invite", "breakout", "rooms",
];

/// A tile's text as a person's name: the marks and states taken off,
/// "Shah, Priya" as "Priya Shah"; `None` when what's left isn't a name.
pub fn clean_name(raw: &str, rules: &Rules) -> Option<String> {
    let mut s = raw.to_string();
    for strip in &rules.profile.strip {
        let re = regex::RegexBuilder::new(&regex::escape(strip)).case_insensitive(true).build().ok()?;
        s = re.replace_all(&s, "").into_owned();
    }
    for mark in &rules.profile.self_mark {
        let re = regex::RegexBuilder::new(&regex::escape(mark)).case_insensitive(true).build().ok()?;
        s = re.replace_all(&s, "").into_owned();
    }
    // Any other bracketed bit: "(Contoso)", "[External]".
    let brackets = regex::Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").ok()?;
    s = brackets.replace_all(&s, "").into_owned();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_matches(|c: char| c == ',' || c == ';' || c == ':' || c == '-' || c.is_whitespace());
    // "Shah, Priya" (a directory's order): "Priya Shah".
    let s = match s.split_once(", ") {
        Some((last, first))
            if !last.contains(' ')
                && first.split_whitespace().count() <= 2
                && first.chars().next().is_some_and(char::is_uppercase)
                && last.chars().next().is_some_and(char::is_uppercase) =>
        {
            format!("{first} {last}")
        }
        Some(_) => return None,
        None => s.to_string(),
    };
    let words: Vec<&str> = s.split_whitespace().collect();
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    let ui = words.iter().any(|w| UI_WORDS.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric())));
    if words.is_empty() || words.len() > 5 || letters < 2 || s.chars().count() > 60 || ui {
        return None;
    }
    Some(s)
}

/// Marked as yourself ("Imogen Hart (You)", Meet's "You").
fn is_self(raw: &str, rules: &Rules) -> bool {
    let lower = raw.to_lowercase();
    raw.trim().eq_ignore_ascii_case("you") || rules.profile.self_mark.iter().any(|m| lower.contains(&m.to_lowercase()))
}

/// The name on the tile the speaking marker at the end of `path` sits in:
/// the nearest element with a tile class (its name, else the first name in
/// it), else the nearest element with a name that is one (≤ 4 up).
fn tile_name(path: &[&Node], rules: &Rules) -> Option<(String, bool)> {
    let tile_class = |n: &Node| n.classes().any(|c| rules.profile.tile_class.iter().any(|t| t == c));
    let named = |n: &Node| clean_name(&n.name, rules).map(|c| (c, is_self(&n.name, rules)));
    if let Some(tile) = path.iter().rev().find(|n| tile_class(n)) {
        if let Some(found) = named(tile) {
            return Some(found);
        }
        return first_name_in(tile, rules);
    }
    path.iter().rev().take(5).find_map(|n| named(n))
}

fn first_name_in(n: &Node, rules: &Rules) -> Option<(String, bool)> {
    for c in &n.children {
        if let Some(name) = clean_name(&c.name, rules) {
            return Some((name, is_self(&c.name, rules)));
        }
        if let Some(found) = first_name_in(c, rules) {
            return Some(found);
        }
    }
    None
}

/// Who `root` (a call window's tree) shows talking.
pub fn read(root: &Node, rules: &Rules) -> Reading {
    let mut r = Reading::default();
    let mut path: Vec<&Node> = Vec::new();
    walk(root, &mut path, rules, &mut r);
    if let Some(me) = r.self_name.clone() {
        if r.speaking.iter().any(|s| s.eq_ignore_ascii_case(&me)) {
            r.self_speaking = true;
        }
        r.speaking.retain(|s| !s.eq_ignore_ascii_case(&me));
    }
    let mut seen: Vec<String> = Vec::new();
    r.speaking.retain(|s| {
        let fresh = !seen.iter().any(|x| x.eq_ignore_ascii_case(s));
        seen.push(s.clone());
        fresh
    });
    r
}

fn walk<'a>(n: &'a Node, path: &mut Vec<&'a Node>, rules: &Rules, r: &mut Reading) {
    r.nodes += 1;
    path.push(n);
    // Your own tile: your name.
    if !n.name.is_empty() && r.self_name.is_none() && rules.profile.self_mark.iter().any(|m| n.name.to_lowercase().contains(&m.to_lowercase())) {
        r.self_name = clean_name(&n.name, rules);
    }
    // The speaking marker on a tile.
    if n.classes().any(|c| rules.profile.speaking_class.iter().any(|s| s == c)) {
        match tile_name(path, rules) {
            Some((_, true)) => r.self_speaking = true,
            Some((name, false)) => r.speaking.push(name),
            None => {}
        }
    }
    // A text saying who's talking.
    for text in n.texts() {
        if text.is_empty() {
            continue;
        }
        let lower = text.to_lowercase();
        if rules.profile.self_speaking_text.iter().any(|t| lower.contains(&t.to_lowercase())) {
            r.self_speaking = true;
            continue;
        }
        for re in &rules.speaking {
            if let Some(name) = re.captures(text).and_then(|c| c.name("name")) {
                if is_self(name.as_str(), rules) {
                    r.self_speaking = true;
                } else if let Some(name) = clean_name(name.as_str(), rules) {
                    r.speaking.push(name);
                }
            }
        }
    }
    for c in &n.children {
        walk(c, path, rules, r);
    }
    path.pop();
}

// ---- readings on the meeting's clock, and the voices they name ----

/// What the app showed over a stretch of the meeting's clock (readings that
/// said the same, run together).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub from_ms: u64,
    pub to_ms: u64,
    pub speaking: Vec<String>,
    #[serde(default)]
    pub me: bool,
}

/// How often the window is read.
pub const READ_EVERY: Duration = Duration::from_millis(800);
/// The app's highlight follows speech by a moment and lingers after it: a
/// voice's stretch is matched with what the app showed from this long after
/// it began…
const LAG_FROM_MS: u64 = 300;
/// …to this long after it ended.
const LAG_TO_MS: u64 = 1_200;
/// A name for a voice needs this many readings' worth of agreeing time…
const MIN_READS: u32 = 3;
/// …and this share of the voice's single-speaker time.
const MIN_SHARE: f32 = 0.6;

/// One stretch of a voice's speech on the meeting's clock.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceTurn {
    pub voice: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Line up what the app showed (`spans`) with who spoke when (`turns`): for
/// each voice, the name the app showed alone (one person talking, not
/// you) for most of its speech. Your own name (`me`) never names a call
/// voice. Pure.
pub fn align(spans: &[Span], turns: &[VoiceTurn], me: Option<&str>) -> Vec<PlatformVote> {
    let mut per_voice: BTreeMap<&str, (u64, BTreeMap<&str, u64>)> = BTreeMap::new();
    for t in turns {
        let (from, to) = (t.start_ms + LAG_FROM_MS.min(t.end_ms.saturating_sub(t.start_ms)), t.end_ms + LAG_TO_MS);
        for s in spans.iter().filter(|s| s.speaking.len() == 1 && !s.me) {
            let overlap = s.to_ms.min(to).saturating_sub(s.from_ms.max(from));
            if overlap == 0 {
                continue;
            }
            let name = s.speaking[0].as_str();
            if me.is_some_and(|m| m.eq_ignore_ascii_case(name)) {
                continue;
            }
            let entry = per_voice.entry(t.voice.as_str()).or_default();
            entry.0 += overlap;
            *entry.1.entry(name).or_default() += overlap;
        }
    }
    let read_ms = READ_EVERY.as_millis() as u64;
    per_voice
        .into_iter()
        .filter_map(|(voice, (total, names))| {
            let (name, ms) = names.into_iter().max_by_key(|(_, ms)| *ms)?;
            let reads = (ms / read_ms) as u32;
            let share = ms as f32 / total.max(1) as f32;
            (reads >= MIN_READS && share >= MIN_SHARE).then(|| PlatformVote {
                voice: voice.to_string(),
                name: name.to_string(),
                reads,
                share: (share * 100.0).round() / 100.0,
            })
        })
        .collect()
}

/// A recording whose call window is being read.
struct Watch {
    note_id: u64,
    /// Its speaker-engine session (`None`: no voices told apart).
    session: Option<u64>,
    /// Its first transcript line.
    base: usize,
    spans: Vec<Span>,
    self_name: Option<String>,
    stop: Arc<AtomicBool>,
}

/// At most this many spans a recording (a day of changes every second).
const MAX_SPANS: usize = 50_000;

static WATCHES: Mutex<Vec<Watch>> = Mutex::new(Vec::new());

fn watches() -> MutexGuard<'static, Vec<Watch>> {
    WATCHES.lock().unwrap_or_else(|p| p.into_inner())
}

/// One reading for note `note_id`'s recording, at `at_ms` on its clock.
fn record(note_id: u64, at_ms: u64, reading: &Reading) {
    let mut all = watches();
    let Some(w) = all.iter_mut().find(|w| w.note_id == note_id) else { return };
    if reading.self_name.is_some() {
        w.self_name.clone_from(&reading.self_name);
    }
    let me = reading.self_speaking && reading.speaking.is_empty();
    let step = READ_EVERY.as_millis() as u64;
    if let Some(last) = w.spans.last_mut() {
        // The same as the last reading: that stretch goes on.
        if last.speaking == reading.speaking && last.me == me && at_ms + step >= last.to_ms && at_ms <= last.to_ms + 5 * step {
            last.to_ms = at_ms + step;
            return;
        }
        // A new reading closes the last stretch where it begins.
        if at_ms > last.from_ms && at_ms < last.to_ms {
            last.to_ms = at_ms;
        }
    }
    if w.spans.len() < MAX_SPANS {
        w.spans.push(Span { from_ms: at_ms, to_ms: at_ms + step, speaking: reading.speaking.clone(), me });
    }
}

/// Readings of the call app are coming in for note `note_id`'s recording.
pub fn watching(note_id: u64) -> bool {
    watches().iter().any(|w| w.note_id == note_id && !w.spans.is_empty())
}

/// The call app's names for timeline `tl`'s voices (`voices`: the engine's
/// voices → the note's ids), for `naming.rs`.
pub fn votes_for(tl: &crate::speakers::diarize::Timeline, voices: &super::labels::VoiceIds) -> Vec<PlatformVote> {
    let all = watches();
    let Some(w) = all.iter().find(|w| w.note_id == tl.note_id && w.session.is_none_or(|s| s == tl.session) && !w.spans.is_empty()) else {
        return Vec::new();
    };
    let turns: Vec<VoiceTurn> = tl
        .segments
        .iter()
        .filter_map(|s| {
            voices.get((s.stream, s.speaker)).map(|v| VoiceTurn { voice: v.to_string(), start_ms: s.start_ms, end_ms: s.end_ms })
        })
        .collect();
    align(&w.spans, &turns, w.self_name.as_deref())
}

/// The call app's name for "Them" in note `note_id`'s recording when no
/// voices are told apart: only when one person had the floor all along (a
/// 1:1 call), the lines on the recording's clock lined up with the app.
pub fn votes(note_id: u64) -> Vec<PlatformVote> {
    let (spans, base, me) = {
        let all = watches();
        match all.iter().find(|w| w.note_id == note_id && w.session.is_none() && !w.spans.is_empty()) {
            Some(w) => (w.spans.clone(), w.base, w.self_name.clone()),
            None => return Vec::new(),
        }
    };
    let Some(note) = crate::notes::get(note_id) else { return Vec::new() };
    let turns: Vec<VoiceTurn> = note
        .transcript
        .iter()
        .skip(base)
        .filter(|s| s.source == super::THEM && s.is_talk() && s.speaker.is_empty())
        .filter_map(|s| Some(VoiceTurn { voice: super::THEM.to_string(), start_ms: s.start_ms?, end_ms: s.end_ms? }))
        .collect();
    align(&spans, &turns, me.as_deref()).into_iter().filter(|v| v.share >= 0.9).collect()
}

/// The recording of `note_id` ended and its names are on its voices:
/// forget what the app showed.
pub fn finish(note_id: u64) {
    let mut all = watches();
    for w in all.iter().filter(|w| w.note_id == note_id) {
        w.stop.store(true, Ordering::SeqCst);
    }
    all.retain(|w| w.note_id != note_id);
}

/// A recording into `note_id` started (`meeting::start`, `session`: its
/// speaker-engine session, `base`: its first line). With "read who's
/// talking from the call app" on (config, experimental), a thread reads
/// the live call's window while it records.
pub fn begin(note_id: u64, session: Option<u64>, base: usize) {
    finish(note_id);
    let stop = Arc::new(AtomicBool::new(false));
    watches().push(Watch { note_id, session, base, spans: Vec::new(), self_name: None, stop: Arc::clone(&stop) });
    #[cfg(debug_assertions)]
    if crate::e2e::active() {
        return; // The suite hands readings in (`e2e_call_readings`).
    }
    if !crate::config::load().meeting_call_names {
        return;
    }
    let spawned = std::thread::Builder::new().name("call-ui".into()).spawn(move || poll(note_id, stop));
    if let Err(e) = spawned {
        tracing::warn!("Call names: couldn't start the reader: {e}");
    }
}

/// Read the live call's window every [`READ_EVERY`] while `note_id`
/// records (slower when a read is slow), from its own thread.
fn poll(note_id: u64, stop: Arc<AtomicBool>) {
    let mut window: Option<(isize, &'static str, Rules)> = None;
    let mut looked = Instant::now() - Duration::from_secs(60);
    let mut every = READ_EVERY;
    let (mut reads, mut found) = (0u64, 0u64);
    tracing::info!(note_id, "Call names: reading the call app's window while the meeting records (experimental)");
    while !stop.load(Ordering::SeqCst) && crate::meetings::meeting::recording_note() == Some(note_id) {
        if window.is_none() && looked.elapsed() >= Duration::from_secs(10) {
            looked = Instant::now();
            window = crate::calls::meeting_detect::call_window_exes().and_then(|(app, _)| {
                let hwnd = crate::meetings::notepad::call_window_quiet()?;
                tracing::info!(note_id, app, "Call names: found the call's window");
                Some((hwnd, app, Rules::new(profile(app))))
            });
        }
        if let Some((hwnd, _, rules)) = &window {
            let t0 = Instant::now();
            match win::snapshot(*hwnd) {
                Ok(Some(root)) => {
                    let reading = read(&root, rules);
                    if let Some(clock) = crate::meetings::meeting::them_clock_ms(note_id) {
                        record(note_id, clock, &reading);
                    }
                    reads += 1;
                    found += u64::from(!reading.speaking.is_empty() || reading.self_speaking);
                    if reads % 300 == 1 {
                        tracing::debug!(note_id, reads, found, nodes = reading.nodes, ms = t0.elapsed().as_millis() as u64, "Call names: reading");
                    }
                }
                // Minimised: it shows nothing (Wispr's "hidden").
                Ok(None) => {}
                Err(e) => {
                    tracing::info!(note_id, "Call names: the window can't be read now ({e}); looking again");
                    window = None;
                }
            }
            // A slow read (a huge tree) slows the reading down, never the PC.
            let took = t0.elapsed();
            every = if took > Duration::from_millis(400) { (took * 4).min(Duration::from_secs(5)) } else { READ_EVERY };
        }
        std::thread::sleep(every);
    }
    tracing::info!(note_id, reads, found, "Call names: stopped reading the call app's window");
}

/// Test mode: what the app "showed" for the recording now running
/// (`e2e_call_readings`), as spans on its clock.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_call_readings(spans: Vec<Span>, self_name: Option<String>) -> Result<usize, String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let note_id = crate::meetings::meeting::recording_note().ok_or("No meeting is being recorded")?;
    let mut all = watches();
    let w = all.iter_mut().find(|w| w.note_id == note_id).ok_or("The recording isn't watched")?;
    w.spans = spans;
    w.self_name = self_name;
    Ok(w.spans.len())
}

// ---- Windows: one cached read of the window's tree ----

#[cfg(windows)]
pub mod win {
    use windows::core::Interface;
    use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement, TreeScope_Subtree,
        UIA_AriaPropertiesPropertyId, UIA_AriaRolePropertyId, UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId,
        UIA_ControlTypePropertyId, UIA_HelpTextPropertyId, UIA_IsOffscreenPropertyId, UIA_ItemStatusPropertyId,
        UIA_NamePropertyId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow};

    use super::Node;

    /// A read gives up past this.
    const TIMEOUT_MS: u32 = 2_000;
    /// At most this many elements kept from one read.
    pub const MAX_NODES: usize = 30_000;

    struct Com(bool);

    impl Com {
        fn init() -> Option<Com> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_ok() {
                Some(Com(true))
            } else if hr == RPC_E_CHANGED_MODE {
                Some(Com(false))
            } else {
                None
            }
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    /// Window `hwnd`'s whole UI Automation tree (the raw view: a page's
    /// plain containers carry the classes), in one cached request — only
    /// properties read, nothing invoked. `Ok(None)`: minimised (it shows
    /// nothing then).
    pub fn snapshot(hwnd: isize) -> Result<Option<Node>, String> {
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return Err("the window closed".to_string());
            }
            if IsIconic(h).as_bool() {
                return Ok(None);
            }
        }
        let _com = Com::init().ok_or("COM")?;
        unsafe {
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))
                .map_err(|e| e.to_string())?;
            if let Ok(a2) = automation.cast::<IUIAutomation2>() {
                let _ = a2.SetConnectionTimeout(TIMEOUT_MS);
                let _ = a2.SetTransactionTimeout(TIMEOUT_MS);
            }
            let cache = automation.CreateCacheRequest().map_err(|e| e.to_string())?;
            for p in [
                UIA_ControlTypePropertyId,
                UIA_NamePropertyId,
                UIA_ClassNamePropertyId,
                UIA_AutomationIdPropertyId,
                UIA_ItemStatusPropertyId,
                UIA_HelpTextPropertyId,
                UIA_AriaRolePropertyId,
                UIA_AriaPropertiesPropertyId,
                UIA_IsOffscreenPropertyId,
            ] {
                cache.AddProperty(p).map_err(|e| e.to_string())?;
            }
            cache.SetTreeScope(TreeScope_Subtree).map_err(|e| e.to_string())?;
            let raw = automation.RawViewCondition().map_err(|e| e.to_string())?;
            cache.SetTreeFilter(&raw).map_err(|e| e.to_string())?;
            let root = automation.ElementFromHandleBuildCache(h, &cache).map_err(|e| e.to_string())?;
            let mut count = 0usize;
            Ok(Some(convert(&root, &mut count)))
        }
    }

    unsafe fn convert(e: &IUIAutomationElement, count: &mut usize) -> Node {
        *count += 1;
        let text = |r: windows::core::Result<windows::core::BSTR>| r.map(|b| b.to_string()).unwrap_or_default();
        let mut n = Node {
            kind: e.CachedControlType().map(|t| control_type(t.0)).unwrap_or_default(),
            name: text(e.CachedName()),
            class: text(e.CachedClassName()),
            id: text(e.CachedAutomationId()),
            status: text(e.CachedItemStatus()),
            help: text(e.CachedHelpText()),
            role: text(e.CachedAriaRole()),
            aria: text(e.CachedAriaProperties()),
            offscreen: e.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(false),
            children: Vec::new(),
        };
        if let Ok(children) = e.GetCachedChildren() {
            let len = children.Length().unwrap_or(0);
            for i in 0..len {
                if *count >= MAX_NODES {
                    break;
                }
                if let Ok(c) = children.GetElement(i) {
                    n.children.push(convert(&c, count));
                }
            }
        }
        n
    }

    /// UIA's control type ids by name (the ones call windows use).
    fn control_type(id: i32) -> String {
        match id {
            50000 => "Button",
            50001 => "Calendar",
            50002 => "CheckBox",
            50003 => "ComboBox",
            50004 => "Edit",
            50005 => "Hyperlink",
            50006 => "Image",
            50007 => "ListItem",
            50008 => "List",
            50009 => "Menu",
            50010 => "MenuBar",
            50011 => "MenuItem",
            50012 => "ProgressBar",
            50013 => "RadioButton",
            50014 => "ScrollBar",
            50015 => "Slider",
            50016 => "Spinner",
            50017 => "StatusBar",
            50018 => "Tab",
            50019 => "TabItem",
            50020 => "Text",
            50021 => "ToolBar",
            50022 => "ToolTip",
            50023 => "Tree",
            50024 => "TreeItem",
            50025 => "Custom",
            50026 => "Group",
            50027 => "Thumb",
            50028 => "DataGrid",
            50029 => "DataItem",
            50030 => "Document",
            50031 => "SplitButton",
            50032 => "Window",
            50033 => "Pane",
            50034 => "Header",
            50035 => "HeaderItem",
            50036 => "Table",
            50037 => "TitleBar",
            50038 => "Separator",
            50039 => "SemanticZoom",
            50040 => "AppBar",
            _ => return format!("{id}"),
        }
        .to_string()
    }
}

#[cfg(not(windows))]
pub mod win {
    use super::Node;

    pub fn snapshot(_hwnd: isize) -> Result<Option<Node>, String> {
        Err("UI Automation is Windows only".to_string())
    }
}

// ---- `yap.exe call-dump`: a real call window's tree, to a file ----

const DUMP_USAGE: &str = r#"yap.exe call-dump — record a call window's UI Automation tree (read only) for Yap's
developers, to teach Yap how the call app shows who's talking.

  --window <text>    the window whose title contains <text> (else: the call app's, found by itself)
  --app <id>         teams | zoom | meet | … (the reading rules to try; else guessed)
  --every <ms>       read every <ms> (default 1000)
  --count <n>        this many reads (default 30)
  --out <file>       where (default call-dump-<app>-<time>.json in this folder)
  --names <map>      "Real Name=Priya Shah;Other Name=Sam Patel": names swapped for these in
                     the file, for sharing (check the file after: it holds whatever the
                     window showed, chat messages included)
  --show-names       print who the rules found talking (else counts only)
"#;

/// `yap.exe call-dump …` (main.rs; any build): read a call window's tree
/// every so often and write it, with what the reading rules made of each
/// read, to a JSON file ([`Dump`]). Reads only.
pub fn dump_main(args: Vec<String>) -> i32 {
    let mut window: Option<String> = None;
    let mut app: Option<String> = None;
    let mut every = 1_000u64;
    let mut count = 30usize;
    let mut out: Option<std::path::PathBuf> = None;
    let mut names: Vec<(String, String)> = Vec::new();
    let mut show = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} needs a value"));
        let r: Result<(), String> = match a.as_str() {
            "--window" => value().map(|v| window = Some(v)),
            "--app" => value().map(|v| app = Some(v)),
            "--every" => value().and_then(|v| v.parse().map(|v| every = v).map_err(|_| "--every: a number".into())),
            "--count" => value().and_then(|v| v.parse().map(|v| count = v).map_err(|_| "--count: a number".into())),
            "--out" => value().map(|v| out = Some(v.into())),
            "--names" => value().map(|v| {
                names = v
                    .split(';')
                    .filter_map(|p| p.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())))
                    .filter(|(a, _)| !a.is_empty())
                    .collect()
            }),
            "--show-names" => {
                show = true;
                Ok(())
            }
            "--help" | "-h" => {
                println!("{DUMP_USAGE}");
                return 0;
            }
            other => Err(format!("unknown option {other}")),
        };
        if let Err(e) = r {
            eprintln!("{e}\n\n{DUMP_USAGE}");
            return 2;
        }
    }
    let Some((hwnd, title, guessed)) = find_window(window.as_deref()) else {
        eprintln!("No call window found. Start the call (or give --window with part of its title) and try again.");
        return 1;
    };
    let app = app.unwrap_or(guessed);
    let rules = Rules::new(profile(&app));
    println!("Reading the window \"{title}\" ({app} rules), {count} times, every {every} ms. Read only.");
    let started = Instant::now();
    let mut dump = Dump {
        version: 1,
        app: app.clone(),
        source: format!("yap.exe call-dump {}", env!("CARGO_PKG_VERSION")),
        snapshots: Vec::new(),
    };
    for i in 0..count {
        let t0 = Instant::now();
        match win::snapshot(hwnd) {
            Ok(Some(root)) => {
                let found = read(&root, &rules);
                let nodes = root.count();
                let who = if show { format!("{:?}", found.speaking) } else { format!("{} talking", found.speaking.len()) };
                println!(
                    "  {:>3}: {nodes} elements, {} ms — {who}{}",
                    i + 1,
                    t0.elapsed().as_millis(),
                    if found.self_speaking { ", you talking" } else { "" }
                );
                dump.snapshots.push(Snapshot {
                    at_ms: started.elapsed().as_millis() as u64,
                    read_ms: t0.elapsed().as_millis() as u64,
                    root,
                    expect: None,
                    found: Some(found),
                });
            }
            Ok(None) => println!("  {:>3}: minimised — nothing to read", i + 1),
            Err(e) => {
                eprintln!("  {:>3}: couldn't read the window: {e}", i + 1);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(every).saturating_sub(t0.elapsed()));
    }
    let mut text = serde_json::to_string_pretty(&dump).unwrap_or_default();
    for (real, stand_in) in &names {
        if let Ok(re) = regex::RegexBuilder::new(&regex::escape(real)).case_insensitive(true).build() {
            text = re.replace_all(&text, stand_in.as_str()).into_owned();
        }
    }
    let path = out.unwrap_or_else(|| format!("call-dump-{app}-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")).into());
    match std::fs::write(&path, text) {
        Ok(()) => {
            println!("\nWritten: {}\nIt holds whatever the window showed (names, chat). Check it before sharing.", path.display());
            0
        }
        Err(e) => {
            eprintln!("Couldn't write {}: {e}", path.display());
            1
        }
    }
}

/// The call window for the dump: titled with `wanted`, else a call app's
/// or a browser showing a meeting. (hwnd, title, app id).
fn find_window(wanted: Option<&str>) -> Option<(isize, String, String)> {
    let (call_exes, _) = crate::calls::meeting_detect::call_audio();
    let mut exes: Vec<String> = call_exes.into_iter().filter(|n| n.ends_with(".exe")).collect();
    exes.extend(crate::calls::meeting_detect::browser_exes().iter().map(|b| b.to_string()));
    let windows = crate::meetings::notepad::windows_of(&exes);
    const APPS: [&str; 8] = ["teams", "zoom", "meet", "webex", "slack", "goto", "whereby", "jitsi"];
    let app_of = |exe: &str, title: &str| -> Option<String> {
        APPS.iter()
            .find(|a| crate::calls::meeting_detect::title_shows(a, title) || crate::calls::meeting_detect::exe_is(a, exe))
            .map(|a| a.to_string())
    };
    let pick = windows
        .iter()
        .filter(|c| !c.minimized && !c.tool && !c.owned && !c.cloaked && !c.title.trim().is_empty())
        .filter_map(|c| {
            let app = app_of(&c.exe, &c.title);
            match wanted {
                Some(w) => c.title.to_lowercase().contains(&w.to_lowercase()).then(|| (c, app.unwrap_or_else(|| "generic".into()))),
                None => app.map(|a| (c, a)),
            }
        })
        .max_by_key(|(c, _)| i64::from(c.rect.w) * i64::from(c.rect.h))?;
    Some((pick.0.hwnd, pick.0.title.clone(), pick.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Dump {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/speakers/call_ui/fixtures").join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn built_in(app: &str) -> Rules {
        let file: ProfileFile = serde_json::from_str(BUILT_IN).expect("profiles.json");
        Rules::new(file.apps.get(app).or_else(|| file.apps.get("generic")).cloned().expect("a profile"))
    }

    /// Every fixture's reads give what it expects, with its app's rules.
    #[test]
    fn fixtures_read_as_expected() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/speakers/call_ui/fixtures");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("fixtures") {
            let path = entry.expect("entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let dump = fixture(&name);
            let rules = built_in(&dump.app);
            for (i, snap) in dump.snapshots.iter().enumerate() {
                let Some(expect) = &snap.expect else { continue };
                let got = read(&snap.root, &rules);
                assert_eq!(got.speaking, expect.speaking, "{name} #{i}");
                assert_eq!(got.self_speaking, expect.self_speaking, "{name} #{i}");
                if expect.self_name.is_some() {
                    assert_eq!(got.self_name, expect.self_name, "{name} #{i}");
                }
                checked += 1;
            }
        }
        assert!(checked >= 9, "{checked} reads checked");
    }

    #[test]
    fn names_come_clean() {
        let r = built_in("teams");
        let c = |s: &str| clean_name(s, &r);
        assert_eq!(c("Priya Shah (Guest)"), Some("Priya Shah".into()));
        assert_eq!(c("Shah, Priya"), Some("Priya Shah".into()));
        assert_eq!(c("Sam Patel, Muted"), Some("Sam Patel".into()));
        assert_eq!(c("Imogen Hart (You)"), Some("Imogen Hart".into()));
        assert_eq!(c("Mute microphone"), None);
        assert_eq!(c("More options"), None);
        assert_eq!(c(""), None);
        assert_eq!(c("This is a very long sentence that is not a name at all"), None);
    }

    fn span(from: u64, to: u64, who: &[&str]) -> Span {
        Span { from_ms: from, to_ms: to, speaking: who.iter().map(|s| s.to_string()).collect(), me: false }
    }

    fn turn(voice: &str, s: u64, e: u64) -> VoiceTurn {
        VoiceTurn { voice: voice.into(), start_ms: s, end_ms: e }
    }

    #[test]
    fn the_app_names_the_voice_it_showed_while_it_spoke() {
        let spans = [
            span(0, 4_000, &["Priya Shah"]),
            span(4_000, 4_800, &[]),
            span(4_800, 9_000, &["Sam Patel"]),
            // Two at once (cross-talk): says nothing.
            span(9_000, 10_000, &["Priya Shah", "Sam Patel"]),
            span(10_000, 14_000, &["Priya Shah"]),
        ];
        let turns = [turn("them-1", 0, 3_800), turn("them-2", 4_600, 8_700), turn("them-1", 10_000, 13_500)];
        let votes = align(&spans, &turns, None);
        assert_eq!(votes.len(), 2);
        assert_eq!((votes[0].voice.as_str(), votes[0].name.as_str()), ("them-1", "Priya Shah"));
        assert!(votes[0].share > 0.8 && votes[0].reads >= 3, "{votes:?}");
        assert_eq!((votes[1].voice.as_str(), votes[1].name.as_str()), ("them-2", "Sam Patel"));
        // Your own name never names a call voice; too little says nothing.
        assert!(align(&spans, &turns, Some("priya shah")).iter().all(|v| v.voice != "them-1"));
        assert!(align(&[span(0, 1_000, &["Priya Shah"])], &[turn("them-1", 0, 900)], None).is_empty());
        // A voice the app showed as two people in turn: no clear name.
        let mixed = [span(0, 3_000, &["Priya Shah"]), span(3_000, 6_000, &["Sam Patel"])];
        assert!(align(&mixed, &[turn("them-1", 0, 6_000)], None).is_empty());
    }

    /// The real UI Automation read, on a window of this process far off
    /// screen: a panel with a label saying who's talking (as a call app's
    /// might).
    #[cfg(windows)]
    #[test]
    #[ignore = "makes a window (off screen, never focused): cargo test --lib -- --ignored reads_a_real_window"]
    fn reads_a_real_window() {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, PostThreadMessageW, MSG, WM_QUIT, WS_CHILD,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
        };
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || unsafe {
            let (panel_class, label_class) = (wide("#32770"), wide("STATIC"));
            let (title, text) = (wide("Weekly sync"), wide("Priya Shah, speaking"));
            // A dialog-class panel (its child windows are its elements),
            // shown (UI Automation leaves a hidden window's children out)
            // but far off screen and never activated.
            let panel = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(panel_class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                -20_000,
                -20_000,
                400,
                200,
                None,
                None,
                None,
                None,
            )
            .expect("panel");
            let _label = CreateWindowExW(
                Default::default(),
                PCWSTR(label_class.as_ptr()),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                300,
                20,
                Some(panel),
                None,
                None,
                None,
            )
            .expect("label");
            tx.send((panel.0 as isize, GetCurrentThreadId())).unwrap();
            // UI Automation asks the window's thread: it pumps messages.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
            let _ = DestroyWindow(panel);
        });
        let (hwnd, thread) = rx.recv().unwrap();
        let t0 = Instant::now();
        let root = win::snapshot(hwnd).expect("read").expect("not minimised");
        let took = t0.elapsed();
        let r = read(&root, &built_in("teams"));
        unsafe {
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        t.join().unwrap();
        assert!(root.count() >= 2, "{root:?}");
        assert_eq!(r.speaking, ["Priya Shah"], "{root:#?}");
        assert!(took < Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn readings_run_together_into_spans() {
        let stop = Arc::new(AtomicBool::new(false));
        watches().push(Watch { note_id: 991, session: None, base: 0, spans: Vec::new(), self_name: None, stop });
        let priya = Reading { speaking: vec!["Priya Shah".into()], ..Default::default() };
        record(991, 0, &priya);
        record(991, 800, &priya);
        record(991, 1_600, &Reading::default());
        let spans = watches().iter().find(|w| w.note_id == 991).unwrap().spans.clone();
        assert_eq!(spans, [span(0, 1_600, &["Priya Shah"]), span(1_600, 2_400, &[])]);
        finish(991);
        assert!(!watches().iter().any(|w| w.note_id == 991));
    }
}
