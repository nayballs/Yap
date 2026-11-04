// Long chats sized to the model (src-tauri/src/chat/budget.rs, compact.rs;
// src/lib/views/chat/ChatThread.svelte, Composer.svelte, SummaryDialog.svelte,
// ContextPanel.svelte), 2026-10-25:
// - A long chat fills the stand-in model's small context (8K): after an
//   answer, in the background, its older messages are summarised by the
//   chat's own model; a thin divider says so where the summary takes over,
//   "Show summary" opens it read-only, the ring drops and its panel says
//   "Summarised"; the next question carries the summary and the latest
//   messages — never the old ones.
// - A paste too long for the model on its own is refused before it's sent,
//   said plainly under the box ("That's too long for GPT-OSS 20B (8K
//   context)…"), Send waiting.
// - A provider's "too long" refusal (llama.cpp's own JSON) is said plainly
//   with what helps — New chat, Shorten what you pasted — never raw JSON.
// - The Voice Assistant's follow-up in a summarised chat carries the summary
//   and the latest messages, within its own smaller budget.
// - A meeting's Ask thread in Notes: its earlier turns go with the next
//   question from Rust (the thread's summary and the latest turns).
// - A long chat on Claude Haiku 5.5 (1M of context, but a prompt over 100K
//   tokens costs five times as much) stays under that price line: the latest
//   messages that fit and a summary, never the whole chat (budget.rs
//   `request_cap`, 2026-10-27).
// Nothing leaves the PC: every provider is the local fake
// (support/fake-llm.js) behind a test key. Fictional details only.
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const TEST_KEY = 'gsk_FAKElong0123456789abcXYZ';
const NOW = Math.floor(Date.now() / 1000);

/** A model as Groq's /openai/v1/models lists one (trimmed). */
const groqModel = (id, name, context) => ({
  id,
  object: 'model',
  created: 1754407957,
  owned_by: 'Fake',
  active: true,
  context_window: context,
  name,
  input_modalities: ['text'],
  output_modalities: ['text'],
  supported_features: ['tools', 'json_mode'],
});

/**
 * About `n` characters of plain planning talk, starting with `lead`: each
 * sentence its own (numbered), since an answer that says the same sentence
 * over and over is a loop Yap stops (chat/stream.rs `repeating_from`).
 */
const padded = (lead, n) => {
  const things = ['the coaches', 'the rooms', 'the caterer', 'the speakers', 'the badges', 'the rain plan', 'the signage', 'the parking'];
  let s = lead;
  for (let i = 1; s.length < n; i++) {
    s += ` Item ${i}: check ${things[i % things.length]} by day ${(i * 7) % 31 + 1} and note who owns it.`;
  }
  return s.slice(0, n);
};

