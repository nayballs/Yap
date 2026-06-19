<script>
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';

  let cfg = $state(null);
  let devices = $state([]);
  let recording = $state(false);
  let saved = $state(false);

  const MODEL_OPTIONS = [
    { value: 'large-v3', label: 'Large v3 (best accuracy, ~3.1GB)' },
    { value: 'large-v3-turbo', label: 'Large v3 Turbo (fast, ~1.5GB)' },
    { value: 'small', label: 'Small (~0.5GB)' },
    { value: 'base', label: 'Base (~150MB)' },
  ];

  onMount(async () => {
    cfg = await invoke('get_config');
    try {
      devices = await invoke('list_audio_devices');
    } catch {
      devices = [];
    }
  });

  onDestroy(stopRecord);

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

<main>
  {#if cfg}
    <h1>Blip Settings</h1>

    <section>
      <h2>Activation</h2>
      <div class="row">
        <div class="ldesc">
          <span class="label">Hotkey</span>
          <span class="hint">Press to start, press again to stop &amp; type</span>
        </div>
        <button class="key" class:recording onclick={startRecord}>
          {recording ? 'Press a key…' : formatHotkey(cfg.hotkey)}
        </button>
      </div>
      <label class="row toggle">
        <div class="ldesc">
          <span class="label">Sound cue</span>
          <span class="hint">Chime when recording starts/stops</span>
        </div>
        <input type="checkbox" bind:checked={cfg.soundEnabled} />
      </label>
    </section>

    <section>
      <h2>Speech Recognition</h2>
      <div class="row">
        <span class="label">Model</span>
        <select bind:value={cfg.modelSize}>
          {#each MODEL_OPTIONS as o}
            <option value={o.value}>{o.label}</option>
          {/each}
        </select>
      </div>
      <label class="row toggle">
        <div class="ldesc">
          <span class="label">GPU acceleration (CUDA)</span>
          <span class="hint">Faster transcription on NVIDIA GPUs</span>
        </div>
        <input type="checkbox" bind:checked={cfg.useGpu} />
      </label>
      <div class="row">
        <span class="label">Microphone</span>
        <select bind:value={cfg.inputDevice}>
          <option value={null}>System default</option>
          {#each devices as d}
            <option value={d}>{d}</option>
          {/each}
        </select>
      </div>
      <p class="note">Model, GPU &amp; microphone changes apply after restarting Blip.</p>
    </section>

    <section>
      <h2>Dictation Dictionary</h2>
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
    </section>

    <div class="actions">
      <button class="save" onclick={save}>{saved ? 'Saved ✓' : 'Save'}</button>
    </div>
  {:else}
    <p class="loading">Loading…</p>
  {/if}
</main>

<style>
  :global(body) {
    background: #0f1117;
  }
  main {
    box-sizing: border-box;
    min-height: 100vh;
    background: #0f1117;
    color: #e5e7eb;
    padding: 18px 20px 28px;
    font-size: 13px;
  }
  h1 {
    font-size: 16px;
    margin: 0 0 16px;
  }
  h2 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: #60a5fa;
    margin: 0 0 8px;
  }
  section {
    margin-bottom: 22px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 7px 0;
  }
  .toggle {
    cursor: pointer;
  }
  .ldesc {
    display: flex;
    flex-direction: column;
  }
  .label {
    color: #e5e7eb;
  }
  .hint {
    color: #6b7280;
    font-size: 11px;
  }
  .note {
    color: #6b7280;
    font-size: 11px;
    margin: 4px 0 10px;
    line-height: 1.5;
  }
  .key {
    min-width: 110px;
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
  select {
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 6px 8px;
    font-size: 13px;
    max-width: 260px;
  }
  input[type='checkbox'] {
    width: 18px;
    height: 18px;
    accent-color: #3b82f6;
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

  .actions {
    position: sticky;
    bottom: 0;
    padding-top: 12px;
    background: linear-gradient(transparent, #0f1117 30%);
  }
  .save {
    width: 100%;
    padding: 10px;
    border: none;
    border-radius: 8px;
    background: #3b82f6;
    color: #fff;
    font-size: 14px;
    cursor: pointer;
  }
  .save:hover {
    background: #2563eb;
  }
  .loading {
    color: #6b7280;
  }
</style>
