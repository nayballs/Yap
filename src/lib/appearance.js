// Settings → General → Appearance (config `readingFont`, `readingSize`,
// `motion`), applied to EVERY Yap window: the main window, onboarding, the
// meeting notepad, the Yap bar and the split preview all call
// `initAppearance()` (App.svelte). It marks <html> and app.css does the rest:
//
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
// WebView2 reports as `prefers-reduced-motion`, as it changes.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

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
 *  saved config). Idempotent; cheap enough for a Svelte $effect. */
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
}

/**
 * Every window, once (App.svelte): apply the saved config now, again after
 * every save from any window, and when Windows' animation setting changes.
 * Returns a function that stops following.
 */
export function initAppearance() {
  if (typeof window === 'undefined') return () => {};
  osReduced = window.matchMedia?.('(prefers-reduced-motion: reduce)') ?? null;
  const onOs = () => applyAppearance(lastCfg);
  osReduced?.addEventListener?.('change', onOs);
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
    unlisten?.();
  };
}
