<script>
  // The mic quiz (dictation/mic_quiz.rs): which of your microphones hears you
  // best? Start, read the line aloud once, and Yap listens on every connected
  // microphone together, each one's level moving as you speak; then each
  // says how it heard you (clear, quiet, noisy, too loud, nothing) and the
  // best is marked. "Use this one" puts it first in the microphones' order
  // (`onuse(id)`: the caller's copy of the order — onboarding's, or
  // Settings'). A Bluetooth headset gets its honest note. The setup's mic
  // step and Settings → Dictation → Microphone (the Help menu's Microphone
  // help) both show it.
  //
  //   onuse(id, mic)  put that microphone first
  //   current         the id of the one in use now (else the first of
  //                   `order` that's connected, else Windows' default)
  //   order           the microphones' order (`micOrder`)
  //   listen          the event subscription to use (onboarding's raw bridge)
  import { invoke } from '@tauri-apps/api/core';
  import { listen as tauriListen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { micInUse, shortMicName } from '../core/mics.js';

  let { onuse, current = '', order = [], listen = tauriListen, compact = false } = $props();

  /** The line to read: a few seconds of ordinary speech. */
  const LINE = 'Yap, take a note: the design review moved to half past three on Thursday, in the small meeting room.';

  /** 'idle' | 'listening' | 'done' */
  let phase = $state('idle');
  let mics = $state([]);
  let levels = $state({});
  let progress = $state(0);
  let result = $state(null);
  let error = $state('');
  let used = $state('');

  async function loadMics() {
    try {
      mics = (await invoke('list_mics')) || [];
    } catch {
      mics = [];
    }
  }

  onMount(() => {
    loadMics();
    let un = null;
    let gone = false;
    Promise.resolve(
      listen('yap-mic-quiz', (e) => {
        const p = e.payload || {};
        if (phase !== 'listening') return;
        levels = p.levels || {};
        progress = p.totalMs ? Math.min(1, (p.elapsedMs || 0) / p.totalMs) : 0;
      })
    ).then((u) => {
      if (gone) u?.();
      else un = u;
    });
    return () => {
      gone = true;
      un?.();
      if (phase === 'listening') invoke('mic_quiz_cancel').catch(() => {});
    };
  });

  async function start() {
    error = '';
    used = '';
    result = null;
    levels = {};
    progress = 0;
    await loadMics();
    phase = 'listening';
    try {
      const q = await invoke('mic_quiz_run');
      if (q?.cancelled) {
        phase = 'idle';
        return;
      }
      result = q;
      phase = 'done';
    } catch (e) {
      error = String(e);
      phase = 'idle';
    }
  }

  function cancel() {
    invoke('mic_quiz_cancel').catch(() => {});
  }

  function use(id) {
    used = id;
    const m = (result?.mics ?? []).find((x) => x.id === id) ?? mics.find((x) => x.id === id);
    onuse?.(id, m ? { id: m.id, name: m.name } : { id, name: '' });
  }

  const QUALITY = { clear: 'Clear', quiet: 'Quiet', noisy: 'Noisy', loud: 'Too loud', silent: 'Nothing heard', failed: "Couldn't listen" };
  const rows = $derived(phase === 'done' && result ? result.mics : mics.filter((m) => !m.virtual));
  const best = $derived(result?.best ?? null);
  const inUse = $derived(used || current || micInUse(order, mics)?.id || '');
</script>

<div class="quiz" class:compact data-phase={phase} data-testid="mic-quiz">
  {#if phase === 'idle'}
    <div class="intro">
      <p class="lead">Which microphone hears you best? Press Start and read this line aloud: Yap listens on every microphone at once.</p>
      <blockquote>“{LINE}”</blockquote>
      <button type="button" class="go" onclick={start} disabled={!mics.length}>Start the mic quiz</button>
      {#if !mics.length}<p class="none">No microphone found. Plug one in or connect a headset.</p>{/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </div>
  {:else if phase === 'listening'}
    <div class="intro">
      <p class="lead" role="status">Listening… read the line aloud.</p>
      <blockquote>“{LINE}”</blockquote>
      <div class="progress" aria-hidden="true"><span style="width:{Math.round(progress * 100)}%"></span></div>
    </div>
  {:else}
    <p class="lead" role="status">
      {#if best}
        Your best microphone is <strong>{shortMicName(result.mics.find((m) => m.id === best)?.name)}</strong>.
      {:else}
        None of them heard you. Check your microphone isn't muted, then try again.
      {/if}
    </p>
  {/if}

  {#if phase !== 'idle'}
    <ul class="mics" aria-label="Your microphones">
      {#each rows as m (m.id)}
        {@const lvl = phase === 'done' ? (m.level ?? 0) : (levels[m.id] ?? 0)}
        <li class="mic" class:best={m.id === best} data-mic={m.id} data-quality={m.quality ?? ''}>
          <div class="top">
            <span class="name">{shortMicName(m.name)}</span>
            {#if m.id === best}<span class="badge best">Best</span>{/if}
            {#if m.bluetooth}<span class="badge">Bluetooth</span>{/if}
            {#if m.id === inUse}<span class="badge inuse">In use</span>{/if}
            {#if phase === 'done'}
              <span class="q">{QUALITY[m.quality] ?? ''}</span>
            {/if}
          </div>
          <div class="meter" aria-hidden="true"><span style="width:{Math.round(Math.min(1, lvl / 0.92) * 100)}%"></span></div>
          {#if phase === 'done'}
            <p class="note">{m.note}</p>
            {#if m.id !== inUse && !['silent', 'failed'].includes(m.quality)}
              <button type="button" class="use" class:primary={m.id === best} onclick={() => use(m.id)}>Use this one</button>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if phase === 'listening'}
    <button type="button" class="ghost" onclick={cancel}>Cancel</button>
  {:else if phase === 'done'}
    <div class="after">
      {#if used}<p class="ok" role="status">✓ {shortMicName(rows.find((m) => m.id === used)?.name)} is first in your microphones now.</p>{/if}
      <button type="button" class="ghost" onclick={start}>Try again</button>
    </div>
  {/if}
</div>

<style>
  .quiz {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
    max-width: 460px;
    padding: 14px 16px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s2);
    box-sizing: border-box;
  }
  .quiz.compact {
    max-width: none;
  }
  .intro {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .lead {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  blockquote {
    margin: 0;
    padding: 8px 12px;
    border-left: 3px solid var(--yap-primary);
    border-radius: 4px;
    background: var(--yap-s1);
    font-family: var(--yap-font-display);
    font-size: 15px;
    line-height: 1.45;
    color: var(--yap-fg);
  }
  .go,
  .use.primary {
    align-self: flex-start;
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
  .go:disabled {
    background: var(--yap-raised);
    color: var(--yap-muted);
    cursor: default;
  }
  .progress {
    height: 4px;
    border-radius: 2px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--yap-primary);
    transition: width 100ms linear;
  }
  .mics {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .mic {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 8px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 9px;
    background: var(--yap-s1);
  }
  .mic.best {
    border-color: var(--yap-primary);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    flex: 0 0 auto;
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    color: var(--yap-fg-80);
    font-size: 11px;
    font-weight: 600;
  }
  .badge.best {
    background: var(--yap-primary-tint);
    color: var(--yap-fg);
  }
  .q {
    margin-left: auto;
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .meter {
    height: 6px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .meter span {
    display: block;
    height: 100%;
    background: var(--yap-primary);
    transition: width 90ms linear;
  }
  .note {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .use {
    align-self: flex-start;
    padding: 4px 11px;
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .use.primary {
    padding: 4px 11px;
    font-size: 12.5px;
  }
  .ghost {
    align-self: flex-start;
    padding: 5px 12px;
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .after {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
  }
  .ok {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-success);
  }
  .none,
  .err {
    margin: 0;
    font-size: 12px;
  }
  .none {
    color: var(--yap-muted);
  }
  .err {
    color: var(--yap-danger);
  }
</style>
