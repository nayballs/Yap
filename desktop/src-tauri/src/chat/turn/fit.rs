//! **Yap's tools fitted to a small context** (8 Nov 2026): a turn whose
//! fixed part — the stable prompt and the tools' schemas, sent with every
//! step — leaves too little room for the question and a little history
//! goes without some families of tools, for that turn only, so a short
//! question on an 8K model is never refused because of Yap's own tools.
//!
//! On 8 Nov 2026 a plain chat on a self-hosted server that doesn't say its
//! context (Yap assumes 8K), with the web and Yap Code on, measured ~6,470
//! tokens of prompt and schemas against the 6,272 an 8K budget has for them
//! and the history together: "hi" was "too long for the model". The browser
//! toolset (`chat/pages/`) and Yap Code's agent tools (`chat/tools/agents.rs`)
//! took the last of it.
//!
//! - **Which families go** is one pure decision ([`choose`]): none while
//!   the room is there ([`Room::least`], or the question's own size when
//!   it's longer); else the fewest from the front of [`ORDER`] — what costs
//!   most and matters least to a small model first — whose measured costs
//!   ([`costs`]) leave it. What the question plainly asks for stays
//!   ([`keeps`]).
//! - **A family goes whole**: its tools and its lines in the prompt
//!   ([`Family::off`] turns its gate off, and the prompt and the tools both
//!   follow the gates), so the prompt never names a tool that isn't offered.
//! - **One measure**: what [`fit`] counts is what the turn sends — the
//!   question's check (`history.rs` `Sizing::measure`), Chat's limits
//!   (`chat_limits`) and its ring (`usage/context.rs`) all come here, and
//!   the turn goes on with the gates it returns.
//! - A model with the room (Claude, GPT-5, Gemini, the on-device model at
//!   a context with room) gets everything as before, byte for byte.
//! - Still too long with every family gone: the question itself is, and
//!   it's refused as before (`budget::too_long`).

use serde_json::Value;

use super::gates::Gates;
use crate::chat::pictures::Gate;

/// **A family of tools a turn may go without**: its tools and its lines in
/// the system prompt, together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// The browser's acting tools (`browser_click`, `browser_type`,
    /// `browser_key`, `browser_hover`, `browser_form_input`, the batch) and
    /// their line (`pages/act.rs`).
    Acting,
    /// Seeing (a model that reads images): the browser's view tools
    /// (`browser_scroll`, `browser_scroll_to`, `browser_screenshot`,
    /// `browser_zoom`) and `look_at_picture`, the pictures' gate from Look to
    /// Show — each picture looked at costs ~800 tokens more at 8K.
    Seeing,
    /// Yap Code's agent tools and lines (never in a Code chat: [`keeps`]).
    Agents,
    /// The connectors' tools and line (`tools/connectors.rs`).
    Connectors,
    /// The pictures' own tools (`find_pictures`, `look_at_picture`,
    /// `show_picture`) and lines; the pages beside the chat stay.
    Pictures,
    /// The documents' rich set (`documents/rich.rs`) for the simple one,
    /// which writes and changes documents too.
    RichDocuments,
    /// `look_at_screen` and its line (`see_screen.rs`).
    Screen,
    /// `check_calendar` and its lines.
    Calendar,
    /// `search_chats` and its line.
    PastChats,
    /// The browser beside the chat (`browser_navigate`, `browser_read`,
    /// `browser_find`, `browser_tabs`, `browser_wait`), its line and the tabs
    /// told with the question; the web's own `web_search` and `read_page`
    /// stay.
    Browser,
    /// `open_app`, `list_apps` and their lines (Yap opens a plainly asked-for
    /// app itself, before any model: `apps::plan_for`).
    Apps,
}

