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
    // A later conversation may get its id: nothing of this one carries over
    // (nor its Claude Code session, on your Claude plan).
    crate::apps::moved_on(Some(id));
    crate::ai::claude_plan::sessions::forget(id);
    Ok(crate::chat::chats::saved()?)
}

/// Several conversations at once (the Chat list's selection, "Delete all
/// chats…"): one write. Returns how many were deleted.
#[tauri::command]
pub fn chat_delete_many(ids: Vec<u64>) -> Result<usize, String> {
    let gone = crate::chat::chats::delete_many(&ids);
    for id in ids {
        crate::apps::moved_on(Some(id));
        crate::ai::claude_plan::sessions::forget(id);
    }
    crate::chat::chats::saved()?;
    Ok(gone)
}

/// The archived conversations (the Chat sidebar's "Archived"), archived
/// last first.
#[tauri::command]
pub fn chats_archived() -> serde_json::Value {
    crate::chat::chats::list_archived()
}

/// Archive conversations (out of the list, kept and still searched) or,
/// with `archived: false`, bring them back. One write; returns how many
/// changed.
#[tauri::command]
pub fn chat_archive(ids: Vec<u64>, archived: Option<bool>) -> Result<usize, String> {
    let changed = crate::chat::chats::set_archived(&ids, archived.unwrap_or(true));
    crate::chat::chats::saved()?;
    Ok(changed)
}

/// Rename a conversation (the Chat sidebar's Rename, in place): the title as
/// stored (one line, trimmed, ≤ 80 characters). Its place in the list stays.
/// A connected phone hears it (Yap Link's drawer and header).
#[tauri::command]
pub fn chat_rename(id: u64, title: String) -> Result<String, String> {
    let title = crate::chat::chats::rename(id, &title)?;
    crate::chat::chats::saved()?;
    crate::link::titled(id, &title);
    Ok(title)
}

/// Send one chat turn from the Chat view ([`chat_turn`]). Returns `{
/// conversationId, reply, toolsUsed, model, sources, messageId,
/// userMessageId, speak, stopped, tokens }` (`tokens`: the answer's last
/// model call, `{prompt, answer, perSecond?}`, when it said): the two messages' ids ("chat:<conversation>:<index>", what the page
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
/// `skill`: a Skill's command (`/decisions`, a one-tap prompt's), run
/// across the meetings (`chat/skills.rs`).
///
/// `think`: Chat's Think button — this one question thought through
/// (`chat/think.rs`): the model's thinking streams as `{kind: "thinking",
/// text}`, and the reply's `thinking` (`{text, ms}`) is kept with the
/// answer. A strong model writing a document thinks without it: `{kind:
/// "think", auto: true}` says so (the button shows lit for the turn).
///
/// `selection`: the passage of a document the question is about (the
/// document panel's "Change this…" / "Ask about this": `{noteId, text,
/// fromLine?, toLine?}`, `documents::Selection`) — the model is told which
/// blocks it is, and the question keeps its quote.
///
/// `pageQuote`: text selected on a page beside the chat and asked about
/// (its Page tab's "Ask about this", `chat/pages/view.rs`: `{text, site}`)
/// — asked as text selected in another app is (`assistant/selected.rs`),
/// its site named.
///
/// `codeFolder`: a new chat made a Code chat before its first question (a
/// workspace's name or path): the conversation it makes is attached to it
/// as it's made (`jobs::code_chat`). Ignored for an existing conversation.
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
    skill: Option<String>,
    think: Option<bool>,
    selection: Option<crate::chat::documents::Selection>,
    page_quote: Option<crate::chat::pages::view::PageQuote>,
    code_folder: Option<String>,
) -> Result<serde_json::Value, String> {
    // A Skill (`/decisions`, a one-tap prompt): asked across the meetings,
    // its own passages and instructions (chat/skills.rs).
    let skill_turn = match skill.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(command) => Some(crate::chat::skills::turn_for(command, &text)?),
        None => None,
    };
    // The screenshots it sends leave Chat's box (`snip/waiting.rs`); a
    // question that fails before its answer gives them back, as the page
    // gives the question back (not one a newer question replaced, nor one
    // stopped: those are kept as asked).
    let image_ids = images.unwrap_or_default();
    let taken = if image_ids.is_empty() { Vec::new() } else { crate::snip::waiting::sent(&app, &image_ids) };
    // A passage selected in the document panel ("Change this…", documents.rs):
    // this question takes it, so a spoken one can't too.
    if selection.is_some() {
        crate::chat::documents::quote_sent();
    }
    let turn = send_turn(&app, conversation_id, text, scope, voice, on_stream, turn_id, image_ids, as_text);
    let send = crate::chat::skills::with(skill_turn, crate::chat::documents::with_selection(selection, turn));
    // Text selected on a page beside the chat: what the question is about.
    // (Boxed: the command's future is made on the main thread, whose stack
    // a turn's whole future overflows.)
    let send = Box::pin(crate::chat::assistant::selected::with(page_quote.and_then(|q| q.selected()), Box::pin(send)));
    // A new chat made a Code chat before its first question (jobs/code_chat.rs).
    let send = Box::pin(crate::jobs::code_chat::with_new(code_folder.filter(|_| conversation_id.is_none()), send));
    // Chat's own turn: Think pressed (the model on this PC), or a cloud
    // model at its effort (`ai/effort.rs`): what it thought comes back.
    let (out, thought) = crate::chat::think::scoped(think.unwrap_or(false), send).await;
    // Thought through: its thinking kept with the answer, and on the reply.
    let out = match (out, thought) {
        (Ok(mut reply), Some(thought)) => {
            let at = (reply["conversationId"].as_u64(), reply["messageId"].as_str().and_then(|id| id.rsplit(':').next()?.parse::<usize>().ok()));
            if let (Some(conv), Some(index)) = at {
                tracing::info!(chars = thought.text.chars().count(), ms = thought.ms, "Chat: an answer thought through");
                crate::chat::chats::set_thinking(conv, index, thought.clone());
            }
            reply["thinking"] = serde_json::to_value(&thought).unwrap_or_default();
            Ok(reply)
        }
        (out, _) => out,
    };
    if let Err(e) = &out {
        if !e.contains(crate::chat::stream::SUPERSEDED) && !e.contains(crate::chat::stream::STOPPED) {
            crate::snip::waiting::put_back(&app, conversation_id, taken);
        }
    }
    out
}

