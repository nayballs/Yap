<script>
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  // Diagnostics -> the backend's rolling yap.log (webview consoles are
  // invisible in normal runs; this made the event-delivery bug debuggable).
  const flog = (m) => invoke('frontend_log', { msg: '[onboarding] ' + m }).catch(() => {});
  import yapIcon from '../assets/yap-icon.png';
  import { MODELS } from './models.js';
  import ModelCard from './ModelCard.svelte';
  import brandAnthropic from '../assets/brands/anthropic.svg';
  import brandOpenai from '../assets/brands/openai.svg';
  import brandMeta from '../assets/brands/meta.svg';
  import brandGoogle from '../assets/brands/google.svg';
  import brandQwen from '../assets/brands/qwen.svg';
  import brandOllama from '../assets/brands/ollama.svg';
  import brandOpenrouter from '../assets/brands/openrouter.svg';

  // Brand badges (superwhisper-style coloured squares). `icon` = white SVG
  // (simple-icons, CC0); brands without one fall back to a letter badge.
  const BRANDS = {
    anthropic: { icon: brandAnthropic, color: '#D97757' },
    openai: { icon: brandOpenai, color: '#10A37F' },
    meta: { icon: brandMeta, color: '#0866FF' },
    google: { icon: brandGoogle, color: '#4285F4' },
    qwen: { icon: brandQwen, color: '#615CED' },
    ollama: { icon: brandOllama, color: '#374151' },
    openrouter: { icon: brandOpenrouter, color: '#6467F2' },
    microsoft: { letter: 'P', color: '#0078D4' }, // Phi
    groq: { letter: 'G', color: '#F55036' },
    custom: { letter: '⚙', color: '#4b5563' },
  };
  // Which brand a curated local model belongs to, by id prefix.
  function brandForModel(id) {
    if (id.startsWith('llama')) return BRANDS.meta;
    if (id.startsWith('qwen')) return BRANDS.qwen;
    if (id.startsWith('gemma')) return BRANDS.google;
    if (id.startsWith('phi')) return BRANDS.microsoft;
    return BRANDS.custom;
  }

  // ---- Stepped flow (superwhisper-style guided onboarding) ----
  // 0 model → 1 mic check → 2 AI cleanup (the wedge) → 3 tray → 4 try it
  const STEPS = ['Model', 'Microphone', 'AI cleanup', 'Tray', 'Try it'];
  let step = $state(0);

  // Full config (loaded once; mutated by steps; persisted via save_config).
  let cfg = $state(null);

  // ---- Step 0: model picker (unchanged mechanics) ----
  let installed = $state([]); // model ids already on disk
  let active = $state(null); // currently active model id
  let busyId = $state(null); // model id being downloaded / switched to
  let percent = $state(0); // download progress for busyId
  let error = $state('');

  // The recommended default (Parakeet V3) — powers the one-click quick-start so a
  // brand-new user can go from install → dictating without hunting through the list.
  const recommended = MODELS.find((m) => m.recommended) || MODELS[0];

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

  async function applyMic() {
    if (!cfg) return;
    try {
      cfgPatch.inputDevice = cfg.inputDevice || null; // set via bind:value
      await invoke('set_input_device', { device: cfg.inputDevice || null });
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

  // ---- Step 2: AI cleanup (Yap's differentiator, offered up front) ----
  let llm = $state({ installed: false, running: false, model: 'Qwen2.5 1.5B Instruct', curated: [] });
  let llmInstalling = $state(false);
  let llmProgress = $state({ stage: '', percent: 0 });
  let llmError = $state('');
  let cleanupEnabled = $state(false); // reflects what we set up this session
  let enabledSummary = $state('');

  // Private vs cloud, superwhisper-style two-card choice.
  let cleanupMode = $state('local'); // 'local' | 'cloud'
  let localPick = $state('qwen2.5-1.5b'); // curated id (the recommended default)

  // Cloud (bring-your-own-key) fields. Base URLs/default models mirror the
  // Settings provider presets.
  // Anthropic works through its OpenAI-compatible /v1/chat/completions layer,
  // so it plugs into Yap's existing cleanup client like any other provider.
  const CLOUD_PROVIDERS = [
    { value: 'groq', label: 'Groq (free tier)', brand: BRANDS.groq, baseUrl: 'https://api.groq.com/openai/v1', model: 'llama-3.1-8b-instant' },
    { value: 'anthropic', label: 'Anthropic (Claude)', brand: BRANDS.anthropic, baseUrl: 'https://api.anthropic.com/v1', model: 'claude-haiku-4-5' },
    { value: 'openai', label: 'OpenAI', brand: BRANDS.openai, baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
    { value: 'openrouter', label: 'OpenRouter', brand: BRANDS.openrouter, baseUrl: 'https://openrouter.ai/api/v1', model: 'meta-llama/llama-3.1-8b-instruct' },
    { value: 'local', label: 'My own server (Ollama · LM Studio)', brand: BRANDS.ollama, baseUrl: 'http://localhost:11434/v1', model: 'llama3.1' },
    { value: 'custom', label: 'Custom endpoint', brand: BRANDS.custom, baseUrl: '', model: '' },
  ];
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

  function fmtSize(mb) {
    return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
  }

  async function refreshLlm() {
    try {
      llm = await invoke('local_llm_status');
      cleanupEnabled = !!cfg?.postProcessEnabled;
      if (cleanupEnabled) {
        enabledSummary =
          cfg.ppProvider === 'ondevice'
            ? `${llm.model} — running privately on this machine`
            : `cloud cleanup via ${cfg.ppProvider}`;
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
          flog(`poll: filling box with "${t.slice(0, 40)}"`);
          shownTs = e.ts;
          showTranscript(t);
        }
      } catch (err) {
        flog('poll: get_history FAILED: ' + err);
      }
    }, 400);
    return () => clearInterval(timer);
  });

  function formatHotkey(spec) {
    if (!spec) return 'None';
    if (spec.startsWith('mouse:')) return `Mouse ${spec.slice(6)}`;
    const m = spec.match(/^kb:(\d+)$/);
    return m ? vkeyName(+m[1]) : spec;
  }
  function vkeyName(v) {
    if (v >= 112 && v <= 123) return `F${v - 111}`;
    if ((v >= 48 && v <= 57) || (v >= 65 && v <= 90)) return String.fromCharCode(v);
    const named = { 32: 'Space', 13: 'Enter', 9: 'Tab', 8: 'Backspace', 192: '`' };
    return named[v] || `Key ${v}`;
  }

  // Mini hotkey recorder (same mechanics as Settings): pause the live binding,
  // capture one keypress, re-apply + persist.
  function startKeyRecord() {
    if (recordingKey) return;
    recordingKey = true;
    invoke('configure_hotkey', { spec: '' }).catch(() => {});
    window.addEventListener('keydown', onRecordKey, true);
  }
  function stopKeyRecord() {
    recordingKey = false;
    window.removeEventListener('keydown', onRecordKey, true);
    if (cfg) {
      cfgPatch.hotkey = cfg.hotkey; // set directly by onRecordKey
      invoke('configure_hotkey', { spec: cfg.hotkey }).catch(() => {});
      persistCfg();
    }
  }
  function onRecordKey(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return stopKeyRecord();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;
    cfg.hotkey = `kb:${e.keyCode}`;
    stopKeyRecord();
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

  function next() {
    if (!cfg) return; // config still loading — steps 1+ bind into it
    if (step < STEPS.length - 1) step += 1;
  }
  function back() {
    if (step > 0) step -= 1;
  }
  function finish() {
    invoke('set_mic_test', { on: false }).catch(() => {});
    invoke('close_onboarding');
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
      (eventId) => () =>
        I.invoke('plugin:event|unlisten', { event, eventId }).catch(() => {}),
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
        const amp = Math.min(1, (e.payload || 0) * 4); // same gain feel as the pill
        bars = [...bars.slice(1), amp];
        if (amp > 0.06) micHeard = true;
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
        flog(`event yap-transcript (step=${step}): "${String(e && e.payload).slice(0, 40)}"`);
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

  // In-window hotkey fallback. When a WebView2 window of OUR OWN app has
  // focus, the global low-level keyboard hook never sees the hotkey (log-
  // proven 2026-07-05: focused presses leave zero hook events — WebView2
  // appears to front-run the hook chain). The key DOES reach this page as a
  // normal keydown though, so catch it here and drive the pipeline directly.
  function hotkeyVkey() {
    const m = (cfg?.hotkey || '').match(/^kb:(\d+)$/);
    return m ? +m[1] : null;
  }
  function onFallbackKeyDown(e) {
    if (recordingKey || e.repeat) return; // shortcut recorder open / key repeat
    if (e.keyCode !== hotkeyVkey()) return;
    e.preventDefault();
    e.stopPropagation();
    flog('in-window hotkey fallback: keydown -> toggle_recording');
    invoke('toggle_recording').catch(() => {});
  }
  function onFallbackKeyUp(e) {
    if (recordingKey) return;
    if (e.keyCode !== hotkeyVkey()) return;
    // Push-to-talk: the release stops the recording (toggle mode ignores it —
    // in toggle mode recording only flips on keydown).
    if (cfg?.recordingMode === 'pushToTalk') {
      e.preventDefault();
      flog('in-window hotkey fallback: keyup -> toggle_recording (PTT)');
      invoke('toggle_recording').catch(() => {});
    }
  }

  onMount(() => {
    flog('mounted; visibilityState=' + document.visibilityState);
    window.addEventListener('keydown', onFallbackKeyDown, true);
    window.addEventListener('keyup', onFallbackKeyUp, true);
    refresh().then(refreshLlm);
    invoke('list_audio_devices')
      .then((d) => (devices = d || []))
      .catch(() => {});
    const onVis = () => {
      pageVisible = !document.hidden;
      flog('visibilitychange: hidden=' + document.hidden);
      // Re-shown after a hide ("Show setup guide again"): re-sync from disk so
      // this long-lived window doesn't present (or later save) stale state —
      // and re-register the event subscriptions (see registerListeners).
      if (pageVisible) {
        registerListeners();
        invoke('get_config')
          .then((fresh) => {
            cfg = { ...fresh, ...cfgPatch };
            refreshLlm();
            refresh();
          })
          .catch(() => {});
      }
    };
    document.addEventListener('visibilitychange', onVis);
    registerListeners();
    return () => {
      unsubs.forEach((p) => p.then((u) => u && u()));
      window.removeEventListener('keydown', onFallbackKeyDown, true);
      window.removeEventListener('keyup', onFallbackKeyUp, true);
      window.removeEventListener('keydown', onRecordKey, true);
      document.removeEventListener('visibilitychange', onVis);
      invoke('set_mic_test', { on: false }).catch(() => {});
    };
  });
</script>

<main>
  <!-- Progress dots -->
  <nav class="dots" aria-label="Setup progress">
    {#each STEPS as s, i}
      <button
        class="dot"
        class:done={i < step}
        class:current={i === step}
        title={s}
        onclick={() => (i < step ? (step = i) : null)}
        aria-label={`Step ${i + 1}: ${s}`}
      ></button>
    {/each}
  </nav>

  {#if step === 0}
    <header>
      <img class="logo" src={yapIcon} alt="" aria-hidden="true" />
      <h1>Welcome to Yap</h1>
      <p class="sub">
        Pick the speech model that turns your voice into text. It runs
        <strong>locally on your GPU</strong> — your voice never leaves this machine.
        <strong>{recommended.name}</strong> is the fast, accurate default.
      </p>
    </header>

    <div class="cards">
      {#each MODELS as m (m.id)}
        <ModelCard model={m} status={statusOf(m.id)} {percent} onclick={choose} />
      {/each}
    </div>
  {:else if step === 1}
    <header>
      <h1>Let's check your microphone</h1>
      <p class="sub">Say something — the bars should react. Silence? Pick a different device.</p>
    </header>

    <div class="mic-box">
      <select
        class="mic-pick"
        bind:value={cfg.inputDevice}
        onchange={applyMic}
        aria-label="Microphone"
      >
        <option value={null}>System default</option>
        {#each devices as d}
          <option value={d}>{d}</option>
        {/each}
      </select>

      <div class="meter" class:live={micHeard} aria-hidden="true">
        {#each bars as b}
          <span style="height:{Math.max(6, b * 100)}%"></span>
        {/each}
      </div>
      <p class="mic-status">
        {#if micHeard}✓ Hearing you loud and clear{:else}Waiting for sound…{/if}
      </p>
    </div>
  {:else if step === 2}
    <header>
      <h1>Make it sound polished</h1>
      <p class="sub">
        Raw dictation is full of "um"s and false starts. Yap's <strong>AI cleanup</strong>
        strips filler, fixes punctuation, and resolves "no wait, I meant…" —
        <strong>entirely on your PC</strong>. No account, no API key, no cloud.
      </p>
    </header>

    <div class="cleanup-box">
      {#if cleanupEnabled}
        <div class="cleanup-done">✓ AI cleanup is on — <strong>{enabledSummary}</strong>.</div>
        <button class="skip" onclick={() => (cleanupEnabled = false)}>
          Choose a different model or provider →
        </button>
        <p class="fine">Or change it any time in Settings → AI Cleanup.</p>
      {:else if llmInstalling}
        <div class="cleanup-progress">
          <span>
            Downloading the {llmProgress.stage === 'runtime' ? 'engine' : 'model'}…
            {llmProgress.percent || 0}%
          </span>
          <div class="bar"><span style="width:{llmProgress.percent || 0}%"></span></div>
        </div>
      {:else}
        <div class="cleanup-demo" aria-hidden="true">
          <div class="demo-raw">"so um, I went to the shop and uh, no wait — I mean I got milk"</div>
          <div class="demo-arrow">↓</div>
          <div class="demo-clean">"I went to the shop and got milk."</div>
        </div>

        <div class="mode-cards">
          <button
            class="mode-card"
            class:sel={cleanupMode === 'local'}
            onclick={() => (cleanupMode = 'local')}
          >
            <span class="mode-title">🔒 Private</span>
            <span class="mode-sub">runs on your PC · no account</span>
            <span class="mode-badge">Recommended</span>
          </button>
          <button
            class="mode-card"
            class:sel={cleanupMode === 'cloud'}
            onclick={() => (cleanupMode = 'cloud')}
          >
            <span class="mode-title">☁ Cloud</span>
            <span class="mode-sub">bring your own key</span>
          </button>
        </div>

        {#if cleanupMode === 'local'}
          <div class="llm-list">
            {#each llm.curated || [] as m (m.id)}
              {@const b = brandForModel(m.id)}
              <label class="llm-row" class:sel={localPick === m.id}>
                <input type="radio" bind:group={localPick} value={m.id} name="llm" />
                <span class="brand" style="background:{b.color}">
                  {#if b.icon}<img src={b.icon} alt="" />{:else}{b.letter}{/if}
                </span>
                <span class="llm-main">
                  <span class="llm-name">{m.display}</span>
                  <span class="llm-blurb">{m.blurb}</span>
                </span>
                <span class="llm-size">{m.installed ? '✓ installed' : fmtSize(m.sizeMb)}</span>
              </label>
            {/each}
          </div>
          {@const pick = (llm.curated || []).find((m) => m.id === localPick)}
          <button class="start wide" onclick={enableLocalCleanup} disabled={!pick}>
            {#if pick && !pick.installed}
              Download {pick.display} ({fmtSize(pick.sizeMb)}) & enable
            {:else if pick}
              Enable {pick.display}
            {:else}
              Enable private AI cleanup
            {/if}
          </button>
          <p class="fine">All of these run fully offline via llamafile. Prefer your own?
            Drop any GGUF into the models folder later (Settings → AI Cleanup).</p>
        {:else}
          <div class="cloud-form">
            <div class="prov-grid">
              {#each CLOUD_PROVIDERS as p (p.value)}
                <button
                  class="prov-chip"
                  class:sel={cloudProvider === p.value}
                  onclick={() => {
                    cloudProvider = p.value;
                    onCloudProviderChange();
                  }}
                >
                  <span class="brand" style="background:{p.brand.color}">
                    {#if p.brand.icon}<img src={p.brand.icon} alt="" />{:else}{p.brand.letter}{/if}
                  </span>
                  <span class="prov-label">{p.label}</span>
                </button>
              {/each}
            </div>
            {#if cloudProvider === 'custom'}
              <input class="cloud-inp" placeholder="Base URL (https://…/v1)" bind:value={cloudBaseUrl} />
            {/if}
            <input class="cloud-inp" type="password" placeholder="API key" bind:value={cloudKey} />
            <input class="cloud-inp" placeholder="Model (e.g. llama-3.1-8b-instant)" bind:value={cloudModel} />
          </div>
          <button class="start wide" onclick={enableCloudCleanup}>Enable cloud cleanup</button>
          <p class="fine">Your key is stored locally. Transcripts (never audio) are sent to the
            provider you chose. Groq's free tier is plenty for dictation.</p>
        {/if}
      {/if}
      {#if llmError}<p class="error">{llmError}</p>{/if}
    </div>
  {:else if step === 3}
    <header>
      <h1>Yap lives in your tray</h1>
      <p class="sub">
        There's no main window to keep open — Yap waits in the corner of your taskbar.
        A floating overlay appears whenever you're dictating.
      </p>
    </header>

    <div class="tray-mock" aria-hidden="true">
      <div class="tray-arrow">⬇</div>
      <div class="tray-bar">
        <span class="tray-caret">^</span>
        <span class="tray-icon yap"><img class="tray-yap-img" src={yapIcon} alt="" /></span>
        <span class="tray-icon">☁</span>
        <span class="tray-lang">ENG</span>
        <span class="tray-icon">🔊</span>
        <span class="tray-clock">10:42</span>
      </div>
    </div>
    <ul class="tray-tips">
      <li><strong>Left-click</strong> the icon → open Settings.</li>
      <li><strong>Right-click</strong> → switch model, cancel a recording, check for updates.</li>
      <li>The dot changes colour while recording &amp; transcribing.</li>
    </ul>
  {:else}
    <header>
      <h1>Try it</h1>
      <p class="sub">
        Press <strong class="key">{formatHotkey(cfg?.hotkey)}</strong>, say something,
        then press <strong class="key">{formatHotkey(cfg?.hotkey)}</strong> again —
        your words appear below.
      </p>
    </header>

    <div class="try-box">
      <textarea
        class="try-area"
        class:flash={tryFlash}
        placeholder="Your words will appear here…"
        rows="5"
        readonly
        bind:value={tryText}
      ></textarea>
      <p class="try-status">
        {#if gotTranscript}
          ✓ It works! You can dictate into any app exactly like this.
        {:else if tryState === 'recording'}
          ● Listening…
        {:else if tryState === 'processing' || tryState === 'processing-slow'}
          … Transcribing
        {:else if tryState === 'needs-model'}
          ⚠ No model installed — go back to step 1.
        {:else}
          Waiting for {formatHotkey(cfg?.hotkey)}…
        {/if}
      </p>
      <button class="skip" onclick={startKeyRecord} disabled={recordingKey}>
        {#if recordingKey}Press the key you want… (Esc to cancel){:else}Change shortcut{/if}
      </button>
    </div>
  {/if}

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <footer>
    {#if step > 0}
      <button class="skip" onclick={back}>← Back</button>
    {:else}
      <button class="skip" onclick={next}>I'll choose later</button>
    {/if}

    {#if step === 0}
      <button class="start" onclick={quickStart} disabled={!!busyId || !cfg}>
        {#if busyId === recommended.id}
          Downloading {recommended.name}… {percent}%
        {:else if busyId}
          Setting up… {percent}%
        {:else if active}
          Continue →
        {:else}
          Download {recommended.name} & continue
        {/if}
      </button>
    {:else if step === 2 && !cleanupEnabled}
      <button class="start ghost" onclick={next} disabled={llmInstalling}>
        {llmInstalling ? 'Downloading…' : 'Maybe later'}
      </button>
    {:else if step === STEPS.length - 1}
      <button class="start" onclick={finish}>
        {gotTranscript ? 'Finish 🎉' : 'Finish'}
      </button>
    {:else}
      <button class="start" onclick={next} disabled={!cfg}>Continue →</button>
    {/if}
  </footer>
</main>

<style>
  :global(body) {
    background: #0f1117;
  }
  main {
    box-sizing: border-box;
    /* Hard viewport bound (NOT min-height): the page itself must never grow
       past the window, or the footer nav ends up below the fold and the model
       list silently stops being a scroll container. `.cards` scrolls instead. */
    height: 100vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: #0f1117;
    color: #e5e7eb;
    padding: 20px 28px 22px;
    font-family: system-ui, -apple-system, sans-serif;
  }

  /* Progress dots */
  .dots {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin-bottom: 18px;
  }
  .dot {
    width: 26px;
    height: 5px;
    border: none;
    border-radius: 3px;
    background: #232936;
    padding: 0;
    cursor: default;
  }
  .dot.done {
    background: #2b4a7a;
    cursor: pointer;
  }
  .dot.current {
    background: #3b82f6;
  }

  header {
    text-align: center;
    margin-bottom: 18px;
  }
  .logo {
    width: 56px;
    height: 56px;
    margin: 0 auto 10px;
    border-radius: 12px;
    object-fit: contain;
  }
  h1 {
    font-size: 21px;
    margin: 0 0 8px;
    letter-spacing: 0.01em;
  }
  .sub {
    color: #9ca3af;
    font-size: 13px;
    line-height: 1.6;
    max-width: 460px;
    margin: 0 auto;
  }
  .key {
    background: #1f2733;
    border: 1px solid #2a2f3a;
    border-radius: 5px;
    padding: 1px 7px;
    color: #e5e7eb;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    flex: 1 1 auto;
    min-height: 0;
    padding-right: 6px; /* breathing room so the scrollbar doesn't hug the cards */
  }
  /* Visible (but subtle) scrollbar so it's obvious the list scrolls. */
  .cards::-webkit-scrollbar {
    width: 8px;
  }
  .cards::-webkit-scrollbar-track {
    background: transparent;
  }
  .cards::-webkit-scrollbar-thumb {
    background: #2a2f3a;
    border-radius: 4px;
  }
  .cards::-webkit-scrollbar-thumb:hover {
    background: #3a4150;
  }

  /* Mic step */
  .mic-box {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
  }
  .mic-pick {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 7px;
    color: #e5e7eb;
    font: inherit;
    font-size: 13px;
    padding: 7px 10px;
    max-width: 320px;
  }
  .meter {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 90px;
    width: 100%;
    max-width: 420px;
  }
  .meter span {
    flex: 1;
    background: #2a2f3a;
    border-radius: 2px;
    transition: height 60ms linear;
  }
  .meter.live span {
    background: #3b82f6;
  }
  .mic-status {
    color: #9ca3af;
    font-size: 13px;
    margin: 0;
  }
  .meter.live + .mic-status {
    color: #34d399;
  }

  /* Cleanup step */
  .cleanup-box {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    gap: 14px;
    padding: 6px 2px;
  }
  .mode-cards {
    display: flex;
    gap: 10px;
    width: 100%;
    max-width: 440px;
  }
  .mode-card {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    background: #15181e;
    border: 1px solid #2a2f3a;
    border-radius: 10px;
    padding: 14px 10px 12px;
    color: #e5e7eb;
    font: inherit;
    cursor: pointer;
    transition: border-color 0.12s ease;
  }
  .mode-card.sel {
    border-color: #3b82f6;
    background: #151d2c;
  }
  .mode-title {
    font-size: 14px;
    font-weight: 600;
  }
  .mode-sub {
    font-size: 11.5px;
    color: #9ca3af;
  }
  .mode-badge {
    position: absolute;
    top: -8px;
    right: 8px;
    background: #1d4ed8;
    color: #fff;
    font-size: 10px;
    border-radius: 5px;
    padding: 1px 6px;
  }
  .llm-list {
    width: 100%;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .llm-row {
    display: flex;
    align-items: center;
    gap: 10px;
    background: #15181e;
    border: 1px solid #2a2f3a;
    border-radius: 8px;
    padding: 8px 12px;
    cursor: pointer;
  }
  .llm-row.sel {
    border-color: #3b82f6;
  }
  .llm-row input {
    accent-color: #3b82f6;
  }
  .llm-main {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .llm-name {
    font-size: 13px;
    font-weight: 500;
  }
  .llm-blurb {
    font-size: 11.5px;
    color: #6b7280;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .llm-size {
    flex: 0 0 auto;
    font-size: 11.5px;
    color: #9ca3af;
    font-variant-numeric: tabular-nums;
  }
  /* Brand badge — coloured rounded square w/ white glyph (or letter). */
  .brand {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: #fff;
    font-size: 12px;
    font-weight: 700;
  }
  .brand img {
    width: 13px;
    height: 13px;
    display: block;
  }
  .cloud-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    max-width: 440px;
  }
  .prov-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .prov-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    background: #15181e;
    border: 1px solid #2a2f3a;
    border-radius: 8px;
    padding: 8px 10px;
    color: #e5e7eb;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.12s ease;
  }
  .prov-chip.sel {
    border-color: #3b82f6;
    background: #151d2c;
  }
  .prov-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cloud-form .mic-pick {
    max-width: none;
  }
  .cloud-inp {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 7px;
    color: #e5e7eb;
    font: inherit;
    font-size: 13px;
    padding: 7px 10px;
  }
  .cloud-inp:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .cleanup-demo {
    text-align: center;
    font-size: 13px;
    line-height: 1.7;
  }
  .demo-raw {
    color: #6b7280;
    font-style: italic;
  }
  .demo-arrow {
    color: #3b82f6;
    font-size: 15px;
  }
  .demo-clean {
    color: #e5e7eb;
    font-weight: 500;
  }
  .cleanup-done {
    color: #34d399;
    font-size: 14px;
    text-align: center;
    line-height: 1.6;
  }
  .cleanup-progress {
    width: 100%;
    max-width: 380px;
    font-size: 13px;
    color: #9ca3af;
    text-align: center;
  }
  .bar {
    margin-top: 8px;
    height: 6px;
    border-radius: 3px;
    background: #1f2733;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: #3b82f6;
    transition: width 0.2s ease;
  }
  .fine {
    color: #6b7280;
    font-size: 11.5px;
    text-align: center;
    max-width: 380px;
    line-height: 1.5;
    margin: 0;
  }

  /* Tray step */
  .tray-mock {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    margin: 26px 0 18px;
  }
  .tray-arrow {
    color: #f59e0b;
    font-size: 20px;
    margin-bottom: 4px;
    margin-left: -132px;
  }
  .tray-bar {
    display: flex;
    align-items: center;
    gap: 14px;
    background: #1c2028;
    border: 1px solid #2a2f3a;
    border-radius: 10px;
    padding: 10px 18px;
    font-size: 13px;
    color: #9ca3af;
  }
  .tray-caret {
    font-weight: 700;
  }
  .tray-icon.yap {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 5px;
    background: #10131a;
    outline: 2px solid #f59e0b;
  }
  .tray-yap-img {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    object-fit: contain;
  }
  .tray-clock {
    font-variant-numeric: tabular-nums;
  }
  .tray-tips {
    color: #9ca3af;
    font-size: 13px;
    line-height: 1.9;
    max-width: 400px;
    margin: 0 auto;
    padding-left: 18px;
  }

  /* Try step */
  .try-box {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }
  .try-area {
    width: 100%;
    max-width: 460px;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 10px;
    color: #e5e7eb;
    font: inherit;
    font-size: 14px;
    line-height: 1.6;
    padding: 12px 14px;
    resize: none;
  }
  .try-area:focus {
    outline: none;
    border-color: #3b82f6;
  }
  .try-area.flash {
    border-color: #34d399;
    background: #10241c;
    transition: none;
  }
  .try-area {
    transition: border-color 0.9s ease, background 0.9s ease;
  }
  .try-status {
    color: #9ca3af;
    font-size: 13px;
    min-height: 18px;
    margin: 0;
  }

  .error {
    color: #fca5a5;
    font-size: 12px;
    text-align: center;
    margin: 12px 0 0;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 18px;
  }
  .skip {
    background: none;
    border: none;
    color: #6b7280;
    font-size: 12.5px;
    cursor: pointer;
    padding: 8px 4px;
  }
  .skip:hover:not(:disabled) {
    color: #9ca3af;
  }
  .start {
    border: none;
    border-radius: 9px;
    background: #3b82f6;
    color: #fff;
    font-size: 14px;
    font-weight: 500;
    padding: 11px 20px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .start:hover:not(:disabled) {
    background: #2563eb;
  }
  .start:disabled {
    background: #1f2733;
    color: #6b7280;
    cursor: default;
  }
  .start.ghost {
    background: #1f2733;
    color: #9ca3af;
  }
  .start.ghost:hover:not(:disabled) {
    background: #262f3d;
  }
  .start.wide {
    width: 100%;
    max-width: 380px;
  }
</style>
