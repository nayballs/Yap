<script>
  // Dictionary — promoted from Settings → Advanced to a first-class sidebar
  // surface (OpenWhispr's DictionaryView). Edits the correction dictionary
  // with its own load/patch-save cycle; after each save it broadcasts
  // 'yap-dictionary-changed' so the always-mounted Settings component updates
  // its own cfg copy instead of clobbering ours on its next auto-save.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let dict = $state([]);
  let loaded = $state(false);
  let saveTimer = null;
  let priming = true;

  onMount(async () => {
    try {
      const cfg = await invoke('get_config');
      dict = Array.isArray(cfg?.dictionary) ? cfg.dictionary : [];
    } catch {
      dict = [];
    }
    loaded = true;
    // Settings can also touch the dictionary (Voice Agent name save) — track it.
    const onExternal = (e) => {
      if (!Array.isArray(e.detail)) return;
      const incoming = JSON.stringify(e.detail);
      if (incoming === JSON.stringify(dict)) return; // no change → no save churn
      priming = true;
      dict = e.detail.map((x) => ({ ...x }));
      setTimeout(() => (priming = false), 50);
    };
    window.addEventListener('yap-dictionary-external', onExternal);
    setTimeout(() => (priming = false), 0);
    return () => window.removeEventListener('yap-dictionary-external', onExternal);
  });

  // Debounced patch-save: merge onto a FRESH config so this view can never
  // clobber settings changed elsewhere in the meantime.
  $effect(() => {
    // deep-read so any row edit re-runs the effect
    JSON.stringify(dict);
    if (!loaded || priming) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(persist, 500);
    return () => clearTimeout(saveTimer);
  });

  async function persist() {
    try {
      const fresh = await invoke('get_config');
      const cleaned = dict
        .map((e) => ({ from: (e.from || '').trim(), to: (e.to || '').trim() }))
        .filter((e) => e.from && e.to);
      await invoke('save_config', { cfg: { ...fresh, dictionary: cleaned } });
      window.dispatchEvent(new CustomEvent('yap-dictionary-changed', { detail: cleaned }));
    } catch {
      /* best-effort */
    }
  }

  function addEntry() {
    dict = [...dict, { from: '', to: '' }];
  }
  function removeEntry(i) {
    dict = dict.filter((_, j) => j !== i);
  }
</script>

<div class="wrap">
  <div class="inner">
    <div class="page-h">
      <h1>Dictionary</h1>
      <p>
        Fix words Yap mishears (e.g. “Power to Keep” → “Parakeet”). Case-insensitive, applied to
        every transcription — and fed to the AI cleanup model as spelling context.
      </p>
    </div>

    <div class="dictcard">
      {#if dict.length > 0}
        <div class="dict-head">
          <span>Heard</span><span></span><span>Replace with</span><span></span>
        </div>
        {#each dict as entry, i (i)}
          <div class="dict-row">
            <input placeholder="Power to Keep" bind:value={entry.from} />
            <span class="arrow">→</span>
            <input placeholder="Parakeet" bind:value={entry.to} />
            <button class="rm" title="Remove" aria-label="Remove" onclick={() => removeEntry(i)}>×</button>
          </div>
        {/each}
      {:else if loaded}
        <div class="empty">No corrections yet.</div>
      {/if}
      <button class="add" onclick={addEntry}>+ Add correction</button>
    </div>
  </div>
</div>

<style>
  .wrap {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .inner {
    max-width: 780px;
    margin: 0 auto;
    padding: 26px 30px 40px;
  }
  .page-h {
    margin: 0 0 22px;
  }
  .page-h h1 {
    margin: 0 0 4px;
    font-size: 19px;
    letter-spacing: -0.01em;
  }
  .page-h p {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }

  .dictcard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 16px;
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
    color: var(--yap-muted-55);
  }
  .dict-row input {
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    color: var(--yap-fg);
    padding: 6px 8px;
    font-size: 13px;
    width: 100%;
    box-sizing: border-box;
  }
  .dict-row input:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .arrow {
    color: var(--yap-muted-55);
    text-align: center;
  }
  .rm {
    background: none;
    border: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    font-size: 17px;
    line-height: 1;
  }
  .rm:hover {
    color: #ef4444;
  }
  .empty {
    color: var(--yap-muted-55);
    font-size: 12px;
    padding: 4px 0 8px;
  }
  .add {
    margin-top: 8px;
    background: none;
    border: 1px dashed var(--yap-border);
    color: var(--yap-muted);
    border-radius: var(--yap-r);
    padding: 6px 12px;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .add:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
</style>
