// Code chats and Yap's memory (desktop/src-tauri/src/agents/code_chat.rs, chat/turn/prompt.rs,
// chat/turn/plan.rs, memory/learn.rs; 7 Nov 2026): project knowledge lives with the project
// (its CLAUDE.md, Claude Code's own memory) and Yap's memory is personal, so a Code chat uses
// none of it — on every model — and is never learned from. About you keeps the name and the
// region there, never the note. On the Claude plan a Code chat runs as a Claude Code session in
// the workspace, with the person's own settings, read only.
//
// The model is the scripted fake (support/fake-llm.js), which records every request; Claude
// Code is this build's stand-in (`yap.exe fake-claude`, ai/claude_code/fake.rs), whose
// `_dump_args` writes the launch it was given to a file. All the people, notes and folders are
// fictional, in this run's own folder; nobody's real Claude Code, account or ~/.claude.
//
// - A Code chat's question (the fake model): none of memory's passages, its "You", how the
//   person wants answers, nor the notes the eager RAG finds; About you's name and region, not
//   its note; no remember, forget or search_chats. Nothing learned from it (no memory pass).
// - The same question in a plain chat: all of it, as before, and it's learned from.
// - A Code chat on the plan: Claude Code's launch in the workspace (`--tools` the read-only
//   built-ins, `--setting-sources user,project,local`, Yap's MCP server only, Claude Code's own
//   prompt with Yap's added, no slash commands), its prompt without memory or the note, Yap's
//   endpoint without search_memory; its transcript in the workspace's projects folder, gone
//   with the chat (an agent's session there stays). A plain plan chat as lean as before.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { fakeClaude, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'code-chats-memory-spec');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const SCRIPTS = path.join(HERE, 'scripts');
const ARGS = path.join(HERE, 'launches.jsonl');
// Claude Code's config folder for this run (never the person's ~/.claude).
const CLAUDE_CONFIG = path.join(HERE, 'claude-config');
const PROJECTS = path.join(CLAUDE_CONFIG, 'projects');

// (In the first person: a plain chat saying it is read for memory, `prompt::worth_a_pass`.)
const QUESTION = 'Where does my watering schedule for Aoife live?';
/** What memory, About you's note and the notes hold: none of it may reach a Code chat's model. */
const PERSONAL = ['Lives in Galway', 'Cousin', 'bullet points', 'allotment', 'Water the tomatoes'];

