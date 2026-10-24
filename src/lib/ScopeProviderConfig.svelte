<script>
  // The provider/model half of a Language-Models "scope" config (mode selector →
  // provider pills → masked key → model list), OpenWhispr-style, operating on a
  // passed-in scope object (`cfg.llmScopes[key]`). Shared by LlmScopeConfig
  // (Note Formatting / Chat) and VoiceAgentConfig so every bubble configures its
  // endpoint the same way. The enable toggle + prompt live in the parent.
  import ModeSelector from './ui/ModeSelector.svelte';
  import PillTabs from './ui/PillTabs.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import EndpointModels from './EndpointModels.svelte';
  import Input from './ui/Input.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Row from './ui/Row.svelte';
  import { PP_CLOUD_MODELS, modelThinks } from './core/ppModels.js';
  import { modelsFor, defaultModel, providerOffers } from './modelCatalog.svelte.js';
  import {
    PROVIDER_MODES as MODES,
    CLOUD_TABS,
    CLOUD_PROVIDER_IDS as CLOUD_IDS,
    BASE_URLS,
    MODEL_HINTS,
    isKeyed,
    modeOf,
    maskKey,
    sharedKey as storeKey,
  } from './core/providers.js';
  import { createExternalLinkHandler } from './core/externalLinks.js';
  import { untrack } from 'svelte';

  // `cfg` is the global Settings config: standard cloud providers share ONE
  // API key per provider across every scope (OpenWhispr keeps openai_api_key
  // etc. global — only "custom" endpoints have a per-scope key), stored in
  // cfg.ppApiKeys. Optional for back-compat: without it keys stay per-scope.
  // `scopeKey` = its key in cfg.llmScopes ("chat", "noteFormatting",
  // "voiceAgent"): which saved key a self-hosted server's model list is
  // asked with.
  let { scope, cfg = null, disabled = false, scopeKey = '' } = $props();

  // The modes, the cloud tabs, the base URLs and the key store are
  // providers.js's: one set of names on every tab (Dictation cleanup's too).
  // Yap supports three of OpenWhispr's five modes — it has no
  // OpenWhispr-hosted "managed cloud" and no enterprise cloud brokerage
  // (Bedrock/Azure/Vertex), which are OpenWhispr's own services.

  // UI state seeded ONCE from the scope (untracked on purpose): Settings mounts
  // one instance per scope tab inside {#key llmScope}, so `scope` is fixed for
  // this component's lifetime and the handlers below keep the two in sync.
  let mode = $state(untrack(() => modeOf(scope.provider)));
  let cloudProvider = $state(untrack(() => (CLOUD_IDS.includes(scope.provider) ? scope.provider : 'groq')));
  let keyEditing = $state(false);

  // Standard cloud providers (everything but custom/local) share the global
  // per-provider key store; custom + self-hosted keys stay per-scope.
  function sharedKey(p) {
    if (!cfg) return scope.apiKeys?.[p] || '';
    return storeKey(cfg, p);
  }
  // Key edits write through to the shared store (and the active cleanup key
  // when it's the same provider) so every scope sees the change immediately.
  function stashApiKey() {
    const p = scope.provider;
    if (!p || p === 'ondevice') return;
    if (cfg && isKeyed(p)) {
      cfg.ppApiKeys = { ...cfg.ppApiKeys, [p]: scope.apiKey };
      if (cfg.ppProvider === p) cfg.ppApiKey = scope.apiKey;
    } else {
      scope.apiKeys = { ...scope.apiKeys, [p]: scope.apiKey };
    }
  }
  // Adopt the shared key on mount: a scope freshly pointed at a provider the
  // user already keyed elsewhere shouldn't sit on an empty key (the bug where
  // Commands hit Anthropic/Groq with no key while Cleanup had one).
  untrack(() => {
    if (cfg && isKeyed(scope.provider)) {
      if (!scope.apiKey) scope.apiKey = sharedKey(scope.provider);
      else if (!cfg.ppApiKeys?.[scope.provider]) stashApiKey();
    }
  });
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
    scope.apiKey = isKeyed(p) ? sharedKey(p) : scope.apiKeys?.[p] || '';
    keyEditing = false;
    // The provider's first model it still offers (modelCatalog.js) when the
    // saved one isn't its.
    if (PP_CLOUD_MODELS[p] && !providerOffers(p, scope.model)) {
      scope.model = defaultModel(p);
    }
  }

  const maskedKey = $derived(maskKey(scope.apiKey));
  // Show the "Disable thinking output" toggle only for reasoning models (or a
  // custom/self-hosted endpoint where the model is unknown).
  const showThinking = $derived(
    mode === 'selfhosted' ||
      (mode === 'cloud' &&
        (cloudProvider === 'custom' ||
          modelThinks(scope.model) ||
          modelsFor(cloudProvider, scope.model).items.some((i) => i.value === scope.model && i.thinks)))
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
              <div class="pp-field"><Input bind:value={scope.model} {disabled} placeholder="model-id" list="scope-custom-models-{scopeKey}" /></div>
            {/snippet}
          </Row>
          <EndpointModels id="scope-custom-models-{scopeKey}" provider="custom" scope={scopeKey} baseUrl={scope.baseUrl} model={scope.model} />
        </div>
      {:else}
        <div class="keyhead">
          <h4>API key</h4>
          <a
            class="keylink"
            href={PP_CLOUD_MODELS[cloudProvider]?.keyUrl}
            onclick={createExternalLinkHandler(PP_CLOUD_MODELS[cloudProvider]?.keyUrl)}
            target="_blank"
            rel="noreferrer">Get your API key</a
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

        <h4 class="selhead">Model</h4>
        <ModelPicker provider={cloudProvider} bind:value={scope.model} {disabled} />
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
            <div class="pp-field"><Input bind:value={scope.model} {disabled} placeholder="llama3.1" list="scope-local-models-{scopeKey}" /></div>
          {/snippet}
        </Row>
        <EndpointModels id="scope-local-models-{scopeKey}" provider="local" scope={scopeKey} baseUrl={scope.baseUrl} model={scope.model} />
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
          Uses the on-device model, the same one as Dictation cleanup: download and pick it under
          <strong>Dictation cleanup → Local</strong>. Nothing leaves this PC.
        </p>
      </div>
    </div>
  {/if}

  {#if showThinking}
    <div class="thinking">
      <Toggle
        bind:checked={scope.disableThinking}
        label="Hide the model's thinking"
        desc="Strips a reasoning model's thinking (Qwen3, GPT-OSS) from the result"
        {disabled}
      />
    </div>
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
  /* A custom or self-hosted endpoint's fields: rows like the rest. */
  .panelcard {
    border-top: 1px solid var(--yap-rule);
  }
  .panelcard > :global(*) {
    border-bottom: 1px solid var(--yap-rule);
  }
  .thinking {
    padding: 12px 0;
    border-top: 1px solid var(--yap-rule);
    border-bottom: 1px solid var(--yap-rule);
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
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .selhead {
    margin-top: 2px;
  }
  .keylink {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .keylink:hover {
    text-decoration-color: currentColor;
  }
  .keylink:focus-visible,
  .keyedit:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: var(--yap-focus);
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    padding: 8px 12px;
  }
  .keymask.editing {
    padding: 0;
    border: none;
    background: none;
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
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .localnote {
    margin: 12px 0;
    font-size: 12.5px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
</style>
