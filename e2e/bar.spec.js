// The Yap bar (src-tauri/src/bar.rs + overlay.rs, src/lib/Overlay.svelte):
// the always-there pill above the taskbar. One transparent window that's
// click-through except over the pill and its cards, never activated, and
// the surface for call prompts while the main window isn't focused.
// Test mode never reads the real cursor or the window in front: the
// debug-only `bar_simulate` puts a pretend cursor on a region (the same
// hit-test and click-through code a real cursor goes through) or fakes a
// fullscreen app, and `bar_debug` reports the window as Windows sees it. The
// real-window checks also ask Windows from outside (support/win32.js:
// GetWindowLongPtr, WindowFromPoint) — nothing moves the real mouse.
import {
  test,
  expect,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  pressHotkey,
  settingsDialog,
} from './support/fixtures.js';
import { windowFacts } from './support/win32.js';
import { standInIcons } from './support/icons.js';

// Teams "installed" (a stand-in: test mode reads no installed apps' icons).
test.use({ yapOptions: { name: 'bar', env: { YAP_E2E_APP_ICONS: standInIcons('bar') } } });

const debug = (yap) => yap.invoke('bar_debug');
const simulate = (yap, args) => yap.invoke('bar_simulate', args);
const callSim = (yap, appId, active) => yap.invoke('meeting_detect_simulate', { appId, active });
const barGroup = (main) => settingsDialog(main).getByRole('group', { name: 'Yap bar' });
/** A call prompt's card on the bar. */
const callCard = (bar, title) => bar.getByRole('status').filter({ hasText: title });
/** One of a call card's other answers, from its ^ menu. */
async function answerFromMenu(card, label) {
  await card.getByRole('button', { name: 'More answers' }).click();
  await card.getByRole('menuitem', { name: label }).click();
}

/** The pretend cursor onto the pill: it opens and the window takes clicks. */
async function hoverPill(yap) {
  await simulate(yap, { pointer: 'pill' });
  await expect.poll(() => debug(yap).then((d) => d.interactive)).toBe(true);
}

async function pointerAway(yap) {
  await simulate(yap, { pointer: 'away' });
  await expect.poll(() => debug(yap).then((d) => d.interactive)).toBe(false);
}

test.afterEach(async ({ yap }) => {
  await simulate(yap, { pointer: 'away', fullscreen: 'none' }).catch(() => {});
});

test('idle: a tiny pill above the taskbar, click-through and never activated', async ({ yap, shot }) => {
  const bar = yap.overlay;
  await expect(bar.getByLabel('Yap bar')).toBeVisible();
  await expect(bar.locator('.pill')).not.toHaveClass(/open/);

  const d = await debug(yap);
  expect(d.shown).toBe(true);
  expect(d.interactive).toBe(false);
  // Inside the app: the window's real ex-styles.
  expect(d.exStyle).toMatchObject({
    transparent: true,
    noActivate: true,
    toolWindow: true,
    layered: true,
    topmost: true,
    appWindow: false,
  });
  // Bottom-centre on the work area (test mode keeps it on the primary
  // monitor; a real cursor's monitor is in the unit tests).
  const { rect, screen } = d;
  expect(rect.bottom).toBe(screen.work.bottom);
  expect(Math.abs(rect.left + rect.right - (screen.work.left + screen.work.right))).toBeLessThanOrEqual(1);
  // From outside the app (PowerShell → user32): the same styles, and a click
  // on the pill would go straight through to whatever is under it.
  const outside = windowFacts(d.hwnd, d.pillPoint);
  expect(outside).toMatchObject({ visible: true, transparent: true, noActivate: true, toolWindow: true, topmost: true });
  expect(outside.windowAtPoint).not.toBe(d.hwnd);
  expect(d.clickOnPillReachesBar).toBe(false);
  expect(d.barInFront).toBe(false);
  // Wispr's measurements (flowbar-spec.md): a 40 × 8 pill, half black with a
  // 1 px half-white border, radius 6, no shadow, its bottom 14 px above the
  // work area, in a 60 × 20 hit wrapper.
  const pill = await bar.locator('.pill').evaluate((el) => {
    const r = el.getBoundingClientRect();
    const h = el.closest('.hit').getBoundingClientRect();
    const s = getComputedStyle(el);
    return {
      size: [r.width, r.height],
      hit: [h.width, h.height],
      fromBottom: window.innerHeight - r.bottom,
      bg: s.backgroundColor,
      border: `${s.borderTopWidth} ${s.borderTopColor}`,
      radius: s.borderTopLeftRadius,
      shadow: s.boxShadow,
    };
  });
  expect(pill).toEqual({
    size: [40, 8],
    hit: [60, 20],
    fromBottom: 14,
    // Wispr's sizes in Yap's bar colours, light by default (--bar-pill,
    // --bar-pill-border; the dark scheme is checked in the Settings test).
    bg: 'rgba(255, 255, 255, 0.85)',
    border: '1px rgba(35, 33, 27, 0.3)',
    radius: '6px',
    shadow: 'none',
  });
  await shot(bar, '01-idle');
});