/// A question's screenshots, ready to go with its turn ([`question_images`]).
pub(crate) struct QuestionImages {
    pub images: crate::snip::turn::TurnImages,
    /// The model they go to, while it isn't known to read them (a refusal
    /// makes it a "no"); none when their text goes instead.
    pub target: Option<(String, String)>,
    /// The question: with their text after it when that goes instead.
    pub text: String,
}

/// **A question's screenshots (or pictures) as Chat sends them**: Chat's
/// typed question's ([`chat_send`]) and a phone's pictures (Yap Link,
/// `assistant::answer`) — one way for both. The model must be able to read
/// them (`CANT_READ`, else the on-device model's projector loaded first:
/// `COULDNT_READ` when it can't), and their copies for it are made (usually
/// already, while the question was typed); or, `as_text`, their text read on
/// this PC goes after the question and no picture goes ("Send its text
/// instead", `snip::ocr`).
pub(crate) async fn question_images(app: &AppHandle, ids: &[String], as_text: bool, text: String) -> Result<QuestionImages, String> {
    if ids.is_empty() {
        return Ok(QuestionImages { images: crate::snip::turn::TurnImages::default(), target: None, text });
    }
    if as_text {
        // Their text instead: read here, after the question; no pictures go.
        let (question, saved) = crate::snip::ocr::question_with_text(ids, &text).await?;
        return Ok(QuestionImages { images: crate::snip::turn::TurnImages { images: saved, ..Default::default() }, target: None, text: question });
    }
    let cfg = config::load();
    let (base_url, _key, model, provider, _persona, _thinking) = resolve_chat_endpoint(&cfg)?;
    if crate::snip::vision::reads_images(&provider, &model) == Some(false) {
        return Err(format!("{} ({model})", crate::snip::vision::CANT_READ));
    }
    // The on-device model loads its projector first: started with it, or
    // restarted with it once no dictation or answer uses the model
    // (local_llm/vision.rs). It doesn't: the page's line, as for a refusal.
    if provider == crate::ai::local_llm::PROVIDER_ONDEVICE {
        if let Err(e) = crate::ai::local_llm::vision::ensure(app).await {
            tracing::info!("Chat: the on-device model can't read the screenshot now ({e})");
            return Err(format!("{} ({model})", crate::snip::vision::COULDNT_READ));
        }
    }
    let prepared = crate::snip::turn::prepare(ids, &provider, &base_url).await?;
    tracing::info!(images = prepared.images.len(), "Chat: a question with screenshots");
    Ok(QuestionImages { images: prepared, target: Some((provider, model)), text })
}

/// [`chat_send`]'s turn.
#[allow(clippy::too_many_arguments)]
async fn send_turn(
    app: &AppHandle,
    conversation_id: Option<u64>,
    text: String,
    scope: Option<String>,
    voice: Option<bool>,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
    image_ids: Vec<String>,
    as_text: Option<bool>,
) -> Result<serde_json::Value, String> {
    let by_voice = voice.unwrap_or(false);
    let stop = crate::chat::stream::Stoppable::new(turn_id.as_deref());
    let watched = on_stream.0.is_some();
    let QuestionImages { images: turn_images, target, text } = question_images(app, &image_ids, as_text.unwrap_or(false), text).await?;
    let turned = crate::snip::turn::with(
        turn_images,
        chat_turn(
            Some(app),
            conversation_id,
            &text,
            scope.as_deref(),
            by_voice,
            |conv, index| {
                // A new chat the page made a Code chat: attached as it's made.
                crate::jobs::code_chat::adopt_new(conv);
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
        // What's said: as much as "Read aloud" says (`assistant::SpokenLength`).
        "spokenText": (speak && !turn.stopped)
            .then(|| crate::chat::assistant::spoken_in_chat(&turn.reply, crate::chat::assistant::SpokenLength::configured(&now))),
        "stopped": turn.stopped,
        "question": text,
        "tokens": turn.tokens,
        // The documents it made or changed (documents.rs): their pills.
        "documents": turn.documents,
        // The pictures it showed beside the chat (pictures.rs): their pills.
        "pictures": turn.pictures,
        // "Search in your browser", when Yap's free web sources couldn't
        // answer it (web/sources.rs), as saved with the answer.
        "browserSearch": crate::chat::chats::browser_search_of(turn.conversation_id, turn.index),
        // The pages it brought up (pages.rs): their tabs; its buttons.
        "pages": turn.pages,
        "offers": turn.offers,
    }))
}
