// The on-device model reading screenshots (Snip to Chat, phase 3,
// src-tauri/src/ai/local_llm/vision.rs, docs/architecture/snip.md):
// - Chat on Qwen3.5 4B on this PC: a screenshot's line offers "Read
//   screenshots on this PC" (its projector, a download) beside "Send its
//   text instead"; downloaded there (its progress in the line), the line
//   goes and the question goes with the picture to the model on this PC,
//   which loaded its projector for it;
// - a model already running without it: a screenshot restarts it with the
//   projector — never while a dictation is going on — and the picture goes;
// - Settings → Language Models: the switch (off: Chat's line says so, with
//   "Turn it on"), Remove, and the download there; the Chat picker's
//   "Reads images" on the 4B.
//
// No real model or projector: the model's files are empty stand-ins
// (support/local-llm.js), the projector a few random MB from a local
// server (YAP_LLM_SOURCES, "vision:qwen3.5-4b"), the started model a fake
// AI on 127.0.0.1 (YAP_E2E_LOCAL_LLM) that records the image parts.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { installStandInModel, startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const CARD = 'NVIDIA GeForce RTX 5070 Ti';
const MODEL_FILE = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';
const PROJECTOR = 'mmproj-Qwen_Qwen3.5-4B-f16.gguf';

const test = base.extend({
  fake: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  localModels: [
    async ({}, use) => {
      const fake = await startFakeLocalModels(path.join(RUNS, 'llm-sources-vision'), ['vision:qwen3.5-4b']);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fake, localModels }, use) => {
      await use({
        name: 'local-vision',
        env: {
          YAP_E2E_GPU: `${CARD},15995`,
          YAP_E2E_LOCAL_LLM: fake.base,
          YAP_E2E_LOCAL_LLM_LOAD_MS: '300',
          ...localModels.env,
        },
        config: { postProcessEnabled: true, ppProvider: 'ondevice', ppLocalModel: MODEL_FILE, meetingReminder: 'never' },
        prepare: (dir) => installStandInModel(dir, MODEL_FILE),
      });
    },
    { scope: 'worker' },
  ],
});

const MONITORS = [{ x: 0, y: 0, width: 1280, height: 720, scale: 1 }];
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const attachments = (main) => main.getByTestId('chat-attachments');
const visionLine = (main) => main.getByTestId('chat-vision-line');
const withImages = (fake, from) => fake.requests.slice(from).filter((r) => r.images?.length);
const vision = (yap) => yap.invoke('e2e_local_vision');
const projectorFile = (yap) => path.join(yap.dataDir, 'llm', 'vision', PROJECTOR);

/** A snip over a fake monitor into Chat: the screenshot waiting in the box. */
async function snipIntoChat(yap, main) {
  await yap.invoke('e2e_snip', { monitors: MONITORS });
  await expect.poll(async () => (await yap.invoke('e2e_snip_last'))?.outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', {
    screen: 0,
    events: [
      { kind: 'down', x: 40, y: 40 },
      { kind: 'move', x: 140, y: 90 },
      { kind: 'up', x: 240, y: 140 },
    ],
  });
  await expect.poll(async () => (await yap.invoke('e2e_snip_last'))?.outcome).toBe('snipped');
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(attachments(main)).toBeVisible();
}

