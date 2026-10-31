//! **Dictation from the phone** (Yap Link, protocol v1's `start {mode:
//! "dictate", target}`): the phone is the microphone of an ordinary
//! dictation on this PC (`dictation/pipeline/phone.rs`) — "Dictate to PC"
//! pastes into the window in front when Talk was pressed, "Dictate to
//! phone" sends the text back for the phone to copy. What's here: which
//! talk a `start` is, who may dictate and when ([`refusal`]), the words the
//! phone shows for what became of it ([`outcome`]), and the phone's end of
//! a dictation ([`Dictating`]: the pipeline's news, as messages, while it's
//! the phone's current one). Pure but for [`Dictating`].

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::dictation::pipeline::{Delivered, PhoneBusy, PhoneNews, PhoneSink, PhoneTarget};
use crate::link::voice::{say, Tx};
use crate::link::wire::{Out, Phase};

/// What a `start` begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Talk {
    /// A question for the Voice Assistant (`mode` "ask", or an older page's
    /// start with none).
    Ask,
    /// A dictation, to the PC or back to the phone.
    Dictate(PhoneTarget),
}

/// Words the phone shows.
pub const NOT_ALLOWED: &str =
    "This phone isn't allowed to type into your PC. In Yap on the PC, turn on “May type into this PC” for it in Settings → Phone.";
pub const MEETING: &str = "Yap is taking meeting notes on your PC, so it can't take a dictation now.";
pub const PC_DICTATING: &str = "Yap is already dictating on your PC. Try again once that's done.";
pub const FINISHING: &str = "Yap is still finishing the last dictation on your PC. Try again in a moment.";
pub const NO_SPEECH: &str = "Yap's speech model isn't ready on the PC.";
pub const CANCELLED: &str = "Cancelled on your PC.";
pub const NOTHING_HEARD: &str = "Yap didn't catch anything. Try again.";
const UNKNOWN_MODE: &str = "Yap on your PC doesn't know that kind of talk. Update Yap on your PC, then try again.";
const UNKNOWN_TARGET: &str = "Yap on your PC doesn't know where to send that dictation. Update Yap on your PC, then try again.";

/// `start`'s `mode` and `target`, read. An older page's start (no mode)
/// is a question; a dictation goes to the PC unless it says the phone.
pub fn talk(mode: &str, target: &str) -> Result<Talk, &'static str> {
    match mode.trim() {
        "" | "ask" => Ok(Talk::Ask),
        "dictate" => match target.trim() {
            "" | "pc" => Ok(Talk::Dictate(PhoneTarget::Pc)),
            "phone" => Ok(Talk::Dictate(PhoneTarget::Phone)),
            _ => Err(UNKNOWN_TARGET),
        },
        _ => Err(UNKNOWN_MODE),
    }
}

/// Why a phone's dictation going `to` can't start now, if it can't: "Dictate
/// to PC" needs the phone's "May type into this PC" (`may_type`); neither
/// starts while a meeting records (`meeting`), nor while a dictation is
/// under way on the PC (`busy`). Pure.
pub fn refusal(to: PhoneTarget, may_type: bool, meeting: bool, busy: Option<PhoneBusy>) -> Option<&'static str> {
    if to == PhoneTarget::Pc && !may_type {
        return Some(NOT_ALLOWED);
    }
    if meeting {
        return Some(MEETING);
    }
    match busy {
        Some(PhoneBusy::Dictating) => Some(PC_DICTATING),
        Some(PhoneBusy::Finishing) => Some(FINISHING),
        None => None,
    }
}

/// The wire's name for where a dictation went.
pub fn target_name(to: PhoneTarget) -> &'static str {
    match to {
        PhoneTarget::Pc => "pc",
        PhoneTarget::Phone => "phone",
    }
}

