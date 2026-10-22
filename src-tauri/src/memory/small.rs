//! **Learning with a small model** (2026-10-22): the path a Chat model that
//! can't call tools takes (Qwen2.5-1.5B, Qwen3.5-2B and 0.8B on this PC).
//! Measured, such a model given the whole chat and the changes' format kept
//! what the person only asked about ("Australia: capital is Canberra"),
//! filed "Nurse at St James's" as a person and took 10–124 s of a processor
//! a pass. **A wrong memory is worse than a missed one**, so here most of
//! the work is Yap's own, and the model only fills in one small form:
//!
//! 1. **Candidates, in Rust** ([`candidates`]): only what the person wrote
//!    about themselves or their life — a clause whose subject is them ("I",
//!    "I'm", "we"), a lasting thing of theirs ("My birthday…", "my sister
//!    Rochelle…") or someone memory has, that's a statement: not a
//!    question, a request ("Can you…", "Give me…"), something read or heard
//!    ("I read that…", "the page says…"), a want or plan ("I need a recipe
//!    for Saturday", "I'm meeting Ingrid tomorrow", "I'll try that"), a
//!    mood or a problem right now ("I'm so confused", "I can't get it to
//!    run"), a one-off thing done ("I sent…"), no secret, and while
//!    sensitive topics are off, none of those; plus how they want answers
//!    ("keep your answers short from now on") and an explicit "remember…".
//!    Leaving a job or a place ("I've left Acme") is Yap's own: the Profile
//!    facts naming it go ([`left_names`]). Most chats have no candidate at
//!    all: then there's no model call.
//! 2. **One short call a candidate** ([`messages`], [`schema`]): ~300
//!    tokens with four one-shots, under llama.cpp's JSON grammar —
//!    `{"fact": "…", "section": "skip" | "profile" | "preferences" |
//!    "people" | "projects" | "topics", "title": "…"}`. A preference needs
//!    none: the person's own words are the memory ([`preference_of`]).
//! 3. **Yap checks the answer** ([`validate`]) before `ops::apply`'s own
//!    guards (secrets, sensitive topics, other people's private lives, the
//!    person must have said it): Profile keeps the person's own clause in
//!    the third person, not the model's paraphrase ([`own_words`]); every
//!    word of a fact in the sentence ([`said_in`]); a person only named as
//!    written, with a relationship or role word next to the name in the
//!    sentence ("my sister Rochelle", "Daniel, my flatmate" — and who they
//!    are comes from those words, not the model), and only facts that say
//!    who they are ([`who_they_are`]); a project only with ownership ("I'm
//!    building", "my novel"); a topic only with a lasting interest ("I
//!    play", "I'm learning"); never a question, a request, a plan for now
//!    or a fact about the world (a capital, a price). A "name" that isn't
//!    one ("I") falls back to what the person said of themselves.
//! 4. **People are found by Yap too** ([`people_in`]): "my sister
//!    Rochelle", "my manager, Mr Okafor", whatever the model wrote.
//!
//! Pure; the calls are `learn.rs`'s.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

use super::ops::{self, Action, Change, Item, Section};

fn re(s: &str) -> Regex {
    Regex::new(s).expect("static regex")
}

/// What a candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The person about themselves or their life.
    About,
    /// How they want answers ("keep your answers short from now on").
    Preference,
    /// Told to memory: "Tell Yap what to change", an import's line,
    /// "Remember that…" — about the person by being told, so no first
    /// person is needed.
    Told,
}

/// One clause worth asking the model about.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub text: String,
    pub kind: Kind,
}

/// What a chat's (or a Tell's) words give the small path: the candidates,
/// newest last, and what the person said they left ("Acme").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub candidates: Vec<Candidate>,
    pub left: Vec<String>,
}

impl Found {
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty() && self.left.is_empty()
    }
}

// ---- sentences and clauses ---------------------------------------------------------------------

/// A text's sentences, each with whether it was asked (ends in "?").
/// Abbreviations ("Mr.", "Dr.", "St.", "e.g.") don't end one; neither does
/// a decimal point.
pub fn sentences(text: &str) -> Vec<(String, bool)> {
    const ABBREV: &[&str] = &["mr", "mrs", "ms", "dr", "st", "prof", "etc", "e.g", "i.e", "eg", "ie", "vs", "approx", "no", "jr", "sr"];
    let mut out = Vec::new();
    for line in text.replace(['\u{2019}', '\u{2018}'], "'").replace(['\u{201C}', '\u{201D}'], "\"").lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut start = 0;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if matches!(c, '.' | '!' | '?') {
                // Run of terminators ("?!", "...").
                let mut j = i;
                while j + 1 < chars.len() && matches!(chars[j + 1], '.' | '!' | '?') {
                    j += 1;
                }
                let next_is_space = j + 1 >= chars.len() || chars[j + 1].is_whitespace() || chars[j + 1] == '"';
                let before: String = chars[start..i].iter().collect();
                let last_word = before.split_whitespace().last().unwrap_or("").to_lowercase();
                let decimal = c == '.' && i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
                let abbrev = c == '.' && j == i && ABBREV.contains(&last_word.trim_matches(|x: char| !x.is_alphanumeric() && x != '.'));
                if next_is_space && !decimal && !abbrev {
                    let s: String = chars[start..i].iter().collect();
                    let asked = chars[i..=j].contains(&'?');
                    let s = s.trim().trim_matches('"').trim().to_string();
                    if !s.is_empty() {
                        out.push((s, asked));
                    }
                    start = j + 1;
                }
                i = j + 1;
                continue;
            }
            i += 1;
        }
        let rest: String = chars[start.min(chars.len())..].iter().collect();
        let rest = rest.trim().trim_matches('"').trim().to_string();
        if !rest.is_empty() {
            out.push((rest, false));
        }
    }
    out
}

/// A sentence cut where a new subject in the first person starts ("I'm a
/// nurse in Norwich and I work nights, so I need…" → three) or a question
/// does ("…called Yap, how does Tauri work?"). Each clause with whether
/// it's asked: only the sentence's last part carries its "?".
pub fn clauses(sentence: &str, asked: bool) -> Vec<(String, bool)> {
    static CUT: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)(?:,\s*(?:(?:and|but|so|because|as|since|though|although|plus|also|then)\s+)?|;\s*|\s+(?:and|but|so|because|since|though|although|plus|while|whereas)\s+|\s+-\s+|\s+—\s+)((?:i|i'm|im|i've|ive|i'd|i'll|we|we're|we've|my|our|how|what|where|when|why|which|who|can you|could you|would you|will you|any|is there|are there|do you|does|should i|is it|is that|are they)\b)")
    });
    // Each part runs from the start of its subject to where the joining
    // words (", and", " so") before the next subject begin. A name before
    // ", my flatmate," isn't a part of its own.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for c in CUT.captures_iter(sentence) {
        let (Some(m), Some(subject)) = (c.get(0), c.get(1)) else {
            continue;
        };
        let before = sentence[start..m.start()].trim();
        let only_a_name = !before.is_empty()
            && before.split_whitespace().count() <= 3
            && before.split_whitespace().all(|w| w.chars().next().is_some_and(char::is_uppercase) && !matches!(w, "I" | "I'm" | "I've"));
        if only_a_name {
            continue;
        }
        spans.push((start, m.start()));
        start = subject.start();
    }
    spans.push((start, sentence.len()));
    let parts: Vec<String> = spans
        .into_iter()
        .map(|(a, b)| sentence[a..b].trim().trim_end_matches([',', ';', '-', '—']).trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let n = parts.len();
    parts.into_iter().enumerate().map(|(i, p)| (p, asked && i + 1 == n)).collect()
}

/// Lowercased words of a clause (apostrophes kept: "i'm").
fn words_of(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Leading filler ("Oh and", "Thanks!", "Also,", "By the way") off a
/// clause, for reading how it starts.
fn without_filler(s: &str) -> String {
    static FILL: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:\s*(?:oh|ah|and|also|so|but|well|ok|okay|right|thanks|thank you|cheers|hey|hi|hello|yap|btw|by the way|anyway|actually|plus|then|just|um|erm|ugh|argh|phew|wow|hmm|honestly|basically|yeah|yes|no|great|cool|nice|lol|haha)\b[,!.]?\s*)+")
    });
    FILL.replace(s.trim(), "").trim().to_string()
}

/// The person speaking about themselves: "I", "I'm", "my", "we", "our"… as
/// a word of the clause ("me" alone isn't: "give me a recipe").
fn first_person(clause: &str) -> bool {
    words_of(clause).iter().any(|w| matches!(w.as_str(), "i" | "i'm" | "im" | "i've" | "ive" | "i'd" | "i'll" | "we" | "we're" | "we've" | "my" | "our" | "mine" | "myself"))
}

/// A question, however it ends: "what's…", "can you…", "is it worth…".
fn is_question(clause: &str, asked: bool) -> bool {
    static Q: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:what|what's|whats|how|how's|where|where's|when|when's|why|which|who|who's|whose|can|could|would|will|shall|should|is|isn't|are|aren't|am|do|does|did|don't|doesn't|have|has|was|were|any|anything|got any|is there|are there)\b")
    });
    asked || Q.is_match(&without_filler(clause))
}

/// A request to do something ("Give me…", "Please write…", "Help me…",
/// "Can you…", "Let's…").
fn is_request(clause: &str) -> bool {
    static R: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:please\s+|kindly\s+|now\s+)?(?:give|tell|write|draft|make|help|show|find|list|explain|suggest|recommend|plan|create|translate|summari[sz]e|check|compare|describe|remind|book|send|open|play|search|look|calculate|convert|fix|rewrite|edit|generate|keep|use|always|never|don't|do not|stop|start|answer|reply|respond|talk|speak|avoid|include|add|put|let's|lets|let me|could you|can you|would you|will you|please|go|get|pick|choose|sort|work out|figure out|be)\b")
    });
    R.is_match(&without_filler(clause))
}

/// How answers should be ("answers", "units", "bullet points"…).
fn answer_style(text: &str) -> bool {
    static A: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:answers?|repl(?:y|ies)|responses?|explanations?|units|metric|imperial|celsius|fahrenheit|spelling|tone|formal|informal|casual|bullet(?:s| points?)?|emojis?|jargon|format(?:ting)?|markdown|headings|code (?:samples|examples)|sources|citations)\b")
    });
    A.is_match(text)
}

/// Said to last ("from now on", "always", "in future"), and not for this
/// once ("this time", "for this one").
fn lasting_ask(text: &str) -> bool {
    static L: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:from now on|always|never|in (?:the )?future|going forward|by default|every time|in general|generally|keep (?:your|the|them|it|things)|i prefer|i'd prefer|i would prefer|i'd rather|i would rather|i like (?:my|your|short|long|brief|detailed)|i want (?:my|your) (?:answers|replies))\b")
    });
    static ONCE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?:this time|this once|for this (?:one|question|answer)|just this|right now|for now|here)\b"));
    L.is_match(text) && !ONCE.is_match(text)
}

/// Something read, heard or believed — about the world, not the person
/// ("I read that…", "I heard…", "I think…", "the page says…").
fn reported(clause: &str) -> bool {
    static SAID: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i|we)(?:\s+|'m\s+|'re\s+|'ve\s+|\s+am\s+|\s+are\s+|\s+have\s+)(?:just\s+|also\s+|once\s+|recently\s+|actually\s+|really\s+|still\s+)?(?:read|heard|saw|seen|learned|learnt|found out|was told|were told|been told|think|thought|believe|guess|suppose|reckon|wonder|wondered|feel like|doubt|bet|assume|imagine|noticed|watched|asked|forgot|can't remember|cannot remember|don't know|didn't know|do not know|did not know|don't understand|didn't understand|don't get|not sure|unsure|no idea|have no idea|got told|keep hearing|keep seeing|always wondered|never understood)\b")
    });
    static QUOTE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:according to|it says|that says|which says|(?:the|my|this|that|a|your|their|one|google's) (?:page|article|site|website|post|answer|result|results|link|video|search|guide|review|book|news|source|app|forum|thread|blog|map|maps|ad|advert) (?:says|said|claims|claimed|mentions|mentioned|shows|showed|suggests|lists)|says that|said that|on (?:wikipedia|reddit|google|youtube|the news|the internet|the web|tiktok)|wikipedia|you said|you told me|you mentioned|chatgpt|google says)\b")
    });
    SAID.is_match(&without_filler(clause)) || QUOTE.is_match(clause)
}

