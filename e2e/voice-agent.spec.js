// Spoken replies and the Voice Agent page (src-tauri/src/tts.rs,
// src/lib/ChatView.svelte, Settings → Voice Agent): the Chat view's Voice
// replies toggle (the same setting as Settings' "Speak replies", kept in
// step both ways and through Settings' auto-save), ▶ Read aloud on every
// answer (markdown out, a code block said as "(code block)"), ■ / Esc to
// stop, "When to speak" (only for a question that came by voice, or
// always), "Talk back hands-free" (a dictated question sent by itself), and
// barge-in (a dictation starting stops the voice).
//
// Test mode plays nothing and opens no audio device: two stand-in voices,
// silence of a plausible length on a simulated clock, and everything Yap
// would have said is kept for the debug-only `tts_e2e_spoken`. A dictated
// question is staged with the debug-only `e2e_chat_dictation` (the
// dictation's own steps from cleanup on, as the chat's mic starts one: no
// microphone needed). The "Groq" here is a local fake (support/fake-llm.js).
import { test as base, expect, openView, openSettings, closeSettings, settingsDialog, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

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
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'voice-agent',
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

/** Change config.json as another surface would, and tell Settings' copy. */
async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
}

const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const speaking = async (yap) => (await yap.invoke('tts_state')).speaking;
const box = (main) => main.getByPlaceholder('Type a message...');
const thread = (main) => main.locator('.msgs');
const voiceToggle = (main) => main.getByRole('button', { name: 'Voice replies' });

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

test.beforeEach(async ({ yap }) => {
  await yap.invoke('tts_stop');
  await yap.invoke('tts_e2e_spoken', { clear: true });
});

test('Settings → Voice Agent: spoken replies, the voice and its preview, the wake word', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice', talkBackHandsFree: true, ttsVoice: '', ttsRate: 1 });
  const dialog = await openSettings(main, 'Voice Agent');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Agent' })).toBeVisible();

  // Off by default; what hangs on it is greyed out until it's on.
  const speak = dialog.getByRole('button', { name: 'Speak replies' });
  await expect(speak).toHaveAttribute('aria-pressed', 'false');
  const handsFree = dialog.getByRole('button', { name: 'Talk back hands-free' });
  await expect(handsFree).toBeDisabled();
  await expect(dialog.getByRole('tab', { name: 'When I asked by voice' })).toBeDisabled();

  // The voices Windows gives apps (test mode: two stand-ins), the default named.
  const voice = dialog.getByRole('combobox', { name: 'Voice' });
  await expect(voice.locator('option')).toHaveText([
    'Windows default (Test Voice One)',
    'Test Voice One · en-GB',
    'Test Voice Two · en-US',
  ]);
  await expect(dialog.getByText('Windows voices', { exact: true })).toBeVisible();
  await expect(dialog.getByText("Windows' own voices, on this PC. More natural voices are coming")).toBeVisible();
  // One Voice Agent: the wake word lives here too.
  const wake = dialog.getByRole('group', { name: 'Wake word' });
  await expect(wake.getByRole('button', { name: 'Answer to its name in any dictation' })).toBeVisible();
  await expect(wake.getByPlaceholder('e.g. Jarvis, Nova, Atlas...')).toBeVisible();
  await shot(main, '01-settings-voice-agent');

  // On, then a voice and a speed: saved.
  await speak.click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === true);
  await expect(handsFree).toBeEnabled();
  await expect(handsFree).toHaveAttribute('aria-pressed', 'true');
  await voice.selectOption({ label: 'Test Voice Two · en-US' });
  await dialog.locator('input[type="range"]').fill('1.5');
  await expect(dialog.getByText('1.5×')).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.ttsVoice === 'e2e-voice-2' && c.ttsRate === 1.5);

  // ▶ Preview: a sample in that voice at that speed; ■ stops it.
  await dialog.getByRole('button', { name: 'Preview the voice' }).click();
  await expect(dialog.getByRole('button', { name: 'Stop the preview' })).toBeVisible();
  const [preview] = await spoken(yap);
  expect(preview).toMatchObject({ messageId: 'preview', voice: 'e2e-voice-2', rate: 1.5 });
  expect(preview.sentences.length).toBeGreaterThan(1);
  await shot(main, '02-preview-playing');
  await dialog.getByRole('button', { name: 'Stop the preview' }).click();
  await expect(dialog.getByRole('button', { name: 'Preview the voice' })).toBeVisible();
  expect(await speaking(yap)).toBe(false);

  // The wake word's model and prompt: Language Models → Voice Agent, which
  // points back here for the name.
  await wake.getByRole('button', { name: 'Language Models → Voice Agent' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'Voice Agent' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByText('Agent name:')).toContainText('Yap');
  await shot(main, '03-language-models-points-back');
  await dialog.getByRole('button', { name: 'Name and voice in Voice Agent' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Agent' })).toBeVisible();

  await speak.click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === false);
  await closeSettings(main);
  await setConfig(yap, main, { ttsVoice: '', ttsRate: 1 });
});

test("Chat's Voice replies toggle is Settings' Speak replies, and survives auto-save", async ({ yap, main, shot }) => {
  await setConfig(yap, main, { speakReplies: false, speakWhen: 'voice' });
  await openView(main, 'Chat');
  await expect(voiceToggle(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(voiceToggle(main)).toHaveAttribute('title', 'Voice replies off');
  await voiceToggle(main).click();
  await expect(voiceToggle(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(voiceToggle(main)).toHaveAttribute('title', /questions you ask by voice/);
  await expectStore(yap, 'config.json', (c) => c.speakReplies === true);
  await shot(main, '04-chat-voice-on');

  // Settings' copy adopted it: shown on, and another setting's save keeps it.
  const dialog = await openSettings(main, 'Voice Agent');
  await expect(dialog.getByRole('button', { name: 'Speak replies' })).toHaveAttribute('aria-pressed', 'true');
  await dialog.getByRole('tab', { name: 'Always' }).click();
  await expectStore(yap, 'config.json', (c) => c.speakWhen === 'always');
  expect(yap.readJson('config.json').speakReplies).toBe(true);

  // And the other way: off in Settings, off in Chat.
  await dialog.getByRole('button', { name: 'Speak replies' }).click();
  await expectStore(yap, 'config.json', (c) => c.speakReplies === false);
  await closeSettings(main);
  await expect(voiceToggle(main)).toHaveAttribute('aria-pressed', 'false');
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
    'Launch plan. The launch is on Friday, see the doc or github.com. (code block) Ask Sam about the venue. Done: Book the room.'
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
