//! An answer grounded in the web: whether a question needs it, the
//! search to run, the prompt a model that can't call tools answers from
//! (the fixed pipeline), and the checks on what an answer says about
//! searching.

use std::sync::LazyLock;

use regex::Regex;

use crate::chat::chats::Source;

use super::page::page_part;
use super::wiki::{game_for, has_phrase, QUERY_STOP};
use super::{site_of, tidy, Hit, DATA_NOT_INSTRUCTIONS};

/// Words that say the question is about the person's own things (notes,
/// meetings): never a web search.
const OWN_STUFF: &[&str] = &[
    "my note", "my notes", "my meeting", "my meetings", "the meeting", "the transcript", "my transcript",
    "my calendar", "my dictation", "in my notes", "notes about", "note about", "meeting notes",
    "did i write", "i wrote", "did i save", "i saved", "did i say", "did we decide", "we decided", "did we agree",
    "we agreed", "we discussed", "my list", "my shopping list", "my to-do", "my todo", "my budget",
    "did i buy", "have i bought", "what i bought",
];

/// Is the question about the person's own notes, meetings or lists ("what
/// did I note about…", "in my notes", "what did we decide…")? Then the web
/// has nothing to add: never searched for it, and an empty notes search
/// isn't followed by a web search. Pure.
pub fn about_own_stuff(question: &str) -> bool {
    static NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bnot(?:e|es|ed)\b").expect("static regex"));
    let lower = format!(" {} ", question.to_lowercase().replace('’', "'"));
    OWN_STUFF.iter().any(|p| lower.contains(p)) || NOTE.is_match(&lower.replace("patch notes", "").replace("release notes", ""))
}

/// Asking what something costs, whether it's on offer or in stock, or about
/// buying it: today's price is on the web, never in a model's training —
/// and the person's own currency matters (a tester's "Can you check the Grok
/// subscriptions please, for me?" got two notes searches and no answer,
/// 2026-10-21; "…I am thinking of buying it" US prices, 2026-10-22). Pure.
pub fn asks_about_prices(question: &str) -> bool {
    static WORDS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(?:prices?|pricing|priced|costs?|costing|subscriptions?|subscribe|memberships?|fees|cheapest|cheaper|deals|best deal|good deal|discounts?|on offer|on sale|in stock|out of stock|sold out|in the sales|tariffs?|how much money|buy|buying|purchase|purchasing|worth the money)\b",
        )
        .expect("static regex")
    });
    // "How much is a PS5?", "how much does it cost?" — not "how much is 15% of 80?".
    static HOW_MUCH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\bhow\s+much\s+(?:is|are|was|were|does|would|will|for|to)\b").expect("static regex"));
    static MATHS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\bhow\s+much\s+(?:is|are)\s+[\d(.,£$€]+\s*(?:%|percent|\+|-|\*|x|times|plus|minus|divided|of)").expect("static regex"));
    WORDS.is_match(question) || (HOW_MUCH.is_match(question) && !MATHS.is_match(question))
}

/// Phrases that ask for the web.
const ASKS_FOR_WEB: &[&str] = &[
    "look up", "look it up", "look that up", "look this up", "search the web", "search online", "search for",
    "search the internet", "google", "on the web", "on the internet", "online", "browse", "check the web",
    "latest", "news", "today's", "this week's", "right now", "currently", "current price", "price of",
    "release date", "patch notes", "walkthrough", "where do i find", "where can i find", "how do i get to",
    "where is the", "where's the",
    // Something new or still to come (a tester's "best RPG games coming out in 2026", 2026-10-21).
    "coming out", "come out", "comes out", "upcoming", "out yet", "new releases", "just released", "this year's",
    "next year's", "who won", "who is winning", "who's winning", "weather", "forecast", "exchange rate", "stock price",
];

/// Should Yap search the web for this question? Conservative: the person
/// asks for it ("look up", "search the web", "google"), wants something
/// current ("latest", "news", "price of", "coming out", a year from last
/// year on: "best indie games 2026"), or asks about a game Yap knows a wiki
/// for — never a question about their own notes or meetings, a request to
/// do something ("write…", "add…", "open…"), nor one about how an earlier
/// answer was made ("did you look that up?", [`about_the_answer`]). The
/// fixed pipeline's test for a model that can't call tools, and when a model
/// on this PC is handed a search before it answers. Pure.
pub fn needs_web(question: &str) -> bool {
    needs_web_in(question, chrono::Datelike::year(&chrono::Local::now()))
}

/// [`needs_web`], in year `this_year`. Pure.
pub fn needs_web_in(question: &str, this_year: i32) -> bool {
    // Asked to do something (a search would switch the actions off for the
    // turn, tools.rs' safety rule), or about a note.
    static ACTS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^\W*(?:(?:hey|hi|ok|okay)\s+\w+[,!]?\s+)?(?:(?:can|could|would|will)\s+you\s+)?(?:please\s+)?(?:create|make|write|draft|save|add|edit|update|change|rename|move|delete|remove|open|launch|start|run|play|copy|remind|summari[sz]e|translate|rewrite|tidy|fix|tick|compose|reply)\b",
        )
        .expect("static regex")
    });
    static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(20\d\d)\b").expect("static regex"));
    let lower = format!(" {} ", question.to_lowercase().replace('’', "'"));
    if about_own_stuff(question) || about_the_answer(question) || ACTS.is_match(question) {
        return false;
    }
    if game_for(question).is_some() || asks_about_prices(question) {
        return true;
    }
    // A year from last year on: what a model's training can't know yet.
    let recent = YEAR.captures_iter(question).any(|c| c[1].parse::<i32>().is_ok_and(|y| y >= this_year - 1));
    recent || ASKS_FOR_WEB.iter().any(|p| has_phrase(&lower, p))
}

/// Does the question ask how an earlier answer was made — "Did you look
/// that up or did you just guess?", "Where did you get that?", "Are you
/// sure?" — rather than ask something new? Then it's about the answer
/// before it: no search for it, and a claim of a search is checked against
/// that answer ([`claims_browsing`]). Pure.
pub fn about_the_answer(question: &str) -> bool {
    static ABOUT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(?:did|have|had|were)\s+you\s+(?:actually\s+|really\s+|just\s+|even\s+)?(?:look|search|google|brows|check|guess|make\s+(?:that|this|it|them)\s+up|invent|verify|use\s+the\s+(?:web|internet))|\bwhere\s+did\s+you\s+(?:get|find|read|see|learn)\b|\bwhere\s+do\s+you\s+get\s+(?:that|this|it|those|these|your)\b|\bwhat(?:'s|’s|\s+is|\s+are|\s+were|\s+was)\s+(?:your|the|its|their)\s+sources?\b|\bhow\s+do\s+you\s+know\b|\bare\s+you\s+sure\b|\bis\s+(?:that|this|it)\s+(?:right|true|correct|accurate|real)\b|\bjust\s+guess(?:ing)?\b",
        )
        .expect("static regex")
    });
    ABOUT.is_match(question)
}

