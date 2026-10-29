//! **Writing samples** (2026-10-29; Wispr Flow's "make it sound like you",
//! by behaviour): a few things the person wrote themselves, pasted into
//! Settings → Commands → Writing samples, so the AI's rewrites sound like
//! them. Each may be tagged with the kind of app it suits — email, chat,
//! documents — or none ("Any app").
//!
//! - **What uses them**: the rewrite shortcut and Commands by name
//!   (`pipeline/rewrite.rs`), and the transforms that reword — Polish,
//!   Shorter and the person's own ([`transform_uses`]; never Fix grammar
//!   only, Translate or Make it a prompt, whose words aren't the person's
//!   voice), on a selection, the last dictation or every dictation. Never
//!   a language shortcut's translation, an AI prompt or a terminal command.
//! - **Never AI cleanup**, at any strength: measured at High on the
//!   on-device 4B (docs/model-evals.md "Writing samples"), they changed none
//!   of ten cleanups — cleanup keeps the words said, and the kind of app's
//!   tone wins — while costing ~160 more prompt tokens a call. The plumbing
//!   is there (`cleanup_styles::Plan::samples`, `llm::cleanup_with_samples`)
//!   for the evaluations to measure again, never set for a dictation.
//! - **Which ones**: those tagged for the kind of app the text goes into
//!   (`cleanup_styles::category`: Email; Work and Personal chat are "chat";
//!   Documents), then the untagged ones, in the person's order ([`pick`]);
//!   everything else gets the untagged ones only.
//! - **How they're sent** ([`block`]): as short examples at the end of the
//!   system prompt — the stable prefix the server caches, never changing
//!   from one dictation to the next into the same kind of app — each cut at
//!   a sentence, all of them under a budget ([`budget_for`]): smaller for a
//!   model on this PC or the home network (its context is small, and on a
//!   processor every token is read slowly), larger for a cloud one.
//!   Nothing at all when there are none: every prompt exactly as before.
//! - **Where they live**: `writing_samples.json` in Yap's data folder, a
//!   file of its own (an older Yap never reads or writes it); fields a
//!   newer Yap adds are kept when this one saves ([`Store::other`]).
//!   They leave this PC only inside a rewrite sent to a cloud model, and the
//!   network receipts say so ("Commands + your writing samples · Groq",
//!   `net::Feature::CommandsSamples`; a transform on every dictation's,
//!   "Transform + your writing samples").
//!
//! The log says counts and lengths, never the text.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::cleanup_styles::Category;
use crate::dictation::transforms::Transform;

/// The most samples kept.
pub const MAX_SAMPLES: usize = 12;
/// The longest a sample is kept (characters).
pub const MAX_CHARS: usize = 1_500;
/// The most of one sample a call carries (cut at a sentence).
pub const SENT_PER_SAMPLE: usize = 600;
/// What a call carries in all (characters), to a model on this PC or the
/// home network: about 200 tokens — measured in docs/model-evals.md
/// "Writing samples".
pub const BUDGET_LOCAL: usize = 800;
/// …and to a cloud model: about 500 tokens.
pub const BUDGET_CLOUD: usize = 2_000;

/// The kinds of app a sample may be tagged with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Untagged: any app.
    Any,
    Email,
    /// Work and personal chat.
    Chat,
    Documents,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Any, Kind::Email, Kind::Chat, Kind::Documents];

    /// Its id in the file and the page ("" for Any).
    pub fn id(self) -> &'static str {
        match self {
            Kind::Any => "",
            Kind::Email => "email",
            Kind::Chat => "chat",
            Kind::Documents => "documents",
        }
    }

    /// A stored id; one this Yap doesn't know (a newer Yap's) is Any.
    pub fn from_id(id: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| k.id() == id.trim()).unwrap_or(Kind::Any)
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Any => "Any app",
            Kind::Email => "Email",
            Kind::Chat => "Chat",
            Kind::Documents => "Documents",
        }
    }
}

/// The samples for a kind of app (`None`: none at all — AI prompts and
/// terminal commands aren't the person's voice). Everything else gets the
/// untagged ones. Pure.
pub fn kind_for(category: Category) -> Option<Kind> {
    match category {
        Category::Email => Some(Kind::Email),
        Category::WorkChat | Category::PersonalChat => Some(Kind::Chat),
        Category::Documents => Some(Kind::Documents),
        Category::Other => Some(Kind::Any),
        Category::AiPrompts | Category::Terminal => None,
    }
}

