<script>
  // The call prompt on the Yap bar (bar.rs `Card` with style "call",
  // meeting_detect.rs `native::bar_card`), after Wispr's "Meeting detected"
  // card (screenshot 25): one dark row — the call app's mark, "Teams call
  // detected" over "● Now", and a light split button, [Yap] Record notes,
  // whose ^ menu holds the other answers (Not now, Don't ask for Teams). The
  // ✕ on its top-left corner is the quiet answer, as is fading after 30 s
  // (a hairline runs along the bottom; paused while the pointer is on it).
  // With "Start notes automatically" a ring drains around the logo and the
  // line under the title counts down; Esc cancels (Rust watches the key).
  // A call that ended while Yap records it: "● Still recording" (red),
  // Stop and summarise | ^ Keep recording. The calendar's reminder
  // (calendar.rs `bar_card`) is one too: the meeting over "● In 1 min · with
  // Tanay +1" (amber, then green once it's on), Join & take notes | ^ Start
  // notes, Snooze 2 min; a calendar glyph when the meeting has no call app.
  import yapLogo from '../../assets/yap-logo.svg';
  import { callAppIcon } from './callApps.js';
  import { fadeTimer, countdownClock } from './cardTimers.svelte.js';

  /** @type {{ card: any, paused?: boolean, hovered?: boolean, onaction: (action: string) => void }} */
  let { card, paused = false, hovered = false, onaction } = $props();

  fadeTimer(() => card, () => paused, (a) => onaction(a));
  const clock = countdownClock(() => card);
  const icon = $derived(callAppIcon(card.app));
  const region = $derived(`card:${card.id}`);
  const RING = 2 * Math.PI * 12;

  let menuOpen = $state(false);
  // The menu closes once the pointer has left the card for a moment.
  let menuTimer = null;
  $effect(() => {
    clearTimeout(menuTimer);
    if (menuOpen && !hovered) menuTimer = setTimeout(() => (menuOpen = false), 700);
    return () => clearTimeout(menuTimer);
  });

  function answer(id) {
    menuOpen = false;
    onaction(id ?? '');
  }
</script>

