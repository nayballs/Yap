<script>
  // Discover (Settings → Connectors; the design's §10.2): Yap's own list of
  // connectors it can use today (connector_catalog: bundled with Yap,
  // nothing fetched), as cards in the Claude app's grid — the mark, the
  // name, one line, "by …", **Tested with Yap** only where it's true, and +
  // (✓ once added). Then "Coming later" for the famous ones Yap can't sign
  // in to yet, each with Why?. The search box filters both. No registry
  // search in v1 (it needs a hosted copy of the MCP Registry, which is the
  // maintainer's to approve): a quiet line says more is coming.
  //
  //   query; onpick(entry, el): open its sheet; onopen(id): open a
  //   connector's page.
  import ConnectorLogo from './ConnectorLogo.svelte';
  import { conn, loadCatalog } from './connectors.svelte.js';
  import { filterCatalog } from './catalog.js';

  let { query = '', onpick, onopen } = $props();

  let why = $state({});

  $effect(() => {
    if (!conn.catalog) loadCatalog();
  });

  const entries = $derived(filterCatalog(conn.catalog?.connectors, query));
  const later = $derived(filterCatalog(conn.catalog?.later, query));
</script>

<div class="discover">
  {#if conn.catalogError}
    <p class="err" role="alert">{conn.catalogError}</p>
  {:else if !conn.catalog}
    <p class="fine">Loading…</p>
  {:else}
    {#if entries.length}
      <h2 class="cap" id="settings-discover-top" data-setting="discover">Top connectors</h2>
      <ul class="grid" aria-label="Connectors you can add">
        {#each entries as e (e.id)}
          <li>
            <div class="card" data-entry={e.id}>
              <button class="open" onclick={(ev) => onpick?.(e, ev.currentTarget)} aria-label={`${e.name}, by ${e.publisher}${e.tested ? ', tested with Yap' : ''}${e.added ? ', added' : ''}`}>
                <ConnectorLogo logo={e.logo} color={e.color} name={e.name} size={40} />
                <span class="text">
                  <span class="name">{e.name}</span>
                  <span class="blurb">{e.blurb}</span>
                  <span class="by">
                    by {e.publisher}
                    {#if e.tested}<span class="tested">Tested with Yap</span>{/if}
                  </span>
                </span>
              </button>
              {#if e.added}
                <button class="plus added" aria-label={`${e.name} is added: open it`} title="Added" onclick={() => onopen?.(e.added)}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg>
                </button>
              {:else}
                <button class="plus" aria-label={`Add ${e.name}`} title="Add" onclick={(ev) => onpick?.(e, ev.currentTarget)}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    {#if later.length}
      <h2 class="cap" data-setting="discover-later">Coming later</h2>
      <ul class="grid" aria-label="Coming later">
        {#each later as l (l.id)}
          <li>
            <div class="card later" data-later={l.id}>
              <div class="open static">
                <ConnectorLogo logo={l.logo} color={l.color} name={l.name} size={40} />
                <span class="text">
                  <span class="name">{l.name}</span>
                  <span class="blurb">{l.blurb}</span>
                  {#if why[l.id]}<span class="why">{l.why}</span>{/if}
                </span>
              </div>
              <button class="whybtn" aria-expanded={!!why[l.id]} onclick={() => (why[l.id] = !why[l.id])}>{why[l.id] ? 'Hide' : 'Why?'}</button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    {#if !entries.length && !later.length}
      <p class="fine">Nothing in Yap's list matches “{query.trim()}”. Any other MCP server: + Add, then Custom connector.</p>
    {/if}

    <p class="quiet" data-setting="discover-more">
      More coming. This is Yap's own list of what it can connect to today; searching the whole MCP Registry comes later. Any other server works through + Add.
    </p>
  {/if}
</div>

<style>
  .discover {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .cap {
    margin: 8px 0 2px;
    font-size: 14px;
    font-weight: 640;
    scroll-margin-top: 16px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .card {
    position: relative;
    display: flex;
    align-items: flex-start;
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    transition: border-color var(--yap-dur) ease;
  }
  .card:hover:not(.later) {
    border-color: var(--yap-border-hover);
  }
  .card.later {
    background: var(--yap-s1);
    box-shadow: none;
  }
  .open {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 14px 4px 14px 14px;
    border: none;
    border-radius: var(--yap-r-lg);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .open.static {
    cursor: default;
  }
  .card.later :global(.tile) {
    filter: grayscale(0.6);
    opacity: 0.7;
  }
  .open:focus-visible,
  .plus:focus-visible,
  .whybtn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .name {
    font-size: 14px;
    font-weight: 640;
  }
  .blurb {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-fg-80);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .by {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .tested {
    padding: 0 7px;
    border-radius: 99px;
    border: 1px solid var(--yap-border);
    color: var(--yap-fg-80);
    font-size: 11px;
    font-weight: 600;
    line-height: 17px;
  }
  .why {
    font-size: 12px;
    line-height: 1.45;
    color: var(--yap-fg-80);
  }
  .plus {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    margin: 12px 12px 0 0;
    border: 1px solid var(--yap-border);
    border-radius: 99px;
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    cursor: pointer;
  }
  .plus:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .plus.added {
    border-color: transparent;
    background: var(--yap-raised);
    color: var(--yap-success-text);
  }
  .plus svg {
    width: 15px;
    height: 15px;
  }
  .whybtn {
    flex: 0 0 auto;
    margin: 12px 12px 0 0;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: 99px;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .quiet,
  .fine {
    margin: 6px 0 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .err {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
</style>
