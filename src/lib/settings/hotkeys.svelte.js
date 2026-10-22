// Settings' four global shortcuts (input_hook.rs): which command applies
// each, their names for clash messages, and whether one is being picked
// right now (Settings' in-page hotkey fallback stands down meanwhile, so
// pressing the current hotkey to replace it doesn't start a dictation).
// Shared by every page that shows a shortcut (Dictation, Commands, Voice
// Assistant, Meetings) and the Shortcuts page, which shows all four.
import { invoke } from '@tauri-apps/api/core';
import { formatHotkeySpec, hotkeysClash } from '../hotkeys.js';

export const HOTKEY_CMD = {
  hotkey: 'configure_hotkey',
  editHotkey: 'configure_edit_hotkey',
  meetingHotkey: 'configure_meeting_hotkey',
  assistantHotkey: 'configure_assistant_hotkey',
};

export const HOTKEY_NAMES = {
  hotkey: 'dictation hotkey',
  editHotkey: 'rewrite shortcut',
  meetingHotkey: 'meeting shortcut',
  assistantHotkey: 'assistant shortcut',
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
 * The four never fire on one press (`hotkeysClash`, input_hook.rs
 * `clashes`): the message for a refused spec, else null.
 */
export function validateHotkey(cfg, target, spec) {
  if (!spec) return null;
  const clash = Object.keys(HOTKEY_NAMES).find((other) => other !== target && hotkeysClash(cfg?.[other], spec));
  if (!clash) return null;
  return cfg[clash] === spec
    ? `Already used by the ${HOTKEY_NAMES[clash]}`
    : `Clashes with the ${HOTKEY_NAMES[clash]} (${formatHotkeySpec(cfg[clash])}): one press would start both`;
}
