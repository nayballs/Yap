<script>
  import { onMount } from 'svelte';
  import { MODELS } from './models.js';
  import { modelStore, refreshModels, setActiveModel } from './modelStore.svelte.js';

  // Settings owns the updater flow + the "Saved" pulse; we just trigger / show.
  let { saved = false, oncheckupdates } = $props();

  let version = $state('0.1.0');
  let menuOpen = $state(false);
  let switching = $state(false);

  onMount(async () => {
    refreshModels();
    try {
      const { getVersion } = await import('@tauri-apps/api/app');
      version = await getVersion();
    } catch {
      version = '0.1.0';
    }
  });

  // Installed models available to switch between, in catalog order.
  const installedModels = $derived(MODELS.filter((m) => modelStore.installed.includes(m.id)));

  const activeModel = $derived(MODELS.find((m) => m.id === modelStore.active) || null);
  const activeName = $derived(activeModel ? activeModel.name : 'No model');
  // Green only when the active model is actually installed and ready.
  const ready = $derived(!!modelStore.active && modelStore.installed.includes(modelStore.active));

  async function pick(id) {
    menuOpen = false;
    if (id === modelStore.active || switching) return;
    switching = true;
    try {
      await setActiveModel(id);
    } catch {
      /* surfaced in the Models section instead */
    } finally {
      switching = false;
    }
  }

  function toggleMenu() {
    if (installedModels.length === 0) return;
    menuOpen = !menuOpen;
  }
</script>

<svelte:window onclick={() => (menuOpen = false)} />

<div class="statusbar">
  <div class="left">
    <button
      class="model"
      class:disabled={installedModels.length === 0}
      onclick={(e) => {
        e.stopPropagation();
        toggleMenu();
      }}
      title="Switch active model"
    >
      <span class="dot" class:ready></span>
      <span class="name">{switching ? 'Switching…' : activeName}</span>
      {#if installedModels.length > 0}
        <svg class="chev" class:up={menuOpen} width="12" height="12" viewBox="0 0 24 24"
          fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M6 9l6 6 6-6" />
        </svg>
      {/if}
    </button>

    {#if menuOpen}
      <div class="menu" role="menu">
        {#each installedModels as m (m.id)}
          <button
            class="menu-item"
            class:current={m.id === modelStore.active}
            role="menuitem"
            onclick={(e) => {
              e.stopPropagation();
              pick(m.id);
            }}
          >
            <span class="mi-dot" class:on={m.id === modelStore.active}></span>
            <span class="mi-name">{m.name}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="right">
    <span class="saved" class:show={saved}>Saved ✓</span>
    <button class="link" onclick={() => oncheckupdates?.()}>Check for updates</button>
    <span class="sep">•</span>
    <span class="ver">v{version}</span>
  </div>
</div>

<style>
  .statusbar {
    flex: 0 0 auto;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.08);
    background: #0c0e14;
    font-size: 12px;
    color: #9ca3af;
  }
  .left {
    position: relative;
    min-width: 0;
  }
  .model {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 240px;
    background: none;
    border: none;
    color: #9ca3af;
    font: inherit;
    font-size: 12px;
    padding: 4px 6px;
    margin: -4px -6px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .model:hover:not(.disabled) {
    background: rgba(255, 255, 255, 0.05);
    color: #e5e7eb;
  }
  .model.disabled {
    cursor: default;
  }
  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #4b5563;
  }
  .dot.ready {
    background: #22c55e;
    box-shadow: 0 0 6px rgba(34, 197, 94, 0.6);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    flex: 0 0 auto;
    transition: transform 0.15s ease;
  }
  .chev.up {
    transform: rotate(180deg);
  }
  .menu {
    position: absolute;
    bottom: calc(100% + 8px);
    left: -6px;
    min-width: 200px;
    max-height: 280px;
    overflow-y: auto;
    padding: 4px;
    background: #161922;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5);
    z-index: 50;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: none;
    border: none;
    color: #cbd5e1;
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    padding: 7px 9px;
    border-radius: 7px;
    cursor: pointer;
  }
  .menu-item:hover {
    background: rgba(255, 255, 255, 0.06);
    color: #e5e7eb;
  }
  .menu-item.current {
    color: #fff;
  }
  .mi-dot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #4b5563;
  }
  .mi-dot.on {
    background: #22c55e;
  }
  .mi-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
  }
  .saved {
    color: #22c55e;
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .saved.show {
    opacity: 1;
  }
  .link {
    background: none;
    border: none;
    color: #9ca3af;
    font: inherit;
    font-size: 12px;
    padding: 0;
    cursor: pointer;
    transition: color 0.15s ease;
  }
  .link:hover {
    color: #3b82f6;
  }
  .sep {
    color: #4b5563;
  }
  .ver {
    color: #6b7280;
    font-variant-numeric: tabular-nums;
  }
</style>
