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
//
// Phase 2 (2026-10-25):
// - A click takes the whole monitor, Enter the one under the pointer; Space
//   (or Alt held) picks the window under the pointer, on either monitor; a
//   box dragged from one monitor into the other is put together from both
//   (black where neither is).
// - Ctrl as you let go (or Ctrl+C) copies, Shift (or Ctrl+S) saves: nothing
//   goes to Chat. The copy lands on a pretend clipboard (the real one is
//   never touched here; the robot tests use it); the save at a path the test
//   names (Windows' dialog never opens).
// - A chat screenshot's right-click menu copies or saves it.
// - A picture pasted into the box (Ctrl+V) and image files dropped on Chat
//   go in as screenshots; a text file is left out.
// - A model that can't read images: "Send its text instead" (the OCR text a
//   test sets in place of Windows', `e2e_snip_ocr`), folded under the
//   question; no text: said.
// - "Reads images" in the Chat model picker and in Settings' Chat list.
//
// The clipboard and the voice (2026-10-25, snip/waiting.rs):
// - A snip to Chat is on the (pretend) clipboard too, the very PNG, "Copied ·
//   Ctrl+V to paste it anywhere" in the box; Settings → Shortcuts' "Copy
//   every snip to the clipboard" off stops it; a pasted picture isn't
//   copied back.
// - Rust keeps the screenshots waiting in the box (`chat_waiting_get`): a
//   question asked out loud (`e2e_assistant_ask`) takes them — the image
//   part and the question to the fake AI, the box cleared, the thumbnail in
//   its bubble; a dictation into the box sends both with Enter; a model that
//   can't read images leaves both in the box with its line; ✕ first: the
//   question goes alone.
// - Saying "snip": once it lands, Yap listens for the question by itself
//   ("Ask about this screenshot…" on the capsule; the question staged with
//   `e2e_snip_question`) and sends both; not during a call, nor after Esc.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { readPng, readDataUrl, worstError, writePng, pixel, near } from './support/png.js';

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
const DARK = [0x20, 0x20, 0x20];
const MID = [0x80, 0x80, 0x80];
const LIGHT = [0xf5, 0xf5, 0xf5];
const BLACK = [0, 0, 0];

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

