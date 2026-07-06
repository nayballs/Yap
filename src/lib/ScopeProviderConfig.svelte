<script>
  // The provider/model half of a Language-Models "scope" config (mode selector →
  // provider pills → masked key → model list), OpenWhispr-style, operating on a
  // passed-in scope object (`cfg.llmScopes[key]`). Shared by LlmScopeConfig
  // (Note Formatting / Chat) and VoiceAgentConfig so every bubble configures its
  // endpoint the same way. The enable toggle + prompt live in the parent.
  import ModeSelector from './ui/ModeSelector.svelte';
  import PillTabs from './ui/PillTabs.svelte';
  import SelectList from './ui/SelectList.svelte';
  import Input from './ui/Input.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Row from './ui/Row.svelte';
  import { PP_CLOUD_MODELS, modelThinks } from './ppModels.js';
  import { PROVIDER_ICONS } from './providerIcons.js';
  import { createExternalLinkHandler } from './externalLinks.js';

  // `cfg` is the global Settings config: standard cloud providers share ONE
  // API key per provider across every scope (OpenWhispr keeps openai_api_key
  // etc. global — only "custom" endpoints have a per-scope key), stored in
  // cfg.ppApiKeys. Optional for back-compat: without it keys stay per-scope.
  let { scope, cfg = null, disabled = false } = $props();

  // Mode labels/descriptions ported from OpenWhispr's per-scope mode copy
  // (locales/en `dictationAgent.modes` etc.). Yap supports three of OpenWhispr's
  // five modes — it has no OpenWhispr-hosted "managed cloud" and no enterprise
  // cloud brokerage (Bedrock/Azure/Vertex), which are OpenWhispr's own services.
  const MODES = [
    { value: 'cloud', label: 'Bring your own key', desc: 'Connect to OpenAI, Anthropic, Groq, or OpenRouter with your own API key.', kind: 'cloud' },
    { value: 'ondevice', label: 'Local', desc: 'Run a local model on your device — fully private.', kind: 'local' },
    { value: 'selfhosted', label: 'Self-hosted', desc: 'Point at a self-hosted OpenAI-compatible endpoint.', kind: 'server' },
  ];
  const CLOUD_TABS = [
    { value: 'groq', label: 'Groq', icon: PROVIDER_ICONS.groq },
    { value: 'anthropic', label: 'Anthropic', icon: PROVIDER_ICONS.anthropic, mono: true },
    { value: 'openai', label: 'OpenAI', icon: PROVIDER_ICONS.openai, mono: true },
    { value: 'gemini', label: 'Gemini', icon: PROVIDER_ICONS.gemini },
    { value: 'openrouter', label: 'OpenRouter' },
    { value: 'custom', label: 'Custom' },
  ];
  const CLOUD_IDS = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter', 'custom'];
  const BASE_URLS = {
    groq: 'https://api.groq.com/openai/v1',
    anthropic: 'https://api.anthropic.com/v1',
    openai: 'https://api.openai.com/v1',
    gemini: 'https://generativelanguage.googleapis.com/v1beta/openai/',
    openrouter: 'https://openrouter.ai/api/v1',
    local: 'http://localhost:11434/v1',
  };
  const MODEL_HINTS = {
    local: 'your Ollama / LM Studio model name (e.g. llama3.1)',
    custom: 'the model id your endpoint expects',
  };

  let mode = $state(
    scope.provider === 'ondevice' ? 'ondevice' : scope.provider === 'local' ? 'selfhosted' : 'cloud'
  );
  let cloudProvider = $state(CLOUD_IDS.includes(scope.provider) ? scope.provider : 'groq');
  let keyEditing = $state(false);

  // Standard cloud providers (everything but custom/local) share the global
  // per-provider key store; custom + self-hosted keys stay per-scope.
  const SHARED_KEY_IDS = new Set(['groq', 'anthropic', 'openai', 'gemini', 'openrouter']);
  function sharedKey(p) {
    if (!cfg) return scope.apiKeys?.[p] || '';
    return cfg.ppApiKeys?.[p] || (cfg.ppProvider === p ? cfg.ppApiKey : '') || '';
  }
  // Key edits write through to the shared store (and the active cleanup key
  // when it's the same provider) so every scope sees the change immediately.
  function stashApiKey() {
    const p = scope.provider;
    if (!p || p === 'ondevice') return;
    if (cfg && SHARED_KEY_IDS.has(p)) {
      cfg.ppApiKeys = { ...cfg.ppApiKeys, [p]: scope.apiKey };
      if (cfg.ppProvider === p) cfg.ppApiKey = scope.apiKey;
    } else {
      scope.apiKeys = { ...scope.apiKeys, [p]: scope.apiKey };
    }
  }
  // Adopt the shared key on mount: a scope freshly pointed at a provider the
  // user already keyed elsewhere shouldn't sit on an empty key (the bug where
  // Voice Agent hit Anthropic/Groq with no key while Cleanup had one).
  if (cfg && SHARED_KEY_IDS.has(scope.provider)) {
    if (!scope.apiKey) scope.apiKey = sharedKey(scope.provider);
    else if (!cfg.ppApiKeys?.[scope.provider]) stashApiKey();
  }
  function onModeChange(m) {
    mode = m;
    if (m === 'ondevice') {
      scope.provider = 'ondevice';
      scope.baseUrl = '';
    } else if (m === 'selfhosted') {
      scope.provider = 'local';
      if (!scope.baseUrl) scope.baseUrl = BASE_URLS.local;
      scope.apiKey = scope.apiKeys?.local || '';
    } else {
      onCloudProviderChange(cloudProvider);
    }
  }
  function onCloudProviderChange(p) {
    cloudProvider = p;
    scope.provider = p;
    if (BASE_URLS[p]) scope.baseUrl = BASE_URLS[p];
    scope.apiKey = SHARED_KEY_IDS.has(p) ? sharedKey(p) : scope.apiKeys?.[p] || '';
    keyEditing = false;
    const reg = PP_CLOUD_MODELS[p];
    if (reg && !reg.models.some((m) => m.value === scope.model)) {
      scope.model = reg.models[0]?.value || '';
    }
  }

  const cloudModelOptions = $derived.by(() => {
    const reg = PP_CLOUD_MODELS[cloudProvider];
    if (!reg) return [];
    const icon = PROVIDER_ICONS[cloudProvider];
    const mono = cloudProvider === 'anthropic' || cloudProvider === 'openai';
    const opts = reg.models.map((m) => ({ value: m.value, label: m.label, desc: m.desc, icon, mono }));
    if (scope.model && !opts.some((o) => o.value === scope.model)) {
      opts.push({ value: scope.model, label: scope.model, desc: 'Custom model id', icon, mono });
    }
    return opts;
  });
  const maskedKey = $derived(
    !scope.apiKey
      ? ''
      : scope.apiKey.length > 8
        ? `${scope.apiKey.slice(0, 3)}…${scope.apiKey.slice(-4)}`
        : '••••••••'
  );
  // Show the "Disable thinking output" toggle only for reasoning models (or a
  // custom/self-hosted endpoint where the model is unknown).
  const showThinking = $derived(
    mode === 'selfhosted' ||
      (mode === 'cloud' && (cloudProvider === 'custom' || modelThinks(scope.model)))
  );
