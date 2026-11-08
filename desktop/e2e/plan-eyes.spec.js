// The screen and the pages beside the chat for a chat on your Claude plan
// (desktop/src-tauri/src/integrations/claude_live/eyes.rs, chat/turn/plan.rs): the plan's own
// Claude Code calls Yap's look_at_screen and browser_* tools over Yap's run endpoint, and
// each call runs the tool loop's own code and rules (chat/see_screen.rs, chat/pages.rs) for the
// chat turn it belongs to. What it takes of the screen goes back to Claude Code as MCP image
// content; what it shows goes to the plan turn's stream, as the tool loop's does.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude <script>`, ai/claude_code/fake.rs),
// switched per test with the debug-only `e2e_claude_script`: the plan chat's session streams its
// tool calls as Claude Code does, then `_mcp` calls Yap's endpoint as Claude Code would, hands the
// answer back as its tool result (image content as image blocks, as 2.1.296 does) and, with
// `dump`, writes what it got to a file this spec reads. The monitors are Yap's test-mode stand-ins
// (`e2e_snip`), the web a local fake (support/fake-web.js): nothing of the real screen, the
// person's Claude Code or the internet. Fictional pages only.
//
// - Typed, the screen: a cloud model, so asked once (the model says it couldn't look; Allow
//   lets it), then the screenshots show with the question and Claude Code gets the pictures.
// - The pages: a page Claude Code's WebSearch found opens in the panel's Page tab, and
//   the model scrolls it and looks: its shots go back as image content and show in the turn's steps,
//   kept with the answer. A click that buys waits on the person's card, as in the tool loop.
// - A Code chat on the plan: the same look, its tools allowed in its session; a picture an agent
//   made in its workspace shown in a picture tab by its path (a file is never loaded as a page).
// - During a call: refused, nothing taken.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import utilsBundle from 'playwright-core/lib/utilsBundle';
import { test as base, expect, expectStore, openView, setConfig } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { startFakeWeb, FORT_JOY_URL } from './support/fake-web.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat } from './support/code-chats.js';

const HERE = path.join(RUNS, 'plan-eyes-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';

// ---- the stand-in's script ----

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const CHAT_INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['WebFetch', 'WebSearch', 'mcp__yap__look_at_screen', 'mcp__yap__browser_navigate', 'mcp__yap__browser_screenshot', 'mcp__yap__browser_click'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: MODEL,
  permissionMode: 'dontAsk',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
const whole = (id, content, stop) => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: MODEL, content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: SID,
});
const result = (text) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: SID,
  total_cost_usd: 0.003,
  usage: { input_tokens: 5200, output_tokens: 30, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** The model calls `name` in message `msg`, streamed (its start first) then whole, as Claude Code writes it. */
const calls = (msg, id, name, input) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id, name, input: {} } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'tool_use', id, name, input }], 'tool_use'),
  ev({ type: 'message_stop' }),
];
/** …one of Yap's: its call, Yap's endpoint called with it (what it gave written to `dump`), the answer its tool result. */
const yapCall = (msg, id, tool, input, dump) => [...calls(msg, id, `mcp__yap__${tool}`, input), { _mcp: { tool, arguments: input, result_for: id, dump } }];
/** A web search that finds the wiki's page. */
const searches = (msg) => [
  ...calls(msg, 'toolu_search', 'WebSearch', { query: 'fort joy treasure map' }),
  {
    type: 'user',
    message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'toolu_search', content: `Web search results for query: "fort joy treasure map"\n\nLinks: [{"title":"Fort Joy - Example Wiki","url":"${FORT_JOY_URL}"}]` }] },
    parent_tool_use_id: null,
    session_id: SID,
    tool_use_result: { query: 'fort joy treasure map', results: [{ tool_use_id: 'srvtoolu_e2e', content: [{ title: 'Fort Joy - Example Wiki', url: FORT_JOY_URL }] }], durationSeconds: 0.3 },
  },
];
/** The answer, streamed then whole, and the turn's result. */
const answers = (msg, text) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'text', text }], 'end_turn'),
  ev({ type: 'message_stop' }),
  result(text),
];
/** One plan chat turn: the question, then `lines`. */
const turn = (...lines) => [{ _wait_user: true }, CHAT_INIT, ...lines.flat()];

