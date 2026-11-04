// Yap's sounds (dictation/sound.rs, dictation/sound/packs.rs): Settings →
// Dictation → Sounds — the pack picker and its ▶, the volume, a switch per
// moment — and the sound each moment plays. A test run plays nothing: what
// would play is kept (debug-only `sound_e2e_heard`: "minimal.start",
// "soft.locked"…). A dictation without a microphone: `e2e_dictate_audio`
// plays a tone in the mic's place between a real start and a real stop (or
// the bar's cancel); `e2e_dictation` a transcript from cleanup on, with
// `e2e_paste_fails` for a paste that can't land. Going hands-free needs the
// real hotkey's gestures and a microphone (skipped without one).
import {
  test,
  expect,
  openSettings,
  settingsDialog,
  closeSettings,
  closeToasts,
  expectStore,
  setConfig,
  inAnotherApp,
  pressHotkey,
} from './support/fixtures.js';

const MOMENTS = ['start', 'stop', 'locked', 'cancelled', 'error', 'slow', 'meetingStart', 'meetingStop', 'notice'];

const heard = (yap, clear = false) => yap.invoke('sound_e2e_heard', { clear });
/** The sounds moments played (not Settings' ▶), oldest first. */
const played = async (yap) => (await heard(yap)).filter((h) => !h.preview).map((h) => h.id);
const previewed = async (yap) => (await heard(yap)).filter((h) => h.preview).map((h) => h.id);
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const capsule = (yap) => yap.overlay.locator('.capsule');
const group = (main) => settingsDialog(main).getByRole('group', { name: 'Sounds', exact: true });

// Saved since sounds came in packs (configVersion 6), so its switches
// stay as given: every moment on, in Minimal.
test.use({
  yapOptions: {
    name: 'sounds',
    config: {
      configVersion: 6,
      soundEnabled: true,
      soundPack: 'minimal',
      audioFeedbackVolume: 0.8,
      soundError: true,
      soundSlow: true,
      soundMeeting: true,
      soundNotice: true,
    },
  },
});

test.beforeEach(async ({ yap }) => {
  await heard(yap, true);
});

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_paste_fails', { why: null }).catch(() => {});
  await yap.invoke('e2e_dictation_timing', { floorMs: 0, sttMs: 0, forget: true }).catch(() => {});
});

