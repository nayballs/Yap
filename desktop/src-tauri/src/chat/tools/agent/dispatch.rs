//! Running a call: which of Yap's tool sources a name belongs to, and the
//! call run there.

use serde_json::Value;

use super::Loop;
use crate::chat::tools::memory::{asked_to_remember, search_chats, MEMORY_TOOL_NAMES};
use crate::chat::tools::notes::execute;
use crate::chat::tools::{tool_error, tool_result, Extra};

/// **Where a tool call runs**: each of the sources a turn offers tools
/// from (`offer.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    /// A connector's tool, or tool search's `find_tools` / `use_tool`
    /// (`tools/connectors.rs`): first, since its names (`<slug>__<tool>`)
    /// are never Yap's own.
    Connectors,
    /// `web_search`, `read_page` (`web.rs`'s per-turn `web::Turn`).
    Web,
    /// Pictures from the web looked at or shown (`chat/pictures.rs`).
    Pictures,
    /// Picture files on this PC without the web (`chat/pictures/local.rs`).
    Files,
    /// `look_at_screen` (`chat/see_screen.rs`).
    Screen,
    /// The browser beside the chat (`chat/pages.rs`: going, reading,
    /// looking, and — for the models that may — acting).
    Pages,
    /// `remember`, `forget` (memory.rs).
    Memory,
    /// `search_chats`.
    Chats,
    /// `search_notes`: Ask across meetings' search.
    NotesSearch,
    /// `check_calendar` (`tools/calendar.rs`).
    Calendar,
    /// `open_app`, `list_apps` (`apps.rs`).
    Apps,
    /// `start_agent`, `agent_status`, `message_agent`, `stop_agent` (`tools/agents.rs`).
    Agents,
    /// Every other name: the note and document tools (`tools/notes.rs`),
    /// which say so for a name they don't know.
    Notes,
}

impl Source {
    /// The source that runs `name` in a turn offering `extra` (`web`: the
    /// web is on; `claude_web`: a strong Claude's own web tools in Chat's
    /// request; `open_apps`: the app tools are offered). The first that
    /// takes it, in this order. Pure.
    pub(super) fn of(name: &str, web: bool, extra: &Extra<'_>, claude_web: bool, open_apps: bool) -> Source {
        if extra.connectors.is_some_and(|c| c.owns(name)) {
            Source::Connectors
        } else if web && crate::web::TOOL_NAMES.contains(&name) {
            Source::Web
        } else if web && !extra.without_pictures && extra.pictures.offers(name, claude_web || extra.own_search) {
            Source::Pictures
        } else if !web && crate::chat::pictures::file_offers(extra.files, name) {
            Source::Files
        } else if crate::chat::see_screen::offers(extra.screen, name) {
            Source::Screen
        } else if web && crate::chat::pages::offers(extra.pictures, extra.act, name) {
            Source::Pages
        } else if extra.memory && MEMORY_TOOL_NAMES.contains(&name) {
            Source::Memory
        } else if extra.search_chats && name == "search_chats" {
            Source::Chats
        } else if name == "search_notes" {
            Source::NotesSearch
        } else if extra.calendar.is_some() && name == crate::chat::tools::calendar::NAME {
            Source::Calendar
        } else if open_apps && crate::apps::TOOL_NAMES.contains(&name) {
            Source::Apps
        } else if extra.agents.is_some() && crate::chat::tools::agents::NAMES.contains(&name) {
            Source::Agents
        } else {
            Source::Notes
        }
    }
}