// A seeded chat for the Voice Assistant: 12 long messages, the first 8
// summarised already.
const HARBOUR = 'Harbour trip';
const harbourMessages = Array.from({ length: 12 }, (_, i) => ({
  role: i % 2 ? 'assistant' : 'user',
  text: i === 0 ? padded('Our harbour trip budget is £6,200 and Ines Varga books the boat.', 1_200) : padded(`Harbour point ${i}.`, 1_200),
  ts: NOW - 3_600 + i,
}));
// A meeting with an Ask thread in Notes, its first turns summarised.
const SUPPLIER_THREAD = Array.from({ length: 8 }, (_, i) => ({
  role: i % 2 ? 'assistant' : 'user',
  text: i === 0 ? 'Who signs off the packaging order?' : padded(`Supplier point ${i}.`, 2_800),
  ts: NOW - 1_800 + i,
}));
// A chat far longer than Claude Haiku 5.5's cheaper rate: 40 messages of
// ~15,000 characters (~170K tokens in all).
const GARDEN = 'Allotment year plan';
const gardenMessages = Array.from({ length: 40 }, (_, i) => ({
  role: i % 2 ? 'assistant' : 'user',
  text: i === 0 ? padded('Our allotment plot is number 14 and the greenhouse goes on the north side.', 15_000) : padded(`Allotment point ${i}.`, 15_000),
  ts: NOW - 7_200 + i,
}));
const CHATS = {
  conversations: [
    {
      id: 9,
      title: GARDEN,
      createdTs: NOW - 7_200,
      updatedTs: NOW - 7_100,
      messages: gardenMessages,
    },
    {
      id: 7,
      title: HARBOUR,
      createdTs: NOW - 3_600,
      updatedTs: NOW - 3_000,
      messages: harbourMessages,
      summary: { text: '- Harbour trip for the design team.\n- Budget £6,200; Ines Varga books the boat.', upto: 8, after: 12, ts: NOW - 3_000 },
    },
    {
      id: 8,
      title: 'Supplier call thread',
      createdTs: NOW - 1_800,
      updatedTs: NOW - 1_700,
      note: 1,
      messages: SUPPLIER_THREAD,
      summary: { text: '- Rafael Okoye signs off the packaging order.', upto: 2, after: 8, ts: NOW - 1_700 },
    },
  ],
};
const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 7_200,
      updatedTs: NOW - 7_200,
      content: 'Packaging order: 4,000 boxes, delivery in March.',
      transcript: [
        { source: 'them', text: 'The packaging order is ready to sign.', ts: NOW - 7_200 },
        { source: 'you', text: 'Great, send it over this week.', ts: NOW - 7_180 },
      ],
    },
  ],
};

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // A small context on the stand-in GPT-OSS 20B, so a chat fills it.
      fake.listModels('groq', [groqModel('openai/gpt-oss-20b', 'GPT OSS 20B', 8192)]);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-long',
        env: { YAP_MODELS_STANDIN: fakeLlm.modelsStandin, YAP_E2E_NET_ALIASES: `${new URL(fakeLlm.base).host}=api.groq.com` },
        data: { 'chats.json': CHATS, 'notes.json': NOTES },
        config: {
          postProcessEnabled: true,
          ppProvider: 'groq',
          ppBaseUrl: fakeLlm.base,
          ppModel: 'openai/gpt-oss-20b',
          ppApiKey: TEST_KEY,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: {
            chat: { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model: 'openai/gpt-oss-20b', apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const ring = (main) => main.getByTestId('context-ring');
const panel = (main) => main.getByRole('dialog', { name: 'Context and AI usage' });
const divider = (main) => main.getByTestId('chat-summary-divider');
const tooLong = (main) => main.getByTestId('chat-too-long');
const lastAnswer = (main) => main.locator('.cbubble.assistant').last();
const textOf = (c) => (Array.isArray(c) ? c.filter((p) => p.type === 'text').map((p) => p.text).join('\n') : String(c ?? ''));
/** A request's messages besides its system prompt, as text. */
const history = (req) => req.messages.filter((m) => m.role !== 'system').map((m) => textOf(m.content));
const chatCalls = (fake) => fake.requests.filter((r) => r.kind === 'other');

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The answers' counts, as the fake sends them. */
const counts = (prompt, answer) => ({ usage: { prompt_tokens: prompt, completion_tokens: answer, total_tokens: prompt + answer } });

async function freshChat(yap, main) {
  // Groq's list (through the stand-in): GPT-OSS 20B with an 8K context.
  await yap.invoke('llm_models', { provider: 'groq', scope: 'chat', force: true });
  await openView(main, 'Chat');
  await main.locator('.convs').getByRole('button', { name: 'New chat', exact: true }).click();
}

test('a long chat is summarised in the background; the next question carries the summary and the latest messages', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(yap, main);
  // Long answers that fill the 8K context, counted near its top.
  fakeLlm.reply('other', () => padded('Here is a plan.', 2_400));
  fakeLlm.usage('other', counts(6_000, 400));
  const first = padded('Our offsite budget is £18,400 and Marta Quill is organising it.', 400);
  const asked = [first];
  await send(main, first);
  await expect(lastAnswer(main)).toContainText('Here is a plan.');
  // More questions until the chat outgrows the room: a summary request in
  // the background, then the divider where the summary takes over.
  for (let i = 2; i <= 8; i++) {
    // (A summary on its way is let finish: the next question would set it aside.)
    await divider(main).waitFor({ timeout: 1_500 }).catch(() => {});
    if (await divider(main).isVisible()) break;
    const q = padded(`Question ${i}: what about the venue, the agenda and the dinners?`, 400);
    asked.push(q);
    await send(main, q);
    await expect(main.locator('.cbubble.user')).toHaveCount(i);
    await expect(main.getByRole('button', { name: 'Stop the answer' })).toHaveCount(0);
  }
  await expect(divider(main)).toBeVisible({ timeout: 10_000 });
  const summaries = fakeLlm.requests.filter((r) => r.kind === 'chatSummary');
  expect(summaries.length).toBeGreaterThanOrEqual(1);
  // The summariser read the oldest messages, the first question among them.
  expect(summaries[0].user).toContain('Our offsite budget is £18,400');
  expect(summaries[0].user).toContain('Notes so far:\n(none yet)');
  await expect(divider(main)).toContainText("Earlier messages summarised to keep this chat within the model's memory");
  // Kept with the conversation in chats.json: the text, what it covers, its model.
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title.startsWith('Our offsite budget'));
  expect(conv.summary.upto).toBeGreaterThan(0);
  expect(conv.summary.text).toContain('Our offsite budget is £18,400');
  expect(conv.summary.model).toMatchObject({ provider: 'groq', id: 'openai/gpt-oss-20b' });
  await divider(main).scrollIntoViewIfNeeded();
  await shot(main, '01-summary-divider');

  // Show summary: read-only, the notes the model gets in their place.
  await divider(main).getByRole('button', { name: 'Show summary' }).click();
  const dialog = main.getByTestId('chat-summary-dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId('chat-summary-text')).toContainText('Our offsite budget is £18,400');
  await expect(dialog).toContainText('Summarised by GPT-OSS 20B');
  await shot(main, '02-show-summary');
  await main.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(divider(main).getByRole('button', { name: 'Show summary' })).toBeFocused();

  // The ring dropped from the last answer's 78% to the next request's
  // estimate, and its panel says the chat is summarised.
  await expect.poll(async () => Number(await ring(main).getAttribute('data-percent'))).toBeLessThan(78);
  await ring(main).getByRole('button').click();
  await expect(panel(main).getByTestId('context-summarised')).toHaveText("Summarised · the chat stays within the model's context");
  await expect(panel(main).getByTestId('context-long')).toHaveCount(0);
  await shot(main, '03-ring-summarised');
  await main.keyboard.press('Escape');

  // The next question: the summary in the system prompt, the latest
  // messages whole, never the first question.
  const before = chatCalls(fakeLlm).length;
  fakeLlm.usage('other', counts(3_000, 200));
  await send(main, 'Remind me who is organising it, and the budget?');
  await expect.poll(() => chatCalls(fakeLlm).length).toBeGreaterThan(before);
  const req = chatCalls(fakeLlm).at(-1);
  expect(req.system).toContain('Earlier in this chat');
  expect(req.system).toContain('Our offsite budget is £18,400');
  const sent = history(req);
  expect(sent.some((t) => t.includes('Our offsite budget is £18,400'))).toBe(false);
  expect(sent.at(-1)).toContain('Remind me who is organising it');
  expect(sent.some((t) => t.includes(asked.at(-1).slice(0, 30)))).toBe(true);
  fakeLlm.usage('other', null);
  fakeLlm.reply('other', () => 'OK');
});

test('a paste too long for the model is said plainly under the box and never sent', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(yap, main);
  const before = fakeLlm.requests.length;
  const paste = padded('Here is the whole supplier contract:', 40_000);
  await box(main).fill(paste);
  const line = tooLong(main);
  await expect(line).toBeVisible();
  await expect(line).toContainText("That's too long for GPT-OSS 20B (8K context).");
  await expect(line).toContainText("there's room for");
  await expect(line.getByRole('button', { name: 'Shorten what you pasted' })).toBeVisible();
  // On a cloud model there's no more room to give.
  await expect(line.getByRole('button', { name: 'Give it more room' })).toHaveCount(0);
  await expect(main.getByRole('button', { name: 'Send' })).toBeDisabled();
  await shot(main, '04-paste-too-long');
  // Enter doesn't send it either.
  await box(main).press('Enter');
  await main.waitForTimeout(500);
  expect(fakeLlm.requests.length).toBe(before);
  await expect(main.locator('.cbubble')).toHaveCount(0);
  // Shorten what you pasted: the message, selected, to cut down.
  await line.getByRole('button', { name: 'Shorten what you pasted' }).click();
  await expect(box(main)).toBeFocused();
  // Rust refuses it too, without saving it, whatever the page does.
  await expect(yap.invoke('chat_send', { conversationId: null, text: paste })).rejects.toThrow("That's too long for GPT-OSS 20B (8K context).");
  expect(fakeLlm.requests.length).toBe(before);
  // Short enough: the line goes.
  await box(main).fill('A short question.');
  await expect(line).toHaveCount(0);
  await box(main).fill('');
});

test("a provider's \"too long\" is said plainly, with New chat and Shorten — never its JSON", async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(yap, main);
  await send(main, 'When is the offsite?');
  await expect(lastAnswer(main)).toContainText('OK');
  // llama.cpp's own refusal, as a self-hosted server sends it.
  fakeLlm.failNext(
    'other',
    1,
    400,
    { error: { code: 400, message: 'the request exceeds the available context size, try increasing it', type: 'exceed_context_size_error', n_prompt_tokens: 9312, n_ctx: 8192 } }
  );
  await send(main, 'And the full agenda, please?');
  const line = tooLong(main);
  await expect(line).toBeVisible();
  await expect(line.locator('.vtext')).toHaveText("That's too long for GPT-OSS 20B (8K context).");
  await expect(line.getByRole('button', { name: 'New chat' })).toBeVisible();
  await expect(line.getByRole('button', { name: 'Shorten what you pasted' })).toBeVisible();
  // Never the provider's words, nor a raw error toast.
  await expect(main.getByText('exceed_context_size_error')).toHaveCount(0);
  await expect(main.getByText(/HTTP 400/)).toHaveCount(0);
  await expect(main.getByText("Couldn't send")).toHaveCount(0);
  // The question is back in the box.
  await expect(box(main)).toHaveValue('And the full agenda, please?');
  await shot(main, '05-provider-too-long');
  // New chat: a fresh chat, the question still in the box, the line gone.
  await line.getByRole('button', { name: 'New chat' }).click();
  await expect(main.locator('.cbubble')).toHaveCount(0);
  await expect(box(main)).toHaveValue('And the full agenda, please?');
  await expect(line).toHaveCount(0);
  await box(main).fill('');
});

