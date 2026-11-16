<script>
  // **An Agent panel row's own view** (15 Nov 2026, the last gap with the Claude app's Code tab, where a
  // background task opens into a view of its own; until then Yap's rows only opened in place). A panel tab of its
  // own (`task:<row key>`, chatDoc.svelte.js `showTask`), opened by a click on a row of the Agent panel
  // (AgentPanel.svelte): an agent's turn, or a helper the chat's own Claude sent. The panel stays as it was.
  // - **Its header**: the row's own character, as its row has it (Yap's crew, CrewAvatar: its model, its Mode's
  //   prop and its pose — walking while it works), its title, Mode · model · effort and how it stands, its clock
  //   (ticking while it works) and **Stop** while it works: an agent's turn stops as the tab's Stop stops it; a
  //   helper's stops the turn that sent it, asked first (Claude Code can't stop one helper alone).
  // - **Its work in full**: its to-do list, then what it said and every stretch of its steps, open (AgentWork:
  //   a row a step, an edit's to its diff, a command's to what it printed); a helper's own steps (StepRows).
  // - **Its report once it's over**: how it ended ("Agent done", "Agent didn't finish · why", "Agent stopped"),
  //   what it changed, its commands and files, what it used, and its whole timeline a click away; a helper's,
  //   what it came back with (agents/runner.rs `report`).
  // Live from the stores (agents.svelte.js, agentLogs.svelte.js): nothing polled; the clock ticks once a second.
  // The crew moves only on screen (the window shown, the view in sight), as in the panel.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { agents as agentsStore, agentById } from '../../stores/agents.svelte.js';
  import { agentLogs, watchAgentLog } from '../../stores/agentLogs.svelte.js';
  import { windowShown, followWindowShown } from '../../stores/windowShown.svelte.js';
  import { panelRows, statsLine, fmtClock, helperSecs, helperReport, modeTone } from '../../core/agentPanel.js';
  import { isLive, reportTitle, turnsOf, shownStepPicture } from '../../core/agents.js';
  import { characterFor, crewLabel } from '../../core/crew.js';
  import { renderMarkdown } from '../../core/markdown.js';
  import { colourCode } from '../../ui/colourCode.js';
  import { askConfirm } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import CrewAvatar from './CrewAvatar.svelte';
  import AgentWork from './AgentWork.svelte';
  import StepRows from './StepRows.svelte';

  /** `doc`: the panel's state (`doc.task`: `{key, agentId, n, helper, title}`); `narrow`: no room beside the chat. */
  let { doc, narrow = false } = $props();

  const ref = $derived(doc.task);
  const agent = $derived(ref ? agentById(ref.agentId) : null);
  $effect(() => {
    if (ref?.agentId) watchAgentLog(ref.agentId);
  });
  const log = $derived(agentLogs.by[ref?.agentId] || []);
  let now = $state(Date.now() / 1000);
  /** Its row, as the panel draws it (the same character, words and numbers). */
  const row = $derived(
    agent && ref ? (panelRows([agent], { logs: { [agent.id]: log }, steps: agentsStore.steps, now, approvals: agentsStore.approvals, dismissed: true }).find((r) => r.key === ref.key) ?? null) : null
  );
  const helper = $derived(row?.kind === 'helper');
  /** Its turn (a helper's: the turn that sent it), with its events. */
  const turn = $derived(row ? (turnsOf(agent, log).find((t) => t.n === row.n) ?? null) : null);
  const working = $derived(!!row && isLive(row.status));
  const started = $derived(!!row && (helper ? Number(row.helper.ts) > 0 : Number(row.turn?.startedTs) > 0));
  $effect(() => {
    if (!working) return;
    now = Date.now() / 1000;
    const tick = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => clearInterval(tick);
  });
  const crew = $derived(characterFor('claude-plan', row?.model));
  const clock = $derived(!row || !started ? '' : fmtClock(helper ? helperSecs(row.helper, turn?.events, now) : row.secs));
  /** Stop: while it works and something runs it (the row menu's own rule). */
  const canStop = $derived(working && !!agent?.active);
  let stopping = $state(false);

  // ---- Its report, once it's over
  const over = $derived(!!row && started && !working);
  const reportWords = $derived.by(() => {
    if (!over) return '';
    if (helper) return row.status === 'failed' ? "Helper didn't finish" : 'Helper done';
    if (row.unused) return row.statusWord;
    if (row.turn?.status === 'interrupted') return 'Agent interrupted';
    return reportTitle(row.turn);
  });
  const fileName = (f) => String(f || '').split(/[\\/]/).pop();
  const changed = $derived(!helper && row?.turn?.files?.length ? row.turn.files.map(fileName).join(', ') : '');
  const report = $derived(over && helper ? helperReport(row.helper, turn?.events) : '');
  const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

  // The crew moves only while it can be seen: the window on screen, the view in sight (as the panel).
  let rootEl = $state(null);
  let inView = $state(true);
  const paused = $derived(!(windowShown.shown && inView));
  onMount(() => {
    const stop = followWindowShown();
    const seen = new IntersectionObserver((entries) => (inView = entries[entries.length - 1].isIntersecting));
    if (rootEl) seen.observe(rootEl);
    return () => {
      stop();
      seen.disconnect();
    };
  });

  async function stopNow() {
    try {
      await invoke('agent_stop', { id: row.agentId });
    } catch (e) {
      toast({ title: helper ? "Couldn't stop the helper" : "Couldn't stop the agent", description: String(e?.message ?? e), variant: 'destructive' });
      return false;
    }
  }

  async function stop(e) {
    if (!row || stopping) return;
    if (helper) {
      askConfirm({
        title: 'Stop the helper?',
        body: "Claude Code can't stop one helper by itself: this stops the turn that sent it, Claude's own work too.",
        confirmLabel: 'Stop',
        busyLabel: 'Stopping…',
        returnFocus: e?.currentTarget ?? null,
        run: stopNow,
      });
      return;
    }
    stopping = true;
    await stopNow();
    stopping = false;
  }

  /** Its whole timeline: the agent's tab, at this turn. */
  function openTimeline() {
    if (!agent || row?.n == null) return;
    doc.showAgent({ id: agent.id, folder: agent.folder, status: agent.status, mode: agent.mode, conversation: agent.conversation, turn: row.n, view: 'timeline' });
  }

  const todoMark = (s) => (s === 'completed' ? '✓' : s === 'in_progress' ? '●' : s === 'stopped' ? '‖' : '○');
  const todoWord = (s) => (s === 'completed' ? ', done' : s === 'in_progress' ? ', in progress' : s === 'stopped' ? ', stopped' : ', to do');