<div class="call" class:counting={!!card.countdown} data-region={region} role="status" aria-live="polite">
  <button class="x" data-region={region} aria-label="Close" onclick={() => answer(card.closeAction)}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
  </button>

  <span class="mark" aria-hidden="true">
    {#if icon.kind === 'glyph'}
      <svg viewBox="0 0 24 24"><path d={icon.path} fill={icon.color} /></svg>
    {:else if icon.kind === 'monogram'}
      <span class="letter" style={`color:${icon.color}`}>{icon.letter}</span>
    {:else if card.icon === 'calendar'}
      <!-- A calendar meeting with no call app's mark (calendar.rs). -->
      <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3.5" y="4.5" width="17" height="16" rx="3" /><path d="M3.5 9.5h17M8 2.8v3.4M16 2.8v3.4" /></svg>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="#26231c" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
    {/if}
  </span>

  <div class="text">
    <div class="title">{card.title}</div>
    <div class="status">
      {#if card.countdown}
        <span class="count">{clock.secs > 0 ? `${card.countdown.label} ${clock.secs}…` : 'Starting notes…'}</span>
        <span class="esc">Esc to cancel</span>
      {:else if card.status}
        {#if card.dot}<span class="dot {card.dot}" aria-hidden="true"></span>{/if}
        <span class="line">{card.status}</span>
      {/if}
    </div>
  </div>

  {#if card.primary}
    <div class="split">
      <button class="go" onclick={() => answer(card.primary.id)}>
        <span class="logo">
          <img src={yapLogo} alt="" />
          {#if card.countdown}
            <svg class="ring" viewBox="0 0 28 28" aria-hidden="true">
              <circle cx="14" cy="14" r="12" class="track" />
              <circle cx="14" cy="14" r="12" class="run" stroke-dasharray={RING} stroke-dashoffset={RING * (1 - clock.fraction)} />
            </svg>
          {/if}
        </span>
        <span class="label">{card.primary.label}</span>
      </button>
      {#if card.secondary || card.link}
        <span class="divider" aria-hidden="true"></span>
        <button
          class="more"
          class:on={menuOpen}
          aria-label="More answers"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onclick={() => (menuOpen = !menuOpen)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 14.5l5-5 5 5" /></svg>
        </button>
      {/if}
      {#if menuOpen}
        <div class="menu" role="menu" data-region={region}>
          {#if card.secondary}
            <button role="menuitem" onclick={() => answer(card.secondary.id)}>{card.secondary.label}</button>
          {/if}
          {#if card.link}
            <button role="menuitem" onclick={() => answer(card.link.id)}>{card.link.label}</button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if card.timeoutMs && !card.countdown}
    <div class="hairline">
      <div class="drain" class:paused style={`animation-duration:${card.timeoutMs}ms`}></div>
    </div>
  {/if}
</div>

<style>
  /* Wispr's "Meeting detected" card in Yap's ink: one row, a big radius, a
     soft shadow that stays well inside the bar window. */
  .call {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 440px;
    box-sizing: border-box;
    padding: 12px 12px 12px 14px;
    border-radius: 18px;
    background: #161411;
    border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.32);
    color: rgba(255, 255, 255, 0.95);
    font-size: 13px;
    pointer-events: auto;
  }
  /* The ✕ sits on the top-left corner. */
  .x {
    position: absolute;
    top: -9px;
    left: -9px;
    display: flex;
    width: 24px;
    height: 24px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 50%;
    background: #2a2620;
    color: rgba(255, 255, 255, 0.8);
    cursor: pointer;
    transition:
      background 150ms ease,
      color 150ms ease;
  }
  .x:hover {
    background: #3a352c;
    color: #fff;
  }
  .x svg {
    width: 11px;
    height: 11px;
  }
  /* The call app's mark on a light tile, in its own colour. */
  .mark {
    flex: 0 0 auto;
    display: flex;
    width: 36px;
    height: 36px;
    align-items: center;
    justify-content: center;
    border-radius: 11px;
    background: #f5f3ee;
  }
  .mark svg {
    width: 21px;
    height: 21px;
  }
  .letter {
    font-size: 19px;
    font-weight: 800;
    line-height: 1;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-size: 14.5px;
    font-weight: 700;
    line-height: 1.25;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    font-size: 12.5px;
    color: rgba(255, 255, 255, 0.55);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #3dbb74;
    box-shadow: 0 0 6px rgba(61, 187, 116, 0.6);
  }
  .dot.recording {
    background: #e5645e;
    box-shadow: 0 0 6px rgba(229, 100, 94, 0.6);
    animation: pulse 1.4s ease-in-out infinite;
  }
  /* A calendar meeting about to start (calendar.rs): Yap's amber. */
  .dot.soon {
    background: #e2982a;
    box-shadow: 0 0 6px rgba(226, 152, 42, 0.6);
  }
  /* A long line ("Started 12 min ago · with Tanay +3") ends in "…". */
  .line {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }
  .count {
    font-weight: 650;
    color: #f6c77a;
  }
  .esc {
    color: rgba(255, 255, 255, 0.4);
    font-size: 11.5px;
  }

  /* The light split button: [Yap] Record notes | ^ */
  .split {
    position: relative;
    flex: 0 0 auto;
    display: flex;
    align-items: stretch;
    height: 38px;
    border-radius: 11px;
    background: #f5f3ee;
    color: #26231c;
  }
  .go,
  .more {
    display: inline-flex;
    align-items: center;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .go {
    gap: 8px;
    padding: 0 12px 0 7px;
    border-radius: 11px 0 0 11px;
    font-size: 13px;
    font-weight: 650;
  }
  .split:not(:has(.more)) .go {
    border-radius: 11px;
  }
  .go:hover,
  .more:hover,
  .more.on {
    background: #ffffff;
  }
  .logo {
    position: relative;
    display: flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
  }
  .logo img {
    width: 24px;
    height: 24px;
    border-radius: 7px;
  }
  .counting .logo img {
    width: 18px;
    height: 18px;
    border-radius: 5px;
  }
  .ring {
    position: absolute;
    inset: -1px;
    width: 28px;
    height: 28px;
    transform: rotate(-90deg);
  }
  .ring circle {
    fill: none;
    stroke-width: 2.6;
  }
  .ring .track {
    stroke: rgba(38, 35, 28, 0.14);
  }
  .ring .run {
    stroke: #c2690a;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s linear;
  }
  .divider {
    width: 1px;
    margin: 8px 0;
    background: rgba(38, 35, 28, 0.16);
  }
  .more {
    justify-content: center;
    width: 34px;
    border-radius: 0 11px 11px 0;
  }
  .more svg {
    width: 15px;
    height: 15px;
    transition: transform 180ms ease;
  }
  .more.on svg {
    transform: rotate(180deg);
  }
  /* The other answers, above the button. */
  .menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 6px;
    border-radius: 14px;
    background: #1c1a16;
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 10px 26px rgba(0, 0, 0, 0.32);
    animation: menu-in 150ms ease-out;
  }
  .menu button {
    padding: 8px 12px;
    border: none;
    border-radius: 9px;
    background: none;
    color: rgba(255, 255, 255, 0.88);
    font: inherit;
    font-size: 13px;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .menu button:hover {
    background: rgba(255, 255, 255, 0.09);
    color: #fff;
  }
  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(5px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .hairline {
    position: absolute;
    left: 20px;
    right: 20px;
    bottom: 0;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .drain {
    height: 100%;
    background: rgba(255, 255, 255, 0.16);
    animation-name: drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .drain.paused {
    animation-play-state: paused;
  }
  @keyframes drain {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
</style>
