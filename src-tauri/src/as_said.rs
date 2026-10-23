//! **"Give me what I actually said"** (2026-10-22): the market's top
//! complaint about AI dictation is cleanup rewording you, so right after a
//! dictation whose words AI cleanup changed, what Yap pasted can be swapped
//! for the words **as said** — the transcript with the corrections, the
//! "press enter" command and snippets, no cleanup (`pipeline::
//! as_said_text`), fitted to the same place as the paste
//! (`text_injector::prepare_dictation_as_said`). (Windows' fluid dictation
//! has "Revert"; TypeWhisper "Restore Raw Transcript".)
//!
//! **Three ways in**: the "Use what I said" shortcut (`config.
//! as_said_hotkey`, unbound by default, Settings → Shortcuts), the Yap
//! bar's capsule for [`OFFER_MS`] after a cleaned dictation ("Cleaned up ·
//! Use what I said", `yap-state` "cleaned"; `config.as_said_offer`, on —
//! only when cleanup changed the words, [`words_differ`]), and the tray's
//! "Paste last transcript (as said)" (a plain paste where you are, not a
//! swap: `text_injector`).
//!
//! **Replacing the paste, safely** ([`swap`]): only within [`SWAP_WITHIN`]
//! of the dictation, while **its window is still in front**, never in a
//! terminal (its line can't be selected) or an app running as
//! administrator — and only once Yap has **selected exactly what it
//! pasted** in the focused field of that app, read over UI Automation:
//! - a **Win32 edit control** (a WinForms box, EDIT, RichEdit): its text
//!   (ValuePattern) and caret (`EM_GETSEL`); the pasted text must end at the
//!   caret (else be in the field exactly once); `EM_SETSEL` selects it and
//!   `EM_GETSEL` must say so;
//! - a **TextPattern** field (browsers, Notepad, Word…): the text just
//!   before the caret must end with the pasted text (else `FindText` must
//!   find it exactly once); a range over it ([`unit_counts`]: the field's
//!   own idea of a character is tried until its text reads back exactly the
//!   paste), `Select()`ed, and the field's selection must read back as it.
//!
//! Then the words as said are pasted over the selection (the dictation's
//! own private, verified paste, back into its own window or nowhere), and
//! the field is read again to check they landed ([`Outcome::Swapped`]'s
//! `checked`). Line breaks, a no-break space and whitespace Yap added at
//! the end are matched loosely ([`match_before`]). Yap never uses the app's
//! own Undo: it can't tell what Undo would take back.
//!
//! **Where it refuses** — the window isn't in front, a terminal, an admin
//! app, a password box or read-only field, a field UI Automation can't read
//! or select in, the pasted text edited or moved — it does nothing to the
//! app and puts the words as said on the clipboard instead (privately, as a
//! paste: out of Win+V history), saying "Couldn't replace it here — copied
//! the raw text instead" (`alert.rs`, kind "pasteBlocked").
//!
//! Test runs touch nothing (`realtest::gate`: a log line, the text
//! returned); robot mode only the test's own windows.
//!
//! **The last paste** ([`remember`], 2026-10-23): Yap keeps the last text
//! it pasted into an app's field — a dictation, or a Command's or a
//! rewrite's answer — with its window, its kind of app and when; beside it
//! the words as said when cleanup changed them, and its history entry
//! ([`recorded`]). Sent with a submit key, or a paste that didn't land:
//! forgotten. Three things use it:
//! - "Use what I said" (above);
//! - **Home's "Paste what I said"** ([`paste_entry`]): on the dictation Yap
//!   still remembers, within [`SWAP_WITHIN`] and not swapped yet, Yap brings
//!   its window back to the front and swaps it exactly as above, every
//!   check included; on any other entry — or where the swap refuses — the
//!   words as said are copied (privately) and Yap says so;
//! - **rewriting the last dictation** (the rewrite shortcut with nothing
//!   selected; Wispr Flow's Command Mode and Transforms, by behaviour):
//!   within [`REWRITE_WITHIN`] of a paste into the window in front, not a
//!   terminal ([`rewritable`], [`rewrite_target`] at the key press), the
//!   spoken instruction is applied to that paste instead of writing new
//!   text. Stricter than the swap: the paste must still **end exactly at
//!   the caret** — typed after, edited, or the cursor moved away refuses,
//!   since the instruction may no longer be about it. Checked before the AI
//!   is asked ([`rewrite_check`]: refused → "Couldn't rewrite your last
//!   dictation", nothing pasted) and again when it answers
//!   ([`replace_last`]: selected exactly, pasted over with the spaces
//!   around it kept, read back; refused → the field left alone and the
//!   rewrite copied instead). The rewrite is then the last paste, so "now
//!   shorter" works on it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::app_kind::AppKind;

/// How long after a dictation its paste can still be swapped.
pub const SWAP_WITHIN: Duration = Duration::from_secs(120);
/// How long after a paste the rewrite shortcut, with nothing selected,
/// rewrites it instead of writing new text (Wispr Flow's minute): time to
/// read it and ask, short enough that "make it more formal" is still about
/// it.
pub const REWRITE_WITHIN: Duration = Duration::from_secs(60);
/// How long the bar offers it.
pub const OFFER_MS: u64 = 6_000;
/// How long a field gets to show a selection or the new text before Yap
/// reads it back.
const SETTLE: Duration = Duration::from_millis(120);
/// How long (in all) a selection is read back for…
const SELECT_WAIT: Duration = Duration::from_millis(1_500);
/// …and the swapped text.
const CHECK_WAIT: Duration = Duration::from_millis(1_000);

const NOTHING: &str = "Nothing to swap: Use what I said works right after a dictation AI cleanup changed";
/// The swap's refusal when the paste isn't where Yap left it.
const MOVED: &str = "the text Yap pasted has changed or moved";
/// The rewrite's, which wants the cursor right after it too.
const EDITED_SINCE: &str = "it's been changed, or the cursor isn't right after it any more";

/// The last text Yap pasted into an app's field (see the module comment).
#[derive(Debug, Clone)]
pub(crate) struct Pasted {
    seq: u64,
    /// The window it went into (`None`: one of Yap's own, or none — never
    /// swapped or rewritten there).
    hwnd: Option<isize>,
    /// The kind of app as read (its paste keys).
    kind: AppKind,
    /// What was pasted (fitted).
    pasted: String,
    /// The words as said, fitted to the same place, when AI cleanup
    /// changed them: what "Use what I said" swaps it for.
    said: Option<String>,
    /// Its history entry (`ts`), once recorded ([`recorded`]).
    entry: Option<u64>,
    at: Instant,
    restore_clipboard: bool,
}

