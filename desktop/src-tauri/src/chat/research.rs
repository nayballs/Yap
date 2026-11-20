//! **Research** (16 Nov 2026; the Claude app's Research mode as Anthropic's
//! help pages describe it: turned on under the message box, then "multiple
//! searches that build on each other", deciding what to look into next from
//! what it finds, and a thorough answer "with easy-to-check citations").
//! Chat's side only, with web search on.
//!
//! - **Asked for** ([`wanted`]): the **Research** switch under the message box
//!   (`ResearchChip.svelte`, beside Style; it stays on until it's turned off,
//!   as the Claude app's does), or a message that plainly asks for research
//!   ([`asks`]: "research the history of…", "do some research on…", "a deep
//!   dive into…"). Either way the model decides in its plan step: a greeting
//!   or a thank-you is answered as ever ([`Planned::NotResearch`]).
//! - **The plan first** ([`PLAN_PROMPT`], [`parse_plan`]): one call with no
//!   tools writes the report's title, a sentence on what it will find out and
//!   a heading per part with its one line — shown as the outline card's
//!   research plan (`cards/outline.rs`, `OutlineCard::research`: Start
//!   research / Edit), and nothing is looked up until the person answers.
//! - **The loop** (`turn/research.rs`): the turn's own tool loop (or, on the
//!   Claude plan, the conversation's Claude Code session) with the plan and
//!   [`instructions`] in the question's context: as many searches and page
//!   reads as it needs ([`UNCAPPED`], since 20 Nov 2026; Yap's tool loop's
//!   reads what the model's request holds), [`TIME`] its backstop
//!   ([`Budget`]). A cap reached is a refusal saying to write the report now
//!   ([`Spent::line`]); past the time and [`GRACE`] the loop asks for the
//!   report with no tools. Progress shows
//!   on the activity line ([`Quiet`]: "… · 6 searches, 3 pages read"); the
//!   report's own words never stream into the thread.
//! - **The report** ([`report`]): a document beside the chat, its citations
//!   only to pages the turn **read** (a search's result isn't one), numbered
//!   1, 2, 3… in the order the report first cites them, each a link to its
//!   page, with a numbered Sources list at the end; in the thread, the
//!   report's summary and the document's card.
//! - **The outside-content rules hold**: the loop is Chat's own, so after the
//!   web apps, the clipboard and memory stay locked (`tools::refused_after_outside`),
//!   and a research turn runs nothing but the web's tools and the notes'
//!   reads ([`allows`]).

use std::future::Future;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

use crate::chat::cards::outline::Section;
use crate::chat::chats::Source;
use crate::chat::stream::{Event, Sink};

pub mod helpers;

/// **No cap** (20 Nov 2026, the maintainer: Research uncapped, as the Claude app's,
/// whose help pages give it no limit on searches, sources or tool calls): a
/// research makes as many searches, and reads as many pages, as the
/// question needs. Until then: 20 searches, 12 pages, 10 minutes, 1M tokens.
pub const UNCAPPED: usize = usize::MAX;
/// The fewest pages a research in Yap's tool loop may read
/// ([`Caps::for_request`]).
pub const MIN_READS: usize = 4;
/// **Not a cap on how far it goes**: a research still looking things up
/// after 45 minutes — the longest Anthropic says its own Research takes —
/// is asked for its report (a loop that never ends would otherwise run on).
pub const TIME: Duration = Duration::from_secs(45 * 60);
/// After [`TIME`], how long the report may take before the loop asks for it
/// with no tools at all.
pub const GRACE: Duration = Duration::from_secs(3 * 60);
/// The tool loop's steps for a research (every other turn: 20): its
/// [`TIME`] is what ends it.
pub const STEPS: usize = 400;

/// **How far a research may go**: searches, page reads, time and tokens
/// ([`UNCAPPED`] and `u64::MAX`: no limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub searches: usize,
    pub reads: usize,
    pub time: Duration,
    pub tokens: u64,
}

impl Caps {
    /// A research's: no limit on searches, pages or tokens; [`TIME`] its
    /// backstop.
    pub const USUAL: Caps = Caps { searches: UNCAPPED, reads: UNCAPPED, time: TIME, tokens: u64::MAX };

    /// [`Caps::USUAL`] with as many page reads as half the model's request
    /// holds in pages of `page` tokens (`request`: its tokens; `None`, no
    /// limit — your Claude plan's Claude Code keeps its own context), at
    /// least [`MIN_READS`]: Yap's tool loop carries every page it read in
    /// each request, so its reads are what the model can hold, never fewer
    /// for being a research. Pure.
    pub fn for_request(request: Option<u64>, page: usize) -> Caps {
        let reads = request.map_or(UNCAPPED, |r| (((r / 2) / page.max(1) as u64) as usize).max(MIN_READS));
        Caps { reads, ..Caps::USUAL }
    }

    /// The web turn's reach (`web::Reach`).
    pub fn reach(&self) -> crate::web::Reach {
        crate::web::Reach { searches: self.searches, reads: self.reads }
    }
}

/// **What ran out.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spent {
    Searches,
    Reads,
    Time,
    Tokens,
}

impl Spent {
    /// What the model reads when it's out: write the report now. Pure.
    pub fn line(self, caps: &Caps) -> String {
        self.line_for(caps, false)
    }

    /// The same for a research's helper (`helper`, chat/research/helpers.rs):
    /// its findings now, not the report. Pure.
    pub fn line_for(self, caps: &Caps, helper: bool) -> String {
        let what = match self {
            Spent::Searches => format!("You've made the {} searches this research may make", caps.searches),
            Spent::Reads => format!("You've read the {} pages your context holds", caps.reads),
            Spent::Time => format!("This research has had its {} minutes", caps.time.as_secs() / 60),
            Spent::Tokens => "This research has used the tokens it may use".to_string(),
        };
        let write = if helper { "write your findings now" } else { "write the report now" };
        format!("{what}: {write}, from the pages you read (a fact none of them gave is said to be unconfirmed, never guessed). Don't call any more tools.")
    }

    /// The chip's ending ("research limit: 20 searches"). Pure.
    pub fn chip(self, caps: &Caps) -> String {
        match self {
            Spent::Searches => format!("research limit: {} searches", caps.searches),
            Spent::Reads => format!("the {} pages the model holds", caps.reads),
            Spent::Time => format!("research limit: {} minutes", caps.time.as_secs() / 60),
            Spent::Tokens => "research limit: tokens".to_string(),
        }
    }
}

/// **What's spent of `caps`** after `searches` searches, `reads` page reads,
/// `elapsed` and `tokens`: the first cap reached, else `None`. Time and
/// tokens first (they stop everything); then the searches, then the reads.
/// Pure.
pub fn spent(caps: &Caps, searches: usize, reads: usize, elapsed: Duration, tokens: u64) -> Option<Spent> {
    if elapsed >= caps.time {
        Some(Spent::Time)
    } else if tokens >= caps.tokens {
        Some(Spent::Tokens)
    } else if searches >= caps.searches {
        Some(Spent::Searches)
    } else if reads >= caps.reads {
        Some(Spent::Reads)
    } else {
        None
    }
}

/// **Past a cap, on the Claude plan** (`turn/plan.rs`): Claude Code's own
/// `WebSearch` and `WebFetch` can't be refused one at a time, so its turn
/// stops at the first call past a cap (`begun`: the searches and fetches it
/// began; `done`: those that came back, a fetch only when it read the page)
/// — or once both caps are reached, or the time or tokens are spent — and
/// is asked for the report. Pure.
pub fn past(caps: &Caps, begun: (usize, usize), done: (usize, usize), elapsed: Duration, tokens: u64) -> Option<Spent> {
    match spent(caps, 0, 0, elapsed, tokens) {
        Some(s) => Some(s),
        None if begun.0 > caps.searches => Some(Spent::Searches),
        None if begun.1 > caps.reads => Some(Spent::Reads),
        None if done.0 >= caps.searches && done.1 >= caps.reads => Some(Spent::Searches),
        None => None,
    }
}

