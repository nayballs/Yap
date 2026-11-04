<script>
  // Settings search's results, in the content pane while there's a query
  // (2026-10-22 review #3, concept 3): grouped by page under "Page › Section",
  // the words found highlighted, a plain on/off setting switchable right
  // here, ↑/↓ to move (the box keeps the focus), Enter or a click to open the
  // page scrolled to the row.
  //
  //   query:    what's typed.
  //   groups:   groupResults(searchSettings(query)).
  //   selected: the index (in reading order) of the highlighted result.
  //   cfg:      Settings' config $state (the live switches).
  //   onopen:   (entry) → open its page at its row.
  import Toggle from '../ui/Toggle.svelte';
  import { highlight } from './settingsIndex.js';

  let { query, groups = [], selected = $bindable(0), cfg, onopen } = $props();

  const flat = $derived(groups.flatMap((g) => g.results));
  const pages = $derived(groups.length);
  let els = $state([]);
  // Keep the highlighted one in view as ↑/↓ move it.
  $effect(() => {
    els[selected]?.scrollIntoView?.({ block: 'nearest' });
  });
</script>

{#snippet marked(text)}
  {#each highlight(text, query) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
{/snippet}

<div class="results-h">
  <h1>Results for “{query.trim()}”</h1>
  <span class="n" aria-live="polite">
    {#if flat.length}
      {flat.length} {flat.length === 1 ? 'setting' : 'settings'} on {pages} {pages === 1 ? 'page' : 'pages'}
    {:else}
      No settings found
    {/if}
  </span>
</div>

{#if !flat.length}
  <p class="none">Nothing matches “{query.trim()}”. Try another word, like “microphone”, “shortcut” or “privacy”.</p>
{/if}

<div id="settings-search-results" role="listbox" aria-label="Settings found">
  {#each groups as g (g.page)}
    {#each g.results as r, i (r.page + r.anchor + r.label)}
      {@const n = flat.indexOf(r)}
      {#if i === 0 || r.section !== g.results[i - 1].section}
        <div class="crumb" role="presentation">
          {g.label}
          {#if r.section && r.section !== g.label}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
            {r.section}
          {/if}
        </div>
      {/if}
      <!-- (The search box handles the keys: ↑/↓/Enter.) -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="result"
        class:sel={n === selected}
        role="option"
        id={`settings-result-${n}`}
        aria-selected={n === selected}
        tabindex="-1"
        bind:this={els[n]}
        onclick={(e) => {
          if (e.target.closest('.live')) return;
          onopen?.(r);
        }}
        onmouseenter={() => (selected = n)}
      >
        <div class="text">
          <div class="label">{@render marked(r.label)}</div>
          {#if r.desc}<div class="desc">{@render marked(r.desc)}</div>{/if}
        </div>
        <div class="ctl">
          {#if n === selected}<span class="enter"><kbd>Enter</kbd> to open</span>{/if}
          {#if r.toggle && cfg && typeof cfg[r.toggle] === 'boolean'}
            <span class="live"><Toggle bind:checked={cfg[r.toggle]} ariaLabel={r.label} /></span>
          {:else}
            <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          {/if}
        </div>
      </div>
    {/each}
  {/each}
</div>

<div class="keys" aria-hidden="true">
  <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
  <span><kbd>Enter</kbd> open the setting</span>
  <span><kbd>Esc</kbd> clear</span>
</div>

<style>
  .results-h {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 6px;
  }
  .results-h h1 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 25px;
    font-weight: 560;
    color: var(--yap-fg);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    flex: 0 0 auto;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .none {
    margin: 18px 0 0;
    font-size: 13px;
    color: var(--yap-muted);
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 22px 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .crumb svg {
    width: 12px;
    height: 12px;
  }
  .result {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: 24px;
    align-items: center;
    padding: 11px 12px;
    margin: 0 -12px;
    border-radius: 10px;
    border-bottom: 1px solid var(--yap-rule);
    cursor: pointer;
  }
  .result:last-child {
    border-bottom-color: transparent;
  }
  .result.sel {
    background: var(--yap-s2);
    border-bottom-color: transparent;
    box-shadow: 0 0 0 1px var(--yap-border), var(--yap-shadow-sm);
  }
  .label {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .desc {
    margin-top: 2px;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  mark {
    background: rgba(246, 169, 60, 0.34);
    color: inherit;
    border-radius: 3px;
    padding: 0 1px;
  }
  .ctl {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
  }
  .enter {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .chev {
    width: 15px;
    height: 15px;
    color: var(--yap-fg-62);
  }
  kbd {
    display: inline-block;
    min-width: 12px;
    padding: 0 4px;
    font: inherit;
    font-size: 10.5px;
    font-weight: 600;
    line-height: 15px;
    text-align: center;
    color: var(--yap-fg-80);
    border: 1px solid var(--yap-border);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--yap-s1);
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    margin-top: 28px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .keys span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .keys kbd {
    margin-right: 2px;
  }
</style>
