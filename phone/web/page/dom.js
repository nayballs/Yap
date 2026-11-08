// Small DOM builders for the page's Code side (code.js, cards.js, agent.js,
// diff.js): elements made from data with textContent and attributes, never
// HTML, so nothing an agent or a web page wrote can run here.

export const NS = 'http://www.w3.org/2000/svg';

/**
 * An element: `h('button', {class: 'x', type: 'button', testid: 'go', on: {click}}, 'Go')`.
 * `text` sets its words; `testid` its data-testid; `data` more data-*;
 * `on` its listeners; anything else an attribute (false/null left out,
 * true as ""). Children: nodes, strings, or arrays of them (null skipped).
 */
export function h(tag, props = {}, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k === 'text') e.textContent = v;
    else if (k === 'testid') e.dataset.testid = v;
    else if (k === 'data') for (const [dk, dv] of Object.entries(v)) dv != null && (e.dataset[dk] = String(dv));
    else if (k === 'on') for (const [ev, fn] of Object.entries(v)) e.addEventListener(ev, fn);
    else if (k === 'hidden') e.hidden = !!v;
    else if (k === 'disabled') e.disabled = !!v;
    else e.setAttribute(k, v === true ? '' : String(v));
  }
  add(e, kids);
  return e;
}

function add(e, kids) {
  for (const k of kids) {
    if (k == null || k === false) continue;
    if (Array.isArray(k)) add(e, k);
    else e.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
}

/** `root`'s children replaced by `kids` (nodes, strings, arrays; null and false left out). */
export function fill(root, ...kids) {
  root.replaceChildren(...kids.flat(Infinity).filter((k) => k != null && k !== false));
  return root;
}

/** A small line icon (24 × 24, drawn in strokes), hidden from a screen reader. */
export function svg(paths, cls = '') {
  const s = document.createElementNS(NS, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('aria-hidden', 'true');
  s.setAttribute('focusable', 'false');
  if (cls) s.setAttribute('class', cls);
  for (const d of paths) {
    const p = document.createElementNS(NS, 'path');
    p.setAttribute('d', d);
    s.appendChild(p);
  }
  return s;
}

/** Words for a screen reader only. */
export const vh = (text) => h('span', { class: 'vh', text });

/** The icons the Code side draws. */
export const ICONS = {
  folder: ['M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z'],
  shield: ['M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z', 'M12 8v5M12 16.5v.01'],
  plan: ['M9 4h6a1 1 0 0 1 1 1v1H8V5a1 1 0 0 1 1-1z', 'M16 5h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h2', 'M9 12h6M9 16h4'],
  chevron: ['m9 6 6 6-6 6'],
  down: ['M7 10l5 5 5-5'],
  close: ['M6.5 6.5l11 11M17.5 6.5l-11 11'],
  plus: ['M12 5v14M5 12h14'],
  phone: ['M9 2.5h6A2.2 2.2 0 0 1 17.2 4.7v14.6a2.2 2.2 0 0 1-2.2 2.2H9a2.2 2.2 0 0 1-2.2-2.2V4.7A2.2 2.2 0 0 1 9 2.5z', 'M11 18h2'],
  arrowDown: ['M12 5v14M6 13l6 6 6-6'],
  file: ['M7 3h7l5 5v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z', 'M14 3v5h5'],
  terminal: ['M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z', 'M7 10l3 2-3 2M12 15h5'],
};
