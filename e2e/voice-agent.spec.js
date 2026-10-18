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
//
// Kokoro (tts/kokoro.rs) is the default engine: the engine row, its download
// row and the "Download Kokoro" offer when replies go on, the 54 voices
// grouped by accent and gender (af_bella the default), Windows voices
// speaking until it's installed. Its download comes from a local server
// (support/kokoro.js, the debug-only YAP_TTS_SOURCES): never the internet.
// Once installed, test mode's Kokoro is silence too.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, settingsDialog, expectStore } from './support/fixtures.js';
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
        name: 'voice-agent',
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

test('Settings → Voice Agent: spoken replies, the engine, the voice and its preview, the wake word', async ({ yap, main, shot }) => {
  await setConfig(yap, main, {
    speakReplies: false,
    speakWhen: 'voice',
    talkBackHandsFree: true,
    ttsEngine: 'kokoro',
    ttsVoice: 'af_bella',
    ttsWindowsVoice: '',
    ttsRate: 1,
  });
  const dialog = await openSettings(main, 'Voice Agent');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Agent' })).toBeVisible();

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
  await expect(krow.locator('.mrow')).toContainText('54 voices');
  await expect(krow.getByRole('button', { name: 'Download', exact: true })).toBeVisible();
  await expect(krow).toContainText('Until then, Windows voices speak');

  // Kokoro's 54 voices, grouped by accent and gender; Bella the default.
  const voice = dialog.getByRole('combobox', { name: 'Voice' });
  await expect(voice).toHaveValue('af_bella');
  await expect(voice.locator('option:checked')).toHaveText('Bella (default)');
  await expect(voice.locator('option')).toHaveCount(54);
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
  // One Voice Agent: the wake word lives here too.
  const wake = dialog.getByRole('group', { name: 'Wake word' });
  await expect(wake.getByRole('button', { name: 'Answer to its name in any dictation' })).toBeVisible();
  await expect(wake.getByPlaceholder('e.g. Jarvis, Nova, Atlas...')).toBeVisible();
  await shot(main, '01-settings-voice-agent');

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
  await dialog.locator('input[type="range"]').fill('1.5');
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
  await setConfig(yap, main, { ttsEngine: 'kokoro', ttsWindowsVoice: '', ttsRate: 1 });
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
  expect(list.kokoro.voices).toHaveLength(54);
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
  const dialog = await openSettings(main, 'Voice Agent');
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
  await expect(offer.getByText(/Downloading… \d+%/)).toBeVisible();
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
