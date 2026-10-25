// The Chat view's state and what it does (ChatView.svelte makes one each
// time Chat opens; the conversation list, the thread and the composer read
// and change it through the object returned): the conversations, the one
// open and its messages, sending and the streamed answer (■ Stop, a newer
// question replacing one still being answered), a question asked out loud
// (the Voice Assistant), Snip to Chat's screenshots and whether the model
// reads them, spoken replies, and several conversations picked at once.
import { invoke } from '@tauri-apps/api/core';
import { tick, untrack } from 'svelte';
import { toast } from '../../ui/toast.svelte.js';
import { askConfirm } from '../../ui/contextMenu.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { modelName } from '../../core/aiModel.js';
import { aiError } from '../../core/aiErrors.js';
import { streamCall, liveAnswer, applyEvent, streamingHeight, STOPPED, SUPERSEDED } from '../../core/streaming.js';
import { speech, initSpeech, readAloud as speakText, speakReply, stopReading as stopSpeech } from '../../stores/speech.svelte.js';
import { DEFAULT_PROMPT, MAX_SHOTS, visionRefused, pastedImage, keepPasted } from './chatImages.js';
import { waitingShots, followWaitingShots, removeWaiting, clearWaiting } from './chatShots.svelte.js';

// The sources an answer's citations point at: its own, else — a follow-up
// that cites "[1]" from an earlier answer without searching again ("the
// top five in that list") — the latest earlier answer's in this chat, so
// its markers are pills too, never raw "[1][2]".
const CITES = /\[\d{1,2}(?:\s*,\s*\d{1,2})*\]/;

// A slow answer says it's still coming: a model on this PC's processor
// can take a minute (Rust gives it 2 minutes, llm::LOCAL_CHAT_TIMEOUT),
// which a bare "Thinking…" makes look hung.
export const STILL_WORKING_S = 8;

/**
 * One Chat view's state. Call it while the view is made (its effects belong
 * to the view). `onopensettings()`: ControlPanel's Settings opener, as the
 * view has it now (the "Couldn't send" toast's Open Language Models).
 */
