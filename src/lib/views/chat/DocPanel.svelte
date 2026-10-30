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
    <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="docbody" role="document" tabindex="0" aria-busy={doc.busy ? 'true' : undefined}>{@html renderMarkdown(doc.shown, { writing: doc.busy })}</div>
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
  .doctitle {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--yap-font-display);
    font-size: 15px;
    font-weight: 650;
    color: var(--yap-paper-ink);
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
  /* Reading text: Settings → General → Appearance (app.css). */
  .docbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 22px 28px;
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.65 * var(--yap-reading-leading));
    overflow-wrap: anywhere;
  }
  .docbody:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .docbody :global(h2) {
    margin: 18px 0 6px;
    font-size: calc(15px * var(--yap-reading-scale));
    color: var(--yap-paper-ink);
  }
  .docbody :global(h3) {
    margin: 14px 0 4px;
    font-size: calc(13.5px * var(--yap-reading-scale));
    color: var(--yap-paper-ink);
  }
  .docbody :global(h4),
  .docbody :global(h5) {
    margin: 12px 0 4px;
    font-size: calc(13px * var(--yap-reading-scale));
  }
  .docbody :global(p) {
    margin: 6px 0;
  }
  .docbody :global(ul),
  .docbody :global(ol) {
    margin: 6px 0;
    padding-left: 20px;
  }
  .docbody :global(li) {
    margin: 2px 0;
  }
  .docbody :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(12px * var(--yap-reading-step));
    background: var(--yap-paper-s2);
    border-radius: 4px;
    padding: 1px 5px;
  }
  .docbody :global(.md-table table) {
    border-collapse: collapse;
    font-size: calc(12.5px * var(--yap-reading-step));
  }
  .docbody :global(.md-table th),
  .docbody :global(.md-table td) {
    padding: 4px 8px;
    border: 1px solid var(--yap-paper-rule);
    text-align: left;
    vertical-align: top;
  }
  .docbody :global(.md-table th) {
    background: var(--yap-paper-s2);
    font-weight: 650;
  }
  .docbody :global(.md-table .c) {
    text-align: center;
  }
  .docbody :global(.md-table .r) {
    text-align: right;
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