/// Apps as people know them, for "Pasted into Notepad" (the same names as
/// the PC's own list, `src/lib/core/appNames.js`).
const KNOWN: &[(&str, &str)] = &[
    ("ms-teams", "Teams"),
    ("msteams", "Teams"),
    ("teams", "Teams"),
    ("msedge", "Edge"),
    ("chrome", "Chrome"),
    ("firefox", "Firefox"),
    ("brave", "Brave"),
    ("opera", "Opera"),
    ("vivaldi", "Vivaldi"),
    ("arc", "Arc"),
    ("code", "VS Code"),
    ("code - insiders", "VS Code Insiders"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("devenv", "Visual Studio"),
    ("idea64", "IntelliJ IDEA"),
    ("pycharm64", "PyCharm"),
    ("rider64", "Rider"),
    ("sublime_text", "Sublime Text"),
    ("notepad++", "Notepad++"),
    ("notepad", "Notepad"),
    ("windowsterminal", "Windows Terminal"),
    ("wt", "Windows Terminal"),
    ("powershell", "PowerShell"),
    ("pwsh", "PowerShell"),
    ("cmd", "Command Prompt"),
    ("conhost", "Console"),
    ("mintty", "Git Bash"),
    ("wezterm-gui", "WezTerm"),
    ("alacritty", "Alacritty"),
    ("winword", "Word"),
    ("excel", "Excel"),
    ("powerpnt", "PowerPoint"),
    ("outlook", "Outlook"),
    ("olk", "Outlook"),
    ("onenote", "OneNote"),
    ("slack", "Slack"),
    ("discord", "Discord"),
    ("whatsapp", "WhatsApp"),
    ("whatsapp.root", "WhatsApp"),
    ("telegram", "Telegram"),
    ("signal", "Signal"),
    ("zoom", "Zoom"),
    ("obsidian", "Obsidian"),
    ("notion", "Notion"),
    ("claude", "Claude"),
    ("chatgpt", "ChatGPT"),
    ("explorer", "File Explorer"),
    ("thunderbird", "Thunderbird"),
    ("spotify", "Spotify"),
    ("yap", "Yap"),
];

/// An app's name from its process ("winword.exe" → "Word", "myapp.exe" →
/// "Myapp"); `None` when there's no process. Pure.
pub fn app_label(process: &str) -> Option<String> {
    let base = process.trim();
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base).trim();
    if base.is_empty() {
        return None;
    }
    let lower = base.to_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(k, _)| *k == lower) {
        return Some((*name).to_string());
    }
    let mut chars = base.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// Where a dictation to the PC landed, by name: an app, or one of Yap's own
/// boxes. Pure.
fn landed_in(report: &Delivered) -> Option<String> {
    match report.kind.as_str() {
        "the Chat view" => Some("Yap's Chat".into()),
        "the quick note" => Some("Yap's quick note".into()),
        _ => app_label(&report.app),
    }
}

/// Why a dictation to the PC wasn't pasted, in words the phone shows. Pure.
fn not_pasted(report: &Delivered) -> String {
    if report.pasted && report.left_on_clipboard {
        // Pasted in steps, stopped part-way (`compose.rs`).
        return "Yap stopped part-way: the rest is copied on the PC".into();
    }
    match report.left_why {
        Some("noTextBox") => "No text box in front: copied on the PC instead".into(),
        Some("admin") => "The app in front runs as administrator: copied on the PC instead".into(),
        Some("keysBlocked") => "Windows didn't let Yap type just then: copied on the PC instead".into(),
        Some("notRead") => "The app in front didn't take the paste: copied on the PC instead".into(),
        _ if report.left_on_clipboard => "The window you were in has gone: copied on the PC instead".into(),
        _ => "The paste didn't go through on the PC".into(),
    }
}

