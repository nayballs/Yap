// **What an answer's drawings may hold** (15 Nov 2026): the maths KaTeX
// draws and the diagrams mermaid draws (ui/markdownVisuals.js) go through
// here before they reach the page, so a model's markdown can't make Yap
// fetch an address, run a script or restyle the window — however the
// drawing library itself behaves.
//
// - **A diagram's source** (`diagramRefused`): one that could reach the web
//   isn't drawn at all, its code shows instead — a CSS `url(` or `@import`,
//   a CSS escape in a style line (`\75rl(` is `url(`), markup that loads
//   something (`<img>`, `<a>`, `<iframe>`…), a picture or icon shape
//   (`@{ img: … }`).
// - **A drawing** (`cleanDiagram`, `cleanMaths`): DOMPurify with only SVG,
//   MathML and the HTML a label needs; no links, pictures, frames or forms;
//   an `href` only to a part of the drawing itself (`#…`); a `url(…)` only
//   to one too (an arrow's marker); a `style` attribute or sheet with any
//   other address, `@import` or an escape is dropped — a diagram whose
//   sheet has one isn't drawn.
//
// DOMPurify needs a DOM (every Yap window has one; the tests bring jsdom).
import DOMPurify from 'dompurify';

/** The longest diagram source drawn (a longer one stays code). */
export const DIAGRAM_MAX = 20_000;

/** Lines of a diagram that are CSS: `style`, `classDef`, `linkStyle`, `cssClass`, an `%%{init}%%`. */
const STYLE_LINE = /^\s*(style|classDef|linkStyle|cssClass)\b|%%\{/i;

/**
 * Why a diagram's source isn't drawn (its code shows instead), or `null`:
 * it's drawn. Pure.
 */
export function diagramRefused(source) {
  const s = String(source ?? '');
  if (!s.trim()) return 'empty';
  if (s.length > DIAGRAM_MAX) return 'too long';
  if (/url\s*\(|@import|image-set\s*\(|expression\s*\(/i.test(s)) return 'an address in its styles';
  if (s.split('\n').some((l) => STYLE_LINE.test(l) && l.includes('\\'))) return 'an escape in its styles';
  if (/<\s*\/?\s*(img|image|a|iframe|frame|object|embed|video|audio|source|track|link|meta|base|use|svg|foreignobject|script|style|form|input)\b/i.test(s)) return 'markup';
  if (/@\{[^}]*\b(img|icon|image)\s*:/i.test(s)) return 'a picture shape';
  return null;
}

// Elements a drawing never keeps (their content goes too).
const FORBID_TAGS = ['a', 'image', 'img', 'picture', 'iframe', 'frame', 'object', 'embed', 'video', 'audio', 'source', 'track', 'link', 'meta', 'base', 'script', 'form', 'input', 'button', 'textarea', 'select', 'option', 'animate', 'animateMotion', 'animateTransform', 'set', 'feImage'];
const FORBID_ATTR = ['src', 'srcset', 'action', 'formaction', 'background', 'poster', 'ping', 'xlink:actuate', 'xlink:show', 'target'];

/** A `url(…)` in CSS or an attribute that isn't to a part of the drawing itself. */
const FOREIGN_URL = /url\s*\(\s*(?!['"]?#)/i;
/** CSS that loads or runs something, or hides what it says (an escape). */
const UNSAFE_CSS = /@import|expression\s*\(|image-set\s*\(|\\/i;

const isUnsafeCss = (css) => FOREIGN_URL.test(css) || UNSAFE_CSS.test(css);

const purifiers = new WeakMap();
/** One DOMPurify for drawings per window, its hooks set once. */
function purifier() {
  const win = globalThis.window;
  let p = purifiers.get(win);
  if (p) return p;
  p = DOMPurify(win);
  if (!p.isSupported) throw new Error('a drawing needs a DOM to sanitise it');
  p.addHook('uponSanitizeAttribute', (node, data) => {
    const name = data.attrName.toLowerCase();
    const value = String(data.attrValue ?? '');
    if ((name === 'href' || name === 'xlink:href') && !value.trim().startsWith('#')) data.keepAttr = false;
    else if (name === 'style' ? isUnsafeCss(value) : FOREIGN_URL.test(value)) data.keepAttr = false;
    else if (name.startsWith('on')) data.keepAttr = false;
  });
  purifiers.set(win, p);
  return p;
}

/**
 * A diagram's SVG (mermaid's) as a safe fragment to put on the page, or
 * `null` when it can't be made safe (its sheet loads something).
 */
export function cleanDiagram(svg) {
  const p = purifier();
  const frag = p.sanitize(String(svg ?? ''), {
    USE_PROFILES: { svg: true, svgFilters: true, html: true },
    ADD_TAGS: ['foreignObject', 'style'],
    ADD_ATTR: ['dominant-baseline', 'marker-end', 'marker-start', 'text-anchor', 'transform-origin'],
    HTML_INTEGRATION_POINTS: { foreignobject: true },
    FORBID_TAGS,
    FORBID_ATTR,
    ALLOW_DATA_ATTR: false,
    RETURN_DOM_FRAGMENT: true,
  });
  // (A sheet's text is never changed by the sanitiser: one that loads something refuses the whole drawing.)
  if ([...frag.querySelectorAll('style')].some((s) => isUnsafeCss(s.textContent || ''))) return null;
  if (!frag.querySelector('svg')) return null;
  return frag;
}

/** KaTeX's HTML for a formula, safe to put on the page (a string). */
export function cleanMaths(html) {
  return purifier().sanitize(String(html ?? ''), {
    USE_PROFILES: { html: true, svg: true, mathMl: true },
    ADD_TAGS: ['semantics', 'annotation'],
    ADD_ATTR: ['encoding'],
    FORBID_TAGS: [...FORBID_TAGS, 'style'],
    FORBID_ATTR,
    ALLOW_DATA_ATTR: false,
  });
}
