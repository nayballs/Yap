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
  //   and **Open** — the turn's whole timeline (the tab's other view). Enter
  //   or Space opens and folds; ↑ ↓ move between rows. Which rows are open
  //   and which groups folded is remembered per agent while the window stays
  //   open (agentPanelMemory.svelte.js); a pill or a report card opens its own.
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
  // Live from the `yap-agent` events the tab and stores/agents.svelte.js
  // hear: nothing polled.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick, untrack } from 'svelte';
  import { panelRows, panelGroups, panelTotals, statsLine, fmtTokens, fmtClock, costWords, modeTone, rowActions } from '../../core/agentPanel.js';
  import { modeLabel, modelLabel, costLine } from '../../core/agents.js';
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
   * `onopen(row)`: Open.
   */
  let { agent, agents = [], logs = {}, steps = {}, approvals = [], cards = [], now = Date.now() / 1000, want = null, onopen = null, onpicture = null } = $props();

  /** Every row, the dismissed ones too (the totals count them); the ones shown. */
  const every = $derived(panelRows(agents, { logs, steps, now, approvals, cards, dismissed: true }));
  const showDismissed = $derived(!!panelMemory.dismissed[agent?.id ?? '']);
  const dismissedCount = $derived(every.filter((r) => r.dismissed).length);
  const rows = $derived(showDismissed ? every : every.filter((r) => !r.dismissed));
  const groups = $derived(panelGroups(rows));
  const totals = $derived(panelTotals(every.filter((r) => r.kind === 'turn')));
  const cost = $derived(costWords(totals.cost, { pay: agent?.pay, estimated: totals.approx && agent?.pay === 'key' }));

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
      const g = rows.find((r) => r.key === key)?.state;
      if (g && isFolded(g)) setFolded(g, false);
      await tick();
      rootEl?.querySelector(`[data-testid="chat-agent-row"][data-key="${CSS.escape(key)}"]`)?.scrollIntoView?.({ block: 'nearest' });
    });
  });

  const toggle = (key) => (panelMemory.open = { ...panelMemory.open, [key]: !panelMemory.open[key] });
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
            body: `The agent in ${r.agent?.folder || 'this workspace'} stops where it is. Continue carries it on later.`,
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

  /** A row's head's keys: ↑ ↓ between rows, the menu key opens its menu. */
  function headKey(e, r) {
    if (menuKey(e, (ev) => rowMenu(ev, r))) return;
    nav(e);
  }

  const toggleDismissed = () => (panelMemory.dismissed = { ...panelMemory.dismissed, [agent?.id ?? '']: !showDismissed });

  const pct = (v) => `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%`;
  const todoMark = (s) => (s === 'completed' ? '✓' : s === 'in_progress' ? '●' : '○');
  const todoWord = (s) => (s === 'completed' ? ', done' : s === 'in_progress' ? ', in progress' : ', to do');
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
        <span class="tv">{fmtClock(totals.secs)}</span>
      </div>
    </div>
  </header>

  {#each groups as g (g.id)}
    {@const shut = isFolded(g.id)}
    <section class="group" data-testid="chat-agent-group" data-group={g.id} aria-label={g.label}>
      <button
        type="button"
        class="ghead"
        class:wide={g.id === 'finished'}
        data-act={g.id === 'finished' ? (shut ? 'Expand' : 'Collapse') : null}
        aria-expanded={!shut}
        onclick={() => setFolded(g.id, !shut)}
        data-testid="chat-agent-group-head"
      >
        <svg class="chev" class:down={!shut} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
        <span>{g.label} · {g.rows.length}</span>
      </button>
      {#if !shut}
        <ul class="rows">
          {#each g.rows as r (r.key)}
            {@const isOpen = !!open[r.key]}
            {@const menu = hasMenu(r)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions (its right-click menu: the ⋯ button and the menu key are the keyboard's way) -->
            <li
              class="row"
              class:open={isOpen}
              class:menuopen={menuOpenFor(rowEls[r.key])}
              class:hasmenu={menu}
              bind:this={rowEls[r.key]}
              data-testid="chat-agent-row"
              data-key={r.key}
              data-kind={r.kind}
              data-turn={r.n}
              data-agent={r.agentId}
              data-state={r.state}
              data-status={r.status ?? ''}
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
                onclick={() => toggle(r.key)}
                onkeydown={(e) => headKey(e, r)}
                use:overflowTip={{ text: r.title }}
              >
                <CrewAvatar
                  character={crewOf(r)}
                  prop={r.modeTone}
                  pose={r.pose}
                  label={crewLabel(crewOf(r), r.pose, r.modeWord)}
                  testid="chat-agent-row-avatar"
                />
                <span class="rmain">
                  <span class="rtop">
                    <span class="ctitle rtitle" data-testid="chat-agent-row-title">{r.title}</span>
                    <span class="sr-only">, {r.statusWord}</span>
                    <svg class="chev rchev" class:down={isOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
                  </span>
                  <span class="rsub" data-testid="chat-agent-row-sub">
                    <span class="mode tone-{r.modeTone}">{r.modeWord}</span>{[r.modelWord, r.effortWord]
                      .filter(Boolean)
                      .map((w) => ` · ${w}`)
                      .join('')}
                  </span>
                  <span class="rthird">
                    <span class="rline" data-testid="chat-agent-row-line">{r.line}</span>
                    <span class="rstats" data-testid="chat-agent-row-stats">{statsLine(r)}</span>
                  </span>
                  <span
                    class="rbar kind-{r.progress.kind} tone-{r.modeTone}"
                    data-testid="chat-agent-row-bar"
                    data-kind={r.progress.kind}
                    data-value={Math.round(r.progress.value * 100)}
                    aria-hidden="true"
                  ><span class="fill" style:width={pct(r.progress.value)}></span></span>
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
              {#if isOpen && r.kind === 'turn'}
                <div class="rmore" id="more-{r.key}" data-testid="chat-agent-row-more">
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
          {/each}
        </ul>
        {#if g.id === 'finished'}{@render dismissedFoot()}{/if}
      {/if}
    </section>
  {:else}
    <p class="pempty">Nothing yet.</p>
  {/each}
  {#if !groups.some((g) => g.id === 'finished')}{@render dismissedFoot()}{/if}
</div>

{#snippet dismissedFoot()}
  {#if dismissedCount}
    <!-- The rows dismissed: shown again (in Finished) or hidden, for this agent while the window stays open. -->
    <button type="button" class="dismissedbtn" data-testid="chat-agent-show-dismissed" aria-pressed={showDismissed} onclick={toggleDismissed}>
      {showDismissed ? 'Hide dismissed' : `Show dismissed · ${dismissedCount}`}
    </button>
  {/if}
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
