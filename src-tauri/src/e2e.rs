//! Test mode for the end-to-end UI suite (`e2e/`, see docs/e2e-tests.md).
//!
//! The suite runs a **debug** build with `YAP_E2E=1` right next to whatever
//! Yap the developer has open, and drives its webviews over CDP. Test mode
//! keeps that instance inside its own windows and portable data dir:
//!
//! - no global keyboard/mouse hook: it never sees, or swallows, real keys;
//! - nothing is pasted, typed or copied into other apps (a dictation's text
//!   still reaches history and the Home feed);
//! - its windows never take the foreground (tao's `set_focus` fallback presses
//!   Alt in whichever app has focus) and it posts no Windows notifications;
//! - it leaves other processes alone (no "orphaned sidecar" sweep) and never
//!   reads or writes the installed app's saved window size and position;
//! - it quits cleanly (tray icon, exit hooks) when the harness closes its
//!   stdin, which also happens when the harness dies.
//!
//! Release builds compile all of this out: [`active`] is always `false`
//! without `debug_assertions`.

use tauri::AppHandle;

/// `true` in a debug build started with `YAP_E2E=1`.
pub fn active() -> bool {
    #[cfg(debug_assertions)]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var("YAP_E2E").is_ok_and(|v| v.trim() == "1"))
    }
    #[cfg(not(debug_assertions))]
    false
}

/// End of setup: say so in the log (the harness won't drive an instance that
/// doesn't) and quit once the harness closes our stdin (it finished, or died).
pub fn start(app: &AppHandle) {
    if !active() {
        return;
    }
    tracing::info!("e2e: test mode on");
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        tracing::info!("e2e: stdin closed, exiting");
        app.exit(0);
    });
}
