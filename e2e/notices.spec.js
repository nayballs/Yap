// Yap's notice policy (src-tauri/src/notices/notice.rs + src/lib/ui/noticePolicy.js,
// after Wispr Flow's rules, by behaviour): one notice in one place, one at a
// time, the most important first; tips and notices held back while a
// dictation runs; the same notice again within its cooldown not shown; tips
// that can be switched off; and call prompts that offer to stop asking
// after three "Not now"s in a row.
//
// Test mode: the debug-only `e2e_notice` puts a notice up through the real
// policy — a Yap bar card (`surface: 'bar'`, its `kind` = the card's id and
// rule), or the main window's toast (`surface: 'window'`, `level` = the
// page's kind) — and stands in for a dictation (`dictating`: the pipeline's
// `yap-state`, which the bar and the toasts follow) so no microphone is
// needed. `e2e_notice_reset` (before every test, in the `main` fixture)
// forgets the cooldowns and fatigue counts.
import {
  test,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  inAnotherApp,
  settingsDialog,
  mainOnScreen,
} from './support/fixtures.js';

test.use({ yapOptions: { name: 'notices' } });

const notice = (yap, args) => yap.invoke('e2e_notice', args);
const bar = (yap) => yap.invoke('bar_status');
const cardIds = (yap) => bar(yap).then((s) => s.cards.map((c) => c.id));
const toasts = (page) => page.getByRole('status');
const toast = (page, text) => page.getByRole('status').filter({ hasText: text });
const dictating = (yap, on) => notice(yap, { dictating: on });

test.afterEach(async ({ yap }) => {
  await dictating(yap, false).catch(() => {});
  // Whatever a test left on the bar, gone.
  for (const id of [...(await cardIds(yap)), ...(await bar(yap)).waiting]) {
    await yap.invoke('bar_card_action', { id, action: '' }).catch(() => {});
  }
});

test('the Yap bar shows one card at a time: an alert before a notice, the notice after it', async ({ yap, shot }) => {
  expect(await notice(yap, { surface: 'bar', kind: 'meeting-notice', title: 'Stopped at 2 hours', body: 'Yap is writing your action plan.' })).toBe(true);
  expect(await notice(yap, { surface: 'bar', kind: 'pasteBlocked', title: 'Your text is copied: press Ctrl+V', body: 'An app running as administrator.' })).toBe(true);
  // The alert matters more: it's up, the notice waits its turn.
  await expect.poll(() => cardIds(yap)).toEqual(['pasteBlocked']);
  expect((await bar(yap)).waiting).toEqual(['meeting-notice']);
  const cards = toasts(yap.overlay);
  await expect(cards).toHaveCount(1);
  await expect(cards).toContainText('Your text is copied');
  await yap.overlay.waitForTimeout(300); // the card's entrance
  await shot(yap.overlay, '01-bar-one-card');

  // Answered: the notice's turn.
  await yap.invoke('bar_card_action', { id: 'pasteBlocked', action: '' });
  await expect.poll(() => cardIds(yap)).toEqual(['meeting-notice']);
  await expect(cards).toHaveCount(1);
  await expect(cards).toContainText('Stopped at 2 hours');
  expect((await bar(yap)).waiting).toEqual([]);
});

test('the main window shows one notice at a time, and the answer to a click beside it', async ({ yap, main, shot }) => {
  await notice(yap, { surface: 'window', kind: 'e2e-a', level: 'info', title: 'First notice', body: 'Up first.', timeoutMs: 60_000 });
  await notice(yap, { surface: 'window', kind: 'e2e-b', level: 'info', title: 'Second notice', body: 'Waits its turn.', timeoutMs: 60_000 });
  await expect(toast(main, 'First notice')).toBeVisible();
  await main.waitForTimeout(500);
  await expect(toast(main, 'Second notice')).toHaveCount(0);
  await expect(toasts(main)).toHaveCount(1);

  // The same notice again doesn't stack: it's the one up, refreshed.
  await notice(yap, { surface: 'window', kind: 'e2e-a', level: 'info', title: 'First notice', body: 'Up first.', timeoutMs: 60_000 });
  await main.waitForTimeout(300);
  await expect(toasts(main)).toHaveCount(1);

  // A click's answer ("Copied") isn't queued behind a notice: it shows
  // beside it, in its own place.
  await openView(main, 'Integrations');
  await main.getByRole('button', { name: 'Copy API guide for agents' }).click();
  await expect(toast(main, 'API guide copied')).toBeVisible();
  await expect(toasts(main)).toHaveCount(2);
  await shot(main, '02-window-one-notice-one-answer');

  // The first notice closed: the second one's turn.
  await toast(main, 'First notice').getByRole('button', { name: 'Close' }).click();
  await expect(toast(main, 'Second notice')).toBeVisible();
  await expect(toast(main, 'First notice')).toHaveCount(0);
  await closeToasts(main);
  await openView(main, 'Home');
});

