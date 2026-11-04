// Home's "AI edited · N changes" (2026-10-23): a dictation whose words AI
// cleanup changed says so in its row and opens into what it changed, word
// by word (lib/core/wordDiff.js: history's `said` — the words as said — against
// the text Yap pasted; capitals and punctuation aren't changes), with
// **Paste what I said** and **Copy what I said**.
//
// Paste what I said (pasting/as_said.rs `paste_entry`): the latest dictation, right
// after it, is swapped in the app it went into — a test run only logs that
// it would — and any other has its words as said copied, Yap saying so. The
// real swap from Home is the robot's (e2e-real/as-said.real.js); the diff
// itself is word-diff.spec.js's.
import fs from 'node:fs';
import { test as base, expect, openView, closeToasts, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const NOW = Math.floor(Date.now() / 1000);
const MODEL = 'parakeet-tdt-0.6b-v3';
const HISTORY = [
  // An hour ago, its words changed by cleanup.
  {
    ts: NOW - 3600,
    raw: 'we need to fix the login bug before the demo',
    text: 'We must fix the sign-in bug before the demo.',
    said: 'we need to fix the login bug before the demo',
    model: MODEL,
    app: 'slack.exe',
  },
  // Cleanup only punctuated it: nothing to show.
  { ts: NOW - 3000, raw: 'send it now', text: 'Send it now.', said: 'send it now', model: MODEL, app: 'notepad.exe' },
  // From before Yap kept the words as said: nothing to show either.
  { ts: NOW - 2400, raw: 'um lunch at noon', text: 'Lunch at noon?', model: MODEL, app: 'chrome.exe' },
];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'home-ai-edits', data: { 'history.json': HISTORY } });
    },
    { scope: 'worker' },
  ],
});

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const row = (main, text) => main.locator('.daylist .item').filter({ hasText: text });
const toastWith = (main, title) => main.getByRole('status').filter({ hasText: title });

/** The page's clipboard, stubbed (the real one is never touched). */
async function stubClipboard(page) {
  await page.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  return () => page.evaluate(() => window.__copied);
}

async function configure(yap, patch) {
  await yap.invoke('save_config', { cfg: { ...(await yap.invoke('get_config')), ...patch } });
}

test('a dictation AI cleanup changed says so, and opens into what it changed (light and dark)', async ({ main, shot }) => {
  await openView(main, 'Home');
  const edited = row(main, 'We must fix the sign-in bug before the demo.');
  const pill = edited.getByRole('button', { name: 'AI edited · 2 changes' });
  await expect(pill).toBeVisible();
  await expect(pill).toHaveAttribute('aria-expanded', 'false');
  // Only punctuation, or no words as said kept: nothing to show.
  await expect(row(main, 'Send it now.').locator('.aiedit')).toHaveCount(0);
  await expect(row(main, 'Lunch at noon?').locator('.aiedit')).toHaveCount(0);
  await shot(main, '01-home-ai-edited');

  await pill.click();
  await expect(pill).toHaveAttribute('aria-expanded', 'true');
  const changes = edited.getByRole('region', { name: 'What AI cleanup changed' });
  await expect(changes).toBeVisible();
  await expect(pill).toHaveAttribute('aria-controls', (await changes.getAttribute('id')) ?? '');
  // What was said, struck; what the AI wrote, marked — each told to a screen reader.
  await expect(changes.locator('del')).toHaveText(['taken out: need to', 'taken out: login']);
  await expect(changes.locator('ins')).toHaveText(['put in: must', 'put in: sign-in']);
  await expect(changes.locator('.difftext')).toHaveText(
    'We taken out: need to put in: must fix the taken out: login put in: sign-in bug before the demo.'
  );
  await expect(changes.getByRole('button', { name: 'Paste what I said' })).toBeEnabled();
  await expect(changes.getByRole('button', { name: 'Copy what I said' })).toBeEnabled();
  await edited.scrollIntoViewIfNeeded();
  await shot(main, '02-home-what-changed-light');

  // The same in the dark theme: the marks keep their contrast.
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'dark'));
  await main.waitForTimeout(150);
  await shot(main, '03-home-what-changed-dark');
  const struck = await changes.locator('del').first().evaluate((el) => getComputedStyle(el).textDecorationLine);
  expect(struck).toContain('line-through');
  const marked = await changes.locator('ins').first().evaluate((el) => getComputedStyle(el).textDecorationLine);
  expect(marked).toContain('underline');
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'light'));

  // From the keyboard: the pill closes it again.
  await pill.focus();
  await main.keyboard.press('Enter');
  await expect(pill).toHaveAttribute('aria-expanded', 'false');
  await expect(changes).toHaveCount(0);
});

