// My languages (dictation/languages.rs) with real speech and a real Whisper
// model: an `engines` build (`npm run test:real:build -- --engines`) and
// Whisper large-v3 (dictation/stt.rs "large"), copied read-only from your
// installed Yap's models folder (support/engines.js). Windows' text-to-speech
// only speaks English here, so the person's languages are set around it:
//
// - My languages = French only, the main shortcut on Auto: Whisper hears
//   English, sure of it (its own language detection, `transcribe_spoken`),
//   is told French (the only language picked) — large-v3 still writes clear
//   English as English — and Yap offers to add English: the nudge, once the
//   dictation is over;
// - English added: the same sentence comes out in English, and no nudge;
// - a language shortcut fixed to English, with French alone picked: English
//   again, and no detection asked.
//
// The timings Whisper's detection takes are in the log ("Language
// detected … ms"). Skipped without the build or the model.
import { test, expect } from './support/robot.js';
import { speechWav } from './support/tts.js';
import { ENGINES_EXE, WHISPER_ID, hasWhisper, linkWhisper } from './support/engines.js';
import fs from 'node:fs';

const why = !fs.existsSync(ENGINES_EXE)
  ? `No engines build at ${ENGINES_EXE} (npm run test:real:build -- --engines)`
  : !hasWhisper()
    ? 'No Whisper large-v3 in your Yap models folder'
    : null;
test.skip(!!why, why ?? '');

test.use({
  robotOptions: {
    name: 'languages',
    exe: ENGINES_EXE,
    prepare: linkWhisper,
    config: {
      modelSize: WHISPER_ID,
      streamingPartials: false,
      myLanguages: ['fr'],
      selectedLanguage: 'auto',
      languageNudge: true,
      languageShortcuts: [{ id: 'l-en', language: 'en', translateTo: '', hotkey: '' }],
    },
  },
});

const SENTENCE = 'The quick brown fox jumps over the lazy dog.';
const words = (s) => s.toLowerCase().match(/[a-zà-ÿ]+/g) ?? [];
const englishWords = (s) => {
  const said = words(SENTENCE);
  return words(s).filter((w) => said.includes(w)).length;
};

/** What the log says Whisper heard, since `mark`: `{ top, p, picked }`. */
function heardSince(robot, mark) {
  const m = /top=(\w+) p="?([\d.]+)"? picked=(\w+) Dictation: the language heard|Dictation: the language heard.*?top=(\w+).*?p="?([\d.]+)"?.*?picked=(\w+)/.exec(robot.logSince(mark));
  if (!m) return null;
  return m[1] ? { top: m[1], p: Number(m[2]), picked: m[3] } : { top: m[4], p: Number(m[5]), picked: m[6] };
}

test('Whisper hears English when only French is picked: told French, and Yap offers to add English', async ({ robot }) => {
  test.setTimeout(300_000);
  const { pad, yap } = robot;
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true });
  await pad.set('box', '');
  let mark = robot.logMark();
  const r = await robot.dictateWav(pad, speechWav(SENTENCE));
  expect(r, 'a transcript').not.toBeNull();
  const heard = heardSince(robot, mark);
  console.log(`French only: heard ${JSON.stringify(heard)}; wrote "${r.raw}"`);
  expect(heard?.top).toBe('en');
  expect(heard?.p).toBeGreaterThanOrEqual(0.85);
  expect(heard?.picked).toBe('fr');
  // Told French, large-v3 still writes clear English as English (measured
  // 2026-10-25): what it wrote is in the log above, not a check.
  await expect.poll(() => robot.logSince(mark), { timeout: 20_000 }).toMatch(/Language nudge: offered.*language=en|language=en.*Language nudge: offered/);

  // English added: the same sentence in English, nothing offered.
  await robot.configure({ myLanguages: ['fr', 'en'] });
  await pad.set('box', '');
  mark = robot.logMark();
  const again = await robot.dictateWav(pad, speechWav(SENTENCE));
  const heard2 = heardSince(robot, mark);
  console.log(`French and English: heard ${JSON.stringify(heard2)}; wrote "${again.raw}"`);
  expect(heard2?.picked).toBe('en');
  expect(englishWords(again.raw), `wrote "${again.raw}"`).toBeGreaterThanOrEqual(words(SENTENCE).length - 2);
  await expect.poll(() => pad.text('box')).toBe(again.text);
  expect(robot.logSince(mark)).not.toContain('Language nudge: offered');
  const detections = [...robot.logSince(0).matchAll(/ms=(\d+)[^\n]*Language detected|Language detected[^\n]*ms=(\d+)/g)].map((m) => Number(m[1] ?? m[2]));
  console.log(`Whisper's language detection: ${detections.join(', ')} ms`);
});

test('a language shortcut fixed to English: English, with French alone picked, and no detection asked', async ({ robot }) => {
  test.setTimeout(240_000);
  const { pad, yap } = robot;
  await robot.configure({ myLanguages: ['fr'] });
  await pad.set('box', '');
  await yap.invoke('e2e_language_stage', { shortcut: 'l-en', heard: null });
  const mark = robot.logMark();
  const r = await robot.dictateWav(pad, speechWav(SENTENCE));
  expect(r, 'a transcript').not.toBeNull();
  const log = robot.logSince(mark);
  expect(log).toContain('Dictation: started with a language shortcut');
  expect(log).not.toContain('Language detected');
  expect(englishWords(r.raw), `wrote "${r.raw}"`).toBeGreaterThanOrEqual(words(SENTENCE).length - 2);
  await expect.poll(() => pad.text('box')).toBe(r.text);
});
