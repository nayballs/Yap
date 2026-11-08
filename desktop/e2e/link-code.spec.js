// Remote Control (desktop/src-tauri/src/link/code.rs, docs/architecture/link.md
// "Remote Control"): Yap Code from a paired phone, wave 1 — the wire and the
// PC's side. The "phone" here is a headless client speaking Yap Link's
// protocol itself (a WebSocket opened from a page of the PC's own origin, as
// the phone's page opens it), against a test Yap with the fake AI and Claude
// Code's stand-in (`e2e_claude_script`, ai/claude_code/fake.rs): nobody's
// real Claude Code, account or files; the workspace is this run's own folder.
//
// - A phone paired while Remote Control is on is ticked for it: `ready.can`
//   has code, code_act and approve, and `ready.code.on`.
// - `code_new`, then a request sent from the phone: a new Code chat in the
//   workspace, its agent started there; `code_list` lists them; the agent
//   opened (`agent_open`): its snapshot, then its live events.
// - Its permission card on the phone (`approve_card`): a tap too soon is
//   refused and the card still waits; a fresh tap allows it — `approve_done`
//   "you", the PC's card closed "Allowed from Kitchen tablet · …", the agent's
//   log and the audit trail saying so (never the command in the trail).
// - Its report said on that phone (`speak_start` and Yap's voice down its
//   socket), never this PC's speakers; `report` and the marker as a
//   `chat_message` with its kind.
// - A Bypass chat: `set_mode` Bypass refused from the phone, and the phone's
//   follow-up runs at Auto; Stop from the phone.
// - Remote Control off (and a phone not ticked): `ready.code` says why, and
//   every Code message is refused.
import fs from 'node:fs';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { test as base, expect, openView, openSettings, closeSettings, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide } from './support/code-chats.js';

