//! **A question about text selected in another app** (2026-10-31; Wispr
//! Flow's Command Mode, its "Lens", by behaviour): select text in any app
//! and ask about it — "what does this mean?", "is this right?", "summarise
//! this" — and the answer comes on the Voice Assistant's path (spoken when
//! spoken replies are on, the bar's card, kept in Chat), **never typed into
//! the app**: the selection and the clipboard stay exactly as they were.
//!
//! Two ways in: the rewrite shortcut on a selection, when what was said is
//! a question rather than an edit (`dictation/ask_or_edit.rs`), and its own
//! shortcut, **Ask about the selection** (unbound by default; always
//! answers; read-only text too, as Read aloud reads a selection). Either way
//! `assistant::ask_about` asks Chat as a spoken question is asked, with
//! [`Selected`] around the turn ([`with`]): the question keeps the
//! selection as its quote ([`quote`]: above it in Chat, and in the history
//! the model reads later, `chats::ChatMessage::sent_text`), and the model
//! is told what was selected and where ([`context_note`]). Chat's model and
//! effort, web search when it's on; no window looked at, no screenshots or
//! document passage taken, no app opened: the selection is what it's about.
//!
//! The text is never logged above `debug` (lengths only).

use std::future::Future;

use tauri::AppHandle;

/// The text a question is about, and the app it was selected in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selected {
    /// The words as selected.
    pub text: String,
    /// The app's process name ("notepad.exe"; "Yap" for one of Yap's own
    /// windows; "" when unknown).
    pub app: String,
}

tokio::task_local! {
    static ABOUT: Option<Selected>;
}

/// Run `f` (a Chat turn) about `selected`.
pub async fn with<F: Future>(selected: Option<Selected>, f: F) -> F::Output {
    ABOUT.scope(selected, f).await
}

/// The selection this turn's question is about, if any.
pub fn current() -> Option<Selected> {
    ABOUT.try_with(|s| s.clone()).ok().flatten()
}

/// "notepad.exe" → "notepad"; "" stays "". Pure.
pub fn app_name(app: &str) -> String {
    let a = app.trim();
    let a = a.rsplit(['\\', '/']).next().unwrap_or(a);
    a.strip_suffix(".exe").or_else(|| a.strip_suffix(".EXE")).unwrap_or(a).to_string()
}

/// The most of a selection the model is given: more for a model with a
/// long context. Pure.
pub fn room(context_tokens: u64) -> usize {
    if context_tokens >= 32_768 {
        12_000
    } else {
        4_000
    }
}

/// The most of a selection kept with its question (the quote above it).
const QUOTE_CHARS: usize = 600;

/// **What the model is told** with the question (its context): what was
/// selected and in which app, at most `max_chars` of it (cut short: said
/// so), and that "this" means it — answered here, never rewritten to be
/// pasted back. Pure.
pub fn context_note(sel: &Selected, max_chars: usize) -> String {
    let text = sel.text.trim();
    let n = text.chars().count();
    let (shown, cut) = if n > max_chars { (text.chars().take(max_chars).collect::<String>(), true) } else { (text.to_string(), false) };
    let app = app_name(&sel.app);
    let from = if app.is_empty() { String::new() } else { format!(" in {app}") };
    let cut_note = if cut { format!(" (only its first {max_chars} characters are here: it was longer)") } else { String::new() };
    format!(
        "The user selected this text{from} and is asking about it{cut_note}. When they say \"this\" or \"it\", they mean it. Answer their question about it here: don't rewrite it to be pasted back, and don't change their notes or documents unless they ask.\n<selected_text>\n{shown}\n</selected_text>"
    )
}

/// What's kept with the question: the start of the selection, and its app.
/// `None`: nothing selected. Pure.
pub fn quote(sel: &Selected) -> Option<crate::chat::documents::Quote> {
    let text: String = sel.text.trim().chars().take(QUOTE_CHARS).collect();
    (!text.is_empty()).then(|| crate::chat::documents::Quote { note_id: 0, text, app: sel.app.trim().to_string() })
}

/// The "Select some text first" alert's kind (its rule in `notice.rs`).
pub const NOTHING_SELECTED: &str = "askNoSelection";

