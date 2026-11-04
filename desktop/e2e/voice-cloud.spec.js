// OpenAI, Gemini and Groq as the voice of spoken replies, on the keys Language
// Models already has (desktop/src-tauri/src/voice/tts/hosted.rs, openai.rs, gemini.rs,
// groq.rs; desktop/src/lib/settings/VoiceSettings.svelte, HostedVoiceSettings.svelte),
// against a local stand-in (support/fake-cloud-tts.js, the debug-only
// YAP_<PROVIDER>_TTS_URL) — never the internet, never a real key — and
// "Turn other apps down while Yap speaks" (voice/duck.rs):
// - Settings → Voice Assistant → Voice: a provider is offered only once
//   Language Models has its key; picked, the privacy line names it, its
//   voices list with ▶ (a fraction of a penny, the tooltip says) and its model;
// - ▶ a voice: the request each provider is sent (body, path, the key in its
//   header only);
// - an answer read aloud through Groq: a sentence over Orpheus' 200
//   characters goes as pieces;
// - a provider that fails with no local voice: one notice, pointing at
//   Language Models;
// - the ducking switch: on by default, saved.
//
// Test mode plays nothing and opens no audio device (and never touches
// another app's sound): the audio is decoded and "played" on a simulated
// clock, and what would have been said is kept for the debug-only
// `tts_e2e_spoken`.
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeCloudTts, OPENAI_KEY, GEMINI_KEY, GROQ_KEY } from './support/fake-cloud-tts.js';

const test = base.extend({
  tts: [
    async ({}, use) => {
      const fake = await startFakeCloudTts();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ tts }, use) => {
      await use({ name: 'voice-cloud', env: { ...tts.env }, config: { ppApiKeys: {} } });
    },
    { scope: 'worker' },
  ],
});

const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const speaking = async (yap) => (await yap.invoke('tts_state')).speaking;
const toasts = (main) => main.getByRole('status');
const KEYS = { openai: OPENAI_KEY, gemini: GEMINI_KEY, groq: GROQ_KEY };
// The preview's first sentence (voice/tts.rs PREVIEW_TEXT).
const FIRST = "Hi, I'm the voice that reads Chat's answers.";

test.beforeEach(async ({ yap, tts }) => {
  await yap.invoke('tts_stop');
  await yap.invoke('tts_e2e_spoken', { clear: true });
  tts.reset();
});

