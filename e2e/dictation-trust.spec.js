// Trusting a dictation (2026-10-22): where the speech model runs on this
// PC (dictation/stt_speed.rs: Settings → Speech-to-Text → Runs on), "Use what I said"
// after AI cleanup (pasting/as_said.rs: the Yap bar's offer, the shortcut, the
// tray's paste as said) and a day of dictation audio to run a dictation
// again (dictation/recent_audio.rs: Home's Run again and Play, a dictation Yap never
// finished, Settings → Privacy & data).
//
// A test run runs the stub engine and pastes nothing: the benchmark's two
// devices cost what YAP_E2E_STT_SPEED says (the processor 150 ms, the
// graphics card 450), "Use what I said" only logs that it would swap (the
// real swap is the robot's: e2e-real/as-said.real.js), and a dictation's
// text is the stub's "[STT stub: …]".
import fs from 'node:fs';
import path from 'node:path';
import {
  test as base,
  expect,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  pressHotkey,
  settingsDialog,
} from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { writeWav } from './support/wav.js';

const MODEL = 'parakeet-tdt-0.6b-v3';
const MODEL_DIR = 'parakeet-tdt-0.6b-v3-int8';
// A dictation from before Yap closed mid-way: its audio and the marker
// saying it was never finished (dictation/recent_audio.rs), ten minutes old.
const PENDING = String(Date.now() - 10 * 60_000);

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'dictation-trust',
        env: { YAP_E2E_STT_SPEED: 'gpu=450,cpu=150' },
        prepare: (dataDir) => {
          // The model "installed" (the stub engine never reads it).
          const model = path.join(dataDir, 'models', MODEL_DIR);
          fs.mkdirSync(model, { recursive: true });
          fs.writeFileSync(path.join(model, 'encoder-model.int8.onnx'), 'stand-in');
          const audio = path.join(dataDir, 'recent-audio');
          fs.mkdirSync(audio, { recursive: true });
          writeWav(path.join(audio, `${PENDING}.wav`), { seconds: 2 });
          fs.writeFileSync(path.join(audio, `${PENDING}.pending`), '');
        },
      });
    },
    { scope: 'worker' },
  ],
});

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

function invokeError(yap, cmd, args) {
  return yap.main.evaluate(
    ([c, a]) =>
      window.__TAURI_INTERNALS__.invoke(c, a).then(
        () => null,
        (e) => String(e)
      ),
    [cmd, args]
  );
}

async function configure(yap, patch) {
  await yap.invoke('save_config', { cfg: { ...(await yap.invoke('get_config')), ...patch } });
}

async function needsMic(yap) {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; recording needs one');
}

