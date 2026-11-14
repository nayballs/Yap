// The Chat view's state and what it does (ChatView.svelte makes one each
// time Chat opens; the conversation list, the thread and the composer read
// and change it through the object returned): the conversations, the one
// open and its messages, sending and the streamed answer (■ Stop, a newer
// question replacing one still being answered), a question asked out loud
// (the Voice Assistant), Snip to Chat's screenshots and whether the model
// reads them, spoken replies, and several conversations picked at once.
// An answer being written belongs to its conversation (`Answer`, 7 Nov
// 2026): several can be answering at once, and the thread and the composer
// show the open one's only.
import { invoke } from '@tauri-apps/api/core';
import { tick, untrack } from 'svelte';
import { toast } from '../../ui/toast.svelte.js';
import { askConfirm } from '../../ui/contextMenu.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { modelName } from '../../core/aiModel.js';
import { aiError, claudePlanProblem, CLAUDE_USAGE_URL } from '../../core/aiErrors.js';
import { openExternalLink } from '../../core/externalLinks.js';
import { signInClaudePlan } from '../../stores/claudePlan.svelte.js';
import { streamCall, liveAnswer, applyEvent, streamingHeight, STOPPED, SUPERSEDED } from '../../core/streaming.js';
import { speech, initSpeech, readAloud as speakText, speakReply, stopReading as stopSpeech } from '../../stores/speech.svelte.js';
import { DEFAULT_PROMPT, MAX_SHOTS, visionRefused, pastedImage, keepPasted } from './chatImages.js';
import { waitingShots, followWaitingShots, removeWaiting, clearWaiting, followPixelRatio } from './chatShots.svelte.js';
import { questionTokens, isTooLong } from './chatBudget.js';
import { fmtContext } from '../../core/aiUsage.js';
import { SkillPicker } from '../../stores/skills.svelte.js';
import { think, takeThink } from './chatThink.svelte.js';
import { createDocPanel } from './chatDoc.svelte.js';
import { createChatConnectors } from './chatConnectors.svelte.js';
// Chat's status dots (stores/chatStatus.svelte.js): the list as read here,
// and the ask-first cards waiting in a turn (needs you).
import { chatStatus, tookList, cardAsked, turnOver } from '../../stores/chatStatus.svelte.js';

// The sources an answer's citations point at: its own, else — a follow-up
// that cites "[1]" from an earlier answer without searching again ("the
// top five in that list") — the latest earlier answer's in this chat, so
// its markers are pills too, never raw "[1][2]".
const CITES = /\[\d{1,2}(?:\s*,\s*\d{1,2})*\]/;

// A slow answer says it's still coming: a model on this PC's processor
// can take a minute (Rust gives it 2 minutes, llm::LOCAL_CHAT_TIMEOUT),
// which a bare "Thinking…" makes look hung.
export const STILL_WORKING_S = 8;

// A turn's events for the panel beside the chat (chatState's `docEvent`):
// its conversation's, so only while that one is open.
const PANEL_EVENTS = new Set(['page', 'offer', 'picture', 'document', 'section', 'agent']);

/** `have` with the pictures in `more` it hasn't got (by id); `have` itself when none are new. */
function withImages(have, more) {
  if (!more?.length) return have;
  const ids = new Set((have || []).map((i) => i?.id));
  const added = more.filter((i) => i?.id && !ids.has(i.id));
  return added.length ? [...(have || []), ...added] : have;
}

/**
 * **An answer being written, in its own conversation** (2026-11-07): this
 * view's typed question's, or a question asked out loud (the Voice
 * Assistant, a phone) that the view follows. Several conversations can be
 * answering at once — Rust ends a turn only for a newer question in its
 * own conversation (chat/stream.rs `Stoppable::answers`) —, so each keeps
 * its own: its question, the answer so far, what it's doing and since when,
 * its ■. The thread shows the open conversation's only; opened again, it
 * picks up where it is.
 */
class Answer {
  /** Its conversation (null: a new chat's, until its question makes it). */
  conv = $state(null);
  /** What it's doing now (chat/activity.rs `step`), and a dictation being waited out. */
  step = $state(null);
  waiting = $state(false);
  /** Its ■ was pressed. */
  stopping = $state(false);

  constructor({ conv = null, turn = null, spoken = false, view = 0, askedId = null, question = null }) {
    this.conv = conv;
    // This view's send number (a typed question: its question and bubble carry it), or null.
    this.turn = turn;
    // Asked out loud (the Voice Assistant, a phone): its bubble is `spoken`.
    this.spoken = spoken;
    // The new chat it was asked in (`viewSeq`), while it has no conversation.
    this.view = view;
    // Its question as saved ("chat:<conversation>:<index>"), once it is.
    this.askedId = askedId;
    // The question as it was sent: shown until the conversation's read back with it.
    this.question = question;
    this.since = Date.now();
    this.live = liveAnswer();
    // What it showed so far, for its bubble when its conversation opens again:
    // the ask-first cards, the documents, pictures, pages and buttons, the screen it looked at.
    this.confirms = null;
    this.docs = [];
    this.pictures = [];
    // The shots the model took of a page (its steps' thumbnails: chat/pages/see.rs).
    this.shots = [];
    this.pages = [];
    this.offers = [];
    // The files it shared into the conversation (chat/shares.rs), a share a call.
    this.shares = [];
    this.screens = [];
    this.pagesShown = new Set();
    // Its ■ and its "a newer question replaces it" (streamCall's), a typed one's.
    this.stop = null;
    this.supersede = null;
  }

  /** What it showed beside the chat, kept for its bubble (a page, a button, a picture, a document). */
  collect(ev) {
    if (ev?.kind === 'picture' && ev.picture?.id && !this.pictures.some((p) => p.id === ev.picture.id)) this.pictures = [...this.pictures, ev.picture];
    if (ev?.kind === 'shot' && ev.picture?.id && !this.shots.some((p) => p.id === ev.picture.id)) this.shots = [...this.shots, ev.picture];
    if (ev?.kind === 'page' && ev.page?.url && !this.pages.some((p) => p.url === ev.page.url)) this.pages = [...this.pages, ev.page];
    if (ev?.kind === 'offer' && ev.offer?.url && !this.offers.some((o) => o.url === ev.offer.url)) this.offers = [...this.offers, ev.offer];
    if (ev?.kind === 'share' && ev.share?.items?.length) this.shares = [...this.shares, ev.share];
    if (ev?.kind === 'document' && ev.noteId != null) {
      const d = { id: ev.noteId, title: ev.title };
      this.docs = this.docs.some((x) => x.id === d.id) ? this.docs.map((x) => (x.id === d.id ? d : x)) : [...this.docs, d];
    }
  }

  /** The step's line and the dictation waited out, from the answer so far. */
  took(ev) {
    this.live = applyEvent(this.live, ev);
    this.step = this.live.step;
    this.waiting = this.live.waiting;
  }
}

/**
 * One Chat view's state. Call it while the view is made (its effects belong
 * to the view). `onopensettings()`: ControlPanel's Settings opener, as the
 * view has it now (the "Couldn't send" toast's Open Language Models).
 */
