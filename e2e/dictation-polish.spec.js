// Dictation polish (2026-10-25): Paste last / Copy last transcript on
// shortcuts of their own (pasting/last.rs), "Taking longer than usual…" with
// Skip cleanup (dictation/slow.rs), the length limit's warning and Recover
// text (dictation/cap.rs), a level meter that moves for whispers
// (dictation/meter.rs) and "Hold on, still transcribing…".
//
// A test run has no input hook (it must never see the developer's keys):
// `e2e_shortcut` sends a shortcut's press as the hook would. It copies and
// pastes nothing either: the log says what it would have done (the real
// paste and clipboard are the robot's, e2e-real/dictation-extras.real.js).
// The stub engine transcribes at once unless `e2e_dictation_timing` slows
// it; `e2e_dictation_cap` shortens the 15-minute limit; `e2e_dictate_audio`
// plays a tone in the mic's place (`amplitude` 0.005: a whisper, -46 dBFS).
import fs from 'node:fs';
import {
  test as base,
  expect,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  inAnotherApp,
  pressHotkey,
  setConfig,
} from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

// Its own Yap: these tests change the timings and the length limit, and
// start with no dictation at all.
const test = base.extend({
  yapOptions: [async ({}, use) => use({ name: 'dictation-polish' }), { scope: 'worker' }],
});

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

/** A key as the keyboard sends it, in a webview (Playwright stops at F12). */
async function pressKey(page, key, vk) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

/** The pretend cursor onto the dictation capsule: the bar takes clicks there. */
async function pointAtCapsule(yap) {
  await yap.invoke('bar_simulate', { pointer: 'dictation' });
  await expect.poll(() => yap.invoke('bar_debug').then((d) => d.interactive)).toBe(true);
}

/** The heights (%) of the capsule's waveform bars, oldest first. */
function waveHeights(capsule) {
  return capsule.locator('.wave span').evaluateAll((bars) => bars.map((b) => parseFloat(b.style.height) || 0));
}

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_dictation_timing', { floorMs: 0, sttMs: 0, forget: true }).catch(() => {});
  await yap.invoke('e2e_dictation_cap', {}).catch(() => {});
  await yap.invoke('bar_simulate', { pointer: 'away' }).catch(() => {});
});

test('Paste last and Copy last with nothing dictated yet: said quietly, nothing done', async ({ yap, main, shot }) => {
  const mark = log(yap).length;
  await yap.invoke('e2e_shortcut', { name: 'paste-last' });
  const nothing = main.getByRole('status').filter({ hasText: 'Nothing to paste yet' });
  await expect(nothing).toBeVisible();
  await shot(main, '01-nothing-to-paste-yet');
  await closeToasts(main);
  await yap.invoke('e2e_shortcut', { name: 'copy-last' });
  await expect(main.getByRole('status').filter({ hasText: 'Nothing to copy yet' })).toBeVisible();
  await closeToasts(main);
  expect(log(yap).slice(mark)).toContain('Paste last transcript: nothing dictated yet');
  expect(log(yap).slice(mark)).toContain('Copy last transcript: nothing dictated yet');
});

