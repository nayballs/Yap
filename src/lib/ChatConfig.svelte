<script>
  // The "Chat & Voice Assistant" Language-Models scope (key `chat`), ported
  // from OpenWhispr's settings/ChatAgentSettings.tsx (InferenceConfigEditor
  // scope="chatIntelligence" + a system-prompt textarea). It drives the AI Chat
  // view, the Notes view's Ask bar and the Voice Assistant's spoken answers
  // (commands.rs resolve_chat_endpoint / chat_turn); the Chat view's model
  // chip switches the same scope (ai_model.rs). The provider/model half is the shared
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

  // Its voice, shortcut and spoken replies are on the Voice Assistant page.
  function openVoiceAssistant() {
    window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'voice' }));
  }
</script>

<div class="scope">
  <Row>
    <Toggle
      bind:checked={scope.enabled}
      label="Enable chat"
      desc="Its own model for Chat, the Ask bar on your notes and the Voice Assistant. Off, they use the Dictation Cleanup model."
    />
  </Row>
  <p class="valine">
    The Voice Assistant asks this model too, and reads its answers aloud.
    <button class="golink" onclick={openVoiceAssistant}>Voice, shortcut and spoken replies in Voice Assistant</button>
  </p>

  {#if enabled}
    <div class="cfg">
      <ScopeProviderConfig {scope} {cfg} disabled={!enabled} />

      <div class="soonpanel">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 8v4.5M12 16h.01" /></svg>
        <p>
          <strong>Note tools need a bigger model.</strong> Cloud models and local models of
          4B or more look up and change your notes; smaller ones answer from a plain search
          of your notes. You can also switch models from the chip under the chat box.
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
  /* "The Voice Assistant asks this model too" + the way to its page. */
  .valine {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
  .golink {
    padding: 5px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .golink:hover {
    background: var(--yap-s2);
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