const HERE = path.join(RUNS, 'link-code-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const PHONE_NAME = 'Kitchen tablet';

const DONE = 'Done. I changed Draft to Final in notes.md and the tests pass.';
const REPORT = 'Done: I changed Draft to Final in notes.md and the tests pass. There is a backup if you want it back.';

// ---- the stand-in's scripts ----

const init = (mode) => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: mode,
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const said = (id, content, stop = 'tool_use') => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const toolResult = (id, text) => ({
  type: 'user',
  message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const result = (text, extra = {}) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: 0.02,
  usage: { input_tokens: 3000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 1000 },
  permission_denials: [],
  terminal_reason: 'completed',
  ...extra,
});

/** Edits notes.md (backed up first), then asks to run the tests, and reports. */
const EDIT = [
  { _wait_user: true },
  init('auto'),
  said('m1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  toolResult('toolu_read', 'Draft notes'),
  said('m2', [{ type: 'tool_use', id: 'toolu_edit', name: 'Edit', input: { file_path: '{{cwd}}\\notes.md', old_string: 'Draft', new_string: 'Final' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_edit', tool_input: { file_path: 'notes.md', old_string: 'Draft', new_string: 'Final' } } } },
  { _write_file: { path: 'notes.md', text: 'Final notes\n' } },
  toolResult('toolu_edit', 'The file notes.md has been updated.'),
  said('m3', [{ type: 'tool_use', id: 'toolu_bash', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'npm test' }, tool_use_id: 'toolu_bash' } } },
  toolResult('toolu_bash', '12 passing'),
  said('m4', [{ type: 'text', text: DONE }], 'end_turn'),
  result(DONE),
  { _wait_user: true },
];
/** Takes its time (a Stop interrupts it). */
const SLOW = [
  { _wait_user: true },
  init('auto'),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 60000 },
  result('Finished slowly.'),
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

// ---- the app and the phone ----

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
    async ({}, use) => {
      await use({ name: 'link-code', env: { YAP_E2E_LINK_PORT: '0' }, config: { linkEnabled: true, memoryLearnChats: false } });
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

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const status = (yap) => yap.invoke('link_status');
const agents = (yap) => yap.invoke('agent_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const spoken = (yap) => yap.invoke('tts_e2e_spoken');

/**
 * **A phone that speaks the wire itself**: a page of the PC's own origin
 * (so the socket's Origin is the page's, as the phone's), its WebSocket,
 * what came down it (text messages, and how many binary frames).
 */
async function phoneClient(browser, port) {
  const context = await browser.newContext({ ignoreHTTPSErrors: true });
  const page = await context.newPage();
  await page.goto(`https://127.0.0.1:${port}/icon.svg`);
  const url = `wss://127.0.0.1:${port}/ws`;
  const c = {
    page,
    async connect(hello) {
      await page.evaluate(
        ({ url, hello }) =>
          new Promise((resolve, reject) => {
            const ws = new WebSocket(url);
            ws.binaryType = 'arraybuffer';
            const me = { ws, msgs: [], frames: 0, framesAt: [], closed: false };
            window.__yap = me;
            ws.onmessage = (e) => {
              if (typeof e.data === 'string') me.msgs.push(JSON.parse(e.data));
              else {
                me.frames += 1;
                me.framesAt.push(me.msgs.length);
              }
            };
            ws.onclose = () => {
              me.closed = true;
            };
            ws.onopen = () => {
              ws.send(JSON.stringify(hello));
              resolve();
            };
            ws.onerror = () => reject(new Error('the socket failed'));
          }),
        { url, hello },
      );
    },
    send: (m) => page.evaluate((m) => window.__yap.ws.send(JSON.stringify(m)), m),
    msgs: () => page.evaluate(() => window.__yap.msgs),
    frames: () => page.evaluate(() => window.__yap.frames),
    /** How many messages came so far (a mark to wait after). */
    mark: async () => (await c.msgs()).length,
    /** The first message of `type` after `after` that `pred` takes. */
    async wait(type, { after = 0, pred = () => true, timeout = 20_000 } = {}) {
      let found = null;
      await expect
        .poll(
          async () => {
            const all = await c.msgs();
            found = all.slice(after).find((m) => m.type === type && pred(m)) ?? null;
            return !!found;
          },
          { timeout, message: `the phone to hear ${type}` },
        )
        .toBe(true);
      return found;
    },
    /** Send `m`, and wait for the answer of `type`. */
    async ask(m, type, opts = {}) {
      const after = await c.mark();
      await c.send(m);
      return c.wait(type, { after, ...opts });
    },
    close: () => page.evaluate(() => window.__yap?.ws.close()),
  };
  return c;
}

/** Hear app event `event` in the main window from now on. */
async function listen(main, event) {
  await main.evaluate(async (ev) => {
    window.__heard = window.__heard || {};
    window.__heard[ev] = [];
    const handler = window.__TAURI_INTERNALS__.transformCallback((e) => window.__heard[ev].push(e.payload));
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: ev, target: { kind: 'Any' }, handler });
  }, event);
}
const heard = (main, event) => main.evaluate((ev) => window.__heard?.[ev] ?? [], event);

/** The agent `id` once it's over. */
async function ended(yap, id, timeout = 40_000) {
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout, message: `agent ${id} to end` }).toMatch(/^(done|failed|stopped)$/);
  return (await agents(yap)).find((j) => j.id === id);
}

test.describe.configure({ mode: 'serial' });

/** Shared by the tests: the phone's device and its client. */
let device = null;
let phone = null;

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  // The stand-in first, so the gates find Claude Code when the config's saved.
  await script(yap, EDIT, 'edit');
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  await yap.invoke('tts_stop');
  await setConfig(yap, main, {
    linkEnabled: true,
    linkRemoteControl: true,
    agentsEnabled: true,
    agentsAsk: 'never',
    agentsSpeak: 'voice',
    webSearch: false,
    speakReplies: false,
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    fs.rmSync(GARDEN, { recursive: true, force: true });
    fs.mkdirSync(GARDEN, { recursive: true });
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
});

test('a phone paired while Remote Control is on is ticked for it, and ready says the Code side is on', async ({ yap, main, browser }) => {
  await expect.poll(async () => (await status(yap)).running).toBe(true);
  const { port } = await status(yap);
  const pair = await yap.invoke('link_pair_start', { address: null });
  const token = pair.url.split('#pair=')[1];
  phone = await phoneClient(browser, port);
  await phone.connect({ type: 'hello', v: 1, name: PHONE_NAME, nonce: 'n-0001', pair: { token } });
  await phone.wait('pair_wait');
  // The person at the PC says yes.
  await expect.poll(async () => (await status(yap)).asks.length).toBe(1);
  await yap.invoke('link_pair_answer', { id: (await status(yap)).asks[0].id, allow: true });
  const paired = await phone.wait('paired');
  device = paired.device;
  const ready = await phone.wait('ready');
  expect(ready.can).toEqual(expect.arrayContaining(['code', 'code_act', 'approve']));
  expect(ready.code).toEqual({ on: true });
  // The dots straight away.
  const dots = await phone.wait('code_status');
  expect(dots).toMatchObject({ answering: expect.any(Array), agents: expect.any(Array), approvals: [], confirms: [] });
  // Ticked in its own file, never in link_devices.json.
  await expectStore(yap, 'remote_control.json', (store) => {
    expect(store.phones[device.id].on).toBe(true);
    return true;
  });
  expect(JSON.stringify(yap.readJson('link_devices.json'))).not.toContain('"on"');
  // Settings → Phone: its tick, and Remote Control's switch.
  const dialog = await openSettings(main, 'Phone');
  await expect(dialog.getByRole('checkbox', { name: `Remote Control for ${PHONE_NAME}` })).toBeChecked();
  await expect(dialog.getByRole('button', { name: 'Remote Control', exact: true })).toBeVisible();
  await closeSettings(main);
});

test('a request from the phone into a new Code chat runs its agent; the phone allows its step with a fresh tap, and hears the report', async ({ yap, main, fakeLlm }) => {
  test.setTimeout(120_000);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await listen(main, 'yap-claude-approve-done');
  // A new Code chat in the workspace, from the phone.
  const thread = await phone.ask({ type: 'code_new', workspace: 'garden-planner' }, 'thread');
  expect(thread).toMatchObject({ id: null, code: { workspace: 'garden-planner', path: GARDEN, phoneMax: 'auto', allowed: true } });
  // The request: typed on the phone.
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Change Draft to Final in notes.md, then run the tests.', title: 'Change Draft to Final' } }] },
    { content: "On it: I've sent an agent to garden-planner." },
    { plain: true, content: REPORT },
  ]);
  const before = (await agents(yap)).length;
  const asked = await phone.mark();
  await phone.send({ type: 'ask', text: 'Change Draft to Final in notes.md and run the tests', chat: 'new' });
  const chat = await phone.wait('chat', { after: asked });
  await phone.wait('answer', { after: asked, pred: (m) => m.final && /sent an agent/.test(m.text) });
  // A Code chat there, its agent in the workspace.
  await expect.poll(async () => (await agents(yap)).length).toBe(before + 1);
  const agent = (await agents(yap))[0];
  expect(agent).toMatchObject({ folder: 'garden-planner', conversation: chat.id, mode: 'auto' });
  expect((await yap.invoke('code_chats_list')).chats[chat.id]?.name).toBe('garden-planner');
  // The PC remembers the question as the phone's (its agent's turn is the phone's).
  await expectStore(yap, 'remote_control.json', (store) => {
    expect(store.asked.some((m) => m.key === agent.messageId && m.device === device.id)).toBe(true);
    return true;
  });

  // The Code side: the workspace, the chat, the agent.
  const list = await phone.ask({ type: 'code_list' }, 'code_list');
  expect(list.workspaces.map((w) => w.name)).toContain('garden-planner');
  expect(list.chats.find((c) => c.id === chat.id)).toMatchObject({ workspace: 'garden-planner' });
  expect(list.agents.find((a) => a.id === agent.id)).toMatchObject({ folder: 'garden-planner', conversation: chat.id, turnList: [expect.objectContaining({ n: 1 })] });

  // The agent opened: its snapshot, then its live events.
  const opened = await phone.mark();
  const snap = await phone.ask({ type: 'agent_open', id: agent.id }, 'agent_snapshot');
  expect(snap).toMatchObject({ agent: { id: agent.id, folder: 'garden-planner' }, seq: 0 });
  expect(Array.isArray(snap.events)).toBe(true);

  // Its permission card comes to the phone.
  const cardMsg = await phone.wait('approve_card', { after: asked, timeout: 30_000 });
  expect(cardMsg.card).toMatchObject({ run: agent.id, conversation: chat.id, tool: 'Bash', summary: 'Run a command: npm test' });
  expect(cardMsg.expiresTs).toBeGreaterThan(cardMsg.card.askedTs);

  // A stale tap (shown under 400 ms): refused, the card still waits.
  const stale = await phone.ask({ type: 'approve', id: cardMsg.card.id, allow: true, shownMs: 120 }, 'error');
  expect(stale.message).toBe('Tap it again: the card had only just come.');
  expect((await yap.invoke('claude_approve_pending')).map((p) => p.id)).toContain(cardMsg.card.id);
  // A card it was never sent: refused.
  const unknown = await phone.ask({ type: 'approve', id: 999_999, allow: true, shownMs: 2000 }, 'error');
  expect(unknown.message).toMatch(/isn't waiting/);

  // Shown on the PC: the conversation open in Chat, its card in the thread.
  await openView(main, 'Chat');
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: { phase: 'open', conversationId: chat.id, show: true } });
  await expect(main.locator('.msgs').getByTestId('chat-agent-approve')).toBeVisible({ timeout: 15_000 });

  // A fresh tap: allowed, from the phone.
  await phone.page.waitForTimeout(800);
  const tapped = await phone.mark();
  await phone.send({ type: 'approve', id: cardMsg.card.id, allow: true, shownMs: 1500 });
  const done = await phone.wait('approve_done', { after: tapped });
  expect(done).toEqual({ type: 'approve_done', id: cardMsg.card.id, outcome: 'allowed', by: 'you' });
  // The PC's card closed saying who.
  await expect.poll(async () => (await heard(main, 'yap-claude-approve-done')).find((e) => e.id === cardMsg.card.id)).toMatchObject({ outcome: 'allowed', by: 'phone', byName: PHONE_NAME, byDevice: device.id });
  await expect(main.locator('.msgs').getByTestId('chat-agent-approve')).toHaveCount(0);
  await expect(main.locator('.msgs').getByTestId('chat-agent-approve-remote')).toHaveText(`Allowed from ${PHONE_NAME} · Run a command: npm test`);
  // A second answer: "Already allowed from …".
  await phone.page.waitForTimeout(100);
  const again = await phone.ask({ type: 'approve', id: cardMsg.card.id, allow: false, shownMs: 3000 }, 'error');
  expect(again.message).toBe(`Already allowed from ${PHONE_NAME}.`);
  // The watched agent's live events, numbered on its lane.
  const ev = await phone.wait('agent_event', { after: opened });
  expect(ev).toMatchObject({ id: agent.id, event: { id: agent.id } });
  expect(ev.seq).toBeGreaterThanOrEqual(1);
  expect(ev.line).toBeGreaterThan(0);

  // The agent finishes; its report is said on the phone, never on this PC.
  const finished = await ended(yap, agent.id);
  expect(finished.status).toBe('done');
  const report = await phone.wait('report', { after: asked, timeout: 30_000 });
  expect(report).toMatchObject({ conversation: chat.id, agent: agent.id, turn: 1, status: 'done' });
  const marker = await phone.wait('chat_message', { after: asked, pred: (m) => m.message.kind === 'agentReport' });
  expect(marker).toMatchObject({ conversation: chat.id, message: { role: 'user', kind: 'agentReport' } });
  await phone.wait('chat_message', { after: asked, pred: (m) => m.message.role === 'assistant' && m.message.text === REPORT });
  const voice = await phone.wait('speak_start', { after: asked, timeout: 40_000 });
  expect(voice.rate).toBe(24_000);
  await expect.poll(() => phone.frames(), { timeout: 20_000 }).toBeGreaterThan(0);
  await expect.poll(async () => (await spoken(yap)).filter((s) => /Draft to Final/.test(s.text)).map((s) => s.elsewhere), { timeout: 20_000 }).toEqual([true]);
  expect((await spoken(yap)).some((s) => /Draft to Final/.test(s.text) && !s.elsewhere)).toBe(false);

  // The agent's log says it, where it happened; the trail too, without the command.
  const { log } = await yap.invoke('agent_get', { id: agent.id });
  expect(log.find((e) => e.remote)).toMatchObject({ step: `Allowed from ${PHONE_NAME} · Run a command: npm test`, remote: { device: device.id, name: PHONE_NAME, action: 'approved' } });
  await expectStore(yap, 'remote_control.json', (store) => {
    const a = store.recent.find((r) => r.action === 'approved');
    expect(a).toMatchObject({ device: device.id, name: PHONE_NAME, agent: agent.id, conversation: chat.id, workspace: 'garden-planner', outcome: 'ok' });
    expect(JSON.stringify(store)).not.toContain('npm test');
    return true;
  });
  // Settings → Phone says it as the phone's last action.
  const dialog = await openSettings(main, 'Phone');
  await expect(dialog.getByTestId('link-phone-last').first()).toContainText('Allowed a step in garden-planner');
  await closeSettings(main);
  phone.chat = chat.id;
});

