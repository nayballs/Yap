//! **Text → Kokoro's phonemes** (G2P). Dictionary first: the misaki
//! lexicons Kokoro v1.0 was trained on (hexgrad/misaki, Apache-2.0; US and
//! GB, "gold" then "silver", ~180k words each), looked up the way misaki's
//! `en.Lexicon` does (its rules ported, without the spaCy tagger:
//! capitals, possessives, plurals, "-ed", "-ing", acronyms spelled out, "the"
//! / "to" / "a" before a vowel). Only words in neither lexicon — mostly
//! names, "Priya" — go to **eSpeak NG**, run as a separate program
//! ([`Espeak`]: `espeak-ng.exe`, downloaded with Kokoro, GPL-3.0, never
//! linked into Yap), and its IPA is mapped to misaki's symbols the way
//! misaki's own espeak fallback maps it. Without eSpeak an unknown word is
//! spelled out.
//!
//! Kokoro's voices for Spanish, French, Hindi, Italian and Brazilian
//! Portuguese read their text through eSpeak NG entirely, as misaki does for
//! those languages ([`Phonemizer::Espeak`]).
//!
//! Pure apart from [`Espeak`]'s process; the lexicon rules are unit-tested
//! on a small dictionary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

const PRIMARY: char = 'ˈ';
const SECONDARY: char = 'ˌ';

fn is_vowel(c: char) -> bool {
    "AIOQWYaiuæɑɒɔəɛɜɪʊʌᵻ".contains(c)
}
fn is_consonant(c: char) -> bool {
    "bdfhjklmnpstvwzðŋɡɹɾʃʒʤʧθ".contains(c)
}
/// Punctuation Kokoro has tokens for, which ends a word's context.
fn is_stop_punct(c: char) -> bool {
    ";:,.!?—…".contains(c)
}
/// Punctuation passed through to Kokoro as itself.
fn is_punct(c: char) -> bool {
    ";:,.!?—…\"“”()".contains(c)
}

/// A dictionary entry: one pronunciation, or one per part of speech
/// (`DEFAULT`, `NOUN`, `VERB`, `None` = said at the end of a phrase…).
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    One(String),
    Tagged(HashMap<String, Option<String>>),
}

impl Entry {
    fn pick(&self, at_end: bool) -> Option<String> {
        match self {
            Entry::One(s) => Some(s.clone()),
            Entry::Tagged(m) => {
                if at_end {
                    if let Some(Some(s)) = m.get("None") {
                        return Some(s.clone());
                    }
                }
                m.get("DEFAULT").cloned().flatten()
            }
        }
    }
}

fn parse_dict(json: &str) -> Result<HashMap<String, Entry>, String> {
    let raw: HashMap<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("Kokoro's dictionary didn't read: {e}"))?;
    let mut out = HashMap::with_capacity(raw.len());
    for (k, v) in raw {
        let entry = match v {
            serde_json::Value::String(s) => Entry::One(s),
            serde_json::Value::Object(m) => Entry::Tagged(
                m.into_iter().map(|(tag, p)| (tag, p.as_str().map(str::to_string))).collect(),
            ),
            _ => continue,
        };
        out.insert(k, entry);
    }
    Ok(grow(out))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c.flat_map(|ch| ch.to_lowercase())).collect(),
        None => String::new(),
    }
}

/// misaki's `grow_dictionary`: "monday" is also "Monday", "Monday" also
/// "monday" (entries already there win).
fn grow(d: HashMap<String, Entry>) -> HashMap<String, Entry> {
    let mut extra = HashMap::new();
    for (k, v) in &d {
        if k.chars().count() < 2 {
            continue;
        }
        let lower = k.to_lowercase();
        if *k == lower {
            let cap = capitalize(k);
            if cap != *k {
                extra.insert(cap, v.clone());
            }
        } else if *k == capitalize(&lower) {
            extra.insert(lower, v.clone());
        }
    }
    let mut d = d;
    for (k, v) in extra {
        d.entry(k).or_insert(v);
    }
    d
}

