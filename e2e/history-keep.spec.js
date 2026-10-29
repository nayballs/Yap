// Settings → Privacy & data → Keep dictation history: always / 24 hours /
// never (2026-10-29, dictation/history/keep.rs). "24 hours" deletes what's
// older than a day now (asked first) and hourly after; "never" is history
// off (what an older Yap reads); the stats stay whichever (dictation/stats.rs).
import { test as base, expect, openView, openSettings, closeSettings, closeToasts, expectStore } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const MODEL = 'parakeet-tdt-0.6b-v3';
const HISTORY = [
  { ts: NOW - 3 * DAY, raw: 'the old plan', text: 'The old plan.', model: MODEL, app: 'notepad.exe' },
  { ts: NOW - 2 * DAY, raw: 'the older draft', text: 'The older draft.', model: MODEL, app: 'notepad.exe' },
  { ts: NOW - 3600, raw: 'this morning', text: 'This morning.', model: MODEL, app: 'notepad.exe' },
];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'history-keep', data: { 'history.json': HISTORY } });
    },
    { scope: 'worker' },
  ],
});

const keepSelect = (dialog) => dialog.getByRole('combobox', { name: 'Keep dictation history' });
const confirmDialog = (main) => main.getByRole('alertdialog', { name: 'Keep history for 24 hours?' });
const total = (s) => (s?.days || []).reduce((n, d) => n + d.words, 0);

test('24 hours deletes what is older than a day (asked first); the stats stay', async ({ yap, main, shot }) => {
  // The history was counted at start: 3 + 3 + 2 words.
  await expectStore(yap, 'dictation_stats.json', (s) => total(s) === 8);
  const dialog = await openSettings(main, 'Privacy & data');
  const select = keepSelect(dialog);
  await select.scrollIntoViewIfNeeded();
  await expect(select).toHaveValue('always');
  await expect(dialog.getByText('Your dictations, kept only on this PC. They show in Home')).toBeVisible();

  // Asked first: Cancel leaves everything as it was.
  await select.selectOption('day');
  await expect(confirmDialog(main)).toContainText('2 dictations are older than a day');
  await shot(main, '01-keep-24-hours-asks');
  await confirmDialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(select).toHaveValue('always');
  expect(yap.readJson('history.json')).toHaveLength(3);

  await select.selectOption('day');
  await confirmDialog(main).getByRole('button', { name: 'Keep 24 hours' }).click();
  await expect(select).toHaveValue('day');
  await expectStore(yap, 'history.json', (h) => h.length === 1 && h[0].text === 'This morning.');
  await expectStore(yap, 'config.json', (c) => c.historyKeep === 'day' && c.historyEnabled === true);
  // A copy beside the stats, for when an older Yap's save drops the setting.
  await expectStore(yap, 'dictation_stats.json', (s) => s.historyKeep === 'day' && total(s) === 8);
  await expect(dialog.getByText('Dictations older than 24 hours are deleted, with their audio')).toBeVisible();
  await expect(dialog.getByText('1 dictation saved')).toBeVisible();
  await shot(main, '02-keep-24-hours');
  await closeSettings(main);

  // Home: the day's dictation, the choice said; Insights: every word still counted.
  await openView(main, 'Home');
  await expect(main.locator('.daylist .item')).toHaveCount(1);
  await expect(main.locator('.notice')).toContainText('History keeps the last 24 hours');
  await openView(main, 'Insights');
  await expect(main.locator('.heroNum')).toHaveText('8');
});

test('never turns history off (what an older Yap reads); always turns it back on', async ({ yap, main }) => {
  const dialog = await openSettings(main, 'Privacy & data');
  const select = keepSelect(dialog);
  await select.scrollIntoViewIfNeeded();
  await select.selectOption('never');
  await expectStore(yap, 'config.json', (c) => c.historyKeep === 'never' && c.historyEnabled === false);
  await expect(dialog.getByText("New dictations aren't kept. Home shows nothing new")).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Keep recent dictation audio' })).toBeDisabled();
  await closeSettings(main);

  // A dictation: not in history, counted in the stats.
  const kept = yap.readJson('history.json').length;
  const before = total(yap.readJson('dictation_stats.json'));
  await yap.invoke('e2e_dictation', { text: 'not kept but counted' });
  await expectStore(yap, 'dictation_stats.json', (s) => total(s) === before + 4);
  expect(yap.readJson('history.json')).toHaveLength(kept);
  await openView(main, 'Home');
  await expect(main.locator('.notice')).toContainText('History is turned off');

  await openSettings(main, 'Privacy & data');
  await keepSelect(settingsDialogOf(main)).selectOption('always');
  await expectStore(yap, 'config.json', (c) => c.historyKeep === 'always' && c.historyEnabled === true);
  await closeSettings(main);
  await closeToasts(main);
});

const settingsDialogOf = (main) => main.getByRole('dialog', { name: 'Settings' });
