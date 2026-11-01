<script>
  // Sign in with ChatGPT (ai/chatgpt.rs): the ChatGPT tab's account card in
  // Settings → Language Models, in place of an API key — every scope's and
  // Dictation Cleanup's. In OpenAI's words (its UI rules for apps that use
  // a person's plan): "Continue with ChatGPT"; once in, "Connected",
  // "Using ChatGPT plan" and "Manage usage"; the one-time "You're using your
  // ChatGPT plan" notice with Got it. Signed in without the plan's
  // permission: turn it on (asked again), or use an API key instead. Sign
  // out revokes it at OpenAI; one OpenAI didn't confirm says where to
  // disconnect Yap in ChatGPT's settings.
  import { onMount } from 'svelte';
  import Button from '../ui/Button.svelte';
  import ProviderMark from '../views/chat/ProviderMark.svelte';
  import { createExternalLinkHandler } from '../core/externalLinks.js';
  import {
    chatgpt,
    loadChatgpt,
    signInChatgpt,
    cancelChatgpt,
    signOutChatgpt,
    chatgptNoticeSeen,
  } from '../stores/chatgpt.svelte.js';

  let { disabled = false } = $props();
  let busy = $state(false);

  onMount(loadChatgpt);

  const who = $derived(chatgpt.email || chatgpt.name || 'your ChatGPT account');
  const notice = $derived(chatgpt.loaded && chatgpt.plan && !chatgpt.noticeSeen);
  const problem = $derived(chatgpt.failed || chatgpt.error);

  async function signOut() {
    busy = true;
    await signOutChatgpt();
    busy = false;
  }
</script>

<div class="gpt" data-testid="chatgpt-card">
  <div class="head">
    <h4>Your ChatGPT plan</h4>
    {#if chatgpt.plan}
      <a class="link" href={chatgpt.usageUrl} onclick={createExternalLinkHandler(chatgpt.usageUrl)}>Manage usage</a>
    {/if}
  </div>

  {#if chatgpt.plan}
    <div class="state on">
      <span class="dot" aria-hidden="true"></span>
      <div class="words">
        <strong>Connected</strong> · Using ChatGPT plan
        <span class="who">{who}</span>
      </div>
      <Button variant="secondary" size="sm" disabled={busy || disabled} onclick={signOut}>Sign out</Button>
    </div>
  {:else if chatgpt.signedIn}
    <div class="state">
      <div class="words">
        Signed in as <strong>{who}</strong>, but Yap isn't allowed to use your ChatGPT plan.
        <span class="who">Turn it on, or use an OpenAI API key on the OpenAI tab instead.</span>
      </div>
      <div class="btns">
        <Button size="sm" disabled={chatgpt.waiting || disabled} onclick={() => signInChatgpt(true)}>Turn on plan usage</Button>
        <Button variant="ghost" size="sm" disabled={busy || disabled} onclick={signOut}>Sign out</Button>
      </div>
    </div>
  {:else if chatgpt.waiting}
    <div class="state">
      <div class="words">
        Finish signing in in your browser.
        <span class="who">Yap is waiting for ChatGPT to hand back.</span>
      </div>
      <Button variant="ghost" size="sm" onclick={cancelChatgpt}>Cancel</Button>
    </div>
  {:else}
    <p class="lead">
      Use the ChatGPT plan you already pay for, instead of an API key. Requests go from this PC straight to OpenAI and
      count towards your plan's usage.
    </p>
    <div>
      <Button disabled={disabled || !chatgpt.loaded} onclick={() => signInChatgpt(false)}>
        <ProviderMark icon="openai" size={15} />
        Continue with ChatGPT
      </Button>
    </div>
  {/if}

  {#if problem}
    <p class="problem" role="alert">{problem}</p>
  {:else if chatgpt.unconfirmed && !chatgpt.signedIn}
    <p class="problem" role="status">
      Signed out here, but OpenAI didn't confirm it. To be sure, disconnect Yap in ChatGPT's settings.
    </p>
  {/if}
</div>

{#if notice}
  <div class="scrim" role="presentation">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="gpt-notice-title">
      <ProviderMark icon="openai" size={30} />
      <h2 id="gpt-notice-title">You're using your ChatGPT plan</h2>
      <p>
        Yap's AI now runs on {who}'s plan: what you ask goes to OpenAI and counts towards its usage. You can change
        that any time in Settings → Language Models.
      </p>
      <div class="dbtns">
        <a class="link" href={chatgpt.usageUrl} onclick={createExternalLinkHandler(chatgpt.usageUrl)}>Manage usage</a>
        <Button onclick={chatgptNoticeSeen}>Got it</Button>
      </div>
    </div>
  </div>
{/if}


<style>
  .gpt {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .head {
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
  .link {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-link);
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .link:hover {
    text-decoration-color: currentColor;
  }
  .link:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: var(--yap-focus);
  }
  .lead {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
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
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .problem {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-danger);
  }
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 96;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: var(--yap-paper-scrim);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    width: min(380px, 100%);
    box-sizing: border-box;
    padding: 26px 24px 22px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 14px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    text-align: center;
  }
  .dialog h2 {
    margin: 4px 0 0;
    font-size: 17px;
    font-weight: 600;
  }
  .dialog p {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--yap-paper-muted);
  }
  .dbtns {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    margin-top: 10px;
  }
</style>