test('Esc, right-click and a thin twitch: nothing is snipped', async ({ yap, main }) => {
  await openView(main, 'Home');
  await startSnip(yap);
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('escape');

  await startSnip(yap);
  await yap.invoke('e2e_snip_input', { screen: 1, events: [{ kind: 'right', x: 10, y: 10 }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('right-click');

  // A 4-pixel-wide twitch (too thin a box, too long a move for a click):
  // still selecting, nothing saved.
  await startSnip(yap);
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

// ---- Phase 2 -----------------------------------------------------------------------------

/** Post events to a fake monitor's overlay; wait for the snip to be taken. */
async function snipBy(yap, screen, events) {
  await yap.invoke('e2e_snip_input', { screen, events });
  await expect.poll(async () => (await last(yap))?.outcome, { message: 'the snip to be taken' }).toBe('snipped');
  return last(yap);
}

const click = (x, y, extra = {}) => [
  { kind: 'down', x, y },
  { kind: 'up', x, y, ...extra },
];

/** Chat's message box emptied of screenshots (they wait between tests). */
async function clearBox(main) {
  await openView(main, 'Chat');
  const remove = attachments(main).getByRole('button', { name: 'Remove the screenshot' });
  while ((await remove.count()) > 0) await remove.first().click();
  await expect(attachments(main)).toBeHidden();
}

/** The PNG a copy put on the test's pretend clipboard. */
async function clipboardPng(yap) {
  const b64 = await yap.invoke('e2e_snip_clipboard');
  return b64 ? readPng(Buffer.from(b64, 'base64')) : null;
}

test('a click takes the whole monitor, and Enter the one under the pointer', async ({ yap, main, shot }) => {
  await clearBox(main);
  await openView(main, 'Home');
  await startSnip(yap);
  // A click (the mouse barely moving) on the 150 % monitor.
  const one = await snipBy(yap, 1, [
    { kind: 'down', x: 500, y: 300 },
    { kind: 'up', x: 502, y: 301 },
  ]);
  expect(one).toMatchObject({ kind: 'monitor', action: 'chat', rect: { x: 1280, y: 0, w: 960, h: 540 } });
  expect(one.image).toMatchObject({ w: 960, h: 540 });
  const png = readPng(fs.readFileSync(imageFile(yap, one.image.id)));
  expect([png.width, png.height]).toEqual([960, 540]);
  expect(near(pixel(png, 300, 60), RED, 0)).toBe(true);
  expect(near(pixel(png, 60, 60), GREY, 0)).toBe(true);
  expect(near(pixel(png, 5, 5), LIGHT, 0)).toBe(true);
  await inChat(main);
  await expect(attachments(main)).toContainText('960 × 540');
  await shot(main, '07-whole-monitor-in-the-box');

  // Enter, with the pointer over the HDR monitor: all of it, its colours
  // converted back.
  await startSnip(yap);
  const all = await snipBy(yap, 0, [
    { kind: 'hover', x: 600, y: 400 },
    { kind: 'key', key: 'enter' },
  ]);
  expect(all).toMatchObject({ kind: 'monitor', rect: { x: 0, y: 0, w: 1280, h: 720 } });
  const hdr = readPng(fs.readFileSync(imageFile(yap, all.image.id)));
  expect([hdr.width, hdr.height]).toEqual([1280, 720]);
  expect(near(pixel(hdr, 100, 90), GREY)).toBe(true);
  expect(near(pixel(hdr, 700, 600), LIGHT)).toBe(true);
  await clearBox(main);
});

test('Space (or Alt held) picks the window under the pointer, on either monitor', async ({ yap, main }) => {
  // Topmost first: a dialog over a big window on the HDR monitor, and a
  // window on the 150 % one.
  const windows = [
    { x: 250, y: 30, w: 220, h: 120 },
    { x: 20, y: 20, w: 700, h: 400 },
    { x: 1280 + 240, y: 20, w: 300, h: 200 },
  ];
  await openView(main, 'Home');
  await startSnip(yap, { windows });
  expect((await last(yap)).windows).toBe(3);
  const dialog = await snipBy(yap, 0, [{ kind: 'key', key: 'space' }, { kind: 'hover', x: 300, y: 60 }, ...click(300, 60)]);
  expect(dialog).toMatchObject({ kind: 'window', rect: { x: 250, y: 30, w: 220, h: 120 } });
  const png = readPng(fs.readFileSync(imageFile(yap, dialog.image.id)));
  expect([png.width, png.height]).toEqual([220, 120]);
  // Its part of the red block, and the grey around it.
  expect(near(pixel(png, 20, 20), RED)).toBe(true);
  expect(near(pixel(png, 5, 5), LIGHT)).toBe(true);
  await inChat(main);

  // Alt held: the window under the pointer while it's held.
  await openView(main, 'Home');
  await startSnip(yap, { windows });
  const big = await snipBy(yap, 0, [{ kind: 'key', key: 'alt' }, { kind: 'hover', x: 30, y: 300 }, ...click(30, 300)]);
  expect(big).toMatchObject({ kind: 'window', rect: { x: 20, y: 20, w: 700, h: 400 } });

  // On the 150 % monitor: its window, exact.
  await openView(main, 'Home');
  await startSnip(yap, { windows });
  const there = await snipBy(yap, 1, [{ kind: 'key', key: 'space' }, { kind: 'hover', x: 300, y: 100 }, ...click(300, 100)]);
  expect(there).toMatchObject({ kind: 'window', rect: { x: 1520, y: 20, w: 300, h: 200 } });
  const p2 = readPng(fs.readFileSync(imageFile(yap, there.image.id)));
  expect([p2.width, p2.height]).toEqual([300, 200]);
  expect(near(pixel(p2, 40, 40), RED, 0)).toBe(true);

  // Over no window: the monitor.
  await openView(main, 'Home');
  await startSnip(yap, { windows });
  const none = await snipBy(yap, 0, [{ kind: 'key', key: 'space' }, ...click(1200, 700)]);
  expect(none.kind).toBe('monitor');
  await clearBox(main);
});

test('a box dragged from one monitor into the other is put together from both', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  await startSnip(yap);
  // From the HDR monitor at (1100, 150) into the second, to (300, 280) on
  // it: the drag stays with the window it started on, past its edge.
  const across = await snipBy(yap, 0, [
    { kind: 'down', x: 1100, y: 150 },
    { kind: 'move', x: 1400, y: 200 },
    { kind: 'up', x: 1580, y: 280 },
  ]);
  expect(across).toMatchObject({ kind: 'box', action: 'chat', rect: { x: 1100, y: 150, w: 480, h: 130 } });
  const png = readPng(fs.readFileSync(imageFile(yap, across.image.id)));
  expect([png.width, png.height]).toEqual([480, 130]);
  // The first monitor's part (converted from HDR), then the second's two blocks.
  expect(near(pixel(png, 10, 10), LIGHT)).toBe(true);
  expect(near(pixel(png, 180 + 50, 20), DARK, 0)).toBe(true);
  expect(near(pixel(png, 180 + 270, 20), MID, 0)).toBe(true);
  await inChat(main);
  await expect(attachments(main)).toContainText('480 × 130');
  await shot(main, '08-box-across-monitors');

  // Below the shorter monitor is on neither: black.
  await openView(main, 'Home');
  await startSnip(yap);
  const corner = await snipBy(yap, 0, [
    { kind: 'down', x: 1200, y: 500 },
    { kind: 'up', x: 1400, y: 700 },
  ]);
  expect(corner.rect).toEqual({ x: 1200, y: 500, w: 200, h: 200 });
  const c = readPng(fs.readFileSync(imageFile(yap, corner.image.id)));
  expect(near(pixel(c, 150, 100), BLACK, 0)).toBe(true);
  expect(near(pixel(c, 150, 20), LIGHT, 0)).toBe(true);
  await clearBox(main);
});

test('Ctrl as you let go copies, Shift saves — nothing goes to Chat', async ({ yap, main }) => {
  await openView(main, 'Home');
  // Ctrl held as the box is let go: copied, privately (a pretend clipboard here).
  await startSnip(yap);
  const copied = await snipBy(yap, 0, [
    { kind: 'down', x: 40, y: 40 },
    { kind: 'move', x: 140, y: 90, ctrl: true },
    { kind: 'up', x: 240, y: 140, ctrl: true },
  ]);
  expect(copied).toMatchObject({ kind: 'box', action: 'copy', size: [200, 100], image: null });
  const clip = await clipboardPng(yap);
  expect([clip.width, clip.height]).toEqual([200, 100]);
  expect(worstError(clip, GREY)).toBeLessThanOrEqual(1);
  // Ctrl+C while the box is up.
  await startSnip(yap);
  const keyed = await snipBy(yap, 0, [
    { kind: 'down', x: 260, y: 40 },
    { kind: 'move', x: 460, y: 140 },
    { kind: 'key', key: 'ctrl' },
    { kind: 'key', key: 'c' },
  ]);
  expect(keyed).toMatchObject({ action: 'copy', rect: { x: 260, y: 40, w: 200, h: 100 } });
  expect(worstError(await clipboardPng(yap), RED)).toBeLessThanOrEqual(1);

  // Shift: saved where the person chooses (here, a path the test names);
  // the folder is remembered for next time.
  const dir = path.join(yap.dataDir, 'saved-snips');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, 'dark block.png');
  await yap.invoke('e2e_snip_save_path', { path: file });
  await startSnip(yap);
  const saved = await snipBy(yap, 0, [
    { kind: 'down', x: 40, y: 160 },
    { kind: 'up', x: 240, y: 260, shift: true },
  ]);
  expect(saved).toMatchObject({ action: 'save', size: [200, 100] });
  await expect.poll(() => fs.existsSync(file)).toBe(true);
  expect(worstError(readPng(fs.readFileSync(file)), DARK)).toBeLessThanOrEqual(1);
  await expectStore(yap, 'config.json', (c) => c.snipSaveDir === dir);
  // Ctrl+S with no box: the whole monitor under the pointer.
  const whole = path.join(dir, 'second monitor.png');
  await yap.invoke('e2e_snip_save_path', { path: whole });
  await startSnip(yap);
  const second = await snipBy(yap, 1, [{ kind: 'hover', x: 100, y: 100 }, { kind: 'key', key: 'ctrl' }, { kind: 'key', key: 's' }]);
  expect(second).toMatchObject({ kind: 'monitor', action: 'save', size: [960, 540] });
  await expect.poll(() => fs.existsSync(whole)).toBe(true);
  // Nothing came up: Home is still showing.
  await expect(sidebar(main).getByRole('button', { name: 'Home', exact: true })).toHaveAttribute('aria-current', 'page');
});

test("a chat screenshot's right-click menu copies and saves it", async ({ yap, main }) => {
  const conv = yap.readJson('chats.json').conversations.find((c) => c.messages.some((m) => m.images?.length && !m.images[0].asText));
  const id = conv.messages.find((m) => m.images?.length).images[0].id;
  await openConversation(main, conv.id);
  const shotBtn = main.locator('.cbubble.user .shots .shot').first();
  await shotBtn.click({ button: 'right' });
  await expect(main.getByRole('menuitem', { name: 'Copy image' })).toBeVisible();
  await expect(main.getByRole('menuitem', { name: 'Save image as…' })).toBeVisible();
  await main.getByRole('menuitem', { name: 'Copy image' }).click();
  await expect.poll(() => yap.invoke('e2e_snip_clipboard')).toBe(fs.readFileSync(imageFile(yap, id)).toString('base64'));

  const file = path.join(yap.dataDir, 'saved-snips', 'from chat.png');
  await yap.invoke('e2e_snip_save_path', { path: file });
  await shotBtn.click({ button: 'right' });
  await main.getByRole('menuitem', { name: 'Save image as…' }).click();
  await expect.poll(() => fs.existsSync(file)).toBe(true);
  expect(fs.readFileSync(file).equals(fs.readFileSync(imageFile(yap, id)))).toBe(true);
});

test('a picture pasted into the box or image files dropped on Chat go in as screenshots', async ({ yap, main, fakeLlm, shot }) => {
  await clearBox(main);
  await box(main).click();
  // Ctrl+V of a picture copied in another app: WebView2's paste event
  // carries it as a PNG file.
  await box(main).evaluate(async (el) => {
    const c = document.createElement('canvas');
    c.width = 64;
    c.height = 48;
    const g = c.getContext('2d');
    g.fillStyle = '#e8a33c';
    g.fillRect(0, 0, 64, 48);
    const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
    const dt = new DataTransfer();
    dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await expect(attachments(main)).toContainText('64 × 48');
  await expect(box(main)).toHaveValue('');

  // Dropped on Chat: a PNG, a JPEG (Windows' own decoder) and a text file.
  const dir = path.join(yap.dataDir, 'to-drop');
  fs.mkdirSync(dir, { recursive: true });
  const pngFile = path.join(dir, 'chart.png');
  fs.writeFileSync(pngFile, writePng(90, 30, [0x20, 0x60, 0xa0]));
  const jpegB64 = await main.evaluate(() => {
    const c = document.createElement('canvas');
    c.width = 40;
    c.height = 40;
    const g = c.getContext('2d');
    g.fillStyle = '#3cb043';
    g.fillRect(0, 0, 40, 40);
    return c.toDataURL('image/jpeg', 0.95).split(',')[1];
  });
  const jpegFile = path.join(dir, 'photo.jpg');
  fs.writeFileSync(jpegFile, Buffer.from(jpegB64, 'base64'));
  const textFile = path.join(dir, 'notes.txt');
  fs.writeFileSync(textFile, 'not a picture');
  await yap.invoke('e2e_drag_drop', { paths: [pngFile, jpegFile, textFile] });
  await expect(attachments(main).locator('.shotchip')).toHaveCount(3);
  await expect(attachments(main)).toContainText('90 × 30');
  await expect(attachments(main)).toContainText('40 × 40');
  await expect(main.getByRole('status').filter({ hasText: "1 of the files weren't added" })).toBeVisible();
  await shot(main, '09-pasted-and-dropped');

  // They go like a snip's: three image parts (Groq takes three).
  const before = fakeLlm.requests.length;
  await box(main).fill('Compare these');
  await box(main).press('Enter');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  expect(asked.images).toHaveLength(3);
  const sizes = asked.images.map(readDataUrl).map((p) => `${p.width}x${p.height}`);
  expect(sizes.sort()).toEqual(['40x40', '64x48', '90x30']);
  const orange = asked.images.map(readDataUrl).find((p) => p.width === 64);
  expect(worstError(orange, [0xe8, 0xa3, 0x3c])).toBeLessThanOrEqual(1);
  const green = asked.images.map(readDataUrl).find((p) => p.width === 40);
  expect(worstError(green, [0x3c, 0xb0, 0x43])).toBeLessThanOrEqual(6);

  // Ctrl+V with the focus elsewhere in Chat (not a text field): into the box too.
  await expect(attachments(main)).toBeHidden();
  await main.evaluate(async () => {
    const c = document.createElement('canvas');
    c.width = 30;
    c.height = 20;
    c.getContext('2d').fillRect(0, 0, 30, 20);
    const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
    const dt = new DataTransfer();
    dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
    document.activeElement?.blur();
    document.body.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await expect(attachments(main)).toContainText('30 × 20');
  await clearBox(main);

  // Text that brings a picture of itself (Excel's cells, PowerPoint's
  // text): the text pastes, no screenshot. A picture copied in Firefox
  // (its address as the text): still the picture.
  const paste = (text) =>
    box(main).evaluate(async (el, text) => {
      const c = document.createElement('canvas');
      c.width = 50;
      c.height = 10;
      c.getContext('2d').fillRect(0, 0, 50, 10);
      const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
      const dt = new DataTransfer();
      dt.setData('text/plain', text);
      dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
      const ev = new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true });
      el.dispatchEvent(ev);
      return ev.defaultPrevented;
    }, text);
  await box(main).click();
  expect(await paste('Q3 totals\t1,204\r\nQ4 totals\t1,388\r\n')).toBe(false);
  await main.waitForTimeout(500);
  await expect(attachments(main)).toBeHidden();
  expect(await paste('https://example.com/chart.png')).toBe(true);
  await expect(attachments(main)).toContainText('50 × 10');
  await clearBox(main);
});

// Until 2026-10-30 a fifth picture pasted took the oldest's place without a
// word; a drop said the box was full.
test('a picture pasted with four waiting says the box is full, as a drop does, and none is taken out', async ({ yap, main }) => {
  await clearBox(main);
  await box(main).click();
  const pastePicture = (w) =>
    box(main).evaluate(async (el, w) => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = 20;
      c.getContext('2d').fillRect(0, 0, w, 20);
      const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
      const dt = new DataTransfer();
      dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
      el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    }, w);
  for (const [i, w] of [31, 32, 33, 34].entries()) {
    await pastePicture(w);
    await expect(attachments(main).locator('.shotchip')).toHaveCount(i + 1);
  }
  const full = main.getByRole('status').filter({ hasText: 'A question carries 4 screenshots at most' });
  await expect(full).toHaveCount(0);
  await pastePicture(35);
  await expect(full).toBeVisible();
  await expect(full).toContainText('Remove one to add another.');
  await expect(attachments(main).locator('.shotchip')).toHaveCount(4);
  await expect(attachments(main)).toContainText('31 × 20');
  await expect(attachments(main)).not.toContainText('35 × 20');
  // A drop says the same.
  const file = path.join(yap.dataDir, 'to-drop', 'one-more.png');
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, writePng(36, 20, [0x20, 0x60, 0xa0]));
  await yap.invoke('e2e_drag_drop', { paths: [file] });
  await expect(full.first()).toBeVisible();
  await expect(attachments(main).locator('.shotchip')).toHaveCount(4);
  await expect(attachments(main)).not.toContainText('36 × 20');
  await clearBox(main);
});

test("a model that can't read images: \"Send its text instead\", read on this PC", async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
  // The text Windows' OCR would read (its languages differ per PC).
  await yap.invoke('e2e_snip_ocr', { text: "Error 0x80070005: Access is denied.\nCouldn't open settings.json" });
  try {
    await clearBox(main);
    await openView(main, 'Home');
    await startSnip(yap);
    await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    await expect(visionLine(main)).toBeVisible();
    const asText = visionLine(main).getByRole('button', { name: 'Send its text instead' });
    await expect(asText).toBeVisible();
    await shot(main, '10-send-its-text-instead');
    const before = fakeLlm.requests.length;
    await box(main).fill('What does it say?');
    await asText.click();
    await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
    const asked = fakeLlm.requests.slice(before).find((r) => r.user.includes('Access is denied'));
    expect(asked, 'the question with the screenshot text').toBeTruthy();
    expect(asked.images).toHaveLength(0);
    expect(asked.model).toBe('openai/gpt-oss-20b');
    expect(asked.user).toContain('What does it say?');
    expect(asked.user).toContain('[Text read from the screenshot on this PC, sent instead of the picture]');
    expect(asked.user).toContain("Couldn't open settings.json");
    // The bubble: the question, the screenshot marked, its text folded under it.
    const q = main.locator('.cbubble.user').last();
    await expect(q.locator('.cbody')).toHaveText('What does it say?');
    await expect(q.locator('.astextnote')).toHaveText('Sent as text');
    const folded = q.getByTestId('chat-screen-text');
    await expect(folded).toContainText('Text from the screenshot');
    await folded.locator('summary').click();
    await expect(folded.locator('pre')).toContainText('Error 0x80070005: Access is denied.');
    await shot(main, '11-sent-as-text');
    // Kept with the question, marked as sent as text; the conversation
    // named by the question's own words.
    await expectStore(yap, 'chats.json', (s) =>
      s.conversations.some((c) => c.title === 'What does it say?' && c.messages.some((m) => m.images?.[0]?.asText === true))
    );

    // No text in it: said, and the question kept.
    await yap.invoke('e2e_snip_ocr', { text: '' });
    await openView(main, 'Home');
    await startSnip(yap);
    await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    await box(main).fill('And this?');
    await visionLine(main).getByRole('button', { name: 'Send its text instead' }).click();
    await expect(main.getByRole('status').filter({ hasText: 'No text found in the screenshot' })).toBeVisible();
    await expect(box(main)).toHaveValue('And this?');
    await expect(attachments(main)).toContainText('200 × 100');
  } finally {
    await yap.invoke('e2e_snip_ocr', { text: null });
    await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'qwen/qwen3.8-27b') } });
    await clearBox(main);
  }
});

