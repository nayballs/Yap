// **The sheet view's sums** (SheetTab.svelte; Rust's chat/sheets/view.rs reads the workbook): where each row
// and column is, which ones are in view, how a cell looks and how far its text runs on. No Svelte state: pure,
// so the e2e suite checks them without the app (sheet-view.spec.js).

/** The context key Chat sets: `{ open(file) }`, an attached spreadsheet's chip opening its view (ChatView). */
export const SHEETS = 'yap-sheets';

/** The endings shown as a sheet (Rust's `sheets::Format::of`). */
export const SHEET_ENDINGS = ['xlsx', 'xlsm', 'xls', 'xlsb', 'ods', 'csv', 'tsv'];

/** Whether a file called `name` (or at path `name`) is a spreadsheet. */
export function isSheetName(name) {
  const base = String(name ?? '').split(/[\\/]/).pop() || '';
  const dot = base.lastIndexOf('.');
  return dot > 0 && SHEET_ENDINGS.includes(base.slice(dot + 1).toLowerCase());
}

/** Excel's own sizes at 100%: a column, a row, the header row, a row number's column at least. */
export const COLUMN_PX = 64;
export const ROW_PX = 20;
export const HEAD_PX = 20;
export const NUMBERS_PX = 34;
/** Blank rows and columns drawn past the last one used, as Excel's grid goes on. */
export const MORE_ROWS = 40;
export const MORE_COLS = 6;
/** The most rows and columns a sheet has (Excel's). */
export const MAX_ROWS = 1_048_576;
export const MAX_COLS = 16_384;
/** Rows asked of Rust at a time. */
export const BLOCK = 200;

/** A column's letters (1 → "A", 28 → "AB"). */
export function columnName(c) {
  let n = Math.floor(Number(c) || 0);
  let out = '';
  while (n > 0) {
    const rem = (n - 1) % 26;
    out = String.fromCharCode(65 + rem) + out;
    n = Math.floor((n - 1) / 26);
  }
  return out;
}

/** A cell's address ("B7"). */
export const cellName = (r, c) => `${columnName(c)}${r}`;

/**
 * **A sheet's grid**: how many rows and columns are drawn (what's used and a margin, as Excel's goes on), each
 * one's size, and their running totals (`rowAt[r]`: row r's top, from row 1 at 0; `colAt[c]` likewise).
 * `sheet`: Rust's `SheetInfo`.
 */
export function gridOf(sheet) {
  const drawn = sheet?.drawn || {};
  let bottom = Number(drawn.bottom) || 0;
  let right = Number(drawn.right) || 0;
  for (const m of sheet?.merges || []) {
    bottom = Math.max(bottom, m.row + m.rows - 1);
    right = Math.max(right, m.col + m.cols - 1);
  }
  bottom = Math.max(bottom, Number(sheet?.frozenRows) || 0);
  right = Math.max(right, Number(sheet?.frozenCols) || 0);
  const rows = Math.min(MAX_ROWS, Math.max(bottom + MORE_ROWS, 60));
  const cols = Math.min(MAX_COLS, Math.max(right + MORE_COLS, 14));
  const rowPx = new Float32Array(rows + 1).fill(ROW_PX);
  rowPx[0] = 0;
  for (const look of sheet?.rowLooks || []) {
    if (look.r < 1 || look.r > rows) continue;
    rowPx[look.r] = look.hidden ? 0 : Number.isFinite(look.px) ? look.px : ROW_PX;
  }
  const colPx = new Float32Array(cols + 1).fill(COLUMN_PX);
  colPx[0] = 0;
  for (const span of sheet?.cols || []) {
    const to = Math.min(cols, span.to);
    for (let c = Math.max(1, span.from); c <= to; c++) colPx[c] = span.hidden ? 0 : Number.isFinite(span.px) ? span.px : COLUMN_PX;
  }
  const rowAt = sums(rowPx);
  const colAt = sums(colPx);
  const digits = String(rows).length;
  return {
    rows,
    cols,
    rowPx,
    colPx,
    rowAt,
    colAt,
    width: colAt[cols + 1],
    height: rowAt[rows + 1],
    numbers: Math.max(NUMBERS_PX, digits * 7 + 14),
    frozenRows: Math.min(rows, Number(sheet?.frozenRows) || 0),
    frozenCols: Math.min(cols, Number(sheet?.frozenCols) || 0),
  };
}

