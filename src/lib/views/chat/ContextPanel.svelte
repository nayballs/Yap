<script>
  // The context ring's panel (ContextRing.svelte; Claude's own panel as the
  // model: rows with thin bars). This chat: the context it uses of the
  // model's, the last answer's tokens and, on a model whose server says,
  // how fast it wrote; past about 80 %, one quiet line with New chat. Then
  // today's and the last 7 days' AI use by model (ai/usage.rs
  // `ai_usage_summary`): on this PC "free", a cloud model with a known
  // price "≈ $0.02" (an estimate at list prices, usage/prices.rs). See all
  // → Insights' AI usage card. Counts only, kept on this PC.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import {
    fmtCount,
    fmtContext,
    splitLine,
    fmtDay,
    percentOf,
    ringColor,
    modelRow,
    costLine,
    speedLine,
    tokensWord,
    EMPTY_TOTALS,
  } from '../../core/aiUsage.js';
  import { providerName } from '../../core/aiModel.js';

  /** `use`: chat_context's answer. `onclose(focusRing)`, `onnewchat()`, `onseeall()`. */
  let { use, onclose, onnewchat, onseeall } = $props();

  let summary = $state(null);
  let panelEl = $state(null);

  const pct = $derived(percentOf(use.used, use.context));
  // "400 tokens · 120 tokens/s" (the speed when the server said).
  const lastLine = $derived(
    [tokensWord(use.lastAnswer), use.perSecond ? speedLine(use.perSecond) : ''].filter(Boolean).join(' · ')
  );
  const full = $derived(pct >= 80);
  const periods = $derived([
    { id: 'today', title: 'Today', t: summary?.today ?? EMPTY_TOTALS, empty: 'No AI use yet today.' },
    { id: 'week', title: 'Last 7 days', t: summary?.week ?? EMPTY_TOTALS, empty: 'No AI use in the last 7 days.' },
  ]);
  const anyCost = $derived(periods.some((p) => p.t.models.some((m) => !m.here && m.costMicros != null)));
  const fineLine = $derived(
    `${anyCost ? `Costs are estimates at list prices${summary?.pricesAsOf ? ` (checked ${fmtDay(summary.pricesAsOf)})` : ''}; free plans cost nothing. ` : ''}Counted on this PC: nothing leaves it.`
  );

  async function load() {
    try {
      summary = await invoke('ai_usage_summary');
    } catch {
      /* keep */
    }
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      e.preventDefault();
      onclose?.(true);
    }
  }

  onMount(() => {
    load();
    tick().then(() => panelEl?.focus());
    // A write of ai_usage.json (a few seconds after each answer).
    const un = listen('yap-ai-usage', load);
    return () => un.then((f) => f());
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="ctxpanel"
  role="dialog"
  aria-label="Context and AI usage"
  tabindex="-1"
  bind:this={panelEl}
  onkeydown={onKeydown}
  data-testid="context-panel"
>
  <section class="chat">
    <h3>This chat</h3>
    <div class="row">
      <span class="lbl">Context</span>
      <span class="val" data-testid="context-used">
        {use.estimate ? '≈ ' : ''}{fmtCount(use.used)} of {use.contextKnown ? '' : '≈ '}{fmtContext(use.context)} tokens
      </span>
    </div>
    <div
      class="bar"
      role="meter"
      aria-label="Context used"
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={Math.min(100, pct)}
      aria-valuetext={`${pct}%`}
    >
      <span style="width:{Math.min(100, pct)}%;background:{ringColor(pct)}"></span>
    </div>
    <div class="row sub">
      <span>{pct}% used</span>
      {#if use.model}<span>{providerName(use.model)}</span>{/if}
    </div>
    {#if use.lastAnswer != null}
      <div class="row">
        <span class="lbl">Last answer</span>
        <span class="val" data-testid="context-last-answer">{lastLine}</span>
      </div>
    {/if}
    {#if use.estimate}
      <p class="note">An estimate: {use.lastAnswer == null ? 'the model hasn’t said its tokens for this chat' : 'counted from the messages since the last answer'}.</p>
    {/if}
    {#if !use.contextKnown}
      <p class="note">
        Context length approximate: {use.model?.provider === 'local' || use.model?.provider === 'custom'
          ? 'the server didn’t say this model’s'
          : `${providerName(use.model) || 'the provider'} doesn’t list this model’s`}.
      </p>
    {/if}
    {#if full}
      <div class="long" role="status" data-testid="context-long">
        <span>
          {use.trimmed
            ? 'Long chat: only the latest messages go to the model. A new chat answers best.'
            : 'Nearly full: a long answer may be cut short. A new chat answers best.'}
        </span>
        <button type="button" class="newchat" onclick={() => onnewchat?.()}>New chat</button>
      </div>
    {/if}
  </section>

  {#each periods as p (p.id)}
    {@const maxTokens = Math.max(1, ...p.t.models.map((m) => m.tokens))}
    <section class="period" data-testid={`usage-${p.id}`}>
      <div class="phead">
        <h3>{p.title}</h3>
        <span class="ptotal">{fmtCount(p.t.tokens)} tokens</span>
      </div>
      {#if !summary}
        <p class="note">Counting…</p>
      {:else if !p.t.calls}
        <p class="note">{p.empty}</p>
      {:else}
        {#each p.t.models.slice(0, 3) as m (`${m.provider}:${m.model}:${m.here}`)}
          {@const row = modelRow(m)}
          <div class="mrow">
            <div class="mtop">
              <span class="mname" title={`${row.name} · ${row.where}`}>{row.name} <span class="mwhere">· {row.where}</span></span>
              <span class="mval">{fmtCount(m.tokens)}</span>
            </div>
            <div class="bar thin"><span style="width:{Math.max(2, Math.round((m.tokens / maxTokens) * 100))}%"></span></div>
            <div class="mcost" class:free={m.here}>{costLine(m)}</div>
          </div>
        {/each}
        {#if p.t.models.length > 3}
          <p class="note">and {p.t.models.length - 3} more</p>
        {/if}
        {#if p.t.models.length > 1}
          <div class="row sub split">{splitLine(p.t)}</div>
        {/if}
      {/if}
    </section>
  {/each}

  <footer>
    <p class="fine">{fineLine}</p>
    <button type="button" class="seeall" onclick={() => onseeall?.()}>See all</button>
  </footer>
</div>

<style>
  .ctxpanel {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    z-index: 22;
    width: 300px;
    max-height: min(560px, 70vh);
    overflow-y: auto;
    padding: 12px 14px 10px;
    border: 1px solid var(--yap-border);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 12px;
  }
  .ctxpanel:focus {
    outline: none;
  }
  .ctxpanel:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  section + section {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  h3 {
    margin: 0 0 2px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--yap-muted);
  }
  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .lbl {
    color: var(--yap-fg-80);
  }
  .val {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .row.sub {
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .bar {
    width: 100%;
    height: 6px;
    border-radius: 999px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar.thin {
    height: 4px;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--yap-primary);
    transition: width 0.4s ease;
  }
  .note {
    margin: 0;
    font-size: 11px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .long {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    padding: 7px 8px;
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    font-size: 11.5px;
    line-height: 1.4;
  }
  .newchat {
    flex: 0 0 auto;
    padding: 4px 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .newchat:hover {
    border-color: var(--yap-border-hover);
  }
  .phead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .ptotal {
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .mrow {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .mtop {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .mname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 550;
  }
  .mwhere {
    font-weight: 400;
    color: var(--yap-muted);
  }
  .mval {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
    color: var(--yap-fg-80);
  }
  .mcost {
    font-size: 11px;
    color: var(--yap-muted);
  }
  .mcost.free {
    color: var(--yap-success);
  }
  footer {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 10px;
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .fine {
    margin: 0;
    font-size: 10.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .seeall {
    flex: 0 0 auto;
    padding: 4px 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .seeall:hover {
    background: var(--yap-s3);
  }
  .seeall:focus-visible,
  .newchat:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>
