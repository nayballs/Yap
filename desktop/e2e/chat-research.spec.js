// Research (desktop/src-tauri/src/chat/research.rs, chat/turn/research.rs, 16 Nov 2026; the Claude
// app's Research mode as Anthropic's help pages describe it):
// - The Research switch under the message box, beside Style (ResearchChip.svelte): on, a question is
//   researched; greyed, with why, when web search is off.
// - The plan first: a call with no tools writes it, and it's the outline card's research plan in the
//   thread ("Research plan", Start research / Edit); nothing is searched until the person answers.
// - The loop on Yap's free web sources (no search key: the stand-in's Wikipedia): searches and page
//   reads, the progress on the activity line ("… · 1 search, 0 pages read"); a tool that isn't the
//   web's refused (an app, memory), the report's words never in the thread.
// - The report: a document beside the chat, its citations only to the pages read (one only found in
//   the results goes), numbered 1, 2 as first cited, each a link to its page, a numbered Sources list;
//   in the thread the summary with its pills, the document's card and the research's chip.
// - ■ Stop mid-research: "Research stopped…", no document. A plain "research …" without the switch
//   researches too; a greeting with it on is answered as ever (the plan step says it's not research).
// - Retry and Edit of a question researched only because the switch was on (16 Nov 2026): researched again, the
//   plan first, though the switch is off by then (the question keeps it: `research` in chats.json); one asked with
//   it off is answered again as ever.
// - On your Claude plan, a page Claude Code's WebFetch read that no search found (16 Nov 2026: it showed its address)
//   is titled with its own <title>, read on this PC, in the Sources row and the report's list; a word to the person
//   before the report's # title goes, the title the report's own.
// The model is a local fake (support/fake-llm.js) scripted call by call — on the plan, Claude Code is this build's
// stand-in (`yap.exe fake-claude`, support/claude-plan.js); the web a local fake (support/fake-web.js). Nothing
// goes on the internet. Light and dark.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, QWERTY_URL, SHOLES_URL, DVORAK_URL } from './support/fake-web.js';
import { fakeClaude, PLAN_CHAT } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { endingOf } from '../src/lib/views/chat/outlineEdit.js';
import { stepsSummary } from '../src/lib/views/chat/chatSteps.js';

const HERE = path.join(RUNS, 'chat-research-spec');
/** A script for the stand-in Claude Code (ai/claude_code/fake.rs), one JSON line each. */
function claudeScript(name, lines) {
  fs.mkdirSync(HERE, { recursive: true });
  const file = path.join(HERE, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // The fake says its context, as a real server does (the web's whole toolset doesn't fit the 8K
      // Yap assumes for a server on this PC that doesn't say).
      fake.setServerModels([{ id: 'fake-chat' }]);
      fake.serveOllama({ loaded: { 'fake-chat': 32768 } });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, fakeWeb }, use) => {
      await use({
        name: 'chat-research',
        // (The plan's test loads its own script: e2e_claude_script.)
        env: { ...fakeWeb.env, ...fakeClaude(claudeScript('idle', [{ _wait_user: true }])) },
        config: {
          // No search key: Yap's free sources (web/sources.rs), the stand-in's Wikipedia.
          webSearch: true,
          openApps: true,
          memoryLearnChats: false,
          meetingReminder: 'never',
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          llmScopes: { chat: fakeChat(fakeLlm) },
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** Chat on the fake model. */
function fakeChat(fakeLlm) {
  return { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' };
}

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chip = (main) => main.getByTestId('chat-research-chip');
const card = (main) => thread(main).getByTestId('chat-outline-card').last();
const panel = (main) => main.getByTestId('chat-doc');
const line = (main) => main.getByTestId('chat-activity');

async function freshChat(main) {
  await openView(main, 'Chat');
  // (A report left open beside the chat: the panel closed, so the chat list shows.)
  const closeDoc = main.getByTestId('chat-doc').getByRole('button', { name: 'Close the document' });
  if (await closeDoc.count()) await closeDoc.first().click();
  const showChats = main.getByRole('button', { name: 'Show your chats' });
  if (await showChats.isVisible()) await showChats.click();
  await main.getByRole('button', { name: 'New chat' }).click();
}
async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}
function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}
/** Switch Research on (or off). */
async function research(main, on) {
  if ((await chip(main).getAttribute('aria-pressed')) !== String(on)) await chip(main).click();
  await expect(chip(main)).toHaveAttribute('aria-pressed', String(on));
}

/** The number Yap gave a page this turn, from the tool results the model read. */
function numberOf(record, url) {
  for (const m of record.messages) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (r?.data?.url === url && r.data.n) return r.data.n;
      const hit = (r?.data?.results ?? []).find((x) => x.url === url);
      if (hit) return hit.n;
    } catch {
      /* not JSON */
    }
  }
  return 99;
}

/** The fake model's report: the summary and the parts, citing the two pages read and one only found. */
function reportOf(record) {
  const [q, s, d] = [numberOf(record, QWERTY_URL), numberOf(record, SHOLES_URL), numberOf(record, DVORAK_URL)];
  return [
    '# How QWERTY came about',
    '',
    '## Summary',
    '',
    `QWERTY was laid out for the Sholes and Glidden typewriter [${s}] and sold to Remington in 1873 [${q}].`,
    '',
    '## Origins',
    '',
    `Sholes patented the arrangement in 1878 [${s}, ${q}].`,
    '',
    '## Why it stayed',
    '',
    `It came with the first typewriter that sold well [${s}].`,
    '',
    '## Rivals',
    '',
    `The Dvorak layout came in 1936 [${d}].`,
    '',
    '## Sources',
    '',
    '1. Wikipedia',
  ].join('\n');
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('researchPlan', null);
  await setConfig(yap, main, { theme: 'light', webSearch: true });
});

