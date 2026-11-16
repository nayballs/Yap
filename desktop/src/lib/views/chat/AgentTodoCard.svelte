<script>
  // **A turn's to-do card** in a Code chat's thread (after the Claude Code
  // app's own plan card): under the agent's pill for that turn, Claude
  // Code's to-do list as its latest `TodoWrite` left it — a ✓ in a filled
  // circle for done (dimmed), a turning circle for the one in progress
  // (bright, with the line the agent is on under it), an empty dashed circle
  // for the rest. Live while the turn works; once it ends it stays as its
  // record, folded to "7 to-dos · all done" with ▸ to open, one still in
  // progress then stopped (16 Nov 2026: a pause mark, never turning once
  // nothing works on it; agents/steps.rs `TODO_STOPPED`). No list, no
  // card. ChatThread loads it (its own small chunk) only in Code chats.
  //
  // **The plan from before a message** (15 Nov 2026, as Claude Code's app
  // has it): when the person writes while the chat's Claude works, the list
  // as it stood then stays in the thread just above their message —
  // "Plan from before your last message" (`before`), never ticked on after
  // (`list`: those to-dos, as they were) — and the live list goes on under
  // their message once Claude writes it again (AgentWork places both).
  //
  // **At the thread's foot while it works** (16 Nov 2026, as the Claude app
  // keeps its plan; `foot`): a direct turn's live list is the thread's last
  // thing, the column's whole width, just above the message box (ChatThread);
  // once the turn ends, its record at the end of the turn's work (AgentWork).
  // Its sizes the app's proportions to the reading text: rows about 1.85× the
  // text apart, circles about the text's size, padding about its size.
  import { isLive } from '../../core/agentNow.js';
  import { todoCounts, todoSummary } from '../../core/agentPanel.js';

  /**
   * `agent`: the agent as stores/agents.svelte.js has it; `turn`: its turn's `n`; `line`: the line the agent is on now;
   * `list`: a plan as it stood (not the turn's live one); `before`: its label ("Plan from before your last message");
   * `foot`: the live list at the thread's foot (the column's width).
   */
  let { agent, turn = 1, line = '', list = null, before = '', foot = false } = $props();

  const note = $derived((agent?.turnList || []).find((t) => t.n === turn) ?? (agent?.turnList?.length ? null : agent));
  const todos = $derived((list ?? note?.todos ?? []).filter((t) => t && typeof t.content === 'string'));
  /** Its turn is still working: open, as the live list is. */
  const turnLive = $derived(isLive(note?.status ?? agent?.status));
  /** The live list (a plan as it stood never ticks on). */
  const live = $derived(turnLive && list == null);
  const counts = $derived(todoCounts(todos, live));
  /** The person's own fold (null: open while it works, folded once it ends). */
  let mine = $state(null);
  const open = $derived(mine ?? turnLive);
  /**
   * **Stopped** (16 Nov 2026): Rust keeps one still in progress when its turn ended as "stopped"
   * (agents/steps.rs `TODO_STOPPED`); one in progress on the turn's own list once the turn is over (one saved before)
   * reads the same. (A plan as it stood before a message keeps its own: in progress then.)
   */
  const stoppedNow = (t) => t.status === 'stopped' || (list == null && t.status === 'in_progress' && !turnLive);
  /** Under the one in progress: the line the agent is on (never the item's own words again). */
  const under = (t) => {
    if (!live) return '';
    const words = (line || t.activeForm || '').trim();
    return words && words !== t.content ? words : '';
  };
</script>