/** A script of plan chat turns. */
function script(name, chatTurns) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  const lines = [{ _for: 'chat' }, ...chatTurns.flat(), { _wait_user: true }];
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

/** A fresh file for the stand-in to write its MCP calls (or arguments) to. */
function fresh(name) {
  fs.mkdirSync(HERE, { recursive: true });
  const file = path.join(HERE, name);
  fs.rmSync(file, { force: true });
  return file;
}
const linesOf = (file) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const imagesIn = (call) => call.content.filter((b) => b.type === 'image');
const resultOf = (call) => JSON.parse(call.content[0].text);

// ---- the app ----

const test = base.extend({
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeWeb }, use) => {
      await use({
        name: 'plan-eyes',
        env: { ...fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl')), ...fakeWeb.env, YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com` },
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false, webSearch: true, assistantScreen: true, assistantScreenCloud: false, assistantScreenAsked: false }),
      });
    },
    { scope: 'worker' },
  ],
});

const MONITORS = [
  { x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 },
  { x: 1280, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 },
];

const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const screenshotsOfLast = (main) => main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ });
const play = (yap, name, chatTurns) => yap.invoke('e2e_claude_script', { script: script(name, chatTurns) });

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  // (A page left open by an earlier test: the panel closed, so the chat list shows.)
  const closePanel = main.getByTestId('chat-page').getByRole('button', { name: 'Close the panel' });
  if (await closePanel.count()) await closePanel.click();
  const cfg = await main.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_config'));
  if (cfg?.agentsEnabled) await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** "Let Yap's AI see your screen?": the main window's toast, or the bar's card. */
async function askCard(yap, main) {
  const title = "Let Yap's AI see your screen?";
  const toast = main.locator('.toast').filter({ hasText: title });
  const card = yap.overlay.getByRole('status').filter({ hasText: title });
  await expect.poll(async () => (await toast.count()) + (await card.count()), { timeout: 15_000 }).toBeGreaterThan(0);
  return (await toast.count()) ? toast.first() : card.first();
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, webSearch: true, assistantScreen: true });
});

