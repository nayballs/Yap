// Onboarding's state across its steps (Onboarding.svelte makes it once: the
// window lives as long as Yap, hidden between showings) — the config and
// the fields this window changed, which step is up, and what each step
// sets up: the speech model, the microphone, AI cleanup (private or a
// cloud provider), the shortcut and the "try it" box — and its event
// subscriptions, made through the raw internals bridge (see `rawListen`).
import { invoke } from '@tauri-apps/api/core';
import { MODELS } from '../core/speechModels.js';
import { CLOUD_PROVIDERS } from './cleanupProviders.js';

// Diagnostics -> the backend's rolling yap.log (webview consoles are
// invisible in normal runs; this made the event-delivery bug debuggable).
export const flog = (m) => invoke('frontend_log', { msg: '[onboarding] ' + m }).catch(() => {});

// ---- Stepped flow (superwhisper-style guided onboarding) ----
// 0 model → 1 mic check → 2 AI cleanup (the wedge) → 3 tray → 4 try it
export const STEPS = ['Model', 'Microphone', 'AI cleanup', 'Tray', 'Try it'];

// The recommended default (Parakeet V3) — powers the one-click quick-start so a
// brand-new user can go from install → dictating without hunting through the list.
export const recommended = MODELS.find((m) => m.recommended) || MODELS[0];

// A first run shows two speech models, the rest behind "More models", as
// the AI-cleanup step does (fourteen cards were a wall to scroll past):
// the recommended one and the best other pick for the language this PC
// speaks — Parakeet V2 (English only, the most accurate there) or Whisper
// Large v3 Turbo (beyond Parakeet's 25 European languages). A model
// already on this PC, or being set up, further down keeps the list open.
const SECOND = (navigator.language || 'en').toLowerCase().startsWith('en') ? 'parakeet-tdt-0.6b-v2' : 'turbo';
const SHORT_MODELS = [recommended.id, SECOND];
export const orderedModels = [
  ...SHORT_MODELS.map((id) => MODELS.find((m) => m.id === id)).filter(Boolean),
  ...MODELS.filter((m) => !SHORT_MODELS.includes(m.id)),
];

// A first run shows two local cleanup models (the one for this PC and one
// more), the rest behind "More models": twelve choices buried the Download
// button below the fold. A pick further down keeps the whole list open.
const SHORT_LIST = 2;