/// misaki's `apply_stress`: `stress` < -1 strips stress marks, -1 / 0 / -0.5
/// demote a primary to a secondary, 0.5–1 add a secondary where there's none,
/// > 1 a primary.
fn apply_stress(ps: &str, stress: Option<f32>) -> String {
    let Some(stress) = stress else {
        return ps.to_string();
    };
    let has = |c: char| ps.contains(c);
    let restress = |ps: String| -> String {
        // Move each stress mark to just before the vowel it belongs to.
        let chars: Vec<char> = ps.chars().collect();
        let mut keyed: Vec<(f32, char)> = chars.iter().enumerate().map(|(i, &c)| (i as f32, c)).collect();
        for (i, &c) in chars.iter().enumerate() {
            if c == PRIMARY || c == SECONDARY {
                if let Some(j) = chars[i..].iter().position(|&v| is_vowel(v)) {
                    keyed[i].0 = (i + j) as f32 - 0.5;
                }
            }
        }
        keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        keyed.into_iter().map(|(_, c)| c).collect()
    };
    if stress < -1.0 {
        ps.replace([PRIMARY, SECONDARY], "")
    } else if stress == -1.0 || ((stress == 0.0 || stress == -0.5) && has(PRIMARY)) {
        ps.replace(SECONDARY, "").replace(PRIMARY, &SECONDARY.to_string())
    } else if (stress == 0.0 || stress == 0.5 || stress == 1.0) && !has(PRIMARY) && !has(SECONDARY) {
        if !ps.chars().any(is_vowel) {
            return ps.to_string();
        }
        restress(format!("{SECONDARY}{ps}"))
    } else if stress >= 1.0 && !has(PRIMARY) && has(SECONDARY) {
        ps.replace(SECONDARY, &PRIMARY.to_string())
    } else if stress > 1.0 && !has(PRIMARY) && !has(SECONDARY) {
        if !ps.chars().any(is_vowel) {
            return ps.to_string();
        }
        restress(format!("{PRIMARY}{ps}"))
    } else {
        ps.to_string()
    }
}

fn is_lexicon_word(w: &str) -> bool {
    w.chars().all(|c| c.is_ascii_alphabetic() || c == '\'' || c == '-')
}

/// One accent's dictionaries.
pub struct Lexicon {
    british: bool,
    gold: HashMap<String, Entry>,
    silver: HashMap<String, Entry>,
}

impl Lexicon {
    /// From the misaki JSON files (`us_gold.json` + `us_silver.json`, or
    /// the `gb_` pair).
    pub fn from_json(british: bool, gold: &str, silver: &str) -> Result<Lexicon, String> {
        Ok(Lexicon { british, gold: parse_dict(gold)?, silver: parse_dict(silver)? })
    }

    pub fn load(dir: &Path, british: bool) -> Result<Lexicon, String> {
        let prefix = if british { "gb" } else { "us" };
        let read = |name: String| {
            std::fs::read_to_string(dir.join(&name)).map_err(|e| format!("Kokoro's dictionary {name}: {e}"))
        };
        Lexicon::from_json(british, &read(format!("{prefix}_gold.json"))?, &read(format!("{prefix}_silver.json"))?)
    }

    pub fn british(&self) -> bool {
        self.british
    }

    fn known_entry(&self, w: &str) -> bool {
        self.gold.contains_key(w) || self.silver.contains_key(w)
    }

    /// misaki's `is_known`.
    fn is_known(&self, w: &str) -> bool {
        if self.known_entry(w) {
            return true;
        }
        if !w.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        let n = w.chars().count();
        if n == 1 {
            return true;
        }
        if w == w.to_uppercase() && self.gold.contains_key(&w.to_lowercase()) {
            return true;
        }
        let rest: String = w.chars().skip(1).collect();
        rest == rest.to_uppercase()
    }

    /// A word spelled out letter by letter ("API" → "ˌAˌpiˈI").
    fn spell(&self, w: &str) -> Option<String> {
        let mut ps = String::new();
        for c in w.chars().filter(|c| c.is_alphabetic()) {
            ps.push_str(&self.gold.get(&c.to_uppercase().to_string())?.pick(false)?);
        }
        let ps = apply_stress(&ps, Some(0.0));
        // The last secondary becomes the primary.
        Some(match ps.rfind(SECONDARY) {
            Some(i) => format!("{}{PRIMARY}{}", &ps[..i], &ps[i + SECONDARY.len_utf8()..]),
            None => ps,
        })
    }

    /// misaki's `lookup` (no part-of-speech tags: a word's default reading;
    /// `as_name` = an acronym-like word, read only from the gold dictionary).
    fn lookup(&self, word: &str, stress: Option<f32>, at_end: bool, as_name: bool) -> Option<String> {
        let mut w = word.to_string();
        if w == w.to_uppercase() && !self.gold.contains_key(&w) {
            w = w.to_lowercase();
        }
        let mut ps = self.gold.get(&w).and_then(|e| e.pick(at_end));
        if ps.is_none() && !as_name {
            ps = self.silver.get(&w).and_then(|e| e.pick(at_end));
        }
        match ps {
            Some(p) if !(as_name && !p.contains(PRIMARY)) => Some(apply_stress(&p, stress)),
            _ => self.spell(&w),
        }
    }

    fn suffix_s(&self, stem: &str) -> String {
        match stem.chars().last() {
            Some(c) if "ptkfθ".contains(c) => format!("{stem}s"),
            Some(c) if "szʃʒʧʤ".contains(c) => format!("{stem}{}z", if self.british { 'ɪ' } else { 'ᵻ' }),
            _ => format!("{stem}z"),
        }
    }