test('a provider is offered once Language Models has its key, and its voices, model and ▶ work', async ({ yap, main, tts, shot }) => {
  await setConfig(yap, main, { ttsProvider: 'local', ppApiKeys: {}, ttsCloud: {}, ttsRate: 1 });
  const dialog = await openSettings(main, 'Voice Assistant');

  // No OpenAI, Gemini or Groq key: just this PC and ElevenLabs, side by side,
  // and a hint where the others come from.
  const who = dialog.getByRole('tablist', { name: 'Speaks with' });
  await expect(who.getByRole('tab')).toHaveText(['On this PC', 'ElevenLabs']);

  // The keys in Language Models: the three show, now a list.
  await setConfig(yap, main, { ppApiKeys: { ...KEYS } });
  const pick = dialog.getByRole('combobox', { name: 'Speaks with' });
  await expect(pick).toBeVisible();
  await expect(pick.locator('option')).toHaveText(['On this PC', 'ElevenLabs', 'OpenAI', 'Gemini', 'Groq']);
  expect(tts.requests).toEqual([]);

  // OpenAI: the privacy line, which key, its voices (Marin first and chosen), its model.
  await pick.selectOption('openai');
  await expectStore(yap, 'config.json', (c) => c.ttsProvider === 'openai');
  await expect(dialog.getByText('The words Yap speaks are sent to OpenAI.')).toBeVisible();
  await expect(dialog.getByText(/Uses your OpenAI API key from Language Models/)).toContainText(`${OPENAI_KEY.slice(0, 3)}…${OPENAI_KEY.slice(-4)}`);
  await expect(dialog).not.toContainText(OPENAI_KEY);
  const list = dialog.getByRole('radiogroup', { name: 'OpenAI voice' });
  await expect(list.getByRole('radio')).toHaveCount(13);
  await expect(list.getByRole('radio', { name: 'Marin' })).toHaveAttribute('aria-checked', 'true');
  const model = dialog.getByRole('combobox', { name: 'Model' });
  await expect(model).toHaveValue('gpt-4o-mini-tts');
  await expect(model.locator('option').first()).toHaveText('GPT-4o mini TTS · Recommended');
  // ▶'s tooltip says what a sample costs.
  await expect(list.getByRole('button', { name: 'Preview Cedar' })).toHaveAttribute('title', 'Say a sample sentence: a fraction of a penny on your OpenAI key');
  await list.scrollIntoViewIfNeeded();
  await shot(main, '00-openai-voices');

  // Pick Cedar, ▶ it: streamed PCM, the key in its header only.
  await list.getByRole('radio', { name: 'Cedar' }).click();
  await expectStore(yap, 'config.json', (c) => c.ttsCloud?.openai?.voice === 'cedar' && c.ttsCloud.openai.voiceName === 'Cedar');
  await list.getByRole('button', { name: 'Preview Cedar' }).click();
  await expect.poll(() => tts.speech('openai').length).toBeGreaterThanOrEqual(1);
  const [asked] = tts.speech('openai');
  expect(asked.path).toBe('/v1/audio/speech');
  expect(asked.auth).toBe(`Bearer ${OPENAI_KEY}`);
  expect(asked.body).toEqual({ model: 'gpt-4o-mini-tts', input: FIRST, voice: 'cedar', response_format: 'pcm', stream_format: 'sse' });
  expect(tts.requests.every((r) => !`${r.path}${r.query}`.includes(OPENAI_KEY))).toBe(true);
  const [preview] = await spoken(yap);
  expect(preview).toMatchObject({ messageId: 'preview', engine: 'openai', voice: 'cedar' });
  await yap.invoke('tts_stop');

  // Gemini: its voices with Google's words for them; ▶ Puck asks the Interactions API.
  await pick.selectOption('gemini');
  await expect(dialog.getByText('The words Yap speaks are sent to Gemini.')).toBeVisible();
  const glist = dialog.getByRole('radiogroup', { name: 'Gemini voice' });
  await expect(glist.getByRole('radio')).toHaveCount(30);
  await expect(glist.locator('.vrow', { hasText: 'Puck' })).toContainText('Upbeat');
  await expect(dialog.getByRole('combobox', { name: 'Model' }).locator('option')).toHaveText([
    'Gemini 3.8 Flash-Lite TTS · Recommended',
    'Gemini 3.8 Flash TTS',
  ]);
  await glist.getByRole('button', { name: 'Preview Puck' }).click();
  await expect.poll(() => tts.speech('gemini').length).toBeGreaterThanOrEqual(1);
  const [g] = tts.speech('gemini');
  expect(g.path).toBe('/v1beta/interactions');
  expect(g.auth).toBe(GEMINI_KEY);
  expect(g.body).toEqual({
    model: 'gemini-3.8-flash-lite-tts',
    input: [{ type: 'user_input', content: [{ type: 'text', text: FIRST }] }],
    response_format: { type: 'audio' },
    generation_config: { speech_config: [{ voice: 'Puck' }] },
    stream: true,
  });
  await expect.poll(async () => (await spoken(yap)).at(-1)?.engine).toBe('gemini');
  await yap.invoke('tts_stop');

  // Groq: Orpheus' six voices, one model; ▶ Troy asks for a WAV.
  await pick.selectOption('groq');
  const qlist = dialog.getByRole('radiogroup', { name: 'Groq voice' });
  await expect(qlist.getByRole('radio')).toHaveCount(6);
  await expect(qlist.getByRole('radio', { name: 'Hannah' })).toHaveAttribute('aria-checked', 'true');
  await expect(dialog.getByText("Groq's Orpheus speaks at its own pace")).toBeVisible();
  await qlist.getByRole('button', { name: 'Preview Troy' }).click();
  await expect.poll(() => tts.speech('groq').length).toBeGreaterThanOrEqual(1);
  const [q] = tts.speech('groq');
  expect(q.path).toBe('/openai/v1/audio/speech');
  expect(q.auth).toBe(`Bearer ${GROQ_KEY}`);
  expect(q.body).toEqual({ model: 'canopylabs/orpheus-v1-english', input: FIRST, voice: 'troy', response_format: 'wav' });
  await expect.poll(async () => (await spoken(yap)).at(-1)?.engine).toBe('groq');
  await yap.invoke('tts_stop');
  await qlist.scrollIntoViewIfNeeded();
  await shot(main, '01-groq-voices');

  // Back to this PC: no request leaves for a cloud voice.
  await pick.selectOption('local');
  await expectStore(yap, 'config.json', (c) => c.ttsProvider === 'local');
  await expect(dialog.getByRole('tablist', { name: 'Engine' })).toBeVisible();
  await expect(dialog.getByText(/The words Yap speaks are sent to/)).toHaveCount(0);
  await closeSettings(main);
  await setConfig(yap, main, { ttsProvider: 'local', ppApiKeys: {} });
});

