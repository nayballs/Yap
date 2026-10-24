<script>
  // Settings → Language Models (`cleanup`; OpenWhispr's Language Models
  // page): the tabs Dictation Cleanup / Commands / Note Formatting / Chat &
  // Voice Assistant (scope keys `cleanup`, `voiceAgent`, `noteFormatting`,
  // `chat`, settings/scopes.js; `cleanup#<scope>` opens one). The other
  // three tabs are VoiceAgentConfig / NoteFormattingConfig / ChatConfig;
  // Dictation cleanup's is here: where it runs (Cloud providers / Local /
  // Self-hosted, providers.js), the key, the model, an endpoint's fields,
  // the on-device models (LocalModels.svelte), Spelling, Styles by app
  // (CleanupStyles.svelte), Prompt Studio, Smart routing (SmartRouting.svelte),
  // Usage today (UsageToday.svelte) and where the text goes. What outlives
  // the page (the tab, the mode, the key field open…) is
  // settings/languageModels.svelte.js's.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  import { tick } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import ModeSelector from '../ui/ModeSelector.svelte';
  import PillTabs from '../ui/PillTabs.svelte';
  import PromptStudio from '../PromptStudio.svelte';
  import VoiceAgentConfig from '../VoiceAgentConfig.svelte';
  import RegionSpelling from '../RegionSpelling.svelte';
  import CleanupStyles from './CleanupStyles.svelte';
  import NoteFormattingConfig from '../NoteFormattingConfig.svelte';
  import ChatConfig from '../ChatConfig.svelte';
  import ModelPicker from '../ModelPicker.svelte';
  import EndpointModels from '../EndpointModels.svelte';
  // CSS order (Vite bundles it in import order): the components above come
  // before Settings' shared styles, as when Settings imported them itself —
  // the scope tabs' own `.offnote` / `.tools` margins lose to `.set-note`'s
  // — and this page's styles and the three below after them (its
  // `.set-note.offnote` wins). Settings.svelte imports this page just
  // before settings.css for the same reason. The layout is pinned by it.
  import './settings.css';
  import LocalModels from './LocalModels.svelte';
  import SmartRouting from './SmartRouting.svelte';
  import UsageToday from './UsageToday.svelte';
  import { PP_CLOUD_MODELS, modelThinks } from '../core/ppModels.js';
  import { modelsFor, defaultModel, providerOffers } from '../stores/modelCatalog.svelte.js';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import {
    PP_PROVIDERS,
    PROVIDER_MODES,
    CLOUD_TABS,
    MODEL_HINTS,
    CLEANUP_PROVIDER_LABELS,
    maskKey,
    stashCleanupKey,
    cleanupNeedsKey,
  } from '../core/providers.js';
  import { SCOPE_TABS, SCOPE_SUBTITLE, SCOPE_DEFAULTS } from './scopes.js';
  import { PP_PRESETS, PP_DEFAULT_BODY } from './cleanupPresets.js';
  import { lm, refreshUsage } from './languageModels.svelte.js';
  import { localLlm, refreshLocalLlm } from './localLlm.svelte.js';
  import { persist } from './configSync.svelte.js';

  let { cfg, goto = null } = $props();

  const wakeName = $derived((cfg?.agentName || '').trim() || 'Yap');
  // The modes' icons, by kind (cloud, this PC, a server).
  const PP_ICON_KIND = Object.fromEntries(PROVIDER_MODES.map((o) => [o.value, o.kind]));

  // Cleanup on, a cloud provider, no key: the red badge's reason, said on
  // the page with the way to fix it (review #9).
  let keyBox = $state(null);
  const needsKey = $derived(cleanupNeedsKey(cfg));
  function focusKey() {
    lm.keyEditing = true;
    tick().then(() => {
      keyBox?.scrollIntoView({ block: 'center' });
      keyBox?.querySelector('input')?.focus();
    });
  }

  // Picking a provider preset fills in its base URL (except "custom", which
  // leaves the field editable).
  function onProviderChange(value) {
    const preset = PP_PROVIDERS.find((p) => p.value === value);
    if (preset && preset.baseUrl) cfg.ppBaseUrl = preset.baseUrl;
    if (value === 'ondevice') refreshLocalLlm();
  }

  // Mode / cloud-provider wiring (OpenWhispr semantics): switching resolves to
  // a concrete cfg.ppProvider; picking a cloud provider auto-selects its first
  // registry model when the current model id doesn't belong to it. Each
  // provider keeps its own API key in cfg.ppApiKeys — stashed on the way out,
  // restored on the way in (the backend only reads the active cfg.ppApiKey).
  function stashApiKey() {
    stashCleanupKey(cfg);
  }
  function onModeChange(mode) {
    stashApiKey();
    if (mode === 'ondevice') {
      cfg.ppProvider = 'ondevice';
      onProviderChange('ondevice');
    } else if (mode === 'selfhosted') {
      cfg.ppProvider = 'local';
      cfg.ppApiKey = cfg.ppApiKeys.local || '';
      onProviderChange('local');
    } else {
      onCloudProviderChange(lm.cloudProvider);
    }
  }
  function onCloudProviderChange(p) {
    stashApiKey();
    lm.cloudProvider = p;
    cfg.ppProvider = p;
    cfg.ppApiKey = cfg.ppApiKeys[p] || '';
    onProviderChange(p);
    // The provider's first model it still offers (Yap's list merged with the
    // provider's own, modelCatalog.js) when the saved one isn't its.
    if (PP_CLOUD_MODELS[p] && !providerOffers(p, cfg.ppModel)) {
      cfg.ppModel = defaultModel(p);
    }
    lm.keyEditing = false;
  }
  // The "Select Model" rows are ModelPicker's: the provider's registry merged
  // with its live list, all carrying the provider's brand icon (OpenWhispr
  // does the same); an unknown saved id stays visible instead of being
  // silently deselected.
  const maskedKey = $derived(maskKey(cfg.ppApiKey));
  // Show the "Disable thinking output" toggle only when the chosen cleanup model
  // reasons (or the endpoint is custom/self-hosted, where the model is unknown) —
  // matches OpenWhispr's supportsThinking gating. Local (on-device) curated models
  // don't reason, so it stays hidden there.
  const cleanupShowThinking = $derived(
    lm.mode === 'selfhosted' ||
      (lm.mode === 'cloud' &&
        (lm.cloudProvider === 'custom' ||
          modelThinks(cfg?.ppModel) ||
          modelsFor(lm.cloudProvider, cfg?.ppModel).items.some((i) => i.value === cfg?.ppModel && i.thinks)))
  );
