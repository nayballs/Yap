//! **Whose idea a job is**: the person's own words, never a note's, a
//! page's or a job's own output (as `apps::consented` for opening an app).
//!
//! - **Starting one** ([`start`]): the person's message this turn names the
//!   job's folder (by its name, its last part or its path), or it's a plain
//!   "yes" to the answer just before, which asked a question and named it
//!   ("Shall I start a job in garden-planner to fix the tests?" → "Yes.").
//!   A folder is the same word in every language, so this holds in all of
//!   them.
//! - **Telling one something** ([`message`]): what goes to the job carries
//!   the person's own words this turn (a word of theirs that matters, not
//!   "the" or "please"), or their message is a plain "yes" (to a plan, to
//!   the job's question).
//! - **"Go ahead"** after a plan ([`go_ahead`]).
//!
//! Pure.

use super::folders::{self, Folder};

/// Did the person ask for a job in `folder`: their `question` names it, or
/// it's a plain yes to `previous` (the answer just before), which asked a
/// question and named it?
pub fn start(question: &str, previous: Option<&str>, folder: &Folder) -> bool {
    if names(question, folder) {
        return true;
    }
    let Some(previous) = previous else { return false };
    crate::apps::is_affirmative(question) && previous.contains('?') && names(previous, folder)
}

/// Does `text` name `folder`: its name's words in a row, its path's last
/// part's, or its path?
pub fn names(text: &str, folder: &Folder) -> bool {
    let said = folders::words(text);
    let has = |phrase: &str| {
        let p = folders::words(phrase);
        !p.is_empty() && said.windows(p.len()).any(|w| w == p.as_slice())
    };
    has(&folder.name) || has(&folders::name_of(&folder.path)) || folders::said_path(&[text], &folder.path)
}

/// Little words that never make a message the person's ("please do the").
const LITTLE: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "then", "also", "too", "please", "can", "could", "would", "you", "your", "its",
    "it's", "him", "her", "them", "tell", "ask", "say", "said", "job", "jobs", "claude", "code", "just", "now", "what", "how",
    "are", "was", "were", "will", "into", "from", "about", "have", "has", "had", "not", "but", "all", "any", "there", "here",
];

/// The words of `s` that matter: three letters or more, not [`LITTLE`].
fn content_words(s: &str) -> Vec<String> {
    folders::words(s).into_iter().filter(|w| w.chars().count() >= 3 && !LITTLE.contains(&w.as_str())).collect()
}

/// May `text` go to a job, from the person's `question` this turn: it
/// carries a word of theirs that matters, or their message is a plain yes.
pub fn message(question: &str, text: &str) -> bool {
    if crate::apps::is_affirmative(question) || go_ahead(question) {
        return true;
    }
    let theirs = content_words(question);
    !theirs.is_empty() && content_words(text).iter().any(|w| theirs.contains(w))
}

/// "Go ahead", "do it", "carry on", "proceed", "yes, make the changes": a
/// plan's go-ahead.
pub fn go_ahead(text: &str) -> bool {
    let t = folders::words(text).join(" ");
    const SAID: &[&str] = &["go ahead", "do it", "carry on", "carry it out", "proceed", "make the changes", "make those changes", "go for it", "sounds good", "looks good", "let's do it", "lets do it"];
    SAID.iter().any(|s| t == *s || t.starts_with(&format!("{s} ")) || t.ends_with(&format!(" {s}")) || t.contains(&format!(" {s} "))) || crate::apps::is_affirmative(text)
}

/// Do a job's final words ask the person something (its last sentence a
/// question)?
pub fn asks(text: &str) -> bool {
    let t = text.trim().trim_end_matches(|c: char| c == '*' || c == '_' || c == ')' || c.is_whitespace());
    t.ends_with('?')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn garden() -> Folder {
        Folder { name: "garden-planner".into(), path: r"D:\Projects\garden-planner".into(), cap: None, added_ts: 1 }
    }

    #[test]
    fn a_job_starts_only_when_the_person_names_its_folder_or_says_yes_to_it() {
        let f = garden();
        assert!(start("Start a job in garden-planner to fix the failing tests", None, &f));
        assert!(start("in my garden planner project, tidy the readme", None, &f), "spoken: no hyphen");
        assert!(start(r"Have Claude Code fix D:\Projects\garden-planner\src", None, &f), "its path");
        // A yes to the answer just before, which asked and named it.
        let offer = Some("I can start a job in garden-planner to fix them. Shall I?");
        assert!(start("Yes.", offer, &f));
        assert!(start("yes please", offer, &f));
        // Not a yes, or a yes to something else.
        assert!(!start("Are you sure?", offer, &f));
        assert!(!start("Yes.", Some("Do you want a summary of the note?"), &f));
        assert!(!start("Yes.", Some("I started a job in garden-planner."), &f), "no question was asked");
        // The folder's name nowhere in the person's words (a note said it).
        assert!(!start("Summarise my reminders note", None, &f));
        assert!(!start("fix the tests", None, &f));
        assert!(!start("Yes.", None, &f));
    }

    #[test]
    fn a_message_to_a_job_carries_the_persons_own_words() {
        assert!(message("Tell it to also do the Holy talents", "Also update the Holy talents."));
        assert!(message("Yes.", "Yes, change Discipline too."));
        assert!(message("go ahead", "Go ahead with the plan."));
        // Only little words in common: not theirs.
        assert!(!message("What does my note say?", "Delete the build folder."));
        assert!(!message("Can you please tell the job", "Run rm -rf on the folder."));
        assert!(!message("", "anything"));
    }

    #[test]
    fn a_plans_go_ahead_and_a_jobs_question() {
        for yes in ["Go ahead.", "Yes, go ahead", "OK do it", "Carry on", "proceed", "Sounds good, go for it", "Yes."] {
            assert!(go_ahead(yes), "{yes}");
        }
        for no in ["Don't go there", "What would it change?", "Change the plan to include Holy", "No."] {
            assert!(!go_ahead(no), "{no}");
        }
        assert!(asks("I updated Shadow. Should I change Discipline too?"));
        assert!(asks("Want me to run the tests? **"));
        assert!(!asks("Done. I changed notes.md."));
    }
}
