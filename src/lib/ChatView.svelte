<script>
  // AI Chat — the Chat surface, ported from OpenWhispr's chat/ChatView.tsx +
  // ConversationList: two panes (conversation sidebar w/ Today / Yesterday /
  // Previous 7 Days / Older grouping + the thread), conversations created on
  // first message (title = first 50 chars), last-20-turn context, and **eager
  // keyword-RAG** over the notes library (chat_send injects top-5 note
  // snippets under their exact framing). Ctrl+N = new chat.
  //
  // A model that can call tools (tools.rs) looks notes up and changes them;
  // each answer shows what it did as chips above it ("Edited "Launch plan":
  // "Friday." → "Monday.""), kept with the message in chats.json. v1
  // divergences (ROADMAP "AI Chat" escalating plan): no streaming (one
  // response per turn, "Thinking…" placeholder), no conversation
  // search/archive/rename.
  //
  // The composer's model chip (ModelChip.svelte) says which model the next
  // message goes to and switches the Chat scope's; each answer keeps the
  // model that wrote it (chats.rs `model`), shown quietly under it.
  //
  // Spoken replies (tts.rs, Settings → Voice Agent): the 🔊 / 🔇 next to the
  // chip turns "Speak replies" on and off (voice_replies_set; Settings'
  // copy adopts it from `yap-voice-replies`); every answer has ▶ Read aloud
  // (■ Stop while it's read, "Speaking…" on it); with replies on, an answer
  // is read by itself as "When to speak" says (chat_send's `speak`: always,
  // or only for a question that came by voice — kept per message, `voice`).
  // A question dictated into the box (the mic, or the hotkey while the box
  // has the focus) arrives as `yap-chat-dictation`, never as a paste, and
  // is sent at once when Rust says so ("Talk back hands-free", or "press
  // enter" said). Esc, leaving Chat, sending the next question and any
  // dictation starting (Rust's barge-in) stop the voice.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import { renderMarkdown } from './markdown.js';
  import { toast } from './ui/toast.svelte.js';
  import { chatRequest } from './chatRequest.svelte.js';
  import ModelChip from './ModelChip.svelte';
  import { modelLine, whereItRuns } from './aiModel.js';
  import { speech, initSpeech, readAloud as speakText, stopReading as stopSpeech } from './speech.svelte.js';

  // Opens a Settings section (ControlPanel's openSettings): the chip's
  // "Language model settings…".
  let { onopensettings = null } = $props();

  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // { role, text, tools?, model?, voice?, id }
  let input = $state('');
  let sending = $state(false);
  let inputEl = $state(null);
  let threadEl = $state(null);
  // 'meetings' when the Meetings view's Ask bar started this chat: answers
  // are grounded in your meeting notes (calendar::meetings_context).
  let scope = $state(null);

  // ---- Spoken replies
  let speakReplies = $state(false);
  let speakWhen = $state('voice');
  // The message being read aloud ("chat:<conversation>:<index>"), if any.
  initSpeech(['chat:']);
  const speakingId = $derived(speech.speakingId);
  // What's in the box came (at least partly) by voice: the question is
  // "asked by voice" (chat_send `voice`).
  let byVoice = false;
  // A hands-free question arrived while an answer was still coming: sent
  // once it's in.
  let sendWhenFree = false;
  let alive = true;

  const chatId = (conv, i) => `chat:${conv}:${i}`;
  const voiceTitle = $derived(
    speakReplies
      ? speakWhen === 'always'
        ? 'Voice replies on: every answer is read aloud'
        : 'Voice replies on: answers to questions you ask by voice are read aloud'
      : 'Voice replies off'
  );

  async function loadVoiceSettings() {
    try {
      const c = await invoke('get_config');
      speakReplies = !!c?.speakReplies;
      speakWhen = c?.speakWhen || 'voice';
    } catch {
      /* keep */
    }
  }

  async function toggleVoiceReplies() {
    const on = !speakReplies;
    speakReplies = on;
    try {
      await invoke('voice_replies_set', { on });
    } catch (e) {
      speakReplies = !on;
      toast({ title: "Couldn't change voice replies", description: String(e), variant: 'destructive' });
    }
  }

  function readAloud(m) {
    if (m.id) speakText(m.text, m.id);
  }

  function stopReading() {
    stopSpeech('chat:');
  }

  // A dictated question (pipeline.rs `deliver_to_chat`): into the box at
  // the caret, then sent when Rust says so.
  async function onChatDictation(p) {
    const text = (p?.text || '').trim();
    if (text) {
      insertAtCaret(text);
      byVoice = true;
    }
    if (p?.send && input.trim()) {
      if (sending) sendWhenFree = true;
      else send();
    } else {
      await tick();
      inputEl?.focus();
    }
  }

  function insertAtCaret(text) {
    const el = inputEl;
    const focused = el && document.activeElement === el;
    const start = focused ? (el.selectionStart ?? input.length) : input.length;
    const end = focused ? (el.selectionEnd ?? start) : input.length;
    const before = input.slice(0, start);
    const after = input.slice(end);
    const lead = before && !/\s$/.test(before) ? ' ' : '';
    const trail = after && !/^[\s.,;:!?)]/.test(after) ? ' ' : '';
    input = before + lead + text + trail + after;
    const caret = (before + lead + text).length;
    tick().then(() => {
      if (!el) return;
      el.focus();
      el.setSelectionRange(caret, caret);
    });
  }

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
      stopReading();
      activeId = id;
      scope = null;
      messages = (conv.messages || []).map((m, i) => ({ ...m, id: chatId(id, i) }));
      scrollThread();
    } catch (e) {
      toast({ title: "Couldn't open conversation", description: String(e), variant: 'destructive' });
    }
  }

  function newChat() {
    stopReading();
    activeId = null;
    messages = [];
    input = '';
    byVoice = false;
    scope = null;
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
    const voice = byVoice;
    input = '';
    byVoice = false;
    // The next question: the last answer stops talking.
    stopReading();
    messages = [...messages, { role: 'user', text, voice, id: null }];
    sending = true;
    scrollThread();
    try {
      const res = await invoke('chat_send', { conversationId: activeId, text, scope, voice });
      if (!alive) return;
      activeId = res.conversationId;
      const asked = messages[messages.length - 1];
      const answer = {
        role: 'assistant',
        text: res.reply,
        tools: res.toolsUsed || [],
        model: res.model ?? null,
        id: res.messageId ?? null,
      };
      messages = [...messages.slice(0, -1), { ...asked, id: res.userMessageId ?? null }, answer];
      refreshList();
      scrollThread();
      // Read out by itself ("When to speak", decided in Rust).
      if (res.speak && answer.id) speakText(answer.text, answer.id);
    } catch (e) {
      if (!alive) return;
      messages = messages.slice(0, -1);
      input = text; // give the message back
      byVoice = voice;
      toast({ title: "Couldn't send", description: String(e), variant: 'destructive' });
    } finally {
      sending = false;
      if (alive && sendWhenFree) {
        sendWhenFree = false;
        send();
      }
    }
  }

  // The mic: a dictation into the chat box (chat_dictate: the text comes
  // back as `yap-chat-dictation`, at the caret). Again: stop and send it in.
  function micChat() {
    inputEl?.focus();
    invoke('chat_dictate').catch(() => {});
  }

  function onKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      newChat();
    } else if (e.key === 'Escape' && speakingId?.startsWith('chat:')) {
      e.preventDefault();
      stopReading();
    }
  }

  // Rust decides whether a dictation is for the chat: the box with the
  // focus is one way (the hotkey in Yap's window, the bar's Dictate).
  function reportFocus(focused) {
    invoke('chat_input_focus', { focused }).catch(() => {});
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
    loadVoiceSettings();
    setTimeout(() => inputEl?.focus(), 0);
    const uns = [
      listen('yap-chat-dictation', (e) => onChatDictation(e.payload)),
      listen('yap-voice-replies', (e) => {
        if (typeof e.payload?.on === 'boolean') speakReplies = e.payload.on;
      }),
      // Settings changed "Speak replies" or "When to speak".
      listen('yap-config-saved', loadVoiceSettings),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });

  // Leaving Chat: its voice stops, and its box no longer has the focus.
  onDestroy(() => {
    alive = false;
    stopReading();
    reportFocus(false);
  });

  // A question handed over by another view (the Meetings view's Ask bar):
  // ask it in a new chat.
  $effect(() => {
    const req = chatRequest.pending;
    if (!req) return;
    chatRequest.pending = null;
    untrack(() => {
      if (req.dictated) {
        onChatDictation(req);
        return;
      }
      newChat();
      scope = req.scope ?? null;
      input = req.text;
      send();
    });
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
          {#if m.role === 'assistant' && m.tools?.length}
            <!-- Tool-activity chips (OpenWhispr shows tool calls above the answer) -->
            <div class="toolchips">
              {#each m.tools as t, j (j)}
                <span class="toolchip">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4.5 4.5 0 0 0 5.8 5.8L17 15.6a4.5 4.5 0 0 1-5.8-5.8z" transform="rotate(90 12 12)" /><path d="m3 21 6.5-6.5" /><path d="M14.7 6.3 21 3l-3.3 6.3" /></svg>
                  {t}
                </span>
              {/each}
            </div>
          {/if}
          {@const reading = m.role === 'assistant' && !!m.id && speakingId === m.id}
          <div class="cbubble {m.role}" class:reading>
            {#if m.role === 'assistant'}
              <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown escapes all input -->
              <div class="cbody">{@html renderMarkdown(m.text)}</div>
            {:else}
              <p class="cbody">
                {#if m.voice}
                  <span class="viavoice" title="Asked by voice" aria-label="Asked by voice">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
                  </span>
                {/if}{m.text}
              </p>
            {/if}
          </div>
          {#if m.role === 'assistant'}
            <div class="msgfoot">
              {#if m.id}
                <button
                  class="readbtn"
                  class:on={reading}
                  aria-label={reading ? 'Stop reading' : 'Read aloud'}
                  title={reading ? 'Stop reading (Esc)' : 'Read aloud'}
                  onclick={() => readAloud(m)}
                >
                  {#if reading}
                    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="5" y="5" width="14" height="14" rx="2.5" /></svg>
                  {:else}
                    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.9v14.2a1.2 1.2 0 0 0 1.8 1l11.6-7.1a1.2 1.2 0 0 0 0-2L8.8 3.9A1.2 1.2 0 0 0 7 4.9z" /></svg>
                  {/if}
                </button>
              {/if}
              {#if reading}
                <span class="speaking" role="status">
                  <span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>
                  Speaking…
                </span>
              {:else if m.model}
                <!-- The model that wrote it (older answers don't say). -->
                <span class="byline" title={whereItRuns(m.model)}>{modelLine(m.model)}</span>
              {/if}
            </div>
          {/if}
        {/each}
        {#if sending}
          <div class="cbubble assistant"><p class="cbody thinking">Thinking…</p></div>
        {/if}
      </div>
    {/if}

    <!-- Composer: the message on top; the mic, the model it goes to and
         Send under it. -->
    <div class="inputbar">
      <input
        class="msginput"
        bind:this={inputEl}
        bind:value={input}
        placeholder="Type a message..."
        aria-busy={sending}
        onkeydown={(e) => e.key === 'Enter' && !e.isComposing && send()}
        oninput={() => {
          if (!input.trim()) byVoice = false;
        }}
        onfocus={() => reportFocus(true)}
        onblur={() => reportFocus(false)}
      />
      <!-- Not `disabled` while an answer comes: disabling the focused box
           throws the caret out of it, and you'd have to click back in to
           type the next message. `send()` ignores Enter until the answer is
           in, so you can type ahead. -->
      <div class="composerrow">
        <button class="micbtn" title="Dictate your message" aria-label="Dictate your message" onclick={micChat}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
        </button>
        <ModelChip {onopensettings} />
        <!-- Spoken replies on / off (Settings → Voice Agent → Speak replies). -->
        <button
          class="voicebtn"
          class:on={speakReplies}
          aria-pressed={speakReplies}
          aria-label="Voice replies"
          title={voiceTitle}
          onclick={toggleVoiceReplies}
        >
          {#if speakReplies}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" fill="currentColor" fill-opacity="0.15" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.4 5.6a9 9 0 0 1 0 12.8" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" /><path d="m22 9-6 6M16 9l6 6" /></svg>
          {/if}
          <span class="vlabel">Voice replies</span>
        </button>
        <span class="spacer"></span>
        <button class="sendbtn" disabled={sending || !input.trim()} onclick={send} aria-label="Send">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m22 2-7 20-4-9-9-4z" /><path d="M22 2 11 13" /></svg>
        </button>
      </div>
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
  .toolchips {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    max-width: 78%;
  }
  .toolchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font-size: 10.5px;
    color: var(--yap-muted);
  }
  .toolchip svg {
    width: 10px;
    height: 10px;
    flex: 0 0 auto;
    color: var(--yap-primary);
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

  .cbubble.reading {
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
  }
  .viavoice {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 5px;
    color: var(--yap-primary);
  }
  .viavoice svg {
    width: 11px;
    height: 11px;
  }
  /* Under an answer: ▶ Read aloud / ■ Stop, then the model or "Speaking…". */
  .msgfoot {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 20px;
    margin: -4px 0 2px 2px;
  }
  .readbtn {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .readbtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .readbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .readbtn.on {
    color: var(--yap-primary);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
    background: var(--yap-primary-wash);
  }
  .readbtn svg {
    width: 10px;
    height: 10px;
  }
  .speaking {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-primary);
  }
  .bars {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 10px;
  }
  .bars i {
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
    animation: talk 0.9s ease-in-out infinite;
  }
  .bars i:nth-child(2) {
    animation-delay: 0.15s;
  }
  .bars i:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes talk {
    0%,
    100% {
      transform: scaleY(0.35);
    }
    50% {
      transform: scaleY(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bars i {
      animation: none;
      transform: scaleY(0.7);
    }
  }
  .byline {
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }

  .voicebtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 9px 0 7px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    flex: 0 0 auto;
    transition:
      border-color var(--yap-dur),
      background var(--yap-dur),
      color var(--yap-dur);
  }
  .voicebtn:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .voicebtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .voicebtn.on {
    color: var(--yap-fg);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
    background: var(--yap-primary-wash);
  }
  .voicebtn.on svg {
    color: var(--yap-primary);
  }
  .voicebtn svg {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
  }
  .vlabel {
    font-weight: 600;
  }

  .inputbar {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
    padding: 8px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .inputbar:focus-within {
    border-color: var(--yap-border-hover);
  }
  .composerrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .spacer {
    flex: 1 1 auto;
  }
  .micbtn {
    display: inline-flex;
    width: 28px;
    height: 28px;
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
    padding: 0 6px;
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
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    cursor: pointer;
    flex: 0 0 auto;
  }
  .sendbtn:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
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
