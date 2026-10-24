//! **The refined transcript, by itself** (2026-10-24, after Wispr Flow's
//! "refined transcript"): when a meeting ends, Yap transcribes it again in
//! the background from its audio on disk (`meeting_audio.rs`) when that
//! helps — and puts the new lines in through the same replacement as
//! "Transcribe again" (`meeting_audio::transcribe_again` → `redo::rebuild`:
//! My thoughts, the names on lines that still match and the summary kept).
//!
//! **When** ([`decide`], pure; Settings → Meetings → "Improve the
//! transcript after the meeting", `meeting_refine`):
//! - **Automatically when it helps** (`auto`, the default): a chunk of the
//!   live transcript couldn't be transcribed (`meeting.rs`, "Failed
//!   chunks"), or a more accurate speech model than the live one is
//!   installed for the meeting's language ([`best_model`]);
//! - **Always**: every meeting, on the best model installed (the one in
//!   use when it's the best);
//! - **Never**: only Try again on a failed stretch, and Transcribe again.
//!
//! Never for a meeting without its audio, a pause (only the end), or one
//! "Started by mistake?".
//!
//! **Which model** ([`best_model`], pure): the most accurate model
//! installed that speaks the meeting's language — `accuracy` from the same
//! leaderboard figures as Settings' bars (`src/lib/models.js`: keep the two
//! in step) — and only when it beats the live one by a margin
//! ([`MARGIN`]). The meeting's language is the one picked in Settings →
//! Speech-to-Text, else Windows' (About you's region); unknown, a model
//! must speak every language the live one does (never Parakeet V2, English
//! only, after Parakeet V3). A model that turns out not to understand the
//! meeting changes nothing: [`agrees`] compares the new words with the
//! live ones and keeps the live transcript when they don't match.
//!
//! **Which comes first, the summary or the refine** ([`Plan::first`]):
//! - **The live transcript is whole** (a better model, or Always): the
//!   summary first — it's written from the live transcript at once (its
//!   digests were made during the meeting, so the action plan of a long
//!   meeting takes seconds; a short meeting still feels instant), and the
//!   refine waits for it, then runs. When it's done and the note has a
//!   summary, the windows offer **"Transcript improved · Update summary"**
//!   (`meeting_summarise`, the summary's own path: a cloud model gets what
//!   it gets today, nothing more). Not automatic: the refined lines rarely
//!   change what was decided, a summary the person read or edited
//!   shouldn't change under them, and a new summary of a long meeting
//!   means writing every digest again (more tokens with a cloud model).
//! - **It has holes** (chunks failed): the refine first, then the summary
//!   (or "Started by mistake?"). A summary from a transcript with holes
//!   misses what was said there — decisions, tasks — which is wrong, not
//!   just rough; a little later and complete beats now and missing.
//!
//! **How it runs** — never in the way:
//! - after the summary (above), never beside a meeting that records (it
//!   waits, "after the meeting that's recording"), and between chunks it
//!   waits for a hotkey dictation and 3 s after one (`meeting::replay`'s
//!   `give_way`): a dictation's transcription always finds the engine free;
//! - on the model in use, it shares the warm engine a chunk at a time
//!   (`pipeline::EngineCell`: a dictation that ends mid-chunk waits that
//!   chunk out, never loads a second copy); on another model, a copy of
//!   its own, dropped after;
//! - each transcription call on a lowest-priority thread ([`LowPriority`]:
//!   the CPU path's calling thread; the runtimes' own worker threads keep
//!   theirs);
//! - progress in the notepad and the Notes view ("Improving the
//!   transcript · 40%", `yap-meeting-redo` kind `refine`), with **Cancel**
//!   (between chunks: the live transcript stays).
//!
//! The audio of a meeting whose audio isn't kept ("Keep meeting audio" off)
//! is held until this is done (`meeting_audio::release`), a day at most.

use tauri::AppHandle;

use crate::meetings::meeting_audio::redo::Redone;
use crate::notes::TranscriptSegment;

/// The setting's choices (`YapConfig::meeting_refine`).
pub const AUTO: &str = "auto";
pub const ALWAYS: &str = "always";
pub const NEVER: &str = "never";

/// A model must beat the live one's accuracy by this much to be worth a
/// second pass (Parakeet V2 over V3 on English: +0.02).
pub const MARGIN: f32 = 0.015;

/// Any language (Whisper's).
const ANY: &[&str] = &["*"];
/// Parakeet V3's and Canary 1B's 25 European languages.
const EUROPEAN: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro",
    "sk", "sl", "es", "sv", "ru", "uk",
];

