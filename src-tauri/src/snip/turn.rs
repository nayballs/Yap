//! A Chat turn's screenshots on their way to the model.
//!
//! Every provider Yap talks to takes an image the same way on its
//! OpenAI-compatible endpoint: a content part `{"type": "image_url",
//! "image_url": {"url": "data:image/png;base64,…"}}` in a user message
//! (Anthropic's, OpenAI's, Gemini's, Groq's, OpenRouter's, llama.cpp's).
//! The order follows each provider's advice: the images first, then the
//! text (Anthropic); the text first for OpenRouter. OpenAI gets `detail:
//! "auto"` (Anthropic ignores it).
//!
//! **The question's images** go with every model call of the turn — each
//! step of the tool loop re-sends the messages, a small model's plain
//! answer, the last no-tools call — through a task-local set around the
//! turn (`chat::commands::chat_send` → [`with`]): the two places that put the
//! question in a request (`tools::tool_loop`, `llm::note_chat`) call
//! [`attach`], which turns the question's message into a content array.
//! Outside a turn with images, [`attach`] does nothing.
//!
//! **Earlier images** ([`history_images`]): the most recent few in the
//! history window are sent again (3, fewer when the provider's per-request
//! cap would be passed — Groq takes 3 in all), each older one becomes a
//! line, "(An earlier screenshot, not sent again.)", and one past "Keep
//! chat screenshots", "(A screenshot was here; it was removed.)". A model on
//! this PC gets only the newest. A model Yap didn't know couldn't read
//! images that turns them down (Chat switched to it after a question with a
//! screenshot) is learned to be a "no" there and then, and the step is
//! asked again with each said in that line ([`without_images`],
//! `chat/tools/agent.rs`); from then on it gets none.

use serde_json::{json, Value};

use crate::chat::chats::{ChatImage, ChatMessage};

/// A turn's screenshots: what's saved with the question, and the parts the
/// model gets (fitted to its provider).
#[derive(Debug, Clone, Default)]
pub struct TurnImages {
    pub images: Vec<ChatImage>,
    pub parts: Vec<Value>,
    pub provider: String,
    /// A line for the question's context about what came with it (the
    /// Voice Assistant's look at the window in front: what the picture is,
    /// or why there's none — `chat/assistant/screen.rs`).
    pub note: Option<String>,
    /// A source the answer names (the window's text, read on this PC, sent
    /// in its picture's place): numbered after the turn's own.
    pub source: Option<crate::chat::chats::Source>,
}

tokio::task_local! {
    static TURN: TurnImages;
}

/// Run a Chat turn with its screenshots.
pub async fn with<F: std::future::Future>(images: TurnImages, f: F) -> F::Output {
    TURN.scope(images, f).await
}

/// The screenshots of the turn running here (none outside one).
pub fn images() -> Vec<ChatImage> {
    TURN.try_with(|t| t.images.clone()).unwrap_or_default()
}

/// The turn's line for the question's context ([`TurnImages::note`]).
pub fn note() -> Option<String> {
    TURN.try_with(|t| t.note.clone()).ok().flatten()
}

/// The answer's sources with the turn's own ([`TurnImages::source`])
/// after them, numbered next.
pub fn with_source(mut sources: Vec<crate::chat::chats::Source>) -> Vec<crate::chat::chats::Source> {
    if let Ok(Some(mut s)) = TURN.try_with(|t| t.source.clone()) {
        s.n = sources.iter().map(|x| x.n).max().unwrap_or(0) + 1;
        sources.push(s);
    }
    sources
}

/// The bytes the turn's own images carry (their copies, before base64).
fn current_bytes() -> usize {
    TURN.try_with(|t| t.parts.iter().filter_map(|p| p["image_url"]["url"].as_str()).map(|u| u.len() * 3 / 4).sum())
        .unwrap_or(0)
}

/// Which fitted earlier images still fit in the request (`room` bytes),
/// newest first, stopping at the first that doesn't: an older one never
/// goes when a newer one couldn't. Pure.
pub fn within_room(fitted: Vec<(String, String, usize)>, mut room: usize) -> std::collections::HashMap<String, String> {
    let mut kept = std::collections::HashMap::new();
    for (id, url, bytes) in fitted {
        if bytes > room {
            break;
        }
        room -= bytes;
        kept.insert(id, url);
    }
    kept
}

