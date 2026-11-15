//! **Incognito chats** (15 Nov 2026; the Claude app's ghost, from Anthropic's
//! help pages): a chat Yap keeps nowhere. A new chat's ghost button (Chat's
//! side only, never a Code chat) makes its first question start one.
//!
//! - **Never saved**: the conversation lives in memory only
//!   (`chats::Conversation::incognito`: chats.json's save leaves it out, the
//!   shared files' copy too), never listed, never found by a search (by
//!   words or by meaning: `chats::with_all` and `chats::search` skip it),
//!   never shown on a paired phone or in Download your data, never the
//!   chat the Voice Assistant carries on (`chats::latest`).
//! - **Never remembered, and memory unused**: no memory's "You" or passages
//!   in its prompt, no `remember` / `forget` / `search_chats` tools, no
//!   "Remember that…", no past chats; and memory never learns from it
//!   (`memory::learn::after_turn`). About you still applies: that's a
//!   setting, not something learned.
//! - **On your Claude plan** its session is never written by Claude Code
//!   (`--no-session-persistence`) nor kept in claude_sessions.json.
//! - **Gone when left** ([`chat_incognito_end`]): the page ends it when the
//!   person opens another chat or a new one, and Yap ends them all as it
//!   quits — its screenshots and pictures deleted, the per-chat switches it
//!   had forgotten (a later chat may take its id). A crash leaves its
//!   pictures for the sweep, which deletes a picture no chat names after a
//!   day.
//! - Its documents are still notes (the person asked for them; they're in
//!   Notes as any document is), and its question still counts in the AI
//!   usage numbers (tokens only). The log never says what it was about.

use std::future::Future;

tokio::task_local! {
    /// A new chat's first question asked incognito (`chat_send`'s
    /// `incognito`).
    static NEW: bool;
}

/// `f` run as a new chat's first question, incognito when `on`.
pub async fn with_new<F: Future>(on: bool, f: F) -> F::Output {
    NEW.scope(on, f).await
}

/// Inside [`with_new`]: the new chat is incognito.
pub fn new_chat() -> bool {
    NEW.try_with(|on| *on).unwrap_or(false)
}

/// **End an incognito chat** (the page left it): it goes, with everything
/// it had. `false` when `id` isn't one (a kept chat is never touched).
#[tauri::command]
pub fn chat_incognito_end(id: u64) -> bool {
    crate::chat::chats::end_incognito(id)
}

/// Debug-only (the e2e suite): the incognito chats Yap holds now.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_chat_incognito_ids() -> Vec<u64> {
    crate::chat::chats::incognito_ids()
}

#[cfg(test)]
mod tests {
    use crate::chat::chats;

    #[test]
    fn an_incognito_chat_is_never_written_listed_or_searched_and_ends_whole() {
        let kept = chats::create("Lisbon trip");
        chats::append(kept.id, "user", "Where should we stay in Lisbon?", &[], None, false).unwrap();
        let ghost = chats::create_incognito("A private question");
        chats::append(ghost.id, "user", "Lisbon dentist prices?", &[], None, false).unwrap();
        assert!(chats::is_incognito(ghost.id) && !chats::is_incognito(kept.id));
        // The page's copy says so; a save never writes it.
        assert!(chats::get(ghost.id).unwrap().incognito);
        let file = chats::saved_json();
        assert!(file.contains("Lisbon trip") && !file.contains("A private question") && !file.contains("dentist"), "{file}");
        // Never listed, searched, latest, nor among them all.
        let listed: Vec<u64> = chats::list().as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
        assert_eq!(listed, vec![kept.id]);
        assert!(chats::search("Lisbon", None, 5).iter().all(|f| f.id != ghost.id));
        assert_eq!(chats::latest().map(|l| l.0), Some(kept.id));
        assert_eq!(chats::with_all(|all| all.iter().map(|c| c.id).collect::<Vec<_>>()), vec![kept.id]);
        // Its pictures are kept while it's open (the sweep sees them).
        // Ending it: gone; a kept chat can't be ended this way.
        assert!(!chats::end_incognito(kept.id));
        assert!(chats::end_incognito(ghost.id));
        assert!(chats::get(ghost.id).is_none() && chats::get(kept.id).is_some());
        assert!(!chats::end_incognito(ghost.id));
    }

    #[tokio::test]
    async fn a_new_chat_is_incognito_only_inside_its_first_question() {
        assert!(!super::new_chat());
        assert!(super::with_new(true, async { super::new_chat() }).await);
        assert!(!super::with_new(false, async { super::new_chat() }).await);
    }
}