test('Runs on: measured once on this PC, everyday dictations decide (the processor clearly faster here), Graphics card and Processor override it', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Speech-to-Text');
  const row = dialog.locator('[data-setting="runs-on"]');
  await row.scrollIntoViewIfNeeded();
  await expect(row).toContainText('Runs on: graphics card (not measured on this PC yet)');
  await shot(main, '01-runs-on-not-measured');

  await row.getByRole('button', { name: 'Measure now' }).click();
  // What it chose and why: each device's time for your dictations.
  const picked = /Runs on: processor — fastest for your dictations on this PC \(1\d\d ms vs 4\d\d ms\)/;
  await expect(row).toContainText(picked, { timeout: 60_000 });
  await expect(row.getByRole('button', { name: 'Measure again' })).toBeEnabled();
  await shot(main, '02-runs-on-measured');

  // Kept for this model on this PC, never in config.json.
  const store = yap.readJson('stt_speed.json');
  const m = store.models[MODEL];
  expect(m.pick).toBe('cpu');
  expect(m.runs.map((r) => r.device).sort()).toEqual(['cpu', 'gpu']);
  expect(m.method).toBe(3);
  // No dictations yet: the two everyday lengths weigh the same.
  expect(m.shortShare).toBe(0.5);
  for (const r of m.runs) {
    expect(r.everydayMs).toBeGreaterThan(100);
    expect(r.agrees).toBe(true);
    // Short, typical and long dictations, three warm runs each.
    expect(r.lengths.map((t) => Math.round(t.clipMs / 1000))).toEqual([3, 11, 36]);
    for (const t of r.lengths) expect(t.runsMs).toHaveLength(3);
  }
  expect(yap.readJson('config.json').sttDevice ?? 'auto').toBe('auto');
  // The log has the numbers (never a transcript), and the warm engine moves
  // to the processor.
  expect(log(yap)).toContain('Speech engine timed');
  expect(log(yap)).not.toMatch(/Speech engine timed.*STT stub/);
  await expect.poll(() => log(yap)).toContain('Speech engine moved to the device chosen for it');

  // Chosen by hand: the graphics card (the old GPU switch follows)…
  const select = row.getByRole('combobox', { name: 'Runs on' });
  await select.selectOption('gpu');
  await expectStore(yap, 'config.json', (c) => c.sttDevice === 'gpu' && c.useGpu === true);
  await expect(row).toContainText(/Runs on: graphics card \(your choice\)\. Automatic picks the processor here \(1\d\d ms vs 4\d\d ms\)/);
  await shot(main, '03-runs-on-graphics-card');
  // …the processor (the GPU switch off, as before)…
  await select.selectOption('cpu');
  await expectStore(yap, 'config.json', (c) => c.sttDevice === 'cpu' && c.useGpu === false);
  await expect(row).toContainText('Runs on: processor (your choice)');
  // …and back to the measured pick.
  await select.selectOption('auto');
  await expectStore(yap, 'config.json', (c) => c.sttDevice === 'auto' && c.useGpu === true);
  await expect(row).toContainText(picked);
  await closeSettings(main);
});