/** Onboarding's state. Call it while the window's page is made (its effects belong to it). */
export function createSetupState() {
  let step = $state(0);

  // Full config (loaded once; mutated by steps; persisted via save_config).
  let cfg = $state(null);

  // ---- Step 0: model picker (unchanged mechanics) ----
  let installed = $state([]); // model ids already on disk
  let active = $state(null); // currently active model id
  let busyId = $state(null); // model id being downloaded / switched to
  let percent = $state(0); // download progress for busyId
  let error = $state('');

  let allModels = $state(false);
  const shownModels = $derived(
    allModels ||
      orderedModels.some(
        (m) => !SHORT_MODELS.includes(m.id) && (installed.includes(m.id) || busyId === m.id || active === m.id)
      )
      ? orderedModels
      : orderedModels.slice(0, SHORT_MODELS.length)
  );

  function statusOf(id) {
    if (busyId === id) return installed.includes(id) ? 'switching' : 'downloading';
    if (active === id) return 'active';
    if (installed.includes(id)) return 'available';
    return 'downloadable';
  }

  async function refresh() {
    try {
      installed = await invoke('installed_models');
      const c = await invoke('get_config');
      if (c) {
        // Keep this window's intentional changes on top of the fresh config.
        cfg = { ...c, ...cfgPatch };
        if (installed.includes(c.modelSize)) active = c.modelSize;
      }
    } catch (e) {
      // best-effort
    }
  }

  // Pick a model: download if needed, then make it the active model.
  async function choose(model) {
    if (busyId) return;
    if (active === model.id) return;
    error = '';
    busyId = model.id;
    percent = 0;
    try {
      if (!installed.includes(model.id)) {
        await invoke('download_model_size', { modelSize: model.id });
        await refresh();
      }
      await invoke('set_active_model', { modelSize: model.id });
      active = model.id;
      await refresh();
    } catch (e) {
      error = `Couldn't set up ${model.name}: ${e}`;
    } finally {
      busyId = null;
      percent = 0;
    }
  }

  // One-click path: download + activate the recommended model, then advance.
  async function quickStart() {
    if (busyId) return;
    if (active) {
      next();
      return;
    }
    await choose(recommended);
    if (active === recommended.id) next();
  }

  // ---- Step 1: mic check ----
  let devices = $state([]);
  let bars = $state(Array(36).fill(0)); // scrolling level meter
  let micHeard = $state(false); // any signal above the floor yet?

  // A microphone picked goes first in the order (the rest stay after it);
  // "System default" ('') empties it. Saved, the mic switches to it live
  // (dictation/mic.rs follows the order; the mic check holds it open).
  async function applyMic(id) {
    if (!cfg) return;
    try {
      const m = devices.find((d) => d.id === id);
      const rest = (cfg.micOrder || []).filter((c) => c.id !== id && (!m || c.name !== m.name));
      cfgPatch.micOrder = m ? [{ id: m.id, name: m.name }, ...rest] : [];
      cfg.micOrder = cfgPatch.micOrder;
      await persistCfg();
      bars = Array(36).fill(0);
      micHeard = false;
    } catch (e) {
      error = `Couldn't switch microphone: ${e}`;
    }
  }

  // Mic-test mode follows the step: levels stream (yap-amp) only on step 1.
  // The window hides (not closes) on X, so also gate on page visibility —
  // otherwise a hidden onboarding parked on the mic step would keep the
  // level meter streaming forever.
  let pageVisible = $state(!document.hidden);
  $effect(() => {
    invoke('set_mic_test', { on: step === 1 && pageVisible }).catch(() => {});
  });

  // What suits this PC (ai/hardware.rs `hardware_advice`, from yap.exe bench's
  // numbers): where speech runs before it's timed here, and whether the
  // private cleanup or a cloud provider is the quicker one — with both
  // times said.
  let advice = $state(null);

  // ---- Step 2: AI cleanup (Yap's differentiator, offered up front) ----
  let llm = $state({ installed: false, running: false, model: 'Qwen2.5 1.5B Instruct', curated: [] });
  let llmInstalling = $state(false);
  let llmProgress = $state({ stage: '', percent: 0 });
  let llmError = $state('');
  let cleanupEnabled = $state(false); // reflects what we set up this session
  let enabledSummary = $state('');

  // Private vs cloud, superwhisper-style two-card choice. The recommendation
  // (2026-10-24, ai/cleanup_styles.rs): on this PC when its graphics card runs
  // the recommended model (ai/gpu.rs) or a model is already here; else a cloud
  // provider (Groq's free plan); the person picks either, or "Maybe later"
  // leaves cleanup off. Without a card the cloud is recommended even where
  // ai/hardware.rs finds the 1.5B quick enough: it follows the styles by app
  // less well (73 % against the 4B's and the cloud's 100 % on the style
  // evals); both times are said under the cards (`advice`), and a small PC's
  // cloud card says "Quicker on this PC".
  let cleanupMode = $state('local'); // 'local' | 'cloud'
  let modePicked = false;
  const recommendLocal = $derived(!!llm.gpuCapable || (llm.curated || []).some((m) => m.installed));
  // Curated id: the model recommended for this PC (local_llm_status's
  // `recommendedId`: Qwen3.5 4B where the graphics card can run it, else
  // Qwen2.5 1.5B) until the person picks another.
  let localPick = $state('qwen2.5-1.5b');
  let localPicked = false;
  // The recommended model first, then the light one that's quick without a
  // graphics card (Qwen2.5 1.5B), then the rest in their order.
  const localModels = $derived.by(() => {
    const rank = (m) => (m.recommended ? 0 : m.id === 'qwen2.5-1.5b' ? 1 : 2);
    return [...(llm.curated || [])].sort((a, b) => rank(a) - rank(b));
  });
  let allLocal = $state(false);
  const shownLocal = $derived(
    allLocal || localModels.findIndex((m) => m.id === localPick) >= SHORT_LIST ? localModels : localModels.slice(0, SHORT_LIST)
  );

  let cloudProvider = $state('groq');
  let cloudBaseUrl = $state(CLOUD_PROVIDERS[0].baseUrl);
  let cloudKey = $state('');
  let cloudModel = $state(CLOUD_PROVIDERS[0].model);

  function onCloudProviderChange() {
    const p = CLOUD_PROVIDERS.find((p) => p.value === cloudProvider);
    if (!p) return;
    cloudBaseUrl = p.baseUrl;
    cloudModel = p.model;
  }

  async function refreshLlm() {
    try {
      llm = await invoke('local_llm_status');
      if (!localPicked && llm.recommendedId) localPick = llm.recommendedId;
      if (!modePicked) cleanupMode = recommendLocal ? 'local' : 'cloud';
      cleanupEnabled = !!cfg?.postProcessEnabled;
      if (cleanupEnabled) {
        enabledSummary =
          cfg.ppProvider === 'ondevice' ? `${llm.model} — running privately on this machine` : `cloud cleanup via ${cfg.ppProvider}`;
      }
    } catch {
      // stub build / best-effort
    }
  }

  async function enableLocalCleanup() {
    if (llmInstalling) return;
    llmError = '';
    llmInstalling = true;
    llmProgress = { stage: '', percent: 0 };
    try {
      // Downloads runtime + the picked model (skips already-present files) and
      // returns the model's GGUF filename for pp_local_model.
      const filename = await invoke('local_llm_install', { model: localPick });
      patchCfg({ postProcessEnabled: true, ppProvider: 'ondevice', ppLocalModel: filename });
      await persistCfg(); // save_config also autostarts the sidecar
      cleanupEnabled = true;
      await refreshLlm();
    } catch (e) {
      llmError = `${e}`;
    } finally {
      llmInstalling = false;
    }
  }

  async function enableCloudCleanup() {
    llmError = '';
    if (!cloudModel.trim() || !cloudBaseUrl.trim()) {
      llmError = 'Pick a provider and model first.';
      return;
    }
    if (!cloudKey.trim() && !['local', 'custom'].includes(cloudProvider)) {
      llmError = 'This provider needs an API key.';
      return;
    }
    patchCfg({
      postProcessEnabled: true,
      ppProvider: cloudProvider,
      ppBaseUrl: cloudBaseUrl.trim(),
      ppApiKey: cloudKey.trim(),
      ppModel: cloudModel.trim(),
    });
    await persistCfg();
    cleanupEnabled = true;
    enabledSummary = `${cloudModel.trim()} via ${cloudProvider}`;
  }

  // ---- Step 4: try it ----
  let tryState = $state('idle'); // mirrors yap-state while on the try step
  let tryText = $state(''); // filled from the yap-transcript EVENT (see below)
  let gotTranscript = $state(false);
  let tryFlash = $state(false); // green flash when the box (re)fills
  let recordingKey = $state(false); // mini hotkey recorder active
  let flashTimer = null;
  // Timestamp (epoch s) of the content currently shown — an in-flight poll
  // response must never overwrite a FRESHER event fill (seen in the log:
  // a stale poll replaced a just-arrived transcript for ~800ms).
  let shownTs = 0;

  // Show a transcript in the box — with an unmissable flash. A silent 0.7s
  // delayed text replacement reads as "nothing happened" (field-tested).
  function showTranscript(t) {
    tryText = t;
    gotTranscript = !!t;
    tryFlash = false;
    clearTimeout(flashTimer);
    requestAnimationFrame(() => (tryFlash = true));
    flashTimer = setTimeout(() => (tryFlash = false), 1200);
  }

  // Reset the demo box each time the user lands on the try step — and, while
  // on it, ALSO poll history for the newest dictation. The box normally fills
  // from the yap-transcript event, but this webview has been observed to go
  // deaf to events after a hide/re-show cycle (root cause under
  // investigation); history is backend truth and always works.
  $effect(() => {
    if (step !== STEPS.length - 1) return;
    tryText = '';
    gotTranscript = false;
    const enteredAt = Math.floor(Date.now() / 1000) - 2; // small clock slack
    flog(`try-step entered, enteredAt=${enteredAt}`);
    shownTs = 0;
    const timer = setInterval(async () => {
      try {
        const h = await invoke('get_history', { limit: 1 });
        const e = Array.isArray(h) ? h[0] : null;
        if (e && e.ts >= enteredAt && e.ts > shownTs && (e.text || '').trim()) {
          const t = e.text.trim();
          flog(`poll: filling box (${t.length} chars)`); // never the text: it's in the log file
          shownTs = e.ts;
          showTranscript(t);
        }
      } catch (err) {
        flog('poll: get_history FAILED: ' + err);
      }
    }, 400);
    return () => clearInterval(timer);
  });

  // The shortcut picker: Settings' own (combos, modifier chords, mouse
  // buttons, the Copilot key), checked against the other shortcuts. The live
  // binding is paused while a new one is picked, then applied and saved.
  function onShortcutCapturing(on) {
    recordingKey = on;
    if (!cfg) return;
    invoke('configure_hotkey', { spec: on ? '' : cfg.hotkey }).catch(() => {});
    if (!on) {
      cfgPatch.hotkey = cfg.hotkey;
      persistCfg();
    }
  }

  // ---- Shared plumbing ----
  // Only the fields onboarding has INTENTIONALLY changed. persistCfg merges
  // these onto a freshly-loaded config, so this window's (possibly stale)
  // snapshot can never clobber changes made meanwhile in Settings — this
  // window survives hide/re-show for the app's whole lifetime.
  let cfgPatch = {};
  function patchCfg(fields) {
    Object.assign(cfgPatch, fields);
    Object.assign(cfg, fields);
  }

  async function persistCfg() {
    if (!cfg) return;
    try {
      const fresh = await invoke('get_config');
      cfg = { ...fresh, ...cfgPatch };
      await invoke('save_config', { cfg });
    } catch (e) {
      error = `Couldn't save settings: ${e}`;
    }
  }

  /** A config read afresh (the window shown again): this window's changes on top. */
  function adoptConfig(fresh) {
    cfg = { ...fresh, ...cfgPatch };
  }

  function next() {
    if (!cfg) return; // config still loading — steps 1+ bind into it
    if (step < STEPS.length - 1) step += 1;
  }
  function back() {
    if (step > 0) step -= 1;
  }
  // Done: Yap's main window comes up on Home, so the setup doesn't end on
  // an empty screen (Yap otherwise lives in the tray and the Yap bar).
  async function finish() {
    invoke('set_mic_test', { on: false }).catch(() => {});
    await invoke('close_onboarding').catch(() => {});
    invoke('open_settings').catch(() => {});
  }

  // Tauri event subscription — via the RAW internals bridge, not the bundled
  // @tauri-apps/api `listen`. Empirically (CDP debugging, 2026-07-05): on this
  // window, listeners registered through the bundled module never fire, while
  // an identical registration through window.__TAURI_INTERNALS__ on the same
  // page receives every event. Root cause unresolved (suspected duplicate api
  // module instance under the Vite dev server); this shape is the one PROVEN
  // to work.
  function rawListen(event, cb) {
    const I = window.__TAURI_INTERNALS__;
    if (!I) return Promise.resolve(() => {});
    return I.invoke('plugin:event|listen', {
      event,
      target: { kind: 'Any' },
      handler: I.transformCallback((e) => cb(e)),
    }).then(
      (eventId) => () => I.invoke('plugin:event|unlisten', { event, eventId }).catch(() => {}),
      () => () => {}
    );
  }

  // Registered at mount AND re-registered whenever the window is re-shown:
  // this window has been observed to go deaf across its hide/re-show
  // lifecycle, so we tear down and re-listen on every show.
  let unsubs = [];
  async function registerListeners() {
    const old = unsubs;
    unsubs = [];
    for (const p of old) {
      try {
        const u = await p;
        if (u) u();
      } catch {
        // best-effort teardown
      }
    }
    flog('registerListeners: registering 5 raw listeners');
    unsubs = [
      rawListen('stt-download-progress', (e) => {
        if (e.payload && e.payload.modelSize === busyId) percent = e.payload.percent;
      }),
      rawListen('yap-amp', (e) => {
        // Shaped in Rust as for the Yap bar (dictation/meter.rs): decibels
        // over the mic's own noise, so the room's hiss is 0 and a whisper
        // about a third. Above 0.3 (a whisper, ~-46 dBFS on a quiet mic)
        // counts as hearing you.
        const amp = Math.max(0, Math.min(1, Number(e.payload) || 0));
        bars = [...bars.slice(1), amp];
        if (amp > 0.3) micHeard = true;
      }),
      rawListen('local-llm-download-progress', (e) => {
        if (e.payload) llmProgress = e.payload;
      }),
      rawListen('yap-state', (e) => {
        tryState = e.payload || 'idle';
      }),
      // The try-box fills from this EVENT, not from the OS-level paste: pasting
      // into our own webview races the clipboard restore (WebView2 handles
      // Ctrl+V asynchronously), so the injected text can vanish. The event is
      // authoritative and timing-proof; the textarea is readonly so the paste
      // (when it does win the race) can't double-write. (Plus the history poll
      // above as belt-and-braces.)
      rawListen('yap-transcript', (e) => {
        flog(`event yap-transcript (step=${step}): ${String((e && e.payload) || '').length} chars`);
        if (step === STEPS.length - 1) {
          const t = (e.payload || '').trim();
          if (t) {
            // Always re-show + flash: dictating the SAME phrase twice must
            // still visibly react, or it reads as "nothing happened".
            shownTs = Math.floor(Date.now() / 1000);
            showTranscript(t);
          }
        }
      }),
    ];
  }
  /** Every subscription let go (the page is going). */
  function unlistenAll() {
    unsubs.forEach((p) => p.then((u) => u && u()));
  }

  return {
    get step() { return step; },
    set step(v) { step = v; },
    get cfg() { return cfg; },
    get installed() { return installed; },
    get active() { return active; },
    get busyId() { return busyId; },
    get percent() { return percent; },
    get error() { return error; },
    get allModels() { return allModels; },
    set allModels(v) { allModels = v; },
    get shownModels() { return shownModels; },
    get devices() { return devices; },
    set devices(v) { devices = v; },
    get bars() { return bars; },
    get micHeard() { return micHeard; },
    get pageVisible() { return pageVisible; },
    set pageVisible(v) { pageVisible = v; },
    get advice() { return advice; },
    set advice(v) { advice = v; },
    get llm() { return llm; },
    get llmInstalling() { return llmInstalling; },
    get llmProgress() { return llmProgress; },
    get llmError() { return llmError; },
    get cleanupEnabled() { return cleanupEnabled; },
    set cleanupEnabled(v) { cleanupEnabled = v; },
    get enabledSummary() { return enabledSummary; },
    get cleanupMode() { return cleanupMode; },
    set cleanupMode(v) { cleanupMode = v; },
    get modePicked() { return modePicked; },
    set modePicked(v) { modePicked = v; },
    get recommendLocal() { return recommendLocal; },
    get localPick() { return localPick; },
    set localPick(v) { localPick = v; },
    get localPicked() { return localPicked; },
    set localPicked(v) { localPicked = v; },
    get localModels() { return localModels; },
    get allLocal() { return allLocal; },
    set allLocal(v) { allLocal = v; },
    get shownLocal() { return shownLocal; },
    get cloudProvider() { return cloudProvider; },
    set cloudProvider(v) { cloudProvider = v; },
    get cloudBaseUrl() { return cloudBaseUrl; },
    set cloudBaseUrl(v) { cloudBaseUrl = v; },
    get cloudKey() { return cloudKey; },
    set cloudKey(v) { cloudKey = v; },
    get cloudModel() { return cloudModel; },
    set cloudModel(v) { cloudModel = v; },
    get tryState() { return tryState; },
    get tryText() { return tryText; },
    set tryText(v) { tryText = v; },
    get gotTranscript() { return gotTranscript; },
    get tryFlash() { return tryFlash; },
    get recordingKey() { return recordingKey; },
    statusOf,
    refresh,
    choose,
    quickStart,
    applyMic,
    onCloudProviderChange,
    refreshLlm,
    enableLocalCleanup,
    enableCloudCleanup,
    onShortcutCapturing,
    adoptConfig,
    next,
    back,
    finish,
    registerListeners,
    unlistenAll,
  };
}
