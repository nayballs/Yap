// ElevenLabs as the voice of spoken replies (src-tauri/src/tts/elevenlabs.rs,
// tts/cloud.rs, tts.rs, src/lib/ElevenLabsSettings.svelte, VoiceSettings
// .svelte), against a local stand-in (support/fake-elevenlabs.js, the
// debug-only YAP_ELEVENLABS_URL) — never the internet, never a real key:
// - Settings → Voice Assistant → Voice: "Speaks with" On this PC /
//   ElevenLabs, the privacy line while ElevenLabs is picked, the masked key
//   and "Get your API key" (their API-keys page, through the opener), the
//   account's voices (its own, then ElevenLabs' defaults), ▶ a voice's own
//   sample (fetched when clicked, without the key), the model (Flash v2.5
//   recommended) and the month's characters; a key it won't take says so.
// - A Chat answer read aloud through ElevenLabs, sentence by sentence: the
//   requests (`pcm_24000`, the model, the key in its header only).
// - Failures: the quota used up with no local voice → one notice, silence;
//   a key it won't take with Kokoro downloaded → Kokoro reads the answer
//   (the failed sentence included), one notice, and the next answer goes
//   straight to Kokoro.
//
// Test mode plays nothing and opens no audio device: the PCM is decoded and
// "played" on a simulated clock, and what would have been said is kept for
// the debug-only `tts_e2e_spoken`. Kokoro comes from the local stand-in
// download (support/kokoro.js) and is silence in test mode.
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeKokoro } from './support/kokoro.js';
import { startFakeElevenLabs, KEY, GEORGE, ALICE, OWN } from './support/fake-elevenlabs.js';
import { RUNS } from './support/yap.js';

