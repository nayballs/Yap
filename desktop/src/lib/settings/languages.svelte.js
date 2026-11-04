// My languages (dictation/languages.rs): the languages you speak, the
// language shortcuts and their names, as Settings shows them. The language
// list, which speech model covers which and the model to recommend are
// Rust's (`languages_catalog`, `my_languages_status`); the names are kept
// here once read, for the shortcuts' names in clash messages
// (hotkeys.svelte.js) and Settings → Shortcuts.
import { invoke } from '@tauri-apps/api/core';

/** The most language shortcuts (dictation/languages.rs `MAX_SHORTCUTS`). */
export const MAX_LANGUAGE_SHORTCUTS = 4;

/** A shortcut's "any of my languages" (and the main shortcut's Auto). */
export const AUTO = 'auto';

/** code → { code, name, native }, once the catalog is in. */
export const catalog = $state({ list: [], byCode: {} });

let loading = null;

/** Read the language list once (every caller gets the same promise). */
export function loadLanguageCatalog() {
  if (!loading) {
    loading = invoke('languages_catalog')
      .then((list) => {
        catalog.list = Array.isArray(list) ? list : [];
        catalog.byCode = Object.fromEntries(catalog.list.map((l) => [l.code, l]));
        return catalog.list;
      })
      .catch(() => {
        loading = null;
        return [];
      });
  }
  return loading;
}

/** "French" for "fr" (the code itself until the list is in). */
export function languageName(code) {
  return catalog.byCode[code]?.name ?? code ?? '';
}

/**
 * The person's languages as codes: the saved list, else (a config from
 * before My languages) the one language the Speech page had picked.
 */
export function myLanguages(cfg) {
  const list = Array.isArray(cfg?.myLanguages) ? cfg.myLanguages.filter(Boolean) : [];
  if (list.length) return [...new Set(list)];
  const one = cfg?.selectedLanguage;
  return one && one !== AUTO ? [one] : [];
}

/** "French", "English → Spanish", "Any language → German" (languages.rs `name`). */
export function shortcutName(s) {
  if (!s) return '';
  const spoken = s.language && s.language !== AUTO ? s.language : null;
  const from = spoken ? languageName(spoken) : 'Any language';
  const to = s.translateTo && s.translateTo !== spoken ? s.translateTo : null;
  return to ? `${from} → ${languageName(to)}` : from;
}

/** A new language shortcut: speaks `language`, pastes as spoken. */
export function newLanguageShortcut(language) {
  return {
    id: `l-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`,
    language: language || AUTO,
    translateTo: '',
    hotkey: '',
  };
}

/** Language shortcuts in config (none for a config from before them). */
export function languageShortcuts(cfg) {
  return Array.isArray(cfg?.languageShortcuts) ? cfg.languageShortcuts : [];
}