    fn stem_s(&self, w: &str, stress: Option<f32>, at_end: bool) -> Option<String> {
        if w.len() < 3 || !w.ends_with('s') {
            return None;
        }
        let stem = if !w.ends_with("ss") && self.is_known(&w[..w.len() - 1]) {
            w[..w.len() - 1].to_string()
        } else if (w.ends_with("'s") || (w.len() > 4 && w.ends_with("es") && !w.ends_with("ies")))
            && self.is_known(&w[..w.len() - 2])
        {
            w[..w.len() - 2].to_string()
        } else if w.len() > 4 && w.ends_with("ies") && self.is_known(&format!("{}y", &w[..w.len() - 3])) {
            format!("{}y", &w[..w.len() - 3])
        } else {
            return None;
        };
        self.lookup(&stem, stress, at_end, false).map(|p| self.suffix_s(&p))
    }

    fn suffix_ed(&self, stem: &str) -> String {
        let chars: Vec<char> = stem.chars().collect();
        let last = chars.last().copied().unwrap_or(' ');
        let weak = if self.british { 'ɪ' } else { 'ᵻ' };
        if "pkfθʃsʧ".contains(last) {
            format!("{stem}t")
        } else if last == 'd' {
            format!("{stem}{weak}d")
        } else if last != 't' {
            format!("{stem}d")
        } else if self.british || chars.len() < 2 {
            format!("{stem}ɪd")
        } else if "AIOWYiuæɑəɛɪɹʊʌ".contains(chars[chars.len() - 2]) {
            format!("{}ɾᵻd", chars[..chars.len() - 1].iter().collect::<String>())
        } else {
            format!("{stem}ᵻd")
        }
    }

    fn stem_ed(&self, w: &str, stress: Option<f32>, at_end: bool) -> Option<String> {
        if w.len() < 4 || !w.ends_with('d') {
            return None;
        }
        let stem = if !w.ends_with("dd") && self.is_known(&w[..w.len() - 1]) {
            &w[..w.len() - 1]
        } else if w.len() > 4 && w.ends_with("ed") && !w.ends_with("eed") && self.is_known(&w[..w.len() - 2]) {
            &w[..w.len() - 2]
        } else {
            return None;
        };
        self.lookup(stem, stress, at_end, false).map(|p| self.suffix_ed(&p))
    }

    fn suffix_ing(&self, stem: &str) -> Option<String> {
        let chars: Vec<char> = stem.chars().collect();
        let last = *chars.last()?;
        if self.british {
            if "əː".contains(last) {
                return None;
            }
        } else if chars.len() > 1 && last == 't' && "AIOWYiuæɑəɛɪɹʊʌ".contains(chars[chars.len() - 2]) {
            return Some(format!("{}ɾɪŋ", chars[..chars.len() - 1].iter().collect::<String>()));
        }
        Some(format!("{stem}ɪŋ"))
    }