test("the Voice Assistant's follow-up in a summarised chat carries the summary and the latest messages", async ({ yap, main, fakeLlm }) => {
  await yap.invoke('llm_models', { provider: 'groq', scope: 'chat', force: true });
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: HARBOUR }).click();
  // The divider before the first message the summary doesn't cover.
  await expect(divider(main)).toBeVisible();
  const before = chatCalls(fakeLlm).length;
  fakeLlm.reply('other', () => 'Ines Varga books the boat.');
  await yap.invoke('e2e_assistant_ask', { text: 'Who books the boat again?', listenMs: 0 });
  await expect(main.locator('.cbubble.assistant').last()).toContainText('Ines Varga books the boat.', { timeout: 15_000 });
  const req = chatCalls(fakeLlm).slice(before).find((r) => r.messages.some((m) => textOf(m.content).includes('Who books the boat again?')));
  expect(req).toBeTruthy();
  expect(req.system).toContain('Earlier in this chat');
  expect(req.system).toContain('Ines Varga books the boat');
  const sent = history(req);
  // The oldest messages go as the summary, never whole.
  expect(sent.some((t) => t.includes('Our harbour trip budget is £6,200'))).toBe(false);
  expect(sent.some((t) => t.includes('Harbour point 1.'))).toBe(false);
  // The latest whole (within the Voice Assistant's smaller budget).
  expect(sent.some((t) => t.includes('Harbour point 11.'))).toBe(true);
  expect(sent.at(-1)).toContain('Who books the boat again?');
  fakeLlm.reply('other', () => 'OK');
});

