//! **A connector's tool asked about out loud** (the Voice Assistant,
//! Connectors wave 6): what Yap says and shows for it — "Use Notion to
//! create page "Launch notes"? Say yes, or press Allow." ([`line`]) and the
//! Yap bar's **Allow once / Don't** ([`card`]), beside the card in Chat's
//! thread. How it's asked and answered — the one question waiting, a clear
//! spoken yes or no in the person's languages, a click only for a tool that
//! can delete and during a meeting — is every spoken OK's
//! (`spoken_ok.rs`), a page action's and an agent's too.

use crate::chat::pages::act::confirm::ConnectorCard;

/// How much of the main argument the line says.
const SAY_CHARS: usize = 60;
/// How many arguments the bar card lists.
const CARD_ARGS: usize = 3;

/// **The line Yap says** before a connector's tool (`spoken`: a spoken yes
/// may answer it): "Use Notion to create page "Launch notes"? Say yes, or
/// press Allow." — the tool's title in words, its main argument when it's
/// short and no secret. Pure.
pub fn line(c: &ConnectorCard, spoken: bool) -> String {
    let what = words_of(&c.tool);
    let main = c.args.first().filter(|a| !a.secret && !a.cut && !a.value.trim().is_empty() && !a.value.contains('\n') && a.value.chars().count() <= SAY_CHARS);
    let mut q = format!("Use {} to {what}", c.connector.trim());
    if let Some(a) = main {
        q.push_str(&format!(" \u{201c}{}\u{201d}", a.value.trim()));
    }
    q.push('?');
    if spoken {
        q.push_str(" Say yes, or press Allow.");
    } else {
        q.push_str(" It needs a click: press Allow on the Yap bar.");
    }
    q
}

/// A tool's title or name as words to say: "Create page" → "create page",
/// "list_lights" → "list lights", "HassTurnOn" → "hass turn on" (a word in
/// capitals stays so: "Send SMS"). Pure.
pub fn words_of(tool: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in tool.trim().chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !out.ends_with(' ') {
            out.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        out.push(ch);
    }
    out.split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| if w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase()) { w.to_string() } else { w.to_lowercase() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// **The bar's card** for question `id`: "Use Notion: Create page?", its
/// first arguments as key: value lines (secrets masked as on the thread's
/// card), **Allow once** and **Don't** (`spoken_ok::bar_card`). Pure.
pub fn card(id: u64, c: &ConnectorCard) -> crate::bar::Card {
    let mut body: Vec<String> = c.args.iter().take(CARD_ARGS).map(|a| format!("{}: {}", a.key, a.value.lines().next().unwrap_or("").trim())).collect();
    if c.args.len() > CARD_ARGS {
        body.push(format!("and {} more in Chat", c.args.len() - CARD_ARGS));
    }
    super::spoken_ok::bar_card(id, format!("Use {}: {}?", c.connector.trim(), c.tool.trim()), body.join("\n"), c.spoken)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::pages::act::confirm::CardArg;

    fn notion(spoken: bool) -> ConnectorCard {
        ConnectorCard {
            connector: "Notion".into(),
            tool: "Create page".into(),
            args: vec![
                CardArg { key: "title".into(), value: "Launch notes".into(), cut: false, secret: false },
                CardArg { key: "body".into(), value: "Agenda\nOwners".into(), cut: false, secret: false },
            ],
            always: true,
            why: "ask".into(),
            kind: "changes".into(),
            voice: true,
            spoken,
        }
    }

    #[test]
    fn the_line_says_the_connector_the_tool_and_its_main_argument() {
        assert_eq!(line(&notion(true), true), "Use Notion to create page \u{201c}Launch notes\u{201d}? Say yes, or press Allow.");
        assert_eq!(line(&notion(false), false), "Use Notion to create page \u{201c}Launch notes\u{201d}? It needs a click: press Allow on the Yap bar.");
        // A secret, a long value or none: not said.
        let mut c = notion(true);
        c.args[0].secret = true;
        assert_eq!(line(&c, true), "Use Notion to create page? Say yes, or press Allow.");
        c.args.clear();
        c.tool = "HassTurnOn".into();
        assert_eq!(line(&c, true), "Use Notion to hass turn on? Say yes, or press Allow.");
        assert_eq!(words_of("delete_all"), "delete all");
        assert_eq!(words_of("Send SMS"), "send SMS");
    }

    #[test]
    fn the_bar_card_asks_allow_once_or_dont_and_reads_back() {
        let c = card(7, &notion(true));
        assert_eq!(c.id, super::super::spoken_ok::CARD);
        assert_eq!(c.title, "Use Notion: Create page?");
        assert_eq!(c.body, "title: Launch notes\nbody: Agenda");
        assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:7", "Allow once")));
        assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:7", "Don't")));
        assert!(c.timeout_ms.is_none() && c.status.is_none());
        assert_eq!(card(7, &notion(false)).status.as_deref(), Some("This one needs a click"));
    }
}
