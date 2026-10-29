<script>
  // The typing test (Wispr Flow's typing vs speaking, by behaviour): type a
  // short passage, and see your typing speed; then say the same line with
  // your dictation shortcut, and see how much faster speaking is. Words a
  // minute: typed, the characters typed right ÷ 5 a minute from the first
  // key; spoken, the words Yap heard ÷ the recording's length. Only the two
  // numbers are kept (`guide_typing`, shell/guide.rs), nothing typed or said.
  // In the first-run setup (TypingStep) and on Insights.
  //
  //   hotkey, mode   the dictation shortcut and how it works
  //   listen         the event subscription (onboarding's raw bridge)
  //   spokenWpm      a speaking speed Yap already knows: the spoken part is
  //                  skipped
  //   ondone({ typingWpm, speakingWpm })
  import { invoke } from '@tauri-apps/api/core';
  import { listen as tauriListen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import { formatHotkeySpec } from '../core/hotkeys.js';

  let { hotkey = '', mode = 'toggle', listen = tauriListen, spokenWpm = null, ondone = null } = $props();

  const PASSAGE =
    "Thanks for sending the notes over. I've read them twice and they look good, so let's book the review for Thursday afternoon and invite the whole team.";
  const PASSAGE_WORDS = PASSAGE.split(/\s+/).length;

  /** 'ready' | 'typing' | 'speak' | 'done' */
  let phase = $state('ready');
  let typed = $state('');
  let startedAt = 0;
  let typingWpm = $state(null);
  let accuracy = $state(null);
  let speakingWpm = $state(null);
  let recState = $state('idle');
  let error = $state('');
  let box = $state(null);
  let spoken = $state(null);
  let saidBox = $state('');

  const key = $derived(formatHotkeySpec(hotkey) || 'your shortcut');
  const hold = $derived(mode === 'pushToTalk');

  /** Characters typed that match the passage, place by place. */
  function correctChars(text) {
    let n = 0;
    for (let i = 0; i < Math.min(text.length, PASSAGE.length); i++) if (text[i] === PASSAGE[i]) n++;
    return n;
  }

  function onInput() {
    if (phase === 'ready' && typed.length) {
      phase = 'typing';
      startedAt = performance.now();
    }
    if (phase === 'typing' && typed.length >= PASSAGE.length) finishTyping();
  }

  function onKeydown(e) {
    if (e.key === 'Enter' && phase === 'typing') {
      e.preventDefault();
      finishTyping();
    }
  }

  async function finishTyping() {
    if (phase !== 'typing') return;
    const minutes = (performance.now() - startedAt) / 60_000;
    const right = correctChars(typed);
    if (minutes <= 0 || right < 20) {
      error = 'Type a little more of the passage first.';
      return;
    }
    error = '';
    typingWpm = Math.max(1, Math.round(right / 5 / minutes));
    accuracy = Math.round((right / Math.max(typed.length, 1)) * 100);
    if (spokenWpm) {
      speakingWpm = Math.round(spokenWpm);
      await save();
      phase = 'done';
      ondone?.({ typingWpm, speakingWpm });
      return;
    }
    await save();
    phase = 'speak';
    await tick();
    spoken?.focus({ preventScroll: true });
  }

  async function save() {
    try {
      await invoke('guide_typing', { wpm: Math.min(typingWpm, 250), speaking: speakingWpm ? Math.min(speakingWpm, 400) : null });
    } catch (e) {
      error = String(e);
    }
  }

  // ---- the spoken line: timed from the dictation's own states ------------
  let recStart = 0;
  let recEnd = 0;
  let wait = null;

  function onState(s) {
    recState = s || 'idle';
    if (phase !== 'speak') return;
    if (s === 'recording') {
      recStart = performance.now();
      recEnd = 0;
    } else if (recStart && !recEnd) {
      recEnd = performance.now();
      // No words back soon (an empty or cancelled dictation): the line's own.
      clearTimeout(wait);
      wait = setTimeout(() => spokenDone(PASSAGE_WORDS), 6_000);
      if (s === 'idle' || s === 'cancelled' || s === 'error') {
        clearTimeout(wait);
        recStart = 0;
      }
    }
  }

  function onTranscript(text) {
    if (phase !== 'speak' || !recStart || !recEnd) return;
    const t = String(text || '').trim();
    if (!t) return;
    saidBox = t;
    spokenDone(t.split(/\s+/).length);
  }

  async function spokenDone(words) {
    clearTimeout(wait);
    if (phase !== 'speak' || !recStart || !recEnd) return;
    const minutes = (recEnd - recStart) / 60_000;
    recStart = 0;
    if (minutes <= 0.02) return;
    speakingWpm = Math.max(1, Math.round(words / minutes));
    await save();
    phase = 'done';
    ondone?.({ typingWpm, speakingWpm });
  }

  function skipSpeaking() {
    phase = 'done';
    ondone?.({ typingWpm, speakingWpm: null });
  }

  function again() {
    typed = '';
    saidBox = '';
    typingWpm = null;
    speakingWpm = null;
    accuracy = null;
    error = '';
    phase = 'ready';
    tick().then(() => box?.focus());
  }

  onMount(() => {
    const uns = [];
    let gone = false;
    const keep = (p) =>
      Promise.resolve(p).then((u) => {
        if (gone) u?.();
        else uns.push(u);
      });
    keep(listen('yap-state', (e) => onState(e.payload)));
    keep(listen('yap-transcript', (e) => onTranscript(e.payload)));
    return () => {
      gone = true;
      clearTimeout(wait);
      uns.forEach((u) => u?.());
    };
  });

  const times = $derived(typingWpm && speakingWpm ? speakingWpm / typingWpm : null);
</script>

<div class="tt" data-phase={phase} data-testid="typing-test">
  {#if phase === 'ready' || phase === 'typing'}
    <p class="lead">Type this as you normally would. The clock starts at your first key; press Enter when you're done.</p>
    <blockquote aria-label="The passage to type">
      {#each PASSAGE.split('') as ch, i}<span class:ok={i < typed.length && typed[i] === ch} class:bad={i < typed.length && typed[i] !== ch}>{ch}</span>{/each}
    </blockquote>
    <textarea
      bind:this={box}
      bind:value={typed}
      oninput={onInput}
      onkeydown={onKeydown}
      rows="3"
      aria-label="Type the passage here"
      placeholder="Start typing…"
      spellcheck="false"
      autocomplete="off"
    ></textarea>
    <div class="row">
      {#if phase === 'typing'}
        <button type="button" class="go" onclick={finishTyping}>Done</button>
      {/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </div>
  {:else if phase === 'speak'}
    <p class="result" role="status">You type at <strong data-testid="typing-wpm">{typingWpm}</strong> words a minute{#if accuracy != null && accuracy < 100}, {accuracy}% right{/if}.</p>
    <p class="lead">
      Now say the same line:
      {#if hold}hold <kbd>{key}</kbd>, read it aloud, then let go.{:else}press <kbd>{key}</kbd>, read it aloud, then press <kbd>{key}</kbd> again.{/if}
    </p>
    <blockquote>{PASSAGE}</blockquote>
    <textarea bind:this={spoken} value={saidBox} readonly rows="2" aria-label="What Yap heard" placeholder={recState === 'recording' ? 'Listening…' : 'Your words appear here…'}></textarea>
    <div class="row">
      <span class="state">{recState === 'recording' ? '● Listening…' : recState === 'processing' || recState === 'processing-slow' ? '… Transcribing' : ''}</span>
      <button type="button" class="ghost" onclick={skipSpeaking}>Skip this part</button>
    </div>
  {:else}
    <div class="done" role="status">
      <div class="stat">
        <span class="num" data-testid="typing-wpm">{typingWpm}</span>
        <span class="lbl">words a minute typed</span>
      </div>
      {#if speakingWpm}
        <div class="stat">
          <span class="num" data-testid="speaking-wpm">{speakingWpm}</span>
          <span class="lbl">words a minute spoken</span>
        </div>
      {/if}
    </div>
    {#if times && times >= 1.1}
      <p class="lead center">Speaking is <strong>{times.toFixed(1)}×</strong> faster than your typing.</p>
    {:else if times}
      <p class="lead center">You type nearly as fast as you speak: dictation still saves your hands.</p>
    {/if}
    <button type="button" class="ghost" onclick={again}>Take it again</button>
  {/if}
</div>

<style>
  .tt {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
    max-width: 480px;
  }
  .lead,
  .result {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .center {
    text-align: center;
  }
  blockquote {
    margin: 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
    font-size: 14px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  blockquote .ok {
    color: var(--yap-fg);
  }
  blockquote .bad {
    color: var(--yap-danger);
    text-decoration: underline;
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 9px 11px;
    border: 1px solid var(--yap-border);
    border-radius: 8px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 14px;
    line-height: 1.5;
    resize: none;
  }
  kbd {
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--yap-primary-tint);
    font: inherit;
    font-weight: 600;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 30px;
  }
  .state {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .go {
    padding: 6px 14px;
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
    align-self: center;
    padding: 5px 12px;
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .err {
    margin: 0;
    font-size: 12px;
    color: var(--yap-danger);
  }
  .done {
    display: flex;
    justify-content: center;
    gap: 36px;
    padding: 8px 0;
  }
  .stat {
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .num {
    font-family: var(--yap-font-display);
    font-size: 44px;
    font-weight: 600;
    line-height: 1.1;
  }
  .lbl {
    font-size: 12px;
    color: var(--yap-muted);
  }
</style>
