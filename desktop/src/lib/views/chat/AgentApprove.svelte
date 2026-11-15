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
  // **Comments on the plan** (15 Nov 2026, the Claude app's plan review):
  // select words of the plan and **Comment** (or **Add a comment** on the
  // whole plan); each comment is listed under the plan, its words marked in
  // it. **Revise the plan** sends them back (`claude_approve_revise`):
  // Claude Code stays in Plan mode and answers with a new plan, another card;
  // this one is kept folded with its comments (PlanHistory.svelte, from the
  // turn's `plansBefore`). Go ahead works as before.
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
  import { tick } from 'svelte';
  import { agents, answerApproval, replyToQuestion, revisePlan, agentById } from '../../stores/agents.svelte.js';
  import QuestionForm from './QuestionForm.svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { longSaid } from '../../core/agents.js';
  import { selectionIn, withComment, markQuotes, plansBeforeOf } from './planComments.js';

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

  // ---- comments on a plan ----
  /** A plan card's comments so far (kept in the store: a card drawn again keeps them). */
  const commentsOf = (a) => agents.planComments[a.id] || [];
  const setComments = (a, list) => (agents.planComments = { ...agents.planComments, [a.id]: list });
  // The plan's shown markdown, and the card's body, by card: where a selection counts and its button goes.
  let planEls = $state({});
  let boxEls = $state({});
  // The selection's Comment button: `{id, quote, top, left}` (in the card's body).
  let picked = $state(null);
  // The comment being written: `{id, quote, text}`.
  let draft = $state(null);

  /** After a mouse-up or a key in the plan: the selected words, and where its Comment button goes. */
  function noticeSelection(a) {
    const box = boxEls[a.id];
    const sel = selectionIn(planEls[a.id]);
    if (!sel || !box) {
      if (picked?.id === a.id) picked = null;
      return;
    }
    const b = box.getBoundingClientRect();
    picked = { id: a.id, quote: sel.quote, top: Math.max(0, sel.rect.bottom - b.top + 4), left: Math.min(Math.max(0, sel.rect.right - b.left - 40), Math.max(0, b.width - 96)) };
  }
  /** Start a comment: on the selected words (`quote`), or on the whole plan (`''`). */
  async function startComment(a, quote = '') {
    picked = null;
    // (A plan folded: open it, so the words commented on show.)
    unfolded = { ...unfolded, [a.id]: true };
    draft = { id: a.id, quote, text: '' };
    await tick();
    document.querySelector(`[data-plan-draft="${a.id}"] textarea`)?.focus();
  }
  /** "Add a comment": on the selected words if some are, else the whole plan. */
  const addComment = (a) => startComment(a, selectionIn(planEls[a.id])?.quote || '');
  function saveComment(a) {
    if (!draft || draft.id !== a.id) return;
    setComments(a, withComment(commentsOf(a), draft.quote, draft.text));
    draft = null;
  }
  function draftKey(e, a) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      saveComment(a);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      draft = null;
    }
  }
  const removeComment = (a, i) => setComments(a, commentsOf(a).filter((_, k) => k !== i));
  async function revise(a) {
    const list = commentsOf(a);
    if (!list.length || sending[a.id]) return;
    sending = { ...sending, [a.id]: true };
    const taken = await revisePlan(a.id, list);
    sending = { ...sending, [a.id]: false };
    if (taken) setComments(a, []);
  }
  /** How many times this turn's plan was sent back with comments already (the card says it's a revision). */
  function revisions(a) {
    const list = agentById(a.run)?.turnList || [];
    const t = [...list].reverse().find((x) => x.status === 'running' || x.status === 'asking') || list.at(-1);
    return plansBeforeOf(t).length;
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
        <p class="awhy">Its own question, not a permission: answer it, or let it choose for itself and say what it chose.</p>
        <!-- The questions, as Chat's own question card has them (QuestionForm). -->
        <QuestionForm
          questions={a.questions}
          picks={picks[a.id] || {}}
          onpick={(i, next) => setPick(a, i, next)}
          onsend={(answers) => send(a, answers)}
          ondecide={() => answerApproval(a.id, false)}
          busy={!!sending[a.id]}
          decideLabel="Let it decide"
          testid="chat-agent-question"
        />
      </div>
    </div>
  {:else if isPlan(a)}
    {@const folds = longSaid(a.detail)}
    {@const comments = commentsOf(a)}
    {@const revised = revisions(a)}
    <div class="japprove plan" role="group" aria-label="The agent's plan is ready" data-testid="chat-agent-approve" data-tool={a.tool} data-kind="plan">
      <svg class="aicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4h6a1 1 0 0 1 1 1v1H8V5a1 1 0 0 1 1-1z" /><path d="M16 5h2a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h2" /><path d="M9 12h6M9 16h4" /></svg>
      <div class="abody" bind:this={boxEls[a.id]}>
        <p class="aq">
          <strong>{revised ? "The agent's revised plan is ready" : "The agent's plan is ready"}</strong> · <button type="button" class="alink" onclick={() => onopen?.(a.run)}>{a.label || 'your folder'}</button>
        </p>
        {#if a.detail?.trim()}
          <!-- svelte-ignore a11y_no_static_element_interactions (selecting words to comment on; Add a comment does it by keyboard) -->
          <div class="aplan" class:folded={folds && !unfolded[a.id]} data-testid="chat-agent-plan" onmouseup={() => noticeSelection(a)} onkeyup={() => noticeSelection(a)}>
            <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
            <div class="md" bind:this={planEls[a.id]} use:markQuotes={comments}>{@html renderMarkdown(a.detail)}</div>
          </div>
          {#if folds}
            <button type="button" class="fold" aria-expanded={!!unfolded[a.id]} onclick={() => (unfolded = { ...unfolded, [a.id]: !unfolded[a.id] })}>
              {unfolded[a.id] ? 'Show less' : 'Show more'}
            </button>
          {/if}
          {#if picked?.id === a.id}
            <!-- The selection's own button, by its end (as the Claude app's plan review has it). -->
            <button type="button" class="pcomment" style:top="{picked.top}px" style:left="{picked.left}px" data-testid="chat-plan-comment-selection" onmousedown={(e) => e.preventDefault()} onclick={() => startComment(a, picked.quote)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" /></svg>
              Comment
            </button>
          {/if}
        {/if}
        {#if comments.length}
          <ul class="pcomments" data-testid="chat-plan-comments">
            {#each comments as c, i (i)}
              <li>
                {#if c.quote}<span class="pquote">“{c.quote}”</span>{:else}<span class="pquote whole">The whole plan</span>{/if}
                <span class="ptext">{c.text}</span>
                <button type="button" class="pdrop" aria-label="Remove this comment" title="Remove" onclick={() => removeComment(a, i)}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if draft?.id === a.id}
          <div class="pdraft" data-plan-draft={a.id} data-testid="chat-plan-draft">
            {#if draft.quote}<p class="pquote">“{draft.quote}”</p>{:else}<p class="pquote whole">On the whole plan</p>{/if}
            <textarea rows="2" maxlength="2000" placeholder="Your comment…" aria-label="Your comment on the plan" bind:value={draft.text} onkeydown={(e) => draftKey(e, a)}></textarea>
            <div class="pdbtns">
              <button type="button" class="padd" onclick={() => saveComment(a)} disabled={!draft.text.trim()}>Add comment</button>
              <button type="button" onclick={() => (draft = null)}>Cancel</button>
            </div>
          </div>
        {/if}
        <p class="awhy">
          {#if comments.length}
            Revise the plan sends your comments back: it stays in Plan mode and answers with a new plan. Go ahead carries this one out as it is.
          {:else}
            Go ahead and it carries the plan out at this chat's Mode. Not yet keeps it a plan, with nothing changed. Select words in it to comment.
          {/if}
        </p>
        <div class="abtns">
          {#if comments.length}
            <button type="button" class="aallow" data-testid="chat-plan-revise" onclick={() => revise(a)} disabled={!!sending[a.id]}>Revise the plan · {comments.length}</button>
            <button type="button" class="adeny" onclick={() => answerApproval(a.id, true)} disabled={!!sending[a.id]}>Go ahead</button>
          {:else}
            <button type="button" class="aallow" onclick={() => answerApproval(a.id, true)}>Go ahead</button>
          {/if}
          <button type="button" class="adeny" onclick={() => answerApproval(a.id, false)} disabled={!!sending[a.id]}>Not yet</button>
          {#if draft?.id !== a.id}
            <button type="button" class="acomment" data-testid="chat-plan-add-comment" onmousedown={(e) => e.preventDefault()} onclick={() => addComment(a)}>Add a comment</button>
          {/if}
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
    position: relative;
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
  /* Comments on a plan: its words marked, the list under it, the one being written. */
  .aplan .md :global(mark.pmark) {
    border-radius: 2px;
    background: color-mix(in srgb, var(--yap-warning) 28%, transparent);
    color: inherit;
  }
  .pcomment {
    position: absolute;
    z-index: 2;
    display: inline-flex;
    gap: 5px;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-primary-line);
    border-radius: 999px;
    background: var(--yap-paper);
    box-shadow: 0 2px 8px color-mix(in srgb, var(--yap-paper-ink) 18%, transparent);
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .pcomment svg {
    width: 13px;
    height: 13px;
  }
  .pcomment:hover {
    background: var(--yap-paper-s2);
  }
  .pcomment:focus-visible,
  .pdrop:focus-visible,
  .pdraft textarea:focus-visible,
  .pdbtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .pcomments {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .pcomments li {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 1px 8px;
    padding: 6px 8px 6px 10px;
    border-left: 3px solid color-mix(in srgb, var(--yap-warning) 55%, var(--yap-paper-rule));
    border-radius: 0 7px 7px 0;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
  }
  .pquote {
    grid-column: 1;
    margin: 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .pquote.whole {
    font-style: italic;
  }
  .ptext {
    grid-column: 1;
    overflow-wrap: anywhere;
  }
  .pdrop {
    grid-column: 2;
    grid-row: 1 / span 2;
    align-self: center;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .pdrop svg {
    width: 13px;
    height: 13px;
  }
  .pdrop:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .pdraft {
    margin: 8px 0 0;
    padding: 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .pdraft textarea {
    box-sizing: border-box;
    width: 100%;
    margin-top: 4px;
    padding: 5px 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 7px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    resize: vertical;
  }
  .pdbtns {
    display: flex;
    gap: 6px;
    margin-top: 6px;
  }
  .pdbtns button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .pdbtns .padd {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .pdbtns button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .abtns .acomment {
    margin-left: auto;
    border-color: transparent;
    background: none;
    color: var(--yap-primary);
  }
  .abtns .acomment:hover {
    background: var(--yap-paper-s2);
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
