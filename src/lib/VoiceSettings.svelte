<script>
  // Settings → Voice Assistant → Voice: who speaks (ttsProvider: "On this
  // PC", the default, or a cloud voice — ElevenLabs, ElevenLabsSettings.svelte
  // — with the privacy line under the choice while it's picked); on this PC,
  // the engine (Kokoro, the natural voices on this PC, recommended; or
  // Windows' own voices), Kokoro's download row (Download / Active / delete,
  // like the speech models'), the voice picker that follows the engine
  // (Kokoro's 54 voices grouped by accent and gender), ▶ Preview, and where
  // Kokoro's files come from; for both, the speed and the volume (tts.rs,
  // tts/kokoro.rs, tts/registry.rs, tts/elevenlabs.rs).
  //
  //   cfg: Settings' config $state (ttsProvider, ttsCloudKeys, ttsCloud,
  //        ttsEngine, ttsVoice — Kokoro's —, ttsWindowsVoice, ttsRate,
  //        ttsVolume). Auto-save does the saving.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Row from './ui/Row.svelte';
  import Select from './ui/Select.svelte';
  import Slider from './ui/Slider.svelte';
  import Segmented from './ui/Segmented.svelte';
  import ModelRow from './ModelRow.svelte';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import ElevenLabsSettings from './ElevenLabsSettings.svelte';
  import { speech } from './speech.svelte.js';
  import { toast } from './ui/toast.svelte.js';
  import { openExternalLink } from './core/externalLinks.js';
  import { kokoro, startKokoro, downloadKokoro, deleteKokoro, formatMb, kokoroOptions } from './kokoro.svelte.js';

  let { cfg } = $props();

  let voiceList = $state(null); // tts::VoiceList
  async function loadVoices() {
    try {
      voiceList = await invoke('tts_voices');
    } catch (e) {
      voiceList = { kokoro: { voices: [] }, windows: { voices: [], error: String(e) }, active: 'windows' };
    }
  }
  onMount(() => {
    startKokoro();
    loadVoices();
    const un = listen('tts-kokoro-changed', loadVoices);
    return () => un.then((f) => f());
  });

  // Who speaks: the providers tts_voices lists (cloud/PROVIDERS), these
  // until it answers.
  const PROVIDERS = [
    { value: 'local', label: 'On this PC' },
    { value: 'elevenlabs', label: 'ElevenLabs' },
  ];
  const providers = $derived(
    voiceList?.providers?.length ? voiceList.providers.map((p) => ({ value: p.id, label: p.name })) : PROVIDERS
  );
  const provider = $derived(providers.some((p) => p.value === cfg.ttsProvider) ? cfg.ttsProvider : 'local');
  const cloud = $derived(provider !== 'local');
  const keysPage = $derived(voiceList?.providers?.find((p) => p.id === provider)?.keysPage || undefined);
  function setProvider(v) {
    cfg.ttsProvider = v;
    if (speech.speakingId === 'preview') invoke('tts_stop').catch(() => {});
  }
  const providerDesc = $derived(
    cloud ? "ElevenLabs' voices, on your own API key" : "Yap's own voice, made on this PC: nothing leaves it"
  );
  // A local voice that can stand in when the cloud voice fails: Kokoro
  // downloaded, or Windows voices chosen (tts.rs local_voice).
  const localFallback = $derived(cfg.ttsEngine === 'windows' || (kokoro.inBuild && kokoro.installed));

  const ENGINES = [
    { value: 'kokoro', label: 'Kokoro' },
    { value: 'windows', label: 'Windows voices' },
  ];
  const engine = $derived(cfg.ttsEngine === 'windows' ? 'windows' : 'kokoro');
  const kokoroReady = $derived(kokoro.inBuild && kokoro.installed);
  const busy = $derived(kokoro.downloading);

  const engineDesc = $derived(
    engine === 'kokoro'
      ? 'Natural voices, made on this PC. Recommended'
      : "Windows' own voices: no download, but more robotic"
  );

  // The voice picker follows the engine.
  const options = $derived.by(() => {
    if (!voiceList) return [];
    if (engine === 'kokoro') return kokoroOptions(voiceList.kokoro?.voices || [], voiceList.kokoro?.defaultVoice || 'af_bella');
    const voices = voiceList.windows?.voices || [];
    const def = voices.find((v) => v.id === voiceList.windows?.defaultVoice);
    const opts = [
      { value: '', label: def ? `Windows default (${shortVoice(def)})` : 'Windows default' },
      ...voices.map((v) => ({ value: v.id, label: `${shortVoice(v)} · ${v.language}` })),
    ];
    // A voice saved here that's gone from Windows still shows as itself.
    if (cfg.ttsWindowsVoice && !voices.some((v) => v.id === cfg.ttsWindowsVoice)) {
      opts.push({ value: cfg.ttsWindowsVoice, label: 'A voice that is no longer installed' });
    }
    return opts;
  });
  function shortVoice(v) {
    return (v?.name || '').replace(/^Microsoft\s+/, '') || v?.id || '';
  }
  const voiceDesc = $derived.by(() => {
    if (engine === 'kokoro') {
      if (!kokoro.inBuild) return 'Kokoro is in the full Yap build. This one speaks with Windows voices';
      if (!kokoro.installed) return "Pick one now: you'll hear it once Kokoro is downloaded";
      return '54 voices in British and American English and seven other languages';
    }
    return voiceList?.windows?.error || 'The voices Windows has installed for apps';
  });

  const previewing = $derived(speech.speakingId === 'preview');
  const canPreview = $derived(!!voiceList && (engine === 'kokoro' ? kokoroReady : !voiceList.windows?.error));
  async function preview() {
    if (previewing) {
      invoke('tts_stop').catch(() => {});
      return;
    }
    const voice = engine === 'kokoro' ? cfg.ttsVoice || 'af_bella' : cfg.ttsWindowsVoice || '';
    try {
      await invoke('tts_preview', { voice, rate: Number(cfg.ttsRate) || 1, engine });
    } catch (e) {
      toast({ title: "Couldn't play the preview", description: String(e), variant: 'destructive' });
    }
  }

  // Kokoro's row, in the speech models' shape.
  const row = $derived({
    id: 'kokoro',
    name: 'Kokoro-82M',
    engine: 'kokoro',
    desc: 'Natural voices on this PC',
    sizeMb: Math.max(1, Math.round((kokoro.sizeBytes || 379_000_000) / 1_000_000)),
    langLabel: '54 voices',
    recommended: true,
    note: 'In the full Yap build',
  });
  const rowStatus = $derived(
    !kokoro.inBuild
      ? 'unavailable'
      : busy
        ? 'downloading'
        : kokoro.installed
          ? engine === 'kokoro'
            ? 'active'
            : 'available'
          : 'downloadable'
  );
  async function onRow() {
    if (kokoro.installed) {
      cfg.ttsEngine = 'kokoro';
      return;
    }
    if (await downloadKokoro()) cfg.ttsEngine = 'kokoro';
  }

  // Delete asks first, in Yap's own dialog.
  let ask = $state({ open: false, busy: false });
  async function confirmDelete() {
    ask.busy = true;
    await deleteKokoro();
    ask = { open: false, busy: false };
  }

  function link(url) {
    return (e) => {
      e.preventDefault();
      openExternalLink(url);
    };
  }
