<script>
  // Note Formatting scope config — ported from OpenWhispr's SettingsPage.tsx
  // `renderNoteFormatting` / `NoteFormattingSettings` (grep "renderNoteFormatting").
  // OpenWhispr's tab is: an "Auto-generate note titles" toggle → an
  // <InferenceConfigEditor scope="noteFormatting" /> (provider/model + prompt) →
  // a PromptStudio. In Yap this model writes the meeting summaries (the action
  // plan, digests) and runs the Actions on a note (Enhance); meeting titles
  // come from it too, so there's no separate titles switch.
  //
  //   scope: the reactive { enabled, provider, baseUrl, model, apiKey, apiKeys,
  //          prompt } object (cfg.llmScopes['noteFormatting'], parent-seeded).
  import ScopeProviderConfig from './ScopeProviderConfig.svelte';
  import PromptStudio from './PromptStudio.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Row from '../ui/Row.svelte';
  import Group from '../ui/Group.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { SCOPE_PROVIDER_LABELS } from '../core/providers.js';

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
  const providerLabel = $derived(SCOPE_PROVIDER_LABELS[scope.provider] || scope.provider);
  const modelLabel = $derived(scope.provider === 'ondevice' ? 'The on-device model' : scope.model);
</script>

<div class="nf">
  <Group title="Note formatting" id="settings-scope-notes">
    <Row setting="scope-model">
      <Toggle
        bind:checked={scope.enabled}
        label="Its own model for notes"
        desc="Meeting summaries and Enhance on a note. Off, they use the cleanup model"
      />
    </Row>
  </Group>

  {#if enabled}
    <Group title="Model" id="settings-scope-model" bare>
      <ScopeProviderConfig {scope} {cfg} scopeKey="noteFormatting" />
    </Group>

    <Group title="Formatting prompt" id="settings-scope-prompt" desc="How notes are shaped when enhanced. The rules in View always apply" bare>
      <PromptStudio bind:prompt={scope.prompt} basePrompt={noteBase} defaultBody={defaultPrompt} {providerLabel} {modelLabel} />
    </Group>
  {:else}
    <p class="set-note offnote">Turn it on to give notes and meeting summaries a model of their own.</p>
  {/if}
</div>

<style>
  .nf {
    display: flex;
    flex-direction: column;
  }
  .offnote {
    margin: -18px 0 30px;
  }
</style>
