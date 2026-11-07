<script>
  // **A job's report in the thread** (Jobs: chat/turn/report.rs): the
  // marker message (`kind: "jobReport"`) is the job's facts and its own
  // summary, which the chat's model reads as the job's (never the person's)
  // and answers in its own words right after. Drawn as a compact job card,
  // not a question: "Job done · garden-planner" (or didn't finish, has a
  // question, its plan is ready), what it changed in a line, and Open, the
  // job's tab. Its own small chunk.
  import { reportTitle, reportFolder, jobOfReport, statusTone } from '../../core/jobs.js';
  import { jobs } from '../../stores/jobs.svelte.js';

  /** `m`: the marker message (its `id` "chat:<conv>:<i>", its `text`); `conversation`: the chat's id; `onopen(job)`. */
  let { m, conversation = null, onopen = null } = $props();

  const folder = $derived(reportFolder(m.text));
  const job = $derived(jobOfReport(jobs.list, m.id, conversation, folder));
  // Its latest report says it as the job is; an earlier one, as it was then.
  const latest = $derived(!!job && job.reportMessageId === m.id);
  const title = $derived(reportTitle(latest ? job : null, m.text));
  const tone = $derived(latest ? statusTone(job.status) : /didn't finish/.test(title) ? 'failed' : 'done');
  // What it changed, in a line (the marker's own "Files it changed: …").
  const changed = $derived((/^Files it changed: (.+)$/m.exec(m.text || '')?.[1] ?? '').replace(/\s*\(backed up first\)\.?$/, ''));
</script>

<div class="jobreport" role="note" aria-label={`${title} · ${job?.folder || folder}`} data-testid="chat-job-report" data-msg-id={m.id || undefined}>
  <span class="jdot tone-{tone}" aria-hidden="true"></span>
  <div class="jtext">
    <p class="jtitle">{title} · <span class="jfolder">{job?.folder || folder || 'a folder'}</span></p>
    {#if changed}<p class="jline">Changed {changed}</p>{/if}
  </div>
  {#if job}
    <button type="button" class="jopen" onclick={() => onopen?.(job)} aria-label={`Open the job in ${job.folder}`}>Open</button>
  {/if}
</div>

<style>
  .jobreport {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    max-width: min(520px, 78%);
    padding: 8px 10px 8px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 12.5px;
  }
  .jdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .jdot.tone-failed {
    background: var(--yap-danger);
  }
  .jdot.tone-live {
    background: var(--yap-primary);
  }
  .jdot.tone-ask {
    background: var(--yap-warning);
  }
  .jdot.tone-idle {
    background: var(--yap-muted);
  }
  .jtext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .jtitle,
  .jline {
    margin: 0;
  }
  .jtitle {
    font-weight: 600;
  }
  .jfolder {
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .jline {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .jopen {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .jopen:hover {
    border-color: var(--yap-border-hover);
  }
  .jopen:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
</style>
