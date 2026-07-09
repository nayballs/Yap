<script>
  // Home — the dictation feed, Wispr-Flow-style (2026-07-08 restyle): a
  // time-of-day greeting with the hotkey rendered as keycaps, a dark hero
  // card teaching voice editing, the day-grouped history feed with hover
  // actions, and a right-rail stats card with serif display numerals.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import { formatHotkeySpec } from './hotkeys.js';

  let { onopensettings = null, onnavigate = null } = $props();

  let entries = $state([]);
  let stats = $state(null);
  let historyEnabled = $state(true);
  let hotkeyLabel = $state('F9');
  let editHotkeyLabel = $state('');
  let cleanupEnabled = $state(false);
  let copiedTs = $state(null);

  // Rotating hero tips: a different card each day (dots to browse). Each tip
  // teaches one feature that already exists — the hero is onboarding forever.
  const TIP_COUNT = 4;
  let tipIndex = $state(Math.floor(Date.now() / 86_400_000) % TIP_COUNT);
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

  async function refresh() {
    try {
      entries = (await invoke('get_history', { limit: 200 })) || [];
    } catch {
      entries = [];
    }
    try {
      stats = await invoke('get_stats');
    } catch {
      stats = null;
    }
  }

  onMount(() => {
    refresh();
    invoke('get_config')
      .then((cfg) => {
        historyEnabled = cfg?.historyEnabled !== false;
        cleanupEnabled = cfg?.postProcessEnabled === true;
        if (cfg?.hotkey) hotkeyLabel = formatHotkeySpec(cfg.hotkey);
        if (cfg?.editHotkey) editHotkeyLabel = formatHotkeySpec(cfg.editHotkey);
      })
      .catch(() => {});
    // New dictation finished → it's already in history; re-pull.
    let un;
    listen('yap-transcript', () => refresh()).then((u) => (un = u));
    return () => un && un();
  });

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

  function appName(app) {
    return (app || '').replace(/\.exe$/i, '');
  }

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
    } catch {
      /* ignore */
    }
    refresh();
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

<div class="home">
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
                  hold your edit key,
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
            {:else}
              <h2>A different style for <em>every app</em></h2>
              <p>
                Formal in email, casual in Slack, precise in your editor — per-app profiles switch the cleanup style automatically.
              </p>
            {/if}
          </div>
          <div class="heroside">
            {#if tipIndex === 0}
              <button class="herocta" onclick={() => onopensettings?.('cleanup')}>
                {editHotkeyLabel ? 'Tune the voice agent' : 'Set up voice editing'}
              </button>
            {:else if tipIndex === 1}
              <button class="herocta" onclick={() => onopensettings?.('cleanup')}>
                {cleanupEnabled ? 'Open Prompt Studio' : 'Turn on AI cleanup'}
              </button>
            {:else if tipIndex === 2}
              <button class="herocta" onclick={() => onnavigate?.('notes')}>Open Notes</button>
            {:else}
              <button class="herocta" onclick={() => onopensettings?.('cleanup')}>Set up profiles</button>
            {/if}
            <div class="herodots" role="tablist" aria-label="Tips">
              {#each Array(TIP_COUNT) as _, i}
                <button
                  class="herodot"
                  class:on={i === tipIndex}
                  role="tab"
                  aria-selected={i === tipIndex}
                  aria-label={`Tip ${i + 1}`}
                  onclick={() => (tipIndex = i)}
                ></button>
              {/each}
            </div>
          </div>
        </div>

        {#if !historyEnabled}
          <div class="notice">
            History is turned off, so new dictations won't appear here.
            <button class="link" onclick={() => onopensettings?.('history')}>Turn it on in Settings → History</button>
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
            <h2>Ready when you are</h2>
            <p>Press {@render keycaps(hotkeyLabel)} in any app to dictate — your transcriptions appear here.</p>
          </div>
        {:else}
          {#each groups as g (g.label)}
            <div class="daycap">{g.label}</div>
            <div class="daylist">
              {#each g.items as e (e.ts + e.text)}
                <div class="item">
                  <span class="time">{timeOf(e.ts)}</span>
                  <div class="body">
                    <p class="text">{e.text}</p>
                    <p class="meta">
                      {#if e.app}{appName(e.app)} · {/if}{e.words}
                      {e.words === 1 ? 'word' : 'words'}{#if e.model}
                        · {e.model}{/if}
                    </p>
                  </div>
                  <div class="actions">
                    <button class="act" title={copiedTs === e.ts ? 'Copied!' : 'Copy'} aria-label="Copy" onclick={() => copy(e)}>
                      {#if copiedTs === e.ts}
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
                      {:else}
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
                      {/if}
                    </button>
                    <button class="act danger" title="Delete" aria-label="Delete" onclick={() => remove(e)}>
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
            <p class="statfoot">{fmtNum(stats.totalTranscriptions)} dictations, stored on this PC.</p>
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

  /* ---- hero: dark ink card, serif display headline (Wispr) ---- */
  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 26px 28px;
    margin-bottom: 26px;
    min-height: 76px;
    border-radius: var(--yap-r-xl);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
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
    color: rgba(247, 245, 240, 0.72);
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
    background: var(--yap-s2);
    color: var(--yap-ink);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .herocta:hover {
    background: #f1eee6;
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
    background: rgba(247, 245, 240, 0.28);
    cursor: pointer;
    transition: background var(--yap-dur) ease;
  }
  .herodot:hover {
    background: rgba(247, 245, 240, 0.55);
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
  .item:hover {
    background: var(--yap-s3);
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
    font-size: 13.5px;
    line-height: 1.8;
  }
</style>