/** Running totals of `px` (index 0 unused): `at[i]` is where item i starts; `at[n + 1]` the end. */
function sums(px) {
  const n = px.length - 1;
  const at = new Float64Array(n + 2);
  for (let i = 1; i <= n; i++) at[i + 1] = at[i] + px[i];
  return at;
}

/** The item (from 1) whose span holds `y` in running totals `at` (n items), clamped to 1..n. */
export function indexAt(at, y) {
  const n = at.length - 2;
  let lo = 1;
  let hi = n;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (at[mid] <= y) lo = mid;
    else hi = mid - 1;
  }
  return Math.max(1, Math.min(n, lo));
}

/**
 * **What's in view**: the rows and columns past the frozen ones that the viewport (`w` × `h`, scrolled to
 * `left`, `top`) shows, a few more each side. `{ r0, r1, c0, c1 }`, inclusive.
 */
export function inView(grid, { top = 0, left = 0, w = 0, h = 0 } = {}, extra = 4) {
  const fh = grid.rowAt[grid.frozenRows + 1];
  const fw = grid.colAt[grid.frozenCols + 1];
  // (In the grid's own terms: its top-left at 0, under the headers.)
  const y0 = top + fh;
  const y1 = top + Math.max(0, h - HEAD_PX);
  const x0 = left + fw;
  const x1 = left + Math.max(0, w - grid.numbers);
  const r0 = Math.max(grid.frozenRows + 1, indexAt(grid.rowAt, y0) - extra);
  const r1 = Math.min(grid.rows, indexAt(grid.rowAt, Math.max(y0, y1)) + extra);
  const c0 = Math.max(grid.frozenCols + 1, indexAt(grid.colAt, x0) - 1);
  const c1 = Math.min(grid.cols, indexAt(grid.colAt, Math.max(x0, x1)) + 1);
  return { r0, r1, c0, c1 };
}

/** The blocks of rows (`BLOCK` each, from 0) that rows `r0`..`r1` are in. */
export function blocksFor(r0, r1) {
  const out = [];
  for (let b = Math.floor((r0 - 1) / BLOCK); b <= Math.floor((r1 - 1) / BLOCK); b++) out.push(b);
  return out;
}

/** A border's edge as CSS: Excel's styles as lines of 1–3 px. */
export function edgeCss(e) {
  if (!e) return '';
  const colour = e.color || 'var(--sheet-line)';
  switch (e.style) {
    case 'medium':
      return `2px solid ${colour}`;
    case 'thick':
      return `3px solid ${colour}`;
    case 'double':
      return `3px double ${colour}`;
    case 'dotted':
      return `1px dotted ${colour}`;
    case 'dashed':
      return `2px dashed ${colour}`;
    default:
      return `1px solid ${colour}`;
  }
}

/** An edge's width in pixels. */
export const edgePx = (e) => (!e ? 0 : e.style === 'medium' || e.style === 'dashed' ? 2 : e.style === 'thick' || e.style === 'double' ? 3 : 1);

const escapeFont = (name) => `"${String(name).replace(/["\\]/g, '')}"`;

/**
 * **A style's CSS** (Rust's `Style`; `book`: the workbook's font and size): its font, sizes in points as
 * pixels, colours as the file gives them. A font with no colour of its own is Excel's "Automatic": black on a
 * fill (as the file shows it), the view's ink with none (light in dark mode).
 */
export function styleCss(style, book) {
  const s = style || {};
  const parts = [];
  if (s.font && s.font !== book?.font) parts.push(`font-family:${escapeFont(s.font)},var(--sheet-font)`);
  if (s.size) parts.push(`font-size:${((s.size / 10) * 4) / 3}px`);
  if (s.bold) parts.push('font-weight:700');
  if (s.italic) parts.push('font-style:italic');
  const lines = [s.underline && 'underline', s.strike && 'line-through'].filter(Boolean);
  if (lines.length) parts.push(`text-decoration:${lines.join(' ')}`);
  if (s.fill) parts.push(`background:${s.fill}`);
  parts.push(`color:${s.color || (s.fill ? '#000000' : 'var(--sheet-ink)')}`);
  return parts.join(';');
}

