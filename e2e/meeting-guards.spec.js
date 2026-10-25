// Meeting guard rails (src-tauri/src/calls/meeting_guard.rs, calls/capture.rs): Yap's
// meeting windows leave screen captures while a meeting records (and Yap
// warns when that's switched off), the maximum recording length (its
// warning, Keep going, the stop), "When a call ends → Stop and summarise
// automatically", the meeting shortcut, and their rows in Settings → General
// → Meetings.
// Test mode: a recording opens no audio device (it's silent), calls come
// from the debug-only `meeting_detect_simulate`, the length timings from the
// debug-only `e2e_meeting_limit`, window affinities from the debug-only
// `capture_affinity`, and the shortcut reaches the app the way the main
// window's in-page fallback sees it (the global hook is off in test mode).
import {
  test,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  settingsDialog,
} from './support/fixtures.js';
import { newMeetingNote } from './support/meetings.js';

test.use({ yapOptions: { name: 'meeting-guards' } });

const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const overlayAffinity = (yap) => yap.invoke('capture_affinity').then((a) => a.windows.overlay);
const simulate = (yap, appId, active) => yap.invoke('meeting_detect_simulate', { appId, active });
/** Settings → Meetings (its own page since 2026-10-22). */
const meetingsCard = (main) => settingsDialog(main).locator('.inner');
const meetingNotes = (yap) => (yap.readJson('notes.json')?.notes ?? []).filter((n) => n.source === 'meeting');
/** WDA_EXCLUDEFROMCAPTURE: on screen, left out of every capture. */
const EXCLUDED = 17;
/**
 * A silent test recording's end: so few words that Yap asks "Started by
 * mistake?" instead of writing a summary (meetings/meeting_end.rs).
 */
const SUMMARY_STEP = /Nothing to summarise yet|Started by mistake\?/;

/**
 * The end of a meeting Yap stopped itself: "Started by mistake?" in the
 * meeting notepad (on screen while a meeting records), or the main window.
 * No window is brought up for it.
 */
async function expectAutoStopEnded(yap, main) {
  const asked = async () =>
    (await toast(main, SUMMARY_STEP).count()) + (await toast(yap.notepad, SUMMARY_STEP).count());
  await expect.poll(asked, { timeout: 20_000 }).toBeGreaterThan(0);
}

/** Win+Alt+M, the meeting shortcut, typed into the main window. */
const pressMeetingShortcut = (main) => main.keyboard.press('Meta+Alt+KeyM');

/** End the recording from the note and wait for the summary step. */
async function endMeeting(yap, main) {
  await main.getByRole('button', { name: 'End meeting & summarise' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, SUMMARY_STEP)).toBeVisible({ timeout: 20_000 });
}

test('the overlay leaves screen captures while a meeting records, and comes back after', async ({
  yap,
  main,
  shot,
}) => {
  const before = await yap.invoke('capture_affinity');
  expect(before).toMatchObject({ hidden: false, windows: { overlay: 0, settings: 0 } });

  await newMeetingNote(main, 'Capture check');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => recording(yap)).toBe(true);
  await expect.poll(() => overlayAffinity(yap)).toBe(EXCLUDED);
  const during = await yap.invoke('capture_affinity');
  expect(during.hidden).toBe(true);
  // The main window stays shareable; the meeting notepad is hidden like the
  // overlay.
  expect(during.windows.settings).toBe(0);
  expect(during.windows.notepad).toBe(EXCLUDED);
  // Hiding is the default: no screen-share warning.
  await expect(toast(main, 'Your meeting notes show up in screen shares')).toHaveCount(0);
  await shot(main, '01-recording-overlay-hidden');

  await endMeeting(yap, main);
  await expect.poll(() => overlayAffinity(yap)).toBe(0);
  expect((await yap.invoke('capture_affinity')).hidden).toBe(false);
  await closeToasts(main);
});

