<script>
  // Settings → Connectors with the connectors preview on, in the Claude app's
  // layout (its Settings → Customize → Connectors; the design's §10):
  //
  //   Connectors                          [ Search connectors ] [+ Add ▾]
  //   Give Chat and the Voice Assistant tools from your apps and services…
  //   ( Yours ) ( Discover )
  //
  // - **Yours**: Yap's calendars first (their cards, as before:
  //   ConnectorsSection.svelte), then a row per connector (ConnectorRow:
  //   status, the button it asks for, its switch, ⋯), each opening its page
  //   (ConnectorDetail).
  // - **Discover**: Yap's own list (DiscoverGrid, ConnectorSheet).
  // - **+ Add ▾**: Custom connector… · Program on this PC… · Import from
  //   Claude Desktop… · Paste a setup… (AddDialog).
  // The search box filters Yours, or Discover. Statuses follow
  // `yap-connector-status` live (connectors.svelte.js). With the preview off
  // Settings shows the calendars' page instead (Settings.svelte).
  //
  //   goto: another Settings page (the AI apps (MCP) link, the calendars').
  import { onMount, untrack } from 'svelte';
  import ConnectorsSection from '../ConnectorsSection.svelte';
  import PillTabs from '../../ui/PillTabs.svelte';
  import ConnectorRow from './ConnectorRow.svelte';
  import ConnectorDetail from './ConnectorDetail.svelte';
  import DiscoverGrid from './DiscoverGrid.svelte';
  import ConnectorSheet from './ConnectorSheet.svelte';
  import AddDialog from './AddDialog.svelte';
  import { conn, loadConnectors, loadCatalog, signOut, removeConnector } from './connectors.svelte.js';
  import { entryOf, filterYours, joinArgs } from './catalog.js';
  import { openContextMenu, askConfirm } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';

  //   visit: counts Settings' visits to the page (its nav item, a link, a
  //   search result): each one comes back to Yours' list.
  let { goto = null, visit = 0 } = $props();

  let query = $state('');
  let addBtn = $state(null);
  let adding = $state(null); // { mode, preset?, returnFocus }
  let sheet = $state(null); // { entry, returnFocus }

  // Each visit: Yours (a search result lands on one of its rows: the
  // calendars, the list, + Add), the list read again. Settings is always
  // mounted, so closing and opening it again is a visit, not a mount.
  $effect(() => {
    visit;
    untrack(() => {
      conn.tab = 'yours';
      conn.open = null;
      query = '';
      loadConnectors();
    });
  });
  onMount(() => {
    if (!conn.catalog) loadCatalog();
  });

  const yours = $derived(filterYours(conn.list, query));
  const entryFor = (c) => (c?.catalog ? entryOf(conn.catalog, c.catalog) : null);
  const openId = $derived(conn.open && conn.list.some((c) => c.id === conn.open) ? conn.open : null);

  function open(id) {
    conn.open = id;
    adding = null;
    sheet = null;
    document.querySelector('.settings-shell .content')?.scrollTo?.({ top: 0 });
  }

  function addMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Custom connector…', icon: 'open', onselect: () => (adding = { mode: 'link', returnFocus: addBtn }) },
        { label: 'Program on this PC…', icon: 'play', onselect: () => (adding = { mode: 'program', returnFocus: addBtn }) },
        { separator: true },
        { label: 'Import from Claude Desktop…', icon: 'import', onselect: () => (adding = { mode: 'import', returnFocus: addBtn }) },
        { label: 'Paste a setup…', icon: 'paste', onselect: () => (adding = { mode: 'paste', returnFocus: addBtn }) },
      ],
      { label: 'Add a connector', row: addBtn }
    );
  }

  function pick(entry, el) {
    sheet = { entry, returnFocus: el };
  }

  /** Discover's Files: + Add's program form, filled in. */
  function program(entry) {
    sheet = null;
    adding = {
      mode: 'program',
      preset: {
        name: entry.name,
        command: entry.command,
        args: entry.args,
        catalog: entry.id,
        argsHint: `Add each folder it may use after these, "quoted" if it has spaces: ${joinArgs(entry.args)} "C:\\Users\\you\\Documents"`,
      },
      returnFocus: addBtn,
    };
  }

  async function doSignOut(c) {
    const r = await signOut(c);
    if (!r.ok) toast({ title: "Couldn't sign out", description: r.why, variant: 'destructive' });
    else if (!r.confirmed) toast({ title: `Signed out of ${c.name}`, description: `${c.name} didn't confirm it ended Yap's access: disconnect Yap on ${c.name}'s own page too.` });
  }

  function askRemove(c, el) {
    askConfirm({
      title: `Remove ${c.name}?`,
      body: `Its tools leave Yap${c.auth === 'oauth' ? `, and Yap signs out of ${c.name}` : ''}${c.hasSecrets ? ' and forgets its secrets' : ''}. You can add it again at any time.`,
      confirmLabel: 'Remove',
      busyLabel: 'Removing…',
      returnFocus: el,
      run: async () => {
        const r = await removeConnector(c);
        if (!r.ok) {
          toast({ title: `Couldn't remove ${c.name}`, description: r.why, variant: 'destructive' });
          return false;
        }
        return true;
      },
    });
  }

  function goMcp() {
    if (goto) goto('mcp');
    else window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'mcp' }));
  }
