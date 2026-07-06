<script>
  // Home — the dictation feed (ported from OpenWhispr's HistoryView on the
  // control panel's Home tab): day-grouped transcriptions with per-item
  // copy/delete, a stats strip, and live refresh as new dictations land.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { formatHotkeySpec } from './hotkeys.js';

  let { onopensettings = null } = $props();

  let entries = $state([]);
  let stats = $state(null);
  let historyEnabled = $state(true);
  let hotkeyLabel = $state('the hotkey');
  let copiedTs = $state(null);

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
        if (cfg?.hotkey) hotkeyLabel = formatHotkeySpec(cfg.hotkey);
      })
      .catch(() => {});
    // New dictation finished → it's already in history; re-pull.
    let un;
    listen('yap-transcript', () => refresh()).then((u) => (un = u));
    return () => un && un();
  });

  // Group entries (newest first) into local-day buckets with friendly labels.
  const groups = $derived.by(() => {
    const out = [];
    let currentKey = null;
    let bucket = null;
    for (const e of entries) {
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
</script>

<div class="home">
  <div class="feed">
    {#if stats && (stats.totalTranscriptions ?? 0) > 0}
      <div class="statsrow">
        <div class="stat">
          <span class="n">{stats.today?.words ?? 0}</span>
          <span class="l">words today</span>
        </div>
        <div class="stat">
          <span class="n">{stats.totalWords ?? 0}</span>
          <span class="l">words all-time</span>
        </div>
        <div class="stat">
          <span class="n">{fmtTimeSaved(stats.timeSavedMinutes)}</span>
          <span class="l">time saved</span>
        </div>
        <div class="stat">
          <span class="n">{stats.streakDays ?? 0}</span>
          <span class="l">day streak</span>
        </div>
      </div>
    {/if}

    {#if !historyEnabled}
      <div class="notice">
        History is turned off, so new dictations won't appear here.
        <button class="link" onclick={() => onopensettings?.('history')}>Turn it on in Settings → History</button>
      </div>
    {/if}

    {#if groups.length === 0}
      <div class="empty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="9" y="2" width="6" height="12" rx="3" />
          <path d="M5 10v1a7 7 0 0 0 14 0v-1" />
          <path d="M12 18v4M8 22h8" />
        </svg>
        <h2>Ready when you are</h2>
        <p>Press <strong>{hotkeyLabel}</strong> in any app to dictate — your transcriptions appear here.</p>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="daycap">{g.label}</div>
        {#each g.items as e (e.ts + e.text)}
          <div class="item">
            <span class="time">{timeOf(e.ts)}</span>
            <div class="body">
              <p class="text">{e.text}</p>
              <p class="meta">
                {#if e.app}{appName(e.app)} · {/if}{e.words} words{#if e.model}
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
      {/each}
    {/if}
  </div>
</div>

<style>
  .home {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .feed {
    max-width: 780px;
    margin: 0 auto;
    padding: 26px 30px 40px;
  }

  .statsrow {
    display: flex;
    gap: 10px;
    margin-bottom: 22px;
  }
  .stat {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .stat .n {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .stat .l {
    font-size: 10.5px;
    color: var(--yap-muted-55);
    text-transform: uppercase;
    letter-spacing: 0.05em;
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
    margin: 18px 0 8px;
    font-size: 10.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }
  .daycap:first-child {
    margin-top: 0;
  }

  .item {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    margin-bottom: 8px;
  }
  .item:hover {
    border-color: var(--yap-border);
  }
  .time {
    flex: 0 0 auto;
    margin-top: 1px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .body {
    flex: 1 1 auto;
    min-width: 0;
  }
  .text {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-fg);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .meta {
    margin: 3px 0 0;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
  .actions {
    display: flex;
    gap: 2px;
    flex: 0 0 auto;
    opacity: 0;
    transition: opacity var(--yap-dur) ease;
  }
  .item:hover .actions {
    opacity: 1;
  }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
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
    background: var(--yap-s1);
    color: var(--yap-fg);
  }
  .act.danger:hover {
    color: #ef4444;
  }
  .act svg {
    width: 13px;
    height: 13px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 80px 20px;
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
    font-size: 15px;
    color: var(--yap-fg);
  }
  .empty p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.6;
  }
</style>
