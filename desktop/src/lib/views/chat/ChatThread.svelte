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
  import { pillsByAnswer, pillsByQuestion } from '../../core/agents.js';
  import { chatStatus } from '../../stores/chatStatus.svelte.js';

  const agentPills = $derived(pillsByAnswer(agents.list, chat.activeId, chat.messages));
  // …and under the person's own message, in a Code chat that works directly
  // (agents/direct.rs): its session's work, with no answer of Chat's between.
  const directPills = $derived(pillsByQuestion(agents.list, chat.activeId, chat.messages));
  const asks = $derived(chat.activeId == null ? [] : agents.approvals.filter((a) => a.conversation === chat.activeId));
  // Answered from a paired phone (Remote Control): "Allowed from Office phone".
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
    openContextMenu(
      e,
      messageItems(m, { read: m.role === 'assistant' && m.id ? () => chat.readAloud(m) : null, reading }),
      { label: m.role === 'assistant' ? 'Answer' : 'Question', row: e.currentTarget }
    );
  }
</script>

{#if chat.messages.length === 0 && (code?.current || code?.onCode)}
  <!-- An empty Code chat: the Code side's own greeting. -->
  <CodeEmpty {chat} {code} {onopensettings} />
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
          {#if m.kind === 'agentReport'}
            {#await import('./AgentReportCard.svelte') then { default: AgentReportCard }}
              <AgentReportCard {m} conversation={chat.activeId} onopen={openAgent} {room} onpicture={openShared} quiet={!!code?.current} />
            {/await}
          {:else}
            <ChatMessage
              {m}
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
              conversation={chat.activeId}
              onshared={openShared}
            />
          {/if}
          {#if directPills.get(i)?.length}
            <!-- A Code chat that works directly: what its session says and does, under the message that asked. -->
            {#await import('./AgentPills.svelte') then { default: AgentPills }}
              <AgentPills agents={directPills.get(i)} onopen={openAgent} quiet />
            {/await}
            {#each directPills.get(i).filter(hasTodos) as p (`${p.id}:${p.turn ?? 1}`)}
              {#await import('./AgentTodoCard.svelte') then { default: AgentTodoCard }}
                <AgentTodoCard agent={p} turn={p.turn ?? 1} line={agents.steps[p.id] || ''} />
              {/await}
            {/each}
            {#each directPills.get(i) as p (`work:${p.id}:${p.turn ?? 1}`)}
              {#await import('./AgentWork.svelte') then { default: AgentWork }}
                <AgentWork agent={p} turn={p.turn ?? 1} line={agents.steps[p.id] || ''} onview={openShared} />
              {/await}
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
              <!-- …and its work as it goes, as Claude Code's own transcript tells it: what it says, and a line for each stretch of steps. -->
              {#each agentPills.get(i) as p (`work:${p.id}:${p.turn ?? 1}`)}
                {#await import('./AgentWork.svelte') then { default: AgentWork }}
                  <AgentWork agent={p} turn={p.turn ?? 1} line={agents.steps[p.id] || ''} onview={openShared} />
                {/await}
              {/each}
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
      </div>
    </div>
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

<style>
  /* Where the summary takes over: a thin rule, quiet words, Show summary. */
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
