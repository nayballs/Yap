<script>
  // "Set up meeting notes" (2026-10-29; Wispr Flow's "Set up Notetaker"
  // checklist, by behaviour): a small card in the main window's sidebar with
  // three things to do — connect a calendar, record a first meeting (or try
  // one), add Yap to an AI app (MCP) — each ticked by itself as it's done
  // (`meeting_setup_status`, shell/guide.rs: asked again as the view changes,
  // a calendar connects, a meeting ends or the window comes back to the
  // front). ✕ dismisses it for good; done, it goes by itself. A short window
  // leaves it out (the sidebar's rows come first).
  //
  //   view        the view on screen (a change asks again)
  //   onconnect   "Connect your calendar"
  //   onmeetings  the Meetings view
  //   onmcp       Settings → AI apps (MCP)
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { calendar } from '../stores/calendar.svelte.js';

  let { view = '', onconnect, onmeetings, onmcp } = $props();

  let status = $state(null);
  let gone = false;

  async function refresh() {
    try {
      const s = await invoke('meeting_setup_status');
      if (!gone) status = s;
    } catch {
      // best-effort
    }
  }

  $effect(() => {
    // Asked again as the view changes, and as a calendar connects.
    view;
    calendar.connections.length;
    refresh();
  });

  onMount(() => {
    const uns = [];
    listen('yap-meeting-ended', refresh).then((u) => uns.push(u));
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      gone = true;
      uns.forEach((u) => u && u());
      window.removeEventListener('focus', onFocus);
    };
  });

  const items = $derived(
    status
      ? [
          { id: 'calendar', label: 'Connect a calendar', done: status.calendar, run: onconnect },
          { id: 'meeting', label: 'Record your first meeting', hint: 'or try one now', done: status.meeting, run: onmeetings },
          { id: 'mcp', label: 'Add Yap to an AI app', hint: 'Claude, ChatGPT…', done: status.mcp, run: onmcp },
        ]
      : []
  );
  const doneCount = $derived(items.filter((i) => i.done).length);

  function dismiss() {
    status = { ...status, shown: false, dismissed: true };
    invoke('meeting_setup_dismiss').catch(() => {});
  }
</script>

{#if status?.shown}
  <section class="setup" aria-labelledby="ms-title" data-testid="meeting-setup">
    <div class="head">
      <h2 id="ms-title">Set up meeting notes</h2>
      <button type="button" class="x" aria-label="Dismiss meeting notes setup" onclick={dismiss}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
      </button>
    </div>
    <div class="bar" role="progressbar" aria-label="Meeting notes setup" aria-valuemin="0" aria-valuemax="3" aria-valuenow={doneCount}>
      <span style="width:{(doneCount / 3) * 100}%"></span>
    </div>
    <ul>
      {#each items as item (item.id)}
        <li>
          <button type="button" class="item" class:done={item.done} data-item={item.id} aria-pressed={item.done} disabled={item.done} onclick={() => item.run?.()}>
            <span class="tick" aria-hidden="true">
              {#if item.done}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M6 12.5l4 4 8-9" /></svg>
              {/if}
            </span>
            <span class="text">
              <span class="label">{item.label}</span>
              {#if item.hint && !item.done}<span class="hint">{item.hint}</span>{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .setup {
    margin: 0 2px 8px;
    padding: 10px 10px 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  h2 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 700;
    color: var(--yap-fg);
  }
  .x {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .x svg {
    width: 12px;
    height: 12px;
  }
  .bar {
    height: 3px;
    margin: 7px 0 6px;
    border-radius: 2px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--yap-primary);
    transition: width 200ms ease;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    padding: 5px 4px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .item:hover:not(:disabled) {
    background: var(--yap-raised-soft);
  }
  .item:disabled {
    cursor: default;
  }
  .tick {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 15px;
    height: 15px;
    margin-top: 1px;
    border: 1.5px solid var(--yap-switch-edge, var(--yap-border));
    border-radius: 50%;
  }
  .item.done .tick {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .tick svg {
    width: 10px;
    height: 10px;
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .label {
    font-size: 12.5px;
    font-weight: 560;
    line-height: 1.3;
  }
  .item.done .label {
    color: var(--yap-muted);
    text-decoration: line-through;
  }
  .hint {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  /* A short window: the sidebar's own rows first. */
  @media (max-height: 700px) {
    .setup {
      display: none;
    }
  }
</style>