    fn stem_ing(&self, w: &str, stress: Option<f32>, at_end: bool) -> Option<String> {
        static DOUBLED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(bbing|cking|dding|gging|kking|lling|mming|nning|pping|rring|ssing|tting|vving|xxing|zzing)$").unwrap()
        });
        if w.len() < 5 || !w.ends_with("ing") {
            return None;
        }
        let base = &w[..w.len() - 3];
        let stem = if w.len() > 5 && self.is_known(base) {
            base.to_string()
        } else if self.is_known(&format!("{base}e")) {
            format!("{base}e")
        } else if w.len() > 5 && DOUBLED.is_match(w) && self.is_known(&w[..w.len() - 4]) {
            w[..w.len() - 4].to_string()
        } else {
            return None;
        };
        self.lookup(&stem, stress, at_end, false).and_then(|p| self.suffix_ing(&p))
    }

    /// One word's phonemes, or `None` when the dictionaries don't have it
    /// (it goes to the fallback). `next_vowel`: the next word starts with a
    /// vowel (`Some(true)`), a consonant (`Some(false)`), or nothing follows
    /// in the phrase (`None`).
    pub fn word(&self, word: &str, next_vowel: Option<bool>) -> Option<String> {
        let w = word.replace(['’', '‘'], "'");
        let at_end = next_vowel.is_none();
        let lower = w.to_lowercase();
        // misaki's special cases, read without a tagger.
        match w.as_str() {
            "a" | "A" if next_vowel.is_some() => return Some("ɐ".into()),
            "a" => return Some("ˈA".into()),
            "an" | "An" => return Some("ɐn".into()),
            "I" => return Some(format!("{SECONDARY}I")),
            "am" | "Am" => {
                return match next_vowel {
                    None => self.lookup("am", None, at_end, false),
                    Some(_) => Some("ɐm".into()),
                }
            }
            "to" | "To" => {
                return match next_vowel {
                    None => self.lookup("to", None, at_end, false),
                    Some(false) => Some("tə".into()),
                    Some(true) => Some("tʊ".into()),
                }
            }
            "in" | "In" => return Some(if next_vowel.is_none() { format!("{PRIMARY}ɪn") } else { "ɪn".into() }),
            "the" | "The" | "THE" => return Some(if next_vowel == Some(true) { "ði".into() } else { "ðə".into() }),
            "vs" | "vs." => return self.lookup("versus", None, at_end, false),
            _ => {}
        }
        // "p.m.", "U.S.": letters.
        if w.contains('.') {
            let letters = w.replace('.', "");
            if !letters.is_empty() && letters.chars().all(|c| c.is_ascii_alphabetic()) && w.split('.').all(|p| p.len() < 3) {
                return self.spell(&letters);
            }
        }
        if !is_lexicon_word(&w) {
            return None;
        }
        let n = w.chars().count();
        let all_caps = n > 1 && w == w.to_uppercase() && w.chars().any(|c| c.is_alphabetic());
        // A short all-caps word that isn't a word: an acronym, spelled out
        // ("API", "GPU"); "THE" and "NASA" are in the dictionary.
        if all_caps && !self.known_entry(&w) && n <= 4 && !self.gold.contains_key(&lower) {
            return self.spell(&w);
        }
        let stress = if w == lower {
            None
        } else if all_caps {
            Some(2.0)
        } else {
            Some(0.5)
        };
        let mut w = w.clone();
        let rest_lower = w.chars().skip(1).collect::<String>() == w.chars().skip(1).collect::<String>().to_lowercase();
        if n > 1
            && w.replace('\'', "").chars().all(char::is_alphabetic)
            && w != lower
            && !self.known_entry(&w)
            && (all_caps || rest_lower)
            && (self.known_entry(&lower)
                || self.stem_s(&lower, stress, at_end).is_some()
                || self.stem_ed(&lower, stress, at_end).is_some()
                || self.stem_ing(&lower, stress, at_end).is_some())
        {
            w = lower.clone();
        }
        if self.is_known(&w) {
            return self.lookup(&w, stress, at_end, false);
        }
        if let Some(stem) = w.strip_suffix("s'") {
            if self.is_known(&format!("{stem}'s")) {
                return self.lookup(&format!("{stem}'s"), stress, at_end, false);
            }
        }
        if let Some(stem) = w.strip_suffix('\'') {
            if self.is_known(stem) {
                return self.lookup(stem, stress, at_end, false);
            }
        }
        if let Some(p) = self.stem_s(&w, stress, at_end) {
            return Some(p);
        }
        if let Some(p) = self.stem_ed(&w, stress, at_end) {
            return Some(p);
        }
        if let Some(p) = self.stem_ing(&w, Some(stress.unwrap_or(0.5)), at_end) {
            return Some(p);
        }
        None
    }
}

// ------------------------------------------------------------- eSpeak NG

/// eSpeak NG as a separate program: `espeak-ng.exe` and its
/// `espeak-ng-data` in one folder. Each call is one short-lived process
/// (stdin in, IPA out, one line per line), with a time limit; nothing of it
/// is linked into Yap.
#[derive(Debug, Clone)]
pub struct Espeak {
    pub dir: PathBuf,
}

impl Espeak {
    pub fn exe(&self) -> PathBuf {
        self.dir.join("espeak-ng.exe")
    }

    /// IPA for each line (in order; a line it couldn't read is empty), in
    /// eSpeak's `voice` ("en-gb", "en-us", "es", "fr-fr"…), multi-letter
    /// phonemes tied with '^'.
    pub fn ipa(&self, voice: &str, lines: &[String]) -> Result<Vec<String>, String> {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        if lines.is_empty() {
            return Ok(Vec::new());
        }
        let mut cmd = Command::new(self.exe());
        cmd.args(["-q", "--ipa", "--tie=^", "-b", "1", "-v", voice])
            .env("ESPEAK_DATA_PATH", &self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("eSpeak NG didn't start: {e}"))?;
        crate::procs::end_with_yap(&child);
        let mut input = String::new();
        for l in lines {
            // One phrase per line: no line breaks inside one.
            input.push_str(&l.replace(['\r', '\n'], " "));
            input.push('\n');
        }
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes());
        }
        let mut stdout = child.stdout.take().ok_or("eSpeak NG has no output")?;
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = stdout.read_to_end(&mut out);
            out
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("eSpeak NG took too long".into());
                }
            }
        }
        let out = reader.join().unwrap_or_default();
        let text = String::from_utf8_lossy(&out);
        let mut got: Vec<String> = text.lines().map(|l| l.trim().to_string()).collect();
        got.truncate(lines.len());
        got.resize(lines.len(), String::new());
        Ok(got)
    }
}

