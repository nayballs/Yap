// `npm run test:lab`: Yap Lab's own tests (docs/lab.md).
// - Its pure parts: options, the model, the config, the build's freshness,
//   and how a turn's record is summed up (no Yap needed).
// - A `--fake` run end to end, offline: build (incremental; YAP_LAB_NO_BUILD=1
//   skips it), start, a typed question whose tools are recorded, a spoken
//   one, a card asked and answered, an agent's permission prompt answered,
//   the plan's path on Yap's stand-in for Claude Code, shots, state, logs,
//   stop. It runs in a lab folder of its own (test-results/lab-test), so a
//   lab someone is using keeps running. The fake AI and the stand-in web are
//   the e2e suite's; nothing goes on the internet and no one's Claude Code
//   or account is used. Its windows sit behind yours (test mode): you can
//   use the PC.
import { test, describe, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { REPO, buildFresh, labConfig, onOff, parseArgs, parseModel, parseSize } from './options.mjs';
import { coloursIn, drawPng, meanLuma, pngSize, readPng, stitch } from './png.mjs';
import { checkTurn, kindOf, probePasses, resultObject, screenChecks } from './script.mjs';
import { pageCuts, PAGE_MAX } from './sheet.mjs';

describe('a tall sheet in pages', () => {
  test('cut between shots where one fits, each under a page', () => {
    const cuts = pageCuts(13021, [0, 40, 900, 5000, 7500, 7700, 9000, 12000]);
    assert.deepEqual(cuts, [{ y: 0, h: 7500 }, { y: 7500, h: 5521 }]);
    assert.ok(cuts.every((c) => c.h <= PAGE_MAX));
  });
  test('one page when it fits; straight cuts when no shot starts in reach', () => {
    assert.deepEqual(pageCuts(5000, [100]), [{ y: 0, h: 5000 }]);
    assert.deepEqual(pageCuts(20000, []), [{ y: 0, h: PAGE_MAX }, { y: PAGE_MAX, h: PAGE_MAX }, { y: 2 * PAGE_MAX, h: 20000 - 2 * PAGE_MAX }]);
  });
});
import { SEGMENT, TILE, parseTurns, placeShot, sheetHtml } from './sheet.mjs';
import { shotNames, themesOf } from './shots.mjs';
import { fillArgs, refIn, templated } from './fake.mjs';
import { cardsOf, conversationOf, cut, endOf, summarize, toolsOf } from './turns.mjs';

// ---------------------------------------------------------------- pure

describe('options', () => {
  test('arguments: values, = values, switches, positionals', () => {
    assert.deepEqual(parseArgs(['hello there', '--spoken', '--chat', 'new', '--timeout=30']), { _: ['hello there'], spoken: true, chat: 'new', timeout: '30' });
    assert.throws(() => parseArgs(['--chat']), /--chat needs a value/);
    assert.equal(onOff('on', false, 'web'), true);
    assert.equal(onOff(undefined, true, 'web'), true);
    assert.throws(() => onOff('maybe', true, 'web'), /on or off/);
  });

  test('the model: the plan by default, short names on the plan, the API only by name', () => {
    assert.deepEqual(parseModel(undefined), { kind: 'plan', provider: 'claude-plan', model: 'claude-sonnet-5-5' });
    assert.deepEqual(parseModel('claude-plan:claude-opus-5-5'), { kind: 'plan', provider: 'claude-plan', model: 'claude-opus-5-5' });
    assert.deepEqual(parseModel('haiku'), { kind: 'plan', provider: 'claude-plan', model: 'claude-haiku-5-5' });
    assert.deepEqual(parseModel('anthropic:claude-haiku-5-5'), { kind: 'api', provider: 'anthropic', model: 'claude-haiku-5-5' });
    assert.deepEqual(parseModel(undefined, { fake: true }), { kind: 'fake', provider: 'custom', model: 'fake-gpt-5' });
    assert.equal(parseModel('claude-plan', { fake: true }).kind, 'plan');
    assert.throws(() => parseModel('gpt-9'), /--model is/);
  });

  test('the config: the plan’s marker, never a key unless given one; nothing spent behind the lab’s back', () => {
    const plan = labConfig({ model: parseModel('claude-plan'), web: true, yapCode: true });
    assert.equal(plan.llmScopes.chat.provider, 'claude-plan');
    assert.deepEqual(plan.ppApiKeys, { 'claude-plan': 'claude-plan' });
    assert.equal(plan.agentsEnabled, true);
    assert.equal(plan.agentsPay, 'plan');
    assert.equal(plan.memoryLearnChats, false);
    assert.equal(plan.postProcessEnabled, false);
    assert.throws(() => labConfig({ model: parseModel('anthropic:claude-haiku-5-5') }), /YAP_LAB_ANTHROPIC_KEY/);
    const api = labConfig({ model: parseModel('anthropic:claude-haiku-5-5'), key: 'sk-ant-lab-test', effort: 'high' });
    assert.equal(api.ppApiKeys.anthropic, 'sk-ant-lab-test');
    assert.deepEqual(api.chatEffort, { 'claude-haiku-5-5': 'high' });
    assert.throws(() => labConfig({ model: parseModel('haiku'), effort: 'huge' }), /--effort/);
  });

  test('a build is fresh from the last commit on', () => {
    const head = { commit: 'b'.repeat(40), time: 2000 };
    assert.equal(buildFresh(null, head).fresh, false);
    assert.equal(buildFresh({ commit: head.commit, builtAt: 1 }, head).fresh, true);
    assert.equal(buildFresh({ commit: 'a'.repeat(40), builtAt: 2500 }, head).fresh, true);
    assert.match(buildFresh({ commit: 'a'.repeat(40), builtAt: 1500 }, head).why, /older than the checkout's last commit/);
  });
});

describe('a turn’s record', () => {
  const claude = (seq, line) => ({ seq, ts: seq, kind: 'claude', pid: 7, line });
  const items = [
    { seq: 1, kind: 'stream', event: { kind: 'turn', conversationId: 4, userMessageId: 'chat:4:0' } },
    { seq: 2, kind: 'tool', via: 'yap', name: 'browser_navigate', args: { url: 'https://example.com/' }, result: { success: true, displayText: 'Brought up example.com beside the chat' } },
    { seq: 3, kind: 'tool', via: 'yap', name: 'browser_click', args: { ref: 1 }, result: { success: false, displayText: "Didn't click ‘Buy’: you said no", data: { asked: true, allowed: false } } },
    claude(4, { type: 'assistant', message: { content: [{ type: 'tool_use', id: 'toolu_1', name: 'mcp__yap__browser_read', input: { mode: 'text' } }, { type: 'tool_use', id: 'toolu_2', name: 'WebSearch', input: { query: 'x'.repeat(900) } }] } }),
    claude(5, { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'toolu_1', content: [{ type: 'text', text: 'Example Domain' }, { type: 'image', data: '<base64: 2000 chars>' }] }] } }),
    claude(6, { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'toolu_2', is_error: true, content: 'Search refused: not allowed here' }] } }),
    { seq: 7, kind: 'stream', event: { kind: 'confirm', id: 9, act: 'press', label: 'Buy', site: 'example.com', does: 'buy', state: 'asking' } },
    { seq: 8, kind: 'stream', event: { kind: 'confirm', id: 9, act: 'press', label: 'Buy', site: 'example.com', does: 'buy', state: 'refused' } },
    { seq: 9, kind: 'stream', event: { kind: 'confirm', id: 10, act: 'connector', label: 'Create page', site: '', does: '', state: 'asking', connector: { connector: 'Notes', tool: 'create', args: [{ key: 'title', value: 'Hi' }] } } },
    { seq: 10, kind: 'stream', event: { kind: 'chips', chips: ['Brought up example.com'], sources: [], web: true } },
  ];

  test('the conversation, from the turn’s first event', () => {
    assert.equal(conversationOf(items), 4);
    assert.equal(conversationOf([{ kind: 'event', name: 'yap-assistant-turn', payload: { phase: 'asked', conversationId: 2 } }]), 2);
  });

  test('a typed turn ends with chat_send’s answer, a spoken one with the Voice Assistant’s', () => {
    assert.deepEqual(endOf(items), { done: false, status: 'running', error: null });
    assert.deepEqual(endOf([{ kind: 'sent', ok: true, reply: { stopped: false } }]), { done: true, status: 'answered', error: null });
    assert.equal(endOf([{ kind: 'sent', ok: false, error: 'No AI model' }]).status, 'failed');
    assert.equal(endOf([{ kind: 'event', name: 'yap-assistant-turn', payload: { phase: 'failed', error: 'x' } }], { spoken: true }).status, 'failed');
    assert.equal(endOf([{ kind: 'event', name: 'yap-assistant-turn', payload: { phase: 'answered' } }], { spoken: true }).status, 'answered');
    // A typed turn isn't ended by the Voice Assistant's events.
    assert.equal(endOf([{ kind: 'event', name: 'yap-assistant-turn', payload: { phase: 'answered' } }]).done, false);
  });

  test('tools: Yap’s and Claude Code’s, paired with their results, trimmed, refusals marked', () => {
    const tools = toolsOf(items);
    assert.deepEqual(tools.map((t) => [t.via, t.server ?? '', t.name, t.ok, !!t.refused]), [
      ['yap', '', 'browser_navigate', true, false],
      ['yap', '', 'browser_click', false, true],
      ['claude', 'yap', 'browser_read', true, false],
      ['claude', '', 'WebSearch', false, true],
    ]);
    assert.match(tools[2].result, /Example Domain\n\[1 picture\]/);
    assert.match(tools[3].args, /\[\+\d+ chars, \d+ in all\]$/);
    assert.deepEqual(toolsOf(items, { full: true })[3].args, { query: 'x'.repeat(900) });
    // An agent at work while the turn ran: its own Claude Code's calls are
    // the agent's, never the turn's (the summary names them apart).
    const busy = [
      ...items,
      { seq: 11, ts: 11, kind: 'claude', pid: 9, agent: true, line: { type: 'assistant', message: { content: [{ type: 'tool_use', id: 'toolu_9', name: 'Edit', input: { file_path: 'src/convert.js' } }] } } },
      { seq: 12, ts: 12, kind: 'claude', pid: 9, agent: true, line: { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'toolu_9', content: 'The file has been updated.' }] } } },
    ];
    assert.deepEqual(toolsOf(busy).at(-1), { via: 'agent', name: 'Edit', args: toolsOf(busy).at(-1).args, result: 'The file has been updated.', ok: true, seq: 11, pid: 9 });
    const conv = { id: 4, messages: [{ role: 'user', text: 'Q' }, { role: 'assistant', text: 'A' }] };
    const out = summarize({ turn: { n: 1, question: 'Q', spoken: false, startedAt: 0, status: 'answered', askedAt: 0 }, items: busy, conv, now: 1000 });
    assert.deepEqual(out.tools.map((t) => t.name), ['browser_navigate', 'browser_click', 'browser_read', 'WebSearch']);
    assert.deepEqual(out.agentTools, ['Edit']);
  });

  test('cards: each as it is now, a connector’s with Always allow; Claude Code’s prompts and plans', () => {
    const cards = cardsOf(items, { approvals: [{ id: 3, tool: 'Bash', summary: 'Run a command: npm test', detail: 'npm test', run: 'a1' }], agents: [{ id: 'a2', folder: 'lab-sandbox', status: 'done', plan: true, planAllowed: false, text: 'The plan' }] });
    assert.deepEqual(cards.map((c) => [c.id, c.kind, c.state, c.answers.join('|')]), [
      ['confirm:9', 'page', 'refused', 'allow|deny'],
      ['confirm:10', 'connector', 'asking', 'allow|always|deny'],
      ['approve:3', 'approve', 'asking', 'allow|deny'],
      ['plan:a2', 'plan', 'asking', 'allow|deny'],
    ]);
    // A plan set aside ("Plan not used") waits on nobody.
    assert.deepEqual(cardsOf([], { agents: [{ id: 'a3', folder: 'lab-sandbox', status: 'done', plan: true, planAllowed: false, planUnused: true }] }), []);
  });

  test('summed up: the answer, steps, tools, the cards waiting, and the hint', () => {
    const conv = { messages: [{ role: 'user', text: 'Q' }, { role: 'assistant', text: 'A'.repeat(5000), tools: ['Brought up example.com'], model: { provider: 'custom', id: 'fake' } }] };
    const out = summarize({ turn: { n: 1, question: 'Q', spoken: false, startedAt: 0, status: 'running', askedAt: 0 }, items, conv, now: 1000 });
    assert.equal(out.status, 'waiting');
    assert.deepEqual(out.pending, ['confirm:10']);
    assert.match(out.hint, /answer confirm:10 allow\|deny\|always/);
    assert.deepEqual(out.steps, ['Brought up example.com']);
    assert.equal(out.answer.length < 5000, true);
    assert.match(out.answer, /\[\+1000 chars, 5000 in all\]$/);
    assert.equal(out.ms, 1000);
    assert.equal(cut('abc', 5), 'abc');
    assert.deepEqual(out.questionImages, []);
  });

  test('the screenshots kept with the question: its own, never an earlier one’s', () => {
    // (8 Nov 2026: a spoken follow-up must send no screenshot of the window in front.)
    const conv = {
      messages: [
        { role: 'user', text: 'What is this error on my screen?', images: [{ id: 'a', source: 'window' }] },
        { role: 'assistant', text: 'A read-only file.' },
        { role: 'user', text: 'How many people usually attend that one?' },
        { role: 'assistant', text: 'About 12,000.' },
      ],
    };
    const turn = (askedAt) => ({ n: 2, question: 'Q', spoken: true, startedAt: 0, endedAt: 1, status: 'answered', askedAt });
    assert.deepEqual(summarize({ turn: turn(0), items: [], conv }).questionImages, [{ source: 'window', asText: false }]);
    const followUp = summarize({ turn: turn(2), items: [], conv });
    assert.deepEqual(followUp.questionImages, []);
    assert.ok(checkTurn({ questionImages: 0 }, { result: followUp, calls: [] }).every((c) => c.pass));
    const missed = checkTurn({ questionImages: 0 }, { result: summarize({ turn: turn(0), items: [], conv }), calls: [] });
    assert.deepEqual(missed.map((c) => [c.check, c.pass]), [['questionImages 0', false]]);
  });

  // 14 Nov 2026: a question to a Code chat that works directly came back at once with no answer — the
  // session's work isn't Chat's stream. Now the turn is the session's, over with its answer in the chat.
  test('a Code chat’s own session: the turn waits for its answer; its steps, its calls, its cards and pages', () => {
    const event = (seq, name, payload) => ({ seq, ts: seq * 1000, kind: 'event', name, payload });
    const call = (seq, session, content, agent = true) => ({ seq, ts: seq * 1000, kind: 'claude', pid: 5, agent, line: { type: content[0].type === 'tool_use' ? 'assistant' : 'user', session_id: session, message: { content } } });
    const sent = { seq: 2, kind: 'sent', ok: true, reply: { conversationId: 3, direct: { id: 'ab12', turn: 2, folder: 'lab-sandbox', mode: 'auto', status: 'queued' } } };
    const working = [
      { seq: 1, kind: 'stream', event: { kind: 'turn', conversationId: 3 } },
      sent,
      event(3, 'yap-agent', { id: 'ab12', turn: 2, status: 'running', step: 'Thinking' }),
      event(4, 'yap-agent', { id: 'ab12', turn: 2, status: 'running', step: 'Opening wiki.example.com/guide beside the chat' }),
      call(5, 'sess-1', [{ type: 'tool_use', id: 'toolu_n', name: 'mcp__yap__browser_navigate', input: { url: 'https://wiki.example.com/guide' } }]),
      call(6, 'sess-1', [{ type: 'tool_result', tool_use_id: 'toolu_n', content: '{"success":true,"displayText":"Brought up wiki.example.com/guide beside the chat"}' }]),
      event(7, 'yap-agent-shown', { id: 'ab12', turn: 2, conversation: 3, event: { kind: 'page', page: { url: 'https://wiki.example.com/guide', title: 'Guide' } } }),
      event(8, 'yap-agent', { id: 'ab12', turn: 2, status: 'running', step: 'Clicking on the page beside the chat' }),
      event(9, 'yap-agent', { id: 'ab12', turn: 2, status: 'running', step: 'Clicking on the page beside the chat' }),
      call(10, 'sess-1', [{ type: 'tool_use', id: 'toolu_c', name: 'mcp__yap__browser_click', input: { ref: 4 } }]),
      // Another agent at work meanwhile: never the session's.
      call(11, 'sess-9', [{ type: 'tool_use', id: 'toolu_e', name: 'Edit', input: { file_path: 'notes.md' } }]),
      event(12, 'yap-agent-shown', { id: 'ab12', turn: 2, conversation: 3, event: { kind: 'confirm', id: 41, act: 'press', label: 'Buy the full guide', site: 'wiki.example.com', does: 'buy', state: 'asking' } }),
    ];
    // Chat's send came back at once, naming the session: the turn isn't over.
    assert.deepEqual(endOf(working), { done: false, status: 'running', error: null, session: true });
    const turn = { n: 3, question: 'Open the guide', spoken: false, startedAt: 0, status: 'running', askedAt: 0 };
    const agents = [{ id: 'ab12', session: 'sess-1', folder: 'lab-sandbox', model: 'claude-haiku-5-5', status: 'running', turnList: [{ n: 2, model: 'claude-haiku-5-5', mode: 'auto', effort: 'low', status: 'running' }] }];
    const now = summarize({ turn, items: working, conv: { messages: [{ role: 'user', text: 'Open the guide' }] }, agents });
    assert.equal(now.status, 'waiting', 'its card waits');
    assert.deepEqual(now.pending, ['confirm:41']);
    assert.deepEqual(now.steps, ['Thinking', 'Opening wiki.example.com/guide beside the chat', 'Clicking on the page beside the chat']);
    assert.deepEqual(now.tools.map((t) => [t.via, t.server, t.name, t.ok]), [['session', 'yap', 'browser_navigate', true], ['session', 'yap', 'browser_click', null]]);
    assert.deepEqual(now.agentTools, ['Edit']);
    assert.deepEqual(now.pagesShown, ['https://wiki.example.com/guide']);
    assert.equal(now.model, 'claude-haiku-5-5');
    assert.deepEqual(now.session, { id: 'ab12', turn: 2, folder: 'lab-sandbox', mode: 'auto', effort: 'low', status: 'running', cost: null });
    // Its turn over: still not, until its answer is in the chat (the daemon gives it 20 s) …
    const over = [...working, event(13, 'yap-agent', { id: 'ab12', turn: 2, status: 'done' })];
    assert.deepEqual(endOf(over), { done: false, status: 'running', error: null, session: true, ended: 13_000, endedStatus: 'done' });
    // … then answered; a failed turn's answer, failed.
    assert.deepEqual(endOf([...over, event(14, 'yap-agent-report', { agent: 'ab12', conversation: 3 })]), { done: true, status: 'answered', error: null, session: true });
    const failed = [...working, event(13, 'yap-agent', { id: 'ab12', turn: 2, status: 'failed' }), event(14, 'yap-agent-report', { agent: 'ab12' })];
    assert.equal(endOf(failed).status, 'failed');
    // An earlier turn's end or another agent's report doesn't end it.
    assert.equal(endOf([...working, event(13, 'yap-agent', { id: 'ab12', turn: 1, status: 'done' }), event(14, 'yap-agent-report', { agent: 'zz99' })]).done, false);
  });
});

