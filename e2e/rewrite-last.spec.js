// "Rewrite your last dictation when nothing is selected" (as_said.rs,
// 2026-10-23): Settings → Commands' switch (`rewriteLastDictation`, on),
// the rewrite shortcut's line following it, Settings search finding it —
// and what the shortcut does with nothing selected right after a dictation:
// on, that dictation is what the AI changes; off, it writes new text, as it
// did before.
//
// A test run reads and pastes into no other app, so its dictations and the
// shortcut go to a stand-in window (`e2e_target_window`: no window has it)
// and `e2e_rewrite` stands in for the shortcut and the speech; the rewrite
// itself, in real windows, is the robot's (e2e-real/rewrite-last.real.js).
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, settingsDialog, expectStore } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript, rewriteText } from './support/fake-llm.js';

const SAID = 'hey can you send me the report by friday';
const CLEANED = 'Hey, can you send me the report by Friday?';
const FORMAL = 'Could you please send me the report by Friday?';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'rewrite-last' });
    },
    { scope: 'worker' },
  ],
});

const ON_LINE = 'With nothing selected within a minute of a dictation, it rewrites that dictation where it is';
const OFF_LINE = 'With nothing selected, it writes where your cursor is';
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

/** Change the config as Settings would, then let Settings' 400 ms auto-save settle (read-aloud.spec.js). */
async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  // Settings' copy auto-saves what it adopted (400 ms later): settled.
  await main.waitForTimeout(700);
  await expectStore(yap, 'config.json', (c) => Object.entries(patch).every(([k, v]) => JSON.stringify(c[k]) === JSON.stringify(v)));
}

test('Settings → Commands: the switch is on by default, saves off and on, the shortcut\'s line follows it, and search finds it', async ({
  yap,
  main,
  shot,
}) => {
  expect(yap.readJson('config.json').rewriteLastDictation ?? true).toBe(true);
  const dialog = await openSettings(main, 'Commands');
  const row = dialog.locator('[data-setting="rewrite-last"]');
  const toggle = row.getByRole('button', { name: 'Rewrite your last dictation when nothing is selected' });
  const shortcut = dialog.locator('[data-setting="rewrite-shortcut"]');
  await row.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(row).toContainText('Within a minute, in the same field');
  await expect(shortcut).toContainText(ON_LINE);
  await shot(main, '01-commands-rewrite-last-on');

  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.rewriteLastDictation === false);
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect(shortcut).toContainText(OFF_LINE);
  await expect(shortcut).not.toContainText('rewrites that dictation');
  await shot(main, '02-commands-rewrite-last-off');

  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.rewriteLastDictation === true);
  await expect(shortcut).toContainText(ON_LINE);

  // Settings search: by what it does.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('last dictation');
  const found = settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
  await expect(found.getByRole('option').filter({ hasText: 'Rewrite your last dictation when nothing is selected' })).toBeVisible();
  await shot(main, '03-search-rewrite-last');
  await search.fill('');
  await closeSettings(main);
});

test('nothing selected right after a dictation: on, the AI rewrites that dictation; off, it writes new text as before', async ({
  yap,
  main,
}) => {
  const llm = await startFakeLlm();
  llm.reply('cleanup', (user) => (cleanupTranscript(user) === SAID ? CLEANED : cleanupTranscript(user)));
  llm.reply('rewrite', () => FORMAL);
  const lastAsked = () => llm.requests.filter((r) => r.kind === 'rewrite').at(-1);
  await setConfig(yap, main, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  await yap.invoke('e2e_target_window', { on: true });
  try {
    // On (the default): the dictation is what the AI changes; a test run
    // reads and pastes nothing, and says so.
    let r = await yap.invoke('e2e_dictation', { text: SAID });
    expect(r.text.trim()).toBe(CLEANED);
    let before = log(yap).length;
    let out = await yap.invoke('e2e_rewrite', { text: 'make it more formal' });
    expect(out).toMatchObject({ last: true, outcome: 'testMode', text: FORMAL });
    expect(rewriteText(lastAsked().system)).toBe(CLEANED);
    expect(log(yap).slice(before)).toContain('Rewrite shortcut with nothing selected: rewriting the last dictation');
    expect(log(yap).slice(before)).toContain('e2e: not rewriting the last dictation');

    // Off: new text at the cursor, the dictation never sent to the AI.
    await setConfig(yap, main, { rewriteLastDictation: false });
    r = await yap.invoke('e2e_dictation', { text: SAID });
    expect(r.text.trim()).toBe(CLEANED);
    before = log(yap).length;
    out = await yap.invoke('e2e_rewrite', { text: 'make it more formal' });
    expect(out).toMatchObject({ last: false, outcome: 'pasted', text: FORMAL });
    expect(rewriteText(lastAsked().system)).toBe('');
    expect(log(yap).slice(before)).not.toContain('rewriting the last dictation');
    expect(log(yap).slice(before)).toContain('e2e: not pasting the text');

    // On again: the last dictation again.
    await setConfig(yap, main, { rewriteLastDictation: true });
    await yap.invoke('e2e_dictation', { text: SAID });
    out = await yap.invoke('e2e_rewrite', { text: 'make it more formal' });
    expect(out).toMatchObject({ last: true, outcome: 'testMode' });
  } finally {
    await yap.invoke('e2e_target_window', { on: false });
    await setConfig(yap, main, { postProcessEnabled: false, ppBaseUrl: '', rewriteLastDictation: true });
    await llm.close();
  }
});