/// misaki's `EspeakFallback.E2M` (longest first) + its accent rules: eSpeak's
/// English IPA in misaki's symbols. (misaki maps a lone "e" before its GB
/// "e^ə" rule, which then never fires; the GB rule runs first here.)
pub fn espeak_english(ps: &str, british: bool) -> String {
    const E2M: &[(&str, &str)] = &[
        ("ʔˌn\u{329}", "ʔn"),
        ("ʔn\u{329}", "ʔn"),
        ("a^ɪ", "I"),
        ("a^ʊ", "W"),
        ("d^ʒ", "ʤ"),
        ("e^ɪ", "A"),
        ("t^ʃ", "ʧ"),
        ("ɔ^ɪ", "Y"),
        ("ə^l", "ᵊl"),
        ("ʲo", "jo"),
        ("ʲə", "jə"),
        ("e", "A"),
        ("ʲ", ""),
        ("ɚ", "əɹ"),
        ("r", "ɹ"),
        ("x", "k"),
        ("ç", "k"),
        ("ɐ", "ə"),
        ("ɬ", "l"),
        ("\u{303}", ""),
    ];
    static SYLLABIC: LazyLock<Regex> = LazyLock::new(|| Regex::new("(\\S)\u{329}").unwrap());
    let mut ps = ps.to_string();
    if british {
        ps = ps.replace("e^ə", "ɛː");
    }
    for (old, new) in E2M {
        ps = ps.replace(old, new);
    }
    ps = SYLLABIC.replace_all(&ps, "ᵊ$1").replace('\u{329}', "");
    if british {
        ps = ps.replace("iə", "ɪə").replace("ə^ʊ", "Q");
    } else {
        ps = ps.replace("o^ʊ", "O").replace("ɜːɹ", "ɜɹ").replace("ɜː", "ɜɹ").replace("ɪə", "iə").replace('ː', "");
    }
    ps.replace('o', "ɔ").replace('ɾ', "T").replace('ʔ', "t").replace('^', "")
}

/// misaki's `EspeakG2P` mapping (Spanish, French, Hindi, Italian, Brazilian
/// Portuguese, for Kokoro v1.0).
pub fn espeak_other(ps: &str) -> String {
    const E2M: &[(&str, &str)] = &[
        ("a^ɪ", "I"),
        ("a^ʊ", "W"),
        ("d^z", "ʣ"),
        ("d^ʒ", "ʤ"),
        ("e^ɪ", "A"),
        ("o^ʊ", "O"),
        ("s^s", "S"),
        ("t^s", "ʦ"),
        ("t^ʃ", "ʧ"),
        ("ɔ^ɪ", "Y"),
        ("ə^ʊ", "Q"),
    ];
    let mut ps = ps.to_string();
    for (old, new) in E2M {
        ps = ps.replace(old, new);
    }
    ps.replace(['^', '-'], "")
}

// ------------------------------------------------------------- phonemizer

/// A piece of text: a word, or punctuation Kokoro reads as a pause.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Word(String),
    Punct(char),
}

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?x)
        (?:[A-Za-z]\.){2,}                      # p.m.  U.S.
      | [\p{L}\p{M}0-9]+(?:['’][\p{L}]+)*(?:-[\p{L}\p{M}0-9]+(?:['’][\p{L}]+)*)*['’]?   # words, it's, forty-five, users'
      | [;:,.!?—…\x22“”()]
      | \s[-–]\s
    ").unwrap()
});

fn pieces(text: &str) -> Vec<Piece> {
    TOKEN
        .find_iter(text)
        .map(|m| {
            let s = m.as_str();
            let t = s.trim();
            if t == "-" || t == "–" {
                Piece::Punct('—')
            } else if t.chars().count() == 1 && is_punct(t.chars().next().unwrap_or(' ')) {
                Piece::Punct(t.chars().next().unwrap_or(','))
            } else {
                Piece::Word(t.to_string())
            }
        })
        .collect()
}

/// A word's first sound: a vowel, a consonant, or `None`.
fn first_sound(ps: &str) -> Option<bool> {
    ps.chars().find(|&c| is_vowel(c) || is_consonant(c)).map(is_vowel)
}

/// How a voice's text becomes phonemes.
pub enum Phonemizer {
    /// English: the lexicon, eSpeak NG for the rest (if it's there).
    English { lexicon: std::sync::Arc<Lexicon>, espeak: Option<Espeak> },
    /// English all through eSpeak NG (Voice Mirror's way; here only when
    /// the lexicon won't read, and for the comparison in the real checks).
    EspeakEnglish { espeak: Espeak, british: bool },
    /// Another language, all through eSpeak NG (`voice` = eSpeak's).
    Espeak { espeak: Espeak, voice: &'static str },
}

fn espeak_en_gb(ps: &str) -> String {
    espeak_english(ps, true)
}
fn espeak_en_us(ps: &str) -> String {
    espeak_english(ps, false)
}

/// What a G2P pass found: the phonemes, and the words no dictionary had
/// (for the logs and the tests).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Phonemes {
    pub text: String,
    pub unknown: Vec<String>,
}

