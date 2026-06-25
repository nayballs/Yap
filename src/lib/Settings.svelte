<script>
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Select from './ui/Select.svelte';
  import Slider from './ui/Slider.svelte';
  import Button from './ui/Button.svelte';
  import ModelManager from './ModelManager.svelte';

  let cfg = $state(null);
  let devices = $state([]);
  let outputs = $state([]); // audio output device names (for the chime)
  let recording = $state(false); // hotkey-recorder active
  let saved = $state(false);
  let section = $state('general'); // general | models | advanced | about
  // Language/translate capability of the active model (drives Models section).
  let langInfo = $state({ supportsLanguage: false, supportsTranslate: false, languages: [] });

  const SECTIONS = [
    { id: 'general', label: 'General' },
    { id: 'models', label: 'Models' },
    { id: 'advanced', label: 'Advanced' },
    { id: 'about', label: 'About' },
  ];

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
    inputDevice: null,
    dictionary: [],
    selectedLanguage: 'auto',
    translateToEnglish: false,
    modelUnloadTimeout: 'never',
    outputDevice: null,
    overlayPosition: 'bottom',
    autoSubmitKey: 'enter',
  };

  onMount(async () => {
    const loaded = await invoke('get_config');
    cfg = { ...FIELD_DEFAULTS, ...loaded };
    if (!Array.isArray(cfg.dictionary)) cfg.dictionary = [];
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
  });

  onDestroy(stopRecord);

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
  function startRecord() {
    if (!cfg) return;
    recording = true;
    invoke('configure_hotkey', { spec: '' }); // pause live binding while choosing
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('mousedown', onMouse, true);
  }
  function stopRecord() {
    if (!recording) return;
    recording = false;
    window.removeEventListener('keydown', onKey, true);
    window.removeEventListener('mousedown', onMouse, true);
    if (cfg) invoke('configure_hotkey', { spec: cfg.hotkey }); // apply live
  }
  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return stopRecord();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;
    cfg.hotkey = `kb:${e.keyCode}`;
    stopRecord();
  }
  function onMouse(e) {
    if (e.button === 0 || e.button === 2) return; // left/right reserved for UI
    e.preventDefault();
    e.stopPropagation();
    const map = { 1: 3, 3: 4, 4: 5 }; // browser button -> our id
    const id = map[e.button] ?? e.button + 1;
    cfg.hotkey = `mouse:${id}`;
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

  async function save() {
    const clean = {
      ...cfg,
      inputDevice: cfg.inputDevice || null,
      outputDevice: cfg.outputDevice || null,
      pillScale: Number(cfg.pillScale),
      audioFeedbackVolume: Number(cfg.audioFeedbackVolume),
      dictionary: cfg.dictionary
        .map((e) => ({ from: (e.from || '').trim(), to: (e.to || '').trim() }))
        .filter((e) => e.from),
    };
    await invoke('save_config', { cfg: clean });
    cfg = clean;
    saved = true;
    setTimeout(() => (saved = false), 1600);
  }
</script>

<div class="shell">
  <nav class="sidebar">
    <div class="brand">Blip</div>
    {#each SECTIONS as s (s.id)}
      <button class="navitem" class:active={section === s.id} onclick={() => (section = s.id)}>
        {s.label}
      </button>
    {/each}
  </nav>

  <main>
    {#if cfg}
      <div class="content">
        {#if section === 'general'}
          <Group title="Activation">
            <Row label="Hotkey" hint="Trigger dictation from any app">
              <button class="key" class:recording onclick={startRecord}>
                {recording ? 'Press a key…' : formatHotkey(cfg.hotkey)}
              </button>
            </Row>
            <Row label="Recording mode" hint="How the hotkey controls recording">
              <Select bind:value={cfg.recordingMode} options={RECORDING_MODES} />
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
              <Toggle bind:checked={cfg.useGpu} label="GPU acceleration (CUDA)" hint="Faster transcription on NVIDIA GPUs — applies after restart" />
            </Row>
            <Row label="Unload model when idle" hint="Free memory when not dictating; reloads on next use">
              <Select bind:value={cfg.modelUnloadTimeout} options={UNLOAD_TIMEOUTS} />
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
              <Toggle bind:checked={cfg.autostart} label="Start on login" hint="Launch Blip when you sign in" onchange={onAutostart} />
            </Row>
          </Group>

          <Group title="Dictation dictionary">
            <Row>
              {#snippet children()}
                <div class="dict">
                  <p class="note">
                    Fix words Blip mishears (e.g. “Power to Keep” → “Parakeet”).
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
          <Group title="About Blip">
            <Row>
              {#snippet children()}
                <div class="about">
                  <div class="abrand">
                    <div class="ablogo" aria-hidden="true"></div>
                    <div>
                      <div class="aname">Blip <span class="ver">0.1.0</span></div>
                      <div class="atag">A tiny local voice-dictation pill.</div>
                    </div>
                  </div>
                  <p class="aline">
                    Press your hotkey, speak, press again — Blip transcribes locally
                    with Whisper and types the text into whatever window is focused.
                  </p>
                  <p class="aprivacy">🔒 Everything runs locally. Your voice never leaves your machine.</p>
                  <p class="adir">Config &amp; models live in <code>%APPDATA%/blip/</code>.</p>
                  <a class="alink" href="https://github.com" target="_blank" rel="noreferrer">GitHub →</a>
                </div>
              {/snippet}
            </Row>
          </Group>
        {/if}
      </div>

      <div class="actions">
        <Button onclick={save}>{saved ? 'Saved ✓' : 'Save'}</Button>
      </div>
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
    flex: 0 0 150px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 12px;
    border-right: 1px solid #2a2f3a;
    background: #0c0e14;
  }
  .brand {
    font-size: 16px;
    font-weight: 700;
    color: #e5e7eb;
    padding: 4px 10px 14px;
    letter-spacing: 0.02em;
  }
  .navitem {
    text-align: left;
    background: none;
    border: none;
    color: #9ca3af;
    padding: 8px 10px;
    border-radius: 7px;
    cursor: pointer;
    font: inherit;
    transition:
      background 0.15s ease,
      color 0.15s ease;
  }
  .navitem:hover {
    color: #e5e7eb;
    background: rgba(255, 255, 255, 0.04);
  }
  .navitem.active {
    color: #fff;
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
    padding: 20px 22px 8px;
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
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: radial-gradient(circle at 35% 30%, #60a5fa, #2563eb);
    box-shadow: 0 0 16px rgba(59, 130, 246, 0.5);
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

  .actions {
    flex: 0 0 auto;
    padding: 12px 22px;
    border-top: 1px solid #2a2f3a;
    background: #0f1117;
  }
  .actions :global(.btn) {
    width: 100%;
    padding: 11px;
    font-size: 14px;
  }
  .loading {
    color: #6b7280;
    padding: 20px;
  }
</style>
