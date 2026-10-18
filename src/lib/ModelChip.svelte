<script>
  // The Chat view's model chip: the model the next message goes to, resolved
  // the way chat_send resolves it (ai_model.rs: the Chat scope, else the
  // Dictation Cleanup endpoint, the on-device model included), and whether
  // it can use Yap's note tools ("· no note tools": a local model under 4B
  // answers from a plain search of the notes and can't change them).
  //
  // Click → a popover: the model and where it runs, a quick switch between
  // the models of every provider with a key and the installed on-device
  // model (chat_set_model: the Chat scope only, saved like Settings saves;
  // Settings adopts it from `yap-chat-model-changed`), and "Language model
  // settings…". With no AI set up and nothing to switch to, the chip opens
  // Settings → Language Models instead.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import { modelName, providerName, modelIcon, whereItRuns, switchGroups, noLongerOffered } from './aiModel.js';
  import { loadCatalog } from './modelCatalog.svelte.js';
  import { toast } from './ui/toast.svelte.js';

  let { onopensettings = null } = $props();

  let status = $state(null);
  let open = $state(false);
  let switching = $state(null); // the key being switched to
  let wrapEl = $state(null);
  let chipEl = $state(null);
  let popEl = $state(null);

  const model = $derived(status?.ready ? status.model : null);
  const icon = $derived(modelIcon(model));
  const groups = $derived(switchGroups(status));
  const canSwitch = $derived(groups.some((g) => g.items.some((i) => !i.fixed)));
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
  const chipLabel = $derived(
    !status
      ? ''
      : !model
        ? 'No AI model. Set one up in Language Models'
        : `Chat model: ${modelName(model)}, ${providerName(model)}.${status.tools ? '' : ` ${noToolsLine}`}${gone ? ` ${GONE}` : ''} Change model`
  );

  export async function refresh() {
    try {
      status = await invoke('ai_model_status', { scope: 'chat' });
    } catch {
      /* keep the last one */
    }
  }

  function openSettings() {
    open = false;
    onopensettings?.('cleanup#chat');
  }

  async function onChip() {
    if (open) {
      open = false;
      return;
    }
    // The providers' live lists too (a day-old one is checked behind it).
    await Promise.all([refresh(), loadCatalog()]);
    if (!status?.ready && !canSwitch) {
      openSettings();
      return;
    }
    open = true;
    await tick();
    (
      popEl?.querySelector('[aria-checked="true"]:not(:disabled)') || popEl?.querySelector('button:not(:disabled)')
    )?.focus();
  }

  async function pick(item) {
    if (switching) return;
    if (item.current || item.fixed) {
      close();
      return;
    }
    switching = item.key;
    try {
      status = await invoke('chat_set_model', { provider: item.provider, model: item.model });
      close();
    } catch (e) {
      toast({ title: "Couldn't switch the model", description: String(e), variant: 'destructive' });
    } finally {
      switching = null;
    }
  }

  function close() {
    open = false;
    chipEl?.focus();
  }

  function onWindowClick(e) {
    if (open && wrapEl && !wrapEl.contains(e.target)) open = false;
  }
  function onKeydown(e) {
    if (open && e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
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

<svelte:window onclick={onWindowClick} />

{#snippet glyph(kind)}
  {#if kind === 'pc'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><rect x="9" y="9" width="6" height="6" /><path d="M9 1v3M15 1v3M9 20v3M15 20v3M1 9h3M1 15h3M20 9h3M20 15h3" /></svg>
  {:else if kind === 'none'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 8v4.5M12 16h.01" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.8 4.9L19 9.7l-4.2 3.1L16.4 18 12 15l-4.4 3 1.6-5.2L5 9.7l5.2-1.8z" /></svg>
  {/if}
{/snippet}

{#snippet mark(m, size)}
  {@const ic = modelIcon(m)}
  <span class="mark" style="--s: {size}px" data-icon={ic?.id ?? (m?.onThisPc ? 'pc' : 'generic')}>
    {#if ic}
      <img src={ic.src} alt="" class:mono={ic.mono} />
    {:else}
      {@render glyph(m?.onThisPc ? 'pc' : 'spark')}
    {/if}
  </span>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modelchip" bind:this={wrapEl} onkeydown={onKeydown}>
  {#if status}
    <button
      class="chip"
      class:none={!model}
      bind:this={chipEl}
      aria-label={chipLabel}
      aria-haspopup={canSwitch || model ? 'dialog' : undefined}
      aria-expanded={open}
      title={model ? `${whereItRuns(model, status.host)}${status.tools ? '' : `\n${noToolsLine}`}${gone ? `\n${GONE}` : ''}` : status.error || ''}
      onclick={onChip}
    >
      {#if model}
        {@render mark(model, 13)}
        <span class="cname">{modelName(model)}</span>
        <span class="cprov">· {providerName(model)}</span>
        {#if !status.tools}<span class="cnotools">· no note tools</span>{/if}
        {#if gone}<span class="cnotools">· no longer offered</span>{/if}
        <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 15 6-6 6 6" /></svg>
      {:else}
        <span class="mark none" style="--s: 13px">{@render glyph('none')}</span>
        <span class="cname">No AI model</span>
      {/if}
    </button>
  {/if}

  {#if open && status}
    <div class="pop" role="dialog" aria-label="Chat model" bind:this={popEl}>
      {#if model}
        <div class="head">
          {@render mark(model, 18)}
          <div class="htext">
            <div class="hname">{modelName(model)}</div>
            <div class="hwhere">{providerName(model)} · {whereItRuns(model, status.host)}</div>
            {#if status.tools}
              <p class="hnote">Looks up and changes your notes with Yap's note tools.</p>
            {:else}
              <p class="hnote warn">{noToolsLine}</p>
            {/if}
            {#if gone}
              <p class="hnote warn" role="alert">{GONE}</p>
            {/if}
            {#if !status.fromScope}
              <p class="hnote">The same model as Dictation Cleanup, until you pick one for Chat.</p>
            {/if}
          </div>
        </div>
      {:else}
        <div class="head">
          <span class="mark none" style="--s: 18px">{@render glyph('none')}</span>
          <div class="htext">
            <div class="hname">No AI model</div>
            <p class="hnote">{status.error}</p>
          </div>
        </div>
      {/if}

      <div class="sep"></div>
      {#if canSwitch}
        <div class="seclabel" id="chat-model-switch">Switch the Chat model</div>
        <div class="list" role="radiogroup" aria-labelledby="chat-model-switch">
          {#each groups as g (g.id)}
            <div class="glabel">{g.label}</div>
            {#each g.items as it (it.key)}
              <button
                class="opt"
                class:current={it.current}
                role="radio"
                aria-checked={it.current}
                disabled={!!switching || it.fixed}
                onclick={() => pick(it)}
              >
                <span class="check" aria-hidden="true">
                  {#if it.current}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12 5 5 9-10" /></svg>
                  {:else if switching === it.key}
                    <span class="spin"></span>
                  {/if}
                </span>
                {@render mark({ provider: it.provider, id: it.model, name: it.provider === 'ondevice' ? it.label : '', onThisPc: it.provider === 'ondevice' }, 13)}
                <span class="oname">{it.label}</span>
                {#if it.tag === 'new'}<span class="otag new">New</span>{/if}
                {#if it.tag === 'gone'}<span class="otag">no longer offered</span>{/if}
                {#if !it.tools}<span class="otag">no note tools</span>{/if}
              </button>
            {/each}
          {/each}
        </div>
        <p class="foot">Changes the model for Chat only. Dictation cleanup stays as it is.</p>
      {:else}
        <p class="foot">Add an API key, or install Yap's on-device model, to switch models here.</p>
      {/if}
      <div class="sep"></div>
      <button class="settings" onclick={openSettings}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7h-9M14 17H5" /><circle cx="17" cy="17" r="3" /><circle cx="7" cy="7" r="3" /></svg>
        Language model settings…
      </button>
    </div>
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
    gap: 5px;
    min-width: 0;
    max-width: 100%;
    height: 26px;
    padding: 0 8px 0 7px;
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
  .chip:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .chip.none {
    color: var(--yap-warning);
    border-color: color-mix(in srgb, var(--yap-warning) 35%, var(--yap-border-subtle));
  }
  .cname {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
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

  .mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--s);
    height: var(--s);
    flex: 0 0 auto;
    color: var(--yap-muted);
  }
  .mark img,
  .mark svg {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  :global([data-yap-theme='dark']) .mark img.mono {
    filter: invert(1);
  }
  .mark.none {
    color: var(--yap-warning);
  }

  .pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 30;
    width: 300px;
    max-width: calc(100vw - 40px);
    padding: 6px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
  }
  .head {
    display: flex;
    gap: 10px;
    padding: 8px 8px 6px;
  }
  .head .mark {
    margin-top: 1px;
  }
  .htext {
    min-width: 0;
  }
  .hname {
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .hwhere {
    margin-top: 1px;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .hnote {
    margin: 6px 0 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .hnote.warn {
    color: var(--yap-warning);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
  .seclabel {
    padding: 6px 8px 2px;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .list {
    max-height: 240px;
    overflow-y: auto;
  }
  .glabel {
    padding: 8px 8px 3px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .opt:hover:not(:disabled),
  .opt:focus-visible {
    background: var(--yap-s3);
    outline: none;
  }
  .opt.current {
    background: var(--yap-primary-wash);
    font-weight: 600;
  }
  .opt:disabled {
    cursor: default;
  }
  .check {
    display: inline-flex;
    width: 12px;
    height: 12px;
    flex: 0 0 auto;
    color: var(--yap-primary);
  }
  .check svg {
    width: 12px;
    height: 12px;
  }
  .oname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .otag {
    flex: 0 0 auto;
    font-size: 10.5px;
    font-weight: 400;
    color: var(--yap-warning);
  }
  .otag.new {
    padding: 0 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-tint);
    color: var(--yap-primary);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
  }
  .foot {
    margin: 4px 8px 6px;
    font-size: 10.5px;
    line-height: 1.5;
    color: var(--yap-muted-55);
  }
  .settings {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    text-align: left;
    cursor: pointer;
  }
  .settings:hover,
  .settings:focus-visible {
    background: var(--yap-s3);
    color: var(--yap-fg);
    outline: none;
  }
  .settings svg {
    width: 12px;
    height: 12px;
    color: var(--yap-muted-55);
  }
  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
