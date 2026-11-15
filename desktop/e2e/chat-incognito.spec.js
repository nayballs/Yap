// Incognito chats (desktop/src-tauri/src/chat/incognito.rs, chat/chats.rs, chat/turn/prompt.rs,
// chat/turn/plan.rs, memory/learn.rs; views/chat/chatState.svelte.js, ChatView.svelte,
// ChatThread.svelte; 15 Nov 2026, the Claude app's ghost from Anthropic's help pages):
//
// - A new chat's ghost (top right of the thread) turns it on: the empty thread says what it
//   means, a ring goes round the thread, and its first question starts an incognito chat.
// - Never saved (chats.json never has it), never listed; its question carries none of Yap's
//   memory and no remember / forget / search_chats (About you whole: a setting); nothing is
//   learned from it. The band over the thread says so.
// - Left (New chat): it's gone — Rust no longer has it — and the next new chat starts kept.
// - On the Claude plan: Claude Code saves nothing of it (`--no-session-persistence`), and
//   claude_sessions.json never names it.
//
// The model is the scripted fake (support/fake-llm.js); Claude Code is this build's stand-in.
// Every person and detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { fakeClaude, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'chat-incognito-spec');
const ARGS = path.join(HERE, 'launches.jsonl');
const CLAUDE_CONFIG = path.join(HERE, 'claude-config');
const QUESTION = 'What should I ask the dentist about a cracked molar?';
/** What memory holds (fictional): none of it may reach an incognito chat's model. */
const MEMORY = ['Lives in Galway', 'bullet points'];

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
      fs.mkdirSync(CLAUDE_CONFIG, { recursive: true });
      await use({
        name: 'chat-incognito',
        env: fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl'), { CLAUDE_CONFIG_DIR: CLAUDE_CONFIG, YAP_FAKE_CLAUDE_TRANSCRIPTS: '1' }),
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          memoryLearnChats: true,
          memorySearchChats: true,
          aboutName: 'Imogen',
          aboutMe: 'I keep bees on an allotment',
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const FAKE_CHAT = (fakeLlm) => ({ chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } });
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const ghost = (main) => main.getByTestId('chat-incognito');
const askedWith = (fakeLlm, text) => fakeLlm.requests.filter((r) => Array.isArray(r.tools) && JSON.stringify(r.messages ?? []).includes(text)).at(-1);
const memoryPasses = (fakeLlm) => fakeLlm.requests.filter((r) => r.kind === 'memory').length;
const launches = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const chatsFile = (yap) => JSON.stringify(yap.readJson('chats.json') ?? {});

// The Claude plan's stand-in (ai/claude_code/fake.rs): says what it was launched with (to ARGS), answers once.
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const init = { type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: SID, tools: [], mcp_servers: [{ name: 'yap', status: 'connected' }], model: MODEL, permissionMode: 'dontAsk', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] };
const answer = (text) => [
  ev({ type: 'message_start', message: { id: 'm1', type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  { type: 'assistant', message: { id: 'm1', type: 'message', role: 'assistant', model: MODEL, content: [{ type: 'text', text }], stop_reason: 'end_turn' }, parent_tool_use_id: null, session_id: SID },
  ev({ type: 'message_stop' }),
  { type: 'result', subtype: 'success', is_error: false, duration_ms: 400, num_turns: 1, result: text, session_id: SID, total_cost_usd: 0.001, usage: { input_tokens: 900, output_tokens: 12, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' },
];
const LAUNCH = [{ _for: 'chat' }, { _wait_user: true }, { _dump_args: ARGS }, init, ...answer('Ask about a crown.'), { _wait_user: true }];
function writeScript(name, lines) {
  const dir = path.join(HERE, 'scripts');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test.beforeAll(async ({ yap }) => {
  await yap.invoke('memory_save_item', { item: { section: 'people', title: 'Aoife', summary: 'Cousin', details: ['Lives in Galway'] } });
  await yap.invoke('memory_save_item', { item: { section: 'preferences', title: '', summary: '', details: ['Likes answers as bullet points'] } });
});

test('the ghost starts an incognito chat: never saved, listed or remembered, and gone once left', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: FAKE_CHAT(fakeLlm) });
  fakeLlm.reply('memory', () => '{"changes": []}');
  // A kept chat first, so the list has something.
  fakeLlm.scriptChat([{ content: 'Hello there.' }]);
  await yap.invoke('chat_send', { conversationId: null, text: 'Hello, a kept chat' });

  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(ghost(main)).toHaveAttribute('aria-pressed', 'false');
  await ghost(main).click();
  await expect(ghost(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(main.getByTestId('chat-incognito-empty')).toContainText("It isn't saved to your chats, and memory isn't used or updated.");
  await expect(main.locator('section.thread')).toHaveClass(/incognito/);
  await shot(main, '01-new-incognito-chat');

  fakeLlm.scriptChat([{ content: 'Ask whether it needs a crown, and what it costs.' }]);
  await box(main).fill(QUESTION);
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('needs a crown');
  await expect(main.getByTestId('chat-incognito-line')).toContainText('Incognito chat · not saved, not remembered');
  // The ghost button is a new chat's only.
  await expect(ghost(main)).toHaveCount(0);
  await shot(main, '02-incognito-answered');
  await theme(yap, main, 'dark');
  await shot(main, '03-incognito-answered-dark');
  await theme(yap, main, 'light');

  // Never listed, never saved.
  await expect(main.locator('.conv')).toHaveText(['Hello, a kept chat']);
  await expect.poll(() => chatsFile(yap)).toContain('Hello, a kept chat');
  expect(chatsFile(yap)).not.toContain('dentist');
  const listed = await yap.invoke('chats_list');
  expect(listed.map((c) => c.title)).toEqual(['Hello, a kept chat']);

  // Its question: no memory, none of memory's tools; About you whole.
  const asked = askedWith(fakeLlm, QUESTION);
  expect(asked, 'the chat request').toBeTruthy();
  const all = JSON.stringify(asked.messages);
  for (const never of MEMORY) expect(all, never).not.toContain(never);
  expect(all).toContain('They told Yap');
  for (const tool of ['remember', 'forget', 'search_chats']) expect(asked.tools, tool).not.toContain(tool);

  // Never learned from: no pass, nothing kept.
  const [ghostId] = await yap.invoke('e2e_chat_incognito_ids');
  expect(ghostId, 'the incognito chat').toBeTruthy();
  const passes = memoryPasses(fakeLlm);
  const learned = await yap.invoke('e2e_memory_learn', { conversationId: ghostId });
  expect(learned.done).toEqual([]);
  expect(memoryPasses(fakeLlm)).toBe(passes);
  expect(JSON.stringify(yap.readJson('memory.json'))).not.toContain(String(QUESTION));

  // Left for a new chat: it's gone, and the new one starts kept.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(ghost(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(main.locator('section.thread')).not.toHaveClass(/incognito/);
  await expect.poll(async () => (await yap.invoke('e2e_chat_incognito_ids')).length).toBe(0);
  await expect(yap.invoke('chat_get', { id: ghostId })).rejects.toThrow();
  expect(chatsFile(yap)).not.toContain('dentist');
});

test('on the Claude plan, Claude Code saves nothing of an incognito chat', async ({ yap }) => {
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_script', { script: writeScript('launch', LAUNCH) });
  await yap.invoke('chat_set_model', { provider: 'claude-plan', model: MODEL, baseUrl: null, file: null });
  await expect.poll(async () => (await yap.invoke('get_config')).llmScopes.chat.provider).toBe('claude-plan');

  const sent = await yap.invoke('chat_send', { conversationId: null, text: 'A private question about a molar', incognito: true });
  expect(sent.reply ?? JSON.stringify(sent)).toContain('Ask about a crown');
  const [launch] = launches();
  expect(launch, 'the stand-in wrote its launch').toBeTruthy();
  expect(launch.args).toContain('--no-session-persistence');
  // Its session is never in claude_sessions.json; the chat never in chats.json.
  expect(JSON.stringify(yap.readJson('claude_sessions.json') ?? {})).not.toContain(`"${sent.conversationId}"`);
  expect(chatsFile(yap)).not.toContain('molar');
  // Ended: gone; a second end finds nothing.
  expect(await yap.invoke('chat_incognito_end', { id: sent.conversationId })).toBe(true);
  expect(await yap.invoke('chat_incognito_end', { id: sent.conversationId })).toBe(false);

  // A kept plan chat: saved by Claude Code as before.
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_script', { script: writeScript('launch', LAUNCH) });
  await yap.invoke('chat_send', { conversationId: null, text: 'What is a cold frame?' });
  const [kept] = launches();
  expect(kept.args).not.toContain('--no-session-persistence');
});
