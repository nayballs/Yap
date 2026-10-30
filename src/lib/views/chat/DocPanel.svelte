<script>
  // The document beside the conversation (chat/documents.rs, chatDoc.svelte.js):
  // its title, what's being written ("Writing: Gear…"), the document as
  // rendered markdown (tables, lists, headings; a section still to come is
  // a quiet line), or — Edit — a plain text box saved as you type, the
  // person's words winning over Chat's writing. Undo (Chat's last change),
  // Edit, expand to the whole of Chat, Open in Notes and ✕.
  import { tick } from 'svelte';
  import { renderMarkdown } from '../../core/markdown.js';

  /** `doc`: the panel's state; `cover`: over the conversation (expanded, or a narrow window); `narrow`: no room beside it. */
  let { doc, cover = false, narrow = false } = $props();

  let box = $state(null);
  let editBtn = $state(null);
  let bodyEl = $state(null);

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
    void doc.editing;
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
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches || document.documentElement.dataset.motion === 'reduced';
    h.scrollIntoView({ block: 'start', behavior: reduced ? 'auto' : 'smooth' });
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
    if (doc.busy && doc.writing != null) return doc.writing ? `Writing: ${doc.writing}…` : 'Writing…';
    const n = doc.pending.length;
    if (n && !doc.busy) return n === 1 ? '1 section not written yet' : `${n} sections not written yet`;
    return '';
  });

  // The box follows the document while you're not typing (Chat's sections
  // landing): the caret stays where it was in your words.
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

  async function toggleEdit() {
    doc.toggleEdit();
    await tick();
    if (doc.editing) box?.focus();
    else editBtn?.focus();
  }
</script>

<aside class="docpanel" class:cover aria-label="Document" data-testid="chat-doc">
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
        <button
          type="button"
          class="docbtn text"
          class:on={doc.editing}
          bind:this={editBtn}
          aria-pressed={doc.editing}
          onclick={toggleEdit}
          title={doc.editing ? 'Show the document' : 'Edit the document as text'}
        >{doc.editing ? 'Done' : 'Edit'}</button>
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
        <button type="button" class="docbtn" onclick={() => doc.openInNotes()} aria-label="Open in Notes" title="Open in Notes">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4 11 13" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
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
  {#if doc.gone}
    <p class="docgone">This document was deleted.</p>
  {:else if doc.editing}
    <!-- Saved as you type (merged with Chat's writing: your words win). -->
    <textarea
      class="docedit"
      bind:this={box}
      aria-label="Document text"
      spellcheck="true"
      oninput={(e) => doc.input(e.currentTarget.value)}
    ></textarea>
  {:else}
    <!-- A page to read, after Claude's own documents: a serif title and
         section headings, a reading-width column, filled table cells. -->
    <div class="docscroll">
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
      <div class="docbody" role="document" tabindex="0" aria-busy={doc.busy ? 'true' : undefined} bind:this={bodyEl} onscroll={onScroll}>
        <article class="docpage" class:railed={outline.length >= RAIL_FROM}>
          <h1 class="docname">{doc.title || 'Untitled document'}</h1>
          <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
          <div class="docmd">{@html renderMarkdown(doc.shown, { writing: doc.busy })}</div>
        </article>
      </div>
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
  .docbtn.text {
    padding: 0 10px;
  }
  .docbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .docbtn.on {
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
  }
  .docbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .docbtn svg {
    width: 15px;
    height: 15px;
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
  .docedit {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 12px 22px 28px;
    border: none;
    resize: none;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(12.5px * var(--yap-reading-step));
    line-height: 1.6;
    outline: none;
  }
  .docedit:focus-visible {
    box-shadow: inset 0 0 0 2px var(--yap-focus-ring);
  }
</style>
