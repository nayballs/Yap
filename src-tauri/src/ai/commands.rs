//! Settings → AI's commands: the immutable base prompts, a cleanup test,
//! the on-device model's status, install, folder and server, and Groq
//! usage.

use tauri::AppHandle;

use crate::config;

/// The immutable cleanup guardrail prompt, exposed so the Settings Prompt
/// Studio can display the full effective system prompt (guardrails + the
/// editable body) in its View tab.
#[tauri::command]
pub fn get_base_prompt() -> String {
    crate::ai::llm::BASE_PROMPT.to_string()
}

/// The immutable **edit/rewrite (Commands)** guardrail prompt, exposed so the
/// Commands tab's Prompt Studio can show the full effective prompt (guardrails
/// + the editable agent body) in its View tab. See `llm::EDIT_BASE_PROMPT`.
#[tauri::command]
pub fn get_edit_base_prompt() -> String {
    crate::ai::llm::EDIT_BASE_PROMPT.to_string()
}

/// Test the AI cleanup settings: run a sample sentence through the saved
/// post-processing config and return the cleaned text (or the error). Lets the
/// user verify their base URL / key / model / prompt from the Settings UI.
/// Never logs the API key.
#[tauri::command]
pub async fn test_post_process(text: String) -> Result<String, String> {
    let cfg = config::load();
    // Route through the on-device sidecar when it's the selected provider + up.
    let (base_url, api_key, model, provider) = crate::ai::local_llm::effective_endpoint(&cfg);
    let spelling = crate::ai::about_you::cleanup_spelling(&cfg);
    // The main body at the strength set (and a spoken list laid out), as a
    // dictation into an app with no style of its own gets it.
    let plan = crate::ai::cleanup_styles::main_plan(&cfg);
    let cleanup = crate::ai::cleanup_styles::clean(&text, &base_url, &api_key, &model, &provider, &plan, spelling.as_deref(), None, &cfg.dictionary, cfg.pp_disable_thinking);
    crate::privacy::net::within(crate::privacy::net::Feature::Dictation, cleanup).await
}