</script>

{#snippet provIcon(v)}
  {@const kind = PP_ICON_KIND[v] || 'cloud'}
  {#if kind === 'local'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="5" y="5" width="14" height="14" rx="2" /><rect x="9" y="9" width="6" height="6" />
      <path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" />
    </svg>
  {:else if kind === 'server'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="3" y="4" width="18" height="7" rx="2" /><rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M7 8h.01M7 17h.01" />
    </svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M17.5 19a4.5 4.5 0 0 0 .5-8.98 6 6 0 0 0-11.6-1.02A4 4 0 0 0 6 19z" />
    </svg>
  {/if}
{/snippet}

<!-- Icons for the Language-Models scope bubbles (OpenWhispr uses lucide
     Wand2 / Sparkles / BookOpen / MessageSquare for Cleanup/Agent/Notes/Chat). -->
{#snippet scopeIcon(v)}
  {#if v === 'voiceAgent'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9.94 15.5A2 2 0 0 0 8.5 14.06l-4.6-1.19a.5.5 0 0 1 0-.96l4.6-1.19A2 2 0 0 0 9.94 9.3l1.19-4.6a.5.5 0 0 1 .96 0l1.19 4.6a2 2 0 0 0 1.44 1.44l4.6 1.19a.5.5 0 0 1 0 .96l-4.6 1.19a2 2 0 0 0-1.44 1.44l-1.19 4.6a.5.5 0 0 1-.96 0z" /><path d="M20 3v3M21.5 4.5h-3M5 18v2M6 19H4" /></svg>
  {:else if v === 'noteFormatting'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 7v14" /><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" /></svg>
  {:else if v === 'chat'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m3 21 9-9M15 4V2M15 16v-2M8 9h2M20 9h2M17.8 11.8 19 13M15 9h.01M17.8 6.2 19 5M12.2 6.2 11 5" /></svg>
  {/if}
{/snippet}

<div class="page-h">
  <h1>Language Models</h1>
  <p>{SCOPE_SUBTITLE[lm.scope].replace('{name}', wakeName)}</p>
</div>
<div class="scope-tabs">
  <PillTabs value={lm.scope} options={SCOPE_TABS} onchange={(v) => (lm.scope = v)} renderIcon={scopeIcon} />
</div>

{#if lm.scope === 'voiceAgent'}
  {#key lm.scope}
    <VoiceAgentConfig {cfg} scope={cfg.llmScopes.voiceAgent} defaultPrompt={SCOPE_DEFAULTS.voiceAgent.prompt} />
  {/key}
{:else if lm.scope === 'noteFormatting'}
  {#key lm.scope}
    <NoteFormattingConfig {cfg} scope={cfg.llmScopes.noteFormatting} defaultPrompt={SCOPE_DEFAULTS.noteFormatting.prompt} />
  {/key}
{:else if lm.scope === 'chat'}
  {#key `${lm.scope}:${lm.chatScopeRev}`}
    <ChatConfig {cfg} scope={cfg.llmScopes.chat} defaultPrompt={SCOPE_DEFAULTS.chat.prompt} />
  {/key}
{:else}
  <Group title="Dictation cleanup" id="settings-cleanup">
    <Row setting="cleanup-enable">
      <Toggle
        bind:checked={cfg.postProcessEnabled}
        label="Clean up dictations with AI"
        desc="Filler words out, grammar and punctuation fixed"
      />
    </Row>
    {#if cfg.postProcessEnabled}
      <ModeSelector bind:value={lm.mode} options={PROVIDER_MODES} icon={provIcon} onchange={onModeChange} />
    {/if}
  </Group>

  {#if !cfg.postProcessEnabled}
    <!-- Off: one line instead of a screen of greyed-out choices. -->
    <p class="set-note offnote">Turn on cleanup to pick a model. Until then, Yap types exactly what you said.</p>
  {:else}
    {#if needsKey}
      <!-- The red badge's reason, said where it's fixed. -->
      <div class="set-banner" role="status" data-testid="cleanup-key-banner">
        <span class="dot" aria-hidden="true"></span>
        <span class="text">
          Add your {CLEANUP_PROVIDER_LABELS[cfg.ppProvider] || cfg.ppProvider} API key
          <span>Cleanup is on, but without a key Yap types your words as they are.</span>
        </span>
        <Button size="sm" onclick={focusKey}>Add key</Button>
      </div>
    {/if}
    {#if lm.mode === 'cloud'}
      <div class="cloudcfg">
        <PillTabs bind:value={lm.cloudProvider} options={CLOUD_TABS} onchange={onCloudProviderChange} />

        {#if lm.cloudProvider === 'custom'}
          <div class="panelcard">
            <Row label="Base URL" desc="Any OpenAI-compatible endpoint">
              {#snippet children()}
                <div class="pp-field">
                  <Input bind:value={cfg.ppBaseUrl} disabled={!cfg.postProcessEnabled} placeholder="https://api.example.com/v1" />
                </div>
              {/snippet}
            </Row>
            <Row label="API key" desc="Stored only on this PC">
              {#snippet children()}
                <div class="pp-field">
                  <Input type="password" bind:value={cfg.ppApiKey} oninput={stashApiKey} disabled={!cfg.postProcessEnabled} placeholder="sk-…" />
                </div>
              {/snippet}
            </Row>
            <Row label="Model" desc={MODEL_HINTS.custom}>
              {#snippet children()}
                <div class="pp-field">
                  <Input bind:value={cfg.ppModel} disabled={!cfg.postProcessEnabled} placeholder="model-id" list="pp-custom-models" />
                </div>
              {/snippet}
            </Row>
            <EndpointModels id="pp-custom-models" provider="custom" scope="cleanup" baseUrl={cfg.ppBaseUrl} model={cfg.ppModel} />
          </div>
        {:else}
          <div class="keyhead">
            <h4>API key</h4>
            <a
              class="keylink"
              href={PP_CLOUD_MODELS[lm.cloudProvider].keyUrl}
              onclick={createExternalLinkHandler(PP_CLOUD_MODELS[lm.cloudProvider].keyUrl)}
              target="_blank"
              rel="noreferrer"
            >
              Get your API key
            </a>
          </div>
          {#if maskedKey && !lm.keyEditing}
            <div class="keymask">
              <span class="keytext">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <circle cx="7.5" cy="15.5" r="4.5" />
                  <path d="M10.8 12.2 21 2M15 8l3 3" />
                </svg>
                {maskedKey}
              </span>
              <button class="keyedit" onclick={() => (lm.keyEditing = true)}>edit</button>
            </div>
          {:else}
            <div class="keymask editing" bind:this={keyBox}>
              <Input type="password" bind:value={cfg.ppApiKey} oninput={stashApiKey} placeholder="sk-…" ariaLabel="API key" />
              {#if maskedKey}
                <button class="keyedit" onclick={() => (lm.keyEditing = false)}>done</button>
              {/if}
            </div>
          {/if}

          <h4 class="selhead">Model</h4>
          <ModelPicker provider={lm.cloudProvider} bind:value={cfg.ppModel} />
        {/if}
      </div>
    {:else if lm.mode === 'selfhosted'}
      <div class="cloudcfg">
        <div class="panelcard">
          <Row label="Base URL" desc="Your Ollama or LM Studio endpoint">
            {#snippet children()}
              <div class="pp-field">
                <Input bind:value={cfg.ppBaseUrl} disabled={!cfg.postProcessEnabled} placeholder="http://localhost:11434/v1" />
              </div>
            {/snippet}
          </Row>
          <Row label="Model" desc={MODEL_HINTS.local}>
            {#snippet children()}
              <div class="pp-field">
                <Input bind:value={cfg.ppModel} disabled={!cfg.postProcessEnabled} placeholder="llama3.1" list="pp-local-models" />
              </div>
            {/snippet}
          </Row>
          <EndpointModels id="pp-local-models" provider="local" scope="cleanup" baseUrl={cfg.ppBaseUrl} model={cfg.ppModel} />
          <Row label="API key" desc="Optional — most local servers don't need one">
            {#snippet children()}
              <div class="pp-field">
                <Input type="password" bind:value={cfg.ppApiKey} oninput={stashApiKey} disabled={!cfg.postProcessEnabled} placeholder="" />
              </div>
            {/snippet}
          </Row>
        </div>
      </div>
    {:else}
      <div class="cloudcfg">
        <LocalModels {cfg} />
      </div>
    {/if}

    {#if cleanupShowThinking}
      <Group bare>
        <Row setting="cleanup-thinking">
          <Toggle
            bind:checked={cfg.ppDisableThinking}
            label="Hide the model's thinking"
            desc="Strips a reasoning model's thinking (Qwen3, GPT-OSS) from the cleaned text"
          />
        </Row>
      </Group>
    {/if}
  {/if}

  <Group title="Spelling" id="settings-cleanup-spelling">
    <Row setting="cleanup-spelling">
      <RegionSpelling {cfg} disabled={!cfg.postProcessEnabled} />
    </Row>
  </Group>

  {#if cfg.postProcessEnabled}
    <CleanupStyles {cfg} />

    <Group title="Prompt Studio" id="settings-prompt-studio" desc="View, change and test what the cleanup model is told" bare>
      <div class="ps-wrap" data-setting="prompt-studio">
        <PromptStudio
          bind:prompt={cfg.ppPrompt}
          bind:preset={cfg.ppPreset}
          presets={PP_PRESETS}
          basePrompt={lm.basePrompt}
          defaultBody={PP_DEFAULT_BODY}
          enabled={cfg.postProcessEnabled}
          testCommand="test_post_process"
          providerLabel={CLEANUP_PROVIDER_LABELS[cfg.ppProvider] || cfg.ppProvider}
          modelLabel={cfg.ppProvider === 'ondevice' ? localLlm.status.model : cfg.ppModel}
          onpersist={persist}
          ontested={refreshUsage}
        />
      </div>
    </Group>

    <SmartRouting {cfg} />

    <UsageToday {cfg} />

    <p class="set-note pp-privacy">
      {#if cfg.ppProvider === 'ondevice'}
        Cleanup runs on this PC: your words never leave it.
      {:else if cfg.ppProvider === 'local'}
        Cleanup goes to your own server.
      {:else}
        Cleanup sends the text of each dictation to {CLEANUP_PROVIDER_LABELS[cfg.ppProvider] || 'the provider'}. Pick Local to keep it on this PC.
      {/if}
      <button class="set-link" onclick={() => goto('privacy#leaves')}>Privacy & data</button>
    </p>
  {/if}
{/if}

<style>
  /* Scope "bubbles" row (Dictation Cleanup / Commands / Note Formatting / Chat & Voice Assistant). */
  .scope-tabs {
    margin: -6px 0 26px;
  }

  /* ---- Language Models: cloud/self-hosted/local config (OpenWhispr layout) ---- */
  .cloudcfg {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 22px;
  }
  /* A custom or self-hosted endpoint's fields: rows like the rest. */
  .panelcard {
    border-top: 1px solid var(--yap-rule);
  }
  .panelcard > :global(*) {
    border-bottom: 1px solid var(--yap-rule);
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-top: 4px;
  }
  .keyhead h4,
  .selhead {
    margin: 0;
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .selhead {
    margin-top: 6px;
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
  .keylink:focus-visible {
    outline: none;
    border-radius: 2px;
    box-shadow: var(--yap-focus);
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .keymask.editing {
    padding: 0;
    border: none;
    background: none;
  }
  .keymask.editing :global(.input) {
    flex: 1 1 auto;
  }
  .keytext {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    color: var(--yap-fg-80);
  }
  .keytext svg {
    width: 12px;
    height: 12px;
    color: var(--yap-fg-45);
  }
  .keyedit {
    border: none;
    background: none;
    padding: 2px 4px;
    border-radius: 4px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    cursor: pointer;
    transition: color var(--yap-dur) ease;
  }
  .keyedit:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .keyedit:hover {
    color: var(--yap-fg);
  }

  /* Cleanup off: one line where its choices were. */
  .offnote {
    margin: -18px 0 30px;
  }
  .pp-privacy {
    margin: -10px 0 0;
  }
  /* AI cleanup */
  .pp-field {
    width: 260px;
    max-width: 260px;
  }
  .ps-wrap {
    margin-top: 6px;
    scroll-margin-top: 16px;
  }
</style>
