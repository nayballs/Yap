<script>
  // Over the bar after a meeting: Generate summary while there's none (and
  // something to summarise), or the summary's error, with a Retry.
  import { summaryFollows } from '../stores/meetingAudio.svelte.js';
  import Icon from './NotepadIcon.svelte';

  /** `ongenerate(button)`: write the summary (Notepad.svelte). */
  let { note, job, recordingThis, askOpen, writing, hasSummary, ongenerate } = $props();
</script>

{#if !recordingThis && !askOpen && job?.state === 'error'}
  <div class="snag" role="alert">
    <span class="snagico"><Icon name="warn" /></span>
    <span class="snagtext">
      <strong>The summary didn't come through</strong>
      <span class="snagdetail" title={job.error}>{job.error}</span>
    </span>
    <button class="snagretry" aria-label="Retry" title="Try again" onclick={(e) => ongenerate(e.currentTarget)}><Icon name="retry" /></button>
  </div>
{:else if !recordingThis && !writing && !hasSummary && !askOpen && !summaryFollows(note.id) && (note.transcript?.length || note.content?.trim())}
  <button class="gen" onclick={() => ongenerate()}><Icon name="plus" />Generate summary</button>
{/if}

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  .gen {
    position: absolute;
    left: 50%;
    bottom: 98px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    box-sizing: border-box;
    padding: 10px 24px;
    border: none;
    border-radius: 9999px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font-size: 15px;
    font-weight: 550;
    line-height: 20px;
    white-space: nowrap;
    transform: translateX(-50%);
    pointer-events: auto;
    transition: background-color 0.4s var(--yap-ease-spring);
  }
  .gen :global(svg) {
    width: 15px;
    height: 15px;
    stroke-width: 2.4;
  }
  .gen:hover {
    background: var(--yap-ink-hover);
  }
  /* The summary's error, where Generate summary would be, with a Retry. */
  .snag {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 98px;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    box-sizing: border-box;
    padding: 0 4px 0 12px;
    color: var(--yap-paper-snag);
    font-size: 13px;
    line-height: 18px;
    pointer-events: auto;
  }
  .snagico {
    display: inline-flex;
  }
  .snagico :global(svg) {
    width: 15px;
    height: 15px;
  }
  .snagtext {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .snagtext strong {
    font-weight: 500;
  }
  .snagdetail {
    font-size: 12px;
    color: var(--yap-paper-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .snagretry {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 28px;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-snag);
    transition: background-color 0.15s;
  }
  .snagretry :global(svg) {
    width: 16px;
    height: 16px;
  }
  .snagretry:hover {
    background: var(--yap-paper-s2);
  }
</style>
