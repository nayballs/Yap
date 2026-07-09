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
    border-top: 1px solid var(--yap-border-subtle);
    background: var(--yap-bg);
    font-size: 12px;
    color: var(--yap-muted);
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
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    padding: 4px 6px;
    margin: -4px -6px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .model:hover:not(.disabled) {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .model.disabled {
    cursor: default;
  }
  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-raised);
  }
  .dot.ready {
    background: var(--yap-success);
    box-shadow: 0 0 6px color-mix(in srgb, var(--yap-success) 50%, transparent);
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
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: 10px;
    box-shadow: var(--yap-shadow-menu);
    z-index: 50;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: none;
    border: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    padding: 7px 9px;
    border-radius: 7px;
    cursor: pointer;
  }
  .menu-item:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .menu-item.current {
    color: var(--yap-fg);
    font-weight: 600;
  }
  .mi-dot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--yap-raised);
  }
  .mi-dot.on {
    background: var(--yap-success);
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
    color: var(--yap-success);
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .saved.show {
    opacity: 1;
  }
  .link {
    background: none;
    border: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    padding: 0;
    cursor: pointer;
    transition: color 0.15s ease;
  }
  .link:hover {
    color: var(--yap-primary);
  }
  .sep {
    color: var(--yap-border-hover);
  }
  .ver {
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
</style>