test('"Reads images" in the Chat model picker and in Settings\' Chat models', async ({ main, shot }) => {
  await openView(main, 'Chat');
  await main.locator('.modelchip .chip').click();
  const pop = main.getByRole('dialog', { name: 'Switch the Chat model' });
  await expect(pop).toBeVisible();
  await pop.getByRole('tab', { name: /^Groq/ }).click();
  const row = (name) => pop.locator('.pane .mrow', { has: main.locator('.nm', { hasText: new RegExp(`^${name}$`) }) });
  await expect(row('Qwen3.8 27B').locator('.tag.img')).toHaveText('Reads images');
  await expect(row('GPT-OSS 20B').locator('.tag.img')).toHaveCount(0);
  // Every Claude reads images, connected or not.
  await pop.getByRole('tab', { name: /^Claude/ }).click();
  await expect(row('Claude Haiku 4.5').locator('.tag.img')).toHaveText('Reads images');
  await expect(row('Claude Haiku 4.5').locator('.tag.key')).toHaveText('Add key');
  await shot(main, '12-picker-reads-images');
  // Search "images": the models that read them.
  await pop.getByRole('searchbox').fill('reads images');
  await expect(pop.locator('.mrow').first()).toBeVisible();
  const names = await pop.locator('.mrow .nm').allTextContents();
  expect(names).toContain('Qwen3.8 27B');
  expect(names).not.toContain('GPT-OSS 20B');
  await main.keyboard.press('Escape');
  await expect(pop).toBeHidden();

  // Settings → Language Models → Chat & Voice Assistant: the same tag.
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  const list = dialog.locator('.picker[data-provider="groq"]');
  await expect(list.locator('.srow', { hasText: 'Qwen3.8 27B' }).locator('.simg')).toHaveText('Reads images');
  await expect(list.locator('.srow', { hasText: 'GPT-OSS 20B' }).locator('.simg')).toHaveCount(0);
  // Dictation Cleanup's list doesn't say it (a screenshot never goes there).
  await dialog.getByRole('tab', { name: 'Dictation Cleanup' }).click();
  await expect(dialog.locator('.simg')).toHaveCount(0);
  await closeSettings(main);
});