/// A want or a plan for now: "I need a recipe", "I want to book",
/// "I'm going to…", "I'll…", "I'm looking for…".
fn wants_or_plans(clause: &str) -> bool {
    static W: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i|we)(?:'m|'re|'ll|'d|\s+am|\s+are|\s+will|\s+would|\s+have been|\s+was)?\s+(?:really\s+|just\s+|also\s+|still\s+|now\s+)?(?:need|needs|want|wants|would like|like to|looking for|trying to|thinking|planning|going to|gonna|wondering|hoping|hope|plan|intend|could|should|might|may|must|can't decide|can't choose|deciding|considering|about to|off to|heading|meeting|seeing|visiting|cooking|making dinner|having|booking|buying|getting|ordering|hosting|throwing)\b")
    });
    // Anything they will do ("I'll try that", "we will see").
    static WILL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i|we)(?:'ll|\s+will|\s+shall|'d better|\s+had better)\b"));
    let t = without_filler(clause);
    W.is_match(&t) || WILL.is_match(&t)
}

/// How they feel or what's wrong right now ("I'm so confused about VAT",
/// "I'm tired", "I can't get my code to compile", "I'm stuck on level 3"):
/// a moment, not who they are.
fn momentary(clause: &str) -> bool {
    static NOW: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i'm|im|i am|we're|we are|i feel|i felt|i was|we were|i've been|i have been|i get|i got|i'm getting|i'm feeling)\s+(?:so\s+|really\s+|a bit\s+|a little\s+|quite\s+|very\s+|totally\s+|completely\s+|kind of\s+|kinda\s+|pretty\s+|super\s+|getting\s+|feeling\s+|still\s+|just\s+)*(?:confused|tired|bored|hungry|thirsty|stuck|lost|stressed|worried|excited|curious|annoyed|sorry|glad|happy|sad|upset|angry|cold|hot|sick|ill|unwell|busy|late|early|done|finished|ready|back|here|home|out|away|off|struggling|wondering|keen|nervous|anxious|scared|afraid|frustrated|fed up|exhausted|knackered|starving|full|free|around|on my way|running late|in a rush|in a hurry|having trouble|having a hard time|having issues|having problems|trying|stuck|overwhelmed|impressed|surprised|disappointed|not happy|not sure|unsure|new to this|about to)\b")
    });
    static TROUBLE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i|we)\s+(?:just\s+|still\s+|really\s+)?(?:can't|cannot|couldn't|can not|keep getting|keep failing|keep losing|keep forgetting|messed up|broke|broken|need help|don't understand|didn't understand)\b")
    });
    static WHERE_NOW: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i'm|im|i am|we're|we are)\s+(?:at|on|in|outside|near|by|on my way to|heading to|stuck at|stuck on|sat|sitting|waiting)\b.*\b(?:now|right now|at the moment|currently|today|tonight)\b")
    });
    let t = without_filler(clause);
    NOW.is_match(&t) || TROUBLE.is_match(&t) || WHERE_NOW.is_match(&t)
}

/// A lasting thing of the person's own, after "my": their birthday, name,
/// job, home town, favourite…, family, pet ("My birthday is 3 May"), not
/// whatever they own right now ("My code keeps crashing", "my train is
/// late").
fn lasting_possessive(text: &str) -> bool {
    static MINE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:my|our)\s+(?:(?:first|middle|full|last|home|native|first|favourite|favorite|main|day|current|new)\s+)?(?:birthday|name|surname|nickname|pronouns|job|work|role|title|profession|career|employer|company|team|department|home ?town|home|hometown|town|city|village|country|accent|language|languages|favourites?|favorites?|dog|dogs|cat|cats|pet|pets|kids|children|family|degree|major|field|hobby|hobbies|age|diet|commute|shift|shifts|rota|schedule|garden|allotment|car|bike|band|club|team)\b")
    });
    MINE.is_match(&without_filler(text))
}

/// A one-off thing done ("I sent the files to Marco", "we went to Rome",
/// "I bought a kettle"): not who they are. Starting somewhere, joining,
/// moving or marrying lasts, so those aren't.
fn one_off_event(clause: &str) -> bool {
    static EVENT: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:i|we|i've|we've|i have|we have)\s+(?:just\s+|already\s+|finally\s+|also\s+|recently\s+)?(?:sent|went|bought|saw|met|had|did|told|asked|called|visited|booked|ordered|finished|watched|came|gave|took|tried|cooked|ate|drank|paid|spent|forgot|missed|emailed|texted|rang|phoned|picked|dropped|fixed|cleaned|lost|found|won|broke|returned|posted|replied|heard|read|been to|got (?:back|home|a |an |some |the |my |our ))\b")
    });
    EVENT.is_match(&without_filler(clause))
}

/// When a clause is about a passing moment: today, tonight, this weekend,
/// tomorrow, next week, a weekday ("on Saturday", "by Friday") — but not
/// a habit ("every Thursday", "on Thursdays").
pub fn passing_time(text: &str) -> bool {
    static HABIT: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:every|each|most|some)\s+(?:day|week|weekend|morning|evening|night|month|year|monday|tuesday|wednesday|thursday|friday|saturday|sunday)s?\b|\b(?:mon|tues|wednes|thurs|fri|satur|sun)days\b")
    });
    static WHEN: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:tonight|today|tomorrow|tmrw|this (?:morning|afternoon|evening|weekend|week|month)|next (?:week|weekend|month|monday|tuesday|wednesday|thursday|friday|saturday|sunday)|right now|at the moment|later today|later on|(?:on|by|this|until|till) (?:monday|tuesday|wednesday|thursday|friday|saturday|sunday)|monday|tuesday|wednesday|thursday|friday|saturday|sunday|in an hour|in a minute|soon)\b")
    });
    let without = HABIT.replace_all(text, " ");
    WHEN.is_match(&without)
}

/// "I've left Acme", "I quit Northgate", "we moved away from York": the
/// names left (a capitalised word or a few), which Yap takes out of Profile
/// itself.
pub fn left_names(clause: &str) -> Vec<String> {
    static LEFT: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?:\b[Ii](?:'ve| have)?|\b[Ww]e(?:'ve| have)?)\s+(?:just\s+|finally\s+|now\s+)?(?:left|quit|resigned from|stopped working (?:at|for)|moved (?:away )?from|no longer work (?:at|for)|don't work (?:at|for)|do not work (?:at|for)|finished (?:at|working at))\s+((?:[A-Z][\w'&.-]*)(?:\s+(?:[A-Z][\w'&.-]*|of|and|&))*)")
    });
    LEFT.captures_iter(clause)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim_end_matches(['.', ',', '!', '\'']).trim().to_string()))
        .map(|n| n.trim_end_matches(" and").trim_end_matches(" of").to_string())
        .filter(|n| !n.is_empty() && !matches!(n.as_str(), "I" | "My" | "The"))
        .collect()
}

/// "Remember that …" / "Please remember …" inside a message: what to
/// remember.
fn remember_part(clause: &str) -> Option<String> {
    static REM: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:please\s+|can you\s+|could you\s+)?remember(?:\s+that|\s+this)?[:,]?\s+(.{3,})$"));
    let plain = without_filler(clause);
    let c = REM.captures(&plain)?;
    let what = c[1].trim().to_string();
    let first = what.split_whitespace().next().unwrap_or("").to_lowercase();
    // "Remember to…", "remember when…?" aren't memory.
    (!matches!(first.as_str(), "to" | "when" | "what" | "how" | "where" | "who" | "why" | "if")).then_some(what)
}

/// The candidates in what the person wrote in a chat (their own messages,
/// oldest first): see the module's doc. `sensitive_ok`: "Include sensitive
/// topics" is on. `known`: the people memory has (their names): a
/// sentence about one of them ("Rochelle's birthday is on the 3rd of May")
/// counts too.
pub fn candidates(user_texts: &[&str], sensitive_ok: bool, known: &[String]) -> Found {
    let mut found = Found::default();
    for text in user_texts {
        // Only what they wrote, not a paste: no quoted blocks or code, at
        // most the first 1,200 characters.
        let mut own = String::new();
        let mut fenced = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if fenced || line.trim_start().starts_with('>') {
                continue;
            }
            own.push_str(line);
            own.push('\n');
        }
        let own: String = own.chars().take(1_200).collect();
        for (sentence, asked) in sentences(&own) {
            for (clause, asked) in clauses(&sentence, asked) {
                if let Some(c) = candidate(&clause, asked, sensitive_ok, known) {
                    if !found.candidates.contains(&c) {
                        found.candidates.push(c);
                    }
                }
                for name in left_names(&clause) {
                    if !found.left.contains(&name) {
                        found.left.push(name);
                    }
                }
            }
        }
    }
    found
}

/// Is a known person the clause's subject ("Rochelle's birthday is…",
/// "Rochelle moved to Bristol")? By their first name, as its first word.
fn about_known(clause: &str, known: &[String]) -> bool {
    let Some(first) = tokens(&without_filler(clause)).into_iter().next() else {
        return false;
    };
    first.chars().next().is_some_and(char::is_uppercase) && known.iter().any(|k| ops::norm(k).split(' ').next() == Some(bare(&first).as_str()))
}

/// One clause as a candidate, or not (and why not stays in Yap: nothing is
/// logged but counts).
fn candidate(clause: &str, asked: bool, sensitive_ok: bool, known: &[String]) -> Option<Candidate> {
    let clause = ops::one_line(clause, 300);
    if clause.split_whitespace().count() < 3 || super::guard::secret_in(&clause).is_some() {
        return None;
    }
    if !sensitive_ok && super::guard::sensitive_in(&clause, "").is_some() {
        return None;
    }
    if let Some(what) = remember_part(&clause) {
        return (!is_question(&what, asked) && super::guard::secret_in(&what).is_none()).then(|| Candidate { text: what, kind: Kind::Told });
    }
    // How they want answers: a request is fine here, a one-off isn't.
    if answer_style(&clause) && lasting_ask(&clause) && !asked {
        return Some(Candidate { text: without_filler(&clause), kind: Kind::Preference });
    }
    // Whose sentence it is: the person's ("I…", "We…"), a lasting thing of
    // theirs ("My birthday…", "My novel…", "my sister Rochelle…"), or
    // someone memory has. "My train is late", "The meeting with my boss
    // went badly": no.
    let plain = without_filler(&clause);
    let starts = |re: &Regex| re.is_match(&plain);
    static PERSON: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i|i'm|im|i've|ive|i'd|we|we're|we've)\b"));
    static MINE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:my|our)\b"));
    let whose = starts(&PERSON)
        || starts(&MINE) && (lasting_possessive(&clause) || names_someone(&clause) || owns_project(&clause) || keeps_up(&clause))
        || names_someone(&clause)
        || about_known(&clause, known);
    if !(first_person(&clause) && whose || about_known(&clause, known))
        || is_question(&clause, asked)
        || is_request(&clause)
        || reported(&clause)
        || wants_or_plans(&clause)
        || momentary(&clause)
        || one_off_event(&clause) && !names_someone(&clause)
    {
        return None;
    }
    // "I've left Acme": Yap's own (`left_names`), unless there's more.
    if !left_names(&clause).is_empty() && clause.split_whitespace().count() <= 6 {
        return None;
    }
    // Today's plans, unless someone in their life is named ("My sister
    // Rochelle is coming this weekend": who she is lasts).
    if passing_time(&clause) && !names_someone(&clause) {
        return None;
    }
    Some(Candidate { text: without_filler(&clause), kind: Kind::About })
}

/// The things told to memory ("Tell Yap what to change", "Remember that…",
/// an import's lines) as candidates: every statement, no first person
/// needed; never a question, a secret or a passing request.
pub fn told(text: &str, import: bool) -> Found {
    let mut found = Found::default();
    let lines: Vec<String> = if import {
        import_lines(text)
    } else {
        sentences(text).into_iter().filter(|(_, asked)| !asked).map(|(s, _)| s).collect()
    };
    for line in lines {
        let parts = if import { import_parts(&line) } else { clauses(&line, false).into_iter().map(|(c, _)| c).collect() };
        for part in parts {
            let part = ops::one_line(part.trim().trim_start_matches(['-', '*', '•']).trim(), 300);
            if part.split_whitespace().count() < 2 || super::guard::secret_in(&part).is_some() || ops::passing(&part) {
                continue;
            }
            if is_question(&part, false) && !import {
                continue;
            }
            for name in left_names(&part) {
                if !found.left.contains(&name) {
                    found.left.push(name);
                }
            }
            // How they want answers ("Prefers concise answers…"): their words.
            let kind = if ops::looks_like_preference(&part) || answer_style(&part) && lasting_ask(&part) { Kind::Preference } else { Kind::Told };
            let c = Candidate { text: part, kind };
            if !found.candidates.contains(&c) {
                found.candidates.push(c);
            }
        }
    }
    found
}

