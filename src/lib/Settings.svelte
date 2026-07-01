<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, onDestroy } from 'svelte';
  import yapIcon from '../assets/yap-icon.png';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Select from './ui/Select.svelte';
  import Slider from './ui/Slider.svelte';
  import Button from './ui/Button.svelte';
  import Input from './ui/Input.svelte';
  import Textarea from './ui/Textarea.svelte';
  import ModelManager from './ModelManager.svelte';
  import StatusBar from './StatusBar.svelte';

  let cfg = $state(null);
  let loaded = $state(false); // gates auto-save until the initial config loads
  let devices = $state([]);
  let outputs = $state([]); // audio output device names (for the chime)
  let recording = $state(false); // hotkey-recorder active
  let saved = $state(false);
  let section = $state('general'); // general | models | advanced | about
  // Language/translate capability of the active model (drives Models section).
  let langInfo = $state({ supportsLanguage: false, supportsTranslate: false, languages: [] });

  // Auto-update state. status: idle | checking | available | uptodate |
  // installing | error | unsupported (portable). The updater JS plugin only
  // works in packaged builds, so every call is wrapped in try/catch and the UI
  // degrades quietly in dev.
  let update = $state({ status: 'idle', version: '', progress: 0 });
  let unlistenUpdateEvent = null;

  const SECTIONS = [
    { id: 'general', label: 'General' },
    { id: 'models', label: 'Models' },
    { id: 'cleanup', label: 'AI Cleanup' },
    { id: 'history', label: 'History' },
    { id: 'advanced', label: 'Advanced' },
    { id: 'about', label: 'About' },
  ];

  // AI cleanup provider presets. Selecting one fills in the base URL; "custom"
  // leaves it editable. The backend only ever uses ppBaseUrl.
  const PP_PROVIDERS = [
    { value: 'groq', label: 'Groq', baseUrl: 'https://api.groq.com/openai/v1' },
    { value: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com/v1' },
    { value: 'openrouter', label: 'OpenRouter', baseUrl: 'https://openrouter.ai/api/v1' },
    { value: 'local', label: 'Local (Ollama · LM Studio)', baseUrl: 'http://localhost:11434/v1' },
    { value: 'custom', label: 'Custom', baseUrl: null },
  ];

  // Cleanup presets: each fills the editable "body" (tone/format). The immutable
  // guardrails live in the backend (llm::BASE_PROMPT) and are always applied, so
  // a preset only changes behaviour, never the refusal rules. "custom" = the user
  // edited the body by hand. Keep `default` in sync with default_pp_prompt() (Rust).
  const PP_PRESETS = [
    {
      value: 'default',
      label: 'Default',
      body:
        'Remove filler words (um, uh, er, like, you know). Fix capitalization, punctuation, and obvious grammar. Resolve spoken self-corrections (e.g. "go to the store, no wait, the bank" → "go to the bank"). Keep the result faithful and natural — don\'t over-format.',
    },
    {
      value: 'email',
      label: 'Email',
      body:
        'Remove filler words and fix grammar, punctuation, and capitalization. Format the result as a clear, professional email body with complete sentences and sensible paragraphs. Keep a polite, professional tone. Don\'t add a greeting or sign-off unless it was dictated.',
    },
    {
      value: 'notes',
      label: 'Notes',
      body:
        'Remove filler words and fix grammar and punctuation. Format the result as concise notes: turn spoken lists into bullet points and trim hedging. Keep it terse and scannable.',
    },
    {
      value: 'slack',
      label: 'Slack / Chat',
      body:
        'Remove filler words and fix obvious grammar. Keep it casual and brief, like a chat message — light punctuation and a conversational tone. Don\'t over-formalize or expand.',
    },
    {
      value: 'code',
      label: 'Code / Technical',
      body:
        'Remove filler words and fix punctuation. This is technical dictation: preserve technical terms, identifiers, file names, code symbols, and casing exactly as spoken — never "correct" jargon, library, or command names. Format numbers and inline code precisely.',
    },
    { value: 'custom', label: 'Custom', body: null },
  ];

  // Example model ids per provider, shown as the Model field hint.
  const PP_MODEL_HINTS = {
    groq: 'e.g. llama-3.1-8b-instant',
    openai: 'e.g. gpt-4o-mini',
    openrouter: 'e.g. meta-llama/llama-3.1-8b-instruct',
    local: 'your Ollama / LM Studio model name (e.g. llama3.1)',
    custom: 'the model id your endpoint expects',
  };

  // AI cleanup Test button state.
  let ppTest = $state({ running: false, result: '', error: '' });

  // History / stats dashboard state.
  let stats = $state(null);
  let historyItems = $state([]);

  async function loadHistory() {
    try {
      [stats, historyItems] = await Promise.all([
        invoke('get_stats'),
        invoke('get_history', { limit: 50 }),
      ]);
    } catch (e) {
      stats = null;
      historyItems = [];
    }
  }

  async function clearHistory() {
    await invoke('clear_history');
    await loadHistory();
  }

  // Refresh the dashboard whenever the History tab is opened.
  $effect(() => {
    if (section === 'history') loadHistory();
  });

  // Populate the per-app routing picker (from history) when AI Cleanup opens.
  $effect(() => {
    if (section === 'cleanup' && loaded) loadRecentApps();
  });

  function fmtMinutes(min) {
    const m = Math.round(min || 0);
    if (m < 60) return `${m} min`;
    const h = Math.floor(m / 60);
    return `${h} h ${m % 60} min`;
  }
  function fmtWhen(ts) {
    try {
      return new Date((ts || 0) * 1000).toLocaleString();
    } catch {
      return '';
    }
  }

  // Daily Groq usage meter (camelCase from get_groq_usage / groq-usage event).
  let usage = $state({ day: 0, tokens: 0, tokenCap: 500000, requests: 0, requestCap: 14400 });
  let unlistenUsage = null;

  // Compact token formatter: <1000 → as-is, else "84.2k".
  function fmtK(n) {
    const v = Number(n) || 0;
    return v < 1000 ? String(v) : `${(v / 1000).toFixed(1)}k`;
  }
  // 0–100 percentage of a value against a cap (guards a zero/absent cap).
  function pctOf(value, cap) {
    const c = Number(cap) || 0;
    if (c <= 0) return 0;
    return Math.min(100, Math.round((Number(value) || 0) / c * 100));
  }
  // Pulse-style colour ramp: green < 70%, amber 70–90%, red ≥ 90%.
  function usageColor(pct) {
    if (pct >= 90) return '#ef4444';
    if (pct >= 70) return '#f59e0b';
    return '#22c55e';
  }

  async function refreshUsage() {
    try {
      usage = await invoke('get_groq_usage');
    } catch {
      /* backend not ready / stub — leave defaults */
    }
  }

  const RECORDING_MODES = [
    { value: 'toggle', label: 'Toggle (press to start/stop)' },
    { value: 'pushToTalk', label: 'Push-to-talk (hold to record)' },
  ];

  const UNLOAD_TIMEOUTS = [
    { value: 'never', label: 'Never (keep model loaded)' },
    { value: '1min', label: 'After 1 minute' },
    { value: '5min', label: 'After 5 minutes' },
    { value: '15min', label: 'After 15 minutes' },
    { value: '30min', label: 'After 30 minutes' },
  ];

  const OVERLAY_POSITIONS = [
    { value: 'bottom', label: 'Bottom' },
    { value: 'top', label: 'Top' },
  ];

  const AUTO_SUBMIT_KEYS = [
    { value: 'enter', label: 'Enter' },
    { value: 'ctrlEnter', label: 'Ctrl + Enter' },
    { value: 'shiftEnter', label: 'Shift + Enter' },
  ];

  // Display names for the language codes the backend returns.
  const LANG_NAMES = {
    en: 'English', es: 'Spanish', fr: 'French', de: 'German', it: 'Italian',
    pt: 'Portuguese', nl: 'Dutch', pl: 'Polish', ru: 'Russian', uk: 'Ukrainian',
    zh: 'Chinese', ja: 'Japanese', ko: 'Korean', ar: 'Arabic', hi: 'Hindi',
    tr: 'Turkish', yue: 'Cantonese',
  };

  // Defaults for fields a stale config.json (or an in-flight backend) might
  // not yet include, so the bindings always have something to bind to.
  const FIELD_DEFAULTS = {
    recordingMode: 'toggle',
    muteWhileRecording: false,
    appendTrailingSpace: false,
    autoSubmit: false,
    restoreClipboard: true,
    startHidden: false,
    showTrayIcon: true,
    autostart: false,
    audioFeedbackVolume: 1.0,
    soundEnabled: true,
    useGpu: true,
    pillScale: 1.0,
    showPill: false,
    showOverlay: true,
    streamingPartials: false,
    historyEnabled: true,
    inputDevice: null,
    dictionary: [],
    selectedLanguage: 'auto',
    translateToEnglish: false,
    modelUnloadTimeout: 'never',
    outputDevice: null,
    overlayPosition: 'bottom',
    autoSubmitKey: 'enter',
    updateChecksEnabled: true,
    postProcessEnabled: false,
    ppProvider: 'groq',
    ppBaseUrl: 'https://api.groq.com/openai/v1',
    ppApiKey: '',
    ppModel: 'llama-3.1-8b-instant',
    ppPreset: 'default',
    ppPrompt:
      'Remove filler words (um, uh, er, like, you know). Fix capitalization, punctuation, and obvious grammar. Resolve spoken self-corrections (e.g. "go to the store, no wait, the bank" → "go to the bank"). Keep the result faithful and natural — don\'t over-format.',
    routingScope: 'all_apps',
    appRoutes: [],
    cleanupProfiles: [],
    editHotkey: '',
  };

  // Real running version — read from Tauri, not hardcoded (was stuck at '0.1.0').
  let APP_VERSION = $state('');

  onMount(async () => {
    try {
      if ('__TAURI_INTERNALS__' in window) {
        const { getVersion } = await import('@tauri-apps/api/app');
        APP_VERSION = await getVersion();
      }
    } catch {
      /* not in Tauri — leave blank */
    }
    const stored = await invoke('get_config');
    cfg = { ...FIELD_DEFAULTS, ...stored };
    if (!Array.isArray(cfg.dictionary)) cfg.dictionary = [];
    if (!Array.isArray(cfg.appRoutes)) cfg.appRoutes = [];
    if (!Array.isArray(cfg.cleanupProfiles)) cfg.cleanupProfiles = [];
    const migrated = migrateRoutesToProfiles();
    loaded = true;
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

    // Auto-check for updates on launch (the settings webview loads at startup,
    // even while hidden). Skip in dev: there's no published release, so the
    // updater plugin would just log a 404 error every launch. Released builds
    // (import.meta.env.DEV === false) check normally.
    if (cfg.updateChecksEnabled && !import.meta.env.DEV) checkForUpdate(false);

    // A "Check for updates" tray item emits this; run a manual check.
    try {
      unlistenUpdateEvent = await listen('check-for-updates', () => {
        section = 'about';
        checkForUpdate(true);
      });
    } catch {
      unlistenUpdateEvent = null;
    }

    // Daily Groq usage: fetch once, then update live as dictations come in.
    refreshUsage();
    try {
      unlistenUsage = await listen('groq-usage', (e) => {
        if (e?.payload) usage = e.payload;
      });
    } catch {
      unlistenUsage = null;
    }
  });

  onDestroy(() => {
    stopRecord();
    if (unlistenUpdateEvent) unlistenUpdateEvent();
    if (unlistenUsage) unlistenUsage();
  });

  // ---- Auto-update ----
  async function checkForUpdate(manual = false) {
    if (update.status === 'checking' || update.status === 'installing') return;
    update = { ...update, status: 'checking' };
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const result = await check();
      if (result) {
        update = { ...update, status: 'available', version: result.version || '' };
      } else if (manual) {
        update = { ...update, status: 'uptodate' };
        setTimeout(() => {
          if (update.status === 'uptodate') update = { ...update, status: 'idle' };
        }, 3000);
      } else {
        update = { ...update, status: 'idle' };
      }
    } catch (e) {
      // Updater is unavailable in dev / unpackaged builds — fail silently for
      // the automatic check, surface a brief note for a manual one.
      console.warn('Update check failed:', e);
      if (manual) {
        update = { ...update, status: 'error' };
        setTimeout(() => {
          if (update.status === 'error') update = { ...update, status: 'idle' };
        }, 4000);
      } else {
        update = { ...update, status: 'idle' };
      }
    }
  }

  async function installUpdate() {
    if (update.status !== 'available') return;
    // Portable installs can't be replaced in place — point the user at GitHub.
    const portable = await invoke('is_portable').catch(() => false);
    if (portable) {
      update = { ...update, status: 'unsupported' };
      return;
    }
    update = { ...update, status: 'installing', progress: 0 };
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const { relaunch } = await import('@tauri-apps/plugin-process');
      const result = await check();
      if (!result) {
        update = { ...update, status: 'idle' };
        return;
      }
      let downloaded = 0;
      let total = 0;
      await result.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          total = event.data?.contentLength ?? 0;
          downloaded = 0;
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          update = {
            ...update,
            progress: total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0,
          };
        }
      });
      await relaunch();
    } catch (e) {
      console.error('Update install failed:', e);
      update = { ...update, status: 'error' };
    }
  }

  const micOptions = $derived([
    { value: null, label: 'System default' },
    ...devices.map((d) => ({ value: d, label: d })),
  ]);

  const outputOptions = $derived([
    { value: null, label: 'System default' },
    ...outputs.map((d) => ({ value: d, label: d })),
  ]);

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

  // ---- Hotkey recorder ----
  // `recordTarget` is which config field the recorder writes to ('hotkey' for
  // dictation, 'editHotkey' for edit/rewrite mode) and which live-binding command
  // to pause/re-apply.
  let recordTarget = $state('hotkey');
  const HOTKEY_CMD = { hotkey: 'configure_hotkey', editHotkey: 'configure_edit_hotkey' };
  function startRecord(target = 'hotkey') {
    if (!cfg) return;
    recordTarget = target;
    recording = true;
    invoke(HOTKEY_CMD[target], { spec: '' }); // pause live binding while choosing
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('mousedown', onMouse, true);
  }
  function stopRecord() {
    if (!recording) return;
    recording = false;
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('mousedown', onMouse, true);
    if (cfg) invoke(HOTKEY_CMD[recordTarget], { spec: cfg[recordTarget] }); // apply live
  }
  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return stopRecord();
    // Backspace clears the (optional) edit hotkey.
    if (e.key === 'Backspace' && recordTarget === 'editHotkey') {
      cfg.editHotkey = '';
      return stopRecord();
    }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;
    cfg[recordTarget] = `kb:${e.keyCode}`;
    stopRecord();
  }
  function onMouse(e) {
    if (e.button === 0 || e.button === 2) return; // left/right reserved for UI
    e.preventDefault();
    e.stopPropagation();
    const map = { 1: 3, 3: 4, 4: 5 }; // browser button -> our id
    const id = map[e.button] ?? e.button + 1;
    cfg[recordTarget] = `mouse:${id}`;
    stopRecord();
  }

  function formatHotkey(spec) {
    if (!spec) return 'None';
    if (spec.startsWith('mouse:')) return `Mouse button ${spec.slice(6)}`;
    const m = spec.match(/^kb:(\d+)$/);
    return m ? vkeyName(+m[1]) : spec;
  }
  function vkeyName(v) {
    if (v >= 112 && v <= 123) return `F${v - 111}`;
    if ((v >= 48 && v <= 57) || (v >= 65 && v <= 90)) return String.fromCharCode(v);
    const named = { 32: 'Space', 13: 'Enter', 9: 'Tab', 8: 'Backspace', 192: '`' };
    return named[v] || `Key ${v}`;
  }

  // ---- Pill size (live preview) ----
  function onScale() {
    invoke('set_pill_scale', { scale: Number(cfg.pillScale) });
  }

  // ---- Show/hide pill (apply immediately + persist on Save) ----
  function onShowPill(visible) {
    invoke('set_pill_visible', { visible });
  }

  // ---- Autostart (apply immediately + persist on Save) ----
  async function onAutostart(enabled) {
    try {
      await invoke('set_autostart', { enabled });
    } catch (e) {
      // revert the toggle if the OS rejected it
      cfg.autostart = !enabled;
    }
  }

  // ---- Dictionary ----
  function addEntry() {
    cfg.dictionary = [...cfg.dictionary, { from: '', to: '' }];
  }
  function removeEntry(i) {
    cfg.dictionary = cfg.dictionary.filter((_, j) => j !== i);
  }

  // ---- AI cleanup ----
  // Picking a provider preset fills in its base URL (except "custom", which
  // leaves the field editable).
  function onProviderChange(value) {
    const preset = PP_PROVIDERS.find((p) => p.value === value);
    if (preset && preset.baseUrl) cfg.ppBaseUrl = preset.baseUrl;
  }

  // Picking a cleanup preset overwrites the editable body with its text. "Custom"
  // leaves whatever's there. Editing the body by hand flips the preset to Custom.
  function onPresetChange(value) {
    const preset = PP_PRESETS.find((p) => p.value === value);
    if (preset && preset.body != null) cfg.ppPrompt = preset.body;
  }
  function onPromptInput(value) {
    cfg.ppPrompt = value;
    if (cfg.ppPreset !== 'custom') cfg.ppPreset = 'custom';
  }

  // ---- Smart routing (reusable profiles + per-app rules) ----
  // Apps the user has actually dictated into, pulled from local history so the
  // "Add rule" picker suggests real targets instead of asking them to type
  // "slack.exe". Populated lazily when the routing UI first renders.
  let recentApps = $state([]);
  let newRouteApp = $state('');
  async function loadRecentApps() {
    try {
      const entries = await invoke('get_history', { limit: 200 });
      const bound = new Set((cfg.appRoutes || []).map((r) => (r.app || '').toLowerCase()));
      const seen = new Map();
      for (const e of entries || []) {
        const app = (e.app || '').trim();
        if (!app || bound.has(app.toLowerCase())) continue;
        if (!seen.has(app.toLowerCase())) seen.set(app.toLowerCase(), app);
      }
      recentApps = [...seen.values()].sort((a, b) => a.localeCompare(b));
    } catch {
      recentApps = [];
    }
  }
  // Strip the ".exe" for a friendlier default label (e.g. "slack.exe" → "Slack").
  function prettyAppLabel(app) {
    const base = (app || '').replace(/\.exe$/i, '');
    return base ? base.charAt(0).toUpperCase() + base.slice(1) : app;
  }

  let idSeq = 1;
  function newId() {
    return globalThis.crypto?.randomUUID?.() ?? `p_${Date.now()}_${idSeq++}`;
  }

  // Migrate any legacy inline-body rules (pre-profiles) into named profiles so
  // the whole app now speaks "profiles". Runs once on load. Returns whether it
  // changed anything (so the caller can persist — otherwise it'd re-migrate and
  // duplicate profiles on every load until the next save).
  function migrateRoutesToProfiles() {
    if (!Array.isArray(cfg.cleanupProfiles)) cfg.cleanupProfiles = [];
    let migrated = false;
    for (const r of cfg.appRoutes || []) {
      if (!r.profileId && (r.prompt || '').trim()) {
        const id = newId();
        cfg.cleanupProfiles = [
          ...cfg.cleanupProfiles,
          { id, name: r.label || prettyAppLabel(r.app), prompt: r.prompt },
        ];
        r.profileId = id;
        r.prompt = '';
        migrated = true;
      }
    }
    return migrated;
  }

  // ---- Profiles library ----
  function addProfile(seedBody = '', name = '') {
    const id = newId();
    const n = name || `Profile ${cfg.cleanupProfiles.length + 1}`;
    cfg.cleanupProfiles = [...cfg.cleanupProfiles, { id, name: n, prompt: seedBody }];
    return id;
  }
  function addProfileFromPreset(value) {
    const preset = PP_PRESETS.find((p) => p.value === value);
    if (!preset || preset.value === 'custom') return addProfile('', '');
    addProfile(preset.body ?? '', preset.label);
  }
  function removeProfile(id) {
    cfg.cleanupProfiles = cfg.cleanupProfiles.filter((p) => p.id !== id);
    // Unbind any rules that pointed at it (they fall back to the global default).
    for (const r of cfg.appRoutes) if (r.profileId === id) r.profileId = '';
  }

  // ---- App rules ----
  function addRoute(app) {
    const name = (app || newRouteApp || '').trim();
    if (!name) return;
    if ((cfg.appRoutes || []).some((r) => (r.app || '').toLowerCase() === name.toLowerCase())) return;
    // Ensure at least one profile exists to bind to (seed from the global body).
    let profileId = cfg.cleanupProfiles[0]?.id;
    if (!profileId) profileId = addProfile(cfg.ppPrompt || '', 'Default');
    cfg.appRoutes = [
      ...(cfg.appRoutes || []),
      { app: name, label: prettyAppLabel(name), profileId, prompt: '' },
    ];
    newRouteApp = '';
    recentApps = recentApps.filter((a) => a.toLowerCase() !== name.toLowerCase());
  }
  function removeRoute(i) {
    cfg.appRoutes = cfg.appRoutes.filter((_, j) => j !== i);
    loadRecentApps();
  }

  async function testCleanup() {
    if (ppTest.running) return;
    // Persist first so the backend tests the settings the user is looking at.
    await persist();
    ppTest = { running: true, result: '', error: '' };
    try {
      const cleaned = await invoke('test_post_process', {
        text: 'um so like i think we should uh go to the the bank tomorrow',
      });
      ppTest = { running: false, result: cleaned, error: '' };
    } catch (e) {
      ppTest = { running: false, result: '', error: String(e) };
    }
    // The Test call goes through cleanup(), so usage moved — refresh the meter.
    refreshUsage();
  }

  // Build the cleaned config payload the backend expects (trimmed dictionary,
  // numeric fields, null device sentinels). Does NOT mutate `cfg` — so calling
  // it from the auto-save effect can't re-trigger the effect.
  function buildClean() {
    return {
      ...cfg,
      inputDevice: cfg.inputDevice || null,
      outputDevice: cfg.outputDevice || null,
      pillScale: Number(cfg.pillScale),
      audioFeedbackVolume: Number(cfg.audioFeedbackVolume),
      dictionary: cfg.dictionary
        .map((e) => ({ from: (e.from || '').trim(), to: (e.to || '').trim() }))
        .filter((e) => e.from),
      appRoutes: (cfg.appRoutes || [])
        .map((r) => ({
          app: (r.app || '').trim(),
          label: (r.label || '').trim() || (r.app || '').trim(),
          profileId: r.profileId || '',
          prompt: r.prompt || '',
        }))
        .filter((r) => r.app),
      cleanupProfiles: (cfg.cleanupProfiles || [])
        .map((p) => ({
          id: p.id,
          name: (p.name || '').trim() || 'Untitled',
          prompt: p.prompt || '',
        }))
        .filter((p) => p.id),
    };
  }

  async function persist() {
    await invoke('save_config', { cfg: buildClean() });
    saved = true;
    setTimeout(() => (saved = false), 1200);
  }

  // ---- Auto-save: debounce any cfg change, then persist ----
  let saveTimer = null;
  let primed = false; // skip the first run (initial load) so we don't save on open
  $effect(() => {
    if (!loaded || !cfg) return;
    // Deep-read every field so the effect re-runs on any change (incl. dictionary).
    JSON.stringify(cfg);
    if (!primed) {
      primed = true;
      return;
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(persist, 400);
  });

  function onCheckUpdates() {
    section = 'about';
    checkForUpdate(true);
  }
</script>

{#snippet navIcon(id)}
  {#if id === 'general'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="9" y="2" width="6" height="12" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0" />
      <path d="M12 18v4" />
    </svg>
  {:else if id === 'models'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
    </svg>
  {:else if id === 'cleanup'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6z" />
      <path d="M18 14l.7 2 2 .7-2 .7-.7 2-.7-2-2-.7 2-.7z" />
    </svg>
  {:else if id === 'history'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5" />
      <path d="M12 8v4l3 2" />
    </svg>
  {:else if id === 'advanced'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M4 6h10M18 6h2M4 12h2M10 12h10M4 18h7M15 18h5" />
      <circle cx="16" cy="6" r="2" />
      <circle cx="8" cy="12" r="2" />
      <circle cx="13" cy="18" r="2" />
    </svg>
  {:else if id === 'about'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 16v-4" />
      <path d="M12 8h.01" />
    </svg>
  {/if}
{/snippet}

<div class="shell">
  <nav class="sidebar">
    <div class="brand">
      <img class="brandlogo" src={yapIcon} alt="" aria-hidden="true" />
      <span class="brandname">Yap</span>
    </div>
    <div class="brand-divider"></div>
    {#each SECTIONS as s (s.id)}
      <button class="navitem" class:active={section === s.id} onclick={() => (section = s.id)}>
        <span class="navicon">{@render navIcon(s.id)}</span>
        <span class="navlabel">{s.label}</span>
      </button>
    {/each}
  </nav>

  <main>
    {#if cfg}
      <div class="content">
        {#if section === 'general'}
          <Group title="Activation">
            <Row label="Hotkey" hint="Trigger dictation from any app">
              <button
                class="key"
                class:recording={recording && recordTarget === 'hotkey'}
                onclick={() => startRecord('hotkey')}
              >
                {recording && recordTarget === 'hotkey' ? 'Press a key…' : formatHotkey(cfg.hotkey)}
              </button>
            </Row>
            <Row label="Recording mode" hint="How the hotkey controls recording">
              <Select bind:value={cfg.recordingMode} options={RECORDING_MODES} />
            </Row>
            <Row
              label="Edit / rewrite hotkey"
              hint="Select text, hold this key, and speak an instruction (e.g. “make this a bulleted list”) to rewrite it in place. Needs AI cleanup configured. Backspace clears."
            >
              <button
                class="key"
                class:recording={recording && recordTarget === 'editHotkey'}
                onclick={() => startRecord('editHotkey')}
              >
                {recording && recordTarget === 'editHotkey'
                  ? 'Press a key…'
                  : formatHotkey(cfg.editHotkey)}
              </button>
            </Row>
          </Group>

          <Group title="Audio">
            <Row label="Microphone" hint="Applies after restart">
              <Select bind:value={cfg.inputDevice} options={micOptions} />
            </Row>
            <Row label="Output device" hint="Where the start/stop chime plays">
              <Select bind:value={cfg.outputDevice} options={outputOptions} disabled={!cfg.soundEnabled} />
            </Row>
            <Row>
              <Toggle bind:checked={cfg.soundEnabled} label="Sound cue" hint="Chime when recording starts/stops" />
            </Row>
            <Row>
              <Slider
                bind:value={cfg.audioFeedbackVolume}
                min={0}
                max={1}
                step={0.05}
                label="Cue volume"
                disabled={!cfg.soundEnabled}
                hint={`${Math.round(cfg.audioFeedbackVolume * 100)}%`}
              />
            </Row>
            <Row>
              <Toggle bind:checked={cfg.muteWhileRecording} label="Mute while recording" hint="Silence system audio output while you dictate" />
            </Row>
          </Group>

          <Group title="Appearance">
            <Row>
              <Toggle
                bind:checked={cfg.showPill}
                label="Show pill"
                hint="The floating dictation pill (you can still use the hotkey when hidden)"
                onchange={onShowPill}
              />
            </Row>
            <Row>
              <Toggle
                bind:checked={cfg.showOverlay}
                label="Show transcribing overlay"
                hint="A floating indicator at the bottom of the screen while you dictate"
              />
            </Row>
            <Row label="Overlay position" hint="Where the transcribing overlay appears">
              <Select bind:value={cfg.overlayPosition} options={OVERLAY_POSITIONS} disabled={!cfg.showOverlay} />
            </Row>
            <Row>
              <Toggle
                bind:checked={cfg.streamingPartials}
                label="Live partial transcript (experimental)"
                hint="Show words in the overlay as you speak. Re-transcribes on a timer, so it adds GPU load. The final result on stop is always authoritative."
              />
            </Row>
            <Row>
              <Slider
                bind:value={cfg.pillScale}
                min={0.6}
                max={1.4}
                step={0.05}
                label="Pill size"
                hint={`${Math.round(cfg.pillScale * 100)}%`}
                oninput={onScale}
              />
            </Row>
          </Group>

        {:else if section === 'models'}
          <Group title="Speech model">
            <Row>
              {#snippet children()}
                <div class="mm-wrap"><ModelManager /></div>
              {/snippet}
            </Row>
          </Group>
          <Group title="Language">
            <Row
              label="Language"
              hint={langInfo.supportsLanguage
                ? 'Spoken language (auto-detect works for most models)'
                : 'This model uses a fixed language'}
            >
              <Select
                bind:value={cfg.selectedLanguage}
                options={langOptions}
                disabled={!langInfo.supportsLanguage}
              />
            </Row>
            <Row>
              <Toggle
                bind:checked={cfg.translateToEnglish}
                label="Translate to English"
                hint={langInfo.supportsTranslate
                  ? 'Output English regardless of the spoken language'
                  : 'This model can’t translate'}
                disabled={!langInfo.supportsTranslate}
              />
            </Row>
          </Group>
          <Group title="Performance">
            <Row>
              <Toggle bind:checked={cfg.useGpu} label="GPU acceleration (Whisper)" hint="Runs Whisper models on your GPU via Vulkan. ONNX models (Parakeet etc.) always use the GPU via DirectML. Applies after restart." />
            </Row>
            <Row label="Unload model when idle" hint="Free memory when not dictating; reloads on next use">
              <Select bind:value={cfg.modelUnloadTimeout} options={UNLOAD_TIMEOUTS} />
            </Row>
          </Group>

        {:else if section === 'cleanup'}
          <Group title="AI Cleanup">
            <Row>
              <Toggle
                bind:checked={cfg.postProcessEnabled}
                label="Enable AI cleanup"
                hint="Clean up filler, punctuation & grammar before pasting. Off = raw transcript."
              />
            </Row>
            <Row label="Provider" hint="Where the cleanup runs">
              <Select
                bind:value={cfg.ppProvider}
                options={PP_PROVIDERS}
                onchange={onProviderChange}
                disabled={!cfg.postProcessEnabled}
              />
            </Row>
            <Row label="Base URL" hint="OpenAI-compatible endpoint">
              {#snippet children()}
                <div class="pp-field">
                  <Input bind:value={cfg.ppBaseUrl} disabled={!cfg.postProcessEnabled} placeholder="https://api.groq.com/openai/v1" />
                </div>
              {/snippet}
            </Row>
            <Row label="API key" hint="Stored locally; not needed for Local providers">
              {#snippet children()}
                <div class="pp-field">
                  <Input type="password" bind:value={cfg.ppApiKey} disabled={!cfg.postProcessEnabled} placeholder="sk-…" />
                </div>
              {/snippet}
            </Row>
            <Row label="Model" hint={PP_MODEL_HINTS[cfg.ppProvider] || ''}>
              {#snippet children()}
                <div class="pp-field">
                  <Input bind:value={cfg.ppModel} disabled={!cfg.postProcessEnabled} placeholder="llama-3.1-8b-instant" />
                </div>
              {/snippet}
            </Row>
            <Row label="Preset" hint="Tone & formatting for the cleanup">
              <Select
                bind:value={cfg.ppPreset}
                options={PP_PRESETS}
                onchange={onPresetChange}
                disabled={!cfg.postProcessEnabled}
              />
            </Row>
            <Row>
              {#snippet children()}
                <div class="pp-prompt">
                  <div class="pp-label">Cleanup instructions</div>
                  <div class="pp-sub">
                    How the model should format & tone the text. Yap always applies its
                    built-in safety rules (clean the text, never answer it) on top of this.
                  </div>
                  <Textarea
                    value={cfg.ppPrompt}
                    oninput={onPromptInput}
                    rows={6}
                    disabled={!cfg.postProcessEnabled}
                  />
                </div>
              {/snippet}
            </Row>
            <Row>
              {#snippet children()}
                <div class="pp-test">
                  <div class="pp-test-head">
                    <Button variant="secondary" size="sm" disabled={!cfg.postProcessEnabled || ppTest.running} onclick={testCleanup}>
                      {ppTest.running ? 'Testing…' : 'Test'}
                    </Button>
                    <span class="pp-test-note">Runs a sample sentence through your settings (saves first).</span>
                  </div>
                  {#if ppTest.error}
                    <div class="pp-result err">{ppTest.error}</div>
                  {:else if ppTest.result}
                    <div class="pp-result">{ppTest.result}</div>
                  {/if}
                </div>
              {/snippet}
            </Row>
          </Group>

          {#if cfg.postProcessEnabled}
            <Group title="Smart routing">
              <Row
                label="Only clean up in apps with a rule"
                hint="When on, dictation is injected raw everywhere except the apps you list below. When off, the cleanup above applies everywhere and rules just override it per app."
              >
                <Toggle
                  checked={cfg.routingScope === 'selected_apps_only'}
                  onchange={(v) => (cfg.routingScope = v ? 'selected_apps_only' : 'all_apps')}
                />
              </Row>
              <Row>
                {#snippet children()}
                  <div class="routes">
                    <div class="routes-sub">Profiles</div>
                    <p class="note">
                      Reusable cleanup styles — e.g. terse for chat, formal for email,
                      code-aware for your editor. Edit one and every app using it updates.
                    </p>
                    {#if cfg.cleanupProfiles.length > 0}
                      {#each cfg.cleanupProfiles as prof (prof.id)}
                        <div class="route">
                          <div class="route-head">
                            <input class="route-label" placeholder="Profile name" bind:value={prof.name} />
                            <span class="route-proc"></span>
                            <button class="rm" title="Delete profile" aria-label="Delete profile" onclick={() => removeProfile(prof.id)}>×</button>
                          </div>
                          <Textarea bind:value={prof.prompt} rows={3} />
                        </div>
                      {/each}
                    {:else}
                      <div class="empty">No profiles yet — add one, then assign it to an app below.</div>
                    {/if}
                    <div class="route-add">
                      <select class="route-pick" onchange={(e) => { addProfileFromPreset(e.currentTarget.value); e.currentTarget.value = ''; }}>
                        <option value="">New from preset…</option>
                        {#each PP_PRESETS.filter((p) => p.value !== 'custom') as p}
                          <option value={p.value}>{p.label}</option>
                        {/each}
                      </select>
                      <button class="add" onclick={() => addProfile()}>+ Blank profile</button>
                    </div>
                  </div>
                {/snippet}
              </Row>
              <Row>
                {#snippet children()}
                  <div class="routes">
                    <div class="routes-sub">App rules</div>
                    <p class="note">
                      Assign a profile to each app. Yap matches the app you were focused
                      on when you started dictating.
                    </p>
                    {#if cfg.appRoutes.length > 0}
                      {#each cfg.appRoutes as route, i (i)}
                        <div class="route-rule">
                          <input class="route-label" placeholder="App name" bind:value={route.label} />
                          <span class="route-proc">{route.app}</span>
                          <select class="route-pick grow" bind:value={route.profileId}>
                            <option value="">Default cleanup</option>
                            {#each cfg.cleanupProfiles as p (p.id)}
                              <option value={p.id}>{p.name}</option>
                            {/each}
                          </select>
                          <button class="rm" title="Remove rule" aria-label="Remove rule" onclick={() => removeRoute(i)}>×</button>
                        </div>
                      {/each}
                    {:else}
                      <div class="empty">No per-app rules yet.</div>
                    {/if}
                    <div class="route-add">
                      {#if recentApps.length > 0}
                        <select class="route-pick" bind:value={newRouteApp}>
                          <option value="">Recent apps…</option>
                          {#each recentApps as app}
                            <option value={app}>{app}</option>
                          {/each}
                        </select>
                      {/if}
                      <input
                        class="route-input"
                        placeholder="or type an app, e.g. slack.exe"
                        bind:value={newRouteApp}
                      />
                      <button class="add" disabled={!newRouteApp.trim()} onclick={() => addRoute()}>+ Add rule</button>
                    </div>
                  </div>
                {/snippet}
              </Row>
            </Group>

            <Group title="Usage today">
              <Row>
                {#snippet children()}
                  {#if cfg.ppProvider === 'local'}
                    <p class="usage-note">Running locally — no usage limits.</p>
                  {:else if cfg.ppProvider === 'groq'}
                    {@const tPct = pctOf(usage.tokens, usage.tokenCap)}
                    {@const rPct = pctOf(usage.requests, usage.requestCap)}
                    <div class="usage">
                      <div class="usage-bar">
                        <div class="usage-row">
                          <span class="usage-label">Tokens today</span>
                          <span class="usage-stat" style="color:{usageColor(tPct)}">
                            {tPct}%&nbsp;·&nbsp;{fmtK(usage.tokens)} / {fmtK(usage.tokenCap)}
                          </span>
                        </div>
                        <div class="track">
                          <div class="value" style="width:{tPct}%;background:{usageColor(tPct)}"></div>
                        </div>
                      </div>
                      <div class="usage-bar">
                        <div class="usage-row">
                          <span class="usage-label">Requests today</span>
                          <span class="usage-stat" style="color:{usageColor(rPct)}">
                            {rPct}%&nbsp;·&nbsp;{usage.requests} / {usage.requestCap}
                          </span>
                        </div>
                        <div class="track">
                          <div class="value" style="width:{rPct}%;background:{usageColor(rPct)}"></div>
                        </div>
                      </div>
                      <p class="usage-caption">Resets at midnight UTC.</p>
                      <p class="usage-fine">Token cap is the free-tier estimate; request count is exact from Groq.</p>
                    </div>
                  {:else}
                    <div class="usage">
                      <div class="usage-raw">
                        <span class="usage-label">Tokens today</span>
                        <span class="usage-stat">{fmtK(usage.tokens)}</span>
                      </div>
                      <div class="usage-raw">
                        <span class="usage-label">Requests today</span>
                        <span class="usage-stat">{usage.requests}</span>
                      </div>
                      <p class="usage-caption">Counts Yap's own cleanup calls. Resets at midnight UTC.</p>
                    </div>
                  {/if}
                {/snippet}
              </Row>
            </Group>
          {/if}

          <Group title="Privacy">
            <Row>
              {#snippet children()}
                <p class="pp-privacy">
                  Cleanup sends your transcript to the chosen endpoint. Use a Local
                  provider (Ollama / LM Studio) to keep everything on-device.
                </p>
              {/snippet}
            </Row>
          </Group>

        {:else if section === 'history'}
          <Group title="Stats">
            {#snippet children()}
              {#if stats}
                <div class="stats-grid">
                  <div class="stat-card">
                    <div class="stat-num">{stats.today?.words ?? 0}</div>
                    <div class="stat-lbl">Words today</div>
                  </div>
                  <div class="stat-card">
                    <div class="stat-num">{stats.totalWords ?? 0}</div>
                    <div class="stat-lbl">Words all-time</div>
                  </div>
                  <div class="stat-card">
                    <div class="stat-num">{fmtMinutes(stats.timeSavedMinutes)}</div>
                    <div class="stat-lbl">Time saved vs typing</div>
                  </div>
                  <div class="stat-card">
                    <div class="stat-num">{stats.streakDays ?? 0}🔥</div>
                    <div class="stat-lbl">Day streak</div>
                  </div>
                </div>
                {#if stats.activity?.length}
                  {@const maxW = Math.max(1, ...stats.activity.map((d) => d.words))}
                  <div class="activity" title="Words per day, last 30 days">
                    {#each stats.activity as d}
                      <span
                        class="abar"
                        class:empty={!d.words}
                        style="height:{Math.max(4, Math.round((d.words / maxW) * 100))}%"
                      ></span>
                    {/each}
                  </div>
                  <div class="activity-cap">Last 30 days · {stats.totalTranscriptions ?? 0} dictations total</div>
                {/if}
              {:else}
                <p class="hist-empty">No stats yet — dictate something to get started.</p>
              {/if}
            {/snippet}
          </Group>

          <Group title="History">
            <Row>
              <Toggle
                bind:checked={cfg.historyEnabled}
                label="Keep local history"
                hint="Stored only on this machine. Powers the stats above."
              />
            </Row>
            <Row>
              {#snippet children()}
                <div class="hist-list">
                  {#if historyItems.length}
                    {#each historyItems as item}
                      <div class="hist-row">
                        <div class="hist-text">{item.text}</div>
                        <div class="hist-meta">
                          {fmtWhen(item.ts)}{item.app ? ` · ${item.app}` : ''}{item.words
                            ? ` · ${item.words}w`
                            : ''}
                        </div>
                      </div>
                    {/each}
                  {:else}
                    <p class="hist-empty">Nothing recorded yet.</p>
                  {/if}
                </div>
              {/snippet}
            </Row>
            <Row>
              {#snippet children()}
                <Button variant="secondary" size="sm" onclick={clearHistory}>Clear history</Button>
              {/snippet}
            </Row>
          </Group>

        {:else if section === 'advanced'}
          <Group title="Output">
            <Row>
              <Toggle bind:checked={cfg.appendTrailingSpace} label="Append trailing space" hint="Add a space after each transcription" />
            </Row>
            <Row>
              <Toggle bind:checked={cfg.autoSubmit} label="Auto-submit (press Enter)" hint="Press Enter after pasting the text" />
            </Row>
            {#if cfg.autoSubmit}
              <Row label="Auto-submit key" hint="Which key to press after pasting">
                <Select bind:value={cfg.autoSubmitKey} options={AUTO_SUBMIT_KEYS} />
              </Row>
            {/if}
            <Row>
              <Toggle bind:checked={cfg.restoreClipboard} label="Restore clipboard after paste" hint="Put your previous clipboard contents back" />
            </Row>
          </Group>

          <Group title="System">
            <Row>
              <Toggle bind:checked={cfg.showTrayIcon} label="Show tray icon" hint="System-tray icon for Settings / Quit" />
            </Row>
            <Row>
              <Toggle bind:checked={cfg.autostart} label="Start on login" hint="Launch Yap when you sign in" onchange={onAutostart} />
            </Row>
          </Group>

          <Group title="Dictation dictionary">
            <Row>
              {#snippet children()}
                <div class="dict">
                  <p class="note">
                    Fix words Yap mishears (e.g. “Power to Keep” → “Parakeet”).
                    Case-insensitive; applied to every transcription.
                  </p>
                  {#if cfg.dictionary.length > 0}
                    <div class="dict-head">
                      <span>Heard</span><span></span><span>Replace with</span><span></span>
                    </div>
                    {#each cfg.dictionary as entry, i (i)}
                      <div class="dict-row">
                        <input placeholder="Power to Keep" bind:value={entry.from} />
                        <span class="arrow">→</span>
                        <input placeholder="Parakeet" bind:value={entry.to} />
                        <button class="rm" title="Remove" aria-label="Remove" onclick={() => removeEntry(i)}>×</button>
                      </div>
                    {/each}
                  {:else}
                    <div class="empty">No corrections yet.</div>
                  {/if}
                  <button class="add" onclick={addEntry}>+ Add correction</button>
                </div>
              {/snippet}
            </Row>
          </Group>

        {:else if section === 'about'}
          <Group title="About Yap">
            <Row>
              {#snippet children()}
                <div class="about">
                  <div class="abrand">
                    <img class="ablogo" src={yapIcon} alt="" aria-hidden="true" />
                    <div>
                      <div class="aname">Yap <span class="ver">{APP_VERSION}</span></div>
                      <div class="atag">A tiny local voice-dictation pill.</div>
                    </div>
                  </div>
                  <p class="aline">
                    Press your hotkey, speak, press again — Yap transcribes locally
                    with Whisper and types the text into whatever window is focused.
                  </p>
                  <p class="aprivacy">🔒 Everything runs locally. Your voice never leaves your machine.</p>
                  <p class="adir">Config &amp; models live in <code>%APPDATA%/yap/</code>.</p>
                  <a class="alink" href="https://github.com/nayballs/Yap" target="_blank" rel="noreferrer">GitHub →</a>
                </div>
              {/snippet}
            </Row>
          </Group>

          <Group title="Updates">
            <Row>
              {#snippet children()}
                <div class="upd">
                  <div class="upd-status">
                    {#if update.status === 'checking'}
                      <span class="muted">Checking for updates…</span>
                    {:else if update.status === 'available'}
                      <span class="upd-avail">Update available{update.version ? ` — v${update.version}` : ''}</span>
                      <button class="upd-btn" onclick={installUpdate}>Download &amp; install</button>
                    {:else if update.status === 'installing'}
                      <span class="muted">
                        {update.progress > 0 && update.progress < 100
                          ? `Downloading… ${update.progress}%`
                          : update.progress === 100
                            ? 'Installing…'
                            : 'Preparing…'}
                      </span>
                    {:else if update.status === 'uptodate'}
                      <span class="muted">You’re up to date ✓</span>
                    {:else if update.status === 'unsupported'}
                      <span class="muted">
                        Portable installs update manually —
                        <a class="alink" href="https://github.com/nayballs/Yap/releases/latest" target="_blank" rel="noreferrer">get the latest release</a>.
                      </span>
                    {:else if update.status === 'error'}
                      <span class="muted">Couldn’t check for updates right now.</span>
                    {:else}
                      <button class="upd-btn ghost" onclick={() => checkForUpdate(true)}>Check for updates</button>
                    {/if}
                  </div>
                  {#if update.status === 'installing' && update.progress > 0 && update.progress < 100}
                    <div class="upd-bar"><div class="upd-fill" style={`width:${update.progress}%`}></div></div>
                  {/if}
                </div>
              {/snippet}
            </Row>
            <Row>
              <Toggle
                bind:checked={cfg.updateChecksEnabled}
                label="Check for updates automatically"
                hint="Look for a newer Yap on launch"
              />
            </Row>
          </Group>
        {/if}
      </div>

      <StatusBar {saved} oncheckupdates={onCheckUpdates} />
    {:else}
      <p class="loading">Loading…</p>
    {/if}
  </main>
</div>

<style>
  :global(body) {
    background: #0f1117;
  }
  .shell {
    display: flex;
    min-height: 100vh;
    background: #0f1117;
    color: #e5e7eb;
    font-size: 13px;
  }

  .sidebar {
    flex: 0 0 158px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 12px;
    border-right: 1px solid rgba(255, 255, 255, 0.08);
    background: #0c0e14;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 4px 8px 4px;
  }
  .brandlogo {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    object-fit: contain;
    flex: 0 0 auto;
  }
  .brandname {
    font-size: 15px;
    font-weight: 600;
    color: #e5e7eb;
    letter-spacing: 0.01em;
  }
  .brand-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.08);
    margin: 10px 4px 8px;
  }
  .navitem {
    display: flex;
    align-items: center;
    gap: 9px;
    text-align: left;
    background: none;
    border: none;
    color: #9ca3af;
    opacity: 0.9;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    transition:
      background 0.15s ease,
      color 0.15s ease,
      opacity 0.15s ease;
  }
  .navicon {
    display: inline-flex;
    flex: 0 0 auto;
  }
  .navicon :global(svg) {
    width: 18px;
    height: 18px;
  }
  .navlabel {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .navitem:hover {
    color: #e5e7eb;
    opacity: 1;
    background: rgba(255, 255, 255, 0.05);
  }
  .navitem.active {
    color: #fff;
    opacity: 1;
    background: #3b82f6;
  }

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
    padding: 22px 24px 10px;
  }

  .key {
    min-width: 130px;
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid #2a2f3a;
    background: #181b22;
    color: #e5e7eb;
    cursor: pointer;
    font-size: 13px;
  }
  .key.recording {
    border-color: #3b82f6;
    color: #93c5fd;
  }

  .mm-wrap {
    width: 100%;
  }

  /* dictionary */
  .dict {
    width: 100%;
  }
  .note {
    color: #6b7280;
    font-size: 11px;
    margin: 0 0 10px;
    line-height: 1.5;
  }
  .dict-head,
  .dict-row {
    display: grid;
    grid-template-columns: 1fr 14px 1fr 22px;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
  }
  .dict-head {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: #6b7280;
  }
  .dict-row input {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 5px;
    color: #e5e7eb;
    padding: 6px 8px;
    font-size: 13px;
    width: 100%;
    box-sizing: border-box;
  }
  .dict-row input:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .arrow {
    color: #6b7280;
    text-align: center;
  }
  .rm {
    background: none;
    border: none;
    color: #6b7280;
    cursor: pointer;
    font-size: 17px;
    line-height: 1;
  }
  .rm:hover {
    color: #ef4444;
  }
  .empty {
    color: #6b7280;
    font-size: 12px;
    padding: 4px 0 8px;
  }
  .add {
    margin-top: 8px;
    background: none;
    border: 1px dashed #2a2f3a;
    color: #9ca3af;
    border-radius: 6px;
    padding: 6px 10px;
    cursor: pointer;
    font-size: 12px;
  }
  .add:hover {
    color: #e5e7eb;
    border-color: #3b82f6;
  }
  .add:disabled {
    opacity: 0.5;
    cursor: default;
    border-color: #2a2f3a;
    color: #6b7280;
  }

  /* Smart routing (profiles + per-app rules) */
  .routes {
    width: 100%;
  }
  .routes-sub {
    color: #e5e7eb;
    font-size: 13px;
    font-weight: 600;
    margin-bottom: 4px;
  }
  .route-rule {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .route-rule .route-proc {
    flex: 0 1 auto;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .route-pick.grow {
    flex: 1 1 auto;
    min-width: 0;
  }
  .route {
    border: 1px solid #2a2f3a;
    border-radius: 8px;
    padding: 10px;
    margin-bottom: 10px;
    background: #15181e;
  }
  .route-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .route-label {
    flex: 0 0 auto;
    width: 140px;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 5px 8px;
    font: inherit;
    font-size: 13px;
  }
  .route-label:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .route-proc {
    flex: 1 1 auto;
    color: #6b7280;
    font-size: 12px;
    font-family: ui-monospace, monospace;
  }
  .route-add {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .route-add .add {
    margin-top: 0;
  }
  .route-pick,
  .route-input {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 6px 8px;
    font: inherit;
    font-size: 13px;
  }
  .route-input {
    flex: 1 1 160px;
    min-width: 140px;
  }
  .route-pick:focus,
  .route-input:focus {
    outline: none;
    border-color: #3b82f6;
  }

  /* AI cleanup */
  .pp-field {
    width: 260px;
    max-width: 260px;
  }
  .pp-prompt {
    width: 100%;
  }
  .pp-label {
    color: #e5e7eb;
  }
  .pp-sub {
    color: #6b7280;
    font-size: 11px;
    margin: 1px 0 8px;
  }
  .pp-test {
    width: 100%;
  }
  .pp-test-head {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .pp-test-note {
    color: #6b7280;
    font-size: 11px;
  }
  .pp-result {
    margin-top: 10px;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    padding: 8px 10px;
    color: #e5e7eb;
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre-wrap;
  }
  .pp-result.err {
    border-color: rgba(239, 68, 68, 0.4);
    color: #fca5a5;
  }
  .pp-privacy {
    width: 100%;
    margin: 0;
    color: #9ca3af;
    font-size: 12.5px;
    line-height: 1.6;
  }

  /* History / stats dashboard */
  .stats-grid {
    width: 100%;
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 10px;
  }
  .stat-card {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 8px;
    padding: 12px 14px;
  }
  .stat-num {
    font-size: 20px;
    font-weight: 700;
    color: #f1f3f7;
  }
  .stat-lbl {
    margin-top: 2px;
    font-size: 12px;
    color: #9ca3af;
  }
  .activity {
    width: 100%;
    margin-top: 14px;
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 56px;
  }
  .activity .abar {
    flex: 1 1 0;
    min-height: 4px;
    background: #3b82f6;
    border-radius: 2px;
  }
  .activity .abar.empty {
    background: #2a2f3a;
  }
  .activity-cap {
    margin-top: 6px;
    font-size: 11.5px;
    color: #9ca3af;
  }
  .hist-list {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 320px;
    overflow-y: auto;
  }
  .hist-row {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    padding: 8px 10px;
  }
  .hist-text {
    color: #e5e7eb;
    font-size: 13px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .hist-meta {
    margin-top: 3px;
    font-size: 11px;
    color: #6b7280;
  }
  .hist-empty {
    margin: 0;
    color: #9ca3af;
    font-size: 12.5px;
  }

  /* Usage meter (Pulse-style daily bars) */
  .usage {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .usage-bar {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .usage-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .usage-label {
    color: #e5e7eb;
    font-size: 12.5px;
  }
  .usage-stat {
    color: #9ca3af;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .usage .track {
    width: 100%;
    height: 6px;
    background: #181b22;
    border-radius: 999px;
    overflow: hidden;
  }
  .usage .value {
    height: 100%;
    border-radius: 999px;
    transition: width 0.4s ease, background 0.4s ease;
  }
  .usage-raw {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .usage-caption {
    margin: 2px 0 0;
    color: #6b7280;
    font-size: 11px;
  }
  .usage-fine {
    margin: 0;
    color: #6b7280;
    font-size: 11px;
    line-height: 1.5;
  }
  .usage-note {
    width: 100%;
    margin: 0;
    color: #9ca3af;
    font-size: 12.5px;
  }

  /* about */
  .about {
    width: 100%;
  }
  .abrand {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .ablogo {
    width: 44px;
    height: 44px;
    border-radius: 10px;
    object-fit: contain;
    flex: 0 0 auto;
  }
  .aname {
    font-size: 15px;
    font-weight: 600;
  }
  .ver {
    color: #6b7280;
    font-weight: 400;
    font-size: 12px;
    margin-left: 4px;
  }
  .atag {
    color: #9ca3af;
    font-size: 12px;
    margin-top: 2px;
  }
  .aline {
    color: #9ca3af;
    font-size: 12.5px;
    line-height: 1.6;
    margin: 14px 0 0;
  }
  .aprivacy {
    color: #93c5fd;
    font-size: 12.5px;
    margin: 12px 0 0;
  }
  .adir {
    color: #6b7280;
    font-size: 12px;
    margin: 10px 0 0;
  }
  .adir code {
    color: #9ca3af;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 4px;
    padding: 1px 5px;
  }
  .alink {
    display: inline-block;
    margin-top: 14px;
    color: #60a5fa;
    text-decoration: none;
    font-size: 12.5px;
  }
  .alink:hover {
    text-decoration: underline;
  }

  /* updates */
  .upd {
    width: 100%;
  }
  .upd-status {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 12.5px;
  }
  .upd .muted {
    color: #9ca3af;
  }
  .upd-avail {
    color: #93c5fd;
    font-weight: 600;
  }
  .upd-btn {
    background: #3b82f6;
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
    font-size: 12.5px;
  }
  .upd-btn:hover {
    background: #2563eb;
  }
  .upd-btn.ghost {
    background: #181b22;
    color: #e5e7eb;
    border: 1px solid #2a2f3a;
  }
  .upd-btn.ghost:hover {
    background: #1f2330;
    border-color: #3b82f6;
  }
  .upd-bar {
    margin-top: 10px;
    height: 6px;
    border-radius: 3px;
    background: #2a2f3a;
    overflow: hidden;
  }
  .upd-fill {
    height: 100%;
    background: #3b82f6;
    transition: width 0.15s ease;
  }

  .loading {
    color: #6b7280;
    padding: 20px;
  }
</style>
