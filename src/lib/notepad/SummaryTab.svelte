<script>
  // The Summary tab: the action plan Rust writes when the meeting ends
  // (meeting_end.rs), with "Step 2 of 3" while it's written; above it the
  // meeting's pre-meeting brief (calendar/brief.rs), open until the plan is
  // written, then folded. The summary is edited in place (note_edits.rs: Edit
  // / Done, what's new since it with Update) and its tasks tick (tasks.rs).
  // Before there's one: the AI notes so far, or what the summary needs.
  import { tick } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { renderMarkdown } from '../core/markdown.js';
  import { briefTeaser } from '../core/brief.js';
  import { withTaskMark, newSinceSummary, newSinceLine } from '../core/notepadText.js';
  import { taskToggled, setTask } from '../core/noteEdits.js';
  import { stepLine } from '../stores/meetingSummary.svelte.js';
  import BriefView from '../components/BriefView.svelte';
  import SpeakerCard from '../components/SpeakerCard.svelte';
  import { LABEL, openSettings } from './actions.js';
  import Icon from './NotepadIcon.svelte';

  /**
   * `job` / `progress`: the note's action-plan job (meetingSummary.svelte.js);
   * `briefFold`: null = as the meeting goes, else the person's choice;
   * `summary`: the SummaryEditor; `ongenerate(button)`: write it (again).
   */
  let {
    note = $bindable(),
    recordingThis,
    job,
    progress,
    writing,
    hasSummary,
    brief,
    briefFold = $bindable(),
    summary,
    changing,
    ongenerate,
    onspeakerchanged,
  } = $props();

  const briefFolded = $derived(briefFold ?? hasSummary);
  const newLines = $derived(recordingThis ? 0 : newSinceSummary(note));
  const minutes = $derived.by(() => {
    const t = (note?.transcript || []).filter((s) => !s.dictated);
    const secs = t.length > 1 ? t[t.length - 1].ts - t[0].ts : 0;
    return note?.transcript?.length ? Math.round(secs / 60) : null;
  });

  let summaryEl = $state(null);
  async function startSummaryEdit() {
    if (!summary.start()) return;
    await tick();
    summaryEl?.focus();
    fitSummary();
  }
  function fitSummary() {
    if (!summaryEl) return;
    summaryEl.style.height = 'auto';
    summaryEl.style.height = `${summaryEl.scrollHeight + 2}px`;
  }
  function queueSummarySave() {
    summary.queueSave();
    fitSummary();
  }
  function onSummaryKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      summary.finish();
    }
  }
  // A task's checkbox, ticked in the rendered summary.
  async function onTaskChange(e) {
    if (!note) return;
    const task = taskToggled(e, note.enhancedContent);
    if (!task) return;
    const id = note.id;
    const before = note.enhancedContent;
    changing('summary');
    note.enhancedContent = withTaskMark(before, task.line, task.done);
    try {
      const md = await setTask(id, task, LABEL);
      if (note?.id === id) note.enhancedContent = md;
    } catch (err) {
      if (note?.id === id) note.enhancedContent = before;
      toast({ title: "Couldn't tick the task", description: String(err), variant: 'destructive' });
    }
  }
</script>

