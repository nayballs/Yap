// Snip to Chat (src-tauri/src/snip.rs, docs/architecture/snip.md): drag a
// box over any screen and the picture goes into Chat for the selected model.
//
// Test mode never reads the real screen: `e2e_snip` (shell/e2e.rs) hands the snip
// fake monitors — a 1280×720 HDR one at 240 nits (its picture composed in
// FP16 as Windows would and converted back by the real snip/hdr.rs path) and
// a 960×540 one at 150 % beside it — each light grey (#F5F5F5) with four
// 200×100 blocks: (40, 40) #C0C0C0, (260, 40) #E53935, (40, 160) #202020,
// (260, 160) #808080. Its overlay windows are made, painted and driven by
// posted window messages (`e2e_snip_input`), but never shown; the real
// mouse and keyboard are never touched.
//
// - The shortcut's snip lands in a new chat: the screenshot in the message
//   box, the caret there, the three prompts; its pixels exact (the HDR one).
// - Enter on an empty box asks "What's this?" with an `image_url` part; the
//   question keeps the screenshot as an id in chats.json (no pixels there).
// - The prompts send in one tap; a snip on the 150 % monitor is exact too.
// - Esc, right-click and a plain click snip nothing.
// - The conversation open in Chat gets the snip.
// - A model that can't read images says so in one line, with the switch to
//   one that can (Groq's GPT-OSS → Qwen3.8 27B); a server that refuses the
//   image gets the same line and keeps the question.
// - Saying "snip" takes one, as a dictation or to the Voice Assistant; "snip
//   the ends off" is dictated.
// - Settings: the shortcut (recorded, clash-checked), the voice switch, how
//   long screenshots are kept; ✂ on the bar, Chat's + and the in-page
//   Win+Alt+S start one.
// - Kept for their time: a screenshot past "Keep chat screenshots" goes and
//   its message says so; deleting a conversation deletes its screenshots.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { readPng, readDataUrl, worstError } from './support/png.js';

