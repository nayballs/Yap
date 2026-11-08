// Yap Lab's screenshots (docs/lab.md, "Screenshots"): the test Yap's
// windows captured over CDP — the whole turn of the thread (scrolled and
// stitched), the panel beside the chat, the live page tab, any element — in
// the light theme, the dark one, or both. The theme is marked on <html> as
// Settings' Theme marks it (`data-yap-theme`, core/appearance.js) and held
// there while the shots are taken, then put back; the live page (a site,
// not Yap) gets `prefers-color-scheme` instead. Nothing is clicked.
import { pngSize, stitch } from './png.mjs';

/** The themes a shot set takes, in order. */
export const THEMES = ['light', 'dark'];

/** `--theme` → the themes to take: "light", "dark", or both (the default). Pure. */
export function themesOf(v) {
  if (v === undefined || v === null || v === true || v === '' || v === 'both') return [...THEMES];
  if (THEMES.includes(v)) return [v];
  throw new Error(`--theme is light, dark or both, not "${v}"`);
}

/**
 * One set's file names, window by window and theme by theme, numbered from
 * `start`: `01-thread-light.png`, `02-thread-dark.png`, `03-panel-light.png`…
 * (`prefix` before the number: a card's set, a named shot). Pure.
 */
export function shotNames(windows, themes, { prefix = '', start = 1 } = {}) {
  const out = [];
  let n = start;
  for (const window of windows) {
    for (const theme of themes) out.push({ window, theme, file: `${prefix}${String(n++).padStart(2, '0')}-${window}-${theme}.png` });
  }
  return out;
}

/** A name as a file name's part: letters, digits and dashes. Pure. */
export const slug = (s) => String(s ?? '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 48) || 'shot';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Two frames drawn (or half a second, for a window that draws none). */
export function frames(page) {
  return page.evaluate(() => new Promise((r) => {
    const t = setTimeout(r, 500);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      clearTimeout(t);
      r();
    }));
  }));
}

/**
 * Yap's page `page` in `theme` ("light" | "dark") and with motion reduced
 * (no transition caught half way), held there — a config save meanwhile
 * marks <html> again, and is put back at once — until `pinTheme(page,
 * null)` puts back what was there.
 */
export async function pinTheme(page, theme) {
  await page.evaluate((t) => {
    const root = document.documentElement;
    const lab = window.__yapLabTheme ?? { was: { yap: root.dataset.yapTheme, bar: root.dataset.barTheme, motion: root.dataset.motion } };
    lab.obs?.disconnect();
    if (!t) {
      const put = (k, v) => (v === undefined ? delete root.dataset[k] : (root.dataset[k] = v));
      put('yapTheme', lab.was.yap);
      put('barTheme', lab.was.bar);
      put('motion', lab.was.motion);
      delete window.__yapLabTheme;
      return;
    }
    const apply = () => {
      if (root.dataset.yapTheme !== t) root.dataset.yapTheme = t;
      if (root.dataset.barTheme !== t) root.dataset.barTheme = t;
      if (root.dataset.motion !== 'reduced') root.dataset.motion = 'reduced';
    };
    apply();
    lab.obs = new MutationObserver(apply);
    lab.obs.observe(root, { attributes: true, attributeFilter: ['data-yap-theme', 'data-bar-theme', 'data-motion'] });
    window.__yapLabTheme = lab;
  }, theme ?? null);
  await frames(page);
}

/** The theme Yap's page `page` shows now ("light" | "dark"). */
export const themeNow = (page) => page.evaluate(() => document.documentElement.dataset.yapTheme ?? 'light');

const SHOT = { scale: 'device', animations: 'disabled', caret: 'hide', timeout: 15_000 };

