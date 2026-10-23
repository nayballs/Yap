<script>
  // The Chat view's model chip: the model the next message goes to,
  // resolved the way chat_send resolves it (ai_model.rs: the Chat scope,
  // else the Dictation Cleanup endpoint, the on-device model included) —
  // the provider's mark, the model, "· Groq" / "· On this PC" — and its
  // warnings: "no note tools" (a local model under 4B, or one its provider
  // says can't call tools: answers come from a plain search of the notes)
  // and "no longer offered" (the provider's live list dropped it).
  //
  // Click → ChatModelPicker.svelte, "Switch the Chat model": a provider
  // rail with every provider's models, connecting a provider in place, and
  // On this PC (Yap's on-device models, downloaded right there, and
  // self-hosted servers). It changes the Chat scope only (chat_set_model,
  // saved like Settings saves; Settings adopts it from
  // `yap-chat-model-changed`). With no AI set up it opens too: connecting
  // one is right there.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import ProviderMark from './ProviderMark.svelte';
  import ChatModelPicker from './ChatModelPicker.svelte';
  import { modelName, providerName, modelIcon, whereItRuns, noLongerOffered } from './aiModel.js';
  import { loadCatalog } from './modelCatalog.svelte.js';
  import { refreshLocal, local, pushRecent } from './chatModels.svelte.js';
  import { chipContextLine } from './localContext.js';

  // `quiet`: no pill, just the words (under the Chat composer, as Claude
  // Code shows its model).
  let { onopensettings = null, quiet = false } = $props();

  let status = $state(null);
  let open = $state(false);
  let wrapEl = $state(null);
  let chipEl = $state(null);

  const model = $derived(status?.ready ? status.model : null);
  // The provider's live list no longer has the chat model (modelCatalog.js).
  const gone = $derived(noLongerOffered(model));
  const GONE = $derived(gone ? `${providerName(model)} no longer offers ${modelName(model)}. Pick another model.` : '');
  const NO_TOOLS =
    "It's too small for Yap's note tools: answers come from a plain search of your notes, and it can't change them.";
  const NO_TOOLS_SAID = "Its provider says it can't use Yap's note tools: answers come from a plain search of your notes.";
  // A cloud model without note tools is one its provider said can't call them.
  const noToolsLine = $derived(
    model && ['groq', 'anthropic', 'openai', 'gemini', 'openrouter'].includes(model.provider) ? NO_TOOLS_SAID : NO_TOOLS
  );
  // The on-device model's context ("Context: 8K tokens"), in the tooltip.
  const ctxLine = $derived(model?.provider === 'ondevice' ? chipContextLine(status?.context) : '');
  const chipLabel = $derived(
    !status
      ? ''
      : !model
        ? 'No AI model. Set one up'
        : `Chat model: ${modelName(model)}, ${providerName(model)}.${status.tools ? '' : ` ${noToolsLine}`}${gone ? ` ${GONE}` : ''} Change model`
  );

  export async function refresh() {
    try {
      status = await invoke('ai_model_status', { scope: 'chat' });
    } catch {
      /* keep the last one */
    }
  }

  /** What Recent keeps of the model in use (so it lists it too). */
  function currentEntry() {
    if (!model) return null;
    if (model.provider === 'ondevice') {
      const st = local.status;
      return { provider: 'ondevice', model: '', file: st?.activeFile || st?.modelFile || '', baseUrl: '' };
    }
    if (model.provider === 'local' || model.provider === 'custom') {
      const s = (status.servers || []).find((x) => x.provider === model.provider && x.host === status.host);
      return s ? { provider: model.provider, model: model.id, file: '', baseUrl: s.baseUrl } : null;
    }
    return { provider: model.provider, model: model.id, file: '', baseUrl: '' };
  }

  async function onChip() {
    if (open) {
      close(true);
      return;
    }
    // The providers' live lists too (a day-old one is checked behind it),
    // and the on-device models on this PC.
    await Promise.all([refresh(), loadCatalog(), refreshLocal()]);
    const cur = currentEntry();
    if (cur && (cur.provider !== 'ondevice' || cur.file)) pushRecent(cur);
    open = true;
  }

  function close(focusChip) {
    open = false;
    if (focusChip) tick().then(() => chipEl?.focus());
  }

  /** Open the picker (Chat's "Choose another model…" for a screenshot). */
  export function openPicker() {
    if (!open) onChip();
  }

  // Outside the chip and its picker: closed. On pointerdown (the path is
  // taken as the event starts, so a row the click re-renders still counts
  // as inside).
  function onWindowPointer(e) {
    if (open && wrapEl && !e.composedPath().includes(wrapEl)) close(false);
  }

  onMount(() => {
    refresh();
    // A switch here or in another window, the sidecar coming up, and any
    // save from Settings: read it again.
    const uns = [listen('yap-chat-model-changed', refresh), listen('yap-config-saved', refresh)];
    loadCatalog();
    return () => uns.forEach((u) => u.then((f) => f()));
  });