/** A style's borders as CSS for the cell's border box (`.sb`, drawn over its left and top neighbours' last pixels). */
export function bordersCss(style) {
  const s = style || {};
  if (!s.top && !s.right && !s.bottom && !s.left) return '';
  const parts = [`left:${-edgePx(s.left)}px`, `top:${-edgePx(s.top)}px`];
  for (const side of ['top', 'right', 'bottom', 'left']) if (s[side]) parts.push(`border-${side}:${edgeCss(s[side])}`);
  return parts.join(';');
}

/** How a cell's text lines up across: the style's, else by its kind (numbers right, true/false and errors centred). */
export function alignOf(cell, style) {
  const h = style?.h;
  if (h === 'center' || h === 'right' || h === 'justify') return h;
  if (h === 'left' || h === 'fill') return 'left';
  if (cell?.k === 'number') return 'right';
  if (cell?.k === 'bool' || cell?.k === 'error') return 'center';
  return 'left';
}

/**
 * **How many columns a cell's text runs on into** (Excel's overflow): a left-aligned text that doesn't wrap
 * runs into the empty columns after it, up to the next one with something in it, a merge, or 20 columns.
 * `cells`: its row's, left to right; `covered(c)`: column c is in a merge.
 */
export function spillOf(cell, style, cells, covered = () => false) {
  if (cell?.k !== 'text' || !cell.t || style?.wrap || alignOf(cell, style) !== 'left') return 0;
  let next = Infinity;
  for (const x of cells || []) {
    if (x.c > cell.c && x.k !== 'empty' && (x.t || x.k !== 'text')) {
      next = x.c;
      break;
    }
  }
  let n = 0;
  for (let c = cell.c + 1; c < next && n < 20; c++) {
    if (covered(c)) break;
    n++;
  }
  return n;
}

/**
 * **The columns a text runs on into**, of the `free` ones after column `c`: as many as it needs (`need` px wide,
 * its cell `colPx[c]`), as Excel's text stops where it ends.
 */
export function spillNeeded(c, free, need, colPx) {
  let room = colPx[c] ?? 0;
  let n = 0;
  while (n < free && room < need) {
    n++;
    room += colPx[c + n] ?? 0;
  }
  return n;
}

/**
 * **A number as its cell shows it**: itself when it fits (`need` px, the cell `w` px), else Excel's `#` across
 * the cell (`hash` px each) — a number cut short would read as another.
 */
export function shownNumber(text, need, w, hash) {
  if (need <= w - 3) return text;
  return '#'.repeat(Math.max(1, Math.floor((w - 4) / Math.max(1, hash))));
}

/** The CSS font of a cell's text (for measuring it): `book`'s font and size unless its style has its own. */
export function fontOf(style, book) {
  const s = style || {};
  const px = s.size ? ((s.size / 10) * 4) / 3 : ((Number(book?.size) || 11) * 4) / 3;
  const family = String(s.font || book?.font || 'Calibri').replace(/["\\]/g, '');
  return `${s.italic ? 'italic ' : ''}${s.bold ? '700 ' : ''}${px}px "${family}", Aptos, "Segoe UI", Arial, sans-serif`;
}

/** The merge whose area holds cell (`r`, `c`), if any. */
export function mergeAt(merges, r, c) {
  for (const m of merges || []) if (r >= m.row && r < m.row + m.rows && c >= m.col && c < m.col + m.cols) return m;
  return null;
}

/** The view's facts line: the sheet's used range, its formulas and how they were worked out, rows left out. */
export function factsOf(book, sheet) {
  if (!book || !sheet) return '';
  const parts = [];
  const used = sheet.used;
  if (used?.top) {
    const a1 = `${cellName(used.top, used.left)}${used.top === used.bottom && used.left === used.right ? '' : `:${cellName(used.bottom, used.right)}`}`;
    parts.push(a1);
  } else parts.push('Empty');
  if (sheet.formulas) parts.push(`${sheet.formulas.toLocaleString('en-GB')} ${sheet.formulas === 1 ? 'formula' : 'formulas'}${book.calculated ? ', calculated by Yap' : ''}`);
  if (sheet.cutRows) parts.push(`its last ${sheet.cutRows.toLocaleString('en-GB')} rows not shown`);
  const hidden = (book.sheets || []).filter((s) => s.hidden).length;
  if (hidden) parts.push(`${hidden} hidden ${hidden === 1 ? 'sheet' : 'sheets'}`);
  if (book.format === 'xlsx' && !book.calculated) parts.push('values only');
  return parts.join(' · ');
}
