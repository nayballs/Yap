//! Who said what in a meeting: the note's **speaker map**, the names it puts
//! on transcript lines (in the notepad, the Notes view, copies, exports, the
//! MCP server and every AI prompt), and the line timings the voice
//! separation splits lines at.
//!
//! **Voices.** A transcript line has a side (`source`: the mic "you", the
//! call "them") and a voice (`TranscriptSegment::speaker`): "you", "them"
//! (everyone on the call, not told apart), later "them-1", "them-2"… from
//! the voice separation, or "p-1", "p-2"… for a person the user named on
//! some lines ("Rename just this line"). A line nobody relabelled has no
//! `speaker` of its own: its side is its voice (so every note recorded
//! before voices existed reads as You/Them, unchanged).
//!
//! **The map** (`Note::speakers`) names voices: `{id → {name, email, source,
//! locked}}`, plus `mergedInto` (two voices that are one person: the lines
//! of one read as the other's) and `not` (names the user said a voice isn't).
//! Where a name comes from, most trusted first: the **user** > the **call
//! app** (`platform`) > a **voice** match > the **calendar** or an **AI**
//! guess. Ported by behaviour from OpenWhispr's `speakerAssignmentPolicy.js`:
//! the user's choice is locked, and no automatic source ever relabels it
//! (nor a line the user assigned: `speaker_locked`). A source never replaces
//! a more trusted one, and "Not Priya" keeps Priya off that voice for good.
//! A calendar or AI name is a guess: shown as "Priya?" with "Not Priya",
//! told to the prompts as a guess, never locked.
//!
//! **Step 1, calendar names for 1:1 calls** ([`calendar_guess`]): a meeting
//! note tied to a calendar event with exactly one other invitee (not you,
//! not a room) names "Them" after that person, as a guess. Someone joining
//! later makes it wrong, but no worse than "Them": it stays visibly a guess.
//!
//! **Line timing**: a recorded line carries its place on the recording's
//! clock (`start_ms`/`end_ms`) and, when the speech model gives them, its
//! words' times ([`word_times`] lines them up with the corrected text).
//! [`split_line`] cuts a line where the speaker changes, at its words, else
//! in proportion to its text.
//!
//! **The voice separation's labels** ([`labels`], beta): `diarize.rs` says
//! who spoke when on the call side; its voices become "them-1", "them-2"…
//! ("Speaker 1"…) on the "Them" lines as the labels settle, and in full
//! when the recording stops, before the summary is written.

pub mod call_ui;
pub mod labels;
pub mod naming;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::notes::{Note, TranscriptSegment};
use crate::stt::TimedText;

/// The voice of the person using Yap (the mic).
pub const YOU: &str = "you";
/// Everyone on the call, not told apart (the call audio).
pub const THEM: &str = "them";
/// A person the user named on some lines: `p-1`, `p-2`…
const PERSON_PREFIX: &str = "p-";
/// A voice the separation told apart: `them-1`, `them-2`…
const SEPARATED_PREFIX: &str = "them-";
/// The longest name kept.
const NAME_MAX_CHARS: usize = 60;

/// A note's speaker map: voice id → what it's called.
pub type Speakers = BTreeMap<String, Speaker>;

/// Where a voice's name came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameSource {
    /// The person using Yap named it (locked).
    User,
    /// The call app said who was talking (Teams / Zoom's active speaker).
    Platform,
    /// A known voice matched.
    Voice,
    /// The calendar invite (a 1:1 call's other invitee): a guess.
    Calendar,
    /// The AI's guess from what was said: a guess.
    Ai,
}

impl NameSource {
    /// How far a name from here is trusted: a source never replaces a more
    /// trusted one.
    fn rank(self) -> u8 {
        match self {
            NameSource::User => 4,
            NameSource::Platform => 3,
            NameSource::Voice => 2,
            NameSource::Calendar | NameSource::Ai => 1,
        }
    }

    /// A guess: shown as "Priya?", with "Not Priya".
    pub fn is_guess(self) -> bool {
        matches!(self, NameSource::Calendar | NameSource::Ai)
    }
}

/// What one voice is called.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    /// Empty: not named ("Them", "Speaker 2").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    /// Where the name came from (`None` while unnamed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<NameSource>,
    /// The user named it: no automatic source may rename it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// Merged into this voice ("Merge with…"): this one's lines read as that
    /// one's, now and when later lines arrive with this id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub merged_into: String,
    /// Names the user said this voice isn't ("Not Priya"): never guessed again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not: Vec<String>,
    /// Who Yap thinks this voice is, for the person to confirm ("Speaker 2
    /// is Priya?": [`naming`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Suggestion>,
}

/// A name Yap suggests for a voice ([`naming`]): the notepad and the Notes
/// view ask "Speaker 2 is Priya?" — Yes / Someone else / Not now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The person ("" when `me`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    /// The voice is the person using Yap (theirs reached the call side).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub me: bool,
    /// How sure, 0–1.
    pub confidence: f32,
    /// The best reason, for the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<Why>,
    /// "Not now": not asked again in this meeting (until the name changes).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub later: bool,
}

/// Why Yap suggests a name: what the card says under the question.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Why {
    /// "introduced" (the voice said it), "asked" (`by` asked them, this
    /// voice answered), "thanked" (`by` thanked them after this voice
    /// spoke), "invite" (the only other invitee), "app" (the call app
    /// showed them talking), "model" (the AI matched it), "echo" (it
    /// repeats what you said).
    pub kind: String,
    /// The voice whose words show it ("asked", "thanked").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    /// Those words, short.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quote: String,
}

