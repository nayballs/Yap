<script>
  // Setup's last step, optional: how fast do you type? The typing test
  // (components/TypingTest.svelte): type a short passage, then say it with
  // your shortcut — or, once Yap has timed a few of your dictations
  // (dictation/stats.rs `wpm`), your speaking speed from them — and see the
  // two speeds side by side. Finish works whether it's taken or not; only
  // the numbers are kept (shell/guide.rs).
  import { invoke } from '@tauri-apps/api/core';
  import TypingTest from '../components/TypingTest.svelte';
  import StepHeader from './StepHeader.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();

  let spokenWpm = $state(null);
  invoke('get_stats')
    .then((s) => (spokenWpm = s?.wpm ?? null))
    .catch(() => {});
</script>

<StepHeader title="How fast do you type?">
  Optional, and a minute at most: type a short passage, then say it, and see
  how much time dictation saves you. Only the two speeds are kept.
</StepHeader>

<div class="typing">
  <TypingTest
    hotkey={setup.cfg?.hotkey ?? ''}
    mode={setup.cfg?.recordingMode ?? 'toggle'}
    listen={setup.listen}
    {spokenWpm}
    ondone={(r) => (setup.typingWpm = r.typingWpm)}
  />
</div>

<style>
  .typing {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    justify-content: center;
    padding: 2px 4px;
  }
</style>
