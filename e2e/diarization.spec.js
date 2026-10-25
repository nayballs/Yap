// Telling speakers apart in meetings (src-tauri/src/speakers/diarize.rs): the speaker
// models in Settings → Speech-to-Text (rows like the speech models', the
// fallback listed as coming later, where the files come from), the "Tell
// speakers apart in meetings" switch that stays off until a model is
// installed and offers the download instead, the download itself (progress,
// SHA-256, the runtime zip unpacked; voice matching with it, and fetched
// alone for a model from before it), the same switch in General →
// Meetings, delete, and test runs never downloading from the internet.
//
// Nothing here downloads from the internet: a local server serves a fake
// model and a fake runtime zip laid out like NVIDIA's (support/
// diarization.js), named in the debug-only YAP_DIARIZATION_SOURCES file with
// their sizes and SHA-256s. The real engine runs in diarization-real.spec.js.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, settingsDialog } from './support/fixtures.js';
import { startFakeSpeakerModels, RUNTIME_DLLS, MODEL_FILE, VOICES_FILE } from './support/diarization.js';
import { RUNS } from './support/yap.js';

const test = base.extend({
  models: [
    async ({}, use) => {
      const fake = await startFakeSpeakerModels(path.join(RUNS, 'diarization-sources'));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ models }, use) => {
      await use({ name: 'diarization', env: models.env });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('diarization_status');
const nemotron = (s) => s.models.find((m) => m.id === 'nemotron-3-diarization');
const config = (yap) => yap.readJson('config.json');
const speakersGroup = (dialog) => dialog.getByRole('group', { name: 'Speakers in meetings' });
const theSwitch = (scope) => scope.getByRole('button', { name: 'Tell speakers apart in meetings' });
const row = (scope, name) => scope.locator('.mrow').filter({ hasText: name });
const folder = (yap, ...parts) => path.join(yap.dataDir, 'models', 'diarization', ...parts);

test('Speech-to-Text lists the speaker models, the switch off until one is installed', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Speech-to-Text');
  const group = speakersGroup(dialog);
  await group.scrollIntoViewIfNeeded();
  await expect(group).toBeVisible();

  // The recommended model, to download, with its brand icon and voices.
  const rec = row(group, 'Nemotron-3 Diarization');
  await expect(rec).toContainText('Recommended');
  await expect(rec).toContainText('Up to 8 voices');
  await expect(rec.getByRole('button', { name: 'Download' })).toBeVisible();
  await expect(rec.locator('img.micon')).toHaveCount(1);
  // The phone-call fallback is listed, and can't be downloaded yet.
  const fallback = row(group, 'pyannote + ERes2Net');
  await expect(fallback).toContainText('For phone-only calls — coming later');
  await expect(fallback.getByRole('button', { name: 'Download' })).toHaveCount(0);

  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'false');
  await expect(group).toContainText('It all runs on this PC');
  await expect(group).toContainText('Needs a speaker model below');
  await expect(group).toContainText('Beta');
  // Where the files come from, and their licences.
  await expect(group).toContainText('OpenMDW-1.1');
  await expect(group).toContainText('NeMo-Speech.cpp 0.2.0');
  await expect(group).toContainText('Apache-2.0');

  const s = await status(yap);
  expect(nemotron(s).installed).toBe(false);
  expect(s.models.find((m) => m.id === 'sherpa-pyannote-eres2net').available).toBe(false);
  await shot(main, '01-speaker-models');
  await closeSettings(main);
});

test('switching it on without a model offers the download, which installs it and turns it on', async ({
  yap,
  main,
  models,
  shot,
}) => {
  const dialog = await openSettings(main, 'Speech-to-Text');
  const group = speakersGroup(dialog);
  await group.scrollIntoViewIfNeeded();

  // Switching on with no model: the offer, and the switch stays off.
  await theSwitch(group).click();
  const offer = group.getByRole('group', { name: 'Download the speaker model' });
  await expect(offer).toBeVisible();
  await expect(offer).toContainText("NVIDIA's Nemotron-3 Diarization");
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '02-offer');
  // "Not now" puts it away without changing anything.
  await offer.getByRole('button', { name: 'Not now' }).click();
  await expect(offer).toHaveCount(0);
  await main.waitForTimeout(600); // past Settings' auto-save
  expect(config(yap).meetingDiarization ?? false).toBe(false);

  // Download and turn on: progress, then installed, active and on.
  await theSwitch(group).click();
  await offer.getByRole('button', { name: 'Download and turn on' }).click();
  await expect(offer.getByText(/Downloading the speaker model… \d+%/)).toBeVisible();
  await expect(offer.getByRole('button', { name: 'Cancel the download: Downloading the speaker model' })).toBeVisible();
  await expect(row(group, 'Nemotron-3 Diarization')).toContainText(/\d+%/);
  await shot(main, '03-downloading');
  await expect(row(group, 'Nemotron-3 Diarization')).toContainText('Active', { timeout: 20_000 });
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'true');
  await expect(offer).toHaveCount(0);
  await expect.poll(() => config(yap).meetingDiarization).toBe(true);
  expect(config(yap).diarizationModel).toBe('nemotron-3-diarization');

  // On disk: the model, the runtime's DLLs and licences (not its CLI, LLM
  // library or headers), and a NOTICE in each folder.
  expect(fs.statSync(folder(yap, 'nemotron-3-diarization', MODEL_FILE)).size).toBe(models.modelBytes);
  for (const dll of RUNTIME_DLLS) expect(fs.existsSync(folder(yap, 'nemo-speech-0.2.0-cpu', dll))).toBe(true);
  expect(fs.readFileSync(folder(yap, 'nemo-speech-0.2.0-cpu', 'licenses', 'LICENSE'), 'utf8')).toContain('Apache');
  expect(fs.existsSync(folder(yap, 'nemo-speech-0.2.0-cpu', 'nemo-speech.exe'))).toBe(false);
  expect(fs.existsSync(folder(yap, 'nemo-speech-0.2.0-cpu', 'llama.dll'))).toBe(false);
  expect(fs.readFileSync(folder(yap, 'nemotron-3-diarization', 'NOTICE.txt'), 'utf8')).toContain('OpenMDW');
  expect(fs.readFileSync(folder(yap, 'nemo-speech-0.2.0-cpu', 'NOTICE.txt'), 'utf8')).toContain('Apache');
  // Voice matching came with it (speakers/voiceprint.rs: ERes2Net, Apache-2.0).
  expect(fs.statSync(folder(yap, 'eres2net-voxceleb-en', VOICES_FILE)).size).toBe(models.voicesBytes);
  expect(fs.readFileSync(folder(yap, 'eres2net-voxceleb-en', 'NOTICE.txt'), 'utf8')).toContain('Apache');
  // Nothing half-done left behind.
  const left = fs.readdirSync(folder(yap)).filter((f) => /partial|installing/.test(f));
  expect(left).toEqual([]);
  expect(models.requests).toEqual(expect.arrayContaining(['/runtime.zip', '/model.gguf', '/voices.onnx']));
  const s = await status(yap);
  expect(nemotron(s).installed).toBe(true);
  expect(nemotron(s).downloadBytes).toBe(0);
  expect(nemotron(s).extrasBytes).toBe(0);
  await expect(group).not.toContainText('Needs a speaker model');
  await expect(group.locator('[data-setting="voice-matching"]')).toHaveCount(0);
  await shot(main, '04-installed');
  await closeSettings(main);
});

