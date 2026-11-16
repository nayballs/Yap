<script>
  // **A side question** (Ctrl+;, chat/side.rs; the Claude app's side chat):
  // a small box over the thread, just above the message box, that asks
  // Chat's model a quick question about this chat. The answer streams in the
  // box and isn't added to the chat: Esc or ✕ and it's gone; Keep it adds the
  // question and answer to the thread as an ordinary exchange (never while
  // the thread is answering: it waits, and says so). Its state is
  // sideQuestion.svelte.js; ChatView opens it.
  import { onMount, tick } from 'svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { closePartial } from '../../core/streaming.js';
  import { KEEP_LATER } from './sideQuestion.svelte.js';
  // Toasts never cover the box (ui/toastClear.js).
  import { keepClear } from '../../ui/toastClear.js';

  /**
   * `side`: the box's state. `busy`: the thread is answering (Keep it waits).
   * `code`: a Code chat (the box says the session never sees it).
   * `onkept()`: it's in the thread now. `onclose()`: after Esc or ✕.
   */
  let { side, busy = false, code = false, onkept, onclose } = $props();

  let el = $state(null);
  let inputEl = $state(null);
  let bottom = $state(86);

  const text = $derived(side.result?.text ?? side.live.text ?? '');
  const shown = $derived(!!side.asked);
  const thinking = $derived(side.answering && !side.live.started);

  // Just above the message box, however tall it grows.
  onMount(() => {
    const composer = el?.parentElement?.querySelector(':scope > .composer');
    const place = () => {
      const pad = parseFloat(getComputedStyle(el.parentElement).paddingBottom) || 16;
      bottom = Math.round((composer?.offsetHeight ?? 60) + pad + 10);
    };
    place();
    const ro = composer && typeof ResizeObserver === 'function' ? new ResizeObserver(place) : null;
    ro?.observe(composer);
    tick().then(() => inputEl?.focus({ preventScroll: true }));
    return () => ro?.disconnect();
  });

  function close() {
    side.close();
    onclose?.();
  }

  function onKeydown(e) {
    // Esc is the box's: never the answer in the chat, nor ChatView's other keys.
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  }

  function onInputKey(e) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      side.ask();
    }
  }

  async function keep() {
    if (await side.keep()) {
      side.close();
      onkept?.();
    }
  }

  function grow(node) {
    const fit = () => {
      node.style.height = 'auto';
      node.style.height = `${Math.min(node.scrollHeight, 120)}px`;
    };
    fit();
    node.addEventListener('input', fit);
    return {
      update: fit,
      destroy: () => node.removeEventListener('input', fit),
    };
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="side"
  bind:this={el}
  role="dialog"
  aria-modal="false"
  aria-label="Side question"
  style:bottom="{bottom}px"
  use:keepClear
  onkeydown={onKeydown}
  data-testid="chat-sideq"
>
  <header>
    <span class="title">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h7A2.5 2.5 0 0 1 16 6.5v4a2.5 2.5 0 0 1-2.5 2.5H9l-3.5 3v-3A2.5 2.5 0 0 1 4 10.5z" /><path d="M19 9.5a2 2 0 0 1 1 1.7v3.6a2 2 0 0 1-2 2h-.5V19l-3-2.2H12" /></svg>
      Side question
    </span>
    <span class="note">{code ? "Not added to the chat, and Claude's session never sees it" : 'Not added to the chat'}</span>
    <button type="button" class="x" aria-label="Close the side question" title="Close (Esc)" onclick={close} data-testid="chat-sideq-close">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
    </button>
  </header>

  {#if shown}
    <div class="body" aria-live="polite">
      <p class="asked" data-testid="chat-sideq-question">{side.asked}</p>
      {#if thinking}
        <p class="thinking">{side.live.step?.line || 'Thinking…'}</p>
      {:else if text}
        <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
        <div class="answer" class:streaming={side.answering} data-testid="chat-sideq-answer">{@html renderMarkdown(side.answering ? closePartial(text) : text)}</div>
        {#if side.result?.stopped}<p class="stopped">Stopped</p>{/if}
      {/if}
      {#if side.error}<p class="err" role="alert">{side.error}</p>{/if}
    </div>
  {/if}

  <div class="ask">
    <textarea
      bind:this={inputEl}
      bind:value={side.input}
      use:grow={side.input}
      rows="1"
      placeholder={shown ? 'Ask another side question' : 'Ask about this chat'}
      aria-label="Your side question"
      onkeydown={onInputKey}
      data-testid="chat-sideq-input"
    ></textarea>
    {#if side.result?.text && !side.answering}
      <button
        type="button"
        class="keep"
        disabled={busy || side.keeping}
        title={busy ? KEEP_LATER : 'Add the question and answer to the chat'}
        onclick={keep}
        data-testid="chat-sideq-keep"
      >Keep it</button>
    {/if}
    <button type="button" class="send" disabled={!side.input.trim()} aria-label="Ask" title="Ask (Enter)" onclick={() => side.ask()} data-testid="chat-sideq-send">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
    </button>
  </div>
  {#if busy && side.result?.text && !side.answering}
    <p class="later" data-testid="chat-sideq-later">{KEEP_LATER}</p>
  {/if}
</div>

<style>
  .side {
    position: absolute;
    left: 50%;
    z-index: 30;
    transform: translateX(-50%);
    box-sizing: border-box;
    width: min(560px, calc(100% - 40px));
    max-height: calc(100% - 120px);
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 10px 10px 14px;
    border: 1px solid var(--yap-border);
    border-radius: 14px;
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-popover);
    color: var(--yap-fg);
    animation: rise 160ms ease-out;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 6px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .side {
      animation: none;
    }
  }
  :global(:root[data-motion='reduced']) .side {
    animation: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .title {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-80);
    white-space: nowrap;
  }
  .title svg {
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .note {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .x {
    display: inline-flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .x svg {
    width: 13px;
    height: 13px;
  }
  .body {
    min-height: 0;
    overflow-y: auto;
    padding-right: 4px;
    font-size: 14px;
    line-height: 1.55;
  }
  .asked {
    margin: 0 0 6px;
    font-size: 13px;
    color: var(--yap-fg-62);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .answer {
    overflow-wrap: anywhere;
  }
  .answer > :global(:first-child) {
    margin-top: 0;
  }
  .answer > :global(:last-child) {
    margin-bottom: 0;
  }
  .answer.streaming > :global(:is(p, li):last-child)::after {
    content: '';
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-left: 5px;
    border-radius: 50%;
    background: var(--yap-primary);
    vertical-align: middle;
  }
  .thinking,
  .stopped,
  .later {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .err {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--yap-danger);
  }
  .ask {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 4px 4px 4px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s2);
  }
  .ask:focus-within {
    border-color: var(--yap-border-hover);
  }
  textarea {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 26px;
    max-height: 120px;
    padding: 4px 0;
    border: none;
    outline: none;
    resize: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 14px;
    line-height: 1.4;
  }
  textarea::placeholder {
    color: var(--yap-muted);
  }
  .send {
    display: inline-flex;
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    cursor: pointer;
  }
  .send:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .send:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .send svg {
    width: 14px;
    height: 14px;
  }
  .keep {
    flex: 0 0 auto;
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-bg);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }
  .keep:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    background: var(--yap-s1);
  }
  .keep:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .x:focus-visible,
  .send:focus-visible,
  .keep:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-focus));
    outline-offset: 2px;
  }
</style>
