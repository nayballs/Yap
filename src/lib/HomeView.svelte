<script>
  // Home — the dictation feed, Wispr-Flow-style (2026-07-08 restyle): a
  // time-of-day greeting with the hotkey rendered as keycaps, a dark hero
  // card teaching voice editing, the day-grouped history feed with hover
  // actions, and a right-rail stats card with serif display numerals.
  // A dictation whose audio Yap still keeps (recent_audio.rs: a day) can be
  // **run again** (on the same model or another) and **played**; one that
  // failed (never transcribed, or its paste didn't land) says so, with Run
  // again at hand. A dictation whose words AI cleanup changed says so —
  // "AI edited · N changes" (`said` beside `text`, lib/wordDiff.js) — and
  // opens into what changed, with **Paste what I said** (as_said.rs: swapped
  // in its app right after the dictation, else copied).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import { homeSnapshot } from './homeData.js';
  import { formatHotkeySpec } from './core/hotkeys.js';
  import { appLabel } from './core/appNames.js';
  import { openContextMenu, menuKey, menuOpenFor, askConfirm } from './ui/contextMenu.svelte.js';
  import { copyText } from './core/itemMenus.js';
  import { toast, dismiss } from './ui/toast.svelte.js';
  import { showSaveError } from './saveErrors.js';
  import { MODELS } from './core/speechModels.js';
  import { attention } from './stores/attention.svelte.js';
  import { wordDiff } from './core/wordDiff.js';

  let { onopensettings = null, onnavigate = null } = $props();
  // Settings' live "needs attention" items: no speech model yet means the
  // hotkey can't do anything, so the empty feed says that first.
  const noSpeechModel = $derived(attention.items.some((i) => i.section === 'models'));

  let entries = $state([]);
  let stats = $state(null);
  let historyEnabled = $state(true);
  let hotkeyLabel = $state('F9');
  let editHotkeyLabel = $state('');
  let assistantHotkeyLabel = $state('');
  let cleanupEnabled = $state(false);
  let copiedTs = $state(null);
  // Run again / Play (recent_audio.rs).
  let installed = $state([]);
  let activeModel = $state('');
  let running = $state(null);
  let playing = $state(null);
  let player = null;
  // What AI cleanup changed (its row's key while open), and Paste what I
  // said under way.
  let openDiff = $state(null);
  let pastingSaid = $state(null);

  // Rotating hero tips: a different card each day (dots to browse). Each tip
  // teaches one feature that already exists — the hero is onboarding forever:
  // Commands (rewrite by voice), AI cleanup, meeting notes, per-app profiles,
  // the Voice Assistant.
  const TIP_COUNT = 5;
  let tipIndex = $state(Math.floor(Date.now() / 86_400_000) % TIP_COUNT);
  // The tip a new user sees first (Speak rough, send polished), and whether
  // they've picked one with the dots since.
  const FIRST_TIP = 1;
  let tipBrowsed = false;
  // Feed search (OpenWhispr Ctrl+K): plain client-side text filter. Wispr
  // style: just an icon until opened.
  let query = $state('');
  let searchOpen = $state(false);
  let searchEl = $state(null);

  async function openSearch() {
    searchOpen = true;
    await tick();
    searchEl?.focus();
  }
  function closeSearch() {
    query = '';
    searchOpen = false;
  }

  const greeting = (() => {
    const h = new Date().getHours();
    if (h < 5) return 'Up late';
    if (h < 12) return 'Good morning';
    if (h < 18) return 'Good afternoon';
    return 'Good evening';
  })();

  // "Ctrl+Shift+Space" → ["Ctrl", "Shift", "Space"] for keycap rendering.
  function keyParts(label) {
    return String(label || '')
      .split('+')
      .map((p) => p.trim())
      .filter(Boolean);
  }

  function onSearchKey(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      openSearch();
    } else if (e.key === 'Escape' && document.activeElement === searchEl) {
      closeSearch();
      searchEl?.blur();
    }
  }

  // Home's data has come (`data-ready` on the page: the e2e suite times
  // Home's first paint by it).
  let ready = $state(false);

  // Everything Home shows, in one round trip (homeData.js): the history
  // page, the stats, the settings it reads and the installed models.
  async function refresh() {
    let snap = null;
    try {
      snap = await homeSnapshot();
    } catch {
      /* drawn empty */
    }
    entries = snap?.history || [];
    stats = snap?.stats ?? null;
    const cfg = snap?.config;
    if (cfg) {
      historyEnabled = cfg.historyEnabled !== false;
      cleanupEnabled = cfg.postProcessEnabled === true;
      activeModel = cfg.modelSize || '';
      if (cfg.hotkey) hotkeyLabel = formatHotkeySpec(cfg.hotkey);
      editHotkeyLabel = cfg.editHotkey ? formatHotkeySpec(cfg.editHotkey) : '';
      assistantHotkeyLabel = cfg.assistantHotkey ? formatHotkeySpec(cfg.assistantHotkey) : '';
    }
    if (Array.isArray(snap?.installedModels)) installed = snap.installedModels;
    ready = true;
    // Someone who hasn't dictated yet starts on the basics — AI cleanup —
    // rather than the day's tip (per-app profiles, say), until they browse.
    if (!tipBrowsed && stats && (stats.totalTranscriptions ?? 0) === 0) tipIndex = FIRST_TIP;
  }

  onMount(() => {
    refresh();
    // History changed (a dictation or upload saved, an entry deleted, all
    // cleared) → re-pull. Not `yap-transcript`: a dictation emits that before
    // it pastes and saves, so a re-pull on it could miss the new entry.
    let un;
    listen('yap-history-changed', () => refresh()).then((u) => (un = u));
    return () => {
      un && un();
      stopPlaying();
    };
  });

  const modelName = (id) => MODELS.find((m) => m.id === id)?.name || id;

  /** Run a kept dictation again (`history_rerun`): `model` null = the one in use. */
  async function rerun(e, model = null) {
    if (running) return;
    running = rowKey(e);
    const busy = toast({ title: 'Running it again…', description: modelName(model || activeModel), busy: true, duration: 0, chip: 'Dictation' });
    try {
      const r = await invoke('history_rerun', { ts: e.ts, audio: e.audio, model });
      dismiss(busy);
      const preview = r.text.length > 90 ? `${r.text.slice(0, 87).trimEnd()}…` : r.text;
      toast({
        title: 'Transcribed again',
        description: `${r.modelName}: “${preview}”`,
        variant: 'success',
        action: { label: 'Copy', onClick: () => copyText(r.text) },
      });
    } catch (err) {
      dismiss(busy);
      toast({ title: "Couldn't run it again", description: String(err), variant: 'destructive' });
    } finally {
      running = null;
      refresh();
    }
  }

  /** Run again on the model in use, or — with others installed — pick one. */
  function rerunMenu(ev, e) {
    const others = installed.filter((m) => m !== activeModel);
    if (!others.length) return rerun(e);
    openContextMenu(
      ev,
      [
        { label: `Run again with ${modelName(activeModel)}`, icon: 'sync', onselect: () => rerun(e) },
        { separator: true },
        ...others.map((m) => ({ label: `With ${modelName(m)}`, onselect: () => rerun(e, m) })),
      ],
      { label: 'Run again' }
    );
  }

  function stopPlaying() {
    if (player) {
      player.pause();
      URL.revokeObjectURL(player.src);
      player = null;
    }
    playing = null;
  }

  /** Play (or stop) a kept dictation's audio. */
  async function play(e) {
    const key = rowKey(e);
    const again = playing === key;
    stopPlaying();
    if (again) return;
    try {
      const bytes = await invoke('recent_audio_wav', { id: e.audio });
      const url = URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' }));
      const a = new Audio(url);
      a.onended = () => player === a && stopPlaying();
      player = a;
      playing = key;
      await a.play();
    } catch (err) {
      stopPlaying();
      toast({ title: "Couldn't play it", description: String(err), variant: 'destructive' });
    }
  }

  // Group entries (newest first) into local-day buckets with friendly labels,
  // after applying the search filter (text, app, or model).
  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const shown = !q
      ? entries
      : entries.filter(
          (e) =>
            (e.text || '').toLowerCase().includes(q) ||
            (e.app || '').toLowerCase().includes(q) ||
            (e.model || '').toLowerCase().includes(q)
        );
    const out = [];
    let currentKey = null;
    let bucket = null;
    for (const e of shown) {
      const d = new Date(e.ts * 1000);
      const key = d.toDateString();
      if (key !== currentKey) {
        currentKey = key;
        bucket = { label: dayLabel(d), items: [] };
        out.push(bucket);
      }
      bucket.items.push(e);
    }
    return out;
  });

  function dayLabel(d) {
    const today = new Date();
    const yesterday = new Date(today.getTime() - 86_400_000);
    if (d.toDateString() === today.toDateString()) return 'Today';
    if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
    return d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' });
  }

  function timeOf(ts) {
    return new Date(ts * 1000).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  const appName = appLabel;

  async function copy(e) {
    try {
      await navigator.clipboard.writeText(e.text);
      copiedTs = e.ts;
      setTimeout(() => (copiedTs = null), 1500);
    } catch {
      /* clipboard unavailable */
    }
  }

  async function remove(e) {
    try {
      await invoke('delete_history_entry', { ts: e.ts, text: e.text });
    } catch (err) {
      // Deleted but not saved yet: Yap says so, with Try again.
      if (!showSaveError(err)) toast({ title: "Couldn't delete the dictation", description: String(err), variant: 'destructive' });
    }
    await refresh();
    return true;
  }

  // ---- Right-click menus (ui/contextMenu.svelte.js, as in Notes): Copy,
  // Copy raw transcript (the words before cleanup and the dictionary, when
  // they differ), Delete… — also from the row's hover buttons' place via the
  // ContextMenu key / Shift+F10 on the focused row. ----
  // (Two dictations can share a second and their words: the audio id tells
  // them apart, and the feed keys rows by their place too.)
  const rowKey = (e) => `${e.ts}:${e.audio ?? ''}:${e.text}`;
  let rowEls = $state({});

  /** The trash button and the menu's Delete… both ask first. */
  function askRemove(e) {
    const preview = e.text.length > 60 ? `${e.text.slice(0, 57).trimEnd()}…` : e.text;
    askConfirm({
      title: 'Delete this dictation?',
      body: e.text
        ? `“${preview}” leaves your history and stats.${e.audio ? ' Its audio goes too.' : ''} The text you dictated stays where Yap typed it.`
        : 'It leaves your history, and its audio goes too.',
      run: () => remove(e),
      returnFocus: rowEls[rowKey(e)],
    });
  }

  // ---- What AI cleanup changed: the words as said (`said`) against what
  // was pasted, word by word (lib/wordDiff.js: capitals and punctuation
  // aside). Only a dictation whose words it changed says "AI edited". ----
  const aiEdits = $derived(
    new Map(
      entries
        .filter((e) => e.said && e.text && e.status !== 'failed')
        .map((e) => [rowKey(e), wordDiff(e.said, e.text)])
        .filter(([, d]) => d.changes > 0)
    )
  );

  function toggleDiff(e) {
    const key = rowKey(e);
    openDiff = openDiff === key ? null : key;
  }

  /** "it's been…" → "It's been…". */
  const sentence = (s) => (s ? s[0].toUpperCase() + s.slice(1) : '');

  /** Paste what I said (`as_said_paste_entry`): swapped in its app right after the dictation, else copied. */
  async function pasteSaid(e) {
    if (pastingSaid) return;
    pastingSaid = rowKey(e);
    try {
      const r = await invoke('as_said_paste_entry', { ts: e.ts, text: e.text });
      if (r.outcome === 'swapped') {
        toast({
          title: 'Swapped for what you said',
          description: r.checked ? 'In the app you dictated into.' : "In the app you dictated into. Yap couldn't read it back to check.",
          variant: 'success',
        });
      } else if (r.outcome === 'copied' && !r.told) {
        toast({ title: 'Copied what you said', description: `${sentence(r.why)}. Press Ctrl+V to paste it.`, chip: 'Dictation', icon: 'clipboard' });
      } else if (r.outcome === 'testMode') {
        toast({ title: 'A test run: nothing swapped' });
      }
    } catch (err) {
      toast({ title: "Couldn't paste what you said", description: String(err), variant: 'destructive' });
    } finally {
      pastingSaid = null;
    }
  }

  function rowMenu(ev, e) {
    const raw = (e.raw || '').trim();
    const edited = aiEdits.has(rowKey(e));
    openContextMenu(
      ev,
      [
        e.text ? { label: 'Copy', icon: 'copy', onselect: () => copyText(e.text) } : null,
        edited ? { label: 'Paste what I said', icon: 'paste', onselect: () => pasteSaid(e) } : null,
        edited ? { label: 'Copy what I said', icon: 'copy', onselect: () => copyText(e.said, 'Copied what you said') } : null,
        raw && raw !== e.text.trim() && !edited
          ? { label: 'Copy raw transcript', icon: 'copy', onselect: () => copyText(raw, 'Copied the raw transcript') }
          : null,
        e.audio ? { label: 'Run again', icon: 'sync', onselect: () => rerun(e) } : null,
        e.audio && installed.some((m) => m !== activeModel)
          ? { label: 'Run again with', items: installed.filter((m) => m !== activeModel).map((m) => ({ label: modelName(m), onselect: () => rerun(e, m) })) }
          : null,
        e.audio ? { label: playing === rowKey(e) ? 'Stop playing' : 'Play', icon: playing === rowKey(e) ? 'stop' : 'play', onselect: () => play(e) } : null,
        { separator: true },
        { label: 'Delete…', icon: 'trash', danger: true, onselect: () => askRemove(e) },
      ],
      { label: 'Dictation', row: rowEls[rowKey(e)] }
    );
  }

  function fmtTimeSaved(min) {
    if (min == null) return '0m';
    if (min < 60) return `${Math.round(min)}m`;
    return `${(min / 60).toFixed(1)}h`;
  }

  function fmtNum(n) {
    return (n ?? 0).toLocaleString();
  }
</script>

{#snippet keycaps(label)}
  <span class="keys">
    {#each keyParts(label) as part, i}
      {#if i > 0}<span class="keyplus">+</span>{/if}
      <kbd class="key">{part}</kbd>
    {/each}
  </span>
{/snippet}

<svelte:window onkeydown={onSearchKey} />

<div class="home" data-ready={ready || undefined}>
  <div class="wrap">
    <h1 class="hello">
      {greeting}. Press {@render keycaps(hotkeyLabel)} to start yapping.
    </h1>

    <div class="cols">
      <div class="feed">
        <div class="hero">
          <div class="herotext">
            {#if tipIndex === 0}
              <h2>Edit anything with <em>your voice</em></h2>
              <p>
                Select text in any app,
                {#if editHotkeyLabel}
                  hold {@render keycaps(editHotkeyLabel)},
                {:else}
                  hold your rewrite shortcut,
                {/if}
                and say the change — “make this a list”, “more formal”, “fix the grammar”.
              </p>
            {:else if tipIndex === 1}
              <h2>Speak rough, send <em>polished</em></h2>
              <p>
                {#if cleanupEnabled}
                  AI cleanup is on — your rambling becomes ready-to-send text. Tune the tone and rules in Prompt Studio.
                {:else}
                  Turn on AI cleanup and your rambling becomes ready-to-send text — private, on this PC if you want.
                {/if}
              </p>
            {:else if tipIndex === 2}
              <h2>The notepad that takes <em>your meeting notes</em></h2>
              <p>
                Hit Record on a note and Yap transcribes you and the other side — then writes up decisions and action items.
              </p>
            {:else if tipIndex === 3}
              <h2>A different style for <em>every app</em></h2>
              <p>
                Formal in email, casual in Slack, precise in your editor — per-app profiles switch the cleanup style automatically.
              </p>
            {:else}
              <h2>Ask out loud, <em>hear the answer</em></h2>
              <p>
                {#if assistantHotkeyLabel}
                  Press {@render keycaps(assistantHotkeyLabel)} in any app and ask about your notes or anything else — Yap answers out loud, without leaving what you're doing.
                {:else}
                  Give the Voice Assistant a shortcut, then ask from any app — Yap answers out loud, and the conversation waits in Chat.
                {/if}
              </p>
            {/if}
          </div>
          <div class="heroside">
            {#if tipIndex === 0}
              <button class="herocta" onclick={() => onopensettings?.('commands')}>
                {editHotkeyLabel ? 'Tune Commands' : 'Set up Commands'}
              </button>
            {:else if tipIndex === 1}
              <button class="herocta" onclick={() => onopensettings?.('cleanup')}>
                {cleanupEnabled ? 'Open Prompt Studio' : 'Turn on AI cleanup'}
              </button>
            {:else if tipIndex === 2}
              <button class="herocta" onclick={() => onnavigate?.('notes')}>Open Notes</button>
            {:else if tipIndex === 3}
              <button class="herocta" onclick={() => onopensettings?.('cleanup')}>Set up profiles</button>
            {:else}
              <button class="herocta" onclick={() => onopensettings?.('voice')}>
                {assistantHotkeyLabel ? 'Voice Assistant settings' : 'Set an assistant shortcut'}
              </button>
            {/if}
            <div class="herodots" role="tablist" aria-label="Tips">
              {#each Array(TIP_COUNT) as _, i}
                <button
                  class="herodot"
                  class:on={i === tipIndex}
                  role="tab"
                  aria-selected={i === tipIndex}
                  aria-label={`Tip ${i + 1}`}
                  onclick={() => {
                    tipIndex = i;
                    tipBrowsed = true;
                  }}
                ></button>
              {/each}
            </div>
          </div>
        </div>

        {#if !historyEnabled}
          <div class="notice">
            History is turned off, so new dictations won't appear here.
            <button class="link" onclick={() => onopensettings?.('privacy#history')}>Turn it on in Settings → Privacy & data</button>
          </div>
        {/if}

        {#if entries.length > 0}
          <div class="searchrow">
            {#if searchOpen || query.trim()}
              <div class="search">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
                <input
                  bind:this={searchEl}
                  bind:value={query}
                  placeholder="Search transcriptions…"
                  onblur={() => !query.trim() && (searchOpen = false)}
                />
                <button class="searchx" aria-label="Close search" onclick={closeSearch}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
                </button>
              </div>
            {:else}
              <button class="searchbtn" title="Search transcriptions (Ctrl+K)" aria-label="Search transcriptions" onclick={openSearch}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m21 21-4.3-4.3" /></svg>
              </button>
            {/if}
          </div>
        {/if}

        {#if groups.length === 0 && query.trim()}
          <div class="empty">
            <h2>No matches</h2>
            <p>Nothing in your history matches “{query.trim()}”.</p>
          </div>
        {:else if groups.length === 0}
          <div class="empty">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="9" y="2" width="6" height="12" rx="3" />
              <path d="M5 10v1a7 7 0 0 0 14 0v-1" />
              <path d="M12 18v4M8 22h8" />
            </svg>
            {#if noSpeechModel}
              <!-- The first thing a new install needs: the hotkey does
                   nothing until a speech model is on this PC. -->
              <h2>One step left: a speech model</h2>
              <p>Yap turns your voice into text with a model that runs on this PC. Download it once, then press {@render keycaps(hotkeyLabel)} in any app.</p>
              <button class="emptycta" onclick={() => onopensettings?.('models')}>Download a speech model</button>
            {:else}
              <h2>Ready when you are</h2>
              <p>Press {@render keycaps(hotkeyLabel)} in any app to dictate — your transcriptions appear here.</p>
            {/if}
          </div>
        {:else}
          {#each groups as g (g.label)}
            <div class="daycap">{g.label}</div>
            <div class="daylist">
              {#each g.items as e, i (`${rowKey(e)}:${i}`)}
                {@const edits = aiEdits.get(rowKey(e))}
                {@const editsOpen = !!edits && openDiff === rowKey(e)}
                {@const editsId = `ai-edits-${e.ts}-${i}`}
                <!-- Focusable for its menu from the keyboard (the ContextMenu key, Shift+F10). -->
                <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
                <div
                  class="item"
                  class:failed={e.status === 'failed' || e.status === 'notPasted'}
                  class:menuopen={menuOpenFor(rowEls[rowKey(e)])}
                  bind:this={rowEls[rowKey(e)]}
                  role="article"
                  tabindex="0"
                  aria-label={`Dictation at ${timeOf(e.ts)}`}
                  oncontextmenu={(ev) => rowMenu(ev, e)}
                  onkeydown={(ev) => ev.target === ev.currentTarget && menuKey(ev, (k) => rowMenu(k, e))}
                >
                  <span class="time">{timeOf(e.ts)}</span>
                  <div class="body">
                    {#if e.status === 'failed'}
                      <p class="text failedtext">Not transcribed{e.error ? ` · ${e.error}` : ''}</p>
                    {:else}
                      <p class="text">{e.text}</p>
                    {/if}
                    {#if edits}
                      <!-- What AI cleanup changed: the words as said against
                           the text pasted (lib/wordDiff.js). -->
                      <button class="aiedit" class:on={editsOpen} aria-expanded={editsOpen} aria-controls={editsId} onclick={() => toggleDiff(e)}>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" /></svg>
                        AI edited · {edits.changes}
                        {edits.changes === 1 ? 'change' : 'changes'}
                        <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
                      </button>
                      {#if editsOpen}
                        <div class="diff" id={editsId} role="region" aria-label="What AI cleanup changed">
                          <!-- One line in the source: the text keeps its own spaces and line breaks. -->
                          <p class="difftext">{#each edits.parts as p, k}{k ? p.sep : ''}{#if p.type === 'del'}<del><span class="sr">{'taken out: '}</span>{p.text}</del>{:else if p.type === 'add'}<ins><span class="sr">{'put in: '}</span>{p.text}</ins>{:else}{p.text}{/if}{/each}</p>
                          <p class="diffkey">
                            <span class="sample del">Struck through</span>: the AI took it out ·
                            <span class="sample ins">underlined</span>: the AI put it in
                          </p>
                          <div class="diffacts">
                            <button class="diffbtn primary" disabled={!!pastingSaid} onclick={() => pasteSaid(e)}>
                              {pastingSaid === rowKey(e) ? 'Pasting…' : 'Paste what I said'}
                            </button>
                            <button class="diffbtn" onclick={() => copyText(e.said, 'Copied what you said')}>Copy what I said</button>
                          </div>
                        </div>
                      {/if}
                    {/if}
                    {#if e.status}
                      <p class="failline">
                        {#if e.status === 'notPasted'}<span class="flag">Didn't paste</span><span class="why">{e.error}</span>{/if}
                        {#if e.audio}
                          <button class="retry" disabled={!!running} onclick={() => rerun(e)}>
                            {running === rowKey(e) ? 'Running…' : 'Run again'}
                          </button>
                        {/if}
                      </p>
                    {/if}
                    <p class="meta">
                      {#if e.app}{appName(e.app)} · {/if}{e.words}
                      {e.words === 1 ? 'word' : 'words'}{#if e.model}
                        · {modelName(e.model)}{/if}
                    </p>
                  </div>
                  <div class="actions">
                    {#if e.audio}
                      <button class="act" class:on={playing === rowKey(e)} title={playing === rowKey(e) ? 'Stop' : 'Play'} aria-label={playing === rowKey(e) ? 'Stop playing' : 'Play'} onclick={() => play(e)}>
                        {#if playing === rowKey(e)}
                          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
                        {:else}
                          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5.5v13l11-6.5z" /></svg>
                        {/if}
                      </button>
                      <button class="act" data-more title="Run again" aria-label="Run again" disabled={!!running} onclick={(ev) => rerunMenu(ev, e)}>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></svg>
                      </button>
                    {/if}
                    {#if e.text}
                    <button class="act" title={copiedTs === e.ts ? 'Copied!' : 'Copy'} aria-label="Copy" onclick={() => copy(e)}>
                      {#if copiedTs === e.ts}
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
                      {:else}
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
                      {/if}
                    </button>
                    {/if}
                    <button class="act danger" title="Delete" aria-label="Delete" onclick={() => askRemove(e)}>
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          {/each}
        {/if}
      </div>

      {#if stats && (stats.totalTranscriptions ?? 0) > 0}
        <aside class="rail">
          <div class="statcard">
            <div class="stat">
              <span class="n">{fmtNum(stats.totalWords)}</span>
              <span class="l">total words</span>
            </div>
            <div class="stat">
              <span class="n">{fmtNum(stats.today?.words)}</span>
              <span class="l">words today</span>
            </div>
            <div class="stat">
              <span class="n">{fmtTimeSaved(stats.timeSavedMinutes)}</span>
              <span class="l">time saved</span>
            </div>
            <div class="stat">
              <span class="n">{fmtNum(stats.streakDays)}</span>
              <span class="l">day streak</span>
            </div>
            <div class="statrule"></div>
            <p class="statfoot">
              {fmtNum(stats.totalTranscriptions)}
              {stats.totalTranscriptions === 1 ? 'dictation' : 'dictations'}, stored on this PC.
            </p>
          </div>
        </aside>
      {/if}
    </div>
  </div>
</div>

<style>
  .home {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .wrap {
    max-width: 980px;
    margin: 0 auto;
    padding: 34px 36px 48px;
    container-type: inline-size;
  }
  /* ---- greeting: the hotkey IS the interface, so it leads ---- */
  .hello {
    margin: 0 0 24px;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.012em;
    color: var(--yap-fg);
  }

  /* Keycaps — the signature motif. Amber, tactile, reserved for real
     hotkeys (the small neutral .hint kbd is for UI shortcuts). */
  .keys {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    margin: 0 2px;
  }
  .key {
    display: inline-block;
    padding: 1px 8px 2px;
    font-family: inherit;
    font-size: 0.88em;
    font-weight: 650;
    color: var(--yap-key-fg);
    background: var(--yap-key-bg);
    border: 1px solid var(--yap-key-border);
    border-bottom-width: 2.5px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .keyplus {
    color: var(--yap-muted-55);
    font-weight: 500;
  }

  .cols {
    display: flex;
    align-items: flex-start;
    gap: 26px;
  }
  .feed {
    flex: 1 1 auto;
    min-width: 0;
  }

  /* ---- hero: a dark feature card in both themes (ink on light, a raised
     warm card on dark), serif display headline (Wispr) ---- */
  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 26px 28px;
    margin-bottom: 26px;
    min-height: 76px;
    border: 1px solid var(--yap-feature-edge);
    border-radius: var(--yap-r-xl);
    background: var(--yap-feature);
    color: var(--yap-feature-fg);
  }
  .herotext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .hero h2 {
    margin: 0 0 7px;
    font-family: var(--yap-font-display);
    font-size: 24px;
    font-weight: 500;
    letter-spacing: 0.005em;
  }
  .hero h2 em {
    font-style: italic;
    font-weight: 500;
  }
  .hero p {
    margin: 0;
    font-size: 13px;
    line-height: 1.65;
    color: color-mix(in srgb, var(--yap-feature-fg) 72%, transparent);
  }
  .heroside {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 12px;
  }
  .herocta {
    padding: 9px 16px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-feature-cta);
    color: var(--yap-feature-cta-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .herocta:hover {
    background: var(--yap-feature-cta-hover);
  }
  .herodots {
    display: flex;
    gap: 6px;
  }
  .herodot {
    width: 6px;
    height: 6px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: color-mix(in srgb, var(--yap-feature-fg) 28%, transparent);
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .herodot:hover {
    background: color-mix(in srgb, var(--yap-feature-fg) 55%, transparent);
  }
  .herodot.on {
    background: var(--yap-key-bg);
  }

  /* ---- search: just an icon until opened (Wispr) ---- */
  .searchrow {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    min-height: 30px;
    margin-bottom: 2px;
  }
  .searchbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .searchbtn:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .searchbtn svg {
    width: 15px;
    height: 15px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: min(340px, 100%);
    padding: 6px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .search:focus-within {
    border-color: var(--yap-border-hover);
  }
  .search svg {
    width: 14px;
    height: 14px;
    color: var(--yap-muted-55);
    flex: 0 0 auto;
  }
  .search input {
    flex: 1 1 auto;
    min-width: 0;
    border: none;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
  }
  .search input:focus {
    outline: none;
  }
  .search input::placeholder {
    color: var(--yap-muted-55);
  }
  .searchx {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
  }
  .searchx:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .searchx svg {
    width: 11px;
    height: 11px;
  }

  .notice {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: baseline;
    margin-bottom: 18px;
    padding: 10px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    font-size: 12px;
    color: var(--yap-muted);
  }
  .link {
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }

  .daycap {
    margin: 22px 0 6px;
    padding: 0 10px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }

  /* ---- feed rows: flat list, hairline separators, hover wash.
     The meta line (app · words · model) only surfaces on hover — the
     transcript text is the row (Wispr). ---- */
  .daylist {
    display: flex;
    flex-direction: column;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 14px;
    padding: 13px 10px;
    border-radius: var(--yap-r);
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .daylist .item:last-child {
    border-bottom: none;
  }
  .item:hover,
  .item.menuopen {
    background: var(--yap-s3);
  }
  .item:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .item.menuopen .meta,
  .item.menuopen .actions {
    opacity: 1;
  }
  .time {
    flex: 0 0 56px;
    margin-top: 2px;
    font-size: 12px;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
  }
  .body {
    flex: 1 1 auto;
    min-width: 0;
  }
  .text {
    margin: 0;
    font-size: 14px;
    line-height: 1.6;
    color: var(--yap-fg);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .meta {
    margin: 3px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted-55);
    opacity: 0;
    transition: opacity var(--yap-dur) ease;
  }
  .item:hover .meta,
  .item:focus-within .meta {
    opacity: 1;
  }
  .actions {
    display: flex;
    gap: 2px;
    flex: 0 0 auto;
    opacity: 0;
    transition: opacity var(--yap-dur) ease;
  }
  .item:hover .actions,
  .item:focus-within .actions {
    opacity: 1;
  }
  /* A dictation that failed (never transcribed, or its paste didn't land):
     said in the row, with Run again at hand (recent_audio.rs). */
  .failedtext {
    color: var(--yap-muted);
    font-style: italic;
  }
  .failline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 5px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .flag {
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--yap-danger) 12%, transparent);
    color: var(--yap-danger);
    font-weight: 650;
  }
  .retry {
    padding: 2px 10px;
    border: 1px solid var(--yap-border);
    border-radius: 999px;
    background: var(--yap-s1, transparent);
    color: var(--yap-fg);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .retry:hover:not(:disabled) {
    background: var(--yap-s3);
  }
  .retry:disabled {
    opacity: 0.6;
    cursor: default;
  }
  /* What AI cleanup changed: a quiet pill under the text (always in view,
     unlike the hover meta), opening into the diff. Removed words struck in
     the danger colour, added ones underlined on an amber wash — never
     colour alone. */
  .aiedit {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin: 5px 0 0;
    padding: 1px 8px 1px 7px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 999px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    line-height: 1.7;
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .aiedit:hover,
  .aiedit.on {
    color: var(--yap-fg);
    border-color: var(--yap-border);
    background: var(--yap-s2);
  }
  .aiedit:focus-visible,
  .diffbtn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .aiedit svg {
    width: 11px;
    height: 11px;
    color: var(--yap-primary);
  }
  .aiedit .chev {
    width: 10px;
    height: 10px;
    color: currentColor;
    transition: transform var(--yap-dur) ease;
  }
  .aiedit.on .chev {
    transform: rotate(180deg);
  }
  .diff {
    margin: 8px 0 2px;
    padding: 10px 12px 11px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
  }
  .difftext {
    margin: 0;
    font-size: 14px;
    line-height: 1.75;
    color: var(--yap-fg);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .difftext del,
  .sample.del {
    color: var(--yap-danger);
    text-decoration: line-through;
    text-decoration-thickness: 1.5px;
    background: color-mix(in srgb, var(--yap-danger) 9%, transparent);
    border-radius: 3px;
    padding: 0 2px;
  }
  .difftext ins,
  .sample.ins {
    color: var(--yap-fg);
    text-decoration: underline;
    text-decoration-color: var(--yap-primary);
    text-decoration-thickness: 2px;
    text-underline-offset: 3px;
    background: var(--yap-primary-wash);
    border-radius: 3px;
    padding: 0 2px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .diffkey {
    margin: 7px 0 0;
    font-size: 11.5px;
    line-height: 1.6;
    color: var(--yap-muted);
  }
  .diffacts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 9px;
  }
  .diffbtn {
    padding: 3px 11px;
    border: 1px solid var(--yap-border);
    border-radius: 999px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .diffbtn:hover:not(:disabled) {
    background: var(--yap-s3);
  }
  .diffbtn.primary {
    border-color: transparent;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .diffbtn.primary:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .diffbtn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .act.on {
    color: var(--yap-fg);
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .act:hover {
    background: var(--yap-raised);
    color: var(--yap-fg);
  }
  .act.danger:hover {
    color: var(--yap-danger);
  }
  .act svg {
    width: 14px;
    height: 14px;
  }

  /* ---- right rail: the numbers get the serif ---- */
  .rail {
    flex: 0 0 232px;
    position: sticky;
    top: 0;
  }
  .statcard {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 22px 24px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .stat {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .stat .n {
    font-family: var(--yap-font-display);
    font-size: 31px;
    font-weight: 550;
    letter-spacing: -0.01em;
    color: var(--yap-fg);
    font-variant-numeric: oldstyle-nums;
  }
  /* Wispr-weight labels: full ink at medium, not washed-out grey. */
  .stat .l {
    font-size: 14px;
    font-weight: 550;
    color: var(--yap-fg);
  }
  .statrule {
    height: 1px;
    background: var(--yap-border-subtle);
  }
  .statfoot {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-muted-70);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 70px 20px;
    text-align: center;
    color: var(--yap-muted);
  }
  .empty svg {
    width: 36px;
    height: 36px;
    color: var(--yap-muted-55);
    margin-bottom: 6px;
  }
  .empty h2 {
    margin: 0;
    font-size: 16px;
    color: var(--yap-fg);
  }
  .empty p {
    margin: 0;
    max-width: 460px;
    font-size: 13.5px;
    line-height: 1.8;
  }
  .emptycta {
    margin-top: 10px;
    padding: 9px 16px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .emptycta:hover {
    background: var(--yap-ink-hover);
  }
  .emptycta:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }

  /* A narrow window (the 860 px minimum, a laptop at 150 %): the stats go
     above the feed as one row instead of squeezing the tip card into a
     column of one word per line. Last in the sheet: a container query adds
     no specificity, so these must come after the rules they change. */
  @container (max-width: 760px) {
    .cols {
      flex-direction: column;
      align-items: stretch;
      gap: 18px;
    }
    .rail {
      order: -1;
      position: static;
      flex: none;
    }
    .statcard {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: 4px 14px;
      padding: 14px 18px;
    }
    .statcard .stat {
      flex-wrap: wrap;
      gap: 0 6px;
    }
    .statcard .stat .n {
      font-size: 24px;
    }
    .statcard .stat .l {
      font-size: 12.5px;
    }
    .statrule,
    .statfoot {
      display: none;
    }
  }
  @container (max-width: 560px) {
    .hero {
      flex-direction: column;
      align-items: flex-start;
    }
    .heroside {
      flex-direction: row;
      align-items: center;
      gap: 16px;
    }
  }
</style>
