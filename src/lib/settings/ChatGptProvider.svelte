<script>
  // Settings → Language Models → Cloud providers → ChatGPT (2026-11-02: the
  // OpenAI and ChatGPT tabs made one, as in Chat's model picker). Two ways to
  // OpenAI's models, the plan first:
  //   1. your ChatGPT plan (ChatGptSignIn.svelte: Sign in with ChatGPT, the
  //      provider "chatgpt"), no cost per message;
  //   2. an OpenAI API key (the provider "openai"), quieter, behind "Use an
  //      API key instead".
  // The scope is on one of them (`provider`), switched here (`onprovider`)
  // as the two tabs switched it: nothing under it changes (config, Rust,
  // usage and effort keep both ids). What shows:
  //   - nothing connected: the plan's card, and "Use an API key instead"
  //     opening the key field below it; no model list;
  //   - the plan only: its card (Connected · Using ChatGPT plan), the
  //     account's models, "Use an OpenAI API key instead…" at the bottom;
  //   - a key only: the key, OpenAI's models, "Use your ChatGPT plan
  //     instead…" at the bottom (it signs in);
  //   - both: "Your plan | API key" on top, on the way the scope uses.
  // Signed in while the scope waits for an OpenAI key it hasn't got: it
  // goes on the plan. Shared by Dictation Cleanup (LanguageModelsPage) and
  // every scope (ScopeProviderConfig).
  //
  //   provider:   'chatgpt' | 'openai', the scope's.
  //   onprovider: (p) => the scope put on p (its key and model follow).
  //   apiKey:     the scope's active key, bound (an OpenAI key on 'openai').
  //   onkeyinput: the key written through to the shared store.
  //   openaiKey:  the OpenAI key the store has, whatever the scope is on.
  //   planKey:    the store's key for the plan (its marker while signed in),
  //               until chatgpt_status is in.
  //   model:      the scope's model, bound.
  //   keyEditing: the key field open (else masked), bound.
  //   keyBox:     the open key field's box (the cleanup banner's Add key).
  //   images:     "Reads images" in the model list (the Chat scope).
  import { onMount, untrack } from 'svelte';
  import ChatGptSignIn from './ChatGptSignIn.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import Input from '../ui/Input.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import { PP_CLOUD_MODELS } from '../core/ppModels.js';
  import { CHATGPT, CHATGPT_PLAN_KEY, GPT_WAYS, maskKey } from '../core/providers.js';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import { chatgpt, loadChatgpt, signInChatgpt } from '../stores/chatgpt.svelte.js';

  let {
    provider,
    onprovider,
    apiKey = $bindable(''),
    onkeyinput = null,
    openaiKey = '',
    planKey = '',
    model = $bindable(''),
    keyEditing = $bindable(false),
    keyBox = $bindable(null),
    disabled = false,
    images = false,
  } = $props();

  onMount(loadChatgpt);

  const KEY_URL = PP_CLOUD_MODELS.openai.keyUrl;
  const planOn = $derived(chatgpt.loaded ? chatgpt.plan : planKey === CHATGPT_PLAN_KEY);
  const keyOn = $derived(!!String(openaiKey || '').trim());
  const onPlan = $derived(provider === CHATGPT);
  const connected = $derived(onPlan ? planOn : keyOn);
  const masked = $derived(maskKey(apiKey));
  // The plan's card: on the plan, or with nothing connected (it comes first).
  const planCard = $derived(onPlan || (!planOn && !keyOn));
  // "Use an API key instead": the connect card's second way, while the plan isn't in.
  const keyToggle = $derived(!planOn && (onPlan || !keyOn));

  function choose(p) {
    if (p === provider) return;
    onprovider(p);
    // A key still to paste: its field open (the parent closes it on a switch).
    if (p === 'openai') keyEditing = !keyOn;
  }

  /** Use your ChatGPT plan instead…: on the plan, signing in when it isn't. */
  function usePlan() {
    choose(CHATGPT);
    if (!planOn && !chatgpt.waiting) signInChatgpt(chatgpt.signedIn && !chatgpt.plan);
  }

  // Signed in (from Continue with ChatGPT, or Chat's picker) while the scope
  // waits on an OpenAI key it hasn't got: it goes on the plan.
  let wasPlan = untrack(() => planOn);
  $effect(() => {
    const on = planOn;
    if (on && !wasPlan && provider === 'openai' && !keyOn) onprovider(CHATGPT);
    wasPlan = on;
  });
