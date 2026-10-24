// "Stop listening when I pause" with real speech and real STT (dictation/pause.rs):
// an `engines` build (`npm run test:real:build -- --engines`) with Parakeet
// V3 copied read-only from your installed Yap (support/engines.js), the live
// preview ON, so the pause watch has both its signals — the mic's level and
// the preview's words. Windows' text-to-speech says the question into WAVs
// (never through the speakers), and the debug-only `e2e_assistant_audio`
// plays them in the mic's place with the assistant shortcut pressed through
// the real key path (toggle mode). The question then goes to a stand-in Chat
// model (../e2e/support/fake-llm.js), which shows what Yap heard. Nothing is
// pasted. Skipped without the build or the model.
import fs from 'node:fs';
import { test, expect } from './support/robot.js';
import { ENGINES_EXE, MODEL_ID, linkModel, sttUnavailable } from './support/engines.js';
import { speechWav } from './support/tts.js';
import { startFakeLlm } from '../e2e/support/fake-llm.js';

const why = sttUnavailable();
test.skip(!!why, why ?? '');

test.use({
  robotOptions: {
    name: 'assistant-pause',
    exe: ENGINES_EXE,
    prepare: linkModel,
    config: { modelSize: MODEL_ID, streamingPartials: true, recordingMode: 'toggle', assistantStopOnPause: true },
  },
});

/** The pause that ends a question (dictation/pause.rs `PAUSE_MS`). */
const PAUSE = 1_500;
const quiet = (secs) => ({ kind: 'quiet', secs });
const wav = (text) => ({ kind: 'wav', path: speechWav(text) });

/** A WAV's length in ms (its data chunk, 16-bit mono 16 kHz). */
function wavMs(file) {
  const buf = fs.readFileSync(file);
  const at = buf.indexOf('data', 12, 'latin1');
  return Math.round((buf.readUInt32LE(at + 4) / 2 / 16_000) * 1_000);
}

/** The parts' length in ms. */
const lengthMs = (parts) => parts.reduce((ms, p) => ms + (p.kind === 'wav' ? wavMs(p.path) : p.secs * 1_000), 0);

/** How long a WAV goes on after its last sound (over −50 dBFS), in ms:
 *  Windows' voice ends every sentence with ~0.86–0.91 s of silence. */
function tailMs(file) {
  const buf = fs.readFileSync(file);
  const at = buf.indexOf('data', 12, 'latin1');
  const n = buf.readUInt32LE(at + 4) / 2;
  let last = n;
  while (last > 0 && Math.abs(buf.readInt16LE(at + 8 + 2 * (last - 1))) <= 104) last--;
  return Math.round(((n - last) / 16_000) * 1_000);
}

/** When the parts' last word ends, in ms from the start: the pause is
 *  counted from there, not from the end of the last WAV's own silence. */
const lastWordMs = (parts) => {
  const last = parts.findLast((p) => p.kind === 'wav');
  return lengthMs(parts) - (last && parts.at(-1) === last ? tailMs(last.path) : 0);
};

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  await robot.configure({
    ppApiKeys: { groq: 'e2e-fake-key' },
    llmScopes: {
      chat: {
        enabled: true,
        provider: 'groq',
        baseUrl: llm.base,
        model: 'openai/gpt-oss-20b',
        apiKey: 'e2e-fake-key',
        apiKeys: {},
        prompt: '',
        disableThinking: false,
      },
    },
  });
});

test.afterAll(async ({ robot }) => {
  await robot.yap.invoke('assistant_stop').catch(() => {});
  await robot.yap.invoke('tts_stop').catch(() => {});
  await llm?.close();
});

/** What the stand-in Chat model was last asked. */
const lastQuestion = () => String(llm.requests.at(-1)?.user ?? '');

test('a spoken question with a thinking pause in it ends on its own after the last word', async ({ robot }) => {
  test.setTimeout(240_000);
  const before = llm.requests.length;
  // The thinking pause between the halves is the voice's own silence: the
  // end of the first WAV and the start of the second, ~1.0–1.2 s (tailMs).
  const asked = [quiet(0.8), wav("What's on my shopping list"), wav('for the weekend?')];
  const parts = [...asked, quiet(6)];
  const r = await robot.yap.invoke('e2e_assistant_audio', { parts });
  expect(r.endedByItself, 'the pause ended it').toBe(true);
  // After the last word and its pause — the preview's words can push it a
  // tick later, never earlier — and well before the quiet ran out.
  const spoken = lastWordMs(asked);
  expect(r.listenedMs).toBeGreaterThanOrEqual(spoken + PAUSE - 300);
  expect(r.listenedMs).toBeLessThanOrEqual(spoken + PAUSE + 1_500);
  // Both halves were asked: the thinking pause didn't cut it.
  await expect.poll(() => llm.requests.length, { timeout: 60_000 }).toBeGreaterThan(before);
  expect(lastQuestion()).toMatch(/shopping/i);
  expect(lastQuestion()).toMatch(/weekend/i);
  await robot.yap.invoke('tts_stop');
});

test('gathering your thoughts first keeps the mic; then the pause ends it', async ({ robot }) => {
  test.setTimeout(240_000);
  const before = llm.requests.length;
  const asked = [quiet(5), wav('What time is my next meeting?')];
  const r = await robot.yap.invoke('e2e_assistant_audio', { parts: [...asked, quiet(6)] });
  expect(r.endedByItself).toBe(true);
  expect(r.listenedMs).toBeGreaterThanOrEqual(lastWordMs(asked) + PAUSE - 300);
  await expect.poll(() => llm.requests.length, { timeout: 60_000 }).toBeGreaterThan(before);
  expect(lastQuestion()).toMatch(/meeting/i);
  await robot.yap.invoke('tts_stop');
});

test('over a noisy room, the question still ends on the pause', async ({ robot }) => {
  test.setTimeout(240_000);
  const before = llm.requests.length;
  // A fan at −40 dBFS under everything.
  const asked = [quiet(1.5), wav('Remind me what the plan for Friday is.')];
  const r = await robot.yap.invoke('e2e_assistant_audio', { parts: [...asked, quiet(6)], noiseDb: -40 });
  expect(r.endedByItself).toBe(true);
  expect(r.listenedMs).toBeLessThanOrEqual(lastWordMs(asked) + PAUSE + 1_500);
  await expect.poll(() => llm.requests.length, { timeout: 60_000 }).toBeGreaterThan(before);
  expect(lastQuestion()).toMatch(/friday/i);
  await robot.yap.invoke('tts_stop');
});