/// One sample, as the file keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Sample {
    /// Stable id ("s-…", made by Settings or here).
    pub id: String,
    pub text: String,
    /// [`Kind::id`]: "" (any app), "email", "chat", "documents".
    pub kind: String,
    /// When it was added (unix ms).
    pub added_ms: u64,
}

/// `writing_samples.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub samples: Vec<Sample>,
    /// Fields a newer Yap wrote, kept as they were.
    #[serde(flatten)]
    pub other: serde_json::Map<String, Value>,
}

// ---- what a call carries -----------------------------------------------------------

/// `text` at most `max` characters, cut after a sentence (or a line) in
/// its second half, else at a word; line endings as "\n", ends trimmed.
/// Pure.
pub fn shorten(text: &str, max: usize) -> String {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    let t = t.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let head: String = t.chars().take(max).collect();
    let half = head.len() / 2;
    let sentence = head
        .char_indices()
        .filter(|(i, c)| *i >= half && matches!(c, '.' | '!' | '?' | '\n'))
        .map(|(i, c)| i + c.len_utf8())
        .next_back();
    let cut = sentence.or_else(|| head.rfind(char::is_whitespace)).unwrap_or(head.len());
    head[..cut].trim().to_string()
}

/// The samples a call carries for `kind` (pure): those tagged `kind`, then
/// the untagged ones, in the person's order, each [`shorten`]ed to
/// [`SENT_PER_SAMPLE`], until `budget` characters (the first one cut to fit
/// when it alone is over).
pub fn pick(samples: &[Sample], kind: Kind, budget: usize) -> Vec<String> {
    let tagged = samples.iter().filter(|s| kind != Kind::Any && Kind::from_id(&s.kind) == kind);
    let any = samples.iter().filter(|s| Kind::from_id(&s.kind) == Kind::Any);
    let mut out: Vec<String> = Vec::new();
    let mut used = 0usize;
    for s in tagged.chain(any) {
        let t = shorten(&s.text, SENT_PER_SAMPLE);
        if t.is_empty() {
            continue;
        }
        let n = t.chars().count();
        if used + n > budget {
            if out.is_empty() {
                let t = shorten(&t, budget);
                if !t.is_empty() {
                    out.push(t);
                }
            }
            break;
        }
        used += n;
        out.push(t);
    }
    out
}

/// The opening of the samples' part of a system prompt.
pub const BLOCK_START: &str = "How the user writes: examples of their own writing, to show their voice.";

/// What the model is told to do with them. Measured on the on-device 4B
/// against two other wordings (docs/model-evals.md "Writing samples"): one
/// asking only for "their kind of words…" moved no transform; one asking for
/// their capitals without "names always keep theirs" wrote "thanks priya".
const BLOCK_RULE: &str = "Write the result the way these examples are written: the same greetings and sign-offs, how \
casual it is, contractions, sentence length, punctuation (their dashes, a full stop left off where they leave it off) and \
capitals (lower case where they use it; names always keep theirs). They only show the voice: never copy their facts, names \
or sentences, never add anything, and never change what the text says. Follow the task's instructions on language and \
layout; for the voice, follow the examples.";

/// The samples' part of a system prompt (`None` for none). Pure.
pub fn block(texts: &[String]) -> Option<String> {
    if texts.is_empty() {
        return None;
    }
    let mut out = String::from(BLOCK_START);
    for t in texts {
        out.push_str("\n<example>\n");
        out.push_str(t.trim());
        out.push_str("\n</example>");
    }
    out.push('\n');
    out.push_str(&eval_rule().unwrap_or_else(|| BLOCK_RULE.to_string()));
    Some(out)
}

/// A model evaluation's own wording for what the model is told to do with
/// the samples (`YAP_EVAL_SAMPLES_RULE`), to compare wordings without a
/// rebuild. Never outside a run.
fn eval_rule() -> Option<String> {
    if !crate::ai::eval::active() {
        return None;
    }
    std::env::var("YAP_EVAL_SAMPLES_RULE").ok().filter(|r| !r.trim().is_empty())
}

/// `system` with the samples' part at its end, after a blank line; exactly
/// `system` without one. Pure.
pub fn with_block(system: &str, samples: Option<&str>) -> String {
    match samples.map(str::trim).filter(|s| !s.is_empty()) {
        Some(b) => format!("{system}\n\n{b}"),
        None => system.to_string(),
    }
}

