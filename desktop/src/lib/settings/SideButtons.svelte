<script>
  // "Set up your mouse's side buttons" (Settings → Shortcuts, and the tip
  // the first time Yap sees one pressed; dictation/side_buttons.rs): Dictate
  // on one, Send on the other, in one step (Wispr Flow's, by behaviour).
  //
  //   1. "Press the button you want for dictating" — Back unless another is
  //      pressed or picked; Skip leaves dictation as it is.
  //   2. "Press the one for Send" — the other one; Skip.
  //   3. What they'll do ("Back: hold to dictate, double-tap for hands-free
  //      · Forward: send"), push-to-talk or toggle, Swap, a button another
  //      shortcut has named (and moved only with its box ticked), and what
  //      the browser loses: Rust's plan (`side_buttons_preview`), saved by
  //      Rust in one step (`side_buttons_apply`, Undo on the toast).
  //
  // The buttons are heard through the input hook while steps 1 and 2 ask
  // (`side_buttons_listen`, renewed: `yap-side-button`), and by the page
  // itself when the hook doesn't hear them (a Yap window in front). The
  // in-page hotkey fallback stands down meanwhile (`hotkeyCapture`). A
  // dialog like Delete some of your data…: Esc, the scrim and Cancel close
  // it; Tab stays inside; the focus goes back where it was.
  import { onMount, tick, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { fillHotkey } from '../core/hotkeys.js';
  import { hotkeyCapture } from './hotkeys.svelte.js';
  import { BACK, FORWARD, applySideButtons, sideButtonName as name } from './sideButtons.svelte.js';
  import Segmented from '../ui/Segmented.svelte';

  // `current`: what they do now (side_buttons.rs `State`); `hold`: the
  // dictation's recording mode is push-to-talk. Both read once, as it opens.
  let { current = null, hold: holdNow = false, returnFocus = null, onclose } = $props();

  // What they're for now, else Back dictates and Forward sends.
  const start = untrack(() => {
    const d = current?.dictate ?? (current?.send === BACK ? FORWARD : BACK);
    return { dictate: d, send: current?.send ?? (d === FORWARD ? BACK : FORWARD), mode: holdNow ? 'hold' : 'toggle' };
  });

  const uid = Math.random().toString(36).slice(2, 8);
  let card = $state(null);
  let step = $state(1);
  let dictate = $state(start.dictate);
  let send = $state(start.send);
  let mode = $state(start.mode);
  let take = $state(false);
  let heard = $state(null); // the button heard on this step
  let warning = $state('');
  let plan = $state(null);
  let error = $state('');
  let busy = $state(false);
  let advance = null;

  const listening = $derived(step < 3);

  onMount(() => {
    tick().then(() => card?.querySelector('[data-primary]')?.focus({ preventScroll: true }));
    const un = listen('yap-side-button', (e) => onHeard(Number(e.payload?.button)));
    return () => {
      clearTimeout(advance);
      un.then((f) => f()).catch(() => {});
    };
  });

  // Listening while a step asks: the hook's (renewed, so a closed window
  // never leaves Back swallowed for long) and the page's own.
  $effect(() => {
    if (!listening) return;
    hotkeyCapture.on = true;
    const ask = () => invoke('side_buttons_listen', { on: true }).catch(() => {});
    ask();
    const renew = setInterval(ask, 20_000);
    const PAGE = { 1: 'middle', 3: BACK, 4: FORWARD }; // a page's mouse button → ours
    const onDown = (e) => {
      const b = PAGE[e.button];
      if (!b) return;
      e.preventDefault();
      e.stopPropagation();
      if (b === 'middle') warning = "That's the middle button. Press one of the two by your thumb.";
      else onHeard(b);
    };
    // A side button's release would go back in a webview.
    const onUp = (e) => {
      if (e.button === 3 || e.button === 4) {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('mouseup', onUp, true);
    window.addEventListener('auxclick', onUp, true);
    return () => {
      clearInterval(renew);
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('mouseup', onUp, true);
      window.removeEventListener('auxclick', onUp, true);
      invoke('side_buttons_listen', { on: false }).catch(() => {});
      hotkeyCapture.on = false;
    };
  });

  function onHeard(button) {
    if (button !== BACK && button !== FORWARD) return;
    if (step === 1) {
      dictate = button;
      if (send === button) send = button === BACK ? FORWARD : BACK;
    } else if (step === 2) {
      if (button === dictate) {
        warning = `${name(button)} dictates. Press the other one, or Skip.`;
        return;
      }
      send = button;
    } else {
      return;
    }
    warning = '';
    heard = button;
    // A beat to see which it heard, then on.
    clearTimeout(advance);
    const from = step;
    advance = setTimeout(() => step === from && next(), 650);
  }

  function go(to) {
    clearTimeout(advance);
    step = to;
    heard = null;
    warning = '';
    error = '';
    tick().then(() => card?.querySelector('[data-primary]')?.focus({ preventScroll: true }));
  }

  function next() {
    if (step === 1) {
      if (dictate !== null && send === dictate) send = dictate === BACK ? FORWARD : BACK;
      if (dictate === null && send === null) send = FORWARD;
      go(2);
    } else if (step === 2) go(3);
  }

  function skip() {
    if (step === 1) {
      dictate = null;
      if (send === null) send = FORWARD;
      go(2);
    } else if (step === 2) {
      send = null;
      go(3);
    }
  }

  function pick(button) {
    if (step === 1) {
      dictate = button;
      if (send === button) send = button === BACK ? FORWARD : BACK;
    } else if (step === 2 && button !== dictate) send = button;
    heard = null;
    warning = '';
  }

  function swap() {
    [dictate, send] = [send, dictate];
  }

  const choice = $derived({ dictate, send, hold: mode === 'hold', take });

  // The summary step: Rust's plan, again whenever the choice changes.
  $effect(() => {
    if (step !== 3) return;
    const c = choice;
    let stale = false;
    invoke('side_buttons_preview', { choice: c })
      .then((p) => {
        if (!stale) {
          plan = p;
          error = '';
        }
      })
      .catch((e) => {
        if (!stale) {
          plan = null;
          error = String(e);
        }
      });
    return () => (stale = true);
  });

  const asked = $derived((plan?.buttons ?? []).filter((b) => b.asked && (b.fate === 'taken' || b.fate === 'moved')));
  const takeLabel = $derived(
    asked.map((b) => `${b.name} to ${b.role === 'dictate' ? 'Dictate' : 'Send'}`).join(' and ')
  );
  const notes = $derived((plan?.buttons ?? []).filter((b) => b.note && !(b.fate === 'moved' && b.asked)));

  async function setUp() {
    if (busy || !plan?.changes) return;
    busy = true;
    try {
      await applySideButtons(choice);
    } catch (e) {
      error = String(e);
      return;
    } finally {
      busy = false;
    }
    close();
  }

  function close() {
    if (busy) return;
    onclose?.();
    tick().then(() => returnFocus?.isConnected && returnFocus.focus?.({ preventScroll: true }));
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const focusable = [...card.querySelectorAll('button:not(:disabled), input:not(:disabled)')].filter((el) => el.offsetParent !== null);
    if (!focusable.length) return;
    const head = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && (document.activeElement === head || document.activeElement === card)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      head.focus();
    }
  }

  const MODES = [
    { value: 'hold', label: 'Hold to talk' },
    { value: 'toggle', label: 'Press to start and stop' },
  ];
</script>

{#snippet buttons(chosen, other = null, otherLabel = '')}
  <div class="pair" role="group" aria-label="Side buttons">
    {#each [BACK, FORWARD] as b (b)}
      <button
        type="button"
        class="side"
        class:chosen={chosen === b}
        class:heard={heard === b}
        aria-pressed={chosen === b}
        disabled={other === b}
        onclick={() => pick(b)}
      >
        <span class="glyph" aria-hidden="true">{b === BACK ? '‹' : '›'}</span>
        <span class="bname">{name(b)}</span>
        <span class="bsub">
          {#if other === b}{otherLabel}{:else if heard === b}Heard ✓{:else}{b === BACK ? 'The one nearer your wrist' : 'The one nearer the front'}{/if}
        </span>
      </button>
    {/each}
  </div>
{/snippet}

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    bind:this={card}
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="sb-title-{uid}"
    tabindex="-1"
    onkeydown={onKeydown}
  >
    <div class="head">
      <p class="stepno">Step {step} of 3</p>
      <h2 id="sb-title-{uid}">Set up your mouse's side buttons</h2>
    </div>

    {#if step === 1}
      <div class="body" aria-live="polite">
        <h3>Press the button you want for dictating</h3>
        <p class="lead">Back, by your thumb, unless you press the other one. You can also pick it here.</p>
        {@render buttons(dictate)}
        {#if warning}<p class="warn" role="alert">{warning}</p>{/if}
      </div>
      <div class="foot">
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <div class="btns">
          <button type="button" class="btn" onclick={skip}>Skip</button>
          <button type="button" class="btn primary" data-primary onclick={next}>Next</button>
        </div>
      </div>
    {:else if step === 2}
      <div class="body" aria-live="polite">
        <h3>Press the one for Send</h3>
        <p class="lead">It presses Enter where you're typing, to send a message: the send key Yap uses after a dictation.</p>
        {@render buttons(send, dictate, 'Dictates')}
        {#if warning}<p class="warn" role="alert">{warning}</p>{/if}
      </div>
      <div class="foot">
        <button type="button" class="btn" onclick={() => go(1)}>Previous</button>
        <div class="btns">
          <button type="button" class="btn" onclick={skip}>Skip</button>
          <button type="button" class="btn primary" data-primary onclick={next}>Next</button>
        </div>
      </div>
    {:else}
      <div class="body">
        <p class="summary" data-summary>{plan?.summary || 'Nothing to set up: pick a button for Dictate or Send.'}</p>
        {#if dictate !== null}
          <div class="mode">
            <Segmented bind:value={mode} options={MODES} label="How the dictation button records" />
          </div>
          {#if plan?.modeNote}<p class="small">{fillHotkey(plan.modeNote.text, plan.modeNote.spec)}</p>{/if}
        {/if}
        {#if dictate !== null && send !== null}
          <p class="small">
            <button type="button" class="set-link" onclick={swap}>Swap them</button>
            · <button type="button" class="set-link" onclick={() => go(1)}>Pick again</button>
          </p>
        {:else}
          <p class="small"><button type="button" class="set-link" onclick={() => go(1)}>Pick again</button></p>
        {/if}
        {#each notes as b (b.button)}
          <p class="note" data-note={b.fate}>{b.note}</p>
        {/each}
        {#if plan?.canTake}
          <label class="take">
            <input type="checkbox" bind:checked={take} />
            <span>Move {takeLabel}{#if asked[0]?.by}{' '}<span class="dim">(from {asked.map((b) => b.by).join(' and ')})</span>{/if}</span>
          </label>
        {/if}
        {#if plan?.browser?.length}
          <ul class="browser" aria-label="In other apps">
            {#each plan.browser as line (line)}<li>{line}</li>{/each}
          </ul>
        {/if}
        {#if error}<p class="warn" role="alert">{error}</p>{/if}
      </div>
      <div class="foot">
        <button type="button" class="btn" onclick={() => go(2)}>Previous</button>
        <div class="btns">
          <button type="button" class="btn" onclick={close} disabled={busy}>Cancel</button>
          <button type="button" class="btn primary" data-primary disabled={busy || !plan?.changes} onclick={setUp}>
            {busy ? 'Setting up…' : 'Set up'}
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: var(--yap-paper-scrim);
    animation: scrim-in 0.15s ease-out;
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 18px;
    width: min(500px, 100%);
    max-height: min(680px, calc(100vh - 48px));
    box-sizing: border-box;
    padding: 22px 24px 20px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 14px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: sheet-in 0.15s var(--yap-ease-spring) both;
  }
  @keyframes sheet-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .sheet {
      animation: none;
    }
  }
  .stepno {
    margin: 0 0 2px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--yap-paper-muted);
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 15px;
    font-weight: 600;
    line-height: 22px;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .lead,
  .small,
  .note {
    margin: 0;
    font-size: 13.5px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
  }
  .small {
    font-size: 13px;
  }
  .note {
    color: var(--yap-paper-ink);
  }
  .warn {
    margin: 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-danger);
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    margin-top: 6px;
  }
  .side {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 12px 14px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      border-color 0.12s ease,
      background 0.12s ease;
  }
  .side:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .side.chosen {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
  }
  .side.heard {
    border-color: var(--yap-primary);
  }
  .side:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .side:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .glyph {
    font-size: 20px;
    line-height: 20px;
    color: var(--yap-paper-muted);
  }
  .bname {
    font-size: 15px;
    font-weight: 600;
  }
  .bsub {
    font-size: 12.5px;
    color: var(--yap-paper-text-2);
  }
  .summary {
    margin: 0;
    padding: 12px 14px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper-card);
    font-size: 14.5px;
    font-weight: 600;
    line-height: 21px;
  }
  .mode {
    display: flex;
  }
  .take {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 13.5px;
    line-height: 20px;
    cursor: pointer;
  }
  .take input {
    margin: 3px 0 0;
    accent-color: var(--yap-primary);
  }
  .dim {
    color: var(--yap-paper-text-2);
  }
  .browser {
    margin: 2px 0 0;
    padding: 10px 12px 10px 28px;
    border-radius: 8px;
    background: var(--yap-paper-band);
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-text-2);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .btns {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .btn {
    height: 36px;
    padding: 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .btn.primary {
    border-color: transparent;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .btn.primary:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .set-link {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .set-link:hover {
    text-decoration: underline;
  }
</style>