/// Does the question point back at an earlier answer ("the top five in that
/// list", "which of those…", "the second one")? A follow-up like that may
/// cite the earlier answer's sources without searching again. Pure.
pub fn refers_back(question: &str) -> bool {
    static BACK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(?:that|this|those|these|the\s+same|the\s+above|your)\s+(?:list|lists|one|ones|game|games|answer|answers|result|results|article|articles|source|sources|link|links|page|pages|item|items|option|options|picks?|suggestions?|recommendations?)\b|\b(?:those|these|them)\b|\bthe\s+(?:first|second|third|fourth|fifth|last)\s+(?:one|ones|game|games|item|items|pick|picks|option|options|two|three|four|five)\b|\bthe\s+top\s+(?:one|two|three|four|five|six|seven|eight|nine|ten|\d+)\b|\b(?:above|you\s+(?:said|mentioned|listed|just\s+said))\b",
        )
        .expect("static regex")
    });
    BACK.is_match(question)
}

/// The search for a question: without "can you look up", "search the web
/// for", "please", "for me"… ("Can you look up the latest Rust release?" →
/// "the latest Rust release"), sentence by sentence, a sentence of nothing
/// else left out ("Can you look up please, for me? Can you look up Groc
/// subscriptions for me, please?" → "Groc subscriptions": a tester's, dictated,
/// 2026-10-21), "tell me" too ("Can you tell me the weather today in
/// Kellbrook?" → "the weather today in Kellbrook": Yap's own search when the
/// model wouldn't make one, a tester's voice chat, 2026-10-23). Pure.
pub fn search_query(question: &str) -> String {
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^\s*(?:hey\s+\w+[,!]?\s+)?(?:(?:can|could|would|will)\s+you\s+(?:please\s+)?|please\s+)?(?:(?:quickly\s+)?(?:look\s+up|look\s+(?:it|this|that)\s+up|search\s+(?:the\s+(?:web|internet)\s+|online\s+)?(?:for\s+)?|web\s+search|google|find\s+out|check|tell\s+me(?:\s+about)?|let\s+me\s+know(?:\s+about)?)\s*(?:online\s+|on\s+the\s+web\s+)?(?:for\s+)?)",
        )
        .expect("static regex")
    });
    static FILLER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i),?\s*\b(?:please|pls|for me|for us|thanks|thank you|if you can|if possible|real quick)\b,?").expect("static regex")
    });
    let parts: Vec<String> = question
        .split_inclusive(['?', '!'])
        .map(|s| {
            let q = ASK.replace(s.trim(), "");
            let q = FILLER.replace_all(&q, " ");
            let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
            q.trim().trim_end_matches(['?', '.', '!', ',']).trim().to_string()
        })
        .filter(|q| q.chars().any(char::is_alphanumeric))
        .collect();
    let q = parts.join(" ");
    let q = if q.trim().is_empty() { question.trim().to_string() } else { q };
    tidy(&q, 200)
}

/// **A price question's search, aimed at the person's country**: "Grok
/// subscription price 2026" → "Grok subscription price 2026 UK". Exa's
/// `userLocation` (GB) alone found x.ai's US page, a US price list and a
/// guide in $ for a tester's "Can you lookup the Grok subscription price for
/// me I am thinking of buying it" (2026-10-22). `None` — the query as it
/// is — when the question isn't about prices or buying
/// ([`asks_about_prices`]), the person is in the US or Yap doesn't know
/// where, or the query already says where or in what money ("… US price",
/// "… in GBP"). Every search of the turn, the model's and Yap's. Pure.
pub fn local_query(query: &str, question: &str, local: Option<&crate::ai::about_you::LocalPrices>) -> Option<String> {
    let local = local?;
    let q = query.trim();
    if q.is_empty() || !asks_about_prices(question) || local.query_names_a_place(q) {
        return None;
    }
    Some(format!("{q} {}", local.place))
}

/// The second search for a price where the person is, when the first found
/// only other countries' prices: the query with their money's sign ("…
/// UK £"; the code for a sign that's a word: "… Sweden SEK"). Pure.
pub fn local_retry_query(query: &str, local: &crate::ai::about_you::LocalPrices) -> String {
    let mentions = |q: &str, w: &str| {
        let (q, w) = (q.to_lowercase(), w.to_lowercase());
        if w.contains(' ') || !w.chars().all(char::is_alphanumeric) {
            q.contains(&w)
        } else {
            q.split(|c: char| !c.is_alphanumeric()).any(|x| x == w)
        }
    };
    let mut q = query.trim().to_string();
    for w in [local.place.as_str(), local.search_sign()] {
        if !mentions(&q, w) {
            q.push(' ');
            q.push_str(w);
        }
    }
    q
}

/// The web tools' line on prices, for a tool-calling model with the web on,
/// someone outside the US: search for their country's price, once more when
/// the results only have another country's, then say plainly that there
/// are none — never an offer to look further. Pure.
pub fn price_note(local: &crate::ai::about_you::LocalPrices) -> String {
    let p = &local.place;
    format!(
        "For a price, a subscription or something to buy, search for the {p} price (\"{p}\" in the query). If the results only give another country's prices, search once more for the {p} price; if there's still none, start your answer with one short line saying you couldn't find {p} prices, then give the others as they are, saying which country they're for. Never offer to look further: search instead."
    )
}

