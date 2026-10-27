//! Uninstalling an on-device model (2026-10-27, the maintainer's ask after
//! a 5.8 GB download that didn't suit them): its GGUF deleted from `llm/`,
//! from the Chat model picker's right-click menu and Settings → Language
//! Models. Only a bare `.gguf` name directly inside `llm/` — never the
//! llamafile engine, its marker or anything outside the folder — and never
//! the model in use: the one every feature routed on-device uses (the
//! sidecar serves one model), or one the engine is busy with. A model
//! loaded but used by nothing is freed first: Windows won't delete a file
//! the engine holds open.

use std::io::ErrorKind;
use std::path::Path;

use tauri::{AppHandle, Emitter};

use super::models::CURATED_MODELS;
use super::{active_model_path, llm_dir, model_id, PROVIDER_ONDEVICE};

/// Why a model can't be uninstalled now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InUse {
    /// Every feature routed on-device uses it (Chat, cleanup, a profile, a
    /// scope: one sidecar, one model): switch to another model first.
    Selected,
    /// Nothing is routed to it, but the engine is busy with it (a request
    /// under way, a start or a restart): in a moment.
    Busy,
}

impl InUse {
    /// `local_llm_status`'s `inUse` for a model: the pickers' note.
    pub fn as_str(self) -> &'static str {
        match self {
            InUse::Selected => "selected",
            InUse::Busy => "busy",
        }
    }
}

/// What decides whether a model is in use ([`in_use`]).
#[derive(Debug, Clone, Default)]
pub struct UseFacts {
    /// The on-device model's file: `pp_local_model`, else the bundled default.
    pub configured: String,
    /// Something is routed on-device: the cleanup's provider (which Chat,
    /// Commands and the meeting notes inherit), a cleanup profile, a
    /// Language Models scope (`models::in_use`).
    pub routed: bool,
    /// The model id ([`model_id`]) the running engine serves.
    pub serving: Option<String>,
    /// The engine busy: a request or a run of them, a start, a restart.
    pub busy: bool,
    /// What's routed on-device, in plain words ([`users`]): what the note
    /// names when the model in use can't go.
    pub users: Vec<String>,
}

/// This PC's facts, now.
pub fn facts(cfg: &crate::config::YapConfig) -> UseFacts {
    UseFacts {
        configured: active_model_path(cfg)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        routed: super::models::in_use(cfg),
        serving: super::serving_model_id(),
        busy: super::requests_in_flight() > 0
            || super::starting()
            || super::restart_under_way()
            || super::context::restart_pending()
            || super::on_demand::loading(),
        users: users(cfg),
    }
}

/// What uses the on-device model, as the pickers name it (2026-10-27: the
/// maintainer's Chat was on Claude, so "Switch to another model first"
/// made no sense there). The routes `models::in_use` counts: dictation
/// cleanup (which Chat, Commands and note formatting follow unless they
/// have their own model), a style by app with its own model, a Language
/// Models scope.
pub fn users(cfg: &crate::config::YapConfig) -> Vec<String> {
    let here = |p: &str| p == PROVIDER_ONDEVICE;
    let mut out = Vec::new();
    if here(&cfg.pp_provider) {
        out.push("dictation cleanup".to_string());
    }
    let styles: Vec<&str> = cfg.cleanup_profiles.iter().filter(|p| here(&p.provider)).map(|p| p.name.trim()).collect();
    match styles.as_slice() {
        [] => {}
        [""] => out.push("a style by app".to_string()),
        [one] => out.push(format!("the {one} style")),
        many => out.push(format!("{} styles by app", many.len())),
    }
    let mut named = 0;
    for (key, label) in [("voiceAgent", "Commands"), ("noteFormatting", "note formatting"), ("chat", "Chat")] {
        if cfg.llm_scopes.get(key).is_some_and(|s| s.enabled && here(&s.provider)) {
            out.push(label.to_string());
            named += 1;
        }
    }
    // A scope this list doesn't name yet still counts.
    if cfg.llm_scopes.values().filter(|s| s.enabled && here(&s.provider)).count() > named {
        out.push("another feature".to_string());
    }
    out
}

/// "Dictation cleanup and note formatting use it. Switch them to another
/// model first": `users` ([`users`]) and the model (`it` in the pickers'
/// note, its name in an uninstall's refusal); `None` with nobody named.
/// No full stop at the end: the note has none. Pure.
pub fn in_use_by(users: &[String], what: &str) -> Option<String> {
    let (last, rest) = users.split_last()?;
    let list = if rest.is_empty() { last.clone() } else { format!("{} and {last}", rest.join(", ")) };
    let mut c = list.chars();
    let list = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    let (verb, them) = if rest.is_empty() && !last.ends_with("styles by app") { ("uses", "that") } else { ("use", "them") };
    Some(format!("{list} {verb} {what}. Switch {them} to another model first"))
}

