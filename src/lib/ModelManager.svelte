<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { MODELS } from './models.js';
  import { modelStore, refreshModels, setActiveModel } from './modelStore.svelte.js';
  import ModelRow from './ModelRow.svelte';
  import PillTabs from './ui/PillTabs.svelte';
  import { ENGINE_PROVIDER, PROVIDER_ICONS } from './providerIcons.js';

  let busyId = $state(null); // model id being downloaded / switched to
  let percent = $state(0); // download progress for busyId
  let error = $state('');

  // Installed/active live in the shared store so the bottom StatusBar selector
  // stays in sync with this manager.
  function statusOf(id) {
    if (busyId === id) return modelStore.installed.includes(id) ? 'switching' : 'downloading';
    if (modelStore.active === id) return 'active';
    if (modelStore.installed.includes(id)) return 'available';
    return 'downloadable';
  }

  // Vendor pill tabs (OpenWhispr-style): brand tabs + a catch-all. A model's
  // tab comes from its engine family (Parakeet/Canary → NVIDIA, Whisper → OpenAI).
  const vendorOf = (m) => ENGINE_PROVIDER[m.engine] ?? 'community';
  const TABS = [
    { value: 'all', label: 'All' },
    { value: 'nvidia', label: 'NVIDIA', icon: PROVIDER_ICONS.nvidia },
    { value: 'openai', label: 'OpenAI', icon: PROVIDER_ICONS.openai, mono: true },
    { value: 'community', label: 'Community' },
  ];
  let tab = $state('all');
  const shown = $derived(tab === 'all' ? MODELS : MODELS.filter((m) => vendorOf(m) === tab));

  onMount(() => {
    refreshModels();
    const un = listen('stt-download-progress', (e) => {
      if (e.payload && e.payload.modelSize === busyId) percent = e.payload.percent;
    });
    return () => un.then((u) => u && u());
  });

  // Card click: download if not installed, otherwise switch to it.
  async function onCard(model) {
    if (busyId) return;
    error = '';
    if (modelStore.installed.includes(model.id)) {
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
      await refreshModels();
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
    if (modelStore.active === model.id) return;
    busyId = model.id;
    try {
      await setActiveModel(model.id);
    } catch (e) {
      if (!quiet) error = `Couldn't switch to ${model.name}: ${e}`;
    } finally {
      busyId = null;
    }
  }

  async function onDelete(model) {
    if (busyId) return;
    if (modelStore.active === model.id) {
      error = `${model.name} is the active model — switch to another model first.`;
      return;
    }
    if (!confirm(`Delete ${model.name}? You can re-download it later.`)) return;
    error = '';
    try {
      await invoke('delete_model', { modelSize: model.id });
      await refreshModels();
    } catch (e) {
      error = `Couldn't delete ${model.name}: ${e}`;
    }
  }
</script>

<div class="manager">
  <PillTabs bind:value={tab} options={TABS} />

  <div class="rows">
    {#each shown as m (m.id)}
      <ModelRow model={m} status={statusOf(m.id)} {percent} onclick={onCard} ondelete={onDelete} />
    {:else}
      <p class="empty">No models in this group.</p>
    {/each}
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}
</div>

<style>
  .manager {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .empty {
    color: var(--yap-muted-70);
    font-size: 12px;
    margin: 2px 0 0;
  }
  .error {
    color: var(--yap-danger);
    font-size: 12px;
    margin: 12px 0 0;
  }
</style>
