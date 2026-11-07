<script>
  // What the way a scope uses is, said plainly (7 Nov 2026, the maintainer:
  // "We really need to make that distinction in the settings very clear"),
  // on the Claude and ChatGPT tabs of every Language Models scope
  // (ClaudePlanProvider.svelte, ChatGptProvider.svelte):
  //   - a line for the way in use ("Your Anthropic API key: quickest,
  //     billed per use."), or
  //   - `compare` (Dictation cleanup and Commands, where speed matters
  //     most): both ways side by side, the one in use marked — "Your plan:
  //     uses your plan's limits. Expect slightly slower responses…" against
  //     "API key: quickest. Billed per use…".
  // The words are providers.js's WAY_NOTES.
  //
  //   way:     the scope's provider ('claude-plan' | 'anthropic' | 'chatgpt' | 'openai').
  //   plan:    the tab's plan way, `key`: its key way (TAB_WAYS).
  //   compare: both ways, in two rows.
  import { WAY_NOTES } from '../core/providers.js';

  let { way, plan, key, compare = false } = $props();
</script>

{#if compare}
  <dl class="compare" data-testid="way-compare">
    {#each [{ id: plan, name: 'Your plan' }, { id: key, name: 'API key' }] as w (w.id)}
      <div class="way" class:on={way === w.id} aria-current={way === w.id ? 'true' : undefined}>
        <dt>{w.name}{#if way === w.id}<span class="inuse">In use</span>{/if}</dt>
        <dd>{WAY_NOTES[w.id].short}</dd>
      </div>
    {/each}
  </dl>
{:else if WAY_NOTES[way]}
  <p class="line" data-testid="way-note">{WAY_NOTES[way].line}</p>
{/if}

<style>
  .line {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .compare {
    display: grid;
    gap: 0;
    margin: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    overflow: hidden;
  }
  .way {
    display: grid;
    grid-template-columns: 118px 1fr;
    gap: 10px;
    align-items: baseline;
    padding: 8px 12px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .way + .way {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .way.on {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  dt {
    margin: 0;
    font-weight: 600;
    color: inherit;
  }
  .inuse {
    display: inline-block;
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-success) 16%, transparent);
    color: var(--yap-success);
    font-size: 10.5px;
    font-weight: 600;
    vertical-align: 1px;
  }
  dd {
    margin: 0;
  }
</style>
