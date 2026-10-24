<script>
  // A meeting note's transcription job, over its transcript in the meeting
  // notepad and the Notes view alike (meetingAudio.svelte.js `jobState`):
  // - running: "Improving the transcript · 40%" (the refine after a
  //   meeting, meeting_refine.rs), "Transcribing again with Parakeet V3 ·
  //   40%"…, with Cancel (the transcript stays as it was);
  // - a refine done on a meeting with a summary: "Transcript improved ·
  //   Update summary" (the summary written again from the improved lines,
  //   `meeting_summarise`), until it is;
  // - a refine that changed nothing: why, with ✕.
  //
  //   noteId: the note.
  //   look:   'paper' (the notepad's palette) or 'app' (the Notes view's).
  import { jobState, jobLine, cancelJob, updateSummary, dismissJob } from './stores/meetingAudio.svelte.js';

  let { noteId, look = 'app' } = $props();

  const s = $derived(jobState(noteId));
  let cancelling = $state(false);
  // A new job brings the Cancel back.
  $effect(() => {
    if (s?.kind !== 'running') cancelling = false;
  });

  async function cancel() {
    cancelling = true;
    await cancelJob(noteId);
  }
</script>

{#if s?.kind === 'running'}
  <div class="mjob {look}" role="status">
    <span class="spin" aria-hidden="true"></span>
    <span class="text">{jobLine(s.job)}</span>
    <button class="link" onclick={cancel} disabled={cancelling}>{cancelling ? 'Cancelling…' : 'Cancel'}</button>
  </div>
{:else if s?.kind === 'improved'}
  <div class="mjob {look}" role="status">
    <svg class="ok" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
    <span class="text" title={s.job.model ? `Transcribed again with ${s.job.model}` : ''}>Transcript improved</span>
    <span class="dot" aria-hidden="true">·</span>
    <button class="link strong" onclick={() => updateSummary(noteId)}>Update summary</button>
  </div>
{:else if s?.kind === 'unchanged'}
  <div class="mjob {look}" role="status">
    <span class="text" title={s.job.error || ''}>The transcript wasn't improved: {s.job.error}</span>
    <button class="x" aria-label="Dismiss" onclick={() => dismissJob(noteId)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
    </button>
  </div>
{/if}

<style>
  .mjob {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spin {
    flex: 0 0 auto;
    border-radius: 50%;
    animation: mjob-spin 0.9s linear infinite;
  }
  @keyframes mjob-spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ok {
    flex: 0 0 auto;
    width: 13px;
    height: 13px;
  }
  .link {
    flex: 0 0 auto;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link.strong {
    font-weight: 600;
  }
  .link:disabled {
    cursor: default;
    text-decoration: none;
    opacity: 0.7;
  }
  .x {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .x svg {
    width: 12px;
    height: 12px;
  }

  /* The meeting notepad's palette (Notepad.svelte). */
  .paper {
    margin: 12px 40px 0 32px;
    font-size: 13px;
    color: var(--yap-paper-text-2);
  }
  .paper .spin {
    width: 11px;
    height: 11px;
    border: 1.5px solid var(--yap-paper-s2);
    border-top-color: var(--yap-live);
  }
  .paper .ok {
    color: var(--yap-live);
  }
  .paper .link {
    color: var(--yap-paper-ink);
  }
  .paper .x:hover {
    background: var(--yap-paper-s2);
  }

  /* The Notes view's (NotesView.svelte). */
  .app {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .app .spin {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--yap-border);
    border-top-color: var(--yap-primary);
  }
  .app .ok {
    color: var(--yap-primary);
  }
  .app .link {
    color: var(--yap-fg);
  }
  .app .x:hover {
    background: var(--yap-s2);
  }
</style>
