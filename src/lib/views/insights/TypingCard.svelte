<script>
  // Insights' "Typing and speaking" (2026-10-29): the typing test's number
  // (shell/guide.rs: your typing speed, words a minute, as the first-run
  // setup or this card last took it) beside your speaking speed — over your
  // own dictations once Yap has timed a few (dictation/stats.rs `wpm`), else
  // the test's line read aloud — and the test itself here
  // (components/TypingTest.svelte), taken again or for the first time. Only
  // the numbers are kept.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import TypingTest from '../../components/TypingTest.svelte';

  let guide = $state(null);
  let cfg = $state(null);
  let measured = $state(null);
  let testing = $state(false);

  async function read() {
    try {
      guide = await invoke('guide_status');
    } catch {
      guide = null;
    }
    try {
      measured = (await invoke('get_stats'))?.wpm ?? null;
    } catch {
      measured = null;
    }
  }

  onMount(() => {
    read();
    invoke('get_config')
      .then((c) => (cfg = c))
      .catch(() => {});
  });

  const typing = $derived(guide?.typingWpm ?? null);
  const speaking = $derived(measured ?? guide?.speakingWpm ?? null);
  const times = $derived(typing && speaking ? speaking / typing : null);
</script>

<div class="section typing" data-testid="typing-card">
  <h2>Typing and speaking</h2>
  {#if testing}
    <div class="box">
      <TypingTest
        hotkey={cfg?.hotkey ?? ''}
        mode={cfg?.recordingMode ?? 'toggle'}
        spokenWpm={measured}
        ondone={() => read()}
      />
      <button type="button" class="link" onclick={() => (testing = false)}>Close the test</button>
    </div>
  {:else if typing}
    <div class="row">
      <div class="stat">
        <span class="num" data-testid="insights-typing-wpm">{typing}</span>
        <span class="lbl">words a minute typed</span>
      </div>
      {#if speaking}
        <div class="stat">
          <span class="num">{speaking}</span>
          <span class="lbl">{measured ? 'words a minute spoken, over your dictations' : 'words a minute spoken'}</span>
        </div>
      {/if}
      <div class="say">
        {#if times && times >= 1.1}
          <p>Speaking is <strong>{times.toFixed(1)}×</strong> faster than your typing.</p>
        {:else if !speaking}
          <p>Say the test's line too, to see how much faster speaking is.</p>
        {/if}
        <button type="button" class="ghost" onclick={() => (testing = true)}>Take the test again</button>
      </div>
    </div>
  {:else}
    <div class="row">
      <p class="lead">How fast do you type? Type a short passage, then say it, and see how much time dictation saves you. A minute at most.</p>
      <button type="button" class="go" onclick={() => (testing = true)}>Take the typing test</button>
    </div>
  {/if}
</div>

<style>
  .typing {
    margin-bottom: 26px;
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 640;
  }
  .row,
  .box {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 16px 18px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .box {
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .stat {
    display: flex;
    flex-direction: column;
  }
  .num {
    font-family: var(--yap-font-display);
    font-size: 30px;
    font-weight: 550;
    line-height: 1.1;
  }
  .lbl {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .say {
    margin-left: auto;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
    text-align: right;
  }
  .say p,
  .lead {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .lead {
    flex: 1;
  }
  .go {
    flex: 0 0 auto;
    padding: 7px 14px;
    border: none;
    border-radius: 8px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .ghost {
    padding: 5px 12px;
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .link {
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
