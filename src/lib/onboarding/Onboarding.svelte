<script>
  // First-run guided setup (superwhisper-style), five steps: the speech
  // model (ModelStep), the microphone (MicStep), AI cleanup (CleanupStep),
  // the tray (TrayStep) and "try it" (TryStep), with progress dots on top
  // and the footer — Back / I'll choose later, and each step's main button —
  // pinned at the bottom. Its state lives in setupState.svelte.js; this
  // window lives as long as Yap (hidden between showings), so it re-reads
  // the config and re-subscribes each time it's shown again.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { hotkeyMatchesKeydown, hotkeyMatchesKeyup } from '../core/hotkeys.js';
  import { createSetupState, flog, STEPS, recommended } from './setupState.svelte.js';
  import ModelStep from './ModelStep.svelte';
  import MicStep from './MicStep.svelte';
  import CleanupStep from './CleanupStep.svelte';
  import TrayStep from './TrayStep.svelte';
  import TryStep from './TryStep.svelte';

  const setup = createSetupState();

  // In-window hotkey fallback. When a WebView2 window of OUR OWN app has
  // focus, the global low-level keyboard hook never sees the hotkey (log-
  // proven 2026-07-05: focused presses leave zero hook events — WebView2
  // appears to front-run the hook chain). The key DOES reach this page as a
  // normal keydown though, so catch it here and drive the pipeline directly.
  function onFallbackKeyDown(e) {
    if (setup.recordingKey || e.repeat) return; // shortcut recorder open / key repeat
    if (!hotkeyMatchesKeydown(e, setup.cfg?.hotkey)) return;
    e.preventDefault();
    e.stopPropagation();
    flog('in-window hotkey fallback: keydown -> dictation_key');
    invoke('dictation_key', { pressed: true }).catch(() => {});
  }
  function onFallbackKeyUp(e) {
    if (setup.recordingKey) return;
    if (!hotkeyMatchesKeyup(e, setup.cfg?.hotkey)) return;
    // Push-to-talk: the release stops the recording, or leaves a double-tap
    // hands-free (toggle mode ignores it). The mode is Rust's to apply.
    e.preventDefault();
    flog('in-window hotkey fallback: keyup -> dictation_key');
    invoke('dictation_key', { pressed: false }).catch(() => {});
  }

  onMount(() => {
    flog('mounted; visibilityState=' + document.visibilityState);
    window.addEventListener('keydown', onFallbackKeyDown, true);
    window.addEventListener('keyup', onFallbackKeyUp, true);
    setup.refresh().then(setup.refreshLlm);
    invoke('hardware_advice')
      .then((a) => {
        setup.advice = a;
        // A small PC: the cloud is offered first (the private model adds
        // seconds to each dictation there).
        if (a?.cleanup === 'cloud' && !setup.cleanupEnabled) setup.cleanupMode = 'cloud';
      })
      .catch(() => {});
    invoke('list_audio_devices')
      .then((d) => (setup.devices = d || []))
      .catch(() => {});
    const onVis = () => {
      setup.pageVisible = !document.hidden;
      flog('visibilitychange: hidden=' + document.hidden);
      // Re-shown after a hide ("Show setup guide again"): re-sync from disk so
      // this long-lived window doesn't present (or later save) stale state —
      // and re-register the event subscriptions (see registerListeners).
      if (setup.pageVisible) {
        setup.registerListeners();
        invoke('get_config')
          .then((fresh) => {
            setup.adoptConfig(fresh);
            setup.refreshLlm();
            setup.refresh();
          })
          .catch(() => {});
      }
    };
    document.addEventListener('visibilitychange', onVis);
    setup.registerListeners();
    return () => {
      setup.unlistenAll();
      window.removeEventListener('keydown', onFallbackKeyDown, true);
      window.removeEventListener('keyup', onFallbackKeyUp, true);
      document.removeEventListener('visibilitychange', onVis);
      invoke('set_mic_test', { on: false }).catch(() => {});
    };
  });
</script>

