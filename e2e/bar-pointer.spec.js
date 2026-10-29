// The Yap bar's first-use pointers (2026-10-29; bar/pointer.rs, the bar's
// page): for someone who has never dictated, "Click a text box, then press
// F24" over the idle pill ("…then hold F24 and talk" in push-to-talk), then
// "Talk, then press F24 again to paste" over the first dictation — each once,
// both gone after the first dictation, never a region (the bar stays
// click-through around it). A test run shows them only once a spec asks
// (`e2e_bar_pointer`), as for someone who has never dictated.
import { test, expect, expectStore, pressHotkey, setConfig } from './support/fixtures.js';

const pointer = (bar) => bar.getByTestId('bar-pointer');

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_bar_pointer', { on: false }).catch(() => {});
});

test('a first-use pointer over the idle pill, then over the first dictation, then never again', async ({ yap, main, shot }) => {
  const bar = yap.overlay;
  // A made-up microphone: its tone, never the PC's own.
  await yap.invoke('e2e_mics', { devices: [{ id: 'usb', name: 'Microphone (Fabrikam USB Mic)', default: true }] });
  // Reloaded, as Yap starts: nothing shown yet this run.
  await bar.reload();
  const due = await yap.invoke('e2e_bar_pointer', { on: true });
  expect(due).toMatchObject({ idle: true, recording: true, hold: false });
  await expect(pointer(bar)).toHaveText('Click a text box, then press F24');
  await expect(pointer(bar)).toHaveAttribute('data-kind', 'idle');
  // Words only: not a region, so the bar stays click-through around it.
  expect(await bar.locator('[data-testid="bar-pointer"][data-region]').count()).toBe(0);
  const regions = await bar.locator('[data-region]').evaluateAll((els) => els.map((e) => e.dataset.region));
  expect(regions).not.toContain('pointer');
  await shot(bar, '01-pointer-idle');
  // Counted once shown.
  await expectStore(yap, 'hints.json', (h) => !!h.pointers?.idle);

  // The first dictation: its own pointer while it records.
  await pressHotkey(main);
  await expect(pointer(bar)).toHaveAttribute('data-kind', 'recording');
  await expect(pointer(bar)).toHaveText('Talk, then press F24 again to paste');
  await shot(bar, '02-pointer-recording');
  await expectStore(yap, 'hints.json', (h) => !!h.pointers?.recording);
  await main.waitForTimeout(1_200);
  await pressHotkey(main);
  await expect(pointer(bar)).toBeHidden();
  // Dictated: neither again, here or after a restart.
  await expectStore(yap, 'hints.json', (h) => h.dictated === true);
  expect((await yap.invoke('bar_status')).firstUse).toBeNull();
  await bar.reload();
  await bar.waitForTimeout(800);
  await expect(pointer(bar)).toHaveCount(0);
  await yap.invoke('e2e_mics', { devices: null });
});

test('in push-to-talk it says hold, and a pointer already shown never shows again', async ({ yap, main }) => {
  const bar = yap.overlay;
  const before = (await yap.invoke('get_config')).recordingMode;
  await setConfig(yap, main, { recordingMode: 'pushToTalk' });
  try {
    await bar.reload();
    await yap.invoke('e2e_bar_pointer', { on: true });
    await expect(pointer(bar)).toHaveText('Click a text box, then hold F24 and talk');
    await expectStore(yap, 'hints.json', (h) => !!h.pointers?.idle);
    // Yap restarted: the idle one has shown, only the dictation's is due.
    await bar.reload();
    expect((await yap.invoke('bar_status')).firstUse).toMatchObject({ idle: false, recording: true, hold: true });
    await bar.waitForTimeout(800);
    await expect(pointer(bar)).toHaveCount(0);
  } finally {
    await setConfig(yap, main, { recordingMode: before });
  }
});

test('with tips off, no pointer', async ({ yap, main }) => {
  const bar = yap.overlay;
  await setConfig(yap, main, { showTips: false });
  try {
    await bar.reload();
    await yap.invoke('e2e_bar_pointer', { on: true });
    await bar.waitForTimeout(800);
    await expect(pointer(bar)).toHaveCount(0);
  } finally {
    await setConfig(yap, main, { showTips: true });
  }
});