impl Speaker {
    fn named(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// Named by a guess (calendar, AI) the user hasn't confirmed.
    pub fn is_guess(&self) -> bool {
        self.named() && !self.locked && self.source.is_some_and(NameSource::is_guess)
    }

    fn rank(&self) -> u8 {
        if !self.named() {
            0
        } else if self.locked {
            NameSource::User.rank()
        } else {
            self.source.map_or(1, NameSource::rank)
        }
    }

    fn refuses(&self, name: &str) -> bool {
        self.not.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

// ---- reading: which voice a line is, and what to call it ----

/// The voice `id` stands for, after any merges.
pub fn resolve<'a>(map: &'a Speakers, id: &'a str) -> &'a str {
    let mut id = id;
    for _ in 0..16 {
        match map.get(id).map(|s| s.merged_into.as_str()) {
            Some(into) if !into.is_empty() && into != id => id = into,
            _ => break,
        }
    }
    id
}

/// The voice of line `seg`, after any merges.
pub fn voice_of<'a>(map: &'a Speakers, seg: &'a TranscriptSegment) -> &'a str {
    resolve(map, seg.speaker_id())
}

/// What an unnamed voice is called: "You", "Them", "Speaker 2".
pub fn default_name(id: &str) -> String {
    match id {
        YOU => "You".to_string(),
        THEM => "Them".to_string(),
        _ => match id.strip_prefix(SEPARATED_PREFIX) {
            Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => format!("Speaker {n}"),
            _ => "Someone".to_string(),
        },
    }
}

/// What voice `id` (resolved) is called, and whether that's only a guess.
/// "You" is always You.
pub fn name_of(map: &Speakers, id: &str) -> (String, bool) {
    if id == YOU {
        return ("You".to_string(), false);
    }
    match map.get(id).filter(|s| s.named()) {
        Some(s) => (s.name.trim().to_string(), s.is_guess()),
        None => (default_name(id), false),
    }
}

/// How the transcript shows voice `id` (resolved): its name, "Priya?" for a
/// guess, else "Them" / "Speaker 2".
pub fn display(map: &Speakers, id: &str) -> String {
    let (name, guess) = name_of(map, id);
    if guess {
        format!("{name}?")
    } else {
        name
    }
}

/// How the transcript shows line `seg`'s speaker ([`display`]).
pub fn display_line(map: &Speakers, seg: &TranscriptSegment) -> String {
    display(map, voice_of(map, seg))
}

/// How the AI prompts label line `seg`: the speaker's name (a guess as the
/// name: [`speakers_line`] tells the model it's a guess), else "You",
/// "Them", "Speaker 2".
pub fn prompt_label(map: &Speakers, seg: &TranscriptSegment) -> String {
    name_of(map, voice_of(map, seg)).0
}

/// The voices of `note`, resolved: those its talk lines are in (in order of
/// first appearance), then any other voice the map knows — but not "Them"
/// once the voice separation has told every call line apart (its name, a
/// 1:1 guess, went to a voice: [`labels`]).
pub fn voices(note: &Note) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for seg in note.transcript.iter().filter(|s| s.is_talk()) {
        let id = voice_of(&note.speakers, seg);
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    let told_apart = !out.iter().any(|o| o == THEM) && out.iter().any(|o| o.starts_with(SEPARATED_PREFIX));
    for id in note.speakers.keys() {
        let id = resolve(&note.speakers, id);
        if !out.iter().any(|o| o == id) && !(told_apart && id == THEM) {
            out.push(id.to_string());
        }
    }
    out
}

/// Everyone the note knows by name: its attendees, then the speaker map's
/// names (a guess included: the invite named them). What the owner checks
/// accept (`meeting_summary::resolve_owner`): a name here is a known person.
pub fn known_people(note: &Note) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let names = note
        .participants
        .iter()
        .map(|p| p.trim().to_string())
        .chain(note.speakers.values().filter(|s| s.named()).map(|s| s.name.trim().to_string()));
    for name in names {
        if !name.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
            out.push(name);
        }
    }
    out
}

/// The "Speakers: …" line for the AI prompts, when the transcript names
/// anyone or tells voices apart (`None` while it's just You and Them):
/// who the other labels are, a guess said to be one.
pub fn speakers_line(note: &Note) -> Option<String> {
    let others: Vec<String> = voices(note)
        .into_iter()
        .filter(|id| id != YOU)
        .filter(|id| {
            note.transcript
                .iter()
                .any(|s| s.is_talk() && voice_of(&note.speakers, s) == id)
        })
        .collect();
    let telling = others.iter().any(|id| id != THEM || note.speakers.get(id).is_some_and(Speaker::named));
    if !telling {
        return None;
    }
    // A name once (a 1:1 guess on a voice and on the lines nobody told
    // apart is one person).
    let mut said: Vec<String> = Vec::new();
    let parts: Vec<String> = others
        .iter()
        .filter(|id| {
            let name = name_of(&note.speakers, id).0;
            let fresh = !said.iter().any(|s| s.eq_ignore_ascii_case(&name));
            said.push(name);
            fresh
        })
        .map(|id| {
            let s = note.speakers.get(id.as_str());
            let (name, guess) = name_of(&note.speakers, id);
            match s {
                _ if guess && s.and_then(|s| s.source) == Some(NameSource::Calendar) => {
                    format!("{name} (a guess: the only other person invited)")
                }
                _ if guess => format!("{name} (a guess)"),
                Some(s) if s.named() => name,
                _ if id == THEM => "Them (anyone else on the call)".to_string(),
                _ => format!("{name} (not named yet)"),
            }
        })
        .collect();
    Some(format!("Speakers besides You: {}", parts.join("; ")))
}

// ---- edits: the user's, and automatic names ----

/// A name as typed: one line, spaces collapsed, no trailing "?", at most
/// [`NAME_MAX_CHARS`] characters.
fn clean_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.trim_end_matches('?').trim();
    if name.is_empty() {
        return Err("A name, please".to_string());
    }
    Ok(name.chars().take(NAME_MAX_CHARS).collect::<String>().trim().to_string())
}

/// The voice called `name` (any case): by its name, or by what it's called
/// unnamed ("You", "Them", "Speaker 2").
fn voice_named(note: &Note, name: &str) -> Option<String> {
    if name.eq_ignore_ascii_case("you") {
        return Some(YOU.to_string());
    }
    voices(note).into_iter().find(|id| name_of(&note.speakers, id).0.eq_ignore_ascii_case(name))
}