/// Each speech model's accuracy (0–1, `src/lib/models.js`'s bars: the Open
/// ASR Leaderboard's English WER, the specialists' on their language) and
/// the languages it speaks.
const MODELS: &[(&str, f32, &[&str])] = &[
    ("parakeet-tdt-0.6b-v3", 0.88, EUROPEAN),
    ("parakeet-tdt-0.6b-v2", 0.90, &["en"]),
    ("turbo", 0.80, ANY),
    ("large", 0.81, ANY),
    ("distil-large-v3.5", 0.80, &["en"]),
    ("medium", 0.76, ANY),
    ("small", 0.65, ANY),
    ("canary-1b-v2", 0.84, EUROPEAN),
    ("cohere-int8", 0.93, &["en", "fr", "de", "it", "es", "pt", "el", "nl", "pl", "zh", "ja", "ko", "vi", "ar"]),
    ("sense-voice-int8", 0.72, &["zh", "en", "ja", "ko", "yue"]),
    ("canary-180m-flash", 0.77, &["en", "de", "es", "fr"]),
    ("gigaam-v3-e2e-ctc", 0.88, &["ru"]),
    ("breeze-asr", 0.84, &["zh"]),
    ("moonshine-base", 0.60, &["en"]),
];

fn model(id: &str) -> Option<&'static (&'static str, f32, &'static [&'static str])> {
    MODELS.iter().find(|m| m.0 == id)
}

fn speaks(langs: &[&str], lang: &str) -> bool {
    langs.contains(&"*") || langs.contains(&lang)
}

/// The most accurate of `installed` for a meeting in `language` (`None`:
/// not known — then one that speaks every language `live` does), if it
/// beats `live` by [`MARGIN`]. Pure.
pub fn best_model(live: &str, installed: &[&str], language: Option<&str>) -> Option<&'static str> {
    let (_, live_acc, live_langs) = *model(live)?;
    let fits = |langs: &[&str]| match language {
        Some(lang) => speaks(langs, lang),
        None => live_langs.iter().all(|l| speaks(langs, l)),
    };
    installed
        .iter()
        .filter_map(|id| model(id))
        .filter(|(id, acc, langs)| *id != live && *acc >= live_acc + MARGIN && fits(langs))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|m| m.0)
}

/// Why a meeting is refined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Chunks of the live transcript failed.
    Failed,
    /// A more accurate model is installed.
    Better,
    /// Settings says Always.
    Always,
}

/// A refine to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The model (a registry id).
    pub model: String,
    pub reason: Reason,
    /// Before the summary (the live transcript has holes), not after.
    pub first: bool,
}

/// Whether, and how, to refine a meeting that just ended (see the module
/// notes): `choice` the setting, `live` the model of the live transcript,
/// `best` [`best_model`]'s answer, `failed` the failed chunks' markers in
/// the note, `audio` whether its audio is there. Pure.
pub fn decide(choice: &str, live: &str, best: Option<&str>, failed: usize, audio: bool) -> Option<Plan> {
    if !audio || choice == NEVER {
        return None;
    }
    let model = best.unwrap_or(live).to_string();
    if failed > 0 {
        return Some(Plan { model, reason: Reason::Failed, first: true });
    }
    match (best, choice) {
        (Some(_), _) => Some(Plan { model, reason: Reason::Better, first: false }),
        (None, ALWAYS) => Some(Plan { model, reason: Reason::Always, first: false }),
        _ => None,
    }
}

/// The models installed on this PC.
fn installed() -> Vec<&'static str> {
    let data = crate::config::data_dir();
    crate::stt::all_model_ids().into_iter().filter(|id| crate::stt::is_model_installed(&data, id)).collect()
}

/// The meeting's language as Yap knows it: the one picked for speech, else
/// Windows' (About you's region). `None`: not known.
fn language(cfg: &crate::config::YapConfig) -> Option<String> {
    let picked = cfg.selected_language.trim();
    if !picked.is_empty() && picked != "auto" {
        return Some(crate::about_you::split_tag(picked).0);
    }
    let region = crate::about_you::profile(cfg).region;
    region.language.map(|tag| crate::about_you::split_tag(&tag).0).filter(|l| !l.is_empty())
}

/// Whether a meeting ending now might be refined (its audio is held for
/// it when "Keep meeting audio" is off: `Recorder::finish`). Failed chunks
/// aside, which the recorder counts itself.
pub fn may_refine(cfg: &crate::config::YapConfig) -> bool {
    match cfg.meeting_refine.as_str() {
        NEVER => false,
        ALWAYS => true,
        _ => best_model(&cfg.model_size, &installed(), language(cfg).as_deref()).is_some(),
    }
}