/// One image part, as `provider` takes it.
pub fn part(url: &str, provider: &str) -> Value {
    let mut p = json!({ "type": "image_url", "image_url": { "url": url } });
    if provider == "openai" {
        p["image_url"]["detail"] = json!("auto");
    }
    p
}

/// A user message's content with images: the parts and the text, in the
/// provider's order; just the text without any. Pure.
pub fn content(text: &str, parts: &[Value], provider: &str) -> Value {
    if parts.is_empty() {
        return json!(text);
    }
    let text_part = json!({ "type": "text", "text": text });
    let mut out = Vec::with_capacity(parts.len() + 1);
    if provider == "openrouter" {
        out.push(text_part);
        out.extend(parts.iter().cloned());
    } else {
        out.extend(parts.iter().cloned());
        out.push(text_part);
    }
    Value::Array(out)
}

/// The text of a message's content, whatever its shape (a string, or
/// parts: the text parts joined). Pure.
pub fn text_of(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let texts: Vec<&str> = parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

/// The turn's screenshots onto its question: the last user message in
/// `messages` (the question, `chats::question_messages`), when the turn
/// running here has any.
pub fn attach(messages: &mut [Value]) {
    let Ok((parts, provider)) = TURN.try_with(|t| (t.parts.clone(), t.provider.clone())) else { return };
    attach_parts(messages, &parts, &provider);
}

/// [`attach`] with the parts given. Pure.
pub fn attach_parts(messages: &mut [Value], parts: &[Value], provider: &str) {
    if parts.is_empty() {
        return;
    }
    if let Some(m) = messages.iter_mut().rev().find(|m| m["role"] == "user") {
        if let Some(text) = m["content"].as_str().map(str::to_string) {
            m["content"] = content(&text, parts, provider);
        }
    }
}

/// Whether a request carries images (its timeout is longer: the upload).
pub fn has_images(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|ms| {
        ms.iter().any(|m| m["content"].as_array().is_some_and(|ps| ps.iter().any(|p| p["type"] == "image_url")))
    })
}

/// A request body with every image's data URL replaced by its size
/// ("data:image/png;base64,<183 KB>"): for logs. Pure.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn redact_images(body: &Value) -> Value {
    match body {
        Value::String(s) if s.starts_with("data:image/") => {
            let head = s.split(',').next().unwrap_or("data:image");
            let kb = (s.len() - head.len()).saturating_sub(1) * 3 / 4 / 1024;
            json!(format!("{head},<{kb} KB>"))
        }
        Value::Array(a) => Value::Array(a.iter().map(redact_images).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), redact_images(v))).collect()),
        other => other.clone(),
    }
}

/// An earlier image that isn't sent again.
pub const NOT_SENT_AGAIN: &str = "(An earlier screenshot, not sent again.)";
/// An earlier image whose file is gone.
pub const REMOVED: &str = "(A screenshot was here; it was removed.)";

/// The earlier screenshots in a request's `messages` taken out, each
/// message's said in a line as one not sent again: for a model that turned
/// them down (`chat/tools/agent.rs`). Whether there were any. Pure.
pub fn without_images(messages: &mut [Value]) -> bool {
    let mut any = false;
    for m in messages.iter_mut().filter(|m| m["role"] == "user") {
        let Some(parts) = m["content"].as_array() else { continue };
        if !parts.iter().any(|p| p["type"] == "image_url") {
            continue;
        }
        let text = text_of(&m["content"]).unwrap_or_default();
        let text = if text.ends_with(NOT_SENT_AGAIN) { text } else { format!("{text}\n\n{NOT_SENT_AGAIN}") };
        m["content"] = json!(text.trim());
        any = true;
    }
    any
}

/// Which earlier images are sent again: the newest `budget` of those still
/// there (message index, image index), newest first — never one `held`
/// back (a screenshot of the window in front where the window itself may
/// not go now: [`with_history_images`]). Pure.
pub fn resend_plan(
    earlier: &[ChatMessage],
    budget: usize,
    exists: &dyn Fn(&str) -> bool,
    held: &dyn Fn(&ChatImage) -> bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (mi, m) in earlier.iter().enumerate().rev() {
        if m.role != "user" {
            continue;
        }
        for (ii, img) in m.images.iter().enumerate().rev() {
            if out.len() >= budget {
                return out;
            }
            if exists(&img.id) && !held(img) {
                out.push((mi, ii));
            }
        }
    }
    out
}

