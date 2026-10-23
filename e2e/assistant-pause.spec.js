// "Stop listening when I pause" (src-tauri/src/pause.rs, pipeline.rs;
// Settings → Voice Assistant): once you've said something to the Voice
// Assistant, a pause ends the question as a second press of its shortcut
// would — never before you've spoken, never under a held push-to-talk key.
//
// The audio is played in the mic's place by the debug-only
// `e2e_assistant_audio`: tones for talk, quiet stretches over a room's hiss
// (or a noisier room), with the shortcut pressed through the real key path —
// toggle, a held push-to-talk key, or a double-tap that locks it hands-free.
// It says whether the recording ended before its audio did (the pause ended
// it) and how much was played. The live preview is off here: the stub's
// preview line is no words at all ("[STT stub: …]"), which holds a question
// open (pause.rs' words check, unit-tested there), so the level alone
// decides, as with the preview off. The stub then transcribes the audio and
// the question goes to a local fake Chat model (support/fake-llm.js).
import path from 'node:path';
import { test as base, expect, openSettings, settingsDialog, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { SCREENSHOTS } from './support/yap.js';

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
        name: 'assistant-pause',
        config: {
          streamingPartials: false,
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

/** Someone talking at a desk mic (dBFS RMS); the room is −62 unless said. */
const TALK = -24;
const tone = (secs, db = TALK) => ({ kind: 'tone', secs, db });
const quiet = (secs) => ({ kind: 'quiet', secs });
/** The pause that ends a question (pause.rs `PAUSE_MS`). */
const PAUSE = 1_500;

/** A question from audio: `{ endedByItself, listenedMs }`. */
const ask = (yap, parts, opts = {}) => yap.invoke('e2e_assistant_audio', { parts, ...opts });
const capsule = (yap) => yap.overlay.locator('.capsule');
/** Questions the fake Chat model was asked, as the stub heard them. */
const asked = (fakeLlm) => fakeLlm.requests.filter((r) => String(r.user).includes('STT stub'));

test.beforeEach(async ({ yap }) => {
  await yap.invoke('assistant_stop');
  await yap.invoke('tts_stop');
});

test('Settings → Voice Assistant: "Stop listening when I pause", on by default', async ({ yap, main, shot }) => {
  expect((await yap.invoke('get_config')).assistantStopOnPause).toBe(true);
  const dialog = await openSettings(main, 'Voice Assistant');
  const row = dialog.locator('[data-setting="stop-on-pause"]');
  await expect(row).toContainText('Stop listening when I pause');
  await expect(row).toContainText("A pause after your question sends it, so you needn't press the shortcut again");
  const toggle = row.getByRole('button', { name: 'Stop listening when I pause' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  // The shortcut's own line says so too.
  const shortcut = dialog.locator('[data-setting="assistant-shortcut"]');
  await expect(shortcut).toContainText('Press it and ask: Yap stops listening when you pause.');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '01-stop-on-pause-on');

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.assistantStopOnPause === false);
  await expect(shortcut).toContainText('Press it, ask, press it again.');
  await shot(main, '02-stop-on-pause-off');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.assistantStopOnPause === true);

  // Push-to-talk: it's about asking hands-free.
  await setConfig(yap, main, { recordingMode: 'pushToTalk' });
  await expect(row).toContainText('Asking hands-free (a double-tap), a pause after your question sends it');
  await setConfig(yap, main, { recordingMode: 'toggle' });

  // Settings search finds it by its name, and by what people call it.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  const found = settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
  await search.fill('stop listening');
  await expect(found.getByRole('option').first()).toContainText('Stop listening when I pause');
  await search.fill('silence');
  await expect(found.getByRole('option').filter({ hasText: 'Stop listening when I pause' })).toHaveCount(1);
  await search.fill('');
  await closeSettings(main);
});

test('a pause after the question ends it, the bar showing it run out', async ({ yap, fakeLlm }) => {
  const before = asked(fakeLlm).length;
  // Gathering your thoughts (0.6 s), the question (1.2 s), then quiet.
  const asking = ask(yap, [quiet(0.6), tone(1.2), quiet(5)]);
  const cap = capsule(yap);
  await expect(cap).toHaveAttribute('data-ask', 'listening');
  // Half a second into the pause the capsule's hairline starts running down.
  const line = cap.locator('.pauseline');
  await expect(line).toHaveCount(1, { timeout: 5_000 });
  // (`shot` stops animations, which runs the line out: shot as it runs.)
  const file = path.join(SCREENSHOTS, 'assistant-pause', '03-pause-running-out.png');
  await yap.overlay.screenshot({ path: file, scale: 'css', animations: 'allow' });
  await test.info().attach('03-pause-running-out', { path: file, contentType: 'image/png' });
  const r = await asking;
  expect(r.endedByItself).toBe(true);
  // 1.8 s of audio, then the pause: give or take the 100 ms the audio is
  // played in, and the watch's look every 50 ms.
  expect(r.listenedMs).toBeGreaterThanOrEqual(1_800 + PAUSE - 100);
  expect(r.listenedMs).toBeLessThanOrEqual(1_800 + PAUSE + 500);
  await expect(line).toHaveCount(0);
  // The question went to Chat, as a press would have sent it.
  await expect.poll(() => asked(fakeLlm).length, { timeout: 10_000 }).toBe(before + 1);
});

test('never before you speak: gathering your thoughts keeps the mic', async ({ yap }) => {
  const r = await ask(yap, [quiet(5)]);
  expect(r).toEqual({ endedByItself: false, listenedMs: 5_000 });
});

test('a short thinking pause mid-question doesn\'t end it', async ({ yap }) => {
  // "What's on my calendar for…" (a second to think) "…Friday?"
  const r = await ask(yap, [quiet(0.6), tone(0.8), quiet(1), tone(0.8), quiet(5)]);
  expect(r.endedByItself).toBe(true);
  // Ended after the second half, not at the thinking pause.
  expect(r.listenedMs).toBeGreaterThanOrEqual(3_200 + PAUSE - 100);
  expect(r.listenedMs).toBeLessThanOrEqual(3_200 + PAUSE + 500);
});

test('in a noisy room the noise is learned, and the pause still ends it', async ({ yap }) => {
  // A fan at −38 dBFS: louder than speech's minimum, so a fixed level would
  // take it for talk and never stop.
  const r = await ask(yap, [quiet(1), tone(1.2, -18), quiet(5)], { noiseDb: -38 });
  expect(r.endedByItself).toBe(true);
  expect(r.listenedMs).toBeLessThanOrEqual(2_200 + PAUSE + 500);
});

test('push-to-talk: a held key keeps listening, a double-tap ends on the pause', async ({ yap, main }) => {
  await setConfig(yap, main, { recordingMode: 'pushToTalk' });
  try {
    // Held: letting go ends it, never the pause.
    const held = await ask(yap, [quiet(0.6), tone(1), quiet(3)], { press: 'hold' });
    expect(held).toEqual({ endedByItself: false, listenedMs: 4_600 });
    // Locked hands-free: the pause ends it.
    const free = await ask(yap, [quiet(0.6), tone(1), quiet(4)], { press: 'handsFree' });
    expect(free.endedByItself).toBe(true);
    expect(free.listenedMs).toBeLessThanOrEqual(1_600 + PAUSE + 500);
  } finally {
    await setConfig(yap, main, { recordingMode: 'toggle' });
  }
});

test('switched off, only the shortcut ends a question', async ({ yap, main }) => {
  await setConfig(yap, main, { assistantStopOnPause: false });
  try {
    const r = await ask(yap, [quiet(0.6), tone(1), quiet(3)]);
    expect(r).toEqual({ endedByItself: false, listenedMs: 4_600 });
  } finally {
    await setConfig(yap, main, { assistantStopOnPause: true });
  }
});
