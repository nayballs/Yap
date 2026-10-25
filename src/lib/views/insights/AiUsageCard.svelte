<script>
  // Insights → AI usage (2026-10-25; ai/usage.rs `ai_usage_summary`): every
  // AI call's tokens, counted on this PC from each answer's own count —
  // today, the last 7 days and the last 30 days; by model (on this PC or
  // the cloud, an estimated cost for a cloud model whose price is known:
  // usage/prices.rs) and by feature; the on-device model's speed by day.
  // Chat's context ring's "See all" brings it into view (`focus`).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick, untrack } from 'svelte';
  import { fmtCount, fmtDay, splitLine, featureLabel, modelRow, costLine, speedLine, tokensWord } from '../../core/aiUsage.js';

  /**
   * `focus`: 'ai-usage' brings the card into view once it and the stats
   * above it are drawn (`ready`: Insights' own data is in); `onfocused()`
   * then.
   */
  let { focus = null, onfocused = null, ready = true } = $props();

  let summary = $state(null);
  let period = $state('week');
  let cardEl = $state(null);

  const PERIODS = [
    { id: 'today', label: 'Today' },
    { id: 'week', label: 'Last 7 days' },
    { id: 'month', label: 'Last 30 days' },
  ];

  const shown = $derived(summary?.[period] ?? null);
  const maxModel = $derived(Math.max(1, ...(shown?.models ?? []).map((m) => m.tokens)));
  const maxFeature = $derived(Math.max(1, ...(shown?.features ?? []).map((f) => f.tokens)));
  // The last 30 days, one bar a day (no speed that day: an empty one).
  const speedDays = $derived.by(() => {
    const by = new Map((summary?.speed ?? []).map((d) => [d.day, d]));
    const out = [];
    const now = new Date();
    for (let i = 29; i >= 0; i--) {
      const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
      const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
      out.push({ key, date: d, speed: by.get(key) ?? null });
    }
    return out;
  });
  const maxSpeed = $derived(Math.max(1, ...(summary?.speed ?? []).map((d) => d.perSecond)));
  const latestSpeed = $derived(summary?.speed?.at(-1) ?? null);

  async function load() {
    try {
      summary = await invoke('ai_usage_summary');
    } catch {
      /* keep the last */
    }
  }

  // Asked to come into view: once it's drawn and nothing above it is still
  // coming (the stats would push it down again).
  $effect(() => {
    if (focus !== 'ai-usage' || !summary || !ready || !cardEl) return;
    untrack(async () => {
      await tick();
      cardEl?.scrollIntoView({ block: 'start', behavior: 'auto' });
      cardEl?.focus({ preventScroll: true });
      onfocused?.();
    });
  });

  function dayLabel(d) {
    return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }

  onMount(() => {
    load();
    // ai_usage.json written (a few seconds after each AI call).
    const un = listen('yap-ai-usage', load);
    return () => un.then((f) => f());
  });
</script>

