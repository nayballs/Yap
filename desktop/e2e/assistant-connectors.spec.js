// Connectors everywhere else in Chat (Connectors wave 6: desktop/src-tauri/src/chat/assistant/connector_ask.rs,
// chat/tools/connectors.rs, integrations/connectors/chats.rs): the Voice Assistant, a paired phone and a meeting.
// A program connector on this PC (support/fake-mcp.js --chat over stdio, "Home lights") and a strong fake model
// (support/fake-llm.js's "fake-gpt-5", scripted call by call); questions asked out loud with the debug-only
// `e2e_assistant_ask` (the shortcut's own steps, a transcript in the mic's place), from "another app".
// - Asked out loud, a read set to Always allow runs and its answer is said.
// - A write that asks: one spoken line ("Use Home lights to create page “Launch notes”? Say yes, or press Allow.")
//   and Allow once / Don't on the Yap bar; the next spoken "yes" in that conversation allows it once (the turn
//   goes on and its answer is said), a spoken "no" says Don't, the bar's own Allow once works too; a tool that can
//   delete takes a click ("That one needs a click…" to a spoken yes).
// - A connector with "Voice Assistant" off in Where Yap uses it isn't offered out loud (typed in Chat it is).
// - From a paired phone (a headless client speaking Yap Link's wire, as link-code.spec.js's): only with the
//   connector's "Your phone" on, its reads only — a read runs, a write is refused with the phone's step line
//   "That needs your OK on the PC · …", and no card.
// - During a call: nothing offered unless "Let connectors work during meetings" and the connector's own switch
//   are both on; then a read runs and a write the person set to Always allow waits for a click in Chat (Allow
//   once / Deny, never Always allow), and the Voice Assistant is off.
// Fictional data only; nothing leaves the PC.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';
import { test as base, expect, openView, setConfig, inAnotherApp, mainOnScreen } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';

const FAKE = fileURLToPath(new URL('./support/fake-mcp.js', import.meta.url));
const NODE = process.execPath;
const DIR = path.join(RUNS, 'assistant-connectors-fakes');
const PHONE_NAME = 'Kitchen tablet';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      fake.setServerModels([{ id: 'fake-chat' }, { id: 'fake-gpt-5' }]);
      fake.serveOllama({ loaded: { 'fake-gpt-5': 32768 } });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'assistant-connectors',
        env: { YAP_E2E_LINK_PORT: '0' },
        config: {
          connectorsPreview: true,
          webSearch: false,
          memoryLearnChats: false,
          speakReplies: true,
          linkEnabled: true,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } },
        },
      });
    },
    { scope: 'worker' },
  ],
  // The phone's browser: headless, no microphone (the client speaks the wire itself).
  browser: [
    async ({}, use) => {
      let channel = process.env.YAP_LINK_CHANNEL;
      if (!channel) {
        try {
          channel = fs.existsSync(chromium.executablePath()) ? 'chromium' : 'msedge';
        } catch {
          channel = 'msedge';
        }
      }
      const b = await chromium.launch({ channel, headless: true });
      await use(b);
      await b.close();
    },
    { scope: 'worker' },
  ],
});

const lines = (file) =>
  fs.existsSync(file)
    ? fs
        .readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];

/** fake-mcp.js added as a program connector ("Home lights"), shown first as Yap does; its id and its calls. */
async function addFake(yap, name) {
  fs.mkdirSync(DIR, { recursive: true });
  const log = path.join(DIR, `${name}.log`);
  fs.rmSync(log, { force: true });
  const spec = { name: 'Home lights', command: NODE, args: [FAKE, '--log', log, '--chat'] };
  const preview = await yap.invoke('connector_preview', { spec });
  const added = await yap.invoke('connector_add', { spec: { ...spec, approved: preview.program.fingerprint } });
  expect(added.status.state).toBe('connected');
  const called = (tool) => lines(log).filter((l) => l.kind === 'request' && l.method === 'tools/call' && (!tool || l.tool === tool));
  return { id: added.id, called };
}

