// An agent's long report, whole (17 Nov 2026: desktop/src-tauri/src/agents/words.rs,
// chat/turn/report.rs, chat/tools/agents.rs `agent_status`). A Claude in one
// of the maintainer's chats: the report reached it cut, "…" where it
// stopped, and the last section ("What I couldn't do") never arrived — Yap
// kept 8,000 characters of a turn's words, told the chat's model 3,000 of
// them (agent_status 1,500), and nothing could read the rest.
//
// Claude Code is this build's own stand-in (`yap.exe fake-claude <script>`,
// ai/claude_code/fake.rs), its final words a ~16,000-character report whose
// last section names PELICAN; Chat's model is the scripted fake
// (support/fake-llm.js). Nobody's real Claude Code, account or files: the
// workspace is in this run's own folder.
//
// - The words are kept whole: agents.json their start (an older Yap reads
//   it), the whole in agents/<id>/words/1.md, and the agent's view (its tab,
//   from the report card's agent) shows all of it.
// - On the stand-in's 8K model the report tells their start, cut at a line,
//   says how much is left, how to read it (agent_status with turn and
//   offset) and where its later sections start; asked "what couldn't the
//   agent do?", the model reads the next part (within its room: how much is
//   left, where to go on), then that section alone by its offset, and
//   quotes the last section's word.
// - A model with room (200K) is told it all: the whole report in the marker
//   and in the report's own call.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide, agentTimeline } from './support/code-chats.js';

const HERE = path.join(RUNS, 'agents-long-report-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

/** The agent's report: ~16,000 characters, the unique word in its last section. */
const LONG = (() => {
  let s = '## What I did\n\n';
  for (let i = 0; i < 200; i++) s += `- Step ${i}: I lined up bed ${i} with the path and checked its watering schedule.\n`;
  s += "\n## What I couldn't do\n\nThe greenhouse sensor (PELICAN) didn't answer, so its readings are still missing.";
  return s;
})();
const LAST = "The greenhouse sensor (PELICAN) didn't answer, so its readings are still missing.";

const init = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: 'auto',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
const REPORTS = [
  { _wait_user: true },
  init,
  {
    type: 'assistant',
    message: { id: 'm1', type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content: [{ type: 'text', text: LONG }], stop_reason: 'end_turn' },
    parent_tool_use_id: null,
    session_id: '{{session_id}}',
  },
  {
    type: 'result',
    subtype: 'success',
    is_error: false,
    duration_ms: 1200,
    num_turns: 1,
    result: LONG,
    session_id: '{{session_id}}',
    total_cost_usd: 0.03,
    usage: { input_tokens: 5000, output_tokens: 4000, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
    permission_denials: [],
    terminal_reason: 'completed',
  },
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // A roomy model beside the stand-in's 8K one: its server says it's loaded at 200K.
      fake.setServerModels([{ id: 'fake-chat' }, { id: 'fake-roomy-chat' }]);
      fake.serveOllama({ loaded: { 'fake-roomy-chat': 200_000 } });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'agents-long-report', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm, model = 'fake-chat') => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model, apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
const agents = (yap) => yap.invoke('agent_list');

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

/** Conversation `id`'s messages once there are `n`. */
async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

/** An agent sent to garden-planner that ends with the long report, reported: the agent and its chat's messages. */
async function longReport(yap, main, fakeLlm, said) {
  await yap.invoke('e2e_claude_script', { script: writeScript('long-report', REPORTS) });
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { folder: 'garden-planner', brief: 'Line the beds up with the path and check the greenhouse sensor.' } }] },
    { content: "On it: I've sent an agent to garden-planner." },
    { plain: true, content: said },
  ]);
  const before = (await agents(yap)).length;
  await openView(main, 'Chat');
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'Send an agent to garden-planner: line the beds up with the path and check the greenhouse sensor.');
  await expect.poll(async () => (await agents(yap)).length, { message: 'an agent to start' }).toBe(before + 1);
  const id = (await agents(yap))[0].id;
  await expect.poll(async () => (await agents(yap)).find((a) => a.id === id)?.status, { timeout: 30_000 }).toBe('done');
  const agent = (await agents(yap)).find((a) => a.id === id);
  const messages = await messagesOf(yap, agent.conversation, 4);
  return { agent, messages };
}