</script>

{#if openId}
  <ConnectorDetail id={openId} entry={entryFor(conn.list.find((c) => c.id === openId))} onback={() => (conn.open = null)} />
{:else}
  <div class="cpage" data-setting="connectors-page">
    <header class="top">
      <div class="titles">
        <h1>Connectors</h1>
        <p>
          Give Chat and the Voice Assistant tools from your apps and services. They connect over MCP, as in Claude and Cursor. To let other AI apps read your meetings instead, see
          <button class="inline" onclick={goMcp}>AI apps (MCP)</button>.
        </p>
      </div>
    </header>
    <div class="bar" data-setting="connectors-tabs">
      <PillTabs
        bind:value={conn.tab}
        options={[
          { value: 'yours', label: 'Yours' },
          { value: 'discover', label: 'Discover' },
        ]}
      />
      <div class="right">
        <div class="search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></svg>
          <input type="search" bind:value={query} placeholder={conn.tab === 'discover' ? 'Search connectors' : 'Search yours'} aria-label="Search connectors" autocomplete="off" spellcheck="false" data-setting="connectors-search" />
        </div>
        <button class="add" bind:this={addBtn} data-more aria-haspopup="menu" onclick={addMenu} data-setting="connector-add">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
          Add
          <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
        </button>
      </div>
    </div>

    {#if conn.tab === 'discover'}
      <DiscoverGrid {query} onpick={pick} onopen={open} />
    {:else}
      <section class="sec" aria-labelledby="yours-cal" id="settings-calendars">
        <h2 id="yours-cal" class="cap">Calendars</h2>
        <p class="fine">Reminders before each meeting, and notes with its name and who's in it. Read straight from this PC.</p>
        <ConnectorsSection {goto} mcpLink={false} />
      </section>

      <section class="sec" aria-labelledby="yours-list" id="settings-your-connectors">
        <h2 id="yours-list" class="cap">Your connectors</h2>
        {#if !conn.checked}
          <p class="fine">Loading…</p>
        {:else if conn.list.length === 0}
          <div class="empty" data-setting="your-connectors">
            <p><b>No connectors yet.</b> Add Notion, Linear, GitHub and more from Discover, or your own server with + Add.</p>
            <button class="btn" onclick={() => (conn.tab = 'discover')}>Discover connectors</button>
          </div>
        {:else if yours.length === 0}
          <p class="fine" data-setting="your-connectors">None of yours matches “{query.trim()}”.</p>
        {:else}
          <div class="list" role="list" aria-label="Your connectors" data-setting="your-connectors">
            {#each yours as c (c.id)}
              <div role="listitem">
                <ConnectorRow {c} entry={entryFor(c)} onopen={() => open(c.id)} onremove={(el) => askRemove(c, el)} onsignout={() => doSignOut(c)} />
              </div>
            {/each}
          </div>
        {/if}
      </section>
    {/if}
  </div>
{/if}

{#if adding}
  <AddDialog mode={adding?.mode} preset={adding?.preset ?? null} returnFocus={adding?.returnFocus} onopen={(id) => open(id)} onclose={() => (adding = null)} />
{/if}
{#if sheet}
  <ConnectorSheet entry={sheet?.entry} returnFocus={sheet?.returnFocus} onopen={(id) => open(id)} onprogram={program} onclose={() => (sheet = null)} />
{/if}

<style>
  .cpage {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .top h1 {
    margin: 0 0 6px;
    font-size: 22px;
    font-weight: 650;
    line-height: 1.2;
  }
  .top p {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .inline {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    width: 210px;
    padding: 0 10px;
    box-sizing: border-box;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg-62);
  }
  .search:focus-within {
    border-color: var(--yap-border-hover);
    box-shadow: var(--yap-focus);
  }
  .search svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
  }
  .search input {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    outline: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 32px;
    padding: 0 10px 0 9px;
    border: 1px solid var(--yap-ink);
    border-radius: var(--yap-r);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 620;
    cursor: pointer;
  }
  .add:hover {
    background: var(--yap-ink-hover);
  }
  .add svg {
    width: 14px;
    height: 14px;
  }
  .add .chev {
    width: 12px;
    height: 12px;
    opacity: 0.8;
  }
  .add:focus-visible,
  .inline:focus-visible,
  .btn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .sec {
    display: flex;
    flex-direction: column;
    gap: 8px;
    scroll-margin-top: 16px;
  }
  .cap {
    margin: 4px 0 0;
    font-size: 14px;
    font-weight: 640;
  }
  .fine {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .list {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--yap-rule);
    border-bottom: 1px solid var(--yap-rule);
  }
  .list > div + div {
    border-top: 1px solid var(--yap-rule);
  }
  .list :global(.row) {
    border-top: none;
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 16px 18px;
    border: 1px dashed var(--yap-border);
    border-radius: var(--yap-r-lg);
  }
  .empty p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .btn {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .btn:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
</style>
