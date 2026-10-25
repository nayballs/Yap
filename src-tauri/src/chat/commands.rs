//! The Chat view's commands: its conversations, and a question sent
//! ([`chat_send`], answered by `turn::chat_turn`).

use tauri::AppHandle;

use crate::chat::turn::{chat_turn, resolve_chat_endpoint};
use crate::config;

/// Conversation summaries for the chat sidebar.
#[tauri::command]
pub fn chats_list() -> serde_json::Value {
    crate::chat::chats::list()
}

/// One conversation with its messages.
#[tauri::command]
pub fn chat_get(id: u64) -> Result<crate::chat::chats::Conversation, String> {
    crate::chat::chats::get(id).map(crate::chat::chats::for_page).ok_or_else(|| "Conversation not found".to_string())
}

/// Delete a conversation. A save that failed is the error (saving.rs):
/// until chats.json is saved, it would come back.
#[tauri::command]
pub fn chat_delete(id: u64) -> Result<(), String> {
    crate::chat::chats::delete(id);
    // A later conversation may get its id: nothing of this one carries over.
    crate::apps::moved_on(Some(id));
    Ok(crate::chat::chats::saved()?)
}

/// Several conversations at once (the Chat list's selection, "Delete all
/// chats…"): one write. Returns how many were deleted.
#[tauri::command]
pub fn chat_delete_many(ids: Vec<u64>) -> Result<usize, String> {
    let gone = crate::chat::chats::delete_many(&ids);
    for id in ids {
        crate::apps::moved_on(Some(id));
    }
    crate::chat::chats::saved()?;
    Ok(gone)
}

/// Rename a conversation (the Chat sidebar's Rename, in place): the title as
/// stored (one line, trimmed, ≤ 80 characters). Its place in the list stays.
#[tauri::command]
pub fn chat_rename(id: u64, title: String) -> Result<String, String> {
    let title = crate::chat::chats::rename(id, &title)?;
    crate::chat::chats::saved()?;
    Ok(title)
}

/// Send one chat turn from the Chat view ([`chat_turn`]). Returns `{
/// conversationId, reply, toolsUsed, model, sources, messageId,
/// userMessageId, speak, stopped }`: the two messages' ids ("chat:<conversation>:<index>", what the page
/// calls them) and whether the answer is read out by itself
/// (`tts::auto_speak`: spoken replies on, and "When to speak" says so —
/// `voice`: the question was dictated, kept with it).
///
/// `onStream`: the page's channel — the answer as it's written (`stream.rs`
/// [`crate::chat::stream::Event`]: `{kind: "text", text}`, `{kind: "tool", name,
/// doing}`, `{kind: "chips", chips, sources, web}`, `{kind: "waiting",
/// waiting}`, plus `{kind: "turn", conversationId}` once the question is
/// saved); `reply` is then the checked answer to put in its place.
/// `turnId`: what `stream_stop` stops it by (the ■, or `superseded`: the page
/// sends a newer question next). The error [`crate::chat::stream::SUPERSEDED`]: a
/// newer question in the conversation replaced it — no answer saved, the
/// question kept.
///
/// `images`: the screenshots the question carries (Snip to Chat: ids from
/// `yap-snip`), saved with it and sent with every model call of the turn
/// as `image_url` parts fitted to the provider (`snip::turn`). A model
/// that can't read images is refused before anything is saved
/// (`snip::vision::CANT_READ`); one whose server turns the image down is
/// remembered as such and the error says so (`snip::vision::COULDNT_READ`).
/// `asText`: "Send its text instead" — the screenshots' text, read on this
/// PC by Windows' OCR (`snip::ocr`), goes after the question in place of
/// the pictures (any model takes it); the screenshots stay with the
/// question, marked so. The reply's `question` is the question as saved.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    conversation_id: Option<u64>,
    text: String,
    scope: Option<String>,
    voice: Option<bool>,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
    images: Option<Vec<String>>,
    as_text: Option<bool>,
) -> Result<serde_json::Value, String> {
    let by_voice = voice.unwrap_or(false);
    let stop = crate::chat::stream::Stoppable::new(turn_id.as_deref());
    let watched = on_stream.0.is_some();
    // Screenshots: the model must be able to read them, and their copies
    // for it are made (usually already, while the question was typed).
    let image_ids = images.unwrap_or_default();
    let mut text = text;
    let (turn_images, target) = if image_ids.is_empty() {
        (crate::snip::turn::TurnImages::default(), None)
    } else if as_text.unwrap_or(false) {
        // Their text instead: read here, after the question; no pictures go.
        let (question, saved) = crate::snip::ocr::question_with_text(&image_ids, &text).await?;
        text = question;
        (crate::snip::turn::TurnImages { images: saved, ..Default::default() }, None)
    } else {
        let cfg = config::load();
        let (base_url, _key, model, provider, _persona, _thinking) = resolve_chat_endpoint(&cfg)?;
        if crate::snip::vision::reads_images(&provider, &model) == Some(false) {
            return Err(format!("{} ({model})", crate::snip::vision::CANT_READ));
        }
        let prepared = crate::snip::turn::prepare(&image_ids, &provider, &base_url).await?;
        tracing::info!(images = prepared.images.len(), "Chat: a question with screenshots");
        (prepared, Some((provider, model)))
    };
    let turned = crate::snip::turn::with(
        turn_images,
        chat_turn(
            Some(&app),
            conversation_id,
            &text,
            scope.as_deref(),
            by_voice,
            |conv, index| {
                // The page learns its conversation (a new one) as soon as it's made.
                on_stream.send_json(serde_json::json!({
                    "kind": "turn",
                    "conversationId": conv,
                    "userMessageId": format!("chat:{conv}:{index}"),
                }));
            },
            watched.then_some(&on_stream as &dyn crate::chat::stream::Sink),
            Some(&stop),
        ),
    )
    .await;
    let turn = match (turned, &target) {
        (Ok(turn), _) => turn,
        // The server turned the image down: that model is a "no" from now
        // on, and the page offers a switch instead of a plain error.
        (Err(e), Some((provider, model))) if crate::snip::vision::is_image_refusal(&e) => {
            tracing::info!(provider, "Chat: the model refused the screenshot");
            crate::snip::vision::refused(provider, model);
            return Err(format!("{} ({model})", crate::snip::vision::COULDNT_READ));
        }
        (Err(e), _) => return Err(e),
    };
    // Read out by itself? (Asked at the answer: a dictation may have started
    // meanwhile, and the settings may have changed.)
    let now = config::load();
    let speak = crate::voice::tts::auto_speak(
        now.speak_replies,
        &now.speak_when,
        by_voice,
        crate::dictation::pipeline::dictation_state().1,
        // A meeting recording or a call: quiet (`tts::meeting_quiet`).
        crate::voice::tts::meeting_quiet().is_some(),
    );
    Ok(serde_json::json!({
        "conversationId": turn.conversation_id,
        "reply": turn.reply,
        "toolsUsed": turn.tools_used,
        "model": turn.model,
        "sources": turn.sources,
        "messageId": turn.message_id(),
        "userMessageId": format!("chat:{}:{}", turn.conversation_id, turn.user_index),
        "speak": speak && !turn.stopped,
        "stopped": turn.stopped,
        "question": text,
    }))
}
