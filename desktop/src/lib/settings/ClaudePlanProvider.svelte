<script>
  // Settings → Language Models → Chat & Voice Assistant → Cloud providers →
  // Claude (2026-11-07, as ChatGPT's tab): Claude's models two ways, the
  // plan first:
  //   1. your Claude plan (ai/claude_plan.rs, the provider "claude-plan"):
  //      Yap runs the person's own Claude Code on this PC, signed in with
  //      their own Claude account; answers use their plan's limits;
  //   2. an Anthropic API key (the provider "anthropic"), pay per use.
  // The Chat scope only: Dictation Cleanup, Commands and Note Formatting
  // keep the key alone (ScopeProviderConfig shows this for "chat").
  //
  // The plan's card (`data-testid="claude-plan-card"`), by what Claude Code
  // is here (claude_plan_status, stores/claudePlan.svelte.js):
  //   - not installed: a link to its install guide, Check again;
  //   - too old: its version and the one Yap needs;
  //   - signed in with an API key (a Console login): not a plan;
  //   - found but not signed in: Sign in (Claude Code's own `claude auth
  //     login`, in a terminal Yap opens; Yap reads it again when its window
  //     gets the focus back);
  //   - signed in: Connected · Using Claude plan, its version and plan,
  //     Manage usage, and Use for Chat when Chat is on the key.
  // Under it, small: Claude Code keeps its own copy of each plan chat, and
  // Yap deletes it with the chat (claude_code/transcripts.rs).
  //
  //   provider:     'claude-plan' | 'anthropic', the scope's.
  //   onprovider:   (p) => the scope put on p (its key and model follow).
  //   apiKey:       the scope's active key, bound (an Anthropic key on 'anthropic').
  //   onkeyinput:   the key written through to the shared store.
  //   anthropicKey: the Anthropic key the store has, whatever the scope is on.
  //   planKey:      the store's key for the plan (its marker while signed in),
  //                 until claude_plan_status is in.
  //   model:        the scope's model, bound.
  //   keyEditing:   the key field open (else masked), bound.
  //   images:       "Reads images" in the model list.
  import { onMount, untrack } from 'svelte';
  import ModelPicker from './ModelPicker.svelte';
  import Input from '../ui/Input.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import ProviderMark from '../views/chat/ProviderMark.svelte';
  import { PP_CLOUD_MODELS } from '../core/ppModels.js';
  import { CLAUDE_PLAN, CLAUDE_PLAN_KEY, CLAUDE_WAYS, maskKey } from '../core/providers.js';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import { claudePlan, loadClaudePlan, signInClaudePlan } from '../stores/claudePlan.svelte.js';

  let {
    provider,
    onprovider,
    apiKey = $bindable(''),
    onkeyinput = null,
    anthropicKey = '',
    planKey = '',
    model = $bindable(''),
    keyEditing = $bindable(false),
    disabled = false,
    images = false,
  } = $props();

  onMount(() => loadClaudePlan(true));

  const KEY_URL = PP_CLOUD_MODELS.anthropic.keyUrl;
  const planOn = $derived(claudePlan.loaded ? claudePlan.ready : planKey === CLAUDE_PLAN_KEY);
  const keyOn = $derived(!!String(anthropicKey || '').trim());
  const onPlan = $derived(provider === CLAUDE_PLAN);
  const connected = $derived(onPlan ? planOn : keyOn);
  const masked = $derived(maskKey(apiKey));
  // The plan's card: on the plan, or with nothing connected (it comes first).
  const planCard = $derived(onPlan || (!planOn && !keyOn));
  // "Use an API key instead": the card's second way, while the plan isn't in.
  const keyToggle = $derived(!planOn && (onPlan || !keyOn));
  const planName = $derived(
    claudePlan.plan ? `${claudePlan.plan.charAt(0).toUpperCase()}${claudePlan.plan.slice(1)} plan` : 'your Claude plan'
  );

  function choose(p) {
    if (p === provider) return;
    onprovider(p);
    // A key still to paste: its field open (the parent closes it on a switch).
    if (p === 'anthropic') keyEditing = !keyOn;
  }

  /** Use your Claude plan instead…: on the plan; signing in when it isn't. */
  function usePlan() {
    choose(CLAUDE_PLAN);
    if (claudePlan.loaded && claudePlan.installed && !claudePlan.tooOld && !claudePlan.loggedIn) signInClaudePlan();
  }

  // Signed in (here, or from Chat's picker) while the scope waits on an
  // Anthropic key it hasn't got: it goes on the plan.
  let wasPlan = untrack(() => planOn);
  $effect(() => {
    const on = planOn;
    if (on && !wasPlan && provider === 'anthropic' && !keyOn) onprovider(CLAUDE_PLAN);
    wasPlan = on;
  });
