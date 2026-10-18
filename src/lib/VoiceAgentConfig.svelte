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
  import Toggle from './ui/Toggle.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

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
  const PROVIDER_NAMES = {
    groq: 'Groq', anthropic: 'Anthropic', openai: 'OpenAI', gemini: 'Gemini', openrouter: 'OpenRouter',
    ondevice: 'Built-in local AI', local: 'Self-hosted', custom: 'Custom',
  };
  const providerLabel = $derived(PROVIDER_NAMES[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'Built-in model' : scope.model);

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

{#snippet sectionHeader(title, desc)}
  <div class="sechead">
    <h3>{title}</h3>
    {#if desc}<p>{desc}</p>{/if}
  </div>
{/snippet}

<div class="voiceagent">
  <!-- 1. Enable toggle -->
  <div class="panel">
    <Toggle
      bind:checked={scope.enabled}
      label="Enable Commands"
      desc={`Dictate instructions to the AI and it types the result. Activate by saying "${displayName}". Configurable below.`}
    />
  </div>

  <!-- 2. Provider / model config (dimmed + disabled when Commands are off) -->
  <div class="providercfg" class:off={!enabled}>
    <ScopeProviderConfig {scope} {cfg} disabled={!enabled} />
  </div>

  <!-- 3. Commands — the name and the rewrite shortcut live in Settings →
       Commands; this tab is their model and prompt. -->
  <section class="hairline">
    {@render sectionHeader(
      'Commands',
      'The model above carries them out. The name they answer to and the rewrite shortcut are on the Commands page'
    )}

    <div class="panelcard linkcard">
      <p class="body">Agent name: <strong>{displayName}</strong></p>
      <button class="golink" onclick={openCommands}>
        Name and shortcut in Commands
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
      </button>
    </div>
  </section>

  <!-- 4. How it works -->
  <section class="hairline">
    {@render sectionHeader('How it works', '')}
    <div class="panelcard">
      <p class="body">
        When you say "Hey {displayName}" followed by an instruction, the AI executes your
        command — composing content, formatting text, or answering questions — and types
        the result into the app you're in, instead of transcribing it verbatim. To hear an
        answer instead, use the Voice Assistant.
      </p>
    </div>
  </section>

  <!-- 5. Examples -->
  <section class="hairline">
    {@render sectionHeader('Examples', '')}
    <div class="panelcard">
      <div class="examples">
        {#each examples as example}
          <div class="example">
            <span class="badge">Instruction</span>
            <p class="body">"{example}"</p>
          </div>
        {/each}
      </div>
    </div>
  </section>

  <!-- 6. Agent prompt (only when enabled) -->
  {#if enabled}
    <section class="hairline">
      {@render sectionHeader(
        'Agent prompt',
        'The system prompt used when the wake word is detected. Falls back to the built-in default.'
      )}
      <PromptStudio bind:prompt={scope.prompt} basePrompt={editBase} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
    </section>
  {/if}
</div>

<style>
  .voiceagent {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .panel {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 16px;
  }

  .providercfg {
    transition: opacity var(--yap-dur) ease;
  }
  .providercfg.off {
    opacity: 0.55;
    pointer-events: none;
  }

  /* hairline-separated sections, matching LlmScopeConfig's visual weight */
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  section.hairline {
    border-top: 1px solid var(--yap-border-subtle);
    padding-top: 18px;
  }

  .sechead {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .sechead h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .sechead p {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }

  .panelcard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 16px;
  }
  .linkcard {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .linkcard strong {
    color: var(--yap-fg);
    font-family: ui-monospace, Consolas, monospace;
  }
  .golink {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
    padding: 6px 10px;
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
    border-color: var(--yap-border-hover);
  }
  .golink svg {
    width: 12px;
    height: 12px;
  }

  .body {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.6;
  }

  .examples {
    display: flex;
    flex-direction: column;
    gap: 11px;
  }
  .example {
    display: flex;
    align-items: flex-start;
    gap: 11px;
  }
  .badge {
    flex: 0 0 auto;
    margin-top: 1px;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-primary);
    background: var(--yap-primary-wash);
    padding: 2px 7px;
    border-radius: var(--yap-r-sm);
  }
</style>
