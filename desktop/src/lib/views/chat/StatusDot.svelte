<script>
  // **A status dot** (core/chatStatus.js, stores/chatStatus.svelte.js), after
  // the Claude Code app's sidebar: a soft pulse while a chat works (the Code
  // teal on the Code side, Yap's accent on Chat), amber with a soft ring while
  // something waits on the person, blue when an answer came while it wasn't
  // on screen. Steady under reduced motion. Named for a screen reader and in
  // its tooltip ("Needs you: an agent waits for you to allow a step").
  //
  // - `inline` (a row's, a folded group's): always there, so it grows in and
  //   fades out (its width and colour glide) and the row never jumps;
  // - `badge` (the Chat | Code switch's, the menu rail's): on a corner of its
  //   icon, only while there's something to say; `decorative` there, its
  //   control saying it instead (`aria-describedby`), so the control's own
  //   name stays "Chat".
  import { statusWords } from '../../core/chatStatus.js';

  /**
   * `s`: `{state, why, count?}`, or null (idle and seen). `code`: working
   * in the Code teal. `place`: "inline" or "badge". `decorative`: hidden
   * from a screen reader (its control describes it). `testid`: its test id.
   */
  let { s = null, code = false, place = 'inline', decorative = false, testid = 'chat-status' } = $props();

  const words = $derived(statusWords(s, s?.count ?? 1));
</script>

{#if place === 'inline' || s?.state}
  <span
    class="sdot {place}"
    class:code
    data-state={s?.state ?? ''}
    data-why={s?.why ?? ''}
    data-testid={testid}
    role={s?.state && !decorative ? 'img' : undefined}
    aria-label={(!decorative && words) || undefined}
    aria-hidden={s?.state && !decorative ? undefined : 'true'}
    title={words || undefined}
  ></span>
{/if}

<style>
  .sdot {
    flex: 0 0 auto;
    box-sizing: border-box;
    height: 7px;
    border-radius: 50%;
    background-color: transparent;
    transition:
      width var(--yap-dur),
      margin var(--yap-dur),
      opacity var(--yap-dur),
      background-color var(--yap-dur),
      box-shadow var(--yap-dur);
  }
  /* A row's: no room taken while there's nothing to say. */
  .sdot.inline {
    width: 0;
    margin: 0;
    opacity: 0;
  }
  .sdot.inline:not([data-state='']) {
    width: 7px;
    margin: 0 4px 0 5px;
    opacity: 1;
  }
  /* A corner of its icon (the switch's segment, the rail's Chat), apart from it by a ring of the surface. */
  .sdot.badge {
    position: absolute;
    top: var(--sdot-top, 2px);
    right: var(--sdot-right, 2px);
    width: 6px;
    height: 6px;
    box-shadow: 0 0 0 1.5px var(--sdot-bg, var(--yap-s1));
    pointer-events: auto;
  }
  .sdot[data-state='working'] {
    background-color: var(--yap-status-working);
  }
  .sdot.code[data-state='working'] {
    background-color: var(--yap-status-working-code);
  }
  .sdot[data-state='needs'] {
    background-color: var(--yap-status-needs);
  }
  .sdot.inline[data-state='needs'] {
    box-shadow: 0 0 0 2.5px var(--yap-status-needs-ring);
  }
  .sdot[data-state='new'] {
    background-color: var(--yap-status-new);
  }
  /* The pulse (working); a steady dot under reduced motion (app.css stops
     every animation then; this never starts one with Windows' own off). */
  @media (prefers-reduced-motion: no-preference) {
    .sdot[data-state='working'] {
      animation: status-pulse 1.6s ease-in-out infinite;
    }
  }
  :global(:root[data-motion='reduced']) .sdot {
    animation: none;
  }
  @keyframes status-pulse {
    0%,
    100% {
      opacity: 1;
      transform: scale(1);
    }
    50% {
      opacity: 0.4;
      transform: scale(0.8);
    }
  }
</style>
