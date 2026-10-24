<script>
  // A compact one-line model row, ported from OpenWhispr's LocalModelCard /
  // ModelCardList: status dot → brand icon → name → size → language → badges,
  // with Download / Active / progress on the right and a hover-reveal delete.
  import { formatSize } from '../core/speechModels.js';
  import { ENGINE_PROVIDER, PROVIDER_ICONS, MONOCHROME_PROVIDERS } from '../core/providerIcons.js';
  import { openContextMenu, menuKey, menuOpenFor } from '../ui/contextMenu.svelte.js';

  // status: downloadable | downloading | available | active | switching |
  // unavailable (listed, can't be installed yet: `model.note` says why).
  // `deleteActive`: the active model can be deleted too (the speaker models,
  // whose switch turns off with it; a speech model must be switched first).
  let { model, status = 'downloadable', percent = 0, onclick, ondelete = null, deleteActive = false } = $props();

  const prov = $derived(ENGINE_PROVIDER[model.engine]);
  const icon = $derived(prov ? PROVIDER_ICONS[prov] : null);
  // Row click switches to an installed model; the Download button handles the rest.
  const rowClickable = $derived(status === 'available');
  const canDelete = $derived(!!ondelete && (status === 'available' || (deleteActive && status === 'active')));

  // Its right-click menu (ui/contextMenu.svelte.js, as everywhere): what its
  // buttons do — Download, or Use (the row's click) — and Delete…, which
  // asks first (the parent's dialog). Nothing while it downloads or switches.
  let rowEl = $state(null);
  function rowMenu(e) {
    openContextMenu(
      e,
      [
        status === 'downloadable' ? { label: 'Download', icon: 'download', onselect: () => onclick?.(model) } : null,
        status === 'available' ? { label: 'Use', icon: 'use', onselect: () => onclick?.(model) } : null,
        canDelete ? { separator: true } : null,
        canDelete ? { label: 'Delete…', icon: 'trash', danger: true, onselect: () => ondelete(model) } : null,
      ],
      { label: model.name, row: rowEl }
    );
  }
</script>

<div
  bind:this={rowEl}
  class="mrow {status}"
  class:clickable={rowClickable}
  class:menuopen={menuOpenFor(rowEl)}
  role="button"
  tabindex="0"
  title={model.desc}
  onclick={() => rowClickable && onclick?.(model)}
  oncontextmenu={rowMenu}
  onkeydown={(e) => {
    if (e.key === 'Enter' && rowClickable) onclick?.(model);
    else if (e.target === e.currentTarget) menuKey(e, rowMenu);
  }}
>
  <span class="dot" aria-hidden="true"></span>

  {#if icon}
    <img class="micon" class:mono={MONOCHROME_PROVIDERS.has(prov)} src={icon} alt="" aria-hidden="true" />
  {:else}
    <svg class="micon gen" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
    </svg>
  {/if}

  <span class="mname">{model.name}</span>
  <span class="mmeta">{formatSize(model.sizeMb)}</span>
  <span class="mmeta mlang">{model.langLabel}</span>
  {#if model.recommended && status !== 'active'}
    <span class="mtag">Recommended</span>
  {/if}

  <span class="spacer"></span>

  {#if status === 'active'}
    <span class="mtag active">Active</span>
  {:else if status === 'switching'}
    <span class="mpct">Switching…</span>
  {:else if status === 'downloading'}
    <span class="mpct">{percent}%</span>
  {:else if status === 'unavailable'}
    <span class="msoon">{model.note || 'Coming later'}</span>
  {:else if status === 'available'}
    <button class="dl" onclick={(e) => { e.stopPropagation(); onclick?.(model); }}>Use</button>
  {:else if status === 'downloadable'}
    <button class="dl" onclick={(e) => { e.stopPropagation(); onclick?.(model); }}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M12 4v11M6 11l6 5 6-5" /><path d="M4 20h16" />
      </svg>
      Download
    </button>
  {/if}

  {#if canDelete}
    <button
      class="trash"
      title="Delete this model"
      aria-label="Delete model"
      onclick={(e) => { e.stopPropagation(); ondelete(model); }}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
      </svg>
    </button>
  {/if}
</div>

<style>
  .mrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    box-sizing: border-box;
    padding: 6px 8px 6px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .mrow.clickable {
    cursor: pointer;
  }
  .mrow.clickable:hover {
    background: var(--yap-s3);
    border-color: var(--yap-border-hover);
  }
  .mrow.active {
    border-color: var(--yap-border-hover);
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
  }
  .mrow.active .dot {
    background: var(--yap-success);
    box-shadow: 0 0 4px color-mix(in srgb, var(--yap-success) 50%, transparent);
  }
  .mrow.available .dot {
    background: var(--yap-switch-edge);
  }
  .mrow.downloading .dot,
  .mrow.switching .dot {
    background: var(--yap-warning);
    box-shadow: 0 0 4px rgba(220, 160, 60, 0.5);
    animation: pulse-glow 1s ease-in-out infinite;
  }

  .micon {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
  }
  /* Monochrome (black) brand logos read as-is on the light theme and turn
     white on the dark one. */
  .micon.mono {
    filter: none;
  }
  :global([data-yap-theme='dark']) .micon.mono {
    filter: invert(1);
  }
  .micon.gen {
    color: var(--yap-muted);
  }

  .mname {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--yap-fg);
    white-space: nowrap;
  }
  .mmeta {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .mlang {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    flex: 0 1 auto;
  }
  .mtag {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-link);
    background: var(--yap-primary-wash);
    padding: 1px 7px;
    border-radius: var(--yap-r-sm);
    letter-spacing: 0.02em;
    white-space: nowrap;
    flex: 0 0 auto;
  }
  .mtag.active {
    color: var(--yap-success-text);
    background: color-mix(in srgb, var(--yap-success) 12%, var(--yap-s2));
  }
  .mpct {
    font-size: 12px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .mrow.unavailable .mname,
  .mrow.unavailable .micon {
    opacity: 0.6;
  }
  .msoon {
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .spacer {
    flex: 1 1 auto;
  }

  /* One Download (and Use) for every model list: speech models, Kokoro,
     the speaker models, the local language models. Quiet: ink is for the
     one recommended action on a page. */
  .dl {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 11px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .dl:hover {
    background: var(--yap-s3);
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .dl:focus-visible,
  .trash:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .dl svg {
    width: 12px;
    height: 12px;
  }

  .trash {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm);
    background: transparent;
    color: var(--yap-fg-62);
    opacity: 0;
    cursor: pointer;
    transition:
      opacity var(--yap-dur) ease,
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .mrow:hover .trash,
  .mrow.menuopen .trash,
  .trash:focus-visible {
    opacity: 1;
  }
  .mrow.menuopen {
    border-color: var(--yap-border-hover);
  }
  .mrow:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .trash:hover {
    color: var(--yap-danger);
    background: color-mix(in srgb, var(--yap-danger) 10%, transparent);
  }
  .trash svg {
    width: 13px;
    height: 13px;
  }

  @keyframes pulse-glow {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }
</style>
