//! **Who each voice is**: names for a meeting's voices, suggested from what
//! the meeting itself says and who was invited, for the person to confirm
//! ("Speaker 2 is Priya?" — Yes / Someone else / Not now). The gap reviewers
//! name most after "no bot" (Granola's "Me/Them is a coin flip", Fireflies'
//! "Speaker 1/2"). No voiceprints: nothing here listens to how anyone
//! sounds; it reads what was said, who was invited, and (behind a flag) who
//! the call app showed talking.
//!
//! **Evidence**, each a cue with a weight for a (voice, person) pair:
//! - **Introductions** — "Hi, I'm Priya", "this is Sam from finance", "Sam
//!   here": the voice saying it is that person.
//! - **Handing over** — "Priya, can you walk us through it?", "What do you
//!   think, Sam?", "Over to you, Siobhan" at the end of a turn: the voice
//!   that answers is that person.
//! - **Thanks** — "Thanks, Priya", "Good point, Sam" at the start of a turn:
//!   the voice just before is that person.
//! - **Never yourself** — a voice that hands over to Priya, thanks her or
//!   talks about her ("Priya will send it") isn't Priya.
//! - **The invite** — a 1:1 meeting's other invitee (`calendar_guess`).
//!   Never "the one invitee left without a voice": an invitee away and
//!   someone uninvited speaking made that wrong in the evaluations.
//! - **The call app** ([`Context::platform`], `call_ui.rs`, a flag): who
//!   Teams, Zoom or Meet showed talking while a voice spoke.
//! - **The AI** ([`Context::model`], optional, when a meeting ends): one short
//!   question per unnamed voice, its answer checked against the transcript.
//! - **"It was me"** — a voice whose lines repeat yours (your own voice
//!   reaching the call side) is you.
//!
//! **Names** come from the note's attendees (the calendar invite's, and any
//! the person added), the names the person already gave voices, Memory's
//! People, and names said in an introduction (or twice when handing over).
//! Never the person's own name (About you): that's "You".
//!
//! **Deciding** ([`propose`]): a pair needs a net score of [`SUGGEST`] and a
//! clear lead over the voice's next-best name and the name's next-best voice
//! ([`MARGIN`]); names go one per voice, strongest first. A suggestion is a
//! question on a card. Only a strong one — [`APPLY`], two separate cues (or
//! an introduction) and nothing against it — is also put on the voice as
//! Yap's guess ("Priya?", told to the AI as a guess), so the action plan
//! reads it. A wrong name is worse than "Speaker 2": when in doubt, nothing.
//!
//! [`refresh`] writes the proposals into the speaker map (`Speaker::
//! suggestion`): it runs after each speaker-labelling pass, after the
//! person's own speaker edits, and when a meeting ends (where the optional
//! AI step comes first). Pure apart from the app glue at the bottom.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{resolve, voice_of, NameSource, Speaker, Suggestion, Why, SEPARATED_PREFIX, THEM, YOU};
use crate::notes::Note;

/// A pair needs this net score to be suggested…
pub const SUGGEST: f32 = 1.0;
/// …and this lead over the voice's next-best name and the name's next-best
/// voice.
pub const MARGIN: f32 = 0.6;
/// Put on the voice as Yap's guess (shown "Priya?") from this net score…
pub const APPLY: f32 = 2.0;
/// …with this lead.
pub const APPLY_MARGIN: f32 = 1.5;
/// A voice with fewer words than this gets no suggestion.
const MIN_WORDS: usize = 4;
/// A handover is answered, a thanks follows, within this long (ms).
const REPLY_GAP_MS: u64 = 15_000;
/// A voice is you when this share of its words repeats yours…
const ECHO_SHARE: f32 = 0.6;
/// …and at least this many.
const ECHO_WORDS: usize = 8;
/// One of your lines is near enough to repeat when within this long (ms).
const ECHO_NEAR_MS: u64 = 6_000;
/// Quotes on the card are at most this many characters.
const QUOTE_CHARS: usize = 90;

// Cue weights.
const W_INTRO: f32 = 1.6;
const W_INTRO_NAME_IS: f32 = 1.8;
const W_INTRO_THIS_IS: f32 = 1.3;
const W_ASKED: f32 = 1.0;
const W_HANDOVER: f32 = 1.2;
const W_THANKED: f32 = 1.0;
const W_ACK: f32 = 0.7;
const W_GREETED: f32 = 0.3;
const W_NOT_SELF: f32 = 2.0;
const W_MENTION: f32 = 1.0;
const W_INTRODUCED_OTHER: f32 = 1.0;
const W_INVITE: f32 = 1.2;
const W_MODEL: f32 = 1.0;
const W_APP: f32 = 3.0;

// ---- what the app hands in ----

/// What the naming reads besides the note.
pub struct Context<'a> {
    /// The person using Yap (About you's name): never a call voice.
    pub me: String,
    /// Memory's People: names that are people, wherever they're said.
    pub known: Vec<String>,
    /// Is this lowercased word an everyday one (not a name)? Words that
    /// aren't can be names said in the meeting.
    pub ordinary: &'a dyn Fn(&str) -> bool,
    /// Who the call app showed talking during each voice's speech.
    pub platform: Vec<PlatformVote>,
    /// The AI's checked answers (the optional step).
    pub model: Vec<ModelVote>,
}

impl Context<'_> {
    /// No extra sources: the transcript, the invite and the map only.
    /// (The speaker evaluations and tests.)
    #[cfg(any(test, debug_assertions))]
    pub fn bare(ordinary: &dyn Fn(&str) -> bool) -> Context<'_> {
        Context {
            me: String::new(),
            known: Vec::new(),
            ordinary,
            platform: Vec::new(),
            model: Vec::new(),
        }
    }
}

/// The call app's readings lined up with a voice: `name` was shown talking
/// in `reads` of the voice's readings, `share` of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformVote {
    pub voice: String,
    pub name: String,
    pub reads: u32,
    pub share: f32,
}

/// The AI said `voice` is `name`, and Rust found `quote` in the transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVote {
    pub voice: String,
    pub name: String,
    pub quote: String,
}

// ---- words and sentences ----

#[derive(Debug, Clone)]
struct Word {
    /// As written.
    raw: String,
    /// Lowercased letters, digits and inner apostrophes; a possessive "'s"
    /// taken off (see `poss`).
    key: String,
    /// It ended in "'s" ("Priya's").
    poss: bool,
    /// Starts with a capital letter.
    cap: bool,
    /// A comma (or a dash, a colon) right after it.
    comma: bool,
}

impl Word {
    fn new(raw: &str) -> Word {
        let raw = raw.trim_matches(|c: char| matches!(c, '"' | '“' | '”' | '(' | ')' | '[' | ']' | '‘'));
        let trail: String = raw
            .chars()
            .rev()
            .take_while(|c| !c.is_alphanumeric())
            .collect();
        let mut key: String = raw
            .chars()
            .map(|c| if c == '’' { '\'' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-')
            .flat_map(char::to_lowercase)
            .collect();
        key = key.trim_matches(|c| c == '\'' || c == '-').to_string();
        let mut poss = false;
        if let Some(base) = key.strip_suffix("'s") {
            if !matches!(base, "it" | "that" | "what" | "there" | "here" | "who" | "he" | "she" | "let") {
                poss = true;
                key = base.to_string();
            }
        }
        Word {
            raw: raw.to_string(),
            key,
            poss,
            cap: raw.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_uppercase),
            comma: trail.contains([',', ';', ':', '—', '–']),
        }
    }

    fn ends_sentence(&self) -> bool {
        let t = self.raw.trim_end_matches(['"', '”', ')', '\'']);
        (t.ends_with(['.', '?', '!']) || t.ends_with('…')) && !ABBREVIATIONS.contains(&self.key.as_str())
    }

    fn question(&self) -> bool {
        self.raw.trim_end_matches(['"', '”', ')', '\'']).ends_with('?')
    }
}

const ABBREVIATIONS: [&str; 10] = ["mr", "mrs", "ms", "dr", "st", "e.g", "i.e", "etc", "vs", "prof"];

fn words(text: &str) -> Vec<Word> {
    text.split_whitespace().map(Word::new).filter(|w| !w.key.is_empty()).collect()
}

/// A sentence of a transcript line, placed in time and in the meeting.
#[derive(Debug, Clone)]
struct Utt {
    /// Its line in the transcript.
    line: usize,
    /// Its voice (after merges).
    voice: String,
    /// Which recording of the note (a resume starts the next).
    rec: usize,
    start: u64,
    end: u64,
    words: Vec<Word>,
}

impl Utt {
    fn question(&self) -> bool {
        self.words.last().is_some_and(Word::question)
    }

    fn text(&self) -> String {
        self.words.iter().map(|w| w.raw.as_str()).collect::<Vec<_>>().join(" ")
    }