test("Chat on the on-device model: a screenshot offers its reader; downloaded there, the picture goes to the model on this PC", async ({
  yap,
  main,
  fake,
  localModels,
  shot,
}) => {
  // Not downloaded: the model can't read it, and says what would let it.
  const before = await yap.invoke('ai_model_status', { scope: 'chat' });
  expect(before.images).toBe(false);
  expect(before.imagesLocal).toMatchObject({ model: 'qwen3.5-4b', sizeMb: 642, installed: false, failed: false });
  await snipIntoChat(yap, main);
  await expect(visionLine(main)).toContainText('can read screenshots on this PC with a 642 MB download');
  const download = visionLine(main).getByRole('button', { name: 'Download it (642 MB)' });
  await expect(download).toBeVisible();
  await expect(visionLine(main).getByRole('button', { name: 'Send its text instead' })).toBeVisible();
  await expect(main.getByRole('button', { name: 'Send', exact: true })).toBeDisabled();
  await shot(main, '01-offer-in-chat');

  // Downloaded from the line: its progress there, held half-way.
  localModels.hold();
  await download.click();
  const progress = main.getByTestId('chat-vision-download');
  await expect(progress).toBeVisible();
  await expect(progress).toContainText('Downloading the screenshot reader');
  await shot(main, '02-downloading-in-chat');
  localModels.release();
  // Whole: the line goes, the file is in llm/vision/, the setting on.
  await expect(visionLine(main)).toBeHidden({ timeout: 15_000 });
  expect(fs.existsSync(projectorFile(yap))).toBe(true);
  await expectStore(yap, 'config.json', (c) => c.localVision === true);
  expect((await yap.invoke('ai_model_status', { scope: 'chat' })).images).toBe(true);
  // Never listed as a model of its own.
  expect((await yap.invoke('local_llm_status')).models).not.toContain(PROJECTOR);

  // The question goes with the picture; the model loaded its projector for it.
  const from = fake.requests.length;
  await box(main).fill('What does it say?');
  await box(main).press('Enter');
  await expect.poll(() => withImages(fake, from).length, { timeout: 15_000 }).toBe(1);
  expect(withImages(fake, from)[0].user).toContain('What does it say?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
  const v = await vision(yap);
  expect(v.ensures).toBeGreaterThanOrEqual(1);
  expect(v.loaded).not.toBeNull();
  await shot(main, '03-answered-on-this-pc');
});

test('a model running without it: a screenshot restarts it with its reader, never during a dictation', async ({ yap, main, fake }) => {
  // Freed, then loaded plain (as a dictation would load it).
  await yap.invoke('local_llm_stop');
  await yap.invoke('local_llm_start');
  await expect.poll(async () => (await vision(yap)).loaded).toBeNull();
  const starts = (await yap.invoke('e2e_local_llm_loads')).starts;

  await openView(main, 'Home');
  await snipIntoChat(yap, main);
  await expect(visionLine(main)).toBeHidden();
  // A dictation is going on: the question waits for it.
  await yap.invoke('e2e_local_llm_busy', { busy: true });
  const from = fake.requests.length;
  try {
    await box(main).fill('Read this');
    await box(main).press('Enter');
    await main.waitForTimeout(1200);
    expect(withImages(fake, from)).toHaveLength(0);
    expect((await yap.invoke('e2e_local_llm_loads')).starts).toBe(starts);
  } finally {
    await yap.invoke('e2e_local_llm_busy', { busy: false });
  }
  // Over: restarted with the projector, and the picture goes.
  await expect.poll(() => withImages(fake, from).length, { timeout: 15_000 }).toBe(1);
  expect((await yap.invoke('e2e_local_llm_loads')).starts).toBe(starts + 1);
  expect((await vision(yap)).loaded).not.toBeNull();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
});

test('Settings: the switch, Remove and the download; off, Chat says so with "Turn it on"', async ({ yap, main, localModels, shot }) => {
  // Downloaded by the first test (or here, when this one runs alone).
  if (!(await yap.invoke('local_llm_status')).vision?.installed) await yap.invoke('local_vision_install');
  // The Chat model picker: the 4B "Reads images" on this PC now.
  await openView(main, 'Chat');
  await main.locator('.modelchip .chip').click();
  const pop = main.getByRole('dialog', { name: 'Switch the Chat model' });
  await expect(pop).toBeVisible();
  await pop.getByRole('tab', { name: /^On this PC/ }).click();
  const pickerRow = pop.locator('.pane .mrow', { has: main.locator('.nm', { hasText: /^Qwen3\.5 4B$/ }) });
  await expect(pickerRow.locator('.tag.img')).toHaveText('Reads images');
  await main.keyboard.press('Escape');
  await expect(pop).toBeHidden();

  await openSettings(main, 'About');
  let dialog = await openSettings(main, 'Language Models');
  const row = dialog.getByTestId('local-vision');
  await expect(row).toBeVisible();
  const toggle = row.getByRole('button', { name: 'Read screenshots on this PC' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '04-settings-on');
  // The Chat picker's rows: the 4B reads images now.
  const st = await yap.invoke('local_llm_status');
  expect(st.vision).toMatchObject({ model: 'qwen3.5-4b', installed: true, on: true });
  expect(st.curated.find((m) => m.id === 'qwen3.5-4b')).toMatchObject({ readsImages: true, projectorMb: 642 });

  // Off: Chat's line says so, with "Turn it on".
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.localVision === false);
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await closeSettings(main);
  await openView(main, 'Home');
  await snipIntoChat(yap, main);
  await expect(visionLine(main)).toContainText('Reading screenshots on this PC is off.');
  await visionLine(main).getByRole('button', { name: 'Turn it on' }).click();
  await expect(visionLine(main)).toBeHidden();
  await expectStore(yap, 'config.json', (c) => c.localVision === true);
  await attachments(main).getByRole('button', { name: 'Remove the screenshot' }).click();

  // Remove: the file goes, and the row offers the download again.
  await openSettings(main, 'About');
  dialog = await openSettings(main, 'Language Models');
  await dialog.getByTestId('local-vision').getByRole('button', { name: 'Remove (642 MB)' }).click();
  await expect(dialog.getByTestId('local-vision').getByRole('button', { name: 'Download (642 MB)' })).toBeVisible();
  expect(fs.existsSync(projectorFile(yap))).toBe(false);
  expect((await yap.invoke('ai_model_status', { scope: 'chat' })).images).toBe(false);
  await dialog.getByTestId('local-vision').scrollIntoViewIfNeeded();
  await shot(main, '05-settings-removed');

  // Downloaded from Settings: the switch again.
  localModels.hold();
  await dialog.getByTestId('local-vision').getByRole('button', { name: 'Download (642 MB)' }).click();
  await expect(dialog.getByTestId('local-vision').getByTestId('download-progress')).toBeVisible();
  localModels.release();
  await expect(dialog.getByTestId('local-vision').getByRole('button', { name: 'Read screenshots on this PC' })).toBeVisible({ timeout: 15_000 });
  expect(fs.existsSync(projectorFile(yap))).toBe(true);
  await closeSettings(main);
});
