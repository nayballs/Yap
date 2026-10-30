//! **Transforms in notes** (2026-10-29): the saved transforms
//! (`dictation/transforms.rs`, the list in Settings → Commands → Transforms:
//! Polish, Shorter, Fix grammar only, Translate, Make it a prompt, your own)
//! run on a note — the text selected in My thoughts or the summary's box, or
//! the whole of either — from the Notes view's note menu or a right-click.
//!
//! The same model and guardrails as a transform anywhere: the Commands
//! scope's model (`pipeline::commands_model`: its own provider when it's set
//! up, else the cleanup model; on this PC or in the cloud) under
//! `llm::TRANSFORM_BASE_PROMPT` with the transform's instruction; its answer
//! tidied the same way (`transforms::clean_output`). The result goes in
//! place under the notes store's lock ([`put_in_place`]: over the selection
//! if it's still there — or, moved, the one place it is now — keeping the
//! spaces around it; over the whole text if it's still what was sent), as a
//! version of its own (`versions.rs`, "Polish"), so Version history — or the
//! window's Undo, [`Transformed::previous`] — takes it back. A summary
//! transformed is the person's (`note_edits::apply_summary_edit`): Yap asks
//! before writing over it. Text changed while the model worked is left
//! alone, and the result is handed back to be copied.
//!
//! Any window showing a note can use it: `note_transform {id, transform,
//! field, selection, origin}` and `note_transforms` for the list; every
//! window follows the change (`yap-note-changed`). The meeting notepad's My
//! thoughts and the quick note's text box can call it with their note's id
//! and their box's selection (UTF-16 offsets, as a text box gives them).
//!
//! The log says which transform (a built-in's kind, or "custom"), the field,
//! the outcome and lengths: never the text.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::versions::{self, Field, How};
use crate::dictation::transforms;

/// The selection a transform works on, as a text box gives it: UTF-16
/// offsets into the field's text, and the text there.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// What a transform in a note did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transformed {
    /// The transform's name ("Polish").
    pub name: String,
    /// "replaced" | "unchanged" (the AI's answer was the text) | "moved"
    /// (the text changed meanwhile: nothing replaced; `result` to copy).
    pub outcome: String,
    /// The field's text now.
    pub text: String,
    /// Where the result is in it (UTF-16 offsets), to select it.
    pub start: usize,
    pub end: usize,
    /// The AI's result.
    pub result: String,
    /// The version to go back to (Undo).
    pub previous: Option<u64>,
}

/// Why nothing was replaced when the text changed while the model worked.
pub const MOVED: &str = "The note changed while Yap was working on it";

