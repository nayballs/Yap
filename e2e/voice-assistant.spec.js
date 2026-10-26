// The Voice Assistant and spoken replies (src-tauri/src/chat/assistant.rs,
// voice/tts.rs, src/lib/views/chat/ChatView.svelte, Settings → Voice Assistant), and how
// they differ from Commands (Settings → Commands: the agent's name in a
// dictation, the rewrite shortcut — the AI types for you):
// - Settings: two pages that each say what they do, with links between
//   them; Language Models' tabs named alike; General's way to the shortcuts.
// - The assistant shortcut: set, refused when it clashes with another
//   shortcut, cleared; the gentle "Set an assistant shortcut" in Chat.
// - A question asked with it (staged with the debug-only
//   `e2e_assistant_ask`: the shortcut's own steps, a transcript in the mic's
//   place): the bar's capsule ("Ask Yap…", "Thinking…", "Speaking…" with ■),
//   the answer read aloud whatever "When to speak" says, a card on the bar
//   while the main window isn't in front, the conversation in Chat
//   continued; a press while it speaks cuts it off (barge-in); during a
//   meeting the answer is shown, never spoken; no AI model → Open settings.
// - The Chat view's Voice replies toggle (the same setting as Settings'
//   "Speak replies", kept in step both ways and through Settings'
//   auto-save), ▶ Read aloud on every answer (markdown out, a code block
//   said as "(code block)"), ■ / Esc to stop, "When to speak" (only for a
//   question that came by voice, or always), "Talk back hands-free" (a
//   dictated question sent by itself), and barge-in (a dictation starting
//   stops the voice).
//
// Test mode plays nothing and opens no audio device: two stand-in voices,
// silence of a plausible length on a simulated clock, and everything Yap
// would have said is kept for the debug-only `tts_e2e_spoken`. A dictated
// question is staged with the debug-only `e2e_chat_dictation` (the
// dictation's own steps from cleanup on, as the chat's mic starts one: no
// microphone needed). The "Groq" here is a local fake (support/fake-llm.js).
//
// Kokoro (voice/tts/kokoro.rs) is the default engine: the engine row, its download
// row and the "Download Kokoro" offer when replies go on, the 41 voices
// grouped by accent and gender (af_bella the default), Windows voices
// speaking until it's installed. Its download comes from a local server
// (support/kokoro.js, the debug-only YAP_TTS_SOURCES): never the internet.
// Once installed, test mode's Kokoro is silence too.
import fs from 'node:fs';
import path from 'node:path';
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  settingsDialog,
  expectStore,
  inAnotherApp,
  mainOnScreen,
  setConfig,
} from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeKokoro } from './support/kokoro.js';
import { RUNS } from './support/yap.js';

