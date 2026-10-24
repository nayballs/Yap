//! Yap — a tiny local voice dictation tool.
//!
//! Press a global hotkey, speak, press again: Yap transcribes locally with
//! Whisper and types the text into whatever window is focused. A chime marks
//! recording start/stop, and a correction dictionary fixes mis-heard jargon.
//!
//! The dictation engine (input hook, STT, text injection) is ported verbatim
//! from Voice Mirror; everything else here is the slim glue.

mod ai;
mod apps;
mod auth;
mod bar;
mod calendar;
mod calls;
mod chat;
mod commands;
mod config;
mod dictation;
mod dictionary;
mod integrations;
mod meetings;
mod memory;
mod notes;
mod notices;
mod pasting;
mod privacy;
mod shell;
mod snip;
mod speakers;
mod updates;
mod voice;
mod web;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Whether a recording/processing overlay is meant to be on screen. A background
/// thread re-asserts "always on top" while this is true, so the overlay can't
/// get buried behind another topmost/fullscreen window mid-recording — which
/// would leave the user unaware a recording is live.
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Bumped on every `yap-state` change so a scheduled auto-clear of the
/// transient "error" state is cancelled if a newer state arrives first.
static STATE_GEN: AtomicU64 = AtomicU64::new(0);

/// Shared app state: the running dictation pipeline.
pub struct AppState {
    pub pipeline: Mutex<Option<dictation::pipeline::Pipeline>>,
}

/// Enable or disable OS autostart via `tauri-plugin-autostart`.
/// Kept as a free function so `commands::set_autostart` can delegate here.
pub fn set_autostart_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    // A dev build shares the installed Yap's "Yap" Run entry: disabling would
    // delete the installed app's launch-at-login, enabling would point it at
    // the dev exe. Leave the OS setting to the installed app.
    if cfg!(debug_assertions) {
        tracing::info!(enabled, "autostart: dev build, OS setting left alone");
        return Ok(());
    }
    let manager = app.autolaunch();
    let res = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    res.map_err(|e| format!("Failed to set autostart: {}", e))
}

/// The argument the sign-in launch carries (the autostart Run entry).
const AUTOSTART_ARG: &str = "--autostart";

/// Does this launch bring Yap's main window up? A launch by the person (the
/// Start menu, a shortcut, the installer's "Run Yap") does — before
/// 2026-10-22 a release build started to the tray with nothing on screen
/// but the Yap bar, so opening Yap seemed to do nothing. Not: a launch at
/// sign-in, "Start hidden", the first run (onboarding comes up instead) or
/// the updater's relaunch (`updates::init` brings the window back if it was
/// open). Pure.
#[cfg_attr(debug_assertions, allow(dead_code))] // dev builds always show Settings
fn launch_shows_window(at_sign_in: bool, start_hidden: bool, has_model: bool, after_update: bool) -> bool {
    !at_sign_in && !start_hidden && has_model && !after_update
}

/// Was Yap started by the sign-in entry? Its `--autostart`, or — for an
/// entry written before the argument existed, until the launch below writes
/// it again — autostart on and Windows up for under ten minutes (with
/// autostart on, a launch by hand later finds Yap running already and just
/// brings its window up through single-instance).
#[cfg_attr(debug_assertions, allow(dead_code))]
fn launched_at_sign_in(autostart: bool) -> bool {
    if std::env::args().any(|a| a == AUTOSTART_ARG) {
        return true;
    }
    #[cfg(windows)]
    {
        let up_ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
        autostart && up_ms < 10 * 60 * 1000
    }
    #[cfg(not(windows))]
    {
        let _ = autostart;
        false
    }
}

/// What the window-state plugin remembers for the main window (see `run`).
/// Excludes VISIBLE so a start-hidden launch never un-hides itself.
pub(crate) fn window_state_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

/// Clean-up every way out of Yap needs: stop the on-device AI sidecar, take
/// down the local API bridge (removing its discovery file), and bring back
/// other apps' sound if a dictation had silenced or paused it (`mute.rs`). Runs from the
/// `RunEvent::Exit` handler AND from the updater's pre-install hook — on
/// Windows the installer step force-exits Yap (`std::process::exit`) without
/// ever reaching the Exit handler (see `updates::build_updater`).
pub(crate) fn shutdown_cleanup() {
    voice::tts::stop();
    ai::local_llm::shutdown();
    integrations::bridge::stop();
    dictation::mute::restore_all();
    // The week of network receipts, as it stands (net.rs).
    privacy::net::flush();
    // A store whose last save failed (saving.rs): one more try.
    shell::saving::flush();
}

/// `yap.exe mcp` (main.rs): serve MCP to an AI app over stdin/stdout until it
/// hangs up, reading notes from the running Yap (see `mcp.rs`). Returns the
/// process exit code. Nothing of the app itself starts.
pub fn run_mcp_server() -> i32 {
    integrations::mcp::run_stdio()
}

/// `yap.exe diarize` (main.rs): the speaker-diarization engine a meeting
/// recording runs as a child process, fed audio on stdin, answering with
/// JSON lines on stdout (see `diarize.rs`). Returns the exit code. Nothing of
/// the app itself starts.
pub fn run_diarize_child() -> i32 {
    speakers::diarize::run_child()
}

/// `yap.exe eval …` (main.rs): the model evaluation harness (`eval.rs`,
/// docs/model-evals.md). Debug builds only; a release build says so and
/// exits. Returns the exit code (0 = every scenario passed).
pub fn run_eval() -> i32 {
    ai::eval::run(std::env::args().skip(2).collect())
}

/// `yap.exe bench …` (main.rs): speech-to-text, AI cleanup and a cold
/// start timed on this PC, a table and a JSON report written (`bench.rs`).
pub fn run_bench() -> i32 {
    shell::bench::main(std::env::args().skip(2).collect())
}

