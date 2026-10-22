//! **Memory feeds dictation**: the names of the people and projects Yap
//! remembers become dictation vocabulary, conservatively.
//!
//! - Every name whose "Spell this name in dictation" is on goes to the
//!   Whisper `initial_prompt` (after the dictionary's own words, ranked like
//!   them by use: `dict_usage`) and the meeting prompt.
//! - For ONNX models (Parakeet, the default) the only way to fix a name is
//!   the fuzzy near-miss pass (`fuzzy.rs`), which could swallow ordinary
//!   words. A name joins it only when it can't: ≥ 4 letters, not an
//!   everyday word (`learn::is_frequent`, then Windows' spell checker: a
//!   word it knows in lower case is an ordinary word — "will", "rose",
//!   "parakeet" — while a name like "rochelle" it doesn't know in lower
//!   case), and not a
//!   first name that's also a word ([`NAME_WORDS`]). Topics never do (their
//!   names are mostly ordinary words). Each word of a longer name that
//!   passes joins too ("Sarah Jenkins": "Jenkins").
//! - And what was **heard** must not be an ordinary word either
//!   (`fuzzy::apply_names`, run in `pipeline::apply_corrections` after the
//!   dictionary's own pass): names alone don't keep ordinary words safe —
//!   without that check "a rescue" became "Archie" and "olive oil" "Olivia
//!   oil". A PC without a spell checker has no near-miss targets at all
//!   (`memory::Words`).
//!
//! The entries are dictionary entries with no trigger (`from` empty): the
//! exact pass skips them, the fuzzy pass matches their spelling.

use crate::config::DictionaryEntry;

use super::ops::{Item, Section};

/// First names that are also everyday words (and words a name often is):
/// never near-miss targets, whatever the spell checker says.
pub const NAME_WORDS: &[&str] = &[
    "will", "mark", "bill", "rose", "hope", "faith", "grace", "dawn", "ruby", "jade", "amber", "holly", "summer", "autumn",
    "april", "june", "may", "august", "hunter", "chase", "wade", "drew", "gene", "miles", "frank", "rich", "pierce",
    "grant", "lance", "dean", "earl", "penny", "daisy", "iris", "lily", "violet", "heather", "robin", "jasmine",
    "crystal", "angel", "river", "stone", "brook", "carol", "sandy", "rusty", "buck", "pearl", "ginger", "olive",
    "basil", "sage", "hazel", "fern", "willow", "cliff", "glen", "dale", "heath", "nick", "kitty", "misty", "destiny",
    "harmony", "melody", "honey", "candy", "cherry", "rocky", "marshall", "porter", "mason", "taylor", "parker",
    "cooper", "carter", "archer", "baker", "fisher", "gardner", "walker", "ward", "wolf", "king", "prince", "duke",
    "sterling", "price", "major", "royal", "sunny", "storm", "rain", "winter", "spring", "snow", "jack", "harry",
    "jerry", "terry", "barry", "gary", "sky", "jay", "joy", "art", "pat", "sue", "max", "ray", "rex", "rob", "bob",
    "don", "ken", "len", "ted", "tom", "tim", "jim", "ben", "dan", "sam", "kim", "lou", "mo", "eve", "ivy", "ash",
    "bliss", "blaze", "brandy", "chance", "charity", "cole", "colt", "dusty", "ellis", "forest", "gale", "golden",
    "haven", "ivory", "journey", "justice", "kai", "lark", "legend", "liberty", "lucky", "marina", "mercy", "noble",
    "patience", "pepper", "poppy", "precious", "raven", "reed", "ridge", "rowan", "scout", "shadow", "shelby",
    "sparrow", "stormy", "true", "trinity", "unity", "vale", "wren", "york", "paris", "florence", "georgia", "virginia",
    "carolina", "sydney", "phoenix", "china", "india", "jordan", "chad", "kent", "lincoln", "jersey", "derby", "chester",
    "dallas", "denver", "austin", "houston", "brooklyn", "camden", "dakota", "devon", "eden", "hudson", "laurel",
    "lane", "page", "piper", "sage", "shepherd", "skipper", "smith", "tanner", "turner", "wells", "young", "may",
];

/// Words of a name that can't be a near-miss target: too short, everyday,
/// a name-word, or a word the spell checker knows in lower case.
fn unsafe_word(word: &str, is_word: &dyn Fn(&str) -> bool) -> bool {
    let key: String = word.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect();
    let letters = key.chars().filter(|c| c.is_alphanumeric()).count();
    letters < 4 || crate::learn::is_frequent(&key) || NAME_WORDS.contains(&key.as_str()) || is_word(&key)
}