    fn keys(&self) -> Vec<&str> {
        self.words.iter().map(|w| w.key.as_str()).collect()
    }
}

/// Every sentence of talk in `note`, in time order: each line split at its
/// sentences, placed by its words' times (in proportion without them).
fn utterances(note: &Note) -> Vec<Utt> {
    let mut out = Vec::new();
    for (i, seg) in note.transcript.iter().enumerate() {
        if !seg.is_talk() || seg.text.trim().is_empty() {
            continue;
        }
        let voice = voice_of(&note.speakers, seg).to_string();
        let rec = note.breaks.iter().filter(|&&b| b <= i).count();
        let ws = words(&seg.text);
        if ws.is_empty() {
            continue;
        }
        let start = seg.start_ms.unwrap_or(seg.ts.saturating_mul(1000));
        let end = seg.end_ms.unwrap_or(start + 400 * ws.len() as u64).max(start);
        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        let times: Vec<(u64, u64)> = if seg.words.len() == raw_words.len() && raw_words.len() == ws.len() {
            seg.words.iter().map(|[s, e]| (start + *s as u64, start + *e as u64)).collect()
        } else {
            let n = ws.len() as u64;
            (0..n).map(|k| (start + (end - start) * k / n, start + (end - start) * (k + 1) / n)).collect()
        };
        let mut from = 0;
        for k in 0..ws.len() {
            if ws[k].ends_sentence() || k + 1 == ws.len() {
                out.push(Utt {
                    line: i,
                    voice: voice.clone(),
                    rec,
                    start: times[from].0,
                    end: times[k].1.max(times[from].0),
                    words: ws[from..=k].to_vec(),
                });
                from = k + 1;
            }
        }
    }
    out.sort_by_key(|u| (u.rec, u.start, u.line));
    out
}

/// A run of one voice's sentences (utterance indices `first..=last`).
#[derive(Debug, Clone)]
struct Turn {
    voice: String,
    first: usize,
    last: usize,
    words: usize,
}

fn turns(utts: &[Utt]) -> (Vec<Turn>, Vec<usize>) {
    let mut turns: Vec<Turn> = Vec::new();
    let mut turn_of = Vec::with_capacity(utts.len());
    for (k, u) in utts.iter().enumerate() {
        match turns.last_mut() {
            Some(t) if t.voice == u.voice && utts[t.last].rec == u.rec => {
                t.last = k;
                t.words += u.words.len();
            }
            _ => turns.push(Turn {
                voice: u.voice.clone(),
                first: k,
                last: k,
                words: u.words.len(),
            }),
        }
        turn_of.push(turns.len() - 1);
    }
    (turns, turn_of)
}

/// "Yeah." "Mm-hmm." "Right, okay." — not a turn anyone hands over to.
fn backchannel(utts: &[Utt], t: &Turn) -> bool {
    t.words <= 3
        && utts[t.first..=t.last]
            .iter()
            .flat_map(|u| u.words.iter())
            .all(|w| BACKCHANNEL.contains(&w.key.as_str()))
}

const BACKCHANNEL: [&str; 24] = [
    "yeah", "yes", "yep", "yup", "mm", "mhm", "mm-hmm", "hmm", "uh-huh", "right", "ok", "okay", "sure", "great", "cool",
    "nice", "thanks", "oh", "ah", "aha", "no", "true", "exactly", "totally",
];

// ---- people ----

#[derive(Debug, Clone)]
struct Person {
    /// As shown.
    name: String,
    email: String,
    /// Word sequences that mean them (lowercased), longest first.
    aliases: Vec<Vec<String>>,
    invited: bool,
    /// Memory, or a name the person gave a voice.
    known: bool,
    /// The person using Yap.
    me: bool,
    /// A voice the person (or the call app) named this already.
    taken_by: Option<String>,
}

const TITLES: [&str; 6] = ["dr", "mr", "mrs", "ms", "miss", "prof"];

fn name_tokens(name: &str) -> Vec<String> {
    name.split_whitespace()
        .map(|w| Word::new(w).key)
        .filter(|k| !k.is_empty() && !TITLES.contains(&k.as_str()))
        .collect()
}

/// The usual short forms of a first name (lowercased): what colleagues
/// call each other when the invite has the full name.
fn short_forms(first: &str) -> &'static [&'static str] {
    match first {
        "alexander" | "alexandra" => &["alex"],
        "andrew" => &["andy", "drew"],
        "anthony" => &["tony", "ant"],
        "benjamin" => &["ben"],
        "catherine" | "katherine" | "kathryn" => &["kate", "katie", "cath"],
        "charles" => &["charlie"],
        "charlotte" => &["charlie", "lottie"],
        "christopher" => &["chris"],
        "christine" | "christina" => &["chris", "tina"],
        "daniel" => &["dan", "danny"],
        "david" => &["dave"],
        "deborah" => &["debbie", "deb"],
        "edward" => &["ed", "eddie", "ted"],
        "elizabeth" => &["liz", "beth", "lizzie"],
        "eleanor" => &["ellie", "nell"],
        "jacob" => &["jake"],
        "james" => &["jim", "jamie"],
        "jennifer" => &["jen", "jenny"],
        "jessica" => &["jess"],
        "jonathan" => &["jon"],
        "joseph" => &["joe"],
        "joshua" => &["josh"],
        "margaret" => &["maggie", "meg"],
        "matthew" => &["matt"],
        "michael" => &["mike", "mick"],
        "nicholas" => &["nick"],
        "patricia" => &["pat", "trish"],
        "patrick" => &["pat"],
        "peter" => &["pete"],
        "rebecca" => &["becky", "becca"],
        "richard" => &["rich", "rick"],
        "robert" => &["rob", "bob", "robbie"],
        "samantha" => &["sam"],
        "samuel" => &["sam"],
        "stephen" | "steven" => &["steve"],
        "susan" => &["sue"],
        "thomas" => &["tom", "tommy"],
        "timothy" => &["tim"],
        "victoria" => &["vicky", "tori"],
        "william" => &["will", "bill", "billy"],
        _ => &[],
    }
}

/// Everyone a voice could be, with how they're said.
struct Roster {
    people: Vec<Person>,
}

impl Roster {
    fn add(&mut self, name: &str, email: &str, set: impl Fn(&mut Person)) {
        let name = name.trim();
        if name.is_empty() || name.contains('@') || name.chars().count() > 60 {
            return;
        }
        let tokens = name_tokens(name);
        if tokens.is_empty() {
            return;
        }
        // The same person again (by full name, or a first name alone).
        if let Some(p) = self.people.iter_mut().find(|p| {
            let theirs = &p.aliases[0];
            *theirs == tokens || (tokens.len() == 1 && theirs.first() == tokens.first()) || (theirs.len() == 1 && tokens.first() == theirs.first())
        }) {
            if tokens.len() > p.aliases[0].len() {
                p.name = name.to_string();
                p.aliases[0] = tokens;
            }
            if p.email.is_empty() {
                p.email = email.trim().to_string();
            }
            set(p);
            return;
        }
        let mut p = Person {
            name: name.to_string(),
            email: email.trim().to_string(),
            aliases: vec![tokens],
            invited: false,
            known: false,
            me: false,
            taken_by: None,
        };
        set(&mut p);
        self.people.push(p);
    }

    /// First names as aliases where only one person has that first name,
    /// and their usual short forms where only one person could be meant
    /// ("Kate" for Katherine, "Rob" for Robert).
    fn finish(&mut self) {
        let firsts: Vec<String> = self.people.iter().filter_map(|p| p.aliases[0].first().cloned()).collect();
        let shorts: Vec<String> = firsts.iter().flat_map(|f| short_forms(f).iter()).map(|s| s.to_string()).collect();
        for p in &mut self.people {
            let full = p.aliases[0].clone();
            p.aliases.truncate(1);
            if full.len() > 1 && firsts.iter().filter(|f| **f == full[0]).count() == 1 {
                p.aliases.push(vec![full[0].clone()]);
            }
            for short in short_forms(&full[0]) {
                let unique = shorts.iter().filter(|s| s.as_str() == *short).count() == 1 && !firsts.iter().any(|f| f == short);
                if unique {
                    p.aliases.push(vec![short.to_string()]);
                }
            }
        }
    }

    /// The person named at `words[i..]`: (person, words used).
    fn at(&self, words: &[Word], i: usize, any_caps: bool) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize)> = None;
        let mut tie = false;
        for (p, person) in self.people.iter().enumerate() {
            for alias in &person.aliases {
                let n = alias.len();
                if i + n > words.len() {
                    continue;
                }
                let fits = alias.iter().enumerate().all(|(k, a)| words[i + k].key == *a && (k + 1 == n || !words[i + k].poss));
                if !fits || (any_caps && !words[i].cap) {
                    continue;
                }
                match best {
                    Some((_, m)) if m > n => {}
                    Some((q, m)) if m == n && q != p => tie = true,
                    _ => {
                        best = Some((p, n));
                        tie = false;
                    }
                }
            }
        }
        if tie {
            None
        } else {
            best
        }
    }
}

/// Words around a name that are part of a greeting or a thanks, or that
/// open a sentence — never a name.
const NOT_NAMES: [&str; 70] = [
    "i", "you", "we", "they", "he", "she", "it", "this", "that", "so", "and", "but", "or", "okay", "ok", "right", "well",
    "yes", "yeah", "no", "hi", "hey", "hello", "thanks", "thank", "cheers", "morning", "afternoon", "evening", "great",
    "good", "sure", "sorry", "please", "everyone", "everybody", "all", "guys", "folks", "team", "now", "then", "just",
    "actually", "also", "anyway", "look", "listen", "oh", "ah", "um", "uh", "alright", "perfect", "lovely", "brilliant",
    "cool", "nice", "awesome", "absolutely", "exactly", "agreed", "fine", "here", "there", "today", "tomorrow", "monday",
    "friday", "sir",
];

/// Words people say after "I'm" that aren't names ("I'm Sure", in a
/// transcript that capitalises oddly).
const AFTER_IM: [&str; 40] = [
    "sure", "fine", "good", "great", "happy", "back", "sorry", "afraid", "glad", "going", "not", "really", "just",
    "here", "there", "on", "in", "at", "with", "from", "the", "a", "an", "also", "still", "done", "ready", "late",
    "free", "busy", "new", "working", "looking", "trying", "thinking", "pretty", "very", "so", "okay", "ok",
];

/// The transcript capitalises words that aren't the first of the sentence
/// elsewhere in this sentence too (a name is the point, not the casing).
fn caps_elsewhere(w: &[Word], i: usize) -> bool {
    w.iter().enumerate().skip(1).any(|(k, x)| k != i && x.cap && x.key != "i" && !x.key.starts_with("i'"))
}

fn plausible_name(w: &Word, ordinary: &dyn Fn(&str) -> bool) -> bool {
    w.cap
        && w.key.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        && w.key.chars().count() >= 2
        && !NOT_NAMES.contains(&w.key.as_str())
        && !ordinary(&w.key)
}

