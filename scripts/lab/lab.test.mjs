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
import { REPO, buildFresh, labConfig, onOff, parseArgs, parseModel } from './options.mjs';
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
  });

  test('cards: each as it is now, a connector’s with Always allow; Claude Code’s prompts and plans', () => {
    const cards = cardsOf(items, { approvals: [{ id: 3, tool: 'Bash', summary: 'Run a command: npm test', detail: 'npm test', run: 'a1' }], agents: [{ id: 'a2', folder: 'lab-sandbox', status: 'done', plan: true, planAllowed: false, text: 'The plan' }] });
    assert.deepEqual(cards.map((c) => [c.id, c.kind, c.state, c.answers.join('|')]), [
      ['confirm:9', 'page', 'refused', 'allow|deny'],
      ['confirm:10', 'connector', 'asking', 'allow|always|deny'],
      ['approve:3', 'approve', 'asking', 'allow|deny'],
      ['plan:a2', 'plan', 'asking', 'allow|deny'],
    ]);
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
  });
});

// ---------------------------------------------------------------- a fake run

const LAB = path.join(REPO, 'test-results', 'lab-test');
const ENV = { ...process.env, YAP_LAB_DIR: LAB };

/** `npm run lab -- <args>` in the test's own lab folder: its JSON. */
function lab(...args) {
  const r = spawnSync(process.execPath, [path.join(REPO, 'scripts', 'lab', 'lab.mjs'), ...args], { cwd: REPO, env: ENV, encoding: 'utf8', timeout: 600_000 });
  const line = r.stdout.trim().split('\n').at(-1) ?? '';
  try {
    return JSON.parse(line);
  } catch {
    throw new Error(`lab ${args[0]} printed no JSON (exit ${r.status}): ${r.stdout}\n${r.stderr}`);
  }
}
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

  test('a card: the turn waits, the lab answers Don’t, the click is refused', () => {
    script([{ tools: [{ name: 'browser_navigate', args: { url: RAIL } }] }, { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] }, { tools: [{ name: 'browser_click', args: { ref: 1 } }] }, { content: 'I didn’t buy it.' }]);
    const asked = ok(lab('ask', `Buy me a ticket for the 09:42: ${RAIL}`, '--timeout', '120'), 'ask');
    assert.equal(asked.status, 'waiting');
    const card = asked.cards.find((c) => c.state === 'asking');
    assert.deepEqual([card.kind, card.label, card.site, card.does], ['page', 'Buy tickets', 'rail.example.com', 'buy']);
    assert.deepEqual(asked.pending, [card.id]);
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
    assert.match(lab('report', 'no-such-prompt').error, /there are browser-tools, connectors, yap-code/);
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
