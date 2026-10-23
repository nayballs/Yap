// Settings → General → Appearance (config `theme`, `readingFont`,
// `readingSize`, `motion`) and the Yap bar's colours (`barTheme`), applied to
// EVERY Yap window: the main window, onboarding, the meeting notepad, the Yap
// bar and the split preview all call `initAppearance()` (App.svelte). It
// marks <html> and app.css does the rest:
//
//   data-yap-theme = light | dark (the Theme: System = Windows' app mode,
//       `prefers-color-scheme`, followed live as Windows switches)
//       → every --yap-* / --yap-paper-* token (app.css's dark block)
//   data-bar-theme = light | dark (Bar colours: Match app = data-yap-theme)
//       → the --bar-* tokens (the Yap bar, and its cards' picture in the
//         Connect calendar dialog)
//   data-reading-font = default | sans | serif | system | dyslexic
//       → --yap-font-reading (+ --yap-reading-fit / --yap-reading-leading,
//         so every font reads at about the same size)
//   data-reading-size = small | default | large | larger
//       → --yap-reading-step; --yap-reading-scale = step × fit
//   data-motion = reduced (absent = full motion)
//       → no non-essential animation or transition (app.css), and the Yap
//         bar's Svelte transitions take 0 ms (`motionMs`).
//
// Only the reading text uses the reading tokens — Chat's messages, notes and
// their Enhanced view, the notepad's My thoughts / Transcript / Summary and
// its Ask answers — never the buttons, menus or sidebar.
//
// The windows follow a change live: Rust emits `yap-config-saved` after
// every save (commands::save_config) and each window reads the config again.
// Settings also applies its own copy at once (before the 400 ms auto-save),
// so the main window changes as you pick. "System" motion follows Windows'
// "Animation effects" (Settings → Accessibility → Visual effects), which
// WebView2 reports as `prefers-reduced-motion`, as it changes; "System"
// theme follows Windows' app mode the same way (`prefers-color-scheme`).
//
// No flash of the wrong theme: the theme choice is kept in this origin's
// localStorage (`yap.theme`, `yap.barTheme`; every Yap window shares it),
// which index.html's head script reads before the first paint, and Rust
// gives the opaque windows' webviews the same background (theme.rs).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** Settings → General → Appearance → Theme. */
export const THEME_OPTIONS = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