test('Settings: four packs, ▶ plays a pack’s whole set, a pick is saved, a moment’s ▶ plays its sound', async ({ yap, main, shot }) => {
  await openSettings(main, 'Dictation');
  const sounds = group(main);
  await sounds.scrollIntoViewIfNeeded();
  const packs = sounds.getByRole('radiogroup', { name: 'Sound pack' }).getByRole('radio');
  await expect(packs).toHaveText([/Classic/, /Soft/, /Crisp/, /Minimal/]);
  await expect(sounds.getByRole('radio', { name: /Minimal/ })).toHaveAttribute('aria-checked', 'true');
  await expect(sounds.getByRole('button', { name: 'Sounds', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '01-sounds');

  // ▶ beside Crisp: its whole set, in Settings' order, at the volume set.
  await sounds.getByRole('button', { name: 'Hear Crisp' }).click();
  await expect.poll(() => previewed(yap)).toEqual(MOMENTS.map((m) => `crisp.${m}`));
  expect((await heard(yap)).every((h) => Math.abs(h.volume - 0.8) < 1e-6)).toBe(true);
  // ■ while it plays.
  await expect(sounds.getByRole('button', { name: 'Stop Crisp' })).toBeVisible();
  await sounds.getByRole('button', { name: 'Stop Crisp' }).click();
  await expect(sounds.getByRole('button', { name: 'Hear Crisp' })).toBeVisible();
  expect(await played(yap)).toEqual([]);

  // Soft picked: saved.
  await sounds.getByRole('radio', { name: /Soft/ }).click();
  await expect(sounds.getByRole('radio', { name: /Soft/ })).toHaveAttribute('aria-checked', 'true');
  await expectStore(yap, 'config.json', (c) => c.soundPack === 'soft');

  // A moment's ▶: its sound in the pack picked; meeting notes' both.
  await heard(yap, true);
  await sounds.getByRole('button', { name: 'Hear Hands-free' }).click();
  await expect.poll(() => previewed(yap)).toEqual(['soft.locked']);
  await sounds.getByRole('button', { name: 'Hear Meeting notes start and stop' }).click();
  await expect.poll(() => previewed(yap)).toEqual(['soft.locked', 'soft.meetingStart', 'soft.meetingStop']);
  await closeSettings(main);
  await setConfig(yap, main, { soundPack: 'minimal' });
});

test('Settings: a switch per moment, saved, and none with Sounds off', async ({ yap, main, shot }) => {
  await openSettings(main, 'Dictation');
  const sounds = group(main);
  await sounds.scrollIntoViewIfNeeded();
  const notice = sounds.getByRole('button', { name: 'A notice on the Yap bar', exact: true });
  await expect(notice).toHaveAttribute('aria-pressed', 'true');
  await notice.click();
  await expect(notice).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.soundNotice === false && c.soundError === true);
  await notice.click();
  await expectStore(yap, 'config.json', (c) => c.soundNotice === true);

  // Sounds off: every switch greyed out (kept as they were), the packs
  // too; ▶ still plays, to listen before switching them on.
  await sounds.getByRole('button', { name: 'Sounds', exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.soundEnabled === false && c.soundNotice === true);
  await expect(notice).toBeDisabled();
  await expect(sounds.getByRole('radio', { name: /Soft/ })).toBeDisabled();
  await shot(main, '02-sounds-off');
  await sounds.getByRole('button', { name: 'Hear Dictation starts' }).click();
  await expect.poll(() => previewed(yap)).toEqual(['minimal.start']);
  // Nothing plays.
  await yap.invoke('e2e_dictate_audio', { seconds: 0.6 });
  expect(await played(yap)).toEqual([]);
  await sounds.getByRole('button', { name: 'Sounds', exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.soundEnabled === true);
  await closeSettings(main);
});

test('a dictation plays its start and stop sounds, a cancel its own, a paste that can’t land the error’s', async ({ yap, main }) => {
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await expect.poll(() => played(yap)).toEqual(['minimal.start', 'minimal.stop']);

  // Cancelled from the bar: the cancel's sound, never the stop's.
  await heard(yap, true);
  await yap.invoke('e2e_dictate_audio', { seconds: 1, cancel: true });
  await expect.poll(() => played(yap)).toEqual(['minimal.start', 'minimal.cancelled']);
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });

  // "Couldn't paste": the error's.
  await heard(yap, true);
  await yap.invoke('e2e_paste_fails', { why: 'keysBlocked' });
  const r = await yap.invoke('e2e_dictation', { text: 'send the report to sam' });
  expect(r.leftWhy).toBe('keysBlocked');
  await expect.poll(() => played(yap)).toEqual(['minimal.error']);
  await closeToasts(main);

  // Switched off: no error sound.
  await setConfig(yap, main, { soundError: false });
  await heard(yap, true);
  await yap.invoke('e2e_dictation', { text: 'and the projector' });
  await main.waitForTimeout(500);
  expect(await played(yap)).toEqual([]);
  await closeToasts(main);
  await setConfig(yap, main, { soundError: true });
});

test('“Taking longer than usual” has its sound, after the stop’s', async ({ yap }) => {
  // The stub transcribes in 4 s; slow after 1.5 s here.
  await yap.invoke('e2e_dictation_timing', { floorMs: 1500, sttMs: 4000, forget: true });
  const r = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(r.raw).toMatch(/STT stub/);
  await expect.poll(() => played(yap)).toEqual(['minimal.start', 'minimal.stop', 'minimal.slow']);
});

test('meeting notes: a sound as they start and as they stop, and while they record only a dictation’s start and stop', async ({ yap, main }) => {
  const note = await yap.invoke('note_create', { title: 'Weekly sync', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  try {
    await expect.poll(() => recording(yap)).toBe(true);
    await expect.poll(() => played(yap)).toEqual(['minimal.meetingStart']);

    // While it records: a card on the bar has no sound, a cancelled
    // dictation only its start, and ▶ is refused.
    await heard(yap, true);
    await inAnotherApp(yap);
    expect(await yap.invoke('e2e_notice', { surface: 'bar', kind: 'e2e-sound', title: 'A card mid-meeting', body: 'Quiet.' })).toBe(true);
    await yap.invoke('e2e_dictate_audio', { seconds: 1, cancel: true });
    await main.waitForTimeout(600);
    expect(await played(yap)).toEqual(['minimal.start']);
    await expect(yap.invoke('sound_preview', { pack: 'soft' })).rejects.toThrow(/while it records a meeting/);
    await yap.invoke('bar_card_action', { id: 'e2e-sound', action: '' });
  } finally {
    await heard(yap, true);
    if (await recording(yap)) {
      await yap.invoke('meeting_pause');
      await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    }
  }
  // Its stop's sound, once the recording let go of the audio.
  await expect.poll(() => played(yap)).toEqual(['minimal.meetingStop']);
  await yap.invoke('meeting_delete', { noteId: note.id }).catch(() => {});
});

test('a card on the Yap bar has its sound, once — not while a dictation records', async ({ yap, main }) => {
  await inAnotherApp(yap);
  // Quiet a moment first: a card right after another sound has none.
  await main.waitForTimeout(1_700);
  expect(await yap.invoke('e2e_notice', { surface: 'bar', kind: 'e2e-sound', title: 'Something to know', body: 'A notice.' })).toBe(true);
  await expect.poll(() => played(yap)).toEqual(['minimal.notice']);
  // The same card again (updated in place): no second sound.
  await yap.invoke('e2e_notice', { surface: 'bar', kind: 'e2e-sound', title: 'Something to know', body: 'A notice, again.' });
  await main.waitForTimeout(500);
  expect(await played(yap)).toEqual(['minimal.notice']);
  await yap.invoke('bar_card_action', { id: 'e2e-sound', action: '' });
});

test('going hands-free (push-to-talk’s double-tap) has its sound, after the start’s', async ({ yap, main }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here: the gestures need one (the e2e mic feed bypasses them)');
  await setConfig(yap, main, { recordingMode: 'pushToTalk' });
  try {
    await heard(yap, true);
    // A double-tap: hands-free (the capsule's lock says it took).
    await pressHotkey(main);
    await pressHotkey(main);
    await expect(capsule(yap).getByRole('img', { name: /Hands-free/ })).toBeVisible();
    await expect.poll(() => played(yap), { timeout: 8_000 }).toEqual(['minimal.start', 'minimal.locked']);
    await main.waitForTimeout(800);
    // Once more: it stops.
    await pressHotkey(main);
    await expect.poll(() => played(yap), { timeout: 8_000 }).toEqual(['minimal.start', 'minimal.locked', 'minimal.stop']);
    await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });
  } finally {
    await setConfig(yap, main, { recordingMode: 'toggle' });
  }
});
