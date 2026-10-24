<script>
  // Settings → Privacy & data → Network connections (one level down): every
  // connection Yap made in the last 7 days, by purpose, then host — the
  // network receipts (src-tauri/src/net.rs). Hosts and purposes only, never
  // what was sent. Cloud and on-device alike: the list shows what each
  // choice sends where, and never claims that nothing leaves.
  //
  //   onback: back to Privacy & data.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import { askConfirm } from '../ui/contextMenu.svelte.js';
  import { toast } from '../ui/toast.svelte.js';

  let { onback } = $props();

  let summary = $state(null); // net::Summary
  let failed = $state(false);
  let filter = $state('all'); // all | leaves | pc
  let open = $state({}); // label → expanded

  async function load() {
    try {
      summary = await invoke('connections_summary');
      failed = false;
    } catch {
      failed = true;
    }
  }
  onMount(() => {
    load();
    // The store is written a few seconds after each connection.
    const un = listen('yap-connections', load);
    return () => un.then((f) => f());
  });

  const PLACE = { pc: 'This PC', network: 'Your network', internet: 'Internet' };
  const keep = (h) => filter === 'all' || (filter === 'pc' ? h.place === 'pc' : h.place !== 'pc');

  // The groups and their hosts the filter leaves, with their sums.
  const groups = $derived(
    (summary?.groups || [])
      .map((g) => {
        const hosts = g.hosts.filter(keep);
        const sum = (k) => hosts.reduce((n, h) => n + h[k], 0);
        const places = [...new Set(hosts.map((h) => h.place))];
        return {
          ...g,
          hosts,
          count: sum('count'),
          failed: sum('failed'),
          sent: sum('sent'),
          received: sum('received'),
          last: Math.max(0, ...hosts.map((h) => h.last)),
          places,
        };
      })
      .filter((g) => g.hosts.length > 0)
  );

  const week = $derived(summary?.week);
  const filters = $derived([
    { value: 'all', label: `All${week ? ` · ${week.count}` : ''}` },
    { value: 'leaves', label: `Left this PC${week ? ` · ${week.leaving}` : ''}` },
    { value: 'pc', label: `On this PC${week ? ` · ${week.local}` : ''}` },
  ]);

  function size(bytes) {
    if (!bytes) return '0 KB';
    const mb = bytes / (1024 * 1024);
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
    if (mb >= 1) return `${mb.toFixed(mb >= 10 ? 0 : 1)} MB`;
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }
  const plural = (n, one, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
  function when(secs) {
    if (!secs || !summary) return '';
    const d = new Date(secs * 1000);
    const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    if (secs >= summary.todayStart) return `today ${time}`;
    if (secs >= summary.todayStart - 86_400) return `yesterday ${time}`;
    return `${d.toLocaleDateString([], { weekday: 'short' })} ${time}`;
  }
  function line(x) {
    const parts = [plural(x.count, 'request')];
    if (x.failed) parts.push(`${x.failed} failed`);
    parts.push(`${size(x.sent)} sent`, `${size(x.received)} received`);
    return parts.join(' · ');
  }
  const status = (m) => (m.failed && !m.status ? 'no answer' : m.status ? `HTTP ${m.status}` : 'answered');

  function clearAll(e) {
    askConfirm({
      title: 'Clear the list of connections?',
      body: 'Yap forgets the connections it recorded. It keeps recording new ones.',
      confirmLabel: 'Clear list',
      busyLabel: 'Clearing…',
      returnFocus: e?.currentTarget ?? null,
      run: async () => {
        try {
          await invoke('connections_clear');
          open = {};
          await load();
        } catch (err) {
          toast({ title: "Couldn't clear the list", description: String(err), variant: 'destructive' });
          return false;
        }
      },
    });
  }
</script>

{#snippet kindIcon(kind)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if kind === 'ai'}
      <path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z" /><path d="M18 15l.8 2.2L21 18l-2.2.8L18 21l-.8-2.2L15 18l2.2-.8z" />
    {:else if kind === 'web'}
      <circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
    {:else if kind === 'download'}
      <path d="M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 20h14" />
    {:else if kind === 'update'}
      <path d="M20 11a8 8 0 0 0-14.6-4.5L4 8m0-4v4h4M4 13a8 8 0 0 0 14.6 4.5L20 16m0 4v-4h-4" />
    {:else if kind === 'account'}
      <circle cx="12" cy="8" r="4" /><path d="M4 20c1.5-3.5 4.5-5 8-5s6.5 1.5 8 5" />
    {:else if kind === 'calendar'}
      <rect x="4" y="5" width="16" height="15" rx="2" /><path d="M4 10h16M9 3v4M15 3v4" />
    {:else if kind === 'voice'}
      <path d="M5 10v4h3l4 4V6L8 10z" /><path d="M16 9a4 4 0 0 1 0 6M18.5 6.5a8 8 0 0 1 0 11" />
    {:else if kind === 'models'}
      <path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01" />
    {:else}
      <path d="M9 7V3M15 7V3M7 7h10v4a5 5 0 0 1-10 0zM12 16v5" />
    {/if}
  </svg>
{/snippet}

<button type="button" class="page-back" onclick={() => onback?.()}>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 6-6 6 6 6" /></svg>
  Privacy & data
</button>
<div class="page-h">
  <h1>Network connections</h1>
  <p>Every connection Yap made in the last 7 days: what it was for and where it went.</p>
</div>

<p class="explain" id="connections-explain">
  Yap turns your voice into text on this PC. Other features connect only when you use them or switch them on: the AI
  provider you picked, web search, a cloud voice, model downloads, update checks, your calendar and your account. Each
  connection is listed here with its purpose, its host and how much went each way. What was in it is never kept.
  <strong>On this PC</strong> means it never left the computer: the on-device AI model, a server of your own on this PC, or
  an app using the Local API.
</p>

{#if failed}
  <p class="set-note">Yap couldn't read its list of connections.</p>
{:else if summary}
  <div class="totals" role="group" aria-label="Totals">
    <div class="tile">
      <span class="tnum">{summary.today.count.toLocaleString()}</span>
      <span class="tlab">today · {summary.today.leaving.toLocaleString()} left this PC</span>
    </div>
    <div class="tile">
      <span class="tnum">{summary.week.count.toLocaleString()}</span>
      <span class="tlab">last 7 days · {size(summary.week.sent)} sent · {size(summary.week.received)} received</span>
    </div>
  </div>

  <Group title="By purpose" id="settings-connections-list" bare>
    <div class="bar">
      <Segmented bind:value={filter} options={filters} label="Show" />
      <Button variant="secondary" size="sm" disabled={!summary.week.count} onclick={clearAll}>Clear list…</Button>
    </div>

    {#if groups.length === 0}
      <p class="empty">
        {#if filter === 'pc'}No connections on this PC in the last 7 days.
        {:else if filter === 'leaves'}Nothing left this PC in the last 7 days.
        {:else}No connections in the last 7 days.{/if}
      </p>
    {:else}
      <ul class="groups" aria-label="Connections by purpose">
        {#each groups as g (g.kind + g.label)}
          {@const id = `conn-${g.kind}-${g.label.replace(/[^a-z0-9]+/gi, '-')}`}
          <li class="grp">
            <button
              type="button"
              class="ghead"
              aria-expanded={!!open[g.label]}
              aria-controls={id}
              onclick={() => (open = { ...open, [g.label]: !open[g.label] })}
            >
              <span class="kind">{@render kindIcon(g.kind)}</span>
              <span class="gtext">
                <span class="glabel">{g.label}</span>
                <span class="gline">{line(g)} · last {when(g.last)}</span>
              </span>
              <span class="chips">
                {#each g.places as p (p)}
                  <span class="chip {p}">{PLACE[p]}</span>
                {/each}
              </span>
              <svg class="chev" class:open={open[g.label]} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
            </button>
            {#if open[g.label]}
              <ul class="hosts" {id} aria-label={`${g.label}: hosts`}>
                {#each g.hosts as h (h.host + h.place)}
                  <li class="host">
                    <div class="hrow">
                      <span class="hname">{h.host || 'unknown host'}</span>
                      <span class="chip {h.place}">{PLACE[h.place]}</span>
                    </div>
                    <div class="hline">{line(h)}</div>
                    <div class="hline">
                      {h.first === h.last ? when(h.last) : `${when(h.first)} to ${when(h.last)}`}{h.features.length
                        ? ` · for ${h.features.join(', ')}`
                        : ''}
                    </div>
                    {#if h.recent.length > 1}
                      <ul class="moments" aria-label="Latest">
                        {#each h.recent as m (m.at)}
                          <li>{when(m.last)} · {plural(m.count, 'request')}{m.failed ? ` · ${m.failed} failed` : ''} · {status(m)}</li>
                        {/each}
                      </ul>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </Group>
  <p class="set-note">
    Kept on this PC for 7 days in <code>connections.json</code>: hosts, purposes, times and sizes only. A
    download shows once it's finished.
  </p>
{/if}

<style>
  .explain {
    margin: 0 0 22px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-muted);
    max-width: 640px;
  }
  .explain strong {
    color: var(--yap-fg-80);
    font-weight: 600;
  }
  .totals {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    margin: 0 0 26px;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 14px;
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .tnum {
    font-size: 20px;
    font-weight: 650;
    color: var(--yap-fg);
    font-variant-numeric: tabular-nums;
  }
  .tlab {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    margin: 4px 0 10px;
  }
  .empty {
    margin: 14px 0;
    font-size: 13px;
    color: var(--yap-muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .grp {
    border-top: 1px solid var(--yap-rule);
  }
  .grp:first-child {
    border-top: none;
  }
  .ghead {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto 16px;
    align-items: center;
    column-gap: 12px;
    width: 100%;
    min-height: 54px;
    padding: 10px 4px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    font: inherit;
    text-align: left;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .ghead:hover {
    background: var(--yap-raised-soft);
  }
  .ghead:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .kind {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: var(--yap-r);
    background: var(--yap-raised);
    color: var(--yap-fg-80);
  }
  .kind svg {
    width: 16px;
    height: 16px;
  }
  .gtext {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .glabel {
    font-size: 13.5px;
    font-weight: 560;
  }
  .gline,
  .hline {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .chip {
    padding: 2px 7px;
    border-radius: var(--yap-r-full);
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    border: 1px solid var(--yap-border-subtle);
    background: var(--yap-s2);
    color: var(--yap-fg-80);
  }
  .chip.pc {
    color: var(--yap-success-text);
  }
  .chip.internet {
    color: var(--yap-fg);
    border-color: var(--yap-border);
  }
  .chev {
    width: 14px;
    height: 14px;
    color: var(--yap-fg-62);
    transition: transform var(--yap-dur) ease;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .hosts {
    margin: 0 0 12px 40px;
    padding: 2px 0 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .host {
    padding: 10px 12px;
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .hrow {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 3px;
  }
  .hname {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12.5px;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .moments {
    margin-top: 6px;
    padding-left: 10px;
    border-left: 2px solid var(--yap-rule);
    font-size: 11.5px;
    color: var(--yap-muted);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    color: var(--yap-fg-80);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
</style>
