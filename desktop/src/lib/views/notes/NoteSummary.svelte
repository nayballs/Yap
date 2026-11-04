<script>
  // The open note's summary (the Summary tab): rendered markdown whose tasks
  // tick in place (notes/tasks.rs), or its markdown while it's edited (Edit
  // summary, notes/note_edits.rs); over it, what's new since it (Update),
  // whether it's the person's ("Edited") and "Back to Yap's summary"; Ctrl+F
  // finds in it (SummaryFind); under it, 👍 / 👎 (SummaryRating, on this PC
  // only).
  import { renderMarkdown } from '../../core/markdown.js';
  import { withTaskMark, newSinceLine } from '../../core/notepadText.js';
  import { taskToggled, setTask } from '../../core/noteEdits.js';
  import { canRestore, restoreSummary } from '../../core/summaryTools.js';
  import { toast } from '../../ui/toast.svelte.js';
  import SummaryFind from '../../components/SummaryFind.svelte';
  import SummaryRating from '../../components/SummaryRating.svelte';
  import { LABEL } from './notesState.svelte.js';
  import { openContextMenu, menuKey } from '../../ui/contextMenu.svelte.js';
  import { selectionMenu } from './noteMenus.js';

  /** `notes`: the view's state (notesState.svelte.js); `onopensettings(section)`: Settings. */
  let { notes, onopensettings = null } = $props();

  // A right-click on text selected in the summary's box: the transforms,
  // Cut, Copy, Select all (noteMenus.js).
  function onEditContextMenu(e) {
    const items = selectionMenu(notes, e.currentTarget, { field: 'summary', openSettings: onopensettings });
    if (items) openContextMenu(e, items, { label: 'Selected text', row: e.currentTarget });
  }

  let find = $state(null);
  let findOpen = $state(false);
  // Ctrl+F: find in the summary (while it's shown, not while it's edited).
  function onWindowKeydown(e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return;
    if (notes.summaryEditing || !notes.selected?.enhancedContent) return;
    e.preventDefault();
    find?.show();
  }
  function restore() {
    const selected = notes.selected;
    if (!selected) return;
    restoreSummary(selected, LABEL, {
      current: () => notes.selected,
      changing: () => notes.changing('enhancedContent', 'summaryEditedMs', 'summaryTs', 'summaryAi', 'summaryRating'),
    });
  }

  function onSummaryKeydown(e) {
    if (menuKey(e, onEditContextMenu)) return;
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

<svelte:window onkeydown={onWindowKeydown} />

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
      {#if canRestore(notes.selected)}
        <button class="sumupdate" title="Put Yap's own summary back (Undo puts yours back)" onclick={restore}>Back to Yap's summary</button>
      {/if}
    {/if}
  </div>
{/if}
{#if notes.summaryEditing}
  <textarea
    class="sumedit"
    aria-label="Edit the summary (Markdown)"
    bind:this={notes.summaryEl}
    bind:value={notes.summaryDraft}
    readonly={notes.transforming?.noteId === notes.selected.id && notes.transforming?.field === 'summary'}
    oninput={notes.queueSummarySave}
    onkeydown={onSummaryKeydown}
    oncontextmenu={onEditContextMenu}
  ></textarea>
{:else}
  <SummaryFind bind:this={find} bind:open={findOpen} root={() => notes.renderedEl} text={notes.selected.enhancedContent} look="app" />
  <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown
       passes no raw HTML and sanitises its output (core/markdown.js);
       its tasks tick in place (notes/tasks.rs) -->
  <div class="sumscroll">
    <div class="rendered" bind:this={notes.renderedEl} onchange={onTaskChange}>{@html renderMarkdown(notes.selected.enhancedContent, { tickable: true })}</div>
    <SummaryRating note={notes.selected} origin={LABEL} look="app" changing={() => notes.changing('summaryRating')} />
  </div>
{/if}

<style>
  .tabspacer {
    flex: 1;
  }
  /* The summary and its rating, scrolled together. */
  .sumscroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .rendered {
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