/// How many earlier images may go with this turn: three, within the
/// provider's per-request cap with the turn's own (`current`); none to a
/// model on this PC. Pure.
pub fn history_budget(provider: &str, on_this_pc: bool, current: usize) -> usize {
    if on_this_pc {
        return 0;
    }
    super::images::per_request(provider).saturating_sub(current).min(3)
}

/// The history's user messages (`messages`, as `chats::history_messages` or
/// the plain history built them from `earlier`, in the same order) with
/// their screenshots: the newest few sent again ([`resend_plan`]), the
/// rest — and every one `held` back — said in a line. `fitted`: an image
/// id → its data URL for this provider (`None`: it couldn't be read).
pub fn history_images(
    messages: &mut [Value],
    earlier: &[ChatMessage],
    provider: &str,
    budget: usize,
    exists: &dyn Fn(&str) -> bool,
    held: &dyn Fn(&ChatImage) -> bool,
    fitted: &dyn Fn(&str) -> Option<String>,
) {
    if !earlier.iter().any(|m| !m.images.is_empty()) {
        return;
    }
    let plan = resend_plan(earlier, budget, exists, held);
    let user_values: Vec<usize> = messages.iter().enumerate().filter(|(_, m)| m["role"] == "user").map(|(i, _)| i).collect();
    let user_messages: Vec<usize> = earlier.iter().enumerate().filter(|(_, m)| m.role == "user").map(|(i, _)| i).collect();
    for (vi, mi) in user_values.into_iter().zip(user_messages) {
        let m = &earlier[mi];
        if m.images.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        let mut notes = Vec::new();
        for (ii, img) in m.images.iter().enumerate() {
            if !exists(&img.id) {
                notes.push(REMOVED);
            } else if plan.contains(&(mi, ii)) {
                match fitted(&img.id) {
                    Some(url) => parts.push(part(&url, provider)),
                    None => notes.push(NOT_SENT_AGAIN),
                }
            } else {
                notes.push(NOT_SENT_AGAIN);
            }
        }
        let mut text = text_of(&messages[vi]["content"]).unwrap_or_default();
        notes.dedup();
        for n in notes {
            text = format!("{text}\n\n{n}");
        }
        messages[vi]["content"] = content(text.trim(), &parts, provider);
    }
}

/// [`history_images`] for a Chat turn to `model` on `provider` at
/// `base_url`: the copies fitted off the async runtime (they're usually
/// made already). A model known not to read images (`vision.rs`: switched
/// to after a question with a screenshot, or one that turned these down
/// before: [`without_images`]) gets none again, each one a line.
/// A screenshot of the window in front (the Voice Assistant's, source
/// "window") goes again only where the window itself may go now
/// (`assistant::screen::window_may_go`): taken for a model on this PC, it
/// never follows a later question to a cloud model without "…with cloud
/// models too".
pub async fn with_history_images(messages: &mut [Value], earlier: &[ChatMessage], provider: &str, model: &str, base_url: &str) {
    if !earlier.iter().any(|m| !m.images.is_empty()) {
        return;
    }
    let here = crate::ai::ai_model::runs_here(provider, base_url);
    let reads = super::vision::reads_images(provider, model) != Some(false);
    let budget = if reads { history_budget(provider, here, images().len()) } else { 0 };
    let fit = super::images::fit_for(provider, here);
    let cap = fit.cap_for(images().len() + budget);
    let exists = |id: &str| super::images::exists(id);
    let window_ok = crate::chat::assistant::screen::window_may_go(&crate::config::load(), provider, base_url);
    let held = |img: &ChatImage| img.source == "window" && !window_ok;
    let wanted: Vec<String> =
        resend_plan(earlier, budget, &exists, &held).into_iter().map(|(mi, ii)| earlier[mi].images[ii].id.clone()).collect();
    let made: Vec<(String, String, usize)> = tauri::async_runtime::spawn_blocking(move || {
        wanted
            .into_iter()
            .filter_map(|id| super::images::fitted(&id, fit.edge, cap).ok().map(|f| (id, f.url.to_string(), f.bytes)))
            .collect()
    })
    .await
    .unwrap_or_default();
    // Within what the provider takes in one request, with the question's own.
    let fitted = within_room(made, fit.request_bytes.saturating_sub(current_bytes()));
    history_images(messages, earlier, provider, budget, &exists, &held, &|id| fitted.get(id).cloned());
}