test('nothing but alerts and questions pops up while you dictate; the rest waits till it ends', async ({ yap, main, shot }) => {
  await dictating(yap, true);
  // A notice on the bar and one in the window: held back.
  await notice(yap, { surface: 'bar', kind: 'meeting-notice', title: 'Yap can’t hear the call', body: 'Held until the dictation ends.' });
  await notice(yap, { surface: 'window', kind: 'e2e-held', level: 'info', title: 'Held in the window', body: 'Shown after the dictation.' });
  // An alert about the dictation itself: at once.
  await notice(yap, { surface: 'bar', kind: 'pasteBlocked', title: 'Your text is copied: press Ctrl+V', body: 'An app running as administrator.' });
  await expect.poll(() => cardIds(yap)).toEqual(['pasteBlocked']);
  expect((await bar(yap)).waiting).toEqual(['meeting-notice']);
  await main.waitForTimeout(800);
  await expect(toast(main, 'Held in the window')).toHaveCount(0);
  await yap.overlay.waitForTimeout(300);
  await shot(yap.overlay, '03-only-the-alert-while-dictating');
  await yap.invoke('bar_card_action', { id: 'pasteBlocked', action: '' });
  // Still dictating: the notice keeps waiting.
  await main.waitForTimeout(500);
  expect(await cardIds(yap)).toEqual([]);
  expect((await bar(yap)).waiting).toEqual(['meeting-notice']);

  // The dictation ends: after a moment (the paste), both show.
  await dictating(yap, false);
  await expect.poll(() => cardIds(yap), { timeout: 5_000 }).toEqual(['meeting-notice']);
  await expect(toast(main, 'Held in the window')).toBeVisible({ timeout: 5_000 });
  await closeToasts(main);
});

test('the same notice again within its cooldown is not shown; a different one is', async ({ yap, main }) => {
  const stopped = { surface: 'bar', kind: 'meeting-notice', title: 'Stopped: the meeting went quiet', body: 'Nobody had spoken for 10 minutes.' };
  expect(await notice(yap, stopped)).toBe(true);
  await expect.poll(() => cardIds(yap)).toEqual(['meeting-notice']);
  await yap.invoke('bar_card_action', { id: 'meeting-notice', action: '' });
  await expect.poll(() => cardIds(yap)).toEqual([]);
  // Again within ten minutes: taken (it was said), not shown.
  expect(await notice(yap, stopped)).toBe(true);
  await yap.overlay.waitForTimeout(500);
  expect(await cardIds(yap)).toEqual([]);
  expect((await bar(yap)).waiting).toEqual([]);
  // Another notice of the same kind is news: it shows.
  await notice(yap, { ...stopped, title: 'Stopped at 2 hours' });
  await expect.poll(() => cardIds(yap)).toEqual(['meeting-notice']);

  // In the window: Windows' microphone switch, said once in ten minutes
  // (the overlay's line says it on every press).
  const mic = { surface: 'window', kind: 'micBlocked', title: 'Windows is blocking the microphone', body: 'Turn on microphone access.' };
  expect(await notice(yap, mic)).toBe(true);
  expect(await notice(yap, mic)).toBe(false);
  await expect(toast(main, 'Windows is blocking the microphone')).toHaveCount(1);
  await closeToasts(main);
});

test('"Show tips" off: no coaching tips, and everything else still shows', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Notifications' });
  await group.scrollIntoViewIfNeeded();
  const tips = group.getByRole('button', { name: 'Show tips' });
  await expect(tips).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '04-settings-notifications');
  await tips.click();
  await expectStore(yap, 'config.json', (c) => c.showTips === false);
  await closeSettings(main);
  try {
    await inAnotherApp(yap);
    await notice(yap, { surface: 'bar', kind: 'hint', title: 'Hold F24 while you talk', body: 'A tip.' });
    await notice(yap, { surface: 'bar', kind: 'meeting-notice', title: 'Taking notes', body: 'Not a tip.' });
    await expect.poll(() => cardIds(yap)).toEqual(['meeting-notice']);
    expect((await bar(yap)).waiting).toEqual([]);
  } finally {
    await openSettings(main, 'General');
    await settingsDialog(main).getByRole('group', { name: 'Notifications' }).getByRole('button', { name: 'Show tips' }).click();
    await expectStore(yap, 'config.json', (c) => c.showTips === true);
    await closeSettings(main);
  }
});

test('three "Not now"s in a row: Yap offers once to stop asking about calls', async ({ yap, main, shot }) => {
  for (const [app, name] of [['teams', 'Teams'], ['zoom', 'Zoom'], ['webex', 'Webex']]) {
    await mainOnScreen(yap);
    await yap.invoke('meeting_detect_simulate', { appId: app, active: true });
    const prompt = toast(main, `${name} call detected`);
    await expect(prompt).toBeVisible();
    await prompt.getByRole('button', { name: 'Not now' }).click();
    await expect(prompt).toHaveCount(0);
    await yap.invoke('meeting_detect_simulate', { appId: app, active: false });
  }
  const offer = toast(main, 'Stop asking about calls?');
  await expect(offer).toBeVisible();
  await expect(offer).toContainText('Settings → Meetings');
  await shot(main, '05-stop-asking-about-calls');
  await offer.getByRole('button', { name: 'Stop asking' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === false);
  expect((await yap.invoke('meeting_detect_status')).enabled).toBe(false);
  // Settings' copy follows (its auto-save won't switch it back on).
  const dialog = await openSettings(main, 'Meetings');
  await expect(
    dialog.getByRole('group', { name: 'Call detection' }).getByRole('button', { name: 'Detect calls and offer to take notes' })
  ).toHaveAttribute('aria-pressed', 'false');
  await dialog.getByRole('group', { name: 'Call detection' }).getByRole('button', { name: 'Detect calls and offer to take notes' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === true);
  await closeSettings(main);
});