// ---------------------------------------------------------------- no app

test("the research plan's card and chips say research, not writing", () => {
  expect(endingOf('allowed', { research: true })).toBe('Researched as planned');
  expect(endingOf('changed', { research: true })).toBe('Researched with your changes');
  expect(endingOf('refused', { research: true })).toBe('Not researched');
  expect(endingOf('allowed', {})).toBe('Written as outlined');
  expect(stepsSummary(['Searched the web: "qwerty"', 'Read en.wikipedia.org/wiki/QWERTY', 'Researched for under a minute: 1 search, 1 page read', 'Wrote the report “QWERTY”'])).toBe(
    'Searched the web, read a page, researched it, wrote the report'
  );
  expect(stepsSummary(['Didn\'t search for "x": research limit: 20 searches', 'Stopped researching: research limit: 20 searches'])).toBe(
    "Stopped at the research limit, 1 didn't go through"
  );
});

// ---------------------------------------------------------------- in Chat

test('Research: the plan first, then searches and page reads on the free sources, then a report beside the chat citing only the pages read', async ({ yap, main, fakeLlm, fakeWeb, shot }) => {
  const midway = gate();
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: 'QWERTY keyboard layout history' } }] },
    { tools: [{ name: 'read_page', args: { url: QWERTY_URL } }, { name: 'web_search', args: { query: 'Sholes and Glidden typewriter keyboard' } }], gate: midway.promise },
    // An app and memory, asked for in a research turn: refused (the web came in too).
    { tools: [{ name: 'read_page', args: { url: SHOLES_URL } }, { name: 'open_app', args: { name: 'Calculator' } }, { name: 'remember', args: { text: 'likes typewriters' } }] },
    { content: (r) => reportOf(r) },
  ]);
  await freshChat(main);
  await research(main, true);
  await shot(main, '01-research-chip-on-light');
  const before = fakeLlm.requests.length;
  const searchesBefore = fakeWeb.requests.length;
  await ask(main, 'How did the QWERTY keyboard layout come about?');

  // The plan, before anything's looked up.
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card(main)).toHaveAttribute('data-research', 'true');
  await expect(card(main).locator('.otitle')).toContainText('Research plan · How did the QWERTY keyboard layout come about');
  await expect(card(main).locator('.okey')).toHaveText(['Origins', 'Why it stayed', 'Rivals']);
  await expect(card(main).getByRole('button', { name: 'Start research' })).toBeVisible();
  await expect(line(main)).toContainText('Waiting for you: the research plan');
  const planCall = fakeLlm.requests.slice(before).find((r) => r.kind === 'researchPlan');
  expect(planCall.tools).toEqual([]);
  expect(planCall.user).toContain("The user's message: How did the QWERTY keyboard layout come about?");
  expect(fakeLlm.requests.slice(before).filter((r) => r.tools.includes('web_search'))).toHaveLength(0);
  expect(fakeWeb.requests.slice(searchesBefore).filter((r) => r.host?.endsWith('wikipedia.org'))).toHaveLength(0);
  await shot(main, '02-research-plan-light');
  await theme(yap, main, 'dark');
  await shot(main, '03-research-plan-dark');
  await theme(yap, main, 'light');

  // Start: the loop, its progress on the line.
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.gated).length, { timeout: 20_000 }).toBe(1);
  await expect(line(main)).toContainText('1 search, 0 pages read');
  await shot(main, '04-researching-light');
  const loop = fakeLlm.requests.slice(before).find((r) => r.tools.includes('web_search'));
  expect(loop.user).toContain('This is a research turn');
  expect(loop.user).toContain('1. Origins — where it came from');
  expect(loop.user).toContain('up to 20 searches (web_search)');
  midway.open();

  // The report: beside the chat, its citations only to the pages read.
  await expect(lastAnswer(main)).toContainText('The full report is open on the right.', { timeout: 30_000 });
  await expect(card(main)).toHaveAttribute('data-state', 'allowed');
  await expect(card(main)).toContainText('Researched as planned');
  await expect(panel(main)).toBeVisible();
  const body = panel(main).locator('.docbody');
  await expect(body.locator('.docname')).toHaveText('How QWERTY came about');
  await expect(body).toContainText('Researched');
  await expect(body).toContainText('2 searches, 2 pages read');
  await expect(body).toContainText('The Dvorak layout came in 1936.');
  const links = await body.locator('a').evaluateAll((as) => as.map((a) => [a.textContent.trim(), a.getAttribute('href')]));
  // Numbered as first cited: Sholes 1, QWERTY 2; the Dvorak page (only found) never.
  expect(links).toContainEqual(['[1]', SHOLES_URL]);
  expect(links).toContainEqual(['[2]', QWERTY_URL]);
  expect(links.some(([, href]) => href === DVORAK_URL)).toBe(false);
  await expect(body.locator('li').filter({ hasText: 'Sholes and Glidden typewriter' })).toHaveCount(1);
  await expect(body.locator('li').filter({ hasText: 'QWERTY' })).toHaveCount(1);
  // The thread: the summary and its two pills, the document's card, the research's chip.
  await expect(lastAnswer(main)).toContainText('QWERTY was laid out for the Sholes and Glidden typewriter');
  await expect(lastAnswer(main).locator('a.md-cite')).toHaveCount(2);
  await expect(thread(main).getByTestId('chat-doc-pill').last()).toContainText('How QWERTY came about');
  const sent = fakeLlm.requests.slice(before);
  // An app and memory in a research turn: refused, the model told why.
  const after = sent.find((r) => r.messages.some((m) => m.role === 'tool' && String(m.content).includes("open_app doesn't run in a research turn")));
  expect(after).toBeTruthy();
  expect(after.messages.some((m) => m.role === 'tool' && String(m.content).includes("remember doesn't run in a research turn"))).toBe(true);
  // The report's words never streamed into the thread.
  await expect(thread(main)).not.toContainText('## Origins');
  // Kept: the document is a note in Chat documents, its body as the panel shows it.
  const note = (yap.readJson('notes.json')?.notes ?? []).find((n) => n.title === 'How QWERTY came about');
  expect(note.folder).toBe('Chat documents');
  expect(note.content).toContain(`[[1]](${SHOLES_URL})`);
  expect(note.content).toContain('## Sources');
  expect(note.content).not.toContain(DVORAK_URL);
  await shot(main, '05-research-report-light');
  await theme(yap, main, 'dark');
  await shot(main, '06-research-report-dark');
  await theme(yap, main, 'light');
});