/** The `agent_status` arguments a message gives to read on (a report's line, or a result's `next`), if any. */
function readOnFrom(message) {
  if (!message) return null;
  if (message.role === 'tool') {
    try {
      return JSON.parse(message.content).data?.agents?.[0]?.next ?? null;
    } catch {
      return null;
    }
  }
  const m = /Read the rest with agent_status (\{[^}]*\})/.exec(String(message.content ?? ''));
  return m ? JSON.parse(m[1]) : null;
}

/** Where this turn reads on from: its last agent_status result, else the report in the history. */
function readOn(record) {
  const last = record.messages.at(-1);
  if (last?.role === 'tool') return readOnFrom(last);
  const report = [...record.messages].reverse().find((m) => m.role === 'user' && String(m.content ?? '').startsWith('Report from the agent you started'));
  return readOnFrom(report);
}

/** The arguments that read `heading`'s section alone, from the turn's last agent_status result. */
function sectionOf(record, heading) {
  const last = record.messages.at(-1);
  const a = JSON.parse(last.content).data.agents[0];
  const s = (a.sections_after || []).find((x) => x.heading === heading);
  return { id: a.id, turn: a.words_turn, offset: s?.offset ?? 0 };
}

/** The agent's words this turn's agent_status calls brought back, in order. */
const partsRead = (record) =>
  record.messages
    .filter((m) => m.role === 'tool')
    .map((m) => {
      try {
        return JSON.parse(m.content).data?.agents?.[0]?.its_words ?? '';
      } catch {
        return '';
      }
    });

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  fs.rmSync(GARDEN, { recursive: true, force: true });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Beds\n');
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsAsk: 'never', agentsSpeak: 'never', webSearch: false, speakReplies: false, llmScopes: chatScope(fakeLlm) });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
});

