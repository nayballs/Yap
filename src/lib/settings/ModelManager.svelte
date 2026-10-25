<script>
  // Settings → Speech-to-Text's models (2026-10-22 review #9 and #12): your
  // model first — what it is, and Download when the one picked isn't on
  // this PC (never "Active" for a model that isn't there) — then the rest of
  // the catalogue under vendor filter pills (OpenWhispr's), with a quiet
  // Download each (ink only for the one recommended action). With no model
  // at all, the red badge's reason sits at the top of the page.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { MODELS, formatSize } from '../core/speechModels.js';
  import { modelStore, refreshModels, setActiveModel, downloadModel, cancelModelDownload } from '../stores/modelStore.svelte.js';
  import DownloadProgress from '../ui/DownloadProgress.svelte';
  import ModelRow from './ModelRow.svelte';
  import Group from '../ui/Group.svelte';
  import Button from '../ui/Button.svelte';
  import PillTabs from '../ui/PillTabs.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { ENGINE_PROVIDER, PROVIDER_ICONS, MONOCHROME_PROVIDERS } from '../core/providerIcons.js';

  // A download outlives this page (modelStore.svelte.js): leaving it
  // mid-download and coming back shows its bar, and a failure that came
  // meanwhile. A switch is this page's own (a moment).
  let switching = $state(null); // model id being switched to
  const busyId = $derived(modelStore.downloading?.id ?? switching); // downloaded / switched to
  const percent = $derived(modelStore.downloading?.percent ?? 0); // download progress for busyId
  let error = $state('');
  const failedDownload = $derived(modelStore.downloadError);
  const shownError = $derived(
    error ||
      (failedDownload
        ? `Couldn't download ${MODELS.find((m) => m.id === failedDownload.id)?.name ?? failedDownload.id}: ${failedDownload.message}`
        : '')
  );

  // Installed/active live in the shared store so the tray and Settings'
  // badge stay in sync with this manager.
  function statusOf(id) {
    if (busyId === id) return modelStore.installed.includes(id) ? 'switching' : 'downloading';
    if (modelStore.active === id && modelStore.installed.includes(id)) return 'active';
    if (modelStore.installed.includes(id)) return 'available';
    return 'downloadable';
  }

  const current = $derived(MODELS.find((m) => m.id === modelStore.active) || MODELS.find((m) => m.recommended) || MODELS[0]);
  const currentInstalled = $derived(modelStore.installed.includes(current.id));
  const noneInstalled = $derived(modelStore.loaded && modelStore.installed.length === 0);
  const recommended = $derived(MODELS.find((m) => m.recommended) || MODELS[0]);
  const prov = $derived(ENGINE_PROVIDER[current.engine]);

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
  // Every model but yours (it's at the top).
  const shown = $derived(
    MODELS.filter((m) => m.id !== current.id && (tab === 'all' || vendorOf(m) === tab))
  );

  onMount(() => {
    refreshModels();
  });

  /** A new action: the last errors go. */
  function clearErrors() {
    error = '';
    modelStore.downloadError = null;
  }

  // Row click / Download / Use: download if not installed, otherwise switch to it.
  async function onCard(model) {
    if (busyId) return;
    clearErrors();
    if (modelStore.installed.includes(model.id)) {
      await switchTo(model);
    } else {
      // Downloaded, then the one in use (it's ready to use): the store's
      // job, whichever page is up when it ends.
      await downloadModel(model.id);
    }
  }

  async function switchTo(model) {
    if (modelStore.active === model.id) return;
    switching = model.id;
    try {
      await setActiveModel(model.id);
    } catch (e) {
      error = `Couldn't switch to ${model.name}: ${e}`;
    } finally {
      switching = null;
    }
  }

  // Delete asks first, in Yap's own dialog: `window.confirm()` in a Tauri
  // window goes to the dialog plugin, which this window isn't allowed, so it
  // threw and the model was never deleted.
  let ask = $state({ open: false, model: null, busy: false });

  function onDelete(model) {
    if (busyId) return;
    if (modelStore.active === model.id) {
      error = `${model.name} is the active model — switch to another model first.`;
      return;
    }
    clearErrors();
    ask = { open: true, model, busy: false };
  }

  async function confirmDelete() {
    const model = ask.model;
    if (!model) return;
    ask.busy = true;
    try {
      await invoke('delete_model', { modelSize: model.id });
      await refreshModels();
    } catch (e) {
      error = `Couldn't delete ${model.name}: ${e}`;
    } finally {
      ask = { open: false, model: null, busy: false };
    }
  }

  let moreEl = $state(null);
  function change() {
    moreEl?.scrollIntoView({ block: 'start', behavior: 'smooth' });
    moreEl?.querySelector('[role="tab"][aria-selected="true"]')?.focus({ preventScroll: true });
  }
