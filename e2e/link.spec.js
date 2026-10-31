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
// - A question: Talk on the phone (the fake mic's audio up), the stub's
//   transcript heard, the fake model's answer streamed to the phone, Yap's
//   voice as audio down (test mode's stand-in voice: silence of the right
//   length) — never this PC's speakers — and the conversation kept in Chat.
// - Remove cuts the phone off at once; a replayed pairing link, a wrong
//   secret and another site's page are refused.
// - Dictation from the phone: "Dictate to PC" through the pipeline's own
//   path (the stub's transcript) into a text box the test keeps open (the
//   stand-in window, as Notepad: a test run pastes into nothing real),
//   "Pasted into Notepad" on the phone, history marked as the phone's;
//   "Dictate to phone" with the text back and no paste; "May type into
//   this PC" switched off in Settings refusing it; an Ask after them.
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
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;

/** Talk tapped, a second of the fake mic, Talk tapped again to send. */
async function talkOnce(page) {
  const talk = page.getByTestId('talk');
  await talk.click();
  await expect.poll(async () => (await snapshot(page)).local, { timeout: 10_000 }).toBe('listening');
  await expect.poll(async () => (await snapshot(page)).frames, { timeout: 10_000 }).toBeGreaterThan(20);
  await page.waitForTimeout(700);
  await talk.click();
}

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

  // A question: Talk, a second of the fake mic, Talk again to send.
  fakeLlm.scriptChat([{ plain: true, content: ANSWER }]);
  const talk = page.getByTestId('talk');
  await talk.click();
  await expect.poll(async () => (await snapshot(page)).frames, { timeout: 10_000 }).toBeGreaterThan(20);
  await page.waitForTimeout(700);
  await talk.click();
  // The stub's transcript heard, the answer shown on the phone.
  await expect(page.getByText(/STT stub: received/)).toBeVisible({ timeout: 20_000 });
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

