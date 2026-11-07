<script>
  // **Chat | Code**, at the top of Chat's conversation list (as the Claude
  // app's sidebar switch): two places, plain conversations and Code chats
  // (codeChats.svelte.js). Shown only with Jobs on. A dot on Code while a
  // job runs and the Chat side shows.
  import { jobs } from '../../stores/jobs.svelte.js';
  import { isLive } from '../../core/jobs.js';
  import { CODE_NAME } from '../../core/codeChats.js';

  /** `code`: the view's Code chats (codeChats.svelte.js). */
  let { code } = $props();

  const busy = $derived(jobs.list.some((j) => isLive(j.status)));

  function onKey(e) {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      const next = code.side === 'chat' ? 'code' : 'chat';
      code.setSide(next);
      e.currentTarget.querySelector(`[data-side="${next}"]`)?.focus();
    }
  }
</script>

<div class="sideswitch" role="radiogroup" aria-label="Chat or {CODE_NAME}" tabindex="-1" data-testid="chat-side-switch" data-side={code.side} onkeydown={onKey}>
  <button
    type="button"
    role="radio"
    class="seg"
    aria-checked={code.side === 'chat'}
    aria-label="Chat"
    title="Chat"
    tabindex={code.side === 'chat' ? 0 : -1}
    data-side="chat"
    onclick={() => code.setSide('chat')}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12.5a7.5 7.5 0 0 1-11.2 6.5L4 20l1.1-4.3A7.5 7.5 0 1 1 20 12.5z" /></svg>
  </button>
  <button
    type="button"
    role="radio"
    class="seg"
    aria-checked={code.side === 'code'}
    aria-label={CODE_NAME}
    title={CODE_NAME}
    tabindex={code.side === 'code' ? 0 : -1}
    data-side="code"
    onclick={() => code.setSide('code')}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 7-5 5 5 5" /><path d="m16 7 5 5-5 5" /></svg>
    {#if busy && code.side === 'chat'}<span class="busy" aria-hidden="true" data-testid="chat-side-busy"></span>{/if}
  </button>
</div>

<style>
  .sideswitch {
    flex: 0 0 auto;
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    margin-right: 2px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    outline: none;
  }
  .seg {
    position: relative;
    width: 25px;
    height: 24px;
    display: grid;
    place-items: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    transition:
      background var(--yap-dur),
      color var(--yap-dur);
  }
  .seg svg {
    width: 14px;
    height: 14px;
  }
  .seg:hover {
    color: var(--yap-fg);
  }
  .seg[aria-checked='true'] {
    background: var(--yap-s3);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-sm);
  }
  .seg[data-side='code'][aria-checked='true'] {
    color: var(--yap-code);
  }
  .seg:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .busy {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--yap-code);
    box-shadow: 0 0 0 1.5px var(--yap-s1);
  }
</style>