impl Phonemizer {
    pub fn phonemize(&self, text: &str) -> Phonemes {
        match self {
            Phonemizer::English { lexicon, espeak } => english(lexicon, espeak.as_ref(), text),
            Phonemizer::EspeakEnglish { espeak, british } => Phonemes {
                text: if *british {
                    espeak_text(espeak, "en-gb", text, espeak_en_gb)
                } else {
                    espeak_text(espeak, "en-us", text, espeak_en_us)
                },
                unknown: Vec::new(),
            },
            Phonemizer::Espeak { espeak, voice } => {
                Phonemes { text: espeak_text(espeak, voice, text, espeak_other), unknown: Vec::new() }
            }
        }
    }
}

/// All of `text` through eSpeak NG, punctuation kept (eSpeak drops it, so
/// the text is cut at punctuation and each run phonemized on its own line,
/// as the phonemizer library misaki uses does).
pub fn espeak_text(espeak: &Espeak, voice: &str, text: &str, map: fn(&str) -> String) -> String {
    let ps = pieces(text);
    let mut runs: Vec<String> = Vec::new();
    let mut layout: Vec<Result<usize, char>> = Vec::new();
    let mut run = String::new();
    for p in &ps {
        match p {
            Piece::Word(w) => {
                if !run.is_empty() {
                    run.push(' ');
                }
                run.push_str(w);
            }
            Piece::Punct(c) => {
                if !run.is_empty() {
                    layout.push(Ok(runs.len()));
                    runs.push(std::mem::take(&mut run));
                }
                layout.push(Err(*c));
            }
        }
    }
    if !run.is_empty() {
        layout.push(Ok(runs.len()));
        runs.push(run);
    }
    let ipa = espeak.ipa(voice, &runs).unwrap_or_else(|e| {
        tracing::warn!("Kokoro: {e}");
        vec![String::new(); runs.len()]
    });
    let mut out = String::new();
    for item in layout {
        match item {
            Ok(i) => {
                if !out.is_empty() && !out.ends_with(['(', '“', '"']) {
                    out.push(' ');
                }
                out.push_str(&map(&ipa[i]));
            }
            Err(c) if "(“".contains(c) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push(c);
            }
            Err(c) => out.push(c),
        }
    }
    out
}