/// A new id for a person named on some lines.
fn new_person(map: &Speakers) -> String {
    let n = map
        .keys()
        .filter_map(|k| k.strip_prefix(PERSON_PREFIX)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{PERSON_PREFIX}{}", n + 1)
}

/// Name voice `id` as the user typed it (locked).
fn set_user_name(map: &mut Speakers, id: &str, name: &str) -> bool {
    let entry = map.entry(id.to_string()).or_default();
    if entry.name == name && entry.locked && entry.source == Some(NameSource::User) {
        return false;
    }
    if !entry.name.eq_ignore_ascii_case(name) {
        entry.email.clear();
    }
    entry.name = name.to_string();
    entry.source = Some(NameSource::User);
    entry.locked = true;
    entry.not.retain(|n| !n.eq_ignore_ascii_case(name));
    entry.suggestion = None;
    true
}

/// Make `from` a part of `into` (both resolved, different): the merged voice
/// keeps the more trusted name, and the names either was refused.
fn merge_into(map: &mut Speakers, from: &str, into: &str) {
    let gone = map.remove(from).unwrap_or_default();
    let target = map.entry(into.to_string()).or_default();
    if gone.named() && gone.rank() > target.rank() {
        target.name = gone.name;
        target.email = gone.email;
        target.source = gone.source;
        target.locked = gone.locked;
    }
    for n in gone.not {
        if !target.refuses(&n) {
            target.not.push(n);
        }
    }
    // Who either was is asked again from the merged voice's evidence.
    target.suggestion = None;
    map.insert(
        from.to_string(),
        Speaker {
            merged_into: into.to_string(),
            ..Speaker::default()
        },
    );
}

/// **Rename everywhere**: voice `id` is `name`, on every line of it (the
/// user's choice: locked). A name another voice already goes by means
/// they're one person: the two merge. "You" is always you.
pub fn rename(note: &mut Note, id: &str, name: &str) -> Result<bool, String> {
    let id = resolve(&note.speakers, id).to_string();
    if id == YOU {
        return Err("That's you. To say someone else said a line, rename just that line.".to_string());
    }
    let name = clean_name(name)?;
    if let Some(other) = voice_named(note, &name).filter(|o| *o != id) {
        if other == YOU {
            return Err("That's you. To say you said a line, rename just that line.".to_string());
        }
        merge_into(&mut note.speakers, &id, &other);
        set_user_name(&mut note.speakers, &other, &name);
        return Ok(true);
    }
    Ok(set_user_name(&mut note.speakers, &id, &name))
}

/// **Rename just this line** (or these lines): `lines` (transcript indices)
/// were said by `name` — a voice already called that, else a new person.
/// The lines are the user's: no automatic relabel moves them again.
pub fn rename_lines(note: &mut Note, lines: &[usize], name: &str) -> Result<bool, String> {
    let name = clean_name(name)?;
    if lines.is_empty() || lines.iter().any(|&i| !note.transcript.get(i).is_some_and(|s| !s.dictated)) {
        return Err("That line isn't in this meeting's transcript any more.".to_string());
    }
    let mut changed = false;
    let target = match voice_named(note, &name) {
        Some(id) => {
            // Naming a line after a guess ("Priya?") confirms the guess.
            if note.speakers.get(&id).is_some_and(|s| s.is_guess() && s.name.trim().eq_ignore_ascii_case(&name)) {
                changed |= set_user_name(&mut note.speakers, &id, &name);
            }
            id
        }
        None => {
            let id = new_person(&note.speakers);
            set_user_name(&mut note.speakers, &id, &name);
            changed = true;
            id
        }
    };
    for &i in lines {
        let seg = &mut note.transcript[i];
        let speaker = if target == seg.source { String::new() } else { target.clone() };
        if seg.speaker != speaker || !seg.speaker_locked {
            seg.speaker = speaker;
            seg.speaker_locked = true;
            changed = true;
        }
    }
    Ok(changed)
}

/// **Merge with…**: voice `from` and voice `into` are one person; `from`'s
/// lines (and any that arrive later) read as `into`'s. The merged voice
/// keeps the more trusted name. "You" doesn't merge.
pub fn merge(note: &mut Note, from: &str, into: &str) -> Result<bool, String> {
    let from = resolve(&note.speakers, from).to_string();
    let into = resolve(&note.speakers, into).to_string();
    if from == into {
        return Ok(false);
    }
    if from == YOU || into == YOU {
        return Err("You can't be merged with someone else. To say you said a line, rename just that line.".to_string());
    }
    merge_into(&mut note.speakers, &from, &into);
    Ok(true)
}

/// **Not Priya**: the guessed name on voice `id` is wrong. The voice goes
/// back to "Them" / "Speaker 2", and that name is never guessed for it
/// again. A name the user gave is renamed instead.
pub fn not(note: &mut Note, id: &str) -> Result<bool, String> {
    let id = resolve(&note.speakers, id).to_string();
    let Some(entry) = note.speakers.get_mut(&id).filter(|s| s.named()) else {
        return Ok(false);
    };
    if entry.locked {
        return Err("You named them yourself: rename them instead.".to_string());
    }
    let name = std::mem::take(&mut entry.name).trim().to_string();
    entry.email.clear();
    entry.source = None;
    if entry.suggestion.as_ref().is_some_and(|s| s.name.eq_ignore_ascii_case(&name)) {
        entry.suggestion = None;
    }
    if !entry.refuses(&name) {
        entry.not.push(name);
    }
    Ok(true)
}

/// An automatic name for voice `id` (the call app, a voice match, the
/// calendar, the AI): taken unless the user named the voice (locked), said
/// it isn't this person, or a more trusted source named it already.
/// `source` [`NameSource::User`] is [`rename`]'s, never this. Returns
/// whether the name changed.
pub fn suggest(note: &mut Note, id: &str, name: &str, email: &str, source: NameSource) -> bool {
    if source == NameSource::User {
        return false;
    }
    let Ok(name) = clean_name(name) else { return false };
    let id = resolve(&note.speakers, id).to_string();
    if id == YOU {
        return false;
    }
    let entry = note.speakers.entry(id).or_default();
    if entry.locked || entry.refuses(&name) || (entry.named() && entry.rank() > source.rank()) {
        return false;
    }
    let email = email.trim();
    if entry.name == name && entry.source == Some(source) && entry.email == email {
        return false;
    }
    entry.name = name;
    entry.email = email.to_string();
    entry.source = Some(source);
    entry.locked = false;
    true
}

/// **Calendar names for 1:1 calls**: a meeting whose invite has exactly one
/// other invitee (`invitees`: everyone invited but you, rooms and declines
/// left out; name, email) names "Them" after them, as a guess. Returns
/// whether it did.
pub fn calendar_guess(note: &mut Note, invitees: &[(String, String)]) -> bool {
    match invitees {
        [(name, email)] => {
            let name = if name.trim().is_empty() { email } else { name };
            suggest(note, THEM, name, email, NameSource::Calendar)
        }
        _ => false,
    }
}

// ---- line timing: where words are, and splitting a line between voices ----

/// Letters and digits of a word, lowercased: what [`word_times`] compares.
fn norm(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Spread `words` over `from..to` (ms) by their length.
fn spread(words: &[&str], from: u64, to: u64) -> Vec<(u64, u64)> {
    let to = to.max(from);
    let weights: Vec<u64> = words.iter().map(|w| w.chars().count() as u64 + 1).collect();
    let total: u64 = weights.iter().sum::<u64>().max(1);
    let mut at = 0u64;
    weights
        .iter()
        .map(|w| {
            let s = from + (to - from) * at / total;
            at += w;
            (s, from + (to - from) * at / total)
        })
        .collect()
}

/// Times (ms from the audio's start) for each word of `text` (split at
/// whitespace), from the speech model's `timed` words or phrases. `text` is
/// the line as kept, after the correction dictionary, so its words are
/// lined up with the model's (longest common run of matching words); a word
/// the dictionary changed or joined ("chat G P T" → "ChatGPT") takes the
/// time of what it replaced, in proportion. Empty when there are no timings.
pub fn word_times(text: &str, timed: &[TimedText]) -> Vec<(u64, u64)> {
    let ms = |s: f32| (s.max(0.0) * 1000.0).round() as u64;
    // The model's words, a phrase's spread over its time.
    let mut raw: Vec<(String, u64, u64)> = Vec::new();
    for piece in timed {
        let words: Vec<&str> = piece.text.split_whitespace().collect();
        let (from, to) = (ms(piece.start), ms(piece.end.max(piece.start)));
        for (w, (s, e)) in words.iter().zip(spread(&words, from, to)) {
            raw.push((norm(w), s, e));
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if raw.is_empty() || words.is_empty() {
        return Vec::new();
    }
    let ours: Vec<String> = words.iter().map(|w| norm(w)).collect();

    // Longest common subsequence of matching words.
    let (n, m) = (ours.len(), raw.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if !ours[i].is_empty() && ours[i] == raw[j].0 {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut matched: Vec<Option<usize>> = vec![None; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if !ours[i].is_empty() && ours[i] == raw[j].0 {
            matched[i] = Some(j);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let mut out: Vec<(u64, u64)> = vec![(0, 0); n];
    let mut i = 0;
    while i < n {
        if let Some(j) = matched[i] {
            out[i] = (raw[j].1, raw[j].2);
            i += 1;
            continue;
        }
        // A run of our words with no match: the time between the matches
        // around it (the model's unmatched words there, else the gap).
        let end = (i..n).find(|&k| matched[k].is_some()).unwrap_or(n);
        let before = (0..i).rev().find_map(|k| matched[k]);
        let after = matched.get(end).copied().flatten();
        let first_free = before.map_or(0, |b| b + 1);
        let last_free = after.unwrap_or(m); // exclusive
        let (from, to) = if first_free < last_free {
            (raw[first_free].1, raw[last_free - 1].2)
        } else {
            (before.map_or(raw[0].1, |b| raw[b].2), after.map_or(raw[m - 1].2, |a| raw[a].1))
        };
        for (k, t) in (i..end).zip(spread(&words[i..end], from, to)) {
            out[k] = t;
        }
        i = end;
    }
    // Never backwards.
    let mut last = 0;
    for t in out.iter_mut() {
        t.0 = t.0.max(last);
        t.1 = t.1.max(t.0);
        last = t.0;
    }
    out
}

/// A line's place on the recording's clock, from where its chunk of audio
/// starts (`chunk_ms`), where its speech starts and its audio ends, and the
/// times of its words in the chunk ([`word_times`]): (start, end, the words
/// as offsets from start). Without word times, onset to end.
pub fn line_timing(chunk_ms: u64, onset_ms: u64, end_ms: u64, words: &[(u64, u64)]) -> (u64, u64, Vec<[u32; 2]>) {
    match (words.first(), words.last()) {
        (Some(first), Some(last)) => {
            let start = chunk_ms + first.0;
            let offsets = words
                .iter()
                .map(|(s, e)| [(chunk_ms + s - start) as u32, (chunk_ms + e - start) as u32])
                .collect();
            (start, (chunk_ms + last.1).max(start), offsets)
        }
        _ => (onset_ms, end_ms.max(onset_ms), Vec::new()),
    }
}

/// One speaker's stretch on the recording's clock, as the voice separation
/// reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start_ms: u64,
    pub end_ms: u64,
    /// The voice: "them-1", "them-2"…
    pub speaker: String,
}

/// A single word shorter than this between two of another speaker's is
/// taken to be theirs (a blip in the separation, not a turn).
const BLIP_MS: u64 = 400;
/// A word this far from every turn takes its neighbours' speaker.
const NEAR_MS: u64 = 1_500;

/// Line `seg` split where its speaker changes, by `turns` (sorted or not;
/// overlapping is fine): each word goes to the turn it overlaps most (else
/// the nearest within [`NEAR_MS`], else its neighbours'), one-word blips are
/// smoothed away, and each run of one speaker becomes a line of its own,
/// with its words and its place. Words come from the line's word times;
/// without them, the line's text is spread over its place in proportion.
/// Returned unchanged: a line the user assigned (`speaker_locked`) or
/// edited (`edited`), a marker or echo, a line with no place, or no
/// turns. One run = the line
/// relabelled. The voice separation calls it as lines get labels
/// ([`labels`]).
pub fn split_line(seg: &TranscriptSegment, turns: &[Turn]) -> Vec<TranscriptSegment> {
    let (Some(start), Some(end)) = (seg.start_ms, seg.end_ms) else {
        return vec![seg.clone()];
    };
    let words: Vec<&str> = seg.text.split_whitespace().collect();
    // (A line the person edited is theirs as written: never split.)
    if seg.speaker_locked || seg.edited || !seg.is_talk() || turns.is_empty() || words.is_empty() {
        return vec![seg.clone()];
    }
    let times: Vec<(u64, u64)> = if seg.words.len() == words.len() {
        seg.words.iter().map(|[s, e]| (start + *s as u64, start + *e as u64)).collect()
    } else {
        spread(&words, start, end)
    };

    // Who said each word.
    let mut who: Vec<Option<&str>> = times
        .iter()
        .map(|&(s, e)| {
            let overlap = |t: &Turn| e.min(t.end_ms).saturating_sub(s.max(t.start_ms));
            let mid = (s + e) / 2;
            let distance = |t: &Turn| {
                if mid < t.start_ms {
                    t.start_ms - mid
                } else {
                    mid.saturating_sub(t.end_ms)
                }
            };
            let best = turns.iter().max_by_key(|t| overlap(t)).filter(|t| overlap(t) > 0);
            let best = best.or_else(|| {
                turns
                    .iter()
                    .filter(|t| (t.start_ms..=t.end_ms).contains(&mid))
                    .max_by_key(|t| t.end_ms - t.start_ms)
            });
            best.or_else(|| turns.iter().min_by_key(|t| distance(t)).filter(|t| distance(t) <= NEAR_MS))
                .map(|t| t.speaker.as_str())
        })
        .collect();
    // Words near no turn take the speaker before them, else after, else the
    // line's own.
    for k in 0..who.len() {
        if who[k].is_none() {
            who[k] = who[..k].iter().rev().flatten().next().copied();
        }
    }
    for k in (0..who.len()).rev() {
        if who[k].is_none() {
            who[k] = who[k + 1..].iter().flatten().next().copied();
        }
    }
    let own = seg.speaker_id().to_string();
    let who: Vec<&str> = who.into_iter().map(|w| w.unwrap_or(&own)).collect();

    // Runs of one speaker; a one-word blip joins the run before it (or the
    // one after, at the start).
    let mut runs: Vec<(usize, usize, &str)> = Vec::new(); // [from, to)
    for (k, &w) in who.iter().enumerate() {
        match runs.last_mut() {
            Some(r) if r.2 == w => r.1 = k + 1,
            _ => runs.push((k, k + 1, w)),
        }
    }
    let mut k = 0;
    while runs.len() > 1 && k < runs.len() {
        let (from, to, _) = runs[k];
        let blip = to - from == 1 && times[from].1.saturating_sub(times[from].0) < BLIP_MS;
        if blip {
            if k > 0 {
                runs[k - 1].1 = to;
            } else {
                runs[1].0 = from;
            }
            runs.remove(k);
            // The runs either side may now be one speaker's.
            if k > 0 && k < runs.len() && runs[k - 1].2 == runs[k].2 {
                runs[k - 1].1 = runs[k].1;
                runs.remove(k);
            }
            k = k.saturating_sub(1);
        } else {
            k += 1;
        }
    }

    runs.into_iter()
        .map(|(from, to, speaker)| {
            let (s, e) = (times[from].0, times[to - 1].1.max(times[from].0));
            let mut part = seg.clone();
            part.text = words[from..to].join(" ");
            part.start_ms = Some(s);
            part.end_ms = Some(e);
            part.words = if seg.words.len() == words.len() {
                times[from..to].iter().map(|&(ws, we)| [(ws - s) as u32, (we - s) as u32]).collect()
            } else {
                Vec::new()
            };
            part.ts = seg.ts + s.saturating_sub(start) / 1000;
            part.speaker = if speaker == seg.source { String::new() } else { speaker.to_string() };
            part.speaker_locked = false;
            part
        })
        .collect()
}

// ---- commands: the menu on a speaker's name, and the name cards ----

/// Every line's voice, lock and echo flag, for the windows to follow an edit.
pub(crate) fn line_speakers(note: &Note) -> Vec<Value> {
    note.transcript
        .iter()
        .map(|s| json!([s.speaker_id(), s.speaker_locked, s.echo]))
        .collect()
}

/// After voice `id` was named (it was `before`, "Speaker 2"): a summary
/// already written says the name too ([`naming::rename_in_summary`]).
fn relabel_summary(note: &mut Note, id: &str, before: &str) -> bool {
    let after = name_of(&note.speakers, resolve(&note.speakers, id)).0;
    naming::rename_in_summary(note, before, &after)
}

/// Apply a speaker edit to note `note_id`, suggest names again from what's
/// left, tell every window (the notepad, the Notes view: `yap-note-changed`
/// with the map and each line's voice, and the summary when it changed),
/// and hand back the note.
fn edit(
    app: &AppHandle,
    note_id: u64,
    origin: Option<String>,
    f: impl FnOnce(&mut Note) -> Result<bool, String>,
) -> Result<Note, String> {
    let sources = naming::Sources::now();
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    let ctx = sources.context(&ordinary, Vec::new());
    let changed = crate::notes::modify(note_id, |n| {
        let edited = f(n)?;
        let named = edited && sources.enabled && naming::refresh(n, &ctx);
        Ok((edited, edited || named))
    })?;
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if changed {
        use tauri::Emitter;
        let mut payload = crate::commands::note_changed_payload(&note, origin.as_deref().unwrap_or(""));
        payload["lineSpeakers"] = json!(line_speakers(&note));
        payload["enhancedContent"] = json!(note.enhanced_content);
        let _ = app.emit("yap-note-changed", payload);
        let _ = app.emit("yap-notes-changed", ());
    }
    Ok(note)
}

/// Rename voice `speaker` of a meeting note everywhere.
#[tauri::command]
pub fn speaker_rename(app: AppHandle, note_id: u64, speaker: String, name: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| {
        let id = resolve(&n.speakers, &speaker).to_string();
        let before = name_of(&n.speakers, &id).0;
        let changed = rename(n, &id, &name)?;
        Ok(relabel_summary(n, &id, &before) || changed)
    })
}

/// Say who said these lines (indices into the transcript) only.
#[tauri::command]
pub fn speaker_rename_lines(
    app: AppHandle,
    note_id: u64,
    lines: Vec<usize>,
    name: String,
    origin: Option<String>,
) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| rename_lines(n, &lines, &name))
}

/// Merge voice `from` into voice `into`.
#[tauri::command]
pub fn speaker_merge(app: AppHandle, note_id: u64, from: String, into: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| {
        let id = resolve(&n.speakers, &from).to_string();
        let before = name_of(&n.speakers, &id).0;
        let changed = merge(n, &id, &into)?;
        Ok(relabel_summary(n, &id, &before) || changed)
    })
}

/// "Not Priya": take a guessed name off voice `speaker`.
#[tauri::command]
pub fn speaker_not(app: AppHandle, note_id: u64, speaker: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| not(n, &speaker))
}

/// The answer to a name card ("Speaker 2 is Priya?"): `answer` = "yes" |
/// "no" (Someone else) | "later" (Not now).
#[tauri::command]
pub fn speaker_suggestion(app: AppHandle, note_id: u64, speaker: String, answer: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| {
        let id = resolve(&n.speakers, &speaker).to_string();
        let before = name_of(&n.speakers, &id).0;
        let changed = naming::answer(n, &id, &answer)?;
        Ok(relabel_summary(n, &id, &before) || changed)
    })
}

/// "It was me": voice `speaker`'s lines are the person's own (their voice
/// reached the call side).
#[tauri::command]
pub fn speaker_is_me(app: AppHandle, note_id: u64, speaker: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| naming::is_me(n, &speaker))
}