/// What a plan turn stopped at a cap is told next: what ran out, and the
/// report now (`turn/plan.rs`). Pure.
pub fn write_now_after(why: Spent, caps: &Caps) -> String {
    format!("(A note from Yap, not the user.) {} Yap stopped the research there. Write the report now as your answer, from the pages you read, as the research turn said: # title, ## Summary, a ## section for each part of the plan, every fact cited as a link to a page you read. Don't search or fetch anything more.", why.line(caps).split(':').next().unwrap_or("The research is over").trim_end_matches('.').to_string() + ".")
}

/// **A research under way**: its caps and its clock — the research's own,
/// or one of its helpers' ([`Budget::helper`], chat/research/helpers.rs).
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub caps: Caps,
    started: Instant,
    /// One of the research's helpers: what it's told to write when it's
    /// out, or answered before searching, is its findings, not the report.
    pub helper: bool,
}

impl Budget {
    pub fn start(caps: Caps) -> Budget {
        Budget { caps, started: Instant::now(), helper: false }
    }

    /// **One of its helpers'** (chat/research/helpers.rs): the research's
    /// own caps and clock — no limit of its own — told as a helper's.
    pub fn for_helper(self) -> Budget {
        Budget { helper: true, ..self }
    }

    /// **The lead's, after its helpers**: the research's clock, or — when
    /// they left it less than [`helpers::LEAD_AT_LEAST`] — that long from now.
    pub fn for_lead(self) -> Budget {
        if self.caps.time.saturating_sub(self.elapsed()) >= helpers::LEAD_AT_LEAST {
            Budget { helper: false, ..self }
        } else {
            Budget::start(Caps { time: helpers::LEAD_AT_LEAST, ..self.caps })
        }
    }

    /// A budget that began `ago` (the tests' clock).
    #[cfg(test)]
    pub fn started_ago(caps: Caps, ago: Duration) -> Budget {
        Budget { caps, started: Instant::now().checked_sub(ago).unwrap_or_else(Instant::now), helper: false }
    }

    /// What the model reads when `why` ran out ([`Spent::line_for`]).
    pub fn line(&self, why: Spent) -> String {
        why.line_for(&self.caps, self.helper)
    }

    /// What a research turn that answered in words before any search is
    /// told (once): [`SEARCH_FIRST`], or a helper's.
    pub fn search_first(&self) -> &'static str {
        if self.helper {
            crate::chat::research::helpers::SEARCH_FIRST
        } else {
            SEARCH_FIRST
        }
    }

    /// The loop's last call, past the time and its grace: [`WRITE_NOW`], or
    /// a helper's.
    pub fn write_now(&self) -> &'static str {
        if self.helper {
            crate::chat::research::helpers::WRITE_NOW
        } else {
            WRITE_NOW
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Time or tokens spent (`tokens`: the turn's so far): what stops every
    /// search and read. The searches' and reads' own caps are the web
    /// turn's reach.
    pub fn out(&self, tokens: u64) -> Option<Spent> {
        let s = spent(&self.caps, 0, 0, self.elapsed(), tokens);
        matches!(s, Some(Spent::Time | Spent::Tokens)).then_some(s).flatten()
    }

    /// Past its time and [`GRACE`]: the report now, with no tools.
    pub fn overdue(&self) -> bool {
        self.elapsed() >= self.caps.time + GRACE
    }
}

// ---- asked for ------------------------------------------------------------------------------

tokio::task_local! {
    static SWITCHED: bool;
}

/// Run `f` (a Chat turn) with the Research switch as the page sent it.
pub async fn scoped<F: Future>(on: bool, f: F) -> F::Output {
    SWITCHED.scope(on, f).await
}

/// The Research switch was on for this turn's question.
pub fn switched() -> bool {
    SWITCHED.try_with(|on| *on).unwrap_or(false)
}

/// **A message that plainly asks for research**: "research the history of
/// the QWERTY layout", "can you research…", "do some research on…", "deep
/// research…", "a deep dive into…", "write me a research report on…",
/// "use research mode". Not a question about research ("what does the
/// research say", "research shows…", "my research"). Pure.
pub fn asks(text: &str) -> bool {
    static START: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(?:(?:hey|hi|hello|ok|okay|so|now|right|yap|claude)\b[\s,!.]*)*(?:(?:please|pls|can you|could you|would you|will you|i'd like you to|i would like you to|i want you to|go and|go)\s+)*research\s+(\S+)").expect("static regex")
    });
    static ANY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:do|run|carry out|make|start)\s+(?:some|a bit of|a little|more|proper|thorough|in-depth|detailed|full|the|a)?\s*(?:deep\s+|thorough\s+|proper\s+)?research\b|\bdeep research\b|\bresearch (?:mode|tool|report)\b|\b(?:do|write|give)\s+(?:me\s+)?a\s+deep[- ]dive\b|\bdeep[- ]dive (?:on|into)\b").expect("static regex")
    });
    // "Research shows…", "research is…": a statement, not a request.
    const NOT_A_VERB: &[&str] = &["shows", "show", "says", "say", "suggests", "suggest", "is", "are", "was", "were", "has", "have", "had", "indicates", "found", "finds", "proves", "papers", "paper", "about", "into"];
    let t = text.trim();
    if let Some(c) = START.captures(t) {
        let next = c[1].trim_matches(|ch: char| !ch.is_alphanumeric()).to_lowercase();
        if !NOT_A_VERB.contains(&next.as_str()) {
            return true;
        }
    }
    ANY.is_match(t)
}

// ---- the plan -------------------------------------------------------------------------------

/// The most parts a plan has, and the fewest it should.
pub const MAX_PARTS: usize = 7;

/// **The research plan** the person sees first: the report's title, a
/// sentence on what it will find out, a heading per part with its one line
/// — and, once answered, their note.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub title: String,
    pub summary: String,
    pub sections: Vec<Section>,
    /// The person's note on the plan (Edit's box, or what they said).
    pub note: String,
}

impl Plan {
    /// The card's arguments (`cards/outline.rs` `review`): a research plan.
    pub fn args(&self) -> Value {
        json!({
            "title": self.title,
            "summary": self.summary,
            "sections": self.sections.iter().map(|s| json!({ "key": s.key, "intent": s.intent })).collect::<Vec<_>>(),
            "research": true,
        })
    }
}

/// What the plan step came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    Plan(Plan),
    /// Not something to research (a greeting, a thank-you): answered as ever.
    NotResearch,
}

/// **The plan step's prompt** (a call with no tools, before any search).
pub const PLAN_PROMPT: &str = "You plan web research for Yap's Research mode, before anything is looked up. Read the user's latest message (and the chat before it, for what it points back to) and reply with JSON only, no other words:\n\
{\"research\": true, \"title\": \"the report's title, a few words\", \"summary\": \"one sentence: what the report will find out\", \"sections\": [{\"key\": \"a short heading\", \"intent\": \"what this part will find out, in one line\"}]}\n\
Give 3 to 6 sections, in the order the report will take them; each a question the research will answer, not a filler like \"Introduction\" or \"Conclusion\". \
A picture with the message (a screenshot) is part of it: \"is this true?\" asks about what the picture shows or says, so research that. \
The user turned Research on, or asked for it: reply {\"research\": false} only for a greeting, thanks, or a change to something already written in this chat.";