/// May `name` be a near-miss target? Every word of it must be safe ([`unsafe_word`]).
pub fn safe_for_fuzzy(name: &str, is_word: &dyn Fn(&str) -> bool) -> bool {
    let words: Vec<&str> = name.split_whitespace().collect();
    !words.is_empty() && words.iter().all(|w| !unsafe_word(w, is_word))
}

/// At most this many names (the newest first).
pub const MAX_NAMES: usize = 40;

/// The vocabulary from `items` (people and projects with "Spell this name in
/// dictation" on, the newest first), leaving out what `dictionary` already
/// spells. `is_word`: "is this lowercased word an ordinary word?" (Windows'
/// spell checker in the app, a stand-in in tests). Pure over its inputs.
pub fn entries(items: &[Item], dictionary: &[DictionaryEntry], is_word: &dyn Fn(&str) -> bool) -> Vec<DictionaryEntry> {
    let mut named: Vec<&Item> = items
        .iter()
        .filter(|i| matches!(i.section, Section::People | Section::Projects) && i.dictation)
        .collect();
    named.sort_by_key(|i| std::cmp::Reverse(i.updated_ts));
    let mut seen: std::collections::HashSet<String> = dictionary
        .iter()
        .filter(|e| !e.snippet)
        .flat_map(|e| [e.to.trim().to_lowercase(), e.from.trim().to_lowercase()])
        .filter(|s| !s.is_empty())
        .collect();
    let mut out = Vec::new();
    for i in named.into_iter().take(MAX_NAMES) {
        let name = i.title.trim();
        if name.is_empty() {
            continue;
        }
        let words: Vec<&str> = name.split_whitespace().collect();
        let mut add = |to: &str, fuzzy: bool| {
            if seen.insert(to.to_lowercase()) {
                out.push(DictionaryEntry { from: String::new(), to: to.to_string(), fuzzy, snippet: false });
            }
        };
        add(name, safe_for_fuzzy(name, is_word));
        // "Sarah Jenkins": each word that's safe alone ("Jenkins") too.
        if words.len() > 1 {
            for w in words {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                if safe_for_fuzzy(w, is_word) {
                    add(w, true);
                }
            }
        }
    }
    out
}

/// "Spell this name in dictation": does the near-miss pass correct it, or
/// only the Whisper prompt? For the Settings row's hint.
pub fn name_hint(name: &str, is_word: &dyn Fn(&str) -> bool) -> &'static str {
    if safe_for_fuzzy(name, is_word) {
        "fixed"
    } else {
        "prompt"
    }
}

#[cfg(test)]
mod tests {
    use super::super::ops::{Item, Origin, Section};
    use super::*;

    /// The tests' "spell checker": a few words it knows in lower case.
    fn words(w: &str) -> bool {
        ["parakeet", "phoenix", "harry", "grace", "beans", "patel"].contains(&w)
    }

    fn person(id: u64, name: &str) -> Item {
        Item {
            id,
            section: Section::People,
            title: name.into(),
            summary: String::new(),
            details: vec![],
            source: Origin::new("chat", None, ""),
            created_ts: id,
            updated_ts: id,
            dictation: true,
        }
    }

    #[test]
    fn only_names_that_cant_swallow_words_are_fixed() {
        for ok in ["Rochelle", "Siobhan", "Priya", "Tanay", "Kubernetes", "Jenkins", "Aoife", "Callum", "Sarah Jenkins"] {
            assert!(safe_for_fuzzy(ok, &words), "{ok}");
        }
        for not in ["Will", "Mark", "Rose", "Sam", "Jo", "Ben", "Grace", "Parakeet", "Harry", "Hope", "Yap", "Summer", "Sarah Will"] {
            assert!(!safe_for_fuzzy(not, &words), "{not}");
        }
    }

