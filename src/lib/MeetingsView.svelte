<script>
  // Meetings — Wispr Flow's Notetaker hub, local-first (calendar.rs via
  // calendar.svelte.js): a header with ◉ Take notes (the meeting shortcut's
  // logic: notes on the call that's on, else a new meeting note; Stop and
  // summarise while one records); today's and the next 7 days' meetings
  // from the connected calendars, three at a time, with Conflict / Maybe
  // labels and their actions (Join + Start from 10 minutes before, Start,
  // Join meeting; Switch while another meeting records) — with no calendar,
  // one grey "No meetings found" card whose Connect calendar opens the
  // "Connect your calendar" dialog; the past meeting notes by day, with a
  // search and "Generate summary" on those without one; and an Ask bar that
  // cycles example questions and hands the question to Chat. Opening a
  // meeting that's still to come makes its note ahead of time (title,
  // attendees, dated to the meeting).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { calendar, clockTime, whenText, syncCalendar, eventAction, openConnectDialog } from './calendar.svelte.js';
  import { noteRequest } from './meetingDetect.svelte.js';
  import { summarise } from './meetingSummary.svelte.js';
  import { chatRequest } from './chatRequest.svelte.js';
  import { formatHotkeySpec } from './hotkeys.js';
  import { toast } from './ui/toast.svelte.js';

  // `onnavigate(view)` switches the main window's view; `onopensettings(section)`.
  let { onnavigate = () => {}, onopensettings = () => {} } = $props();

  const PAGE = 3; // Wispr: three at a time, "Show more"
  const EARLY = 10 * 60; // actions from 10 minutes before the start

  let now = $state(Date.now() / 1000);
  let meeting = $state({ recording: false });
  let todayShown = $state(PAGE);
  let upcomingShown = $state(PAGE);

  let past = $state([]);
  let query = $state('');
  let pastShown = $state(5);
  // The meeting shortcut ("Win + Alt + M"), for Take notes' tooltip.
  let shortcut = $state('');
  let starting = $state(false);

  const ASK_EXAMPLES = [
    'What questions were left unanswered in my last meeting?',
    'Who am I waiting to hear back from?',
    'What did we decide about the launch date?',
    'What are my action items from this week?',
  ];
  let askText = $state('');
  let askIndex = $state(0);

  const connected = $derived(calendar.connections.length > 0);
  const dayKey = (secs) => new Date(secs * 1000).toDateString();
  const todayKey = $derived(new Date(now * 1000).toDateString());
  // Today: what's still to come today, and anything on now.
  const isToday = (e) => e.end > now && (dayKey(e.start) === todayKey || e.start <= now);
  const todays = $derived(calendar.events.filter(isToday));
  const upcoming = $derived(calendar.events.filter((e) => e.end > now && !isToday(e)));

  // Upcoming, grouped by day ("Tomorrow", "Wed 7 Oct"), within what's shown.
  const upcomingGroups = $derived.by(() => {
    const groups = [];
    for (const ev of upcoming.slice(0, upcomingShown)) {
      const label = dayLabel(ev.start);
      const last = groups[groups.length - 1];
      if (last?.label === label) last.events.push(ev);
      else groups.push({ label, events: [ev] });
    }
    return groups;
  });

  function dayLabel(secs) {
    const d = new Date(secs * 1000);
    const tomorrow = new Date(now * 1000);
    tomorrow.setDate(tomorrow.getDate() + 1);
    if (d.toDateString() === tomorrow.toDateString()) return 'Tomorrow';
    return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }

  const todayLabel = $derived(
    new Date(now * 1000).toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' })
  );

  // The note being recorded, and the meeting it belongs to.
  const recordingKey = $derived(
    meeting.recording ? calendar.events.find((e) => e.noteId != null && e.noteId === meeting.noteId)?.key : null
  );

  function live(ev) {
    return now >= ev.start - EARLY && now < ev.end;
  }

  async function refreshPast() {
    try {
      past = (await invoke('calendar_meeting_notes', { query: query.trim() || null })) || [];
    } catch {
      past = [];
    }
  }

  let queryTimer = null;
  function onQuery() {
    clearTimeout(queryTimer);
    queryTimer = setTimeout(() => {
      pastShown = 5;
      refreshPast();
    }, 200);
  }

  function openNote(id) {
    onnavigate('notes');
    noteRequest.pending = { id, stop: false };
  }

  // ◉ Take notes: what the meeting shortcut does (meeting_guard.rs) — notes
  // on the call that's on, else in a new meeting note (both open in Notes);
  // while a meeting records, stop it and write its action plan.
  async function takeNotes() {
    if (starting) return;
    starting = true;
    try {
      await invoke('meeting_shortcut', { origin: 'settings' });
    } catch (e) {
      toast({ title: "Couldn't start meeting notes", description: String(e), variant: 'destructive' });
    } finally {
      // The shortcut takes one press a second.
      setTimeout(() => (starting = false), 1_000);
    }
  }

  // "Generate summary" on a past meeting without one: its action plan, in
  // the note (where it shows being written, or what it needs).
  async function generate(n) {
    try {
      await summarise(n.id);
      openNote(n.id);
    } catch (e) {
      toast({ title: "Couldn't write the summary", description: String(e), variant: 'destructive' });
    }
  }

  // Past meeting notes by day ("Today, 6 Oct"), within what's shown.
  const pastGroups = $derived.by(() => {
    const groups = [];
    for (const n of past.slice(0, pastShown)) {
      const label = pastDay(n.when);
      const last = groups[groups.length - 1];
      if (last?.label === label) last.notes.push(n);
      else groups.push({ label, notes: [n] });
    }
    return groups;
  });

  function ask(e) {
    e?.preventDefault();
    const text = askText.trim() || ASK_EXAMPLES[askIndex];
    askText = '';
    chatRequest.pending = { text, scope: 'meetings' };
    onnavigate('chat');
  }

  /** "Today, 6 Oct", "Yesterday, 5 Oct", "Sat, 3 Oct" (a past note's day). */
  function pastDay(secs) {
    const d = new Date(secs * 1000);
    const today = new Date(now * 1000);
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);
    const date = d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
    if (d.toDateString() === today.toDateString()) return `Today, ${date}`;
    if (d.toDateString() === yesterday.toDateString()) return `Yesterday, ${date}`;
    return d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  }

  onMount(() => {
    refreshPast();
    invoke('meeting_state')
      .then((s) => (meeting = s || { recording: false }))
      .catch(() => {});
    invoke('get_config')
      .then((c) => {
        const keys = c?.meetingHotkey ? formatHotkeySpec(c.meetingHotkey) : '';
        shortcut = keys && keys !== 'None' ? keys : '';
      })
      .catch(() => {});
    const uns = [];
    listen('yap-meeting-state', (e) => {
      meeting = e.payload || { recording: false };
      refreshPast();
    }).then((u) => uns.push(u));
    listen('yap-notes-changed', () => refreshPast()).then((u) => uns.push(u));
    const tick = setInterval(() => (now = Date.now() / 1000), 15_000);
    const cycle = setInterval(() => {
      if (!askText) askIndex = (askIndex + 1) % ASK_EXAMPLES.length;
    }, 4_000);
    return () => {
      clearInterval(tick);
      clearInterval(cycle);
      clearTimeout(queryTimer);
      uns.forEach((u) => u && u());
    };
  });