const TEST_KEY = 'e2e-fake-key';

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
        name: 'snip',
        config: {
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: groqChat(fakeLlm, 'qwen/qwen3.8-27b') },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** A Chat scope on the fake, as Groq (config.rs `LlmScope`). */
function groqChat(fakeLlm, model, provider = 'groq') {
  return { enabled: true, provider, baseUrl: fakeLlm.base, model, apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

const MONITORS = [
  { x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: true, whiteNits: 240 },
  { x: 1280, y: 0, width: 960, height: 540, scale: 1.5 },
];
const GREY = [0xc0, 0xc0, 0xc0];
const RED = [0xe5, 0x39, 0x35];

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const attachments = (main) => main.getByTestId('chat-attachments');
const visionLine = (main) => main.getByTestId('chat-vision-line');

async function last(yap) {
  return yap.invoke('e2e_snip_last');
}

/** A snip over the fake monitors, its overlay up. */
async function startSnip(yap, opts = {}) {
  await yap.invoke('e2e_snip', { monitors: MONITORS, ...opts });
  await expect.poll(async () => (await last(yap))?.outcome, { message: 'the overlay to come up' }).toBe('selecting');
}

/** A drag on a fake monitor's overlay (window pixels), released: the image. */
async function drag(yap, screen, [x0, y0], [x1, y1]) {
  await yap.invoke('e2e_snip_input', {
    screen,
    events: [
      { kind: 'down', x: x0, y: y0 },
      { kind: 'move', x: (x0 + x1) >> 1, y: (y0 + y1) >> 1 },
      { kind: 'up', x: x1, y: y1 },
    ],
  });
  await expect.poll(async () => (await last(yap))?.outcome, { message: 'the snip to be saved' }).toBe('snipped');
  return (await last(yap)).image;
}

/** Chat open on a conversation (its row in the list). */
async function openConversation(main, id) {
  await openView(main, 'Chat');
  await main.locator(`.conv[data-conv="${id}"]`).click();
  await expect(main.locator(`.conv[data-conv="${id}"]`)).toHaveClass(/active/);
}

function imageFile(yap, id) {
  return path.join(yap.dataDir, 'chat-images', `${id}.png`);
}

async function inChat(main) {
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
}

test('a snip from the shortcut lands in a new chat: the screenshot in the box, the caret there, its pixels exact', async ({ yap, main, fakeLlm, shot }) => {
  // On Home: the main window isn't showing a chat, so a new one.
  await startSnip(yap);
  const started = await last(yap);
  expect(started.monitors).toBe(2);
  expect(started.hdrMonitors).toBe(1);
  // The light grey block on the HDR monitor.
  const img = await drag(yap, 0, [40, 40], [240, 140]);
  expect(img).toMatchObject({ w: 200, h: 100, source: 'snip' });
  expect(img.id).toMatch(/^img_[0-9a-f]{16}$/);

  await inChat(main);
  await expect(attachments(main)).toBeVisible();
  await expect(attachments(main)).toContainText('200 × 100');
  await expect(box(main)).toHaveAttribute('placeholder', 'Ask about this screenshot…');
  await expect(box(main)).toBeFocused();
  for (const p of ["What's this?", 'Explain this error', 'Summarise']) {
    await expect(attachments(main).getByRole('button', { name: p })).toBeVisible();
  }
  // Nothing was sent by itself.
  await main.waitForTimeout(300);
  expect(fakeLlm.requests.filter((r) => r.images?.length)).toHaveLength(0);
  // The PNG: exactly the block's grey, after Windows' HDR boost and Yap's
  // conversion back (an 8-bit read would have clipped it to white).
  const png = readPng(fs.readFileSync(imageFile(yap, img.id)));
  expect([png.width, png.height]).toEqual([200, 100]);
  expect(worstError(png, GREY)).toBeLessThanOrEqual(1);
  // The thumbnail draws.
  await expect
    .poll(() => attachments(main).locator('img').first().evaluate((el) => el.complete && el.naturalWidth))
    .toBe(200);
  await shot(main, '01-snip-in-the-box');
});

test('Enter on an empty box asks "What\'s this?" with the screenshot, kept as an id', async ({ yap, main, fakeLlm, shot }) => {
  const before = fakeLlm.requests.length;
  // A screenshot waits in the box (the first test's, kept while Home
  // showed; a fresh one if this test runs alone).
  await openView(main, 'Chat');
  if (!(await attachments(main).isVisible())) {
    await startSnip(yap);
    await drag(yap, 0, [40, 40], [240, 140]);
  }
  await expect(attachments(main)).toBeVisible();
  const waiting = await last(yap);
  await box(main).press('Enter');
  await expect(main.locator('.cbubble.user .shots img')).toBeVisible();
  await expect(main.locator('.cbubble.user').last()).toContainText("What's this?");
  await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
  await expect(attachments(main)).toBeHidden();
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  expect(asked, 'a request with the screenshot').toBeTruthy();
  expect(asked.user).toContain("What's this?");
  expect(asked.images).toHaveLength(1);
  const sent = readDataUrl(asked.images[0]);
  expect([sent.width, sent.height]).toEqual([200, 100]);
  expect(worstError(sent, GREY)).toBeLessThanOrEqual(1);
  // The question as Groq gets it: the image part first, then the text.
  const q = asked.messages.filter((m) => m.role === 'user').at(-1);
  expect(q.content[0].type).toBe('image_url');
  expect(q.content.at(-1).type).toBe('text');
  // chats.json: the id and size, never the pixels.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.images?.[0]?.id === waiting.image.id && m.images[0].w === 200))
  );
  const raw = fs.readFileSync(path.join(yap.dataDir, 'chats.json'), 'utf8');
  expect(raw).not.toContain('base64');
  await shot(main, '02-asked-with-the-screenshot');
});

test('the three prompts ask in one tap; a snip on the 150 % monitor is exact too', async ({ yap, main, fakeLlm }) => {
  await openView(main, 'Home');
  await startSnip(yap);
  // The red block on the second monitor (its window's own pixels).
  const img = await drag(yap, 1, [460, 140], [260, 40]);
  expect(img).toMatchObject({ w: 200, h: 100 });
  expect(worstError(readPng(fs.readFileSync(imageFile(yap, img.id))), RED)).toBeLessThanOrEqual(1);
  await inChat(main);
  const before = fakeLlm.requests.length;
  await attachments(main).getByRole('button', { name: 'Explain this error' }).click();
  await expect(main.locator('.cbubble.user').last()).toContainText('Explain this error');
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length).toBe(1);
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  expect(asked.user).toContain('Explain this error');
  expect(worstError(readDataUrl(asked.images[0]), RED)).toBeLessThanOrEqual(1);
});