/// The characters a call to `base_url` carries (pure but for a test run's
/// stand-in services): [`BUDGET_LOCAL`] for a model on this PC or the home
/// network, else [`BUDGET_CLOUD`].
pub fn budget_for(base_url: &str) -> usize {
    match crate::privacy::net::place_of_url(base_url) {
        Some(crate::privacy::net::Place::Internet) => BUDGET_CLOUD,
        _ => BUDGET_LOCAL,
    }
}

/// The block for `kind` from `store`, sized for `base_url` (pure but for
/// [`budget_for`]).
pub fn block_from(store: &Store, kind: Kind, base_url: &str) -> Option<String> {
    block(&pick(&store.samples, kind, budget_for(base_url)))
}

/// Whether transform `t` rewords in the person's voice: Polish, Shorter and
/// the person's own — never Fix grammar only, Translate or Make it a prompt.
/// Pure.
pub fn transform_uses(t: &Transform) -> bool {
    use crate::dictation::transforms::{POLISH, SHORTER};
    t.builtin.is_empty() || t.builtin == POLISH || t.builtin == SHORTER
}

/// The block for a rewrite or a transform into `category` (`None`: no
/// samples, or none for it), sized for `base_url`.
pub fn for_rewrite(category: Category, base_url: &str) -> Option<String> {
    let kind = kind_for(category)?;
    let b = block_from(&snapshot(), kind, base_url);
    if let Some(b) = &b {
        tracing::debug!(chars = b.chars().count(), kind = kind.id(), "A rewrite with the writing samples");
    }
    b
}

/// The block for transform `t` into `category`, when it uses them.
pub fn for_transform(t: &Transform, category: Category, base_url: &str) -> Option<String> {
    transform_uses(t).then(|| for_rewrite(category, base_url)).flatten()
}

// ---- the file ----------------------------------------------------------------------

static STORE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg_attr(test, allow(dead_code))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("writing_samples.json")
}

/// The file, read the first time.
fn loaded(slot: &mut Option<Store>) -> &mut Store {
    slot.get_or_insert_with(read_file)
}

/// writing_samples.json as it is on disk (none, or unreadable — set aside,
/// never written over: no samples).
#[cfg(not(test))]
fn read_file() -> Store {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
            crate::config::quarantine_corrupt(&path);
            Store::default()
        }),
        Err(_) => Store::default(),
    }
}

/// A unit test never touches the disk.
#[cfg(test)]
fn read_file() -> Store {
    Store::default()
}

/// The samples as they are now.
pub fn snapshot() -> Store {
    loaded(&mut STORE.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

/// writing_samples.json's saves (`saving.rs`): the person's own text, so a
/// failed save is said, and tried again with the next.
#[cfg_attr(test, allow(dead_code))]
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("writingSamples", "your writing samples", save_now);

#[cfg_attr(test, allow(dead_code))]
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(s) = STORE.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        write(s)?;
    }
    SAVER.check()
}

/// Write `s` to the file (a unit test: nowhere).
#[cfg(not(test))]
fn write(s: &Store) -> Result<(), crate::shell::saving::SaveError> {
    SAVER.write_json(&path(), s, true)
}

#[cfg(test)]
fn write(_: &Store) -> Result<(), crate::shell::saving::SaveError> {
    Ok(())
}

/// A sample as Settings sends it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SampleIn {
    pub id: String,
    pub text: String,
    pub kind: String,
}

