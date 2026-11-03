// Yap Link, home preview (src-tauri/src/link.rs, docs/architecture/link.md):
// a phone talks to Yap's AI over the home Wi-Fi. Here the "phone" is a
// headless Chromium (Playwright's own if installed, else Edge) with a fake
// microphone playing a tone, opening the real page Yap serves
// (src-tauri/link-page/) over Yap's own HTTPS server — its self-signed
// certificate accepted as a phone's "Proceed" does (ignoreHTTPSErrors).
// - Settings → Phone: the switch, Pair a phone (the QR code, the link with
//   Copy, the one-time warning said), the paired phones with Remove.
// - Pairing: the QR's link opened on the phone, "Allow ‘…’ to use Yap?" in
//   the main window with the code the phone shows too, Allow.
// - A question: Talk on the phone with hands-free on (the fake mic's audio
//   up), the stub's transcript heard, the fake model's answer streamed to
//   the phone, Yap's voice as audio down (test mode's stand-in voice:
//   silence of the right length) — never this PC's speakers — and the
//   conversation kept in Chat.
// - Talk's words in the box (hands-free off, the default): Done puts the
//   stub's transcript in the phone's text box, nothing in Chat; fixed by
//   hand, Send asks it as the spoken question it was (`ask {speak}`): by
//   voice in Chat, the spoken answer's note to the model, said on the phone.
// - A typed question (the composer's `ask`): the fake model's answer in the
//   same conversation, typed (not "by voice"), nothing said; ▶ Read aloud
//   then says it on the phone. ☰'s drawer with that conversation marked;
//   + New chat and a question in a conversation of its own; a picture from
//   the phone that GPT-OSS can't read, sent as its text (test mode's OCR)
//   and kept with the question; the first conversation opened again. Chat's
//   picker over the wire: the rail, Latest, a switch that switches Chat's
//   model on this PC.
// - Remove cuts the phone off at once; a replayed pairing link, a wrong
//   secret and another site's page are refused.
// - A phone's tabs (3 Nov: Firefox restored a phone's tabs, each with an old
//   `#pair=` code, and their refusals locked the phone out): restored tabs
//   with old codes connect on the device; the code scanned again on a paired
//   phone is spent and Settings says it's paired already; a new phone's tabs
//   with old codes are refused, five lock its address out ("Try again in 10
//   minutes."), yet the paired phone still connects, the live code still
//   pairs, and the refused tabs come back with the new device.
//
// The server listens on a free port in a test run (YAP_E2E_LINK_PORT=0;
// `link_status` says which); the phone connects to 127.0.0.1 (a loopback
// peer is allowed, and no real network is involved).
import fs from 'node:fs';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const TEST_KEY = 'e2e-fake-key';
const ANSWER = 'Boil it for six minutes, then cool it in cold water.';
const TYPED = 'How long should the bread dough rest?';
const TYPED_ANSWER = 'Rest the dough for about an hour, until it has doubled.';
const NEW_Q = 'What time does the station café open?';
const NEW_ANSWER = 'The station café opens at 7 am on weekdays.';
const SIGN_Q = 'What does this sign say?';
const SIGN_TEXT = 'Platform 4: the 10:42 to the coast is delayed.';
const SIGN_ANSWER = 'It says the 10:42 to the coast, from platform 4, is delayed.';
const WORDS_ANSWER = 'That came through clearly: a short test from the phone.';

/** Every message in Chat on this PC (chats.json). */
const messages = (store) => store.conversations.reduce((n, c) => n + c.messages.length, 0);

/** The phone's hands-free switch (⋯ → Hands-free), turned to `on`. */
async function handsFree(page, on) {
  await page.getByTestId('settings').click();
  const sw = page.getByTestId('handsfree');
  if ((await sw.isChecked()) !== on) await page.getByTestId('settings-sheet').getByText('Hands-free').click();
  await expect(sw).toBeChecked({ checked: on });
  await page.getByTestId('settings-sheet').getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('settings-sheet')).toBeHidden();
}