/// The plan step's message: the chat's last few messages (`earlier`, (role,
/// text)), today, and the question. Pure.
pub fn plan_request(question: &str, earlier: &[(String, String)], today: &str) -> String {
    let mut s = String::new();
    let recent: Vec<&(String, String)> = earlier.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    if !recent.is_empty() {
        s.push_str("Earlier in this chat:\n");
        for (role, text) in recent {
            let who = if role == "assistant" { "Assistant" } else { "User" };
            let text: String = text.chars().take(600).collect();
            s.push_str(&format!("{who}: {}\n", text.trim()));
        }
        s.push('\n');
    }
    if !today.is_empty() {
        s.push_str(&format!("Today is {today}.\n\n"));
    }
    s.push_str("The user's message: ");
    s.push_str(question.trim());
    s
}

/// The first JSON object in `reply` (a fenced block, or words around it).
fn first_object(reply: &str) -> Option<Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    (end > start).then(|| serde_json::from_str::<Value>(&reply[start..=end]).ok()).flatten().filter(Value::is_object)
}

/// `s` on one line, at most `max` characters ("…" when cut). Pure.
fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// **A plan from the question alone**: when the plan step's reply had none
/// (or the call failed): its words as the title, three general parts the
/// person can change. Pure.
pub fn fallback_plan(question: &str) -> Plan {
    let q = question.trim().trim_start_matches(|c: char| !c.is_alphanumeric());
    let q = Regex::new(r"(?i)^(?:(?:please|can you|could you|would you)\s+)*(?:do (?:some |a bit of )?research (?:on|into|about)|research)\s+").map(|r| r.replace(q, "").into_owned()).unwrap_or_else(|_| q.to_string());
    let mut title = one_line(q.trim_end_matches(['.', '?', '!']), 80);
    if let Some(first) = title.chars().next() {
        title = first.to_uppercase().collect::<String>() + &title[first.len_utf8()..];
    }
    if title.is_empty() {
        title = "Research".to_string();
    }
    let sec = |k: &str, i: &str| Section { key: k.into(), intent: i.into() };
    Plan {
        title,
        summary: String::new(),
        sections: vec![sec("Background", "what it is and where it comes from"), sec("What the sources say", "the main findings, and where sources disagree"), sec("What it means", "what follows from it, and what's still open")],
        note: String::new(),
    }
}

/// **The plan step's reply, read** (`question`: the person's, for a plan
/// when the reply has none). Pure.
pub fn parse_plan(reply: &str, question: &str) -> Planned {
    let Some(v) = first_object(reply) else { return Planned::Plan(fallback_plan(question)) };
    if v["research"] == json!(false) {
        return Planned::NotResearch;
    }
    let mut sections = crate::chat::cards::outline::sections_of(&v["sections"]);
    sections.truncate(MAX_PARTS);
    let title = one_line(v["title"].as_str().unwrap_or(""), 100);
    if sections.is_empty() {
        let mut plan = fallback_plan(question);
        if !title.is_empty() {
            plan.title = title;
        }
        return Planned::Plan(plan);
    }
    let title = if title.is_empty() { fallback_plan(question).title } else { title };
    Planned::Plan(Plan { title, summary: one_line(v["summary"].as_str().unwrap_or(""), 300), sections, note: String::new() })
}

// ---- the loop's words -------------------------------------------------------------------------

/// **What the research loop is told** (the question's context): the plan,
/// how to research it, the caps, and how to write the report — Yap's tool
/// loop (`web_search`, `read_page`; `claude_code`: Claude Code's `WebSearch`
/// and `WebFetch`, citing as links). Pure.
///
/// **The lead of a research whose helpers have done the searching**
/// (`helpers`: their findings, `helpers::lead_block`): it writes the report
/// from them, searching more itself only where a part is thin.
pub fn instructions(plan: &Plan, caps: &Caps, claude_code: bool, helpers: Option<&str>) -> String {
    let (search, read, cite) = if claude_code {
        ("WebSearch", "WebFetch", "cite it right after it as a markdown link to that page whose text is a number, like [3](https://example.com/page)")
    } else {
        ("web_search", "read_page", "cite it right after it with the page's number in square brackets, like [3]")
    };
    let mut parts: Vec<String> = Vec::new();
    for (i, s) in plan.sections.iter().enumerate() {
        if s.intent.is_empty() {
            parts.push(format!("{}. {}", i + 1, s.key));
        } else {
            parts.push(format!("{}. {} — {}", i + 1, s.key, s.intent));
        }
    }
    let mut out = format!(
        "This is a research turn (Yap's Research mode). The user approved this plan for a report titled \"{}\"{}:\n{}",
        plan.title,
        if plan.summary.is_empty() { String::new() } else { format!(" ({})", plan.summary) },
        parts.join("\n")
    );
    if !plan.note.is_empty() {
        out.push_str(&format!("\nTheir note for the research, to follow throughout: \"{}\"", plan.note));
    }
    let room = if caps.reads == UNCAPPED { String::new() } else { format!(" You can read up to {} pages in all: what your context holds.", caps.reads) };
    if let Some(block) = helpers {
        out.push_str("\n\n");
        out.push_str(block);
        out.push_str(&format!(
            "\n\nYou lead this research: write the report from your helpers' findings. There's no limit on searches or pages: where a part is thin, its helper didn't finish, or the findings disagree on something that matters, search and read more yourself first ({search}, then {read} on the pages your facts come from; a search result's snippet isn't a source), the searches and reads that don't depend on each other at once.{room} Otherwise write the report now, without searching again. Where sources disagree, say so.\n\n\
Write the report as your answer, in markdown: a # title; then ## Summary, two to four sentences that answer the question; then a ## section for each part of the plan, in its order (add one only for something important the plan didn't foresee). \
Every fact from a page: {cite}. Cite only pages read — your helpers' (their notes' numbers) or your own with {read} — never a page only seen in search results. No list of sources at the end (Yap adds it), no offer to do more. \
Don't make a note or a document yourself: Yap puts your report in a document beside the chat."
        ));
        return out;
    }
    out.push_str(&format!(
        "\n\nResearch it thoroughly on the web ({search}), as a careful researcher would, with many searches that build on each other: start broad, then follow what you find — names, dates, claims to check, the other side of a question — and read the pages your facts come from ({read}; a search result's snippet isn't a source). \
There's no limit on searches or pages, and Research mode is for depth: the user turned it on to have this researched thoroughly, well past what an ordinary answer looks up. \
Give every part of the plan searches of its own (at least two or three, from different angles), follow the leads they turn up, and confirm each important fact in more than one independent source (an official or expert body, a study, a reputable publication); don't stop because the first pages agree. \
Make the searches and reads that don't depend on each other at once (several tool calls in one step).{} Where sources disagree, say so.\n\n\
When every part of the plan is well answered, stop calling tools and write the report as your answer, in markdown: a # title; then ## Summary, two to four sentences that answer the question; then a ## section for each part of the plan, in its order (add one only for something important the plan didn't foresee). \
Every fact from a page: {cite}. Cite only pages you read with {read}, never a page you only saw in search results. No list of sources at the end (Yap adds it), no offer to do more. \
Don't make a note or a document yourself: Yap puts your report in a document beside the chat.",
        room,
    ));
    out
}

/// **The report, now** (the loop's last call with no tools, past the time
/// and its grace; and the Claude plan's message after a cap).
pub const WRITE_NOW: &str = "(A note from Yap, not the user.) The research is over: write the report now as your answer, from the pages you read, as the research turn said (# title, ## Summary, a ## section for each part of the plan, every fact cited by the number of a page you read). Don't call any tool.";

