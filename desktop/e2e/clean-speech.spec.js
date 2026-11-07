// "Clean up what you say…" (desktop/src-tauri/src/ai/cleanup_styles.rs
// `cleans_question` / `question_plan`, dictation/pipeline/delivery.rs
// `ask_assistant` / `clean_question`, chat/assistant.rs `shortcut_goes`,
// chat/assistant/screen.rs `ask_early`; Settings' CleanSpeech.svelte): a
// question said to Yap's AI — the Voice Assistant's shortcut, the Chat
// view's mic — goes through AI cleanup as an AI prompt, word for word
// (fillers and false starts out), on Dictation cleanup's model, whatever
// "AI cleanup" itself says. Each side its own switch: Chat (Settings →
// Voice Assistant) and Code chats (Settings → Jobs), both on by default.
// - Chat's switch: a plain chat's question cleaned, the cleanup told it's a
//   prompt for an AI and literal; the Chat view's mic too. Off: as said, even
//   with AI cleanup on — while the Code chats' switch is on.
// - Code chats' switch: a new code chat's question and one in a Code chat
//   cleaned; off: as said there, while Chat's is on.
// - Best effort: a cleanup that fails gives the words as said; a slow one
//   says so on the bar with Skip cleanup, which asks the words as said now.
// - A question that may be about the screen: Chat's model is asked that
//   while the question is cleaned, once.
// - No cleanup model: the words as said, no request; both Settings rows say
//   what's missing, with the way there. Light and dark.
//
// The test suite turns both switches off for every other spec
// (support/yap.js BASE_CONFIG); this one turns them on. The cleanup model
// and Chat's are a local fake (support/fake-llm.js).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, settingsDialog, setConfig, inAnotherApp, mainOnScreen } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat } from './support/code-chats.js';

const HERE = path.join(RUNS, 'clean-speech-spec');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

const SAID = 'um what is uh the capital of France';
const CLEAN = 'What is the capital of France?';

