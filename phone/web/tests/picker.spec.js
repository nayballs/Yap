// Chat's model picker on the phone (picker.js over the desktop's own
// core/chatPicker.js): the provider rail with its logos, each provider's
// Latest first and More models (N) opening in place, OpenAI's two ways as
// "ChatGPT", the on-device models, "Reads images", a switch sent as the
// desktop picker makes it, and the effort. Yap on the PC mocked
// (support/harness.js `pickerData`, fictional).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, pickerData, PC, MODELS, CAN } from './support/harness.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

const rows = (page) => page.getByTestId('picker-pane').getByTestId('picker-row');
const names = async (loc) => (await loc.locator('.nm').allTextContents()).map((s) => s.trim());

test("the rail with its logos, Latest, More models in place, and a switch", async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // The chip: the on-device model's family mark, its name, where it runs.
  const chip = page.getByTestId('model-chip');
  await expect(chip).toContainText('Qwen3.5 4B');
  await expect(chip).toContainText('On this PC');
  await expect(chip.locator('.pmark')).toHaveAttribute('data-icon', 'qwen');
  await chip.click();
  const sheet = page.getByTestId('models');
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole('heading')).toHaveText('Switch the Chat model');

  // The rail, in Chat's order, each provider with its logo and its dot.
  const rail = page.getByTestId('picker-rail');
  const tabs = rail.getByRole('tab');
  await expect(tabs).toHaveText(['Recent', 'Groq', 'Gemini', 'Claude', 'ChatGPT', 'OpenRouter', 'On this PC']);
  // Each says whether it's connected, for a screen reader too (the dot is green or hollow).
  for (const [i, name] of ['Groq, connected', 'Gemini, not set up', 'Claude, connected', 'ChatGPT, not set up', 'OpenRouter, not set up', 'On this PC, ready'].entries()) {
    await expect(tabs.nth(i + 1)).toHaveAccessibleName(name);
  }
  await expect(rail.locator('.st:not(.off)')).toHaveCount(3);
  for (const [tab, icon] of [
    ['Groq', 'groq'],
    ['Gemini', 'gemini'],
    ['Claude', 'claude'],
    ['ChatGPT', 'openai'],
    ['OpenRouter', 'openrouter'],
  ]) {
    await expect(rail.getByRole('tab', { name: new RegExp(`^${tab}`) }).locator('.pmark')).toHaveAttribute('data-icon', icon);
  }
  // The logos are the PC's own files, and they load.
  await expect(rail.locator('.pmark img').first()).toHaveJSProperty('complete', true);
  expect(await rail.locator('.pmark img').evaluateAll((imgs) => imgs.every((i) => i.naturalWidth > 0))).toBe(true);
  const masks = await rail.locator('.pmark .mask').evaluateAll((m) => m.map((e) => e.style.maskImage || e.style.webkitMaskImage));
  expect(masks.length).toBeGreaterThan(0);
  for (const m of masks) expect(m).toMatch(/^url\("providers\/[a-z-]+\.svg"\)$/);

  // It opens on the model in use: On this PC, ✓ on Qwen.
  await expect(rail.getByRole('tab', { name: /^On this PC/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('picker-pane')).toContainText('Private: nothing leaves this PC');
  await expect(rows(page).filter({ hasText: 'Qwen3.5 4B' })).toHaveAttribute('aria-pressed', 'true');
  await expect(rows(page).filter({ hasText: 'Qwen3.5 4B' })).toContainText('In use');
  // A curated model not downloaded: the PC's to download.
  await expect(page.getByTestId('picker-pane')).toContainText("Another model to download in Chat's model picker on your PC.");

  // Claude: Latest — the newest of each family, in Chat's order — then More models (7), closed.
  await rail.getByRole('tab', { name: /^Claude/ }).click();
  const pane = page.getByTestId('picker-pane');
  await expect(pane.locator('.ptitle')).toHaveText('Anthropic');
  await expect(pane.getByRole('group', { name: 'Latest' })).toBeVisible();
  expect(await names(pane.getByRole('group', { name: 'Latest' }))).toEqual(['Claude Opus 5.5', 'Claude Fable 5.2', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  await expect(pane.getByRole('group', { name: 'Latest' }).getByText('Reads images').first()).toBeVisible();
  const more = page.getByTestId('picker-more');
  await expect(more).toHaveText('More models (7)');
  await expect(more).toHaveAttribute('aria-expanded', 'false');
  await expect(rows(page)).toHaveCount(4);
  // More models opens in place, newest first.
  await more.click();
  await expect(page.getByTestId('picker-more')).toHaveAttribute('aria-expanded', 'true');
  await expect(rows(page)).toHaveCount(11);
  const older = (await names(rows(page))).slice(4);
  expect(older.slice(0, 2)).toEqual(['Claude Fable 5.1', 'Claude Opus 4.8']);
  expect(older).toContain('Claude Haiku 4.5');

  // Picked: the PC told as the desktop picker tells it; the chip at once.
  await rows(page).filter({ hasText: 'Claude Opus 4.8' }).click();
  await expect
    .poll(() => pc.of('set_model'))
    .toEqual([{ type: 'set_model', provider: 'anthropic', model: 'claude-opus-4-8', baseUrl: null, file: null }]);
  await expect(sheet).toBeHidden();
  await expect(chip).toContainText('Claude Opus 4.8');
  await expect(chip).toContainText('Anthropic');
  await expect(chip.locator('.pmark')).toHaveAttribute('data-icon', 'claude');

  // The PC's word: Chat on it now. Opened again, on Claude, More models
  // open where the model in use is, ✓ there.
  pc.picker = pickerData({ model: 'opus-4-8' });
  pc.model = { id: 'anthropic|claude-opus-4-8', label: 'Claude Opus 4.8 · Anthropic', onThisPc: false };
  pc.ready();
  await expect(chip).toContainText('Claude Opus 4.8');
  await chip.click();
  await expect(rail.getByRole('tab', { name: /^Claude/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('picker-more')).toHaveAttribute('aria-expanded', 'true');
  await expect(rows(page).filter({ hasText: 'Claude Opus 4.8' })).toHaveAttribute('aria-pressed', 'true');

  // Recent: the models used, as each provider lists them now.
  await rail.getByRole('tab', { name: /^Recent/ }).click();
  expect(await names(rows(page))).toEqual(['Claude Opus 4.8', 'Qwen3.5 4B']);
  // Each says where it runs (which way pays), as Chat's Recent does.
  await expect(rows(page).first().locator('.ds')).toHaveText(/^Claude · /);
});

test('Groq: its live list split by family; picking the on-device model back', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('model-chip').click();
  const rail = page.getByTestId('picker-rail');
  await rail.getByRole('tab', { name: /^Groq/ }).click();
  const pane = page.getByTestId('picker-pane');
  await expect(pane.locator('.ptitle')).toHaveText('Groq');
  await expect(pane.locator('.psub')).toHaveText(/^Connected · checked (just now|\d+ min ago)$/);
  expect(await names(pane.getByRole('group', { name: 'Latest' }))).toEqual(['GPT-OSS 120B', 'GPT-OSS 20B']);
  await expect(page.getByTestId('picker-more')).toHaveText('More models (1)');
  // The family's own mark on each row.
  await expect(rows(page).first().locator('.pmark')).toHaveAttribute('data-icon', 'openai-oss');
  await rows(page).filter({ hasText: 'GPT-OSS 20B' }).click();
  await expect.poll(() => pc.of('set_model').at(-1)).toEqual({ type: 'set_model', provider: 'groq', model: 'openai/gpt-oss-20b', baseUrl: null, file: null });
  // On this PC: the installed model, by its file.
  await page.getByTestId('model-chip').click();
  await rail.getByRole('tab', { name: /^On this PC/ }).click();
  await rows(page).filter({ hasText: 'Qwen3.5 4B' }).click();
  // (It was the one in use: picking it again just closes the sheet.)
  await expect(page.getByTestId('models')).toBeHidden();
  expect(pc.of('set_model')).toHaveLength(1);
});

test('a provider without a key: its models greyed, and how to connect it on the PC', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('model-chip').click();
  const rail = page.getByTestId('picker-rail');
  await rail.getByRole('tab', { name: /^Gemini/ }).click();
  await expect(page.getByTestId('picker-pane').locator('.psub')).toHaveText('Not set up');
  await expect(page.getByTestId('picker-connect')).toContainText('Connect Gemini');
  await expect(page.getByTestId('picker-connect')).toContainText('Settings → Language Models');
  const first = rows(page).first();
  await expect(first).toHaveAttribute('aria-disabled', 'true');
  await expect(first).toContainText('Add key');
  // A tap on one picks nothing (it points at how to connect).
  await first.click({ force: true });
  // ChatGPT: the plan first, as the desktop's connect card says it.
  await rail.getByRole('tab', { name: /^ChatGPT/ }).click();
  await expect(page.getByTestId('picker-connect')).toContainText('Use your ChatGPT plan');
  await page.waitForTimeout(200);
  expect(pc.of('set_model')).toHaveLength(0);
  await expect(page.getByTestId('models')).toBeVisible();
});

test('ChatGPT with both ways connected: "Your plan | API key"', async ({ page }) => {
  const picker = pickerData();
  picker.status.keyed = ['groq', 'anthropic', 'chatgpt', 'openai'];
  picker.catalog.chatgpt = {
    provider: 'chatgpt',
    ok: true,
    fetchedAt: Math.floor(Date.now() / 1000),
    checkedAt: 0,
    error: null,
    checking: false,
    models: [
      { id: 'gpt-6-astra', name: 'GPT-6 Astra' },
      { id: 'gpt-6-luna', name: 'GPT-6 Luna' },
    ],
  };
  const pc = await mockPc(page, { picker });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('model-chip').click();
  await page.getByTestId('picker-rail').getByRole('tab', { name: /^ChatGPT/ }).click();
  const ways = page.getByTestId('picker-ways');
  await expect(ways.getByRole('radio', { name: 'Your plan' })).toBeChecked();
  await expect(page.getByTestId('picker-pane').locator('.psub')).toHaveText('Using ChatGPT plan');
  expect(await names(rows(page))).toEqual(['GPT-6 Astra', 'GPT-6 Luna']);
  await ways.getByRole('radio', { name: 'API key' }).check();
  await expect(ways.getByRole('radio', { name: 'API key' })).toBeChecked();
  // The key's own list: Yap's curated OpenAI models, Latest first.
  await expect(page.getByTestId('picker-pane').getByRole('group', { name: 'Latest' })).toBeVisible();
  await rows(page).first().click();
  await expect.poll(() => pc.of('set_model').at(-1)?.provider).toBe('openai');
});

test('the effort of the model in use, and Fast mode', async ({ page }) => {
  const effort = { levels: ['low', 'medium', 'high', 'xhigh', 'max'], default: 'medium', chosen: null, fast: false };
  const pc = await mockPc(page, {
    picker: pickerData({ model: 'opus', effort }),
    model: { id: 'anthropic|claude-opus-5-5', label: 'Claude Opus 5.5 · Anthropic', onThisPc: false },
  });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('model-chip').click();
  const box = page.getByTestId('picker-effort');
  await expect(box).toContainText('Effort · Claude Opus 5.5');
  await expect(box.getByRole('radio', { name: 'Medium' })).toBeChecked();
  await expect(box).toContainText("Medium: balanced, for everyday questions. The model's default.");
  await box.getByRole('radio', { name: 'Extra' }).check();
  await expect.poll(() => pc.of('set_effort')).toEqual([{ type: 'set_effort', level: 'xhigh' }]);
  await expect(page.getByTestId('picker-effort').getByRole('radio', { name: 'Extra' })).toBeChecked();
  // Fast mode's line names the model in use (Opus 5.5 and Fable 5.2 have it).
  await expect(page.getByTestId('picker-effort')).toContainText('Faster answers from Claude Opus 5.5, at twice the price per token.');
  await page.getByTestId('picker-effort').getByText('Fast mode').click();
  await expect.poll(() => pc.of('set_fast')).toEqual([{ type: 'set_fast', on: true }]);
  // The on-device model has no effort: none shown.
  pc.picker = pickerData();
  pc.model = MODELS[0];
  pc.ready();
  await expect(page.getByTestId('picker-effort')).toHaveCount(0);
});

test('a PC from before the picker: no rail, its plain list', async ({ page }) => {
  const pc = await mockPc(page, { old: true });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('model-chip').click();
  await expect(page.getByTestId('picker-rail')).toHaveCount(0);
  await expect(page.getByTestId('picker-effort')).toHaveCount(0);
  expect(await names(rows(page))).toEqual(['Qwen3.5 4B', 'Claude Haiku 5.5']);
  expect(CAN).toContain('picker');
  expect(PC).toBe('Office PC');
});