test('Esc, right-click and a plain click: nothing is snipped', async ({ yap, main }) => {
  await openView(main, 'Home');
  await startSnip(yap);
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('escape');

  await startSnip(yap);
  await yap.invoke('e2e_snip_input', { screen: 1, events: [{ kind: 'right', x: 10, y: 10 }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('right-click');

  // A click, and a 4-pixel twitch: still selecting, nothing saved.
  await startSnip(yap);
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'down', x: 300, y: 300 }, { kind: 'up', x: 300, y: 300 }] });
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'down', x: 300, y: 300 }, { kind: 'up', x: 304, y: 330 }] });
  await main.waitForTimeout(300);
  expect((await last(yap)).outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  // Home is still showing: nothing came up.
  await expect(sidebar(main).getByRole('button', { name: 'Home', exact: true })).toHaveAttribute('aria-current', 'page');
});

test('the conversation open in Chat gets the snip', async ({ yap, main }) => {
  // The prompts test's conversation, opened.
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title.startsWith('Explain this error'));
  await openConversation(main, conv.id);
  await expect(main.locator('.cbubble.user').first()).toContainText('Explain this error');
  const before = await main.locator('.cbubble').count();
  await startSnip(yap);
  expect((await last(yap)).conversationId).not.toBeNull();
  await drag(yap, 0, [40, 160], [240, 260]);
  await expect(attachments(main)).toBeVisible();
  // Same conversation: its messages are still there.
  await expect(main.locator('.cbubble')).toHaveCount(before);
  await attachments(main).getByRole('button', { name: 'Remove the screenshot' }).click();
  await expect(attachments(main)).toBeHidden();
});

test("a model that can't read images says so, with the switch to one that can", async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
  await openView(main, 'Home');
  await startSnip(yap);
  await drag(yap, 0, [40, 40], [240, 140]);
  await inChat(main);
  await expect(visionLine(main)).toBeVisible();
  await expect(visionLine(main)).toContainText("GPT-OSS 20B can't read images.");
  const use = visionLine(main).getByRole('button', { name: 'Use Qwen3.8 27B on Groq' });
  await expect(use).toBeVisible();
  // Send waits; Enter does nothing; so do the prompts.
  await expect(main.getByRole('button', { name: 'Send', exact: true })).toBeDisabled();
  const before = fakeLlm.requests.length;
  await box(main).press('Enter');
  await main.waitForTimeout(300);
  expect(fakeLlm.requests.length).toBe(before);
  await expect(attachments(main).getByRole('button', { name: "What's this?" })).toBeDisabled();
  await shot(main, '03-cant-read-images');
  // One click: the Chat model reads images, the line goes, and it sends.
  await use.click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes?.chat?.model === 'qwen/qwen3.8-27b' && c.llmScopes.chat.provider === 'groq');
  await expect(visionLine(main)).toBeHidden();
  await box(main).fill('What does it say?');
  await box(main).press('Enter');
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length).toBe(1);
  expect(fakeLlm.requests.slice(before).find((r) => r.images?.length).model).toBe('qwen/qwen3.8-27b');
});