const TEST_KEY = 'e2e-fake-key';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  kokoro: [
    async ({}, use) => {
      const fake = await startFakeKokoro(path.join(RUNS, 'kokoro-sources'));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, kokoro }, use) => {
      await use({
        name: 'voice-assistant',
        env: kokoro.env,
        config: {
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: {
            chat: {
              enabled: true,
              provider: 'groq',
              baseUrl: fakeLlm.base,
              model: 'openai/gpt-oss-20b',
              apiKey: TEST_KEY,
              apiKeys: {},
              prompt: '',
              disableThinking: false,
            },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const speaking = async (yap) => (await yap.invoke('tts_state')).speaking;
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
// Voice replies sits in the composer's + menu: open it first.
async function voiceToggle(main) {
  const toggle = main.getByRole('button', { name: 'Voice replies', exact: true });
  if (!(await toggle.isVisible())) await main.getByRole('button', { name: 'Chat options' }).click();
  return toggle;
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

test.beforeEach(async ({ yap }) => {
  await yap.invoke('assistant_stop');
  await yap.invoke('tts_stop');
  await yap.invoke('tts_e2e_spoken', { clear: true });
});

// ---- The Voice Assistant (chat/assistant.rs)

/** A question to the Voice Assistant, as its shortcut asks it (the transcript in the mic's place). */
const ask = (yap, text, listenMs = 0) => yap.invoke('e2e_assistant_ask', { text, listenMs });
/** The dictation capsule on the Yap bar (its assistant variant has `data-ask`). */
const capsule = (yap) => yap.overlay.locator('.capsule');
/** The answer's card on the Yap bar. */
const answerCard = (yap) => yap.overlay.getByRole('status').filter({ has: yap.overlay.locator('.asked') });

/** The person's in another app, Yap's main window closed (hidden in the tray). */
async function hideMain(yap) {
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
}

/** A key going down and up in one of Yap's windows (its in-page fallbacks). */
async function pressKey(page, key, vk) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

test('Commands and the Voice Assistant: two pages that say what they do, with links between them', async ({ main, shot }) => {
  let dialog = await openSettings(main, 'Commands');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Commands' })).toBeVisible();
  // One plain sentence and an example.
  await expect(dialog.getByText('Tell Yap\'s AI what to write, and it types the result where you are.')).toBeVisible();
  const example = dialog.getByRole('note');
  await expect(example).toContainText("Hey Yap, reply that I'll be late");
  await expect(example).toContainText('Yap writes it and types it there.');
  // The agent's name and its switch, and the rewrite shortcut, live here.
  const name = dialog.getByRole('group', { name: 'Say its name' });
  await expect(name.getByRole('button', { name: 'Answer to its name in any dictation' })).toBeVisible();
  await expect(name.getByPlaceholder('e.g. Jarvis, Nova, Atlas...')).toBeVisible();
  const rewrite = dialog.getByRole('group', { name: 'Rewrite selected text' });
  await expect(rewrite).toContainText('Rewrite shortcut');
  await expect(rewrite.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  // No spoken replies or voices: that's the Voice Assistant.
  await expect(dialog.getByRole('button', { name: 'Speak replies' })).toHaveCount(0);
  await shot(main, '10-settings-commands');

  // Its model: Language Models → Commands, which points back for the name.
  await dialog.getByRole('group', { name: 'Model and prompt' }).getByRole('button', { name: /^Model and prompt/ }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
  await expect(dialog.locator('.scope-tabs').getByRole('tab')).toHaveText([
    'Dictation Cleanup',
    'Commands',
    'Note Formatting',
    'Chat & Voice Assistant',
  ]);
  await expect(dialog.getByRole('tab', { name: 'Commands' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByText('The model that carries out Commands: "Hey Yap, …" in a dictation')).toBeVisible();
  await expect(dialog.getByText('Agent name:')).toContainText('Yap');
  await shot(main, '11-language-models-commands');
  await dialog.getByRole('button', { name: 'Name and shortcut in Commands' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Commands' })).toBeVisible();

  // Over to the Voice Assistant, which says what it does and links back.
  await dialog.getByRole('note').getByRole('button', { name: 'Voice Assistant' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' })).toBeVisible();
  await expect(dialog.getByText('Ask Yap something by voice and hear the answer, like a voice assistant.')).toBeVisible();
  await expect(dialog.getByRole('note')).toContainText('Yap answers out loud, and the conversation is in Chat.');
  await expect(dialog.getByRole('button', { name: 'Answer to its name in any dictation' })).toHaveCount(0);
  await shot(main, '12-settings-voice-assistant-top');
  await dialog.getByRole('note').getByRole('button', { name: 'Commands' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Commands' })).toBeVisible();

  // Shortcuts has all six together, the unset ones saying so.
  dialog = await openSettings(main, 'Shortcuts');
  const all = dialog.getByRole('group', { name: 'Global shortcuts' });
  await expect(all.locator('[data-setting="shortcut-dictation"] kbd')).toHaveText(['F24']);
  await expect(all.locator('[data-setting="shortcut-rewrite"]').getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await expect(all.locator('[data-setting="shortcut-assistant"]').getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await expect(all.locator('[data-setting="shortcut-read"]').getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await expect(all.locator('[data-setting="shortcut-as-said"]').getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  await expect(all.locator('[data-setting="shortcut-meeting"] kbd')).toHaveText(['Win', 'Alt', 'M']);
  await shot(main, '12b-settings-shortcuts');
  await closeSettings(main);
});

test('the assistant shortcut: set in Settings, never one that clashes, and Chat points to it', async ({ yap, main, shot }) => {
  // None set: an empty chat says how to ask out loud.
  await freshChat(main);
  // The shortcut's line, not Ask across meetings' (Chat shows both, 25 Oct 2026).
  await expect(main.locator('.asknote', { hasText: 'assistant shortcut' })).toContainText('Set an assistant shortcut to ask out loud from any app');
  await shot(main, '13-chat-set-an-assistant-shortcut');
  await main.getByRole('button', { name: 'Set an assistant shortcut' }).click();
  const dialog = settingsDialog(main);
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' })).toBeVisible();
  const group = dialog.getByRole('group', { name: 'Assistant shortcut' });
  await expect(group).toContainText('Set an assistant shortcut to ask Yap from any app');

  await group.getByRole('button', { name: 'Set a shortcut' }).click();
  // The dictation hotkey (F24 in the suite): refused.
  await pressKey(main, 'F24', 135);
  await expect(group).toContainText('Already used by the dictation hotkey');
  // A combo the meeting shortcut (Win+Alt+M) fires inside: refused too.
  await main.keyboard.press('Control+Meta+Alt+KeyM');
  await expect(group).toContainText('Clashes with the meeting shortcut (Win + Alt + M): one press would start both');
  await shot(main, '14-assistant-shortcut-clash');
  expect(yap.readJson('config.json').assistantHotkey ?? '').toBe('');
  // F23: taken, saved, and the page's example names it.
  await pressKey(main, 'F23', 134);
  await expectStore(yap, 'config.json', (c) => c.assistantHotkey === 'kb:134');
  await expect(group.getByRole('button', { name: /^F23\s*Change$/ })).toBeVisible();
  await expect(group).not.toContainText('Set an assistant shortcut to ask Yap');
  await expect(dialog.getByRole('note')).toContainText('Press F23 in any app');
  await shot(main, '15-assistant-shortcut-set');
  // The rewrite shortcut can't take it now.
  await openSettings(main, 'Commands');
  const rewrite = dialog.getByRole('group', { name: 'Rewrite selected text' });
  await rewrite.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F23', 134);
  await expect(rewrite).toContainText('Already used by the assistant shortcut');
  await main.keyboard.press('Escape');
  expect(yap.readJson('config.json').editHotkey).toBe('');
  await closeSettings(main);
  // Chat names it.
  await expect(main.locator('.asknote', { hasText: 'to ask out loud' })).toContainText('Or press F23 in any app to ask out loud');

  // Cleared again.
  await openSettings(main, 'Voice Assistant');
  await group.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.assistantHotkey === '');
  await closeSettings(main);
});

test('asked from another app with Yap closed: listening, thinking, speaking, a card, and the chat continued', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  // Spoken replies on ("When I asked by voice"): the shortcut's answers are read aloud.
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice' });
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'The launch party is on Friday.' }]);
  await send(main, 'When is the launch party?');
  await expect(thread(main)).toContainText('The launch party is on Friday.');
  const conv = () => yap.readJson('chats.json').conversations.find((c) => c.title === 'When is the launch party?');
  const id = conv().id;
  await hideMain(yap);

  // The question: "Ask Yap…" while it listens, then "Thinking…".
  fakeLlm.delay('other', 1500);
  fakeLlm.scriptChat([{ content: 'It starts at **ten**, in the big room. Sam has the slides.' }]);
  const cap = capsule(yap);
  const asking = ask(yap, 'what time does it start', 1500);
  await expect(cap).toHaveAttribute('data-ask', 'listening');
  await expect(cap).toContainText('Ask Yap…');
  await shot(yap.overlay, '20-assistant-listening');
  await asking;
  await expect(cap).toHaveAttribute('data-ask', 'thinking');
  await expect(cap).toContainText('Thinking…');
  await shot(yap.overlay, '21-assistant-thinking');
  fakeLlm.delay('other', 0);

  // The answer, read aloud, with ■ on the capsule and the answer on a card.
  await expect(cap).toHaveAttribute('data-ask', 'speaking', { timeout: 10_000 });
  await expect(cap).toContainText('Speaking…');
  await expect(cap.getByRole('button', { name: 'Stop speaking' })).toBeVisible();
  const card = answerCard(yap);
  await expect(card).toContainText('what time does it start');
  await expect(card).toContainText('It starts at ten, in the big room. Sam has the slides.');
  await expect(card.getByRole('button', { name: 'Open in Chat' })).toBeVisible();
  await shot(yap.overlay, '22-assistant-speaking-card');
  const [said] = await spoken(yap);
  expect(said).toMatchObject({ messageId: `chat:${id}:3`, text: 'It starts at ten, in the big room. Sam has the slides.' });
  // The conversation open in Chat, continued; the question asked by voice.
  expect(conv().messages.map((m) => [m.role, !!m.voice])).toEqual([
    ['user', false],
    ['assistant', false],
    ['user', true],
    ['assistant', false],
  ]);
  expect(conv().messages[2].text).toBe('what time does it start');

  // The voice ends: the capsule and the card go.
  await expect.poll(() => speaking(yap), { timeout: 30_000 }).toBe(false);
  await expect(cap).toHaveCount(0);
  await expect(card).toHaveCount(0);

  // Again: a follow-up in the same conversation.
  fakeLlm.scriptChat([{ content: 'Sam has them.' }]);
  await ask(yap, 'who has the slides');
  await expect.poll(() => conv().messages.length).toBe(6);
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text)).toEqual([said.text, 'Sam has them.']);
  await yap.invoke('tts_stop');

  // Back in Yap: the Chat view followed it all.
  await mainOnScreen(yap);
  const asked = thread(main).locator('.cbubble.user');
  await expect(asked).toHaveCount(3);
  await expect(asked.nth(1)).toContainText('what time does it start');
  await expect(asked.nth(1).getByLabel('Asked by voice')).toBeVisible();
  await expect(asked.nth(2).getByLabel('Asked by voice')).toBeVisible();
  await expect(thread(main)).toContainText('Sam has them.');
  await shot(main, '23-chat-followed-the-assistant');
});

test('asked while Yap is in front: Chat comes up with the question and the answer, no card', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await expect(main.locator('.chat')).toHaveCount(0);
  fakeLlm.delay('other', 800);
  fakeLlm.scriptChat([{ content: 'Your next meeting is the design review.' }]);
  await ask(yap, 'what is my next meeting');
  // Chat comes up on it: the question at once, "Thinking…", then the answer.
  await expect(thread(main).locator('.cbubble.user').last()).toContainText('what is my next meeting');
  await expect(thread(main).locator('.thinking')).toBeVisible();
  fakeLlm.delay('other', 0);
  await expect(thread(main)).toContainText('Your next meeting is the design review.');
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toBeVisible();
  await expect(thread(main).locator('.cbubble.user').last().getByLabel('Asked by voice')).toBeVisible();
  // The window's in front: no card on the bar.
  await expect(answerCard(yap)).toHaveCount(0);
  await shot(main, '24-asked-in-front-of-yap');
  await yap.invoke('tts_stop');
  await expect(capsule(yap)).toHaveCount(0);
});

// A spoken answer being written into the open chat has Chat's own ■ too
// (26 Oct 2026: a looping on-device answer ran on with no Stop in the box).
test('a spoken answer still being written into Chat has ■ in the message box, and it stops it', async ({ yap, main, fakeLlm }) => {
  await mainOnScreen(yap);
  fakeLlm.scriptChat([
    { content: 'Your next meeting is the design review. Then lunch with the team at one. Then the planning call at three.', stream: { gapMs: 150, hangAfter: 3 } },
  ]);
  await ask(yap, 'what is on today');
  const stop = main.getByRole('button', { name: 'Stop the answer' });
  await expect(stop).toBeVisible({ timeout: 10_000 });
  await stop.click();
  await expect.poll(() => fakeLlm.requests.at(-1).aborted, { timeout: 10_000 }).toBe(true);
  await expect(stop).toHaveCount(0, { timeout: 10_000 });
  await expect(capsule(yap)).toHaveCount(0, { timeout: 10_000 });
});

test('■ stops the answer, and a press while Yap speaks cuts it off to listen (barge-in)', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await inAnotherApp(yap);
  const long = Array.from({ length: 8 }, (_, i) => `Point number ${i + 1} of a long answer.`).join(' ');
  const cap = capsule(yap);

  // ■ on the capsule: the voice stops, the capsule and the card go.
  fakeLlm.scriptChat([{ content: long }]);
  await ask(yap, 'tell me everything');
  await expect(cap).toHaveAttribute('data-ask', 'speaking', { timeout: 10_000 });
  await cap.getByRole('button', { name: 'Stop speaking' }).click();
  await expect.poll(() => speaking(yap)).toBe(false);
  await expect(cap).toHaveCount(0);
  await expect(answerCard(yap)).toHaveCount(0);

  // ✕ on the card (or Esc): the same.
  fakeLlm.scriptChat([{ content: long }]);
  await ask(yap, 'once more');
  await expect(answerCard(yap)).toBeVisible({ timeout: 10_000 });
  await answerCard(yap).getByRole('button', { name: 'Close' }).click();
  await expect.poll(() => speaking(yap)).toBe(false);
  await expect(cap).toHaveCount(0);

  // A new question while it speaks: the voice stops at once and it listens.
  fakeLlm.scriptChat([{ content: long }, { content: 'Just the first: point one.' }]);
  await ask(yap, 'again please');
  await expect(cap).toHaveAttribute('data-ask', 'speaking', { timeout: 10_000 });
  const asking = ask(yap, 'actually, just the first point', 1500);
  await expect.poll(() => speaking(yap), { timeout: 1_000 }).toBe(false);
  await expect(cap).toHaveAttribute('data-ask', 'listening');
  await shot(yap.overlay, '25-barge-in-listening');
  await asking;
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text).at(-1), { timeout: 10_000 }).toBe(
    'Just the first: point one.'
  );
  await yap.invoke('tts_stop');

  // The shortcut pressed in Yap's own window (its in-page fallback) while
  // Yap speaks: the voice stops there too.
  await setConfig(yap, main, { assistantHotkey: 'kb:134' });
  await yap.invoke('tts_speak', { text: long, messageId: 'chat:999:1' });
  await expect.poll(() => speaking(yap)).toBe(true);
  await pressKey(main, 'F23', 134);
  await expect.poll(() => speaking(yap), { timeout: 2_000 }).toBe(false);
  // With a microphone it's listening now: called off.
  await yap.invoke('cancel_recording');
  await setConfig(yap, main, { assistantHotkey: '' });
});

// A tester's voice chat (2026-10-23; the town is fictional): a question was
// still being answered (by a model on this PC, so set aside for the
// barge-in's recording) when a newer one was asked; the older was asked
// again after the newer one's answer, and both answers showed under both
// questions. The newer question replaces it now (chat/stream.rs
// `Halt::Superseded`): one answer, to the newer question, said once.
test('a barge-in question replaces the one still being answered: one answer, to the newer one', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice' });
  await freshChat(main);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  // The first answer stops coming after its first two sentences (until Yap
  // hangs up): the barge-in always lands while it's being answered.
  fakeLlm.scriptChat([
    {
      content:
        'Kellbrook is mild today with sunny spells. Light winds come from the west all afternoon. Dry until late evening, then a little rain.',
      stream: { gapMs: 150, hangAfter: 9 },
    },
    { content: 'Kellbrook stays dry and mild today.' },
  ]);
  const chatCalls = () => fakeLlm.requests.filter((r) => r.kind === 'other').length;
  const before = chatCalls();
  const first = 'Can you tell me the weather today in Kellbrook?';
  await ask(yap, first);
  const live = thread(main).getByTestId('chat-streaming');
  await expect(live).toContainText('Kellbrook is mild today', { timeout: 10_000 });
  // The barge-in: the shortcut again, a newer question (misheard, as it was).
  await ask(yap, 'Look up the weather kelp for me today.', 1500);
  const answers = thread(main).locator('.cbubble.assistant');
  await expect(answers).toHaveText(['Kellbrook stays dry and mild today.'], { timeout: 15_000 });
  await expect(live).toHaveCount(0);
  await expect(thread(main).locator('.thinking')).toHaveCount(0);
  const questions = thread(main).locator('.cbubble.user');
  await expect(questions).toHaveCount(2);
  await expect(questions.nth(0).getByLabel('Asked by voice')).toBeVisible();
  await expect(questions.nth(1).getByLabel('Asked by voice')).toBeVisible();
  await shot(main, '28-barge-in-replaced-the-older-question');
  // Said: the newer answer; nothing more of the older one after the barge-in.
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text).at(-1), { timeout: 10_000 }).toBe(
    'Kellbrook stays dry and mild today.'
  );
  expect((await spoken(yap)).some((s) => s.text.includes('until late evening'))).toBe(false);
  // Saved: both questions, one answer. The older was never asked again
  // (past the 2 s a model on this PC waits after a dictation).
  await main.waitForTimeout(3_000);
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title === first);
  expect(conv.messages.map((m) => [m.role, !!m.voice])).toEqual([
    ['user', true],
    ['user', true],
    ['assistant', false],
  ]);
  expect(chatCalls()).toBe(before + 2);
  await yap.invoke('tts_stop');
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice' });
});

// Streamed (chat/stream.rs): the voice starts at the answer's first whole
// sentence, while the rest is still being written — and says it all once.
test('a spoken answer starts at its first sentence, while the rest is still being written', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice' });
  await inAnotherApp(yap);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  const answer = 'The match starts at eight tonight. It is on the main channel. Kick-off is at the home ground.';
  fakeLlm.scriptChat([{ content: answer, stream: { gapMs: 300 } }]);
  const cap = capsule(yap);
  await ask(yap, 'when does the match start');
  // Speaking, and the answer still coming.
  await expect
    .poll(async () => (await speaking(yap)) && !fakeLlm.requests.at(-1).ended, { timeout: 10_000 })
    .toBe(true);
  await expect(cap).toHaveAttribute('data-ask', 'speaking');
  await shot(yap.overlay, '26-assistant-speaking-while-written');
  // All of it said, once, as it was written.
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text), { timeout: 15_000 }).toEqual([answer]);
  const [said] = await spoken(yap);
  expect(said.streamed).toBe(true);
  await yap.invoke('tts_stop');
  await mainOnScreen(yap);
});

// A step's opening words that turn out to come before a tool call are taken
// back (chat/stream.rs `Step`): long enough to pass `releases`, the voice had
// already started on them. It stops there, drops what it hadn't said, and the
// answer after the tool is said from its first sentence (chat/assistant.rs
// `Streamed::taken_back`). Before (23 Oct follow-up): the voice went on with
// the words taken back, and the answer's first sentences were skipped as
// already said.
test('words taken back for a tool call stop the voice, and the answer is said from its start', async ({
  yap,
  main,
  fakeLlm,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice' });
  await inAnotherApp(yap);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  const preamble = 'Let me look through your notes for that. I will check the launch plan first. Then I can tell you who has them.';
  const answer = 'Sam has the slides. They are in the shared launch folder.';
  fakeLlm.scriptChat([
    { preamble, tools: [{ name: 'search_notes', args: { query: 'launch slides' } }], stream: { gapMs: 300 } },
    { content: answer, stream: { gapMs: 700 } },
  ]);
  const cap = capsule(yap);
  const chatCalls = () => fakeLlm.requests.filter((r) => r.kind === 'other' && r.tools.length > 0);
  const before = chatCalls().length;
  await ask(yap, 'who has the launch slides');
  // The voice started on the words before the call.
  await expect.poll(() => speaking(yap), { timeout: 10_000 }).toBe(true);
  await expect(cap).toHaveAttribute('data-ask', 'speaking');
  // The call came: the voice stops while the answer is still to come, and
  // the capsule is back to thinking.
  await expect.poll(() => chatCalls().length, { timeout: 10_000 }).toBe(before + 2);
  await expect.poll(() => speaking(yap), { timeout: 1_500 }).toBe(false);
  await expect(cap).toHaveAttribute('data-ask', 'thinking');
  // The answer, said whole from its first sentence; the words taken back
  // never reached it.
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text).at(-1), { timeout: 15_000 }).toBe(answer);
  const said = await spoken(yap);
  expect(said.at(-1).streamed).toBe(true);
  expect(said.filter((s) => s.text.includes('Sam has the slides'))).toHaveLength(1);
  expect(said.some((s) => s.text.includes('Then I can tell you'))).toBe(false);
  await expect(answerCard(yap)).toContainText('Sam has the slides');
  await expect(answerCard(yap)).not.toContainText('Let me look');
  await yap.invoke('tts_stop');
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice' });
  await mainOnScreen(yap);
});

// The maintainer (2026-10-18): "We do not want this feature enabled during meetings,
// we don't want Bella to talk … this would be too much". While a meeting
// records or a call is on, the shortcut doesn't listen and Yap doesn't talk.
const offCard = (yap) =>
  yap.overlay.getByRole('status').filter({ hasText: 'The Voice Assistant is off during meetings' });

test('during a meeting: the Voice Assistant is off — no listening, no answer, no voice', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await inAnotherApp(yap);
  const chatsBefore = yap.readJson('chats.json')?.conversations?.length ?? 0;
  const note = await yap.invoke('note_create', { title: 'Standup', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(async () => (await yap.invoke('meeting_state')).recording).toBe(true);
  try {
    await expect(ask(yap, 'when is the budget review')).rejects.toThrow(/off during meetings/);
    await expect(offCard(yap)).toBeVisible({ timeout: 10_000 });
    await expect(capsule(yap)).toHaveCount(0);
    await expect(yap.overlay.getByLabel('Recording meeting notes')).toBeVisible();
    await shot(yap.overlay, '26-assistant-off-during-a-meeting');
    // Nothing asked, nothing said, and ▶ is refused too.
    expect(yap.readJson('chats.json')?.conversations?.length ?? 0).toBe(chatsBefore);
    await expect(yap.invoke('tts_speak', { text: 'Hello', messageId: 'chat:test' })).rejects.toThrow(/records a meeting/);
    expect(await spoken(yap)).toEqual([]);
  } finally {
    await yap.invoke('meeting_delete', { noteId: note.id });
  }
});

test('on a call (not recording): the Voice Assistant is off and spoken replies stay quiet', async ({
  yap,
  main,
  fakeLlm,
}) => {
  await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: true });
  try {
    await inAnotherApp(yap);
    await expect(ask(yap, 'what is next')).rejects.toThrow(/off during meetings/);
    await expect(offCard(yap)).toBeVisible({ timeout: 10_000 });
    await expect(yap.invoke('tts_speak', { text: 'Hello', messageId: 'chat:test' })).rejects.toThrow(/during a call/);
    // Chat's own spoken replies ("Always") don't read a typed answer either.
    await setConfig(yap, main, { speakReplies: true, speakWhen: 'always' });
    await mainOnScreen(yap);
    await freshChat(main);
    fakeLlm.scriptChat([{ content: 'Quietly answered.' }]);
    await send(main, 'are you there');
    await expect(thread(main)).toContainText('Quietly answered.');
    await main.waitForTimeout(500);
    expect(await spoken(yap)).toEqual([]);
  } finally {
    await yap.invoke('meeting_detect_simulate', { appId: 'teams', active: false });
    await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice' });
  }
});