test('a plan chat asks to look at the screen: asked once, then the screenshots show with the question and Claude Code gets the pictures', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { assistantScreenCloud: false, assistantScreenAsked: false });
  await yap.invoke('e2e_snip', { monitors: MONITORS, windows: [], start: false });
  const DUMP = fresh('look.jsonl');
  const NOT_YET = "I can't see your screen yet: you can let me in Settings → Voice Assistant.";
  const LOOKED = 'Those are the speakers on your second monitor: the left one has a 3.5mm input.';
  await play(yap, 'look', [
    turn(yapCall('m1', 'toolu_look1', 'look_at_screen', {}, DUMP), answers('m2', NOT_YET)),
    turn(yapCall('m3', 'toolu_look2', 'look_at_screen', {}, DUMP), answers('m4', LOOKED)),
  ]);
  await freshChat(main);
  await send(main, 'Here are the speakers, can you have a look at my screen?');

  // The plan is a cloud model: not without asking. The model is told it couldn't look; the
  // alert asks once (see_screen.rs, as for any cloud model); nothing was taken.
  await expect(lastAnswer(main)).toHaveText(NOT_YET, { timeout: 20_000 });
  await expect(chipsOfLast(main)).toHaveText(["Didn't look at your screen: not to a cloud model yet"]);
  await expect.poll(() => linesOf(DUMP).length).toBe(1);
  const refused = linesOf(DUMP)[0];
  expect(refused).toMatchObject({ tool: 'look_at_screen', isError: true });
  expect(imagesIn(refused)).toEqual([]);
  expect(resultOf(refused).error).toContain("haven't let Yap send their screen to");
  const card = await askCard(yap, main);
  await expect(card).toContainText('a screenshot would go there');
  expect((await yap.invoke('get_config')).assistantScreenAsked).toBe(true);
  await shot(main, '01-plan-chat-asks-before-the-screen');
  await card.getByRole('button', { name: 'Allow' }).click();
  await expect.poll(async () => (await yap.invoke('get_config')).assistantScreenCloud).toBe(true);

  // Allowed: both monitors taken (Yap's windows left out), shown with the question in the
  // screenshot frame and kept with it, and the pictures go to Claude Code as image content.
  await send(main, 'Have a look now');
  await expect(lastAnswer(main)).toHaveText(LOOKED, { timeout: 20_000 });
  await expect(chipsOfLast(main)).toHaveText(['Looked at your screen (2 monitors)']);
  await expect(screenshotsOfLast(main)).toHaveCount(2);
  const looked = linesOf(DUMP)[1];
  expect(looked).toMatchObject({ tool: 'look_at_screen', isError: false });
  expect(resultOf(looked)).toMatchObject({ success: true, data: { monitors: 2 }, displayText: 'Looked at your screen (2 monitors)' });
  expect(resultOf(looked).note).toContain('follow in this result');
  const pictures = imagesIn(looked);
  expect(pictures).toHaveLength(2);
  for (const p of pictures) {
    expect(p.mimeType).toBe('image/png');
    expect(p.bytes).toBeGreaterThan(100);
  }
  // Each picture after the line saying which it is.
  const lines = looked.content.filter((b) => b.type === 'text').map((b) => b.text);
  expect(lines[1]).toContain('Monitor 1 of 2 as it is now');
  expect(lines[2]).toContain('Monitor 2 of 2 as it is now');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text === 'Have a look now' && m.images?.filter((i) => i.source === 'screen').length === 2))
  );
  await shot(main, '02-plan-chat-looked-at-the-screen');
  await theme(yap, main, 'dark');
  await shot(main, '03-plan-chat-looked-at-the-screen-dark');
  await theme(yap, main, 'light');
});

test('a plan chat opens the page its search found, scrolls it and looks: the shots come back as image content and show in its steps, never a tab', async ({ yap, main, shot }) => {
  const DUMP = fresh('pages.jsonl');
  const SAID = 'The Fort Joy map is open beside the chat; I looked at its map section.';
  await play(yap, 'pages', [
    turn(
      searches('m1'),
      yapCall('m2', 'toolu_open', 'browser_navigate', { url: FORT_JOY_URL }, DUMP),
      yapCall('m3', 'toolu_to', 'browser_scroll_to', { text: 'Map' }, DUMP),
      yapCall('m4', 'toolu_snap', 'browser_screenshot', {}, DUMP),
      yapCall('m5', 'toolu_zoom', 'browser_zoom', { region: 'top-left' }, DUMP),
      answers('m6', SAID)
    ),
  ]);
  await freshChat(main);
  // (Worded so Yap leaves it to the model: "show me <address>" it would open itself.)
  await send(main, 'Find the Fort Joy treasure map and bring it up beside the chat with a screenshot');

  // The page Claude Code's own WebSearch found: brought up in a Page tab, as the tool loop does.
  await expect(main.getByTestId('chat-page')).toBeVisible({ timeout: 30_000 });
  await expect(main.getByTestId('chat-page-address')).toHaveValue(FORT_JOY_URL, { timeout: 20_000 });
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 40_000 });
  await expect(chipsOfLast(main)).toHaveText([
    'Searched the web: "fort joy treasure map"',
    'Brought up wiki.example.com/wiki/Fort_Joy beside the chat',
    'Scrolled to ‘Map’ on wiki.example.com',
    'Looked at the page · wiki.example.com',
    'Zoomed in on the page · wiki.example.com',
  ]);
  const [opened, scrolled, looked, zoomed] = linesOf(DUMP);
  expect(resultOf(opened)).toMatchObject({ success: true, shown: true });
  expect(resultOf(scrolled)).toMatchObject({ success: true, data: { kind: 'heading' } });
  // The shot and the zoom go back to Claude Code as image content, after the line saying what each is.
  expect(resultOf(looked)).toMatchObject({ success: true, data: { picture: 'p1' } });
  expect(resultOf(looked).note).toContain("The page's pictures follow in this result");
  expect(imagesIn(looked)).toHaveLength(1);
  expect(looked.content[1].text).toContain('as it\'s in view now');
  expect(imagesIn(zoomed)).toHaveLength(1);
  // Never a picture tab or a pill: the shots are the turn's steps', kept with the answer.
  await expect(main.getByTestId('chat-picture')).toHaveCount(0);
  await expect(main.getByTestId('chat-picture-pill')).toHaveCount(0);
  const steps = thread(main).getByTestId('chat-steps').last();
  await expect(steps.getByTestId('chat-steps-shots').getByTestId('chat-step-shot')).toHaveCount(2);
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) =>
      c.messages.some((m) => m.role === 'assistant' && m.text === SAID && m.pages?.[0]?.url === FORT_JOY_URL && m.pictures?.length === 2 && m.pictures.every((p) => p.chip))
    )
  );
  await steps.locator('summary').click();
  await expect(steps.locator('.toolchips').getByTestId('chat-step-shot')).toHaveCount(2);
  await shot(main, '04-plan-chat-page-and-its-shots-in-the-steps');
  await theme(yap, main, 'dark');
  await shot(main, '05-plan-chat-page-and-its-shots-in-the-steps-dark');
  await theme(yap, main, 'light');
});

