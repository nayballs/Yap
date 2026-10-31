<script>
  // AI Chat — the Chat surface, ported from OpenWhispr's chat/ChatView.tsx +
  // ConversationList: two panes (conversation sidebar w/ Today / Yesterday /
  // Previous 7 Days / Older grouping + the thread), conversations created on
  // first message (title = first 50 chars), last-20-turn context, and **eager
  // keyword-RAG** over the notes library (chat_send injects top-5 note
  // snippets under their exact framing). Ctrl+N = new chat.
  //
  // A model that can call tools (chat/tools.rs) looks notes up and changes them;
  // each answer shows what it did as chips above it ("Edited "Launch plan":
  // "Friday." → "Monday.""), kept with the message in chats.json. No
  // conversation search/archive yet.
  //
  // Streamed (chat/stream.rs, core/streaming.js; 2026-10-22): the answer grows in
  // place as it's written — markdown rendered as it comes (closePartial), a
  // tool call shown as its chip ("Searching the web…") while it runs, the
  // ■ in the composer stopping it (what came stays, checked like any
  // answer, marked Stopped). At the end the CHECKED answer (chat_send's
  // reply: no made-up sources list, no claimed search, no citation with
  // nothing behind it) takes the streamed text's place: the same text
  // stays put; a changed one glides to its new height and fades in
  // (`settle`). A question asked out loud streams here too
  // (`yap-assistant-stream`).
  //
  // Right-click menus (ui/contextMenu.svelte.js, as in Notes): a
  // conversation (Rename, Copy as Markdown, Delete…) and a message (Copy /
  // Copy as Markdown / Read aloud; a question: Copy).
  //
  // The composer's model chip (ModelChip.svelte) says which model the next
  // message goes to and switches the Chat scope's; each answer keeps the
  // model that wrote it (chat/chats.rs `model`), shown quietly under it.
  //
  // Spoken replies (voice/tts.rs, Settings → Voice Assistant): the composer's +
  // turns "Speak replies" on and off (voice_replies_set; Settings' copy
  // adopts it from `yap-voice-replies`); every answer has ▶ Read aloud
  // (■ Stop while it's read, "Speaking…" on it); with replies on, an answer
  // is read by itself as "When to speak" says (chat_send's `speak`: always,
  // or only for a question that came by voice — kept per message, `voice`).
  // A question dictated into the box (the mic, or the hotkey while the box
  // has the focus) arrives as `yap-chat-dictation`, never as a paste, and
  // is sent at once when Rust says so ("Talk back hands-free", or "press
  // enter" said). Esc, leaving Chat, sending the next question and any
  // dictation starting (Rust's barge-in) stop the voice.
  //
  // The Voice Assistant (chat/assistant.rs): a question asked out loud with its
  // shortcut, from any app, lands in Chat — the conversation open here, else
  // the last one if it's recent, else a new one — so this view tells Rust
  // what it shows (`chat_view_state`) and follows the question live
  // (`yap-assistant-turn`: the question, marked asked by voice, then
  // "Thinking…", then the answer, which Rust reads aloud). With no shortcut
  // set, the empty thread says how to set one.
  //
  // A newer question replaces an older one still being answered in the
  // same conversation (chat/stream.rs `Halt::Superseded`, a tester's voice chat,
  // 2026-10-23): asked out loud, typed here while a spoken one is answered,
  // or dictated here and sent by itself while this view's answer comes
  // (`replaceAnswer`). The older turn saves no answer: its bubble goes, its
  // question stays (the model reads it with the newer one), and no error
  // shows (`SUPERSEDED`, phase `superseded`). Each spoken question's events
  // carry its id (`userMessageId`), so an older one's never land in the
  // newer one's bubble.
  //
  // The web (web.rs, "Look things up on the web" in Settings → Voice
  // Assistant): an answer written from web pages cites them as [1], drawn
  // as small pills (core/markdown.js), with a Sources row under it — number,
  // title, site — that opens each in the browser (open_external). Kept with
  // the message (chat/chats.rs `sources`). Its search / read chips carry a globe,
  // and an action refused after reading the web a shield.
  //
  // Snip to Chat (snip.rs, 2026-10-23): a snip lands here (`yap-snip`, or
  // ControlPanel's chatRequest when Chat wasn't open) in the conversation
  // Rust chose — the one open here, if the window was on screen on one; else
  // a new chat — as a thumbnail above the box, the caret in the box: type or
  // dictate a question, tap one of the three prompts, or press Enter on an
  // empty box for "What's this?". Nothing is ever sent by itself. A model
  // that can't read images says so above the box with a one-click switch
  // (ai_model_status `images` / `imagesSwitch`, snip/vision.rs); Send waits
  // meanwhile. The question keeps its screenshots (chat/chats.rs `images`): a
  // thumbnail in its bubble, or "Screenshot removed after 30 days" once
  // "Keep chat screenshots" has removed it. + → "Take a screenshot" snips too.
  // Phase 2 (2026-10-25): a picture pasted into the box (Ctrl+V) or files
  // dropped on Chat join the screenshots (snip/attach.rs); a screenshot's
  // right-click menu copies it (out of Win+V history) or saves it; a model
  // that can't read images is also offered "Send its text instead"
  // (Windows' OCR on this PC, snip/ocr.rs: the text folds under the
  // question in its bubble).
  //
  // Documents beside the chat (chat/documents.rs, 2026-10-30): an answer to
  // keep (a build, a plan, a guide) is written as a document that opens in
  // a panel on the right (DocPanel, chatDoc.svelte.js) and fills section by
  // section; a pill under the answer reopens it. The split between the
  // conversation and the panel is dragged (or moved with the arrow keys on
  // its handle) and remembered; with no room beside the conversation (a
  // narrow window) the panel covers it, ✕ going back.
  //
  // This file is the view: it makes its state (chatState.svelte.js), follows
  // Rust's events while Chat is open, takes the keys, and lays out the
  // conversation list (ConversationList) beside the thread (ChatThread,
  // ChatMessage) and the composer (Composer), and the document panel.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { onMount, onDestroy, untrack } from 'svelte';
  import { chatRequest } from '../../stores/chatRequest.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { createChatState } from './chatState.svelte.js';
  import { chatImageUrl, imageItems } from './chatImages.js';
  import ConversationList from './ConversationList.svelte';
  import ChatThread from './ChatThread.svelte';
  import Composer from './Composer.svelte';
  import { MIN_WIDTH, THREAD_MIN, LIST_STAYS } from './chatDoc.svelte.js';

  // Opens a Settings section (ControlPanel's openSettings): the chip's
  // "Language model settings…". `onopennote({id, line, ts, field})`: a
  // passage an answer cites, opened in Notes at its line (ControlPanel).
  // `onopenusage()`: the context ring's "See all" (Insights' AI usage card).
  let { onopensettings = null, onopennote = null, onopenusage = null } = $props();

  const chat = createChatState({ onopensettings: () => onopensettings, onopennote: () => onopennote });
  const doc = chat.doc;

  // Chat's own width: too narrow for the document beside the conversation,
  // it covers it instead.
  let chatEl = $state(null);
  let chatWidth = $state(0);
  // The conversation list's own width (a long title widens it). While a
  // document is open beside the chat, the list steps aside unless Chat is
  // wide: the conversation on the left, the document on the right ("Show
  // your chats" over the thread brings it back; so does ✕).
  let listWidth = $state(224);
  let listWanted = $state(false);
  const listAside = $derived(doc.open && !doc.expanded && chatWidth < LIST_STAYS && !listWanted);
  // Asked back while one document was open: until it closes.
  $effect(() => {
    if (!doc.open) listWanted = false;
  });
  // The most the panel may take: the list (when it shows) and the thread keep theirs.
  const room = $derived(chatWidth - (listAside ? 0 : listWidth) - THREAD_MIN);
  const narrow = $derived(chatWidth > 0 && chatWidth - THREAD_MIN < MIN_WIDTH);
  const cover = $derived(doc.open && (narrow || doc.expanded));
  const maxWidth = $derived(Math.max(MIN_WIDTH, room));
  // Its width now: the remembered one, within what there's room for.
  const shownWidth = $derived(Math.min(doc.width, maxWidth));
  $effect(() => {
    const el = chatEl;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const list = el.querySelector('.convs');
    const measure = () => {
      chatWidth = el.clientWidth;
      // (Not while it's stepped aside: it keeps its last width.)
      const w = list?.getBoundingClientRect().width;
      if (w) listWidth = w;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (list) ro.observe(list);
    measure();
    return () => ro.disconnect();
  });

  // The split's handle: dragged, or the arrow keys on it.
  let dragging = false;
  function splitDown(e) {
    if (e.button !== 0) return;
    dragging = true;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function splitMove(e) {
    if (!dragging || !chatEl) return;
    const right = chatEl.getBoundingClientRect().right;
    doc.setWidth(right - e.clientX, maxWidth);
  }
  function splitUp() {
    if (!dragging) return;
    dragging = false;
    doc.keepWidth();
  }
  function splitKey(e) {
    const step = e.shiftKey ? 80 : 24;
    if (e.key === 'ArrowLeft') doc.setWidth(shownWidth + step, maxWidth);
    else if (e.key === 'ArrowRight') doc.setWidth(shownWidth - step, maxWidth);
    else if (e.key === 'Home') doc.setWidth(maxWidth, maxWidth);
    else if (e.key === 'End') doc.setWidth(MIN_WIDTH, maxWidth);
    else return;
    e.preventDefault();
    doc.keepWidth();
  }

  function onKeydown(e) {
    // The document covering the conversation (expanded): Esc puts it back beside it.
    if (e.key === 'Escape' && doc.open && doc.expanded && !narrow && !e.defaultPrevented) {
      doc.expanded = false;
      return;
    }
    if (e.key === 'Escape' && chat.viewing) {
      chat.viewing = null;
      return;
    }
    if (e.key === 'Escape' && chat.plusOpen) {
      chat.plusOpen = false;
      return;
    }
    // A spoken question being listened for (after saying "snip", say): Esc
    // calls it off, as ✕ on the capsule does; the screenshot stays in the box.
    if (e.key === 'Escape' && chat.assistantListening && !e.defaultPrevented) {
      e.preventDefault();
      chat.assistantListening = false;
      invoke('cancel_recording').catch(() => {});
      return;
    }
    if (e.key === 'Escape' && chat.picked.length && !e.defaultPrevented) {
      chat.clearPicked();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      chat.newChat();
    } else if (e.key === 'Escape' && chat.speakingId?.startsWith('chat:')) {
      e.preventDefault();
      chat.stopReading();
    } else if (e.key === 'Escape' && ((chat.sending && chat.streaming) || chat.assistantAnswering) && !e.defaultPrevented) {
      e.preventDefault();
      chat.stopAnswer();
    }
  }

  // Ctrl+V of a picture anywhere in Chat but another text field (a thumbnail
  // or the thread has the focus): into the box too. The box's own paste
  // handled it already when it had the focus.
  function onPaste(e) {
    if (e.defaultPrevented) return;
    if (e.target?.closest?.('input, textarea, [contenteditable="true"], [contenteditable=""]') && e.target !== chat.inputEl) return;
    chat.pasteImage(e);
  }

  onMount(() => {
    chat.refreshList();
    chat.loadVoiceSettings();
    chat.refreshVision();
    chat.refreshLimits();
    // Opened while the Voice Assistant thinks or speaks: its ■ shows here too.
    invoke('assistant_phase').then((p) => (chat.assistantPhase = p)).catch(() => {});
    setTimeout(() => chat.inputEl?.focus(), 0);
    const uns = [
      // A snip landed while Chat is open (snip.rs).
      listen('yap-snip', (e) => chat.takeSnip(e.payload)),
      // Another Chat model: can it read the screenshot? And what may a
      // question be for it (chat/budget.rs)?
      listen('yap-chat-model-changed', () => {
        chat.refreshVision();
        chat.refreshLimits();
      }),
      // The on-device model's context changed (Give it more room, Settings).
      listen('yap-local-llm-changed', chat.refreshLimits),
      // A long chat's older messages were summarised (chat/compact.rs).
      listen('yap-chat-summary', (e) => chat.onSummary(e.payload)),
      listen('yap-chat-images-deleted', chat.reloadActive),
      // The document beside the chat, changed in another window or by Chat's
      // tools (its sections landing), or deleted.
      listen('yap-note-changed', (e) => doc.onNoteChanged(e.payload)),
      listen('yap-note-deleted', (e) => doc.onNoteDeleted(e.payload)),
      listen('yap-chat-dictation', (e) => chat.onChatDictation(e.payload)),
      // A question asked with the assistant shortcut, live (and its card's
      // "Open in Chat").
      listen('yap-assistant-turn', (e) => chat.followAssistant(e.payload)),
      listen('yap-assistant-stream', (e) => chat.followAssistantStream(e.payload)),
      // Whether a spoken question is being listened for (Esc calls it off).
      listen('yap-assistant', (e) => {
        chat.assistantListening = e.payload?.phase === 'listening';
        chat.assistantPhase = e.payload?.phase;
      }),
      listen('yap-voice-replies', (e) => {
        if (typeof e.payload?.on === 'boolean') chat.speakReplies = e.payload.on;
      }),
      listen('yap-voice-volume', (e) => {
        if (Number.isFinite(e.payload?.volume)) chat.voiceVolume = e.payload.volume;
      }),
      // Settings changed "Speak replies" or "When to speak" (or the Chat
      // model, or how long screenshots are kept).
      listen('yap-config-saved', () => {
        chat.loadVoiceSettings();
        chat.refreshVision();
        chat.refreshLimits();
      }),
      // Pictures dropped on Chat (Windows' drag and drop: Tauri reports the
      // files' paths) go in the box like a snip (snip/attach.rs).
      getCurrentWebview().onDragDropEvent((event) => {
        const t = event.payload.type;
        if (t === 'enter' || t === 'over') chat.dragging = true;
        else if (t === 'leave') chat.dragging = false;
        else if (t === 'drop') chat.dropFiles(event.payload.paths || []);
      }),
    ];
    return () => uns.forEach((u) => u.then((f) => f()).catch(() => {}));
  });

  // Leaving Chat: its voice stops, and its box no longer has the focus.
  onDestroy(() => {
    chat.close();
    chat.stopReading();
    chat.reportFocus(false);
    chat.reportView(false);
  });

  // A question handed over by another view (the Meetings view's Ask bar):
  // ask it in a new chat.
  $effect(() => {
    const req = chatRequest.pending;
    if (!req) return;
    chatRequest.pending = null;
    untrack(() => {
      // A snip that landed while Chat wasn't open (ControlPanel brought it up).
      if (req.snip) {
        chat.takeSnip(req.snip);
        return;
      }
      if (req.dictated) {
        chat.onChatDictation(req);
        return;
      }
      // A question asked out loud while Chat wasn't open (ControlPanel
      // brought it up): its conversation, as it goes.
      if (req.assistant) {
        chat.followAssistant(req.assistant);
        return;
      }
      chat.newChat();
      chat.scope = req.scope ?? null;
      chat.input = req.text;
      // A one-tap prompt or a "/" Skill from the Meetings view's Ask bar.
      chat.send(req.skill ? { skill: req.skill } : undefined);
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpaste={onPaste} />

<div class="chat" bind:this={chatEl}>
  <!-- Conversation sidebar (out of reach while the document covers it) -->
  <div class="listslot" class:aside={listAside} inert={cover || listAside || undefined}>
    <ConversationList {chat} />
  </div>

  <!-- Thread -->
  <section class="thread" class:withbtn={listAside} inert={cover || undefined}>
    {#if listAside}
      <!-- The list stepped aside for the document: back with one click. -->
      <button type="button" class="showchats" onclick={() => (listWanted = true)} aria-label="Show your chats" title="Show your chats">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></svg>
      </button>
    {/if}
    <ChatThread {chat} {onopensettings} />
    <Composer {chat} {onopensettings} {onopenusage} />
  </section>

  {#if doc.open}
    {#if !cover}
      <!-- The split: drag it, or the arrow keys (Shift: further). -->
      <div
        class="split"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the document"
        aria-valuenow={shownWidth}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={maxWidth}
        tabindex="0"
        onpointerdown={splitDown}
        onpointermove={splitMove}
        onpointerup={splitUp}
        onpointercancel={splitUp}
        onkeydown={splitKey}
      ></div>
    {/if}
    <div class="docslot" class:cover style={cover ? undefined : `width: ${shownWidth}px`}>
      <!-- Its own chunk, loaded the first time a document opens. -->
      {#await import('./DocPanel.svelte') then { default: DocPanel }}
        <DocPanel {doc} {cover} {narrow} onquote={chat.setQuote} />
      {/await}
    </div>
  {/if}
</div>

{#if chat.viewing}
  <!-- A screenshot opened large: a click or Esc closes it. -->
  <!-- Its right-click menu copies or saves it. -->
  <div class="shotview" role="dialog" aria-label="Screenshot" aria-modal="true" tabindex="-1" onclick={() => (chat.viewing = null)} onkeydown={(e) => e.key === 'Escape' && (chat.viewing = null)}>
    <img
      src={chatImageUrl(chat.viewing)}
      alt="Screenshot"
      oncontextmenu={(e) => openContextMenu(e, imageItems({ id: chat.viewing }), { label: 'Screenshot' })}
    />
  </div>
{/if}

<style>
  .chat {
    position: relative;
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }
  .listslot.aside {
    display: none;
  }
  .listslot {
    display: contents;
  }
  /* The document beside the conversation, its width the split's. */
  .docslot {
    flex: 0 0 auto;
    display: flex;
    min-height: 0;
  }
  .docslot > :global(.docpanel) {
    flex: 1 1 auto;
  }
  .docslot.cover {
    position: absolute;
    inset: 0;
    z-index: 5;
  }
  .split {
    flex: 0 0 6px;
    margin: 0 -3px;
    position: relative;
    z-index: 2;
    cursor: col-resize;
    touch-action: none;
  }
  .split::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 2px;
    width: 2px;
    background: transparent;
    transition: background 0.15s;
  }
  .split:hover::after,
  .split:focus-visible::after {
    background: var(--yap-primary-line);
  }
  .split:focus-visible {
    outline: none;
  }
  .thread {
    position: relative;
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 16px 20px;
  }
  .thread.withbtn {
    padding-left: 44px;
  }
  .showchats {
    position: absolute;
    top: 10px;
    left: 8px;
    z-index: 3;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-muted);
    cursor: pointer;
  }
  .showchats:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .showchats:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .showchats svg {
    width: 15px;
    height: 15px;
  }
  .shotview {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 32px;
    background: var(--yap-scrim);
    cursor: zoom-out;
  }
  .shotview img {
    max-width: 100%;
    max-height: 100%;
    border-radius: var(--yap-r);
    box-shadow: var(--yap-shadow-modal);
    background: var(--yap-s1);
  }
</style>
