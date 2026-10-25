<script>
  // Find in a long summary (Ctrl+F in the notepad's Summary tab and the
  // Notes view's): a slim bar over it — the words, "2 of 5", ↑ ↓ (Enter /
  // Shift+Enter), ✕ (Esc) — that lights every match and the one it's on,
  // scrolled into view. The summary is rendered markdown, so the matches are
  // painted with the CSS Custom Highlight API (no change to its HTML, which
  // its checkboxes map to lines); found again whenever the summary changes
  // under it (a tick, another window's edit).
  import { tick } from 'svelte';

  /**
   * `root()`: the rendered summary's element; `text`: its markdown (a new
   * one searches again); `look`: 'paper' (the notepad) | 'app' (Notes).
   */
  let { root, text = '', look = 'paper', open = $bindable(false) } = $props();

  let query = $state('');
  let hits = $state([]);
  let at = $state(0);
  let inputEl = $state(null);
  const painting = typeof CSS !== 'undefined' && 'highlights' in CSS && typeof Highlight !== 'undefined';

  /** Ctrl+F: open (or back to) the bar, its words selected. */
  export async function show() {
    open = true;
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  function find(q) {
    const el = root?.();
    const needle = q.trim().toLowerCase();
    const out = [];
    if (!el || !needle) return out;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const hay = n.nodeValue.toLowerCase();
      for (let i = hay.indexOf(needle); i >= 0; i = hay.indexOf(needle, i + needle.length)) {
        const r = new Range();
        r.setStart(n, i);
        r.setEnd(n, i + needle.length);
        out.push(r);
      }
    }
    return out;
  }

  function paint() {
    if (!painting) return;
    if (hits.length) CSS.highlights.set('yap-find', new Highlight(...hits));
    else CSS.highlights.delete('yap-find');
    if (hits[at]) CSS.highlights.set('yap-find-now', new Highlight(hits[at]));
    else CSS.highlights.delete('yap-find-now');
  }

  function reveal() {
    hits[at]?.startContainer?.parentElement?.scrollIntoView({ block: 'center' });
  }

  function search(keepPlace = false) {
    hits = find(query);
    if (!keepPlace || at >= hits.length) at = 0;
    paint();
    if (!keepPlace) reveal();
  }

  function step(by) {
    if (!hits.length) return;
    at = (at + by + hits.length) % hits.length;
    paint();
    reveal();
  }

  export function close() {
    open = false;
    query = '';
    hits = [];
    at = 0;
    if (painting) {
      CSS.highlights.delete('yap-find');
      CSS.highlights.delete('yap-find-now');
    }
  }

  function onkeydown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  // The summary re-rendered (its nodes are new): find again, in place.
  $effect(() => {
    void text;
    if (!open || !query.trim()) return;
    tick().then(() => search(true));
  });
  // Gone with its view: no highlight left behind.
  $effect(() => () => {
    if (painting) {
      CSS.highlights.delete('yap-find');
      CSS.highlights.delete('yap-find-now');
    }
  });
</script>

{#if open}
  <div class="find {look}" role="search" aria-label="Find in the summary">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" /></svg>
    <input
      bind:this={inputEl}
      bind:value={query}
      oninput={() => search()}
      {onkeydown}
      placeholder="Find in the summary"
      aria-label="Find in the summary"
      spellcheck="false"
    />
    <span class="count" aria-live="polite">{query.trim() ? (hits.length ? `${at + 1} of ${hits.length}` : 'No matches') : ''}</span>
    <button aria-label="Previous match" title="Previous (Shift+Enter)" disabled={!hits.length} onclick={() => step(-1)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 15l6-6 6 6" /></svg>
    </button>
    <button aria-label="Next match" title="Next (Enter)" disabled={!hits.length} onclick={() => step(1)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
    </button>
    <button aria-label="Close find" title="Close (Esc)" onclick={close}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7L7 17" /></svg>
    </button>
  </div>
{/if}

<style>
  .find {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0 0 8px;
    padding: 4px 6px 4px 10px;
    border-radius: 10px;
  }
  .find.paper {
    margin-inline: 12px;
    border: 1px solid var(--yap-paper-rule);
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
  }
  .find.app {
    border: 1px solid var(--yap-border);
    background: var(--yap-s1);
    color: var(--yap-fg);
  }
  .find > svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    opacity: 0.6;
  }
  input {
    flex: 1 1 auto;
    min-width: 0;
    height: 26px;
    padding: 0 4px;
    border: none;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .count {
    flex: 0 0 auto;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    opacity: 0.65;
    white-space: nowrap;
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .paper button:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .app button:hover:not(:disabled) {
    background: var(--yap-s2);
  }
  button :global(svg) {
    width: 15px;
    height: 15px;
  }
  button:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-paper-ring));
    outline-offset: 1px;
  }
  /* The matches, and the one the bar is on. */
  :global(::highlight(yap-find)) {
    background-color: color-mix(in srgb, var(--yap-live) 28%, transparent);
  }
  :global(::highlight(yap-find-now)) {
    background-color: var(--yap-live);
    color: var(--yap-paper);
  }
</style>
