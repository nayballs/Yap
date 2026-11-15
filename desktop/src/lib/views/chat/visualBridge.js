// **The page's side of a visual's frame** (chat/made.rs, 15 Nov 2026; after the Claude app's inline
// visuals): every frame registers here, and one `message` listener answers them all — by the frame the
// message came from (`event.source`), never by anything the message says about itself.
//
// - `ready` → Yap's colours, as the theme has them now (`themeVars`), and again whenever the theme changes.
// - `size {h}` → the frame's height (`onsize`), clamped.
// - `prompt {text}` → `onprompt(text)`: text only, trimmed, at most 4,000 characters, and only while the
//   person's last click was in that frame (it has the focus: `document.activeElement`), one a second and a
//   half at most — a visual can't send messages by itself.
// - `link {href}` → the person's browser (http and https only), on their click the same way.
// Anything else is ignored. The frame is sandboxed (`allow-scripts` only, and its own policy says so): an
// opaque origin with none of Yap's commands, which can reach Yap only through these few messages.
import { convertFileSrc } from '@tauri-apps/api/core';
import { openExternalLink } from '../../core/externalLinks.js';

/** The longest message a visual's button may send. */
export const PROMPT_MAX = 4000;
/** How soon a frame may send again (ms). */
export const PROMPT_GAP_MS = 1500;
/** A frame's height, at least and at most (CSS px). */
export const MIN_H = 24;
export const MAX_H = 2400;

/**
 * The colours a visual gets, as Yap's own tokens name them now: what `--text`, `--surface`… are in the
 * light or dark theme, and a series palette (`--c1`…`--c6`, each with a `-soft` fill) that reads on both.
 */
const FROM_TOKENS = {
  '--text': '--yap-fg',
  '--text-2': '--yap-fg-62',
  '--muted': '--yap-muted',
  '--bg': '--yap-s1',
  '--surface': '--yap-s2',
  '--surface-2': '--yap-s3',
  '--border': '--yap-border',
  '--accent': '--yap-primary',
  '--accent-text': '--yap-primary-fg',
  '--accent-soft': '--yap-primary-wash',
  '--good': '--yap-success',
  '--warn': '--yap-warning',
  '--bad': '--yap-danger',
};
const SERIES = {
  light: ['#2f6fd0', '#1f8a7a', '#c2690a', '#7b55c7', '#c23b32', '#4f7a1f'],
  dark: ['#7aa7f0', '#4cc3ad', '#e2982a', '#ad8cf0', '#ec7a73', '#93c45a'],
};

/** The theme now: 'dark' or 'light'. */
export function themeNow() {
  return document.documentElement.dataset.yapTheme === 'dark' ? 'dark' : 'light';
}

/** Yap's colours for a visual, as `{ '--text': '#23211b', … }`. */
export function themeVars() {
  const css = getComputedStyle(document.documentElement);
  const out = {};
  for (const [name, token] of Object.entries(FROM_TOKENS)) {
    const v = css.getPropertyValue(token).trim();
    if (v) out[name] = v;
  }
  const scheme = themeNow();
  SERIES[scheme].forEach((c, i) => {
    out[`--c${i + 1}`] = c;
    out[`--c${i + 1}-soft`] = `color-mix(in srgb, ${c} ${scheme === 'dark' ? 22 : 16}%, transparent)`;
  });
  const font = getComputedStyle(document.body).fontFamily;
  if (font) out['--font'] = font;
  return out;
}

/** frame element → { onsize, onprompt, last } */
const frames = new Map();
let listening = false;
let themeWatch = null;

function sendTheme(el) {
  try {
    el.contentWindow?.postMessage({ yapVisual: 1, op: 'theme', scheme: themeNow(), vars: themeVars() }, '*');
  } catch {
    /* gone */
  }
}

function onMessage(e) {
  let el = null;
  for (const f of frames.keys()) {
    if (f.contentWindow && f.contentWindow === e.source) {
      el = f;
      break;
    }
  }
  if (!el) return;
  const d = e.data;
  if (!d || typeof d !== 'object' || d.yapVisual !== 1 || typeof d.op !== 'string') return;
  const f = frames.get(el);
  switch (d.op) {
    case 'ready':
      sendTheme(el);
      break;
    case 'size': {
      const h = Number(d.h);
      if (Number.isFinite(h)) f.onsize?.(Math.max(MIN_H, Math.min(MAX_H, Math.ceil(h))));
      break;
    }
    case 'prompt': {
      if (typeof d.text !== 'string') return;
      const text = d.text.slice(0, PROMPT_MAX).trim();
      // The person's click, in this frame: it has the focus.
      if (!text || document.activeElement !== el) return;
      const now = Date.now();
      if (now - f.last < PROMPT_GAP_MS) return;
      f.last = now;
      f.onprompt?.(text);
      break;
    }
    case 'link': {
      if (typeof d.href !== 'string' || document.activeElement !== el) return;
      let url;
      try {
        url = new URL(d.href);
      } catch {
        return;
      }
      if (url.protocol === 'https:' || url.protocol === 'http:') openExternalLink(url.href);
      break;
    }
    default:
      break;
  }
}

/**
 * **A frame registered**: `onsize(h)` its height, `onprompt(text)` a message its button sends. Returns
 * the function that lets it go.
 */
export function registerFrame(el, { onsize, onprompt }) {
  if (!listening) {
    window.addEventListener('message', onMessage);
    // The theme changes: every frame gets the new colours.
    themeWatch = new MutationObserver(() => frames.forEach((_, f) => sendTheme(f)));
    themeWatch.observe(document.documentElement, { attributes: true, attributeFilter: ['data-yap-theme'] });
    listening = true;
  }
  frames.set(el, { onsize, onprompt, last: 0 });
  return () => {
    frames.delete(el);
    if (!frames.size && listening) {
      window.removeEventListener('message', onMessage);
      themeWatch?.disconnect();
      themeWatch = null;
      listening = false;
    }
  };
}

/** A visual's frame address (the `yapmade` scheme: `http://yapmade.localhost/v/<chat>/<id>` in WebView2; chat/made/serve.rs). */
export function visualSrc(conversation, id) {
  return convertFileSrc(`v/${Number(conversation)}/${String(id).replace(/[^a-z0-9_]/g, '')}`, 'yapmade');
}
