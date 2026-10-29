// Insights' stats (2026-10-29, dictation/stats.rs): counted on this PC in
// dictation_stats.json — numbers only, a row per day — whether or not
// history keeps the dictations, so they outlast clearing it.
//
// - The history already there is counted the first time (never an Upload).
// - Words a minute over speaking time (a dictation's audio length, on its
//   history entry as `secs`); "time saved" uses it, and says so.
// - Day and week streaks (the longest too); the words AI cleanup changed,
//   the dictionary's fixes, this month against last.
// - "You've dictated 100 words", once (dictation/milestones.rs), with See Insights.
// - Share: the numbers as a picture, copied or saved (dictation/stats/share.rs).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, closeToasts, expectStore, setConfig } from './support/fixtures.js';
import { SCREENSHOTS } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const MODEL = 'parakeet-tdt-0.6b-v3';
const HISTORY = [
  // Two days ago: its words changed by cleanup (3 words), and a plain one.
  { ts: NOW - 2 * DAY, raw: 'we need to fix the login bug', text: 'We must fix the sign-in bug.', said: 'we need to fix the login bug', model: MODEL, app: 'slack.exe' },
  { ts: NOW - 2 * DAY + 60, raw: 'send it now', text: 'Send it now.', model: MODEL, app: 'notepad.exe' },
  // An hour ago, timed.
  { ts: NOW - 3600, raw: 'lunch at noon', text: 'Lunch at noon.', model: MODEL, app: 'slack.exe', secs: 2.5 },
  // An Upload (a file's name where a dictation has an app): never counted.
  { ts: NOW - 1800, raw: 'a long podcast transcript', text: 'A long podcast transcript.', model: MODEL, app: 'interview.mp3' },
];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'insights-stats',
        data: { 'history.json': HISTORY },
        prepare: (dataDir) => {
          // The model "installed", for Transcribe (the stub engine never reads it).
          const model = path.join(dataDir, 'models', 'parakeet-tdt-0.6b-v3-int8');
          fs.mkdirSync(model, { recursive: true });
          fs.writeFileSync(path.join(model, 'encoder-model.int8.onnx'), 'stand-in');
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** A day `back` days ago on this PC's calendar: "2026-10-29". */
function dayKey(back = 0) {
  const d = new Date(Date.now() - back * DAY * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
const row = (back, fields) => ({ date: dayKey(back), words: 0, dictations: 0, ...fields });
const stats = (yap) => yap.readJson('dictation_stats.json');
const total = (s) => (s?.days || []).reduce((n, d) => n + d.words, 0);
const card = (main, label) => main.locator('.insights .card').filter({ hasText: label });
const toastWith = (main, title) => main.getByRole('status').filter({ hasText: title });

test('the history already there is counted, never an Upload, and the stats outlast clearing it', async ({ yap, main, shot }) => {
  // Counted at start: 6 + 3 + 3 words, the upload left out.
  await expectStore(yap, 'dictation_stats.json', (s) => total(s) === 12 && s.countedTo === NOW - 3600);
  expect(stats(yap).days.some((d) => d.apps?.['interview.mp3'])).toBe(false);
  // Never a word said: numbers, dates and app names only.
  const raw = fs.readFileSync(path.join(yap.dataDir, 'dictation_stats.json'), 'utf8');
  for (const said of ['login', 'lunch', 'Send it', 'podcast']) expect(raw).not.toContain(said);

  await openView(main, 'Insights');
  await expect(main.locator('.heroNum')).toHaveText('12');
  // One dictation timed: too little to say a speed.
  await expect(card(main, 'words a minute').locator('.num')).toHaveText('–');
  await expect(main.locator('.how')).toContainText('until Yap has timed a few of your dictations');
  await expect(card(main, 'words AI cleanup changed').locator('.num')).toHaveText('3');
  await expect(main.getByRole('heading', { name: 'Where you dictate' })).toBeVisible();
  await shot(main, '01-insights-from-history');

  // Home: the same numbers; no meetings card for someone with no calendar and no meetings.
  await openView(main, 'Home');
  await expect(main.locator('.statcard .stat').first()).toContainText('12');
  await expect(main.getByRole('region', { name: 'Meetings' })).toHaveCount(0);

  // History cleared: Home's feed empties, the numbers stay.
  await yap.invoke('clear_history');
  await expect(main.locator('.daylist .item')).toHaveCount(0);
  await expect(main.locator('.statcard .stat').first()).toContainText('12');
  await openView(main, 'Insights');
  await expect(main.locator('.heroNum')).toHaveText('12');
});

test('words a minute over speaking time, the week streaks and the fixes', async ({ yap, main, shot }) => {
  // Four timed dictations, 120 words over a minute of speech; three weeks in
  // a row now, four in a row in the summer.
  await yap.invoke('e2e_stats_set', {
    store: {
      days: [
        row(91, { words: 40, dictations: 2 }),
        row(84, { words: 40, dictations: 2 }),
        row(77, { words: 40, dictations: 2 }),
        row(70, { words: 40, dictations: 2 }),
        row(14, { words: 30, dictations: 1, timed: 1, timedWords: 30, speakingSecs: 15, apps: { 'slack.exe': 30 } }),
        row(7, { words: 30, dictations: 1, timed: 1, timedWords: 30, speakingSecs: 15, apps: { 'code.exe': 30 }, cleanupWords: 2 }),
        row(0, { words: 60, dictations: 2, timed: 2, timedWords: 60, speakingSecs: 30, apps: { 'slack.exe': 60 }, cleanupWords: 5, dictionaryFixes: 3 }),
      ],
      milestone: 100,
    },
  });
  await openView(main, 'Insights');
  await expect(main.locator('.heroNum')).toHaveText('280');
  await expect(card(main, 'words a minute').locator('.num')).toHaveText('120');
  await expect(main.locator('.how')).toHaveText('You talk at 120 words a minute, timed over 4 dictations; most people type about 40.');
  await expect(card(main, 'words today').locator('.num')).toHaveText('60');
  await expect(card(main, 'week streak').locator('.num')).toHaveText('3');
  await expect(card(main, 'week streak')).toContainText('longest 4');
  await expect(card(main, 'words AI cleanup changed').locator('.num')).toHaveText('7');
  await expect(card(main, 'words your dictionary fixed').locator('.num')).toHaveText('3');
  await expect(card(main, 'fixes this month')).toContainText(/fixes this month, (up|down|the same|nothing)/);
  // Today's cell is the last of the 30.
  await expect(main.locator('.activity .acell')).toHaveCount(30);
  await expect(main.locator('.activity .acell.today')).toHaveAttribute('data-level', '4');
  await shot(main, '02-insights-light');
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'dark'));
  await main.waitForTimeout(150);
  await shot(main, '03-insights-dark');
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'light'));
});