</script>

<svelte:window onpointerdowncapture={onWindowPointer} />

<div class="modelchip" bind:this={wrapEl}>
  {#if status}
    <button
      class="chip"
      class:quiet
      class:none={!model}
      bind:this={chipEl}
      aria-label={chipLabel}
      aria-haspopup="dialog"
      aria-expanded={open}
      title={model ? `${whereItRuns(model, status.host)}${ctxLine ? `\n${ctxLine}` : ''}${status.tools ? '' : `\n${noToolsLine}`}${gone ? `\n${GONE}` : ''}` : status.error || ''}
      onclick={onChip}
    >
      {#if model}
        <ProviderMark icon={modelIcon(model)} glyph={model.onThisPc ? 'pc' : 'spark'} size={14} />
        <span class="cname">{modelName(model)}</span>
        <span class="cprov">· {providerName(model)}</span>
        {#if !status.tools}<span class="cnotools">· no note tools</span>{/if}
        {#if gone}<span class="cnotools">· no longer offered</span>{/if}
      {:else}
        <span class="nonemark"><ProviderMark glyph="none" size={14} /></span>
        <span class="cname">No AI model</span>
      {/if}
      <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 15 6-6 6 6" /></svg>
    </button>
  {/if}

  {#if open && status}
    <ChatModelPicker
      {status}
      anchor={chipEl}
      onclose={close}
      onstatus={(s) => (status = s)}
      {onopensettings}
    />
  {/if}
</div>

<style>
  .modelchip {
    position: relative;
    min-width: 0;
    display: flex;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    height: 26px;
    padding: 0 8px 0 6px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    transition:
      border-color var(--yap-dur),
      background var(--yap-dur);
  }
  .chip:hover,
  .chip[aria-expanded='true'] {
    border-color: var(--yap-border-hover);
    background: var(--yap-s3);
  }
  .chip.quiet {
    border-color: transparent;
    background: none;
    color: var(--yap-muted);
  }
  .chip.quiet .cname {
    font-weight: 500;
    color: var(--yap-fg-80);
  }
  .chip.quiet:hover,
  .chip.quiet[aria-expanded='true'] {
    border-color: transparent;
    background: var(--yap-s3);
  }
  .chip:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  .chip.none {
    color: var(--yap-warning);
    border-color: color-mix(in srgb, var(--yap-warning) 35%, var(--yap-border-subtle));
  }
  .nonemark {
    display: inline-flex;
    --pm-glyph: var(--yap-warning);
  }
  .cname {
    font-weight: 600;
    color: var(--yap-fg);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip.none .cname {
    color: var(--yap-warning);
  }
  .cprov {
    color: var(--yap-muted);
    flex: 0 0 auto;
  }
  .cnotools {
    color: var(--yap-warning);
    flex: 0 0 auto;
  }
  .chev {
    width: 11px;
    height: 11px;
    flex: 0 0 auto;
    color: var(--yap-muted-70);
  }
</style>
