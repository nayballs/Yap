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
mod search;
mod shell;
mod snip;
mod speakers;
mod updates;
mod voice;
mod web;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{Emitter, Listener, Manager};
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

/// Clean-up every way out of Yap needs: stop the on-device AI sidecar, take
/// down the local API bridge (removing its discovery file), and bring back
/// other apps' sound if a dictation had silenced or paused it (`mute.rs`). Runs from the
/// `RunEvent::Exit` handler AND from the updater's pre-install hook — on
/// Windows the installer step force-exits Yap (`std::process::exit`) without
/// ever reaching the Exit handler (see `updates::build_updater`).
pub(crate) fn shutdown_cleanup() {
    voice::tts::stop();
    ai::local_llm::shutdown();
    // Meeting search's model (search/embedder.rs) on the same engine.
    search::shutdown();
    integrations::bridge::stop();
    dictation::mute::restore_all();
    // The week of network receipts, as it stands (net.rs).
    privacy::net::flush();
    // The AI calls' tokens counted since the last write (usage/store.rs).
    ai::usage::store::flush();
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

/// `yap.exe partials-dump …` (main.rs, debug builds): the live preview
/// replayed on recordings, every line the bar would show written down
/// (`dictation/partials/dump.rs`).
#[cfg(debug_assertions)]
pub fn run_partials_dump() -> i32 {
    dictation::partials::dump::main(std::env::args().skip(2).collect())
}

/// `yap.exe dictation-check …` (main.rs, debug builds): the person's own
/// dictations replayed through speech-to-text and cleanup, scored against
/// what was said (`dictation/check.rs`).
#[cfg(debug_assertions)]
pub fn run_dictation_check() -> i32 {
    dictation::check::main(std::env::args().skip(2).collect())
}

pub fn run() {
    // Decide portable-vs-installed once, before anything reads the data dir
    // (the file log lives under it).
    shell::portable::init();
    shell::logging::init_logging();
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
            Some(vec![shell::autostart::AUTOSTART_ARG]),
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
                .with_state_flags(shell::window::window_state_flags())
                .with_denylist(&["overlay", "onboarding", "notepad", "split-preview", "quick-note"])
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
            dictation::commands::toggle_recording,
            dictation::commands::dictation_key,
            shell::settings::get_config,
            ai::about_you::about_you_status,
            shell::settings::save_config,
            shell::saving::saves_retry,
            dictation::commands::download_model_size,
            dictation::commands::installed_models,
            shell::window::open_settings,
            shell::window::open_onboarding,
            shell::window::close_onboarding,
            dictation::commands::list_audio_devices,
            dictation::commands::list_output_devices,
            dictation::commands::set_mic_test,
            dictation::mics::list_mics,
            // Getting started (2026-10-29): the mic quiz, the bar's first-use
            // pointers, the setup's record and the meeting notes checklist,
            // the Help menu's shortcut sheet.
            dictation::mic_quiz::mic_quiz_run,
            dictation::mic_quiz::mic_quiz_cancel,
            bar::pointer::bar_pointer_shown,
            shell::guide::guide_status,
            shell::guide::guide_calendar_step,
            shell::guide::guide_typing,
            shell::guide::meeting_setup_status,
            shell::guide::meeting_setup_dismiss,
            shell::help::shortcut_sheet,
            notices::kinds::notice_allow,
            dictation::mics::offer::mic_offer_respond,
            shell::logging::frontend_log,
            dictation::commands::model_language_info,
            dictation::commands::configure_hotkey,
            dictation::commands::configure_edit_hotkey,
            dictation::commands::set_active_model,
            dictation::commands::delete_model,
            shell::commands::open_external,
            dictation::commands::cancel_recording,
            shell::autostart::set_autostart,
            shell::commands::is_portable,
            ai::commands::test_post_process,
            ai::commands::get_base_prompt,
            ai::commands::get_edit_base_prompt,
            ai::commands::local_llm_status,
            ai::cleanup_styles::cleanup_styles_catalog,
            ai::cleanup_styles::cleanup_offer_status,
            ai::cleanup_styles::cleanup_offer_seen,
            ai::commands::local_llm_start,
            ai::commands::local_llm_stop,
            ai::commands::local_llm_install,
            shell::download::download_cancel,
            ai::commands::local_llm_uninstall,
            ai::commands::local_vision_install,
            ai::commands::local_vision_remove,
            ai::commands::local_vision_set,
            ai::local_llm::local_llm_set_context,
            ai::commands::open_llm_folder,
            ai::commands::get_groq_usage,
            ai::usage::ai_usage_summary,
            ai::usage::context::chat_context,
            chat::turn::chat_limits,
            dictation::commands::get_history,
            dictation::commands::clear_history,
            dictation::commands::delete_history_entry,
            dictation::commands::get_stats,
            dictation::commands::home_snapshot,
            dictation::stats::welcome_back_dismiss,
            dictation::stats::share::insights_card_copy,
            dictation::stats::share::insights_card_save,
            dictation::history::keep::history_counts,
            pasting::text_injector::paste_last_transcript,
            pasting::as_said::as_said_swap,
            pasting::as_said::as_said_paste_entry,
            dictation::commands::configure_as_said_hotkey,
            dictation::commands::configure_snip_hotkey,
            dictation::commands::configure_paste_last_hotkey,
            dictation::commands::configure_copy_last_hotkey,
            dictation::commands::configure_quick_note_hotkey,
            dictation::commands::configure_more_hotkeys,
            #[cfg(debug_assertions)]
            dictation::commands::e2e_hotkey_bindings,
            pasting::last::copy_last_transcript,
            dictation::transforms::configure_transform_hotkey,
            dictation::languages::languages_catalog,
            dictation::languages::my_languages_status,
            dictation::languages::configure_language_hotkey,
            dictation::languages::language_key,
            dictation::languages::nudge::language_nudge_respond,
            snip::snip_start,
            snip::chat_images_usage,
            snip::chat_images_open_folder,
            snip::chat_images_delete_all,
            snip::chat_images_prepare,
            snip::chat_image_copy,
            snip::chat_image_save,
            snip::chat_models_read_images,
            snip::attach::chat_image_paste,
            snip::attach::chat_image_drop,
            snip::waiting::chat_waiting_get,
            snip::waiting::chat_waiting_remove,
            snip::waiting::chat_waiting_clear,
            dictation::stt_speed::stt_speed_status,
            dictation::stt_speed::stt_speed_measure,
            ai::hardware::hardware_advice,
            dictation::recent_audio::recent_audio_status,
            dictation::recent_audio::recent_audio_clear,
            dictation::recent_audio::recent_audio_wav,
            dictation::recent_audio::recent_audio_keep,
            dictation::recent_audio::recent_audio_save,
            #[cfg(debug_assertions)]
            dictation::recent_audio::e2e_audio_save_path,
            dictation::recent_audio::history_rerun,
            dictation::commands::transcribe_file,
            dictation::commands::cancel_file_transcription,
            dictation::commands::audio_file_info,
            notes::commands::notes_list,
            notes::commands::note_get,
            notes::commands::note_create,
            notes::commands::note_update,
            notes::commands::note_delete,
            notes::commands::note_enhance,
            notes::note_edits::transcript_line_edit,
            notes::note_edits::note_summary_update,
            notes::transcript_edits::transcript_line_revert,
            notes::transcript_edits::transcript_line_delete,
            notes::transcript_edits::transcript_line_restore,
            notes::transcript_edits::transcript_line_split,
            notes::tasks::tasks_list,
            notes::tasks::note_task_set,
            notes::tasks::note_tasks_get,
            notes::tasks::task_add,
            notes::tasks::task_remove,
            notes::tasks::tasks_set,
            notes::tasks::tasks_archive,
            notes::commands::get_note_base_prompt,
            notes::commands::notes_folders,
            notes::commands::notes_folder_create,
            notes::commands::notes_folder_rename,
            notes::commands::notes_folder_delete,
            notes::commands::notes_actions,
            notes::commands::action_create,
            notes::commands::action_update,
            notes::commands::action_delete,
            shell::logging::log_info,
            shell::logging::open_logs_folder,
            privacy::net::connections_summary,
            privacy::net::connections_clear,
            privacy::net::connections_note,
            privacy::diagnostics::diagnostics_preview,
            privacy::diagnostics::diagnostics_save,
            privacy::diagnostics::diagnostics_reveal,
            privacy::diagnostics::diagnostics_issue_url,
            #[cfg(debug_assertions)]
            privacy::diagnostics::e2e_save_path,
            privacy::export::data_export_summary,
            privacy::export::data_export_save,
            privacy::export::data_export_cancel,
            privacy::export::data_export_reveal,
            #[cfg(debug_assertions)]
            privacy::export::e2e_export_pace,
            privacy::reset::data_reset_plan,
            privacy::reset::data_reset,
            meetings::commands::meeting_start,
            meetings::commands::meeting_state,
            notes::commands::note_export,
            notes::commands::note_ask,
            chat::commands::chats_list,
            chat::commands::chat_get,
            chat::commands::chat_delete,
            chat::commands::chat_delete_many,
            chat::commands::chat_rename,
            chat::commands::chats_archived,
            chat::commands::chat_archive,
            chat::commands::chat_send,
            chat::think::chat_think_info,
            chat::stream::stream_stop,
            ai::ai_model::ai_model_status,
            ai::ai_model::chat_set_model,
            ai::ai_model::chat_provider_key,
            ai::model_catalog::model_catalog,
            ai::model_catalog::model_catalog_refresh,
            ai::model_catalog::llm_models,
            dictation::commands::chat_dictate,
            dictation::commands::chat_input_focus,
            dictation::commands::configure_assistant_hotkey,
            dictation::commands::configure_read_hotkey,
            dictation::commands::assistant_key,
            voice::read_aloud::read_aloud_text,
            voice::read_aloud::read_aloud_selection,
            voice::read_aloud::read_aloud_action,
            voice::read_aloud::read_aloud_state,
            chat::assistant::chat_view_state,
            chat::assistant::assistant_stop,
            chat::assistant::assistant_phase,
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
            integrations::bridge::bridge_status,
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
            meetings::meeting_audio::export::meeting_audio_export,
            meetings::meeting_audio::meeting_transcribe_again,
            meetings::meeting_audio::meeting_recover,
            meetings::meeting_audio::meeting_retry_chunk,
            meetings::meeting_audio::meeting_job_cancel,
            meetings::meeting_assist::meeting_catch_up,
            meetings::meeting_assist::meeting_assist,
            meetings::meeting_assist::meeting_skill,
            meetings::meeting_assist::meeting_longer,
            meetings::coach_marks::notepad_coach_marks,
            meetings::coach_marks::notepad_coach_mark_shown,
            chat::skills::skills_list,
            chat::skills::skill_save,
            chat::skills::skill_delete,
            chat::note_chats::note_chat,
            chat::note_chats::note_chat_open,
            chat::note_chats::note_chat_add,
            chat::note_chats::note_chat_rename,
            notes::note_edits::note_summary_restore,
            notes::note_edits::note_summary_put_back,
            notes::note_edits::note_summary_rate,
            // Pinned notes, version history, transforms in notes.
            notes::pins::note_pin,
            notes::versions::note_versions,
            notes::versions::note_version_restore,
            notes::transform::note_transforms,
            notes::transform::note_transform,
            speakers::speaker_rename,
            speakers::speaker_rename_lines,
            speakers::speaker_merge,
            speakers::speaker_not,
            speakers::speaker_suggestion,
            speakers::speaker_is_me,
            speakers::speaker_not_me,
            speakers::rerun::speaker_find,
            speakers::clip::speaker_clip,
            speakers::flags::speaker_flag,
            meetings::notepad::notepad_open,
            meetings::notepad::notepad_state,
            meetings::notepad::notepad_split,
            meetings::notepad::notepad_split_preview,
            meetings::notepad::notepad_consent_message,
            notes::quick_note::quick_note_toggle,
            notes::quick_note::quick_note_open,
            notes::quick_note::quick_note_hide,
            notes::quick_note::quick_note_state,
            notes::quick_note::quick_note_save,
            notes::quick_note::quick_note_new,
            notes::quick_note::quick_note_dictate,
            notes::quick_note::quick_note_bar_button,
            notes::quick_note::key::quick_note_key,
            notes::quick_note::tabs::quick_note_tabs,
            notes::quick_note::send::quick_note_send_target,
            notes::quick_note::send::quick_note_send,
            calls::meeting_guard::meeting_shortcut,
            calls::meeting_guard::meeting_keep_going,
            calls::meeting_guard::meeting_limit_status,
            calls::meeting_guard::meeting_checkin_respond,
            calls::meeting_guard::meeting_checkin_status,
            dictation::commands::configure_meeting_hotkey,
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
            calendar::calendar_day,
            calendar::filing::meeting_file,
            calendar::filing::meeting_file_past,
            calendar::filing::meeting_unfile,
            search::commands::search_status,
            search::commands::search_install,
            search::commands::search_set_enabled,
            search::commands::search_rebuild,
            search::commands::search_delete,
            search::commands::search_moments,
            search::commands::notes_search,
            search::commands::chats_search,
            bar::bar_status,
            bar::bar_regions,
            bar::bar_pointer_left,
            bar::bar_action,
            bar::bar_card_action,
            bar::menu::bar_menu,
            bar::menu::bar_use_mic,
            dictionary::learn::word_suggestion_respond,
            dictionary::learn::summary_word_offer,
            dictionary::snippet_offer::snippet_suggestion_respond,
            dictionary::snippet_offer::snippet_suggestions_enabled,
            dictionary::snippet_offer::snippet_suggestions_set,
            dictionary::dict_usage::dictionary_usage,
            dictionary::files::dictionary_import_pick,
            dictionary::files::dictionary_export_save,
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
            speakers::rerun::e2e_find_speakers,
            #[cfg(debug_assertions)]
            speakers::voiceprint::e2e_voice_prints,
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
            meetings::disk_watch::e2e_disk_space,
            #[cfg(debug_assertions)]
            meetings::meeting_audio::export::e2e_audio_export_file,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_main_on_screen,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictation_mic,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_mic_status,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictate_audio,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_shortcut,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictation_timing,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_dictation_cap,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_crash_next_dictation,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_fail_saves,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_upload_pace,
            #[cfg(debug_assertions)]
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_input,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_last,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_save_path,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_clipboard,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_ocr,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_snip_question,
            #[cfg(debug_assertions)]
            shell::e2e::e2e_drag_drop,
            #[cfg(debug_assertions)]
            dictation::mic_privacy::e2e_mic_privacy,
            #[cfg(debug_assertions)]
            dictation::mics::e2e_mics,
            #[cfg(debug_assertions)]
            dictation::mics::e2e_mic_plug,
            #[cfg(debug_assertions)]
            dictation::mics::e2e_mic_unplug,
            #[cfg(debug_assertions)]
            dictation::mic_tips::e2e_mic_tips,
            #[cfg(debug_assertions)]
            dictation::lid::e2e_lid,
            #[cfg(debug_assertions)]
            pasting::elevated::e2e_paste_elevated,
            #[cfg(debug_assertions)]
            pasting::text_injector::e2e_paste_fails,
            #[cfg(debug_assertions)]
            pasting::modifiers::e2e_held_keys,
            #[cfg(debug_assertions)]
            shell::context_menu::e2e_context_menus,
            #[cfg(debug_assertions)]
            shell::webview_memory::e2e_webview_memory,
            #[cfg(debug_assertions)]
            shell::theme::e2e_webview_background,
            #[cfg(debug_assertions)]
            dictation::coaching::e2e_coaching_reset,
            #[cfg(debug_assertions)]
            dictation::tips::e2e_feature_tips,
            #[cfg(debug_assertions)]
            bar::pointer::e2e_bar_pointer,
            #[cfg(debug_assertions)]
            dictation::mic_quiz::e2e_mic_quiz,
            #[cfg(debug_assertions)]
            shell::guide::e2e_guide_reset,
            #[cfg(debug_assertions)]
            notices::kinds::e2e_notice_caps,
            #[cfg(debug_assertions)]
            dictation::stats::e2e_stats_set,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_serving,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_restarts,
            #[cfg(debug_assertions)]
            ai::local_llm::e2e_local_llm_busy,
            #[cfg(debug_assertions)]
            ai::local_llm::on_demand::e2e_local_llm_loads,
            #[cfg(debug_assertions)]
            ai::local_llm::on_demand::e2e_local_llm_idle_ms,
            #[cfg(debug_assertions)]
            ai::local_llm::vision::e2e_local_vision,
            #[cfg(debug_assertions)]
            notices::notice::e2e_notice_reset,
            #[cfg(debug_assertions)]
            notices::notice::e2e_notice,
            #[cfg(debug_assertions)]
            notices::notice::e2e_focus,
            #[cfg(debug_assertions)]
            dictionary::learn::e2e_learn_correction,
            #[cfg(debug_assertions)]
            dictionary::files::e2e_dictionary_file,
            #[cfg(debug_assertions)]
            dictionary::snippet_offer::e2e_snippet_offer_reset,
            #[cfg(debug_assertions)]
            calls::meeting_guard::e2e_meeting_limit,
            #[cfg(debug_assertions)]
            calls::meeting_guard::e2e_meeting_checkins,
            #[cfg(debug_assertions)]
            calls::call_audio::e2e_call_audio,
            #[cfg(debug_assertions)]
            meetings::sleep::e2e_sleep,
            #[cfg(debug_assertions)]
            calendar::calendar_e2e_opened,
            #[cfg(debug_assertions)]
            calendar::calendar_e2e_call_window,
            #[cfg(debug_assertions)]
            shell::commands::e2e_opened_urls,
            #[cfg(debug_assertions)]
            voice::tts::tts_e2e_spoken,
            #[cfg(debug_assertions)]
            dictation::commands::e2e_chat_dictation,
            #[cfg(debug_assertions)]
            notes::quick_note::e2e_quick_note_dictation,
            #[cfg(debug_assertions)]
            notes::quick_note::send::e2e_quick_note_before,
            #[cfg(debug_assertions)]
            notes::quick_note::send::e2e_quick_note_sent,
            #[cfg(debug_assertions)]
            shell::tray::e2e_tray,
            #[cfg(debug_assertions)]
            dictation::commands::e2e_dictation,
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
            dictation::commands::e2e_rewrite,
            #[cfg(debug_assertions)]
            dictation::transforms::e2e_transform,
            #[cfg(debug_assertions)]
            dictation::transforms::e2e_transform_slots,
            #[cfg(debug_assertions)]
            dictation::languages::e2e_language_stage,
            #[cfg(debug_assertions)]
            dictation::languages::e2e_language_slots,
            #[cfg(debug_assertions)]
            dictation::languages::e2e_language_heard,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_transform,
            #[cfg(debug_assertions)]
            dictation::commands::e2e_target_window,
            #[cfg(debug_assertions)]
            ai::cleanup_styles::e2e_style_app,
            #[cfg(debug_assertions)]
            ai::cleanup_context::e2e_cursor_before,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_dictate_wav,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_clipboard,
            #[cfg(debug_assertions)]
            shell::realtest::commands::realtest_copy_selection,
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
            // Edit mode's Ctrl+C says when it left the clipboard alone (text_injector.rs).
            pasting::text_injector::init(&handle);
            // Temp files a crash mid-write left behind (config.rs): Yap's
            // own names only, off the startup thread.
            let _ = std::thread::Builder::new().name("temp-sweep".into()).spawn(config::sweep_temp_files);

            // Register the app handle so the usage tracker can emit live
            // `groq-usage` updates after each AI-cleanup call.
            ai::usage::set_app_handle(handle.clone());
            // Every AI call's tokens, kept 90 days in ai_usage.json
            // (usage/store.rs): written by its own thread.
            ai::usage::store::init(&handle);
            // Spoken replies (tts.rs): its state goes to every window.
            voice::tts::init(&handle);

            // Global input hook + dictation hotkey. (Not in e2e test runs: a
            // test instance must never see, or swallow, the developer's keys.
            // Robot mode runs it, acting on the robot's own keys only.)
            if (!shell::e2e::active() || shell::realtest::on()) && !bench_child() {
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
            // Paste last / Copy last transcript (pasting/last.rs; unbound by
            // default, never a clash).
            dictation::input_hook::configure_paste_last_from(&cfg);
            dictation::input_hook::configure_copy_last_from(&cfg);
            // Quick note (notes/quick_note.rs; unbound by default, never a clash).
            dictation::input_hook::configure_quick_note_from(&cfg);
            // More shortcuts for each ("+ Add another"; giving way to all of these).
            dictation::input_hook::configure_more_from(&cfg);
            // Language shortcuts (My languages, languages.rs; none by
            // default), then saved transforms' (transforms.rs; none by default).
            dictation::input_hook::configure_languages_from(&cfg);
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

            // Windows' microphones, read and watched (mics.rs): which one
            // dictation and meetings use, and a new one plugged in.
            dictation::mics::start(&handle);
            // The laptop's lid (lid.rs): closed, its own mic goes last.
            dictation::lid::start(&handle);
            // The stats (Home, Insights) loaded, history they haven't
            // counted counted, before any dictation (stats.rs).
            dictation::stats::init();
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
            // "Keep history: 24 hours": older dictations go, now and hourly
            // (history/keep.rs).
            dictation::history::keep::init(&handle);
            // Meeting audio past its days goes, and what a crash left of a
            // meeting is offered to transcribe (meeting_audio.rs).
            meetings::meeting_audio::init(&handle);
            // Meeting transcripts past "Delete meeting transcripts after"
            // go, their notes and summaries kept (notes/retention.rs).
            notes::retention::init(&handle);
            // A meeting recording watches the disk space it has left
            // (meetings/disk_watch.rs).
            meetings::disk_watch::init(&handle);

            // Clear any orphaned cleanup sidecar from a previous session (a
            // crash or task-kill skips every exit path; installers launched by
            // older builds' updater did too), then — only with "Load the
            // on-device model when Yap starts" on — start a fresh one
            // off-thread; otherwise it loads when it's first needed, and is
            // freed when idle (local_llm/on_demand.rs). (An e2e test run
            // leaves other processes alone.)
            if !shell::e2e::active() && !bench_child() {
                ai::local_llm::kill_orphans();
            }
            ai::local_llm::on_demand::init(&handle);
            {
                let cfg2 = cfg.clone();
                tauri::async_runtime::spawn(async move {
                    // A runtime cut off mid-swap (or taken by an antivirus)
                    // comes back first; leftovers go.
                    ai::local_llm::runtime::tidy_runtime().await;
                    ai::local_llm::autostart_if_configured(&cfg2).await;
                });
            }
            // An older llamafile runtime (installs before 22 Oct 2026) is
            // replaced in the background, never under a dictation.
            ai::local_llm::runtime::spawn_runtime_upgrade(handle.clone());

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
                shell::window::behind_in_test_runs(&settings);
            }
            // Release builds: a launch by the person brings the main window
            // up (`launch_shows_window`); sign-in and "Start hidden" stay in
            // the tray.
            #[cfg(not(debug_assertions))]
            if shell::autostart::launch_shows_window(
                shell::autostart::launched_at_sign_in(cfg.autostart),
                cfg.start_hidden,
                !dictation::commands::installed_models().is_empty(),
                updates::relaunch_after_update(),
            ) {
                if let Err(e) = shell::window::show_settings(&handle) {
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

            // Paste last / Copy last transcript (pasting/last.rs), each on
            // its own thread once the shortcut's keys are up.
            let paste_last_handle = handle.clone();
            handle.listen("paste-last-key-pressed", move |_| pasting::last::paste_soon(&paste_last_handle));
            let copy_last_handle = handle.clone();
            handle.listen("copy-last-key-pressed", move |_| pasting::last::copy_soon(&copy_last_handle));
            // Quick note (notes/quick_note/key.rs): a tap opens it, or puts
            // it away while it's in front; a hold dictates into it.
            let quick_note_handle = handle.clone();
            handle.listen("quick-note-key-pressed", move |event| {
                notes::quick_note::key::on_key(&quick_note_handle, true, event.payload().parse::<u64>().ok());
            });
            let quick_note_up_handle = handle.clone();
            handle.listen("quick-note-key-released", move |event| {
                notes::quick_note::key::on_key(&quick_note_up_handle, false, event.payload().parse::<u64>().ok());
            });
            snip::init(&handle);

            // A saved transform's shortcut (transforms.rs): its slot, run on
            // its own thread.
            let transform_handle = handle.clone();
            handle.listen("transform-key-pressed", move |event| {
                if let Ok(slot) = event.payload().trim().parse::<usize>() {
                    dictation::transforms::on_key(&transform_handle, slot);
                }
            });
            // A language shortcut's press and release (My languages): a
            // dictation in its language, as the dictation key's.
            dictation::languages::init(&handle);

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
                if let Err(e) = shell::autostart::set_autostart_enabled(&handle, cfg.autostart) {
                    tracing::debug!("autostart reconcile skipped: {}", e);
                }
            } else if cfg.autostart && !std::env::args().any(|a| a == shell::autostart::AUTOSTART_ARG) {
                // An entry written before `--autostart` existed starts Yap
                // without it, which would now open the window at every
                // sign-in: written again, with it. (A launch that carries
                // the argument proves the entry already has it.)
                if let Err(e) = shell::autostart::set_autostart_enabled(&handle, true) {
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
            for label in ["onboarding", "settings", "notepad", notes::quick_note::LABEL] {
                if let Some(w) = app.get_webview_window(label) {
                    shell::window::init_hidden_webview(&w);
                }
            }

            // The meeting notepad (docked beside a call) and the end of a
            // meeting (pause or end, "Started by mistake?", the action plan
            // written in Rust): both follow the recorder's events.
            meetings::notepad::init(&handle);
            meetings::meeting_end::init(&handle);
            // The quick note: closing it hides it (notes/quick_note.rs).
            notes::quick_note::init(&handle);
            // Memory: its events, and reading quiet chats for what to
            // remember (memory.rs).
            memory::init(&handle);
            // The speaker engine's voices onto meeting lines (beta).
            speakers::labels::init(&handle);

            // First run: if no model is downloaded yet, greet the user with the
            // onboarding model picker instead of a silent "needs-model" pill.
            // Suppressed when launched hidden (e.g. autostart at login).
            if !cfg.start_hidden && dictation::commands::installed_models().is_empty() {
                if let Err(e) = shell::window::show_onboarding(&handle) {
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
            // The call's own audio while it's recorded (its microphone,
            // muted or not), and the PC going to sleep mid-meeting.
            calls::call_audio::init(&handle);
            meetings::sleep::init(&handle);

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

            // Meeting search: every meeting and note read by the search
            // model in the background, when it's on (search.rs).
            search::init(&handle);

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
