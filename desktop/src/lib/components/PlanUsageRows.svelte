<script>
  // Your Claude plan's usage limits as rows (after the Claude desktop app's
  // "Plan usage limits"): each limit's label ("Weekly · all models",
  // "Weekly · Fable", "Current session"), when it resets in local time, its
  // percent and a thin bar coloured by its severity; then extra usage, when
  // it's on, as one line in the answer's currency. Chat's usage ring
  // (views/chat/PlanUsageRing.svelte) and Settings' plan card
  // (settings/ClaudePlanProvider.svelte, `small`). Words: core/planUsage.js.
  import { limitLabel, pct, resetsLabel, severityColor, extraLine } from '../core/planUsage.js';

  /** `usage`: claude_plan_usage's `usage` ({ limits, extra }). `small`: the card's size. */
  let { usage, small = false } = $props();

  const rows = $derived(Array.isArray(usage?.limits) ? usage.limits : []);
  const extra = $derived(extraLine(usage?.extra));
</script>

<div class="planrows" class:small data-testid="plan-usage-rows">
  {#each rows as l, i (`${l.kind}:${l.scope ?? ''}:${i}`)}
    {@const label = limitLabel(l)}
    {@const resets = resetsLabel(l.resetsAt)}
    {@const p = pct(l.percent)}
    <div class="lrow" data-testid="plan-usage-row" data-severity={l.severity}>
      <div class="ltop">
        <span class="lname">
          <span data-testid="plan-usage-label">{label}</span>{#if resets}<span class="lreset">{' · '}<span data-testid="plan-usage-reset">{resets}</span></span>{/if}
        </span>
        <span class="lpct" data-testid="plan-usage-percent">{p}%</span>
      </div>
      <div
        class="bar"
        role="meter"
        aria-label={label}
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={Math.min(100, p)}
        aria-valuetext={`${p}%`}
      >
        <span style="width:{Math.min(100, p)}%;background:{severityColor(l.severity)}"></span>
      </div>
    </div>
  {/each}
  {#if extra}
    <div class="extra" data-testid="plan-usage-extra">{extra}</div>
  {/if}
</div>

<style>
  .planrows {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 12px;
    color: var(--yap-fg);
  }
  .planrows.small {
    gap: 8px;
    font-size: 11.5px;
  }
  .lrow {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ltop {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .lname {
    min-width: 0;
    font-weight: 550;
    overflow-wrap: anywhere;
  }
  .lreset {
    font-weight: 400;
    color: var(--yap-muted);
  }
  .lpct {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
    color: var(--yap-fg-80);
  }
  .bar {
    width: 100%;
    height: 4px;
    border-radius: 999px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    min-width: 2px;
    border-radius: 999px;
    transition: width 0.4s ease;
  }
  .extra {
    padding-top: 2px;
    color: var(--yap-fg-80);
    font-variant-numeric: tabular-nums;
  }
  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
  :global([data-yap-motion='reduced']) .bar span {
    transition: none;
  }
</style>