impl Pasted {
    /// What was pasted, as the AI is to rewrite it.
    pub(crate) fn text(&self) -> &str {
        self.pasted.trim()
    }

    /// How long ago it was pasted.
    pub(crate) fn age(&self) -> Duration {
        self.at.elapsed()
    }
}

static LAST: Mutex<Option<Pasted>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);

fn last() -> MutexGuard<'static, Option<Pasted>> {
    LAST.lock().unwrap_or_else(|p| p.into_inner())
}

/// A text's words, compared: lower case, letters, digits and apostrophes.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .map(|w| w.trim_matches(['\'', '’']).replace('’', "'").to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether cleanup changed the words, not only the punctuation and capitals
/// (the bar offers the swap only then; the shortcut works either way).
pub fn words_differ(cleaned: &str, said: &str) -> bool {
    words(cleaned) != words(said)
}

/// Text just pasted (`pasted`, fitted) into `hwnd`: the last paste from now
/// on. `said`: a dictation's words as said, fitted the same way, when
/// cleanup changed them. Returns its number when there's something to swap
/// for them. Nothing pasted into another app's window and nothing to swap:
/// forgotten.
pub fn remember(hwnd: Option<isize>, kind: AppKind, pasted: &str, said: Option<&str>, restore_clipboard: bool) -> Option<u64> {
    let said = said.filter(|s| *s != pasted && !s.trim().is_empty());
    let mut l = last();
    if pasted.trim().is_empty() || (hwnd.is_none() && said.is_none()) {
        *l = None;
        return None;
    }
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    *l = Some(Pasted {
        seq,
        hwnd,
        kind,
        pasted: pasted.to_string(),
        said: said.map(str::to_string),
        entry: None,
        at: Instant::now(),
        restore_clipboard,
    });
    said.is_some().then_some(seq)
}

/// Nothing Yap pasted is left in a field to swap or rewrite (a paste that
/// didn't land, one sent with a submit key).
pub fn forget() {
    *last() = None;
}

/// The paste just remembered is history's entry `ts` (its text `text`, as
/// history keeps it): Home's "Paste what I said" on that entry can swap it.
pub fn recorded(ts: u64, text: &str) {
    if let Some(p) = last().as_mut() {
        if p.entry.is_none() && p.pasted.trim() == text.trim() {
            p.entry = Some(ts);
        }
    }
}

/// Dictation `seq` is still the one to swap (the bar's offer runs out
/// otherwise).
pub fn offered(seq: u64) -> bool {
    last().as_ref().is_some_and(|p| p.seq == seq && p.said.is_some())
}

/// What "Use what I said" did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum Outcome {
    /// The paste was swapped for the words as said. `checked`: Yap read the
    /// field afterwards and they're there. `how`: "edit" (a Win32 edit
    /// control) or "text" (UI Automation's TextPattern).
    Swapped { checked: bool, how: &'static str, said: String },
    /// Not here (`why`): the words as said are on the clipboard instead.
    /// `told`: Yap has said so already (an alert); else the caller says it.
    Copied { why: String, said: String, told: bool },
    /// A test run: nothing was touched.
    TestMode { said: String },
}

/// Swap the last cleaned dictation's paste for its words as said (see the
/// module comment). Blocking: UI Automation, the paste. `Err`: nothing to
/// swap, or robot mode refused the window.
pub fn swap(app: &AppHandle) -> Result<Outcome, String> {
    let p = last().clone().filter(|p| p.said.is_some()).ok_or(NOTHING)?;
    if p.at.elapsed() > SWAP_WITHIN {
        *last() = None;
        return Err(NOTHING.into());
    }
    if let Some(outcome) = gate_swap(&p)? {
        return Ok(outcome);
    }
    // One swap per dictation, whatever comes of it.
    *last() = None;
    Ok(swap_now(app, &p))
}

/// A swap of `p` in a test run (nothing touched: `Some`), robot mode's
/// refusal (`Err`), or go (`None`).
fn gate_swap(p: &Pasted) -> Result<Option<Outcome>, String> {
    let said = p.said.clone().unwrap_or_default();
    match crate::realtest::gate(p.hwnd, "swap the paste for the words as said") {
        crate::realtest::Gate::Go => Ok(None),
        crate::realtest::Gate::TestMode => {
            *last() = None;
            tracing::info!(chars = said.chars().count(), "e2e: not swapping the paste for the words as said");
            Ok(Some(Outcome::TestMode { said }))
        }
        crate::realtest::Gate::Refused(why) => Err(why),
    }
}

/// Swap `p` (gated already): replaced, else copied and said.
fn swap_now(app: &AppHandle, p: &Pasted) -> Outcome {
    let said = p.said.clone().unwrap_or_default();
    let outcome = match replace_in_field(p, &|_| said.clone(), true) {
        Ok(Put::Replaced { checked, how, .. }) => Outcome::Swapped { checked, how, said },
        // On the clipboard already, and nothing was pasted.
        Ok(Put::OnClipboard) => Outcome::Copied { why: "the window couldn't be reached".into(), said, told: false },
        Err(why) => copy_instead(app, p, why),
    };
    match &outcome {
        Outcome::Swapped { checked, how, .. } => tracing::info!(checked, how, "Use what I said: the paste swapped"),
        Outcome::Copied { why, .. } => tracing::info!(why = %why, "Use what I said: copied instead"),
        Outcome::TestMode { .. } => {}
    }
    outcome
}

/// Home's **Paste what I said** on its entry at `ts` with `text`: the
/// dictation Yap still remembers (within [`SWAP_WITHIN`], not swapped yet)
/// is swapped in its field — its window brought back to the front first,
/// then every check of [`swap`] —; any other, or where that refuses, has
/// its words as said copied (privately). Blocking.
pub fn paste_entry(app: &AppHandle, ts: u64, text: &str) -> Result<Outcome, String> {
    let entry = crate::history::find(ts, text).ok_or("This dictation isn't in your history any more")?;
    let said = entry
        .said
        .filter(|s| !s.trim().is_empty())
        .ok_or("Yap didn't keep what you said for this dictation")?;
    let remembered = last().clone().filter(|p| p.said.is_some() && p.entry == Some(ts) && p.pasted.trim() == text.trim());
    let why = match remembered {
        Some(p) if p.at.elapsed() <= SWAP_WITHIN => {
            if let Some(outcome) = gate_swap(&p)? {
                return Ok(outcome);
            }
            *last() = None;
            let back = p.hwnd.is_some_and(bring_back);
            tracing::info!(back, "Paste what I said: the latest dictation, in its own window");
            if !back {
                return Ok(copy_instead(app, &p, "Yap couldn't bring its window back".into()));
            }
            return Ok(swap_now(app, &p));
        }
        Some(_) => "it's been more than two minutes since you said it",
        None => "Yap only swaps your latest dictation, right after it",
    };
    copy_said(said.trim(), why)
}

