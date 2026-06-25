<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { MODELS } from './models.js';
  import ModelCard from './ModelCard.svelte';

  let installed = $state([]); // model ids already on disk
  let active = $state(null); // currently loaded / active model id
  let busyId = $state(null); // model id being downloaded / switched to
  let percent = $state(0); // download progress for busyId
  let error = $state('');

  function statusOf(id) {
    if (busyId === id) return installed.includes(id) ? 'switching' : 'downloading';
    if (active === id) return 'active';
    if (installed.includes(id)) return 'available';
    return 'downloadable';
  }

  // "Your models" (installed) vs "Available" grouping.
  const yours = $derived(MODELS.filter((m) => installed.includes(m.id)));
  const available = $derived(MODELS.filter((m) => !installed.includes(m.id)));

  async function refresh() {
    try {
      installed = await invoke('installed_models');
      const cfg = await invoke('get_config');
      if (cfg) active = cfg.modelSize ?? null;
    } catch (e) {
      // best-effort
    }
  }

  onMount(() => {
    refresh();
    const un = listen('stt-download-progress', (e) => {
      if (e.payload && e.payload.modelSize === busyId) percent = e.payload.percent;
    });
    return () => un.then((u) => u && u());
  });

  // Card click: download if not installed, otherwise switch to it.
  async function onCard(model) {
    if (busyId) return;
    error = '';
    if (installed.includes(model.id)) {
      await switchTo(model);
    } else {
      await download(model);
    }
  }

  async function download(model) {
    busyId = model.id;
    percent = 0;
    try {
      await invoke('download_model_size', { modelSize: model.id });
      await refresh();
      // Newly downloaded model becomes active (it's now ready to use).
      await switchTo(model, true);
    } catch (e) {
      error = `Couldn't download ${model.name}: ${e}`;
    } finally {
      busyId = null;
      percent = 0;
    }
  }

  async function switchTo(model, quiet = false) {
    if (active === model.id) return;
    busyId = model.id;
    try {
      await invoke('set_active_model', { modelSize: model.id });
      active = model.id;
      await refresh();
    } catch (e) {
      if (!quiet) error = `Couldn't switch to ${model.name}: ${e}`;
    } finally {
      busyId = null;
    }
  }

  async function onDelete(model) {
    if (busyId) return;
    if (active === model.id) {
      error = `${model.name} is the active model — switch to another model first.`;
      return;
    }
    if (!confirm(`Delete ${model.name}? You can re-download it later.`)) return;
    error = '';
    try {
      await invoke('delete_model', { modelSize: model.id });
      await refresh();
    } catch (e) {
      error = `Couldn't delete ${model.name}: ${e}`;
    }
  }
</script>

<div class="manager">
  {#if yours.length > 0}
    <h3 class="grouptitle">Your models</h3>
    <div class="cards">
      {#each yours as m (m.id)}
        <ModelCard
          model={m}
          status={statusOf(m.id)}
          {percent}
          onclick={onCard}
          ondelete={onDelete}
        />
      {/each}
    </div>
  {/if}

  <h3 class="grouptitle">{yours.length > 0 ? 'Available to download' : 'Available models'}</h3>
  {#if available.length > 0}
    <div class="cards">
      {#each available as m (m.id)}
        <ModelCard model={m} status={statusOf(m.id)} {percent} onclick={onCard} />
      {/each}
    </div>
  {:else}
    <p class="empty">All models downloaded.</p>
  {/if}

  {#if error}
    <p class="error">{error}</p>
  {/if}
</div>

<style>
  .manager {
    display: flex;
    flex-direction: column;
  }
  .grouptitle {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #9ca3af;
    margin: 4px 0 8px;
  }
  .grouptitle:not(:first-child) {
    margin-top: 18px;
  }
  .cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .empty {
    color: #6b7280;
    font-size: 12px;
    margin: 2px 0 0;
  }
  .error {
    color: #fca5a5;
    font-size: 12px;
    margin: 12px 0 0;
  }
</style>