test('a long report is kept whole; on a small model the chat reads the rest in parts and quotes its last section', async ({ yap, main, fakeLlm, shot }) => {
  const { agent, messages } = await longReport(yap, main, fakeLlm, 'Done: the beds are lined up with the path. The rest of its report is longer than I was told.');

  // Kept whole: the page's copy has every word, agents.json their start
  // (what an older Yap reads), the whole in the agent's own words file.
  expect(agent.text).toBe(LONG);
  expect(agent.turnList[0].text).toBe(LONG);
  expect(agent.turnList[0].words).toBe(LONG.length);
  const saved = JSON.parse(fs.readFileSync(path.join(yap.dataDir, 'agents.json'), 'utf8')).agents.find((a) => a.id === agent.id);
  expect(saved.turnList[0].text.length).toBeLessThanOrEqual(8_000);
  expect(saved.turnList[0].text.endsWith('…')).toBe(true);
  expect(saved.turnList[0].text).not.toContain('PELICAN');
  expect(saved.text).toBe(saved.turnList[0].text);
  expect(fs.readFileSync(path.join(yap.dataDir, 'agents', agent.id, 'words', '1.md'), 'utf8')).toBe(LONG);

  // The report on the stand-in's 8K model: their start, cut at a line, and
  // how much is left with how to read it.
  const marker = messages[2];
  expect(marker.kind).toBe('agentReport');
  expect(marker.text).toContain("Its own words: ## What I did");
  expect(marker.text).not.toContain('PELICAN');
  const goesOn = marker.text.split('\n').at(-1);
  const SECTION_AT = LONG.indexOf("## What I couldn't do");
  expect(goesOn).toMatch(/^\[The agent's words go on: [\d,]+ more characters, \d+% of them, aren't here\. Read the rest with agent_status \{"id": "[0-9a-f]+", "turn": 1, "offset": \d+\}\. Its sections there: "What I couldn't do" from offset (\d+)\.\]$/);
  expect(goesOn.endsWith(`"What I couldn't do" from offset ${SECTION_AT}.]`)).toBe(true);
  const args = readOnFrom({ role: 'user', content: goesOn });
  expect(args).toMatchObject({ id: agent.id, turn: 1 });
  // (Where it stopped: a line's end; the rest from the next line's start.)
  expect(LONG.slice(args.offset)).toMatch(/^- Step \d+:/);
  // The report's own call (no tools) has far more of them than the marker.
  const reportCall = fakeLlm.requests.filter((r) => r.tools.length === 0).at(-1);
  const told = String(reportCall.messages.at(-1).content);
  expect(told.startsWith('Report from the agent you started (not from the person): ')).toBe(true);
  expect(told.length).toBeGreaterThan(marker.text.length);
  expect(reportCall.system).toContain("don't guess the rest");

  // Asked what it couldn't do: the model reads on from where the report
  // stopped (the next part, within its room), then goes straight to the
  // section it needs by its offset, and quotes it.
  fakeLlm.scriptChat([
    { tools: [{ name: 'agent_status', args: (r) => readOn(r) }] },
    { tools: [{ name: 'agent_status', args: (r) => sectionOf(r, "What I couldn't do") }] },
    {
      content: (r) => {
        const line = partsRead(r).join('\n').split('\n').find((l) => l.includes('PELICAN')) ?? 'nothing about it';
        return `It couldn't finish one thing: ${line}`;
      },
    },
  ]);
  const calls = fakeLlm.requests.length;
  await send(main, "What couldn't the agent do?");
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText(`It couldn't finish one thing: ${LAST}`, { timeout: 30_000 });
  const last = fakeLlm.requests.slice(calls).filter((r) => r.tools.includes('agent_status')).at(-1);
  const [onwards, section] = last.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  // The next part: from where the report stopped, how much is left, where
  // to go on, and where each later section starts.
  expect(onwards.success).toBe(true);
  const a = onwards.data.agents[0];
  expect(a.words_turn).toBe(1);
  expect(a.words_from).toBe(args.offset);
  expect(LONG.slice(args.offset).startsWith(a.its_words)).toBe(true);
  expect(a.next).toMatchObject({ id: agent.id, turn: 1 });
  expect(a.words_left).toBe(LONG.length - a.next.offset);
  expect(a.sections_after.at(-1)).toEqual({ heading: "What I couldn't do", offset: SECTION_AT });
  expect(onwards.data.note).toContain('Its words go on');
  // That section alone, to the report's end.
  expect(section.success).toBe(true);
  const s = section.data.agents[0];
  expect(s.its_words.startsWith("## What I couldn't do")).toBe(true);
  expect(s.its_words.endsWith(LAST)).toBe(true);
  expect(s.next).toBeUndefined();
  await expect(thread(main).locator('.toolchips').last().locator('.toolchip')).toHaveText(['Checked more of the agent in garden-planner', 'Checked more of the agent in garden-planner']);
  await shot(main, 'read-on-in-parts');

  // The agent's view (its tab, at that turn): the whole report.
  await agentTimeline(main);
  await expect(main.getByTestId('chat-agent-text')).toContainText(LAST);
  await expect(main.getByTestId('chat-agent-text')).toContainText('Step 0: I lined up bed 0');
  await shot(main, 'agent-view-whole');
});

test('a model with room is told the whole report', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { llmScopes: chatScope(fakeLlm, 'fake-roomy-chat') });
  const { messages } = await longReport(yap, main, fakeLlm, "Done: the beds are lined up, but the greenhouse sensor (PELICAN) didn't answer.");
  const marker = messages[2];
  expect(marker.text).toContain(`Its own words: ${LONG}`);
  expect(marker.text).not.toContain("words go on");
  // The report's own call: all of it too.
  const reportCall = fakeLlm.requests.filter((r) => r.tools.length === 0).at(-1);
  expect(String(reportCall.messages.at(-1).content)).toContain(LAST);
  expect(messages[3].text).toContain('PELICAN');
});
