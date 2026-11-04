// What the dev launcher draws: the Yap mark in half blocks (24-bit colour),
// the wordmark, a waveform like the Yap bar's, the stages and the build bar,
// the raw log view. Everything lands on a Canvas of cells that turns into
// ANSI rows (only the changed ones get written) or plain text (the
// YAP_DEV_FRAMES snapshots). Glyphs stay inside the WGL4 set (block halves,
// shades, simple geometric shapes) so conhost's Consolas shows them too.

import { fmtDuration } from './parse.mjs';

const rgb = (r, g, b) => (r << 16) | (g << 8) | b;
const R = (c) => (c >> 16) & 255;
const G = (c) => (c >> 8) & 255;
const B = (c) => c & 255;
const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v);
export function mix(a, b, t) {
  t = clamp01(t);
  return rgb(
    Math.round(R(a) + (R(b) - R(a)) * t),
    Math.round(G(a) + (G(b) - G(a)) * t),
    Math.round(B(a) + (B(b) - B(a)) * t),
  );
}

// The brand (desktop/src/assets/yap-logo.svg, app.css) on a warm near-black page.
export const C = {
  page: rgb(0x14, 0x12, 0x0f),
  bar: rgb(0x26, 0x21, 0x1b),
  badgeA: rgb(0x33, 0x2e, 0x25),
  badgeB: rgb(0x1f, 0x1c, 0x16),
  mouthA: rgb(0xf0, 0xb0, 0x4a),
  mouthB: rgb(0xc2, 0x69, 0x0a),
  eye: rgb(0x1f, 0x1c, 0x16),
  cream: rgb(0xf7, 0xf5, 0xf0),
  amber: rgb(0xf0, 0xb0, 0x4a),
  amberDeep: rgb(0xc2, 0x69, 0x0a),
  amberHi: rgb(0xff, 0xe0, 0xa6),
  text: rgb(0xdc, 0xd5, 0xc8),
  dim: rgb(0x9c, 0x92, 0x82),
  faint: rgb(0x5f, 0x57, 0x4b),
  track: rgb(0x30, 0x2b, 0x23),
  green: rgb(0x93, 0xc9, 0x7c),
  red: rgb(0xef, 0x6b, 0x5b),
  redSoft: rgb(0xe3, 0xa8, 0x9e),
  yellow: rgb(0xea, 0xc2, 0x5e),
  ink: rgb(0x1f, 0x1c, 0x16),
};

// ── text widths ─────────────────────────────────────────────────────────────

export function charWidth(cp) {
  if (cp < 0x300) return cp < 32 ? 0 : 1;
  if ((cp >= 0x300 && cp <= 0x36f) || (cp >= 0x200b && cp <= 0x200f) || (cp >= 0xfe00 && cp <= 0xfe0f)) return 0;
  if (
    (cp >= 0x1100 && cp <= 0x115f) ||
    (cp >= 0x2e80 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1faff) ||
    (cp >= 0x20000 && cp <= 0x3fffd)
  )
    return 2;
  return 1;
}

export function strWidth(s) {
  let w = 0;
  for (const ch of s) w += charWidth(ch.codePointAt(0));
  return w;
}

/** At most `max` columns, an ellipsis where it was cut. */
export function truncate(s, max) {
  if (max <= 0) return '';
  if (strWidth(s) <= max) return s;
  let out = '';
  let w = 0;
  for (const ch of s) {
    const cw = charWidth(ch.codePointAt(0));
    if (w + cw > max - 1) break;
    out += ch;
    w += cw;
  }
  return out + '…';
}

/** The end of `s` in at most `max` columns (for paths). */
export function truncateLeft(s, max) {
  if (strWidth(s) <= max) return s;
  const chars = [...s];
  let out = '';
  let w = 0;
  for (let i = chars.length - 1; i >= 0; i--) {
    const cw = charWidth(chars[i].codePointAt(0));
    if (w + cw > max - 1) break;
    out = chars[i] + out;
    w += cw;
  }
  return '…' + out;
}

// ── the canvas ──────────────────────────────────────────────────────────────

export class Canvas {
  constructor(w, h, base = C.page) {
    this.w = w;
    this.h = h;
    this.base = base;
    const n = w * h;
    this.ch = new Array(n).fill(' ');
    this.pch = new Array(n).fill(null); // the plain-text stand-in, when it differs
    this.fg = new Int32Array(n).fill(C.text);
    this.bg = new Int32Array(n).fill(base);
    this.bold = new Uint8Array(n);
  }

  set(x, y, ch, fg, bg, plain = null, bold = 0) {
    if (x < 0 || y < 0 || x >= this.w || y >= this.h) return;
    const i = y * this.w + x;
    this.ch[i] = ch;
    this.pch[i] = plain;
    if (fg != null) this.fg[i] = fg;
    if (bg != null) this.bg[i] = bg;
    this.bold[i] = bold ? 1 : 0;
  }

  fill(x, y, w, bg) {
    for (let i = 0; i < w; i++) this.set(x + i, y, ' ', null, bg);
  }

  /** Writes text from (x, y), cut at `max` (exclusive column); returns the next x. */
  text(x, y, s, fg = C.text, o = {}) {
    if (y < 0 || y >= this.h || s == null) return x;
    const limit = Math.min(this.w, o.max ?? this.w);
    s = truncate(String(s).replace(/\t/g, '    '), limit - x);
    for (const ch of s) {
      const w = charWidth(ch.codePointAt(0));
      if (w === 0) continue;
      if (x + w > limit) break;
      this.set(x, y, ch, fg, o.bg, null, o.bold);
      if (w === 2) this.set(x + 1, y, '', fg, o.bg);
      x += w;
    }
    return x;
  }