/// An import's lines: inside its code block when it has one (the words
/// around it are the other assistant talking), one memory a line.
fn import_lines(text: &str) -> Vec<String> {
    let text = text.replace('\r', "");
    let inside: Option<String> = {
        let mut parts = text.split("```");
        parts.next();
        parts.next().map(|block| block.lines().skip_while(|l| !l.trim().is_empty() && l.split_whitespace().count() == 1 && !l.contains(' ')).collect::<Vec<_>>().join("\n"))
    };
    let body = inside.filter(|b| !b.trim().is_empty()).unwrap_or(text);
    body.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !assistant_talk(l)).collect()
}

/// A line where the other assistant talks about its answer, not the person
/// ("That's everything I have stored.", "Here's what I remember:",
/// "Preferences:"). Pure.
fn assistant_talk(line: &str) -> bool {
    static TALK: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:i have stored|i've stored|i have saved|i've saved|i remember|i've learned|i have learned|i know about you|that's everything|that is everything|that's all|that is all|here's what|here is what|here are|let me know|hope this helps|i don't have|i do not have|my memory|saved memories|in my memory|from our (?:past )?(?:chats|conversations)|as an ai)\b")
    });
    line.trim_end().ends_with(':') || TALK.is_match(line)
}

/// An import line with several memories ("Name is Kit, lives in Norwich, UK";
/// "Works as a developer, building an app called Yap") in parts, cut only
/// where a new verb starts so a part never loses its subject.
fn import_parts(line: &str) -> Vec<String> {
    static CUT: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)(?:,\s*(?:and\s+)?|;\s*|\s+and\s+)((?:lives|works|has|plays|likes|loves|enjoys|prefers|uses|speaks|studies|owns|runs|is building|is making|is writing|building|making|writing|learning|working on|developing)\b)")
    });
    // Each part from its verb to the joining words before the next.
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for c in CUT.captures_iter(line) {
        if let (Some(m), Some(verb)) = (c.get(0), c.get(1)) {
            spans.push((start, m.start()));
            start = verb.start();
        }
    }
    spans.push((start, line.len()));
    spans.into_iter().map(|(a, b)| line[a..b].trim().trim_end_matches([',', ';']).trim().to_string()).filter(|p| !p.is_empty()).collect()
}

// ---- people, projects, topics: what the sentence must say --------------------------------------

/// Who someone is to the person (after "my"): family, friends, work, pets.
const RELATION: &[&str] = &[
    "sister", "brother", "mum", "mom", "mother", "dad", "father", "wife", "husband", "partner", "son", "daughter",
    "friend", "colleague", "coworker", "co-worker", "boss", "manager", "cousin", "aunt", "auntie", "uncle", "grandma",
    "grandmother", "nan", "nana", "gran", "granny", "grandad", "granddad", "grandfather", "grandpa", "niece", "nephew",
    "girlfriend", "boyfriend", "fiancé", "fiancée", "fiance", "fiancee", "flatmate", "housemate", "roommate", "neighbour",
    "neighbor", "teacher", "doctor", "gp", "dog", "cat", "pet", "puppy", "kitten", "horse", "rabbit", "stepmother",
    "stepfather", "stepmum", "stepdad", "stepson", "stepdaughter", "child", "kid", "baby", "twin", "client", "customer",
    "mentor", "landlord", "landlady", "dentist", "classmate", "teammate", "tutor", "coach", "vet", "parent", "supervisor",
    "lead", "assistant", "editor", "agent", "accountant", "solicitor", "lawyer", "babysitter", "nanny", "childminder",
    "trainer", "physio", "therapist", "student", "pupil", "apprentice", "intern", "director", "ceo", "founder",
    "cofounder", "co-founder", "godmother", "godfather", "godson", "goddaughter", "in-law", "brother-in-law",
    "sister-in-law", "mother-in-law", "father-in-law", "ex", "bestie", "mate", "pal", "buddy",
];

/// Words that may sit between "my" and the relation ("my best friend", "my
/// older sister", "my line manager").
const RELATION_ADJ: &[&str] = &[
    "best", "older", "younger", "little", "big", "eldest", "youngest", "oldest", "twin", "new", "old", "former", "ex",
    "step", "half", "line", "team", "closest", "close", "good", "work", "uni", "school", "childhood", "kids'", "son's",
    "daughter's", "product", "project", "practice", "head", "senior", "junior", "lovely", "baby", "great",
];

/// Family words that name someone by themselves ("my mum").
const KIN_NAMES: &[&str] = &["mum", "mom", "mother", "dad", "father", "nan", "nana", "gran", "granny", "grandma", "grandad", "granddad", "grandpa"];