// ---- The clipboard and the voice (2026-10-25, snip/waiting.rs) ----
// The maintainer: "When you take a screenshot it should go into Ctrl+V";
// "If I press the AI Assistant after saying snip, it doesn't send what I
// said and the snip; just one or the other".

/** The dictation capsule on the Yap bar (its assistant variant has `data-ask`). */
const capsule = (yap) => yap.overlay.locator('.capsule');
const waitingIds = async (yap) => (await yap.invoke('chat_waiting_get')).images.map((i) => i.id);
const pngB64 = (yap, id) => fs.readFileSync(imageFile(yap, id)).toString('base64');

test('a snip to Chat is on the clipboard too; the setting off stops it; a pasted picture is not copied back', async ({ yap, main, shot }) => {
  await clearBox(main);
  await openView(main, 'Home');
  await yap.invoke('e2e_snip_clipboard', { clear: true });
  await startSnip(yap);
  const img = await drag(yap, 0, [260, 40], [460, 140]);
  await inChat(main);
  // The very PNG Chat has (a pretend clipboard here; the robot test reads the real one).
  await expect.poll(() => yap.invoke('e2e_snip_clipboard')).toBe(pngB64(yap, img.id));
  await expect.poll(async () => (await last(yap)).copied).toBe(true);
  await expect(main.getByTestId('chat-snip-copied')).toHaveText('Copied · Ctrl+V to paste it anywhere');
  await shot(main, '13-snip-copied');

  // Settings → Shortcuts: "Copy every snip to the clipboard", on by default.
  const dialog = await openSettings(main, 'Shortcuts');
  const row = dialog.locator('[data-setting="snip-copy"]');
  const sw = row.getByRole('button', { name: 'Copy every snip to the clipboard' });
  await expect(sw).toHaveAttribute('aria-pressed', 'true');
  await sw.click();
  await expectStore(yap, 'config.json', (c) => c.snipCopy === false);
  await closeSettings(main);
  await clearBox(main);
  await yap.invoke('e2e_snip_clipboard', { clear: true });
  await openView(main, 'Home');
  await startSnip(yap);
  await drag(yap, 0, [40, 40], [240, 140]);
  await inChat(main);
  await expect(attachments(main)).toBeVisible();
  await main.waitForTimeout(500);
  expect(await yap.invoke('e2e_snip_clipboard')).toBeNull();
  expect((await last(yap)).copied).toBe(false);
  await expect(main.getByTestId('chat-snip-copied')).toHaveCount(0);
  await setConfig(yap, main, { snipCopy: true });

  // A picture pasted into the box came from the clipboard: not copied back.
  await clearBox(main);
  await box(main).click();
  await box(main).evaluate(async (el) => {
    const c = document.createElement('canvas');
    c.width = 32;
    c.height = 24;
    c.getContext('2d').fillRect(0, 0, 32, 24);
    const blob = await new Promise((r) => c.toBlob(r, 'image/png'));
    const dt = new DataTransfer();
    dt.items.add(new File([blob], 'image.png', { type: 'image/png' }));
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await expect(attachments(main)).toContainText('32 × 24');
  // …and it waits in Rust's box too.
  await expect.poll(async () => (await waitingIds(yap)).length).toBe(1);
  await main.waitForTimeout(300);
  expect(await yap.invoke('e2e_snip_clipboard')).toBeNull();
  await clearBox(main);
  expect(await waitingIds(yap)).toEqual([]);
});

test('a screenshot waiting in the box goes with a question asked out loud; a dictation into the box sends both too', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await clearBox(main);
  await openView(main, 'Home');
  await startSnip(yap);
  const img = await drag(yap, 0, [260, 40], [460, 140]);
  await inChat(main);
  await expect(attachments(main)).toBeVisible();
  // Rust holds it as the page does.
  expect(await waitingIds(yap)).toEqual([img.id]);

  // The Voice Assistant's shortcut, the question said (staged: no microphone).
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'A red block.' }]);
  await yap.invoke('e2e_assistant_ask', { text: 'What colour is this block?', listenMs: 50 });
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length, { timeout: 10_000 }).toBe(1);
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  expect(asked.user).toContain('What colour is this block?');
  expect(asked.images).toHaveLength(1);
  expect(worstError(readDataUrl(asked.images[0]), RED)).toBeLessThanOrEqual(1);
  // The question as Groq gets it: the image part, then the text.
  const q = asked.messages.filter((m) => m.role === 'user').at(-1);
  expect(q.content[0].type).toBe('image_url');
  // The box cleared (in Rust and here); the question in Chat with its thumbnail.
  await expect(attachments(main)).toBeHidden();
  expect(await waitingIds(yap)).toEqual([]);
  const bubble = main.locator('.cbubble.user', { hasText: 'What colour is this block?' });
  await expect(bubble.locator('.shots img')).toBeVisible();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('A red block.');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.text === 'What colour is this block?' && m.images?.[0]?.id === img.id))
  );
  await shot(main, '14-asked-out-loud-with-the-screenshot');

  // A dictation into Chat's box (its mic, or the hotkey there): the words
  // go in the box beside the screenshot, and Enter sends both.
  await startSnip(yap);
  const dark = await drag(yap, 0, [40, 160], [240, 260]);
  await inChat(main);
  await yap.invoke('e2e_chat_dictation', { text: 'What is the dark block?' });
  await expect(box(main)).toHaveValue(/What is the dark block\?/);
  await expect(attachments(main)).toContainText('200 × 100');
  const typed = fakeLlm.requests.length;
  await box(main).press('Enter');
  await expect.poll(() => fakeLlm.requests.slice(typed).filter((r) => r.images?.length).length).toBe(1);
  const both = fakeLlm.requests.slice(typed).find((r) => r.images?.length);
  expect(both.user).toContain('What is the dark block?');
  expect(worstError(readDataUrl(both.images[0]), DARK)).toBeLessThanOrEqual(1);
  expect(await waitingIds(yap)).not.toContain(dark.id);
});

