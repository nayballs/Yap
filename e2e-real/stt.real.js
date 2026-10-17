// Real speech-to-text: an `engines` build (`npm run test:real:build --
// --engines`) with Yap's default model, Parakeet V3, copied read-only from
// your installed Yap's models folder (support/engines.js). Windows'
// text-to-speech says a sentence into a WAV, the robot plays it into the
// dictation, and what the model heard is pasted into the test pad.
// Skipped without the build or the model.
import { test, expect } from './support/robot.js';
import { ENGINES_EXE, MODEL_ID, linkModel, sttUnavailable } from './support/engines.js';
import { speechWav } from './support/tts.js';

const why = sttUnavailable();
test.skip(!!why, why ?? '');

test.use({
  robotOptions: {
    name: 'stt',
    exe: ENGINES_EXE,
    prepare: linkModel,
    // The live preview off: one transcription, so the timing is the model's.
    config: { modelSize: MODEL_ID, streamingPartials: false },
  },
});

const words = (s) => s.toLowerCase().match(/[a-z]+/g) ?? [];

test('Parakeet hears a spoken sentence, and it lands in the field', async ({ robot }) => {
  test.setTimeout(240_000);
  const { pad } = robot;
  await pad.set('box', '');
  const sentence = 'The quick brown fox jumps over the lazy dog.';
  const r = await robot.dictateWav(pad, speechWav(sentence));
  expect(r, 'a transcript').not.toBeNull();
  const heard = words(r.raw);
  const said = words(sentence);
  const found = said.filter((w) => heard.includes(w));
  // Synthetic speech, one model: most words, not necessarily all.
  expect(found.length, `heard "${r.raw}"`).toBeGreaterThanOrEqual(said.length - 2);
  await expect.poll(() => pad.text('box')).toBe(r.text);
});