/// **What the phone is told** when its dictation going `to` is done (or
/// ended without text), then `state idle`. A dictation's `heard` carries
/// the text as it was pasted (fitted, cleaned up, corrected) and `to`; one
/// to the PC adds `pasted`. Nothing heard: an `error`. Pure.
pub fn outcome(news: &PhoneNews, to: PhoneTarget) -> Vec<Out> {
    let heard = |text: &str| Out::Heard { text: text.to_string(), to: Some(target_name(to).to_string()) };
    let error = |m: &str| Out::Error { message: m.to_string() };
    match news {
        PhoneNews::Transcribing => vec![Out::State { phase: Phase::Transcribing }],
        PhoneNews::Failed(why) => vec![error(why), Out::State { phase: Phase::Idle }],
        PhoneNews::Cancelled => vec![error(CANCELLED), Out::State { phase: Phase::Idle }],
        PhoneNews::Delivered(report) => {
            let text = report.text.trim();
            let mut out = Vec::new();
            match to {
                PhoneTarget::Phone if text.is_empty() => out.push(error(NOTHING_HEARD)),
                PhoneTarget::Phone => out.push(heard(text)),
                // Only "press enter" said: the key pressed, nothing typed.
                PhoneTarget::Pc if text.is_empty() && report.submit.is_some() && report.error.is_none() => {
                    out.push(Out::Pasted { ok: true, app: landed_in(report), reason: None })
                }
                PhoneTarget::Pc if text.is_empty() => out.push(error(NOTHING_HEARD)),
                PhoneTarget::Pc => {
                    out.push(heard(text));
                    out.push(if report.pasted && !report.left_on_clipboard {
                        Out::Pasted { ok: true, app: landed_in(report), reason: None }
                    } else {
                        Out::Pasted { ok: false, app: None, reason: Some(not_pasted(report)) }
                    });
                }
            }
            out.push(Out::State { phase: Phase::Idle });
            out
        }
    }
}

/// **The phone's end of a dictation**: the pipeline's news
/// ([`PhoneSink`]) said to the phone while this is its current talk
/// (`latest` is still `id`) and the phone hasn't called it off.
pub struct Dictating {
    id: u64,
    latest: Arc<AtomicU64>,
    tx: Tx,
    to: PhoneTarget,
    /// Its recording in the pipeline (`PhoneMic`).
    pub session: AtomicU64,
    /// Still recording: the phone's up frames are its.
    listening: AtomicBool,
    /// The phone's Cancel came (or it left).
    called_off: AtomicBool,
}

impl Dictating {
    pub fn new(id: u64, latest: Arc<AtomicU64>, tx: Tx, to: PhoneTarget) -> Arc<Dictating> {
        Arc::new(Dictating {
            id,
            latest,
            tx,
            to,
            session: AtomicU64::new(0),
            listening: AtomicBool::new(true),
            called_off: AtomicBool::new(false),
        })
    }

    pub fn session(&self) -> u64 {
        self.session.load(Ordering::SeqCst)
    }

    /// Still recording (its frames go to the pipeline).
    pub fn listening(&self) -> bool {
        self.listening.load(Ordering::SeqCst) && !self.called_off.load(Ordering::SeqCst)
    }

    /// The phone's Cancel (or it left): nothing more is said, and nothing
    /// is pasted if it hasn't been yet.
    pub fn call_off(&self) {
        self.called_off.store(true, Ordering::SeqCst);
        self.listening.store(false, Ordering::SeqCst);
    }

    fn current(&self) -> bool {
        self.latest.load(Ordering::SeqCst) == self.id && !self.called_off.load(Ordering::SeqCst)
    }
}

impl PhoneSink for Dictating {
    fn tell(&self, news: PhoneNews) {
        self.listening.store(false, Ordering::SeqCst);
        if !self.current() {
            return;
        }
        if let PhoneNews::Delivered(r) = &news {
            tracing::info!(to = target_name(self.to), pasted = r.pasted, chars = r.text.chars().count(), "Yap Link: the phone's dictation done");
        }
        for out in outcome(&news, self.to) {
            say(&self.tx, out);
        }
    }

