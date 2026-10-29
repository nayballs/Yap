<script>
  // Home's meetings (after Wispr Flow's Home): what's coming up from the
  // connected calendars (stores/calendar.svelte.js) and the latest meeting
  // notes (homeMeetings.svelte.js), three of each. A meeting to come opens
  // its note (made ahead of time, as the Meetings view does); a past one
  // opens its note in Notes. See all goes to Meetings. HomeView shows it
  // only to someone with a calendar connected or a meeting noted.
  import { calendar, clockTime, eventAction } from '../../stores/calendar.svelte.js';
  import { homeMeetings, SHOWN } from './homeMeetings.svelte.js';

  let { onnavigate = null, onopennote = null } = $props();

  let now = $state(Date.now() / 1000);
  $effect(() => {
    const tick = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(tick);
  });

  const connected = $derived(calendar.connections.length > 0);
  const upcoming = $derived(calendar.events.filter((e) => e.end > now).slice(0, SHOWN));
  const past = $derived(homeMeetings.past);
  // Upcoming first when there's something coming (else what happened).
  let picked = $state(null);
  const tab = $derived(picked ?? (connected && (upcoming.length || !past.length) ? 'upcoming' : 'past'));

  function dayWord(secs) {
    const d = new Date(secs * 1000);
    const today = new Date(now * 1000);
    const other = new Date(today);
    other.setDate(today.getDate() + 1);
    if (d.toDateString() === today.toDateString()) return 'Today';
    if (d.toDateString() === other.toDateString()) return 'Tomorrow';
    other.setDate(today.getDate() - 1);
    if (d.toDateString() === other.toDateString()) return 'Yesterday';
    return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }
  /** "Now", "Today · 14:30", "Wed 4 Nov · 10:00". */
  const upcomingWhen = (ev) => (ev.start <= now ? 'Now' : `${dayWord(ev.start)} · ${clockTime(ev.start)}`);
  /** "Recording", "Yesterday", "Mon 26 Oct". */
  const pastWhen = (n) => (n.recording ? 'Recording now' : dayWord(n.when));

  function openUpcoming(ev) {
    eventAction(ev, 'open');
  }
  function openPast(n) {
    onopennote?.({ id: n.id });
  }

  function onTabKey(e) {
    if (!connected || !['ArrowLeft', 'ArrowRight'].includes(e.key)) return;
    e.preventDefault();
    picked = tab === 'upcoming' ? 'past' : 'upcoming';
    e.currentTarget.parentElement?.querySelector(`[data-tab="${picked}"]`)?.focus();
  }
</script>

<section class="mcard" aria-labelledby="home-meetings-title">
  <div class="mhead">
    <h2 id="home-meetings-title">Meetings</h2>
    <button class="seeall" onclick={() => onnavigate?.('meetings')}>See all</button>
  </div>
  {#if connected}
    <div class="mtabs" role="tablist" aria-label="Meetings">
      {#each [['upcoming', 'Upcoming'], ['past', 'Past']] as [id, label] (id)}
        <button
          class="mtab"
          class:on={tab === id}
          role="tab"
          data-tab={id}
          aria-selected={tab === id}
          aria-controls="home-meetings-list"
          tabindex={tab === id ? 0 : -1}
          onclick={() => (picked = id)}
          onkeydown={onTabKey}>{label}</button
        >
      {/each}
    </div>
  {/if}
  <div class="mlist" id="home-meetings-list" role={connected ? 'tabpanel' : undefined}>
    {#if tab === 'upcoming'}
      {#each upcoming as ev (ev.key)}
        <button class="mrow" onclick={() => openUpcoming(ev)} title="Open its note">
          <span class="mtitle">{ev.title || 'Untitled meeting'}</span>
          <span class="mwhen" class:live={ev.start <= now}>{upcomingWhen(ev)}</span>
        </button>
      {:else}
        <p class="mempty">Nothing on your calendar in the next 7 days.</p>
      {/each}
    {:else}
      {#each past as n (n.id)}
        <button class="mrow" onclick={() => openPast(n)} title="Open its note">
          <span class="mtitle">{n.title || 'Untitled meeting'}</span>
          <span class="mwhen" class:live={n.recording}>{pastWhen(n)}</span>
        </button>
      {:else}
        <p class="mempty">No meeting notes yet.</p>
      {/each}
    {/if}
  </div>
</section>

<style>
  .mcard {
    padding: 16px 16px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .mhead {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    padding: 0 6px;
    margin-bottom: 8px;
  }
  .mhead h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 650;
    color: var(--yap-fg);
  }
  .seeall {
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: 12px;
    font-weight: 550;
    color: var(--yap-link);
    cursor: pointer;
  }
  .seeall:hover {
    text-decoration: underline;
  }
  .mtabs {
    display: flex;
    gap: 4px;
    padding: 0 6px;
    margin-bottom: 6px;
  }
  .mtab {
    padding: 3px 10px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    font: inherit;
    font-size: 12px;
    font-weight: 550;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .mtab:hover {
    color: var(--yap-fg);
  }
  .mtab.on {
    border-color: var(--yap-border);
    background: var(--yap-raised);
    color: var(--yap-fg);
  }
  .mlist {
    display: flex;
    flex-direction: column;
  }
  .mrow {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: 7px 6px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mrow:hover {
    background: var(--yap-raised-soft);
  }
  .mtitle {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 550;
    color: var(--yap-fg);
  }
  .mwhen {
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .mwhen.live {
    color: var(--yap-live-strong);
    font-weight: 600;
  }
  .mempty {
    margin: 4px 6px 8px;
    font-size: 12px;
    color: var(--yap-muted);
  }
</style>
