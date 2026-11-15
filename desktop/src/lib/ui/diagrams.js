// **Diagrams, drawn** (15 Nov 2026, after the Claude app's): mermaid (MIT) in
// chunks of its own, loaded by ui/markdownVisuals.js the first time an
// answer, a note or a document shows a ```mermaid fence. Never in the main
// chunk (mermaid is big; its diagram kinds load only when used).
//
// - **Safe**: mermaid's strict mode (no click handlers, no scripts), its
//   labels' markup cut to text-like tags (`dompurifyConfig`), the theme and
//   CSS never set by the diagram itself (`secure`); a source that could
//   reach an address isn't drawn at all, and the SVG made is sanitised again
//   before it's shown (core/visualSafety.js). The view puts it in a shadow
//   root, so its sheet styles nothing else on the page.
// - **Yap's colours**: mermaid's base theme from the `--yap-*` tokens
//   (`themeVariables`), light and dark: each diagram is drawn in the theme
//   shown, then in the other one too, so a change of theme swaps it at once
//   (the theme not shown is read from app.css's own rules).
// - One at a time (mermaid measures text on the page while it draws), each
//   kept by its source and theme: an answer rendered again as it's written
//   draws a diagram once.
import mermaid from 'mermaid';
import { DIAGRAM_MAX, cleanDiagram, diagramRefused } from '../core/visualSafety.js';

/** 'light' | 'dark': the theme the page shows now. */
export const themeNow = () => (globalThis.document.documentElement.dataset.yapTheme === 'dark' ? 'dark' : 'light');

/** app.css's `--yap-*` tokens for a theme, as its rules set them (`:root`, then the dark block over it). */
const sheetTokens = new Map();
function tokensFromSheets(theme) {
  if (sheetTokens.has(theme)) return sheetTokens.get(theme);
  const light = new Map();
  const dark = new Map();
  const read = (rule, into) => {
    for (const name of rule.style) if (name.startsWith('--yap-')) into.set(name, rule.style.getPropertyValue(name).trim());
  };
  const walk = (rules) => {
    for (const rule of rules) {
      if (rule.cssRules && !rule.selectorText) {
        walk(rule.cssRules);
        continue;
      }
      const sel = (rule.selectorText || '').split(',').map((s) => s.trim());
      if (sel.includes(':root') || sel.includes(':root[data-yap-theme="light"]')) read(rule, light);
      if (sel.includes(':root[data-yap-theme="dark"]')) read(rule, dark);
    }
  };
  for (const sheet of globalThis.document.styleSheets) {
    try {
      walk(sheet.cssRules);
    } catch {
      /* another origin's sheet */
    }
  }
  const both = { light, dark: new Map([...light, ...dark]) };
  // (Kept once the sheet is there: before, read again next time.)
  if (light.size) for (const [k, v] of Object.entries(both)) sheetTokens.set(k, v);
  return both[theme];
}

/**
 * A theme's token reader: app.css's rules (a `var()` followed), the same
 * whichever theme shows, so a drawing made ahead is found when its theme
 * comes; the page's computed value where the rules have none.
 */
function tokensOf(theme) {
  const map = tokensFromSheets(theme);
  const shown = theme === themeNow() ? globalThis.getComputedStyle(globalThis.document.documentElement) : null;
  const get = (name, depth = 0) => {
    const v = map.get(name) || '';
    const ref = /^var\((--[\w-]+)\)$/.exec(v);
    if (ref && depth < 5) return get(ref[1], depth + 1);
    return v || shown?.getPropertyValue(name).trim() || '';
  };
  return get;
}

/** Mermaid's base theme in Yap's colours, for `theme`. */
function themeVariables(theme) {
  const dark = theme === 'dark';
  const t = tokensOf(theme);
  const pick = (name, fallback) => t(name) || fallback;
  const fg = pick('--yap-fg', dark ? '#ece8df' : '#23211b');
  const muted = pick('--yap-muted', dark ? '#a39e92' : '#6e6a5f');
  const node = pick(dark ? '--yap-s3' : '--yap-s2', dark ? '#2c2a25' : '#ffffff');
  const line = pick('--yap-border-hover', dark ? '#4a463d' : '#c8c2b3');
  const soft = pick('--yap-s1', dark ? '#1f1d19' : '#faf9f6');
  const accent = pick('--yap-primary', '#c2690a');
  const body = globalThis.getComputedStyle(globalThis.document.body).fontFamily || 'Segoe UI, sans-serif';
  return {
    darkMode: dark,
    background: soft,
    fontFamily: body,
    fontSize: '13px',
    primaryColor: node,
    primaryTextColor: fg,
    primaryBorderColor: muted,
    secondaryColor: soft,
    secondaryTextColor: fg,
    secondaryBorderColor: line,
    tertiaryColor: soft,
    tertiaryTextColor: fg,
    tertiaryBorderColor: line,
    lineColor: muted,
    textColor: fg,
    mainBkg: node,
    nodeBorder: muted,
    clusterBkg: soft,
    clusterBorder: line,
    edgeLabelBackground: soft,
    titleColor: fg,
    noteBkgColor: soft,
    noteTextColor: fg,
    noteBorderColor: accent,
    actorBkg: node,
    actorBorder: muted,
    actorTextColor: fg,
    signalColor: fg,
    signalTextColor: fg,
    labelTextColor: fg,
  };
}