    fn called_off(&self) -> bool {
        self.called_off.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_is_a_question_or_a_dictation_and_older_pages_ask() {
        assert_eq!(talk("ask", ""), Ok(Talk::Ask));
        assert_eq!(talk("", ""), Ok(Talk::Ask), "an older page's start");
        assert_eq!(talk("dictate", "pc"), Ok(Talk::Dictate(PhoneTarget::Pc)));
        assert_eq!(talk("dictate", ""), Ok(Talk::Dictate(PhoneTarget::Pc)), "to the PC unless it says");
        assert_eq!(talk("dictate", "phone"), Ok(Talk::Dictate(PhoneTarget::Phone)));
        assert!(talk("dictate", "fridge").is_err());
        assert!(talk("meeting", "").is_err(), "not a phone's to start yet");
    }

    #[test]
    fn who_may_dictate_and_when() {
        use PhoneTarget::*;
        // Typing into the PC needs the phone's permission; its own dictation doesn't.
        assert_eq!(refusal(Pc, false, false, None), Some(NOT_ALLOWED));
        assert_eq!(refusal(Phone, false, false, None), None);
        assert_eq!(refusal(Pc, true, false, None), None);
        // Never during a meeting, either way.
        assert_eq!(refusal(Pc, true, true, None), Some(MEETING));
        assert_eq!(refusal(Phone, true, true, None), Some(MEETING));
        // Nor beside a dictation on the PC.
        assert_eq!(refusal(Pc, true, false, Some(PhoneBusy::Dictating)), Some(PC_DICTATING));
        assert_eq!(refusal(Phone, false, false, Some(PhoneBusy::Finishing)), Some(FINISHING));
        // The permission is said first: it's the one to fix.
        assert_eq!(refusal(Pc, false, true, Some(PhoneBusy::Dictating)), Some(NOT_ALLOWED));
    }

    #[test]
    fn apps_by_the_names_people_know() {
        assert_eq!(app_label("notepad.exe").as_deref(), Some("Notepad"));
        assert_eq!(app_label("WINWORD.EXE").as_deref(), Some("Word"));
        assert_eq!(app_label("ms-teams.exe").as_deref(), Some("Teams"));
        assert_eq!(app_label("myapp.exe").as_deref(), Some("Myapp"));
        assert_eq!(app_label(""), None);
        assert_eq!(app_label(" .exe"), None);
    }

    fn report(text: &str) -> Delivered {
        Delivered { text: text.into(), raw: text.into(), app: "notepad.exe".into(), pasted: true, ..Delivered::default() }
    }

    #[test]
    fn a_dictation_to_the_pc_says_what_was_pasted_and_where() {
        let r = PhoneNews::Delivered(Box::new(report("Send the report on Friday.")));
        assert_eq!(
            outcome(&r, PhoneTarget::Pc),
            vec![
                Out::Heard { text: "Send the report on Friday.".into(), to: Some("pc".into()) },
                Out::Pasted { ok: true, app: Some("Notepad".into()), reason: None },
                Out::State { phase: Phase::Idle },
            ]
        );
        // No text box in front: on the clipboard, said plainly.
        let left = Delivered { pasted: false, left_on_clipboard: true, left_why: Some("noTextBox"), ..report("Hello.") };
        let out = outcome(&PhoneNews::Delivered(Box::new(left)), PhoneTarget::Pc);
        assert_eq!(
            out[1],
            Out::Pasted { ok: false, app: None, reason: Some("No text box in front: copied on the PC instead".into()) }
        );
        // An app as administrator, a window gone, a paste that failed.
        let admin = Delivered { pasted: false, left_on_clipboard: true, left_why: Some("admin"), ..report("Hi.") };
        assert!(matches!(&outcome(&PhoneNews::Delivered(Box::new(admin)), PhoneTarget::Pc)[1], Out::Pasted { ok: false, reason: Some(r), .. } if r.contains("administrator")));
        let gone = Delivered { pasted: false, left_on_clipboard: true, error: Some("the dictation's window is gone".into()), ..report("Hi.") };
        assert!(matches!(&outcome(&PhoneNews::Delivered(Box::new(gone)), PhoneTarget::Pc)[1], Out::Pasted { ok: false, reason: Some(r), .. } if r.contains("has gone")));
        let failed = Delivered { pasted: false, error: Some("Inject task panicked".into()), ..report("Hi.") };
        assert!(matches!(&outcome(&PhoneNews::Delivered(Box::new(failed)), PhoneTarget::Pc)[1], Out::Pasted { ok: false, reason: Some(r), .. } if r.contains("didn't go through")));
        // Into Yap's own Chat box (it had the focus here).
        let chat = Delivered { kind: "the Chat view".into(), app: "yap.exe".into(), ..report("What's on today?") };
        assert!(matches!(&outcome(&PhoneNews::Delivered(Box::new(chat)), PhoneTarget::Pc)[1], Out::Pasted { ok: true, app: Some(a), .. } if a == "Yap's Chat"));
        // An app Yap couldn't name: pasted, no name.
        let nameless = Delivered { app: String::new(), ..report("Hi.") };
        assert_eq!(outcome(&PhoneNews::Delivered(Box::new(nameless)), PhoneTarget::Pc)[1], Out::Pasted { ok: true, app: None, reason: None });
    }

    #[test]
    fn a_dictation_to_the_phone_comes_back_with_no_paste() {
        let r = Delivered { kind: "your phone".into(), app: "phone".into(), pasted: false, ..report("Milk, eggs and bread.") };
        assert_eq!(
            outcome(&PhoneNews::Delivered(Box::new(r)), PhoneTarget::Phone),
            vec![Out::Heard { text: "Milk, eggs and bread.".into(), to: Some("phone".into()) }, Out::State { phase: Phase::Idle }]
        );
    }

    #[test]
    fn nothing_heard_failed_and_cancelled_end_in_words_then_idle() {
        let empty = PhoneNews::Delivered(Box::new(Delivered { pasted: false, ..report("") }));
        for to in [PhoneTarget::Pc, PhoneTarget::Phone] {
            let out = outcome(&empty, to);
            assert!(matches!(&out[0], Out::Error { .. }), "nothing heard: said");
            assert_eq!(out.last(), Some(&Out::State { phase: Phase::Idle }));
        }
        // Only "press enter": pressed, nothing typed.
        let enter = Delivered { submit: Some("enter".into()), pasted: false, ..report("") };
        assert_eq!(outcome(&PhoneNews::Delivered(Box::new(enter)), PhoneTarget::Pc)[0], Out::Pasted { ok: true, app: Some("Notepad".into()), reason: None });
        assert_eq!(
            outcome(&PhoneNews::Failed("No model."), PhoneTarget::Pc),
            vec![Out::Error { message: "No model.".into() }, Out::State { phase: Phase::Idle }]
        );
        assert_eq!(outcome(&PhoneNews::Cancelled, PhoneTarget::Pc)[0], Out::Error { message: CANCELLED.into() });
        assert_eq!(outcome(&PhoneNews::Transcribing, PhoneTarget::Pc), vec![Out::State { phase: Phase::Transcribing }]);
    }

    #[test]
    fn an_old_or_called_off_dictation_says_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let latest = Arc::new(AtomicU64::new(3));
        let d = Dictating::new(3, Arc::clone(&latest), tx, PhoneTarget::Phone);
        assert!(d.listening());
        d.tell(PhoneNews::Transcribing);
        assert!(!d.listening(), "transcribing: its frames are over");
        assert!(rx.try_recv().is_ok(), "the current one is heard");
        // A newer talk started: this one's news is dropped.
        latest.store(4, Ordering::SeqCst);
        d.tell(PhoneNews::Failed("x"));
        assert!(rx.try_recv().is_err());
        // Called off: dropped, and its paste is stopped.
        latest.store(3, Ordering::SeqCst);
        d.call_off();
        assert!(PhoneSink::called_off(d.as_ref()));
        d.tell(PhoneNews::Cancelled);
        assert!(rx.try_recv().is_err());
    }
}