/// **The order families go in** — what costs most and matters least to a
/// small model first (8 Nov 2026, measured with `budget::estimate`, schemas
/// and lines together; see the `costs` tests):
///
/// 1. acting, ~580 tokens (the on-device Qwen) — a small model acts on pages
///    least well, and only a typed question about an open page needs it;
/// 2. seeing, ~400 (a model that reads images), and every picture it looks
///    at ~800 more;
/// 3. agents, ~1,560 in a plain chat — the most of any, and a Code chat
///    (Chat's Code side) is where they're the point;
/// 4. connectors, up to a tenth of the context (strong cloud models only);
/// 5. pictures, ~300;
/// 6. the documents' rich set for the simple one, ~800 (strong models);
/// 7. the screen, ~150;
/// 8. the calendar, ~250;
/// 9. past chats, ~110;
/// 10. the browser beside the chat, ~550 (the web's search and page reads
///     stay);
/// 11. apps, ~280 — last, after the browser: its line says websites open
///     there "never with open_app" ([`Family::waits`]).
///
/// Never: the notes and documents' tools, the web's, memory's `remember` /
/// `forget` — what Chat is for.
pub(super) const ORDER: [Family; 11] = [
    Family::Acting,
    Family::Seeing,
    Family::Agents,
    Family::Connectors,
    Family::Pictures,
    Family::RichDocuments,
    Family::Screen,
    Family::Calendar,
    Family::PastChats,
    Family::Browser,
    Family::Apps,
];

impl Family {
    /// Its name in the log.
    pub(super) fn name(self) -> &'static str {
        match self {
            Family::Acting => "acting",
            Family::Seeing => "seeing",
            Family::Agents => "agents",
            Family::Connectors => "connectors",
            Family::Pictures => "pictures",
            Family::RichDocuments => "rich_documents",
            Family::Screen => "screen",
            Family::Calendar => "calendar",
            Family::PastChats => "past_chats",
            Family::Browser => "browser",
            Family::Apps => "apps",
        }
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }

    /// **`g` without this family**: its gate off, so the prompt's lines and
    /// the tools offered both leave it out. Pure.
    pub(super) fn off(self, g: Gates) -> Gates {
        match self {
            Family::Acting => Gates { act: false, ..g },
            Family::Seeing if g.pictures == Gate::Look => Gates { pictures: Gate::Show, ..g },
            Family::Seeing => g,
            Family::Agents => Gates { agents: false, agent_line: false, ..g },
            Family::Connectors => Gates { connectors: false, ..g },
            Family::Pictures => Gates { picture_tools: false, ..g },
            Family::RichDocuments => Gates { doc_tools: crate::chat::documents::ToolSet::Simple, ..g },
            Family::Screen => Gates { screen: Gate::Off, ..g },
            Family::Calendar => Gates { calendar: false, ..g },
            Family::PastChats => Gates { past_chats: false, ..g },
            Family::Browser => Gates { pictures: Gate::Off, act: false, ..g },
            Family::Apps => Gates { apps: false, ..g },
        }
    }

    /// It can't go while `g` keeps another that names its tools: apps while
    /// the browser stays (its line: "Websites open here, never with
    /// open_app"). Pure.
    fn waits(self, g: &Gates) -> bool {
        self == Family::Apps && g.pictures != Gate::Off
    }
}

/// **The families a turn goes without** (or keeps: [`keeps`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Shed(u16);

impl Shed {
    pub(super) fn has(self, f: Family) -> bool {
        self.0 & f.bit() != 0
    }

    pub(super) fn with(self, f: Family) -> Shed {
        Shed(self.0 | f.bit())
    }

    pub(super) fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Their names, in [`ORDER`] (the log).
    pub(super) fn names(self) -> Vec<&'static str> {
        ORDER.iter().filter(|f| self.has(**f)).map(|f| f.name()).collect()
    }

    /// `g` without these families. Pure.
    pub(super) fn apply(self, g: Gates) -> Gates {
        ORDER.iter().filter(|f| self.has(**f)).fold(g, |g, f| f.off(g))
    }
}

/// The words that ask for one of Yap Code's agents, in Yap's languages.
const AGENT_WORDS: &[&str] = &["agent", "agents", "agente", "agentes", "agenten", "agenti"];

/// Do `text`'s words ask about an agent — "agent" said, or one of the
/// `workspaces` named (`agents::consent::names`: an agent in a plain chat
/// needs its folder named)? Pure.
fn about_agents(text: &str, workspaces: &[crate::agents::workspaces::Folder]) -> bool {
    crate::agents::workspaces::words(text).iter().any(|w| AGENT_WORDS.contains(&w.as_str())) || workspaces.iter().any(|f| crate::agents::consent::names(text, f))
}