/// The byte offset of UTF-16 offset `unit` in `text`, when it falls between
/// characters.
pub fn byte_at(text: &str, unit: usize) -> Option<usize> {
    let mut u = 0;
    for (b, c) in text.char_indices() {
        if u == unit {
            return Some(b);
        }
        if u > unit {
            return None;
        }
        u += c.len_utf16();
    }
    (u == unit).then_some(text.len())
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `out` in place of `input` in `now` (pure): over the selection `sel`
/// (UTF-16 offsets) when it still holds `input` — else the one place it is
/// now — with the spaces it had around it; with no selection, over the
/// whole text when it's still `input`. The new text and where the result is
/// in it (UTF-16 offsets); `Err(MOVED)` when it can't be found.
pub fn put_in_place(now: &str, input: &str, sel: Option<(usize, usize)>, out: &str) -> Result<(String, usize, usize), String> {
    let range = match sel {
        None if now == input => 0..now.len(),
        None => return Err(MOVED.to_string()),
        Some((start, end)) => {
            let at = byte_at(now, start).zip(byte_at(now, end)).filter(|(a, b)| a <= b && &now[*a..*b] == input);
            match at {
                Some((a, b)) => a..b,
                None => {
                    let mut found = now.match_indices(input);
                    match (found.next(), found.next()) {
                        (Some((a, _)), None) if !input.is_empty() => a..a + input.len(),
                        _ => return Err(MOVED.to_string()),
                    }
                }
            }
        }
    };
    let put = crate::pasting::as_said::keep_spaces(input, out);
    let text = format!("{}{}{}", &now[..range.start], put, &now[range.end..]);
    let start = units(&now[..range.start]);
    Ok((text, start, start + units(&put)))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// What the log calls a transform: a built-in's kind, else "custom" (the
/// name of a person's own is theirs).
fn kind_of(t: &transforms::Transform) -> &str {
    if t.builtin.is_empty() {
        "custom"
    } else {
        &t.builtin
    }
}

/// The transforms a note offers (`{id, name}`, the person's order): the
/// list in Settings → Commands → Transforms.
#[tauri::command]
pub fn note_transforms() -> Vec<serde_json::Value> {
    transforms::list(&crate::config::load())
        .iter()
        .map(|t| serde_json::json!({ "id": t.id, "name": t.name() }))
        .collect()
}

/// Run transform `transform` on note `id`'s `field` ("notes": My thoughts,
/// the default; "summary"): on `selection`, or the whole text. See the
/// module notes. Every window follows (`yap-note-changed`; `origin`: the
/// window that asked, which shows the result itself).
#[tauri::command]
pub async fn note_transform(
    app: AppHandle,
    id: u64,
    transform: String,
    field: Option<String>,
    selection: Option<Selection>,
    origin: Option<String>,
) -> Result<Transformed, String> {
    let cfg = crate::config::load();
    let t = transforms::find(&cfg, &transform)
        .ok_or("That transform isn't in your list any more (Settings → Commands → Transforms)")?;
    let name = t.name();
    let field = Field::parse(field.as_deref().unwrap_or("notes")).ok_or("Transforms work on My thoughts or the summary")?;
    let note = crate::notes::get(id).ok_or("Note not found")?;
    let input = match &selection {
        Some(s) if s.text.trim().is_empty() => return Err("Select some text first".to_string()),
        Some(s) => s.text.clone(),
        None => field.of(&note).to_string(),
    };
    if input.trim().is_empty() {
        return Err(match field {
            Field::Notes => "There's nothing in this note to transform yet",
            Field::Summary => "There's no summary to transform yet",
        }
        .to_string());
    }

    // The model: the Commands scope's, else the cleanup model.
    let m = crate::dictation::pipeline::commands_model(&cfg);
    if m.base_url.is_empty() {
        let restarting = m.provider == crate::ai::local_llm::PROVIDER_ONDEVICE && crate::ai::local_llm::restart_under_way();
        return Err(if restarting {
            "The on-device AI model is restarting. Try again in a moment.".to_string()
        } else {
            "Set up an AI model to use transforms (Language Models)".to_string()
        });
    }
    if m.api_key.is_empty() && crate::ai::ai_model::KEYED_PROVIDERS.contains(&m.provider.as_str()) {
        return Err(format!("Transforms: no {} API key — add one in Language Models", m.provider));
    }
    let _sidecar = crate::ai::local_llm::hold(&m.base_url);
    let asked = crate::privacy::net::within(
        crate::privacy::net::Feature::Notes,
        crate::ai::usage::within(
            crate::ai::usage::Feature::Transforms,
            crate::ai::llm::transform(&t.instruction(), &input, &m.base_url, &m.api_key, &m.model, &m.provider, m.disable_thinking),
        ),
    )
    .await;
    let out = match asked {
        Ok(out) => transforms::clean_output(&out),
        Err(e) => {
            // Its kind at warn; the error itself (a server's can quote what
            // the model wrote) at debug only.
            tracing::warn!(transform = kind_of(&t), error = %crate::ai::llm::error_kind(&e), "A transform in a note failed");
            tracing::debug!("A transform in a note failed: {e}");
            return Err(format!("{name} didn't work: {e}"));
        }
    };
    if out.trim().is_empty() {
        return Err(format!("{name}: the AI gave back nothing"));
    }
    let sel = selection.as_ref().map(|s| (s.start, s.end));
    if crate::pasting::as_said::same_text(out.trim(), input.trim()) {
        let (start, end) = sel.unwrap_or((0, units(field.of(&note))));
        tracing::info!(transform = kind_of(&t), field = ?field, outcome = "unchanged", chars = out.chars().count(), "Transform in a note");
        return Ok(Transformed { name, outcome: "unchanged".into(), text: field.of(&note).to_string(), start, end, result: out, previous: None });
    }

    // In place, under the store's lock, as a version of its own.
    let now = now_ms();
    let placed = versions::by(How::named(versions::kind::TRANSFORM, &name), || {
        crate::notes::modify(id, |note| {
            let (text, start, end) = put_in_place(field.of(note), &input, sel, &out)?;
            let changed = match field {
                Field::Notes => {
                    note.content = text;
                    true
                }
                Field::Summary => super::note_edits::apply_summary_edit(note, &text, now),
            };
            Ok(((field.of(note).to_string(), start, end), changed))
        })
    });
    let (text, start, end) = match placed {
        Ok(p) => p,
        Err(e) if e == MOVED => {
            tracing::info!(transform = kind_of(&t), field = ?field, outcome = "moved", chars = out.chars().count(), "Transform in a note");
            let text = crate::notes::get(id).map(|n| field.of(&n).to_string()).unwrap_or_default();
            return Ok(Transformed { name, outcome: "moved".into(), text, start: 0, end: 0, result: out, previous: None });
        }
        Err(e) => return Err(e),
    };
    let origin = origin.as_deref().unwrap_or("transform");
    match field {
        Field::Notes => super::commands::note_changed(&app, id, origin),
        Field::Summary => super::note_edits::summary_changed(&app, id, origin),
    }
    tracing::info!(note_id = id, transform = kind_of(&t), field = ?field, outcome = "replaced", chars = out.chars().count(), "Transform in a note");
    crate::notes::saved()?;
    Ok(Transformed { name, outcome: "replaced".into(), text, start, end, result: out, previous: versions::undo_target(id, field) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_find_their_place_between_characters() {
        let t = "Café 🎉 ok";
        assert_eq!(byte_at(t, 0), Some(0));
        assert_eq!(byte_at(t, 4), Some(5), "é is one UTF-16 unit, two bytes");
        // The emoji is two units: its middle is no place.
        assert_eq!(byte_at(t, 5), Some(6));
        assert_eq!(byte_at(t, 6), None);
        assert_eq!(byte_at(t, 7), Some(10));
        assert_eq!(byte_at(t, units(t)), Some(t.len()));
        assert_eq!(byte_at(t, units(t) + 1), None);
    }

    #[test]
    fn a_result_goes_over_the_selection_with_its_spaces() {
        let now = "Notes:\nso basically we ship friday \nThanks";
        let sel = "so basically we ship friday ";
        let start = units("Notes:\n");
        let (text, a, b) = put_in_place(now, sel, Some((start, start + units(sel))), "We ship on Friday.").unwrap();
        assert_eq!(text, "Notes:\nWe ship on Friday. \nThanks");
        assert_eq!(&text[a..b], "We ship on Friday. ");
        // Moved since (a line typed above it): found where it is now, once.
        let later = format!("Agenda\n{now}");
        let (text, a, _) = put_in_place(&later, sel, Some((start, start + units(sel))), "We ship on Friday.").unwrap();
        assert_eq!(text, "Agenda\nNotes:\nWe ship on Friday. \nThanks");
        assert_eq!(a, units("Agenda\nNotes:\n"));
        // Gone, or there twice: nothing replaced.
        assert_eq!(put_in_place("Notes:\nThanks", sel, Some((7, 30)), "x").unwrap_err(), MOVED);
        let twice = format!("{sel}{sel}");
        assert_eq!(put_in_place(&twice, sel, Some((3, 9)), "x").unwrap_err(), MOVED);
    }

    #[test]
    fn the_whole_text_is_replaced_only_while_it_is_what_was_sent() {
        let (text, a, b) = put_in_place("um ship it friday", "um ship it friday", None, "Ship it on Friday.").unwrap();
        assert_eq!((text.as_str(), a, b), ("Ship it on Friday.", 0, 18));
        assert_eq!(put_in_place("um ship it friday!", "um ship it friday", None, "x").unwrap_err(), MOVED);
    }
}
