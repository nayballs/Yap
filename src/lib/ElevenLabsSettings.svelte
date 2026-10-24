<script>
  // Settings → Voice Assistant → Voice, "ElevenLabs" picked (tts.rs,
  // tts/elevenlabs.rs, tts/cloud.rs):
  // - the API key, kept like Yap's other cloud keys (cfg.ttsCloudKeys, as
  //   cfg.webSearchKeys), masked, with "Get your API key" (their API-keys
  //   page, through the opener). Until it's set, Yap's own voice speaks.
  // - the voices of the account — its own first, then ElevenLabs' default
  //   ones — each with ▶: the voice's own sample, fetched only when clicked
  //   and played like an answer (Yap's volume, the output device);
  // - the model (the lowest-latency one recommended), the month's characters
  //   when the key may read them, and what happens when ElevenLabs doesn't
  //   answer (Yap's own voice steps in, if there's one).
  // The lists are read with the key as typed (not yet saved), a moment after
  // typing stops.
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  //   keysPage: where keys are made (tts_voices' provider list).
  //   localFallback: a local voice can stand in (Kokoro downloaded, or
  //     Windows voices chosen).
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack } from 'svelte';
  import Row from './ui/Row.svelte';
  import Select from './ui/Select.svelte';
  import Input from './ui/Input.svelte';
  import { speech } from './speech.svelte.js';
  import { toast } from './ui/toast.svelte.js';
  import { createExternalLinkHandler } from './externalLinks.js';
  import { maskKey } from './providers.js';

  let { cfg, keysPage = 'https://elevenlabs.io/app/settings/api-keys', localFallback = false } = $props();

  const ID = 'elevenlabs';
  const DEFAULT_VOICE = 'JBFqnCBsd6RMkjVDRZzb';
  const DEFAULT_MODEL = 'eleven_flash_v2_5';

  const key = $derived((cfg.ttsCloudKeys ?? {})[ID] ?? '');
  const choice = $derived((cfg.ttsCloud ?? {})[ID] ?? {});
  const voiceId = $derived(choice.voice || DEFAULT_VOICE);
  const model = $derived(choice.model || DEFAULT_MODEL);
  const masked = $derived(maskKey(key));
  let editing = $state(false);

  function setKey(v) {
    cfg.ttsCloudKeys = { ...(cfg.ttsCloudKeys ?? {}), [ID]: (v ?? '').trim() };
  }
  function setChoice(patch) {
    cfg.ttsCloud = { ...(cfg.ttsCloud ?? {}), [ID]: { voice: '', voiceName: '', model: '', ...choice, ...patch } };
  }

  // ---- the account's voices, models and usage
  let voices = $state(null); // null: not read (no key, or it failed)
  let models = $state([]);
  let usage = $state(null);
  let error = $state('');
  let loading = $state(false);
  let seq = 0;
  let timer = null;

  async function load() {
    const k = key;
    const mine = ++seq;
    if (!k) {
      voices = null;
      models = [];
      usage = null;
      error = '';
      loading = false;
      return;
    }
    loading = true;
    error = '';
    const args = { provider: ID, apiKey: k };
    const [v, m, u] = await Promise.all([
      invoke('tts_cloud_voices', args).then(
        (list) => ({ list }),
        (e) => ({ error: String(e) })
      ),
      invoke('tts_cloud_models', args).catch(() => []),
      invoke('tts_cloud_usage', args).catch(() => null),
    ]);
    if (mine !== seq) return;
    voices = v.list ?? null;
    error = v.error ?? '';
    models = m || [];
    usage = v.list ? u : null;
    loading = false;
  }
  // Read at once when the page opens; after that, a moment after the key
  // stops changing (typed, pasted, or changed elsewhere).
  let first = true;
  $effect(() => {
    void key;
    untrack(() => {
      clearTimeout(timer);
      if (first) {
        first = false;
        load();
      } else {
        timer = setTimeout(load, 600);
      }
    });
  });
  onMount(() => () => clearTimeout(timer));

  const groups = $derived.by(() => {
    if (!voices) return [];
    const own = voices.filter((v) => v.own);
    const theirs = voices.filter((v) => !v.own);
    return [
      { label: 'Your voices', voices: own },
      { label: 'ElevenLabs voices', voices: theirs },
    ].filter((g) => g.voices.length);
  });
  const gone = $derived(!!voices && !!choice.voice && !voices.some((v) => v.id === choice.voice));
  function pick(v) {
    setChoice({ voice: v.id, voiceName: v.name });
  }

  // ▶: one voice's sample at a time (speech.speakingId "preview").
  let previewed = $state('');
  const playing = $derived(speech.speakingId === 'preview' ? previewed : '');
  async function preview(v) {
    if (playing === v.id) {
      invoke('tts_stop').catch(() => {});
      return;
    }
    previewed = v.id;
    try {
      await invoke('tts_cloud_preview', { provider: ID, voice: v.id, apiKey: key, model, rate: Number(cfg.ttsRate) || 1 });
    } catch (e) {
      toast({ title: "Couldn't play the preview", description: String(e), variant: 'destructive' });
    }
  }

  const modelOptions = $derived.by(() => {
    const opts = models.map((m) => ({ value: m.id, label: m.recommended ? `${m.name} · Recommended` : m.name }));
    if (!opts.some((o) => o.value === model)) opts.push({ value: model, label: model });
    return opts;
  });
  const modelDesc = $derived(
    models.find((m) => m.id === model)?.desc || 'How the voice is made: the fastest to start is recommended for replies'
  );

  const usageText = $derived.by(() => {
    if (!usage || !Number.isFinite(usage.used) || !Number.isFinite(usage.limit)) return '';
    const n = (x) => Number(x).toLocaleString();
    let s = `${n(usage.used)} of ${n(usage.limit)} characters used this month`;
    if (usage.resetsAt) {
      const d = new Date(usage.resetsAt * 1000);
      if (!Number.isNaN(d.getTime())) s += ` · resets ${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}`;
    }
    return s;
  });
  const usageShare = $derived(usage && usage.limit > 0 ? Math.min(100, Math.round((usage.used / usage.limit) * 100)) : 0);

  const status = $derived(
    !key
      ? "Paste your key to hear ElevenLabs' voices. Until then, Yap's own voice speaks."
      : loading
        ? 'Checking the key…'
        : error
          ? `ElevenLabs didn't accept it: ${error}.`
          : voices
            ? `Key works · ${voices.length} ${voices.length === 1 ? 'voice' : 'voices'}`
            : ''
  );
