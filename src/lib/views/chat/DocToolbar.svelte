<script>
  // The document's toolbar (2026-10-31; after Claude's own documents', none
  // of its code): over the page while it's Preview, a row of small buttons —
  // text style (Paragraph, Heading 1–3), code, a table (in a table: add or
  // delete a row or column, delete it), a checklist, and + (a divider, a
  // quote, a bulleted or numbered list, a code block). Each acts on the
  // page's editor (docEditor.js `run`), which keeps the note markdown; the
  // ones on at the caret are pressed. Off while Chat is writing (the page
  // is being written) and before the editor has loaded.
  //
  // No image button: markdown.js shows no images (a model's would make Yap
  // fetch an address), and a picture of the person's own would need a
  // store beside the note, its export and its sync — not a toolbar button.
  // No tab switcher either: a Chat document is one note, an answer's.
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';

  /**
   * `ed`: the page's editor (null while it loads or Chat writes); `state`:
   * what's on at its caret; `writing`: Chat is writing the document.
   */
  let { ed = null, state = {}, writing = false } = $props();

  const off = $derived(!ed || writing);
  const why = $derived(writing ? 'Chat is writing the document' : '');

  const STYLES = [
    { id: 'paragraph', label: 'Paragraph' },
    { id: 1, label: 'Heading 1' },
    { id: 2, label: 'Heading 2' },
    { id: 3, label: 'Heading 3' },
  ];
  const styleName = $derived(STYLES.find((s) => s.id === state.style)?.label ?? (typeof state.style === 'number' ? `Heading ${state.style}` : 'Text style'));

  /** A menu under its button (it takes the focus, and gives it back if nothing's chosen). */
  function menu(e, items, label) {
    const at = e.currentTarget;
    openContextMenu({ type: 'keydown', currentTarget: at, preventDefault() {}, stopPropagation() {} }, items, { label, row: at });
  }

  function styleMenu(e) {
    menu(
      e,
      STYLES.map((s) => ({ label: s.label, checked: state.style === s.id, onselect: () => ed?.run('style', s.id) })),
      'Text style'
    );
  }

  function tableMenu(e) {
    if (!state.table) {
      ed?.run('table');
      return;
    }
    menu(
      e,
      [
        { label: 'Add a row below', onselect: () => ed?.run('rowAfter') },
        { label: 'Add a column to the right', onselect: () => ed?.run('columnAfter') },
        { separator: true },
        { label: 'Delete this row', onselect: () => ed?.run('deleteRow') },
        { label: 'Delete this column', onselect: () => ed?.run('deleteColumn') },
        { label: 'Delete the table', icon: 'trash', danger: true, onselect: () => ed?.run('deleteTable') },
      ],
      'Table'
    );
  }

  function insertMenu(e) {
    menu(
      e,
      [
        { label: 'Divider', onselect: () => ed?.run('rule') },
        { label: 'Quote', checked: state.quote ? true : undefined, onselect: () => ed?.run('quote') },
        { label: 'Bulleted list', checked: state.bullets && !state.checklist ? true : undefined, onselect: () => ed?.run('bullets') },
        { label: 'Numbered list', checked: state.numbers ? true : undefined, onselect: () => ed?.run('numbers') },
        { label: 'Code block', checked: state.codeBlock ? true : undefined, onselect: () => ed?.run('codeBlock') },
      ],
      'Insert'
    );
  }

  // The editor keeps its selection while a button is pressed.
  const keep = (e) => e.preventDefault();
</script>

<div class="doctools" role="toolbar" aria-label="Format the document" data-testid="chat-doc-toolbar" title={why || undefined}>
  <button type="button" class="tool wide" disabled={off} aria-haspopup="menu" aria-label={`Text style: ${styleName}`} title="Text style" onmousedown={keep} onclick={styleMenu}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h10M8 6v13M14 11h7M17.5 11v8" /></svg>
    <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
  </button>
  <button type="button" class="tool" disabled={off} aria-pressed={!!state.code} aria-label="Code" title="Code (Ctrl+E)" onmousedown={keep} onclick={() => ed?.run('code')}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 7-5 5 5 5M16 7l5 5-5 5" /></svg>
  </button>
  <span class="sep" aria-hidden="true"></span>
  <button
    type="button"
    class="tool"
    disabled={off}
    aria-pressed={!!state.table}
    aria-haspopup={state.table ? 'menu' : undefined}
    aria-label={state.table ? 'Table' : 'Insert a table'}
    title={state.table ? 'Table: rows and columns' : 'Insert a table'}
    onmousedown={keep}
    onclick={tableMenu}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="M3.5 9.5h17M3.5 14.5h17M9.5 9.5v10M15 9.5v10" /></svg>
  </button>
  <button type="button" class="tool" disabled={off} aria-pressed={!!state.checklist} aria-label="Checklist" title="Checklist (Ctrl+Enter ticks an item)" onmousedown={keep} onclick={() => ed?.run('checklist')}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4.5" width="6" height="6" rx="1.5" /><path d="m5 7.6 1.4 1.4L8.7 6.4" /><rect x="3.5" y="13.5" width="6" height="6" rx="1.5" /><path d="M13 7.5h7.5M13 16.5h7.5" /></svg>
  </button>
  <span class="sep" aria-hidden="true"></span>
  <button type="button" class="tool wide" disabled={off} aria-haspopup="menu" aria-label="Insert" title="Insert a divider, quote, list or code block" onmousedown={keep} onclick={insertMenu}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
    <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg>
  </button>
  {#if writing}<span class="toolnote">Chat is writing…</span>{/if}
</div>

<style>
  /* A slim row over the page, its buttons the header's size, quiet until
     hovered or on (Claude's documents' toolbar, measured: 28px buttons,
     a hairline under the row). */
  .doctools {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 36px;
    padding: 3px 12px;
    border-bottom: 1px solid var(--yap-paper-rule);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tool {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 1px;
    min-width: 28px;
    height: 28px;
    padding: 0 4px;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .tool.wide {
    padding: 0 3px 0 5px;
  }
  .tool svg {
    width: 16px;
    height: 16px;
  }
  .tool .chev {
    width: 11px;
    height: 11px;
    opacity: 0.8;
  }
  .tool:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .tool[aria-pressed='true'] {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .tool:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tool:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .sep {
    flex: 0 0 auto;
    width: 1px;
    height: 16px;
    margin: 0 4px;
    background: var(--yap-paper-s2);
  }
  .toolnote {
    margin-left: 8px;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
    white-space: nowrap;
  }
</style>
