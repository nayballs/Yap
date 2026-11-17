//! **Claude Code's plugins** (17 Nov 2026: Settings → Yap Code → Plugins,
//! and a Code chat's + menu → Plugins → Manage plugins…): the ones installed,
//! from Claude Code's own documented `claude plugin list --json`, each
//! switched on or off with `claude plugin enable|disable <id> --scope <its
//! scope>` — the person's Claude Code setting, as in their terminal. Nothing
//! is ever installed or removed from Yap. A plugin's one line is its own
//! manifest's (`.claude-plugin/plugin.json`).
//!
//! The log has counts and outcomes, never a plugin's name.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::claude_code::cli::{self, Program};

/// How long `claude plugin …` gets.
const PLUGIN_WAIT: Duration = Duration::from_secs(30);

/// The scopes Yap may switch a plugin in (a plugin synced from claude.ai or
/// managed by an organisation is switched where it comes from).
const SWITCHABLE: &[&str] = &["user", "project", "local"];

/// **One installed plugin**, as Settings lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Installed {
    /// `name@marketplace`.
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub version: String,
    /// `user`, `project`, `local`, `synced`, `managed`, …
    pub scope: String,
    pub enabled: bool,
    /// Its manifest's one line.
    pub description: String,
    /// Yap may switch it (its scope is one of [`SWITCHABLE`]).
    pub switchable: bool,
    #[serde(skip_serializing)]
    pub path: String,
}

/// **`claude plugin list --json`'s answer**, read: every plugin (fields it
/// doesn't know left aside). Pure.
pub fn parse_list(stdout: &str) -> Option<Vec<Installed>> {
    let start = stdout.find('[')?;
    let end = stdout.rfind(']')?;
    let list: Vec<Value> = serde_json::from_str(stdout.get(start..=end)?).ok()?;
    Some(
        list.iter()
            .filter_map(|v| {
                let id = v.get("id").and_then(Value::as_str)?.trim().to_string();
                if id.is_empty() {
                    return None;
                }
                let (name, marketplace) = id.split_once('@').map(|(n, m)| (n.to_string(), m.to_string())).unwrap_or_else(|| (id.clone(), String::new()));
                let scope = v.get("scope").and_then(Value::as_str).unwrap_or_default().to_string();
                let on = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
                Some(Installed {
                    switchable: SWITCHABLE.contains(&scope.as_str()),
                    enabled: on("enabled") || on("projectEnabled"),
                    version: v.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
                    path: v.get("installPath").and_then(Value::as_str).unwrap_or_default().to_string(),
                    description: String::new(),
                    id,
                    name,
                    marketplace,
                    scope,
                })
            })
            .collect(),
    )
}

/// `claude plugin list --json` run in `dir` (its project's plugins too).
fn list_in(program: &Program, dir: &Path) -> Result<Vec<Installed>, String> {
    let place = cli::Place { dir: dir.to_path_buf(), env: cli::clean_env(std::env::vars_os()), what: "Claude Code" };
    let out = cli::run_captured_at(program, &["plugin", "list", "--json"], place, PLUGIN_WAIT)?;
    parse_list(&out.stdout).ok_or_else(|| "Claude Code didn't list its plugins.".to_string())
}

/// **The plugins on in `dir`** (a Code chat's + menu, before a run there
/// has said: `menu.rs`'s look).
#[cfg_attr(test, allow(dead_code))]
pub fn enabled_in(program: &Program, dir: &Path) -> Vec<super::menu::Plugin> {
    match list_in(program, dir) {
        Ok(list) => list
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| super::menu::Plugin { name: p.name, id: p.id, version: p.version, description: String::new(), path: p.path })
            .collect(),
        Err(e) => {
            tracing::info!("Plugins: couldn't list them ({e})");
            Vec::new()
        }
    }
}

/// What Settings shows.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// Claude Code is on this PC.
    pub installed: bool,
    pub plugins: Vec<Installed>,
    /// Why there's no list (an older Claude Code, an error), said plainly.
    pub note: Option<String>,
}