/// A clause's tokens with their original case, and punctuation kept as
/// tokens (",", "(").
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.replace('\u{2019}', "'").chars() {
        if c.is_alphanumeric() || c == '\'' || c == '-' {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if matches!(c, ',' | '(' | ')' | ':' | ';' | '.' | '!' | '?') {
                out.push(c.to_string());
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn bare(w: &str) -> String {
    w.to_lowercase().trim_end_matches("'s").trim_end_matches('\'').to_string()
}

/// Who `name` is to the person, from the words next to it in `text` — "my
/// sister Rochelle", "my best friend Imogen", "my manager, Mr Okafor",
/// "Daniel, my flatmate", "Rochelle is my sister", "my sister's name is
/// Rochelle": "Sister", "Best friend", "Manager", "Flatmate". `None`
/// without one (a name in passing). Pure.
pub fn relation_of(text: &str, name: &str) -> Option<String> {
    let toks = tokens(text);
    let name_toks = tokens(name);
    let first = name_toks.first()?;
    if KIN_NAMES.contains(&bare(first).as_str()) && name_toks.len() == 1 {
        // "my mum": the word is both.
        let at = toks.iter().position(|t| bare(t) == bare(first))?;
        return (at > 0 && matches!(bare(&toks[at - 1]).as_str(), "my" | "our")).then(|| capitalise(&bare(first)));
    }
    // The name as written, with its capital.
    let at = toks.iter().position(|t| t == first || bare(t) == bare(first) && t.chars().next().is_some_and(char::is_uppercase))?;
    if !toks[at].chars().next().is_some_and(char::is_uppercase) {
        return None;
    }
    let end = at + name_toks.len();
    // Before: "my [adj…] relation [, | 's name is | called | named]" within
    // a few words of the name — or, at the start of what's said, "has a
    // sister called…" / "I have a brother named…" (an import's line).
    let from = at.saturating_sub(6);
    let owner = |i: usize| {
        let t = bare(&toks[i]);
        matches!(t.as_str(), "my" | "our")
            || matches!(t.as_str(), "a" | "an" | "two" | "three")
                && i <= 3
                && i > 0
                && matches!(bare(&toks[i - 1]).as_str(), "has" | "have" | "got" | "i've")
                && toks[..i - 1].iter().all(|w| matches!(bare(w).as_str(), "i" | "i've" | "we" | "we've" | "also" | "and" | "user" | "the"))
    };
    if let Some(my) = (from..at).rev().find(|&i| owner(i)) {
        let my = from + my;
        let between: Vec<String> = toks[my + 1..at].iter().map(|t| bare(t)).collect();
        if let Some(r) = relation_phrase(&between) {
            return Some(r);
        }
    }
    // After: "Name, my relation", "Name (my relation)", "Name is my
    // relation", "Name, who is my relation".
    let after: Vec<String> = toks.iter().skip(end).take(6).map(|t| bare(t)).collect();
    let skip = after.iter().take_while(|t| matches!(t.as_str(), "," | "(" | "is" | "was" | "who" | "-" | "s")).count();
    if after.get(skip).is_some_and(|t| t == "my" || t == "our") {
        let rest: Vec<String> = after[skip + 1..].iter().take_while(|t| !matches!(t.as_str(), "," | ")" | ":" | ";")).cloned().collect();
        // "my sister and…": the relation is the words up to the first non-relation word after it.
        for n in (1..=rest.len()).rev() {
            if let Some(r) = relation_phrase(&rest[..n]) {
                return Some(r);
            }
        }
    }
    None
}

/// "best friend", "sister", "line manager" (adjectives then one relation
/// word, possibly "'s name is" / "called" after it) → "Best friend".
fn relation_phrase(words: &[String]) -> Option<String> {
    let words: Vec<&str> = words.iter().map(String::as_str).filter(|w| !matches!(*w, "," | "(" | ")")).collect();
    // Trailing "name is", "called", "named", "is", "was", and a title the
    // name starts with ("my manager, Mr Okafor").
    let mut end = words.len();
    while end > 0
        && matches!(words[end - 1], "name" | "is" | "was" | "called" | "named" | "s" | "mr" | "mrs" | "ms" | "miss" | "dr" | "prof" | "sir" | "dame")
    {
        end -= 1;
    }
    let words = &words[..end];
    let (last, adj) = words.split_last()?;
    if !RELATION.contains(last) || !adj.iter().all(|w| RELATION_ADJ.contains(w)) || adj.len() > 2 {
        return None;
    }
    Some(capitalise(&words.join(" ")))
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Words that start with a capital but aren't a name.
fn not_a_name(w: &str) -> bool {
    const NOT: &[&str] = &[
        "i", "i'm", "i've", "i'd", "i'll", "the", "a", "an", "my", "our", "monday", "tuesday", "wednesday", "thursday",
        "friday", "saturday", "sunday", "january", "february", "march", "april", "june", "july", "august", "september",
        "october", "november", "december", "christmas", "easter", "and", "but", "so", "he", "she", "they", "it", "we",
        "who", "what", "is", "was", "has", "had", "this", "that", "there", "here", "yes", "no", "ok", "okay", "thanks",
        "yesterday", "today", "tomorrow", "tonight", "also", "then", "sadly", "luckily", "honestly", "actually",
        "apparently", "unfortunately", "hopefully", "recently", "lately", "sometimes", "often", "anyway", "plus",
        "well", "oh", "hey", "hi", "hello", "sorry", "please", "last", "next", "every", "when", "if", "because",
    ];
    NOT.contains(&bare(w).as_str())
}

/// A name's honorific, which belongs to it ("Mr Okafor").
fn honorific(w: &str) -> bool {
    matches!(bare(w).trim_end_matches('.'), "mr" | "mrs" | "ms" | "miss" | "dr" | "prof" | "sir" | "dame")
}

/// **The people a sentence names with who they are to the person**, found
/// by Yap from their own words, without a model: "my sister Rochelle", "my
/// manager, Mr Okafor", "my sister's name is Rochelle", "Daniel, my
/// flatmate", "Rochelle is my sister", "has a brother called Tobias" (an
/// import's line). `(name, who they are)`. Pure.
pub fn people_in(text: &str) -> Vec<(String, String)> {
    let toks = tokens(text);
    let mut out: Vec<(String, String)> = Vec::new();
    let is_name = |t: &str| t.chars().next().is_some_and(char::is_uppercase) && !not_a_name(t) && t.chars().any(char::is_alphabetic);
    // A run of name words from `i` (an honorific, then up to three capitalised words).
    let name_at = |i: usize| -> Option<(String, usize)> {
        let mut j = i;
        let mut words = Vec::new();
        if j < toks.len() && honorific(&toks[j]) {
            words.push(toks[j].trim_end_matches('.').to_string());
            j += 1;
            // "Mr. Okafor"
            if toks.get(j).is_some_and(|t| t == ".") {
                j += 1;
            }
        }
        while j < toks.len() && words.len() < 4 && is_name(&toks[j]) && !honorific(&toks[j]) {
            words.push(toks[j].trim_end_matches("'s").to_string());
            if toks[j].ends_with("'s") {
                j += 1;
                break;
            }
            j += 1;
        }
        let named = words.iter().any(|w| !honorific(w));
        named.then(|| (words.join(" "), j))
    };
    let mut push = |name: String, rel: String| {
        if ops::looks_like_a_name(&name) && !out.iter().any(|(n, _)| ops::norm(n) == ops::norm(&name)) {
            out.push((name, rel));
        }
    };
    // "my [adj…] relation [, | 's name is | called | named | is] Name".
    for i in 0..toks.len() {
        let t = bare(&toks[i]);
        let owner = matches!(t.as_str(), "my" | "our")
            || matches!(t.as_str(), "a" | "an")
                && (1..=3).contains(&i)
                && matches!(bare(&toks[i - 1]).as_str(), "has" | "have" | "got" | "i've")
                && toks[..i - 1].iter().all(|w| matches!(bare(w).as_str(), "i" | "i've" | "we" | "we've" | "also" | "and" | "user" | "the"));
        if !owner {
            continue;
        }
        // Up to two adjectives, then the relation.
        let mut j = i + 1;
        let mut phrase: Vec<String> = Vec::new();
        while j < toks.len() && phrase.len() < 3 {
            let w = bare(&toks[j]);
            phrase.push(w.clone());
            j += 1;
            if RELATION.contains(&w.as_str()) {
                break;
            }
            if !RELATION_ADJ.contains(&w.as_str()) {
                break;
            }
        }
        let Some(rel) = relation_phrase(&phrase) else {
            continue;
        };
        // "'s name is", "called", "named", "is", ",".
        while j < toks.len() && matches!(bare(&toks[j]).as_str(), "," | "name" | "is" | "was" | "called" | "named" | "(") {
            j += 1;
        }
        if let Some((name, _)) = name_at(j) {
            push(name, rel);
        }
    }
    // "Name, my relation" / "Name (my relation)" / "Name is my relation".
    for i in 0..toks.len() {
        if i > 0 && is_name(&toks[i - 1]) || !is_name(&toks[i]) && !honorific(&toks[i]) {
            continue;
        }
        let Some((name, end)) = name_at(i) else {
            continue;
        };
        let mut j = end;
        while j < toks.len() && matches!(bare(&toks[j]).as_str(), "," | "(" | "is" | "was" | "who") {
            j += 1;
        }
        if j < toks.len() && matches!(bare(&toks[j]).as_str(), "my" | "our") {
            let rest: Vec<String> = toks[j + 1..].iter().take(3).map(|t| bare(t)).take_while(|t| !matches!(t.as_str(), "," | ")" | ":" | ";")).collect();
            if let Some(n) = (1..=rest.len()).rev().find(|&n| relation_phrase(&rest[..n]).is_some()) {
                // "Yesterday, my sister Rochelle…": the relation is Rochelle's.
                let next = toks.get(j + 1 + n);
                if next.is_none_or(|t| !is_name(t) && !honorific(t)) {
                    if let Some(rel) = relation_phrase(&rest[..n]) {
                        push(name, rel);
                    }
                }
            }
        }
    }
    out
}

/// The changes for the people [`people_in`] finds in a candidate: each
/// added (or, if memory has them, kept) with who they are. Pure.
pub fn people_changes(c: &Candidate) -> Vec<Change> {
    people_in(&c.text)
        .into_iter()
        .map(|(name, rel)| Change { action: Action::Add, id: None, section: Some(Section::People), name, about: Some(rel), facts: vec![], remove_facts: vec![], say: String::new() })
        .collect()
}

/// **A project the person owns, named in their words** ("I'm writing a
/// fantasy novel called The Salt Road", "is writing a cookbook
/// called Small Plates"): `(name, what it is)`, found by Yap. Needs ownership and a
/// capitalised name after "called" / "named" / "titled". Pure.
pub fn project_in(text: &str) -> Option<(String, String)> {
    static NAMED: LazyLock<Regex> = LazyLock::new(|| {
        re(r"\b(?:an?|my|our)\s+((?:[a-z][\w-]*\s+){0,3}[a-z][\w-]*)\s+(?:called|named|titled)\s+((?:The\s+|A\s+)?[A-Z0-9][\w'&.-]*(?:\s+(?:[A-Z0-9][\w'&.-]*|of|and|the|&))*)")
    });
    if !owns_project(text) {
        return None;
    }
    let c = NAMED.captures(text)?;
    let what = c[1].trim();
    let name = c[2].trim().trim_end_matches(" of").trim_end_matches(" and").trim_end_matches(" the").trim_end_matches(['.', '\'']).to_string();
    // "a sister called…" is a person, not a project.
    if RELATION.contains(&what.split_whitespace().last().unwrap_or("")) || name.is_empty() {
        return None;
    }
    Some((name, capitalise(what)))
}

/// The change for the project [`project_in`] finds in a candidate.
pub fn project_changes(c: &Candidate) -> Vec<Change> {
    project_in(&c.text)
        .map(|(name, what)| Change { action: Action::Add, id: None, section: Some(Section::Projects), name, about: Some(what), facts: vec![], remove_facts: vec![], say: String::new() })
        .into_iter()
        .collect()
}

/// **A birthday, in the person's words** ("Rochelle's birthday is on the 3rd
/// of May", "my sister Rochelle's birthday is May 3"): `(whose, "Birthday 3
/// May")` — whose is the name before "'s birthday" (`None` for "my
/// birthday": the person's own). Pure.
pub fn birthday_in(text: &str) -> Option<(Option<String>, String)> {
    static BDAY: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:(my)|((?-i:[A-Z][a-z]+))'s)\s+birthday\s+is\s+(?:on\s+)?(?:the\s+)?(?:(\d{1,2})(?:st|nd|rd|th)?\s+(?:of\s+)?(january|february|march|april|may|june|july|august|september|october|november|december)|(january|february|march|april|may|june|july|august|september|october|november|december)\s+(?:the\s+)?(\d{1,2})(?:st|nd|rd|th)?)\b")
    });
    let c = BDAY.captures(text)?;
    let day: u32 = c.get(3).or_else(|| c.get(6))?.as_str().parse().ok()?;
    let month = capitalise(&c.get(4).or_else(|| c.get(5))?.as_str().to_lowercase());
    if !(1..=31).contains(&day) {
        return None;
    }
    let whose = if c.get(1).is_some() { None } else { c.get(2).map(|m| m.as_str().to_string()) };
    Some((whose, format!("Birthday {day} {month}")))
}

/// The change for a birthday [`birthday_in`] finds: the person's own
/// (Profile), or someone memory has, or someone the sentence names with who
/// they are. Pure.
pub fn birthday_changes(c: &Candidate, items: &[Item]) -> Vec<Change> {
    let Some((whose, fact)) = birthday_in(&c.text) else {
        return vec![];
    };
    let add = |section: Section, name: String| Change { action: Action::Add, id: None, section: Some(section), name, about: None, facts: vec![fact.clone()], remove_facts: vec![], say: String::new() };
    match whose {
        None => vec![add(Section::Profile, String::new())],
        Some(name) => {
            let known = items.iter().find(|i| i.section == Section::People && ops::same_thing(Section::People, &i.title, &name));
            let named = people_in(&c.text).into_iter().find(|(n, _)| ops::same_thing(Section::People, n, &name));
            match (known, named) {
                (Some(k), _) => vec![add(Section::People, k.title.clone())],
                (None, Some((n, _))) => vec![add(Section::People, n)],
                _ => vec![],
            }
        }
    }
}

/// **How they want answers, in their own words** — a preference candidate
/// needs no model: "Please keep your answers short from now on" → "Keep
/// your answers short"; "always use metric units" → "Always use metric
/// units". Pure.
pub fn preference_of(c: &Candidate) -> Option<Change> {
    static LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:(?:please|kindly|can you|could you|would you|will you|yap|from now on|in future|in the future|going forward|asked (?:me|you) to)[,]?\s+)+"));
    static TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[,]?\s+(?:from now on|in future|in the future|going forward|please|thanks|thank you)\s*[.!]*$"));
    if c.kind != Kind::Preference {
        return None;
    }
    let t = without_filler(&c.text);
    let t = LEAD.replace(&t, "");
    let t = TAIL.replace(&t, "");
    // "I prefer short answers" → "Prefers short answers".
    let fact = as_memory(&t);
    (fact.split_whitespace().count() >= 2 && answer_style(&fact)).then(|| Change {
        action: Action::Add,
        id: None,
        section: Some(Section::Preferences),
        name: String::new(),
        about: None,
        facts: vec![fact],
        remove_facts: vec![],
        say: String::new(),
    })
}

/// Does the candidate need the model? A preference doesn't (its own words
/// are the memory).
pub fn needs_model(c: &Candidate) -> bool {
    c.kind != Kind::Preference
}

/// Does a clause name someone with who they are to the person ("my sister
/// Rochelle")? (A clause about them isn't "passing" for its date.)
fn names_someone(clause: &str) -> bool {
    tokens(clause).iter().filter(|t| t.chars().next().is_some_and(char::is_uppercase) && t.as_str() != "I" && !t.starts_with("I'")).any(|t| relation_of(clause, t).is_some())
}

/// The person owns or makes it: "I'm building/writing/making…", "I run…",
/// "my project/app/novel…" (and, told, a line that starts "building…").
pub fn owns_project(text: &str) -> bool {
    static OWN: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:i'?m|i am|we'?re|we are)\s+(?:currently\s+|now\s+|still\s+|slowly\s+|also\s+)?(?:building|making|writing|developing|designing|creating|working on|coding|programming|running|launching|organi[sz]ing|recording|producing|restoring|renovating|starting|composing|directing|editing|self-publishing|publishing)\b|\b(?:i|we)\s+(?:built|made|wrote|created|developed|designed|run|maintain|founded|started|launched|own|publish|host|organi[sz]e)\b|\b(?:my|our)\s+(?:own\s+|side\s+|pet\s+|new\s+|latest\s+|current\s+|first\s+|second\s+|little\s+|hobby\s+|open[- ]source\s+)?(?:project|app|game|website|site|startup|company|business|book|novel|podcast|channel|band|blog|shop|library|tool|plugin|mod|thesis|dissertation|album|film|script|screenplay|newsletter|course|garden|allotment)\b|^(?:is\s+)?(?:building|making|writing|developing|creating|working on|running)\b")
    });
    OWN.is_match(text)
}

/// A lasting interest: "I play…", "I love…", "I'm learning…", "I'm into…",
/// "my hobby…" (and, told, a line that starts "plays…").
pub fn keeps_up(text: &str) -> bool {
    static INTEREST: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:i|we)\s+(?:really\s+|also\s+|still\s+|regularly\s+|often\s+|usually\s+)?(?:love|enjoy|play|collect|follow|support|practi[cs]e|study|train|climb|swim|paint|knit|sew|bake|fish|cycle|ride|run|hike|sing|dance|draw|read|watch|listen to|go to|go)\b|\bi'?m\s+(?:really\s+|also\s+|still\s+|a big\s+|a huge\s+)?(?:into|learning|studying|training for|a (?:big |huge )?fan of|obsessed with|passionate about|interested in)\b|\bi(?:'ve| have)\s+(?:just\s+)?(?:been|started)\s+(?:learning|playing|doing|collecting|studying|practi[cs]ing|training)\b|\bmy (?:hobby|hobbies|favourite|favorite|passion|thing)\b|^(?:plays|loves|enjoys|collects|follows|supports|practi[cs]es|studies|is learning|learning|is into|into)\b")
    });
    INTEREST.is_match(text)
}

/// Something true of the world rather than of the person: a capital, a
/// population, a price, a measurement, "X is the …". Pure.
pub fn about_the_world(fact: &str) -> bool {
    static WORLD: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)\b(?:capital|population|is located|located in|was (?:born|founded|built|written|invented|discovered|released|published)|largest|smallest|tallest|longest|highest|deepest|million|billion|per cent|percent|degrees|visible from|costs?|prices?|priced|cheapest|tablespoons?|teaspoons?|calories|recipe|weather|forecast|timetable|time ?zone)\b|[£$€]\s?\d|\d\s?(?:km|kg|mph|miles|metres|meters|feet|%)\b")
    });
    static SUBJECT: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:The\s+)?[A-Z][\w'-]*(?:\s+[A-Z][\w'-]*)*\s+(?:is|are|was|were|has|have)\b"));
    let first = bare(fact.split_whitespace().next().unwrap_or(""));
    let own = RELATION.contains(&first.as_str()) || matches!(first.as_str(), "name" | "birthday" | "job" | "home" | "surname" | "nickname");
    WORLD.is_match(fact) || SUBJECT.is_match(fact) && !own
}

/// A fact that's really a question ("Where to eat in York?", "How to
/// outline a chapter").
fn fact_is_question(fact: &str) -> bool {
    static Q: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:what|what's|how|how to|where|where to|when|why|which|who|whose|can you|could you|would you|should i|is it|is there|are there|do you|does it|any ideas)\b")
    });
    fact.trim_end().ends_with('?') || Q.is_match(fact)
}

/// A fact that's really a request to Yap's AI ("Give me a recipe", "Help
/// with an outline").
fn fact_is_ask(fact: &str) -> bool {
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:please|can you|could you|would you|give me|tell me|help me|help with|show me|let's|write me|find me|recommend|suggest|draft|explain|asks? (?:yap|the ai|you) to|needs? help|wants? help|wants? (?:a|an|some|to know|ideas|tips|advice)|looking for|asked (?:for|about|how|what))\b")
    });
    ASK.is_match(fact)
}

/// The known person a told sentence is about, by its first word ("Rochelle
/// moved to Bristol"): such a fact is theirs, not the person's.
fn subject_person<'a>(c: &Candidate, items: &'a [Item]) -> Option<&'a Item> {
    if first_person(&c.text) {
        return None;
    }
    let first = tokens(&c.text).into_iter().next()?;
    if !first.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }
    let first = bare(&first);
    items.iter().find(|i| i.section == Section::People && ops::norm(&i.title).split(' ').next() == Some(first.as_str()))
}

