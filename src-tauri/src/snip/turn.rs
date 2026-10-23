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
//! turn (`commands::chat_send` → [`with`]): the two places that put the
//! question in a request (`tools::tool_loop`, `llm::note_chat`) call
//! [`attach`], which turns the question's message into a content array.
//! Outside a turn with images, [`attach`] does nothing.
//!
//! **Earlier images** ([`history_images`]): the most recent few in the
//! history window are sent again (3, fewer when the provider's per-request
//! cap would be passed — Groq takes 3 in all), each older one becomes a
//! line, "(An earlier screenshot, not sent again.)", and one past "Keep
//! chat screenshots", "(A screenshot was here; it was removed.)". A model on
//! this PC gets only the newest.

use serde_json::{json, Value};

use crate::chats::{ChatImage, ChatMessage};

/// A turn's screenshots: what's saved with the question, and the parts the
/// model gets (fitted to its provider).
#[derive(Debug, Clone, Default)]
pub struct TurnImages {
    pub images: Vec<ChatImage>,
    pub parts: Vec<Value>,
    pub provider: String,
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

/// Which earlier images are sent again: the newest `budget` of those still
/// there (message index, image index), newest first. Pure.
pub fn resend_plan(earlier: &[ChatMessage], budget: usize, exists: &dyn Fn(&str) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (mi, m) in earlier.iter().enumerate().rev() {
        if m.role != "user" {
            continue;
        }
        for (ii, img) in m.images.iter().enumerate().rev() {
            if out.len() >= budget {
                return out;
            }
            if exists(&img.id) {
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
/// rest said in a line. `fitted`: an image id → its data URL for this
/// provider (`None`: it couldn't be read).
pub fn history_images(
    messages: &mut [Value],
    earlier: &[ChatMessage],
    provider: &str,
    budget: usize,
    exists: &dyn Fn(&str) -> bool,
    fitted: &dyn Fn(&str) -> Option<String>,
) {
    if !earlier.iter().any(|m| !m.images.is_empty()) {
        return;
    }
    let plan = resend_plan(earlier, budget, exists);
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

/// [`history_images`] for a Chat turn to `provider` at `base_url`: the
/// copies fitted off the async runtime (they're usually made already).
pub async fn with_history_images(messages: &mut [Value], earlier: &[ChatMessage], provider: &str, base_url: &str) {
    if !earlier.iter().any(|m| !m.images.is_empty()) {
        return;
    }
    let here = crate::ai_model::runs_here(provider, base_url);
    let budget = history_budget(provider, here, images().len());
    let edge = super::images::long_edge(provider, here);
    let exists = |id: &str| super::images::exists(id);
    let wanted: Vec<String> = resend_plan(earlier, budget, &exists).into_iter().map(|(mi, ii)| earlier[mi].images[ii].id.clone()).collect();
    let fitted: std::collections::HashMap<String, String> = tauri::async_runtime::spawn_blocking(move || {
        wanted
            .into_iter()
            .filter_map(|id| super::images::fitted(&id, edge).ok().map(|f| (id, f.url.to_string())))
            .collect()
    })
    .await
    .unwrap_or_default();
    history_images(messages, earlier, provider, budget, &exists, &|id| fitted.get(id).cloned());
}

/// What `commands::chat_send` needs before a turn with screenshots: the
/// stored images (ids checked, at most the provider's per-request cap) and
/// their copies fitted for `provider`.
pub async fn prepare(ids: &[String], provider: &str, base_url: &str) -> Result<TurnImages, String> {
    let here = crate::ai_model::runs_here(provider, base_url);
    let edge = super::images::long_edge(provider, here);
    let per = super::images::per_request(provider);
    let ids: Vec<String> = ids.iter().filter(|id| super::images::is_id(id)).take(per).cloned().collect();
    let provider_owned = provider.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut images = Vec::new();
        let mut parts = Vec::new();
        for id in &ids {
            let stored = super::images::stored(id).ok_or("That screenshot isn't there any more")?;
            let fitted = super::images::fitted(id, edge)?;
            parts.push(part(&fitted.url, &provider_owned));
            images.push(stored);
        }
        Ok(TurnImages { images, parts, provider: provider_owned })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(id: &str) -> ChatImage {
        ChatImage { id: id.into(), w: 10, h: 10, bytes: 100, source: "snip".into(), gone: false }
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
        let turn = TurnImages { images: vec![img("img_0123456789abcdef")], parts: vec![part("data:x", "gemini")], provider: "gemini".into() };
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
        // Three: d, c, b (newest first); a is said in a line; the removed one too.
        assert_eq!(resend_plan(&earlier, 3, &exists), vec![(4, 0), (2, 0), (0, 1)]);
        let mut messages: Vec<Value> =
            earlier.iter().map(|m| json!({ "role": m.role, "content": m.text })).collect();
        // A replayed web search between them (assistant + tool) changes nothing.
        messages.insert(1, json!({ "role": "tool", "content": "{}" }));
        history_images(&mut messages, &earlier, "anthropic", 3, &exists, &|id| Some(format!("data:{id}")));
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
    }
}