{#if todos.length}
  <section
    class="todocard"
    class:live
    class:before={!!before}
    class:foot
    data-testid={before ? 'chat-agent-todo-before' : 'chat-agent-todo-card'}
    data-agent={agent?.id}
    data-turn={turn}
    data-live={live}
    data-open={open}
    aria-label={before || 'To-dos'}
  >
    <button type="button" class="chead" aria-expanded={open} onclick={() => (mine = !open)} data-testid="chat-agent-todo-head">
      <svg class="chev" class:down={open} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      {#if open}
        <span class="ctitle">{before || 'To-dos'}</span>
        <span class="ccount">{counts.done} of {counts.total} done</span>
      {:else}
        <span class="ctitle" data-testid="chat-agent-todo-summary">{before ? `${before} · ` : ''}{todoSummary(todos)}</span>
      {/if}
    </button>
    {#if open}
      <ul class="items" data-testid="chat-agent-todo-items">
        {#each todos as t, i (i)}
          {@const sub = t.status === 'in_progress' ? under(t) : ''}
          <!-- (One in progress once its turn is over, or Rust's "stopped", is stopped: never turning.) -->
          {@const status = stoppedNow(t) ? 'stopped' : t.status}
          <li class="item st-{status}" data-status={status}>
            <span class="circle" aria-hidden="true">
              {#if status === 'completed'}
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m4.5 8.3 2.3 2.3 4.7-5" /></svg>
              {:else if status === 'in_progress' && live}
                <span class="spin"></span>
              {:else if status === 'in_progress'}
                <span class="dots">⋯</span>
              {:else if status === 'stopped'}
                <svg class="paused" viewBox="0 0 16 16" fill="currentColor"><rect x="5" y="4.5" width="2" height="7" rx="0.6" /><rect x="9" y="4.5" width="2" height="7" rx="0.6" /></svg>
              {/if}
            </span>
            <span class="words">
              <span class="text">{t.content}</span>
              {#if sub}<span class="sub" data-testid="chat-agent-todo-now">{sub}</span>{/if}
              <span class="sr-only">{status === 'completed' ? ', done' : status === 'in_progress' ? ', in progress' : status === 'stopped' ? ', stopped' : ', to do'}</span>
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  /* (Its sizes in em of the reading text, the thread's, in the Claude app's proportions.) */
  .todocard {
    align-self: flex-start;
    box-sizing: border-box;
    width: min(420px, 100%);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: calc(12.5px * var(--yap-reading-scale, 1));
  }
  /* At the thread's foot: the column's whole width, as the app's plan card. */
  .todocard.foot {
    width: 100%;
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 0.55em;
    width: 100%;
    box-sizing: border-box;
    padding: 0.72em 1.05em;
    border: none;
    border-radius: var(--yap-r-lg);
    background: none;
    color: inherit;
    font: inherit;
    font-size: 1em;
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
    width: 0.95em;
    height: 0.95em;
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
  /* A plan as it stood before a message: its label quieter, as the Claude app's. */
  .before .ctitle {
    color: var(--yap-muted);
    font-size: 0.92em;
    font-weight: 600;
  }
  .ccount {
    margin-left: auto;
    color: var(--yap-muted);
    font-size: 0.92em;
    font-variant-numeric: tabular-nums;
  }
  /* (The app's rows: about 1.85× the text apart, a circle about the text's size, the words just after it.) */
  .items {
    margin: 0;
    padding: 0 1.05em 0.62em;
    list-style: none;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 0.6em;
    padding: 0.2em 0;
    line-height: 1.45;
  }
  .circle {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 1.04em;
    height: 1.04em;
    margin-top: 0.2em;
    border-radius: 50%;
  }
  .st-completed .circle {
    background: var(--yap-success);
    color: var(--yap-s1);
  }
  .st-completed .circle svg {
    width: 0.8em;
    height: 0.8em;
  }
  .st-in_progress .circle {
    border: 1.5px solid var(--yap-code);
    color: var(--yap-code);
  }
  .st-pending .circle {
    border: 1.5px dashed var(--yap-muted);
  }
  .spin {
    width: 0.56em;
    height: 0.56em;
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
    font-size: 0.88em;
    line-height: 1;
  }
  /* Stopped (its turn ended with it in progress): still, muted, never turning. */
  .st-stopped .circle {
    border: 1.5px solid var(--yap-muted);
    color: var(--yap-muted);
  }
  .paused {
    width: 10px;
    height: 10px;
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
  .st-pending .text,
  .st-stopped .text {
    color: var(--yap-fg-80);
  }
  .sub {
    color: var(--yap-muted);
    font-size: 0.92em;
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
