// Push-to-talk's gestures (gesture.rs), its coaching hints (coaching.rs) and
// the dictation mic's watchdog (mic_watch.rs). The hotkey (F24) reaches the
// app through the main window's in-page fallback (test mode has no global
// hook), which takes the hook's own path into the pipeline: `dictation_key`.
// Recording needs a microphone, so those tests are skipped without one (CI);
// the state machines have unit tests. The watchdog's "muted mic" is the real
// mic with its audio zeroed (debug-only `e2e_dictation_mic`), so its stream
// really is reopened.
import fs from 'node:fs';
import {
  test,
  expect,
  openSettings,
  settingsDialog,
  closeSettings,
  closeToasts,
  expectStore,
  hotkeyDown,
  hotkeyUp,
  pressHotkey,
} from './support/fixtures.js';

test.use({ yapOptions: { name: 'push-to-talk', config: { recordingMode: 'pushToTalk' } } });

const dictations = (yap) => (yap.readJson('history.json') || []).length;
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const LONE_TAP = 'a lone tap, nothing to transcribe';

async function needsMic(yap) {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; no-mic.spec.js covers that path');
}

/** The dictation overlay (in the Yap bar's place while a dictation runs). */
const capsule = (yap) => yap.overlay.locator('.capsule');

/** Wait for a dictation to be over, so the next one isn't refused. */
async function idle(yap) {
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });
}

test.beforeEach(async ({ yap }) => {
  // Every hint due again, and the real mic's sound back.
  await yap.invoke('e2e_coaching_reset');
  await yap.invoke('e2e_dictation_mic', { silent: false });
});

test('Settings → General says how push-to-talk works, double-tap included', async ({ main, shot }) => {
  await openSettings(main, 'General');
  await expect(
    settingsDialog(main).getByText(
      'Hold F24 while you talk. Double-tap it to keep recording hands-free, then press it once more to stop.'
    )
  ).toBeVisible();
  await shot(main, '01-settings-recording-mode');
  await closeSettings(main);
});

test('a hold records while the key is down and transcribes when it comes up', async ({ yap, main, shot }) => {
  await needsMic(yap);
  const before = dictations(yap);
  await hotkeyDown(main);
  await expect(capsule(yap)).toBeVisible();
  await main.waitForTimeout(1_200);
  await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
  await expect(capsule(yap).getByRole('img', { name: /Hands-free/ })).toHaveCount(0);
  // Held: no ✕ / ✓ to point at (letting go is the stop).
  await expect(capsule(yap).getByRole('button')).toHaveCount(0);
  expect(await capsule(yap).getAttribute('data-region')).toBe(null);
  await shot(yap.overlay, '02-holding');
  await hotkeyUp(main);
  await expectStore(yap, 'history.json', (h) => h.length === before + 1 && JSON.stringify(h).includes('STT stub'));
  await idle(yap);
  await expect(main.getByRole('status').filter({ hasText: 'while you talk' })).toHaveCount(0);
});

test('a lone tap is thrown away, with a hint once', async ({ yap, main, shot }) => {
  await needsMic(yap);
  const before = dictations(yap);
  const lone = logCount(yap, LONE_TAP);
  // Down and straight back up: a tap. It records through the double-tap
  // window, then lets go of it.
  await pressHotkey(main);
  await expect.poll(() => logCount(yap, LONE_TAP), { timeout: 5_000 }).toBe(lone + 1);
  await idle(yap);
  const hint = main.getByRole('status').filter({ hasText: 'Hold F24 while you talk' });
  await expect(hint).toBeVisible();
  await expect(hint).toContainText('Tip');
  await expect(hint).toContainText('double-tap F24 to keep recording hands-free');
  await shot(main, '03-tap-hint');
  await expectStore(yap, 'hints.json', (h) => h.tap === 1 && h.lastShownAt > 0);
  expect(dictations(yap)).toBe(before);

  // Another tap straight away: thrown away too, but no hint this time (they
  // keep ten minutes apart).
  await closeToasts(main);
  await pressHotkey(main);
  await expect.poll(() => logCount(yap, LONE_TAP), { timeout: 5_000 }).toBe(lone + 2);
  await idle(yap);
  await main.waitForTimeout(500);
  await expect(main.getByRole('status')).toHaveCount(0);
  expect(yap.readJson('hints.json').tap).toBe(1);
  expect(dictations(yap)).toBe(before);
});

