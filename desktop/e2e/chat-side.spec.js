// A side question (desktop/src-tauri/src/chat/side.rs; views/chat/SideQuestion.svelte,
// sideQuestion.svelte.js, stores/sideQuestionKey.js; 15 Nov 2026, the Claude app's side chat):
//
// - Ctrl+; in Chat opens a small box over the thread, through the in-page hotkey fallback
//   (the global hook never fires while a Yap window has the focus).
// - The question goes to Chat's model with the thread as its history and no tools at all; the
//   answer streams in the box. Nothing is saved: chats.json never has it, the thread is as it
//   was, and memory's learning pass never reads it. Esc throws it away.
// - Asked while an answer in the chat is still being written, it's answered all the same (the
//   person's question still being answered is context, not a question); Keep it waits for the
//   chat's answer, then adds the question and the answer to the thread as an ordinary exchange.
// - In a Code chat working directly on the Claude plan, with its Claude mid-turn: the question
//   never enters the chat's Claude Code session (a one-shot run with no tools that keeps no
//   transcript answers it); kept, the session hears of it with its next message.
//
// The model is the scripted fake (support/fake-llm.js); Claude Code is this build's stand-in.
// Every person and detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm, SIDE_QUESTION } from './support/fake-llm.js';
import { fakeClaude, PLAN_CHAT } from './support/claude-plan.js';
import { newCodeChat } from './support/code-chats.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'chat-side-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const ARGS = path.join(HERE, 'args.jsonl');
const CLAUDE_CONFIG = path.join(HERE, 'claude-config');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

// (About the person: memory's learning pass reads a chat that says something about them.)
const FIRST = 'I keep an allotment. When do I sow runner beans?';
const FIRST_ANSWER = 'Mid-May, outdoors, once the frosts are over.';
const FIRST_2 = 'When should I plant out my courgettes?';
const SIDE = 'What does "hardening off" mean?';
const SIDE_ANSWER = 'Hardening off is getting seedlings used to the outdoors a few hours a day for a week or so.';

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
      fs.rmSync(HERE, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
      fs.mkdirSync(CLAUDE_CONFIG, { recursive: true });
      await use({
        name: 'chat-side',
        // (The Code chat's test loads its own script: e2e_claude_script.)
        env: fakeClaude(writeScript('idle', [{ _wait_user: true }]), { CLAUDE_CONFIG_DIR: CLAUDE_CONFIG, YAP_FAKE_CLAUDE_TRANSCRIPTS: '1' }),
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          memoryLearnChats: true,
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          llmScopes: { chat: fakeChat(fakeLlm) },
        },
      });
    },
    { scope: 'worker' },
  ],
});

function fakeChat(fakeLlm) {
  return { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };
}
const thread = (main) => main.locator('.msgs');
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const sideBox = (main) => main.getByTestId('chat-sideq');
const sideInput = (main) => main.getByTestId('chat-sideq-input');
const sideAnswer = (main) => main.getByTestId('chat-sideq-answer');
const keepIt = (main) => main.getByTestId('chat-sideq-keep');
const sideAsks = (fakeLlm) => fakeLlm.requests.filter((r) => r.kind === 'side');
const chatsFile = (yap) => JSON.stringify(yap.readJson('chats.json') ?? {});
const launches = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
/** Every transcript the stand-in kept (Claude Code's own sessions' files), as one text. */
function transcripts() {
  const out = [];
  const walk = (dir) => {
    if (!fs.existsSync(dir)) return;
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith('.jsonl')) out.push(fs.readFileSync(p, 'utf8'));
    }
  };
  walk(path.join(CLAUDE_CONFIG, 'projects'));
  return out.join('\n');
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

async function newChatWith(main, fakeLlm, question, answer) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  fakeLlm.scriptChat([{ content: answer }]);
  await box(main).fill(question);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText(answer);
  return main.evaluate((title) => window.__TAURI_INTERNALS__.invoke('chats_list').then((l) => l.find((c) => c.title === title)?.id), question);
}