/// Undo "It was me" on voice `speaker`.
#[tauri::command]
pub fn speaker_not_me(app: AppHandle, note_id: u64, speaker: String, origin: Option<String>) -> Result<Note, String> {
    edit(&app, note_id, origin, |n| naming::not_me(n, &speaker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: &str, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            source: source.into(),
            text: text.into(),
            ..Default::default()
        }
    }

    fn note(lines: &[(&str, &str)]) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        n.transcript = lines.iter().map(|(s, t)| seg(s, t)).collect();
        n
    }

    fn shown(n: &Note) -> Vec<String> {
        n.transcript.iter().map(|s| display_line(&n.speakers, s)).collect()
    }

    #[test]
    fn unnamed_voices_read_as_today() {
        let n = note(&[("you", "Hi"), ("them", "Hello")]);
        assert_eq!(shown(&n), ["You", "Them"]);
        assert_eq!(speakers_line(&n), None);
        assert_eq!(default_name("them-3"), "Speaker 3");
        assert_eq!(default_name("p-1"), "Someone");
        // An old line's voice is its side; a relabelled one says so.
        let mut s = seg("them", "x");
        assert_eq!(s.speaker_id(), "them");
        s.speaker = "them-2".into();
        assert_eq!(s.speaker_id(), "them-2");
    }

    #[test]
    fn a_one_to_one_invite_names_them_as_a_guess() {
        let mut n = note(&[("you", "Hi Priya"), ("them", "Hi")]);
        let one = [("Priya Shah".to_string(), "priya@example.com".to_string())];
        assert!(calendar_guess(&mut n, &one));
        assert_eq!(shown(&n), ["You", "Priya Shah?"]);
        assert_eq!(prompt_label(&n.speakers, &n.transcript[1]), "Priya Shah");
        assert_eq!(n.speakers[THEM].email, "priya@example.com");
        assert!(speakers_line(&n).unwrap().contains("Priya Shah (a guess: the only other person invited)"));
        assert!(known_people(&n).contains(&"Priya Shah".to_string()));
        // Two invitees, or none: no guess.
        let mut two = note(&[("them", "Hi")]);
        let both = [("A".to_string(), String::new()), ("B".to_string(), String::new())];
        assert!(!calendar_guess(&mut two, &both));
        assert!(!calendar_guess(&mut two, &[]));
        assert!(two.speakers.is_empty());
        // Again: nothing changes.
        assert!(!calendar_guess(&mut n, &one));
    }

    #[test]
    fn not_priya_undoes_a_guess_for_good() {
        let mut n = note(&[("them", "Hi")]);
        calendar_guess(&mut n, &[("Priya".into(), String::new())]);
        assert!(not(&mut n, THEM).unwrap());
        assert_eq!(shown(&n), ["Them"]);
        assert_eq!(n.speakers[THEM].not, ["Priya"]);
        // The calendar can't put her back; another automatic name can.
        assert!(!calendar_guess(&mut n, &[("priya".into(), String::new())]));
        assert!(suggest(&mut n, THEM, "Sam", "", NameSource::Voice));
        assert_eq!(shown(&n), ["Sam"]);
        // A name the user gave isn't "not"-ed: it's renamed.
        rename(&mut n, THEM, "Sam").unwrap();
        assert!(not(&mut n, THEM).is_err());
        // Nothing named: nothing to undo.
        let mut plain = note(&[("them", "Hi")]);
        assert_eq!(not(&mut plain, THEM), Ok(false));
    }

    #[test]
    fn the_users_name_is_locked_against_every_automatic_source() {
        let mut n = note(&[("them", "Hi")]);
        assert!(rename(&mut n, THEM, "  Tom   Baker? ").unwrap());
        assert_eq!(shown(&n), ["Tom Baker"]);
        assert!(n.speakers[THEM].locked);
        for source in [NameSource::Platform, NameSource::Voice, NameSource::Calendar, NameSource::Ai] {
            assert!(!suggest(&mut n, THEM, "Priya", "", source), "{source:?}");
        }
        assert_eq!(shown(&n), ["Tom Baker"]);
        // Renaming to the same name changes nothing.
        assert_eq!(rename(&mut n, THEM, "Tom Baker"), Ok(false));
        assert!(rename(&mut n, THEM, "  ").is_err());
        // You is you.
        assert!(rename(&mut n, YOU, "Imogen").is_err());
    }

    #[test]
    fn automatic_names_follow_the_order_of_trust() {
        let mut n = note(&[("them", "Hi")]);
        assert!(suggest(&mut n, THEM, "Priya", "", NameSource::Calendar));
        // The call app beats the calendar; a voice match doesn't beat the app.
        assert!(suggest(&mut n, THEM, "Sam", "", NameSource::Platform));
        assert!(!suggest(&mut n, THEM, "Lee", "", NameSource::Voice));
        assert!(!suggest(&mut n, THEM, "Lee", "", NameSource::Ai));
        assert_eq!(shown(&n), ["Sam"]); // not a guess: no "?"
        assert!(suggest(&mut n, THEM, "Samuel", "", NameSource::Platform));
        assert!(!suggest(&mut n, THEM, "Me", "", NameSource::User));
        assert!(!suggest(&mut n, YOU, "Me", "", NameSource::Platform));
    }

    #[test]
    fn rename_just_these_lines_moves_them_to_a_person() {
        let mut n = note(&[("them", "One"), ("them", "Two"), ("them", "Three"), ("you", "Four")]);
        n.transcript.push(TranscriptSegment::dictation_marker(9));
        assert!(rename_lines(&mut n, &[1], "Sam").unwrap());
        assert_eq!(shown(&n)[..4], ["Them", "Sam", "Them", "You"]);
        assert_eq!(n.transcript[1].speaker, "p-1");
        assert!(n.transcript[1].speaker_locked);
        // Another line by the same name joins Sam; "You" is the mic's voice.
        assert!(rename_lines(&mut n, &[2], "sam").unwrap());
        assert_eq!(n.transcript[2].speaker, "p-1");
        assert!(rename_lines(&mut n, &[0], "You").unwrap());
        assert_eq!(shown(&n)[..4], ["You", "Sam", "Sam", "You"]);
        // A mic line said by someone else, then back to the mic.
        assert!(rename_lines(&mut n, &[3], "Ola").unwrap());
        assert_eq!(n.transcript[3].speaker, "p-2");
        assert!(rename_lines(&mut n, &[3], "you").unwrap());
        assert_eq!(n.transcript[3].speaker, "", "back to its side's own voice");
        assert!(n.transcript[3].speaker_locked);
        // A marker, or a line that isn't there, can't be renamed.
        assert!(rename_lines(&mut n, &[4], "Sam").is_err());
        assert!(rename_lines(&mut n, &[40], "Sam").is_err());
        // Renaming everywhere doesn't move the user's lines.
        rename(&mut n, THEM, "Priya").unwrap();
        assert_eq!(shown(&n)[..3], ["You", "Sam", "Sam"]);
    }

    #[test]
    fn naming_a_line_after_a_guess_confirms_it() {
        let mut n = note(&[("them", "One"), ("them", "Two")]);
        calendar_guess(&mut n, &[("Priya".into(), String::new())]);
        assert!(rename_lines(&mut n, &[0], "Priya").unwrap());
        assert_eq!(n.transcript[0].speaker, "");
        assert_eq!(shown(&n), ["Priya", "Priya"]);
        assert!(n.speakers[THEM].locked);
    }

    #[test]
    fn merging_two_voices_makes_one_person() {
        let mut n = note(&[("them", "One"), ("them", "Two"), ("them", "Three")]);
        n.transcript[1].speaker = "them-1".into();
        n.transcript[2].speaker = "them-2".into();
        assert_eq!(shown(&n), ["Them", "Speaker 1", "Speaker 2"]);
        assert!(speakers_line(&n).unwrap().contains("Speaker 2 (not named yet)"));
        // Speaker 2 is Priya (the user said); Speaker 1 is the same person.
        rename(&mut n, "them-2", "Priya").unwrap();
        assert!(merge(&mut n, "them-1", "them-2").unwrap());
        assert_eq!(shown(&n), ["Them", "Priya", "Priya"]);
        // A later line from the merged voice reads as the merged one.
        n.transcript.push(TranscriptSegment { speaker: "them-1".into(), ..seg("them", "Four") });
        assert_eq!(shown(&n)[3], "Priya");
        // The merged voice keeps the more trusted name, either way round.
        suggest(&mut n, THEM, "Sam", "", NameSource::Calendar);
        assert!(merge(&mut n, "them-2", THEM).unwrap());
        assert_eq!(shown(&n), ["Priya", "Priya", "Priya", "Priya"]);
        assert!(n.speakers[THEM].locked);
        assert_eq!(merge(&mut n, "them-1", THEM), Ok(false), "already one");
        assert!(merge(&mut n, THEM, YOU).is_err());
        assert_eq!(voices(&n), [THEM]);
    }

    #[test]
    fn renaming_to_another_voices_name_merges_them() {
        let mut n = note(&[("them", "One"), ("them", "Two")]);
        n.transcript[1].speaker = "them-2".into();
        rename(&mut n, THEM, "Priya").unwrap();
        assert!(rename(&mut n, "them-2", "priya").unwrap());
        assert_eq!(n.speakers["them-2"].merged_into, THEM);
        assert_eq!(shown(&n), ["priya", "priya"]);
        // "Speaker 1" is a name too; "You" isn't one to take.
        let mut m = note(&[("them", "a"), ("them", "b")]);
        m.transcript[0].speaker = "them-1".into();
        m.transcript[1].speaker = "them-2".into();
        assert!(rename(&mut m, "them-2", "Speaker 1").unwrap());
        assert_eq!(shown(&m), ["Speaker 1", "Speaker 1"]);
        assert!(rename(&mut m, "them-1", "You").is_err());
    }

    #[test]
    fn old_notes_and_maps_load() {
        let old: Note = serde_json::from_value(json!({
            "id": 3, "createdTs": 1, "updatedTs": 1,
            "transcript": [{ "source": "them", "text": "hi", "ts": 5 }]
        }))
        .unwrap();
        assert!(old.speakers.is_empty());
        assert_eq!(old.transcript[0].speaker_id(), "them");
        assert_eq!(old.transcript[0].start_ms, None);
        // Nothing new reaches disk until there is something.
        let back = serde_json::to_value(&old).unwrap();
        assert!(back.get("speakers").is_none());
        assert_eq!(back["transcript"][0], json!({ "source": "them", "text": "hi", "ts": 5 }));
        // A map round-trips in its own shape.
        let mut n = note(&[("them", "x")]);
        calendar_guess(&mut n, &[("Priya".into(), "p@example.com".into())]);
        let v = serde_json::to_value(&n.speakers).unwrap();
        assert_eq!(v, json!({ "them": { "name": "Priya", "email": "p@example.com", "source": "calendar" } }));
        let again: Speakers = serde_json::from_value(v).unwrap();
        assert_eq!(again, n.speakers);
    }

    fn timed(words: &[(&str, f32, f32)]) -> Vec<TimedText> {
        words
            .iter()
            .map(|(t, s, e)| TimedText { text: t.to_string(), start: *s, end: *e })
            .collect()
    }

    #[test]
    fn word_times_follow_the_corrected_text() {
        // Word for word (punctuation and case don't matter).
        let t = timed(&[("Hello,", 0.0, 0.4), ("there", 0.5, 0.9)]);
        assert_eq!(word_times("hello there.", &t), [(0, 400), (500, 900)]);
        // The dictionary joined four words into one: it takes their time.
        let t = timed(&[("ask", 0.0, 0.2), ("chat", 0.3, 0.5), ("g", 0.5, 0.6), ("p", 0.6, 0.7), ("t", 0.7, 0.8), ("now", 1.0, 1.2)]);
        assert_eq!(word_times("ask ChatGPT now", &t), [(0, 200), (300, 800), (1000, 1200)]);
        // A word swapped for another: the one it replaced.
        let t = timed(&[("the", 0.0, 0.1), ("jaison", 0.2, 0.6), ("file", 0.7, 0.9)]);
        assert_eq!(word_times("the JSON file", &t), [(0, 100), (200, 600), (700, 900)]);
        // Whisper's phrases: words spread over the phrase by length.
        let t = timed(&[(" one two", 0.0, 1.0), (" three", 1.0, 1.5)]);
        let w = word_times("one two three", &t);
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].0, w[1].1), (0, 1000));
        assert!(w[0].1 == w[1].0 && w[0].1 > 0 && w[0].1 < 1000);
        assert_eq!(w[2], (1000, 1500));
        // A word added with nothing to match, at the end.
        let t = timed(&[("go", 0.0, 0.3)]);
        assert_eq!(word_times("go now", &t), [(0, 300), (300, 300)]);
        // No timings (Moonshine, the stub): none.
        assert!(word_times("hello", &[]).is_empty());
        assert!(word_times("", &t).is_empty());
    }

    #[test]
    fn line_timing_places_a_line_on_the_recordings_clock() {
        let (s, e, w) = line_timing(10_000, 10_300, 25_000, &[(400, 700), (800, 1_200)]);
        assert_eq!((s, e), (10_400, 11_200));
        assert_eq!(w, [[0, 300], [400, 800]]);
        assert_eq!(line_timing(10_000, 10_300, 25_000, &[]), (10_300, 25_000, vec![]));
    }

    fn timed_line(text: &str, start: u64, words: &[[u32; 2]]) -> TranscriptSegment {
        TranscriptSegment {
            source: "them".into(),
            text: text.into(),
            ts: 100,
            start_ms: Some(start),
            end_ms: Some(start + words.last().map_or(0, |w| w[1] as u64)),
            words: words.to_vec(),
            ..Default::default()
        }
    }

    fn turn(s: u64, e: u64, who: &str) -> Turn {
        Turn { start_ms: s, end_ms: e, speaker: who.into() }
    }

    #[test]
    fn a_line_splits_at_its_words_where_the_speaker_changes() {
        // "Sure, I can. Thanks Priya, bye." — speaker 1 then speaker 2 at 2.0 s.
        let line = timed_line(
            "Sure, I can. Thanks Priya, bye.",
            10_000,
            &[[0, 400], [450, 600], [650, 1_200], [2_100, 2_500], [2_550, 3_000], [3_050, 3_400]],
        );
        let parts = split_line(&line, &[turn(9_800, 11_900, "them-1"), turn(12_000, 14_000, "them-2")]);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].text.as_str(), parts[0].speaker.as_str()), ("Sure, I can.", "them-1"));
        assert_eq!((parts[1].text.as_str(), parts[1].speaker.as_str()), ("Thanks Priya, bye.", "them-2"));
        assert_eq!((parts[0].start_ms, parts[0].end_ms), (Some(10_000), Some(11_200)));
        assert_eq!((parts[1].start_ms, parts[1].end_ms), (Some(12_100), Some(13_400)));
        assert_eq!(parts[1].words, [[0, 400], [450, 900], [950, 1_300]]);
        assert_eq!(parts[1].ts, 102); // two seconds in
        // One speaker all through: the line relabelled, whole.
        let one = split_line(&line, &[turn(9_000, 15_000, "them-3")]);
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].text.as_str(), one[0].speaker.as_str()), (line.text.as_str(), "them-3"));
    }

    #[test]
    fn without_word_times_a_line_splits_in_proportion() {
        let mut line = timed_line("aaaa bbbb cccc dddd", 0, &[]);
        line.end_ms = Some(4_000);
        let parts = split_line(&line, &[turn(0, 2_000, "them-1"), turn(2_000, 4_000, "them-2")]);
        let texts: Vec<_> = parts.iter().map(|p| (p.text.as_str(), p.speaker.as_str())).collect();
        assert_eq!(texts, [("aaaa bbbb", "them-1"), ("cccc dddd", "them-2")]);
        assert!(parts.iter().all(|p| p.words.is_empty()));
    }

    #[test]
    fn blips_gaps_and_the_users_lines() {
        let line = timed_line("one two three four", 0, &[[0, 500], [600, 900], [1_000, 1_500], [1_600, 2_000]]);
        // A 300 ms blip of another voice in the middle: smoothed away.
        let parts = split_line(
            &line,
            &[turn(0, 590, "them-1"), turn(600, 900, "them-2"), turn(950, 2_000, "them-1")],
        );
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].speaker, "them-1");
        // A word far from every turn goes with its neighbours.
        let far = timed_line("one two three", 0, &[[0, 500], [600, 900], [4_000, 4_500]]);
        let gap = split_line(&far, &[turn(0, 950, "them-1")]);
        assert_eq!(gap.len(), 1);
        assert_eq!((gap[0].speaker.as_str(), gap[0].text.as_str()), ("them-1", "one two three"));
        // The user's line, a marker, a line with no place, no turns: unchanged.
        let mut mine = line.clone();
        mine.speaker_locked = true;
        mine.speaker = "p-1".into();
        assert_eq!(split_line(&mine, &[turn(0, 9_000, "them-1")])[0].speaker, "p-1");
        // A line the person edited: theirs as written, never split.
        let mut fixed = line.clone();
        fixed.edited = true;
        let turns = [turn(0, 590, "them-1"), turn(600, 2_000, "them-2")];
        assert_eq!(split_line(&line, &turns).len(), 2);
        assert_eq!(split_line(&fixed, &turns).len(), 1);
        let marker = TranscriptSegment::dictation_marker(1);
        assert_eq!(split_line(&marker, &[turn(0, 9_000, "them-1")]).len(), 1);
        let mut unplaced = line.clone();
        unplaced.start_ms = None;
        assert_eq!(split_line(&unplaced, &[turn(0, 9_000, "them-1")])[0].speaker, "");
        assert_eq!(split_line(&line, &[])[0].speaker, "");
    }
}