/// Is `file` (a GGUF in `llm/`) in use? The model every feature routed
/// on-device uses — configured, or the one the engine serves — while
/// anything is routed there; else the one the engine is busy with. A model
/// nothing uses, loaded or not, can go. Names compare as Windows does
/// (case aside). Pure.
pub fn in_use(file: &str, f: &UseFacts) -> Option<InUse> {
    let id = model_id(Path::new(file));
    let configured = f.configured.eq_ignore_ascii_case(file);
    let served = f.serving.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&id));
    if !(configured || served) {
        return None;
    }
    if f.routed {
        Some(InUse::Selected)
    } else if f.busy {
        Some(InUse::Busy)
    } else {
        None
    }
}

/// A model's file name as the uninstall takes it: a bare `.gguf` name —
/// no folder, no `..`, no drive or stream `:`, nothing hidden around it —
/// so it can only name a file directly inside `llm/`, never the engine
/// (`llamafile.exe`), its marker or anything outside. Pure.
pub fn checked_name(file: &str) -> Result<&str, String> {
    let ok = file.len() > ".gguf".len()
        && file == file.trim()
        && !file.contains(['/', '\\', ':'])
        && !file.contains("..")
        && !file.chars().any(char::is_control)
        && file.to_ascii_lowercase().ends_with(".gguf");
    if ok {
        Ok(file)
    } else {
        Err("That isn't one of the on-device models on this PC.".to_string())
    }
}

/// Delete `name` from `dir` when it's one of the GGUFs the folder lists
/// (a real file directly inside it, as `list_models` sees them): its size
/// in bytes, `None` when it isn't there (already gone).
pub fn remove_from(dir: &Path, name: &str) -> std::io::Result<Option<u64>> {
    let listed = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_str() == Some(name) && e.path().is_file());
    if !listed {
        return Ok(None);
    }
    let path = dir.join(name);
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// What to say when Windows refuses the delete, in plain words — never the
/// system's own message. `another_yap`: another Yap on this PC (a dev build
/// beside the installed app) runs the engine from this folder. Pure.
pub fn refusal(kind: ErrorKind, code: Option<i32>, shown: &str, another_yap: bool) -> String {
    // ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION, ERROR_USER_MAPPED_FILE.
    let open_elsewhere = matches!(code, Some(32 | 33 | 1224));
    // ERROR_ACCESS_DENIED: read-only, no permission, or a file mapped by
    // another process.
    let denied = kind == ErrorKind::PermissionDenied || code == Some(5);
    if another_yap && (open_elsewhere || denied) {
        return format!("Another Yap on this PC is using {shown}. Close that Yap, then try again.");
    }
    if open_elsewhere {
        return format!("Windows wouldn't delete {shown}: another program has it open. Close that program, then try again.");
    }
    if denied {
        return format!("Windows wouldn't let Yap delete {shown}. Try deleting it from the models folder.");
    }
    format!("Couldn't uninstall {shown}. Try again, or delete it from the models folder.")
}

/// A model as the messages name it: a curated one's name, else (a model
/// the person added) its file name.
fn shown(name: &str) -> String {
    CURATED_MODELS
        .iter()
        .find(|m| m.filename.eq_ignore_ascii_case(name))
        .map(|m| m.display.to_string())
        .unwrap_or_else(|| name.to_string())
}

