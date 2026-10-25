<script>
  // The open note's summary (the Summary tab): rendered markdown whose tasks
  // tick in place (notes/tasks.rs), or its markdown while it's edited (Edit
  // summary, notes/note_edits.rs); over it, what's new since it (Update) and
  // whether it's the person's ("Edited").
  import { renderMarkdown } from '../../core/markdown.js';
  import { withTaskMark, newSinceLine } from '../../core/notepadText.js';
  import { taskToggled, setTask } from '../../core/noteEdits.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { LABEL } from './notesState.svelte.js';

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

  function onSummaryKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      notes.finishSummaryEdit();
    }
  }
  // A task's checkbox, ticked in the rendered summary.
  async function onTaskChange(e) {
    const selected = notes.selected;
    if (!selected) return;
    const task = taskToggled(e, selected.enhancedContent);
    if (!task) return;
    const id = selected.id;
    const before = selected.enhancedContent;
    notes.changing('enhancedContent');
    selected.enhancedContent = withTaskMark(before, task.line, task.done);
    try {
      const md = await setTask(id, task, LABEL);
      if (notes.selected?.id === id) notes.selected.enhancedContent = md;
    } catch (err) {
      if (notes.selected?.id === id) notes.selected.enhancedContent = before;
      toast({ title: "Couldn't tick the task", description: String(err), variant: 'destructive' });
    }
  }
</script>

{#if (notes.newLines || notes.selected.summaryEditedMs) && !notes.summaryEditing}
  <!-- What's new since the summary (Update), and whether it's the person's. -->
  <div class="sumbar">
    {#if notes.newLines}
      <p class="sumnew" aria-live="polite">
        {newSinceLine(notes.newLines)} ·
        <button class="sumupdate" onclick={(e) => notes.summariseMeeting(e.currentTarget)}>Update</button>
      </p>
    {/if}
    <span class="tabspacer"></span>
    {#if notes.selected.summaryEditedMs}
      <span class="sumedited" title="You changed this summary: Yap asks before writing it again">Edited</span>
    {/if}
  </div>
{/if}
{#if notes.summaryEditing}
  <textarea
    class="sumedit"
    aria-label="Edit the summary (Markdown)"
    bind:this={notes.summaryEl}
    bind:value={notes.summaryDraft}
    oninput={notes.queueSummarySave}
    onkeydown={onSummaryKeydown}
  ></textarea>
{:else}
  <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown
       passes no raw HTML and sanitises its output (core/markdown.js);
       its tasks tick in place (notes/tasks.rs) -->
  <div class="rendered" bind:this={notes.renderedEl} onchange={onTaskChange}>{@html renderMarkdown(notes.selected.enhancedContent, { tickable: true })}</div>
{/if}

<style>
  .tabspacer {
    flex: 1;
  }
  .rendered {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    border: none;
    background: transparent;
    padding: 4px 2px;
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.7 * var(--yap-reading-leading));
  }
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 14px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2) {
    font-size: calc(15px * var(--yap-reading-scale));
  }
  .rendered :global(h3) {
    font-size: calc(13.5px * var(--yap-reading-scale));
  }
  .rendered :global(p) {
    margin: 6px 0;
  }
  .rendered :global(ul),
  .rendered :global(ol) {
    margin: 6px 0;
    padding-left: 22px;
  }
  .rendered :global(li) {
    margin: 3px 0;
  }
  .rendered :global(li.task) {
    list-style: none;
    margin-left: -18px;
  }
  /* A task ticks in place (notes/tasks.rs); a done one is struck through. */
  .rendered :global(li.task > input) {
    margin: 0 4px 0 0;
    accent-color: var(--yap-primary);
    vertical-align: -1px;
  }
  .rendered :global(li.task > input:not([disabled])) {
    cursor: pointer;
  }
  .rendered :global(li.task:has(> input:checked)) {
    color: var(--yap-muted);
    text-decoration: line-through;
  }
  .rendered :global(li.task > input:focus-visible) {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  /* Over the summary: what's new since it (Update), and "Edited". */
  .sumbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 6px;
  }
  .sumnew {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .sumupdate {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .sumupdate:hover {
    text-decoration: underline;
  }
  .sumupdate:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }
  .sumedited {
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-muted-70);
  }
  .sumedit {
    flex: 1 1 auto;
    min-height: 200px;
    padding: 8px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12.5px;
    line-height: 1.55;
    resize: vertical;
  }
  .sumedit:focus {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 0;
  }
  /* A list under a task: indented under its text, past the checkbox. */
  .rendered :global(li.task > :is(ul, ol)) {
    margin-left: 18px;
  }
  .rendered :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(12px * var(--yap-reading-step));
    background: var(--yap-s1);
    border-radius: 4px;
    padding: 1px 5px;
  }
</style>