/**
 * **The thread's turn, whole**: from the turn's question (the last of the
 * thread's questions that starts with `question`'s words, else the last
 * one) to the end of the thread, however long — the thread scrolled a
 * screen at a time, each screen captured at the device's pixels, and the
 * pieces stitched into one tall PNG, from a little above the question but
 * never into the turn before it. The thread's "↓" button and the list's
 * button over its corner are hidden while it's done; the thread's scroll is
 * put back after. Null when Chat's
 * thread isn't showing. `{ png, cssHeight, scrollHeight, clientHeight,
 * pieces, dpr, found }` (`found`: the question was in the thread).
 */
export async function captureThread(main, { question = '' } = {}) {
  const prep = await main.evaluate(async (q) => {
    const col = document.querySelector('[data-testid="chat-column"]');
    const sc = col?.parentElement;
    if (!sc || !sc.getClientRects().length) return null;
    // Pictures in the thread loaded first (a lazy one would grow the turn mid-capture).
    const imgs = [...col.querySelectorAll('img')];
    for (const img of imgs) if (img.loading === 'lazy') img.loading = 'eager';
    const deadline = Date.now() + 4000;
    while (Date.now() < deadline && imgs.some((i) => !i.complete)) await new Promise((r) => setTimeout(r, 100));
    // What floats over the thread without being the turn's: the "↓" button,
    // and the list's button over its top-left corner ("Show your chats",
    // when the list is hidden: in every capture's corner, half cut off, a
    // stray square — 8 Nov 2026).
    const hide = document.createElement('style');
    hide.id = '__yap_lab_shot';
    hide.textContent = '[data-testid="chat-to-bottom"],[data-testid="chat-list-show"]{visibility:hidden!important}';
    document.head.appendChild(hide);
    const was = { top: sc.scrollTop, bottom: sc.scrollHeight - sc.clientHeight - sc.scrollTop < 4, behavior: sc.style.scrollBehavior };
    sc.style.scrollBehavior = 'auto';
    window.__yapLabThread = was;
    const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim().toLowerCase();
    const want = norm(q).slice(0, 40);
    const asks = [...col.querySelectorAll('.cbubble.user')];
    const ask = (want && [...asks].reverse().find((b) => norm(b.innerText).includes(want))) || asks.at(-1) || null;
    const box = sc.getBoundingClientRect();
    const at = (el) => el.getBoundingClientRect().top - box.top + sc.scrollTop;
    // A margin above the question, but never into the turn before it: its
    // last line (a byline's round button) showed as a sliver at the top.
    const before = (() => {
      let row = ask;
      while (row && row.parentElement && row.parentElement !== col) row = row.parentElement;
      const prev = row?.parentElement === col ? row.previousElementSibling : null;
      if (!prev) return 0;
      let bottom = prev.getBoundingClientRect().bottom;
      const inside = prev.querySelectorAll('*');
      for (let i = 0; i < inside.length && i < 4000; i++) {
        const r = inside[i].getBoundingClientRect();
        if (r.height > 0 && r.bottom > bottom) bottom = r.bottom;
      }
      return Math.ceil(bottom - box.top + sc.scrollTop) + 1;
    })();
    const from = ask ? Math.max(0, Math.min(at(ask), Math.max(at(ask) - 12, before))) : 0;
    const to = Math.min(sc.scrollHeight, at(col) + col.offsetHeight + 12);
    return {
      from,
      to: Math.max(to, from + 1),
      x: box.left + sc.clientLeft,
      y: box.top + sc.clientTop,
      w: sc.clientWidth,
      h: sc.clientHeight,
      scrollHeight: sc.scrollHeight,
      dpr: window.devicePixelRatio || 1,
      found: !!ask && (!want || norm(ask.innerText).includes(want)),
    };
  }, question);
  if (!prep) return null;
  const pieces = [];
  try {
    let covered = prep.from;
    for (let guard = 0; covered < prep.to - 0.5 && guard < 400; guard++) {
      const s = await main.evaluate(
        (y) =>
          new Promise((r) => {
            const sc = document.querySelector('[data-testid="chat-column"]').parentElement;
            sc.scrollTop = y;
            const t = setTimeout(() => r(sc.scrollTop), 500);
            requestAnimationFrame(() => requestAnimationFrame(() => {
              clearTimeout(t);
              r(sc.scrollTop);
            }));
          }),
        covered
      );
      const keepTo = Math.min(s + prep.h, prep.to);
      if (keepTo <= covered + 0.5) break;
      const png = await main.screenshot({ ...SHOT, clip: { x: prep.x, y: prep.y, width: prep.w, height: prep.h } });
      const { height } = pngSize(png);
      const dpr = height / prep.h;
      pieces.push({ png, from: (covered - s) * dpr, to: (keepTo - s) * dpr });
      covered = keepTo;
    }
  } finally {
    await main
      .evaluate(() => {
        document.getElementById('__yap_lab_shot')?.remove();
        const sc = document.querySelector('[data-testid="chat-column"]')?.parentElement;
        const was = window.__yapLabThread;
        delete window.__yapLabThread;
        if (!sc || !was) return;
        sc.scrollTop = was.bottom ? sc.scrollHeight : was.top;
        sc.style.scrollBehavior = was.behavior;
      })
      .catch(() => {});
  }
  if (!pieces.length) return null;
  return { png: stitch(pieces), cssHeight: Math.round(prep.to - prep.from), scrollHeight: Math.round(prep.scrollHeight), clientHeight: Math.round(prep.h), pieces: pieces.length, dpr: prep.dpr, found: prep.found };
}

