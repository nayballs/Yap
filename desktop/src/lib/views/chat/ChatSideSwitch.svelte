<script>
  // **Chat | Code**, at the top of Chat's conversation list (as the Claude
  // app's sidebar switch): two places, plain conversations and Code chats
  // (codeChats.svelte.js). Shown only with Jobs on. The side not shown has
  // a status dot by the rows' own rules (core/chatStatus.js): the strongest
  // of its chats' — needs you, working, new.
  import { CODE_NAME, sideOf } from '../../core/codeChats.js';
  import { strongest, statusWords } from '../../core/chatStatus.js';
  import { chatStatus } from '../../stores/chatStatus.svelte.js';
  import StatusDot from './StatusDot.svelte';

  /** `chat`: the view's state (its conversations); `code`: its Code chats (codeChats.svelte.js). */
  let { chat = null, code } = $props();

  // The side not shown, and the strongest state among its chats.
  const other = $derived(code.side === 'chat' ? 'code' : 'chat');
  const otherState = $derived(
    strongest(
      chatStatus.byId,
      (chat?.conversations ?? []).filter((c) => sideOf(c.id, code.chats) === other).map((c) => c.id)
    )
  );
  // (Said with the segment too, for a screen reader.)
  const otherWords = $derived(statusWords(otherState, otherState?.count ?? 1));

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
    aria-describedby={other === 'chat' && otherWords ? 'chat-side-status-words' : undefined}
    title="Chat"
    tabindex={code.side === 'chat' ? 0 : -1}
    data-side="chat"
    onclick={() => code.setSide('chat')}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12.5a7.5 7.5 0 0 1-11.2 6.5L4 20l1.1-4.3A7.5 7.5 0 1 1 20 12.5z" /></svg>
    {#if other === 'chat'}<StatusDot s={otherState} place="badge" decorative testid="chat-side-status" />{/if}
  </button>
  <button
    type="button"
    role="radio"
    class="seg"
    aria-checked={code.side === 'code'}
    aria-label={CODE_NAME}
    aria-describedby={other === 'code' && otherWords ? 'chat-side-status-words' : undefined}
    title={CODE_NAME}
    tabindex={code.side === 'code' ? 0 : -1}
    data-side="code"
    onclick={() => code.setSide('code')}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 7-5 5 5 5" /><path d="m16 7 5 5-5 5" /></svg>
    {#if other === 'code'}<StatusDot s={otherState} code place="badge" decorative testid="chat-side-status" />{/if}
  </button>
  {#if otherWords}<span id="chat-side-status-words" class="sr-only">{otherWords}</span>{/if}
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
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
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
  /* (The side not shown's dot: on the segment's top right, ringed in the switch's own surface.) */
  .seg {
    --sdot-top: 2px;
    --sdot-right: 2px;
    --sdot-bg: var(--yap-s1);
  }
</style>