/// The words as said on the clipboard (privately), for the caller to say
/// so (Home's toast). A test run never touches the clipboard.
fn copy_said(said: &str, why: &str) -> Result<Outcome, String> {
    let out = || Outcome::Copied { why: why.to_string(), said: said.to_string(), told: false };
    if crate::e2e::active() && !crate::realtest::on() {
        tracing::info!(chars = said.chars().count(), "e2e: not copying the words as said");
        return Ok(out());
    }
    if !crate::text_injector::copy_privately(said) {
        return Err("The clipboard couldn't take the text".into());
    }
    tracing::info!(chars = said.chars().count(), "Paste what I said: copied");
    Ok(out())
}

/// Bring `hwnd` back to the front (Home's "Paste what I said": Yap's own
/// window has it), and give the app a moment to put the focus back in its
/// field. `false`: it didn't come.
fn bring_back(hwnd: isize) -> bool {
    if crate::text_injector::current_foreground() == Some(hwnd) {
        return true;
    }
    if !crate::text_injector::window_exists(hwnd) {
        return false;
    }
    crate::text_injector::focus(hwnd);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(800) {
        if crate::text_injector::current_foreground() == Some(hwnd) {
            std::thread::sleep(SETTLE);
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

// ---- Rewriting the last paste -----------------------------------------------------

/// The rewrite shortcut's rule (pure): with nothing selected, the last
/// paste is what it rewrites when it went into the window in front now
/// (`target`), not a terminal (its line can't be selected), at most
/// [`REWRITE_WITHIN`] ago.
pub fn rewritable(pasted_in: Option<isize>, kind: AppKind, age: Duration, target: Option<isize>) -> bool {
    pasted_in.is_some() && pasted_in == target && !kind.is_terminal() && age <= REWRITE_WITHIN
}

/// The last paste, when the rewrite shortcut — nothing selected in
/// `target` — is to rewrite it ([`rewritable`]). A test run never (it
/// reads no other app); robot mode only in the test's own windows.
pub(crate) fn rewrite_target(target: Option<isize>) -> Option<Pasted> {
    let p = last().clone()?;
    (rewritable(p.hwnd, p.kind, p.at.elapsed(), target) && crate::realtest::permits(p.hwnd)).then_some(p)
}

/// Before the AI is asked: the last paste `p` is still in its field exactly
/// as pasted, the cursor right after it, its window in front (not an admin
/// app). `Err(why)`. Read only; blocking (UI Automation).
pub(crate) fn rewrite_check(p: &Pasted) -> Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = p.hwnd.ok_or("Yap didn't paste it into another app")?;
        usable(hwnd, p.kind)?;
        let pid = crate::text_injector::window_process(hwnd).ok_or("the window is gone")?;
        if uia::ends_at_caret(hwnd, pid, &p.pasted) {
            Ok(())
        } else {
            Err(EDITED_SINCE.into())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        Err("Yap can only do this on Windows".into())
    }
}

/// The rewrite shortcut found nothing selected and its last dictation not
/// as Yap pasted it (`why`): said plainly, nothing pasted.
pub(crate) fn cant_rewrite(app: &AppHandle, why: &str) {
    tracing::info!(why = %why, "Rewrite the last dictation: refused, nothing pasted");
    crate::alert::show_soon(
        app,
        crate::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: "Couldn't rewrite your last dictation".into(),
            body: format!("Yap left it as it is ({why}). Select the text you want to change, then try again."),
            action: None,
            timeout_ms: 12_000,
        },
    );
}

/// What became of a rewrite of the last paste.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum Rewrote {
    /// The last paste was replaced by the rewrite (`text`, as pasted).
    /// `checked`: read back. `how`: "edit" or "text", as for a swap.
    Replaced { checked: bool, how: &'static str, text: String },
    /// Not there any more (`why`): the field was left alone and the rewrite
    /// is on the clipboard (the person told).
    Copied { why: String, text: String },
    /// A test run: nothing was touched.
    TestMode { text: String },
}

/// The AI's `rewrite` of the last paste `p` put in its place: selected
/// exactly (still ending at the caret), pasted over with the spaces around
/// it kept as they were ([`keep_spaces`]), read back — then it's the last
/// paste. Where that can't be done safely the field is left alone and the
/// rewrite goes on the clipboard instead, said. Blocking.
pub(crate) fn replace_last(app: &AppHandle, p: &Pasted, rewrite: &str) -> Result<Rewrote, String> {
    match crate::realtest::gate(p.hwnd, "rewrite the last dictation") {
        crate::realtest::Gate::Go => {}
        crate::realtest::Gate::TestMode => {
            tracing::info!(chars = rewrite.chars().count(), "e2e: not rewriting the last dictation");
            return Ok(Rewrote::TestMode { text: rewrite.to_string() });
        }
        crate::realtest::Gate::Refused(why) => return Err(why),
    }
    let outcome = match replace_in_field(p, &|selected| keep_spaces(selected, rewrite), false) {
        Ok(Put::Replaced { checked, how, text }) => {
            remember(p.hwnd, p.kind, &text, None, p.restore_clipboard);
            Rewrote::Replaced { checked, how, text }
        }
        Ok(Put::OnClipboard) => rewrite_copied(app, "the window couldn't be reached".into(), rewrite, false),
        Err(why) => rewrite_copied(app, why, rewrite, true),
    };
    match &outcome {
        Rewrote::Replaced { checked, how, .. } => tracing::info!(checked, how, "Rewrite the last dictation: replaced"),
        Rewrote::Copied { why, .. } => tracing::info!(why = %why, "Rewrite the last dictation: copied instead"),
        Rewrote::TestMode { .. } => {}
    }
    Ok(outcome)
}

/// The rewrite couldn't take the last paste's place: on the clipboard
/// (privately; `copy`: not there yet), and the person told.
fn rewrite_copied(app: &AppHandle, why: String, rewrite: &str, copy: bool) -> Rewrote {
    let text = rewrite.trim().to_string();
    let copied = !copy || crate::text_injector::copy_privately(&text);
    crate::alert::show_soon(
        app,
        crate::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: if copied {
                "Couldn't replace your last dictation — copied the rewrite instead".into()
            } else {
                "Couldn't replace your last dictation".into()
            },
            body: if copied {
                format!("Yap left your text as it was ({why}). The rewrite is on the clipboard: press Ctrl+V.")
            } else {
                format!("Yap left your text as it was ({why}), and the clipboard couldn't take the rewrite.")
            },
            action: None,
            timeout_ms: 12_000,
        },
    );
    Rewrote::Copied { why, text }
}

