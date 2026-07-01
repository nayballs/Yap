<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import yapIcon from '../assets/yap-icon.png';
  import { MODELS } from './models.js';
  import ModelCard from './ModelCard.svelte';

  let installed = $state([]); // model ids already on disk
  let active = $state(null); // currently active model id
  let busyId = $state(null); // model id being downloaded / switched to
  let percent = $state(0); // download progress for busyId
  let error = $state('');

  // The recommended default (Parakeet V3) — powers the one-click quick-start so a
  // brand-new user can go from install → dictating without hunting through the list.
  const recommended = MODELS.find((m) => m.recommended) || MODELS[0];

  function statusOf(id) {
    if (busyId === id) return installed.includes(id) ? 'switching' : 'downloading';
    if (active === id) return 'active';
    if (installed.includes(id)) return 'available';
    return 'downloadable';
  }

  async function refresh() {
    try {
      installed = await invoke('installed_models');
      const cfg = await invoke('get_config');
      if (cfg && installed.includes(cfg.modelSize)) active = cfg.modelSize;
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

  // Pick a model: download if needed, then make it the active model.
  async function choose(model) {
    if (busyId) return;
    if (active === model.id) return;
    error = '';
    busyId = model.id;
    percent = 0;
    try {
      if (!installed.includes(model.id)) {
        await invoke('download_model_size', { modelSize: model.id });
        await refresh();
      }
      await invoke('set_active_model', { modelSize: model.id });
      active = model.id;
      await refresh();
    } catch (e) {
      error = `Couldn't set up ${model.name}: ${e}`;
    } finally {
      busyId = null;
      percent = 0;
    }
  }

  // One-click path: download + activate the recommended model, then finish. If a
  // model is already active (user picked one from the list), just finish.
  async function quickStart() {
    if (busyId) return;
    if (active) {
      getStarted();
      return;
    }
    await choose(recommended);
    if (active === recommended.id) getStarted();
  }

  function getStarted() {
    invoke('close_onboarding');
  }
</script>

<main>
  <header>
    <img class="logo" src={yapIcon} alt="" aria-hidden="true" />
    <h1>Welcome to Yap</h1>
    <p class="sub">
      Just hit <strong>Download {recommended.name} &amp; start</strong> below — or pick a
      different model from the list. Everything runs locally on your machine; your voice
      never leaves it. You can change this any time in Settings.
    </p>
  </header>

  <div class="cards">
    {#each MODELS as m (m.id)}
      <ModelCard model={m} status={statusOf(m.id)} {percent} onclick={choose} />
    {/each}
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <footer>
    <button class="skip" onclick={getStarted}>I'll choose later</button>
    <button class="start" onclick={quickStart} disabled={!!busyId}>
      {#if busyId === recommended.id}
        Downloading {recommended.name}… {percent}%
      {:else if busyId}
        Setting up… {percent}%
      {:else if active}
        Get started →
      {:else}
        Download {recommended.name} & start
      {/if}
    </button>
  </footer>
</main>

<style>
  :global(body) {
    background: #0f1117;
  }
  main {
    box-sizing: border-box;
    min-height: 100vh;
    background: #0f1117;
    color: #e5e7eb;
    padding: 26px 28px 22px;
    font-family: system-ui, -apple-system, sans-serif;
  }

  header {
    text-align: center;
    margin-bottom: 22px;
  }
  .logo {
    width: 64px;
    height: 64px;
    margin: 0 auto 12px;
    border-radius: 14px;
    object-fit: contain;
  }
  h1 {
    font-size: 22px;
    margin: 0 0 8px;
    letter-spacing: 0.01em;
  }
  .sub {
    color: #9ca3af;
    font-size: 13px;
    line-height: 1.6;
    max-width: 440px;
    margin: 0 auto;
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .error {
    color: #fca5a5;
    font-size: 12px;
    text-align: center;
    margin: 12px 0 0;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 20px;
  }
  .skip {
    background: none;
    border: none;
    color: #6b7280;
    font-size: 12.5px;
    cursor: pointer;
    padding: 8px 4px;
  }
  .skip:hover {
    color: #9ca3af;
  }
  .start {
    border: none;
    border-radius: 9px;
    background: #3b82f6;
    color: #fff;
    font-size: 14px;
    font-weight: 500;
    padding: 11px 20px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .start:hover:not(:disabled) {
    background: #2563eb;
  }
  .start:disabled {
    background: #1f2733;
    color: #6b7280;
    cursor: default;
  }
</style>
