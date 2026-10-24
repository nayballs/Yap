// Settings' seven global shortcuts (input_hook.rs): which command applies
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

export const HOTKEY_CMD = {
  hotkey: 'configure_hotkey',
  editHotkey: 'configure_edit_hotkey',
  meetingHotkey: 'configure_meeting_hotkey',
  assistantHotkey: 'configure_assistant_hotkey',
  readHotkey: 'configure_read_hotkey',
  asSaidHotkey: 'configure_as_said_hotkey',
  snipHotkey: 'configure_snip_hotkey',
};

export const HOTKEY_NAMES = {
  hotkey: 'dictation hotkey',
  editHotkey: 'rewrite shortcut',
  meetingHotkey: 'meeting shortcut',
  assistantHotkey: 'assistant shortcut',
  readHotkey: 'read-aloud shortcut',
  asSaidHotkey: 'Use-what-I-said shortcut',
  snipHotkey: 'snip shortcut',
};

/** True while any <SettingsHotkey> is capturing a new key. */
export const hotkeyCapture = $state({ on: false });

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
 * Every shortcut but `except` (a fixed one's config key, or `transform:<id>`),
 * as [name, spec]: the seven fixed ones, then the saved transforms'
 * (transforms.rs; "the Polish transform").
 */
function otherShortcuts(cfg, except) {
  const fixed = Object.keys(HOTKEY_NAMES)
    .filter((k) => k !== except)
    .map((k) => [`the ${HOTKEY_NAMES[k]}`, cfg?.[k]]);
  const transforms = (Array.isArray(cfg?.transforms) ? cfg.transforms : [])
    .filter((t) => `transform:${t.id}` !== except && t.hotkey)
    .map((t) => [`the ${transformName(t)} transform`, t.hotkey]);
  return [...fixed, ...transforms];
}

/** The message for `spec` refused because of `[name, spec]`. */
function clashMessage([name, other], spec) {
  return other === spec
    ? `Already used by ${name}`
    : `Clashes with ${name} (${formatHotkeySpec(other)}): one press would start both`;
}

/**
 * The seven, and the saved transforms', never fire on one press
 * (`hotkeysClash`, input_hook.rs `clashes`): the message for a refused
 * spec, else null.
 */
export function validateHotkey(cfg, target, spec) {
  if (!spec) return null;
  const clash = otherShortcuts(cfg, target).find(([, other]) => hotkeysClash(other, spec));
  return clash ? clashMessage(clash, spec) : null;
}

/** A saved transform's shortcut (`id`), checked the same way. */
export function validateTransformHotkey(cfg, id, spec) {
  return validateHotkey(cfg, `transform:${id}`, spec);
}

/** A transform's key is being picked: its live binding paused, as for the seven. */
export function onTransformCapturing(cfg, id, on) {
  hotkeyCapture.on = on;
  const t = (cfg?.transforms || []).find((x) => x.id === id);
  invoke('configure_transform_hotkey', { id, spec: on ? '' : t?.hotkey || '' }).catch(() => {});
}
