<script>
  // Where an answer came from, under it: a web page opens in the browser;
  // a passage of the person's meetings opens its meeting at the line; the
  // window's text read on this PC opens nothing. Compact (2026-10-31): each
  // page once, the ones the answer or its document cites and the pages read
  // first, a few of them and "+N more" for the rest (sources.js).
  import { openExternalLink } from '../../core/externalLinks.js';
  import { sourceRow } from './sources.js';

  /** `m`: the answer; `onsource(s)` opens a passage or a meeting. */
  let { m, onsource = null } = $props();

  // A passage of the person's meetings and notes, or a meeting on their
  // calendar (chat/tools/calendar.rs), not a web page.
  const isPassage = (s) => (s?.note != null || !!s?.event) && !s?.url;
  const fromMeetings = $derived((m.sources?.length ?? 0) > 0 && m.sources.every(isPassage));
  const fromCalendar = $derived(fromMeetings && m.sources.every((s) => !!s.event));
  const label = $derived(fromCalendar ? 'From your calendar' : fromMeetings ? 'From your meetings' : 'Sources');

  const row = $derived(sourceRow(m.sources, m.text));
  let open = $state(false);
  const shown = $derived(open ? row.all : row.few);
</script>

<div class="sources" role="group" aria-label={label} data-testid="chat-sources">
  <span class="srclabel">{label}</span>
  {#each shown as s (s.n)}
    {#if s.screen}
      <!-- The window in front's text, read on this PC (the Voice
           Assistant to a model that can't read images): nothing to open. -->
      <span class="srcchip screen" data-testid="chat-screen-source" title="Yap read the window's text on this PC and sent it with the question">
        <span class="srcn" aria-hidden="true">{s.n}</span>
        <span class="srctitle">{s.title}</span>
      </span>
    {:else if isPassage(s)}
      <button
        class="srcchip passage"
        class:meeting={!!s.event}
        data-testid={s.event ? 'chat-calendar-source' : undefined}
        title={s.quote || s.title}
        aria-label={`${s.event ? 'Meeting' : 'Passage'} ${s.n}: ${s.title}${s.site ? `, ${s.site}` : ''}. Opens ${s.event ? 'its notes' : 'it'}.`}
        onclick={() => onsource?.(s)}
      >
        <span class="srcn" aria-hidden="true">{s.n}</span>
        <span class="srctitle">{s.title}</span>
        {#if s.site}<span class="srcsite">{s.site}</span>{/if}
      </button>
    {:else}
      <button
        class="srcchip"
        class:opened={s.opened}
        data-testid="chat-source"
        title={`${s.title && s.title !== s.label ? `${s.title}\n` : ''}${s.opened ? 'Read in full · ' : ''}${s.url}`}
        onclick={() => openExternalLink(s.url)}
      >
        <span class="srcn" aria-hidden="true">{s.n}</span>
        <span class="srctitle">{s.label}</span>
        <!-- (Google Search's links are titled with their site: once is enough.) -->
        {#if s.site && s.site !== s.label}<span class="srcsite">{s.site}</span>{/if}
      </button>
    {/if}
  {/each}
  {#if row.more}
    <button type="button" class="srcmore" data-testid="chat-sources-more" aria-expanded={open} onclick={() => (open = !open)}>
      {open ? 'Show fewer' : `+${row.more} more`}
    </button>
  {/if}
</div>

<style>
  /* The Sources row under a web answer. */
  .sources {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    max-width: 78%;
    margin: -2px 0 0 2px;
  }
  .srclabel {
    margin-right: 2px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .srcchip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 260px;
    padding: 2px 8px 2px 3px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }
  .srcchip:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .srcchip:focus-visible,
  .srcmore:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .srcn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 15px;
    height: 15px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-muted);
    font-size: 9.5px;
    font-weight: 650;
  }
  /* A page the answer read in full: its number in the accent. */
  .srcchip.opened .srcn {
    background: var(--yap-primary-wash);
    color: var(--yap-primary);
  }
  .srctitle {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .srcsite {
    flex: 0 0 auto;
    color: var(--yap-muted-55);
  }
  /* "+N more" / "Show fewer": quiet, the row's own size. */
  .srcmore {
    padding: 2px 8px;
    border: 1px dashed var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: transparent;
    color: var(--yap-muted);
    font: inherit;
    font-size: 10.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .srcmore:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  /* The window's text, read on this PC: a source with nothing to open. */
  .srcchip.screen {
    cursor: default;
  }
  .srcchip.screen:hover {
    border-color: var(--yap-border-subtle);
    background: var(--yap-s1);
  }
</style>
