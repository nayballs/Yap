<script>
  // Settings → Voice Assistant → Voice, with OpenAI, Gemini or Groq picked
  // (voice/tts/hosted.rs and its openai.rs, gemini.rs, groq.rs): they speak
  // on the key Language Models already has for that provider — no key of
  // their own here, and VoiceSettings only offers one once it has a key.
  // - which key: "Uses your OpenAI API key from Language Models (sk-…1a2b)";
  // - the voices (Yap's list of the provider's own), each with ▶: the sample
  //   sentence said through the provider, a fraction of a penny on the key
  //   (its tooltip says so), played like an answer (Yap's volume, the output
  //   device);
  // - the model (the recommended one first) and what happens when the
  //   provider doesn't answer (Yap's own voice steps in, if there's one).
  //
  //   cfg: Settings' config $state (ttsCloud[provider] = {voice, voiceName,
  //        model}). Auto-save does the saving.
  //   provider: tts_voices' entry ({id, name, defaultVoice, defaultModel}).
  //   localFallback: a local voice can stand in (Kokoro downloaded, or
  //     Windows voices chosen).
  import { invoke } from '@tauri-apps/api/core';
  import Row from '../ui/Row.svelte';
  import Select from '../ui/Select.svelte';
  import CloudVoiceList from './CloudVoiceList.svelte';
  import { speech } from '../stores/speech.svelte.js';
  import { toast } from '../ui/toast.svelte.js';
  import { maskKey, sharedKey } from '../core/providers.js';

  let { cfg, provider, localFallback = false } = $props();

  const id = $derived(provider.id);
  const name = $derived(provider.name);
  // The key Chat would use for it (its own, else the shared one): the
  // backend's ai_model::provider_key, read from this copy so a key just
  // pasted in Language Models works before it's saved.
  const key = $derived.by(() => {
    const chat = cfg.llmScopes?.chat;
    const own = chat ? (chat.provider === id ? chat.apiKey : chat.apiKeys?.[id]) : '';
    return own || sharedKey(cfg, id) || '';
  });
  const choice = $derived((cfg.ttsCloud ?? {})[id] ?? {});

  let voices = $state([]);
  let models = $state([]);
  let error = $state('');
  let seq = 0;
  async function load() {
    const mine = ++seq;
    const args = { provider: id, apiKey: key };
    const [v, m] = await Promise.all([
      invoke('tts_cloud_voices', args).then(
        (list) => ({ list }),
        (e) => ({ error: String(e) })
      ),
      invoke('tts_cloud_models', args).catch(() => []),
    ]);
    if (mine !== seq) return;
    voices = v.list ?? [];
    error = v.error ?? '';
    models = m || [];
  }
  $effect(() => {
    void id;
    void key;
    load();
  });

  const voiceId = $derived(
    voices.some((v) => v.id === choice.voice) ? choice.voice : provider.defaultVoice || voices[0]?.id || ''
  );
  const model = $derived(choice.model || provider.defaultModel || models[0]?.id || '');
  function setChoice(patch) {
    cfg.ttsCloud = { ...(cfg.ttsCloud ?? {}), [id]: { voice: '', voiceName: '', model: '', ...choice, ...patch } };
  }
  function pick(v) {
    setChoice({ voice: v.id, voiceName: v.name });
  }

  // ▶: the sample sentence through the provider (speech.speakingId "preview").
  let previewed = $state('');
  const playing = $derived(speech.speakingId === 'preview' ? previewed : '');
  async function preview(v) {
    if (playing === v.id) {
      invoke('tts_stop').catch(() => {});
      return;
    }
    previewed = v.id;
    try {
      await invoke('tts_cloud_preview', { provider: id, voice: v.id, apiKey: key, model, rate: Number(cfg.ttsRate) || 1 });
    } catch (e) {
      toast({ title: "Couldn't play the preview", description: String(e), variant: 'destructive' });
    }
  }

  const modelOptions = $derived.by(() => {
    const opts = models.map((m) => ({ value: m.id, label: m.recommended ? `${m.name} · Recommended` : m.name }));
    if (model && !opts.some((o) => o.value === model)) opts.push({ value: model, label: model });
    return opts;
  });
  const modelDesc = $derived(models.find((m) => m.id === model)?.desc || '');
  const groups = $derived([{ label: '', voices }]);
</script>

<Row>
  <p class="keyline">
    Uses your {name} API key from Language Models{#if key}&nbsp;<span class="mask">({maskKey(key)})</span>{/if}.
  </p>
</Row>

{#if error}
  <Row>
    <p class="err" role="alert">{error}</p>
  </Row>
{:else if voices.length}
  <Row>
    <div class="voices">
      <span class="klabel">Voice</span>
      <CloudVoiceList
        label="{name} voice"
        {groups}
        selected={voiceId}
        {playing}
        onpick={pick}
        onpreview={preview}
        previewTitle={() => `Say a sample sentence: a fraction of a penny on your ${name} key`}
      />
    </div>
  </Row>
{/if}

{#if modelOptions.length}
  <Row label="Model" desc={modelDesc} setting="voice-model">
    <Select bind:value={() => model, (v) => setChoice({ model: v })} options={modelOptions} ariaLabel="Model" disabled={modelOptions.length < 2} />
  </Row>
{/if}

<Row>
  <p class="fallback">
    {#if localFallback}
      If {name} doesn't answer (no connection, a key it won't take, no credit left), Yap's own voice reads the answer
      instead and tells you why, once.
    {:else}
      If {name} doesn't answer, nothing is read aloud and Yap tells you why. Download Kokoro under On this PC to have Yap's
      own voice step in.
    {/if}
    Every answer read aloud is billed to your {name} account, a fraction of a penny a sentence: Insights shows it under AI
    usage.
  </p>
</Row>

<style>
  .voices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .klabel {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .keyline,
  .fallback {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .keyline {
    font-size: 12px;
    color: var(--yap-fg-80, var(--yap-fg));
  }
  .mask {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .err {
    margin: 0;
    font-size: 12px;
    color: var(--yap-danger);
  }
</style>
