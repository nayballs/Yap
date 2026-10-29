<script>
  // Setup's calendar step, for meeting notes: what a connected calendar
  // gives (a heads-up before each meeting, one-click join, names in the
  // notes) and Connect calendar, which opens the main window's own "Connect
  // your calendar" dialog over the setup (ConnectCalendarDialog, mounted in
  // Onboarding.svelte). Connected, or skipped with the footer's Skip for now,
  // the step never shows again (shell/guide.rs). The calendar's state comes
  // from calendar.rs's snapshot, asked again every 1.5 s while the step is
  // up (this window's events can go quiet: see rawListen in setupState).
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { calendar, connectDialog, openConnectDialog } from '../stores/calendar.svelte.js';
  import CalendarMark from '../components/CalendarMark.svelte';
  import StepHeader from './StepHeader.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();

  let answered = false;

  async function read() {
    try {
      const s = await invoke('calendar_status');
      if (!s) return;
      const before = calendar.connections.length;
      // The snapshot as the main window keeps it; its reminder card is the
      // main window's or the bar's, never shown here.
      Object.assign(calendar, s, { loaded: true, card: null });
      const now = calendar.connections.length;
      if (now > before && connectDialog.open && !connectDialog.done) {
        const c = calendar.connections.at(-1);
        connectDialog.done = { kind: c?.kind, label: c?.label, meetings: calendar.events?.length ?? null };
      }
      if (now > 0 && !answered) {
        answered = true;
        setup.answerCalendar('connected');
      }
    } catch {
      // stub / best-effort
    }
  }

  onMount(() => {
    read();
    const t = setInterval(read, 1500);
    return () => clearInterval(t);
  });

  const connected = $derived(calendar.connections.length > 0);
  const meetings = $derived(calendar.events?.length ?? 0);
</script>

<StepHeader title="Meeting notes, with everyone's names">
  Connect your calendar and Yap gives you a heads-up before each meeting,
  joins in one click and puts the right names in your notes. Read only: Yap
  never changes your calendar.
</StepHeader>

<div class="cal">
  <ul class="gives">
    <li><span class="dot" aria-hidden="true"></span>A reminder a minute before, with Join &amp; take notes</li>
    <li><span class="dot" aria-hidden="true"></span>Notes named after the meeting, with who was there</li>
    <li><span class="dot" aria-hidden="true"></span>A brief before you go in: last time, open tasks</li>
  </ul>

  {#if connected}
    <p class="done" role="status" data-testid="calendar-connected">
      ✓ Calendar connected{#if calendar.connections[0]?.label}: {calendar.connections[0].label}{/if}
      · {meetings === 1 ? '1 meeting' : `${meetings} meetings`} coming up
    </p>
  {:else}
    <button type="button" class="connect" onclick={openConnectDialog}>
      <span class="marks" aria-hidden="true">
        <CalendarMark kind="g" />
        <CalendarMark kind="outlook" />
      </span>
      Connect calendar
    </button>
    <p class="later">Google, Outlook, iCloud or any calendar's private link. You can do it later in Meetings.</p>
  {/if}
</div>

<style>
  .cal {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 18px;
    padding-top: 6px;
  }
  .gives {
    list-style: none;
    margin: 0;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s2);
    font-size: 13px;
    color: var(--yap-fg-80);
  }
  .gives li {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .dot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-primary);
  }
  .connect {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 11px 20px;
    border: none;
    border-radius: 9px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .connect:hover {
    background: var(--yap-ink-hover);
  }
  .marks {
    display: inline-flex;
    gap: 4px;
  }
  .marks :global(svg) {
    width: 16px;
    height: 16px;
  }
  .later {
    margin: 0;
    max-width: 380px;
    text-align: center;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .done {
    margin: 0;
    font-size: 13.5px;
    color: var(--yap-success);
  }
</style>