test("the phone's own page (wave 2): its Code side lists the workspace and the Code chat, whose thread has the agent's pill and report card, and the pill opens its live view", async ({ yap, browser }) => {
  test.setTimeout(60_000);
  const { port } = await status(yap);
  // The page as the PC serves it, on a phone-sized screen, paired as the client above.
  const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 390, height: 844 } });
  await context.addInitScript((d) => {
    if (!sessionStorage.getItem('seeded')) {
      localStorage.setItem('yap.link.device', JSON.stringify(d));
      sessionStorage.setItem('seeded', '1');
    }
  }, device);
  const page = await context.newPage();
  await page.goto(`https://127.0.0.1:${port}/`);
  await expect(page.getByTestId('conn')).toHaveText(/^Connected to /, { timeout: 15_000 });
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await expect(page.getByTestId('code-workspace').filter({ hasText: 'garden-planner' })).toBeVisible();
  const row = page.locator(`[data-testid="code-chat"][data-id="${phone.chat}"]`);
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByTestId('workspace-chip')).toHaveText('garden-planner');
  await expect(page.getByTestId('report-card').first().getByTestId('report-title')).toHaveText('Agent done · garden-planner');
  await expect(page.getByTestId('heard').first()).toHaveText('Change Draft to Final in notes.md and run the tests');
  await page.getByTestId('agent-pill').first().click();
  await expect(page.getByTestId('agent-view')).toBeVisible();
  await expect(page.getByTestId('agent-title')).toHaveText('garden-planner');
  await expect(page.getByTestId('agent-status')).toHaveText('Done');
  await expect(page.getByTestId('agent-file').first()).toContainText('notes.md');
  await expect(page.getByTestId('agent-cost')).toContainText('Your Claude plan');
  await page.getByTestId('agent-close').click();
  await expect(page.getByTestId('agent-view')).toBeHidden();
  await context.close();
});

