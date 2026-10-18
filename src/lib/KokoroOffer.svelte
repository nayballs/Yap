<script>
  // Turning on "Speak replies" while Kokoro isn't downloaded: the offer to
  // download it (Settings → Voice Agent). Speak replies stays on either way;
  // Windows voices speak until Kokoro is in.
  import Button from './ui/Button.svelte';
  import { kokoro, downloadKokoro, formatMb } from './kokoro.svelte.js';

  let { open = $bindable(false), cfg } = $props();

  const busy = $derived(kokoro.downloading);

  async function download() {
    if (await downloadKokoro()) {
      cfg.ttsEngine = 'kokoro';
      open = false;
    }
  }
</script>

{#if (open || busy) && kokoro.inBuild && !kokoro.installed}
  <div class="offer" role="group" aria-label="Download Kokoro">
    <p>
      For a natural voice, download Kokoro ({formatMb(kokoro.downloadBytes || kokoro.sizeBytes)}): it downloads once, then
      everything runs on this PC. Until then Yap speaks with Windows voices.
    </p>
    {#if busy}
      <div class="progress" role="progressbar" aria-label="Downloading Kokoro" aria-valuenow={busy.percent} aria-valuemin="0" aria-valuemax="100">
        <span style:width="{busy.percent}%"></span>
      </div>
      <span class="pct">Downloading… {busy.percent}%</span>
    {:else}
      <div class="actions">
        <Button size="sm" onclick={download}>Download Kokoro ({formatMb(kokoro.downloadBytes || kokoro.sizeBytes)})</Button>
        <Button size="sm" variant="ghost" onclick={() => (open = false)}>Not now</Button>
      </div>
    {/if}
    {#if kokoro.error}<p class="err" role="alert">{kokoro.error}</p>{/if}
  </div>
{/if}

<style>
  .offer {
    display: flex;
    flex-direction: column;
    gap: 9px;
    margin-top: 10px;
    padding: 11px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .offer p {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-fg);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .progress {
    height: 4px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    overflow: hidden;
  }
  .progress span {
    display: block;
    height: 100%;
    background: var(--yap-primary);
    transition: width var(--yap-dur) ease;
  }
  .pct {
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .err {
    color: var(--yap-danger) !important;
  }
</style>