test('an answer read by Groq: a sentence over 200 characters goes in pieces', async ({ yap, main, tts }) => {
  await setConfig(yap, main, { ttsProvider: 'groq', ppApiKeys: { groq: GROQ_KEY }, ttsCloud: { groq: { voice: 'daniel', voiceName: 'Daniel', model: '' } } });
  const long =
    'The plan for the launch has three parts, and each one needs its own owner, its own date and its own list of what could go wrong, so that nobody is surprised when the week arrives and the room is full of people waiting.';
  expect(long.length).toBeGreaterThan(200);
  await yap.invoke('tts_speak', { text: `Here it is. ${long}`, messageId: 'groq-long' });
  await expect.poll(async () => (await spoken(yap)).length).toBe(1);
  const [said] = await spoken(yap);
  expect(said).toMatchObject({ engine: 'groq', voice: 'daniel', messageId: 'groq-long' });
  await expect.poll(() => speaking(yap), { timeout: 15_000 }).toBe(false);
  const inputs = tts.speech('groq').map((r) => r.body.input);
  expect(inputs.length).toBeGreaterThanOrEqual(3);
  expect(inputs.every((i) => [...i].length <= 200)).toBe(true);
  expect(inputs.slice(1).join(' ')).toBe(long);
  expect(tts.speech('groq').every((r) => r.body.voice === 'daniel' && r.auth === `Bearer ${GROQ_KEY}`)).toBe(true);
  await expect(toasts(main).filter({ hasText: "didn't answer" })).toHaveCount(0);
  await setConfig(yap, main, { ttsProvider: 'local', ppApiKeys: {} });
});

test("OpenAI out of credit with no local voice: one notice, pointing at the key's account", async ({ yap, main, tts }) => {
  expect((await yap.invoke('tts_kokoro_status')).status.installed).toBe(false);
  await setConfig(yap, main, { ttsProvider: 'openai', ttsEngine: 'kokoro', ppApiKeys: { openai: OPENAI_KEY }, ttsCloud: {} });
  tts.fail('openai', 429);
  await yap.invoke('tts_speak', { text: 'First answer. It has two sentences.', messageId: 'openai-quota' });
  const notice = toasts(main).filter({ hasText: "OpenAI didn't answer (quota used up)" });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("Yap's own voice isn't downloaded, so this answer wasn't read aloud.");
  await expect(notice).toContainText('Your OpenAI account is out of credit, or at its limit for today.');
  expect(tts.speech('openai')).toHaveLength(1);
  await expect.poll(() => speaking(yap)).toBe(false);
  await notice.getByRole('button', { name: 'Close', exact: true }).click();
  await setConfig(yap, main, { ttsProvider: 'local', ppApiKeys: {} });
});

test('"Turn other apps down while Yap speaks": on by default, and saved', async ({ yap, main, shot }) => {
  expect((await yap.invoke('get_config')).duckOtherApps).toBe(true);
  const dialog = await openSettings(main, 'Voice Assistant');
  const toggle = dialog.getByRole('button', { name: 'Turn other apps down while Yap speaks' });
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '02-duck-other-apps');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.duckOtherApps === false);
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.duckOtherApps === true);
  await closeSettings(main);
});