test('a Bypass chat runs at Auto from the phone, which never picks Bypass; Stop from the phone', async ({ yap, main, fakeLlm }) => {
  test.setTimeout(90_000);
  const chat = phone.chat;
  await yap.invoke('agent_bypass_confirmed');
  await yap.invoke('agent_mode_set', { conversation: chat, mode: 'bypassPermissions' });
  // Never Bypass from the phone.
  const bypass = await phone.ask({ type: 'set_mode', chat, mode: 'bypassPermissions' }, 'error');
  expect(bypass.message).toBe('From your phone, agents run at most at Auto: Bypass permissions is for the PC only.');
  expect((await yap.invoke('agent_mode_get', { conversation: chat })).mode).toBe('bypassPermissions');
  // The phone opens the chat: its thread shows the Bypass Mode and Auto as the phone's highest.
  const thread = await phone.ask({ type: 'open_chat', id: chat }, 'thread');
  expect(thread.code).toMatchObject({ workspace: 'garden-planner', mode: 'bypassPermissions', phoneMax: 'auto' });
  expect(thread.messages.find((m) => m.kind === 'agentReport')).toBeTruthy();
  expect(thread.agents.length).toBeGreaterThan(0);

  // A follow-up from the phone: the same agent's next turn, at Auto though the chip says Bypass.
  await script(yap, SLOW, 'slow');
  const [agent] = (await agents(yap)).filter((a) => a.conversation === chat);
  fakeLlm.scriptChat([
    // Sized by the chat as on the PC (2026-11-08, agents/sizing.rs): its model and effort go; its Mode stays capped.
    { tools: [{ name: 'start_agent', args: { brief: 'Now add a watering schedule to notes.md.', model: 'claude-haiku-5-5', effort: 'low', sizing: 'a small edit' } }] },
    { content: "On it: I've passed that to the agent in garden-planner." },
  ]);
  const asked = await phone.mark();
  await phone.send({ type: 'ask', text: 'Now add a watering schedule to notes.md', chat });
  await phone.wait('answer', { after: asked, pred: (m) => m.final });
  await expect.poll(async () => (await agents(yap)).find((a) => a.id === agent.id)?.turnList?.length, { timeout: 15_000 }).toBe(2);
  await expect.poll(async () => (await agents(yap)).find((a) => a.id === agent.id)?.status, { timeout: 15_000 }).toBe('running');
  const now = (await agents(yap)).find((a) => a.id === agent.id);
  expect(now.mode).toBe('auto');
  expect(now.turnList.at(-1).mode).toBe('auto');
  expect(now.turnList.at(-1)).toMatchObject({ model: 'claude-haiku-5-5', effort: 'low', sizing: 'a small edit' });
  // Its turn is the phone's (the report goes there too).
  await expectStore(yap, 'remote_control.json', (store) => {
    expect(store.asked.some((m) => m.key === now.turnList.at(-1).messageId && m.device === device.id)).toBe(true);
    return true;
  });

  // A lower Mode from the phone: fine, said to every phone.
  const mode = await phone.ask({ type: 'set_mode', chat, mode: 'plan' }, 'mode');
  expect(mode).toMatchObject({ chat, mode: 'plan', label: 'Plan' });
  expect((await yap.invoke('agent_mode_get', { conversation: chat })).mode).toBe('plan');

  // Stop from the phone.
  const acted = await phone.ask({ type: 'agent_stop', id: agent.id }, 'acted');
  expect(acted).toMatchObject({ action: 'stop', id: agent.id, message: 'Stopping the agent in garden-planner.' });
  expect((await ended(yap, agent.id, 30_000)).status).toBe('stopped');
  await expectStore(yap, 'remote_control.json', (store) => {
    expect(store.recent.map((r) => r.action)).toEqual(expect.arrayContaining(['approved', 'mode', 'stopped']));
    expect(store.recent.find((r) => r.action === 'mode')).toMatchObject({ from: 'bypassPermissions', to: 'plan' });
    return true;
  });
  void main;
});

