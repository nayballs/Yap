<script>
  // The Yap bar (the `overlay` window; behaviour in src-tauri/src/bar.rs).
  // One fixed-size transparent window, bottom-centre on the monitor with the
  // cursor; everything is drawn inside it, to the measurements of Wispr
  // Flow's Flow Bar (E:\Projects\references\wispr-flow\flowbar-spec.md):
  //   idle      a 40 × 8 pill outline, its bottom 14 px above the work area,
  //             in a 60 × 20 hit wrapper;
  //   hover     it opens into 🎤 Dictate, ◉ Meeting notes and a ^ menu, with
  //             a tooltip naming each button's shortcut;
  //   dictating the dictation overlay (waveform, live text, Transcribing…,
  //             the error) in the pill's place;
  //   meeting   a 69 × 30 recording pill at half opacity (5 bars, the stop
  //             circle) that shows its timer when hovered;
  //   cards     26 px above the pill: Yap's notices while its window isn't
  //             focused, the first nearest the pill.
  // The window is click-through except over what this page reports as
  // interactive (`data-region`: the pill's hover zone, cards, the menu):
  // Rust watches the cursor against those rects and makes the window
  // clickable only there, and tells us which one the pointer is on
  // (`yap-bar-pointer`) — the authority on hover, since a click-through
  // window gets no mouseleave. With the bar off this is just the dictation
  // overlay, shown only while dictating (as before the bar).
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { flip } from 'svelte/animate';
  import { cubicOut } from 'svelte/easing';
  import { formatHotkeySpec } from './hotkeys.js';
  import { dictation, initDictation } from './bar/dictation.svelte.js';
  import DictationCapsule from './bar/DictationCapsule.svelte';
  import BarCard from './bar/BarCard.svelte';
  import CallCard from './bar/CallCard.svelte';

  initDictation();

  let bar = $state({
    enabled: true,
    shown: false,
    edge: 'bottom',
    // 'none' | 'borderless' | 'exclusive': over a fullscreen app the idle
    // pill hides (a card can still show over a borderless one).
    fullscreen: 'none',
    hotkey: '',
    meetingHotkey: null,
    meeting: { recording: false },
    call: null,
    cards: [],
  });
  /** The region under the pointer, as Rust sees it ('pill' | 'menu' | 'card:…' | null). */
  let pointer = $state(null);
  /** The DOM's view while the window is clickable (Rust's `pointer` wins on leave). */
  let domHover = $state(false);
  let expanded = $state(false);
  let menuOpen = $state(false);
  /** The hovered button's tooltip: 'dictate' | 'notes' | 'open' | 'stop' | null. */
  let tip = $state(null);
  let popping = $state(false);

  const busy = $derived(
    dictation.state === 'recording' ||
      dictation.state === 'processing' ||
      dictation.state === 'processing-slow' ||
      dictation.state === 'error'
  );
  const mode = $derived(
    busy
      ? 'dictation'
      : !bar.enabled
        ? 'off'
        : bar.meeting?.recording
          ? 'meeting'
          : bar.fullscreen !== 'none'
            ? 'cards-only'
            : 'idle'
  );
  const hovering = $derived(pointer === 'pill' || pointer === 'menu' || domHover);
  // At most two cards: the first stays nearest the pill and a second one
  // goes above it (Wispr's stacking), so a card being read never jumps.
  const cards = $derived(bar.enabled ? bar.cards.slice(-2) : []);
  // The pill's place, measured: cards sit 26 px above the pill whatever it
  // shows (the stage's 8 px + the dock + 20 px), gliding when it grows.
  let dockH = $state(20);

  // Open on hover; close a moment after the pointer leaves, so moving
  // between the pill and its menu doesn't snap it shut.
  let collapseTimer = null;
  $effect(() => {
    const want = mode === 'idle' && (hovering || menuOpen);
    clearTimeout(collapseTimer);
    if (want) {
      expanded = true;
    } else if (untrack(() => expanded)) {
      collapseTimer = setTimeout(() => {
        expanded = false;
        tip = null;
      }, 260);
    }
  });
  // The menu closes once the pointer has left the bar for a moment.
  let menuTimer = null;
  $effect(() => {
    clearTimeout(menuTimer);
    if (menuOpen && pointer === null && !domHover) {
      menuTimer = setTimeout(() => (menuOpen = false), 600);
    }
  });
  $effect(() => {
    if (mode !== 'idle') menuOpen = false;
  });

  // ---- the meeting timer ----
  // Started from the recording's elapsed time once per recording (later
  // snapshots carry whole seconds, which would only make it stutter).
  let now = $state(Date.now());
  let meetingStart = $state(0);
  let timedNote = null;
  $effect(() => {
    const m = bar.meeting;
    if (!m?.recording) {
      timedNote = null;
    } else if (m.noteId !== timedNote) {
      timedNote = m.noteId;
      meetingStart = Date.now() - (m.elapsedSecs ?? 0) * 1000;
      now = Date.now();
    }
  });
  $effect(() => {
    if (mode !== 'meeting') return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(t);
  });
  const elapsed = $derived.by(() => {
    const s = Math.max(0, Math.floor((now - meetingStart) / 1000));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, '0');
    return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
  });

  // ---- tooltips ----
  const keys = (spec) => {
    const text = spec ? formatHotkeySpec(spec) : '';
    return text && text !== 'None' ? text : '';
  };
  const tips = $derived({
    dictate: { label: 'Dictate', keys: keys(bar.hotkey) },
    notes: { label: bar.call ?? 'New note', keys: keys(bar.meetingHotkey) },
    open: { label: 'Open notes', keys: '' },
    stop: { label: 'Stop and summarise', keys: keys(bar.meetingHotkey) },
  });
  /** Where the tooltip points: the hovered button's centre, in the hover zone. */
  let tipX = $state(0);
  let tipTimer = null;
  function hoverTip(which, e) {
    clearTimeout(tipTimer);
    if (!which) {
      tip = null;
      return;
    }
    const btn = e?.currentTarget;
    const zone = btn?.closest?.('.hit');
    if (btn && zone) {
      const b = btn.getBoundingClientRect();
      tipX = b.left + b.width / 2 - zone.getBoundingClientRect().left;
    }
    tipTimer = setTimeout(() => (tip = which), tip ? 0 : 220);
  }

  // ---- actions ----
  // Every button goes through `bar_action`: Rust gives the focus back to the
  // app you're in first, then dictates, starts or stops meeting notes (as
  // the meeting shortcut does, meeting_guard.rs; a stop ends the meeting in
  // meeting_end.rs, which writes the action plan), or opens the notepad.
  async function act(action) {
    tip = null;
    try {
      await invoke('bar_action', { action });
    } catch (e) {
      invoke('frontend_log', { msg: `bar: ${action} failed: ${e}` }).catch(() => {});
    }
  }
  const dictate = () => act('dictate');
  const meetingNotes = () => act('meeting');
  const stopMeeting = () => act('stop-meeting');
  const openMeeting = () => act('open-note');
  function menuItem(action) {
    menuOpen = false;
    act(action);
  }
  function cardAction(card, action) {
    invoke('bar_card_action', { id: card.id, action }).catch((e) =>
      invoke('frontend_log', { msg: `bar: card ${card.id} ${action} failed: ${e}` }).catch(() => {})
    );
  }

  // ---- regions → Rust ----
  // Everything marked `data-region` takes the pointer; Rust makes the window
  // clickable only over these rects. Re-sent on every layout change.
  let lastSent = '';
  let raf = 0;
  function report() {
    raf = 0;
    const regions = [...document.querySelectorAll('[data-region]')]
      .map((el) => {
        const r = el.getBoundingClientRect();
        return { id: el.dataset.region, x: r.left, y: r.top, w: r.width, h: r.height };
      })
      .filter((r) => r.w > 0 && r.h > 0);
    const json = JSON.stringify(regions);
    if (json === lastSent) return;
    lastSent = json;
    invoke('bar_regions', { regions }).catch(() => {});
  }
  function scheduleReport() {
    if (!raf) raf = requestAnimationFrame(report);
  }
  // The pointer moved onto a transparent part while the window was
  // clickable: click-through again at once (not at Rust's next poll).
  let leftSent = false;
  function onStageMove(e) {
    if (e.target.closest?.('[data-region]')) {
      leftSent = false;
      return;
    }
    domHover = false;
    if (!leftSent) {
      leftSent = true;
      invoke('bar_pointer_left').catch(() => {});
    }
  }

  onMount(() => {
    const uns = [];
    invoke('bar_status')
      .then((s) => Object.assign(bar, s))
      .catch(() => {});
    listen('yap-bar', (e) => Object.assign(bar, e.payload)).then((u) => uns.push(u));
    listen('yap-bar-pointer', (e) => {
      pointer = e.payload?.region ?? null;
      if (pointer === null) {
        // Click-through again: no mouseleave will come, so drop DOM hover.
        domHover = false;
        hoverTip(null);
      }
      leftSent = false;
    }).then((u) => uns.push(u));
    listen('yap-bar-moved', () => {
      popping = false;
      requestAnimationFrame(() => (popping = true));
      setTimeout(() => (popping = false), 260);
    }).then((u) => uns.push(u));
    const mo = new MutationObserver(scheduleReport);
    mo.observe(document.body, { subtree: true, childList: true, attributes: true });
    document.addEventListener('transitionend', scheduleReport);
    document.addEventListener('animationend', scheduleReport);
    window.addEventListener('resize', scheduleReport);
    scheduleReport();
    return () => {
      uns.forEach((u) => u && u());
      mo.disconnect();
      document.removeEventListener('transitionend', scheduleReport);
      document.removeEventListener('animationend', scheduleReport);
      window.removeEventListener('resize', scheduleReport);
    };
  });