test("asked out loud with a screenshot, to a model that can't read images: nothing goes, the question waits in the box with it", async ({
  yap,
  main,
  fakeLlm,
}) => {
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
  try {
    await clearBox(main);
    await openView(main, 'Home');
    await startSnip(yap);
    const img = await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    const before = fakeLlm.requests.length;
    await yap.invoke('e2e_assistant_ask', { text: 'What does it say?', listenMs: 50 });
    // As a typed question: the line with the switch, the question and the
    // screenshot in the box, nothing sent.
    await expect(box(main)).toHaveValue(/What does it say\?/);
    await expect(visionLine(main)).toContainText("GPT-OSS 20B can't read images.");
    await expect(attachments(main)).toContainText('200 × 100');
    await main.waitForTimeout(300);
    expect(fakeLlm.requests.length).toBe(before);
    expect(await waitingIds(yap)).toEqual([img.id]);
  } finally {
    await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'qwen/qwen3.8-27b') } });
  }
  await box(main).fill('');
  await clearBox(main);
});

test('✕ takes the screenshot out of the box: a question asked out loud then goes alone', async ({ yap, main, fakeLlm }) => {
  await clearBox(main);
  await openView(main, 'Home');
  await startSnip(yap);
  await drag(yap, 0, [40, 40], [240, 140]);
  await inChat(main);
  await attachments(main).getByRole('button', { name: 'Remove the screenshot' }).click();
  await expect(attachments(main)).toBeHidden();
  await expect.poll(() => waitingIds(yap)).toEqual([]);
  const before = fakeLlm.requests.length;
  await yap.invoke('e2e_assistant_ask', { text: 'Anything on today?', listenMs: 50 });
  await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.user?.includes('Anything on today?')), { timeout: 10_000 }).toBe(true);
  expect(fakeLlm.requests.slice(before).every((r) => !r.images?.length)).toBe(true);
});