/// Uninstall the on-device model `file` ([`checked_name`]): refused while
/// it's in use ([`in_use`]); a loaded one nothing uses is freed first; the
/// configured one (nothing routed to it) leaves `pp_local_model` back on
/// the default. Then the pickers re-read (`yap-local-llm-changed`, and
/// `yap-chat-model-changed` with the default when the config changed: Settings'
/// copy adopts it). Logged with its size, never its name or path.
pub fn uninstall(app: &AppHandle, file: &str) -> Result<(), String> {
    let name = checked_name(file)?;
    let shown = shown(name);
    let cfg = {
        // No start while it goes (a start opens the configured file); one
        // under way counts as busy, which only the model it loads minds.
        let no_start = super::STARTING.try_lock().ok();
        let cfg = crate::config::load();
        let mut f = facts(&cfg);
        f.busy |= no_start.is_none();
        match in_use(name, &f) {
            Some(InUse::Selected) => {
                let why = in_use_by(&f.users, &shown).unwrap_or_else(|| format!("{shown} is in use. Switch to another model first"));
                return Err(format!("{why}."));
            }
            Some(InUse::Busy) => return Err(format!("{shown} is in use right now. Try again in a moment.")),
            None => {}
        }
        // Loaded, but nothing uses it: freed, so Windows lets the file go.
        if f.serving.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(&model_id(Path::new(name)))) {
            super::stop();
        }
        match remove_from(&llm_dir(), name) {
            Ok(Some(bytes)) => {
                let curated = CURATED_MODELS.iter().find(|m| m.filename.eq_ignore_ascii_case(name)).map(|m| m.id).unwrap_or("");
                tracing::info!(size_mb = bytes / 1_048_576, curated, "Uninstalled an on-device model");
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(kind = ?e.kind(), code = e.raw_os_error(), "Couldn't uninstall an on-device model");
                return Err(refusal(e.kind(), e.raw_os_error(), &shown, super::runtime::other_yaps_sidecar()));
            }
        }
        cfg
    };
    // The configured one (nothing was routed to it): the default again,
    // never a name pointing at nothing.
    if cfg.pp_local_model.eq_ignore_ascii_case(name) {
        let saved = crate::shell::settings::change_config(app, |c| {
            if c.pp_local_model.eq_ignore_ascii_case(name) {
                c.pp_local_model.clear();
            }
            Ok(())
        })?;
        let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null, "ppLocalModel": saved.pp_local_model }));
    }
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bare_gguf_name_inside_the_folder() {
        for ok in ["Qwen_Qwen3.5-9B-Q4_K_M.gguf", "My Model.GGUF", "qwen2.5-1.5b-instruct-q4_k_m.gguf"] {
            assert_eq!(checked_name(ok), Ok(ok), "{ok}");
        }
        for bad in [
            "",
            ".gguf",
            "../config.json",
            "..\\x.gguf",
            "../x.gguf",
            "sub/x.gguf",
            "sub\\x.gguf",
            "C:x.gguf",
            "C:\\Users\\someone\\x.gguf",
            "x.gguf:stream",
            "a..b.gguf",
            " x.gguf",
            "x.gguf ",
            "x.gguf\n",
            "x\0.gguf",
            "llamafile.exe",
            "llamafile.json",
            "llamafile-next.exe",
            "x.partial",
            "x.gguf.partial",
        ] {
            assert!(checked_name(bad).is_err(), "{bad:?}");
        }
    }

    fn facts(configured: &str, routed: bool, serving: Option<&str>, busy: bool) -> UseFacts {
        UseFacts { configured: configured.to_string(), routed, serving: serving.map(str::to_string), busy, users: Vec::new() }
    }

    #[test]
    fn the_note_names_what_uses_the_model() {
        use crate::config::{CleanupProfile, LlmScope, YapConfig};
        let mut cfg = YapConfig { pp_provider: "groq".into(), ..Default::default() };
        cfg.cleanup_profiles.clear();
        cfg.llm_scopes.clear();
        assert!(users(&cfg).is_empty());
        assert_eq!(in_use_by(&users(&cfg), "it"), None);

        cfg.pp_provider = PROVIDER_ONDEVICE.into();
        assert_eq!(in_use_by(&users(&cfg), "it").unwrap(), "Dictation cleanup uses it. Switch that to another model first");

        let scope = |provider: &str, enabled: bool| LlmScope { enabled, provider: provider.into(), ..Default::default() };
        cfg.llm_scopes.insert("noteFormatting".into(), scope(PROVIDER_ONDEVICE, true));
        cfg.llm_scopes.insert("chat".into(), scope("anthropic", true));
        // Off: it follows cleanup, which is already named.
        cfg.llm_scopes.insert("voiceAgent".into(), scope(PROVIDER_ONDEVICE, false));
        assert_eq!(
            in_use_by(&users(&cfg), "gemma-2-9b-it-Q4_K_M.gguf").unwrap(),
            "Dictation cleanup and note formatting use gemma-2-9b-it-Q4_K_M.gguf. Switch them to another model first"
        );

        let style = |name: &str, provider: &str| CleanupProfile { name: name.into(), provider: provider.into(), ..Default::default() };
        cfg.pp_provider = "groq".into();
        cfg.llm_scopes.clear();
        cfg.cleanup_profiles = vec![style("Email", PROVIDER_ONDEVICE), style("Terminal", "")];
        assert_eq!(in_use_by(&users(&cfg), "it").unwrap(), "The Email style uses it. Switch that to another model first");
        cfg.cleanup_profiles.push(style("Documents", PROVIDER_ONDEVICE));
        assert_eq!(in_use_by(&users(&cfg), "it").unwrap(), "2 styles by app use it. Switch them to another model first");
        cfg.llm_scopes.insert("voiceAgent".into(), scope(PROVIDER_ONDEVICE, true));
        cfg.llm_scopes.insert("somethingNew".into(), scope(PROVIDER_ONDEVICE, true));
        assert_eq!(
            in_use_by(&users(&cfg), "it").unwrap(),
            "2 styles by app, Commands and another feature use it. Switch them to another model first"
        );
    }

    #[test]
    fn the_model_in_use_stays_and_one_nothing_uses_can_go() {
        const NINE_B: &str = "Qwen_Qwen3.5-9B-Q4_K_M.gguf";
        const QWEN: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";
        // Chat, cleanup or a scope on the on-device model: the configured one stays.
        assert_eq!(in_use(NINE_B, &facts(NINE_B, true, None, false)), Some(InUse::Selected));
        // … and the one the engine serves (a switch not applied yet).
        assert_eq!(in_use(NINE_B, &facts(QWEN, true, Some("Qwen_Qwen3.5-9B-Q4_K_M"), false)), Some(InUse::Selected));
        // The bundled default follows the same rule.
        assert_eq!(in_use(QWEN, &facts(QWEN, true, None, false)), Some(InUse::Selected));
        // Another model can go.
        assert_eq!(in_use(NINE_B, &facts(QWEN, true, Some("qwen2.5-1.5b-instruct-q4_k_m"), true)), None);
        // Nothing routed on-device: the configured one can go, loaded or not…
        assert_eq!(in_use(NINE_B, &facts(NINE_B, false, None, false)), None);
        assert_eq!(in_use(NINE_B, &facts(NINE_B, false, Some("Qwen_Qwen3.5-9B-Q4_K_M"), false)), None);
        // … unless the engine is busy with it.
        assert_eq!(in_use(NINE_B, &facts(NINE_B, false, Some("Qwen_Qwen3.5-9B-Q4_K_M"), true)), Some(InUse::Busy));
        assert_eq!(in_use(NINE_B, &facts(NINE_B, false, None, true)), Some(InUse::Busy));
        // Names as Windows compares them.
        assert_eq!(in_use("QWEN_QWEN3.5-9B-Q4_K_M.GGUF", &facts(NINE_B, true, None, false)), Some(InUse::Selected));
        assert_eq!(in_use("qwen_qwen3.5-9b-q4_k_m.gguf", &facts(QWEN, true, Some("Qwen_Qwen3.5-9B-Q4_K_M"), false)), Some(InUse::Selected));
        assert_eq!(InUse::Selected.as_str(), "selected");
        assert_eq!(InUse::Busy.as_str(), "busy");
    }

    #[test]
    fn only_a_gguf_directly_inside_the_folder_is_deleted() {
        let dir = std::env::temp_dir().join(format!("yap-uninstall-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("llamafile.exe"), b"engine").unwrap();
        std::fs::write(dir.join("llamafile.json"), b"{}").unwrap();
        std::fs::write(dir.join("mine.gguf"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.join("sub").join("deep.gguf"), b"x").unwrap();

        assert_eq!(remove_from(&dir, "mine.gguf").unwrap(), Some(3000));
        assert!(!dir.join("mine.gguf").exists());
        // Gone already: nothing to do.
        assert_eq!(remove_from(&dir, "mine.gguf").unwrap(), None);
        // Not directly inside: left alone.
        assert_eq!(remove_from(&dir, "deep.gguf").unwrap(), None);
        assert!(dir.join("sub").join("deep.gguf").exists());
        // The engine and its marker are never touched.
        assert!(dir.join("llamafile.exe").exists() && dir.join("llamafile.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_delete_is_said_plainly() {
        let nine = "Qwen3.5 9B";
        let open = refusal(ErrorKind::Other, Some(32), nine, false);
        assert_eq!(open, "Windows wouldn't delete Qwen3.5 9B: another program has it open. Close that program, then try again.");
        assert!(refusal(ErrorKind::PermissionDenied, Some(5), nine, false).starts_with("Windows wouldn't let Yap delete Qwen3.5 9B."));
        assert!(refusal(ErrorKind::PermissionDenied, Some(5), nine, true).starts_with("Another Yap on this PC is using Qwen3.5 9B."));
        assert!(refusal(ErrorKind::Other, Some(1224), "my-model.gguf", true).starts_with("Another Yap"));
        assert!(refusal(ErrorKind::Other, Some(1117), nine, false).starts_with("Couldn't uninstall Qwen3.5 9B."));
        // A curated model by its name, one of the person's own (or one
        // taken off the list since) by its file.
        assert_eq!(shown("Qwen_Qwen3.5-9B-Q4_K_M.gguf"), nine);
        assert_eq!(shown("my-model.gguf"), "my-model.gguf");
        assert_eq!(shown("gemma-2-9b-it-Q4_K_M.gguf"), "gemma-2-9b-it-Q4_K_M.gguf");
    }
}
