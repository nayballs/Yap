// Chat's model picker on the phone: the desktop's own picker
// (src/lib/views/chat/ChatModelPicker.svelte) as a sheet from the bottom,
// with the same structure and words — the provider rail with its logos
// (Recent, Groq, Gemini, Claude, ChatGPT, OpenRouter, Custom, On this PC),
// each provider's **Latest** first and **More models (N)** opening in place,
// OpenAI's two ways as one "ChatGPT" entry ("Your plan | API key"), the
// on-device models, "Reads images" and the other tags, ✓ on the model in
// use, and the effort under it.
//
// The rows are built by the desktop's own code: core/chatPicker.js and the
// pure modules it imports (src/lib/core/, served by the PC at core/ as they
// are), from what the PC sends in `ready.picker` — the very data the
// desktop picker reads (link/models.rs `picker`). Picking sends `set_model`
// with the row's provider, model, server or file, which the PC switches as
// the desktop picker does. A PC from before the picker (no `ready.picker`)
// gets its plain list.
//
// What the phone leaves to the PC: adding a key or signing in, downloading
// or uninstalling an on-device model, asking a self-hosted server for its
// list, and search. The sheet says where.
//
// Everything is built as DOM nodes (textContent), never HTML; the logos are
// the PC's static files (providers/<id>.svg).
import {
  cloudInfo,
  isCloud,
  gptState,
  railTabs,
  startTab,
  cloudRows,
  localRows,
  serverRows,
  providerLine,
  whereLocal,
  slowWarning,
  recentRows,
  recentEntry,
  sameEntry,
} from './core/chatPicker.js';
import { ICON_FILES, MONOCHROME_PROVIDERS, ICON_TINTS, familyIcon, providerIcon } from './core/modelIcons.js';
import { labelFor } from './core/modelCatalog.js';
import { CHATGPT, GPT_TAB, GPT_WAYS, PROVIDER_LABELS } from './core/providerIds.js';
import { effortNow, effortOptions, effortHint, effortLabel, FAST_NOTE } from './core/effort.js';

/** The marks the PC serves (src-tauri/build.rs LINK_SHARED). */
const SERVED = new Set(['openai', 'openai-oss', 'claude', 'gemini', 'gemma', 'groq', 'llama', 'mistral', 'openrouter', 'qwen', 'xai']);
const RECENT_KEY = 'yap.link.recent';
const NS = 'http://www.w3.org/2000/svg';

// ------------------------------------------------------------------ nodes