/// What a research turn that answered in words before any search is told
/// (once).
pub const SEARCH_FIRST: &str = "(A note from Yap, not the user.) This is a research turn: research the plan on the web first (web_search, then read_page on the pages your facts come from), then write the report.";

/// The tools a research turn runs: the web's, `find_pictures` aside, the
/// notes' reads, and a big project's (`read_project_file`,
/// `search_project_files`: read only, offered only in a project chat whose
/// files are searched in parts — until 16 Nov 2026 offered there and then
/// refused). Everything else is refused ([`refusal`]). Pure.
pub fn allows(name: &str) -> bool {
    crate::web::TOOL_NAMES.contains(&name) || matches!(name, "search_notes" | "get_note" | "read_note" | "search_in_note") || crate::chat::projects::knowledge::NAMES.contains(&name)
}

/// A tool a research turn doesn't run: its result. Pure.
pub fn refusal(name: &str) -> Value {
    json!({
        "success": false,
        "error": format!("{name} doesn't run in a research turn: only web_search and read_page (and reading notes or the project's files). When you've researched enough, write the report as your answer; Yap puts it in a document."),
        "data": Value::Null,
        "displayText": "",
    })
}

// ---- progress -------------------------------------------------------------------------------

/// How many searches and page reads `chips` and `sources` say the turn
/// made (a search's chip, a page read's mark). Pure.
pub fn counts(chips: &[String], sources: &[Source]) -> (usize, usize) {
    let searches = chips.iter().filter(|c| c.starts_with("Searched the web")).count();
    let reads = sources.iter().filter(|s| s.opened).count();
    (searches, reads)
}

/// "6 searches, 3 pages read". Pure.
pub fn progress(searches: usize, reads: usize) -> String {
    let s = if searches == 1 { "1 search".to_string() } else { format!("{searches} searches") };
    let r = if reads == 1 { "1 page read".to_string() } else { format!("{reads} pages read") };
    format!("{s}, {r}")
}

/// **The research's stream, quiet**: the report's words never show in the
/// thread (it goes into a document), the line says "Writing the report"
/// instead, and every line carries the progress so far ("Reading
/// en.wikipedia.org/wiki/QWERTY · 6 searches, 3 pages read") — told again
/// whenever a search or a read comes back.
pub struct Quiet<'a> {
    inner: &'a dyn Sink,
    state: Mutex<QuietState>,
}

#[derive(Default)]
struct QuietState {
    searches: usize,
    reads: usize,
    /// The helpers' searches, before this loop's (`Quiet::after`).
    before: usize,
    /// The report's words have begun ("Writing the report").
    writing: bool,
    /// The step under way, its line without the progress.
    step: Option<crate::chat::activity::Activity>,
}

impl<'a> Quiet<'a> {
    /// `inner`'s stream, quiet — after a research's helpers (chat/research/
    /// helpers.rs; none: 0 and 0), their `searches` counted in from the
    /// start (their pages read are the lead's first sources, so its chips
    /// count them already), `reads` until its first step comes back.
    pub fn after(inner: &'a dyn Sink, searches: usize, reads: usize) -> Quiet<'a> {
        Quiet { inner, state: Mutex::new(QuietState { searches, reads, before: searches, ..QuietState::default() }) }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, QuietState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The step's line with the progress so far, to the page.
    fn tell(&self, mut step: crate::chat::activity::Activity) {
        let (s, r) = {
            let st = self.state();
            (st.searches, st.reads)
        };
        step.line = format!("{} · {}", step.line, progress(s, r));
        self.inner.send(Event::Step { step });
    }

    /// "Writing the report", once its words start.
    fn writing(&self) {
        let step = crate::chat::activity::Activity::research("Writing the report");
        let told = {
            let mut st = self.state();
            st.step = Some(step.clone());
            std::mem::replace(&mut st.writing, true)
        };
        if !told {
            self.tell(step);
        }
    }
}

impl Sink for Quiet<'_> {
    fn send(&self, ev: Event) {
        match ev {
            Event::Text { text } => {
                if !text.trim().is_empty() {
                    self.writing();
                }
            }
            Event::Step { step } if step.mark == "answer" => self.writing(),
            Event::Step { step } => {
                // (A tool call after the report began: not writing any more.)
                {
                    let mut st = self.state();
                    st.writing = false;
                    st.step = Some(step.clone());
                }
                self.tell(step);
            }
            Event::Chips { chips, sources, web } => {
                let (s, r) = counts(&chips, &sources);
                let again = {
                    let mut st = self.state();
                    let s = s + st.before;
                    let changed = (st.searches, st.reads) != (s, r);
                    st.searches = s;
                    st.reads = r;
                    st.step.clone().filter(|_| changed)
                };
                self.inner.send(Event::Chips { chips, sources, web });
                // The line again, counted.
                if let Some(step) = again {
                    self.tell(step);
                }
            }
            other => self.inner.send(other),
        }
    }

    fn unattended(&self) -> bool {
        self.inner.unattended()
    }
}

// ---- the report -----------------------------------------------------------------------------

/// **The report, made ready** ([`report`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The document's title.
    pub title: String,
    /// The document: its # title, the report with its citations as links to
    /// the pages read, and the Sources list.
    pub body: String,
    /// The thread's short answer: the summary (its citations as Yap's [n]),
    /// and where the report is.
    pub answer: String,
    /// The answer's sources: the pages the report cites, numbered as it
    /// cites them (marked cited and read), then the other pages read — never
    /// a search's result nobody opened (a Lab run on Opus found 103 pages
    /// and read 8: "+99 more" under the answer said nothing).
    pub sources: Vec<Source>,
    /// How many pages the report cites.
    pub cited: usize,
    /// Citations taken out: to a page the turn never read, or no page at all.
    pub dropped: usize,
}

/// A run of citations: "[1]", "[2, 3]", "[4–6]", "[1][4]" (not a markdown
/// link's text, "[1](https://…)").
static RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" ?(?:\[\d{1,3}(?:\s*[,–-]\s*\d{1,3})*\])+").expect("static regex"));

/// The numbers in one run (a range of ten at most spelled out). Pure.
fn run_numbers(run: &str) -> Vec<usize> {
    static ONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]").expect("static regex"));
    let mut out = Vec::new();
    for c in ONE.captures_iter(run) {
        let inner = &c[1];
        let nums: Vec<usize> = inner.split([',', '–', '-']).filter_map(|n| n.trim().parse().ok()).collect();
        let range = inner.contains(['–', '-']) && nums.len() == 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10;
        let nums = if range { (nums[0]..=nums[1]).collect() } else { nums };
        for n in nums {
            if n > 0 && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// **The report's citations renumbered**: each `[n]` run kept only for the
/// pages the turn read (`read`: the turn's numbers of those), numbered 1, 2,
/// 3… in the order `text` first cites them (`order`, the turn's numbers,
/// carried on from an earlier part), written by `write` (the new numbers in
/// the run); a run with none left goes, with the space before it. Its count
/// of citations taken out. Pure.
fn renumber(text: &str, read: &[usize], order: &mut Vec<usize>, write: &dyn Fn(&[usize]) -> String) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut dropped = 0;
    for m in RUN.find_iter(text) {
        // A markdown link's own text ("[1](https://…)") isn't a citation.
        if text[m.end()..].starts_with('(') {
            continue;
        }
        let nums = run_numbers(m.as_str());
        let mut new: Vec<usize> = Vec::new();
        for n in nums {
            if !read.contains(&n) {
                dropped += 1;
                continue;
            }
            let at = match order.iter().position(|o| *o == n) {
                Some(i) => i + 1,
                None => {
                    order.push(n);
                    order.len()
                }
            };
            if !new.contains(&at) {
                new.push(at);
            }
        }
        out.push_str(&text[last..m.start()]);
        if !new.is_empty() {
            if m.as_str().starts_with(' ') {
                out.push(' ');
            }
            out.push_str(&write(&new));
        }
        last = m.end();
    }
    out.push_str(&text[last..]);
    (out, dropped)
}

/// A heading line's level and words ("## Summary" → (2, "Summary")). Pure.
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    (level > 0 && level <= 6 && t[level..].starts_with(' ')).then(|| (level, t[level..].trim()))
}

