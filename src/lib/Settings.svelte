<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy, tick } from 'svelte';
  import { initSpeech } from './speech.svelte.js';
  import { applyAppearance } from './appearance.js';
  import { startKokoro } from './kokoro.svelte.js';
  import { creditCleanupKey } from './providers.js';
  import { loadCatalog } from './modelCatalog.svelte.js';
  import { attention, attentionCount, followAttention } from './attention.svelte.js';
  import { refreshLocalLlm, listenLocalProgress, followLocalLlm } from './settings/localLlm.svelte.js';
  import AccountSection from './AccountSection.svelte';
  import ConnectorsSection from './ConnectorsSection.svelte';
  import McpSection from './McpSection.svelte';
  import MemorySection from './MemorySection.svelte';
  import { account, displayName, initAccount, initials } from './account.svelte.js';
  // Last before settings.css: the CSS order (LanguageModelsPage.svelte says why).
  import LanguageModelsPage from './settings/LanguageModelsPage.svelte';
  import './settings/settings.css';
  import { NAV_GROUPS, resolveTarget } from './settings/nav.js';
  import { searchSettings, groupResults, countByPage } from './settings/settingsIndex.js';
  import { hotkeyCapture } from './settings/hotkeys.svelte.js';
  import { followHotkeyFallback } from './settings/hotkeyFallback.svelte.js';
  import { followDictionary } from './settings/dictionarySync.svelte.js';
  import { followOutsideChanges } from './settings/adoptions.svelte.js';
  import { config, syncConfig, readConfig, setPristine, persist } from './settings/configSync.svelte.js';
  import SearchResults from './settings/SearchResults.svelte';
  import GeneralPage from './settings/GeneralPage.svelte';
  import DictationPage from './settings/DictationPage.svelte';
  import ShortcutsPage from './settings/ShortcutsPage.svelte';
  import MeetingsPage from './settings/MeetingsPage.svelte';
  import PrivacyPage from './settings/PrivacyPage.svelte';
  import AboutPage from './settings/AboutPage.svelte';
  import CommandsPage from './settings/CommandsPage.svelte';
  import VoicePage from './settings/VoicePage.svelte';
  import SpeechPage from './settings/SpeechPage.svelte';
  import { SCOPE_TABS, ensureScopes } from './settings/scopes.js';
  import { migrateRoutesToProfiles } from './settings/profiles.js';
  import {
    lm,
    providerUi,
    loadBasePrompt,
    loadRecentApps,
    refreshUsage,
    listenUsage,
  } from './settings/languageModels.svelte.js';
  import { noteAccountPicture } from './connections.js';

  // Embedded mode: rendered inside the ControlPanel's Settings modal
  // (OpenWhispr SettingsModal-style) instead of filling the window. The parent
  // keeps this component ALWAYS MOUNTED — the in-window hotkey fallback and
  // auto-save below must live for the whole window, not just while the modal
  // is open. `onclose` closes the modal (the ✕ button); `open` says whether
  // it's on screen (Ctrl+F, the search's Esc); `onshowview` leaves Settings
  // for a main-window view (Dictation's Dictionary row).
  let { embedded = false, open = true, onclose = null, onshowview = null } = $props();

  // Settings' copy of the config, in step with config.json
  // (settings/configSync.svelte.js): read at mount, auto-saved after any
  // change, what other surfaces save taken in. A provider adopted sets the
  // Language Models tab's mode; scopes adopted remount their tabs (they read
  // their scope once, ScopeProviderConfig).
  syncConfig({
    onAdopted(taken) {
      if (taken.includes('ppProvider')) providerUi(cfg);
      if (taken.includes('llmScopes')) {
        ensureScopes(cfg);
        lm.chatScopeRev += 1;
      }
    },
  });
  const cfg = $derived(config.cfg);
  // General → Appearance (the theme, the reading text, Motion) shows in this
  // window as you pick (the other windows follow after the auto-save:
  // lib/appearance.js). So do the bar's colours: the main window draws the
  // bar's cards too (the Connect calendar dialog's picture).
  $effect(() => {
    if (cfg)
      applyAppearance({
        theme: cfg.theme,
        barTheme: cfg.barTheme,
        readingFont: cfg.readingFont,
        readingSize: cfg.readingSize,
        motion: cfg.motion,
      });
  });
  let devices = $state([]);
  let outputs = $state([]); // audio output device names (for the chime)
  let section = $state('general'); // a page id (settings/nav.js)
  // Sidebar account button (account state lives in account.svelte.js).
  initAccount();
  let acctUser = $derived(account.status?.signedIn ? account.status.user : null);
  let acctImgFailed = $state(false);
  // Language/translate capability of the active model (drives Models section).
  let langInfo = $state({ supportsLanguage: false, supportsTranslate: false, languages: [] });

  // Update state lives in updates.svelte.js (Rust's updates.rs drives it);
  // ControlPanel starts it, About + the attention badge render it.

  // The red attention count (attention.svelte.js), kept live here: Settings
  // is always mounted, so the ControlPanel's bell is current while it's closed.
  followAttention(() => (config.loaded ? cfg : null));
  // The on-device model's status while on-device is picked but warming up,
  // and the model browser's family tab (settings/localLlm.svelte.js).
  followLocalLlm(() => cfg);
  // The dictation, assistant, meeting, read-aloud and snip keys while this
  // window has the focus (settings/hotkeyFallback.svelte.js).
  followHotkeyFallback(() => cfg);
  // What other surfaces save, taken into this copy: the Dictionary view's
  // list both ways (settings/dictionarySync.svelte.js) and the one-off
  // adoptions (settings/adoptions.svelte.js).
  followDictionary(() => cfg, () => config.loaded);
  followOutsideChanges(() => cfg);

  // Populate the per-app routing picker (from history) when AI Cleanup opens,
  // …and the providers' live model lists (a day-old one is checked behind),
  // and read the on-device model's state afresh (where its layers went, the
  // bigger model on offer).
  $effect(() => {
    if (section === 'cleanup' && config.loaded) {
      loadRecentApps();
      loadCatalog();
      refreshLocalLlm();
    }
  });

  // The day's Groq usage, live (Language Models → Usage today).
  let unlistenUsage = null;

  // Real running version — read from Tauri, not hardcoded (was stuck at '0.1.0').
  let APP_VERSION = $state('');

  onMount(async () => {
    // The cleanup guardrails, for Prompt Studio's View (Language Models).
    await loadBasePrompt();
    try {
      if ('__TAURI_INTERNALS__' in window) {
        const { getVersion } = await import('@tauri-apps/api/app');
        APP_VERSION = await getVersion();
      }
    } catch {
      /* not in Tauri — leave blank */
    }
    await readConfig();
    providerUi(cfg);
    // Per-provider key store (providers.js): the active key credited to its provider.
    creditCleanupKey(cfg, lm.cloudProvider);
    if (!Array.isArray(cfg.dictionary)) cfg.dictionary = [];
    if (!Array.isArray(cfg.appRoutes)) cfg.appRoutes = [];
    if (!Array.isArray(cfg.cleanupProfiles)) cfg.cleanupProfiles = [];
    ensureScopes(cfg);
    // On-device cleanup status + live download progress.
    refreshLocalLlm();
    listenLocalProgress();
    // This copy as set up, before any change worth saving (the migration
    // below is one): what `fill` puts in base's holes.
    setPristine();
    const migrated = migrateRoutesToProfiles(cfg);
    config.loaded = true;
    // Persist the migration immediately so it isn't redone (and duplicated) on
    // the next load. The auto-save effect skips its first (priming) run.
    if (migrated) persist();
    try {
      devices = await invoke('list_audio_devices');
    } catch {
      devices = [];
    }
    try {
      outputs = await invoke('list_output_devices');
    } catch {
      outputs = [];
    }

    // Daily Groq usage: fetch once, then update live as dictations come in.
    refreshUsage();
    try {
      unlistenUsage = await listenUsage();
    } catch {
      unlistenUsage = null;
    }
  });

  onDestroy(() => {
    if (unlistenUsage) unlistenUsage();
  });

  // Refresh language capability whenever the active model changes.
  $effect(() => {
    const ms = cfg?.modelSize;
    if (!ms) return;
    invoke('model_language_info', { modelSize: ms })
      .then((info) => {
        if (info) langInfo = info;
      })
      .catch(() => {});
  });

  // ---- Settings search (2026-10-22 review #3): the box at the top of the
  // nav (Ctrl+F or / while Settings is open), over a static index of every
  // setting (settings/settingsIndex.js). While there's a query the content
  // pane shows the results; ↑/↓ move, Enter opens the page at the row, Esc
  // clears (and a second Esc closes Settings).
  let query = $state('');
  let selected = $state(0);
  let searchEl = $state(null);
  let railSearch = $state(false); // the icon rail's search box, open
  const searching = $derived(query.trim().length > 0);
  const results = $derived(searching ? searchSettings(query) : []);
  const resultGroups = $derived(groupResults(results));
  const flatResults = $derived(resultGroups.flatMap((g) => g.results));
  const counts = $derived(searching ? countByPage(results) : null);
  $effect(() => {
    query;
    selected = 0;
  });
  function openResult(r) {
    goto({ page: r.page, anchor: r.anchor, fallback: r.fallback, scope: r.scope, focus: true });
  }
  function onSearchKey(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selected = Math.min(selected + 1, Math.max(0, flatResults.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
    } else if (e.key === 'Enter') {
      const r = flatResults[selected];
      if (r) {
        e.preventDefault();
        openResult(r);
      }
    } else if (e.key === 'Escape') {
      if (query) {
        e.preventDefault();
        e.stopPropagation();
        query = '';
      } else if (railSearch) {
        e.preventDefault();
        e.stopPropagation();
        railSearch = false;
      }
    }
  }
  function focusSearch() {
    if (rail) railSearch = true;
    tick().then(() => {
      searchEl?.focus();
      searchEl?.select();
    });
  }
  function typingIn(el) {
    return !!el?.closest?.('input, textarea, select, [contenteditable="true"]');
  }
  function onFindKey(e) {
    if (!open || hotkeyCapture.on) return;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      focusSearch();
    } else if (e.key === '/' && !e.ctrlKey && !e.altKey && !e.metaKey && !typingIn(e.target)) {
      e.preventDefault();
      focusSearch();
    }
  }
  $effect(() => {
    window.addEventListener('keydown', onFindKey);
    return () => window.removeEventListener('keydown', onFindKey);
  });
  // Closed: the next open starts on a clean page, not old results.
  $effect(() => {
    if (!open) {
      query = '';
      railSearch = false;
    }
  });

  // ---- The minimum window (860 × 600): below ~960 px the nav becomes an
  // icon rail with tooltips and a search button (review #5, concept 5;
  // OpenWhispr's compact mode). ----
  let rail = $state(false);
  onMount(() => {
    const mq = window.matchMedia?.('(max-width: 960px)');
    if (!mq) return;
    rail = mq.matches;
    const on = () => {
      rail = mq.matches;
      if (!rail) railSearch = false;
    };
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });
  let railTip = $state(null); // { label, x, y } of the item pointed at
  function showTip(e, label) {
    if (!rail) return;
    const r = e.currentTarget.getBoundingClientRect();
    railTip = { label, x: r.right + 8, y: r.top + r.height / 2 };
  }
  function hideTip() {
    railTip = null;
  }

  // Where a link lands (settings/nav.js: aliases keep old links working —
  // "general#meetings", "general#screen-sharing" in Windows notifications
  // still in the notification centre, "history", "advanced"). "page#anchor"
  // scrolls to #settings-<anchor> or the row [data-setting=<anchor>] and
  // gives it a short wash; "cleanup#chat" opens that Language Models tab.
  let contentEl = $state(null);
  let meetingApps = $state(false); // Meetings → "Ask about calls in"'s list
  let privacyConnections = $state(false); // Privacy & data → Network connections
  function goto(target) {
    const to = resolveTarget(target, SCOPE_TABS.map((t) => t.value));
    if (!to) return false;
    to.focus = !!target?.focus;
    to.fallback = typeof target?.fallback === 'string' ? target.fallback : '';
    query = '';
    railSearch = false;
    section = to.page;
    meetingApps = to.page === 'meetings' && to.anchor === 'call-apps-list';
    privacyConnections = to.page === 'privacy' && to.anchor === 'connections-list';
    if (to.scope) lm.scope = to.scope;
    tick().then(() => {
      if (!to.anchor) {
        contentEl?.scrollTo?.({ top: 0 });
        return;
      }
      // A row that only exists once its page has loaded (the AI apps) gets
      // a moment to appear.
      let tries = 0;
      const find = (a) => a && (document.getElementById(`settings-${a}`) || contentEl?.querySelector(`[data-setting="${CSS.escape(a)}"]`));
      const land = () => {
        // The search's fallback: the row to land on while the setting itself
        // isn't showing (Prompt Studio with cleanup off).
        const el = find(to.anchor) || (tries >= 3 ? find(to.fallback) : null);
        if (!el) {
          if (++tries < 20) setTimeout(land, 100);
          return;
        }
        el.scrollIntoView({ block: 'start' });
        const row = el.matches('[data-setting]') ? el : null;
        if (row) {
          row.classList.remove('set-flash');
          void row.offsetWidth;
          row.classList.add('set-flash');
          setTimeout(() => row.classList.remove('set-flash'), 1900);
          // From the search, the keyboard goes on to the setting itself.
          if (to.focus) {
            const ctl = row.matches('button, a[href], input, select, textarea')
              ? row
              : row.querySelector('button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href]');
            ctl?.focus({ preventScroll: true });
          }
        }
      };
      land();
    });
    return true;
  }
  function onSettingsGoto(e) {
    if (typeof e.detail === 'string' || (e.detail && typeof e.detail === 'object')) goto(e.detail);
  }
  $effect(() => {
    window.addEventListener('yap-settings-goto', onSettingsGoto);
    return () => window.removeEventListener('yap-settings-goto', onSettingsGoto);
  });
  // Voice Assistant → Spoken replies (tts.rs): ▶ Preview's voice and
  // Kokoro's state for the window's life (VoiceSettings, KokoroOffer).
  initSpeech(['preview']);
  startKokoro();
  // The "Download Kokoro" offer, open across page switches (VoicePage).
  let offerKokoro = $state(false);
  // General → Yap bar (bar.rs): "Hide the bar for 1 hour" isn't a setting;
  // the group shows it with a way to bring the bar back now.
  let barHiddenUntil = $state(null);
  onMount(() => {
    invoke('bar_status')
      .then((s) => (barHiddenUntil = s?.hiddenUntil ?? null))
      .catch(() => {});
    const un = listen('yap-bar', (e) => (barHiddenUntil = e.payload?.hiddenUntil ?? null));
    return () => un.then((f) => f());
  });
