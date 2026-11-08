<script>
  // **One turn of an agent**, in its tab (AgentTab.svelte; core/agents.js
  // `turnsOf`): one request to the person's Claude Code and what came of it
  // — the line it's on while it works, its to-do list, its timeline (every
  // step and what it said between them: AgentTimeline.svelte), the files it
  // changed with their diffs (AgentFiles.svelte) and its own Undo, the
  // commands it ran (a few, then "Show N more"), what it wasn't allowed to
  // do, and its final words (markdown). An agent is one Claude Code session per
  // Code chat: each follow-up is a turn of its own (agents/runner.rs).
  import { renderMarkdown } from '../../core/markdown.js';
  import { timelineOf, latestStep, todosOf, todoCount, diffsOf, commandsOf, NO_BACKUPS } from '../../core/agents.js';
  import AgentTimeline from './AgentTimeline.svelte';
  import AgentFiles from './AgentFiles.svelte';

  /**
   * `agent`: the agent as it is now; `turn`: this turn (`turnsOf`, its `events`);
   * `live`: this turn is working now; `single`: the agent's only turn (an
   * older agent's commands from the agent); `undoable`: Undo this turn offered;
   * `undone`: what Undo this turn put back (its line); `refused`: why it
   * couldn't; `busy`; `onundo()`.
   */
  let { agent, turn, live = false, single = false, undoable = false, undone = '', refused = '', busy = false, onundo = null } = $props();

  let allCommands = $state(false);
  /** Commands shown before "Show N more". */
  const FEW_COMMANDS = 3;

  const events = $derived(turn?.events || []);
  // Its steps and what it said, its final words apart.
  const timeline = $derived(timelineOf(events, live ? '' : turn?.text));
  const stepCount = $derived(timeline.filter((t) => t.kind === 'step').length);
  // The line it's on now: its latest step, or the to-do in progress.
  const latest = $derived(live ? latestStep(events, agent?.status) : '');
  const todos = $derived(todosOf(events));
  const diffs = $derived(diffsOf(events));
  // The commands it ran, each with what it was for (its own events'; an
  // older agent's swept log: the agent's own list).
  const commands = $derived.by(() => {
    const told = events.filter((e) => typeof e?.command === 'string');
    if (told.length || !single) return commandsOf({ commands: told.map((e) => e.command) }, told);
    return commandsOf(agent, events);
  });
  const shownCommands = $derived(allCommands ? commands : commands.slice(0, FEW_COMMANDS));
</script>