test('■ Stop mid-research: no report, and a plain "research …" researches without the switch', async ({ main, fakeLlm }) => {
  const held = gate();
  fakeLlm.scriptChat([{ tools: [{ name: 'web_search', args: { query: 'typewriter history' } }] }, { tools: [{ name: 'read_page', args: { url: QWERTY_URL } }], gate: held.promise }]);
  await freshChat(main);
  await research(main, false);
  await ask(main, 'research the history of the typewriter keyboard');
  await expect(card(main)).toHaveAttribute('data-research', 'true', { timeout: 20_000 });
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect.poll(() => fakeLlm.requests.filter((r) => r.gated).length, { timeout: 20_000 }).toBeGreaterThan(0);
  await main.getByRole('button', { name: 'Stop the answer' }).click();
  await expect(lastAnswer(main)).toContainText('Research stopped before the report was written.', { timeout: 20_000 });
  await expect(panel(main)).toHaveCount(0);
  held.open();
});

test('a greeting with Research on is answered as ever; with web search off the switch says why', async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('researchPlan', () => '{"research": false}');
  fakeLlm.scriptChat([{ content: 'Hello! What shall we look into?' }]);
  await freshChat(main);
  await research(main, true);
  await ask(main, 'Hi there');
  await expect(lastAnswer(main)).toContainText('Hello! What shall we look into?', { timeout: 20_000 });
  await expect(thread(main).getByTestId('chat-outline-card')).toHaveCount(0);
  // Web search off: the switch can't, and says why.
  await setConfig(yap, main, { webSearch: false });
  await chip(main).click();
  await expect(chip(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(chip(main)).toHaveAttribute('title', /turn on web search/);
  await setConfig(yap, main, { webSearch: true });
});

/** A research loop on the fake: one search, then the report titled `title`. */
const loopOf = (title) => [
  { tools: [{ name: 'web_search', args: { query: 'QWERTY keyboard layout history' } }] },
  { content: `# ${title}\n\n## Summary\n\nQWERTY came with the Sholes and Glidden typewriter.\n\n## Origins\n\nIt was sold to Remington in 1873.` },
];
const questions = (main) => thread(main).locator('.cbubble.user');
const plans = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.kind === 'researchPlan');
/** The newest conversation whose title starts so (the spec's tests ask the same question). */
const newest = (yap, title) => (yap.readJson('chats.json')?.conversations ?? []).filter((x) => x.title.startsWith(title)).sort((a, b) => b.id - a.id)[0];
const noteTitled = (yap, title) => (yap.readJson('notes.json')?.notes ?? []).some((n) => n.title === title);