const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const init = (tools) => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools,
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: MODEL,
  permissionMode: 'dontAsk',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const answer = (text) => [
  ev({ type: 'message_start', message: { id: 'm1', type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  { type: 'assistant', message: { id: 'm1', type: 'message', role: 'assistant', model: MODEL, content: [{ type: 'text', text }], stop_reason: 'end_turn' }, parent_tool_use_id: null, session_id: SID },
  ev({ type: 'message_stop' }),
  {
    type: 'result',
    subtype: 'success',
    is_error: false,
    duration_ms: 400,
    num_turns: 1,
    result: text,
    session_id: SID,
    total_cost_usd: 0.001,
    usage: { input_tokens: 900, output_tokens: 12, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
    permission_denials: [],
    terminal_reason: 'completed',
  },
];
/** A plan chat's session: says what it was launched with (to ARGS), answers once in its folder. */
const LAUNCH = [{ _for: 'chat' }, { _wait_user: true }, { _dump_args: ARGS }, init(['Read', 'Grep', 'Glob']), ...answer('It runs in {{cwd}}.'), { _wait_user: true }];

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
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(GARDEN, { recursive: true });
      fs.writeFileSync(path.join(GARDEN, 'water.ts'), 'export const days = ["Tue"];\n');
      fs.mkdirSync(CLAUDE_CONFIG, { recursive: true });
      await use({
        name: 'code-chats-memory',
        env: {
          ...fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl'), { CLAUDE_CONFIG_DIR: CLAUDE_CONFIG, YAP_FAKE_CLAUDE_TRANSCRIPTS: '1' }),
          // Windows' region as a test run has it (ai/about_you.rs): the UK.
          YAP_E2E_LOCALE: JSON.stringify({ locale: 'en-GB', country: 'GB', currency: 'GBP', symbol: '£', metric: true, timeZone: 'GMT Standard Time' }),
        },
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          agentsEnabled: true,
          memoryLearnChats: true,
          memorySearchChats: true,
          aboutName: 'Imogen',
          aboutMe: 'I keep bees on an allotment',
          // Claude Code (the stand-in) found and signed in: the plan's key-store marker stays.
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const FAKE_CHAT = (fakeLlm) => ({ chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } });
/** The chat request that asked `text` (its system prompt, context and question, as sent). */
const askedWith = (fakeLlm, text) => fakeLlm.requests.filter((r) => Array.isArray(r.tools) && JSON.stringify(r.messages ?? []).includes(text)).at(-1);
const memoryPasses = (fakeLlm) => fakeLlm.requests.filter((r) => r.kind === 'memory').length;
/** Every launch the stand-in wrote, oldest first. */
const launches = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i < 0 ? undefined : args[i + 1];
};
const samePath = (a, b) => path.resolve(a).toLowerCase() === path.resolve(b).toLowerCase();
/** Every transcript the stand-in kept: `{ folder, file }`. */
const transcripts = () =>
  fs.existsSync(PROJECTS)
    ? fs.readdirSync(PROJECTS).flatMap((folder) => fs.readdirSync(path.join(PROJECTS, folder)).filter((f) => f.endsWith('.jsonl')).map((file) => ({ folder, file })))
    : [];

test.beforeAll(async ({ yap }) => {
  // What Yap remembers (fictional), and a note the eager RAG would find.
  await yap.invoke('memory_save_item', { item: { section: 'people', title: 'Aoife', summary: 'Cousin', details: ['Lives in Galway'] } });
  await yap.invoke('memory_save_item', { item: { section: 'preferences', title: '', summary: '', details: ['Likes answers as bullet points'] } });
  await yap.invoke('note_create', { title: 'Watering schedule', content: 'Water the tomatoes every Tuesday.' });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
});

test('a Code chat’s question carries none of Yap’s memory, About you without its note, and nothing is learned from it', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { llmScopes: FAKE_CHAT(fakeLlm) });
  fakeLlm.reply('memory', () => JSON.stringify({ changes: [{ action: 'add', section: 'people', name: 'Aoife', facts: ['Builds garden planners'] }] }));
  fakeLlm.scriptChat([{ content: 'It lives in water.ts.' }]);
  const sent = await yap.invoke('chat_send', { conversationId: null, text: QUESTION, codeFolder: 'garden-planner' });
  expect((await yap.invoke('code_chat_workspace_get', { conversation: sent.conversationId }))?.name).toBe('garden-planner');
  const asked = askedWith(fakeLlm, QUESTION);
  expect(asked, 'the chat request').toBeTruthy();
  const all = JSON.stringify(asked.messages);
  for (const never of PERSONAL) expect(all, never).not.toContain(never);
  expect(all).not.toContain('What Yap remembers');
  // About you: the name and the region (spelling, units), not the note.
  expect(all).toContain('Their name is Imogen.');
  expect(all).toContain("they're in the United Kingdom (British English)");
  expect(all).not.toContain('They told Yap');
  // It's a Code chat (its line), and its tools read meetings and notes on request — never memory's.
  expect(all).toContain("This is a Code chat in the user's folder garden-planner");
  expect(asked.tools).toEqual(expect.arrayContaining(['search_notes', 'get_note', 'start_agent']));
  for (const tool of ['remember', 'forget', 'search_chats']) expect(asked.tools, tool).not.toContain(tool);

  // A quiet Code chat is never read for memory: no pass, nothing kept.
  const passes = memoryPasses(fakeLlm);
  const before = yap.readJson('memory.json').items.length;
  const learned = await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  expect(learned.done).toEqual([]);
  expect(memoryPasses(fakeLlm)).toBe(passes);
  expect(yap.readJson('memory.json').items.length).toBe(before);
  expect(yap.readJson('memory.json').items.find((i) => i.title === 'Aoife').details).toEqual(['Lives in Galway']);
});

test('the same question in a plain chat carries Yap’s memory as before, and is learned from', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { llmScopes: FAKE_CHAT(fakeLlm) });
  fakeLlm.reply('memory', () => '{"changes": []}');
  fakeLlm.scriptChat([{ content: 'Your note says Tuesdays.' }]);
  const text = `${QUESTION} (a plain chat)`;
  const sent = await yap.invoke('chat_send', { conversationId: null, text });
  expect(await yap.invoke('code_chat_workspace_get', { conversation: sent.conversationId })).toBeNull();
  const all = JSON.stringify(askedWith(fakeLlm, text).messages);
  for (const kept of PERSONAL) expect(all, kept).toContain(kept);
  expect(all).toContain('They told Yap');
  expect(all).not.toContain('This is a Code chat');
  expect(askedWith(fakeLlm, text).tools).toEqual(expect.arrayContaining(['remember', 'forget', 'search_chats']));
  // Read for memory once it's quiet: a pass over what the person said.
  const passes = memoryPasses(fakeLlm);
  await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  expect(memoryPasses(fakeLlm)).toBe(passes + 1);
  expect(fakeLlm.requests.filter((r) => r.kind === 'memory').at(-1).user).toContain(text);
});