/// A change, not a state: "Left Acme", "Quit…", "No longer…", "Used to…".
fn a_change(fact: &str) -> bool {
    static CHANGE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:has\s+|have\s+|just\s+)?(?:left|quit|resigned|stopped|no longer|used to|formerly|former|previously|moved away)\b"));
    CHANGE.is_match(fact)
}

// ---- the call ----------------------------------------------------------------------------------

/// The extraction's system prompt (≈ 140 tokens). Starts like every pass's
/// ("You keep Yap's memory of the user"), which the e2e suite's fake model
/// knows them by.
pub const SYSTEM: &str = "You keep Yap's memory of the user. Read one thing the user said. If it tells you something lasting about the user, write one memory, else skip.
profile: their work, home, pets, what they do (fact: a few words).
preferences: how they want answers.
people: someone in their life (title: the name as written; fact: something lasting about them, or \"\").
projects: something they make or build (title: its name; fact: what it is).
topics: a hobby they keep up (title: its name).
Skip questions, requests, plans for now and facts about the world. JSON only.";

/// Four one-shots, as earlier turns (a small model follows those best),
/// their keys in the order the grammar makes the model write them
/// (serde_json's, alphabetical). Their names never reach memory: every
/// title must be in the sentence.
const SHOTS: &[(&str, &str)] = &[
    ("My brother Tom lives in Glasgow, he's visiting this weekend", r#"{"fact":"Lives in Glasgow","section":"people","title":"Tom"}"#),
    ("I work night shifts at the hospital", r#"{"fact":"Works night shifts at the hospital","section":"profile","title":""}"#),
    ("I'm making a board game called Hexfall", r#"{"fact":"Board game","section":"projects","title":"Hexfall"}"#),
    ("I read that Paris has two million people", r#"{"fact":"","section":"skip","title":""}"#),
];

/// The messages of one extraction call for `c`.
pub fn messages(c: &Candidate) -> Value {
    let mut m = vec![json!({ "role": "system", "content": SYSTEM })];
    for (user, reply) in SHOTS {
        m.push(json!({ "role": "user", "content": user }));
        m.push(json!({ "role": "assistant", "content": reply }));
    }
    m.push(json!({ "role": "user", "content": c.text }));
    Value::Array(m)
}

/// The extraction's JSON schema (llama.cpp makes it a grammar): the section
/// first, so the model decides before it writes.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "section": { "type": "string", "enum": ["skip", "profile", "preferences", "people", "projects", "topics"] },
            "title": { "type": "string", "maxLength": 60 },
            "fact": { "type": "string", "maxLength": 100 }
        },
        "required": ["section", "title", "fact"],
        "additionalProperties": false
    })
}

/// What the model wrote for a candidate.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extracted {
    /// `None`: skip (or unreadable).
    pub section: Option<Section>,
    pub title: String,
    pub fact: String,
}

/// The model's reply, leniently: `{"section", "title", "fact"}` (also
/// "name" / "about" / "facts"), `{"skip": true}`, or nothing readable (=
/// skip). Pure.
pub fn parse(reply: &str) -> Extracted {
    let t = reply.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: Value = match serde_json::from_str(t) {
        Ok(v) => v,
        Err(_) => match (t.find('{'), t.rfind('}')) {
            (Some(a), Some(b)) if a < b => serde_json::from_str(&t[a..=b]).unwrap_or(Value::Null),
            _ => Value::Null,
        },
    };
    // `{"changes": [...]}` (the full pass's shape): its first change.
    let v = match v.get("changes").and_then(Value::as_array) {
        Some(a) => a.first().cloned().unwrap_or(Value::Null),
        None => v,
    };
    if v["skip"] == json!(true) {
        return Extracted::default();
    }
    let text = |keys: &[&str]| keys.iter().find_map(|k| v[*k].as_str().map(|s| s.trim().to_string())).unwrap_or_default();
    let section = v["section"].as_str().filter(|s| !s.eq_ignore_ascii_case("skip") && !s.eq_ignore_ascii_case("none")).and_then(Section::parse);
    let mut fact = text(&["fact", "about"]);
    if fact.is_empty() {
        fact = v["facts"].as_array().and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("").trim().to_string();
    }
    Extracted { section, title: text(&["title", "name"]), fact }
}

// ---- checking what the model wrote -------------------------------------------------------------

/// Words a fact may add to what was said: how memory phrases things, small
/// joining words, and a few generic nouns ("company", "job").
const PHRASING: &[&str] = &[
    "user", "users", "works", "work", "working", "lives", "live", "living", "has", "have", "had", "called", "named",
    "name", "plays", "likes", "loves", "enjoys", "prefers", "wants", "answers", "answer", "replies", "short", "brief",
    "concise", "uses", "is", "are", "was", "the", "and", "for", "with", "their", "them", "they", "from", "who", "been",
    "being", "currently", "regularly", "also", "into", "onto", "about", "that", "this", "which", "job", "company",
    "team", "place", "city", "town", "home", "house", "person", "people", "role", "position", "hobby", "interest",
    "project", "app", "game", "now", "new", "always", "usually", "often", "every", "each", "own", "goes", "does",
];

/// Is every word of `fact` that memory doesn't add itself ([`PHRASING`]) in
/// `said` (or the same word with another ending)? Stricter than
/// `ops::grounded` (one word): Qwen2.5-1.5B wrote "Works night shifts at
/// the hospital" for "I work night shifts", from a one-shot. Pure.
pub fn said_in(fact: &str, said: &str) -> bool {
    let src: Vec<String> = ops::norm(said).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
    let has = |w: &str| {
        src.iter().any(|s| {
            s == w || (w.len().min(s.len()) >= 3 && (s.starts_with(w) || w.starts_with(s.as_str())))
                || (s.len() >= 5 && w.len() >= 5 && s.is_char_boundary(4) && w.is_char_boundary(4) && s[..4] == w[..4])
        })
    };
    ops::norm(fact).split(' ').filter(|w| w.chars().count() >= 3 && !PHRASING.contains(w)).all(has)
}

/// A person's fact without their name in front ("Imogen is a vet in
/// Bristol" → "Vet in Bristol"); `None` when what's left still names them
/// or restates who they are ("Has a sister called Rochelle"). Pure.
fn their_fact(fact: &str, name: &str) -> Option<String> {
    let mut f = fact.trim().to_string();
    for w in name.split_whitespace() {
        if let Some(rest) = f.strip_prefix(w) {
            f = rest.trim_start_matches("'s").trim().to_string();
        }
    }
    static IS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:is|was)\s+(?:an?\s+)?"));
    let f = ops::clean_fact(&IS.replace(&f, ""));
    let n = format!(" {} ", ops::norm(&f));
    let names_them = name.split_whitespace().filter(|w| w.chars().count() >= 3).any(|w| n.contains(&format!(" {} ", ops::norm(w))));
    let restates = n.contains(" called ") || n.contains(" named ") || n.contains(" name is ");
    (!f.is_empty() && !names_them && !restates).then_some(f)
}

/// A fact that says who someone is or what lasts about them: where they
/// live, their work, a birthday, what they love — a noun phrase ("Vet in
/// Bristol") or one of those verbs; not what they keep doing or did ("Leaves
/// the heating on all day", Qwen2.5-1.5B, 2026-10-22). Pure.
fn who_they_are(fact: &str) -> bool {
    static OK: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:lives|works|is|was born|born|birthday|studies|plays|loves|likes|enjoys|has (?:a|an|two|three|four|kids|children)|speaks|grew up|comes from|married|retired|teaches|runs|owns|coaches|trains|volunteers|supports|manages|leads|founded|moved to|based in|from)\b")
    });
    static VERBISH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\w+(?:s|ed|ing)\b"));
    let f = fact.trim();
    OK.is_match(f) || !VERBISH.is_match(f) && !f.to_lowercase().starts_with("keep") && !f.to_lowercase().starts_with("always") && !f.to_lowercase().starts_with("never")
}

/// What the person said about themselves, as memory keeps it: their own
/// clause in the third person ("I'm a nurse at St James's in Norwich" →
/// "Nurse at St James's in Norwich"). A small model's paraphrase can lose
/// the point ("Works at St James's in Norwich", no nurse) or add to it.
/// `None` when it's too long to be one fact.
fn own_words(c: &Candidate) -> Option<String> {
    static TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[,;]?\s+(?:by the way|btw|lol|haha|though|thanks|thank you|if that helps|for context|fyi)\s*[.!]*$"));
    let t = without_filler(&c.text);
    let t = TAIL.replace(&t, "").to_string();
    let f = (t.chars().count() <= ops::MAX_FACT).then(|| as_memory(&t)).filter(|f| !f.is_empty())?;
    // "I'm Declan" → "Name is Declan".
    let words: Vec<&str> = f.split_whitespace().collect();
    let a_name = words.len() <= 3 && words.iter().all(|w| w.chars().next().is_some_and(char::is_uppercase)) && ops::looks_like_a_name(&f);
    Some(if a_name { format!("Name is {f}") } else { f })
}

/// Someone else starts the sentence: a name ("Daniel, my flatmate, keeps…")
/// or "my/our" + who they are ("My flatmate keeps…") — but "my dog Beans",
/// a pet, is the person's own.
fn someone_else_first(text: &str) -> bool {
    let toks = tokens(&without_filler(text));
    let Some(first) = toks.first() else {
        return false;
    };
    if first.chars().next().is_some_and(char::is_uppercase) && !not_a_name(first) && !honorific(first) {
        return true;
    }
    if matches!(bare(first).as_str(), "my" | "our") {
        let pets = ["dog", "cat", "pet", "puppy", "kitten", "horse", "rabbit"];
        let rel: Vec<String> = toks[1..].iter().take(3).map(|t| bare(t)).collect();
        return (1..=rel.len()).any(|n| relation_phrase(&rel[..n]).is_some() && !pets.contains(&rel[n - 1].as_str()));
    }
    false
}

/// The person themselves is the clause's subject ("I play chess…", "I'm a
/// nurse…"; told: a line starting with what they do, "Lives in Norwich").
fn the_person_speaks(c: &Candidate) -> bool {
    static SUBJECT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i|i'm|im|i've|ive|i'd|we|we're|we've)\b"));
    static TOLD: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^(?:lives|works|has|plays|likes|loves|enjoys|prefers|uses|speaks|studies|owns|runs|is|was|building|making|writing|learning|working|grew up|comes from|supports|follows|drives|cycles|walks)\b")
    });
    let t = without_filler(&c.text);
    SUBJECT.is_match(&t) || c.kind == Kind::Told && TOLD.is_match(&t)
}

/// Is every word of the title in the text as written there (with its
/// capitals for a name)?
fn title_said(title: &str, text: &str, as_name: bool) -> bool {
    let toks = tokens(text);
    let words: Vec<String> = tokens(title).into_iter().filter(|w| !matches!(w.as_str(), "," | "(" | ")" | ":" | ";" | "." | "!" | "?")).collect();
    !words.is_empty()
        && words.iter().all(|w| {
            let small = matches!(w.to_lowercase().as_str(), "the" | "of" | "and" | "a" | "an" | "&");
            toks.iter().any(|t| if as_name && !small { t == w || t.trim_end_matches("'s") == w } else { bare(t) == bare(w) })
        })
}

/// A fact as memory writes it: in the third person, without "I" ("I work
/// nights" → "Works nights", "I'm a nurse" → "Nurse", "Moved to Bristol" →
/// "Lives in Bristol"). Pure.
pub fn as_memory(fact: &str) -> String {
    static I_AM: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i'm|i am|im)\s+(?:an?\s+)?"));
    static I_VERB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i|we)\s+(?:also\s+|still\s+|now\s+)?(\w+)\b"));
    static MOVED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:has\s+|just\s+|recently\s+)*moved to\s+"));
    static I_HAVE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:i've|ive|i have)\s+"));
    static WE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:we've|we have)\s+|^(?:we're|we are)\s+(?:an?\s+)?"));
    let mut f = ops::clean_fact(fact.trim().trim_matches('"'));
    if let Some(m) = I_HAVE.find(&f) {
        f = format!("Has {}", &f[m.end()..]);
    } else if let Some(m) = WE.find(&f) {
        let have = m.as_str().to_lowercase().contains("ve") || m.as_str().to_lowercase().contains("have");
        f = format!("{}{}", if have { "Have " } else { "" }, &f[m.end()..]);
    } else if let Some(m) = I_AM.find(&f) {
        f = f[m.end()..].to_string();
    } else if let Some(c) = I_VERB.captures(&f) {
        let verb = c[1].to_lowercase();
        let third = match verb.as_str() {
            "have" => "has".to_string(),
            "do" => "does".to_string(),
            "go" => "goes".to_string(),
            v if v.ends_with('s') || v.ends_with("sh") || v.ends_with("ch") || v.ends_with('x') => format!("{v}es"),
            v if v.ends_with('y') && !v.ends_with("ay") && !v.ends_with("ey") && !v.ends_with("oy") => format!("{}ies", &v[..v.len() - 1]),
            v => format!("{v}s"),
        };
        f = format!("{third}{}", &f[c.get(0).map_or(0, |m| m.end())..]);
    } else if let Some(rest) = f.strip_prefix("My ").or_else(|| f.strip_prefix("my ")) {
        f = rest.to_string();
    }
    if let Some(m) = MOVED.find(&f) {
        f = format!("Lives in {}", &f[m.end()..]);
    }
    // "Walks my dog" → "Walks their dog" (memory speaks of the person).
    static MY: LazyLock<Regex> = LazyLock::new(|| re(r"\bmy\b"));
    let f = MY.replace_all(&f, "their");
    ops::clean_fact(&f)
}