test('no AI model: the assistant says so, with a way to Language Models', async ({ yap, main, shot }) => {
  const cfg = await yap.invoke('get_config');
  const chat = cfg.llmScopes.chat;
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: { ...chat, enabled: false } } });
  try {
    await inAnotherApp(yap);
    await ask(yap, 'is anyone there');
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Pick an AI model for the Voice Assistant' });
    await expect(card).toBeVisible({ timeout: 10_000 });
    await expect(card).toContainText('Language Models → Chat & Voice Assistant');
    await expect(capsule(yap)).toHaveCount(0);
    await shot(yap.overlay, '27-no-ai-model');
    await card.getByRole('button', { name: 'Open settings' }).click();
    const dialog = settingsDialog(main);
    await expect(dialog.getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
    await expect(dialog.getByRole('tab', { name: 'Chat & Voice Assistant' })).toHaveAttribute('aria-selected', 'true');
    expect(await spoken(yap)).toEqual([]);
  } finally {
    // Settings closed first: its auto-save must not write the switched-off
    // model back after the restore (the next test would get "Pick an AI model").
    await closeSettings(main).catch(() => {});
    await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat } });
    await expect.poll(() => yap.readJson('config.json').llmScopes?.chat?.enabled).toBe(true);
  }
});

test("the model fails: the assistant says why, and the question waits in Chat", async ({ yap, main, fakeLlm }) => {
  await inAnotherApp(yap);
  fakeLlm.failNext('other', 1);
  try {
    await ask(yap, 'will this fail');
    const card = yap.overlay.getByRole('status').filter({ hasText: "The Voice Assistant couldn't answer" });
    await expect(card).toBeVisible({ timeout: 10_000 });
    await expect(card).toContainText('Your question is in Chat.');
    await expect(capsule(yap)).toHaveCount(0);
    expect(await spoken(yap)).toEqual([]);
    const asked = yap
      .readJson('chats.json')
      .conversations.flatMap((c) => c.messages)
      .filter((m) => m.text === 'will this fail');
    expect(asked).toEqual([expect.objectContaining({ role: 'user', voice: true })]);
    await card.getByRole('button', { name: 'Close' }).click();
  } finally {
    fakeLlm.failNext('other', 0);
  }
});