/// Status of the on-device cleanup sidecar: whether the runtime + model are
/// installed on disk, and whether the server is currently running.
#[tauri::command]
pub fn local_llm_status() -> serde_json::Value {
    let cfg = config::load();
    let on_disk = crate::ai::local_llm::list_models();
    // The graphics card picks the model recommended for this PC (gpu.rs).
    let gpu = crate::ai::gpu::this_pc();
    let capable = crate::ai::gpu::can_run_big_model(gpu.as_ref());
    let recommended = crate::ai::local_llm::models::recommended_id(capable);
    // Which installed model is in use (the pickers' Uninstall waits for a
    // switch): local_llm/uninstall.rs.
    let use_facts = crate::ai::local_llm::uninstall::facts(&cfg);
    let in_use = |f: &str| crate::ai::local_llm::uninstall::in_use(f, &use_facts).map(|u| u.as_str());
    let file_mb = |f: &str| std::fs::metadata(crate::ai::local_llm::llm_dir().join(f)).map(|m| m.len() / 1_048_576).ok();
    // The curated one-click download set, with per-model installed state.
    let curated: Vec<serde_json::Value> = crate::ai::local_llm::models::CURATED_MODELS
        .iter()
        .map(|m| {
            let installed = on_disk.iter().any(|f| f == m.filename);
            serde_json::json!({
                "id": m.id,
                "display": m.display,
                "blurb": m.blurb,
                "filename": m.filename,
                "sizeMb": m.size_mb,
                "url": m.url,
                // "Recommended for this PC"; `isDefault`: the model an empty
                // `ppLocalModel` means.
                "recommended": m.id == recommended,
                "isDefault": m.default,
                "needsGpu": m.needs_gpu,
                "tools": crate::chat::tools::supports_tools(
                    crate::ai::local_llm::PROVIDER_ONDEVICE,
                    &crate::ai::local_llm::model_id(std::path::Path::new(m.filename)),
                ),
                "family": m.family,
                "installed": installed,
                // Installed: the file's own size (what an uninstall frees),
                // and why it can't go now ("selected" | "busy" | null).
                "fileMb": if installed { file_mb(m.filename) } else { None },
                "inUse": if installed { in_use(m.filename) } else { None },
                // What it was trained for ("up to 256K"): the file's own
                // header once it's here, else the curated figure.
                "maxContext": crate::ai::local_llm::context::trained_context(&crate::ai::local_llm::llm_dir().join(m.filename)),
            })
        })
        .collect();
    // Where the running model went, and whether that makes it slow.
    let running = crate::ai::local_llm::shown_running();
    let placement = running.then(crate::ai::local_llm::placement).flatten();
    let serving = crate::ai::local_llm::shown_model_file(&cfg);
    let serving_mb = std::fs::metadata(crate::ai::local_llm::llm_dir().join(&serving)).map(|m| m.len() / 1_048_576).unwrap_or(0);
    let active_file = crate::ai::local_llm::active_model_path(&cfg)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // GGUFs of the person's own in the folder (not Yap's curated set), for
    // the Chat picker's "On this PC".
    let own: Vec<serde_json::Value> = on_disk
        .iter()
        .filter(|f| !crate::ai::local_llm::models::CURATED_MODELS.iter().any(|m| m.filename == f.as_str()))
        .map(|f| {
            let id = crate::ai::local_llm::model_id(std::path::Path::new(f));
            serde_json::json!({
                "file": f,
                "name": id,
                "sizeMb": file_mb(f).unwrap_or(0),
                "inUse": in_use(f),
                "tools": crate::chat::tools::supports_tools(crate::ai::local_llm::PROVIDER_ONDEVICE, &id),
                "maxContext": crate::ai::local_llm::context::trained_context(&crate::ai::local_llm::llm_dir().join(f)),
            })
        })
        .collect();
    // Another feature than Chat on the on-device model (cleanup, a profile,
    // a scope): it shares the one sidecar, so switching it switches theirs.
    let shared_on_device = (cfg.post_process_enabled && cfg.pp_provider == crate::ai::local_llm::PROVIDER_ONDEVICE)
        || cfg.cleanup_profiles.iter().any(|p| p.provider == crate::ai::local_llm::PROVIDER_ONDEVICE)
        || cfg
            .llm_scopes
            .iter()
            .any(|(k, s)| k != "chat" && s.enabled && s.provider == crate::ai::local_llm::PROVIDER_ONDEVICE);
    let upgrade = crate::ai::local_llm::models::upgrade_offer(
        capable,
        crate::ai::local_llm::models::in_use(&cfg),
        &active_file,
        cfg.local_llm_upgrade_dismissed,
    )
    .map(|m| {
        serde_json::json!({
            "id": m.id,
            "display": m.display,
            "sizeMb": m.size_mb,
            "installed": on_disk.iter().any(|f| f == m.filename),
        })
    });
    serde_json::json!({
        "installed": crate::ai::local_llm::is_installed(&cfg),
        "running": running,
        // Loading now (on demand, or at launch): Settings says "Loading…";
        // installed, not running and not loading: "Not loaded · loads when
        // needed" (local_llm/on_demand.rs).
        "loading": !running && (crate::ai::local_llm::on_demand::loading() || crate::ai::local_llm::starting()),
        "modelFile": crate::ai::local_llm::MODEL_FILENAME,
        "model": crate::ai::local_llm::active_model_display(&cfg),
        "engine": crate::ai::local_llm::ENGINE_DISPLAY,
        // The llamafile on disk, the one Yap pins, the background upgrade
        // ("downloading" | "waiting" | ""), and the running server's MTP.
        "runtime": {
            "version": crate::ai::local_llm::runtime::runtime_version().map(|v| v.to_string()),
            "pinned": crate::ai::local_llm::RUNTIME_VERSION,
            "upgrade": crate::ai::local_llm::runtime::upgrade_phase(),
            "mtp": crate::ai::local_llm::serving_plan().map(|p| p.server().mtp),
        },
        "activeModel": cfg.pp_local_model,
        "activeFile": active_file,
        "models": on_disk,
        "own": own,
        "sharedOnDevice": shared_on_device,
        // Why the model in use can't be uninstalled, in plain words: "Dictation
        // cleanup and note formatting use it. Switch them to another model
        // first" (the pickers' note; null with nothing named).
        "inUseNote": crate::ai::local_llm::uninstall::in_use_by(&use_facts.users, "it"),
        "curated": curated,
        "gpu": gpu,
        "gpuCapable": capable,
        "recommendedId": recommended,
        "placement": placement.map(|p| serde_json::json!({
            "kind": p.kind(),
            "gpuLayers": p.gpu_layers,
            "layers": p.layers,
        })),
        "slowOnThisPc": running && crate::ai::local_llm::slow_on_this_pc(&serving, serving_mb, placement),
        "upgrade": upgrade,
        // The active model's context: the setting, its maximum, the
        // choices with what each takes (local_llm::context::context_view).
        "context": crate::ai::local_llm::context::context_view(&cfg),
    })
}

