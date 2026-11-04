<script>
  // The calendar providers' marks, shared by Settings → Connectors' cards and
  // the "Connect your calendar" dialog's buttons (ConnectCalendarDialog):
  //   'google'   Google Calendar (the Connectors card),
  //   'g'        Google's "G", for Continue with Google (as on the Account page),
  //   'outlook'  Outlook,
  //   'ics'      any calendar by its private iCal link, in the text colour.
  // Sized by the parent (the svg fills its box).
  /** @type {{ kind: 'google' | 'g' | 'outlook' | 'ics' }} */
  let { kind } = $props();
</script>

{#if kind === 'google'}
  <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="3.5" width="18" height="17.5" rx="3" fill="#fff" stroke="#4285f4" stroke-width="1.6" /><path d="M3 8.5h18" stroke="#4285f4" stroke-width="1.6" /><rect x="3.8" y="4.3" width="16.4" height="3.6" rx="1.6" fill="#4285f4" /><path d="M8 12.5h2.6l-1.6 2.3a1.6 1.6 0 1 1-1.3 2.6" fill="none" stroke="#34a853" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /><path d="M14 12.8l1.6-.8v5.6" fill="none" stroke="#ea4335" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
{:else if kind === 'g'}
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.27-4.74 3.27-8.1z" />
    <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23z" />
    <path fill="#FBBC05" d="M5.84 14.1a6.6 6.6 0 0 1 0-4.2V7.06H2.18a11 11 0 0 0 0 9.88l3.66-2.84z" />
    <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1A11 11 0 0 0 2.18 7.06l3.66 2.84C6.71 7.3 9.14 5.38 12 5.38z" />
  </svg>
{:else if kind === 'outlook'}
  <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="3.5" width="18" height="17.5" rx="3" fill="#0078d4" /><path d="M3 8.5h18" stroke="#50a5ec" stroke-width="1.2" /><ellipse cx="12" cy="14.5" rx="3.3" ry="3.6" fill="none" stroke="#fff" stroke-width="1.7" /></svg>
{:else}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3.5" width="18" height="17.5" rx="3" /><path d="M3 8.5h18M8 2v3M16 2v3" /><path d="M10.4 15.6l3.2-3.2M11.7 11.6l.6-.6a1.9 1.9 0 0 1 2.7 2.7l-.6.6M12.3 16.4l-.6.6a1.9 1.9 0 0 1-2.7-2.7l.6-.6" /></svg>
{/if}

<style>
  svg {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