/// `yap.exe bench`'s throwaway copy for the cold start (`bench/cold.rs`):
/// `YAP_BENCH_CHILD=1`, honoured only in portable mode (its own data
/// folder). It never hands over to a Yap already running (single
/// instance), hooks the keyboard, changes launch-at-sign-in or the saved
/// window place, or touches another process.
fn bench_child() -> bool {
    shell::portable::is_portable() && std::env::var_os("YAP_BENCH_CHILD").is_some_and(|v| v == "1")
}

/// `yap.exe call-dump …` (main.rs): a call window's UI Automation tree,
/// read only, to a file (`speakers/call_ui.rs`).
pub fn run_call_dump() -> i32 {
    speakers::call_ui::dump_main(std::env::args().skip(2).collect())
}

/// `yap.exe snip-dump …` (main.rs, debug builds): every monitor captured
/// and timed, the PNGs written to a folder (`snip/dump.rs`).
#[cfg(debug_assertions)]
pub fn run_snip_dump() -> i32 {
    snip::dump::main(std::env::args().skip(2).collect())
}

/// Reload handle for the global log filter — lets the Settings "Debug mode"
/// toggle (OpenWhispr's Debug Logging section) bump verbosity at runtime
/// without a restart. See `set_debug_logging`.
static LOG_RELOAD: std::sync::OnceLock<
    tracing_subscriber::reload::Handle<
        tracing_subscriber::EnvFilter,
        tracing_subscriber::Registry,
    >,
> = std::sync::OnceLock::new();

/// The filter directives for normal vs debug mode. Debug raises YAP's own
/// crate to `debug` while keeping dependencies at `info` (whisper/ort/reqwest
/// at debug would drown the file). ONNX Runtime's own log (`ort`) stays at
/// `warn` either way: at `info` every model load wrote ~1,000 lines of graph
/// optimisation notes, 96% of a release build's log at startup.
fn log_directives(debug: bool) -> &'static str {
    if debug {
        "info,ort=warn,yap_lib=debug"
    } else {
        "info,ort=warn"
    }
}

/// Live-switch the log verbosity (the Settings → Privacy & data → Debug logging
/// toggle; persisted as `config.debug_logging` and re-applied on save/startup).
/// An explicit RUST_LOG env var always wins (it was applied at init and this
/// is only called from config paths when the value CHANGES).
pub(crate) fn set_debug_logging(enabled: bool) {
    if let Some(handle) = LOG_RELOAD.get() {
        let _ = handle.reload(tracing_subscriber::EnvFilter::new(log_directives(enabled)));
        // NB: the identifier `debug` must not appear inside tracing macros —
        // it resolves to the macro's own level shorthand and breaks the build.
        let state = if enabled { "enabled" } else { "disabled" };
        tracing::info!(debug_mode = enabled, "Debug logging {state}");
    }
}

/// Initialise tracing to BOTH stdout (for `tauri dev`) and a rolling file in the
/// data dir. Installed builds are windowed with no console, so stdout logs are
/// invisible — the file log is the only way to diagnose a shipped build (e.g.
/// the "stuck on transcribing" report was a CPU-only whisper build with no
/// visible logs). Must run AFTER `shell::portable::init()` so the data dir resolves.
/// The filter sits behind a reload layer so "Debug mode" can raise it live.
fn init_logging() {
    use tracing_subscriber::prelude::*;

    // Startup level: RUST_LOG wins, else the persisted Debug-mode setting.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(log_directives(config::load().debug_logging))
        });
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    let _ = LOG_RELOAD.set(reload_handle);

    let log_dir = config::data_dir().join("logs");
    let file_layer = match std::fs::create_dir_all(&log_dir) {
        Ok(_) => {
            let appender = tracing_appender::rolling::daily(&log_dir, "yap.log");
            let (nb, guard) = tracing_appender::non_blocking(appender);
            // The writer's flush guard must outlive the app; the process owns it
            // for its whole lifetime, so leaking it is intentional.
            std::mem::forget(guard);
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(nb),
            )
        }
        Err(_) => None, // no data dir writable — fall back to stdout only
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer()) // stdout (dev)
        .with(file_layer)
        .try_init();

    // Route panics into the log file too — a panicking thread otherwise dies
    // silently in a windowed build (and even in dev the console scrolls away).
    // A native access violation can't be caught here, but every Rust-level
    // panic now leaves a trace with its location.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        tracing::error!(location = %loc, "PANIC: {}", msg);
    }));
}

