<script>
  // The "Chat" Language-Models scope, ported from OpenWhispr's
  // settings/ChatAgentSettings.tsx (InferenceConfigEditor scope="chatIntelligence"
  // + a system-prompt textarea). Yap has no AI Chat surface yet, so the endpoint
  // config is saved now and takes effect once that surface lands — see the
  // "coming soon" note below. The provider/model half is the shared
  // ScopeProviderConfig; this file owns the enable toggle + chat prompt, matching
  // OpenWhispr's section order (config editor → system prompt).
  //
  //   scope: the reactive { enabled, provider, baseUrl, model, apiKey, apiKeys,
  //          prompt } object (`cfg.llmScopes.chat`; the parent seeds it).
  import ScopeProviderConfig from './ScopeProviderConfig.svelte';
  import PromptStudio from './PromptStudio.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Row from './ui/Row.svelte';

  let { scope, cfg = null, defaultPrompt = '' } = $props();

  const enabled = $derived(scope.enabled);
  const PROVIDER_NAMES = {
    groq: 'Groq', anthropic: 'Anthropic', openai: 'OpenAI', gemini: 'Gemini', openrouter: 'OpenRouter',
    ondevice: 'Built-in local AI', local: 'Self-hosted', custom: 'Custom',
  };
  const providerLabel = $derived(PROVIDER_NAMES[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'Built-in model' : scope.model);
</script>

<div class="scope">
  <Row>
    <Toggle
      bind:checked={scope.enabled}
      label="Enable chat"
      desc="A voice assistant that answers questions."
    />
  </Row>

  {#if enabled}
    <div class="cfg">
      <ScopeProviderConfig {scope} {cfg} disabled={!enabled} />

      <div class="soonpanel">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
        <p>
          <strong>Coming soon.</strong> Chat runs once Yap's AI Chat surface lands —
          a voice assistant you can ask questions. Your settings here are saved now
          and will apply automatically when it ships.
        </p>
      </div>

      <div class="section">
        <div class="sechead">
          <h4>System Prompt</h4>
          <p>Custom instructions for the agent.</p>
        </div>
        <PromptStudio bind:prompt={scope.prompt} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
      </div>
    </div>
  {/if}
</div>

<style>
  .scope {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .cfg {
    display: flex;
    flex-direction: column;
    gap: 16px;
    margin-top: 12px;
  }

  .soonpanel {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 12px 14px;
  }
  .soonpanel svg {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .soonpanel p {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .soonpanel strong {
    color: var(--yap-fg);
    font-weight: 600;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-top: 1px solid var(--yap-border-subtle);
    padding-top: 16px;
  }
  .sechead h4 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .sechead p {
    margin: 3px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
</style>