/// Names said in the meeting that no list has: in an introduction ("I'm
/// Rochelle", "my name is Ola"), or twice where someone is handed over to
/// or thanked. A word Windows' spell checker knows in lower case is an
/// everyday word, never one of these.
fn said_names(utts: &[Utt], roster: &Roster, ordinary: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut intro: Vec<String> = Vec::new();
    let mut vocative: BTreeMap<String, (String, u32)> = BTreeMap::new();
    let any_caps = |u: &Utt| u.words.iter().any(|w| w.cap);
    for u in utts {
        let w = &u.words;
        let keys = u.keys();
        let caps = any_caps(u);
        for i in 0..w.len() {
            if roster.at(w, i, caps).is_some() || w[i].poss {
                continue;
            }
            let at_start = i == 0 || leading_filler(&keys[..i]) || keys[..i].iter().all(|k| FILLER.contains(k) || GREETINGS.contains(k));
            // An introduction: a capitalised word that isn't one of the
            // words people say after "I'm".
            let intro_word = w[i].cap
                && w[i].key.chars().all(|c| c.is_alphabetic() || c == '-')
                && w[i].key.chars().count() >= 2
                && !NOT_NAMES.contains(&w[i].key.as_str())
                && !AFTER_IM.contains(&w[i].key.as_str());
            if intro_word {
                let n = 1;
                let here = at_start && i + n < w.len() && matches!(keys[i + n], "here" | "speaking") && (w.len() <= i + n + 5 || w[i + n].comma);
                let from = at_start && from_here(&keys, i + n);
                if (intro_before(&keys, i).is_some() && intro_after_ok(w, i + n) && (!caps_elsewhere(w, i) || !ordinary(&w[i].key))) || here || from {
                    intro.push(w[i].raw.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
                    continue;
                }
            }
            if !plausible_name(&w[i], ordinary) {
                continue;
            }
            // A second capitalised word that isn't an everyday one: a surname.
            let full = if i + 1 < w.len() && plausible_name(&w[i + 1], ordinary) && !w[i].comma {
                format!("{} {}", w[i].raw.trim_matches(|c: char| !c.is_alphanumeric()), w[i + 1].raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
            } else {
                w[i].raw.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
            };
            let n = if full.contains(' ') { 2 } else { 1 };
            if intro_before(&keys, i).is_some() && intro_after_ok(w, i + n) {
                intro.push(full);
                continue;
            }
            let addressed = (at_start && w[i + n - 1].comma && (u.question() || request(&keys)))
                || (i + n == w.len() && i > 0 && (w[i - 1].comma || handover_before(&keys, i)) && (u.question() || request(&keys)))
                || (i > 0 && thanks_before(&keys[..i]).is_some());
            if addressed {
                let e = vocative.entry(full.to_lowercase()).or_insert((full.clone(), 0));
                e.1 += 1;
            }
        }
    }
    let mut out = intro;
    out.extend(vocative.into_values().filter(|(_, n)| *n >= 2).map(|(name, _)| name));
    out
}

fn build_roster(note: &Note, ctx: &Context, utts: &[Utt]) -> Roster {
    let mut r = Roster { people: Vec::new() };
    if !ctx.me.trim().is_empty() {
        r.add(&ctx.me, "", |p| p.me = true);
    }
    for name in &note.participants {
        // Your own name in the list is still you.
        r.add(name, "", |p| {
            if !p.me {
                p.invited = true;
            }
        });
    }
    // Names already on voices: the person's own (taken), and guesses.
    for (id, s) in &note.speakers {
        if !s.named() || !s.merged_into.is_empty() {
            continue;
        }
        let taken = s.locked || s.source.is_some_and(|src| src.rank() >= NameSource::Voice.rank());
        let id = id.clone();
        r.add(&s.name, &s.email, |p| {
            p.known = true;
            if taken {
                p.taken_by = Some(id.clone());
            }
        });
        if let Some(sg) = &s.suggestion {
            if !sg.name.is_empty() {
                r.add(&sg.name, &sg.email, |_| {});
            }
        }
    }
    for name in &ctx.known {
        r.add(name, "", |p| p.known = true);
    }
    for v in &ctx.platform {
        r.add(&v.name, "", |p| p.known = true);
    }
    r.finish();
    for name in said_names(utts, &r, ctx.ordinary) {
        r.add(&name, "", |_| {});
    }
    r.finish();
    r
}

// ---- the cues ----

/// What one cue says about a (voice, person) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub voice: String,
    pub person: usize,
    /// Positive: they are; negative: they aren't.
    pub weight: f32,
    pub why: Why,
    /// The sentence it came from (one cue a sentence counts as one).
    pub utt: Option<usize>,
}

const GREETINGS: [&str; 9] = ["hi", "hey", "hello", "morning", "afternoon", "evening", "welcome", "hiya", "back"];
const THANKS: [&str; 4] = ["thanks", "thank", "cheers", "ta"];
const ACK: [&str; 43] = [
    "you", "so", "much", "very", "great", "good", "point", "question", "nice", "one", "yes", "yeah", "right", "sure",
    "ok", "okay", "agreed", "agree", "with", "exactly", "absolutely", "fair", "true", "lovely", "brilliant", "perfect",
    "and", "i", "totally", "that's", "a", "really", "interesting", "helpful", "useful", "makes", "sense", "oh", "ah",
    "well", "excellent", "awesome", "cool",
];
const FILLER: [&str; 23] = [
    "so", "okay", "ok", "right", "and", "well", "now", "um", "uh", "alright", "great", "then", "hi", "hey", "hello",
    "morning", "afternoon", "evening", "yeah", "yes", "oh", "just", "sorry",
];
const REQUEST: [&str; 24] = [
    "can", "could", "would", "will", "do", "did", "are", "have", "what", "how", "why", "when", "go", "ahead", "take",
    "walk", "share", "tell", "give", "update", "thoughts", "you", "your", "you're",
];

/// The words before `i` open the sentence (a greeting, "so", "okay").
fn leading_filler(keys: &[&str]) -> bool {
    keys.len() <= 2 && keys.iter().all(|k| FILLER.contains(k))
}

/// The sentence asks or hands over ("can you", "what do you think").
fn request(keys: &[&str]) -> bool {
    keys.iter().any(|k| REQUEST.contains(k))
}

/// "over to you, N", "hear from N", "hand over to N", "pass it to N".
fn handover_before(keys: &[&str], i: usize) -> bool {
    let before = &keys[..i];
    let tail = |n: usize| &before[before.len().saturating_sub(n)..];
    let handing = |k: &&str| matches!(*k, "hand" | "pass" | "handing" | "passing" | "it" | "me" | "turn");
    tail(3) == ["over", "to", "you"]
        || (tail(2) == ["over", "to"] && (before.len() == 2 || tail(3).first().is_some_and(handing)))
        || tail(2) == ["to", "you"]
        || tail(2) == ["hear", "from"]
        || tail(2) == ["turn", "to"]
        || (tail(1) == ["to"] && before.iter().any(|k| matches!(*k, "hand" | "pass" | "handing" | "passing")))
        || tail(2) == ["about", "you"]
        || tail(1) == ["and"] && before.len() == 1
}

/// What follows a name at the start of a sentence that hands over to them:
/// "N, you're up", "N, go ahead", "N, take it away", "N, are you there".
fn handover_after(keys: &[&str], j: usize) -> bool {
    let after = &keys[j..];
    let starts = |p: &[&str]| after.len() >= p.len() && after[..p.len()] == *p;
    starts(&["you're", "up"])
        || starts(&["you're", "next"])
        || starts(&["you're", "on", "mute"])
        || starts(&["go", "ahead"])
        || starts(&["take", "it", "away"])
        || starts(&["over", "to", "you"])
        || starts(&["your", "turn"])
        || starts(&["are", "you", "there"])
        || starts(&["did", "you", "want"])
        || starts(&["do", "you", "want"])
        || starts(&["anything", "to", "add"])
        // ("floor's" reads as "floor": its "'s" is taken off as a possessive.)
        || starts(&["floor", "yours"])
        || starts(&["the", "floor", "yours"])
        || starts(&["the", "floor", "is", "yours"])
}

/// An introduction just before word `i`: "I'm", "my name is", "this is",
/// "it's" (near the start of the sentence). Its weight.
fn intro_before(keys: &[&str], i: usize) -> Option<f32> {
    let before = &keys[..i];
    let tail = |n: usize| &before[before.len().saturating_sub(n)..];
    let near_start = |n: usize| i - n <= 4 && before[..i - n].iter().all(|k| FILLER.contains(k) || NOT_NAMES.contains(k));
    if tail(3) == ["my", "name", "is"] || tail(2) == ["my", "name's"] {
        Some(W_INTRO_NAME_IS)
    } else if tail(1) == ["i'm"] || tail(2) == ["i", "am"] {
        Some(W_INTRO)
    } else if (tail(2) == ["this", "is"] && near_start(2)) || (tail(1) == ["it's"] && near_start(1)) || (tail(2) == ["it", "is"] && near_start(2)) {
        Some(W_INTRO_THIS_IS)
    } else {
        None
    }
}

/// "<name> from legal here", "<name> from the agency here": "from", then
/// one to three words, then "here", right after the name (`j`).
fn from_here(keys: &[&str], j: usize) -> bool {
    keys.get(j) == Some(&"from") && (j + 2..=j + 4).any(|h| keys.get(h) == Some(&"here"))
}

/// After an introduced name at `j`: the sentence ends, or "here",
/// "speaking", "from finance", "joining", ", I…". Not ", our new designer"
/// or "who…" (introducing someone else).
fn intro_after_ok(w: &[Word], j: usize) -> bool {
    if j >= w.len() || w[j - 1].ends_sentence() {
        return true;
    }
    let next = w[j].key.as_str();
    if w[j - 1].comma {
        return matches!(next, "i" | "i'm" | "i've" | "i'll" | "just" | "and" | "here" | "speaking" | "calling");
    }
    matches!(next, "here" | "speaking" | "from" | "with" | "at" | "in" | "on" | "joining" | "calling" | "and" | "by")
}

/// Introducing someone else: "this is Priya, our new designer", "this is
/// Sam who runs finance".
fn introduces_other(w: &[Word], j: usize) -> bool {
    j < w.len()
        && matches!(
            w[j].key.as_str(),
            "who" | "she" | "he" | "they" | "our" | "my" | "your" | "she's" | "he's" | "they're" | "the" | "a"
        )
}

/// A thanks or acknowledgement before the name (all of `before`): its weight.
fn thanks_before(before: &[&str]) -> Option<f32> {
    if before.is_empty() || before.len() > 6 {
        return None;
    }
    let ok = before.iter().all(|k| ACK.contains(k) || THANKS.contains(k) || GREETINGS.contains(k) || FILLER.contains(k));
    if !ok {
        return None;
    }
    if before.iter().any(|k| THANKS.contains(k)) {
        Some(W_THANKED)
    } else if before.iter().any(|k| GREETINGS.contains(k)) {
        Some(W_GREETED)
    } else if before.iter().any(|k| ACK.contains(k) && !matches!(*k, "and" | "i" | "you" | "so" | "a" | "with" | "well" | "oh" | "ah")) {
        Some(W_ACK)
    } else {
        None
    }
}

fn short_quote(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= QUOTE_CHARS {
        return t.to_string();
    }
    let cut: String = t.chars().take(QUOTE_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(a, _)| a).trim_end_matches([',', ';', ':']);
    format!("{cut}…")
}

/// The turn after turn `t` that someone answered in: the next one that
/// isn't a backchannel, within [`REPLY_GAP_MS`] of `after_ms`.
fn next_turn(utts: &[Utt], turns: &[Turn], t: usize, after_ms: u64) -> Option<usize> {
    let rec = utts[turns[t].last].rec;
    turns
        .iter()
        .enumerate()
        .skip(t + 1)
        .take(4)
        .take_while(|(_, n)| utts[n.first].rec == rec && utts[n.first].start <= after_ms + REPLY_GAP_MS)
        .find(|(_, n)| n.voice != turns[t].voice && !backchannel(utts, n))
        .map(|(k, _)| k)
}

/// The turn before turn `t` someone was thanked for.
fn prev_turn(utts: &[Utt], turns: &[Turn], t: usize, before_ms: u64) -> Option<usize> {
    let rec = utts[turns[t].first].rec;
    turns[..t]
        .iter()
        .enumerate()
        .rev()
        .take(4)
        .take_while(|(_, p)| utts[p.last].rec == rec && utts[p.last].end + REPLY_GAP_MS >= before_ms)
        .find(|(_, p)| p.voice != turns[t].voice && !backchannel(utts, p))
        .map(|(k, _)| k)
}

/// Every cue the transcript gives.
fn transcript_cues(utts: &[Utt], roster: &Roster, ordinary: &dyn Fn(&str) -> bool) -> Vec<Cue> {
    let (turns, turn_of) = turns(utts);
    let mut cues = Vec::new();
    for (k, u) in utts.iter().enumerate() {
        let w = &u.words;
        let keys = u.keys();
        let caps = w.iter().any(|x| x.cap) || !utts.iter().any(|x| x.words.iter().any(|y| y.cap));
        let t = turn_of[k];
        let turn = &turns[t];
        let from_end = turn.last - k; // 0: the turn's last sentence
        let from_start = k - turn.first;
        let mut names: Vec<(usize, usize, usize)> = Vec::new(); // (person, i, n)
        let mut i = 0;
        while i < w.len() {
            match roster.at(w, i, caps) {
                Some((p, n)) => {
                    names.push((p, i, n));
                    i += n;
                }
                None => i += 1,
            }
        }
        // "Thanks Priya and Sam": one thanks for several people, so for
        // nobody in particular.
        let thanked_names = {
            let in_names = |k: usize| names.iter().any(|&(_, i, n)| (i..i + n).contains(&k));
            let ack = |k: usize| {
                let key = keys[k];
                in_names(k) || key == "and" || ACK.contains(&key) || THANKS.contains(&key) || GREETINGS.contains(&key) || FILLER.contains(&key)
            };
            let run_end = (0..w.len()).find(|&k| !ack(k)).unwrap_or(w.len());
            names.iter().filter(|&&(_, i, _)| i < run_end).count().max(1) as f32
        };
        for &(p, i, n) in &names {
            let person = &roster.people[p];
            if person.me {
                continue;
            }
            let j = i + n;
            let quote = short_quote(&u.text());
            let not_self = |weight: f32, cues: &mut Vec<Cue>| {
                cues.push(Cue {
                    voice: u.voice.clone(),
                    person: p,
                    weight: -weight,
                    why: Why::default(),
                    utt: Some(k),
                });
            };
            // "I'm Priya", "this is Sam from finance".
            if !w[j - 1].poss {
                if let Some(weight) = intro_before(&keys, i) {
                    if intro_after_ok(w, j) {
                        cues.push(Cue {
                            voice: u.voice.clone(),
                            person: p,
                            weight,
                            why: Why { kind: "introduced".into(), by: String::new(), quote },
                            utt: Some(k),
                        });
                        continue;
                    }
                    if introduces_other(w, j) {
                        not_self(W_INTRODUCED_OTHER, &mut cues);
                        continue;
                    }
                }
                // "Sam here." "Sorry, Ben here." "Priya speaking." "Declan
                // from legal here."
                let opens = i == 0 || leading_filler(&keys[..i]) || keys[..i].iter().all(|k| FILLER.contains(k) || GREETINGS.contains(k));
                if opens && from_here(&keys, j) {
                    cues.push(Cue {
                        voice: u.voice.clone(),
                        person: p,
                        weight: W_INTRO,
                        why: Why { kind: "introduced".into(), by: String::new(), quote: quote.clone() },
                        utt: Some(k),
                    });
                    continue;
                }
                if opens && j < w.len() && matches!(w[j].key.as_str(), "here" | "speaking") && (w.len() <= i + 6 || w[j].comma) {
                    cues.push(Cue {
                        voice: u.voice.clone(),
                        person: p,
                        weight: W_INTRO,
                        why: Why { kind: "introduced".into(), by: String::new(), quote },
                        utt: Some(k),
                    });
                    continue;
                }
            }
            // "Thanks, Priya." at the start of a turn: the voice before.
            let clause = (0..i).rev().find(|&k| w[k].comma).map_or(0, |k| k + 1);
            let greeting_next = i > 0 && GREETINGS.contains(&keys[i - 1]);
            let thanks = thanks_before(&keys[..i])
                .or_else(|| thanks_before(&keys[clause..i]))
                .filter(|&wt| wt > W_GREETED || greeting_next)
                .filter(|_| !w[j - 1].poss)
                .filter(|&wt| from_start <= 1 && (wt > W_GREETED || (from_start == 0 && !u.question())));
            if let Some(weight) = thanks {
                not_self(W_NOT_SELF, &mut cues);
                if let Some(pt) = prev_turn(utts, &turns, t, u.start) {
                    if turns[pt].voice != YOU {
                        cues.push(Cue {
                            voice: turns[pt].voice.clone(),
                            person: p,
                            weight: weight / thanked_names,
                            why: Why { kind: "thanked".into(), by: u.voice.clone(), quote },
                            utt: Some(k),
                        });
                    }
                }
                continue;
            }
            // Handing over: "Priya, can you…", "…, Sam?", "over to you, Siobhan".
            let at_start = i == 0 || leading_filler(&keys[..i]);
            let alone = i == 0 && j == w.len() && u.question();
            let explicit = (at_start && w[j - 1].comma && handover_after(&keys, j)) || handover_before(&keys, i);
            let greeted = i > 0 && keys[..i].iter().all(|k| GREETINGS.contains(k) || FILLER.contains(k)) && u.question();
            let asked_start = (at_start || greeted) && w[j - 1].comma && (u.question() || request(&keys[j..]));
            let asked_mid = i > 0 && j < w.len() && w[i - 1].comma && w[j - 1].comma && (u.question() || request(&keys[j..]));
            let asked_end = j == w.len() && i > 0 && (w[i - 1].comma || handover_before(&keys, i)) && (u.question() || request(&keys[..i]));
            if (alone || explicit || asked_start || asked_mid || asked_end) && !w[j - 1].poss {
                not_self(W_NOT_SELF, &mut cues);
                let factor = match from_end {
                    0 => 1.0,
                    1 => 0.6,
                    2 => 0.3,
                    _ => 0.0,
                };
                if factor > 0.0 {
                    if let Some(nt) = next_turn(utts, &turns, t, u.end) {
                        if turns[nt].voice != YOU {
                            let base = if explicit { W_HANDOVER } else { W_ASKED };
                            cues.push(Cue {
                                voice: turns[nt].voice.clone(),
                                person: p,
                                weight: base * factor,
                                why: Why { kind: "asked".into(), by: u.voice.clone(), quote },
                                utt: Some(k),
                            });
                        }
                    }
                }
                continue;
            }
            // Talking about them: not them (an everyday-word name opening a
            // sentence — "Will we…" — says nothing).
            let everyday = n == 1 && ordinary(&person.aliases.last().map_or(String::new(), |a| a[0].clone()));
            if !(everyday && i == 0) {
                not_self(W_MENTION, &mut cues);
            }
        }
    }
    cues
}

// ---- "It was me": a voice that repeats you ----

fn norm_words(text: &str) -> Vec<String> {
    words(text).into_iter().map(|w| w.key).collect()
}

/// Longest common subsequence length of two word lists.
fn lcs(a: &[String], b: &[String]) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for x in a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y { prev[j] + 1 } else { prev[j + 1].max(cur[j]) };
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Lines of voice `voice` (call side) that repeat one of your lines said
/// near them, and how many of its words repeat yours / it has.
pub fn echo_lines(note: &Note, voice: &str) -> (Vec<usize>, usize, usize) {
    let mine: Vec<(usize, u64, u64, Vec<String>)> = note
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, s)| s.source == YOU && s.is_talk() && voice_of(&note.speakers, s) == YOU)
        .filter_map(|(i, s)| Some((i, s.start_ms?, s.end_ms?, norm_words(&s.text))))
        .collect();
    let rec_of = |i: usize| note.breaks.iter().filter(|&&b| b <= i).count();
    let (mut lines, mut same, mut total) = (Vec::new(), 0, 0);
    for (i, s) in note.transcript.iter().enumerate() {
        // (A voice "It was me" merged into You still owns its own lines.)
        if s.source != THEM || !s.is_talk() || (voice_of(&note.speakers, s) != voice && s.speaker != voice) {
            continue;
        }
        let theirs = norm_words(&s.text);
        total += theirs.len();
        let (Some(start), Some(end)) = (s.start_ms, s.end_ms) else { continue };
        let rec = rec_of(i);
        let near: Vec<String> = mine
            .iter()
            .filter(|(j, ms, me, _)| rec_of(*j) == rec && *ms <= end + ECHO_NEAR_MS && *me + ECHO_NEAR_MS >= start)
            .flat_map(|(_, _, _, w)| w.iter().cloned())
            .collect();
        let hit = lcs(&theirs, &near);
        same += hit;
        if !theirs.is_empty() && hit * 10 >= theirs.len() * 6 {
            lines.push(i);
        }
    }
    (lines, same, total)
}