test('Settings → Voice Assistant: spoken replies, the engine, the voice and its preview', async ({ yap, main, shot }) => {
  await setConfig(yap, main, {
    speakReplies: false,
    speakWhen: 'voice',
    talkBackHandsFree: true,
    ttsEngine: 'kokoro',
    ttsVoice: 'af_bella',
    ttsWindowsVoice: '',
    ttsRate: 1,
  });
  const dialog = await openSettings(main, 'Voice Assistant');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' })).toBeVisible();

  // Off by default; what hangs on it is greyed out until it's on.
  const speak = dialog.getByRole('button', { name: 'Speak replies' });
  await expect(speak).toHaveAttribute('aria-pressed', 'false');
  const handsFree = dialog.getByRole('button', { name: 'Talk back hands-free' });
  await expect(handsFree).toBeDisabled();
  await expect(dialog.getByRole('tab', { name: 'When I asked by voice' })).toBeDisabled();

  // The engine: Kokoro, recommended, a download away.
  const engine = dialog.getByRole('tablist', { name: 'Engine' });
  await expect(engine.getByRole('tab', { name: 'Kokoro' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByText('Natural voices, made on this PC. Recommended')).toBeVisible();
  const krow = dialog.getByRole('group', { name: 'Kokoro' });
  await expect(krow.locator('.mrow')).toContainText('Kokoro-82M');
  await expect(krow.locator('.mrow')).toContainText('Recommended');
  await expect(krow.locator('.mrow')).toContainText('41 voices');
  await expect(krow.getByRole('button', { name: 'Download', exact: true })).toBeVisible();
  await expect(krow).toContainText('Until then, Windows voices speak');

  // Kokoro's 41 voices (its Japanese and Mandarin ones aren't offered: they
  // read text as English), grouped by accent and gender; Bella the default.
  const voice = dialog.getByRole('combobox', { name: 'Voice' });
  await expect(voice).toHaveValue('af_bella');
  await expect(voice.locator('option:checked')).toHaveText('Bella (default)');
  await expect(voice.locator('option')).toHaveCount(41);
  expect(await voice.locator('optgroup').evaluateAll((gs) => gs.map((g) => g.label))).toEqual([
    'British English · Female',
    'British English · Male',
    'American English · Female',
    'American English · Male',
    'Other languages · Female',
    'Other languages · Male',
  ]);
  await expect(voice.locator('optgroup[label="British English · Female"] option')).toHaveText(['Alice', 'Emma', 'Isabella', 'Lily']);
  await expect(voice.locator('optgroup[label="British English · Male"] option')).toHaveText(['Daniel', 'Fable', 'George', 'Lewis']);
  await expect(voice.locator('optgroup[label="Other languages · Male"] option').first()).toHaveText('Alex · Brazilian Portuguese');
  // Not heard until it's downloaded; where it comes from, and its licences.
  await expect(dialog.getByRole('button', { name: 'Preview the voice' })).toBeDisabled();
  await expect(dialog.locator('.credit')).toContainText('Apache-2.0');
  await expect(dialog.locator('.credit')).toContainText('eSpeak NG');
  await expect(dialog.locator('.credit')).toContainText('GPL-3.0');
  // The agent's name isn't here: that's Commands.
  await expect(dialog.getByRole('button', { name: 'Answer to its name in any dictation' })).toHaveCount(0);
  await dialog.getByRole('group', { name: 'Spoken replies in Chat' }).scrollIntoViewIfNeeded();
  await shot(main, '01-settings-voice-assistant');

  // Windows voices: no download, the voices Windows gives apps (test mode:
  // two stand-ins), the default named.
  await engine.getByRole('tab', { name: 'Windows voices' }).click();
  await expectStore(yap, 'config.json', (c) => c.ttsEngine === 'windows');
  await expect(krow).toHaveCount(0);
  await expect(dialog.getByText("Windows' own voices: no download, but more robotic")).toBeVisible();
  await expect(voice.locator('option')).toHaveText([
    'Windows default (Test Voice One)',
    'Test Voice One · en-GB',
    'Test Voice Two · en-US',
  ]);

  // On, then a voice and a speed: saved (Kokoro's voice kept for later).
  await speak.click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === true);
  // Windows chosen: no Kokoro offer.
  await expect(dialog.getByRole('group', { name: 'Download Kokoro' })).toHaveCount(0);
  await expect(handsFree).toBeEnabled();
  await expect(handsFree).toHaveAttribute('aria-pressed', 'true');
  await voice.selectOption({ label: 'Test Voice Two · en-US' });
  await dialog.getByRole('slider', { name: 'Speed' }).fill('1.5');
  await expect(dialog.getByText('1.5×')).toBeVisible();
  await expectStore(
    yap,
    'config.json',
    (c) => c.ttsWindowsVoice === 'e2e-voice-2' && c.ttsRate === 1.5 && c.ttsVoice === 'af_bella'
  );

  // ▶ Preview: a sample in that voice at that speed; ■ stops it.
  await dialog.getByRole('button', { name: 'Preview the voice' }).click();
  await expect(dialog.getByRole('button', { name: 'Stop the preview' })).toBeVisible();
  const [preview] = await spoken(yap);
  expect(preview).toMatchObject({ messageId: 'preview', engine: 'windows', voice: 'e2e-voice-2', rate: 1.5 });
  expect(preview.sentences.length).toBeGreaterThan(1);
  await shot(main, '02-preview-playing');
  await dialog.getByRole('button', { name: 'Stop the preview' }).click();
  await expect(dialog.getByRole('button', { name: 'Preview the voice' })).toBeVisible();
  expect(await speaking(yap)).toBe(false);

  // Its model is Chat's: Language Models → Chat & Voice Assistant, which
  // points back here.
  await dialog.getByRole('group', { name: 'Model', exact: true }).getByRole('button', { name: /^Model/ }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'Chat & Voice Assistant' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByText('The Voice Assistant asks this model too')).toBeVisible();
  await shot(main, '03-language-models-chat-and-voice-assistant');
  await dialog.getByRole('button', { name: 'Voice, shortcut and spoken replies in Voice Assistant' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' })).toBeVisible();

  await speak.click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === false);
  await closeSettings(main);
  await setConfig(yap, main, { ttsEngine: 'kokoro', ttsWindowsVoice: '', ttsRate: 1 });
});

test("Chat's Voice replies toggle is Settings' Speak replies, and survives auto-save", async ({ yap, main, shot }) => {
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice' });
  await openView(main, 'Chat');
  await expect(await voiceToggle(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(await voiceToggle(main)).toHaveAttribute('title', 'Voice replies off');
  await (await voiceToggle(main)).click();
  await expect(await voiceToggle(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(await voiceToggle(main)).toHaveAttribute('title', /questions you ask by voice/);
  await expectStore(yap, 'config.json', (c) => c.speakReplies === true);
  await shot(main, '04-chat-voice-on');

  // Settings' copy adopted it: shown on, and another setting's save keeps it.
  const dialog = await openSettings(main, 'Voice Assistant');
  await expect(dialog.getByRole('button', { name: 'Speak replies' })).toHaveAttribute('aria-pressed', 'true');
  await dialog.getByRole('tab', { name: 'Always' }).click();
  await expectStore(yap, 'config.json', (c) => c.speakWhen === 'always');
  expect(yap.readJson('config.json').speakReplies).toBe(true);

  // And the other way: off in Settings, off in Chat.
  await dialog.getByRole('button', { name: 'Speak replies' }).click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === false);
  await closeSettings(main);
  await expect(await voiceToggle(main)).toHaveAttribute('aria-pressed', 'false');
  await setConfig(yap, main, { speakWhen: 'voice' });
});

test('▶ reads an answer aloud — markdown out, code as "(code block)" — and ■ or Esc stops it', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: false });
  await freshChat(main);
  fakeLlm.scriptChat([
    {
      content:
        '## Launch plan\n\nThe launch is on **Friday**, see [the doc](https://docs.example.com/plan) or https://www.github.com/acme/app.\n\n```js\nconsole.log("hi");\n```\n\n- Ask *Sam* about the venue 👍\n- [x] Book the room',
    },
  ]);
  await send(main, 'When is the launch?');
  await expect(thread(main)).toContainText('The launch is on Friday');
  // Voice replies off: nothing said by itself.
  await main.waitForTimeout(500);
  expect(await spoken(yap)).toEqual([]);

  const read = thread(main).getByRole('button', { name: 'Read aloud' });
  await expect(read).toHaveCount(1);
  await read.click();
  await expect(thread(main).getByRole('button', { name: 'Stop reading' })).toBeVisible();
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toBeVisible();
  const [said] = await spoken(yap);
  expect(said.text).toBe(
    'Launch plan. The launch is on Friday, see the doc or github dot com. (code block) Ask Sam about the venue. Done: Book the room.'
  );
  expect(said.messageId).toMatch(/^chat:\d+:1$/);
  await shot(main, '05-answer-speaking');

  // ■ stops it at once.
  await thread(main).getByRole('button', { name: 'Stop reading' }).click();
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toHaveCount(0);
  expect(await speaking(yap)).toBe(false);

  // So does Esc.
  await read.click();
  await expect(thread(main).getByRole('button', { name: 'Stop reading' })).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(thread(main).getByRole('button', { name: 'Read aloud' })).toBeVisible();
  expect(await speaking(yap)).toBe(false);

  // And leaving Chat.
  await read.click();
  await expect.poll(() => speaking(yap)).toBe(true);
  await openView(main, 'Home');
  await expect.poll(() => speaking(yap)).toBe(false);
});

test('"Only when I asked by voice": a dictated question is answered aloud, a typed one quietly', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice', talkBackHandsFree: false });
  await freshChat(main);

  fakeLlm.scriptChat([{ content: 'Typed: the launch is Friday.' }]);
  await send(main, 'Typed question');
  await expect(thread(main)).toContainText('Typed: the launch is Friday.');
  await main.waitForTimeout(500);
  expect(await spoken(yap)).toEqual([]);

  // A dictated question (as the chat's mic hears it): into the box, not
  // sent (hands-free is off), nothing pasted anywhere.
  const delivered = await yap.invoke('e2e_chat_dictation', { text: 'what is on friday' });
  expect(delivered.kind).toBe('the Chat view');
  expect(delivered.submit).toBe(null);
  await expect(box(main)).toHaveValue('what is on friday');
  fakeLlm.scriptChat([{ content: 'On **Friday**: the launch, with the whole team there and the venue booked.' }]);
  await box(main).press('Enter');
  await expect(thread(main)).toContainText('On Friday: the launch, with the whole team there');
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text)).toEqual(['On Friday: the launch, with the whole team there and the venue booked.']);
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toBeVisible();
  // The question says it came by voice, here and in chats.json.
  await expect(thread(main).locator('.cbubble.user').last().getByLabel('Asked by voice')).toBeVisible();
  await shot(main, '06-voice-question-answered-aloud');
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title === 'Typed question');
  expect(conv.messages.map((m) => !!m.voice)).toEqual([false, false, true, false]);
  // A typed message after it is quiet again.
  await yap.invoke('tts_stop');
  fakeLlm.scriptChat([{ content: 'Quiet one.' }]);
  await send(main, 'And typed again');
  await expect(thread(main)).toContainText('Quiet one.');
  await main.waitForTimeout(500);
  expect((await spoken(yap)).length).toBe(1);

  // "Always": every answer.
  await setConfig(yap, main, { speakWhen: 'always' });
  fakeLlm.scriptChat([{ content: 'Spoken anyway.' }]);
  await send(main, 'Typed, but always');
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text).at(-1)).toBe('Spoken anyway.');
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice', talkBackHandsFree: true });
});

