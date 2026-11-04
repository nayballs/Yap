<script>
  // A big download under way, the same in every place one is shown
  // (core/downloads.js, src-tauri/src/shell/download.rs): what's downloading
  // and how far ("Qwen3.5 4B… 42%"), a bar, "1.2 GB of 2.8 GB · 24 MB/s ·
  // about 1 min left" once the speed is known, and Cancel — which stops it
  // and takes its half-downloaded file away.
  //
  // `progress`: `downloads.fromEvent(payload)`; `label`: what downloads;
  // `oncancel`: Cancel's click (none: no Cancel); `compact`: one line
  // (a model row's right side).
  import { downloadLine } from '../core/downloads.js';

  let { progress = null, label = 'Downloading', oncancel = null, compact = false } = $props();

  const percent = $derived(Math.max(0, Math.min(100, Math.round(progress?.percent ?? 0))));
  const line = $derived(downloadLine(progress));
  let cancelling = $state(false);

  function cancel(e) {
    e.stopPropagation();
    if (cancelling) return;
    cancelling = true;
    Promise.resolve(oncancel?.()).finally(() => (cancelling = false));
  }
</script>

<div class="dlp" class:compact data-testid="download-progress" role="group" aria-label={`${label}: ${percent}%`}>
  <div class="dlp-text">
    <span class="dlp-label">{label}… <span class="dlp-pct">{percent}%</span></span>
    {#if line}<span class="dlp-line" data-testid="download-line">{line}</span>{/if}
  </div>
  <div class="dlp-row">
    <div class="dlp-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent}>
      <span style="width:{percent}%"></span>
    </div>
    {#if oncancel}
      <button type="button" class="dlp-cancel" onclick={cancel} disabled={cancelling} aria-label={`Cancel the download: ${label}`}>
        {cancelling ? 'Cancelling…' : 'Cancel'}
      </button>
    {/if}
  </div>
</div>

<style>
  .dlp {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    width: 100%;
  }
  .dlp-text {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 8px;
    min-width: 0;
  }
  .dlp-label {
    font-size: 11.5px;
    color: var(--yap-fg-80);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .dlp-pct,
  .dlp-line {
    font-variant-numeric: tabular-nums;
  }
  .dlp-line {
    font-size: 11.5px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .dlp-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .dlp-bar {
    flex: 1 1 auto;
    height: 5px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
    min-width: 40px;
  }
  .dlp-bar span {
    display: block;
    height: 100%;
    background: var(--yap-ink);
    transition: width 0.2s ease;
  }
  .dlp-cancel {
    flex: 0 0 auto;
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .dlp-cancel:hover:not(:disabled) {
    background: var(--yap-s3);
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .dlp-cancel:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .dlp-cancel:disabled {
    opacity: 0.6;
    cursor: default;
  }
  /* A model row's right side: the line under a narrow bar. */
  .dlp.compact {
    width: 220px;
    max-width: 46vw;
  }
  .dlp.compact .dlp-label {
    display: none;
  }
  .dlp.compact .dlp-text {
    order: 2;
  }
  @media (prefers-reduced-motion: reduce) {
    .dlp-bar span {
      transition: none;
    }
  }
  :global([data-yap-motion='reduced']) .dlp-bar span {
    transition: none;
  }
</style>