/// Voice `voice` mostly repeats your own lines (your voice reaching the
/// call side): "Speaker 3 is you?". Never a 1:1 invite's guess either.
pub fn repeats_you(note: &Note, voice: &str) -> bool {
    let (_, same, total) = echo_lines(note, voice);
    same >= ECHO_WORDS && total > 0 && same as f32 >= ECHO_SHARE * total as f32
}

// ---- deciding ----

/// One suggestion: voice `voice` is `name` (or you).
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub voice: String,
    pub name: String,
    pub email: String,
    pub me: bool,
    pub score: f32,
    pub confidence: f32,
    pub why: Why,
    /// Strong enough to put on the voice as Yap's guess.
    pub apply: bool,
    /// The call app said so firmly: put on as the call app's name.
    pub platform: bool,
}

/// Every voice's tally, for the evaluations' reports.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Tally {
    pub voice: String,
    pub name: String,
    pub positive: f32,
    pub negative: f32,
    pub cues: usize,
}

#[derive(Debug, Default, Clone)]
struct Pair {
    pos: f32,
    neg: f32,
    best: Option<(f32, Why)>,
    utts: Vec<usize>,
    kinds: Vec<String>,
}

impl Pair {
    fn net(&self) -> f32 {
        self.pos - self.neg
    }
}

/// The voices a name could go to: talk lines of their own, not you, not a
/// voice the person or the call app named. "Them" only while no voices are
/// told apart.
fn open_voices(note: &Note) -> Vec<(String, usize)> {
    let told_apart = note
        .transcript
        .iter()
        .any(|s| s.is_talk() && voice_of(&note.speakers, s).starts_with(SEPARATED_PREFIX));
    let mut out: Vec<(String, usize)> = Vec::new();
    for s in note.transcript.iter().filter(|s| s.is_talk()) {
        let v = voice_of(&note.speakers, s);
        if v == YOU || (v == THEM && told_apart) {
            continue;
        }
        let n = s.text.split_whitespace().count();
        match out.iter_mut().find(|(id, _)| id == v) {
            Some((_, w)) => *w += n,
            None => out.push((v.to_string(), n)),
        }
    }
    out.retain(|(id, _)| !settled(note.speakers.get(id)));
    out
}

/// Named by the person or the call app (or a voice match): no suggestion.
fn settled(s: Option<&Speaker>) -> bool {
    s.is_some_and(|s| s.named() && (s.locked || s.source.is_some_and(|src| src.rank() >= NameSource::Voice.rank())))
}

fn confidence(net: f32, kinds: &[String]) -> f32 {
    let base = 1.0 - (-net / 1.6).exp();
    let several = if kinds.len() > 1 { 0.05 } else { 0.0 };
    (base + several).clamp(0.05, 0.97)
}

