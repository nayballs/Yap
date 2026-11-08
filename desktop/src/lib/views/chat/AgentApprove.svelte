<script>
  // **A running agent asks to take a step** (Yap Code: the run endpoint's
  // `approve`, integrations/claude_live/approve.rs): its card in the thread
  // of the chat that started the agent — the tool, what it would do in plain
  // words ("Run a command: npm test"), the command or edit itself, and
  // **Allow once** / **Deny** (`claude_approve_answer`). Gone once it's
  // answered here, on the Yap bar, or no longer waiting
  // (`yap-claude-approve-done`). Never answered by voice. Its own small
  // chunk.
  //
  // **A plan** (Plan mode's ExitPlanMode, 2026-11-07) reads as one: "The
  // plan is ready", the plan itself in markdown (folded when long, Show
  // more), and **Go ahead** / **Not yet**. Go ahead allows it, and the agent
  // carries the plan out at the chat's Mode once its turn ends
  // (agents/runner.rs); Not yet keeps it a plan.
  import { answerApproval } from '../../stores/agents.svelte.js';
  import { renderMarkdown } from '../../core/markdown.js';
  import { longSaid } from '../../core/agents.js';

  /**
   * `approvals`: this chat's prompts waiting (`Pending`), oldest first;
   * `answered`: the ones a paired phone answered (Remote Control,
   * `agents.remoteAnswers`), each closed with who; `onopen(run)` opens the
   * agent's tab.
   */
  let { approvals = [], answered = [], onopen = null } = $props();

  /** Plan mode's "may I go ahead?" (approve.rs `EXIT_PLAN`). */
  const isPlan = (a) => a?.tool === 'ExitPlanMode';
  /** "Allowed from Office phone · Run a command: npm test". */
  function fromPhone(a) {
    const yes = a.outcome === 'allowed';
    if (isPlan(a)) return yes ? `Went ahead from ${a.byName}: the plan` : `Kept as a plan from ${a.byName}`;
    return `${yes ? 'Allowed' : 'Denied'} from ${a.byName} · ${a.summary}`;
  }
  // Plans opened past their fold, by the prompt's id.
  let unfolded = $state({});
</script>

