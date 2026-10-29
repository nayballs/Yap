// Settings' eleven global shortcuts (dictation/input_hook.rs): which command applies
// each, their names for clash messages, and whether one is being picked
// right now (Settings' in-page hotkey fallback stands down meanwhile, so
// pressing the current hotkey to replace it doesn't start a dictation).
// Shared by every page that shows a shortcut (Dictation, Commands, Voice
// Assistant, Meetings) and the Shortcuts page, which shows them all. The
// saved transforms' shortcuts (Commands → Transforms) are checked against
// them, and they against the transforms'.
import { invoke } from '@tauri-apps/api/core';
import { formatHotkeySpec, hotkeysClash } from '../core/hotkeys.js';
import { transformName } from './transforms.js';
import { languageShortcuts, loadLanguageCatalog, shortcutName } from './languages.svelte.js';

// The language shortcuts' names ("the French shortcut") need the language list.
loadLanguageCatalog();

export const HOTKEY_CMD = {
  hotkey: 'configure_hotkey',
  editHotkey: 'configure_edit_hotkey',
  meetingHotkey: 'configure_meeting_hotkey',
  assistantHotkey: 'configure_assistant_hotkey',
  readHotkey: 'configure_read_hotkey',
  asSaidHotkey: 'configure_as_said_hotkey',
  snipHotkey: 'configure_snip_hotkey',
  pasteLastHotkey: 'configure_paste_last_hotkey',
  copyLastHotkey: 'configure_copy_last_hotkey',
  quickNoteHotkey: 'configure_quick_note_hotkey',
  sendHotkey: 'configure_send_hotkey',
};

export const HOTKEY_NAMES = {
  hotkey: 'dictation hotkey',
  editHotkey: 'rewrite shortcut',
  meetingHotkey: 'meeting shortcut',
  assistantHotkey: 'assistant shortcut',
  readHotkey: 'read-aloud shortcut',
  asSaidHotkey: 'Use-what-I-said shortcut',
  snipHotkey: 'snip shortcut',
  pasteLastHotkey: 'Paste-last shortcut',
  copyLastHotkey: 'Copy-last shortcut',
  quickNoteHotkey: 'Quick-note shortcut',
  sendHotkey: 'Send shortcut',
};

/** True while any <SettingsHotkey> is capturing a new key. */
export const hotkeyCapture = $state({ on: false });

/** More shortcuts for one action, beside its first (input_hook.rs `MORE`). */
export const MORE_SHORTCUTS = 3;

/** An action's more shortcuts ("+ Add another"): `cfg.moreHotkeys[target]`. */
export function moreHotkeys(cfg, target) {
  const list = cfg?.moreHotkeys?.[target];
  return Array.isArray(list) ? list.filter((s) => typeof s === 'string' && s) : [];
}

/** Set an action's more shortcuts (blanks dropped; none: the key goes). */
export function setMoreHotkeys(cfg, target, list) {
  const clean = list.filter(Boolean).slice(0, MORE_SHORTCUTS);
  const all = { ...(cfg.moreHotkeys || {}) };
  if (clean.length) all[target] = clean;
  else delete all[target];
  cfg.moreHotkeys = all;
}

/**
 * Capturing started or stopped: pause the live global binding while the
 * person picks (so pressing the current key doesn't fire it), then apply
 * the picked spec.
 */
export function onHotkeyCapturing(cfg, target, on) {
  if (!cfg) return;
  hotkeyCapture.on = on;
  invoke(HOTKEY_CMD[target], { spec: on ? '' : cfg[target] }).catch(() => {});
}

/**
 * One of an action's more shortcuts (`index`; one past the list: the one
 * being added) is being picked: that one paused, then the list applied.
 */
export function onMoreCapturing(cfg, target, index, on) {
  if (!cfg) return;
  hotkeyCapture.on = on;
  const specs = moreHotkeys(cfg, target).map((s, i) => (on && i === index ? '' : s));
  invoke('configure_more_hotkeys', { target, specs }).catch(() => {});
}

