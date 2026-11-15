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

const REACT = /<react\b([^>]*)>([\s\S]*?)<\/react>/;

/**
 * An element's React details from its block's `<react …>` part (pick.rs
 * `ReactPart`): `{component, source, build, owners, inside, props}`, or
 * null — the shape the message box's chip has from Rust, `source` already
 * as said ("… (as the page serves it)").
 */
function reactOf(inner) {
  const m = String(inner).match(REACT);
  if (!m) return null;
  const a = attrsOf(m[1]);
  const owners = part(m[2], 'owners');
  const inside = part(m[2], 'inside');
  const said = owners || inside;
  const props = part(m[2], 'props');
  return {
    component: a.component || '',
    source: a.source || '',
    sourceFrom: '',
    build: a.build || '',
    minified: a.names === 'minified',
    owners: said ? said.split(' ‹ ').filter(Boolean) : [],
    inside: !owners && !!inside,
    props: props ? props.split('\n').filter(Boolean) : [],
  };
}

/**
 * A message's text → its own words, and the elements picked with it (in
 * order): `{tag, id, classes, url, picture, text, path, styles, html, siblings, react}`.
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
      react: reactOf(inner),
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

/**
 * Its React component, as its card's first rows show it: `<BasketRow />` and
 * where its JSX was written — the box's chip has Rust's `source` and
 * `sourceFrom`, a sent one's block says it already. Null off React.
 */
export function elementReact(el) {
  const r = el?.react;
  if (!r || (!r.component && !r.source)) return null;
  const source = r.sourceFrom === 'stack' && r.source && !/\(as the page serves it\)$/.test(r.source) ? `${r.source} (as the page serves it)` : r.source || '';
  // (A production build's shortened names said as such: a component "a" is no <a> element.)
  const component = r.component ? `<${r.component} />${r.minified ? ' (a minified name)' : ''}` : '';
  return { component, source, build: r.build || '' };
}

/** Its card's details past the first: its React owners and props, where it is, its page, its styles, its HTML (those it has). */
export function elementDetails(el) {
  const out = [];
  const r = el?.react;
  if (r?.owners?.length) out.push({ name: r.inside ? 'Inside' : 'Owners', value: r.owners.join(' ‹ ') });
  if (r?.props?.length) out.push({ name: 'Props', value: r.props.join('\n'), code: true });
  if (el?.path) out.push({ name: 'Path', value: el.path });
  if (el?.url) out.push({ name: 'Page', value: el.url });
  const styles = Object.entries(el?.styles || {}).filter(([, v]) => String(v).trim());
  if (styles.length) out.push({ name: 'Styles', value: styles.map(([k, v]) => `${k}: ${v}`).join('\n') });
  if (el?.html) out.push({ name: 'HTML', value: el.html, code: true });
  if (el?.siblings) out.push({ name: 'Next to it', value: el.siblings, code: true });
  return out;
}
