<script>
  // The document beside the conversation (chat/documents.rs, chatDoc.svelte.js):
  // its title, what's being written ("Writing: Gear…", "Editing…"), and two
  // views (2026-10-31, after Claude Code's own markdown file view, none of
  // its code): **Preview**, the document as a page to read (and, below,
  // to edit), and **Source**, its markdown coloured and editable — saved as
  // you type, the person's words winning over Chat's writing (it took the
  // old Edit box's place). Find in the document (Ctrl+F, either view), and in ⋯: Copy as Markdown,
  // Export as Markdown…, Open in Notes. Undo (Chat's last change), expand
  // to the whole of Chat, ✕. Select words on the page and a small popover
  // offers "Ask about this" and "Change this…": the question goes to Chat's
  // box with the passage quoted (`onquote`). A change Chat makes lands with
  // its blocks marked a moment; each document's scroll is kept per view.
  // **As Chat writes** (2026-10-31, after Claude's own documents): a
  // section flows in line by line at a pace that follows how far behind it
  // is (`revealed`, docStream.js `revealStep`, a frame at a time; motion
  // reduced, as it comes), a small labelled caret at the end of its text
  // ("Chat"), the page following it while it's in view; the sections still
  // to write wait as grey lines, their name and what they'll say. The page
  // is drawn a piece at a time (`drawParts`): only the section being
  // written is drawn again as it grows.
  // **A page you can edit** (2026-10-31, stage 2): once nothing's flowing
  // in, Preview is the page in an editor (docEditor.js, TipTap on
  // ProseMirror, its own chunk loaded the first time it's shown; the page
  // as drawn until then), with a toolbar over it (DocToolbar: text style,
  // code, table, checklist, + insert). The note stays markdown: a change
  // goes out as the markdown of the block it's in, the rest exactly as it
  // was (docMarkdown.js), saved as Source's typing is. While Chat writes,
  // the page is drawn as before and the toolbar waits.
  import { tick, untrack } from 'svelte';
  import DocToolbar from './DocToolbar.svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { highlightMarkdown } from '../../core/markdownSource.js';
  import { motionReduced } from '../../core/appearance.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import SummaryFind from '../../components/SummaryFind.svelte';
  import { shownOffset, copyDocument, exportDocument } from './docTools.js';
  import { splitSections, drawParts, placeCaret } from './docPieces.js';
  import { shownSegments, revealStep, revealCut, revealOn } from './docStream.js';

  /**
   * `doc`: the panel's state; `cover`: over the conversation (expanded, or a
   * narrow window); `narrow`: no room beside it; `onquote({noteId, title,
   * text, fromLine, toLine, mode})`: a selected passage asked about ("ask")
   * or to change ("change").
   */
  let { doc, cover = false, narrow = false, onquote = null } = $props();

  let box = $state(null);
  let bodyEl = $state(null);
  let srcEl = $state(null);
  let mirrorEl = $state(null);
  let mdEl = $state(null);
  let finder = $state(null);
  let finding = $state(false);

  const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches || document.documentElement.dataset.motion === 'reduced';

  // ---- the flow: a section's text onto the page at its own pace ------------------------

  // How much of each section on its way (`doc.live`) is shown: characters,
  // a fraction kept.
  let revealed = $state({});
  // Each one's text as it was last time (what `revealed` counted in).
  let was = {};
  let frame = 0;
  let timer = 0;
  let last = 0;

  $effect(() => {
    const live = doc.live;
    untrack(() => {
      const instant = motionReduced();
      revealed = Object.fromEntries(Object.entries(live).map(([k, t]) => [k, instant ? t.length : revealOn(revealed[k], t, was[k])]));
      was = { ...live };
      pace();
    });
  });
  $effect(() => () => {
    cancelAnimationFrame(frame);
    clearTimeout(timer);
  });

  /** The next frame of the flow (a timer stands in when frames don't come: a hidden window). */
  function pace() {
    if (frame || timer) return;
    if (!last) last = performance.now();
    const run = () => {
      cancelAnimationFrame(frame);
      clearTimeout(timer);
      frame = timer = 0;
      flow();
    };
    frame = requestAnimationFrame(run);
    timer = setTimeout(run, 150);
  }

  /** One frame: each section a little further on; one that landed, all shown, leaves. */
  function flow() {
    const now = performance.now();
    const dt = now - last;
    last = now;
    const instant = motionReduced();
    const next = {};
    const done = [];
    let more = false;
    for (const [k, t] of Object.entries(doc.live)) {
      next[k] = instant ? t.length : revealStep(revealed[k] ?? 0, t.length, dt);
      if (next[k] < t.length) more = true;
      else if (doc.isSettled(k)) done.push(k);
    }
    revealed = next;
    for (const k of done) doc.release(k);
    if (more) pace();
    else last = 0;
  }

  // What the page shows: each live section as far as it has come.
  const onPage = $derived(Object.fromEntries(Object.entries(doc.live).map(([k, t]) => [k, t.slice(0, revealCut(t, revealed[k] ?? 0))])));
  const segments = $derived(shownSegments(doc.content, doc.title, onPage));
  const shown = $derived(segments.map((s) => s.md).join('\n'));

  // ---- the page you can edit -----------------------------------------------------------

  // Preview with nothing flowing in: the editor (`createDocEditor`, its
  // chunk loaded the first time it's wanted), on `doc.draft` — the text
  // Source edits too — saving through `doc.input`.
  const editing = $derived(doc.view === 'preview' && !doc.gone && !doc.streaming);
  let makeEditor = $state(null);
  let editEl = $state(null);
  let ed = $state(null);
  let tools = $state({});
  let seenLanded = 0;

  $effect(() => {
    if (!editing || makeEditor) return;
    import('./docEditor.js')
      .then((m) => (makeEditor = m.createDocEditor))
      .catch(() => {
        /* the page stays as drawn; Source still edits */
      });
  });

  $effect(() => {
    const el = editEl;
    const make = makeEditor;
    if (!el || !make) return;
    const ctl = untrack(() =>
      make({
        mount: el,
        text: doc.draft,
        title: doc.title,
        onchange: (md) => doc.input(md),
        onstate: (s) => (tools = s),
      })
    );
    seenLanded = untrack(() => doc.landed);
    // A change marked on the page a moment ago (Chat's turn just ended): still marked.
    const left = 1700 - (performance.now() - washed.at);
    if (left > 120 && washed.id === untrack(() => doc.id)) ctl.markBlocks(washed.idx, left);
    ed = ctl;
    return () => {
      ctl.destroy();
      if (ed === ctl) ed = null;
      tools = {};
    };
  });

  /** A click on the page under its last block: the caret at the end of the document. */
  function pageClick(e) {
    if (!ed || !editEl || editEl.contains(e.target)) return;
    if (e.clientY < editEl.getBoundingClientRect().bottom) return;
    ed.focus('end');
  }

  // It follows the document from outside (Chat's change merged in, Undo,
  // another window): the blocks that changed are replaced, and a change
  // that landed is marked.
  $effect(() => {
    const text = doc.draft;
    const landed = doc.landed;
    const title = doc.title;
    const ctl = ed;
    if (!ctl) return;
    untrack(() => ctl.load(text, { title, mark: landed !== seenLanded }));
    seenLanded = landed;
  });

  // **The sections' rail** (after Claude's document outline, none of its
  // code): a document of three sections or more gets a thin rail of ticks
  // at its left, one per section, the one being read marked; hovered or
  // clicked, it opens the list of sections, and one click goes there.
  let outline = $state([]);
  let current = $state(0);
  let outlineOpen = $state(false);
  // The pointer is over the rail: it's open, and a click keeps it so (a
  // click toggles it from the keyboard).
  let hovering = false;
  const RAIL_FROM = 3;
  const sections = () => (bodyEl ? [...bodyEl.querySelectorAll('.docmd > h3')] : []);

  $effect(() => {
    // Again whenever the document's text or view changes.
    void shown;
    void doc.view;
    if (!bodyEl) {
      outline = [];
      return;
    }
    tick().then(() => {
      const now = sections().map((h, i) => ({ i, text: h.textContent.trim() }));
      // (Every frame while a section flows in: only a change is news.)
      if (now.map((h) => h.text).join('\n') !== outline.map((h) => h.text).join('\n') || now.length !== outline.length) outline = now;
      markCurrent();
    });
  });

  function onScroll() {
    pop = null;
    if (bodyEl) doc.keepScroll(bodyEl.scrollTop, 'preview');
    markCurrent();
  }

  /** The section being read: the last whose heading is near the top or above it. */
  function markCurrent() {
    const hs = sections();
    if (!hs.length || !bodyEl) return;
    const top = bodyEl.getBoundingClientRect().top + 56;
    let c = 0;
    hs.forEach((h, i) => {
      if (h.getBoundingClientRect().top <= top) c = i;
    });
    current = c;
  }

  function jump(i) {
    const h = sections()[i];
    if (!h) return;
    h.scrollIntoView({ block: 'start', behavior: reduced() ? 'auto' : 'smooth' });
    current = i;
    outlineOpen = false;
  }

  function railKey(e) {
    if (e.key === 'Escape' && outlineOpen) {
      e.stopPropagation();
      outlineOpen = false;
    }
  }

  function railBlur(e) {
    if (!e.currentTarget.contains(e.relatedTarget)) outlineOpen = false;
  }

  const status = $derived.by(() => {
    if (doc.busy && doc.action === 'editing') return 'Editing…';
    if (doc.busy && doc.writing != null) return doc.writing ? `Writing: ${doc.writing}…` : 'Writing…';
    const n = doc.pending.length;
    if (n && !doc.busy) return n === 1 ? '1 section not written yet' : `${n} sections not written yet`;
    return '';
  });

  // ---- the two views and where each was scrolled to ------------------------------------

  // Back where it was in this view of this document (opened again, switched).
  $effect(() => {
    void doc.id;
    const v = doc.view;
    // (Read now: the page's first scroll event would record the top.)
    const top = doc.scrollFor(v);
    tick().then(() => {
      const el = v === 'source' ? srcEl : bodyEl;
      if (el) el.scrollTop = top;
    });
  });

  async function setView(v) {
    if (doc.view === v) return;
    pop = null;
    doc.setView(v);
    await tick();
    if (v === 'source') box?.focus({ preventScroll: true });
  }

  function viewKey(e) {
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const next = doc.view === 'source' ? 'preview' : 'source';
      setView(next);
      e.currentTarget.parentElement?.querySelector(`[data-view="${next}"]`)?.focus();
    }
  }

  // The Source box follows the document while you're not typing (Chat's
  // sections landing): the caret stays where it was in your words.
  $effect(() => {
    const text = doc.draft;
    const el = box;
    if (!el || el.value === text) return;
    const focused = document.activeElement === el;
    const start = el.selectionStart ?? 0;
    const before = el.value.slice(0, start);
    const caret = text.startsWith(before) ? start : Math.max(0, start + (text.length - el.value.length));
    el.value = text;
    if (focused) el.setSelectionRange(caret, caret);
  });

  // ---- find, copy, export --------------------------------------------------------------

  /** What find searches: the page (its title too) or the coloured source. */
  function findRoot() {
    return doc.view === 'source' ? mirrorEl : (bodyEl?.querySelector('.docpage') ?? null);
  }

  function openFind() {
    pop = null;
    finder?.show();
  }

  function moreMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Find in the document', onselect: openFind },
        { separator: true },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => copyDocument(doc), disabled: doc.gone },
        { label: 'Export as Markdown…', icon: 'download', onselect: () => exportDocument(doc), disabled: doc.gone },
        { separator: true },
        { label: 'Open in Notes', icon: 'open', onselect: () => doc.openInNotes(), disabled: doc.gone },
      ],
      { label: 'Document' }
    );
  }

  /** The panel's keys: Ctrl+F finds; Esc puts a popover away. */
  function panelKey(e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f' && !doc.gone) {
      e.preventDefault();
      e.stopPropagation();
      openFind();
    } else if (e.key === 'Escape' && pop) {
      e.preventDefault();
      e.stopPropagation();
      pop = null;
    }
  }

  // ---- "Ask about this" / "Change this…" on a selection --------------------------------

  // The popover over a selection on the page: where, and what.
  let pop = $state(null);
  let scrollEl = $state(null);

  /** The selection on the page now, if it's in the document's text. */
  function selected() {
    const sel = window.getSelection?.();
    if (!sel || sel.isCollapsed || !sel.rangeCount) return null;
    const range = sel.getRangeAt(0);
    const page = bodyEl?.querySelector('.docpage');
    if (!page?.contains(range.commonAncestorContainer)) return null;
    const text = sel.toString().replace(/\s+\n/g, '\n').trim();
    if (!text) return null;
    // In the editor: the lines of the blocks it's in, as the editor writes them.
    if (ed && editEl?.contains(range.commonAncestorContainer)) {
      const at = ed.selectionLines();
      return { range, text, fromLine: at?.fromLine ?? null, toLine: at?.toLine ?? null };
    }
    if (!mdEl) return null;
    // The markdown's lines (Rust maps them to blocks): the blocks the
    // selection starts and ends in. Not while a section streams in (the
    // page's lines aren't the note's then).
    const block = (node) => (node?.nodeType === 1 ? node : node?.parentElement)?.closest?.('.docmd > [data-line]');
    const a = block(range.startContainer);
    let b = block(range.endContainer);
    // A selection that ends at the very start of the next block (a
    // triple-click) has nothing of it: it ends in the one before.
    if (a && b && a !== b && range.endOffset === 0) b = b.previousElementSibling?.matches('[data-line]') ? b.previousElementSibling : a;
    let fromLine = null;
    let toLine = null;
    if (!doc.streaming && (a || b)) {
      const off = shownOffset(doc.content, doc.title);
      const lines = (el) => el.getAttribute('data-line').split('-').map(Number);
      const start = lines(a ?? b);
      const end = lines(b ?? a);
      fromLine = start[0] + off;
      toLine = Math.max(start[0], end[1] - 1) + off;
    }
    return { range, text, fromLine, toLine };
  }

  function showPop() {
    if (doc.view !== 'preview' || doc.gone || !onquote) return;
    const s = selected();
    if (!s || !scrollEl) {
      pop = null;
      return;
    }
    const r = s.range.getBoundingClientRect();
    const area = scrollEl.getBoundingClientRect();
    const x = Math.min(Math.max(r.left + r.width / 2 - area.left, 96), area.width - 96);
    const above = r.top - area.top - 8;
    pop = { x, y: above > 44 ? above : r.bottom - area.top + 8, below: above <= 44, text: s.text, fromLine: s.fromLine, toLine: s.toLine };
  }

  function pageUp() {
    // After the browser has set the selection.
    setTimeout(showPop, 0);
  }

  function pageKey(e) {
    if (e.shiftKey || e.key === 'End' || e.key === 'Home') setTimeout(showPop, 0);
  }

  // The selection went (a click elsewhere): so does the popover.
  $effect(() => {
    if (!pop) return;
    const gone = () => {
      const sel = window.getSelection?.();
      if (!sel || sel.isCollapsed) pop = null;
    };
    document.addEventListener('selectionchange', gone);
    return () => document.removeEventListener('selectionchange', gone);
  });

  function quote(mode) {
    const p = pop;
    if (!p || doc.id == null) return;
    pop = null;
    window.getSelection?.()?.removeAllRanges();
    // What's typed is saved first: the lines are the note's.
    doc.flushSave();
    onquote?.({ noteId: doc.id, title: doc.title, text: p.text, fromLine: p.fromLine, toLine: p.toLine, mode });
  }

  // ---- the page, drawn a piece at a time; a change that landed, marked ----------------

  // The page's pieces: each section being written its own, the rest cut at
  // their sections (`splitSections`), each with its first line in the
  // shown text (for `data-line`).
  const parts = $derived.by(() => {
    const out = [];
    let line = 0;
    for (const seg of segments) {
      if (seg.live != null) out.push({ md: seg.md, live: seg.live, line });
      else for (const s of splitSections(seg.md)) out.push({ md: s.md, live: null, line: line + s.line });
      line += seg.md.split('\n').length;
    }
    return out;
  });

  // Each piece's HTML by its markdown (its lines counted from its own
  // start; drawParts puts them on the page's): rendered once, not every
  // frame a section flows in.
  let rendered = new Map();
  function partHtml(md, keep) {
    let html = rendered.get(md);
    if (html == null) html = renderMarkdown(md, { lines: true });
    keep.set(md, html);
    return html;
  }

  // Which document and change the page was last drawn for: the blocks new
  // since a change landed (Chat's edit, Undo) are marked a moment.
  let drawnFor = null;
  let drawnLanded = 0;
  // The caret was in view: the page follows it as it writes.
  let following = true;
  // The blocks last marked (their places on the page) and when: the editor,
  // taking the page's place as the turn ends, marks them on.
  let washed = { at: 0, idx: [], id: null };

  $effect(() => {
    const el = mdEl;
    const list = parts;
    const id = doc.id;
    const landed = doc.landed;
    if (!el) return;
    const keep = new Map();
    const html = list.map((p) => ({ html: partHtml(p.md, keep), live: p.live != null, line: p.line }));
    rendered = keep;
    const fresh = drawParts(el, html, { compare: drawnFor === id && landed !== drawnLanded }).filter((b) => !b.classList.contains('md-pending'));
    if (drawnFor !== id) following = true;
    drawnFor = id;
    drawnLanded = landed;
    for (const b of fresh) {
      b.setAttribute('data-changed', '');
      setTimeout(() => b.removeAttribute('data-changed'), 1700);
    }
    if (fresh.length) washed = { at: performance.now(), idx: fresh.map((b) => [...el.children].indexOf(b)), id };
    const caret = placeCaret(el, 'Chat');
    if (!bodyEl) return;
    const area = bodyEl.getBoundingClientRect();
    if (caret) {
      // Follow the writing while it's in view; scrolled away, the page stays put.
      let r = caret.getBoundingClientRect();
      if (following && r.bottom > area.bottom - 28 && r.top < area.bottom + 160) {
        bodyEl.scrollTop += r.bottom - (area.bottom - 28);
        r = caret.getBoundingClientRect();
      }
      following = r.bottom > area.top && r.top < area.bottom;
    } else if (fresh[0]) {
      // A change that landed: its first block, in view.
      const r = fresh[0].getBoundingClientRect();
      if (r.bottom < area.top || r.top > area.bottom) fresh[0].scrollIntoView({ block: 'nearest', behavior: reduced() ? 'auto' : 'smooth' });
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="docpanel" class:cover aria-label="Document" data-testid="chat-doc" onkeydown={panelKey}>
  <header class="dochead">
    <svg class="docicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 13h6M9 17h4" /></svg>
    <h2 class="doctitle" title={doc.title}>{doc.title || 'Untitled document'}</h2>
    <div class="docactions">
      {#if doc.canUndo && !doc.busy && !doc.gone}
        <button type="button" class="docbtn" onclick={() => doc.undo()} aria-label="Undo Chat's last change" title="Undo Chat's last change">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" /></svg>
        </button>
      {/if}
      {#if !doc.gone}
        <!-- Preview (the page) or Source (its markdown, editable); ←/→ switch. -->
        <div class="viewseg" role="group" aria-label="View">
          {#each [['preview', 'Preview', 'The document as a page'], ['source', 'Source', 'Its Markdown, to read or edit']] as [v, label, hint] (v)}
            <button
              type="button"
              class="segbtn"
              class:on={doc.view === v}
              data-view={v}
              aria-pressed={doc.view === v}
              tabindex={doc.view === v ? 0 : -1}
              title={hint}
              onclick={() => setView(v)}
              onkeydown={viewKey}>{label}</button
            >
          {/each}
        </div>
      {/if}
      {#if !narrow}
        <button
          type="button"
          class="docbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      {#if !doc.gone}
        <button type="button" class="docbtn" data-more aria-label="More for the document" title="Find, copy, export, Open in Notes" aria-haspopup="menu" onclick={moreMenu}>
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18.5" cy="12" r="1.6" /></svg>
        </button>
      {/if}
      <button type="button" class="docbtn" onclick={() => doc.close()} aria-label="Close the document" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>
  {#if !doc.gone && doc.view === 'preview'}
    <DocToolbar {ed} state={tools} writing={doc.streaming} />
  {/if}
  {#if status}
    <p class="docstatus" class:live={doc.busy} data-testid="chat-doc-status" aria-live="polite">
      {#if doc.busy}<span class="dot" aria-hidden="true"></span>{/if}{status}
    </p>
  {/if}
  {#if !doc.gone}
    <div class="docfind">
      <SummaryFind bind:this={finder} bind:open={finding} root={findRoot} text={doc.view === 'source' || ed ? doc.draft : shown} look="paper" label="Find in the document" />
    </div>
  {/if}
  {#if doc.gone}
    <p class="docgone">This document was deleted.</p>
  {:else if doc.view === 'source'}
    <!-- Its markdown, coloured (a copy under a clear text box, character
         for character), saved as you type and merged with Chat's writing:
         your words win. -->
    <div class="srcscroll" bind:this={srcEl} onscroll={() => srcEl && doc.keepScroll(srcEl.scrollTop, 'source')}>
      <div class="srcwrap">
        <!-- eslint-disable-next-line svelte/no-at-html-tags — highlightMarkdown escapes every character it doesn't wrap -->
        <pre class="srcmirror" aria-hidden="true" bind:this={mirrorEl}>{@html highlightMarkdown(doc.draft)}{'\n​'}</pre>
        <textarea
          class="srcedit"
          bind:this={box}
          aria-label="Document source (Markdown)"
          spellcheck="false"
          oninput={(e) => doc.input(e.currentTarget.value)}
        ></textarea>
      </div>
    </div>
  {:else}
    <!-- A page to read, after Claude's own documents: a serif title and
         section headings, a reading-width column, filled table cells. -->
    <div class="docscroll" bind:this={scrollEl}>
      {#if outline.length >= RAIL_FROM}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <nav
          class="docrail"
          aria-label="Sections"
          data-testid="chat-doc-outline"
          onmouseenter={() => (hovering = outlineOpen = true)}
          onmouseleave={() => (hovering = outlineOpen = false)}
          onfocusout={railBlur}
          onkeydown={railKey}
        >
          <button
            type="button"
            class="railbtn"
            aria-expanded={outlineOpen}
            aria-label="Sections"
            title="Sections"
            onclick={() => (outlineOpen = hovering || !outlineOpen)}
          >
            {#each outline as h (h.i)}<span class="tick" class:on={h.i === current} aria-hidden="true"></span>{/each}
          </button>
          {#if outlineOpen}
            <ol class="docoutline">
              {#each outline as h (h.i)}
                <li>
                  <button type="button" class:on={h.i === current} aria-current={h.i === current ? 'location' : undefined} onclick={() => jump(h.i)}>
                    <span class="otick" aria-hidden="true"></span><span class="otext">{h.text}</span>
                  </button>
                </li>
              {/each}
            </ol>
          {/if}
        </nav>
      {/if}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="docbody"
        role="document"
        tabindex="0"
        aria-busy={doc.streaming ? 'true' : undefined}
        bind:this={bodyEl}
        onscroll={onScroll}
        onpointerup={pageUp}
        onkeyup={pageKey}
      >
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions (a pointer's way to the end: the keyboard has Ctrl+End) -->
        <article class="docpage" class:railed={outline.length >= RAIL_FROM} onclick={pageClick}>
          <h1 class="docname">{doc.title || 'Untitled document'}</h1>
          {#if editing && makeEditor}
            <!-- The editor's root (docEditor.js): the page, editable. One
                 per document (its own undo history). -->
            {#key doc.id}
              <div class="docmd docedit" bind:this={editEl}></div>
            {/key}
          {:else}
            <!-- Drawn by `drawParts` (renderMarkdown's sanitised HTML, a piece at a time). -->
            <div class="docmd" bind:this={mdEl}></div>
          {/if}
        </article>
      </div>
      {#if pop}
        <!-- Over the selected words: ask about them, or have them changed. -->
        <div class="selpop" class:below={pop.below} style={`left: ${pop.x}px; top: ${pop.y}px`} role="toolbar" aria-label="The selected passage" data-testid="chat-doc-selection">
          <button type="button" onpointerdown={(e) => e.preventDefault()} onclick={() => quote('ask')}>Ask about this</button>
          <span class="selsep" aria-hidden="true"></span>
          <button type="button" onpointerdown={(e) => e.preventDefault()} onclick={() => quote('change')}>Change this…</button>
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .docpanel {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--yap-paper);
    border-left: 1px solid var(--yap-border-subtle);
    color: var(--yap-paper-ink);
  }
  /* Over the conversation: expanded, or no room beside it. */
  .docpanel.cover {
    position: absolute;
    inset: 0;
    z-index: 5;
    width: auto !important;
    border-left: none;
  }
  .dochead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .docicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  /* The header's name: a label (the page below has the big title). */
  .doctitle {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .docactions {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .docbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .docbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .docbtn:focus-visible,
  .segbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .docbtn svg {
    width: 15px;
    height: 15px;
  }
  /* Preview | Source: a small segmented switch, the one shown raised. */
  .viewseg {
    display: inline-flex;
    margin: 0 4px;
    padding: 2px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .segbtn {
    height: 24px;
    padding: 0 9px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .segbtn:hover {
    color: var(--yap-paper-ink);
  }
  .segbtn.on {
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--yap-paper-ink) 12%, transparent);
  }
  .docstatus {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    padding: 6px 18px;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .docstatus.live {
    color: var(--yap-primary);
    font-weight: 600;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    animation: pulse 1.1s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 0.3;
    }
    50% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
  :global(:root[data-motion='reduced']) .dot {
    animation: none;
  }
  /* Find: the bar sits under the header when it's open. */
  .docfind :global(.find) {
    position: static;
    margin: 8px 12px 0;
  }
  .docgone {
    margin: 18px;
    font-size: 12.5px;
    color: var(--yap-paper-muted);
  }
  /* **The page** (2026-10-30, measured against Claude's own document view,
     none of its code): a reading-width column, centred when the panel is
     wide; a serif title and section headings (EB Garamond, Yap's display
     face), the smaller headings in the reading font; 16px between blocks;
     tables of filled cells, no rules. Reading text: Settings → General →
     Appearance (app.css). The markdown's headings come a level down
     (markdown.js): `#` → h2, a section's `##` → h3, `###` → h4. */
  .docscroll {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
  }
  .docbody {
    --doc-gap: calc(15px * var(--yap-reading-scale));
    flex: 1 1 auto;
    min-width: 0;
    overflow-y: auto;
    font-family: var(--yap-font-reading);
    font-size: calc(14px * var(--yap-reading-scale));
    line-height: calc(1.62 * var(--yap-reading-leading));
    overflow-wrap: break-word;
  }
  .docbody:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .docpage {
    box-sizing: content-box;
    max-width: 68ch;
    margin: 0 auto;
    padding: 22px clamp(18px, 6%, 56px) 64px;
    /* Its words can be selected (copied, asked about, changed); the app's
       chrome around it can't. */
    user-select: text;
    -webkit-user-select: text;
  }
  .docpage::selection,
  .docpage :global(::selection) {
    background: color-mix(in srgb, var(--yap-primary) 26%, transparent);
  }
  /* Room for the sections' rail in a narrow panel. */
  .docpage.railed {
    padding-left: max(40px, 6%);
  }
  .docname {
    margin: 0 0 calc(14px * var(--yap-reading-scale));
    font-family: var(--yap-font-display);
    font-size: calc(27px * var(--yap-reading-step));
    font-weight: 600;
    line-height: 1.18;
    letter-spacing: -0.005em;
    color: var(--yap-paper-ink);
    text-wrap: balance;
  }
  .docmd > :global(*) {
    margin: 0 0 var(--doc-gap);
  }
  .docmd > :global(:last-child) {
    margin-bottom: 0;
  }
  /* The summary under the title: a touch larger, like a lead. */
  .docmd > :global(p:first-child:not(.md-pending)) {
    font-size: 1.04em;
    color: var(--yap-paper-text-2);
  }
  .docmd :global(p),
  .docmd :global(li) {
    text-wrap: pretty;
  }
  /* A section still to write: its name and what it will say, quiet and
     bracketed, as Claude's documents show a section's intent while they
     fill (the brackets drawn, never the text's). */
  .docmd > :global(p.md-pending) {
    margin: 0 0 calc(10px * var(--yap-reading-scale));
    padding: 0;
    border: none;
    font-style: normal;
    color: var(--yap-paper-date);
  }
  .docmd > :global(p.md-pending)::before {
    content: '[';
  }
  .docmd > :global(p.md-pending)::after {
    content: ']';
  }
  .docmd :global(.md-pending-key) {
    font-weight: 600;
  }
  /* The writing caret: a thin bar in the accent at the end of the text
     being written, its label ("Chat") a small tab over it — after Claude's
     documents' labelled caret. Still (no blink): it moves as the text
     flows in. */
  .docmd :global(.doccaret) {
    position: relative;
    display: inline-block;
    width: 2px;
    height: 1.15em;
    margin: 0 1px 0 2px;
    vertical-align: -0.2em;
    border-radius: 1px;
    background: var(--yap-primary);
    pointer-events: none;
  }
  /* **The editor**: the same page, typed into. ProseMirror's own needs
     here (TipTap's sheet is left out: its colours aren't Yap's). */
  .docedit {
    position: relative;
    min-height: 2.5em;
    outline: none;
    white-space: pre-wrap;
    white-space: break-spaces;
    word-wrap: break-word;
    font-variant-ligatures: none;
    caret-color: var(--yap-paper-ink);
  }
  .docedit :global([contenteditable='false']) {
    white-space: normal;
  }
  .docedit :global(pre) {
    white-space: pre-wrap;
  }
  .docedit :global(img.ProseMirror-separator) {
    display: inline !important;
    width: 0 !important;
    height: 0 !important;
    margin: 0 !important;
    border: none !important;
  }
  .docedit:global(.ProseMirror-hideselection) :global(*::selection) {
    background: transparent;
  }
  .docedit:global(.ProseMirror-hideselection) {
    caret-color: transparent;
  }
  /* A block picked whole (a section still to write, a rule): ringed. */
  .docedit :global(.ProseMirror-selectednode) {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 3px;
    border-radius: 3px;
  }
  /* Table cells picked together. */
  .docedit :global(td.selectedCell),
  .docedit :global(th.selectedCell) {
    background: color-mix(in srgb, var(--yap-primary) 22%, var(--yap-paper-s1));
  }
  /* The caret between two blocks it can't sit in (a table, a rule). */
  .docedit :global(.ProseMirror-gapcursor) {
    display: none;
    position: absolute;
    margin: 0;
    pointer-events: none;
  }
  .docedit :global(.ProseMirror-gapcursor)::after {
    content: '';
    display: block;
    position: absolute;
    top: -2px;
    width: 20px;
    border-top: 1px solid var(--yap-paper-ink);
  }
  .docedit:global(.ProseMirror-focused) :global(.ProseMirror-gapcursor) {
    display: block;
  }
  :global(.docdrop) {
    background: var(--yap-primary);
  }
  /* A list item's words: no room of their own; its parts a little apart. */
  .docedit :global(li > .itembody > p) {
    margin: 0;
  }
  .docedit :global(li > .itembody > * + *) {
    margin-top: 0.35em;
  }
  .docedit :global(li.task[data-checked='true'] > .itembody) {
    color: var(--yap-paper-muted);
    text-decoration: line-through;
    text-decoration-color: var(--yap-paper-faint);
  }
  /* Markdown the editor keeps whole (edited in Source): drawn as the page is. */
  .docedit :global(.docraw > *) {
    margin: 0 0 var(--doc-gap);
  }
  .docedit :global(.docraw > :last-child) {
    margin-bottom: 0;
  }
  .docedit :global(p.md-pending) {
    cursor: default;
  }
  .docmd :global(.doccaret)::after {
    content: attr(data-who);
    position: absolute;
    bottom: calc(100% + 1px);
    left: 0;
    padding: 1px 5px;
    border-radius: 4px 4px 4px 0;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font-family: 'Figtree Variable', ui-sans-serif, system-ui, sans-serif;
    font-size: 10px;
    font-style: normal;
    font-weight: 650;
    line-height: 1.35;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
  .docmd :global(h2),
  .docmd :global(h3) {
    font-family: var(--yap-font-display);
    font-weight: 600;
    color: var(--yap-paper-ink);
    text-wrap: balance;
  }
  .docmd > :global(h2) {
    margin: calc(30px * var(--yap-reading-scale)) 0 calc(10px * var(--yap-reading-scale));
    font-size: calc(23px * var(--yap-reading-step));
    line-height: 1.2;
  }
  /* A section's heading: clearly above the text, with room before it. */
  .docmd > :global(h3) {
    margin: calc(28px * var(--yap-reading-scale)) 0 calc(8px * var(--yap-reading-scale));
    font-size: calc(21px * var(--yap-reading-step));
    line-height: 1.22;
  }
  .docmd > :global(h4) {
    margin: calc(20px * var(--yap-reading-scale)) 0 calc(6px * var(--yap-reading-scale));
    font-size: calc(15px * var(--yap-reading-scale));
    font-weight: 650;
    line-height: 1.35;
    color: var(--yap-paper-ink);
  }
  .docmd > :global(h5) {
    margin: calc(16px * var(--yap-reading-scale)) 0 calc(4px * var(--yap-reading-scale));
    font-size: 1em;
    font-weight: 650;
    color: var(--yap-paper-ink);
  }
  .docmd > :global(:is(h2, h3, h4, h5):first-child) {
    margin-top: 0;
  }
  /* Two headings in a row: less room between (Claude's file view). */
  .docmd > :global(:is(h2, h3, h4) + :is(h3, h4, h5)) {
    margin-top: calc(10px * var(--yap-reading-scale));
  }
  .docmd :global(strong) {
    font-weight: 650;
    color: var(--yap-paper-ink);
  }
  .docmd :global(ul),
  .docmd :global(ol) {
    padding-left: 1.35em;
  }
  .docmd :global(li + li) {
    margin-top: 0.35em;
  }
  .docmd :global(li > ul),
  .docmd :global(li > ol) {
    margin: 0.35em 0 0;
  }
  .docmd :global(li::marker) {
    color: var(--yap-paper-muted);
  }
  .docmd :global(a.md-link) {
    color: var(--yap-link);
    text-decoration-thickness: 1px;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 45%, transparent);
  }
  .docmd :global(a.md-link:hover) {
    text-decoration-color: currentColor;
  }
  .docmd :global(:not(pre) > code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 0.88em;
    background: var(--yap-paper-s1);
    border-radius: 4px;
    padding: 0.1em 0.35em;
    box-decoration-break: clone;
  }
  /* (The page's code block is `pre.md-code`, the editor's a plain `pre`.) */
  .docmd :global(pre) {
    padding: 10px 12px;
    border-radius: 6px;
    background: var(--yap-paper-s1);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 0.86em;
    line-height: 1.55;
    overflow-x: auto;
  }
  .docmd :global(blockquote) {
    margin-left: 0;
    padding-left: 1em;
    border-left: 2px solid var(--yap-paper-s2-strong);
    color: var(--yap-paper-text-2);
  }
  .docmd :global(hr) {
    height: 0;
    margin: calc(26px * var(--yap-reading-scale)) 0;
    border: none;
    border-top: 1px solid var(--yap-paper-s2);
  }
  /* Checklist items: the box where the bullet was (the page's is still,
     the editor's ticks); a ticked one quieter, struck through. */
  .docmd :global(li.task) {
    position: relative;
    list-style: none;
  }
  .docmd :global(li.task > input) {
    position: absolute;
    top: 0.34em;
    left: -1.3em;
    width: 0.92em;
    height: 0.92em;
    margin: 0;
    accent-color: var(--yap-paper-ink);
  }
  .docmd :global(li.task > input:not([disabled])) {
    cursor: pointer;
  }
  .docmd :global(li.task:has(> input:checked)) {
    color: var(--yap-paper-muted);
    text-decoration: line-through;
    text-decoration-color: var(--yap-paper-faint);
  }
  /* Tables: cells filled, no rules — the header row a shade stronger; a
     wide table scrolls inside itself (the page's `.md-table`, the editor's
     `.tableWrapper`), never the page. */
  .docmd :global(:is(.md-table, .tableWrapper)) {
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
  }
  .docmd :global(:is(.md-table, .tableWrapper) table) {
    width: 100% !important;
    border-collapse: separate;
    border-spacing: 2px;
    font-size: 0.93em;
    line-height: 1.5;
    font-variant-numeric: tabular-nums lining-nums;
  }
  .docmd :global(:is(.md-table, .tableWrapper) :is(th, td)) {
    min-width: 3em;
    padding: 6px 10px;
    border: none;
    border-radius: 3px;
    text-align: left;
    vertical-align: top;
    overflow-wrap: break-word;
  }
  .docmd :global(:is(.md-table, .tableWrapper) th) {
    background: var(--yap-paper-s2);
    font-weight: 600;
    color: var(--yap-paper-ink);
    vertical-align: bottom;
    text-wrap: balance;
  }
  .docmd :global(:is(.md-table, .tableWrapper) td) {
    background: var(--yap-paper-s1);
  }
  .docmd :global(.tableWrapper :is(th, td) > p) {
    margin: 0;
  }
  /* A header row of empty cells (a table written without one) isn't shown. */
  .docmd :global(.md-table thead:not(:has(th:not(:empty)))) {
    display: none;
  }
  .docmd :global(.md-table .c) {
    text-align: center;
  }
  .docmd :global(.md-table .r) {
    text-align: right;
  }
  /* A change Chat just made: its blocks washed in the accent a moment
     (Claude's document view marks a landed change so). Reduced motion: the
     wash, no fade. */
  .docmd > :global([data-changed]) {
    border-radius: 4px;
    animation: landed 1.6s ease-out;
  }
  @keyframes landed {
    0%,
    25% {
      background-color: var(--yap-primary-wash);
      box-shadow: 0 0 0 4px var(--yap-primary-wash);
    }
    100% {
      background-color: transparent;
      box-shadow: 0 0 0 4px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .docmd > :global([data-changed]) {
      animation: none;
      background-color: var(--yap-primary-wash);
      box-shadow: 0 0 0 4px var(--yap-primary-wash);
    }
  }
  :global(:root[data-motion='reduced']) .docmd > :global([data-changed]) {
    animation: none;
    background-color: var(--yap-primary-wash);
    box-shadow: 0 0 0 4px var(--yap-primary-wash);
  }
  /* "Ask about this" / "Change this…": a small pill over the selection. */
  .selpop {
    position: absolute;
    z-index: 4;
    display: flex;
    align-items: center;
    padding: 3px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9px;
    background: var(--yap-paper-card);
    box-shadow: var(--yap-shadow-popover);
    transform: translate(-50%, -100%);
    white-space: nowrap;
  }
  .selpop.below {
    transform: translate(-50%, 0);
  }
  .selpop button {
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .selpop button:hover {
    background: var(--yap-paper-s1);
  }
  .selpop button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .selsep {
    width: 1px;
    height: 16px;
    background: var(--yap-paper-s2);
  }
  /* The sections' rail: in the page's left margin, staying put as it
     scrolls; a tick per section (the one being read longer, in the accent),
     the list of them on hover or a click. */
  .docrail {
    position: absolute;
    z-index: 2;
    top: 18px;
    left: 2px;
  }
  .railbtn {
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding: 6px 5px;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    cursor: pointer;
  }
  .railbtn:hover,
  .railbtn[aria-expanded='true'] {
    background: var(--yap-paper-s1);
  }
  .railbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tick {
    display: block;
    width: 10px;
    height: 2px;
    border-radius: 2px;
    background: var(--yap-paper-faint);
  }
  .tick.on {
    width: 14px;
    height: 3px;
    background: var(--yap-primary);
  }
  .docoutline {
    position: absolute;
    top: 0;
    left: calc(100% + 4px);
    width: max-content;
    min-width: 180px;
    max-width: min(260px, 70vw);
    max-height: 60vh;
    overflow-y: auto;
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--yap-paper-card);
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    box-shadow: 0 6px 20px color-mix(in srgb, var(--yap-paper-ink) 14%, transparent);
  }
  .docoutline button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 12.5px;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
  }
  .docoutline button:hover {
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
  }
  .docoutline button.on {
    color: var(--yap-paper-ink);
    font-weight: 600;
  }
  .docoutline button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .otick {
    flex: 0 0 auto;
    width: 10px;
    height: 2px;
    border-radius: 2px;
    background: var(--yap-paper-faint);
  }
  .docoutline button.on .otick {
    width: 14px;
    height: 3px;
    background: var(--yap-primary);
  }
  .otext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* **Source**: the markdown, coloured, under a clear text box that takes
     the typing — the two share every measure, so the caret and the
     selection sit on the coloured words. */
  .srcscroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    background: var(--yap-paper);
  }
  .srcwrap {
    position: relative;
    min-height: 100%;
  }
  .srcmirror,
  .srcedit {
    box-sizing: border-box;
    margin: 0;
    padding: 14px 22px 48px;
    border: none;
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(12.5px * var(--yap-reading-step));
    line-height: 1.6;
    letter-spacing: normal;
    tab-size: 4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    word-break: normal;
  }
  .srcmirror {
    min-height: 100%;
    color: var(--yap-paper-ink);
    pointer-events: none;
  }
  .srcedit {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    resize: none;
    overflow: hidden;
    background: transparent;
    color: transparent;
    -webkit-text-fill-color: transparent;
    caret-color: var(--yap-paper-ink);
    outline: none;
  }
  /* (No focus ring round the whole view: the caret says where you are, as
     in an editor.) */
  .srcedit::selection {
    background: color-mix(in srgb, var(--yap-primary) 26%, transparent);
  }
  /* Colour only — never a bolder or slanted face, which could move a glyph
     out from under the text box's caret. */
  .srcmirror :global(.md-s-heading) {
    color: var(--yap-primary);
  }
  .srcmirror :global(.md-s-mark) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-strong) {
    color: var(--yap-paper-ink);
    text-decoration: underline 1px color-mix(in srgb, var(--yap-paper-ink) 25%, transparent);
    text-underline-offset: 3px;
  }
  .srcmirror :global(.md-s-em) {
    color: var(--yap-paper-text-2);
  }
  .srcmirror :global(.md-s-link) {
    color: var(--yap-link);
  }
  .srcmirror :global(.md-s-url) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-icode),
  .srcmirror :global(.md-s-code) {
    color: var(--yap-paper-text-2);
    background: var(--yap-paper-s1);
  }
  .srcmirror :global(.md-s-fence) {
    color: var(--yap-paper-muted);
  }
  .srcmirror :global(.md-s-quote) {
    color: var(--yap-paper-text-2);
  }
  .srcmirror :global(.md-s-comment) {
    color: var(--yap-paper-muted);
    background: var(--yap-paper-s1);
  }
</style>