test('Settings → Shortcuts: Paste last and Copy last transcript are unbound until set, and never take another shortcut\'s key', async ({
  yap,
  main,
  shot,
}) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const paste = dialog.locator('[data-setting="shortcut-paste-last"]');
  const copy = dialog.locator('[data-setting="shortcut-copy-last"]');
  await expect(paste).toContainText('Paste last transcript');
  await expect(copy).toContainText('Copy last transcript');
  await expect(paste.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await expect(copy.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();

  // Paste last: the dictation hotkey (F24 in the suite) refused, F21 taken.
  await paste.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F24', 135);
  await expect(paste).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F21', 132);
  await expectStore(yap, 'config.json', (c) => c.pasteLastHotkey === 'kb:132');
  await expect(paste.getByRole('button', { name: /^F21\s*Change$/ })).toBeVisible();

  // Copy last: not Paste last's key; F20 taken.
  await copy.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F21', 132);
  await expect(copy).toContainText('Already used by the Paste-last shortcut');
  await pressKey(main, 'F20', 131);
  await expectStore(yap, 'config.json', (c) => c.copyLastHotkey === 'kb:131');
  await copy.scrollIntoViewIfNeeded();
  await shot(main, '02-shortcuts-paste-and-copy-last');

  // Settings' search finds them.
  const search = dialog.getByRole('searchbox', { name: 'Search settings' });
  await search.fill('copy last');
  await expect(dialog.getByRole('listbox', { name: 'Settings found' }).getByRole('option').first()).toContainText('Copy last transcript');
  await search.fill('');

  // Both removed again.
  await openSettings(main, 'Shortcuts');
  await paste.getByRole('button', { name: 'Remove hotkey' }).click();
  await copy.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.pasteLastHotkey === '' && c.copyLastHotkey === '');
  await closeSettings(main);
});

test('Paste last and Copy last through their shortcuts: the latest dictation, and a quiet "Copied" where you are', async ({
  yap,
  main,
  shot,
}) => {
  const r = await yap.invoke('e2e_dictation', { text: 'Book the big room for Tuesday.' });
  expect(r.text.trim()).toBe('Book the big room for Tuesday.');

  // Copy last from a Yap window (its in-page fallback's command): the text.
  expect(await yap.invoke('copy_last_transcript')).toBe('Book the big room for Tuesday.');
  await expect(main.getByRole('status').filter({ hasText: 'Copied your last dictation' })).toBeVisible();
  await closeToasts(main);

  // The shortcuts, as the input hook sends them.
  const mark = log(yap).length;
  await yap.invoke('e2e_shortcut', { name: 'copy-last' });
  const copied = main.getByRole('status').filter({ hasText: 'Copied your last dictation' });
  await expect(copied).toBeVisible();
  await expect(copied).toContainText('kept out of your clipboard history');
  await shot(main, '03-copied-your-last-dictation');
  await closeToasts(main);
  await yap.invoke('e2e_shortcut', { name: 'paste-last' });
  await expect.poll(() => log(yap).slice(mark)).toContain('e2e: not pasting the last transcript');
  expect(log(yap).slice(mark)).toContain('e2e: not copying the last transcript');
  // Lengths in the log, never the words.
  expect(log(yap).slice(mark)).not.toContain('big room');

  // In another app: a card on the Yap bar instead (never a Windows
  // notification for this).
  await inAnotherApp(yap);
  await yap.invoke('e2e_shortcut', { name: 'copy-last' });
  await expect(yap.overlay.getByText('Copied your last dictation')).toBeVisible();
  await shot(yap.overlay, '04-copied-on-the-bar');
});

test('"Taking longer than usual…": a slow transcription says so on the bar, no Skip; then it pastes', async ({ yap, shot }) => {
  const capsule = yap.overlay.locator('.capsule');
  // Never before 1.5 s here (4 s for real); nothing known about this PC
  // yet: 1.5 s + half the audio's length. The stub takes 4 s.
  await yap.invoke('e2e_dictation_timing', { floorMs: 1500, sttMs: 4000, forget: true });
  const mark = log(yap).length;
  const run = yap.invoke('e2e_dictate_audio', { seconds: 1 });
  // (The live words, then — past its time — the notice in their place.)
  await expect(capsule.locator('.dot.proc')).toBeVisible({ timeout: 10_000 });
  await expect(capsule).toContainText('Taking longer than usual…', { timeout: 5_000 });
  await expect(capsule.getByRole('button', { name: 'Skip cleanup' })).toHaveCount(0);
  await shot(yap.overlay, '05-taking-longer-transcribing');
  const r = await run;
  expect(r.raw).toMatch(/STT stub/);
  expect(log(yap).slice(mark)).toMatch(/taking longer than usual.*stage=Transcribing|stage=Transcribing.*taking longer than usual/i);
  await expect(capsule).toBeHidden({ timeout: 15_000 });
});

