<script>
  // **The plans sent back with comments** (15 Nov 2026, Plan mode's review;
  // agents/store.rs `TurnNote::plans_before`): each plan of the turn that the
  // person sent back with Revise the plan, folded — "Plan before your
  // comments · 2 comments" — as the to-do list keeps "Plan from before your
  // last message" (AgentTodoCard.svelte). Opened: the plan as it was, the
  // words commented on marked, and the comments under it. While Claude
  // writes the new plan (`revising`), a line says so under the last.
  import { renderMarkdown } from '../../core/markdown.js';
  import { beforeLabel, markQuotes } from './planComments.js';

  /** `plans`: `[{plan, comments: [{quote, text}], ts}]`, oldest first; `revising`: the new plan isn't in yet. */
  let { plans = [], revising = false } = $props();
</script>

{#each plans as p, i (p.ts + ':' + i)}
  <details class="pbefore" data-testid="chat-plan-before">
    <summary>
      <svg class="pchev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      <span>{beforeLabel(p, i, plans.length)}</span>
    </summary>
    <div class="pbody">
      <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
      <div class="md" use:markQuotes={p.comments}>{@html renderMarkdown(p.plan || '')}</div>
      {#if p.comments?.length}
        <ul class="pcomments">
          {#each p.comments as c, k (k)}
            <li>
              {#if c.quote}<span class="pquote">“{c.quote}”</span>{:else}<span class="pquote whole">The whole plan</span>{/if}
              <span class="ptext">{c.text}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </details>
{/each}
{#if revising && plans.length}
  <p class="prevising" data-testid="chat-plan-revising">Revising the plan with your comments…</p>
{/if}

<style>
  .pbefore {
    align-self: flex-start;
    box-sizing: border-box;
    width: min(640px, 100%);
    margin: 2px 0;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  summary {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 7px 12px;
    color: var(--yap-paper-muted);
    font-size: 12.5px;
    cursor: pointer;
    list-style: none;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary:hover {
    color: var(--yap-paper-ink);
  }
  summary:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
    border-radius: 10px;
  }
  .pchev {
    flex: none;
    width: 13px;
    height: 13px;
    transition: transform 0.12s;
  }
  details[open] .pchev {
    transform: rotate(90deg);
  }
  .pbody {
    padding: 0 12px 10px 31px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .md {
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4) {
    margin: 6px 0 4px;
    font-size: 13px;
    font-weight: 650;
  }
  .md :global(:first-child) {
    margin-top: 0;
  }
  .md :global(p) {
    margin: 0 0 6px;
  }
  .md :global(ul),
  .md :global(ol) {
    margin: 2px 0 6px;
    padding-left: 20px;
  }
  .md :global(code) {
    font-size: 11.5px;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--yap-paper-s2);
  }
  .md :global(mark.pmark) {
    border-radius: 2px;
    background: color-mix(in srgb, var(--yap-warning) 28%, transparent);
    color: inherit;
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
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 5px 8px 5px 10px;
    border-left: 3px solid color-mix(in srgb, var(--yap-warning) 55%, var(--yap-paper-rule));
    border-radius: 0 7px 7px 0;
    background: var(--yap-paper);
  }
  .pquote {
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .pquote.whole {
    font-style: italic;
  }
  .ptext {
    overflow-wrap: anywhere;
  }
  .prevising {
    align-self: flex-start;
    margin: 2px 0 0 12px;
    color: var(--yap-paper-muted);
    font-size: 12.5px;
  }
</style>