{#if latest}
  <!-- What it's doing now. -->
  <p class="now" data-testid="chat-agent-now" aria-live="polite"><span class="spin" aria-hidden="true"></span>{latest}</p>
{/if}

{#if todos.length}
  <!-- Claude Code's to-do list, as this turn's latest TodoWrite left it. -->
  <div class="section todos" data-testid="chat-agent-todos">
    <h3>To-do <span class="count">{todoCount(todos)}</span></h3>
    <ul class="todolist">
      {#each todos as t, i (i)}
        <li class="todo" class:done={t.status === 'completed'} class:doing={t.status === 'in_progress'} data-status={t.status}>
          <span class="tmark" aria-hidden="true">{t.status === 'completed' ? '✓' : t.status === 'in_progress' ? '●' : '○'}</span>
          <span class="ttext">{t.content}</span>
          <span class="sr-only">{t.status === 'completed' ? ', done' : t.status === 'in_progress' ? ', in progress' : ', to do'}</span>
        </li>
      {/each}
    </ul>
  </div>
{/if}

{#if timeline.length}
  <details class="section" open>
    <summary>Steps <span class="count">{stepCount}</span></summary>
    <AgentTimeline items={timeline} {live} />
  </details>
{/if}

{#if turn?.files?.length}
  <div class="section" data-testid="chat-agent-files">
    <h3>Files changed <span class="count">{turn.files.length}</span></h3>
    <AgentFiles files={turn.files} {diffs} />
    {#if agent?.noBackups}
      <p class="nobackups" data-testid="chat-agent-no-backups">{NO_BACKUPS}</p>
    {/if}
    {#if undoable}
      <button type="button" class="undoturn" data-testid="chat-agent-undo-turn" onclick={() => onundo?.()} disabled={busy}>Undo this turn</button>
    {/if}
    {#if refused}
      <p class="refused" role="alert" data-testid="chat-agent-turn-refused">{refused}</p>
    {/if}
  </div>
{/if}
{#if commands.length}
  <div class="section" data-testid="chat-agent-commands">
    <h3>Commands run <span class="count">{commands.length}</span></h3>
    <ul class="cmds">
      {#each shownCommands as c, i (i)}
        <li data-testid="chat-agent-command">
          {#if c.about}<p class="cabout">{c.about}</p>{/if}
          <code class="cmd">{c.command}</code>
        </li>
      {/each}
    </ul>
    {#if commands.length > FEW_COMMANDS}
      <button type="button" class="showmore" aria-expanded={allCommands} onclick={() => (allCommands = !allCommands)}>
        {allCommands ? 'Show fewer' : `Show ${commands.length - FEW_COMMANDS} more`}
      </button>
    {/if}
  </div>
{/if}
{#if turn?.denials?.length}
  <div class="section" data-testid="chat-agent-denials">
    <h3>Not allowed</h3>
    <ul class="plain">
      {#each turn.denials as d, i (i)}<li>{d.detail || d.tool}</li>{/each}
    </ul>
  </div>
{/if}

{#if !live && turn?.text}
  <div class="section final" data-testid="chat-agent-text">
    <h3>{turn.plan ? 'Its plan' : 'What it said'}</h3>
    <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
    <div class="md">{@html renderMarkdown(turn.text)}</div>
  </div>
{/if}

{#if undone}
  <p class="undone" aria-live="polite" data-testid="chat-agent-undone">{undone}</p>
{:else if turn?.undone}
  <p class="undone">Undone: its files are back as they were.</p>
{/if}

<style>
  .now {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0 0;
    padding: 8px 10px;
    border-radius: 9px;
    background: var(--yap-paper-s1);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .spin {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
    border: 2px solid color-mix(in srgb, var(--yap-primary) 30%, transparent);
    border-top-color: var(--yap-primary);
    border-radius: 50%;
  }
  @media (prefers-reduced-motion: no-preference) {
    .spin {
      animation: spin 0.9s linear infinite;
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .section {
    margin: 14px 0 0;
  }
  .section > summary,
  .section > h3 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 650;
    color: var(--yap-paper-text-2);
    cursor: default;
  }
  .section > summary {
    cursor: pointer;
  }
  .count {
    margin-left: 4px;
    font-weight: 500;
    color: var(--yap-paper-muted);
  }
  .todolist {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .todo {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 2px 0;
    font-size: 12.5px;
    color: var(--yap-paper-ink);
  }
  .tmark {
    flex: 0 0 auto;
    width: 14px;
    text-align: center;
    color: var(--yap-paper-muted);
    font-size: 11px;
    line-height: 19px;
  }
  .todo.done .tmark {
    color: var(--yap-success-text);
    font-weight: 700;
  }
  .todo.done .ttext {
    color: var(--yap-paper-muted);
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--yap-paper-muted) 60%, transparent);
  }
  .todo.doing .tmark {
    color: var(--yap-primary);
  }
  .todo.doing .ttext {
    font-weight: 600;
  }
  .ttext {
    min-width: 0;
    overflow-wrap: anywhere;
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
  .plain {
    margin: 0;
    padding: 0 0 0 2px;
    list-style: none;
  }
  .plain li {
    padding: 2px 0;
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .cmds {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .cmds li {
    padding: 3px 0;
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .cmds li + li {
    border-top: 1px solid color-mix(in srgb, var(--yap-paper-rule) 60%, transparent);
  }
  .cabout {
    margin: 0 0 2px;
    color: var(--yap-paper-text-2);
    font-size: 12px;
  }
  code {
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--yap-paper-s1);
  }
  code.cmd {
    display: block;
    padding: 4px 7px;
    white-space: pre-wrap;
  }
  .showmore {
    margin: 4px 0 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .showmore:hover {
    text-decoration: underline;
  }
  .showmore:focus-visible,
  .undoturn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .final .md {
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: 1.6;
    overflow-wrap: anywhere;
  }
  .final .md :global(p) {
    margin: 0 0 8px;
  }
  .nobackups {
    margin: 8px 0 0;
    padding: 7px 10px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--yap-warning) 10%, var(--yap-paper));
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .undoturn {
    margin: 8px 0 0;
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .undoturn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .undoturn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .refused {
    margin: 8px 0 0;
    padding: 7px 10px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--yap-warning) 12%, var(--yap-paper));
    color: var(--yap-paper-ink);
    font-size: 12.5px;
  }
  .undone {
    margin: 12px 0 0;
    padding: 8px 10px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--yap-success) 9%, var(--yap-paper));
    font-size: 12.5px;
  }
</style>
