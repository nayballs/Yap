<script>
  // **The Agent panel**: the face of an agent's tab beside the chat
  // (AgentTab.svelte), after savvy-progress's agents panel (MIT,
  // github.com/JohnnyVizz/claude-kit; its maths in core/agentPanel.js).
  // - **The header**: the agent's Mode, model and effort, its workspace's
  //   path, and three tiles — Cost (≈ on the plan: what the API would
  //   charge), Tokens and Time (its turns added up).
  // - **Groups**, each with its count, folding: Needs you (a card waits on
  //   the person), Running, Finished (its head says Collapse / Expand),
  //   Queued. A row is one turn of the Code chat's agent (and of any other
  //   agent it has): **its model's character** (Yap's crew,
  //   CrewAvatar.svelte: Haiku, Sonnet, Opus or Fable, holding its Mode's
  //   prop — walking while it works, looking up with an amber "!" when it
  //   needs you, sitting with a ✓ done, grey stopped, a red badge failed),
  //   its title (whole on a hover), Mode · model · effort, "3/7 · the to-do
  //   in progress" (else the line it's on), ctx % · tokens · ≈ $ · time, and
  //   a thin bar: its to-dos done, else its context's fill in grey.
  // - **Rows start folded**, the running one too; **▸ opens a row** in
  //   place: why Chat sized it so ("Opus 5.5 · High: a multi-file change",
  //   the turn's `sizing`), its to-do list, its work (what it said and did, as AgentWork shows it: a line a stretch, rows that open to a diff or a command), how many commands and files,
  //   and **Open** — the turn's whole timeline (the tab's other view). Space
  //   (→ ←) opens and folds; ↑ ↓ move between rows. Which rows are open
  //   and which groups folded is remembered per agent while the window stays
  //   open (agentPanelMemory.svelte.js); a pill or a report card opens its own.
  // - **A click on a row opens its own view** (15 Nov 2026, as the Claude app
  //   opens a background task): a tab of the panel (AgentTaskTab.svelte) with
  //   the row's character, its work in full, its clock, Stop while it works and
  //   its report; Enter too. ▸ (and Space, → ←) still opens it here in place.
  //   The rows look as they did: nothing restyled.
  // - **The crew moves only on screen**: paused while the window is hidden
  //   or minimised (stores/windowShown.svelte.js) or the panel is out of
  //   view; still under reduced motion (crew.css).
  // - **Needs you only while something truly waits, live** (2026-11-08):
  //   a permission prompt still waiting, a plan's go-ahead on the latest
  //   turn with nothing newer begun, an ask-first card — each row with its
  //   card's own buttons inline (Allow once / Deny and what it asks, Go
  //   ahead / Not yet, Send / Don't), through the same commands the thread's
  //   cards use, so whichever is answered first, both close. A plan nobody
  //   acted on is "Plan not used", in Finished (Go ahead in its details).
  // - **A row's menu** (⋯, right-click, the menu key): Move to finished (a
  //   row whose agent nothing runs), Dismiss / Restore ("Show dismissed" at
  //   Finished's foot), Stop (asked first) for an agent at work.
  // - **Its helpers** (12 Nov 2026, as Claude Code's own app lists a
  //   session's subagents): under a turn's row, an entry for each part its
  //   session handed to a helper — what it was asked ("Check the docs"),
  //   the step it's on while it works (a spinner), then ✓ and how many
  //   steps it took (✕: it didn't finish). An entry opens to the helper's
  //   own steps, rows as the thread's (StepRows: an edit to its diff, a
  //   command to what it printed). core/agents.js `helpersOf`.
  // - **The chat itself is no row** (14 Nov 2026, the maintainer: "Anytime I
  //   speak it starts an agent on the right hand side… right now I speak to
  //   you and you decide whether to pass work to agents"): a Code chat that
  //   works directly sends the person's message to its own session — the
  //   chat talking, as in Claude Code's own app — so that turn has no row,
  //   and the helpers its Claude sends are rows of their own (Helper · what
  //   it was asked · the step it's on), opening to their steps. The header
  //   still adds up all the chat used. Nothing sent yet: "No agents yet".
  // - **Tidy, as the Claude app's agents panel** (18 Nov 2026, the
  //   maintainer's chat listed "Finished · 92", every row a card of four
  //   lines): **Running** at the top, always open — each row its character
  //   walking, its title, its clock ticking in the Code teal, what it's on and
  //   a thin live bar (its to-dos, else a sweep). **Finished today**: compact
  //   rows of two lines — a small character, the title and a ✓, then "Opus
  //   5.5 · 5/5 done · ≈ $2.37 · 4:53" — a bar only where to-dos were left.
  //   **Older work folds by day** behind one quiet line, "Show 89 earlier ›",
  //   opening to "Yesterday", "Mon 16 Nov", … with the same rows: nothing
  //   deleted, each row still opening to its view. The fold is per chat, and
  //   a new day folds it again (`panelMemory.earlier`: the day it was
  //   opened). Only what's open is drawn, and the rows are read again once a
  //   minute by the clock (a running row's clock and the Time tile tick on
  //   their own), so a chat of 100 rows stays quick.
  // Live from the `yap-agent` events the tab and stores/agents.svelte.js
  // hear: nothing polled.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick, untrack } from 'svelte';
  import {
    panelRows,
    panelGroups,
    panelTotals,
    statsLine,
    fmtTokens,
    fmtClock,
    costWords,
    modeTone,
    rowActions,
    finishedByDay,
    finishedAt,
    earlierOpen,
    dayKey,
    compactOf,
    liveSecs,
  } from '../../core/agentPanel.js';
  import { modeLabel, modelLabel, costLine, isLive } from '../../core/agents.js';
  import { effortLabel } from '../../core/effort.js';
  import { characterFor, crewLabel } from '../../core/crew.js';
  import { overflowTip } from '../../ui/overflowTip.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { windowShown, followWindowShown } from '../../stores/windowShown.svelte.js';
  import { answerApproval, goAheadWithPlan, rowAction } from '../../stores/agents.svelte.js';
  import { panelMemory } from './agentPanelMemory.svelte.js';
  import CrewAvatar from './CrewAvatar.svelte';

  /**
   * `agent`: the tab's agent as it is now; `agents`: the Code chat's agents,
   * the tab's first; `logs`: agent id → its log; `steps`: agent id → the line
   * it's on; `approvals`: the permission prompts waiting (stores/agents);
   * `cards`: the agent ask-first cards waiting in its chat
   * (stores/chatStatus); `now`: unix seconds (the clock); `want`: `{key,
   * seq}`, a row to open and show (a pill's, a report card's);
   * `onopen(row)`: Open; `ontask(row)`: a row's own view (a click on it).
   */
  let { agent, agents = [], logs = {}, steps = {}, approvals = [], cards = [], now = Date.now() / 1000, want = null, onopen = null, onpicture = null, ontask = null } = $props();

  /**
   * The clock by the minute: the rows are read again with it (a plan's wait, a
   * finished row's day), never every second — a running row's clock and the
   * Time tile read `now` themselves (`liveSecs`).
   */
  const minute = $derived(Math.floor(now / 60) * 60);
  /** Every row, the dismissed ones too (the totals count them); the ones shown. */
  const every = $derived(panelRows(agents, { logs, steps, now: minute, approvals, cards, dismissed: true }));
  const showDismissed = $derived(!!panelMemory.dismissed[agent?.id ?? '']);
  const dismissedCount = $derived(every.filter((r) => r.dismissed).length);
  const rows = $derived(showDismissed ? every : every.filter((r) => !r.dismissed));
  const groups = $derived(panelGroups(rows));
  // (What the chat used in all: its own turns too, though they're no rows.)
  const ownRows = $derived(panelRows(agents, { logs, steps, now: minute, approvals, dismissed: true, own: true }).filter((r) => r.kind === 'turn'));
  const totals = $derived(panelTotals(ownRows));
  /** The Time tile, ticking: the turns at work counted to now. */
  const totalSecs = $derived(totals.secs + ownRows.filter((r) => isLive(r.status)).reduce((s, r) => s + Math.max(0, liveSecs(r, now) - r.secs), 0));
  /** A Code chat that works directly: the person's own messages go to its session (no rows of their own). */
  const talks = $derived(agents.some((a) => (a?.turnList || []).some((t) => t.direct)));
  const cost = $derived(costWords(totals.cost, { pay: agent?.pay, estimated: totals.approx && agent?.pay === 'key' }));

  // ---- Finished: today's, then the days before, folded ----

  /** The groups above Finished (Needs you, Running, Queued), and Finished's rows by day. */
  const above = $derived(groups.filter((g) => g.id !== 'finished'));
  const finished = $derived(groups.find((g) => g.id === 'finished') ?? null);
  const byDay = $derived(finishedByDay(finished?.rows ?? [], minute));
  /** The chat the fold is kept for (its agents share it); an agent of no chat, its own. */
  const chatKey = $derived(String(agent?.conversation ?? agent?.id ?? ''));
  /** The days before today: open only on the day they were opened (a new day folds them again). */
  const showEarlier = $derived(earlierOpen(panelMemory.earlier[chatKey], minute));
  const toggleEarlier = () => (panelMemory.earlier = { ...panelMemory.earlier, [chatKey]: showEarlier ? '' : dayKey(minute) });

  let rootEl = $state(null);
  /** Row key → open, "<agent>:<group>" → folded: remembered while the window stays open. */
  const open = $derived(panelMemory.open);
  const groupKey = (g) => `${agent?.id ?? ''}:${g}`;
  const isFolded = (g) => !!panelMemory.folded[groupKey(g)];
  const setFolded = (g, v) => (panelMemory.folded = { ...panelMemory.folded, [groupKey(g)]: v });

  // The crew moves only while it can be seen: the window on screen, the panel in view.
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

  // A pill's or a report card's turn: its row open, its group unfolded, in view.
  $effect(() => {
    const key = want?.key;
    want?.seq;
    if (!key) return;
    untrack(async () => {
      panelMemory.open = { ...panelMemory.open, [key]: true };
      const r = rows.find((x) => x.key === key);
      const g = r?.state;
      if (g && isFolded(g)) setFolded(g, false);
      // A row finished before today: the days before open, to show it.
      if (r && g === 'finished' && dayKey(finishedAt(r)) !== dayKey(minute) && !showEarlier) toggleEarlier();
      await tick();
      rootEl?.querySelector(`[data-testid="chat-agent-row"][data-key="${CSS.escape(key)}"]`)?.scrollIntoView?.({ block: 'nearest' });
    });
  });

  const toggle = (key) => (panelMemory.open = { ...panelMemory.open, [key]: !panelMemory.open[key] });
  /** A helper's entry, opened to its steps ("<row>|<helper>": kept with the rows opened). */
  const helperKey = (r, h) => `${r.key}|${h.key}`;
  /** A row's character, from its model: Haiku, Sonnet, Opus or Fable (Sonnet when it says none). */
  const crewOf = (r) => characterFor('claude-plan', r.model);

  /** ↑ ↓ (and Home, End) between the rows' heads. */
  function nav(e) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
    const heads = [...(rootEl?.querySelectorAll('[data-row-head]') ?? [])];
    const at = heads.indexOf(e.currentTarget);
    if (at < 0) return;
    e.preventDefault();
    const to = e.key === 'Home' ? 0 : e.key === 'End' ? heads.length - 1 : Math.max(0, Math.min(heads.length - 1, at + (e.key === 'ArrowDown' ? 1 : -1)));
    heads[to]?.focus();
  }

  // ---- Needs you's buttons, and a row's menu ----

  /** The row key being answered (its buttons off meanwhile). */
  let acting = $state('');
  /** Each row's element (its menu's focus back, its ⋯ kept while the menu is up). */
  const rowEls = $state({});

  async function doing(r, what, run) {
    if (acting) return;
    acting = r.key;
    try {
      await run();
    } catch (e) {
      toast({ title: `Couldn't ${what}`, description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      acting = '';
    }
  }

  /** A permission prompt's Allow once / Deny — a plan's Go ahead / Not yet: the thread's card's own answer. */
  const answer = (r, allow) => doing(r, allow ? 'allow it' : 'deny it', () => answerApproval(r.needs.approval.id, allow));
  /** A plan its turn ended with: Go ahead (its report card's own), Not yet (set aside: Plan not used). */
  const goAhead = (r) => doing(r, 'go ahead with the plan', () => goAheadWithPlan(r.agentId, r.n));
  const notYet = (r) => doing(r, 'set the plan aside', () => rowAction('agent_move_to_finished', r.agentId, r.n));
  /** An ask-first card's Send / Don't (`chat_page_confirm`, as the thread's card answers it). */
  const confirm = (r, allow) => doing(r, allow ? 'send it' : 'cancel it', () => invoke('chat_page_confirm', { id: r.card.id, allow }));

  /** **A row's menu**: Move to finished, Dismiss / Restore, Stop (asked first). */
  function rowMenu(e, r) {
    const a = rowActions(r);
    const items = [
      a.finish && { label: 'Move to finished', icon: 'check', onselect: () => doing(r, 'move it to Finished', () => rowAction('agent_move_to_finished', r.agentId, r.n)) },
      a.dismiss && { label: 'Dismiss', icon: 'archive', onselect: () => doing(r, 'dismiss it', () => rowAction('agent_dismiss', r.agentId, r.n, { dismissed: true })) },
      a.restore && { label: 'Restore', icon: 'restore', onselect: () => doing(r, 'restore it', () => rowAction('agent_dismiss', r.agentId, r.n, { dismissed: false })) },
      a.stop && { separator: true },
      a.stop && {
        label: 'Stop…',
        icon: 'stop',
        danger: true,
        onselect: () =>
          askConfirm({
            title: 'Stop the agent?',
            body: `The agent in ${r.agent?.folder || 'this workspace'} stops where it is. Ask Claude in the chat to carry it on later.`,
            confirmLabel: 'Stop',
            busyLabel: 'Stopping…',
            returnFocus: rowEls[r.key] ?? null,
            run: async () => {
              try {
                await invoke('agent_stop', { id: r.agentId });
              } catch (err) {
                toast({ title: "Couldn't stop the agent", description: String(err?.message ?? err), variant: 'destructive' });
                return false;
              }
            },
          }),
      },
    ];
    return openContextMenu(e, items, { label: `Turn ${r.title}`, row: rowEls[r.key] ?? null });
  }
  const hasMenu = (r) => {
    const a = rowActions(r);
    return a.finish || a.dismiss || a.restore || a.stop;
  };

  /** A row with a view of its own (an agent's turn, a helper): not an ask-first card's. */
  const hasView = (r) => !!ontask && (r.kind === 'turn' || r.kind === 'helper');

  /**
   * **A click on a row opens its own view** (15 Nov 2026, as the Claude app opens a background task: AgentTaskTab,
   * a tab of the panel); its ▸ opens it here in place, as ever.
   */
  function headClick(e, r) {
    if (hasView(r) && !e.target?.closest?.('[data-fold]')) ontask(r);
    else toggle(r.key);
  }

  /** A row's head's keys: ↑ ↓ between rows, the menu key opens its menu; Space, → and ← open and fold it in place (Enter, its view). */
  function headKey(e, r) {
    if (menuKey(e, (ev) => rowMenu(ev, r))) return;
    if (hasView(r) && (e.key === ' ' || ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && !!open[r.key] === (e.key === 'ArrowLeft')))) {
      e.preventDefault();
      toggle(r.key);
      return;
    }
    nav(e);
  }

  const toggleDismissed = () => (panelMemory.dismissed = { ...panelMemory.dismissed, [agent?.id ?? '']: !showDismissed });

  const pct = (v) => `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%`;
  const todoMark = (s) => (s === 'completed' ? '✓' : s === 'in_progress' ? '●' : s === 'stopped' ? '‖' : '○');
  const todoWord = (s) => (s === 'completed' ? ', done' : s === 'in_progress' ? ', in progress' : s === 'stopped' ? ', stopped' : ', to do');
  const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
</script>

<div class="apanel" bind:this={rootEl} data-testid="chat-agent-panel" data-crew-paused={paused}>
  <header class="phead">
    <p class="pmeta" data-testid="chat-agent-panel-meta">
      <span class="mode tone-{modeTone(agent?.mode)}">{modeLabel(agent?.mode)}</span>
      {#if agent?.model}<span>· {modelLabel(agent.model)}</span>{/if}
      {#if agent?.effort}<span>· {effortLabel(agent.effort)}</span>{/if}
    </p>
    {#if agent?.path}<p class="ppath" title={agent.path}>{agent.path}</p>{/if}
    <div class="tiles" role="group" aria-label="In all">
      <div class="tile" data-testid="chat-agent-total-cost" title={costLine(agent)}>
        <span class="tk">Cost <span class="tc">{agent?.pay === 'key' ? 'API key' : 'on the API'}</span></span>
        <span class="tv">{cost || '—'}</span>
      </div>
      <div class="tile" data-testid="chat-agent-total-tokens">
        <span class="tk">Tokens</span>
        <span class="tv">{totals.known ? fmtTokens(totals.tokens) : '—'}</span>
      </div>
      <div class="tile" data-testid="chat-agent-total-time">
        <span class="tk">Time</span>
        <span class="tv">{fmtClock(totalSecs)}</span>
      </div>
    </div>
  </header>

  {#each above as g (g.id)}
    {@const shut = g.id !== 'running' && isFolded(g.id)}
    <section class="group" data-testid="chat-agent-group" data-group={g.id} aria-label={g.label}>
      {#if g.id === 'running'}
        <!-- Running: always open (what works now is never folded away). -->
        <h3 class="ghead static" data-testid="chat-agent-group-head"><span>{g.label} · {g.rows.length}</span></h3>
      {:else}
        <button type="button" class="ghead" aria-expanded={!shut} onclick={() => setFolded(g.id, !shut)} data-testid="chat-agent-group-head">
          <svg class="chev" class:down={!shut} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          <span>{g.label} · {g.rows.length}</span>
        </button>
      {/if}
      {#if !shut}
        <ul class="rows">
          {#each g.rows as r (r.key)}
            {@render row(r, g.id === 'running' ? 'running' : 'full')}
          {/each}
        </ul>
      {/if}
    </section>
  {/each}

  {#if finished}
    {@const shut = isFolded('finished')}
    <section class="group" data-testid="chat-agent-group" data-group="finished" aria-label="Finished">
      {#if byDay.today.length}
        <button
          type="button"
          class="ghead wide"
          data-act={shut ? 'Expand' : 'Collapse'}
          aria-expanded={!shut}
          onclick={() => setFolded('finished', !shut)}
          data-testid="chat-agent-group-head"
        >
          <svg class="chev" class:down={!shut} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          <span>Finished today · {byDay.today.length}</span>
        </button>
        {#if !shut}
          <ul class="rows compact">
            {#each byDay.today as r (r.key)}
              {@render row(r, 'compact')}
            {/each}
          </ul>
        {/if}
      {/if}
      {#if byDay.count && !(byDay.today.length && shut)}
        <!-- Work from before today: one quiet line, opening to it day by day (drawn only while open). -->
        <button type="button" class="earlierbtn" data-testid="chat-agent-earlier" aria-expanded={showEarlier} onclick={toggleEarlier}>
          <span>{showEarlier ? `Hide ${byDay.count} earlier` : `Show ${byDay.count} earlier`}</span>
          <svg class="chev" class:down={showEarlier} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
        </button>
        {#if showEarlier}
          {#each byDay.earlier as d (d.key)}
            <section class="day" data-testid="chat-agent-day" data-day={d.key} aria-label={d.label}>
              <h3 class="dayhead" data-testid="chat-agent-day-head">{d.label} · {d.rows.length}</h3>
              <ul class="rows compact">
                {#each d.rows as r (r.key)}
                  {@render row(r, 'compact')}
                {/each}
              </ul>
            </section>
          {/each}
        {/if}
      {/if}
      {@render dismissedFoot()}
    </section>
  {/if}

  {#if !groups.length}
    <!-- (A Code chat that works directly: its own Claude sends agents — helpers — when part of the work needs one.) -->
    <p class="pempty" data-testid="chat-agent-panel-empty">{talks ? 'No agents yet. Claude sends one when part of the work needs it.' : 'Nothing yet.'}</p>
  {/if}
  {#if !finished}{@render dismissedFoot()}{/if}
</div>

{#snippet dismissedFoot()}
  {#if dismissedCount}
    <!-- The rows dismissed: shown again (in Finished) or hidden, for this agent while the window stays open. -->
    <button type="button" class="dismissedbtn" data-testid="chat-agent-show-dismissed" aria-pressed={showDismissed} onclick={toggleDismissed}>
      {showDismissed ? 'Hide dismissed' : `Show dismissed · ${dismissedCount}`}
    </button>
  {/if}
{/snippet}

<!--
  **A row**, as its group shows it (`variant`): 'full' (Needs you, Queued: its Mode · model · effort, its line and
  stats, its card's buttons), 'running' (its clock ticking, what it's on, a thin live bar) or 'compact' (Finished:
  two lines, a small character, a ✓). Each opens its own view on a click, in place on ▸, its ⋯ menu, its helpers
  under it.
-->
{#snippet row(r, variant)}
  {@const isOpen = !!open[r.key]}
  {@const menu = hasMenu(r)}
  {@const c = variant === 'compact' ? compactOf(r) : null}
  {@const sub = [r.modeWord, r.modelWord, r.effortWord].filter(Boolean).join(' · ')}
  {@const sweep = variant === 'running' && r.progress.kind !== 'todos'}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (its right-click menu: the ⋯ button and the menu key are the keyboard's way) -->
  <li
    class="row v-{variant}"
    class:open={isOpen}
    class:menuopen={menuOpenFor(rowEls[r.key])}
    class:hasmenu={menu}
    class:dismissed={r.dismissed}
    bind:this={rowEls[r.key]}
    data-testid="chat-agent-row"
    data-key={r.key}
    data-kind={r.kind}
    data-turn={r.n}
    data-agent={r.agentId}
    data-state={r.state}
    data-status={r.status ?? ''}
    data-variant={variant}
    data-open={isOpen}
    data-unused={r.unused ? 'true' : null}
    data-dismissed={r.dismissed ? 'true' : null}
    oncontextmenu={(e) => menu && rowMenu(e, r)}
  >
    <button
      type="button"
      class="rhead"
      data-row-head
      aria-expanded={isOpen}
      aria-controls="more-{r.key}"
      onclick={(e) => headClick(e, r)}
      onkeydown={(e) => headKey(e, r)}
      use:overflowTip={{ text: r.title }}
    >
      <CrewAvatar
        character={crewOf(r)}
        prop={r.modeTone}
        pose={r.pose}
        size={variant === 'compact' ? 0.5 : 1}
        label={crewLabel(crewOf(r), r.pose, r.modeWord)}
        testid="chat-agent-row-avatar"
      />
      <span class="rmain">
        <span class="rtop">
          <span class="ctitle rtitle" data-testid="chat-agent-row-title">{r.title}</span>
          <span class="sr-only">, {r.statusWord}{hasView(r) ? '. Opens in its own tab; Space shows it here' : ''}</span>
          <!-- ▸: opens the row here, in place (a click elsewhere on it opens its own view). Its hit area a little
               bigger than it looks, nothing moved. -->
          <span class="rfold" data-fold data-testid="chat-agent-row-fold" title={hasView(r) ? (isOpen ? 'Fold it here' : 'Show it here') : undefined}>
            <svg class="chev rchev" class:down={isOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          </span>
          {#if variant === 'running'}
            <!-- Its clock, ticking (the Code teal: the side's own accent). -->
            <span class="rclock" data-testid="chat-agent-row-clock">{fmtClock(liveSecs(r, now))}</span>
          {:else if c?.mark}
            <span class="cmark mark-{c.mark}" data-testid="chat-agent-row-mark" aria-hidden="true">{c.mark === 'done' ? '✓' : '✕'}</span>
          {/if}
        </span>
        {#if variant === 'compact'}
          <span class="csub" title={sub}>
            <span data-testid="chat-agent-row-sub">{c.who}</span>{#if c.what}<span aria-hidden="true">{' · '}</span><span data-testid="chat-agent-row-line">{c.what}</span>{/if}{#if c.stats}<span aria-hidden="true">{' · '}</span><span data-testid="chat-agent-row-stats">{c.stats}</span>{/if}
          </span>
          {#if c.bar}
            <span class="rbar thin tone-{r.modeTone}" data-testid="chat-agent-row-bar" data-kind={r.progress.kind} data-value={Math.round(r.progress.value * 100)} aria-hidden="true"
              ><span class="fill" style:width={pct(r.progress.value)}></span></span
            >
          {/if}
        {:else if variant === 'running'}
          <span class="csub" title={sub}>
            <span data-testid="chat-agent-row-sub">{r.kind === 'helper' ? 'Helper' : r.modelWord || r.modeWord}</span><span aria-hidden="true">{' · '}</span><span class="rline" data-testid="chat-agent-row-line">{r.line}</span>
          </span>
          <span
            class="rbar thin tone-{r.modeTone}"
            class:sweep
            data-testid="chat-agent-row-bar"
            data-kind={sweep ? 'live' : r.progress.kind}
            data-value={sweep ? null : Math.round(r.progress.value * 100)}
            aria-hidden="true"><span class="fill" style:width={sweep ? null : pct(r.progress.value)}></span></span
          >
        {:else}
          <span class="rsub" data-testid="chat-agent-row-sub">
            <span class="mode tone-{r.modeTone}">{r.modeWord}</span>{[r.modelWord, r.effortWord]
              .filter(Boolean)
              .map((w) => ` · ${w}`)
              .join('')}
          </span>
          <span class="rthird">
            <span class="rline" data-testid="chat-agent-row-line">{r.line}</span>
            <span class="rstats" data-testid="chat-agent-row-stats">{statsLine({ ...r, secs: liveSecs(r, now) })}</span>
          </span>
          <span
            class="rbar kind-{r.progress.kind} tone-{r.modeTone}"
            data-testid="chat-agent-row-bar"
            data-kind={r.progress.kind}
            data-value={Math.round(r.progress.value * 100)}
            aria-hidden="true"
          ><span class="fill" style:width={pct(r.progress.value)}></span></span>
        {/if}
      </span>
    </button>
    {#if menu}
      <button
        type="button"
        class="more"
        title="More"
        aria-label={`More for ${r.title}`}
        aria-haspopup="menu"
        data-more
        data-testid="chat-agent-row-menu"
        onclick={(e) => rowMenu(e, r)}
      >
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
      </button>
    {/if}
    {#if r.needs}
      {@const busyRow = acting === r.key}
      <!-- What waits on the person, with its card's own buttons. -->
      <div class="rneeds" role="group" aria-label={`${r.title}: needs you`} data-testid="chat-agent-row-needs" data-kind={r.needs.kind}>
        {#if r.needs.kind === 'approve'}
          <p class="nwhat">Asks to: <strong data-testid="chat-agent-row-asks">{r.needs.approval.summary}</strong></p>
          {#if r.needs.approval.detail && r.needs.approval.detail.trim() !== (r.needs.approval.summary || '').trim()}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
            <pre class="ndetail" tabindex="0" aria-label="What exactly it would do">{r.needs.approval.detail}</pre>
          {/if}
          <div class="nbtns">
            <button type="button" class="nbtn primary" disabled={busyRow} onclick={() => answer(r, true)}>Allow once</button>
            <button type="button" class="nbtn" disabled={busyRow} onclick={() => answer(r, false)}>Deny</button>
          </div>
        {:else if r.needs.kind === 'question'}
          <!-- Its own question (Claude Code's AskUserQuestion): answered on its card in the chat, or left to it. -->
          <p class="nwhat">Asks you: <strong data-testid="chat-agent-row-asks">{r.needs.approval.summary}</strong></p>
          <p class="nwhat">Its options are on its card in the chat.</p>
          <div class="nbtns">
            <button type="button" class="nbtn" disabled={busyRow} onclick={() => answer(r, false)}>Let it decide</button>
          </div>
        {:else if r.needs.kind === 'plan'}
          <div class="nbtns">
            <button type="button" class="nbtn primary" disabled={busyRow} onclick={() => answer(r, true)}>Go ahead</button>
            <button type="button" class="nbtn" disabled={busyRow} onclick={() => answer(r, false)}>Not yet</button>
          </div>
        {:else if r.needs.kind === 'goahead'}
          <div class="nbtns">
            <button type="button" class="nbtn primary" disabled={busyRow} onclick={() => goAhead(r)}>Go ahead</button>
            <button type="button" class="nbtn" disabled={busyRow} onclick={() => notYet(r)}>Not yet</button>
          </div>
        {:else if r.needs.kind === 'confirm'}
          {#if r.request}
            <!-- The brief in full: it's seen before it goes (the safety rule). -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
            <div class="ndetail brief" tabindex="0" role="region" aria-label="The agent's brief" data-testid="chat-agent-row-brief">{r.request}</div>
          {/if}
          <div class="nbtns">
            <button type="button" class="nbtn primary" disabled={busyRow} onclick={() => confirm(r, true)}>Send</button>
            <button type="button" class="nbtn" disabled={busyRow} onclick={() => confirm(r, false)}>Don't</button>
          </div>
        {/if}
      </div>
    {/if}
    {#if r.helpers?.length}
      <!-- Its helpers (Claude Code's own subagents): an entry each, as Claude Code's app lists them. -->
      <ul class="helpers" aria-label={`${r.title}: its helpers`} data-testid="chat-agent-row-helpers">
        {#each r.helpers as h (h.key)}
          {@const hk = helperKey(r, h)}
          {@const hOpen = !!panelMemory.open[hk] && h.steps.length > 0}
          <li class="helper" data-testid="chat-agent-helper" data-status={h.status} data-open={hOpen ? 'true' : 'false'}>
            <button type="button" class="hhead" aria-expanded={h.steps.length ? hOpen : undefined} disabled={!h.steps.length} title={h.steps.length ? (hOpen ? 'Hide its steps' : 'Show its steps') : undefined} onclick={() => toggle(hk)}>
              <span class="hmark" aria-hidden="true">
                {#if h.status === 'working'}<span class="hspin"></span>{:else if h.status === 'failed'}✕{:else}✓{/if}
              </span>
              <span class="hmain">
                <span class="htitle" data-testid="chat-agent-helper-title">{h.title}</span>
                <span class="hline" data-testid="chat-agent-helper-line">{h.line}</span>
              </span>
              <span class="sr-only">, a helper, {h.status === 'working' ? 'working' : h.status === 'failed' ? "didn't finish" : 'done'}</span>
              {#if h.steps.length}
                <svg class="chev hchev" class:down={hOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
              {/if}
            </button>
            {#if hOpen}
              <div class="hsteps" data-testid="chat-agent-helper-steps">
                {#await import('./StepRows.svelte') then { default: StepRows }}
                  <StepRows steps={h.steps} busy={h.status === 'working'} onview={onpicture} />
                {/await}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if isOpen && r.kind === 'helper'}
      <!-- A helper's own steps, rows as the thread's (an edit to its diff, a command to what it printed). -->
      <div class="rmore" id="more-{r.key}" data-testid="chat-agent-row-more">
        {#if r.helper.steps.length}
          <div class="rwork" data-testid="chat-agent-row-steps">
            {#await import('./StepRows.svelte') then { default: StepRows }}
              <StepRows steps={r.helper.steps} busy={r.status === 'running'} onview={onpicture} />
            {/await}
          </div>
        {:else}
          <p class="rsizing">{r.steps ? `Its ${r.steps} steps aren't kept any more.` : 'No steps yet.'}</p>
        {/if}
      </div>
    {/if}
    {#if isOpen && r.kind === 'turn'}
      <div class="rmore" id="more-{r.key}" data-testid="chat-agent-row-more">
        {#if variant !== 'full'}
          <!-- (Its Mode, model and effort, the compact row's tooltip; its context and tokens.) -->
          <p class="rsizing" data-testid="chat-agent-row-detail">{[sub, statsLine({ ...r, secs: liveSecs(r, now) })].filter(Boolean).join(' · ')}</p>
        {/if}
        {#if r.sizing}
          <!-- Why Chat sized it so: "Opus 5.5 · High: a multi-file change". -->
          <p class="rsizing" data-testid="chat-agent-row-sizing">{[r.modelWord, r.effortWord].filter(Boolean).join(' · ')}: {r.sizing}</p>
        {/if}
        {#if r.todos.length}
          <ul class="todolist" data-testid="chat-agent-row-todos" aria-label="To-dos">
            {#each r.todos as t, i (i)}
              <li class="todo" class:done={t.status === 'completed'} class:doing={t.status === 'in_progress'} data-status={t.status}>
                <span class="tmark" aria-hidden="true">{todoMark(t.status)}</span>
                <span class="ttext">{t.content}</span>
                <span class="sr-only">{todoWord(t.status)}</span>
              </li>
            {/each}
          </ul>
        {/if}
        {#if r.events.length}
          <!-- Its work (12 Nov 2026): what it said, and a line for each stretch of steps that opens into
               rows, an edit's to its diff, a command's to itself and what it printed — as the thread shows
               a chat's own. An agent Chat sent out shows it here, never in the thread. -->
          <div class="rwork" data-testid="chat-agent-row-steps">
            {#await import('./AgentWork.svelte') then { default: AgentWork }}
              <AgentWork agent={agents.find((a) => a.id === r.agentId) ?? agent} turn={r.n} events={r.events} status={r.status} line={steps[r.agentId] || ''} wide onview={onpicture} />
            {/await}
          </div>
        {/if}
        <div class="rfoot">
          <span class="counts" data-testid="chat-agent-row-counts">{plural(r.commands, 'command', 'commands')} · {plural(r.files, 'file', 'files')} changed</span>
          {#if rowActions(r).goAhead}
            <!-- A plan nobody used: start it now, as a turn of its own. -->
            <button type="button" class="openbtn primary" data-testid="chat-agent-row-go-ahead" disabled={acting === r.key} onclick={() => goAhead(r)} aria-label={`Go ahead with the plan for ${r.title}`}>Go ahead</button>
          {/if}
          <button type="button" class="openbtn" data-testid="chat-agent-row-open" onclick={() => onopen?.(r)} aria-label={`Open the whole timeline of ${r.title}`}>Open</button>
        </div>
      </div>
    {/if}
  </li>
{/snippet}

<style>
  .apanel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .phead {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 6px;
  }
  .pmeta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .ppath {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  /* The Mode, coloured by what runs without asking. */
  .mode {
    font-weight: 650;
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
  .mode {
    color: var(--tone);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 6px;
    margin-top: 8px;
  }
  /* Compact tiles, as savvy's: the key over the value, 40 px tall. */
  .tile {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    min-width: 0;
    height: 40px;
    box-sizing: border-box;
    padding: 0 9px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .tk {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-text-2);
    font-size: 11px;
    line-height: 14px;
  }
  .tv {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-ink);
    font-size: 15px;
    font-weight: 650;
    line-height: 19px;
    font-variant-numeric: tabular-nums;
  }
  .tc {
    color: var(--yap-paper-muted);
    font-size: 10.5px;
  }
  .tc::before {
    content: '· ';
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .ghead {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    align-self: flex-start;
    padding: 4px 4px 4px 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .ghead:hover {
    color: var(--yap-paper-ink);
  }
  /* Finished's head across the row, "Collapse" / "Expand" at its right (read
     as the head's state, aria-expanded, not as more words). */
  .ghead.wide {
    align-self: stretch;
  }
  .ghead.wide::after {
    content: attr(data-act) / '';
    margin-left: auto;
    font-weight: 500;
  }
  .ghead:focus-visible,
  .rhead:focus-visible,
  .openbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    transition: transform 0.15s ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    position: relative;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .row:last-child {
    border-bottom: none;
  }
  /* Room for the row's ⋯ at its top right. */
  .row.hasmenu .rtop {
    padding-right: 24px;
  }
  /* A row's ⋯ (its menu): on a hover, a focus inside, or while its menu is up. */
  .more {
    position: absolute;
    top: 6px;
    right: 2px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
    opacity: 0;
  }
  .more svg {
    width: 15px;
    height: 15px;
  }
  .row:hover .more,
  .row:focus-within .more,
  .row.menuopen .more {
    opacity: 1;
  }
  .more:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .more:focus-visible {
    opacity: 1;
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* Its helpers: an entry each under the row's head, in line with its words. */
  .helpers {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: -4px 4px 8px 50px;
    padding: 0;
    list-style: none;
  }
  .helper {
    min-width: 0;
  }
  .hhead {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    box-sizing: border-box;
    width: 100%;
    margin: 0;
    padding: 3px 6px 3px 4px;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .hhead:disabled {
    cursor: default;
  }
  .hhead:hover:not(:disabled) {
    background: var(--yap-paper-s1);
  }
  .hhead:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .hmark {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 13px;
    height: 18px;
    color: var(--yap-success-text);
    font-size: 10.5px;
    font-weight: 700;
  }
  .helper[data-status='failed'] .hmark {
    color: var(--yap-danger);
  }
  .hspin {
    width: 9px;
    height: 9px;
    border: 1.5px solid color-mix(in srgb, var(--yap-code) 30%, transparent);
    border-top-color: var(--yap-code);
    border-radius: 50%;
    animation: helper-spin 0.9s linear infinite;
  }
  @keyframes helper-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(:root[data-motion='reduced']) .hspin {
    animation: none;
  }
  .hmain {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    min-width: 0;
  }
  .htitle {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .hline {
    overflow: hidden;
    color: var(--yap-paper-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hchev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    margin-top: 3px;
    opacity: 0.7;
  }
  .hsteps {
    margin: 2px 0 6px 20px;
  }
  /* What waits on the person, with its card's own buttons, under the row's head. */
  .rneeds {
    margin: -2px 4px 10px 50px;
    font-size: 12px;
  }
  .nwhat {
    margin: 0 0 6px;
    color: var(--yap-paper-ink);
    overflow-wrap: anywhere;
  }
  .nwhat strong {
    font-weight: 650;
  }
  .ndetail {
    max-height: 120px;
    margin: 0 0 8px;
    padding: 6px 9px;
    overflow: auto;
    border-radius: 7px;
    background: var(--yap-paper-s1);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .ndetail.brief {
    font-family: inherit;
    font-size: 12px;
    line-height: 1.45;
  }
  .ndetail:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .nbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .nbtn {
    height: 26px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .nbtn:hover {
    background: var(--yap-paper-s2);
  }
  .nbtn.primary,
  .openbtn.primary {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .nbtn.primary:hover,
  .openbtn.primary:hover {
    background: var(--yap-primary-hover);
  }
  .nbtn:disabled,
  .openbtn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .nbtn:focus-visible,
  .dismissedbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* "Show dismissed · 2" at Finished's foot. */
  .dismissedbtn {
    align-self: flex-start;
    margin: 4px 0 0;
    padding: 2px 4px 2px 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .dismissedbtn:hover {
    color: var(--yap-paper-ink);
  }
  .rhead {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    box-sizing: border-box;
    padding: 8px 4px 9px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .rhead:hover {
    background: var(--yap-paper-s1);
  }
  /* The row's character (CrewAvatar.svelte, crew.css): level with the title. */
  .rhead > :global(.crew) {
    margin-top: 1px;
  }
  .rmain {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .rtop {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .rtitle {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 650;
  }
  .rchev {
    color: var(--yap-paper-muted);
  }
  /* ▸'s hit area: 24 px round its 12, the row laid out as before (the margin gives the padding back). */
  .rfold {
    flex: 0 0 auto;
    display: inline-flex;
    margin: -6px;
    padding: 6px;
  }
  .rsub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-text-2);
    font-size: 11px;
  }
  .rthird {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: 10px;
    min-width: 0;
    font-size: 11px;
  }
  .rline {
    flex: 1 1 120px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-ink);
    font-variant-numeric: tabular-nums;
  }
  .rstats {
    flex: 0 0 auto;
    margin-left: auto;
    color: var(--yap-paper-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .rbar {
    display: block;
    height: 4px;
    margin-top: 5px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--yap-paper-s2);
  }
  .rbar .fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: var(--tone);
  }
  .rbar.kind-context .fill {
    background: var(--yap-paper-muted);
  }
  /* ---- The tidy rows (18 Nov 2026): Running's, and Finished's in two lines ---- */
  .ghead.static {
    cursor: default;
    margin: 0;
  }
  .ghead.static:hover {
    color: var(--yap-paper-muted);
  }
  .csub {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
    font-size: 11px;
    line-height: 15px;
    font-variant-numeric: tabular-nums;
  }
  .row.v-running .csub {
    color: var(--yap-paper-text-2);
  }
  .row.v-running .csub .rline {
    color: var(--yap-paper-ink);
  }
  /* A running row's clock, ticking: the Code side's own accent. */
  .rclock {
    flex: 0 0 auto;
    margin-left: auto;
    color: var(--yap-code);
    font-size: 11.5px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }
  .row.v-running.hasmenu .rtop,
  .row.v-compact.hasmenu .rtop {
    padding-right: 22px;
  }
  /* A thin bar: 2 px, as the Claude app's agents panel. */
  .rbar.thin {
    height: 2px;
    margin-top: 4px;
    border-radius: 1px;
  }
  .rbar.thin .fill {
    border-radius: 1px;
  }
  /* No list to count: a sweep, while it works (still when the crew is still). */
  .rbar.sweep {
    position: relative;
  }
  .rbar.sweep .fill {
    position: absolute;
    left: 0;
    width: 30%;
    animation: panel-sweep 1.6s ease-in-out infinite;
  }
  @keyframes panel-sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }
  .apanel[data-crew-paused='true'] .rbar.sweep .fill {
    animation-play-state: paused;
  }
  :global(:root[data-motion='reduced']) .rbar.sweep .fill {
    animation: none;
    width: 100%;
    opacity: 0.35;
  }
  /* Finished's rows: two lines, a small character. */
  .row.v-compact .rhead {
    gap: 8px;
    padding: 6px 4px 7px;
  }
  .row.v-compact .rhead > :global(.crew) {
    margin-top: 0;
  }
  /* (Its ✓ is at the row's end, not on the little character.) */
  .row.v-compact .rhead > :global(.crew .badge) {
    display: none;
  }
  .row.v-compact .rmain {
    gap: 1px;
  }
  .row.v-compact .rtitle {
    font-size: 12.5px;
    font-weight: 600;
    line-height: 16px;
  }
  .row.v-compact .more {
    top: 3px;
  }
  /* (Its ▸ on a hover or a focus, or while it's open: the ✓ is the row's end, as the Claude app's.) */
  .row.v-compact .rfold {
    opacity: 0;
  }
  .row.v-compact:hover .rfold,
  .row.v-compact:focus-within .rfold,
  .row.v-compact.open .rfold {
    opacity: 1;
  }
  .row.v-compact .helpers,
  .row.v-compact .rmore {
    margin-left: 30px;
  }
  .row.v-compact .helpers {
    margin-top: -2px;
  }
  .row.dismissed .rmain {
    opacity: 0.65;
  }
  .cmark {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: 11px;
    font-weight: 700;
    line-height: 16px;
  }
  .cmark.mark-done {
    color: var(--yap-success-text);
  }
  .cmark.mark-failed {
    color: var(--yap-danger);
  }
  /* "Show 89 earlier ›": one quiet line; the days under it. */
  .earlierbtn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
    margin: 4px 0 0;
    padding: 3px 4px 3px 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .earlierbtn:hover {
    color: var(--yap-paper-ink);
  }
  .earlierbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .day {
    display: flex;
    flex-direction: column;
    margin-top: 6px;
  }
  .dayhead {
    margin: 0;
    padding: 4px 0 2px;
    border-bottom: 1px solid var(--yap-paper-rule);
    color: var(--yap-paper-text-2);
    font-size: 11.5px;
    font-weight: 650;
  }
  .rmore {
    margin: 0 4px 10px 50px;
    font-size: 12px;
  }
  .todolist {
    margin: 0 0 8px;
    padding: 0;
    list-style: none;
  }
  .rwork {
    margin: 0 0 8px;
  }
  .todo {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    padding: 1px 0;
    color: var(--yap-paper-ink);
  }
  .tmark {
    flex: 0 0 auto;
    width: 13px;
    text-align: center;
    color: var(--yap-paper-muted);
    font-size: 10.5px;
    line-height: 18px;
  }
  .todo.done .tmark {
    color: var(--yap-success-text);
    font-weight: 700;
  }
  .todo.done .ttext {
    color: var(--yap-paper-muted);
  }
  .todo.doing .tmark {
    color: var(--yap-code);
  }
  .todo.doing .ttext {
    font-weight: 600;
  }
  .ttext {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .rfoot {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .rsizing {
    margin: 0 0 8px;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .counts {
    flex: 1 1 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .openbtn {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .openbtn:hover {
    background: var(--yap-paper-s2);
  }
  .pempty {
    margin: 8px 0;
    color: var(--yap-paper-muted);
    font-size: 12.5px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