test("a meeting's Ask thread in Notes: the next question carries the thread's summary and its latest turns", async ({ yap, main, fakeLlm }) => {
  await openView(main, 'Notes');
  // The meeting is in the Meetings folder.
  await main.getByRole('complementary').getByRole('button', { name: /^Meetings\b/ }).first().click();
  await main.locator('.items .item').filter({ hasText: 'Supplier call' }).click();
  // Its kept thread shows (chat/note_chats.rs).
  await expect(main.locator('.chatthread')).toContainText('Who signs off the packaging order?');
  const before = fakeLlm.requests.length;
  fakeLlm.reply('other', () => 'Rafael Okoye signs it off.');
  await main.getByPlaceholder('Ask anything…').fill('And when is the delivery?');
  await main.getByPlaceholder('Ask anything…').press('Enter');
  await expect(main.locator('.chatthread')).toContainText('Rafael Okoye signs it off.');
  const req = fakeLlm.requests.slice(before).find((r) => textOf(r.user).includes('And when is the delivery?'));
  expect(req).toBeTruthy();
  expect(req.system).toContain('The user is asking about the following note');
  expect(req.system).toContain('Earlier in this chat');
  expect(req.system).toContain('Rafael Okoye signs off the packaging order.');
  const sent = history(req);
  // The turns the summary covers aren't sent whole; the ones after it are.
  expect(sent.some((t) => t === 'Who signs off the packaging order?')).toBe(false);
  expect(sent.some((t) => t.includes('Supplier point 7.'))).toBe(true);
  fakeLlm.reply('other', () => 'OK');
});

