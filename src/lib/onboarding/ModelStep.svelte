<script>
  // Setup's first step: the speech model (two shown — Parakeet V3,
  // recommended, and the best other pick for the PC's language — the rest
  // behind "More models"), where it runs on this PC, and the one-click
  // download in the footer.
  import { orderedModels, recommended } from './setupState.svelte.js';
  import StepHeader from './StepHeader.svelte';
  import ModelCard from './ModelCard.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();
</script>

<StepHeader title="Welcome to Yap" logo>
  Pick the speech model that turns your voice into text. It runs
  <strong>on this PC</strong>
  {#if setup.advice?.speechDevice === 'cpu'}(on its processor: no graphics card needed){:else}(on its graphics card){/if}
  — your voice never leaves this machine.
  <strong>{recommended.name}</strong> is the fast, accurate default.
</StepHeader>

<div class="cards">
  {#each setup.shownModels as m (m.id)}
    <ModelCard model={m} status={setup.statusOf(m.id)} percent={setup.percent} onclick={setup.choose} />
  {/each}
  {#if setup.shownModels.length < orderedModels.length}
    <button class="more-models" onclick={() => (setup.allModels = true)}>
      More models ({orderedModels.length - setup.shownModels.length})
    </button>
  {/if}
</div>

<style>
  .cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
    padding-right: 6px; /* breathing room so the scrollbar doesn't hug the cards */
  }
  /* Visible (but subtle) scrollbar so it's obvious the list scrolls. */
  .cards::-webkit-scrollbar {
    width: 8px;
  }
  .cards::-webkit-scrollbar-track {
    background: transparent;
  }
  .cards::-webkit-scrollbar-thumb {
    background: var(--yap-border);
    border-radius: 4px;
  }
  .cards::-webkit-scrollbar-thumb:hover {
    background: var(--yap-border-hover);
  }
  .more-models {
    align-self: flex-start;
    margin: 2px 0 0 4px;
    padding: 4px 6px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .more-models:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .more-models:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
</style>