    #[test]
    fn people_and_projects_become_vocabulary() {
        let mut items = vec![person(1, "Rochelle"), person(2, "Will"), person(3, "Sarah Jenkins"), person(4, "Kubernetes")];
        items[3].section = Section::Projects;
        let mut off = person(5, "Siobhan");
        off.dictation = false;
        items.push(off);
        let mut topic = person(6, "Grafana");
        topic.section = Section::Topics;
        items.push(topic);
        let dict = vec![DictionaryEntry { from: "cube and eighties".into(), to: "Kubernetes".into(), fuzzy: true, snippet: false }];
        let got: Vec<(String, bool)> = entries(&items, &dict, &words).into_iter().map(|e| (e.to, e.fuzzy)).collect();
        assert_eq!(
            got,
            [
                ("Sarah Jenkins".to_string(), true),
                ("Sarah".to_string(), true),
                ("Jenkins".to_string(), true),
                ("Will".to_string(), false),
                ("Rochelle".to_string(), true),
            ]
        );
        // No trigger: the exact pass never touches them.
        assert!(entries(&items, &[], &words).iter().all(|e| e.from.is_empty() && !e.snippet));
    }

    /// Everyday sentences with the names Yap remembers as near-miss targets:
    /// not one word changes (the names pass, as an ONNX dictation runs it,
    /// with this PC's own spell checker — without one, no name is a target
    /// at all, `memory::Words`). Without the spell checker's heard-side
    /// check, "a rescue" became "Archie" and "olive oil" "Olivia oil"
    /// (2026-10-22): names alone can't keep ordinary words safe.
    #[test]
    fn no_everyday_sentence_changes() {
        let ordinary = crate::learn::Ordinary::new();
        if !ordinary.has_spell_checker() {
            eprintln!("No spell checker on this PC: memory's names are never near-miss targets here.");
            return;
        }
        let is_word = |w: &str| ordinary.is(w);
        let names = [
            "Rochelle", "Siobhan", "Priya", "Tanay", "Sarah Jenkins", "Callum", "Aoife", "Oliver", "Olivia", "Charlotte",
            "Dave", "Becky", "Nathan", "Emma", "Sophie", "Lucy", "Chloe", "Megan", "Hannah", "Jessica", "Rebecca", "Thomas",
            "James", "Daniel", "Matthew", "Joseph", "Samuel", "George", "Arthur", "Freddie", "Alfie", "Archie", "Theo",
            "Leo", "Isla", "Amelia", "Mia", "Ava", "Ella", "Zara", "Aisha", "Mohammed", "Fatima", "Wei", "Hiroshi",
            "Kubernetes", "Grafana", "Terraform", "Figma", "Notion", "Slack", "Jira", "Confluence", "Salesforce",
            "Shopify", "Kafka", "Postgres", "Redis", "Nginx", "Django", "Flask", "React", "Svelte", "Tauri", "Parakeet",
            "Yap", "Acme", "Bloom", "Harbour", "Lumen", "Orion", "Atlas", "Nova", "Apollo", "Sasha", "Kieran", "Declan",
            "Niamh", "Saoirse", "Ciara", "Eoin", "Ruairi", "Gareth", "Rhys", "Dylan", "Megan", "Bethan", "Ffion",
        ];
        let items: Vec<Item> = names.iter().enumerate().map(|(i, n)| person(i as u64 + 1, n)).collect();
        // Every name (memory gives dictation its newest MAX_NAMES at a time).
        let vocab: Vec<DictionaryEntry> = items.chunks(MAX_NAMES).flat_map(|c| entries(c, &[], &is_word)).collect();
        let targets: Vec<&str> = vocab.iter().filter(|e| e.fuzzy).map(|e| e.to.as_str()).collect();
        println!("{} of {} names are near-miss targets: {}", targets.len(), vocab.len(), targets.join(", "));
        assert!(targets.len() > 40, "most names are near-miss targets");
        let sentences = [
            "Can you send me the report by Friday?",
            "I'll call you back in five minutes.",
            "We should mention it at the meeting tomorrow.",
            "The shell script failed again last night.",
            "Let's grab lunch at the new place on the high street.",
            "I need to pick up some milk, bread and eggs.",
            "Please remind me to book the dentist next week.",
            "The deadline moved to the end of the month.",
            "Could you check the mail for a parcel?",
            "She said she would think about it over the weekend.",
            "I'm running a bit late, sorry about that.",
            "Thanks for the update, that makes sense.",
            "We need more time to finish the design.",
            "The train was cancelled so I'm working from home.",
            "Let me know if you have any questions.",
            "I really enjoyed the film on Saturday.",
            "Can we move the call to half past three?",
            "The weather is meant to be lovely this weekend.",
            "I've attached the latest version of the slides.",
            "Have a great holiday and see you when you're back.",
            "The kids have a school trip on Thursday.",
            "My phone battery keeps dying.",
            "I think the issue is with the database connection.",
            "We shipped the release this morning.",
            "Our team is hiring a new designer.",
            "I'll be in the office on Monday and Tuesday.",
            "Remind me to water the plants.",
            "The meeting notes are in the shared folder.",
            "Could you review my pull request when you get a chance?",
            "I forgot my password again.",
            "We should get a takeaway tonight.",
            "Shall we go for a walk after dinner?",
            "I'm going to the gym after work.",
            "Do you want tea or coffee?",
            "The heating isn't working properly.",
            "The car needs an MOT next month.",
            "I left my keys at my mum's house.",
            "I'll pick the children up from school.",
            "We've got a lot on this week.",
            "That's a really good point, I agree.",
            "Hang on, I'll be with you in a minute.",
            "Where did you put the charger?",
            "Can you turn the music down a little?",
            "I'm not sure that's going to work.",
            "Let's try again tomorrow morning.",
            "It was nice to see you yesterday.",
            "The printer is out of paper.",
            "Please book a table for four at seven.",
            "My brother is visiting at the weekend.",
            "We need to sort out the garden before winter.",
            "Mission accomplished, the server is back up.",
            "Pretty please with a cherry on top.",
            "The sheep were in the field by the river.",
            "I'll make a note of that.",
            "Can you share your screen?",
            "I'm on mute, sorry.",
            "We're over budget on the project.",
            "The new hire starts on Monday.",
            "I'd rather stay in tonight.",
            "Happy birthday, have a lovely day!",
            "The deploy went out without any errors.",
            "Turn left at the next roundabout.",
            "The tea is too hot to drink.",
            "Is it okay if I leave early today?",
            "The match starts at eight o'clock.",
            "We had pizza for dinner.",
            "I can't find the file anywhere.",
            "Let me sleep on it.",
            "Meet me at the station at noon.",
            "I'll text you when I'm outside.",
            "That was a close call.",
            "The flat needs a good clean.",
            "We are going to Spain in July.",
            "Shall I order more printer ink?",
            "Is the shop open on Sundays?",
            "I need a haircut.",
            "Please don't forget the tickets.",
            "The baby finally fell asleep.",
            "It's raining cats and dogs.",
            "We sold the old sofa online.",
            "Who's bringing the dessert?",
            "The bus was really busy this morning.",
            "I'll check with my manager first.",
            "Let's keep it simple for now.",
            "Did you hear the news about the merger?",
            "Sorry for the late reply.",
            "I'll sort it out first thing.",
            "That's fine by me.",
            "We could try the Italian place instead.",
            "The cat knocked over a glass of water.",
            "Can you hear me okay?",
            "The internet is really slow today.",
            "I'll ask around and get back to you.",
            "Please lock the door when you leave.",
            "We ran out of milk again.",
            "The report looks good, just a few typos.",
            "My sister is coming for dinner.",
            "Bring a coat, it's cold outside.",
            "We'll need a bigger van.",
            "I'm halfway through the book.",
            "The sun is shining at last.",
            "Let's not overthink it.",
            "I'm off sick today.",
            "Shall we start the meeting?",
            "I'll see you in a bit.",
            "Our neighbours are having a party.",
            "The plumber is coming at nine.",
            "Can I borrow your charger?",
            "I'll bring some snacks.",
            "Is there any more cake?",
            "The garden looks lovely in the spring.",
            "My laptop needs an update.",
            "The office is closed on bank holidays.",
            "I'll put the kettle on.",
            "We got stuck in traffic for an hour.",
            "Could you resend the invite?",
            "I owe you one.",
            "Have you had a chance to look at it?",
            "The window cleaner came today.",
            "Let's split the bill.",
            "The parcel was left with a neighbour.",
            "I'm learning to play the guitar.",
            "I watched a documentary about whales.",
            "The bakery sells amazing sourdough.",
            "We're painting the kitchen this weekend.",
            "Nobody told me about the change.",
            "We need to talk about the budget.",
            "I'll leave it with you.",
            "Is everyone happy with that plan?",
            "Let's circle back on this next week.",
            "Please mute your microphone.",
            "We're running out of time.",
            "My train gets in at six fifteen.",
            "I've booked the hotel for two nights.",
            "We should invite the neighbours.",
            "I need to renew my passport.",
            "Remember to bring your ID.",
            "Their flight was delayed by two hours.",
            "This soup needs more salt.",
            "We are out of washing up liquid.",
            "The fridge is making a funny noise.",
            "I think I left the oven on.",
            "Can we reschedule for Thursday?",
            "The football is on tonight.",
            "Mind the gap between the train and the platform.",
            "I have a doctor's appointment at ten.",
            "Our anniversary is next month.",
            "Let's go to the beach if it's sunny.",
            "I'm saving up for a new bike.",
            "We're adopting a rescue dog.",
            "I'll message the group chat.",
            "This coffee is amazing.",
            "The shelf is a bit wonky.",
            "Shell out for the better seats.",
            "We took the row shell out on the river.",
            "A rowing shell is a narrow boat.",
            "Thomas the tank engine was on television.",
            "We need fresh herbs for the recipe.",
            "My daughter's school play is tonight.",
            "Charlotte Street is closed for roadworks.",
            "Olive oil and lemon juice.",
            "Notion of time is relative.",
            "We should react quickly.",
            "The slack in the rope is dangerous.",
            "Atlas of the world.",
            "The bloom on the roses is early.",
            "The harbour was full of boats.",
            "Nova Scotia is beautiful.",
            "Do you want to go to the cinema on Friday night?",
            "I'm feeling lucky today.",
            "Pass me the olive oil.",
            "We need to react faster.",
            "The roses are in full bloom.",
            "We walked along the harbour.",
            "They had to rescue the cat from the tree.",
            "That was a real mission.",
            "Lemon and ginger tea, please.",
            "Archive the old files first.",
            "The theme park was brilliant.",
            "I need a new pair of jeans.",
            "The mayor gave a long speech.",
            "Grab a pen and some paper.",
            "Let's have a picnic in the meadow.",
            "Stop dithering and decide.",
            "The novel was a slow burn.",
            "It's a matter of taste.",
            "We sat on the patio until late.",
            "She's a member of the choir.",
            "Turn the radio on, please.",
            "The weather forecast says sleet.",
            "Put the kettle on, I'm parched.",
            "The siren went off at noon.",
            "We need more shelving in the garage.",
            "The dentist said my teeth are fine.",
            "Our daughter loves her ballet classes.",
            "The engineer fixed the boiler.",
            "Order some flowers for the reception.",
            "My nephew is turning seven.",
            "The figures don't add up.",
            "Slack off a bit, you've earned it.",
            "Let's notion this out on paper.",
        ];
        let mut changed = Vec::new();
        for s in sentences {
            let out = crate::fuzzy::apply_names(s, &vocab, &is_word);
            if out != s {
                changed.push(format!("{s} → {out}"));
            }
        }
        assert!(changed.is_empty(), "everyday sentences changed:\n{}", changed.join("\n"));
    }