test('hovering opens the pill and makes just that clickable, with tooltips', async ({ yap, shot }) => {
  const bar = yap.overlay;
  await hoverPill(yap);
  await expect(bar.locator('.pill')).toHaveClass(/open/);
  await expect(bar.getByRole('button', { name: 'Dictate' })).toBeVisible();
  await expect(bar.getByRole('button', { name: 'Meeting notes' })).toBeVisible();
  await expect(bar.getByRole('button', { name: 'More' })).toBeVisible();

  // Clickable now: WS_EX_TRANSPARENT is cleared, and a click on the pill
  // reaches the bar (asked of Windows from outside too).
  const d = await debug(yap);
  expect(d.exStyle.transparent).toBe(false);
  expect(d.exStyle.noActivate).toBe(true);
  expect(d.clickOnPillReachesBar).toBe(true);
  const outside = windowFacts(d.hwnd, d.pillPoint);
  expect(outside.transparent).toBe(false);
  expect(outside.windowAtPoint).toBe(d.hwnd);
  await bar.waitForTimeout(350); // the open animation
  await shot(bar, '02-expanded');

  // Tooltips name each button's shortcut (the seeded hotkey is F24).
  await bar.getByRole('button', { name: 'Dictate' }).hover();
  await expect(bar.getByRole('tooltip')).toHaveText(/Dictate\s*F24/);
  // Wispr's tooltip (radius 8, 12 / 600) in Yap's ink (--bar-tooltip).
  const tip = await bar.getByRole('tooltip').evaluate((el) => {
    const s = getComputedStyle(el);
    return [s.backgroundColor, s.borderTopLeftRadius, s.fontSize, s.fontWeight];
  });
  expect(tip).toEqual(['rgb(38, 35, 28)', '8px', '12px', '600']);
  await shot(bar, '03-tooltip-dictate');
  await bar.getByRole('button', { name: 'Meeting notes' }).hover();
  await expect(bar.getByRole('tooltip')).toContainText('New note');
  await shot(bar, '04-tooltip-new-note');

  // The pointer leaving: click-through again at once, and the pill closes.
  await pointerAway(yap);
  expect((await debug(yap)).exStyle.transparent).toBe(true);
  await bar.mouse.move(1, 1); // (the page's own hover, as a real pointer leaving)
  await expect(bar.locator('.pill')).not.toHaveClass(/open/);
});

test('its menu hides it for an hour, and Settings brings it back', async ({ yap, main, shot }) => {
  const bar = yap.overlay;
  await hoverPill(yap);
  await bar.getByRole('button', { name: 'More' }).click();
  const menu = bar.getByRole('menu');
  await expect(menu).toBeVisible();
  for (const item of ['Open Yap', 'New meeting note', 'Settings', 'Hide the bar for 1 hour', 'Turn off the bar']) {
    await expect(menu.getByRole('menuitem', { name: item })).toBeVisible();
  }
  await shot(bar, '05-menu');
  // Clicking it never made it the active window (Windows' own record, not
  // just the foreground: the active window of Yap's UI thread).
  const d = await debug(yap);
  expect(d.barActive).toBe(false);
  expect(d.threadActiveIsBar).toBe(false);
  expect(d.barInFront).toBe(false);

  await menu.getByRole('menuitem', { name: 'Hide the bar for 1 hour' }).click();
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(false);
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'overlay' })).toBe(false);

  await openSettings(main, 'General');
  const group = barGroup(main);
  await group.scrollIntoViewIfNeeded();
  await expect(group).toContainText(/Hidden until \d/);
  await shot(main, '06-settings-hidden-for-an-hour');
  await group.getByRole('button', { name: 'Show it now' }).click();
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(true);
  await expect(group).not.toContainText('Hidden until');
  await closeSettings(main);
});