</script>

{#snippet navIcon(id)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if id === 'general'}
      <path d="M6 4v4M6 12v8M12 4v10M12 18v2M18 4v2M18 10v10" /><circle cx="6" cy="10" r="2" /><circle cx="12" cy="16" r="2" /><circle cx="18" cy="8" r="2" />
    {:else if id === 'dictation'}
      <rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 11a7 7 0 0 0 14 0" /><path d="M12 18v4" />
    {:else if id === 'shortcuts'}
      <rect x="2.5" y="6" width="19" height="12" rx="2.5" /><path d="M6.5 10h1M10.5 10h1M14.5 10h1M8 14h8" />
    {:else if id === 'meetings'}
      <circle cx="12" cy="12" r="8.5" /><circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" />
    {:else if id === 'commands'}
      <!-- A wand: the AI does it. -->
      <path d="m4 20 11-11" /><path d="m13 7 4 4" /><path d="M17 3v3M15.5 4.5h3M20 9v2M19 10h2M9 3v2M8 4h2" />
    {:else if id === 'voice'}
      <path d="M11 5 6 9H2v6h4l5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.4 5.6a9 9 0 0 1 0 12.8" />
    {:else if id === 'memory'}
      <!-- A bookmark with a spark: what Yap keeps about you (the toast's chip too). -->
      <path d="M6 3h12v18l-6-4-6 4z" /><path d="M12 7.5v4M10 9.5h4" />
    {:else if id === 'models'}
      <path d="M3 12h2M7 8v8M11 5v14M15 9v6M19 11v2" />
    {:else if id === 'cleanup'}
      <path d="M12 5a3 3 0 0 0-3-3 3 3 0 0 0-3 3 3 3 0 0 0-2 2.8 3 3 0 0 0 .3 4.2A3.2 3.2 0 0 0 7.5 21c1 0 1.9-.4 2.5-1.1.6.7 1.5 1.1 2.5 1.1a3.2 3.2 0 0 0 3.2-4 3 3 0 0 0 .3-4.2 3 3 0 0 0-2-2.8 3 3 0 0 0-3-3z" /><path d="M12 5v14" />
    {:else if id === 'connectors'}
      <path d="M9 3v4M15 3v4" /><path d="M6 7h12v3a6 6 0 0 1-12 0z" /><path d="M12 16v5" />
    {:else if id === 'mcp'}
      <!-- The Model Context Protocol's knot mark, redrawn on the 24 grid. -->
      <path d="M3.1 12 11.4 3.7c1.2-1.2 3.1-1.2 4.2 0s1.2 3.1 0 4.2L9.3 14.2" /><path d="m9.4 14.1 6.2-6.2c1.2-1.2 3.1-1.2 4.2 0c1.2 1.2 1.2 3.1 0 4.2l-7.6 7.6c-.4.4-.4 1 0 1.4l1.6 1.6" /><path d="M13.5 5.8 7.3 12c-1.2 1.2-1.2 3.1 0 4.2s3.1 1.2 4.2 0l6.2-6.2" />
    {:else if id === 'privacy'}
      <path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6z" /><path d="m9 12 2 2 4-4" />
    {:else if id === 'about'}
      <circle cx="12" cy="12" r="9" /><path d="M12 16v-4" /><path d="M12 8h.01" />
    {:else if id === 'search'}
      <circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" />
    {/if}
  </svg>
{/snippet}

<div class="shell settings-shell" class:embedded class:rail>
  {#if embedded}
    <button class="modal-x" title="Close settings" aria-label="Close settings" onclick={() => onclose?.()}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  {/if}
  <nav class="sidebar" aria-label="Settings sections">
    {#if rail}
      <button
        class="railsearch"
        class:on={railSearch || searching}
        aria-label="Search settings"
        onclick={() => (railSearch ? (railSearch = false) : focusSearch())}
        onmouseenter={(e) => showTip(e, 'Search settings · Ctrl F')}
        onmouseleave={hideTip}
        onfocus={(e) => showTip(e, 'Search settings · Ctrl F')}
        onblur={hideTip}
      >
        {@render navIcon('search')}
      </button>
    {:else}
      <div class="search" class:on={searching}>
        {@render navIcon('search')}
        <input
          bind:this={searchEl}
          bind:value={query}
          type="search"
          placeholder="Search settings"
          aria-label="Search settings"
          aria-controls="settings-search-results"
          aria-activedescendant={searching && flatResults.length ? `settings-result-${selected}` : undefined}
          autocomplete="off"
          spellcheck="false"
          onkeydown={onSearchKey}
        />
        <kbd>{searching ? 'Esc' : 'Ctrl F'}</kbd>
      </div>
    {/if}

    <div class="navlist">
      {#each NAV_GROUPS as g, gi (g.label ?? gi)}
        {#if g.label && !rail}<div class="navcap">{g.label}</div>{/if}
        {#if g.label && rail}<div class="railsep" aria-hidden="true"></div>{/if}
        {#each g.items as s (s.id)}
          {@const attn = attentionCount(s.id)}
          {@const hits = counts ? (counts[s.id] ?? 0) : null}
          <button
            class="navitem"
            class:active={!searching && section === s.id}
            class:dim={hits === 0}
            aria-current={section === s.id ? 'page' : undefined}
            aria-label={rail ? (attn > 0 ? `${s.label} ${attn}` : s.label) : undefined}
            onclick={() => goto(s.id)}
            onmouseenter={(e) => showTip(e, s.label)}
            onmouseleave={hideTip}
            onfocus={(e) => showTip(e, s.label)}
            onblur={hideTip}
          >
            <span class="navicon">{@render navIcon(s.id)}</span>
            {#if !rail}<span class="navlabel">{s.label}</span>{/if}
            {#if hits}
              {#if !rail}<span class="navcount" aria-label={`${hits} found`}>{hits}</span>{/if}
            {:else if attn > 0}
              {#if rail}
                <span class="navdot" aria-hidden="true"></span>
              {:else}
                <span class="navbadge" title={attention.items.filter((i) => i.section === s.id).map((i) => i.label).join(' · ')}>{attn}</span>
              {/if}
            {/if}
          </button>
        {/each}
      {/each}
    </div>

    <div class="side-spacer"></div>
    <button
      class="acct"
      class:active={section === 'account'}
      class:signed={!!acctUser}
      aria-current={section === 'account' ? 'page' : undefined}
      aria-label={rail ? (acctUser ? `Account: ${displayName(acctUser)}` : 'Sign in') : undefined}
      onclick={() => goto('account')}
      onmouseenter={(e) => showTip(e, acctUser ? 'Account' : 'Sign in')}
      onmouseleave={hideTip}
      onfocus={(e) => showTip(e, acctUser ? 'Account' : 'Sign in')}
      onblur={hideTip}
    >
      <span class="acct-avatar">
        {#if acctUser?.image && !acctImgFailed}
          <img src={acctUser.image} alt="" referrerpolicy="no-referrer" onload={() => noteAccountPicture(acctUser.image)} onerror={() => (acctImgFailed = true)} />
        {:else if acctUser}
          <span class="acct-ini">{initials(acctUser)}</span>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="8" r="3.2" /><path d="M5 20a7 7 0 0 1 14 0" />
          </svg>
        {/if}
      </span>
      {#if !rail}
        <span class="acct-who">
          {#if acctUser}
            <span class="l1">{displayName(acctUser)}</span>
            <span class="l2">{acctUser.name?.trim() ? acctUser.email : 'Yap account'}</span>
          {:else}
            <span class="l1">Sign in</span>
            <span class="l2">Optional · works offline</span>
          {/if}
        </span>
      {/if}
    </button>
    <!-- The version, and the "Saved" pulse every change gets (Wispr's
         "Flow v1.6" in its nav's footer). -->
    {#if !rail}
      <div class="navfoot">
        <span class="ver">Yap {APP_VERSION}</span>
        <span class="saved" class:show={config.saved} aria-live="polite">{config.saved ? 'Saved ✓' : ''}</span>
      </div>
    {:else}
      <span class="railsaved" class:show={config.saved} aria-hidden="true">✓</span>
    {/if}
  </nav>

  {#if railTip}
    <span class="railtip" style:left="{railTip.x}px" style:top="{railTip.y}px" role="tooltip">{railTip.label}</span>
  {/if}

  <main>
    {#if cfg}
      <div class="content" bind:this={contentEl}>
        <div class="inner">
          {#if rail && railSearch}
            <div class="search inpage" class:on={searching}>
              {@render navIcon('search')}
              <input
                bind:this={searchEl}
                bind:value={query}
                type="search"
                placeholder="Search settings"
                aria-label="Search settings"
                aria-controls="settings-search-results"
                aria-activedescendant={searching && flatResults.length ? `settings-result-${selected}` : undefined}
                autocomplete="off"
                spellcheck="false"
                onkeydown={onSearchKey}
              />
              <kbd>Esc</kbd>
            </div>
          {/if}
          <!-- Which page is shown: each is its own component (settings/*Page.svelte);
               the few that are a section and a heading are here. -->
          {#if searching}
            <SearchResults {query} groups={resultGroups} bind:selected {cfg} onopen={openResult} />
          {:else if section === 'general'}
            <GeneralPage {cfg} {barHiddenUntil} {goto} />
          {:else if section === 'dictation'}
            <DictationPage {cfg} {devices} {outputs} {onshowview} />
          {:else if section === 'shortcuts'}
            <ShortcutsPage {cfg} {goto} />
          {:else if section === 'meetings'}
            <MeetingsPage {cfg} {goto} bind:apps={meetingApps} />
          {:else if section === 'commands'}
            <CommandsPage {cfg} {goto} />
          {:else if section === 'voice'}
            <VoicePage {cfg} {goto} bind:offerKokoro />
          {:else if section === 'memory'}
            <div class="page-h">
              <h1>Memory</h1>
              <p>What Yap remembers about you, so its AI answers you better. See, change or clear any of it.</p>
            </div>
            <MemorySection {cfg} {goto} />
          {:else if section === 'models'}
            <SpeechPage {cfg} {langInfo} />
          {:else if section === 'cleanup'}
            <LanguageModelsPage {cfg} {goto} />
          {:else if section === 'connectors'}
            <div class="page-h">
              <h1>Connectors</h1>
              <p>A calendar gives you a reminder before each meeting, and notes with its name and who's in it. Yap reads it straight from this PC.</p>
            </div>
            <ConnectorsSection {goto} />
          {:else if section === 'mcp'}
            <div class="page-h">
              <h1>AI apps (MCP)</h1>
              <p>Let your AI apps read your meeting transcripts and notes, through Yap on this PC. Never your dictations.</p>
            </div>
            <McpSection {cfg} {goto} />
          {:else if section === 'privacy'}
            <PrivacyPage {cfg} {goto} bind:connections={privacyConnections} />
          {:else if section === 'about'}
            <AboutPage {cfg} version={APP_VERSION} {goto} />
          {:else if section === 'account'}
            <div class="page-h">
              <h1>Account</h1>
              <p>Optional. Yap works fully offline without one; sign in only for the extras that need it.</p>
            </div>
            <div data-setting="account">
              <AccountSection />
            </div>
          {/if}
        </div>
      </div>
    {:else}
      <p class="loading">Loading…</p>
    {/if}
  </main>
</div>

<style>
  :global(body) {
    background: var(--yap-s1);
  }
  .shell {
    display: flex;
    min-height: 100vh;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font-size: 13px;
  }
  /* Embedded (ControlPanel settings modal): fill the modal card, not the
     viewport, and scroll internally. */
  .shell.embedded {
    position: relative;
    min-height: 100%;
    height: 100%;
    overflow: hidden;
  }
  .shell.embedded main {
    max-height: 100%;
  }
  .modal-x {
    position: absolute;
    top: 10px;
    right: 12px;
    z-index: 10;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .modal-x:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .modal-x:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .modal-x svg {
    width: 15px;
    height: 15px;
  }

  /* ---- the nav: search, the pages by scope, the account and the version ---- */
  .sidebar {
    flex: 0 0 224px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 14px 10px 10px;
    box-sizing: border-box;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-bg);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 6px 0 10px;
    margin: 0 2px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg-62);
    transition:
      border-color var(--yap-dur) ease,
      box-shadow var(--yap-dur) ease;
  }
  .search:hover {
    border-color: var(--yap-border-hover);
  }
  .search:focus-within {
    border-color: var(--yap-border-hover);
    box-shadow: var(--yap-focus);
  }
  .search :global(svg) {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
  }
  .search input {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    outline: none;
    background: none;
    font: inherit;
    font-size: 12.5px;
    color: var(--yap-fg);
  }
  .search input::placeholder {
    color: var(--yap-fg-62);
  }
  .search input::-webkit-search-cancel-button {
    display: none;
  }
  .search kbd {
    flex: 0 0 auto;
    font: inherit;
    font-size: 10.5px;
    font-weight: 600;
    line-height: 15px;
    padding: 0 4px;
    color: var(--yap-fg-62);
    border: 1px solid var(--yap-border);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--yap-s1);
  }
  /* The rail's search, at the top of the page it opens over. */
  .search.inpage {
    margin: 0 0 22px;
    height: 36px;
  }
  .navlist {
    flex: 0 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin: 0 -4px;
    padding: 0 4px 6px;
  }
  .navcap {
    padding: 0 10px;
    margin: 12px 0 3px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .navitem {
    position: relative;
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 10px;
    height: 31px;
    padding: 0 8px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 13px;
    font-weight: 520;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease,
      opacity var(--yap-dur) ease;
  }
  .navitem:hover {
    color: var(--yap-fg);
    background: var(--yap-raised-soft);
  }
  .navitem:focus-visible,
  .acct:focus-visible,
  .railsearch:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  /* The active page: a white pill with the amber icon chip. */
  .navitem.active {
    color: var(--yap-fg);
    font-weight: 620;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .navitem.active:focus-visible {
    box-shadow: var(--yap-focus);
  }
  /* A search with no results on a page dims it. */
  .navitem.dim {
    opacity: 0.45;
  }
  .navicon {
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    border-radius: 6px;
    display: grid;
    place-items: center;
    color: var(--yap-fg-62);
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .navicon :global(svg) {
    width: 15px;
    height: 15px;
  }
  .navitem.active .navicon {
    background: var(--yap-primary-tint);
    color: var(--yap-link);
  }
  .navlabel {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Wispr-style red attention count. */
  .navbadge {
    flex: 0 0 auto;
    margin-left: auto;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    box-sizing: border-box;
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
  /* How many search results a page has. */
  .navcount {
    margin-left: auto;
    padding: 0 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    color: var(--yap-fg-80);
    font-size: 11px;
    font-weight: 650;
    line-height: 17px;
  }

  .side-spacer {
    flex: 1 1 auto;
    min-height: 8px;
  }
  .acct {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 8px 6px;
    margin-top: 2px;
    border: none;
    border-top: 1px solid var(--yap-border-subtle);
    border-radius: 0;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .acct:hover .l1,
  .acct.active .l1 {
    color: var(--yap-fg);
  }
  .acct:hover .acct-avatar {
    background: var(--yap-border-subtle);
  }
  .acct-avatar img {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    object-fit: cover;
  }
  .acct-ini {
    font-size: 11.5px;
    font-weight: 700;
    color: var(--yap-link);
  }
  .acct.signed .acct-avatar {
    background: var(--yap-primary-tint);
    overflow: hidden;
  }
  .acct-avatar {
    width: 28px;
    height: 28px;
    flex: 0 0 28px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    display: grid;
    place-items: center;
    color: var(--yap-fg-62);
    transition: background var(--yap-dur) ease;
  }
  .acct.active .acct-avatar {
    box-shadow: 0 0 0 2px var(--yap-bg), 0 0 0 3.5px var(--yap-primary);
  }
  .acct-avatar :global(svg) {
    width: 16px;
    height: 16px;
  }
  .acct-who {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .acct-who .l1 {
    font-size: 12.5px;
    color: var(--yap-fg-80);
    font-weight: 560;
    line-height: 1.3;
  }
  .acct-who .l2 {
    font-size: 11.5px;
    color: var(--yap-muted);
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .navfoot {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    padding: 4px 8px 0;
    font-size: 11px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .navfoot .saved {
    color: var(--yap-success-text);
    font-weight: 600;
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .navfoot .saved.show {
    opacity: 1;
  }

  /* ---- 860 × 600: the nav as an icon rail (concept 5) ---- */
  .shell.rail .sidebar {
    flex-basis: 58px;
    align-items: center;
    padding: 12px 8px 8px;
  }
  .railsearch {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 38px;
    height: 32px;
    margin-bottom: 8px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .railsearch:hover,
  .railsearch.on {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .railsearch :global(svg) {
    width: 15px;
    height: 15px;
  }
  .shell.rail .navlist {
    align-items: center;
    margin: 0;
    padding: 0;
    scrollbar-width: none;
  }
  /* 13 pages, the search and the account in 600 px of window: every icon
     on screen without scrolling the rail. */
  .shell.rail .navitem {
    width: 38px;
    height: 28px;
    padding: 0;
    justify-content: center;
  }
  .railsep {
    flex: 0 0 auto;
    width: 24px;
    height: 1px;
    margin: 3px 0;
    background: var(--yap-border);
  }
  .navdot {
    position: absolute;
    top: 5px;
    right: 6px;
    width: 7px;
    height: 7px;
    border-radius: 99px;
    background: #e5484d;
    box-shadow: 0 0 0 1.5px var(--yap-bg);
  }
  .shell.rail .acct {
    width: auto;
    padding: 8px 4px 4px;
    justify-content: center;
  }
  .railsaved {
    font-size: 11px;
    font-weight: 700;
    color: var(--yap-success-text);
    opacity: 0;
    transition: opacity 0.2s ease;
  }
  .railsaved.show {
    opacity: 1;
  }
  .railtip {
    position: fixed;
    z-index: 100;
    transform: translateY(-50%);
    padding: 5px 9px;
    border-radius: 7px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    box-shadow: var(--yap-shadow-menu);
    pointer-events: none;
  }
  .railtip::before {
    content: '';
    position: absolute;
    left: -4px;
    top: 50%;
    margin-top: -4px;
    border: 4px solid transparent;
    border-right-color: var(--yap-ink);
    border-left: 0;
  }

  /* ---- the page: one content column (700 px) on every page ---- */
  main {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    max-height: 100vh;
  }
  .content {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 32px 44px 48px 48px;
  }
  .inner {
    max-width: var(--yap-settings-w);
  }
  .shell.rail .content {
    padding: 26px 30px 36px 34px;
  }
  .loading {
    color: var(--yap-muted);
    padding: 20px;
  }
</style>