test('saying "snip": the screenshot lands in Chat, Yap listens for the question by itself, and both go', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await clearBox(main);
  await openView(main, 'Home');
  await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
  // What the question will say (in the microphone's place), heard for 1.5 s.
  await yap.invoke('e2e_snip_question', { text: 'Which colour is this block?', listenMs: 1500 });
  const said = await yap.invoke('e2e_dictation', { text: 'Snip.' });
  expect(said.text).toBe('');
  await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'It is red.' }]);
  await drag(yap, 0, [260, 40], [460, 140]);
  await inChat(main);
  // Listening by itself: the capsule asks about the screenshot.
  await expect.poll(async () => (await last(yap)).listened).toBe(true);
  const cap = capsule(yap);
  await expect(cap).toHaveAttribute('data-ask', 'listening');
  await expect(cap).toContainText('Ask about this screenshot…');
  await shot(yap.overlay, '15-ask-about-this-screenshot');
  // The question goes with the screenshot.
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length, { timeout: 10_000 }).toBe(1);
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  expect(asked.user).toContain('Which colour is this block?');
  expect(worstError(readDataUrl(asked.images[0]), RED)).toBeLessThanOrEqual(1);
  await expect(attachments(main)).toBeHidden();
  await expect(main.locator('.cbubble.user', { hasText: 'Which colour is this block?' }).locator('.shots img')).toBeVisible();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('It is red.');
});

