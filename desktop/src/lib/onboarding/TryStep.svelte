<script>
  // Setup's last step: try it here. The box fills from the dictation that
  // lands (the `yap-transcript` event, and history as a fallback), with a
  // flash; under it what Yap is doing, and Settings' own shortcut picker
  // (combos, chords, mouse buttons, the Copilot key), checked against the
  // other shortcuts.
  import { formatHotkeySpec as formatHotkey } from '../core/hotkeys.js';
  import { validateHotkey } from '../settings/hotkeys.svelte.js';
  import HotkeyInput from '../ui/HotkeyInput.svelte';
  import StepHeader from './StepHeader.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();
</script>

<StepHeader title="Try it">
  {#if setup.cfg?.recordingMode === 'pushToTalk'}
    Hold <strong class="key">{formatHotkey(setup.cfg?.hotkey)}</strong>, say something,
    then let go — your words appear below.
  {:else}
    Press <strong class="key">{formatHotkey(setup.cfg?.hotkey)}</strong>, say something,
    then press <strong class="key">{formatHotkey(setup.cfg?.hotkey)}</strong> again —
    your words appear below.
  {/if}
</StepHeader>

<div class="try-box">
  <textarea
    class="try-area"
    class:flash={setup.tryFlash}
    placeholder="Your words will appear here…"
    rows="5"
    readonly
    bind:value={setup.tryText}
  ></textarea>
  <p class="try-status">
    {#if setup.gotTranscript}
      ✓ It works! You can dictate into any app exactly like this.
    {:else if setup.tryState === 'recording'}
      ● Listening…
    {:else if setup.tryState === 'processing' || setup.tryState === 'processing-slow'}
      … Transcribing
    {:else if setup.tryState === 'needs-model'}
      ⚠ No model installed — go back to step 1.
    {:else}
      Waiting for {formatHotkey(setup.cfg?.hotkey)}…
    {/if}
  </p>
  {#if setup.cfg}
    <div class="shortcut" role="group" aria-label="Dictation shortcut">
      <span class="shortcut-label">Your shortcut</span>
      <HotkeyInput
        bind:value={setup.cfg.hotkey}
        validate={(s) => validateHotkey(setup.cfg, 'hotkey', s)}
        oncapturingchange={setup.onShortcutCapturing}
      />
    </div>
  {/if}
</div>

<style>
  /* Amber keycap chip (matches the Home greeting hotkey). */
  .key {
    background: var(--yap-primary-tint);
    border: 1px solid var(--yap-primary-line);
    border-radius: 5px;
    padding: 1px 7px;
    color: var(--yap-fg);
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  .shortcut {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 6px;
  }
  .shortcut-label {
    font-size: 12.5px;
    color: var(--yap-muted);
  }

  /* Try step */
  .try-box {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .try-area {
    width: 100%;
    max-width: 460px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: 10px;
    color: var(--yap-fg);
    font: inherit;
    font-size: 14px;
    line-height: 1.6;
    padding: 12px 14px;
    resize: none;
  }
  .try-area:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .try-area.flash {
    border-color: var(--yap-success);
    background: rgba(61, 187, 116, 0.1);
    transition: none;
  }
  .try-area {
    transition: border-color 0.9s ease, background 0.9s ease;
  }
  .try-status {
    color: var(--yap-muted);
    font-size: 13px;
    min-height: 18px;
    margin: 0;
  }
</style>