impl Loop<'_> {
    /// **One call run** where its `source` says, with `args` as fixed
    /// (`args.rs`): its result, for the model. A picture or the screen
    /// looked at goes in `looks` while it fits in what the request has left
    /// (`room.rs`); what the page should see at once (a picture shown, a
    /// page brought up, the screen taken) goes to it as it happens.
    pub(super) async fn dispatch(&mut self, source: Source, name: &str, args: Value, looks: &mut Vec<crate::chat::pictures::Look>) -> Value {
        match source {
            Source::Connectors => {
                // (Named only while the turn offers connectors, `Source::of`.)
                let Some(turn) = self.extra.connectors else { return execute(name, &args) };
                let own = self.own;
                let ctx = crate::chat::tools::connectors::Ctx { question: self.question, own, outside: self.seen().any(), live: self.live };
                let ran = turn.execute(name, &args, ctx).await;
                // What a server returned is outside content from here on:
                // Yap's own actions lock, the page tools' typed words are
                // checked, and the connectors' writes ask (consent.rs).
                if ran.reached {
                    self.connector_seen = true;
                    if let Some(w) = self.web.as_deref_mut() {
                        w.mark_seen();
                    }
                }
                // Its pictures go with the next step while they fit, as a
                // page's shot does.
                let mut result = ran.result;
                let mut left_out = 0;
                for look in ran.looks {
                    if self.room.takes_picture(&self.messages, looks, &look) {
                        looks.push(look);
                    } else {
                        left_out += 1;
                    }
                }
                if left_out > 0 {
                    tracing::info!(provider = self.provider, left_out, "Chat: no room left in the model's context for a connector's picture");
                    result["note"] = Value::String(crate::chat::budget::NO_ROOM_TO_LOOK.to_string());
                }
                result
            }
            Source::Web | Source::Pictures | Source::Pages => {
                // Outside content the turn's web side doesn't record itself
                // (the page tools' addresses the model writes: pages::written).
                let outside = self.agent_seen || self.screen_seen || self.connector_seen;
                // (Named only with the web on, `Source::of`; without it a
                // name is the note tools', as any other.)
                let Some(turn) = self.web.as_deref_mut() else { return execute(name, &args) };
                match source {
                    Source::Web => turn.execute(name, &args).await,
                    Source::Pictures => {
                        // A picture from the web looked at or shown (chat/pictures.rs):
                        // a shown one goes to the page at once (the panel opens on it).
                        let run = crate::chat::pictures::execute(turn, name, &args, self.provider, self.base_url).await;
                        if let (Some(picture), Some(live)) = (run.shown, self.live) {
                            live.sink.send(crate::chat::stream::Event::Picture { picture });
                        }
                        // A picture looked at goes with every later step: only
                        // while it fits in what the request has left (budget.rs).
                        match run.look {
                            Some(look) if !self.room.takes_picture(&self.messages, looks, &look) => {
                                tracing::info!(provider = self.provider, "Chat: no room left in the model's context for the picture; not looked at");
                                crate::ai::eval::post_fix("look_no_room");
                                tool_error(crate::chat::budget::NO_ROOM_TO_LOOK.to_string(), Value::Null, "Didn't look at a picture: no room left in the model's context".to_string())
                            }
                            look => {
                                looks.extend(look);
                                run.result
                            }
                        }
                    }
                    _ => {
                        // The browser beside the chat (chat/pages.rs): a page
                        // opened, a shot in the steps, a card before an action
                        // that buys or pays — the page hears each at once; a
                        // shot goes with every later step while it fits.
                        // (Yap's web_search, or a strong Claude's own by the same name.)
                        let outside = outside || (crate::chat::pages::NAVIGATES.contains(&name) && crate::chat::pages::after_web_answer(self.extra.conversation));
                        let ctx = crate::chat::pages::Ctx {
                            gate: self.extra.pictures,
                            act: self.extra.act,
                            provider: self.provider,
                            base_url: self.base_url,
                            live: self.live,
                            outside,
                            search: Some(crate::chat::pages::WEB_SEARCH),
                        };
                        let run = crate::chat::pages::execute(turn, name, &args, &ctx).await;
                        let mut result = run.result;
                        let mut left_out = 0;
                        for look in run.looks {
                            if self.room.takes_picture(&self.messages, looks, &look) {
                                looks.push(look);
                            } else {
                                left_out += 1;
                            }
                        }
                        if left_out > 0 {
                            tracing::info!(provider = self.provider, left_out, "Chat: no room left in the model's context for a page's shot");
                            crate::ai::eval::post_fix("look_no_room");
                            result["note"] = Value::String(crate::chat::budget::NO_ROOM_TO_LOOK.to_string());
                        }
                        result
                    }
                }
            }
            Source::Files => {
                // A picture file on this PC without the web (chat/pictures/
                // local.rs): shown, the page hears it at once; looked at, it
                // goes with the next step while it fits — outside content
                // from then on, as the screen's is.
                let Some(files) = self.extra.file_pictures else { return execute(name, &args) };
                let run = files.execute(name, &args, self.provider, self.base_url).await;
                if let (Some(picture), Some(live)) = (run.shown, self.live) {
                    live.sink.send(crate::chat::stream::Event::Picture { picture });
                }
                match run.look {
                    Some(look) if !self.room.takes_picture(&self.messages, looks, &look) => {
                        tracing::info!(provider = self.provider, "Chat: no room left in the model's context for the picture file; not looked at");
                        tool_error(crate::chat::budget::NO_ROOM_TO_LOOK.to_string(), Value::Null, "Didn't look at a picture: no room left in the model's context".to_string())
                    }
                    Some(look) => {
                        self.screen_seen = true;
                        looks.push(look);
                        run.result
                    }
                    None => run.result,
                }
            }
            Source::Screen => {
                // The screen as it is now (chat/see_screen.rs): its pictures
                // go with the next step while they fit, its text in the result.
                let run = crate::chat::see_screen::execute(&args, self.extra.screen, self.provider, self.base_url).await;
                self.screen_seen |= run.seen;
                // Shown with the question, as it was taken.
                if !run.images.is_empty() {
                    if let Some(conv) = self.extra.conversation {
                        crate::chat::chats::add_question_images(conv, &run.images);
                    }
                    if let Some(live) = self.live {
                        live.sink.send(crate::chat::stream::Event::Screen { images: run.images.clone() });
                    }
                }
                let mut fits = true;
                for look in run.looks {
                    if self.room.takes_picture(&self.messages, looks, &look) {
                        looks.push(look);
                    } else {
                        fits = false;
                    }
                }
                if fits {
                    run.result
                } else {
                    tracing::info!(provider = self.provider, "Chat: no room left in the model's context for the screen");
                    tool_error(crate::chat::budget::NO_ROOM_TO_LOOK.to_string(), Value::Null, "Didn't look at your screen: no room left in the model's context".to_string())
                }
            }
            Source::Memory => {
                if asked_to_remember(self.question) {
                    crate::memory::learn::tool(name, &args)
                } else {
                    tool_error(
                        format!("{name} only runs when the user's own message asks to remember or forget something. Don't call it again this turn."),
                        Value::Null,
                        "Didn't change Yap's memory: not asked to".to_string(),
                    )
                }
            }
            Source::Chats => search_chats(&args, self.extra.conversation).await,
            // Ask across meetings' search: passages, by meaning too.
            Source::NotesSearch => crate::chat::tools::notes::search_notes_tool(&args).await,
            Source::Calendar => {
                // (Named only while the calendar's offered, `Source::of`.)
                let Some(cal) = self.extra.calendar else { return execute(name, &args) };
                // Its meetings numbered with the turn's web sources (one
                // numbering for the answer's citations).
                let mut counter = cal.log.len();
                let mut next = || match self.web.as_deref_mut() {
                    Some(t) => t.reserve_number(),
                    None => {
                        counter += 1;
                        counter
                    }
                };
                crate::chat::tools::calendar::run(&args, cal.log, cal.zone, cal.now, &mut next)
            }
            Source::Apps => {
                // Off the async runtime: the index, the launch.
                let (name, question, previous) = (name.to_string(), self.question.to_string(), self.previous.clone());
                tauri::async_runtime::spawn_blocking(move || crate::apps::execute_tool(&name, &args, &question, previous.as_deref()))
                    .await
                    .unwrap_or_else(|e| tool_result(false, Value::Null, format!("The app tool failed: {e}")))
            }
            Source::Agents => {
                // (Named only while agents are offered, `Source::of`.)
                let Some(turn) = self.extra.agents else { return execute(name, &args) };
                let web_seen = self.seen().any();
                let ctx = crate::chat::tools::agents::Ctx {
                    question: self.question,
                    previous: self.previous.as_deref(),
                    conversation: self.extra.conversation,
                    turn,
                    web_seen,
                    live: self.live,
                };
                let ran = crate::chat::tools::agents::execute(name, &args, ctx).await;
                // An agent's own words: web content from here on (the safety rule).
                self.agent_seen |= ran.output_seen;
                // An agent started: its tab opens at once, as a page does.
                if let (Some(agent), Some(live)) = (ran.started, self.live) {
                    live.sink.send(crate::chat::stream::Event::Agent { agent });
                }
                ran.result
            }
            Source::Notes => execute(name, &args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_runs_where_the_turn_offers_it() {
        let none = Extra::default();
        // The web's tools only with the web on; else the note tools say they don't know it.
        assert_eq!(Source::of("web_search", true, &none, false, false), Source::Web);
        assert_eq!(Source::of("read_page", true, &none, false, false), Source::Web);
        assert_eq!(Source::of("web_search", false, &none, false, false), Source::Notes);
        // Memory and past chats only when offered.
        let memory = Extra { memory: true, search_chats: true, ..Default::default() };
        assert_eq!(Source::of("remember", false, &memory, false, false), Source::Memory);
        assert_eq!(Source::of("remember", false, &none, false, false), Source::Notes);
        assert_eq!(Source::of("search_chats", false, &memory, false, false), Source::Chats);
        assert_eq!(Source::of("search_chats", false, &none, false, false), Source::Notes);
        // The notes search, always; the note tools, everything else.
        assert_eq!(Source::of("search_notes", false, &none, false, false), Source::NotesSearch);
        assert_eq!(Source::of("edit_note", true, &memory, false, true), Source::Notes);
        assert_eq!(Source::of("create_document", true, &memory, false, true), Source::Notes);
        // Apps while they're offered.
        assert_eq!(Source::of("open_app", false, &none, false, true), Source::Apps);
        assert_eq!(Source::of("open_app", false, &none, false, false), Source::Notes);
        // Agents while they're offered.
        let agents = Extra { agents: Some(crate::chat::tools::AgentsTurn::default()), ..Default::default() };
        assert_eq!(Source::of("start_agent", false, &agents, false, false), Source::Agents);
        assert_eq!(Source::of("message_agent", true, &agents, false, true), Source::Agents);
        assert_eq!(Source::of("start_agent", false, &none, false, false), Source::Notes);
        // Pictures and pages with their gate and the web; the screen with its own gate.
        let look = Extra { pictures: crate::chat::pictures::Gate::Look, screen: crate::chat::pictures::Gate::Look, act: true, ..Default::default() };
        assert_eq!(Source::of(crate::chat::pictures::SHOW, true, &look, false, false), Source::Pictures);
        assert_eq!(Source::of(crate::chat::pictures::SHOW, false, &look, false, false), Source::Notes);
        assert_eq!(Source::of(crate::chat::see_screen::LOOK, false, &look, false, false), Source::Screen);
        assert_eq!(Source::of(crate::chat::see_screen::LOOK, false, &none, false, false), Source::Notes);
        // Picture files without the web: show (and look, for a model that reads images).
        let files = Extra { files: crate::chat::pictures::Gate::Look, ..Default::default() };
        assert_eq!(Source::of(crate::chat::pictures::SHOW, false, &files, false, false), Source::Files);
        assert_eq!(Source::of(crate::chat::pictures::LOOK, false, &files, false, false), Source::Files);
        assert_eq!(Source::of(crate::chat::pictures::FIND, false, &files, false, false), Source::Notes);
        assert_eq!(Source::of(crate::chat::pictures::LOOK, false, &Extra { files: crate::chat::pictures::Gate::Show, ..files }, false, false), Source::Notes);
        assert_eq!(Source::of(crate::chat::pictures::SHOW, true, &files, false, false), Source::Notes, "never with the web");
        // The browser: every tool of it with the pages; acting only for a model that may.
        for name in crate::chat::pages::ALL {
            assert_eq!(Source::of(name, true, &look, false, false), Source::Pages, "{name}");
        }
        assert_eq!(Source::of(crate::chat::pages::act::CLICK, true, &Extra { act: false, ..look }, false, false), Source::Notes);
        assert_eq!(Source::of(crate::chat::pages::SCREENSHOT, true, &Extra { pictures: crate::chat::pictures::Gate::Show, ..look }, false, false), Source::Notes);
        // An older name: the tool it is now.
        assert_eq!(Source::of("open_page", true, &look, false, false), Source::Pages);
        assert_eq!(Source::of("click", true, &look, false, false), Source::Pages);
        assert_eq!(Source::of("click", true, &Extra { act: false, ..look }, false, false), Source::Notes);
        // `find_pictures` only where Yap's own page reads aren't offered.
        assert_eq!(Source::of(crate::chat::pictures::FIND, true, &look, false, false), Source::Notes);
        assert_eq!(Source::of(crate::chat::pictures::FIND, true, &look, true, false), Source::Pictures);
    }

    #[test]
    fn a_connectors_tool_runs_there_while_the_turn_offers_it() {
        use crate::chat::tools::connectors::{Offered, Surface, Turn, FIND, USE};
        let tool = |wire: &str, tokens: u64| Offered {
            wire: wire.into(),
            id: "c1".into(),
            connector: "Home lights".into(),
            tool: wire.split("__").nth(1).unwrap_or("").into(),
            title: None,
            description: "A tool.".into(),
            schema: serde_json::json!({ "type": "object" }),
            kind: crate::integrations::connectors::cache::Kind::Reads,
            setting: crate::integrations::connectors::store::Setting::Allow,
            hash: "h".into(),
            tokens,
        };
        let all = Turn::of(vec![tool("homelights__list_lights", 40)], 32_768, None, false, Surface::Chat, false, false, "custom").unwrap();
        let with = Extra { connectors: Some(&all), ..Default::default() };
        assert_eq!(Source::of("homelights__list_lights", true, &with, false, true), Source::Connectors);
        // Tool search's names only past the budget.
        assert_eq!(Source::of(FIND, false, &with, false, false), Source::Notes);
        let many = Turn::of((0..300).map(|i| tool(&format!("homelights__t{i}"), 40)).collect(), 32_768, None, false, Surface::Chat, false, false, "custom").unwrap();
        let searched = Extra { connectors: Some(&many), ..Default::default() };
        assert_eq!(Source::of(FIND, false, &searched, false, false), Source::Connectors);
        assert_eq!(Source::of(USE, false, &searched, false, false), Source::Connectors);
        // Not offered: the note tools say they don't know it; Yap's own names never go there.
        assert_eq!(Source::of("homelights__list_lights", true, &Extra::default(), false, true), Source::Notes);
        assert_eq!(Source::of("web_search", true, &with, false, false), Source::Web);
        assert_eq!(Source::of("search_notes", true, &with, false, false), Source::NotesSearch);
    }
}