  /** Text segments [[text, fg, bold?], ...] from x. */
  segs(x, y, segs, max) {
    for (const [s, fg, bold] of segs) {
      if (x >= (max ?? this.w)) break;
      x = this.text(x, y, s, fg, { bold, max });
    }
    return x;
  }

  rowAnsi(y, { transparent = false, skipLastCell = false } = {}) {
    let out = '';
    let f = -2;
    let b = -2;
    let bo = 0;
    const end = skipLastCell ? this.w - 1 : this.w;
    for (let x = 0; x < end; x++) {
      const i = y * this.w + x;
      const ch = this.ch[i];
      if (ch === '') continue;
      if (ch !== ' ') {
        if (this.bold[i] !== bo) {
          bo = this.bold[i];
          out += bo ? '\x1b[1m' : '\x1b[22m';
        }
        const fc = this.fg[i];
        if (fc !== f) {
          f = fc;
          out += `\x1b[38;2;${R(fc)};${G(fc)};${B(fc)}m`;
        }
      }
      let bc = this.bg[i];
      if (transparent && bc === this.base) bc = -1;
      if (bc !== b) {
        b = bc;
        out += bc < 0 ? '\x1b[49m' : `\x1b[48;2;${R(bc)};${G(bc)};${B(bc)}m`;
      }
      out += ch;
    }
    return out + '\x1b[0m';
  }

  rowPlain(y) {
    let out = '';
    for (let x = 0; x < this.w; x++) {
      const i = y * this.w + x;
      if (this.ch[i] === '') continue;
      out += this.pch[i] ?? this.ch[i];
    }
    return out.replace(/\s+$/, '');
  }
}

// ── the mark ────────────────────────────────────────────────────────────────
// The SVG's shapes in its own 1024-unit space: the rounded badge (rx 230),
// the mouth (r 230 at 390,505 with a wedge cut out to the right), the eye, and
// three arcs with their own centres (worked out from the SVG's end points).

const ARCS = [
  [660, 390, 620, 160],
  [738, 348, 662, 210],
  [816, 306, 704, 260],
].map(([x, y1, y2, r]) => {
  const hc = (y2 - y1) / 2;
  const cx = x - Math.sqrt(r * r - hc * hc);
  return { x, y1, y2, r, cx, cy: (y1 + y2) / 2, theta: Math.atan2(hc, x - cx) };
});

