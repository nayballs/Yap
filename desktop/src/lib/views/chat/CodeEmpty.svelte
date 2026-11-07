<script>
  // **An empty Code chat** (codeChats.svelte.js): the Code side's own
  // greeting in the thread's place, quiet, in the Code chat's teal — the
  // code mark, "Work in garden-planner" with its path under it, what a Code
  // chat is for, three one-tap prompts that fill the box (never sent by
  // themselves: a request here can start a job, so you read it first), and
  // what stands between this PC and a job when something does (Claude Code
  // not on this PC, too old, or signed out on the plan: Sign in, Claude
  // Code's own in a terminal; the store is the Claude plan card's,
  // stores/claudePlan.svelte.js). Chat's own greeting is ChatThread's.
  import { onMount } from 'svelte';
  import { claudePlan, loadClaudePlan, signInClaudePlan } from '../../stores/claudePlan.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { CODE_INTRO, CODE_PROMPTS, claudeCodeHint } from '../../core/codeChats.js';

  /** `chat`: the view's state; `code`: its Code chats; `onopensettings(section)`: Settings. */
  let { chat, code, onopensettings = null } = $props();

  const ws = $derived(code.current);
  const hint = $derived(claudeCodeHint(claudePlan, code.pay));

  onMount(() => loadClaudePlan());

  /** A prompt into the box, the caret after it: yours to change or send. */
  function fill(text) {
    if (chat.sending) return;
    chat.byVoice = false;
    chat.input = text;
    setTimeout(() => {
      const el = chat.inputEl;
      if (!el) return;
      el.focus();
      el.setSelectionRange?.(text.length, text.length);
    }, 0);
  }

  /** Add workspace…: Windows' folder picker; the folder picked opens a new code chat. */
  async function addThenOpen() {
    try {
      const w = await code.addWorkspace();
      if (w) code.newCodeChat(w);
    } catch {
      /* the picker said why, or was cancelled */
    }
  }

  function act(h) {
    if (h.action === 'install' || h.action === 'update') openExternalLink(claudePlan.installUrl);
    else if (h.action === 'sign-in') signInClaudePlan();
    else loadClaudePlan(true, true);
  }
</script>

<div class="codeempty" data-testid="chat-code-empty" data-workspace={ws?.name ?? ''}>
  <svg class="mark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"
    ><path d="m8 7-5 5 5 5" /><path d="m16 7 5 5-5 5" /><path d="m13.5 5-3 14" /></svg
  >
  {#if !ws}
    <!-- The Code side with no workspace to work in (none yet, or the one
         picked was removed): pick one, or add one. -->
    <h2 class="title">Choose a workspace</h2>
    <p class="intro">A code chat works in a folder on this PC. Pick one, or add one, and jobs can work there.</p>
    <div class="prompts" role="group" aria-label="Workspaces">
      {#each code.workspaces.slice(0, 3) as w (w.path)}
        <button type="button" class="prompt" data-testid="chat-code-empty-workspace" title={w.path} onclick={() => code.newCodeChat(w)}>{w.name}</button>
      {/each}
      <button type="button" class="prompt" data-testid="chat-code-empty-add" onclick={addThenOpen}>Add workspace…</button>
    </div>
  {:else}
  <h2 class="title">Work in <span class="name">{ws.name}</span></h2>
  <p class="path" title={ws.path}>{ws.path}</p>
  <p class="intro">{CODE_INTRO}</p>
  <div class="prompts" role="group" aria-label="Try asking">
    {#each CODE_PROMPTS as p (p)}
      <button type="button" class="prompt" data-testid="chat-code-prompt" disabled={chat.sending} onclick={() => fill(p)}>{p}</button>
    {/each}
  </div>
  {#if hint}
    <!-- What stands between this PC and a job: one line, one button. -->
    <p class="note" data-testid="chat-code-claude" data-state={hint.state}>
      {hint.text}
      <button type="button" class="link" disabled={hint.action === 'check' && claudePlan.checking} onclick={() => act(hint)}>{hint.label}</button>
    </p>
    {#if claudePlan.failed}
      <p class="note" role="alert">{claudePlan.failed}</p>
    {/if}
  {/if}
  {/if}
  {#if !chat.chatReady}
    <!-- Before a first question fails: what Chat needs (a Code chat's too). -->
    <p class="note">
      Chat needs an AI model: a free key from a cloud provider, or one that runs on this PC.
      <button type="button" class="link" onclick={() => onopensettings?.('cleanup#chat')}>Set up an AI model</button>
    </p>
  {/if}
</div>

<style>
  .codeempty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 0 16px;
    text-align: center;
    color: var(--yap-muted);
  }
  .mark {
    width: 34px;
    height: 34px;
    margin-bottom: 4px;
    color: var(--yap-code);
    opacity: 0.85;
  }
  .title {
    margin: 0;
    max-width: 100%;
    font-size: 14px;
    font-weight: 600;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .name {
    color: var(--yap-code);
  }
  .path {
    margin: -4px 0 0;
    max-width: min(360px, 100%);
    font-size: 11px;
    color: var(--yap-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .intro {
    margin: 4px 0 0;
    max-width: 340px;
    font-size: 12px;
    line-height: 1.6;
    text-wrap: balance;
  }
  .prompts {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    max-width: 440px;
    margin-top: 8px;
  }
  .prompt {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .prompt:hover:not(:disabled) {
    background: var(--yap-code-wash);
    border-color: var(--yap-code-line);
  }
  .prompt:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .prompt:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  .note {
    margin: 8px 0 0;
    max-width: 320px;
    font-size: 12px;
    line-height: 1.6;
    color: var(--yap-fg-80);
    text-wrap: balance;
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }
  .link:hover:not(:disabled) {
    text-decoration: underline;
  }
  .link:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .link:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }
</style>