/**
 * Every shortcut but `except` (a fixed one's config key, `more:<key>:<n>`
 * for its more ones, `transform:<id>` or `language:<id>`), as [name, spec]:
 * the ten fixed ones with their more shortcuts (one of the action being
 * set is "another of this action's shortcuts"), the language shortcuts'
 * (dictation/languages.rs; "the French shortcut"), then the saved
 * transforms' (dictation/transforms.rs; "the Polish transform").
 */
function otherShortcuts(cfg, except) {
  const own = String(except).startsWith('more:') ? String(except).split(':')[1] : except;
  const nameOf = (k) => (k === own ? "another of this action's shortcuts" : `the ${HOTKEY_NAMES[k]}`);
  const fixed = Object.keys(HOTKEY_NAMES).flatMap((k) => [
    ...(k !== except ? [[nameOf(k), cfg?.[k]]] : []),
    ...moreHotkeys(cfg, k)
      .map((spec, i) => [`more:${k}:${i}`, spec])
      .filter(([id]) => id !== except)
      .map(([, spec]) => [nameOf(k), spec]),
  ]);
  const languages = languageShortcuts(cfg)
    .filter((s) => `language:${s.id}` !== except && s.hotkey)
    .map((s) => [`the ${shortcutName(s)} shortcut`, s.hotkey]);
  const transforms = (Array.isArray(cfg?.transforms) ? cfg.transforms : [])
    .filter((t) => `transform:${t.id}` !== except && t.hotkey)
    .map((t) => [`the ${transformName(t)} transform`, t.hotkey]);
  return [...fixed, ...languages, ...transforms];
}

/** The message for `spec` refused because of `[name, spec]`. */
function clashMessage([name, other], spec) {
  return other === spec
    ? `Already used by ${name}`
    : `Clashes with ${name} (${formatHotkeySpec(other)}): one press would start both`;
}

/**
 * The ten, the language shortcuts' and the saved transforms' never fire on one press
 * (`hotkeysClash`, dictation/input_hook.rs `clashes`): the message for a refused
 * spec, else null.
 */
export function validateHotkey(cfg, target, spec) {
  if (!spec) return null;
  // The Send shortcut presses Enter: Enter itself would press it again
  // (pasting/send_key.rs `is_enter`).
  const own = String(target).startsWith('more:') ? String(target).split(':')[1] : target;
  if (own === 'sendHotkey' && /^kb:(?:.*\+)?13$/.test(String(spec).trim())) return 'Enter sends already: pick another key';
  const clash = otherShortcuts(cfg, target).find(([, other]) => hotkeysClash(other, spec));
  return clash ? clashMessage(clash, spec) : null;
}

/** One of an action's more shortcuts (`index`), checked the same way. */
export function validateMoreHotkey(cfg, target, index, spec) {
  return validateHotkey(cfg, `more:${target}:${index}`, spec);
}

/** A saved transform's shortcut (`id`), checked the same way. */
export function validateTransformHotkey(cfg, id, spec) {
  return validateHotkey(cfg, `transform:${id}`, spec);
}

/** A language shortcut's key (`id`), checked the same way. */
export function validateLanguageHotkey(cfg, id, spec) {
  return validateHotkey(cfg, `language:${id}`, spec);
}

/** A language shortcut's key is being picked: its live binding paused. */
export function onLanguageCapturing(cfg, id, on) {
  hotkeyCapture.on = on;
  const s = languageShortcuts(cfg).find((x) => x.id === id);
  invoke('configure_language_hotkey', { id, spec: on ? '' : s?.hotkey || '' }).catch(() => {});
}

/** A transform's key is being picked: its live binding paused, as for the ten. */
export function onTransformCapturing(cfg, id, on) {
  hotkeyCapture.on = on;
  const t = (cfg?.transforms || []).find((x) => x.id === id);
  invoke('configure_transform_hotkey', { id, spec: on ? '' : t?.hotkey || '' }).catch(() => {});
}