test('Paste what I said: the latest dictation is swapped where it went (a test run logs it), any other copied, and Yap says so', async ({
  yap,
  main,
  shot,
}) => {
  const llm = await startFakeLlm();
  llm.reply('cleanup', () => 'Send the report to Sam by Friday.');
  await configure(yap, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  try {
    await openView(main, 'Home');
    const r = await yap.invoke('e2e_dictation', { text: 'um so send the report to sam by friday' });
    expect(r.text.trim()).toBe('Send the report to Sam by Friday.');
    // History keeps the words as said beside it.
    await expectStore(yap, 'history.json', (h) =>
      h.some((e) => e.text === 'Send the report to Sam by Friday.' && e.said === 'um so send the report to sam by friday')
    );
    const latest = row(main, 'Send the report to Sam by Friday.');
    await latest.getByRole('button', { name: 'AI edited · 1 change' }).click();
    const changes = latest.getByRole('region', { name: 'What AI cleanup changed' });
    await expect(changes.locator('del')).toHaveText(['taken out: um so']);
    await expect(changes.locator('ins')).toHaveCount(0);

    // The latest, right after it: swapped in the app it went into (a test
    // run touches no other app: only the log says so).
    let before = log(yap).length;
    await changes.getByRole('button', { name: 'Paste what I said' }).click();
    await expect.poll(() => log(yap).slice(before)).toContain('e2e: not swapping the paste for the words as said');
    await expect(toastWith(main, 'A test run: nothing swapped')).toBeVisible();
    await closeToasts(main);

    // Once: then it's copied, and Yap says why.
    before = log(yap).length;
    await changes.getByRole('button', { name: 'Paste what I said' }).click();
    const copied = toastWith(main, 'Copied what you said');
    await expect(copied).toContainText('Yap only swaps your latest dictation, right after it. Press Ctrl+V to paste it.');
    await expect.poll(() => log(yap).slice(before)).toContain('e2e: not copying the words as said');
    await shot(main, '04-home-paste-what-i-said-copied');
    await closeToasts(main);

    // An older dictation: copied.
    const older = row(main, 'We must fix the sign-in bug before the demo.');
    await older.getByRole('button', { name: 'AI edited · 2 changes' }).click();
    await older.getByRole('button', { name: 'Paste what I said' }).click();
    await expect(toastWith(main, 'Copied what you said')).toBeVisible();
    await closeToasts(main);

    // Copy what I said: the words as said, not the raw transcript.
    const clip = await stubClipboard(main);
    await older.getByRole('button', { name: 'Copy what I said' }).click();
    await expect.poll(clip).toBe('we need to fix the login bug before the demo');
    await closeToasts(main);

    // The row's menu has them too (no "Copy raw transcript" beside them).
    await older.click({ button: 'right' });
    const menu = main.getByRole('menu').first();
    await expect(menu.locator('[role^="menuitem"]')).toHaveText(['Copy', 'Paste what I said', 'Copy what I said', 'Delete…']);
    await main.keyboard.press('Escape');

    // Cleanup that only punctuated: nothing to show.
    llm.reply('cleanup', () => 'Book the room.');
    await yap.invoke('e2e_dictation', { text: 'book the room' });
    await expect(row(main, 'Book the room.')).toBeVisible();
    await expect(row(main, 'Book the room.').locator('.aiedit')).toHaveCount(0);
  } finally {
    await configure(yap, { postProcessEnabled: false, ppBaseUrl: '' });
    await llm.close();
  }
});
