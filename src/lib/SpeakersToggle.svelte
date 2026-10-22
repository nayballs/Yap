<script>
  // "Tell speakers apart in meetings" (beta; diarize.rs). One switch, shown in
  // Settings → Speech-to-Text under the speaker models and again in Settings
  // → Meetings → While a meeting records, both bound to the same config fields (meetingDiarization,
  // diarizationModel). It stays off until a speaker model is installed:
  // switching it on without one offers the download instead.
  import { onMount } from 'svelte';
  import Toggle from './ui/Toggle.svelte';
  import Button from './ui/Button.svelte';
  import {
    diarization,
    startDiarization,
    isInstalled,
    offeredModel,
    downloadDiarization,
    formatBytes,
  } from './diarization.svelte.js';

  let {
    enabled = $bindable(false),
    model = $bindable(''),
    // 'models' (under the model rows) | 'meetings' (the Meetings page)
    place = 'models',
    onopenmodels = null,
  } = $props();

  onMount(startDiarization);

  const ready = $derived(isInstalled(model));
  const offer = $derived(offeredModel(model));
  const busy = $derived(offer && diarization.downloading?.id === offer.id ? diarization.downloading : null);
  let asking = $state(false);

  function set(on) {
    if (on && !ready) {
      asking = true;
      return;
    }
    asking = false;
    enabled = on;
  }

  async function downloadAndTurnOn() {
    if (!offer) return;
    if (await downloadDiarization(offer.id)) {
      model = offer.id;
      enabled = true;
      asking = false;
    }
  }

  const desc = $derived.by(() => {
    const what = 'Speaker 1, 2, 3… for each voice on the call. It all runs on this PC.';
    if (ready || !diarization.loaded) return what;
    return place === 'meetings'
      ? `${what} Needs the speaker model${offer ? ` (${formatBytes(offer.downloadBytes)})` : ''}.`
      : `${what} Needs a speaker model below.`;
  });
</script>

<div class="speakers">
  <Toggle
    bind:checked={() => enabled && ready, set}
    label="Tell speakers apart in meetings"
    badge="Beta"
    {desc}
    hint="NVIDIA's Nemotron-3 Diarization tells up to 8 voices apart, on your CPU (about a tenth of one core while a meeting records). Still in beta: it's been measured on test meetings, not yet on real calls. Voices that sound alike can come out as one, and calls where everyone dials in by phone aren't supported yet."
  />

  {#if (asking || busy) && !ready && offer}
    <div class="offer" role="group" aria-label="Download the speaker model">
      <p>
        Telling speakers apart needs {offer.vendor}'s {offer.name} ({formatBytes(offer.downloadBytes)}). It downloads
        once; then everything runs on this PC.
      </p>
      {#if busy}
        <div class="progress" role="progressbar" aria-label="Downloading the speaker model" aria-valuenow={busy.percent} aria-valuemin="0" aria-valuemax="100">
          <span style:width="{busy.percent}%"></span>
        </div>
        <span class="pct">Downloading… {busy.percent}%</span>
      {:else}
        <div class="actions">
          <Button size="sm" onclick={downloadAndTurnOn}>Download and turn on</Button>
          <Button size="sm" variant="ghost" onclick={() => (asking = false)}>Not now</Button>
        </div>
      {/if}
      <!-- Under the model rows, the section says it itself. -->
      {#if diarization.error && place === 'meetings'}<p class="err" role="alert">{diarization.error}</p>{/if}
    </div>
  {/if}

  {#if place === 'meetings' && onopenmodels}
    <button type="button" class="link" onclick={onopenmodels}>Speaker models in Speech-to-Text</button>
  {/if}
</div>

<style>
  .speakers {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .offer {
    display: flex;
    flex-direction: column;
    gap: 9px;
    padding: 11px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .offer p {
    margin: 0;
    font-size: 12.5px;
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
    background: var(--yap-ink);
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
  .link {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link:hover {
    text-decoration-color: currentColor;
  }
  .link:focus-visible {
    outline: none;
    border-radius: 2px;
    box-shadow: var(--yap-focus);
  }
</style>
