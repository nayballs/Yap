<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy, tick } from 'svelte';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Select from './ui/Select.svelte';
  import LinkRow from './ui/LinkRow.svelte';
  import Segmented from './ui/Segmented.svelte';
  import ModelManager from './ModelManager.svelte';
  import SpeakerModels from './SpeakerModels.svelte';
  import AgentNameField from './AgentNameField.svelte';
  import { initSpeech, readPageSelection } from './speech.svelte.js';
  import VoiceSettings from './VoiceSettings.svelte';
  import OpenAppsSettings from './OpenAppsSettings.svelte';
  import WebSearchSettings from './WebSearchSettings.svelte';
  import { applyAppearance } from './appearance.js';
  import KokoroOffer from './KokoroOffer.svelte';
  import { kokoro, startKokoro } from './kokoro.svelte.js';
  import { creditCleanupKey, adoptProviderKey } from './providers.js';
  import { loadCatalog } from './modelCatalog.svelte.js';
  import { hotkeyMatchesKeydown, hotkeyMatchesKeyup, formatHotkeySpec, fillHotkey } from './hotkeys.js';
  import { attention, attentionCount, followAttention } from './attention.svelte.js';
  import { localLlm, refreshLocalLlm, listenLocalProgress, followLocalLlm } from './settings/localLlm.svelte.js';
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
  import { config, syncConfig, readConfig, setPristine, persist } from './settings/configSync.svelte.js';
  import SettingsHotkey from './settings/SettingsHotkey.svelte';
  import TransformsSettings from './settings/TransformsSettings.svelte';
  import SearchResults from './settings/SearchResults.svelte';
  import GeneralPage from './settings/GeneralPage.svelte';
  import DictationPage from './settings/DictationPage.svelte';
  import ShortcutsPage from './settings/ShortcutsPage.svelte';
  import MeetingsPage from './settings/MeetingsPage.svelte';
  import PrivacyPage from './settings/PrivacyPage.svelte';
  import RunsOn from './settings/RunsOn.svelte';
  import AboutPage from './settings/AboutPage.svelte';
  import { SCOPE_TABS, SCOPE_DEFAULTS, ensureScopes, scopeModelLabel as scopeLabel } from './settings/scopes.js';
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

  const UNLOAD_TIMEOUTS = [
    { value: 'never', label: 'Never' },
    { value: '1min', label: 'After 1 minute' },
    { value: '5min', label: 'After 5 minutes' },
    { value: '15min', label: 'After 15 minutes' },
    { value: '30min', label: 'After 30 minutes' },
  ];

  // Display names for the language codes the backend returns.
  const LANG_NAMES = {
    en: 'English', es: 'Spanish', fr: 'French', de: 'German', it: 'Italian',
    pt: 'Portuguese', nl: 'Dutch', pl: 'Polish', ru: 'Russian', uk: 'Ukrainian',
    zh: 'Chinese', ja: 'Japanese', ko: 'Korean', ar: 'Arabic', hi: 'Hindi',
    tr: 'Turkish', yue: 'Cantonese',
  };

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

  // Language dropdown: Auto + whatever the active model supports.
  const langOptions = $derived([
    { value: 'auto', label: 'Auto-detect' },
    ...(langInfo.languages || []).map((c) => ({ value: c, label: LANG_NAMES[c] || c })),
  ]);

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

  // In-window hotkey fallback: when OUR OWN window has focus, the global
  // low-level hook never sees the hotkey (WebView2 front-runs the hook chain;
  // log-proven 2026-07-05). The page gets the keydown normally, so drive the
  // pipeline directly — combo-aware via lib/hotkeys.js, matching the Rust
  // hook's semantics. Guarded while the shortcut recorder is capturing. The
  // meeting shortcut (Win+Alt+M) goes first: a combo is more specific than a
  // bare dictation key; then the Voice Assistant's shortcut. Down and up go
  // to `dictation_key` / `assistant_key`, the hook's own paths: the recording
  // mode (and push-to-talk's tap / double-tap) is Rust's to decide.
  function onFallbackKeyDown(e) {
    // (Not while a shortcut is being picked: settings/hotkeys.svelte.js.)
    if (hotkeyCapture.on || e.repeat) return;
    if (hotkeyMatchesKeydown(e, cfg?.meetingHotkey)) {
      e.preventDefault();
      e.stopPropagation();
      invoke('meeting_shortcut', { origin: 'settings' }).catch(() => {});
      return;
    }
    if (hotkeyMatchesKeydown(e, cfg?.assistantHotkey)) {
      e.preventDefault();
      e.stopPropagation();
      invoke('assistant_key', { pressed: true }).catch(() => {});
      return;
    }
    // Read aloud: what's selected on this page (read_aloud.rs).
    if (hotkeyMatchesKeydown(e, cfg?.readHotkey)) {
      e.preventDefault();
      e.stopPropagation();
      readPageSelection();
      return;
    }
    // Snip to Chat (snip.rs): Yap's own windows leave the shot.
    if (hotkeyMatchesKeydown(e, cfg?.snipHotkey)) {
      e.preventDefault();
      e.stopPropagation();
      invoke('snip_start', { origin: 'shortcut' }).catch(() => {});
      return;
    }
    if (!hotkeyMatchesKeydown(e, cfg?.hotkey)) return;
    e.preventDefault();
    e.stopPropagation();
    invoke('dictation_key', { pressed: true }).catch(() => {});
  }
  function onFallbackKeyUp(e) {
    if (hotkeyCapture.on) return;
    if (hotkeyMatchesKeyup(e, cfg?.assistantHotkey)) {
      e.preventDefault();
      invoke('assistant_key', { pressed: false }).catch(() => {});
      return;
    }
    if (!hotkeyMatchesKeyup(e, cfg?.hotkey)) return;
    e.preventDefault();
    invoke('dictation_key', { pressed: false }).catch(() => {});
  }
  $effect(() => {
    window.addEventListener('keydown', onFallbackKeyDown, true);
    window.addEventListener('keyup', onFallbackKeyUp, true);
    return () => {
      window.removeEventListener('keydown', onFallbackKeyDown, true);
      window.removeEventListener('keyup', onFallbackKeyUp, true);
    };
  });

  // ---- ControlPanel integration ----
  // Deep-links from the shell (e.g. "Sign in" → account section) and dictionary
  // sync with the DictionaryView surface: both edit the same config field from
  // separate components, so events keep the two copies converged instead of the
  // last-saved-wins clobbering each other.
  let lastReceivedDictJson = null;
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
  function onDictChanged(e) {
    const entries = Array.isArray(e.detail) ? e.detail : e.detail?.entries;
    if (!cfg || !Array.isArray(entries)) return;
    lastReceivedDictJson = JSON.stringify(entries);
    cfg.dictionary = entries.map((x) => ({ ...x }));
    // dictionaryFuzzy and learnWords are owned by DictionaryView but ride
    // along in this cfg copy — adopt their new values or our next auto-save
    // would revert the toggles.
    if (typeof e.detail?.fuzzy === 'boolean') cfg.dictionaryFuzzy = e.detail.fuzzy;
    if (typeof e.detail?.learnWords === 'boolean') cfg.learnWords = e.detail.learnWords;
  }
  // A view outside Settings saved a config field (the Integrations view's
  // Local API switch): adopt it, or this copy's next auto-save would put the
  // old value back.
  function onConfigPatched(e) {
    if (cfg && e.detail && typeof e.detail === 'object') {
      Object.assign(cfg, e.detail);
      // The scope tabs read their scope once (ScopeProviderConfig).
      if (e.detail.llmScopes) lm.chatScopeRev += 1;
    }
  }
  // The Chat model picker connected a provider (ai_model.rs
  // `set_provider_key` saved it): the key where this copy keeps it too —
  // the shared store, cleanup's own when cleanup is on that provider, the
  // Chat scope's — or this copy's next auto-save would drop it.
  function onProviderKeySaved(e) {
    const { provider: p, key } = e.detail || {};
    if (!cfg || !p || !key) return;
    if (adoptProviderKey(cfg, p, key)) lm.chatScopeRev += 1;
  }
  $effect(() => {
    window.addEventListener('yap-settings-goto', onSettingsGoto);
    window.addEventListener('yap-dictionary-changed', onDictChanged);
    window.addEventListener('yap-config-patched', onConfigPatched);
    window.addEventListener('yap-provider-key-saved', onProviderKeySaved);
    return () => {
      window.removeEventListener('yap-settings-goto', onSettingsGoto);
      window.removeEventListener('yap-dictionary-changed', onDictChanged);
      window.removeEventListener('yap-config-patched', onConfigPatched);
      window.removeEventListener('yap-provider-key-saved', onProviderKeySaved);
    };
  });
  // Meetings → "Ask about calls in" (MeetingsPage.svelte) saves a flipped
  // app as the person's choice in `meetingDetectApps`.
  function setCallAppAsks(id, asks) {
    cfg.meetingDetectApps = { ...(cfg.meetingDetectApps || {}), [id]: asks };
  }
  // "Don't ask for Teams" on a call prompt saves that in Rust: adopt it into
  // this config copy, or the next auto-save would switch Teams back on.
  onMount(() => {
    const un = listen('yap-meeting-detect-choice', (e) => {
      const c = e.payload;
      if (cfg && c?.app && typeof c.asks === 'boolean') setCallAppAsks(c.app, c.asks);
      // "Stop asking about calls?" → Stop asking, or "Turn back on".
      if (cfg && typeof c?.enabled === 'boolean') cfg.meetingDetection = c.enabled;
    });
    return () => un.then((f) => f());
  });
  // Words added from a suggestion ("Add “Kubernetes” to your dictionary?",
  // learn.rs) are saved in Rust: adopt the list, or the next auto-save would
  // drop them. Not marked as received, so the effect below passes the new
  // list on to the Dictionary view.
  onMount(() => {
    const un = listen('yap-dictionary-learned', (e) => {
      const entries = e.payload?.entries;
      if (cfg && Array.isArray(entries)) cfg.dictionary = entries.map((x) => ({ ...x }));
    });
    return () => un.then((f) => f());
  });
  // The on-device model's context was set (here or in the Chat model
  // picker: local_llm_set_context saved it): adopt it, or the next
  // auto-save would put the old one back; and read the model's state again
  // as its restart waits and ends.
  onMount(() => {
    const uns = [
      listen('yap-local-context-changed', (e) => {
        if (cfg && typeof e.payload?.localContext === 'number') cfg.localContext = e.payload.localContext;
      }),
      listen('yap-local-llm-changed', () => refreshLocalLlm()),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });
  // The Chat view's model chip switched the Chat scope's model (ai_model.rs
  // chat_set_model saved it): adopt its endpoint, or the next auto-save
  // would put the old model back. The prompt stays this copy's. The Chat
  // tab remounts (`lm.chatScopeRev`): its provider pills read the scope once.
  onMount(() => {
    const un = listen('yap-chat-model-changed', (e) => {
      // The picker chose another of the on-device models: the one for
      // every feature (the sidecar serves one).
      if (cfg && typeof e.payload?.ppLocalModel === 'string') {
        cfg.ppLocalModel = e.payload.ppLocalModel;
        refreshLocalLlm();
      }
      const s = e.payload?.scope;
      if (!cfg || !s || typeof s !== 'object') return;
      if (!cfg.llmScopes || typeof cfg.llmScopes !== 'object') cfg.llmScopes = {};
      const cur = cfg.llmScopes.chat || {};
      // The whole saved scope (every field, so the tab's bindings have
      // values), with this copy's keys and prompt kept.
      cfg.llmScopes.chat = {
        disableThinking: false,
        ...cur,
        ...s,
        apiKeys: { ...(cur.apiKeys || {}), ...(s.apiKeys || {}) },
        prompt: cur.prompt || s.prompt || SCOPE_DEFAULTS.chat.prompt,
      };
      lm.chatScopeRev += 1;
    });
    return () => un.then((f) => f());
  });
  // Voice Assistant → Spoken replies (tts.rs). The Chat view's 🔊 saves "Speak
  // replies" in Rust: adopt it, or the next auto-save would undo it. The
  // engine, Kokoro's download, the voice and ▶ Preview are VoiceSettings;
  // switching replies on while Kokoro isn't downloaded offers it
  // (KokoroOffer), Windows voices speaking meanwhile.
  initSpeech(['preview']);
  startKokoro();
  let offerKokoro = $state(false);
  function setSpeakReplies(on) {
    cfg.speakReplies = on;
    // Not while a cloud voice (ElevenLabs) speaks: Kokoro is only its stand-in.
    offerKokoro = on && cfg.ttsProvider !== 'elevenlabs' && cfg.ttsEngine !== 'windows' && kokoro.inBuild && !kokoro.installed;
  }
  onMount(() => {
    const un = listen('yap-voice-replies', (e) => {
      if (cfg && typeof e.payload?.on === 'boolean') cfg.speakReplies = e.payload.on;
    });
    // The Chat view's volume slider (tts_volume_set).
    const unVolume = listen('yap-voice-volume', (e) => {
      if (cfg && Number.isFinite(e.payload?.volume)) cfg.ttsVolume = e.payload.volume;
    });
    // The reading card's − / + (read_aloud.rs saved it already).
    const unRate = listen('yap-voice-rate', (e) => {
      if (cfg && Number.isFinite(e.payload?.rate)) cfg.ttsRate = e.payload.rate;
    });
    return () => {
      un.then((f) => f());
      unVolume.then((f) => f());
      unRate.then((f) => f());
    };
  });
  const SPEAK_WHEN = [
    { value: 'voice', label: 'When I asked by voice' },
    { value: 'always', label: 'Always' },
  ];
  const SPEAK_WHEN_DESC = {
    voice: 'In Chat, answers to questions you dictate are read aloud; typed ones stay quiet',
    always: 'Every answer in Chat is read aloud',
  };
  const wakeName = $derived((cfg?.agentName || '').trim() || 'Yap');
  // A link row's value: which model a Language Models tab runs on now
  // ("Groq · openai/gpt-oss-20b", "On this PC · Qwen3.5 4B"; settings/scopes.js).
  function scopeModelLabel(scope) {
    return scopeLabel(cfg, scope, localLlm.status.model);
  }
  // Commands' model and prompt: Language Models → Commands. The Voice
  // Assistant's: Language Models → Chat & Voice Assistant.
  function openScope(scope) {
    goto({ page: 'cleanup', scope });
  }

  // The notepad's consent message, edited there (notepad.rs): the same.
  onMount(() => {
    const un = listen('yap-consent-message-changed', (e) => {
      if (cfg && typeof e.payload?.message === 'string') cfg.meetingConsentMessage = e.payload.message;
    });
    return () => un.then((f) => f());
  });

  // General → Yap bar (bar.rs). "Turn off the bar" in the bar's own menu
  // saves that in Rust: adopt it here, or the next auto-save would turn the
  // bar back on. "Hide the bar for 1 hour" isn't a setting; the group shows
  // it with a way to bring the bar back now.
  let barHiddenUntil = $state(null);
  onMount(() => {
    invoke('bar_status')
      .then((s) => (barHiddenUntil = s?.hiddenUntil ?? null))
      .catch(() => {});
    const uns = [
      listen('yap-bar', (e) => (barHiddenUntil = e.payload?.hiddenUntil ?? null)),
      listen('yap-bar-changed', (e) => {
        if (cfg && typeof e.payload?.enabled === 'boolean') cfg.barEnabled = e.payload.enabled;
      }),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });

  // Broadcast Settings-side dictionary edits (e.g. the Commands name save)
  // to DictionaryView — skipping echoes of updates we just received from it.
  $effect(() => {
    const json = JSON.stringify(cfg?.dictionary ?? null);
    if (!config.loaded || json === null || json === lastReceivedDictJson) return;
    window.dispatchEvent(
      new CustomEvent('yap-dictionary-external', { detail: JSON.parse(json) })
    );
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
          <!-- Commands (pipeline.rs run_agent; Wispr Flow's Command Mode and
               its old "Hey Flow"): the AI does what you say and types the
               result where you are. Not the Voice Assistant, which answers
               out loud. -->
          <div class="page-h">
            <h1>Commands</h1>
            <p>Tell Yap's AI what to write, and it types the result where you are.</p>
          </div>
          <div class="explain" role="note">
            <p class="example">
              Say <q>Hey {wakeName}, reply that I'll be late</q> in any app; Yap writes it and types it there.
            </p>
            <p class="crossref">
              Want an answer you can hear instead?
              <button class="set-link" onclick={() => goto('voice')}>Voice Assistant</button>
            </p>
          </div>
          <Group title="Say its name" id="settings-wake-word">
            <Row setting="wake-word">
              <Toggle
                bind:checked={cfg.llmScopes.voiceAgent.enabled}
                label="Answer to its name in any dictation"
                desc={`Say "${wakeName}, …" while you dictate and the AI does what you ask, instead of typing your words`}
              />
            </Row>
            <Row label="Agent name" desc="Short and easy to say. Until you pick one, it's “Yap”" setting="agent-name">
              <AgentNameField {cfg} />
            </Row>
          </Group>
          <Group title="Rewrite selected text" id="settings-rewrite">
            <Row
              label="Rewrite shortcut"
              desc={cfg.rewriteLastDictation
                ? 'Select text in any app, press it and say how to change it. With nothing selected within a minute of a dictation, it rewrites that dictation where it is; otherwise it writes where your cursor is'
                : 'Select text in any app, press it and say how to change it. With nothing selected, it writes where your cursor is'}
              hint="For example “make this a bulleted list”, “more formal” or “in French”."
              setting="rewrite-shortcut"
            >
              <SettingsHotkey {cfg} target="editHotkey" clearable />
            </Row>
            <!-- as_said.rs: nothing selected, right after a dictation into
                 the same field (still ending at the cursor) → that dictation
                 rewritten. Off: write mode, as before. -->
            <Row setting="rewrite-last">
              <Toggle
                bind:checked={cfg.rewriteLastDictation}
                label="Rewrite your last dictation when nothing is selected"
                desc="Within a minute, in the same field"
                hint="Only while it's exactly as Yap typed it, with the cursor right after it; otherwise Yap says so and changes nothing."
              />
            </Row>
          </Group>
          <!-- Saved transforms (transforms.rs): one-key rewrites, no speaking. -->
          <Group
            title="Transforms"
            desc="Rewrite the selected text in one press, without saying anything. With nothing selected, your last dictation, by the rules above."
            id="settings-transforms"
          >
            <TransformsSettings {cfg} />
          </Group>
          <Group title="Model and prompt" id="settings-commands-model">
            <LinkRow
              label="Model and prompt"
              desc="In Language Models. With “Answer to its name” off, the rewrite shortcut and transforms use the cleanup model"
              value={scopeModelLabel('voiceAgent')}
              setting="commands-model"
              onclick={() => openScope('voiceAgent')}
            />
          </Group>

        {:else if section === 'voice'}
          <!-- The Voice Assistant (assistant.rs + tts.rs): ask Yap's Chat
               out loud and hear the answer. Not Commands, which type. -->
          <div class="page-h">
            <h1>Voice Assistant</h1>
            <p>Ask Yap something by voice and hear the answer, like a voice assistant.</p>
          </div>
          <div class="explain" role="note">
            <p class="example">
              {#if cfg.assistantHotkey}
                Press {formatHotkeySpec(cfg.assistantHotkey)} in any app and ask <q>What's on my notes for the launch?</q>: Yap answers out loud, and the conversation is in Chat.
              {:else}
                Set a shortcut below, press it in any app and ask <q>What's on my notes for the launch?</q>: Yap answers out loud, and the conversation is in Chat.
              {/if}
              {#if cfg.openApps}Or say <q>Open Spotify</q>, and Yap opens it.{/if}
              {#if cfg.webSearch}Or ask <q>Where do I find Gale in Baldur's Gate 3?</q>, and Yap looks it up on the web.{/if}
            </p>
            <p class="crossref">
              Want Yap to write something into the app you're in?
              <button class="set-link" onclick={() => goto('commands')}>Commands</button>
            </p>
          </div>
          <Group title="Assistant shortcut" id="settings-assistant-shortcut">
            <Row
              label="Assistant shortcut"
              desc={cfg.recordingMode === 'pushToTalk'
                ? 'Hold it while you ask, or double-tap it to ask hands-free. It never types into your app'
                : cfg.assistantStopOnPause
                  ? 'Press it and ask: Yap stops listening when you pause. It never types into your app'
                  : 'Press it, ask, press it again. It never types into your app'}
              hint="Press it while Yap speaks to cut in. Answers are read aloud with Speak replies on, and shown on the Yap bar otherwise. Paused during meetings and calls."
              setting="assistant-shortcut"
            >
              <SettingsHotkey {cfg} target="assistantHotkey" clearable />
            </Row>
            {#if !cfg.assistantHotkey}
              <Row>
                <p class="set-note">Set an assistant shortcut to ask Yap from any app, even while its window is closed.</p>
              </Row>
            {/if}
            <!-- "Stop listening when I pause" (pause.rs): a pause after the
                 question ends it, as a second press would. -->
            <Row setting="stop-on-pause">
              <Toggle
                bind:checked={cfg.assistantStopOnPause}
                label="Stop listening when I pause"
                desc={cfg.recordingMode === 'pushToTalk'
                  ? 'Asking hands-free (a double-tap), a pause after your question sends it'
                  : "A pause after your question sends it, so you needn't press the shortcut again"}
                hint="A second and a half of quiet once you've said something; the Yap bar shows it running out. Before you start talking, Yap waits as long as you need. The shortcut still ends it at once, and while you hold it down, letting go does."
              />
            </Row>
            <Row setting="answer-card">
              <Toggle
                bind:checked={cfg.assistantAnswerCard}
                label="Show the answer on the Yap bar"
                desc="Your question and its answer, above the bar while Yap reads it out"
                hint="Off: just the voice, with ■ on the bar to stop it. An answer Yap couldn't read aloud still shows."
              />
            </Row>
          </Group>
          <!-- Read aloud, anywhere (read_aloud.rs): any selected text, in
               the voice below, with the reading card on the Yap bar. -->
          <Group title="Read aloud" id="settings-read-aloud">
            <Row
              label="Read aloud shortcut"
              desc="Select text in any app and press it: Yap reads it in the voice below, each word lit up on the Yap bar as it's said"
              hint="Press it again, or Esc, to stop. 🔊 on the Yap bar and the tray's Read selection aloud do the same, and notes and Chat answers have Read aloud too. The text is in your reading font (General → Appearance)."
              setting="read-shortcut"
            >
              <SettingsHotkey {cfg} target="readHotkey" clearable />
            </Row>
          </Group>
          <!-- "Open Teams": apps installed on this PC (apps.rs). -->
          <OpenAppsSettings {cfg} />
          <!-- Looking things up on the web (web.rs), for Chat too. -->
          <WebSearchSettings {cfg} {goto} />
          <Group title="Spoken replies in Chat" id="settings-spoken-replies">
            <Row setting="speak-replies">
              <Toggle
                bind:checked={() => cfg.speakReplies, setSpeakReplies}
                label="Speak replies"
                desc="Yap reads its answers aloud, in Chat and from the assistant shortcut. Off, Yap never speaks"
                hint="The same switch as Voice replies in Chat."
              />
              <KokoroOffer bind:open={offerKokoro} {cfg} />
            </Row>
            <Row label="When to speak" desc={SPEAK_WHEN_DESC[cfg.speakWhen] ?? SPEAK_WHEN_DESC.voice} setting="speak-when">
              <Segmented bind:value={cfg.speakWhen} options={SPEAK_WHEN} label="When to speak" disabled={!cfg.speakReplies} />
            </Row>
            <Row setting="talk-back">
              <Toggle
                bind:checked={cfg.talkBackHandsFree}
                label="Talk back hands-free"
                desc="A question you dictate into Chat is sent when you stop, and the answer read out"
                hint={fillHotkey('Press the mic or {key} to ask the next one: it cuts Yap off mid-answer.', cfg.hotkey)}
                disabled={!cfg.speakReplies}
              />
            </Row>
          </Group>
          <Group title="Voice" id="settings-voice">
            <VoiceSettings {cfg} />
          </Group>
          <Group title="Model" id="settings-assistant-model">
            <LinkRow
              label="Model"
              desc="The same as Chat's, in Language Models"
              value={scopeModelLabel('chat')}
              setting="assistant-model"
              onclick={() => openScope('chat')}
            />
          </Group>

        {:else if section === 'models'}
          <div class="page-h">
            <h1>Speech-to-Text</h1>
            <p>The model on this PC that turns your voice into text.</p>
          </div>
          <!-- Your model first (with the red badge's reason, when there's
               none), then the catalogue: ModelManager.svelte. -->
          <ModelManager />
          <!-- Speaker diarization's models + its switch (diarize.rs); the
               Meetings page has the switch too and links here. -->
          <Group title="Speakers in meetings" id="settings-speakers">
            <Row setting="speakers">
              <SpeakerModels bind:enabled={cfg.meetingDiarization} bind:model={cfg.diarizationModel} />
            </Row>
          </Group>
          <Group title="Language" id="settings-stt-language">
            <Row
              label="Language"
              desc={langInfo.supportsLanguage ? 'The language you speak. Auto-detect works for most models' : 'This model uses a fixed language'}
              setting="language"
            >
              <Select bind:value={cfg.selectedLanguage} options={langOptions} ariaLabel="Language" disabled={!langInfo.supportsLanguage} />
            </Row>
            <Row setting="translate">
              <Toggle
                bind:checked={cfg.translateToEnglish}
                label="Translate to English"
                desc={langInfo.supportsTranslate ? 'Output English whatever language you speak' : 'This model can’t translate'}
                disabled={!langInfo.supportsTranslate}
              />
            </Row>
          </Group>
          <Group title="Performance" id="settings-performance">
            <!-- The graphics card or the processor, measured per model on
                 this PC (stt_speed.rs). -->
            <RunsOn {cfg} />
            <Row label="Unload model when idle" desc="Frees memory between dictations. It loads again for the next one" setting="unload">
              <Select bind:value={cfg.modelUnloadTimeout} options={UNLOAD_TIMEOUTS} ariaLabel="Unload model when idle" />
            </Row>
          </Group>

        {:else if section === 'cleanup'}
          <LanguageModelsPage {cfg} {goto} />

        {:else if section === 'privacy'}
          <PrivacyPage {cfg} {goto} bind:connections={privacyConnections} />

        {:else if section === 'about'}
          <AboutPage {cfg} version={APP_VERSION} {goto} />

        {:else if section === 'memory'}
          <div class="page-h">
            <h1>Memory</h1>
            <p>What Yap remembers about you, so its AI answers you better. See, change or clear any of it.</p>
          </div>
          <MemorySection {cfg} {goto} />

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
  /* Commands / Voice Assistant: what the page does, in one example, and the
     way to the other one (they're easy to mix up). */
  .explain {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: -8px 0 28px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-raised-soft);
  }
  .explain p {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
  }
  .explain .example {
    color: var(--yap-fg);
  }
  .explain q {
    font-style: italic;
  }
  .explain .crossref {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    color: var(--yap-muted);
  }
  .loading {
    color: var(--yap-muted);
    padding: 20px;
  }
</style>