/// `rewrite` with the spaces `selected` (what it replaces) had around it,
/// so the text on either side stays as it was.
pub fn keep_spaces(selected: &str, rewrite: &str) -> String {
    if selected.trim().is_empty() {
        return rewrite.trim().to_string();
    }
    let lead = &selected[..selected.len() - selected.trim_start().len()];
    let trail = &selected[selected.trim_end().len()..];
    let body = rewrite.trim();
    if body.is_empty() {
        return String::new();
    }
    format!("{lead}{body}{trail}")
}

/// [`swap`] off the caller's thread (the shortcut, the bar), the bar's
/// offer ended either way.
pub fn swap_soon(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("as-said".into()).spawn(move || {
        // The shortcut's own modifiers let go first: Ctrl+V with Alt still
        // down is another key in most apps.
        wait_for_modifiers_up();
        if let Err(e) = swap(&app) {
            tracing::info!("Use what I said: {e}");
            let _ = app.emit("yap-error", e);
        }
        end_offer(&app);
    });
}

/// Wait (≤ 1.5 s) until Ctrl, Shift, Alt and Win are all up.
fn wait_for_modifiers_up() {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(vkey: i32) -> i16;
        }
        // VK_SHIFT, VK_CONTROL, VK_MENU (Alt), VK_LWIN, VK_RWIN.
        const MODIFIERS: [i32; 5] = [0x10, 0x11, 0x12, 0x5B, 0x5C];
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(1_500) {
            let down = MODIFIERS.iter().any(|&vk| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0);
            if !down {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The bar's "Cleaned up · Use what I said" goes (unless a dictation took
/// its place).
fn end_offer(app: &AppHandle) {
    use tauri::Manager;
    if let Some(state) = app.try_state::<crate::AppState>() {
        if let Ok(guard) = state.pipeline.lock() {
            if guard.as_ref().is_some_and(|p| p.is_busy()) {
                return;
            }
        }
    }
    let _ = app.emit("yap-state", "idle");
}

/// What [`replace_in_field`] did.
enum Put {
    /// The paste was replaced by `text` (`checked`: read back).
    #[cfg_attr(not(windows), allow(dead_code))]
    Replaced { checked: bool, how: &'static str, text: String },
    /// The window couldn't be reached: the text is on the clipboard (the
    /// paste's own fallback) and nothing was pasted.
    #[cfg_attr(not(windows), allow(dead_code))]
    OnClipboard,
}

/// Replace the paste `p` in its field: select exactly what was pasted —
/// ending at the caret, or (`anywhere`) else there exactly once —, paste
/// `with(what was selected)` over it, read it back. `Err(why)`: not here
/// (nothing was changed).
#[cfg(not(windows))]
fn replace_in_field(_p: &Pasted, _with: &dyn Fn(&str) -> String, _anywhere: bool) -> Result<Put, String> {
    Err("Yap can only do this on Windows".into())
}

#[cfg(windows)]
fn replace_in_field(p: &Pasted, with: &dyn Fn(&str) -> String, anywhere: bool) -> Result<Put, String> {
    let hwnd = p.hwnd.ok_or("Yap didn't paste it into another app")?;
    usable(hwnd, p.kind)?;
    let pid = crate::text_injector::window_process(hwnd).ok_or("the window is gone")?;
    let (how, selected) = uia::select_pasted(hwnd, pid, &p.pasted, anywhere)?;
    let text = with(&selected);
    if text.trim().is_empty() {
        return Err("there was nothing to put in its place".into());
    }
    let delivery = tauri::async_runtime::block_on(crate::text_injector::inject_text(
        &text,
        p.restore_clipboard,
        Some(hwnd),
        true,
        p.kind,
    ))?;
    match delivery {
        crate::text_injector::Delivery::Pasted | crate::text_injector::Delivery::Typed => {}
        crate::text_injector::Delivery::LeftOnClipboard | crate::text_injector::Delivery::WindowGone => {
            return Ok(Put::OnClipboard);
        }
    }
    // Read back until the field shows it (an app may take a moment).
    let started = Instant::now();
    let checked = loop {
        std::thread::sleep(SETTLE);
        if uia::ends_at_caret(hwnd, pid, &text) {
            break true;
        }
        if started.elapsed() > CHECK_WAIT {
            break false;
        }
    };
    Ok(Put::Replaced { checked, how, text })
}

/// The paste's window may be touched: still in front, not a terminal (its
/// line can't be selected), not an app running as administrator.
#[cfg(windows)]
fn usable(hwnd: isize, kind: AppKind) -> Result<(), String> {
    if kind.is_terminal() {
        return Err("a terminal's line can't be selected".into());
    }
    let front = crate::text_injector::current_foreground();
    if front != Some(hwnd) {
        tracing::debug!(dictation = hwnd, front = ?front, "The last paste: another window is in front");
        return Err("you've moved on to another window".into());
    }
    if crate::elevated::target_blocked(hwnd) {
        return Err("the app runs as administrator".into());
    }
    Ok(())
}

/// Not here: the words as said on the clipboard (privately), and the person
/// told.
fn copy_instead(app: &AppHandle, p: &Pasted, why: String) -> Outcome {
    let said = p.said.as_deref().unwrap_or_default().trim().to_string();
    let copied = crate::text_injector::copy_privately(&said);
    crate::alert::show_soon(
        app,
        crate::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: if copied {
                "Couldn't replace it here — copied the raw text instead".into()
            } else {
                "Couldn't replace it here".into()
            },
            body: if copied {
                format!("Yap left what it pasted alone ({why}). What you said is on the clipboard: press Ctrl+V.")
            } else {
                format!("Yap left what it pasted alone ({why}), and the clipboard couldn't take the text.")
            },
            action: None,
            timeout_ms: 12_000,
        },
    );
    Outcome::Copied { why, said, told: true }
}

// ---- the pure parts ---------------------------------------------------------------

/// A field's character as compared with the paste: any line break as
/// `\n`, a no-break space as a space.
fn norm(c: char) -> char {
    match c {
        '\r' => '\n',
        '\u{a0}' => ' ',
        c => c,
    }
}

/// Where `pasted` starts in `text` (a field's own text) when it ends at
/// byte `end`: compared with line breaks ("\r\n", "\r", "\n") and no-break
/// spaces made alike; whitespace Yap put at its end may be missing or
/// followed by more (the field's own). `None`: it isn't there.
pub fn match_before(text: &str, end: usize, pasted: &str) -> Option<usize> {
    let exact = |end: usize, want: &str| -> Option<usize> {
        let want: Vec<char> = want.chars().filter(|&c| c != '\r').map(norm).collect();
        let head = text.get(..end)?;
        let mut i = head.len();
        for &w in want.iter().rev() {
            let prev = head[..i].chars().next_back()?;
            let len = if prev == '\n' && head[..i - 1].ends_with('\r') { 2 } else { prev.len_utf8() };
            if norm(prev) != w {
                return None;
            }
            i -= len;
        }
        Some(i)
    };
    if let Some(start) = exact(end, pasted) {
        return Some(start);
    }
    // The trailing space Yap added, gone or not where it put it.
    let trimmed = pasted.trim_end();
    if trimmed.len() == pasted.len() || trimmed.is_empty() {
        return None;
    }
    let head = text.get(..end)?;
    let stripped = head.trim_end_matches([' ', '\u{a0}', '\t']);
    exact(stripped.len(), trimmed)
}

/// Where `pasted` is in `text` if it's there exactly once (line breaks made
/// alike): its byte range.
pub fn unique_in(text: &str, pasted: &str) -> Option<(usize, usize)> {
    let want = pasted.trim();
    if want.is_empty() {
        return None;
    }
    let mut found = None;
    for (end, _) in text.char_indices().map(|(i, c)| (i + c.len_utf8(), c)) {
        if let Some(start) = match_before(text, end, want) {
            if found.is_some() {
                return None;
            }
            found = Some((start, end));
        }
    }
    found
}

/// How many characters `piece` (a field's own text) is in the units a
/// field may count in, most likely first and without repeats: code points,
/// code points with "\r\n" as one, UTF-16 units. The one whose range reads
/// back exactly the paste is the field's.
pub fn unit_counts(piece: &str) -> Vec<usize> {
    let chars = piece.chars().count();
    let crlf = piece.matches("\r\n").count();
    let utf16 = piece.encode_utf16().count();
    let mut out = Vec::new();
    for n in [chars, chars - crlf, utf16, utf16 - crlf] {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Whether two texts read the same, line breaks and no-break spaces aside.
pub fn same_text(a: &str, b: &str) -> bool {
    let n = |s: &str| -> String { s.replace("\r\n", "\n").chars().map(norm).collect() };
    n(a) == n(b)
}

/// A Win32 edit control's units in `text[..byte]`: UTF-16 units, a
/// RichEdit counting any line break as one.
pub fn units(text: &str, byte: usize, rich: bool) -> usize {
    let head = &text[..byte.min(text.len())];
    let crlf = if rich { head.matches("\r\n").count() } else { 0 };
    head.encode_utf16().count() - crlf
}

/// The byte in `text` at `units` of the control's (see [`units`]); never
/// between a "\r" and its "\n". `None` past the end.
pub fn byte_at_units(text: &str, units: usize, rich: bool) -> Option<usize> {
    let mut u = 0usize;
    let mut prev = '\0';
    for (i, c) in text.char_indices() {
        let mid = prev == '\r' && c == '\n';
        if !mid && u >= units {
            return Some(i);
        }
        if !(rich && mid) {
            u += c.len_utf16();
        }
        prev = c;
    }
    (u >= units).then_some(text.len())
}

// ---- UI Automation ----------------------------------------------------------------

#[cfg(windows)]
mod uia {
    use windows::core::{Interface, BOOL, BSTR};
    use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
        IUIAutomationTextPattern2, IUIAutomationTextRange, IUIAutomationValuePattern, TextPatternRangeEndpoint_End,
        TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_IsReadOnlyAttributeId, UIA_TextPattern2Id,
        UIA_TextPatternId, UIA_ValuePatternId,
    };

    use super::{byte_at_units, match_before, same_text, unique_in, unit_counts, units, EDITED_SINCE, MOVED, SELECT_WAIT, SETTLE};

    /// Why the paste couldn't be found: moved or edited (`anywhere`: the
    /// swap, which takes it wherever it is once), or not right before the
    /// caret (the rewrite).
    fn not_found(anywhere: bool) -> String {
        (if anywhere { MOVED } else { EDITED_SINCE }).to_string()
    }

    /// UI Automation's answers, at most this long each.
    const TIMEOUT_MS: u32 = 800;
    /// How far up from the focused element its text field is looked for.
    const PARENT_LEVELS: usize = 3;
    /// A field longer than this isn't searched whole (only at the caret).
    const MAX_FIELD_CHARS: i32 = 20_000;

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

    fn automation() -> Option<IUIAutomation> {
        unsafe {
            let a: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))
                .ok()?;
            if let Ok(a2) = a.cast::<IUIAutomation2>() {
                let _ = a2.SetConnectionTimeout(TIMEOUT_MS);
                let _ = a2.SetTransactionTimeout(TIMEOUT_MS);
            }
            Some(a)
        }
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn SendMessageTimeoutW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize, flags: u32, timeout_ms: u32, result: *mut usize) -> isize;
    }
    const EM_GETSEL: u32 = 0x00B0;
    const EM_SETSEL: u32 = 0x00B1;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    const SMTO_ERRORONEXIT: u32 = 0x0020;
    const ES_PASSWORD: i32 = 0x0020;
    const ES_READONLY: i32 = 0x0800;

    /// A Win32 edit control's class: `Some(true)` a RichEdit (2.0+),
    /// `Some(false)` an EDIT (WinForms' too). As `selection.rs` tells them.
    fn edit_class(class: &str) -> Option<bool> {
        let class = class.to_ascii_lowercase();
        let base = match class.strip_prefix("windowsforms10.") {
            Some(rest) => rest.split('.').next().unwrap_or_default(),
            None => class.as_str(),
        };
        if base == "edit" {
            Some(false)
        } else if base.starts_with("richedit") && base != "richedit" {
            Some(true)
        } else {
            None
        }
    }

    /// The control with the keyboard focus in `target`'s thread, if it's a
    /// Win32 edit control of the same process that takes text.
    fn focused_edit(target: isize) -> Option<(HWND, bool)> {
        use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GetWindowLongW, GetWindowThreadProcessId, GUITHREADINFO, GWL_STYLE};
        unsafe {
            let top = HWND(target as *mut core::ffi::c_void);
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(top, Some(&mut pid));
            if thread == 0 {
                return None;
            }
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            GetGUIThreadInfo(thread, &mut info).ok()?;
            let focus = info.hwndFocus;
            if focus.is_invalid() {
                return None;
            }
            let mut focus_pid = 0u32;
            GetWindowThreadProcessId(focus, Some(&mut focus_pid));
            if focus_pid != pid {
                return None;
            }
            let mut buf = [0u16; 256];
            let n = GetClassNameW(focus, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
            let rich = edit_class(&String::from_utf16_lossy(&buf[..n]))?;
            if GetWindowLongW(focus, GWL_STYLE) & (ES_PASSWORD | ES_READONLY) != 0 {
                return None;
            }
            Some((focus, rich))
        }
    }

    fn em_getsel(hwnd: HWND) -> Option<(usize, usize)> {
        let mut result = 0usize;
        let sent = unsafe { SendMessageTimeoutW(hwnd, EM_GETSEL, 0, 0, SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT, TIMEOUT_MS, &mut result) };
        let packed = result as u32;
        (sent != 0 && packed != u32::MAX).then(|| {
            let (a, b) = ((packed & 0xFFFF) as usize, (packed >> 16) as usize);
            (a.min(b), a.max(b))
        })
    }

    fn em_setsel(hwnd: HWND, start: usize, end: usize) -> bool {
        let mut result = 0usize;
        let sent = unsafe { SendMessageTimeoutW(hwnd, EM_SETSEL, start, end as isize, SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT, TIMEOUT_MS, &mut result) };
        sent != 0
    }

    /// The text of the field `element`, if it isn't read-only.
    unsafe fn value_of(element: &IUIAutomationElement) -> Option<String> {
        let p: IUIAutomationValuePattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
        if p.CurrentIsReadOnly().ok()?.as_bool() {
            return None;
        }
        Some(p.CurrentValue().ok()?.to_string())
    }

    /// The focused element in `pid`, or the text field (TextPattern) it's
    /// part of: not a password box.
    unsafe fn text_field(a: &IUIAutomation, pid: u32) -> Result<(IUIAutomationElement, IUIAutomationTextPattern), String> {
        let focused = a.GetFocusedElement().map_err(|_| "Yap couldn't tell which field has the focus")?;
        if focused.CurrentProcessId().map(|p| p as u32).ok() != Some(pid) {
            return Err("another app has the focus".into());
        }
        if focused.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
            return Err("it's a password box".into());
        }
        let walker = a.ControlViewWalker().ok();
        let mut e = focused;
        for _ in 0..=PARENT_LEVELS {
            if e.CurrentProcessId().map(|p| p as u32).ok() != Some(pid) {
                break;
            }
            if let Some(p) = e.GetCurrentPattern(UIA_TextPatternId).ok().and_then(|u| u.cast::<IUIAutomationTextPattern>().ok()) {
                return Ok((e, p));
            }
            match walker.as_ref().and_then(|w| w.GetParentElement(&e).ok()) {
                Some(parent) => e = parent,
                None => break,
            }
        }
        Err("Yap can't read this field".into())
    }

    /// The caret (or the selection) in a TextPattern field.
    unsafe fn caret(element: &IUIAutomationElement, pattern: &IUIAutomationTextPattern) -> Option<IUIAutomationTextRange> {
        match pattern.GetSelection() {
            Ok(r) if r.Length().unwrap_or(0) > 0 => r.GetElement(0).ok(),
            _ => {
                let p2: IUIAutomationTextPattern2 = element.GetCurrentPattern(UIA_TextPattern2Id).ok()?.cast().ok()?;
                let mut active = BOOL(0);
                p2.GetCaretRange(&mut active).ok()
            }
        }
    }

    /// `range` collapsed to its start.
    unsafe fn at_start(range: &IUIAutomationTextRange) -> Option<IUIAutomationTextRange> {
        let r = range.Clone().ok()?;
        r.MoveEndpointByRange(TextPatternRangeEndpoint_End, range, TextPatternRangeEndpoint_Start).ok()?;
        Some(r)
    }

    /// The range of `count` characters before `end` (a collapsed range).
    unsafe fn back_from(end: &IUIAutomationTextRange, count: usize) -> Option<IUIAutomationTextRange> {
        let r = end.Clone().ok()?;
        r.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(count as i32)).ok()?;
        Some(r)
    }

    fn text(range: &IUIAutomationTextRange) -> Option<String> {
        unsafe { range.GetText(-1).ok().map(|b| b.to_string()) }
    }

    /// Select exactly `pasted` (just pasted into `hwnd`, of process `pid`)
    /// in the focused field — ending at the caret, or (`anywhere`) else
    /// wherever it is exactly once: "edit" or "text" (how), and the text
    /// selected (the paste as the field has it). `Err(why)` when it can't be
    /// done safely; nothing was changed then (but a selection).
    pub fn select_pasted(hwnd: isize, pid: u32, pasted: &str, anywhere: bool) -> Result<(&'static str, String), String> {
        let _com = Com::init().ok_or("UI Automation isn't available")?;
        let a = automation().ok_or("UI Automation isn't available")?;
        // A Win32 edit control whose text UI Automation gives as a value
        // (WinForms, EDIT): asked directly. One that only has a TextPattern
        // (Windows 11 Notepad's RichEdit) goes the TextPattern way.
        if let Some((edit, rich)) = focused_edit(hwnd) {
            if let Some(value) = edit_value(&a, edit, rich) {
                return select_in_edit(edit, rich, value, pasted, anywhere).map(|t| ("edit", t));
            }
        }
        unsafe { select_in_text(&a, pid, pasted, anywhere).map(|t| ("text", t)) }
    }

    /// A Win32 edit control's text as UI Automation's ValuePattern gives
    /// it (a RichEdit's last paragraph mark left out), if it does.
    fn edit_value(a: &IUIAutomation, edit: HWND, rich: bool) -> Option<String> {
        let element = unsafe { a.ElementFromHandle(edit) }.ok()?;
        let mut value = unsafe { value_of(&element) }?;
        if rich && value.ends_with('\r') {
            value.pop();
        }
        Some(value)
    }

    /// A Win32 edit control: its text (`value`), the pasted text ending at
    /// its caret (or, `anywhere`, there once), `EM_SETSEL`, and `EM_GETSEL`
    /// saying so. Returns the text selected.
    fn select_in_edit(edit: HWND, rich: bool, value: String, pasted: &str, anywhere: bool) -> Result<String, String> {
        let (_, caret) = em_getsel(edit).ok_or("the field didn't answer")?;
        let end_b = byte_at_units(&value, caret, rich).ok_or("the field didn't answer as expected")?;
        let (start_b, end_b) = match match_before(&value, end_b, pasted) {
            Some(start) => (start, end_b),
            None if anywhere => unique_in(&value, pasted).ok_or_else(|| not_found(true))?,
            None => return Err(not_found(false)),
        };
        let (su, eu) = (units(&value, start_b, rich), units(&value, end_b, rich));
        if !em_setsel(edit, su, eu) || em_getsel(edit) != Some((su, eu)) {
            return Err("the field wouldn't select the text".into());
        }
        Ok(value[start_b..end_b].to_string())
    }

    /// A TextPattern field: the text before the caret ending with the paste
    /// (else, `anywhere`, `FindText` finding it once), a range over it that
    /// reads back exactly the paste, `Select()`, and the field's selection
    /// reading back the same. Returns the text selected.
    unsafe fn select_in_text(a: &IUIAutomation, pid: u32, pasted: &str, anywhere: bool) -> Result<String, String> {
        let (element, pattern) = text_field(a, pid)?;
        let document = pattern.DocumentRange().map_err(|_| "Yap can't read this field")?;
        if let Ok(v) = document.GetAttributeValue(UIA_IsReadOnlyAttributeId) {
            if bool::try_from(&v).unwrap_or(false) {
                return Err("the field is read-only".into());
            }
        }
        let range = match caret(&element, &pattern).and_then(|c| at_start(&c)) {
            Some(end) => before_caret(&end, pasted),
            None => None,
        }
        .or_else(|| if anywhere { found_once(&document, pasted) } else { None })
        .ok_or_else(|| not_found(anywhere))?;
        if let Err(e) = range.Select() {
            tracing::debug!(code = ?e.code(), "Use what I said: the field refused Select()");
            return Err("the field wouldn't select the text".into());
        }
        // The field's own selection must now be exactly the paste.
        let want = text(&range).unwrap_or_default();
        let started = std::time::Instant::now();
        loop {
            std::thread::sleep(SETTLE / 2);
            let selected = pattern.GetSelection().ok().filter(|r| r.Length().unwrap_or(0) > 0).and_then(|r| r.GetElement(0).ok());
            let got = selected.as_ref().and_then(text);
            if got.as_deref().is_some_and(|t| same_text(t, &want)) {
                return Ok(want);
            }
            if started.elapsed() > SELECT_WAIT {
                tracing::debug!(
                    want_chars = want.chars().count(),
                    got_chars = got.as_ref().map(|t| t.chars().count()),
                    "Use what I said: the field's selection didn't read back as the paste"
                );
                return Err("the field wouldn't select the text".into());
            }
        }
    }

    /// The range of the paste just before the caret (`end`, collapsed):
    /// the field's characters counted its way ([`unit_counts`]) until the
    /// range reads back exactly what was pasted.
    unsafe fn before_caret(end: &IUIAutomationTextRange, pasted: &str) -> Option<IUIAutomationTextRange> {
        // Enough of the text before the caret: the paste and some.
        let n = pasted.encode_utf16().count() + pasted.matches('\n').count() + 8;
        let before = text(&back_from(end, n)?)?;
        let start = match_before(&before, before.len(), pasted)?;
        let piece = &before[start..];
        unit_counts(piece).into_iter().find_map(|k| {
            let r = back_from(end, k)?;
            text(&r).filter(|t| t == piece || same_text(t, piece)).map(|_| r)
        })
    }

    /// The paste where it's the only one in the field (the caret moved
    /// since): `FindText`, and none after it.
    unsafe fn found_once(document: &IUIAutomationTextRange, pasted: &str) -> Option<IUIAutomationTextRange> {
        let all = document.GetText(MAX_FIELD_CHARS).ok()?.to_string();
        if all.encode_utf16().count() >= MAX_FIELD_CHARS as usize {
            return None;
        }
        unique_in(&all, pasted)?;
        let want = pasted.trim();
        for variant in [want.to_string(), want.replace('\n', "\r\n"), want.replace('\n', "\r")] {
            let Ok(found) = document.FindText(&BSTR::from(variant.as_str()), false, false) else {
                continue;
            };
            let rest = document.Clone().ok()?;
            rest.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &found, TextPatternRangeEndpoint_End).ok()?;
            if rest.FindText(&BSTR::from(variant.as_str()), false, false).is_ok() {
                return None;
            }
            return Some(found);
        }
        None
    }

    /// Whether `said` ends at the caret of the focused field in `hwnd` (the
    /// swap landed; the last paste is still there for a rewrite).
    pub fn ends_at_caret(hwnd: isize, pid: u32, said: &str) -> bool {
        let Some(_com) = Com::init() else { return false };
        let Some(a) = automation() else { return false };
        if let Some((edit, rich, value)) = focused_edit(hwnd).and_then(|(e, r)| edit_value(&a, e, r).map(|v| (e, r, v))) {
            return em_getsel(edit)
                .and_then(|(_, caret)| byte_at_units(&value, caret, rich))
                .is_some_and(|end| match_before(&value, end, said).is_some());
        }
        unsafe {
            let Ok((element, pattern)) = text_field(&a, pid) else { return false };
            let Some(end) = caret(&element, &pattern).and_then(|c| at_start(&c)) else { return false };
            let n = said.encode_utf16().count() + said.matches('\n').count() + 8;
            back_from(&end, n)
                .as_ref()
                .and_then(text)
                .is_some_and(|before| match_before(&before, before.len(), said).is_some())
        }
    }
}