export function createChatState({ onopensettings = () => null, onopennote = () => null } = {}) {
  let conversations = $state([]);
  let activeId = $state(null);
  let messages = $state([]); // { role, text, tools?, model?, voice?, id }
  // The panel beside the conversation (chat/documents.rs, chatDoc.svelte.js):
  // a tab per document and picture of this conversation.
  // The tabs closed in the open conversation's panel: kept with it (chats.rs `closed_tabs`), so they stay closed.
  const doc = createDocPanel({
    onopennote,
    onclosed: (keys) => {
      if (activeId != null) invoke('chat_tabs_closed', { id: activeId, keys }).catch(() => {});
    },
  });
  // This chat's connectors (the + menu's Connectors ▸, chatConnectors.svelte.js).
  const connectors = createChatConnectors(() => activeId);

  /**
   * A turn's document events (`document`: made or changed, the panel shows
   * it; `section`: one being written), a picture it showed (`picture`, the
   * panel shows it: chat/pictures.rs) and its tool calls, for the panel;
   * `m`: the answer's bubble, which gets the document's or the picture's
   * pill as it comes. True when the event was the panel's alone.
   */
  function docEvent(ev, m) {
    // A live page brought up beside the chat (chat/pages.rs): its Page tab.
    if (ev?.kind === 'page') {
      const p = ev.page;
      if (!p?.url) return true;
      doc.showPage(p);
      if (m && !(m.livePages || []).some((x) => x.url === p.url)) m.livePages = [...(m.livePages || []), p];
      return true;
    }
    // A button to open a page in the person's browser, under the answer.
    if (ev?.kind === 'offer') {
      const o = ev.offer;
      if (m && o?.url && !(m.liveOffers || []).some((x) => x.url === o.url)) m.liveOffers = [...(m.liveOffers || []), o];
      return true;
    }
    if (ev?.kind === 'picture') {
      const p = ev.picture;
      if (!p?.id) return true;
      doc.showPicture(p);
      if (m && !(m.livePictures || []).some((x) => x.id === p.id)) m.livePictures = [...(m.livePictures || []), p];
      return true;
    }
    if (ev?.kind === 'document') {
      doc.fromTurn(ev);
      if (m) {
        const have = m.liveDocs || [];
        m.liveDocs = have.some((d) => d.id === ev.noteId)
          ? have.map((d) => (d.id === ev.noteId ? { id: ev.noteId, title: ev.title } : d))
          : [...have, { id: ev.noteId, title: ev.title }];
      }
      return true;
    }
    if (ev?.kind === 'section') {
      doc.onSection(ev);
      return true;
    }
    // An agent this turn started (chat/tools/agents.rs): its tab opens at once,
    // as a page's does; its pill comes with the agents list (stores/agents.svelte.js).
    // Opened by itself (`auto`): its panel's rows stay as the person left them.
    if (ev?.kind === 'agent') {
      if (ev.agent?.id) doc.showAgent({ ...ev.agent, auto: true });
      return true;
    }
    if (ev?.kind === 'tool') doc.onTool(ev);
    return false;
  }

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
  // A passage selected in the document panel ("Ask about this", "Change
  // this…"): `{noteId, title, text, fromLine, toLine, mode}`, quoted over the
  // box and sent with the next question (chat_send's `selection`).
  let quote = $state(null);

  // ---- Answers being written, each in its own conversation (`Answer`)
  // By conversation id (a new chat's typed question: "new:<turn>" until its
  // question makes the conversation).
  let turns = $state({});
  // Numbers the new chats shown (`newChat`): a new chat's question, before
  // its conversation is made, belongs to the new chat it was asked in.
  let viewSeq = $state(0);
  const keyOf = (rec) => (rec.conv != null ? String(rec.conv) : `new:${rec.turn}`);
  function setTurn(rec) {
    turns[keyOf(rec)] = rec;
  }
  /** `rec` is over: out of the list (unless a newer one there replaced it already). */
  function dropTurn(rec) {
    const k = keyOf(rec);
    if (turns[k] === rec) delete turns[k];
  }
  /** `rec`'s conversation was made (a new chat's first question saved): kept under its id. */
  function adopt(rec, conv) {
    if (rec.conv === conv) return;
    dropTurn(rec);
    rec.conv = conv;
    setTurn(rec);
  }
  /** `rec`'s conversation is the one open (a new chat's: the new chat it was asked in). */
  const isShown = (rec) => (rec.conv != null ? activeId === rec.conv : activeId == null && rec.view === viewSeq);
  /** **The open conversation's answer being written**, if any: the only one the thread and the composer show. */
  const here = $derived.by(() => {
    if (activeId != null) return turns[String(activeId)] ?? null;
    for (const r of Object.values(turns)) if (r.conv == null && r.view === viewSeq) return r;
    return null;
  });
  // The open conversation's typed question is being answered: Enter waits
  // (type ahead), Send is ■. (Another conversation answering never holds
  // this one up: Rust answers each in its own turn.)
  const sending = $derived(!!here && !here.spoken);
  const stopping = $derived(!!here?.stopping);
  // An answer is being written in the thread (its bubble shows, not "Thinking…").
  const streaming = $derived(messages.some((m) => m.streaming));
  // The Voice Assistant's phase (`yap-assistant`: idle, listening, thinking,
  // speaking) and the conversation its question went to. While it thinks or
  // speaks in the open conversation (or an answer asked out loud is still
  // being written there), ■ and Esc stop it, voice and all; in another
  // conversation, nothing of it shows here.
  let assistantPhase = $state('idle');
  let assistantConv = $state(null);
  const assistantAnswering = $derived(
    !!here?.spoken ||
      ((assistantPhase === 'thinking' || assistantPhase === 'speaking') && assistantConv != null && assistantConv === activeId) ||
      messages.some((m) => m.streaming && m.spoken)
  );
  /** `yap-assistant`'s payload (or `assistant_phase`'s; a bare phase from before). */
  function setAssistant(v) {
    if (v == null || typeof v === 'string') {
      assistantPhase = v || 'idle';
      return;
    }
    assistantPhase = v.phase || 'idle';
    assistantConv = v.conversationId ?? null;
  }
  /** Where the Voice Assistant is now, read again (its question's conversation, once it's chosen). */
  // **A read of the phase never overwrites a newer event** (14 Nov 2026): each `yap-assistant` event heard
  // counts (the `assistantPhase` setter); a read begun before one is stale, and its answer is dropped.
  // A question asked out loud that went straight to a Code chat's session goes idle at once: the read
  // began while it was thinking, came back after the idle event, and left ■ "Stop the answer" in the box
  // for good — Send gone, nothing typed could go.
  let phaseHeard = 0;
  /** The phase as Rust has it now: taken unless an event came meanwhile; the answer either way. */
  function readAssistant() {
    const at = phaseHeard;
    return invoke('assistant_phase').then((v) => {
      if (at === phaseHeard) setAssistant(v);
      return v;
    });
  }
  function refreshAssistant() {
    readAssistant().catch(() => {});
  }
  let inputEl = $state(null);
  let threadEl = $state(null);
  // The thread's scrollbar's width now (ChatThread measures it, 0 without
  // one): the composer's column stays centred on the messages' over it.
  let threadGutter = $state(0);
  // The thread following its bottom (ChatThread's core/stickToBottom.js),
  // while it's drawn: a question sent and a conversation opened go back
  // there; everything else that makes it grow it follows by itself.
  let follow = null;
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
  // The screen's pixel ratio: screenshots are drawn at their device pixels.
  followPixelRatio();
  // Can the Chat model read them (ai_model_status): images true | false |
  // null (not known: it may try), the switch Rust suggests, the model.
  // `imagesLocal`: the on-device model's projector to download or switch on
  // ("Read screenshots on this PC", ai/local_llm/vision.rs).
  let vision = $state({ images: null, imagesSwitch: null, imagesLocal: null, model: null });
  // "Keep chat screenshots" (config chatImageDays), for a removed one's line.
  let keepDays = $state(30);
  // A screenshot opened large ({ id, w, h }: ShotViewer.svelte), or null.
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
  // A hands-free question arrived while the open conversation's answer was
  // still coming: sent once that one (this `Answer`) is over.
  let sendWhenFree = null;
  // Numbers this view's sends (their question and bubble carry it: `turn`).
  let sendSeq = 0;
  let alive = true;

  // ---- The Voice Assistant
  // Its shortcut ('' = none set: the empty thread offers to set one).
  let assistantHotkey = $state('');
  // Chat's model is set up (ai_model_status; re-read on every config save).
  let chatReady = $state(true);
  // The open conversation, while a question asked out loud is being answered there ("Thinking…").
  const assistantPending = $derived(here?.spoken ? here.conv : null);
  // A spoken question is being listened for (`yap-assistant`): Esc here
  // calls it off (the screenshot stays in the box).
  let assistantListening = $state(false);
  // "New chat" was clicked: a spoken question starts a new conversation
  // rather than continuing the last one.
  let choseNew = false;

  let modelHere = $state(false);
  // What the open conversation's answer is doing now (chat/activity.rs
  // `step` events): the line at the thread's foot (ActivityLine.svelte),
  // beside how long it's taken so far (`waitedS`, from when it was asked:
  // opened again mid-answer, the time goes on). None yet: "Thinking".
  // `activityWaiting`: a dictation is being waited out.
  const activity = $derived(here?.step ?? null);
  const activityWaiting = $derived(!!here?.waiting);
  let clock = $state(Date.now());
  const waitedS = $derived(here ? Math.max(0, Math.floor((clock - here.since) / 1000)) : 0);
  // Goes up when an answer (or a question) lands: the context ring reads
  // the conversation's use again (ContextRing.svelte, `chat_context`).
  let contextStamp = $state(0);

  // ---- Sized to the model (chat/budget.rs, chat/compact.rs)
  // What a question may be for the Chat model now (`chat_limits`):
  // { questionMax, context, label, tooLong, more }.
  let limits = $state(null);
  // The open conversation's summary of its older messages ({ text, upto,
  // model }): a divider over the messages after it, "Show summary".
  let summary = $state(null);
  // The summary shown read-only (Show summary).
  let showingSummary = $state(false);
  // The last send's "too long" (Rust's plain sentence), until the next send.
  let tooLong = $state('');
  // A paste too long for the model: said under the box, never sent.
  const inputTokens = $derived(questionTokens(input));
  const inputTooLong = $derived(!!limits?.questionMax && inputTokens > limits.questionMax);

  async function refreshLimits() {
    try {
      limits = await invoke('chat_limits');
    } catch {
      /* keep the last */
    }
  }

  /** A conversation's summary was written (`yap-chat-summary`): its divider and the ring follow. */
  async function onSummary(p) {
    const id = p?.conversationId;
    if (id == null || id !== activeId) return;
    try {
      const conv = await invoke('chat_get', { id });
      if (activeId === id) summary = conv.summary ?? null;
    } catch {
      /* deleted meanwhile */
    }
    contextStamp += 1;
  }

  /**
   * "Give it more room": the on-device model's context raised to the next
   * size that fits the graphics card (Settings → Language Models → Local
   * does the same); it restarts once nothing is using it.
   */
  async function giveMoreRoom() {
    const tokens = limits?.more;
    if (!tokens) return;
    try {
      await invoke('local_llm_set_context', { tokens });
      toast({
        title: `Giving ${limits.label} ${fmtContext(tokens)} of context`,
        description: 'It restarts with the room in a moment. Then send your message again.',
        variant: 'success',
      });
      tooLong = '';
      await refreshLimits();
    } catch (e) {
      toast({ title: "Couldn't give it more room", description: String(e), variant: 'destructive' });
    }
  }

  /** "Shorten what you pasted": the message selected in the box, to cut down. */
  function shortenPaste() {
    tick().then(() => {
      inputEl?.focus();
      inputEl?.select?.();
    });
  }
  $effect(() => {
    if (!here) return;
    invoke('ai_model_status', { scope: 'chat' })
      .then((s) => (modelHere = !!s?.model?.onThisPc))
      .catch(() => {});
    clock = Date.now();
    const timer = setInterval(() => (clock = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  const chatId = (conv, i) => `chat:${conv}:${i}`;

  // ---- An answer's place in the thread (its conversation open)

  /** Its question's index in the thread (-1: not there). */
  function questionAt(rec) {
    if (rec.turn != null) {
      const i = messages.findIndex((m) => m.turn === rec.turn && m.role === 'user');
      if (i >= 0) return i;
    }
    return rec.askedId ? messages.findIndex((m) => m.id === rec.askedId && m.role === 'user') : -1;
  }
  /** Its bubble's index in the thread (-1: none yet). */
  const bubbleAt = (rec) =>
    rec.spoken
      ? messages.findLastIndex((m) => m.streaming && m.spoken)
      : messages.findIndex((m) => m.turn === rec.turn && m.role === 'assistant');
  /**
   * Something to show in its bubble yet: text, a tool call or a card that
   * asks (asked out loud too) — and for a typed question also a dictation
   * waited out, its thinking —; until then the line at the foot says
   * "Thinking".
   */
  const hasBubble = (rec) => rec.live.started || !!rec.confirms?.length || (!rec.spoken && (rec.live.waiting || !!rec.live.thinking));
  /** Its bubble as the answer is now. */
  function bubbleOf(rec) {
    const l = rec.live;
    const b = { role: 'assistant', streaming: true, text: l.text, tools: l.chips, doing: l.doing, liveSources: l.sources, waiting: l.waiting, liveThinking: l.thinking, id: null };
    if (rec.spoken) b.spoken = true;
    else b.turn = rec.turn;
    if (rec.confirms?.length) b.confirms = rec.confirms;
    if (rec.docs.length) b.liveDocs = rec.docs;
    if (rec.pictures.length) b.livePictures = rec.pictures;
    if (rec.shots.length) b.liveShots = rec.shots;
    if (rec.pages.length) b.livePages = rec.pages;
    if (rec.offers.length) b.liveOffers = rec.offers;
    if (rec.shares.length) b.liveFiles = rec.shares;
    return b;
  }
  /** Its conversation open: its bubble drawn as the answer is now (made once there's something to show). */
  function paint(rec) {
    if (!isShown(rec)) return;
    const at = bubbleAt(rec);
    if (at < 0) {
      if (!hasBubble(rec)) return;
      // (A typed answer hangs on its question.)
      if (!rec.spoken && questionAt(rec) < 0) return;
      messages = [...messages, bubbleOf(rec)];
    } else {
      const m = messages[at];
      if (!m?.streaming) return;
      const l = rec.live;
      m.text = l.text;
      m.tools = l.chips;
      m.doing = l.doing;
      m.liveSources = l.sources;
      m.waiting = l.waiting;
      m.liveThinking = l.thinking;
      if (rec.confirms) m.confirms = rec.confirms;
      if (rec.shots.length) m.liveShots = rec.shots;
      if (rec.shares.length) m.liveFiles = rec.shares;
    }
    followThread();
  }
  /** The screen the model looked at (look_at_screen): with its question, as its screenshots are. */
  function lookedAt(rec, images) {
    rec.screens = withImages(rec.screens, images) || [];
    if (!isShown(rec)) return;
    const qi = questionAt(rec);
    if (qi < 0) return;
    const next = withImages(messages[qi].images, images);
    if (next !== messages[qi].images) messages = messages.map((m, i) => (i === qi ? { ...m, images: next } : m));
  }

  /**
   * **Conversation `id`'s saved messages as the thread shows them**, with its
   * answer being written (if any) in place: its question — marked as the
   * turn's, or as it was sent while it isn't saved yet — and the answer so
   * far after it. Opened again mid-answer, it picks up where it is: the text
   * so far and the line, nothing twice.
   */
  function withLive(id, saved) {
    const list = (saved || []).map((m, i) => ({ ...m, id: chatId(id, i) }));
    const rec = turns[String(id)];
    if (!rec) return list;
    let qi = rec.askedId ? list.findIndex((m) => m.id === rec.askedId && m.role === 'user') : -1;
    // (Saved, but its id not told yet: the question at the end, as sent.)
    if (qi < 0 && !rec.askedId && rec.question && list.at(-1)?.role === 'user' && list.at(-1).text === rec.question.text) qi = list.length - 1;
    // Its answer saved already (read as it landed): as saved.
    if (qi >= 0 && list[qi + 1]?.role === 'assistant') return list;
    if (qi < 0) {
      if (!rec.question) return list;
      list.push({ ...rec.question });
      qi = list.length - 1;
    }
    list[qi] = { ...list[qi], ...(rec.turn != null ? { turn: rec.turn } : {}), images: withImages(list[qi].images, rec.screens) };
    if (hasBubble(rec)) list.push(bubbleOf(rec));
    return list;
  }

  // ---- Snip to Chat

  /** Whether the Chat model reads images, and the switch if it can't. */
  async function refreshVision() {
    try {
      const s = await invoke('ai_model_status', { scope: 'chat' });
      vision = {
        images: s?.images ?? null,
        imagesSwitch: s?.imagesSwitch ?? null,
        imagesLocal: s?.imagesLocal ?? null,
        model: s?.ready ? s.model : null,
      };
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
    // The box full: said, as for a drop (never the oldest dropped for it).
    if (waitingShots.list.length >= MAX_SHOTS) {
      boxFull();
      return;
    }
    try {
      await attachPictures([await keepPasted(file)]);
    } catch (err) {
      toast({ title: "Couldn't paste the picture", description: String(err), variant: 'destructive' });
    }
  }

  /** A picture pasted or dropped with the box full: said, nothing taken out. */
  function boxFull() {
    toast({ title: `A question carries ${MAX_SHOTS} screenshots at most`, description: 'Remove one to add another.', chip: 'Chat' });
  }

  /** Files dropped on Chat (Windows' drag and drop): the pictures among them go in the box. */
  async function dropFiles(paths) {
    dragging = false;
    if (!paths?.length) return;
    const room = MAX_SHOTS - waitingShots.list.length;
    if (room <= 0) {
      boxFull();
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
        // A newer question while this chat's answer comes: it replaces that
        // one (nothing saved for it), and goes as soon as it has.
        sendWhenFree = here;
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
      tookList(conversations);
    } catch {
      conversations = [];
    }
  }

  async function selectConversation(id) {
    if (id === activeId) return;
    try {
      const answering = !!turns[String(id)];
      let conv = await invoke('chat_get', { id });
      // Its answer landed while it was read: read again, so the thread has it.
      if (answering && !turns[String(id)]) conv = await invoke('chat_get', { id });
      stopReading();
      // Another conversation: its documents and pictures are its own (their
      // pills reopen the panel, its tabs with it — less the ones closed in it).
      doc.reset(conv.closedTabs);
      quote = null;
      activeId = id;
      scope = conv.scope || null;
      summary = conv.summary ?? null;
      showingSummary = false;
      tooLong = '';
      // (Answering still: its question and the answer so far, where they were.)
      messages = withLive(id, conv.messages);
      scrollThread();
    } catch (e) {
      toast({ title: "Couldn't open conversation", description: String(e), variant: 'destructive' });
    }
  }

  // The open conversation answered by a turn this view doesn't follow (one
  // asked before Chat was last opened, an agent's report): read again once
  // Rust says it's done (`yap-chat-answering`), so its answer shows.
  // (`{id, followed}`: the open conversation while Rust lists it answering,
  // and whether that's a turn this view follows — its own `Answer`.)
  let answeringSeen = null;
  $effect(() => {
    const id = activeId;
    const listed = id != null && chatStatus.answering.includes(String(id));
    untrack(() => {
      const was = answeringSeen?.id === id ? answeringSeen : null;
      if (listed) {
        answeringSeen = { id, followed: !!here || !!was?.followed };
        return;
      }
      answeringSeen = null;
      if (was && !was.followed && !here && alive) reloadActive();
    });
  });

  // Every chat screenshot deleted (Settings → Privacy & data), an agent's
  // report landing: the open conversation read again (not while its own
  // typed question is answered; another chat's answering never holds it up).
  // Asked for while its own question was being answered: done once that
  // answer is in (`reloadWanted`: the conversation). An agent's report that
  // landed just then was never read in otherwise (11 Nov 2026, seen in a Yap
  // Lab run on the real Claude Code: a question sent in the seconds between
  // an agent finishing and its report — no "Agent done" card, and no pill
  // under the next answer, until the chat was opened again).
  let reloadWanted = null;
  $effect(() => {
    const busy = sending;
    untrack(() => {
      if (busy || reloadWanted == null) return;
      const id = reloadWanted;
      reloadWanted = null;
      if (id === activeId && alive) reloadActive();
    });
  });
  async function reloadActive() {
    if (activeId == null) return;
    if (sending) {
      reloadWanted = activeId;
      return;
    }
    const id = activeId;
    try {
      const conv = await invoke('chat_get', { id });
      if (activeId === id) {
        messages = withLive(id, conv.messages);
        summary = conv.summary ?? null;
      }
    } catch {
      /* it stays as it was */
    }
  }

  /**
   * A passage selected in the document panel, asked about ("ask") or to be
   * changed ("change"): quoted over the box, which takes the focus.
   */
  function setQuote(q) {
    if (!q?.text?.trim() || (q.noteId == null && !q.page)) return;
    quote = { ...q, text: q.text.trim() };
    setTimeout(() => inputEl?.focus(), 0);
  }

  /**
   * Words selected on a page beside the chat, asked about (its menu's "Ask
   * Yap about this", `yap-page-quote`: chat/pages/view.rs): quoted over the
   * box as "About <the page>", asked as a selection in another app is.
   */
  function onPageQuote(p) {
    if (!p?.text?.trim()) return;
    const title = p.title || p.site || 'the page';
    setQuote({ noteId: 0, title, text: p.text, mode: 'ask', page: { url: p.url || '', title, site: p.site || '' } });
  }

  function clearQuote() {
    quote = null;
    inputEl?.focus();
  }

  // The quote over the box, in Rust too (documents.rs `chat_quote_set`): a
  // question asked out loud with Chat in view takes it as a typed or
  // dictated one does (`send`), in the conversation it's quoted in.
  function reportQuote(q, conversationId) {
    // (A page's words go with a typed or dictated question only.)
    const waiting = q && !q.page
      ? { conversationId, noteId: q.noteId, title: q.title || '', text: q.text, fromLine: q.fromLine ?? null, toLine: q.toLine ?? null, mode: q.mode || '' }
      : null;
    invoke('chat_quote_set', { quote: waiting }).catch(() => {});
  }
  $effect(() => {
    const q = quote;
    const conv = activeId;
    untrack(() => {
      if (alive) reportQuote(q, conv);
    });
  });

  /**
   * Rust took the quote for a question asked out loud (`yap-chat-quote`
   * `{quote: null}`: the chip goes, the question shows it), or gave it back
   * when no answer came.
   */
  function onQuote(p) {
    const q = p?.quote;
    if (!q) {
      quote = null;
      return;
    }
    if (!quote && q.conversationId != null && q.conversationId === activeId) {
      quote = { noteId: q.noteId, title: q.title, text: q.text, fromLine: q.fromLine ?? null, toLine: q.toLine ?? null, mode: q.mode || 'ask' };
    }
  }

  // `keepShots`: a snip landing in a new chat (Rust has the box already).
  // `keepInput`: "New chat" under a message too long for this chat — the
  // message goes with it.
  function newChat(opts) {
    stopReading();
    doc.reset();
    quote = null;
    activeId = null;
    // Another new chat: a question still being asked in the last one is that one's.
    viewSeq += 1;
    messages = [];
    summary = null;
    showingSummary = false;
    tooLong = '';
    if (opts?.keepInput !== true) input = '';
    if (opts?.keepShots !== true) clearWaiting();
    byVoice = false;
    scope = null;
    choseNew = true;
    // A new chat has every connector on.
    connectors.resetNew();
    reportView();
    setTimeout(() => inputEl?.focus(), 0);
  }

  /** A new chat asked across the meetings (its questions and follow-ups). */
  function askAcrossMeetings() {
    newChat();
    scope = 'meetings';
  }

  // ---- Skills (chat/skills.rs): "/" in the box, and the one-tap prompts ----
  // A Skill is asked across the meetings, its passages cited, wherever it's
  // run; a plain chat with messages stays a plain chat.
  const skills = new SkillPicker('meetings');
  $effect(() => skills.update(input));
  /** The "/" menu's keys, before the box's own (true: the menu took it). */
  function skillKey(e) {
    const picked = skills.key(e);
    if (picked?.skill) pickSkill(picked.skill, picked.run);
    return !!picked;
  }
  /** A Skill from the "/" menu: run now, or put in the box to add words. */
  function pickSkill(skill, run) {
    input = `/${skill.command}${run ? '' : ' '}`;
    if (run) send();
    else inputEl?.focus();
  }
  /**
   * A one-tap prompt (Write my stand-up, List my open to-dos…):
   * its question, sent at once as its Skill. On an empty chat it's asked
   * across the meetings from then on.
   */
  function runPrompt(skill) {
    if (sending || !skill?.question) return;
    if (!messages.length) scope = 'meetings';
    input = skill.question;
    send({ skill: skill.command });
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
  // asked for, or — nothing chosen — no preference; and a new code chat's
  // workspace (its new conversation is a Code chat there, `new_code_chat`).
  function reportView(open = true) {
    invoke('chat_view_state', {
      open,
      conversationId: activeId,
      newChat: activeId == null && choseNew,
      codeFolder: activeId == null ? (pendingFolder?.() ?? null) : null,
    }).catch(() => {});
  }
  $effect(() => {
    void activeId;
    untrack(() => {
      if (alive) reportView();
    });
  });

  // A question asked with the assistant shortcut (chat/assistant.rs), or on a
  // paired phone: this view switches to its conversation as it's asked and
  // shows it as it goes — the question at once, "Thinking…" while the answer
  // comes, then the answer (read aloud by Rust). Never stops the voice: it's
  // the answer being followed. Each is an `Answer` in its own conversation
  // (`spoken`): left for another chat, its answer goes on being written
  // there, shown again when that one opens, and lands there — the view never
  // switches back for it.

  // The spoken answer's own bubble (`spoken`: never this view's own send's).
  const spokenAt = () => messages.findLastIndex((m) => m.streaming && m.spoken);
  // The conversations this view is opening for a question asked out loud,
  // and the pages of answers that landed meanwhile (`followAssistant`).
  const opening = new Set();
  const landedEarly = new Map();
  // The spoken question being answered in conversation `id`, if any.
  const spokenIn = (id) => (id != null && turns[String(id)]?.spoken ? turns[String(id)] : null);
  // An event about that question (one without an id, as before, counts):
  // an older one a newer question there replaced is left alone, so its
  // events never land in the newer one's bubble.
  const followedBy = (rec, p) => !p?.userMessageId || !rec.askedId || p.userMessageId === rec.askedId;

  /**
   * **Acting on a page beside the chat, and a card that asks first**, as a
   * turn's event — typed or asked out loud (chat/pages/act.rs): its tab says
   * so (with Stop: this turn's ■), a tab the model switched to shows, one it
   * closed goes (its page is gone already), and an action that would buy,
   * pay, book… — or an agent, a connector's tool — asks on its card on the
   * answer being written (PageConfirm.svelte and its kin) and in its page
   * tab's strip (the panel may cover the thread: chatDoc's `asking`).
   * `true`: it was one of these.
   */
  function actingEvent(rec, ev, shown) {
    if (ev?.kind === 'acting') {
      if (shown) doc.setActing(ev.key, () => stopTurn(rec));
      return true;
    }
    if (ev?.kind === 'switchTab') {
      if (shown) doc.showTab(ev.key);
      return true;
    }
    if (ev?.kind === 'closeTab') {
      if (shown) doc.closeTab(ev.key);
      return true;
    }
    if (ev?.kind === 'confirm') {
      if (shown) doc.setAsking(ev);
      const have = rec.confirms || [];
      rec.confirms = have.some((c) => c.id === ev.id) ? have.map((c) => (c.id === ev.id ? ev : c)) : [...have, ev];
      paint(rec);
      return true;
    }
    return false;
  }

  /**
   * **What a Code chat's own Claude showed beside the chat** (`yap-agent-shown`,
   * agents/runner.rs: Yap's browser in its session, 14 Nov 2026): a page it
   * opened, a picture it showed — the panel opens on it, as on a plan chat's.
   * Its answer keeps them (`pages`, `pictures`), so they come back with the
   * chat. Another conversation's: nothing here (its answer has them).
   */
  function agentShown(p) {
    if (p?.conversation == null || p.conversation !== activeId) return;
    const ev = p.event;
    if (ev?.kind === 'page' && (ev.page?.url || ev.page?.tabKey)) doc.showPage(ev.page);
    else if (ev?.kind === 'picture' && ev.picture?.id) doc.showPicture(ev.picture);
  }

  function followAssistantStream(p) {
    const id = p?.conversationId;
    // An ask-first card in a spoken question's turn waits on the person (the status dots).
    if (p?.event?.kind === 'confirm') cardAsked(id, p.event);
    const rec = spokenIn(id);
    if (!alive || !rec || !followedBy(rec, p)) return;
    const ev = p.event;
    const shown = isShown(rec);
    // Acting on a page, and a card asking first (a page's, an agent's, a
    // connector's), as for a typed question (8 Nov 2026: a spoken turn acts
    // on pages too; its card is also said and on the Yap bar, spoken_ok.rs).
    if (actingEvent(rec, ev, shown)) return;
    // (A page Yap opened before this view had a new conversation is shown when its answer lands.)
    if (ev?.kind === 'page' && ev.page?.url && shown) rec.pagesShown.add(ev.page.url);
    // The screen the model looked at (look_at_screen): with the question
    // asked out loud, as a typed question's are.
    if (ev?.kind === 'screen' && Array.isArray(ev.images) && ev.images.length) {
      lookedAt(rec, ev.images);
      return;
    }
    rec.collect(ev);
    // A document asked for out loud: the panel shows it filling (its conversation open).
    if (PANEL_EVENTS.has(ev?.kind) || ev?.kind === 'tool') {
      if (shown) {
        const at = bubbleAt(rec);
        if (docEvent(ev, at >= 0 ? messages[at] : null)) return;
      } else if (ev.kind !== 'tool') return;
    }
    rec.took(ev);
    paint(rec);
  }

  async function followAssistant(p) {
    const id = p?.conversationId ?? null;
    const phase = p?.phase;
    // The pages its answer showed live beside the chat.
    let pagesShown = new Set();
    if (phase === 'asked') {
      if (id != null) {
        // Its own `Answer` (a newer question there replaces the one it
        // answered: chat/stream.rs); its question as said, until it's read back.
        setTurn(
          new Answer({
            conv: id,
            spoken: true,
            askedId: p.userMessageId ?? null,
            question: p.text ? { role: 'user', text: p.text, voice: true, id: p.userMessageId ?? null } : null,
          })
        );
        // The Voice Assistant's ■ is for its question's conversation (a phone's isn't its).
        if (assistantPhase === 'thinking') refreshAssistant();
      }
    } else if (phase === 'answered' || phase === 'failed' || phase === 'superseded') {
      const rec = spokenIn(id);
      // An older question a newer one replaced ends after the newer was
      // asked: the newer one stays pending.
      if (rec && !followedBy(rec, p)) return;
      if (rec) {
        pagesShown = rec.pagesShown;
        dropTurn(rec);
      }
      turnOver(id);
      if (id != null && activeId === id) doc.turnEnded();
      if (phase === 'superseded') {
        // No answer was saved for it: its bubble goes, its question stays.
        // (Nothing to read back: the newer question's events show the rest.)
        if (id != null && activeId === id) messages = messages.filter((m) => !(m.streaming && m.spoken));
        return;
      }
    }
    if (id == null) return;
    // The view follows a question as it's asked (and the bar's Open in
    // Chat); an answer landing in another chat stays there (its dot says
    // so), never pulling you away from the one you're reading.
    const follows = phase === 'asked' || phase === 'open';
    if (!follows && activeId !== id) {
      // Answered while this view is still opening that conversation for its
      // question (a quick one: "Show me <address>" needs no model): the
      // pages it showed are brought up once the view gets there.
      if (phase === 'answered' && opening.has(id)) landedEarly.set(id, pagesShown);
      refreshList();
      return;
    }
    if (phase === 'asked') opening.add(id);
    try {
      const conv = await invoke('chat_get', { id });
      if (!alive) return;
      if (!follows && activeId !== id) {
        refreshList();
        return;
      }
      const switched = activeId !== id;
      if (switched) {
        // Another conversation: the panel's documents and pictures were the last one's
        // (its tabs closed before stay closed; a new chat it became keeps what it shows).
        if (activeId != null) doc.reset(conv.closedTabs);
        else doc.takeClosed(conv.closedTabs);
        activeId = id;
        scope = conv.scope || null;
      }
      summary = conv.summary ?? null;
      // Its answer is saved already: a quick one's "answered" came while this
      // view was still opening for the question (ControlPanel brought Chat up
      // on "asked", before this view listened), and it'd say "Thinking" for
      // good. The saved answer after the question ends the wait.
      // (Its pages are shown below, as a landed answer's: "Show me <address>"
      // out loud needs no model, so its answer can land before this view has
      // its conversation, and the "answered" for it found the view elsewhere.)
      let landed = false;
      if (phase === 'asked') {
        opening.delete(id);
        const rec = spokenIn(id);
        const at = Number(String(p.userMessageId ?? '').split(':').pop());
        if (rec && rec.askedId === p.userMessageId && Number.isInteger(at) && conv.messages?.[at + 1]?.role === 'assistant') {
          pagesShown = rec.pagesShown;
          landed = true;
          dropTurn(rec);
          doc.turnEnded();
        } else if (landedEarly.has(id)) {
          pagesShown = landedEarly.get(id);
          landed = true;
        }
        landedEarly.delete(id);
      }
      // The answer that streamed here, replaced by the saved (checked) one
      // (only an answer: a newer question's arrival replaces nothing).
      const streamedAt = phase === 'answered' ? spokenAt() : -1;
      const streamed = streamedAt >= 0 ? messages[streamedAt].text : null;
      const fromHeight = streamingHeight(threadEl);
      messages = withLive(id, conv.messages).map((m, i) => ({
        ...m,
        revisedFrom: i === streamedAt && streamed && streamed.trim() !== String(m.text ?? '').trim() ? fromHeight : 0,
      }));
      // A page the answer opened whose live event came before this view had
      // the conversation ("open YouTube" out loud into a new chat: Yap opens
      // it at once): the panel shows it now.
      if (phase === 'answered' || landed) {
        const answer = messages.at(-1);
        if (answer?.role === 'assistant') for (const pg of answer.pages || []) if (pg?.url && !pagesShown.has(pg.url)) doc.showPage(pg);
      }
      // (Asked out loud in a Code chat that works directly, agents/direct.rs: it went straight to its
      // session — the chat talking, no answer of Chat's and no tab opened, as a typed message's.)
      refreshList();
      // The question asked out loud (the person's own), another conversation
      // brought up, the bar's Open in Chat: at its newest. Its answer
      // landing: followed only if the thread is (never a yank to the bottom
      // while you read further up).
      if (switched || phase === 'asked' || phase === 'open') scrollThread();
      else followThread();
      contextStamp += 1;
      // The bar's Open in Chat on an answer that made a document (or showed
      // a picture): it opens too.
      const last = phase === 'open' ? messages.at(-1) : null;
      const made = last?.documents?.at(-1);
      if (made) doc.show(made.id, { title: made.title });
      else if (last?.pictures?.some((p) => !p.chip)) doc.showPicture(last.pictures.filter((p) => !p.chip).at(-1));
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
  // (Code chats, codeChats.svelte.js: the list shows one side of Chat at a
  // time — `groupBy(conversations, byDate)` picks it.)
  let groupBy = $state(null);
  // A new chat's workspace for its first question (codeChats.svelte.js):
  // taken by the question sent, and (`pendingFolder`) the one shown.
  let newChatFolder = null;
  let pendingFolder = null;
  const groups = $derived(groupBy ? groupBy(conversations, byDate) : byDate(conversations));
  function byDate(conversations) {
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
  }
  const ordered = $derived(groups.flatMap((g) => g.items));
  // **A selection is only ever rows the list shows** (its Archive and Delete
  // act on it): a chat on the other side of Chat | Code, in a folded
  // workspace, archived or deleted from elsewhere drops out of it, so nothing
  // is deleted unseen. (ConversationList also lets it go while its search or
  // Archived shows in the list's place.)
  const shownPicked = $derived.by(() => {
    if (!picked.length) return picked;
    const shown = new Set(ordered.map((c) => c.id));
    return picked.filter((id) => shown.has(id));
  });
  // …for good: back on that side (or the workspace unfolded) it isn't picked again.
  $effect(() => {
    if (shownPicked.length !== picked.length) picked = shownPicked;
  });

  function clickConversation(e, c) {
    if (e.shiftKey) {
      const ids = ordered.map((x) => x.id);
      const from = anchor ?? activeId ?? c.id;
      const a = ids.indexOf(from);
      const b = ids.indexOf(c.id);
      const range = a < 0 ? [c.id] : ids.slice(Math.min(a, b), Math.max(a, b) + 1);
      picked = e.ctrlKey || e.metaKey ? [...new Set([...shownPicked, ...range])] : range;
      if (anchor == null) anchor = from;
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      // The open conversation counts as picked when a selection starts.
      const base = shownPicked.length ? shownPicked : activeId != null && activeId !== c.id ? [activeId] : [];
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

  // `all`: "Delete all chats…" (every conversation the list shows). Only rows
  // the list shows are ever deleted here: never a Code chat from the Chat
  // side (or a plain one from the Code side), a folded workspace's, an
  // archived one or a meeting's Ask thread (those two never come in
  // `conversations`: chats.rs `list`).
  function askDeleteMany(ids, { all = false } = {}) {
    const convs = ordered.filter((c) => ids.includes(c.id));
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

  /**
   * To the thread's newest, following it again (the person's own question,
   * a conversation opened). A thread not drawn yet starts there by itself.
   */
  function scrollThread() {
    follow?.toBottom();
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
    // Too long for the model on its own: the line under the box says so,
    // and nothing goes (chat/budget.rs checks again).
    if (inputTooLong) {
      shortenPaste();
      return;
    }
    tooLong = '';
    const voice = byVoice;
    // "/decisions last week" (or a one-tap prompt): a Skill, asked across
    // the meetings (chat/skills.rs).
    const skill = opts?.skill ?? skills.parse(text)?.skill?.command ?? null;
    if (skill && !messages.length && !scope) scope = 'meetings';
    input = '';
    // The passage it's about (the document panel), out of the box with it.
    const about = skill ? null : quote;
    quote = null;
    // Out of the box (Rust takes them with the question, chat_send).
    waitingShots.list = [];
    byVoice = false;
    // Think it through (chat/think.rs): this question only.
    const thinkIt = takeThink();
    // The next question: the last answer stops talking.
    stopReading();
    // This send's question and bubble carry its `turn`, and are found by
    // it: a spoken question's bubble may come and go in between (one a
    // newer question replaced), moving them.
    const turn = ++sendSeq;
    // (Words from a page beside the chat: quoted as a selection, its site named.)
    const sentQuote = about ? (about.page ? { noteId: 0, text: about.text, app: about.page.site || about.page.title || '' } : { noteId: about.noteId, text: about.text }) : null;
    const question = { role: 'user', text, voice, id: null, turn, images: shots, quote: sentQuote };
    messages = [...messages, question];
    // **The answer, in its own conversation** (`Answer`): a new chat's is
    // that new chat's until its question makes the conversation. Another
    // conversation opened while it's written (a click in the list, New
    // chat): the one shown is left as it is, and this one goes on being
    // written — shown again, where it is, when its conversation opens; it
    // lands there.
    const rec = new Answer({ conv: activeId, turn, view: viewSeq, question });
    setTurn(rec);
    scrollThread();
    // The conversation it went to, for the status dots: an ask-first card
    // waiting there is "needs you", even once Chat is left.
    let cardsIn = activeId;
    const onEvent = (ev) => {
      if (ev?.kind === 'turn' && ev.conversationId != null) cardsIn = ev.conversationId;
      if (ev?.kind === 'confirm') cardAsked(cardsIn, ev);
      if (!alive) return;
      if (ev?.kind === 'turn') {
        rec.askedId = ev.userMessageId ?? null;
        if (ev.conversationId != null && rec.conv !== ev.conversationId) {
          // A new conversation: open here while its new chat still is, and
          // in the list from the first question.
          const shownHere = isShown(rec);
          adopt(rec, ev.conversationId);
          if (shownHere && activeId == null) activeId = ev.conversationId;
          refreshList();
        }
        return;
      }
      const shown = isShown(rec);
      // The screen the model looked at (look_at_screen): with the question, as its screenshots are.
      if (ev?.kind === 'screen' && Array.isArray(ev.images) && ev.images.length) {
        lookedAt(rec, ev.images);
        return;
      }
      if (actingEvent(rec, ev, shown)) return;
      // The pictures, pages and buttons it showed: its pills, should the reply not list them.
      rec.collect(ev);
      // A document, a picture or a page of this turn (the panel), while this chat is the one shown.
      if (PANEL_EVENTS.has(ev?.kind) || ev?.kind === 'tool') {
        if (shown) {
          const at = bubbleAt(rec);
          if (docEvent(ev, at >= 0 ? messages[at] : null)) return;
        } else if (ev.kind !== 'tool') return;
      }
      // The answer so far and the line at the thread's foot: drawn while this chat is the one shown.
      rec.took(ev);
      paint(rec);
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
        skill,
        think: thinkIt || null,
        selection: about && !about.page ? { noteId: about.noteId, text: about.text, fromLine: about.fromLine ?? null, toLine: about.toLine ?? null } : null,
        // Words from a page beside the chat (chat/pages/view.rs `PageQuote`).
        pageQuote: about?.page ? { text: about.text, site: about.page.site || '' } : null,
        // A new chat made a Code chat before its first question (agents/code_chat.rs).
        codeFolder: activeId == null ? (newChatFolder?.() ?? null) : null,
        // A new chat's connectors switched off in its + menu (connectors/chats.rs).
        connectorsOff: activeId == null ? connectors.newChatOff() : null,
      },
      onEvent
    );
    rec.stop = call.stop;
    rec.supersede = call.supersede;
    try {
      const res = await call.promise;
      if (!alive) return;
      if (res.conversationId != null && rec.conv !== res.conversationId) {
        const shownHere = isShown(rec);
        adopt(rec, res.conversationId);
        if (shownHere && activeId == null) activeId = res.conversationId;
      }
      if (res.userMessageId) rec.askedId = res.userMessageId;
      if (!isShown(rec)) {
        // Saved in its own conversation (the list follows, its dot says
        // New); shown there when it opens.
        refreshList();
        if (res.speak && res.messageId) speakReply(res.spokenText || res.reply, res.messageId);
        return;
      }
      // A Code chat that works directly (agents/direct.rs): the message
      // went straight to its session — no answer of Chat's. The question
      // stays as saved (its id is what its session's work hangs under), and
      // its own words come as its report. No tab opens (14 Nov 2026): that's
      // the chat talking, as in Claude Code's own app, not an agent sent —
      // the panel shows the agents (helpers) its Claude sends.
      if (res.direct) {
        messages = messages
          .filter((m) => !(m.role === 'assistant' && m.streaming && m.turn === turn))
          .map((m) => (m.role === 'user' && m.turn === turn && m.id == null ? { ...m, id: res.userMessageId ?? rec.askedId ?? null } : m));
        refreshList();
        followThread();
        contextStamp += 1;
        return;
      }
      const qi = questionAt(rec);
      if (qi < 0) {
        // (Read back without its question: the conversation as saved.)
        dropTurn(rec);
        await reloadActive();
        refreshList();
        contextStamp += 1;
        return;
      }
      const asked = messages[qi];
      const ai = bubbleAt(rec);
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
        tokens: res.tokens ?? null,
        thinking: res.thinking ?? null,
        // The documents it made or changed (chat/documents.rs): their pills.
        documents: res.documents || [],
        // The pictures it showed beside the chat (chat/pictures.rs): their pills.
        pictures: res.pictures ?? [...rec.pictures, ...rec.shots],
        // The files it shared into the conversation (chat/shares.rs): under it.
        files: res.files ?? rec.shares,
        // "Search in your browser", when Yap's free web sources couldn't give it.
        browserSearch: res.browserSearch ?? null,
        // The pages it brought up (their tabs) and its buttons (chat/pages.rs).
        pages: res.pages ?? rec.pages,
        offers: res.offers ?? rec.offers,
        // The questions it asked before acting on a page (their cards, answered).
        confirms: (ai >= 0 ? messages[ai].confirms : null) || rec.confirms || null,
        revisedFrom: streamed != null && streamed.trim() !== '' && streamed.trim() !== String(res.reply).trim() ? fromHeight : 0,
      };
      // The question as saved: with its screenshots' text when it went as text.
      const saved = asText && res.question ? { text: res.question, images: shots.map((s) => ({ ...s, asText: true })) } : {};
      messages = [...messages.slice(0, qi), { ...asked, ...saved, id: res.userMessageId ?? null }, answer];
      refreshList();
      followThread();
      contextStamp += 1;
      // Read out by itself ("When to speak", decided in Rust): the voice
      // alone, no reading card (that's ▶'s), as much as "How much to read
      // aloud" says (Rust's spokenText).
      if (res.speak && answer.id) speakReply(res.spokenText || answer.text, answer.id);
    } catch (e) {
      if (!alive) return;
      // Its conversation shown now (another one: its messages and its box stay as they are).
      const shown = isShown(rec);
      if (String(e).includes(SUPERSEDED)) {
        // A newer question in this conversation replaced it (asked out
        // loud, or dictated here and sent): no answer was saved, its bubble
        // goes, the question stays as saved. No error: nothing went wrong.
        if (shown) {
          messages = messages
            .filter((m) => !(m.role === 'assistant' && m.streaming && m.turn === turn))
            .map((m) => (m.role === 'user' && m.turn === turn && m.id == null ? { ...m, id: rec.askedId } : m));
        }
        refreshList();
        contextStamp += 1;
        return;
      }
      const qi = questionAt(rec);
      if (rec.stopping && String(e).includes(STOPPED)) {
        // Stopped before any of the answer came: the question stays.
        if (shown && qi >= 0) messages = [...messages.slice(0, qi), { ...messages[qi], stopped: true }];
        refreshList();
        contextStamp += 1;
        return;
      }
      // Said so it's clear what to do (aiErrors.js); the raw error is in the log.
      // A sentence reads as a sentence; only an error Yap can't explain gets
      // the copyable error box.
      const why = aiError(e);
      if (!shown) {
        // Another chat's question: it stays there as asked (Rust keeps it),
        // and this chat's box is left alone.
        toast({
          title: "Couldn't answer in another chat",
          description: why.text,
          ...(why.known ? { variant: 'default', kind: 'error', chip: 'Error', icon: 'alert' } : { variant: 'destructive' }),
        });
        return;
      }
      if (qi >= 0) messages = messages.slice(0, qi);
      input = text; // give the message back
      quote = about; // …and the passage it was about
      if (thinkIt) think.on = true; // …thought through, as it was asked
      // …and its screenshots (Rust puts them back too, yap-chat-waiting).
      waitingShots.list = shots;
      byVoice = voice;
      // Too long for the model (Rust's plain sentence, never the provider's
      // JSON): said over the box with what helps — New chat, more room for
      // the on-device model, shortening the message.
      if (isTooLong(e)) {
        tooLong = String(e?.message ?? e);
        refreshLimits();
        return;
      }
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
      const openSettings = onopensettings();
      // On your Claude plan: the buttons that help (Sign in, Manage usage,
      // this chat on an Anthropic API key).
      const planHelp = await claudePlanHelp(e);
      toast({
        title: "Couldn't send",
        description: why.text,
        ...(why.known ? { variant: 'default', kind: 'error', chip: 'Error', icon: 'alert' } : { variant: 'destructive' }),
        ...(planHelp
          ? planHelp
          : why.settings && openSettings
            ? { action: { label: 'Open Language Models', onClick: () => openSettings('cleanup#chat') } }
            : {}),
      });
    } finally {
      // Nothing is being written into the document any more (its
      // conversation's panel: another chat's may still be writing).
      if (isShown(rec)) doc.turnEnded();
      // (No card of its waits any more.)
      turnOver(cardsIn);
      dropTurn(rec);
      if (alive && sendWhenFree === rec) {
        sendWhenFree = null;
        send();
      }
    }
  }

  /**
   * **What helps when your Claude plan couldn't answer** (aiErrors.js
   * `claudePlanProblem`), as the toast's buttons: Claude Code not signed in
   * here → Sign in to Claude Code (its own sign-in, in a terminal); the
   * plan's limit → Manage usage, and — an Anthropic key saved — Use my API
   * key for this chat (Chat on the key, the same model, the question sent
   * again). `null`: none of those.
   */
  async function claudePlanHelp(e) {
    const kind = claudePlanProblem(e);
    if (kind === 'signIn') {
      return { action: { label: 'Sign in to Claude Code', onClick: () => signInClaudePlan() } };
    }
    if (kind !== 'limit') return null;
    const usage = { label: 'Manage usage', onClick: () => openExternalLink(CLAUDE_USAGE_URL) };
    const st = await invoke('ai_model_status', { scope: 'chat' }).catch(() => null);
    const id = st?.model?.provider === 'claude-plan' ? st.model.id : '';
    if (!id || !st?.keyed?.includes('anthropic')) return { action: usage };
    const onKey = async () => {
      try {
        await invoke('chat_set_model', { provider: 'anthropic', model: id, baseUrl: null, file: null });
        if (alive && !sending && input.trim()) send();
      } catch (err) {
        toast({ title: "Couldn't switch the model", description: String(err), variant: 'destructive' });
      }
    };
    return { action: { label: 'Use my API key for this chat', onClick: onKey }, secondary: usage };
  }

  /**
   * ■ (and Esc): stop **the open conversation's** answer being written —
   * never another chat's (what came stays, checked).
   */
  function stopAnswer() {
    const rec = here;
    if (rec && !rec.spoken) {
      stopTurn(rec);
      return;
    }
    if (rec?.spoken || assistantAnswering) stopSpoken(rec?.conv ?? activeId);
  }

  /** `rec`'s ■: a typed question's by its own stream; one asked out loud as `stopSpoken`. */
  function stopTurn(rec) {
    if (!rec) return;
    if (rec.spoken) {
      stopSpoken(rec.conv);
      return;
    }
    if (rec.stopping || !rec.stop) return;
    rec.stopping = true;
    rec.stop();
  }

  /**
   * ■ on an answer asked out loud in conversation `conv`: the Voice
   * Assistant's own question (its voice and all, `assistant_stop`) when it's
   * that conversation's; else — one it has moved on from, a phone's — the
   * turn answering there (`stream_stop_conversation`).
   */
  async function stopSpoken(conv) {
    if (conv == null) return;
    const now = await readAssistant().catch(() => null);
    const its = (now?.phase === 'thinking' || now?.phase === 'speaking') && now?.conversationId === conv;
    if (its) invoke('assistant_stop').catch(() => {});
    else invoke('stream_stop_conversation', { conversation: conv }).catch(() => {});
  }

  /**
   * A newer question is waiting to go (`sendWhenFree`): the open
   * conversation's answer still coming is dropped, nothing saved for it, so
   * the newer one goes now — not after an answer to a question you've moved
   * on from.
   */
  function replaceAnswer() {
    const rec = here;
    if (!rec || rec.spoken || rec.stopping || !rec.supersede) return;
    rec.supersede();
  }

  /**
   * Keep the newest text in view while it's written — unless you scrolled up
   * to read. (The thread's ResizeObserver follows every change by itself:
   * core/stickToBottom.js. This only nudges it.)
   */
  function followThread() {
    follow?.stick();
  }

  // Rust decides whether a dictation is for the chat: the box with the
  // focus is one way (the hotkey in Yap's window, the bar's Dictate).
  function reportFocus(focused) {
    invoke('chat_input_focus', { focused }).catch(() => {});
  }

  /** Leaving Chat: nothing answers into this view any more (what's typed in the document is saved). */
  function close() {
    alive = false;
    doc.flushSave();
    // The quote over the box goes with the view.
    reportQuote(null);
    // The live pages beside the chat close (their tabs open them again).
    doc.closePages();
  }

  return {
    get doc() { return doc; },
    get connectors() { return connectors; },
    get conversations() { return conversations; },
    set conversations(v) { conversations = v; },
    get activeId() { return activeId; },
    get messages() { return messages; },
    get input() { return input; },
    set input(v) { input = v; },
    get quote() { return quote; },
    setQuote,
    clearQuote,
    onQuote,
    onPageQuote,
    get sending() { return sending; },
    // The open conversation has an answer being written (typed or asked out
    // loud): the line at the thread's foot shows (never another chat's).
    get answeringHere() { return here != null; },
    get assistantAnswering() { return assistantAnswering; },
    agentShown,
    // `yap-assistant`'s payload, or `assistant_phase`'s.
    set assistantPhase(v) {
      phaseHeard += 1;
      setAssistant(v);
    },
    refreshAssistant,
    get stopping() { return stopping; },
    get streaming() { return streaming; },
    get inputEl() { return inputEl; },
    set inputEl(v) { inputEl = v; },
    get threadEl() { return threadEl; },
    set threadEl(v) { threadEl = v; },
    get threadGutter() { return threadGutter; },
    set threadGutter(v) { threadGutter = v; },
    get follow() { return follow; },
    set follow(v) { follow = v; },
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
    get activity() { return activity; },
    get activityWaiting() { return activityWaiting; },
    get modelHere() { return modelHere; },
    get contextStamp() { return contextStamp; },
    get limits() { return limits; },
    get summary() { return summary; },
    get showingSummary() { return showingSummary; },
    set showingSummary(v) { showingSummary = v; },
    get tooLong() { return tooLong; },
    set tooLong(v) { tooLong = v; },
    get inputTokens() { return inputTokens; },
    get inputTooLong() { return inputTooLong; },
    refreshLimits,
    onSummary,
    giveMoreRoom,
    shortenPaste,
    get picked() { return shownPicked; },
    set picked(v) { picked = v; },
    get groups() { return groups; },
    get ordered() { return ordered; },
    // Code chats (codeChats.svelte.js): the list's side, and a new chat's workspace.
    set groupBy(fn) { groupBy = fn; },
    set newChatFolder(fn) { newChatFolder = fn; },
    set pendingFolder(fn) { pendingFolder = fn; },
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
    reloadActive,
    newChat,
    askAcrossMeetings,
    skills,
    skillKey,
    pickSkill,
    runPrompt,
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