/// The refine due for note `note_id`, which just ended, if any.
pub fn plan(note_id: u64) -> Option<Plan> {
    let cfg = crate::config::load();
    let note = crate::notes::get(note_id)?;
    let recordings = crate::meetings::meeting_audio::recordings(note_id);
    // The live pass's model: its least accurate recording's (a model
    // switched between a pause and a resume), else the one in use.
    let live = recordings
        .iter()
        .map(|m| m.model.as_str())
        .filter(|m| !m.is_empty())
        .min_by(|a, b| {
            let acc = |id: &str| model(id).map_or(0.0, |m| m.1);
            acc(a).total_cmp(&acc(b))
        })
        .unwrap_or(&cfg.model_size)
        .to_string();
    let best = best_model(&live, &installed(), language(&cfg).as_deref());
    let failed = note.transcript.iter().filter(|s| s.failed).count();
    let plan = decide(&cfg.meeting_refine, &live, best, failed, !recordings.is_empty());
    tracing::info!(note_id, choice = %cfg.meeting_refine, live = %live, best = ?best, failed, plan = ?plan.as_ref().map(|p| (&p.model, p.reason, p.first)), "Meeting refine: decided");
    plan
}

/// Start `plan` for note `note_id` (published at once; it waits its turn:
/// `meeting_audio::start_refine`). With `plan.first`, the end-of-meeting
/// steps (`meeting_end::end_steps`, from the window `origin`) follow it,
/// however it ends. `false`: it couldn't start (and nothing follows).
pub fn start(app: &AppHandle, note_id: u64, plan: &Plan, origin: Option<String>) -> bool {
    let (first, reason) = (plan.first, plan.reason);
    let started = crate::meetings::meeting_audio::start_refine(app, note_id, &plan.model, first, move |app, result| {
        tracing::info!(note_id, ?reason, ok = result.is_ok(), "Meeting refine: ended");
        let app = app.clone();
        // (Window queries wait on the main thread: not from this task.)
        std::thread::spawn(move || {
            if first {
                crate::meetings::meeting_end::end_steps(&app, note_id, origin.as_deref());
            }
            crate::meetings::meeting_audio::release(&app, note_id);
        });
    });
    match started {
        Ok(()) => {
            tracing::info!(note_id, ?reason, first, model = %plan.model, "Meeting refine: started");
            true
        }
        Err(e) => {
            tracing::info!(note_id, "Meeting refine: not started ({e})");
            false
        }
    }
}

/// Lowercase words (letters and digits, two or more), for [`agrees`].
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(str::to_lowercase)
}

/// Below this share of the live words found again, the refined transcript
/// says something else: it isn't used.
const AGREE: f32 = 0.5;
/// Fewer live words than this: too few to judge.
const AGREE_MIN_WORDS: usize = 30;

/// Whether the refined lines (`redone`) say what the live ones (`old`,
/// those in the recordings done again) did: at least [`AGREE`] of the live
/// words are there again. A model that didn't understand the meeting's
/// language (English-only on a French call) writes other words: the live
/// transcript is kept. Its holes aside, a model on the same audio finds
/// most of the same words. Pure.
pub fn agrees(old: &[TranscriptSegment], redone: &[Redone]) -> Result<(), String> {
    let within = |ts: u64| redone.iter().any(|r| ts * 1000 + 999 >= r.started_ms && ts * 1000 <= r.end_ms + 1000);
    let live: Vec<String> = old.iter().filter(|s| s.is_talk() && within(s.ts)).flat_map(|s| words(&s.text)).collect();
    if live.len() < AGREE_MIN_WORDS {
        return Ok(());
    }
    let mut new: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for w in redone.iter().flat_map(|r| r.lines.iter()).filter(|s| s.is_talk()).flat_map(|s| words(&s.text)) {
        *new.entry(w).or_default() += 1;
    }
    let mut found = 0usize;
    for w in &live {
        if let Some(n) = new.get_mut(w).filter(|n| **n > 0) {
            *n -= 1;
            found += 1;
        }
    }
    let share = found as f32 / live.len() as f32;
    tracing::info!(share, words = live.len(), "Meeting refine: the new lines against the live ones");
    if share < AGREE {
        return Err("The improved transcript didn't match the meeting (another language?), so the live one is kept".to_string());
    }
    Ok(())
}

/// The calling thread at the lowest priority until dropped (Windows): a
/// refine's transcription calls (`meeting::transcribe_chunk`). The
/// runtimes' own worker threads keep theirs; what keeps a refine out of a
/// dictation's way is when it runs (see "How it runs").
pub struct LowPriority {
    #[cfg(windows)]
    before: i32,
}

impl LowPriority {
    pub fn now() -> Self {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority, SetThreadPriority, THREAD_PRIORITY_LOWEST};
            // SAFETY: the current thread's pseudo-handle; nothing is freed.
            unsafe {
                let me = GetCurrentThread();
                let before = GetThreadPriority(me);
                let _ = SetThreadPriority(me, THREAD_PRIORITY_LOWEST);
                LowPriority { before }
            }
        }
        #[cfg(not(windows))]
        LowPriority {}
    }
}