{#if summary && (summary.month.calls > 0 || focus === 'ai-usage')}
  <section class="aiusage" id="ai-usage" bind:this={cardEl} tabindex="-1" aria-labelledby="ai-usage-title" data-testid="ai-usage-card">
    <h2 id="ai-usage-title">AI usage</h2>
    <p class="lead">Counted on this PC from each AI answer’s own token count. Nothing leaves this PC.</p>

    <div class="totals">
      {#each PERIODS as p (p.id)}
        {@const t = summary[p.id]}
        <div class="tcard" data-testid={`ai-usage-total-${p.id}`}>
          <div class="num">{fmtCount(t.tokens)}</div>
          <div class="lbl">tokens · {p.label.toLowerCase()}</div>
          <div class="sub">{splitLine(t)}</div>
        </div>
      {/each}
    </div>

    <div class="seg" role="group" aria-label="Show">
      {#each PERIODS as p (p.id)}
        <button type="button" aria-pressed={period === p.id} onclick={() => (period = p.id)}>{p.label}</button>
      {/each}
    </div>

    {#if !shown?.calls}
      <p class="note">No AI use {period === 'today' ? 'today' : `in the ${PERIODS.find((p) => p.id === period).label.toLowerCase()}`}.</p>
    {:else}
      <div class="cols">
        <div class="col">
          <h3>By model</h3>
          <div class="rows" data-testid="ai-usage-models">
            {#each shown.models as m (`${m.provider}:${m.model}:${m.here}`)}
              {@const row = modelRow(m)}
              <div class="urow">
                <span class="uname" title={`${row.name} · ${row.where}`}>{row.name}<span class="uwhere">{` · ${row.where}`}</span></span>
                <span class="ubar"><span style="width:{Math.max(3, Math.round((m.tokens / maxModel) * 100))}%"></span></span>
                <span class="uval">{fmtCount(m.tokens)}</span>
                <span class="ucost" class:free={m.here}>{costLine(m)}{m.perSecond ? ` · ${speedLine(m.perSecond)}` : ''}</span>
              </div>
            {/each}
          </div>
        </div>
        <div class="col">
          <h3>By feature</h3>
          <div class="rows" data-testid="ai-usage-features">
            {#each shown.features as f (f.feature)}
              <div class="urow">
                <span class="uname">{featureLabel(f.feature)}</span>
                <span class="ubar"><span style="width:{Math.max(3, Math.round((f.tokens / maxFeature) * 100))}%"></span></span>
                <span class="uval">{fmtCount(f.tokens)}</span>
                <span class="ucost">{f.calls} {f.calls === 1 ? 'call' : 'calls'}</span>
              </div>
            {/each}
          </div>
        </div>
      </div>
      {#if shown.unreported > 0}
        <p class="note">{shown.unreported} {shown.unreported === 1 ? 'call' : 'calls'} didn’t say {shown.unreported === 1 ? 'its' : 'their'} tokens: counted as calls only.</p>
      {/if}
    {/if}

    {#if summary.speed.length}
      <h3>On-device speed</h3>
      <div class="speed" role="img" aria-label={`On-device model speed by day, last 30 days${latestSpeed ? `: latest ${speedLine(latestSpeed.perSecond)}` : ''}`}>
        {#each speedDays as d (d.key)}
          <span
            class="sbar"
            class:none={!d.speed}
            style={d.speed ? `height:${Math.max(6, Math.round((d.speed.perSecond / maxSpeed) * 100))}%` : ''}
            title={d.speed ? `${dayLabel(d.date)} · ${speedLine(d.speed.perSecond)} · ${tokensWord(d.speed.tokens)} written` : dayLabel(d.date)}
          ></span>
        {/each}
      </div>
      <p class="note">
        {#if latestSpeed}Latest: {speedLine(latestSpeed.perSecond)} written{latestSpeed.readPerSecond ? `, ${speedLine(latestSpeed.readPerSecond)} read` : ''}. {/if}One bar a day, the last 30 days.
      </p>
    {/if}

    <p class="fine">
      Cloud costs are estimates at the providers’ list prices (checked {fmtDay(summary.pricesAsOf)}): free plans cost nothing, and cached prompts cost less.
      {#if shown?.unpriced > 0}{fmtCount(shown.unpriced)} tokens from models without a known price aren’t in it.{/if}
      Kept {summary.daysKept} days.
    </p>
  </section>
{/if}

<style>
  .aiusage {
    margin-bottom: 26px;
    scroll-margin-top: 20px;
  }
  .aiusage:focus {
    outline: none;
  }
  h2 {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  h3 {
    margin: 16px 0 8px;
    font-size: 12px;
    font-weight: 650;
    color: var(--yap-fg-80);
  }
  .lead {
    margin: 0 0 12px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .totals {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    margin-bottom: 14px;
  }
  .tcard {
    padding: 14px 16px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .num {
    font-family: var(--yap-font-display);
    font-size: 25px;
    font-weight: 550;
    font-variant-numeric: oldstyle-nums;
  }
  .lbl {
    margin-top: 2px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .sub {
    margin-top: 6px;
    font-size: 11.5px;
    color: var(--yap-fg-80);
    font-variant-numeric: tabular-nums;
  }
  .seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--yap-r);
    background: var(--yap-raised);
  }
  .seg button {
    padding: 4px 10px;
    border: none;
    border-radius: calc(var(--yap-r) - 2px);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .seg button[aria-pressed='true'] {
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-weight: 600;
    box-shadow: var(--yap-shadow-sm);
  }
  .seg button:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 22px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 9px;
  }
  .urow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 54px;
    grid-template-areas:
      'name val'
      'bar bar'
      'cost cost';
    row-gap: 3px;
    column-gap: 8px;
    align-items: baseline;
  }
  .uname {
    grid-area: name;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 500;
  }
  .uwhere {
    font-weight: 400;
    color: var(--yap-muted);
  }
  .ubar {
    grid-area: bar;
    height: 6px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .ubar span {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--yap-primary);
  }
  .uval {
    grid-area: val;
    text-align: right;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--yap-fg-80);
  }
  .ucost {
    grid-area: cost;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .ucost.free {
    color: var(--yap-success);
  }
  .speed {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 56px;
    padding: 4px 0;
  }
  .sbar {
    flex: 1 1 0;
    max-width: 16px;
    border-radius: 3px 3px 0 0;
    background: var(--yap-primary);
  }
  .sbar.none {
    height: 3px;
    background: var(--yap-raised);
  }
  .note {
    margin: 8px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .fine {
    margin: 14px 0 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-muted-55);
  }
  @media (max-width: 640px) {
    .totals,
    .cols {
      grid-template-columns: 1fr;
    }
  }
</style>
