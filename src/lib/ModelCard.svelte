<script>
  import { formatSize } from './models.js';
  import { ENGINE_PROVIDER, PROVIDER_ICONS } from './providerIcons.js';

  // status: downloadable | downloading | available | active | switching
  let {
    model,
    status = 'downloadable',
    percent = 0,
    onclick,
    ondelete = null,
  } = $props();

  const installed = $derived(status === 'available' || status === 'active');
  const busy = $derived(status === 'downloading' || status === 'switching');

  // Brand icon (same mapping the Settings model list uses): NVIDIA for
  // Parakeet/Canary, OpenAI for Whisper; engines without a truthful brand
  // icon fall back to a generic waveform glyph.
  const prov = $derived(ENGINE_PROVIDER[model.engine]);
  const icon = $derived(prov ? PROVIDER_ICONS[prov] : null);
</script>

<div class="card {status}">
  <button
    class="hit"
    onclick={() => onclick?.(model)}
    disabled={busy || status === 'active'}
    title={status === 'active'
      ? 'Active model'
      : installed
        ? 'Switch to this model'
        : 'Download this model'}
  >
    <div class="top">
      <div class="info">
        <div class="titleline">
          {#if icon}
            <img class="micon" src={icon} alt="" aria-hidden="true" />
          {:else}
            <svg class="micon gen" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12h2l2-6 3 12 3-9 2 3h6" /></svg>
          {/if}
          <h3>{model.name}</h3>
          {#if model.recommended && status !== 'active'}
            <span class="badge">Recommended</span>
          {/if}
          {#if status === 'active'}
            <span class="badge">✓ Active</span>
          {:else if status === 'switching'}
            <span class="badge ghost">Switching…</span>
          {/if}
        </div>
        <p class="desc">{model.desc}</p>
      </div>

      <div class="scores">
        <div class="score">
          <span>Accuracy</span>
          <div class="bar"><div class="fill" style="width:{model.accuracy * 100}%"></div></div>
        </div>
        <div class="score">
          <span>Speed</span>
          <div class="bar"><div class="fill" style="width:{model.speed * 100}%"></div></div>
        </div>
      </div>
    </div>

    <hr />

    <div class="meta">
      {#if model.engine}
        <span class="tag engine">{model.engine}</span>
      {/if}
      <span class="tag">🌐 {model.langLabel ?? (model.multilang ? 'Multi-language' : 'English only')}</span>
      {#if model.translate}
        <span class="tag">🔤 Translate</span>
      {/if}
      <span class="size">
        {installed ? '💾' : '⬇'}
        {formatSize(model.sizeMb)}
      </span>
    </div>

    {#if status === 'downloading'}
      <div class="progress">
        <div class="track"><div class="value" style="width:{percent}%"></div></div>
        <span class="pct">Downloading… {percent}%</span>
      </div>
    {/if}
  </button>

  {#if ondelete && status === 'available'}
    <button
      class="del"
      title="Delete this model"
      aria-label="Delete model"
      onclick={() => ondelete(model)}
    >🗑</button>
  {/if}
</div>

<!-- layout note: the card is a flex row so the optional delete button sits
     as its own column to the right, never overlapping the score bars. -->

<style>
  .card {
    display: flex;
    align-items: stretch;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    transition:
      border-color 0.18s ease,
      background 0.18s ease,
      box-shadow 0.18s ease;
  }
  .card:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
    box-shadow: 0 8px 22px rgba(60, 50, 30, 0.1);
  }
  .card.active {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
  }

  .hit {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    text-align: left;
    background: none;
    border: none;
    border-radius: 12px;
    padding: 13px 16px;
    cursor: pointer;
    color: inherit;
    font: inherit;
  }
  .hit:disabled {
    cursor: default;
  }
  .card.active .hit:disabled {
    opacity: 1;
  }

  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
  }
  .info {
    min-width: 0;
  }
  .titleline {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  h3 {
    font-size: 15px;
    font-weight: 600;
    margin: 0;
  }
  .desc {
    color: var(--yap-muted);
    font-size: 12.5px;
    line-height: 1.45;
    margin: 3px 0 0;
  }

  .badge {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-fg);
    background: var(--yap-primary-tint);
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-full);
    padding: 2px 8px;
    white-space: nowrap;
  }
  .badge.ghost {
    color: var(--yap-muted);
    background: var(--yap-raised-soft);
    border-color: var(--yap-border-subtle);
  }

  .micon {
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
    object-fit: contain;
  }
  .micon.gen {
    color: var(--yap-muted);
  }

  .scores {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 0 0 auto;
  }
  .score {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }
  .score span {
    font-size: 11px;
    color: var(--yap-muted);
    width: 56px;
    text-align: right;
  }
  .bar {
    width: 70px;
    height: 6px;
    background: var(--yap-raised);
    border-radius: var(--yap-r-full);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--yap-primary), var(--yap-primary-hover));
    border-radius: var(--yap-r-full);
  }

  hr {
    width: 100%;
    border: none;
    border-top: 1px solid var(--yap-border-subtle);
    margin: 0;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 14px;
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .tag.engine {
    color: var(--yap-fg-80);
    background: var(--yap-primary-wash);
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-full);
    padding: 1px 8px;
    font-size: 10.5px;
    font-weight: 600;
  }
  .size {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .progress {
    margin-top: 2px;
  }
  .track {
    width: 100%;
    height: 6px;
    background: var(--yap-raised);
    border-radius: var(--yap-r-full);
    overflow: hidden;
  }
  .value {
    height: 100%;
    background: linear-gradient(90deg, var(--yap-primary), var(--yap-primary-hover));
    border-radius: var(--yap-r-full);
    transition: width 0.25s ease;
  }
  .pct {
    display: block;
    font-size: 11px;
    color: var(--yap-muted);
    margin-top: 4px;
  }

  .del {
    flex: 0 0 auto;
    width: 42px;
    border: none;
    border-left: 1px solid var(--yap-border-subtle);
    border-radius: 0 var(--yap-r-lg) var(--yap-r-lg) 0;
    background: transparent;
    color: var(--yap-fg-45);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    transition:
      color 0.15s ease,
      background 0.15s ease;
  }
  .del:hover {
    color: var(--yap-danger);
    background: rgba(224, 86, 79, 0.12);
  }
</style>
