<script>
  // Prompt Studio, ported from OpenWhispr's ui/PromptStudio.tsx: one card with
  // View / Customize / Test tabs for the cleanup system prompt. Adapted to
  // Yap's split-prompt design — the immutable guardrails (llm::BASE_PROMPT,
  // fetched via get_base_prompt) are always prepended to the editable body, so
  // View shows the full effective prompt while Customize edits only the body.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Textarea from './ui/Textarea.svelte';
  import Select from './ui/Select.svelte';

  // Generalized so every Language-Models scope reuses one Prompt Studio. The
  // parent binds the editable body (`prompt`) and passes the scope's immutable
  // guardrails (`basePrompt`) + `defaultBody`. `presets`/`testCommand` are
  // cleanup-only (optional): [] hides the preset dropdown, null marks live Test
  // as not-yet-available (Note/Chat). `enabled` gates the Test tab; `onpersist`
  // saves the config before a test; `ontested` fires after (usage refresh).
  let {
    prompt = $bindable(''),
    preset = $bindable('custom'),
    presets = [],
    basePrompt = '',
    defaultBody = '',
    providerLabel = '',
    modelLabel = '',
    enabled = true,
    testCommand = null,
    testSample = 'um so like i think we should uh go to the the bank tomorrow',
    onpersist,
    ontested,
  } = $props();

  const hasPresets = $derived(presets.length > 0);

  let activeTab = $state('view'); // view | edit | test
  // Snapshots of the saved prompt/preset for the Customize tab; re-synced on
  // mount and every time the tab is entered (see the tab button).
  let editedPrompt = $state('');
  let editedPreset = $state('custom');
  let savedNote = $state(false);
  let copied = $state(false);
  let testText = $state(testSample);
  let testResult = $state('');
  let testRunning = $state(false);

  // The immutable guardrails (basePrompt) are always part of the effective
  // prompt. OpenWhispr-style, View AND Customize show the *full* prompt, so
  // they always match. `compose` prepends the guardrails to a tone body but
  // never doubles them (a body that already carries them is returned as-is —
  // the same idempotence the backend's build_system_prompt() enforces).
  function compose(body) {
    if (!basePrompt) return body;
    return body.startsWith(basePrompt) ? body : `${basePrompt}\n\n${body}`;
  }
  const fullDefault = $derived(compose(defaultBody));
  const currentPrompt = $derived(compose(prompt));
  const isCustomPrompt = $derived(currentPrompt !== fullDefault);

  onMount(() => {
    editedPreset = preset ?? 'custom';
    editedPrompt = compose(prompt);
  });

  function onPresetPick(v) {
    editedPreset = v;
    const p = presets.find((x) => x.value === v);
    if (p && p.body != null) editedPrompt = compose(p.body);
  }
  function onEditInput(v) {
    editedPrompt = v;
    if (hasPresets && editedPreset !== 'custom') editedPreset = 'custom';
  }
  function savePrompt() {
    prompt = editedPrompt;
    if (hasPresets) preset = editedPrompt === fullDefault ? 'default' : editedPreset;
    savedNote = true;
    setTimeout(() => (savedNote = false), 2500);
  }
  function resetToDefault() {
    editedPrompt = fullDefault;
    editedPreset = 'default';
    prompt = fullDefault;
    if (hasPresets) preset = 'default';
  }
  function copyText(text) {
    navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  async function runTest() {
    if (!testText.trim() || testRunning || !enabled || !testCommand) return;
    testRunning = true;
    testResult = '';
    // Test what's being edited (OpenWhispr semantics): apply the edited prompt,
    // run, then restore the saved one.
    const previousPrompt = prompt;
    const previousPreset = preset;
    prompt = editedPrompt;
    await onpersist?.();
    try {
      testResult = await invoke(testCommand, { text: testText });
    } catch (e) {
      testResult = `Test failed: ${e}`;
    } finally {
      prompt = previousPrompt;
      if (hasPresets) preset = previousPreset;
      await onpersist?.();
      testRunning = false;
      ontested?.();
    }
  }
</script>

<div class="studio">
  <div class="tabs">
    <button class="tab" class:active={activeTab === 'view'} onclick={() => (activeTab = 'view')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M2 12s3.5-6.5 10-6.5S22 12 22 12s-3.5 6.5-10 6.5S2 12 2 12z" /><circle cx="12" cy="12" r="2.8" />
      </svg>
      View
    </button>
    <button class="tab" class:active={activeTab === 'edit'} onclick={() => { editedPrompt = compose(prompt); editedPreset = preset ?? 'custom'; activeTab = 'edit'; }}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M17 3a2.8 2.8 0 0 1 4 4L8 20l-5 1 1-5z" />
      </svg>
      Customize
    </button>
    <button class="tab" class:active={activeTab === 'test'} onclick={() => (activeTab = 'test')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M9 3h6M10 3v6.3L4.6 18a2 2 0 0 0 1.7 3h11.4a2 2 0 0 0 1.7-3L14 9.3V3" />
      </svg>
      Test
    </button>
  </div>

  {#if activeTab === 'view'}
    <div class="pane">
      <div class="pane-head">
        <span class="caps-row">
          <span class="caps">{isCustomPrompt ? 'Custom prompt' : 'Default prompt'}</span>
          {#if isCustomPrompt}<span class="chip">Modified</span>{/if}
        </span>
        <button class="ghost" onclick={() => copyText(currentPrompt)}>
          {#if copied}
            <svg class="ok" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
            Copied
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>
            Copy
          {/if}
        </button>
      </div>
      <div class="promptbox">
        <pre>{currentPrompt}</pre>
      </div>
    </div>
  {:else if activeTab === 'edit'}
    <div class="pane hairline">
      <p class="caution">
        <span class="warn">Caution</span> This is the full prompt, exactly as shown in View.
        Editing it may affect transcription quality. The safety rules up top (clean the text,
        never answer it) are always enforced even if you change them.
      </p>
    </div>
    <div class="pane hairline">
      {#if hasPresets}
        <div class="preset-row">
          <span class="caps">Preset</span>
          <Select value={editedPreset} options={presets} onchange={onPresetPick} />
        </div>
      {/if}
      <Textarea value={editedPrompt} oninput={onEditInput} rows={16} placeholder="Enter your custom system prompt..." />
    </div>
    <div class="pane hairline">
      <div class="btnrow">
        <button class="primary" onclick={savePrompt}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" /><path d="M17 21v-8H7v8M7 3v5h8" /></svg>
          Save
        </button>
        <button class="outline" onclick={resetToDefault}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" /></svg>
          Reset
        </button>
      </div>
      {#if savedNote}
        <p class="savednote">Prompt Saved — your custom prompt will be used for all future AI processing.</p>
      {/if}
    </div>
  {:else}
    <div class="pane hairline">
      <div class="meta-row">
        <span class="caps">Model</span>
        <span class="meta-val mono">{modelLabel || 'None'}</span>
        <span class="vr"></span>
        <span class="caps">Provider</span>
        <span class="meta-val">{providerLabel || 'None'}</span>
      </div>
    </div>
    {#if !testCommand}
      <div class="pane hairline">
        <div class="warnbox">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
          <p>Live testing for this mode arrives with the feature — your prompt is saved and used once it ships.</p>
        </div>
      </div>
    {:else}
      {#if !enabled}
        <div class="pane hairline">
          <div class="warnbox">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 2 20h20L12 3z" /><path d="M12 9v5M12 17.5h.01" /></svg>
            <p>This mode is disabled. Enable it above to test prompts.</p>
          </div>
        </div>
      {/if}
      <div class="pane hairline">
        <div class="pane-head">
          <span class="lbl">Input</span>
        </div>
        <Textarea bind:value={testText} rows={3} placeholder="Enter text to test..." />
      </div>
      <div class="pane hairline">
        <button class="primary wide" onclick={runTest} disabled={!testText.trim() || testRunning || !enabled}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4l14 8-14 8V4z" /></svg>
          {testRunning ? 'Processing...' : 'Run Test'}
        </button>
      </div>
    {/if}
    {#if testResult}
      <div class="pane hairline">
        <div class="pane-head">
          <span class="lbl">Output</span>
          <button class="ghost" onclick={() => copyText(testResult)} aria-label="Copy output">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15V5a2 2 0 0 1 2-2h10" /></svg>
          </button>
        </div>
        <div class="promptbox out">
          <pre>{testResult}</pre>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .studio {
    width: 100%;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s2);
    overflow: hidden;
  }
  .tabs {
    display: flex;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .tab {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 10px 14px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .tab svg {
    width: 13px;
    height: 13px;
  }
  .tab:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .tab.active {
    border-bottom-color: var(--yap-primary);
    color: var(--yap-fg);
    background: color-mix(in srgb, var(--yap-primary) 5%, transparent);
  }

  .pane {
    padding: 14px 18px;
  }
  .pane.hairline + .pane.hairline {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .pane-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 9px;
  }
  .caps-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .caps {
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .lbl {
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .chip {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-primary);
    background: var(--yap-primary-wash);
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
  }
  .chip.dim {
    color: var(--yap-muted);
    background: var(--yap-raised);
  }

  .ghost {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: none;
    background: none;
    padding: 4px 8px;
    border-radius: var(--yap-r-sm);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .ghost:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .ghost svg {
    width: 12px;
    height: 12px;
  }
  .ghost .ok {
    color: var(--yap-success);
  }

  .promptbox {
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    padding: 13px 15px;
    max-height: 320px;
    overflow-y: auto;
  }
  .promptbox pre {
    margin: 0;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    line-height: 1.65;
    color: var(--yap-muted);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .promptbox.out {
    max-height: 190px;
  }
  .promptbox.out pre {
    color: var(--yap-fg);
    font-family: inherit;
    font-size: 12px;
  }

  .caution {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .warn {
    font-weight: 600;
    color: var(--yap-warning);
  }
  .preset-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .preset-row :global(select) {
    max-width: 220px;
  }

  .btnrow {
    display: flex;
    gap: 8px;
  }
  .primary,
  .outline {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    height: 30px;
    padding: 0 14px;
    border-radius: var(--yap-r);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease,
      transform var(--yap-dur) ease;
  }
  .primary:active,
  .outline:active {
    transform: scale(0.985);
  }
  .primary {
    flex: 1;
    border: none;
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
  }
  .primary:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .primary.wide {
    width: 100%;
  }
  .primary svg,
  .outline svg {
    width: 13px;
    height: 13px;
  }
  .outline {
    border: 1px solid var(--yap-border);
    background: transparent;
    color: var(--yap-fg-80);
  }
  .outline:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .savednote {
    margin: 9px 0 0;
    font-size: 11.5px;
    color: var(--yap-success);
  }

  .warnbox {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 30%, transparent);
    background: color-mix(in srgb, var(--yap-warning) 7%, transparent);
    border-radius: var(--yap-r-lg);
    padding: 11px 13px;
  }
  .warnbox svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
    margin-top: 1px;
    color: var(--yap-warning);
  }
  .warnbox p {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.55;
  }

  .meta-row {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .meta-val {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--yap-fg);
  }
  .meta-val.mono {
    font-family: ui-monospace, Consolas, monospace;
  }
  .vr {
    width: 1px;
    height: 12px;
    background: var(--yap-border);
    margin: 0 4px;
  }
</style>