test('Retry and Edit of a question researched by the switch alone research it again, the switch off by then; one asked without it is answered again as ever', async ({ yap, main, fakeLlm, shot }) => {
  test.setTimeout(150_000);
  await freshChat(main);
  await research(main, true);
  fakeLlm.scriptChat(loopOf('Where QWERTY came from'));
  // (Words that don't ask for research themselves: the switch alone researches them.)
  await ask(main, 'How did the QWERTY keyboard layout come about?');
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect(lastAnswer(main)).toContainText('The full report is open on the right.', { timeout: 30_000 });
  // Kept with the question: it was asked with the switch on.
  const conv = () => newest(yap, 'How did the QWERTY');
  await expect.poll(() => conv()?.messages?.[0]?.research ?? null).toBe(true);
  expect(conv().messages[1].research).toBeUndefined();

  // The switch off now: ↻ researches again, as it was asked — the plan first.
  await research(main, false);
  const atRetry = fakeLlm.requests.length;
  fakeLlm.scriptChat(loopOf('QWERTY, looked into again'));
  await lastAnswer(main).hover();
  await thread(main).getByTestId('chat-retry').last().click();
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card(main)).toHaveAttribute('data-research', 'true');
  await expect(card(main).locator('.otitle')).toContainText('Research plan · How did the QWERTY keyboard layout come about');
  expect(plans(fakeLlm, atRetry)).toHaveLength(1);
  await expect(chip(main)).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '07-retry-researches-again-light');
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect(lastAnswer(main)).toContainText('The full report is open on the right.', { timeout: 30_000 });
  await expect.poll(() => noteTitled(yap, 'QWERTY, looked into again')).toBe(true);
  await expect(thread(main).getByTestId('chat-branch-count')).toHaveText('2/2');
  const searched = fakeLlm.requests.slice(atRetry).find((r) => r.tools.includes('web_search'));
  expect(searched.user).toContain('This is a research turn');
  await theme(yap, main, 'dark');
  await shot(main, '08-retried-research-dark');
  await theme(yap, main, 'light');

  // Edit: the new words researched too, the plan first (■ there: nothing researched, the edit kept).
  await questions(main).first().hover();
  await thread(main).getByTestId('chat-edit-message').first().click();
  await thread(main).getByTestId('chat-edit-input').fill('How did the Dvorak keyboard layout come about?');
  const atEdit = fakeLlm.requests.length;
  await thread(main).getByTestId('chat-edit-save').click();
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card(main)).toHaveAttribute('data-research', 'true');
  expect(plans(fakeLlm, atEdit)).toHaveLength(1);
  expect(plans(fakeLlm, atEdit)[0].user).toContain("The user's message: How did the Dvorak keyboard layout come about?");
  await shot(main, '09-edit-researches-again-light');
  await main.getByRole('button', { name: 'Stop the answer' }).click();
  await expect(lastAnswer(main)).toContainText('Research stopped before the report was written.', { timeout: 20_000 });
  await expect(questions(main)).toHaveText(['How did the Dvorak keyboard layout come about?']);
  await expect.poll(() => conv()?.messages?.[0]?.research ?? null).toBe(true);

  // Asked with the switch off (words that don't ask): its ↻ answers it again as any question, no plan.
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'A machine for typing letters.' }]);
  await ask(main, 'What is a typewriter?');
  await expect(lastAnswer(main)).toContainText('A machine for typing letters.', { timeout: 20_000 });
  const atPlain = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'A writing machine with keys.' }]);
  await lastAnswer(main).hover();
  await thread(main).getByTestId('chat-retry').last().click();
  await expect(lastAnswer(main)).toContainText('A writing machine with keys.', { timeout: 20_000 });
  await expect(thread(main).getByTestId('chat-outline-card')).toHaveCount(0);
  expect(plans(fakeLlm, atPlain)).toHaveLength(0);
});