test('"Taking longer than usual…" with Skip cleanup: a slow AI cleanup gives way, the words as transcribed are pasted now', async ({
  yap,
  main,
  shot,
}) => {
  const capsule = yap.overlay.locator('.capsule');
  const llm = await startFakeLlm();
  llm.reply('cleanup', () => 'Book the big room for Tuesday.');
  try {
    await setConfig(yap, main, {
      postProcessEnabled: true,
      ppProvider: 'custom',
      ppBaseUrl: llm.base,
      ppModel: 'fake-cleanup',
      ppApiKey: '',
      cleanupStyles: false,
    });
    await yap.invoke('e2e_dictation_timing', { floorMs: 1500, sttMs: 0, forget: true });
    // A usual cleanup: quick, and now what's usual for this model.
    let r = await yap.invoke('e2e_dictation', { text: 'um book the big room for tuesday' });
    expect(r.text.trim()).toBe('Book the big room for Tuesday.');

    // The model slows right down: past its usual time the bar says so,
    // and offers Skip cleanup.
    llm.delay('cleanup', 10_000);
    const started = Date.now();
    const slow = yap.invoke('e2e_dictation', { text: 'um book the big room for tuesday' });
    await expect(capsule).toContainText('Taking longer than usual…', { timeout: 5_000 });
    const skip = capsule.getByRole('button', { name: 'Skip cleanup' });
    await expect(skip).toBeVisible();
    await expect.poll(() => capsule.getAttribute('data-region')).toBe('dictation');
    await shot(yap.overlay, '06-taking-longer-skip-cleanup');
    await pointAtCapsule(yap);
    await skip.click();
    r = await slow;
    expect(Date.now() - started).toBeLessThan(8_000);
    expect(r.text.trim()).toBe('um book the big room for tuesday');
    expect(log(yap)).toContain('AI cleanup skipped from the bar');
    await expect(capsule).toBeHidden({ timeout: 15_000 });

    // A cleanup that's slow but under its time says nothing: the next one
    // at its usual pace pastes the cleaned words.
    llm.delay('cleanup', 0);
    r = await yap.invoke('e2e_dictation', { text: 'um book the big room for tuesday' });
    expect(r.text.trim()).toBe('Book the big room for Tuesday.');
  } finally {
    llm.delay('cleanup', 0);
    await setConfig(yap, main, { postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
    await llm.close();
  }
});

test('the length limit: warned a moment before, stopped at it, nothing lost — and Recover text copies it', async ({ yap, main, shot }) => {
  const capsule = yap.overlay.locator('.capsule');
  // 8 s instead of 15 minutes, the warning at 4 s instead of 14 minutes.
  await yap.invoke('e2e_dictation_cap', { limitMs: 8_000, warnMs: 4_000 });
  await setConfig(yap, main, { streamingPartials: false });
  try {
    const mark = log(yap).length;
    // Fifteen seconds of talk: the recording stops itself at eight.
    const run = yap.invoke('e2e_dictate_audio', { seconds: 15 });
    await expect(capsule).toContainText("4 seconds left · it'll stop at 8 seconds", { timeout: 10_000 });
    await expect(capsule.locator('.dot.rec')).toBeVisible();
    await expect(capsule.locator('.capline')).toBeAttached();
    await shot(yap.overlay, '07-length-limit-warning');
    // The words give way to the waveform again, the hairline still running.
    await expect(capsule.locator('.wave')).toBeVisible({ timeout: 4_000 });
    await expect(capsule.locator('.capline')).toBeAttached();
    const r = await run;
    // What was said up to the limit, transcribed and delivered.
    expect(r).not.toBeNull();
    expect(r.raw).toMatch(/received 8\.0s of audio/);
    await expectStore(yap, 'history.json', (h) => h.some((e) => (e.text || '').includes('received 8.0s of audio')));
    const said = log(yap).slice(mark);
    expect(said).toContain('Dictation: nearing its length limit');
    expect(said).toContain('Dictation: reached its length limit, stopping and transcribing');

    // Said, with Recover text.
    const stopped = main.getByRole('status').filter({ hasText: 'Dictation stopped at 8 seconds' });
    await expect(stopped).toBeVisible();
    await expect(stopped).toContainText('Everything you said is pasted');
    await shot(main, '08-dictation-stopped-recover-text');
    await stopped.getByRole('button', { name: 'Recover text' }).click();
    await expect(main.getByRole('status').filter({ hasText: 'Copied your dictation' })).toBeVisible();
    await expect.poll(() => log(yap).slice(mark)).toContain('e2e: not copying the recovered dictation');
    await closeToasts(main);
  } finally {
    await setConfig(yap, main, { streamingPartials: true });
  }
});

test('the level meter moves for a whisper, and a shout never reaches the top', async ({ yap, main, shot }) => {
  const capsule = yap.overlay.locator('.capsule');
  // The waveform, not the live words, in the capsule.
  await setConfig(yap, main, { streamingPartials: false });
  try {
    // A whisper: a tone peaking at 0.005 (-46 dBFS). The old linear meter
    // drew it at the 7 % minimum.
    let run = yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 0.005 });
    await expect(capsule.locator('.wave span').first()).toBeAttached({ timeout: 10_000 });
    await expect.poll(async () => Math.max(0, ...(await waveHeights(capsule)).slice(-10)), { timeout: 5_000 }).toBeGreaterThanOrEqual(25);
    // A second on: still moving (the meter's floor follows a steady hum,
    // never the dips between syllables), the dips at the bottom.
    await yap.overlay.waitForTimeout(1_200);
    const whisper = (await waveHeights(capsule)).slice(-10);
    expect(Math.max(...whisper)).toBeGreaterThanOrEqual(25);
    expect(Math.max(...whisper)).toBeLessThan(45);
    expect(Math.min(...whisper)).toBeLessThanOrEqual(10);
    await shot(yap.overlay, '09-meter-whisper');
    await run;
    await expect(capsule).toBeHidden({ timeout: 15_000 });

    // Full scale: high, but short of the top.
    run = yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 1.0 });
    await expect(capsule.locator('.wave span').first()).toBeAttached({ timeout: 10_000 });
    await expect.poll(async () => Math.max(0, ...(await waveHeights(capsule)).slice(-10)), { timeout: 5_000 }).toBeGreaterThanOrEqual(85);
    await yap.overlay.waitForTimeout(1_200);
    const shout = (await waveHeights(capsule)).slice(-10);
    expect(Math.max(...shout)).toBeLessThanOrEqual(92);
    await shot(yap.overlay, '10-meter-full-scale');
    await run;
    await expect(capsule).toBeHidden({ timeout: 15_000 });
  } finally {
    await setConfig(yap, main, { streamingPartials: true });
  }
});

test('"Hold on, still transcribing…": the dictation shortcut pressed during a transcription is said, not ignored silently', async ({
  yap,
  main,
  shot,
}) => {
  const capsule = yap.overlay.locator('.capsule');
  await yap.invoke('e2e_dictation_timing', { floorMs: 0, sttMs: 3000, forget: true });
  const run = yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await expect(capsule.locator('.dot.proc')).toBeVisible({ timeout: 10_000 });
  await pressHotkey(main);
  await expect(capsule).toContainText('Hold on, still transcribing…');
  await shot(yap.overlay, '11-hold-on-still-transcribing');
  const r = await run;
  expect(r.raw).toMatch(/STT stub/);
  // The press started nothing.
  expect(log(yap)).toContain('Ignoring start — a transcription is still processing');
  await expect(capsule).toBeHidden({ timeout: 15_000 });
});