</script>

{#snippet row(ev)}
  {@const isLive = live(ev)}
  {@const recordingThis = recordingKey === ev.key}
  <div class="mrow" class:live={isLive} role="listitem">
    <button
      class="mopen"
      onclick={() => eventAction(ev, 'open')}
      title={ev.start > now ? 'Open its note, ready for the meeting' : 'Open its note'}
    >
      <span class="mtime">
        <span class="t1">{clockTime(ev.start)}</span>
        <span class="t2">{clockTime(ev.end)}</span>
      </span>
      <span class="mmain">
        <span class="mtitle">
          <span class="mname">{ev.title}</span>
          {#if ev.conflict}<span class="tag conflict">Conflict</span>{/if}
          {#if ev.tentative}<span class="tag maybe">Maybe</span>{/if}
        </span>
        <span class="mmeta">
          {#if isLive}<span class="soon">{whenText(ev.start, now)}</span>{/if}
          {#if ev.serviceLabel}<span>{ev.serviceLabel}</span>{/if}
          {#if ev.with}<span class="with">{ev.with}</span>{/if}
          {#if ev.noteId != null && !recordingThis}<span class="hasnote">Note ready</span>{/if}
        </span>
      </span>
    </button>
    {#if recordingThis}
      <div class="mact">
        <span class="recpill"><span class="recdot" aria-hidden="true"></span>Recording</span>
        <button class="ghost" onclick={() => openNote(meeting.noteId)}>Open note</button>
      </div>
    {:else if isLive}
      <div class="mact">
        {#if meeting.recording}
          {#if ev.hasLink}<button class="ghost" onclick={() => eventAction(ev, 'join')}>Join meeting</button>{/if}
          <button class="ink" onclick={() => eventAction(ev, ev.hasLink ? 'joinSwitch' : 'switch')}>
            {ev.hasLink ? 'Join + Switch' : 'Switch notes'}
          </button>
        {:else if ev.hasLink}
          <button class="ghost" onclick={() => eventAction(ev, 'join')}>Join meeting</button>
          <button class="soft" onclick={() => eventAction(ev, 'start')}>Start</button>
          <button class="ink" onclick={() => eventAction(ev, 'joinStart')}>Join + Start</button>
        {:else}
          <button class="ink" onclick={() => eventAction(ev, 'start')}>Start</button>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

<div class="meetings">
  <div class="scroll">
    <div class="wrap">
      <header class="head">
        <h1 tabindex="-1" data-dialog-return>Meetings</h1>
        <div class="headbtns">
          {#if connected}
            <button
              class="hbtn icononly"
              onclick={syncCalendar}
              disabled={calendar.syncing}
              aria-label="Sync calendar"
              title={calendar.syncing ? 'Syncing…' : 'Sync calendar'}
            >
              <svg class:spin={calendar.syncing} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 21v-5h5" /><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5" /></svg>
            </button>
          {/if}
          <button class="hbtn icononly" onclick={() => onopensettings('general#meetings')} aria-label="Meeting settings" title="Meeting settings">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
          </button>
          <!-- Wispr's "◉ Start Notetaker": the meeting shortcut, as a button. -->
          <button
            class="takenotes"
            class:rec={meeting.recording}
            onclick={takeNotes}
            disabled={starting}
            title={meeting.recording
              ? `Stop the meeting and write its action plan${shortcut ? ` (${shortcut})` : ''}`
              : `Take notes on the call you're on, or start a meeting note${shortcut ? ` (${shortcut})` : ''}`}
          >
            {#if meeting.recording}
              <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6.5" y="6.5" width="11" height="11" rx="2.5" fill="currentColor" /></svg>
              <span>Stop and summarise</span>
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="7.5" /><circle cx="12" cy="12" r="3.4" fill="currentColor" stroke="none" /></svg>
              <span>Take notes</span>
            {/if}
          </button>
        </div>
      </header>

      <section class="sec" aria-labelledby="mt-today">
        <div class="seccap"><h2 id="mt-today">Today</h2><span class="secdate">{todayLabel}</span></div>
        {#if !connected}
          <!-- Wispr's one grey card: the way in is the "Connect your calendar" dialog. -->
          <div class="empty">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="16" rx="2.5" /><path d="M3.5 9.5h17M8 2.8v3.4M16 2.8v3.4" /><path d="M8 13h.01M12 13h.01M16 13h.01M8 16.5h.01M12 16.5h.01" stroke-width="2.2" /></svg>
            <p class="e1">No meetings found</p>
            <button class="ink connect" onclick={openConnectDialog}>Connect calendar</button>
          </div>
        {:else if todays.length === 0}
          <p class="quiet">Nothing else on your calendar today.</p>
        {:else}
          <div class="mlist" role="list" aria-label="Today's meetings">
            {#each todays.slice(0, todayShown) as ev (ev.key)}
              {@render row(ev)}
            {/each}
          </div>
          {#if todays.length > todayShown}
            <button class="more" onclick={() => (todayShown += PAGE)}>Show more</button>
          {/if}
        {/if}
      </section>

      {#if connected}
        <section class="sec" aria-labelledby="mt-upcoming">
          <div class="seccap"><h2 id="mt-upcoming">Upcoming</h2><span class="secdate">Next 7 days</span></div>
          {#if upcoming.length === 0}
            <p class="quiet">No meetings in the next 7 days.</p>
          {:else}
            {#each upcomingGroups as g (g.label)}
              <div class="daylabel">{g.label}</div>
              <div class="mlist" role="list" aria-label={`Meetings ${g.label}`}>
                {#each g.events as ev (ev.key)}
                  {@render row(ev)}
                {/each}
              </div>
            {/each}
            {#if upcoming.length > upcomingShown}
              <button class="more" onclick={() => (upcomingShown += PAGE)}>Show more</button>
            {/if}
          {/if}
        </section>
      {/if}

      <section class="sec" aria-labelledby="mt-past">
        <div class="pasthead">
          <h2 id="mt-past" class="h2big">Past meeting notes</h2>
          <label class="search">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
            <input type="search" placeholder="Search meetings" aria-label="Search meetings" bind:value={query} oninput={onQuery} />
          </label>
        </div>
        {#if past.length === 0}
          <p class="quiet">{query.trim() ? 'No meeting notes match that.' : 'Your meeting notes will appear here.'}</p>
        {:else}
          {#each pastGroups as g (g.label)}
            <div class="pday">{g.label}</div>
            <div class="plist" role="list" aria-label={`Meeting notes, ${g.label}`}>
              {#each g.notes as n (n.id)}
                <!-- The title opens the note (and the whole row, for the
                     pointer); "Generate summary" is a button of its own. -->
                <div class="prow" role="listitem">
                  <span class="picon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
                  </span>
                  <span class="pmain">
                    <span class="ptitle">
                      <button class="popen" onclick={() => openNote(n.id)}>{n.title || 'Untitled meeting'}</button>
                      {#if n.recording}<span class="recpill small"><span class="recdot" aria-hidden="true"></span>Recording</span>{/if}
                      {#if n.hasPlan}<span class="tag plan">Action plan</span>{/if}
                    </span>
                    <span class="pmeta">
                      <span>{clockTime(n.when)}</span>
                      {#if n.participants?.length}<span class="pwho">{n.participants.join(', ')}</span>{/if}
                      {#if !n.hasPlan && !n.recording && n.hasTalk}
                        <span><button class="gen" onclick={() => generate(n)}>Generate summary</button></span>
                      {/if}
                    </span>
                  </span>
                </div>
              {/each}
            </div>
          {/each}
          {#if past.length > pastShown}
            <button class="more" onclick={() => (pastShown += 10)}>View more</button>
          {/if}
        {/if}
      </section>
    </div>
  </div>

  <!-- Wispr's Ask bar: example questions cycle in the placeholder; the
       question opens in Chat, grounded in your meeting notes. -->
  <form class="askbar" onsubmit={ask}>
    <svg class="askicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" /><path d="M19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8z" /></svg>
    <input placeholder={ASK_EXAMPLES[askIndex]} aria-label="Ask about your meetings" bind:value={askText} />
    <button type="button" class="pastchats" onclick={() => onnavigate('chat')}>
      Past chats
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 17 17 7M8 7h9v9" /></svg>
    </button>
  </form>
</div>

<style>
  .meetings {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .wrap {
    max-width: 780px;
    margin: 0 auto;
    padding: 30px 36px 24px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 20px;
  }
  .head h1 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 30px;
    font-weight: 550;
    letter-spacing: -0.01em;
  }
  .headbtns {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .hbtn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--yap-dur) ease, border-color var(--yap-dur) ease;
  }
  .hbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    background: var(--yap-s3);
  }
  .hbtn:disabled {
    cursor: default;
    color: var(--yap-muted);
  }
  /* Icon buttons (Wispr's gear): no box until hovered. */
  .hbtn.icononly {
    width: 32px;
    padding: 0;
    justify-content: center;
    border-color: transparent;
    background: none;
    color: var(--yap-fg-80);
  }
  .hbtn.icononly:hover:not(:disabled) {
    border-color: transparent;
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .hbtn svg {
    width: 17px;
    height: 17px;
  }
  .hbtn:focus-visible,
  .takenotes:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  /* Wispr's beige "◉ Start Notetaker" (radius 8, 32 tall). */
  .takenotes {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    margin-left: 2px;
    padding: 0 12px 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-paper-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .takenotes:hover:not(:disabled) {
    background: var(--yap-paper-s2-strong);
  }
  .takenotes:disabled {
    cursor: default;
  }
  .takenotes svg {
    width: 15px;
    height: 15px;
  }
  /* Recording: a stop square in the notepad's emerald. */
  .takenotes.rec svg {
    color: var(--yap-live-strong);
  }
  .spin {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .sec {
    margin-bottom: 28px;
  }
  .seccap {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin: 0 0 10px;
    padding-left: 2px;
  }
  .seccap h2 {
    margin: 0;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--yap-muted-70);
  }
  .secdate {
    font-size: 12px;
    color: var(--yap-muted-55);
  }
  .daylabel {
    margin: 14px 0 6px 2px;
    font-size: 12px;
    font-weight: 650;
    color: var(--yap-fg-62);
  }

  .mlist {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mrow {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px 4px 4px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    transition: border-color var(--yap-dur) ease, box-shadow var(--yap-dur) ease;
  }
  .mrow:hover {
    border-color: var(--yap-border-hover);
  }
  .mrow.live {
    border-color: var(--yap-primary-line);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .mopen {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 9px 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mtime {
    flex: 0 0 52px;
    display: flex;
    flex-direction: column;
    font-variant-numeric: tabular-nums;
  }
  .t1 {
    font-size: 13px;
    font-weight: 650;
  }
  .t2 {
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .mmain {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .mtitle {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .mname {
    font-size: 13.5px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mmeta {
    display: flex;
    flex-wrap: wrap;
    gap: 0 8px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .mmeta > span + span::before {
    content: '·';
    margin-right: 8px;
    color: var(--yap-muted-55);
  }
  .soon {
    color: var(--yap-primary-hover);
    font-weight: 650;
  }
  .hasnote {
    color: var(--yap-success);
  }
  .tag {
    flex: 0 0 auto;
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .tag.conflict {
    color: var(--yap-danger);
    background: color-mix(in srgb, var(--yap-danger) 10%, transparent);
  }
  .tag.maybe {
    color: var(--yap-warning);
    background: color-mix(in srgb, var(--yap-warning) 12%, transparent);
  }
  .tag.plan {
    color: var(--yap-success);
    background: color-mix(in srgb, var(--yap-success) 10%, transparent);
  }
  .mact {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ink,
  .soft,
  .ghost {
    height: 30px;
    padding: 0 12px;
    border-radius: var(--yap-r);
    font: inherit;
    font-size: 12.5px;
    font-weight: 650;
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--yap-dur) ease;
  }
  .ink {
    border: none;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .ink:hover {
    background: var(--yap-ink-hover);
  }
  .soft {
    border: 1px solid var(--yap-border);
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .soft:hover {
    background: var(--yap-s3);
  }
  .ghost {
    border: none;
    background: none;
    color: var(--yap-fg-80);
  }
  .ghost:hover {
    background: var(--yap-s3);
  }
  .recpill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    border-radius: var(--yap-r-full);
    background: color-mix(in srgb, var(--yap-danger) 9%, transparent);
    color: var(--yap-danger);
    font-size: 11.5px;
    font-weight: 700;
  }
  .recpill.small {
    padding: 1px 7px;
    font-size: 10.5px;
  }
  .recdot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    animation: pulse 1.4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .more {
    margin-top: 8px;
    padding: 4px 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 650;
    cursor: pointer;
  }
  .more:hover {
    background: var(--yap-s3);
  }
  .quiet {
    margin: 4px 2px;
    font-size: 13px;
    color: var(--yap-muted);
  }
  /* No calendar: Wispr's one grey card (no meetings, Connect calendar). */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 30px 20px 32px;
    border-radius: 16px;
    background: var(--yap-paper-s1);
    text-align: center;
  }
  .empty svg {
    width: 26px;
    height: 26px;
    color: var(--yap-paper-muted);
  }
  .empty .e1 {
    margin: 2px 0 6px;
    font-size: 16px;
    font-weight: 500;
    color: var(--yap-paper-muted);
  }
  .ink.connect {
    height: 34px;
    padding: 0 16px;
    font-size: 13.5px;
  }
  .ink:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px var(--yap-s1), 0 0 0 4px var(--yap-ink);
  }

  .pasthead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .h2big {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
  }
  .search {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-muted);
  }
  .search:focus-within {
    border-color: var(--yap-border-hover);
  }
  .search svg {
    width: 14px;
    height: 14px;
  }
  .search input {
    width: 180px;
    border: none;
    outline: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  /* Past notes by day ("TODAY, 6 OCT"), as Wispr's. */
  .pday {
    margin: 16px 0 4px 8px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--yap-muted-70);
  }
  .pasthead + .pday {
    margin-top: 4px;
  }
  .plist {
    display: flex;
    flex-direction: column;
  }
  .prow {
    position: relative;
    display: flex;
    width: 100%;
    box-sizing: border-box;
    align-items: center;
    gap: 12px;
    padding: 9px 8px;
    border-radius: var(--yap-r);
    transition: background var(--yap-dur) ease;
  }
  .prow:hover {
    background: var(--yap-s3);
  }
  .prow:has(.popen:focus-visible) {
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .picon {
    flex: 0 0 auto;
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border-radius: var(--yap-r);
    background: var(--yap-raised-soft);
    color: var(--yap-muted);
  }
  .picon svg {
    width: 15px;
    height: 15px;
  }
  .pmain {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .ptitle {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  /* The title opens the note; its ::after stretches over the row, so the
     whole row does (Generate summary sits above it). */
  .popen {
    min-width: 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    font-weight: 600;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .popen::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
  }
  .popen:focus-visible {
    outline: none;
  }
  .pmeta {
    display: flex;
    align-items: baseline;
    min-width: 0;
    gap: 0 6px;
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .pmeta > span + span::before {
    content: '·';
    margin-right: 6px;
    color: var(--yap-muted-55);
  }
  .pwho {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gen {
    position: relative;
    z-index: 1;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: var(--yap-muted-55);
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .gen:hover {
    color: var(--yap-fg);
    text-decoration-color: currentColor;
  }
  .gen:focus-visible {
    outline: none;
    border-radius: 3px;
    box-shadow: 0 0 0 2px var(--yap-primary-wash);
  }

  /* The Ask bar, docked to the bottom of the view (Wispr's hub). */
  .askbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 auto 14px;
    width: min(780px, calc(100% - 48px));
    box-sizing: border-box;
    padding: 6px 6px 6px 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
  }
  .askicon {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    color: var(--yap-primary);
  }
  .askbar input {
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    border: none;
    outline: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
  }
  .askbar input::placeholder {
    color: var(--yap-muted-70);
  }
  .pastchats {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 11px;
    border: none;
    border-radius: var(--yap-r-full);
    background: var(--yap-s3);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12px;
    font-weight: 650;
    cursor: pointer;
  }
  .pastchats:hover {
    background: var(--yap-raised);
  }
  .pastchats svg {
    width: 12px;
    height: 12px;
  }
</style>
