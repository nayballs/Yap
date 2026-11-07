<script>
  // **The jobs running now**, above Chat's conversation list (Jobs,
  // stores/jobs.svelte.js): a row each — the folder, what it's doing now
  // (or "Waiting for you" while a step waits to be allowed), and how long it
  // has run; a click opens its chat and its Job tab. Nothing while none run.
  import { onMount } from 'svelte';
  import { jobs } from '../../stores/jobs.svelte.js';
  import { isLive, statusWord, statusTone, elapsed, turnNow } from '../../core/jobs.js';

  /** `chat`: the view's state (its conversations and the panel). */
  let { chat } = $props();

  let now = $state(Date.now() / 1000);
  const running = $derived(jobs.list.filter((j) => isLive(j.status)));

  onMount(() => {
    const t = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => clearInterval(t);
  });

  async function openJob(j) {
    if (j.conversation != null && j.conversation !== chat.activeId) await chat.selectConversation(j.conversation);
    chat.doc.showJob(j);
  }

  const lineOf = (j) => (j.status === 'asking' ? 'Waiting for you to allow a step' : jobs.steps[j.id] || statusWord(j.status));
  // How long it has been at its request now: its turn's time (a follow-up's, not the job's first).
  const tookOf = (j, at) => {
    const t = turnNow(j);
    return elapsed(t ? { ...t, status: j.status } : j, at);
  };
</script>

{#if running.length}
  <div class="running" role="group" aria-label="Jobs running" data-testid="chat-running-jobs">
    {#each running as j (j.id)}
      <button type="button" class="jrow" data-testid="chat-running-job" data-status={j.status} onclick={() => openJob(j)} title={j.path || j.folder}>
        <span class="dot tone-{statusTone(j.status)}" aria-hidden="true"></span>
        <span class="jtext">
          <span class="jtop">
            <span class="jfolder">{j.folder}</span>
            {#if tookOf(j, now)}<span class="jtime">{tookOf(j, now)}</span>{/if}
          </span>
          <span class="jstep">{lineOf(j)}</span>
        </span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .running {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0 8px 8px;
    padding: 4px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s1);
  }
  .jrow {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .jrow:hover {
    background: var(--yap-s3);
  }
  .jrow:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    margin-top: 5px;
    border-radius: 50%;
    background: var(--yap-muted);
  }
  .dot.tone-live {
    background: var(--yap-primary);
  }
  .dot.tone-ask {
    background: var(--yap-warning);
  }
  @media (prefers-reduced-motion: no-preference) {
    .dot.tone-live {
      animation: pulse 1.4s ease-in-out infinite;
    }
  }
  :global(:root[data-motion='reduced']) .dot.tone-live {
    animation: none;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .jtext {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1 1 auto;
  }
  .jtop {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .jfolder {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
  }
  .jtime {
    flex: 0 0 auto;
    margin-left: auto;
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .jstep {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
</style>
