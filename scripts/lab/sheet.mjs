// Yap Lab's contact sheet (docs/lab.md, `sheet`): a run's screenshots on
// one page a person can scan — a section a turn (its number, how it ended,
// its question's first words), its shots in two columns (light beside
// dark), each at no more than its own CSS size, a tall thread shot cut into
// screens of at most SEGMENT pixels so it stays readable — written as HTML
// (the shots by relative path, so it opens from disk) and rendered to one
// PNG in the lab's own headless browser (Playwright's Chromium, else Edge).
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { pngSize, stitch } from './png.mjs';

/** The sheet's width in pixels. */
export const WIDTH = 1600;
/** Its margin, and the gap between two shots. */
const PAD = 24;
const GAP = 16;
/** A shot's column: two a row. */
export const TILE = (WIDTH - 2 * PAD - GAP) / 2;
/** The tallest a piece of a shot is on the sheet (a taller one is cut). */
export const SEGMENT = 1400;
/** The tallest one capture of the sheet is (taller: in parts, stitched). */
const CAPTURE_MAX = 8000;

/**
 * `--turns` → which turns: "3", "1-3", "2-" (from 2 on), "1,4,6", or
 * everything (undefined). Pure.
 */
export function parseTurns(spec) {
  if (spec === undefined || spec === null || spec === true || String(spec).trim() === '') return () => true;
  const parts = String(spec).split(',').map((p) => p.trim()).filter(Boolean);
  const tests = parts.map((p) => {
    const m = /^(\d+)?\s*(-)?\s*(\d+)?$/.exec(p);
    if (!m || (!m[1] && !m[3])) throw new Error(`--turns is like 3, 1-3, 2- or 1,4 — not "${spec}"`);
    const lo = m[1] ? Number(m[1]) : 1;
    const hi = m[2] ? (m[3] ? Number(m[3]) : Infinity) : lo;
    return (n) => n >= lo && n <= hi;
  });
  return (n) => tests.some((t) => t(n));
}

const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
/** The first `n` words of `s`, with "…" when there were more. Pure. */
export function firstWords(s, n = 14) {
  const words = String(s ?? '').replace(/\s+/g, ' ').trim().split(' ').filter(Boolean);
  return words.length > n ? `${words.slice(0, n).join(' ')}…` : words.join(' ');
}

/** What a set of shots was: after the turn, a card waiting, or a named shot. Pure. */
function setLabel(set) {
  if (!set || set === 'end') return 'After the turn';
  if (set.startsWith('card:')) return `While card ${set.slice(5)} waited`;
  if (set.startsWith('named:')) return `Shot “${set.slice(6)}”`;
  return set;
}

/**
 * How a shot of `width` × `height` device pixels at `dpr` sits in a column:
 * its size on the sheet (never over its CSS size, never over the column)
 * and its pieces' offsets and heights. Pure.
 */
export function placeShot({ width, height, dpr = 1 }) {
  const w = Math.max(1, Math.min(TILE, Math.round(width / (dpr || 1))));
  const scale = w / width;
  const h = Math.max(1, Math.round(height * scale));
  // Pieces of about the same height (never a sliver left at the end).
  const each = Math.ceil(h / Math.ceil(h / SEGMENT));
  const pieces = [];
  for (let at = 0; at < h; at += each) pieces.push({ at, h: Math.min(each, h - at) });
  return { w, h, pieces };
}

/**
 * **The sheet's HTML**: `turns` — `[{ turn, question, status, shots: [{ file,
 * window, theme, set, width, height, dpr }] }]` — under `title` and
 * `subtitle`, its pictures by their path from `dir` (where the HTML goes).
 * Pure.
 */
