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
  // This file is the view: it makes its state (chatState.svelte.js), follows
  // Rust's events while Chat is open, takes the keys, and lays out the
  // conversation list (ConversationList) beside the thread (ChatThread,
  // ChatMessage) and the composer (Composer).
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

  // Opens a Settings section (ControlPanel's openSettings): the chip's
  // "Language model settings…". `onopennote({id, line, ts, field})`: a
  // passage an answer cites, opened in Notes at its line (ControlPanel).
  let { onopensettings = null, onopennote = null } = $props();

  const chat = createChatState({ onopensettings: () => onopensettings, onopennote: () => onopennote });

  function onKeydown(e) {
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
    } else if (e.key === 'Escape' && chat.sending && chat.streaming && !e.defaultPrevented) {
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
    setTimeout(() => chat.inputEl?.focus(), 0);
    const uns = [
      // A snip landed while Chat is open (snip.rs).
      listen('yap-snip', (e) => chat.takeSnip(e.payload)),
      // Another Chat model: can it read the screenshot?
      listen('yap-chat-model-changed', chat.refreshVision),
      listen('yap-chat-dictation', (e) => chat.onChatDictation(e.payload)),
      // A question asked with the assistant shortcut, live (and its card's
      // "Open in Chat").
      listen('yap-assistant-turn', (e) => chat.followAssistant(e.payload)),
      listen('yap-assistant-stream', (e) => chat.followAssistantStream(e.payload)),
      // Whether a spoken question is being listened for (Esc calls it off).
      listen('yap-assistant', (e) => (chat.assistantListening = e.payload?.phase === 'listening')),
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
      chat.send();
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpaste={onPaste} />

<div class="chat">
  <!-- Conversation sidebar -->
  <ConversationList {chat} />

  <!-- Thread -->
  <section class="thread">
    <ChatThread {chat} {onopensettings} />
    <Composer {chat} {onopensettings} />
  </section>
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
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }
  .thread {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 16px 20px;
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
