<script>
  // **A Code chat's message box** (codeChats.svelte.js; its look is all in
  // codeChat.css): the label on its top edge — "Code · garden-planner", the
  // workspace's path on hover — and, while an agent from this chat runs, the
  // slow light along its edge ("Animate Code chats"; never with reduced
  // motion). Inside the box (Composer's `.inputbar`); nothing in a plain chat.
  // The label opens the chat's **Environment** card (its workspace, Remote
  // Control for it, the phones, the Mode: CodeEnvironment.svelte, its own chunk).
  // **"+1 folder"** beside it (17 Nov 2026) once the + menu's Add folder gave the chat's Claude Code more folders
  // (agents/menu.rs): hovered or focused, a card lists them; clicked, the summary panel opens on its Environment,
  // where each has its ×.
  import './codeChat.css';
  import { codeLabel } from '../../core/codeChats.js';
  import { chatSummary } from '../../stores/chatSummary.svelte.js';

  /** `code`: the view's Code chats; `conversation`: the chat shown (none: a new chat); `folders`: its added ones. */
  let { code, conversation = null, folders = [] } = $props();

  let open = $state(false);
  // Another chat, or a plain one: the card closes.
  $effect(() => {
    void conversation;
    void code?.current?.name;
    open = false;
  });
  const n = $derived(folders?.length ?? 0);
  const plus = $derived(n === 1 ? '+1 folder' : `+${n} folders`);
  /** The summary panel, on its Environment (open already: as it is). */
  function showFolders() {
    if (!chatSummary.open) chatSummary.presses += 1;
  }
</script>

{#if code?.current}
  <span class="code-edge-row">
    <button
      type="button"
      class="code-edge-label"
      title={code.current.path}
      aria-haspopup="dialog"
      aria-expanded={open}
      data-testid="chat-code-label"
      onclick={() => (open = !open)}>{codeLabel(code.current.name)}</button
    >
    {#if n > 0}
      <span class="code-edge-more">
        <button
          type="button"
          class="code-edge-label code-edge-folders"
          aria-describedby="code-edge-folders-card"
          aria-label={`${plus} Claude Code may use too: show them in the summary`}
          data-testid="chat-code-folders"
          onclick={showFolders}>{plus}</button
        >
        <span class="code-edge-card" role="tooltip" id="code-edge-folders-card" data-testid="chat-code-folders-card">
          <span class="cec-head">Claude Code may use too</span>
          {#each folders as f (f.path)}
            <span class="cec-row" class:gone={!f.exists}>
              <b>{f.name}</b>
              <span class="cec-path">{f.exists ? f.path : `${f.path} (not on this PC)`}</span>
            </span>
          {/each}
          <span class="cec-foot">Click to see them in the summary</span>
        </span>
      </span>
    {/if}
  </span>
  {#if open}
    {#await import('./CodeEnvironment.svelte') then { default: CodeEnvironment }}
      <CodeEnvironment workspace={code.current} {conversation} onclose={() => (open = false)} />
    {/await}
  {/if}
  {#if code.moving}
    <span class="code-edge-light" aria-hidden="true" data-testid="chat-code-light"></span>
  {/if}
{/if}