/// **Ask about the selection** pressed with nothing selected (or the focus
/// in Yap's own window with nothing selected on its page): said, nothing
/// recorded.
pub fn nothing_selected(app: &AppHandle) {
    tracing::info!("Ask about the selection: nothing selected");
    crate::notices::alert::show_soon(app, nothing_selected_alert());
}

/// The alert itself. Pure.
pub fn nothing_selected_alert() -> crate::notices::alert::Alert {
    crate::notices::alert::Alert {
        kind: NOTHING_SELECTED,
        icon: "assistant",
        title: "Select some text first".into(),
        body: "Select the text you want to ask about in any app, then press the shortcut and ask your question.".into(),
        action: None,
        timeout_ms: 6_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(text: &str, app: &str) -> Selected {
        Selected { text: text.into(), app: app.into() }
    }

    #[test]
    fn the_model_is_told_what_was_selected_where_and_that_this_means_it() {
        let note = context_note(&sel("  error[E0425]: cannot find value `x` in this scope  ", "C:\\Tools\\Code.exe"), 4_000);
        assert!(note.starts_with("The user selected this text in Code and is asking about it."), "{note}");
        assert!(note.contains("When they say \"this\" or \"it\", they mean it."));
        assert!(note.contains("don't rewrite it to be pasted back"));
        assert!(note.ends_with("<selected_text>\nerror[E0425]: cannot find value `x` in this scope\n</selected_text>"));
        // No app known: no "in".
        assert!(context_note(&sel("Hello", ""), 100).starts_with("The user selected this text and is asking about it."));
    }

    #[test]
    fn a_long_selection_is_cut_to_the_room_and_says_so() {
        let long = "a".repeat(5_000);
        let note = context_note(&sel(&long, "notepad.exe"), room(8_192));
        assert!(note.contains("(only its first 4000 characters are here: it was longer)"));
        let inside = note.split("<selected_text>\n").nth(1).and_then(|s| s.split("\n</selected_text>").next()).unwrap();
        assert_eq!(inside, "a".repeat(4_000));
        // A long context takes more.
        assert_eq!(room(200_000), 12_000);
        assert!(!context_note(&sel(&long, ""), room(200_000)).contains("only its first"));
    }

    #[test]
    fn the_quote_keeps_the_start_and_the_app() {
        let q = quote(&sel(&"word ".repeat(300), " notepad.exe ")).unwrap();
        assert_eq!(q.note_id, 0, "no note: another app's text");
        assert_eq!(q.app, "notepad.exe");
        assert_eq!(q.text.chars().count(), QUOTE_CHARS);
        assert!(quote(&sel("   ", "notepad.exe")).is_none());
    }

    #[test]
    fn a_follow_up_still_knows_what_this_was() {
        // The question as the model reads it in the history (chats.rs).
        let m = crate::chat::chats::ChatMessage {
            role: "user".into(),
            text: "What does this mean?".into(),
            quote: quote(&sel("Ceci n'est pas une pipe.", "notepad.exe")),
            ..Default::default()
        };
        assert_eq!(m.sent_text(), "(About this text the user selected in notepad: \"Ceci n'est pas une pipe.\")\n\nWhat does this mean?");
        // An answer never carries one.
        let a = crate::chat::chats::ChatMessage { role: "assistant".into(), text: "It's a painting.".into(), ..m.clone() };
        assert_eq!(a.sent_text(), "It's a painting.");
    }

    #[test]
    fn app_names_lose_their_folder_and_exe() {
        assert_eq!(app_name("notepad.exe"), "notepad");
        assert_eq!(app_name("C:\\Program Files\\App\\WINWORD.EXE"), "WINWORD");
        assert_eq!(app_name("Yap"), "Yap");
        assert_eq!(app_name(""), "");
    }

    #[tokio::test]
    async fn a_turn_knows_its_selection_and_no_other_does() {
        assert!(current().is_none());
        let inside = with(Some(sel("Hi", "x.exe")), async { current() }).await;
        assert_eq!(inside, Some(sel("Hi", "x.exe")));
        assert!(with(None, async { current() }).await.is_none());
        assert!(current().is_none());
    }

    #[test]
    fn nothing_selected_says_what_to_do() {
        let a = nothing_selected_alert();
        assert_eq!(a.kind, NOTHING_SELECTED);
        assert_eq!(a.title, "Select some text first");
        assert!(a.action.is_none());
    }
}
