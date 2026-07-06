<script>
  // AI Chat — the Chat surface, ported from OpenWhispr's chat/ChatView.tsx +
  // ConversationList: two panes (conversation sidebar w/ Today / Yesterday /
  // Previous 7 Days / Older grouping + the thread), conversations created on
  // first message (title = first 50 chars), last-20-turn context, and **eager
  // keyword-RAG** over the notes library (chat_send injects top-5 note
  // snippets under their exact framing). Ctrl+N = new chat.
  //
  // v1 divergences (ROADMAP "AI Chat" escalating plan): no streaming (one
  // response per turn, "Thinking…" placeholder), no tool-calling loop yet, no
  // conversation search/archive/rename.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { renderMarkdown } from './markdown.js';
  import { toast } from './ui/toast.svelte.js';

  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // { role, text }
  let input = $state('');
  let sending = $state(false);
  let inputEl = $state(null);
  let threadEl = $state(null);

  async function refreshList() {
    try {
      conversations = (await invoke('chats_list')) || [];
    } catch {
      conversations = [];
    }
  }

  async function selectConversation(id) {
    if (id === activeId) return;
    try {
      const conv = await invoke('chat_get', { id });
      activeId = id;
      messages = conv.messages || [];
      scrollThread();
    } catch (e) {
      toast({ title: "Couldn't open conversation", description: String(e), variant: 'destructive' });
    }
  }

  function newChat() {
    activeId = null;
    messages = [];
    input = '';
    setTimeout(() => inputEl?.focus(), 0);
  }

  async function removeConversation(id, e) {
    e?.stopPropagation();
    try {
      await invoke('chat_delete', { id });
      if (activeId === id) newChat();
      refreshList();
    } catch {
      /* ignore */
    }
  }

  function scrollThread() {
    setTimeout(() => {
      if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
    }, 0);
  }

  async function send() {
    const text = input.trim();
    if (!text || sending) return;
    input = '';
    messages = [...messages, { role: 'user', text }];
    sending = true;
    scrollThread();
    try {
      const res = await invoke('chat_send', { conversationId: activeId, text });
      activeId = res.conversationId;
      messages = [...messages, { role: 'assistant', text: res.reply }];
      refreshList();
      scrollThread();
    } catch (e) {
      messages = messages.slice(0, -1);
      input = text; // give the message back
      toast({ title: "Couldn't send", description: String(e), variant: 'destructive' });
    } finally {
      sending = false;
    }
  }

  // The mic: focus the input and start dictation — Yap types wherever the
  // caret is, so the chat box IS the dictation target.
  function micChat() {
    inputEl?.focus();
    invoke('toggle_recording').catch(() => {});
  }

  function onKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      newChat();
    }
  }

  // Group conversations OpenWhispr-style: Today / Yesterday / Previous 7 Days / Older.
  const groups = $derived.by(() => {
    const now = new Date();
    const startOfDay = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const today = startOfDay(now);
    const yesterday = today - 86_400_000;
    const week = today - 7 * 86_400_000;
    const buckets = [
      { label: 'Today', items: [] },
      { label: 'Yesterday', items: [] },
      { label: 'Previous 7 Days', items: [] },
      { label: 'Older', items: [] },
    ];
    for (const c of conversations) {
      const t = c.updatedTs * 1000;
      if (t >= today) buckets[0].items.push(c);
      else if (t >= yesterday) buckets[1].items.push(c);
      else if (t >= week) buckets[2].items.push(c);
      else buckets[3].items.push(c);
    }
    return buckets.filter((b) => b.items.length > 0);
  });

  onMount(() => {
    refreshList();
    setTimeout(() => inputEl?.focus(), 0);
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="chat">
  <!-- Conversation sidebar -->
  <aside class="convs">
    <div class="tools">
      <button class="tool" onclick={newChat}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
        New chat
      </button>
    </div>

    {#if conversations.length === 0}
      <p class="convempty">Start your first conversation</p>
    {:else}
      <div class="convscroll">
        {#each groups as g (g.label)}
          <div class="gcap">{g.label}</div>
          {#each g.items as c (c.id)}
            <div
              class="conv"
              class:active={activeId === c.id}
              role="button"
              tabindex="0"
              onclick={() => selectConversation(c.id)}
              onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && selectConversation(c.id)}
            >
              <span class="ctitle">{c.title || 'Untitled'}</span>
              <button
                class="cdel"
                title="Delete"
                aria-label="Delete conversation"
                onclick={(e) => removeConversation(c.id, e)}
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
              </button>
            </div>
          {/each}
        {/each}
      </div>
    {/if}
  </aside>

  <!-- Thread -->
  <section class="thread">
    {#if messages.length === 0}
      <div class="empty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M8 9h8M8 13h5" /></svg>
        <p>Ask about your notes, transcripts, or anything else</p>
      </div>
    {:else}
      <div class="msgs" bind:this={threadEl}>
        {#each messages as m, i (i)}
          <div class="cbubble {m.role}">
            {#if m.role === 'assistant'}
              <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown escapes all input -->
              <div class="cbody">{@html renderMarkdown(m.text)}</div>
            {:else}
              <p class="cbody">{m.text}</p>
            {/if}
          </div>
        {/each}
        {#if sending}
          <div class="cbubble assistant"><p class="cbody thinking">Thinking…</p></div>
        {/if}
      </div>
    {/if}

    <div class="inputbar">
      <button class="micbtn" title="Dictate your message" aria-label="Dictate your message" onclick={micChat}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
      </button>
      <input
        class="msginput"
        bind:this={inputEl}
        bind:value={input}
        placeholder="Type a message..."
        disabled={sending}
        onkeydown={(e) => e.key === 'Enter' && send()}
      />
      <button class="sendbtn" disabled={sending || !input.trim()} onclick={send} aria-label="Send">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m22 2-7 20-4-9-9-4z" /><path d="M22 2 11 13" /></svg>
      </button>
    </div>
  </section>
</div>

<style>
  .chat {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }

  .convs {
    flex: 0 0 224px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s0, var(--yap-s1));
  }
  .tools {
    padding: 12px 8px 4px;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .tool:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .tool svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
  .convempty {
    margin: 10px 18px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .convscroll {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: 0 8px 12px;
  }
  .gcap {
    padding: 12px 10px 4px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--yap-muted-55);
  }
  .conv {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 10px;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .conv:hover {
    background: var(--yap-s2);
  }
  .conv.active {
    background: var(--yap-primary-wash);
  }
  .ctitle {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .conv.active .ctitle {
    font-weight: 600;
  }
  .cdel {
    display: inline-flex;
    width: 20px;
    height: 20px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .conv:hover .cdel {
    opacity: 1;
  }
  .cdel:hover {
    color: #ef4444;
  }
  .cdel svg {
    width: 11px;
    height: 11px;
  }

  .thread {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 16px 20px;
  }
  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
    color: var(--yap-muted-55);
  }
  .empty svg {
    width: 36px;
    height: 36px;
    opacity: 0.7;
  }
  .empty p {
    margin: 0;
    max-width: 220px;
    font-size: 12px;
    line-height: 1.6;
  }
  .msgs {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 2px;
  }
  .cbubble {
    max-width: 78%;
    border-radius: 12px;
    padding: 8px 12px;
    font-size: 12.5px;
    line-height: 1.6;
  }
  .cbubble.user {
    align-self: flex-end;
    background: var(--yap-primary-wash);
  }
  .cbubble.assistant {
    align-self: flex-start;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .cbody {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .cbody.thinking {
    color: var(--yap-muted-55);
    font-style: italic;
  }
  .cbubble.assistant :global(p) {
    margin: 4px 0;
  }
  .cbubble.assistant :global(ul),
  .cbubble.assistant :global(ol) {
    margin: 4px 0;
    padding-left: 18px;
  }
  .cbubble.assistant :global(h2),
  .cbubble.assistant :global(h3) {
    margin: 8px 0 4px;
    font-size: 13px;
  }
  .cbubble.assistant :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    background: var(--yap-s1);
    border-radius: 4px;
    padding: 1px 5px;
  }

  .inputbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding: 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .micbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .micbtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .micbtn svg {
    width: 13px;
    height: 13px;
  }
  .msginput {
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .msginput:focus {
    outline: none;
  }
  .msginput::placeholder {
    color: var(--yap-muted-55);
  }
  .sendbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .sendbtn:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .sendbtn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .sendbtn svg {
    width: 13px;
    height: 13px;
  }
</style>