test('Talk back hands-free: a dictated question is sent by itself and its answer read out', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice', talkBackHandsFree: true });
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'Sam is booking the venue. It is on Friday.' }]);
  const delivered = await yap.invoke('e2e_chat_dictation', { text: 'Who is booking the venue?' });
  expect(delivered.submit).toBe('enter');
  // Sent without an Enter: the question in the thread, the box empty again.
  await expect(thread(main).locator('.cbubble.user')).toHaveText(['Who is booking the venue?']);
  await expect(box(main)).toHaveValue('');
  await expect(thread(main)).toContainText('Sam is booking the venue.');
  await expect
    .poll(async () => (await spoken(yap)).map((s) => s.sentences))
    .toEqual([['Sam is booking the venue.', 'It is on Friday.']]);
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toBeVisible();
  await shot(main, '07-hands-free-answer-speaking');
  await yap.invoke('tts_stop');

  // Spoken replies off: hands-free is off with them; a dictated question
  // waits in the box — unless it ends with "press enter".
  await setConfig(yap, main, { speakReplies: false });
  await yap.invoke('e2e_chat_dictation', { text: 'Just this one' });
  await expect(box(main)).toHaveValue('Just this one');
  await box(main).fill('');
  fakeLlm.scriptChat([{ content: 'Sent by the voice command.' }]);
  await yap.invoke('e2e_chat_dictation', { text: 'Send this one, press enter' });
  await expect(thread(main).locator('.cbubble.user').last()).toHaveText('Send this one');
  await expect(thread(main)).toContainText('Sent by the voice command.');
  await main.waitForTimeout(400);
  expect((await spoken(yap)).length).toBe(1); // replies are off: not read
  await setConfig(yap, main, { talkBackHandsFree: true });
});

