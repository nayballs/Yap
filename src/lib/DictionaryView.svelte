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
  //
  // "Suggest words from your corrections" (`learnWords`, learn.rs): words
  // added from a suggestion are saved in Rust; Settings adopts them and
  // passes them on here as `yap-dictionary-external`.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import Toggle from './ui/Toggle.svelte';
  import { openContextMenu, menuKey, askConfirm } from './ui/contextMenu.svelte.js';

  let dict = $state([]);
  let fuzzy = $state(true);
  let learnWords = $state(true);
  let loaded = $state(false);
  let saveTimer = null;
  let priming = true;

  const correctionCount = $derived(dict.filter((e) => !e.snippet).length);
  const snippetCount = $derived(dict.filter((e) => e.snippet).length);

  // Not an async onMount: Svelte drops the cleanup an async one returns, and
  // every visit to the view left its listener behind.
  onMount(() => {
    let gone = false;
    // Settings can also touch the dictionary (Commands' name save) — track it.
    const onExternal = (e) => {
      if (!Array.isArray(e.detail)) return;
      const incoming = JSON.stringify(e.detail);
      if (incoming === JSON.stringify(dict)) return; // no change → no save churn
      priming = true;
      dict = e.detail.map((x) => ({ ...x }));
      setTimeout(() => (priming = false), 50);
    };
    (async () => {
      try {
        const cfg = await invoke('get_config');
        dict = Array.isArray(cfg?.dictionary) ? cfg.dictionary : [];
        fuzzy = cfg?.dictionaryFuzzy ?? true;
        learnWords = cfg?.learnWords ?? true;
      } catch {
        dict = [];
      }
      loaded = true;
      if (gone) return;
      window.addEventListener('yap-dictionary-external', onExternal);
      setTimeout(() => (priming = false), 0);
    })();
    return () => {
      gone = true;
      window.removeEventListener('yap-dictionary-external', onExternal);
    };
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
      await invoke('save_config', {
        cfg: { ...fresh, dictionary: cleaned, dictionaryFuzzy: fuzzy, learnWords },
      });
      // The always-mounted Settings component keeps its own full-cfg copy and
      // auto-saves it wholesale — the event must carry EVERY dictionary field
      // we own (entries, fuzzy, learnWords), or Settings' next save reverts
      // the missing one.
      window.dispatchEvent(
        new CustomEvent('yap-dictionary-changed', { detail: { entries: cleaned, fuzzy, learnWords } })
      );
    } catch {
      /* best-effort */
    }
  }

  function setFuzzy(on) {
    fuzzy = on;
    persist();
  }
  function setLearnWords(on) {
    learnWords = on;
    persist();
  }

  async function addEntry(snippet) {
    dict = [...dict, { from: '', to: '', fuzzy: true, snippet }];
    await tick();
    document.querySelector(snippet ? '.snip-row:last-of-type input' : '.dict-row:last-of-type input')?.focus();
  }
  function removeEntry(entry) {
    dict = dict.filter((x) => x !== entry);
  }

  // ---- Right-click menus (ui/contextMenu.svelte.js, as in Notes): a row's
  // Edit, the ≈ near-miss switch (corrections) and Delete…, which asks
  // first, as the row's × does — an empty row just goes. A right-click in
  // one of its boxes is the box's own edit menu. ----
  const firstInput = (entry) => rowEls.get(entry)?.querySelector('input');
  // Each entry's row (the rows are keyed by place, so a row's entry can change).
  const rowEls = new Map();
  function rowRef(el, entry) {
    let current = entry;
    rowEls.set(current, el);
    const forget = () => rowEls.get(current) === el && rowEls.delete(current);
    return {
      update(next) {
        forget();
        current = next;
        rowEls.set(current, el);
      },
      destroy: forget,
    };
  }

  function askRemove(entry) {
    const from = (entry.from || '').trim();
    const to = (entry.to || '').trim();
    if (!from && !to) {
      removeEntry(entry);
      return;
    }
    const firstLine = to.split('\n')[0];
    askConfirm({
      title: entry.snippet ? `Delete the snippet “${from || firstLine}”?` : `Delete “${to || from}”?`,
      body: entry.snippet
        ? from
          ? `Saying “${from}” stops typing its text.`
          : 'The snippet goes for good.'
        : from && to
          ? `Yap stops changing “${from}” to “${to}”.`
          : 'The correction goes for good.',
      run: () => removeEntry(entry),
      returnFocus: firstInput(entry),
    });
  }

  function rowMenu(e, entry) {
    const items = [
      {
        label: 'Edit',
        icon: 'pencil',
        onselect: () => {
          const input = firstInput(entry);
          input?.focus();
          input?.select();
        },
      },
    ];
    if (!entry.snippet) {
      items.push({
        label: 'Catch near-misses',
        icon: 'approx',
        toggle: true,
        checked: entry.fuzzy ?? true,
        onselect: () => (entry.fuzzy = !(entry.fuzzy ?? true)),
      });
    }
    items.push({ separator: true });
    items.push({ label: 'Delete…', icon: 'trash', danger: true, onselect: () => askRemove(entry) });
    openContextMenu(e, items, {
      label: entry.snippet ? 'Snippet' : 'Correction',
      row: e.type === 'keydown' && e.target?.focus ? e.target : firstInput(entry),
    });
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
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="dict-row"
                  use:rowRef={entry}
                  oncontextmenu={(e) => rowMenu(e, entry)}
                  onkeydown={(e) => menuKey(e, (k) => rowMenu(k, entry))}
                >
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
                  <button class="rm" title="Remove" aria-label="Remove" onclick={() => askRemove(entry)}>×</button>
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
          desc="Also fix words that nearly match a correction, by spelling or by sound — “jaison” still becomes “JSON”. Turn ≈ off on a row whose near-misses are real words (“Jason”). With a Whisper model, the dictionary goes straight to the recognizer instead, the words you've used most lately first."
          checked={fuzzy}
          onchange={setFuzzy}
        />
      </div>

      <div class="fuzzycard">
        <Toggle
          label="Suggest words from your corrections"
          desc="Fix a word Yap typed — “cube and eighties” to “Kubernetes” — and Yap offers to add it here. It never adds one without asking. For under a minute after each dictation, Yap reads the text around your cursor to notice the fix; it stays on this PC and never looks into password fields."
          checked={learnWords}
          onchange={setLearnWords}
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
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="snip-row"
                  use:rowRef={entry}
                  oncontextmenu={(e) => rowMenu(e, entry)}
                  onkeydown={(e) => menuKey(e, (k) => rowMenu(k, entry))}
                >
                  <div class="say">
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                      <rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5" />
                      <path d="M3 7.5a5 5 0 0 0 10 0M8 12.5v2.5" />
                    </svg>
                    <input placeholder="e.g. my address" aria-label="Phrase you say" bind:value={entry.from} />
                  </div>
                  <span class="arrow">→</span>
                  <textarea
                    placeholder={'e.g. 12 Station Road\nNorwich NR1 4AB'}
                    aria-label="Text Yap types"
                    rows={snippetRows(entry.to)}
                    bind:value={entry.to}
                  ></textarea>
                  <button class="rm" title="Remove" aria-label="Remove snippet" onclick={() => askRemove(entry)}>×</button>
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
  /* Hints, not entries: an empty row must never pass for a saved one. */
  input::placeholder,
  textarea::placeholder {
    color: var(--yap-fg-45);
    font-style: italic;
  }
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
  /* The serif display title, as Meetings and Settings have it. */
  .page-h h1 {
    margin: 0 0 6px;
    font-family: var(--yap-font-display);
    font-size: 28px;
    font-weight: 550;
    letter-spacing: -0.005em;
    line-height: 1.2;
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
    color: var(--yap-danger);
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
