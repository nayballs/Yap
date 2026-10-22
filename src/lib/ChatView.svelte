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
  // "Friday." → "Monday.""), kept with the message in chats.json. No
  // conversation search/archive yet.
  //
  // Streamed (stream.rs, streaming.js; 2026-10-22): the answer grows in
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
  // Right-click menus (ui/contextMenu.svelte.js, as in Notes): a conversation
  // — also from its hover ⋯ and the ContextMenu key / Shift+F10 — has Rename
  // (in place: Enter or leaving it saves, Esc cancels; chats.rs `rename`),
  // Copy as Markdown (the whole conversation) and Delete… (asked first); a
  // right-click opens it too. An answer has Copy / Copy as Markdown / Read
  // aloud, a question Copy (itemMenus.js).
  //
  // The composer's model chip (ModelChip.svelte) says which model the next
  // message goes to and switches the Chat scope's; each answer keeps the
  // model that wrote it (chats.rs `model`), shown quietly under it.
  //
  // Spoken replies (tts.rs, Settings → Voice Assistant): the 🔊 / 🔇 next to the
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
  //
  // The Voice Assistant (assistant.rs): a question asked out loud with its
  // shortcut, from any app, lands in Chat — the conversation open here, else
  // the last one if it's recent, else a new one — so this view tells Rust
  // what it shows (`chat_view_state`) and follows the question live
  // (`yap-assistant-turn`: the question, marked asked by voice, then
  // "Thinking…", then the answer, which Rust reads aloud). With no shortcut
  // set, the empty thread says how to set one.
  //
  // The web (web.rs, "Look things up on the web" in Settings → Voice
  // Assistant): an answer written from web pages cites them as [1], drawn
  // as small pills (markdown.js), with a Sources row under it — number,
  // title, site — that opens each in the browser (open_external). Kept with
  // the message (chats.rs `sources`). Its search / read chips carry a globe,
  // and an action refused after reading the web a shield.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import { renderMarkdown } from './markdown.js';
  import { toast } from './ui/toast.svelte.js';
  import { chatRequest } from './chatRequest.svelte.js';
  import ModelChip from './ModelChip.svelte';
  import { modelLine, whereItRuns } from './aiModel.js';
  import { speech, initSpeech, readAloud as speakText, speakReply, stopReading as stopSpeech } from './speech.svelte.js';
  import { formatHotkeySpec } from './hotkeys.js';
  import { aiError } from './aiErrors.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm, devInspect } from './ui/contextMenu.svelte.js';
  import { copyText, messageItems, conversationMarkdown } from './itemMenus.js';
  import { openExternalLink } from './externalLinks.js';
  import { streamCall, liveAnswer, applyEvent, closePartial, settle, streamingHeight, STOPPED } from './streaming.js';

  // A chip about the web (web.rs) or the safety rule (tools.rs).
  const WEB_CHIP = /^(Searched the web|Couldn't search the web|Didn't search for|Read [^"]|Couldn't read [^"]|Didn't read |Answered from )/;
  const chipKind = (t) => (/: not after reading the web$/.test(t) ? 'guard' : WEB_CHIP.test(t) ? 'web' : 'tool');

  // Opens a Settings section (ControlPanel's openSettings): the chip's
  // "Language model settings…".
  let { onopensettings = null } = $props();

  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // { role, text, tools?, model?, voice?, id }

  // The sources an answer's citations point at: its own, else — a follow-up
  // that cites "[1]" from an earlier answer without searching again ("the
  // top five in that list") — the latest earlier answer's in this chat, so
  // its markers are pills too, never raw "[1][2]".
  const CITES = /\[\d{1,2}(?:\s*,\s*\d{1,2})*\]/;
  function citedSources(i) {
    // Still being written: the sources found so far.
    const own = messages[i]?.sources?.length ? messages[i].sources : messages[i]?.liveSources;
    if (own?.length) return own;
    if (!CITES.test(messages[i]?.text ?? '')) return null;
    for (let k = i - 1; k >= 0; k--) {
      const s = messages[k].role === 'assistant' ? messages[k].sources : null;
      if (s?.length) return s;
    }
    return null;
  }
  let input = $state('');
  let sending = $state(false);
  // The ■ of the answer being written (streamCall's stop), and whether it
  // was pressed.
  let stopAnswerCall = null;
  let stopping = $state(false);
  // An answer is being written in the thread (its bubble shows, not "Thinking…").
  const streaming = $derived(messages.some((m) => m.streaming));
  let inputEl = $state(null);
  let threadEl = $state(null);
  // 'meetings' when the Meetings view's Ask bar started this chat: answers
  // are grounded in your meeting notes (calendar::meetings_context).
  let scope = $state(null);

  // ---- Spoken replies
  let speakReplies = $state(false);
  // The composer's + (the maintainer, 2026-10-20, after Claude Code's; the model
  // stays out of it, quiet under the box on the left, always in view): Voice
  // replies and the voice's volume ("so loud, even at 1% on the volume
  // mixer") — moving it changes what's being said at once; letting go keeps
  // it (tts_volume_set), and Settings follows.
  let voiceVolume = $state(0.5);
  let plusOpen = $state(false);
  let plusEl = $state(null);
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

  // ---- The Voice Assistant
  // Its shortcut ('' = none set: the empty thread offers to set one).
  let assistantHotkey = $state('');
  // Chat's model is set up (ai_model_status; re-read on every config save).
  let chatReady = $state(true);
  // The conversation whose spoken question is being answered ("Thinking…").
  let assistantPending = $state(null);
  // "New chat" was clicked: a spoken question starts a new conversation
  // rather than continuing the last one.
  let choseNew = false;

  // A slow answer says it's still coming: a model on this PC's processor
  // can take a minute (Rust gives it 2 minutes, llm::LOCAL_CHAT_TIMEOUT),
  // which a bare "Thinking…" makes look hung.
  const STILL_WORKING_S = 8;
  let waitedS = $state(0);
  let modelHere = $state(false);
  $effect(() => {
    const busy = sending || (assistantPending != null && assistantPending === activeId);
    if (!busy) {
      waitedS = 0;
      return;
    }
    const started = Date.now();
    invoke('ai_model_status', { scope: 'chat' })
      .then((s) => (modelHere = !!s?.model?.onThisPc))
      .catch(() => {});
    const timer = setInterval(() => (waitedS = Math.floor((Date.now() - started) / 1000)), 1000);
    return () => clearInterval(timer);
  });

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
      voiceVolume = Number.isFinite(Number(c?.ttsVolume)) ? Number(c.ttsVolume) : 0.5;
      speakWhen = c?.speakWhen || 'voice';
      assistantHotkey = c?.assistantHotkey || '';
    } catch {
      /* keep */
    }
    // No AI model yet: the empty chat says so before a question fails.
    invoke('ai_model_status', { scope: 'chat' })
      .then((s) => (chatReady = s?.ready !== false))
      .catch(() => {});
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

  function moveVolume(v) {
    voiceVolume = v;
    invoke('tts_volume_set', { volume: v }).catch(() => {});
  }
  async function keepVolume() {
    try {
      await invoke('tts_volume_set', { volume: voiceVolume, save: true });
    } catch (e) {
      toast({ title: "Couldn't save the voice volume", description: String(e), variant: 'destructive' });
    }
  }
  function plusOutside(e) {
    if (plusOpen && plusEl && !plusEl.contains(e.target)) plusOpen = false;
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
    choseNew = true;
    reportView();
    setTimeout(() => inputEl?.focus(), 0);
  }

  // What this view shows, for a question asked out loud (assistant.rs
  // `choose_conversation`): the conversation open here, a new chat someone
  // asked for, or — nothing chosen — no preference.
  function reportView(open = true) {
    invoke('chat_view_state', {
      open,
      conversationId: activeId,
      newChat: activeId == null && choseNew,
    }).catch(() => {});
  }
  $effect(() => {
    void activeId;
    untrack(() => {
      if (alive) reportView();
    });
  });

  // A question asked with the assistant shortcut (assistant.rs): this view
  // switches to its conversation and shows it as it goes — the question at
  // once, "Thinking…" while the answer comes, then the answer (read aloud by
  // Rust). Never stops the voice: it's the answer being followed.
  // A spoken question's answer as it's written (assistant.rs): it grows at
  // the end of its conversation while that's the one shown.
  let assistantLive = liveAnswer();
  function followAssistantStream(p) {
    const id = p?.conversationId;
    if (id == null || id !== activeId || assistantPending !== id) return;
    assistantLive = applyEvent(assistantLive, p.event);
    let last = messages[messages.length - 1];
    if (!last?.streaming) {
      if (!assistantLive.started) return;
      messages = [...messages, { role: 'assistant', streaming: true, text: '', tools: [], id: null }];
      last = messages[messages.length - 1];
    }
    last.text = assistantLive.text;
    last.tools = assistantLive.chips;
    last.doing = assistantLive.doing;
    last.liveSources = assistantLive.sources;
    last.waiting = assistantLive.waiting;
    followThread();
  }

  async function followAssistant(p) {
    const id = p?.conversationId;
    if (p?.phase === 'asked') {
      assistantPending = id ?? null;
      assistantLive = liveAnswer();
    } else if (p?.phase === 'answered' || p?.phase === 'failed') assistantPending = null;
    if (id == null) return;
    try {
      const conv = await invoke('chat_get', { id });
      if (!alive) return;
      if (activeId !== id) {
        activeId = id;
        scope = null;
      }
      // The answer that streamed here, replaced by the saved (checked) one.
      const streamedAt = messages.findIndex((m) => m.streaming);
      const streamed = streamedAt >= 0 ? messages[streamedAt].text : null;
      const fromHeight = streamingHeight(threadEl);
      messages = (conv.messages || []).map((m, i) => ({
        ...m,
        id: chatId(id, i),
        revisedFrom: i === streamedAt && streamed && streamed.trim() !== m.text.trim() ? fromHeight : 0,
      }));
      refreshList();
      scrollThread();
    } catch {
      /* deleted meanwhile */
    }
    // A failed answer: Rust says so where it'll be seen (an alert, alert.rs).
  }

  async function removeConversation(id) {
    try {
      await invoke('chat_delete', { id });
      if (activeId === id) newChat();
      await refreshList();
      return true;
    } catch (e) {
      toast({ title: "Couldn't delete the conversation", description: String(e), variant: 'destructive' });
      return false;
    }
  }

  // ---- Several at once (the maintainer, 2026-10-19: "shift click some chats and
  // delete them all together", as Claude Code's sessions list) ----
  // Click opens a conversation; Shift+click selects the range from the last
  // one clicked, Ctrl+click adds or takes out one (File Explorer's rules).
  // A selection shows a "3 selected · Delete" bar; Delete deletes it, Esc
  // clears it, Ctrl+A in the list selects every one. "Delete all chats…"
  // is in the list's ⋯ menu, asked first like every delete.
  let picked = $state([]);
  let anchor = $state(null);
  const ordered = $derived(groups.flatMap((g) => g.items));

  function clickConversation(e, c) {
    if (e.shiftKey) {
      const ids = ordered.map((x) => x.id);
      const from = anchor ?? activeId ?? c.id;
      const a = ids.indexOf(from);
      const b = ids.indexOf(c.id);
      const range = a < 0 ? [c.id] : ids.slice(Math.min(a, b), Math.max(a, b) + 1);
      picked = e.ctrlKey || e.metaKey ? [...new Set([...picked, ...range])] : range;
      if (anchor == null) anchor = from;
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      // The open conversation counts as picked when a selection starts.
      const base = picked.length ? picked : activeId != null && activeId !== c.id ? [activeId] : [];
      picked = base.includes(c.id) ? base.filter((id) => id !== c.id) : [...base, c.id];
      anchor = c.id;
      return;
    }
    picked = [];
    anchor = c.id;
    selectConversation(c.id);
  }

  function clearPicked() {
    picked = [];
    anchor = null;
  }

  // `all`: "Delete all chats…" (every conversation the list shows).
  function askDeleteMany(ids, { all = false } = {}) {
    const convs = conversations.filter((c) => ids.includes(c.id));
    if (!convs.length) return;
    if (convs.length === 1 && !all) return askDelete(convs[0]);
    const count = convs.reduce((n, c) => n + (c.messageCount || 0), 0);
    askConfirm({
      title: all
        ? convs.length === 1
          ? 'Delete your only chat?'
          : `Delete all ${convs.length} chats?`
        : `Delete ${convs.length} chats?`,
      body: `${count === 1 ? 'Their 1 message goes' : `Their ${count} messages go`} for good. Your notes and meetings stay.`,
      run: () => removeConversations(convs.map((c) => c.id)),
    });
  }

  async function removeConversations(ids) {
    try {
      const gone = await invoke('chat_delete_many', { ids });
      if (ids.includes(activeId)) newChat();
      clearPicked();
      await refreshList();
      toast({ title: gone === 1 ? 'Chat deleted' : `${gone} chats deleted`, variant: 'success' });
      return true;
    } catch (e) {
      toast({ title: "Couldn't delete the chats", description: String(e), variant: 'destructive' });
      return false;
    }
  }

  // The list's own keys: Delete, Esc and Ctrl+A (Enter and the menu keys
  // stay with each row).
  function listKeydown(e) {
    if (renaming || e.target.closest?.('input, textarea')) return;
    if (e.key === 'Delete' && (picked.length || e.target.dataset?.conv)) {
      e.preventDefault();
      askDeleteMany(picked.length ? picked : [Number(e.target.dataset.conv)]);
    } else if (e.key === 'Escape' && picked.length) {
      e.preventDefault();
      e.stopPropagation();
      clearPicked();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      picked = ordered.map((c) => c.id);
    }
  }

  function listMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Select all', icon: 'check', onselect: () => (picked = ordered.map((c) => c.id)) },
        { separator: true },
        {
          label: 'Delete all chats…',
          icon: 'trash',
          danger: true,
          onselect: () => askDeleteMany(ordered.map((c) => c.id), { all: true }),
        },
      ],
      { label: 'Chats' }
    );
  }

  // ---- Right-click menus (ui/contextMenu.svelte.js) ----

  // The conversation rows, by id (the menu gives the focus back to its row).
  let convEls = $state({});
  // Rename in place, like Notes' folders: `{ id, value }`.
  let renaming = $state(null);

  function conversationMenu(e, c) {
    // On a row of a selection: the selection's menu.
    if (picked.length > 1 && picked.includes(c.id)) {
      openContextMenu(
        e,
        [
          { label: `Delete ${picked.length} chats…`, icon: 'trash', danger: true, onselect: () => askDeleteMany(picked) },
          { separator: true },
          { label: 'Clear selection', onselect: clearPicked },
        ],
        { label: `${picked.length} selected chats`, row: convEls[c.id] }
      );
      return;
    }
    clearPicked();
    // A right-click opens it too, as a click does (not while an answer comes).
    if (e.type === 'contextmenu' && !devInspect(e) && !sending && activeId !== c.id) selectConversation(c.id);
    openContextMenu(
      e,
      [
        { label: 'Rename', icon: 'pencil', onselect: () => startRename(c) },
        { separator: true },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => copyConversation(c.id) },
        { separator: true },
        { label: 'Delete…', icon: 'trash', danger: true, onselect: () => askDelete(c) },
      ],
      { label: `Conversation ${c.title || 'Untitled'}`, row: convEls[c.id] }
    );
  }

  function startRename(c) {
    renaming = { id: c.id, value: c.title || '' };
  }
  async function finishRename(save = true) {
    const r = renaming;
    if (!r) return;
    renaming = null;
    const before = conversations.find((c) => c.id === r.id)?.title || '';
    const title = r.value.trim();
    if (save && title && title !== before) {
      try {
        const stored = await invoke('chat_rename', { id: r.id, title });
        conversations = conversations.map((c) => (c.id === r.id ? { ...c, title: stored } : c));
      } catch (e) {
        toast({ title: "Couldn't rename the conversation", description: String(e), variant: 'destructive' });
      }
    }
    setTimeout(() => convEls[r.id]?.focus({ preventScroll: true }), 0);
  }
  function focusOnMount(el) {
    el.focus();
    el.select();
  }

  async function copyConversation(id) {
    try {
      const conv = await invoke('chat_get', { id });
      await copyText(conversationMarkdown(conv), 'Copied as Markdown');
    } catch (e) {
      toast({ title: "Couldn't copy", description: String(e), variant: 'destructive' });
    }
  }

  function askDelete(c) {
    const count = c.messageCount || 0;
    askConfirm({
      title: `Delete “${c.title || 'Untitled'}”?`,
      body:
        count === 0
          ? 'The conversation goes for good.'
          : `Its ${count === 1 ? 'message goes' : `${count} messages go`} for good.`,
      run: () => removeConversation(c.id),
      returnFocus: convEls[c.id],
    });
  }

  function messageMenu(e, m) {
    const reading = !!m.id && speakingId === m.id;
    openContextMenu(
      e,
      messageItems(m, { read: m.role === 'assistant' && m.id ? () => readAloud(m) : null, reading }),
      { label: m.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget }
    );
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
    const qi = messages.length - 1;
    sending = true;
    stopping = false;
    scrollThread();
    // The answer as it's written: its bubble appears with the first text
    // or tool call, at `ai`, and grows in place.
    let live = liveAnswer();
    let ai = -1;
    const onEvent = (ev) => {
      if (!alive) return;
      if (ev?.kind === 'turn') {
        // A new conversation: it's in the list from the first question.
        if (activeId == null && ev.conversationId != null) {
          activeId = ev.conversationId;
          refreshList();
        }
        return;
      }
      live = applyEvent(live, ev);
      if (ai < 0) {
        if (!live.started && !live.waiting) return;
        messages = [...messages, { role: 'assistant', streaming: true, text: '', tools: [], id: null }];
        ai = messages.length - 1;
      }
      const m = messages[ai];
      if (!m?.streaming) return;
      m.text = live.text;
      m.tools = live.chips;
      m.doing = live.doing;
      m.liveSources = live.sources;
      m.waiting = live.waiting;
      followThread();
    };
    const call = streamCall('chat_send', { conversationId: activeId, text, scope, voice }, onEvent);
    stopAnswerCall = call.stop;
    try {
      const res = await call.promise;
      if (!alive) return;
      activeId = res.conversationId;
      const asked = messages[qi];
      const streamed = ai >= 0 ? messages[ai].text : null;
      const fromHeight = streamingHeight(threadEl);
      // The checked answer takes the streamed one's place: the same bubble,
      // a soft glide when the check changed the text.
      const answer = {
        role: 'assistant',
        text: res.reply,
        tools: res.toolsUsed || [],
        model: res.model ?? null,
        sources: res.sources || [],
        id: res.messageId ?? null,
        stopped: !!res.stopped,
        revisedFrom:
          streamed != null && streamed.trim() !== '' && streamed.trim() !== String(res.reply).trim() ? fromHeight : 0,
      };
      messages = [...messages.slice(0, qi), { ...asked, id: res.userMessageId ?? null }, answer];
      refreshList();
      followThread();
      // Read out by itself ("When to speak", decided in Rust): the voice
      // alone, no reading card (that's ▶'s).
      if (res.speak && answer.id) speakReply(answer.text, answer.id);
    } catch (e) {
      if (!alive) return;
      if (stopping && String(e).includes(STOPPED)) {
        // Stopped before any of the answer came: the question stays.
        messages = [...messages.slice(0, qi), { ...messages[qi], stopped: true }];
        refreshList();
        return;
      }
      messages = messages.slice(0, qi);
      input = text; // give the message back
      byVoice = voice;
      // Said so it's clear what to do (aiErrors.js); the raw error is in the log.
      // A sentence reads as a sentence; only an error Yap can't explain gets
      // the copyable error box.
      const why = aiError(e);
      toast({
        title: "Couldn't send",
        description: why.text,
        ...(why.known
          ? { variant: 'default', kind: 'error', chip: 'Error', icon: 'alert' }
          : { variant: 'destructive' }),
        ...(why.settings && onopensettings
          ? { action: { label: 'Open Language Models', onClick: () => onopensettings('cleanup#chat') } }
          : {}),
      });
    } finally {
      sending = false;
      stopping = false;
      stopAnswerCall = null;
      if (alive && sendWhenFree) {
        sendWhenFree = false;
        send();
      }
    }
  }

  /** ■: stop the answer being written (what came stays, checked). */
  function stopAnswer() {
    if (!sending || stopping || !stopAnswerCall) return;
    stopping = true;
    stopAnswerCall();
  }

  /** Keep the newest text in view while it's written — unless you scrolled up to read. */
  function followThread() {
    const el = threadEl;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (near) tick().then(() => (el.scrollTop = el.scrollHeight));
  }

  // The mic: a dictation into the chat box (chat_dictate: the text comes
  // back as `yap-chat-dictation`, at the caret). Again: stop and send it in.
  function micChat() {
    inputEl?.focus();
    invoke('chat_dictate').catch(() => {});
  }

  function onKeydown(e) {
    if (e.key === 'Escape' && plusOpen) {
      plusOpen = false;
      return;
    }
    if (e.key === 'Escape' && picked.length && !e.defaultPrevented) {
      clearPicked();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
      e.preventDefault();
      newChat();
    } else if (e.key === 'Escape' && speakingId?.startsWith('chat:')) {
      e.preventDefault();
      stopReading();
    } else if (e.key === 'Escape' && sending && streaming && !e.defaultPrevented) {
      e.preventDefault();
      stopAnswer();
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
      // A question asked with the assistant shortcut, live (and its card's
      // "Open in Chat").
      listen('yap-assistant-turn', (e) => followAssistant(e.payload)),
      listen('yap-assistant-stream', (e) => followAssistantStream(e.payload)),
      listen('yap-voice-replies', (e) => {
        if (typeof e.payload?.on === 'boolean') speakReplies = e.payload.on;
      }),
      listen('yap-voice-volume', (e) => {
        if (Number.isFinite(e.payload?.volume)) voiceVolume = e.payload.volume;
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
    reportView(false);
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
      // A question asked out loud while Chat wasn't open (ControlPanel
      // brought it up): its conversation, as it goes.
      if (req.assistant) {
        followAssistant(req.assistant);
        return;
      }
      newChat();
      scope = req.scope ?? null;
      input = req.text;
      send();
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={plusOutside} />

<div class="chat">
  <!-- Conversation sidebar -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <aside class="convs" onkeydown={listKeydown}>
    {#if picked.length}
      <div class="tools selbar" role="toolbar" aria-label="Selected chats">
        <span class="selcount" title="Shift+click selects a range, Ctrl+click adds or takes out one">{picked.length} selected</span>
        <button class="selbtn danger" onclick={() => askDeleteMany(picked)}>Delete</button>
        <button class="selbtn" onclick={clearPicked}>Cancel</button>
      </div>
    {:else}
      <div class="tools">
        <button class="tool" onclick={newChat}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
          New chat
        </button>
        {#if conversations.length}
          <button class="listmore" title="More" aria-label="More for chats" aria-haspopup="menu" onclick={listMenu}>
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
          </button>
        {/if}
      </div>
    {/if}

    {#if conversations.length === 0}
      <p class="convempty">Start your first conversation</p>
    {:else}
      <div class="convscroll">
        {#each groups as g (g.label)}
          <div class="gcap">{g.label}</div>
          {#each g.items as c (c.id)}
            {#if renaming?.id === c.id}
              <!-- Rename in place (as Notes' folders): Enter or leaving it saves, Esc cancels. -->
              <div class="conv renaming">
                <input
                  class="renameinput"
                  aria-label={`Rename conversation ${c.title || 'Untitled'}`}
                  maxlength="80"
                  bind:value={renaming.value}
                  use:focusOnMount
                  onkeydown={(e) => {
                    if (e.key === 'Enter') finishRename(true);
                    if (e.key === 'Escape') {
                      e.stopPropagation();
                      finishRename(false);
                    }
                  }}
                  onblur={() => finishRename(true)}
                />
              </div>
            {:else}
              <div
                bind:this={convEls[c.id]}
                class="conv"
                class:active={activeId === c.id}
                class:picked={picked.includes(c.id)}
                class:menuopen={menuOpenFor(convEls[c.id])}
                role="button"
                tabindex="0"
                aria-haspopup="menu"
                data-conv={c.id}
                onmousedown={(e) => e.shiftKey && e.preventDefault()}
                onclick={(e) => clickConversation(e, c)}
                oncontextmenu={(e) => conversationMenu(e, c)}
                onkeydown={(e) => {
                  if (e.target !== e.currentTarget) return;
                  if (e.key === 'Enter' || e.key === ' ') selectConversation(c.id);
                  else menuKey(e, (ev) => conversationMenu(ev, c));
                }}
              >
                {#if picked.includes(c.id)}
                  <span class="pickmark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg></span>
                {/if}
                <span class="ctitle">{c.title || 'Untitled'}{#if picked.includes(c.id)}<span class="sr-only">, selected</span>{/if}</span>
                <button
                  class="more"
                  title="More"
                  aria-label={`More for ${c.title || 'Untitled'}`}
                  aria-haspopup="menu"
                  data-more
                  onclick={(e) => conversationMenu(e, c)}
                >
                  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
                </button>
              </div>
            {/if}
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
        {#if !chatReady}
          <!-- Before a first question fails: what Chat needs. -->
          <p class="asknote needsai">
            Chat needs an AI model: a free key from a cloud provider, or one that runs on this PC.
            <button class="setkey" onclick={() => onopensettings?.('cleanup#chat')}>Set up an AI model</button>
          </p>
        {/if}
        <!-- The Voice Assistant: the same chat, asked out loud from any app. -->
        <p class="asknote">
          {#if assistantHotkey}
            Or press <kbd>{formatHotkeySpec(assistantHotkey)}</kbd> in any app to ask out loud and hear the answer.
          {:else}
            <button class="setkey" onclick={() => onopensettings?.('voice#assistant-shortcut')}>Set an assistant shortcut</button>
            to ask out loud from any app and hear the answer.
          {/if}
        </p>
      </div>
    {:else}
      <div class="msgs" bind:this={threadEl}>
        {#each messages as m, i (i)}
          {#if m.role === 'assistant' && (m.tools?.length || m.doing)}
            <!-- Tool-activity chips (OpenWhispr shows tool calls above the answer) -->
            <div class="toolchips">
              {#each m.tools as t, j (j)}
                {@const kind = chipKind(t)}
                <span class="toolchip {kind}">
                  {#if kind === 'web'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18" /><path d="M12 3a14 14 0 0 1 0 18a14 14 0 0 1 0-18z" /></svg>
                  {:else if kind === 'guard'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3 4.5 6v5.5c0 4.6 3.1 8.2 7.5 9.5 4.4-1.3 7.5-4.9 7.5-9.5V6z" /><path d="m9 12 2 2 4-4" /></svg>
                  {:else}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.7 6.3a4.5 4.5 0 0 0 5.8 5.8L17 15.6a4.5 4.5 0 0 1-5.8-5.8z" transform="rotate(90 12 12)" /><path d="m3 21 6.5-6.5" /><path d="M14.7 6.3 21 3l-3.3 6.3" /></svg>
                  {/if}
                  {t}
                </span>
              {/each}
              {#if m.streaming && m.doing}
                <!-- The tool call under way. -->
                <span class="toolchip doing" data-testid="chat-doing" aria-live="polite">
                  <span class="spin" aria-hidden="true"></span>
                  {m.doing}
                </span>
              {/if}
            </div>
          {/if}
          {@const reading = m.role === 'assistant' && !!m.id && speakingId === m.id}
          {#if m.role === 'assistant' && m.streaming && !m.text}
            <!-- Tools run, or a dictation is being waited out, before the first words. -->
            <div class="cbubble assistant" aria-busy="true">
              <p class="cbody thinking">{m.waiting ? 'Waiting for your dictation to finish…' : 'Thinking…'}</p>
            </div>
          {:else}
          <!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="cbubble {m.role}"
            class:reading
            class:streaming={m.streaming}
            role="article"
            tabindex="0"
            aria-label={m.role === 'assistant' ? 'Answer' : 'Your question'}
            aria-busy={m.streaming ? 'true' : undefined}
            data-testid={m.streaming ? 'chat-streaming' : undefined}
            use:settle={m.revisedFrom || 0}
            oncontextmenu={(e) => messageMenu(e, m)}
            onkeydown={(e) => e.target === e.currentTarget && menuKey(e, (ev) => messageMenu(ev, m))}
          >
            {#if m.role === 'assistant'}
              <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
              <div class="cbody">{@html renderMarkdown(m.streaming ? closePartial(m.text) : m.text, { sources: citedSources(i) })}</div>
              {#if m.streaming && m.waiting}
                <p class="cwait" aria-live="polite">Waiting for your dictation to finish…</p>
              {/if}
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
          {/if}
          {#if m.role === 'user' && m.stopped && i === messages.length - 1}
            <!-- Stopped before any of the answer came. -->
            <p class="stoppednote">Stopped</p>
          {/if}
          {#if m.role === 'assistant' && m.sources?.length}
            <!-- Where a web answer came from: each opens in the browser. -->
            <div class="sources" role="group" aria-label="Sources">
              <span class="srclabel">Sources</span>
              {#each m.sources as s (s.n)}
                <button class="srcchip" title={s.url} onclick={() => openExternalLink(s.url)}>
                  <span class="srcn" aria-hidden="true">{s.n}</span>
                  <span class="srctitle">{s.title || s.site}</span>
                  <!-- (Google Search's links are titled with their site: once is enough.) -->
                  {#if s.site && s.site !== s.title}<span class="srcsite">{s.site}</span>{/if}
                </button>
              {/each}
            </div>
          {/if}
          {#if m.role === 'assistant' && !m.streaming}
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
              {#if m.stopped}
                <span class="byline stopped" title="You stopped this answer while it was being written">· Stopped</span>
              {/if}
            </div>
          {/if}
        {/each}
        {#if (sending || (assistantPending != null && assistantPending === activeId)) && !streaming}
          <div class="cbubble assistant">
            <p class="cbody thinking">Thinking…</p>
            {#if waitedS >= STILL_WORKING_S}
              <p class="cwait" data-testid="chat-still-working" aria-live="polite">
                {#if modelHere}
                  Still working ({waitedS} s): the model runs on this PC, so this can take a minute.
                {:else}
                  Still working ({waitedS} s)…
                {/if}
              </p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Composer (after Claude Code's): + (voice replies and their volume),
         the message, the mic, and Send once there's something to send;
         under it, quiet, whether answers are read aloud and the model the
         message goes to. The model picker opens above it all. -->
    <div class="composer" data-model-picker-above>
      <div class="inputbar">
        <div class="pluswrap" bind:this={plusEl}>
          <button
            class="plusbtn"
            aria-label="Chat options"
            aria-haspopup="dialog"
            aria-expanded={plusOpen}
            title={speakReplies ? `Voice replies on · ${Math.round(voiceVolume * 100)}%` : 'Voice replies and volume'}
            onclick={() => (plusOpen = !plusOpen)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            <!-- Voice replies are on: a small amber dot, the switch is inside. -->
            {#if speakReplies}<span class="plusdot" aria-hidden="true"></span>{/if}
          </button>
          {#if plusOpen}
            <div class="pluspop" role="dialog" aria-label="Chat options">
              <!-- Spoken replies on / off (Settings → Voice Assistant → Speak replies). -->
              <button class="optrow" aria-pressed={speakReplies} aria-label="Voice replies" title={voiceTitle} onclick={toggleVoiceReplies}>
                {#if speakReplies}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" fill="currentColor" fill-opacity="0.15" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.4 5.6a9 9 0 0 1 0 12.8" /></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 5 6 9H2v6h4l5 4z" /><path d="m22 9-6 6M16 9l6 6" /></svg>
                {/if}
                <span class="opttext">
                  <span class="optname">Voice replies</span>
                  <span class="optdesc">{speakReplies ? (speakWhen === 'always' ? 'Every answer is read aloud' : 'Answers to spoken questions are read aloud') : 'Answers stay quiet'}</span>
                </span>
                <span class="switch" class:on={speakReplies} aria-hidden="true"><span></span></span>
              </button>
              <div class="optvol">
                <label class="vollabel" for="chat-voice-volume">Voice volume</label>
                <div class="volrow">
                  <input
                    id="chat-voice-volume"
                    type="range"
                    min="0"
                    max="100"
                    step="5"
                    value={Math.round(voiceVolume * 100)}
                    oninput={(e) => moveVolume(Number(e.currentTarget.value) / 100)}
                    onchange={keepVolume}
                  />
                  <span class="volval">{Math.round(voiceVolume * 100)}%</span>
                </div>
              </div>
              <div class="optsep"></div>
              <button
                class="optlink"
                onclick={() => {
                  plusOpen = false;
                  onopensettings?.('voice');
                }}>Voice settings…</button
              >
            </div>
          {/if}
        </div>
        <input
          class="msginput"
          bind:this={inputEl}
          bind:value={input}
          placeholder="Ask about your notes, or anything else"
          aria-label="Message"
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
        <button class="micbtn" title="Dictate your message" aria-label="Dictate your message" onclick={micChat}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v1a7 7 0 0 0 14 0v-1" /><path d="M12 18v4" /></svg>
        </button>
        {#if sending}
          <!-- ■: stop the answer being written (Esc too). -->
          <button class="sendbtn stopbtn" disabled={stopping} onclick={stopAnswer} aria-label="Stop the answer" title="Stop (Esc)">
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
          </button>
        {:else if input.trim()}
          <button class="sendbtn" disabled={sending} onclick={send} aria-label="Send" title="Send (Enter)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 10 4 15l5 5" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
          </button>
        {/if}
      </div>
      <div class="composerfoot">
        <ModelChip {onopensettings} quiet />
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
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .listmore {
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .listmore:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .listmore svg {
    width: 14px;
    height: 14px;
  }
  .selbar {
    gap: 6px;
    padding: 12px 10px 4px;
  }
  .selcount {
    flex: 1 1 auto;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .selbtn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .selbtn:hover {
    border-color: var(--yap-border-hover);
  }
  .selbtn.danger {
    color: var(--yap-danger);
  }
  .conv.picked,
  .conv.picked:hover,
  .conv.picked.active {
    background: var(--yap-primary-tint);
  }
  .pickmark {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    display: grid;
    place-items: center;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .pickmark svg {
    width: 10px;
    height: 10px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 9px;
    flex: 1 1 auto;
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
  .conv.menuopen {
    background: var(--yap-s2);
  }
  .conv.active.menuopen {
    background: var(--yap-primary-wash);
  }
  .conv:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .conv.renaming {
    padding: 3px 4px;
    cursor: default;
  }
  .renameinput {
    width: 100%;
    min-width: 0;
    height: 26px;
    padding: 0 7px;
    box-sizing: border-box;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    outline: none;
  }
  /* The ⋯ on a row (its menu without right-clicking), as in Notes: shown on
     hover, on focus and while its menu is open. */
  .more {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .more:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .more:focus-visible {
    opacity: 1;
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .more svg {
    width: 13px;
    height: 13px;
  }
  .conv:hover .more,
  .conv:focus-within .more,
  .conv.menuopen .more {
    opacity: 1;
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
  /* The Voice Assistant's line: a gentle way to its shortcut. */
  .empty .asknote {
    max-width: 280px;
    margin-top: 6px;
    color: var(--yap-muted);
  }
  .empty .asknote.needsai {
    max-width: 320px;
    color: var(--yap-fg-80);
  }
  .asknote kbd {
    padding: 1px 6px;
    border: 1px solid var(--yap-key-border);
    border-radius: var(--yap-r-sm);
    background: var(--yap-key-bg);
    color: var(--yap-key-fg);
    font: inherit;
    font-size: 11px;
    font-weight: 650;
  }
  .setkey {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }
  .setkey:hover {
    text-decoration: underline;
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
  /* Reading text: Settings → General → Appearance (app.css). */
  .cbubble {
    max-width: 78%;
    border-radius: 12px;
    padding: 8px 12px;
    font-family: var(--yap-font-reading);
    font-size: calc(12.5px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
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
  .cbubble:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .cbody {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .cbody.thinking {
    color: var(--yap-muted-55);
    font-style: italic;
  }
  /* An answer being written: a soft caret at the end of its last line. */
  .cbubble.streaming .cbody > :global(:is(p, h2, h3, h4, h5, blockquote):last-child)::after,
  .cbubble.streaming .cbody > :global(:is(ul, ol):last-child > li:last-child)::after {
    content: '';
    display: inline-block;
    width: 7px;
    height: 7px;
    margin-left: 5px;
    border-radius: 50%;
    background: var(--yap-primary);
    vertical-align: middle;
    animation: caret 1s ease-in-out infinite;
  }
  @keyframes caret {
    0%,
    100% {
      opacity: 0.25;
    }
    50% {
      opacity: 0.9;
    }
  }
  .toolchip.doing {
    color: var(--yap-fg);
  }
  .spin {
    width: 9px;
    height: 9px;
    border: 1.5px solid var(--yap-border);
    border-top-color: var(--yap-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .cbubble.streaming .cbody > :global(*)::after,
    .cbubble.streaming .cbody > :global(* > li)::after,
    .spin {
      animation: none;
    }
  }
  :global(:root[data-motion='reduced']) .cbubble.streaming .cbody > :global(*)::after,
  :global(:root[data-motion='reduced']) .cbubble.streaming .cbody > :global(* > li)::after,
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  .stoppednote {
    align-self: flex-end;
    margin: -4px 4px 2px 0;
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }
  .cwait {
    margin: 4px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
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
  .toolchip.guard svg {
    color: var(--yap-success);
  }
  /* A web answer's citations: small numbered pills in the text. */
  .cbubble.assistant :global(.md-cites) {
    display: inline-flex;
    gap: 2px;
    margin-left: 2px;
    vertical-align: 1px;
  }
  .cbubble.assistant :global(a.md-cite) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
    line-height: 1;
    text-decoration: none;
  }
  .cbubble.assistant :global(a.md-cite:hover) {
    color: var(--yap-primary);
    border-color: color-mix(in srgb, var(--yap-primary) 45%, var(--yap-border-subtle));
  }
  /* The Sources row under a web answer. */
  .sources {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: 78%;
    margin: -2px 0 0 2px;
  }
  .srclabel {
    margin-right: 2px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .srcchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 260px;
    padding: 2px 8px 2px 3px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }
  .srcchip:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .srcchip:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .srcn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
  }
  .srctitle {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .srcsite {
    flex: 0 0 auto;
    color: var(--yap-muted-55);
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
    font-size: calc(13px * var(--yap-reading-scale));
  }
  .cbubble.assistant :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(11.5px * var(--yap-reading-step));
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

  .vollabel {
    display: block;
    margin-bottom: 6px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .volrow {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .volrow input {
    flex: 1 1 auto;
    min-width: 0;
    accent-color: var(--yap-primary);
  }
  .volval {
    width: 34px;
    text-align: right;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    color: var(--yap-muted);
  }

  .composer {
    flex: 0 0 auto;
    margin-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .inputbar {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 48px;
    padding: 6px 8px 6px 6px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 14px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .inputbar:focus-within {
    border-color: var(--yap-border-hover);
  }
  .pluswrap {
    position: relative;
    flex: 0 0 auto;
  }
  .plusbtn {
    position: relative;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 9px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .plusbtn svg {
    width: 16px;
    height: 16px;
  }
  .plusbtn:hover,
  .plusbtn[aria-expanded='true'] {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .plusbtn:focus-visible,
  .optrow:focus-visible,
  .optlink:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .pluspop {
    position: absolute;
    left: 0;
    bottom: calc(100% + 10px);
    z-index: 20;
    width: 280px;
    padding: 6px;
    border: 1px solid var(--yap-border);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
  }
  .optrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .optrow:hover {
    background: var(--yap-s3);
  }
  .optrow > svg {
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
    color: var(--yap-muted);
  }
  .optrow[aria-pressed='true'] > svg {
    color: var(--yap-primary);
  }
  .opttext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .optname {
    font-size: 12.5px;
    font-weight: 600;
  }
  .optdesc {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .switch {
    flex: 0 0 auto;
    width: 28px;
    height: 16px;
    padding: 2px;
    border-radius: 999px;
    background: var(--yap-raised);
    transition: background var(--yap-dur);
  }
  .switch span {
    display: block;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    transition: transform var(--yap-dur);
  }
  .switch.on {
    background: var(--yap-primary);
  }
  .switch.on span {
    transform: translateX(12px);
  }
  .optvol {
    padding: 6px 8px 8px 34px;
  }
  .optsep {
    height: 1px;
    margin: 2px 6px;
    background: var(--yap-border-subtle);
  }
  .optlink {
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .optlink:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .composerfoot {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 4px;
  }
  .plusdot {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--yap-primary);
    box-shadow: 0 0 0 2px var(--yap-s2);
  }
  .spacer {
    flex: 1 1 auto;
  }
  .micbtn {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 9px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    flex: 0 0 auto;
  }
  .micbtn:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .micbtn:focus-visible,
  .sendbtn:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .micbtn svg {
    width: 15px;
    height: 15px;
  }
  .msginput {
    flex: 1 1 auto;
    min-width: 0;
    height: 32px;
    padding: 0 4px;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
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
    width: 15px;
    height: 15px;
  }
</style>
