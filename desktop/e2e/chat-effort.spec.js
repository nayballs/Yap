// Effort, per model, in Chat's model picker (desktop/src-tauri/src/ai/effort.rs,
// desktop/src/lib/views/chat/ChatModelPicker.svelte, ModelChip.svelte), as the Claude
// Code app's model menu has it:
// - the chip names the model and its effort ("Claude Opus 5.5 Medium": its
//   default until one is chosen);
// - the picker's Effort shows only the levels the model in use takes (five
//   for Opus 5.5, three for Groq's GPT-OSS, none for a model without a
//   reasoning control or the one on this PC);
// - a level chosen there is on the chip at once, kept for that model alone
//   (config.json `chatEffort`), and sent the provider's way — Claude on
//   Anthropic's Messages API with adaptive thinking, its summary folded
//   above the answer;
// - Fast mode for Claude Opus 5.5 and Claude Fable 5.2: off until switched
//   on, then `speed: "fast"`, its line naming the model in use;
// - Claude declining a request (`stop_reason: "refusal"`) is said in plain
//   words; thinking the API won't take back is left out and asked again,
//   and forgotten for the rest of the turn;
// - Settings → Language Models → Chat has the same choice, and the two
//   agree;
// - Think is gone for a cloud model and there for the on-device one.
// The models are a local fake (support/fake-llm.js, speaking Anthropic's
// Messages API too); the on-device model an empty stand-in started as the
// fake (support/local-llm.js). Nothing goes on the internet.
import { test as base, expect, openView, openSettings, closeSettings, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { installStandInModel } from './support/local-llm.js';

const CLAUDE_KEY = 'sk-ant-e2e-fake-1234567890';
const QWEN = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';
const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

function claude(fakeLlm, model) {
  return { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model, apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-effort',
        env: { YAP_E2E_GPU: 'NVIDIA GeForce RTX 4070,12282', YAP_E2E_LOCAL_LLM: fakeLlm.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '200' },
        config: {
          postProcessEnabled: false,
          ppProvider: 'ondevice',
          ppLocalModel: QWEN,
          ppApiKeys: { anthropic: CLAUDE_KEY },
          llmScopes: { chat: claude(fakeLlm, 'claude-opus-5-5') },
          meetingReminder: 'never',
        },
        prepare: (dir) => installStandInModel(dir, QWEN),
      });
    },
    { scope: 'worker' },
  ],
});

const chip = (main) => main.locator('.modelchip .chip');
const chipEffort = (main) => main.getByTestId('chip-effort');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const effortStrip = (main) => picker(main).getByTestId('chat-effort');
const levels = (main) => effortStrip(main).getByRole('tab');
const fastSwitch = (main) => effortStrip(main).getByRole('button', { name: 'Fast mode' });
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const thinkButton = (main) => main.getByRole('button', { name: 'Think it through' });

async function openPicker(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
}