/// A forced first search's query without the words the model carried over
/// from earlier in the conversation: Qwen3.5-4B searched "Grok subscription
/// management check account billing" for "Can you look up Groc
/// subscriptions for me, please?" — every word after "subscription" from
/// the turn before ("check", its search, its answer about billing), and the
/// answer was about checking a bill (a tester's chat, 2026-10-21; told in the
/// tool's description to leave them out, it still did 1 time in 3). A word
/// stays when the question has it (or a near-miss of it: "Groc" → the
/// model's "Grok"), when nothing earlier had it (the model's own "price",
/// "UK", "match", a year), or when it's a number. Nothing changes when the
/// question points back at earlier messages ("it", "that one", "those") or
/// shares fewer than two words with the query (too little to stand alone:
/// "and in the UK?"), nor when too little would be left.
///
/// **A word the person said before, misheard now, stays**: "Can you tell me
/// the weather today in Kellbrook?", then "Look up the weather kelp for me
/// today." — the speech engine heard the town as "kelp", the model searched
/// "weather Kellbrook Fellshire today", and this cut it to "weather
/// Fellshire today": county-wide storm news for a question about one town
/// (a tester's voice chat, 2026-10-23; a fictional town and county here).
/// So an earlier word stays when the person's **own** earlier message
/// (`own`: theirs, not the model's answers or searches; its asking — "can
/// you check" — left out, [`search_query`]) has it, that message is about
/// this one's subject too (a word in common), and this question has a word
/// the query left out that sounds like it, or like its start
/// (`fuzzy::sounds_like`: the model wrote the misheard word as the person
/// had said it). Pure.
pub fn fresh_query(query: &str, question: &str, earlier: &[&str], own: &[&str]) -> String {
    fn words(s: &str) -> Vec<String> {
        s.to_lowercase()
            .replace('’', "'")
            .split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .map(|w| w.trim_matches('\'').trim_end_matches("'s").to_string())
            .filter(|w| !w.is_empty())
            .collect()
    }
    fn same(a: &str, b: &str) -> bool {
        let stem = |w: &str| if w.chars().count() > 3 { w.trim_end_matches('s').to_string() } else { w.to_string() };
        a == b
            || stem(a) == stem(b)
            || (a.chars().count() >= 4 && b.chars().count() >= 4 && crate::dictionary::fuzzy::plain_spelling_distance(a, b) <= 0.25)
    }
    static BACK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:it|its|that|this|they|them|those|these|one|ones|he|she|him|her|his|their|there)\b").expect("static regex")
    });
    if earlier.is_empty() || refers_back(question) || BACK.is_match(question) {
        return query.to_string();
    }
    let asked = words(question);
    let content: Vec<&String> = asked.iter().filter(|w| w.chars().count() >= 3 && !QUERY_STOP.contains(&w.as_str())).collect();
    let q = words(query);
    let shared = content.iter().filter(|a| q.iter().any(|w| same(w, a))).count();
    if shared < 2 {
        return query.to_string();
    }
    let before: Vec<String> = earlier.iter().flat_map(|t| words(t)).collect();
    // The question's words the query left out ("kelp"), one by one and two
    // side by side ("kel brook").
    let missing = |w: &String| w.chars().count() >= 3 && !QUERY_STOP.contains(&w.as_str()) && !q.iter().any(|x| same(x, w));
    let mut left_out: Vec<String> = Vec::new();
    for (i, w) in asked.iter().enumerate() {
        if missing(w) {
            left_out.push(w.clone());
            if let Some(next) = asked.get(i + 1).filter(|n| missing(n)) {
                left_out.push(format!("{w}{next}"));
            }
        }
    }
    // The person's own words for what they asked before, in a message about
    // this question's subject too.
    let theirs: Vec<String> = if left_out.is_empty() {
        Vec::new()
    } else {
        own.iter()
            .map(|t| words(&search_query(t)))
            .filter(|said| content.iter().any(|c| said.iter().any(|w| same(w, c))))
            .flatten()
            .collect()
    };
    let misheard = |w: &str| theirs.iter().any(|t| same(w, t)) && left_out.iter().any(|h| crate::dictionary::fuzzy::sounds_like(h, w));
    let kept: Vec<&str> = query
        .split_whitespace()
        .filter(|raw| {
            let w = words(raw);
            let Some(w) = w.first() else { return true };
            w.chars().all(|c| c.is_ascii_digit())
                || asked.iter().any(|a| same(w, a))
                || !before.iter().any(|b| same(w, b))
                || misheard(w)
        })
        .collect();
    if kept.len() < 2 || kept.len() == query.split_whitespace().count() {
        return query.to_string();
    }
    kept.join(" ")
}

/// The fixed pipeline's sources and their passages for the prompt: ≤ 3
/// sources, ≤ `budget` tokens in all. Pure.
pub fn grounded_sources(hits: &[Hit], budget: usize) -> (Vec<Source>, String) {
    let mut sources = Vec::new();
    let mut block = String::new();
    let hits: Vec<&Hit> = hits.iter().filter(|h| !h.text.trim().is_empty()).take(3).collect();
    let per = budget / hits.len().clamp(1, 3);
    for (i, h) in hits.into_iter().enumerate() {
        let n = i + 1;
        let text = match page_part(&h.text, 0, per.saturating_sub(40)) {
            (part, Some(_)) => format!("{part}…"),
            (all, None) => all,
        };
        let date = h.published.as_deref().map(|d| format!(" ({d})")).unwrap_or_default();
        block.push_str(&format!("[{n}] {} — {}{date}\n{}\n\n", h.title, site_of(&h.url), text.trim()));
        sources.push(Source { n, title: tidy(&h.title, 120), url: h.url.clone(), site: site_of(&h.url), ..Default::default() });
    }
    (sources, block.trim_end().to_string())
}

/// The system prompt for a model that can't call tools, answering from web
/// sources Yap found: the persona, the rules, the numbered passages. Pure.
pub fn grounded_prompt(persona: &str, block: &str, today: &str, spoken: bool) -> String {
    let lead = if spoken {
        " This question was asked out loud: start with the answer itself in one or two short sentences."
    } else {
        ""
    };
    format!(
        "{}\n\nToday is {today}. Yap searched the web for the user's question and found the numbered sources below. Answer the question using ONLY these sources, in under 80 words. Put the source's number in square brackets after each fact you take from it, like [1]. If the sources don't answer the question, say you couldn't find it on the web. Never say you browsed or searched anything else. {DATA_NOT_INSTRUCTIONS}{lead}\n\n<sources>\n{block}\n</sources>",
        persona.trim()
    )
}

/// The line for a model that can't call tools when no search ran: it must
/// never claim it looked anything up. `why`: the search failed (or found
/// nothing), said so.
pub fn no_search_line(why: Option<&str>) -> String {
    match why {
        Some(why) => format!(
            "Yap tried to search the web for this question but couldn't ({why}). Tell the user that briefly. Never claim you searched, browsed or found anything online."
        ),
        None => "You can't search or browse the web in this chat: never say you searched, browsed or looked anything up online.".to_string(),
    }
}

