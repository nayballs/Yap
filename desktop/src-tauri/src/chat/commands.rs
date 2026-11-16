//! The Chat view's commands: its conversations, and a question sent
//! ([`chat_send`], answered by `turn::chat_turn`).

use tauri::AppHandle;

use crate::chat::turn::{chat_turn, resolve_chat_endpoint};
use crate::config;

/// Conversation summaries for the chat sidebar; a routine's own chat
/// marked `routine` (its Skill's command: chat/routines.rs).
#[tauri::command]
pub fn chats_list() -> serde_json::Value {
    let mut list = crate::chat::chats::list();
    crate::chat::routines::chat_marks(&mut list);
    list
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
    // (nor its Claude Code session, on your Claude plan, nor its workspace).
    crate::apps::moved_on(Some(id));
    crate::ai::claude_plan::sessions::forget(id);
    crate::agents::code_chat::chat_deleted(id);
    // Its own connectors' switches (the + menu's Connectors ▸) go with it,
    // and its style (chat/styles.rs).
    crate::integrations::connectors::chats::chat_deleted(id);
    crate::chat::styles::chats_deleted(&[id]);
    crate::chat::projects::chats_deleted(&[id]);
    // A routine's own chat: its next run makes another (chat/routines.rs).
    crate::chat::routines::chats_deleted(&[id]);
    Ok(crate::chat::chats::saved()?)
}