describe('shots, the sheet and scripts', () => {
  test('themes, names, the window’s size, which turns', () => {
    assert.deepEqual(themesOf(undefined), ['light', 'dark']);
    assert.deepEqual(themesOf('dark'), ['dark']);
    assert.throws(() => themesOf('sepia'), /light, dark or both/);
    assert.deepEqual(shotNames(['thread', 'panel'], ['light', 'dark']).map((n) => n.file), ['01-thread-light.png', '02-thread-dark.png', '03-panel-light.png', '04-panel-dark.png']);
    assert.equal(shotNames(['page'], ['light'], { prefix: 'card-confirm-1-' })[0].file, 'card-confirm-1-01-page-light.png');
    assert.equal(parseSize(undefined), '1280x860');
    assert.equal(parseSize('1600×900'), '1600x900');
    assert.throws(() => parseSize('800x600'), /at least 860x600/);
    const t = parseTurns('1-3,7');
    assert.deepEqual([1, 2, 3, 4, 7].map(t), [true, true, true, false, true]);
    assert.equal(parseTurns('5-')(99), true);
    assert.equal(parseTurns(undefined)(1), true);
    assert.throws(() => parseTurns('a-b'), /--turns/);
  });

  test('PNGs: a size from the header, pieces stitched, a made-up picture', () => {
    const red = drawPng({ width: 40, height: 30, shape: 'square', color: '#ff0000', background: '#ff0000' });
    const blue = drawPng({ width: 40, height: 50, shape: 'square', color: '#0000ff', background: '#0000ff' });
    assert.deepEqual(pngSize(red), { width: 40, height: 30 });
    const both = readPng(stitch([{ png: red, from: 10, to: 30 }, { png: blue }]));
    assert.deepEqual([both.width, both.height], [40, 70]);
    assert.deepEqual([...both.data.subarray(0, 3)], [255, 0, 0]);
    assert.deepEqual([...both.data.subarray((69 * 40) * 4, (69 * 40) * 4 + 3)], [0, 0, 255]);
    const circle = readPng(drawPng({ width: 100, height: 60 }));
    assert.ok(coloursIn(circle) === 2 && meanLuma(circle) > 0);
  });

  test('the sheet: a column at its CSS size at most, a tall shot in even pieces, light beside dark', () => {
    assert.deepEqual(placeShot({ width: 460, height: 800, dpr: 1 }), { w: 460, h: 800, pieces: [{ at: 0, h: 800 }] });
    const tall = placeShot({ width: 1155, height: 4500, dpr: 1.5 });
    assert.equal(tall.w, TILE);
    assert.ok(tall.pieces.length === Math.ceil(tall.h / SEGMENT) && tall.pieces.every((p) => p.h <= SEGMENT && p.h > SEGMENT / 2), JSON.stringify(tall));
    const dir = path.join(REPO, 'test-results', 'lab-test', 'sheets');
    const shot = (window, theme, height) => ({ file: path.join(REPO, 'test-results', 'lab-test', 'shots', '002', `x-${window}-${theme}.png`), window, theme, set: 'end', width: 700, height, dpr: 1 });
    const html = sheetHtml({ title: 'T', turns: [{ turn: 2, question: 'one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen', status: 'answered', shots: [shot('thread', 'light', 3000), shot('thread', 'dark', 3000), shot('panel', 'light', 800)] }], dir });
    assert.match(html, /Turn 2 <span class="st st-answered">answered<\/span>/);
    assert.match(html, /“one two three four five six seven eight nine ten eleven twelve thirteen fourteen…”/);
    assert.match(html, /src="\.\.\/shots\/002\/x-thread-light\.png"/);
    // 3000 px: three pieces each, interleaved light / dark, then the panel.
    const order = [...html.matchAll(/<b>(\w+)<\/b> · (\w+)( · \d\/\d)?/g)].map((m) => `${m[1]} ${m[2]}${m[3] ?? ''}`);
    assert.deepEqual(order, ['thread light · 1/3', 'thread dark · 1/3', 'thread light · 2/3', 'thread dark · 2/3', 'thread light · 3/3', 'thread dark · 3/3', 'panel light']);
  });

  test('an expect: tools with their arguments and results, counts, cards, agents; the screen’s as probes', () => {
    const calls = [
      { via: 'yap', name: 'browser_scroll', args: { direction: 'down', tab: 1 }, result: { success: true, data: { tab: 1, scroll: { y: 1320, at_top: false } }, displayText: 'Scrolled down example.com' }, ok: true },
      { via: 'claude', server: 'yap', name: 'browser_click', args: { ref: 9999 }, result: [{ type: 'text', text: '{"success":false,"error":"There is no element [9999]: the refs go up to 41."}' }], ok: false },
    ];
    const result = { status: 'answered', answer: 'Scrolled to 1320.', cards: [{ id: 'confirm:1', kind: 'page', does: 'buy', state: 'refused' }], pictures: [{}], files: [], documents: [], pagesShown: ['https://example.com/'], sources: [], tools: calls };
    const passes = (expect) => checkTurn(expect, { result, calls }).every((c) => c.pass);
    assert.equal(resultObject(calls[1]).error, 'There is no element [9999]: the refs go up to 41.');
    assert.ok(passes({ status: 'answered', used: ['browser_scroll', '/click/'], notUsed: 'browser_type', pictures: 1, files: 0, pagesShown: { min: 1 }, answerContains: '1320', card: { kind: 'page', does: 'buy' } }));
    assert.ok(passes({ tool: [{ name: 'browser_scroll', args: { tab: 1 }, path: 'data.scroll.y', gte: 600 }, { name: 'browser_click', args: { ref: 9999 }, ok: false, resultNot: '/page changed/i', path: 'error', matches: 'refs go up' }] }));
    assert.ok(!passes({ tool: { name: 'browser_scroll', path: 'data.scroll.at_top', equals: true } }));
    assert.ok(!passes({ refused: false, failed: false }) && passes({ failed: true }));
    const missed = checkTurn({ used: 'browser_find', pictures: 2 }, { result, calls }).filter((c) => !c.pass);
    assert.deepEqual(missed.map((c) => c.check), ['used browser_find', 'pictures 2']);
    assert.match(missed[0].detail, /called: browser_scroll, browser_click/);
    assert.equal(checkTurn({ used: 'x' }, { result: null })[0].detail, 'no turn yet: ask first');
    const agents = { agents: [{ id: 'a1', status: 'done', files: ['notes.md'] }] };
    assert.ok(checkTurn({ agent: { status: 'done', files: { min: 1 } } }, { result: null, agents }).every((c) => c.pass));
    // An agent's mode, plan and read-only turn (agent-panel's Bypass catch-up).
    const bypass = { agents: [{ id: 'a1', status: 'done', mode: 'auto', files: ['notes.md'] }, { id: 'a2', status: 'done', mode: 'bypassPermissions', plan: false, readOnly: true, files: [] }] };
    assert.ok(checkTurn({ agent: { mode: 'bypassPermissions', readOnly: true, plan: false, status: 'done' } }, { result: null, agents: bypass }).every((c) => c.pass));
    const notRead = checkTurn({ agent: { mode: 'auto', readOnly: true } }, { result: null, agents: bypass });
    assert.ok(!notRead[0].pass);
    assert.match(notRead[0].check, /mode auto, readOnly true/);
    assert.match(notRead[0].detail, /a2: done, bypassPermissions read only/);
    // Its latest turn's model and effort (agent-sizing: Chat sizes each agent).
    const sized = { agents: [{ id: 'a3', status: 'done', mode: 'auto', model: 'claude-opus-5-5', effort: 'xhigh', sizing: 'hard debugging', files: [] }] };
    assert.ok(checkTurn({ agent: { model: 'claude-opus-5-5', effort: ['high', 'xhigh'] } }, { result: null, agents: sized }).every((c) => c.pass));
    assert.ok(checkTurn({ agent: { model: '/opus|fable/' } }, { result: null, agents: sized }).every((c) => c.pass));
    const small = checkTurn({ agent: { model: ['claude-haiku-5-5', 'claude-sonnet-5-5'], effort: 'low' } }, { result: null, agents: sized });
    assert.ok(!small[0].pass);
    assert.match(small[0].check, /model claude-haiku-5-5\|claude-sonnet-5-5, effort low/);
    assert.match(small[0].detail, /a3: done, auto, claude-opus-5-5 at xhigh \(hard debugging\)/);
    const screen = screenChecks({ selector: [{ window: 'panel', css: '.x' }, { css: '.y', present: false }], visible: 'THE END' });
    assert.deepEqual(screen.map((s) => s.args.window), ['panel', 'main', 'thread']);
    assert.equal(probePasses(screen[0].want, { visible: 2 }), true);
    assert.equal(probePasses(screen[1].want, { visible: 1 }), false);
    assert.equal(probePasses(screen[2].want, { has: true }), true);
    assert.equal(kindOf({ shot: 'x', window: 'panel' }), 'shot');
    assert.throws(() => kindOf({ click: 1 }), /A step is one of/);
  });
});