/// The suggestions for `note`'s voices, strongest first, and every pair's
/// tally.
pub fn propose(note: &Note, ctx: &Context) -> (Vec<Proposal>, Vec<Tally>) {
    let utts = utterances(note);
    let roster = build_roster(note, ctx, &utts);
    let open = open_voices(note);
    let mut cues = transcript_cues(&utts, &roster, ctx.ordinary);

    let person_named = |name: &str| -> Option<usize> {
        let tokens = name_tokens(name);
        roster.people.iter().position(|p| p.aliases.contains(&tokens))
    };
    // The 1:1 invite's guess, wherever the voice separation put it.
    for (id, s) in &note.speakers {
        if s.merged_into.is_empty() && s.is_guess() && s.source == Some(NameSource::Calendar) {
            if let Some(p) = person_named(&s.name) {
                let voice = resolve(&note.speakers, id).to_string();
                cues.push(Cue {
                    voice,
                    person: p,
                    weight: W_INVITE,
                    why: Why { kind: "invite".into(), ..Default::default() },
                    utt: None,
                });
            }
        }
    }
    for v in &ctx.platform {
        if let Some(p) = person_named(&v.name) {
            cues.push(Cue {
                voice: resolve(&note.speakers, &v.voice).to_string(),
                person: p,
                weight: W_APP * v.share.clamp(0.0, 1.0),
                why: Why { kind: "app".into(), ..Default::default() },
                utt: None,
            });
        }
    }
    for v in &ctx.model {
        if let Some(p) = person_named(&v.name) {
            cues.push(Cue {
                voice: resolve(&note.speakers, &v.voice).to_string(),
                person: p,
                weight: W_MODEL,
                why: Why { kind: "model".into(), quote: short_quote(&v.quote), ..Default::default() },
                utt: None,
            });
        }
    }

    // Tally every pair.
    let mut pairs: BTreeMap<(String, usize), Pair> = BTreeMap::new();
    for c in cues {
        let pair = pairs.entry((c.voice.clone(), c.person)).or_default();
        if c.weight < 0.0 {
            pair.neg += -c.weight;
            continue;
        }
        pair.pos += c.weight;
        if pair.best.as_ref().is_none_or(|(w, _)| c.weight > *w) {
            pair.best = Some((c.weight, c.why.clone()));
        }
        if let Some(u) = c.utt {
            if !pair.utts.contains(&u) {
                pair.utts.push(u);
            }
        }
        if !pair.kinds.contains(&c.why.kind) {
            pair.kinds.push(c.why.kind.clone());
        }
    }
    let tallies: Vec<Tally> = pairs
        .iter()
        .map(|((voice, p), pair)| Tally {
            voice: voice.clone(),
            name: roster.people[*p].name.clone(),
            positive: pair.pos,
            negative: pair.neg,
            cues: pair.utts.len() + pair.kinds.iter().filter(|k| !matches!(k.as_str(), "asked" | "thanked" | "introduced")).count(),
        })
        .collect();

    let mut out: Vec<Proposal> = Vec::new();
    let mut voices_done: Vec<String> = Vec::new();
    let mut people_done: Vec<usize> = Vec::new();

    // "It was me": a voice repeating you.
    for (voice, words) in &open {
        if *words < ECHO_WORDS || voice == THEM {
            continue;
        }
        if note.speakers.get(voice).is_some_and(|s| s.refuses(ME_REFUSED)) {
            continue;
        }
        if repeats_you(note, voice) {
            let (_, same, total) = echo_lines(note, voice);
            out.push(Proposal {
                voice: voice.clone(),
                name: String::new(),
                email: String::new(),
                me: true,
                score: 3.0,
                confidence: (same as f32 / total as f32).clamp(0.5, 0.95),
                why: Why { kind: "echo".into(), ..Default::default() },
                apply: false,
                platform: false,
            });
            voices_done.push(voice.clone());
        }
    }

    let words_of = |voice: &str| open.iter().find(|(v, _)| v == voice).map_or(0, |(_, w)| *w);
    let usable = |voice: &str, p: usize| {
        let person = &roster.people[p];
        !person.me
            && person.taken_by.as_deref().is_none_or(|t| t == voice)
            && words_of(voice) >= MIN_WORDS
            && !note.speakers.get(voice).is_some_and(|s| s.refuses(&person.name))
    };
    let mut ranked: Vec<(&(String, usize), &Pair)> = pairs
        .iter()
        .filter(|((voice, p), pair)| pair.net() >= SUGGEST && open.iter().any(|(v, _)| v == voice) && usable(voice, *p))
        .collect();
    ranked.sort_by(|a, b| b.1.net().total_cmp(&a.1.net()));
    for ((voice, p), pair) in ranked {
        if voices_done.contains(voice) || people_done.contains(p) {
            continue;
        }
        let net = pair.net();
        // The voice's next-best name, and the name's next-best voice.
        let rival_name = pairs
            .iter()
            .filter(|((v, q), _)| v == voice && q != p && !roster.people[*q].me)
            .map(|(_, x)| x.net())
            .fold(0.0f32, f32::max);
        let rival_voice = pairs
            .iter()
            .filter(|((v, q), _)| q == p && v != voice && *v != YOU && open.iter().any(|(o, _)| o == v))
            .map(|(_, x)| x.net())
            .fold(0.0f32, f32::max);
        let lead = net - rival_name.max(rival_voice);
        if lead < MARGIN {
            continue;
        }
        let person = &roster.people[*p];
        let intro = pair.kinds.iter().any(|k| k == "introduced");
        let transcript_kinds = pair.kinds.iter().filter(|k| matches!(k.as_str(), "asked" | "thanked" | "introduced")).count();
        let only_soft = pair.kinds.iter().all(|k| matches!(k.as_str(), "model" | "invite"));
        let apply = net >= APPLY
            && lead >= APPLY_MARGIN
            && pair.neg == 0.0
            && !only_soft
            && (pair.utts.len() >= 2 || intro || pair.kinds.len() >= 2)
            && (person.invited || person.known || intro || transcript_kinds >= 2);
        let app = ctx
            .platform
            .iter()
            .find(|v| resolve(&note.speakers, &v.voice) == voice && name_tokens(&v.name) == person.aliases[0]);
        let platform = app.is_some_and(|v| v.reads >= 3 && v.share >= 0.75) && rival_name < 1.0;
        let why = pair.best.clone().map(|(_, w)| w).unwrap_or_default();
        out.push(Proposal {
            voice: voice.clone(),
            name: person.name.clone(),
            email: person.email.clone(),
            me: false,
            score: net,
            confidence: confidence(net, &pair.kinds),
            why,
            apply: apply || platform,
            platform,
        });
        voices_done.push(voice.clone());
        people_done.push(*p);
    }

    out.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    (out, tallies)
}

/// The name "Not me" keeps off a voice (a voice the person said isn't
/// them isn't suggested as them again).
pub const ME_REFUSED: &str = "You";

/// Write the suggestions for `note` into its speaker map: a card's question
/// for each, and Yap's guess on the voice for a strong one. A suggestion
/// whose evidence went away goes too (and the guess it put on the voice);
/// one from the call app or the AI stays unless something now says
/// otherwise. Returns whether the map changed.
pub fn refresh(note: &mut Note, ctx: &Context) -> bool {
    let (proposals, _) = propose(note, ctx);
    let mut changed = apply_proposals(note, &proposals);
    // A voice the person said was them ("It was me"): its new lines that
    // repeat theirs are hidden too, as its old ones were.
    let mine: Vec<String> = note
        .speakers
        .iter()
        .filter(|(id, s)| s.merged_into == YOU && id.starts_with(SEPARATED_PREFIX))
        .map(|(id, _)| id.clone())
        .collect();
    for voice in mine {
        for i in echo_lines(note, &voice).0 {
            note.transcript[i].echo = true;
            changed = true;
        }
    }
    changed
}

fn apply_proposals(note: &mut Note, proposals: &[Proposal]) -> bool {
    let mut changed = false;
    let taken_names: Vec<String> = proposals.iter().filter(|p| !p.me).map(|p| p.name.to_lowercase()).collect();
    // Gone: suggestions nobody proposes now (an AI or call-app one stays
    // while its voice is open and its name not given elsewhere).
    let ids: Vec<String> = note.speakers.keys().cloned().collect();
    for id in &ids {
        let proposed = proposals.iter().any(|p| p.voice == *id);
        let Some(s) = note.speakers.get(id) else { continue };
        let Some(sg) = s.suggestion.clone() else { continue };
        if proposed {
            continue;
        }
        let sticky = sg.why.as_ref().is_some_and(|w| matches!(w.kind.as_str(), "model" | "app"))
            && s.merged_into.is_empty()
            && !settled(Some(s))
            && !s.refuses(&sg.name)
            && !taken_names.contains(&sg.name.to_lowercase());
        if sticky {
            continue;
        }
        let entry = note.speakers.get_mut(id).expect("present");
        entry.suggestion = None;
        // The guess it put on the voice goes with it.
        if entry.source == Some(NameSource::Ai) && !entry.locked && entry.name.eq_ignore_ascii_case(&sg.name) {
            entry.name.clear();
            entry.email.clear();
            entry.source = None;
        }
        changed = true;
    }
    for p in proposals {
        let entry = note.speakers.entry(p.voice.clone()).or_default();
        if settled(Some(entry)) || !entry.merged_into.is_empty() {
            continue;
        }
        // The call app said so firmly: its name, no question.
        if p.platform {
            entry.suggestion = None;
            changed |= super::suggest(note, &p.voice, &p.name, &p.email, NameSource::Platform);
            continue;
        }
        let later = entry
            .suggestion
            .as_ref()
            .is_some_and(|s| s.later && s.me == p.me && s.name.eq_ignore_ascii_case(&p.name));
        let next = Suggestion {
            name: p.name.clone(),
            email: p.email.clone(),
            me: p.me,
            confidence: (p.confidence * 100.0).round() / 100.0,
            why: Some(p.why.clone()),
            later,
        };
        if entry.suggestion.as_ref() != Some(&next) {
            entry.suggestion = Some(next);
            changed = true;
        }
        if p.me {
            continue;
        }
        if p.apply {
            let source = if p.platform { NameSource::Platform } else { NameSource::Ai };
            changed |= super::suggest(note, &p.voice, &p.name, &p.email, source);
            // Nobody else goes by that guess now (a 1:1 invite's, say).
            let others: Vec<String> = note
                .speakers
                .iter()
                .filter(|(id, s)| **id != p.voice && s.is_guess() && s.name.eq_ignore_ascii_case(&p.name))
                .map(|(id, _)| id.clone())
                .collect();
            for id in others {
                if id == THEM {
                    continue; // "Them" keeps it for the lines nobody told apart.
                }
                let s = note.speakers.get_mut(&id).expect("present");
                s.name.clear();
                s.email.clear();
                s.source = None;
                changed = true;
            }
        } else {
            // A guess of ours that the evidence no longer backs.
            let entry = note.speakers.get_mut(&p.voice).expect("present");
            if entry.source == Some(NameSource::Ai) && !entry.locked && !entry.name.eq_ignore_ascii_case(&p.name) {
                entry.name.clear();
                entry.email.clear();
                entry.source = None;
                changed = true;
            }
        }
    }
    changed
}

/// The cards still to answer, best first: (voice, suggestion).
pub fn pending(note: &Note) -> Vec<(String, Suggestion)> {
    let mut out: Vec<(String, Suggestion)> = note
        .speakers
        .iter()
        .filter(|(_, s)| s.merged_into.is_empty())
        .filter_map(|(id, s)| s.suggestion.clone().filter(|g| !g.later).map(|g| (id.clone(), g)))
        .collect();
    out.sort_by(|a, b| b.1.confidence.total_cmp(&a.1.confidence));
    out
}

// ---- the person's answers ----

/// "It was me": voice `id`'s lines are yours (your voice reached the call
/// side). Its lines read as You from now on, and those that repeat one of
/// your mic's lines are hidden as heard twice (left out of the summaries,
/// like echo). "Them" (everyone on the call) can't be you.
pub fn is_me(note: &mut Note, id: &str) -> Result<bool, String> {
    let id = resolve(&note.speakers, id).to_string();
    if id == YOU {
        return Ok(false);
    }
    if id == THEM {
        return Err("Them is everyone on the call. To say you said a line, rename just that line.".to_string());
    }
    let (repeats, _, _) = echo_lines(note, &id);
    for i in repeats {
        note.transcript[i].echo = true;
    }
    let entry = note.speakers.entry(id).or_default();
    let not = std::mem::take(&mut entry.not);
    *entry = Speaker {
        merged_into: YOU.to_string(),
        not,
        ..Speaker::default()
    };
    Ok(true)
}

/// Undo "It was me" on voice `id`: its lines are the call's again.
pub fn not_me(note: &mut Note, id: &str) -> Result<bool, String> {
    let Some(entry) = note.speakers.get_mut(id).filter(|s| s.merged_into == YOU) else {
        return Ok(false);
    };
    entry.merged_into.clear();
    if !entry.refuses(ME_REFUSED) {
        entry.not.push(ME_REFUSED.to_string());
    }
    for seg in note.transcript.iter_mut() {
        if seg.source == THEM && seg.speaker == id && seg.echo {
            seg.echo = false;
        }
    }
    Ok(true)
}