test('a plan chat acts on a page with the same card: a click that buys waits for the person, a no stops it', async ({ yap, main, shot }) => {
  const DUMP = fresh('act.jsonl');
  const SAID = 'I didn’t buy it: you said no. You can buy it yourself in the panel.';
  const results = 'https://rail.example.com/results?from=Kellbrook&to=Port+Ellis&when=Today';
  await play(yap, 'act', [
    turn(
      yapCall('m1', 'toolu_open', 'browser_navigate', { url: results }, DUMP),
      yapCall('m2', 'toolu_find', 'browser_find', { query: 'Buy tickets' }, DUMP),
      yapCall('m3', 'toolu_click', 'browser_click', { ref: 1 }, DUMP),
      answers('m4', SAID)
    ),
  ]);
  await freshChat(main);
  await send(main, `Buy me the 09:42 ticket on ${results}`);
  // The card, in the plan turn's thread: the call waits for the person.
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 40_000 });
  await expect(card).toContainText('Yap wants to press ‘Buy tickets’ on rail.example.com');
  await shot(main, '06-plan-chat-asks-before-buying');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 30_000 });
  await expect(card).toHaveAttribute('data-state', 'refused');
  const clicked = linesOf(DUMP)[2];
  expect(clicked).toMatchObject({ tool: 'browser_click', isError: true });
  expect(resultOf(clicked)).toMatchObject({ success: false, displayText: "Didn't click ‘Buy tickets’: you said no", data: { asked: true, allowed: false } });
  await expect(chipsOfLast(main)).toHaveText([
    /Brought up rail\.example\.com\/results.* beside the chat/,
    /Found \d+ matches for ‘Buy tickets’ on rail\.example\.com/,
    "Didn't click ‘Buy tickets’: you said no",
  ]);
  // Nothing bought: the page is still the results.
  const page = (await yap.invoke('e2e_page_views')).find((v) => v.url.startsWith('https://rail.example.com/results'));
  expect(JSON.parse(await yap.invoke('e2e_page_view_eval', { key: page.key, script: 'location.pathname' }))).toMatch(/\/results$/);
});

