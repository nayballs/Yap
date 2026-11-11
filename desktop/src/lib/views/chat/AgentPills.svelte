<script>
  // The agents an answer started or carried on (Yap Code: chat/tools/agents.rs
  // `start_agent`, `message_agent`): a pill each under it — the folder, that
  // turn's request once the agent has several, and a status dot; a click
  // opens its Agent tab beside the chat at that turn (one tab an agent). Found
  // by the question each turn keeps (`messageId`, core/agents.js
  // `pillsByAnswer`). Its own small chunk (ChatThread loads it for an
  // answer that has some).
  import { statusWord, statusTone, turnStatusWord } from '../../core/agents.js';

  /** `agents`: the answer's agents; `onopen(agent)` opens one's tab. */
  /** `quiet`: a Code chat's thread — a plain line, no box and no "Open" (as Claude Code's transcript has none: 11 Nov 2026). */
  let { agents = [], onopen = null, quiet = false } = $props();
</script>

<div class="agentpills" role="group" aria-label="Agents">
  {#each agents as j (`${j.id}:${j.turn ?? 1}`)}
    {@const status = j.turnStatus ?? j.status}
    {@const several = (j.turnList?.length ?? 0) > 1}
    {@const word = several && j.turnStatus ? turnStatusWord({ status: j.turnStatus, startedTs: j.turnStarted }) : statusWord(status)}
    <button
      type="button"
      class="agentpill"
      class:quiet
      data-testid="chat-agent-pill"
      data-status={status}
      data-turn={j.turn ?? 1}
      onclick={() => onopen?.(j)}
      aria-label={`Open the agent in ${j.folder}${several && j.turnTitle ? `, at ${j.turnTitle}` : ''}: ${word}`}
      title={j.path || j.folder}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
      <span class="jname" class:alone={!(several && j.turnTitle)}>{j.folder}</span>
      {#if several && j.turnTitle}<span class="jturn">· {j.turnTitle}</span>{/if}
      <span class="jstate tone-{statusTone(status)}"><span class="dot" aria-hidden="true"></span>{word}</span>
      <span class="jopen" aria-hidden="true">· Open</span>
    </button>
  {/each}
</div>

<style>
  .agentpills {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-width: 78%;
  }
  /* PagePills' look. */
  .agentpill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    box-sizing: border-box;
    max-width: min(360px, 100%);
    height: 32px;
    padding: 0 12px 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .agentpill:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  /* A Code chat's: a quiet line of the thread. */
  .agentpill.quiet {
    height: auto;
    /* (In line with a bubble's words.) */
    padding: 1px 4px 1px 13px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    font-weight: 500;
  }
  .agentpill.quiet:hover {
    background: none;
    color: var(--yap-fg-80);
  }
  .agentpill.quiet svg {
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
  }
  .agentpill.quiet .jopen {
    display: none;
  }
  .agentpill:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .agentpill svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  /* Beside a turn's title the folder's name never gives way (up to most of
     the pill): the title does ("lab-san… · Add tablespoons line to RE…",
     11 Nov 2026, cut both — and a name shrunk by a fraction of a pixel
     still loses its last letters to the ellipsis). Alone, it has the pill. */
  .jname {
    flex: 0 0 auto;
    max-width: 60%;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .jname.alone {
    flex: 0 1 auto;
    max-width: none;
  }
  .jturn {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--yap-fg-80);
  }
  .jstate {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: 0 0 auto;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-muted);
  }
  .tone-live .dot {
    background: var(--yap-primary);
  }
  .tone-ask .dot {
    background: var(--yap-warning);
  }
  .tone-done .dot {
    background: var(--yap-success);
  }
  .tone-failed .dot {
    background: var(--yap-danger);
  }
  .jopen {
    flex: 0 0 auto;
    color: var(--yap-primary);
  }
</style>