test('a long chat on Claude Haiku 5.5 stays under its 100K price line: the latest messages, never the whole chat', async ({ yap, main, fakeLlm, shot }) => {
  // Haiku 5.5 as Anthropic lists it: a million tokens of context.
  fakeLlm.listModels('anthropic', [
    { id: 'claude-haiku-5-5', type: 'model', display_name: 'Claude Haiku 5.5', created_at: '2026-09-01T00:00:00Z', max_input_tokens: 1_000_000 },
  ]);
  const cfg = await yap.invoke('get_config');
  const haiku = { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model: 'claude-haiku-5-5', apiKey: 'sk-ant-e2e-fake-1234567890', apiKeys: {}, prompt: '', disableThinking: false };
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: haiku } });
  try {
    await yap.invoke('llm_models', { provider: 'anthropic', scope: 'chat', force: true });
    await openView(main, 'Chat');
    await main.locator('.conv').filter({ hasText: GARDEN }).click();
    const before = chatCalls(fakeLlm).length;
    fakeLlm.reply('other', () => 'The greenhouse goes on the north side.');
    await send(main, 'Where did we say the greenhouse goes?');
    await expect(lastAnswer(main)).toContainText('The greenhouse goes on the north side.');
    await shot(main, '06-haiku-long-chat');
    const req = chatCalls(fakeLlm).slice(before).find((r) => r.messages.some((m) => textOf(m.content).includes('Where did we say the greenhouse goes?')));
    expect(req).toBeTruthy();
    // Under the line (by the fake's own count), and still most of the room:
    // the latest messages that fit, not a handful.
    expect(req.tokens).toBeLessThan(100_000);
    // (The window's start moves 6 messages at a time: a slightly longer
    // prompt — the rich tools' complete-guide rule, 31 Oct 2026 — moved it
    // one step, from just over 50K to 44K of the line's 80K.)
    expect(req.tokens).toBeGreaterThan(40_000);
    const sent = history(req);
    expect(sent.some((t) => t.includes('Our allotment plot is number 14'))).toBe(false);
    expect(sent.some((t) => t.includes('Allotment point 39.'))).toBe(true);
    expect(sent.at(-1)).toContain('Where did we say the greenhouse goes?');
    // The older messages are summarised in the background, by the chat's model.
    await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'chatSummary' && r.model === 'claude-haiku-5-5').length, { timeout: 15_000 }).toBeGreaterThan(0);
    for (const r of fakeLlm.requests.filter((x) => x.model === 'claude-haiku-5-5')) expect(r.tokens).toBeLessThan(100_000);
  } finally {
    fakeLlm.reply('other', () => 'OK');
    await setConfig(yap, main, { llmScopes: cfg.llmScopes });
  }
});