{#each answered as a (a.id)}
  <!-- Answered from a paired phone (Remote Control): closed here, with who. -->
  <p class="aremote" data-testid="chat-agent-approve-remote" data-outcome={a.outcome}>
    <svg class="pmark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="7" y="2.5" width="10" height="19" rx="2.2" /><path d="M11 18h2" /></svg>
    {fromPhone(a)}
  </p>
{/each}
{#each approvals as a (a.id)}
  {#if isPlan(a)}
    {@const folds = longSaid(a.detail)}
    <div class="japprove plan" role="group" aria-label="The agent's plan is ready" data-testid="chat-agent-approve" data-tool={a.tool} data-kind="plan">
      <svg class="aicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4h6a1 1 0 0 1 1 1v1H8V5a1 1 0 0 1 1-1z" /><path d="M16 5h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h2" /><path d="M9 12h6M9 16h4" /></svg>
      <div class="abody">
        <p class="aq">
          <strong>The agent's plan is ready</strong> · <button type="button" class="alink" onclick={() => onopen?.(a.run)}>{a.label || 'your folder'}</button>
        </p>
        {#if a.detail?.trim()}
          <div class="aplan" class:folded={folds && !unfolded[a.id]} data-testid="chat-agent-plan">
            <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
            <div class="md">{@html renderMarkdown(a.detail)}</div>
          </div>
          {#if folds}
            <button type="button" class="fold" aria-expanded={!!unfolded[a.id]} onclick={() => (unfolded = { ...unfolded, [a.id]: !unfolded[a.id] })}>
              {unfolded[a.id] ? 'Show less' : 'Show more'}
            </button>
          {/if}
        {/if}
        <p class="awhy">Go ahead and it carries the plan out at this chat's Mode. Not yet keeps it a plan, with nothing changed.</p>
        <div class="abtns">
          <button type="button" class="aallow" onclick={() => answerApproval(a.id, true)}>Go ahead</button>
          <button type="button" class="adeny" onclick={() => answerApproval(a.id, false)}>Not yet</button>
        </div>
      </div>
    </div>
  {:else}
    <div class="japprove" role="group" aria-label="An agent asks to take a step" data-testid="chat-agent-approve" data-tool={a.tool}>
      <svg class="aicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z" /><path d="M12 8v5M12 16.5v.01" /></svg>
      <div class="abody">
        <p class="aq">
          The agent in <button type="button" class="alink" onclick={() => onopen?.(a.run)}>{a.label || 'your folder'}</button> asks to:
          <strong>{a.summary}</strong>
        </p>
        {#if a.detail && a.detail.trim() !== a.summary.trim()}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
          <pre class="adetail" tabindex="0" aria-label="What exactly it would do">{a.detail}</pre>
        {/if}
        <p class="awhy">{a.tool}{a.tool ? ' · ' : ''}Only you can allow it, and only for this one step.</p>
        <div class="abtns">
          <button type="button" class="aallow" onclick={() => answerApproval(a.id, true)}>Allow once</button>
          <button type="button" class="adeny" onclick={() => answerApproval(a.id, false)}>Deny</button>
        </div>
      </div>
    </div>
  {/if}
{/each}

<style>
  .aremote {
    align-self: flex-start;
    display: inline-flex;
    gap: 6px;
    align-items: center;
    margin: 0;
    color: var(--yap-muted);
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .pmark {
    flex: none;
    width: 13px;
    height: 13px;
  }
  .japprove {
    align-self: flex-start;
    display: flex;
    gap: 10px;
    box-sizing: border-box;
    width: min(560px, 100%);
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 45%, var(--yap-paper-rule));
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-warning) 14%, transparent);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  /* A plan: the agent's own proposal, in Yap's calm accent, not a warning. */
  .japprove.plan {
    width: min(640px, 100%);
    border-color: color-mix(in srgb, var(--yap-primary) 40%, var(--yap-paper-rule));
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-primary) 12%, transparent);
  }
  .aicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-warning);
  }
  .plan .aicon {
    color: var(--yap-primary);
  }
  .abody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .aq,
  .awhy {
    margin: 0;
  }
  .aq strong {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .alink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link, var(--yap-primary));
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .alink:hover {
    text-decoration: underline;
  }
  .alink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
    border-radius: 3px;
  }
  .adetail {
    max-height: 160px;
    margin: 6px 0 0;
    padding: 8px 10px;
    overflow: auto;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .adetail:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* The plan itself: markdown on the card's page, folded to its first lines. */
  .aplan {
    margin: 8px 0 0;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .aplan .md {
    overflow-wrap: anywhere;
  }
  .aplan .md :global(h1),
  .aplan .md :global(h2),
  .aplan .md :global(h3),
  .aplan .md :global(h4) {
    margin: 6px 0 4px;
    font-size: 13px;
    font-weight: 650;
  }
  .aplan .md :global(:first-child) {
    margin-top: 0;
  }
  .aplan .md :global(p) {
    margin: 0 0 6px;
  }
  .aplan .md :global(:last-child) {
    margin-bottom: 0;
  }
  .aplan .md :global(ul),
  .aplan .md :global(ol) {
    margin: 2px 0 6px;
    padding-left: 20px;
  }
  .aplan .md :global(code) {
    font-size: 11.5px;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--yap-paper-s2);
  }
  .aplan.folded .md {
    max-height: 7.5em;
    overflow: hidden;
    mask-image: linear-gradient(to bottom, #000 60%, transparent);
  }
  .fold {
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
  .fold:hover {
    text-decoration: underline;
  }
  .fold:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .awhy {
    margin-top: 6px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .abtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .abtns button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .abtns .aallow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .abtns button:hover {
    background: var(--yap-paper-s2);
  }
  .abtns .aallow:hover {
    background: var(--yap-primary-hover);
  }
  .abtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