const LLM_KEY = 'e2e-fake-key';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  eleven: [
    async ({}, use) => {
      const fake = await startFakeElevenLabs();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  kokoro: [
    async ({}, use) => {
      const fake = await startFakeKokoro(path.join(RUNS, 'kokoro-sources-elevenlabs'));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, eleven, kokoro }, use) => {
      await use({
        name: 'voice-elevenlabs',
        env: { ...kokoro.env, ...eleven.env },
        config: {
          ppApiKeys: { groq: LLM_KEY },
          llmScopes: {
            chat: {
              enabled: true,
              provider: 'groq',
              baseUrl: fakeLlm.base,
              model: 'openai/gpt-oss-20b',
              apiKey: LLM_KEY,
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
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const toasts = (main) => main.getByRole('status');

async function ask(main, question) {
  await box(main).fill(question);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

const ELEVEN = {
  ttsProvider: 'elevenlabs',
  ttsCloudKeys: { elevenlabs: KEY },
  ttsCloud: { elevenlabs: { voice: GEORGE, voiceName: 'George', model: '' } },
};

test.beforeEach(async ({ yap, eleven }) => {
  await yap.invoke('tts_stop');
  // Also forgets ElevenLabs' last failure, so each test starts afresh.
  await yap.invoke('tts_e2e_spoken', { clear: true });
  eleven.reset();
});

test('Settings: pick ElevenLabs, paste a key, its voices load, ▶ plays a sample, model and usage', async ({
  yap,
  main,
  eleven,
  shot,
}) => {
  await setConfig(yap, main, { ttsProvider: 'local', ttsCloudKeys: {}, ttsCloud: {}, ttsEngine: 'kokoro', ttsRate: 1 });
  const dialog = await openSettings(main, 'Voice Assistant');
  const who = dialog.getByRole('tablist', { name: 'Speaks with' });
  const privacy = dialog.getByText('The words Yap speaks are sent to ElevenLabs.');

  // On this PC by default: Kokoro's rows, no privacy line, nothing asked of ElevenLabs.
  await expect(who.getByRole('tab', { name: 'On this PC' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.getByRole('tablist', { name: 'Engine' })).toBeVisible();
  await expect(privacy).toHaveCount(0);

  // ElevenLabs: the privacy line under the choice, the local rows gone.
  await who.getByRole('tab', { name: 'ElevenLabs' }).click();
  await expectStore(yap, 'config.json', (c) => c.ttsProvider === 'elevenlabs');
  await expect(privacy).toBeVisible();
  await expect(dialog.getByRole('tablist', { name: 'Engine' })).toHaveCount(0);
  await expect(dialog.getByText("Paste your key to hear ElevenLabs' voices. Until then, Yap's own voice speaks.")).toBeVisible();
  // No Kokoro downloaded: it says the answer would go unsaid if ElevenLabs fails.
  await expect(dialog.getByText(/If ElevenLabs doesn't answer, nothing is read aloud/)).toBeVisible();
  expect(eleven.requests).toEqual([]);
  await dialog.getByRole('group', { name: 'Voice', exact: true }).scrollIntoViewIfNeeded();
  await shot(main, '00-elevenlabs-picked-no-key');

  // "Get your API key" opens their API-keys page in the browser.
  await dialog.getByRole('link', { name: 'Get your API key' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://elevenlabs.io/app/settings/api-keys');

  // A key ElevenLabs won't take says so.
  const keyField = dialog.getByLabel('ElevenLabs API key');
  await keyField.fill('sk_not_a_real_key_000000');
  await expect(dialog.getByText("ElevenLabs didn't accept it: API key not accepted.")).toBeVisible();
  await expect(dialog.getByRole('radiogroup', { name: 'ElevenLabs voice' })).toHaveCount(0);

  // The right key: saved like the other cloud keys, its voices listed.
  await keyField.fill(KEY);
  await expect(dialog.getByText('Key works · 3 voices')).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.ttsCloudKeys?.elevenlabs === KEY);
  const list = dialog.getByRole('radiogroup', { name: 'ElevenLabs voice' });
  await expect(list.locator('.vgroup')).toHaveText(['Your voices', 'ElevenLabs voices']);
  await expect(list.getByRole('radio')).toHaveCount(3);
  await expect(list.getByRole('radio', { name: 'Nathan (cloned)' })).toBeVisible();
  // George, the default, chosen until another is picked.
  await expect(list.getByRole('radio', { name: 'George' })).toHaveAttribute('aria-checked', 'true');
  await expect(list.locator('.vrow', { hasText: 'Alice' })).toContainText('British · female · middle aged');
  // The key went in its header, never in an address, and only to ElevenLabs' stand-in.
  const listed = eleven.requests.filter((r) => r.path === '/v2/voices');
  expect(listed.at(-1)).toMatchObject({ method: 'GET', key: KEY });
  expect(eleven.requests.every((r) => !`${r.path}${r.query}`.includes(KEY))).toBe(true);

  // The model: Flash v2.5, the lowest-latency one, recommended; a model
  // that can't do text to speech isn't offered.
  const model = dialog.getByRole('combobox', { name: 'Model' });
  await expect(model).toHaveValue('eleven_flash_v2_5');
  await expect(model.locator('option')).toHaveText(['Flash v2.5 · Recommended', 'Multilingual v2']);
  // The month's characters.
  await expect(dialog.getByText('1,234 of 10,000 characters used this month', { exact: false })).toBeVisible();

  // Another voice: saved with its name.
  await list.getByRole('radio', { name: 'Alice' }).click();
  await expectStore(yap, 'config.json', (c) => c.ttsCloud?.elevenlabs?.voice === ALICE && c.ttsCloud.elevenlabs.voiceName === 'Alice');
  await expect(list.getByRole('radio', { name: 'Alice' })).toHaveAttribute('aria-checked', 'true');
  // Another model, and back.
  await model.selectOption('eleven_multilingual_v2');
  await expectStore(yap, 'config.json', (c) => c.ttsCloud?.elevenlabs?.model === 'eleven_multilingual_v2' && c.ttsCloud.elevenlabs.voice === ALICE);
  await expect(dialog.getByText('Richer, slower to start, 29 languages')).toBeVisible();
  await model.selectOption('eleven_flash_v2_5');
  await expectStore(yap, 'config.json', (c) => c.ttsCloud?.elevenlabs?.model === 'eleven_flash_v2_5');

  // Leaving the field shows the key masked.
  await dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' }).click();
  await expect(dialog.locator('.keytext')).toHaveText(`${KEY.slice(0, 3)}…${KEY.slice(-4)}`);
  await expect(dialog).not.toContainText(KEY);
  await list.scrollIntoViewIfNeeded();
  await shot(main, '01-elevenlabs-settings');

  // ▶ Alice: her own sample, fetched now, without the key; no characters used.
  eleven.requests.length = 0;
  await list.getByRole('button', { name: 'Preview Alice' }).click();
  await expect(list.getByRole('button', { name: 'Stop Alice' })).toBeVisible();
  const [preview] = await spoken(yap);
  expect(preview).toMatchObject({ messageId: 'preview', engine: 'elevenlabs', voice: ALICE });
  await expect.poll(() => eleven.requests.map((r) => [r.path, r.key])).toEqual([['/samples/alice.wav', null]]);
  expect(eleven.speech()).toEqual([]);
  await shot(main, '02-elevenlabs-preview-playing');
  await list.getByRole('button', { name: 'Stop Alice' }).click();
  await expect(list.getByRole('button', { name: 'Preview Alice' })).toBeVisible();
  expect(await speaking(yap)).toBe(false);

  // ▶ on a voice without a sample says the preview sentence through ElevenLabs.
  await list.getByRole('button', { name: 'Preview Nathan (cloned)' }).click();
  await expect.poll(() => eleven.speech().map((r) => r.path)).toContain(`/v1/text-to-speech/${OWN}/stream`);
  await yap.invoke('tts_stop');

  // A key without permission to read its usage: no usage line, nothing else lost.
  eleven.usage = false;
  await who.getByRole('tab', { name: 'On this PC' }).click();
  await who.getByRole('tab', { name: 'ElevenLabs' }).click();
  await expect(dialog.getByText('Key works · 3 voices')).toBeVisible();
  await expect(dialog.getByText(/characters used this month/)).toHaveCount(0);

  await closeSettings(main);
  await setConfig(yap, main, { ttsProvider: 'local' });
});

test('a Chat answer is read aloud by ElevenLabs, sentence by sentence', async ({ yap, main, eleven, fakeLlm }) => {
  await setConfig(yap, main, { ...ELEVEN, speakReplies: true, speakWhen: 'always', ttsRate: 1 });
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'The launch is on **Friday**. Sam books the room at 2:30 pm.' }]);
  await ask(main, 'When is the launch?');
  await expect(thread(main)).toContainText('The launch is on Friday.');
  await expect.poll(async () => (await spoken(yap)).length).toBe(1);
  const [said] = await spoken(yap);
  expect(said).toMatchObject({ engine: 'elevenlabs', voice: GEORGE, fallback: null });
  expect(said.sentences).toEqual(['The launch is on Friday.', 'Sam books the room at two thirty p.m.']);
  // One request a sentence, raw PCM at 24 kHz, the fastest model, the key in its header.
  await expect.poll(() => eleven.speech().length).toBe(2);
  for (const [i, r] of eleven.speech().entries()) {
    expect(r.path).toBe(`/v1/text-to-speech/${GEORGE}/stream`);
    expect(r.query).toBe('?output_format=pcm_24000');
    expect(r.key).toBe(KEY);
    expect(r.body).toEqual({ text: said.sentences[i], model_id: 'eleven_flash_v2_5' });
  }
  await expect(thread(main).getByRole('status').filter({ hasText: 'Speaking…' })).toBeVisible();
  // Played to its end; no notice.
  await expect.poll(() => speaking(yap), { timeout: 10_000 }).toBe(false);
  await expect(toasts(main).filter({ hasText: "didn't answer" })).toHaveCount(0);
  await setConfig(yap, main, { ttsProvider: 'local', speakReplies: false, speakWhen: 'voice' });
});

test('quota used up and no local voice: one notice, and the answer goes unsaid', async ({ yap, main, eleven, fakeLlm, shot }) => {
  expect((await yap.invoke('tts_kokoro_status')).status.installed).toBe(false);
  await setConfig(yap, main, { ...ELEVEN, ttsEngine: 'kokoro', speakReplies: true, speakWhen: 'always' });
  eleven.fail(402);
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'First answer. It has two sentences.' }]);
  await ask(main, 'Anything?');
  const notice = toasts(main).filter({ hasText: "ElevenLabs didn't answer (quota used up)" });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("Yap's own voice isn't downloaded, so this answer wasn't read aloud.");
  await expect(notice).toContainText('Your ElevenLabs plan has no characters left this month.');
  await shot(main, '03-quota-no-local-voice');
  // Asked once, never again for that answer; nothing else said.
  expect(eleven.speech()).toHaveLength(1);
  await expect.poll(() => speaking(yap)).toBe(false);
  expect((await spoken(yap)).map((s) => [s.engine, s.fallback])).toEqual([['elevenlabs', null]]);

  // The next answer fails the same way: no second notice.
  await notice.getByRole('button', { name: 'Close', exact: true }).click();
  fakeLlm.scriptChat([{ content: 'Second answer.' }]);
  await ask(main, 'And now?');
  await expect.poll(() => eleven.speech().length).toBe(2);
  await main.waitForTimeout(800);
  await expect(toasts(main).filter({ hasText: "didn't answer" })).toHaveCount(0);
  await setConfig(yap, main, { ttsProvider: 'local', speakReplies: false, speakWhen: 'voice' });
});

test("a key ElevenLabs won't take: Kokoro reads the answer, with one notice, and the next answer goes straight to Kokoro", async ({
  yap,
  main,
  eleven,
  fakeLlm,
  shot,
}) => {
  // Kokoro downloaded (from the local stand-in): Yap's own voice.
  await yap.invoke('tts_kokoro_download');
  expect((await yap.invoke('tts_kokoro_status')).status.installed).toBe(true);
  await setConfig(yap, main, {
    ...ELEVEN,
    ttsCloudKeys: { elevenlabs: 'sk_revoked_key_0000000000' },
    ttsEngine: 'kokoro',
    ttsVoice: 'bf_emma',
    speakReplies: true,
    speakWhen: 'always',
  });
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'Gale is on the beach. Pull him out of the portal.' }]);
  await ask(main, 'Where is Gale?');
  const notice = toasts(main).filter({ hasText: "ElevenLabs didn't answer (API key not accepted)" });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("Speaking with Yap's own voice.");
  await expect(notice).toContainText('Check your ElevenLabs API key in Settings → Voice Assistant.');
  // ElevenLabs asked once (401); Kokoro read the whole answer, the failed sentence included.
  await expect.poll(async () => (await spoken(yap)).map((s) => [s.engine, s.fallback])).toEqual([
    ['elevenlabs', null],
    ['kokoro', 'ElevenLabs: API key not accepted'],
  ]);
  const [, local] = await spoken(yap);
  expect(local).toMatchObject({ voice: 'bf_emma', sentences: ['Gale is on the beach.', 'Pull him out of the portal.'] });
  expect(eleven.speech()).toHaveLength(1);
  expect(eleven.speech()[0].key).toBe('sk_revoked_key_0000000000');
  await shot(main, '04-fallback-to-kokoro-notice');

  // The next answer: straight to Kokoro (ElevenLabs just said no), no second notice.
  await notice.getByRole('button', { name: 'Close', exact: true }).click();
  await yap.invoke('tts_e2e_spoken').then(() => {});
  fakeLlm.scriptChat([{ content: 'He is a wizard.' }]);
  await ask(main, 'Who is he?');
  await expect.poll(async () => (await spoken(yap)).at(-1)?.engine).toBe('kokoro');
  expect((await spoken(yap)).at(-1)).toMatchObject({ text: 'He is a wizard.', fallback: null });
  expect(eleven.speech()).toHaveLength(1);
  await main.waitForTimeout(500);
  await expect(toasts(main).filter({ hasText: "didn't answer" })).toHaveCount(0);

  // The notice's button opens Settings on the voice.
  await yap.invoke('tts_stop');
  await yap.invoke('alert_action', { action: 'openVoiceSettings' });
  const dialog = main.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByRole('heading', { level: 1, name: 'Voice Assistant' })).toBeVisible();
  // Kokoro is there to stand in now: Settings says so.
  await expect(dialog.getByText(/Yap's own voice reads the\s+answer instead/)).toBeVisible();
  await closeSettings(main);
  await setConfig(yap, main, { ttsProvider: 'local', speakReplies: false, speakWhen: 'voice', ttsVoice: 'af_bella' });
});