/** The config keys a diagram's own `%%{init}%%` may never change. */
const SECURE = [
  'secure', 'securityLevel', 'startOnLoad', 'maxTextSize', 'maxEdges', 'suppressErrorRendering', 'dompurifyConfig',
  'theme', 'themeVariables', 'themeCSS', 'darkMode', 'fontFamily', 'altFontFamily', 'look', 'htmlLabels', 'arrowMarkerAbsolute',
];

/** What a label may keep of its own markup: line breaks and text styles, nothing that loads. */
const LABELS = {
  ALLOWED_TAGS: ['b', 'strong', 'i', 'em', 'u', 's', 'sub', 'sup', 'br', 'span', 'p', 'code', 'small'],
  ALLOWED_ATTR: ['class'],
};

let setFor = null;
/** Mermaid set up for these theme variables. */
function setUp(vars, key) {
  if (key === setFor) return;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    secure: SECURE,
    dompurifyConfig: LABELS,
    suppressErrorRendering: true,
    maxTextSize: DIAGRAM_MAX,
    maxEdges: 400,
    theme: 'base',
    darkMode: vars.darkMode,
    themeVariables: vars,
    fontFamily: vars.fontFamily,
    // (Tighter than mermaid's own spacing: an answer's column is narrow.)
    flowchart: { htmlLabels: true, useMaxWidth: true, nodeSpacing: 30, rankSpacing: 36, padding: 10 },
  });
  setFor = key;
}

/** Diagrams being drawn, by theme and source (a Promise each), and those drawn: `{ svg }` (sanitised markup) or `{ error }`. */
const drawing = new Map();
const drawn = new Map();
let queue = Promise.resolve();
let n = 0;

/** A theme's variables and the key its drawings are kept by. */
function themed(theme) {
  const vars = themeVariables(theme);
  return { vars, key: JSON.stringify(vars) };
}

/**
 * A diagram drawn for `theme` (the one shown, by default), without waiting:
 * `{ svg }` (safe markup for a shadow root), `{ error }` (its code shows
 * instead), or `undefined`: not drawn yet (`drawDiagram`).
 */
export function drawnNow(source, theme = themeNow()) {
  const refused = diagramRefused(source);
  return refused ? { error: refused } : drawn.get(`${themed(theme).key}\n${source}`);
}

/**
 * A diagram's source drawn for `theme` (`drawnNow`'s answer, when it's
 * ready); each once per theme. Drawn for the theme shown, it's then drawn
 * for the other one too, in the background.
 */
export function drawDiagram(source, theme = themeNow()) {
  const now = drawnNow(source, theme);
  if (now) return Promise.resolve(now);
  const { vars, key: themeKey } = themed(theme);
  const key = `${themeKey}\n${source}`;
  if (drawing.has(key)) return drawing.get(key);
  const id = `yap-diagram-${++n}`;
  const job = queue.then(async () => {
    setUp(vars, themeKey);
    try {
      const { svg } = await mermaid.render(id, source);
      const clean = cleanDiagram(svg);
      if (!clean) return { error: 'not safe to show' };
      const box = globalThis.document.createElement('div');
      box.append(clean);
      return { svg: box.innerHTML };
    } catch {
      return { error: 'not a diagram mermaid can draw' };
    } finally {
      // (What mermaid left on the page while it measured, if it failed.)
      for (const left of globalThis.document.querySelectorAll(`#d${id}, #i${id}`)) left.remove();
    }
  });
  queue = job.then(
    () => {},
    () => {}
  );
  const done = job.then((result) => {
    drawing.delete(key);
    if (drawn.size > 200) drawn.clear();
    drawn.set(key, result);
    // The other theme too, so switching shows it at once.
    if (result.svg && theme === themeNow()) drawDiagram(source, theme === 'dark' ? 'light' : 'dark').catch(() => {});
    return result;
  });
  drawing.set(key, done);
  return done;
}
