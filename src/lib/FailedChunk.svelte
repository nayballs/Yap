<script>
  // A stretch of a meeting the speech model couldn't transcribe (a `failed`
  // segment, meeting.rs "Failed chunks"), in the meeting notepad and the
  // Notes view alike: "Couldn't transcribe 0:42–1:12 · Try again" — that
  // stretch transcribed again from the meeting's audio on disk
  // (`meeting_retry_chunk`), its lines in its place.
  //
  //   noteId: the note.     seg: the marker (its side, `ts`, `startMs`/`endMs`).
  //   look:   'paper' (the notepad's palette) or 'app' (the Notes view's).
  //   why:    why Try again can't run now (it records, a job runs, no
  //           audio), or null.
  import { failedText } from './core/notepadText.js';
  import { retryChunk, runningJob } from './meetingAudio.svelte.js';

  let { noteId, seg, look = 'app', why = null } = $props();

  const side = $derived(seg?.source === 'you' ? 'your mic' : 'the call');
  const busy = $derived(!!runningJob(noteId));
</script>

<div
  class="failed {look}"
  role="note"
  aria-label={failedText(seg)}
  title={`The speech model couldn't transcribe this part of ${side}. Its audio is kept.`}
>
  <span class="rule"></span>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
  <span class="ftext">{failedText(seg)}</span>
  <span aria-hidden="true">·</span>
  <button class="try" onclick={() => retryChunk(noteId, seg)} disabled={!!why || busy} title={why || (busy ? 'Yap is transcribing this meeting' : '')}>
    Try again
  </button>
  <span class="rule"></span>
</div>

<style>
  .failed {
    display: flex;
    align-items: center;
    gap: 7px;
    white-space: nowrap;
  }
  .rule {
    flex: 1 1 auto;
    border-top: 1px dashed currentColor;
    opacity: 0.6;
  }
  svg {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
  }
  .try {
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .try:disabled {
    cursor: default;
    text-decoration: none;
    opacity: 0.6;
  }

  /* The meeting notepad's palette (its ⏸ and "You dictated here" dividers). */
  .paper {
    height: 32px;
    box-sizing: border-box;
    padding: 10px 0;
    font-size: 12px;
    color: var(--yap-paper-date);
  }
  .paper svg {
    color: var(--yap-live);
  }
  .paper .try {
    color: var(--yap-paper-ink);
  }

  /* The Notes view's. */
  .app {
    align-self: stretch;
    margin: 1px 0;
    font-size: 10.5px;
    color: var(--yap-muted);
  }
  .app svg {
    color: var(--yap-warning);
  }
  .app .try {
    color: var(--yap-fg);
  }
</style>