fn english(lexicon: &Lexicon, espeak: Option<&Espeak>, text: &str) -> Phonemes {
    let british = lexicon.british();
    // Split digits off letters ("mp3" → "mp", "3") and write any number
    // left as words.
    let mut ps: Vec<Piece> = Vec::new();
    for p in pieces(text) {
        match p {
            Piece::Word(w) if w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| !c.is_ascii_digit()) && !w.contains('.') => {
                let mut cur = String::new();
                let mut digit = None;
                for ch in w.chars() {
                    let d = ch.is_ascii_digit();
                    if digit.is_some_and(|x| x != d) && !cur.is_empty() {
                        ps.push(Piece::Word(std::mem::take(&mut cur)));
                    }
                    if ch != '-' {
                        cur.push(ch);
                    } else if !cur.is_empty() {
                        ps.push(Piece::Word(std::mem::take(&mut cur)));
                    }
                    digit = Some(d);
                }
                if !cur.is_empty() {
                    ps.push(Piece::Word(cur));
                }
            }
            other => ps.push(other),
        }
    }
    let ps: Vec<Piece> = ps
        .into_iter()
        .flat_map(|p| match p {
            Piece::Word(w) if w.chars().all(|c| c.is_ascii_digit()) => {
                let words = w.parse::<u64>().map(super::normalize::cardinal).unwrap_or_else(|_| super::normalize::digits(&w));
                words.split([' ', '-']).filter(|s| !s.is_empty()).map(|s| Piece::Word(s.to_string())).collect::<Vec<_>>()
            }
            other => vec![other],
        })
        .collect();

    // First pass: every word the lexicon knows, without context; the rest
    // (and the context-dependent little words) wait.
    let mut phon: Vec<Option<String>> = vec![None; ps.len()];
    let mut unknown: Vec<usize> = Vec::new();
    const CONTEXTUAL: &[&str] = &["a", "A", "am", "Am", "to", "To", "in", "In", "the", "The", "THE"];
    for (i, p) in ps.iter().enumerate() {
        let Piece::Word(w) = p else { continue };
        if CONTEXTUAL.contains(&w.as_str()) {
            continue;
        }
        let at_end = !matches!(ps.get(i + 1), Some(Piece::Word(_)));
        let parts: Vec<&str> = w.split('-').filter(|s| !s.is_empty()).collect();
        let got = lexicon.word(w, if at_end { None } else { Some(false) }).or_else(|| {
            // "forty-five", "follow-up": each part, if they're all known.
            if parts.len() > 1 {
                let each: Option<Vec<String>> = parts.iter().map(|s| lexicon.word(s, Some(false))).collect();
                each.map(|v| v.join(" "))
            } else {
                None
            }
        });
        match got {
            Some(p) => phon[i] = Some(p),
            None => unknown.push(i),
        }
    }
    // The words no dictionary has: eSpeak NG, all in one call.
    let mut missing_words = Vec::new();
    if !unknown.is_empty() {
        let words: Vec<String> = unknown
            .iter()
            .map(|&i| match &ps[i] {
                Piece::Word(w) => w.replace(['’', '‘'], "'"),
                Piece::Punct(_) => String::new(),
            })
            .collect();
        missing_words = words.clone();
        let from_espeak = espeak.and_then(|e| e.ipa(if british { "en-gb" } else { "en-us" }, &words).ok());
        for (k, &i) in unknown.iter().enumerate() {
            let ipa = from_espeak.as_ref().map(|v| v[k].clone()).unwrap_or_default();
            phon[i] = if ipa.is_empty() {
                // No eSpeak (or it said nothing): letter by letter.
                lexicon.spell(&words[k])
            } else {
                Some(espeak_english(&ipa, british))
            };
        }
    }
    // Right to left: "the" / "to" / "a" by what follows.
    let mut next: Option<bool> = None;
    for i in (0..ps.len()).rev() {
        match &ps[i] {
            Piece::Punct(c) => {
                if is_stop_punct(*c) {
                    next = None;
                }
            }
            Piece::Word(w) => {
                if CONTEXTUAL.contains(&w.as_str()) {
                    phon[i] = lexicon.word(w, next);
                }
                if let Some(p) = &phon[i] {
                    next = first_sound(p).or(next);
                }
            }
        }
    }
    // Words with spaces, punctuation against the word before it.
    let mut out = String::new();
    for (i, p) in ps.iter().enumerate() {
        match p {
            Piece::Word(_) => {
                let Some(w) = phon[i].as_deref().filter(|w| !w.is_empty()) else { continue };
                if !out.is_empty() && !out.ends_with(['(', '“']) && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(w);
            }
            Piece::Punct(c) => {
                if "(“".contains(*c) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push(*c);
                } else if *c == '"' {
                    out.push('"');
                } else {
                    out.push(*c);
                }
            }
        }
    }
    let text = if british { out } else { out.replace('ɾ', "T").replace('ʔ', "t") };
    Phonemes { text, unknown: missing_words }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A few words in misaki's own spelling (from us_gold.json).
    fn lex() -> Lexicon {
        let gold = r#"{
            "A": "ˈA", "B": "bˈi", "C": "sˈi", "D": "dˈi", "I": "ˈI", "P": "pˈi", "M": "ˈɛm",
            "S": "ˈɛs", "U": "jˈu", "G": "ʤˈi", "E": "ˈi",
            "hello": "həlˈO", "world": "wˈɜɹld", "the": "ðə", "to": "tˈu", "am": "ˈæm",
            "meeting": "mˈiɾɪŋ", "meet": "mˈit", "book": "bˈʊk", "room": "ɹˈum", "want": "wˈɑnt",
            "draft": "dɹˈæft", "email": "ˈimAl", "nasa": "nˈæsə", "NASA": "nˈæsə",
            "Thursday": "θˈɜɹzdA", "walk": "wˈɔk", "kiss": "kˈɪs", "city": "sˈɪɾi",
            "read": {"DEFAULT": "ɹˈid", "VBD": "ɹˈɛd"}, "forty": "fˈɔɹɾi", "five": "fˈIv",
            "follow": "fˈɑlO", "up": "ˈʌp", "apple": "ˈæpᵊl", "one": "wˈʌn", "two": "tˈu",
            "three": "θɹˈi", "it's": "ɪts", "versus": "vˈɜɹsəs"
        }"#;
        Lexicon::from_json(false, gold, r#"{"budget": "bˈʌʤət"}"#).unwrap()
    }

    fn ph(text: &str) -> Phonemes {
        english(&lex(), None, text)
    }

    #[test]
    fn dictionary_words_and_punctuation() {
        assert_eq!(ph("Hello, world!").text, "həlˈO, wˈɜɹld!");
        assert_eq!(ph("Want the budget?").text, "wˈɑnt ðə bˈʌʤət?");
        assert!(ph("Hello world").unknown.is_empty());
        // Capitals: "Hello" is "hello", louder.
        assert_eq!(ph("HELLO").text, "həlˈO");
        // Heteronyms read their default without a tagger.
        assert_eq!(ph("read").text, "ɹˈid");
    }

    #[test]
    fn little_words_follow_the_next_sound() {
        assert_eq!(ph("the apple").text, "ði ˈæpᵊl");
        assert_eq!(ph("the book").text, "ðə bˈʊk");
        assert_eq!(ph("to email").text, "tʊ ˈimAl");
        assert_eq!(ph("to book").text, "tə bˈʊk");
        assert_eq!(ph("want to.").text, "wˈɑnt tˈu.");
        assert_eq!(ph("a book").text, "ɐ bˈʊk");
        assert_eq!(ph("I am").text, "ˌI ˈæm");
    }

    #[test]
    fn suffixes_from_the_stem() {
        assert_eq!(ph("meets").text, "mˈits");
        assert_eq!(ph("walks").text, "wˈɔks");
        assert_eq!(ph("kisses").text, "kˈɪsᵻz");
        assert_eq!(ph("cities").text, "sˈɪTiz");
        assert_eq!(ph("walked").text, "wˈɔkt");
        assert_eq!(ph("booked").text, "bˈʊkt");
        assert_eq!(ph("walking").text, "wˈɔkɪŋ");
        assert_eq!(ph("Thursday's").text, "θˈɜɹzdAz");
    }

    #[test]
    fn acronyms_letters_numbers_and_compounds() {
        // Not a word: spelled out, the last letter stressed.
        assert_eq!(ph("API").text, "ˌApˌiˈI");
        // In the dictionary as a word.
        assert_eq!(ph("NASA").text, "nˈæsə");
        assert_eq!(ph("p.m.").text, "pˌiˈɛm");
        // US English: misaki v1.0's flap "T".
        assert_eq!(ph("forty-five").text, "fˈɔɹTi fˈIv");
        assert_eq!(ph("follow-up").text, "fˈɑlO ˈʌp");
        // Digits left over are words.
        assert_eq!(ph("B2").text, "bˈi tˈu");
        assert_eq!(ph("2").text, "tˈu");
    }

    #[test]
    fn unknown_words_without_espeak_are_spelled() {
        // The test dictionary has no R or Y: nothing to say for it.
        let p = ph("Priya");
        assert_eq!(p.unknown, vec!["Priya"]);
        assert_eq!(p.text, "");
        let p = ph("Meet Bed");
        assert_eq!(p.unknown, vec!["Bed"]);
        assert_eq!(p.text, "mˈit bˌiˌidˈi");
    }

    #[test]
    fn stress_rules() {
        assert_eq!(apply_stress("həlˈO", None), "həlˈO");
        assert_eq!(apply_stress("həlˈO", Some(-2.0)), "həlO");
        assert_eq!(apply_stress("həlˈO", Some(-1.0)), "həlˌO");
        assert_eq!(apply_stress("ðə", Some(0.5)), "ðˌə");
        assert_eq!(apply_stress("ðə", Some(2.0)), "ðˈə");
        assert_eq!(apply_stress("həlˌO", Some(1.0)), "həlˈO");
        assert_eq!(apply_stress("st", Some(2.0)), "st");
    }

    #[test]
    fn espeak_ipa_in_misakis_symbols() {
        // eSpeak NG 1.52's own output for "Priya", "hello", "Kubernetes", "WhatsApp".
        assert_eq!(espeak_english("pɹˈɪjə", true), "pɹˈɪjə");
        assert_eq!(espeak_english("həlˈo^ʊ", false), "həlˈO");
        assert_eq!(espeak_english("həlˈə^ʊ", true), "həlˈQ");
        assert_eq!(espeak_english("kˌuːbɚnˈɛɾiːz", false), "kˌubəɹnˈɛTiz");
        assert_eq!(espeak_english("kæfˈe^ɪ na^ɪˈiːv", false), "kæfˈA nIˈiv");
        assert_eq!(espeak_english("t^ʃˈe^ə", true), "ʧˈɛː");
        assert_eq!(espeak_other("t^ʃa^ʊ"), "ʧW");
    }

    #[test]
    fn pieces_keep_words_and_pauses() {
        assert_eq!(
            pieces("It's 2 p.m. — ok, (yes) users' forty-five!"),
            vec![
                Piece::Word("It's".into()),
                Piece::Word("2".into()),
                Piece::Word("p.m.".into()),
                Piece::Punct('—'),
                Piece::Word("ok".into()),
                Piece::Punct(','),
                Piece::Punct('('),
                Piece::Word("yes".into()),
                Piece::Punct(')'),
                Piece::Word("users'".into()),
                Piece::Word("forty-five".into()),
                Piece::Punct('!'),
            ]
        );
    }
}
