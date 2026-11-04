<script>
  // A snippet's text (2026-10-31, rich snippets): the box, a small
  // formatting bar under it — bold, italic, a link, a bulleted or numbered
  // list; Ctrl+B / Ctrl+I / Ctrl+K in the box — beside its length, and,
  // once it has formatting, a preview of how it pastes: formatted (Outlook,
  // Word, Gmail, Teams, Slack…) or as plain text (terminals, code editors,
  // AI chat boxes). The formatting is Yap's markdown subset, kept as the
  // snippet's text (richSnippet.js; the paste is src-tauri's pasting/
  // rich.rs). The bar's edits go into the box as typing does, so Ctrl+Z
  // takes them back. The preview is drawn from the parsed text, never as
  // HTML, and its links aren't followed.
  import { tick } from 'svelte';
  import Segmented from '../../ui/Segmented.svelte';
  import { parse, isRich, toPlain, toggleWrap, insertLink, toggleList } from './richSnippet.js';

  /** `value`: the snippet's text (bound); `max`: its longest; `label`: whose snippet (the bar's name). */
  let { value = $bindable(''), max, rows = 2, placeholder = '', label = 'the snippet' } = $props();

  let box = $state(null);
  let view = $state('formatted');
  const rich = $derived(isRich(value));
  const blocks = $derived(rich ? parse(value) : []);
  const plain = $derived(rich && view === 'plain' ? toPlain(value) : '');
  /** Its length, as a person counts characters (an emoji is one). */
  const count = $derived([...String(value ?? '')].length);

  const EDITS = {
    bold: (s) => toggleWrap(s, '**'),
    italic: (s) => toggleWrap(s, '*'),
    link: insertLink,
    bullets: (s) => toggleList(s, false),
    numbers: (s) => toggleList(s, true),
  };

  /** One of the bar's edits on the box's selection. */
  async function edit(name) {
    if (!box) return;
    const before = box.value;
    const next = EDITS[name]({ text: before, start: box.selectionStart, end: box.selectionEnd });
    if (next.text.length > max) return;
    box.focus();
    if (next.text !== before) {
      // Only what changed, typed over (the box keeps it in its Undo).
      let a = 0;
      while (a < before.length && a < next.text.length && before[a] === next.text[a]) a++;
      let z = 0;
      while (z < before.length - a && z < next.text.length - a && before[before.length - 1 - z] === next.text[next.text.length - 1 - z]) z++;
      box.setSelectionRange(a, before.length - z);
      const typed = document.execCommand?.('insertText', false, next.text.slice(a, next.text.length - z));
      if (!typed || box.value !== next.text) {
        box.value = next.text;
        value = next.text;
      }
    }
    await tick();
    box.setSelectionRange(next.start, next.end);
  }

  function keys(e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
    const name = { b: 'bold', i: 'italic', k: 'link' }[e.key.toLowerCase()];
    if (!name) return;
    e.preventDefault();
    e.stopPropagation();
    edit(name);
  }

  // The box keeps its selection while a button is pressed.
  const keep = (e) => e.preventDefault();

  const TOOLS = [
    { id: 'bold', label: 'Bold', title: 'Bold (Ctrl+B)', keys: 'Control+B', paths: ['M4.5 2.5h4.2a2.8 2.8 0 0 1 0 5.6H4.5zM4.5 8.1h5a2.7 2.7 0 0 1 0 5.4h-5z'] },
    { id: 'italic', label: 'Italic', title: 'Italic (Ctrl+I)', keys: 'Control+I', paths: ['M7 2.5h5M4 13.5h5M10 2.5l-4 11'] },
    {
      id: 'link',
      label: 'Link',
      title: 'Link (Ctrl+K)',
      keys: 'Control+K',
      paths: ['M6.8 9.2a2.6 2.6 0 0 0 3.7 0l2.2-2.2a2.6 2.6 0 0 0-3.7-3.7l-.9.9M9.2 6.8a2.6 2.6 0 0 0-3.7 0L3.3 9a2.6 2.6 0 0 0 3.7 3.7l.9-.9'],
    },
    { id: 'bullets', label: 'Bulleted list', title: 'Bulleted list', paths: ['M6.5 4h7M6.5 8h7M6.5 12h7'], dots: [4, 8, 12] },
    { id: 'numbers', label: 'Numbered list', title: 'Numbered list', paths: ['M7 4h6.5M7 8h6.5M7 12h6.5M2.6 2.8l.9-.5v3.4M2.3 9.2a1 1 0 0 1 1.9.3c0 .9-1.9 1.4-1.9 2.2h2'] },
  ];
  /** The bar is one tab stop: ← → (Home, End) move along it. */
  let toolAt = $state(0);
  function toolKeys(e) {
    const step = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
    const to = step ? (toolAt + step + TOOLS.length) % TOOLS.length : { Home: 0, End: TOOLS.length - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    toolAt = to;
    e.currentTarget.querySelectorAll('button')[to]?.focus();
  }
</script>

<!-- (On one line: whitespace here would show between the runs.) -->
<!-- prettier-ignore -->
{#snippet inline(nodes)}{#each nodes as n, i (i)}{#if n.type === 'text'}{n.text}{:else if n.type === 'bold'}<b>{@render inline(n.children)}</b>{:else if n.type === 'italic'}<i>{@render inline(n.children)}</i>{:else}<span class="pv-link" title={n.url}>{#if n.children.length}{@render inline(n.children)}{:else}{n.url.replace(/^mailto:/i, '')}{/if}</span>{/if}{/each}{/snippet}

<div class="snip-text">
  <textarea
    bind:this={box}
    bind:value
    {placeholder}
    aria-label="Text Yap types"
    {rows}
    maxlength={max}
    onkeydown={keys}
  ></textarea>
  <div class="snip-tools">
    <div class="fmt" role="toolbar" aria-label={`Format ${label}`} tabindex="-1" onkeydown={toolKeys}>
      {#each TOOLS as t, i (t.id)}
        <button
          type="button"
          aria-label={t.label}
          title={t.title}
          aria-keyshortcuts={t.keys}
          tabindex={i === toolAt ? 0 : -1}
          onmousedown={keep}
          onfocus={() => (toolAt = i)}
          onclick={() => edit(t.id)}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            {#each t.paths as d (d)}<path {d} />{/each}
            {#each t.dots ?? [] as cy (cy)}<circle cx="3" {cy} r=".9" class="dot" />{/each}
          </svg>
        </button>
      {/each}
    </div>
    {#if value}
      <span class="snip-count" class:full={count >= max} title={`${count.toLocaleString()} of at most ${max.toLocaleString()} characters`}>
        {count.toLocaleString()} {count === 1 ? 'character' : 'characters'}
      </span>
    {/if}
  </div>
  {#if rich}
    <div class="snip-preview" role="group" aria-label="How it pastes">
      <div class="pv-head">
        <span class="pv-title">Pastes as</span>
        <Segmented
          bind:value={view}
          options={[
            { value: 'formatted', label: 'Formatted' },
            { value: 'plain', label: 'Plain text' },
          ]}
          label="Preview"
        />
      </div>
      {#if view === 'formatted'}
        <div class="pv-body" data-testid="snippet-preview">
          {#each blocks as b, i (i)}
            {#if b.type === 'line'}
              {#if b.inlines.length}<div class="pv-line">{@render inline(b.inlines)}</div>{:else}<div class="pv-line pv-blank" aria-hidden="true"></div>{/if}
            {:else if b.ordered}
              <ol start={b.start}>
                {#each b.items as item, k (k)}<li>{@render inline(item)}</li>{/each}
              </ol>
            {:else}
              <ul>
                {#each b.items as item, k (k)}<li>{@render inline(item)}</li>{/each}
              </ul>
            {/if}
          {/each}
        </div>
        <p class="pv-note">In mail, documents and chats, such as Outlook, Word, Gmail, Teams and Slack.</p>
      {:else}
        <pre class="pv-body pv-plain" data-testid="snippet-preview-plain">{plain}</pre>
        <p class="pv-note">In terminals, code editors and AI chat boxes, and apps that only take plain text.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .snip-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  textarea {
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    color: var(--yap-fg);
    padding: 6px 8px;
    font: inherit;
    font-size: 13px;
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    line-height: 1.45;
    min-height: 34px;
  }
  textarea:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .snip-tools {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 22px;
  }
  /* Quiet until the row is hovered or has the focus. */
  .fmt {
    display: flex;
    gap: 1px;
    opacity: 0.55;
    transition: opacity var(--yap-dur) ease;
  }
  :global(.snip-row:hover) .fmt,
  :global(.snip-row:focus-within) .fmt {
    opacity: 1;
  }
  .fmt button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .fmt button:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .fmt button:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .fmt svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .fmt svg .dot {
    fill: currentColor;
    stroke: none;
  }
  .snip-count {
    font-size: 11px;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .snip-count.full {
    color: var(--yap-danger);
  }

  .snip-preview {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    padding: 6px 10px 8px;
  }
  .pv-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 4px;
  }
  .pv-title {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--yap-muted-55);
  }
  .pv-head :global(.seg-btn) {
    padding: 2px 9px;
    font-size: 11px;
  }
  .pv-body {
    font-size: 13px;
    line-height: 1.45;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .pv-line {
    white-space: pre-wrap;
  }
  .pv-blank {
    height: 1.45em;
  }
  .pv-body ul,
  .pv-body ol {
    margin: 2px 0;
    padding-left: 22px;
  }
  .pv-link {
    color: var(--yap-link);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: help;
  }
  .pv-plain {
    margin: 0;
    font-family: inherit;
    white-space: pre-wrap;
  }
  .pv-note {
    margin: 6px 0 0;
    font-size: 11px;
    line-height: 1.4;
    color: var(--yap-muted-55);
  }
</style>
