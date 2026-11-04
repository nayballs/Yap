<script>
  // What an answer's tools did (chat/activity.rs), folded into one line
  // under it as the Claude Code app folds its steps: "Searched the web 3
  // times, read 2 pages, wrote 4 sections ▸". It opens to the steps
  // themselves, the chips ("Searched the web: "…"", "Read …", "Wrote 3
  // sections in …"; a globe for the web, a shield for an action refused
  // after reading it). While the answer is written it grows step by step,
  // the line under it saying what's under way (ActivityLine.svelte).
  import { chipKind, stepsSummary } from './chatSteps.js';

  /** `tools`: the answer's chips. */
  let { tools = [] } = $props();

  const summary = $derived(stepsSummary(tools));
</script>

{#if tools.length}
  <details class="steps" data-testid="chat-steps">
    <summary>
      <span class="sumtext">{summary}</span>
      <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
    </summary>
    <div class="toolchips" role="list" aria-label="Steps">
      {#each tools as t, j (j)}
        {@const kind = chipKind(t)}
        <span class="toolchip {kind}" role="listitem">
          {#if kind === 'web'}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z" /></svg>
          {:else if kind === 'calendar'}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="5" width="17" height="15" rx="2.5" /><path d="M3.5 10h17" /><path d="M8 3v4M16 3v4" /></svg>
          {:else if kind === 'doc'}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
          {:else if kind === 'guard'}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 4.5 6v5.5c0 4.6 3.1 8.2 7.5 9.5 4.4-1.3 7.5-4.9 7.5-9.5V6z" /><path d="m9 12 2 2 4-4" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4.5 4.5 0 0 0 5.8 5.8L17 15.6a4.5 4.5 0 0 1-5.8-5.8z" transform="rotate(90 12 12)" /><path d="m3 21 6.5-6.5" /><path d="M14.7 6.3 21 3l-3.3 6.3" /></svg>
          {/if}
          {t}
        </span>
      {/each}
    </div>
  </details>
{/if}

<style>
  .steps {
    align-self: flex-start;
    max-width: 78%;
    min-width: 0;
    margin: -2px 0 0 2px;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .steps summary {
    display: flex;
    align-items: center;
    gap: 4px;
    width: fit-content;
    max-width: 100%;
    list-style: none;
    cursor: pointer;
    border-radius: var(--yap-r-sm, 6px);
  }
  .steps summary::-webkit-details-marker {
    display: none;
  }
  .steps summary:hover {
    color: var(--yap-fg);
  }
  .steps summary:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .sumtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
    transition: transform 0.15s ease-out;
  }
  .steps[open] .chev {
    transform: rotate(90deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
  /* The steps, one a line, as they came. */
  .toolchips {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin: 6px 0 2px;
    padding-left: 9px;
    border-left: 2px solid var(--yap-border-subtle);
  }
  .toolchip {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    font-size: 11px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .toolchip svg {
    width: 11px;
    height: 11px;
    flex: 0 0 auto;
    margin-top: 2px;
    color: var(--yap-primary);
  }
  .toolchip.guard svg {
    color: var(--yap-success);
  }
</style>