test('with hiding off, a meeting starting warns, and Update settings goes to Meetings', async ({
  yap,
  main,
  shot,
}) => {
  await openSettings(main, 'Meetings');
  const hide = meetingsCard(main).getByRole('button', { name: 'Hide Yap from screen sharing' });
  await expect(hide).toHaveAttribute('aria-pressed', 'true');
  await hide.click();
  await expectStore(yap, 'config.json', (c) => c.meetingHideFromCapture === false);
  await closeSettings(main);

  await newMeetingNote(main, 'Shared screen');
  await main.getByRole('button', { name: 'Record' }).click();
  const tip = toast(main, 'Your meeting notes show up in screen shares and screenshots.');
  await expect(tip).toBeVisible();
  await expect(tip).toContainText('Screen sharing');
  await tip.hover(); // its timer waits while the pointer is on it
  await shot(main, '02-screen-share-warning');
  expect(await recording(yap)).toBe(true);
  expect(await overlayAffinity(yap)).toBe(0);

  // "Update settings" opens Meetings at the switch.
  await tip.getByRole('button', { name: 'Update settings' }).click();
  await expect(settingsDialog(main)).toBeVisible();
  await expect(
    settingsDialog(main).getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: /^Meetings/ })
  ).toHaveAttribute('aria-current', 'page');
  await expect(hide).toBeInViewport();
  await expect(hide).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '03-update-settings-meetings');

  // Switched back on mid-meeting: hidden at once.
  await hide.click();
  await expectStore(yap, 'config.json', (c) => c.meetingHideFromCapture === true);
  await expect.poll(() => overlayAffinity(yap)).toBe(EXCLUDED);
  await closeSettings(main);
  await endMeeting(yap, main);
  await expect.poll(() => overlayAffinity(yap)).toBe(0);
  await closeToasts(main);
});

test('the maximum length warns, Keep going moves it, and at the limit Yap stops and summarises', async ({
  yap,
  main,
  shot,
}) => {
  // Seconds instead of hours (debug-only): stop 6 s in, warn 4 s before,
  // Keep going adds 6 s.
  await yap.invoke('e2e_meeting_limit', { limitMs: 6_000, warnMs: 4_000, keepGoingMs: 6_000 });
  try {
    await newMeetingNote(main, 'Long meeting');
    await main.getByRole('button', { name: 'Record' }).click();
    const warning = toast(main, /Notes stop in \d+ seconds?/);
    await expect(warning).toBeVisible({ timeout: 10_000 });
    await expect(warning).toContainText('Yap will stop recording and write your action plan.');
    await expect(warning).toContainText('Keep going gives you 6 seconds more.');
    expect((await yap.invoke('meeting_limit_status')).warning).toMatchObject({
      title: expect.stringMatching(/^Notes stop in \d+ seconds?$/),
    });
    await shot(main, '04-length-warning');

    await warning.getByRole('button', { name: 'Keep going' }).click();
    await expect(warning).toHaveCount(0);
    expect((await yap.invoke('meeting_limit_status')).warning).toBeNull();
    expect(await recording(yap)).toBe(true);

    // Warned again before the new limit; left alone, it stops and the
    // action plan step runs, as after "End meeting & summarise".
    await expect(warning).toBeVisible({ timeout: 10_000 });
    const stopped = toast(main, /Stopped at \d+ seconds/);
    await expect(stopped).toBeVisible({ timeout: 15_000 });
    await expect(stopped).toContainText('The meeting reached the maximum recording length.');
    await expect(warning).toHaveCount(0);
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    await expectAutoStopEnded(yap, main);
    await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Long meeting');
    await shot(main, '05-stopped-at-limit');
  } finally {
    await yap.invoke('e2e_meeting_limit', {});
  }
  await closeToasts(main);
  await closeToasts(yap.notepad);
});

