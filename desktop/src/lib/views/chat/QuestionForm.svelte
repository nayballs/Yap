<script>
  // **A question card's questions, to answer** (15 Nov 2026): the one form
  // for Claude Code's own questions in a Code chat (AgentApprove.svelte,
  // integrations/claude_live/approve.rs) and Chat's own model's (ChatQuestion.svelte,
  // chat/cards/ask.rs), so both look and work the same, as the Claude app's
  // question card does: each question with its header chip, its options as
  // buttons with their descriptions, **Something else…** for the person's own
  // words, and the button that leaves the choice to the model.
  //
  // One question with one answer is answered by the click on its option;
  // several questions, or one where several options may be picked, by
  // **Send** once each has an answer (core/agentNow.js `questionAnswers`:
  // their own words take a pick's place, or join the picks where several are
  // allowed). **A preview** (an option's `preview`: a draft, a layout, a bit
  // of code in markdown) shows beside the options for the one pointed at or
  // focused, else the one picked, else the first — under them on a narrow card.
  import { renderMarkdown } from '../../core/markdown.js';
  import { questionAnswers, questionsAnswered } from '../../core/agentNow.js';

  /**
   * `questions`: `[{question, header, options: [{label, description, preview?}], multiSelect}]`;
   * `picks`: by question index, `{labels, other, own}`; `onpick(i, patch)`;
   * `onsend(answers)`: each question's text → a label, a list, or own words;
   * `ondecide()`; `busy`: an answer on its way; `decideLabel`; `testid`: the
   * question's test id (its option's adds "-option").
   */
  let { questions = [], picks = {}, onpick, onsend, ondecide, busy = false, decideLabel = 'Let it decide', testid = 'chat-question' } = $props();

  /** One question with one answer: a click on its option answers it. */
  const quick = $derived(questions.length === 1 && !questions[0]?.multiSelect);
  const pickOf = (i) => picks?.[i] || { labels: [], other: '', own: false };
  // The option pointed at or focused, by question: its preview shows.
  let seen = $state({});

  /** An option clicked: the answer itself for a quick card; picked (or put back) otherwise. */
  function choose(i, q, label) {
    if (quick) return onsend?.({ [q.question]: label });
    const now = pickOf(i).labels;
    const labels = q.multiSelect ? (now.includes(label) ? now.filter((l) => l !== label) : [...now, label]) : [label];
    onpick?.(i, { labels, ...(q.multiSelect ? {} : { other: '', own: false }) });
  }
  const sendAll = () => onsend?.(questionAnswers(questions, picks));
  /** Enter in a question's own-words box: the answer, for a quick card. */
  function ownKey(e) {
    if (e.key === 'Enter' && !e.shiftKey && quick) {
      e.preventDefault();
      sendAll();
    }
  }
  /** The option whose preview shows for question `i`. */
  function previewOf(i, q) {
    const at = seen[i] ?? q.options.findIndex((o) => pickOf(i).labels.includes(o.label));
    return q.options[at >= 0 ? at : 0];
  }
</script>

<div class="qform">
{#each questions as q, i (i)}
  {@const p = pickOf(i)}
  {@const previews = q.options.some((o) => o.preview)}
  <fieldset class="qset" data-testid={testid} disabled={busy}>
    <legend class="qtext">{#if q.header}<span class="qhead">{q.header}</span>{/if}{q.question}</legend>
    {#if q.multiSelect}<p class="qhint">Pick any that fit.</p>{/if}
    <div class="qbody" class:previews>
      <div class="qopts">
        {#each q.options as o, j (o.label)}
          <!-- (One answer: their own words take the pick's place.) -->
          {@const on = p.labels.includes(o.label) && (q.multiSelect || !p.other?.trim())}
          <button
            type="button"
            class="qopt"
            class:on
            class:seen={previews && previewOf(i, q) === o}
            aria-pressed={quick ? undefined : on}
            data-testid="{testid}-option"
            onclick={() => choose(i, q, o.label)}
            onmouseenter={() => (seen = { ...seen, [i]: j })}
            onfocus={() => (seen = { ...seen, [i]: j })}
          >
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
            oninput={(e) => onpick?.(i, { other: e.currentTarget.value })}
            onkeydown={ownKey}
          />
        {:else}
          <button type="button" class="qopt qelse" onclick={() => onpick?.(i, { own: true })}>Something else…</button>
        {/if}
      </div>
      {#if previews}
        {@const shown = previewOf(i, q)}
        <div class="qpreview" data-testid="{testid}-preview" aria-label={`Preview: ${shown?.label ?? ''}`}>
          {#if shown?.preview}
            <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
            <div class="md">{@html renderMarkdown(shown.preview)}</div>
          {:else}
            <p class="qnone">No preview for {shown?.label ?? 'this one'}.</p>
          {/if}
        </div>
      {/if}
    </div>
  </fieldset>
{/each}
<div class="qbtns">
  {#if !quick || pickOf(0).own}
    <button type="button" class="qsend" disabled={busy || !questionsAnswered(questions, picks)} onclick={sendAll}>Send</button>
  {/if}
  <button type="button" class="qdecide" disabled={busy} onclick={() => ondecide?.()}>{decideLabel}</button>
</div>
</div>

<style>
  /* (Its own width decides where a preview goes.) */
  .qform {
    container-type: inline-size;
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
  .qbody {
    margin-top: 6px;
  }
  /* Options on the left, the preview beside them (under them when narrow). */
  .qbody.previews {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr);
    gap: 8px;
    align-items: start;
  }
  @container (max-width: 460px) {
    .qbody.previews {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .qopts {
    display: flex;
    flex-direction: column;
    gap: 5px;
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
  .qopt:hover:not(:disabled),
  .qopt.seen {
    background: var(--yap-paper-s2);
  }
  .qopt.on {
    border-color: var(--yap-primary-line);
    background: color-mix(in srgb, var(--yap-primary) 10%, var(--yap-paper));
  }
  .qopt:focus-visible,
  .qown:focus-visible,
  .qbtns button:focus-visible {
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
  .qpreview {
    min-width: 0;
    max-height: 260px;
    overflow: auto;
    padding: 8px 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-size: 12px;
    line-height: 1.5;
  }
  .qpreview .md {
    overflow-wrap: anywhere;
  }
  .qpreview .md :global(:first-child) {
    margin-top: 0;
  }
  .qpreview .md :global(:last-child) {
    margin-bottom: 0;
  }
  .qpreview .md :global(h1),
  .qpreview .md :global(h2),
  .qpreview .md :global(h3) {
    margin: 4px 0;
    font-size: 13px;
    font-weight: 650;
  }
  .qpreview .md :global(p) {
    margin: 0 0 6px;
  }
  .qpreview .md :global(pre) {
    margin: 0;
    white-space: pre-wrap;
    font-size: 11.5px;
  }
  .qpreview .md :global(ul),
  .qpreview .md :global(ol) {
    margin: 2px 0 6px;
    padding-left: 18px;
  }
  .qnone {
    margin: 0;
    color: var(--yap-paper-muted);
  }
  .qbtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .qbtns button {
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
  .qbtns .qsend {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .qbtns button:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .qbtns .qsend:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .qbtns button:disabled {
    cursor: default;
    opacity: 0.55;
  }
</style>