// ---------------------------------------------------------------- a fake run

const LAB = path.join(REPO, 'test-results', 'lab-test');
const ENV = { ...process.env, YAP_LAB_DIR: LAB };

/** `npm run lab -- <args>` in the test's own lab folder: its JSON (and its exit code, as `exit`). */
function lab(...args) {
  const r = spawnSync(process.execPath, [path.join(REPO, 'scripts', 'lab', 'lab.mjs'), ...args], { cwd: REPO, env: ENV, encoding: 'utf8', timeout: 600_000 });
  const line = r.stdout.trim().split('\n').at(-1) ?? '';
  try {
    return Object.defineProperty(JSON.parse(line), 'exit', { value: r.status, enumerable: false });
  } catch {
    throw new Error(`lab ${args[0]} printed no JSON (exit ${r.status}): ${r.stdout}\n${r.stderr}`);
  }
}
describe('the fake AI’s placeholders', () => {
  // A request as the fake AI records it: the turn's tool results so far.
  const request = {
    messages: [
      { role: 'user', content: 'Put Item 3 first' },
      { role: 'tool', content: JSON.stringify({ data: { elements: '[4] link "Lighthouse"\n[9] textbox "Search"' } }) },
      { role: 'tool', content: JSON.stringify({ data: { size: '459×601', width: 1280 }, frame: "browser_click's x, y are in this picture's pixels: 459×601, the page's view." }) },
      { role: 'tool', content: JSON.stringify({ data: { matches: '[12] heading "Lighthouse"\n[31] item "Item 3"' } }) },
    ],
  };
  test('a ref by its words (and kind), a point of the latest shot; anything else as it is', () => {
    assert.equal(refIn(request, 'Lighthouse'), 12, 'the latest list that has it');
    assert.equal(refIn(request, 'Lighthouse', 'link'), 4);
    assert.equal(refIn(request, 'Search'), 9);
    assert.equal(refIn(request, 'Nowhere'), 0);
    assert.equal(refIn(request, 'Item*', 'item'), 31, 'words that start so');
    assert.equal(refIn(request, 'Light*', 'link'), 4);
    assert.deepEqual(fillArgs({ ref: '$ref:heading:Lighthouse', count: 2 }, request), { ref: 12, count: 2 });
    assert.deepEqual(fillArgs({ from: { ref: '$ref:item:Item 3' }, to: { x: '$x:0.5', y: '$y:0.1' } }, request), { from: { ref: 31 }, to: { x: 230, y: 60 } });
    assert.deepEqual(fillArgs({ url: 'https://example.com/$ref', keys: 'Enter' }, request), { url: 'https://example.com/$ref', keys: 'Enter' });
  });
  test('only calls with a placeholder become functions of the request', () => {
    const steps = templated([{ tools: [{ name: 'browser_click', args: { ref: '$ref:Search' } }, { name: 'browser_read', args: { mode: 'elements' } }] }, { content: 'Done.' }]);
    assert.equal(typeof steps[0].tools[0].args, 'function');
    assert.deepEqual(steps[0].tools[0].args(request), { ref: 9 });
    assert.deepEqual(steps[0].tools[1].args, { mode: 'elements' });
    assert.deepEqual(steps[1], { content: 'Done.' });
  });
});