// ---- commands ---------------------------------------------------------------------

/// "Use what I said": swap the last cleaned dictation's paste for the words
/// as said (the bar's button and the shortcut go through [`swap_soon`]).
#[tauri::command]
pub async fn as_said_swap(app: AppHandle) -> Result<Outcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = swap(&app);
        end_offer(&app);
        r
    })
    .await
    .map_err(|e| format!("The swap failed: {e}"))?
}

/// Home's **Paste what I said** on a dictation (`ts` and `text`: the pair
/// Home lists it by) — see [`paste_entry`].
#[tauri::command]
pub async fn as_said_paste_entry(app: AppHandle, ts: u64, text: String) -> Result<Outcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = paste_entry(&app, ts, &text);
        end_offer(&app);
        r
    })
    .await
    .map_err(|e| format!("Paste what I said failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_kind::{Host, Terminal};

    #[test]
    fn the_rewrite_shortcut_takes_the_last_paste_only_in_its_window_within_a_minute() {
        let s = Duration::from_secs;
        assert!(rewritable(Some(7), AppKind::Other, s(3), Some(7)));
        assert!(rewritable(Some(7), AppKind::Messaging, s(60), Some(7)), "a minute on the dot");
        assert!(!rewritable(Some(7), AppKind::Other, s(61), Some(7)), "older: write mode, as before");
        assert!(!rewritable(Some(7), AppKind::Other, s(3), Some(8)), "another window in front");
        assert!(!rewritable(Some(7), AppKind::Other, s(3), None), "Yap's own window in front");
        assert!(!rewritable(None, AppKind::Other, s(3), None), "never pasted into another app");
        let terminal = AppKind::Terminal(Terminal { coding_cli: None, host: Host::WindowsTerminal });
        assert!(!rewritable(Some(7), terminal, s(3), Some(7)), "a terminal's line can't be selected");
    }

    #[test]
    fn a_rewrite_keeps_the_spaces_around_what_it_replaces() {
        // Yap's trailing space, kept after the rewrite.
        assert_eq!(keep_spaces("hey can you send it ", " Could you send it, please?\n"), "Could you send it, please? ");
        // A paste mid-text, a space before it.
        assert_eq!(keep_spaces(" the blue one", "the red one"), " the red one");
        // The field dropped the trailing space: none added.
        assert_eq!(keep_spaces("Send it.", "Please send it."), "Please send it.");
        assert_eq!(keep_spaces("Send it. ", "  "), "", "nothing from the AI: nothing");
        assert_eq!(keep_spaces("  ", "Send it."), "Send it.");
    }

    #[test]
    fn the_paste_is_found_ending_at_the_caret() {
        let field = "Notes: send the plan to Sam. ";
        let end = field.len();
        assert_eq!(match_before(field, end, "send the plan to Sam. "), Some(7));
        // A paste mid-text: it ends where the caret is.
        let field = "I want the blue one please";
        assert_eq!(match_before(field, 19, " the blue one"), Some(6));
        // Edited since: not there.
        assert_eq!(match_before("send the plan to Pam. ", 22, "send the plan to Sam. "), None);
        // Line breaks as the field has them.
        let field = "Dear Sam,\r\nthe plan is attached.";
        assert_eq!(match_before(field, field.len(), "Dear Sam,\nthe plan is attached."), Some(0));
        assert_eq!(match_before("Dear Sam,\rthe plan.", 19, "Dear Sam,\nthe plan."), Some(0));
        // A no-break space for a space.
        assert_eq!(match_before("a\u{a0}b", 4, "a b"), Some(0));
        // The trailing space Yap added, which the field dropped, or with
        // more after it.
        assert_eq!(match_before("Hello there.", 12, "Hello there. "), Some(0));
        assert_eq!(match_before("Hello there.  ", 14, "Hello there. "), Some(0));
        assert_eq!(match_before("x", 5, "x"), None, "past the end");
    }

    #[test]
    fn the_paste_elsewhere_counts_only_when_it_is_there_once() {
        assert_eq!(unique_in("one. Send it. two", "Send it."), Some((5, 13)));
        assert_eq!(unique_in("Send it. Send it.", "Send it."), None);
        assert_eq!(unique_in("nothing here", "Send it."), None);
    }

    #[test]
    fn a_field_counts_characters_its_own_way() {
        assert_eq!(unit_counts("plain"), vec![5]);
        // "\r\n" as one or two; an emoji as one code point or two UTF-16 units.
        assert_eq!(unit_counts("a\r\nb"), vec![4, 3]);
        assert_eq!(unit_counts("hi 👋"), vec![4, 5]);
    }

    #[test]
    fn an_edit_controls_units_and_bytes() {
        let t = "ab\r\ncd";
        // A plain EDIT counts "\r\n" as two, a RichEdit as one.
        assert_eq!(units(t, t.len(), false), 6);
        assert_eq!(units(t, t.len(), true), 5);
        assert_eq!(byte_at_units(t, 4, false), Some(4));
        assert_eq!(byte_at_units(t, 3, true), Some(4));
        // Never between "\r" and "\n".
        assert_eq!(byte_at_units(t, 3, false), Some(4));
        assert_eq!(byte_at_units(t, 9, false), None);
        assert_eq!(byte_at_units("é👋", 3, false), Some("é👋".len()));
    }

    #[test]
    fn the_bar_offers_it_only_when_cleanup_changed_the_words() {
        assert!(words_differ("I think we should ship on Friday.", "um I think we should uh ship on friday"));
        assert!(!words_differ("Send the plan to Sam.", "send the plan to sam"));
        assert!(!words_differ("It’s done.", "it's done"), "a curly apostrophe is the same word");
        assert!(words_differ("It's done.", "its done"));
    }

    /// (The one test that uses the last paste itself: tests run at once.)
    #[test]
    fn the_last_paste_is_remembered_swapped_only_when_cleanup_changed_it() {
        let seq = remember(Some(42), AppKind::Other, "Hello there. ", Some("hello there um "), true).unwrap();
        assert!(offered(seq));
        // Its history entry, by its text as history keeps it; not another's.
        recorded(1_700_000_001, "Something else.");
        assert_eq!(last().as_ref().unwrap().entry, None);
        recorded(1_700_000_000, "Hello there.");
        assert_eq!(last().as_ref().unwrap().entry, Some(1_700_000_000));
        // The rewrite shortcut in that window takes it; in another, not.
        assert_eq!(rewrite_target(Some(42)).map(|p| p.text().to_string()).as_deref(), Some("Hello there."));
        assert!(rewrite_target(Some(43)).is_none());
        // Nothing changed: nothing to swap or offer, but it's still the last
        // paste (a rewrite may take it).
        assert!(remember(Some(42), AppKind::Other, "Same. ", Some("Same. "), true).is_none());
        assert!(!offered(seq));
        let p = last().clone().unwrap();
        assert_eq!((p.said.as_deref(), p.text(), p.entry), (None, "Same.", None));
        assert!(remember(Some(42), AppKind::Other, "A. ", None, true).is_none());
        assert_eq!(last().as_ref().map(|p| p.text().to_string()).as_deref(), Some("A."));
        // Into one of Yap's own windows with nothing to swap: forgotten.
        assert!(remember(None, AppKind::Other, "B. ", None, true).is_none());
        assert!(last().is_none());
        // There with the words as said: kept for "Use what I said" (which
        // copies them there), never for a rewrite.
        assert!(remember(None, AppKind::Other, "B. ", Some("b um "), true).is_some());
        assert!(rewrite_target(None).is_none());
        forget();
        assert!(last().is_none());
    }
}
