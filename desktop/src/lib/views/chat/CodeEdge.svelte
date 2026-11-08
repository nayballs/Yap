<script>
  // **A Code chat's message box** (codeChats.svelte.js; its look is all in
  // codeChat.css): the label on its top edge — "Code · garden-planner", the
  // workspace's path on hover — and, while an agent from this chat runs, the
  // slow light along its edge ("Animate Code chats"; never with reduced
  // motion). Inside the box (Composer's `.inputbar`); nothing in a plain chat.
  import './codeChat.css';
  import { codeLabel } from '../../core/codeChats.js';

  /** `code`: the view's Code chats. */
  let { code } = $props();
</script>

{#if code?.current}
  <span class="code-edge-label" title={code.current.path} data-testid="chat-code-label">{codeLabel(code.current.name)}</span>
  {#if code.moving}
    <span class="code-edge-light" aria-hidden="true" data-testid="chat-code-light"></span>
  {/if}
{/if}