impl Drop for LowPriority {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY};
            // SAFETY: as above; the blocking pool's thread gets its old
            // priority back.
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY(self.before));
            }
        }
    }
}

/// Test mode only: each refine chunk takes `YAP_E2E_REFINE_CHUNK_MS` (the
/// stub engine is instant): a refine long enough for a dictation to land in
/// the middle of it.
#[cfg(debug_assertions)]
pub fn e2e_slow_chunk() {
    if !crate::e2e::active() {
        return;
    }
    if let Some(ms) = std::env::var("YAP_E2E_REFINE_CHUNK_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms.min(10_000)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V3: &str = "parakeet-tdt-0.6b-v3";
    const V2: &str = "parakeet-tdt-0.6b-v2";

    #[test]
    fn the_best_model_speaks_the_meeting_s_language_and_beats_the_live_one() {
        // English: Cohere over Parakeet V2 over V3.
        assert_eq!(best_model(V3, &[V3, V2, "cohere-int8"], Some("en")), Some("cohere-int8"));
        assert_eq!(best_model(V3, &[V3, V2], Some("en")), Some(V2));
        // German: Parakeet V2 (English only) never; Cohere speaks it.
        assert_eq!(best_model(V3, &[V3, V2], Some("de")), None);
        assert_eq!(best_model(V3, &[V3, V2, "cohere-int8"], Some("de")), Some("cohere-int8"));
        // Language unknown: only a model that speaks all of V3's languages.
        assert_eq!(best_model(V3, &[V3, V2, "cohere-int8", "large"], None), None);
        assert_eq!(best_model("small", &["small", "large"], None), Some("large"));
        // Not more accurate (Whisper Large after Parakeet), or by too little.
        assert_eq!(best_model(V3, &[V3, "large", "turbo"], Some("en")), None);
        assert_eq!(best_model("large", &["large", "turbo"], Some("en")), None);
        // Nothing else installed, or a model Yap doesn't know.
        assert_eq!(best_model(V3, &[V3], Some("en")), None);
        assert_eq!(best_model("my-own", &[V3], Some("en")), None);
    }

    #[test]
    fn the_refine_runs_when_it_helps_always_or_never() {
        // Automatically: failed chunks (first, before the summary)…
        let p = decide(AUTO, V3, None, 2, true).unwrap();
        assert_eq!((p.model.as_str(), p.reason, p.first), (V3, Reason::Failed, true));
        let p = decide(AUTO, V3, Some("cohere-int8"), 1, true).unwrap();
        assert_eq!((p.model.as_str(), p.reason, p.first), ("cohere-int8", Reason::Failed, true));
        // …or a better model (after the summary)…
        let p = decide(AUTO, V3, Some(V2), 0, true).unwrap();
        assert_eq!((p.model.as_str(), p.reason, p.first), (V2, Reason::Better, false));
        // …else nothing.
        assert_eq!(decide(AUTO, V3, None, 0, true), None);
        // Always: on the live model when it's the best.
        let p = decide(ALWAYS, V3, None, 0, true).unwrap();
        assert_eq!((p.model.as_str(), p.reason, p.first), (V3, Reason::Always, false));
        // Never, whatever happened; and never without the audio.
        assert_eq!(decide(NEVER, V3, Some(V2), 3, true), None);
        assert_eq!(decide(ALWAYS, V3, Some(V2), 3, false), None);
    }

    fn talk(text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment { source: "them".into(), text: text.into(), ts, ..Default::default() }
    }

    #[test]
    fn a_refine_that_says_something_else_is_not_used() {
        let t0 = 1_761_000_000_000u64;
        let said = "we agreed the budget review moves to Friday and Morgan sends the draft to the finance team before then so everyone has time to read it properly";
        let old = vec![talk(said, t0 / 1000 + 5), talk(said, t0 / 1000 + 20)];
        let redone = |text: &str| vec![Redone { started_ms: t0, end_ms: t0 + 60_000, lines: vec![talk(text, t0 / 1000 + 5), talk(text, t0 / 1000 + 20)] }];
        // The same meeting, a few words better: used.
        let better = said.replace("Morgan sends", "Morgan will send").replace("so everyone", "so that everyone");
        assert!(agrees(&old, &redone(&better)).is_ok());
        // An English-only model on a call in another language: kept out.
        let other = "wir haben vereinbart dass die Budgetprüfung auf Freitag verschoben wird und Morgan schickt den Entwurf";
        assert!(agrees(&old, &redone(other)).is_err());
        // Too little said to judge.
        assert!(agrees(&[talk("hello there", t0 / 1000)], &redone("something else")).is_ok());
        // Lines from another recording (no audio) don't count.
        let elsewhere = vec![talk(said, t0 / 1000 - 3_600)];
        assert!(agrees(&elsewhere, &redone(other)).is_ok());
    }
}
