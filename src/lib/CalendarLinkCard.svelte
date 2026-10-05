<script>
  // Integrations → Calendar: a pointer to Settings → Connectors, where the
  // calendars are connected (calendar.rs), with what's connected now.
  import { calendar, openConnectors } from './calendar.svelte.js';

  const connected = $derived(calendar.connections.map((c) => c.label));
</script>

<div class="card">
  <div class="card-top">
    <div class="card-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4.5" width="18" height="16" rx="2.5" /><path d="M3 9.5h18M8 2.5v4M16 2.5v4" /></svg>
    </div>
    <div class="card-head">
      <div class="titlerow">
        <h2>Calendar</h2>
        {#if connected.length}<span class="badge on">Connected</span>{/if}
      </div>
      <p>
        {#if connected.length}
          {connected.join(', ')}. Reminders before meetings, and meeting notes with their names and attendees.
        {:else}
          Google Calendar, Outlook or any iCal link: reminders before meetings, and meeting notes with their names and attendees. Read straight from this PC.
        {/if}
      </p>
    </div>
    <button class="go" onclick={openConnectors}>{connected.length ? 'Manage' : 'Connect'}</button>
  </div>
</div>

<style>
  .card {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 16px;
    margin-bottom: 14px;
  }
  .card-top {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .card-icon {
    width: 34px;
    height: 34px;
    flex: 0 0 auto;
    border-radius: 8px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--yap-muted);
  }
  .card-icon svg {
    width: 17px;
    height: 17px;
  }
  .card-head {
    flex: 1 1 auto;
    min-width: 0;
  }
  .titlerow {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-head h2 {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
  }
  .card-head p {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .badge {
    font-size: 10px;
    padding: 2px 7px;
    border-radius: 999px;
    border: 1px solid var(--yap-border);
    white-space: nowrap;
  }
  .badge.on {
    color: var(--yap-success);
    border-color: color-mix(in srgb, var(--yap-success) 35%, transparent);
    background: color-mix(in srgb, var(--yap-success) 10%, transparent);
  }
  .go {
    flex: 0 0 auto;
    background: var(--yap-ink, var(--yap-primary));
    border: none;
    color: var(--yap-ink-fg, #fff);
    border-radius: var(--yap-r);
    padding: 7px 14px;
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }
  .go:hover {
    filter: brightness(1.08);
  }
</style>