test('"Turn off the bar" saves it off; Settings turns it back on', async ({ yap, main }) => {
  const bar = yap.overlay;
  await hoverPill(yap);
  await bar.getByRole('button', { name: 'More' }).click();
  await bar.getByRole('menu').getByRole('menuitem', { name: 'Turn off the bar' }).click();
  await expectStore(yap, 'config.json', (c) => c.barEnabled === false);
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(false);

  // Settings adopted it (its auto-save mustn't turn the bar back on)…
  await openSettings(main, 'General');
  const show = barGroup(main).getByRole('button', { name: 'Show the Yap bar', exact: true });
  await expect(show).toHaveAttribute('aria-pressed', 'false');
  await expect(barGroup(main).getByRole('button', { name: 'Hide in fullscreen apps' })).toBeDisabled();
  // …and switching it on there brings it back.
  await show.click();
  await expectStore(yap, 'config.json', (c) => c.barEnabled === true);
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(true);
  await closeSettings(main);
});

test('Settings → General → Yap bar', async ({ yap, main, shot }) => {
  await openSettings(main, 'General');
  const group = barGroup(main);
  await group.scrollIntoViewIfNeeded();
  await expect(group.getByRole('button', { name: 'Show the Yap bar', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(group.getByRole('button', { name: 'Hide in fullscreen apps' })).toHaveAttribute('aria-pressed', 'true');
  await expect(group.getByRole('button', { name: 'Live transcription preview' })).toBeVisible();
  await expect(group).toContainText('Position');
  await shot(main, '07-settings-yap-bar');

  // Colours: light by default; Dark recolours the bar at once (and the main
  // window's picture of its cards), and Light brings it back.
  const theme = () => yap.overlay.evaluate(() => document.documentElement.dataset.barTheme);
  const surface = () =>
    yap.overlay.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--bar-surface').trim());
  await expect(group.getByRole('tab', { name: 'Light' })).toHaveAttribute('aria-selected', 'true');
  expect([await theme(), await surface()]).toEqual(['light', '#ffffff']);
  await group.getByRole('tab', { name: 'Dark' }).click();
  await expectStore(yap, 'config.json', (c) => c.barTheme === 'dark');
  await expect.poll(theme).toBe('dark');
  expect(await surface()).toBe('#1c1a16');
  expect(await main.evaluate(() => document.documentElement.dataset.barTheme)).toBe('dark');
  await shot(main, '07-settings-yap-bar-dark');
  await group.getByRole('tab', { name: 'Light' }).click();
  await expectStore(yap, 'config.json', (c) => c.barTheme === 'light');
  await expect.poll(theme).toBe('light');
  const meetings = settingsDialog(main).getByRole('group', { name: 'Meetings' });
  await meetings.scrollIntoViewIfNeeded();
  await expect(
    meetings.getByRole('button', { name: 'Start notes automatically after 10 seconds' })
  ).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '08-settings-meetings-auto-start');
  await closeSettings(main);
});

test('a call prompt is a "Meeting detected" card; answering one place answers both', async ({ yap, main, shot }) => {
  const bar = yap.overlay;
  // The main window never counts as focused in a test run, so the prompt
  // goes on the bar instead of a Windows notification (and in the main
  // window's toasts too while that's on screen).
  await callSim(yap, 'discord', true); // not asked about by default…
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.calls.length)).toBe(1);
  expect((await yap.invoke('bar_status')).cards).toEqual([]);
  await callSim(yap, 'discord', false);
  await callSim(yap, 'teams', true); // …Teams is
  const card = callCard(bar, 'Teams call');
  await expect(card).toBeVisible();
  await expect(card).toContainText('Now');
  await expect(card.getByRole('button', { name: 'Record notes' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Close' })).toBeVisible();
  const prompt = (await yap.invoke('meeting_detect_status')).prompt;
  expect(prompt).toMatchObject({ app: 'teams', kind: 'start' });
  if (prompt.inApp) await expect(main.getByRole('status').filter({ hasText: 'Teams call detected' })).toBeVisible();
  // The app's own icon, as installed here (CallAppIcon, as in Settings).
  const teamsIcon = card.locator('[data-app-icon="teams"]');
  await expect(teamsIcon).toHaveAttribute('data-icon', 'installed');
  await expect(teamsIcon.locator('img')).toBeVisible();
  await bar.waitForTimeout(300); // the card's entrance
  await shot(bar, '09-call-card');
  await shot(card, '09-call-card-app-icon');

  // The card takes the pointer (and clicks) while the cursor is on it; its
  // ^ menu holds the other answers.
  await simulate(yap, { pointer: 'card:call' });
  await expect.poll(() => debug(yap).then((d) => d.interactive)).toBe(true);
  await card.getByRole('button', { name: 'More answers' }).click();
  await expect(card.getByRole('menuitem', { name: 'Not now' })).toBeVisible();
  await expect(card.getByRole('menuitem', { name: "Don't ask for Teams" })).toBeVisible();
  await shot(bar, '10-call-card-menu');
  // "Not now" there answers the prompt: the toast goes too.
  await card.getByRole('menuitem', { name: 'Not now' }).click();
  await expect(card).toHaveCount(0);
  await expect(main.getByRole('status').filter({ hasText: 'Teams call detected' })).toHaveCount(0);
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.prompt)).toBeNull();
  expect((await yap.invoke('meeting_state')).recording).toBe(false);
  await callSim(yap, 'teams', false);

  // An app that isn't installed shows its bundled mark: Google Meet's, here.
  await simulate(yap, { pointer: 'away' });
  await callSim(yap, 'meet', true);
  const meet = callCard(bar, 'Google Meet call');
  await expect(meet).toBeVisible();
  await expect(meet.locator('[data-app-icon="meet"]')).toHaveAttribute('data-icon', 'mark');
  await bar.waitForTimeout(300);
  await shot(bar, '11-call-card-google-meet');
  // Its corner ✕ is "Not now".
  await meet.getByRole('button', { name: 'Close' }).click();
  await expect(meet).toHaveCount(0);
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.prompt)).toBeNull();
  await callSim(yap, 'meet', false);
});