test('"When a call ends: Stop and summarise automatically" stops without asking', async ({
  yap,
  main,
  shot,
}) => {
  await openSettings(main, 'Meetings');
  const callEnd = meetingsCard(main).getByRole('combobox', { name: 'When a call ends' });
  await expect(callEnd.locator('option:checked')).toHaveText('Ask me');
  await callEnd.selectOption({ label: 'Stop and summarise' });
  await expectStore(yap, 'config.json', (c) => c.meetingCallEnd === 'stop');
  await expect(meetingsCard(main)).toContainText('Yap stops and writes your action plan, no question asked');
  await closeSettings(main);

  await simulate(yap, 'zoom', true);
  await toast(main, 'Zoom call detected').getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  const note = meetingNotes(yap).at(-1);
  expect(note.title).toMatch(/^Zoom call · /);

  // The call ends: no "Stop and summarise?" — it stops, and says so.
  await simulate(yap, 'zoom', false);
  const notice = toast(main, 'Yap stopped recording and is writing your action plan.');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('Zoom call ended');
  await expect(main.getByRole('button', { name: 'Stop and summarise' })).toHaveCount(0);
  expect((await yap.invoke('meeting_detect_status')).prompt).toBeNull();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expectAutoStopEnded(yap, main);
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(note.title);
  await shot(main, '06-call-ended-stopped-automatically');
  await closeToasts(main);
  await closeToasts(yap.notepad);

  // Back to asking.
  await openSettings(main, 'Meetings');
  await callEnd.selectOption({ label: 'Ask me' });
  await expectStore(yap, 'config.json', (c) => c.meetingCallEnd === 'ask');
  await closeSettings(main);
});

test('the meeting shortcut starts notes in a new meeting note, and stops them', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  await pressMeetingShortcut(main);

  // A meeting note of its own opens in Notes, recording.
  const title = main.getByPlaceholder('Untitled Note');
  await expect(title).toHaveValue(/^Meeting · \d{1,2} [A-Z][a-z]{2}, \d{2}:\d{2}$/, { timeout: 15_000 });
  await expect.poll(() => recording(yap)).toBe(true);
  const started = toast(main, 'Press Win + Alt + M again to stop and write the action plan.');
  await expect(started).toBeVisible();
  await expect(started).toContainText('Taking notes');
  const note = meetingNotes(yap).at(-1);
  expect(note).toMatchObject({ folder: 'Meetings', noteType: 'meeting', source: 'meeting' });
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: true, noteId: note.id });
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();
  await shot(main, '07-shortcut-started');

  // Again: stop and write the action plan. (Presses closer than a second
  // apart count as one.) Pressed in the main window, so it asks there.
  await main.waitForTimeout(1_100);
  await pressMeetingShortcut(main);
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, SUMMARY_STEP)).toBeVisible({ timeout: 20_000 });
  await expect(title).toHaveValue(note.title);
  await shot(main, '08-shortcut-stopped');
  await closeToasts(main);
});

test('with a call going on, the meeting shortcut takes notes on that call', async ({ yap, main }) => {
  await simulate(yap, 'webex', true);
  const prompt = toast(main, 'Webex call detected');
  await expect(prompt).toBeVisible();

  await pressMeetingShortcut(main);
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(/^Webex call · /, { timeout: 15_000 });
  await expect.poll(() => recording(yap)).toBe(true);
  await expect(prompt).toHaveCount(0); // the shortcut answered it
  await expect(toast(main, 'Taking notes on your Webex call')).toBeVisible();

  await main.waitForTimeout(1_100);
  await pressMeetingShortcut(main);
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, SUMMARY_STEP)).toBeVisible({ timeout: 20_000 });
  await simulate(yap, 'webex', false);
  await closeToasts(main);
});