</script>


<div class="stage" class:top={bar.edge === 'top'} class:pop={popping} onpointermove={onStageMove} role="presentation">
  <div class="cards" style={`${bar.edge === 'top' ? 'top' : 'bottom'}:${8 + Math.max(dockH, 20) + 20}px`}>
    {#each cards as card (card.id)}
      <div
        class="cardwrap"
        animate:flip={{ duration: 160, easing: cubicOut }}
        in:fade={{ duration: 120 }}
        out:fade={{ duration: 120 }}
      >
        {#if card.style === 'call'}
          <CallCard
            {card}
            hovered={pointer === `card:${card.id}`}
            paused={pointer === `card:${card.id}` || !bar.shown}
            onaction={(a) => cardAction(card, a)}
          />
        {:else}
          <BarCard {card} paused={pointer === `card:${card.id}` || !bar.shown} onaction={(a) => cardAction(card, a)} />
        {/if}
        <!-- Wispr's 4 px hit strip on the pill's side of every card. -->
        <span class="strip" data-region={`card:${card.id}`} aria-hidden="true"></span>
      </div>
    {/each}
  </div>

  <div class="dock" bind:clientHeight={dockH}>
    {#if mode === 'dictation'}
      <div class="hit quiet" in:scale={{ start: 0.7, duration: 220, easing: cubicOut }} out:fade={{ duration: 120 }}>
        <DictationCapsule />
      </div>
    {:else if mode === 'meeting'}
      <div
        class="hit"
        data-region="pill"
        role="presentation"
        onpointerenter={() => (domHover = true)}
        onpointerleave={() => (domHover = false)}
      >
        <div class="slot" in:scale={{ start: 0.7, duration: 220, easing: cubicOut }} out:fade={{ duration: 120 }}>
          <div class="meeting" class:hot={hovering} aria-label="Recording meeting notes">
            <button
              class="mbody"
              aria-label="Open the meeting notes"
              onclick={openMeeting}
              onpointerenter={(e) => hoverTip('open', e)}
              onpointerleave={() => hoverTip(null)}
            >
              <span class="bars" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span>
              <span class="time">{elapsed}</span>
            </button>
            <button
              class="mstop"
              aria-label="Stop and summarise"
              onclick={stopMeeting}
              onpointerenter={(e) => hoverTip('stop', e)}
              onpointerleave={() => hoverTip(null)}
            >
              <span class="square" aria-hidden="true"></span>
            </button>
          </div>
        </div>
        {#if tip && !menuOpen}
          <span class="tip" style={`left:${tipX}px`} role="tooltip" transition:fade={{ duration: 110 }}>
            {tips[tip].label}{#if tips[tip].keys}<b>{tips[tip].keys}</b>{/if}
          </span>
        {/if}
      </div>
    {:else if mode === 'idle'}
      <div
        class="hit"
        data-region="pill"
        role="presentation"
        onpointerenter={() => (domHover = true)}
        onpointerleave={() => (domHover = false)}
      >
        <div class="pill" class:open={expanded} aria-label="Yap bar">
          <button
            class="btn mic"
            aria-label="Dictate"
            tabindex={expanded ? 0 : -1}
            onclick={dictate}
            onpointerenter={(e) => hoverTip('dictate', e)}
            onpointerleave={() => hoverTip(null)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5.5 11a6.5 6.5 0 0 0 13 0" /><path d="M12 17.5V21" /></svg>
          </button>
          <span class="sep" aria-hidden="true"></span>
          <button
            class="btn notes"
            aria-label={bar.call ?? 'Meeting notes'}
            tabindex={expanded ? 0 : -1}
            onclick={meetingNotes}
            onpointerenter={(e) => hoverTip('notes', e)}
            onpointerleave={() => hoverTip(null)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="12" cy="12" r="7.5" /><circle cx="12" cy="12" r="3.4" fill="currentColor" stroke="none" /></svg>
            {#if bar.call}<span class="live" aria-hidden="true"></span>{/if}
          </button>
          <button
            class="btn more"
            class:on={menuOpen}
            aria-label="More"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            tabindex={expanded ? 0 : -1}
            onclick={() => {
              tip = null;
              menuOpen = !menuOpen;
            }}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 14.5l5-5 5 5" /></svg>
          </button>
        </div>
        {#if tip && !menuOpen}
          <span class="tip" style={`left:${tipX}px`} role="tooltip" transition:fade={{ duration: 110 }}>
            {tips[tip].label}{#if tips[tip].keys}<b>{tips[tip].keys}</b>{/if}
          </span>
        {/if}
        {#if menuOpen}
          <div class="menu" data-region="menu" role="menu" transition:fade={{ duration: 120 }}>
            <button role="menuitem" onclick={() => menuItem('open')}>Open Yap</button>
            <button role="menuitem" onclick={() => menuItem('new-note')}>New meeting note</button>
            <button role="menuitem" onclick={() => menuItem('settings')}>Settings</button>
            <span class="rule" aria-hidden="true"></span>
            <button role="menuitem" onclick={() => menuItem('hide')}>Hide the bar for 1 hour</button>
            <button role="menuitem" onclick={() => menuItem('off')}>Turn off the bar</button>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  /* The window is transparent and click-through; only what's drawn here
     shows. Sizes, colours and timings follow Wispr's measured Flow Bar
     (flowbar-spec.md). No shadow on the pills, and the cards' is 1–2 px, so
     nothing reaches a transparent WebView2 window's edge (a grey box) and
     there's no backdrop-filter for the same reason. */
  .stage {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: center;
    /* The hit wrapper's bottom 8 px up + its 6 px padding: every pill sits
       14 px above the work area, as Wispr's does. */
    padding: 0 0 8px;
    box-sizing: border-box;
    font-family: inherit;
    user-select: none;
  }
  .stage.top {
    justify-content: flex-start;
    padding: 8px 0 0;
  }
  /* Cards, anchored 26 px above the pill's top (positioned by the script
     from the dock's height); the first nearest the pill. */
  .cards {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column-reverse;
    align-items: center;
    gap: 12px;
    transition:
      bottom 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      top 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95);
  }
  .stage.top .cards {
    flex-direction: column;
  }
  .cardwrap {
    position: relative;
  }
  /* Wispr's 4 px hit strip, on the pill's side of each card, so the
     pointer can travel from the pill up onto a card. */
  .strip {
    position: absolute;
    left: 1px;
    right: 1px;
    bottom: -3px;
    height: 4px;
    background: rgba(0, 0, 0, 0.004);
  }
  .stage.top .strip {
    bottom: auto;
    top: -3px;
  }
  .stage.pop .dock,
  .stage.pop .cards {
    animation: pop 240ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @keyframes pop {
    from {
      opacity: 0.35;
      transform: translateY(6px) scale(0.94);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  /* The pill's place. Its states share one grid cell, so one can fade out
     while the next scales in, centred, without moving anything else. */
  .dock {
    display: grid;
    place-items: end center;
  }
  .stage.top .dock {
    place-items: start center;
  }
  .dock > :global(*) {
    grid-area: 1 / 1;
  }
  .slot {
    display: flex;
  }
  /* The hit wrapper (reported as region "pill"): Wispr's 6 × 10 px of
     nearly invisible padding round the pill, 60 × 20 when idle, so hovering
     just outside the pill still counts; it grows with what it holds. */
  .hit {
    position: relative;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    padding: 6px 10px;
    background: rgba(0, 0, 0, 0.004);
  }
  .stage.top .hit {
    align-items: flex-start;
  }
  .hit.quiet {
    background: none;
  }

  /* ---- the idle pill (40 × 8) → the open pill (one capsule morphing) ---- */
  .pill {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 8px;
    padding: 0;
    box-sizing: border-box;
    overflow: hidden;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.5);
    border: 1px solid rgba(255, 255, 255, 0.5);
    transition: all 0.1s cubic-bezier(0.05, 0.6, 0.4, 0.95);
  }
  /* (The open pill isn't measured yet: Yap's own until it is.) */
  .pill.open {
    width: 128px;
    height: 38px;
    padding: 0 4px;
    border-radius: 19px;
    background: #000;
    border-color: rgb(48, 48, 47);
    transition: all 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95);
  }
  .btn {
    position: relative;
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 0;
    height: 30px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: none;
    color: rgb(252, 252, 251);
    opacity: 0;
    cursor: pointer;
    pointer-events: none;
    transition:
      width 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      opacity 0.1s ease,
      background 0.15s ease;
  }
  .pill.open .btn {
    width: 34px;
    opacity: 1;
    pointer-events: auto;
    transition:
      width 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      opacity 0.14s ease 0.06s,
      background 0.15s ease;
  }
  .pill.open .btn.more {
    width: 26px;
  }
  .btn svg {
    width: 17px;
    height: 17px;
    flex: 0 0 auto;
  }
  .btn.more svg {
    width: 15px;
    height: 15px;
    transition: transform 200ms ease;
  }
  .btn.more.on svg {
    transform: rotate(180deg);
  }
  .btn:hover,
  .btn.on {
    background: rgba(255, 255, 255, 0.12);
  }
  .sep {
    flex: 0 0 auto;
    width: 0;
    height: 16px;
    margin: 0;
    background: rgba(255, 255, 255, 0.16);
    opacity: 0;
    transition: opacity 0.14s ease;
  }
  .pill.open .sep {
    width: 1px;
    margin: 0 4px;
    opacity: 1;
  }
  /* A live call ◉ would record: a small green light on it. */
  .live {
    position: absolute;
    top: 5px;
    right: 6px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #3dbb74;
    box-shadow: 0 0 0 1.5px #000;
  }

  /* ---- tooltip: "Dictate F9", "New note Win + Alt + M" (Wispr's ink
     tooltip: radius 8, 12 / 600 / 20) ---- */
  .tip {
    position: absolute;
    bottom: calc(100% + 2px);
    transform: translateX(-50%);
    display: inline-flex;
    gap: 5px;
    padding: 6px 12px;
    border-radius: 8px;
    background: rgb(26, 26, 26);
    color: rgb(238, 235, 227);
    font-size: 12px;
    font-weight: 600;
    line-height: 20px;
    white-space: nowrap;
    pointer-events: none;
  }
  .tip b {
    font-weight: 600;
    color: #fff;
  }
  .stage.top .tip {
    bottom: auto;
    top: calc(100% + 2px);
  }

  /* ---- the ^ menu ---- */
  .menu {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    min-width: 214px;
    padding: 6px;
    border-radius: 12px;
    background: #000;
    border: 1px solid rgb(48, 48, 47);
    box-shadow: 0 1px 2px rgba(26, 26, 26, 0.05);
  }
  .stage.top .menu {
    bottom: auto;
    top: calc(100% + 4px);
  }
  .menu button {
    padding: 8px 12px;
    border: none;
    border-radius: 8px;
    background: none;
    color: rgb(238, 235, 227);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover {
    background: rgba(255, 255, 255, 0.09);
    color: #fff;
  }
  .rule {
    height: 1px;
    margin: 5px 8px;
    background: rgb(48, 48, 47);
  }

  /* ---- the meeting recording pill: 69 × 30, black, a 2 px ring in Yap's
     recording red (Wispr: emerald), half opacity until hovered, when it
     comes up and shows its timer ---- */
  .meeting {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 30px;
    padding: 0 5px;
    box-sizing: border-box;
    border-radius: 22.5px;
    background: #000;
    border: 2px solid #e5645e;
    opacity: 0.5;
    transition:
      opacity 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      border-color 0.13s ease;
  }
  .meeting.hot {
    opacity: 1;
  }
  .mbody,
  .mstop {
    position: relative;
    display: inline-flex;
    align-items: center;
    padding: 0;
    border: none;
    background: none;
    color: rgb(252, 252, 251);
    font: inherit;
    cursor: pointer;
  }
  .mbody {
    height: 26px;
    gap: 0;
  }
  /* 5 white bars, 2 × 18, gap 2, Wispr's staggered timing. */
  .bars {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 18px;
    padding: 0 1.5px 0 4.5px;
  }
  .bars i {
    width: 2px;
    height: 18px;
    border-radius: 0.5px;
    background: #fff;
    transform-origin: center;
    animation: eq 1s ease-in-out infinite;
  }
  .bars i:nth-child(2) {
    animation-delay: 0.1s;
  }
  .bars i:nth-child(3) {
    animation-delay: 0.2s;
  }
  .bars i:nth-child(4) {
    animation-delay: -0.2s;
  }
  .bars i:nth-child(5) {
    animation-delay: -0.1s;
  }
  /* The timer: folded away at rest (69 px has no room), out on hover. */
  .time {
    max-width: 0;
    overflow: hidden;
    opacity: 0;
    font-size: 12.5px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: rgb(252, 252, 251);
    transition:
      max-width 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      margin 0.16s cubic-bezier(0.05, 0.6, 0.4, 0.95),
      opacity 0.12s ease;
  }
  .meeting.hot .time {
    max-width: 52px;
    margin-left: 10px;
    opacity: 1;
  }
  /* The stop: a 19 px circle holding an 8 × 8 rounded square. */
  .mstop {
    justify-content: center;
    width: 19px;
    height: 19px;
    border-radius: 50%;
    background: rgb(77, 74, 66);
    transition: background 0.15s ease;
  }
  .mstop:hover {
    background: rgb(98, 94, 84);
  }
  .square {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: #fff;
  }
  @keyframes eq {
    0%,
    100% {
      transform: scaleY(0.3);
    }
    50% {
      transform: scaleY(1);
    }
  }
</style>