async function closePicker(main) {
  await main.keyboard.press('Escape');
  await expect(picker(main)).toBeHidden();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** A step held until the spec lets it go. */
function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

/** The thinking a Messages API request sends back, in order. */
const thinkingIn = (request) =>
  request.body.messages
    .flatMap((m) => (Array.isArray(m.content) ? m.content : []))
    .filter((b) => b.type === 'thinking')
    .map((b) => b.thinking);

test('effort: chosen in the picker for the model in use, on the chip, kept per model and sent', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  // Its default until one is chosen: medium on Opus 5.5.
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic API key');
  await openPicker(main);
  await expect(effortStrip(main)).toContainText('Claude Opus 5.5');
  await expect(levels(main)).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(levels(main).filter({ hasText: 'Medium' })).toHaveAttribute('aria-selected', 'true');
  await expect(effortStrip(main).getByTestId('chat-effort-hint')).toHaveText("Medium: balanced, for everyday questions. The model's default.");
  // Fast mode: Opus 5.5's, off until switched on, its price said.
  await expect(fastSwitch(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(effortStrip(main)).toContainText('Faster answers from Claude Opus 5.5, at twice the price per token.');
  await shot(main, '01-effort-default');

  // Extra: on the chip at once, kept for this model.
  await levels(main).filter({ hasText: 'Extra' }).click();
  await expect(levels(main).filter({ hasText: 'Extra' })).toHaveAttribute('aria-selected', 'true');
  await expect(chipEffort(main)).toHaveText('Extra');
  await expect(effortStrip(main).getByTestId('chat-effort-hint')).toHaveText('Extra: thinks longer, for hard problems.');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'xhigh');
  await fastSwitch(main).click();
  await expect(fastSwitch(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic API key');
  await expectStore(yap, 'config.json', (c) => c.fastMode === true);
  await shot(main, '02-effort-extra-fast');
  await closePicker(main);

  // Asked at Extra: adaptive thinking, its summary folded above the answer,
  // room to think, fast. No Think button for a cloud model.
  await freshChat(main);
  await expect(thinkButton(main)).toHaveCount(0);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ reasoning: 'Three rooms, then the hall: four coats in all.', content: 'Four coats of paint.', plain: true }]);
  await send(main, 'How many coats of paint for the hall and three rooms?');
  await expect(thread(main)).toContainText('Four coats of paint.', { timeout: 15_000 });
  const asked = fakeLlm.requests.slice(before).find((r) => r.api === 'messages' && r.user?.includes('coats of paint'));
  expect(asked.think.outputConfig).toEqual({ effort: 'xhigh' });
  expect(asked.think.thinking).toEqual({ type: 'adaptive', display: 'summarized' });
  expect(asked.maxTokens).toBe(64000);
  expect(asked.body.speed).toBe('fast');
  const thought = main.getByTestId('chat-thought').last();
  await expect(thought).toContainText(/Thought/);
  await thought.locator('summary').click();
  await expect(thought).toContainText('four coats in all');
  await shot(main, '03-answer-at-extra');

  // Per model: Sonnet 5.5 has its own (high, its default), and no fast mode.
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-sonnet-5-5') } });
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 High · Anthropic API key');
  await openPicker(main);
  await expect(levels(main)).toHaveCount(5);
  await expect(fastSwitch(main)).toHaveCount(0);
  await closePicker(main);
  // Fable 5.2: its own effort (high, its default) and Fast mode, the one
  // switch still on, its line naming Fable.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-fable-5-2') } });
  await expect(chip(main)).toHaveText('Claude Fable 5.2 High · Fast · Anthropic API key');
  await openPicker(main);
  await expect(levels(main)).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(fastSwitch(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(effortStrip(main)).toContainText('Faster answers from Claude Fable 5.2, at twice the price per token.');
  await shot(main, '06-fable-fast-mode');
  await closePicker(main);
  const fableAsked = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'One coat.', plain: true }]);
  await freshChat(main);
  await send(main, 'And for the shed?');
  await expect(thread(main)).toContainText('One coat.', { timeout: 15_000 });
  const fable = fakeLlm.requests.slice(fableAsked).find((r) => r.api === 'messages' && r.user?.includes('the shed'));
  expect([fable.model, fable.body.speed, fable.think.outputConfig]).toEqual(['claude-fable-5-2', 'fast', { effort: 'high' }]);
  // A Claude with no effort control: no Effort at all.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-haiku-4-5') } });
  await expect(chip(main)).toHaveText('Claude Haiku 4.5 · Anthropic API key');
  await openPicker(main);
  await expect(effortStrip(main)).toHaveCount(0);
  await closePicker(main);
  // Back on Opus 5.5: Extra, as it was left.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic API key');

  // Back to its default, fast mode off.
  await openPicker(main);
  await levels(main).filter({ hasText: 'Medium' }).click();
  await fastSwitch(main).click();
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic API key');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'medium' && c.fastMode === false);
  await closePicker(main);
});

