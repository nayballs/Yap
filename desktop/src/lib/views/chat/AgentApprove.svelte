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
  //
  // **A question** (Claude Code's AskUserQuestion, 12 Nov 2026), as Claude
  // Code's own app asks one: "The agent has a question", each question with
  // its options as buttons and their descriptions, **Something else…** for
  // the person's own words, and **Let it decide**. One question with one
  // answer is answered by the click on its option; several questions, or
  // one where several options may be picked, by **Send** once each has an
  // answer (`claude_approve_reply`: each question's text → what was picked
  // or written; core/agentNow.js `questionAnswers`: their own words take a
  // pick's place, or join the picks where several are allowed). Before, a question was
  // refused at once and Claude chose for itself. Never under Bypass
  // permissions, where nothing asks.
  import { agents, answerApproval, replyToQuestion } from '../../stores/agents.svelte.js';
  import { questionAnswers, questionsAnswered } from '../../core/agentNow.js';
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

  /** Claude Code's own question for the person (approve.rs `ASK`). */
  const isQuestion = (a) => a?.tool === 'AskUserQuestion' && a.questions?.length > 0;
  /** One question with one answer: a click on its option answers it. */
  const quick = (a) => a.questions.length === 1 && !a.questions[0].multiSelect;
  // What's picked so far: card id → question index → {labels, other, own} — kept in the store
  // (`agents.questionPicks`), so a card drawn again mid-answer still has it.
  const picks = $derived(agents.questionPicks);
  // Cards whose answer is on its way (their buttons wait).
  let sending = $state({});
  const pickOf = (a, i) => picks[a.id]?.[i] || { labels: [], other: '', own: false };
  const setPick = (a, i, next) => (agents.questionPicks = { ...agents.questionPicks, [a.id]: { ...(agents.questionPicks[a.id] || {}), [i]: { ...pickOf(a, i), ...next } } });
  async function send(a, answers) {
    if (sending[a.id] || !Object.keys(answers).length) return;
    sending = { ...sending, [a.id]: true };
    await replyToQuestion(a.id, answers);
    sending = { ...sending, [a.id]: false };
  }
  /** An option clicked: the answer itself for a quick card; picked (or put back) otherwise. */
  function choose(a, i, q, label) {
    if (quick(a)) return send(a, { [q.question]: label });
    const now = pickOf(a, i).labels;
    const labels = q.multiSelect ? (now.includes(label) ? now.filter((l) => l !== label) : [...now, label]) : [label];
    setPick(a, i, { labels, ...(q.multiSelect ? {} : { other: '', own: false }) });
  }
  const sendAll = (a) => send(a, questionAnswers(a.questions, picks[a.id]));
  /** Enter in a question's own-words box: the answer, for a quick card. */
  function ownKey(e, a) {
    if (e.key === 'Enter' && !e.shiftKey && quick(a)) {
      e.preventDefault();
      sendAll(a);
    }
  }
</script>

{#each answered as a (a.id)}
  <!-- Answered from a paired phone (Remote Control): closed here, with who. -->
  <p class="aremote" data-testid="chat-agent-approve-remote" data-outcome={a.outcome}>
    <svg class="pmark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="7" y="2.5" width="10" height="19" rx="2.2" /><path d="M11 18h2" /></svg>
    {fromPhone(a)}
  </p>
{/each}
{#each approvals as a (a.id)}
  {#if isQuestion(a)}
    <div class="japprove ask" role="group" aria-label="The agent has a question" data-testid="chat-agent-approve" data-tool={a.tool} data-kind="question">
      <svg class="aicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M9.5 9.2a2.6 2.6 0 0 1 5 .9c0 1.7-2.5 2.1-2.5 3.9" /><path d="M12 17.2h.01" /></svg>
      <div class="abody">
        <p class="aq">
          <strong>The agent has a question</strong> · <button type="button" class="alink" onclick={() => onopen?.(a.run)}>{a.label || 'your folder'}</button>
        </p>
        {#each a.questions as q, i (i)}
          {@const p = pickOf(a, i)}
          <fieldset class="qset" data-testid="chat-agent-question" disabled={!!sending[a.id]}>
            <legend class="qtext">{#if q.header}<span class="qhead">{q.header}</span>{/if}{q.question}</legend>
            {#if q.multiSelect}<p class="qhint">Pick any that fit.</p>{/if}
            <div class="qopts">
              {#each q.options as o (o.label)}
                <!-- (One answer: their own words take the pick's place.) -->
                {@const on = p.labels.includes(o.label) && (q.multiSelect || !p.other?.trim())}
                <button type="button" class="qopt" class:on aria-pressed={quick(a) ? undefined : on} data-testid="chat-agent-question-option" onclick={() => choose(a, i, q, o.label)}>
                  <span class="qlabel">{o.label}</span>
                  {#if o.description}<span class="qdesc">{o.description}</span>{/if}
                </button>
              {/each}
              {#if p.own}
                <!-- Their own words: what they meant, whatever is picked. -->
                <input
                  class="qown"
                  type="text"
                  aria-label="Your own answer"
                  placeholder="Your own answer"
                  maxlength="2000"
                  value={p.other}
                  oninput={(e) => setPick(a, i, { other: e.currentTarget.value })}
                  onkeydown={(e) => ownKey(e, a)}
                />
              {:else}
                <button type="button" class="qopt qelse" onclick={() => setPick(a, i, { own: true })}>Something else…</button>
              {/if}
            </div>
          </fieldset>
        {/each}
        <p class="awhy">Its own question, not a permission: answer it, or let it choose for itself and say what it chose.</p>
        <div class="abtns">
          {#if !quick(a) || pickOf(a, 0).own}
            <button type="button" class="aallow" disabled={!!sending[a.id] || !questionsAnswered(a.questions, picks[a.id])} onclick={() => sendAll(a)}>Send</button>
          {/if}
          <button type="button" class="adeny" disabled={!!sending[a.id]} onclick={() => answerApproval(a.id, false)}>Let it decide</button>
        </div>
      </div>
    </div>
  {:else if isPlan(a)}
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
  /* A question: the agent's own, in Yap's calm accent as a plan is. */
  .japprove.ask {
    width: min(600px, 100%);
    border-color: color-mix(in srgb, var(--yap-primary) 40%, var(--yap-paper-rule));
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-primary) 12%, transparent);
  }
  .ask .aicon {
    color: var(--yap-primary);
  }
  .qset {
    min-width: 0;
    margin: 9px 0 0;
    padding: 0;
    border: none;
  }
  .qtext {
    padding: 0;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .qhead {
    display: inline-block;
    margin-right: 7px;
    padding: 0 7px;
    border-radius: 999px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-muted);
    font-size: 11px;
    font-weight: 600;
    line-height: 18px;
    vertical-align: 1px;
  }
  .qhint {
    margin: 2px 0 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .qopts {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-top: 6px;
  }
  .qopt {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    padding: 6px 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 8px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }
  .qopt:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .qopt.on {
    border-color: var(--yap-primary-line);
    background: color-mix(in srgb, var(--yap-primary) 10%, var(--yap-paper));
  }
  .qopt:focus-visible,
  .qown:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .qopt:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .qlabel {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .qdesc {
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .qelse {
    color: var(--yap-paper-muted);
  }
  .qown {
    box-sizing: border-box;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 8px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
  }
  .abtns button:disabled {
    cursor: default;
    opacity: 0.55;
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
