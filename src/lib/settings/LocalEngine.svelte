<script>
  // Settings → Language Models → the on-device models: the engine that runs
  // them (ai/local_llm/engine.rs). One line — "Engine: llama.cpp b11982 · up
  // to date" — with Check now; "Keep the engine up to date" (on by default:
  // a look once a day, a build taken once it's 3 days old and tried with the
  // person's own model, at most one a week by itself); what happened last
  // ("Updated to b11980 · faster on your card", "b11990 didn't start here:
  // staying on b11960"); and Go back to the build before. After a fresh
  // install's build didn't start: "setting up another build for this PC",
  // then the build that runs ("CUDA didn't start here, so the engine runs on
  // Vulkan"), or — none of them started — "none runs on this PC" with Try
  // again. A download names its size (CUDA's ~590 MB). The state is
  // Rust's (`local_llm_status().engineState`, then `yap-local-engine` as a
  // check moves along), kept in the engine's own marker, never config.json.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Toggle from '../ui/Toggle.svelte';

  let { initial = null } = $props();

  let eng = $state(null);
  let busy = $state(false);
  let error = $state('');

  /** The newest status kept (Rust numbers each, `rev`): Check now answers
   * with one made before its look, and the look's events can come first. */
  function take(next) {
    if (!next) return;
    if (eng?.rev != null && next.rev != null && next.rev < eng.rev) return;
    eng = next;
  }

  $effect(() => {
    if (initial && !eng) eng = initial;
  });

  onMount(() => {
    let off = null;
    let alive = true;
    listen('yap-local-engine', (e) => take(e.payload))
      .then((u) => {
        if (alive) off = u;
        else u();
      })
      .catch(() => {});
    invoke('local_engine_status')
      .then(take)
      .catch(() => {});
    return () => {
      alive = false;
      off?.();
    };
  });

  const s = $derived(eng || initial || {});
  const working = $derived(!!s.phase || !!s.rescuing);
  // "Engine: …": the build, or — after the first build didn't start — what
  // Yap is doing about it, or that none runs here (said plainly).
  const name = $derived(s.label || (s.rescuing ? 'setting up another build for this PC' : s.failed ? 'none runs on this PC' : ''));

  // What it's doing now, in place of "up to date".
  const status = $derived.by(() => {
    const c = s.candidate;
    const size = s.downloadMb ? ` (${s.downloadMb >= 1024 ? `${(s.downloadMb / 1024).toFixed(1)} GB` : `${s.downloadMb} MB`})` : '';
    switch (s.phase) {
      case 'checking':
        return 'checking for a newer build…';
      case 'downloading':
        return `downloading ${c || 'a newer build'}${size}${s.percent ? ` · ${s.percent}%` : ''}`;
      case 'waiting':
        return `${c || 'a newer build'} is ready to try · waiting until the model isn't in use`;
      case 'trying':
        return `trying ${c || 'a newer build'} with your model…`;
    }
    if (!s.label) return '';
    if (s.kind === 'llamafile') return s.auto ? "moves to llama.cpp's own builds once one has been tried here" : '';
    if (s.upToDate) return 'up to date';
    if (s.latest && s.build && s.latest !== s.build) return `${s.latest} is available`;
    return '';
  });

  async function checkNow() {
    error = '';
    busy = true;
    try {
      take(await invoke('local_engine_check'));
    } catch (e) {
      error = `${e}`;
    } finally {
      busy = false;
    }
  }

  async function setAuto(on) {
    error = '';
    try {
      take(await invoke('local_engine_set_auto', { on }));
    } catch (e) {
      error = `${e}`;
    }
  }

  async function goBack() {
    error = '';
    try {
      take(await invoke('local_engine_go_back'));
    } catch (e) {
      error = `${e}`;
    }
  }
</script>

{#if name}
  <div class="lb-load lb-engine" data-testid="local-engine" data-setting="local-engine">
    <div class="lb-state">
      <span class="lb-statetext" class:lb-enginefailed={s.failed && !s.label} role="status" data-testid="local-engine-line">
        Engine: {name}{#if s.variant && s.kind === 'llamacpp'}<span class="lb-variant">{` (${s.variant})`}</span>{/if}{#if status}<span class="lb-engstatus">{` · ${status}`}</span>{/if}
      </span>
      <button class="lb-later" onclick={checkNow} disabled={working || busy}>{s.failed && !s.label ? 'Try again' : 'Check now'}</button>
    </div>
    {#if s.line}
      <span class="lb-engineevent" data-testid="local-engine-event">{s.line}</span>
    {/if}
    {#if s.error || error}
      <span class="lb-engineerr" role="alert">{error || s.error}</span>
    {/if}
    <Toggle
      checked={s.auto !== false}
      label="Keep the engine up to date"
      desc="Yap looks once a day and takes a newer llama.cpp build once it's 3 days old, after trying it with your model out of sight. At most one a week by itself; Check now takes one at once."
      onchange={setAuto}
    />
    {#if s.backTo}
      <span class="lb-enginefoot">
        <button class="lb-later" onclick={goBack} disabled={working}>Go back to {s.backTo}</button>
      </span>
    {/if}
  </div>
{/if}

<style>
  .lb-load {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s1);
    font-size: 12.5px;
  }
  .lb-state {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    color: var(--yap-fg);
    font-weight: 600;
  }
  .lb-statetext {
    flex: 1 1 auto;
  }
  .lb-variant,
  .lb-engstatus {
    font-weight: 400;
    color: var(--yap-muted);
  }
  .lb-engineevent,
  .lb-enginefoot {
    color: var(--yap-muted);
    line-height: 1.45;
  }
  .lb-engineerr,
  .lb-enginefailed {
    color: var(--yap-danger);
  }
  .lb-later {
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--yap-fg-62);
    font-size: 12px;
    cursor: pointer;
    padding: 2px 4px;
  }
  .lb-later:hover:not(:disabled) {
    color: var(--yap-fg);
  }
  .lb-later:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .lb-later:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
</style>