</script>

<Row>
  <div class="keyblock">
    <div class="keyhead">
      <span class="klabel">ElevenLabs API key</span>
      <a class="keylink" href={keysPage} onclick={createExternalLinkHandler(keysPage)} target="_blank" rel="noreferrer">
        Get your API key
      </a>
    </div>
    {#if masked && !editing}
      <div class="keymask">
        <span class="keytext">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="7.5" cy="15.5" r="4.5" />
            <path d="M10.8 12.2 21 2M15 8l3 3" />
          </svg>
          {masked}
        </span>
        <button class="keyedit" onclick={() => (editing = true)}>edit</button>
      </div>
    {:else}
      <!-- Typing keeps the field open; leaving it shows the key masked. -->
      <div class="keymask editing" onfocusout={() => (editing = false)}>
        <Input
          type="password"
          value={key}
          ariaLabel="ElevenLabs API key"
          oninput={(v) => {
            editing = true;
            setKey(v);
          }}
          placeholder="Paste your ElevenLabs API key"
        />
        {#if masked}
          <button class="keyedit" onclick={() => (editing = false)}>done</button>
        {/if}
      </div>
    {/if}
    {#if status}
      <span class="kstatus" class:bad={!!error} class:good={!!voices && !error && !loading} aria-live="polite">{status}</span>
    {/if}
  </div>
</Row>

{#if voices}
  <Row>
    <div class="voices">
      <span class="klabel">Voice</span>
      {#if gone}
        <span class="note">Your saved voice{choice.voiceName ? ` (${choice.voiceName})` : ''} isn't in this account any more: pick another.</span>
      {/if}
      <div class="vlist" role="radiogroup" aria-label="ElevenLabs voice">
        {#each groups as g (g.label)}
          <div class="vgroup">{g.label}</div>
          {#each g.voices as v (v.id)}
            <div class="vrow" class:sel={v.id === voiceId}>
              <button type="button" class="vpick" role="radio" aria-checked={v.id === voiceId} aria-label={v.name} onclick={() => pick(v)}>
                <span class="dot" aria-hidden="true"></span>
                <span class="vname">{v.name}</span>
                {#if v.desc}<span class="vdesc">{v.desc}</span>{/if}
              </button>
              <button
                type="button"
                class="vplay"
                class:on={playing === v.id}
                aria-label={playing === v.id ? `Stop ${v.name}` : `Preview ${v.name}`}
                title={playing === v.id ? 'Stop' : v.preview ? 'Play its sample' : 'Say a sample sentence (uses your characters)'}
                onclick={() => preview(v)}
              >
                {#if playing === v.id}
                  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
                {/if}
              </button>
            </div>
          {/each}
        {/each}
      </div>
    </div>
  </Row>
{/if}

{#if key}
  <Row label="Model" desc={modelDesc}>
    <Select bind:value={() => model, (v) => setChoice({ model: v })} options={modelOptions} ariaLabel="Model" />
  </Row>
{/if}

{#if usageText}
  <Row>
    <div class="usage">
      <span class="utext">{usageText}</span>
      <span class="ubar" aria-hidden="true"><span style:width="{usageShare}%"></span></span>
    </div>
  </Row>
{/if}

<Row>
  <p class="fallback">
    {#if localFallback}
      If ElevenLabs doesn't answer (no connection, a key it won't take, characters used up), Yap's own voice reads the
      answer instead and tells you why, once.
    {:else}
      If ElevenLabs doesn't answer, nothing is read aloud and Yap tells you why. Download Kokoro under On this PC to have
      Yap's own voice step in.
    {/if}
    Each character Yap reads counts against your ElevenLabs plan.
  </p>
</Row>

<style>
  .keyblock,
  .voices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .klabel {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .keylink {
    font-size: 11.5px;
    color: var(--yap-link);
    text-decoration: none;
    white-space: nowrap;
  }
  .keylink:hover {
    text-decoration: underline;
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 7px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .keymask.editing {
    padding: 0;
    border: none;
    background: none;
  }
  .keytext {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    color: var(--yap-fg);
  }
  .keytext svg {
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
  }
  .keyedit {
    padding: 2px 6px;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .kstatus,
  .note {
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .kstatus.bad {
    color: var(--yap-danger);
  }
  .kstatus.good {
    color: var(--yap-fg-80, var(--yap-fg));
  }
  .vlist {
    display: flex;
    flex-direction: column;
    max-height: 264px;
    overflow-y: auto;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .vgroup {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 6px 12px 4px;
    background: var(--yap-s1);
    font-size: 10.5px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--yap-muted-70, var(--yap-muted));
  }
  .vrow {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 6px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .vgroup + .vrow {
    border-top: none;
  }
  .vrow.sel {
    background: var(--yap-raised-soft);
  }
  .vpick {
    display: flex;
    flex: 1;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    padding: 8px 12px;
    border: none;
    background: none;
    font: inherit;
    text-align: left;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .vpick:focus-visible,
  .vplay:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--yap-focus-ring);
  }
  .dot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    transform: translateY(-1px);
  }
  .vrow.sel .dot {
    background: var(--yap-ink);
  }
  .vname {
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
  }
  .vdesc {
    overflow: hidden;
    font-size: 11.5px;
    color: var(--yap-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .vplay {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .vplay:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .vplay.on {
    color: var(--yap-link);
    background: var(--yap-primary-wash);
  }
  .vplay svg {
    width: 11px;
    height: 11px;
  }
  .usage {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .utext {
    font-size: 12px;
    color: var(--yap-fg-80, var(--yap-fg));
  }
  .ubar {
    height: 4px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    overflow: hidden;
  }
  .ubar span {
    display: block;
    height: 100%;
    background: var(--yap-ink);
  }
  .fallback {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
</style>
