<script>
  // Insights — the stats dashboard (Wispr "Insights"), counted on this PC in
  // dictation_stats.json (dictation/stats.rs): numbers only, kept when history
  // is cleared, kept 24 hours or turned off. The serif hero number with the
  // time saved and how it's worked out (this person's own words a minute
  // once Yap has timed a few dictations), a stat grid (today, speed, the day
  // and week streaks…), the fixes (AI cleanup's, the dictionary's, this
  // month against the same days of last month), the 30-day activity and
  // where you dictate most; Share makes a picture of the numbers
  // (shareCard.js). Then AI usage (AiUsageCard: every AI call's tokens,
  // by model and feature, counted on this PC); `focus` 'ai-usage' brings it
  // into view (Chat's context ring's "See all").
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { appLabel } from '../../core/appNames.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import AiUsageCard from './AiUsageCard.svelte';
  import TypingCard from './TypingCard.svelte';
  import { statsCardPng } from './shareCard.js';

  let { focus = null, onfocused = null } = $props();

  let stats = $state(null);
  // The stats are in (the AI usage card comes into view after them).
  let loaded = $state(false);

  async function refresh() {
    try {
      stats = await invoke('get_stats');
    } catch {
      stats = null;
    }
    loaded = true;
  }

  onMount(() => {
    refresh();
    // A dictation counted (`yap-stats-changed`, history kept or not), or
    // history changed: re-pull, once for the two.
    let queued = false;
    const soon = () => {
      if (queued) return;
      queued = true;
      setTimeout(() => {
        queued = false;
        refresh();
      }, 60);
    };
    const uns = [];
    listen('yap-stats-changed', soon).then((u) => uns.push(u));
    listen('yap-history-changed', soon).then((u) => uns.push(u));
    return () => uns.forEach((u) => u && u());
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
  /** A day as the stats write it ("2026-10-29"), on the PC's calendar. */
  const dayOf = (date) => new Date(`${date}T12:00:00`);
  function activityDayLabel(date) {
    return dayOf(date).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' });
  }

  const avgWords = $derived.by(() => {
    const t = stats?.totalTranscriptions ?? 0;
    if (!t) return 0;
    return Math.round((stats?.totalWords ?? 0) / t);
  });

  // Since when: "October 2026".
  const since = $derived(stats?.since ? dayOf(stats.since).toLocaleDateString(undefined, { month: 'long', year: 'numeric' }) : '');

  // How "time saved" is worked out, in a sentence.
  const how = $derived.by(() => {
    const t = stats?.timeSaved;
    if (!t) return '';
    if (t.measured) {
      const n = stats.timedDictations ?? 0;
      return `You talk at ${fmtNum(t.speakingWpm)} words a minute, timed over ${fmtNum(n)} ${n === 1 ? 'dictation' : 'dictations'}; most people type about ${t.typingWpm}.`;
    }
    return `Worked out at ${t.speakingWpm} words a minute for talking and ${t.typingWpm} for typing, until Yap has timed a few of your dictations.`;
  });

  // This month's fixes against the same days of last month.
  const change = $derived.by(() => {
    const p = stats?.fixes?.changePct;
    if (p == null) return 'nothing to compare yet';
    if (p === 0) return 'the same as this time last month';
    return `${p > 0 ? 'up' : 'down'} ${Math.abs(p)}% on this time last month`;
  });

  // ---- Share: the numbers as a picture (shareCard.js; dictation/stats/share.rs) ----
  let sharing = $state(false);
  async function shareAs(how) {
    if (sharing) return;
    sharing = true;
    try {
      const png = await statsCardPng(stats);
      if (how === 'copy') {
        await invoke('insights_card_copy', png);
        toast({ title: 'Copied your stats as a picture', description: 'Numbers only. Paste it anywhere.', variant: 'success', chip: 'Insights' });
      } else if (await invoke('insights_card_save', png)) {
        toast({ title: 'Saved your stats as a picture', variant: 'success', chip: 'Insights' });
      }
    } catch (err) {
      toast({ title: "Couldn't make the picture", description: String(err), variant: 'destructive' });
    } finally {
      sharing = false;
    }
  }
  function shareMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Copy as a picture', icon: 'copy', onselect: () => shareAs('copy') },
        { label: 'Save as a picture…', icon: 'download', onselect: () => shareAs('save') },
      ],
      { label: 'Share' }
    );
  }

  // Where you dictate most: words per app (by the app's own name:
  // "ms-teams.exe" and "Teams.exe" are one Teams), top 5.
  const topApps = $derived.by(() => {
    const byApp = new Map();
    for (const a of stats?.topApps || []) {
      const app = appLabel(a.app);
      if (!app) continue;
      byApp.set(app, (byApp.get(app) || 0) + (a.words || 0));
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
      <div class="titlerow">
        <h1>Insights</h1>
        {#if stats && (stats.totalTranscriptions ?? 0) > 0}
          <!-- Your numbers as a picture: never a word you said. -->
          <button class="share" data-more disabled={sharing} onclick={shareMenu} aria-haspopup="menu" title="Share your stats as a picture">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12v7a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7" /><path d="M16 6l-4-4-4 4" /><path d="M12 2v13" /></svg>
            {sharing ? 'Making it…' : 'Share'}
          </button>
        {/if}
      </div>
      <p>How you dictate, counted on this PC. Numbers only, never what you said; they stay when you clear your history.</p>
    </div>

    {#if stats && (stats.totalTranscriptions ?? 0) > 0}
      <div class="heroCard">
        <div class="heroNum">{fmtNum(stats.totalWords)}</div>
        <div class="heroLbl">
          words dictated{since ? ` since ${since}` : ''}, about <strong>{fmtMinutes(stats.timeSaved?.minutes ?? stats.timeSavedMinutes)}</strong>
          saved compared with typing.
        </div>
        <p class="how">{how}</p>
      </div>

      <div class="grid">
        <div class="card">
          <div class="num">{fmtNum(stats.today?.words)}</div>
          <div class="lbl">words today</div>
        </div>
        <div class="card" title={stats.wpm == null ? 'Measured once Yap has timed a few of your dictations' : 'Over the time you spent talking'}>
          <div class="num">{stats.wpm == null ? '–' : fmtNum(stats.wpm)}</div>
          <div class="lbl">words a minute</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(stats.streakDays)}</div>
          <div class="lbl">day streak</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(stats.weekStreak)}</div>
          <div class="lbl">week streak · longest {fmtNum(stats.longestWeekStreak)}</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(stats.totalTranscriptions)}</div>
          <div class="lbl">{stats.totalTranscriptions === 1 ? 'dictation' : 'dictations'}</div>
        </div>
        <div class="card">
          <div class="num">{fmtNum(avgWords)}</div>
          <div class="lbl">words per dictation</div>
        </div>
      </div>

      {#if stats.fixes}
        <div class="section">
          <h2>Fixes</h2>
          <div class="grid">
            <div class="card">
              <div class="num">{fmtNum(stats.fixes.cleanupWords)}</div>
              <div class="lbl">words AI cleanup changed</div>
            </div>
            <div class="card">
              <div class="num">{fmtNum(stats.fixes.dictionaryFixes)}</div>
              <div class="lbl">words your dictionary fixed</div>
            </div>
            <div class="card">
              <div class="num">{fmtNum(stats.fixes.thisMonth)}</div>
              <div class="lbl">fixes this month, {change}</div>
            </div>
          </div>
        </div>
      {/if}

      {#if stats.activity?.length}
        {@const maxW = Math.max(1, ...stats.activity.map((d) => d.words))}
        {@const todayDate = stats.activity[stats.activity.length - 1]?.date}
        <div class="section">
          <h2>Last 30 days</h2>
          <div class="activity">
            {#each stats.activity as d (d.date)}
              <span
                class="acell"
                class:today={d.date === todayDate}
                data-level={activityLevel(d.words, maxW)}
                title="{activityDayLabel(d.date)} · {d.words} {d.words === 1 ? 'word' : 'words'}"
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

    <!-- Your typing speed against your speaking speed (the typing test). -->
    <TypingCard />

    <!-- AI usage: shown once any AI call has been counted (or asked for). -->
    <AiUsageCard {focus} {onfocused} ready={loaded} />
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
  /* The serif display title, as Meetings and Settings have it. */
  .head h1 {
    margin: 0 0 6px;
    font-family: var(--yap-font-display);
    font-size: 28px;
    font-weight: 550;
    letter-spacing: -0.005em;
    line-height: 1.2;
  }
  .head p {
    margin: 0;
    font-size: 13px;
    color: var(--yap-muted);
  }
  .titlerow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .share {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .share:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .share:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .share svg {
    width: 15px;
    height: 15px;
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
  .how {
    margin: 8px 0 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-muted-70);
    max-width: 60ch;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    margin-bottom: 26px;
  }
  .section .grid {
    margin-bottom: 0;
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