test('a question dictated for the chat after you left it takes you back to Chat with it', async ({
  yap,
  main,
  fakeLlm,
}) => {
  await setConfig(yap, main, { speakReplies: true, speakWhen: 'voice', talkBackHandsFree: true });
  // The main fixture starts on Home: the chat's dictation ends there.
  await expect(main.locator('.chat')).toHaveCount(0);
  fakeLlm.scriptChat([{ content: 'Here you are again.' }]);
  await yap.invoke('e2e_chat_dictation', { text: 'Did I leave the chat?' });
  await expect(thread(main).locator('.cbubble.user').last()).toHaveText('Did I leave the chat?');
  await expect(thread(main)).toContainText('Here you are again.');
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text)).toEqual(['Here you are again.']);
  await yap.invoke('tts_stop');
  await setConfig(yap, main, { speakReplies: false });
});

test("the Notes view's Ask bar answers have ▶ Read aloud too", async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('note_create', { title: 'Voice plan', content: 'The launch is on Friday.' });
  await openView(main, 'Notes');
  await main.locator('.items .item').filter({ hasText: 'Voice plan' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Voice plan');
  // A plain chat call (no tools): the fake answers "other" calls with this.
  const answer = 'It ships on **Friday**. The venue is booked, and Sam sends the invites to the whole team this afternoon.';
  fakeLlm.reply('other', () => answer);
  try {
    await main.getByPlaceholder('Ask anything…').fill('When does it ship?');
    await main.getByPlaceholder('Ask anything…').press('Enter');
    const thread = main.locator('.chatthread');
    await expect(thread).toContainText('It ships on Friday.');
    await thread.getByRole('button', { name: 'Read aloud' }).click();
    await expect(thread.getByRole('button', { name: 'Stop reading' })).toBeVisible();
    const [said] = await spoken(yap);
    expect(said.sentences).toEqual([
      'It ships on Friday.',
      'The venue is booked, and Sam sends the invites to the whole team this afternoon.',
    ]);
    expect(said.messageId).toMatch(/^notes:\d+:\d+$/);
    await shot(main, '08-notes-ask-bar-reading');
    // Leaving Notes stops it.
    await openView(main, 'Home');
    await expect.poll(() => speaking(yap)).toBe(false);
  } finally {
    fakeLlm.reply('other', () => 'OK');
  }
});

test('a dictation starting stops the voice at once (barge-in)', async ({ yap, main }) => {
  await setConfig(yap, main, { speakReplies: false });
  const long = Array.from({ length: 8 }, (_, i) => `This is sentence number ${i + 1} of a long answer.`).join(' ');
  await yap.invoke('tts_speak', { text: long, messageId: 'chat:999:1' });
  await expect.poll(() => speaking(yap)).toBe(true);
  // The hotkey, the bar's Dictate, the chat's mic: all start here. (With no
  // microphone the start fails, and the voice still stops.)
  await yap.invoke('toggle_recording');
  await expect.poll(() => speaking(yap), { timeout: 2_000 }).toBe(false);
  // A recording that did start (a mic here): thrown away.
  await yap.invoke('cancel_recording');
  expect((await spoken(yap)).map((s) => s.messageId)).toEqual(['chat:999:1']);
  // Nothing is said while a dictation records.
  const mics = await yap.invoke('list_audio_devices');
  if (mics.length > 0) {
    await yap.invoke('toggle_recording');
    await expect(yap.invoke('tts_speak', { text: 'Not now.', messageId: 'chat:999:2' })).rejects.toThrow(
      /listening to a dictation/
    );
    await yap.invoke('cancel_recording');
  }
});

const kokoroDir = (yap, ...parts) => path.join(yap.dataDir, 'models', 'tts', ...parts);

test("Kokoro isn't installed: Windows voices read the replies, numbers written out for the ear", async ({ yap, main }) => {
  await setConfig(yap, main, { speakReplies: false, ttsEngine: 'kokoro', ttsVoice: 'bf_emma', ttsWindowsVoice: 'e2e-voice-2', ttsRate: 1 });
  const list = await yap.invoke('tts_voices');
  expect(list.engine).toBe('kokoro');
  expect(list.active).toBe('windows');
  expect(list.kokoroStatus).toMatchObject({ inBuild: true, installed: false });
  expect(list.kokoro.voices).toHaveLength(41);
  expect(list.kokoro.voices.some((v) => v.language === 'ja' || v.language === 'zh')).toBe(false);
  expect(list.kokoro.defaultVoice).toBe('af_bella');
  await yap.invoke('tts_speak', { text: 'Sure! It came to £12,400 on 18 Oct, about 45% over.', messageId: 'chat:900:1' });
  const [said] = await spoken(yap);
  // The Windows voice chosen there, not Kokoro's.
  expect(said).toMatchObject({ engine: 'windows', voice: 'e2e-voice-2' });
  expect(said.text).toBe(
    'Sure! It came to twelve thousand four hundred pounds on the eighteenth of October, about forty-five percent over.'
  );
  await yap.invoke('tts_stop');
  // Kokoro's ▶ needs it downloaded.
  const err = await yap.invoke('tts_preview', { voice: 'bf_emma', rate: 1, engine: 'kokoro' }).catch((e) => String(e));
  expect(err).toContain('Download Kokoro to hear its voices');
  await setConfig(yap, main, { ttsVoice: 'af_bella', ttsWindowsVoice: '' });
});

test('switching replies on offers Kokoro; it downloads, becomes the voice, and deletes back to Windows voices', async ({
  yap,
  main,
  kokoro,
  shot,
}) => {
  await setConfig(yap, main, { speakReplies: false, ttsEngine: 'kokoro', ttsVoice: 'bf_emma', ttsWindowsVoice: '', ttsRate: 1 });
  const dialog = await openSettings(main, 'Voice Assistant');
  const speak = dialog.getByRole('button', { name: 'Speak replies' });

  // Replies on without Kokoro: the offer, and replies stay on (Windows
  // voices meanwhile).
  await speak.click();
  const offer = dialog.getByRole('group', { name: 'Download Kokoro' });
  await expect(offer).toBeVisible();
  await expect(offer).toContainText('Until then Yap speaks with Windows voices');
  await expectStore(yap, 'config.json', (c) => c.speakReplies === true);
  await shot(main, '09-kokoro-offer');
  // "Not now" puts it away and changes nothing.
  await offer.getByRole('button', { name: 'Not now' }).click();
  await expect(offer).toHaveCount(0);
  expect(yap.readJson('config.json').speakReplies).toBe(true);

  // Off and on again: the offer, then the download.
  await speak.click();
  await speak.click();
  await expect(offer).toBeVisible();
  await offer.getByRole('button', { name: /^Download Kokoro \(\d+ MB\)$/ }).click();
  await expect(offer.getByText(/Downloading Kokoro… \d+%/)).toBeVisible();
  await expect(offer.getByRole('button', { name: 'Cancel the download: Downloading Kokoro' })).toBeVisible();
  const krow = dialog.getByRole('group', { name: 'Kokoro' });
  await expect(krow.locator('.mrow')).toContainText(/\d+%/);
  await shot(main, '10-kokoro-downloading');
  await expect(krow.locator('.mrow')).toContainText('Active', { timeout: 20_000 });
  await expect(offer).toHaveCount(0);
  await expect(krow).not.toContainText('Windows voices speak');

  // On disk: the model, every voice, the dictionaries, eSpeak NG (only the
  // data Kokoro reads) with its GPL licence, a NOTICE in each; nothing
  // half-done left behind.
  expect(fs.statSync(kokoroDir(yap, 'kokoro-v1.0', 'model.onnx')).size).toBe(kokoro.modelBytes);
  expect(fs.readdirSync(kokoroDir(yap, 'kokoro-v1.0', 'voices'))).toHaveLength(54);
  expect(fs.existsSync(kokoroDir(yap, 'kokoro-v1.0', 'lexicon', 'gb_gold.json'))).toBe(true);
  expect(fs.readFileSync(kokoroDir(yap, 'kokoro-v1.0', 'NOTICE.txt'), 'utf8')).toContain('Apache');
  expect(fs.readFileSync(kokoroDir(yap, 'espeak-ng-1.52.0', 'COPYING'), 'utf8')).toContain('GNU GENERAL PUBLIC LICENSE');
  expect(fs.readFileSync(kokoroDir(yap, 'espeak-ng-1.52.0', 'NOTICE.txt'), 'utf8')).toContain(
    'Source code: https://github.com/espeak-ng/espeak-ng'
  );
  expect(fs.existsSync(kokoroDir(yap, 'espeak-ng-1.52.0', 'espeak-ng.exe'))).toBe(true);
  expect(fs.existsSync(kokoroDir(yap, 'espeak-ng-1.52.0', 'espeak-ng-data', 'en_dict'))).toBe(true);
  expect(fs.existsSync(kokoroDir(yap, 'espeak-ng-1.52.0', 'espeak-ng-data', 'ru_dict'))).toBe(false);
  expect(fs.readdirSync(kokoroDir(yap)).filter((f) => /partial|installing|unpacked/.test(f))).toEqual([]);
  const list = await yap.invoke('tts_voices');
  expect(list.active).toBe('kokoro');
  expect(list.kokoroStatus).toMatchObject({ installed: true, espeak: true, downloadBytes: 0 });

  // Now Kokoro speaks: ▶ in the chosen voice, then another voice picked.
  const voice = dialog.getByRole('combobox', { name: 'Voice' });
  await expect(voice).toHaveValue('bf_emma');
  await dialog.getByRole('button', { name: 'Preview the voice' }).click();
  await expect(dialog.getByRole('button', { name: 'Stop the preview' })).toBeVisible();
  let [said] = await spoken(yap);
  expect(said).toMatchObject({ messageId: 'preview', engine: 'kokoro', voice: 'bf_emma' });
  await dialog.getByRole('button', { name: 'Stop the preview' }).click();
  await voice.selectOption({ label: 'George' });
  await expectStore(yap, 'config.json', (c) => c.ttsVoice === 'bm_george');
  await shot(main, '11-kokoro-active');
  // A reply: Kokoro, a sentence too short to say alone joined to the next.
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await yap.invoke('tts_speak', { text: 'Sure! The launch is on Friday, at 2:30 pm.', messageId: 'chat:901:1' });
  [said] = await spoken(yap);
  expect(said).toMatchObject({ engine: 'kokoro', voice: 'bm_george' });
  expect(said.sentences).toEqual(['Sure! The launch is on Friday, at two thirty p.m.']);
  await yap.invoke('tts_stop');

  // Delete asks in Yap's own dialog; then Windows voices again.
  await krow.locator('.mrow').hover();
  await krow.getByRole('button', { name: 'Delete model', exact: true }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete Kokoro?' });
  await expect(confirm).toContainText('Spoken replies switch to Windows voices');
  await shot(main, '12-kokoro-delete-confirm');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(krow.getByRole('button', { name: 'Download', exact: true })).toBeVisible();
  expect(fs.existsSync(kokoroDir(yap, 'kokoro-v1.0'))).toBe(false);
  expect(fs.existsSync(kokoroDir(yap, 'espeak-ng-1.52.0'))).toBe(false);
  expect((await yap.invoke('tts_voices')).active).toBe('windows');
  // The choice is still Kokoro: it speaks again once it's back.
  expect(yap.readJson('config.json').ttsEngine).toBe('kokoro');
  await closeSettings(main);
  await setConfig(yap, main, { speakReplies: false, ttsVoice: 'af_bella' });
});

test('a test run never downloads Kokoro from the internet', async ({ yap, kokoro }) => {
  kokoro.setModelUrl(
    'https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/onnx/model.onnx'
  );
  try {
    const err = await yap.invoke('tts_kokoro_download').catch((e) => String(e));
    expect(err).toContain('Test runs never download from the internet');
    expect((await yap.invoke('tts_kokoro_status')).status.installed).toBe(false);
  } finally {
    kokoro.setModelUrl(null);
  }
});

// The maintainer (2026-10-18): "this bubble I do like it, but we need a setting that
// can turn that off". Settings → Voice Assistant → "Show the answer on the
// Yap bar": off, a spoken answer has no card (■ stays on the capsule). (An
// answer that isn't read aloud still shows: assistant::card_on_bar, unit-tested.)
test('"Show the answer on the Yap bar" off: just the voice, with ■ on the bar', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  const dialog = await openSettings(main, 'Voice Assistant');
  const toggle = dialog.getByRole('button', { name: 'Show the answer on the Yap bar' });
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => yap.readJson('config.json').assistantAnswerCard).toBe(false);
  await shot(main, '30-settings-answer-card-off');
  await closeSettings(main);
  await setConfig(yap, main, { speakReplies: true });
  try {
    // Read aloud: the capsule says so, with ■, and no card.
    await inAnotherApp(yap);
    fakeLlm.scriptChat([{ content: 'It is at ten.' }]);
    await ask(yap, 'what time is it on');
    const cap = capsule(yap);
    await expect(cap).toHaveAttribute('data-ask', 'speaking', { timeout: 10_000 });
    await expect(cap.getByRole('button', { name: 'Stop speaking' })).toBeVisible();
    await expect.poll(async () => (await spoken(yap)).map((s) => s.text)).toEqual(['It is at ten.']);
    await expect(answerCard(yap)).toHaveCount(0);
    await shot(yap.overlay, '31-assistant-voice-only');
    await yap.invoke('assistant_stop');

  } finally {
    await setConfig(yap, main, { assistantAnswerCard: true, speakReplies: false });
  }
});