test('Settings → Meetings has the guard rails, and the shortcut can be changed', async ({
  yap,
  main,
  shot,
}) => {
  await openSettings(main, 'Meetings');
  const card = meetingsCard(main);
  const callEnd = card.getByRole('combobox', { name: 'When a call ends' });
  await callEnd.scrollIntoViewIfNeeded();
  await expect(callEnd.locator('option:checked')).toHaveText('Ask me');
  await expect(card).toContainText('Yap asks whether to stop recording and summarise');
  const maxLength = card.getByRole('combobox', { name: 'Maximum recording length' });
  await expect(maxLength.locator('option:checked')).toHaveText('2 hours');
  await expect(card.getByRole('button', { name: 'Hide Yap from screen sharing' })).toHaveAttribute('aria-pressed', 'true');
  // The shortcut in Yap's keycaps, with Change.
  const row = card.locator('[data-setting="meeting-shortcut"]');
  await expect(row).toContainText('Take notes on a call, and press again to stop and summarise');
  await expect(row.locator('kbd')).toHaveText(['Win', 'Alt', 'M']);
  await card.getByText('Meeting shortcut').scrollIntoViewIfNeeded();
  await shot(main, '09-settings-meeting-guards');

  // The length: one hour, then back.
  await maxLength.selectOption({ label: '1 hour' });
  await expectStore(yap, 'config.json', (c) => c.meetingMaxMinutes === 60);
  await maxLength.selectOption({ label: '2 hours' });
  await expectStore(yap, 'config.json', (c) => c.meetingMaxMinutes === 120);

  // A new shortcut, recorded like the dictation key; then Win+Alt+M again.
  const shortcut = row.getByRole('button', { name: /^Win\s*Alt\s*M\s*Change$/ });
  await shortcut.click();
  await main.keyboard.press('Control+Alt+KeyK');
  await expectStore(yap, 'config.json', (c) => c.meetingHotkey === 'kb:ctrl+alt+75');
  await expect(row.locator('kbd')).toHaveText(['Ctrl', 'Alt', 'K']);
  // The Shortcuts page shows the same, with the other three.
  await openSettings(main, 'Shortcuts');
  await expect(settingsDialog(main).locator('[data-setting="shortcut-meeting"] kbd')).toHaveText(['Ctrl', 'Alt', 'K']);
  await openSettings(main, 'Meetings');
  await row.getByRole('button', { name: /^Ctrl\s*Alt\s*K\s*Change$/ }).click();
  await main.keyboard.press('Meta+Alt+KeyM');
  await expectStore(yap, 'config.json', (c) => c.meetingHotkey === 'kb:alt+win+77');
  await expect(shortcut).toBeVisible();
});

test('little disk left: "About N minutes of recording left", then a clean stop when it runs out', async ({
  yap,
  main,
  shot,
}) => {
  // The drive's free space and the meeting's bytes a minute, stood in for
  // (debug-only `e2e_disk_space`, meetings/disk_watch.rs): 25 minutes above
  // the 200 MB Yap keeps for saving, at 1 MB a minute.
  const MB = 1024 * 1024;
  const RESERVE = 200 * MB;
  await yap.invoke('e2e_disk_space', { free: RESERVE + 25 * MB, perMin: MB });
  try {
    await newMeetingNote(main, 'Low disk');
    await main.getByRole('button', { name: 'Record' }).click();
    const left = toast(main, 'About 25 minutes of recording left on this drive');
    await expect(left).toBeVisible({ timeout: 10_000 });
    await expect(left).toContainText('Your disk is almost full');
    await shot(main, '13-low-disk-warning');
    expect(await recording(yap)).toBe(true);
    await closeToasts(main);

    // Near the end: said once more.
    await yap.invoke('e2e_disk_space', { free: RESERVE + 6 * MB, perMin: MB });
    await expect(toast(main, 'About 6 minutes of recording left on this drive')).toBeVisible({ timeout: 10_000 });
    expect(await recording(yap)).toBe(true);
    await closeToasts(main);

    // Out of space: the recording stops, keeping everything, and says why.
    await yap.invoke('e2e_disk_space', { free: RESERVE - MB, perMin: MB });
    const stopped = toast(main, 'Stopped: your disk is full');
    await expect(stopped).toBeVisible({ timeout: 10_000 });
    await expect(stopped).toContainText('everything up to now is kept');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    await expectAutoStopEnded(yap, main);
    expect(yap.readJson('notes.json').notes.some((n) => n.title === 'Low disk' && n.noteType === 'meeting')).toBe(true);
    await shot(main, '14-stopped-disk-full');
  } finally {
    await yap.invoke('e2e_disk_space', {});
  }
  await closeToasts(main);
  await closeToasts(yap.notepad);
});
