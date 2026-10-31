// Yap's sounds for a config from before sounds came in packs
// (dictation/sound.rs; config.rs `migrate_once` v6): the suite's own
// configVersion 1, its chime on. It keeps exactly the sounds it had — the
// start and stop chime in Classic — and nothing more. The rest of the
// sounds' tests: sounds.spec.js.
import { test, expect, openSettings, settingsDialog, closeSettings } from './support/fixtures.js';

const heard = (yap, clear = false) => yap.invoke('sound_e2e_heard', { clear });
const played = async (yap) => (await heard(yap)).filter((h) => !h.preview).map((h) => h.id);
const group = (main) => settingsDialog(main).getByRole('group', { name: 'Sounds', exact: true });

test.use({ yapOptions: { name: 'sounds-existing', config: { soundEnabled: true } } });

test('keeps the start and stop chime in Classic, and no more', async ({ yap, main }) => {
  const cfg = await yap.invoke('get_config');
  expect(cfg.soundPack).toBe('classic');
  expect([cfg.soundStart, cfg.soundStop, cfg.soundLocked, cfg.soundCancelled, cfg.soundError, cfg.soundSlow, cfg.soundMeeting, cfg.soundNotice]).toEqual([
    true, true, false, false, false, false, false, false,
  ]);
  await heard(yap, true);
  await yap.invoke('e2e_dictate_audio', { seconds: 1, cancel: true });
  await main.waitForTimeout(600);
  expect(await played(yap)).toEqual(['classic.start']);
  await openSettings(main, 'Dictation');
  const sounds = group(main);
  await expect(sounds.getByRole('radio', { name: /Classic/ })).toHaveAttribute('aria-checked', 'true');
  await expect(sounds.getByRole('button', { name: 'Dictation starts', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(sounds.getByRole('button', { name: 'Hands-free', exact: true })).toHaveAttribute('aria-pressed', 'false');
  await closeSettings(main);
});