#[cfg(target_os = "windows")]
mod dwm {
    use std::ffi::c_void;
    pub const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    pub const DWMWA_CLOAK: u32 = 13;
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
        pub fn DwmFlush() -> i32;
    }

    /// Set a BOOL-valued DWM window attribute (best-effort).
    pub fn set_flag(hwnd: *mut c_void, attribute: u32, on: bool) {
        let value: i32 = on.into();
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                &value as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

/// One-shot show+hide of a still-hidden webview window so WebView2 finishes
/// initializing while VISIBLE (see the call site in `setup`), without anything
/// reaching the screen: the window is DWM-cloaked (composed, never drawn) with
/// its open/close animations disabled for the round-trip.
///
/// The previous version parked the window at (-32000,-32000) and moved it back
/// straight after, which flashed a blank "Welcome to Yap" window on every
/// launch — `hide()` starts DWM's ~200 ms close animation, and that animation
/// followed the window back to its real on-screen position. (Parking also
/// leaked -32000 into the window-state plugin's saved position.)
#[cfg(target_os = "windows")]
fn init_hidden_webview(window: &tauri::WebviewWindow) {
    // Already on screen (dev builds show Settings at launch): it initialized
    // visible anyway, and a show+hide here would just hide it again.
    if window.is_visible().unwrap_or(false) {
        return;
    }
    let Ok(hwnd) = window.hwnd() else { return };
    dwm::set_flag(hwnd.0, dwm::DWMWA_CLOAK, true);
    dwm::set_flag(hwnd.0, dwm::DWMWA_TRANSITIONS_FORCEDISABLED, true);
    let _ = window.show();
    let _ = window.hide();
    // Let DWM process the hide (one composition pass) while the window is
    // still cloaked, then restore normal behaviour for real opens.
    unsafe { dwm::DwmFlush() };
    dwm::set_flag(hwnd.0, dwm::DWMWA_TRANSITIONS_FORCEDISABLED, false);
    dwm::set_flag(hwnd.0, dwm::DWMWA_CLOAK, false);
}

pub fn run() {
    // Decide portable-vs-installed once, before anything reads the data dir
    // (the file log lives under it).
    shell::portable::init();
    init_logging();
    tracing::info!(
        engines = cfg!(feature = "engines"),
        version = env!("CARGO_PKG_VERSION"),
        "Yap starting — build capabilities (GPU whisper via Vulkan, ONNX via DirectML; falls back to CPU with no GPU)"
    );

    let builder = tauri::Builder::default();

    // Single-instance: RELEASE builds only. Dev builds must coexist with an
    // installed Yap — with the plugin on, a `tauri dev` instance pings the
    // running installed app and silently exits within seconds, which broke
    // every "run the dev build" workflow (incl. Voice Mirror's App Preview).
    // On a duplicate release launch, surface the app instead of doing nothing.
    // (Nor `yap.exe bench`'s throwaway copy: it must start, not wake yours.)
    #[cfg(not(debug_assertions))]
    let builder = if bench_child() {
        builder
    } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(settings) = app.get_webview_window("settings") {
                let _ = settings.show();
                let _ = settings.unminimize();
                let _ = settings.set_focus();
            }
        }))
    };

    let builder = builder
        // Yap accounts: com.contextmirror.yap:// sign-in links (auth.rs). Must
        // come after single-instance, which forwards links from a second launch.
        .plugin(tauri_plugin_deep_link::init())
        // A launch at sign-in carries `--autostart`, so it stays in the tray
        // while a launch by the person brings the window up (`launch_shows_window`).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        // Auto-update from GitHub Releases. Driven Rust-side by updates.rs
        // (background checks, download, install-on-request); the plugin's JS
        // commands aren't granted to the webviews. Desktop-only.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // External links ("Get your API key", GitHub, Learn more) open in the
        // default browser — target=_blank does nothing in a Tauri webview.
        .plugin(tauri_plugin_opener::init())
        // Native file-open dialog for the Upload surface's Browse button.
        .plugin(tauri_plugin_dialog::init());

    // Remember the MAIN window's size/position across launches. Only the
    // "settings" window is managed: the overlay, the notepad and its split
    // preview are positioned programmatically, and onboarding is one-shot —
    // restoring stale bounds would misplace them. Flags exclude VISIBLE so the
    // window never un-hides itself on a start-hidden launch. Not in e2e test
    // runs: the state file sits outside the portable data dir, shared with the
    // installed app.
    let builder = if shell::e2e::active() || bench_child() {
        builder
    } else {
        builder.plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .with_denylist(&["overlay", "onboarding", "notepad", "split-preview"])
                .build(),
        )
    };

    builder
        .manage(AppState {
            pipeline: Mutex::new(None),
        })
        // Chat's screenshots, by id only (Snip to Chat, snip.rs):
        // yapsnip://localhost/img/<id> (http://yapsnip.localhost/… in WebView2).
        .register_uri_scheme_protocol("yapsnip", |_ctx, request| snip::serve(&request))
        .invoke_handler(tauri::generate_handler![
            commands::toggle_recording,
            commands::dictation_key,
            commands::get_config,
            ai::about_you::about_you_status,
            commands::save_config,
            shell::saving::saves_retry,
            commands::download_model_size,
            commands::installed_models,
            commands::open_settings,
            commands::open_onboarding,
            commands::close_onboarding,
            commands::list_audio_devices,
            commands::list_output_devices,
            commands::set_mic_test,
            commands::set_input_device,
            commands::frontend_log,
            commands::model_language_info,
            commands::configure_hotkey,
            commands::configure_edit_hotkey,
            commands::set_active_model,
            commands::delete_model,
            commands::open_external,
            commands::cancel_recording,
            commands::set_autostart,
            commands::is_portable,
            commands::test_post_process,
            commands::get_base_prompt,
            commands::get_edit_base_prompt,
            commands::local_llm_status,
            ai::cleanup_styles::cleanup_styles_catalog,
            ai::cleanup_styles::cleanup_offer_status,
            ai::cleanup_styles::cleanup_offer_seen,
            commands::local_llm_start,
            commands::local_llm_stop,
            commands::local_llm_install,
            commands::local_llm_delete,
            ai::local_llm::local_llm_set_context,
            commands::open_llm_folder,
            commands::get_groq_usage,
            commands::get_history,
            commands::clear_history,
            commands::delete_history_entry,
            commands::get_stats,
            commands::home_snapshot,
            pasting::text_injector::paste_last_transcript,
            pasting::as_said::as_said_swap,
            pasting::as_said::as_said_paste_entry,
            commands::configure_as_said_hotkey,
            commands::configure_snip_hotkey,
            dictation::transforms::configure_transform_hotkey,
            snip::snip_start,
            snip::chat_images_usage,
            snip::chat_images_prepare,
            dictation::stt_speed::stt_speed_status,
            dictation::stt_speed::stt_speed_measure,
            ai::hardware::hardware_advice,
            dictation::recent_audio::recent_audio_status,
            dictation::recent_audio::recent_audio_clear,
            dictation::recent_audio::recent_audio_wav,
            dictation::recent_audio::history_rerun,
            commands::transcribe_file,
            commands::cancel_file_transcription,
            commands::audio_file_info,
            commands::notes_list,
            commands::note_get,
            commands::note_create,
            commands::note_update,
            commands::note_delete,
            commands::note_enhance,
            notes::note_edits::transcript_line_edit,
            notes::note_edits::note_summary_update,
            notes::tasks::tasks_list,
            notes::tasks::note_task_set,
            commands::get_note_base_prompt,
            commands::notes_folders,
            commands::notes_folder_create,
            commands::notes_folder_rename,
            commands::notes_folder_delete,
            commands::notes_actions,
            commands::action_create,
            commands::action_update,
            commands::action_delete,
            commands::log_info,
            commands::open_logs_folder,
            privacy::net::connections_summary,
            privacy::net::connections_clear,
            privacy::net::connections_note,
            privacy::diagnostics::diagnostics_preview,
            privacy::diagnostics::diagnostics_save,
            privacy::diagnostics::diagnostics_reveal,
            privacy::diagnostics::diagnostics_issue_url,
            #[cfg(debug_assertions)]
            privacy::diagnostics::e2e_save_path,
            commands::meeting_start,
            commands::meeting_state,
            commands::note_export,
            commands::note_ask,
            commands::chats_list,
            commands::chat_get,
            commands::chat_delete,
            commands::chat_delete_many,
            commands::chat_rename,
            commands::chat_send,
            chat::stream::stream_stop,
            ai::ai_model::ai_model_status,
            ai::ai_model::chat_set_model,
            ai::ai_model::chat_provider_key,
            ai::model_catalog::model_catalog,
            ai::model_catalog::model_catalog_refresh,
            ai::model_catalog::llm_models,
            commands::chat_dictate,
            commands::chat_input_focus,
            commands::configure_assistant_hotkey,
            commands::configure_read_hotkey,
            commands::assistant_key,
            voice::read_aloud::read_aloud_text,
            voice::read_aloud::read_aloud_selection,
            voice::read_aloud::read_aloud_action,
            voice::read_aloud::read_aloud_state,
            chat::assistant::chat_view_state,
            chat::assistant::assistant_stop,
            voice::tts::tts_voices,
            voice::tts::tts_speak,
            voice::tts::tts_preview,
            voice::tts::tts_stop,
            voice::tts::tts_state,
            voice::tts::voice_replies_set,
            voice::tts::tts_volume_set,
            voice::tts::tts_kokoro_status,
            voice::tts::tts_kokoro_download,
            voice::tts::tts_kokoro_delete,
            voice::tts::tts_cloud_voices,
            voice::tts::tts_cloud_models,
            voice::tts::tts_cloud_usage,
            voice::tts::tts_cloud_preview,
            commands::bridge_status,
            integrations::mcp_clients::mcp_clients_status,
            integrations::mcp_clients::mcp_client_add,
            integrations::mcp_clients::mcp_client_remove,
            auth::auth_status,
            auth::auth_check_methods,
            auth::auth_start,
            auth::auth_cancel,
            auth::auth_device_start,
            auth::auth_device_cancel,
            auth::auth_submit_code,
            auth::auth_email_send,
            auth::auth_email_verify,
            auth::auth_sign_out,
            auth::auth_delete_account,
            auth::auth_list_sessions,
            auth::auth_revoke_other_sessions,
            auth::auth_revoke_session,
            updates::update_status,
            updates::update_check,
            updates::update_install,
            updates::update_ack,
            updates::update_ack_updated,
            calls::meeting_detect::meeting_detect_status,
            calls::meeting_detect::meeting_detect_respond,
            // Debug builds only (the e2e suite): not in a release build at all.
            #[cfg(debug_assertions)]
            calls::meeting_detect::meeting_detect_simulate,
            calls::app_icons::meeting_app_icons,
            apps::apps_list,
            apps::apps_icons,
            meetings::meeting_end::meeting_end,
            meetings::meeting_end::meeting_pause,
            meetings::meeting_end::meeting_summarise,
            meetings::meeting_end::meeting_summary_status,
            meetings::meeting_end::meeting_discard,
            meetings::meeting_end::meeting_delete,
            meetings::meeting_audio::meeting_audio_status,
            meetings::meeting_audio::meeting_audio_clear,
            meetings::meeting_audio::meeting_audio_note,
            meetings::meeting_audio::meeting_audio_clip,
            meetings::meeting_audio::meeting_transcribe_again,
            meetings::meeting_audio::meeting_recover,
            meetings::meeting_audio::meeting_retry_chunk,
            meetings::meeting_audio::meeting_job_cancel,
            meetings::meeting_assist::meeting_catch_up,
            meetings::meeting_assist::meeting_assist,
            speakers::speaker_rename,
            speakers::speaker_rename_lines,
            speakers::speaker_merge,
            speakers::speaker_not,
            speakers::speaker_suggestion,
            speakers::speaker_is_me,
            speakers::speaker_not_me,
            meetings::notepad::notepad_open,
            meetings::notepad::notepad_state,
            meetings::notepad::notepad_split,
            meetings::notepad::notepad_split_preview,
            meetings::notepad::notepad_consent_message,
            calls::meeting_guard::meeting_shortcut,
            calls::meeting_guard::meeting_keep_going,
            calls::meeting_guard::meeting_limit_status,
            calls::meeting_guard::meeting_checkin_respond,
            calls::meeting_guard::meeting_checkin_status,
            commands::configure_meeting_hotkey,
            #[cfg(debug_assertions)]
            calls::capture::capture_affinity,
            calendar::calendar_status,
            calendar::calendar_sync,
            calendar::calendar_connect_google,
            calendar::calendar_cancel_google,
            calendar::calendar_add_link,
            calendar::calendar_disconnect,
            calendar::calendar_event,
            calendar::calendar_card,
            calendar::calendar_nudge,
            calendar::calendar_meeting_notes,
            calendar::calendar_brief,
            calendar::calendar_brief_for_note,
            calendar::calendar_brief_open,
            bar::bar_status,
            bar::bar_regions,
            bar::bar_pointer_left,
            bar::bar_action,
            bar::bar_card_action,
            dictionary::learn::word_suggestion_respond,
            notices::alert::alert_action,
            #[cfg(debug_assertions)]
            bar::bar_simulate,
            #[cfg(debug_assertions)]
            bar::bar_debug,
            speakers::diarize::diarization_status,
            speakers::diarize::diarization_download,
            speakers::diarize::diarization_delete,
            #[cfg(debug_assertions)]
            speakers::diarize::diarize_wav,
            #[cfg(debug_assertions)]
            speakers::diarize::diarization_timeline,
            #[cfg(debug_assertions)]
            speakers::diarize::e2e_diarize_timeline,
            #[cfg(debug_assertions)]
            speakers::call_ui::e2e_call_readings,
            // Test mode only (e2e.rs); not in release builds at all.
            memory::memory_list,
            memory::memory_save_item,
            memory::memory_delete_item,
            memory::memory_set_dictation,
            memory::memory_clear,
            memory::memory_undo,
            memory::memory_import_prompt,
            memory::memory_tell,
            memory::memory_import,
            memory::memory_apply,
            memory::memory_export,
            #[cfg(debug_assertions)]
            memory::e2e_memory_learn,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_feed,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_dictation,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_output_change,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_quiet,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_fail,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_meeting_start_twice,
            #[cfg(debug_assertions)]
            meetings::meeting_audio::e2e_meeting_recovery_offer,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_main_on_screen,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictation_mic,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_mic_status,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictate_audio,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_crash_next_dictation,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_fail_saves,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_upload_pace,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_assistant_audio,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_input,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_last,
            #[cfg(debug_assertions)]
            dictation::mic_privacy::e2e_mic_privacy,
            #[cfg(debug_assertions)]
            pasting::elevated::e2e_paste_elevated,
            #[cfg(debug_assertions)]
            shell::context_menu::e2e_context_menus,
            #[cfg(debug_assertions)]
            shell::webview_memory::e2e_webview_memory,
            #[cfg(debug_assertions)]
            shell::theme::e2e_webview_background,
            #[cfg(debug_assertions)]
            dictation::coaching::e2e_coaching_reset,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_serving,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_restarts,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_busy,
            #[cfg(debug_assertions)]
            notices::notice::e2e_notice_reset,
            #[cfg(debug_assertions)]
            notices::notice::e2e_notice,
            #[cfg(debug_assertions)]
            notices::notice::e2e_focus,
            #[cfg(debug_assertions)]
            dictionary::learn::e2e_learn_correction,
            #[cfg(debug_assertions)]
            calls::meeting_guard::e2e_meeting_limit,
            #[cfg(debug_assertions)]
            calls::meeting_guard::e2e_meeting_checkins,
            #[cfg(debug_assertions)]
            calendar::calendar_e2e_opened,
            #[cfg(debug_assertions)]
            commands::e2e_opened_urls,
            #[cfg(debug_assertions)]
            voice::tts::tts_e2e_spoken,
            #[cfg(debug_assertions)]
            commands::e2e_chat_dictation,
            #[cfg(debug_assertions)]
            commands::e2e_dictation,
            #[cfg(debug_assertions)]
            chat::assistant::e2e_assistant_ask,
            #[cfg(debug_assertions)]
            apps::e2e_opened_apps,
            // Robot mode only (realtest.rs): the real-app tests.
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_window,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_refusals,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_dictate_text,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_rewrite_text,
            #[cfg(debug_assertions)]
            commands::e2e_rewrite,
            #[cfg(debug_assertions)]
            dictation::transforms::e2e_transform,
            #[cfg(debug_assertions)]
            dictation::transforms::e2e_transform_slots,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_transform,
            #[cfg(debug_assertions)]
            commands::e2e_target_window,
            #[cfg(debug_assertions)]
            ai::cleanup_styles::e2e_style_app,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_dictate_wav,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_clipboard,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_other_apps,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_delivered,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_read_selection,
        ])
        .on_page_load(|webview, payload| {
            // No browser context menu (Back, Refresh, Print, Inspect…) in any
            // Yap window: only the editing commands (context_menu.rs). Once
            // per webview, whichever window it is (the split preview is made
            // on demand).
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                shell::context_menu::install(webview);
            }
            // A meeting window (the notepad, the overlay) that loads while a
            // meeting records leaves screen captures too (capture.rs).
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                if let Some(window) = webview.app_handle().get_webview_window(webview.label()) {
                    calls::capture::sync_window(&window);
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = config::load();
            // Every save of config.json tells the windows (Settings' copy
            // adopts it); one Yap couldn't read at start is said once the
            // windows are up (config.rs).
            config::init(&handle);
            // Network receipts (net.rs): every connection Yap makes, kept a
            // week in connections.json.
            privacy::net::init(&handle);
            // A store's save that fails is said from now on (saving.rs).
            shell::saving::init(&handle);
            // Temp files a crash mid-write left behind (config.rs): Yap's
            // own names only, off the startup thread.
            let _ = std::thread::Builder::new().name("temp-sweep".into()).spawn(config::sweep_temp_files);

            // Register the app handle so the usage tracker can emit live
            // `groq-usage` updates after each AI-cleanup call.
            ai::usage::set_app_handle(handle.clone());
            // Spoken replies (tts.rs): its state goes to every window.
            voice::tts::init(&handle);

            // Global input hook + dictation hotkey. (Not in e2e test runs: a
            // test instance must never see, or swallow, the developer's keys.)
            if !shell::e2e::active() && !bench_child() {
                dictation::input_hook::start_input_hook(handle.clone());
            }
            if let Err(e) = dictation::input_hook::configure_dictation(&cfg.hotkey) {
                tracing::warn!("Failed to configure hotkey: {}", e);
            }
            // Optional edit/rewrite-mode hotkey (empty = unbound / opt-in).
            if let Err(e) = dictation::input_hook::configure_edit(&cfg.edit_hotkey) {
                tracing::warn!("Failed to configure edit hotkey: {}", e);
            }
            // The meeting shortcut (Win+Alt+M; meeting_guard.rs).
            if let Err(e) = dictation::input_hook::configure_meeting(&cfg.meeting_hotkey) {
                tracing::warn!("Failed to configure meeting hotkey: {}", e);
            }
            // The Voice Assistant's shortcut (assistant.rs; unbound by
            // default, and never one that clashes with the other three).
            dictation::input_hook::configure_assistant_from(&cfg);
            // Read aloud's (read_aloud.rs; unbound by default, never a clash).
            dictation::input_hook::configure_read_from(&cfg);
            // "Use what I said" (as_said.rs; unbound by default).
            dictation::input_hook::configure_as_said_from(&cfg);
            // Snip to Chat (snip.rs; Win+Alt+S by default, never a clash).
            dictation::input_hook::configure_snip_from(&cfg);
            // Saved transforms' shortcuts (transforms.rs; none by default).
            dictation::input_hook::configure_transforms_from(&cfg);

            // Clear ort's 0-byte DirectML.dll stub so ONNX uses the real system
            // DirectML, then fix the transcribe-rs accelerator policy before any
            // model loads: whisper → CUDA (Auto), ONNX → DirectML. No-ops in stub.
            dictation::stt::fix_directml_stub();
            dictation::stt::apply_accelerator_settings(cfg.use_gpu);
            // DirectML on the graphics card, not an integrated GPU DXGI
            // happens to list first (hybrid laptops; gpu.rs).
            dictation::stt::set_gpu_adapter(ai::gpu::speech_adapter());
            // Each model loads on the device that runs it fastest here
            // (Settings → Speech-to-Text → Runs on; stt_speed.rs).
            dictation::stt_speed::sync(&cfg);

            // Start the dictation pipeline (audio capture + STT engine). It
            // runs without a microphone too (see `Pipeline::start`).
            let pipeline = dictation::pipeline::Pipeline::start(handle.clone(), cfg.clone());
            if let Ok(mut guard) = app.state::<AppState>().pipeline.lock() {
                *guard = Some(pipeline);
            }
            // A model this PC hasn't timed yet (or a new graphics driver):
            // measured once things have settled.
            dictation::stt_speed::schedule(&handle, dictation::stt_speed::AFTER_START);
            // Recent dictations' audio past its time goes (recent_audio.rs),
            // and one Yap stopped transcribing (a crash) is listed as failed.
            dictation::recent_audio::init(&handle);
            // Meeting audio past its days goes, and what a crash left of a
            // meeting is offered to transcribe (meeting_audio.rs).
            meetings::meeting_audio::init(&handle);

            // Clear any orphaned cleanup sidecar from a previous session (a
            // crash or task-kill skips every exit path; installers launched by
            // older builds' updater did too), then — if on-device cleanup is
            // selected + installed — warm up a fresh one off-thread so the
            // first dictation cleanup skips the cold load. (An e2e test run
            // leaves other processes alone.)
            if !shell::e2e::active() && !bench_child() {
                ai::local_llm::kill_orphans();
            }
            {
                let cfg2 = cfg.clone();
                tauri::async_runtime::spawn(async move {
                    // A runtime cut off mid-swap (or taken by an antivirus)
                    // comes back first; leftovers go.
                    ai::local_llm::tidy_runtime().await;
                    ai::local_llm::autostart_if_configured(&cfg2).await;
                });
            }
            // An older llamafile runtime (installs before 22 Oct 2026) is
            // replaced in the background, never under a dictation.
            ai::local_llm::spawn_runtime_upgrade(handle.clone());

            // Local API bridge (Integrations): loopback server for CLIs/agents.
            integrations::bridge::sync(&handle, cfg.bridge_enabled);

            // Yap accounts: sign-in deep links + restore the stored session.
            auth::init(&handle);

            // The overlay window is the Yap bar: click-through, always on
            // top, never focused, following the cursor's monitor (bar.rs).
            bar::init(&handle);
            // Yap's notice policy reads "Show tips" from Settings.
            notices::notice::sync(&config::load());

            // Dev builds: show Settings on launch. Every Yap window is hidden
            // at startup by design (tray-first UX), which leaves dev runs — and
            // anything trying to preview/capture the app, like Voice Mirror's
            // App Preview — with literally nothing on screen.
            #[cfg(debug_assertions)]
            if let Some(settings) = app.get_webview_window("settings") {
                let _ = settings.show();
            }
            // Release builds: a launch by the person brings the main window
            // up (`launch_shows_window`); sign-in and "Start hidden" stay in
            // the tray.
            #[cfg(not(debug_assertions))]
            if launch_shows_window(
                launched_at_sign_in(cfg.autostart),
                cfg.start_hidden,
                !commands::installed_models().is_empty(),
                updates::relaunch_after_update(),
            ) {
                if let Err(e) = commands::show_settings(&handle) {
                    tracing::warn!("Could not show the main window: {}", e);
                }
            }

            // Route hotkey press/release into the pipeline. Done Rust-side so the
            // core loop doesn't depend on any webview being ready. The
            // pipeline picks toggle vs push-to-talk live from its config, so
            // both events go through `on_key`. Each carries the moment the
            // key moved (`dictation::gesture::now_ms`, stamped in the hook): events are
            // handled one after another, and push-to-talk's tap / double-tap
            // timing mustn't stretch while the last one is still starting.
            let press_handle = handle.clone();
            handle.listen("dictation-key-pressed", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = press_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_key(true, at);
                    }
                }
            });
            let release_handle = handle.clone();
            handle.listen("dictation-key-released", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = release_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_key(false, at);
                    }
                }
            });

            // Route the edit/rewrite hotkey the same way (its own event pair).
            let edit_press_handle = handle.clone();
            handle.listen("edit-key-pressed", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = edit_press_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_edit_key(true, at);
                    }
                }
            });
            let edit_release_handle = handle.clone();
            handle.listen("edit-key-released", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = edit_release_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_edit_key(false, at);
                    }
                }
            });

            // …and the Voice Assistant's shortcut (a question for Chat,
            // answered out loud; assistant.rs).
            let assistant_press_handle = handle.clone();
            handle.listen("assistant-key-pressed", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = assistant_press_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_assistant_key(true, at);
                    }
                }
            });
            let assistant_release_handle = handle.clone();
            handle.listen("assistant-key-released", move |event| {
                let at = event.payload().parse::<u64>().ok();
                let state = assistant_release_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_assistant_key(false, at);
                    }
                }
            });

            // …and Read aloud's: the selection in the app you're in, read
            // aloud (read_aloud.rs). Only its press.
            let read_handle = handle.clone();
            handle.listen("read-key-pressed", move |_| {
                voice::read_aloud::read_selection(&read_handle, voice::read_aloud::Origin::Shortcut);
            });

            // "Use what I said": the last cleaned dictation's paste swapped
            // for the words as said (as_said.rs), on its own thread.
            let as_said_handle = handle.clone();
            handle.listen("as-said-key-pressed", move |_| pasting::as_said::swap_soon(&as_said_handle));

            // Snip to Chat: drag a box over any screen, the image into Chat
            // (snip.rs, on its own thread); again while it's up cancels.
            let snip_handle = handle.clone();
            handle.listen("snip-key-pressed", move |_| snip::start(&snip_handle, snip::Origin::Shortcut));
            snip::init(&handle);

            // A saved transform's shortcut (transforms.rs): its slot, run on
            // its own thread.
            let transform_handle = handle.clone();
            handle.listen("transform-key-pressed", move |event| {
                if let Ok(slot) = event.payload().trim().parse::<usize>() {
                    dictation::transforms::on_key(&transform_handle, slot);
                }
            });

            // Drive the overlay (the Yap bar) from the pipeline's `yap-state`
            // event (decoupled from the pipeline itself). It always shows
            // while recording/processing, and briefly on error — it's the
            // hot-mic indicator, bar or no bar; only the live-text preview
            // inside it is user-toggleable (`streaming_partials`).
            let overlay_handle = handle.clone();
            handle.listen("yap-state", move |event| {
                let state = event.payload().trim_matches('"'); // payload is a JSON string
                let generation = STATE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
                let show = matches!(state, "recording" | "processing" | "processing-slow" | "error");
                notices::notice::on_pipeline_state(state);
                bar::on_pipeline_state(&overlay_handle, state);
                OVERLAY_ACTIVE.store(show, Ordering::Relaxed);
                // Keep the tray icon + menu in sync with the recording state.
                shell::tray::update_tray(&overlay_handle, state);
                // A dictation just ended → a restart-to-update requested
                // mid-dictation (or a held-back update notice) can go ahead.
                updates::on_pipeline_state(&overlay_handle, state);

                // The "error" state is transient: auto-clear it back to idle a
                // few seconds later, unless a newer state has arrived since.
                if state == "error" {
                    let h = overlay_handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(4));
                        if STATE_GEN.load(Ordering::Relaxed) == generation {
                            let _ = h.emit("yap-state", "idle");
                        }
                    });
                }
            });

            // While recording/processing, keep the overlay genuinely on top:
            // re-assert "always on top" a few times a second so another app's
            // topmost or fullscreen window can't bury it and leave the user
            // unaware a recording is live.
            let topmost_handle = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(350));
                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(w) = topmost_handle.get_webview_window("overlay") {
                    if w.is_visible().unwrap_or(false) {
                        bar::overlay::force_topmost(&w);
                    }
                }
            });

            // System tray (Handy-style: state-aware icon + model submenu).
            // Always built — it's the only persistent surface now that the
            // pill is retired; without it Settings would be unreachable.
            shell::tray::ensure_tray(app.handle(), &cfg);

            // Reconcile OS autostart state with the saved config.
            // Only touch OS autostart when the desired state differs from the
            // current one. Avoids a spurious "disable" call that errors in dev
            // (the dev exe was never registered) and spams the log on launch.
            // (`yap.exe bench`'s throwaway copy leaves your Yap's entry be.)
            let autostart_now = app.autolaunch().is_enabled().unwrap_or(false);
            if bench_child() {
                tracing::info!("bench child: autostart left alone");
            } else if autostart_now != cfg.autostart {
                if let Err(e) = set_autostart_enabled(&handle, cfg.autostart) {
                    tracing::debug!("autostart reconcile skipped: {}", e);
                }
            } else if cfg.autostart && !std::env::args().any(|a| a == AUTOSTART_ARG) {
                // An entry written before `--autostart` existed starts Yap
                // without it, which would now open the window at every
                // sign-in: written again, with it. (A launch that carries
                // the argument proves the entry already has it.)
                if let Err(e) = set_autostart_enabled(&handle, true) {
                    tracing::debug!("autostart entry not rewritten: {}", e);
                }
            }

            // Closing the settings / onboarding windows hides them (so they can
            // reopen) instead of destroying them — the app lives in the tray.
            for label in ["settings", "onboarding"] {
                if let Some(win) = app.get_webview_window(label) {
                    let w = win.clone();
                    let main = label == "settings";
                    let focus_handle = handle.clone();
                    win.on_window_event(move |event| match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            api.prevent_close();
                            let _ = w.hide();
                            if !main {
                                notices::notice::on_onboarding(false);
                                // Its mic check's level meter holds the mic
                                // open (mic.rs); the page can't tell it's
                                // hidden (WebView2 says it's still visible).
                                if let Ok(guard) = w.app_handle().state::<AppState>().pipeline.lock() {
                                    if let Some(p) = guard.as_ref() {
                                        p.set_mic_test(false);
                                    }
                                }
                            }
                        }
                        // The main window came up (tray, notification, second
                        // launch) or was clicked: a stale update check and a
                        // pending "update ready" toast can happen now.
                        tauri::WindowEvent::Focused(true) if main => {
                            updates::on_main_window_focused(&focus_handle);
                            // …and a pending call prompt or calendar card
                            // moves into the window (one place: notice.rs).
                            calls::meeting_detect::on_main_window_focused(&focus_handle);
                            calendar::on_main_window_focused();
                        }
                        _ => {}
                    });
                }
            }

            // The theme (Settings → General → Appearance): the opaque windows'
            // webviews get their page's background before anything shows, so
            // a dark Yap never flashes WebView2's white (theme.rs).
            shell::theme::init(&handle);

            // WebView2 subsystems initialized against a HIDDEN window can come
            // up permanently broken — most relevantly the Rust→JS event/eval
            // delivery channel (same created-hidden bug family as
            // tauri-apps/tauri#3654 and wry#1639; drag-drop got fixed in
            // wry#1638, the rest of the surface was never audited). A one-shot,
            // DWM-cloaked show+hide forces those windows to finish
            // initialization while VISIBLE without anything flashing on screen.
            #[cfg(target_os = "windows")]
            for label in ["onboarding", "settings", "notepad"] {
                if let Some(w) = app.get_webview_window(label) {
                    init_hidden_webview(&w);
                }
            }

            // The meeting notepad (docked beside a call) and the end of a
            // meeting (pause or end, "Started by mistake?", the action plan
            // written in Rust): both follow the recorder's events.
            meetings::notepad::init(&handle);
            meetings::meeting_end::init(&handle);
            // Memory: its events, and reading quiet chats for what to
            // remember (memory.rs).
            memory::init(&handle);
            // The speaker engine's voices onto meeting lines (beta).
            speakers::labels::init(&handle);

            // First run: if no model is downloaded yet, greet the user with the
            // onboarding model picker instead of a silent "needs-model" pill.
            // Suppressed when launched hidden (e.g. autostart at login).
            if !cfg.start_hidden && commands::installed_models().is_empty() {
                if let Err(e) = commands::show_onboarding(&handle) {
                    tracing::warn!("Could not show onboarding: {}", e);
                }
            }

            // Background update checks (GitHub Releases only). Last, so a
            // relaunch after "Restart to update" can reopen the main window
            // once the hidden webviews are initialized.
            updates::init(&handle);

            // The AI providers' live model lists (only providers with a
            // saved key; each at most once a day).
            ai::model_catalog::init(&handle);

            // Call detection: offer to take notes when a call starts (reads
            // Windows' per-app microphone record; local only).
            calls::meeting_detect::init(&handle);

            // Meeting guard rails: the maximum length, the meeting shortcut,
            // and keeping meeting windows out of screen shares.
            calls::meeting_guard::init(&handle);
            calls::capture::init(&handle);

            // Other apps' sound a crash left silenced mid-dictation comes back.
            dictation::mute::recover();

            // Windows off screen (the main window in the tray, onboarding,
            // the notepad between meetings) ask WebView2 to keep less in
            // memory (webview_memory.rs).
            shell::webview_memory::init(&handle);

            // Calendar: the connected calendars' meetings, reminders before
            // them, and their titles and attendees for meeting notes (talks
            // to Google or the person's iCal link only; calendar.rs).
            calendar::init(&handle);

            // e2e test runs: announce test mode, quit when stdin closes.
            shell::e2e::start(&handle);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Yap")
        .run(|_app_handle, event| {
            // No orphaned llamafile server, no stale cli-bridge.json pointing
            // at a dead port, no speakers left muted.
            if let tauri::RunEvent::Exit = event {
                shutdown_cleanup();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{launch_shows_window, log_directives};

    #[test]
    fn a_launch_by_the_person_brings_the_window_up() {
        // (at sign-in, start hidden, a model installed, the updater's relaunch)
        assert!(launch_shows_window(false, false, true, false));
        // Sign-in and "Start hidden" stay in the tray.
        assert!(!launch_shows_window(true, false, true, false));
        assert!(!launch_shows_window(false, true, true, false));
        // The first run shows onboarding instead.
        assert!(!launch_shows_window(false, false, false, false));
        // After "Restart to update", updates.rs decides.
        assert!(!launch_shows_window(false, false, true, true));
    }

    #[test]
    fn onnx_runtime_logs_warnings_only() {
        // Its graph-optimisation notes at `info` were ~1,000 lines per model
        // load; Yap's own debug mode doesn't bring them back.
        for debug in [false, true] {
            let d = log_directives(debug);
            assert!(d.contains("ort=warn"), "{d}");
            tracing_subscriber::EnvFilter::try_new(d).expect("valid directives");
        }
        assert!(log_directives(true).contains("yap_lib=debug"));
        assert!(!log_directives(false).contains("yap_lib=debug"));
    }
}
