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
  // its handle, a double-click back to the default) and remembered; with no
  // room beside the conversation (a narrow window) the panel covers it, ✕
  // going back. The conversation list has its own button (Hide your chats ·
  // Ctrl+B, 2026-11-07, as the Claude app's sidebar), remembered; it steps
  // aside by itself only while a panel needs the room, its button showing
  // (chatLayout.svelte.js, chatRoom.js). Words selected there
  // ("Change this…") wait over the box as a quote, and the next question
  // takes them however it comes: typed, dictated into the box, or asked out
  // loud with Chat in view (Rust keeps a copy, `yap-chat-quote`).
  //
  // The panel is a workspace (2026-10-31, find and show): a tab per document
  // and per picture Chat found online in this conversation (its answers'
  // `pictures`, chat/pictures.rs; a turn's `picture` event opens it on one);
  // a picture's tab fits it at its device pixels, zooms and pans
  // (PictureTab.svelte).
  //
  // This file is the view: it makes its state (chatState.svelte.js), follows
  // Rust's events while Chat is open, takes the keys, and lays out the
  // conversation list (ConversationList) beside the thread (ChatThread,
  // ChatMessage) and the composer (Composer), and the document panel.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { onMount, onDestroy, untrack, tick } from 'svelte';
  import { chatRequest } from '../../stores/chatRequest.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { createChatState } from './chatState.svelte.js';
  import { createCodeChats } from './codeChats.svelte.js';
  import ConversationList from './ConversationList.svelte';
  import ChatThread from './ChatThread.svelte';
  import Composer from './Composer.svelte';
  import ShotViewer from './ShotViewer.svelte';
  import { MIN_WIDTH, tabKeyOf } from './chatDoc.svelte.js';
  import { LIST_MIN } from './chatRoom.js';
  import { createChatLayout } from './chatLayout.svelte.js';
  import { chatBrowser } from '../../stores/chatBrowser.svelte.js';
  import { memoryTakesDrops } from '../../stores/memory.svelte.js';
  import { agents, initAgents, agentById } from '../../stores/agents.svelte.js';
  import { initChatStatus, setShown, markSeen } from '../../stores/chatStatus.svelte.js';
  import { lastAnswerAt } from '../../core/chatStatus.js';

  // Opens a Settings section (ControlPanel's openSettings): the chip's
  // "Language model settings…". `onopennote({id, line, ts, field})`: a
  // passage an answer cites, opened in Notes at its line (ControlPanel).
  // `onopenusage()`: the context ring's "See all" (Insights' AI usage card).
  let { onopensettings = null, onopennote = null, onopenusage = null } = $props();

  const chat = createChatState({ onopensettings: () => onopensettings, onopennote: () => onopennote });
  const doc = chat.doc;
  // **Code chats** (codeChats.svelte.js): with Yap Code on, the list's Chat |
  // Code switch, a Code chat's workspace and its look.
  const code = createCodeChats(chat, { opensettings: () => onopensettings });

  // **Yap Code** (stores/agents.svelte.js): the bar card's or a notification's
  // Open (`yap-agent-open`) shows the agent's chat and its tab; a report landing
  // in the chat shown (`yap-agent-report`) is read in, never switching chats.
  initAgents();
  $effect(() => {
    const want = agents.openWanted;
    if (!want?.id) return;
    agents.openWanted = null;
    untrack(async () => {
      if (want.conversation != null && want.conversation !== chat.activeId) await chat.selectConversation(want.conversation);
      const j = agentById(want.id);
      doc.showAgent(j ?? { id: want.id, conversation: want.conversation });
    });
  });
  // **The status dots** (stores/chatStatus.svelte.js): the conversation on
  // screen is never "new", and its answers are seen as they land.
  initChatStatus();
  $effect(() => {
    const id = chat.activeId ?? null;
    untrack(() => setShown(id));
  });
  $effect(() => {
    const id = chat.activeId;
    const at = lastAnswerAt(chat.messages);
    if (id != null && at >= 0) untrack(() => markSeen(id, at));
  });
  onDestroy(() => setShown(null));
  let reportsSeen = agents.reports;
  $effect(() => {
    const n = agents.reports;
    const r = agents.report;
    untrack(() => {
      if (n === reportsSeen) return;
      reportsSeen = n;
      if (r?.conversation != null && r.conversation === chat.activeId) chat.reloadActive();
      chat.refreshList();
    });
  });

  // **The browser** (the title bar's button, Ctrl+Shift+B; as Claude Code's):
  // the panel's page tabs — the last page shown, unless its tab was closed,
  // else a new tab — or, with a page tab showing, the panel closed (the pages
  // stay alive behind it).
  function toggleBrowser() {
    if (doc.open && doc.showing === 'page') {
      doc.close();
      return;
    }
    const last = doc.page;
    if (last && !doc.hidden.includes(tabKeyOf(last))) doc.showPage(last);
    else doc.newPage();
  }
  let browserPresses = untrack(() => chatBrowser.presses);
  $effect(() => {
    const n = chatBrowser.presses;
    if (n === browserPresses) return;
    browserPresses = n;
    untrack(toggleBrowser);
  });
  $effect(() => {
    chatBrowser.open = doc.open && doc.showing === 'page';
  });
  onDestroy(() => (chatBrowser.open = false));

  // **Chat's layout** (chatLayout.svelte.js, the rules chatRoom.js's): the
  // conversation list shown or hidden (its button, Ctrl+B; stepped aside
  // while a panel needs the room, back when it closes), the split between
  // the thread and the panel, and what gives way in a narrow window (the
  // list, then the panel's width, then the panel covering the conversation).
  const layout = createChatLayout(doc);
  const room = $derived(layout.room);
  const cover = $derived(room.cover);
  const narrow = $derived(room.narrow);
  let chatEl = $state(null);
  $effect(() => {
    const el = chatEl;
    if (!el || typeof ResizeObserver === 'undefined') return;
    // The app's rail beside Chat (ControlPanel), measured with it: the two
    // always add up to the same, gliding or not (chatRoom.js).
    const rail = document.querySelector('[data-nav-rail]');
    const measure = () => {
      layout.chatWidth = el.clientWidth;
      layout.railWidth = rail ? Math.round(rail.getBoundingClientRect().width) : 0;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (rail) ro.observe(rail);
    measure();
    return () => ro.disconnect();
  });

  // The panel's own chunk (DocPanel: documents, pictures, pages, agents),
  // fetched once Chat is idle so a panel opens whole in its first frame —
  // never an empty gap it then pops into (an agent starting, 7 Nov 2026).
  let DocPanel = $state(null);
  let panelLoading = null;
  function loadPanel() {
    panelLoading ??= import('./DocPanel.svelte')
      .then((m) => (DocPanel = m.default))
      .catch(() => (panelLoading = null));
    return panelLoading;
  }
  onMount(() => {
    const idle = window.requestIdleCallback ?? ((f) => setTimeout(f, 300));
    idle(() => loadPanel());
  });
  $effect(() => {
    if (doc.open && !DocPanel) untrack(loadPanel);
  });

  // The split's handle: dragged (the whole of Chat keeps the resize cursor
  // while it's held; Esc puts it back), its keys, a double-click to the default.
  function splitDown(e) {
    if (e.button !== 0) return;
    layout.dragStart();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.currentTarget.focus?.({ preventScroll: true });
    e.preventDefault();
  }
  function splitMove(e) {
    if (!layout.dragging || !chatEl) return;
    layout.dragTo(e.clientX, chatEl.getBoundingClientRect().right);
  }
  function splitUp() {
    layout.dragEnd();
  }
  function splitKey(e) {
    if (e.key === 'Escape' && layout.dragging) {
      e.preventDefault();
      e.stopPropagation();
      layout.dragEnd(true);
    } else if (layout.key(e)) e.preventDefault();
  }

  // The list's edge (as the split's): dragged from the list's own left edge
  // (Chat's, or the floating list's), its keys, a double-click to 224 px.
  let listEl = $state(null);
  function edgeDown(e) {
    if (e.button !== 0) return;
    layout.listDragStart();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.currentTarget.focus?.({ preventScroll: true });
    e.preventDefault();
  }
  function edgeMove(e) {
    if (!layout.listDragging) return;
    const left = (listEl ?? chatEl)?.getBoundingClientRect().left ?? 0;
    layout.listDragTo(e.clientX, left);
  }
  function edgeUp() {
    layout.listDragEnd();
  }
  function edgeKey(e) {
    if (e.key === 'Escape' && layout.listDragging) {
      e.preventDefault();
      e.stopPropagation();
      layout.listDragEnd(true);
    } else if (layout.listKey(e)) e.preventDefault();
  }

  // B, by its letter or (a page beside the chat's keys come as `yap-page-key`) its key code.
  const isB = (e) => e.key?.toLowerCase() === 'b' || (!e.key && e.keyCode === 66);

  /**
   * Ctrl+B: the list shown or hidden — not where Ctrl+B makes text bold (a
   * document beside the chat, any rich text), nor under a panel covering
   * the conversation.
   */
  function listKey(e) {
    if (e.defaultPrevented || cover) return false;
    if (e.target?.closest?.('.ProseMirror, [contenteditable="true"], [contenteditable=""], .docpart')) return false;
    // (Nor from a dialog or a menu over Chat: Settings, a confirm.)
    if (e.target?.closest?.('[role="dialog"], [role="alertdialog"], [role="menu"]')) return false;
    toggleList();
    return true;
  }

  /**
   * The list's button (either of its places) and Ctrl+B. The focus on the
   * button, or in the list as it goes, moves to the button's other place, so
   * the keyboard never loses its way; anywhere else (the box) it stays.
   */
  async function toggleList() {
    const had = document.activeElement;
    const follow = !had || had === document.body || !!had.closest?.('.convs, .showchats');
    layout.toggle();
    if (!follow) return;
    await tick();
    chatEl?.querySelector(room.listShown ? '[data-testid="chat-list-toggle"]' : '.showchats')?.focus();
  }

  function onKeydown(e) {
    // The list floating over the thread (asked back with no room beside it): Esc puts it aside.
    if (e.key === 'Escape' && room.listFloats && !e.defaultPrevented) {
      const inList = !!document.activeElement?.closest?.('.convs');
      layout.dismissFloating();
      if (inList) tick().then(() => chatEl?.querySelector('.showchats')?.focus());
      return;
    }
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
      // On the Code side: a new code chat in this one's workspace.
      if (code.onCode && (code.current ?? code.workspaces[0])) code.newCodeChat(code.current ?? code.workspaces[0]);
      else chat.newChat();
    } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && !e.altKey && isB(e) && !e.defaultPrevented && !e.target?.closest?.('.ProseMirror')) {
      // (Not over a document being written: the editor's own keys first.)
      e.preventDefault();
      toggleBrowser();
    } else if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && isB(e) && !e.repeat) {
      // The list's button (Hide / Show your chats).
      if (listKey(e)) e.preventDefault();
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
    // Opened while the Voice Assistant thinks or speaks: its ■ shows here too
    // (on its question's conversation).
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
      // The quote over the box, taken by a question asked out loud (or
      // given back: no answer came).
      listen('yap-chat-quote', (e) => chat.onQuote(e.payload)),
      // Words selected on a page beside the chat, asked about (its menu).
      listen('yap-page-quote', (e) => chat.onPageQuote(e.payload)),
      // A question asked with the assistant shortcut, live (and its card's
      // "Open in Chat").
      listen('yap-assistant-turn', (e) => chat.followAssistant(e.payload)),
      listen('yap-assistant-stream', (e) => chat.followAssistantStream(e.payload)),
      // Whether a spoken question is being listened for (Esc calls it off).
      listen('yap-assistant', (e) => {
        chat.assistantListening = e.payload?.phase === 'listening';
        // (Its phase and its question's conversation: its ■ shows there only.)
        chat.assistantPhase = e.payload;
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
        // Settings → Memory over Chat takes it as a memory import.
        if (memoryTakesDrops()) {
          chat.dragging = false;
          return;
        }
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
      // (A plain chat, on the Chat side: never a Code chat's.)
      code.toChat();
      chat.newChat();
      chat.scope = req.scope ?? null;
      chat.input = req.text;
      // A one-tap prompt or a "/" Skill from the Meetings view's Ask bar.
      chat.send(req.skill ? { skill: req.skill } : undefined);
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpaste={onPaste} />

<div class="chat" class:resizing={layout.dragging} bind:this={chatEl}>
  <!-- The conversation list: hidden by its button (or Ctrl+B), stepped aside
       for a panel in a narrow Chat, floating over the thread when asked back
       with no room beside both; out of reach while a panel covers it. -->
  <div
    class="listslot"
    class:aside={!room.listShown}
    class:floating={room.listFloats}
    inert={cover || !room.listShown || undefined}
    style:--chat-list-w="{room.listWidth}px"
  >
    <ConversationList bind:el={listEl} {chat} {code} ontogglelist={toggleList} />
    <!-- The list's edge: drag it (Esc puts it back), ←/→ (Shift: further),
         Home / End, a double-click back to 224 px. A focusable separator
         is a widget (WAI-ARIA's window splitter). -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
    <div
      class="listedge"
      class:held={layout.listDragging}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize your chats"
      aria-controls="chat-list"
      aria-valuenow={room.listWidth}
      aria-valuemin={LIST_MIN}
      aria-valuemax={room.listMax}
      aria-valuetext={`${room.listWidth} pixels wide`}
      title="Drag to resize · Double-click to reset"
      tabindex="0"
      data-testid="chat-list-edge"
      onpointerdown={edgeDown}
      onpointermove={edgeMove}
      onpointerup={edgeUp}
      onpointercancel={edgeUp}
      onlostpointercapture={edgeUp}
      ondblclick={() => layout.listReset()}
      onkeydown={edgeKey}
    ></div>
  </div>

  <!-- Thread (a press in it puts a floating list aside) -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <section
    class="thread"
    class:withbtn={!room.listShown}
    inert={cover || undefined}
    onpointerdown={room.listFloats ? () => layout.dismissFloating() : undefined}
  >
    {#if !room.listShown}
      <!-- The list's button in its other place: the list back. -->
      <button
        type="button"
        class="showchats"
        onclick={toggleList}
        aria-label="Show your chats"
        title="Show your chats · Ctrl+B"
        aria-keyshortcuts="Control+B"
        aria-expanded="false"
        aria-controls="chat-list"
        data-testid="chat-list-show"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2.5" /><path d="M9 4v16" /></svg>
      </button>
    {/if}
    <ChatThread {chat} {code} {onopensettings} />
    <Composer {chat} {code} {onopensettings} {onopenusage} />
  </section>

  {#if doc.open}
    {#if !cover}
      <!-- The split: drag it (Esc puts it back), the arrow keys (Shift:
           further; Home / End), a double-click back to the default. A
           focusable separator is a widget (WAI-ARIA's window splitter). -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
      <div
        class="split"
        class:held={layout.dragging}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the panel"
        aria-controls="chat-panel"
        aria-valuenow={room.panelWidth}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={room.maxWidth}
        aria-valuetext={`${room.panelWidth} pixels wide`}
        title="Drag to resize · Double-click to reset"
        tabindex="0"
        data-testid="chat-split"
        onpointerdown={splitDown}
        onpointermove={splitMove}
        onpointerup={splitUp}
        onpointercancel={splitUp}
        onlostpointercapture={splitUp}
        ondblclick={() => layout.reset()}
        onkeydown={splitKey}
      ></div>
    {/if}
    <!-- Its width from its first frame, its content faded in once (never with
         reduced motion); the panel's ground while its chunk is on its way. -->
    <div id="chat-panel" class="docslot" class:cover class:ground={!DocPanel} style={cover ? undefined : `width: ${room.panelWidth}px`} data-testid="chat-panel-slot">
      {#if DocPanel}
        <DocPanel {doc} {cover} {narrow} onquote={chat.setQuote} messages={chat.messages} conversationId={chat.activeId} />
      {/if}
    </div>
  {/if}
</div>

{#if chat.viewing}
  <!-- A screenshot opened large: a click or Esc closes it; its right-click
       menu copies or saves it. -->
  <ShotViewer img={chat.viewing} onclose={() => (chat.viewing = null)} />
{/if}

<style>
  .chat {
    position: relative;
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }
  /* The split held: the resize cursor everywhere, nothing selected on the way. */
  .chat.resizing,
  .chat.resizing :global(*) {
    cursor: col-resize !important;
    user-select: none;
  }
  .listslot.aside {
    display: none;
  }
  .listslot {
    display: contents;
  }
  /* Asked back with no room beside the thread and the panel: over the thread. */
  .listslot.floating {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 4;
    display: flex;
    box-shadow: var(--yap-shadow-menu);
  }
  /* The panel beside the conversation, its width the split's from its first
     frame; its content fades in once as it opens (nothing moves). */
  .docslot {
    flex: 0 0 auto;
    display: flex;
    min-width: 0;
    min-height: 0;
    animation: yap-panel-in 140ms ease-out;
  }
  .docslot.ground {
    background: var(--yap-paper);
    border-left: 1px solid var(--yap-border-subtle);
  }
  @keyframes yap-panel-in {
    from {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .docslot {
      animation: none;
    }
  }
  :global(:root[data-motion='reduced']) .docslot {
    animation: none;
  }
  .docslot > :global(.docpanel) {
    flex: 1 1 auto;
  }
  .docslot.cover {
    position: absolute;
    inset: 0;
    z-index: 5;
  }
  /* The split: a 10 px handle over the panel's edge; its line and grip on
     hover, focus and while it's held. */
  .split {
    flex: 0 0 10px;
    margin: 0 -5px;
    position: relative;
    z-index: 3;
    cursor: col-resize;
    touch-action: none;
  }
  .split::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 4px;
    width: 2px;
    background: transparent;
    transition: background var(--yap-dur, 0.15s);
  }
  .split::before {
    content: '';
    position: absolute;
    top: 50%;
    left: 2px;
    z-index: 1;
    width: 6px;
    height: 32px;
    margin-top: -16px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-line);
    opacity: 0;
    transition: opacity var(--yap-dur, 0.15s);
  }
  .split:hover::after,
  .split:focus-visible::after,
  .split.held::after {
    background: var(--yap-primary-line);
  }
  .split:hover::before,
  .split:focus-visible::before,
  .split.held::before {
    opacity: 1;
  }
  .split.held::after,
  .split.held::before {
    background: var(--yap-primary);
  }
  .split:focus-visible {
    outline: none;
  }
  /* The list's edge: the split's look, over the list's right border. */
  .listedge {
    flex: 0 0 10px;
    margin: 0 -5px;
    position: relative;
    z-index: 3;
    cursor: col-resize;
    touch-action: none;
  }
  .listedge::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 4px;
    width: 2px;
    background: transparent;
    transition: background var(--yap-dur, 0.15s);
  }
  .listedge::before {
    content: '';
    position: absolute;
    top: 50%;
    left: 2px;
    z-index: 1;
    width: 6px;
    height: 32px;
    margin-top: -16px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-line);
    opacity: 0;
    transition: opacity var(--yap-dur, 0.15s);
  }
  .listedge:hover::after,
  .listedge:focus-visible::after,
  .listedge.held::after {
    background: var(--yap-primary-line);
  }
  .listedge:hover::before,
  .listedge:focus-visible::before,
  .listedge.held::before {
    opacity: 1;
  }
  .listedge.held::after,
  .listedge.held::before {
    background: var(--yap-primary);
  }
  .listedge:focus-visible {
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
  /* The list's button over the thread's top left: the thread's own parts
     keep clear of it; the composer keeps its whole width (a narrow thread).
     The messages' column (ChatThread) keeps clear by itself, only as far as
     it needs to: centred in a wide thread, it never moves. */
  .thread.withbtn {
    --thread-clear: 26px;
  }
  .thread.withbtn > :global(:not(.composer):not(.showchats):not(.threadbox):not(.scopeline)) {
    margin-left: 26px;
  }
  /* The list's button where the list was: as in its head, the same place. */
  .showchats {
    position: absolute;
    top: 10px;
    left: 8px;
    z-index: 3;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .showchats:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .showchats:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .showchats svg {
    width: 16px;
    height: 16px;
  }
</style>
