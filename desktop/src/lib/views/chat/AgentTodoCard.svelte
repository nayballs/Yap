<script>
  // **A turn's to-do card** in a Code chat's thread (after the Claude Code
  // app's own plan card): under the agent's pill for that turn, Claude
  // Code's to-do list as its latest `TodoWrite` left it — a ✓ in a filled
  // circle for done (dimmed), a turning circle for the one in progress
  // (bright, with the line the agent is on under it), an empty dashed circle
  // for the rest. Live while the turn works; once it ends it stays as its
  // record, folded to "7 to-dos · all done" with ▸ to open. No list, no
  // card. ChatThread loads it (its own small chunk) only in Code chats.
  import { isLive } from '../../core/agentNow.js';
  import { todoCounts, todoSummary } from '../../core/agentPanel.js';

  /** `agent`: the agent as stores/agents.svelte.js has it; `turn`: its turn's `n`; `line`: the line the agent is on now. */
  let { agent, turn = 1, line = '' } = $props();

  const note = $derived((agent?.turnList || []).find((t) => t.n === turn) ?? (agent?.turnList?.length ? null : agent));
  const todos = $derived((note?.todos || []).filter((t) => t && typeof t.content === 'string'));
  const live = $derived(isLive(note?.status ?? agent?.status));
  const counts = $derived(todoCounts(todos, live));
  /** The person's own fold (null: open while it works, folded once it ends). */
  let mine = $state(null);
  const open = $derived(mine ?? live);
  /** Under the one in progress: the line the agent is on (never the item's own words again). */
  const under = (t) => {
    if (!live) return '';
    const words = (line || t.activeForm || '').trim();
    return words && words !== t.content ? words : '';
  };
</script>

{#if todos.length}
  <section class="todocard" class:live data-testid="chat-agent-todo-card" data-agent={agent?.id} data-turn={turn} data-live={live} data-open={open} aria-label="To-dos">
    <button type="button" class="chead" aria-expanded={open} onclick={() => (mine = !open)} data-testid="chat-agent-todo-head">
      <svg class="chev" class:down={open} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      {#if open}
        <span class="ctitle">To-dos</span>
        <span class="ccount">{counts.done} of {counts.total} done</span>
      {:else}
        <span class="ctitle" data-testid="chat-agent-todo-summary">{todoSummary(todos)}</span>
      {/if}
    </button>
    {#if open}
      <ul class="items" data-testid="chat-agent-todo-items">
        {#each todos as t, i (i)}
          {@const sub = t.status === 'in_progress' ? under(t) : ''}
          <li class="item st-{t.status}" data-status={t.status}>
            <span class="circle" aria-hidden="true">
              {#if t.status === 'completed'}
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m4.5 8.3 2.3 2.3 4.7-5" /></svg>
              {:else if t.status === 'in_progress' && live}
                <span class="spin"></span>
              {:else if t.status === 'in_progress'}
                <span class="dots">⋯</span>
              {/if}
            </span>
            <span class="words">
              <span class="text">{t.content}</span>
              {#if sub}<span class="sub" data-testid="chat-agent-todo-now">{sub}</span>{/if}
              <span class="sr-only">{t.status === 'completed' ? ', done' : t.status === 'in_progress' ? ', in progress' : ', to do'}</span>
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .todocard {
    align-self: flex-start;
    box-sizing: border-box;
    width: min(420px, 100%);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    color: var(--yap-fg);
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    box-sizing: border-box;
    padding: 9px 12px;
    border: none;
    border-radius: var(--yap-r-lg);
    background: none;
    color: inherit;
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }
  .chead:hover {
    background: var(--yap-s2);
  }
  .chead:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    color: var(--yap-muted);
    transition: transform 0.15s ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
  .ctitle {
    font-weight: 650;
  }
  .ccount {
    margin-left: auto;
    color: var(--yap-muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .items {
    margin: 0;
    padding: 0 12px 10px;
    list-style: none;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    padding: 4px 0;
    font-size: 12.5px;
    line-height: 1.45;
  }
  .circle {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 16px;
    height: 16px;
    margin-top: 1px;
    border-radius: 50%;
  }
  .st-completed .circle {
    background: var(--yap-success);
    color: var(--yap-s1);
  }
  .st-completed .circle svg {
    width: 12px;
    height: 12px;
  }
  .st-in_progress .circle {
    border: 1.5px solid var(--yap-code);
    color: var(--yap-code);
  }
  .st-pending .circle {
    border: 1.5px dashed var(--yap-muted);
  }
  .spin {
    width: 8px;
    height: 8px;
    box-sizing: border-box;
    border: 1.5px solid color-mix(in srgb, var(--yap-code) 30%, transparent);
    border-top-color: var(--yap-code);
    border-radius: 50%;
  }
  @media (prefers-reduced-motion: no-preference) {
    .spin {
      animation: spin 0.9s linear infinite;
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .dots {
    font-size: 11px;
    line-height: 1;
  }
  .words {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .text {
    overflow-wrap: anywhere;
  }
  .st-completed .text {
    color: var(--yap-muted);
  }
  .st-in_progress .text {
    color: var(--yap-fg);
    font-weight: 600;
  }
  .st-pending .text {
    color: var(--yap-fg-80);
  }
  .sub {
    color: var(--yap-muted);
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