/// An answer to a card: "yes" (the person confirms: the name is theirs,
/// locked; for "is it you?" the voice becomes you), "no" (Someone else:
/// never suggested again for this voice; a guess it put on goes), "later"
/// (Not now: not asked again in this meeting).
pub fn answer(note: &mut Note, id: &str, reply: &str) -> Result<bool, String> {
    let id = resolve(&note.speakers, id).to_string();
    let Some(sg) = note.speakers.get(&id).and_then(|s| s.suggestion.clone()) else {
        return Ok(false);
    };
    match reply {
        "yes" if sg.me => is_me(note, &id),
        "yes" => {
            let changed = super::rename(note, &id, &sg.name)?;
            let target = resolve(&note.speakers, &id).to_string();
            if let Some(s) = note.speakers.get_mut(&target) {
                if s.email.is_empty() {
                    s.email = sg.email.clone();
                }
                s.suggestion = None;
            }
            if let Some(s) = note.speakers.get_mut(&id) {
                s.suggestion = None;
            }
            // The card goes even when the name was already on the voice.
            let _ = changed;
            Ok(true)
        }
        "no" => {
            let entry = note.speakers.get_mut(&id).expect("present");
            entry.suggestion = None;
            let refused = if sg.me { ME_REFUSED.to_string() } else { sg.name.clone() };
            if entry.is_guess() && entry.name.eq_ignore_ascii_case(&sg.name) {
                super::not(note, &id)?;
            } else if !entry.refuses(&refused) {
                entry.not.push(refused);
            }
            Ok(true)
        }
        "later" => {
            let entry = note.speakers.get_mut(&id).expect("present");
            match entry.suggestion.as_mut() {
                Some(s) if !s.later => {
                    s.later = true;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
        other => Err(format!("Not an answer: {other}")),
    }
}

/// After the person put a name on a voice whose lines said "Speaker 2" in a
/// summary already written: the summary and the digests say the name.
pub fn rename_in_summary(note: &mut Note, old_label: &str, new_name: &str) -> bool {
    if old_label == new_name || !old_label.starts_with("Speaker ") || new_name.trim().is_empty() {
        return false;
    }
    let mut changed = false;
    let re = format!(r"\b{}\b", regex_escape(old_label));
    let Ok(re) = regex::Regex::new(&re) else { return false };
    if re.is_match(&note.enhanced_content) {
        let hash_was_fresh = note.enhanced_at_hash == crate::notes::content_hash(&note.content);
        note.enhanced_content = re.replace_all(&note.enhanced_content, new_name).into_owned();
        if hash_was_fresh {
            note.enhanced_at_hash = crate::notes::content_hash(&note.content);
        }
        changed = true;
    }
    for d in &mut note.digests {
        for a in &mut d.actions {
            if a.owner == old_label {
                a.owner = new_name.to_string();
                changed = true;
            }
        }
    }
    changed
}

fn regex_escape(s: &str) -> String {
    regex::escape(s)
}

/// The JSON object in a model's reply (fenced or not, words around it).
fn json_in(reply: &str) -> Option<Value> {
    let t = reply.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return v.is_object().then_some(v);
    }
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    (a < b).then(|| serde_json::from_str::<Value>(&t[a..=b]).ok()).flatten().filter(Value::is_object)
}

// ---- the optional AI step ----

/// One question for the meeting's model: who voice `voice` is, among
/// `names`, from `excerpt`.
#[derive(Debug, Clone, Serialize)]
pub struct Ask {
    pub voice: String,
    pub label: String,
    pub names: Vec<String>,
    pub excerpt: String,
}

/// A voice gets a question only with this many words…
const ASK_WORDS: usize = 10;
/// …at most this many questions a meeting…
pub const ASK_MAX: usize = 4;
/// …each excerpt at most this many words (~1,000 tokens with the prompt).
const ASK_EXCERPT_WORDS: usize = 650;
/// A line in an excerpt is cut at this many words.
const ASK_LINE_WORDS: usize = 45;

/// The system prompt: match from what's said, never from topic or style.
pub const ASK_PROMPT: &str = "You identify one voice in a meeting transcript. You are given the people in the meeting and excerpts of the transcript, each line labelled with who said it (\"You\" is the person taking notes; \"Speaker 1\", \"Speaker 2\"… are voices not named yet).\n\nDecide whether the transcript shows which of the people the voice you are asked about is. It shows it only when:\n- someone addresses that person by name and the voice answers right after (\"Priya, can you…?\" then the voice speaks), or\n- someone thanks or answers that person by name right after the voice spoke (the voice speaks, then \"Thanks, Priya\"), or\n- the voice introduces itself (\"I'm Priya\", \"this is Priya\").\n\nNever decide from the topic, the job, or how someone talks. A voice that addresses or thanks a person is not that person. If the excerpts don't show it, the answer is \"unknown\".\n\nReply with JSON only: {\"evidence\": \"<the exact words from the transcript that show it, or empty>\", \"name\": \"<one of the people, exactly as listed, or unknown>\"}";

/// The questions to ask for `note`: unnamed voices with no suggestion yet
/// (or only a soft one), that said enough, with names still open.
pub fn asks(note: &Note, ctx: &Context) -> Vec<Ask> {
    let utts = utterances(note);
    let roster = build_roster(note, ctx, &utts);
    let (proposals, _) = propose(note, ctx);
    let open = open_voices(note);
    let firm: Vec<&Proposal> = proposals.iter().filter(|p| p.apply || p.me).collect();
    let names: Vec<String> = roster
        .people
        .iter()
        .filter(|p| !p.me && p.taken_by.is_none() && (p.invited || p.known))
        .filter(|p| !firm.iter().any(|f| f.name.eq_ignore_ascii_case(&p.name)))
        .map(|p| p.name.clone())
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    let any_caps = utts.iter().any(|u| u.words.iter().any(|w| w.cap));
    let mentions: Vec<usize> = (0..utts.len())
        .filter(|&k| {
            let w = &utts[k].words;
            (0..w.len()).any(|i| roster.at(w, i, any_caps).is_some_and(|(p, _)| !roster.people[p].me))
        })
        .collect();
    let mut out = Vec::new();
    for (voice, words) in open {
        // "Them" is everyone on the call: only voices told apart are asked
        // about; and a voice the AI already answered for isn't asked again
        // ("Generate summary" runs this a second time).
        let answered = note.speakers.get(&voice).and_then(|s| s.suggestion.as_ref()).and_then(|g| g.why.as_ref()).is_some_and(|w| w.kind == "model");
        if voice == THEM || answered || words < ASK_WORDS || out.len() >= ASK_MAX || firm.iter().any(|f| f.voice == voice) {
            continue;
        }
        let names: Vec<String> = names
            .iter()
            .filter(|n| !note.speakers.get(&voice).is_some_and(|s| s.refuses(n)))
            .cloned()
            .collect();
        if names.is_empty() {
            continue;
        }
        // Lines near where a name is said and this voice speaks, then the
        // voice's first lines.
        let mut keep: Vec<usize> = Vec::new();
        for &m in &mentions {
            let from = m.saturating_sub(2);
            let to = (m + 3).min(utts.len());
            if utts[from..to].iter().any(|u| u.voice == voice) {
                keep.extend(from..to);
            }
        }
        keep.extend((0..utts.len()).filter(|&k| utts[k].voice == voice).take(3));
        keep.sort_unstable();
        keep.dedup();
        let label = super::name_of(&note.speakers, &voice).0;
        let mut lines: Vec<String> = Vec::new();
        let mut budget = ASK_EXCERPT_WORDS;
        let mut last: Option<usize> = None;
        for k in keep {
            let u = &utts[k];
            let text: Vec<&str> = u.words.iter().map(|w| w.raw.as_str()).take(ASK_LINE_WORDS).collect();
            if text.len() > budget {
                break;
            }
            budget -= text.len();
            let gap = last.is_some_and(|l| k > l + 1);
            if gap {
                lines.push("…".to_string());
            }
            // The same voice going on: the same line.
            if !gap && last.is_some_and(|l| utts[l].voice == u.voice) {
                if let Some(prev) = lines.last_mut() {
                    prev.push(' ');
                    prev.push_str(&text.join(" "));
                    last = Some(k);
                    continue;
                }
            }
            let who = super::name_of(&note.speakers, &u.voice).0;
            let who = if u.voice == voice { format!("{who} (the voice to identify)") } else { who };
            lines.push(format!("{who}: {}", text.join(" ")));
            last = Some(k);
        }
        if lines.is_empty() {
            continue;
        }
        out.push(Ask { voice, label, names, excerpt: lines.join("\n") });
    }
    out
}

/// The messages for one question.
pub fn ask_messages(ask: &Ask) -> Value {
    let user = format!(
        "People in the meeting besides You: {}\n\nTranscript excerpts:\n{}\n\nWhich of the people is {}? Answer with JSON only.",
        ask.names.join(", "),
        ask.excerpt,
        ask.label
    );
    json!([{ "role": "system", "content": ASK_PROMPT }, { "role": "user", "content": user }])
}

/// The answer's JSON schema (a model on this PC gets it as llama.cpp's
/// grammar): the evidence first, so the name follows from it (the
/// properties go out in alphabetical order, and the grammar follows them).
pub fn ask_schema(ask: &Ask) -> Value {
    let mut names: Vec<Value> = ask.names.iter().map(|n| json!(n)).collect();
    names.push(json!("unknown"));
    json!({
        "type": "object",
        "properties": {
            "evidence": { "type": "string", "maxLength": 200 },
            "name": { "enum": names },
        },
        "required": ["evidence", "name"],
        "additionalProperties": false,
    })
}

/// Rust's check of an answer: a listed name, a quote that's really in the
/// excerpt and says the name (or the voice's own introduction), and not
/// the voice addressing that person itself. `None`: unknown, or not shown.
pub fn check_answer(ask: &Ask, reply: &str) -> Option<ModelVote> {
    let json = json_in(reply)?;
    let name = json["name"].as_str()?.trim();
    let quote = json["evidence"].as_str().or_else(|| json["quote"].as_str()).unwrap_or("").trim();
    let person = ask.names.iter().find(|n| n.eq_ignore_ascii_case(name))?;
    let tokens = name_tokens(person);
    let first = tokens.first()?.as_str();
    // The excerpt's lines: who said it (the voice asked about?) and their words.
    let lines: Vec<(bool, Vec<String>)> = ask
        .excerpt
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(who, body)| (who.ends_with("(the voice to identify)"), norm_words(body))))
        .collect();
    // The quote, part by part where it spans lines ("Speaker 1: Sam, can
    // you…? Speaker 2 (the voice to identify): Sure…"), labels off.
    let label = regex::Regex::new(r"(?:^|\s)(?:You|Speaker \d+|[A-Z][\w.'-]*(?: [A-Z][\w.'-]*){0,2})(?: \(the voice to identify\))?:\s").ok()?;
    let mut parts: Vec<&str> = Vec::new();
    let mut at = 0;
    for m in label.find_iter(quote) {
        parts.push(&quote[at..m.start()]);
        at = m.end();
    }
    parts.push(&quote[at..]);
    let mut named_by: Option<bool> = None;
    let mut intro = false;
    let mut found_any = false;
    for part in parts.iter().map(|p| norm_words(p)).filter(|q| q.len() >= 2) {
        // Word for word (or nearly) in one line of the excerpt.
        let line = lines
            .iter()
            .find(|(_, w)| w.windows(part.len()).any(|win| win == part.as_slice()) || (part.len() >= 4 && lcs(w, &part) * 10 >= part.len() * 9))?;
        found_any = true;
        if part.iter().any(|w| w == first) {
            // Who says the name: the voice asked about, or someone else.
            named_by = Some(named_by.unwrap_or(false) || !line.0);
            if line.0 {
                intro |= part.windows(2).any(|w| {
                    (matches!(w[0].as_str(), "i'm" | "am" | "is") && w[1] == first)
                        || (w[0] == first && matches!(w[1].as_str(), "here" | "speaking" | "from"))
                });
            }
        }
    }
    if !found_any {
        return None;
    }
    // The name must be in the evidence, said by someone else about the
    // voice — or by the voice introducing itself; never the voice
    // addressing that person.
    match named_by {
        Some(true) => {}
        Some(false) if intro => {}
        _ => return None,
    }
    Some(ModelVote { voice: ask.voice.clone(), name: person.clone(), quote: quote.to_string() })
}