test('Claude declines a request: said in plain words, the question handed back', async ({ main, fakeLlm, shot }) => {
  await freshChat(main);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ declined: 'cyber', plain: true }]);
  await send(main, 'Something its safety check stops.');
  const toast = main.getByRole('status').filter({ hasText: "Couldn't send" });
  await expect(toast).toContainText("Claude declined to answer that: Anthropic's safety check stopped it. Try wording it another way, or pick another model.", { timeout: 15_000 });
  // A sentence, never the raw error or "an empty answer"; the question is given back.
  await expect(toast.locator('.errbox')).toHaveCount(0);
  await expect(main.getByText(/empty answer/i)).toHaveCount(0);
  await expect(box(main)).toHaveValue('Something its safety check stops.');
  await shot(main, '07-claude-declined');
  // Asked once: a decline isn't asked again.
  expect(fakeLlm.requests.slice(before).filter((r) => r.api === 'messages' && r.user?.includes('safety check stops')).length).toBe(1);
  await box(main).fill('');
});

test('thinking the API refuses to take back: asked again without it, and forgotten for the rest of the turn', async ({ main, fakeLlm }) => {
  await freshChat(main);
  const held = gate();
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { gate: held.promise, reasoning: 'Look in the notes first.', tools: [{ name: 'search_notes', args: { query: 'paint' } }] },
    { reasoning: 'Nothing there; one more look.', tools: [{ name: 'search_notes', args: { query: 'coats' } }] },
    { content: 'Your notes don’t say.' },
  ]);
  await send(main, 'How many coats of paint did my notes say?');
  await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.gated)).toBe(true);
  // Its next step is refused once, in Anthropic's own words (a cause Yap
  // didn't foresee: the stand-in is told to).
  fakeLlm.failNext('other', 1, 400, {
    type: 'error',
    error: { type: 'invalid_request_error', message: 'messages.1.content.0: Invalid `signature` in `thinking` block. The block is bound to a different conversation. Remove the block.' },
  });
  held.open();
  await expect(thread(main)).toContainText('Your notes don’t say.', { timeout: 20_000 });
  await expect(main.getByText("Couldn't send")).toHaveCount(0);
  // The first step; the refused one; the same again without its thinking;
  // and the last, with the later step's thinking only — the loop forgot the
  // first too, or the stand-in would have refused it (a block put back in
  // front of one that never followed it).
  const steps = fakeLlm.requests.slice(before).filter((r) => r.api === 'messages' && r.kind === 'other');
  expect(steps.map(thinkingIn)).toEqual([[], ['Look in the notes first.'], [], ['Nothing there; one more look.']]);
  expect(steps.some((s) => s.unboundThinking)).toBe(false);
});