test('"Use what I said": the bar offers it after AI cleanup changed the words, once; the tray pastes it', async ({ yap, main, shot }) => {
  const llm = await startFakeLlm();
  llm.reply('cleanup', () => 'Send the report to Sam by Friday.');
  await configure(yap, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  const bar = yap.overlay;
  const capsule = bar.locator('.capsule');
  try {
    const r = await yap.invoke('e2e_dictation', { text: 'um so send the report to sam by friday' });
    expect(r.text.trim()).toBe('Send the report to Sam by Friday.');
    expect(r.asSaid.trim()).toBe('um so send the report to sam by friday');
    await expect(capsule).toContainText('Cleaned up');
    const swap = capsule.getByRole('button', { name: 'Use what I said' });
    await expect(swap).toBeVisible();
    await bar.waitForTimeout(250);
    await shot(bar, '04-bar-use-what-i-said');
    // A region of the click-through bar, never activated.
    await yap.invoke('bar_simulate', { pointer: 'dictation' });
    await expect.poll(() => yap.invoke('bar_debug').then((d) => d.interactive)).toBe(true);
    expect((await yap.invoke('bar_debug')).barActive).toBe(false);
    const before = log(yap).length;
    await swap.click();
    await expect.poll(() => log(yap).slice(before)).toContain('e2e: not swapping the paste for the words as said');
    await expect(capsule).toBeHidden();
    await yap.invoke('bar_simulate', { pointer: 'away' });
    // Once per dictation.
    expect(await invokeError(yap, 'as_said_swap')).toMatch(/Nothing to swap/);

    // Left alone, the offer goes after a few seconds — the shortcut still
    // swaps it (its command).
    await yap.invoke('e2e_dictation', { text: 'um so send the report to sam by friday' });
    await expect(capsule).toContainText('Cleaned up');
    await expect(capsule).toBeHidden({ timeout: 10_000 });
    expect((await yap.invoke('as_said_swap')).outcome).toBe('testMode');

    // The tray's "Paste last transcript (as said)": the words before
    // cleanup (a test run returns them without pasting).
    expect((await yap.invoke('paste_last_transcript', { asSaid: true })).trim()).toBe('um so send the report to sam by friday');
    expect((await yap.invoke('paste_last_transcript')).trim()).toBe('Send the report to Sam by Friday.');

    // Cleanup that only punctuated: nothing to offer.
    llm.reply('cleanup', () => 'Send it now.');
    await yap.invoke('e2e_dictation', { text: 'send it now' });
    await bar.waitForTimeout(700);
    await expect(capsule).toBeHidden();

    // Settings → Dictation: the offer can be switched off.
    const dialog = await openSettings(main, 'Dictation');
    const offer = dialog.getByRole('button', { name: 'Offer what you said after AI cleanup' });
    await expect(offer).toHaveAttribute('aria-pressed', 'true');
    await offer.scrollIntoViewIfNeeded();
    await shot(main, '05-settings-offer-what-you-said');
    await offer.click();
    await expectStore(yap, 'config.json', (c) => c.asSaidOffer === false);
    await closeSettings(main);
    // (Settings' own copy of the config predates the cleanup set up above.)
    await configure(yap, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
    llm.reply('cleanup', () => 'Send the report to Sam by Friday.');
    await yap.invoke('e2e_dictation', { text: 'um so send the report to sam by friday' });
    await bar.waitForTimeout(700);
    await expect(capsule).toBeHidden();
    // (The shortcut still works without the bar's offer.)
    expect((await yap.invoke('as_said_swap')).outcome).toBe('testMode');
  } finally {
    await configure(yap, { postProcessEnabled: false, ppBaseUrl: '', asSaidOffer: true });
    await llm.close();
  }
});

test('the tray\'s "Paste last transcript (as said)" after a dictation cleanup left alone (cleanup off): the dictation itself, and Yap still answers', async ({ yap }) => {
  // Cleanup off, the default (the test before switched it back off): no
  // words as said for this dictation.
  await configure(yap, { postProcessEnabled: false });
  const r = await yap.invoke('e2e_dictation', { text: 'send the plan to sam' });
  expect(r.asSaid ?? null).toBeNull();
  // It froze Yap (pasting/text_injector.rs took the last dictation's lock twice):
  // the command never answered, nor anything after it.
  const frozen = new Promise((resolve) => setTimeout(() => resolve('Yap froze'), 5_000));
  const said = await Promise.race([yap.invoke('paste_last_transcript', { asSaid: true }), frozen]);
  expect(said.trim()).toBe(r.text.trim());
  // Still answering: the plain paste, and another dictation.
  expect((await yap.invoke('paste_last_transcript')).trim()).toBe(r.text.trim());
  expect((await yap.invoke('e2e_dictation', { text: 'still here' })).text.trim()).toContain('still here');
});

test('Shortcuts: "Use what I said" is unbound until set, and can\'t take another shortcut\'s key', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const row = dialog.locator('[data-setting="shortcut-as-said"]');
  await expect(row).toContainText('Use what I said');
  await expect(row.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await row.getByRole('button', { name: 'Set a shortcut' }).click();
  // The dictation hotkey (F24 in the suite): refused.
  await pressKey(main, 'F24', 135);
  await expect(row).toContainText('Already used by the dictation hotkey');
  // F22: taken and live.
  await pressKey(main, 'F22', 133);
  await expectStore(yap, 'config.json', (c) => c.asSaidHotkey === 'kb:133');
  await expect(row.getByRole('button', { name: /^F22\s*Change$/ })).toBeVisible();
  await shot(main, '06-shortcuts-use-what-i-said');
  await row.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.asSaidHotkey === '');
  await closeSettings(main);
});

test('a dictation Yap never finished is in Home, failed, and Run again transcribes its kept audio', async ({ yap, main, shot }) => {
  // Listed at start from its marker (dictation/recent_audio.rs), dated when it was
  // recorded.
  await expectStore(yap, 'history.json', (h) => h.some((e) => e.audio === PENDING && e.status === 'failed'));
  await main.getByRole('button', { name: 'Home', exact: true }).click().catch(() => {});
  const failed = main.locator('.item.failed').filter({ hasText: 'Not transcribed' });
  await expect(failed).toContainText('Not transcribed · Yap closed before it was transcribed');
  await failed.scrollIntoViewIfNeeded();
  await shot(main, '07-home-failed-dictation');

  await failed.locator('.retry').click();
  await expect(main.getByRole('status').filter({ hasText: 'Transcribed again' })).toBeVisible();
  await expectStore(yap, 'history.json', (h) =>
    h.some((e) => e.audio === PENDING && !e.status && e.text.includes('STT stub') && e.model === MODEL)
  );
  await expect(main.locator('.item.failed')).toHaveCount(0);
  await shot(main, '08-home-run-again-done');
  await closeToasts(main);
  // Its audio is still there to play (a WAV).
  const bytes = await main.evaluate(
    (id) => window.__TAURI_INTERNALS__.invoke('recent_audio_wav', { id }).then((b) => b.byteLength),
    PENDING
  );
  expect(bytes).toBeGreaterThan(44);
});

test('a spoken dictation keeps its audio a day: Play, Run again from the menu, and deleted with its entry', async ({ yap, main, shot }) => {
  await needsMic(yap);
  const audioDir = path.join(yap.dataDir, 'recent-audio');
  const wavs = () => fs.readdirSync(audioDir).filter((f) => f.endsWith('.wav'));
  const kept = wavs().length;
  const before = (yap.readJson('history.json') || []).length;
  await pressHotkey(main);
  await main.waitForTimeout(1_500);
  await pressHotkey(main);
  await expectStore(yap, 'history.json', (h) => h.length === before + 1);
  const entry = yap.readJson('history.json').at(-1);
  expect(entry.audio).toMatch(/^\d+$/);
  await expect.poll(wavs).toHaveLength(kept + 1);
  // No marker left: it's in history.
  expect(fs.existsSync(path.join(audioDir, `${entry.audio}.pending`))).toBe(false);

  const row = main.locator('.item').filter({ hasText: entry.text }).first();
  await row.hover();
  await row.getByRole('button', { name: 'Play' }).click();
  await expect(row.getByRole('button', { name: 'Stop playing' })).toBeVisible();
  await shot(main, '09-home-play');
  await row.getByRole('button', { name: 'Stop playing' }).click();

  // Run again from the row's menu.
  await row.click({ button: 'right' });
  await main.getByRole('menuitem', { name: 'Run again', exact: true }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Transcribed again' })).toBeVisible();
  await closeToasts(main);

  // Deleted from Home: its audio goes with it.
  await row.hover();
  await row.getByRole('button', { name: 'Delete' }).click();
  await main.getByRole('alertdialog').getByRole('button', { name: 'Delete', exact: true }).click();
  await expect.poll(() => fs.existsSync(path.join(audioDir, `${entry.audio}.wav`))).toBe(false);
});

test('Privacy & data: what leaves this PC says the audio never does; the switch, and Clear', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Privacy & data');
  const table = dialog.getByRole('table', { name: 'What leaves this PC' });
  await expect(table.getByRole('row').filter({ hasText: 'Recent dictation audio' })).toContainText(
    'Never leaves this PC: kept here for 24 hours, to run a dictation again'
  );
  const keep = dialog.getByRole('button', { name: 'Keep recent dictation audio' });
  await expect(keep).toHaveAttribute('aria-pressed', 'true');
  const clear = dialog.locator('[data-setting="clear-recent-audio"]');
  await expect(clear).toContainText(/\d+ dictations? · \d+ (KB|MB)/);
  await clear.scrollIntoViewIfNeeded();
  await shot(main, '10-privacy-recent-audio');

  await clear.getByRole('button', { name: 'Clear audio…' }).click();
  await main.getByRole('alertdialog').getByRole('button', { name: 'Clear audio' }).click();
  await expect(clear).toContainText('Nothing kept right now');
  expect(fs.existsSync(path.join(yap.dataDir, 'recent-audio'))).toBe(false);
  // History stays; its entries just can't be run again.
  expect((yap.readJson('history.json') || []).length).toBeGreaterThan(0);
  expect((await yap.invoke('get_history', { limit: 50 })).some((e) => e.audio)).toBe(false);

  // Off: not kept, and the table says so.
  await keep.click();
  await expectStore(yap, 'config.json', (c) => c.keepRecentAudio === false);
  await expect(table.getByRole('row').filter({ hasText: 'Recent dictation audio' })).toContainText(
    'Not kept: gone once a dictation is transcribed'
  );
  await keep.click();
  await expectStore(yap, 'config.json', (c) => c.keepRecentAudio === true);
  await closeSettings(main);
  expect(settingsDialog(main)).toBeTruthy();
});

/** A key press as the keyboard sends it (Settings' hotkey recorder). */
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