// ---------------------------------------------------------------- on your Claude plan

const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const initOf = (tools, servers) => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: SID, tools, mcp_servers: servers, model: MODEL, permissionMode: 'dontAsk', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const whole = (id, content, stop) => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: MODEL, content, stop_reason: stop }, parent_tool_use_id: null, session_id: SID });
const resultOf = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 900, num_turns: 4, result: text, session_id: SID, total_cost_usd: 0.004, usage: { input_tokens: 6100, output_tokens: 240, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
/** The model calls `name`, streamed then whole, as Claude Code writes it; then the tool's result. */
const called = (msg, id, name, input, content, structured) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id, name, input: {} } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'tool_use', id, name, input }], 'tool_use'),
  ev({ type: 'message_stop' }),
  { type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content }] }, parent_tool_use_id: null, session_id: SID, tool_use_result: structured },
];
/** Its words, streamed then whole, and the turn's result. */
const said = (msg, text) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'text', text }], 'end_turn'),
  ev({ type: 'message_stop' }),
  resultOf(text),
];
const fetched = (msg, id, url, text) => called(msg, id, 'WebFetch', { url, prompt: 'What does it say about the layout?' }, text, { bytes: 2400, code: 200, codeText: 'OK', result: text, durationMs: 300, url });

test('on your Claude plan, a page Claude Code read that no search found is titled with its own title in the Sources row and the report, and a word before the report goes', async ({ yap, main, fakeLlm, fakeWeb, shot }) => {
  test.setTimeout(120_000);
  const PLAN = { research: true, title: "The QWERTY keyboard's story", summary: 'Where QWERTY came from and what followed.', sections: [{ key: 'Origins', intent: 'where it came from' }, { key: 'Why it stayed', intent: 'what kept it going' }, { key: 'Rivals', intent: 'what tried to replace it' }] };
  const REPORT = [
    "I have enough sources now, so here's the report.",
    '',
    '# How the QWERTY layout came about',
    '',
    '## Summary',
    '',
    `QWERTY came with the Sholes and Glidden typewriter [1](${QWERTY_URL}); the Dvorak layout followed in 1936 [2](${DVORAK_URL}).`,
    '',
    '## Origins',
    '',
    `It was sold to Remington in 1873 [1](${QWERTY_URL}).`,
    '',
    '## Rivals',
    '',
    `Dvorak was patented as an alternative [2](${DVORAK_URL}).`,
  ].join('\n');
  await yap.invoke('e2e_claude_script', {
    script: claudeScript('plan-research', [
      // The plan step: a one-shot run with no tools.
      { _for: 'ask' },
      { _wait_user: true },
      initOf([], []),
      resultOf(JSON.stringify(PLAN)),
      { _wait_user: true },
      // The chat's session: a search that finds two pages, then two pages read — one the search never found.
      { _for: 'chat' },
      { _wait_user: true },
      initOf(['WebFetch', 'WebSearch'], [{ name: 'yap', status: 'connected' }]),
      ...called('m1', 'toolu_search', 'WebSearch', { query: 'QWERTY keyboard history' }, `Web search results for query: "QWERTY keyboard history"\n\nLinks: [{"title":"QWERTY - Wikipedia","url":"${QWERTY_URL}"},{"title":"Sholes and Glidden typewriter - Wikipedia","url":"${SHOLES_URL}"}]`, {
        query: 'QWERTY keyboard history',
        results: [{ tool_use_id: 'srvtoolu_e2e', content: [{ title: 'QWERTY - Wikipedia', url: QWERTY_URL }, { title: 'Sholes and Glidden typewriter - Wikipedia', url: SHOLES_URL }] }],
        durationSeconds: 0.4,
      }),
      ...fetched('m2', 'toolu_fetch1', QWERTY_URL, 'QWERTY was designed for the Sholes and Glidden typewriter and sold to Remington in 1873.'),
      ...fetched('m3', 'toolu_fetch2', DVORAK_URL, 'The Dvorak keyboard layout was patented in 1936 as an alternative to QWERTY.'),
      ...said('m4', REPORT),
      { _wait_user: true },
    ]),
  });
  await setConfig(yap, main, { llmScopes: { chat: PLAN_CHAT } });
  try {
    await freshChat(main);
    await research(main, true);
    const pagesBefore = fakeWeb.requests.length;
    await ask(main, 'How did the QWERTY keyboard layout come about?');
    await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 30_000 });
    await expect(card(main).locator('.otitle')).toContainText("Research plan · The QWERTY keyboard's story");
    await card(main).getByRole('button', { name: 'Start research' }).click();
    await expect(lastAnswer(main)).toContainText('The full report is open on the right.', { timeout: 40_000 });

    // The Sources row: each page read by its own title — the one no search found too, read on this PC.
    const row = thread(main).getByTestId('chat-sources').last();
    await expect(row.getByTestId('chat-source').locator('.srctitle')).toHaveText(['QWERTY - Wikipedia', 'Dvorak keyboard layout - Wikipedia']);
    await expect(row).not.toContainText('en.wikipedia.org/wiki/Dvorak');
    const titleReads = fakeWeb.requests.slice(pagesBefore).filter((r) => r.service === 'standin' && r.host === 'en.wikipedia.org' && r.path.startsWith('/wiki/'));
    expect(titleReads.map((r) => r.path)).toEqual(['/wiki/Dvorak_keyboard_layout']);

    // The report: its own title (the word before it gone, one # title), and its Sources by the pages' titles.
    const body = panel(main).locator('.docbody');
    await expect(body.locator('.docname')).toHaveText('How the QWERTY layout came about');
    await expect(body).not.toContainText('enough sources');
    await expect(body.locator('li').filter({ hasText: 'Dvorak keyboard layout - Wikipedia' })).toHaveCount(1);
    const note = (yap.readJson('notes.json')?.notes ?? []).find((n) => n.title === 'How the QWERTY layout came about');
    expect(note.content).toContain(`[Dvorak keyboard layout - Wikipedia](${DVORAK_URL})`);
    expect(note.content).toContain(`[QWERTY - Wikipedia](${QWERTY_URL})`);
    expect(note.content.split('\n').filter((l) => l.startsWith('# '))).toHaveLength(1);
    // Kept so with the answer.
    const answer = newest(yap, 'How did the QWERTY')?.messages?.at(-1);
    expect(answer.sources.filter((s) => s.opened).map((s) => s.title)).toEqual(['QWERTY - Wikipedia', 'Dvorak keyboard layout - Wikipedia']);
    await shot(main, '10-plan-research-sources-light');
    await theme(yap, main, 'dark');
    await shot(main, '11-plan-research-sources-dark');
    await theme(yap, main, 'light');
  } finally {
    await setConfig(yap, main, { llmScopes: { chat: fakeChat(fakeLlm) } }).catch(() => {});
  }
});
