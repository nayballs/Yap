<script>
  // Chat's plan usage ring (after the Claude desktop app's, 2026-11-07): on
  // your Claude plan, beside the model chip where "Manage usage" was, a small
  // ring filled to the highest active limit's percent and coloured by its
  // severity (core/planUsage.js). Hover, or click (Enter): "Plan usage limits"
  // — a row per limit with when it resets and a thin bar
  // (components/PlanUsageRows.svelte), extra usage when it's on, and last
  // how full this chat is ("This chat · 16% of context": its last model
  // call's, chat/turn/plan.rs), with "See all →" to the plan's usage page.
  //
  // The numbers are Claude Code's own (`get_usage`, ai/claude_plan/usage.rs,
  // stores/claudePlanUsage.svelte.js): asked when this mounts and when the
  // window gets the focus back (at most once a minute), after each plan turn,
  // and moved live by a turn's rate-limit events. None to be had (an older
  // Claude Code, an API key's login, a shape that changed): no ring at all.
  import { onMount, tick } from 'svelte';
  import PlanUsageRows from '../../components/PlanUsageRows.svelte';
  import { planUsage, watchPlanUsage } from '../../stores/claudePlanUsage.svelte.js';
  import { ringOf, ringName, severityColor, contextLine } from '../../core/planUsage.js';
  import { openExternalLink } from '../../core/externalLinks.js';

  /** `conversationId`: the open conversation (its context line). `usageUrl`: See all. */
  let { conversationId = null, usageUrl = 'https://claude.ai/settings/usage' } = $props();

  const ring = $derived(ringOf(planUsage.usage));
  const ctx = $derived(conversationId == null ? '' : contextLine(planUsage.contexts?.[String(conversationId)]));
  // The ring: r 7 in an 18 px box, as Chat's context ring.
  const R = 7;
  const C = 2 * Math.PI * R;
  const dash = $derived(C * (1 - Math.min(100, Math.max(0, ring?.percent ?? 0)) / 100));

  let pinned = $state(false);
  let hover = $state(false);
  const open = $derived(pinned || hover);
  let wrapEl = $state(null);
  let ringEl = $state(null);
  let popEl = $state(null);
  let leaveTimer = 0;

  function enter() {
    clearTimeout(leaveTimer);
    hover = true;
  }
  // A moment's grace: the pointer crosses the gap into the popover.
  function leave() {
    clearTimeout(leaveTimer);
    leaveTimer = setTimeout(() => (hover = false), 180);
  }

  function toggle() {
    if (pinned) {
      close(false);
      return;
    }
    pinned = true;
    tick().then(() => popEl?.focus());
  }

  function close(focusRing) {
    pinned = false;
    hover = false;
    if (focusRing) tick().then(() => ringEl?.focus());
  }

  function onKeydown(e) {
    if (e.key === 'Escape' && open) {
      e.stopPropagation();
      e.preventDefault();
      close(true);
    }
  }

  function onWindowPointer(e) {
    if (pinned && wrapEl && !e.composedPath().includes(wrapEl)) close(false);
  }

  function seeAll() {
    close(false);
    openExternalLink(usageUrl);
  }

  onMount(() => {
    const unwatch = watchPlanUsage();
    return () => {
      unwatch();
      clearTimeout(leaveTimer);
    };
  });
</script>

<svelte:window onpointerdowncapture={onWindowPointer} />

{#if ring}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="planring"
    bind:this={wrapEl}
    data-testid="plan-usage-ring"
    data-percent={ring.percent}
    data-severity={ring.severity}
    onpointerenter={enter}
    onpointerleave={leave}
    onkeydown={onKeydown}
  >
    <button
      class="ring"
      bind:this={ringEl}
      aria-label={ringName(ring)}
      aria-haspopup="dialog"
      aria-expanded={open}
      onclick={toggle}
    >
      <svg viewBox="0 0 18 18" aria-hidden="true">
        <circle class="track" cx="9" cy="9" r={R} />
        <circle
          class="fill"
          cx="9"
          cy="9"
          r={R}
          style="stroke:{severityColor(ring.severity)};stroke-dasharray:{C};stroke-dashoffset:{dash}"
        />
      </svg>
    </button>
    {#if open}
      <div
        class="pop"
        role="dialog"
        aria-label="Plan usage limits"
        tabindex="-1"
        bind:this={popEl}
        data-testid="plan-usage-popover"
      >
        <h3>Plan usage limits</h3>
        <PlanUsageRows usage={planUsage.usage} />
        <footer>
          {#if ctx}
            <span class="ctx" data-testid="plan-usage-context">{ctx}</span>
          {:else}
            <span></span>
          {/if}
          <button type="button" class="seeall" onclick={seeAll}>See all →</button>
        </footer>
      </div>
    {/if}
  </div>
{/if}

<style>
  .planring {
    position: relative;
    display: flex;
    align-items: center;
    flex: 0 0 auto;
  }
  .ring {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-full);
    background: none;
    cursor: pointer;
  }
  .ring:hover,
  .ring[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .ring:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  svg {
    width: 16px;
    height: 16px;
    transform: rotate(-90deg);
  }
  circle {
    fill: none;
    stroke-width: 2.5;
  }
  .track {
    stroke: var(--yap-raised);
  }
  .fill {
    stroke-linecap: round;
    transition:
      stroke-dashoffset 0.4s ease,
      stroke 0.4s ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
  :global([data-yap-motion='reduced']) .fill {
    transition: none;
  }
  /* Above the ring, from its left edge (the chip's row is on the left). */
  .pop {
    position: absolute;
    left: -6px;
    bottom: calc(100% + 8px);
    z-index: 22;
    width: 290px;
    padding: 12px 14px 10px;
    border: 1px solid var(--yap-border);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 12px;
    text-align: left;
    white-space: normal;
  }
  /* The gap below it still counts as over it (the pointer crosses it). */
  .pop::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 100%;
    height: 10px;
  }
  .pop:focus {
    outline: none;
  }
  .pop:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  h3 {
    margin: 0 0 10px;
    font-size: 12.5px;
    font-weight: 650;
    color: var(--yap-fg);
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 12px;
    padding-top: 9px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .ctx {
    min-width: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .seeall {
    flex: 0 0 auto;
    padding: 4px 6px;
    margin-right: -6px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .seeall:hover {
    background: var(--yap-s3);
  }
  .seeall:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
</style>