test("a notice card has Wispr's measurements in Yap's colours, 26 px above the pill", async ({ yap, shot }) => {
  const bar = yap.overlay;
  // The update card (a demo: a test build never downloads an update).
  await simulate(yap, { card: 'update' });
  const card = bar.getByRole('status').filter({ hasText: 'Yap 0.2.0 is ready' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('Update'); // its chip
  await expect(card.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Later' })).toBeVisible();
  const measure = () =>
    card.evaluate((el) => {
      const r = el.getBoundingClientRect();
      const pill = document.querySelector('.pill').getBoundingClientRect();
      const s = getComputedStyle(el);
      const style = (sel) => getComputedStyle(el.querySelector(sel));
      const close = el.querySelector('.close');
      const strip = el.parentElement.querySelector('.strip').getBoundingClientRect();
      return {
        width: r.width,
        aboveThePill: Math.round(pill.top - r.bottom),
        bg: s.backgroundColor,
        border: `${s.borderTopWidth} ${s.borderTopColor}`,
        radius: s.borderTopLeftRadius,
        padding: s.paddingTop,
        close: [close.offsetWidth, close.offsetHeight, style('.close').opacity],
        chip: style('.chip').backgroundColor,
        title: [style('.title').fontSize, style('.title').fontWeight],
        body: [style('.body').fontSize, style('.body').color],
        cream: [style('.cream').backgroundColor, style('.cream').borderTopLeftRadius],
        ghost: [style('.ghost').color, style('.ghost').borderTopLeftRadius],
        strip: Math.round(strip.height),
      };
    });
  await expect.poll(measure).toEqual({
    width: 400,
    aboveThePill: 26,
    bg: 'rgb(255, 255, 255)', // Yap's bar colours, light (--bar-*, app.css)
    border: '1px rgb(220, 215, 203)',
    radius: '16px',
    padding: '20px',
    close: [24, 24, '0.3'],
    chip: 'rgb(246, 169, 60)',
    title: ['15px', '600'],
    body: ['15px', 'rgb(110, 106, 95)'],
    cream: ['rgb(38, 35, 28)', '8px'],
    ghost: ['rgba(35, 33, 27, 0.82)', '8px'],
    strip: 4,
  });
  await shot(bar, '12-update-card');
  // "Later" just closes it.
  await card.getByRole('button', { name: 'Later' }).click();
  await expect(card).toHaveCount(0);
});

test('fullscreen: the pill hides; a card shows over a borderless app, waits out an exclusive one', async ({ yap, main }) => {
  const bar = yap.overlay;
  // Borderless (a game in a borderless window, a video, F11): no pill…
  await simulate(yap, { fullscreen: 'borderless' });
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(false);
  expect((await yap.invoke('bar_status')).fullscreen).toBe('borderless');
  // …but a call still gets its card, over the app, without the pill.
  await callSim(yap, 'whereby', true);
  const card = callCard(bar, 'Whereby call');
  await expect(card).toBeVisible();
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(true);
  await expect(bar.getByLabel('Yap bar')).toHaveCount(0);
  await answerFromMenu(card, 'Not now');
  await expect(card).toHaveCount(0);
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(false);
  await callSim(yap, 'whereby', false);
  await closeToasts(main);

  // Exclusive (a game in exclusive fullscreen, a slideshow): the card waits,
  // unseen, until it's over, then shows.
  await simulate(yap, { fullscreen: 'exclusive' });
  await callSim(yap, 'jitsi', true);
  await expect.poll(() => yap.invoke('bar_status').then((s) => s.cards.map((c) => c.id))).toEqual(['call']);
  expect((await debug(yap)).shown).toBe(false);
  await simulate(yap, { fullscreen: 'none' });
  await expect.poll(() => debug(yap).then((d) => d.shown)).toBe(true);
  const waited = callCard(bar, 'Jitsi Meet call');
  await expect(waited).toBeVisible();
  await expect(bar.getByLabel('Yap bar')).toBeVisible();
  // The call ending takes it away.
  await callSim(yap, 'jitsi', false);
  await expect(waited).toHaveCount(0);
  await closeToasts(main);
});

test('◉ starts meeting notes: the recording pill opens the notepad and ends the meeting', async ({ yap, shot }) => {
  const bar = yap.overlay;
  const notepadShown = () => yap.invoke('plugin:window|is_visible', { label: 'notepad' });
  // Don't let the notepad open by itself when the meeting starts, so the
  // pill's own way to it is what's tested.
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, meetingOpenNotepad: false } });
  await hoverPill(yap);
  await bar.getByRole('button', { name: 'Meeting notes' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((m) => m.recording)).toBe(true);
  const { noteId } = await yap.invoke('meeting_state');
  const note = await yap.invoke('note_get', { id: noteId });
  expect(note.title).toMatch(/^Meeting · \d{1,2} \w{3}, \d\d:\d\d$/);
  expect(note.folder).toBe('Meetings');
  expect(await notepadShown()).toBe(false);

  // The pill becomes the recording pill (Wispr's measurements): 69 × 30,
  // black, a 2 px ring in Yap's recording red, radius 22.5, half opacity
  // until hovered, its bottom 14 px up; 5 bars and the stop circle.
  await pointerAway(yap);
  const open = bar.getByRole('button', { name: 'Open the meeting notes' });
  await expect(open).toBeVisible();
  await expect(open).toContainText(/\d+:\d\d/); // the timer, folded away
  const pillLooks = () =>
    bar.locator('.meeting').evaluate((el) => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      return {
        size: [Math.round(r.width), r.height],
        fromBottom: window.innerHeight - r.bottom,
        opacity: s.opacity,
        border: `${s.borderTopWidth} ${s.borderTopColor}`,
        radius: s.borderTopLeftRadius,
        bars: el.querySelectorAll('.bars i').length,
        stop: el.querySelector('.mstop').getBoundingClientRect().width,
      };
    });
  await expect.poll(pillLooks).toEqual({
    size: [69, 30],
    fromBottom: 14,
    opacity: '0.5',
    border: '2px rgb(229, 100, 94)',
    radius: '22.5px',
    bars: 5,
    stop: 19,
  });
  await bar.waitForTimeout(1_200);
  await shot(bar, '13-meeting-pill');

  // Hovered, it comes up to full opacity and shows the timer.
  await simulate(yap, { pointer: 'pill' });
  await expect.poll(() => pillLooks().then((p) => [p.opacity, p.size[0] > 69])).toEqual(['1', true]);
  await bar.waitForTimeout(300);
  await shot(bar, '14-meeting-pill-hover');

  // Its body opens the meeting notepad on that note…
  await open.click();
  await expect.poll(notepadShown).toBe(true);
  // …and ■ ends the meeting (the action plan is written in Rust).
  await bar.getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((m) => m.recording), { timeout: 20_000 }).toBe(false);
  await expect(bar.getByLabel('Yap bar')).toBeVisible();
  await yap.invoke('save_config', { cfg: { ...(await yap.invoke('get_config')), meetingOpenNotepad: true } });
});