test('a server that refuses the image: the same line, the question kept', async ({ yap, main, fakeLlm }) => {
  // A self-hosted model nothing says about: Chat lets it try.
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'fake-chat', 'custom') } });
  fakeLlm.rejectImages(true);
  try {
    await openView(main, 'Home');
    await startSnip(yap);
    await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    await expect(visionLine(main)).toBeHidden();
    await box(main).fill('Read this');
    await box(main).press('Enter');
    await expect(visionLine(main)).toBeVisible();
    await expect(visionLine(main)).toContainText("can't read images.");
    // The question and the screenshot are back in the box; no error toast.
    await expect(box(main)).toHaveValue('Read this');
    await expect(attachments(main)).toContainText('200 × 100');
    await expect(main.getByRole('status').filter({ hasText: "Couldn't send" })).toHaveCount(0);
    await expect(visionLine(main).getByRole('button', { name: 'Use Qwen3.8 27B on Groq' })).toBeVisible();
    // "Send without the screenshot": the question goes alone.
    const before = fakeLlm.requests.length;
    await visionLine(main).getByRole('button', { name: 'Send without the screenshot' }).click();
    await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
    expect(fakeLlm.requests.slice(before).every((r) => !r.images?.length)).toBe(true);
  } finally {
    fakeLlm.rejectImages(false);
    await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'qwen/qwen3.8-27b') } });
  }
});

test('saying "snip" takes one, as a dictation or a question; "snip the ends off" is dictated', async ({ yap, main }) => {
  await openView(main, 'Home');
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  const said = await yap.invoke('e2e_dictation', { text: 'Take a screenshot.' });
  expect(said.text).toBe('');
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  expect((await last(yap)).outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');

  // To the Voice Assistant: "Snip it."
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  await yap.invoke('e2e_assistant_ask', { text: 'Snip it.', listenMs: 50 });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');

  // Inside a sentence: dictated as said.
  const typed = await yap.invoke('e2e_dictation', { text: 'snip the ends off the beans' });
  expect(typed.text.toLowerCase()).toContain('snip the ends off');
  expect((await last(yap)).outcome).toBe('cancelled');

  // Switched off: "snip" is just a word.
  await setConfig(yap, main, { snipVoice: false });
  const word = await yap.invoke('e2e_dictation', { text: 'Snip.' });
  expect(word.text.toLowerCase()).toContain('snip');
  expect((await last(yap)).outcome).toBe('cancelled');
  await setConfig(yap, main, { snipVoice: true });
});

test('Settings: the snip shortcut, saying "snip", and how long screenshots are kept', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const row = dialog.locator('[data-setting="shortcut-snip"]');
  await expect(row).toContainText('Snip to Chat');
  // Win + Alt + S by default.
  await expect(row.getByRole('button', { name: /Change$/ })).toContainText('S');
  expect((await yap.invoke('get_config')).snipHotkey).toBe('kb:alt+win+83');
  await shot(main, '04-shortcuts-snip');
  await row.getByRole('button', { name: /Change$/ }).click();
  // The dictation hotkey (F24 in the suite): refused.
  await pressKey(main, 'F24', 135);
  await expect(row).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F21', 132);
  await expectStore(yap, 'config.json', (c) => c.snipHotkey === 'kb:132');
  await expect(row.getByRole('button', { name: /^F21\s*Change$/ })).toBeVisible();
  await row.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.snipHotkey === '');

  // Dictation → Typing into apps: the voice switch.
  await openSettings(main, 'Dictation');
  const voice = dialog.locator('[data-setting="snip-voice"]');
  await expect(voice).toContainText('Say “snip” to take a screenshot');
  const sw = voice.getByRole('button', { name: 'Say “snip” to take a screenshot' });
  await expect(sw).toHaveAttribute('aria-pressed', 'true');
  await sw.click();
  await expectStore(yap, 'config.json', (c) => c.snipVoice === false);
  await sw.click();
  await expectStore(yap, 'config.json', (c) => c.snipVoice === true);

  // Privacy & data → Chat screenshots: 7 / 30 / 90 days / Forever.
  await openSettings(main, 'Privacy');
  const keep = dialog.locator('[data-setting="chat-images"]');
  await expect(keep).toContainText('Keep chat screenshots');
  await expect(keep).toContainText(/screenshots? ·/);
  const select = keep.getByRole('combobox', { name: 'Keep chat screenshots' });
  await expect(select).toHaveValue('30');
  await select.selectOption({ label: '90 days' });
  await expectStore(yap, 'config.json', (c) => c.chatImageDays === 90);
  await select.selectOption({ label: 'Forever' });
  await expectStore(yap, 'config.json', (c) => c.chatImageDays === 0);
  await shot(main, '05-keep-chat-screenshots');
  await closeSettings(main);
  await setConfig(yap, main, { snipHotkey: 'kb:alt+win+83', chatImageDays: 30 });
});

