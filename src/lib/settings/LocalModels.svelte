<script>
  // Settings → Language Models → Dictation cleanup → Local: the on-device
  // model browser (ai/local_llm.rs, Mozilla llamafile): Recommended for this
  // PC, the bigger model offered once, the slow-on-this-PC line, the Context
  // control (LocalContext.svelte), the curated models by family with
  // Download / Use / Delete…, and your own GGUFs. The status and the actions
  // are settings/localLlm.svelte.js's: a download carries on, with its
  // progress, when you leave the page.
  //
  //   cfg: Settings' config $state (auto-save does the saving).
  import Select from '../ui/Select.svelte';
  import PillTabs from '../ui/PillTabs.svelte';
  import LocalContext from '../components/LocalContext.svelte';
  import { upTo } from '../core/localContext.js';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm } from '../ui/contextMenu.svelte.js';
  import {
    localLlm,
    LLM_FAMILIES,
    localFamilies,
    activeLocalFile,
    onLocalModelChange,
    openLlmFolder,
    downloadCurated,
    activateCurated,
    takeUpgrade,
    dismissUpgrade,
    deleteCurated,
  } from './localLlm.svelte.js';

  let { cfg } = $props();

  const llm = $derived(localLlm.status);
  // What each install stage actually downloads, so the progress bar names it.
  const LOCAL_STAGE_LABELS = {
    runtime: 'llamafile engine (local AI server)',
    model: 'model',
  };

  const activeFile = $derived(activeLocalFile(cfg));
  // User-supplied GGUFs in the folder that aren't part of the curated set.
  const byoModelOptions = $derived(
    (llm.models || [])
      .filter((f) => !(llm.curated || []).some((c) => c.filename === f))
      .map((f) => ({ value: f, label: f.replace(/\.gguf$/i, '') }))
  );
  function familyIcon(fam) {
    return (LLM_FAMILIES.find((f) => f.value === fam) || {}).icon || '';
  }
  // Families actually present in the curated set, kept in the fixed order.
  const families = $derived(localFamilies());
  const familyModels = $derived((llm.curated || []).filter((m) => m.family === localLlm.family));

  function fmtLlmSize(mb) {
    const n = Number(mb) || 0;
    return n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${n} MB`;
  }
  // The HuggingFace repo page behind a curated model's resolve URL ("Learn more").
  function hfRepo(url) {
    if (!url) return '';
    const i = url.indexOf('/resolve/');
    return i > 0 ? url.slice(0, i) : url;
  }

  // The trash button and the card's right-click Delete… ask first, as the
  // speech models do.
  let curatedEls = $state({});
  function askDeleteCurated(m) {
    askConfirm({
      title: `Delete ${m.display}?`,
      body: `The ${fmtLlmSize(m.sizeMb)} download leaves this PC. You can download it again at any time.`,
      run: () => deleteCurated(m),
      returnFocus: curatedEls[m.id],
    });
  }

  // A local model card's right-click menu (ui/contextMenu.svelte.js): what
  // its buttons do — Download, or Use — and Delete…. Nothing while it
  // downloads, or with cleanup off (its buttons are off then too).
  function curatedMenu(e, m) {
    const isActive = m.installed && activeFile === m.filename;
    if (localLlm.downloadingId === m.id) return;
    const off = !cfg.postProcessEnabled;
    openContextMenu(
      e,
      [
        !m.installed
          ? { label: 'Download', icon: 'download', disabled: off || !!localLlm.downloadingId, onselect: () => downloadCurated(m) }
          : null,
        m.installed && !isActive
          ? { label: 'Use', icon: 'use', disabled: off || localLlm.switching, onselect: () => activateCurated(m) }
          : null,
        m.installed && !isActive ? { separator: true } : null,
        m.installed && !isActive
          ? { label: 'Delete…', icon: 'trash', danger: true, onselect: () => askDeleteCurated(m) }
          : null,
      ],
      { label: m.display, row: e.type === 'keydown' ? e.target : curatedEls[m.id] }
    );
  }
</script>

<!-- Brand icon for a local-model family tab (img for known brands, chip svg for Phi). -->
{#snippet familyTabIcon(v)}
  {#if familyIcon(v)}
    <img src={familyIcon(v)} alt="" aria-hidden="true" />
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2" /><rect x="9.5" y="9.5" width="5" height="5" /></svg>
  {/if}
{/snippet}

<div class="localbrowser">
  <div class="lb-head">
    <span class="lb-title">On-device models</span>
    <span class="lb-sub">Downloaded once, then they run on this PC (Mozilla llamafile): no key, nothing sent.</span>
  </div>
  {#if llm.slowOnThisPc}
    {@const small = (llm.curated || []).find((c) => c.isDefault)}
    <div class="lb-note slow" role="status" data-testid="local-slow">
      <span>
        {#if llm.placement?.kind === 'partial'}
          Runs partly on the processor on this PC ({llm.placement.gpuLayers} of {llm.placement.layers} layers on the graphics card): answers will be slow — {small?.display || 'Qwen2.5 1.5B'} is quicker.
        {:else}
          Runs on the processor on this PC: answers will be slow — {small?.display || 'Qwen2.5 1.5B'} is quicker.
        {/if}
      </span>
      {#if small}
        {#if small.installed}
          <button class="lb-use" onclick={() => activateCurated(small)} disabled={!cfg.postProcessEnabled || localLlm.switching}>Use {small.display}</button>
        {:else}
          <button class="lb-use" onclick={() => downloadCurated(small)} disabled={!cfg.postProcessEnabled || !!localLlm.downloadingId}>Download {small.display}</button>
        {/if}
      {/if}
    </div>
  {:else if llm.upgrade}
    {@const up = (llm.curated || []).find((c) => c.id === llm.upgrade.id)}
    <div class="lb-note" data-testid="local-upgrade">
      <span>
        {#if llm.gpu?.name}Your {llm.gpu.name} can run{:else}This PC can run{/if}
        {llm.upgrade.display}: better answers in Chat, and it can use Chat's tools on your notes, apps and the web. {llm.upgrade.installed ? 'Already downloaded.' : `A ${fmtLlmSize(llm.upgrade.sizeMb)} download.`}
      </span>
      {#if up}
        <button class="lb-use" onclick={() => takeUpgrade(up)} disabled={!cfg.postProcessEnabled || !!localLlm.downloadingId || localLlm.switching}>
          {up.installed ? `Use ${up.display}` : `Download ${up.display}`}
        </button>
      {/if}
      <button class="lb-later" onclick={dismissUpgrade}>No thanks</button>
    </div>
  {/if}
  {#if llm.installed}
    <!-- How much the on-device model reads at once: one setting
         for Chat, cleanup and meeting notes (one sidecar). -->
    <div class="lb-context">
      <LocalContext status={llm} disabled={!cfg.postProcessEnabled} onchange={(v) => (localLlm.status = { ...localLlm.status, context: v })} />
    </div>
  {/if}
  {#if families.length > 1}
    <div class="lb-families">
      <PillTabs value={localLlm.family} options={families} onchange={(v) => (localLlm.family = v)} renderIcon={familyTabIcon} />
    </div>
  {/if}
  <div class="lb-list">
    {#each familyModels as m (m.id)}
      {@const isActive = m.installed && activeFile === m.filename}
      {@const isDownloading = localLlm.downloadingId === m.id}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="lb-card"
        class:sel={isActive}
        class:menuopen={menuOpenFor(curatedEls[m.id])}
        bind:this={curatedEls[m.id]}
        tabindex="-1"
        oncontextmenu={(e) => curatedMenu(e, m)}
        onkeydown={(e) => menuKey(e, (k) => curatedMenu(k, m))}
      >
        <span class="lb-dot" class:on={isActive}></span>
        {#if familyIcon(m.family)}
          <img class="lb-brand" src={familyIcon(m.family)} alt="" aria-hidden="true" />
        {:else}
          <span class="lb-brand lb-brandfb">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2" /><rect x="9.5" y="9.5" width="5" height="5" /></svg>
          </span>
        {/if}
        <div class="lb-info">
          <div class="lb-top">
            <span class="lb-name">{m.display}</span>
            <span class="lb-size">{[fmtLlmSize(m.sizeMb), upTo(m.maxContext)].filter(Boolean).join(' · ')}</span>
            {#if m.recommended}<span class="lb-badge">Recommended for this PC</span>{/if}
            {#if m.url}<a class="lb-learn" href={hfRepo(m.url)} onclick={createExternalLinkHandler(hfRepo(m.url))} target="_blank" rel="noreferrer">Learn more ↗</a>{/if}
          </div>
          <span class="lb-blurb">{m.blurb}</span>
          {#if isDownloading}
            <div class="lb-progress">
              <span class="lb-plabel">{LOCAL_STAGE_LABELS[localLlm.progress.stage] || 'Downloading'}… {localLlm.progress.percent}%{#if localLlm.progress.total_mb} · {Math.round(localLlm.progress.downloaded_mb)} / {Math.round(localLlm.progress.total_mb)} MB{/if}</span>
              <div class="lb-bar"><span style="width:{localLlm.progress.percent}%"></span></div>
            </div>
          {/if}
        </div>
        <div class="lb-action">
          {#if isDownloading}
            <span class="lb-busy">Downloading…</span>
          {:else if isActive}
            <span class="lb-activetag">✓ Active</span>
          {:else if m.installed}
            <button class="lb-use" onclick={() => activateCurated(m)} disabled={!cfg.postProcessEnabled || localLlm.switching}>Use</button>
            <button class="lb-del" title="Delete download" aria-label="Delete download" onclick={() => askDeleteCurated(m)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M6 6v14a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V6M10 11v6M14 11v6" /></svg>
            </button>
          {:else}
            <button class="lb-dl" onclick={() => downloadCurated(m)} disabled={!cfg.postProcessEnabled || !!localLlm.downloadingId}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12M7 10l5 5 5-5M5 21h14" /></svg>
              Download
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
  <p class="lb-byo">
    Or bring your own: drop any <code>.gguf</code> into the
    <button class="ondevice-link" onclick={openLlmFolder}>models folder</button>.
  </p>
  {#if byoModelOptions.length}
    <div class="ondevice-models">
      <Select bind:value={cfg.ppLocalModel} options={byoModelOptions} onchange={onLocalModelChange} disabled={!cfg.postProcessEnabled || localLlm.switching} />
    </div>
  {/if}
  {#if localLlm.error}<span class="ondevice-err">{localLlm.error}</span>{/if}
</div>

<style>
  .ondevice-models {
    margin-top: 8px;
  }
  .ondevice-link {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .ondevice-err {
    color: var(--yap-danger);
    font-size: 12.5px;
  }

  /* On-device model browser (curated downloadable cleanup models). */
  .localbrowser {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .lb-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .lb-title {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .lb-sub {
    font-size: 12.5px;
    color: var(--yap-muted);
    line-height: 1.45;
  }
  .lb-list {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .lb-card {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 13px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    transition:
      border-color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .lb-card.sel {
    border-color: var(--yap-border-hover);
    box-shadow: var(--yap-shadow-sm);
  }
  .lb-card.menuopen {
    border-color: var(--yap-border-hover);
  }
  .lb-card:focus {
    outline: none;
  }
  .lb-dot {
    width: 7px;
    height: 7px;
    flex: 0 0 auto;
    border-radius: var(--yap-r-full);
    background: var(--yap-border-hover);
  }
  .lb-dot.on {
    background: var(--yap-success);
  }
  .lb-families {
    margin-bottom: 2px;
  }
  .lb-brand {
    width: 18px;
    height: 18px;
    flex: 0 0 auto;
    object-fit: contain;
    border-radius: 4px;
  }
  .lb-brandfb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--yap-muted);
  }
  .lb-brandfb svg {
    width: 16px;
    height: 16px;
  }
  .lb-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .lb-top {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .lb-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .lb-size {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .lb-context {
    margin: 2px 0 10px;
  }
  .lb-badge {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--yap-link);
    background: var(--yap-primary-wash);
    padding: 1px 6px;
    border-radius: var(--yap-r-full);
  }
  .lb-learn {
    font-size: 12px;
    color: var(--yap-link);
    text-decoration: none;
  }
  .lb-learn:hover {
    text-decoration: underline;
  }
  .lb-blurb {
    font-size: 12.5px;
    color: var(--yap-muted);
    line-height: 1.4;
  }
  .lb-progress {
    margin-top: 4px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .lb-plabel {
    font-size: 11.5px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .lb-bar {
    height: 5px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .lb-bar span {
    display: block;
    height: 100%;
    background: var(--yap-ink);
    transition: width 0.2s ease;
  }
  .lb-action {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 0 0 auto;
  }
  .lb-dl,
  .lb-use {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 11px;
    border-radius: var(--yap-r);
    border: 1px solid var(--yap-border);
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      border-color var(--yap-dur) ease,
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  /* The same quiet Download as the speech models' (ModelRow). */
  .lb-dl svg {
    width: 12px;
    height: 12px;
  }
  .lb-dl:focus-visible,
  .lb-use:focus-visible,
  .lb-del:focus-visible,
  .lb-later:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .lb-dl:hover:not(:disabled),
  .lb-use:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .lb-dl:disabled,
  .lb-use:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .lb-del {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: transparent;
    color: var(--yap-muted);
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .lb-del svg {
    width: 13px;
    height: 13px;
  }
  .lb-del:hover {
    color: var(--yap-danger);
    border-color: color-mix(in srgb, var(--yap-danger) 45%, var(--yap-border));
  }
  .lb-activetag {
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-success-text);
  }
  .lb-busy {
    font-size: 12px;
    color: var(--yap-muted);
  }
  /* A quiet line under the heading: the bigger model offered once, or a
     big model left on the processor. */
  .lb-note {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-fg);
    background: var(--yap-raised-soft);
    border-radius: var(--yap-r-md, 8px);
    padding: 8px 10px;
  }
  .lb-note span {
    flex: 1 1 260px;
  }
  .lb-note.slow {
    background: var(--yap-raised);
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
  .lb-later:hover {
    color: var(--yap-fg);
  }
  .lb-byo {
    margin: 2px 0 0;
    font-size: 12.5px;
    color: var(--yap-muted);
    line-height: 1.45;
  }
  .lb-byo code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    background: var(--yap-raised);
    padding: 1px 5px;
    border-radius: var(--yap-r-sm);
  }
</style>