const chatCalls = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.tools?.length);
const spoken = async (yap) => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text);
const said = (yap, text) => expect.poll(async () => (await spoken(yap)).join('\n'), { timeout: 20_000, message: `Yap to say “${text}”` }).toContain(text);
const ask = (yap, text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
const barCards = async (yap) => (await yap.invoke('bar_status')).cards;
const barCard = async (yap) => (await barCards(yap)).find((c) => c.id === 'askFirst') ?? null;
const create = { name: 'homelights__create_page', args: { title: 'Launch notes' } };
/** The conversations with a question that says `text`. */
const chatsAsking = (yap, text) => (yap.readJson('chats.json')?.conversations ?? []).filter((c) => c.messages.some((m) => m.role === 'user' && m.text === text));

test.beforeEach(async ({ yap }) => {
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await inAnotherApp(yap);
});

test('asked out loud, a read set to Always allow runs and its answer is said', async ({ yap, fakeLlm }) => {
  const fake = await addFake(yap, 'read');
  try {
    const from = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ tools: [{ name: 'homelights__list_lights', args: {} }] }, { content: 'The kitchen light is on and the hall is off.' }]);
    await ask(yap, 'Which of my lights are on?');
    await said(yap, 'The kitchen light is on and the hall is off.');
    expect(fake.called('list_lights')).toHaveLength(1);
    const calls = chatCalls(fakeLlm, from);
    expect(calls[0].tools).toEqual(expect.arrayContaining(['homelights__list_lights', 'homelights__create_page']));
    // No question to answer: no card on the bar.
    expect(await barCard(yap)).toBeNull();
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('a write that asks: the spoken line and the bar card, then a spoken yes allows it once and the answer is said', async ({ yap, fakeLlm, shot }) => {
  const fake = await addFake(yap, 'yes');
  // (The main window's fixture brings it up: asked from another app again.)
  await inAnotherApp(yap);
  try {
    fakeLlm.scriptChat([{ tools: [create] }, { content: 'I made the page Launch notes.' }]);
    await ask(yap, 'Make a page called Launch notes');
    // One line, said; Allow once / Don't on the bar.
    await said(yap, 'Use Home lights to create page “Launch notes”? Say yes, or press Allow.');
    await expect.poll(() => barCard(yap), { timeout: 15_000 }).not.toBeNull();
    const card = await barCard(yap);
    expect(card).toMatchObject({ title: 'Use Home lights: Create page?', body: 'title: Launch notes', icon: 'assistant' });
    expect([card.primary.label, card.secondary.label]).toEqual(['Allow once', "Don't"]);
    await expect(yap.overlay.locator('.card').filter({ hasText: 'Use Home lights: Create page?' })).toBeVisible();
    await shot(yap.overlay, '01-bar-card-light');
    expect(fake.called('create_page')).toHaveLength(0);
    // The spoken yes: the same conversation's next question.
    await ask(yap, 'Yes please');
    await said(yap, 'I made the page Launch notes.');
    expect(fake.called('create_page')).toHaveLength(1);
    await expect.poll(() => barCard(yap)).toBeNull();
    // The yes was an answer, not a question of its own: the question, then its answer, and no "Yes please".
    const [conv] = chatsAsking(yap, 'Make a page called Launch notes');
    expect(conv.messages.some((m) => m.text === 'Yes please')).toBe(false);
    const at = conv.messages.findIndex((m) => m.role === 'user' && m.text === 'Make a page called Launch notes');
    expect(conv.messages.slice(at).map((m) => m.role)).toEqual(['user', 'assistant']);
    expect(conv.messages[at + 1].confirms?.[0]).toMatchObject({ act: 'connector', state: 'allowed', connector: { voice: true, spoken: true } });
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test("a spoken no says Don't; the bar's Allow once answers too, in the dark theme", async ({ yap, main, fakeLlm, shot }) => {
  const fake = await addFake(yap, 'no');
  const theme = (await yap.invoke('get_config')).theme;
  await inAnotherApp(yap);
  try {
    fakeLlm.scriptChat([{ tools: [create] }, { content: 'OK, I left it.' }]);
    await ask(yap, 'Make a page called Launch notes');
    await expect.poll(() => barCard(yap), { timeout: 15_000 }).not.toBeNull();
    await ask(yap, 'No thanks');
    await said(yap, 'OK, I left it.');
    expect(fake.called('create_page')).toHaveLength(0);
    await expect.poll(() => barCard(yap)).toBeNull();

    // The bar's own Allow once, dark.
    await setConfig(yap, main, { theme: 'dark' });
    await inAnotherApp(yap);
    fakeLlm.scriptChat([{ tools: [create] }, { content: 'Made it from the bar.' }]);
    await ask(yap, 'Make a page called Launch notes');
    await expect.poll(() => barCard(yap), { timeout: 15_000 }).not.toBeNull();
    await expect(yap.overlay.locator('.card').filter({ hasText: 'Use Home lights: Create page?' })).toBeVisible();
    await shot(yap.overlay, '02-bar-card-dark');
    const card = await barCard(yap);
    await yap.invoke('bar_card_action', { id: 'askFirst', action: card.primary.id });
    await said(yap, 'Made it from the bar.');
    expect(fake.called('create_page')).toHaveLength(1);
  } finally {
    await setConfig(yap, main, { theme });
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('a tool that can delete takes a click: a spoken yes is told so and it still waits', async ({ yap, fakeLlm }) => {
  const fake = await addFake(yap, 'delete');
  try {
    fakeLlm.scriptChat([{ tools: [{ name: 'homelights__delete_all', args: {} }] }, { content: 'I left the automations alone.' }]);
    await ask(yap, 'Delete every automation');
    await said(yap, 'Use Home lights to delete all? It needs a click: press Allow on the Yap bar.');
    await expect.poll(() => barCard(yap), { timeout: 15_000 }).not.toBeNull();
    expect((await barCard(yap)).status).toBe('This one needs a click');
    await ask(yap, 'Yes');
    await said(yap, 'That one needs a click: press Allow on the Yap bar.');
    // Still waiting: nothing ran, the card's still up.
    expect(fake.called('delete_all')).toHaveLength(0);
    expect(await barCard(yap)).not.toBeNull();
    const card = await barCard(yap);
    await yap.invoke('bar_card_action', { id: 'askFirst', action: card.secondary.id });
    await said(yap, 'I left the automations alone.');
    expect(fake.called('delete_all')).toHaveLength(0);
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('a connector with the Voice Assistant off in Where Yap uses it is never offered out loud', async ({ yap, fakeLlm }) => {
  const fake = await addFake(yap, 'where');
  try {
    await yap.invoke('connector_set_where', { id: fake.id, places: { voice: false } });
    const from = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: "I can't reach your lights from here." }]);
    await ask(yap, 'Which of my lights are on?');
    await said(yap, "I can't reach your lights from here.");
    const [call] = chatCalls(fakeLlm, from);
    expect(call.tools.some((t) => t.startsWith('homelights__'))).toBe(false);
    expect(fake.called()).toHaveLength(0);
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

/** A phone that speaks the wire itself (link-code.spec.js's client), paired with this Yap. */
async function pairedPhone(yap, browser) {
  await expect.poll(async () => (await yap.invoke('link_status')).running).toBe(true);
  const { port } = await yap.invoke('link_status');
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  const page = await context.newPage();
  await page.goto(`https://127.0.0.1:${port}/icon.svg`);
  const url = `wss://127.0.0.1:${port}/ws`;
  const msgs = () => page.evaluate(() => window.__yap.msgs);
  const phone = {
    msgs,
    send: (m) => page.evaluate((m) => window.__yap.ws.send(JSON.stringify(m)), m),
    async wait(type, pred = () => true, timeout = 20_000) {
      let found = null;
      await expect
        .poll(async () => {
          found = (await msgs()).find((m) => m.type === type && pred(m)) ?? null;
          return !!found;
        }, { timeout, message: `the phone to hear ${type}` })
        .toBe(true);
      return found;
    },
    close: () => context.close(),
  };
  const pair = await yap.invoke('link_pair_start', { address: null });
  const token = pair.url.split('#pair=')[1];
  await page.evaluate(
    ({ url, hello }) =>
      new Promise((resolve, reject) => {
        const ws = new WebSocket(url);
        ws.binaryType = 'arraybuffer';
        window.__yap = { ws, msgs: [] };
        ws.onmessage = (e) => typeof e.data === 'string' && window.__yap.msgs.push(JSON.parse(e.data));
        ws.onopen = () => {
          ws.send(JSON.stringify(hello));
          resolve();
        };
        ws.onerror = () => reject(new Error('the socket failed'));
      }),
    { url, hello: { type: 'hello', v: 1, name: PHONE_NAME, nonce: 'n-0001', pair: { token } } },
  );
  await phone.wait('pair_wait');
  await expect.poll(async () => (await yap.invoke('link_status')).asks.length).toBe(1);
  await yap.invoke('link_pair_answer', { id: (await yap.invoke('link_status')).asks[0].id, allow: true });
  await phone.wait('paired');
  await phone.wait('ready');
  return phone;
}

test("from the phone: only with Your phone on, its reads only — a read runs, a write is refused with the phone's line", async ({ yap, fakeLlm, browser }) => {
  const fake = await addFake(yap, 'phone');
  const phone = await pairedPhone(yap, browser);
  try {
    // "Your phone" off (the default): nothing offered there.
    let from = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Nothing to check from here.' }]);
    await phone.send({ type: 'ask', text: 'Which of my lights are on?', chat: 'new' });
    await phone.wait('answer', (m) => m.final && /Nothing to check/.test(m.text ?? ''));
    expect(chatCalls(fakeLlm, from)[0].tools.some((t) => t.startsWith('homelights__'))).toBe(false);

    // On: the read set to Always allow runs; the write, never offered, is refused by name.
    await yap.invoke('connector_set_where', { id: fake.id, places: { phone: true } });
    from = fakeLlm.requests.length;
    fakeLlm.scriptChat([
      { tools: [{ name: 'homelights__list_lights', args: {} }] },
      { tools: [create] },
      { content: 'The kitchen light is on. Making the page needs your OK on the PC.' },
    ]);
    await phone.send({ type: 'ask', text: 'Which lights are on? And make a page called Launch notes', chat: 'new' });
    await phone.wait('step', (m) => m.label === 'That needs your OK on the PC · Home lights · Create page');
    await phone.wait('answer', (m) => m.final && /needs your OK on the PC/.test(m.text ?? ''));
    expect(fake.called('list_lights')).toHaveLength(1);
    expect(fake.called('create_page')).toHaveLength(0);
    const calls = chatCalls(fakeLlm, from);
    expect(calls[0].tools).toContain('homelights__list_lights');
    expect(calls[0].tools).not.toContain('homelights__create_page');
    const results = calls.at(-1).messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
    expect(results.at(-1)).toMatchObject({ success: false, displayText: "Didn't use Home lights · Create page: it needs your OK on the PC" });
    // No card anywhere, nor on this PC's bar.
    expect(await barCard(yap)).toBeNull();
  } finally {
    await phone.close();
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('during a call: nothing unless both meeting switches allow it, then a read runs and a write waits for a click', async ({ yap, main, fakeLlm, shot }) => {
  const fake = await addFake(yap, 'meeting');
  const card = () => main.getByTestId('chat-connector-confirm');
  const typed = async (text) => {
    await openView(main, 'Chat');
    await main.getByRole('button', { name: 'New chat' }).first().click();
    await main.getByRole('textbox', { name: 'Message' }).fill(text);
    await main.getByRole('button', { name: 'Send' }).click();
  };
  try {
    // A write the person set to Always allow (it'd run outside a meeting).
    await yap.invoke('connector_set_tool', { id: fake.id, tool: 'create_page', setting: 'allow' });
    await mainOnScreen(yap);
    await yap.invoke('meeting_detect_simulate', { appId: 'webex', active: true });
    // The Voice Assistant is off during a call.
    await expect(ask(yap, 'Which of my lights are on?')).rejects.toThrow(/off during meetings/);
    // Neither switch: nothing offered in Chat.
    let from = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Not during the call.' }]);
    await typed('Which of my lights are on?');
    await expect(main.getByText('Not during the call.')).toBeVisible({ timeout: 15_000 });
    expect(chatCalls(fakeLlm, from)[0].tools.some((t) => t.startsWith('homelights__'))).toBe(false);
    // "Let connectors work during meetings" alone isn't enough: the connector's own switch too.
    await setConfig(yap, main, { connectorsInMeetings: true });
    from = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Still not.' }]);
    await typed('Which of my lights are on?');
    await expect(main.getByText('Still not.')).toBeVisible({ timeout: 15_000 });
    expect(chatCalls(fakeLlm, from)[0].tools.some((t) => t.startsWith('homelights__'))).toBe(false);
    await yap.invoke('connector_set_where', { id: fake.id, places: { meetings: true } });
    // Both: a read runs; the write waits for a click (Allow once or Deny, never Always allow).
    fakeLlm.scriptChat([{ tools: [{ name: 'homelights__list_lights', args: {} }] }, { tools: [create] }, { content: 'The kitchen light is on; I left the page.' }]);
    await typed('Which lights are on? And make a page called Launch notes');
    await expect(card()).toHaveAttribute('data-state', 'asking', { timeout: 15_000 });
    await expect(card()).toContainText('Use Home lights: Create page?');
    await expect(card().getByRole('button')).toHaveText(['Allow once', 'Deny']);
    await shot(main, '03-a-write-during-a-call');
    expect(fake.called('list_lights')).toHaveLength(1);
    await card().getByRole('button', { name: 'Deny' }).click();
    await expect(main.getByText('The kitchen light is on; I left the page.')).toBeVisible({ timeout: 15_000 });
    expect(fake.called('create_page')).toHaveLength(0);
  } finally {
    await yap.invoke('meeting_detect_simulate', { appId: 'webex', active: false });
    await setConfig(yap, main, { connectorsInMeetings: false });
    await yap.invoke('connector_remove', { id: fake.id });
  }
});