test('a dictation counts with how long it was said for, kept in history or not', async ({ yap, main }) => {
  await yap.invoke('e2e_stats_set', { store: { days: [] } });
  await openView(main, 'Insights');
  await expect(main.getByRole('heading', { name: 'No insights yet' })).toBeVisible();

  // Through the whole dictation path (the stub's transcript): its audio's length kept.
  const r = await yap.invoke('e2e_dictate_audio', { seconds: 2 });
  expect(r?.text).toBeTruthy();
  await expectStore(yap, 'history.json', (h) => h.at(-1)?.secs >= 1.9 && h.at(-1)?.secs <= 2.5);
  await expectStore(yap, 'dictation_stats.json', (s) => s.days.length === 1 && s.days[0].timed === 1 && s.days[0].speakingSecs >= 1.9);
  await expect(main.locator('.heroNum')).not.toHaveText('0');

  // History off: nothing new in history, still counted.
  await setConfig(yap, main, { historyEnabled: false, historyKeep: 'never' });
  const kept = yap.readJson('history.json').length;
  await yap.invoke('e2e_dictation', { text: 'one two three four' });
  await expectStore(yap, 'dictation_stats.json', (s) => s.days[0].dictations === 2);
  expect(yap.readJson('history.json').length).toBe(kept);
  await expect(card(main, 'dictations').locator('.num')).toHaveText('2');
  await setConfig(yap, main, { historyEnabled: true, historyKeep: 'always' });
});