test('the opt-in countdown starts notes by itself; Esc cancels it', async ({ yap, main, shot }) => {
  const bar = yap.overlay;
  await openSettings(main, 'General');
  const meetings = settingsDialog(main).getByRole('group', { name: 'Meetings' });
  await meetings.getByRole('button', { name: 'Start notes automatically after 10 seconds' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingAutoStart === true);
  await closeSettings(main);

  // Esc (watched only while such a card is on screen) cancels: "Not now".
  await callSim(yap, 'zoom', true);
  const card = callCard(bar, 'Zoom call');
  await expect(card).toContainText(/Notes start in \d/);
  await expect(card.getByRole('button', { name: 'Start now' })).toBeVisible();
  await expect(card).toContainText('Esc');
  await bar.waitForTimeout(1_200); // a second off the ring
  await shot(bar, '15-countdown-card');
  await simulate(yap, { escape: true });
  await expect(card).toHaveCount(0);
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.prompt)).toBeNull();
  expect((await yap.invoke('meeting_state')).recording).toBe(false);
  await callSim(yap, 'zoom', false);
  await closeToasts(main);

  // Left alone, it records once the countdown runs out.
  await callSim(yap, 'webex', true);
  await expect(callCard(bar, 'Webex call')).toBeVisible();
  await expect
    .poll(() => yap.invoke('meeting_state').then((m) => m.recording), { timeout: 20_000 })
    .toBe(true);
  const { noteId } = await yap.invoke('meeting_state');
  expect((await yap.invoke('note_get', { id: noteId })).title).toMatch(/^Webex call · /);
  await callSim(yap, 'webex', false);
  // The call ending asks to stop: on the bar too, with no countdown.
  const end = callCard(bar, 'Webex call ended');
  await expect(end).toBeVisible();
  await expect(end).toContainText('Stop recording and summarise your notes?');
  await expect(end).not.toContainText('Notes start');
  await shot(bar, '16-call-ended-card');
  await end.getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((m) => m.recording), { timeout: 20_000 }).toBe(false);

  await openSettings(main, 'General');
  await meetings.getByRole('button', { name: 'Start notes automatically after 10 seconds' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingAutoStart === false);
  await closeSettings(main);
});

test('dictating: the dictation overlay in the pill\'s place', async ({ yap, main, shot }) => {
  const bar = yap.overlay;
  const mics = await yap.invoke('list_audio_devices');
  await pressHotkey(main);
  if (mics.length > 0) {
    await expect(bar.locator('.capsule')).toBeVisible();
    await expect(bar.getByLabel('Yap bar')).toHaveCount(0);
    expect((await debug(yap)).interactive).toBe(false);
    await bar.waitForTimeout(1_200);
    await shot(bar, '17-dictating');
    await pressHotkey(main);
    await expectStore(yap, 'history.json', (h) => JSON.stringify(h).includes('STT stub'));
  } else {
    // No microphone (CI): the same capsule says so.
    await expect(bar.locator('.capsule')).toContainText('No microphone found');
    await shot(bar, '17-dictating-no-microphone');
  }
  await expect(bar.getByLabel('Yap bar')).toBeVisible({ timeout: 10_000 });
});
