<script>
  // The tabs, 44 px with a hairline under the whole row: My thoughts,
  // Transcript (a live waveform on it while recording) and Summary (a +
  // until there's one).
  import Icon from './NotepadIcon.svelte';

  let { tab = $bindable(), recordingThis, hasSummary } = $props();
</script>

<div class="tabsrow">
  <div class="tablist" role="tablist" aria-label="Meeting notepad">
    <button role="tab" class="tab" aria-selected={tab === 'thoughts'} data-tip="Your own notes, as you go" onclick={() => (tab = 'thoughts')}>
      My thoughts
    </button>
    <button role="tab" class="tab" aria-selected={tab === 'transcript'} onclick={() => (tab = 'transcript')}>
      {#if recordingThis}
        <span class="wave" aria-hidden="true"><i></i><i></i><i></i></span>
      {/if}
      Transcript
    </button>
    <button role="tab" class="tab" aria-selected={tab === 'summary'} onclick={() => (tab = 'summary')}>
      {#if !hasSummary}<span class="plus" aria-hidden="true"><Icon name="plus" /></span>{/if}
      Summary
    </button>
  </div>
</div>

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  .tabsrow {
    flex: 0 0 44px;
    margin-top: 4px;
    padding: 0 20px;
    border-bottom: 1px solid var(--yap-paper-s2);
  }
  .tablist {
    display: flex;
    height: 100%;
  }
  .tab {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 12px;
    border: none;
    background: linear-gradient(var(--yap-paper-ink), var(--yap-paper-ink)) no-repeat 12px 100% / 0 2px;
    color: var(--yap-paper-text-2);
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    transition: color 0.15s var(--yap-ease-spring);
  }
  .tab:hover {
    color: var(--yap-paper-ink);
  }
  .tab[aria-selected='true'] {
    color: var(--yap-paper-ink);
    background-size: calc(100% - 24px) 2px;
  }
  .plus {
    display: inline-flex;
  }
  .plus :global(svg) {
    width: 11px;
    height: 11px;
    stroke-width: 3.2;
  }
  /* The live waveform on the Transcript tab while recording. */
  .wave {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    width: 12px;
    height: 16px;
  }
  .wave i {
    width: 2px;
    height: 11px;
    border-radius: 0.5px;
    background: var(--yap-live);
    animation: wavebar 0.9s ease-in-out infinite;
  }
  .wave i:nth-child(2) {
    animation-delay: 0.1s;
  }
  .wave i:nth-child(3) {
    animation-delay: -0.1s;
  }
  @keyframes wavebar {
    0%,
    100% {
      transform: scaleY(0.35);
    }
    50% {
      transform: scaleY(1);
    }
  }
</style>