test('a speaker model from before voice matching offers it, and fetches only that', async ({ yap, main, models, shot }) => {
  // As a model installed before 25 Oct: no voice-matching folder.
  fs.rmSync(folder(yap, 'eres2net-voxceleb-en'), { recursive: true, force: true });
  const before = models.requests.length;
  const s = await status(yap);
  expect(nemotron(s).installed).toBe(true);
  expect(nemotron(s).extrasBytes).toBe(models.voicesBytes);
  expect(s.voiceMatching).toBe(false);

  // (Another page first: the speaker models' page asks again as it opens.)
  await openSettings(main, 'Meetings');
  await closeSettings(main);
  const dialog = await openSettings(main, 'Speech-to-Text');
  const group = speakersGroup(dialog);
  await group.scrollIntoViewIfNeeded();
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'true');
  const offer = group.locator('[data-setting="voice-matching"]');
  await expect(offer).toContainText("keeps a voice's number across a pause");
  await shot(main, '04b-voice-matching-offer');
  await offer.getByRole('button', { name: 'Download voice matching' }).click();
  await expect(offer).toHaveCount(0, { timeout: 20_000 });
  expect(models.requests.slice(before)).toEqual(['/voices.onnx']);
  expect(fs.statSync(folder(yap, 'eres2net-voxceleb-en', VOICES_FILE)).size).toBe(models.voicesBytes);
  await closeSettings(main);
});

