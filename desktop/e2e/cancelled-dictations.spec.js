// No dictation is lost (2026-10-25; dictation/recent_audio.rs, history.rs):
// a cancelled one (the bar's ✕, Esc) keeps its audio a day and is listed in
// Home as "Cancelled", with **Transcribe** and **Keep** — after the bar's
// 10 s of Undo, which takes it back out; **Save audio…** on any dictation
// writes its WAV (Windows' dialog stubbed: `e2e_audio_save_path`); and a
// dictation of ten minutes suggests meeting notes, once (`coaching.rs`, the
// length shortened for the test). Dictations here are a tone played in the
// microphone's place (`e2e_dictate_audio`): no microphone needed.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, closeToasts, expectStore } from './support/fixtures.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) =>
      use({
        name: 'cancelled-dictations',
        prepare: (dataDir) => {
          // The model "installed", for Transcribe (the stub engine never reads it).
          const model = path.join(dataDir, 'models', 'parakeet-tdt-0.6b-v3-int8');
          fs.mkdirSync(model, { recursive: true });
          fs.writeFileSync(path.join(model, 'encoder-model.int8.onnx'), 'stand-in');
        },
      }),
    { scope: 'worker' },
  ],
});

const history = (yap) => yap.readJson('history.json') || [];
const cancelled = (yap) => history(yap).filter((e) => e.status === 'cancelled');
const row = (main, text) => main.locator('.daylist .item').filter({ hasText: text });
const audioDir = (yap) => path.join(yap.dataDir, 'recent-audio');

test.beforeEach(async ({ yap }) => {
  await yap.invoke('e2e_coaching_reset');
  await yap.invoke('e2e_dictation_cap', {});
});

test('a cancelled dictation is kept: "Cancelled" in Home, Keep, then Transcribe', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  const before = cancelled(yap).length;
  // Cancelled at its end, as the bar's ✕ does: nothing transcribed.
  expect(await yap.invoke('e2e_dictate_audio', { seconds: 1.5, cancel: true })).toBeNull();
  await expectStore(yap, 'history.json', (h) => h.filter((e) => e.status === 'cancelled').length === before + 1);
  const entry = cancelled(yap).at(-1);
  expect(entry.text).toBe('');
  expect(entry.audio).toMatch(/^\d+$/);

  // In Home: "Cancelled", with Transcribe and Keep.
  const item = main.locator('.daylist .item').filter({ hasText: 'Cancelled · not transcribed' }).first();
  await expect(item).toBeVisible();
  await expect(item.getByRole('button', { name: 'Transcribe' })).toBeVisible();
  const keep = item.getByRole('button', { name: 'Keep', exact: true });
  await expect(keep).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '01-cancelled-in-home');

  // Keep: past its day — moved where no sweep reads (an older Yap's neither).
  await keep.click();
  await expect(item.getByRole('button', { name: 'Kept' })).toHaveAttribute('aria-pressed', 'true');
  expect(fs.existsSync(path.join(audioDir(yap), 'kept', `${entry.audio}.wav`))).toBe(true);
  expect(fs.existsSync(path.join(audioDir(yap), `${entry.audio}.wav`))).toBe(false);

  // Transcribe: a dictation like any other now (nothing pasted).
  await item.getByRole('button', { name: 'Transcribe' }).click();
  await expect
    .poll(
      () => {
        const e = history(yap).find((x) => x.audio === entry.audio);
        return !!e && !e.status && e.text.includes('STT stub');
      },
      { timeout: 20_000, message: 'the cancelled dictation to be transcribed' },
    )
    .toBe(true);
  await expect(row(main, 'STT stub').first()).toBeVisible();
  await closeToasts(main);
});

test('Undo takes a cancelled dictation back out of Home: it is transcribed as a new one', async ({ yap, main }) => {
  await openView(main, 'Home');
  expect(await yap.invoke('e2e_dictate_audio', { seconds: 1.2, cancel: true })).toBeNull();
  await expectStore(yap, 'history.json', (h) => h.at(-1)?.status === 'cancelled');
  const id = history(yap).at(-1).audio;
  const total = history(yap).length;
  await yap.invoke('bar_action', { action: 'dictation-undo' });
  // The "Cancelled" entry is gone, a transcribed one in its place.
  await expectStore(yap, 'history.json', (h) => h.length === total && !h.some((e) => e.audio === id) && h.at(-1).text.includes('STT stub'));
  expect(fs.existsSync(path.join(audioDir(yap), `${id}.wav`))).toBe(false);
});

test('Save audio… writes a dictation\'s WAV where you pick', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  const r = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(r.raw).toContain('STT stub');
  await expectStore(yap, 'history.json', (h) => !!h.at(-1)?.audio);
  const target = path.join(yap.runDir, 'saved-dictation.wav');
  fs.rmSync(target, { force: true });
  await yap.invoke('e2e_audio_save_path', { path: target });
  const item = row(main, 'STT stub').first();
  await item.click({ button: 'right' });
  const menu = main.getByRole('menu');
  await expect(menu.getByRole('menuitem', { name: 'Save audio…' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Keep its audio' })).toBeVisible();
  await shot(main, '02-row-menu-save-audio');
  await menu.getByRole('menuitem', { name: 'Save audio…' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Audio saved' })).toContainText('saved-dictation.wav');
  const bytes = fs.readFileSync(target);
  expect(bytes.subarray(0, 4).toString('ascii')).toBe('RIFF');
  expect(bytes.subarray(8, 12).toString('ascii')).toBe('WAVE');
  // About a second of 16 kHz 16-bit mono.
  expect(bytes.length).toBeGreaterThan(16_000 * 2 * 0.8);
  await closeToasts(main);
});

test('a ten-minute dictation suggests meeting notes, once (the length shortened)', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  await yap.invoke('e2e_dictation_cap', { meetingHintMs: 1_500 });
  // Shorter than that: no tip.
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await main.waitForTimeout(1_500);
  await expect(main.getByRole('status').filter({ hasText: 'Try meeting notes' })).toHaveCount(0);

  await yap.invoke('e2e_dictate_audio', { seconds: 2 });
  const tip = main.getByRole('status').filter({ hasText: 'Long dictation? Try meeting notes' });
  await expect(tip).toBeVisible({ timeout: 10_000 });
  await expect(tip).toContainText('Tip');
  // It names the meeting shortcut (Win+Alt+M).
  await expect(tip).toContainText(/Win.*Alt.*M/);
  await shot(main, '03-meeting-notes-tip');
  await expectStore(yap, 'hints.json', (h) => h.longDictation === 1);

  // Once, ever: another long one says nothing.
  await closeToasts(main);
  await yap.invoke('e2e_dictate_audio', { seconds: 2 });
  await main.waitForTimeout(2_500);
  await expect(main.getByRole('status').filter({ hasText: 'Try meeting notes' })).toHaveCount(0);
  expect(yap.readJson('hints.json').longDictation).toBe(1);
});
