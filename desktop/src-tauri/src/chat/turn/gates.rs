//! What a turn's model may do: tools, the documents' set, the web and
//! whose search, pictures, the screen, acting on pages, the calendar —
//! settled once, before the prompt, from the settings, the model and where
//! the question was asked.

use super::{in_chat, Endpoint};
use crate::chat::pictures::Gate;
use crate::config::YapConfig;

/// **What a turn's model may do** ([`Gates::for_turn`]): the same for the
/// whole turn, so the system prompt's lines, the tools every step offers
/// and the budget that counts them agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Gates {
    /// It calls tools (`tools::supports_tools`). Its context then goes in
    /// the question's own message (`chats::with_context`), the system
    /// prompt kept the same from turn to turn.
    pub tools: bool,
    /// Which documents' tools (documents.rs): the rich set for the models
    /// that handle it, the simple one for the rest.
    pub doc_tools: crate::chat::documents::ToolSet,
    /// A strong model (documents/rich.rs `strong`: Claude, GPT-5, Gemini…):
    /// its notes and documents still change after the web, and a document
    /// it writes is researched deeper and thought through first.
    pub strong: bool,
    /// A strong Claude searches and reads pages itself, in Chat's own
    /// request (llm/anthropic.rs, web/native.rs `in_chat`): Anthropic's web
    /// tools in place of Yap's `web_search` and `read_page` — with "The AI's
    /// own search" on and Yap able to use it for this model.
    pub claude_in_chat: bool,
    /// Any other cloud model with "The AI's own search" on searches with its
    /// provider's own search, which reads the pages: Yap's `read_page`
    /// isn't offered (own tools only, the maintainer, 1 Nov 2026).
    pub own_search: bool,
    /// The web's lines in the system prompt (`tools::WebLines`).
    pub web_lines: crate::chat::tools::WebLines,
    /// "Look things up on the web" (web.rs). Not for the Meetings view's Ask
    /// bar: that's about the meetings.
    pub web_on: bool,
    /// Pictures from the web (chat/pictures.rs): looked at and shown by a
    /// model that reads images, shown by any other that calls tools.
    /// (Not for the Voice Assistant away from Chat, nor a phone: no panel
    /// there to see them in.)
    pub pictures: Gate,
    /// The screen (chat/see_screen.rs): with tools and the switch on, web or
    /// not — with Chat in view (a phone's question never captures the PC;
    /// the Voice Assistant from another app has its own look at the window
    /// in front).
    pub screen: Gate,
    /// Acting on the pages beside the chat (chat/pages/act.rs): the models
    /// proven good at it, in a typed Chat turn (never the Voice Assistant
    /// or the meetings: a click that buys asks in Chat's thread).
    pub act: bool,
    /// The calendar (chat/tools/calendar.rs): a tool while one is connected
    /// (not across the meetings, whose passages are numbered on their own).
    pub calendar: bool,
    /// Yap Code (chat/tools/agents.rs): with tools, agents on and Claude Code on
    /// this PC; not across the meetings (that's about the meetings).
    pub agents: bool,
    /// Connectors' tools (chat/tools/connectors.rs): the preview on, a strong
    /// cloud model, Chat or the Voice Assistant on this PC (never a phone's
    /// question in this wave), not across the meetings, and some connector's
    /// tool there to offer (during a meeting, only those allowed then).
    pub connectors: bool,
}

impl Gates {
    /// The gates for a question asked at `scope` (`None`: Chat; "meetings";
    /// "assistant") of `ep`'s model: `meetings`, asked across the meetings
    /// (the Meetings view, a Skill); `chat_in_view`, Chat itself or the
    /// Voice Assistant asked with Chat in view.
    pub(super) fn for_turn(cfg: &YapConfig, ep: &Endpoint, scope: Option<&str>, meetings: bool, chat_in_view: bool) -> Gates {
        let (provider, model) = (ep.provider.as_str(), ep.model.as_str());
        let tools = crate::chat::tools::supports_tools(provider, model);
        let doc_tools = crate::chat::documents::ToolSet::for_model(provider, model);
        let strong = tools && crate::chat::documents::rich::strong(provider, model);
        let claude_in_chat = in_chat(cfg, provider, model, &ep.base_url, &ep.api_key);
        let own_search = cfg.web_search && !claude_in_chat && crate::web::Settings::for_chat(cfg, provider, model, &ep.base_url, &ep.api_key).native.is_some();
        let web_lines = crate::chat::tools::WebLines::of(strong, claude_in_chat, own_search);
        let web_on = cfg.web_search && !meetings;
        let pictures = Gate::for_model(web_on && tools && (scope != Some("assistant") || chat_in_view), provider, model);
        let screen = if chat_in_view { crate::chat::see_screen::gate(cfg, tools, provider, model) } else { Gate::Off };
        let act = pictures != Gate::Off && scope.is_none() && !meetings && crate::chat::pages::act::can_act(provider, model);
        let calendar = !meetings && crate::calendar::connected();
        let agents = tools && !meetings && crate::agents::offered(cfg);
        // (An evaluation's connectors scenario gives any model that calls
        // tools the connectors, to measure which may: `eval::connectors_forced`.)
        let evaluated = tools && provider != crate::ai::claude_plan::PROVIDER && crate::ai::eval::connectors_forced();
        let connectors = !meetings && (evaluated || crate::chat::tools::connectors::may(provider, strong)) && crate::chat::tools::connectors::offers_any(cfg, scope);
        let gates = Gates { tools, doc_tools, strong, claude_in_chat, own_search, web_lines, web_on, pictures, screen, act, calendar, agents, connectors };
        if provider == crate::ai::claude_plan::PROVIDER {
            return gates.on_the_plan();
        }
        gates
    }

