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
  //   place: its to-do list, its latest steps, how many commands and files,
  //   and **Open** — the turn's whole timeline (the tab's other view). Enter
  //   or Space opens and folds; ↑ ↓ move between rows. Which rows are open
  //   and which groups folded is remembered per agent while the window stays
  //   open (agentPanelMemory.svelte.js); a pill or a report card opens its own.
  // - **The crew moves only on screen**: paused while the window is hidden
  //   or minimised (stores/windowShown.svelte.js) or the panel is out of
  //   view; still under reduced motion (crew.css).
  // Live from the `yap-agent` events the tab and stores/agents.svelte.js
  // hear: nothing polled.
  import { onMount, tick, untrack } from 'svelte';
  import { panelRows, panelGroups, panelTotals, statsLine, fmtTokens, fmtClock, costWords, latestSteps, modeTone } from '../../core/agentPanel.js';
  import { modeLabel, modelLabel, costLine } from '../../core/agents.js';
  import { effortLabel } from '../../core/effort.js';
  import { characterFor, crewLabel } from '../../core/crew.js';
  import { overflowTip } from '../../ui/overflowTip.js';
  import { windowShown, followWindowShown } from '../../stores/windowShown.svelte.js';
  import { panelMemory } from './agentPanelMemory.svelte.js';
  import CrewAvatar from './CrewAvatar.svelte';

  /**
   * `agent`: the tab's agent as it is now; `agents`: the Code chat's agents,
   * the tab's first; `logs`: agent id → its log; `steps`: agent id → the line
   * it's on; `now`: unix seconds (the clock); `want`: `{key, seq}`, a row to
   * open and show (a pill's, a report card's); `onopen(row)`: Open.
   */
  let { agent, agents = [], logs = {}, steps = {}, now = Date.now() / 1000, want = null, onopen = null } = $props();

  const rows = $derived(panelRows(agents, { logs, steps, now }));
  const groups = $derived(panelGroups(rows));
  const totals = $derived(panelTotals(rows));
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
            {@const steps8 = isOpen ? latestSteps(r.events) : []}
            <li class="row" class:open={isOpen} data-testid="chat-agent-row" data-key={r.key} data-turn={r.n} data-agent={r.agentId} data-state={r.state} data-status={r.status ?? ''} data-open={isOpen}>
              <button
                type="button"
                class="rhead"
                data-row-head
                aria-expanded={isOpen}
                aria-controls="more-{r.key}"
                onclick={() => toggle(r.key)}
                onkeydown={nav}
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
              {#if isOpen}
                <div class="rmore" id="more-{r.key}" data-testid="chat-agent-row-more">
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
                  {#if steps8.length}
                    <ol class="steps" data-testid="chat-agent-row-steps" aria-label="Latest steps">
                      {#each steps8 as s, i (i)}<li>{s.step}</li>{/each}
                    </ol>
                  {/if}
                  <div class="rfoot">
                    <span class="counts" data-testid="chat-agent-row-counts">{plural(r.commands, 'command', 'commands')} · {plural(r.files, 'file', 'files')} changed</span>
                    <button type="button" class="openbtn" data-testid="chat-agent-row-open" onclick={() => onopen?.(r)} aria-label={`Open the whole timeline of ${r.title}`}>Open</button>
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {:else}
    <p class="pempty">Nothing yet.</p>
  {/each}
</div>

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
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .row:last-child {
    border-bottom: none;
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
  .todolist,
  .steps {
    margin: 0 0 8px;
    padding: 0;
    list-style: none;
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
  .steps li {
    position: relative;
    padding: 1px 0 1px 13px;
    color: var(--yap-paper-text-2);
    overflow-wrap: anywhere;
  }
  .steps li::before {
    content: '';
    position: absolute;
    left: 3px;
    top: 8px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--yap-paper-muted);
  }
  .rfoot {
    display: flex;
    align-items: center;
    gap: 8px;
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