test('a phone dictates into the PC and to itself; typing into the PC switched off is refused; an Ask still works', async ({
  yap,
  main,
  phone,
  fakeLlm,
  shot,
}) => {
  test.setTimeout(180_000);
  const dialog = await openSettings(main, 'Phone');
  // The last test's pairing code may still be up (Settings stays mounted).
  const done = dialog.getByRole('button', { name: 'Done' });
  if (await done.isVisible()) await done.click();
  const { local } = await pairingLink(yap, dialog);
  const context = await phone.newContext({
    ignoreHTTPSErrors: true,
    viewport: { width: 390, height: 844 },
    permissions: ['microphone', 'clipboard-read', 'clipboard-write'],
  });
  const page = await context.newPage();
  await page.goto(local);
  // The pairing question names typing into this PC.
  const ask = main.getByRole('alertdialog');
  await expect(ask).toContainText('type what you dictate on it into this PC');
  await ask.getByRole('button', { name: 'Allow' }).click();
  await expect.poll(async () => (await snapshot(page)).ready, { timeout: 15_000 }).toBe(true);
  // Paired from now on: it may type into this PC.
  const list = dialog.getByRole('list', { name: 'Paired phones' });
  const typing = list.getByRole('button', { name: /^May type into this PC/ });
  await expect(typing).toHaveAttribute('aria-pressed', 'true');
  expect((await status(yap)).phones[0].typeIntoPc).toBe(true);
  expect((await snapshot(page)).mayType).toBe(true);
  await shot(main, '06-settings-phone-typing');

  // The text box the test opens: Yap's stand-in window, kept open, as
  // Notepad (a test run reads and pastes into no real window).
  await yap.invoke('e2e_target_window', { on: true, open: true });
  await yap.invoke('e2e_style_app', { exe: 'notepad.exe', title: 'Untitled - Notepad' });
  try {
    const pastes = logCount(yap, 'e2e: not pasting the text');
    await page.getByRole('radio', { name: 'Dictate to PC', exact: true }).click();
    await talkOnce(page);
    // The pipeline's own path: the stub's transcript, pasted, said on the phone.
    await expect(page.getByTestId('result-line')).toHaveText('Pasted into Notepad', { timeout: 30_000 });
    await expect(page.getByTestId('result-text')).toContainText('STT stub: received');
    await expect.poll(() => logCount(yap, 'e2e: not pasting the text')).toBe(pastes + 1);
    expect(fs.readFileSync(yap.logPath, 'utf8')).toContain('Recording started from a phone');
    // In history, marked as the phone's, in the app it went into.
    await expectStore(yap, 'history.json', (h) =>
      h.some((e) => e.via === 'phone' && e.app !== 'phone' && e.text.includes('STT stub: received')),
    );
    await shot(page, '06-phone-dictated-to-pc');

    // Dictate to phone: the text comes back, copied; nothing pasted here.
    await page.getByRole('radio', { name: 'Dictate to phone', exact: true }).click();
    await talkOnce(page);
    await expect(page.getByTestId('result-text')).toContainText('STT stub: received', { timeout: 30_000 });
    await expect(page.getByTestId('result-line')).toHaveText('Copied');
    expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('STT stub: received');
    expect(logCount(yap, 'e2e: not pasting the text')).toBe(pastes + 1);
    await expectStore(yap, 'history.json', (h) => h.some((e) => e.app === 'phone' && e.via === 'phone'));
    await shot(page, '07-phone-dictated-to-phone');

    // "May type into this PC" off: the phone hears at once, and Yap itself
    // refuses a dictation to the PC (a page that asked anyway).
    await typing.click();
    await expect(typing).toHaveAttribute('aria-pressed', 'false');
    await expectStore(yap, 'link_devices.json', (s) => s.devices[0].typeIntoPc === false);
    await expect.poll(async () => (await snapshot(page)).mayType).toBe(false);
    await page.getByRole('radio', { name: 'Dictate to PC', exact: true }).click();
    await expect(page.getByTestId('mode-hint')).toBeVisible();
    const { port } = await status(yap);
    const refused = await page.evaluate(
      (port) =>
        new Promise((done) => {
          const device = JSON.parse(localStorage.getItem('yap.link.device'));
          const ws = new WebSocket(`wss://127.0.0.1:${port}/ws`);
          ws.onopen = () => ws.send(JSON.stringify({ type: 'hello', v: 1, name: 'Test phone', nonce: 'n0nce-2', device }));
          ws.onmessage = (e) => {
            const m = JSON.parse(e.data);
            if (m.type === 'ready') ws.send(JSON.stringify({ type: 'start', mode: 'dictate', target: 'pc' }));
            if (m.type === 'error' || m.type === 'denied') {
              done(m);
              ws.close();
            }
          };
          ws.onerror = () => done({ type: 'socket-error' });
        }),
      port,
    );
    expect(refused.type).toBe('error');
    expect(refused.message).toContain('May type into this PC');
    expect(logCount(yap, 'e2e: not pasting the text')).toBe(pastes + 1);
    // Dictate to phone needs no permission.
    await page.getByRole('radio', { name: 'Dictate to phone', exact: true }).click();
    await expect(page.getByTestId('mode-hint')).toBeHidden();

    // An Ask after it all: answered as ever.
    fakeLlm.scriptChat([{ plain: true, content: ANSWER }]);
    await page.getByRole('radio', { name: 'Ask Yap', exact: true }).click();
    await talkOnce(page);
    await expect(page.getByText(ANSWER)).toBeVisible({ timeout: 30_000 });
    await expect.poll(async () => (await snapshot(page)).phase, { timeout: 30_000 }).toBe('idle');
  } finally {
    await yap.invoke('e2e_target_window', { on: false }).catch(() => {});
    await yap.invoke('e2e_style_app', {}).catch(() => {});
  }
  await list.getByRole('button', { name: /^Remove/ }).click();
  await expect(dialog.getByText('No phones paired yet.')).toBeVisible();
  await context.close();
  await closeSettings(main);
});
