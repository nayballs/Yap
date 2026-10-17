<script>
  // Dictionary — promoted from Settings → Advanced to a first-class sidebar
  // surface (OpenWhispr's DictionaryView). Edits the correction dictionary
  // with its own load/patch-save cycle; after each save it broadcasts
  // 'yap-dictionary-changed' so the always-mounted Settings component updates
  // its own cfg copy instead of clobbering ours on its next auto-save.
  //
  // Two kinds of entry share the one list (config.rs DictionaryEntry):
  // corrections (heard → replace with) and snippets (`snippet: true` — a
  // phrase you say → the text Yap types, which may span lines;
  // config::apply_snippets). They're shown as separate sections.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import Toggle from './ui/Toggle.svelte';

  let dict = $state([]);
  let fuzzy = $state(true);
  let loaded = $state(false);
  let saveTimer = null;
  let priming = true;

  const correctionCount = $derived(dict.filter((e) => !e.snippet).length);
  const snippetCount = $derived(dict.filter((e) => e.snippet).length);

  onMount(async () => {
    try {
      const cfg = await invoke('get_config');
      dict = Array.isArray(cfg?.dictionary) ? cfg.dictionary : [];
      fuzzy = cfg?.dictionaryFuzzy ?? true;
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
        .map((e) => ({
          from: (e.from || '').trim(),
          // A snippet's text keeps its inner line breaks; only the ends trim.
          to: (e.to || '').trim(),
          fuzzy: e.fuzzy ?? true,
          snippet: !!e.snippet,
        }))
        .filter((e) => e.from && e.to);
      await invoke('save_config', { cfg: { ...fresh, dictionary: cleaned, dictionaryFuzzy: fuzzy } });
      // The always-mounted Settings component keeps its own full-cfg copy and
      // auto-saves it wholesale — the event must carry EVERY dictionary field
      // we own (entries + fuzzy), or Settings' next save reverts the missing one.
      window.dispatchEvent(
        new CustomEvent('yap-dictionary-changed', { detail: { entries: cleaned, fuzzy } })
      );
    } catch {
      /* best-effort */
    }
  }

  function setFuzzy(on) {
    fuzzy = on;
    persist();
  }

  async function addEntry(snippet) {
    dict = [...dict, { from: '', to: '', fuzzy: true, snippet }];
    await tick();
    document.querySelector(snippet ? '.snip-row:last-of-type input' : '.dict-row:last-of-type input')?.focus();
  }
  function removeEntry(i) {
    dict = dict.filter((_, j) => j !== i);
  }
  // Grow the text box with its text (2–8 lines).
  function snippetRows(text) {
    return Math.min(8, Math.max(2, (text || '').split('\n').length));
  }
</script>

<div class="wrap">
  <div class="inner">
    <div class="page-h">
      <h1>Dictionary</h1>
      <p>
        Corrections fix words Yap mishears. Snippets type out text you use a lot when you say a
        short phrase.
      </p>
    </div>

    <section aria-labelledby="dict-corrections">
      <div class="sec-h">
        <h2 id="dict-corrections">Corrections</h2>
        <p>
          “Power to Keep” → “Parakeet”. Case-insensitive, applied to every transcription — and given
          to the AI cleanup model as spelling context.
        </p>
      </div>
      <div class="dictcard">
        {#if correctionCount > 0}
          <div class="dict-head">
            <span>Heard</span><span></span><span>Replace with</span><span></span><span></span>
          </div>
          <div class="rows">
            {#each dict as entry, i (i)}
              {#if !entry.snippet}
                <div class="dict-row">
                  <input placeholder="Power to Keep" aria-label="Heard" bind:value={entry.from} />
                  <span class="arrow">→</span>
                  <input placeholder="Parakeet" aria-label="Replace with" bind:value={entry.to} />
                  <button
                    class="fz"
                    class:on={entry.fuzzy ?? true}
                    class:dim={!fuzzy}
                    title={(entry.fuzzy ?? true)
                      ? 'Near-misses are corrected too — click for exact matches only (e.g. so “Jason” stops becoming “JSON”)'
                      : 'Exact matches only — click to also catch near-misses'}
                    aria-label="Toggle near-miss matching for this correction"
                    onclick={() => (entry.fuzzy = !(entry.fuzzy ?? true))}
                  >≈</button>
                  <button class="rm" title="Remove" aria-label="Remove" onclick={() => removeEntry(i)}>×</button>
                </div>
              {/if}
            {/each}
          </div>
        {:else if loaded}
          <div class="empty">No corrections yet.</div>
        {/if}
        <button class="add" onclick={() => addEntry(false)}>+ Add correction</button>
      </div>

      <div class="fuzzycard">
        <Toggle
          label="Catch near-misses"
          desc="Also fix words that ALMOST match a correction — “jaison” still becomes “JSON”. Uses spelling + sound-alike matching (same engine as Handy). The ≈ button on a row exempts that correction — handy when a near-miss is a real word (“Jason”). With a Whisper model the dictionary is fed straight into the recognizer instead, the words you've used most lately first."
          checked={fuzzy}
          onchange={setFuzzy}
        />
      </div>
    </section>

    <section aria-labelledby="dict-snippets">
      <div class="sec-h">
        <h2 id="dict-snippets">Snippets</h2>
        <p>
          Say the phrase while you dictate and Yap types the text in its place — sign-offs,
          addresses, links. Capitals and punctuation around the phrase don't matter. Dictations
          only; meeting notes and uploads keep what was said.
        </p>
      </div>
      <div class="dictcard snippets">
        {#if snippetCount > 0}
          <div class="snip-head">
            <span>You say</span><span></span><span>Yap types</span><span></span>
          </div>
          <div class="rows">
            {#each dict as entry, i (i)}
              {#if entry.snippet}
                <div class="snip-row">
                  <div class="say">
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                      <rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5" />
                      <path d="M3 7.5a5 5 0 0 0 10 0M8 12.5v2.5" />
                    </svg>
                    <input placeholder="my sign-off" aria-label="Phrase you say" bind:value={entry.from} />
                  </div>
                  <span class="arrow">→</span>
                  <textarea
                    placeholder={'Speak soon,\nYour name'}
                    aria-label="Text Yap types"
                    rows={snippetRows(entry.to)}
                    bind:value={entry.to}
                  ></textarea>
                  <button class="rm" title="Remove" aria-label="Remove snippet" onclick={() => removeEntry(i)}>×</button>
                </div>
              {/if}
            {/each}
          </div>
        {:else if loaded}
          <div class="empty">
            No snippets yet. Try one like “my sign-off” → “Speak soon,” with your name on the next line.
          </div>
        {/if}
        <button class="add" onclick={() => addEntry(true)}>+ Add snippet</button>
      </div>
    </section>
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
  .page-h p,
  .sec-h p {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  section + section {
    margin-top: 28px;
  }
  .sec-h {
    margin: 0 0 10px;
  }
  .sec-h h2 {
    margin: 0 0 2px;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.005em;
  }

  .dictcard {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 14px 16px;
  }
  .fuzzycard {
    margin-top: 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 12px 16px;
  }
  .dict-head,
  .dict-row {
    display: grid;
    grid-template-columns: 1fr 14px 1fr 24px 22px;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
  }
  .dict-head,
  .snip-head {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--yap-muted-55);
  }
  .dict-row input,
  .snip-row input,
  .snip-row textarea {
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    color: var(--yap-fg);
    padding: 6px 8px;
    font: inherit;
    font-size: 13px;
    width: 100%;
    box-sizing: border-box;
  }
  .dict-row input:focus,
  .snip-row input:focus,
  .snip-row textarea:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .arrow {
    color: var(--yap-muted-55);
    text-align: center;
  }

  /* Snippets: the phrase reads as something said (a mic and a quote-ish
     pill), the text as a small document — so they never look like a
     correction row. */
  .snip-head,
  .snip-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 14px minmax(0, 1.6fr) 22px;
    gap: 6px;
    padding: 3px 0;
  }
  .snip-head {
    align-items: center;
  }
  .snip-row {
    align-items: start;
    padding: 5px 0;
  }
  .snip-row + .snip-row {
    border-top: 1px dashed var(--yap-border-subtle);
  }
  .snip-row .arrow,
  .snip-row .rm {
    margin-top: 6px;
  }
  .say {
    position: relative;
  }
  .say svg {
    position: absolute;
    left: 9px;
    top: 50%;
    width: 13px;
    height: 13px;
    transform: translateY(-50%);
    fill: none;
    stroke: var(--yap-muted);
    stroke-width: 1.4;
    stroke-linecap: round;
    pointer-events: none;
  }
  .snip-row .say input {
    border-radius: var(--yap-r-full);
    padding-left: 27px;
    background: var(--yap-s3);
  }
  .snip-row textarea {
    resize: vertical;
    line-height: 1.45;
    min-height: 34px;
  }

  .fz {
    background: none;
    border: 1px solid var(--yap-border);
    border-radius: 50%;
    width: 22px;
    height: 22px;
    padding: 0;
    color: var(--yap-muted-55);
    cursor: pointer;
    font-size: 13px;
    line-height: 1;
  }
  .fz.on {
    color: var(--yap-primary);
    border-color: var(--yap-primary);
  }
  .fz.dim {
    opacity: 0.35;
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