</script>

<section
  class="tasktab"
  bind:this={rootEl}
  aria-label={row ? `${helper ? 'Helper' : 'Agent'}: ${row.title}` : 'Agent'}
  data-testid="chat-task"
  data-key={ref?.key ?? ''}
  data-kind={row?.kind ?? ''}
  data-status={row?.status ?? ''}
  data-crew-paused={paused}
>
  <header class="taskhead">
    {#if row}
      <!-- Its row's own character, as the row has it. -->
      <CrewAvatar character={crew} prop={row.modeTone} pose={row.pose} label={crewLabel(crew, row.pose, row.modeWord)} testid="chat-task-avatar" />
    {/if}
    <div class="tmain">
      <h2 class="ttitle" title={row?.title || ref?.title} data-testid="chat-task-title">{row?.title || ref?.title || 'Agent'}</h2>
      {#if row}
        <p class="tsub" data-testid="chat-task-sub">
          <span class="mode tone-{modeTone(row.mode)}">{row.modeWord}</span>{[row.modelWord, row.effortWord]
            .filter(Boolean)
            .map((w) => ` · ${w}`)
            .join('')}
        </p>
      {/if}
    </div>
    {#if clock}<span class="tclock" class:live={working} data-testid="chat-task-clock" aria-label={`Worked ${clock}`}>{clock}</span>{/if}
    {#if canStop}
      <button type="button" class="tstop" data-testid="chat-task-stop" disabled={stopping} onclick={stop} aria-label={helper ? 'Stop the helper (stops the turn that sent it)' : 'Stop the agent'}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
        Stop
      </button>
    {/if}
    <div class="tactions">
      {#if !narrow}
        <button
          type="button"
          class="tbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="tbtn" onclick={() => doc.close()} aria-label="Close the panel" title={working ? 'Close (it carries on)' : 'Close'}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>

  <div class="taskbody">
    {#if !agent}
      <p class="tnote">This agent isn't in Yap any more.</p>
    {:else if !row}
      <p class="tnote">Reading its work…</p>
    {:else}
      <!-- How it stands: the line its row says ("3/7 · Writing the planting guide", "Waiting for you…"). -->
      <p class="tline" class:live={working} data-testid="chat-task-line">
        {#if working}<span class="spin" aria-hidden="true"></span>{/if}{row.line}
      </p>
      {#if !helper && row.todos.length}
        <ul class="todolist" data-testid="chat-task-todos" aria-label="To-dos">
          {#each row.todos as t, i (i)}
            <li class="todo" class:done={t.status === 'completed'} class:doing={t.status === 'in_progress'} data-status={t.status}>
              <span class="tmark" aria-hidden="true">{todoMark(t.status)}</span>
              <span class="ttext">{t.content}</span>
              <span class="sr-only">{todoWord(t.status)}</span>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="twork" data-testid="chat-task-work">
        {#if helper}
          {#if row.helper.steps.length}
            <StepRows steps={row.helper.steps} busy={working} onview={(p) => doc.showPicture(shownStepPicture(p, agent.folder))} />
          {:else}
            <p class="tnote">No steps yet.</p>
          {/if}
        {:else if row.events.length}
          <AgentWork agent={row.agent} turn={row.n} events={row.events} status={row.status} line={agentsStore.steps[row.agentId] || ''} wide stretchesOpen onview={(p) => doc.showPicture(p)} />
        {:else}
          <p class="tnote">{started ? 'Nothing to show yet.' : 'Not started yet.'}</p>
        {/if}
      </div>
      {#if over}
        <!-- Its report: how it ended, what it changed, what it used; a helper's, what it came back with. -->
        <div class="treport tone-{row.status === 'failed' ? 'failed' : row.status === 'stopped' || row.status === 'interrupted' ? 'idle' : 'done'}" role="note" data-testid="chat-task-report">
          <p class="rtitle" data-testid="chat-task-report-title"><span class="rdot" aria-hidden="true"></span>{reportWords}</p>
          {#if row.status === 'failed' && row.turn?.error}<p class="rline">{row.turn.error}</p>{/if}
          {#if helper}
            {#if report}
              <div class="rtext" data-testid="chat-task-report-text" use:colourCode>{@html renderMarkdown(report)}</div>
            {/if}
            <p class="rline" data-testid="chat-task-report-counts">{row.line}</p>
          {:else}
            {#if changed}<p class="rline" data-testid="chat-task-report-changed">Changed {changed}</p>{/if}
            <p class="rline" data-testid="chat-task-report-counts">{plural(row.commands, 'command', 'commands')} · {plural(row.files, 'file', 'files')} changed{statsLine(row) ? ` · ${statsLine(row)}` : ''}</p>
            <button type="button" class="tpill" data-testid="chat-task-timeline" onclick={openTimeline}>Its whole timeline</button>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</section>

<style>
  .tasktab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .taskhead {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px 9px 16px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .tmain {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .ttitle {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13.5px;
    font-weight: 650;
    color: var(--yap-paper-ink);
  }
  .tsub {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-text-2);
    font-size: 11.5px;
  }
  /* The Mode, coloured as the panel colours it. */
  .mode {
    font-weight: 650;
    color: var(--tone);
  }
  .tone-auto {
    --tone: var(--yap-code);
  }
  .tone-manual {
    --tone: var(--yap-paper-text-2);
  }
  .tone-edits {
    --tone: var(--yap-primary);
  }
  .tone-plan {
    --tone: var(--yap-success-text);
  }
  .tone-bypass {
    --tone: var(--yap-warning);
  }
  .tclock {
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .tclock.live {
    color: var(--yap-paper-text-2);
  }
  .tstop {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .tstop svg {
    width: 10px;
    height: 10px;
  }
  .tstop:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .tstop:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .tactions {
    display: flex;
    gap: 2px;
  }
  .tbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .tbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .tbtn:focus-visible,
  .tstop:focus-visible,
  .tpill:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tbtn svg {
    width: 15px;
    height: 15px;
  }
  .taskbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 16px 18px;
  }
  .tnote {
    margin: 4px 0;
    color: var(--yap-paper-muted);
    font-size: 12.5px;
  }
  .tline {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0 0 10px;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .spin {
    flex: 0 0 auto;
    width: 10px;
    height: 10px;
    border: 1.6px solid color-mix(in srgb, var(--yap-paper-muted) 40%, transparent);
    border-top-color: var(--yap-paper-text-2);
    border-radius: 50%;
    animation: tspin 0.8s linear infinite;
  }
  @keyframes tspin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
  .todolist {
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }
  .todo {
    display: flex;
    gap: 7px;
    padding: 2px 0;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .todo.done .ttext {
    color: var(--yap-paper-muted);
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--yap-paper-muted) 60%, transparent);
  }
  .todo.doing {
    color: var(--yap-paper-ink);
    font-weight: 600;
  }
  .tmark {
    flex: 0 0 auto;
    width: 12px;
    text-align: center;
  }
  .twork {
    min-width: 0;
  }
  .treport {
    margin-top: 14px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
  }
  .rtitle {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    font-weight: 650;
    color: var(--yap-paper-ink);
  }
  .rdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .treport.tone-failed .rdot {
    background: var(--yap-danger);
  }
  .treport.tone-idle .rdot {
    background: var(--yap-paper-muted);
  }
  .rline {
    margin: 4px 0 0 15px;
    color: var(--yap-paper-text-2);
  }
  .rtext {
    margin: 6px 0 0 15px;
    color: var(--yap-paper-ink);
    line-height: 1.5;
  }
  .rtext :global(p) {
    margin: 0 0 6px;
  }
  .tpill {
    margin: 8px 0 0 15px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .tpill:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