async function askSide(main, question) {
  await main.keyboard.press('Control+;');
  await expect(sideBox(main)).toBeVisible();
  await expect(sideInput(main)).toBeFocused();
  await sideInput(main).fill(question);
  await sideInput(main).press('Enter');
}

test('Ctrl+; asks about the chat in a box over the thread: no tools, nothing saved, and Esc throws it away', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: fakeChat(fakeLlm) }, agentsEnabled: false });
  const id = await newChatWith(main, fakeLlm, FIRST, FIRST_ANSWER);
  const before = (await yap.invoke('chat_get', { id })).messages.length;
  fakeLlm.reply('side', () => SIDE_ANSWER);
  fakeLlm.streamPace('side', { gapMs: 60 });

  await askSide(main, SIDE);
  await expect(main.getByTestId('chat-sideq-question')).toHaveText(SIDE);
  await expect(sideAnswer(main)).toHaveText(SIDE_ANSWER);
  await expect(sideBox(main)).toContainText('Not added to the chat');
  await expect(keepIt(main)).toBeEnabled();
  await shot(main, '01-side-question-answered-light');

  // Chat's model, the thread as its history and the question last, with no tools.
  const asked = sideAsks(fakeLlm).at(-1);
  expect(asked.tools).toEqual([]);
  expect(asked.toolChoice).toBeNull();
  expect(asked.system).toContain(SIDE_QUESTION);
  expect(asked.system).toContain('You have no tools here');
  expect(asked.messages.map((m) => m.role)).toEqual(['system', 'user', 'assistant', 'user']);
  expect(asked.messages[1].content).toBe(FIRST);
  expect(asked.messages[3].content).toBe(SIDE);
  expect(asked.stream).toBe(true);

  // Nothing saved: not in the thread, nor chats.json.
  expect((await yap.invoke('chat_get', { id })).messages.length).toBe(before);
  await expect(thread(main).locator('.cbubble')).toHaveCount(before);
  expect(chatsFile(yap)).not.toContain(SIDE);
  expect(chatsFile(yap)).not.toContain(SIDE_ANSWER);

  // Esc: gone, and the focus back in the message box.
  await sideInput(main).press('Escape');
  await expect(sideBox(main)).toHaveCount(0);
  await expect(box(main)).toBeFocused();
  // Memory's learning pass over the chat never reads it.
  const memory = () => fakeLlm.requests.filter((r) => r.kind === 'memory');
  const n = memory().length;
  fakeLlm.reply('memory', () => '{"changes": []}');
  await yap.invoke('e2e_memory_learn', { conversationId: id });
  await expect.poll(() => memory().length).toBeGreaterThan(n);
  expect(JSON.stringify(memory().slice(n))).not.toContain(SIDE_ANSWER);
  expect(JSON.stringify(memory().slice(n))).not.toContain('hardening off');

  // ✕ closes it too; Ctrl+; again toggles it.
  await main.keyboard.press('Control+;');
  await expect(sideBox(main)).toBeVisible();
  await main.getByTestId('chat-sideq-close').click();
  await expect(sideBox(main)).toHaveCount(0);
  await main.keyboard.press('Control+;');
  await expect(sideBox(main)).toBeVisible();
  await main.keyboard.press('Control+;');
  await expect(sideBox(main)).toHaveCount(0);
});

