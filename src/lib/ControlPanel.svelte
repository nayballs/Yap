<script>
  // The main window "control panel" — ported from OpenWhispr's ControlPanel.tsx
  // + ControlPanelSidebar.tsx. Yap's main window is no longer a settings dialog:
  // a slim sidebar routes between surfaces (Home = the dictation feed, Chat /
  // Notes / Upload coming with Phase 6-7, Dictionary), and Settings opens as a
  // modal overlay via the cogwheel — exactly OpenWhispr's structure.
  //
  // <Settings> stays ALWAYS MOUNTED (hidden when the modal is closed): its
  // window-level listeners (in-window hotkey fallback — the WebView2-focus
  // gotcha), auto-save effect, and update checker must run for the lifetime of
  // the window, not only while the modal is open.
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';
  import yapIcon from '../assets/yap-icon.png';
  import ToastHost from './ui/ToastHost.svelte';
  import { toast } from './ui/toast.svelte.js';
  import Settings from './Settings.svelte';
  import { attention } from './attention.svelte.js';
  import HomeView from './HomeView.svelte';
  import InsightsView from './InsightsView.svelte';
  import DictionaryView from './DictionaryView.svelte';
  import UploadView from './UploadView.svelte';
  import NotesView from './NotesView.svelte';
  import ChatView from './ChatView.svelte';
  import IntegrationsView from './IntegrationsView.svelte';

  let activeView = $state('home');
  let settingsOpen = $state(false);

  // Sidebar nav (Wispr order: Home, Insights, then the work surfaces).
  const NAV = [
    { id: 'home', label: 'Home' },
    { id: 'insights', label: 'Insights' },
    { id: 'chat', label: 'Chat' },
    { id: 'notes', label: 'Notes' },
    { id: 'upload', label: 'Upload' },
    { id: 'dictionary', label: 'Dictionary' },
    { id: 'integrations', label: 'Integrations' },
  ];

  // Custom window chrome (the settings window is undecorated — Wispr-style
  // warm frame all the way to the top). Buttons mirror the native caption
  // controls; double-click on the drag region toggles maximize (Tauri
  // built-in). Close goes through close() so hide-on-close keeps working.
  const appWindow = getCurrentWindow();
  let maximized = $state(false);
  async function refreshMaximized() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* ignore */
    }
  }

  function openSettings(section = null) {
    settingsOpen = true;
    if (section) {
      window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: section }));
    }
  }

  function onOverlayKeydown(e) {
    if (e.key === 'Escape' && settingsOpen) {
      settingsOpen = false;
      e.stopPropagation();
    }
  }

  // Backend error events (rewrite failures, missing keys, …) surface as
  // destructive toasts in the main window (OpenWhispr-style notifications).
  onMount(() => {
    const uns = [];
    listen('yap-error', (e) => {
      const msg = String(e.payload || 'Something went wrong');
      toast({ title: 'Yap ran into a problem', description: msg, variant: 'destructive' });
    }).then((u) => uns.push(u));
    refreshMaximized();
    appWindow.onResized(() => refreshMaximized()).then((u) => uns.push(u));
    return () => uns.forEach((u) => u && u());
  });
</script>