/** A made-up photo drawn in the phone's page, as a file to choose. */
async function photo(page) {
  const bytes = await page.evaluate(async () => {
    const c = new OffscreenCanvas(1600, 1200);
    const g = c.getContext('2d');
    g.fillStyle = '#2c5d8f';
    g.fillRect(0, 0, 1600, 1200);
    g.fillStyle = '#ffffff';
    g.fillRect(200, 400, 1200, 400);
    const blob = await c.convertToBlob({ type: 'image/png' });
    return [...new Uint8Array(await blob.arrayBuffer())];
  });
  return { name: 'sign.png', mimeType: 'image/png', buffer: Buffer.from(bytes) };
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
        name: 'link',
        env: { YAP_E2E_LINK_PORT: '0' },
        config: {
          linkEnabled: true,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: {
            chat: {
              enabled: true,
              provider: 'groq',
              baseUrl: fakeLlm.base,
              model: 'openai/gpt-oss-20b',
              apiKey: TEST_KEY,
              apiKeys: {},
              prompt: '',
              disableThinking: false,
            },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
  // The phone: one headless browser for the spec, a fake mic playing a tone.
  phone: [
    async ({}, use) => {
      fs.mkdirSync(RUNS, { recursive: true });
      const wav = writeWav(path.join(RUNS, 'link-phone-mic.wav'), { seconds: 3, rate: 48_000, amplitude: 6000 });
      let channel = process.env.YAP_LINK_CHANNEL;
      if (!channel) {
        try {
          channel = fs.existsSync(chromium.executablePath()) ? 'chromium' : 'msedge';
        } catch {
          channel = 'msedge';
        }
      }
      const browser = await chromium.launch({
        channel,
        headless: true,
        args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${wav}`],
      });
      await use(browser);
      await browser.close();
    },
    { scope: 'worker' },
  ],
});

/** A phone-sized page that trusts Yap's self-signed certificate, as a phone does after "Proceed". */
async function phonePage(phone) {
  const context = await phone.newContext({
    ignoreHTTPSErrors: true,
    viewport: { width: 390, height: 844 },
    permissions: ['microphone'],
  });
  return context.newPage();
}

const snapshot = (page) => page.evaluate(() => window.__yapLink.snapshot());
const status = (yap) => yap.invoke('link_status');

/** The pairing link Settings shows, pointed at 127.0.0.1 (the test's phone is on this PC). */
async function pairingLink(yap, dialog) {
  await dialog.getByRole('button', { name: 'Pair a phone' }).click();
  await expect(dialog.getByRole('img', { name: "QR code that opens Yap's page on your phone" })).toBeVisible();
  const url = (await dialog.getByLabel('Pairing link').textContent()).trim();
  expect(url).toMatch(/^https:\/\/[^/]+:\d+\/#pair=[A-Za-z0-9_-]{22}$/);
  const { port } = await status(yap);
  return { url, local: url.replace(/^https:\/\/[^/]+/, `https://127.0.0.1:${port}`) };
}

test.describe.configure({ mode: 'serial' });

test('Settings → Phone: the switch, a QR code to pair with, and the warning said once', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Phone');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Phone' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Use Yap from your phone (on your home Wi-Fi)' })).toBeVisible();
  await expect.poll(async () => (await status(yap)).running).toBe(true);
  await expect(dialog.getByText('No phones paired yet.')).toBeVisible();
  const { url } = await pairingLink(yap, dialog);
  await expect(dialog.getByText("Chrome warns once that the connection isn't private")).toBeVisible();
  await expect(dialog.getByText(/Good once, for \d+ min\./)).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Copy link' })).toBeVisible();
  expect(url).toContain(`:${(await status(yap)).port}/`);
  await shot(main, '01-settings-phone-pair');
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(dialog.getByRole('button', { name: 'Pair a phone' })).toBeVisible();
  await closeSettings(main);
});

test('a phone pairs with the code both screens show, asks out loud and hears the answer', async ({ yap, main, phone, fakeLlm, shot }) => {
  test.setTimeout(120_000);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  const dialog = await openSettings(main, 'Phone');
  const { local } = await pairingLink(yap, dialog);
  const page = await phonePage(phone);
  const response = await page.goto(local);
  // Yap's own page, with a strict policy.
  expect(response.headers()['content-security-policy']).toContain("script-src 'self'");
  expect(response.headers()['content-type']).toBe('text/html; charset=utf-8');

  // The main window asks, with the code the phone shows.
  const ask = main.getByRole('alertdialog');
  await expect(ask).toContainText(/Allow ‘.+’ to use Yap\?/);
  const code = (await ask.locator('.code b').textContent()).trim();
  expect(code).toMatch(/^\d{4}$/);
  await expect(page.getByTestId('pair-code')).toHaveText(code);
  await shot(main, '02-allow-the-phone');
  await ask.getByRole('button', { name: 'Allow' }).click();
  await expect(ask).toBeHidden();
  await expect.poll(async () => (await snapshot(page)).ready, { timeout: 15_000 }).toBe(true);
  // The phone keeps its id and secret; Yap only the secret's hash.
  const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.device')));
  expect(kept.id).toMatch(/^p-/);
  await expectStore(yap, 'link_devices.json', (store) => {
    expect(store.devices).toHaveLength(1);
    expect(store.devices[0].id).toBe(kept.id);
    expect(store.devices[0].secretHash).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.stringify(store)).not.toContain(kept.secret);
    return true;
  });
  // Settings lists it, connected.
  const list = dialog.getByRole('list', { name: 'Paired phones' });
  await expect(list).toContainText('connected now');
  await shot(main, '03-settings-phone-paired');
  // Chat's model is the phone's to see.
  expect((await snapshot(page)).model?.id).toBe('groq|openai/gpt-oss-20b');

  // A question out loud, answered out loud: hands-free on, Talk, a second
  // of the fake mic, Talk again (Done) to send.
  await handsFree(page, true);
  fakeLlm.scriptChat([{ plain: true, content: ANSWER }]);
  const talk = page.getByTestId('talk');
  await talk.click();
  await expect.poll(async () => (await snapshot(page)).frames, { timeout: 10_000 }).toBeGreaterThan(20);
  await page.waitForTimeout(700);
  // (Hands-free may have ended it by itself on the tone's quiet: then it's sent already.)
  if ((await snapshot(page)).local === 'listening') await talk.click();
  // The stub's transcript heard, the answer shown on the phone. (In the
  // question's bubble: the chat's title under the header says it too, once
  // the PC's `chat` comes.)
  await expect(page.getByTestId('heard').filter({ hasText: /STT stub: received/ })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText(ANSWER)).toBeVisible({ timeout: 20_000 });
  // Yap's voice came down to the phone (at 24 kHz) and played.
  await expect.poll(async () => (await snapshot(page)).player.rate, { timeout: 20_000 }).toBe(24_000);
  await shot(page, '04-phone-answered');
  // Said by Yap's speaker (test mode records it), streamed, never on this PC.
  await expect
    .poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join(' '), { timeout: 20_000 })
    .toContain('six minutes');
  // Back to ready when the voice is done.
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 30_000 }).toBe('idle');
  // Kept in Chat, as the Voice Assistant's questions are.
  await expectStore(yap, 'chats.json', (store) => {
    const text = JSON.stringify(store);
    expect(text).toContain('STT stub: received');
    expect(text).toContain(ANSWER);
    return true;
  });
  // Its network receipts: the page's files and the conversation, inbound.
  await expect
    .poll(async () => (await yap.invoke('connections_summary')).groups.some((g) => g.label === 'Yap Link · your phone'), { timeout: 15_000 })
    .toBe(true);

  // Talk's words in the box (hands-free off, the default): Done puts the
  // stub's transcript in the phone's text box to check — nothing asked,
  // nothing in Chat — and Send asks it as a typed question, not said.
  await handsFree(page, false);
  const said = messages(yap.readJson('chats.json'));
  const turns = (await snapshot(page)).turns.length;
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await talk.click();
  await expect.poll(async () => (await snapshot(page)).frames, { timeout: 10_000 }).toBeGreaterThan(20);
  await page.waitForTimeout(700);
  await talk.click();
  const box = page.getByTestId('input');
  await expect(box).toHaveValue(/STT stub: received/, { timeout: 20_000 });
  await expect(page.getByTestId('send')).toBeEnabled();
  await shot(page, '04a-phone-words-in-the-box');
  expect((await snapshot(page)).turns).toHaveLength(turns);
  await page.waitForTimeout(500);
  expect(messages(yap.readJson('chats.json'))).toBe(said);
  // Checked and fixed by hand, then Send: asked as the spoken question it
  // was — the Voice Assistant's spoken answer, said on the phone.
  await box.press('End');
  await box.pressSequentially(' please');
  const words = await box.inputValue();
  fakeLlm.scriptChat([{ plain: true, content: WORDS_ANSWER }]);
  const asked = fakeLlm.requests.length;
  await page.getByTestId('send').click();
  await expect(box).toHaveValue('');
  await expect(page.getByRole('img', { name: 'Asked by voice' }).last()).toBeVisible();
  await expect(page.getByText(WORDS_ANSWER)).toBeVisible({ timeout: 20_000 });
  await expect
    .poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join(' '), { timeout: 20_000 })
    .toContain('came through clearly');
  await expect.poll(async () => (await snapshot(page)).player.rate, { timeout: 20_000 }).toBe(24_000);
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 30_000 }).toBe('idle');
  // The model was told it was asked out loud (the spoken answer's note, in
  // the question's context block).
  const SPOKEN_NOTE = 'This question was asked out loud';
  const prompt = fakeLlm.requests.slice(asked).find((r) => JSON.stringify(r.messages).includes(words.trim()));
  expect(prompt, 'the question went to the model').toBeTruthy();
  expect(JSON.stringify(prompt.messages)).toContain(SPOKEN_NOTE);
  await expectStore(yap, 'chats.json', (store) => {
    // Asked by voice: the words as the box sent them.
    const q = store.conversations.flatMap((c) => c.messages).find((m) => m.role === 'user' && m.text.trim() === words.trim());
    expect(q, 'the words, asked').toBeTruthy();
    expect(q.voice).toBe(true);
    expect(messages(store)).toBe(said + 2);
    return true;
  });

  // A typed question (the composer): answered in the same conversation, as
  // Chat answers a typed one, and not said — not on the phone, not here.
  await yap.invoke('tts_e2e_spoken', { clear: true });
  fakeLlm.scriptChat([{ plain: true, content: TYPED_ANSWER }]);
  const typedAt = fakeLlm.requests.length;
  await page.getByTestId('input').fill(TYPED);
  await page.getByTestId('input').press('Enter');
  await expect(page.getByText(TYPED_ANSWER)).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await snapshot(page)).turns.at(-1)?.answerId, { timeout: 20_000 }).toMatch(/^chat:\d+:\d+$/);
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 15_000 }).toBe('idle');
  await page.waitForTimeout(1000);
  expect(await yap.invoke('tts_e2e_spoken')).toEqual([]);
  expect((await snapshot(page)).player.active).toBe(false);
  // Typed: no spoken answer's note for the model.
  const typedPrompt = fakeLlm.requests.slice(typedAt).find((r) => JSON.stringify(r.messages.at(-1)).includes(TYPED));
  expect(typedPrompt, 'the typed question went to the model').toBeTruthy();
  expect(JSON.stringify(typedPrompt.messages.at(-1))).not.toContain(SPOKEN_NOTE);
  await expectStore(yap, 'chats.json', (store) => {
    const conv = store.conversations.find((c) => c.messages.some((m) => m.text === TYPED));
    expect(conv, 'the typed question is in Chat').toBeTruthy();
    // The same conversation as the spoken question; typed, not "by voice".
    expect(conv.messages.some((m) => m.role === 'user' && m.voice && m.text.includes('STT stub: received'))).toBe(true);
    expect(conv.messages.find((m) => m.text === TYPED).voice).toBeFalsy();
    expect(conv.messages.some((m) => m.role === 'assistant' && m.text === TYPED_ANSWER)).toBe(true);
    return true;
  });
  await shot(page, '04b-phone-typed');
  // ▶ Read aloud under it: said by Yap's speaker, on the phone.
  await page.getByTestId('read-aloud').last().click();
  await expect
    .poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join(' '), { timeout: 20_000 })
    .toContain('Rest the dough');
  await expect.poll(async () => (await snapshot(page)).reading, { timeout: 30_000 }).toBe('');
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 30_000 }).toBe('idle');

  // The phone keeps the conversation its first question went to (`chat`),
  // and ☰ lists Chat's conversations on this PC with that one marked.
  const first = (await snapshot(page)).chat;
  expect(first).toEqual(expect.any(Number));
  await page.getByTestId('menu').click();
  await expect(page.getByTestId('drawer')).toBeVisible();
  await expect(page.getByTestId('drawer').locator('[aria-current="true"]')).toContainText('STT stub: received');
  await shot(page, '04d-phone-drawer');
  // + New chat: the next question starts a conversation of its own.
  await page.getByTestId('new-chat').click();
  await expect(page.getByTestId('turn')).toHaveCount(0);
  fakeLlm.scriptChat([{ plain: true, content: NEW_ANSWER }]);
  await page.getByTestId('input').fill(NEW_Q);
  await page.getByTestId('input').press('Enter');
  await expect(page.getByText(NEW_ANSWER)).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await snapshot(page)).chat, { timeout: 10_000 }).not.toBe('new');
  const second = (await snapshot(page)).chat;
  expect(second).not.toBe(first);
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 15_000 }).toBe('idle');

  // A picture from the phone: Chat's model here (GPT-OSS) can't read
  // images, so the composer says so, as Chat's does; "Send its text
  // instead" reads it on this PC (test mode's stand-in for Windows' OCR).
  await yap.invoke('e2e_snip_ocr', { text: SIGN_TEXT });
  const sign = await photo(page);
  await page.getByTestId('plus').click();
  const chooser = page.waitForEvent('filechooser');
  await page.getByTestId('choose-photos').click();
  await (await chooser).setFiles(sign);
  await expect(page.getByTestId('shot')).toHaveCount(1);
  await page.getByTestId('input').fill(SIGN_Q);
  await expect(page.getByTestId('vision')).toContainText("can't read images");
  await shot(page, '04e-phone-picture');
  fakeLlm.scriptChat([{ plain: true, content: SIGN_ANSWER }]);
  await page.getByTestId('vision-as-text').click();
  await expect(page.getByText(SIGN_ANSWER)).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await snapshot(page)).phase, { timeout: 15_000 }).toBe('idle');
  await yap.invoke('e2e_snip_ocr', { text: null });
  await expectStore(yap, 'chats.json', (store) => {
    const conv = store.conversations.find((c) => c.id === second);
    expect(conv, 'the new chat').toBeTruthy();
    // Only its own questions: not the first conversation's.
    expect(conv.messages.some((m) => m.text === TYPED)).toBe(false);
    expect(conv.messages.some((m) => m.text === NEW_Q)).toBe(true);
    // The picture kept as Chat keeps a pasted one, its text sent instead.
    const q = conv.messages.find((m) => m.role === 'user' && m.text.startsWith(SIGN_Q));
    expect(q.text).toContain(SIGN_TEXT);
    expect(q.images).toHaveLength(1);
    expect(q.images[0]).toMatchObject({ w: 1600, h: 1200, asText: true });
    return true;
  });

  // Back to the first conversation from the drawer: its messages, from this PC.
  await page.getByTestId('menu').click();
  await page.getByTestId('recent').filter({ hasText: 'STT stub: received' }).click();
  await expect(page.getByTestId('heard').filter({ hasText: TYPED })).toHaveCount(1, { timeout: 10_000 });
  await expect(page.getByText(TYPED_ANSWER)).toBeVisible();
  expect((await snapshot(page)).chat).toBe(first);

  // Chat's picker over the wire: the rail with Groq connected; a switch
  // switches Chat's model on this PC.
  await page.getByTestId('model-chip').click();
  await expect(page.getByTestId('picker-rail').getByRole('tab', { name: /^Groq, connected/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('picker-pane').getByRole('group', { name: 'Latest' })).toBeVisible();
  await shot(page, '04c-phone-picker');
  await page.getByTestId('picker-pane').getByTestId('picker-row').filter({ hasText: 'GPT-OSS 120B' }).click();
  await expect.poll(async () => (await yap.invoke('ai_model_status', { scope: 'chat' })).model?.id, { timeout: 10_000 }).toBe('openai/gpt-oss-120b');
  await expect.poll(async () => (await snapshot(page)).model?.id).toBe('groq|openai/gpt-oss-120b');

  // Remove: the phone is cut off at once, and says how to pair again.
  await list.getByRole('button', { name: /^Remove/ }).click();
  await expect(dialog.getByText('No phones paired yet.')).toBeVisible();
  await expect.poll(async () => (await snapshot(page)).ready, { timeout: 10_000 }).toBe(false);
  await expect(page.getByText('This phone was removed in Yap.')).toBeVisible();
  expect((await status(yap)).phones).toHaveLength(0);
  await expectStore(yap, 'link_devices.json', (store) => store.devices.length === 0);
  await shot(page, '05-phone-removed');
  await page.context().close();
  await closeSettings(main);
});

