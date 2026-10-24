<script>
  // Settings → Speech-to-Text → "Speakers in meetings": the speaker
  // (diarization) models in the same rows as the speech models — Download /
  // Active / delete — with the "Tell speakers apart" switch and where the
  // files come from (diarize.rs, diarize/registry.rs).
  import { onMount } from 'svelte';
  import ModelRow from './ModelRow.svelte';
  import SpeakersToggle from './SpeakersToggle.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { openExternalLink } from '../core/externalLinks.js';
  import { diarization, startDiarization, downloadDiarization, deleteDiarization, isInstalled } from './diarization.svelte.js';

  let { enabled = $bindable(false), model = $bindable('') } = $props();

  onMount(startDiarization);

  // The registry's rows in ModelRow's shape.
  const rows = $derived(
    diarization.models.map((m) => ({
      ...m,
      sizeMb: Math.max(1, Math.round(m.sizeBytes / 1_000_000)),
      langLabel: m.available ? `Up to ${m.speakers} voices` : '',
    }))
  );
  const credited = $derived(diarization.models.find((m) => m.recommended) || null);

  function statusOf(m) {
    if (!m.available) return 'unavailable';
    if (diarization.downloading?.id === m.id) return 'downloading';
    if (m.installed) return m.id === model ? 'active' : 'available';
    return 'downloadable';
  }

  async function onRow(m) {
    if (m.installed) {
      model = m.id;
      return;
    }
    if (await downloadDiarization(m.id)) {
      // The first one in becomes the one used.
      if (!isInstalled(model)) model = m.id;
    }
  }

  // Delete asks first (Yap's own dialog: in a Tauri window `confirm()` is
  // the dialog plugin's, which this window isn't allowed).
  let ask = $state({ open: false, model: null, busy: false });
  function onDelete(m) {
    if (diarization.downloading) return;
    ask = { open: true, model: m, busy: false };
  }
  async function confirmDelete() {
    const m = ask.model;
    if (!m) return;
    ask.busy = true;
    const ok = await deleteDiarization(m.id);
    if (ok && m.id === model) enabled = false;
    ask = { open: false, model: null, busy: false };
  }

  function link(url) {
    return (e) => {
      e.preventDefault();
      openExternalLink(url);
    };
  }
</script>

<div class="speaker-models">
  <SpeakersToggle bind:enabled bind:model place="models" />

  <div class="rows" role="list" aria-label="Speaker models">
    {#each rows as m (m.id)}
      <div role="listitem">
        <ModelRow
          model={m}
          status={statusOf(m)}
          percent={diarization.downloading?.id === m.id ? diarization.downloading.percent : 0}
          onclick={onRow}
          ondelete={onDelete}
          deleteActive
        />
      </div>
    {/each}
  </div>

  {#if diarization.error}
    <p class="error" role="alert">{diarization.error}</p>
  {/if}

  {#if credited}
    <details class="set-more">
    <summary>Credits</summary>
    <p class="credit more-body">
      {credited.name} by {credited.vendor},
      <a href={credited.licenseUrl} onclick={link(credited.licenseUrl)}>{credited.license}</a>, on
      <a href="https://github.com/NVIDIA/NeMo-Speech.cpp" onclick={link('https://github.com/NVIDIA/NeMo-Speech.cpp')}>{credited.runtimeName}</a>
      ({credited.runtimeLicense}). Downloaded from NVIDIA on
      <a href={credited.sourceUrl} onclick={link(credited.sourceUrl)}>Hugging Face</a> and GitHub, checked against pinned
      checksums; their licences are saved with them.
    </p>
    </details>
  {/if}
</div>

<ConfirmDialog
  bind:open={ask.open}
  title={ask.model ? `Delete ${ask.model.name}?` : ''}
  body={ask.model?.id === model && enabled
    ? 'Tell speakers apart turns off. You can download it again later.'
    : 'You can download it again later.'}
  busy={ask.busy}
  onconfirm={confirmDelete}
/>

<style>
  .speaker-models {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .error {
    color: var(--yap-danger);
    font-size: 12px;
    margin: 0;
  }
  .credit {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .credit a {
    color: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .credit a:hover {
    color: var(--yap-fg);
  }
</style>