test('a Code chat on the plan runs Claude Code in its workspace, read only, with the person’s own settings', async ({ yap, main }) => {
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_script', { script: writeScript('launch', LAUNCH) });
  await yap.invoke('chat_set_model', { provider: 'claude-plan', model: MODEL, baseUrl: null, file: null });
  await expect.poll(async () => (await yap.invoke('get_config')).llmScopes.chat.provider).toBe('claude-plan');

  const sent = await yap.invoke('chat_send', { conversationId: null, text: 'How is this project laid out?', codeFolder: 'garden-planner' });
  expect(sent.reply ?? JSON.stringify(sent)).toContain('It runs in');
  const [launch] = launches();
  expect(launch, 'the stand-in wrote its launch').toBeTruthy();
  const args = launch.args;
  // In the workspace, as in the person's own terminal…
  expect(samePath(launch.cwd, GARDEN), launch.cwd).toBe(true);
  expect(after(args, '--setting-sources')).toBe('user,project,local');
  // …read only: the read-only built-ins alone (the web off), never asking.
  expect(after(args, '--tools')).toBe('Read,Grep,Glob');
  expect(after(args, '--permission-mode')).toBe('dontAsk');
  const allowed = after(args, '--allowedTools').split(',');
  for (const never of ['Edit', 'Write', 'MultiEdit', 'NotebookEdit', 'Bash', 'PowerShell']) expect(allowed, never).not.toContain(never);
  expect(allowed).toEqual(expect.arrayContaining(['mcp__yap__get_meeting', 'mcp__yap__search_notes', 'mcp__yap__start_agent']));
  expect(allowed).not.toContain('mcp__yap__search_memory');
  expect(args).toContain('--strict-mcp-config');
  expect(args).toContain('--disable-slash-commands');
  // Claude Code's own prompt (its project context), Yap's added: no memory, no note.
  expect(args).not.toContain('--system-prompt-file');
  const added = fs.readFileSync(after(args, '--append-system-prompt-file'), 'utf8');
  expect(added).toContain('Code chat in Yap');
  expect(added).toContain('garden-planner');
  expect(added).toContain('Their name is Imogen.');
  for (const never of PERSONAL) expect(added, never).not.toContain(never);
  expect(added).not.toContain('What Yap remembers');
  // Yap's MCP server only, and its run never offers memory.
  const mcp = JSON.parse(fs.readFileSync(after(args, '--mcp-config'), 'utf8'));
  expect(Object.keys(mcp.mcpServers)).toEqual(['yap']);
  const settings = JSON.parse(fs.readFileSync(after(args, '--settings'), 'utf8'));
  expect(settings.permissions.blockReadsOutsideWorkingDirectories).toBe(true);

  // Its session kept with the folder it ran in; Claude Code's copy of it in the workspace's
  // projects folder, beside an agent's session there (another id: theirs).
  const conv = String(sent.conversationId);
  await expect.poll(() => yap.readJson('claude_sessions.json')?.conversations?.[conv]?.cwd).toBeTruthy();
  const kept = yap.readJson('claude_sessions.json').conversations[conv];
  expect(samePath(kept.cwd, GARDEN)).toBe(true);
  const folderName = GARDEN.replace(/[^a-zA-Z0-9]/g, '-').toLowerCase();
  const mine = () => transcripts().filter((t) => t.file === `${kept.session}.jsonl`);
  await expect.poll(() => mine().map((t) => t.folder.toLowerCase())).toEqual([folderName]);
  const theirs = path.join(PROJECTS, mine()[0].folder, '1b2c3d4e-0000-4000-8000-00000000c0de.jsonl');
  fs.writeFileSync(theirs, '{}\n');
  // The chat deleted: its copy goes; the agent's stays.
  await yap.invoke('chat_delete', { id: sent.conversationId });
  await expect.poll(() => mine().length).toBe(0);
  expect(fs.existsSync(theirs)).toBe(true);
  expect(await yap.invoke('code_chat_workspace_get', { conversation: sent.conversationId })).toBeNull();

  // A plain plan chat: as lean as before, in Yap's own folder.
  await yap.invoke('chat_send', { conversationId: null, text: 'What is a cold frame?' });
  await expect.poll(() => launches().length).toBe(2);
  const plain = launches()[1];
  expect(samePath(plain.cwd, path.join(yap.dataDir, 'claude', 'chat')), plain.cwd).toBe(true);
  expect(after(plain.args, '--setting-sources')).toBe('');
  expect(after(plain.args, '--tools')).toBe('');
  expect(plain.args).toContain('--system-prompt-file');
  expect(plain.args).not.toContain('--disable-slash-commands');
  expect(after(plain.args, '--allowedTools').split(',')).toContain('mcp__yap__search_memory');
});