<main>
  <!-- Progress dots -->
  <nav class="dots" aria-label="Setup progress">
    {#each STEPS as s, i}
      <button
        class="dot"
        class:done={i < setup.step}
        class:current={i === setup.step}
        title={s}
        onclick={() => (i < setup.step ? (setup.step = i) : null)}
        aria-label={`Step ${i + 1}: ${s}`}
      ></button>
    {/each}
  </nav>

  {#if setup.step === 0}
    <ModelStep {setup} />
  {:else if setup.step === 1}
    <MicStep {setup} />
  {:else if setup.step === 2}
    <CleanupStep {setup} />
  {:else if setup.step === 3}
    <TrayStep />
  {:else}
    <TryStep {setup} />
  {/if}

  {#if setup.error}
    <p class="error">{setup.error}</p>
  {/if}

  <footer>
    {#if setup.step > 0}
      <button class="skip" onclick={setup.back}>← Back</button>
    {:else}
      <button class="skip" onclick={setup.next} disabled={!setup.cfg}>I'll choose later</button>
    {/if}

    {#if setup.step === 0}
      <button class="start" onclick={setup.quickStart} disabled={!!setup.busyId || !setup.cfg}>
        {#if setup.busyId === recommended.id && setup.percent >= 100}
          <!-- Downloaded: checked, unpacked and loaded (~5–8 s), not "100%". -->
          Setting up {recommended.name}…
        {:else if setup.busyId === recommended.id}
          Downloading {recommended.name}… {setup.percent}%
        {:else if setup.busyId}
          Setting up… {setup.percent}%
        {:else if setup.active}
          Continue →
        {:else}
          Download {recommended.name} & continue
        {/if}
      </button>
    {:else if setup.step === 2 && !setup.cleanupEnabled}
      <button class="start ghost" onclick={setup.next} disabled={setup.llmInstalling}>
        {setup.llmInstalling ? 'Downloading…' : 'Maybe later'}
      </button>
    {:else if setup.step === STEPS.length - 1}
      <button class="start" onclick={setup.finish}>
        {setup.gotTranscript ? 'Finish 🎉' : 'Finish'}
      </button>
    {:else}
      <button class="start" onclick={setup.next} disabled={!setup.cfg}>Continue →</button>
    {/if}
  </footer>
</main>

<style>
  :global(body) {
    background: var(--yap-bg);
  }
  main {
    box-sizing: border-box;
    /* Hard viewport bound (NOT min-height): the page itself must never grow
       past the window, or the footer nav ends up below the fold and the model
       list silently stops being a scroll container. `.cards` scrolls instead. */
    height: 100vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--yap-bg);
    color: var(--yap-fg);
    padding: 20px 28px 22px;
  }

  /* Progress dots */
  .dots {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin-bottom: 18px;
  }
  .dot {
    width: 26px;
    height: 5px;
    border: none;
    border-radius: 3px;
    background: var(--yap-raised);
    padding: 0;
    cursor: default;
  }
  .dot.done {
    background: var(--yap-primary-tint);
    cursor: pointer;
  }
  .dot.current {
    background: var(--yap-primary);
  }

  .error {
    color: var(--yap-danger);
    font-size: 12px;
    text-align: center;
    margin: 12px 0 0;
  }

  /* Pinned to the bottom on every step, short ones too (the tray step's
     footer used to float up under its tips). */
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: auto;
    padding-top: 18px;
  }
  .skip {
    background: none;
    border: none;
    color: var(--yap-muted-70);
    font-size: 12.5px;
    cursor: pointer;
    padding: 8px 4px;
  }
  .skip:hover:not(:disabled) {
    color: var(--yap-fg);
  }
  /* Primary CTA = ink filled button, matching the main window. */
  .start {
    border: none;
    border-radius: 9px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font-size: 14px;
    font-weight: 600;
    padding: 11px 20px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .start:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .start:disabled {
    background: var(--yap-raised);
    color: var(--yap-muted);
    cursor: default;
  }
  .start.ghost {
    background: var(--yap-raised);
    color: var(--yap-fg-80);
  }
  .start.ghost:hover:not(:disabled) {
    background: var(--yap-raised-soft);
  }
</style>
