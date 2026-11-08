//! Offering tools: the one list every step of a turn sends.

use serde_json::Value;

use crate::chat::tools::memory::memory_definitions;
use crate::chat::tools::{app_definitions, web_definitions, Extra};

/// The tools a turn's steps offer (every step the same): the note tools,
/// and the apps', the web's, memory's, the calendar's and the agents' when the
/// turn has them. Also what Chat's budget measures (`turn.rs`).
pub fn offered_tools(open_apps: bool, web: bool, extra: Extra<'_>) -> Value {
    // The note tools, with the documents' set this model gets.
    let mut tools = crate::chat::tools::notes::definitions_for(extra.doc_tools);
    if open_apps {
        if let (Some(all), Some(apps)) = (tools.as_array_mut(), app_definitions().as_array()) {
            all.extend(apps.iter().cloned());
        }
    }
    if web {
        // (A cloud model on its provider's own search: `web_search` alone.)
        if let (Some(all), Some(web_tools)) = (tools.as_array_mut(), web_definitions().as_array()) {
            all.extend(web_tools.iter().filter(|t| !(extra.own_search && t["function"]["name"] == "read_page")).cloned());
        }
    }
    // Pictures from the web (chat/pictures.rs): `find_pictures` where Yap's
    // `read_page` isn't offered (a Claude searching in Chat's own request,
    // a cloud model on its provider's own search).
    if let (Some(all), Some(pictures)) = (tools.as_array_mut(), crate::chat::pictures::definitions(extra.pictures, !web || extra.own_search).as_array()) {
        all.extend(pictures.iter().cloned());
    }
    // Pages beside the chat (chat/pages.rs): for the same models; acting on
    // them (chat/pages/act.rs) for the ones that may.
    if let (Some(all), Some(pages)) = (tools.as_array_mut(), crate::chat::pages::definitions(extra.pictures, extra.act).as_array()) {
        all.extend(pages.iter().cloned());
    }
    // The screen (chat/see_screen.rs): not tied to the web.
    if let (Some(all), Some(screen)) = (tools.as_array_mut(), crate::chat::see_screen::definitions(extra.screen).as_array()) {
        all.extend(screen.iter().cloned());
    }
    if let (Some(all), Some(memory_tools)) = (tools.as_array_mut(), memory_definitions(extra).as_array()) {
        all.extend(memory_tools.iter().cloned());
    }
    // The calendar (a calendar is connected): read-only, like the notes.
    if extra.calendar.is_some() {
        if let (Some(all), Some(cal)) = (tools.as_array_mut(), crate::chat::tools::calendar::definitions().as_array()) {
            all.extend(cal.iter().cloned());
        }
    }
    // Yap Code (tools/agents.rs): agents on and Claude Code on this PC.
    if extra.agents.is_some() {
        if let (Some(all), Some(agents)) = (tools.as_array_mut(), crate::chat::tools::agents::definitions().as_array()) {
            all.extend(agents.iter().cloned());
        }
    }
    tools
}
