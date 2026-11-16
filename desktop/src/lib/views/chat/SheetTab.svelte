<script>
  // **A spreadsheet beside the chat** (15 Nov 2026): a workbook drawn as Excel draws it — its sheet tabs, column
  // letters and row numbers, widths and heights, fonts, colours, fills and borders, number and date formats,
  // merged cells, frozen panes and its formulas' values — read only, live while a Code chat's Claude builds it.
  // Rust reads it (chat/sheets/view.rs: an .xlsx calculated by IronCalc; .xls/.ods by calamine; .csv/.tsv), only
  // a file in the chat's own workspace or an attached file's kept copy.
  //
  // - **Big sheets scroll smoothly**: only the rows and columns in view are drawn (sheetView.js `inView`), their
  //   rows asked of Rust a block at a time (`sheet_rows`). Frozen rows and columns, and the headers, are sticky
  //   layers over the cells, so the browser scrolls it all itself.
  // - **Live**: a workspace's file is checked every 1.2 s (`sheet_stamp`: its size and time, nothing read);
  //   changed, it's opened again, keeping its sheet, its place and the cell selected. A file caught
  //   half-written keeps what's shown and is tried again when it changes.
  // - **The formula bar**: the cell selected (a click, the arrow keys), its formula, else its value.
  // - **Editing** (16 Nov 2026; a Code chat's .xlsx or .csv, chat/sheets/edit.rs): typed in the cell (a key, F2,
  //   a double-click) or in the formula bar; Enter, Tab and the arrows as in Excel (Enter down, Shift+Enter up,
  //   Tab right, Shift+Tab left; an arrow ends what's typed and moves, unless F2 or the bar put it in its edit mode,
  //   where it moves the caret); Esc lets it go; Delete empties the cell. Calculated on this PC by IronCalc, each
  //   change answered with the workbook again. Ctrl+Z / Ctrl+Y, and the header's ↶ ↷. Alt+Enter: a new line inside
  //   the cell (its text wrapped, as Excel does); typed input read in the person's Region (16/11/2026 a date in the UK).
  // - **Rows and columns** (16 Nov 2026): a row number's or a column letter's right-click menu inserts one above or
  //   below (left or right) or deletes it, every reference moved as Excel moves it (chat/sheets/shift.rs), or refused
  //   with why; Undo puts it back.
  // - **Saved** 700 ms after the last change (Ctrl+S at once; and on the way out), as Yap's Markdown files are —
  //   only while the file is what was read (chat/sheets/save.rs). Changed on disk with changes not saved here: never
  //   read over; a bar says so, with Reload and Save as a copy.
  // - **Read only**: an attached copy, an .xls or .ods, a file Yap can't save into (`book.edit.note`, said when you
  //   try); and **while Claude works** in the chat's folder: live, its changes shown as they land, nothing typed.
  // - **Open in Excel**: the file in the person's own spreadsheet app (Windows' default), on that click only.
  // - **Colours**: the file's own, as it gives them; a cell with no fill is the panel's paper and Excel's
  //   "Automatic" text is its ink, so a sheet reads in dark mode too.
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy, tick, untrack } from 'svelte';
  import { copyText } from '../../core/itemMenus.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { windowShown } from '../../stores/windowShown.svelte.js';
  import { sheetKey } from './chatDoc.svelte.js';
  import { BLOCK, HEAD_PX, alignOf, blocksFor, bordersCss, cellName, columnName, factsOf, fontOf, gridOf, inView, indexAt, mergeAt, shownNumber, spillNeeded, spillOf, styleCss } from './sheetView.js';

  /** `doc`: the panel's state (`doc.sheet`: `{src, name, seq}`); `conversationId`: the chat. */
  let { doc, narrow = false, conversationId = null } = $props();

  const LIVE_MS = 1200;

  const ref = $derived(doc.sheet);
  let book = $state.raw(null);
  let error = $state('');
  let loading = $state(false);
  let current = $state(0);
  // The rows read (row → its cells), the blocks asked for, and a count that moves when rows arrive.
  let rowsVer = $state(0);
  let rows = new Map();
  let blocks = new Map();
  let scroller = $state(null);
  let view = $state.raw({ top: 0, left: 0, w: 0, h: 0 });
  let win = $state.raw({ r0: 1, r1: 1, c0: 1, c1: 1 });
  let sel = $state.raw({ r: 1, c: 1 });
  let updated = $state(0);
  let stale = $state(false);
  let opening = 0;
  let openedKey = '';
  let failed = null;
  let polling = false;
  // Each sheet's place, by its name (switching back finds it where it was).
  const places = new Map();

  const sheet = $derived(book?.sheets?.[current] ?? null);
  const grid = $derived(sheet ? gridOf(sheet) : null);
  const looks = $derived(book ? book.styles.map((s) => ({ style: s, css: styleCss(s, book), borders: bordersCss(s), font: fontOf(s, book) })) : []);

  // A text's width as drawn (for how far it runs on into the next columns): measured once per text and font.
  let measurer = null;
  const widths = new Map();
  function textWidth(text, font) {
    const key = `${font}|${text}`;
    let w = widths.get(key);
    if (w != null) return w;
    measurer ??= document.createElement('canvas').getContext('2d');
    measurer.font = font;
    w = measurer.measureText(text).width + 7;
    if (widths.size > 5000) widths.clear();
    widths.set(key, w);
    return w;
  }
  const fontCss = $derived(book ? `--sheet-font-own:"${String(book.font || 'Calibri').replace(/["\\]/g, '')}";--sheet-size:${((Number(book.size) || 11) * 4) / 3}px` : '');
  const name = $derived(book?.name || ref?.name || String(ref?.src?.rel || '').split('/').pop() || 'Spreadsheet');
  const folder = $derived(ref?.src?.from === 'workspace' ? String(ref.src.rel || '').split('/').slice(0, -1).join('/') : '');
  const live = $derived(ref?.src?.from === 'workspace');
  const facts = $derived(factsOf(book, sheet));
  const appLabel = $derived(book?.format === 'ods' ? 'Open in app' : 'Open in Excel');

  // ---- editing (chat/sheets/edit.rs) --------------------------------------------------------------

  const SAVE_MS = 700;
  // What's being typed: the cell, its words, 'enter' (an arrow ends it) or 'edit' (an arrow moves the caret),
  // and where (the cell or the formula bar).
  let editing = $state(null);
  let editEl = $state(null);
  let barEl = $state(null);
  // Why typing didn't take (said for a moment under the formula bar), and a change Rust refused.
  let hint = $state('');
  let hintTimer = 0;
  // '' | 'saving' | 'saved' | 'unsaved'; a save that failed, said with Try again.
  let saveState = $state('');
  let saveError = $state('');
  let saveTimer = 0;
  let saveChain = Promise.resolve();
  let saving = false;
  // The file's stamp after our own last save (an edit's answer from before it never puts back an older one).
  let savedStamp = null;
  // Claude working in the chat's folder now (sheet_stamp), and the file changed on disk under changes not saved.
  let working = $state(false);
  let conflict = $state(false);
  let busy = $state(false);

  const note = $derived(book?.edit?.note || '');
  const editable = $derived(!!book && !note && !working && !conflict);
  const canUndo = $derived(!!book?.edit?.canUndo && editable);
  const canRedo = $derived(!!book?.edit?.canRedo && editable);

  /** Why the sheet can't take what's typed now, if it can't. */
  const whyNot = () => (note ? note : working ? "Claude is working in this chat's folder: the sheet is read only until it's done." : conflict ? `${name} changed on disk: Reload, or save your changes as a copy.` : '');

  function say(text) {
    hint = text;
    clearTimeout(hintTimer);
    if (text) hintTimer = setTimeout(() => (hint = ''), 6000);
  }

  /** What a cell's editing starts from: its formula, its value as typed, else as shown. */
  const contentOf = (cell) => (cell ? (cell.f ?? cell.i ?? cell.t ?? '') : '');

  /** Start typing in the cell selected: `text` (none: what it holds), in `mode`, in the cell or the formula bar. */
  async function startEdit(mode, text = null, where = 'cell') {
    if (!editable) {
      say(whyNot());
      return false;
    }
    if (!grid) return false;
    const r = selMerge?.row ?? sel.r;
    const c = selMerge?.col ?? sel.c;
    editing = { r, c, text: text ?? contentOf(selCell), was: contentOf(selCell), mode, where };
    say('');
    await tick();
    const el = where === 'bar' ? barEl : editEl;
    if (el && document.activeElement !== el) {
      el.focus();
      const end = el.value.length;
      el.setSelectionRange?.(end, end);
    }
    return true;
  }

  /** Let what's typed go. */
  function cancelEdit() {
    editing = null;
    scroller?.focus();
  }

  /** The selection moved by `dr` rows and `dc` columns, past hidden ones (a merge from its far side). */
  function moveBy(dr, dc) {
    if (!grid) return;
    const m = selMerge;
    let { r, c } = sel;
    if (dr > 0 && m) r = m.row + m.rows - 1;
    if (dc > 0 && m) c = m.col + m.cols - 1;
    let nr = r + dr;
    while (dr && nr >= 1 && nr <= grid.rows && grid.rowPx[nr] <= 0) nr += Math.sign(dr);
    let nc = c + dc;
    while (dc && nc >= 1 && nc <= grid.cols && grid.colPx[nc] <= 0) nc += Math.sign(dc);
    select(dr ? nr : sel.r, dc ? nc : sel.c);
  }

  /** **What's typed, put in its cell** (when it changed), then the selection moved (`move`: [dr, dc]). */
  async function commit(move = null) {
    const ed = editing;
    if (!ed) return;
    editing = null;
    // The selection moves at once (typing on doesn't wait for the sum); the change goes in behind it.
    if (move) moveBy(move[0], move[1]);
    scroller?.focus();
    if (ed.text === ed.was) return;
    const id = book?.id;
    const ok = await apply(() => invoke('sheet_edit', { id, sheet: current, row: ed.r, col: ed.c, input: ed.text }), { retry: true });
    if (!ok && book?.id === id && !editing) {
      // Nothing typed is lost: back in its box, with why.
      select(ed.r, ed.c);
      editing = { ...ed, where: 'cell' };
      await tick();
      editEl?.focus();
    }
  }

  /** Empty the cell selected (Delete). */
  async function clearSel() {
    if (!editable) return say(whyNot());
    if (!contentOf(selCell)) return;
    const r = selMerge?.row ?? sel.r;
    const c = selMerge?.col ?? sel.c;
    await apply(() => invoke('sheet_edit', { id: book.id, sheet: current, row: r, col: c, input: '' }), { retry: true });
  }

  // Changes go to Rust one after another, in the order they were made.
  let applyChain = Promise.resolve(true);

  /** **A change made** (`call`: Rust's answer, the workbook again): its rows read, then shown; then saved soon. */
  function apply(call, opts = {}) {
    const next = applyChain.then(() => applyNow(call, opts));
    applyChain = next.catch(() => false);
    return next;
  }

  async function applyNow(call, { retry = false } = {}) {
    busy = true;
    // (Saved after this change, not before it.)
    const waiting = !!saveTimer;
    clearTimeout(saveTimer);
    saveTimer = waiting ? -1 : 0;
    try {
      let res;
      try {
        res = await call();
      } catch (e) {
        const msg = String(e?.message ?? e);
        // The file changed since it was shown (nothing typed into it yet): read again, then the change once more.
        if (retry && (msg === 'changed' || msg === 'gone')) {
          await open({ keep: true });
          res = await call();
        } else throw e;
      }
      await showEdited(res.book);
      if (res.at) {
        if (res.at[0] !== current) await toSheet(res.at[0]);
        select(res.at[1], res.at[2]);
      }
      return true;
    } catch (e) {
      const msg = String(e?.message ?? e);
      if (msg !== 'changed' && msg !== 'gone') say(msg);
      // Read only now (a file Yap can't save into, found at the first change): said with the workbook.
      if (book && /read only here/.test(msg)) book = { ...book, edit: { ...(book.edit || {}), note: msg } };
      return false;
    } finally {
      busy = false;
      scheduleSave();
    }
  }

  /** The workbook as Rust has it after a change: the rows in view read first, so nothing flashes empty. */
  async function showEdited(b) {
    const ahead = await rowsAhead(b, current).catch(() => null);
    // (A save of ours that landed meanwhile has the file's newer stamp: never put back an older one.)
    const mine = savedStamp && book?.id === b.id && savedStamp.ms >= (b.stamp?.ms ?? 0) ? savedStamp : b.stamp;
    book = { ...b, stamp: mine };
    if (ahead) {
      rows = ahead.into;
      blocks = ahead.read;
    } else {
      rows = new Map();
      blocks = new Map();
    }
    rowsVer += 1;
    await tick();
    measure();
  }

  const undo = () => canUndo && apply(() => invoke('sheet_undo', { id: book.id }));
  const redo = () => canRedo && apply(() => invoke('sheet_redo', { id: book.id }));

  // ---- saving (chat/sheets/save.rs) ---------------------------------------------------------------

  function scheduleSave() {
    clearTimeout(saveTimer);
    saveTimer = 0;
    if (!book?.edit?.dirty) {
      // Undone back to what the file has: nothing to save, and it says so.
      if (saveState === 'unsaved' && !saveError && !conflict && !saving) saveState = 'saved';
      return;
    }
    saveState = 'unsaved';
    saveTimer = setTimeout(() => saveNow(), SAVE_MS);
  }

  /** **Save now** (after any save on its way): only while the file is what was read. */
  function saveNow() {
    clearTimeout(saveTimer);
    saveTimer = 0;
    const id = book?.id;
    if (id == null) return saveChain;
    saveChain = saveChain.then(async () => {
      if (book?.id !== id || !book?.edit?.dirty || conflict) return;
      saving = true;
      saveState = 'saving';
      try {
        const res = await invoke('sheet_save', { id });
        if (book?.id !== id) return;
        if (res.conflict) {
          conflict = true;
          saveState = 'unsaved';
        } else {
          saveError = '';
          saveState = res.edit?.dirty ? 'unsaved' : 'saved';
        }
        if (res.stamp && res.saved) savedStamp = res.stamp;
        book = { ...book, stamp: res.stamp || book.stamp, edit: { ...res.edit, note: book.edit?.note } };
      } catch (e) {
        saveError = String(e?.message ?? e);
        saveState = 'unsaved';
      } finally {
        saving = false;
      }
    });
    return saveChain;
  }

  /** Changed on disk: let the changes here go and read the file again. */
  async function reload() {
    if (!book) return;
    await invoke('sheet_discard', { id: book.id }).catch(() => {});
    conflict = false;
    saveState = '';
    saveError = '';
    await open({ keep: true });
  }

  /** Changed on disk: the changes here in a copy beside it (the copy shown next). */
  async function saveCopy() {
    if (!book) return;
    try {
      const copy = await invoke('sheet_save_copy', { id: book.id });
      conflict = false;
      saveState = '';
      toast({ title: `Saved as ${copy}`, description: `${name} is left as it is on disk.` });
      const src = ref?.src;
      if (src?.from === 'workspace') {
        const rel = folder ? `${folder}/${copy}` : copy;
        doc.showSheet({ src: { from: 'workspace', conversation: src.conversation, rel }, name: copy });
      }
    } catch (e) {
      toast({ title: "Couldn't save a copy", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  // The bar's text and the cell's are one: typed in either.
  function onEditInput(e) {
    if (editing) editing = { ...editing, text: e.currentTarget.value };
  }

  /** Keys while typing in the cell (or the formula bar): Excel's. */
  function onEditKey(e) {
    if (!editing) return;
    e.stopPropagation();
    const k = e.key;
    if (k === 'Escape') {
      e.preventDefault();
      cancelEdit();
    } else if (k === 'Enter' && e.altKey && !e.ctrlKey && !e.metaKey) {
      // Alt+Enter: a new line inside the cell (Excel's).
      e.preventDefault();
      const el = e.currentTarget;
      el.setRangeText('\n', el.selectionStart, el.selectionEnd, 'end');
      editing = { ...editing, text: el.value };
    } else if (k === 'Enter' && !e.altKey) {
      e.preventDefault();
      commit([e.shiftKey ? -1 : 1, 0]);
    } else if (k === 'Tab') {
      e.preventDefault();
      commit([0, e.shiftKey ? -1 : 1]);
    } else if (k === 'F2') {
      e.preventDefault();
      editing = { ...editing, mode: editing.mode === 'enter' ? 'edit' : 'enter' };
    } else if (editing.mode === 'enter' && !e.shiftKey && !e.ctrlKey && !e.altKey && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(k)) {
      e.preventDefault();
      commit(k === 'ArrowUp' ? [-1, 0] : k === 'ArrowDown' ? [1, 0] : k === 'ArrowLeft' ? [0, -1] : [0, 1]);
    } else if ((e.ctrlKey || e.metaKey) && !e.altKey && (k === 's' || k === 'S')) {
      // Ctrl+S while typing: what's typed goes in, then saved.
      e.preventDefault();
      commit().then(() => saveNow());
    }
  }

  /** The formula bar taken: typing there, in its edit mode. */
  function onBarFocus() {
    if (editing) return;
    if (!editable) {
      if (book) say(whyNot());
      return;
    }
    startEdit('edit', null, 'bar');
  }

  /** Typing left for somewhere else (the chat's box, a click outside): what's typed goes in. */
  function onEditBlur(e) {
    if (!editing) return;
    const to = e.relatedTarget;
    if (to && (to === editEl || to === barEl)) {
      editing = { ...editing, where: to === barEl ? 'bar' : 'cell' };
      return;
    }
    commit();
  }

  // ---- reading it ---------------------------------------------------------------------------------

  function firstShown(b) {
    const i = (b?.sheets || []).findIndex((s) => !s.hidden);
    return i < 0 ? 0 : i;
  }

  function forgetRows() {
    rows = new Map();
    blocks = new Map();
    rowsVer += 1;
  }

  /** Rows Rust sent for the block from row `from` into `into`: a merge's top-left cell from above added to its row, never over a row read whole. */
  function takeRows(into, got, from) {
    for (const row of got || []) {
      if (row.r < from && into.has(row.r)) {
        const have = into.get(row.r);
        for (const c of row.cells) if (!have.some((x) => x.c === c.c)) have.push(c);
        have.sort((x, y) => x.c - y.c);
      } else into.set(row.r, row.cells);
    }
  }

  /** The rows in view (and the frozen ones) of workbook `b`'s sheet `si`, read before it's shown: a live change never flashes empty. */
  async function rowsAhead(b, si) {
    const want = new Set(blocksFor(win.r0, win.r1));
    const frozen = Number(b.sheets[si]?.frozenRows) || 0;
    if (frozen) for (const x of blocksFor(1, frozen)) want.add(x);
    const into = new Map();
    const read = new Map();
    await Promise.all(
      [...want].map(async (blk) => {
        const from = blk * BLOCK + 1;
        takeRows(into, await invoke('sheet_rows', { id: b.id, sheet: si, from, to: from + BLOCK - 1 }), from);
        read.set(blk, 'read');
      }),
    );
    return { into, read };
  }

  /** Open it (again): `keep` its sheet, place and selection (the same file changed, or asked again). */
  async function open({ keep = false, livePass = false } = {}) {
    const src = ref?.src;
    if (!src) return;
    const mine = ++opening;
    loading = true;
    try {
      const b = await invoke('sheet_open', { src });
      if (mine !== opening) return;
      const was = book?.sheets?.[current]?.name;
      let i = keep && was != null ? b.sheets.findIndex((s) => s.name === was) : -1;
      if (i < 0) i = firstShown(b);
      const ahead = keep && book ? await rowsAhead(b, i).catch(() => null) : null;
      if (mine !== opening) return;
      book = b;
      savedStamp = null;
      error = '';
      failed = null;
      stale = false;
      current = i;
      // Where its editing stands: changes not saved come back with it (saved soon), and a file changed under them says so.
      working = !!b.edit?.working;
      conflict = !!b.edit?.conflict;
      if (b.edit?.dirty) {
        saveState = 'unsaved';
        if (!conflict) scheduleSave();
      } else if (!keep) saveState = '';
      if (ahead) {
        rows = ahead.into;
        blocks = ahead.read;
        rowsVer += 1;
      } else forgetRows();
      if (livePass) updated = Date.now();
      if (!keep) {
        places.clear();
        sel = { r: 1, c: 1 };
        await tick();
        if (scroller) {
          scroller.scrollTop = 0;
          scroller.scrollLeft = 0;
        }
      }
      await tick();
      measure();
    } catch (e) {
      if (mine !== opening) return;
      // Changed while shown and not readable now (half-written): what's shown stays, tried again when it changes.
      if (keep && book) {
        stale = true;
        return;
      }
      book = null;
      error = String(e?.message ?? e);
    } finally {
      if (mine === opening) loading = false;
    }
  }

  // Opened (again: `seq`), another file: read it. The same file asked again keeps its place.
  $effect(() => {
    void ref?.seq;
    const key = ref?.src ? sheetKey(ref.src) : '';
    untrack(() => {
      const same = key === openedKey && !!book;
      // Another file: changes waiting to be saved in this one go first.
      if (!same && saveTimer) saveNow();
      if (!same) {
        editing = null;
        conflict = false;
        saveError = '';
      }
      openedKey = key;
      open({ keep: same });
    });
  });

  /** The rows in view asked of Rust, a block at a time (the frozen ones too). */
  function ensureRows() {
    if (!book || !grid) return;
    const want = new Set(blocksFor(win.r0, win.r1));
    if (grid.frozenRows) for (const b of blocksFor(1, grid.frozenRows)) want.add(b);
    for (const b of want) if (!blocks.has(b)) fetchBlock(b);
  }

  async function fetchBlock(b) {
    const id = book.id;
    const si = current;
    blocks.set(b, 'asked');
    const from = b * BLOCK + 1;
    try {
      const got = await invoke('sheet_rows', { id, sheet: si, from, to: from + BLOCK - 1 });
      if (book?.id !== id || current !== si) return;
      takeRows(rows, got, from);
      blocks.set(b, 'read');
      rowsVer += 1;
    } catch (e) {
      blocks.delete(b);
      if (String(e?.message ?? e) === 'gone') open({ keep: true });
    }
  }

  // ---- what's in view -----------------------------------------------------------------------------

  /** The viewport's size and place; the rows and columns in view kept only when they change. */
  function measure() {
    if (!scroller || !grid) return;
    view = { top: scroller.scrollTop, left: scroller.scrollLeft, w: scroller.clientWidth, h: scroller.clientHeight };
    const v = inView(grid, view, 8);
    if (v.r0 !== win.r0 || v.r1 !== win.r1 || v.c0 !== win.c0 || v.c1 !== win.c1) win = v;
    ensureRows();
  }

  let frame = 0;
  function onScroll() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      measure();
    });
  }

  $effect(() => {
    if (!scroller) return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(scroller);
    return () => ro.disconnect();
  });

  // Another sheet, or the grid changed: what's in view again.
  $effect(() => {
    void grid;
    untrack(() => tick().then(measure));
  });

  /** Which layer a cell is drawn in: frozen rows on top, frozen columns at the left, both in the corner. */
  const layerOf = (r, c) => (r <= (grid?.frozenRows || 0) ? (c <= (grid?.frozenCols || 0) ? 'corner' : 'top') : c <= (grid?.frozenCols || 0) ? 'left' : 'body');

  /** **What's drawn now**: each layer's cells, grid lines and headers. */
  const drawn = $derived.by(() => {
    void rowsVer;
    if (!grid || !sheet) return null;
    const g = grid;
    const fr = g.frozenRows;
    const fc = g.frozenCols;
    const rowsOf = { body: [win.r0, win.r1], top: [1, fr], left: [win.r0, win.r1], corner: [1, fr] };
    const colsOf = { body: [win.c0, win.c1], top: [win.c0, win.c1], left: [1, fc], corner: [1, fc] };
    const out = { body: [], top: [], left: [], corner: [] };
    const x = (c) => g.numbers + g.colAt[c];
    const y = (r) => HEAD_PX + g.rowAt[r];
    const span = (c0, c1) => g.colAt[Math.min(g.cols, c1) + 1] - g.colAt[c0];
    const tall = (r0, r1) => g.rowAt[Math.min(g.rows, r1) + 1] - g.rowAt[r0];
    // Merges in view: drawn from their top-left cell; the cells under them never.
    const covered = new Set();
    const merges = [];
    for (const m of sheet.merges || []) {
      const layer = layerOf(m.row, m.col);
      const [ra, rb] = rowsOf[layer];
      const [ca, cb] = colsOf[layer];
      if (m.row > rb || m.row + m.rows - 1 < ra || m.col > cb || m.col + m.cols - 1 < ca) continue;
      merges.push({ m, layer });
      for (let r = m.row; r < m.row + m.rows; r++) for (let c = m.col; c < m.col + m.cols; c++) covered.add(`${r}:${c}`);
    }
    const item = (layer, r, c, cell, w, h, spillW) => {
      if (w <= 0 || h <= 0) return;
      const look = looks[cell.s || 0] || looks[0];
      // A number that doesn't fit its cell: Excel's ####, never a number cut short.
      let text = cell.t || '';
      if (cell.k === 'number' && text && !look?.style?.wrap) text = shownNumber(text, textWidth(text, look?.font || '') - 7, w, textWidth('#', look?.font || '') - 7);
      out[layer].push({
        key: `${r}:${c}`,
        r,
        c,
        x: x(c),
        y: y(r),
        w,
        h,
        spill: spillW,
        text,
        css: look?.css || '',
        borders: look?.borders || '',
        align: alignOf(cell, look?.style),
        v: look?.style?.v || 'bottom',
        wrap: !!look?.style?.wrap,
        filled: !!look?.style?.fill,
      });
    };
    for (const { m, layer } of merges) {
      const cell = rows.get(m.row)?.find((x2) => x2.c === m.col) || { c: m.col, k: 'empty' };
      item(layer, m.row, m.col, cell, span(m.col, m.col + m.cols - 1), tall(m.row, m.row + m.rows - 1), 0);
    }
    for (const layer of ['body', 'top', 'left', 'corner']) {
      const [ra, rb] = rowsOf[layer];
      const [ca, cb] = colsOf[layer];
      if (rb < ra || cb < ca) continue;
      for (let r = ra; r <= rb; r++) {
        const cells = rows.get(r);
        if (!cells || g.rowPx[r] <= 0) continue;
        for (const cell of cells) {
          if (cell.c > cb) break;
          if (covered.has(`${r}:${cell.c}`)) continue;
          const look = looks[cell.s || 0];
          const free = spillOf(cell, look?.style, cells, (c) => covered.has(`${r}:${c}`));
          const spill = free ? spillNeeded(cell.c, free, textWidth(cell.t, look?.font || ''), g.colPx) : 0;
          if (cell.c + spill < ca) continue;
          if (cell.k === 'empty' && !cell.s) continue;
          const w = g.colPx[cell.c];
          item(layer, r, cell.c, cell, w, g.rowPx[r], spill ? span(cell.c, cell.c + spill) : 0);
        }
      }
    }
    // Grid lines (Excel's View → Gridlines): a line under each row and right of each column in view.
    const lines = { body: [], top: [], left: [], corner: [] };
    if (sheet.grid) {
      for (const layer of ['body', 'top', 'left', 'corner']) {
        const [ra, rb] = rowsOf[layer];
        const [ca, cb] = colsOf[layer];
        if (rb < ra || cb < ca) continue;
        const left = x(ca);
        const width = span(ca, cb);
        const top = y(ra);
        const height = tall(ra, rb);
        for (let r = ra; r <= rb; r++) if (g.rowPx[r] > 0) lines[layer].push({ key: `h${r}`, x: left, y: y(r + 1) - 1, w: width, h: 1 });
        for (let c = ca; c <= cb; c++) if (g.colPx[c] > 0) lines[layer].push({ key: `v${c}`, x: x(c + 1) - 1, y: top, w: 1, h: height });
      }
    }
    // The headers: letters across (the frozen columns' in the corner), numbers down.
    const heads = { top: [], left: [], corner: [] };
    const colHead = (layer, c) => g.colPx[c] > 0 && heads[layer].push({ key: `c${c}`, x: x(c), y: 0, w: g.colPx[c], h: HEAD_PX, text: columnName(c), on: sel.c === c, col: true });
    const rowHead = (layer, r) => g.rowPx[r] > 0 && heads[layer].push({ key: `r${r}`, x: 0, y: y(r), w: g.numbers, h: g.rowPx[r], text: String(r), on: sel.r === r, col: false });
    for (let c = win.c0; c <= win.c1; c++) colHead('top', c);
    for (let c = 1; c <= fc; c++) colHead('corner', c);
    for (let r = win.r0; r <= win.r1; r++) rowHead('left', r);
    for (let r = 1; r <= fr; r++) rowHead('corner', r);
    return { cells: out, lines, heads, fw: g.colAt[fc + 1], fh: g.rowAt[fr + 1] };
  });

  // ---- the cell selected --------------------------------------------------------------------------

  const selMerge = $derived(sheet ? mergeAt(sheet.merges, sel.r, sel.c) : null);
  const selCell = $derived.by(() => {
    void rowsVer;
    return rows.get(sel.r)?.find((x) => x.c === sel.c) ?? null;
  });
  const selBox = $derived.by(() => {
    if (!grid) return null;
    const m = selMerge;
    const r0 = m ? m.row : sel.r;
    const c0 = m ? m.col : sel.c;
    const r1 = m ? m.row + m.rows - 1 : sel.r;
    const c1 = m ? m.col + m.cols - 1 : sel.c;
    return {
      layer: layerOf(r0, c0),
      x: grid.numbers + grid.colAt[c0],
      y: HEAD_PX + grid.rowAt[r0],
      w: grid.colAt[Math.min(grid.cols, c1) + 1] - grid.colAt[c0],
      h: grid.rowAt[Math.min(grid.rows, r1) + 1] - grid.rowAt[r0],
    };
  });
  // The formula bar: what's being typed, else the cell's formula, its value as typed, or as shown (Excel's).
  const barText = $derived(editing ? editing.text : contentOf(selCell));
  // The box typed in over the cell: where the selection is drawn, as wide as its words need.
  const editBox = $derived.by(() => {
    if (!editing || !selBox) return null;
    const look = looks[selCell?.s || 0] || looks[0];
    // As wide as its longest line needs, and as tall as its lines (Alt+Enter's).
    const parts = String(editing.text || '').split('\n');
    const need = Math.max(...parts.map((t) => textWidth(t, look?.font || ''))) + 8;
    const lineH = Math.round(((Number(book?.size) || 11) * 4) / 3 * 1.2);
    const h = Math.max(selBox.h, parts.length * lineH + 6);
    return { ...selBox, w: Math.max(selBox.w, Math.min(need, 640)), h: Math.min(h, 400), css: look?.css || '' };
  });
  // The formula bar: one line, up to four for a cell with line breaks.
  const barRows = $derived(Math.min(4, String(barText || '').split('\n').length));

  /** Select cell (`r`, `c`): a merge's top-left cell for any of its cells; kept in view. */
  function select(r, c, { show = true } = {}) {
    if (!grid) return;
    r = Math.max(1, Math.min(grid.rows, r));
    c = Math.max(1, Math.min(grid.cols, c));
    const m = mergeAt(sheet?.merges, r, c);
    sel = m ? { r: m.row, c: m.col } : { r, c };
    if (show) showSel();
  }

  /** The selected cell scrolled into view (past the frozen rows and columns). */
  function showSel() {
    if (!scroller || !grid) return;
    const fh = grid.rowAt[grid.frozenRows + 1];
    const fw = grid.colAt[grid.frozenCols + 1];
    const h = scroller.clientHeight - HEAD_PX;
    const w = scroller.clientWidth - grid.numbers;
    if (sel.r > grid.frozenRows) {
      const top = grid.rowAt[sel.r];
      const bottom = grid.rowAt[sel.r + 1];
      if (top - fh < scroller.scrollTop) scroller.scrollTop = top - fh;
      else if (bottom > scroller.scrollTop + h) scroller.scrollTop = bottom - h;
    }
    if (sel.c > grid.frozenCols) {
      const left = grid.colAt[sel.c];
      const right = grid.colAt[sel.c + 1];
      if (left - fw < scroller.scrollLeft) scroller.scrollLeft = left - fw;
      else if (right > scroller.scrollLeft + w) scroller.scrollLeft = Math.min(left - fw, right - w);
    }
  }

  /** The cell under a click (none on the headers). */
  function hit(e) {
    if (!scroller || !grid) return null;
    const rect = scroller.getBoundingClientRect();
    const vx = e.clientX - rect.left - grid.numbers;
    const vy = e.clientY - rect.top - HEAD_PX;
    if (vx < 0 || vy < 0) return null;
    const fw = grid.colAt[grid.frozenCols + 1];
    const fh = grid.rowAt[grid.frozenRows + 1];
    const gx = vx < fw ? vx : vx + scroller.scrollLeft;
    const gy = vy < fh ? vy : vy + scroller.scrollTop;
    return { r: indexAt(grid.rowAt, gy), c: indexAt(grid.colAt, gx) };
  }

  function onPointer(e) {
    if (e.button !== 0) return;
    // (A click in the box being typed in stays there.)
    if (editing && e.target === editEl) return;
    const at = hit(e);
    if (!at) return;
    if (editing) commit();
    select(at.r, at.c, { show: false });
  }

  /** A double-click: typing in that cell, in its edit mode (as F2). */
  function onDouble(e) {
    if (e.button !== 0 || editing) return;
    const at = hit(e);
    if (!at) return;
    select(at.r, at.c, { show: false });
    startEdit('edit');
  }

  /** The keys Excel moves the selection with, and the ones that start typing in a cell, Undo, Redo and save. */
  function onKey(e) {
    if (!grid || editing) return;
    const ctrl = e.ctrlKey || e.metaKey;
    const lower = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (ctrl && !e.altKey && (lower === 'z' || lower === 'y')) {
      e.preventDefault();
      if (lower === 'y' || e.shiftKey) redo();
      else undo();
      return;
    }
    if (ctrl && !e.altKey && lower === 's') {
      e.preventDefault();
      if (book?.edit?.dirty) saveNow();
      return;
    }
    if (e.key === 'F2') {
      e.preventDefault();
      startEdit('edit');
      return;
    }
    if (e.key === 'Delete' && !ctrl && !e.altKey) {
      e.preventDefault();
      clearSel();
      return;
    }
    if (e.key === 'Backspace' && !ctrl && !e.altKey) {
      e.preventDefault();
      startEdit('enter', '');
      return;
    }
    if (e.key === 'Enter' && !ctrl && !e.altKey) {
      e.preventDefault();
      moveBy(e.shiftKey ? -1 : 1, 0);
      return;
    }
    // A character typed: a new value in the cell, in its enter mode (an arrow ends it).
    if (e.key.length === 1 && !ctrl && !e.altKey && !e.isComposing) {
      e.preventDefault();
      startEdit('enter', e.key);
      return;
    }
    const page = Math.max(1, win.r1 - win.r0 - 16);
    const next = (r, dr) => {
      let n = r + dr;
      while (n >= 1 && n <= grid.rows && grid.rowPx[n] <= 0) n += Math.sign(dr) || 1;
      return n;
    };
    const nextCol = (c, dc) => {
      let n = c + dc;
      while (n >= 1 && n <= grid.cols && grid.colPx[n] <= 0) n += Math.sign(dc) || 1;
      return n;
    };
    const m = selMerge;
    let { r, c } = sel;
    switch (e.key) {
      case 'ArrowDown':
        r = next(m ? m.row + m.rows - 1 : r, 1);
        break;
      case 'ArrowUp':
        r = next(r, -1);
        break;
      case 'ArrowRight':
      case 'Tab':
        if (e.key === 'Tab' && e.shiftKey) c = nextCol(c, -1);
        else c = nextCol(m ? m.col + m.cols - 1 : c, 1);
        break;
      case 'ArrowLeft':
        c = nextCol(c, -1);
        break;
      case 'PageDown':
        r = Math.min(grid.rows, r + page);
        break;
      case 'PageUp':
        r = Math.max(1, r - page);
        break;
      case 'Home':
        if (e.ctrlKey) r = 1;
        c = 1;
        break;
      case 'End':
        if (e.ctrlKey && sheet?.used?.top) {
          r = sheet.used.bottom;
          c = sheet.used.right;
        }
        break;
      case 'c':
      case 'C':
        if (e.ctrlKey && !e.shiftKey && !e.altKey) {
          e.preventDefault();
          copyText(selCell?.t || '', 'Cell copied');
        }
        return;
      default:
        return;
    }
    e.preventDefault();
    select(r, c);
  }

  // ---- rows and columns (chat/sheets/shift.rs) ----------------------------------------------------

  /** **Rows or columns put in or taken out**: one, before line `at` (`insert`) or that line; the selection kept on its row or column. */
  async function lines(rowsAxis, at, insert) {
    if (!editable) return say(whyNot());
    if (editing) await commit();
    const id = book?.id;
    const keep = rowsAxis ? { r: at, c: sel.c } : { r: sel.r, c: at };
    await apply(() => invoke('sheet_lines', { id, sheet: current, rows: rowsAxis, at, count: 1, insert }).then((res) => ({ ...res, at: [current, keep.r, keep.c] })), { retry: true });
  }

  /** The row number or column letter under a right-click (none elsewhere). */
  function headAt(e) {
    if (!scroller || !grid) return null;
    const rect = scroller.getBoundingClientRect();
    const vx = e.clientX - rect.left - grid.numbers;
    const vy = e.clientY - rect.top - HEAD_PX;
    if (vx >= 0 && vy >= 0) return null;
    if (vx < 0 && vy < 0) return null;
    const fw = grid.colAt[grid.frozenCols + 1];
    const fh = grid.rowAt[grid.frozenRows + 1];
    if (vx < 0) return { row: indexAt(grid.rowAt, vy < fh ? vy : vy + scroller.scrollTop) };
    return { col: indexAt(grid.colAt, vx < fw ? vx : vx + scroller.scrollLeft) };
  }

  /** **A row number's or a column letter's menu**: insert one before or after it, or delete it (Undo puts it back). */
  function onHeadMenu(e) {
    const at = headAt(e);
    if (!at) return;
    const why = editable ? '' : whyNot();
    const off = !editable;
    if (at.row != null) {
      if (sel.r !== at.row) select(at.row, sel.c, { show: false });
      const r = at.row;
      openContextMenu(
        e,
        [
          { label: 'Insert row above', icon: 'plus', disabled: off, onselect: () => lines(true, r, true) },
          { label: 'Insert row below', icon: 'plus', disabled: off, onselect: () => lines(true, r + 1, true) },
          { separator: true },
          { label: `Delete row ${r}`, icon: 'trash', danger: true, disabled: off, onselect: () => lines(true, r, false) },
          why && { note: why },
        ],
        { label: `Row ${r}`, row: scroller },
      );
    } else {
      if (sel.c !== at.col) select(sel.r, at.col, { show: false });
      const c = at.col;
      openContextMenu(
        e,
        [
          { label: 'Insert column left', icon: 'plus', disabled: off, onselect: () => lines(false, c, true) },
          { label: 'Insert column right', icon: 'plus', disabled: off, onselect: () => lines(false, c + 1, true) },
          { separator: true },
          { label: `Delete column ${columnName(c)}`, icon: 'trash', danger: true, disabled: off, onselect: () => lines(false, c, false) },
          why && { note: why },
        ],
        { label: `Column ${columnName(c)}`, row: scroller },
      );
    }
  }

  // ---- sheets -------------------------------------------------------------------------------------

  async function toSheet(i) {
    if (!book || i === current) return;
    if (scroller) places.set(sheet?.name, { top: scroller.scrollTop, left: scroller.scrollLeft, sel });
    current = i;
    forgetRows();
    const place = places.get(book.sheets[i]?.name);
    sel = place?.sel ?? { r: 1, c: 1 };
    await tick();
    if (scroller) {
      scroller.scrollTop = place?.top ?? 0;
      scroller.scrollLeft = place?.left ?? 0;
    }
    measure();
  }

  // ---- live ---------------------------------------------------------------------------------------

  async function poll() {
    const src = ref?.src;
    if (!book || polling || saving || !src || src.from !== 'workspace' || !windowShown.shown || doc.showing !== 'sheet' || !doc.open) return;
    polling = true;
    try {
      const st = await invoke('sheet_stamp', { src });
      if (ref?.src !== src || saving) return;
      // Claude at work in the chat's folder: live and read only (what's being typed waits in its box).
      working = !!st.working;
      if (st.len === book.stamp.len && st.ms === book.stamp.ms) return;
      // Changes not saved here: never read over. The bar says so.
      if (book.edit?.dirty || saveTimer) {
        conflict = true;
        return;
      }
      if (failed && failed.len === st.len && failed.ms === st.ms) return;
      await open({ keep: true, livePass: true });
      if (stale) failed = st;
    } catch {
      /* gone, or not there: said when it's opened again */
    } finally {
      polling = false;
    }
  }

  $effect(() => {
    if (!live) return;
    const t = setInterval(poll, LIVE_MS);
    return () => clearInterval(t);
  });

  // "Updated" for a moment after a change landed.
  let justNow = $state(false);
  $effect(() => {
    if (!updated) return;
    justNow = true;
    const t = setTimeout(() => (justNow = false), 2500);
    return () => clearTimeout(t);
  });

  onDestroy(() => {
    if (frame) cancelAnimationFrame(frame);
    clearTimeout(hintTimer);
    // Changes waiting to be saved go on the way out (the panel closed, another tab).
    if (saveTimer) saveNow();
  });

  // ---- the header's buttons -----------------------------------------------------------------------

  async function openInApp() {
    try {
      await invoke('sheet_open_app', { src: ref.src });
    } catch (e) {
      toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
</script>

<section class="sheettab" aria-label="Spreadsheet" data-testid="chat-sheet" data-name={name} data-sheet={sheet?.name ?? ''} data-book={book?.id ?? ''} data-conversation={conversationId ?? ''} style={fontCss}>
  <header class="sheethead">
    <svg class="sheeticon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 9.5h18M3 15h18M9 4v16" /></svg>
    <h2 class="sheettitle" title={book?.path || name}>
      <span class="sname" data-testid="chat-sheet-name">{name}</span>{#if folder}<span class="sdir">{folder}</span>{/if}
    </h2>
    {#if live}
      <span class="livepill" class:fresh={justNow} class:working title={stale ? "It changed, and Yap couldn't read the newest version yet: it tries again when it changes" : working ? "Claude is working in this chat's folder: its changes show as they land, and the sheet is read only until it's done" : 'Shown as it changes on disk'} data-testid="chat-sheet-live">
        <span class="livedot" aria-hidden="true"></span>{justNow ? 'Updated' : stale ? 'Changing…' : working ? 'Live · read only' : 'Live'}
      </span>
    {/if}
    {#if book && note}
      <span class="ropill" title={note} data-testid="chat-sheet-readonly">Read only</span>
    {:else if book && saveState}
      <span class="savestate" class:warn={saveState === 'unsaved' && (saveError || conflict)} data-testid="chat-sheet-save" data-state={saveState}>
        {saveState === 'saving' ? 'Saving…' : saveState === 'saved' ? 'Saved' : saveError || conflict ? 'Not saved' : 'Edited'}
      </span>
    {/if}
    <div class="sheetactions">
      {#if book && !note}
        <button type="button" class="sbtn" onclick={undo} aria-label="Undo" title="Undo (Ctrl+Z)" disabled={!canUndo || busy} data-testid="chat-sheet-undo">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" /></svg>
        </button>
        <button type="button" class="sbtn" onclick={redo} aria-label="Redo" title="Redo (Ctrl+Y)" disabled={!canRedo || busy} data-testid="chat-sheet-redo">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 14 5-5-5-5" /><path d="M20 9H9a5 5 0 0 0 0 10h3" /></svg>
        </button>
      {/if}
      <button type="button" class="sbtn" onclick={() => open({ keep: true })} aria-label="Read it again" title="Read it again" disabled={loading}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></svg>
      </button>
      {#if book?.path}
        <button type="button" class="sbtn" onclick={() => copyText(book.path, 'Path copied')} aria-label="Copy path" title="Copy path">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></svg>
        </button>
      {/if}
      <button type="button" class="sbtn wide" onclick={openInApp} title="Open it in your spreadsheet app" aria-label={appLabel} data-testid="chat-sheet-open-app" disabled={!book}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
        <span>{appLabel}</span>
      </button>
      {#if !narrow}
        <button type="button" class="sbtn" onclick={() => (doc.expanded = !doc.expanded)} aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'} title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}>
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="sbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>

  {#if error}
    <p class="snote" role="alert" data-testid="chat-sheet-error">{error}</p>
  {:else if book && grid && drawn}
    {#if conflict}
      <!-- Changed on disk under changes not saved here: never written over. -->
      <div class="sbar" role="alert" data-testid="chat-sheet-conflict">
        <span>{name} changed on disk since Yap read it, so your changes here aren't saved.</span>
        <button type="button" class="barbtn" onclick={reload} data-testid="chat-sheet-reload">Reload</button>
        <button type="button" class="barbtn" onclick={saveCopy} data-testid="chat-sheet-save-copy">Save as a copy</button>
      </div>
    {:else if saveError}
      <div class="sbar" role="alert" data-testid="chat-sheet-save-error">
        <span>{saveError}</span>
        <button type="button" class="barbtn" onclick={() => { saveError = ''; saveNow(); }}>Try again</button>
      </div>
    {/if}
    <!-- The formula bar: the cell selected, its formula or its value; typed in, when the sheet can be. -->
    <div class="fbar" class:editing={editing?.where === 'bar'}>
      <span class="namebox" data-testid="chat-sheet-cell">{cellName(selMerge?.row ?? sel.r, selMerge?.col ?? sel.c)}</span>
      <span class="fx" aria-hidden="true">fx</span>
      <textarea
        class="ftext"
        bind:this={barEl}
        readonly={!editable}
        value={barText}
        rows={barRows}
        oninput={onEditInput}
        onfocus={onBarFocus}
        onkeydown={onEditKey}
        onblur={onEditBlur}
        spellcheck="false"
        autocomplete="off"
        aria-label={`The formula or value of ${cellName(sel.r, sel.c)}`}
        data-testid="chat-sheet-formula"
      ></textarea>
    </div>
    {#if hint}<p class="shint" role="status" data-testid="chat-sheet-hint">{hint}</p>{/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="sgrid"
      bind:this={scroller}
      onscroll={onScroll}
      onpointerdown={onPointer}
      ondblclick={onDouble}
      oncontextmenu={onHeadMenu}
      onkeydown={onKey}
      tabindex="0"
      role="grid"
      aria-label={editable ? sheet.name : `${sheet.name}, read only`}
      aria-readonly={editable ? undefined : 'true'}
      data-editable={editable ? 'true' : 'false'}
      aria-rowcount={grid.rows}
      aria-colcount={grid.cols}
      data-testid="chat-sheet-grid"
      data-rows={win.r0 + '-' + win.r1}
    >
      <div class="scanvas" style:width={`${grid.numbers + grid.width}px`} style:height={`${HEAD_PX + grid.height}px`}>
        {#snippet cellsOf(layer)}
          {#each drawn.lines[layer] as l (l.key)}<div class="gl" style:left={`${l.x}px`} style:top={`${l.y}px`} style:width={`${l.w}px`} style:height={`${l.h}px`}></div>{/each}
          {#each drawn.cells[layer] as it (it.key)}
            {#if it.spill}<div class="smask" style:left={`${it.x + it.w - 1}px`} style:top={`${it.y}px`} style:width={`${it.spill - it.w + 1}px`} style:height={`${it.h - 1}px`}></div>{/if}
            <div class="sc" class:wrap={it.wrap} class:filled={it.filled} data-cell={cellName(it.r, it.c)} style={`left:${it.x}px;top:${it.y}px;width:${it.w}px;height:${it.h}px;${it.css}`}>
              {#if it.borders}<span class="sb" style={it.borders}></span>{/if}
              {#if it.text}<span class="st a-{it.align} v-{it.v}" style:width={it.spill ? `${it.spill}px` : null}>{it.text}</span>{/if}
            </div>
          {/each}
          {#if selBox?.layer === layer}<div class="ssel" style:left={`${selBox.x - 1}px`} style:top={`${selBox.y - 1}px`} style:width={`${selBox.w + 1}px`} style:height={`${selBox.h + 1}px`}></div>{/if}
          {#if editBox?.layer === layer}
            <!-- What's typed, over its cell (Excel's in-cell editing). -->
            <textarea
              class="sedit"
              bind:this={editEl}
              value={editing.text}
              oninput={onEditInput}
              onkeydown={onEditKey}
              onblur={onEditBlur}
              onpointerdown={(e) => e.stopPropagation()}
              spellcheck="false"
              autocomplete="off"
              aria-label={`Editing ${cellName(editing.r, editing.c)}`}
              data-testid="chat-sheet-editor"
              style={`left:${editBox.x - 1}px;top:${editBox.y - 1}px;width:${editBox.w + 1}px;height:${editBox.h + 1}px;${editBox.css}`}
            ></textarea>
          {/if}
        {/snippet}
        {#snippet headsOf(layer)}
          {#each drawn.heads[layer] as h (h.key)}
            <div class="hd" class:col={h.col} class:on={h.on} data-head={h.col ? `col-${h.text}` : `row-${h.text}`} style:left={`${h.x}px`} style:top={`${h.y}px`} style:width={`${h.w}px`} style:height={`${h.h}px`}>{h.text}</div>
          {/each}
        {/snippet}
        <!-- The cells that scroll. -->
        <div class="layer body">{@render cellsOf('body')}</div>
        <!-- Frozen rows and the column letters: stuck at the top. -->
        <div class="layer top">
          <div class="band" style:width={`${grid.numbers + grid.width}px`} style:height={`${HEAD_PX + drawn.fh}px`}></div>
          {@render cellsOf('top')}
          {@render headsOf('top')}
          {#if drawn.fh}<div class="freeze h" style:top={`${HEAD_PX + drawn.fh - 1}px`} style:width={`${grid.numbers + grid.width}px`}></div>{/if}
        </div>
        <!-- Frozen columns and the row numbers: stuck at the left. -->
        <div class="layer left">
          <div class="band" style:width={`${grid.numbers + drawn.fw}px`} style:height={`${HEAD_PX + grid.height}px`}></div>
          {@render cellsOf('left')}
          {@render headsOf('left')}
          {#if drawn.fw}<div class="freeze v" style:left={`${grid.numbers + drawn.fw - 1}px`} style:height={`${HEAD_PX + grid.height}px`}></div>{/if}
        </div>
        <!-- Both: the corner. -->
        <div class="layer corner">
          <div class="band" style:width={`${grid.numbers + drawn.fw}px`} style:height={`${HEAD_PX + drawn.fh}px`}></div>
          {@render cellsOf('corner')}
          <div class="hd all" style:width={`${grid.numbers}px`} style:height={`${HEAD_PX}px`}></div>
          {@render headsOf('corner')}
        </div>
      </div>
    </div>
    <!-- The sheets, as Excel's tabs at the foot (a hidden one isn't shown, as in Excel). -->
    <div class="sheetfoot">
      <div class="stabs" role="tablist" aria-label="Sheets" data-testid="chat-sheet-tabs">
        {#each book.sheets as s, i (i)}
          {#if !s.hidden}
            <button type="button" role="tab" class="stab" class:on={i === current} aria-selected={i === current} onclick={() => toSheet(i)} style:--tab={s.color || null} title={s.name}>{s.name}</button>
          {/if}
        {/each}
      </div>
      {#if facts}<p class="sfacts" data-testid="chat-sheet-facts">{facts}</p>{/if}
    </div>
  {:else if loading}
    <p class="snote">Reading…</p>
  {/if}
</section>

<style>
  .sheettab {
    /* The sheet's own colours, from Yap's: its paper, its ink, a light grid. A file's colours are its own. */
    --sheet-bg: var(--yap-paper);
    --sheet-ink: var(--yap-paper-ink);
    --sheet-line: var(--yap-paper-ink);
    --sheet-grid: var(--yap-paper-s2);
    --sheet-head: var(--yap-paper-s1);
    --sheet-head-ink: var(--yap-paper-muted);
    --sheet-head-rule: var(--yap-paper-s2-strong);
    --sheet-font: var(--sheet-font-own, 'Calibri'), 'Aptos', 'Segoe UI', Arial, sans-serif;
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
    container-type: inline-size;
  }
  /* A narrow panel: "Open in Excel" is its icon (its name in the tooltip), so the file's name has the room. */
  @container (max-width: 560px) {
    .sbtn.wide span {
      display: none;
    }
  }
  .sheethead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .sheeticon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-code);
  }
  .sheettitle {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  /* The name keeps its room; the folder gives way first. */
  .sname {
    flex: 0 1 auto;
    min-width: 5em;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sdir {
    flex: 0 1000 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    font-weight: 400;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .livepill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: 0 0 auto;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--yap-paper-s1);
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-muted);
    transition: background 0.3s;
  }
  .livepill.fresh {
    background: var(--yap-primary-tint);
    color: var(--yap-paper-ink);
  }
  .livedot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--yap-code);
  }
  .livepill.working .livedot {
    animation: livepulse 1.4s ease-in-out infinite;
  }
  @keyframes livepulse {
    50% {
      opacity: 0.35;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .livepill.working .livedot {
      animation: none;
    }
  }
  :global([data-motion='reduced']) .livepill.working .livedot {
    animation: none;
  }
  /* Read only (an attached copy, an .xls…): why, in its tooltip and when you try to type. */
  .ropill,
  .savestate {
    flex: 0 0 auto;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-muted);
    white-space: nowrap;
  }
  .ropill {
    border: 1px solid var(--yap-paper-rule);
  }
  .savestate.warn {
    color: var(--yap-danger, var(--yap-paper-ink));
  }
  /* Changed on disk, a save that failed: a quiet bar over the formula bar. */
  .sbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px 6px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
    background: var(--yap-paper-s1);
    color: var(--yap-paper-text-2);
    font-size: 12px;
  }
  .sbar span {
    flex: 1 1 auto;
    min-width: 0;
  }
  .barbtn {
    flex: 0 0 auto;
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r);
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .barbtn:hover {
    background: var(--yap-paper-s2);
  }
  .barbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .shint {
    margin: 0;
    padding: 4px 12px 4px 18px;
    border-bottom: 1px solid var(--sheet-head-rule);
    background: var(--yap-paper-s1);
    color: var(--yap-paper-text-2);
    font-size: 12px;
  }
  .sheetactions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
  }
  .sbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    min-width: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .sbtn.wide {
    padding: 0 8px;
    border-color: var(--yap-paper-rule);
    color: var(--yap-paper-text-2);
    white-space: nowrap;
  }
  .sbtn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .sbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .sbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .sbtn svg {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
  }

  /* The formula bar. */
  .fbar {
    display: flex;
    align-items: flex-start;
    gap: 0;
    min-height: 28px;
    border-bottom: 1px solid var(--sheet-head-rule);
    background: var(--sheet-bg);
    font-size: 12px;
  }
  .namebox {
    flex: 0 0 auto;
    width: 72px;
    padding: 0 8px;
    border-right: 1px solid var(--sheet-head-rule);
    color: var(--yap-paper-text-2);
    font-variant-numeric: tabular-nums;
    line-height: 28px;
  }
  .fx {
    flex: 0 0 auto;
    padding: 0 9px;
    border-right: 1px solid var(--sheet-head-rule);
    color: var(--yap-paper-muted);
    font-style: italic;
    font-family: Georgia, 'Times New Roman', serif;
    line-height: 28px;
  }
  .ftext {
    flex: 1 1 auto;
    min-width: 0;
    box-sizing: border-box;
    min-height: 28px;
    margin: 0;
    padding: 6px 8px 5px;
    border: none;
    background: none;
    color: var(--sheet-ink);
    font: inherit;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    line-height: 16px;
    resize: none;
    overflow: hidden;
    white-space: pre;
  }
  .ftext:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .ftext:not([readonly]) {
    cursor: text;
  }
  .fbar.editing .ftext {
    outline: 2px solid var(--yap-primary);
    outline-offset: -2px;
  }
  /* What's typed, over its cell: the cell's own font and colours, Excel's thick edge. */
  .sedit {
    position: absolute;
    box-sizing: border-box;
    z-index: 3;
    margin: 0;
    padding: 0 3px;
    border: 2px solid var(--yap-primary);
    border-radius: 0;
    background: var(--sheet-bg);
    color: var(--sheet-ink);
    font: inherit;
    line-height: 1.2;
    user-select: text;
    outline: none;
    box-shadow: 0 1px 4px rgb(0 0 0 / 0.18);
    /* A textarea, so Alt+Enter's lines show: no handle, no scroll bar of its own. */
    resize: none;
    overflow: hidden;
    white-space: pre;
  }

  /* The grid. */
  .sgrid {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    background: var(--sheet-bg);
    font-family: var(--sheet-font);
    font-size: var(--sheet-size, 14.67px);
    line-height: 1.2;
    color: var(--sheet-ink);
    overscroll-behavior: contain;
    user-select: none;
  }
  .sgrid:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .scanvas {
    position: relative;
  }
  /* Each layer takes no room: its cells are placed absolutely; the frozen ones stick. */
  .layer {
    position: sticky;
    width: 0;
    height: 0;
  }
  .layer.body {
    position: absolute;
    inset: 0;
    width: auto;
    height: auto;
    z-index: 1;
  }
  .layer.top {
    top: 0;
    z-index: 3;
  }
  .layer.left {
    left: 0;
    z-index: 2;
  }
  .layer.corner {
    top: 0;
    left: 0;
    z-index: 4;
  }
  .band {
    position: absolute;
    left: 0;
    top: 0;
    background: var(--sheet-bg);
  }
  :global([data-yap-theme='dark']) .sheettab {
    --sheet-grid: var(--yap-paper-s2-strong);
  }
  .gl {
    position: absolute;
    background: var(--sheet-grid);
  }
  .smask {
    position: absolute;
    background: var(--sheet-bg);
  }
  .sc {
    position: absolute;
    box-sizing: border-box;
  }
  .st {
    position: absolute;
    inset: 0;
    display: flex;
    padding: 0 3px;
    overflow: hidden;
    white-space: pre;
  }
  .sc.wrap .st {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .st.a-left {
    justify-content: flex-start;
    text-align: left;
  }
  .st.a-center {
    justify-content: center;
    text-align: center;
  }
  .st.a-right {
    justify-content: flex-end;
    text-align: right;
  }
  .st.a-justify {
    justify-content: flex-start;
    text-align: justify;
  }
  .st.v-bottom {
    align-items: flex-end;
    padding-bottom: 1px;
  }
  .st.v-center {
    align-items: center;
  }
  .st.v-top {
    align-items: flex-start;
    padding-top: 1px;
  }
  .sb {
    position: absolute;
    right: 0;
    bottom: 0;
    pointer-events: none;
    z-index: 1;
  }
  .ssel {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--yap-primary);
    pointer-events: none;
    z-index: 2;
  }
  .hd {
    position: absolute;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    border-right: 1px solid var(--sheet-head-rule);
    border-bottom: 1px solid var(--sheet-head-rule);
    background: var(--sheet-head);
    color: var(--sheet-head-ink);
    font-family: 'Segoe UI', system-ui, sans-serif;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    overflow: hidden;
  }
  .hd.on {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-weight: 600;
    box-shadow: inset 0 -2px 0 var(--yap-primary);
  }
  .hd.on:not(.col) {
    box-shadow: inset -2px 0 0 var(--yap-primary);
  }
  .hd.all {
    left: 0;
    top: 0;
  }
  .freeze {
    position: absolute;
    background: var(--yap-paper-rule);
  }
  .freeze.h {
    left: 0;
    height: 1px;
  }
  .freeze.v {
    top: 0;
    width: 1px;
  }

  /* The sheets' tabs. */
  .sheetfoot {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 30px;
    border-top: 1px solid var(--sheet-head-rule);
    background: var(--sheet-head);
  }
  .stabs {
    display: flex;
    flex: 0 1 auto;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .stab {
    flex: 0 0 auto;
    max-width: 180px;
    padding: 6px 14px 5px;
    border: none;
    border-right: 1px solid var(--sheet-head-rule);
    border-bottom: 3px solid var(--tab, transparent);
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .stab:hover {
    background: var(--yap-paper-s2);
  }
  .stab.on {
    background: var(--sheet-bg);
    color: var(--yap-paper-ink);
    font-weight: 600;
    box-shadow: inset 0 2px 0 var(--yap-code);
  }
  .stab:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .sfacts {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    padding-right: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .snote {
    margin: 18px;
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
</style>