</script>

{#snippet modeIcon(v)}
  {@const kind = MODES.find((m) => m.value === v)?.kind || 'cloud'}
  {#if kind === 'local'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><rect x="9" y="9" width="6" height="6" /><path d="M9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3" /></svg>
  {:else if kind === 'server'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="7" rx="1.5" /><rect x="3" y="13" width="18" height="7" rx="1.5" /><path d="M7 7.5h.01M7 16.5h.01" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17.5 19a4.5 4.5 0 0 0 .5-8.98A6 6 0 0 0 6.1 9.5 4 4 0 0 0 6.5 19z" /></svg>
  {/if}
{/snippet}

<div class="cfg">
  <ModeSelector value={mode} options={MODES} icon={modeIcon} onchange={onModeChange} {disabled} />

  {#if mode === 'cloud'}
    <div class="cloudcfg">
      <PillTabs value={cloudProvider} options={CLOUD_TABS} onchange={onCloudProviderChange} />

      {#if cloudProvider === 'custom'}
        <div class="panelcard">
          <Row label="Base URL" desc="Any OpenAI-compatible endpoint">
            {#snippet children()}
              <div class="pp-field"><Input bind:value={scope.baseUrl} {disabled} placeholder="https://api.example.com/v1" /></div>
            {/snippet}
          </Row>
          <Row label="API key" desc="Stored only on this PC">
            {#snippet children()}
              <div class="pp-field"><Input type="password" bind:value={scope.apiKey} oninput={stashApiKey} {disabled} placeholder="sk-…" /></div>
            {/snippet}
          </Row>
          <Row label="Model" desc={MODEL_HINTS.custom}>
            {#snippet children()}
              <div class="pp-field"><Input bind:value={scope.model} {disabled} placeholder="model-id" /></div>
            {/snippet}
          </Row>
        </div>
      {:else}
        <div class="keyhead">
          <h4>API Key</h4>
          <a
            class="keylink"
            href={PP_CLOUD_MODELS[cloudProvider]?.keyUrl}
            onclick={createExternalLinkHandler(PP_CLOUD_MODELS[cloudProvider]?.keyUrl)}
            target="_blank"
            rel="noreferrer">Get your API key →</a
          >
        </div>
        {#if maskedKey && !keyEditing}
          <div class="keymask">
            <span class="keytext">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="7.5" cy="15.5" r="4.5" /><path d="M10.8 12.2 21 2M15 8l3 3" /></svg>
              {maskedKey}
            </span>
            <button class="keyedit" onclick={() => (keyEditing = true)}>edit</button>
          </div>
        {:else}
          <div class="keymask editing">
            <Input type="password" bind:value={scope.apiKey} oninput={stashApiKey} {disabled} placeholder="sk-…" />
            {#if maskedKey}<button class="keyedit" onclick={() => (keyEditing = false)}>done</button>{/if}
          </div>
        {/if}

        <h4 class="selhead">Select Model</h4>
        <SelectList bind:value={scope.model} options={cloudModelOptions} {disabled} />
      {/if}
    </div>
  {:else if mode === 'selfhosted'}
    <div class="cloudcfg">
      <div class="panelcard">
        <Row label="Base URL" desc="Your Ollama or LM Studio endpoint">
          {#snippet children()}
            <div class="pp-field"><Input bind:value={scope.baseUrl} {disabled} placeholder="http://localhost:11434/v1" /></div>
          {/snippet}
        </Row>
        <Row label="Model" desc={MODEL_HINTS.local}>
          {#snippet children()}
            <div class="pp-field"><Input bind:value={scope.model} {disabled} placeholder="llama3.1" /></div>
          {/snippet}
        </Row>
        <Row label="API key" desc="Optional — most local servers don't need one">
          {#snippet children()}
            <div class="pp-field"><Input type="password" bind:value={scope.apiKey} oninput={stashApiKey} {disabled} placeholder="" /></div>
          {/snippet}
        </Row>
      </div>
    </div>
  {:else}
    <div class="cloudcfg">
      <div class="panelcard">
        <p class="localnote">
          Uses Yap's built-in on-device model — the same local server as Dictation Cleanup.
          Install it and pick the model under <strong>Dictation Cleanup → Local</strong>; this
          scope will share it. Nothing leaves your PC.
        </p>
      </div>
    </div>
  {/if}

  {#if showThinking}
    <Toggle
      bind:checked={scope.disableThinking}
      label="Disable thinking output"
      desc="Strip the model's reasoning blocks from the result — for reasoning models like Qwen3 or GPT-OSS"
      {disabled}
    />
  {/if}
</div>

<style>
  .cfg {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .cloudcfg {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .panelcard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 4px 14px;
  }
  .pp-field {
    min-width: 220px;
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .keyhead h4,
  .selhead {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .selhead {
    margin-top: 2px;
  }
  .keylink {
    font-size: 11.5px;
    color: var(--yap-primary);
    text-decoration: none;
  }
  .keylink:hover {
    text-decoration: underline;
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    padding: 8px 12px;
  }
  .keymask.editing {
    padding: 6px 8px 6px 12px;
  }
  .keytext {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .keytext svg {
    width: 13px;
    height: 13px;
  }
  .keyedit {
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .localnote {
    margin: 8px 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
</style>
