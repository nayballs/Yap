<script>
  // The note open in the Notes view, OpenWhispr's NoteEditor: a borderless
  // page — the title, the chip row (NoteMeta), My thoughts ↔ Summary tabs
  // with copy (the summary edited in place: NoteSummary), what's being
  // written and what it needs, the meeting transcript (NoteTranscript) and
  // the note's own chat with the action picker (NoteChat). With no note
  // open, OpenWhispr's empty state.
  import NoteMeta from './NoteMeta.svelte';
  import NoteSummary from './NoteSummary.svelte';
  import NoteTranscript from './NoteTranscript.svelte';
  import NoteChat from './NoteChat.svelte';
  import MeetingTasks from '../../components/MeetingTasks.svelte';

  /** `notes`: the view's state; `onopensettings(section)`: ControlPanel's Settings. */
  let { notes, onopensettings = null } = $props();

  const hasSummary = $derived(!!(notes.selected?.enhancedContent || notes.summaryEditing));
  // A meeting note has a Tasks tab (the summary's tasks and the ones added).
  const isMeeting = $derived(notes.selected?.noteType === 'meeting' || !!notes.selected?.transcript?.length);
</script>

<section class="editor">
  {#if !notes.selected}
    <!-- Matches OpenWhispr's main empty state copy -->
    <div class="empty">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
      <h2>No notes here yet</h2>
      <p>Create your first note to start writing</p>
      <button class="primary" onclick={notes.newNote}>+ Create note</button>
    </div>
  {:else}
    <div class="edhead">
      <input
        bind:this={notes.titleEl}
        class="title"
        class:auto={notes.selected.titleAuto}
        placeholder="Untitled Note"
        bind:value={notes.selected.title}
        oninput={() => notes.queueSave('title')}
      />
    </div>

    <!-- Meta chip row (OpenWhispr NoteEditor header): date · attendees ·
         folder, with export at the right. -->
    <NoteMeta {notes} />

    {#if hasSummary || isMeeting}
      <div class="tabs">
        <!-- The meeting notepad's names for the same things. -->
        <button
          class="tab"
          class:active={notes.tab === 'raw'}
          onclick={() => {
            notes.finishSummaryEdit();
            notes.tab = 'raw';
          }}>My thoughts</button
        >
        {#if hasSummary}
          <button class="tab" class:active={notes.tab === 'enhanced'} onclick={() => (notes.tab = 'enhanced')}>
            Summary
            {#if notes.isStale}<span class="staledot" title="Your notes changed since this summary: run the action again to update it"></span>{/if}
          </button>
        {/if}
        {#if isMeeting}
          <!-- The meeting's tasks: the summary's and those added (notes/tasks.rs). -->
          <button
            class="tab"
            class:active={notes.tab === 'tasks'}
            onclick={() => {
              notes.finishSummaryEdit();
              notes.tab = 'tasks';
            }}>Tasks</button
          >
        {/if}
        <span class="tabspacer"></span>
        {#if !(notes.tab === 'tasks' && isMeeting)}
          {#if notes.tab === 'enhanced'}
            <!-- Fix the summary's words in place (notes/note_edits.rs). -->
            <button
              class="copy"
              aria-pressed={notes.summaryEditing}
              onclick={() => (notes.summaryEditing ? notes.finishSummaryEdit() : notes.startSummaryEdit())}
            >
              {notes.summaryEditing ? 'Done' : 'Edit summary'}
            </button>
          {/if}
          <button class="copy" onclick={() => notes.copyEnhanced(false)}>
            {notes.copied === 'markdown' ? 'Copied!' : 'Copy as Markdown'}
          </button>
          <button class="copy" onclick={() => notes.copyEnhanced(true)}>
            {notes.copied === 'text' ? 'Copied!' : 'Copy as text'}
          </button>
        {/if}
      </div>
    {/if}

    {#if notes.runningAction?.noteId === notes.selected.id || notes.jobRunning}
      <p class="runline" aria-live="polite">
        <span class="spin"></span>
        {#if notes.summaryProgress?.total > 0}
          Catching up on the meeting — part {notes.summaryProgress.done + 1} of {notes.summaryProgress.total}…
        {:else if notes.jobRunning || notes.runningAction?.name === notes.actionPlanAction?.name}
          Writing your action plan…
        {:else}
          Running {notes.runningAction.name}…
        {/if}
      </p>
    {/if}

    {#if notes.needsAi}
      <div class="aicard">
        <div class="aitext">
          <strong>Your meeting is saved.</strong>
          To turn it into an action plan, Yap needs an AI model: set one up in Settings → Language
          Models (an on-device model keeps everything on this PC), then press Action plan.
          <span class="aidetail">{notes.needsAi}</span>
        </div>
        <button class="aibtn" onclick={() => onopensettings?.('cleanup')}>Open Language Models</button>
      </div>
    {/if}

    {#if notes.error}
      <p class="errline">{notes.error}</p>
    {:else if notes.job?.state === 'error'}
      <p class="errline">{notes.job.error}</p>
    {/if}

    {#if notes.tab === 'tasks' && isMeeting}
      <MeetingTasks noteId={notes.selected.id} look="app" />
    {:else if notes.tab === 'enhanced' && (notes.selected.enhancedContent || notes.summaryEditing)}
      <NoteSummary {notes} />
    {:else}
      <textarea
        class="raw"
        placeholder={notes.recordingThisNote
          ? 'Take rough notes while Yap listens — they get woven into the minutes…'
          : 'Start writing…'}
        bind:value={notes.selected.content}
        oninput={() => notes.queueSave('content')}
      ></textarea>
    {/if}

    <!-- Meeting transcript (You/Them chat bubbles, OpenWhispr
         MeetingTranscriptChat style). Live while recording; kept afterwards. -->
    {#if notes.recordingThisNote || notes.selected.transcript?.length}
      <NoteTranscript {notes} />
    {:else if notes.selected.transcriptDeletedTs}
      <!-- Deleted after its days (Settings → Privacy & data, notes/retention.rs). -->
      <p class="tdeleted" role="note">
        This meeting's transcript was deleted after the days set in Settings → Privacy & data. Your notes, the summary
        and its tasks are kept.
      </p>
    {/if}

    <NoteChat {notes} {onopensettings} />
  {/if}
</section>

<style>
  .editor {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: 18px 22px 20px;
  }
  .edhead {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 10px;
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    border: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .title:focus {
    outline: none;
  }
  .title::placeholder {
    color: var(--yap-muted-55);
  }
  /* A title Yap made up ("Teams call · 5 Oct, 14:30"), until the AI names
     the meeting or the person does. */
  .title.auto {
    color: var(--yap-muted);
    font-weight: 600;
  }

  /* "Writing your action plan…" */
  .runline {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--yap-muted);
  }

  /* The summary step needs an AI model: say so, kindly. */
  .aicard {
    display: flex;
    align-items: center;
    gap: 14px;
    margin: 0 0 12px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-primary-wash);
  }
  .aitext {
    flex: 1 1 auto;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-fg);
  }
  .aidetail {
    display: block;
    margin-top: 4px;
    font-size: 11px;
    color: var(--yap-muted);
  }
  .aibtn {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 13px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .aibtn:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .spin {
    width: 11px;
    height: 11px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 10px;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .tab.active {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-weight: 600;
  }
  .tabspacer {
    flex: 1;
  }
  .copy {
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .copy:hover {
    text-decoration: underline;
  }

  .staledot {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-key-bg);
    flex: 0 0 auto;
  }

  /* Document-style editor (OpenWhispr NoteEditor: borderless page, not a
     boxed textarea). */
  .raw {
    flex: 1 1 auto;
    min-height: 0;
    resize: none;
    border: none;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    /* Reading text: Settings → General → Appearance (app.css). */
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.7 * var(--yap-reading-leading));
    padding: 4px 2px;
  }
  .raw:focus {
    outline: none;
  }
  .raw::placeholder {
    color: var(--yap-muted-55);
  }
  .copy:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
    border-radius: 4px;
  }

  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    text-align: center;
    color: var(--yap-muted);
    max-width: 380px;
    margin: 0 auto;
  }
  .empty svg {
    width: 34px;
    height: 34px;
    color: var(--yap-muted-55);
    margin-bottom: 4px;
  }
  .empty h2 {
    margin: 0;
    font-size: 15px;
    color: var(--yap-fg);
  }
  .empty p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.6;
  }
  .primary {
    margin-top: 10px;
    height: 32px;
    padding: 0 16px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .primary:hover {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }

  .errline {
    margin: 0 0 10px;
    font-size: 12px;
    color: var(--yap-danger);
    line-height: 1.5;
  }
  /* A transcript deleted after its days (notes/retention.rs). */
  .tdeleted {
    margin: 14px 0 0;
    padding: 10px 12px;
    border: 1px dashed var(--yap-border);
    border-radius: var(--yap-r-lg);
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
</style>
