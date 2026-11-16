<script>
  // Chat's thread: the conversation's messages (ChatMessage), and at its
  // foot while an answer comes what the turn is doing now and for how long
  // (ActivityLine: "Searching the web · … · 1m 59s"; a model on this PC slow
  // to start says why), or — no messages yet — the start screen: Yap's mark
  // and a greeting by the time of day (core/greeting.js), the box right under
  // it in the middle (ChatView), and what Chat needs if no AI model is set up.
  // An empty Code chat greets you as the Code side instead (CodeEmpty.svelte).
  //
  // It keeps its newest line in view while it grows (core/stickToBottom.js,
  // 2026-11-07): pinned at the bottom while you're there — streamed text,
  // a card, a pill, a picture loading, the report of an agent read in, the
  // panel opening beside it —, left where you put it once you scroll up,
  // with "↓" (and "New messages" when something came meanwhile) to go back.
  import { untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import { motionMs } from '../../core/appearance.js';
  import { followBottom } from '../../core/stickToBottom.js';
  import { messageItems } from '../../core/itemMenus.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { STILL_WORKING_S } from './chatState.svelte.js';
  import { SUMMARY_DIVIDER } from './chatBudget.js';
  import { greetingFor } from '../../core/greeting.js';
  import yapMark from '../../../assets/yap-logo.svg';
  import ChatMessage from './ChatMessage.svelte';
  import ActivityLine from './ActivityLine.svelte';
  import CodeEmpty from './CodeEmpty.svelte';
  // Yap Code (desktop/src-tauri/src/agents.rs): the pills of the agents an answer
  // started, an agent's report as an agent card (`kind: "agentReport"`), in a
  // Code chat each turn's to-do card under its pill (AgentTodoCard), and a
  // running agent's permission prompts for this chat — each in a small chunk.
  import { agents, agentById } from '../../stores/agents.svelte.js';
  import { pillsByAnswer, pillsByQuestion, answeredAfter, directLine, plainReport, agentOfReport, reportFolder, messageTurn, notesAfter, turnsOf, turnParts, cardPart, todoAtFoot } from '../../core/agents.js';
  import { agentLogs } from '../../stores/agentLogs.svelte.js';
  import { threadViewOf } from './threadView.svelte.js';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { chatStatus } from '../../stores/chatStatus.svelte.js';
  import QueuedMessage from './QueuedMessage.svelte';
  // A long chat's chapters (chat/cards/chapters.rs): a divider above the question each starts at (one pinned under
  // an answer: under its question, above the answer's turn), and the floating contents (ChatChapters, its own chunk)
  // to jump between them.
  import { chaptersOf } from './chapters.js';
  import { chippedElsewhere } from './fileChip.js';
  // What the chat made (chat/made.rs): its visuals and its kept pages' cards, each in the answer to its question.
  import { made, followMade, onMade, placeMade, promptFromVisual } from './made.svelte.js';
  // The rows' times ("12 minutes ago") move on while the thread is shown.
  import { followClock } from './clock.svelte.js';

  $effect(() => followClock());
  const agentPills = $derived(pillsByAnswer(agents.list, chat.activeId, chat.messages));
  const chapters = $derived(chaptersOf(chat.messages));
  const chapterAt = $derived(new Map(chapters.map((c) => [`${c.at}:${c.below ? 'below' : 'above'}`, c])));
  // **Visuals** (chat/made.rs, 15 Nov 2026): read with the chat, heard as they're made; each shows in the
  // answer to its question (above its words), or under the question's work while there's no answer yet.
  followMade();
  $effect(() => {
    const id = chat.activeId;
    if (id != null) untrack(() => made.load(id));
  });
  const placed = $derived(placeMade(chat.messages, made.of(chat.activeId), chat.forks));
  /** A visual's button sent a message (`sendPrompt`): into this chat (made.svelte.js `promptFromVisual`). */
  const sendFromVisual = (text, v, conversation) => promptFromVisual(chat, text, v, conversation);
  /** A kept page's card: the page beside the chat, at that version (chat/made/pages.rs). */
  const openKept = (page, n) => chat.doc.showKept({ id: page.id, conversation: chat.activeId, title: page.title, n });
  // **A page saved in this chat as its answer comes** (save_page): it opens beside the chat, as a
  // document a turn writes does.
  $effect(() =>
    onMade((conversation, m) => {
      if (m?.kind === 'page' && conversation === chat.activeId) chat.doc.showKept({ id: m.id, conversation, title: m.title, n: 0 });
    })
  );
  // …and under the person's own message, in a Code chat that works directly
  // (agents/direct.rs): its session's work, with no answer of Chat's between.
  const directPills = $derived(pillsByQuestion(agents.list, chat.activeId, chat.messages));
  // **A plain reply is a plain reply** (12 Nov 2026): a direct turn that ended with nothing to report but its
  // words has no line of its own and no "Agent done" line — its answer is all there is, as in Claude Code's app.
  /** A report's marker with nothing to say (its turn a plain reply). */
  const quietReport = (m) => plainReport(m, agentOfReport(agents.list, m.id, chat.activeId, reportFolder(m.text)));
  // **One chip per file per turn** (16 Nov 2026): a report's file the turn's answer shares is its name in the
  // report's line, its chip under the answer (fileChip.js).
  const chippedAway = $derived(chippedElsewhere(chat.messages));
  const asks = $derived(chat.activeId == null ? [] : agents.approvals.filter((a) => a.conversation === chat.activeId));
  // Answered from a paired phone (Remote Control): "Allowed from Office phone".
  // **The plans a turn sent back with comments** (Plan mode's review, PlanHistory.svelte): its `plansBefore`, and
  // whether the new plan is still being written (the turn at work, no plan card of its agent waiting).
  const plansOf = (p) => (p?.turnList || []).find((t) => t.n === (p.turn ?? 1))?.plansBefore || [];
  const revisingNow = (p) => ['running', 'asking'].includes(p?.turnStatus) && !asks.some((a) => a.run === p.id && a.tool === 'ExitPlanMode');
  const answeredRemote = $derived(chat.activeId == null ? [] : agents.remoteAnswers.filter((a) => a.conversation === chat.activeId));
  const openAgent = (j) => chat.doc.showAgent(j);
  // A picture shared into the conversation (chat/shares.rs): in the panel's
  // picture tab while the panel's open, else the viewer.
  const openShared = (p) => (chat.doc.open ? chat.doc.showPicture(p) : (chat.viewing = { id: p.id, w: p.w, h: p.h }));
  const openRun = (run) => {
    const j = agentById(run);
    if (j) chat.doc.showAgent(j);
  };
  /** A pill's turn has a to-do list (its card under the pill, AgentTodoCard). */
  const hasTodos = (p) => ((p.turnList || []).find((t) => t.n === (p.turn ?? 1)) ?? (p.turnList?.length ? null : p))?.todos?.length > 0;
  // **A direct turn in order with the person's messages** (15 Nov 2026, as Claude Code's app lays a turn out): a
  // message they sent while it worked shows where Claude read it, and the work after it under it (AgentWork's parts).
  /** Each direct turn's messages it may have read (`notesAfter`), by the index of the message that asked. */
  const turnNotes = $derived(new Map([...directPills.keys()].map((i) => [i, notesAfter(chat.messages, i, (k) => directPills.has(k))])));
  /** A message it may have read → the turns whose next part may go under it. */
  const partsUnder = $derived.by(() => {
    const out = new Map();
    for (const [i, notes] of turnNotes) for (const id of notes) out.set(id, [...(out.get(id) || []), ...directPills.get(i).map((p) => ({ p, i, notes }))]);
    return out;
  });
  // **The live to-do list at the thread's foot** (16 Nov 2026, as the Claude app keeps its plan): while a direct
  // turn works, its list is the last thing in the thread, under the latest message, words or work line and just above
  // the message box, moving down as the thread grows (part of the thread, never floating over it). Once the turn
  // ends it's the record at the end of its work (AgentWork); a message read mid-turn keeps the plan as it stood
  // above it ("Plan from before your last message"). (An agent Chat sent out keeps its card under its pill.)
  const footTodos = $derived([...directPills].flatMap(([i, ps]) => ps.filter((p) => todoAtFoot(p, agentLogs.by[p.id] || [], turnNotes.get(i) || []))));
  /** How Claude's work shows in this chat (its ⋯ → Transcript view). */
  const view = $derived(threadViewOf(chat.activeId));
  // **An ask-first card goes with the work it's about** (15 Nov 2026, as the Claude app lays a turn out): under
  // the part of the turn it asked in — the latest when it came (`cardPart`), not always the message that asked.
  /** Turn `p`'s cards before acting on a page that go under its part after message `after` (null: the first). */
  function cardsIn(p, notes, after) {
    const cards = chat.agentConfirms[`${p.id}:${p.turn ?? 1}`];
    if (!cards?.length) return [];
    const events = turnsOf(p, agentLogs.by[p.id] || []).find((t) => t.n === (p.turn ?? 1))?.events || [];
    const parts = turnParts(events, notes);
    return cards.filter((c) => cardPart(parts, c.after) === after);
  }
  /** A step's picture, large: paged across its turn's (‹ 2 of 4 ›). */
  const openPaged = (list, at) => (chat.viewing = { ...list[at], list, at });

  // **The start screen's greeting** (core/greeting.js): "Afternoon, Sam" by the local time, About you's name, the
  // band followed while it shows. The e2e suite and Yap Lab put their own time in the clock's place
  // (`window.__yapTestClock`, ms, then a `yap-test-clock` event; null: the real time again), to shoot a late night.
  let clockNow = $state(Date.now());
  let testClock = $state(typeof window !== 'undefined' && Number.isFinite(window.__yapTestClock) ? window.__yapTestClock : null);
  $effect(() => {
    const onTest = () => (testClock = Number.isFinite(window.__yapTestClock) ? window.__yapTestClock : null);
    window.addEventListener('yap-test-clock', onTest);
    return () => window.removeEventListener('yap-test-clock', onTest);
  });
  $effect(() => {
    if (chat.messages.length) return;
    clockNow = Date.now();
    const t = setInterval(() => (clockNow = Date.now()), 60_000);
    return () => clearInterval(t);
  });
  const hello = $derived(greetingFor(chat.aboutName, new Date(testClock ?? clockNow)));

  // The answer's words have begun (a slow start's hint goes).
  const answering = $derived(chat.messages.some((m) => m.streaming && m.text));
  // Another chat is answering too (Rust's list, stores/chatStatus.svelte.js):
  // a model on this PC shares itself between them, so a slow start says so.
  const othersAnswering = $derived(chatStatus.answering.some((id) => id !== String(chat.activeId)));
  const slowHint = $derived(
    othersAnswering
      ? 'Still working: the model runs on this PC and is answering another chat too, so this can take a minute.'
      : 'Still working: the model runs on this PC, so this can take a minute.'
  );

  // Show summary's button, the focus's way back when its dialog closes.
  let summaryBtn = $state(null);
  function closeSummary() {
    chat.showingSummary = false;
    summaryBtn?.focus({ preventScroll: true });
  }

  /**
   * `chat`: the view's state; `code`: its Code chats (codeChats.svelte.js);
   * `onopensettings(section)`: ControlPanel's Settings.
   */
  let { chat, code = null, onopensettings = null } = $props();

  // The width a question's screenshots may take: a bubble's (78 % of the
  // messages' column). Settled a moment after a resize, so a window being
  // dragged wider doesn't ask Rust for a thumbnail at every pixel (they're
  // drawn at exactly their device pixels: chatImages.js `sentPlaced`). The
  // column's content box, not its clientWidth: Svelte reads clientWidth
  // when the border box resizes, which a scrollbar appearing never does.
  let msgsBox = $state(null);
  let colBox = $state(null);
  const colWidth = $derived(colBox?.width ?? 0);
  let room = $state(0);
  // **The readable column** (2026-11-07, as the Claude app's and ChatGPT's
  // 48rem): the messages at most `--yap-thread-width` wide, centred in a
  // wide thread, the composer on the same column (Composer.svelte); the
  // scroller stays the thread's whole width, so its scrollbar sits at the
  // thread's edge and the composer's column is moved by half its width.
  $effect(() => {
    msgsBox;
    const el = chat.threadEl;
    chat.threadGutter = el ? Math.max(0, el.offsetWidth - el.clientWidth) : 0;
  });
  $effect(() => {
    const next = colWidth > 0 ? Math.floor(colWidth * 0.78) : 0;
    if (!untrack(() => room) || !next) {
      room = next;
      return;
    }
    const t = setTimeout(() => (room = next), 250);
    return () => clearTimeout(t);
  });

  // ---- Following the bottom (core/stickToBottom.js)
  // `pinned`: at the bottom, following it. `unseen`: something came (a
  // message, more of the answer, an agent's card) while you'd scrolled up.
  let contentEl = $state(null);
  let pinned = $state(true);
  let unseen = $state(false);
  $effect(() => {
    const el = chat.threadEl;
    const inner = contentEl;
    if (!el || !inner) return;
    const follow = untrack(() => {
      pinned = true;
      unseen = false;
      return followBottom(el, inner, {
        onchange: (s) => {
          pinned = s.pinned;
          if (s.pinned) unseen = false;
        },
      });
    });
    // chatState's own moves: a question sent, a conversation opened.
    chat.follow = follow;
    return () => {
      follow.destroy();
      if (chat.follow === follow) chat.follow = null;
    };
  });
  // What's new at the thread's end: a message, the answer's words, an agent's prompt.
  const tail = $derived(`${chat.messages.length}:${chat.messages.at(-1)?.text?.length ?? 0}:${asks.length}`);
  let tailSeen = '';
  $effect(() => {
    const t = tail;
    untrack(() => {
      if (tailSeen && t !== tailSeen && !pinned) unseen = true;
      tailSeen = t;
    });
  });
  /** "↓": back to the newest, following it again (the focus to the box, where the next words go). */
  function toLatest() {
    chat.follow?.toBottom({ smooth: true });
    unseen = false;
    chat.inputEl?.focus({ preventScroll: true });
  }

  function messageMenu(e, m) {
    const reading = !!m.id && chat.speakingId === m.id;
    const items = messageItems(m, { read: m.role === 'assistant' && m.id ? () => chat.readAloud(m) : null, reading });
    // The person's message: Edit, or in a Code chat Rewind to here and Fork from here (as under it); an answer: Fork
    // from here and Pin as chapter (as under it).
    const a = m.role === 'assistant' ? null : actionsFor(m);
    const r = m.role === 'assistant' ? answerFor(m, chat.messages.indexOf(m)) : null;
    if (a?.onedit || a?.onrewind || r?.onfork || r?.onpin) items.push({ separator: true });
    if (a?.onedit) items.push({ label: a.edit ? `Edit (${a.edit.toLowerCase()})` : 'Edit', icon: 'pencil', disabled: !!a.edit, onselect: a.onedit });
    if (a?.onrewind) items.push({ label: a.rewind ? `Rewind to here (${a.rewind.toLowerCase()})` : 'Rewind to here', icon: 'undo', disabled: !!a.rewind, onselect: a.onrewind });
    const fork = a?.onfork ? a : r?.onfork ? r : null;
    if (fork) items.push({ label: fork.fork ? `Fork from here (${fork.fork.toLowerCase()})` : 'Fork from here', icon: 'split', disabled: !!fork.fork, onselect: fork.onfork });
    if (r?.onpin) items.push({ label: r.pinned ? 'Unpin chapter' : 'Pin as chapter', icon: 'pin', disabled: !!r.pinning, onselect: r.onpin });
    openContextMenu(e, items, { label: m.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget });
  }

  // **Under the person's message** (the Claude app's rows, 16 Nov 2026): in Chat, Copy and Edit (a new version from
  // there, ‹ 1/2 › on it); in a Code chat — one that started a turn of Claude's: agents/rewind.rs —, ✓✓ and its time,
  // Copy, Rewind to here (back to before it: asked first, the files Claude changed since put back as they were unless
  // unticked; its words back in the box) and Fork from here (a new chat with the conversation up to its answer).
  function actionsFor(m) {
    if (m?.role !== 'user' || m.kind || chat.activeId == null) return null;
    // Still being sent (no id until its answer is in): the row is there already, Copy in it and the rest waiting —
    // drawn only once the answer was in, it pushed the thread 36 px down at every answer's end (15 Nov 2026,
    // chat-scroll.spec.js; the browser's scroll anchoring hid it but moved the scrollbar).
    if (!m.id) return code?.current ? { code: true } : { edit: busyWhy || 'Not saved yet', onedit: () => {} };
    if (!code?.current) return { edit: busyWhy, onedit: () => chat.startEdit(m) };
    const t = messageTurn(agents.list, chat.activeId, m.id);
    // A message no turn of Claude's started (yet): Copy, Rewind and Fork once its turn is known — the same row.
    if (!t) return { code: true };
    return { code: true, rewind: t.rewind, fork: t.fork, onrewind: () => askRewind(m, t.files), onfork: () => chat.forkAt(m) };
  }

  // **Under an answer** (16 Nov 2026, the Claude app's row; 👍 / 👎 and ↻ Retry taken out): Fork from here — a new
  // chat up to and including it (in a Code chat Claude's own fork of its session, from the message it answers) — and
  // Pin as chapter (its title made as the model's chapters are: chat/cards/chapters.rs `pin_title`). Copy and Read
  // aloud are the message's own. Never an agent's report, nor an answer still being written.
  const pins = $state({});
  /** The row's actions under answer `m` (index `i`), or null. */
  function answerFor(m, i) {
    if (m?.role !== 'assistant' || m.kind || m.streaming || chat.activeId == null) return null;
    const pinned = !!m.chapter?.title;
    const pin = m.id ? { pinned, pinning: !!pins[m.id], onpin: () => pinAnswer(m) } : {};
    if (!code?.current) return { ...pin, fork: m.id ? '' : 'Not saved yet', onfork: () => chat.forkAnswer(m) };
    // A Code chat: Fork from the message this answer answers (its turn), as Fork from here under that message.
    const qi = chat.messages.findLastIndex((x, k) => k < i && x.role === 'user' && !x.kind);
    const q = chat.messages[qi];
    const t = q?.id ? messageTurn(agents.list, chat.activeId, q.id) : null;
    return { ...pin, code: true, ...(t ? { fork: t.fork, onfork: () => chat.forkAt(q) } : {}) };
  }
  async function pinAnswer(m) {
    if (!m?.id || pins[m.id]) return;
    pins[m.id] = true;
    try {
      await chat.pinChapter(m, !m.chapter?.title);
    } finally {
      delete pins[m.id];
    }
  }

  // **Edit and ‹ ›** (15 Nov 2026, the Claude app's message actions; chat/chats/branches.rs): the pencil under the
  // person's message opens it in place to change; what was there is kept as a version, ‹ 1/2 › to switch (an older
  // Retry's on its answer too). Not while an answer is being written.
  const busyWhy = $derived(chat.sending || chat.answeringHere ? 'Unavailable while an answer is being written' : '');
  /** The ‹ k/n › of the message at `i`, or null. */
  function branchOf(i) {
    const fork = chat.forkOf(i);
    return fork ? { fork, why: busyWhy, onbranch: (place) => chat.switchBranch(i, place) } : null;
  }

  let rewindOpen = $state(false);
  let rewindAsk = $state(null);
  let rewindFiles = $state(true);
  let rewindBusy = $state(false);
  function askRewind(m, files) {
    rewindAsk = { m, files };
    rewindFiles = true;
    rewindOpen = true;
  }
  async function doRewind() {
    if (!rewindAsk) return;
    const { m, files } = rewindAsk;
    rewindBusy = true;
    const ok = await chat.rewindAt(m, files > 0 && rewindFiles);
    rewindBusy = false;
    rewindOpen = false;
    if (ok) chat.inputEl?.focus({ preventScroll: true });
  }
</script>

{#if !code?.onCode && (chat.projectHere || (chat.activeId == null && chat.newProject))}
  <!-- The project this chat is in (chat/projects.rs): its page a click away. -->
  {@const inProject = chat.projectHere || chat.newProject}
  <p class="scopeline projline" data-testid="chat-project-line" style:--thread-gutter="{chat.threadGutter}px">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v6.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z" /></svg>
    {chat.activeId == null ? 'New chat in' : 'In'}
    <button type="button" class="projlink" onclick={() => chat.openProject(inProject.id)}>{inProject.name}</button>
  </p>
{/if}
{#if chat.messages.length === 0 && (code?.current || code?.onCode)}
  <!-- An empty Code chat: the Code side's own greeting. -->
  <CodeEmpty {chat} {code} {onopensettings} />
{:else if chat.messages.length === 0}
  <!-- **The start screen** (16 Nov 2026, after the Claude app's new-chat screen): Yap's mark and a greeting by the
       time of day in the display serif, centred, the message box right under it in the middle of the thread
       (ChatView's `.thread.start`); the first message sends the box down to its place at the foot. A new chat with
       its ghost on (chat/incognito.rs) says so in the greeting's place. -->
  <div class="hello chatstart" data-testid="chat-start" data-band={chat.incognito ? 'incognito' : hello.band}>
    <h1 class="greet">
      {#if chat.incognito}
        <svg class="ghostmark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 20V11a7 7 0 0 1 14 0v9l-2.33-1.5L14.33 20 12 18.5 9.67 20l-2.34-1.5z" /><circle cx="9.5" cy="11" r="1" fill="currentColor" stroke="none" /><circle cx="14.5" cy="11" r="1" fill="currentColor" stroke="none" /></svg>
        <span class="greettext" data-testid="chat-greeting">Incognito chat</span>
      {:else}
        <img class="mark" src={yapMark} alt="" aria-hidden="true" draggable="false" />
        <span class="greettext" class:waiting={chat.aboutName == null} data-testid="chat-greeting">{hello.text}</span>
      {/if}
    </h1>
    {#if chat.incognito}
      <p class="hellonote" data-testid="chat-incognito-empty">It isn't saved to your chats, and memory isn't used or updated. It's gone when you leave it.</p>
    {:else if chat.scope === 'meetings'}
      <!-- Asked across the meetings (search/ask.rs; the Meetings view's Ask bar, a Skill): found on this PC, cited. -->
      <p class="hellonote" data-testid="chat-start-meetings">Asking across your meetings and notes: answers cite the moment they come from.</p>
    {/if}
    {#if !chat.chatReady}
      <!-- Before a first question fails: what Chat needs. -->
      <p class="hellonote needsai">
        Chat needs an AI model: a free key from a cloud provider, or one that runs on this PC.
        <button class="setkey" onclick={() => onopensettings?.('cleanup#chat')}>Set up an AI model</button>
      </p>
    {/if}
  </div>
{:else}
  {#if chat.incognito}
    <!-- An incognito chat says so over its thread (chat/incognito.rs). -->
    <p class="scopeline ghostline" data-testid="chat-incognito-line" style:--thread-gutter="{chat.threadGutter}px">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 20V11a7 7 0 0 1 14 0v9l-2.33-1.5L14.33 20 12 18.5 9.67 20l-2.34-1.5z" /></svg>
      Incognito chat · not saved, not remembered · gone when you leave it
    </p>
  {/if}
  {#if chat.scope === 'meetings'}
    <!-- This conversation asks across the meetings, follow-ups too. -->
    <p class="scopeline" data-testid="chat-across-meetings" style:--thread-gutter="{chat.threadGutter}px">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
      Asking across your meetings and notes
    </p>
  {/if}
  <div class="threadbox" style:--thread-gutter="{chat.threadGutter}px">
    <!-- The thread scrolls (.msgs); what's in it (.msgsin, the readable
         column) is what grows. -->
    <div class="msgs" bind:this={chat.threadEl} bind:contentRect={msgsBox}>
      <div class="msgsin" bind:this={contentEl} bind:contentRect={colBox} data-testid="chat-column">
        {#each chat.messages as m, i (i)}
          {#if chat.summary && i === chat.summary.upto}
            <!-- A long chat's older messages, summarised (chat/compact.rs): the
                 model gets the summary in their place, the latest whole. -->
            <div class="summarised" role="separator" aria-label={SUMMARY_DIVIDER} data-testid="chat-summary-divider">
              <span class="rule"></span>
              <span class="stext">{SUMMARY_DIVIDER} ·</span>
              <button type="button" class="showsum" bind:this={summaryBtn} onclick={() => (chat.showingSummary = true)}>Show summary</button>
              <span class="rule"></span>
            </div>
          {/if}
          {#if chapterAt.has(`${i}:above`)}
            <!-- A new chapter starts at this question (mark_chapter), or at this answer pinned with no question of the
                 person's before it: its title, as a divider. -->
            {@render chapterDivider(chapterAt.get(`${i}:above`))}
          {/if}
          {#if m.kind === 'agentReport'}
            {#if !quietReport(m)}
              {#await import('./AgentReportCard.svelte') then { default: AgentReportCard }}
                <AgentReportCard {m} conversation={chat.activeId} onopen={openAgent} {room} onpicture={openShared} quiet={!!code?.current} elsewhere={chippedAway.get(i) ?? null} />
              {/await}
            {/if}
          {:else}
            <ChatMessage
              {m}
              tick={m.role === 'user' ? chat.tickOf(m.id) : null}
              last={i === chat.messages.length - 1}
              sources={chat.citedSources(i)}
              reading={m.role === 'assistant' && !!m.id && chat.speakingId === m.id}
              keepDays={chat.keepDays}
              {room}
              onview={(img) => (chat.viewing = img)}
              onmenu={(e) => messageMenu(e, m)}
              onread={() => chat.readAloud(m)}
              onsource={(s) => chat.openSource(s)}
              ondoc={(d) => chat.doc.show(d.id, { title: d.title })}
              onpicture={(p) => chat.doc.showPicture(p)}
              onpage={(p) => chat.doc.showPage(p)}
              pagethumb={chat.pageThumb}
              conversation={chat.activeId}
              onshared={openShared}
              actions={m.role === 'user' ? actionsFor(m) : null}
              onnext={(b) => chat.sendNextStep(b)}
              busy={chat.sending}
              visuals={placed.get(i)?.inAnswer ? placed.get(i).visuals : null}
              keptPages={placed.get(i)?.inAnswer ? placed.get(i).pages : null}
              onvisualprompt={(text, v) => sendFromVisual(text, v, chat.activeId)}
              onkept={openKept}
              branch={m.id && !m.streaming ? branchOf(i) : null}
              answer={m.role === 'assistant' ? answerFor(m, i) : null}
              editing={m.role === 'user' && chat.editing?.id === m.id ? chat.editing : null}
              onedited={(words) => chat.saveEdit(m, words)}
              oneditcancel={() => chat.cancelEdit()}
            />
          {/if}
          {#if chapterAt.has(`${i}:below`)}
            <!-- An answer to this question pinned as a chapter (Pin as chapter): its title above the answer's turn, as
                 the Claude app draws it — under the question, before the work and the answer. -->
            {@render chapterDivider(chapterAt.get(`${i}:below`))}
          {/if}
          {#if directPills.get(i)?.length}
            <!-- A Code chat that works directly: what its session says and does, under the message that asked
                 (the chat's own work, as Claude Code's transcript shows its own). -->
            <!-- (Its own line only once it ended with something to report: while it works its work says so, and a plain reply has none.) -->
            {@const lines = directPills.get(i).filter(directLine)}
            {#if lines.length}
              {#await import('./AgentPills.svelte') then { default: AgentPills }}
                <AgentPills agents={lines} onopen={openAgent} quiet />
              {/await}
            {/if}
            <!-- (Its to-do list: at the thread's foot while it works, below; then AgentWork's, at the end of its work —
                 and as it stood, above a message it read.) -->
            {#each directPills.get(i) as p (`work:${p.id}:${p.turn ?? 1}`)}
              {@const here = cardsIn(p, turnNotes.get(i) || [], null)}
              {#await import('./AgentWork.svelte') then { default: AgentWork }}
                <AgentWork
                  agent={p}
                  turn={p.turn ?? 1}
                  line={agents.steps[p.id] || ''}
                  onview={openShared}
                  onpaged={openPaged}
                  answered={answeredAfter(chat.messages, i, (k) => directPills.has(k))}
                  notes={turnNotes.get(i) || []}
                  {view}
                  cards
                />
              {/await}
              {#if plansOf(p).length}
                <!-- Its plans sent back with comments (Plan mode's review), folded above the one that followed. -->
                {#await import('./PlanHistory.svelte') then { default: PlanHistory }}
                  <PlanHistory plans={plansOf(p)} revising={revisingNow(p)} />
                {/await}
              {/if}
              <!-- Its cards before acting on a page (an action that would buy, send or sign in): Allow once / Don't —
                   here while this part is where the turn was when it asked. -->
              {#if here.length}
                {#await import('./PageConfirm.svelte') then { default: PageConfirm }}
                  <PageConfirm confirms={here} />
                {/await}
              {/if}
            {/each}
          {/if}
          {#if m.role === 'user' && m.id && partsUnder.get(m.id)?.length}
            <!-- Sent while a turn worked: what Claude did after reading it, under it (nothing until it's read). -->
            {#each partsUnder.get(m.id) as o (`work:${o.p.id}:${o.p.turn ?? 1}:${m.id}`)}
              {@const here = cardsIn(o.p, o.notes, m.id)}
              {#await import('./AgentWork.svelte') then { default: AgentWork }}
                <AgentWork
                  agent={o.p}
                  turn={o.p.turn ?? 1}
                  line={agents.steps[o.p.id] || ''}
                  onview={openShared}
                  onpaged={openPaged}
                  answered={answeredAfter(chat.messages, o.i, (k) => directPills.has(k))}
                  notes={o.notes}
                  after={m.id}
                  {view}
                  cards
                />
              {/await}
              <!-- Its cards asked after Claude read this message: with this part's work. -->
              {#if here.length}
                {#await import('./PageConfirm.svelte') then { default: PageConfirm }}
                  <PageConfirm confirms={here} />
                {/await}
              {/if}
            {/each}
          {/if}
          {#if agentPills.get(i)?.length}
            {#await import('./AgentPills.svelte') then { default: AgentPills }}
              <AgentPills agents={agentPills.get(i)} onopen={openAgent} quiet={!!code?.current} />
            {/await}
            {#if code?.current}
              <!-- In a Code chat: each turn's to-do list under its pill, live while it works, then folded as its record. -->
              {#each agentPills.get(i).filter(hasTodos) as p (`${p.id}:${p.turn ?? 1}`)}
                {#await import('./AgentTodoCard.svelte') then { default: AgentTodoCard }}
                  <AgentTodoCard agent={p} turn={p.turn ?? 1} line={agents.steps[p.id] || ''} />
                {/await}
              {/each}
              {#each agentPills.get(i).filter((p) => plansOf(p).length) as p (`plans:${p.id}:${p.turn ?? 1}`)}
                <!-- An agent's plans sent back with comments, folded (Plan mode's review). -->
                {#await import('./PlanHistory.svelte') then { default: PlanHistory }}
                  <PlanHistory plans={plansOf(p)} revising={revisingNow(p)} />
                {/await}
              {/each}
              <!-- (An agent Chat sent out works in its own panel, as a subagent does in Claude Code: its work is
                   in its row there (AgentPanel), never here. Only the chat's own work, above, shows in the thread.) -->
            {/if}
          {/if}
          {#if placed.get(i) && !placed.get(i).inAnswer}
            <!-- Visuals and pages made for this question with no answer yet (a Code chat's Claude still working). -->
            {#await import('./VisualFrame.svelte') then { default: VisualFrame }}
              {#each placed.get(i).visuals as v (v.id)}
                <VisualFrame {v} conversation={chat.activeId} onprompt={(text) => sendFromVisual(text, v, chat.activeId)} />
              {/each}
            {/await}
            {#if placed.get(i).pages.length}
              {#await import('./PageCard.svelte') then { default: PageCard }}
                {#each placed.get(i).pages as p (`${p.page.id}:${p.version.n}`)}
                  <div><PageCard page={p.page} version={p.version} onopen={openKept} /></div>
                {/each}
              {/await}
            {/if}
          {/if}
        {/each}
        {#if asks.length || answeredRemote.length}
          <!-- A running agent of this chat asks to take a step: Allow once / Deny
               (and the ones a paired phone answered, closed with who). -->
          {#await import('./AgentApprove.svelte') then { default: AgentApprove }}
            <AgentApprove approvals={asks} answered={answeredRemote} onopen={openRun} />
          {/await}
        {/if}
        {#if chat.answeringHere}
          <!-- What this conversation's turn is doing now, with its time
               (chat/activity.rs): never another chat's. -->
          <ActivityLine
            step={chat.activity}
            waiting={chat.activityWaiting}
            seconds={chat.waitedS}
            hint={chat.modelHere && !answering && chat.waitedS >= STILL_WORKING_S ? slowHint : ''}
          />
        {/if}
        {#each chat.queued as q (q.qid)}
          <!-- Sent while Claude works: waiting for it (chatState's queue), as in Claude Code's own app. -->
          <QueuedMessage {q} oncancel={chat.cancelQueued} onsend={chat.sendQueuedNow} />
        {/each}
        {#each footTodos as p (`foot:${p.id}:${p.turn ?? 1}`)}
          <!-- The live to-do list: the thread's last thing while the turn works, just above the message box. -->
          {#await import('./AgentTodoCard.svelte') then { default: AgentTodoCard }}
            <div class="todofoot" data-testid="chat-todo-foot"><AgentTodoCard agent={p} turn={p.turn ?? 1} line={agents.steps[p.id] || ''} foot /></div>
          {/await}
        {/each}
      </div>
    </div>
    {#if chapters.length}
      <!-- The chat's contents: its chapters, to jump between (its own chunk). -->
      {#await import('./ChatChapters.svelte') then { default: ChatChapters }}
        <ChatChapters {chapters} thread={chat.threadEl} />
      {/await}
    {/if}
    {#if !pinned}
      <!-- Scrolled up: back to the newest, following it again. -->
      <button
        type="button"
        class="tobottom"
        class:unseen
        data-testid="chat-to-bottom"
        aria-label={unseen ? 'New messages: scroll to the latest' : 'Scroll to the latest'}
        title="Scroll to the latest"
        onclick={toLatest}
        transition:fade={{ duration: motionMs(120) }}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14" /><path d="m6 13 6 6 6-6" /></svg>
        {#if unseen}<span>New messages</span>{/if}
      </button>
    {/if}
  </div>
{/if}

{#if chat.showingSummary && chat.summary}
  <!-- Its own chunk, loaded the first time it opens. -->
  {#await import('./SummaryDialog.svelte') then { default: SummaryDialog }}
    <SummaryDialog summary={chat.summary} onclose={closeSummary} />
  {/await}
{/if}

{#snippet chapterDivider(c)}
  <!-- A chapter's title between two rules; one the person pinned (Pin as chapter) has the pin before it. -->
  <div class="chapterdiv" class:pinned={c.pinned} role="separator" aria-label={`Chapter: ${c.title}`} title={c.summary || undefined} data-testid="chat-chapter">
    <span class="rule"></span>
    {#if c.pinned}
      <svg class="cpin" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><g transform="rotate(45 12 12)"><path d="M8.6 2.6h6.8l-1 6.4 3.6 3.4v2.4H6.6v-2.4L10.2 9z" /><path d="M12 14.8v6.8" fill="none" /></g></svg>
    {/if}
    <span class="ctitle">{c.title}</span>
    <span class="rule"></span>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={rewindOpen}
  title="Rewind to before this message?"
  body="This message and everything after it go, and Claude won't see them again. Its words go back in the message box."
  confirmLabel="Rewind"
  busyLabel="Rewinding…"
  busy={rewindBusy}
  onconfirm={doRewind}
>
  {#if rewindAsk?.files}
    <label class="rewindfiles" data-testid="chat-rewind-files">
      <input type="checkbox" bind:checked={rewindFiles} />
      Also put back the {rewindAsk.files === 1 ? 'file' : `${rewindAsk.files} files`} Claude changed since, as they were
    </label>
  {/if}
</ConfirmDialog>

<style>
  .rewindfiles {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 10px;
    color: var(--yap-paper-ink);
    font-size: 13px;
    line-height: 1.4;
    cursor: pointer;
  }
  .rewindfiles input {
    margin-top: 2px;
  }
  /* Where the summary takes over: a thin rule, quiet words, Show summary. */
  /* A chapter's divider (mark_chapter): its title between two rules, a touch stronger than the summary's. */
  .chapterdiv {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 14px 0 4px;
    color: var(--yap-fg);
    font-size: 12px;
    font-weight: 600;
    scroll-margin-top: 40px;
  }
  .chapterdiv .rule {
    flex: 1 1 16px;
    height: 1px;
    background: var(--yap-border-subtle);
  }
  /* (Pinned by the person: the pin, then its title.) */
  .chapterdiv.pinned {
    gap: 6px;
  }
  .chapterdiv .cpin {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    margin-left: 4px;
    color: var(--yap-muted);
  }
  .chapterdiv.pinned .ctitle {
    margin-right: 4px;
  }
  .chapterdiv .ctitle {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .summarised {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .summarised .rule {
    flex: 1 1 16px;
    height: 1px;
    background: var(--yap-border-subtle);
  }
  .summarised .stext {
    flex: 0 1 auto;
    text-align: center;
  }
  .showsum {
    flex: 0 0 auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .showsum:hover {
    text-decoration: underline;
  }
  .showsum:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }
  /* **The start screen** (the Claude app's new-chat screen): the mark and the greeting centred at the foot of the
     thread's upper part, the box right under them (ChatView's `.thread.start` shares the room out). */
  .hello {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 0 8px 22px;
    text-align: center;
  }
  .greet {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4em;
    max-width: 100%;
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: calc(34px * var(--yap-reading-step));
    font-weight: 400;
    line-height: 1.15;
    letter-spacing: -0.005em;
    color: var(--yap-fg);
  }
  .greettext {
    min-width: 0;
    overflow-wrap: anywhere;
    text-wrap: balance;
    transition: opacity 0.14s;
  }
  /* (The name not read yet: the line's room kept, its words a moment later.) */
  .greettext.waiting {
    opacity: 0;
  }
  /* Yap's mark, a touch taller than the greeting's capitals. */
  .greet .mark {
    flex: 0 0 auto;
    width: 0.92em;
    height: 0.92em;
    user-select: none;
  }
  .greet .ghostmark {
    flex: 0 0 auto;
    width: 0.9em;
    height: 0.9em;
    color: var(--yap-fg);
  }
  .hellonote {
    max-width: 420px;
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .hellonote.needsai {
    color: var(--yap-fg-80);
  }
  /* A conversation asked across the meetings says so over its thread. */
  .scopeline {
    position: relative;
    /* (On the messages' column: the scrollbar's half to the left, as the composer.) */
    left: calc(-1 * min(var(--thread-gutter, 0px) / 2, max(0px, (100% - var(--yap-thread-width)) / 2)));
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .scopeline svg {
    width: 12px;
    height: 12px;
    color: var(--yap-primary);
  }
  .ghostline {
    color: var(--yap-fg-80);
  }
  .projlink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-weight: 650;
    cursor: pointer;
  }
  .projlink:hover {
    color: var(--yap-primary);
    text-decoration: underline;
  }
  .projlink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }
  .ghostline svg {
    color: var(--yap-fg);
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
  /* The thread's place: the scroller, and "↓" over its foot. */
  .threadbox {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .msgs {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: 2px;
  }
  /* The readable column: at most --yap-thread-width, centred in a wide
     thread; in a narrow one the thread's whole width, clear of the list's
     button while the list is hidden (ChatView's --thread-clear) — and in a
     wide one the centre never moves when it shows or goes. */
  .msgsin,
  .scopeline {
    max-width: var(--yap-thread-width);
    margin-left: max(var(--thread-clear, 0px), calc((100% - var(--yap-thread-width)) / 2));
    margin-right: auto;
  }
  .msgsin {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  /* The live to-do list at the foot: the column's whole width, as the Claude app's plan card, a little clear of the
     work above it. */
  .todofoot {
    align-self: stretch;
    display: flex;
    margin-top: 4px;
  }
  /* Scrolled up: a small round "↓" over the thread's foot, a pill with
     "New messages" when something came (as Claude's app and Slack have it). */
  .tobottom {
    position: absolute;
    /* (Under the column's centre: the scrollbar's half to the left.) */
    left: calc(50% - var(--thread-gutter, 0px) / 2);
    bottom: 10px;
    z-index: 2;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 30px;
    height: 30px;
    padding: 0 7px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-popover);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .tobottom.unseen {
    padding: 0 12px 0 9px;
    color: var(--yap-primary);
  }
  .tobottom:hover {
    background: var(--yap-s2);
    border-color: var(--yap-border-hover);
  }
  .tobottom:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  .tobottom svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
  }
</style>
