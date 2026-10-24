<script>
  // Language Models → **Commands** tab (the `voiceAgent` scope; called "Voice
  // Agent" until 2026-10-18), ported from OpenWhispr's DictationAgentSettings.tsx.
  // Lets you dictate *instructions* to the AI by saying its name ("Hey
  // <agentName>, …") or with the rewrite shortcut — it executes the command
  // (compose / reformat / answer) and types the result instead of your words.
  // Not the Voice Assistant (answers out loud: the Chat & Voice Assistant
  // tab). Section order mirrors OpenWhispr:
  //   enable toggle → provider/model config → Commands (its name and shortcut:
  //   a link to Settings → Commands) → How it works → Examples → Agent prompt.
  //
  //   scope: the reactive { enabled, provider, baseUrl, model, apiKey, apiKeys,
  //          prompt } object for cfg.llmScopes.voiceAgent (parent seeds it).
  //   cfg:   the Settings config $state — used for cfg.agentName + cfg.dictionary.
  import ScopeProviderConfig from './ScopeProviderConfig.svelte';
  import PromptStudio from './PromptStudio.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { SCOPE_PROVIDER_LABELS } from '../core/providers.js';

  let { cfg, scope, defaultPrompt = '' } = $props();

  // The edit/rewrite guardrails (llm::EDIT_BASE_PROMPT) shown in the Agent prompt
  // Prompt Studio's View tab — always applied on top of the editable agent body.
  let editBase = $state('');
  onMount(async () => {
    try {
      editBase = await invoke('get_edit_base_prompt');
    } catch {
      /* older backend — View shows the body alone */
    }
  });
  const providerLabel = $derived(SCOPE_PROVIDER_LABELS[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'The on-device model' : scope.model);

  // The wake word for all display copy. Falls back to a placeholder ("Yap") when
  // the user hasn't named the agent yet — display only; nothing is persisted.
  const displayName = $derived((cfg.agentName || '').trim() || 'Yap');

  const enabled = $derived(scope.enabled);

  const examples = $derived([
    `Hey ${displayName}, write a formal email about the budget`,
    `Hey ${displayName}, make this more professional`,
    `Hey ${displayName}, convert this to bullet points`,
  ]);

  // The name and the rewrite shortcut live in Settings → Commands; this tab
  // is their model and prompt.
  function openCommands() {
    window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'commands' }));
  }
</script>

<div class="voiceagent">
  <Group title="Commands" id="settings-scope-commands">
    <Row setting="scope-model">
      <Toggle
        bind:checked={scope.enabled}
        label="Answer to its name in any dictation"
        desc={`Say "${displayName}, …" and this model does what you ask. The same switch as on the Commands page`}
      />
    </Row>
    <LinkRow label="Name and shortcut in Commands" value={`Agent name: ${displayName}`} onclick={openCommands} />
  </Group>

  {#if enabled}
    <Group title="Model" id="settings-scope-model" bare>
      <ScopeProviderConfig {scope} {cfg} scopeKey="voiceAgent" />
    </Group>
  {:else}
    <!-- Off: one line instead of a screen of greyed-out choices. -->
    <p class="set-note offnote">Turn on Commands to pick the model that carries them out.</p>
  {/if}

  <Group title="Examples" id="settings-scope-examples" desc={`Say "Hey ${displayName}" and an instruction: the AI writes the result and types it where you are, instead of your words. To hear an answer instead, use the Voice Assistant.`}>
    {#each examples as example}
      <div class="example">
        <span class="badge">Instruction</span>
        <p class="body">"{example}"</p>
      </div>
    {/each}
  </Group>

  {#if enabled}
    <Group title="Agent prompt" id="settings-scope-prompt" desc="What the model is told when you say its name. Its built-in default until you change it" bare>
      <PromptStudio bind:prompt={scope.prompt} basePrompt={editBase} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
    </Group>
  {/if}
</div>

<style>
  .voiceagent {
    display: flex;
    flex-direction: column;
  }
  .offnote {
    margin: -18px 0 30px;
  }
  .example {
    display: flex;
    align-items: flex-start;
    gap: 11px;
    padding: 11px 0;
  }
  .body {
    margin: 0;
    font-size: 13px;
    color: var(--yap-fg-80);
    line-height: 1.5;
  }
  .badge {
    flex: 0 0 auto;
    margin-top: 2px;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-link);
    background: var(--yap-primary-wash);
    padding: 2px 7px;
    border-radius: var(--yap-r-sm);
  }
</style>