/** Settings → General → Yap bar → Colours. */
export const BAR_THEME_OPTIONS = [
  { value: 'app', label: 'Match app' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

const THEME_IDS = new Set(THEME_OPTIONS.map((t) => t.value));
const BAR_THEME_IDS = new Set(BAR_THEME_OPTIONS.map((t) => t.value));
const THEME_KEY = 'yap.theme';
const BAR_THEME_KEY = 'yap.barTheme';

function stored(key, ids, fallback) {
  try {
    const v = localStorage.getItem(key);
    return ids.has(v) ? v : fallback;
  } catch {
    return fallback;
  }
}
function store(key, value) {
  try {
    if (localStorage.getItem(key) !== value) localStorage.setItem(key, value);
  } catch {
    /* private storage off: the head script falls back to System */
  }
}

// The choices in force in this window: the last config seen, else what the
// head script used (so a window doesn't blink to the default while its
// first get_config is on the way).
let themeChoice = stored(THEME_KEY, THEME_IDS, 'system');
let barChoice = stored(BAR_THEME_KEY, BAR_THEME_IDS, 'app');
let osDark = null; // MediaQueryList, once initAppearance ran

/** 'light' | 'dark': the theme `choice` resolves to with Windows' app mode
 *  dark (`systemDark`) or not. */
export function resolveTheme(choice, systemDark) {
  if (choice === 'dark' || choice === 'light') return choice;
  return systemDark ? 'dark' : 'light';
}

/** 'light' | 'dark': the bar's colours for `barChoice` in an app theme. */
export function resolveBarTheme(choice, appTheme) {
  return choice === 'dark' || choice === 'light' ? choice : appTheme;
}

function systemDark() {
  if (osDark) return osDark.matches;
  if (typeof window === 'undefined') return false;
  return !!window.matchMedia?.('(prefers-color-scheme: dark)').matches;
}

function applyTheme() {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  const theme = resolveTheme(themeChoice, systemDark());
  const bar = resolveBarTheme(barChoice, theme);
  if (root.dataset.yapTheme !== theme) root.dataset.yapTheme = theme;
  if (root.dataset.barTheme !== bar) root.dataset.barTheme = bar;
  store(THEME_KEY, themeChoice);
  store(BAR_THEME_KEY, barChoice);
}

/** The bar's colours as Rust's bar status says them ('app' | 'light' |
 *  'dark'; Overlay.svelte), the same value as the config's `barTheme`. */
export function setBarChoice(choice) {
  barChoice = BAR_THEME_IDS.has(choice) ? choice : 'app';
  applyTheme();
}

/** The reading fonts, in the order Settings lists them. `family` is what the
 *  option is drawn in (each shows its name in its own font). */
export const READING_FONTS = [
  { value: 'default', label: 'Default', family: "'Figtree Variable', 'Segoe UI Variable Text', 'Segoe UI', sans-serif" },
  { value: 'sans', label: 'Sans', family: "'Geist Variable', 'Segoe UI Variable Text', 'Segoe UI', sans-serif" },
  { value: 'serif', label: 'Serif', family: "'Literata Variable', Georgia, serif" },
  { value: 'system', label: 'System', family: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif" },
  { value: 'dyslexic', label: 'OpenDyslexic', family: "'OpenDyslexic', 'Segoe UI', sans-serif" },
];

/** What each font is, under the picker. */
export const READING_FONT_DESC = {
  default: 'Figtree, the same as the rest of Yap',
  sans: 'Geist: a clean, even sans',
  serif: 'Literata: a book serif made for reading on screens',
  system: 'Segoe UI, Windows’ own',
  dyslexic: 'Weighted letters that are harder to flip or swap',
};

export const READING_SIZES = [
  { value: 'small', label: 'Small' },
  { value: 'default', label: 'Default' },
  { value: 'large', label: 'Large' },
  { value: 'larger', label: 'Larger' },
];

export const MOTION_OPTIONS = [
  { value: 'system', label: 'System' },
  { value: 'reduced', label: 'Reduced' },
];

const FONT_IDS = new Set(READING_FONTS.map((f) => f.value));
const SIZE_IDS = new Set(READING_SIZES.map((s) => s.value));

let lastCfg = null;
let osReduced = null; // MediaQueryList, once initAppearance ran

/** The choices as <html> attributes, unknown values as the default. */
export function appearanceOf(cfg) {
  const font = FONT_IDS.has(cfg?.readingFont) ? cfg.readingFont : 'default';
  const size = SIZE_IDS.has(cfg?.readingSize) ? cfg.readingSize : 'default';
  const motion = cfg?.motion === 'reduced' ? 'reduced' : 'system';
  return { font, size, motion };
}

/** Is motion reduced in this window right now (the setting, or Windows'
 *  "Animation effects" off while the setting follows the system)? */
export function motionReduced() {
  if (typeof document === 'undefined') return false;
  return document.documentElement.dataset.motion === 'reduced';
}

/** A Svelte transition's duration: 0 while motion is reduced. Read when the
 *  transition starts (Svelte evaluates transition params then). */
export function motionMs(ms) {
  return motionReduced() ? 0 : ms;
}

/** Mark this window's <html> for the config `cfg` (Settings' copy, or the
 *  saved config). Idempotent; cheap enough for a Svelte $effect. A config
 *  without `theme` / `barTheme` (null before the first read) leaves the
 *  theme as it is. */
export function applyAppearance(cfg) {
  if (typeof document === 'undefined') return;
  lastCfg = cfg;
  const { font, size, motion } = appearanceOf(cfg);
  const root = document.documentElement;
  if (root.dataset.readingFont !== font) root.dataset.readingFont = font;
  if (root.dataset.readingSize !== size) root.dataset.readingSize = size;
  const reduced = motion === 'reduced' || !!osReduced?.matches;
  if (reduced) root.dataset.motion = 'reduced';
  else delete root.dataset.motion;
  if (cfg && 'theme' in cfg) themeChoice = THEME_IDS.has(cfg.theme) ? cfg.theme : 'system';
  if (cfg && 'barTheme' in cfg) barChoice = BAR_THEME_IDS.has(cfg.barTheme) ? cfg.barTheme : 'app';
  applyTheme();
}

/**
 * Every window, once (App.svelte): apply the saved config now, again after
 * every save from any window, and when Windows' animation setting changes.
 * Returns a function that stops following.
 */
export function initAppearance() {
  if (typeof window === 'undefined') return () => {};
  osReduced = window.matchMedia?.('(prefers-reduced-motion: reduce)') ?? null;
  osDark = window.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
  const onOs = () => applyAppearance(lastCfg);
  osReduced?.addEventListener?.('change', onOs);
  // Windows switching app mode (light ↔ dark) while the theme follows it.
  osDark?.addEventListener?.('change', applyTheme);
  applyAppearance(null);
  const reload = () =>
    invoke('get_config')
      .then((cfg) => applyAppearance(cfg))
      .catch(() => {});
  reload();
  let unlisten = null;
  let stopped = false;
  listen('yap-config-saved', reload)
    .then((u) => {
      if (stopped) u();
      else unlisten = u;
    })
    .catch(() => {});
  return () => {
    stopped = true;
    osReduced?.removeEventListener?.('change', onOs);
    osDark?.removeEventListener?.('change', applyTheme);
    unlisten?.();
  };
}
