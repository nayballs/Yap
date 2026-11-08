<script>
  // **An agent's timeline** in its tab (AgentTab.svelte; core/agents.js
  // `timelineOf`): every step in plain words with its icon (reading,
  // searching, editing, running a command, the web, waiting for you), and
  // what Claude Code said between them, in order — in markdown, a long one
  // folded to its first lines ("Show more"). The box follows the live end
  // as a terminal does (ui/followEnd.js): pinned to the newest line while
  // the person is at the bottom, never moved while they've scrolled up to
  // read, "↓" to go back down.
  import { renderMarkdown } from '../../core/markdown.js';
  import { stepKind, longSaid } from '../../core/agents.js';
  import { followEnd, jumpToEnd } from '../../ui/followEnd.js';

  /** `items`: the timeline (`timelineOf`); `live`: the agent is working (its latest step marked). */
  let { items = [], live = false } = $props();

  let box = $state(null);
  let pinned = $state(true);
  // Narration opened past its fold, by its place.
  let unfolded = $state({});

  const lastStep = $derived(live ? items.findLastIndex((t) => t.kind === 'step') : -1);
</script>

<div class="tlwrap">
  <div class="tl" bind:this={box} use:followEnd={{ onchange: (p) => (pinned = p) }} data-testid="chat-agent-steps" data-pinned={pinned}>
    <ol class="steps">
      {#each items as t, i (i)}
        {#if t.kind === 'said'}
          {@const folds = longSaid(t.text)}
          <li class="said" class:folded={folds && !unfolded[i]} data-testid="chat-agent-said">
            <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
            <div class="md">{@html renderMarkdown(t.text)}</div>
            {#if folds}
              <button type="button" class="fold" aria-expanded={!!unfolded[i]} onclick={() => (unfolded = { ...unfolded, [i]: !unfolded[i] })}>
                {unfolded[i] ? 'Show less' : 'Show more'}
              </button>
            {/if}
          </li>
        {:else}
          {@const kind = stepKind(t)}
          <li class="step k-{kind}" class:latest={i === lastStep}>
            <span class="sicon" aria-hidden="true">
              {#if kind === 'edit'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></svg>
              {:else if kind === 'command'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m4 17 6-6-6-6" /><path d="M12 19h8" /></svg>
              {:else if kind === 'web'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
              {:else if kind === 'read' || kind === 'search'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
              {:else if kind === 'ask'}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z" /></svg>
              {:else}
                <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="2.4" /></svg>
              {/if}
            </span>
            <span class="stext" title={t.command || undefined}>{t.step}</span>
          </li>
        {/if}
      {/each}
    </ol>
  </div>
  {#if !pinned}
    <button type="button" class="tojump" onclick={() => jumpToEnd(box)} aria-label="Back to the newest step" title="Back to the newest step" data-testid="chat-agent-follow">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6" /></svg>
    </button>
  {/if}
</div>

<style>
  .tlwrap {
    position: relative;
  }
  .tl {
    max-height: min(50vh, 440px);
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .steps {
    margin: 0;
    padding: 0 2px 0 0;
    list-style: none;
  }
  .step {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 3px 0;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .step.latest {
    color: var(--yap-paper-ink);
    font-weight: 600;
  }
  .sicon {
    flex: 0 0 auto;
    display: inline-flex;
    width: 14px;
    height: 14px;
    margin-top: 2px;
    color: var(--yap-paper-muted);
  }
  .k-edit .sicon {
    color: var(--yap-primary);
  }
  .k-ask .sicon {
    color: var(--yap-warning);
  }
  .sicon svg {
    width: 14px;
    height: 14px;
  }
  .stext {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  /* What it said: its own words, set apart from the steps. */
  .said {
    margin: 6px 0 6px 22px;
    padding: 0 0 0 10px;
    border-left: 2px solid var(--yap-paper-rule);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .said .md {
    overflow-wrap: anywhere;
  }
  .said .md :global(p) {
    margin: 0 0 4px;
  }
  .said .md :global(p:last-child) {
    margin-bottom: 0;
  }
  .said .md :global(ul),
  .said .md :global(ol) {
    margin: 2px 0 4px;
    padding-left: 18px;
  }
  .said .md :global(code) {
    font-size: 11.5px;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--yap-paper-s1);
  }
  .said.folded .md {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    overflow: hidden;
  }
  .fold {
    margin: 2px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .fold:hover {
    text-decoration: underline;
  }
  .fold:focus-visible,
  .tojump:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tojump {
    position: absolute;
    right: 6px;
    bottom: 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: 0 2px 6px color-mix(in srgb, var(--yap-paper-ink) 14%, transparent);
    cursor: pointer;
  }
  .tojump:hover {
    background: var(--yap-paper-s2);
  }
  .tojump svg {
    width: 15px;
    height: 15px;
  }
</style>
