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
  // "Suggest words from your corrections" (`learnWords`, dictionary/learn.rs): words
  // added from a suggestion are saved in Rust; Settings adopts them and
  // passes them on here as `yap-dictionary-external`.
  //
  // For a dictionary of hundreds (2026-10-25): a search (Ctrl+F) over what's
  // heard and written, stars and four sorts (dictionaryList.js; the sort is
  // remembered in this window), checkboxes with Shift-click ranges and
  // Ctrl+A, a bar for what's selected (Delete… with Undo, ≈ on or off, a
  // star, Export…), and Import… / Export… as CSV or text (dictionaryFile.js;
  // the files themselves are dictionary/files.rs's). The list draws 200 rows
  // at a time and more as you scroll, so 2,000 entries stay quick. The order
  // is worked out when the search, the sort or the entries change — never as
  // you type in a row, so a row never jumps away from the cursor — and a row
  // just added with + Add stays at the bottom, whatever the search or sort.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Toggle from '../../ui/Toggle.svelte';
  import { openContextMenu, menuKey, askConfirm, menuOrDialogOpen } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { SORTS, sortOrDefault, arrange, usageKey, entriesLabel, restoreRemoved } from './dictionaryList.js';

  // Import… and Export… (the file's reading and writing, the preview) load
  // the first time they're used: the main window's chunk stays as small as
  // it can (vite.config.js).
  const fileTools = () => import('./dictionaryFile.js');
  let ImportPreview = $state(null);

  let dict = $state([]);
  let fuzzy = $state(true);
  let learnWords = $state(true);
  let loaded = $state(false);
  let saveTimer = null;
  let pending = false;
  let priming = true;
  let gone = false;

  const correctionCount = $derived(dict.filter((e) => !e.snippet).length);
  const snippetCount = $derived(dict.filter((e) => e.snippet).length);

  // ---- Search, sort and the rows drawn ----
  const SORT_KEY = 'yap.dictionarySort';
  const PAGE = 200;
  let query = $state('');
  let sort = $state(sortOrDefault(readSort()));
  let usage = $state({});
  let searchEl = $state(null);
  let limitC = $state(PAGE);
  let limitS = $state(PAGE);
  /** Rows added with + Add on this visit: at the bottom, unsorted, always shown. */
  const fresh = new Set();
  let said = $state('');

  function readSort() {
    try {
      return localStorage.getItem(SORT_KEY);
    } catch {
      return null;
    }
  }
  function setSort(value) {
    sort = sortOrDefault(value);
    try {
      localStorage.setItem(SORT_KEY, sort);
    } catch {
      /* the sort just isn't remembered */
    }
    if (sort === 'used') loadUsage();
  }
  async function loadUsage() {
    try {
      usage = (await invoke('dictionary_usage')) ?? {};
    } catch {
      usage = {};
    }
  }

  // The order, worked out again when the entries (not their text), the
  // search, the sort or the counts change.
  const shown = $derived.by(() => {
    const members = dict.slice();
    const q = query;
    const s = sort;
    const u = usage;
    return untrack(() => {
      const out = arrange(
        members.filter((e) => !fresh.has(e)),
        { query: q, sort: s, usage: u }
      );
      for (const e of members) if (fresh.has(e)) out.push(e);
      return out;
    });
  });
  const shownCorrections = $derived(shown.filter((e) => !e.snippet));
  const shownSnippets = $derived(shown.filter((e) => e.snippet));
  const shownSet = $derived(new Set(shown));
  /** The rows drawn: the first `limit`, and any just added (they're at the end). */
  function drawn(list, limit) {
    const head = list.slice(0, limit);
    for (const e of list.slice(limit)) if (fresh.has(e)) head.push(e);
    return head;
  }
  const drawnCorrections = $derived(drawn(shownCorrections, limitC));
  const drawnSnippets = $derived(drawn(shownSnippets, limitS));
  const searching = $derived(query.trim() !== '');
  const matchCount = $derived(shown.filter((e) => !fresh.has(e)).length);

  // A new search or sort starts from the top of the list.
  $effect(() => {
    query;
    sort;
    limitC = PAGE;
    limitS = PAGE;
  });
  // Said for a screen reader: how many a search found.
  $effect(() => {
    if (!loaded) return;
    said = searching
      ? matchCount
        ? `${entriesLabel(matchCount)} match`
        : 'Nothing matches'
      : '';
  });

  // ---- Selection ----
  const selected = new SvelteSet();
  let anchor = null;
  const visibleOrder = () => [...shownCorrections, ...shownSnippets];
  const selectedList = $derived(dict.filter((e) => selected.has(e)));
  const selectedCorrections = $derived(selectedList.filter((e) => !e.snippet));
  const hiddenSelected = $derived(selectedList.filter((e) => !shownSet.has(e)).length);

  function pick(e, entry) {
    const on = e.currentTarget.checked;
    const order = visibleOrder();
    if (e.shiftKey && anchor && order.includes(anchor)) {
      const a = order.indexOf(anchor);
      const b = order.indexOf(entry);
      for (const x of order.slice(Math.min(a, b), Math.max(a, b) + 1)) {
        if (on) selected.add(x);
        else selected.delete(x);
      }
    } else if (on) {
      selected.add(entry);
    } else {
      selected.delete(entry);
    }
    anchor = entry;
  }
  function selectAllShown() {
    for (const e of visibleOrder()) selected.add(e);
    said = `${entriesLabel(selected.size)} selected`;
  }
  function clearSelection() {
    selected.clear();
    anchor = null;
  }
  function sectionState(list) {
    const n = list.filter((e) => selected.has(e)).length;
    return { all: n > 0 && n === list.length, some: n > 0 && n < list.length };
  }
  function toggleSection(list) {
    const { all } = sectionState(list);
    for (const e of list) {
      if (all) selected.delete(e);
      else selected.add(e);
    }
  }
  // Entries gone from the dictionary leave the selection too.
  $effect(() => {
    const live = new Set(dict);
    untrack(() => {
      for (const e of [...selected]) if (!live.has(e)) selected.delete(e);
    });
  });

  // ---- Keys: Ctrl+F (the page's, unless something's over it), and in the
  // list Ctrl+A, Delete and Esc (outside a text box). ----
  const typingIn = (el) => !!el?.closest?.('input:not([type="checkbox"]), textarea, select, [contenteditable="true"]');
  /** Settings, a dialog or a menu is over the page: the keys are theirs. */
  function covered() {
    if (menuOrDialogOpen() || importing) return true;
    return [...document.querySelectorAll('[aria-modal="true"]')].some((el) => el.checkVisibility?.() ?? el.offsetParent !== null);
  }
  function onWindowKey(e) {
    if (e.defaultPrevented || e.altKey) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'f') {
      if (covered()) return;
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    }
  }
  function onListKey(e) {
    if (e.defaultPrevented || typingIn(e.target) || covered()) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      selectAllShown();
    } else if (e.key === 'Escape' && selected.size) {
      e.preventDefault();
      e.stopPropagation();
      clearSelection();
    } else if (e.key === 'Delete' && selected.size && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      askRemoveSelected(e.currentTarget);
    }
  }
  /** The selection's bar: ← → (Home, End) move between its buttons, as a toolbar's do. */
  function onToolbarKey(e) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    const buttons = [...e.currentTarget.querySelectorAll('button:not(:disabled)')];
    const at = buttons.indexOf(document.activeElement);
    if (!buttons.length) return;
    e.preventDefault();
    const next =
      e.key === 'Home' ? 0 : e.key === 'End' ? buttons.length - 1 : (at + (e.key === 'ArrowRight' ? 1 : -1) + buttons.length) % buttons.length;
    buttons[next].focus();
  }
  function onSearchKey(e) {
    if (e.key === 'Escape' && query) {
      e.preventDefault();
      e.stopPropagation();
      query = '';
    }
  }

  // Not an async onMount: Svelte drops the cleanup an async one returns, and
  // every visit to the view left its listener behind.
  onMount(() => {
    // Settings can also touch the dictionary (Commands' name save) — track it.
    const onExternal = (e) => {
      if (!Array.isArray(e.detail)) return;
      if (JSON.stringify(clean(e.detail)) === JSON.stringify(clean(dict))) return; // no change → no save churn
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
    if (sort === 'used') loadUsage();
    return () => {
      gone = true;
      window.removeEventListener('yap-dictionary-external', onExternal);
      // Leaving with an edit not saved yet (the half-second wait): save it now.
      if (pending) persist();
    };
  });

  // Debounced patch-save: merge onto a FRESH config so this view can never
  // clobber settings changed elsewhere in the meantime.
  $effect(() => {
    // deep-read so any row edit re-runs the effect
    JSON.stringify(dict);
    if (!loaded || priming) return;
    pending = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(persist, 500);
    return () => clearTimeout(saveTimer);
  });

  /** The entries as config.json keeps them: trimmed, a star only when set, no empty rows. */
  function clean(list) {
    return list
      .map((e) => ({
        from: (e.from || '').trim(),
        // A snippet's text keeps its inner line breaks; only the ends trim.
        to: (e.to || '').trim(),
        fuzzy: e.fuzzy ?? true,
        snippet: !!e.snippet,
        ...(e.starred ? { starred: true } : {}),
      }))
      .filter((e) => e.from && e.to);
  }

  async function persist() {
    pending = false;
    clearTimeout(saveTimer);
    try {
      const fresh = await invoke('get_config');
      const cleaned = clean(dict);
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

  /**
   * Undo `change` (a pure list → list): here while the view is open, else
   * on config.json as it is by then (the toast outlives the view).
   */
  async function undo(change) {
    try {
      if (!gone) {
        dict = change(dict);
      } else {
        const cfg = await invoke('get_config');
        const entries = clean(change(Array.isArray(cfg?.dictionary) ? cfg.dictionary : []));
        await invoke('save_config', { cfg: { ...cfg, dictionary: entries } });
        window.dispatchEvent(
          new CustomEvent('yap-dictionary-changed', {
            detail: { entries, fuzzy: cfg.dictionaryFuzzy ?? true, learnWords: cfg.learnWords ?? true },
          })
        );
      }
      toast({ title: 'Undone', variant: 'success', chip: 'Dictionary', icon: 'dictionary', duration: 3_000 });
    } catch (err) {
      toast({ title: "Couldn't undo that", description: String(err), variant: 'destructive' });
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
    fresh.add(dict[dict.length - 1]);
    await tick();
    document.querySelector(snippet ? '.snip-row:last-of-type input:not([type="checkbox"])' : '.dict-row:last-of-type input:not([type="checkbox"])')?.focus();
  }

  /** Take `entries` out, with an Undo that puts them back where they were. */
  function removeEntries(entries) {
    const out = new Set(entries);
    const removed = [];
    dict.forEach((entry, index) => {
      if (out.has(entry)) removed.push({ entry: { ...entry }, index });
    });
    dict = dict.filter((e) => !out.has(e));
    for (const e of entries) selected.delete(e);
    const kept = removed.filter(({ entry }) => (entry.from || '').trim() && (entry.to || '').trim());
    if (!kept.length) return;
    toast({
      kind: 'info',
      title: kept.length === 1 ? `Deleted “${label(kept[0].entry)}”` : `Deleted ${entriesLabel(kept.length)}`,
      chip: 'Dictionary',
      icon: 'dictionary',
      duration: 8_000,
      action: { label: 'Undo', onClick: () => undo((list) => restoreRemoved(list, kept)) },
    });
  }

  /** How a row is named in a question or a toast: the word, or the snippet's phrase. */
  function label(entry) {
    const from = (entry.from || '').trim();
    const to = (entry.to || '').trim();
    return entry.snippet ? from || to.split('\n')[0] : to || from;
  }

  // ---- Right-click menus (ui/contextMenu.svelte.js, as in Notes): a row's
  // Edit, its star, the ≈ near-miss switch (corrections), Select and
  // Delete…, which asks first, as the row's × does — an empty row just goes.
  // A right-click in one of its boxes is the box's own edit menu. ----
  const firstInput = (entry) => rowEls.get(entry)?.querySelector('input:not([type="checkbox"])');
  // Each entry's row.
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
      removeEntries([entry]);
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
      run: () => removeEntries([entry]),
      returnFocus: firstInput(entry),
    });
  }

  function askRemoveSelected(returnFocus = null) {
    const list = selectedList;
    if (!list.length) return;
    if (list.length === 1) {
      askRemove(list[0]);
      return;
    }
    const c = list.filter((e) => !e.snippet).length;
    const s = list.length - c;
    const parts = [c && `${c.toLocaleString()} ${c === 1 ? 'correction' : 'corrections'}`, s && `${s.toLocaleString()} ${s === 1 ? 'snippet' : 'snippets'}`].filter(Boolean);
    askConfirm({
      title: `Delete ${entriesLabel(list.length)}?`,
      body: `${parts.join(' and ')} go from your dictionary${hiddenSelected ? `, ${hiddenSelected.toLocaleString()} of them not shown by the search` : ''}. You can undo it straight after.`,
      run: () => removeEntries(list),
      returnFocus,
    });
  }

  function toggleStar(entry) {
    if (entry.starred) delete entry.starred;
    else entry.starred = true;
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
      { label: 'Star', icon: 'star', toggle: true, checked: !!entry.starred, onselect: () => toggleStar(entry) },
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

  // ---- What's selected, together ----
  function setSelectedFuzzy(on) {
    for (const e of selectedCorrections) e.fuzzy = on;
    said = `Near-misses ${on ? 'on' : 'off'} for ${selectedCorrections.length.toLocaleString()} ${selectedCorrections.length === 1 ? 'correction' : 'corrections'}`;
  }
  const allStarred = $derived(selectedList.length > 0 && selectedList.every((e) => e.starred));
  function starSelected() {
    const on = !allStarred;
    for (const e of selectedList) {
      if (on) e.starred = true;
      else delete e.starred;
    }
    said = `${on ? 'Starred' : 'Unstarred'} ${entriesLabel(selectedList.length)}`;
  }

  // ---- Export ----
  function exportMenu(e, entries, what) {
    const snippetsOnly = entries.length > 0 && entries.every((x) => x.snippet);
    openContextMenu(
      e,
      [
        { label: 'As CSV…', icon: 'download', disabled: !entries.length, onselect: () => exportAs(entries, 'csv') },
        {
          label: 'As text…',
          icon: 'download',
          disabled: !entries.length || snippetsOnly,
          onselect: () => exportAs(entries, 'txt'),
        },
        entries.length
          ? { note: 'CSV keeps snippets, stars and ≈; text is one “heard → written” a line, corrections only.' }
          : { note: 'Nothing to export yet.' },
      ],
      { label: `Export ${what}` }
    );
  }
  async function exportAs(entries, kind) {
    const list = clean(entries);
    try {
      const { toCsv, toText, exportName } = await fileTools();
      let text;
      let left = 0;
      if (kind === 'csv') text = toCsv(list);
      else ({ text, left } = toText(list));
      const saved = await invoke('dictionary_export_save', { text, name: exportName(kind), kind });
      if (!saved) return;
      const n = list.length - left;
      toast({
        kind: 'done',
        title: `Exported ${entriesLabel(n)}`,
        description: `Saved as “${saved}”.${left ? ` Snippets need CSV: ${left.toLocaleString()} left out.` : ''}`,
        variant: 'success',
        chip: 'Dictionary',
        icon: 'dictionary',
        duration: 5_000,
      });
    } catch (err) {
      toast({ title: "Couldn't export the dictionary", description: String(err), variant: 'destructive' });
    }
  }

  // ---- Import ----
  let importing = $state(null);
  let importButton = $state(null);

  async function importFile() {
    let picked;
    let file;
    try {
      [picked, file, { default: ImportPreview }] = await Promise.all([
        invoke('dictionary_import_pick'),
        fileTools(),
        import('./ImportPreview.svelte'),
      ]);
    } catch (err) {
      toast({ title: "Couldn't read that file", description: String(err), variant: 'destructive' });
      return;
    }
    if (!picked || gone) return;
    const { format, rows, skipped } = file.parseDictionaryFile(picked.text, picked.name);
    importing = { name: picked.name, format, skipped, plan: file.planImport(rows, dict), file };
  }

  function applyImport(choices) {
    const job = importing;
    importing = null;
    if (!job) return;
    const { add, respell } = job.file.importChanges(job.plan, choices);
    const respelled = [];
    for (const { entry, to } of respell) {
      if (entry.to === to) continue;
      respelled.push({ from: entry.from, snippet: !!entry.snippet, was: entry.to, now: to });
      entry.to = to;
    }
    if (add.length) dict = [...dict, ...add];
    if (!add.length && !respelled.length) return;
    const bits = [add.length && `Added ${entriesLabel(add.length)}`, respelled.length && `${add.length ? 'changed' : 'Changed'} ${respelled.length.toLocaleString()}`].filter(Boolean);
    toast({
      kind: 'info',
      title: bits.join(', '),
      description: `From “${job.name}”.`,
      chip: 'Dictionary',
      icon: 'dictionary',
      duration: 8_000,
      action: { label: 'Undo', onClick: () => undo((list) => job.file.undoImport(list, { added: add, respelled })) },
    });
  }

  // Draw more rows as the end of a list comes into view (or its button is pressed).
  function autoMore(node, grow) {
    let current = grow;
    const io = new IntersectionObserver((seen) => seen.some((s) => s.isIntersecting) && current(), {
      root: node.closest('.wrap'),
      rootMargin: '0px 0px 600px 0px',
    });
    io.observe(node);
    return {
      update(next) {
        current = next;
      },
      destroy: () => io.disconnect(),
    };
  }

  const useCount = (entry) => usage?.[usageKey(entry)]?.uses ?? 0;

  /** A checkbox's mixed state (some of a section selected): a property, not an attribute. */
  function mixed(node, on) {
    node.indeterminate = !!on;
    return {
      update(next) {
        node.indeterminate = !!next;
      },
    };
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#snippet star(entry)}
  <button
    type="button"
    class="star"
    class:on={entry.starred}
    aria-pressed={!!entry.starred}
    aria-label="Star"
    title={entry.starred ? 'Starred: listed first, and given to the speech model first. Click to unstar.' : 'Star: list it first, and give it to the speech model first'}
    onclick={() => toggleStar(entry)}
  >
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.6l1.95 4.02 4.42.62-3.22 3.1.79 4.4L8 11.65l-3.94 2.09.79-4.4-3.22-3.1 4.42-.62z" /></svg>
  </button>
{/snippet}

{#snippet check(entry, name)}
  <input
    type="checkbox"
    class="pick"
    checked={selected.has(entry)}
    aria-label={`Select ${name}`}
    onclick={(e) => pick(e, entry)}
  />
{/snippet}

{#snippet sectionCheck(list, what)}
  {@const st = sectionState(list)}
  <input
    type="checkbox"
    class="pick"
    checked={st.all}
    use:mixed={st.some}
    disabled={!list.length}
    aria-label={`Select every ${what} shown`}
    onclick={() => toggleSection(list)}
  />
{/snippet}

{#snippet more(total, drawnCount, grow)}
  {#if total > drawnCount}
    <button type="button" class="more" use:autoMore={grow} onclick={grow}>
      Show {Math.min(PAGE, total - drawnCount).toLocaleString()} more ({(total - drawnCount).toLocaleString()} not shown yet)
    </button>
  {/if}
{/snippet}

<div class="wrap">
  <div class="inner">
    <div class="page-h">
      <h1>Dictionary</h1>
      <p>
        Corrections fix words Yap mishears. Snippets type out text you use a lot when you say a
        short phrase.
      </p>
    </div>

    <div class="tools">
      <div class="toolrow">
        <div class="search">
          <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4.6" /><path d="M10.4 10.4L14 14" /></svg>
          <input
            bind:this={searchEl}
            bind:value={query}
            type="search"
            placeholder="Search words, how they're heard, snippets"
            aria-label="Search the dictionary"
            autocomplete="off"
            spellcheck="false"
            onkeydown={onSearchKey}
          />
          {#if query}
            <button type="button" class="clear" aria-label="Clear the search" onclick={() => ((query = ''), searchEl?.focus())}>×</button>
          {:else}
            <kbd aria-hidden="true">Ctrl F</kbd>
          {/if}
        </div>
        <label class="sort">
          <span>Sort</span>
          <select value={sort} aria-label="Sort the dictionary" onchange={(e) => setSort(e.currentTarget.value)}>
            {#each SORTS as s (s.value)}
              <option value={s.value}>{s.label}</option>
            {/each}
          </select>
        </label>
        <button type="button" class="tool" bind:this={importButton} onclick={importFile}>Import…</button>
        <button type="button" class="tool" data-more aria-haspopup="menu" onclick={(e) => exportMenu(e, dict, 'everything')}>Export…</button>
      </div>

      {#if selected.size > 0}
        <div class="bulk" role="toolbar" aria-label="Selected entries" tabindex="-1" onkeydown={onToolbarKey}>
          <span class="count">
            {entriesLabel(selected.size)} selected{#if hiddenSelected}<span class="hidden-note">{` · ${hiddenSelected.toLocaleString()} not shown`}</span>{/if}
          </span>
          <button type="button" class="bbtn" onclick={starSelected}>{allStarred ? 'Unstar' : 'Star'}</button>
          <button
            type="button"
            class="bbtn"
            disabled={!selectedCorrections.length}
            title="Also correct near-misses of these words"
            onclick={() => setSelectedFuzzy(true)}
          >≈ Catch near-misses</button>
          <button
            type="button"
            class="bbtn"
            disabled={!selectedCorrections.length}
            title="Exact matches only for these words"
            onclick={() => setSelectedFuzzy(false)}
          >Exact only</button>
          <button type="button" class="bbtn" data-more aria-haspopup="menu" onclick={(e) => exportMenu(e, selectedList, 'the selection')}>Export…</button>
          <button type="button" class="bbtn danger" onclick={(e) => askRemoveSelected(e.currentTarget)}>Delete…</button>
          <button type="button" class="bbtn quiet" aria-label="Clear the selection" title="Clear the selection (Esc)" onclick={clearSelection}>×</button>
        </div>
      {/if}

      {#if searching && loaded}
        <p class="found" aria-hidden="true">
          {#if matchCount}
            {entriesLabel(matchCount)} of {dict.length.toLocaleString()} match “{query.trim()}”
          {:else}
            Nothing matches “{query.trim()}”.
            <button type="button" class="linkish" onclick={() => (query = '')}>Clear the search</button>
          {/if}
        </p>
      {/if}
      <!-- Said, not shown (not role="status": that's the toasts'). -->
      <p class="sr" aria-live="polite" aria-atomic="true">{said}</p>
    </div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="lists" tabindex="-1" onkeydown={onListKey}>
      <section aria-labelledby="dict-corrections">
        <div class="sec-h">
          <div class="sec-title">
            <h2 id="dict-corrections">Corrections</h2>
            {#if correctionCount}<span class="sec-count">{correctionCount.toLocaleString()}</span>{/if}
          </div>
          <p>
            “Power to Keep” → “Parakeet”. Case-insensitive, applied to every transcription — and given
            to the AI cleanup model as spelling context.
          </p>
        </div>
        <div class="dictcard">
          {#if correctionCount > 0}
            <div class="dict-head" class:uses-col={sort === 'used'}>
              {@render sectionCheck(shownCorrections, 'correction')}
              <span></span><span>Heard</span><span></span><span>Replace with</span>
              {#if sort === 'used'}<span class="uses-h">Used</span>{/if}
              <span></span><span></span>
            </div>
            <div class="rows">
              {#each drawnCorrections as entry (entry)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="dict-row"
                  class:sel={selected.has(entry)}
                  class:uses-col={sort === 'used'}
                  use:rowRef={entry}
                  oncontextmenu={(e) => rowMenu(e, entry)}
                  onkeydown={(e) => menuKey(e, (k) => rowMenu(k, entry))}
                >
                  {@render check(entry, entry.to || entry.from ? `“${entry.to || entry.from}”` : 'this correction')}
                  {@render star(entry)}
                  <input placeholder="Power to Keep" aria-label="Heard" bind:value={entry.from} />
                  <span class="arrow">→</span>
                  <input placeholder="Parakeet" aria-label="Replace with" bind:value={entry.to} />
                  {#if sort === 'used'}
                    {@const n = useCount(entry)}
                    <span class="uses" title={n ? `Used ${n.toLocaleString()} ${n === 1 ? 'time' : 'times'}` : 'Not used yet'}>{n ? `${n.toLocaleString()}×` : '–'}</span>
                  {/if}
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
              {/each}
            </div>
            {@render more(shownCorrections.length, drawnCorrections.length, () => (limitC += PAGE))}
            {#if !shownCorrections.length}
              <div class="empty">No corrections match.</div>
            {/if}
          {:else if loaded}
            <div class="empty">No corrections yet.</div>
          {/if}
          <button class="add" onclick={() => addEntry(false)}>+ Add correction</button>
        </div>

        <div class="fuzzycard">
          <Toggle
            label="Catch near-misses"
            desc="Also fix words that nearly match a correction, by spelling or by sound — “jaison” still becomes “JSON”. Turn ≈ off on a row whose near-misses are real words (“Jason”). With a Whisper model, the dictionary goes straight to the recognizer instead: starred words first, then the ones you've used most lately."
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
          <div class="sec-title">
            <h2 id="dict-snippets">Snippets</h2>
            {#if snippetCount}<span class="sec-count">{snippetCount.toLocaleString()}</span>{/if}
          </div>
          <p>
            Say the phrase while you dictate and Yap types the text in its place — sign-offs,
            addresses, links. Capitals and punctuation around the phrase don't matter. Dictations
            only; meeting notes and uploads keep what was said.
          </p>
        </div>
        <div class="dictcard snippets">
          {#if snippetCount > 0}
            <div class="snip-head">
              {@render sectionCheck(shownSnippets, 'snippet')}
              <span></span><span>You say</span><span></span><span>Yap types</span><span></span>
            </div>
            <div class="rows">
              {#each drawnSnippets as entry (entry)}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="snip-row"
                  class:sel={selected.has(entry)}
                  use:rowRef={entry}
                  oncontextmenu={(e) => rowMenu(e, entry)}
                  onkeydown={(e) => menuKey(e, (k) => rowMenu(k, entry))}
                >
                  {@render check(entry, entry.from ? `the snippet “${entry.from}”` : 'this snippet')}
                  {@render star(entry)}
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
              {/each}
            </div>
            {@render more(shownSnippets.length, drawnSnippets.length, () => (limitS += PAGE))}
            {#if !shownSnippets.length}
              <div class="empty">No snippets match.</div>
            {/if}
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
</div>

{#if importing && ImportPreview}
  <ImportPreview
    name={importing.name}
    format={importing.format}
    plan={importing.plan}
    skipped={importing.skipped}
    returnFocus={importButton}
    onapply={applyImport}
    onclose={() => (importing = null)}
  />
{/if}

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
    margin: 0 0 16px;
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

  /* ---- The tools: search, sort, import/export; the selection's bar. Kept
     in view while the list scrolls under them. ---- */
  .tools {
    position: sticky;
    top: 0;
    z-index: 2;
    margin: 0 -8px 18px;
    padding: 8px 8px 6px;
    /* The view's own surface (ControlPanel's .view), so rows pass under it. */
    background: var(--yap-s1);
  }
  .toolrow {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .search {
    flex: 1 1 240px;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 32px;
    padding: 0 6px 0 10px;
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
  .search svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
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
    font-style: normal;
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
  .search .clear {
    flex: 0 0 auto;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-fg-62);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }
  .search .clear:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .sort {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .sort select,
  .tool {
    height: 32px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .sort select {
    padding: 0 6px;
  }
  .tool {
    padding: 0 12px;
  }
  .sort select:hover,
  .tool:hover {
    border-color: var(--yap-border-hover);
  }
  .sort select:focus-visible,
  .tool:focus-visible,
  .bbtn:focus-visible,
  .more:focus-visible,
  .add:focus-visible,
  .linkish:focus-visible,
  .search .clear:focus-visible,
  .star:focus-visible,
  .fz:focus-visible,
  .rm:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }

  .bulk {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 8px;
    padding: 6px 6px 6px 12px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
    outline: none;
  }
  .bulk .count {
    margin-right: auto;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .hidden-note {
    font-weight: 400;
    color: var(--yap-muted);
  }
  .bbtn {
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .bbtn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .bbtn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .bbtn.danger {
    color: var(--yap-danger);
  }
  .bbtn.quiet {
    width: 28px;
    padding: 0;
    border-color: transparent;
    background: none;
    color: var(--yap-muted);
    font-size: 16px;
  }
  .found {
    margin: 8px 2px 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .linkish {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
  .lists {
    outline: none;
  }

  section + section {
    margin-top: 28px;
  }
  .sec-h {
    margin: 0 0 10px;
  }
  .sec-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 0 2px;
  }
  .sec-h h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .sec-count {
    font-size: 11.5px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
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
    grid-template-columns: 16px 22px 1fr 14px 1fr 24px 22px;
    align-items: center;
    gap: 6px;
    padding: 3px 0;
  }
  .dict-head.uses-col,
  .dict-row.uses-col {
    grid-template-columns: 16px 22px 1fr 14px 1fr 40px 24px 22px;
  }
  .dict-row,
  .snip-row {
    border-radius: var(--yap-r-sm);
    /* Off-screen rows skip layout and paint (a long list scrolls smoothly). */
    content-visibility: auto;
    contain-intrinsic-size: auto 38px;
  }
  .dict-row.sel,
  .snip-row.sel {
    background: var(--yap-primary-wash);
    box-shadow: -6px 0 0 var(--yap-primary-wash), 6px 0 0 var(--yap-primary-wash);
  }
  .dict-head,
  .snip-head {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--yap-muted-55);
  }
  .pick {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--yap-primary);
    cursor: pointer;
  }
  .pick:disabled {
    cursor: default;
  }
  .pick:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  .star {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .star svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linejoin: round;
  }
  .star:hover {
    color: var(--yap-fg-62);
  }
  .star.on {
    color: var(--yap-primary);
  }
  .star.on svg {
    fill: currentColor;
  }
  .uses,
  .uses-h {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .uses {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .dict-row input:not(.pick),
  .snip-row input:not(.pick),
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
  .dict-row input:not(.pick):focus,
  .snip-row input:not(.pick):focus,
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
    grid-template-columns: 16px 22px minmax(0, 1fr) 14px minmax(0, 1.6fr) 22px;
    gap: 6px;
    padding: 3px 0;
  }
  .snip-head {
    align-items: center;
  }
  .snip-row {
    align-items: start;
    padding: 5px 0;
    contain-intrinsic-size: auto 64px;
  }
  .snip-row + .snip-row {
    border-top: 1px dashed var(--yap-border-subtle);
  }
  .snip-row .arrow,
  .snip-row .rm {
    margin-top: 6px;
  }
  .snip-row .pick {
    margin-top: 10px;
  }
  .snip-row .star {
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
  .more {
    display: block;
    width: 100%;
    margin-top: 6px;
    padding: 7px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .more:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
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