// ---- the app ----

/// What the naming reads from the app (taken before the notes' lock): the
/// person's own name (About you) and Memory's People.
pub struct Sources {
    /// "Suggest who's speaking" is on.
    pub enabled: bool,
    pub me: String,
    pub known: Vec<String>,
}

impl Sources {
    pub fn now() -> Sources {
        let cfg = crate::config::load();
        if !cfg.meeting_speaker_names {
            return Sources { enabled: false, me: String::new(), known: Vec::new() };
        }
        let known = crate::memory::items()
            .into_iter()
            .filter(|i| i.section == crate::memory::ops::Section::People)
            .map(|i| i.title)
            .collect();
        Sources { enabled: true, me: cfg.about_name.trim().to_string(), known }
    }

    pub fn context<'a>(&self, ordinary: &'a dyn Fn(&str) -> bool, platform: Vec<PlatformVote>) -> Context<'a> {
        Context {
            me: self.me.clone(),
            known: self.known.clone(),
            ordinary,
            platform,
            model: Vec::new(),
        }
    }
}

/// Suggest names for note `note_id`'s voices now (Rust only: cheap), and
/// tell the windows if anything changed. The notes' lock is held for the
/// pass alone.
pub fn refresh_note(app: &tauri::AppHandle, note_id: u64) {
    let sources = Sources::now();
    if !sources.enabled {
        return;
    }
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    let platform = super::call_ui::votes(note_id);
    let ctx = sources.context(&ordinary, platform);
    let changed = crate::notes::modify(note_id, |n| {
        let c = refresh(n, &ctx);
        Ok((c, c))
    });
    if let Ok(true) = changed {
        announce(app, note_id);
    }
}

/// Every window: the map (with the cards).
pub fn announce(app: &tauri::AppHandle, note_id: u64) {
    use tauri::Emitter;
    let Some(note) = crate::notes::get(note_id) else { return };
    let pending = pending(&note).len();
    tracing::debug!(note_id, pending, "Speakers: suggestions updated");
    let payload = crate::notes::commands::note_changed_payload(&note, "speakers");
    let _ = app.emit("yap-note-changed", payload);
}

/// Reply tokens for one question (the quote and a name).
const ASK_REPLY_TOKENS: u32 = 120;

