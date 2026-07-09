<script>
  // A compact one-line model row, ported from OpenWhispr's LocalModelCard /
  // ModelCardList: status dot → brand icon → name → size → language → badges,
  // with Download / Active / progress on the right and a hover-reveal delete.
  import { formatSize } from './models.js';
  import { ENGINE_PROVIDER, PROVIDER_ICONS, MONOCHROME_PROVIDERS } from './providerIcons.js';

  // status: downloadable | downloading | available | active | switching
  let { model, status = 'downloadable', percent = 0, onclick, ondelete = null } = $props();

  const busy = $derived(status === 'downloading' || status === 'switching');
  const prov = $derived(ENGINE_PROVIDER[model.engine]);
  const icon = $derived(prov ? PROVIDER_ICONS[prov] : null);
  // Row click switches to an installed model; the Download button handles the rest.
  const rowClickable = $derived(status === 'available');
</script>

<div
  class="mrow {status}"
  class:clickable={rowClickable}
  role="button"
  tabindex="0"
  title={model.desc}
  onclick={() => rowClickable && onclick?.(model)}
  onkeydown={(e) => e.key === 'Enter' && rowClickable && onclick?.(model)}
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
    <span class="mtag">Active</span>
  {:else if status === 'switching'}
    <span class="mpct">Switching…</span>
  {:else if status === 'downloading'}
    <span class="mpct">{percent}%</span>
  {:else if status === 'downloadable'}
    <button class="dl" onclick={(e) => { e.stopPropagation(); onclick?.(model); }}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M12 4v11M6 11l6 5 6-5" /><path d="M4 20h16" />
      </svg>
      Download
    </button>
  {/if}

  {#if ondelete && status === 'available'}
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
    gap: 7px;
    padding: 8px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .mrow.clickable {
    cursor: pointer;
  }
  .mrow.clickable:hover {
    background: var(--yap-s2);
    border-color: var(--yap-border-hover);
  }
  .mrow.active {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
  }
  .mrow.active .dot {
    background: var(--yap-primary);
    box-shadow: 0 0 6px var(--yap-primary-tint);
    animation: pulse-glow 2s ease-in-out infinite;
  }
  .mrow.available .dot {
    background: var(--yap-success);
    box-shadow: 0 0 4px color-mix(in srgb, var(--yap-success) 50%, transparent);
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
  /* Monochrome (black) brand logos read as-is on the light theme; only the
     dark-scoped onboarding window still needs the white inversion. */
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
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--yap-fg);
    white-space: nowrap;
  }
  .mmeta {
    font-size: 11px;
    color: var(--yap-fg-45);
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
    color: var(--yap-primary);
    background: var(--yap-primary-tint);
    padding: 1px 7px;
    border-radius: var(--yap-r-sm);
    letter-spacing: 0.02em;
    white-space: nowrap;
    flex: 0 0 auto;
  }
  .mrow.active .mtag {
    color: #cfc9ff;
  }
  .mpct {
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .dl {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .dl:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .dl svg {
    width: 11px;
    height: 11px;
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
    color: var(--yap-fg-45);
    opacity: 0;
    cursor: pointer;
    transition:
      opacity var(--yap-dur) ease,
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .mrow:hover .trash,
  .trash:focus-visible {
    opacity: 1;
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