function inBadge(x, y) {
  const r = 230;
  const qx = Math.abs(x - 512) - (512 - r);
  const qy = Math.abs(y - 512) - (512 - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r <= 0;
}

function arcDistance(x, y, a) {
  const dx = x - a.cx;
  const dy = y - a.cy;
  if (Math.abs(Math.atan2(dy, dx)) <= a.theta) return Math.abs(Math.hypot(dx, dy) - a.r);
  return Math.min(Math.hypot(x - a.x, y - a.y1), Math.hypot(x - a.x, y - a.y2));
}

const CLS = { none: 0, badge: 1, mouth: 2, eye: 3, arc: 4 };

/**
 * The mark as N×N pixels: packed colours (-1 where it's see-through) and what
 * each pixel mostly is (for the plain-text frames). `a.open` 0..1 is how far
 * the mouth is open (0.82 is the logo's own 40° each side), `a.arcs` each
 * arc's opacity, `a.blink` squints the eye.
 */
export function logoPixels(N, a) {
  const S = 3;
  const scale = 1024 / N;
  // The arcs are about a pixel wide at console sizes: smoothed, they'd fade to
  // grey, so they're drawn as crisp pixel lines (each pixel's centre decides).
  const hw = Math.max(20, 0.62 * scale);
  const eyeR = Math.max(33, 0.8 * scale);
  const eyeRy = eyeR * (a.blink ? 0.25 : 1);
  const half = ((4 + 44 * clamp01(a.open)) * Math.PI) / 180;
  const col = new Int32Array(N * N);
  const cls = new Uint8Array(N * N);
  const counts = new Uint16Array(5);
  for (let py = 0; py < N; py++) {
    for (let px = 0; px < N; px++) {
      let r = 0;
      let g = 0;
      let b = 0;
      let cov = 0;
      counts.fill(0);
      for (let sy = 0; sy < S; sy++) {
        for (let sx = 0; sx < S; sx++) {
          const X = (px + (sx + 0.5) / S) * scale;
          const Y = (py + (sy + 0.5) / S) * scale;
          if (!inBadge(X, Y)) {
            counts[0]++;
            continue;
          }
          let c = mix(C.badgeA, C.badgeB, (X + Y) / 2048);
          let k = CLS.badge;
          const dx = X - 390;
          const dy = Y - 505;
          if (dx * dx + dy * dy <= 230 * 230 && Math.abs(Math.atan2(dy, dx)) > half) {
            c = mix(C.mouthA, C.mouthB, (Y - 275) / 460);
            k = CLS.mouth;
            const ex = (X - 400) / eyeR;
            const ey = (Y - 385) / eyeRy;
            if (ex * ex + ey * ey <= 1) {
              c = C.eye;
              k = CLS.eye;
            }
          }
          counts[k]++;
          r += R(c);
          g += G(c);
          b += B(c);
          cov++;
        }
      }
      const i = py * N + px;
      if (!cov) {
        col[i] = -1;
        continue;
      }
      // blend the covered part over the page
      const t = cov / (S * S);
      const avg = rgb(Math.round(r / cov), Math.round(g / cov), Math.round(b / cov));
      col[i] = t >= 0.999 ? avg : mix(C.page, avg, t);
      let best = 0;
      for (let k = 1; k < 5; k++) if (counts[k] > counts[best]) best = k;
      cls[i] = best;
      const cx = (px + 0.5) * scale;
      const cy = (py + 0.5) * scale;
      if (best === CLS.badge && inBadge(cx, cy)) {
        for (let k = 0; k < 3; k++) {
          const al = a.arcs[k];
          if (al > 0.02 && arcDistance(cx, cy, ARCS[k]) <= hw) {
            col[i] = mix(col[i], C.cream, al);
            if (al >= 0.5) cls[i] = CLS.arc;
          }
        }
      }
    }
  }
  return { col, cls };
}

const INK = (k) => k === CLS.mouth || k === CLS.arc;

/** The mark at (x, y): N columns, N/2 rows, two pixels per cell (▀). */
export function drawLogo(cv, x, y, N, anim) {
  const { col, cls } = logoPixels(N, anim);
  for (let row = 0; row < N / 2; row++) {
    for (let c = 0; c < N; c++) {
      const ti = row * 2 * N + c;
      const bi = ti + N;
      const top = col[ti];
      const bot = col[bi];
      if (top < 0 && bot < 0) continue;
      const it = INK(cls[ti]);
      const ib = INK(cls[bi]);
      const shade = cls[ti] === CLS.badge || cls[bi] === CLS.badge ? '░' : ' ';
      const plain = it && ib ? '█' : it ? '▀' : ib ? '▄' : shade;
      cv.set(x + c, y + row, '▀', top < 0 ? cv.base : top, bot < 0 ? cv.base : bot, plain);
    }
  }
}

// Mouth and arcs over time. Yapping: syllables of different sizes in
// phrases, the arcs appearing one after another and fading at the phrase's
// end. Calm (running): the logo's own pose, a ripple out through the arcs
// every few seconds, now and then a blink.
const smooth = (v) => {
  v = clamp01(v);
  return v * v * (3 - 2 * v);
};
const hash = (n) => {
  const s = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return s - Math.floor(s);
};

export function yapAnim(t) {
  const PHRASE = 1.9;
  const p = t % PHRASE;
  const phrase = Math.floor(t / PHRASE);
  let open = 0.08;
  if (p < 1.4) {
    const SYL = 0.233;
    const k = Math.floor(p / SYL);
    const ph = (p % SYL) / SYL;
    const amp = 0.45 + 0.55 * hash(phrase * 13 + k);
    open = 0.08 + (amp - 0.08) * Math.sin(Math.PI * ph);
  }
  const fadeOut = 1 - smooth((p - 1.45) / 0.35);
  const arcs = [0.12, 0.42, 0.72].map((s) => smooth((p - s) / 0.18) * fadeOut);
  return { open, arcs, blink: false };
}

export function calmAnim(t) {
  const PERIOD = 3.2;
  const p = t % PERIOD;
  const arcs = [0, 1, 2].map((i) => {
    const d = (p - i * 0.28) / 0.5;
    const bump = d > 0 && d < 1 ? Math.sin(Math.PI * d) : 0;
    return 0.62 + 0.38 * bump;
  });
  const blink = t % 5.3 < 0.16;
  return { open: 0.82, arcs, blink };
}

export const QUIET = { open: 0.06, arcs: [0, 0, 0], blink: false };

// ── the wordmark ────────────────────────────────────────────────────────────

const GLYPHS = {
  Y: ['XX..XX', 'XX..XX', '.XXXX.', '..XX..', '..XX..', '..XX..', '......', '......'],
  a: ['......', '......', '.XXXXX', 'XX..XX', 'XX..XX', '.XXXXX', '......', '......'],
  p: ['......', '......', 'XXXXX.', 'XX..XX', 'XX..XX', 'XXXXX.', 'XX....', 'XX....'],
};
export const WORDMARK_W = 22;

/** "Yap" in big half-block letters: 22 columns, 4 rows. */
export function drawWordmark(cv, x, y, fg = C.cream) {
  let cx = x;
  for (const letter of 'Yap') {
    const g = GLYPHS[letter];
    for (let row = 0; row < 4; row++) {
      for (let c = 0; c < 6; c++) {
        const t = g[row * 2][c] === 'X';
        const b = g[row * 2 + 1][c] === 'X';
        if (!t && !b) continue;
        cv.set(cx + c, y + row, t && b ? '█' : t ? '▀' : '▄', fg, null);
      }
    }
    cx += 8;
  }
}

// ── the waveform ────────────────────────────────────────────────────────────

/** Bars every other column over 3 rows, newest on the right, like the Yap bar. */
export function drawWave(cv, x, y, width, values, colour = 'amber') {
  const bars = Math.floor((width + 1) / 2);
  const vs = values.slice(-bars);
  while (vs.length < bars) vs.unshift(0);
  for (let i = 0; i < bars; i++) {
    const v = clamp01(vs[i]);
    const cx = x + i * 2;
    const level = v < 0.06 ? 0 : v < 0.25 ? 1 : v < 0.5 ? 2 : v < 0.75 ? 3 : 4;
    const fg =
      colour === 'red' ? C.red : colour === 'dim' ? C.faint : level === 0 ? C.faint : mix(C.amberDeep, C.amberHi, 0.25 + v * 0.75);
    if (level === 0) cv.set(cx, y + 1, '·', fg, null);
    else if (level === 1) cv.set(cx, y + 1, '▬', fg, null);
    else if (level === 2) cv.set(cx, y + 1, '█', fg, null);
    else if (level === 3) {
      cv.set(cx, y, '▄', fg, null);
      cv.set(cx, y + 1, '█', fg, null);
      cv.set(cx, y + 2, '▀', fg, null);
    } else for (let r = 0; r < 3; r++) cv.set(cx, y + r, '█', fg, null);
  }
}

// ── the progress bar ────────────────────────────────────────────────────────

/** frac 0..1, or null for "no idea yet" (a segment going back and forth). */
export function drawBar(cv, x, y, w, frac, t, { error = false } = {}) {
  if (w < 4) return;
  if (frac == null) {
    const seg = Math.max(3, Math.round(w / 6));
    const span = w - seg;
    const p = (t * 0.6) % 2;
    const start = Math.round((p < 1 ? p : 2 - p) * span);
    for (let i = 0; i < w; i++) {
      const on = i >= start && i < start + seg;
      cv.set(x + i, y, ' ', null, on ? mix(C.amberDeep, C.amber, (i - start) / seg) : C.track, on ? '█' : '░');
    }
    return;
  }
  const exact = clamp01(frac) * w;
  const full = Math.floor(exact);
  const halfCell = exact - full >= 0.5;
  // a soft highlight sweeping over the filled part: the bar is alive even when the number isn't moving
  const sweep = ((t * 0.7) % 1.6) * (full + 8) - 4;
  for (let i = 0; i < w; i++) {
    let fill = error ? C.red : mix(C.amberDeep, C.amber, w > 1 ? i / (w - 1) : 1);
    if (!error) fill = mix(fill, C.amberHi, Math.max(0, 1 - Math.abs(i - sweep) / 4) * 0.55);
    if (i < full) cv.set(x + i, y, ' ', null, fill, '█');
    else if (i === full && halfCell) cv.set(x + i, y, '▌', fill, C.track, '▌');
    else cv.set(x + i, y, ' ', null, C.track, '░');
  }
}

// ── screens ─────────────────────────────────────────────────────────────────

const SPIN = ['▀', '▐', '▄', '▌'];
const spinner = (t) => SPIN[Math.floor(t * 7) % 4];
const clock = (ms) => {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
};
const plural = (n, one, many = one + 's') => `${n} ${n === 1 ? one : many}`;

/** The mark's pose for what's going on. */
export function logoAnimFor(m) {
  const { state, ui, t } = m;
  const stage = ui.mode === 'update' ? (ui.updateDone ? 'running' : 'update') : state.stage;
  if (stage === 'error' || stage === 'exited') return QUIET;
  if (stage === 'running' && t >= (ui.yapUntil || 0)) return calmAnim(t);
  return yapAnim(t);
}

/** One waveform sample a tick, newest last. */
export function waveSample(m) {
  const { state, ui, t } = m;
  const stage = ui.mode === 'update' ? 'update' : state.stage;
  if (stage === 'error' || stage === 'exited') return 0;
  if (stage === 'running' && t >= (ui.yapUntil || 0)) {
    return 0.03 + 0.07 * hash(Math.floor(t * 10)) + (ui.waveKick || 0);
  }
  const { open } = yapAnim(t);
  return clamp01(open * (0.75 + 0.5 * hash(Math.floor(t * 10) + 7)) + 0.05);
}

function logoSize(m, W, H) {
  const tall = m.state.stage === 'error' ? 40 : 30;
  if (H >= tall && W >= 72) return 24;
  if (H >= 22 && W >= 60) return 16;
  return 0;
}

function drawHeader(cv, m, N) {
  const { ui } = m;
  const y0 = 1;
  const x0 = 3;
  if (!N) {
    let x = cv.text(x0, y0, 'Yap', C.cream, { bold: true });
    x = cv.text(x + 1, y0, '·', C.faint);
    x = cv.text(x + 1, y0, 'dev', C.amber, { bold: true });
    if (ui.stub) cv.text(x + 2, y0, 'stub', C.yellow, { bold: true });
    drawCommit(cv, x0, y0 + 1, ui, cv.w - 2);
    return y0 + 2;
  }
  drawLogo(cv, x0, y0, N, logoAnimFor(m));
  const tx = x0 + N + 4;
  const avail = cv.w - tx - 2;
  const top = N === 24 ? y0 + 1 : y0;
  if (avail >= WORDMARK_W + 8) {
    drawWordmark(cv, tx, top);
    let x = cv.text(tx + WORDMARK_W, top + 2, ' dev ', C.ink, { bg: C.amber, bold: true });
    if (ui.stub) cv.text(x + 1, top + 2, ' stub ', C.ink, { bg: C.yellow, bold: true });
  } else {
    let x = cv.text(tx, top + 1, 'Yap', C.cream, { bold: true });
    x = cv.text(x + 1, top + 1, '·', C.faint);
    cv.text(x + 1, top + 1, 'dev', C.amber, { bold: true });
  }
  const commitY = N === 24 ? top + 5 : top + 4;
  drawCommit(cv, tx, commitY, ui, cv.w - 2);
  const waveW = Math.min(41, avail);
  if (waveW >= 9) drawWave(cv, tx, commitY + (N === 24 ? 2 : 1), waveW, ui.wave || [], waveColour(m));
  return y0 + N / 2;
}

function waveColour(m) {
  if (m.ui.mode !== 'update' && (m.state.stage === 'error' || m.state.stage === 'exited')) return m.state.stage === 'error' ? 'red' : 'dim';
  return 'amber';
}

function drawCommit(cv, x, y, ui, max) {
  if (!ui.commit) {
    cv.text(x, y, 'commit unknown', C.faint, { max });
    return;
  }
  const nx = cv.text(x, y, ui.commit.hash, C.amber, { max });
  cv.text(nx + 2, y, ui.commit.title, C.text, { max });
}

// A screen is a list of lines laid out top-down; each is
// { segs, right?, x?, bar?, kind? }.
const seg = (text, fg = C.text, bold = 0) => [text, fg, bold];

function updateLines(ui, t, { compact = false } = {}) {
  const u = ui.update;
  const L = [];
  if (ui.mode === 'update' && !ui.updateDone) {
    const what = u?.npm ? 'installing dependencies (npm install)' : u?.status === 'checking' ? 'checking GitHub for the latest version' : 'starting';
    L.push({ segs: [seg(spinner(t), C.amber), seg('  Updating from GitHub', C.cream, 1), seg(` · ${what}`, C.dim)] });
    if (u?.npm && ui.updateTail) L.push({ x: 7, segs: [seg(ui.updateTail, C.faint)] });
    return L;
  }
  if (!u) {
    L.push({ segs: [seg('●', C.faint), seg('  GitHub', C.dim), seg(' · checked before this screen opened', C.faint)] });
    return L;
  }
  const npm = u.npm ? ' · npm install ran' : '';
  if (u.status === 'updated') {
    L.push({ segs: [seg('●', C.green), seg('  Updated from GitHub', C.cream, 1), seg(` · ${plural(u.commits.length, 'new commit')}${npm}`, C.dim)] });
    if (!compact) {
      const show = u.commits.slice(0, 3);
      for (const c of show) L.push({ x: 7, segs: [seg(c.hash, C.amberDeep), seg('  ' + c.title, C.dim)] });
      if (u.commits.length > show.length) L.push({ x: 7, segs: [seg(`+${u.commits.length - show.length} more`, C.faint)] });
    }
  } else if (u.status === 'uptodate') {
    L.push({ segs: [seg('●', C.green), seg('  Up to date with GitHub', C.cream, 1), seg(npm, C.dim)] });
  } else if (u.status === 'skipped') {
    L.push({ segs: [seg('!', C.yellow, 1), seg('  Not updated', C.cream, 1), seg(` · ${u.reason || 'skipped'}${npm}`, C.yellow)] });
    if (!compact && u.files.length) {
      const show = u.files.slice(0, 3);
      L.push({ x: 7, segs: [seg(show.join('  '), C.dim), seg(u.files.length > 3 ? `  +${u.files.length - 3} more` : '', C.faint)] });
    }
  } else {
    L.push({ segs: [seg('●', C.faint), seg('  GitHub', C.dim), seg(' · no result from the update step', C.faint)] });
  }
  if (!compact) for (const n of u.notes) L.push({ x: 7, segs: [seg(n, C.yellow)] });
  return L;
}

function viteLine(m) {
  const { state, ui, t } = m;
  const v = state.vite;
  if (ui.mode === 'update') return { segs: [seg('○', C.faint), seg('  Start Vite', C.faint)] };
  if (v.done) {
    const bits = [];
    if (v.url) bits.push(v.url.replace(/^https?:\/\//, '').replace(/\/$/, ''));
    if (v.ms != null) bits.push(`${Math.round(v.ms)} ms`);
    return { segs: [seg('●', C.green), seg('  Vite ready', C.cream, 1), seg(bits.length ? ` · ${bits.join(' · ')}` : '', C.dim)] };
  }
  if (state.errors && state.errors.stage === 'vite') return { segs: [seg('×', C.red, 1), seg('  Vite failed', C.red, 1)] };
  if (v.skipped) return { segs: [seg('●', C.faint), seg('  Vite', C.dim), seg(' · no output seen', C.faint)] };
  return {
    segs: [seg(spinner(t), C.amber), seg('  Starting Vite', C.cream, 1), seg(' · the frontend dev server', C.dim)],
    right: fmtDuration(m.now - state.startedAt),
  };
}

function rustLines(m, W) {
  const { state, ui, t, now } = m;
  const b = state.build;
  const L = [];
  if (ui.mode === 'update' || (!b && state.stage === 'vite')) {
    L.push({ segs: [seg('○', C.faint), seg('  Compile Rust', C.faint)] });
    return L;
  }
  if (state.stage === 'error' && state.errors?.stage === 'compiling') {
    L.push({ segs: [seg('×', C.red, 1), seg(b?.rebuild ? '  Rebuild failed' : '  Rust build failed', C.red, 1)], right: b ? fmtDuration(state.errors.at - b.start) : null });
    return L;
  }
  if (state.stage !== 'compiling' || !b) {
    const lb = state.lastBuild;
    const took = lb ? fmtDuration(lb.ms) : '';
    const units = lb?.units ? ` · ${lb.units} crates` : '';
    L.push({ segs: [seg('●', C.green), seg('  Rust built', C.cream, 1), seg(lb ? ` · ${took}${units}` : '', C.dim)] });
    return L;
  }
  const p = state.progress(now);
  const title = b.rebuild ? '  Rebuilding Rust' : '  Compiling Rust';
  const why = b.rebuild && b.file ? ` · ${b.file} changed` : '';
  L.push({ segs: [seg(spinner(t), C.amber), seg(title, C.cream, 1), seg(why, C.dim)], right: fmtDuration(p.elapsed) });
  const barW = Math.max(10, Math.min(56, W - 30));
  const pct = p.frac == null ? '' : `${Math.floor(p.frac * 100)}%`;
  const eta = p.eta != null && p.frac != null ? `about ${fmtDuration(p.eta)} left` : '';
  L.push({ x: 5, bar: { w: barW, frac: p.frac }, segs: [seg(pct.padStart(4), C.cream, 1), seg(eta ? '  ' + eta : '', C.dim)] });
  L.push({ x: 5, segs: buildDetail(p) });
  return L;
}

function buildDetail(p) {
  const count = p.m != null ? `${p.n}/${p.m} crates` : p.compiled ? `${p.compiled} compiled` : '';
  const head = count ? [seg(count, C.text), seg(' · ', C.faint)] : [];
  if (p.waitingLock) return [...head, seg('waiting for another build to let go of the build folder', C.yellow)];
  if (p.phase === 'waiting') return [seg('waiting for cargo…', C.dim)];
  if (p.phase === 'deps') {
    const names = p.crates.length ? p.crates.join(', ') : p.lastCrate || '';
    return [...head, seg(names, C.dim)];
  }
  if (p.step) {
    const s = p.step;
    const tail = s.known
      ? s.elapsed <= s.expected
        ? ` of ~${fmtDuration(s.expected)}`
        : ` · longer than last time (${fmtDuration(s.expected)})`
      : ' · the slow last step, no timing from an earlier build yet';
    return [...head, seg(s.label, C.amber, 1), seg(` · ${fmtDuration(s.elapsed)}`, C.cream), seg(tail, C.dim)];
  }
  return head;
}

function launchLine(m) {
  const { state, ui, t, now } = m;
  if (ui.mode === 'update' || state.stage === 'vite' || state.stage === 'compiling' || (state.stage === 'error' && !state.launchAt)) {
    return { segs: [seg('○', C.faint), seg('  Launch Yap', C.faint)] };
  }
  if (state.stage === 'launching') {
    return { segs: [seg(spinner(t), C.amber), seg('  Launching Yap', C.cream, 1), seg(' · starting yap.exe', C.dim)], right: fmtDuration(now - state.launchAt) };
  }
  if (state.stage === 'error') return { segs: [seg('×', C.red, 1), seg('  Yap stopped while starting', C.red, 1)] };
  return { segs: [seg('●', C.green), seg('  Launched', C.cream, 1)] };
}

function errorLines(m, room) {
  const e = m.state.errors;
  const L = [];
  if (!e) return L;
  const lines = e.lines.slice();
  while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
  const hint = errorHint(m);
  const hintLine = hint ? { x: 5, wrap: true, segs: [seg(hint, C.yellow)] } : null;
  const avail = Math.max(1, room - 1 - (hintLine ? lineRows(hintLine, m.W ?? 80) : 0));
  let show = lines.slice(0, avail);
  const summary = lines.findLast?.((l) => /^error: could not compile/.test(l)) ?? lines.filter((l) => /^error: could not compile/.test(l)).pop();
  if (lines.length > avail && summary && !show.includes(summary)) show = [...show.slice(0, avail - 1), summary];
  L.push({ segs: [] });
  for (const l of show) L.push({ x: 5, segs: [seg(l, errorColour(l))] });
  if (hintLine) L.push(hintLine);
  return L;
}

function errorColour(l) {
  if (/^(error|thread '.*' panicked|npm (error|ERR!))/.test(l) || /\bPort \d+ is already in use/.test(l)) return C.red;
  if (/^\s*-->/.test(l)) return C.amber;
  if (/^\s*\d*\s*\|/.test(l)) return C.dim;
  if (/^warning/.test(l)) return C.yellow;
  return C.redSoft;
}

function errorHint(m) {
  const e = m.state.errors;
  if (!e) return null;
  if (e.accessDenied) {
    return "Something still has yap.exe open (an AI app's Yap helper, another Yap - Dev?). Close it, then save a Rust file to rebuild.";
  }
  if (e.port) return `Port ${e.port} is taken: is another Yap - Dev already running? Close it, then start this one again.`;
  if (e.stage === 'compiling') return 'Fix it and save: tauri dev rebuilds by itself.  L shows the whole log.';
  return 'L shows the whole log.';
}

function runningLines(m) {
  const { state, ui, t, now } = m;
  const L = [];
  if (state.stage === 'stopping') {
    L.push({ segs: [seg(spinner(t), C.amber), seg('  Stopping Yap', C.cream, 1), seg(' · ending tauri dev, Vite and yap.exe', C.dim)] });
    return L;
  }
  L.push({
    segs: [seg('●', C.green), seg('  Yap is running', C.green, 1)],
    right: `up ${fmtDuration(now - (state.runningSince ?? now))}`,
  });
  const facts = [];
  if (state.lastBuild) facts.push(`built in ${fmtDuration(state.lastBuild.ms)}`);
  if (state.vite.url) facts.push(state.vite.url.replace(/^https?:\/\//, '').replace(/\/$/, ''));
  facts.push(`this window open ${fmtDuration(now - state.startedAt)}`);
  L.push({ x: 5, segs: [seg(facts.join(' · '), C.dim)] });
  L.push({ segs: [] });
  if (!state.hot.count && !state.rustRebuilds) {
    L.push({ x: 5, segs: [seg('No reloads yet: ', C.text), seg('save in desktop/src/ and the page hot-reloads; in desktop/src-tauri/ and Yap rebuilds.', C.dim)] });
  } else {
    L.push({ x: 5, segs: [seg(plural(state.hot.count, 'hot reload'), C.cream, 1), seg(' · ', C.faint), seg(plural(state.rustRebuilds, 'Rust rebuild'), C.cream, 1)] });
    for (const r of state.hot.recent.slice(0, 4)) {
      const tag = r.kind === 'rust' ? 'Rust' : 'UI  ';
      const extra = r.kind === 'rust' && r.ms != null ? ` · rebuilt in ${fmtDuration(r.ms)}` : '';
      L.push({ x: 7, segs: [seg(clock(r.at), C.faint), seg('  ' + tag, r.kind === 'rust' ? C.amber : C.green), seg('  ' + r.what, C.text), seg(extra, C.dim)] });
    }
  }
  if (state.viteError) L.push({ x: 5, segs: [seg('Vite: ', C.red, 1), seg(state.viteError.text.trim(), C.redSoft)] });
  if (state.panic) L.push({ x: 5, segs: [seg('A panic was logged: ', C.red, 1), seg(state.panic.text.trim(), C.redSoft)] });
  const { warn, error } = state.appLog;
  if (warn || error) {
    L.push({ x: 5, segs: [seg('App log: ', C.dim), seg(plural(warn, 'warning'), warn ? C.yellow : C.dim), seg(' · ', C.faint), seg(plural(error, 'error'), error ? C.red : C.dim), seg('  (L shows them)', C.faint)] });
  }
  return L;
}

function exitedLines(m) {
  const { state, now } = m;
  const e = state.exit;
  const L = [];
  const how = e?.byUser ? 'stopped with Ctrl+C' : `exit code ${e?.code ?? '?'}`;
  L.push({ segs: [seg('■', e?.code && !e.byUser ? C.red : C.dim, 1), seg('  Yap Dev stopped', C.cream, 1), seg(` · ${how} · ran ${fmtDuration((e?.at ?? now) - state.startedAt)}`, C.dim)] });
  return L;
}

/** A line's segments word-wrapped between x0 and maxX (later rows at contX): [[text, fg, bold, x], ...] per row. */
export function wrapSegs(segs, x0, maxX, contX) {
  const rows = [[]];
  let x = x0;
  for (const [text, fg, bold] of segs) {
    for (const word of text.match(/\S+\s*|\s+/g) || []) {
      const w = strWidth(word.trimEnd());
      if (x + w > maxX && x > (rows.length === 1 ? x0 : contX)) {
        rows.push([]);
        x = contX;
        if (!word.trim()) continue;
      }
      rows[rows.length - 1].push([word, fg, bold, x]);
      x += strWidth(word);
    }
  }
  return rows;
}

function lineRows(l, W) {
  if (!l.wrap) return 1;
  const x = l.x ?? 3;
  return wrapSegs(l.segs, x, W - 3, x + 3).length;
}

function drawLines(cv, lines, y, maxY, t) {
  for (const l of lines) {
    if (y >= maxY) break;
    if (l.wrap) {
      const x0 = l.x ?? 3;
      for (const row of wrapSegs(l.segs, x0, cv.w - 3, x0 + 3)) {
        if (y >= maxY) break;
        for (const [text, fg, bold, x] of row) cv.text(x, y, text, fg, { bold, max: cv.w - 3 });
        y++;
      }
      continue;
    }
    const x = 3 + (l.x ? l.x - 3 : 0);
    let nx = x;
    if (l.bar) {
      drawBar(cv, x, y, l.bar.w, l.bar.frac, t, { error: l.bar.error });
      nx = x + l.bar.w + 1;
    }
    const rightW = l.right ? strWidth(l.right) + 2 : 0;
    cv.segs(nx, y, l.segs, cv.w - 3 - rightW);
    if (l.right) cv.text(cv.w - 3 - strWidth(l.right), y, l.right, C.dim);
    y++;
  }
  return y;
}

function drawFooter(cv, m) {
  const { ui } = m;
  const y = cv.h - 1;
  for (let x = 3; x < cv.w - 3; x++) cv.set(x, y - 1, '─', C.track, null);
  const keys = ui.mode === 'update' ? [['Ctrl+C', 'stop']] : [['L', ui.view === 'log' ? 'back' : 'log'], ['O', 'open log'], ['Ctrl+C', 'stop Yap']];
  if (ui.admin && ui.mode !== 'update') keys.splice(2, 0, ['R', 'restart as you']);
  let x = 3;
  for (const [k, label] of keys) {
    x = cv.text(x, y, k, C.amber, { bold: true });
    x = cv.text(x + 1, y, label, C.dim) + 3;
  }
  if (ui.message) {
    cv.text(x, y, ui.message, C.green, { max: cv.w - 3 });
    return;
  }
  if (ui.logShown) {
    const room = cv.w - 3 - x - 1;
    if (room > 12) {
      const p = truncateLeft(ui.logShown, room - 5);
      const px = cv.w - 3 - strWidth(p) - 5;
      cv.text(px, y, 'log ', C.faint);
      cv.text(px + 4, y, p, C.dim, { max: cv.w - 2 });
    }
  }
}

/** The animated screen. */
export function drawMain(m, W, H) {
  m = { ...m, W };
  const cv = new Canvas(W, H);
  const { state, ui } = m;
  const N = logoSize(m, W, H);
  let y = drawHeader(cv, m, N) + 1;
  const maxY = H - 3;
  const lines = [];
  if (ui.admin) {
    lines.push({
      wrap: true,
      segs: [
        seg('!', C.yellow, 1),
        seg('  Running as administrator', C.yellow, 1),
        seg(" · Windows won't let you drag files from Explorer into Yap, and pasting behaves unlike a normal install. ", C.dim),
        seg('Start Yap - Dev with a plain double-click', C.text),
        seg(', or press R.', C.dim),
      ],
    });
  }
  if (ui.stub) {
    lines.push({ wrap: true, segs: [seg('!', C.yellow, 1), seg('  Stub build', C.yellow, 1), seg(' · no Vulkan SDK, so no real transcription (placeholder text). ', C.dim), seg('winget install LunarG.VulkanSDK', C.text), seg(', then reopen this window.', C.dim)] });
  }
  const stage = state.stage;
  if (ui.mode !== 'update' && (stage === 'running' || stage === 'stopping')) {
    lines.push(...updateLines(ui, m.t, { compact: true }));
    lines.push(...runningLines(m));
  } else if (ui.mode !== 'update' && stage === 'exited') {
    lines.push(...updateLines(ui, m.t, { compact: true }));
    lines.push(...exitedLines(m));
  } else {
    lines.push(...updateLines(ui, m.t));
    lines.push(viteLine(m));
    lines.push(...rustLines(m, W));
    lines.push(launchLine(m));
    if (stage === 'error' && ui.mode !== 'update') {
      const used = lines.reduce((n, l) => n + lineRows(l, W), 0);
      lines.push(...errorLines(m, maxY - y - used));
    }
  }
  drawLines(cv, lines, y, maxY, m.t);
  drawFooter(cv, m);
  return cv;
}

const LOG_COLOURS = {
  error: C.red,
  'error-summary': C.red,
  panic: C.red,
  'npm-error': C.red,
  'tauri-error': C.red,
  'port-in-use': C.red,
  warning: C.yellow,
  'warning-summary': C.yellow,
  compiling: C.dim,
  finished: C.green,
  'run-exe': C.green,
  'vite-ready': C.green,
  rebuild: C.amber,
  'vite-hmr': C.amber,
  launcher: C.amber,
};
const LEVEL_COLOURS = { ERROR: C.red, WARN: C.yellow, INFO: C.text, DEBUG: C.faint, TRACE: C.faint };

/** The raw log: the child's own lines, newest at the bottom. */
export function drawLog(m, W, H) {
  const cv = new Canvas(W, H);
  const { ui, state } = m;
  cv.fill(0, 0, W, C.bar);
  let x = cv.text(1, 0, ' Yap ', C.ink, { bg: C.amber, bold: true });
  x = cv.text(x + 1, 0, 'raw log', C.cream, { bg: C.bar, bold: true });
  cv.text(x + 2, 0, statusWord(m), C.dim, { bg: C.bar, max: W - 44 });
  const help = 'L back · Up/Down PgUp/PgDn scroll · End follow';
  cv.text(Math.max(x + 20, W - strWidth(help) - 2), 0, help, C.dim, { bg: C.bar });
  const rows = H - 3;
  const lines = ui.logLines;
  const end = Math.max(0, lines.length - ui.logScroll);
  const start = Math.max(0, end - rows);
  for (let i = start, y = 1; i < end; i++, y++) {
    const l = lines[i];
    const fg = l.kind === 'app-log' ? LEVEL_COLOURS[l.level] || C.text : LOG_COLOURS[l.kind] || C.text;
    cv.text(1, y, l.text, fg, { max: W - 1 });
  }
  if (!lines.length) cv.text(2, 2, 'Nothing yet.', C.faint);
  for (let i = 1; i < W - 1; i++) cv.set(i, H - 2, '─', C.track, null);
  const where = ui.logScroll ? `scrolled up ${ui.logScroll} lines · End to follow` : 'following';
  let fx = cv.text(1, H - 1, where, C.dim);
  if (ui.logShown) {
    const room = W - fx - 4;
    if (room > 10) cv.text(W - 2 - Math.min(room, strWidth(ui.logShown)), H - 1, truncateLeft(ui.logShown, room), C.faint);
  }
  void state;
  return cv;
}

function statusWord(m) {
  const { state, now } = m;
  switch (state.stage) {
    case 'vite':
      return 'starting Vite';
    case 'compiling': {
      const p = state.progress(now);
      return p?.frac != null ? `compiling ${Math.floor(p.frac * 100)}%` : 'compiling';
    }
    case 'launching':
      return 'launching';
    case 'running':
      return `running · up ${fmtDuration(now - (state.runningSince ?? now))}`;
    case 'error':
      return 'stopped at an error';
    case 'stopping':
      return 'stopping';
    case 'exited':
      return 'stopped';
    default:
      return '';
  }
}

/** The window title and the taskbar's progress (OSC 9;4) for what's going on. */
export function chrome(m) {
  const { state, ui, now } = m;
  if (ui.mode === 'update') return { title: 'Yap Dev · updating', progress: [3, 0] };
  switch (state.stage) {
    case 'compiling': {
      const p = state.progress(now);
      if (p?.frac == null) return { title: 'Yap Dev · compiling', progress: [3, 0] };
      const pct = Math.floor(p.frac * 100);
      return { title: `Yap Dev · compiling ${pct}%`, progress: [1, pct] };
    }
    case 'error':
      return { title: 'Yap Dev · build failed', progress: [2, 100] };
    case 'running':
      return { title: 'Yap Dev · running', progress: [0, 0] };
    case 'exited':
      return { title: 'Yap Dev (stopped)', progress: [0, 0] };
    case 'stopping':
      return { title: 'Yap Dev · stopping', progress: [3, 0] };
    default:
      return { title: 'Yap Dev · starting', progress: [3, 0] };
  }
}

/** What's printed in the console when it's over: the mark (mouth shut) and the facts. */
export function drawSummary(m, W) {
  const { state, ui } = m;
  const N = W >= 50 ? 16 : 0;
  const facts = [];
  const e = state.exit;
  const head = e?.byUser ? 'Yap Dev stopped (Ctrl+C)' : e?.code ? `Yap Dev stopped · exit code ${e.code}` : 'Yap Dev stopped';
  facts.push([[head, e?.code && !e.byUser ? C.red : C.cream, 1]]);
  const bits = [`ran ${fmtDuration((e?.at ?? m.now) - state.startedAt)}`];
  if (state.builds) bits.push(plural(state.builds, 'build'));
  if (state.hot.count) bits.push(plural(state.hot.count, 'hot reload'));
  if (state.rustRebuilds) bits.push(plural(state.rustRebuilds, 'Rust rebuild'));
  facts.push([[bits.join(' · '), C.dim]]);
  if (ui.commit) facts.push([[ui.commit.hash, C.amber], ['  ' + ui.commit.title, C.text]]);
  if (ui.logFull) facts.push([['log  ', C.faint], [ui.logFull, C.dim]]);
  const err = state.errors?.lines?.filter((l) => l.trim()) || [];
  const tail = !err.length && e?.code && !e.byUser ? ui.logLines.slice(-12).map((l) => l.text) : [];
  const extra = [...err.slice(0, 24), ...tail];
  const H = Math.max(N / 2, facts.length) + 2 + (extra.length ? extra.length + 1 : 0);
  const cv = new Canvas(W, H);
  if (N) drawLogo(cv, 3, 1, N, QUIET);
  const tx = N ? 3 + N + 3 : 3;
  const fy = N ? 1 + Math.max(0, Math.floor((N / 2 - facts.length) / 2)) : 1;
  facts.forEach((segs, i) => cv.segs(tx, fy + i, segs, W - 2));
  let y = Math.max(N / 2, facts.length) + 2;
  for (const l of extra) cv.text(3, y++, l, err.length ? errorColour(l) : C.dim, { max: W - 2 });
  return cv;
}