test('Remote Control off, or a phone not ticked: ready says why and every Code message is refused', async ({ yap, main, browser }) => {
  // Not ticked: told at once, and refused.
  const told = await phone.mark();
  await yap.invoke('link_phone_remote', { id: device.id, on: false });
  const notTicked = await phone.wait('ready', { after: told, pred: (m) => m.code && !m.code.on });
  expect(notTicked.code.why).toMatch(/^This phone isn't allowed Remote Control\. Tick it in Settings → Phone on /);
  const refused = await phone.ask({ type: 'code_list' }, 'error');
  expect(refused.message).toBe(notTicked.code.why);
  await yap.invoke('link_phone_remote', { id: device.id, on: true });
  await phone.wait('ready', { after: told, pred: (m) => m.code?.on === true });

  // The switch off: every phone told, everything refused.
  const off = await phone.mark();
  await setConfig(yap, main, { linkRemoteControl: false });
  const ready = await phone.wait('ready', { after: off, pred: (m) => m.code && !m.code.on });
  expect(ready.code.why).toMatch(/^Remote Control is off on .+\. Turn it on in Settings → Phone\.$/);
  for (const m of [
    { type: 'code_list' },
    { type: 'agent_open', id: (await agents(yap))[0].id },
    { type: 'set_mode', chat: phone.chat, mode: 'plan' },
    { type: 'approve', id: 1, allow: true, shownMs: 2000 },
    { type: 'code_new', workspace: 'garden-planner' },
  ]) {
    const e = await phone.ask(m, 'error');
    expect(e.message, m.type).toMatch(/Remote Control is off|isn't waiting/);
  }
  // A new connection says so in its ready, too.
  const { port } = await status(yap);
  const second = await phoneClient(browser, port);
  await second.connect({ type: 'hello', v: 1, name: PHONE_NAME, nonce: 'n-0002', device });
  expect((await second.wait('ready')).code).toMatchObject({ on: false });
  await second.close();
  // Its tick stayed (the switch is the master).
  await expectStore(yap, 'remote_control.json', (store) => {
    expect(store.phones[device.id].on).toBe(true);
    return true;
  });
  await phone.close();
  // The Environment card: the label on a Code chat's box opens it.
  await openView(main, 'Chat');
  await toSide(main, 'Code');
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: { phase: 'open', conversationId: phone.chat, show: true } });
  await main.getByTestId('chat-code-label').click();
  const env = main.getByTestId('code-environment');
  await expect(env).toBeVisible();
  await expect(env).toContainText('garden-planner');
  await expect(env.getByTestId('code-environment-remote')).toHaveAttribute('aria-checked', 'false');
});