export function createChatState({ onopensettings = () => null, onopennote = () => null } = {}) {
  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // { role, text, tools?, model?, voice?, id }

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
  // was pressed; and its "a newer question replaces it" (streamCall's
  // supersede).
  let stopAnswerCall = null;
  let supersedeAnswerCall = null;
  let stopping = $state(false);
  // An answer is being written in the thread (its bubble shows, not "Thinking…").
  const streaming = $derived(messages.some((m) => m.streaming));
  let inputEl = $state(null);
  let threadEl = $state(null);
  // 'meetings' when this chat asks across the meetings (the Meetings view's
  // Ask bar, or "Ask across your meetings" here): answers come from the
  // passages Yap finds on this PC and cite them (search/ask.rs). Kept with
  // the conversation (chats.rs `scope`), so its follow-ups are too.
  let scope = $state(null);

  // ---- Snip to Chat (snip.rs)
  // Screenshots waiting in the message box: { id, w, h, bytes, source };
  // Rust keeps them (snip/waiting.rs: a spoken question takes them too) and
  // this follows it, kept while another view shows (chatShots.svelte.js).
  followWaitingShots();
  // Can the Chat model read them (ai_model_status): images true | false |
  // null (not known: it may try), the switch Rust suggests, the model.
  let vision = $state({ images: null, imagesSwitch: null, model: null });
  // "Keep chat screenshots" (config chatImageDays), for a removed one's line.
  let keepDays = $state(30);
  // A screenshot opened large (its id), or null.
  let viewing = $state(null);
  // The snip shortcut, for the + menu's "Take a screenshot".
  let snipHotkey = $state('');
  // Files are being dragged over the window (a drop lands in the box).
  let dragging = $state(false);
  const visionBlocked = $derived(waitingShots.list.length > 0 && vision.images === false);
  const switchName = $derived.by(() => {
    const s = vision.imagesSwitch;
    if (!s) return '';
    const named = modelName({ provider: s.provider, id: s.model });
    return named && named !== 'Unknown model' ? named : s.name || s.model;
  });

  // ---- Spoken replies
  let speakReplies = $state(false);
  // The composer's + (the maintainer, 2026-10-20, after Claude Code's; the model
  // stays out of it, quiet under the box on the left, always in view): Voice
  // replies and the voice's volume ("so loud, even at 1% on the volume
  // mixer") — moving it changes what's being said at once; letting go keeps
  // it (tts_volume_set), and Settings follows.
  let voiceVolume = $state(0.5);
  let plusOpen = $state(false);
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
  // Numbers this view's sends (their question and bubble carry it: `turn`).
  let sendSeq = 0;
  let alive = true;

  // ---- The Voice Assistant
  // Its shortcut ('' = none set: the empty thread offers to set one).
  let assistantHotkey = $state('');
  // Chat's model is set up (ai_model_status; re-read on every config save).
  let chatReady = $state(true);
  // The conversation whose spoken question is being answered ("Thinking…"),
  // and that question's id ("chat:<conversation>:<index>").
  let assistantPending = $state(null);
  let assistantTurn = null;
  // A spoken question is being listened for (`yap-assistant`): Esc here
  // calls it off (the screenshot stays in the box).
  let assistantListening = $state(false);
  // "New chat" was clicked: a spoken question starts a new conversation
  // rather than continuing the last one.
  let choseNew = false;

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

  // ---- Snip to Chat

  /** Whether the Chat model reads images, and the switch if it can't. */
  async function refreshVision() {
    try {
      const s = await invoke('ai_model_status', { scope: 'chat' });
      vision = { images: s?.images ?? null, imagesSwitch: s?.imagesSwitch ?? null, model: s?.ready ? s.model : null };
    } catch {
      /* keep the last */
    }
  }

  /**
   * A snip landed (`yap-snip`): in the conversation Rust chose — the one
   * open here when the snip started, else a new chat — in the box, the
   * caret there. Never sent by itself.
   */
  async function takeSnip(p) {
    const img = p?.image;
    if (!img?.id) return;
    if (p.conversationId != null) {
      if (p.conversationId !== activeId) await selectConversation(p.conversationId);
    } else if (activeId != null) {
      // Rust has the box as it should be (yap-chat-waiting): kept.
      newChat({ keepShots: true });
    }
    if (!waitingShots.list.some((a) => a.id === img.id)) waitingShots.list = [...waitingShots.list, img].slice(-MAX_SHOTS);
    refreshVision();
    await tick();
    inputEl?.focus();
  }

  /** Pictures pasted or dropped: into the box, as a snip lands (never sent by itself). */
  async function attachPictures(images) {
    const fresh = (images || []).filter((img) => img?.id && !waitingShots.list.some((a) => a.id === img.id));
    if (!fresh.length) return;
    waitingShots.list = [...waitingShots.list, ...fresh].slice(-MAX_SHOTS);
    refreshVision();
    await tick();
    inputEl?.focus();
  }

  /**
   * Ctrl+V in the box: a picture copied in any app goes in as a screenshot
   * (snip/attach.rs); text pastes as ever.
   */
  async function pasteImage(e) {
    const file = pastedImage(e);
    if (!file) return;
    e.preventDefault();
    try {
      await attachPictures([await keepPasted(file)]);
    } catch (err) {
      toast({ title: "Couldn't paste the picture", description: String(err), variant: 'destructive' });
    }
  }

  /** Files dropped on Chat (Windows' drag and drop): the pictures among them go in the box. */
  async function dropFiles(paths) {
    dragging = false;
    if (!paths?.length) return;
    const room = MAX_SHOTS - waitingShots.list.length;
    if (room <= 0) {
      toast({ title: `A question carries ${MAX_SHOTS} screenshots at most`, description: 'Remove one to add another.', chip: 'Chat' });
      return;
    }
    try {
      const got = await invoke('chat_image_drop', { paths, room });
      await attachPictures(got?.images);
      if (got?.skipped) {
        toast({
          title: got.images?.length ? `${got.skipped} of the files weren't added` : "Those files weren't added",
          description: `Chat takes pictures (PNG, JPEG, GIF, BMP, WebP…), up to ${MAX_SHOTS} a question.`,
          chip: 'Chat',
        });
      }
    } catch (err) {
      toast({ title: "Couldn't add the pictures", description: String(err), variant: 'destructive' });
    }
  }

  async function loadVoiceSettings() {
    try {
      const c = await invoke('get_config');
      if (Number.isFinite(c?.chatImageDays)) keepDays = c.chatImageDays;
      snipHotkey = c?.snipHotkey || '';
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

  function readAloud(m) {
    if (m.id) speakText(m.text, m.id);
  }

  function stopReading() {
    stopSpeech('chat:');
  }

  // A dictated question (dictation/pipeline.rs `deliver_to_chat`): into the box at
  // the caret, then sent when Rust says so.
  async function onChatDictation(p) {
    const text = (p?.text || '').trim();
    if (text) {
      insertAtCaret(text);
      byVoice = true;
    }
    if (p?.send && input.trim()) {
      if (sending) {
        // A newer question while the answer comes: it replaces that one
        // (nothing saved for it), and goes as soon as it has.
        sendWhenFree = true;
        replaceAnswer();
      } else send();
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
      scope = conv.scope || null;
      messages = (conv.messages || []).map((m, i) => ({ ...m, id: chatId(id, i) }));
      scrollThread();
    } catch (e) {
      toast({ title: "Couldn't open conversation", description: String(e), variant: 'destructive' });
    }
  }

  // `keepShots`: a snip landing in a new chat (Rust has the box already).
  function newChat(opts) {
    stopReading();
    activeId = null;
    messages = [];
    input = '';
    if (opts?.keepShots !== true) clearWaiting();
    byVoice = false;
    scope = null;
    choseNew = true;
    reportView();
    setTimeout(() => inputEl?.focus(), 0);
  }

  /** A new chat asked across the meetings (its questions and follow-ups). */
  function askAcrossMeetings() {
    newChat();
    scope = 'meetings';
  }

  /**
   * A passage an answer cites (its pill, its row under the answer): its
   * meeting or note opened in Notes at the line it came from.
   */
  function openSource(s) {
    // A meeting on the calendar with no note yet (chat/tools/calendar.rs):
    // its note, made from the invite as the Meetings view's row makes it.
    if (s?.note == null && s?.event) {
      invoke('calendar_event', { key: s.event, action: 'open' })
        .then((id) => {
          if (id != null) onopennote()?.({ id, line: null, ts: null, field: '' });
        })
        .catch((e) => toast({ title: "Couldn't open the meeting", description: String(e?.message ?? e), variant: 'destructive' }));
      return;
    }
    if (s?.note == null) return;
    onopennote()?.({ id: s.note, line: s.line ?? null, ts: s.ts ?? null, field: s.field === 'calendar' ? '' : s.field || '' });
  }

  // What this view shows, for a question asked out loud (chat/assistant.rs
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

  // A question asked with the assistant shortcut (chat/assistant.rs): this view
  // switches to its conversation and shows it as it goes — the question at
  // once, "Thinking…" while the answer comes, then the answer (read aloud by
  // Rust). Never stops the voice: it's the answer being followed.
  // A spoken question's answer as it's written (chat/assistant.rs): it grows at
  // the end of its conversation while that's the one shown.
  let assistantLive = liveAnswer();
  // The spoken answer's own bubble (`spoken`: never this view's own send's).
  const spokenAt = () => messages.findLastIndex((m) => m.streaming && m.spoken);
  // An event about the spoken question this view follows (one without an
  // id, as before, counts).
  const followed = (p) => !p?.userMessageId || !assistantTurn || p.userMessageId === assistantTurn;
  function followAssistantStream(p) {
    const id = p?.conversationId;
    if (id == null || id !== activeId || assistantPending !== id || !followed(p)) return;
    assistantLive = applyEvent(assistantLive, p.event);
    let at = spokenAt();
    if (at < 0) {
      if (!assistantLive.started) return;
      messages = [...messages, { role: 'assistant', streaming: true, spoken: true, text: '', tools: [], id: null }];
      at = messages.length - 1;
    }
    const last = messages[at];
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
      assistantTurn = p.userMessageId ?? null;
      assistantLive = liveAnswer();
    } else if (['answered', 'failed', 'superseded'].includes(p?.phase)) {
      // An older question a newer one replaced ends after the newer was
      // asked: the newer one stays pending.
      if (!followed(p)) return;
      assistantPending = null;
      assistantTurn = null;
    }
    if (p?.phase === 'superseded') {
      // No answer was saved for it: its bubble goes, its question stays.
      // (Nothing to read back: the newer question's events show the rest.)
      messages = messages.filter((m) => !(m.streaming && m.spoken));
      return;
    }
    if (id == null) return;
    try {
      const conv = await invoke('chat_get', { id });
      if (!alive) return;
      if (activeId !== id) {
        activeId = id;
        scope = conv.scope || null;
      }
      // The answer that streamed here, replaced by the saved (checked) one
      // (only an answer: a newer question's arrival replaces nothing).
      const streamedAt = p?.phase === 'answered' ? spokenAt() : -1;
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
    // A failed answer: Rust says so where it'll be seen (an alert, notices/alert.rs).
  }

  async function removeConversation(id) {
    try {
      await invoke('chat_delete', { id });
    } catch (e) {
      // Deleted but not saved yet: gone here, and Yap says so.
      if (!showSaveError(e)) {
        toast({ title: "Couldn't delete the conversation", description: String(e), variant: 'destructive' });
        return false;
      }
    }
    if (activeId === id) newChat();
    await refreshList();
    return true;
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
      title: all ? (convs.length === 1 ? 'Delete your only chat?' : `Delete all ${convs.length} chats?`) : `Delete ${convs.length} chats?`,
      body: `${count === 1 ? 'Their 1 message goes' : `Their ${count} messages go`} for good. Your notes and meetings stay.`,
      run: () => removeConversations(convs.map((c) => c.id)),
    });
  }

  async function removeConversations(ids) {
    let gone = null;
    try {
      gone = await invoke('chat_delete_many', { ids });
    } catch (e) {
      // Deleted but not saved yet: gone here, and Yap says so (no "deleted").
      if (!showSaveError(e)) {
        toast({ title: "Couldn't delete the chats", description: String(e), variant: 'destructive' });
        return false;
      }
    }
    if (ids.includes(activeId)) newChat();
    clearPicked();
    await refreshList();
    if (gone != null) toast({ title: gone === 1 ? 'Chat deleted' : `${gone} chats deleted`, variant: 'success' });
    return true;
  }

  // The conversation rows, by id (a menu or a dialog gives the focus back to its row).
  let convEls = $state({});

  function askDelete(c) {
    const count = c.messageCount || 0;
    askConfirm({
      title: `Delete “${c.title || 'Untitled'}”?`,
      body: count === 0 ? 'The conversation goes for good.' : `Its ${count === 1 ? 'message goes' : `${count} messages go`} for good.`,
      run: () => removeConversation(c.id),
      returnFocus: convEls[c.id],
    });
  }

  function scrollThread() {
    setTimeout(() => {
      if (threadEl) threadEl.scrollTop = threadEl.scrollHeight;
    }, 0);
  }

  /**
   * Send the question (and the screenshots waiting). `{ asText: true }`:
   * "Send its text instead" — the screenshots' text, read on this PC by
   * Windows (snip/ocr.rs), goes in their place to a model that can't read
   * images. (As a click handler it gets the event: a plain send.)
   */
  async function send(opts) {
    const asText = opts?.asText === true;
    // With a screenshot waiting, an empty box asks "What's this?".
    const shots = waitingShots.list;
    const text = input.trim() || (shots.length ? DEFAULT_PROMPT : '');
    if (!text || sending) return;
    // The model can't read the screenshot: the line above the box says so.
    if (shots.length && vision.images === false && !asText) return;
    const voice = byVoice;
    input = '';
    // Out of the box (Rust takes them with the question, chat_send).
    waitingShots.list = [];
    byVoice = false;
    // The next question: the last answer stops talking.
    stopReading();
    // This send's question and bubble carry its `turn`, and are found by
    // it: a spoken question's bubble may come and go in between (one a
    // newer question replaced), moving them.
    const turn = ++sendSeq;
    messages = [...messages, { role: 'user', text, voice, id: null, turn, images: shots }];
    const firstAt = messages.length - 1;
    const mine = (role) => messages.findIndex((m) => m.turn === turn && m.role === role);
    const questionAt = () => {
      const i = mine('user');
      return i >= 0 ? i : firstAt;
    };
    let askedId = null;
    sending = true;
    stopping = false;
    scrollThread();
    // The answer as it's written: its bubble appears with the first text
    // or tool call and grows in place.
    let live = liveAnswer();
    let made = false;
    const onEvent = (ev) => {
      if (!alive) return;
      if (ev?.kind === 'turn') {
        askedId = ev.userMessageId ?? null;
        // A new conversation: it's in the list from the first question.
        if (activeId == null && ev.conversationId != null) {
          activeId = ev.conversationId;
          refreshList();
        }
        return;
      }
      live = applyEvent(live, ev);
      let ai = mine('assistant');
      if (ai < 0) {
        // Not yet (or no longer: another conversation opened meanwhile).
        if (made || (!live.started && !live.waiting)) return;
        messages = [...messages, { role: 'assistant', streaming: true, text: '', tools: [], id: null, turn }];
        ai = messages.length - 1;
        made = true;
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
    const call = streamCall(
      'chat_send',
      {
        conversationId: activeId,
        text,
        scope,
        voice,
        images: shots.length ? shots.map((s) => s.id) : null,
        asText: asText && shots.length ? true : null,
      },
      onEvent
    );
    stopAnswerCall = call.stop;
    supersedeAnswerCall = call.supersede;
    try {
      const res = await call.promise;
      if (!alive) return;
      activeId = res.conversationId;
      const qi = questionAt();
      const asked = messages[qi];
      const ai = mine('assistant');
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
        revisedFrom: streamed != null && streamed.trim() !== '' && streamed.trim() !== String(res.reply).trim() ? fromHeight : 0,
      };
      // The question as saved: with its screenshots' text when it went as text.
      const saved = asText && res.question ? { text: res.question, images: shots.map((s) => ({ ...s, asText: true })) } : {};
      messages = [...messages.slice(0, qi), { ...asked, ...saved, id: res.userMessageId ?? null }, answer];
      refreshList();
      followThread();
      // Read out by itself ("When to speak", decided in Rust): the voice
      // alone, no reading card (that's ▶'s).
      if (res.speak && answer.id) speakReply(answer.text, answer.id);
    } catch (e) {
      if (!alive) return;
      if (String(e).includes(SUPERSEDED)) {
        // A newer question in this conversation replaced it (asked out
        // loud, or dictated here and sent): no answer was saved, its bubble
        // goes, the question stays as saved. No error: nothing went wrong.
        messages = messages
          .filter((m) => !(m.role === 'assistant' && m.streaming && m.turn === turn))
          .map((m) => (m.role === 'user' && m.turn === turn && m.id == null ? { ...m, id: askedId } : m));
        refreshList();
        return;
      }
      const qi = questionAt();
      if (stopping && String(e).includes(STOPPED)) {
        // Stopped before any of the answer came: the question stays.
        messages = [...messages.slice(0, qi), { ...messages[qi], stopped: true }];
        refreshList();
        return;
      }
      messages = messages.slice(0, qi);
      input = text; // give the message back
      // …and its screenshots (Rust puts them back too, yap-chat-waiting).
      waitingShots.list = shots;
      byVoice = voice;
      // Its text couldn't be read (no text in it, no OCR language): said.
      if (asText) {
        toast({ title: "Couldn't send its text", description: String(e), variant: 'default', kind: 'error', chip: 'Error', icon: 'alert' });
        return;
      }
      // The model couldn't read the screenshot: the line above the box says
      // so, with the switch (Rust remembers it as a "no" now).
      if (shots.length && visionRefused(e)) {
        await refreshVision();
        if (vision.images !== false) vision = { ...vision, images: false };
        return;
      }
      // Said so it's clear what to do (aiErrors.js); the raw error is in the log.
      // A sentence reads as a sentence; only an error Yap can't explain gets
      // the copyable error box.
      const why = aiError(e);
      const openSettings = onopensettings();
      toast({
        title: "Couldn't send",
        description: why.text,
        ...(why.known ? { variant: 'default', kind: 'error', chip: 'Error', icon: 'alert' } : { variant: 'destructive' }),
        ...(why.settings && openSettings
          ? { action: { label: 'Open Language Models', onClick: () => openSettings('cleanup#chat') } }
          : {}),
      });
    } finally {
      sending = false;
      stopping = false;
      stopAnswerCall = null;
      supersedeAnswerCall = null;
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

  /**
   * A newer question is waiting to go (`sendWhenFree`): the answer still
   * coming is dropped, nothing saved for it, so the newer one goes now —
   * not after an answer to a question you've moved on from.
   */
  function replaceAnswer() {
    if (!sending || stopping || !supersedeAnswerCall) return;
    supersedeAnswerCall();
  }

  /** Keep the newest text in view while it's written — unless you scrolled up to read. */
  function followThread() {
    const el = threadEl;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (near) tick().then(() => (el.scrollTop = el.scrollHeight));
  }

  // Rust decides whether a dictation is for the chat: the box with the
  // focus is one way (the hotkey in Yap's window, the bar's Dictate).
  function reportFocus(focused) {
    invoke('chat_input_focus', { focused }).catch(() => {});
  }

  /** Leaving Chat: nothing answers into this view any more. */
  function close() {
    alive = false;
  }

  return {
    get conversations() { return conversations; },
    set conversations(v) { conversations = v; },
    get activeId() { return activeId; },
    get messages() { return messages; },
    get input() { return input; },
    set input(v) { input = v; },
    get sending() { return sending; },
    get stopping() { return stopping; },
    get streaming() { return streaming; },
    get inputEl() { return inputEl; },
    set inputEl(v) { inputEl = v; },
    get threadEl() { return threadEl; },
    set threadEl(v) { threadEl = v; },
    get scope() { return scope; },
    set scope(v) { scope = v; },
    get attached() { return waitingShots.list; },
    get copiedShot() { return waitingShots.copied; },
    removeShot: removeWaiting,
    clearShots: clearWaiting,
    get assistantListening() { return assistantListening; },
    set assistantListening(v) { assistantListening = v; },
    get vision() { return vision; },
    get keepDays() { return keepDays; },
    get viewing() { return viewing; },
    set viewing(v) { viewing = v; },
    get snipHotkey() { return snipHotkey; },
    get dragging() { return dragging; },
    set dragging(v) { dragging = v; },
    get visionBlocked() { return visionBlocked; },
    get switchName() { return switchName; },
    get speakReplies() { return speakReplies; },
    set speakReplies(v) { speakReplies = v; },
    get voiceVolume() { return voiceVolume; },
    set voiceVolume(v) { voiceVolume = v; },
    get plusOpen() { return plusOpen; },
    set plusOpen(v) { plusOpen = v; },
    get speakWhen() { return speakWhen; },
    get speakingId() { return speakingId; },
    get byVoice() { return byVoice; },
    set byVoice(v) { byVoice = v; },
    get assistantHotkey() { return assistantHotkey; },
    get chatReady() { return chatReady; },
    get assistantPending() { return assistantPending; },
    get waitedS() { return waitedS; },
    get modelHere() { return modelHere; },
    get picked() { return picked; },
    set picked(v) { picked = v; },
    get groups() { return groups; },
    get ordered() { return ordered; },
    get convEls() { return convEls; },
    citedSources,
    refreshVision,
    takeSnip,
    attachPictures,
    pasteImage,
    dropFiles,
    loadVoiceSettings,
    readAloud,
    stopReading,
    onChatDictation,
    refreshList,
    selectConversation,
    newChat,
    askAcrossMeetings,
    openSource,
    reportView,
    followAssistantStream,
    followAssistant,
    clickConversation,
    clearPicked,
    askDeleteMany,
    askDelete,
    send,
    stopAnswer,
    reportFocus,
    close,
  };
}