function h(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

/** An inline icon from path data, in currentColor (never HTML). */
function svg(paths, attrs = {}) {
  const s = document.createElementNS(NS, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('aria-hidden', 'true');
  s.setAttribute('focusable', 'false');
  for (const [k, v] of Object.entries(attrs)) s.setAttribute(k, v);
  for (const p of paths) {
    const [tag, a] = typeof p === 'string' ? ['path', { d: p }] : p;
    const e = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(a)) e.setAttribute(k, v);
    s.appendChild(e);
  }
  return s;
}

/** Yap's own glyphs (views/chat/ProviderMark.svelte). */
const GLYPHS = {
  pc: [['rect', { x: 3, y: 4, width: 18, height: 12, rx: 2 }], 'M8 20h8M12 16v4'],
  recent: [['circle', { cx: 12, cy: 12, r: 8.5 }], 'M12 7.5V12l3 2'],
  server: [['rect', { x: 3, y: 4, width: 18, height: 7, rx: 1.5 }], ['rect', { x: 3, y: 13, width: 18, height: 7, rx: 1.5 }], 'M7 7.5h.01M7 16.5h.01'],
  chip: [['rect', { x: 5, y: 5, width: 14, height: 14, rx: 2 }], ['rect', { x: 9.5, y: 9.5, width: 5, height: 5 }], 'M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3'],
  spark: ['M12 3l1.8 4.9L19 9.7l-4.2 3.1L16.4 18 12 15l-4.4 3 1.6-5.2L5 9.7l5.2-1.8z'],
};

/**
 * A provider's or a model family's mark (modelIcons.js): the logo as it
 * is, a monochrome one as a mask in the text's colour, Claude's in its own
 * colour; else one of Yap's glyphs.
 */
export function markEl(icon, glyph = 'spark', size = 18, tile = false) {
  const span = h('span', `pmark${tile ? ' tile' : ''}`);
  span.setAttribute('aria-hidden', 'true');
  span.style.setProperty('--s', `${size}px`);
  span.dataset.icon = icon || glyph;
  const file = icon ? ICON_FILES[icon] : null;
  if (file && SERVED.has(file)) {
    const url = `providers/${file}.svg`;
    const tint = ICON_TINTS[icon];
    if (MONOCHROME_PROVIDERS.has(icon) || tint) {
      const m = h('span', 'mask');
      m.style.setProperty('-webkit-mask-image', `url("${url}")`);
      m.style.setProperty('mask-image', `url("${url}")`);
      if (tint) m.style.backgroundColor = tint;
      span.appendChild(m);
    } else {
      const img = new Image();
      img.alt = '';
      img.decoding = 'async';
      img.src = url;
      span.appendChild(img);
    }
  } else {
    span.appendChild(svg(GLYPHS[glyph] || GLYPHS.spark));
  }
  return span;
}

/** The chip's mark for Chat's model: its provider's, else its family's. */
export function modelMark(model, picker) {
  const m = picker?.status?.model;
  if (m) return { icon: providerIcon(m.provider) ?? familyIcon(m.name, m.id, model?.label), glyph: m.provider === 'ondevice' ? 'chip' : 'spark' };
  const provider = String(model?.id || '').split('|')[0];
  return { icon: providerIcon(provider) ?? familyIcon(model?.label), glyph: provider === 'ondevice' ? 'chip' : 'spark' };
}

/**
 * The chip's words, as Chat's chip says them (core/aiModel.js `modelName`,
 * `providerName`): the model's name — the on-device model's own, Yap's
 * curated label, the provider's own name for it — and where it runs ("Groq",
 * "On this PC"). From the picker's status when the PC sends it, else from
 * the PC's label ("GPT OSS 20B · Groq").
 */
export function chipWords(model, picker) {
  const m = picker?.status?.ready ? picker.status.model : null;
  if (m) {
    const name = m.name || labelFor(m.provider, m.id, picker.catalog?.[m.provider]) || model?.label || m.id;
    const where = (m.provider === 'local' || m.provider === 'custom') && m.onThisPc ? 'On this PC' : PROVIDER_LABELS[m.provider] || m.provider;
    return { name, where };
  }
  const label = model?.label || model?.id || 'Model';
  const at = label.lastIndexOf(' · ');
  if (at > 0) return { name: label.slice(0, at), where: label.slice(at + 3) };
  return { name: label, where: model?.onThisPc ? 'On this PC' : '' };
}

/**
 * The chip's effort, as the Claude app's chip says it ("Extra" for xhigh):
 * the level Chat's model answers at, '' when it has none to choose.
 */
export function chipEffort(picker) {
  const s = picker?.status;
  const eff = s?.ready && s.model ? s.effort : null;
  return eff ? effortLabel(effortNow(eff)) : '';
}

// ----------------------------------------------------------------- recent

function readRecent() {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
    return Array.isArray(v) ? v.filter((e) => e && typeof e.provider === 'string').slice(0, 3) : [];
  } catch {
    return [];
  }
}

function pushRecent(entry) {
  if (!entry?.provider) return;
  const list = [entry, ...readRecent().filter((e) => !sameEntry(e, entry))].slice(0, 3);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* a convenience only */
  }
}

// ----------------------------------------------------------------- picker

/**
 * The sheet. `send(msg)` goes to the PC; `onPicked(model)` tells the page
 * (the chip shows the pick at once; the PC's next `ready` has the last word).
 */
