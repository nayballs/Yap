<script>
  // Chat's thread: the conversation's messages (ChatMessage), and at its
  // foot while an answer comes what the turn is doing now and for how long
  // (ActivityLine: "Searching the web · … · 1m 59s"; a model on this PC slow
  // to start says why), or — no messages yet — what Chat is for, what it
  // needs if no AI model is set up, and the Voice Assistant's shortcut (or
  // how to set one). An empty Code chat greets you as the Code side instead
  // (CodeEmpty.svelte).
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
  import { formatHotkeySpec } from '../../core/hotkeys.js';
  import { messageItems } from '../../core/itemMenus.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { STILL_WORKING_S } from './chatState.svelte.js';
  import { SUMMARY_DIVIDER } from './chatBudget.js';
  import { skills } from '../../stores/skills.svelte.js';
  import { calendar } from '../../stores/calendar.svelte.js';
  import { promptChips } from '../../core/skills.js';
  import ChatMessage from './ChatMessage.svelte';
  import ActivityLine from './ActivityLine.svelte';
  import CodeEmpty from './CodeEmpty.svelte';
  // Yap Code (desktop/src-tauri/src/agents.rs): the pills of the agents an answer
  // started, an agent's report as an agent card (`kind: "agentReport"`), in a
  // Code chat each turn's to-do card under its pill (AgentTodoCard), and a
  // running agent's permission prompts for this chat — each in a small chunk.
  import { agents, agentById } from '../../stores/agents.svelte.js';
  import { pillsByAnswer, pillsByQuestion, answeredAfter, directLine, plainReport, agentOfReport, reportFolder, messageTurn, notesAfter, turnsOf, turnParts, cardPart } from '../../core/agents.js';
  import { agentLogs } from '../../stores/agentLogs.svelte.js';
  import { threadViewOf } from './threadView.svelte.js';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { chatStatus } from '../../stores/chatStatus.svelte.js';
  import QueuedMessage from './QueuedMessage.svelte';
  // A long chat's chapters (chat/cards/chapters.rs): a divider above the question each starts at, and the
  // floating contents (ChatChapters, its own chunk) to jump between them.
  import { chaptersOf } from './chapters.js';
  // What the chat made (chat/made.rs): its visuals and its kept pages' cards, each in the answer to its question.
  import { made, followMade, onMade, placeMade, promptFromVisual } from './made.svelte.js';

  const agentPills = $derived(pillsByAnswer(agents.list, chat.activeId, chat.messages));
  const chapters = $derived(chaptersOf(chat.messages));
  const chapterAt = $derived(new Map(chapters.map((c) => [c.at, c])));
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

  // The one-tap prompts (chat/skills.rs: the first three of Prepare for my
  // next meeting — with a calendar —, Write my stand-up, List my open
  // to-dos, What did we decide?), each asked across the meetings.
  const prompts = $derived(promptChips(skills.list, { calendar: calendar.connections.length > 0 }));

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
    // The person's message: Edit, and in a Code chat Rewind and Fork from here too (as under it); an answer: Retry.
    const a = actionsFor(m);
    const r = m.role === 'assistant' ? retryOf(m, chat.messages.indexOf(m)) : null;
    if (a?.onedit || a?.onrewind || r) items.push({ separator: true });
    if (a?.onedit) items.push({ label: a.edit ? `Edit (${a.edit.toLowerCase()})` : 'Edit', icon: 'pencil', disabled: !!a.edit, onselect: a.onedit });
    if (r) items.push({ label: r.why ? `Retry (${r.why.toLowerCase()})` : 'Retry', icon: 'sync', disabled: !!r.why, onselect: r.onretry });
    if (a?.onrewind) {
      items.push({ label: a.rewind ? `Rewind (${a.rewind.toLowerCase()})` : 'Rewind', icon: 'undo', disabled: !!a.rewind, onselect: a.onrewind });
      items.push({ label: a.fork ? `Fork from here (${a.fork.toLowerCase()})` : 'Fork from here', icon: 'split', disabled: !!a.fork, onselect: a.onfork });
    }
    openContextMenu(e, items, { label: m.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget });
  }

  // **Rewind and Fork from here** (14 Nov 2026, agents/rewind.rs; the Claude Code app's message actions): under the
  // person's own message in a Code chat — one that started a turn of Claude's —, Copy, Rewind (back to before it:
  // asked first, the files Claude changed since put back as they were unless unticked) and Fork from here (a new chat
  // with the conversation up to its answer).
  function actionsFor(m) {
    if (m?.role !== 'user' || m.kind || chat.activeId == null) return null;
    // Still being sent (no id until its answer is in): the row is there already, Copy in it and the rest waiting —
    // drawn only once the answer was in, it pushed the thread 36 px down at every answer's end (15 Nov 2026,
    // chat-scroll.spec.js; the browser's scroll anchoring hid it but moved the scrollbar).
    if (!m.id) return code?.current ? {} : { edit: busyWhy || 'Not saved yet', onedit: () => {} };
    // Chat: Copy and Edit (a new version from there, ‹ 1/2 › on it).
    if (!code?.current) return { edit: busyWhy, onedit: () => chat.startEdit(m) };
    const t = messageTurn(agents.list, chat.activeId, m.id);
    // A message no turn of Claude's started (yet): Copy, Rewind and Fork once its turn is known — the same row.
    if (!t) return {};
    // A Code chat's Edit is a Rewind with new words (agents/rewind.rs): when it can rewind.
    return { rewind: t.rewind, fork: t.fork, onrewind: () => askRewind(m, t.files), onfork: () => chat.forkAt(m), edit: t.rewind || busyWhy, onedit: () => chat.startEdit(m) };
  }

  // **Retry, Edit and ‹ ›** (15 Nov 2026, the Claude app's message actions; chat/chats/branches.rs): ↻ under an
  // answer asks its question again, the pencil under the person's message opens it in place to change; either keeps
  // what was there as a version, ‹ 1/2 › to switch. Not while an answer is being written. In a Code chat both are
  // Claude Code's own Rewind (asked first, as Rewind is), then the message sent again: no versions kept there.
  const busyWhy = $derived(chat.sending || chat.answeringHere ? 'Unavailable while an answer is being written' : '');
  /** ↻ under answer `m` (index `i`), or null where there's none. */
  function retryOf(m, i) {
    if (m?.role !== 'assistant' || !m.id || m.kind || m.streaming || i < 1) return null;
    if (code?.current) {
      const qi = chat.messages.findLastIndex((x, k) => k < i && x.role === 'user' && !x.kind);
      const q = chat.messages[qi];
      const t = q?.id ? messageTurn(agents.list, chat.activeId, q.id) : null;
      if (!t) return null;
      return { why: t.rewind || busyWhy, onretry: () => askCodeSend(q, null, t.files) };
    }
    const q = chat.messages[i - 1];
    if (q?.role !== 'user' || q.kind || !q.id) return null;
    return { why: busyWhy, onretry: () => chat.retryAt(m) };
  }
  /** The ‹ k/n › of the message at `i`, or null. */
  function branchOf(i) {
    const fork = chat.forkOf(i);
    return fork ? { fork, why: busyWhy, onbranch: (place) => chat.switchBranch(i, place) } : null;
  }
  /** Save and send an edited message: in a Code chat, asked first as a Rewind is. */
  function saveEdited(m, words) {
    if (!code?.current) {
      chat.saveEdit(m, words);
      return;
    }
    const t = messageTurn(agents.list, chat.activeId, m.id);
    if (!t || t.rewind) return;
    askCodeSend(m, words, t.files);
  }

  let rewindOpen = $state(false);
  let rewindAsk = $state(null);
  let rewindFiles = $state(true);
  let rewindBusy = $state(false);
  function askRewind(m, files) {
    rewindAsk = { m, files, mode: 'rewind' };
    rewindFiles = true;
    rewindOpen = true;
  }
  /** A Code chat's Retry (`words` null: the message as it was) or Edit: back to before `m`, then sent — asked first. */
  function askCodeSend(m, words, files) {
    rewindAsk = { m, files, mode: words == null ? 'retry' : 'edit', words };
    rewindFiles = true;
    rewindOpen = true;
  }
  async function doRewind() {
    if (!rewindAsk) return;
    const { m, files, mode, words } = rewindAsk;
    if (mode !== 'rewind') {
      rewindOpen = false;
      chat.saveEdit(m, words ?? chat.wordsOf(m), { files: files > 0 && rewindFiles, code: true });
      return;
    }
    rewindBusy = true;
    const ok = await chat.rewindAt(m, files > 0 && rewindFiles);
    rewindBusy = false;
    rewindOpen = false;
    if (ok) chat.inputEl?.focus({ preventScroll: true });
  }
  // (Its words: what the dialog says for each.)
  const rewindText = $derived(
    rewindAsk?.mode === 'retry'
      ? { title: 'Retry from this message?', body: "Everything after it goes, as with Rewind, and Claude won't see it again. Then the message is sent again.", confirm: 'Retry', busy: 'Retrying…' }
      : rewindAsk?.mode === 'edit'
        ? { title: 'Send the edited message?', body: "This message and everything after it go, as with Rewind, and Claude won't see them again. Your new words are sent in its place.", confirm: 'Send', busy: 'Sending…' }
        : { title: 'Rewind to before this message?', body: "This message and everything after it go, and Claude won't see them again. Its words go back in the message box.", confirm: 'Rewind', busy: 'Rewinding…' }
  );
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
{:else if chat.messages.length === 0 && chat.incognito}
  <!-- A new chat with its ghost on (chat/incognito.rs): what that means. -->
  <div class="empty" data-testid="chat-incognito-empty">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 20V11a7 7 0 0 1 14 0v9l-2.33-1.5L14.33 20 12 18.5 9.67 20l-2.34-1.5z" /><circle cx="9.5" cy="11" r="1" fill="currentColor" stroke="none" /><circle cx="14.5" cy="11" r="1" fill="currentColor" stroke="none" /></svg>
    <p class="across">Incognito chat</p>
    <p class="asknote">It isn't saved to your chats, and memory isn't used or updated. It's gone when you leave it.</p>
  </div>
{:else if chat.messages.length === 0}
  <div class="empty">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M8 9h8M8 13h5" /></svg>
    {#if chat.scope === 'meetings'}
      <!-- Asked across the meetings (search/ask.rs): found on this PC, cited. -->
      <p class="across">Ask across your meetings and notes</p>
      <p class="asknote">Answers come from what was said and written, and cite the moment: click a number to open it.</p>
    {:else}
      <p>Ask about your notes, transcripts, or anything else</p>
      <p class="asknote">
        <button class="setkey" onclick={() => chat.askAcrossMeetings()}>Ask across your meetings</button>
        for answers that cite the moment they come from.
      </p>
    {/if}
    {#if prompts.length}
      <!-- One-tap prompts: the question filled in and sent, across the meetings. -->
      <div class="prompts" role="group" aria-label="Try asking">
        {#each prompts as p (p.id)}
          <button class="prompt" disabled={chat.sending} onclick={() => chat.runPrompt(p)}>{p.chip}</button>
        {/each}
      </div>
      <p class="asknote">Or type / for Skills, like /standup or /decisions.</p>
    {/if}
    {#if !chat.chatReady}
      <!-- Before a first question fails: what Chat needs. -->
      <p class="asknote needsai">
        Chat needs an AI model: a free key from a cloud provider, or one that runs on this PC.
        <button class="setkey" onclick={() => onopensettings?.('cleanup#chat')}>Set up an AI model</button>
      </p>
    {/if}
    <!-- The Voice Assistant: the same chat, asked out loud from any app. -->
    <p class="asknote">
      {#if chat.assistantHotkey}
        Or press <kbd>{formatHotkeySpec(chat.assistantHotkey)}</kbd> in any app to ask out loud and hear the answer.
      {:else}
        <button class="setkey" onclick={() => onopensettings?.('voice#assistant-shortcut')}>Set an assistant shortcut</button>
        to ask out loud from any app and hear the answer.
      {/if}
    </p>
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
          {#if chapterAt.has(i)}
            <!-- A new chapter starts at this question (mark_chapter): its title, as a divider. -->
            {@const c = chapterAt.get(i)}
            <div class="chapterdiv" role="separator" aria-label={`Chapter: ${c.title}`} title={c.summary || undefined} data-testid="chat-chapter">
              <span class="rule"></span>
              <span class="ctitle">{c.title}</span>
              <span class="rule"></span>
            </div>
          {/if}
          {#if m.kind === 'agentReport'}
            {#if !quietReport(m)}
              {#await import('./AgentReportCard.svelte') then { default: AgentReportCard }}
                <AgentReportCard {m} conversation={chat.activeId} onopen={openAgent} {room} onpicture={openShared} quiet={!!code?.current} />
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
              retry={m.role === 'assistant' ? retryOf(m, i) : null}
              editing={m.role === 'user' && chat.editing?.id === m.id ? chat.editing : null}
              onedited={(words) => saveEdited(m, words)}
              oneditcancel={() => chat.cancelEdit()}
            />
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
            <!-- (Its to-do list is AgentWork's: at its top, or as it stood above a message it read.) -->
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

<ConfirmDialog
  bind:open={rewindOpen}
  title={rewindText.title}
  body={rewindText.body}
  confirmLabel={rewindText.confirm}
  busyLabel={rewindText.busy}
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
  .empty .across {
    max-width: 280px;
    color: var(--yap-fg);
    font-weight: 600;
  }
  /* The one-tap prompts on an empty chat. */
  .prompts {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    max-width: 420px;
    margin-top: 6px;
  }
  .prompt {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }
  .prompt:hover:not(:disabled) {
    background: var(--yap-s2);
    border-color: var(--yap-border-hover);
  }
  .prompt:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .prompt:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
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