/// **What the question plainly asks for**, kept whatever the room (cheap
/// checks on its words; pure): the agents in a Code chat (`code`), or for a
/// question about an agent or naming one of the `workspaces` — or a reply to
/// an answer (`previous`) that asked about one —, the calendar for a
/// question about it, past chats for one pointing back at one, the browser —
/// acting and seeing too — for one about the page open beside the chat, the
/// screen for one about it.
pub(super) fn keeps(question: &str, previous: Option<&str>, code: bool, workspaces: &[crate::agents::workspaces::Folder]) -> Shed {
    let mut keep = Shed::default();
    if code || about_agents(question, workspaces) || previous.is_some_and(|p| p.contains('?') && about_agents(p, workspaces)) {
        keep = keep.with(Family::Agents);
    }
    if crate::chat::tools::calendar::asks_about_calendar(question) {
        keep = keep.with(Family::Calendar);
    }
    if crate::chat::chats::refers_to_past(question) {
        keep = keep.with(Family::PastChats);
    }
    if crate::chat::pages::about_open_page(question) {
        keep = keep.with(Family::Browser).with(Family::Acting).with(Family::Seeing);
    }
    if crate::chat::assistant::screen::about_in(question, &[]) == crate::chat::assistant::screen::About::Yes {
        keep = keep.with(Family::Screen);
    }
    keep
}

/// **The room a turn's requests have** for a fixed part of so many tokens
/// (`budget::budget`, as the turn's check and history use it).
#[derive(Debug, Clone, Copy)]
pub(super) struct Room {
    pub(super) context: u64,
    pub(super) known: bool,
    pub(super) kind: crate::chat::budget::Kind,
    pub(super) tools: bool,
    /// The request's cap (`budget::rate_cap` for the question's check).
    pub(super) cap: Option<u64>,
}

impl Room {
    /// The history's and the question's room beside `fixed` tokens. Pure.
    pub(super) fn history(&self, fixed: u64) -> u64 {
        crate::chat::budget::budget(self.context, self.known, fixed, self.kind, self.tools, self.cap).history
    }

    /// **The least room a turn keeps** for the question and a little
    /// history: an eighth of the request, 512 to 4,096 tokens (896 at 8K,
    /// a short question and an exchange or two). Pure.
    pub(super) fn least(&self) -> u64 {
        (crate::chat::budget::budget(self.context, self.known, 0, self.kind, self.tools, self.cap).request / 8).clamp(512, 4_096)
    }
}

/// **Each family's cost** in [`ORDER`], as it goes after the ones before it
/// (`estimate`: a fixed part's tokens for some gates): what the prompt and
/// the schemas lose without it. Not the ones `keep` keeps, nor any the
/// gates don't offer (no cost), nor one that must wait for another kept
/// ([`Family::waits`]). Pure.
pub(super) fn costs(g: Gates, keep: Shed, estimate: impl Fn(&Gates) -> u64) -> Vec<(Family, u64)> {
    let mut out = Vec::new();
    let (mut at, mut tokens) = (g, estimate(&g));
    for f in ORDER {
        if keep.has(f) || f.waits(&at) {
            continue;
        }
        let next = f.off(at);
        let left = estimate(&next);
        if left < tokens {
            out.push((f, tokens - left));
            (at, tokens) = (next, left);
        }
    }
    out
}

/// **Which families go** (the one decision; pure): none while `fixed`
/// tokens leave `want` of room (`room`: the room beside a fixed part);
/// else the fewest from the front of `costs` (in [`ORDER`]) that leave it —
/// all of them when even that doesn't.
pub(super) fn choose(costs: &[(Family, u64)], fixed: u64, want: u64, room: impl Fn(u64) -> u64) -> Shed {
    let mut shed = Shed::default();
    let mut left = fixed;
    for &(f, cost) in costs {
        if room(left) >= want {
            break;
        }
        shed = shed.with(f);
        left = left.saturating_sub(cost);
    }
    shed
}

/// **What a turn's fixed part is made from**, besides its gates: the
/// persona, About you and the clock, whether it's a Code chat, the turn's
/// connectors and the conversation asking.
pub(super) struct Parts<'a> {
    pub(super) persona: &'a str,
    pub(super) profile: &'a crate::ai::about_you::Profile,
    pub(super) now: chrono::DateTime<chrono::Utc>,
    pub(super) code: bool,
    pub(super) connectors: Option<&'a crate::chat::tools::connectors::Turn>,
    pub(super) conversation: Option<u64>,
}

