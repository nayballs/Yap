<script>
  // The document beside the conversation (chat/documents.rs, chatDoc.svelte.js):
  // its title, what's being written ("Writing: Gear…", "Editing…"), and two
  // views (2026-10-31, after Claude Code's own markdown file view, none of
  // its code): **Preview**, the document as a page to read, and **Source**,
  // its markdown coloured and editable — saved as you type, the person's
  // words winning over Chat's writing (it took the old Edit box's place).
  // Find in the document (Ctrl+F, either view), and in ⋯: Copy as Markdown,
  // Export as Markdown…, Open in Notes. Undo (Chat's last change), expand
  // to the whole of Chat, ✕. Select words on the page and a small popover
  // offers "Ask about this" and "Change this…": the question goes to Chat's
  // box with the passage quoted (`onquote`). A change Chat makes lands with
  // its blocks marked a moment; each document's scroll is kept per view.
  import { tick } from 'svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { highlightMarkdown } from '../../core/markdownSource.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import SummaryFind from '../../components/SummaryFind.svelte';
  import { shownOffset, copyDocument, exportDocument } from './docTools.js';

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
    void doc.shown;
    void doc.view;
    if (!bodyEl) {
      outline = [];
      return;
    }
    tick().then(() => {
      outline = sections().map((h, i) => ({ i, text: h.textContent.trim() }));
      onScroll();
    });
  });

  /** The section being read: the last whose heading is near the top or above it. */
  function onScroll() {
    pop = null;
    if (bodyEl) doc.keepScroll(bodyEl.scrollTop, 'preview');
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
    if (!sel || sel.isCollapsed || !sel.rangeCount || !mdEl) return null;
    const range = sel.getRangeAt(0);
    const page = mdEl.closest('.docpage');
    if (!page?.contains(range.commonAncestorContainer)) return null;
    const text = sel.toString().replace(/\s+\n/g, '\n').trim();
    if (!text) return null;
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
    if (!doc.busy && (a || b)) {
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
    onquote?.({ noteId: doc.id, title: doc.title, text: p.text, fromLine: p.fromLine, toLine: p.toLine, mode });
  }

  // ---- a change that landed, marked ---------------------------------------------------

  // The page's blocks as they were last drawn (their HTML, lines aside), and
  // which document and change that was: the blocks new since are marked.
  let drawn = new Set();
  let drawnFor = null;
  let drawnLanded = 0;
  const plainHtml = (el) => el.outerHTML.replace(/ data-(line|changed)="[^"]*"/g, '');

  $effect(() => {
    void doc.shown;
    const landed = doc.landed;
    const id = doc.id;
    if (doc.view !== 'preview') return;
    tick().then(() => {
      if (!mdEl) return;
      const els = [...mdEl.children];
      if (drawnFor === id && landed !== drawnLanded && drawn.size) {
        const fresh = els.filter((el) => !drawn.has(plainHtml(el)) && !el.classList.contains('md-pending'));
        for (const el of fresh) {
          el.setAttribute('data-changed', '');
          setTimeout(() => el.removeAttribute('data-changed'), 1700);
        }
        // The first one, in view.
        if (fresh[0] && bodyEl) {
          const r = fresh[0].getBoundingClientRect();
          const b = bodyEl.getBoundingClientRect();
          if (r.bottom < b.top || r.top > b.bottom) fresh[0].scrollIntoView({ block: 'nearest', behavior: reduced() ? 'auto' : 'smooth' });
        }
      }
      drawn = new Set(els.map(plainHtml));
      drawnFor = id;
      drawnLanded = landed;
    });
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
  {#if status}
    <p class="docstatus" class:live={doc.busy} data-testid="chat-doc-status" aria-live="polite">
      {#if doc.busy}<span class="dot" aria-hidden="true"></span>{/if}{status}
    </p>
  {/if}
  {#if !doc.gone}
    <div class="docfind">
      <SummaryFind bind:this={finder} bind:open={finding} root={findRoot} text={doc.view === 'source' ? doc.draft : doc.shown} look="paper" label="Find in the document" />
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
        aria-busy={doc.busy ? 'true' : undefined}
        bind:this={bodyEl}
        onscroll={onScroll}
        onpointerup={pageUp}
        onkeyup={pageKey}
      >
        <article class="docpage" class:railed={outline.length >= RAIL_FROM}>
          <h1 class="docname">{doc.title || 'Untitled document'}</h1>
          <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
          <div class="docmd" bind:this={mdEl}>{@html renderMarkdown(doc.shown, { writing: doc.busy, lines: true })}</div>
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
  .docmd :global(pre.md-code) {
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
  .docmd :global(hr.md-rule) {
    margin: calc(26px * var(--yap-reading-scale)) 0;
    border-top: 1px solid var(--yap-paper-s2);
  }
  /* Tables: cells filled, no rules — the header row a shade stronger; a
     wide table scrolls inside itself (`.md-table`), never the page. */
  .docmd :global(.md-table) {
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
  }
  .docmd :global(.md-table table) {
    width: 100%;
    border-collapse: separate;
    border-spacing: 2px;
    font-size: 0.93em;
    line-height: 1.5;
    font-variant-numeric: tabular-nums lining-nums;
  }
  .docmd :global(.md-table th),
  .docmd :global(.md-table td) {
    min-width: 3em;
    padding: 6px 10px;
    border: none;
    border-radius: 3px;
    text-align: left;
    vertical-align: top;
    overflow-wrap: break-word;
  }
  .docmd :global(.md-table th) {
    background: var(--yap-paper-s2);
    font-weight: 600;
    color: var(--yap-paper-ink);
    vertical-align: bottom;
    text-wrap: balance;
  }
  .docmd :global(.md-table td) {
    background: var(--yap-paper-s1);
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
