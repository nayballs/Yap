// "Spell names the way they're written on screen" (screen_terms.rs) with
// real speech: an `engines` build (`npm run test:real:build -- --engines`)
// and Parakeet V3 — and Whisper large-v3 when it's installed —, copied
// read-only from your installed Yap's models folder (support/engines.js).
// Windows' text-to-speech says each sentence (names as a person says them,
// spelled out for its voice) into the Edge page that shows the names, once
// with the setting off (before) and once on (after). Skipped without the
// build or the model.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { OUT } from './support/targets.js';
import { SCREEN } from './support/screen.js';
import { speechWav } from './support/tts.js';
import { ENGINES_EXE, MODEL_ID, WHISPER_ID, hasWhisper, linkModel, linkWhisper, sttUnavailable } from './support/engines.js';

const why = sttUnavailable();
test.skip(!!why, why ?? '');

test.use({
  robotOptions: {
    name: 'screen-stt',
    exe: ENGINES_EXE,
    prepare: (dir) => {
      linkModel(dir);
      if (hasWhisper()) linkWhisper(dir);
    },
    config: { modelSize: MODEL_ID, streamingPartials: false },
    // How Whisper's prompt takes the names (screen_terms.rs): list (Yap's
    // default), sentence or off — to compare.
    env: process.env.YAP_REALTEST_SCREEN_WHISPER ? { YAP_REALTEST_SCREEN_WHISPER: process.env.YAP_REALTEST_SCREEN_WHISPER } : {},
  },
});

/** `YAP_SCREEN_STT_MODELS=large`: Whisper only (a comparison of its prompt). */
const ONLY = process.env.YAP_SCREEN_STT_MODELS;

/** Sentences as a person says them (sounds spelled out for the TTS voice), and the names on screen they hold. */
const SPOKEN = [
  { say: 'Please send the Zentrix launch notes to Rochelle.', names: ['Zentrix', 'Rochelle'] },
  { say: 'Shivawn Nee Breean will review the plan tomorrow.', names: ['Siobhan Ní Bhriain'] },
  { say: 'Can Kubernetes run G P T O S S on the cluster?', names: ['Kubernetes', 'GPT-OSS'] },
  { say: 'Zentrix should ask Shivawn about the pilot.', names: ['Zentrix', 'Siobhan'] },
];
/** Everyday sentences near the names' look-alikes: never changed by the screen. */
const EVERYDAY = ['We took the row shell out on the river.', 'I learned some zen tricks at the retreat.'];

/** Each sentence twice into the page: the setting off, then on. */
async function compare(robot, page, model) {
  const rows = [];
  for (const { say, names } of [...SPOKEN, ...EVERYDAY.map((s) => ({ say: s, names: [] }))]) {
    const wav = speechWav(say);
    const once = async (on) => {
      await robot.configure({ screenNames: on });
      await page.set('');
      const r = await robot.dictateWav(page, wav);
      expect(r, `a transcript of "${say}"`).not.toBeNull();
      await expect.poll(() => page.value()).toBe(r.text);
      return r;
    };
    const before = await once(false);
    const after = await once(true);
    const right = (text) => names.filter((n) => text.includes(n));
    rows.push({
      model,
      say,
      names,
      before: before.text,
      heard: after.raw,
      after: after.text,
      rightBefore: right(before.text),
      rightAfter: right(after.text),
    });
  }
  return rows;
}

test('Parakeet, then Whisper: names spelled as on screen, everyday sentences untouched', async ({ robot }, testInfo) => {
  test.setTimeout(900_000);
  const page = await robot.editor(SCREEN);
  const rows = ONLY === WHISPER_ID ? [] : await compare(robot, page, MODEL_ID);
  if (hasWhisper() && (!ONLY || ONLY === WHISPER_ID)) {
    await robot.yap.invoke('set_active_model', { modelSize: WHISPER_ID });
    rows.push(...(await compare(robot, page, WHISPER_ID)));
    await robot.yap.invoke('set_active_model', { modelSize: MODEL_ID });
  }
  const report = path.join(OUT, `screen-names${process.env.YAP_REALTEST_SCREEN_WHISPER ? `-${process.env.YAP_REALTEST_SCREEN_WHISPER}` : ''}.json`);
  fs.mkdirSync(path.dirname(report), { recursive: true });
  fs.writeFileSync(report, JSON.stringify(rows, null, 2));
  await testInfo.attach('before-after', { path: report, contentType: 'application/json' });
  for (const r of rows) {
    console.log(`[${r.model}] "${r.say}"\n   before: ${r.before}\n   heard:  ${r.heard}\n   after:  ${r.after}`);
  }
  const onScreen = /Zentrix|Rochelle|Siobhan|Bhriain|Kubernetes|GPT-OSS/;
  for (const r of rows) {
    // Never worse: every name right before is still right after.
    for (const n of r.rightBefore) expect(r.rightAfter, `${r.model}: "${r.say}" lost ${n}`).toContain(n);
    // Everyday sentences: nothing of the screen's in them, and not a word
    // changed from what the model heard. (Whisper hears the same WAV a
    // little differently from run to run — "row shell", "rail shell" —, so
    // it's checked against this run's own transcript, not the one before.)
    if (!r.names.length) {
      expect(r.after, `${r.model}: "${r.say}"`).not.toMatch(onScreen);
      expect(r.after, `${r.model}: "${r.say}"`).toBe(r.heard);
    }
  }
  // And it helps: more names right with the setting on than off.
  const count = (k) => rows.reduce((n, r) => n + r[k].length, 0);
  expect(count('rightAfter')).toBeGreaterThan(count('rightBefore'));
});