test('a cancelled dictation counts once it is transcribed, with its speaking time', async ({ yap }) => {
  await yap.invoke('e2e_stats_set', { store: { days: [] } });
  expect(await yap.invoke('e2e_dictate_audio', { seconds: 1.5, cancel: true })).toBeNull();
  await expectStore(yap, 'history.json', (h) => h.at(-1)?.status === 'cancelled');
  // Cancelled: never transcribed, not counted.
  expect(stats(yap).days).toEqual([]);
  const e = yap.readJson('history.json').at(-1);
  await yap.invoke('history_rerun', { ts: e.ts, audio: e.audio, model: null });
  await expectStore(yap, 'dictation_stats.json', (s) => s.days.length === 1 && s.days[0].dictations === 1 && s.days[0].timed === 1 && s.days[0].speakingSecs >= 1.4);
});

test('"You\'ve dictated 100 words": said once, with See Insights', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_stats_set', { store: { days: [row(0, { words: 95, dictations: 5 })], milestone: 0 } });
  await openView(main, 'Home');
  await yap.invoke('e2e_dictation', { text: 'one two three four five six seven eight nine ten' });
  const moment = toastWith(main, "You've dictated 100 words");
  await expect(moment).toBeVisible({ timeout: 10_000 });
  await shot(main, '04-milestone');
  await expectStore(yap, 'dictation_stats.json', (s) => s.milestone === 100);
  await moment.getByRole('button', { name: 'See Insights' }).click();
  await expect(main.getByRole('heading', { level: 1, name: 'Insights' })).toBeVisible();
  await closeToasts(main);

  // Once: the next dictation says nothing.
  await yap.invoke('e2e_dictation', { text: 'and a few more words' });
  await expectStore(yap, 'dictation_stats.json', (s) => s.days[0].dictations === 7);
  await main.waitForTimeout(2_500);
  await expect(toastWith(main, 'You\'ve dictated')).toHaveCount(0);
});

test('Share: the numbers as a picture, copied or saved', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_stats_set', {
    store: { days: [row(1, { words: 900, dictations: 30, timed: 30, timedWords: 900, speakingSecs: 420, cleanupWords: 40, dictionaryFixes: 6 }), row(0, { words: 340, dictations: 12 })], milestone: 1000 },
  });
  await yap.invoke('e2e_snip_clipboard', { clear: true });
  await openView(main, 'Insights');
  await main.getByRole('button', { name: 'Share' }).click();
  await main.getByRole('menuitem', { name: 'Copy as a picture' }).click();
  await expect(toastWith(main, 'Copied your stats as a picture')).toBeVisible();
  const b64 = await yap.invoke('e2e_snip_clipboard', { clear: true });
  const png = Buffer.from(b64, 'base64');
  expect(png.subarray(1, 4).toString()).toBe('PNG');
  expect([png.readUInt32BE(16), png.readUInt32BE(20)]).toEqual([1200, 630]);
  // Kept beside the screenshots, to look at.
  fs.mkdirSync(path.join(SCREENSHOTS, 'insights-stats'), { recursive: true });
  fs.writeFileSync(path.join(SCREENSHOTS, 'insights-stats', '06-share-card.png'), png);

  const saved = path.join(yap.dataDir, 'e2e-insights-card.png');
  fs.rmSync(saved, { force: true });
  await main.getByRole('button', { name: 'Share' }).click();
  await main.getByRole('menuitem', { name: 'Save as a picture…' }).click();
  await expect(toastWith(main, 'Saved your stats as a picture')).toBeVisible();
  expect(fs.readFileSync(saved).subarray(1, 4).toString()).toBe('PNG');
  await shot(main, '05-share-saved');
  await closeToasts(main);
});