/// Does an answer claim it looked something up on the web? (A small model
/// sometimes says so when nothing was searched: "I did look it up on the
/// web", Qwen3.5-4B, 2026-10-21.) A sentence that denies it ("I didn't
/// search", "I can't browse") isn't a claim. Pure.
pub fn claims_browsing(reply: &str) -> bool {
    static CLAIM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(?:i\s+(?:(?:just|quickly|actually|also|then|really|indeed)\s+)*(?:searched|googled|browsed|looked\s+(?:it|this|that|them|those|these)?\s*up|checked\s+(?:online|the\s+web|the\s+internet))|i\s+(?:(?:actually|really|indeed)\s+)?(?:did|have|had)\s+(?:(?:just|actually|also|in\s+fact|really)\s+)?(?:search|google|browse|look\s+(?:it|this|that|them|those|these)?\s*up|check\s+(?:online|the\s+web|the\s+internet))|i\s+(?:(?:just|also|then|actually)\s+)?used\s+(?:the\s+)?`?web_search|`?web_search`?\s+tool|(?:according\s+to|based\s+on)\s+(?:my|a)\s+(?:web\s+|online\s+|quick\s+)?search|my\s+(?:web\s+|online\s+)search|from\s+(?:a|my|the)\s+(?:quick\s+)?(?:web|online|internet|google)\s+search|(?:a|my)\s+quick\s+(?:web\s+|online\s+)?search|results\s+i\s+(?:fetched|found|got|pulled)|i\s+(?:just\s+)?found\s+(?:it|this|that|them)\s+online)",
        )
        .expect("static regex")
    });
    // Denials, and offers ("I can do a quick web search if you like").
    static DENIAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)n't\b|\bnot\b|\bnever\b|\bcannot\b|\bwithout\b|\bi\s+(?:can|could|will)\b|\bi'll\b|\blet\s+me\b|\bwant\s+me\b|\bwould\s+you\b|\bif\s+you\b|\bshall\s+i\b")
            .expect("static regex")
    });
    // "I searched your notes": not the web (unless it says so).
    static OWN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:notes?|meetings?|transcripts?|library|folders?|calendar|files?)\b").expect("static regex")
    });
    static WEB: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(?:web|online|internet|google)\b").expect("static regex"));
    reply.replace(['\u{2019}', '\u{2018}'], "'").split_inclusive(['.', '!', '?', '\n']).any(|s| {
        CLAIM.is_match(s) && !DENIAL.is_match(s) && !s.trim_end().ends_with('?') && (!OWN.is_match(s) || WEB.is_match(s))
    })
}

/// The line for a model that can't call tools when web search is on but the
/// question didn't need it: no search ran for this one.
pub const NOT_SEARCHED_LINE: &str = "Yap didn't search the web for this question: answer from your own knowledge, and never say you searched, browsed or looked anything up online.";

/// Appended to an answer that claims a search that didn't happen.
pub const NOT_SEARCHED_NOTE: &str =
    "*Yap didn't search the web for this answer: it comes from the AI model's own knowledge, which may be out of date.*";

/// Appended when an answer to "did you look that up?" claims a search the
/// answer it's about didn't have ([`about_the_answer`]).
pub const NOT_SEARCHED_THAT_NOTE: &str =
    "*Yap didn't search the web for that answer: it came from the AI model's own knowledge, which may be out of date.*";

/// Does the question ask for sources, references or reading — a list of
/// them is then the answer itself ("What sources could I use for my
/// essay?", "any further reading?")? Not when it asks the answer to show
/// where it came from ("Include your sources", "with sources"): that's
/// what Yap's Sources row under a web answer is for (`web-no-sources-list`).
/// Pure.
pub fn asks_for_sources(question: &str) -> bool {
    static ASKS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:sources?|references?|citations?|cite|cited|citing|bibliograph\w*|reading|to\s+read|read\s+(?:more|up)|works\s+cited)\b")
            .expect("static regex")
    });
    static CITE_YOURS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:your|its|with|include|including|add|cite)\s+(?:your\s+|the\s+|its\s+)?(?:sources?|references?|citations?)\b|\bwhere\s+(?:you|that|it)\s+(?:got|found|get|comes?)\b")
            .expect("static regex")
    });
    ASKS.is_match(question) && !CITE_YOURS.is_match(question)
}

/// An answer without what a model may write in Yap's own voice: the "(Yap
/// searched the web for this answer. Its sources: …)" note (Qwen3.5-4B
/// copied it from the history, 2026-10-21, with sources it made up) and a
/// list of sources at its end ("Sources:" then "[1] …" lines) — Yap shows an
/// answer's real sources under it. The list stays when the `question` asks
/// for sources, references or reading ([`asks_for_sources`]: it's the
/// answer), and the looser kinds of item — a bullet ending in its number, a
/// site and a quoted title, any bullets to the end of the answer — count
/// only when the turn `looked` on the web (a list of books or reports under
/// "References" from the model's own knowledge is what was asked for, not
/// Yap's sources). `None` when there was nothing to take out. Pure.
pub fn strip_sources_note(reply: &str, looked: bool, question: &str) -> Option<String> {
    // "(Yap searched the web … (espn.com) …)", nested brackets and all; or the
    // same words outside brackets, to the end of the line.
    static NOTE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?im)[ \t]*\(\s*yap\s+searched\s+the\s+web(?:[^()]|\([^()]*\))*\)\.?|[ \t]*\byap\s+searched\s+the\s+web\b.*$")
            .expect("static regex")
    });
    // A heading or line naming the sources ("Sources:", "**References**"),
    // perhaps with the first of them on it.
    static HEAD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(?:#{1,4}\s*)?(?:\*\*|__|\*)?\s*(?:sources?|references?|citations?)(?:\s+used)?\s*(?:\*\*|__|\*)?\s*:?\s*(?:\*\*|__|\*)?\s*(.*)$")
            .expect("static regex")
    });
    // One of them: "[1] Title", "1. Title — site", "- [2] …", "- Title (site.com)".
    static ITEM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(?:[-*•]\s*)?(?:\[\d{1,2}\]|\d{1,2}[.)])\s*\S|^\s*[-*•]\s+.*(?:https?://|\b[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}\b)")
            .expect("static regex")
    });
    // After a web search, also: "- IndieGames: "The best…" [1]" (a bullet
    // ending in its number, or a site and a quoted title: Qwen3.5-4B's,
    // web-no-sources-list, 2026-10-22).
    static WEB_ITEM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)^\s*[-*•]\s+.*\[\d{1,2}\][\s.,;:)]*$|^\s*[-*•]\s+[^"“\n]{1,60}?[:—–-]\s*["“].+["”]"#)
            .expect("static regex")
    });
    // Any bullet or numbered line: after a web search, under a sources
    // heading with nothing but such lines to the end of the answer, they're
    // its sources.
    static BULLET: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[-*•]|\d{1,2}[.)])\s+\S").expect("static regex"));
    let noted = NOTE.replace_all(reply, "").into_owned();
    if asks_for_sources(question) {
        let out = noted.trim_end().to_string();
        return (out != reply.trim_end()).then_some(out);
    }
    let item = |l: &str| ITEM.is_match(l) || (looked && WEB_ITEM.is_match(l));
    // A list of sources: its heading and the sources under it (blank lines
    // between them allowed) go; what follows ("Would you like more…?") stays.
    let lines: Vec<&str> = noted.lines().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let inline = HEAD.captures(lines[i]).and_then(|c| c.get(1)).map(|m| m.as_str().trim());
        let Some(inline) = inline.filter(|t| t.is_empty() || t.starts_with('[') || item(t) || t.contains("http")) else {
            kept.push(lines[i]);
            i += 1;
            continue;
        };
        // The sources under the heading.
        let (mut end, mut j) = (i + 1, i + 1);
        while j < lines.len() && (lines[j].trim().is_empty() || item(lines[j])) {
            if !lines[j].trim().is_empty() {
                end = j + 1;
            }
            j += 1;
        }
        // Or a plain list to the very end ("- IndieGames — The best indie
        // games of 2026"): nothing after a sources heading but its items.
        let rest: Vec<&str> = lines[i + 1..].iter().copied().filter(|l| !l.trim().is_empty()).collect();
        if looked && !rest.is_empty() && rest.iter().all(|l| BULLET.is_match(l)) {
            end = lines.len();
        }
        if end == i + 1 && inline.is_empty() {
            kept.push(lines[i]); // a heading with nothing under it: not a list
            i += 1;
            continue;
        }
        // The blank line before the list goes with it.
        while kept.last().is_some_and(|l| l.trim().is_empty()) {
            kept.pop();
        }
        i = end;
    }
    let out = kept.join("\n").trim_end().to_string();
    (out != reply.trim_end()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::tests::uk_prices;

    #[test]
    fn small_models_search_only_when_the_question_needs_the_web() {
        for yes in [
            "Where do I find Gale in Baldur's Gate 3?",
            "Can you look up the latest Rust release?",
            "search the web for cheap flights to Lisbon",
            "what's the latest news about the Mars mission",
            "google the opening hours of the Louvre",
            "How do I beat Malenia in Elden Ring",
            "what's the price of a Steam Deck right now",
        ] {
            assert!(needs_web(yes), "{yes}");
        }
        for no in [
            "What did we decide in my meeting with Alice?",
            "Summarise my notes about the launch",
            "Write a haiku about autumn",
            "What's 15% of 80?",
            "Explain recursion simply",
            "Where's the latest version of my notes about the budget?",
            "hello",
        ] {
            assert!(!needs_web(no), "{no}");
        }
        // Something new or still to come (a tester's chat, 2026-10-21), a
        // year from last year on.
        for yes in [
            "What are some of the best RPG games coming out in 2026",
            "best indie games 2026",
            "Any upcoming Zelda games?",
            "Who won the match last night?",
            "Is Hades 3 out yet?",
            "What were the best films of 2025?",
        ] {
            assert!(needs_web_in(yes, 2026), "{yes}");
        }
        for no in [
            // How an earlier answer was made: about it, not a new search.
            "Did you look up that information or did you just guess?",
            "Did you look that up?",
            "Where did you get that from?",
            // Asked to do something, or about a note.
            "Write a haiku about 2026",
            "Add my 2026 goals to a note",
            "Open the latest Spotify playlist",
            "Play the latest Taylor Swift song",
            "In 2026 I want to learn piano, put that in my notes",
            // An old year.
            "What's a classic RPG from 1997?",
            "Which classic RPG from the 1990s would you recommend to me?",
            "Can you tell me the top five in that list?",
        ] {
            assert!(!needs_web_in(no, 2026), "{no}");
        }
        assert!(needs_web_in("What are the latest patch notes for Baldur's Gate 3?", 2026));
        // Prices, plans, deals and stock (a tester's Grok chat, 2026-10-21).
        for yes in [
            "Can you check the Grok subscriptions please, for me?",
            "Can you web search Grok's subscription and how much it costs for me?",
            "How much is a Nintendo Switch 2 in the UK?",
            "How much does Spotify Premium cost a month?",
            "What's the cheapest flight to Lisbon?",
            "Are there any deals on the Steam Deck?",
            "Is the PS5 Pro in stock anywhere?",
            "What's the price of a Netflix subscription?",
            "how much is £50 in dollars",
        ] {
            assert!(needs_web_in(yes, 2026), "{yes}");
        }
        for no in [
            // About their own notes, meetings and lists.
            "What did I note about the Grok subscription?",
            "In my notes, how much did the plumber cost?",
            "What did we decide about the price in the meeting?",
            "What did I write about subscriptions?",
            "How much is left in my budget?",
            // Sums, not prices.
            "How much is 15% of 80?",
            "how much is 3 times 7",
            "How much water should I drink a day?",
        ] {
            assert!(!needs_web_in(no, 2026), "{no}");
        }
        assert!(about_own_stuff("what did I note about the trip?") && about_own_stuff("Anything in my notes about Grok?"));
        assert!(!about_own_stuff("Can you check the Grok subscriptions please, for me?"));
        assert!(!about_own_stuff("What are the latest patch notes for Hades 2?"), "patch notes are the web's");
        assert_eq!(search_query("Can you look up the latest Rust release?"), "the latest Rust release");
        assert_eq!(search_query("search the web for cheap flights to Lisbon please"), "cheap flights to Lisbon");
        assert_eq!(search_query("Google the opening hours of the Louvre."), "the opening hours of the Louvre");
        assert_eq!(search_query("Where do I find Gale in Baldur's Gate 3?"), "Where do I find Gale in Baldur's Gate 3");
        // A tester's, dictated (2026-10-21): the asking and the please left out.
        assert_eq!(search_query("Can you check the Grok subscriptions please, for me?"), "the Grok subscriptions");
        assert_eq!(search_query("Can you look up please, for me? Can you look up Groc subscriptions for me, please?"), "Groc subscriptions");
        assert_eq!(
            search_query("Can you web search Grok's subscription and how much it costs for me?"),
            "Grok's subscription and how much it costs"
        );
        // A tester's voice question (2026-10-23; a fictional town): "tell me" is
        // asking too.
        assert_eq!(search_query("Can you tell me the weather today in Kellbrook?"), "the weather today in Kellbrook");
        assert_eq!(search_query("Tell me about the Starfall Requiem patch notes."), "the Starfall Requiem patch notes");
        assert_eq!(search_query("Could you let me know the exchange rate for euros?"), "the exchange rate for euros");
        // Nothing else in it: the question as it is.
        assert_eq!(search_query("Please?"), "Please?");
    }

    #[test]
    fn a_price_question_searches_where_the_user_is() {
        let gb = uk_prices();
        let grok = "Can you lookup the Grok subscription price for me I am thinking of buying it";
        // A tester's (2026-10-22): the model's own query, aimed at the UK.
        assert_eq!(local_query("Grok subscription price 2026", grok, Some(&gb)).as_deref(), Some("Grok subscription price 2026 UK"));
        // Buying counts; a query that already says where or in what money
        // stays; not a price question, the US, or nowhere known: nothing.
        assert!(asks_about_prices("I'm thinking of buying an iPhone 17 Pro, what does it go for?"));
        assert!(needs_web_in("Should I buy the Xbox Series X?", 2026));
        assert!(!needs_web_in("What did I buy last week?", 2026), "their own notes");
        assert_eq!(local_query("Grok price UK", grok, Some(&gb)), None);
        assert_eq!(local_query("Grok US price", grok, Some(&gb)), None);
        assert_eq!(local_query("Grok price GBP", grok, Some(&gb)), None);
        assert_eq!(local_query("Gale location", "Where do I find Gale?", Some(&gb)), None);
        assert_eq!(local_query("Grok subscription price", grok, None), None);
        // Once more for their money: the place and the sign.
        assert_eq!(local_retry_query("Grok subscription price 2026 UK", &gb), "Grok subscription price 2026 UK £");
        assert_eq!(local_retry_query("Spotify Premium price", &gb), "Spotify Premium price UK £");
        assert_eq!(local_retry_query("Netflix price UK £", &gb), "Netflix price UK £");
        // The tool models' line.
        let note = price_note(&gb);
        assert!(note.starts_with("For a price, a subscription or something to buy, search for the UK price (\"UK\" in the query)."), "{note}");
        assert!(note.contains("couldn't find UK prices") && note.ends_with("Never offer to look further: search instead."), "{note}");
    }

    #[test]
    fn a_forced_search_leaves_out_words_from_earlier_turns() {
        // A tester's chat (2026-10-21): turn 1 searched "check" and answered
        // about billing; turn 2's own search carried them over.
        let earlier = [
            "Can you check the Grok subscriptions please, for me?",
            r#"{"query":"Grok subscription management X platform account settings billing"}"#,
            "To check your Grok subscription, go to grok.com/dashboard/billing while logged in to your account.",
        ];
        // The person's own message: its "check" is how they asked, not what.
        let own = [earlier[0]];
        let q2 = "Can you look up please, for me? Can you look up Groc subscriptions for me, please?";
        assert_eq!(fresh_query("Grok subscription management check account billing", q2, &earlier, &own), "Grok subscription");
        // The model's own words that nothing earlier had stay, and numbers.
        assert_eq!(fresh_query("Grok subscriptions price UK 2026 check", q2, &earlier, &own), "Grok subscriptions price UK 2026");
        // A filler the query leaves out ("Hmm") changes nothing of that.
        let hmm = "Hmm, can you look up Groc subscriptions for me?";
        assert_eq!(fresh_query("Grok subscription management check account billing", hmm, &earlier, &own), "Grok subscription");
        // Nothing earlier: as written ("Arsenal… tonight 21 October 2026 time").
        let arsenal = "Arsenal Football Club match tonight 21 October 2026 time";
        assert_eq!(fresh_query(arsenal, "When are Arsenal playing tonight?", &[], &[]), arsenal);
        // A follow-up pointing back, or one too short to stand alone: as written.
        let rpg = ["What are the best RPG games coming out in 2026?", "1. Starfall Requiem (12 March)"];
        let rpg_own = [rpg[0]];
        assert_eq!(fresh_query("Starfall Requiem price", "How much is the first one?", &rpg, &rpg_own), "Starfall Requiem price");
        assert_eq!(fresh_query("Starfall Requiem price UK", "And how much is it in the UK?", &rpg, &rpg_own), "Starfall Requiem price UK");
        assert_eq!(fresh_query("Grok subscription price UK", "and in pounds?", &earlier, &own), "Grok subscription price UK");
        // Nothing to leave out.
        assert_eq!(fresh_query("Grok subscriptions", q2, &earlier, &own), "Grok subscriptions");
    }

    // A tester's voice chat (2026-10-23; the town here is fictional): the
    // Voice Assistant heard the town the second time as "kelp", the model
    // searched for the town as first said, and the guard took it out —
    // "weather Fellshire today", county-wide storm news for one town.
    #[test]
    fn a_misheard_word_the_person_said_before_stays_in_the_search() {
        let q1 = "Can you tell me the weather today in Kellbrook? I might walk over the hills this afternoon.";
        let q2 = "Hmm. Look up the weather kelp for me today.";
        let model_query = "weather Kellbrook Fellshire today";
        // The first question was still being answered: only it came before.
        assert_eq!(fresh_query(model_query, q2, &[q1], &[q1]), model_query);
        // …and the same once it had its answer (and the search behind it).
        let answered = [q1, r#"{"query":"weather Kellbrook today"}"#, "Kellbrook is mild today with sunny spells [1]."];
        assert_eq!(fresh_query(model_query, q2, &answered, &[q1]), model_query);
        // Heard as two words ("kel brook"): the same.
        let q2b = "Look up the weather in kel brook for me today.";
        assert_eq!(fresh_query("weather Kellbrook today", q2b, &[q1], &[q1]), "weather Kellbrook today");

        // Only the person's own words: a name from the model's answer that
        // the question doesn't have is still left out…
        let q0 = "Can you tell me the weather today on the coast?";
        let from_model = [q0, "Along the coast near Kellbrook it is mild today."];
        assert_eq!(fresh_query(model_query, q2, &from_model, &[q0]), "weather Fellshire today");
        // …only for a word the question has misheard (nothing like
        // "cycling" sounds like the town: it goes)…
        let other_town = "Can you look up the weather today in Ashford Vale for cycling?";
        assert_eq!(
            fresh_query("weather today Ashford Vale Kellbrook", other_town, &[q1], &[q1]),
            "weather today Ashford Vale"
        );
        // …and only from a message about this subject too.
        let unrelated = "Is there a farm shop in Kellbrook?";
        let q3 = "Look up the forecast kelp for tomorrow.";
        assert_eq!(fresh_query("forecast Kellbrook tomorrow", q3, &[unrelated], &[unrelated]), "forecast tomorrow");
        // A filler the query leaves out is no misheard word: "okay" isn't
        // the start of the "account" the person said before.
        let billing = "Can you check the Grok subscriptions on my account?";
        assert_eq!(
            fresh_query("Grok subscription account", "Okay, look up Groc subscriptions plans.", &[billing], &[billing]),
            "Grok subscription"
        );
    }

    #[test]
    fn questions_about_an_answer_and_ones_pointing_back_are_told_apart() {
        for about in [
            "Did you look up that information or did you just guess?",
            "did you actually search for that",
            "Where did you get that?",
            "Where do you get that from?",
            "What are your sources?",
            "How do you know?",
            "Are you sure?",
            "Is that right?",
            "Did you make that up?",
        ] {
            assert!(about_the_answer(about), "{about}");
        }
        // "Where do you find Gale…?" asks where one finds him (the Gale
        // scenario of the model evaluations, 2026-10-21).
        for not in [
            "What are some RPGs coming out in 2026",
            "Did they win?",
            "Where do I find Gale?",
            "Where do you find Gale in Baldur's Gate 3 act 1?",
            "Make up a story",
        ] {
            assert!(!about_the_answer(not), "{not}");
        }
        for back in [
            "Can you tell me the top five in that list?",
            "Which of those would suit someone who likes cozy games?",
            "Tell me more about the second one",
            "What did you say about the first game?",
            "Is the last one out on Switch?",
        ] {
            assert!(refers_back(back), "{back}");
        }
        for not in ["What are some of the best RPG games coming out in 2026", "best indie games 2026", "When is the next Zelda?"] {
            assert!(!refers_back(not), "{not}");
        }
    }

    #[test]
    fn a_sources_note_or_list_the_model_wrote_is_taken_out() {
        // After a web search, for a question that doesn't ask for sources.
        let web = |reply: &str| strip_sources_note(reply, true, "What are the best indie games of 2026?");
        // Qwen3.5-4B's copy of the history note (a tester's chat, 2026-10-21).
        let made_up = "Here are some RPGs:\n\n1. **Black Myth: Wukong** [1]\n2. **Dragon's Dogma 2** [5]\n\nWould you like more details?\n\n(Yap searched the web for this answer. Its sources: [1] Black Myth: Wukong - Wikipedia; Starfield: Bound by Honor - Microsoft Store; [5] Dragon's Dogma 2 - IGN.)";
        assert_eq!(
            web(made_up).unwrap(),
            "Here are some RPGs:\n\n1. **Black Myth: Wukong** [1]\n2. **Dragon's Dogma 2** [5]\n\nWould you like more details?"
        );
        // Nested brackets inside the note, and the note on its own line.
        assert_eq!(web("8 pm [1]. (Yap searched the web for this answer. Its sources: [1] Bayern vs. Arsenal (espn.com).)").unwrap(), "8 pm [1].");
        assert_eq!(web("8 pm.\nYap searched the web for this answer.").unwrap(), "8 pm.");
        // A list of sources at the end, in its usual shapes.
        for list in [
            "Hollow Lantern leads [1].\n\nSources:\n[1] The best indie games of 2026 — indiegames.example\n[2] GOTY contenders — gamecritic.example",
            "Hollow Lantern leads [1].\n\n**Sources:**\n- [1] The best indie games of 2026\n- [2] GOTY contenders",
            "Hollow Lantern leads [1].\n\n### References\n1. The best indie games of 2026 (indiegames.example)",
            "Hollow Lantern leads [1].\n\nSources: [1] The best indie games of 2026; [2] GOTY contenders",
            "Hollow Lantern leads [1].\n\nSources:\n- The best indie games of 2026 (indiegames.example)",
        ] {
            assert_eq!(web(list).as_deref(), Some("Hollow Lantern leads [1]."), "{list}");
        }
        // Qwen3.5-4B's, with its closing question after it (2026-10-21 runs).
        assert_eq!(
            web("Top picks [1][2].\n\nSources:\n1. indiegames.example - \"The best indie games of 2026 so far\"\n\n2. gamecritic.example - \"Contenders\"\n\nWould you like more details?").as_deref(),
            Some("Top picks [1][2].\n\nWould you like more details?")
        );
        // Qwen3.5-4B's on llamafile 0.10.3 and 0.10.6 alike (web-no-sources-list,
        // the model evaluations of 2026-10-22): a site, a quoted title and the
        // number — the answer's own list above it (its lines end in [n] too)
        // stays.
        let answer = "Based on recent reviews and lists, the top indie games of 2026 include:\n\n1. **Hollow Lantern** – A hand-drawn metroidvania praised for its atmosphere and tight gameplay [1][2].\n2. **Pixel Tides** – A cozy fishing simulation that became a surprise hit of the summer [1][2][3].\n3. **Static Hearts** – A narrative mystery set in a 1990s radio station [1].\n\n**Hollow Lantern** currently leads in critical acclaim with a 94 average score, followed by Glasswing and Static Hearts [2].";
        let with_list = format!("{answer}\n\nSources:\n- Indie Games: \"The best indie games of 2026 so far\" [1]\n- GameCritic: \"Indie game of the year 2026: the contenders\" [2]\n- CozyGames: \"10 cozy indie games from 2026\" [3]");
        assert_eq!(web(&with_list).as_deref(), Some(answer));
        assert_eq!(web(answer), None, "a list of games is not a list of sources");
        // The same with a closing question after it, without the numbers,
        // with dashes, or a bare list to the end.
        for list in [
            "Sources:\n- Indie Games: \"The best indie games of 2026 so far\"\n- GameCritic: \"The contenders\"\n\nWant more?",
            "**Sources**\n- IndieGames — “The best indie games of 2026 so far”\n- GameCritic — “The contenders”\n\nWant more?",
            "Sources:\n- The best indie games of 2026 so far [1].\n- The contenders [2]\n\nWant more?",
        ] {
            assert_eq!(web(&format!("Hollow Lantern leads [1].\n\n{list}")).as_deref(), Some("Hollow Lantern leads [1].\n\nWant more?"), "{list}");
        }
        assert_eq!(
            web("Hollow Lantern leads [1].\n\nSources:\n- IndieGames, The best indie games of 2026 so far\n- GameCritic, The contenders").as_deref(),
            Some("Hollow Lantern leads [1].")
        );
        // Not a list of sources: left as it is.
        for keep in [
            "Good sources of protein: beans, lentils and tofu.",
            "Sources:\n\nThe page says Alt+2 opens scientific mode, and history keeps your sums.",
            "Hollow Lantern leads [1].",
            "Source: Wikipedia",
            "Sources of fibre:\n- oats\n- beans\n- lentils",
            "Sources:\n- The page says Alt+2 opens scientific mode.\n\nThat's all it says about the calculator, and history keeps your sums.",
        ] {
            assert_eq!(web(keep), None, "{keep}");
        }
    }

    /// A list the person asked for is the answer, not Yap's sources: kept
    /// whatever the turn did when the question asks for sources, references
    /// or reading; and without a web search only the numbered, linked kind
    /// counts as a list of sources (the review's essay example, 2026-10-22).
    #[test]
    fn a_list_of_sources_the_person_asked_for_stays() {
        let essay = "Here are some sources you could use for your essay:\n\n### Sources\n- IPCC Sixth Assessment Report (2021)\n- NASA climate website";
        let refs = "Good starting points:\n\n**References:**\n- Smith, The Climate Book (2020)\n- Jones, Warming World (2019)";
        let reading = "Try these:\n\nSources:\n1. The Climate Book — Greta Thunberg\n2. How to Avoid a Climate Disaster — Bill Gates";
        for asked in [
            "What sources could I use for my climate essay?",
            "Any references I should cite?",
            "Can you suggest some further reading on climate change?",
        ] {
            for (looked, reply) in [(false, essay), (true, essay), (false, refs), (true, refs), (false, reading), (true, reading)] {
                assert_eq!(strip_sources_note(reply, looked, asked), None, "{asked} / {looked}: {reply}");
            }
        }
        // Not asked for, and no web search: a plain list stays (it's the
        // model's own answer)…
        let help = "Help me plan my climate essay.";
        assert_eq!(strip_sources_note(essay, false, help), None);
        assert_eq!(strip_sources_note(refs, false, help), None);
        // …after one, it's the model's copy of what Yap shows under the answer.
        assert_eq!(strip_sources_note(essay, true, help).as_deref(), Some("Here are some sources you could use for your essay:"));
        assert_eq!(strip_sources_note(refs, true, help).as_deref(), Some("Good starting points:"));
        // A numbered list of sources at the end goes either way (as before),
        // unless it was asked for.
        assert_eq!(strip_sources_note(reading, false, help).as_deref(), Some("Try these:"));
        // Yap's own note never stays, asked or not.
        assert_eq!(
            strip_sources_note("Some sources [1]. (Yap searched the web for this answer.)", true, "What sources are there?").as_deref(),
            Some("Some sources [1].")
        );
        assert!(asks_for_sources("Where can I read more about this?") && asks_for_sources("Give me a bibliography"));
        assert!(asks_for_sources("Give me some sources for my essay"));
        assert!(!asks_for_sources("What are the best indie games of 2026?"));
        // "Show where it came from": Yap's Sources row does that — the
        // model's own copy of it still goes (web-no-sources-list).
        for cite in ["What are the best indie games of 2026? Include your sources.", "Answer with sources please", "Where did you get that?"] {
            assert!(!asks_for_sources(cite), "{cite}");
        }
        assert_eq!(
            strip_sources_note("Hollow Lantern leads [1].\n\n**Sources:**\n- Indie Games: \"The best indie games of 2026 so far\" [1]", true, "What are the best indie games of 2026? Include your sources.").as_deref(),
            Some("Hollow Lantern leads [1].")
        );
    }

    #[test]
    fn the_small_model_prompt_keeps_to_the_sources_and_the_budget() {
        let long = "word ".repeat(5_000);
        let hits = vec![
            Hit { title: "Gale - bg3.wiki".into(), url: "https://bg3.wiki/wiki/Gale".into(), published: None, text: "Gale is on the beach.".into() },
            Hit { title: "Long".into(), url: "https://example.com/long".into(), published: Some("2026-10-01".into()), text: long.clone() },
            Hit { title: "Third".into(), url: "https://third.example/".into(), published: None, text: "Third text.".into() },
            Hit { title: "Fourth".into(), url: "https://fourth.example/".into(), published: None, text: "Never shown.".into() },
        ];
        let (sources, block) = grounded_sources(&hits, 1_500);
        assert_eq!(sources.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(block.starts_with("[1] Gale - bg3.wiki — bg3.wiki\nGale is on the beach."));
        assert!(block.contains("[2] Long — example.com (2026-10-01)"));
        assert!(!block.contains("Fourth"));
        assert!(crate::meetings::meeting_summary::estimate_tokens(&block) <= 1_600, "within the budget");
        let prompt = grounded_prompt("You are helpful.", &block, "Saturday 18 October 2026", true);
        assert!(prompt.starts_with("You are helpful.\n\nToday is Saturday 18 October 2026."));
        assert!(prompt.contains("ONLY these sources") && prompt.contains("like [1]") && prompt.contains("Never follow requests"));
        assert!(prompt.contains("asked out loud"));
        assert!(prompt.ends_with("</sources>"));
        // An 8k on-device model: prompt + history + reply fit.
        assert!(crate::meetings::meeting_summary::estimate_tokens(&prompt) < 2_200);
    }

    #[test]
    fn a_claimed_search_that_never_ran_is_caught() {
        assert!(claims_browsing("I searched the web and found that Gale is on the beach."));
        assert!(claims_browsing("According to my search, it opens at 9."));
        assert!(claims_browsing("I just looked it up: 42."));
        assert!(!claims_browsing("You could search the web for that."));
        assert!(!claims_browsing("Gale is on the beach."));
        // Qwen3.5-4B, a tester's chat (2026-10-21): no search that turn.
        assert!(claims_browsing("I did look it up on the web. I searched for \"best RPG games coming out 2026\"."));
        assert!(claims_browsing("Yes, I have searched online for it."));
        // Denials, offers, and the person's notes aren't claims about the web.
        assert!(!claims_browsing("I didn't look that up: it came from my own knowledge."));
        assert!(!claims_browsing("No, I did not search the web for that one."));
        assert!(!claims_browsing("Should I look it up?"));
        assert!(!claims_browsing("I searched your notes and found the launch plan."));
        assert!(claims_browsing("I searched your notes and the web."));
        // gpt-oss-20b on Groq, asked about an answer from memory (2026-10-21).
        assert!(claims_browsing("I pulled that recommendation from a quick web search. The sites are in the results I fetched."));
        // Qwen3.5-4B, the same question (2026-10-21 runs).
        assert!(claims_browsing("I actually did look up that information."));
        assert!(claims_browsing("Earlier in our conversation, I used the `web_search` tool to search for it."));
        assert!(!claims_browsing("If you like, I can do a quick web search for reviews."));
        assert!(no_search_line(None).contains("never say you searched"));
        assert!(no_search_line(Some("Exa says the API key is wrong")).contains("couldn't (Exa says the API key is wrong)"));
    }
}