</script>

<div id="settings-voice-provider">
  <Row label="Speaks with" desc={providerDesc} setting="voice-provider">
    <Segmented bind:value={() => provider, setProvider} options={providers} label="Speaks with" />
  </Row>
</div>

{#if cloud}
  <!-- Where the words go, said plainly while a cloud voice is picked. -->
  <Row>
    <p class="privacy">The words Yap speaks are sent to ElevenLabs.</p>
  </Row>
  <ElevenLabsSettings {cfg} {keysPage} {localFallback} />
{:else}
<Row label="Engine" desc={engineDesc}>
  <Segmented bind:value={cfg.ttsEngine} options={ENGINES} label="Engine" />
</Row>

{#if engine === 'kokoro'}
  <Row>
    {#snippet children()}
      <div class="kokoro" role="group" aria-label="Kokoro">
        <ModelRow model={row} status={rowStatus} percent={busy?.percent ?? 0} onclick={onRow} ondelete={() => (ask = { open: true, busy: false })} deleteActive />
        {#if busy}
          <div class="progress" role="progressbar" aria-label="Downloading Kokoro" aria-valuenow={busy.percent} aria-valuemin="0" aria-valuemax="100">
            <span style:width="{busy.percent}%"></span>
          </div>
          <span class="note">Downloading… {busy.percent}%. Windows voices speak until it's done</span>
        {:else if !kokoro.inBuild}
          <span class="note">This build of Yap has no Kokoro (it's in the installed Yap): it speaks with Windows voices</span>
        {:else if !kokoro.installed}
          <span class="note">Downloads once ({formatMb(kokoro.downloadBytes || kokoro.sizeBytes)}), then it all runs on this PC. Until then, Windows voices speak</span>
        {:else if !kokoro.espeak}
          <span class="note">
            eSpeak NG, which sounds out names the dictionary doesn't have, isn't installed: Kokoro spells them out.
            <button type="button" class="retry" onclick={downloadKokoro}>Try again</button>
          </span>
        {/if}
        {#if kokoro.error}<p class="err" role="alert">{kokoro.error}</p>{/if}
      </div>
    {/snippet}
  </Row>
{/if}

<Row label="Voice" desc={voiceDesc} setting="voice">
  <div class="voicepick">
    {#if voiceList}
      {#if engine === 'kokoro'}
        <Select bind:value={cfg.ttsVoice} {options} ariaLabel="Voice" />
      {:else}
        <Select bind:value={cfg.ttsWindowsVoice} {options} ariaLabel="Voice" disabled={!!voiceList.windows?.error} />
      {/if}
    {:else}
      <span class="enginename">Reading the voices…</span>
    {/if}
    <button
      class="previewbtn"
      class:on={previewing}
      aria-label={previewing ? 'Stop the preview' : 'Preview the voice'}
      title={previewing ? 'Stop' : canPreview ? 'Preview' : 'Download Kokoro to hear its voices'}
      disabled={!canPreview && !previewing}
      onclick={preview}
    >
      {#if previewing}
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
        Stop
      {:else}
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
        Preview
      {/if}
    </button>
  </div>
</Row>
{/if}

<Row
  label="Speed"
  desc={cloud && (Number(cfg.ttsRate) < 0.7 || Number(cfg.ttsRate) > 1.2) ? 'ElevenLabs speaks between 0.7× and 1.2×: it uses the nearest' : ''}
  setting="voice-speed"
>
  <Slider inline bind:value={cfg.ttsRate} min={0.5} max={2} step={0.1} label="Speed" format={(v) => `${Number(v).toFixed(1)}×`} />
</Row>

<!-- Yap's own volume for the voice (tts.rs `gain`), under the Windows
     mixer's: moving it changes what's being said at once; auto-save keeps it. -->
<Row label="Volume" setting="voice-volume">
  <Slider
    inline
    bind:value={cfg.ttsVolume}
    min={0}
    max={1}
    step={0.05}
    label="Volume"
    format={(v) => `${Math.round(Number(v) * 100)}%`}
    oninput={(v) => invoke('tts_volume_set', { volume: Number(v) }).catch(() => {})}
  />
</Row>

{#if !cloud && engine === 'kokoro'}
  <Row>
    <details class="set-more">
    <summary>Credits</summary>
    <p class="credit more-body">
      <a href="https://huggingface.co/hexgrad/Kokoro-82M" onclick={link('https://huggingface.co/hexgrad/Kokoro-82M')}>Kokoro-82M</a>
      by hexgrad and its voices, Apache-2.0, downloaded from onnx-community on
      <a href="https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX" onclick={link('https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX')}>Hugging Face</a>;
      pronunciations from <a href="https://github.com/hexgrad/misaki" onclick={link('https://github.com/hexgrad/misaki')}>misaki</a>
      (Apache-2.0). Words it doesn't know, mostly names, are sounded out by
      <a href="https://github.com/espeak-ng/espeak-ng" onclick={link('https://github.com/espeak-ng/espeak-ng')}>eSpeak NG</a>
      (GPL-3.0), downloaded with it and run as a separate program. All checked against pinned checksums; their
      licences and the source link are saved with them.
    </p>
    </details>
  </Row>
{/if}

<ConfirmDialog
  bind:open={ask.open}
  title="Delete Kokoro?"
  body="Spoken replies switch to Windows voices. You can download it again later."
  busy={ask.busy}
  onconfirm={confirmDelete}
/>

<style>
  .kokoro {
    display: flex;
    flex-direction: column;
    gap: 7px;
    width: 100%;
  }
  .note {
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
  .privacy {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-fg-80, var(--yap-fg));
  }
  .err {
    margin: 0;
    font-size: 12px;
    color: var(--yap-danger);
  }
  .retry {
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-weight: 600;
    color: var(--yap-fg);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
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
  .voicepick {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .voicepick :global(select) {
    max-width: 260px;
  }
  .previewbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .previewbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .previewbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .previewbtn.on {
    color: var(--yap-fg);
    border-color: var(--yap-fg-62);
  }
  .previewbtn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .previewbtn svg {
    width: 11px;
    height: 11px;
  }
  .enginename {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-80, var(--yap-fg));
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
