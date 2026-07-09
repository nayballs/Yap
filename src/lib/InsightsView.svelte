<script>
  // Insights — the stats dashboard promoted out of Settings → History into its
  // own surface (Wispr "Insights"): serif hero number, stat grid, the 30-day
  // activity heatmap, and where you dictate most (top apps from history).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  let stats = $state(null);
  let entries = $state([]);

  async function refresh() {
    try {
      stats = await invoke('get_stats');
    } catch {
      stats = null;
    }
    try {
      entries = (await invoke('get_history', { limit: 500 })) || [];
    } catch {
      entries = [];
    }
  }

  onMount(() => {
    refresh();
    let un;
    listen('yap-transcript', () => refresh()).then((u) => (un = u));
    return () => un && un();
  });

  function fmtNum(n) {
    return (n ?? 0).toLocaleString();
  }
  function fmtMinutes(min) {
    const v = Number(min) || 0;
    const m = Math.round(v);
    if (m < 1) return v > 0 ? '<1 min' : '0 min';
    if (m < 60) return `${m} min`;
    const h = Math.floor(m / 60);
    return `${h} h ${m % 60} min`;
  }
  function activityLevel(words, max) {
    if (!words) return 0;
    const t = words / Math.max(1, max);
    return t > 0.75 ? 4 : t > 0.5 ? 3 : t > 0.25 ? 2 : 1;
  }
  // Day-numbers are UTC (unix secs / 86400, same trick as the backend).
  function activityDayLabel(day) {
    return new Date((day || 0) * 86400000).toLocaleDateString(undefined, {
      timeZone: 'UTC',
      weekday: 'short',
      month: 'short',
      day: 'numeric',
    });
  }

  const avgWords = $derived.by(() => {
    const t = stats?.totalTranscriptions ?? 0;
    if (!t) return 0;
    return Math.round((stats?.totalWords ?? 0) / t);
  });

  // Where you dictate most: words summed per focused app, top 5.
  const topApps = $derived.by(() => {
    const byApp = new Map();
    for (const e of entries) {
      const app = (e.app || '').replace(/\.exe$/i, '');
      if (!app) continue;
      byApp.set(app, (byApp.get(app) || 0) + (e.words || 0));
    }
    return [...byApp.entries()]
      .map(([app, words]) => ({ app, words }))
      .sort((a, b) => b.words - a.words)
      .slice(0, 5);
  });
</script>