test('a used pairing link, a wrong secret, a denied phone and another site are refused', async ({ yap, main, phone }) => {
  const dialog = await openSettings(main, 'Phone');
  const { local } = await pairingLink(yap, dialog);
  const token = new URL(local).hash.replace('#pair=', '');
  const { port } = await status(yap);
  const page = await phonePage(phone);
  await page.goto(`https://127.0.0.1:${port}/`);
  /** One hello on a fresh socket: Yap's first answer. */
  const hello = (body) =>
    page.evaluate(
      ([port, body]) =>
        new Promise((done) => {
          const ws = new WebSocket(`wss://127.0.0.1:${port}/ws`);
          ws.onopen = () => ws.send(JSON.stringify({ type: 'hello', v: 1, name: 'Test phone', nonce: 'n0nce', ...body }));
          ws.onmessage = (e) => {
            const m = JSON.parse(e.data);
            done(m);
            // A pairing waits for the PC's answer on the open socket.
            if (m.type !== 'pair_wait') ws.close();
          };
          ws.onerror = () => done({ type: 'socket-error' });
        }),
      [port, body],
    );
  // A wrong secret: refused.
  expect(await hello({ device: { id: 'p-nobody', secret: 'guess' } })).toEqual({
    type: 'denied',
    reason: 'This phone was removed in Yap. Pair it again in Settings → Phone.',
  });
  // The link, once: pair_wait (the question comes up on the PC; Deny)…
  const first = await hello({ pair: { token } });
  expect(first.type).toBe('pair_wait');
  const ask = main.getByRole('alertdialog');
  await expect(ask).toContainText('Allow ‘Test phone’ to use Yap?');
  await ask.getByRole('button', { name: 'Deny' }).click();
  await expect(ask).toBeHidden();
  // …and again: used.
  expect((await hello({ pair: { token } })).reason).toContain('already used');

  // Another site's page can't open Yap's socket (its Origin isn't Yap's).
  const other = await phonePage(phone);
  await other.route('https://elsewhere.example/', (r) => r.fulfill({ status: 200, contentType: 'text/html', body: '<p>Elsewhere</p>' }));
  await other.goto('https://elsewhere.example/');
  const fromElsewhere = await other.evaluate(
    (port) =>
      new Promise((done) => {
        const ws = new WebSocket(`wss://127.0.0.1:${port}/ws`);
        ws.onopen = () => done('open');
        ws.onerror = () => done('refused');
      }),
    port,
  );
  expect(fromElsewhere).toBe('refused');
  await other.context().close();
  // Nothing was paired by any of it.
  expect((await status(yap)).phones).toHaveLength(0);
  await page.context().close();
  await closeSettings(main);
});