export function createPicker({ dialog, send, onPicked, onChanged }) {
  let ready = null; // the last `ready`
  let sel = 'groq';
  let gptWay = CHATGPT;
  const moreChoice = {};

  const data = () => ready?.picker || null;
  /** What chatPicker.js reads (the desktop fills the same from its stores). */
  function ctx() {
    const p = data();
    const images = p?.images || {};
    return {
      status: p?.status || null,
      catalog: p?.catalog || {},
      local: p?.local || null,
      downloading: '',
      readsImages: (provider, model) => ((images[provider] || []).includes(model) ? true : null),
      servers: {},
    };
  }

  function close() {
    if (typeof dialog.close === 'function' && dialog.open) dialog.close();
    else dialog.removeAttribute('open');
  }

  function open() {
    const p = data();
    if (p?.status) {
      sel = startTab(p.status);
      gptWay = (p.status.ready ? p.status.model?.provider : p.status.provider) === 'openai' ? 'openai' : CHATGPT;
      // The one in use counts as used (Recent), as Chat's chip has it.
      const m = p.status.ready ? p.status.model : null;
      if (m) {
        const server = (p.status.servers || []).find((s) => s.provider === m.provider && s.host === p.status.host);
        pushRecent({ provider: m.provider, model: m.provider === 'ondevice' ? '' : m.id, file: m.provider === 'ondevice' ? p.local?.activeFile || '' : '', baseUrl: server?.baseUrl || '' });
      }
    }
    build();
    if (typeof dialog.showModal === 'function') {
      if (!dialog.open) dialog.showModal();
    } else dialog.setAttribute('open', '');
    // The model in use in view (build put the rail's tab in view).
    build();
    const cur = dialog.querySelector('.mrow.on');
    if (cur) cur.scrollIntoView({ block: 'nearest' });
  }

  function update(msg) {
    ready = msg;
    if (dialog.open) build();
  }

  // ---------------------------------------------------------------- build

  function build() {
    const scroll = dialog.querySelector('.pane')?.scrollTop || 0;
    const across = dialog.querySelector('.rail')?.scrollLeft || 0;
    const frag = document.createDocumentFragment();
    frag.appendChild(h('div', 'grab')).setAttribute('aria-hidden', 'true');
    const title = h('h2', null, 'Switch the Chat model');
    title.id = 'models-title';
    frag.appendChild(title);
    dialog.setAttribute('aria-labelledby', 'models-title');
    if (data()?.status) {
      frag.appendChild(rail());
      frag.appendChild(pane());
      const e = effort();
      if (e) frag.appendChild(e);
    } else {
      frag.appendChild(plainList());
    }
    frag.appendChild(foot());
    dialog.replaceChildren(frag);
    const pn = dialog.querySelector('.pane');
    if (pn) pn.scrollTop = scroll;
    // The rail where it was, the chosen provider in view.
    const rl = dialog.querySelector('.rail');
    if (rl) {
      rl.scrollLeft = across;
      const tab = rl.querySelector('.tab[aria-selected="true"]');
      if (tab) {
        const r = tab.getBoundingClientRect();
        const box = rl.getBoundingClientRect();
        if (r.left < box.left) rl.scrollLeft -= box.left - r.left + 16;
        else if (r.right > box.right) rl.scrollLeft += r.right - box.right + 16;
      }
    }
  }

  function rail() {
    const box = h('div', 'rail');
    box.setAttribute('role', 'tablist');
    box.setAttribute('aria-label', 'Providers');
    box.dataset.testid = 'picker-rail';
    for (const t of railTabs(ctx())) {
      if (t.sepBefore) box.appendChild(h('span', 'sep')).setAttribute('role', 'presentation');
      const b = h('button', 'tab');
      b.type = 'button';
      b.setAttribute('role', 'tab');
      b.dataset.tab = t.id;
      b.setAttribute('aria-selected', String(sel === t.id));
      b.appendChild(markEl(t.icon ?? null, t.glyph ?? 'spark', 18));
      b.appendChild(h('span', 'tlabel', t.label));
      if (t.state) {
        const dot = h('span', `st${t.ready ? '' : ' off'}`);
        dot.setAttribute('aria-hidden', 'true');
        b.appendChild(dot);
        // One line for a screen reader: "Groq, connected".
        b.setAttribute('aria-label', `${t.label}, ${t.state}`);
      }
      b.addEventListener('click', () => {
        sel = t.id;
        build();
        dialog.querySelector('.pane').scrollTop = 0;
      });
      box.appendChild(b);
    }
    return box;
  }

  function head(icon, glyph, title, lines) {
    const box = h('div', 'phead');
    box.appendChild(markEl(icon, glyph, 36, true));
    const text = h('div', 'ptext');
    text.appendChild(h('div', 'ptitle', title));
    for (const l of lines.filter((x) => x?.text)) {
      const s = h('div', `psub${l.warn ? ' warn' : ''}`, l.text);
      text.appendChild(s);
    }
    box.appendChild(text);
    return box;
  }

  function group(label, rows, extra = {}) {
    const g = h('div', 'group');
    g.setAttribute('role', 'group');
    g.setAttribute('aria-label', label);
    if (extra.header !== false) {
      const gh = h('div', 'gh');
      if (extra.glyph) gh.appendChild(markEl(null, extra.glyph, 14));
      gh.appendChild(document.createTextNode(label));
      if (extra.meta) gh.appendChild(h('span', 'meta', extra.meta));
      g.appendChild(gh);
    }
    for (const r of rows) g.appendChild(row(r, extra.where));
    return g;
  }

  function row(r, where = false) {
    const b = h('button', 'mrow');
    b.type = 'button';
    b.dataset.key = r.key;
    b.dataset.testid = 'picker-row';
    b.setAttribute('aria-pressed', String(!!r.current));
    if (r.current) b.classList.add('on');
    const chk = h('span', 'chk');
    chk.setAttribute('aria-hidden', 'true');
    if (r.current) chk.appendChild(svg(['m5 12 5 5 9-10']));
    b.appendChild(chk);
    b.appendChild(markEl(r.icon ?? null, r.glyph ?? 'spark', 20));
    const txt = h('span', 'txt');
    txt.appendChild(h('span', 'nm', r.name));
    const desc = where ? [r.where, r.desc].filter(Boolean).join(' · ') : r.desc;
    if (desc) txt.appendChild(h('span', 'ds', desc));
    // The tags under the words (a phone's width: the name stays whole).
    if (r.tags?.length) {
      const tags = h('span', 'tags');
      for (const t of r.tags) tags.appendChild(h('span', `tag ${t.t}`, t.text));
      txt.appendChild(tags);
    }
    b.appendChild(txt);
    if (r.action === 'key') {
      b.classList.add('locked');
      b.setAttribute('aria-disabled', 'true');
      b.addEventListener('click', () => dialog.querySelector('.connect')?.scrollIntoView({ block: 'nearest' }));
    } else if (r.action === 'pick') {
      b.addEventListener('click', () => pick(r));
    }
    return b;
  }

  /** A cloud provider's models: Latest, then More models (N) in place. */
  function models(list, box) {
    const all = [...list.latest, ...list.older];
    if (list.missing && all.some((r) => r.current && r.model === list.missing.value)) {
      box.appendChild(h('p', 'pnote warn', `${PROVIDER_LABELS[list.provider] || list.provider} no longer offers ${list.missing.label}. Pick another model.`));
    }
    if (list.split) box.appendChild(group('Latest', list.latest));
    else box.appendChild(group(cloudInfo(list.provider)?.short || list.provider, list.latest, { header: false }));
    if (list.older.length > 0) {
      const openMore = moreChoice[list.provider] ?? list.older.some((r) => r.current);
      const more = h('button', 'morerow');
      more.type = 'button';
      more.dataset.testid = 'picker-more';
      more.setAttribute('aria-expanded', String(openMore));
      more.appendChild(h('span', 'chk'));
      more.appendChild(svg(['m9 6 6 6-6 6'], { class: 'ch' }));
      const label = h('span', 'mlabel');
      label.appendChild(document.createTextNode('More models '));
      label.appendChild(h('span', 'count', `(${list.older.length})`));
      more.appendChild(label);
      more.addEventListener('click', () => {
        moreChoice[list.provider] = !openMore;
        build();
      });
      box.appendChild(more);
      if (openMore) box.appendChild(group(`More ${cloudInfo(list.provider)?.short || ''} models`, list.older, { header: false }));
      else {
        // Closed by hand: the model in use still on show, under it.
        const cur = list.older.filter((r) => r.current);
        if (cur.length) box.appendChild(group('In use', cur, { header: false }));
      }
    }
  }

  /** A provider without a key: connecting it is the PC's (keys never leave it). */
  function connectCard(p) {
    const c = h('div', 'connect');
    c.dataset.testid = 'picker-connect';
    const info = cloudInfo(p);
    if (p === CHATGPT || p === GPT_TAB) {
      c.appendChild(h('div', 'ctitle', 'Use your ChatGPT plan'));
      c.appendChild(
        h('div', 'cdesc', 'Sign in with ChatGPT in Yap on your PC (Settings → Language Models → ChatGPT), or add an OpenAI API key there. Then pick its models here.'),
      );
    } else {
      c.appendChild(h('div', 'ctitle', `Connect ${info?.short || p}`));
      c.appendChild(h('div', 'cdesc', `Add an ${info?.name || p} API key in Yap on your PC: Settings → Language Models. Keys stay on the PC; then pick its models here.`));
    }
    return c;
  }

  function pane() {
    const c = ctx();
    const status = c.status;
    const box = h('div', 'pane');
    box.setAttribute('role', 'tabpanel');
    box.dataset.testid = 'picker-pane';
    if (sel === 'recent') {
      box.appendChild(head(null, 'recent', 'Recent', [{ text: 'The models you used last in Chat' }]));
      const rows = recentRows(readRecent(), c);
      if (rows.length) box.appendChild(group('Recent', rows, { header: false, where: true }));
      else box.appendChild(h('p', 'pk-empty', 'The models you pick show up here.'));
    } else if (sel === GPT_TAB) {
      const g = gptState(status, gptWay);
      const w = g.shown;
      box.appendChild(head('openai', 'spark', 'ChatGPT', [w ? providerLine(w, c) : { text: 'Not set up' }]));
      if (g.planOn && g.keyOn) box.appendChild(ways(w));
      if (!w) box.appendChild(connectCard(GPT_TAB));
      else models({ provider: w, ...cloudRows(w, c) }, box);
    } else if (isCloud(sel)) {
      const info = cloudInfo(sel);
      const list = { provider: sel, ...cloudRows(sel, c) };
      box.appendChild(head(providerIcon(sel), 'spark', info.name, [providerLine(sel, c)]));
      if (!list.connected) box.appendChild(connectCard(sel));
      models(list, box);
    } else if (sel === 'custom') {
      box.appendChild(head(null, 'server', 'Custom server', [{ text: 'Your own OpenAI-compatible servers, as set up in Settings' }]));
      for (const s of (status?.servers || []).filter((x) => !x.onThisPc)) {
        box.appendChild(group(s.host, serverRows(s, c), { glyph: 'server' }));
      }
    } else {
      box.appendChild(head(null, 'pc', 'On this PC', [{ text: 'Private: nothing leaves this PC' }, { text: whereLocal(c.local) }]));
      const warn = slowWarning(c.local);
      if (warn) box.appendChild(h('p', 'pnote warn', warn.text));
      if (c.local?.sharedOnDevice) {
        box.appendChild(h('p', 'pnote', 'Dictation cleanup uses the on-device model too: picking another one here switches it there as well.'));
      }
      const { installed, available } = localRows(c);
      const servers = (status?.servers || []).filter((s) => s.onThisPc);
      if (installed.length) box.appendChild(group('Installed', installed));
      for (const s of servers) box.appendChild(group(`Self-hosted · ${s.host}`, serverRows(s, c), { glyph: 'server' }));
      if (!installed.length && !servers.length) box.appendChild(h('p', 'pk-empty', 'No model is installed on this PC yet.'));
      if (available.length) {
        box.appendChild(h('p', 'pnote', `${available.length === 1 ? 'Another model' : `${available.length} more models`} to download in Chat's model picker on your PC. Then it runs offline.`));
      }
    }
    return box;
  }

  /** The ChatGPT entry with both ways connected: which list shows. */
  function ways(w) {
    const box = h('div', 'ways seg');
    box.setAttribute('role', 'radiogroup');
    box.setAttribute('aria-label', 'Connect ChatGPT with');
    box.dataset.testid = 'picker-ways';
    for (const o of GPT_WAYS) {
      const l = h('label');
      const i = h('input');
      i.type = 'radio';
      i.name = 'gpt-way';
      i.value = o.value;
      i.checked = o.value === w;
      i.addEventListener('change', () => {
        gptWay = o.value;
        build();
      });
      l.append(i, h('span', null, o.label));
      box.appendChild(l);
    }
    return box;
  }

  /** The model in use's effort, kept for it (ai/effort.rs), and Fast mode. */
  function effort() {
    const status = data()?.status;
    const eff = status?.ready && status.model ? status.effort : null;
    if (!eff) return null;
    const name = chipWords(ready?.model, data()).name;
    const box = h('div', 'effort');
    box.dataset.testid = 'picker-effort';
    const label = h('span', 'elabel', 'Effort ');
    label.appendChild(h('span', 'efor', `· ${name}`));
    box.appendChild(label);
    const seg = h('div', 'seg');
    seg.setAttribute('role', 'radiogroup');
    seg.setAttribute('aria-label', `Effort for ${name}`);
    const now = effortNow(eff);
    for (const o of effortOptions(eff)) {
      const l = h('label');
      const i = h('input');
      i.type = 'radio';
      i.name = 'effort';
      i.value = o.value;
      i.checked = o.value === now;
      i.addEventListener('change', () => {
        eff.chosen = o.value || null;
        send({ type: 'set_effort', level: o.value || null });
        if (onChanged) onChanged();
        build();
      });
      l.append(i, h('span', null, o.label));
      seg.appendChild(l);
    }
    box.appendChild(seg);
    box.appendChild(h('p', 'ehint', effortHint(eff)));
    if (typeof eff.fast === 'boolean') {
      const sw = h('label', 'switch');
      const t = h('span', 'switch-text');
      t.appendChild(h('span', 'switch-label', 'Fast mode'));
      t.appendChild(h('span', 'hint', FAST_NOTE));
      const i = h('input');
      i.type = 'checkbox';
      i.setAttribute('role', 'switch');
      i.checked = eff.fast;
      i.addEventListener('change', () => {
        eff.fast = i.checked;
        send({ type: 'set_fast', on: i.checked });
      });
      const track = h('span', 'track');
      track.setAttribute('aria-hidden', 'true');
      sw.append(t, i, track);
      box.appendChild(sw);
    }
    return box;
  }

  /** A PC from before Chat's picker: its plain list (`ready.models`). */
  function plainList() {
    const box = h('div', 'pane');
    box.dataset.testid = 'picker-pane';
    const rows = (ready?.models || []).map((m) => {
      const provider = String(m.id || '').split('|')[0];
      const { icon, glyph } = modelMark(m, null);
      // "GPT OSS 20B · Groq": the name, and where it runs under it.
      const label = m.label || m.id;
      const at = label.lastIndexOf(' · ');
      return {
        key: m.id,
        name: at > 0 ? label.slice(0, at) : label,
        desc: at > 0 ? label.slice(at + 3) : '',
        where: at > 0 ? label.slice(at + 3) : '',
        icon,
        glyph,
        current: ready?.model?.id === m.id,
        action: 'pick',
        plain: m,
        provider,
        tags: [{ t: m.onThisPc ? 'use' : 'key', text: m.onThisPc ? 'On this PC' : 'Cloud' }],
      };
    });
    if (rows.length) box.appendChild(group('Models', rows, { header: false }));
    else box.appendChild(h('p', 'pk-empty', 'No AI model is set up in Yap on your PC yet.'));
    return box;
  }

  function foot() {
    const box = h('div', 'pk-foot');
    const status = data()?.status;
    if (status && sel !== 'pc') {
      box.appendChild(
        h(
          'span',
          null,
          status.ready && !status.fromScope
            ? "Chat uses Dictation Cleanup's model until you pick one here. Cleanup stays as it is."
            : 'Changes the model for Chat only. Dictation cleanup stays as it is.',
        ),
      );
    }
    const b = h('button', 'btn secondary-btn', 'Close');
    b.type = 'button';
    b.addEventListener('click', close);
    box.appendChild(b);
    return box;
  }

  // ------------------------------------------------------------------ pick

  function pick(r) {
    if (r.current) {
      close();
      return;
    }
    let sent;
    if (r.plain) sent = send({ type: 'set_model', id: r.plain.id });
    else {
      sent = send({ type: 'set_model', provider: r.provider, model: r.model || '', baseUrl: r.baseUrl || null, file: r.file || null });
      pushRecent(recentEntry(r));
      // An older one: its More models open next time, where it is.
      if (r.older) moreChoice[r.provider] = true;
    }
    if (sent) {
      const where = r.plain ? (r.plain.onThisPc ? 'On this PC' : r.where) : r.kind === 'ondevice' ? 'On this PC' : r.kind === 'cloud' ? PROVIDER_LABELS[r.provider] || cloudInfo(r.provider)?.short || '' : r.where || '';
      onPicked({ label: r.name, where, icon: r.icon ?? (r.provider ? providerIcon(r.provider) : null), glyph: r.glyph || 'spark', onThisPc: r.kind === 'ondevice' || !!r.plain?.onThisPc });
    }
    close();
  }

  dialog.addEventListener('click', (e) => {
    // A tap outside the sheet closes it.
    if (e.target === dialog) close();
  });

  return {
    open,
    close,
    update,
    get isOpen() {
      return !!dialog.open;
    },
  };
}