test('a Code chat on the plan looks at the screen too: its session allows the tools, the screenshots show with the question', async ({ yap, main, shot }) => {
  fs.rmSync(GARDEN, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'style.css'), 'body { color: #333; }\n');
  await setConfig(yap, main, { agentsEnabled: true, assistantScreenCloud: true });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
  await yap.invoke('e2e_snip', { monitors: [MONITORS[0]], windows: [], start: false });
  const DUMP = fresh('code-look.jsonl');
  const ARGS = fresh('code-args.jsonl');
  const SAID = 'The header on your screen uses the grey from style.css.';
  await play(yap, 'code-look', [[{ _wait_user: true }, { _dump_args: ARGS }, CHAT_INIT, ...yapCall('m1', 'toolu_look', 'look_at_screen', {}, DUMP), ...answers('m2', SAID)]]);
  await openView(main, 'Chat');
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Take a screenshot of my screen and tell me where the header colour comes from');
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  await expect(chipsOfLast(main)).toHaveText(['Looked at your screen']);
  await expect(screenshotsOfLast(main)).toHaveCount(1);
  const looked = linesOf(DUMP)[0];
  expect(looked).toMatchObject({ tool: 'look_at_screen', isError: false });
  expect(imagesIn(looked)).toHaveLength(1);
  // The Code chat's session: in its workspace, the screen's and the pages' tools allowed.
  const [launch] = linesOf(ARGS);
  const allowed = launch.args[launch.args.indexOf('--allowedTools') + 1] ?? '';
  for (const name of ['look_at_screen', 'browser_navigate', 'browser_read', 'browser_tabs', 'browser_screenshot', 'browser_click', 'show_picture']) {
    expect(allowed).toContain(`mcp__yap__${name}`);
  }
  expect(allowed).not.toContain('mcp__yap__open_page');
  expect(path.resolve(launch.cwd).toLowerCase()).toBe(path.resolve(GARDEN).toLowerCase());
  await shot(main, '06-code-chat-looked-at-the-screen');
  // Its warm session works in the workspace (Windows keeps a folder a process works in): the
  // chat deleted ends it, so a later fresh folder can go.
  const conversation = (await yap.invoke('chats_list'))[0].id;
  await yap.invoke('chat_delete', { id: conversation });
});

/** A `w` × `h` "concept" picture (a garden bed on a sky), as PNG bytes: Playwright's own pngjs. */
function conceptPng(w, h) {
  const p = new utilsBundle.PNG({ width: w, height: h });
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      const bed = x > w * 0.15 && x < w * 0.85 && y > h * 0.55 && y < h * 0.85;
      const row = bed && Math.floor((x - w * 0.15) / (w * 0.07)) % 2 === 0;
      const [r, g, b] = bed ? (row ? [74, 128, 62] : [120, 86, 52]) : [150 + Math.round((60 * y) / h), 196, 232];
      p.data.set([r, g, b, 255], i);
    }
  }
  return utilsBundle.PNG.sync.write(p);
}