// The maintainer (2026-10-18): "I turned off the voice yet heard the voice still".
// Speak replies (Chat's Voice replies) is the one voice switch: off, the
// shortcut's answer is shown on the bar and nothing is said.
test('voice off: the assistant shortcut answers on the bar and says nothing', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { speakReplies: false });
  await inAnotherApp(yap);
  fakeLlm.scriptChat([{ content: 'It is at ten.' }]);
  await ask(yap, 'what time is it on');
  const card = answerCard(yap);
  await expect(card).toContainText('It is at ten.', { timeout: 10_000 });
  await expect(capsule(yap)).toHaveCount(0);
  await main.waitForTimeout(500);
  expect(await spoken(yap)).toEqual([]);
  await card.getByRole('button', { name: 'Close' }).click();
});

// The maintainer (2026-10-20): "the voice that gets spoken back is so loud, even at
// 1% on the volume mixer". Yap's own volume for it (voice/tts.rs `gain`, squared):
// Settings → Voice Assistant → Volume, and the slider next to Voice replies
// in Chat; either one moves the other.
test('the voice has its own volume, in Settings and next to Voice replies', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Voice Assistant');
  const volume = dialog.getByRole('slider', { name: 'Volume' });
  await volume.scrollIntoViewIfNeeded();
  await expect(volume).toHaveValue('0.5');
  await volume.fill('0.3');
  await expect(dialog.getByText('30%')).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.ttsVolume === 0.3);
  // What's said plays at it.
  await yap.invoke('tts_e2e_spoken', { clear: true });
  // (Windows voices: Kokoro isn't installed at this point of the run.)
  await yap.invoke('tts_preview', { voice: '', rate: 1, engine: 'windows' });
  await expect.poll(async () => (await spoken(yap))[0]?.volume).toBeCloseTo(0.3);
  await yap.invoke('tts_stop');
  await closeSettings(main);

  // Chat: the slider next to Voice replies.
  await openView(main, 'Chat');
  const button = main.getByRole('button', { name: 'Chat options' });
  await button.click();
  const pop = main.getByRole('dialog', { name: 'Chat options' });
  const chatSlider = pop.getByRole('slider', { name: 'Voice volume' });
  await expect(chatSlider).toHaveValue('30');
  await chatSlider.fill('15');
  await expect(pop).toContainText('15%');
  await expectStore(yap, 'config.json', (c) => Math.abs(c.ttsVolume - 0.15) < 1e-6);
  await shot(main, '40-chat-voice-volume');
  await main.keyboard.press('Escape');
  await expect(pop).toHaveCount(0);

  // Settings' copy followed (its auto-save can't put 30% back).
  const again = await openSettings(main, 'Voice Assistant');
  await expect(again.getByRole('slider', { name: 'Volume' })).toHaveValue('0.15');
  await closeSettings(main);
  await expect.poll(() => yap.readJson('config.json').ttsVolume).toBeCloseTo(0.15);
  await setConfig(yap, main, { ttsVolume: 0.5 });
});