</script>

<div class="claudecfg" data-testid="claude-provider" data-way={provider}>
  {#if planOn && keyOn}
    <!-- Both connected: which one this scope uses. -->
    <div class="ways" data-setting="claude-ways">
      <Segmented bind:value={() => provider, choose} options={CLAUDE_WAYS} label="Connect Claude with" {disabled} />
    </div>
  {/if}

  {#if planCard}
    <div class="plan" data-testid="claude-plan-card" data-setting="claude-plan">
      <div class="head">
        <h4>Your Claude plan</h4>
        {#if claudePlan.ready}
          <a class="link" href={claudePlan.usageUrl} onclick={createExternalLinkHandler(claudePlan.usageUrl)}>Manage usage</a>
        {/if}
      </div>
      <p class="lead">Yap runs Claude Code on this PC, signed in with your own Claude account. Answers use your plan's limits.</p>

      {#if !claudePlan.loaded}
        <div class="state" role="status"><div class="words">Looking for Claude Code on this PC…</div></div>
      {:else if !claudePlan.installed}
        <div class="state" data-state="missing">
          <div class="words">
            Claude Code isn't installed on this PC.
            <span class="who">Install it, sign in with your Claude account, then check again.</span>
          </div>
          <div class="btns">
            <a class="link" href={claudePlan.installUrl} onclick={createExternalLinkHandler(claudePlan.installUrl)}>Install Claude Code</a>
            <Button variant="ghost" size="sm" disabled={claudePlan.checking} onclick={() => loadClaudePlan(true, true)}>Check again</Button>
          </div>
        </div>
      {:else if claudePlan.tooOld}
        <div class="state" data-state="old">
          <div class="words">
            Claude Code {claudePlan.version} is too old.
            <span class="who">Yap needs {claudePlan.minVersion} or newer: update it, then check again.</span>
          </div>
          <div class="btns">
            <a class="link" href={claudePlan.installUrl} onclick={createExternalLinkHandler(claudePlan.installUrl)}>How to update</a>
            <Button variant="ghost" size="sm" disabled={claudePlan.checking} onclick={() => loadClaudePlan(true, true)}>Check again</Button>
          </div>
        </div>
      {:else if claudePlan.onAKey}
        <div class="state" data-state="key">
          <div class="words">
            Claude Code here is signed in with an API key, not a Claude plan.
            <span class="who">Sign in again with your Claude account to use your plan.</span>
          </div>
          <Button size="sm" {disabled} onclick={signInClaudePlan}>Sign in</Button>
        </div>
      {:else if !claudePlan.loggedIn}
        <div class="state" data-state="signed-out">
          <div class="words">
            {#if claudePlan.signingIn}
              Finish signing in to Claude Code in the terminal Yap opened.
              <span class="who">Yap checks again when you come back to this window.</span>
            {:else}
              Claude Code {claudePlan.version || ''} is on this PC, but it isn't signed in.
              <span class="who">Sign in with your Claude account, in Claude Code's own sign-in.</span>
            {/if}
          </div>
          <div class="btns">
            {#if claudePlan.signingIn}
              <Button variant="ghost" size="sm" disabled={claudePlan.checking} onclick={() => loadClaudePlan(true, true)}>Check again</Button>
            {:else}
              <span data-setting="claude-plan-sign-in"><Button size="sm" {disabled} onclick={signInClaudePlan}><ProviderMark icon="claude" size={14} />Sign in</Button></span>
            {/if}
          </div>
        </div>
      {:else}
        <div class="state on" data-state="ready">
          <span class="dot" aria-hidden="true"></span>
          <div class="words">
            <strong>Connected</strong> · Using Claude plan
            <span class="who">Claude Code {claudePlan.version} · {planName}</span>
          </div>
          {#if !onPlan}
            <Button size="sm" {disabled} onclick={() => choose(CLAUDE_PLAN)}>Use for Chat</Button>
          {/if}
        </div>
      {/if}

      {#if claudePlan.failed || claudePlan.error}
        <p class="problem" role="alert">{claudePlan.failed || claudePlan.error}</p>
      {/if}
      <p class="copies" data-testid="claude-plan-copies">Claude Code keeps its own copy of each plan chat; Yap deletes it with the chat.</p>
    </div>
  {/if}

  {#if keyToggle}
    <button
      type="button"
      class="keytoggle"
      aria-expanded={!onPlan}
      aria-controls={onPlan ? undefined : 'claude-key-way'}
      {disabled}
      onclick={() => choose(onPlan ? 'anthropic' : CLAUDE_PLAN)}
    >
      <svg class="chev" class:open={!onPlan} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      Use an API key instead
    </button>
  {/if}

  {#if !onPlan}
    <div class="keyway" id="claude-key-way">
      <div class="keyhead">
        <h4>Anthropic API key</h4>
        <a class="keylink" href={KEY_URL} onclick={createExternalLinkHandler(KEY_URL)} target="_blank" rel="noreferrer">Get your API key</a>
      </div>
      <p class="keynote">Pays per message on your Anthropic account. Stored only on this PC.</p>
      {#if masked && !keyEditing}
        <div class="keymask">
          <span class="keytext">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="7.5" cy="15.5" r="4.5" /><path d="M10.8 12.2 21 2M15 8l3 3" /></svg>
            {masked}
          </span>
          <button class="keyedit" onclick={() => (keyEditing = true)}>edit</button>
        </div>
      {:else}
        <div class="keymask editing">
          <Input type="password" bind:value={apiKey} oninput={onkeyinput} {disabled} placeholder="sk-ant-…" ariaLabel="Anthropic API key" />
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
      <button type="button" class="waylink" {disabled} onclick={() => choose('anthropic')}>Use an Anthropic API key instead…</button>
    {:else if !onPlan && keyOn}
      <button type="button" class="waylink" {disabled} onclick={usePlan}>Use your Claude plan instead…</button>
    {/if}
  {/if}
</div>

<style>
  .claudecfg,
  .plan {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .plan {
    gap: 10px;
  }
  .ways {
    display: flex;
  }
  .head,
  .keyhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  h4 {
    margin: 0;
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .selhead {
    margin-top: 2px;
  }
  .lead {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .copies {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .state {
    display: flex;
    align-items: center;
    gap: 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    padding: 10px 12px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .words {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    color: var(--yap-fg);
    line-height: 1.45;
  }
  .who {
    display: block;
    font-size: 12px;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .btns {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .problem {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-danger);
  }
  /* Claude's mark on Sign in: the button's own text colour. */
  .plan :global(.btn .pmark) {
    color: inherit;
  }
  .keyway {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .keynote {
    margin: -4px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .link,
  .keylink {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .link:hover,
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
  /* The second way: quieter than the plan's card above it. */
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
  .link:focus-visible,
  .keylink:focus-visible,
  .keyedit:focus-visible,
  .waylink:focus-visible,
  .keytoggle:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: var(--yap-focus);
  }
</style>