test('the stand-in checks Claude’s thinking as Anthropic documents it (no app)', async ({ fakeLlm }) => {
  // What the two tests around this one lean on: thinking sent back after
  // the tools changed is refused; the oldest thinking may be left out; a
  // block put back in front of one that never followed it is refused.
  const tool = { name: 'search_notes', description: 'Search the notes.', input_schema: { type: 'object' } };
  const web = { type: 'web_search_20260209', name: 'web_search', max_uses: 5 };
  const asked = { role: 'user', content: [{ type: 'text', text: 'Hi.' }] };
  const result = (answer) => ({ role: 'user', content: [{ type: 'tool_result', tool_use_id: answer.content.find((b) => b.type === 'tool_use').id, content: 'Nothing.' }] });
  const post = async (messages, tools, step) => {
    if (step) fakeLlm.scriptChat([step]);
    const r = await fetch(`${fakeLlm.base}/messages`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ model: 'claude-fable-5-2', max_tokens: 100, system: [{ type: 'text', text: 'A test.' }], tools, messages }),
    });
    return { status: r.status, ...(await r.json()) };
  };
  const call = (thought) => ({ reasoning: thought, tools: [{ name: 'search_notes', args: { query: 'x' } }] });
  const without = (answer) => ({ role: 'assistant', content: answer.content.filter((b) => b.type !== 'thinking') });
  const turn = (answer) => ({ role: 'assistant', content: answer.content });

  const one = await post([asked], [tool], call('First.'));
  expect(one.content.map((b) => b.type)).toEqual(['thinking', 'tool_use']);
  // Sent back as it came, the tools the same: taken, and the next thinks on.
  const two = await post([asked, turn(one), result(one)], [tool], call('Second.'));
  expect(two.status).toBe(200);
  // The tools changed since the first thought: refused, in Anthropic's words.
  const changed = await post([asked, turn(one), result(one)], [tool, web]);
  expect(changed.status).toBe(400);
  expect(changed.error.message).toContain('messages.1.content.0: Invalid `signature` in `thinking` block. The block is bound to a different conversation.');
  // Without that thinking: taken.
  expect((await post([asked, without(one), result(one)], [tool, web], { content: 'OK.' })).status).toBe(200);
  // Both sent back, the tools as they were: taken. The oldest left out: taken too.
  const both = [asked, turn(one), result(one), turn(two), result(two)];
  expect((await post(both, [tool], { content: 'OK.' })).status).toBe(200);
  expect((await post([asked, without(one), result(one), turn(two), result(two)], [tool], { content: 'OK.' })).status).toBe(200);
  // A thought made with no thinking before it, then one put back in front: refused.
  const bare = [asked, without(one), result(one), without(two), result(two)];
  const three = await post(bare, [tool], call('Third.'));
  expect(three.status).toBe(200);
  const putBack = await post([asked, without(one), result(one), turn(two), result(two), turn(three), result(three)], [tool]);
  expect(putBack.status).toBe(400);
  expect(putBack.error.message).toContain('messages.5.content.0');
  // An earlier message changed: refused.
  const edited = await post([{ role: 'user', content: [{ type: 'text', text: 'Hello there.' }] }, turn(one), result(one)], [tool]);
  expect(edited.status).toBe(400);
});

test('Settings → Language Models → Chat has the same effort for the Chat model', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(chipEffort(main)).toHaveText('Medium');
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  const choice = dialog.getByRole('tablist', { name: 'Effort' });
  await expect(choice.getByRole('tab')).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(choice.getByRole('tab', { name: 'Medium' })).toHaveAttribute('aria-selected', 'true');
  await choice.getByRole('tab', { name: 'High' }).click();
  await expect(dialog.getByTestId('settings-effort-hint')).toHaveText('High: thinks more, for detailed work.');
  await shot(main, '05-settings-effort');
  // Saved by Settings' auto-save, under the model's key; the chip follows.
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'high');
  await closeSettings(main);
  await expect(chipEffort(main)).toHaveText('High');
  // Chosen in the picker: Settings has it next time it's opened.
  await openPicker(main);
  await levels(main).filter({ hasText: 'Low' }).click();
  await closePicker(main);
  const again = await openSettings(main, 'Language Models');
  await again.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  await expect(again.getByRole('tablist', { name: 'Effort' }).getByRole('tab', { name: 'Low' })).toHaveAttribute('aria-selected', 'true');
  await again.getByRole('tablist', { name: 'Effort' }).getByRole('tab', { name: 'Medium' }).click();
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'medium');
  await closeSettings(main);
});

test('Think: hidden for a cloud model, shown for the one on this PC, which has no effort', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await expect(chip(main)).toContainText('Claude Opus 5.5');
  await expect(box(main)).toBeVisible();
  await expect(thinkButton(main)).toHaveCount(0);

  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: ON_DEVICE_CHAT } });
  try {
    await freshChat(main);
    await expect(chip(main)).toHaveText('Qwen3.5 4B · On this PC');
    await expect(thinkButton(main)).toBeVisible({ timeout: 15_000 });
    await shot(main, '04-think-on-this-pc');
    await openPicker(main);
    await expect(effortStrip(main)).toHaveCount(0);
    await closePicker(main);
  } finally {
    await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  }
  await freshChat(main);
  await expect(chip(main)).toContainText('Claude Opus 5.5');
  await expect(thinkButton(main)).toHaveCount(0);
});