const ok = (out, what) => {
  assert.equal(out.ok, true, `${what}: ${out.error ?? JSON.stringify(out)}`);
  return out;
};
const script = (steps) => ok(lab('fake', JSON.stringify(steps)), 'fake');

const RAIL = 'https://rail.example.com/results?from=Kellbrook&to=Port+Ellis&when=Today';

describe('a --fake run, end to end', { timeout: 900_000 }, () => {
  before(() => {
    fs.rmSync(LAB, { recursive: true, force: true });
    if (process.env.YAP_LAB_NO_BUILD !== '1') {
      const built = lab('build');
      ok(built, 'build');
      assert.ok(fs.existsSync(built.exe));
    }
  });
  after(() => lab('stop'));

  test('start: its own data folder, never %APPDATA%; the windows; the scratch workspace', () => {
    const out = ok(lab('start', '--fake', '--yap-code', 'on', '--idle', '15'), 'start');
    assert.ok(out.dataDir.startsWith(LAB), out.dataDir);
    assert.ok(!/AppData\\Roaming\\yap/i.test(out.dataDir));
    assert.ok(out.windows.includes('settings') && out.windows.includes('overlay'));
    assert.equal(out.workspace.name, 'lab-sandbox');
    assert.ok(fs.existsSync(path.join(out.workspace.path, 'src', 'convert.js')));
    assert.equal(out.claudeCode.standIn, true);
    // The main window at the lab's fixed size (shell/lab.rs), so shots compare between runs.
    assert.deepEqual([out.window.width, out.window.height], [1280, 860]);
    // One lab at a time.
    assert.match(lab('start', '--fake').error, /already running/);
  });

  test('a typed question: the tools it called with their arguments and results, the page beside the chat', () => {
    script([{ tools: [{ name: 'browser_navigate', args: { url: RAIL } }] }, { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] }, { content: 'The next train leaves at 09:42.' }]);
    const out = ok(lab('ask', `When is the next train? ${RAIL}`, '--timeout', '120'), 'ask');
    assert.equal(out.status, 'answered');
    assert.equal(out.answer, 'The next train leaves at 09:42.');
    assert.deepEqual(out.tools.map((t) => [t.name, t.ok]), [['browser_navigate', true], ['browser_read', true]]);
    assert.match(out.tools[1].result, /Buy tickets/);
    assert.equal(out.pages[0].url, RAIL);
    assert.ok(out.steps.some((s) => /^Brought up rail\.example\.com/.test(s)), out.steps.join(' | '));
    const transcript = JSON.parse(fs.readFileSync(out.transcript, 'utf8'));
    assert.equal(transcript.tools[0].args.url, RAIL);
    assert.ok(transcript.record.length > 0);
  });

  test('shots after the turn: the thread’s turn whole (a long answer, stitched), the panel and the page, light and dark', () => {
    const paragraphs = Array.from({ length: 60 }, (_, i) => `Paragraph ${i + 1}: the keeper wound the clockwork every four hours, and the lens turned on its bath of mercury.`);
    script([{ tools: [{ name: 'browser_navigate', args: { url: RAIL } }] }, { content: [...paragraphs, 'THE END'].join('\n\n') }]);
    const out = ok(lab('ask', `Tell me about lighthouses, at length, and open ${RAIL}`, '--timeout', '120'), 'ask');
    assert.equal(out.status, 'answered');
    assert.deepEqual(
      out.shots.map((s) => path.basename(s.file)),
      ['01-thread-light.png', '02-thread-dark.png', '03-panel-light.png', '04-panel-dark.png', '05-page-light.png', '06-page-dark.png']
    );
    for (const s of out.shots) {
      assert.ok(s.file.startsWith(path.join(LAB, 'shots', String(out.turn).padStart(3, '0'))), s.file);
      assert.deepEqual(pngSize(fs.readFileSync(s.file)), { width: s.width, height: s.height });
    }
    const [light, dark] = out.shots.slice(0, 2);
    // Taller than the thread's view, so it took several screens; its height is the whole thread's.
    assert.ok(light.scrollHeight > 1.5 * light.clientHeight && light.pieces >= 2, JSON.stringify(light));
    assert.ok(Math.abs(light.height / light.dpr - light.scrollHeight) <= 40, `${light.height / light.dpr} px of a ${light.scrollHeight} px thread`);
    assert.ok(Math.abs(light.height - Math.round(light.cssHeight * light.dpr)) <= light.pieces + 1, JSON.stringify(light));
    // The question at the top and the answer's end at the bottom were drawn (not a blank band).
    const img = readPng(fs.readFileSync(light.file));
    assert.ok(coloursIn(img, 0, Math.round(80 * light.dpr)) > 4, 'the question at the top');
    assert.ok(coloursIn(img, img.height - Math.round(160 * light.dpr), img.height) > 4, 'the answer’s end at the bottom');
    // Nothing floats over its top-left corner (8 Nov 2026: the list's button
    // sat there, half cut off, in every thread shot).
    const corner = new Set();
    const side = Math.round(24 * light.dpr);
    for (let y = 0; y < side; y++) for (let x = 0; x < side; x++) corner.add(img.data.readUInt32BE((y * img.width + x) * 4));
    assert.equal(corner.size, 1, `${corner.size} colours in the thread shot's top-left corner`);
    // Light and dark really are.
    assert.ok(meanLuma(img) > meanLuma(readPng(fs.readFileSync(dark.file))) + 80, 'the dark shot is darker');
    assert.equal(light.height, dark.height);
    // The theme put back after: the window is light again (the e2e base config's).
    const after = ok(lab('probe', '--window', 'thread', '--text', 'THE END'), 'probe');
    assert.deepEqual([after.theme, after.has], ['light', true]);
    const index = JSON.parse(fs.readFileSync(path.join(path.dirname(light.file), 'index.json'), 'utf8'));
    assert.equal(index.shots.length, 6);
    assert.match(index.question, /^Tell me about lighthouses/);
  });

  test('a card: the turn waits, the lab answers Don’t, the click is refused', () => {
    script([{ tools: [{ name: 'browser_navigate', args: { url: RAIL } }] }, { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] }, { tools: [{ name: 'browser_click', args: { ref: 1 } }] }, { content: 'I didn’t buy it.' }]);
    const asked = ok(lab('ask', `Buy me a ticket for the 09:42: ${RAIL}`, '--timeout', '120'), 'ask');
    assert.equal(asked.status, 'waiting');
    const card = asked.cards.find((c) => c.state === 'asking');
    assert.deepEqual([card.kind, card.label, card.site, card.does], ['page', 'Buy tickets', 'rail.example.com', 'buy']);
    assert.deepEqual(asked.pending, [card.id]);
    // Shots of the card waiting, taken before it was answered.
    const cardShots = asked.shots.filter((s) => s.set === `card:${card.id}`);
    assert.deepEqual([...new Set(cardShots.map((s) => s.window))], ['thread', 'panel', 'page']);
    assert.ok(cardShots.every((s) => path.basename(s.file).startsWith(`card-${card.id.replace(':', '-')}-`)), JSON.stringify(cardShots));
    // A shot while it waits: the main window and the page itself.
    for (const window of ['main', 'panel', 'page']) {
      const shot = ok(lab('shot', '--window', window), `shot ${window}`);
      assert.ok(fs.statSync(shot.shot).size > 1000, window);
    }
    const now = ok(lab('state'), 'state');
    assert.deepEqual(now.pending.map((c) => c.id), [card.id]);
    assert.ok(now.panelTabs.some((t) => t.key?.startsWith('page:https://rail.example.com/') && t.selected), JSON.stringify(now.panelTabs));
    assert.equal(now.turn.status, 'waiting');
    const done = ok(lab('answer', card.id, 'deny', '--timeout', '120'), 'answer');
    assert.equal(done.status, 'answered');
    assert.equal(done.answered.wasWaiting, true);
    const click = done.tools.find((t) => t.name === 'browser_click');
    assert.deepEqual([click.ok, click.refused], [false, true]);
    assert.equal(done.cards.find((c) => c.id === card.id).state, 'refused');
  });

  test('a spoken question, through the Voice Assistant’s path', () => {
    script([{ content: 'Ben Nevis is 1,345 metres high.' }]);
    const out = ok(lab('ask', 'How high is Ben Nevis?', '--spoken', '--timeout', '60'), 'ask --spoken');
    assert.equal(out.status, 'answered');
    assert.equal(out.spoken, true);
    assert.equal(out.answer, 'Ben Nevis is 1,345 metres high.');
    // …and a follow-up typed into the same conversation.
    script([{ content: 'It is in Scotland.' }]);
    const next = ok(lab('ask', 'Where is it?', '--chat', String(out.conversation), '--timeout', '60'), 'ask --chat');
    assert.equal(next.conversation, out.conversation);
    assert.equal(next.answer, 'It is in Scotland.');
  });

  test('Yap Code: an agent in the scratch workspace, its permission prompt answered', () => {
    script([{ tools: [{ name: 'start_agent', args: { folder: 'lab-sandbox', brief: 'Change Draft to Final in notes.md, then run the tests.' } }] }, { content: 'On it.' }]);
    const asked = ok(lab('ask', 'Send an agent to lab-sandbox to tidy notes.md', '--chat', 'code:lab-sandbox', '--timeout', '60'), 'ask code');
    assert.equal(asked.tools[0].name, 'start_agent');
    const waiting = ok(lab('wait', '--agents', '--timeout', '60'), 'wait --agents');
    const card = waiting.pending.find((c) => c.kind === 'approve');
    assert.equal(card?.tool, 'Bash', JSON.stringify(waiting));
    const done = ok(lab('answer', card.id, 'allow', '--timeout', '60'), 'answer approve');
    assert.equal(done.agents[0].status, 'done');
    assert.match(done.reports[0].report, /lab-sandbox/);
  });

  test('a report: a built-in prompt asked, its answer saved as Markdown with the tool calls', () => {
    script([{ tools: [{ name: 'web_search', args: { query: 'Ben Nevis' } }] }, { content: '## Summary\nThe web search works.' }]);
    const out = ok(lab('report', 'browser-tools', '--timeout', '120'), 'report');
    assert.equal(out.status, 'answered');
    assert.match(out.question, /^You are testing your own tools/);
    const text = fs.readFileSync(out.report, 'utf8');
    assert.match(text, /^# Yap Lab report: browser-tools/);
    assert.match(text, /## Summary\nThe web search works\./);
    assert.match(text, /\| web_search \| 1 \| 0 \| 0 \|/);
    assert.equal(JSON.parse(fs.readFileSync(out.transcript, 'utf8')).result.report, out.report);
    // Its shots on a contact sheet beside it, linked from the Markdown with relative paths.
    assert.ok(fs.existsSync(out.sheet.png) && fs.existsSync(out.sheet.html), JSON.stringify(out.sheet));
    assert.match(text, /## Screenshots\n\n!\[The turn's contact sheet\]\([^)]+\.sheet\.png\)/);
    assert.match(text, /- \[01-thread-light\.png\]\(\.\.\/shots\/\d{3}\/01-thread-light\.png\): thread, light/);
    assert.match(lab('report', 'no-such-prompt').error, /there are browser-tools, connectors, yap-code/);
  });

  test('set: a setting and files seeded into the scratch workspace (only there)', () => {
    const out = ok(lab('set', JSON.stringify({ web: true, files: [{ path: 'pictures/sun.png', png: { width: 120, height: 80, shape: 'circle' } }, { path: 'notes.md', text: '# Notes\n' }] })), 'set');
    assert.deepEqual(out.config, { webSearch: true });
    assert.deepEqual(out.files, ['pictures/sun.png', 'notes.md']);
    assert.match(lab('set', JSON.stringify({ files: [{ path: '../outside.txt', text: 'x' }] })).error, /isn't inside the workspace/);
    assert.match(lab('set', JSON.stringify({ config: { ppApiKeys: {} } })).error, /doesn't change ppApiKeys/);
  });

  test('run: a feature script passes; a wrong expect fails it with exit 1', () => {
    const file = path.join(LAB, 'check-run.json');
    const steps = [
      { fake: [{ tools: [{ name: 'browser_navigate', args: { url: RAIL } }] }, { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] }, { content: 'The 09:42 leaves from platform 2.' }] },
      { ask: `When does the 09:42 leave? ${RAIL}`, timeout: 120 },
      {
        expect: {
          status: 'answered',
          used: ['browser_navigate', 'browser_read'],
          notUsed: 'browser_click',
          refused: false,
          card: false,
          tool: { name: 'browser_read', args: { mode: 'elements' }, ok: true, result: 'Buy tickets' },
          answerContains: 'platform 2',
          pictures: 0,
          files: 0,
          visible: 'platform 2',
          selector: { window: 'main', css: '[data-testid="chat-panel-slot"]' },
        },
      },
      { shot: 'the page', window: 'page', theme: 'light' },
      { fake: [{ content: 'Still platform 2.' }] },
      { ask: 'And the 10:12?', chat: 'same', timeout: 60 },
      { expect: { answerContains: 'Still platform 2', notVisible: 'no such words' } },
      { expect: { used: 'browser_find' }, soft: true, note: 'A soft check only warns.' },
    ];
    fs.writeFileSync(file, JSON.stringify({ name: 'check-run', steps }));
    const passed = lab('run', file);
    assert.equal(passed.ok, true, JSON.stringify(passed.failures));
    assert.equal(passed.exit, 0);
    assert.equal(passed.failed, 0);
    assert.equal(passed.warnings.length, 1);
    assert.equal(passed.turns.length, 2);
    assert.equal(passed.steps.find((s) => s.step === 'ask' && s.n === 6).conversation, passed.steps[1].conversation);
    assert.ok(passed.steps[3].shots[0].endsWith('the-page-01-page-light.png'), JSON.stringify(passed.steps[3]));
    assert.deepEqual(pngSize(fs.readFileSync(passed.sheet.png)).width, 1600);
    // A wrong expect: the run fails, says which check and why, exits 1.
    fs.writeFileSync(file, JSON.stringify({ name: 'check-run', steps: [steps[0], { ...steps[1], shots: false }, { expect: { used: 'browser_click', answerContains: 'platform 9' } }] }));
    const failed = lab('run', file);
    assert.equal(failed.ok, false);
    assert.equal(failed.exit, 1);
    assert.deepEqual(failed.failures.map((f) => f.check), ['used browser_click', 'answer has platform 9']);
    assert.match(failed.failures[0].detail, /called: browser_navigate, browser_read/);
    assert.equal(failed.sheet, null, 'no shots, no sheet');
    // A script that needs what this lab wasn't started with says so.
    fs.writeFileSync(file, JSON.stringify({ name: 'check-run', needs: { connectors: true }, steps: [{ note: 'x' }] }));
    assert.match(lab('run', file).failures[0].error, /needs the lab started with --connectors on/);
  });

  test('sheet: one PNG 1600 wide of the turns asked for, and its page beside it', () => {
    const out = ok(lab('sheet', '--turns', '2-3', '--out', path.join(LAB, 'sheets', 'test-sheet.png')), 'sheet');
    assert.deepEqual(out.turns, [2, 3]);
    assert.deepEqual(pngSize(fs.readFileSync(out.png)), { width: 1600, height: out.height });
    assert.ok(out.height > 1000, String(out.height));
    const html = fs.readFileSync(out.html, 'utf8');
    assert.match(html, /<h2>Turn 2 /);
    // Its own copies of its shots, so the next run's shots can't replace them.
    assert.match(html, /<img src="test-sheet_files\/002-01-thread-light\.png"/);
    assert.ok(fs.existsSync(path.join(LAB, 'sheets', 'test-sheet_files', '002-01-thread-light.png')));
    assert.match(lab('sheet', '--turns', '99').error, /No shots for turns 99/);
  });

  test('logs, and stop leaves the lab folder', () => {
    const logs = ok(lab('logs', '--grep', 'test mode on'), 'logs');
    assert.equal(logs.lines.length, 1);
    const stopped = ok(lab('stop'), 'stop');
    assert.equal(stopped.stopped, true);
    assert.ok(fs.existsSync(path.join(stopped.dataDir, 'config.json')));
    assert.match(lab('state').error, /isn't running/);
  });

  test('the plan’s path on the stand-in for Claude Code: Claude Code’s own tool calls recorded', () => {
    ok(lab('start', '--fake', '--model', 'claude-plan', '--idle', '15'), 'start plan');
    const out = ok(lab('ask', 'What are the tide times at Example Bay?', '--timeout', '90'), 'ask plan');
    assert.equal(out.status, 'answered');
    assert.equal(out.model.provider, 'claude-plan');
    assert.deepEqual(out.tools.map((t) => [t.via, t.name, t.ok]), [['claude', 'WebSearch', true]]);
    assert.match(out.answer, /High tide is at 06:12/);
    ok(lab('stop'), 'stop');
  });
});
