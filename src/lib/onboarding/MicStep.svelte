<script>
  // Setup's microphone check: the device (switched live: picked, it goes
  // first in the microphones' order, dictation/mics.rs) and a scrolling level
  // meter (`set_mic_test`'s idle levels, `yap-amp`); with no microphone, it
  // says so.
  import StepHeader from './StepHeader.svelte';
  import { findMic } from '../core/mics.js';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();

  // The one in use now: the first in the order that's connected ('' =
  // Windows' default).
  const picked = $derived.by(() => {
    for (const c of setup.cfg?.micOrder || []) {
      const m = findMic(c, setup.devices);
      if (m) return m.id;
    }
    return '';
  });
</script>

<StepHeader title="Let's check your microphone">Say something — the bars should react. Silence? Pick a different device.</StepHeader>

<div class="mic-box">
  <select class="mic-pick" value={picked} onchange={(e) => setup.applyMic(e.currentTarget.value)} aria-label="Microphone">
    <option value="">System default</option>
    {#each setup.devices as d (d.id)}
      <option value={d.id}>{d.name}</option>
    {/each}
  </select>

  <div class="meter" class:live={setup.micHeard} aria-hidden="true">
    {#each setup.bars as b}
      <span style="height:{Math.max(6, b * 100)}%"></span>
    {/each}
  </div>
  <p class="mic-status">
    {#if setup.micHeard}✓ Hearing you loud and clear{:else}Waiting for sound…{/if}
  </p>
  {#if !setup.devices.length}
    <p class="mic-none" role="status">
      No microphone found. Plug one in or connect a headset, and it shows up here.
      Yap still transcribes audio files without one (Upload).
    </p>
  {/if}
</div>

<style>
  .mic-box {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
  }
  .mic-pick {
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    padding: 7px 10px;
    max-width: 320px;
  }
  .meter {
    display: flex;
    align-items: center;
    gap: 3px;
    height: 64px;
    width: 100%;
    max-width: 420px;
  }
  .meter span {
    flex: 1;
    background: var(--yap-raised);
    border-radius: 2px;
    transition: height 60ms linear;
  }
  .meter.live span {
    background: var(--yap-primary);
  }
  .mic-status {
    color: var(--yap-muted);
    font-size: 13px;
    margin: 0;
  }
  .meter.live + .mic-status {
    color: var(--yap-success);
  }
  .mic-none {
    max-width: 380px;
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    text-align: center;
    color: var(--yap-fg-80);
  }
</style>