test("asked while the chat's answer is still coming, it's answered; Keep it waits, then adds an ordinary exchange", async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: fakeChat(fakeLlm) }, agentsEnabled: false });
  const id = await newChatWith(main, fakeLlm, FIRST_2, FIRST_ANSWER);
  fakeLlm.reply('side', () => SIDE_ANSWER);
  fakeLlm.streamPace('side', null);

  // The chat's next answer held back while the side question is asked.
  let release;
  const gate = new Promise((r) => (release = r));
  fakeLlm.scriptChat([{ content: 'About 5 cm deep.', gate }]);
  await box(main).fill('And how deep?');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => fakeLlm.requests.some((r) => r.gated)).toBe(true);

  await askSide(main, SIDE);
  await expect(sideAnswer(main)).toHaveText(SIDE_ANSWER);
  // The question still being answered is context in the prompt, never the question.
  const asked = sideAsks(fakeLlm).at(-1);
  expect(asked.system).toContain('still being answered');
  expect(asked.system).toContain('And how deep?');
  expect(asked.messages.at(-1).content).toBe(SIDE);
  expect(asked.messages.map((m) => m.role)).toEqual(['system', 'user', 'assistant', 'user']);
  // Keep it waits for the chat's answer.
  await expect(keepIt(main)).toBeDisabled();
  await expect(main.getByTestId('chat-sideq-later')).toHaveText('Keep it once the answer in the chat is done.');
  await theme(yap, main, 'dark');
  await shot(main, '02-side-question-while-answering-dark');
  // (The chat's own answer is still coming: the side question took nothing from it.)
  release();
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText('About 5 cm deep.');
  await expect(keepIt(main)).toBeEnabled();
  await expect(main.getByTestId('chat-sideq-later')).toHaveCount(0);
  await shot(main, '03-side-question-keep-dark');

  await keepIt(main).click();
  await expect(sideBox(main)).toHaveCount(0);
  const kept = (await yap.invoke('chat_get', { id })).messages;
  expect(kept.map((m) => [m.role, m.text])).toEqual([
    ['user', FIRST_2],
    ['assistant', FIRST_ANSWER],
    ['user', 'And how deep?'],
    ['assistant', 'About 5 cm deep.'],
    ['user', SIDE],
    ['assistant', SIDE_ANSWER],
  ]);
  // Chat's model wrote it, and it's in the thread as any answer.
  expect(kept.at(-1).model?.id).toBe('fake-chat');
  await expect(thread(main).locator('.cbubble.user').last()).toContainText(SIDE);
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText(SIDE_ANSWER);
  expect(chatsFile(yap)).toContain(SIDE_ANSWER);
  await shot(main, '04-side-question-kept-dark');
  await theme(yap, main, 'light');
  await shot(main, '05-side-question-kept-light');

  // The next question has it in its history.
  fakeLlm.scriptChat([{ content: 'Yes, harden them off first.' }]);
  await box(main).fill('Should I do that for beans?');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText('Yes, harden them off first.');
  const next = fakeLlm.requests.filter((r) => r.kind !== 'side' && r.tools.length && JSON.stringify(r.messages).includes('Should I do that for beans?')).at(-1);
  expect(JSON.stringify(next.messages)).toContain(SIDE_ANSWER);
});