/// Open the on-device models folder (`<data>/llm/`) in the file manager so the
/// user can drop in their own GGUF models.
#[tauri::command]
pub fn open_llm_folder() -> Result<(), String> {
    let dir = crate::ai::local_llm::llm_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Start the on-device cleanup sidecar (runtime + model must be installed).
/// Resolves once the server's /health endpoint is ready.
#[tauri::command]
pub async fn local_llm_start() -> Result<(), String> {
    // Asked for: tried now, whatever failed a moment ago.
    crate::ai::local_llm::on_demand::forget_failure();
    crate::ai::local_llm::start().await.map(|_| ())
}

/// Stop the on-device cleanup sidecar.
#[tauri::command]
pub fn local_llm_stop() {
    crate::ai::local_llm::stop();
}

/// Download the on-device cleanup runtime + a model (each SHA-verified) on
/// demand. `model` = a curated id from `local_llm_status().curated` (None →
/// the bundled default). Returns the model's GGUF filename so the caller can
/// set `pp_local_model`. Emits `local-llm-download-progress` per stage.
/// No-op for files already present.
#[tauri::command]
pub async fn local_llm_install(app: AppHandle, model: Option<String>) -> Result<String, String> {
    match model {
        Some(id) => crate::ai::local_llm::models::install_curated(&id, Some(&app))
            .await
            .map(|m| m.filename.to_string()),
        None => crate::ai::local_llm::models::install(Some(&app))
            .await
            .map(|_| crate::ai::local_llm::MODEL_FILENAME.to_string()),
    }
}

/// Uninstall an on-device model: its GGUF deleted from `<data>/llm/`
/// (`file`: a bare name from `local_llm_status`'s `curated` / `own`). Only a
/// `.gguf` directly in that folder, never the model in use (switch first),
/// a loaded one nothing uses freed first; a refusal in plain words
/// (local_llm/uninstall.rs). The Chat model picker's and Settings' Uninstall.
#[tauri::command]
pub async fn local_llm_uninstall(app: AppHandle, file: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::ai::local_llm::uninstall::uninstall(&app, &file))
        .await
        .map_err(|_| "Couldn't uninstall the model. Try again.".to_string())?
}

/// Today's Groq AI-cleanup usage snapshot for the Settings meter.
/// Shape: `{ day, tokens, tokenCap, requests, requestCap }`. Tokens are Yap's
/// own accumulated `usage.total_tokens`; the token cap is the free-tier estimate
/// (constant), while requests use Groq's exact daily header math. Resets at
/// midnight UTC.
#[tauri::command]
pub fn get_groq_usage() -> serde_json::Value {
    crate::ai::usage::snapshot()
}