<div class="summary" role="region" aria-label="Summary">
  {#if note && !recordingThis}
    <SpeakerCard {note} origin={LABEL} onchanged={onspeakerchanged} />
  {/if}
  {#if brief}
    <!-- What to know going in (calendar/brief.rs): open until the
         action plan is written, then folded above it. -->
    <section class="briefcard" class:folded={briefFolded} aria-label="Brief before the meeting">
      <button class="briefhead" aria-expanded={!briefFolded} onclick={() => (briefFold = !briefFolded)}>
        <Icon name="note" />
        <span class="bh">Brief before the meeting</span>
        {#if briefFolded}<span class="bt">{briefTeaser(brief)}</span>{/if}
        <span class="chev" class:up={!briefFolded} aria-hidden="true"><Icon name="chevron" /></span>
      </button>
      {#if !briefFolded}
        <div class="briefbody"><BriefView {brief} /></div>
      {/if}
    </section>
  {/if}
  {#if writing}
    <div class="steps" aria-live="polite">
      <span class="stepdot" aria-hidden="true"></span>
      <span class="stepline">{stepLine(job, progress, minutes)}</span>
      <span class="stepcount">Step {job.step} of {job.steps}</span>
    </div>
  {:else if job?.state === 'needsAi'}
    <div class="aicard">
      <p><strong>Your meeting is saved.</strong> To turn it into an action plan, Yap needs an AI model: set one up in Language Models (an on-device model keeps everything on this PC), then press Generate summary.</p>
      <button class="ink" onclick={() => openSettings('cleanup')}>Open Language Models</button>
    </div>
  {:else if job?.state === 'nothing'}
    <p class="sumnote">Nothing to summarise yet: no speech was transcribed and nothing was typed.</p>
  {/if}

  {#if (hasSummary || summary.editing) && !writing}
    <!-- Edit it in place (note_edits.rs); what's new since it, with Update. -->
    <div class="sumbar">
      {#if newLines && !summary.editing}
        <p class="sumnew" aria-live="polite">
          {newSinceLine(newLines)} ·
          <button class="sumupdate" onclick={(e) => ongenerate(e.currentTarget)}>Update</button>
        </p>
      {/if}
      <span class="grow"></span>
      {#if note.summaryEditedMs && !summary.editing}
        <span class="sumedited" title="You changed this summary: Yap asks before writing it again">Edited</span>
      {/if}
      <button
        class="sumeditbtn"
        aria-pressed={summary.editing}
        onclick={() => (summary.editing ? summary.finish() : startSummaryEdit())}
      >
        <Icon name={summary.editing ? 'check' : 'pencil'} />{summary.editing ? 'Done' : 'Edit'}
      </button>
    </div>
    {#if summary.editing}
      <textarea
        class="sumedit"
        aria-label="Edit the summary (Markdown)"
        bind:this={summaryEl}
        bind:value={summary.draft}
        oninput={queueSummarySave}
        onkeydown={onSummaryKeydown}
      ></textarea>
    {:else}
      <!-- renderMarkdown passes no raw HTML and sanitises its output (core/markdown.js);
           its tasks tick in place (tasks.rs). -->
      <div class="rendered" onchange={onTaskChange}>{@html renderMarkdown(note.enhancedContent, { tickable: true })}</div>
    {/if}
  {:else if !writing && job?.state !== 'needsAi'}
    <p class="sumempty">
      <Icon name="note" />
      {recordingThis ? 'Your summary is written when you stop' : 'No summary yet'}
    </p>
    {#if note.digests?.length}
      <div class="sofar">
        <p class="sofarcap">AI notes so far</p>
        {#each note.digests as d, i (i)}
          <ul>
            {#each d.keyPoints || [] as p, j (j)}<li>{p}</li>{/each}
            {#each d.actions || [] as a, j (j)}<li class="task">☐ {a.owner}: {a.task}{a.due ? ` (due ${a.due})` : ''}</li>{/each}
          </ul>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  button {
    font: inherit;
    cursor: pointer;
  }
  .grow {
    flex: 1;
  }

  /* Rows here line up with the transcript's timer box (x 32, padded 12). */
  .summary {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 40px 180px 32px;
  }
  /* Thin scrollbars, as on the page they sit on (8px, a soft thumb). */
  .summary::-webkit-scrollbar {
    width: 8px;
  }
  .summary::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-paper-ink) 16%, transparent);
  }
  /* The meeting's brief, on top: a paper card, folded once there's a plan. */
  .briefcard {
    margin-bottom: 18px;
    border-radius: 12px;
    background: var(--yap-paper-s1);
  }
  .briefhead {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 10px 12px;
    border: none;
    border-radius: 12px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .briefhead:hover {
    background: var(--yap-paper-s2);
  }
  .briefhead :global(svg) {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-paper-muted);
  }
  .briefhead .bh {
    font-weight: 650;
  }
  .briefhead .bt {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--yap-paper-muted);
  }
  .briefhead .chev {
    margin-left: auto;
    display: inline-flex;
    transition: transform 160ms ease;
  }
  .briefhead .chev.up {
    transform: rotate(180deg);
  }
  .briefbody {
    padding: 2px 14px 14px 35px;
    user-select: text;
  }
  .steps {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
  }
  .stepdot {
    flex: 0 0 auto;
    width: 5px;
    height: 5px;
    margin: 0 6.5px;
    border-radius: 50%;
    background: var(--yap-paper-ink);
    animation: breathe 1.2s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
  }
  .stepline {
    flex: 1;
    font-size: 13px;
  }
  .stepcount {
    font-size: 12px;
    color: var(--yap-paper-faint);
    font-variant-numeric: tabular-nums;
  }
  .aicard {
    margin-bottom: 14px;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--yap-paper-s1);
  }
  .aicard p {
    margin: 0 0 10px;
    font-size: 13px;
    line-height: 20px;
  }
  .sumnote {
    margin: 0 0 12px;
    padding: 0 12px;
    font-size: 13px;
    color: var(--yap-paper-muted);
  }
  .sumempty {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    padding: 10px 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-paper-faint);
  }
  .sumempty :global(svg) {
    width: 15px;
    height: 15px;
  }
  .rendered {
    padding: 0 12px;
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
    user-select: text;
    -webkit-user-select: text;
  }
  /* renderMarkdown shifts levels: "## Action plan" is an h3, "### Alice" an
     h4. Sections in the display serif, people in the UI sans. */
  .rendered :global(h2),
  .rendered :global(h3),
  .rendered :global(h4),
  .rendered :global(h5) {
    margin: 18px 0 6px;
    line-height: 1.3;
  }
  .rendered :global(h2),
  .rendered :global(h3) {
    font-family: var(--yap-font-display);
    font-size: calc(24px * var(--yap-reading-step));
    font-weight: 400;
    letter-spacing: -0.02em;
  }
  .rendered :global(h4),
  .rendered :global(h5) {
    margin-top: 12px;
    font-size: calc(15px * var(--yap-reading-scale));
    font-weight: 600;
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
  /* A task ticks in place (tasks.rs): the ink's own colour, a done one struck through. */
  .rendered :global(li.task > input) {
    margin: 0 4px 0 0;
    accent-color: var(--yap-paper-ink);
    vertical-align: -1px;
  }
  .rendered :global(li.task > input:not([disabled])) {
    cursor: pointer;
  }
  .rendered :global(li.task:has(> input:checked)) {
    color: var(--yap-paper-muted);
    text-decoration: line-through;
    text-decoration-color: var(--yap-paper-faint);
  }
  .rendered :global(li.task > input:focus-visible) {
    outline: 3px solid var(--yap-paper-ring);
    outline-offset: 2px;
  }

  /* Over the summary: what's new since it (Update), "Edited", Edit / Done. */
  .sumbar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    margin: 0 0 6px;
    padding: 0 12px;
  }
  .sumbar .grow {
    flex: 1;
  }
  .sumnew {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-paper-muted);
  }
  .sumupdate {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .sumedited {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-paper-faint);
  }
  .sumeditbtn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .sumeditbtn:hover {
    color: var(--yap-paper-ink);
    background: var(--yap-paper-s1);
  }
  .sumeditbtn :global(svg) {
    width: 13px;
    height: 13px;
  }
  .sumupdate:focus-visible,
  .sumeditbtn:focus-visible {
    outline: 3px solid var(--yap-paper-ring);
    outline-offset: 2px;
  }
  .sumedit {
    display: block;
    box-sizing: border-box;
    width: calc(100% - 24px);
    min-height: 240px;
    margin: 0 12px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 13px;
    line-height: 1.55;
    resize: vertical;
  }
  .sumedit:focus {
    outline: 3px solid var(--yap-paper-ring);
    outline-offset: 0;
  }
  /* A list under a task: indented under its text, past the checkbox. */
  .rendered :global(li.task > :is(ul, ol)) {
    margin-left: 18px;
  }
  .sofar {
    margin-top: 8px;
    padding: 0 12px;
    font-size: 14px;
    line-height: 21px;
  }
  .sofarcap {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-paper-faint);
  }
  .sofar ul {
    margin: 0 0 8px;
    padding-left: 18px;
    font-family: var(--yap-font-reading);
    font-size: calc(14px * var(--yap-reading-scale));
    line-height: calc(1.5 * var(--yap-reading-leading));
  }
  .sofar li.task {
    list-style: none;
    margin-left: -14px;
  }
</style>
