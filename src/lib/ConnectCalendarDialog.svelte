<script>
  // "Connect your calendar": Wispr Flow's connect modal (its screenshot 27),
  // in Yap's own flows. Every Connect calendar button outside Settings opens
  // it (`openConnectDialog` in calendar.svelte.js: the Meetings view's empty
  // state, the nudge after a meeting, Integrations' Calendar card); the main
  // window mounts it once.
  //   top     a beige band with Yap's real pre-meeting card: bar/CallCard in
  //           its static `preview` mode, on sample data, so the picture
  //           always matches the bar;
  //   bottom  "Connect your calendar", one line on what it gives you, and
  //           three ways in:
  //     - Continue with Google: the one-click connect (calendar.rs →
  //       calendar/google.rs: the system browser, PKCE), waiting here with
  //       Cancel. Closing the dialog doesn't stop it: finishing in the
  //       browser still connects (with a toast). A build without Google's
  //       client keeps the button off and points at an iCal link instead;
  //     - Continue with Outlook: the guided publish-your-calendar ICS link;
  //     - iCloud or another calendar: any private iCal link.
  //     Yap fetches a link before keeping it (`calendar_add_link`), so a
  //     wrong one says why, here.
  // A connection shows "Calendar connected · 6 meetings in the next 7 days"
  // for a moment, then the dialog closes on a Meetings view that already
  // lists them. On Wispr's measured dialog tokens (notepad-spec.md): the
  // scrim, a paper card with a hairline border and the dialog shadow,
  // scaling in. Esc, ✕ and the scrim close it; the focus stays inside while
  // it's open and goes back where it was.
  import { invoke } from '@tauri-apps/api/core';
  import { tick, untrack } from 'svelte';
  import { calendar, connectDialog, closeConnectDialog, connectDialogOpener } from './calendar.svelte.js';
  import CallCard from './bar/CallCard.svelte';
  import CalendarMark from './CalendarMark.svelte';

  /** The band's card: the bar's pre-meeting card, on made-up data. */
  const SAMPLE = {
    id: 'preview',
    style: 'call',
    icon: 'calendar',
    app: 'meet',
    title: 'Design review',
    status: 'In 2 min · with Sam',
    dot: 'soon',
    primary: { id: 'join', label: 'Join & take notes' },
    secondary: { id: 'start', label: 'Start notes' },
    link: { id: 'snooze', label: 'Snooze 2 min' },
  };
  /** How long "Calendar connected" stays before the dialog closes. */
  const DONE_MS = 2400;

  /** 'choose' | 'outlook' | 'ics' */
  let step = $state('choose');
  let link = $state('');
  let adding = $state(false);
  let formError = $state('');
  /** This dialog started a Google sign-in and waits for it. */
  let googleFlow = $state(false);
  let dialog = $state(null);
  let downOnScrim = false;

  const done = $derived(connectDialog.done);
  const full = $derived(calendar.connections.length >= calendar.maxConnections);
  const waiting = $derived(googleFlow || calendar.google.waiting);
  const doneLine = $derived(done ? meetingsLine(done.meetings) : '');
  // For screen readers: what changed without the focus moving.
  const announce = $derived(
    done ? `Calendar connected. ${doneLine}.` : waiting ? 'Finish connecting in your browser.' : ''
  );

  function meetingsLine(n) {
    if (n == null) return 'Your meetings are on their way';
    if (n === 0) return 'No meetings in the next 7 days';
    return `${n} meeting${n === 1 ? '' : 's'} in the next 7 days`;
  }

  // ---- open, close, focus ----

  let wasOpen = false;
  $effect(() => {
    const open = connectDialog.open;
    untrack(() => {
      if (open && !wasOpen) opened();
      else if (!open && wasOpen) closed();
      wasOpen = open;
    });
  });

  async function opened() {
    await tick();
    focusFirst();
  }

  function closed() {
    // Next time, from the start. (A Google sign-in still waiting carries on:
    // finishing it in the browser connects, with a toast.)
    step = 'choose';
    link = '';
    formError = '';
    adding = false;
    googleFlow = false;
    // Back to the button that opened it; if a connection replaced that (the
    // empty state is gone), the view's title.
    const back = connectDialogOpener() ?? document.querySelector('[data-dialog-return]');
    back?.focus?.({ preventScroll: true });
  }

  function close() {
    closeConnectDialog();
  }

  function focusables() {
    if (!dialog) return [];
    return [...dialog.querySelectorAll('button, input, a[href], [tabindex]:not([tabindex="-1"])')].filter(
      (el) => !el.disabled && !el.closest('[inert]') && el.getClientRects().length > 0
    );
  }

  /** The first way in that works, or the step's first field. */
  function focusFirst() {
    const target = dialog?.querySelector('[data-first]:not(:disabled)') ?? focusables()[0] ?? dialog;
    target?.focus({ preventScroll: true });
  }

  // Esc closes; Tab goes round inside. Capturing on the window, so Esc is
  // the dialog's before anything else's (the reminder card, Settings).
  function onKeydown(e) {
    if (!dialog) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const items = focusables();
    if (items.length === 0) {
      e.preventDefault();
      dialog.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    const inside = dialog.contains(active) && active !== dialog;
    if (e.shiftKey && (!inside || active === first)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (!inside || active === last)) {
      e.preventDefault();
      first.focus();
    }
  }

  $effect(() => {
    if (!connectDialog.open) return;
    window.addEventListener('keydown', onKeydown, true);
    return () => window.removeEventListener('keydown', onKeydown, true);
  });

  // A press that starts on the scrim and ends there closes it (a drag out of
  // the link field doesn't).
  function onScrimDown(e) {
    downOnScrim = e.target === e.currentTarget;
  }
  function onScrimClick(e) {
    if (downOnScrim && e.target === e.currentTarget) close();
    downOnScrim = false;
  }

  // ---- Google ----

  async function continueWithGoogle() {
    if (!calendar.google.available || waiting || full) return;
    connectDialog.error = '';
    googleFlow = true;
    await tick();
    dialog?.querySelector('[data-cancel]')?.focus({ preventScroll: true });
    try {
      await invoke('calendar_connect_google');
    } catch (e) {
      googleFlow = false;
      connectDialog.error = String(e);
      await tick();
      focusFirst();
    }
  }

  function cancelGoogle() {
    googleFlow = false;
    invoke('calendar_cancel_google').catch(() => {});
    tick().then(focusFirst);
  }

  // The sign-in ended without a word to this dialog (cancelled in Settings,
  // timed out): back to the choices. A connection or a failure says so
  // through `done` / `error`, which end the wait too.
  let sawWaiting = false;
  $effect(() => {
    const w = calendar.google.waiting;
    untrack(() => {
      if (!googleFlow) sawWaiting = false;
      else if (w) sawWaiting = true;
      else if (sawWaiting) {
        googleFlow = false;
        sawWaiting = false;
      }
    });
  });
  $effect(() => {
    if (connectDialog.error || connectDialog.done) untrack(() => (googleFlow = false));
  });

  // ---- a link (Outlook, iCloud, any calendar) ----

  async function go(next) {
    step = next;
    link = '';
    formError = '';
    connectDialog.error = '';
    await tick();
    dialog?.querySelector('input')?.focus({ preventScroll: true });
  }

  async function back() {
    const from = step;
    step = 'choose';
    formError = '';
    await tick();
    (dialog?.querySelector(`[data-way="${from}"]`) ?? dialog)?.focus({ preventScroll: true });
  }

  async function addLink() {
    const value = link.trim();
    if (!value || adding) return;
    const kind = step === 'outlook' ? 'outlook' : 'ics';
    adding = true;
    formError = '';
    try {
      const result = await invoke('calendar_add_link', { link: value, kind });
      if (connectDialog.open && !connectDialog.done) connectDialog.done = result ?? { kind };
    } catch (e) {
      formError = String(e);
    } finally {
      adding = false;
    }
  }

  // ---- connected ----

  $effect(() => {
    if (!connectDialog.open || !done) return;
    untrack(() => {
      // The message covers the step, from the top of it.
      const body = dialog?.querySelector('.body');
      if (body) body.scrollTop = 0;
      dialog?.focus({ preventScroll: true });
    });
    const timer = setTimeout(closeConnectDialog, DONE_MS);
    return () => clearTimeout(timer);
  });
</script>

{#if connectDialog.open}
  <div class="scrim" role="presentation" onpointerdown={onScrimDown} onclick={onScrimClick}>
    <div
      class="dialog"
      bind:this={dialog}
      role="dialog"
      aria-modal="true"
      aria-labelledby={done ? 'cc-done-title' : 'cc-title'}
      aria-describedby={done ? 'cc-done-line' : 'cc-lead'}
      tabindex="-1"
    >
      <div class="band" class:compact={step !== 'choose'}>
        <div class="art" aria-hidden="true">
          <CallCard card={SAMPLE} preview />
        </div>
        <button class="x" type="button" aria-label="Close" onclick={close}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
        </button>
      </div>

      <div class="body" class:isdone={!!done}>
        <div class="step" inert={!!done}>
          {#if step === 'choose'}
            <h2 id="cc-title">Connect your calendar</h2>
            <p class="lead" id="cc-lead">Get a heads-up before each meeting, join in one click, and notes with everyone's names.</p>
            <div class="ways">
              {#if waiting}
                <div class="way waiting">
                  <span class="spinner" aria-hidden="true"></span>
                  <span class="wtext">Finish connecting in your browser…</span>
                  <button class="cancel" type="button" data-cancel onclick={cancelGoogle}>Cancel</button>
                </div>
                <p class="note"><span>Choose your Google account and let Yap see your calendar.</span> <span>This updates by itself.</span></p>
              {:else}
                <button
                  class="way"
                  type="button"
                  data-way="google"
                  data-first
                  onclick={continueWithGoogle}
                  disabled={!calendar.google.available || full}
                >
                  <span class="mark"><CalendarMark kind="g" /></span>
                  Continue with Google
                </button>
                {#if !calendar.google.available}
                  <p class="note"><span>Google sign-in is in the installed Yap.</span> <span>In this build, use a private iCal link below.</span></p>
                {/if}
              {/if}
              <button class="way" type="button" data-way="outlook" data-first onclick={() => go('outlook')} disabled={full}>
                <span class="mark"><CalendarMark kind="outlook" /></span>
                Continue with Outlook
              </button>
              <button class="way" type="button" data-way="ics" data-first onclick={() => go('ics')} disabled={full}>
                <span class="mark ics"><CalendarMark kind="ics" /></span>
                iCloud or another calendar
              </button>
            </div>
            {#if connectDialog.error}<p class="err" role="alert">{connectDialog.error}</p>{/if}
            {#if full}
              <p class="note left">That's as many calendars as Yap holds ({calendar.maxConnections}). Disconnect one in Settings → Connectors to add another.</p>
            {/if}
          {:else}
            <div class="stephead">
              <button class="back" type="button" aria-label="Back" onclick={back}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14.5 6l-6 6 6 6" /></svg>
              </button>
              <h2 id="cc-title">{step === 'outlook' ? 'Outlook Calendar' : 'iCloud or another calendar'}</h2>
            </div>
            {#if step === 'outlook'}
              <p class="lead" id="cc-lead">Outlook can share your calendar as a private link. Publish it, then paste the link here.</p>
              <ol class="steps">
                <li>Open Outlook on the web (outlook.office.com, or outlook.live.com for a personal account).</li>
                <li>Go to <b>Settings → Calendar → Shared calendars</b>.</li>
                <li>Under <b>Publish a calendar</b>, pick your calendar and <b>Can view all details</b> (so Yap sees who's invited), then <b>Publish</b>.</li>
                <li>Copy the <b>ICS</b> link and paste it below.</li>
              </ol>
            {:else}
              <p class="lead" id="cc-lead">Paste your calendar's private iCal link. Yap checks it straight away.</p>
              <ul class="steps plain">
                <li><b>iCloud</b>: Calendar → the share icon by a calendar → <b>Public Calendar</b> → Copy Link.</li>
                <li><b>Google Calendar</b>: Settings → your calendar → Integrate calendar → <b>Secret address in iCal format</b>.</li>
                <li><b>Fastmail and others</b>: the calendar's sharing or export settings, its <b>iCal / ICS</b> link.</li>
              </ul>
            {/if}
            <!-- novalidate: Yap's own checks say what's wrong with a link. -->
            <form
              class="lrow"
              novalidate
              onsubmit={(e) => {
                e.preventDefault();
                addLink();
              }}
            >
              <input
                class="linput"
                type="url"
                placeholder={step === 'outlook' ? 'https://outlook.office365.com/owa/calendar/…/calendar.ics' : 'https:// or webcal:// …'}
                aria-label={step === 'outlook' ? 'Outlook calendar ICS link' : 'Calendar iCal link'}
                bind:value={link}
                disabled={adding}
              />
              <button class="go" type="submit" disabled={adding || !link.trim()}>{adding ? 'Connecting…' : 'Connect'}</button>
            </form>
            {#if formError}<p class="err" role="alert">{formError}</p>{/if}
            <p class="fine">The link works like a password: anyone with it can read the calendar. Yap keeps it in Windows Credential Manager and reads it straight from this PC.</p>
          {/if}
        </div>

        {#if done}
          <div class="done">
            <span class="tick" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
            </span>
            <div>
              <h2 id="cc-done-title">Calendar connected</h2>
              <p class="lead" id="cc-done-line">{doneLine}</p>
            </div>
          </div>
        {/if}
        <p class="sr" aria-live="polite">{announce}</p>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Wispr's dialog: a 30 % ink scrim, a paper card with a hairline border
     and the dialog shadow, scaling in from 0.9 (notepad-spec.md); about
     506 px wide with large radii (screenshot 27). Above Settings, under the
     toasts (a meeting about to start still shows). Its top stays put (about
     centred at first) while the steps change its height. */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: clamp(16px, calc(50vh - 262px), 200px) 16px 16px;
    box-sizing: border-box;
    background: rgba(26, 26, 26, 0.3);
    animation: scrim-in 0.15s ease-out;
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .dialog {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 506px;
    max-width: 100%;
    max-height: 100%;
    box-sizing: border-box;
    overflow: hidden;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 20px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: dialog-in 0.15s var(--yap-ease-spring) both;
  }
  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: scale(0.9);
    }
  }

  /* The illustration: the bar's card on a warm beige band. Smaller while a
     link's steps need the room. */
  .band {
    position: relative;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 232px;
    background: var(--yap-paper-band);
    border-bottom: 1px solid var(--yap-paper-s2);
    transition: height 0.4s var(--yap-ease-spring);
  }
  .band.compact {
    height: 148px;
  }
  .art {
    /* The card's ✕ sticks out 9 px above its corner. */
    padding-top: 9px;
  }
  .x {
    position: absolute;
    top: 10px;
    right: 11px;
    display: flex;
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: rgba(26, 26, 26, 0.05);
    color: var(--yap-paper-ink);
    cursor: pointer;
    transition: background-color 0.15s ease;
  }
  .x:hover {
    background: rgba(26, 26, 26, 0.1);
  }
  .x:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring), inset 0 0 0 1px var(--yap-paper-text-2);
  }
  .x svg {
    width: 13px;
    height: 13px;
  }

  .body {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 30px 24px 24px;
  }
  h2 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    line-height: 28px;
    letter-spacing: -0.01em;
  }
  .lead {
    margin: 6px 0 20px;
    font-size: 15px;
    line-height: 22px;
    color: var(--yap-paper-text-2);
    text-wrap: pretty;
  }

  /* Three full-width outlined buttons, the mark beside the words. */
  .ways {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .way {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    width: 100%;
    height: 40px;
    padding: 0 16px;
    box-sizing: border-box;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper);
    box-shadow: 0 1px 2px rgba(26, 26, 26, 0.05);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    cursor: pointer;
    transition:
      background-color 0.4s var(--yap-ease-spring),
      border-color 0.15s ease;
  }
  button.way:hover:not(:disabled) {
    background: #fff;
    border-color: var(--yap-paper-s2-strong);
  }
  button.way:focus-visible {
    outline: none;
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  button.way:disabled {
    cursor: default;
    color: var(--yap-paper-date);
    border-color: var(--yap-paper-s1);
    box-shadow: none;
  }
  button.way:disabled .mark {
    filter: grayscale(1);
    opacity: 0.45;
  }
  .mark {
    display: flex;
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
  }
  .mark.ics {
    color: var(--yap-paper-text-2);
  }
  /* Google's sign-in in the browser, in the Google button's place. */
  .way.waiting {
    justify-content: flex-start;
    padding: 0 4px 0 14px;
    background: var(--yap-paper-s1);
    box-shadow: none;
    cursor: default;
  }
  .spinner {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    box-sizing: border-box;
    border: 2px solid var(--yap-paper-s2-strong);
    border-top-color: var(--yap-paper-ink);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .wtext {
    flex: 1 1 auto;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cancel {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 6px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .cancel:hover {
    border-color: var(--yap-paper-s2-strong);
    background: #fff;
  }
  .cancel:focus-visible {
    outline: none;
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .note {
    margin: -4px 8px 0;
    font-size: 13px;
    line-height: 18px;
    text-align: center;
    text-wrap: balance;
    color: var(--yap-paper-muted);
  }
  /* A sentence a line. */
  .note span {
    display: block;
  }
  .note.left {
    margin: 14px 0 0;
    text-align: left;
  }
  .err {
    margin: 14px 0 0;
    font-size: 13.5px;
    line-height: 19px;
    color: var(--yap-danger);
  }

  /* A link's steps: back, its name, how to get the link, the field. */
  .stephead {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: -6px;
  }
  .back {
    display: flex;
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--yap-paper-ink);
    cursor: pointer;
  }
  .back:hover {
    background: var(--yap-paper-s1);
  }
  .back:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .back svg {
    width: 18px;
    height: 18px;
  }
  .steps {
    margin: -6px 0 16px;
    padding-left: 20px;
    font-size: 13.5px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
  }
  .steps li + li {
    margin-top: 3px;
  }
  .steps.plain {
    padding-left: 18px;
  }
  .steps b {
    font-weight: 600;
    color: var(--yap-paper-ink);
  }
  .lrow {
    display: flex;
    gap: 8px;
  }
  .linput {
    flex: 1 1 auto;
    min-width: 0;
    height: 40px;
    box-sizing: border-box;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: #fff;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    user-select: text;
    -webkit-user-select: text;
  }
  .linput::placeholder {
    color: var(--yap-paper-date);
  }
  .linput:focus {
    outline: none;
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .go {
    flex: 0 0 auto;
    height: 40px;
    padding: 0 18px;
    border: none;
    border-radius: 8px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }
  .go:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .go:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .fine {
    margin: 12px 0 0;
    font-size: 12px;
    line-height: 17px;
    color: var(--yap-paper-muted);
  }

  /* Connected: over the step, so the dialog keeps its size. */
  .body.isdone {
    overflow: hidden;
  }
  .step[inert] {
    visibility: hidden;
  }
  .done {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 0 24px;
    animation: done-in 0.3s var(--yap-ease-spring) both;
  }
  @keyframes done-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .done .lead {
    margin: 2px 0 0;
  }
  .tick {
    display: flex;
    flex: 0 0 auto;
    width: 44px;
    height: 44px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: rgba(16, 185, 129, 0.14);
    color: var(--yap-live-strong);
  }
  .tick svg {
    width: 22px;
    height: 22px;
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
</style>