/// The samples Settings sent, as they're kept (pure): each one's text
/// trimmed and at most [`MAX_CHARS`] (cut at a sentence), its kind one Yap
/// knows, an id each (kept, else made: "s-<now>-<n>"), its date kept from
/// `before`; empty ones dropped, at most [`MAX_SAMPLES`].
pub fn tidy(incoming: &[SampleIn], before: &[Sample], now_ms: u64) -> Vec<Sample> {
    let mut out: Vec<Sample> = Vec::new();
    for (n, s) in incoming.iter().enumerate() {
        let text = shorten(&s.text, MAX_CHARS);
        if text.is_empty() {
            continue;
        }
        let id = match s.id.trim() {
            id if !id.is_empty() && !out.iter().any(|o| o.id == id) => id.to_string(),
            _ => format!("s-{now_ms}-{n}"),
        };
        let added_ms = before.iter().find(|b| b.id == id).map(|b| b.added_ms).unwrap_or(now_ms);
        out.push(Sample { id, text, kind: Kind::from_id(&s.kind).id().to_string(), added_ms });
        if out.len() == MAX_SAMPLES {
            break;
        }
    }
    out
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// What Settings shows: the samples, the limits and what a call carries
/// (characters) on this PC and to the cloud.
fn status(s: &Store) -> Value {
    json!({
        "samples": s.samples,
        "max": MAX_SAMPLES,
        "maxChars": MAX_CHARS,
        "budgetLocal": BUDGET_LOCAL,
        "budgetCloud": BUDGET_CLOUD,
        "kinds": Kind::ALL.iter().map(|k| json!({ "id": k.id(), "name": k.name() })).collect::<Vec<_>>(),
    })
}

/// Settings → Commands → Writing samples: what's kept.
#[tauri::command]
pub fn writing_samples_get() -> Value {
    status(&snapshot())
}

/// Settings → Commands → Writing samples: the samples as the page has them
/// now (in order); tidied ([`tidy`]), kept and saved. A failed save is the
/// command's error (`saving.rs`), the samples kept in memory meanwhile.
#[tauri::command]
pub fn writing_samples_set(samples: Vec<SampleIn>) -> Result<Value, String> {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let store = loaded(&mut guard);
    let next = Store { samples: tidy(&samples, &store.samples, now_ms()), other: store.other.clone() };
    tracing::info!(samples = next.samples.len(), "Writing samples saved");
    *store = next.clone();
    // Written under the lock: two saves land in the order they were made.
    write(&next)?;
    drop(guard);
    Ok(status(&next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: &str, kind: &str, text: &str) -> Sample {
        Sample { id: id.into(), text: text.into(), kind: kind.into(), added_ms: 1 }
    }

    #[test]
    fn the_kind_of_app_picks_its_samples_then_the_untagged_ones_in_order() {
        let list = vec![
            s("a", "", "Any one."),
            s("e1", "email", "Hiya all, the shed's sorted. Cheers, Robin"),
            s("c", "chat", "ta, see you there"),
            s("e2", "email", "Morning both, quick one about Tuesday."),
            s("x", "notes", "A newer Yap's kind: any app here."),
        ];
        assert_eq!(pick(&list, Kind::Email, 10_000), vec![
            "Hiya all, the shed's sorted. Cheers, Robin",
            "Morning both, quick one about Tuesday.",
            "Any one.",
            "A newer Yap's kind: any app here.",
        ]);
        assert_eq!(pick(&list, Kind::Chat, 10_000)[0], "ta, see you there");
        // Everything else: the untagged ones only.
        assert_eq!(pick(&list, Kind::Any, 10_000), vec!["Any one.", "A newer Yap's kind: any app here."]);
        // AI prompts and terminal commands: none at all.
        assert_eq!(kind_for(Category::AiPrompts), None);
        assert_eq!(kind_for(Category::Terminal), None);
        assert_eq!(kind_for(Category::WorkChat), Some(Kind::Chat));
        assert_eq!(kind_for(Category::PersonalChat), Some(Kind::Chat));
        assert_eq!(kind_for(Category::Other), Some(Kind::Any));
        assert_eq!(Kind::from_id("documents"), Kind::Documents);
    }

    #[test]
    fn samples_are_capped_each_at_a_sentence_and_all_of_them_by_the_budget() {
        let long = "One short sentence here. ".repeat(60);
        let list = vec![s("1", "email", &long), s("2", "email", &long), s("3", "", "Kept if it fits.")];
        let got = pick(&list, Kind::Email, BUDGET_LOCAL);
        assert_eq!(got.len(), 1, "two of 600 don't fit in {BUDGET_LOCAL}");
        assert!(got[0].chars().count() <= SENT_PER_SAMPLE && got[0].ends_with("here."), "{:?}", got[0]);
        let total: usize = pick(&list, Kind::Email, BUDGET_CLOUD).iter().map(|t| t.chars().count()).sum();
        assert!(total <= BUDGET_CLOUD);
        assert_eq!(pick(&list, Kind::Email, BUDGET_CLOUD).len(), 3);
        // One sample bigger than the budget alone: cut to fit, never none.
        let got = pick(&[s("1", "", &long)], Kind::Any, 100);
        assert!(!got.is_empty() && got[0].chars().count() <= 100);
        // A word cut, not mid-word, when there's no sentence to cut at.
        assert_eq!(shorten("alpha beta gamma delta", 13), "alpha beta");
        assert_eq!(shorten("  Line one.\r\nLine two  ", 100), "Line one.\nLine two");
        assert!(pick(&[s("1", "", "   ")], Kind::Any, 100).is_empty());
    }

    #[test]
    fn the_block_goes_at_the_end_of_the_system_prompt_and_nothing_without_samples() {
        let b = block(&["Hiya all.".to_string(), "Cheers, Robin".to_string()]).unwrap();
        assert!(b.starts_with(BLOCK_START));
        assert!(b.contains("<example>\nHiya all.\n</example>\n<example>\nCheers, Robin\n</example>"));
        assert!(b.contains("never copy their facts"));
        assert_eq!(block(&[]), None);
        assert_eq!(with_block("SYSTEM", Some(&b)), format!("SYSTEM\n\n{b}"));
        assert_eq!(with_block("SYSTEM", None), "SYSTEM");
        assert_eq!(with_block("SYSTEM", Some("  ")), "SYSTEM");
        // Sized by where the model is.
        assert_eq!(budget_for("http://127.0.0.1:8080/v1"), BUDGET_LOCAL);
        assert_eq!(budget_for("http://192.168.1.20:11434/v1"), BUDGET_LOCAL);
        assert_eq!(budget_for("https://api.groq.com/openai/v1"), BUDGET_CLOUD);
    }

    #[test]
    fn which_rewrites_take_them() {
        let builtin = |k: &str| Transform { id: k.into(), builtin: k.into(), ..Default::default() };
        assert!(transform_uses(&builtin("polish")) && transform_uses(&builtin("shorter")));
        assert!(!transform_uses(&builtin("grammar")) && !transform_uses(&builtin("translate")) && !transform_uses(&builtin("prompt")));
        assert!(transform_uses(&Transform { id: "t-1".into(), instruction: "Warmer".into(), ..Default::default() }));
        // AI cleanup: never — a plan as a dictation makes it carries none,
        // at any strength.
        for strength in ["light", "medium", "high"] {
            let mut cfg = crate::config::YapConfig { cleanup_strength: strength.into(), ..Default::default() };
            crate::ai::cleanup_styles::on_load(&mut cfg);
            for c in Category::ALL {
                let plan = crate::ai::cleanup_styles::resolve(&cfg, &crate::ai::cleanup_styles::sample_target(c));
                assert!(plan.is_none_or(|p| p.samples.is_none()), "{strength} {c:?}");
            }
        }
    }

    #[test]
    fn what_settings_sends_is_tidied_and_unknown_fields_are_kept() {
        let incoming = vec![
            SampleIn { id: "s-1".into(), text: "  Hiya all  ".into(), kind: "email".into() },
            SampleIn { id: String::new(), text: "Second".into(), kind: "postcard".into() },
            SampleIn { id: "s-1".into(), text: "Same id again".into(), kind: String::new() },
            SampleIn { id: "s-9".into(), text: "   ".into(), kind: String::new() },
            SampleIn { id: "s-4".into(), text: "x".repeat(MAX_CHARS + 50), kind: "chat".into() },
        ];
        let before = vec![Sample { id: "s-1".into(), text: "old".into(), kind: String::new(), added_ms: 42 }];
        let got = tidy(&incoming, &before, 1_000);
        assert_eq!(got.len(), 4);
        assert_eq!((got[0].id.as_str(), got[0].text.as_str(), got[0].kind.as_str(), got[0].added_ms), ("s-1", "Hiya all", "email", 42));
        assert_eq!((got[1].id.as_str(), got[1].kind.as_str(), got[1].added_ms), ("s-1000-1", "", 1_000));
        assert_eq!(got[2].id, "s-1000-2", "an id twice gets a new one");
        assert!(got[3].text.chars().count() <= MAX_CHARS);
        let many: Vec<SampleIn> = (0..20).map(|i| SampleIn { text: format!("Sample {i}"), ..Default::default() }).collect();
        assert_eq!(tidy(&many, &[], 5).len(), MAX_SAMPLES);
        // A newer Yap's fields survive this one's save; an empty file reads.
        let s: Store = serde_json::from_str(r#"{"samples":[{"id":"a","text":"Hi","kind":"email","addedMs":3,"tone":"x"}],"future":{"on":true}}"#).unwrap();
        assert_eq!(s.samples[0].text, "Hi");
        assert_eq!(serde_json::to_value(&s).unwrap()["future"]["on"], true);
        let empty: Store = serde_json::from_str("{}").unwrap();
        assert!(empty.samples.is_empty());
    }
}