impl Parts<'_> {
    /// The stable prompt and the tools offered for `g`, as the turn writes
    /// and offers them (`Gates::stable_prompt`, `Gates::offered`).
    fn texts(&self, g: &Gates) -> (String, Value) {
        (g.stable_prompt(self.persona, self.profile, self.now, self.code), g.offered(self.connectors, self.conversation, self.code))
    }

    /// `g`'s fixed part, estimated (the costs).
    fn estimate(&self, g: &Gates) -> u64 {
        let (stable, tools) = self.texts(g);
        crate::chat::budget::estimate(&stable) + if tools.is_null() { 0 } else { crate::chat::budget::estimate(&crate::chat::budget::loaded_tools(&tools).to_string()) }
    }

    /// `g`'s fixed part as `counter` counts it: the stable prompt, and the
    /// tools' schemas (deferred tools cost nothing until found).
    async fn count(&self, g: &Gates, counter: &crate::chat::budget::Counter) -> (u64, u64) {
        let (stable, tools) = self.texts(g);
        let schemas = if tools.is_null() { 0 } else { counter.count(&crate::chat::budget::loaded_tools(&tools).to_string()).await };
        (counter.count(&stable).await, schemas)
    }
}

/// **A turn's tools, fitted** ([`fit`]): the gates it goes on with, the
/// families left out, and its fixed part as counted.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fitted {
    pub(super) gates: Gates,
    pub(super) shed: Shed,
    /// The stable prompt's tokens.
    pub(super) stable: u64,
    /// The tools' schemas' tokens.
    pub(super) schemas: u64,
}

impl Fitted {
    /// The fixed part: the stable prompt and the schemas.
    pub(super) fn fixed(&self) -> u64 {
        self.stable + self.schemas
    }
}

/// The gates `g` as they are, their fixed part counted (a model evaluation
/// sending the last messages by count: no budget to fit).
pub(super) async fn as_is(g: Gates, parts: &Parts<'_>, counter: &crate::chat::budget::Counter) -> Fitted {
    let (stable, schemas) = parts.count(&g, counter).await;
    Fitted { gates: g, shed: Shed::default(), stable, schemas }
}

