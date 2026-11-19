//! **`open_application`** (phase 3, the desktop contract's): an allowed app
//! brought to the front — a window of it raised (restored if minimised),
//! else the app started from the Start menu's own entry (`apps::open`, never
//! a path the model wrote) and its first window brought forward.
//!
//! Only an app on the chat's list, at any tier: bringing an app forward only
//! shows it. Yap has no background app mode (the desktop app's `app_*`
//! tools): an app opened is in front, as under the desktop app's full-screen
//! control. A test run starts nothing (`apps::open` records it; the stand-in
//! desktop gets a window for it).

use std::time::Duration;

use serde_json::{json, Value};

use super::grants::{self, Granted};
use super::{failed, shot, Ctx, Run};

pub const OPEN_APPLICATION: &str = "open_application";

/// What opening an app did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    /// A window of it was on screen or minimised: now in front.
    Brought,
    /// Started; its window is in front.
    Started,
    /// Started; no window yet.
    Starting,
}

/// How long a started app's first window is waited for.
const WINDOW_WAIT: Duration = Duration::from_secs(8);

/// Its definition, as the tool loop takes it. Pure.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": OPEN_APPLICATION,
            "description": "Bring an allowed app to the front, starting it if it isn't running (a minimised window is restored). The app must already be allowed in this chat: call request_access first. Take a screenshot after it.",
            "parameters": {
                "type": "object",
                "properties": { "app": { "type": "string", "description": "The app as the Start menu names it (\"Notepad\", \"Paint\"), as request_access allowed it." } },
                "required": ["app"],
                "additionalProperties": false
            }
        }
    })
}

/// **The allowed app the model named**: its name exactly (any case), else
/// the one allowed app whose name holds it or is held by it. Pure.
pub fn which<'a>(apps: &'a [Granted], asked: &str) -> Option<&'a Granted> {
    let a = asked.trim().to_lowercase();
    if a.is_empty() {
        return None;
    }
    if let Some(g) = apps.iter().find(|g| g.name.to_lowercase() == a) {
        return Some(g);
    }
    let near: Vec<&Granted> = apps.iter().filter(|g| {
        let n = g.name.to_lowercase();
        n.contains(&a) || a.contains(&n)
    }).collect();
    (near.len() == 1).then(|| near[0])
}

/// **`open_application`** for the chat `ctx` says.
pub async fn execute(args: &Value, ctx: &Ctx<'_>) -> Run {
    let asked = args["app"].as_str().unwrap_or("").trim().to_string();
    if asked.is_empty() {
        return failed("app is needed: the app's name as the Start menu has it.", "Didn't open an app: none named").into();
    }
    let (apps, display) = grants::with(ctx.conversation, |s| (s.apps.clone(), s.last.as_ref().map(|l| l.frame.display)));
    if apps.is_empty() {
        return failed("No apps are allowed in this chat yet: call request_access first.", "Didn't open an app: none allowed").into();
    }
    let Some(g) = which(&apps, &asked).cloned() else {
        let allowed: Vec<&str> = apps.iter().map(|g| g.name.as_str()).collect();
        return failed(
            &format!("\"{asked}\" isn't one of the apps allowed in this chat ({}): call request_access for it first.", allowed.join(", ")),
            &format!("Didn't open {asked}: not allowed"),
        )
        .into();
    };
    // The PC, for this chat (one at a time; never after the person took over this turn).
    let driving = match super::watch::begin(ctx.conversation, ctx.turn) {
        Ok(d) => d,
        Err((error, chip)) => return failed(error, chip).into(),
    };
    let hands = match shot::hands() {
        Ok(h) => h,
        Err(e) => return failed(&format!("Yap can't open apps here ({e})."), &format!("Couldn't open {}", g.name)).into(),
    };
    let entry = crate::apps::index::apps(false).iter().find(|a| a.id == g.id).cloned();
    let name = g.name.clone();
    super::indicator::driving(&format!("Opening {name}"), display);
    let opened = tauri::async_runtime::spawn_blocking(move || hands.open(&g, entry.as_ref())).await;
    super::indicator::done();
    drop(driving);
    let (note, chip) = match opened {
        Ok(Ok(Opened::Brought)) => (format!("{name} is in front. Take a screenshot to see it."), format!("Brought {name} to the front")),
        Ok(Ok(Opened::Started)) => (format!("Started {name}; its window is in front. Take a screenshot to see it."), format!("Opened {name}")),
        Ok(Ok(Opened::Starting)) => (format!("Started {name}, but its window isn't up yet: wait a moment, then take a screenshot."), format!("Opened {name}")),
        Ok(Err(e)) => {
            tracing::info!("Computer use: an app didn't open");
            return failed(&format!("Yap couldn't open {name} ({e}). Ask the user to open it."), &format!("Couldn't open {name}")).into();
        }
        Err(_) => return failed(&format!("Yap couldn't open {name}."), &format!("Couldn't open {name}")).into(),
    };
    json!({ "success": true, "data": { "app": name }, "note": note, "displayText": chip }).into()
}

/// **On this PC**: a window of the app (never the desktop or the taskbar,
/// File Explorer's too) raised, else the app started and its first window
/// brought forward once it's up. Blocking.
#[cfg(windows)]
pub fn on_this_pc(g: &Granted, app: Option<&crate::apps::App>) -> Result<Opened, String> {
    use super::windows::{self, Win, SHELL_CLASSES};
    let mine = |w: &Win| g.owns(w) && !SHELL_CLASSES.contains(&w.class.as_str());
    if let Some(w) = windows::on_screen().into_iter().find(mine).or_else(|| windows::minimised().into_iter().find(mine)) {
        windows::raise(w.hwnd);
        return Ok(Opened::Brought);
    }
    let app = app.ok_or("it isn't in the Start menu any more")?;
    crate::apps::open(app)?;
    let since = std::time::Instant::now();
    while since.elapsed() < WINDOW_WAIT {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(w) = windows::on_screen().into_iter().find(mine) {
            windows::raise(w.hwnd);
            return Ok(Opened::Started);
        }
    }
    Ok(Opened::Starting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_named_is_one_allowed() {
        let app = |id: &str, name: &str| Granted::of(&crate::apps::index::App::new(id, name));
        let apps = vec![app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad"), app("Microsoft.Paint_8wekyb3d8bbwe!App", "Paint"), app(r"C:\Tools\paint3d.exe", "Paint 3D")];
        assert_eq!(which(&apps, "notepad").map(|g| g.name.as_str()), Some("Notepad"));
        assert_eq!(which(&apps, " Paint ").map(|g| g.name.as_str()), Some("Paint"), "an exact name wins");
        assert_eq!(which(&apps, "3D").map(|g| g.name.as_str()), Some("Paint 3D"));
        assert!(which(&apps, "Calculator").is_none(), "not allowed");
        assert!(which(&apps, "").is_none());
        assert!(which(&apps, "pa").is_none(), "two allowed apps hold it: ask, don't guess");
    }
}