    #[test]
    fn a_misheard_name_is_fixed() {
        let items = vec![person(1, "Siobhan"), person(2, "Rochelle"), person(3, "Priya")];
        let vocab = entries(&items, &[], &words);
        let is_word = |w: &str| crate::learn::is_frequent(w) || ["row", "shell", "shiv", "lucky"].contains(&w);
        assert_eq!(crate::fuzzy::apply_names("I'll ask Shiobhan about it.", &vocab, &is_word), "I'll ask Siobhan about it.");
        assert_eq!(crate::fuzzy::apply_names("Siobahn is coming.", &vocab, &is_word), "Siobhan is coming.");
        // Too far from the name (Handy's threshold): left as heard.
        assert_eq!(crate::fuzzy::apply_names("I'll ask Shivon.", &vocab, &is_word), "I'll ask Shivon.");
        assert_eq!(crate::fuzzy::apply_names("Tell rochelle I'm late.", &vocab, &is_word), "Tell Rochelle I'm late.");
        assert_eq!(crate::fuzzy::apply_names("Pria said yes.", &vocab, &is_word), "Priya said yes.");
        // Heard as ordinary words: left alone. ("row shell" sounds and
        // spells near enough that the plain near-miss pass takes it.)
        let rowing = "We took the row shell out on the river.";
        assert_eq!(crate::fuzzy::apply_fuzzy(rowing, &vocab), "We took the Rochelle out on the river.");
        assert_eq!(crate::fuzzy::apply_names(rowing, &vocab, &is_word), rowing);
        let lucy = entries(&[person(9, "Lucy")], &[], &words);
        assert_eq!(crate::fuzzy::apply_names("I'm so lucky.", &lucy, &is_word), "I'm so lucky.");
    }
}