test('saying "snip" during a call, or cancelling the snip: nothing listens, and the screenshot waits', async ({ yap, main, fakeLlm }) => {
  await clearBox(main);
  await yap.invoke('e2e_snip_question', { text: 'What is this?', listenMs: 50 });
  try {
    // Esc in the overlay: no snip, nothing listens.
    await openView(main, 'Home');
    await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
    await yap.invoke('e2e_dictation', { text: 'Take a snip.' });
    await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
    const before = fakeLlm.requests.length;
    await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
    await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
    await main.waitForTimeout(500);
    expect((await last(yap)).listened).toBe(false);
    await expect(capsule(yap)).toHaveCount(0);

    // A call is on: the Voice Assistant is off, so the snip just waits in the box.
    await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: true });
    await yap.invoke('e2e_snip', { monitors: MONITORS, start: false });
    await yap.invoke('e2e_dictation', { text: 'Snip it.' });
    await expect.poll(async () => (await last(yap))?.outcome).toBe('selecting');
    const img = await drag(yap, 0, [40, 40], [240, 140]);
    await inChat(main);
    await expect(attachments(main)).toBeVisible();
    await main.waitForTimeout(600);
    expect((await last(yap)).listened).toBe(false);
    await expect(capsule(yap)).toHaveCount(0);
    expect(fakeLlm.requests.length).toBe(before);
    expect(await waitingIds(yap)).toEqual([img.id]);
  } finally {
    await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: false });
    await yap.invoke('e2e_snip_question', { text: null });
  }
  await clearBox(main);
});