test('a Code chat on the plan shows a picture an agent made in its workspace: a file is never a page, show_picture opens it in a picture tab', async ({ yap, main, shot }) => {
  fs.rmSync(GARDEN, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  const DOCS = path.join(GARDEN, 'docs');
  fs.mkdirSync(DOCS, { recursive: true });
  const CONCEPT = path.join(DOCS, 'concept.png');
  fs.writeFileSync(CONCEPT, conceptPng(480, 320));
  fs.writeFileSync(path.join(DOCS, 'bed-layout.html'), '<!doctype html><title>Bed layout</title><p>Raised beds</p>\n');
  await setConfig(yap, main, { agentsEnabled: true });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
  const DUMP = fresh('code-picture.jsonl');
  const SAID = 'Here is the concept picture beside the chat. The HTML version only opens in your own browser.';
  await play(yap, 'code-picture', [
    turn(
      yapCall('m1', 'toolu_html', 'browser_navigate', { url: pathToFileURL(path.join(DOCS, 'bed-layout.html')).href }, DUMP),
      yapCall('m2', 'toolu_file', 'browser_navigate', { url: pathToFileURL(CONCEPT).href }, DUMP),
      yapCall('m3', 'toolu_show', 'show_picture', { picture: CONCEPT, caption: 'Garden concept' }, DUMP),
      answers('m4', SAID)
    ),
  ]);
  await openView(main, 'Chat');
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Show me the concept pictures you made');
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 30_000 });
  // A file is never loaded as a page: the model is told how to show a picture instead
  // (a web page saved here isn't opened at all), then show_picture opens it.
  await expect(chipsOfLast(main)).toHaveText([
    /Didn't (bring up|go to) bed-layout\.html: a file on this PC$/,
    /Didn't (bring up|go to) concept\.png: a file on this PC$/,
    'Showed ‘concept.png’ from docs',
  ]);
  const [html, file, showed] = linesOf(DUMP);
  expect(html).toMatchObject({ tool: 'browser_navigate', isError: true });
  expect(resultOf(html).error).toContain('(HTML)');
  expect(file).toMatchObject({ tool: 'browser_navigate', isError: true });
  expect(resultOf(file).error).toContain('show_picture');
  expect(showed).toMatchObject({ tool: 'show_picture', isError: false });
  expect(resultOf(showed)).toMatchObject({ success: true, shown: true, w: 480, h: 320 });
  expect((await yap.invoke('e2e_page_views')).some((v) => !String(v.url).startsWith('http'))).toBe(false);

  // The picture tab: named by the file, its folder where it's from, the picture at its device pixels.
  const tab = main.getByTestId('chat-picture');
  await expect(tab).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('concept.png');
  await expect.poll(async () => (await main.getByTestId('chat-picture-source').textContent()).trim().toLowerCase()).toBe(`from ${DOCS}`.toLowerCase());
  await expect(main.getByTestId('chat-picture-open')).toHaveCount(0);
  await expect
    .poll(() => main.getByTestId('chat-picture-img').evaluate((img) => img.complete && img.naturalWidth > 0), { timeout: 10_000 })
    .toBe(true);
  await expect(main.getByTestId('chat-picture-pill')).toHaveCount(1);
  // Kept with the answer, stored like a pasted picture.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) =>
      c.messages.some((m) => m.role === 'assistant' && m.text === SAID && m.pictures?.length === 1 && m.pictures[0].caption === 'concept.png' && m.pictures[0].page?.site === 'docs' && !m.pictures[0].imageUrl)
    )
  );
  const id = resultOf(showed).id;
  expect(fs.existsSync(path.join(yap.dataDir, 'chat-images', `${id}.png`))).toBe(true);
  await shot(main, '08-code-chat-shows-a-picture-from-its-workspace');
  await theme(yap, main, 'dark');
  await shot(main, '09-code-chat-shows-a-picture-from-its-workspace-dark');
  await theme(yap, main, 'light');
  // Its warm session works in the workspace: the chat deleted ends it.
  const conversation = (await yap.invoke('chats_list'))[0].id;
  await yap.invoke('chat_delete', { id: conversation });
});

test('during a call a plan chat never looks at the screen: refused before anything is asked or taken', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { assistantScreenCloud: true });
  await yap.invoke('e2e_snip', { monitors: MONITORS, windows: [], start: false });
  const DUMP = fresh('call.jsonl');
  const SAID = "I can't look at your screen during a call.";
  await play(yap, 'call', [turn(yapCall('m1', 'toolu_look', 'look_at_screen', {}, DUMP), answers('m2', SAID))]);
  await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: true });
  try {
    await freshChat(main);
    // (Sent with Enter: the call's own card, "Teams call detected", sits over Send.)
    await main.getByRole('textbox', { name: 'Message' }).fill('Have a look at my screen');
    await main.getByRole('textbox', { name: 'Message' }).press('Enter');
    await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
    await expect(chipsOfLast(main)).toHaveText(["Didn't look at your screen: a meeting or a call"]);
    const [refused] = linesOf(DUMP);
    expect(refused).toMatchObject({ tool: 'look_at_screen', isError: true });
    expect(imagesIn(refused)).toEqual([]);
    await expect(main.getByRole('group', { name: 'Screenshots' })).toHaveCount(0);
    await shot(main, '07-plan-chat-no-look-during-a-call');
  } finally {
    await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: false });
  }
});
