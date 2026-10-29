//! **The Help menu's shortcut sheet** (2026-10-29): every global shortcut as
//! it's set now, read from the input hook's own list of bindings
//! (`input_hook::FIXED_KEYS`, each action's first shortcut and its more), so
//! a binding added there shows up here by itself — then the saved
//! transforms' and the language shortcuts' keys. The page names each
//! action (`lib/views/help/ShortcutSheet.svelte`); one it doesn't know yet
//! goes by the hook's own name for it.

use serde::Serialize;

use crate::dictation::input_hook;

/// One action's shortcuts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// Its config key ("hotkey", "meetingHotkey"…), or a transform's or
    /// language shortcut's id.
    pub key: String,
    /// The hook's name for it ("meeting shortcut"), or the transform's or
    /// language's.
    pub name: String,
    /// Its keys (input_hook specs), the first first; none: unbound.
    pub specs: Vec<String>,
}

/// The sheet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    /// "toggle" | "pushToTalk": how the dictation key works.
    pub mode: String,
    pub fixed: Vec<Binding>,
    pub transforms: Vec<Binding>,
    pub languages: Vec<Binding>,
}

/// Every binding in `cfg`, in the hook's order. Pure.
pub fn sheet(cfg: &crate::config::YapConfig) -> Sheet {
    let fixed = input_hook::FIXED_KEYS
        .iter()
        .enumerate()
        .map(|(i, key)| {
            let first = input_hook::first_spec(cfg, i).trim().to_string();
            let specs = std::iter::once(first).chain(input_hook::more_specs(cfg, i)).filter(|s| !s.is_empty()).collect();
            Binding { key: (*key).to_string(), name: input_hook::FIXED_NAMES[i].to_string(), specs }
        })
        .collect();
    let transforms = cfg
        .transforms
        .iter()
        .filter(|t| !t.hotkey.trim().is_empty())
        .map(|t| Binding { key: t.id.clone(), name: t.name(), specs: vec![t.hotkey.trim().to_string()] })
        .collect();
    let languages = cfg
        .language_shortcuts
        .iter()
        .filter(|l| !l.hotkey.trim().is_empty())
        .map(|l| Binding { key: l.id.clone(), name: l.name(), specs: vec![l.hotkey.trim().to_string()] })
        .collect();
    Sheet { mode: cfg.recording_mode.clone(), fixed, transforms, languages }
}

/// The Help menu's Keyboard shortcuts (and Ctrl+/).
#[tauri::command]
pub fn shortcut_sheet() -> Sheet {
    sheet(&crate::config::load())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_the_hook_knows_is_on_the_sheet_as_set() {
        let mut cfg = crate::config::YapConfig { hotkey: "kb:120".into(), snip_hotkey: String::new(), ..Default::default() };
        cfg.more_hotkeys.insert("hotkey".into(), vec!["mouse:4".into()]);
        let s = sheet(&cfg);
        assert_eq!(s.fixed.len(), input_hook::FIXED_COUNT);
        assert_eq!(s.fixed.iter().map(|b| b.key.as_str()).collect::<Vec<_>>(), input_hook::FIXED_KEYS);
        let dictate = &s.fixed[0];
        assert_eq!(dictate.specs, ["kb:120", "mouse:4"]);
        assert_eq!(dictate.name, "dictation hotkey");
        let snip = s.fixed.iter().find(|b| b.key == "snipHotkey").unwrap();
        assert!(snip.specs.is_empty(), "unbound");
        assert_eq!(s.mode, cfg.recording_mode);
    }

    #[test]
    fn transforms_and_language_shortcuts_with_a_key_are_listed() {
        let mut cfg = crate::config::YapConfig::default();
        let mut polish = cfg.transforms.first().cloned().unwrap_or_default();
        polish.hotkey = "kb:ctrl+alt+80".into();
        cfg.transforms = vec![polish.clone(), crate::dictation::transforms::Transform { hotkey: String::new(), ..polish.clone() }];
        let s = sheet(&cfg);
        assert_eq!(s.transforms.len(), 1);
        assert_eq!(s.transforms[0].specs, ["kb:ctrl+alt+80"]);
        assert_eq!(s.transforms[0].name, polish.name());
        assert!(s.languages.is_empty());
    }
}
