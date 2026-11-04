<script>
  // What the turn is doing now, at the foot of the thread while it answers
  // (chat/activity.rs), as the Claude Code app shows it: a small moving
  // mark, the step under way and the turn's time — "Searching the web ·
  // tidecaller lone wolf build · 1m 59s", "Reading emberfall.example/wiki/
  // Talents · 6m 30s", "Writing ‘Character creation’", "Thinking" until a
  // step comes. A model on this PC that stepped aside for a dictation says
  // so instead; one slow to start says why (`hint`). Once the answer is in,
  // its steps fold under it (ChatSteps.svelte).
  import { elapsed } from './chatSteps.js';

  /**
   * `step`: the latest `{mark, line}` (none yet: "Thinking"); `waiting`: a
   * dictation is being waited out; `seconds`: the turn's time so far;
   * `hint`: a line under it ("Still working: …"), or ''.
   */
  let { step = null, waiting = false, seconds = 0, hint = '' } = $props();

  const line = $derived(waiting ? 'Waiting for your dictation to finish' : step?.line || 'Thinking');
</script>

<div class="activity thinking" data-testid="chat-activity" data-mark={waiting ? 'waiting' : step?.mark || 'think'}>
  <span class="mark" aria-hidden="true"><i></i><i></i><i></i></span>
  <span class="aline" role="status" title={line}>{line}</span>
  {#if seconds > 0}
    <!-- (Not read out each second.) -->
    <span class="atime" aria-hidden="true">· {elapsed(seconds)}</span>
  {/if}
</div>
{#if hint}
  <p class="ahint" data-testid="chat-still-working" aria-live="polite">{hint}</p>
{/if}

<style>
  .activity {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: 100%;
    min-width: 0;
    margin: 0 0 0 2px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .aline {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .atime {
    flex: 0 0 auto;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
  /* Three amber dots, lit in turn. */
  .mark {
    flex: 0 0 auto;
    display: inline-flex;
    gap: 2.5px;
  }
  .mark i {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--yap-primary);
    opacity: 0.3;
    animation: lit 1.2s ease-in-out infinite;
  }
  .mark i:nth-child(2) {
    animation-delay: 0.2s;
  }
  .mark i:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes lit {
    0%,
    100% {
      opacity: 0.3;
    }
    40% {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mark i {
      animation: none;
      opacity: 0.8;
    }
  }
  :global(:root[data-motion='reduced']) .mark i {
    animation: none;
    opacity: 0.8;
  }
  .ahint {
    align-self: flex-start;
    margin: -4px 0 0 20px;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
</style>