/** The fake cleanup model: fillers out, a capital and a question mark, word for word otherwise. */
function tidy(text) {
  const s = text.replace(/\b(um|uh|er)\b,?\s*/gi, '').replace(/\s+/g, ' ').trim();
  return s ? s.charAt(0).toUpperCase() + s.slice(1).replace(/[.?!]?$/, '?') : '';
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
    async ({}, use) => {
      await use({ name: 'clean-speech', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
/** Dictation cleanup's model: a server of one's own (no key needed). */
const cleanupModel = (fakeLlm) => ({ ppProvider: 'custom', ppBaseUrl: fakeLlm.base, ppModel: 'fake-cleanup', ppApiKey: '' });

const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const lastQuestion = (main) => thread(main).locator('.cbubble.user').last();
const askAloud = (yap, text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
const cleanups = (fakeLlm) => fakeLlm.requests.filter((r) => r.kind === 'cleanup');
/** The chat calls whose messages carry `text`. */
const chatCalls = (fakeLlm, text) => fakeLlm.requests.filter((r) => r.kind !== 'cleanup' && JSON.stringify(r.messages ?? []).includes(text));
const SCREEN_ASK = 'You decide whether a question someone asked a voice assistant out loud is about something on their screen';
const screenAsks = (fakeLlm) => fakeLlm.requests.filter((r) => String(r.system ?? '').startsWith(SCREEN_ASK));
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

async function plainChat(main) {
  await openView(main, 'Chat');
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

/** Asked out loud, answered with `answer`: the question as it landed in Chat. */
async function ask(yap, main, fakeLlm, text, answer) {
  fakeLlm.scriptChat([{ content: answer }]);
  await askAloud(yap, text);
  await expect(lastAnswer(main)).toHaveText(answer);
  return (await lastQuestion(main).innerText()).trim();
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  fs.mkdirSync(GARDEN, { recursive: true });
  fakeLlm.reply('cleanup', (user) => tidy(cleanupTranscript(user)));
  fakeLlm.delay('cleanup', 0);
  await mainOnScreen(yap);
  await setConfig(yap, main, {
    jobsEnabled: true,
    jobsSpeak: 'never',
    webSearch: false,
    speakReplies: false,
    postProcessEnabled: false,
    chatCleanSpeech: true,
    jobsCleanSpeech: true,
    llmScopes: chatScope(fakeLlm),
    ...cleanupModel(fakeLlm),
  });
  const have = (await yap.invoke('job_folders_list')).map((f) => f.name);
  if (!have.includes('garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });
});

test('Chat’s switch: a plain chat’s spoken question is cleaned word for word, AI cleanup off; off, it’s as said even with AI cleanup on', async ({ yap, main, fakeLlm }) => {
  // The Code chats' switch off: Chat's alone decides a plain chat's.
  await setConfig(yap, main, { jobsCleanSpeech: false });
  await plainChat(main);
  const before = cleanups(fakeLlm).length;
  expect(await ask(yap, main, fakeLlm, SAID, 'Paris.')).toContain(CLEAN);
  await expect(lastQuestion(main)).not.toContainText('um');
  // One cleanup, as an AI prompt, literal: never answered or reworded.
  const asked = cleanups(fakeLlm).slice(before);
  expect(asked).toHaveLength(1);
  expect(cleanupTranscript(asked[0].user)).toBe(SAID);
  const told = `${asked[0].system}\n${asked[0].user}`;
  expect(told).toContain('This dictation is a prompt for an AI.');
  expect(told).toContain('Tone: literal');
  // Chat's model got the cleaned question, never the fillers.
  expect(chatCalls(fakeLlm, CLEAN).length).toBeGreaterThan(0);
  expect(chatCalls(fakeLlm, 'uh the capital')).toHaveLength(0);
  // The log has the time it took, never the words.
  expect(log(yap)).toMatch(/Spoken question: AI cleanup.*ms=\d+/);
  expect(log(yap)).not.toContain('capital of France');

  // The Chat view's mic: the same.
  const r = await yap.invoke('e2e_chat_dictation', { text: 'um is it uh far from Lyon' });
  expect(r.text.trim()).toBe('Is it far from Lyon?');

  // Chat's off (Code chats' on), AI cleanup on: a plain chat's question as
  // said, as before — no cleanup asked.
  await setConfig(yap, main, { chatCleanSpeech: false, jobsCleanSpeech: true, postProcessEnabled: true });
  const mark = cleanups(fakeLlm).length;
  expect(await ask(yap, main, fakeLlm, 'um and uh the capital of Spain', 'Madrid.')).toContain('um and uh the capital of Spain');
  expect(cleanups(fakeLlm).length).toBe(mark);
});

test('Code chats’ switch: a new code chat’s question and one in a Code chat are cleaned; off, as said there while Chat’s is on', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { chatCleanSpeech: false, jobsCleanSpeech: true });
  await openView(main, 'Chat');
  await newCodeChat(main, 'garden-planner');
  const before = cleanups(fakeLlm).length;
  expect(await ask(yap, main, fakeLlm, 'um where are uh the seed notes', 'In seeds.md.')).toContain('Where are the seed notes?');
  expect(cleanups(fakeLlm).length).toBe(before + 1);
  // It's a Code chat in garden-planner, as asked.
  await expect(main.getByTestId('chat-workspace-chip')).toHaveText('garden-planner');
  // Asked again in it: still the Code chats' switch.
  expect(await ask(yap, main, fakeLlm, 'uh how many are there', 'Twelve.')).toContain('How many are there?');

  // Code chats' off, Chat's on: as said in the Code chat…
  await setConfig(yap, main, { chatCleanSpeech: true, jobsCleanSpeech: false });
  const mark = cleanups(fakeLlm).length;
  expect(await ask(yap, main, fakeLlm, 'um which ones uh need sun', 'The tomatoes.')).toContain('um which ones uh need sun');
  expect(cleanups(fakeLlm).length).toBe(mark);
  // …and a plain chat's cleaned.
  await plainChat(main);
  expect(await ask(yap, main, fakeLlm, SAID, 'Paris.')).toContain(CLEAN);
});

test('best effort: a cleanup that fails gives the words as said; a slow one offers Skip cleanup on the bar', async ({ yap, main, fakeLlm, shot }) => {
  await plainChat(main);
  // The model fails: the words as said go, nothing lost.
  fakeLlm.failNext('cleanup', 1, 500);
  expect(await ask(yap, main, fakeLlm, 'um what time is uh sunset', 'At 6.')).toContain('um what time is uh sunset');

  // A usual cleanup, then a slow one: past its usual time the bar says so,
  // with Skip cleanup, which asks the words as said at once.
  await yap.invoke('e2e_dictation_timing', { floorMs: 1500, sttMs: 0, forget: true });
  expect(await ask(yap, main, fakeLlm, SAID, 'Paris.')).toContain(CLEAN);
  const capsule = yap.overlay.locator('.capsule');
  fakeLlm.delay('cleanup', 10_000);
  fakeLlm.scriptChat([{ content: 'Rome.' }]);
  const started = Date.now();
  const slow = askAloud(yap, 'um the capital of uh Italy');
  await expect(capsule).toContainText('Taking longer than usual…', { timeout: 6_000 });
  const skip = capsule.getByRole('button', { name: 'Skip cleanup' });
  await expect(skip).toBeVisible();
  await shot(yap.overlay, '01-a-slow-cleanup-of-a-question-skip-cleanup');
  await yap.invoke('bar_simulate', { pointer: 'dictation' });
  await skip.click();
  await slow;
  expect(Date.now() - started).toBeLessThan(8_000);
  await expect(lastAnswer(main)).toHaveText('Rome.');
  await expect(lastQuestion(main)).toContainText('um the capital of uh Italy');
  expect(log(yap)).toContain('AI cleanup skipped from the bar');
  fakeLlm.delay('cleanup', 0);
});

test('a question that may be about the screen: Chat’s model is asked that while the question is cleaned, once', async ({ yap, main, fakeLlm }) => {
  await plainChat(main);
  // The person is in another app (the window in front could be what it's about).
  await inAnotherApp(yap);
  fakeLlm.delay('cleanup', 2_500);
  fakeLlm.scriptChat([{ content: 'It looks right.' }]);
  const before = screenAsks(fakeLlm).length;
  const started = Date.now();
  const asked = askAloud(yap, 'um is uh this right');
  // Asked before the cleanup is back…
  await expect.poll(() => screenAsks(fakeLlm).length, { timeout: 2_000 }).toBe(before + 1);
  expect(Date.now() - started).toBeLessThan(2_400);
  await asked;
  await mainOnScreen(yap);
  await expect(lastAnswer(main)).toHaveText('It looks right.');
  await expect(lastQuestion(main)).toContainText('Is this right?');
  // …and once: the cleaned question took that word.
  expect(screenAsks(fakeLlm).length).toBe(before + 1);
  expect(log(yap)).toContain("asking whether the question is about the screen while it's cleaned up");
  fakeLlm.delay('cleanup', 0);
});

test('no cleanup model: the words as said, and both Settings rows say what’s missing, light and dark', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { ppBaseUrl: '' });
  await plainChat(main);
  const mark = cleanups(fakeLlm).length;
  expect(await ask(yap, main, fakeLlm, SAID, 'Paris.')).toContain(SAID);
  expect(cleanups(fakeLlm).length).toBe(mark);

  const dialog = settingsDialog(main);
  const rows = [
    ['Voice Assistant', 'chat-clean-speech', 'Clean up what you say in Chat'],
    ['Jobs', 'jobs-clean-speech', 'Clean up what you say in Code chats'],
  ];
  for (const [page, setting, label] of rows) {
    await openSettings(main, page);
    const row = dialog.locator(`[data-setting="${setting}"]`);
    await row.scrollIntoViewIfNeeded();
    await expect(row.getByRole('button', { name: label })).toHaveAttribute('aria-pressed', 'true');
    await expect(row).toContainText("Fillers and false starts taken out of spoken questions");
    await expect(row).toContainText("Uses your dictation cleanup's model.");
    await expect(row.getByTestId('clean-speech-needs')).toContainText('Needs a cleanup model in Language Models → Dictation cleanup');
    await shot(main, `02-${setting}-needs-a-model-light`);
    await theme(yap, main, 'dark');
    await row.scrollIntoViewIfNeeded();
    await shot(main, `03-${setting}-needs-a-model-dark`);
    await theme(yap, main, 'light');
  }
  // The way there: Language Models' Dictation cleanup.
  await dialog.locator('[data-setting="jobs-clean-speech"]').getByRole('button', { name: 'Set up a cleanup model' }).click();
  await expect(dialog.locator('#settings-cleanup')).toBeVisible();

  // A model set up: the note goes, and the row stands alone (light and dark).
  await setConfig(yap, main, cleanupModel(fakeLlm));
  await openSettings(main, 'Voice Assistant');
  const row = dialog.locator('[data-setting="chat-clean-speech"]');
  await row.scrollIntoViewIfNeeded();
  await expect(row.getByTestId('clean-speech-needs')).toHaveCount(0);
  await shot(main, '04-chat-clean-speech-light');
  await theme(yap, main, 'dark');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '05-chat-clean-speech-dark');
  await theme(yap, main, 'light');
  // Off: no note even without a model.
  await row.getByRole('button', { name: 'Clean up what you say in Chat' }).click();
  await expect.poll(async () => (await yap.invoke('get_config')).chatCleanSpeech).toBe(false);
  await closeSettings(main);
});