/// Why a candidate's answer was left out (for the log's counts).
pub type Why = &'static str;

/// **Check the model's answer** for candidate `c` and turn it into a
/// change for `ops::apply` (whose own guards still run), or say why not.
/// `items`: memory now. Pure.
pub fn validate(c: &Candidate, x: &Extracted, items: &[Item]) -> Result<Change, Why> {
    let Some(section) = x.section else {
        return Err("skipped");
    };
    let title = ops::one_line(x.title.trim().trim_matches(['"', '\'']), ops::MAX_NAME);
    match validate_in(c, section, title.clone(), &x.fact, items) {
        // A person, project or topic whose "name" isn't one in the sentence
        // ("I" for "I play chess at a club", "The name as written" — the
        // 1.5B, 2026-10-22), when the person is the sentence's subject: what
        // they said about themselves is still theirs (Profile), if it passes.
        Err(why) if !section.is_you() && the_person_speaks(c) && !names_in(c, &title) => {
            validate_in(c, Section::Profile, String::new(), &x.fact, items).map_err(|_| why)
        }
        other => other,
    }
}

/// Is `title` a name (or a capitalised title) the sentence really has?
fn names_in(c: &Candidate, title: &str) -> bool {
    !title.trim().is_empty() && !matches!(bare(title).as_str(), "i" | "me" | "user" | "the user" | "you") && title_said(title, &c.text, true)
}

fn validate_in(c: &Candidate, mut section: Section, mut title: String, raw_fact: &str, items: &[Item]) -> Result<Change, Why> {
    let mut fact = as_memory(raw_fact);
    // Never a question or a request, whatever the section.
    if fact_is_question(&fact) || fact_is_ask(&fact) {
        fact.clear();
    }
    let fact_ok = |f: &str| !f.is_empty() && !about_the_world(f) && !passing_time(f) && !a_change(f) && !ops::passing(f) && said_in(f, &c.text);
    let told = c.kind == Kind::Told;
    // A told "Rochelle moved to Bristol" filed under Profile is Rochelle's.
    if section.is_you() {
        if let Some(p) = subject_person(c, items) {
            section = Section::People;
            title = p.title.clone();
        }
    }
    // How to answer: only from a candidate about answers; else a liking is
    // the person's own (profile).
    if section == Section::Preferences && !(c.kind == Kind::Preference || told && answer_style(&c.text)) {
        section = Section::Profile;
    }
    if c.kind == Kind::Preference && section != Section::Preferences {
        if answer_style(&fact) {
            section = Section::Preferences;
        } else {
            return Err("not how they want answers");
        }
    }
    let add = |section: Section, name: String, about: Option<String>, facts: Vec<String>| Change {
        action: Action::Add,
        id: None,
        section: Some(section),
        name,
        about,
        facts,
        remove_facts: vec![],
        say: String::new(),
    };
    match section {
        Section::Profile | Section::Preferences => {
            // Someone else is the sentence's subject ("Daniel, my flatmate,
            // keeps…", "My flatmate keeps…"): not the person's.
            if !the_person_speaks(c) && !told && someone_else_first(&c.text) {
                return Err("about someone else");
            }
            // The person's own: said of themselves ("I…"), told, or a lasting
            // thing of theirs ("My birthday is…") — not "My code keeps
            // crashing".
            if !the_person_speaks(c) && !told && c.kind != Kind::Preference && !lasting_possessive(&c.text) {
                return Err("not about the person");
            }
            // Their own words, when they said it of themselves.
            let fact = match own_words(c) {
                Some(own) if (the_person_speaks(c) || told) && fact_ok(&own) => own,
                _ => fact,
            };
            if !fact_ok(&fact) {
                return Err("a fact the user didn't say about themselves");
            }
            // About someone else ("Sister lives in York").
            let first = bare(fact.split_whitespace().next().unwrap_or(""));
            if RELATION.contains(&first.as_str()) && fact.split_whitespace().nth(1).is_some_and(|w| matches!(w, "is" | "lives" | "works" | "has")) {
                return Err("about someone else");
            }
            if section == Section::Preferences && !answer_style(&fact) && !answer_style(&c.text) {
                return Err("not how they want answers");
            }
            let mut change = add(section, String::new(), None, vec![fact.clone()]);
            change.remove_facts = replaced(&fact, items.iter().find(|i| i.section == section));
            Ok(change)
        }
        Section::People => {
            if !ops::looks_like_a_name(&title) || !title_said(&title, &c.text, true) {
                return Err("a person not named as written");
            }
            let known = items.iter().find(|i| i.section == Section::People && ops::same_thing(Section::People, &i.title, &title));
            let relation = relation_of(&c.text, &title);
            // Someone memory has, told about or the sentence's subject
            // ("Rochelle's birthday is…"), needs no relationship word.
            let their_own = known.is_some_and(|k| told || subject_person(c, items).is_some_and(|s| s.id == k.id));
            if relation.is_none() && !their_own {
                return Err("a person with no relationship word");
            }
            let mut facts = Vec::new();
            if let Some(f) = their_fact(&fact, &title) {
                let restates = relation.as_deref().is_some_and(|r| ops::norm(r) == ops::norm(&f) || ops::norm(&f).ends_with(&ops::norm(r)));
                if fact_ok(&f) && !restates && who_they_are(&f) {
                    facts.push(f);
                }
            }
            let about = relation.filter(|_| known.is_none_or(|k| k.summary.trim().is_empty()));
            let mut change = add(Section::People, title, about, facts);
            change.remove_facts = change.facts.first().map(|f| replaced(f, known)).unwrap_or_default();
            Ok(change)
        }
        Section::Projects => {
            if !(owns_project(&c.text) || told && items.iter().any(|i| i.section == Section::Projects && ops::same_thing(Section::Projects, &i.title, &title))) {
                return Err("a project the user doesn't own");
            }
            if title.is_empty() || !title_said(&title, &c.text, true) || !tokens(&title).iter().any(|w| w.chars().next().is_some_and(char::is_uppercase)) {
                return Err("a project not named as written");
            }
            let fact = if fact_ok(&fact) && ops::norm(&fact) != ops::norm(&title) { Some(fact) } else { None };
            // What it is, in a few words, is its "about" (as the remember tool does).
            let (about, facts) = match fact {
                Some(f) if f.split_whitespace().count() <= 8 => (Some(f), vec![]),
                Some(f) => (None, vec![f]),
                None => (None, vec![]),
            };
            Ok(add(Section::Projects, title, about, facts))
        }
        Section::Topics => {
            if !keeps_up(&c.text) {
                return Err("not a lasting interest");
            }
            if title.is_empty() || !title_said(&title, &c.text, false) || about_the_world(&title) {
                return Err("a topic not named as written");
            }
            let facts = if fact_ok(&fact) && ops::norm(&fact) != ops::norm(&title) { vec![fact] } else { vec![] };
            Ok(add(Section::Topics, capitalise(&title), None, facts))
        }
    }
}

/// A new "Lives in X" replaces the item's other "Lives in …" (one home);
/// nothing else is replaced by itself. Pure.
fn replaced(fact: &str, item: Option<&Item>) -> Vec<String> {
    let lives = |f: &str| f.to_lowercase().starts_with("lives in ");
    match item {
        Some(i) if lives(fact) => i.details.iter().filter(|d| lives(d) && ops::norm(d) != ops::norm(fact)).cloned().collect(),
        _ => vec![],
    }
}

/// The Profile facts naming a place or employer the person left ("I've left
/// Acme" → "Works at Acme as a nurse"), as one update. Pure.
pub fn left_changes(left: &[String], items: &[Item]) -> Vec<Change> {
    let Some(profile) = items.iter().find(|i| i.section == Section::Profile) else {
        return vec![];
    };
    let gone: Vec<String> = profile
        .details
        .iter()
        .filter(|d| {
            let d = format!(" {} ", ops::norm(d));
            left.iter().any(|l| {
                let l = ops::norm(l);
                !l.is_empty() && d.contains(&format!(" {l} "))
            })
        })
        .cloned()
        .collect();
    if gone.is_empty() {
        return vec![];
    }
    vec![Change { action: Action::Update, id: Some(profile.id), section: None, name: String::new(), about: None, facts: vec![], remove_facts: gone, say: String::new() }]
}