test('Meetings has the same switch, and links to the models', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Meetings');
  const meetings = dialog.getByRole('group', { name: 'While a meeting records' });
  const toggle = theSwitch(meetings);
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '05-meetings-switch');

  // Off here is off there.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => config(yap).meetingDiarization).toBe(false);
  await meetings.getByRole('button', { name: 'Speaker models in Speech-to-Text' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Speech-to-Text' })).toBeVisible();
  const group = speakersGroup(dialog);
  await expect(group).toBeInViewport();
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'false');
  // And back on from here (the model is installed: no offer).
  await theSwitch(group).click();
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(() => config(yap).meetingDiarization).toBe(true);
  await closeSettings(main);
});

test('deleting the model takes the files away and turns the switch off', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Speech-to-Text');
  const group = speakersGroup(dialog);
  await group.scrollIntoViewIfNeeded();
  const rec = row(group, 'Nemotron-3 Diarization');
  await rec.hover();
  await rec.getByRole('button', { name: 'Delete model' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete Nemotron-3 Diarization?' });
  await expect(confirm).toContainText('Tell speakers apart turns off');
  await shot(main, '06-delete-confirm');
  // Cancel keeps it.
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toHaveCount(0);
  await expect(rec).toContainText('Active');
  await rec.hover();
  await rec.getByRole('button', { name: 'Delete model' }).click();
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(rec.getByRole('button', { name: 'Download' })).toBeVisible();
  await expect(theSwitch(group)).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => config(yap).meetingDiarization).toBe(false);
  // The model and, as nothing else uses it, the runtime.
  expect(fs.existsSync(folder(yap, 'nemotron-3-diarization'))).toBe(false);
  expect(fs.existsSync(folder(yap, 'nemo-speech-0.2.0-cpu'))).toBe(false);
  await shot(main, '07-deleted');
  await closeSettings(main);
});

test('a test run never downloads from the internet', async ({ yap, models }) => {
  // The model's source pointed at Hugging Face: refused before any request.
  models.setModelUrl(
    'https://huggingface.co/nvidia/Nemotron-3-Diarization/resolve/f667ed73aee57d40cc39428eb768b4fd87a0a29e/Nemotron-3-Diarization.q8_0.gguf'
  );
  try {
    const err = await yap.invoke('diarization_download', { id: 'nemotron-3-diarization' }).catch((e) => String(e));
    expect(err).toContain('Test runs never download from the internet');
    expect(nemotron(await status(yap)).installed).toBe(false);
    // The fallback can't be downloaded at all yet.
    const soon = await yap.invoke('diarization_download', { id: 'sherpa-pyannote-eres2net' }).catch((e) => String(e));
    expect(soon).toContain("isn't available yet");
    // And the debug-only real-engine command says what's missing.
    const wav = await yap.invoke('diarize_wav', { path: 'C:\\nowhere.wav' }).catch((e) => String(e));
    expect(wav).toContain('No speaker model is installed');
  } finally {
    models.setModelUrl(null);
  }
});
