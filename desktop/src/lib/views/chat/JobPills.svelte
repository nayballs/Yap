<script>
  // The jobs an answer started (Jobs: chat/tools/jobs.rs `start_job`): a
  // pill each under it — the folder and a status dot; a click opens its Job
  // tab beside the chat. Found by the question the job keeps (`messageId`,
  // core/jobs.js `pillsByAnswer`). Its own small chunk (ChatThread loads it
  // for an answer that has some).
  import { statusWord, statusTone } from '../../core/jobs.js';

  /** `jobs`: the answer's jobs; `onopen(job)` opens one's tab. */
  let { jobs = [], onopen = null } = $props();
</script>

<div class="jobpills" role="group" aria-label="Jobs">
  {#each jobs as j (j.id)}
    <button
      type="button"
      class="jobpill"
      data-testid="chat-job-pill"
      data-status={j.status}
      onclick={() => onopen?.(j)}
      aria-label={`Open the job in ${j.folder}: ${statusWord(j.status)}`}
      title={j.path || j.folder}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
      <span class="jname">{j.folder}</span>
      <span class="jstate tone-{statusTone(j.status)}"><span class="dot" aria-hidden="true"></span>{statusWord(j.status)}</span>
      <span class="jopen" aria-hidden="true">· Open</span>
    </button>
  {/each}
</div>

<style>
  .jobpills {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-width: 78%;
  }
  /* PagePills' look. */
  .jobpill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    box-sizing: border-box;
    max-width: min(360px, 100%);
    height: 32px;
    padding: 0 12px 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .jobpill:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .jobpill:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .jobpill svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .jname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .jstate {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: 0 0 auto;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-muted);
  }
  .tone-live .dot {
    background: var(--yap-primary);
  }
  .tone-ask .dot {
    background: var(--yap-warning);
  }
  .tone-done .dot {
    background: var(--yap-success);
  }
  .tone-failed .dot {
    background: var(--yap-danger);
  }
  .jopen {
    flex: 0 0 auto;
    color: var(--yap-primary);
  }
</style>