/// What `chat::commands::chat_send` needs before a turn with screenshots: the
/// stored images (ids checked, at most the provider's per-request cap) and
/// their copies fitted for `provider`.
pub async fn prepare(ids: &[String], provider: &str, base_url: &str) -> Result<TurnImages, String> {
    let here = crate::ai::ai_model::runs_here(provider, base_url);
    let fit = super::images::fit_for(provider, here);
    let per = super::images::per_request(provider);
    let ids: Vec<String> = ids.iter().filter(|id| super::images::is_id(id)).take(per).cloned().collect();
    // Each smaller when there are several, so all of them fit one request.
    let cap = fit.cap_for(ids.len());
    let provider_owned = provider.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut images = Vec::new();
        let mut parts = Vec::new();
        for id in &ids {
            let stored = super::images::stored(id).ok_or("That screenshot isn't there any more")?;
            let fitted = super::images::fitted(id, fit.edge, cap)?;
            parts.push(part(&fitted.url, &provider_owned));
            images.push(stored);
        }
        Ok(TurnImages { images, parts, provider: provider_owned, ..Default::default() })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(id: &str) -> ChatImage {
        ChatImage { id: id.into(), w: 10, h: 10, bytes: 100, source: "snip".into(), ..Default::default() }
    }

    fn said(role: &str, text: &str, images: &[&str]) -> ChatMessage {
        ChatMessage { role: role.into(), text: text.into(), images: images.iter().map(|i| img(i)).collect(), ..Default::default() }
    }

    #[test]
    fn each_provider_gets_the_image_its_way() {
        let p = part("data:image/png;base64,AAA", "anthropic");
        assert_eq!(p, json!({ "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA" } }));
        assert_eq!(part("data:x", "openai")["image_url"]["detail"], "auto");
        assert!(part("data:x", "groq")["image_url"].get("detail").is_none());
        // Images first, then the text…
        let c = content("What's this?", &[part("data:x", "anthropic")], "anthropic");
        assert_eq!(c[0]["type"], "image_url");
        assert_eq!(c[1], json!({ "type": "text", "text": "What's this?" }));
        for provider in ["groq", "gemini", "openai", "custom", "ondevice"] {
            assert_eq!(content("q", &[part("data:x", provider)], provider)[0]["type"], "image_url", "{provider}");
        }
        // …but the text first for OpenRouter.
        let c = content("q", &[part("data:x", "openrouter")], "openrouter");
        assert_eq!((c[0]["type"].as_str(), c[1]["type"].as_str()), (Some("text"), Some("image_url")));
        // No images: a plain string, as every other message.
        assert_eq!(content("q", &[], "groq"), json!("q"));
    }

    #[test]
    fn the_question_gets_the_images_and_nothing_else_does() {
        let mut messages = vec![
            json!({ "role": "system", "content": "sys" }),
            json!({ "role": "user", "content": "earlier" }),
            json!({ "role": "assistant", "content": "answer" }),
            json!({ "role": "user", "content": "<context>…</context>\n\nWhat's this?" }),
        ];
        attach_parts(&mut messages, &[part("data:x", "groq")], "groq");
        assert_eq!(messages[1]["content"], "earlier");
        assert_eq!(messages[3]["content"][0]["type"], "image_url");
        assert_eq!(text_of(&messages[3]["content"]).unwrap(), "<context>…</context>\n\nWhat's this?");
        // Outside a turn with images: nothing changes.
        let mut plain = vec![json!({ "role": "user", "content": "q" })];
        attach(&mut plain);
        assert_eq!(plain[0]["content"], "q");
    }

    #[tokio::test]
    async fn a_turn_carries_its_images_to_every_call() {
        let turn = TurnImages {
            images: vec![img("img_0123456789abcdef")],
            parts: vec![part("data:x", "gemini")],
            provider: "gemini".into(),
            ..Default::default()
        };
        let got = with(turn, async {
            let mut m = vec![json!({ "role": "user", "content": "q" })];
            attach(&mut m);
            (images().len(), m)
        })
        .await;
        assert_eq!(got.0, 1);
        assert_eq!(got.1[0]["content"][1]["text"], "q");
        assert!(images().is_empty(), "outside the turn");
    }

    #[tokio::test]
    async fn a_turn_names_its_own_source_after_the_answers() {
        let web = |n| crate::chat::chats::Source { n, title: format!("Page {n}"), url: format!("https://example.com/{n}"), ..Default::default() };
        let own = crate::chat::chats::Source { title: "The window's text".into(), screen: true, ..Default::default() };
        let turn = TurnImages { note: Some("A line.".into()), source: Some(own), ..Default::default() };
        let (said, sources) = with(turn, async { (note(), with_source(vec![web(1), web(3)])) }).await;
        assert_eq!(said.as_deref(), Some("A line."));
        assert_eq!(sources.iter().map(|s| s.n).collect::<Vec<_>>(), vec![1, 3, 4]);
        assert!(sources[2].screen);
        // Outside a turn: nothing added.
        assert_eq!(with_source(vec![web(1)]).len(), 1);
        assert!(note().is_none());
    }

    #[test]
    fn a_requests_images_are_found_and_kept_out_of_logs() {
        let url = format!("data:image/png;base64,{}", "A".repeat(4 * 1024 * 4 / 3 + 4));
        let body = json!({ "model": "m", "messages": [{ "role": "user", "content": [part(&url, "groq"), { "type": "text", "text": "q" }] }] });
        assert!(has_images(&body));
        assert!(!has_images(&json!({ "messages": [{ "role": "user", "content": "q" }] })));
        let redacted = redact_images(&body);
        assert_eq!(redacted["messages"][0]["content"][0]["image_url"]["url"], "data:image/png;base64,<4 KB>");
        assert_eq!(redacted["messages"][0]["content"][1]["text"], "q");
        assert!(!redacted.to_string().contains("AAAA"));
    }

    #[test]
    fn the_newest_earlier_images_go_again_and_the_rest_are_a_line() {
        let earlier = vec![
            said("user", "first", &["img_a", "img_b"]),
            said("assistant", "ok", &[]),
            said("user", "second", &["img_c"]),
            said("assistant", "ok", &[]),
            said("user", "third", &["img_d", "img_gone"]),
            said("assistant", "ok", &[]),
        ];
        let exists = |id: &str| id != "img_gone";
        let none_held = |_: &ChatImage| false;
        // Three: d, c, b (newest first); a is said in a line; the removed one too.
        assert_eq!(resend_plan(&earlier, 3, &exists, &none_held), vec![(4, 0), (2, 0), (0, 1)]);
        let mut messages: Vec<Value> =
            earlier.iter().map(|m| json!({ "role": m.role, "content": m.text })).collect();
        // A replayed web search between them (assistant + tool) changes nothing.
        messages.insert(1, json!({ "role": "tool", "content": "{}" }));
        history_images(&mut messages, &earlier, "anthropic", 3, &exists, &none_held, &|id| Some(format!("data:{id}")));
        let first = &messages[0]["content"];
        assert_eq!(first[0]["image_url"]["url"], "data:img_b");
        assert!(text_of(first).unwrap().ends_with(NOT_SENT_AGAIN), "{first}");
        assert_eq!(messages[3]["content"][0]["image_url"]["url"], "data:img_c");
        let third = &messages[5]["content"];
        assert_eq!(third[0]["image_url"]["url"], "data:img_d");
        assert!(text_of(third).unwrap().ends_with(REMOVED));
        // Groq with one image this turn: two earlier ones at most; a model
        // on this PC: none.
        assert_eq!(history_budget("groq", false, 1), 2);
        assert_eq!(history_budget("anthropic", false, 1), 3);
        assert_eq!(history_budget("groq", false, 4), 0);
        assert_eq!(history_budget("custom", true, 1), 0);
        // Turned down by the model: every one taken out, each message's said
        // in one line (not twice where it had one already).
        assert!(without_images(&mut messages));
        assert!(!has_images(&json!({ "messages": messages })));
        assert_eq!(messages[0]["content"], format!("first\n\n{NOT_SENT_AGAIN}"));
        assert_eq!(messages[3]["content"], format!("second\n\n{NOT_SENT_AGAIN}"));
        assert_eq!(messages[5]["content"], format!("third\n\n{REMOVED}\n\n{NOT_SENT_AGAIN}"));
        assert_eq!(messages[2]["content"], "ok");
        assert!(!without_images(&mut messages), "none left");
    }

    /// The Voice Assistant took the window in front for a model on this PC;
    /// Chat's model is a cloud one by the follow-up: the window's
    /// screenshot isn't sent again unless the window itself may go there.
    #[test]
    fn a_window_in_fronts_screenshot_goes_again_only_where_the_window_may_go() {
        let window = ChatImage { source: "window".into(), ..img("img_w") };
        let earlier = vec![
            said("user", "first", &["img_a"]),
            said("assistant", "ok", &[]),
            ChatMessage { role: "user".into(), text: "what's this error?".into(), images: vec![window], ..Default::default() },
            said("assistant", "ok", &[]),
        ];
        let exists = |_: &str| true;
        let held = |i: &ChatImage| i.source == "window";
        // Held back: never planned, and it takes no place from an older snip.
        assert_eq!(resend_plan(&earlier, 1, &exists, &held), vec![(0, 0)]);
        let plain = || -> Vec<Value> { earlier.iter().map(|m| json!({ "role": m.role, "content": m.text })).collect() };
        let mut messages = plain();
        history_images(&mut messages, &earlier, "anthropic", 3, &exists, &held, &|id| Some(format!("data:{id}")));
        assert_eq!(messages[2]["content"], json!(format!("what's this error?\n\n{NOT_SENT_AGAIN}")));
        assert_eq!(messages[0]["content"][0]["image_url"]["url"], "data:img_a");
        assert!(!Value::Array(messages).to_string().contains("data:img_w"));
        // Where it may go: as any other screenshot.
        let mut messages = plain();
        history_images(&mut messages, &earlier, "anthropic", 3, &exists, &|_| false, &|id| Some(format!("data:{id}")));
        assert_eq!(messages[2]["content"][0]["image_url"]["url"], "data:img_w");

        // The rule: the window's own (screen.rs `allowed`).
        use crate::chat::assistant::screen::window_may_go;
        let mut cfg = crate::config::YapConfig { assistant_screen: true, assistant_screen_cloud: false, ..Default::default() };
        assert!(window_may_go(&cfg, "ondevice", "http://127.0.0.1:8080/v1"));
        assert!(!window_may_go(&cfg, "anthropic", "https://api.anthropic.com/v1"));
        assert!(!window_may_go(&cfg, "custom", "http://192.168.1.20:8080/v1"));
        cfg.assistant_screen_cloud = true;
        assert!(window_may_go(&cfg, "anthropic", "https://api.anthropic.com/v1"));
        cfg.assistant_screen = false;
        assert!(!window_may_go(&cfg, "anthropic", "https://api.anthropic.com/v1"));
    }

    #[test]
    fn earlier_images_go_only_while_the_request_has_room() {
        let made = |sizes: &[usize]| -> Vec<(String, String, usize)> {
            sizes.iter().enumerate().map(|(i, &b)| (format!("img_{i}"), format!("data:{i}"), b)).collect()
        };
        // Newest first: both fit.
        assert_eq!(within_room(made(&[1_000, 2_000]), 3_000).len(), 2);
        // The newest doesn't fit: nothing older goes either.
        assert!(within_room(made(&[4_000, 100]), 3_000).is_empty());
        // The second stops it; the third, smaller, still doesn't go.
        let kept = within_room(made(&[2_000, 2_000, 500]), 3_000);
        assert_eq!(kept.keys().cloned().collect::<Vec<_>>(), ["img_0"]);
        // No room at all.
        assert!(within_room(made(&[1]), 0).is_empty());
    }

    #[test]
    fn each_provider_takes_what_it_accepts_in_one_request() {
        use super::super::images::{fit_for, MAX_SEND_BYTES};
        let groq = fit_for("groq", false);
        // Base64 adds a third: under Groq's 4 MB for one image.
        assert!(groq.max_bytes * 4 / 3 < 4 * 1024 * 1024);
        // Three Groq images share its request budget.
        assert_eq!(groq.cap_for(3), groq.request_bytes / 3);
        assert_eq!(groq.cap_for(1), groq.max_bytes);
        assert_eq!(groq.cap_for(0), groq.max_bytes);
        // Anthropic's 5 MB an image, base64.
        assert!(fit_for("anthropic", false).max_bytes * 4 / 3 < 5 * 1024 * 1024);
        // A model on this PC: its own edge, the usual cap.
        let here = fit_for("custom", true);
        assert_eq!((here.edge, here.max_bytes), (1024, MAX_SEND_BYTES));
    }
}