test("in a Code chat with its Claude mid-turn: never in the chat's session, and kept, its next message carries it", async ({ yap, main, fakeLlm, shot }) => {
  test.setTimeout(150_000);
  fs.rmSync(GARDEN, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { llmScopes: { chat: PLAN_CHAT }, agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', assistantScreen: false });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });

  const SID = '{{session_id}}';
  const init = (tools) => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: SID, tools, mcp_servers: [], model: 'claude-haiku-5-5', permissionMode: 'auto', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
  const said = (id, text, stop = 'end_turn') => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [{ type: 'text', text }], stop_reason: stop }, parent_tool_use_id: null, session_id: SID });
  const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
  const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 400, num_turns: 1, result: text, session_id: SID, total_cost_usd: 0.001, usage: { input_tokens: 900, output_tokens: 12, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
  const streamed = (text) => [
    ev({ type: 'message_start', message: { id: 'm1', type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [] } }),
    ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
    ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
    ev({ type: 'content_block_stop', index: 0 }),
    said('m1', text),
    ev({ type: 'message_stop' }),
    result(text),
  ];
  const DONE = 'Done: notes.md has a watering line.';
  const CODE_SIDE = 'Which file holds the schedule?';
  const CODE_ANSWER = 'notes.md, going by the chat so far.';
  // Its session works for a while (its words, then a long step); a one-shot question meanwhile answers the side question.
  await yap.invoke('e2e_claude_script', {
    script: writeScript('working', [
      { _for: 'agent' },
      { _wait_user: true },
      { _dump_args: ARGS },
      init(['Bash', 'Edit', 'Read', 'Write']),
      said('w1', "I'll add the watering schedule to notes.md.", 'tool_use'),
      { _sleep_ms: 12000 },
      said('w2', DONE),
      result(DONE),
      { _wait_user: true },
      { _for: 'ask' },
      { _wait_user: true },
      { _dump_args: ARGS },
      init([]),
      ...streamed(CODE_ANSWER),
      { _wait_user: true },
    ]),
  });

  await openView(main, 'Chat');
  await newCodeChat(main, 'garden-planner');
  await box(main).fill('Add a watering schedule');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(async () => (await yap.invoke('agent_list'))[0]?.status, { timeout: 20_000 }).toBe('running');
  const agent = (await yap.invoke('agent_list'))[0];
  const id = agent.conversation;

  await askSide(main, CODE_SIDE);
  await expect(sideAnswer(main)).toHaveText(CODE_ANSWER, { timeout: 20_000 });
  await expect(sideBox(main)).toContainText("Claude's session never sees it");
  // A one-shot run with no tools that keeps no transcript: never the chat's session.
  const ask = launches().find((l) => l.user_text?.includes(CODE_SIDE));
  expect(ask.args).toContain('--no-session-persistence');
  expect(ask.args[ask.args.indexOf('--tools') + 1]).toBe('');
  expect(ask.args).not.toContain('--resume');
  expect(ask.args).not.toContain(agent.session);
  // Claude is still at work, and Keep it waits for it.
  expect((await yap.invoke('agent_list'))[0].status).toBe('running');
  await expect(keepIt(main)).toBeDisabled();
  await shot(main, '06-code-chat-side-question-while-claude-works');

  await expect.poll(async () => (await yap.invoke('agent_list'))[0]?.status, { timeout: 40_000 }).toBe('done');
  // (Its final words are the chat's answer, after its work.)
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.at(-1)?.text ?? '', { timeout: 20_000 }).toBe(DONE);
  const before = (await yap.invoke('chat_get', { id })).messages.length;
  await expect(keepIt(main)).toBeEnabled();
  await keepIt(main).click();
  await expect(sideBox(main)).toHaveCount(0);
  const kept = (await yap.invoke('chat_get', { id })).messages;
  expect(kept.slice(before).map((m) => [m.role, m.text])).toEqual([
    ['user', CODE_SIDE],
    ['assistant', CODE_ANSWER],
  ]);
  // The session never had it (its own transcript has its brief).
  expect(transcripts()).toContain('Add a watering schedule');
  expect(transcripts()).not.toContain(CODE_SIDE);

  // Its next message carries it, after the person's words.
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_script', {
    script: writeScript('next', [{ _for: 'agent' }, { _wait_user: true }, { _dump_args: ARGS }, init(['Read']), said('b1', 'Got it.'), result('Got it.'), { _wait_user: true }]),
  });
  await box(main).fill('Make it weekly');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => launches().at(-1)?.user_text ?? '', { timeout: 30_000 }).toContain('Make it weekly');
  const words = launches().at(-1).user_text;
  expect(words.startsWith('Make it weekly')).toBe(true);
  expect(words).toContain('<earlier_in_this_chat>');
  expect(words).toContain(`User: ${CODE_SIDE}`);
  expect(words).toContain(`Assistant: ${CODE_ANSWER}`);
  // The thread shows the person's own words only.
  await expect(thread(main).locator('.cbubble.user').last()).toHaveText(/^\s*Make it weekly\s*$/);
  await expect.poll(async () => (await yap.invoke('agent_list'))[0]?.status, { timeout: 30_000 }).toBe('done');
});
