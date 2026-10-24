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
  import Toggle from '../ui/Toggle.svelte';
  import Row from '../ui/Row.svelte';
  import Group from '../ui/Group.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import { SCOPE_PROVIDER_LABELS } from '../core/providers.js';

  let { scope, cfg = null, defaultPrompt = '' } = $props();

  const enabled = $derived(scope.enabled);
  const providerLabel = $derived(SCOPE_PROVIDER_LABELS[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'The on-device model' : scope.model);

  // Its voice, shortcut and spoken replies are on the Voice Assistant page.
  function openVoiceAssistant() {
    window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'voice' }));
  }
</script>

<div class="scope">
  <Group title="Chat & Voice Assistant" id="settings-scope-chat">
    <Row setting="scope-model">
      <Toggle
        bind:checked={scope.enabled}
        label="Its own model for Chat"
        desc="For Chat, the Ask bar on your notes and the Voice Assistant. Off, they use the cleanup model"
      />
    </Row>
    <LinkRow
      label="Voice, shortcut and spoken replies in Voice Assistant"
      desc="The Voice Assistant asks this model too, and reads its answers aloud"
      onclick={openVoiceAssistant}
    />
  </Group>

  {#if enabled}
    <Group title="Model" id="settings-scope-model" bare>
      <ScopeProviderConfig {scope} {cfg} scopeKey="chat" />
      <p class="set-note tools">
        Note tools need a bigger model: cloud models and local ones of 4B or more look up and change your notes;
        smaller ones answer from a plain search of them. Switch models from the chip under the chat box too.
      </p>
    </Group>

    <Group title="System prompt" id="settings-scope-prompt" desc="How the assistant answers" bare>
      <PromptStudio bind:prompt={scope.prompt} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
    </Group>
  {:else}
    <p class="set-note offnote">Turn it on to give Chat a model of its own.</p>
  {/if}
</div>

<style>
  .scope {
    display: flex;
    flex-direction: column;
  }
  .offnote {
    margin: -18px 0 30px;
  }
  .tools {
    margin-top: 12px;
  }
</style>
