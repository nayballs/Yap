<script>
  // A meeting's brief (calendar/brief.rs, Rust's `Brief`): In short (the
  // meeting's AI, when it wrote some), Last time (earlier meetings with these
  // people, dated; "First meeting with…"), Open items (owner, task, due),
  // From memory, On the invite. Every line carries its sources' numbers as
  // small pills; a pill opens its note in Yap's Notes (memory: Settings →
  // Memory; the invite: the meeting's own note) through
  // `calendar_brief_open`, from any window — the main window, the notepad or
  // the Yap bar.
  //
  //   brief:   the brief.
  //   tone:    'paper' (the main window, the notepad), 'dark' (the in-app
  //            card's toast) or 'bar' (the Yap bar's call card: its colours).
  //   compact: smaller type, for a card.
  import { invoke } from '@tauri-apps/api/core';

  /** @type {{ brief: any, tone?: 'paper' | 'dark' | 'bar', compact?: boolean, onopened?: () => void }} */
  let { brief, tone = 'paper', compact = false, onopened = () => {} } = $props();

  const sourceOf = (n) => brief.sources?.find((s) => s.n === n);

  /** "Thu 16 Oct". */
  function day(secs) {
    return new Date(secs * 1000).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }

  function label(n) {
    const s = sourceOf(n);
    if (!s) return `Source ${n}`;
    if (s.kind === 'note') return `${s.title}${s.date ? `, ${day(s.date)}` : ''}: open the note`;
    if (s.kind === 'memory') return `What Yap remembers about ${s.title}: open Memory`;
    return "The meeting's invite: open its note";
  }

  async function open(n) {
    try {
      await invoke('calendar_brief_open', { key: brief.key, n });
      onopened();
    } catch {
      /* the brief changed under it: the next one has its own sources */
    }
  }
</script>

<div class="brief {tone}" class:compact data-brief={brief.key}>
  {#each brief.parts as part (part.kind)}
    <section class="part" data-kind={part.kind}>
      <h4>
        {#if part.kind === 'summary'}
          <svg class="spark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" /></svg>
        {/if}
        {part.title}
      </h4>
      <ul>
        {#each part.lines as line, i (i)}
          <li class:absence={!line.cites.length} class:task={part.kind === 'open'}>
            {#if part.kind === 'open'}<span class="box" aria-hidden="true"></span>{/if}
            <span class="line">
              {#if line.who}<span class="who">{line.who}{part.kind === 'open' ? '' : ':'}</span>{/if}
              <span class="text">{line.text}</span>
              {#if part.kind === 'last' && line.cites.length && sourceOf(line.cites[0])?.date}
                <span class="when">· {day(sourceOf(line.cites[0]).date)}</span>
              {/if}
              {#if line.due}<span class="due">due {line.due}</span>{/if}
              {#each line.cites as n, j (j)}
                <button class="cite" title={label(n)} aria-label={label(n)} onclick={() => open(n)}>{n}</button>
              {/each}
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  {#if brief.writing}
    <p class="note" aria-live="polite"><span class="dot" aria-hidden="true"></span>Writing a short summary…</p>
  {:else if brief.note}
    <p class="note">{brief.note}</p>
  {/if}
</div>

<style>
  .brief {
    --b-text: var(--yap-paper-ink, #26231c);
    --b-muted: var(--yap-paper-muted, #6e6a5f);
    --b-faint: var(--yap-paper-date, #8a8578);
    --b-pill: var(--yap-paper-s2, #e8e4dc);
    --b-pill-hover: var(--yap-paper-s2-strong, #dcd7cb);
    --b-pill-text: var(--yap-paper-text-2, #4a463c);
    --b-box: var(--yap-paper-faint, #b3ad9f);
    --b-accent: var(--yap-primary, #c2690a);
    display: flex;
    flex-direction: column;
    gap: 12px;
    color: var(--b-text);
    font-size: 14px;
    line-height: 1.45;
  }
  .brief.dark {
    --b-text: #f4efe6;
    --b-muted: rgba(244, 239, 230, 0.66);
    --b-faint: rgba(244, 239, 230, 0.5);
    --b-pill: rgba(244, 239, 230, 0.14);
    --b-pill-hover: rgba(244, 239, 230, 0.24);
    --b-pill-text: rgba(244, 239, 230, 0.86);
    --b-box: rgba(244, 239, 230, 0.4);
    --b-accent: #f0b04a;
  }
  .brief.bar {
    --b-text: var(--bar-text);
    --b-muted: var(--bar-text-muted);
    --b-faint: var(--bar-text-faint);
    --b-pill: var(--bar-surface-hover);
    --b-pill-hover: var(--bar-border);
    --b-pill-text: var(--bar-text-soft);
    --b-box: var(--bar-text-faint);
    --b-accent: var(--bar-accent);
  }
  .brief.compact {
    gap: 9px;
    font-size: 13px;
    line-height: 1.4;
  }
  h4 {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0 0 3px;
    font-size: 11px;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--b-muted);
  }
  .spark {
    width: 12px;
    height: 12px;
    color: var(--b-accent);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  li {
    display: flex;
    align-items: baseline;
    gap: 7px;
  }
  /* The summary's bullets and the other parts' lines: a quiet dot. */
  li:not(.task)::before {
    content: '';
    flex: 0 0 auto;
    width: 4px;
    height: 4px;
    margin: 0 2px 0 1px;
    border-radius: 50%;
    background: var(--b-faint);
    transform: translateY(-3px);
  }
  li.absence {
    color: var(--b-muted);
    font-style: italic;
  }
  .box {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
    border: 1.5px solid var(--b-box);
    border-radius: 3px;
    transform: translateY(1px);
  }
  .line {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .who {
    font-weight: 600;
    margin-right: 4px;
  }
  .task .who::after {
    content: '·';
    margin-left: 4px;
    font-weight: 400;
    color: var(--b-faint);
  }
  .when,
  .due {
    margin-left: 4px;
    color: var(--b-muted);
  }
  .due {
    font-size: 0.92em;
  }
  .cite {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    margin-left: 4px;
    padding: 0 4px;
    border: none;
    border-radius: 8px;
    background: var(--b-pill);
    color: var(--b-pill-text);
    font: inherit;
    font-size: 10.5px;
    font-weight: 650;
    line-height: 1;
    vertical-align: 1px;
    cursor: pointer;
  }
  .cite:hover,
  .cite:focus-visible {
    background: var(--b-pill-hover);
    outline: none;
  }
  .note {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    font-size: 12px;
    color: var(--b-muted);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--b-accent);
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot {
      animation: none;
    }
  }
</style>