{#snippet navIcon(id)}
  {#if id === 'home'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 10.5 12 3l9 7.5" /><path d="M5 9.5V21h14V9.5" /><path d="M10 21v-6h4v6" /></svg>
  {:else if id === 'insights'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20V10" /><path d="M10 20V4" /><path d="M16 20v-7" /><path d="M22 20H2" /></svg>
  {:else if id === 'chat'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
  {:else if id === 'notes'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
  {:else if id === 'upload'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M17 8l-5-5-5 5" /><path d="M12 3v12" /></svg>
  {:else if id === 'dictionary'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 4h6a4 4 0 0 1 4 4v12a3 3 0 0 0-3-3H2z" /><path d="M22 4h-6a4 4 0 0 0-4 4v12a3 3 0 0 1 3-3h7z" /></svg>
  {:else if id === 'integrations'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 2v5" /><path d="M15 2v5" /><path d="M6 7h12l-.8 6.4A4 4 0 0 1 13.23 17h-2.46a4 4 0 0 1-3.97-3.6z" /><path d="M12 17v5" /></svg>
  {/if}
{/snippet}

<svelte:window onkeydown={onOverlayKeydown} />

<div class="panel">
  <div class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <img class="brandlogo" data-tauri-drag-region src={yapIcon} alt="" aria-hidden="true" />
      <span class="brandname" data-tauri-drag-region>Yap</span>
    </div>
    <div class="winbtns">
      <button class="winbtn" title="Minimize" aria-label="Minimize" onclick={() => appWindow.minimize()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 12h16" /></svg>
      </button>
      <button class="winbtn" title={maximized ? 'Restore' : 'Maximize'} aria-label={maximized ? 'Restore' : 'Maximize'} onclick={() => appWindow.toggleMaximize()}>
        {#if maximized}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 8V5a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-3" /><rect x="4" y="8" width="12" height="12" rx="1" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><rect x="4.5" y="4.5" width="15" height="15" rx="1.5" /></svg>
        {/if}
      </button>
      <button class="winbtn close" title="Close" aria-label="Close" onclick={() => appWindow.close()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
      </button>
    </div>
  </div>

  <div class="cols">
  <nav class="side">
    <div class="nav">
      {#each NAV as item (item.id)}
        <button
          class="navitem"
          class:active={activeView === item.id}
          onclick={() => (activeView = item.id)}
        >
          <span class="navicon">{@render navIcon(item.id)}</span>
          <span class="navlabel">{item.label}</span>
        </button>
      {/each}
    </div>

    <div class="spacer"></div>

    <div class="bottom">
      <button class="navitem" onclick={() => openSettings()}>
        <span class="navicon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
        </span>
        <span class="navlabel">Settings</span>
        {#if attention.items.length > 0}
          <span class="navbadge" title={attention.items.map((i) => i.label).join(' · ')}>{attention.items.length}</span>
        {/if}
      </button>

      <div class="rule"></div>

      <button class="acct" onclick={() => openSettings('account')}>
        <span class="acct-avatar">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8" r="3.2" /><path d="M5 20a7 7 0 0 1 14 0" /></svg>
        </span>
        <span class="acct-who">
          <span class="l1">Sign in</span>
          <span class="l2">Optional · works offline</span>
        </span>
      </button>
    </div>
  </nav>

  <main class="view">
    {#if activeView === 'home'}
      <HomeView onopensettings={openSettings} onnavigate={(v) => (activeView = v)} />
    {:else if activeView === 'insights'}
      <InsightsView />
    {:else if activeView === 'dictionary'}
      <DictionaryView />
    {:else if activeView === 'chat'}
      <ChatView />
    {:else if activeView === 'notes'}
      <NotesView />
    {:else if activeView === 'upload'}
      <UploadView />
    {:else if activeView === 'integrations'}
      <IntegrationsView />
    {/if}
  </main>
  </div>
</div>

<ToastHost />

<!-- Settings modal overlay. <Settings> is always mounted (see header comment);
     the backdrop just hides it. -->
<div
  class="backdrop"
  class:open={settingsOpen}
  onclick={(e) => e.target === e.currentTarget && (settingsOpen = false)}
  role="presentation"
>
  <div class="card">
    <Settings embedded onclose={() => (settingsOpen = false)} />
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--yap-bg);
    color: var(--yap-fg);
    font-size: 13.5px;
    overflow: hidden;
  }

  /* ---- custom window chrome (undecorated window) ---- */
  .titlebar {
    flex: 0 0 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
    -webkit-user-select: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 0 0 18px;
  }
  .brandlogo {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    pointer-events: none;
  }
  .brandname {
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.01em;
  }
  .winbtns {
    display: flex;
    align-self: stretch;
  }
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    border: none;
    background: none;
    color: var(--yap-fg-62);
    cursor: default;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .winbtn svg {
    width: 14px;
    height: 14px;
  }
  .winbtn:hover {
    background: var(--yap-raised);
    color: var(--yap-fg);
  }
  .winbtn.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  .cols {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
  }

  .side {
    flex: 0 0 200px;
    display: flex;
    flex-direction: column;
    padding: 4px 10px 12px;
    background: var(--yap-bg);
  }

  .nav,
  .bottom {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  /* Wispr-weight nav: labels read in near-ink medium even when inactive. */
  .navitem {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 36px;
    padding: 0 11px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 13.5px;
    font-weight: 550;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .navitem:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  /* Active nav = a white pill lifted off the warm-gray sidebar (Wispr). */
  .navitem.active {
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg);
    font-weight: 650;
  }
  .navitem.active .navicon {
    color: var(--yap-primary);
  }
  .navicon {
    display: inline-flex;
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
  }
  .navicon :global(svg) {
    width: 100%;
    height: 100%;
  }
  /* Wispr-style "needs attention" count on the Settings row. */
  .navbadge {
    flex: 0 0 auto;
    margin-left: auto;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--yap-r-full);
    background: #e5484d;
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
  }

  .spacer {
    flex: 1;
  }
  .rule {
    height: 1px;
    margin: 6px 4px;
    background: var(--yap-border-subtle);
  }

  .acct {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .acct:hover {
    background: var(--yap-s2);
  }
  .acct-avatar {
    display: inline-flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    border: 1px solid var(--yap-border);
    color: var(--yap-muted);
    flex: 0 0 auto;
  }
  .acct-avatar svg {
    width: 14px;
    height: 14px;
  }
  .acct-who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .acct-who .l1 {
    font-size: 12.5px;
    font-weight: 600;
  }
  .acct-who .l2 {
    font-size: 11px;
    color: var(--yap-muted-55);
  }

  /* The content pane is an inset rounded sheet on the warm-gray frame —
     the Wispr hallmark. Children keep managing their own scroll. */
  .view {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    margin: 0 10px 10px 2px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    box-shadow: var(--yap-shadow-sm);
  }

  /* Settings modal */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(45, 38, 25, 0.35);
  }
  .backdrop.open {
    display: flex;
  }
  .card {
    width: min(1100px, calc(100vw - 48px));
    height: calc(100vh - 48px);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-xl);
    overflow: hidden;
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-modal);
  }
</style>