test('a double-tap records hands-free until the next press', async ({ yap, main, shot }) => {
  await needsMic(yap);
  const before = dictations(yap);
  const lone = logCount(yap, LONE_TAP);
  await pressHotkey(main);
  await pressHotkey(main);
  const lock = capsule(yap).getByRole('img', { name: /Hands-free/ });
  await expect(lock).toBeVisible();
  // Well past the double-tap window, the key up: still recording.
  await main.waitForTimeout(1_500);
  await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
  await expect(lock).toBeVisible();
  expect(logCount(yap, LONE_TAP)).toBe(lone);
  await shot(yap.overlay, '04-hands-free');

  // The next press stops it, and it's transcribed.
  await pressHotkey(main);
  await expectStore(yap, 'history.json', (h) => h.length === before + 1 && JSON.stringify(h).includes('STT stub'));
  await expectStore(yap, 'hints.json', (h) => h.handsFreeUsed === true);
  await idle(yap);

  // Double-tap learned: a lone tap is still thrown away, but earns no hint.
  await pressHotkey(main);
  await expect.poll(() => logCount(yap, LONE_TAP), { timeout: 5_000 }).toBe(lone + 1);
  await idle(yap);
  await main.waitForTimeout(500);
  await expect(main.getByRole('status').filter({ hasText: 'while you talk' })).toHaveCount(0);
});

// The capsule's ✕ / ✓ are off by default (bar.spec.js); switched on, they
// follow push-to-talk's gestures.
test('the capsule\'s ✕ / ✓: never while the key is held, even on "Always"; hands-free, as set', async ({
  yap,
  main,
  shot,
}) => {
  await needsMic(yap);
  const buttons = () => capsule(yap).getByRole('button');
  const done = capsule(yap).getByRole('button', { name: 'Stop and paste' });
  const setButtons = async (value) => {
    await yap.invoke('save_config', { cfg: { ...(await yap.invoke('get_config')), barDictationButtons: value } });
    await expect.poll(() => yap.invoke('bar_status').then((s) => s.dictationButtons)).toBe(value);
  };
  await setButtons('always');
  try {
    // Held: none, and nothing for the pointer (the capsule isn't a region).
    const before = dictations(yap);
    await hotkeyDown(main);
    await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
    await main.waitForTimeout(1_000);
    await expect(buttons()).toHaveCount(0);
    expect(await capsule(yap).getAttribute('data-region')).toBe(null);
    await shot(yap.overlay, '08-held-no-buttons-even-on-always');
    await hotkeyUp(main);
    await expectStore(yap, 'history.json', (h) => h.length === before + 1);
    await idle(yap);

    // A double-tap: hands-free, so "Always" shows them (with the lock).
    await pressHotkey(main);
    await pressHotkey(main);
    await expect(capsule(yap).getByRole('img', { name: /Hands-free/ })).toBeVisible();
    await expect(done).toBeVisible();
    await expect(capsule(yap).getByRole('button', { name: 'Cancel dictation' })).toBeVisible();
    await main.waitForTimeout(800);
    await shot(yap.overlay, '09-hands-free-buttons-always');
    // ✓ stops it, as the next press would.
    await yap.invoke('bar_simulate', { pointer: 'dictation' });
    await done.click();
    await expectStore(yap, 'history.json', (h) => h.length === before + 2);
    await idle(yap);

    // "When you point at it": hands-free shows them only then.
    await setButtons('hover');
    await yap.invoke('bar_simulate', { pointer: 'away' });
    await pressHotkey(main);
    await pressHotkey(main);
    await expect(capsule(yap).getByRole('img', { name: /Hands-free/ })).toBeVisible();
    await expect(done).toBeHidden();
    await expect.poll(() => capsule(yap).getAttribute('data-region')).toBe('dictation');
    await yap.invoke('bar_simulate', { pointer: 'dictation' });
    await expect(done).toBeVisible();
    await pressHotkey(main);
    await expectStore(yap, 'history.json', (h) => h.length === before + 3);
    await idle(yap);
  } finally {
    await yap.invoke('bar_simulate', { pointer: 'away' }).catch(() => {});
    await setButtons('off'); // the default
  }
});

