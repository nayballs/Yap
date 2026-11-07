<script>
  // **A running job asks to take a step** (Jobs: the run endpoint's
  // `approve`, integrations/claude_live/approve.rs): its card in the thread
  // of the chat that started the job — the tool, what it would do in plain
  // words ("Run a command: npm test"), the command or edit itself, and
  // **Allow once** / **Deny** (`claude_approve_answer`). Gone once it's
  // answered here, on the Yap bar, or no longer waiting
  // (`yap-claude-approve-done`). Never answered by voice. Its own small
  // chunk.
  import { answerApproval } from '../../stores/jobs.svelte.js';

  /** `approvals`: this chat's prompts waiting (`Pending`), oldest first; `onopen(run)` opens the job's tab. */
  let { approvals = [], onopen = null } = $props();
</script>

{#each approvals as a (a.id)}
  <div class="japprove" role="group" aria-label="A job asks to take a step" data-testid="chat-job-approve" data-tool={a.tool}>
    <svg class="aicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z" /><path d="M12 8v5M12 16.5v.01" /></svg>
    <div class="abody">
      <p class="aq">
        The job in <button type="button" class="alink" onclick={() => onopen?.(a.run)}>{a.label || 'your folder'}</button> asks to:
        <strong>{a.summary}</strong>
      </p>
      {#if a.detail && a.detail.trim() !== a.summary.trim()}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (it scrolls: the keyboard reaches it) -->
        <pre class="adetail" tabindex="0" aria-label="What exactly it would do">{a.detail}</pre>
      {/if}
      <p class="awhy">{a.tool}{a.tool ? ' · ' : ''}Only you can allow it, and only for this one step.</p>
      <div class="abtns">
        <button type="button" class="aallow" onclick={() => answerApproval(a.id, true)}>Allow once</button>
        <button type="button" class="adeny" onclick={() => answerApproval(a.id, false)}>Deny</button>
      </div>
    </div>
  </div>
{/each}

<style>
  .japprove {
    align-self: flex-start;
    display: flex;
    gap: 10px;
    box-sizing: border-box;
    width: min(560px, 100%);
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 45%, var(--yap-paper-rule));
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-warning) 14%, transparent);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .aicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-warning);
  }
  .abody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .aq,
  .awhy {
    margin: 0;
  }
  .aq strong {
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .alink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link, var(--yap-primary));
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .alink:hover {
    text-decoration: underline;
  }
  .alink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
    border-radius: 3px;
  }
  .adetail {
    max-height: 160px;
    margin: 6px 0 0;
    padding: 8px 10px;
    overflow: auto;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .adetail:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .awhy {
    margin-top: 6px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .abtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .abtns button {
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
  .abtns .aallow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .abtns button:hover {
    background: var(--yap-paper-s2);
  }
  .abtns .aallow:hover {
    background: var(--yap-primary-hover);
  }
  .abtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