<div class="insights">
  <div class="wrap">
    <div class="head">
      <h1>Insights</h1>
      <p>How you dictate — computed from your local history, nothing leaves this PC.</p>
    </div>

    {#if stats && (stats.totalTranscriptions ?? 0) > 0}
      <div class="heroCard">
        <div class="heroNum">{fmtNum(stats.totalWords)}</div>
        <div class="heroLbl">
          words dictated all-time — about <strong>{fmtMinutes(stats.timeSavedMinutes)}</strong>
          saved vs typing (you speak ~150 wpm, most people type ~40)
        </div>
      </div>

      <div class="grid">
        <div class="card">
          <div class="num">{fmtNum(stats.today?.words)}</div>
          <div class="lbl">words today</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(stats.streakDays)}</div>
          <div class="lbl">day streak</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(stats.totalTranscriptions)}</div>
          <div class="lbl">dictations</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(avgWords)}</div>
          <div class="lbl">words per dictation</div>
        </div>
      </div>

      {#if stats.activity?.length}
        {@const maxW = Math.max(1, ...stats.activity.map((d) => d.words))}
        {@const todayDay = stats.activity[stats.activity.length - 1]?.day}
        <div class="section">
          <h2>Last 30 days</h2>
          <div class="activity">
            {#each stats.activity as d}
              <span
                class="acell"
                class:today={d.day === todayDay}
                data-level={activityLevel(d.words, maxW)}
                title="{activityDayLabel(d.day)} · {d.words} {d.words === 1 ? 'word' : 'words'}"
              ></span>
            {/each}
          </div>
          <div class="legend">
            <span>One cell per day</span>
            <span class="scale">
              Less
              <span class="acell" data-level="0"></span>
              <span class="acell" data-level="1"></span>
              <span class="acell" data-level="2"></span>
              <span class="acell" data-level="3"></span>
              <span class="acell" data-level="4"></span>
              More
            </span>
          </div>
        </div>
      {/if}

      {#if topApps.length}
        {@const maxApp = topApps[0].words || 1}
        <div class="section">
          <h2>Where you dictate</h2>
          <div class="apps">
            {#each topApps as a (a.app)}
              <div class="approw">
                <span class="appname">{a.app}</span>
                <span class="appbar"><span style="width:{Math.max(3, Math.round((a.words / maxApp) * 100))}%"></span></span>
                <span class="appwords">{fmtNum(a.words)} {a.words === 1 ? 'word' : 'words'}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    {:else}
      <div class="empty">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20V10" /><path d="M10 20V4" /><path d="M16 20v-7" /><path d="M22 20H2" /></svg>
        <h2>No insights yet</h2>
        <p>Dictate a few things and your stats will show up here.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .insights {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .wrap {
    max-width: 760px;
    margin: 0 auto;
    padding: 34px 36px 48px;
  }

  .head {
    margin-bottom: 24px;
  }
  .head h1 {
    margin: 0 0 5px;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.012em;
  }
  .head p {
    margin: 0;
    font-size: 13px;
    color: var(--yap-muted);
  }

  .heroCard {
    padding: 26px 28px;
    margin-bottom: 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .heroNum {
    font-family: var(--yap-font-display);
    font-size: 50px;
    font-weight: 550;
    line-height: 1.1;
    letter-spacing: -0.015em;
    font-variant-numeric: oldstyle-nums;
  }
  .heroLbl {
    margin-top: 6px;
    font-size: 13px;
    line-height: 1.6;
    color: var(--yap-muted);
    max-width: 52ch;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-bottom: 26px;
  }
  .card {
    padding: 16px 18px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .num {
    font-family: var(--yap-font-display);
    font-size: 27px;
    font-weight: 550;
    font-variant-numeric: oldstyle-nums;
  }
  .lbl {
    margin-top: 2px;
    font-size: 12px;
    color: var(--yap-muted);
  }

  .section {
    margin-bottom: 26px;
  }
  .section h2 {
    margin: 0 0 10px;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--yap-muted-55);
  }

  .activity {
    display: flex;
    gap: 4px;
  }
  .acell {
    flex: 1 1 0;
    max-width: 18px;
    aspect-ratio: 1 / 1;
    border-radius: 4px;
    background: var(--yap-raised);
  }
  .acell[data-level='1'] {
    background: color-mix(in srgb, var(--yap-primary) 25%, var(--yap-raised));
  }
  .acell[data-level='2'] {
    background: color-mix(in srgb, var(--yap-primary) 55%, var(--yap-raised));
  }
  .acell[data-level='3'] {
    background: color-mix(in srgb, var(--yap-primary) 80%, var(--yap-raised));
  }
  .acell[data-level='4'] {
    background: var(--yap-primary);
  }
  .acell.today {
    box-shadow: 0 0 0 1.5px var(--yap-primary-line);
  }
  .legend {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 8px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .legend .scale {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .legend .acell {
    width: 11px;
    max-width: 11px;
    flex: 0 0 auto;
  }

  .apps {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .approw {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .appname {
    flex: 0 0 130px;
    font-size: 12.5px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .appbar {
    flex: 1 1 auto;
    height: 8px;
    border-radius: 4px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .appbar span {
    display: block;
    height: 100%;
    border-radius: 4px;
    background: var(--yap-primary);
  }
  .appwords {
    flex: 0 0 90px;
    text-align: right;
    font-size: 11.5px;
    color: var(--yap-muted-55);
    font-variant-numeric: tabular-nums;
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
    font-size: 16px;
    color: var(--yap-fg);
  }
  .empty p {
    margin: 0;
    font-size: 13.5px;
  }
</style>
