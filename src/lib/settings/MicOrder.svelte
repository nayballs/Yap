<script>
  // Settings → Dictation → Microphone (dictation/mics.rs, 2026-10-25): the
  // microphones in order — dictation and meetings use the first one that's
  // connected, then Windows' default. Empty, Windows' default alone (as
  // before). Each row says whether it's connected and which one is in use;
  // ↑ ↓ move it, ✕ takes it off; "Add a microphone" lists the connected
  // ones not in the order. Windows' list comes from `list_mics` and again on
  // `yap-mics-changed` (a headset plugged in or out while this is open).
  //
  //   cfg: Settings' config copy (`micOrder`; the auto-save saves it).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Select from '../ui/Select.svelte';
  import { findMic, micInUse, shortMicName } from '../core/mics.js';

  let { cfg } = $props();

  let mics = $state([]);
  let adding = $state('');

  async function refresh() {
    try {
      mics = (await invoke('list_mics')) || [];
    } catch {
      mics = [];
    }
  }

  onMount(() => {
    refresh();
    const un = listen('yap-mics-changed', refresh);
    return () => un.then((f) => f()).catch(() => {});
  });

  const order = $derived(Array.isArray(cfg.micOrder) ? cfg.micOrder : []);
  const inUse = $derived(micInUse(order, mics));
  const windowsDefault = $derived(mics.find((m) => m.default) || null);
  const rows = $derived(
    order.map((c, i) => {
      const here = findMic(c, mics);
      return { c, i, here, using: !!here && !!inUse && here.id === inUse.id };
    }),
  );
  // The default row is in use when nothing in the order is connected.
  const defaultInUse = $derived(!!inUse && !rows.some((r) => r.using));
  const addable = $derived(mics.filter((m) => !order.some((c) => findMic(c, mics)?.id === m.id)));
  const addOptions = $derived([
    { value: '', label: order.length ? 'Add a microphone…' : 'Choose an order…' },
    ...addable.map((m) => ({ value: m.id, label: m.name })),
  ]);

  /** The order as saved: every connected one keeps its endpoint id from now on. */
  function save(next) {
    cfg.micOrder = next.map((c) => {
      const here = c.id ? null : findMic(c, mics);
      return here ? { id: here.id, name: c.name } : { id: c.id || '', name: c.name || '' };
    });
  }

  function move(i, by) {
    const next = [...order];
    const j = i + by;
    if (j < 0 || j >= next.length) return;
    [next[i], next[j]] = [next[j], next[i]];
    save(next);
  }

  function remove(i) {
    save(order.filter((_, k) => k !== i));
  }

  function add(id) {
    const m = mics.find((x) => x.id === id);
    adding = '';
    if (!m) return;
    save([...order, { id: m.id, name: m.name }]);
  }
</script>

<div class="micorder">
  <ol class="list" aria-label="Microphones in order">
    {#each rows as r (r.i)}
      <li class="mic" class:away={!r.here} class:using={r.using}>
        <span class="rank" aria-hidden="true">{r.i + 1}</span>
        <span class="name" title={r.c.name}>{shortMicName(r.c.name)}</span>
        <span class="state">{r.using ? 'In use' : r.here ? 'Connected' : 'Not connected'}</span>
        <span class="acts">
          <button class="act" title="Move up" aria-label={`Move ${shortMicName(r.c.name)} up`} disabled={r.i === 0} onclick={() => move(r.i, -1)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 15l6-6 6 6" /></svg>
          </button>
          <button class="act" title="Move down" aria-label={`Move ${shortMicName(r.c.name)} down`} disabled={r.i === rows.length - 1} onclick={() => move(r.i, 1)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          <button class="act" title="Take it off the list" aria-label={`Remove ${shortMicName(r.c.name)}`} onclick={() => remove(r.i)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        </span>
      </li>
    {/each}
    <li class="mic fallback" class:using={defaultInUse}>
      <span class="rank" aria-hidden="true">{order.length ? 'then' : ''}</span>
      <span class="name">
        Windows' default{#if windowsDefault}<span class="sub">{` · ${shortMicName(windowsDefault.name)}`}</span>{/if}
      </span>
      <span class="state">{defaultInUse ? 'In use' : mics.length ? '' : 'No microphone connected'}</span>
    </li>
  </ol>
  {#if addable.length}
    <Select bind:value={adding} options={addOptions} ariaLabel="Add a microphone" wide onchange={add} />
  {/if}
</div>

<style>
  .micorder {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    overflow: hidden;
  }
  .mic {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    padding: 4px 8px 4px 10px;
    box-sizing: border-box;
    font-size: 13px;
    color: var(--yap-fg);
  }
  .mic + .mic {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .rank {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    color: var(--yap-muted);
  }
  .away .name {
    color: var(--yap-fg-62);
  }
  .state {
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .using .state {
    color: var(--yap-primary);
    font-weight: 600;
  }
  .fallback {
    grid-template-columns: 28px minmax(0, 1fr) auto;
    color: var(--yap-fg-80);
  }
  .acts {
    display: flex;
    gap: 2px;
  }
  .act {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
    padding: 0;
  }
  .act:hover:not(:disabled) {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .act:focus-visible {
    outline: 2px solid var(--yap-focus);
    outline-offset: 1px;
  }
  .act:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .act svg {
    width: 14px;
    height: 14px;
  }
</style>