/// **Settings → Yap Code → Plugins**: every installed plugin.
#[tauri::command]
pub async fn claude_plugins() -> Result<Listing, String> {
    let Some(program) = cli::find() else { return Ok(Listing { installed: false, plugins: Vec::new(), note: None }) };
    tauri::async_runtime::spawn_blocking(move || match list_in(&program, &std::env::temp_dir()) {
        Ok(mut plugins) => {
            for p in plugins.iter_mut() {
                p.description = super::menu::manifest_line(&p.path);
            }
            tracing::info!(plugins = plugins.len(), on = plugins.iter().filter(|p| p.enabled).count(), "Plugins: listed");
            Listing { installed: true, plugins, note: None }
        }
        Err(e) => {
            tracing::info!("Plugins: couldn't list them ({e})");
            Listing { installed: true, plugins: Vec::new(), note: Some("Claude Code didn't list its plugins: manage them in Claude Code (/plugin).".into()) }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The arguments that switch plugin `id` on or off in `scope`. Pure.
pub fn switch_args(id: &str, on: bool, scope: &str) -> Result<Vec<String>, String> {
    let id = id.trim();
    // (Its id as Claude Code lists it: never anything a shell or Claude Code would read as more.)
    if id.is_empty() || id.starts_with('-') || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '-' | '_' | '.' | ':')) {
        return Err("That isn't one of Claude Code's plugins.".into());
    }
    if !SWITCHABLE.contains(&scope) {
        return Err("That plugin is switched where it comes from (claude.ai, or your organisation), not here.".into());
    }
    Ok(vec!["plugin".into(), if on { "enable" } else { "disable" }.into(), id.into(), "--scope".into(), scope.into()])
}

/// **A plugin switched on or off** (Settings → Yap Code → Plugins): the
/// person's Claude Code setting. Every Code chat's menu is looked at again.
#[tauri::command]
pub async fn claude_plugin_set(id: String, on: bool, scope: String) -> Result<(), String> {
    let program = cli::find().ok_or("Claude Code isn't installed on this PC.")?;
    let args = switch_args(&id, on, &scope)?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        cli::run_captured(&program, &args, PLUGIN_WAIT)
    })
    .await
    .map_err(|e| e.to_string())??;
    if out.code != Some(0) {
        let said = out.stderr.lines().chain(out.stdout.lines()).map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect::<String>();
        tracing::info!(on, code = ?out.code, "Plugins: Claude Code didn't switch one");
        return Err(if said.is_empty() { "Claude Code couldn't switch that plugin.".into() } else { format!("Claude Code couldn't switch that plugin: {said}") });
    }
    tracing::info!(on, "Plugins: one switched");
    super::menu::stale();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_list_is_read_with_each_plugins_scope_and_whether_yap_may_switch_it() {
        let out = r#"[
          {"id":"garden-tools@example-market","version":"1.2.0","scope":"user","enabled":true,"installPath":"C:\\Users\\imogen\\.claude\\plugins\\cache\\example-market\\garden-tools\\1.2.0","projectEnabled":false},
          {"id":"lint-helper@example-market","version":"0.3.1","scope":"project","enabled":false,"projectEnabled":true},
          {"id":"team-kit@synced","version":"0.2.0","scope":"synced","enabled":true},
          {"id":"","scope":"user"},
          {"id":"old-thing@example-market","version":"1.0.0","scope":"user","enabled":false}
        ]"#;
        let list = parse_list(out).unwrap();
        assert_eq!(list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["garden-tools", "lint-helper", "team-kit", "old-thing"]);
        assert_eq!(list[0].marketplace, "example-market");
        assert!(list[0].enabled && list[0].switchable);
        assert!(list[1].enabled, "on in its project");
        assert!(!list[2].switchable, "a synced plugin is switched where it comes from");
        assert!(!list[3].enabled);
        assert!(parse_list("not json").is_none());
        assert_eq!(parse_list("Plugins:\n[]\n").unwrap(), Vec::new());
    }

    #[test]
    fn a_switch_names_the_plugin_and_its_scope_and_nothing_else() {
        assert_eq!(switch_args("garden-tools@example-market", false, "user").unwrap(), ["plugin", "disable", "garden-tools@example-market", "--scope", "user"]);
        assert_eq!(switch_args("garden-tools@example-market", true, "project").unwrap()[1], "enable");
        assert!(switch_args("--all", false, "user").is_err());
        assert!(switch_args("a b", false, "user").is_err());
        assert!(switch_args("x&calc", false, "user").is_err());
        assert!(switch_args("team-kit@synced", false, "synced").is_err());
    }
}