export function sheetHtml({ title = 'Yap Lab', subtitle = '', turns, dir }) {
  const sections = turns.map((t) => {
    const sets = [];
    for (const s of t.shots) {
      let set = sets.find((x) => x.set === (s.set ?? 'end'));
      if (!set) sets.push((set = { set: s.set ?? 'end', shots: [] }));
      set.shots.push(s);
    }
    const blocks = sets.map(({ set, shots }) => {
      const cells = [];
      const windows = [...new Set(shots.map((s) => s.window))];
      for (const window of windows) {
        const mine = shots.filter((s) => s.window === window).map((s) => ({ s, place: placeShot(s) }));
        const most = Math.max(...mine.map((m) => m.place.pieces.length));
        for (let k = 0; k < most; k++) {
          for (const [i, { s, place }] of mine.entries()) {
            const piece = place.pieces[k];
            if (!piece) continue;
            const src = path.relative(dir, s.file).split(path.sep).join('/');
            const part = place.pieces.length > 1 ? ` · ${k + 1}/${place.pieces.length}` : '';
            // Light and dark of one window side by side: the first of a pair starts a row.
            const first = mine.length === 2 && i === 0 ? ' first' : '';
            cells.push(
              `<figure class="cell${first}"><figcaption><b>${esc(window)}</b> · ${esc(s.theme ?? '')}${part}<span class="dim">${esc(path.basename(s.file))} · ${s.width}×${s.height}</span></figcaption>` +
                `<div class="clip" style="width:${place.w}px;height:${piece.h}px"><img src="${esc(src)}" alt="" style="width:${place.w}px;height:${place.h}px;margin-top:-${piece.at}px"></div></figure>`
            );
          }
        }
      }
      return `<h3>${esc(setLabel(set))}</h3><div class="grid">${cells.join('')}</div>`;
    });
    const status = t.status ? `<span class="st st-${esc(t.status)}">${esc(t.status)}</span>` : '';
    const how = [t.spoken ? 'spoken' : '', t.conversation != null ? `conversation ${t.conversation}` : ''].filter(Boolean).join(' · ');
    return `<section class="turn"><h2>Turn ${esc(t.turn)} ${status}${how ? `<span class="how">${esc(how)}</span>` : ''}</h2><p class="q">“${esc(firstWords(t.question))}”</p>${blocks.join('')}</section>`;
  });
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>${esc(title)}</title>
<style>
  * { box-sizing: border-box; }
  html, body { margin: 0; background: #e6e6ea; color: #1d1d22; font: 14px/1.4 'Segoe UI', system-ui, sans-serif; }
  body { width: ${WIDTH}px; padding: ${PAD}px; }
  header h1 { margin: 0; font-size: 22px; }
  header p { margin: 4px 0 0; color: #55555f; }
  .turn { margin-top: ${PAD}px; padding-top: 12px; border-top: 2px solid #c9c9d1; }
  h2 { margin: 0; font-size: 18px; display: flex; gap: 10px; align-items: baseline; }
  .st { font-size: 12px; font-weight: 600; padding: 1px 8px; border-radius: 99px; background: #d7d7de; }
  .st-answered { background: #cdebd6; } .st-failed { background: #f3d0cf; } .st-waiting, .st-running { background: #f3e6c4; }
  .how { font-size: 12px; font-weight: 400; color: #55555f; }
  .q { margin: 4px 0 0; font-size: 15px; color: #33333b; }
  h3 { margin: 14px 0 6px; font-size: 13px; font-weight: 600; color: #55555f; text-transform: uppercase; letter-spacing: .04em; }
  .grid { display: grid; grid-template-columns: ${TILE}px ${TILE}px; gap: ${GAP}px; align-items: start; }
  .cell { margin: 0; }
  .cell.first { grid-column: 1; }
  figcaption { display: flex; gap: 6px; align-items: baseline; font-size: 12px; margin-bottom: 4px; }
  figcaption .dim { margin-left: auto; color: #6b6b75; }
  .clip { overflow: hidden; outline: 1px solid #b9b9c2; background: #fff; }
  .clip img { display: block; }
</style></head>
<body><header><h1>${esc(title)}</h1>${subtitle ? `<p>${esc(subtitle)}</p>` : ''}</header>
${sections.join('\n')}
</body></html>
`;
}

/** The shots of the lab's run in `lab` (its `shots/NNN/index.json`), the turns `which` picks, in order. */
export function runShots(lab, which = () => true) {
  const dir = path.join(lab, 'shots');
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const name of fs.readdirSync(dir)) {
    if (!/^\d{3,}$/.test(name)) continue;
    const file = path.join(dir, name, 'index.json');
    if (!fs.existsSync(file)) continue;
    try {
      const t = JSON.parse(fs.readFileSync(file, 'utf8'));
      if (!which(Number(t.turn))) continue;
      t.shots = (t.shots ?? []).filter((s) => fs.existsSync(s.file));
      if (t.shots.length) out.push(t);
    } catch {
      /* an index being written: the next sheet has it */
    }
  }
  return out.sort((a, b) => a.turn - b.turn);
}

/** The lab's own headless browser: Playwright's Chromium when it's installed, else Edge (YAP_LAB_CHANNEL to choose). */
async function browser() {
  const { chromium } = await import('@playwright/test');
  let channel = process.env.YAP_LAB_CHANNEL || undefined;
  if (!channel) {
    try {
      if (!fs.existsSync(chromium.executablePath())) channel = 'msedge';
    } catch {
      channel = 'msedge';
    }
  }
  return chromium.launch({ headless: true, ...(channel && channel !== 'chromium' ? { channel } : {}) });
}

/** A page of a sheet at most this tall: chat apps refuse images over 8000 px. */
export const PAGE_MAX = 7600;

/**
 * Where to cut a sheet `height` px tall into pages of at most `max` px:
 * at a block's top (`tops`: each turn's header and each shot's cell, in px)
 * where one fits, else straight at `max` (a block taller than a page).
 * `[{ y, h }]`. Pure.
 */
export function pageCuts(height, tops, max = PAGE_MAX) {
  const starts = [...new Set(tops.filter((t) => t > 0 && t < height).map(Math.round))].sort((a, b) => a - b);
  const out = [];
  let y = 0;
  while (y < height) {
    if (height - y <= max) {
      out.push({ y, h: height - y });
      break;
    }
    const fit = starts.filter((t) => t > y && t - y <= max).pop();
    const end = fit ?? y + max;
    out.push({ y, h: end - y });
    y = end;
  }
  return out;
}

/**
 * The HTML file `html` rendered to the PNG `png`, its whole height, and — when
 * it's taller than a page — the same cut into pages between its shots
 * (`<png>-p1.png`, …), each under [`PAGE_MAX`], to send or view whole.
 * `{ width, height, pages }`.
 */
export async function renderSheet(html, png) {
  const b = await browser();
  try {
    const page = await b.newPage({ viewport: { width: WIDTH, height: 900 }, deviceScaleFactor: 1 });
    await page.goto(pathToFileURL(html).href, { waitUntil: 'load' });
    await page.evaluate(() => Promise.all([...document.images].map((i) => (i.complete ? null : new Promise((r) => (i.onload = i.onerror = r))))));
    const height = await page.evaluate(() => Math.ceil(document.documentElement.scrollHeight));
    const grab = async (y, h) => {
      const parts = [];
      for (let at = y; at < y + h; at += CAPTURE_MAX) {
        const ph = Math.min(CAPTURE_MAX, y + h - at);
        parts.push({ png: await page.screenshot({ fullPage: true, clip: { x: 0, y: at, width: WIDTH, height: ph }, timeout: 60_000 }) });
      }
      return parts.length === 1 ? parts[0].png : stitch(parts);
    };
    const bytes = await grab(0, height);
    fs.writeFileSync(png, bytes);
    const pages = [];
    if (height > PAGE_MAX) {
      const tops = await page.evaluate(() => [...document.querySelectorAll('h2, .turn, figure.cell')].map((e) => e.getBoundingClientRect().top + window.scrollY));
      const cuts = pageCuts(height, tops);
      const stem = png.replace(/\.png$/i, '');
      for (const [i, c] of cuts.entries()) {
        const file = `${stem}-p${i + 1}.png`;
        fs.writeFileSync(file, await grab(c.y, c.h));
        pages.push(file);
      }
    }
    return { ...pngSize(bytes), pages };
  } finally {
    await b.close().catch(() => {});
  }
}

/**
 * **A contact sheet** of the lab's run in `lab`: the turns `turns` picks
 * (`--turns`), to `out` (a PNG; the HTML beside it, `.html`), else
 * `sheets/sheet-<time>.png`. `{ png, html, width, height, turns, shots }`.
 */
export async function makeSheet({ lab, turns, out, title = 'Yap Lab', subtitle = '' }) {
  const which = typeof turns === 'function' ? turns : parseTurns(turns);
  const list = runShots(lab, which);
  if (!list.length) throw new Error(`No shots${turns ? ` for turns ${turns}` : ''} in this run (${path.join(lab, 'shots')}): ask something first (shots are on unless --no-shots)`);
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const png = out ? path.resolve(out) : path.join(lab, 'sheets', `sheet-${stamp}.png`);
  const html = png.replace(/\.png$/i, '') + '.html';
  fs.mkdirSync(path.dirname(png), { recursive: true });
  // The sheet keeps its own copies of its shots (`<sheet>_files/`): the next
  // `start` writes its turns' shots into the same `shots/NNN/` folders, which
  // left an earlier sheet's page pointing at another run's pictures.
  const own = png.replace(/\.png$/i, '') + '_files';
  fs.mkdirSync(own, { recursive: true });
  const kept = list.map((t) => ({
    ...t,
    shots: t.shots.map((s) => {
      const name = `${String(t.turn).padStart(3, '0')}-${path.basename(s.file)}`;
      const copy = path.join(own, name);
      try {
        fs.copyFileSync(s.file, copy);
        return { ...s, file: copy };
      } catch {
        return s;
      }
    }),
  }));
  fs.writeFileSync(html, sheetHtml({ title, subtitle, turns: kept, dir: path.dirname(html) }));
  const size = await renderSheet(html, png);
  return { png, html, ...size, turns: list.map((t) => t.turn), shots: list.reduce((n, t) => n + t.shots.length, 0) };
}
