<script>
  // **An agent's work, in the thread** (11 Nov 2026, after Claude Code's own
  // transcript): under the answer that sent it, in a Code chat, what the agent
  // says as it goes — plain words, no bubble — and between them one quiet line
  // for each stretch of steps ("Ran 3 commands, edited convert.js +4 −1 ›"),
  // which opens into a row for each step, each opening to its diff or its
  // command and what it printed (StepRows). Before, the thread said "On it"
  // and then nothing until the report: all of this was only in the Agent tab.
  //
  // - Its log comes from stores/agentLogs.svelte.js (read once, then live);
  //   the turn's events are `turnsOf`'s, grouped by core/agents.js `workOf`.
  // - While it works, the last line is the step it's on, with a spinner.
  // - A step whose tool call came back with a picture (a picture file read,
  //   a screenshot a tool took: agents/pictures.rs) ends its line with it,
  //   small (StepPictures), as Claude Code's transcript does; opened, its
  //   row has it. A click opens it (`onview`).
  // - Its final words aren't here: its report tells them, right below.
  // - Nothing to show (no log yet, a turn that never ran): nothing drawn.
  //
  // Its own small chunk (ChatThread imports it lazily, as the to-do card).
  import { workOf, workSummary, turnsOf, groupPictures, shownStepPicture } from '../../core/agents.js';
  import { isLive } from '../../core/agentNow.js';
  import { renderMarkdown } from '../../core/markdown.js';
  import { colourCode } from '../../ui/colourCode.js';
  import { agentLogs, watchAgentLog } from '../../stores/agentLogs.svelte.js';
  import StepPictures from './StepPictures.svelte';
  import StepRows from './StepRows.svelte';

  /** `agent`: the pill's (its record, as the store has it); `turn`: which of its turns; `line`: the line it's on now; `onview(picture)`: open a step's picture (a `ShownPicture`). */
  let { agent, turn = 1, line = '', onview = null } = $props();
  const view = (p) => onview?.(shownStepPicture(p, agent?.folder));

  $effect(() => {
    if (agent?.id) watchAgentLog(agent.id);
  });
  const log = $derived(agentLogs.by[agent?.id] || []);
  const note = $derived(turnsOf(agent, log).find((t) => t.n === turn) ?? null);
  const live = $derived(isLive(note?.status));
  const items = $derived(note?.startedTs || live ? workOf(note?.events || [], live ? '' : note?.text, live) : []);
  /** The stretches the person opened (by where they start: a stretch keeps its place as more come). */
  let opened = $state({});
  const toggle = (key) => (opened = { ...opened, [key]: !opened[key] });
</script>

{#if items.length}
  <div class="agentwork" data-testid="chat-agent-work" data-agent={agent?.id} data-turn={turn} data-live={live ? 'true' : 'false'}>
    {#each items as it, i (i)}
      {#if it.kind === 'said'}
        <div class="wsaid" data-testid="chat-agent-work-said" use:colourCode>{@html renderMarkdown(it.text)}</div>
      {:else}
        {@const busy = live && i === items.length - 1}
        {@const parts = busy && line ? [{ text: line }] : workSummary(it, busy)}
        {@const open = !!opened[i]}
        <div class="wgroup" data-testid="chat-agent-work-steps" data-open={open ? 'true' : 'false'} data-busy={busy ? 'true' : 'false'}>
          <div class="wrow">
            <button type="button" class="wline" aria-expanded={open} onclick={() => toggle(i)} title={open ? 'Hide the steps' : 'Show the steps'}>
              {#if busy}<span class="spin" aria-hidden="true"></span>{/if}
              <span class="wtext">
                {#each parts as p, k (k)}{k ? ', ' : ''}{p.text}{#if p.added}<span class="plus">+{p.added}</span>{/if}{#if p.removed}<span class="minus">−{p.removed}</span>{/if}{/each}
              </span>
              <svg class="chev" class:down={open} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
            </button>
            <!-- The pictures its steps came back with, at the end of the line (opened: each by its step). -->
            {#if !open}<StepPictures pictures={groupPictures(it)} onview={view} />{/if}
          </div>
          {#if open}<StepRows steps={it.steps} {busy} onview={view} />{/if}
        </div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .agentwork {
    align-self: flex-start;
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    width: min(620px, 86%);
    /* (In line with a bubble's words: its border and padding.) */
    padding: 2px 2px 2px 13px;
    color: var(--yap-fg);
    font-size: calc(12.5px * var(--yap-reading-scale, 1));
    line-height: 1.5;
  }
  /* What it said: plain words, the thread's own text, no bubble. */
  .wsaid {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .wsaid :global(p) {
    margin: 0 0 6px;
  }
  .wsaid :global(p:last-child) {
    margin-bottom: 0;
  }
  .wsaid :global(ul),
  .wsaid :global(ol) {
    margin: 0 0 6px;
    padding-left: 20px;
  }
  .wsaid :global(code) {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--yap-s2);
    font-size: 0.92em;
  }
  .wsaid :global(pre) {
    margin: 0 0 6px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--yap-s2);
    overflow-x: auto;
  }
  /* A stretch of steps: one quiet line, its pictures at its end. */
  .wgroup {
    min-width: 0;
  }
  .wrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .wline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    margin: 0;
    padding: 1px 4px 1px 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .wline:hover {
    color: var(--yap-fg-80);
  }
  .wline:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .wtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .plus,
  .minus {
    margin-left: 5px;
  }
  .plus {
    color: var(--yap-success-text);
    font-variant-numeric: tabular-nums;
  }
  .minus {
    color: var(--yap-danger);
    font-variant-numeric: tabular-nums;
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    opacity: 0.8;
    transition: transform 120ms ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  .spin {
    flex: 0 0 auto;
    width: 10px;
    height: 10px;
    border: 1.5px solid color-mix(in srgb, var(--yap-code) 30%, transparent);
    border-top-color: var(--yap-code);
    border-radius: 50%;
    animation: work-spin 0.9s linear infinite;
  }
  @keyframes work-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
</style>