/// Words a sources list's heading has ("Sources", "References"…).
fn is_sources_heading(words: &str) -> bool {
    let w = words.trim().trim_end_matches(':').to_lowercase();
    matches!(w.as_str(), "sources" | "references" | "bibliography" | "works cited" | "citations" | "further reading" | "sources consulted" | "sources cited")
}

/// **What came before a report's `# title`, if it's only a word to the
/// person** (Opus on the plan, 16 Nov 2026: "I have enough sources now, so
/// here's the report."): nothing, or at most [`PREAMBLE_LINES`] short lines
/// with no heading — never a part of the report itself. Pure.
fn preamble(lines: &[&str]) -> bool {
    let said: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    said.len() <= PREAMBLE_LINES && said.iter().all(|l| heading(l).is_none() && l.chars().count() <= 300)
}

/// The most lines a word before the report's title may be ([`preamble`]).
const PREAMBLE_LINES: usize = 3;

/// **The report as Yap keeps it** (`written`: the model's report, its
/// citations Yap's `[n]` — a plan turn's links made so by `claude_plan::
/// map::cite` —; `sources`: every page the turn found, read ones marked
/// `opened`; `plan_title`: the plan's, when the report has no # title;
/// `byline`: the line under the title, "*Researched 16 November 2026 · 12
/// searches, 8 pages read*"). A word to the person before its # title
/// ([`preamble`]) goes. Pure.
pub fn report(written: &str, sources: &[Source], plan_title: &str, byline: &str) -> Report {
    // Its own title, and nothing from a sources list it wrote anyway.
    let mut title = String::new();
    let mut lines: Vec<&str> = Vec::new();
    let mut in_sources = false;
    for line in written.lines() {
        match heading(line) {
            // (After a word or two before it — "I have enough sources now, so here's the report." — the
            // report starts there: those words go, never the document's first line under a title of its own.)
            Some((1, words)) if title.is_empty() && preamble(&lines) => {
                title = one_line(words, 120);
                lines.clear();
                continue;
            }
            Some((_, words)) if is_sources_heading(words) => {
                in_sources = true;
                continue;
            }
            Some(_) => in_sources = false,
            None => {}
        }
        if !in_sources {
            lines.push(line);
        }
    }
    if title.is_empty() {
        title = one_line(plan_title, 120);
    }
    if title.is_empty() {
        title = "Research".to_string();
    }
    let rest = lines.join("\n").trim().to_string();
    let read: Vec<usize> = sources.iter().filter(|s| s.opened).map(|s| s.n).collect();
    // The document's citations: links to the pages, numbered as first cited.
    let mut order: Vec<usize> = Vec::new();
    let link = |new: &[usize], order: &[usize]| -> String {
        new.iter()
            .map(|at| {
                let url = order.get(at - 1).and_then(|n| sources.iter().find(|s| s.n == *n)).map(|s| s.url.replace(['(', ')', ' '], "")).unwrap_or_default();
                format!("[[{at}]]({url})")
            })
            .collect()
    };
    // (Two passes: the order is set by the first, then the links written from it.)
    let (_, dropped) = renumber(&rest, &read, &mut order, &|new| new.iter().map(|n| format!("[{n}]")).collect());
    let fixed_order = order.clone();
    let mut again = fixed_order.clone();
    let (linked, _) = renumber(&rest, &read, &mut again, &|new| link(new, &fixed_order));
    // The summary for the thread: the ## Summary section's words, else the
    // first paragraph — its citations as Yap's [n], the same numbers.
    let summary = summary_of(&rest);
    let mut again = fixed_order.clone();
    let (short, _) = renumber(&summary, &read, &mut again, &|new| new.iter().map(|n| format!("[{n}]")).collect());
    // The answer's sources: the cited first, as numbered; then the other pages read.
    let mut out: Vec<Source> = Vec::new();
    for (i, n) in order.iter().enumerate() {
        if let Some(s) = sources.iter().find(|s| s.n == *n) {
            out.push(Source { n: i + 1, cited: true, ..s.clone() });
        }
    }
    let cited = out.len();
    for (i, s) in sources.iter().filter(|s| s.opened && !order.contains(&s.n)).enumerate() {
        out.push(Source { n: cited + i + 1, cited: false, ..s.clone() });
    }
    // The document.
    let mut body = format!("# {title}\n");
    if !byline.trim().is_empty() {
        body.push_str(&format!("\n*{}*\n", byline.trim().trim_matches('*')));
    }
    if !linked.is_empty() {
        body.push('\n');
        body.push_str(&linked);
        body.push('\n');
    }
    if cited > 0 {
        body.push_str("\n## Sources\n\n");
        for s in out.iter().take(cited) {
            let name = if s.title.trim().is_empty() { crate::web::short_url(&s.url) } else { s.title.replace(['[', ']'], "") };
            let site = if s.site.is_empty() { String::new() } else { format!(" · {}", s.site) };
            body.push_str(&format!("{}. [{}]({}){}\n", s.n, name, s.url.replace(['(', ')', ' '], ""), site));
        }
    }
    let answer = if short.trim().is_empty() {
        format!("The report, “{title}”, is open on the right.")
    } else {
        format!("{}\n\nThe full report is open on the right.", short.trim())
    };
    Report { title, body, answer, sources: out, cited, dropped }
}

/// **The report's summary**: its ## Summary (or "Key findings", "In short"…)
/// section's words, else its first paragraph that isn't a heading. Pure.
fn summary_of(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some((_, words)) = heading(line) {
            if inside {
                break;
            }
            let w = words.to_lowercase();
            inside = w.starts_with("summary") || w.starts_with("in short") || w.starts_with("key findings") || w.starts_with("the short answer") || w == "tl;dr" || w.starts_with("overview");
            continue;
        }
        if inside {
            lines.push(line);
        }
    }
    let found = lines.join("\n").trim().to_string();
    if !found.is_empty() {
        return found;
    }
    // The first paragraph.
    let mut para: Vec<&str> = Vec::new();
    for line in text.lines() {
        if heading(line).is_some() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        if line.trim().is_empty() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        para.push(line);
    }
    para.join("\n").trim().to_string()
}

/// The line under the report's title: "Researched 16 November 2026 · 12
/// searches, 8 pages read". Pure.
pub fn byline(day: &str, searches: usize, reads: usize) -> String {
    if day.is_empty() {
        format!("Researched · {}", progress(searches, reads))
    } else {
        format!("Researched {day} · {}", progress(searches, reads))
    }
}

/// The turn's chip for the research: "Researched for 6 min: 12 searches, 8
/// pages read". Pure.
pub fn done_chip(searches: usize, reads: usize, elapsed: Duration, helpers: usize) -> String {
    let mins = (elapsed.as_secs() + 30) / 60;
    let took = if mins == 0 { "under a minute".to_string() } else { format!("{mins} min") };
    let with = if helpers > 1 { format!(" with {helpers} helpers") } else { String::new() };
    format!("Researched for {took}{with}: {}", progress(searches, reads))
}

