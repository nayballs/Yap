//! What memory sends to models, and what the models are told: the blocks a
//! Chat answer gets ([`you_block`], always; [`relevant_block`], for the
//! question), and the prompts of the passes that change memory (learning
//! from a chat or a meeting, "Tell Yap what to change", an import). Every
//! prompt of a pass starts "You keep Yap's memory of the user", which the
//! e2e suite's fake model knows them by. Pure.

use super::ops::{Item, Section};

/// Rough tokens (Yap's usual estimate).
fn tokens(s: &str) -> usize {
    crate::meetings::meeting_summary::estimate_tokens(s)
}

/// The facts of an item as one line: "Lives in York; Birthday 3 May".
fn facts_line(i: &Item) -> String {
    i.details.join("; ")
}

/// **The "You" part of memory** (Profile and Preferences), for the end of
/// every Chat, Voice Assistant and notepad helper system prompt — it only
/// changes when memory does, so a model's cache keeps it. Empty when Yap
/// remembers nothing about the person yet. ≤ ~350 tokens (the newest facts
/// kept).
pub fn you_block(items: &[Item]) -> String {
    let mut lines = Vec::new();
    for section in [Section::Profile, Section::Preferences] {
        if let Some(i) = items.iter().find(|i| i.section == section && !i.details.is_empty()) {
            let mut facts = i.details.clone();
            // Newest last: drop the oldest past the budget.
            while facts.len() > 1 && tokens(&facts.join("; ")) > 160 {
                facts.remove(0);
            }
            let label = if section == Section::Profile { "About them" } else { "How they want answers (follow it)" };
            lines.push(format!("- {label}: {}.", facts.join("; ")));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "What Yap remembers about the user from earlier chats (they see and change it in Settings → Memory):\n{}\nAnswer the way they want answers. Use the rest when it helps the question; don't bring it up otherwise.",
        lines.join("\n")
    )
}

/// How the person wants answers (Preferences), for the end of a question's
/// context, where a small model heeds it: in the system prompt alone,
/// Qwen3.5-4B answered "two or three sentences, no lists" with 138 words
/// and a numbered list (the model evaluations, 2026-10-22). `None` without
/// preferences.
pub fn answer_note(items: &[Item]) -> Option<String> {
    let prefs = items.iter().find(|i| i.section == Section::Preferences && !i.details.is_empty())?;
    Some(format!("How the user wants answers (from Yap's memory, follow it): {}.", prefs.details.join("; ")))
}

/// Words that say nothing about which memory a question is about (the eager
/// note search's list, `tools.rs`, plus a few).
fn is_stop(w: &str) -> bool {
    const STOP: &[&str] = &[
        "and", "the", "for", "are", "was", "were", "what", "what's", "whats", "when", "when's", "where", "where's", "which",
        "who", "who's", "whose", "why", "how", "how's", "with", "this", "that", "that's", "these", "those", "from", "have",
        "has", "had", "does", "did", "can", "could", "would", "should", "will", "about", "into", "your", "you", "our",
        "their", "there", "then", "than", "them", "they", "she", "her", "him", "his", "its", "it's", "any", "all", "some",
        "one", "also", "just", "not", "but", "yet", "too", "very", "much", "many", "more", "most", "get", "got", "i'm",
        "i've", "i'll", "let's", "isn't", "don't", "doesn't", "tell", "show", "give", "find", "know", "need", "want",
        "please", "thanks", "remind", "again", "still", "now", "today", "my", "me", "mine", "yap", "remember", "like",
        "good", "make", "time", "something", "anything", "think", "help", "out", "use", "new", "next", "last", "day",
        "week", "year", "going", "really", "well",
    ];
    STOP.contains(&w)
}

/// A question's words as the memory search reads them: lowercase, without
/// stop words, "Rochelle's" as "rochelle", plural "sisters" also "sister".
fn words(q: &str) -> Vec<String> {
    q.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').trim_end_matches("'s").to_string())
        .filter(|w| w.chars().count() > 2 && !is_stop(w))
        .collect()
}

/// Does `text` hold `w` as a whole word (or its plural)?
fn has_word(text: &str, w: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|t| t == w || t.strip_suffix('s') == Some(w) || w.strip_suffix('s') == Some(t))
}