test("the bar's ✂, Chat's + and Win+Alt+S in Yap's own window start a snip", async ({ yap, main }) => {
  // ✂ on the Yap bar (bar_action "snip").
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  await yap.invoke('bar_action', { action: 'snip' });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  // The shortcut again cancels.
  await yap.invoke('snip_start', { origin: 'shortcut' });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('the shortcut again');

  // Chat's + → Take a screenshot.
  await openView(main, 'Chat');
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  await main.getByRole('button', { name: 'Chat options' }).click();
  await main.getByRole('button', { name: 'Take a screenshot' }).click();
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');

  // Win+Alt+S while Yap's window has the focus (the in-page fallback: the
  // global hook never sees keys then).
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  const cdp = await main.context().newCDPSession(main);
  try {
    const ev = { key: 's', code: 'KeyS', windowsVirtualKeyCode: 83, nativeVirtualKeyCode: 83, modifiers: 1 | 4 };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
});

test('screenshots are kept their time: removed after 7 days, and with their conversation', async ({ yap, main, shot }) => {
  // Two conversations with a screenshot each.
  for (const question of ['Keep this one?', 'And this one?']) {
    await openView(main, 'Home');
    await startSnip(yap);
    await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    await box(main).fill(question);
    await box(main).press('Enter');
    await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
  }
  const chats = yap.readJson('chats.json');
  const withImages = chats.conversations.filter((c) => c.messages.some((m) => m.images?.length));
  expect(withImages.length).toBeGreaterThan(1);
  const [older, other] = withImages.slice(-2);
  const olderId = older.messages.find((m) => m.images?.length).images[0].id;
  const otherIds = other.messages.flatMap((m) => (m.images ?? []).map((i) => i.id));
  expect(fs.existsSync(imageFile(yap, olderId))).toBe(true);
  // Written 40 days ago; "Keep chat screenshots" set to 7 days: it goes.
  const old = new Date(Date.now() - 40 * 86_400_000);
  fs.utimesSync(imageFile(yap, olderId), old, old);
  await setConfig(yap, main, { chatImageDays: 7 });
  await expect.poll(() => fs.existsSync(imageFile(yap, olderId)), { message: 'the old screenshot to go' }).toBe(false);
  // Its message keeps its place and says why the picture is gone.
  await openConversation(main, older.id);
  await expect(main.getByTestId('chat-shot-gone').first()).toHaveText('Screenshot removed after 7 days');
  await shot(main, '06-screenshot-removed');
  await setConfig(yap, main, { chatImageDays: 30 });

  // Deleting a conversation deletes its screenshots.
  expect(otherIds.length).toBeGreaterThan(0);
  expect(otherIds.every((id) => fs.existsSync(imageFile(yap, id)))).toBe(true);
  await yap.invoke('chat_delete', { id: other.id });
  await expect.poll(() => otherIds.some((id) => fs.existsSync(imageFile(yap, id)))).toBe(false);
});

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