    /// **Your Claude plan** (chat/turn/plan.rs): it answers, with Claude
    /// Code's web search and page reads and Yap's read-only tools. No
    /// documents beside the chat: those stay Yap's tool loop's, which a plan
    /// turn never runs. The look at the screen, the browser beside the chat
    /// (acting on its pages too, since 8 Nov 2026, each buy, send or sign-in
    /// on the person's card) and the pictures from the web stay as the tool
    /// loop has them here: their tools go over Yap's run endpoint
    /// (`integrations/claude_live/eyes.rs`), the tool loop's own code — so
    /// the tabs open are told with the question, as for any model. Agents
    /// stay as they are too (on, not across the meetings,
    /// `claude_live/agents.rs`).
    ///
    /// No connectors in the tool loop's sense (its gate, its prompt line, its
    /// budget): a plan run gets them through Yap's run endpoint instead
    /// (Connectors wave 7: `plan.rs` `plan_connectors`,
    /// `claude_live/connectors.rs`), the same `connectors::Turn`.
    fn on_the_plan(self) -> Gates {
        Gates {
            strong: false,
            connectors: false,
            claude_in_chat: false,
            own_search: false,
            web_lines: crate::chat::tools::WebLines::of(false, false, false),
            ..self
        }
    }

    /// The tools' schemas every step sends, counted by `counter` (none for
    /// a model without tools): what the prompt and the question must fit
    /// beside. `conversation`: the one asking (`search_chats` leaves it out);
    /// `ep`, `scope` and `context` (the model's, in tokens): what the
    /// connectors' tools come to (`connectors::Turn::for_turn`, as the turn
    /// builds them). `code`: it's a Code chat, which offers none of memory's
    /// tools (`run.rs`), so they aren't counted.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn schemas(&self, cfg: &YapConfig, ep: &Endpoint, scope: Option<&str>, context: u64, counter: &crate::chat::budget::Counter, conversation: Option<u64>, code: bool) -> u64 {
        if !self.tools {
            return 0;
        }
        let connectors = self.connectors.then(|| crate::chat::tools::connectors::Turn::for_turn(cfg, &ep.provider, &ep.model, scope, context)).flatten();
        // (Only whether the calendar's offered matters to its schema.)
        let log = crate::chat::tools::calendar::Log::default();
        let extra = crate::chat::tools::Extra {
            memory: !code,
            search_chats: !code && cfg.memory_search_chats,
            conversation,
            calendar: self.calendar.then_some(crate::chat::tools::CalendarTurn { log: &log, zone: crate::chat::tools::calendar::Zone::of(None), now: 0 }),
            room: None,
            docs: None,
            doc_tools: self.doc_tools,
            strong: self.strong,
            pictures: self.pictures,
            screen: self.screen,
            act: self.act,
            own_search: self.own_search,
            web_question: false,
            agents: self.agents.then_some(crate::chat::tools::AgentsTurn::default()),
            connectors: connectors.as_ref(),
        };
        // (Tools deferred behind Claude's tool search cost nothing until found.)
        let offered = crate::chat::tools::offered_tools(cfg.open_apps, self.web_on && !self.claude_in_chat, extra);
        counter.count(&crate::chat::budget::loaded_tools(&offered).to_string()).await
    }

    /// The part of the request that's there whatever the question, for these
    /// gates ([`super::stable_prompt`]), in a Code chat (`code`) as it has it.
    pub(super) fn stable_prompt(&self, cfg: &YapConfig, persona: &str, profile: &crate::ai::about_you::Profile, now: chrono::DateTime<chrono::Utc>, code: bool) -> String {
        super::stable_prompt(cfg, persona, self.tools, self.doc_tools, self.web_lines, self.web_on, self.pictures, self.act, self.calendar, self.connectors, profile, now, code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(provider: &str, model: &str) -> Endpoint {
        Endpoint {
            base_url: "https://api.example.com/v1".into(),
            api_key: String::new(),
            model: model.into(),
            provider: provider.into(),
            persona: String::new(),
            disable_thinking: false,
        }
    }

    #[test]
    fn where_a_question_is_asked_decides_what_its_model_may_do() {
        let cfg = YapConfig { web_search: true, web_search_native: false, ..Default::default() };
        let strong = endpoint("custom", "gpt-5-example");
        // Typed in Chat: the web, its pictures and pages, acting on them.
        let chat = Gates::for_turn(&cfg, &strong, None, false, true);
        assert!(chat.tools && chat.strong && chat.web_on && !chat.claude_in_chat && !chat.own_search);
        assert_ne!(chat.pictures, Gate::Off);
        assert!(chat.act);
        assert_eq!(chat.doc_tools, crate::chat::documents::ToolSet::Rich);
        // Across the meetings: no web, no pages, no calendar.
        let across = Gates::for_turn(&cfg, &strong, Some("meetings"), true, false);
        assert!(across.tools && !across.web_on && !across.act && !across.calendar);
        assert_eq!(across.pictures, Gate::Off);
        assert_eq!(across.screen, Gate::Off);
        // The Voice Assistant from another app: the web, but no panel to show pictures in.
        let away = Gates::for_turn(&cfg, &strong, Some("assistant"), false, false);
        assert!(away.web_on && !away.act);
        assert_eq!(away.pictures, Gate::Off);
        // …and with Chat in view, its pictures; acting on pages stays a typed turn's.
        let in_view = Gates::for_turn(&cfg, &strong, Some("assistant"), false, true);
        assert_ne!(in_view.pictures, Gate::Off);
        assert!(!in_view.act);
        // The web off: none of it.
        let off = Gates::for_turn(&YapConfig { web_search: false, ..cfg.clone() }, &strong, None, false, true);
        assert!(!off.web_on && !off.act);
        assert_eq!(off.pictures, Gate::Off);
        // A model that can't call tools: no tools, nothing strong.
        let small = Gates::for_turn(&cfg, &endpoint("local", "qwen2.5-1.5b-instruct"), None, false, true);
        assert!(!small.tools && !small.strong && small.web_on);
        assert_eq!(small.pictures, Gate::Off);
        // Your Claude plan: it answers, with the web, never documents (Yap's
        // tool loop's, which it never runs); the browser (acting on its
        // pages too, typed in Chat), pictures and the screen as the tool
        // loop has them (over Yap's run endpoint): a Claude reads images.
        let plan_ep = endpoint(crate::ai::claude_plan::PROVIDER, "claude-opus-5-5");
        let plan = Gates::for_turn(&YapConfig { assistant_screen: true, ..cfg.clone() }, &plan_ep, None, false, true);
        assert!(plan.tools && plan.web_on && !plan.strong && plan.act && !plan.claude_in_chat && !plan.own_search);
        assert_eq!((plan.pictures, plan.screen), (Gate::Look, Gate::Look));
        // …the same as a Claude on an API key.
        let keyed = Gates::for_turn(&YapConfig { assistant_screen: true, ..cfg.clone() }, &endpoint("anthropic", "claude-opus-5-5"), None, false, true);
        assert_eq!((keyed.pictures, keyed.screen, keyed.act), (plan.pictures, plan.screen, plan.act));
        // Asked out loud with Chat in view: no acting, as for any model.
        assert!(!Gates::for_turn(&YapConfig { assistant_screen: true, ..cfg.clone() }, &plan_ep, Some("assistant"), false, true).act);
        // From another app, or with the screen's switch off: as for any model.
        let away = Gates::for_turn(&YapConfig { assistant_screen: true, ..cfg.clone() }, &plan_ep, Some("assistant"), false, false);
        assert_eq!((away.pictures, away.screen), (Gate::Off, Gate::Off));
        let off = Gates::for_turn(&YapConfig { assistant_screen: false, ..cfg.clone() }, &plan_ep, None, false, true);
        assert_eq!((off.pictures, off.screen), (Gate::Look, Gate::Off));
        // Across the meetings: none of them.
        let across = Gates::for_turn(&YapConfig { assistant_screen: true, ..cfg.clone() }, &plan_ep, Some("meetings"), true, false);
        assert_eq!((across.pictures, across.screen), (Gate::Off, Gate::Off));
        // …and keeps agents as the turn has them (over Yap's run endpoint).
        assert!(Gates { agents: true, ..plan }.on_the_plan().agents);
        assert!(!Gates { agents: false, ..plan }.on_the_plan().agents);
        assert!(!Gates::for_turn(&YapConfig { agents_enabled: true, ..cfg.clone() }, &endpoint(crate::ai::claude_plan::PROVIDER, "claude-opus-5-5"), Some("meetings"), true, false).agents, "never across the meetings");
    }
}
