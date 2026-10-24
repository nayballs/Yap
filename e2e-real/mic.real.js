// The dictation's microphone, opened on demand (dictation/mic.rs), on this PC's real
// microphone: closed while Yap idles, opened by a dictation, kept warm
// (YAP_MIC_WARM_MS, 3 s here instead of a minute), then closed — the
// stream's own state (`e2e_mic_status`), and Windows' record of who uses the
// mic (the consent store's LastUsedTimeStop for the robot's yap.exe) where
// this Windows build keeps one (2026-10-22 on the maintainer's PC: not
// updated since its last boot, so only the stream's state was checked). The
// dictation plays a WAV in the mic's place (`realtest_dictate_wav`), and the
// whole of it reaches the paste. Skipped without a microphone.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { speechWav } from './support/tts.js';

const WARM_MS = 3_000;

test.use({ robotOptions: { name: 'mic', env: { YAP_MIC_WARM_MS: String(WARM_MS) } } });

/**
 * Windows' record of the robot's yap.exe on the microphone (only its own
 * key, read): `{ start, stop }` as FILETIMEs (BigInt; `stop` 0 while it's on
 * the mic now), or null before it ever used one.
 */
function consent(exe) {
  const key =
    'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged\\' +
    exe.replace(/\\/g, '#');
  let out;
  try {
    out = execFileSync('reg', ['query', key], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  } catch {
    return null;
  }
  const read = (name) => {
    const m = new RegExp(`${name}\\s+REG_QWORD\\s+0x([0-9a-f]+)`, 'i').exec(out);
    return m ? BigInt(`0x${m[1]}`) : null;
  };
  return { start: read('LastUsedTimeStart'), stop: read('LastUsedTimeStop') };
}
const inUse = (c) => !!c && c.start > 0n && c.stop === 0n;

/** A 16-bit mono WAV's length in seconds. */
function wavSeconds(file) {
  const b = fs.readFileSync(file);
  const rate = b.readUInt32LE(24);
  const data = b.indexOf('data', 12);
  return b.readUInt32LE(data + 4) / 2 / rate;
}

test('a dictation opens the real microphone, keeps it warm, then closes it (as Windows records, where it does)', async ({ robot }) => {
  test.setTimeout(120_000);
  const { yap, pad } = robot;
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here');
  const exe = path.join(yap.appDir, 'yap.exe');

  // Idle: no stream, and Windows doesn't count Yap as using the mic.
  await yap.main.waitForTimeout(2_000);
  expect(await yap.invoke('e2e_mic_status')).toMatchObject({ open: false, opens: 0 });
  expect(inUse(consent(exe)), 'Yap on the mic while idle').toBe(false);

  await pad.set('box', '');
  const wav = speechWav('The microphone opens when you dictate, and closes after.');
  const r = await robot.dictateWav(pad, wav);
  expect(r).not.toBeNull();
  // All of the WAV reached the transcription (the stub says how much it got).
  const got = Number(/received ([\d.]+)s of audio/.exec(r.raw)?.[1]);
  expect(got).toBeGreaterThanOrEqual(Math.floor(wavSeconds(wav) * 10) / 10);
  await expect.poll(() => pad.text('box')).toContain('STT stub: received');

  // The real mic was opened for it, and is still warm.
  const status = await yap.invoke('e2e_mic_status');
  expect(status.opens).toBe(1);
  expect(status.open).toBe(true);
  expect(status.last.firstAudioMs).toBeLessThan(3_000);
  console.log(`mic: ${status.last.device}: open ${status.last.openMs} ms, first audio ${status.last.firstAudioMs} ms, live ${status.last.liveMs} ms after asking`);
  // Windows sees Yap on it — where it keeps the record (some Windows
  // builds stopped updating this store: then the stream's own state is
  // what's checked, and the test says so).
  const records = consent(exe) !== null;
  if (records) {
    expect(inUse(consent(exe)), 'Windows counts Yap on the mic while it is warm').toBe(true);
  } else {
    test.info().annotations.push({ type: 'note', description: "Windows didn't record the robot's mic use in its consent store on this PC" });
  }

  // The warm window runs out: closed, and Windows records Yap as done.
  await expect.poll(async () => (await yap.invoke('e2e_mic_status')).open, { timeout: WARM_MS + 10_000 }).toBe(false);
  expect((await yap.invoke('e2e_mic_status')).closes).toBe(1);
  if (records) {
    await expect.poll(() => inUse(consent(exe)), { timeout: 10_000 }).toBe(false);
    const after = consent(exe);
    expect(after.stop).toBeGreaterThan(after.start);
  }
});

test('a second dictation within the warm window reuses the open microphone', async ({ robot }) => {
  const { yap, pad } = robot;
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here');
  const before = await yap.invoke('e2e_mic_status');
  await pad.set('box', '');
  await robot.dictateWav(pad, speechWav('First one.'));
  await robot.dictateWav(pad, speechWav('And a quick second one.'));
  const after = await yap.invoke('e2e_mic_status');
  expect(after.opens).toBe(before.opens + 1);
  await expect.poll(() => pad.text('box')).toMatch(/STT stub.*STT stub/s);
});