test("a phone's tabs: old codes never lock it out, the code scanned again is spent, a fresh code still pairs", async ({ yap, main, phone }) => {
  test.setTimeout(150_000);
  const BAD_LINK = "This pairing link isn't Yap's latest. Pair again in Settings → Phone on your PC.";
  const dialog = await openSettings(main, 'Phone');
  const { port } = await status(yap);
  const at = (hash) => `https://127.0.0.1:${port}/${hash}`;
  const connected = (p) => expect.poll(async () => (await snapshot(p)).ready, { timeout: 15_000 }).toBe(true);
  const devicesKept = () => (yap.readJson('link_devices.json')?.devices ?? []).length;
  async function allow() {
    const ask = main.getByRole('alertdialog');
    await expect(ask).toContainText(/Allow ‘.+’ to use Yap\?/);
    await ask.getByRole('button', { name: 'Allow' }).click();
    await expect(ask).toBeHidden();
  }

  // (The last spec's code may still be up: Done closes it.)
  if (!(await dialog.getByRole('button', { name: 'Pair a phone' }).isVisible())) {
    await dialog.getByRole('button', { name: 'Done', exact: true }).click();
  }
  // A phone pairs from the QR.
  const first = await pairingLink(yap, dialog);
  const a1 = await phonePage(phone);
  await a1.goto(first.local);
  await allow();
  await connected(a1);
  await expect.poll(devicesKept).toBe(1);

  // Its tabs restored at once, each with an old code in its address — the
  // one it paired with (used now), and older ones (made up): each connects
  // on the device it has; nothing asks on the PC, nothing new is paired.
  const hashes = [new URL(first.local).hash, '#pair=OldCodeFromThisMorning', '#pair=StaleCodeFromYesterday'];
  const restored = [];
  for (let i = 0; i < hashes.length; i++) restored.push(await a1.context().newPage());
  await Promise.all(restored.map((p, i) => p.goto(at(hashes[i]))));
  await Promise.all(restored.map(connected));
  for (const p of restored) expect(await p.evaluate(() => location.hash)).toBe('');
  await expect(main.getByRole('alertdialog')).toHaveCount(0);
  await expect.poll(devicesKept).toBe(1);

  // The code scanned again on the paired phone: connected, the code spent,
  // and Settings says it's paired already (no code left waiting there).
  const again = await pairingLink(yap, dialog);
  const rescan = await a1.context().newPage();
  await rescan.goto(again.local);
  await connected(rescan);
  await expect(main.getByText(/is already paired/)).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Pair a phone' })).toBeVisible();
  await expect(main.getByRole('alertdialog')).toHaveCount(0);
  await expect.poll(devicesKept).toBe(1);

  // A code made now; then a new phone's tabs (no device) with old codes:
  // refused, a guess each; five, and its address waits, told for how long.
  const live = await pairingLink(yap, dialog);
  const b = await phonePage(phone);
  const others = [b];
  for (let i = 0; i < 4; i++) others.push(await b.context().newPage());
  await Promise.all(others.map((p, i) => p.goto(at(`#pair=AnOlderPairingCode${i}`))));
  for (const p of others) await expect(p.getByTestId('pair-why')).toHaveText(BAD_LINK);
  const sixth = await b.context().newPage();
  await sixth.goto(at('#pair=OneMoreOldPairingCode'));
  await expect(sixth.getByTestId('pair-why')).toHaveText('Too many tries from this phone. Try again in 10 minutes.');
  // ...yet the paired phone, on the same address, still connects,
  await a1.reload();
  await connected(a1);
  // and the live code still pairs the new one.
  const ctxB = b.context();
  for (const p of others.slice(0, 4)) await p.close();
  const fresh = await ctxB.newPage();
  await fresh.goto(live.local);
  await expect(fresh.getByTestId('pair-code')).toHaveText(/^\d{4}$/);
  await allow();
  await connected(fresh);
  await expect.poll(devicesKept).toBe(2);
  // Its tabs refused before come back with the new device by themselves.
  await connected(others[4]);
  await connected(sixth);

  // Tidy up: no phones left paired.
  for (const p of (await status(yap)).phones) await yap.invoke('link_remove', { id: p.id });
  await expect.poll(() => devicesKept()).toBe(0);
  await a1.context().close();
  await ctxB.close();
  await closeSettings(main);
});
