<script>
  // **Edit** (15 Nov 2026, the Claude app's): the person's message open in place, its words to change. Save and send
  // asks them instead — a new version of the conversation from there, ‹ 1/2 › on the message (chat/chats/branches.rs);
  // in a Code chat, a Rewind to before it first (agents/rewind.rs). Enter saves and sends, Shift+Enter is a new line,
  // Esc cancels. `text`: its words; `note`: a line beside the buttons (what else goes again: its screenshots).
  import { onMount } from 'svelte';

  let { text = '', note = '', onsave, oncancel } = $props();
  // (Its words as it opened: what's typed after is this box's own.)
  // svelte-ignore state_referenced_locally
  let draft = $state(text);
  let el = $state(null);

  onMount(() => {
    fit();
    el?.focus({ preventScroll: true });
    el?.setSelectionRange(draft.length, draft.length);
  });

  /** As tall as its words, up to a point (then it scrolls). */
  function fit() {
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight + 2, 320)}px`;
  }

  function save() {
    const words = draft.trim();
    if (words) onsave?.(words);
  }

  function key(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      oncancel?.();
    } else if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      save();
    }
  }
</script>

<div class="medit" data-testid="chat-edit">
  <textarea bind:this={el} bind:value={draft} oninput={fit} onkeydown={key} rows="2" aria-label="Edit your message" data-testid="chat-edit-input"></textarea>
  <div class="meditbar">
    {#if note}<span class="meditnote">{note}</span>{/if}
    <button type="button" class="mbtn" data-testid="chat-edit-cancel" onclick={() => oncancel?.()}>Cancel</button>
    <button type="button" class="mbtn primary" data-testid="chat-edit-save" disabled={!draft.trim()} onclick={save}>Save and send</button>
  </div>
</div>

<style>
  .medit {
    align-self: flex-end;
    width: min(100%, 560px);
    max-width: 92%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--yap-primary-line);
    border-radius: 12px;
    background: var(--yap-s1);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    min-height: 44px;
    resize: none;
    border: none;
    outline: none;
    padding: 4px 6px;
    background: transparent;
    color: var(--yap-fg);
    font-family: var(--yap-font-reading);
    font-size: calc(12.5px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
  }
  .meditbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }
  .meditnote {
    margin-right: auto;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .mbtn {
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .mbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .mbtn.primary {
    border-color: transparent;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .mbtn.primary:hover:not(:disabled) {
    filter: brightness(1.06);
  }
  .mbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .mbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