/// "Forget …" for a model without tools, without a model: the facts and
/// the people, projects and topics that name what's to be forgotten ("my
/// old job at Acme" → every fact with "Acme", Sam "Colleague at Acme", the
/// "Acme ward rota" project; "that I work at Acme" → that fact). Profile
/// and Preferences lose facts only. Pure.
pub fn forget(items: &[Item], what: &str) -> Vec<Change> {
    const VAGUE: &[&str] = &["forget", "remember", "everything", "anything", "about", "please", "that", "this", "with", "from", "they", "them", "there", "were", "what", "which", "also", "stuff", "things", "thing", "old", "former", "previous", "last", "month", "week", "year", "ago", "left", "used"];
    // Names said: capitalised words other than at the start, and uncommon words.
    let names: Vec<String> = tokens(what)
        .iter()
        .filter(|t| t.chars().next().is_some_and(char::is_uppercase) && !matches!(t.as_str(), "I" | "Forget" | "Please" | "Remove" | "Delete" | "Everything" | "My" | "That"))
        .map(|t| bare(t))
        .filter(|t| t.chars().count() >= 3 && !VAGUE.contains(&t.as_str()))
        .collect();
    if !names.is_empty() {
        let mut out = Vec::new();
        for i in items {
            let has = |s: &str| {
                let s = format!(" {} ", ops::norm(s));
                names.iter().any(|n| s.contains(&format!(" {n} ")))
            };
            if !i.section.is_you() && (has(&i.title) || has(&i.summary)) {
                out.push(Change { action: Action::Remove, id: Some(i.id), section: None, name: String::new(), about: None, facts: vec![], remove_facts: vec![], say: String::new() });
                continue;
            }
            let facts: Vec<String> = i.details.iter().filter(|d| has(d)).cloned().collect();
            if !facts.is_empty() {
                out.push(Change { action: Action::Update, id: Some(i.id), section: None, name: String::new(), about: None, facts: vec![], remove_facts: facts, say: String::new() });
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    // No name: the fact as memory words it, or nearly (the chat's forget tool's rule).
    super::learn::forget_changes(items, what)
}

/// "Forget …", "Remove …", "Delete …" as a Tell: what to forget.
pub fn forget_request(text: &str) -> Option<String> {
    static F: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?i)^\s*(?:please\s+)?(?:forget|remove|delete|erase|stop remembering|don't remember|do not remember)\b(?:\s+(?:that|about|everything about|all about))?[:,]?\s*(.+?)\s*[.!]?\s*$")
    });
    F.captures(text.trim()).map(|c| c[1].trim().to_string()).filter(|w| w.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::super::ops::Origin;
    use super::*;

    fn texts(found: &Found) -> Vec<String> {
        found.candidates.iter().map(|c| c.text.clone()).collect()
    }

    #[test]
    fn sentences_and_clauses_are_cut_where_a_new_subject_starts() {
        let s = sentences("I'm a nurse at St James's in Norwich and I work night shifts, so I need easy meals. Any ideas?");
        assert_eq!(s.len(), 2);
        assert!(s[1].1, "a question");
        let c: Vec<String> = clauses(&s[0].0, s[0].1).into_iter().map(|(c, _)| c).collect();
        assert_eq!(c, ["I'm a nurse at St James's in Norwich", "I work night shifts", "I need easy meals"]);
        // A question after a statement, in one sentence.
        let c = clauses("I'm building a dictation app called Yap, how does Tauri's event system work", true);
        assert_eq!(c, [("I'm building a dictation app called Yap".to_string(), false), ("how does Tauri's event system work".to_string(), true)]);
        // A two-word start stays whole.
        let c: Vec<String> = clauses("The page says it's £58 return, is that per day", true).into_iter().map(|(c, _)| c).collect();
        assert_eq!(c, ["The page says it's £58 return", "is that per day"]);
        // "Mr." and decimals don't end a sentence.
        assert_eq!(sentences("My boss is Mr. Okafor. It costs 2.50 a day.").len(), 2);
    }

    #[test]
    fn questions_requests_and_the_world_are_never_candidates() {
        let none = [
            "What's the capital of Canada?",
            "How many tablespoons are in a cup?",
            "Can you recommend a good sci-fi film for tonight?",
            "I read that Canberra is the capital of Australia, not Sydney.",
            "I also heard the Great Wall isn't visible from space.",
            "Can you draft a short message to Marco saying the invoice will be a week late?",
            "I'm meeting Ingrid for coffee tomorrow near the station.",
            "Tobias said the new Zelda game is brilliant, is it worth buying?",
            "I need a chocolate birthday cake recipe for Saturday, and I want to book a table for six in Manchester on Sunday.",
            "Just give me a one-line answer this time: what's 15% of 80?",
            "Have you heard of the Zed editor? Is it faster than VS Code for Rust?",
            "The page says a car and two adults is £58 return, is that per day?",
            "Can you give me a quick lasagne recipe? I'm cooking for four tonight.",
            "Give me a short answer this time.",
            "My search says flights to Rome cost £89, is that good?",
        ];
        for t in none {
            let f = candidates(&[t], false, &[]);
            assert!(f.candidates.is_empty(), "{t}: {:?}", f.candidates);
        }
    }

    /// Everyday chat in the first person that says nothing lasting about the
    /// person: never a candidate (no model call, nothing to get wrong).
    #[test]
    fn everyday_chat_in_the_first_person_is_not_a_candidate() {
        let none = [
            "I'm so confused about how VAT works.",
            "I'm not sure what to cook.",
            "I think the new iPhone is overpriced.",
            "I can't get my code to compile.",
            "I'm tired, can you keep it brief?",
            "I'm stuck on level 3 of this puzzle.",
            "I've got a headache.",
            "I just bought a new kettle.",
            "We went to Rome last year and loved it.",
            "I watched Dune last night.",
            "I need a gift idea for a colleague.",
            "I want to learn how to make sourdough.",
            "I'm looking for a cheap flight to Madrid.",
            "I'm thinking of buying a bike.",
            "I'll be late tomorrow.",
            "I'm going to the gym later.",
            "I was wondering if you could help.",
            "I don't understand this error message.",
            "I forgot my umbrella today.",
            "I'm back, sorry about that.",
            "I asked my boss for a raise.",
            "I'm at the station now.",
            "I have no idea what that means.",
            "My train is late again.",
            "Thanks, I'll try that.",
            "I hope it doesn't rain on Saturday.",
            "I guess that makes sense.",
            "I'm trying to decide between two laptops.",
            "Can I ask you something?",
            "Should I bring an umbrella tomorrow?",
        ];
        for t in none {
            let f = candidates(&[t], false, &[]);
            assert!(f.candidates.is_empty(), "{t}: {:?}", f.candidates);
        }
        // "I've got a dog called Rex" is a pet, not a one-off.
        let f = candidates(&["I've got a dog called Rex."], false, &[]);
        assert_eq!(texts(&f), ["I've got a dog called Rex"]);
        assert_eq!(people_in("I've got a dog called Rex"), [("Rex".to_string(), "Dog".to_string())]);
        // Profile is the person's own: "My code keeps crashing" isn't, "My birthday is 3 May" is.
        assert!(validate(&cand("My code keeps crashing when I run it"), &x(Section::Profile, "", "Code keeps crashing"), &[]).is_err());
        assert!(validate(&cand("My birthday is on the 3rd of May"), &x(Section::Profile, "", "Birthday is on the 3rd of May"), &[]).is_ok());
    }

    #[test]
    fn what_the_person_says_about_themselves_is() {
        let f = candidates(&["I'm a nurse at St James's in Norwich and I work night shifts, so I need easy meals I can take to work. Any ideas?"], false, &[]);
        assert_eq!(texts(&f), ["I'm a nurse at St James's in Norwich", "I work night shifts"]);
        let f = candidates(&["Thanks! Oh and they need to be quick, I have to walk my dog Beans before every shift."], false, &[]);
        assert_eq!(texts(&f), ["I have to walk my dog Beans before every shift"]);
        // Someone in their life, this weekend: the person is the candidate.
        let f = candidates(&["My sister Rochelle is coming to stay this weekend. Any ideas for a rainy Saturday in York?"], false, &[]);
        assert_eq!(texts(&f), ["My sister Rochelle is coming to stay this weekend"]);
        // How they want answers.
        let f = candidates(&["Please keep your answers short from now on, I read them on my phone. And always use metric units."], false, &[]);
        assert_eq!(f.candidates.iter().filter(|c| c.kind == Kind::Preference).count(), 2, "{:?}", f.candidates);
        // A project; a hobby.
        assert_eq!(texts(&candidates(&["I'm building a dictation app called Yap in Rust and Svelte. How does Tauri's event system work?"], false, &[])), ["I'm building a dictation app called Yap in Rust and Svelte"]);
        assert_eq!(texts(&candidates(&["I play chess at a club every Thursday and I'm trying to get better at endgames."], false, &[])), ["I play chess at a club every Thursday"]);
        // Secrets and (with the setting off) sensitive topics: not even asked.
        assert!(candidates(&["The wifi password is tulip-99-garden, we live at 14 Elm Road, Norwich NR6 2AB."], false, &[]).candidates.is_empty());
        assert!(candidates(&["I've just been diagnosed with type 2 diabetes."], false, &[]).candidates.is_empty());
        assert_eq!(candidates(&["I've just been diagnosed with type 2 diabetes."], true, &[]).candidates.len(), 1);
        // Leaving: Yap's own; the new job is a candidate.
        let f = candidates(&["I've left Acme! I started at St James's Hospital last week as a ward manager."], false, &[]);
        assert_eq!(f.left, ["Acme"]);
        assert_eq!(texts(&f), ["I started at St James's Hospital last week as a ward manager"]);
        // "Remember that…" inside a message.
        // Someone memory has, as the subject.
        let known = ["Rochelle".to_string()];
        let f = candidates(&["Rochelle's birthday is on the 3rd of May. What should I get my sister? She loves gardening."], false, &known);
        assert_eq!(texts(&f), ["Rochelle's birthday is on the 3rd of May"]);
        assert!(candidates(&["Rochelle's birthday is on the 3rd of May."], false, &[]).candidates.is_empty(), "unknown: not about the person");
        let f = candidates(&["Oh and remember that I'm vegetarian when you suggest recipes."], false, &[]);
        assert_eq!((f.candidates[0].kind, f.candidates[0].text.as_str()), (Kind::Told, "I'm vegetarian when you suggest recipes"));
    }

    #[test]
    fn a_person_needs_a_relationship_word_next_to_their_name() {
        assert_eq!(relation_of("My sister Rochelle is coming to stay", "Rochelle").as_deref(), Some("Sister"));
        assert_eq!(relation_of("My best friend Imogen is a vet in Bristol", "Imogen").as_deref(), Some("Best friend"));
        assert_eq!(relation_of("My manager, Mr Okafor, wants a report", "Mr Okafor").as_deref(), Some("Manager"));
        assert_eq!(relation_of("My manager, Mr Okafor, wants a report", "Okafor").as_deref(), Some("Manager"));
        assert_eq!(relation_of("Daniel, my flatmate, keeps leaving the heating on", "Daniel").as_deref(), Some("Flatmate"));
        assert_eq!(relation_of("Rochelle is my sister", "Rochelle").as_deref(), Some("Sister"));
        assert_eq!(relation_of("My sister's name is Rochelle", "Rochelle").as_deref(), Some("Sister"));
        assert_eq!(relation_of("My colleague Sam Patel, the product manager at Acme, asked me", "Sam Patel").as_deref(), Some("Colleague"));
        assert_eq!(relation_of("I have to walk my dog Beans before every shift", "Beans").as_deref(), Some("Dog"));
        assert_eq!(relation_of("My mum is visiting", "Mum").as_deref(), Some("Mum"));
        // Names in passing.
        assert_eq!(relation_of("I'm meeting Ingrid for coffee", "Ingrid"), None);
        assert_eq!(relation_of("Tobias said the game is brilliant", "Tobias"), None);
        assert_eq!(relation_of("I sent my report to Marco", "Marco"), None);
        assert_eq!(relation_of("My sister Rochelle is visiting", "Imogen"), None);
    }

    #[test]
    fn yap_finds_people_named_with_who_they_are() {
        let found = |t: &str| people_in(t).into_iter().map(|(n, r)| format!("{n}: {r}")).collect::<Vec<_>>();
        assert_eq!(found("My sister Rochelle is coming to stay this weekend"), ["Rochelle: Sister"]);
        assert_eq!(found("My best friend Imogen is a vet in Bristol. My manager, Mr Okafor, wants a report"), ["Imogen: Best friend", "Mr Okafor: Manager"]);
        assert_eq!(found("Daniel, my flatmate, keeps leaving the heating on"), ["Daniel: Flatmate"]);
        assert_eq!(found("My colleague Sam Patel, the product manager at Acme, asked me"), ["Sam Patel: Colleague"]);
        assert_eq!(found("My sister's name is Rochelle"), ["Rochelle: Sister"]);
        assert_eq!(found("Has a sister called Rochelle who lives in York"), ["Rochelle: Sister"]);
        assert_eq!(found("I have to walk my dog Beans before every shift"), ["Beans: Dog"]);
        // Not: names in passing, a relation with no name, a sentence's first word.
        assert!(found("I'm meeting Ingrid for coffee and sending Marco the invoice").is_empty());
        assert!(found("My flatmate keeps leaving the heating on").is_empty());
        assert_eq!(found("Yesterday, my sister Rochelle came round"), ["Rochelle: Sister"]);
        assert!(found("My friend has a dog called Rex").is_empty(), "the friend's dog");
    }

    #[test]
    fn yap_finds_projects_and_birthdays_in_the_persons_words() {
        assert_eq!(project_in("I'm writing a fantasy novel called The Salt Road in the evenings"), Some(("The Salt Road".into(), "Fantasy novel".into())));
        assert_eq!(project_in("I'm building a dictation app called Yap in Rust and Svelte"), Some(("Yap".into(), "Dictation app".into())));
        assert_eq!(project_in("is writing a cookbook called Small Plates"), Some(("Small Plates".into(), "Cookbook".into())));
        // No ownership, or a person: none.
        assert_eq!(project_in("My friend made an app called Bloop"), None);
        assert_eq!(project_in("Has a sister called Rochelle"), None);
        assert_eq!(birthday_in("Rochelle's birthday is on the 3rd of May"), Some((Some("Rochelle".into()), "Birthday 3 May".into())));
        assert_eq!(birthday_in("my birthday is May 14th"), Some((None, "Birthday 14 May".into())));
        assert_eq!(birthday_in("My sister's birthday is on the 3rd of May"), None, "whose: no name");
        let known = vec![item(7, Section::People, "Rochelle", "Sister", &["Lives in York"])];
        let c = birthday_changes(&cand("Rochelle's birthday is on the 3rd of May"), &known);
        assert_eq!((c[0].name.as_str(), c[0].facts.clone()), ("Rochelle", vec!["Birthday 3 May".to_string()]));
        assert!(birthday_changes(&cand("Priya's birthday is on the 3rd of May"), &known).is_empty(), "someone unknown, no relation");
        // Told preferences: no call, their words.
        let f = told("- Prefers concise answers with British spelling\n- Asked me to always reply in a friendly tone", true);
        let p: Vec<String> = f.candidates.iter().filter_map(preference_of).map(|c| c.facts[0].clone()).collect();
        assert_eq!(p, ["Prefers concise answers with British spelling", "Always reply in a friendly tone"]);
    }

    #[test]
    fn a_preference_is_kept_in_the_persons_own_words() {
        let p = |t: &str| preference_of(&Candidate { text: t.into(), kind: Kind::Preference }).map(|c| c.facts[0].clone());
        assert_eq!(p("Please keep your answers short from now on").as_deref(), Some("Keep your answers short"));
        assert_eq!(p("always use metric units").as_deref(), Some("Always use metric units"));
        assert_eq!(preference_of(&cand("I work nights")), None);
    }

    fn cand(text: &str) -> Candidate {
        Candidate { text: text.into(), kind: Kind::About }
    }

    #[test]
    fn a_bad_name_from_the_model_falls_back_to_what_the_person_said_about_themselves() {
        let none: Vec<Item> = vec![];
        // The 1.5B's "people: I" for a hobby.
        let c = validate(&cand("I play chess at a club every Thursday"), &x(Section::People, "I", "I play chess at a club every Thursday"), &none).unwrap();
        assert_eq!((c.section, c.facts.clone()), (Some(Section::Profile), vec!["Plays chess at a club every Thursday".to_string()]));
        // A real name in passing stays out (it's about them, not the person).
        assert!(validate(&cand("I sent the photos to Marco"), &x(Section::People, "Marco", "Sent the photos"), &none).is_err());
        // A person's fact without their name; a restatement of who they are goes.
        assert_eq!(their_fact("Imogen is a vet in Bristol", "Imogen").as_deref(), Some("Vet in Bristol"));
        assert_eq!(their_fact("Has a sister called Rochelle who lives in York", "Rochelle"), None);
        // A fact with a word nobody said.
        assert!(!said_in("Works night shifts at the hospital", "I work night shifts"));
        assert!(said_in("Works night shifts", "I work night shifts"));
        // One-off events aren't candidates.
        assert!(candidates(&["I sent the photos to Marco yesterday.", "We went to Rome."], false, &[]).candidates.is_empty());
    }

    fn x(section: Section, title: &str, fact: &str) -> Extracted {
        Extracted { section: Some(section), title: title.into(), fact: fact.into() }
    }

    #[test]
    fn the_models_answer_is_checked_against_the_sentence() {
        let none: Vec<Item> = vec![];
        // A person: who they are comes from the sentence.
        let c = validate(&cand("My sister Rochelle is coming to stay this weekend"), &x(Section::People, "Rochelle", "Coming to stay this weekend"), &none).unwrap();
        assert_eq!((c.name.as_str(), c.about.as_deref(), c.facts.len()), ("Rochelle", Some("Sister"), 0));
        let c = validate(&cand("My best friend Imogen is a vet in Bristol"), &x(Section::People, "Imogen", "Vet in Bristol"), &none).unwrap();
        assert_eq!((c.about.as_deref(), c.facts.clone()), (Some("Best friend"), vec!["Vet in Bristol".to_string()]));
        // The 1.5B's "Nurse at St James's in Norwich" as a person: what the
        // person said of themselves, in their words, under Profile.
        let c = validate(&cand("I'm a nurse at St James's in Norwich"), &x(Section::People, "Nurse at St James's in Norwich", ""), &none).unwrap();
        assert_eq!((c.section, c.name.as_str(), c.facts.clone()), (Some(Section::Profile), "", vec!["Nurse at St James's in Norwich".to_string()]));
        // "I'm Declan, by the way" with a nonsense title: the name, said plainly.
        let c = validate(&cand("I'm Declan, by the way"), &x(Section::People, "the name as written", "My name is Declan"), &none).unwrap();
        assert_eq!(c.facts, ["Name is Declan"]);
        // A name not in the sentence; a name with no relation.
        assert!(validate(&cand("My sister Rochelle is visiting"), &x(Section::People, "Tom", "Brother"), &none).is_err());
        assert!(validate(&cand("I'm meeting Ingrid for coffee near the station"), &x(Section::People, "Ingrid", "Friend"), &none).is_err());
        // Profile: the person's own words, whatever the model added.
        let c = validate(&cand("I work night shifts"), &x(Section::Profile, "", "I work night shifts"), &none).unwrap();
        assert_eq!(c.facts, ["Works night shifts"]);
        let c = validate(&cand("I work night shifts"), &x(Section::Profile, "", "Works night shifts in Glasgow"), &none).unwrap();
        assert_eq!(c.facts, ["Works night shifts"]);
        // Someone else's habit isn't the person's.
        assert!(validate(&cand("Daniel, my flatmate, keeps leaving the heating on"), &x(Section::Profile, "", "Keeps leaving the heating on"), &none).is_err());
        let c = validate(&cand("Daniel, my flatmate, keeps leaving the heating on"), &x(Section::People, "Daniel", "Leaves the heating on all day"), &none).unwrap();
        assert_eq!((c.about.as_deref(), c.facts.len()), (Some("Flatmate"), 0));
        // A fact about the world from the model never lands: the person's words do.
        let c = validate(&cand("I live near Canberra"), &x(Section::Profile, "", "Canberra is the capital of Australia"), &none).unwrap();
        assert_eq!(c.facts, ["Lives near Canberra"]);
        assert!(validate(&cand("My sister says Canberra is lovely"), &x(Section::Profile, "", "Canberra is the capital of Australia"), &none).is_err());
        assert!(validate(&cand("I bought one, a Switch costs £400"), &x(Section::Profile, "", "Switch costs £400"), &none).is_err());
        // Topics need a lasting interest; projects need ownership.
        assert!(validate(&cand("I play chess at a club every Thursday"), &x(Section::Topics, "Chess", "Plays at a club every Thursday"), &none).is_ok());
        assert!(validate(&cand("My trip to Australia was great"), &x(Section::Topics, "Australia", "Capital is Canberra"), &none).is_err());
        let c = validate(&cand("I'm writing a fantasy novel called The Salt Road in the evenings"), &x(Section::Projects, "The Salt Road", "Fantasy novel"), &none).unwrap();
        assert_eq!((c.name.as_str(), c.about.as_deref()), ("The Salt Road", Some("Fantasy novel")));
        assert!(validate(&cand("My friend uses the Zed editor a lot"), &x(Section::Projects, "Zed", "Editor"), &none).is_err());
        // Preferences only from a candidate about answers.
        let pref = Candidate { text: "Please keep your answers short from now on".into(), kind: Kind::Preference };
        assert_eq!(validate(&pref, &x(Section::Preferences, "", "Short answers"), &none).unwrap().section, Some(Section::Preferences));
        assert!(validate(&cand("I like tea"), &x(Section::Preferences, "", "Likes tea"), &none).is_ok_and(|c| c.section == Some(Section::Profile)));
        // Skip is skip.
        assert!(validate(&cand("I like tea"), &Extracted::default(), &none).is_err());
    }

    fn item(id: u64, section: Section, title: &str, summary: &str, details: &[&str]) -> Item {
        Item {
            id,
            section,
            title: title.into(),
            summary: summary.into(),
            details: details.iter().map(|d| d.to_string()).collect(),
            source: Origin::new("chat", None, ""),
            created_ts: 0,
            updated_ts: 0,
            dictation: true,
        }
    }

    #[test]
    fn a_move_or_a_job_left_replaces_what_memory_had() {
        let items = vec![
            item(1, Section::Profile, "Profile", "", &["Works at Acme as a nurse", "Lives in Norwich"]),
            item(2, Section::People, "Rochelle", "Sister", &["Lives in York", "Loves gardening"]),
        ];
        let c = left_changes(&["Acme".into()], &items);
        assert_eq!((c[0].id, c[0].remove_facts.clone()), (Some(1), vec!["Works at Acme as a nurse".to_string()]));
        assert!(left_changes(&["Northgate".into()], &items).is_empty());
        // "Rochelle moved to Bristol", told.
        let told = Candidate { text: "Rochelle moved to Bristol".into(), kind: Kind::Told };
        let c = validate(&told, &x(Section::People, "Rochelle", "Moved to Bristol"), &items).unwrap();
        assert_eq!((c.facts.clone(), c.remove_facts.clone()), (vec!["Lives in Bristol".to_string()], vec!["Lives in York".to_string()]));
        // Forgetting a job: everything naming it.
        let mut more = items.clone();
        more.push(item(3, Section::People, "Sam Patel", "Colleague at Acme", &["Product manager"]));
        more.push(item(4, Section::Projects, "Acme ward rota", "", &[]));
        let c = forget(&more, "everything about my old job at Acme");
        let removed: Vec<u64> = c.iter().filter(|c| c.action == Action::Remove).filter_map(|c| c.id).collect();
        assert_eq!(removed, [3, 4]);
        assert_eq!(c.iter().find(|c| c.id == Some(1)).unwrap().remove_facts, ["Works at Acme as a nurse"]);
        assert!(c.iter().all(|c| c.id != Some(2)), "Rochelle stays");
        assert_eq!(forget_request("Forget everything about my old job at Acme").as_deref(), Some("my old job at Acme"));
        assert_eq!(forget_request("Forget: I work at Acme, I left last month").as_deref(), Some("I work at Acme, I left last month"));
        assert_eq!(forget_request("My dog's name is Beans"), None);
    }

    #[test]
    fn told_text_and_imports_come_in_parts() {
        let f = told("My dog's name is Beans", false);
        assert_eq!(texts(&f), ["My dog's name is Beans"]);
        let import = "```\n- Prefers concise answers with British spelling\n- Name is Kit, lives in Norwich, UK\n- Works as a pharmacist, and is writing a cookbook called Small Plates\n- Saved password hint: tulip99\n- Wanted a recipe for lasagne on 3 March\n```\nThat's everything I have stored.";
        let t = texts(&told(import, true));
        assert_eq!(
            t,
            [
                "Prefers concise answers with British spelling",
                "Name is Kit",
                "lives in Norwich, UK",
                "Works as a pharmacist",
                "is writing a cookbook called Small Plates"
            ]
        );
        // Without a code block: the assistant's own talk isn't the person.
        let t = texts(&told("Here's what I remember about you:\n- Lives in Norwich\n- Plays chess\nThat's everything I have stored.", true));
        assert_eq!(t, ["Lives in Norwich", "Plays chess"]);
        assert!(owns_project("is writing a cookbook called Small Plates"));
        assert!(keeps_up("Plays Baldur's Gate 3 and World of Warcraft"));
    }

    #[test]
    fn a_reply_is_read_leniently() {
        assert_eq!(parse(r#"{"section": "people", "title": "Rochelle", "fact": ""}"#).section, Some(Section::People));
        assert_eq!(parse(r#"{"section": "skip", "title": "", "fact": ""}"#), Extracted::default());
        assert_eq!(parse(r#"{"skip": true}"#), Extracted::default());
        assert_eq!(parse("not json"), Extracted::default());
        assert_eq!(parse(r#"{"changes": []}"#), Extracted::default());
        assert_eq!(parse("```json\n{\"section\": \"profile\", \"title\": \"\", \"fact\": \"Works nights\"}\n```").fact, "Works nights");
        // The prompt stays within ~300 tokens with a sentence (its texts and
        // ~4 tokens of chat template a message; Yap's estimate runs ~15 %
        // high, so ≤ 350 of it).
        let m = messages(&cand("I'm a nurse at St James's in Norwich and I work night shifts at the hospital"));
        let tokens: usize = m.as_array().unwrap().iter().map(|x| crate::meeting_summary::estimate_tokens(x["content"].as_str().unwrap()) + 4).sum();
        assert!(tokens <= 350, "{tokens}");
    }

    #[test]
    fn memory_writes_facts_in_the_third_person() {
        assert_eq!(as_memory("I work night shifts"), "Works night shifts");
        assert_eq!(as_memory("I'm a nurse at St James's"), "Nurse at St James's");
        assert_eq!(as_memory("I have a dog called Beans"), "Has a dog called Beans");
        assert_eq!(as_memory("I play chess every Thursday"), "Plays chess every Thursday");
        assert_eq!(as_memory("Moved to Bristol"), "Lives in Bristol");
        assert_eq!(as_memory("I've just started learning Spanish"), "Has just started learning Spanish");
        assert_eq!(as_memory("I have to walk my dog Beans"), "Has to walk their dog Beans");
        assert_eq!(as_memory("We're vegetarian"), "Vegetarian");
        // Who someone is, not what they keep doing.
        assert!(who_they_are("Vet in Bristol") && who_they_are("Lives in York") && who_they_are("Birthday 3 May"));
        assert!(!who_they_are("Leaves the heating on all day") && !who_they_are("Keeps forgetting the bins"));
        // Someone else starts the sentence; a pet is the person's own.
        assert!(someone_else_first("Daniel, my flatmate, keeps leaving the heating on"));
        assert!(someone_else_first("My flatmate keeps leaving the heating on"));
        assert!(!someone_else_first("My dog Beans is a spaniel"));
        assert!(!someone_else_first("I work nights"));
        assert_eq!(as_memory("The user works nights."), "Works nights");
        assert!(passing_time("Coming to stay this weekend"));
        assert!(passing_time("Wants a report by Friday"));
        assert!(!passing_time("Plays chess every Thursday"));
        assert!(!passing_time("Plays chess on Thursdays"));
        assert!(about_the_world("Canberra is the capital of Australia"));
        assert!(about_the_world("Ferry costs £58 return"));
        assert!(!about_the_world("Works night shifts"));
        assert!(!about_the_world("Dog is called Beans"));
    }
}