</script>

<div class="gptcfg" data-testid="chatgpt-provider" data-way={provider}>
  {#if planOn && keyOn}
    <!-- Both connected: which one this scope uses. -->
    <div class="ways">
      <Segmented bind:value={() => provider, choose} options={GPT_WAYS} label="Connect ChatGPT with" {disabled} />
    </div>
  {/if}

  {#if planCard}
    <ChatGptSignIn {disabled} onstart={() => choose(CHATGPT)} />
  {/if}

  {#if keyToggle}
    <button
      type="button"
      class="keytoggle"
      aria-expanded={!onPlan}
      aria-controls={onPlan ? undefined : 'gpt-key-way'}
      {disabled}
      onclick={() => choose(onPlan ? 'openai' : CHATGPT)}
    >
      <svg class="chev" class:open={!onPlan} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      Use an API key instead
    </button>
  {/if}

  {#if !onPlan}
    <div class="keyway" id="gpt-key-way">
      <div class="keyhead">
        <h4>OpenAI API key</h4>
        <a class="keylink" href={KEY_URL} onclick={createExternalLinkHandler(KEY_URL)} target="_blank" rel="noreferrer">Get your API key</a>
      </div>
      <p class="keynote">Pays per message on your OpenAI account. Stored only on this PC.</p>
      {#if masked && !keyEditing}
        <div class="keymask">
          <span class="keytext">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="7.5" cy="15.5" r="4.5" /><path d="M10.8 12.2 21 2M15 8l3 3" /></svg>
            {masked}
          </span>
          <button class="keyedit" onclick={() => (keyEditing = true)}>edit</button>
        </div>
      {:else}
        <div class="keymask editing" bind:this={keyBox}>
          <Input type="password" bind:value={apiKey} oninput={onkeyinput} {disabled} placeholder="sk-…" ariaLabel="OpenAI API key" />
          {#if masked}<button class="keyedit" onclick={() => (keyEditing = false)}>done</button>{/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if connected}
    <h4 class="selhead">Model</h4>
    <ModelPicker {provider} bind:value={model} {disabled} {images} />
  {/if}

  {#if !(planOn && keyOn)}
    {#if onPlan && planOn}
      <button type="button" class="waylink" {disabled} onclick={() => choose('openai')}>Use an OpenAI API key instead…</button>
    {:else if !onPlan && (keyOn || planOn)}
      <button type="button" class="waylink" {disabled} onclick={usePlan}>Use your ChatGPT plan instead…</button>
    {/if}
  {/if}
</div>

<style>
  .gptcfg {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .ways {
    display: flex;
  }
  .keyway {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .keyhead h4,
  .selhead {
    margin: 0;
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .selhead {
    margin-top: 2px;
  }
  .keynote {
    margin: -4px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .keylink {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .keylink:hover {
    text-decoration-color: currentColor;
  }
  .keymask {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    padding: 8px 12px;
  }
  .keymask.editing {
    padding: 0;
    border: none;
    background: none;
  }
  .keymask.editing :global(.input) {
    flex: 1 1 auto;
  }
  .keytext {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .keytext svg {
    width: 13px;
    height: 13px;
  }
  .keyedit,
  .waylink {
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .waylink {
    align-self: flex-start;
  }
  .waylink:hover:not(:disabled) {
    text-decoration: underline;
  }
  /* The second way: quieter than the plan's button above it. */
  .keytoggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
    margin-top: -2px;
    padding: 2px 4px 2px 0;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12.5px;
    font-weight: 560;
    cursor: pointer;
  }
  .keytoggle:hover:not(:disabled) {
    color: var(--yap-fg);
  }
  .chev {
    width: 13px;
    height: 13px;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  @media (prefers-reduced-motion: no-preference) {
    .chev {
      transition: transform var(--yap-dur);
    }
  }
  .waylink:disabled,
  .keytoggle:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .keylink:focus-visible,
  .keyedit:focus-visible,
  .waylink:focus-visible,
  .keytoggle:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: var(--yap-focus);
  }
</style>
