<script>
  // **An agent's report in the thread** (Yap Code: chat/turn/report.rs): the
  // marker message (`kind: "agentReport"`) is the agent's facts and its own
  // summary, which the chat's model reads as the agent's (never the person's)
  // and answers in its own words right after. Drawn as a compact agent card,
  // not a question: "Agent done · garden-planner" (or didn't finish, has a
  // question, its plan is ready), what it changed in a line, and Open, the
  // agent's tab at that report's turn (an agent reports each turn: one tab an
  // agent). A plan waiting for its go-ahead has **Go ahead** too: the
  // person's click carries it out (`agent_go_ahead`, Yap's own words at the
  // chat's Mode), once — disabled as soon as it's pressed. Its own small
  // chunk.
  import { reportTitle, reportFolder, agentOfReport, turnOfReport, statusTone, planWaits } from '../../core/agents.js';
  import { agents, goAheadWithPlan } from '../../stores/agents.svelte.js';

  /**
   * `m`: the marker message (its `id` "chat:<conv>:<i>", its `text`, the
   * pictures and new files its turn made: `files`, chat/shares.rs);
   * `conversation`: the chat's id; `onopen(agent)`; `room`: the thread's
   * width (CSS px) and `onpicture(p)` for those pictures.
   */
  let { m, conversation = null, onopen = null, room = 0, onpicture = null } = $props();

  const folder = $derived(reportFolder(m.text));
  const agent = $derived(agentOfReport(agents.list, m.id, conversation, folder));
  // The turn it reports: said as that turn is (each has its own report);
  // an agent from before turns, its latest report as the agent is.
  const turn = $derived(turnOfReport(agent, m.id));
  const latest = $derived(!!agent && agent.reportMessageId === m.id);
  const as = $derived(turn ?? (latest ? agent : null));
  const title = $derived(reportTitle(as, m.text));
  const tone = $derived(as ? statusTone(as.status) : /didn't finish/.test(title) ? 'failed' : 'done');
  // What it changed, in a line (the marker's own "Files it changed: …").
  const changed = $derived((/^Files it changed: (.+)$/m.exec(m.text || '')?.[1] ?? '').replace(/\s*\(backed up first\)\.?$/, ''));
  // A plan still waiting for its go-ahead, or one this card sent on (kept
  // in the store: used once, even if the thread draws the card again).
  let failed = $state('');
  const waits = $derived(planWaits(agent, turn, latest));
  const sent = $derived(!!agent && agent.id in agents.wentAhead && agents.wentAhead[agent.id] === (turn?.n ?? 0));

  async function goAhead() {
    if (sent || !agent) return;
    failed = '';
    try {
      await goAheadWithPlan(agent.id, turn?.n ?? 0);
    } catch (e) {
      failed = String(e?.message ?? e ?? '') || "Couldn't go ahead.";
    }
  }
</script>

<div class="agentreport" role="note" aria-label={`${title} · ${agent?.folder || folder}`} data-testid="chat-agent-report" data-msg-id={m.id || undefined} data-turn={turn?.n ?? undefined}>
  <span class="jdot tone-{tone}" aria-hidden="true"></span>
  <div class="jtext">
    <p class="jtitle">{title} · <span class="jfolder">{agent?.folder || folder || 'a folder'}</span></p>
    {#if changed}<p class="jline">Changed {changed}</p>{/if}
    {#if failed}<p class="jline jfail" role="alert">{failed}</p>{/if}
  </div>
  {#if agent && (waits || sent)}
    <button type="button" class="jgo" disabled={sent} onclick={goAhead} aria-label={sent ? `Going ahead with the plan in ${agent.folder}` : `Go ahead with the plan in ${agent.folder}`} data-testid="chat-agent-go-ahead">
      {sent ? 'Going ahead' : 'Go ahead'}
    </button>
  {/if}
  {#if agent}
    <button type="button" class="jopen" onclick={() => onopen?.({ ...agent, turn: turn?.n ?? null })} aria-label={`Open the agent in ${agent.folder}`}>Open</button>
  {/if}
</div>
{#if m.files?.length}
  <!-- Its turn's work, shown: the pictures it made or changed (up to four,
       then "+N more"), its new files as compact cards (chat/shares.rs). -->
  {#await import('./SharedFiles.svelte') then { default: SharedFiles }}
    <SharedFiles shares={m.files} {conversation} {room} {onpicture} report />
  {/await}
{/if}

<style>
  .agentreport {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    max-width: min(520px, 78%);
    padding: 8px 10px 8px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 12.5px;
  }
  .jdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .jdot.tone-failed {
    background: var(--yap-danger);
  }
  .jdot.tone-live {
    background: var(--yap-primary);
  }
  .jdot.tone-ask {
    background: var(--yap-warning);
  }
  .jdot.tone-idle {
    background: var(--yap-muted);
  }
  .jtext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .jtitle,
  .jline {
    margin: 0;
  }
  .jtitle {
    font-weight: 600;
  }
  .jfolder {
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .jline {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .jopen {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .jopen:hover {
    border-color: var(--yap-border-hover);
  }
  .jopen:focus-visible,
  .jgo:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .jfail {
    color: var(--yap-danger);
    white-space: normal;
  }
  /* A plan's go-ahead: the card's one filled button. */
  .jgo {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-full);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .jgo:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .jgo:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
