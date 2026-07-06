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
  import { onMount } from 'svelte';
  import yapIcon from '../assets/yap-icon.png';
  import ToastHost from './ui/ToastHost.svelte';
  import { toast } from './ui/toast.svelte.js';
  import Settings from './Settings.svelte';
  import HomeView from './HomeView.svelte';
  import DictionaryView from './DictionaryView.svelte';
  import UploadView from './UploadView.svelte';
  import NotesView from './NotesView.svelte';
  import ComingSoonView from './ComingSoonView.svelte';

  let activeView = $state('home');
  let settingsOpen = $state(false);

  // Sidebar nav, OpenWhispr order (Home / Chat / Notes / Upload / Dictionary).
  const NAV = [
    { id: 'home', label: 'Home' },
    { id: 'chat', label: 'Chat' },
    { id: 'notes', label: 'Notes' },
    { id: 'upload', label: 'Upload' },
    { id: 'dictionary', label: 'Dictionary' },
  ];

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
    let un;
    listen('yap-error', (e) => {
      const msg = String(e.payload || 'Something went wrong');
      toast({ title: 'Yap ran into a problem', description: msg, variant: 'destructive' });
    }).then((u) => (un = u));
    return () => un && un();
  });
</script>

{#snippet navIcon(id)}
  {#if id === 'home'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 10.5 12 3l9 7.5" /><path d="M5 9.5V21h14V9.5" /><path d="M10 21v-6h4v6" /></svg>
  {:else if id === 'chat'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
  {:else if id === 'notes'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
  {:else if id === 'upload'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M17 8l-5-5-5 5" /><path d="M12 3v12" /></svg>
  {:else if id === 'dictionary'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 4h6a4 4 0 0 1 4 4v12a3 3 0 0 0-3-3H2z" /><path d="M22 4h-6a4 4 0 0 0-4 4v12a3 3 0 0 1 3-3h7z" /></svg>
  {/if}
{/snippet}

<svelte:window onkeydown={onOverlayKeydown} />

<div class="panel">
  <nav class="side">
    <div class="brand">
      <img class="brandlogo" src={yapIcon} alt="" aria-hidden="true" />
      <span class="brandname">Yap</span>
    </div>

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
      <HomeView onopensettings={openSettings} />
    {:else if activeView === 'dictionary'}
      <DictionaryView />
    {:else if activeView === 'chat'}
      <ComingSoonView
        kind="chat"
        title="Chat"
        body="A voice assistant you can ask questions — coming with Yap's AI Chat surface. Its model and prompt are already configurable, so it works the moment it ships."
        linkText="Configure its model in Settings → Language Models → Chat"
        onlink={() => openSettings('llm')}
      />
    {:else if activeView === 'notes'}
      <NotesView />
    {:else if activeView === 'upload'}
      <UploadView />
    {/if}
  </main>
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
    height: 100vh;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 13px;
    overflow: hidden;
  }

  .side {
    flex: 0 0 190px;
    display: flex;
    flex-direction: column;
    padding: 12px 8px 10px;
    background: var(--yap-s0, var(--yap-s1));
    border-right: 1px solid var(--yap-border-subtle);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px 12px;
  }
  .brandlogo {
    width: 22px;
    height: 22px;
    border-radius: 6px;
  }
  .brandname {
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 0.01em;
  }

  .nav,
  .bottom {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .navitem {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .navitem:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .navitem.active {
    background: var(--yap-primary-wash);
    color: var(--yap-fg);
    font-weight: 600;
  }
  .navitem.active .navicon {
    color: var(--yap-primary);
  }
  .navicon {
    display: inline-flex;
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
  }
  .navicon :global(svg) {
    width: 100%;
    height: 100%;
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
    font-size: 12px;
    font-weight: 600;
  }
  .acct-who .l2 {
    font-size: 10.5px;
    color: var(--yap-muted-55);
  }

  .view {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* Settings modal */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
  }
  .backdrop.open {
    display: flex;
  }
  .card {
    width: min(1100px, calc(100vw - 48px));
    height: calc(100vh - 48px);
    border: 1px solid var(--yap-border);
    border-radius: 12px;
    overflow: hidden;
    background: var(--yap-s1);
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.5);
  }
</style>
