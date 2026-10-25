// "Give it more room" (src-tauri/src/chat/turn.rs `chat_limits`, chat/budget.rs;
// src/lib/views/chat/Composer.svelte), 2026-10-25: a paste too long for the
// on-device model at its context is said plainly under the box, never sent,
// with a button that raises the model's context to the next size that fits
// the graphics card (Settings → Language Models → Local does the same) —
// only when the card has the room.
//
// The model is a stand-in GGUF header with Qwen3.5-4B's real shape and no
// weights (support/gguf.js), "running" at 8K (`e2e_local_llm_serving`), on a
// stand-in 8 GB card (YAP_E2E_GPU): ~5.8 GB of room beside Windows and
// speech recognition. Nothing is ever sent to it.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, expectStore } from './support/fixtures.js';
import { qwen35_4b, writeModel } from './support/gguf.js';

const CARD = 'NVIDIA GeForce RTX 4060';
const BIG_FILE = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';

function models(dataDir) {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  writeModel(path.join(llm, BIG_FILE), qwen35_4b());
}

const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

test.use({
  yapOptions: {
    name: 'chat-room',
    env: { YAP_E2E_GPU: `${CARD},7957` },
    config: {
      postProcessEnabled: true,
      ppProvider: 'ondevice',
      ppLocalModel: BIG_FILE,
      llmScopes: { chat: ON_DEVICE_CHAT },
    },
    prepare: models,
  },
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const tooLong = (main) => main.getByTestId('chat-too-long');

test('a paste too long for the on-device model offers to give it more room, and does', async ({ yap, main, shot }) => {
  await yap.invoke('local_llm_set_context', { tokens: 8192 });
  await yap.invoke('e2e_local_llm_serving', { file: BIG_FILE, gpuLayers: 34, layers: 34, ctx: 8192, usedMib: null });
  // What a question may be: the on-device model's real context, and the
  // next size that fits the card.
  const limits = await yap.invoke('chat_limits');
  expect(limits).toMatchObject({ context: 8192, contextKnown: true, label: 'Qwen3.5 4B', more: 16384 });
  expect(limits.tooLong).toBe("That's too long for Qwen3.5 4B (8K context).");
  expect(limits.questionMax).toBeGreaterThan(2_000);
  expect(limits.questionMax).toBeLessThan(5_800);

  await openView(main, 'Chat');
  await main.locator('.convs').getByRole('button', { name: 'New chat', exact: true }).click();
  await box(main).fill('Here are the minutes from every board meeting this year. '.repeat(700));
  const line = tooLong(main);
  await expect(line).toContainText("That's too long for Qwen3.5 4B (8K context).");
  await expect(main.getByRole('button', { name: 'Send' })).toBeDisabled();
  await shot(main, '01-give-it-more-room');
  await line.getByRole('button', { name: 'Give it more room' }).click();
  // Saved as Settings saves it; the model restarts with it once idle.
  await expectStore(yap, 'config.json', (c) => c.localContext === 16384, 'the context raised to 16K');
  await expect(main.getByText('Giving Qwen3.5 4B 16K of context')).toBeVisible();
  // Room for more now; this paste still needs more than that.
  await expect.poll(async () => (await yap.invoke('chat_limits')).context).toBe(16384);
  await box(main).fill('');
  await yap.invoke('local_llm_set_context', { tokens: 8192 });
});

test('no more room is offered past what the card can hold', async ({ yap }) => {
  await yap.invoke('local_llm_set_context', { tokens: 131072 });
  await yap.invoke('e2e_local_llm_serving', { file: BIG_FILE, gpuLayers: 34, layers: 34, ctx: 131072, usedMib: null });
  await expect.poll(async () => (await yap.invoke('chat_limits')).context).toBe(131072);
  // 256K takes more than the card's ~5.8 GB of room.
  expect((await yap.invoke('chat_limits')).more).toBeNull();
  await yap.invoke('local_llm_set_context', { tokens: 8192 });
});