// ---- what the page asks -----------------------------------------------------------------------

/// **Whether Chat's model can research now** (the Research chip: shown
/// greyed with why, when it can't).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchInfo {
    pub can: bool,
    /// Why not, in plain words ("" when it can).
    pub why: String,
}

/// [`ResearchInfo`] for Chat's model and settings: web search on, and a
/// model that calls tools (or your Claude plan). Pure but for the config.
pub fn info_for(cfg: &crate::config::YapConfig) -> ResearchInfo {
    if !cfg.web_search {
        return ResearchInfo { can: false, why: "Research looks things up on the web: turn on web search in Settings → Voice Assistant → Web search.".into() };
    }
    match crate::chat::turn::resolve_chat_endpoint(cfg) {
        Err(why) => ResearchInfo { can: false, why },
        Ok((_, _, model, provider, _, _)) => {
            if provider == crate::ai::claude_plan::PROVIDER || crate::chat::tools::supports_tools(&provider, &model) {
                ResearchInfo { can: true, why: String::new() }
            } else {
                ResearchInfo { can: false, why: "This model can't search the web step by step: pick one that calls tools for Research.".into() }
            }
        }
    }
}

/// The Research chip's question: can Chat's model research now?
#[tauri::command]
pub fn chat_research_info() -> ResearchInfo {
    info_for(&crate::config::load())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(n: usize, url: &str, opened: bool) -> Source {
        Source { n, title: format!("Page {n}"), url: url.into(), site: crate::web::site_of(url), opened, ..Default::default() }
    }

    #[test]
    fn a_plain_ask_for_research_is_one_and_a_question_about_research_isnt() {
        for yes in [
            "research the history of the QWERTY keyboard layout",
            "Research QWERTY's history",
            "Can you research why the QWERTY layout won?",
            "please research tidal power in the UK",
            "Hey Yap, research the Dvorak layout",
            "Do some research on keyboard layouts",
            "could you do a bit of research into typewriters",
            "I'd like a deep dive into the typewriter's history",
            "Write me a research report on mechanical keyboards",
            "use research mode for this: who invented the typewriter",
            "deep research: the first typewriters",
        ] {
            assert!(asks(yes), "{yes}");
        }
        for no in [
            "What does the research say about QWERTY?",
            "Research shows QWERTY was designed to slow typists, is that true?",
            "Research is hard",
            "my research notes are in the folder",
            "How do I research a topic well?",
            "Hello",
            "what's the capital of France",
        ] {
            assert!(!asks(no), "{no}");
        }
    }

    #[test]
    fn the_caps_stop_time_and_tokens_first_then_searches_then_reads() {
        // A research's own: no limit on searches, pages or tokens; 45 minutes its backstop.
        let caps = Caps::USUAL;
        let min = Duration::from_secs(60);
        assert_eq!((caps.searches, caps.reads, caps.tokens, caps.time), (UNCAPPED, UNCAPPED, u64::MAX, Duration::from_secs(45 * 60)));
        assert_eq!(spent(&caps, 0, 0, min, 0), None);
        assert_eq!(spent(&caps, 5_000, 5_000, Duration::from_secs(44 * 60), 50_000_000), None, "thousands of searches and pages, 50M tokens: still going");
        assert_eq!(spent(&caps, 0, 0, TIME, 0), Some(Spent::Time));
        // Caps the tests set (and a tool loop's reads, sized to its model): in that order.
        let caps = Caps { searches: 20, reads: 12, time: TIME, tokens: 1_000_000 };
        assert_eq!(spent(&caps, 19, 11, min, 999_999), None);
        assert_eq!(spent(&caps, 20, 0, min, 0), Some(Spent::Searches));
        assert_eq!(spent(&caps, 0, 12, min, 0), Some(Spent::Reads));
        assert_eq!(spent(&caps, 20, 12, min, 0), Some(Spent::Searches));
        assert_eq!(spent(&caps, 0, 0, TIME, 0), Some(Spent::Time));
        assert_eq!(spent(&caps, 0, 0, min, 1_000_000), Some(Spent::Tokens));
        assert_eq!(spent(&caps, 20, 12, TIME, 1_000_000), Some(Spent::Time));
        // A budget: only time and tokens stop it (the web turn counts the searches).
        let b = Budget::started_ago(caps, Duration::from_secs(30));
        assert_eq!(b.out(0), None);
        assert_eq!(b.out(1_000_000), Some(Spent::Tokens));
        assert_eq!(Budget::started_ago(Caps::USUAL, Duration::from_secs(30)).out(u64::MAX - 1), None);
        assert!(!b.overdue());
        let late = Budget::started_ago(caps, TIME + Duration::from_secs(5));
        assert_eq!(late.out(0), Some(Spent::Time));
        assert!(!late.overdue());
        assert!(Budget::started_ago(caps, TIME + GRACE).overdue());
        // The line the model reads, and the chip.
        assert!(Spent::Searches.line(&caps).starts_with("You've made the 20 searches"), "{}", Spent::Searches.line(&caps));
        assert!(Spent::Time.line(&caps).contains("write the report now"));
        assert_eq!(Spent::Reads.chip(&caps), "the 12 pages the model holds");
    }

    #[test]
    fn a_plan_turn_stops_at_the_first_call_past_a_cap() {
        let caps = Caps { searches: 3, reads: 2, ..Caps::USUAL };
        let min = Duration::from_secs(60);
        assert_eq!(past(&caps, (3, 2), (3, 1), min, 0), None);
        // The fourth search begun, or a third fetch: stopped.
        assert_eq!(past(&caps, (4, 1), (3, 1), min, 0), Some(Spent::Searches));
        assert_eq!(past(&caps, (2, 3), (2, 2), min, 0), Some(Spent::Reads));
        // Both caps reached and back: the report.
        assert_eq!(past(&caps, (3, 2), (3, 2), min, 0), Some(Spent::Searches));
        // Time and tokens first.
        assert_eq!(past(&caps, (0, 0), (0, 0), TIME, 0), Some(Spent::Time));
        assert_eq!(past(&caps, (0, 0), (0, 0), min, u64::MAX), Some(Spent::Tokens));
        // Uncapped (your Claude plan's): only the time stops it.
        assert_eq!(past(&Caps::USUAL, (400, 300), (400, 300), Duration::from_secs(44 * 60), 90_000_000), None);
        assert_eq!(past(&Caps::USUAL, (0, 0), (0, 0), TIME, 0), Some(Spent::Time));
        let said = write_now_after(Spent::Searches, &caps);
        assert!(said.starts_with("(A note from Yap, not the user.) You've made the 3 searches this research may make. Yap stopped the research there."), "{said}");
        assert!(said.contains("Don't search or fetch anything more."), "{said}");
    }

    #[test]
    fn the_reads_are_sized_to_what_the_models_request_holds() {
        // Your Claude plan's Claude Code keeps its own context: no limit.
        assert_eq!(Caps::for_request(None, 3_000).reads, UNCAPPED);
        // Yap's tool loop: as many pages as half the request holds, never fewer than 4 (until 20 Nov: at most 12).
        assert_eq!(Caps::for_request(Some(200_000), 3_000).reads, 33);
        assert_eq!(Caps::for_request(Some(1_000_000), 3_000).reads, 166);
        // Claude's own fetch puts 8,000 tokens a page in: an 80K request holds 5.
        assert_eq!(Caps::for_request(Some(80_000), 8_000).reads, 5);
        assert_eq!(Caps::for_request(Some(8_000), 3_000).reads, MIN_READS);
        assert_eq!(Caps::for_request(Some(8_000), 3_000).searches, UNCAPPED);
        assert_eq!(Caps::USUAL.reach(), crate::web::Reach { searches: UNCAPPED, reads: UNCAPPED });
    }

    #[test]
    fn the_plan_is_read_from_the_reply_and_a_greeting_isnt_research() {
        let reply = "```json\n{\"research\": true, \"title\": \"The QWERTY layout\", \"summary\": \"Where it came from and why it stayed.\", \"sections\": [{\"key\": \"Origins\", \"intent\": \"Sholes and the typewriter\"}, {\"key\": \"## Why it stayed\", \"intent\": \"\"}, \"Rivals\"]}\n```";
        let Planned::Plan(p) = parse_plan(reply, "research qwerty") else { panic!("a plan") };
        assert_eq!(p.title, "The QWERTY layout");
        assert_eq!(p.sections.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["Origins", "Why it stayed", "Rivals"]);
        assert_eq!(p.args()["research"], json!(true));
        assert_eq!(parse_plan("{\"research\": false}", "thanks!"), Planned::NotResearch);
        // No JSON: a plan from the question, its asking left out.
        let Planned::Plan(f) = parse_plan("Sure, I'll look into it.", "Can you research the history of the QWERTY keyboard layout?") else { panic!() };
        assert_eq!(f.title, "The history of the QWERTY keyboard layout");
        assert_eq!(f.sections.len(), 3);
        // A title but no sections: the title kept, the general parts.
        let Planned::Plan(t) = parse_plan("{\"title\": \"Typewriters\"}", "research typewriters") else { panic!() };
        assert_eq!((t.title.as_str(), t.sections.len()), ("Typewriters", 3));
        // At most MAX_PARTS.
        let many: Vec<String> = (0..12).map(|i| format!("{{\"key\": \"Part {i}\"}}")).collect();
        let Planned::Plan(m) = parse_plan(&format!("{{\"title\": \"X\", \"sections\": [{}]}}", many.join(",")), "x") else { panic!() };
        assert_eq!(m.sections.len(), MAX_PARTS);
    }

    #[test]
    fn the_loop_is_told_the_plan_the_caps_and_how_to_cite() {
        let plan = Plan { title: "QWERTY".into(), summary: "Its history.".into(), sections: vec![Section { key: "Origins".into(), intent: "the typewriter".into() }], note: "Keep it short".into() };
        let yaps = instructions(&plan, &Caps::for_request(Some(200_000), 3_000), false, None);
        assert!(yaps.contains("1. Origins — the typewriter"), "{yaps}");
        // No limit on searches; the tool loop's pages, what its context holds.
        assert!(yaps.contains("There's no limit on searches or pages") && yaps.contains("several tool calls in one step"), "{yaps}");
        assert!(yaps.contains("You can read up to 33 pages in all: what your context holds."), "{yaps}");
        assert!(!yaps.contains("minutes") && !yaps.contains("up to 20 searches"), "{yaps}");
        assert!(yaps.contains("like [3]") && yaps.contains("\"Keep it short\""), "{yaps}");
        let code = instructions(&plan, &Caps::for_request(None, 8_000), true, None);
        assert!(code.contains("WebFetch") && code.contains("[3](https://example.com/page)"), "{code}");
        assert!(code.contains("There's no limit on searches or pages") && !code.contains("You can read up to"), "{code}");
        // The lead after its helpers: their findings, then the report from them,
        // more searching only where a part is thin.
        let block = "Your research helpers have researched the parts of this plan at the same time…\n\n<findings part=\"1\" heading=\"Origins\">\nSholes [1].\n</findings>";
        let lead = instructions(&plan, &Caps::for_request(None, 8_000), true, Some(block));
        assert!(lead.contains("1. Origins — the typewriter") && lead.contains("\"Keep it short\""), "{lead}");
        assert!(lead.contains(block), "{lead}");
        assert!(lead.contains("You lead this research: write the report from your helpers' findings. There's no limit on searches or pages"), "{lead}");
        assert!(lead.contains("Otherwise write the report now, without searching again"), "{lead}");
        assert!(lead.contains("Cite only pages read — your helpers' (their notes' numbers) or your own with WebFetch"), "{lead}");
        assert!(lead.contains("[3](https://example.com/page)") && lead.contains("## Summary"), "{lead}");
        assert!(!lead.contains("Give every part of the plan searches of its own"), "the helpers did: {lead}");
        // A helper's budget: the research's caps and clock, its words its findings'.
        let research = Budget::start(Caps::USUAL);
        let helper = research.for_helper();
        assert!(helper.helper && helper.caps == Caps::USUAL);
        assert!(helper.line(Spent::Time).contains("write your findings now") && research.line(Spent::Time).contains("write the report now"));
        assert_eq!((helper.search_first(), helper.write_now()), (helpers::SEARCH_FIRST, helpers::WRITE_NOW));
        assert_eq!((research.search_first(), research.write_now()), (SEARCH_FIRST, WRITE_NOW));
        // The lead after them: the research's clock while it has 10 minutes left, else 10 from now.
        assert!(!helper.for_lead().helper && helper.for_lead().caps.time == TIME);
        let late = Budget::started_ago(Caps::USUAL, TIME - Duration::from_secs(60)).for_helper().for_lead();
        assert_eq!(late.caps.time, helpers::LEAD_AT_LEAST);
        assert!(late.elapsed() < Duration::from_secs(5) && !late.helper);
        // Only the web's tools and the notes' (and a big project's) reads run.
        assert!(allows("web_search") && allows("read_page") && allows("search_notes"));
        assert!(allows("read_project_file") && allows("search_project_files"));
        for no in ["open_app", "copy_to_clipboard", "remember", "forget", "create_note", "create_document", "start_agent", "browser_click"] {
            assert!(!allows(no), "{no}");
            assert_eq!(refusal(no)["success"], json!(false));
        }
    }

    #[test]
    fn the_reports_citations_point_only_at_pages_read_numbered_as_first_cited() {
        let sources = vec![
            src(1, "https://en.wikipedia.org/wiki/QWERTY", true),
            src(2, "https://search-result.example/only-seen", false),
            src(3, "https://typewriters.example/sholes", true),
            src(4, "https://museum.example/remington", true),
            src(5, "https://another.example/unread", false),
        ];
        let written = "# The QWERTY layout\n\n## Summary\n\nQWERTY came from Sholes's typewriter [3] and spread with the Remington No. 2 [4][2].\n\n## Origins\n\nSholes patented it in 1878 [3, 1]. Some say it slowed typists [2]. See also [the museum](https://museum.example/remington).\n\n## Why it stayed\n\nTraining and habit [1–4] kept it [9].\n\n## Sources\n\n1. Wikipedia\n2. A result\n";
        let r = report(written, &sources, "Plan title", "Researched 16 November 2026 · 5 searches, 3 pages read");
        assert_eq!(r.title, "The QWERTY layout");
        // Read pages only, renumbered 1, 2, 3 as first cited: 3 → 1, 4 → 2, 1 → 3.
        assert_eq!(r.sources.iter().filter(|s| s.cited).map(|s| (s.n, s.url.as_str())).collect::<Vec<_>>(), [(1, "https://typewriters.example/sholes"), (2, "https://museum.example/remington"), (3, "https://en.wikipedia.org/wiki/QWERTY")]);
        assert_eq!(r.cited, 3);
        // [2] twice, [2] in the range, [9]: taken out.
        assert_eq!(r.dropped, 4);
        assert!(r.body.contains("Sholes patented it in 1878 [[1]](https://typewriters.example/sholes)[[3]](https://en.wikipedia.org/wiki/QWERTY)."), "{}", r.body);
        assert!(r.body.contains("Some say it slowed typists."), "{}", r.body);
        assert!(r.body.contains("Training and habit [[3]](https://en.wikipedia.org/wiki/QWERTY)[[1]](https://typewriters.example/sholes)[[2]](https://museum.example/remington) kept it."), "{}", r.body);
        // A link of the model's own stays.
        assert!(r.body.contains("[the museum](https://museum.example/remington)"));
        // Its own sources list went; Yap's is the cited pages, numbered.
        assert_eq!(r.body.matches("## Sources").count(), 1);
        assert!(r.body.contains("\n1. [Page 3](https://typewriters.example/sholes) · typewriters.example\n2. [Page 4](https://museum.example/remington)"), "{}", r.body);
        assert!(r.body.starts_with("# The QWERTY layout\n\n*Researched 16 November 2026 · 5 searches, 3 pages read*\n"), "{}", r.body);
        // The thread: the summary with the same numbers as pills, then where the report is.
        assert_eq!(r.answer, "QWERTY came from Sholes's typewriter [1] and spread with the Remington No. 2 [2].\n\nThe full report is open on the right.");
        // Its sources: the pages read, the cited first; never a result nobody opened.
        assert_eq!(r.sources.len(), 3);
        assert!(r.sources.iter().all(|s| s.opened));
        let one_more = report(written, &[sources.clone(), vec![src(6, "https://read.example/uncited", true)]].concat(), "", "");
        assert_eq!(one_more.sources.iter().map(|s| (s.n, s.cited)).collect::<Vec<_>>(), [(1, true), (2, true), (3, true), (4, false)]);
        // Every citation number in the document is a page read.
        for n in crate::web::citations(&r.answer) {
            assert!(r.sources.iter().any(|s| s.n == n && s.opened && s.cited), "{n}");
        }
    }

    #[test]
    fn a_report_with_no_title_or_summary_takes_the_plans_and_its_first_paragraph() {
        let sources = vec![src(7, "https://a.example/page", true)];
        let r = report("Typewriters came first [7].\n\n## More\n\nAnd more.", &sources, "Typewriters", "");
        assert_eq!(r.title, "Typewriters");
        assert_eq!(r.answer, "Typewriters came first [1].\n\nThe full report is open on the right.");
        assert!(r.body.starts_with("# Typewriters\n\nTypewriters came first [[1]](https://a.example/page)."), "{}", r.body);
        // Nothing read: no citations, no sources list, the answer says where it is.
        let r = report("# T\n\nA claim [1].", &[src(1, "https://b.example/", false)], "", "");
        assert_eq!((r.cited, r.dropped), (0, 1));
        assert!(!r.body.contains("## Sources"));
        assert_eq!(r.answer, "A claim.\n\nThe full report is open on the right.");
        let r = report("", &[], "", "");
        assert_eq!(r.answer, "The report, “Research”, is open on the right.");
    }

    #[test]
    fn a_word_to_the_person_before_the_reports_title_goes_and_its_title_is_the_reports() {
        // Opus on the plan (16 Nov 2026): a line to the person, then the report.
        let sources = vec![src(1, "https://a.example/mayfly", true)];
        let written = "I have enough sources now, so here's the report.\n\n# The Short Life of Adult Mayflies\n\n## Summary\n\nAdults live a day or two [1].\n\n## Lifespan\n\nMost live hours [1].";
        let r = report(written, &sources, "How long do adult mayflies live", "Researched 16 November 2026 · 2 searches, 1 page read");
        assert_eq!(r.title, "The Short Life of Adult Mayflies");
        assert!(!r.body.contains("enough sources"), "{}", r.body);
        assert_eq!(r.body.lines().filter(|l| l.starts_with("# ")).count(), 1, "one title: {}", r.body);
        assert!(r.body.starts_with("# The Short Life of Adult Mayflies\n"), "{}", r.body);
        assert_eq!(r.answer, "Adults live a day or two [1].\n\nThe full report is open on the right.");
        // A part of the report before a # heading (a heading of its own, or more than a word or two): kept; the
        // title is the plan's.
        let r = report("## Background\n\nMayflies are insects [1].\n\n# Lifespan\n\nHours [1].", &sources, "Mayflies", "");
        assert_eq!(r.title, "Mayflies");
        assert!(r.body.contains("## Background") && r.body.contains("# Lifespan"), "{}", r.body);
        let long = "One.\nTwo.\nThree.\nFour.\n\n# Lifespan\n\nHours [1].";
        let r = report(long, &sources, "Mayflies", "");
        assert_eq!(r.title, "Mayflies");
        assert!(r.body.contains("Four.") && r.body.contains("# Lifespan"), "{}", r.body);
    }

    #[test]
    fn progress_and_the_chip_say_what_the_research_did() {
        let chips = vec!["Searched the web: \"qwerty\"".to_string(), "Read en.wikipedia.org/wiki/QWERTY".to_string(), "Searched the web: \"sholes\" (Wikipedia)".to_string()];
        let sources = vec![src(1, "https://en.wikipedia.org/wiki/QWERTY", true), src(2, "https://b.example/", false)];
        assert_eq!(counts(&chips, &sources), (2, 1));
        assert_eq!(progress(2, 1), "2 searches, 1 page read");
        assert_eq!(progress(1, 0), "1 search, 0 pages read");
        assert_eq!(done_chip(12, 8, Duration::from_secs(6 * 60 + 10), 0), "Researched for 6 min: 12 searches, 8 pages read");
        assert_eq!(done_chip(1, 1, Duration::from_secs(20), 0), "Researched for under a minute: 1 search, 1 page read");
        assert_eq!(done_chip(14, 20, Duration::from_secs(3 * 60), 4), "Researched for 3 min with 4 helpers: 14 searches, 20 pages read");
        assert_eq!(byline("16 November 2026", 3, 2), "Researched 16 November 2026 · 3 searches, 2 pages read");
    }

    #[test]
    fn the_quiet_stream_shows_progress_and_never_the_reports_words() {
        #[derive(Default)]
        struct Log(Mutex<Vec<Event>>);
        impl Sink for Log {
            fn send(&self, ev: Event) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let log = Log::default();
        let q = Quiet::after(&log, 0, 0);
        q.send(Event::Chips { chips: vec!["Searched the web: \"qwerty\"".into()], sources: vec![src(1, "https://a.example/", true)], web: true });
        q.send(Event::Step { step: crate::chat::activity::of_call("web_search", &json!({ "query": "sholes typewriter" })) });
        // The search came back: the line again, counted; the same counts again: nothing new.
        let two = vec!["Searched the web: \"qwerty\"".to_string(), "Searched the web: \"sholes typewriter\"".to_string()];
        q.send(Event::Chips { chips: two.clone(), sources: vec![src(1, "https://a.example/", true)], web: true });
        q.send(Event::Chips { chips: two, sources: vec![src(1, "https://a.example/", true)], web: true });
        q.send(Event::Text { text: "# The report…".into() });
        q.send(Event::Text { text: "# The report… more".into() });
        let events = log.0.lock().unwrap();
        let lines: Vec<String> = events.iter().filter_map(|e| if let Event::Step { step } = e { Some(step.line.clone()) } else { None }).collect();
        assert_eq!(
            lines,
            ["Searching the web · sholes typewriter · 1 search, 1 page read", "Searching the web · sholes typewriter · 2 searches, 1 page read", "Writing the report · 2 searches, 1 page read"]
        );
        assert!(!events.iter().any(|e| matches!(e, Event::Text { .. })));
    }
}
