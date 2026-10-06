<script>
  // Settings → Connectors (Wispr Flow's Connectors page, local-first): one
  // rounded card per calendar source, each with its Connect button and, once
  // connected, a row per calendar with a ⋯ menu (Sync now / Disconnect).
  // - Google Calendar: one click (calendar.rs → calendar/google.rs: the
  //   system browser, PKCE, two read-only scopes).
  // - Outlook Calendar: Yap has no Microsoft app, so Connect walks through
  //   publishing the calendar and pasting its ICS link.
  // - Other calendar: any private iCal link (iCloud, Fastmail, Google's
  //   secret address…).
  // Links and tokens go to Windows Credential Manager, never the page: the
  // snapshot only carries each calendar's name and its link's host. Last, a
  // row to the MCP page (`yap-settings-goto` 'mcp').
  import { invoke } from '@tauri-apps/api/core';
  import { calendar } from './calendar.svelte.js';
  import { toast } from './ui/toast.svelte.js';
  import CalendarMark from './CalendarMark.svelte';
  import claudeLogo from '../assets/providers/claude.svg';
  import openaiLogo from '../assets/providers/openai.svg';
  import geminiLogo from '../assets/providers/gemini.svg';

  // The link form that's open: 'outlook' | 'ics' | null.
  let form = $state(null);
  let link = $state('');
  let adding = $state(false);
  let formError = $state('');
  // The connection whose ⋯ menu is open.
  let menu = $state(null);
  let googleExplain = $state(false);
  let now = $state(Date.now() / 1000);

  const google = $derived(calendar.connections.filter((c) => c.kind === 'google'));
  const outlook = $derived(calendar.connections.filter((c) => c.kind === 'outlook'));
  const other = $derived(calendar.connections.filter((c) => c.kind === 'ics'));
  const full = $derived(calendar.connections.length >= calendar.maxConnections);

  $effect(() => {
    const t = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(t);
  });

  async function connectGoogle() {
    if (!calendar.google.available) {
      googleExplain = true;
      return;
    }
    try {
      await invoke('calendar_connect_google');
    } catch (e) {
      toast({ title: "Couldn't connect Google Calendar", description: String(e), variant: 'destructive' });
    }
  }

  function cancelGoogle() {
    invoke('calendar_cancel_google').catch(() => {});
  }

  function openForm(kind) {
    form = form === kind ? null : kind;
    link = '';
    formError = '';
  }

  async function addLink() {
    if (!link.trim() || adding) return;
    adding = true;
    formError = '';
    try {
      await invoke('calendar_add_link', { link: link.trim(), kind: form });
      link = '';
      form = null;
    } catch (e) {
      formError = String(e);
    } finally {
      adding = false;
    }
  }

  async function syncNow() {
    menu = null;
    try {
      await invoke('calendar_sync');
    } catch (e) {
      toast({ title: "Couldn't sync", description: String(e), variant: 'destructive' });
    }
  }

  async function disconnect(c) {
    menu = null;
    try {
      await invoke('calendar_disconnect', { id: c.id });
      toast({ title: `Disconnected ${c.label}`, description: 'Its meetings are gone from Yap; notes made from them stay.', variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't disconnect", description: String(e), variant: 'destructive' });
    }
  }

  function ago(ts) {
    const s = Math.max(0, now - ts);
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
    return new Date(ts * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  }

  function subline(c) {
    if (c.error) return c.error;
    const where = c.kind === 'google' ? 'Google Calendar' : c.detail;
    const when = c.syncedTs ? `Synced ${ago(c.syncedTs)}` : 'Syncing…';
    return where ? `${where} · ${when}` : when;
  }

  function goMcp() {
    window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'mcp' }));
  }

  function onWindowClick(e) {
    if (menu && !e.target.closest?.('.cmenu-wrap')) menu = null;
  }
</script>

<svelte:window onclick={onWindowClick} />

