// Shared hotkey-spec helpers for the capture UI and the in-window fallbacks.
//
// Spec formats (kept in sync with src-tauri/src/input_hook.rs parse_key_spec):
//   "kb:120"            — bare key (F9)
//   "kb:ctrl+shift+32"  — modifier combo (Ctrl+Shift+Space)
//   "kb:165"            — single right-side modifier (VK_RMENU = RightAlt)
//   "mods:ctrl+alt"     — modifier-only chord (hold Ctrl+Alt)
//   "mouse:4"           — mouse button
//
// On Windows Chrome/WebView2, `e.keyCode` IS the Win32 virtual-key code for
// non-modifier keys — the recorder has always exploited that. Modifier keys
// report the *generic* VK (16/17/18/91) though, so side-specific bindings
// (RightAlt = 165) are matched via `e.code` ("AltRight") instead.

export const MOD_ORDER = ['ctrl', 'alt', 'shift', 'win'];
export const MOD_LABELS = { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', win: 'Win' };

// DOM e.code → Win32 VK for side-specific modifier keys.
export const MOD_CODE_TO_VK = {
  ShiftLeft: 160,
  ShiftRight: 161,
  ControlLeft: 162,
  ControlRight: 163,
  AltLeft: 164,
  AltRight: 165,
  MetaLeft: 91,
  MetaRight: 92,
};
const VK_TO_MOD_CODE = Object.fromEntries(
  Object.entries(MOD_CODE_TO_VK).map(([code, vk]) => [vk, code])
);

/// Modifier family ('ctrl'|'alt'|'shift'|'win') of a keyboard event, or null.
export function eventModifierFamily(e) {
  switch (e.keyCode) {
    case 16:
      return 'shift';
    case 17:
      return 'ctrl';
    case 18:
      return 'alt';
    case 91:
    case 92:
    case 93:
      return 'win';
    default:
      return null;
  }
}

function eventMods(e) {
  return { ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, win: e.metaKey };
}

export function parseHotkeySpec(spec) {
  if (!spec) return { kind: 'none' };
  if (spec.startsWith('mouse:')) return { kind: 'mouse', button: +spec.slice(6) };
  if (spec.startsWith('MouseButton')) return { kind: 'mouse', button: +spec.slice(11) };
  if (spec.startsWith('mods:')) {
    return { kind: 'mods', mods: spec.slice(5).toLowerCase().split('+').filter(Boolean) };
  }
  if (spec.startsWith('kb:')) {
    const parts = spec.slice(3).split('+');
    const vk = +parts[parts.length - 1];
    if (Number.isNaN(vk)) return { kind: 'none' };
    return { kind: 'kb', vk, mods: parts.slice(0, -1).map((p) => p.toLowerCase()) };
  }
  return { kind: 'none' };
}

export function vkeyName(v) {
  if (v >= 112 && v <= 135) return `F${v - 111}`;
  if ((v >= 48 && v <= 57) || (v >= 65 && v <= 90)) return String.fromCharCode(v);
  if (v >= 96 && v <= 105) return `Num ${v - 96}`;
  const named = {
    8: 'Backspace',
    9: 'Tab',
    13: 'Enter',
    19: 'Pause',
    20: 'CapsLock',
    32: 'Space',
    33: 'PageUp',
    34: 'PageDown',
    35: 'End',
    36: 'Home',
    37: 'Left',
    38: 'Up',
    39: 'Right',
    40: 'Down',
    45: 'Insert',
    46: 'Delete',
    91: 'Left Win',
    92: 'Right Win',
    144: 'NumLock',
    145: 'ScrollLock',
    160: 'Left Shift',
    161: 'Right Shift',
    162: 'Left Ctrl',
    163: 'Right Ctrl',
    164: 'Left Alt',
    165: 'Right Alt',
    186: ';',
    187: '=',
    188: ',',
    189: '-',
    190: '.',
    191: '/',
    192: '`',
    219: '[',
    220: '\\',
    221: ']',
    222: "'",
  };
  return named[v] || `Key ${v}`;
}

/// The display pieces of a spec, e.g. ['Ctrl','Shift','Space'] — chips-ready.
export function hotkeyParts(spec) {
  const p = parseHotkeySpec(spec);
  if (p.kind === 'mouse') return [`Mouse ${p.button}`];
  if (p.kind === 'mods') return p.mods.map((m) => MOD_LABELS[m] || m);
  if (p.kind === 'kb') return [...p.mods.map((m) => MOD_LABELS[m] || m), vkeyName(p.vk)];
  return [];
}

export function formatHotkeySpec(spec) {
  const parts = hotkeyParts(spec);
  if (!parts.length) return 'None';
  const p = parseHotkeySpec(spec);
  return parts.join(' + ') + (p.kind === 'mods' ? ' (hold)' : '');
}

/// Does this keydown event trigger the binding? (In-window fallback matcher —
/// mirrors input_hook.rs semantics: combos need their modifiers held, a
/// modifier-only chord fires when it completes, side-specific modifier
/// bindings match via e.code.)
export function hotkeyMatchesKeydown(e, spec) {
  const p = parseHotkeySpec(spec);
  if (p.kind === 'kb') {
    if (VK_TO_MOD_CODE[p.vk]) return e.code === VK_TO_MOD_CODE[p.vk];
    if (e.keyCode !== p.vk) return false;
    const held = eventMods(e);
    return p.mods.every((m) => held[m]);
  }
  if (p.kind === 'mods') {
    if (!eventModifierFamily(e)) return false;
    const held = eventMods(e);
    return p.mods.every((m) => held[m]);
  }
  return false;
}

/// Does this keyup event end the binding's press? (main key up, the bound
/// side-specific modifier up, or a chord/required modifier released)
export function hotkeyMatchesKeyup(e, spec) {
  const p = parseHotkeySpec(spec);
  if (p.kind === 'kb') {
    if (VK_TO_MOD_CODE[p.vk]) return e.code === VK_TO_MOD_CODE[p.vk];
    if (e.keyCode === p.vk) return true;
    // releasing a required modifier ends a combo press (PTT can't stick)
    const fam = eventModifierFamily(e);
    return !!fam && p.mods.includes(fam);
  }
  if (p.kind === 'mods') {
    const fam = eventModifierFamily(e);
    return !!fam && p.mods.includes(fam);
  }
  return false;
}
