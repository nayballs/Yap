<script>
  // Note Formatting scope config — ported from OpenWhispr's SettingsPage.tsx
  // `renderNoteFormatting` / `NoteFormattingSettings` (grep "renderNoteFormatting").
  // OpenWhispr's tab is: an "Auto-generate note titles" toggle → an
  // <InferenceConfigEditor scope="noteFormatting" /> (provider/model + prompt) →
  // a PromptStudio. Yap has no notes surface yet, so this is the endpoint config
  // Yap CAN honour today (saved now, applied once the Notepad surface lands).
  //
  // We deliberately SKIP OpenWhispr's "Auto-generate note titles" toggle: it binds
  // to a note-title generation step that only exists inside a notes backend Yap
  // doesn't have yet — there is nothing for the flag to drive, so persisting a dead
  // toggle would just mislead. It returns with the Notepad surface (see ROADMAP).
  //
  //   scope: the reactive { enabled, provider, baseUrl, model, apiKey, apiKeys,
  //          prompt } object (cfg.llmScopes['noteFormatting'], parent-seeded).
  import ScopeProviderConfig from './ScopeProviderConfig.svelte';
  import PromptStudio from './PromptStudio.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Row from './ui/Row.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let { scope, cfg = null, defaultPrompt = '' } = $props();

  // The immutable note-enhancement guardrails (llm::NOTE_BASE_PROMPT) shown in
  // the Prompt Studio View tab — always prepended to the editable fragment.
  let noteBase = $state('');
  onMount(async () => {
    try {
      noteBase = await invoke('get_note_base_prompt');
    } catch {
      /* older backend — View shows the fragment alone */
    }
  });

  const enabled = $derived(scope.enabled);
  const PROVIDER_NAMES = {
    groq: 'Groq', anthropic: 'Anthropic', openai: 'OpenAI', gemini: 'Gemini', openrouter: 'OpenRouter',
    ondevice: 'Built-in local AI', local: 'Self-hosted', custom: 'Custom',
  };
  const providerLabel = $derived(PROVIDER_NAMES[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'Built-in model' : scope.model);
</script>

<div class="nf">
  <Row>
    <Toggle
      bind:checked={scope.enabled}
      label="Enable note formatting"
      desc="Turn dictation into clean, structured notes."
    />
  </Row>

  {#if enabled}
    <!-- Live: this scope powers the Notes surface's Enhance button. -->
    <div class="soon">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" />
      </svg>
      <p>
        This model runs when you hit <strong>Enhance</strong> on a note (main sidebar →
        <strong>Notes</strong>) — it turns raw notes into clean, structured markdown.
      </p>
    </div>

    <ScopeProviderConfig {scope} {cfg} disabled={!enabled} />

    <div class="sep"></div>

    <div class="sectionhead">
      <h4>Formatting prompt</h4>
      <p>How notes are shaped when enhanced. Guardrails (View tab) are always applied.</p>
    </div>
    <PromptStudio bind:prompt={scope.prompt} basePrompt={noteBase} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
  {/if}
</div>

<style>
  .nf {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .soon {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 11px 13px;
  }
  .soon svg {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
    margin-top: 1px;
    color: var(--yap-muted);
  }
  .soon p {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .soon strong {
    color: var(--yap-fg);
    font-weight: 600;
  }
  .sep {
    height: 1px;
    background: var(--yap-border-subtle);
    margin: 2px 0;
  }
  .sectionhead h4 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .sectionhead p {
    margin: 3px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.5;
  }
</style>