/// **The AI step** (when a meeting ends, before its action plan; with
/// "Suggest who's speaking" on, and only a model that can call tools): one
/// short question per voice still without a firm name (at most
/// [`ASK_MAX`]) on the meeting's model, each
/// answer checked by Rust ([`check_answer`]) before it counts as one more
/// cue. On a model on this PC the answer comes under its JSON schema as
/// llama.cpp's grammar, inside the sidecar's Busy guard, never during a
/// dictation (`meeting_summary::chat_beside_dictation_with`). Best-effort:
/// a failure leaves the Rust names. Returns how many answers counted.
pub async fn ask_model(app: &tauri::AppHandle, note_id: u64) -> usize {
    let Ok(ep) = crate::meetings::meeting_summary::resolve_endpoint(&crate::config::load()) else {
        return 0;
    };
    // Only a model big enough to call tools (a cloud one, or ≥ 4B on this
    // PC): measured, Qwen2.5-1.5B on a processor answered 45 questions in
    // ~11 s each and Rust had to throw out every answer it gave
    // (docs/architecture/speakers.md, "Evaluations").
    if !crate::chat::tools::supports_tools(&ep.provider, &ep.model) {
        return 0;
    }
    let questions = {
        let Some(note) = crate::notes::get(note_id) else { return 0 };
        let sources = Sources::now();
        let words = crate::memory::Words::new();
        let ordinary = |w: &str| words.is(w);
        asks(&note, &sources.context(&ordinary, Vec::new()))
    };
    if questions.is_empty() {
        return 0;
    }
    let started = std::time::Instant::now();
    let mut votes = Vec::new();
    for ask in &questions {
        let opts = ep.options(ASK_REPLY_TOKENS, 0.0);
        let mut extra = serde_json::Map::new();
        if ep.is_local() {
            extra.insert(
                "response_format".into(),
                json!({ "type": "json_schema", "json_schema": { "name": "speaker", "schema": ask_schema(ask) } }),
            );
        }
        match crate::meetings::meeting_summary::chat_beside_dictation_with(app, &ep, ask_messages(ask), &opts, extra).await {
            Ok(reply) => {
                if let Some(v) = check_answer(ask, &reply) {
                    votes.push(v);
                }
            }
            Err(e) => {
                tracing::info!(note_id, "Speakers: the AI step stopped ({e})");
                break;
            }
        }
    }
    tracing::info!(
        note_id,
        asked = questions.len(),
        counted = votes.len(),
        ms = started.elapsed().as_millis() as u64,
        "Speakers: asked the AI who's who"
    );
    if votes.is_empty() {
        return 0;
    }
    let counted = votes.len();
    let sources = Sources::now();
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    let mut ctx = sources.context(&ordinary, Vec::new());
    ctx.model = votes;
    if let Ok(true) = crate::notes::modify(note_id, |n| {
        let c = refresh(n, &ctx);
        Ok((c, c))
    }) {
        announce(app, note_id);
    }
    counted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    fn ordinary(w: &str) -> bool {
        matches!(
            w,
            "the" | "numbers" | "deck" | "budget" | "design" | "great" | "update" | "hiring" | "finance" | "will" | "mark" | "rose"
        )
    }

    /// A meeting from `(voice, text)` lines, one after another (2.5 words a
    /// second, half a second between lines).
    fn meeting(lines: &[(&str, &str)]) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        let mut at = 0u64;
        for (voice, text) in lines {
            let count = text.split_whitespace().count() as u64;
            let dur = count * 400;
            let words: Vec<[u32; 2]> = (0..count).map(|k| [(k * 400) as u32, (k * 400 + 350) as u32]).collect();
            let source = if *voice == "you" { "you" } else { "them" };
            n.transcript.push(TranscriptSegment {
                source: source.into(),
                speaker: if *voice == source { String::new() } else { voice.to_string() },
                text: text.to_string(),
                ts: 1_000 + at / 1000,
                start_ms: Some(at),
                end_ms: Some(at + dur),
                words,
                ..Default::default()
            });
            at += dur + 500;
        }
        n
    }

    fn ctx() -> Context<'static> {
        Context::bare(&ordinary)
    }

    fn named(p: &[Proposal]) -> Vec<(String, String)> {
        p.iter().map(|p| (p.voice.clone(), if p.me { "You".to_string() } else { p.name.clone() })).collect()
    }

    #[test]
    fn handing_over_names_the_voice_that_answers() {
        let mut n = meeting(&[
            ("them-1", "Morning all. Let's start with design."),
            ("them-1", "Priya, can you walk us through the new screens?"),
            ("them-2", "Sure. We finished the onboarding flow last week and tested it with five people."),
            ("them-1", "Thanks, Priya. Sam, what about the budget?"),
            ("them-3", "We're on track, about two percent under."),
            ("them-1", "Great, thanks Sam."),
        ]);
        n.participants = vec!["Priya Shah".into(), "Sam Patel".into(), "Siobhan Kelly".into()];
        let (p, _) = propose(&n, &ctx());
        assert_eq!(
            named(&p),
            [("them-2".to_string(), "Priya Shah".to_string()), ("them-3".to_string(), "Sam Patel".to_string())]
        );
        // Two cues each, nothing against: put on the voices as guesses.
        assert!(p.iter().all(|p| p.apply), "{p:?}");
        assert_eq!(p[0].why.kind, "asked");
        assert_eq!(p[0].why.by, "them-1");
        // The chair addressed both, so is neither; nobody said who they are.
        assert!(!p.iter().any(|p| p.voice == "them-1"));
    }

    #[test]
    fn introductions_name_the_speaker_and_introducing_someone_else_doesnt() {
        let mut n = meeting(&[
            ("them-1", "Hi everyone, I'm Rochelle from the agency."),
            ("them-2", "And this is Sam, our new analyst. He'll be joining us on Thursdays."),
            ("them-3", "Hello, hello."),
        ]);
        n.participants = vec!["Sam Patel".into()];
        let (p, _) = propose(&n, &ctx());
        // Rochelle wasn't invited, but said so herself.
        assert_eq!(named(&p), [("them-1".to_string(), "Rochelle".to_string())]);
        assert!(!p[0].apply, "one introduction: a question, not a guess");
        assert_eq!(p[0].why.kind, "introduced");
    }

    #[test]
    fn a_reply_to_someone_else_isnt_them() {
        // Priya is asked; Sam answers that she dropped off.
        let mut n = meeting(&[
            ("them-1", "Priya, are you there? Can you share the deck?"),
            ("them-2", "I think Priya dropped off, I'll share it."),
        ]);
        n.participants = vec!["Priya Shah".into(), "Sam Patel".into()];
        let (p, tallies) = propose(&n, &ctx());
        assert!(p.is_empty(), "{p:?}");
        let t = tallies.iter().find(|t| t.voice == "them-2" && t.name == "Priya Shah").unwrap();
        assert!(t.positive > 0.0 && t.negative > 0.0);
    }

    #[test]
    fn your_own_name_and_your_answers_never_name_a_call_voice() {
        let mut n = meeting(&[
            ("them-1", "Imogen, what do you think?"),
            ("you", "I think we should ship it."),
            ("them-1", "Thanks Imogen."),
        ]);
        n.participants = vec!["Imogen Hart".into(), "Priya Shah".into()];
        let mut c = ctx();
        c.me = "Imogen Hart".into();
        let (p, _) = propose(&n, &c);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn one_cue_is_a_question_two_is_a_guess() {
        let mut n = meeting(&[
            ("them-1", "Siobhan, could you give us the hiring update?"),
            ("them-2", "Yes, we've made two offers and both were accepted."),
            ("them-1", "Okay, moving on."),
        ]);
        n.participants = vec!["Siobhan Kelly".into(), "Sam Patel".into()];
        let (p, _) = propose(&n, &ctx());
        assert_eq!(named(&p), [("them-2".to_string(), "Siobhan Kelly".to_string())]);
        assert!(!p[0].apply);
        // Applying: a card, and no name on the voice.
        assert!(refresh(&mut n, &ctx()));
        let s = &n.speakers["them-2"];
        assert_eq!(s.suggestion.as_ref().unwrap().name, "Siobhan Kelly");
        assert!(!s.named());
        assert_eq!(pending(&n).len(), 1);
        // Running again changes nothing.
        assert!(!refresh(&mut n, &ctx()));
    }

    #[test]
    fn conflicting_evidence_names_nobody() {
        // One voice asked as Priya and thanked as Sam: two people merged, or
        // a mix-up. Nothing.
        let mut n = meeting(&[
            ("them-1", "Priya, can you start?"),
            ("them-2", "Sure, the screens are done."),
            ("them-1", "Thanks, Sam."),
        ]);
        n.participants = vec!["Priya Shah".into(), "Sam Patel".into()];
        let (p, _) = propose(&n, &ctx());
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn a_voice_that_repeats_you_is_you() {
        let mut n = meeting(&[
            ("you", "Can everyone see my screen with the roadmap on it?"),
            ("them-2", "Can everyone see my screen with the roadmap on it?"),
            ("them-1", "Yes, we can see it fine."),
        ]);
        // The echo sits right on top of your line.
        n.transcript[1].start_ms = Some(300);
        n.transcript[1].end_ms = Some(4_300);
        let (p, _) = propose(&n, &ctx());
        assert_eq!(named(&p), [("them-2".to_string(), "You".to_string())]);
        assert!(refresh(&mut n, &ctx()));
        assert!(answer(&mut n, "them-2", "yes").unwrap());
        assert_eq!(resolve(&n.speakers, "them-2"), YOU);
        assert!(n.transcript[1].echo, "the repeat is hidden");
        // A later repeat from that voice is hidden too.
        n.transcript.push(TranscriptSegment {
            source: "them".into(),
            speaker: "them-2".into(),
            text: "Can everyone see my screen with the roadmap on it?".into(),
            ts: 1_009,
            start_ms: Some(9_300),
            end_ms: Some(13_300),
            ..Default::default()
        });
        n.transcript.push(TranscriptSegment {
            source: "you".into(),
            text: "Can everyone see my screen with the roadmap on it?".into(),
            ts: 1_009,
            start_ms: Some(9_000),
            end_ms: Some(13_000),
            ..Default::default()
        });
        assert!(refresh(&mut n, &ctx()));
        assert!(n.transcript[3].echo);
        assert!(!refresh(&mut n, &ctx()), "once");
        // Undo: the call's again, and not suggested again.
        assert!(not_me(&mut n, "them-2").unwrap());
        assert!(!n.transcript[1].echo);
        assert!(n.speakers["them-2"].refuses(ME_REFUSED));
        let (p, _) = propose(&n, &ctx());
        assert!(p.is_empty(), "{p:?}");
        // "Them" (everyone) can't be you.
        let mut m = meeting(&[("them", "hello")]);
        assert!(is_me(&mut m, THEM).is_err());
    }

    #[test]
    fn answers_confirm_refuse_or_put_off() {
        let mut n = meeting(&[
            ("them-1", "Over to you, Priya."),
            ("them-2", "Thanks. So the screens are done and tested."),
            ("them-1", "Thanks Priya, that's great."),
        ]);
        n.participants = vec!["Priya Shah".into(), "Sam Patel".into()];
        assert!(refresh(&mut n, &ctx()));
        // Strong: on the voice as a guess already.
        assert_eq!(n.speakers["them-2"].name, "Priya Shah");
        assert!(n.speakers["them-2"].is_guess());
        // Not now: no card, the guess stays.
        assert!(answer(&mut n, "them-2", "later").unwrap());
        assert!(pending(&n).is_empty());
        assert!(!refresh(&mut n, &ctx()), "a put-off card stays put off");
        // Yes: the person's own name now, locked.
        let mut yes = n.clone();
        assert!(answer(&mut yes, "them-2", "yes").unwrap());
        assert!(yes.speakers["them-2"].locked);
        assert!(yes.speakers["them-2"].suggestion.is_none());
        assert!(!refresh(&mut yes, &ctx()));
        // Someone else: the guess goes, and Priya is never suggested again.
        let mut no = n.clone();
        no.speakers.get_mut("them-2").unwrap().suggestion.as_mut().unwrap().later = false;
        assert!(answer(&mut no, "them-2", "no").unwrap());
        assert!(!no.speakers["them-2"].named());
        assert!(no.speakers["them-2"].refuses("Priya Shah"));
        refresh(&mut no, &ctx());
        assert!(no.speakers["them-2"].suggestion.is_none());
        assert!(!no.speakers["them-2"].named());
    }

    #[test]
    fn a_name_the_person_gave_is_never_suggested_elsewhere() {
        let mut n = meeting(&[
            ("them-1", "Priya, can you start?"),
            ("them-2", "Sure, the screens are done."),
            ("them-1", "Thanks, Priya."),
        ]);
        n.participants = vec!["Priya Shah".into()];
        // The person said Speaker 1 is Priya (the separation mixed them up).
        super::super::rename(&mut n, "them-1", "Priya Shah").unwrap();
        let (p, _) = propose(&n, &ctx());
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn nobody_is_named_by_elimination() {
        let mut n = meeting(&[
            ("them-1", "Sam, can you start us off with the numbers for this quarter?"),
            ("them-2", "Sure. Revenue is up four percent, costs are flat, and hiring is on plan for the rest of the year as agreed."),
            ("them-1", "Thanks Sam. That's really helpful for the planning we need to do next month across all of the teams."),
            ("them-3", "One question from me on the hiring plan: are the two contractor roles in that number or on top of it, because it matters for my budget."),
        ]);
        n.participants = vec!["Sam Patel".into(), "Siobhan Kelly".into(), "Priya Shah".into()];
        let (p, _) = propose(&n, &ctx());
        // Sam from the transcript; nobody can say who Speaker 1 and 3 are
        // (two invitees left for two voices).
        assert_eq!(named(&p), [("them-2".to_string(), "Sam Patel".to_string())]);
        // Nor with only one invitee left: Speaker 3 could be someone uninvited.
        n.participants.pop();
        n.speakers.insert("them-1".into(), Speaker { name: "Siobhan Kelly".into(), source: Some(NameSource::User), locked: true, ..Default::default() });
        let (p, _) = propose(&n, &ctx());
        assert!(p.iter().all(|p| p.name != "Siobhan Kelly"));
        assert!(!p.iter().any(|p| p.voice == "them-3"), "{p:?}");
    }

    #[test]
    fn a_calendar_guess_asks_to_be_confirmed() {
        let mut n = meeting(&[("you", "Hi, how was the trip?"), ("them-1", "Good thanks, long flight though.")]);
        n.participants = vec!["Priya Shah".into()];
        super::super::calendar_guess(&mut n, &[("Priya Shah".into(), "priya@example.com".into())]);
        // (The voice separation hands the guess to the one voice.)
        super::super::suggest(&mut n, "them-1", "Priya Shah", "priya@example.com", NameSource::Calendar);
        assert!(refresh(&mut n, &ctx()));
        let (voice, s) = &pending(&n)[0];
        assert_eq!((voice.as_str(), s.name.as_str()), ("them-1", "Priya Shah"));
        assert_eq!(s.why.as_ref().unwrap().kind, "invite");
        assert_eq!(s.email, "priya@example.com");
        assert_eq!(n.speakers["them-1"].source, Some(NameSource::Calendar), "still the calendar's guess");
    }

    #[test]
    fn words_and_sentences() {
        let w = words("Thanks, Priya's \"deck\" — great!");
        assert_eq!(w.iter().map(|w| w.key.as_str()).collect::<Vec<_>>(), ["thanks", "priya", "deck", "great"]);
        assert!(w[0].comma && w[1].poss && w[1].cap && !w[2].cap);
        assert!(w[3].ends_sentence());
        assert!(!Word::new("Dr.").ends_sentence());
        let n = meeting(&[("them-1", "Hello there. Priya, can you start? Thanks.")]);
        let u = utterances(&n);
        assert_eq!(u.len(), 3);
        assert!(u[1].question());
        assert!(u[0].start < u[1].start && u[1].start < u[2].start);
    }

    #[test]
    fn summary_follows_a_name() {
        let mut n = meeting(&[("them-1", "hi")]);
        n.enhanced_content = "## Speaker 2\n- Send the deck\n## Speaker 12\n- x".into();
        n.digests.push(crate::notes::MeetingDigest {
            from_seg: 0,
            to_seg: 1,
            start_ts: 0,
            end_ts: 0,
            key_points: vec![],
            decisions: vec![],
            actions: vec![crate::notes::DigestAction { owner: "Speaker 2".into(), task: "x".into(), due: String::new() }],
            questions: vec![],
            raw: String::new(),
        });
        assert!(rename_in_summary(&mut n, "Speaker 2", "Priya"));
        assert_eq!(n.enhanced_content, "## Priya\n- Send the deck\n## Speaker 12\n- x");
        assert_eq!(n.digests[0].actions[0].owner, "Priya");
        assert!(!rename_in_summary(&mut n, "Them", "Priya"));
    }

    #[test]
    fn model_answers_are_checked_against_the_transcript() {
        let mut n = meeting(&[
            ("them-1", "Right, Siobhan, could you give us the update on hiring for the new team?"),
            ("them-2", "Yes. Two offers out, one accepted, and the second should land on Friday."),
            ("them-3", "That's good news for the roadmap."),
        ]);
        n.participants = vec!["Siobhan Kelly".into(), "Sam Patel".into()];
        let questions = asks(&n, &ctx());
        let a = questions.iter().find(|a| a.voice == "them-2").expect("asked about Speaker 2");
        assert!(a.excerpt.contains("Speaker 2 (the voice to identify): Yes."));
        assert_eq!(a.names, ["Siobhan Kelly", "Sam Patel"]);
        let good = r#"{"quote": "Siobhan, could you give us the update on hiring", "name": "Siobhan Kelly"}"#;
        assert_eq!(check_answer(a, good).map(|v| v.name), Some("Siobhan Kelly".to_string()));
        // Not in the transcript, not a listed name, unknown, or no name in the quote.
        assert!(check_answer(a, r#"{"quote": "Siobhan leads hiring", "name": "Siobhan Kelly"}"#).is_none());
        assert!(check_answer(a, r#"{"quote": "Siobhan, could you give us", "name": "Rochelle"}"#).is_none());
        assert!(check_answer(a, r#"{"quote": "", "name": "unknown"}"#).is_none());
        assert!(check_answer(a, r#"{"quote": "Two offers out, one accepted", "name": "Sam Patel"}"#).is_none());
        // The voice addressing Sam isn't Sam.
        let mut m = meeting(&[("them-1", "Sam, could you send me the numbers later today please?"), ("them-2", "Will do, I'll send them after lunch today.")]);
        m.participants = vec!["Sam Patel".into(), "Priya Shah".into()];
        let a1 = asks(&m, &ctx()).into_iter().find(|a| a.voice == "them-1").unwrap();
        assert!(check_answer(&a1, r#"{"quote": "Sam, could you send me the numbers", "name": "Sam Patel"}"#).is_none());
        // Quoted with its label, or across two lines: the label is no part of it.
        let labelled = r#"{"evidence": "Speaker 1: Right, Siobhan, could you give us the update on hiring for the new team? Speaker 2 (the voice to identify): Yes. Two offers out", "name": "Siobhan Kelly"}"#;
        assert_eq!(check_answer(a, labelled).map(|v| v.name), Some("Siobhan Kelly".to_string()));
        // The voice's own words naming her: not her, unless an introduction.
        let own = r#"{"evidence": "Speaker 2 (the voice to identify): Yes. Two offers out, one accepted", "name": "Siobhan Kelly"}"#;
        assert!(check_answer(a, own).is_none());
        let schema = ask_schema(a);
        assert_eq!(schema["properties"]["name"]["enum"][2], "unknown");
    }
}
