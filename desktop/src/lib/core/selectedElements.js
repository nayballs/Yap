// **Elements picked on a page, in a sent message** (chat/pages/pick.rs
// `block`): the `<selected-element …>…</selected-element>` blocks Rust puts
// after the person's words for the model, split back out so the bubble shows
// their words and a chip each — never the raw block. Pure.

const BLOCK = /<selected-element\b([^>]*)>([\s\S]*?)<\/selected-element>/g;

function unescape(s) {
  return String(s ?? '').replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&amp;/g, '&');
}

function attrsOf(s) {
  const out = {};
  for (const m of String(s).matchAll(/([a-z-]+)="([^"]*)"/gi)) out[m[1].toLowerCase()] = unescape(m[2]);
  return out;
}

function part(inner, name) {
  const m = inner.match(new RegExp(`<${name}>([\\s\\S]*?)</${name}>`));
  return m ? m[1] : '';
}

/** "color: red; display: block" → { color: 'red', display: 'block' }. */
function stylesOf(s) {
  const out = {};
  for (const bit of String(s).split(';')) {
    const at = bit.indexOf(':');
    if (at > 0) out[bit.slice(0, at).trim()] = bit.slice(at + 1).trim();
  }
  return out;
}

/**
 * A message's text → its own words, and the elements picked with it (in
 * order): `{tag, id, classes, url, picture, text, path, styles, html, siblings}`.
 */
export function splitElements(text) {
  const t = String(text ?? '');
  if (!t.includes('<selected-element')) return { text: t, elements: [] };
  const elements = [];
  for (const m of t.matchAll(BLOCK)) {
    const a = attrsOf(m[1]);
    const inner = m[2];
    elements.push({
      tag: a.tag || 'element',
      id: a.id || '',
      classes: (a.class || '').split(/\s+/).filter(Boolean),
      url: a.page || '',
      picture: a.picture === 'attached',
      text: part(inner, 'text'),
      path: part(inner, 'path'),
      styles: stylesOf(part(inner, 'styles')),
      html: part(inner, 'html'),
      siblings: part(inner, 'siblings'),
    });
  }
  return { text: t.replace(BLOCK, '').trim(), elements };
}

/** How a chip names one: `<div class="order open">`, `<a id="top">`. */
export function elementLabel(el) {
  const tag = el?.tag || 'element';
  if (el?.id) return `<${tag} id="${el.id}">`;
  if (el?.classes?.length) return `<${tag} class="${el.classes.join(' ')}">`;
  return `<${tag}>`;
}

/** Its card's "Element" line: `<div class="order open" />`. */
export function elementLine(el) {
  return elementLabel(el).replace(/>$/, ' />');
}

/** Its card's details past the first: where it is, its page, its styles, its HTML (those it has). */
export function elementDetails(el) {
  const out = [];
  if (el?.path) out.push({ name: 'Path', value: el.path });
  if (el?.url) out.push({ name: 'Page', value: el.url });
  const styles = Object.entries(el?.styles || {}).filter(([, v]) => String(v).trim());
  if (styles.length) out.push({ name: 'Styles', value: styles.map(([k, v]) => `${k}: ${v}`).join('\n') });
  if (el?.html) out.push({ name: 'HTML', value: el.html, code: true });
  if (el?.siblings) out.push({ name: 'Next to it', value: el.siblings, code: true });
  return out;
}
