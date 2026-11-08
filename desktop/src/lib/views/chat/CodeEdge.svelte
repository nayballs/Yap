<script>
  // **A Code chat's message box** (codeChats.svelte.js; its look is all in
  // codeChat.css): the label on its top edge — "Code · garden-planner", the
  // workspace's path on hover — and, while an agent from this chat runs, the
  // slow light along its edge ("Animate Code chats"; never with reduced
  // motion). Inside the box (Composer's `.inputbar`); nothing in a plain chat.
  // The label opens the chat's **Environment** card (its workspace, Remote
  // Control for it, the phones, the Mode: CodeEnvironment.svelte, its own chunk).
  import './codeChat.css';
  import { codeLabel } from '../../core/codeChats.js';

  /** `code`: the view's Code chats; `conversation`: the chat shown (none: a new chat). */
  let { code, conversation = null } = $props();

  let open = $state(false);
  // Another chat, or a plain one: the card closes.
  $effect(() => {
    void conversation;
    void code?.current?.name;
    open = false;
  });
</script>

{#if code?.current}
  <button
    type="button"
    class="code-edge-label"
    title={code.current.path}
    aria-haspopup="dialog"
    aria-expanded={open}
    data-testid="chat-code-label"
    onclick={() => (open = !open)}>{codeLabel(code.current.name)}</button
  >
  {#if open}
    {#await import('./CodeEnvironment.svelte') then { default: CodeEnvironment }}
      <CodeEnvironment workspace={code.current} {conversation} onclose={() => (open = false)} />
    {/await}
  {/if}
  {#if code.moving}
    <span class="code-edge-light" aria-hidden="true" data-testid="chat-code-light"></span>
  {/if}
{/if}