test('Privacy & data: where chat screenshots are kept, Open folder, and Delete all', async ({ yap, main, shot }) => {
  // One kept screenshot (its file is all Settings counts).
  const dir = path.join(yap.dataDir, 'chat-images');
  fs.mkdirSync(dir, { recursive: true });
  const id = 'img_00000000000e2e01';
  fs.writeFileSync(path.join(dir, `${id}.png`), Buffer.from('not really a png'));
  const dialog = await openSettings(main, 'Privacy');
  // Yap's own folder, never Pictures.
  const where = dialog.locator('[data-setting="chat-images-folder"]');
  await expect(where).toContainText("Where they're kept");
  await expect(where).toContainText('chat-images');
  const before = fs.readFileSync(yap.logPath, 'utf8').length;
  await where.getByRole('button', { name: 'Open folder' }).click();
  await expect.poll(() => fs.readFileSync(yap.logPath, 'utf8').slice(before)).toContain('e2e: not opening the chat screenshots folder');
  // Delete all…: asked first, then the folder empty and the count says so.
  const del = dialog.locator('[data-setting="chat-images-delete"]');
  await expect(del).toContainText(/screenshots? ·/);
  await where.scrollIntoViewIfNeeded();
  await shot(main, '06-chat-screenshots-folder');
  await del.getByRole('button', { name: 'Delete all…' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete all chat screenshots?' });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Delete screenshots' }).click();
  await expect(del).toContainText('None kept right now');
  expect(fs.existsSync(path.join(dir, `${id}.png`))).toBe(false);
  await expect(del.getByRole('button', { name: 'Delete all…' })).toBeDisabled();
  await closeSettings(main);
});

test('Snip to Chat on Print Screen: the recorder takes its release alone, and "Use Print Screen" sets it', async ({ yap, main }) => {
  let dialog = await openSettings(main, 'Shortcuts');
  const row = () => dialog.locator('[data-setting="shortcut-snip"]');
  // Windows gives the page only Print Screen's release, never its press.
  await row().getByRole('button', { name: /Change$/ }).first().click();
  const cdp = await main.context().newCDPSession(main);
  try {
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'PrintScreen', code: 'PrintScreen', windowsVirtualKeyCode: 44, nativeVirtualKeyCode: 44 });
  } finally {
    await cdp.detach().catch(() => {});
  }
  await expectStore(yap, 'config.json', (c) => c.snipHotkey === 'kb:44');
  await expect(row()).toContainText('Print Screen');
  await expect(dialog.getByRole('button', { name: 'Use Print Screen' })).toHaveCount(0);
  await closeSettings(main);
  // Back to Win+Alt+S: one click puts Print Screen there.
  await setConfig(yap, main, { snipHotkey: 'kb:alt+win+83' });
  dialog = await openSettings(main, 'Shortcuts');
  await dialog.getByRole('button', { name: 'Use Print Screen' }).click();
  await expectStore(yap, 'config.json', (c) => c.snipHotkey === 'kb:44');
  await closeSettings(main);
  await setConfig(yap, main, { snipHotkey: 'kb:alt+win+83' });
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