/// **The gates `g` fitted to `room`** for a question of `question` tokens:
/// as they are while the room is there (counted once: a model with room
/// pays nothing more); else without the families [`choose`] picks (those
/// `keep` says the turn keeps stay: [`keeps`], read only then), counted
/// again, and one more at a time while the count still leaves too little —
/// the estimates and a server's own tokenizer may differ.
pub(super) async fn fit(g: Gates, parts: &Parts<'_>, counter: &crate::chat::budget::Counter, room: Room, question: u64, keep: impl FnOnce() -> Shed) -> Fitted {
    let whole = as_is(g, parts, counter).await;
    let want = room.least().max(question);
    if !g.tools || room.history(whole.fixed()) >= want {
        return whole;
    }
    let keep = keep();
    let costs = costs(g, keep, |g| parts.estimate(g));
    let mut shed = choose(&costs, whole.fixed(), want, |fixed| room.history(fixed));
    let mut fitted = whole;
    while !shed.is_empty() {
        let gates = shed.apply(g);
        let (stable, schemas) = parts.count(&gates, counter).await;
        fitted = Fitted { gates, shed, stable, schemas };
        if room.history(fitted.fixed()) >= want {
            break;
        }
        match costs.iter().map(|(f, _)| *f).find(|f| !shed.has(*f)) {
            Some(f) => shed = shed.with(f),
            None => break,
        }
    }
    tracing::debug!(
        shed = ?fitted.shed.names(),
        kept = ?keep.names(),
        before = whole.fixed(),
        after = fitted.fixed(),
        want,
        room = room.history(fitted.fixed()),
        context = room.context,
        "Chat: some of Yap's tools left out of this turn to fit the model's context"
    );
    fitted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::budget::{estimate, Counter, Kind};
    use crate::chat::documents::ToolSet;
    use crate::chat::tools::WebLines;

    /// A self-hosted model that calls tools but neither sees nor acts (the
    /// e2e's fake, a small server), with the web and Yap Code on.
    fn small_model() -> Gates {
        Gates {
            tools: true,
            doc_tools: ToolSet::Simple,
            strong: false,
            claude_in_chat: false,
            own_search: false,
            web_lines: WebLines::of(false, false, false),
            web_on: true,
            pictures: Gate::Show,
            screen: Gate::Show,
            act: false,
            calendar: false,
            agents: true,
            connectors: false,
            apps: true,
            past_chats: true,
            picture_tools: true,
            agent_line: true,
        }
    }

    /// A strong model that sees and acts, with everything on.
    fn strong_model() -> Gates {
        Gates {
            doc_tools: ToolSet::Rich,
            strong: true,
            web_lines: WebLines::of(true, false, false),
            pictures: Gate::Look,
            screen: Gate::Look,
            act: true,
            calendar: true,
            ..small_model()
        }
    }

    fn persona() -> String {
        let m: crate::chat::chats::ChatModel = serde_json::from_value(serde_json::json!({ "provider": "custom", "id": "fake-chat", "onThisPc": true })).unwrap();
        crate::ai::ai_model::persona_with_answering("You are a helpful assistant. Answer concisely.", &m)
    }

    fn room(context: u64) -> Room {
        Room { context, known: false, kind: Kind::Chat, tools: true, cap: None }
    }

    /// The fixed part's tokens and the room beside it, and every name of a
    /// tool offered, for `g`.
    struct Seen {
        prompt: String,
        tools: Value,
        names: Vec<String>,
    }

    fn seen(g: &Gates, parts: &Parts<'_>) -> Seen {
        let (prompt, tools) = parts.texts(g);
        let names = tools.as_array().map(|a| a.iter().filter_map(|t| t["function"]["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
        Seen { prompt, tools, names }
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    /// `name` as a word of `text` (not part of a longer name).
    fn names(text: &str, name: &str) -> bool {
        text.match_indices(name).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }

    #[test]
    fn an_8k_model_with_the_web_and_yap_code_now_answers_a_short_question() {
        // 8 Nov 2026: ~6,470 tokens of prompt and schemas against the 6,272
        // an 8K model's budget has for them and the history: "hi" refused.
        let profile = crate::ai::about_you::Profile::default();
        let persona = persona();
        let parts = Parts { persona: &persona, profile: &profile, now: chrono::Utc::now(), code: false, connectors: None, conversation: Some(7) };
        let (counter, at_8k) = (Counter::estimate_only(), room(8_192));
        let hi = estimate("hi") + crate::chat::budget::MESSAGE_OVERHEAD;
        let whole = run(as_is(small_model(), &parts, &counter));
        assert!(at_8k.history(whole.fixed()) < hi, "no room for a short question before: {} of {}", at_8k.history(whole.fixed()), whole.fixed());
        let fitted = run(fit(small_model(), &parts, &counter, at_8k, hi, || keeps("hi", None, false, &[])));
        // Room for the question and a little history, Yap Code's agents
        // left out (the most any family costs, in a plain chat)…
        assert!(at_8k.history(fitted.fixed()) >= at_8k.least(), "room {} (fixed {})", at_8k.history(fitted.fixed()), fitted.fixed());
        assert_eq!(fitted.shed.names(), ["agents"]);
        assert!(!fitted.gates.agents && !fitted.gates.agent_line);
        // …and only them: the web, the browser, pictures, apps, memory stay.
        let after = seen(&fitted.gates, &parts);
        for kept in ["web_search", "read_page", "browser_navigate", "browser_read", "show_picture", "look_at_screen", "open_app", "remember", "search_chats", "search_notes"] {
            assert!(after.names.iter().any(|n| n == kept), "{kept}: {:?}", after.names);
        }
        for gone in crate::chat::tools::agents::NAMES {
            assert!(!after.names.iter().any(|n| n == gone) && !names(&after.prompt, gone), "{gone}");
        }
        // What's measured is what the turn sends.
        assert_eq!(fitted.stable, estimate(&after.prompt));
        assert_eq!(fitted.schemas, estimate(&crate::chat::budget::loaded_tools(&after.tools).to_string()));
    }

    #[test]
    fn a_model_with_room_gets_every_tool_and_the_prompt_as_before() {
        let profile = crate::ai::about_you::Profile::default();
        let persona = persona();
        let parts = Parts { persona: &persona, profile: &profile, now: chrono::Utc::now(), code: false, connectors: None, conversation: Some(7) };
        let counter = Counter::estimate_only();
        // Claude's 200K, GPT-5's 128K (also on a plan's tokens a minute
        // that leaves room), Gemini's 1M, the on-device model at 32K.
        for (g, context, cap) in [
            (strong_model(), 200_000, None),
            (strong_model(), 128_000, Some(400_000)),
            (strong_model(), 1_048_576, None),
            (Gates { act: true, ..small_model() }, 32_768, None),
        ] {
            let at = Room { cap, ..room(context) };
            let fitted = run(fit(g, &parts, &counter, at, 40, || keeps("What's on at the cinema tonight?", None, false, &[])));
            assert!(fitted.shed.is_empty(), "{context}: {:?}", fitted.shed.names());
            assert_eq!(fitted.gates, g, "{context}");
            // Byte for byte the prompt and the tools these gates always gave.
            let (before, after) = (seen(&g, &parts), seen(&fitted.gates, &parts));
            assert_eq!(before.prompt, after.prompt);
            assert_eq!(before.tools, after.tools);
        }
    }

    #[test]
    fn as_the_gates_always_were_the_prompt_and_the_tools_are_the_ones_before_fitting() {
        // Unfitted gates (`Gates::for_turn`'s) give what Chat sent before
        // 8 Nov 2026: the tools as `offered_tools` had them from the turn's
        // switches, and the lines as the switches said.
        let cfg = crate::config::YapConfig { open_apps: true, memory_search_chats: true, ..Default::default() };
        let g = Gates { apps: cfg.open_apps, past_chats: cfg.memory_search_chats, ..strong_model() };
        let log = crate::chat::tools::calendar::Log::default();
        for code in [false, true] {
            let before = crate::chat::tools::Extra {
                memory: !code,
                search_chats: !code && cfg.memory_search_chats,
                conversation: Some(7),
                calendar: g.calendar.then_some(crate::chat::tools::CalendarTurn { log: &log, zone: crate::chat::tools::calendar::Zone::of(None), now: 0 }),
                doc_tools: g.doc_tools,
                strong: g.strong,
                pictures: g.pictures,
                screen: g.screen,
                act: g.act,
                own_search: g.own_search,
                agents: g.agents.then_some(crate::chat::tools::AgentsTurn::default()),
                ..Default::default()
            };
            assert_eq!(g.offered(None, Some(7), code), crate::chat::tools::offered_tools(cfg.open_apps, true, before), "code: {code}");
        }
        let profile = crate::ai::about_you::Profile::default();
        let prompt = g.stable_prompt("Persona.", &profile, chrono::Utc::now(), false);
        let lines = [
            crate::chat::tools::tool_instructions(ToolSet::Rich),
            crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" "),
            crate::chat::tools::web_tool_instructions(g.web_lines),
            crate::chat::pictures::instructions(Gate::Look, false),
            crate::chat::pages::instructions(Gate::Look, true),
            crate::chat::tools::MEMORY_TOOL_INSTRUCTIONS.join(" "),
            crate::chat::tools::CHATS_TOOL_INSTRUCTIONS.join(" "),
            crate::chat::tools::calendar::CALENDAR_TOOL_INSTRUCTIONS.join(" "),
            crate::chat::tools::agents::AGENT_TOOL_INSTRUCTIONS.join(" "),
            crate::chat::see_screen::instructions(Gate::Look).to_string(),
        ];
        let mut at = 0;
        for line in lines {
            let found = prompt[at..].find(&line).unwrap_or_else(|| panic!("in order: {line}"));
            at += found + line.len();
        }
    }

    #[test]
    fn a_code_chat_keeps_its_agents_and_goes_without_others() {
        let profile = crate::ai::about_you::Profile::default();
        let persona = persona();
        let parts = Parts { persona: &persona, profile: &profile, now: chrono::Utc::now(), code: true, connectors: None, conversation: Some(7) };
        let (counter, at_8k) = (Counter::estimate_only(), room(8_192));
        let question = estimate("Fix the failing test in the parser") + crate::chat::budget::MESSAGE_OVERHEAD;
        let fitted = run(fit(Gates { act: true, ..small_model() }, &parts, &counter, at_8k, question, || keeps("Fix the failing test in the parser", None, true, &[])));
        assert!(fitted.gates.agents && fitted.gates.agent_line, "{:?}", fitted.shed.names());
        assert!(!fitted.shed.has(Family::Agents) && !fitted.shed.is_empty());
        assert!(at_8k.history(fitted.fixed()) >= at_8k.least(), "room {}", at_8k.history(fitted.fixed()));
        let after = seen(&fitted.gates, &parts);
        assert!(after.names.iter().any(|n| n == "start_agent") && names(&after.prompt, "start_agent"));
        // The first to go, in order: acting, then pictures (seeing and the
        // rest it doesn't offer, or costs nothing).
        assert_eq!(&fitted.shed.names()[..2], ["acting", "pictures"]);
    }

    #[test]
    fn the_prompt_never_names_a_tool_that_isnt_offered() {
        let profile = crate::ai::about_you::Profile::default();
        let persona = persona();
        for code in [false, true] {
            let parts = Parts { persona: &persona, profile: &profile, now: chrono::Utc::now(), code, connectors: None, conversation: Some(7) };
            for g in [strong_model(), small_model(), Gates { act: true, ..small_model() }] {
                let all = seen(&g, &parts).names;
                // Each family gone after the ones before it, as a turn sheds them.
                let mut shed = Shed::default();
                for f in ORDER {
                    shed = shed.with(f);
                    let now = seen(&shed.apply(g), &parts);
                    let text = format!("{}\n{}", now.prompt, now.tools);
                    for gone in all.iter().filter(|n| !now.names.contains(n)) {
                        assert!(!names(&text, gone), "{} gone (code: {code}): {gone} still named", f.name());
                    }
                }
                // Each family alone, too, where nothing it waits on stays.
                for f in ORDER.into_iter().filter(|f| !f.waits(&g)) {
                    let now = seen(&f.off(g), &parts);
                    let text = format!("{}\n{}", now.prompt, now.tools);
                    for gone in all.iter().filter(|n| !now.names.contains(n)) {
                        assert!(!names(&text, gone), "{} alone (code: {code}): {gone} still named", f.name());
                    }
                }
            }
        }
    }

    #[test]
    fn what_the_question_asks_for_stays() {
        assert!(keeps("anything", None, true, &[]).has(Family::Agents));
        assert!(!keeps("anything", None, false, &[]).has(Family::Agents));
        // A plain chat's agents: asked for, a workspace named, or a yes to an
        // answer that asked about one.
        let garden = [crate::agents::workspaces::Folder { name: "garden-planner".into(), path: r"D:\Projects\garden-planner".into(), cap: None, added_ts: 1 }];
        assert!(keeps("How's the agent getting on?", None, false, &[]).has(Family::Agents));
        assert!(keeps("Fix the watering bug in garden planner", None, false, &garden).has(Family::Agents));
        assert!(keeps("Yes please", Some("Shall I send an agent to garden-planner to fix it?"), false, &garden).has(Family::Agents));
        assert!(!keeps("Yes please", Some("Here's the recipe."), false, &garden).has(Family::Agents));
        assert!(!keeps("What's a good tomato feed?", None, false, &garden).has(Family::Agents));
        assert!(keeps("Am I free at 3 on Friday?", None, false, &[]).has(Family::Calendar));
        assert!(keeps("What did we say about the offsite last time?", None, false, &[]).has(Family::PastChats));
        let page = keeps("Fill in the form on this page", None, false, &[]);
        assert!(page.has(Family::Browser) && page.has(Family::Acting) && page.has(Family::Seeing));
        assert!(keeps("What's on my screen?", None, false, &[]).has(Family::Screen));
        assert!(keeps("hi", None, false, &[]).is_empty());
        // Kept, it costs nothing to the decision; apps wait for the browser.
        let g = Gates { act: true, ..small_model() };
        let est = |g: &Gates| u64::from(g.act) * 500 + u64::from(g.agents) * 1_500 + u64::from(g.pictures != Gate::Off) * 550 + u64::from(g.apps) * 280;
        assert_eq!(costs(g, Shed::default(), est), [(Family::Acting, 500), (Family::Agents, 1_500), (Family::Browser, 550), (Family::Apps, 280)]);
        assert_eq!(costs(g, keeps("Click the first link on this page", None, false, &[]), est), [(Family::Agents, 1_500)]);
    }

    #[test]
    fn the_fewest_families_go_in_order() {
        let costs = [(Family::Acting, 500), (Family::Agents, 1_500), (Family::Pictures, 300), (Family::Browser, 550)];
        let room = |fixed: u64| 6_272u64.saturating_sub(fixed);
        // Room already: none.
        assert!(choose(&costs, 5_000, 900, room).is_empty());
        // Short by 600: acting, then agents (500 isn't enough alone).
        let shed = choose(&costs, 5_972, 900, room);
        assert_eq!(shed.names(), ["acting", "agents"]);
        // Short by more than they all cost: all of them.
        assert_eq!(choose(&costs, 9_000, 900, room).names(), ["acting", "agents", "pictures", "browser"]);
        // The question's own size counts when it's the bigger.
        assert_eq!(choose(&costs, 4_000, 2_500, room).names(), ["acting"]);
    }
}