{#snippet connRow(c)}
  <div class="crow">
    <span class="cdot" class:bad={!!c.error} aria-hidden="true"></span>
    <div class="crow-text">
      <div class="clabel">{c.label}</div>
      <div class="csub" class:err={!!c.error}>{subline(c)}</div>
    </div>
    {#if c.kind === 'google' && c.reconnect}
      <button class="cbtn" onclick={connectGoogle}>Connect again</button>
    {/if}
    <div class="cmenu-wrap">
      <button
        class="dots"
        aria-label={`More options for ${c.label}`}
        aria-haspopup="menu"
        aria-expanded={menu === c.id}
        onclick={() => (menu = menu === c.id ? null : c.id)}
      >
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
      </button>
      {#if menu === c.id}
        <div class="cmenu" role="menu" aria-label={`${c.label} options`}>
          <button role="menuitem" onclick={syncNow} disabled={calendar.syncing}>Sync now</button>
          <button role="menuitem" class="danger" onclick={() => disconnect(c)}>Disconnect</button>
        </div>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet linkForm(kind)}
  <div class="lform">
    {#if kind === 'outlook'}
      <ol class="steps">
        <li>Open Outlook on the web (outlook.office.com, or outlook.live.com for a personal account).</li>
        <li>Go to <b>Settings → Calendar → Shared calendars</b>.</li>
        <li>Under <b>Publish a calendar</b>, pick your calendar and <b>Can view all details</b> (so Yap sees who's invited), then <b>Publish</b>.</li>
        <li>Copy the <b>ICS</b> link and paste it here.</li>
      </ol>
    {:else}
      <ul class="steps plain">
        <li><b>Google Calendar</b>: Settings → your calendar → Integrate calendar → <b>Secret address in iCal format</b>.</li>
        <li><b>iCloud</b>: Calendar → the share icon by a calendar → <b>Public Calendar</b> → Copy Link.</li>
        <li><b>Fastmail and others</b>: the calendar's sharing or export settings, its <b>iCal / ICS</b> link.</li>
      </ul>
    {/if}
    <form
      class="lrow"
      onsubmit={(e) => {
        e.preventDefault();
        addLink();
      }}
    >
      <input
        class="linput"
        type="url"
        placeholder={kind === 'outlook' ? 'https://outlook.office365.com/owa/calendar/…/calendar.ics' : 'https:// or webcal:// …'}
        aria-label={kind === 'outlook' ? 'Outlook calendar ICS link' : 'Calendar iCal link'}
        bind:value={link}
        disabled={adding}
      />
      <button class="cbtn ink" type="submit" disabled={adding || !link.trim()}>{adding ? 'Adding…' : 'Add calendar'}</button>
      <button class="cbtn" type="button" onclick={() => (form = null)}>Cancel</button>
    </form>
    {#if formError}<p class="lerr" role="alert">{formError}</p>{/if}
    <p class="fine">The link works like a password: anyone with it can read the calendar. Yap keeps it in Windows Credential Manager and reads it straight from this PC.</p>
  </div>
{/snippet}

<div class="connectors">
  <div class="ccard" role="group" aria-label="Google Calendar">
    <div class="chead">
      <span class="cicon"><CalendarMark kind="google" /></span>
      <div class="ctext">
        <div class="cname">Google Calendar</div>
        <div class="cdesc">Reminders for Google meetings before they begin</div>
      </div>
      <div class="cright">
        {#if calendar.google.waiting}
          <span class="waiting">Finish in your browser…</span>
          <button class="cbtn" onclick={cancelGoogle}>Cancel</button>
        {:else if google.length === 0}
          <button class="cbtn" class:muted={!calendar.google.available} onclick={connectGoogle} disabled={full}>Connect</button>
        {/if}
      </div>
    </div>
    {#each google as c (c.id)}
      {@render connRow(c)}
    {/each}
    {#if googleExplain && !calendar.google.available}
      <p class="cnote" role="status">
        Google sign-in is in the installed Yap. In this build, use a private iCal link below, under <b>Other calendar</b>.
      </p>
    {/if}
  </div>

  <div class="ccard" role="group" aria-label="Outlook Calendar">
    <div class="chead">
      <span class="cicon"><CalendarMark kind="outlook" /></span>
      <div class="ctext">
        <div class="cname">Outlook Calendar</div>
        <div class="cdesc">Reminders for Outlook meetings before they begin</div>
      </div>
      <div class="cright">
        {#if form !== 'outlook'}
          <button class="cbtn" onclick={() => openForm('outlook')} disabled={full}>{outlook.length ? 'Add another' : 'Connect'}</button>
        {/if}
      </div>
    </div>
    {#each outlook as c (c.id)}
      {@render connRow(c)}
    {/each}
    {#if form === 'outlook'}{@render linkForm('outlook')}{/if}
  </div>

  <div class="ccard" role="group" aria-label="Other calendar">
    <div class="chead">
      <span class="cicon other"><CalendarMark kind="ics" /></span>
      <div class="ctext">
        <div class="cname">Other calendar</div>
        <div class="cdesc">iCloud, Fastmail or any calendar with a private iCal link</div>
      </div>
      <div class="cright">
        {#if form !== 'ics'}
          <button class="cbtn" onclick={() => openForm('ics')} disabled={full}>{other.length ? 'Add another' : 'Connect'}</button>
        {/if}
      </div>
    </div>
    {#each other as c (c.id)}
      {@render connRow(c)}
    {/each}
    {#if form === 'ics'}{@render linkForm('ics')}{/if}
  </div>

  {#if full}
    <p class="fine">That's as many calendars as Yap holds ({calendar.maxConnections}). Disconnect one to add another.</p>
  {/if}

  <div class="mcprow">
    <span class="logos" aria-hidden="true">
      <img src={claudeLogo} alt="" />
      <img src={openaiLogo} alt="" />
      <img src={geminiLogo} alt="" />
    </span>
    <span class="mcptext">Give your AI access to your meeting transcripts and notes</span>
    <button class="cbtn" onclick={goMcp}>Go to MCP</button>
  </div>
</div>

<style>
  .connectors {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 660px;
  }
  /* Wispr's connector cards: rounded, light, the action on the right. */
  .ccard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s3);
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 15px 16px 15px 18px;
  }
  .cicon {
    flex: 0 0 auto;
    display: inline-flex;
    width: 22px;
    height: 22px;
  }
  .cicon.other {
    color: var(--yap-muted);
  }
  .cicon :global(svg) {
    width: 100%;
    height: 100%;
  }
  .ctext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .cname {
    font-size: 14px;
    font-weight: 650;
  }
  .cdesc {
    margin-top: 2px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .cright {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .waiting {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .cbtn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur) ease, border-color var(--yap-dur) ease;
  }
  .cbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .cbtn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .cbtn.muted {
    color: var(--yap-muted);
  }
  .cbtn.ink {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .cbtn.ink:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .cnote {
    margin: 0 18px 14px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }

  .crow {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 10px 10px;
    padding: 9px 10px 9px 12px;
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .cdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .cdot.bad {
    background: var(--yap-danger);
  }
  .crow-text {
    flex: 1 1 auto;
    min-width: 0;
  }
  .clabel {
    font-size: 13px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .csub {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .csub.err {
    color: var(--yap-danger);
  }
  .cmenu-wrap {
    position: relative;
    flex: 0 0 auto;
  }
  .dots {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .dots:hover,
  .dots[aria-expanded='true'] {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .dots svg {
    width: 16px;
    height: 16px;
  }
  .cmenu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    min-width: 150px;
    padding: 5px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
  }
  .cmenu button {
    display: block;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .cmenu button:hover:not(:disabled) {
    background: var(--yap-s3);
  }
  .cmenu button.danger {
    color: var(--yap-danger);
  }

  .lform {
    margin: 0 16px 16px 18px;
    padding-top: 2px;
  }
  .steps {
    margin: 0 0 12px;
    padding-left: 20px;
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--yap-fg-80);
  }
  .steps.plain {
    padding-left: 16px;
  }
  .lrow {
    display: flex;
    gap: 8px;
  }
  .linput {
    flex: 1 1 auto;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .linput:focus {
    outline: none;
    border-color: var(--yap-primary-line);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  .lerr {
    margin: 8px 0 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .fine {
    margin: 8px 0 0;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-muted-70);
  }

  /* The plain row to MCP (not a card), as Wispr's. */
  .mcprow {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 2px 0;
  }
  .logos {
    display: inline-flex;
  }
  .logos img {
    width: 22px;
    height: 22px;
    padding: 3px;
    box-sizing: border-box;
    border-radius: 50%;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .logos img + img {
    margin-left: -5px;
  }
  .mcptext {
    flex: 1 1 auto;
    font-size: 13px;
    color: var(--yap-fg-80);
  }
</style>