/// Several conversations at once (the Chat list's selection, "Delete all
/// chats…"): one write. Returns how many were deleted.
#[tauri::command]
pub fn chat_delete_many(ids: Vec<u64>) -> Result<usize, String> {
    let gone = crate::chat::chats::delete_many(&ids);
    crate::integrations::connectors::chats::chats_deleted(&ids);
    crate::chat::styles::chats_deleted(&ids);
    crate::chat::projects::chats_deleted(&ids);
    crate::chat::routines::chats_deleted(&ids);
    for id in ids {
        crate::apps::moved_on(Some(id));
        crate::ai::claude_plan::sessions::forget(id);
        crate::agents::code_chat::chat_deleted(id);
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

/// **The tabs closed in a conversation's panel** (14 Nov 2026): kept with
/// it, so they stay closed when it opens again (`chats::set_closed_tabs`).
#[tauri::command]
pub fn chat_tabs_closed(id: u64, keys: Vec<String>) -> Result<(), String> {
    if crate::chat::chats::set_closed_tabs(id, &keys)? {
        crate::chat::chats::saved()?;
    }
    Ok(())
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
/// as it's made (`agents::code_chat`). Ignored for an existing conversation.
///
/// `connectorsOff`: the connectors a new chat's + menu switched off before
/// its first question (Connectors ▸, `connectors/chats.rs`): this turn
/// offers none of theirs, and the conversation it makes keeps them off.
/// Ignored for an existing conversation (its own switches are saved as
/// they're flipped, `connector_chat_set`).
///
/// `files`: the files attached in Chat's box (`chat/attachments.rs`: ids
/// from `chat_files_add`), their text sent before the question and kept
/// with it; one gone since is said, nothing sent.
///
/// `edit`: **Edit** (15 Nov 2026, the Claude app's) — the person's message
/// at that index replaced by `text`: in Chat, it and everything after it
/// set aside as a branch (‹ 1/2 › on the message, `chats/branches.rs`), back
/// as it was when the new words never get an answer; in a Code chat, a
/// Rewind to before it (`agents/rewind.rs`, `editFiles`: the files Claude
/// changed since go back too), then sent. Either way its screenshots and the
/// passage it was about go again; the reply's `forks` are the chat's ‹ ›.
///
/// `incognito`: a new chat's ghost was on (chat/incognito.rs): the
/// conversation it makes is kept in memory only. Ignored for an existing
/// conversation (it is one or it isn't, from its start).
///
/// `project`: a new chat started from a project's page (chat/projects.rs):
/// the conversation it makes is in that project. Ignored for an existing one
/// (its ⋯ moves it: `chat_project_set`).
///
/// `feedbackFrom` (with `edit`; 16 Nov 2026, a Code chat's Retry with the
/// note on 👎's card): that answer's 👎 goes to Claude after the message,
/// this turn only (`chat/feedback.rs`); the message is saved as it is.
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
    elements: Option<Vec<crate::chat::pages::pick::Picked>>,
    code_folder: Option<String>,
    connectors_off: Option<Vec<String>>,
    files: Option<Vec<String>>,
    edit: Option<usize>,
    edit_files: Option<bool>,
    incognito: Option<bool>,
    project: Option<String>,
    feedback_from: Option<usize>,
) -> Result<serde_json::Value, String> {
    // The files attached in the box (chat/attachments.rs): each still there.
    let files = crate::chat::attachments::taken(&files.unwrap_or_default())?;
    // A Retry with its 👎's note (an edit with the same words): read before the edit rewinds past the answer.
    let note = match (edit, conversation_id, feedback_from) {
        (Some(_), Some(id), Some(answer)) => crate::chat::feedback::retry_note_at(id, answer),
        _ => None,
    };
    // A Skill (`/decisions`, a one-tap prompt): asked across the meetings,
    // its own passages and instructions (chat/skills.rs).
    let skill_turn = match skill.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(command) => Some(crate::chat::skills::turn_for(command, &text)?),
        None => None,
    };
    // An Edit: the message it replaces set aside (Chat) or rewound (a Code chat) first
    // (after the last check that can refuse the message: nothing to put back before it).
    let editing = match (edit, conversation_id) {
        (Some(at), Some(id)) => Some(begin_edit(id, at, edit_files.unwrap_or(false)).await?),
        (Some(_), None) => return Err("That chat isn't there any more.".to_string()),
        (None, _) => None,
    };
    // (An edit's files: as its message had them, unless new ones came with it.)
    let files = match &editing {
        Some(e) if files.is_empty() => e.question.attachments.clone(),
        _ => files,
    };
    let (images, as_text, selection, page_quote) = match &editing {
        Some(e) => {
            let (ids, as_text) = images_again(&e.question);
            let (sel, page) = quote_again(&e.question);
            (Some(ids), as_text.then_some(true), selection.or(sel), page_quote.or(page))
        }
        None => (images, as_text, selection, page_quote),
    };
    // The screenshots it sends leave Chat's box (`snip/waiting.rs`); a
    // question that fails before its answer gives them back, as the page
    // gives the question back (not one a newer question replaced, nor one
    // stopped: those are kept as asked).
    // Elements picked on a page beside the chat (`pick.rs`): their blocks after the words, for the model.
    let text = crate::chat::pages::pick::with_elements(&text, &elements.unwrap_or_default());
    let image_ids = images.unwrap_or_default();
    let taken = if image_ids.is_empty() { Vec::new() } else { crate::snip::waiting::sent(&app, &image_ids) };
    // A passage selected in the document panel ("Change this…", documents.rs):
    // this question takes it, so a spoken one can't too.
    if selection.is_some() {
        crate::chat::documents::quote_sent();
    }
    // Where each came from in the box (a page's picked element, an annotated page): kept on the question.
    // (An edit's: as its message had them.)
    let sources: Vec<(String, String)> = match &editing {
        Some(e) => e.question.images.iter().map(|i| (i.id.clone(), i.source.clone())).collect(),
        None => taken.iter().map(|i| (i.id.clone(), i.source.clone())).collect(),
    };
    let turn = send_turn(&app, conversation_id, text, scope, voice, on_stream, turn_id, image_ids, sources, as_text);
    let turn = crate::chat::attachments::with(files, turn);
    let send = crate::chat::skills::with(skill_turn, crate::chat::documents::with_selection(selection, turn));
    // Text selected on a page beside the chat: what the question is about.
    // (Boxed: the command's future is made on the main thread, whose stack
    // a turn's whole future overflows.)
    let send = Box::pin(crate::chat::assistant::selected::with(page_quote.and_then(|q| q.selected()), Box::pin(send)));
    // A new chat made a Code chat before its first question (agents/code_chat.rs).
    let code_folder_none = code_folder.as_deref().is_none_or(|f| f.trim().is_empty());
    // (Its type erased here: the layers' nesting stays under the compiler's
    // recursion limit for proving the command's future is Send.)
    let send: std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>> =
        Box::pin(crate::agents::code_chat::with_new(code_folder.filter(|_| conversation_id.is_none()), send));
    // A new chat's connectors switched off in its + menu (connectors/chats.rs).
    let new_off = conversation_id.is_none().then(|| connectors_off.unwrap_or_default().into_iter().collect());
    let send = Box::pin(crate::integrations::connectors::chats::with_new(new_off, send));
    // A new chat asked incognito (never a Code chat: its folder wins).
    let ghost = conversation_id.is_none() && code_folder_none && incognito.unwrap_or(false);
    let send = Box::pin(crate::chat::incognito::with_new(ghost, send));
    // A new chat started in a project (its page's New chat: chat/projects.rs);
    // never an incognito or a Code chat.
    let in_project = project.filter(|p| conversation_id.is_none() && code_folder_none && !ghost && !p.trim().is_empty());
    let send = Box::pin(crate::chat::projects::with_new(in_project, send));
    // A Retry with its 👎's note: given to the model (or the Code chat's session) this turn only.
    let send = Box::pin(crate::chat::feedback::with_note(note, send));
    // Chat's own turn: Think pressed (the model on this PC), or a cloud
    // model at its effort (`ai/effort.rs`): what it thought comes back.
    let (out, thought) = crate::chat::think::scoped(think.unwrap_or(false), send).await;
    let out = keep_thought(out, thought);
    if let Err(e) = &out {
        if !e.contains(crate::chat::stream::SUPERSEDED) && !e.contains(crate::chat::stream::STOPPED) {
            crate::snip::waiting::put_back(&app, conversation_id, taken);
        }
    }
    // An edit's branch kept — answered, or stopped or replaced as any
    // question is — or, never answered, back to the message as it was.
    let out = match (editing.and_then(|e| e.begun), conversation_id) {
        (Some(b), Some(id)) => {
            let kept = out.as_ref().map_or_else(|e| e.contains(crate::chat::stream::SUPERSEDED) || e.contains(crate::chat::stream::STOPPED), |_| true);
            if crate::chat::chats::end_branch(id, b.at, b.back_to, kept) {
                crate::memory::read_again_from(id, b.at);
            }
            with_forks(out, id)
        }
        _ => out,
    };
    // A Yap Lab run keeps how the turn ended (shell/lab.rs).
    crate::shell::lab::sent(conversation_id, &out);
    out
}

/// **Thought through** (Think, or a cloud model at its effort): its
/// thinking kept with the answer, and on the reply.
fn keep_thought(out: Result<serde_json::Value, String>, thought: Option<crate::chat::think::Thought>) -> Result<serde_json::Value, String> {
    match (out, thought) {
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
    }
}

// ---- Retry, Edit and ‹ › (15 Nov 2026, the Claude app's message actions: chat/chats/branches.rs) ----

/// Said while a conversation is answering: Retry, Edit and ‹ › wait for it.
const WAIT_FOR_ANSWER: &str = "Wait for the answer being written, or stop it first.";

/// What an Edit set going: in Chat, the branch to go back to
/// (`chats::begin_edit`); in a Code chat, none (it rewound). And the message
/// it replaces.
struct Editing {
    begun: Option<crate::chat::chats::Begun>,
    question: crate::chat::chats::ChatMessage,
}

/// **An Edit begun** on conversation `id`'s message `at`: in Chat, it and
/// what follows set aside as a branch; in a Code chat, Claude's own way —
/// Rewound to before it (`agents/rewind.rs`; `files`: the files Claude
/// changed since too), its screenshots kept to go again.
async fn begin_edit(id: u64, at: usize, files: bool) -> Result<Editing, String> {
    if crate::chat::stream::is_answering(id) {
        return Err(WAIT_FOR_ANSWER.to_string());
    }
    if crate::agents::code_chat::folder_of(Some(id)).is_some() {
        let question = tauri::async_runtime::spawn_blocking(move || crate::agents::rewind::rewind_for_edit(id, at, files)).await.map_err(|e| e.to_string())??;
        return Ok(Editing { begun: None, question });
    }
    let begun = crate::chat::chats::begin_edit(id, at)?;
    tracing::info!(conversation = id, at, "Chat: a message edited (its branch set aside)");
    Ok(Editing { question: begun.question.clone(), begun: Some(begun) })
}

/// A message's screenshots, to go again (Retry, Edit): those still on this
/// PC; and whether they went as their text (read again on this PC). Pure
/// but for the files.
fn images_again(m: &crate::chat::chats::ChatMessage) -> (Vec<String>, bool) {
    let here: Vec<&crate::chat::chats::ChatImage> = m.images.iter().filter(|i| crate::snip::images::exists(&i.id)).collect();
    let as_text = here.iter().any(|i| i.as_text);
    (here.into_iter().map(|i| i.id.clone()).collect(), as_text)
}

/// The passage a message was asked about, to go again with an edit: a
/// document's, or text selected elsewhere (as web content: what it was
/// isn't kept). Pure.
fn quote_again(m: &crate::chat::chats::ChatMessage) -> (Option<crate::chat::documents::Selection>, Option<crate::chat::pages::view::PageQuote>) {
    match &m.quote {
        Some(q) if q.note_id != 0 => (Some(crate::chat::documents::Selection { note_id: q.note_id, text: q.text.clone(), from_line: None, to_line: None }), None),
        Some(q) => (None, Some(crate::chat::pages::view::PageQuote { text: q.text.clone(), site: q.app.clone() })),
        None => (None, None),
    }
}

/// The reply with the chat's ‹ › now (`forks`, `chats/branches.rs`).
fn with_forks(out: Result<serde_json::Value, String>, id: u64) -> Result<serde_json::Value, String> {
    out.map(|mut reply| {
        let forks = crate::chat::chats::get(id).map(|c| crate::chat::chats::branches::views(&c)).unwrap_or_default();
        reply["forks"] = serde_json::to_value(forks).unwrap_or_default();
        reply
    })
}

/// A Skill's question ("/decisions last week") run again as that Skill.
fn skill_again(text: &str) -> Option<crate::chat::skills::SkillTurn> {
    let command = text.trim().strip_prefix('/')?.split_whitespace().next()?;
    crate::chat::skills::turn_for(command, text).ok()
}

/// **Retry** (15 Nov 2026, the Claude app's): conversation `conversationId`'s
/// answer `index` set aside as a branch with everything after it
/// (`chats::begin_retry`), and the person's question before it answered
/// again — its screenshots, the passage it was about and the Skill it ran
/// with it; ‹ 2/2 › on the new answer switches back. Streamed like
/// [`chat_send`] (`onStream`, `turnId` for ■), the same reply, with the
/// chat's `forks`. An answer that never comes (an error, ■ before any of
/// it) puts the old one back. Not in a Code chat: there Retry is an Edit
/// with the same words (Claude Code's own Rewind, `chat_send`'s `edit`).
///
/// `feedback` (16 Nov 2026, 👎's card: Retry with this note): the answer's
/// 👎 — what was wrong, the person's words, the earlier answer for
/// reference — goes to the model with the question, this turn only
/// (`chat/feedback.rs`). The question stays as it was.
#[tauri::command]
pub async fn chat_retry(
    app: AppHandle,
    conversation_id: u64,
    index: usize,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
    feedback: Option<bool>,
) -> Result<serde_json::Value, String> {
    if crate::chat::stream::is_answering(conversation_id) {
        return Err(WAIT_FOR_ANSWER.to_string());
    }
    if crate::agents::code_chat::folder_of(Some(conversation_id)).is_some() {
        return Err("In a Code chat, Retry sends the message again from a Rewind.".to_string());
    }
    // (Its 👎's note: read before the answer is set aside.)
    let note = if feedback == Some(true) { crate::chat::feedback::retry_note_at(conversation_id, index) } else { None };
    let begun = crate::chat::chats::begin_retry(conversation_id, index)?;
    tracing::info!(conversation = conversation_id, index, "Chat: an answer retried (the old one set aside)");
    let question = &begun.question;
    let (image_ids, as_text) = images_again(question);
    // (A screenshot that went as its text: that text is in its words already.)
    let image_ids = if as_text { Vec::new() } else { image_ids };
    // (Its words, and its attached files through the turn's own: chat/attachments.rs.)
    let turn = send_turn(&app, Some(conversation_id), question.sent_words(), None, Some(question.voice), on_stream, turn_id, image_ids, Vec::new(), None);
    let turn = crate::chat::attachments::with(question.attachments.clone(), turn);
    let send = crate::chat::skills::with(skill_again(&question.text), turn);
    let send = Box::pin(crate::chat::feedback::with_note(note, crate::chat::turn::again(conversation_id, begun.question_at, send)));
    let (out, thought) = crate::chat::think::scoped(false, send).await;
    let out = keep_thought(out, thought);
    if crate::chat::chats::end_branch(conversation_id, begun.at, begun.back_to, true) {
        crate::memory::read_again_from(conversation_id, begun.at);
    }
    let out = with_forks(out, conversation_id);
    crate::shell::lab::sent(Some(conversation_id), &out);
    out
}

/// **‹ › on a message** (Retry's and Edit's versions): conversation `id`'s
/// fork at message `at` shows its version in place `place` (1…), what was
/// shown from there kept as a version itself. The conversation as `chat_get`
/// gives it. Not while it's answering.
#[tauri::command]
pub fn chat_branch(id: u64, at: usize, place: u32) -> Result<crate::chat::chats::Conversation, String> {
    if crate::chat::stream::is_answering(id) {
        return Err(WAIT_FOR_ANSWER.to_string());
    }
    let conv = crate::chat::chats::switch_branch(id, at, place)?;
    crate::memory::read_again_from(id, at);
    Ok(conv)
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

/// A question's pictures keep the source they had in Chat's box ("element",
/// "annotation", "snip"…): the stored copies only know they're pictures.
fn keep_sources(images: &mut [crate::chat::chats::ChatImage], sources: &[(String, String)]) {
    for image in images {
        if let Some((_, source)) = sources.iter().find(|(id, s)| *id == image.id && !s.is_empty()) {
            image.source = source.clone();
        }
    }
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
    sources: Vec<(String, String)>,
    as_text: Option<bool>,
) -> Result<serde_json::Value, String> {
    let by_voice = voice.unwrap_or(false);
    let stop = crate::chat::stream::Stoppable::new(turn_id.as_deref());
    let watched = on_stream.0.is_some();
    let QuestionImages { images: mut turn_images, target, text } = question_images(app, &image_ids, as_text.unwrap_or(false), text).await?;
    // Each picture keeps its source from Chat's box: an element's shows in its
    // chip, never as a screenshot tile (pick.rs; before, all said "snip").
    keep_sources(&mut turn_images.images, &sources);
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
                crate::agents::code_chat::adopt_new(conv);
                // …and its connectors' switches, kept for it.
                crate::integrations::connectors::chats::adopt_new(conv);
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
    let mut shared = turn.files.clone();
    crate::chat::shares::for_page(&mut shared);
    Ok(serde_json::json!({
        "conversationId": turn.conversation_id,
        "reply": turn.reply,
        "toolsUsed": turn.tools_used,
        "model": turn.model,
        "sources": turn.sources,
        "messageId": turn.direct.is_none().then(|| turn.message_id()),
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
        // The files it shared into the conversation (shares.rs), as the page has them.
        "files": shared,
        // "Search in your browser", when Yap's free web sources couldn't
        // answer it (web/sources.rs), as saved with the answer.
        "browserSearch": crate::chat::chats::browser_search_of(turn.conversation_id, turn.index),
        // The pages it brought up (pages.rs): their tabs; its buttons.
        "pages": turn.pages,
        "offers": turn.offers,
        // Sent straight to the Code chat's session (agents/direct.rs): no
        // answer here — its work shows under the question, and its own
        // words come as its report.
        "direct": turn.direct,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::chats::ChatImage;

    #[test]
    fn a_question_keeps_its_pictures_sources_from_the_box() {
        let stored = |id: &str| ChatImage { id: id.into(), w: 4, h: 4, bytes: 10, source: "snip".into(), ..Default::default() };
        let mut images = vec![stored("a"), stored("b"), stored("c")];
        let sources = vec![("a".to_string(), "element".to_string()), ("b".to_string(), String::new())];
        keep_sources(&mut images, &sources);
        let got: Vec<&str> = images.iter().map(|i| i.source.as_str()).collect();
        assert_eq!(got, ["element", "snip", "snip"]);
    }
}