/// The people, projects and topics a question is about, best first: a
/// keyword score like the eager note search's — the name ×3, who or what it
/// is ×2, its facts ×1 — and only items that match the name, or two of the
/// question's words ("When's my sister's birthday?" finds Rochelle, "Sister"
/// with a birthday fact). At most `limit`.
pub fn relevant(items: &[Item], question: &str, limit: usize) -> Vec<Item> {
    let words = words(question);
    if words.is_empty() {
        return Vec::new();
    }
    let need = words.len().min(2);
    let mut scored: Vec<(usize, u64, &Item)> = items
        .iter()
        .filter(|i| !i.section.is_you())
        .filter_map(|i| {
            let title = i.title.to_lowercase();
            let about = i.summary.to_lowercase();
            let facts = i.details.join(" \n ").to_lowercase();
            let mut score = 0;
            let mut distinct = 0;
            let mut named = false;
            for w in &words {
                let (t, a, f) = (has_word(&title, w), has_word(&about, w), has_word(&facts, w));
                score += 3 * t as usize + 2 * a as usize + f as usize;
                distinct += (t || a || f) as usize;
                named |= t;
            }
            (named || distinct >= need).then_some((score, i.updated_ts, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    scored.into_iter().take(limit).map(|(_, _, i)| i.clone()).collect()
}

/// The question's memory, for its `<context>` block (a tool model) or the
/// system prompt (one without tools): `None` when nothing matches.
pub fn relevant_block(items: &[Item], question: &str) -> Option<String> {
    let found = relevant(items, question, 3);
    if found.is_empty() {
        return None;
    }
    let lines: Vec<String> = found
        .iter()
        .map(|i| {
            let mut line = format!("- {} ({})", i.title, i.section.singular());
            if !i.summary.is_empty() {
                line.push_str(&format!(": {}.", i.summary.trim_end_matches('.')));
            }
            if !i.details.is_empty() {
                line.push_str(&format!(" {}.", facts_line(i)));
            }
            line
        })
        .collect();
    Some(format!("From Yap's memory (what the user told Yap before; use it if it helps):\n{}", lines.join("\n")))
}

/// Memory as a pass reads it: one line per item with its id, the newest
/// first while they fit in `budget` tokens. "Nothing yet." when empty.
pub fn listing(items: &[Item], budget: usize) -> String {
    let mut sorted: Vec<&Item> = items.iter().collect();
    // Profile, Preferences, then the newest.
    sorted.sort_by_key(|i| {
        let you = if i.section.is_you() { i.section as u8 } else { u8::MAX };
        (you, std::cmp::Reverse(i.updated_ts))
    });
    let mut lines = Vec::new();
    let mut used = 0;
    for i in sorted {
        let line = if i.section.is_you() {
            format!("[{}] {}: {}", i.id, i.section.key(), i.details.join(" | "))
        } else {
            let mut l = format!("[{}] {} \"{}\"", i.id, i.section.singular(), i.title);
            let mut rest: Vec<String> = Vec::new();
            if !i.summary.is_empty() {
                rest.push(i.summary.clone());
            }
            rest.extend(i.details.iter().cloned());
            if !rest.is_empty() {
                l.push_str(&format!(": {}", rest.join(" | ")));
            }
            l
        };
        let t = tokens(&line);
        if used + t > budget {
            break;
        }
        used += t;
        lines.push(line);
    }
    if lines.is_empty() {
        "Nothing yet.".to_string()
    } else {
        lines.join("\n")
    }
}

/// How every pass writes its changes (and what "section" means).
const FORMAT: &str = r#"Reply with JSON only: {"changes": [ ... ]}, where each change is one of:
{"action": "add", "section": "profile" | "preferences" | "people" | "projects" | "topics", "name": "...", "about": "...", "facts": ["..."], "say": "..."}
{"action": "update", "id": 12, "about": "...", "facts": ["..."], "remove_facts": ["..."], "say": "..."}
{"action": "remove", "id": 12, "say": "..."}
- profile: who the user is: their work, where they live, their pets, what they do (facts only, no name or about). preferences: how they want answers (facts only). people: someone in their life (name = that person's name; about = who they are to the user, like "Sister" or "Colleague at Acme"). projects: something they're working on or building (name = its name, like "Yap"). topics: a lasting interest.
- facts: short, a few words each, without "the user": "Lives in Norwich", "Works night shifts".
- update: an item Yap already has, by its [id] in "What Yap remembers now" (never any other number): "facts" adds facts, "remove_facts" takes out facts as written. When something changed, take the old fact out and put the new one in, in the same update. Never add a second item for someone or something Yap has.
- remove: a whole person, project or topic. Profile and preferences are never removed: take out their facts with remove_facts.
- say: the change in a few words for the user, in the second person: "your sister is Rochelle".

Examples:
Chat: "User: My brother Tom is visiting from Glasgow, where should we eat tonight?"
{"changes": [{"action": "add", "section": "people", "name": "Tom", "about": "Brother", "facts": ["Lives in Glasgow"], "say": "your brother Tom lives in Glasgow"}]}
Memory has [3] person "Tom": Brother | Lives in Glasgow. Chat: "User: Tom just moved to Norwich and starts at the BBC next week."
{"changes": [{"action": "update", "id": 3, "facts": ["Lives in Norwich", "Works at the BBC"], "remove_facts": ["Lives in Glasgow"], "say": "Tom lives in Norwich now"}]}
Chat: "User: What's a good film for tonight?"
{"changes": []}"#;

/// Never kept, whoever asks.
const NEVER: &str = "Never keep passwords, keys, codes, card or account numbers, addresses, phone numbers or email addresses.";

/// The line on sensitive topics: kept only when the setting allows.
fn sensitive_line(allowed: bool) -> &'static str {
    if allowed {
        "Sensitive topics (health, religion, politics, sexuality, ethnicity, criminal records) may be kept when the user tells them about themselves and they would help later."
    } else {
        "Never keep sensitive topics: health, religion, politics, sexuality, ethnicity or immigration, criminal records, union membership — the user asked Yap not to."
    }
}

/// The learning pass over a chat's latest messages.
pub fn learn_chat_system(sensitive: bool) -> String {
    format!(
        "You keep Yap's memory of the user: short, lasting facts that help Yap's AI answer them in later chats. You get what Yap remembers now, each item with its [id], and the latest part of a chat between the user and Yap's AI.\n\nKeep what the user says about themselves that will still be true in a month: their work, where they live, the people in their life by name (who each is to them: sister, colleague and their role…), pets, birthdays, the projects they work on, lasting interests, and how they want answers.\nNever keep:\n- passing details: today's plans, a one-off task, what they're doing right now;\n- things they asked about that aren't about them: a recipe, a film, a place, a fact;\n- what Yap's AI said, unless the user confirmed it about themselves;\n- other people's private details (their health, family troubles, relationships): only who they are to the user, their role and how the user knows them.\n{NEVER} {}\nWhen the chat holds nothing like that, reply {{\"changes\": []}}.\n\n{FORMAT}\nAt most 5 changes.",
        sensitive_line(sensitive)
    )
}

/// The learning pass over a finished meeting (its attendees and action plan).
pub fn learn_meeting_system(sensitive: bool) -> String {
    format!(
        "You keep Yap's memory of the user: short, lasting facts that help Yap's AI answer them later. You get what Yap remembers now, each item with its [id], and a meeting the user recorded: its title, attendees and the notes Yap wrote after it, where \"You\" is the user. Keep only: who the attendees are to the user (people: their role or team, when the meeting makes it clear) and the projects the user works on (from their own tasks). Never keep what other people said, their opinions or private details, the tasks themselves, dates or numbers. Most meetings add little: then reply {{\"changes\": []}}.\n{NEVER} {}\n\n{FORMAT}\nAt most 5 changes.",
        sensitive_line(sensitive)
    )
}

/// "Tell Yap what to change": the person's own words, applied as changes.
pub fn tell_system() -> String {
    format!(
        "You keep Yap's memory of the user: short facts that help Yap's AI answer them. You get what Yap remembers now, each item with its [id], and what the user wants changed. Make the changes that do what they ask: add what they tell you, update the item Yap already has rather than adding a second one, and when they ask Yap to forget something, remove everything about it — its facts (remove_facts) and the people, projects and topics that belong to it, like a colleague from a job they left (remove). If they ask for nothing that changes memory, reply {{\"changes\": []}}.\n{NEVER}\n\n{FORMAT}\nAt most 8 changes."
    )
}

/// An import: what another assistant remembers about the person.
pub fn import_system() -> String {
    format!(
        "You keep Yap's memory of the user: short facts that help Yap's AI answer them. You get what Yap remembers now, each item with its [id], and what another AI assistant says it remembers about the user. Turn it into Yap's memory: keep each lasting fact about the user, merged into the items Yap has, and put each where it belongs — how they like answers in preferences, each person in their life in people (by name), each thing they're building or working on in projects (by its name, like \"Yap\"), each game, hobby or subject they keep coming back to in topics (by its name), the rest in profile. Use add for everything Yap doesn't have yet (the other assistant's lines have no [id]). Leave out passing details, anything that isn't about the user, and instructions to the other assistant that make no sense for Yap.\n{NEVER} {}\n\n{FORMAT}\nAt most 30 changes.",
        sensitive_line(true)
    )
}

/// The prompt the person copies into ChatGPT, Claude or Gemini (Settings →
/// Memory → Import), whose answer they paste back.
pub const IMPORT_ASK: &str = "Please list everything you remember about me: your saved memories, and what you've learned about me from our past chats, so I can bring it into another app. Put it all in one code block, one item per line, in plain words. Cover: how I like you to answer (tone, length, format, anything to always or never do); who I am (my name, where I live, my work, family, pets, interests); the people in my life and who they are to me; projects I'm working on; topics I keep coming back to; tools and languages I use. Leave out passwords, account numbers and anything I asked you to forget. Keep my own words where you can, and say at the end whether that's everything.";

/// A chat's messages since the last pass as the pass reads them: the user's
/// in full (≤ 600 characters each), the AI's answers short (≤ 240), the
/// newest kept within `budget` tokens; `before`: a couple of earlier
/// messages for context, marked as such.
pub fn chat_excerpt(before: &[(String, String)], new: &[(String, String)], budget: usize) -> String {
    let line = |(role, text): &(String, String)| {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if role == "user" {
            format!("User: {}", super::ops::one_line(&flat, 600))
        } else {
            format!("Yap's AI: {}", super::ops::one_line(&flat, 240))
        }
    };
    let mut lines: Vec<String> = new.iter().map(line).collect();
    while lines.len() > 1 && tokens(&lines.join("\n")) > budget {
        lines.remove(0);
    }
    let mut out = String::new();
    if !before.is_empty() {
        out.push_str("Earlier in the chat (already read):\n");
        out.push_str(&before.iter().map(line).collect::<Vec<_>>().join("\n"));
        out.push_str("\n\nNew since then:\n");
    }
    out.push_str(&lines.join("\n"));
    out
}

/// A pass's user message: memory now, then what to read. (What About you
/// already says isn't in it: told it, Qwen3.5-4B wrote About you back and
/// left the chat's new fact out, 2026-10-22; `ops::drop_about_you` takes
/// the repeats out after instead.)
pub fn pass_input(listing: &str, heading: &str, body: &str) -> String {
    format!("What Yap remembers now:\n{listing}\n\n{heading}:\n{body}")
}

/// Is a chat's new part worth a pass? Something the user said about
/// themselves ("I", "my", "we", "our"…), or asked Yap to remember: else
/// there's nothing to learn and no model call is made (Groq's free plan
/// counts every token).
pub fn worth_a_pass(user_texts: &[&str]) -> bool {
    static CUE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:i|i'm|im|i've|ive|i'd|i'll|me|my|mine|myself|we|we're|we've|our|ours|us|remember|call me)\b")
            .expect("static regex")
    });
    user_texts.iter().any(|t| CUE.is_match(&t.replace('’', "'")))
}

/// The changes' JSON schema for a model on this PC (llama.cpp makes it a
/// grammar): every field required, in the order the prompt writes them —
/// `id` 0, `name` / `about` "" and empty lists where they don't apply (Yap
/// reads those as absent).
pub fn schema(max_changes: usize) -> serde_json::Value {
    let text = serde_json::json!({ "type": "string" });
    let list = serde_json::json!({ "type": "array", "items": { "type": "string" }, "maxItems": 8 });
    serde_json::json!({
        "type": "object",
        "properties": {
            "changes": {
                "type": "array",
                "maxItems": max_changes,
                "items": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["add", "update", "remove"] },
                        "id": { "type": "integer" },
                        "section": { "type": "string", "enum": ["profile", "preferences", "people", "projects", "topics"] },
                        "name": text,
                        "about": text,
                        "facts": list,
                        "remove_facts": list,
                        "say": text
                    },
                    "required": ["action", "id", "section", "name", "about", "facts", "remove_facts", "say"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["changes"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::super::ops::{Item, Origin, Section};
    use super::*;

    fn item(id: u64, section: Section, title: &str, summary: &str, details: &[&str]) -> Item {
        Item {
            id,
            section,
            title: title.into(),
            summary: summary.into(),
            details: details.iter().map(|d| d.to_string()).collect(),
            source: Origin::new("chat", None, ""),
            created_ts: id,
            updated_ts: id,
            dictation: true,
        }
    }

    fn sample() -> Vec<Item> {
        vec![
            item(1, Section::Profile, "Profile", "", &["Works as a nurse", "Lives in Norwich"]),
            item(2, Section::Preferences, "Preferences", "", &["Short answers"]),
            item(3, Section::People, "Rochelle", "Sister", &["Lives in York", "Birthday 3 May"]),
            item(4, Section::People, "Sam Patel", "Colleague at St James's", &["Product manager"]),
            item(5, Section::Projects, "Allotment", "Their vegetable plot", &["Grows tomatoes"]),
            item(6, Section::Topics, "Baldur's Gate 3", "A game they play", &[]),
        ]
    }

    #[test]
    fn the_you_block_goes_with_every_answer() {
        let block = you_block(&sample());
        assert!(block.contains("About them: Works as a nurse; Lives in Norwich."), "{block}");
        assert!(block.contains("How they want answers (follow it): Short answers."), "{block}");
        assert!(!block.contains("Rochelle"), "people only when relevant: {block}");
        assert_eq!(you_block(&[]), "");
    }

    #[test]
    fn only_what_a_question_is_about_comes_with_it() {
        let items = sample();
        let titles = |q: &str| relevant(&items, q, 3).iter().map(|i| i.title.clone()).collect::<Vec<_>>();
        assert_eq!(titles("When is Rochelle's birthday?"), ["Rochelle"]);
        assert_eq!(titles("When's my sister's birthday?"), ["Rochelle"]);
        assert_eq!(titles("What should I get Sam for his leaving do?"), ["Sam Patel"]);
        assert_eq!(titles("Any tips for my allotment tomatoes?"), ["Allotment"]);
        // Nothing to do with memory: nothing comes with it.
        assert!(titles("What's the capital of France?").is_empty());
        assert!(titles("How long do I boil an egg?").is_empty());
        assert!(titles("Write me a poem about the sea").is_empty());
        // One shared word that isn't the name isn't enough.
        assert!(titles("Where does the product launch happen?").is_empty());
        let block = relevant_block(&items, "When is Rochelle's birthday?").unwrap();
        assert_eq!(block, "From Yap's memory (what the user told Yap before; use it if it helps):\n- Rochelle (person): Sister. Lives in York; Birthday 3 May.");
    }

    #[test]
    fn a_pass_reads_memory_with_ids_and_the_chat_short() {
        let l = listing(&sample(), 1_000);
        assert!(l.starts_with("[1] profile: Works as a nurse | Lives in Norwich\n[2] preferences: Short answers"), "{l}");
        assert!(l.contains("[3] person \"Rochelle\": Sister | Lives in York | Birthday 3 May"), "{l}");
        assert_eq!(listing(&[], 100), "Nothing yet.");
        let ex = chat_excerpt(
            &[("user".into(), "Hi".into())],
            &[("user".into(), "My sister Rochelle is visiting".into()), ("assistant".into(), "x ".repeat(400))],
            2_000,
        );
        assert!(ex.contains("Earlier in the chat (already read):\nUser: Hi"), "{ex}");
        assert!(ex.contains("User: My sister Rochelle is visiting"), "{ex}");
        assert!(ex.lines().last().unwrap().chars().count() < 260);
    }

    #[test]
    fn a_chat_with_nothing_about_the_user_gets_no_pass() {
        assert!(!worth_a_pass(&["What's the capital of France?", "How do volcanoes work"]));
        assert!(worth_a_pass(&["My sister's coming over"]));
        assert!(worth_a_pass(&["I’m a nurse"]));
        assert!(worth_a_pass(&["Remember that the code freeze is Friday"]));
        assert!(worth_a_pass(&["we just moved to Norwich"]));
    }

    #[test]
    fn every_pass_prompt_starts_the_same() {
        for p in [learn_chat_system(false), learn_meeting_system(true), tell_system(), import_system()] {
            assert!(p.starts_with("You keep Yap's memory of the user"), "{p}");
            assert!(p.contains("Never keep passwords"), "{p}");
        }
        assert!(learn_chat_system(false).contains("Never keep sensitive topics"));
        assert!(learn_chat_system(true).contains("may be kept"));
        // A pass stays light for an 8k model.
        // (≈ 950 with its examples, + ≤ 900 of memory, ≤ 1,800 of chat and
        // an 800-token reply: within a 4k-token budget on an 8k model.)
        assert!(tokens(&learn_chat_system(false)) < 1_050, "{}", tokens(&learn_chat_system(false)));
    }
}
