// A model on this PC whose chat format is strict (src-tauri/src/ai/llm.rs
// `taking_turns`, chat/tools.rs `supports_tools`, ai/local_llm.rs
// `template_takes_tools`), 2026-10-27: Gemma 2 9B's format has no place for
// tools and refuses a conversation that doesn't take turns — llama.cpp
// answered HTTP 400 for a replayed search, Yap kept the question with no
// answer, and every later question in that chat sent two questions in a
// row and was refused too.
// - Gemma 2 gets none of Chat's tools ("no note tools" on the chip); a
//   conversation its format refuses is asked again taking turns — a
//   question that got no answer before goes with the next one as one
//   message — and every later one to it goes so: the chat answers again.
// - Any other model on this PC: its chat format says for itself (llama.cpp's
//   `/props`) once it has loaded; one with no place for tools gets none.
//
// The model files are empty stand-ins (support/local-llm.js), and the started
// model is a fake AI on 127.0.0.1 (YAP_E2E_LOCAL_LLM) that answers `/props`
// and refuses as llama.cpp does (support/fake-llm.js `strictTurns`).
import { test as base, expect, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { installStandInModel } from './support/local-llm.js';
import { launchYap } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const GEMMA = 'gemma-2-9b-it-Q4_K_M.gguf';
const OTHER = 'example-chat-12b-it-Q4_K_M.gguf';
const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };
/** What llama.cpp's `/props` said about Gemma 2 9B's chat format. */
const GEMMA_CAPS = { supports_system_role: false, supports_tools: false, supports_tool_calls: false, supports_parallel_tool_calls: false, supports_string_content: true };

// A chat whose last question got no answer (the model refused it).
const TITLE = 'Laptop fan';
const CHATS = {
  conversations: [
    {
      id: 3,
      title: TITLE,
      createdTs: NOW - 600,
      updatedTs: NOW - 300,
      messages: [
        { role: 'user', text: 'My laptop fan is loud. What should I check first?', ts: NOW - 600 },
        { role: 'assistant', text: 'Check the vents for dust, then which app keeps the processor busy.', ts: NOW - 590 },
        { role: 'user', text: 'Can you look up why the air flow is so weak?', ts: NOW - 300 },
      ],
    },
  ],
};

const test = base.extend({
  fake: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      fake.strictTurns(true);
      fake.reply('other', () => 'Weak air flow is usually dust in the intake: clean the vents with short bursts of air.');
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [async ({ fake }, use) => use(options(fake, GEMMA, 'chat-format')), { scope: 'worker' }],
});

/** A Yap whose Chat is on the on-device `model` (an empty stand-in), answered by `fake`. */
function options(fake, model, name) {
  return {
    name,
    env: { YAP_E2E_GPU: 'NVIDIA GeForce RTX 4070,12282', YAP_E2E_LOCAL_LLM: fake.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '200' },
    data: { 'chats.json': CHATS },
    config: { postProcessEnabled: false, ppProvider: 'ondevice', ppLocalModel: model, llmScopes: { chat: ON_DEVICE_CHAT } },
    prepare: (dir) => installStandInModel(dir, model),
  };
}

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const lastAnswer = (main) => main.locator('.cbubble.assistant').last();
const chatCalls = (fake) => fake.requests.filter((r) => r.kind === 'other');
const textOf = (c) => (Array.isArray(c) ? c.filter((p) => p.type === 'text').map((p) => p.text).join('\n') : String(c ?? ''));

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test.describe('Gemma 2 9B on this PC', () => {
  test('gets no tools, and a question that got no answer goes with the next: the chat answers again', async ({ yap, main, fake, shot }) => {
    fake.setTemplateCaps(GEMMA_CAPS);
    await openView(main, 'Chat');
    await main.locator('.conv').filter({ hasText: TITLE }).click();
    await expect(main.locator('.modelchip .chip')).toContainText('no note tools');
    const before = chatCalls(fake).length;
    await send(main, 'Try again, please.');
    await expect(lastAnswer(main)).toContainText('Weak air flow is usually dust in the intake');
    await shot(main, '01-gemma-answers-again');

    // Refused once as Yap keeps it (two questions in a row), then asked
    // again taking turns: the question that got no answer went with this
    // one, the earlier first. No tools offered either time.
    const refused = fake.requests.filter((r) => r.kind === 'formatRefused');
    expect(refused.map((r) => r.roles)).toEqual([['user', 'assistant', 'user', 'user']]);
    const asked = chatCalls(fake).slice(before);
    expect(asked).toHaveLength(1);
    expect(asked[0].tools).toEqual([]);
    const turns = asked[0].messages.filter((m) => m.role !== 'system');
    expect(turns.map((m) => m.role)).toEqual(['user', 'assistant', 'user']);
    const question = textOf(turns[2].content);
    expect(question).toContain('Can you look up why the air flow is so weak?');
    expect(question).toContain('Try again, please.');
    expect(question.indexOf('air flow is so weak')).toBeLessThan(question.indexOf('Try again, please.'));
    // Kept as asked: two questions, then the answer.
    const conv = yap.readJson('chats.json').conversations.find((c) => c.id === 3);
    expect(conv.messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'user', 'assistant']);

    // The next question goes taking turns from the start: nothing refused.
    await send(main, 'And if cleaning the vents does not help?');
    await expect(main.locator('.cbubble.assistant')).toHaveCount(3);
    await expect(main.getByRole('button', { name: 'Stop the answer' })).toHaveCount(0);
    expect(fake.requests.filter((r) => r.kind === 'formatRefused')).toHaveLength(1);
    const next = chatCalls(fake).slice(before + 1);
    expect(next).toHaveLength(1);
    expect(next[0].messages.filter((m) => m.role !== 'system').map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'user']);
  });
});

test("another model on this PC whose chat format has no place for tools gets none once it's loaded", async ({ fake }) => {
  // By its size, a 12B would get them: its format says otherwise.
  const other = await launchYap(options(fake, OTHER, 'chat-format-other'));
  try {
    expect(await other.invoke('ai_model_status', { scope: 'chat' })).toMatchObject({ tools: true });
    fake.setTemplateCaps({ ...GEMMA_CAPS, supports_system_role: true });
    const before = chatCalls(fake).length;
    const res = await other.invoke('chat_send', { conversationId: null, text: 'What should I check on a loud laptop fan?' });
    expect(res.reply).toContain('Weak air flow is usually dust in the intake');
    const asked = chatCalls(fake).slice(before);
    expect(asked).toHaveLength(1);
    expect(asked[0].tools).toEqual([]);
    expect(await other.invoke('ai_model_status', { scope: 'chat' })).toMatchObject({ tools: false });
  } finally {
    await other.stop();
  }
});