</script>

{#if noneInstalled && !busyId}
  <!-- The red badge's reason. Its fix is your model's Download just below
       (one ink button on the page), or the recommended one's here when your
       pick is another. -->
  <div class="set-banner" role="status" data-testid="stt-banner">
    <span class="dot" aria-hidden="true"></span>
    <span class="text">
      No speech-to-text model on this PC yet
      {#if current.id === recommended.id}
        <span>Download {current.name} ({formatSize(current.sizeMb)}) below to start dictating. It downloads once, then runs here.</span>
      {:else}
        <span>Yap needs one to turn your voice into text. It downloads once, then runs here.</span>
      {/if}
    </span>
    {#if current.id !== recommended.id}
      <Button variant="secondary" size="sm" onclick={() => onCard(recommended)}>Download {recommended.name} ({formatSize(recommended.sizeMb)})</Button>
    {/if}
  </div>
{/if}

<Group title="Your model" id="settings-speech-model" bare>
  <div class="current" data-setting="stt-model" class:missing={!currentInstalled}>
    {#if prov && PROVIDER_ICONS[prov]}
      <img class="cicon" class:mono={MONOCHROME_PROVIDERS.has(prov)} src={PROVIDER_ICONS[prov]} alt="" aria-hidden="true" />
    {:else}
      <svg class="cicon gen" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="6" y="6" width="12" height="12" rx="2" />
        <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
      </svg>
    {/if}
    <div class="ctext">
      <div class="cname">
        {current.name}
        {#if currentInstalled}
          <span class="state on">In use</span>
        {:else if busyId === current.id}
          <span class="state">Downloading… {percent}%</span>
        {:else}
          <span class="state off">Not downloaded</span>
        {/if}
      </div>
      <div class="cmeta">{formatSize(current.sizeMb)} · {current.langLabel} · {current.desc}</div>
      {#if busyId === current.id && modelStore.downloading}
        <DownloadProgress progress={modelStore.downloading} label={`Downloading ${current.name}`} oncancel={() => cancelModelDownload(current.id)} />
      {/if}
    </div>
    <div class="cact">
      {#if !currentInstalled && busyId !== current.id}
        <Button size="sm" onclick={() => onCard(current)} disabled={!!busyId}>Download</Button>
      {:else}
        <Button variant="secondary" size="sm" onclick={change}>Change</Button>
      {/if}
    </div>
  </div>
</Group>

<div bind:this={moreEl} class="more-anchor">
  <Group title="More models" id="settings-more-models" desc="Each downloads once and runs on this PC. Click an installed one to use it." bare>
    <div class="manager">
      <PillTabs bind:value={tab} options={TABS} />
      <div class="rows">
        {#each shown as m (m.id)}
          <ModelRow
            model={m}
            status={statusOf(m.id)}
            {percent}
            progress={modelStore.downloading?.id === m.id ? modelStore.downloading : null}
            oncancel={() => cancelModelDownload(m.id)}
            onclick={onCard}
            ondelete={onDelete}
          />
        {:else}
          <p class="empty">No other models in this group.</p>
        {/each}
      </div>
      {#if shownError}
        <p class="error" role="alert">{shownError}</p>
      {/if}
    </div>
  </Group>
</div>

<ConfirmDialog
  bind:open={ask.open}
  title={ask.model ? `Delete ${ask.model.name}?` : ''}
  body="You can download it again later."
  busy={ask.busy}
  onconfirm={confirmDelete}
/>

<style>
  .current {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 4px 0 0;
    padding: 14px 16px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    scroll-margin-top: 16px;
  }
  .cicon {
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
  }
  .cicon.gen {
    color: var(--yap-muted);
  }
  .ctext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .cname {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 620;
    color: var(--yap-fg);
  }
  .state {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    color: var(--yap-fg-80);
    font-variant-numeric: tabular-nums;
  }
  .state.on {
    background: color-mix(in srgb, var(--yap-success) 12%, var(--yap-s2));
    color: var(--yap-success-text);
  }
  .state.off {
    background: color-mix(in srgb, var(--yap-danger) 9%, var(--yap-s2));
    color: var(--yap-danger);
  }
  .cmeta {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .cact {
    flex: 0 0 auto;
  }
  .bar {
    height: 4px;
    margin-top: 4px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--yap-ink);
    transition: width var(--yap-dur) ease;
  }
  .more-anchor {
    scroll-margin-top: 16px;
  }
  .manager {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 0 0;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .empty {
    color: var(--yap-muted);
    font-size: 12.5px;
    margin: 2px 0 0;
  }
  .error {
    color: var(--yap-danger);
    font-size: 12.5px;
    margin: 4px 0 0;
  }
</style>