/** **An element** of Yap's page `page` (the first `selector` matches), or null when none shows. */
export async function captureElement(page, selector) {
  const el = page.locator(selector).first();
  if (!(await el.isVisible().catch(() => false))) return null;
  const dpr = await page.evaluate(() => window.devicePixelRatio || 1);
  return { png: await el.screenshot(SHOT), dpr };
}

/** **The panel beside the chat** (its document, picture, page or Agent tab), or null when it's closed. */
export const capturePanel = (main) => captureElement(main, '[data-testid="chat-panel-slot"]');

/** **A whole window** of Yap's (`main`, the bar). */
export async function captureWindow(page) {
  const dpr = await page.evaluate(() => window.devicePixelRatio || 1);
  return { png: await page.screenshot(SHOT), dpr };
}

/**
 * The live page beside the chat — its own WebView2, not Yap's, on the same
 * DevTools port in a browser context of its own: the one on screen (from
 * `views`, `e2e_page_views`), else the last. Null when there's none.
 */
export function livePage(yap, views, { onScreenOnly = false } = {}) {
  const ours = new Set(Object.values(yap.pages));
  const shown = views.find((v) => v.onScreen ?? v.on_screen) ?? (onScreenOnly ? null : views.at(-1));
  if (!shown) return null;
  const pages = yap.browser.contexts().flatMap((c) => c.pages()).filter((p) => !ours.has(p) && !p.url().startsWith('http://tauri.localhost/'));
  const strip = (u) => String(u ?? '').replace(/\/+$/, '');
  // (Its address, or where it loads from: a stand-in web's.)
  const at = [shown.url, shown.address, shown.load].filter(Boolean).map(strip);
  return pages.find((p) => at.includes(strip(p.url()))) ?? pages.at(-1) ?? null;
}

/**
 * **The live page** `page` as a PC in `theme` shows it (`prefers-color-scheme`:
 * a site's own dark scheme, if it has one), motion reduced; then as it was.
 */
export async function captureLivePage(page, theme) {
  try {
    if (theme) await page.emulateMedia({ colorScheme: theme, reducedMotion: 'reduce' });
    await sleep(150);
    const dpr = await page.evaluate(() => window.devicePixelRatio || 1).catch(() => 1);
    return { png: await page.screenshot(SHOT), dpr };
  } finally {
    if (theme) await page.emulateMedia({ colorScheme: null, reducedMotion: null }).catch(() => {});
  }
}
