<script>
  // Chat's context ring (after Claude's own chat box, 2026-10-25): bottom
  // right under the message box, a small ring that fills as the open
  // conversation uses up the Chat model's context (ai/usage/context.rs
  // `chat_context`: the last answer's prompt and answer as the model counted
  // them, else an estimate; against the model the chip names). Hover or
  // focus: "38% of context used". Click: ContextPanel — this chat, today's
  // and the week's AI use by model, "See all" in Insights.
  //
  // Read again after each answer (`stamp`), when another conversation opens,
  // and when the Chat model changes (the picker, Settings, the on-device
  // model's context). Nothing to show on a new chat: it isn't there.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick, untrack } from 'svelte';
  import { percentOf, ringColor, ringLabel } from '../../core/aiUsage.js';

  /**
   * `conversationId`: the open conversation (null: a new chat).
   * `stamp`: goes up after each answer. `onnewchat()`: the panel's New
   * chat. `onseeall()`: Insights' AI usage card.
   */
  let { conversationId = null, stamp = 0, onnewchat = null, onseeall = null } = $props();

  let use = $state(null);
  let open = $state(false);
  let hover = $state(false);
  let wrapEl = $state(null);
  let ringEl = $state(null);
  let seq = 0;

  const shown = $derived(!!use && use.context > 0 && use.messages > 0);
  const pct = $derived(use ? percentOf(use.used, use.context) : 0);
  const label = $derived(use ? ringLabel(use) : '');
  // The ring: r 7 in an 18 px box.
  const R = 7;
  const C = 2 * Math.PI * R;
  const dash = $derived(C * (1 - Math.min(100, Math.max(0, pct)) / 100));

  async function refresh() {
    const mine = ++seq;
    const id = conversationId;
    if (id == null) {
      use = null;
      open = false;
      return;
    }
    try {
      const u = await invoke('chat_context', { conversationId: id });
      if (mine === seq) use = u;
    } catch {
      /* keep the last */
    }
  }

  $effect(() => {
    void conversationId;
    void stamp;
    untrack(refresh);
  });

  function toggle() {
    open = !open;
    if (open) refresh();
  }

  function close(focusRing) {
    open = false;
    if (focusRing) tick().then(() => ringEl?.focus());
  }

  function onWindowPointer(e) {
    if (open && wrapEl && !e.composedPath().includes(wrapEl)) close(false);
  }

  onMount(() => {
    // The Chat model switched (the picker, Settings) or the on-device
    // model's context changed: the denominator follows.
    const uns = [listen('yap-chat-model-changed', refresh), listen('yap-config-saved', refresh), listen('yap-local-llm-changed', refresh)];
    return () => uns.forEach((u) => u.then((f) => f()));
  });
</script>

<svelte:window onpointerdowncapture={onWindowPointer} />

{#if shown}
  <div class="ctxring" bind:this={wrapEl} data-testid="context-ring" data-percent={pct}>
    <button
      class="ring"
      bind:this={ringEl}
      aria-label={label}
      aria-haspopup="dialog"
      aria-expanded={open}
      onclick={toggle}
      onpointerenter={() => (hover = true)}
      onpointerleave={() => (hover = false)}
      onfocus={() => (hover = true)}
      onblur={() => (hover = false)}
    >
      <svg viewBox="0 0 18 18" aria-hidden="true">
        <circle class="track" cx="9" cy="9" r={R} />
        <circle
          class="fill"
          cx="9"
          cy="9"
          r={R}
          style="stroke:{ringColor(pct)};stroke-dasharray:{C};stroke-dashoffset:{dash}"
        />
      </svg>
    </button>
    {#if hover && !open}
      <span class="tip" role="tooltip" data-testid="context-tip">{label}</span>
    {/if}
    {#if open}
      <!-- Its own chunk, loaded the first time it opens (the main window's
           stays under Vite's 700 kB warning). -->
      {#await import('./ContextPanel.svelte') then { default: ContextPanel }}
        <ContextPanel
          {use}
          onclose={close}
          onnewchat={() => {
            close(false);
            onnewchat?.();
          }}
          onseeall={() => {
            close(false);
            onseeall?.();
          }}
        />
      {/await}
    {/if}
  </div>
{/if}

<style>
  .ctxring {
    position: relative;
    display: flex;
    align-items: center;
    flex: 0 0 auto;
  }
  .ring {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-full);
    background: none;
    cursor: pointer;
  }
  .ring:hover,
  .ring[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .ring:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  svg {
    width: 18px;
    height: 18px;
    transform: rotate(-90deg);
  }
  circle {
    fill: none;
    stroke-width: 2.5;
  }
  .track {
    stroke: var(--yap-raised);
  }
  .fill {
    stroke-linecap: round;
    transition:
      stroke-dashoffset 0.4s ease,
      stroke 0.4s ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
  :global([data-yap-motion='reduced']) .fill {
    transition: none;
  }
  /* Beside the ring, on its left (above it would cover the mic). */
  .tip {
    position: absolute;
    right: calc(100% + 6px);
    top: 50%;
    transform: translateY(-50%);
    z-index: 21;
    padding: 4px 8px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg);
    font-size: 11.5px;
    white-space: nowrap;
    pointer-events: none;
  }
</style>
