// Shared hotkey-spec helpers for the capture UI and the in-window fallbacks.
//
// Spec formats (kept in sync with src-tauri/src/dictation/input_hook.rs parse_key_spec):
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
// How a combo reads: Win first, as Windows writes its own shortcuts
// ("Win + Alt + M"). Specs keep MOD_ORDER.
const DISPLAY_ORDER = ['win', 'ctrl', 'alt', 'shift'];
const byDisplayOrder = (mods) =>
  [...mods].sort((a, b) => DISPLAY_ORDER.indexOf(a) - DISPLAY_ORDER.indexOf(b));

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

// The modifier family of a side-specific (or generic) modifier VK, else null.
const VK_FAMILY = {
  16: 'shift', 160: 'shift', 161: 'shift',
  17: 'ctrl', 162: 'ctrl', 163: 'ctrl',
  18: 'alt', 164: 'alt', 165: 'alt',
  91: 'win', 92: 'win',
};
const ALIASES = { control: 'ctrl', option: 'alt', super: 'win', meta: 'win', cmd: 'win', command: 'win' };
const modSet = (mods) => new Set(mods.map((m) => ALIASES[m] || m));
const within = (a, b) => [...a].every((m) => b.has(m));

/// Whether two specs would fire on one press, so both would start: the same
/// rule as dictation/input_hook.rs `clashes`. The same key, button or chord however
/// it's written; a bare key fires only with no modifier held (F9 and Ctrl+F9
/// are two keys) unless it's a modifier itself (Right Alt); combos on one key
/// nest (Ctrl+Space fires inside Ctrl+Shift+Space); a chord completes inside
/// a combo or a bigger chord holding its modifiers (Ctrl+Alt in Ctrl+Alt+K).
export function hotkeysClash(a, b) {
  const p = parseHotkeySpec((a || '').trim());
  const q = parseHotkeySpec((b || '').trim());
  if (p.kind === 'none' || q.kind === 'none') return false;
  if (p.kind === 'mouse' || q.kind === 'mouse') return p.kind === q.kind && p.button === q.button;
  if (p.kind === 'kb' && q.kind === 'kb') {
    if (p.vk !== q.vk) return false;
    const m = modSet(p.mods);
    const n = modSet(q.mods);
    if (!m.size || !n.size) return m.size === n.size || !!VK_FAMILY[p.vk];
    return within(m, n) || within(n, m);
  }
  if (p.kind === 'mods' && q.kind === 'mods') {
    const m = modSet(p.mods);
    const n = modSet(q.mods);
    return within(m, n) || within(n, m);
  }
  const [chord, key] = p.kind === 'mods' ? [p, q] : [q, p];
  const c = modSet(chord.mods);
  const m = modSet(key.mods);
  if (!m.size) return !!VK_FAMILY[key.vk] && c.has(VK_FAMILY[key.vk]);
  return within(c, m);
}

/// The Copilot key on new Windows 11 laptops: the keyboard sends Win + Shift
/// + F23 (Microsoft's spec), which the input hook binds like any combo — F23
/// is swallowed, so Copilot doesn't open, and the menu-mask tap keeps the
/// Start menu shut. Shown as one key, "Copilot". Nothing tells a PC that has
/// one apart, and Windows may take the key before Yap's window sees it, so
/// the shortcut picker offers it as a button too (HotkeyInput).
export const COPILOT_KEY = 'kb:shift+win+134';

/// Is this spec the Copilot key (Win + Shift + F23, however it's written)?
export function isCopilotKey(spec) {
  const p = parseHotkeySpec((spec || '').trim());
  if (p.kind !== 'kb' || p.vk !== 134) return false;
  const m = modSet(p.mods);
  return m.size === 2 && m.has('shift') && m.has('win');
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
    44: 'Print Screen',
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
  if (isCopilotKey(spec)) return ['Copilot'];
  const p = parseHotkeySpec(spec);
  if (p.kind === 'mouse') return [`Mouse ${p.button}`];
  if (p.kind === 'mods') return byDisplayOrder(p.mods).map((m) => MOD_LABELS[m] || m);
  if (p.kind === 'kb') return [...byDisplayOrder(p.mods).map((m) => MOD_LABELS[m] || m), vkeyName(p.vk)];
  return [];
}

export function formatHotkeySpec(spec) {
  if (isCopilotKey(spec)) return 'Copilot';
  const parts = hotkeyParts(spec);
  if (!parts.length) return 'None';
  const p = parseHotkeySpec(spec);
  return parts.join(' + ') + (p.kind === 'mods' ? ' (hold)' : '');
}

/// A notice's text with `{key}` written as the hotkey's name: Rust's
/// push-to-talk coaching hints (dictation/coaching.rs) carry the spec and leave the
/// naming to the page. ("Hold Right Alt while you talk", no "(hold)".)
export function fillHotkey(text, spec) {
  if (!text) return '';
  if (isCopilotKey(spec)) return text.replaceAll('{key}', 'the Copilot key');
  const parts = hotkeyParts(spec);
  return text.replaceAll('{key}', parts.length ? parts.join(' + ') : 'the hotkey');
}

/// Does this keydown event trigger the binding? (In-window fallback matcher —
/// mirrors dictation/input_hook.rs semantics: combos need their modifiers held, a
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

/// Every key of one of the ten actions (dictation/input_hook.rs): its first
/// shortcut (`cfg[key]`: "hotkey", "meetingHotkey"…) and its more ones
/// ("+ Add another", `cfg.moreHotkeys[key]`). For the in-page fallbacks.
export function actionHotkeys(cfg, key) {
  const more = cfg?.moreHotkeys?.[key];
  return [cfg?.[key], ...(Array.isArray(more) ? more : [])].filter((s) => typeof s === 'string' && s);
}

/// Does this keydown press any of the action's keys?
export function actionMatchesKeydown(e, cfg, key) {
  return actionHotkeys(cfg, key).some((spec) => hotkeyMatchesKeydown(e, spec));
}

/// Does this keyup end any of the action's keys?
export function actionMatchesKeyup(e, cfg, key) {
  return actionHotkeys(cfg, key).some((spec) => hotkeyMatchesKeyup(e, spec));
}