test('with the main window away, the hint is a card on the Yap bar', async ({ yap, shot }) => {
  await needsMic(yap);
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
  try {
    // The hook's path, as from another app.
    await yap.invoke('dictation_key', { pressed: true });
    await yap.invoke('dictation_key', { pressed: false });
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Hold F24 while you talk' });
    await expect(card).toBeVisible({ timeout: 5_000 });
    await expect(card).toContainText('Tip'); // its chip
    await expect(card).toContainText('double-tap F24');
    await yap.overlay.waitForTimeout(300); // the card's entrance
    await shot(yap.overlay, '05-tap-hint-on-bar');
    await expectStore(yap, 'hints.json', (h) => h.tap === 1);
    // Its ✕ (as the bar reports a click on it).
    await yap.invoke('bar_card_action', { id: 'hint', action: '' });
    await expect(card).toHaveCount(0);
  } finally {
    await yap.invoke('open_settings'); // back as the suite started it
  }
});

test('a mic giving nothing but digital silence is reopened, then named; stopping says why', async ({
  yap,
  main,
  shot,
}) => {
  await needsMic(yap);
  const before = dictations(yap);
  const reopens = logCount(yap, 'reopening it');
  await yap.invoke('e2e_dictation_mic', { silent: true });
  try {
    // The bar's Dictate (in push-to-talk, a recording that runs until the
    // next press).
    await yap.invoke('toggle_recording');
    await expect(capsule(yap)).toBeVisible();
    // At ~3 s the stream is reopened, once; the recording goes on.
    await expect.poll(() => logCount(yap, 'reopening it'), { timeout: 6_000 }).toBe(reopens + 1);
    // At ~5 s it says so in the waveform's place — and only there (one
    // place: no toast in the main window as well).
    const warning = capsule(yap).getByRole('alert');
    await expect(warning).toHaveText(/^No sound from .+ — is it muted\?$/, { timeout: 5_000 });
    await expect(warning).toHaveAttribute('title', /^No sound from .+ — check it isn't muted or unplugged$/);
    await expect(main.getByRole('status').filter({ hasText: 'No sound from' })).toHaveCount(0);
    await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
    await shot(yap.overlay, '06-mic-silent');
    expect(logCount(yap, 'reopening it')).toBe(reopens + 1);

    // Stopped: nothing to transcribe; the overlay ends on the reason.
    await yap.invoke('toggle_recording');
    await expect(capsule(yap)).toContainText(/No sound from .+ — is it muted\?/);
    await expect(capsule(yap)).toHaveClass(/err/);
    await shot(yap.overlay, '07-mic-silent-stopped');
    await idle(yap);
    expect(dictations(yap)).toBe(before);
  } finally {
    await yap.invoke('e2e_dictation_mic', { silent: false });
  }
});

test('sound coming back after the warning takes it back, and the dictation goes through', async ({ yap }) => {
  await needsMic(yap);
  const before = dictations(yap);
  await yap.invoke('e2e_dictation_mic', { silent: true });
  try {
    await yap.invoke('toggle_recording');
    const warning = capsule(yap).getByRole('alert');
    await expect(warning).toBeVisible({ timeout: 8_000 });
    // "Unmuted": the real mic's sound again.
    await yap.invoke('e2e_dictation_mic', { silent: false });
    await expect(warning).toHaveCount(0, { timeout: 3_000 });
    await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
    await expect(capsule(yap)).not.toHaveClass(/err/);
    await yap.invoke('toggle_recording');
    await expectStore(yap, 'history.json', (h) => h.length === before + 1 && JSON.stringify(h).includes('STT stub'));
    await idle(yap);
  } finally {
    await yap.invoke('e2e_dictation_mic', { silent: false });
  }
});
